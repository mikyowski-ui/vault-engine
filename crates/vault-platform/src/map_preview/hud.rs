//! SPEC-014 §3.19-3.27 in the interactive world: the interface bar under
//! the 640-by-380 map view (message monitor, HP/AC counters, AP pips, the
//! active item slot, buttons and the combat panel), the indicator boxes,
//! the in-world cursor modes and their clicks, and the game-loop hotkeys.
use super::MapPreview;
use crate::{input::key_text, interface::Painter};
use std::collections::VecDeque;
use vault_core::{
    ObjectAction, ObjectRef, WorldEffect,
    assets::WorldAssets,
    clicks::{self, Click, Policy},
    combat::attack_mode,
    critical::{hit_mode, location},
    progression, skills,
    ui::{
        Rect,
        bar::{self, Action, BarButton, Key, WeaponMode},
        cursor::{self, Mode},
    },
};
use vault_formats::{
    Fid,
    pro::{Category, ExtendedFlags, ItemData, ItemSubtype, PrototypeData},
};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// §3.21 a counter's running drum roll: its frames with their colors, the
/// frame shown, the wait after each frame, and when the next one is due.
pub(crate) struct Roll {
    frames: Vec<([bar::Glyph; 4], bar::Band)>,
    index: usize,
    delay: u64,
    next: u64,
}
/// §3.21 one counter: the remembered value (none before the first draw,
/// which is unanimated), whether its next change rolls, and the roll.
#[derive(Default)]
pub(crate) struct Counter {
    shown: Option<i32>,
    band: Option<bar::Band>,
    pub animate: bool,
    pub roll: Option<Roll>,
}
impl Counter {
    /// Advance to `now` toward `value`; returns whether the drawing changed.
    fn tick(
        &mut self,
        now: u64,
        value: i32,
        segments: impl Fn(i32, bar::Band) -> Vec<(i32, i32, bar::Band)>,
    ) -> bool {
        if let Some(roll) = &mut self.roll {
            let mut changed = false;
            while now >= roll.next {
                if roll.index + 1 >= roll.frames.len() {
                    // The roll ends on its last frame's wait.
                    let band = roll.frames.last().map(|(_, band)| *band);
                    self.roll = None;
                    self.band = band;
                    return true;
                }
                roll.index += 1;
                roll.next += roll.delay;
                changed = true;
            }
            return changed;
        }
        if self.shown == Some(value) {
            self.animate = false;
            return false;
        }
        let animate = std::mem::take(&mut self.animate);
        let previous = self.shown.replace(value);
        if previous.is_none() {
            // The first draw: what is drawn does not change.
            return false;
        }
        match previous {
            Some(old) if animate => {
                let first = self.band.unwrap_or(bar::Band::White);
                let frames = bar::roll(old, &segments(old, first));
                let delay = bar::roll_delay(old, value);
                self.roll = Some(Roll {
                    frames,
                    index: 0,
                    delay,
                    next: now + delay,
                });
            }
            _ => self.band = None,
        }
        true
    }
    /// The glyphs and color drawn now.
    fn glyphs(&self, value: i32, band: bar::Band) -> ([bar::Glyph; 4], bar::Band) {
        match &self.roll {
            Some(roll) => roll.frames[roll.index],
            None => (bar::counter_glyphs(self.shown.unwrap_or(value)), band),
        }
    }
}

/// §3.25 the combat panel: whether it is open (its End turn and End
/// combat buttons exist once the open animation has played), the running
/// open or close animation (opening, start), the light (green on the
/// player's turn), and the last `endanim.frm` frame drawn (none before the
/// first open).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Panel {
    pub open: bool,
    pub animation: Option<(bool, u64)>,
    pub light: Option<bool>,
    pub frame: Option<u8>,
}
impl Panel {
    /// The state restored by a load, without animation or sound (§3.19:
    /// "on load the panel state is restored without animation").
    pub fn settled(combat: bool, your_turn: bool) -> Self {
        Self {
            open: combat,
            animation: None,
            light: combat.then_some(your_turn),
            frame: Some(if combat { 4 } else { 0 }),
        }
    }
    /// The End turn and End combat buttons exist.
    pub fn buttons(&self) -> bool {
        self.open && self.animation.is_none()
    }
}

/// Interface state that lives with the map view.
pub(crate) struct Hud {
    pub monitor: bar::Monitor,
    pub mode: Mode,
    /// Held modifiers: shift, control, alt.
    pub modifiers: [bool; 3],
    /// The player has had a turn in this combat: until then the pips keep
    /// their empty state (§3.22).
    turn_seen: bool,
    /// §3.21 the HP and AC counters.
    pub hp: Counter,
    pub ac: Counter,
    /// The bar button pressed and still held: it reports on the release.
    pub held: Option<BarButton>,
    /// §3.23 the left and right hands' slot actions.
    pub slots: [bar::HandSlot; 2],
    /// §3.25 the combat panel.
    pub panel: Panel,
    /// SPEC-013 §3.15 the Set Timer window, over whichever window opened it.
    pub timer: Option<(ObjectRef, vault_core::skills::SetTimer)>,
    /// §3.28 a frame pixel the system pointer is to be moved to (the
    /// action menu's release); the shell takes it.
    pub warp: Option<[i32; 2]>,
    /// Sounds due later: a knob or switch's release cue 70 ms after its
    /// press cue (§3.18).
    pub cues: Vec<(u64, &'static [u8])>,
    /// §3.23 a swap-hands animation was registered and has not ended: the
    /// bar and the mouse are disabled and the cursor is `wait2.frm`.
    pub swapping: bool,
    /// §3.26 the animated watch: when it was set and the frame last drawn.
    pub watch: Option<(u64, u32)>,
    /// §3.27 key presses and releases typed while combat locks the
    /// keyboard, read in order once it is enabled again.
    pub held_keys: VecDeque<HeldKey>,
    /// The player turns started (`CombatLock::turns`) when the held keys
    /// were last flushed.
    pub key_turns: u64,
    /// §3.38 step 3: an out-of-combat crosshair click that started combat,
    /// replayed once on the player's first turn.
    pub replay: Option<ObjectRef>,
    /// §3.38 the called-shot window.
    pub called: Option<super::called::CalledShot>,
}

/// §3.27 a key event held while combat locks the keyboard: a press, with
/// the modifiers it was typed with, or a release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeldKey {
    Press(KeyCode, [bool; 3]),
    Release,
}
/// §3.27 at most 32 key presses and releases are held. Which survive when
/// more are typed is `[May-improve]`; the safe rule keeps the first 32.
pub(crate) const HELD_KEYS: usize = 32;
impl Default for Hud {
    fn default() -> Self {
        Self {
            monitor: bar::Monitor::default(),
            mode: Mode::Movement,
            modifiers: [false; 3],
            turn_seen: false,
            hp: Counter::default(),
            ac: Counter::default(),
            held: None,
            slots: Default::default(),
            panel: Panel::default(),
            timer: None,
            warp: None,
            cues: Vec::new(),
            swapping: false,
            watch: None,
            held_keys: VecDeque::new(),
            key_turns: 0,
            replay: None,
            called: None,
        }
    }
}

/// §3.26 `wait2.frm`: 6 frames at 10 fps.
pub(crate) const WATCH_FRAMES: u32 = cursor::ANIMATED[1].1;

/// A cap on the slot's reload loop (see `reload_from_slot`).
const RELOAD_LIMIT: usize = 1000;

/// A bar button's screen rectangle.
fn bar_button_rect(button: BarButton) -> Rect {
    let [bx, by] = bar::BAR_ORIGIN;
    let (_, rect, ..) = bar::BAR_BUTTONS
        .iter()
        .find(|(found, ..)| *found == button)
        .expect("a bar button");
    Rect::new(bx + rect.x, by + rect.y, rect.w, rect.h)
}

/// A message with its first `%d` replaced by `value`.
fn replace_number(text: &[u8], value: i32) -> Vec<u8> {
    match text.windows(2).position(|pair| pair == b"%d") {
        Some(at) => [&text[..at], value.to_string().as_bytes(), &text[at + 2..]].concat(),
        None => text.to_vec(),
    }
}

