//! SPEC-014 §3.17-3.18 in game: the options menu, the pause window, the
//! quit confirmation and the preferences screen (knobs, switches, sliders,
//! the speed-up checkbox, Default, Done and Cancel), with preferences applied
//! to the world and written to `fallout.cfg` when a path is configured.
use super::{MapPreview, Screen};
use crate::{AppResult, audio::Volumes, interface::Painter};
use vault_core::ui::{
    BUTTON_PRESS, ColorKey, Rect,
    flow::{self, DialogKey},
    options::{self, MenuKey, OptionsAction, Preferences},
};
use vault_formats::cfg::ConfigFile;
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

pub(crate) enum OptionsScreen {
    Menu,
    Pause,
    Quit,
    Preferences(Box<PreferenceState>),
}
pub(crate) struct PreferenceState {
    values: Preferences,
    backup: Preferences,
    changed: bool,
    /// Ping-pong directions of the five knobs (true is up).
    knob_up: [bool; 5],
    /// The knob or switch pressed, evaluated on the release (§3.18).
    armed: Option<Control>,
    /// A slider's modal drag loop.
    drag: Option<SliderDrag>,
    /// §3.18: a knob or switch change blocks for 70 ms between its press
    /// and release cues; input until then is discarded.
    blocked_until: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Knob(usize),
    Switch(usize),
}
/// §3.18 a slider drag: each iteration lasts at least 35 ms; the release is
/// tested before the movement, so a release before the first iteration
/// changes nothing.
#[derive(Debug, Clone, Copy)]
struct SliderDrag {
    index: usize,
    position: i32,
    iteration: u32,
    next: u64,
    released: bool,
}
/// One drag iteration lasts at least 35 ms.
const DRAG_ITERATION_MS: u64 = 35;
/// A knob or switch's release cue follows its press cue after 70 ms, and
/// the screen blocks meanwhile.
const CONTROL_CUE_MS: u64 = 70;
/// The effects slider's sample sound, every 8th iteration; the speech
/// slider's, every 41st.
const EFFECTS_SAMPLE: (&[u8], u32) = (b"butin1", 8);
const SPEECH_SAMPLE: (&str, u32) = ("sound/speech/narrator/options.acm", 41);

/// The preferences screen's red buttons: Default, Done, Cancel.
const PREFERENCE_BUTTONS: [([i32; 2], [i32; 2], i32, &str); 3] = [
    ([23, 450], [43, 449], 120, "options.msg"),
    ([148, 450], [169, 449], 4, "options.msg"),
    ([263, 450], [283, 449], 121, "options.msg"),
];
const CHECKBOX: Rect = Rect::new(383, 68, 18, 18);

fn menu_key(key: KeyCode, control: bool) -> Option<MenuKey> {
    Some(match key {
        KeyCode::Escape => MenuKey::Escape,
        KeyCode::Enter | KeyCode::NumpadEnter => MenuKey::Enter,
        KeyCode::KeyQ if control => MenuKey::CtrlQ,
        KeyCode::KeyX if control => MenuKey::CtrlX,
        KeyCode::F10 => MenuKey::F10,
        _ => {
            let name = format!("{key:?}");
            MenuKey::Char(
                name.strip_prefix("Key")?
                    .chars()
                    .next()?
                    .to_ascii_lowercase(),
            )
        }
    })
}
fn knob_values(values: &mut Preferences) -> [&mut i32; 5] {
    [
        &mut values.game_difficulty,
        &mut values.combat_difficulty,
        &mut values.violence_level,
        &mut values.target_highlight,
        &mut values.running_burning_guy,
    ]
}
fn switch_values(values: &mut Preferences) -> [&mut i32; 6] {
    [
        &mut values.combat_messages,
        &mut values.combat_taunts,
        &mut values.language_filter,
        &mut values.running,
        &mut values.subtitles,
        &mut values.item_highlight,
    ]
}
/// Slider values as reals with their ranges, in caption order.
fn slider_value(values: &Preferences, index: usize) -> (f64, f64, f64) {
    match index {
        0 => (f64::from(values.combat_speed), 0.0, 50.0),
        1 => (values.text_base_delay, 1.0, 6.0),
        2 => (f64::from(values.master_volume), 0.0, 32767.0),
        3 => (f64::from(values.music_volume), 0.0, 32767.0),
        4 => (f64::from(values.sndfx_volume), 0.0, 32767.0),
        5 => (f64::from(values.speech_volume), 0.0, 32767.0),
        6 => (
            values.brightness,
            options::BRIGHTNESS_MIN,
            options::BRIGHTNESS_MAX,
        ),
        _ => (values.mouse_sensitivity, 1.0, 2.5),
    }
}
fn set_slider(values: &mut Preferences, index: usize, value: f64) {
    match index {
        0 => values.combat_speed = value as i32,
        1 => values.text_base_delay = value.clamp(1.0, 6.0),
        2 => values.master_volume = value as i32,
        3 => values.music_volume = value as i32,
        4 => values.sndfx_volume = value as i32,
        5 => values.speech_volume = value as i32,
        6 => values.brightness = value.clamp(options::BRIGHTNESS_MIN, options::BRIGHTNESS_MAX),
        _ => values.mouse_sensitivity = value.clamp(1.0, 2.5),
    }
}
fn slider_position(values: &Preferences, index: usize) -> i32 {
    let (value, min, max) = slider_value(values, index);
    if index == 1 {
        options::delay_position(value)
    } else {
        options::slider_position(value, min, max)
    }
}

