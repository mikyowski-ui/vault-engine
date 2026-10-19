//! SPEC-014 §3.17-3.18 in game: the options menu, the pause window, the
//! quit confirmation and the preferences screen (knobs, switches, sliders,
//! the speed-up checkbox, Default, Done and Cancel), with preferences applied
//! to the world and written to `fallout.cfg` when a path is configured.
use super::{MapPreview, Screen};
use crate::{AppResult, interface::Painter};
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
}

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
        })));
    }
    /// Apply preferences to the world (the save words included) and write
    /// the configuration file when one is configured.
    pub(crate) fn apply_preferences(&mut self) {
        let values = self.preferences.clone();
        let world = &mut self.host.world;
        world.difficulty = values.game_difficulty;
        world.combat_difficulty = Some(values.combat_difficulty);
        world.violence = Some(values.violence_level);
        world.running_burning = values.running_burning_guy;
        world.ai.combat_taunts = Some(values.combat_taunts);
        world.ai.language_filter = Some(values.language_filter);
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
                    painter.blit_opaque(&mut image, "opbtnoff.frm", 0, [ox + rect.x, oy + rect.y]);
                    let caption = painter.message("options.msg", i as i32);
                    painter.centered(
                        &mut image,
                        103,
                        ColorKey::LABEL,
                        &caption,
                        [ox + rect.x, oy + rect.y + 8],
                        rect.w,
                    );
                }
            }
            OptionsScreen::Pause => {
                let [ox, oy] = options::PAUSE_ORIGIN;
                painter.blit_opaque(&mut image, "charwin.frm", 0, [ox, oy]);
                painter.blit(&mut image, "donebox.frm", 0, [ox + 13, oy + 42]);
                let done = painter.message("options.msg", 300);
                painter.text(
                    &mut image,
                    103,
                    ColorKey::LABEL,
                    &done,
                    [ox + 52, oy + 45],
                    80,
                );
                let paused = painter.message("options.msg", 301);
                let x = options::paused_title_x(painter.width(104, &paused));
                painter.text(
                    &mut image,
                    104,
                    ColorKey::LABEL,
                    &paused,
                    [ox + x, oy + 10],
                    139,
                );
                painter.blit(&mut image, "lilredup.frm", 0, [ox + 26, oy + 46]);
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
                    painter.blit(&mut image, "lilredup.frm", 0, [ox + rx, oy + ry]);
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
            painter.blit(
                image,
                "prfsldof.frm",
                0,
                [slider_position(&state.values, index), *ky],
            );
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
            painter.blit(image, "lilredup.frm", 0, button);
            let text = painter.message(file, id);
            painter.text(image, 103, ColorKey::LABEL, &text, caption, 100);
        }
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
        let brightness = matches!(
            key,
            KeyCode::Equal | KeyCode::Minus | KeyCode::NumpadAdd | KeyCode::NumpadSubtract
        );
        if brightness {
            self.brightness_step(matches!(key, KeyCode::Equal | KeyCode::NumpadAdd));
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
    fn close_preferences(&mut self, keep: bool) {
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
        let Screen::Options(screen) = &mut self.screen else {
            return false;
        };
        match screen {
            OptionsScreen::Menu => {
                let [ox, oy] = options::OPTIONS_ORIGIN;
                let local = [point[0] - ox, point[1] - oy];
                if let Some(i) = (0..5).find(|i| options::options_button(*i).contains(local)) {
                    self.play_effect(b"ib3p1xx1", None);
                    self.options_action(options::OPTIONS_BUTTONS[i]);
                }
            }
            OptionsScreen::Pause => self.screen = Screen::Map,
            OptionsScreen::Quit => {
                let [ox, oy] = options::QUIT_ORIGIN;
                for button in flow::dialog_buttons(true) {
                    let [rx, ry] = button.button;
                    if Rect::new(ox + rx, oy + ry, 15, 16).contains(point) {
                        self.press();
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
            if Rect::new(button[0], button[1], 15, 16).contains(point) {
                self.press();
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
        for (index, knob) in options::KNOBS.iter().enumerate() {
            if index == 4 && covered {
                continue;
            }
            let value = *knob_values(&mut state.values)[index];
            if let Some((new, up)) = options::knob_click(
                knob,
                &knob_widths[index],
                point,
                value,
                state.knob_up[index],
            ) {
                // A label click that picks the current value still sets
                // the direction.
                state.changed = true;
                *knob_values(&mut state.values)[index] = new;
                state.knob_up[index] = up;
                return;
            }
        }
        for (index, switch) in options::SWITCHES.iter().enumerate() {
            let value = *switch_values(&mut state.values)[index];
            if let Some(new) = options::switch_click(switch, switch_widths[index], point, value) {
                state.changed = true;
                *switch_values(&mut state.values)[index] = new;
                return;
            }
        }
        for (index, ky) in options::SLIDER_KNOB_Y.iter().enumerate() {
            if options::slider_rect(*ky).contains(point) {
                let position =
                    options::slider_drag(slider_position(&state.values, index), point[0]);
                let (_, min, max) = slider_value(&state.values, index);
                let value = if index == 1 {
                    options::delay_value(position)
                } else {
                    options::slider_value(position, min, max)
                };
                set_slider(&mut state.values, index, value);
                state.changed = true;
                return;
            }
        }
        if CHECKBOX.contains(point) {
            state.values.player_speedup ^= 1;
            state.changed = true;
        }
    }
}