/// The game-loop key for a physical key (with Shift for the shifted digits).
pub(crate) fn game_key(key: KeyCode, [shift, control, _]: [bool; 3]) -> Option<Key> {
    use KeyCode::*;
    Some(match key {
        KeyQ if control => Key::CtrlQ,
        KeyX if control => Key::CtrlX,
        KeyP if control => Key::CtrlP,
        KeyS if control => Key::CtrlS,
        KeyL if control => Key::CtrlL,
        Tab => Key::Tab,
        Space => Key::Space,
        Enter | NumpadEnter => Key::Enter,
        Escape => Key::Escape,
        Home => Key::Home,
        F2 => Key::F2,
        F3 => Key::F3,
        F4 => Key::F4,
        F5 => Key::F5,
        F6 => Key::F6,
        F7 => Key::F7,
        F10 => Key::F10,
        F12 => Key::F12,
        Comma => Key::Char(','),
        Period => Key::Char('.'),
        Slash if shift => Key::Char('?'),
        Slash => Key::Char('/'),
        Minus | NumpadSubtract => Key::Char('-'),
        Equal | NumpadAdd => Key::Char('='),
        // A letter whatever Shift; a number-row digit or its shifted symbol.
        _ => match key_text(key, false).filter(u8::is_ascii_alphanumeric)? {
            digit if digit.is_ascii_digit() => Key::Char(key_text(key, shift)?.into()),
            letter => Key::Char(letter.into()),
        },
    })
}