impl MapPreview {
    pub(crate) fn open_options(&mut self) {
        if matches!(self.screen, Screen::Map) {
            self.screen = Screen::Options(OptionsScreen::Menu);
        }
    }
    pub(crate) fn open_pause(&mut self) {
        if matches!(self.screen, Screen::Map) {
            self.screen = Screen::Options(OptionsScreen::Pause);
        }
    }
    pub(crate) fn open_quit_confirm(&mut self) {
        if matches!(self.screen, Screen::Map | Screen::Options(_)) {
            self.screen = Screen::Options(OptionsScreen::Quit);
        }
    }
    fn open_preferences(&mut self) {
        let values = self.preferences.clone();
        self.screen = Screen::Options(OptionsScreen::Preferences(Box::new(PreferenceState {
            backup: values.clone(),
            values,
            changed: false,
            knob_up: [true; 5],
            armed: None,
            drag: None,
            blocked_until: 0,
        })));
    }
    /// Apply preferences to the world (the save words included) and the
    /// mixer, and write the configuration file when one is configured.
    pub(crate) fn apply_preferences(&mut self) {
        self.store_preferences();
        // SPEC-014 §3.18: reapplying sets the volumes, so an F2/F3 master
        // offset (which never reaches the preference) does not survive it.
        let volumes = Volumes::from_preferences(&self.preferences);
        self.volume_source = Some(volumes);
        self.apply_volumes(volumes);
    }
    /// Store preferences in the world (the save words included) and write
    /// the configuration file when one is configured. A brightness hotkey
    /// does only this (and applies its gamma): it reapplies no volume.
    pub(crate) fn store_preferences(&mut self) {
        let values = self.preferences.clone();
        let world = &mut self.host.world;
        world.difficulty = values.game_difficulty;
        world.combat_difficulty = Some(values.combat_difficulty);
        world.violence = Some(values.violence_level);
        world.running_burning = values.running_burning_guy;
        world.ai.combat_taunts = Some(values.combat_taunts);
        world.ai.language_filter = Some(values.language_filter);
        world.combat_speed = values.combat_speed;
        world.player_speedup = values.player_speedup != 0;
        world.persistence.preferences = values.to_words();
        if let Some(path) = &self.config_path {
            let mut config = std::fs::read_to_string(path)
                .map(|text| ConfigFile::parse(&text))
                .unwrap_or_default();
            values.write_config(&mut config);
            if let Err(error) = std::fs::write(path, config.write()) {
                self.status = format!("{}: {error}", path.display()).into_bytes();
            }
        }
    }
    pub(crate) fn options_image(&mut self) -> AppResult<IndexedImage> {
        let view = self.map_view_image()?;
        let mut image = self.compose_world(view);
        let Screen::Options(screen) = &self.screen else {
            return Ok(image);
        };
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        match screen {
            OptionsScreen::Menu => {
                let [ox, oy] = options::OPTIONS_ORIGIN;
                painter.blit_opaque(&mut image, "opbase.frm", 0, [ox, oy]);
                for i in 0..5 {
                    let rect = options::options_button(i);
                    let rect = Rect::new(ox + rect.x, oy + rect.y, rect.w, rect.h);
                    let art = self.button_art(rect, "opbtnoff.frm", "opbtnon.frm");
                    painter.blit_opaque(&mut image, art, 0, [rect.x, rect.y]);
                    let key = if art == "opbtnon.frm" {
                        ColorKey::LABEL_PRESSED
                    } else {
                        ColorKey::LABEL
                    };
                    let caption = painter.message("options.msg", i as i32);
                    painter.centered(&mut image, 103, key, &caption, [rect.x, rect.y + 8], rect.w);
                }
            }
            OptionsScreen::Pause => {
                // G-24 / §3.17: the game window (the map view above the bar)
                // is converted through the grey lookup table first.
                let grey = vault_core::ui::grey_table(self.game_palette());
                for pixel in &mut image.pixels_mut()[..640 * bar_top()] {
                    *pixel = grey[usize::from(*pixel)];
                }
                draw_pause(&painter, &mut image, options::PAUSE_ORIGIN);
            }
            OptionsScreen::Quit => {
                let [ox, oy] = options::QUIT_ORIGIN;
                let (art, [width, _]) = flow::DialogSize::Large.art();
                painter.blit_opaque(&mut image, art, 0, [ox, oy]);
                let title = painter.message("misc.msg", 0);
                painter.centered(
                    &mut image,
                    101,
                    ColorKey::ERROR,
                    &title,
                    [ox, oy + flow::dialog_title_y(0)],
                    width,
                );
                for button in flow::dialog_buttons(true) {
                    let [bx, by] = button.box_origin;
                    if button.transparent_box {
                        painter.blit(&mut image, flow::DONE_BOX.0, 0, [ox + bx, oy + by]);
                    } else {
                        painter.blit_opaque(&mut image, flow::DONE_BOX.0, 0, [ox + bx, oy + by]);
                    }
                    let caption = painter.message("dbox.msg", button.caption);
                    let [cx, cy] = button.caption_origin;
                    painter.text(
                        &mut image,
                        103,
                        ColorKey::LABEL,
                        &caption,
                        [ox + cx, oy + cy],
                        60,
                    );
                    let [rx, ry] = button.button;
                    let rect = Rect::new(ox + rx, oy + ry, 15, 16);
                    let art = self.button_art(rect, "lilredup.frm", "lilreddn.frm");
                    painter.blit(&mut image, art, 0, [rect.x, rect.y]);
                }
            }
            OptionsScreen::Preferences(state) => {
                self.draw_preferences(&painter, &mut image, state);
            }
        }
        Ok(image)
    }
    fn draw_preferences(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        state: &PreferenceState,
    ) {
        painter.blit_opaque(image, "prefscrn.frm", 0, [0, 0]);
        let title = painter.message("options.msg", 100);
        painter.text(image, 104, ColorKey::LABEL, &title, [74, 10], 300);
        let covered = self.preferences_covered();
        let mut values = state.values.clone();
        for (index, (knob, value)) in options::KNOBS
            .iter()
            .zip(knob_values(&mut values))
            .enumerate()
        {
            if index == 4 && covered {
                continue;
            }
            let caption = painter.message("options.msg", 101 + index as i32);
            let y = [48, 125, 203, 286, 363][index];
            painter.centered(image, 103, ColorKey::LABEL, &caption, [99 - 80, y], 160);
            let [kx, ky] = knob.origin;
            painter.blit_region(
                image,
                "prfbknbs.frm",
                Rect::new(0, 47 * *value, 46, 47),
                [kx, ky],
                false,
            );
            for (i, id) in knob.labels.iter().enumerate() {
                let label = painter.message("options.msg", *id);
                let origin = knob.label_origin(i, painter.width(101, &label));
                painter.text(image, 101, ColorKey::LABEL, &label, origin, 120);
            }
        }
        for (index, (switch, value)) in options::SWITCHES
            .iter()
            .zip(switch_values(&mut values))
            .enumerate()
        {
            let caption = painter.message("options.msg", 106 + index as i32);
            let y = [49, 116, 181, 247, 313, 380][index];
            // Switch captions start at x = 206 (knob captions are centered).
            painter.text(image, 103, ColorKey::LABEL, &caption, [206, y], 160);
            let [kx, ky] = switch.origin;
            let frame = switch.frame(*value);
            painter.blit_region(
                image,
                "prflknbs.frm",
                Rect::new(0, 25 * frame, 22, 25),
                [kx, ky],
                false,
            );
            for (i, id) in switch.labels.iter().enumerate() {
                let label = painter.message("options.msg", *id);
                let x = if i == 0 {
                    kx + 4 - painter.width(101, &label)
                } else {
                    kx + 21
                };
                painter.text(image, 101, ColorKey::LABEL, &label, [x, ky - 5], 80);
            }
        }
        for (index, ky) in options::SLIDER_KNOB_Y.iter().enumerate() {
            let caption = painter.message("options.msg", 112 + index as i32);
            let y = [19, 94, 165, 216, 268, 319, 369, 420][index];
            painter.text(image, 103, ColorKey::LABEL, &caption, [384, y], 240);
            // The dragged knob shows `prfsldon.frm` at its drag position.
            let (art, x) = match state.drag {
                Some(drag) if drag.index == index => ("prfsldon.frm", drag.position),
                _ => ("prfsldof.frm", slider_position(&state.values, index)),
            };
            painter.blit(image, art, 0, [x, *ky]);
        }
        let checkbox = if state.values.player_speedup != 0 {
            "prfxin.frm"
        } else {
            "prfxout.frm"
        };
        painter.blit(image, checkbox, 0, [CHECKBOX.x, CHECKBOX.y]);
        let caption = painter.message("options.msg", 122);
        painter.text(image, 103, ColorKey::LABEL, &caption, [405, 72], 200);
        if covered {
            painter.blit_opaque(image, "prefcvr.frm", 0, [0, 356]);
        }
        for (button, caption, id, file) in PREFERENCE_BUTTONS {
            let rect = Rect::new(button[0], button[1], 15, 16);
            painter.blit(
                image,
                self.button_art(rect, "lilredup.frm", "lilreddn.frm"),
                0,
                button,
            );
            let text = painter.message(file, id);
            painter.text(image, 103, ColorKey::LABEL, &text, caption, 100);
        }
    }
    /// A slider drag is under way, or a knob or switch change blocks.
    fn preferences_busy(&self) -> bool {
        matches!(&self.screen, Screen::Options(OptionsScreen::Preferences(state))
            if state.drag.is_some() || self.clock < state.blocked_until)
    }
    /// A left release over the preferences: the armed knob or switch acts
    /// when the release is inside its rectangle (any release there marks
    /// "changed", even in a dead zone); a change plays the press cue, then
    /// blocks input for 70 ms and plays the release cue. A drag notes its
    /// release for its next iteration.
    pub(crate) fn options_release(&mut self, point: Option<[i32; 2]>) -> bool {
        let now = self.clock;
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        let width = |id: i32| painter.width(101, &painter.message("options.msg", id));
        let Screen::Options(OptionsScreen::Preferences(state)) = &mut self.screen else {
            return false;
        };
        if let Some(drag) = &mut state.drag {
            drag.released = true;
            return true;
        }
        if now < state.blocked_until {
            return true;
        }
        let (Some(control), Some(point)) = (state.armed.take(), point) else {
            return false;
        };
        let cues: (&'static [u8], &'static [u8]) = match control {
            Control::Knob(index) => {
                let knob = &options::KNOBS[index];
                let widths: Vec<i32> = knob.labels.iter().map(|id| width(*id)).collect();
                let value = *knob_values(&mut state.values)[index];
                let Some((new, up)) =
                    options::knob_click(knob, &widths, point, value, state.knob_up[index])
                else {
                    return true;
                };
                state.changed = true;
                state.knob_up[index] = up;
                if new == value {
                    return true;
                }
                *knob_values(&mut state.values)[index] = new;
                (b"ib3p1xx1", b"ib3lu1x1")
            }
            Control::Switch(index) => {
                let switch = &options::SWITCHES[index];
                let widths = switch.labels.map(width);
                let value = *switch_values(&mut state.values)[index];
                let Some(new) = options::switch_click(switch, widths, point, value) else {
                    return true;
                };
                state.changed = true;
                if new == value {
                    return true;
                }
                *switch_values(&mut state.values)[index] = new;
                (b"ib2p1xx1", b"ib2lu1x1")
            }
        };
        state.blocked_until = now + CONTROL_CUE_MS;
        self.play_effect(cues.0, None);
        self.cue_later(CONTROL_CUE_MS, cues.1);
        true
    }
    /// One frame of the options window: a slider drag's iteration when due.
    /// Each iteration tests the release first (`ib1lu1x1`, the idle knob,
    /// and mouse sensitivity takes effect), then follows the pointer with
    /// the hysteresis of `options::slider_drag`. Volumes and brightness
    /// apply at once (the live preview), the text delay is stored as it
    /// changes; the effects slider samples `butin1` on the first and every
    /// 8th iteration, the speech slider the narrator's `options.acm` on the
    /// first and every 41st.
    pub(crate) fn options_tick(&mut self, now: u64) -> bool {
        let pointer = self.pointer.at;
        let Screen::Options(OptionsScreen::Preferences(state)) = &mut self.screen else {
            return false;
        };
        let Some(drag) = &mut state.drag else {
            return false;
        };
        if now < drag.next {
            return false;
        }
        if drag.released {
            state.drag = None;
            self.play_effect(vault_core::ui::BUTTON_RELEASE.as_bytes(), None);
            return true;
        }
        drag.next = now + DRAG_ITERATION_MS;
        let iteration = drag.iteration;
        drag.iteration += 1;
        let index = drag.index;
        if let Some([x, _]) = pointer {
            drag.position = options::slider_drag(drag.position, x);
        }
        let position = drag.position;
        let (old, min, max) = slider_value(&state.values, index);
        let value = if index == 1 {
            options::delay_value(position)
        } else {
            options::slider_value(position, min, max)
        };
        set_slider(&mut state.values, index, value);
        let moved = slider_value(&state.values, index).0 != old;
        state.changed |= moved;
        let values = state.values.clone();
        // The live preview: brightness and the volumes take effect now
        // (Cancel restores the backup).
        match index {
            2 => self.preferences.master_volume = values.master_volume,
            3 => self.preferences.music_volume = values.music_volume,
            4 => self.preferences.sndfx_volume = values.sndfx_volume,
            5 => self.preferences.speech_volume = values.speech_volume,
            6 => self.preferences.brightness = values.brightness,
            _ => {}
        }
        if index == 4 && iteration.is_multiple_of(EFFECTS_SAMPLE.1) {
            self.play_effect(EFFECTS_SAMPLE.0, None);
        }
        if index == 5
            && iteration.is_multiple_of(SPEECH_SAMPLE.1)
            && let Some(samples) = self.sounds.load(&*self.host.assets.vfs, SPEECH_SAMPLE.0)
        {
            self.audio
                .play(samples, 1.0, crate::audio::Channel::Speech, false);
        }
        true
    }
    /// Global 603 at its initial 0 covers the fifth knob.
    fn preferences_covered(&self) -> bool {
        self.host
            .world
            .gvars
            .as_ref()
            .and_then(|globals| globals.get(options::COVER_GLOBAL))
            .is_none_or(|value| *value == 0)
    }
    fn options_action(&mut self, action: OptionsAction) {
        match action {
            OptionsAction::SaveGame => {
                self.screen = Screen::Map;
                self.open_save_screen(vault_core::ui::saveload::Mode::Save);
            }
            OptionsAction::LoadGame => {
                self.screen = Screen::Map;
                self.open_save_screen(vault_core::ui::saveload::Mode::LoadInGame);
            }
            OptionsAction::Preferences => self.open_preferences(),
            OptionsAction::ExitGame => self.screen = Screen::Options(OptionsScreen::Quit),
            OptionsAction::Done => self.screen = Screen::Map,
        }
    }
    pub(crate) fn options_key(&mut self, key: KeyCode) -> bool {
        let control = self.hud.modifiers[1];
        // A slider drag is modal and a knob change blocks: other input is
        // ignored.
        if self.preferences_busy() {
            return true;
        }
        let brightness = matches!(
            key,
            KeyCode::Equal | KeyCode::Minus | KeyCode::NumpadAdd | KeyCode::NumpadSubtract
        );
        if brightness {
            let up = matches!(key, KeyCode::Equal | KeyCode::NumpadAdd);
            self.brightness_step(up);
            // G-31 / §3.18 `[May-improve]`: inside preferences the hotkey
            // also moves the slider and joins the cancellable state, so
            // Done keeps it and Cancel restores the old gamma (and file).
            if let Screen::Options(OptionsScreen::Preferences(state)) = &mut self.screen {
                state.values.brightness = self.preferences.brightness;
                state.changed = true;
            }
            return true;
        }
        // §3.17 F12 saves a screenshot (and is not an options shortcut).
        if key == KeyCode::F12 && !matches!(self.screen, Screen::Options(OptionsScreen::Pause)) {
            self.screenshot();
            return true;
        }
        let Screen::Options(screen) = &self.screen else {
            return false;
        };
        match screen {
            OptionsScreen::Menu => {
                let Some((action, sound)) = menu_key(key, control).and_then(options::options_key)
                else {
                    return false;
                };
                if sound {
                    self.play_effect(BUTTON_PRESS.as_bytes(), None);
                }
                self.options_action(action);
            }
            OptionsScreen::Pause => self.screen = Screen::Map,
            OptionsScreen::Quit => {
                let dialog = match key {
                    KeyCode::Enter | KeyCode::NumpadEnter => DialogKey::Enter,
                    KeyCode::Escape => DialogKey::Escape,
                    KeyCode::KeyY => DialogKey::Char('y'),
                    KeyCode::KeyN => DialogKey::Char('n'),
                    _ => return false,
                };
                if let Some(yes) = flow::dialog_answer(dialog, true) {
                    self.answer_quit(yes);
                }
            }
            OptionsScreen::Preferences(_) => match key {
                KeyCode::Escape => self.close_preferences(false),
                // §3.18 Ctrl+Q, Ctrl+X and F10: the quit confirmation, whose
                // Yes acts as Cancel.
                KeyCode::KeyQ | KeyCode::KeyX if control => self.confirm_quit(),
                KeyCode::F10 => self.confirm_quit(),
                KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::KeyP => {
                    self.play_effect(BUTTON_PRESS.as_bytes(), None);
                    self.close_preferences(true);
                }
                _ => return false,
            },
        }
        true
    }
    /// Yes raises a return-to-main-menu request; it never exits the program.
    fn answer_quit(&mut self, yes: bool) {
        self.screen = Screen::Map;
        if yes {
            self.quit_requested = true;
        }
    }
    pub(super) fn close_preferences(&mut self, keep: bool) {
        let Screen::Options(OptionsScreen::Preferences(state)) =
            std::mem::replace(&mut self.screen, Screen::Options(OptionsScreen::Menu))
        else {
            return;
        };
        self.preferences = if keep { state.values } else { state.backup };
        if keep && state.changed || !keep {
            self.apply_preferences();
        }
    }
    pub(crate) fn options_click(&mut self, point: [i32; 2]) -> bool {
        if self.preferences_busy() {
            return true;
        }
        let Screen::Options(screen) = &mut self.screen else {
            return false;
        };
        match screen {
            OptionsScreen::Menu => {
                let [ox, oy] = options::OPTIONS_ORIGIN;
                let local = [point[0] - ox, point[1] - oy];
                if let Some(i) = (0..5).find(|i| options::options_button(*i).contains(local)) {
                    let rect = options::options_button(i);
                    self.press_button_cue(
                        b"ib3p1xx1",
                        b"ib3lu1x1",
                        Rect::new(ox + rect.x, oy + rect.y, rect.w, rect.h),
                    );
                    self.options_action(options::OPTIONS_BUTTONS[i]);
                }
            }
            OptionsScreen::Pause => self.screen = Screen::Map,
            OptionsScreen::Quit => {
                let [ox, oy] = options::QUIT_ORIGIN;
                for button in flow::dialog_buttons(true) {
                    let [rx, ry] = button.button;
                    let rect = Rect::new(ox + rx, oy + ry, 15, 16);
                    if rect.contains(point) {
                        self.press_button(rect);
                        self.answer_quit(button.caption == 101);
                        break;
                    }
                }
            }
            OptionsScreen::Preferences(_) => self.preferences_click(point),
        }
        true
    }
    fn preferences_click(&mut self, point: [i32; 2]) {
        let covered = self.preferences_covered();
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        let knob_widths: Vec<Vec<i32>> = options::KNOBS
            .iter()
            .map(|knob| {
                knob.labels
                    .iter()
                    .map(|id| painter.width(101, &painter.message("options.msg", *id)))
                    .collect()
            })
            .collect();
        let switch_widths: Vec<[i32; 2]> = options::SWITCHES
            .iter()
            .map(|switch| {
                switch
                    .labels
                    .map(|id| painter.width(101, &painter.message("options.msg", id)))
            })
            .collect();
        for (index, (button, ..)) in PREFERENCE_BUTTONS.iter().enumerate() {
            let rect = Rect::new(button[0], button[1], 15, 16);
            if rect.contains(point) {
                self.press_button(rect);
                match index {
                    0 => {
                        if let Screen::Options(OptionsScreen::Preferences(state)) = &mut self.screen
                        {
                            state.values = Preferences::default();
                            state.changed = true;
                        }
                    }
                    1 => self.close_preferences(true),
                    _ => self.close_preferences(false),
                }
                return;
            }
        }
        let Screen::Options(OptionsScreen::Preferences(state)) = &mut self.screen else {
            return;
        };
        // G-23 / §3.18: knobs and switches act on the release inside their
        // rectangle; the press only arms them.
        state.armed = options::KNOBS
            .iter()
            .enumerate()
            .filter(|(index, _)| !(*index == 4 && covered))
            .find(|(index, knob)| knob.rect(&knob_widths[*index]).contains(point))
            .map(|(index, _)| Control::Knob(index))
            .or_else(|| {
                options::SWITCHES
                    .iter()
                    .enumerate()
                    .find(|(index, switch)| switch.rect(switch_widths[*index]).contains(point))
                    .map(|(index, _)| Control::Switch(index))
            });
        if state.armed.is_some() {
            return;
        }
        // A press in a slider's strip starts its modal drag loop with
        // `ib1p1xx1` and the active knob; the first iteration runs on the
        // next frame.
        if let Some(index) = options::SLIDER_KNOB_Y
            .iter()
            .position(|ky| options::slider_rect(*ky).contains(point))
        {
            state.drag = Some(SliderDrag {
                index,
                position: slider_position(&state.values, index),
                iteration: 0,
                next: self.clock,
                released: false,
            });
            self.play_effect(BUTTON_PRESS.as_bytes(), None);
            return;
        }
        // The checkbox toggles on the press with `ib2p1xx1`; the reference
        // plays `ib2p1xx1` on the release too, the safe rule `ib2lu1x1`.
        if CHECKBOX.contains(point) {
            state.values.player_speedup ^= 1;
            state.changed = true;
            self.press_cue(b"ib2p1xx1", b"ib2lu1x1");
        }
    }
}

/// The bar window's top row: the game window above it is what the pause
/// greys (the bar is a separate window).
fn bar_top() -> usize {
    vault_core::ui::bar::BAR_ORIGIN[1] as usize
}

/// §3.17 the pause window at `origin`: `charwin.frm`, `donebox.frm` drawn
/// transparently at (13, 42), DONE (`options.msg` 300) at (52, 45) in font
/// 103, PAUSED (301) in font 104 centered at y = 10, and the red button at
/// (26, 46).
pub(super) fn draw_pause(painter: &Painter, image: &mut IndexedImage, origin: [i32; 2]) {
    let [ox, oy] = origin;
    painter.blit_opaque(image, "charwin.frm", 0, [ox, oy]);
    painter.blit(image, "donebox.frm", 0, [ox + 13, oy + 42]);
    let done = painter.message("options.msg", 300);
    painter.text(image, 103, ColorKey::LABEL, &done, [ox + 52, oy + 45], 80);
    let paused = painter.message("options.msg", 301);
    let x = options::paused_title_x(painter.width(104, &paused));
    painter.text(image, 104, ColorKey::LABEL, &paused, [ox + x, oy + 10], 139);
    painter.blit(image, "lilredup.frm", 0, [ox + 26, oy + 46]);
}
