//! The `enhanced` preset's in-play conveniences: the game speed, the combat
//! tactical overlay, the dedicated quick-save slot and the HUD message that
//! reports them. Engine extension, not part of any specification. Under
//! Vanilla ([`MapPreview::qol`] is false) nothing here runs: world time is
//! the shell clock, no overlay or message draws, and F6/F7 keep SPEC-014
//! §3.16's pick screen.
//!
//! - **Game speed.** The world sees its own clock, which advances by the
//!   shell clock's frame delta times the speed (1.0x, 1.5x or 2.0x),
//!   counted in exact half milliseconds. Everything the world times runs
//!   on it: animation and movement sequences, attack shows, combat turns,
//!   scripts, the game clock and floating texts. The interface keeps the
//!   shell clock (holds, the action menu, fades, counters, the camera),
//!   and so do movies, audio and world-map travel. While a conversation is
//!   open world time runs at 1.0x. At 1.0x world time is the shell clock
//!   exactly, so the specification's timings are untouched.
//! - **Tactical overlay** (Alt+H, on by default). In combat on the map:
//!   a compact bar above each injured critter (current HP below its
//!   maximum), and the SPEC-007 to-hit chance beside the crosshair while it
//!   is over a target, at once rather than after the hover's rest.
//! - **Quick save and load.** F6 saves to and F7 loads from
//!   [`QUICK_SLOT`] at once, with no picker and no prompt.
use super::{MapPreview, Screen, pointer::Feedback};
use crate::{enhanced::GameSpeed, interface::Painter, storage::Namespace};
#[cfg(test)]
use vault_core::host_save::quick_slot_directory;
use vault_core::{
    EnginePreset, ObjectRef,
    host_save::{QUICK_SLOT, SaveInfo},
    ui::{ColorKey, cursor},
};
use vault_formats::{
    Fid, Palette,
    pro::{BasicFlags, Category},
};
use vault_render::{IndexedImage, map::MapView};

/// How long a HUD message shows in full, then how long it dissolves
/// (shell milliseconds).
pub(crate) const MESSAGE_MS: u64 = 2000;
pub(crate) const MESSAGE_FADE_MS: u64 = 500;
/// The message plate's top edge, centered across the 640-pixel frame: below
/// the touch UI's pill bar (y 12-52) and clear of the party pills' column.
const MESSAGE_TOP: i32 = 62;
/// The HP bar at scale 1: its outer size and its gap above the sprite.
const BAR: [i32; 2] = [26, 4];
const BAR_GAP: i32 = 2;
/// The first palette-cycling index (SPEC-001 §3.2): engine-drawn colors
/// stay below it, so they never animate.
const CYCLING: u8 = 229;
/// The 4x4 ordered (Bayer) matrix the message dissolves through.
const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

pub(crate) const QUICK_SAVED: &[u8] = b"Quick Saved.";
pub(crate) const QUICK_LOADED: &[u8] = b"Quick Loaded.";
pub(crate) const QUICK_SAVE_FAILED: &[u8] = b"Quick save failed.";
pub(crate) const QUICK_LOAD_FAILED: &[u8] = b"Quick load failed.";
pub(crate) const NO_QUICK_SAVE: &[u8] = b"No quick save to load.";

/// A brief HUD message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Message {
    pub text: Vec<u8>,
    /// A failure, drawn in red.
    pub error: bool,
    /// The shell clock when it was posted.
    since: u64,
    /// The dissolve step last drawn (0 whole, 16 gone).
    step: u8,
}

/// The conveniences' state.
#[derive(Debug)]
pub(crate) struct Qol {
    pub speed: GameSpeed,
    /// World time minus the shell clock, in whole milliseconds, and the
    /// half millisecond carried.
    ahead: u64,
    half: u64,
    /// The shell clock at the last tick.
    last: Option<u64>,
    /// The tactical overlay is on (Alt+H).
    pub overlay: bool,
    pub message: Option<Message>,
}

impl Default for Qol {
    fn default() -> Self {
        Self {
            speed: GameSpeed::Normal,
            ahead: 0,
            half: 0,
            last: None,
            overlay: true,
            message: None,
        }
    }
}

/// The palette index nearest `rgb` (8-bit channels) below the cycling
/// range, index 0 excluded; the lowest index wins a tie.
fn nearest(palette: &Palette, rgb: [u8; 3]) -> u8 {
    let target = rgb.map(|channel| i32::from(channel >> 2));
    let distance = |index: u8| {
        let color = palette.rgb6(index).map(i32::from);
        (0..3)
            .map(|axis| (color[axis] - target[axis]).pow(2))
            .sum::<i32>()
    };
    let mapped = (1..CYCLING).filter(|index| palette.is_mapped(*index));
    mapped
        .min_by_key(|index| (distance(*index), *index))
        .unwrap_or_else(|| {
            (1..CYCLING)
                .min_by_key(|index| (distance(*index), *index))
                .unwrap_or(1)
        })
}

