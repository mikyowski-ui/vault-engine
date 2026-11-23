//! Watch Mode's banner and its overlay on the map: what the bot is doing, how
//! fast, and the way back to the player.
//!
//! The banner is translucent by ordered dither: solid black in most cells of
//! the [`BAYER`] matrix, and the cells between left alone. A palette table
//! would darken the whole bar on the Vanilla frame, but the unlocked view finds
//! its see-through pixels by composing the interface over two fills
//! (`enhanced_map_ui`): a darkened pixel differs between them, so it would go
//! fully clear. Dither cells are the same over both, so one drawing reads alike
//! on either path.
use super::{BAYER, MapPreview, MapView, bot::BotSpeed};
use crate::interface::Painter;
use vault_core::ui::{ColorKey, Rect};
use vault_formats::HexGrid;
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// The interface frame's width.
const FRAME_WIDTH: i32 = 640;
/// A banner row: the first ends above the touch UI's pill bar (y 12).
const ROW: i32 = 12;
const FONT: i32 = 101;
const SPEED_LEFT: i32 = 2;
const SPEED_SIZE: [i32; 2] = [26, 10];
const SPEED_STEP: i32 = 28;
/// Where the map, tick and goal begin on a banner of one row.
const INFO_LEFT: i32 = SPEED_LEFT + BotSpeed::ALL.len() as i32 * SPEED_STEP + 4;
/// The [`BAYER`] cells of 16 the banner paints solid.
const SOLID_CELLS: u8 = 12;
const TAKEOVER: &[u8] = b"[Space: Pause / Human Takeover]";
const RESUME: &[u8] = b"[Space: Resume the Bot]";
/// A hex's corners around its center, as neighbors 32 apart along a row and
/// (16, 12) across rows meet.
const HEX_CORNERS: [[i64; 2]; 6] = [[0, -8], [16, -4], [16, 4], [0, 8], [-16, 4], [-16, -4]];

/// The speed buttons with their places on the frame. They sit at the left, so
/// neither the debugger's panel nor the text's width ever moves them.
fn speed_buttons() -> impl Iterator<Item = (BotSpeed, Rect)> {
    (0..).zip(BotSpeed::ALL).map(|(k, speed)| {
        let left = SPEED_LEFT + k * SPEED_STEP;
        (speed, Rect::at([left, 1], SPEED_SIZE))
    })
}

/// Paint [`SOLID_CELLS`] of `rect`'s dither cells, clipped to `image`.
fn stipple(image: &mut IndexedImage, rect: [i32; 4], color: u8) {
    let [left, top, width, height] = rect;
    let stride = image.width() as usize;
    for y in top.max(0)..(top + height).min(image.height() as i32) {
        for x in left.max(0)..(left + width).min(image.width() as i32) {
            let (x, y) = (x as usize, y as usize);
            if BAYER[y % 4][x % 4] < SOLID_CELLS {
                image.pixels_mut()[y * stride + x] = color;
            }
        }
    }
}

/// `text` with a black shadow, which keeps it legible over the dither.
fn shadowed(
    painter: &Painter,
    image: &mut IndexedImage,
    key: ColorKey,
    text: &[u8],
    origin: [i32; 2],
    limit: i32,
) {
    let shade = [origin[0] + 1, origin[1] + 1];
    painter.text(image, FONT, ColorKey::BLACK, text, shade, limit);
    painter.text(image, FONT, key, text, origin, limit);
}

/// A line `size` pixels thick, clipped to `image`.
fn line(image: &mut IndexedImage, from: [i64; 2], to: [i64; 2], size: i32, color: u8) {
    let reach = i64::from(size);
    let limit = [i64::from(image.width()), i64::from(image.height())];
    if (0..2).any(|axis| {
        from[axis].max(to[axis]) < -reach || from[axis].min(to[axis]) >= limit[axis] + reach
    }) {
        return;
    }
    let [run, rise] = [(to[0] - from[0]).abs(), -(to[1] - from[1]).abs()];
    let step = [(to[0] - from[0]).signum(), (to[1] - from[1]).signum()];
    let mut at = from;
    let mut error = run + rise;
    loop {
        let [x, y] = at.map(|value| value.clamp(-1 << 30, 1 << 30) as i32);
        image.fill_rect([x - size / 2, y - size / 2, size, size], color);
        if at == to {
            return;
        }
        let doubled = 2 * error;
        if doubled >= rise {
            error += rise;
            at[0] += step[0];
        }
        if doubled <= run {
            error += run;
            at[1] += step[1];
        }
    }
}