impl MapPreview {
    /// A message for the monitor (and the status line); the monitor sound
    /// plays out of combat at most once per 500 ms. G-31: only the game's
    /// own messages come here (world messages and the `.msg` texts the
    /// specifications name); diagnostics stay in the status line and the
    /// log and never reach the monitor.
    pub(crate) fn notify(&mut self, text: Vec<u8>) {
        self.monitor_push(&text);
        self.status = text;
    }
    fn monitor_push(&mut self, text: &[u8]) {
        if text.is_empty() {
            return;
        }
        let font = self.host.assets.font_number(101);
        let width = |line: &[u8]| {
            font.as_ref()
                .map_or(line.len() as i32 * 6, |f| f.text_width(line, false) as i32)
        };
        let now = self.host.world.elapsed_ms;
        if self
            .hud
            .monitor
            .push(text, width, now, self.host.world.combat)
        {
            self.play_effect(bar::MONITOR_SOUND.as_bytes(), None);
        }
    }
    /// The 640-by-480 frame: the map view, the indicator boxes and the bar.
    pub(crate) fn compose_world(&mut self, view: IndexedImage) -> IndexedImage {
        if !self.host.world.combat {
            self.hud.turn_seen = false;
        } else if self.host.world.combat_state.player_turn {
            self.hud.turn_seen = true;
        }
        let mut image = IndexedImage::new(640, 480).expect("fixed size");
        image.copy_image(&view, [0, 0]);
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        self.draw_indicators(&painter, &mut image);
        self.draw_bar(&painter, &mut image);
        image
    }
    fn draw_indicators(&self, painter: &Painter, image: &mut IndexedImage) {
        let sheet = self.host.world.player_sheet();
        let flags = sheet.map_or(0, |sheet| sheet.flags);
        let boxes = bar::indicators(
            flags & progression::ADDICTED_FLAG != 0,
            flags & 1 != 0,
            flags & progression::LEVEL_UP_FLAG != 0,
        );
        let [ox, oy] = bar::INDICATOR_ORIGIN;
        for (k, indicator) in boxes.iter().enumerate() {
            let x = ox + bar::indicator_x(k);
            if k == 0 {
                painter.blit_region(
                    image,
                    bar::INDICATOR_BOX.0,
                    Rect::new(3, 0, 127, 21),
                    [x, oy],
                    true,
                );
            } else {
                painter.blit_opaque(image, bar::INDICATOR_BOX.0, 0, [x, oy]);
            }
            let (id, key) = indicator.label();
            let label = painter.message("intrface.msg", id);
            let width = if k == 0 { 127 } else { 130 };
            painter.centered(
                image,
                101,
                key,
                &label,
                [x, oy + bar::INDICATOR_LABEL_Y],
                width,
            );
        }
    }
    fn draw_bar(&self, painter: &Painter, image: &mut IndexedImage) {
        let [bx, by] = bar::BAR_ORIGIN;
        painter.blit_opaque(image, bar::BAR_BACKGROUND, 0, [bx, by]);
        let panel = self.hud.panel;
        for (button, _, up, down, _) in bar::BAR_BUTTONS {
            // The End turn and End combat buttons exist only while the
            // panel is open; disabled (the red light) they show the up art.
            if matches!(button, BarButton::EndTurn | BarButton::EndCombat) {
                if !panel.buttons() {
                    continue;
                }
                if panel.light != Some(true) {
                    let rect = bar_button_rect(button);
                    painter.blit(image, up, 0, [rect.x, rect.y]);
                    continue;
                }
            }
            if button != BarButton::ItemSlot {
                let rect = bar_button_rect(button);
                painter.blit(image, self.button_art(rect, up, down), 0, [rect.x, rect.y]);
            }
        }
        // The message monitor: six lines, each one pixel further right.
        for (k, line) in self.hud.monitor.visible().iter().enumerate() {
            let x = bx + bar::monitor_line_x(k);
            let y = by + bar::MONITOR.y + 10 * k as i32;
            painter.text(image, 101, bar::MONITOR_COLOR, line, [x, y], bar::MONITOR.w);
        }
        // HP and AC counters: a running roll's frame, else the remembered
        // value (§3.21).
        let player = self.host.world.player;
        let hp = self.host.world.hit_points(player).unwrap_or(0);
        let max = self.host.world.player_stat(7);
        let ac = self.host.world.player_stat(9);
        let shown_hp = self.hud.hp.shown.unwrap_or(hp);
        for ((glyphs, band), origin) in [
            (
                self.hud.hp.glyphs(hp, bar::hp_band(shown_hp, max)),
                bar::HP_COUNTER,
            ),
            (self.hud.ac.glyphs(ac, bar::Band::White), bar::AC_COUNTER),
        ] {
            for (cell, glyph) in bar::COUNTER_CELLS.iter().zip(glyphs) {
                painter.blit_region(
                    image,
                    "numbers.frm",
                    Rect::new(band.offset() + glyph.x(), 0, glyph.width(), 17),
                    [bx + origin[0] + cell, by + origin[1]],
                    true,
                );
            }
        }
        // AP pips: green on the player's turn, red otherwise, none outside
        // combat or before the player's first turn.
        let combat = self.host.world.combat;
        let ap = (combat && self.hud.turn_seen).then(|| {
            self.host
                .world
                .critter_state(player)
                .map_or(0, |state| state.ap)
        });
        let your_turn = self.host.world.combat_state.player_turn;
        for (i, pip) in bar::pips(ap, your_turn).iter().enumerate() {
            let art = match pip {
                Some(bar::Pip::Green) => bar::PIP_ART.0,
                Some(bar::Pip::Red) => bar::PIP_ART.1,
                None => continue,
            };
            painter.blit_opaque(image, art, 0, [bx + bar::pip_x(i), by + bar::PIP_STRIP.y]);
        }
        self.draw_item_slot(painter, image);
        self.draw_ammo_bar(image);
        // §3.25 the combat panel: its animation frame (or the last one
        // drawn), and once open the light over it.
        let (art, origin) = bar::PANEL;
        let at = [bx + origin[0], by + origin[1]];
        if let Some(frame) = panel.frame {
            painter.blit_opaque(image, art, usize::from(frame), at);
        }
        if panel.buttons()
            && let Some(green) = panel.light
        {
            let light = if green {
                bar::LIGHTS[0].0
            } else {
                bar::LIGHTS[1].0
            };
            painter.blit(image, light, 0, at);
        }
    }
    /// §3.23 the ammo bar: one pixel wide at (463, 26), 70 tall, filled from
    /// the bottom in raw indices 196 and 14 by the active item's rounds
    /// (a weapon) or charges (a miscellaneous item), else empty.
    fn draw_ammo_bar(&self, image: &mut IndexedImage) {
        let [bx, by] = bar::BAR_ORIGIN;
        let world = &self.host.world;
        let (current, max) = self.active_item().map_or((0, 0), |item| {
            let data = world
                .get(item)
                .and_then(|object| match &object.prototype.as_ref()?.data {
                    PrototypeData::Item(item) => Some(&item.data),
                    _ => None,
                });
            match data {
                Some(ItemData::Weapon(_)) => world
                    .weapon_profile(item, self.slot_hit_mode(self.slot_mode()))
                    .map_or((0, 0), |weapon| (weapon.rounds, weapon.capacity)),
                Some(ItemData::Misc { charges, .. }) => {
                    (world.charges(item).unwrap_or(0), *charges)
                }
                _ => (0, 0),
            }
        });
        for (y, color) in bar::ammo_bar(current, max).iter().enumerate() {
            let (x, y) = (bx + bar::AMMO_BAR[0], by + bar::AMMO_BAR[1] + y as i32);
            if let Some(pixel) = image.pixels_mut().get_mut((y * 640 + x) as usize) {
                *pixel = *color;
            }
        }
    }
    /// §3.23 the active weapon is disabled: both arms crippled, or one with
    /// a two-handed weapon (an empty hand is a weapon too). The slot then
    /// shows only the disabled background and the item art, and its button
    /// does nothing.
    pub(crate) fn slot_disabled(&self) -> bool {
        use vault_core::critical::effect::{CRIPPLED_LEFT_ARM, CRIPPLED_RIGHT_ARM};
        let item = self.active_item();
        if !self.slot_weapon(item) {
            return false;
        }
        let flags = self
            .host
            .world
            .critter_state(self.host.world.player)
            .map_or(0, |state| state.results.0);
        let two_handed = item
            .and_then(|item| self.host.world.get(item))
            .and_then(|object| object.prototype.as_ref())
            .is_some_and(|prototype| prototype.extended.contains(ExtendedFlags::TWO_HANDED));
        bar::slot_disabled(
            flags & CRIPPLED_LEFT_ARM != 0,
            flags & CRIPPLED_RIGHT_ARM != 0,
            two_handed,
        )
    }
    /// §3.25 one frame of the combat panel: entering combat plays
    /// `iciboxx1` and shows frames 0-4, one every 200 ms (the bar blocks
    /// meanwhile), then the buttons appear disabled with the red light and
    /// `icombat1`; the player's turn lights it green with `icombat2`, its
    /// end red again with `icombat1`; a combat end removes the buttons,
    /// plays `icibcxx1` and shows frames 4-0. Returns whether it changed.
    pub(crate) fn panel_tick(&mut self, now: u64) -> bool {
        let panel = &mut self.hud.panel;
        if let Some((opening, start)) = panel.animation {
            let elapsed = now.saturating_sub(start);
            let frame = bar::panel_frame(elapsed, opening);
            let changed = panel.frame != Some(frame);
            panel.frame = Some(frame);
            if elapsed >= 4 * bar::PANEL_FRAME_MS {
                panel.animation = None;
                if opening {
                    panel.light = Some(false);
                    self.play_effect(bar::LIGHTS[1].1.as_bytes(), None);
                }
                return true;
            }
            return changed;
        }
        let world = &self.host.world;
        let combat = world.combat;
        let your_turn = combat && world.combat_state.player_turn;
        if combat != panel.open {
            panel.open = combat;
            panel.light = None;
            let (frame, sound) = if combat {
                (0, bar::PANEL_OPEN_SOUND)
            } else {
                (4, bar::PANEL_CLOSE_SOUND)
            };
            panel.frame = Some(frame);
            panel.animation = Some((combat, now));
            self.play_effect(sound.as_bytes(), None);
            return true;
        }
        if panel.open && panel.light != Some(your_turn) {
            panel.light = Some(your_turn);
            let sound = if your_turn {
                bar::LIGHTS[0].1
            } else {
                bar::LIGHTS[1].1
            };
            self.play_effect(sound.as_bytes(), None);
            return true;
        }
        false
    }
    /// The bar blocks game input and the world: a counter roll (§3.21) or
    /// the panel's animation, which busy-waits (§3.25).
    pub(crate) fn bar_blocks(&self) -> bool {
        self.counters_rolling() || self.hud.panel.animation.is_some()
    }
    /// SPEC-014 §3.26-§3.27 the combat lock: from combat start to the
    /// player's turn, between player turns, and while the combat-wide count
    /// of running sequences that rose during the player's turn is above 0,
    /// the game mouse, the keyboard and the bar take nothing: clicks are
    /// discarded and keys held.
    pub(crate) fn combat_locked(&self) -> bool {
        clicks::input_locked(&self.host.click_state())
    }
    /// §3.27 the keyboard is disabled by the combat lock: no key is read.
    pub(crate) fn keyboard_locked(&self) -> bool {
        matches!(self.screen, super::Screen::Map)
            && !self.host.world.dialogue.open
            && self.combat_locked()
    }
    /// §3.27 a key event typed while the keyboard is disabled is held, up
    /// to 32 (the first 32 are kept).
    pub(crate) fn hold_key(&mut self, event: HeldKey) {
        if self.hud.held_keys.len() < HELD_KEYS {
            self.hud.held_keys.push_back(event);
        }
    }
    /// A key went up: held while the keyboard is disabled or a roll or the
    /// panel animation defers keys, so it counts toward the 32 events
    /// (§3.21, §3.27). Game input acts on presses only.
    pub fn key_up(&mut self, key: KeyCode) {
        self.enhanced_key_up(key);
        if !self.fade_blocks() && (self.keyboard_locked() || self.bar_blocks()) {
            self.hold_key(HeldKey::Release);
        }
    }
    /// §3.27 the held keys: flushed at the start of each player turn,
    /// before its combat script and stand-up (keys typed at combat start,
    /// on NPC turns or in the last sequence of the player's previous turn
    /// are lost), and otherwise read in order once the keyboard is enabled
    /// again, the end of combat included. A key that locks it again (a
    /// sequence it starts) leaves the rest held. Returns whether any acted.
    pub(crate) fn read_held_keys(&mut self) -> bool {
        let turns = self.host.combat_lock.turns;
        if self.hud.key_turns != turns {
            self.hud.key_turns = turns;
            self.hud.held_keys.clear();
        }
        let mut read = false;
        // A fade or a counter roll still holds input back (§3.2, §3.21).
        while !self.keyboard_locked() && !self.fade_blocks() && !self.bar_blocks() {
            let Some(event) = self.hud.held_keys.pop_front() else {
                break;
            };
            // §3.29: the event is translated as it is read, with the
            // modifiers it was typed with; a modifier key alone is no key
            // code, so its event is removed and nothing acts.
            if let HeldKey::Press(key, modifiers) = event {
                let current = std::mem::replace(&mut self.hud.modifiers, modifiers);
                self.set_enhanced_replay(true);
                self.step(key);
                self.set_enhanced_replay(false);
                self.hud.modifiers = current;
                read |= !crate::input::is_modifier(key);
            }
        }
        read
    }
    /// §3.23 swap hands, step 4: while its animation holds the player the
    /// bar and the mouse are disabled.
    pub(crate) fn swapping_hands(&self) -> bool {
        self.hud.swapping && self.host.owns_motion(self.host.world.player)
    }
    /// §3.26 the animated watch `wait2.frm` shows: the combat lock or a
    /// hand swap. Meanwhile the in-world cursor is hidden and map clicks
    /// and bar presses are ignored.
    pub(crate) fn watch_shown(&self) -> bool {
        self.combat_locked() || self.swapping_hands()
    }
    /// The §3.27 policy for a player's click that passed the lock.
    pub(crate) fn click_policy(&self, click: Click) -> Policy {
        clicks::click_policy(click, &self.host.click_state(), self.interrupt_walk)
    }
    /// One frame of the swap and the watch: a finished swap returns a
    /// crosshair or use mode to movement (§3.23 step 5), and the watch
    /// advances one frame every 100 ms from when it was set (§3.26).
    /// Returns whether the frame changed.
    pub(crate) fn watch_tick(&mut self, now: u64) -> bool {
        let mut changed = false;
        if self.hud.swapping && !self.host.owns_motion(self.host.world.player) {
            self.hud.swapping = false;
            if matches!(self.hud.mode, Mode::Crosshair | Mode::UseCrosshair) {
                self.hud.mode = Mode::Movement;
            }
            changed = true;
        }
        let shown = self.watch_shown();
        match (shown, self.hud.watch) {
            (false, None) => {}
            (false, Some(_)) => {
                self.hud.watch = None;
                changed = true;
            }
            (true, None) => {
                self.hud.watch = Some((now, 0));
                changed = true;
            }
            (true, Some((since, frame))) => {
                let current = cursor::animated_frame(now.saturating_sub(since), WATCH_FRAMES);
                if current != frame {
                    self.hud.watch = Some((since, current));
                    changed = true;
                }
            }
        }
        changed
    }
    /// After a load the panel shows the restored combat state at once.
    pub(crate) fn settle_panel(&mut self) {
        let world = &self.host.world;
        self.hud.panel = Panel::settled(world.combat, world.combat_state.player_turn);
    }
    /// §3.21 advance the counters: an unrequested change is drawn at once,
    /// a requested one rolls one unit at a time. Returns whether the bar
    /// changed.
    pub(crate) fn counters_tick(&mut self, now: u64) -> bool {
        let player = self.host.world.player;
        let hp = self.host.world.hit_points(player).unwrap_or(0);
        let max = self.host.world.player_stat(7);
        let ac = self.host.world.player_stat(9);
        let hp_changed = self.hud.hp.tick(now, hp, |old, first| {
            bar::roll_segments(old, hp, max, first)
        });
        let ac_changed = self
            .hud
            .ac
            .tick(now, ac, |old, _| vec![(old, ac, bar::Band::White)]);
        hp_changed || ac_changed
    }
    /// A counter roll blocks the caller: the world and game input wait.
    pub(crate) fn counters_rolling(&self) -> bool {
        self.hud.hp.roll.is_some() || self.hud.ac.roll.is_some()
    }
    /// The active hand's item, if any.
    pub(crate) fn active_item(&self) -> Option<ObjectRef> {
        let mode = if self.host.world.active_hand == 0 {
            hit_mode::LEFT_PRIMARY
        } else {
            hit_mode::RIGHT_PRIMARY
        };
        self.host.world.wielded(self.host.world.player, mode)
    }
    /// §3.23 the active item slot, in its up image or, while held, its
    /// down image: `sattkbdn.frm`, the label darkened to light 59641/65536
    /// one pixel right and two up, the cost one right and two up, and the
    /// item art darkened to 63571/65536 one right and two up.
    fn draw_item_slot(&self, painter: &Painter, image: &mut IndexedImage) {
        let [bx, by] = bar::BAR_ORIGIN;
        let slot = bar::SLOT;
        let origin = [bx + slot.x, by + slot.y];
        let pressed = self.held_button() == Some(Rect::new(origin[0], origin[1], slot.w, slot.h));
        let level = |light: u32| (light * 128 / 65536) as u8;
        let item = self.active_item();
        let use_on = item
            .and_then(|item| self.host.world.get(item))
            .and_then(|object| object.prototype.as_ref())
            .is_some_and(|prototype| prototype.permits_use_on());
        // A weapon (or the empty hand, which punches) shows its chosen
        // mode: reload, or the attack animation of the mode's hit mode with
        // the bullseye when aimed; a non-weapon shows use or use-on.
        let mode = self.slot_mode();
        let disabled = self.slot_disabled();
        let (action, cost, aimed) = if !self.slot_weapon(item) {
            let action = if use_on {
                bar::SlotAction::UseOn
            } else {
                bar::SlotAction::Use
            };
            (action, 2, false)
        } else if mode == WeaponMode::Reload {
            (bar::SlotAction::Reload, 2, false)
        } else {
            let hit = self.slot_hit_mode(mode);
            let animation = match item {
                None => attack_mode::PUNCH,
                Some(item) => self
                    .host
                    .world
                    .weapon_profile(item, hit)
                    .map_or(attack_mode::NONE, |weapon| weapon.mode),
            };
            (
                bar::SlotAction::Attack(animation),
                self.slot_attack_cost(hit, mode.aimed()),
                mode.aimed(),
            )
        };
        match bar::cost_digit(cost).filter(|_| !disabled) {
            // No cost, or a disabled weapon: the plain background replaces
            // the labels.
            None => {
                painter.blit_opaque(image, bar::SLOT_DISABLED, 0, origin);
            }
            Some(digit) => {
                let background = if pressed {
                    "sattkbdn.frm"
                } else {
                    "sattkbup.frm"
                };
                painter.blit_opaque(image, background, 0, origin);
                if let Some(label) = bar::slot_label(action)
                    && let Some([w, _]) = painter.size(label)
                {
                    let [x, y] = bar::slot_label_origin(w, pressed);
                    let at = [origin[0] + x, origin[1] + y];
                    if pressed {
                        painter.blit_lit(image, label, at, level(bar::PRESSED_LABEL_LIGHT));
                    } else {
                        painter.blit(image, label, 0, at);
                    }
                }
                if aimed {
                    let (art, up_at, down_at) = bar::BULLSEYE;
                    let at = if pressed { down_at } else { up_at };
                    painter.blit(image, art, 0, [origin[0] + at[0], origin[1] + at[1]]);
                }
                let (art, up_at, down_at) = bar::COST_LABEL;
                let label_at = if pressed { down_at } else { up_at };
                painter.blit(
                    image,
                    art,
                    0,
                    [origin[0] + label_at[0], origin[1] + label_at[1]],
                );
                // Digit cells are 10 wide; the last (9) is clipped to the
                // strip's 9 remaining columns (safe rule).
                let (digits, up_at, down_at) = bar::COST_DIGIT;
                let digit_at = if pressed { down_at } else { up_at };
                let width = if digit == 9 { 9 } else { 10 };
                painter.blit_region(
                    image,
                    digits,
                    Rect::new(10 * i32::from(digit), 0, width, 12),
                    [origin[0] + digit_at[0], origin[1] + digit_at[1]],
                    false,
                );
            }
        }
        let Some(item) = item else {
            return;
        };
        let inventory_art = self.host.world.get(item).and_then(|object| {
            object
                .prototype
                .as_ref()
                .and_then(|prototype| match &prototype.data {
                    PrototypeData::Item(data) => Some(data.inventory_art),
                    _ => None,
                })
        });
        // The item art, centered and (safe rule) clipped to the slot; the
        // down image draws it darkened at (x + 1, y - 2), from row 0 with
        // two rows fewer when that goes above the slot.
        if let Some(fid) = inventory_art
            && let Ok(art) = vault_core::assets::WorldAssets::object_art(&self.host.assets, fid)
            && let Some(frame) = art.frame(0, 0)
        {
            let (w, h) = (i32::from(frame.width), i32::from(frame.height));
            let [mut x, mut y] = bar::item_art_origin(w, h);
            let mut rows = h;
            let pressed = pressed && !disabled;
            if pressed {
                x += 1;
                y -= 2;
                if y < 0 {
                    y = 0;
                    rows = h - 2;
                }
            }
            for row in 0..rows.max(0) {
                for column in 0..w {
                    let (sx, sy) = (x + column, y + row);
                    let color = frame.pixels()[(row * w + column) as usize];
                    if color == 0 || !(0..slot.w).contains(&sx) || !(0..slot.h).contains(&sy) {
                        continue;
                    }
                    let (dx, dy) = (origin[0] + sx, origin[1] + sy);
                    image.pixels_mut()[(dy * 640 + dx) as usize] = if pressed {
                        painter
                            .palette
                            .intensity(color, level(bar::PRESSED_ITEM_LIGHT))
                    } else {
                        color
                    };
                }
            }
        }
    }
    /// §3.20 the message monitor scroll area under screen `point`: `true`
    /// the upper half (one line older), `false` the lower (newer). Both
    /// belong to the bar's enable state, so neither exists while the bar
    /// is disabled: by a script, the combat lock or a hand swap.
    pub(crate) fn monitor_area(&self, point: [i32; 2]) -> Option<bool> {
        if self.host.world.ui_disabled || self.watch_shown() {
            return None;
        }
        let [bx, by] = bar::BAR_ORIGIN;
        let local = [point[0] - bx, point[1] - by];
        if bar::MONITOR_OLDER.contains(local) {
            Some(true)
        } else if bar::MONITOR_NEWER.contains(local) {
            Some(false)
        } else {
            None
        }
    }
    /// A press in the bar (screen y of at least 379): a left press holds a
    /// button down until the release, which sends its key; a right press
    /// on the item slot cycles the item mode. Returns whether it was
    /// handled.
    pub(crate) fn bar_click(&mut self, point: [i32; 2], right: bool) -> bool {
        let [bx, by] = bar::BAR_ORIGIN;
        let local = [point[0] - bx, point[1] - by];
        // §3.20: the monitor's scroll areas scroll once on the press, with
        // no sound and no repeat, and only while the bar is enabled (the
        // combat lock and a hand swap were refused before this).
        if let Some(older) = self.monitor_area(point) {
            if older {
                self.hud.monitor.older();
            } else {
                self.hud.monitor.newer();
            }
            return true;
        }
        let panel = self.hud.panel;
        let Some(&(button, ..)) = bar::BAR_BUTTONS.iter().find(|(button, rect, ..)| {
            rect.contains(local)
                && (panel.buttons() || !matches!(button, BarButton::EndTurn | BarButton::EndCombat))
        }) else {
            return local[1] >= 0;
        };
        // §3.25 disabled combat buttons (the red light) and §3.23 a disabled
        // item slot take no press.
        let disabled = match button {
            BarButton::EndTurn | BarButton::EndCombat => panel.light != Some(true),
            BarButton::ItemSlot => self.slot_disabled(),
            _ => false,
        };
        if disabled {
            return true;
        }
        // G-03 / SPEC-005 0x8133: while a script has the interface disabled
        // the §3.19 buttons do nothing (no press, sound or key), and the
        // message monitor's scroll areas (§3.20) with them.
        if self.host.world.ui_disabled {
            return true;
        }
        let (press, release) = if button == BarButton::ItemSlot {
            bar::SLOT_SOUNDS
        } else {
            bar::BUTTON_SOUNDS
        };
        if right {
            if button == BarButton::ItemSlot {
                self.press_cue(press.as_bytes(), release.as_bytes());
                self.game_action(Action::CycleItemMode);
            }
            return true;
        }
        self.press_button_cue(
            press.as_bytes(),
            release.as_bytes(),
            bar_button_rect(button),
        );
        self.hud.held = Some(button);
        true
    }
    /// §3.27 a left press in the bar while the watch shows: the other bar
    /// buttons are disabled, but the Swap hands button never is. It still
    /// shows its pressed art and plays its sounds; the `b` its release
    /// sends does nothing while the bar is disabled (`bar_release`).
    /// `[May-improve]` (disable it too) is not taken.
    pub(crate) fn swap_button_press(&mut self, point: [i32; 2]) {
        let rect = bar_button_rect(BarButton::SwapHands);
        if !rect.contains(point) || self.host.world.ui_disabled {
            return;
        }
        let (press, release) = bar::BUTTON_SOUNDS;
        self.press_button_cue(press.as_bytes(), release.as_bytes(), rect);
        self.hud.held = Some(BarButton::SwapHands);
    }
    /// A left release over the held bar button `button` (`over` is the
    /// held rectangle when the release is over it): the button's release
    /// sound, then its key, which plays `ib1p1xx1` as the game loop does,
    /// or the item slot's use. A release off the button does nothing.
    pub(crate) fn bar_release(&mut self, button: Option<BarButton>, over: Option<Rect>) -> bool {
        let Some(button) = button else {
            return false;
        };
        // The combat buttons are gone once the panel closes, and do nothing
        // while disabled.
        let panel = self.hud.panel;
        let gone = matches!(button, BarButton::EndTurn | BarButton::EndCombat)
            && (!panel.buttons() || panel.light != Some(true))
            || (button == BarButton::ItemSlot && self.slot_disabled());
        // A button sends its key on the release, which a disabled interface
        // ignores too (G-03).
        // §3.23, §3.27: nor while the watch shows (the combat lock or a
        // hand swap).
        if over != Some(bar_button_rect(button))
            || gone
            || self.host.world.ui_disabled
            || self.watch_shown()
        {
            return false;
        }
        if let Some(cue) = self.pressed.take() {
            self.play_effect(cue, None);
        }
        if !matches!(
            button,
            BarButton::ItemSlot | BarButton::EndTurn | BarButton::EndCombat
        ) {
            self.play_effect(vault_core::ui::BUTTON_PRESS.as_bytes(), None);
        }
        let _ = match button {
            BarButton::ItemSlot => {
                self.use_item_slot();
                true
            }
            BarButton::Inventory => self.game_action(Action::Inventory),
            BarButton::Options => self.game_action(Action::Options),
            BarButton::Skilldex => self.game_action(Action::Skilldex),
            BarButton::Map => self.game_action(Action::Automap),
            BarButton::Character => self.game_action(Action::Character),
            BarButton::PipBoy => self.game_action(Action::PipBoy),
            BarButton::SwapHands => self.game_action(Action::SwapHands),
            BarButton::EndTurn => {
                self.host.end_player_turn();
                true
            }
            BarButton::EndCombat => {
                let mut ai = std::mem::replace(
                    &mut self.host.combat_ai,
                    Box::new(vault_core::combat_state::NoAi),
                );
                self.host.request_end_combat(&mut *ai);
                self.host.combat_ai = ai;
                true
            }
        };
        true
    }
    /// §3.23 left click on the item slot: a weapon sets the crosshair (and
    /// starts combat) or reloads; a non-weapon used on a target sets the
    /// use crosshair; a use-only non-weapon is used at once.
    fn use_item_slot(&mut self) {
        let item = self.active_item();
        if let Some(item) = item.filter(|item| !self.slot_weapon(Some(*item))) {
            let use_on = self
                .host
                .world
                .get(item)
                .and_then(|object| object.prototype.as_ref())
                .is_some_and(|prototype| prototype.permits_use_on());
            if use_on {
                self.hud.mode = Mode::UseCrosshair;
                return;
            }
            // G-15: a use-only item is used at once; in combat it needs and
            // spends 2 AP (a book then refuses with 902 but the AP stay
            // spent, SPEC-013 §3.25). The use is the inventory's: a skill
            // book by the item-slot route, an explosive's Set Timer window
            // (SPEC-013 §3.10), a Motion Sensor's automap (§3.33), a drug,
            // a kit on the player, else the use procedure.
            if self.host.world.combat {
                let player = self.host.world.player;
                let Some(state) = self.host.world.critter_state_mut(player) else {
                    return;
                };
                if state.ap < 2 {
                    return;
                }
                state.ap -= 2;
            }
            self.use_item_now(item, vault_core::BookRoute::ItemSlot);
            return;
        }
        if let Some(weapon) = item
            && self.slot_mode() == WeaponMode::Reload
        {
            self.reload_from_slot(weapon);
            return;
        }
        // Another weapon mode (the empty hand too): the crosshair, starting
        // combat when not in combat.
        self.hud.mode = Mode::Crosshair;
        self.request_combat();
    }
    /// §3.23 left-click in reload mode: reload from the inventory until a
    /// reload fails (the SPEC-011 §3.15 search reload, so several stacks
    /// can go). In combat 2 AP are needed, checked first, and spent only
    /// when something loaded. Then the mode advances and the weapon's
    /// ready sound plays (SPEC-010).
    fn reload_from_slot(&mut self, weapon: ObjectRef) {
        let player = self.host.world.player;
        let combat = self.host.world.combat;
        if combat
            && self
                .host
                .world
                .critter_state(player)
                .is_none_or(|state| state.ap < 2)
        {
            return;
        }
        let mut loaded = false;
        // Each reload fills the magazine or uses up (and destroys) one unit,
        // so the loop ends; the cap only bounds malformed empty packs.
        for _ in 0..RELOAD_LIMIT {
            match self.host.world.search_reload(player, weapon) {
                Ok(true) => loaded = true,
                Ok(false) => break,
                Err(error) => {
                    self.status = error.to_string().into_bytes();
                    break;
                }
            }
        }
        if loaded
            && combat
            && let Some(state) = self.host.world.critter_state_mut(player)
        {
            state.ap = (state.ap - 2).max(0);
        }
        let next = bar::next_weapon_mode(WeaponMode::Reload, |mode| self.slot_mode_available(mode));
        let hand = self.hand();
        self.hud.slots[hand].set(Some(weapon), next);
        let name = self
            .host
            .weapon_sound_name(
                vault_core::ai::READY_SOUND,
                weapon,
                hit_mode::RIGHT_PRIMARY,
                0,
            )
            .unwrap_or_default();
        self.host.world.emit(WorldEffect::WeaponSound {
            object: player,
            weapon,
            effect: vault_core::ai::READY_SOUND,
            name,
        });
        self.drain_effects();
    }
    /// The player asks for combat when not in it (`a`, and the item slot's
    /// attack modes).
    fn request_combat(&mut self) {
        if !self.host.world.combat {
            let player = self.host.world.player;
            self.host.world.pending_combat = Some(vault_core::combat_state::CombatRequest {
                attacker: player,
                defender: 0,
                accuracy: 0,
                damage: [0, i32::MAX],
                flags: None,
            });
        }
    }
    /// The active hand: 0 left, 1 right.
    fn hand(&self) -> usize {
        usize::from(self.host.world.active_hand != 0)
    }
    /// §3.23 a hand holding a weapon, or an empty hand (a weapon with punch
    /// as both its primary and secondary hit mode).
    pub(crate) fn slot_weapon(&self, item: Option<ObjectRef>) -> bool {
        item.is_none_or(|item| self.host.world.item_subtype(item) == Some(ItemSubtype::Weapon))
    }
    /// §3.23 the active hand's chosen action mode: the object in the hand
    /// keeps it; another object starts at the primary attack.
    pub(crate) fn slot_mode(&self) -> WeaponMode {
        self.hud.slots[self.hand()].mode(self.active_item())
    }
    /// The active hand's hit mode in `mode` (SPEC-007 §3.6): punch for an
    /// empty hand, else the hand's primary or secondary hit mode. Reload
    /// reports no attack (`slot_attack`); here it reads the primary, for
    /// the ammo bar.
    fn slot_hit_mode(&self, mode: WeaponMode) -> u8 {
        if self.active_item().is_none() {
            return hit_mode::PUNCH;
        }
        match (self.hand(), mode.secondary()) {
            (0, false) => hit_mode::LEFT_PRIMARY,
            (0, true) => hit_mode::LEFT_SECONDARY,
            (_, false) => hit_mode::RIGHT_PRIMARY,
            (_, true) => hit_mode::RIGHT_SECONDARY,
        }
    }
    /// §3.23 the AP cost the slot shows for an attack: SPEC-007 §3.4's cost
    /// (base cost, the player's reductions, +1 when aimed, at least 1).
    fn slot_attack_cost(&self, hit: u8, aimed: bool) -> i32 {
        let world = &self.host.world;
        let player = world.player;
        match world.attack_profile(player, player, hit, location::UNCALLED, aimed) {
            Ok(attack) => attack.ap_cost(),
            // No character to cost it against: the prototype's cost.
            Err(_) => {
                let base = self
                    .active_item()
                    .and_then(|item| world.weapon_profile(item, hit))
                    .map_or(3, |weapon| weapon.ap_cost);
                base + i32::from(aimed)
            }
        }
    }
    /// §3.23 whether the active hand can take `mode` when the slot cycles:
    /// aimed modes need called shots allowed (SPEC-007 §3.4); secondary
    /// modes a secondary hit mode that is not punch or kick (an empty
    /// hand's is punch) and has an attack type; reload current ammunition
    /// different from the maximum.
    fn slot_mode_available(&self, mode: WeaponMode) -> bool {
        let world = &self.host.world;
        let player = world.player;
        let item = self.active_item();
        let hit = self.slot_hit_mode(mode);
        match mode {
            WeaponMode::Primary => true,
            WeaponMode::Reload => item
                .and_then(|item| world.weapon_profile(item, hit))
                .is_some_and(|weapon| weapon.rounds != weapon.capacity),
            _ => {
                // An attack type: modes 9-15 count as none, as 0 does
                // (SPEC-007 §3.5 safe rule).
                let secondary = !mode.secondary()
                    || item
                        .and_then(|item| world.weapon_profile(item, hit))
                        .is_some_and(|weapon| {
                            vault_core::combat::Subtype::of(weapon.mode).is_some()
                        });
                secondary
                    && (!mode.aimed()
                        || world
                            .attack_profile(player, player, hit, location::UNCALLED, true)
                            .is_ok_and(|attack| attack.aimed_shot_available()))
            }
        }
    }
    /// §3.23 right-click on the slot or `n`: a weapon or the empty hand
    /// runs primary, primary aimed, secondary, secondary aimed, reload,
    /// then primary again, skipping the modes it does not have. A
    /// non-weapon keeps its action.
    fn cycle_item_mode(&mut self) {
        let item = self.active_item();
        if !self.slot_weapon(item) {
            return;
        }
        let next = bar::next_weapon_mode(self.slot_mode(), |mode| self.slot_mode_available(mode));
        let hand = self.hand();
        self.hud.slots[hand].set(item, next);
    }
    /// One game-loop action (SPEC-014 §3.19). Returns whether it acted.
    pub(crate) fn game_action(&mut self, action: Action) -> bool {
        match action {
            Action::Inventory => self.open_inventory(),
            Action::Character => self.open_character(),
            action @ (Action::PipBoy | Action::AlarmClock) => {
                if self.host.world.combat {
                    // §3.13: a title-only large box at (192, 116), key 32328.
                    self.play_effect(vault_core::ui::ERROR_SOUND.as_bytes(), None);
                    let title = self.message("misc.msg", 7);
                    self.show_dialog(super::dialog::Dialog::done(
                        [192, 116],
                        title,
                        Vec::new(),
                        vault_core::ui::ColorKey::ERROR,
                    ));
                } else if action == Action::AlarmClock {
                    // G-26: `z` opens the Pip-Boy on its alarm-clock page.
                    self.open_pipboy_on(super::pipboy::Tab::Alarm);
                } else {
                    self.open_pipboy();
                }
            }
            Action::Skilldex => self.open_skilldex(),
            Action::Options => self.open_options(),
            Action::Pause => self.open_pause(),
            Action::QuitConfirm => self.open_quit_confirm(),
            Action::Save => self.open_save_screen(vault_core::ui::saveload::Mode::Save),
            Action::Load => self.open_save_screen(vault_core::ui::saveload::Mode::LoadInGame),
            Action::QuickSave => self.quick_save(),
            Action::QuickLoad => self.quick_load(),
            Action::SwapHands => {
                // §3.23: the core flips the hand and registers the put-away
                // and take-out; while that animation holds the player the
                // bar and the mouse are disabled under the watch, and its
                // end returns a crosshair or use mode to movement
                // (`watch_tick`). Discarded (§3.10.1), the mode returns now.
                if self.host.swap_hands() && self.host.owns_motion(self.host.world.player) {
                    self.hud.swapping = true;
                } else if matches!(self.hud.mode, Mode::Crosshair | Mode::UseCrosshair) {
                    self.hud.mode = Mode::Movement;
                }
            }
            Action::CycleCursor => self.cycle_cursor(),
            Action::CycleItemMode => self.cycle_item_mode(),
            Action::Automap => self.open_automap(false),
            Action::StartCombat => self.request_combat(),
            Action::Skill(row) => {
                if let Some(index) = skills::skill_hotkey(row) {
                    self.play_effect(vault_core::ui::BUTTON_PRESS.as_bytes(), None);
                    self.choose_skill(index);
                }
            }
            Action::CenterPlayer => self.center_on_player(),
            Action::Rotate(clockwise) => {
                // §3.27: rotating works only while the player's art is
                // standing (and never under the combat lock).
                if self.click_policy(Click::Rotate) != Policy::Proceed {
                    return false;
                }
                let player = self.host.world.player;
                if let Some(object) = self.host.world.get_mut(player) {
                    object.raw.orientation =
                        (object.raw.orientation + if clockwise { 1 } else { 5 }) % 6;
                }
            }
            Action::Brightness(step) => self.brightness_step(step > 0),
            Action::DateTime => {
                let text = self.clock_text();
                self.notify(text);
            }
            Action::Screenshot => self.screenshot(),
            Action::MasterVolume(delta) => self.master_volume_step(delta),
        }
        true
    }
    pub(crate) fn message(&self, file: &str, id: i32) -> Vec<u8> {
        Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        }
        .message(file, id)
    }
    /// A right-click in the map area or `m` (SPEC-014 §3.27).
    pub(crate) fn cycle_cursor(&mut self) {
        let weapon = self.active_item().is_none_or(|item| {
            self.host.world.item_subtype(item) == Some(vault_formats::pro::ItemSubtype::Weapon)
        });
        self.hud.mode = cursor::cycle_mode(self.hud.mode, self.host.world.combat, weapon);
    }
    /// Whether a critter can be talked to: the prototype's talk flag and an
    /// active critter (SPEC-013 §3.16).
    fn talkable(&self, critter: ObjectRef) -> bool {
        let flag = self.host.world.get(critter).is_some_and(|object| {
            object
                .prototype
                .as_ref()
                .is_some_and(|prototype| prototype.extended.contains(ExtendedFlags::TALK))
        });
        flag && self
            .host
            .world
            .critter_state(critter)
            .is_some_and(|state| state.results.0 & (0x01 | 0x80 | 0x8000) == 0)
    }
    /// The SPEC-014 §3.27 hover target kind of an object.
    pub(crate) fn target_kind(&self, object: ObjectRef) -> Option<cursor::Target> {
        let found = self.host.world.get(object)?;
        let usable = found.prototype.as_ref().is_some_and(|p| p.permits_use());
        Some(match found.category() {
            Category::Item => cursor::Target::Item {
                container: matches!(
                    found.prototype.as_ref().map(|p| &p.data),
                    Some(PrototypeData::Item(item)) if matches!(item.data, ItemData::Container { .. })
                ),
            },
            Category::Critter if object == self.host.world.player => cursor::Target::Player,
            Category::Critter => cursor::Target::Critter {
                talkable: self.talkable(object),
            },
            Category::Scenery => cursor::Target::Scenery { usable },
            _ => cursor::Target::Wall { usable },
        })
    }
    /// §3.23 the attack query: the active slot's current attack, its hit
    /// mode (punch for an empty hand, else the hand's primary or secondary
    /// mode) and aimed flag. Reload, use and use-on report no attack, and
    /// with it the UI aiming flag is false (SPEC-007 §3.4).
    pub(crate) fn slot_attack(&self) -> Option<(u8, bool)> {
        let mode = self.slot_mode();
        (self.slot_weapon(self.active_item()) && mode != WeaponMode::Reload)
            .then(|| (self.slot_hit_mode(mode), mode.aimed()))
    }
    /// §3.38 the player's attack request against `target` from a crosshair
    /// click (or its replay): nothing unless the input gate is set and the
    /// slot reports an attack; then the preflight, whose first failing test
    /// prints its `combat.msg` line. Out of combat a passing click starts
    /// combat and is replayed once on the player's first turn; in combat a
    /// mode that is not aimed attacks at once, uncalled, and an aimed mode
    /// opens the called-shot window.
    pub(crate) fn attack_request(&mut self, target: ObjectRef) {
        use vault_core::critical::state::INPUT_GATE;
        let player = self.host.world.player;
        let gate = self.host.world.combat_state.flags & INPUT_GATE != 0;
        let Some((mode, aimed)) = self.slot_attack().filter(|_| gate) else {
            return;
        };
        let attack =
            match self
                .host
                .world
                .attack_profile(player, target, mode, location::UNCALLED, aimed)
            {
                Ok(attack) => attack,
                Err(error) => {
                    self.status = error.to_string().into_bytes();
                    return;
                }
            };
        if let Err(refusal) = self.host.world.preflight(player, target, &attack) {
            self.refuse_attack(refusal, attack.ap_cost(), mode);
            return;
        }
        if !self.host.world.combat {
            self.host.world.pending_combat = Some(vault_core::combat_state::CombatRequest {
                attacker: player,
                defender: target,
                accuracy: 0,
                damage: [0, i32::MAX],
                flags: None,
            });
            self.hud.replay = Some(target);
            return;
        }
        if aimed {
            self.open_called(target, mode);
            return;
        }
        // The kick draw follows the preflight (no draws).
        match self
            .host
            .perform_attack(player, target, mode, location::UNCALLED, false)
        {
            Ok(Ok(_)) => {}
            Ok(Err(refusal)) => self.refuse_attack(refusal, attack.ap_cost(), mode),
            Err(error) => self.status = error.to_string().into_bytes(),
        }
    }
    /// §3.38 step 2: a refused attack prints its `combat.msg` line (100
    /// with the AP cost), and an empty magazine plays the weapon's
    /// out-of-ammo sound (SPEC-010 effect `O`).
    pub(crate) fn refuse_attack(&mut self, refusal: vault_core::Refusal, cost: i32, mode: u8) {
        tracing::debug!(?refusal, "player attack refused");
        if let Some(id) = vault_core::ui::called::refusal_message(refusal) {
            let text = self.message("combat.msg", id);
            let text = replace_number(&text, cost);
            self.notify(text);
        }
        if refusal == vault_core::Refusal::NoAmmunition
            && let Some(weapon) = self.active_item()
        {
            let player = self.host.world.player;
            let effect = vault_core::ai::OUT_OF_AMMO_SOUND;
            let name = self
                .host
                .weapon_sound_name(effect, weapon, mode, 0)
                .unwrap_or_default();
            self.host.world.emit(WorldEffect::WeaponSound {
                object: player,
                weapon,
                effect,
                name,
            });
        }
    }
    /// §3.38 step 3: the click that started combat is replayed once the
    /// player's first turn has given the input back (steps 1 and 2 again,
    /// with the refilled AP, then step 4). A combat that ends first drops
    /// it.
    pub(crate) fn replay_attack(&mut self) -> bool {
        let Some(target) = self.hud.replay else {
            return false;
        };
        if !self.host.world.combat {
            if self.host.world.pending_combat.is_none() {
                self.hud.replay = None;
            }
            return false;
        }
        if !self.host.world.combat_state.player_turn
            || self.combat_locked()
            || !matches!(self.screen, super::Screen::Map)
        {
            return false;
        }
        self.hud.replay = None;
        self.attack_request(target);
        true
    }
    /// A left release in the map area by cursor mode (SPEC-014 §3.27).
    pub(crate) fn map_click(&mut self, point: [i32; 2], object: Option<ObjectRef>) {
        let player = self.host.world.player;
        match self.hud.mode {
            Mode::Movement => {
                // With Ctrl in combat the current attack's AP cost is kept
                // out of the route's budget. A slot reporting no attack
                // would subtract an undefined cost (`[May-improve]`): 2, the
                // cost the slot shows for reload, use and use-on (§3.23).
                let reserve = if self.host.world.combat && self.hud.modifiers[1] {
                    match self.slot_attack() {
                        Some((mode, aimed)) => self
                            .host
                            .world
                            .attack_profile(player, player, mode, location::UNCALLED, aimed)
                            .map_or(0, |attack| attack.ap_cost()),
                        None => 2,
                    }
                } else {
                    0
                };
                // SPEC-007 §3.11.1: a run when the `running` preference is
                // on, which Shift inverts.
                let run = (self.preferences.running != 0) != self.hud.modifiers[0];
                // §3.27: the combat lock discards the click. Past it, the
                // hex comes first (off the grid nothing is cancelled); the
                // busy rules, the run's sneak clear and the no-route stop
                // are core's, so a busy player's click still reaches it.
                if clicks::input_locked(&self.host.click_state()) {
                    return;
                }
                if let Some(hex) = self.pointer_view().grid.pick_hex(point) {
                    let interrupt = self.interrupt_walk;
                    match self
                        .host
                        .walk_toward_with(player, hex, run, reserve, interrupt)
                    {
                        Ok(true) => {}
                        Ok(false) => tracing::debug!(hex, "the player's walk did not register"),
                        Err(error) => self.status = error.to_string().into_bytes(),
                    }
                }
            }
            Mode::Arrow => {
                let Some(object) = object else {
                    return;
                };
                self.selected = Some(object);
                let Some(target) = self.target_kind(object) else {
                    return;
                };
                let action = match cursor::primary_action(target, self.host.world.combat) {
                    cursor::Primary::Use if matches!(target, cursor::Target::Item { .. }) => {
                        ObjectAction::Pickup
                    }
                    cursor::Primary::Use => ObjectAction::Use,
                    cursor::Primary::Talk => ObjectAction::Talk,
                    cursor::Primary::Look => ObjectAction::Description,
                    cursor::Primary::Rotate => {
                        self.game_action(Action::Rotate(true));
                        return;
                    }
                };
                if !self.action_allowed(object, action) {
                    return;
                }
                self.interact(action);
            }
            Mode::Crosshair => {
                let Some(target) = object.filter(|o| {
                    *o != player
                        && self
                            .host
                            .world
                            .get(*o)
                            .is_some_and(|o| Fid(o.raw.art_id).category() == 1)
                }) else {
                    return;
                };
                // SPEC-007 §3.10: the player acts again once every attack
                // has finished playing.
                if self.host.world.combat && self.host.presenting() {
                    return;
                }
                // §3.38: the preflight, combat start out of combat, and the
                // called-shot window in an aimed mode. While the slot
                // reports no attack (reload mode) the click does nothing.
                self.attack_request(target);
            }
            Mode::UseCrosshair => {
                // §3.27: the mode returns to movement in every case; in
                // combat the use needs and spends 2 AP, and without them
                // nothing happens and nothing is shown.
                self.hud.mode = Mode::Movement;
                let (Some(target), Some(item)) = (object, self.active_item()) else {
                    return;
                };
                if !self.action_allowed(target, ObjectAction::UseObject(item)) {
                    return;
                }
                if self.host.world.combat {
                    let Some(state) = self.host.world.critter_state_mut(player) else {
                        return;
                    };
                    if state.ap < 2 {
                        return;
                    }
                    state.ap -= 2;
                }
                self.selected = Some(target);
                self.interact(ObjectAction::UseObject(item));
            }
            Mode::Skill(index) => {
                // SPEC-013 §3.1: no object returns to movement; an accepted
                // request returns to movement, a rejected one stays.
                let Some(target) = object else {
                    self.hud.mode = Mode::Movement;
                    return;
                };
                if !self.action_allowed(target, ObjectAction::UseSkill(i32::from(index))) {
                    return;
                }
                match self.host.use_skill_on(player, target, i32::from(index)) {
                    Ok(true) => self.hud.mode = Mode::Movement,
                    Ok(false) => {}
                    Err(error) => self.status = error.to_string().into_bytes(),
                }
                self.drain_effects();
            }
        }
    }
    /// §3.27 "Clicks while the player is busy" for an arrow click or an
    /// action-menu entry: pick up, use, loot, talk and skill uses follow
    /// the click policy (a near talk registers even while the player is
    /// busy); examining starts no sequence and always acts. The world
    /// applies the same rules again as it registers.
    pub(crate) fn action_allowed(&self, target: ObjectRef, action: ObjectAction) -> bool {
        let click = match action {
            ObjectAction::LookAt | ObjectAction::Description => return true,
            ObjectAction::Talk if self.host.near_talk(target) => Click::NearTalk,
            _ => Click::Action,
        };
        self.click_policy(click) != Policy::Ignore
    }
    /// A chosen skill: Sneak toggles (after clearing the player's
    /// animation); any other enters its targeting mode.
    pub(crate) fn choose_skill(&mut self, index: i32) {
        if index == skills::skill::SNEAK {
            if let Err(error) = self.host.toggle_sneak() {
                self.status = error.to_string().into_bytes();
            }
            return;
        }
        if let Ok(skill) = u8::try_from(index) {
            self.hud.mode = Mode::Skill(skill);
        }
    }
    /// The cursor-mode label for the window title.
    pub(crate) fn mode_name(&self) -> String {
        match self.hud.mode {
            Mode::Movement => "movement".into(),
            Mode::Arrow => "arrow".into(),
            Mode::Crosshair => "crosshair".into(),
            Mode::UseCrosshair => "use".into(),
            Mode::Skill(skill) => format!("skill {skill}"),
        }
    }
    /// Escape on the map opens the options menu; other bar hotkeys act
    /// through `game_action`. Returns whether the key was handled.
    pub(crate) fn game_hotkey(&mut self, key: KeyCode) -> bool {
        let Some(key) = game_key(key, self.hud.modifiers) else {
            return false;
        };
        // §3.27: under the combat lock the keyboard is not read (`step`
        // holds its keys), so no hotkey acts (S-04: no window opens and no
        // end of combat is requested on the enemy turn).
        if self.combat_locked() {
            return true;
        }
        // §3.23: a hand swap's wait discards its keys before they get here.
        let enabled = !self.host.world.ui_disabled;
        match bar::game_key(key, enabled, self.hud.modifiers[2]) {
            Some(action) => {
                if !matches!(action, Action::Skill(_)) {
                    self.play_effect(vault_core::ui::BUTTON_PRESS.as_bytes(), None);
                }
                self.game_action(action)
            }
            // The combat input loop: Space ends the turn, Enter requests
            // the end of combat.
            None if self.host.world.combat && key == Key::Space => {
                self.host.end_player_turn();
                true
            }
            None if self.host.world.combat && key == Key::Enter => {
                let mut ai = std::mem::replace(
                    &mut self.host.combat_ai,
                    Box::new(vault_core::combat_state::NoAi),
                );
                self.host.request_end_combat(&mut *ai);
                self.host.combat_ai = ai;
                true
            }
            None => false,
        }
    }
    /// §3.19 F12, caught as the key enters the input queue (`step_key`), so
    /// it works in every window: the whole screen recomposed, the cursor
    /// included when it shows, written as the §2.5 bitmap under the
    /// installed palette before brightness into `screenshot_dir`. The
    /// monitor then prints `misc.msg` 3 "Saved screenshot.", or 8 "Error
    /// saving screenshot." when no name is free or the file cannot be
    /// written (`save_screenshot`). No button sound plays.
    pub(crate) fn screenshot(&mut self) {
        let image = match self.image() {
            Ok(image) => Some(image),
            Err(error) => {
                tracing::warn!(%error, "screen not composed for the screenshot");
                self.pointer_frame()
            }
        };
        let saved = match image {
            Some(image) => {
                let palette = self.installed_palette();
                crate::interface::save_screenshot(&self.screenshot_dir, &image, &palette)
            }
            None => Err("no screen to save".into()),
        };
        let id = match saved {
            Ok(_) => 3,
            Err(error) => {
                tracing::warn!(%error, "screenshot not saved");
                8
            }
        };
        let text = self.message("misc.msg", id);
        self.notify(text);
    }
    /// The system pointer's pending warp (§3.28), for the shell.
    pub fn take_warp(&mut self) -> Option<[i32; 2]> {
        self.hud.warp.take()
    }
    /// A sound to play `delay` milliseconds from now.
    pub(crate) fn cue_later(&mut self, delay: u64, sound: &'static [u8]) {
        self.hud.cues.push((self.clock + delay, sound));
    }
    /// Play the sounds that have come due.
    pub(crate) fn cues_tick(&mut self, now: u64) {
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.hud.cues)
            .into_iter()
            .partition(|(at, _)| *at <= now);
        self.hud.cues = later;
        for (_, sound) in due {
            self.play_effect(sound, None);
        }
    }
    /// The menu, selection and editor share brightness (SPEC-014 §3.18).
    /// Brightness hotkeys apply, store and write the configuration at once.
    pub(crate) fn brightness_step(&mut self, up: bool) {
        self.preferences.brightness =
            vault_core::ui::options::brightness_step(self.preferences.brightness, up);
        self.store_preferences();
    }
}