/// The HP bar's fill color for `hp` of `max`: green above half, amber
/// above a quarter, else red.
fn bar_color(hp: i32, max: i32) -> [u8; 3] {
    if hp * 2 > max {
        [0, 200, 0]
    } else if hp * 4 > max {
        [232, 176, 0]
    } else {
        [216, 0, 0]
    }
}

/// The overlay's scale on the world canvas drawn at `zoom`: whole canvas
/// pixels per overlay pixel, so it never shrinks below its own size on
/// screen when the view is zoomed out (1 at zoom 0.75 and above).
pub(crate) fn overlay_scale(zoom: f32) -> i32 {
    if zoom >= 0.75 {
        1
    } else {
        (1.0 / zoom.max(0.1)).round().max(1.0) as i32
    }
}

impl MapPreview {
    /// The `enhanced` preset's conveniences are on.
    pub(crate) fn qol(&self) -> bool {
        self.host.preset() == EnginePreset::Enhanced
    }

    // Game speed.

    /// The game speed (1.0x outside the enhanced preset).
    #[cfg(test)]
    pub(crate) fn game_speed(&self) -> GameSpeed {
        if self.qol() {
            self.qol_state.speed
        } else {
            GameSpeed::Normal
        }
    }
    /// A new world starts at the session's speed, silently.
    pub(crate) fn init_game_speed(&mut self, speed: GameSpeed) {
        if self.qol() {
            self.qol_state.speed = speed;
        }
    }
    /// The session's speed changed (Alt+F, or its logged record): it
    /// applies from the next frame on, and the HUD names it. Ignored under
    /// Vanilla.
    pub fn set_game_speed(&mut self, speed: GameSpeed) {
        if !self.qol() || self.qol_state.speed == speed {
            return;
        }
        self.qol_state.speed = speed;
        self.post_message(format!("Game speed {}", speed.label()).into_bytes(), false);
    }
    /// World time at shell time `now`: the shell clock plus what the speed
    /// has run ahead of it. At 1.0x (always under Vanilla, and while a
    /// conversation is open) a frame adds nothing ahead, so world time is
    /// the shell clock itself.
    pub(crate) fn world_time(&mut self, now: u64) -> u64 {
        let halves = if self.qol() && !self.host.world.dialogue.open {
            self.qol_state.speed.halves()
        } else {
            2
        };
        let state = &mut self.qol_state;
        if let Some(last) = state.last {
            let extra = now.saturating_sub(last) * (halves - 2) + state.half;
            state.ahead += extra / 2;
            state.half = extra % 2;
        }
        state.last = Some(now);
        now.saturating_add(state.ahead)
    }

    // The HUD message.

    /// Show `text` on the HUD for [`MESSAGE_MS`], then dissolve it.
    pub(crate) fn post_message(&mut self, text: Vec<u8>, error: bool) {
        self.qol_state.message = Some(Message {
            text,
            error,
            since: self.clock,
            step: 0,
        });
        self.pointer.redraw = true;
    }
    /// The message's dissolve step at `now`: 0 shown whole, 16 gone.
    fn message_step(since: u64, now: u64) -> u8 {
        let fading = now.saturating_sub(since).saturating_sub(MESSAGE_MS);
        (fading * 16 / MESSAGE_FADE_MS).min(16) as u8
    }
    /// One frame of the message: it dissolves, then goes. Returns whether
    /// what it draws changed.
    pub(crate) fn qol_tick(&mut self, now: u64) -> bool {
        let Some(message) = &mut self.qol_state.message else {
            return false;
        };
        let step = Self::message_step(message.since, now);
        if step >= 16 {
            self.qol_state.message = None;
            return true;
        }
        std::mem::replace(&mut message.step, step) != step
    }
    /// The message, centered near the top of the interface frame: an amber
    /// plate with its text amber on black (red for a failure), dissolving
    /// through a 4x4 ordered dither in its last [`MESSAGE_FADE_MS`].
    pub(crate) fn draw_message(&self, painter: &Painter, image: &mut IndexedImage) {
        let Some(message) = &self.qol_state.message else {
            return;
        };
        let step = Self::message_step(message.since, self.clock);
        if step >= 16 {
            return;
        }
        let width = painter.width(101, &message.text) + 16;
        let [x, y, w, h] = [320 - width / 2, MESSAGE_TOP, width, 16];
        let stride = image.width() as usize;
        let rows = y.max(0)..(y + h).min(image.height() as i32);
        let columns = x.max(0)..(x + w).min(image.width() as i32);
        // What the plate covers, kept for the dissolve.
        let before: Vec<u8> = rows
            .clone()
            .flat_map(|row| {
                let start = row as usize * stride;
                image.pixels()[start + columns.start as usize..start + columns.end as usize]
                    .to_vec()
            })
            .collect();
        let amber = painter.color(ColorKey::AMBER);
        let black = painter.color(ColorKey::BLACK);
        image.fill_rect([x + 1, y, w - 2, h], amber);
        image.fill_rect([x, y + 1, w, h - 2], amber);
        image.fill_rect([x + 1, y + 1, w - 2, h - 2], black);
        let ink = if message.error {
            ColorKey::RED
        } else {
            ColorKey::AMBER
        };
        painter.centered(image, 101, ink, &message.text, [x, y + 4], w);
        if step == 0 {
            return;
        }
        let span = columns.len();
        let pixels = image.pixels_mut();
        for (k, row) in rows.enumerate() {
            for (j, column) in columns.clone().enumerate() {
                if BAYER[row as usize % 4][column as usize % 4] < step {
                    pixels[row as usize * stride + column as usize] = before[k * span + j];
                }
            }
        }
    }