/// `route` as a line from hex center to hex center and `target` as a hex
/// outline, each over a black shadow. `scale` is canvas pixels per overlay
/// pixel: hexes keep their canvas size under zoom, and only the strokes grow.
/// A hex off the grid is left out.
fn draw_route(
    image: &mut IndexedImage,
    grid: HexGrid,
    route: &[i32],
    target: Option<i32>,
    scale: i32,
    [shade, path, mark]: [u8; 3],
) {
    let size = scale.max(1);
    let by = i64::from(size);
    let offset = |at: [i64; 2], by: i64| [at[0] + by, at[1] + by];
    let centers: Vec<[i64; 2]> = route
        .iter()
        .filter_map(|hex| grid.hex_center(*hex).ok())
        .collect();
    for (color, by) in [(shade, by), (path, 0)] {
        for pair in centers.windows(2) {
            line(image, offset(pair[0], by), offset(pair[1], by), size, color);
        }
    }
    let Some(center) = target.and_then(|hex| grid.hex_center(hex).ok()) else {
        return;
    };
    let corner = |k: usize| {
        let [dx, dy] = HEX_CORNERS[k % HEX_CORNERS.len()];
        [center[0] + dx, center[1] + dy]
    };
    for (color, by) in [(shade, by), (mark, 0)] {
        for k in 0..HEX_CORNERS.len() {
            let ends = [offset(corner(k), by), offset(corner(k + 1), by)];
            line(image, ends[0], ends[1], size, color);
        }
    }
}

/// A key the player can press with the bot still playing: a modifier on its
/// own, the debugger's F9 and the screenshot's F12, which only look at a game
/// the player is watching. Every other key is the player's move.
fn only_looks(key: KeyCode) -> bool {
    crate::input::is_modifier(key) || matches!(key, KeyCode::F9 | KeyCode::F12)
}