    // The tactical overlay.

    /// Alt+H: the overlay on or off (presentation only), named on the HUD.
    pub(crate) fn toggle_tactical(&mut self) {
        let on = !self.qol_state.overlay;
        self.qol_state.overlay = on;
        let text = if on {
            "Tactical overlay on"
        } else {
            "Tactical overlay off"
        };
        self.post_message(text.as_bytes().to_vec(), false);
        self.thaw_view();
    }
    /// Whether the overlay draws: the enhanced preset with it on, in
    /// combat, on the map outside a conversation.
    pub(crate) fn tactical_shown(&self) -> bool {
        self.qol()
            && self.qol_state.overlay
            && self.host.world.combat
            && matches!(self.screen, Screen::Map)
            && !self.host.world.dialogue.open
    }
    /// The injured critters the overlay marks on `view`: each with its
    /// sprite's top center and its current and maximum HP. A critter is
    /// injured when its current HP is below its maximum; the dead, the
    /// hidden and those on other elevations have none.
    pub(crate) fn injured_critters(&self, view: MapView) -> Vec<(ObjectRef, [i64; 2], [i32; 2])> {
        let world = &self.host.world;
        world
            .iter()
            .filter_map(|(critter, object)| {
                if object.category() != Category::Critter
                    || object.owner.is_some()
                    || object.raw.elevation != view.elevation
                    || object.raw.hex.is_none()
                    || object.raw.flags.contains(BasicFlags::HIDDEN)
                    || world.is_dead(critter)
                {
                    return None;
                }
                let hp = world.hit_points(critter)?;
                let max = object.character.as_ref()?.stat(7, world.context(critter));
                if max <= 0 || hp >= max {
                    return None;
                }
                let (frame, [x, y]) =
                    vault_render::map::object_sprite(view, &self.art, &object.raw)?;
                Some((critter, [x + i64::from(frame.width / 2), y], [hp, max]))
            })
            .collect()
    }
    /// The to-hit chance the overlay shows beside the crosshair: the
    /// target under the pointer as the crosshair hover picks it, and the
    /// same SPEC-007 chance its text shows. `None` when there is no target
    /// (the outer `Option`), or the attack is refused (the inner one).
    pub(crate) fn tactical_hit(&self) -> Option<(ObjectRef, Option<i32>)> {
        if self.hud.mode != cursor::Mode::Crosshair {
            return None;
        }
        let at = self.world_cursor_at()?;
        let player = self.host.world.player;
        let target = self.object_at(at).filter(|object| {
            *object != player
                && self
                    .host
                    .world
                    .get(*object)
                    .is_some_and(|o| Fid(o.raw.art_id).category() == 1)
        })?;
        Some((target, self.hover_hit_chance(target)))
    }
    /// Draw the overlay on a map image showing `view` (the 640-by-380 map
    /// view, or the `enhanced` world canvas at `scale` canvas pixels per
    /// overlay pixel).
    pub(crate) fn draw_tactical(&self, image: &mut IndexedImage, view: MapView, scale: i32) {
        if !self.tactical_shown() {
            return;
        }
        let palette = self.game_palette();
        let frame = nearest(palette, [0, 0, 0]);
        let empty = nearest(palette, [56, 52, 48]);
        let [bw, bh] = BAR.map(|value| value * scale);
        for (_, [cx, top], [hp, max]) in self.injured_critters(view) {
            let x = (cx - i64::from(bw / 2)).clamp(-1 << 30, 1 << 30) as i32;
            let y = (top - i64::from(bh + BAR_GAP * scale)).clamp(-1 << 30, 1 << 30) as i32;
            image.fill_rect([x, y, bw, bh], frame);
            let inner = [x + scale, y + scale, bw - 2 * scale, bh - 2 * scale];
            image.fill_rect(inner, empty);
            let filled = inner[2] * hp.max(0) / max;
            let fill = nearest(palette, bar_color(hp, max));
            image.fill_rect([inner[0], inner[1], filled.max(scale), inner[3]], fill);
        }
        // The to-hit chance: shown at once, until the hover's own text
        // (the same number) replaces it after the rest.
        if matches!(self.pointer.feedback, Some(Feedback::Hit(..))) {
            return;
        }
        let Some((_, chance)) = self.tactical_hit() else {
            return;
        };
        let Some(at) = self.world_cursor_at() else {
            return;
        };
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        let text = cursor::hit_text(chance);
        let key = if chance.is_some() {
            ColorKey::WHITE
        } else {
            ColorKey::RED
        };
        let width = painter.width(101, &text) + 2;
        let height = painter.line_height(101).max(1) + 2;
        let Ok(mut stamp) = IndexedImage::new(width.max(1) as u32, height as u32) else {
            return;
        };
        // A sentinel the text never draws marks the stamp's clear pixels.
        let [white, black, red] =
            [ColorKey::WHITE, ColorKey::BLACK, ColorKey::RED].map(|key| painter.color(key));
        let clear = (1..=255)
            .find(|index| ![white, black, red].contains(index))
            .unwrap_or(0);
        stamp.fill_rect([0, 0, width, height], clear);
        for dy in -1..=1 {
            for dx in -1..=1 {
                if (dx, dy) != (0, 0) {
                    painter.text(
                        &mut stamp,
                        101,
                        ColorKey::BLACK,
                        &text,
                        [1 + dx, 1 + dy],
                        width,
                    );
                }
            }
        }
        painter.text(&mut stamp, 101, key, &text, [1, 1], width);
        let origin = [at[0] + 12, at[1] - 4 * scale];
        stamp_scaled(image, &stamp, clear, origin, scale);
    }

    // Quick save and quick load.

    /// Enhanced F6: the game saved to the quick-save slot at once.
    pub(crate) fn enhanced_quick_save(&mut self) {
        let info = SaveInfo {
            description: b"Quick save".to_vec(),
            host_date: [0; 3],
            host_time: 0,
            thumbnail: Some(self.capture_thumbnail()),
        };
        match self.write_game_slot(QUICK_SLOT, &info) {
            Ok(()) => self.post_message(QUICK_SAVED.to_vec(), false),
            Err(error) => {
                tracing::warn!(%error, slot = QUICK_SLOT, "quick save failed");
                self.status = error.to_string().into_bytes();
                self.post_message(QUICK_SAVE_FAILED.to_vec(), true);
            }
        }
    }
    /// Enhanced F7: the quick-save slot loaded at once.
    pub(crate) fn enhanced_quick_load(&mut self) {
        let keys = self
            .slot_storage()
            .list(Namespace::Savegame, Some(QUICK_SLOT));
        if keys.is_ok_and(|keys| keys.is_empty()) {
            self.post_message(NO_QUICK_SAVE.to_vec(), true);
            return;
        }
        match self.read_game_slot(QUICK_SLOT) {
            Ok(()) => {
                self.return_to_map();
                self.settle_panel();
                self.post_message(QUICK_LOADED.to_vec(), false);
            }
            Err(error) => {
                tracing::warn!(%error, slot = QUICK_SLOT, "quick load failed");
                self.status = error.to_string().into_bytes();
                self.post_message(QUICK_LOAD_FAILED.to_vec(), true);
            }
        }
    }
}

/// Copy `stamp` onto `image` with its corner at `origin`, each pixel a
/// `scale`-by-`scale` block, skipping the `clear` pixels; clipped.
fn stamp_scaled(
    image: &mut IndexedImage,
    stamp: &IndexedImage,
    clear: u8,
    origin: [i32; 2],
    scale: i32,
) {
    let width = stamp.width() as usize;
    for (index, pixel) in stamp.pixels().iter().enumerate() {
        if *pixel == clear {
            continue;
        }
        let [x, y] = [(index % width) as i32, (index / width) as i32];
        image.fill_rect(
            [origin[0] + x * scale, origin[1] + y * scale, scale, scale],
            *pixel,
        );
    }
}

#[cfg(test)]
#[path = "qol_tests.rs"]
mod tests;