impl MapPreview {
    /// A key Watch Mode takes. Space hands control to the player, or back to
    /// the bot, and never reaches `step_key`, where it would end a combat turn,
    /// page dialogue or arm the enhanced pan. Its repeats change nothing until
    /// it comes up. Any other key but [`only_looks`] hands control over and
    /// then acts as usual.
    pub(crate) fn autoplay_key(&mut self, key: KeyCode) -> bool {
        let Some(watch) = self.watch.as_mut() else {
            return false;
        };
        if key == KeyCode::Space {
            if !watch.swallow_space {
                watch.paused = !watch.paused;
                watch.swallow_space = true;
            }
            return true;
        }
        if !only_looks(key) {
            watch.paused = true;
        }
        false
    }
    /// The release of a key Watch Mode took.
    pub(crate) fn autoplay_key_up(&mut self, key: KeyCode) -> bool {
        let taken = self.autoplay_swallows(key, false);
        if let Some(watch) = self.watch.as_mut().filter(|_| taken) {
            watch.swallow_space = false;
        }
        taken
    }
    /// A press on the banner, or a manual action that takes control back.
    pub(crate) fn autoplay_click(&mut self, point: [i32; 2]) -> bool {
        let at = self.banner_pixel(point);
        let Some(watch) = self.watch.as_mut() else {
            return false;
        };
        let speed = speed_buttons().find(|(_, rect)| rect.contains(at));
        if let Some((speed, _)) = speed {
            watch.speed = speed;
            return true;
        }
        // The debugger's panel is its own control, not a manual action.
        if self.debug_panel_at(at) {
            return false;
        }
        if let Some(watch) = self.watch.as_mut() {
            watch.paused = true;
        }
        false
    }
    /// Whether Watch Mode keeps this key event from the game. The shell must
    /// not log such an event as a `Key`: a replay runs no Watch Mode, so its
    /// Space would reach `step_key`. It logs `InputCommand::Autoplay` instead.
    pub fn autoplay_swallows(&self, key: KeyCode, pressed: bool) -> bool {
        self.watch
            .as_ref()
            .is_some_and(|watch| key == KeyCode::Space && (pressed || watch.swallow_space))
    }
    /// What `InputCommand::Autoplay` should say, if it changed: whether the bot
    /// has control, against `logged`, the last value the shell logged (false
    /// before any). The shell keeps that value, calls this after each input it
    /// handles and logs what comes back, as it does `take_debug_commands`.
    pub fn take_autoplay_change(&self, logged: &mut bool) -> Option<bool> {
        let on = self.watch.as_ref().is_some_and(|watch| !watch.paused);
        (on != *logged).then(|| {
            *logged = on;
            on
        })
    }
    /// A key the bot presses for itself. It must not read as the player's,
    /// which would pause the bot on its own Escape.
    pub(crate) fn bot_key(&mut self, key: KeyCode) -> bool {
        let watch = self.watch.take();
        let handled = self.step(key);
        self.watch = watch;
        handled
    }
    /// The banner's right edge: the debugger's panel keeps its top rows.
    fn banner_right(&self) -> i32 {
        (0..FRAME_WIDTH)
            .find(|x| self.debug_panel_at([*x, 0]))
            .unwrap_or(FRAME_WIDTH)
    }
    /// The frame pixel of a press at `point`. On the unlocked world `click` gets
    /// a canvas pixel, after the enhanced path asked with the frame pixel, so
    /// both calls go by the pointer: a canvas pixel that happens to fall on a
    /// button's numbers is no press on it.
    fn banner_pixel(&self, point: [i32; 2]) -> [i32; 2] {
        let layout = self.unlocked().and_then(|enhanced| enhanced.layout);
        let (Some(layout), Some(at), true) = (layout, self.pointer.at, self.pointer.world) else {
            return point;
        };
        let per_world = f64::from(layout.ui_scale * layout.zoom);
        [0, 1].map(|axis| {
            let canvas = f64::from(at[axis]) + 0.5;
            let surface = f64::from(layout.world_rect[axis]) + canvas * per_world;
            let from_frame = surface - f64::from(layout.ui_rect[axis]);
            (from_frame / f64::from(layout.ui_scale)).floor() as i32
        })
    }
    /// The banner, over everything but the debugger's panel. Text too long
    /// for one row takes a second, unless the touch UI's pills are there.
    pub(crate) fn draw_autoplay_hud(&self, image: &mut IndexedImage) {
        let Some(watch) = &self.watch else {
            return;
        };
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        let (takeover, takeover_key) = if watch.paused {
            (RESUME, ColorKey::AMBER)
        } else {
            (TAKEOVER, ColorKey::WHITE)
        };
        let right = self.banner_right();
        let width = painter.width(FONT, takeover);
        let left = right - width - 3;
        let status = watch.agent.status();
        let map = self.path.rsplit(['/', '\\']).next().unwrap_or_default();
        let info = format!("{map}  tick {}  {}", status.ticks, status.goal);
        let info = info.as_bytes();
        let room = left - INFO_LEFT - 6;
        let one_row = painter.width(FONT, info) <= room || self.touch.on;
        let (origin, limit, height) = if one_row {
            ([INFO_LEFT, 2], room, ROW)
        } else {
            ([3, ROW + 2], right - 6, 2 * ROW)
        };

        let black = painter.color(ColorKey::BLACK);
        stipple(image, [0, 0, right, height], black);
        image.fill_rect([0, height - 1, right, 1], black);
        for (speed, rect) in speed_buttons() {
            let (plate, key) = if speed == watch.speed {
                (ColorKey::GREEN, ColorKey::BLACK)
            } else {
                (ColorKey::BLACK, ColorKey::WHITE)
            };
            image.fill_rect([rect.x, rect.y, rect.w, rect.h], painter.color(plate));
            let label = speed.label().as_bytes();
            let text_left = rect.x + (rect.w - painter.width(FONT, label)) / 2;
            painter.text(image, FONT, key, label, [text_left, rect.y + 1], rect.w);
        }
        shadowed(&painter, image, takeover_key, takeover, [left, 2], width);
        shadowed(&painter, image, ColorKey::WHITE, info, origin, limit);
    }
    /// The target hex and route, on the world image. Only with the map in
    /// front: behind a window or a conversation there is nothing to steer.
    pub(crate) fn draw_autoplay_world(&self, image: &mut IndexedImage, view: MapView, scale: i32) {
        let Some(watch) = self.watch.as_ref().filter(|_| self.world_pointer()) else {
            return;
        };
        let palette = self.game_palette();
        let colors =
            [ColorKey::BLACK, ColorKey::GREEN, ColorKey::SELECTED].map(|key| key.resolve(palette));
        let status = watch.agent.status();
        draw_route(
            image,
            view.grid,
            &status.route,
            status.target_hex,
            scale,
            colors,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        bot::Watch,
        hud::Panel,
        test_support::{interactive, local_corpus, synthetic_corpus},
    };
    use super::*;
    use crate::enhanced::{EnhancedSettings, PointerAt, PointerButton};
    use vault_core::autoplay::AutoplayAgent;
    use vault_render::layers::EnhancedLayout;

    fn watching(preview: &mut MapPreview) {
        let agent = AutoplayAgent::new(1, false).unwrap();
        preview.watch = Some(Box::new(Watch::new(agent)));
    }
    fn paused(preview: &MapPreview) -> bool {
        preview.watch.as_ref().unwrap().paused
    }
    /// The middle of a speed button.
    fn button(index: usize) -> [i32; 2] {
        let (_, rect) = speed_buttons().nth(index).unwrap();
        [rect.x + rect.w / 2, rect.y + rect.h / 2]
    }
    /// The player's turn in combat, ready for keys.
    fn in_combat(preview: &mut MapPreview) {
        preview.tick(16).unwrap();
        preview.host.world.combat = true;
        preview.host.world.combat_state.player_turn = true;
        preview.hud.panel = Panel::settled(true, false);
    }
    /// The unlocked view, framed for `surface`, and how to point in it.
    fn unlocked(preview: &mut MapPreview, surface: [u32; 2]) -> EnhancedLayout {
        preview.set_enhanced(Some(EnhancedSettings {
            smooth_camera: false,
            ..Default::default()
        }));
        preview.tick(16).unwrap();
        let layout = EnhancedLayout::compute(surface, Some(1.0), 1.0).unwrap();
        preview.enhanced_frame(&layout).unwrap().unwrap();
        layout
    }
    fn press_at(preview: &mut MapPreview, layout: &EnhancedLayout, point: [f64; 2]) -> bool {
        let at = PointerAt {
            ui: layout.ui_pixel(point),
            world: layout.world_point(point),
            side: layout.in_side_panel(point),
        };
        preview.enhanced_button(at, PointerButton::Left, true)
    }

    #[test]
    fn autoplay_hud_space_in_combat_pauses_and_never_ends_the_turn() {
        let dir = synthetic_corpus();
        // Without Watch Mode this Space ends the turn.
        let mut plain = interactive(dir.path());
        in_combat(&mut plain);
        plain.step(KeyCode::Space);
        assert!(plain.host.world.combat_state.turn_ended);

        let mut preview = interactive(dir.path());
        in_combat(&mut preview);
        watching(&mut preview);
        assert!(preview.step(KeyCode::Space));
        assert!(paused(&preview));
        assert!(!preview.host.world.combat_state.turn_ended);
        assert!(preview.hud.held_keys.is_empty(), "not held for later");
    }

    #[test]
    fn autoplay_hud_space_resumes_with_the_same_swallowing() {
        let dir = synthetic_corpus();
        let mut preview = interactive(dir.path());
        in_combat(&mut preview);
        watching(&mut preview);
        assert!(preview.step(KeyCode::Space));
        preview.key_up(KeyCode::Space);
        assert!(preview.step(KeyCode::Space));
        assert!(!paused(&preview), "the bot has control again");
        assert!(preview.watch.as_ref().unwrap().swallow_space);
        assert!(!preview.host.world.combat_state.turn_ended);
    }

    #[test]
    fn autoplay_hud_release_is_swallowed_once() {
        let dir = synthetic_corpus();
        let mut preview = interactive(dir.path());
        watching(&mut preview);
        preview.step(KeyCode::Space);
        assert!(!preview.autoplay_key_up(KeyCode::KeyM));
        assert!(preview.watch.as_ref().unwrap().swallow_space, "another key");
        assert!(preview.autoplay_key_up(KeyCode::Space));
        assert!(!preview.watch.as_ref().unwrap().swallow_space);
        assert!(!preview.autoplay_key_up(KeyCode::Space), "once");
    }

    #[test]
    fn autoplay_hud_repeat_of_held_space_does_not_toggle() {
        let dir = synthetic_corpus();
        let mut preview = interactive(dir.path());
        in_combat(&mut preview);
        watching(&mut preview);
        for _ in 0..5 {
            assert!(preview.step(KeyCode::Space));
            assert!(paused(&preview));
        }
        assert!(!preview.host.world.combat_state.turn_ended);
    }

    #[test]
    fn autoplay_hud_manual_key_pauses_then_acts() {
        let dir = synthetic_corpus();
        let mut preview = interactive(dir.path());
        preview.tick(16).unwrap();
        watching(&mut preview);
        let mode = preview.hud.mode;
        assert!(!preview.autoplay_swallows(KeyCode::KeyM, true));
        preview.step(KeyCode::KeyM);
        assert!(paused(&preview));
        assert_ne!(preview.hud.mode, mode, "the key still acted");
    }

    #[test]
    fn autoplay_hud_keys_that_only_look_leave_the_bot_playing() {
        let dir = synthetic_corpus();
        let mut preview = interactive(dir.path());
        watching(&mut preview);
        for key in [KeyCode::ShiftLeft, KeyCode::AltLeft, KeyCode::F9] {
            preview.step(key);
            assert!(!paused(&preview), "{key:?}");
        }
        assert!(preview.debug_enabled(), "F9 still acted");
    }

    #[test]
    fn autoplay_hud_the_bots_own_key_does_not_pause_it() {
        let dir = synthetic_corpus();
        let mut preview = interactive(dir.path());
        preview.tick(16).unwrap();
        watching(&mut preview);
        let mode = preview.hud.mode;
        preview.bot_key(KeyCode::KeyM);
        assert!(!paused(&preview));
        assert_ne!(preview.hud.mode, mode, "the key still acted");
        assert!(preview.watch.is_some(), "Watch Mode stands");
    }

    /// A replay runs no agent, but its `Autoplay` record keeps a logged press
    /// on the banner off the map, as the session's press stayed off it.
    #[test]
    fn autoplay_hud_a_replay_routes_a_banner_press_as_the_session_did() {
        let dir = synthetic_corpus();
        let mut preview = interactive(dir.path());
        preview.tick(16).unwrap();
        preview.replay_autoplay(true).unwrap();
        assert!(preview.click(button(2), false));
        assert_eq!(preview.watch.as_ref().unwrap().speed, BotSpeed::X4);
        assert!(preview.pointer.held.is_none(), "no press on the map");
        preview.replay_autoplay(false).unwrap();
        assert!(paused(&preview));
    }

    #[test]
    fn autoplay_hud_click_on_a_speed_button_sets_it_and_goes_no_further() {
        let dir = synthetic_corpus();
        let mut preview = interactive(dir.path());
        preview.tick(16).unwrap();
        watching(&mut preview);
        for (index, speed) in BotSpeed::ALL.into_iter().enumerate() {
            assert!(preview.click(button(index), false));
            assert_eq!(preview.watch.as_ref().unwrap().speed, speed);
        }
        assert!(!paused(&preview), "choosing a speed keeps the bot playing");
        assert!(preview.pointer.held.is_none(), "no press on the map");
        let gap = [button(0)[0] + SPEED_STEP / 2, button(0)[1]];
        assert!(!preview.autoplay_click(gap), "between two buttons");
    }

    #[test]
    fn autoplay_hud_manual_click_pauses_and_still_acts() {
        let dir = synthetic_corpus();
        let at = [320, 200];
        let mut plain = interactive(dir.path());
        plain.tick(16).unwrap();
        plain.click(at, false);
        assert!(plain.pointer.held.is_some(), "a press on the map");

        let mut preview = interactive(dir.path());
        preview.tick(16).unwrap();
        watching(&mut preview);
        preview.click(at, false);
        assert!(paused(&preview));
        assert!(preview.pointer.held.is_some(), "the press still acted");
    }

    #[test]
    fn autoplay_hud_every_hook_is_inert_without_watch() {
        let dir = synthetic_corpus();
        let mut preview = interactive(dir.path());
        preview.tick(16).unwrap();
        let before = preview.image().unwrap();
        assert!(!preview.autoplay_key(KeyCode::Space));
        assert!(!preview.autoplay_key_up(KeyCode::Space));
        assert!(!preview.autoplay_click(button(1)));
        assert!(!preview.autoplay_swallows(KeyCode::Space, true));
        let mut logged = false;
        assert_eq!(preview.take_autoplay_change(&mut logged), None);
        assert_eq!(preview.image().unwrap().pixels(), before.pixels());
    }

    #[test]
    fn autoplay_hud_the_shell_takes_each_change_once() {
        let dir = synthetic_corpus();
        let mut preview = interactive(dir.path());
        let mut logged = false;
        watching(&mut preview);
        assert_eq!(preview.take_autoplay_change(&mut logged), Some(true));
        assert_eq!(preview.take_autoplay_change(&mut logged), None);
        assert!(preview.autoplay_swallows(KeyCode::Space, true));
        preview.step(KeyCode::Space);
        assert_eq!(preview.take_autoplay_change(&mut logged), Some(false));
        assert_eq!(preview.take_autoplay_change(&mut logged), None);
        assert!(preview.autoplay_swallows(KeyCode::Space, false));
        preview.key_up(KeyCode::Space);
        assert!(!preview.autoplay_swallows(KeyCode::Space, false));
        preview.step(KeyCode::Space);
        assert_eq!(preview.take_autoplay_change(&mut logged), Some(true));
        preview.watch = None;
        assert_eq!(preview.take_autoplay_change(&mut logged), Some(false));
    }

    #[test]
    fn autoplay_hud_banner_dims_the_top_row_only() {
        let dir = synthetic_corpus();
        let mut preview = interactive(dir.path());
        preview.tick(16).unwrap();
        // A flat map, so what shows through is plain to see.
        preview.view_fill = Some(7);
        let before = preview.image().unwrap();
        watching(&mut preview);
        let shown = preview.image().unwrap();
        let black = preview.palette.lookup_rgb5([0, 0, 0]).unwrap();
        assert_ne!(black, 7);
        let row = |image: &IndexedImage, y: usize| image.pixels()[y * 640..][..640].to_vec();
        // Past the buttons and text, three cells in four are solid and the
        // fourth still shows the map.
        let (y, start) = (6, 8 * 75);
        let (old, new) = (row(&before, y), row(&shown, y));
        for x in start..start + 16 {
            let solid = BAYER[y % 4][x % 4] < SOLID_CELLS;
            let want = if solid { black } else { old[x] };
            assert_eq!(new[x], want, "column {x}");
        }
        assert_eq!(&shown.pixels()[12 * 640..], &before.pixels()[12 * 640..]);
    }

    #[test]
    fn autoplay_hud_banner_leaves_the_debugger_its_panel() {
        let dir = synthetic_corpus();
        let mut preview = interactive(dir.path());
        preview.tick(16).unwrap();
        preview.step(KeyCode::F9);
        preview.view_fill = Some(7);
        let before = preview.image().unwrap();
        watching(&mut preview);
        let shown = preview.image().unwrap();
        let right = preview.banner_right();
        assert!(right < FRAME_WIDTH);
        for y in 0..2 * ROW as usize {
            let row = y * 640;
            let tail = row + right as usize..row + 640;
            assert_eq!(shown.pixels()[tail.clone()], before.pixels()[tail]);
        }
        assert_ne!(shown.pixels()[..640], before.pixels()[..640]);
        assert_eq!(
            shown.pixels()[2 * ROW as usize * 640..],
            before.pixels()[2 * ROW as usize * 640..],
        );
    }

    /// The synthetic corpus has no font, so text has no width there.
    #[test]
    #[ignore = "needs the local corpus"]
    fn autoplay_hud_long_text_takes_a_second_row_clear_of_the_touch_pills() {
        let Some(vfs) = local_corpus() else {
            return;
        };
        let mut preview = MapPreview::load(vfs, "maps/v13ent.map", "english", None, 1).unwrap();
        preview.tick(16).unwrap();
        preview.view_fill = Some(7);
        let plain = preview.image().unwrap();
        let below = |image: &IndexedImage, rows: i32| {
            image.pixels()[(rows * ROW) as usize * 640..].to_vec()
        };
        watching(&mut preview);
        assert_eq!(
            below(&preview.image().unwrap(), 1),
            below(&plain, 1),
            "it fits"
        );
        preview.path = "x".repeat(400);
        let wrapped = preview.image().unwrap();
        assert_ne!(below(&wrapped, 1), below(&plain, 1));
        assert_eq!(below(&wrapped, 2), below(&plain, 2));
        preview.touch.on = true;
        let pills = {
            let watch = preview.watch.take();
            let image = preview.image().unwrap();
            preview.watch = watch;
            image
        };
        assert_eq!(below(&preview.image().unwrap(), 1), below(&pills, 1));
    }

    #[test]
    fn autoplay_hud_banner_stays_translucent_on_the_unlocked_view() {
        let dir = synthetic_corpus();
        let mut preview = interactive(dir.path());
        let layout = unlocked(&mut preview, [1280, 960]);
        watching(&mut preview);
        let frame = preview.enhanced_frame(&layout).unwrap().unwrap();
        let clear = frame.ui_transparent.expect("the map shows through");
        let black = preview.palette.lookup_rgb5([0, 0, 0]).unwrap();
        let (y, x) = (6, 8 * 75);
        for x in x..x + 16 {
            let want = if BAYER[y % 4][x % 4] < SOLID_CELLS {
                black
            } else {
                clear
            };
            assert_eq!(frame.ui.pixels()[y * 640 + x], want, "column {x}");
        }
    }

    #[test]
    fn autoplay_hud_unlocked_press_hits_the_button_it_is_on_and_no_other() {
        let dir = synthetic_corpus();
        let mut preview = interactive(dir.path());
        let surface = [1280, 960];
        let layout = unlocked(&mut preview, surface);
        watching(&mut preview);
        // The frame is bottom-centered, so its corner is away from the
        // canvas's: this press is on the frame's 10x button.
        let on_button = button(3);
        let point = [0, 1]
            .map(|axis| f64::from(layout.ui_rect[axis]) + (f64::from(on_button[axis]) + 0.5) * 1.0);
        assert!(press_at(&mut preview, &layout, point));
        let watch = preview.watch.as_ref().unwrap();
        assert_eq!(watch.speed, BotSpeed::X10);
        assert!(!watch.paused);
        // This press's canvas pixel has a button's numbers, but the pointer
        // is far from the banner: it takes control and goes on to the map.
        let canvas = button(0).map(|value| f64::from(value) + 0.5);
        assert!(press_at(&mut preview, &layout, canvas));
        let watch = preview.watch.as_ref().unwrap();
        assert_eq!(watch.speed, BotSpeed::X10);
        assert!(watch.paused);
    }

    #[test]
    fn autoplay_hud_hex_outline_is_the_hex_the_grid_picks() {
        let grid = HexGrid::centered(20100, 640, 380).unwrap();
        let hex = 20100;
        let center = grid.hex_center(hex).unwrap();
        for [dx, dy] in HEX_CORNERS {
            // A little inside each corner.
            let at = [center[0] + dx * 7 / 8, center[1] + dy * 7 / 8].map(|value| value as i32);
            assert_eq!(grid.pick_hex(at), Some(hex), "corner ({dx}, {dy})");
        }
    }

    #[test]
    fn autoplay_hud_route_is_drawn_clipped_and_scaled() {
        let grid = HexGrid::centered(20100, 640, 380).unwrap();
        let colors = [1, 2, 3];
        let count = |image: &IndexedImage, color: u8| {
            image.pixels().iter().filter(|p| **p == color).count()
        };
        let mut thin = IndexedImage::new(640, 380).unwrap();
        // Hexes off the grid are left out, not an error.
        let route = [-1, 20100, 20102, 40000];
        draw_route(&mut thin, grid, &route, Some(20104), 1, colors);
        assert!(count(&thin, 2) > 0, "the route");
        assert!(count(&thin, 3) > 0, "the target");
        let center = grid.hex_center(20100).unwrap();
        let at = (center[1] * 640 + center[0]) as usize;
        assert_eq!(thin.pixels()[at], 2, "the route starts at the hex's center");

        let mut thick = IndexedImage::new(640, 380).unwrap();
        draw_route(&mut thick, grid, &route, Some(20104), 3, colors);
        assert!(
            count(&thick, 3) > count(&thin, 3),
            "strokes grow with scale"
        );

        // A route across the image's corner, and one beyond it: clipped.
        let mut small = IndexedImage::new(8, 8).unwrap();
        line(&mut small, [-40, 4], [40, 4], 2, 9);
        assert!(small.pixels().contains(&9));
        let mut empty = IndexedImage::new(8, 8).unwrap();
        line(&mut empty, [-400, -40], [-40, -400], 2, 9);
        assert!(!empty.pixels().contains(&9));
        draw_route(&mut empty, grid, &[], None, 1, colors);
    }
}
