//! SPEC-014 §3.17-3.18 in game: the options menu, the pause window, the
//! quit confirmation and the preferences screen (knobs, switches, sliders,
//! the speed-up checkbox, Default, Done and Cancel), with preferences applied
//! to the world and written to `fallout.cfg` when a path is configured.
use super::{MapPreview, Screen};
use crate::{AppResult, audio::Volumes, input::key_text, interface::Painter};
use vault_core::ui::{
    BUTTON_PRESS, ColorKey, Rect,
    options::{
        self, DragStep, LabelWidths, MenuKey, OptionsOutcome, OptionsPage, OptionsWindow,
        PreferenceControl, PreferenceKey, PreferenceOutcome, Preferences, PreferencesScreen,
    },
};
use vault_formats::cfg::ConfigFile;
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// SPEC-020 §2.1: the options windows' state is the core's: the menu,
/// pause and quit pages are an [`OptionsWindow`], the preferences a
/// [`PreferencesScreen`].
pub(crate) enum OptionsScreen {
    Window(OptionsWindow),
    Preferences(Box<PreferencesScreen>),
}
impl OptionsScreen {
    pub(crate) const fn page(page: OptionsPage) -> Self {
        Self::Window(OptionsWindow { page })
    }
    /// The pause window is up.
    pub(crate) fn paused(&self) -> bool {
        matches!(
            self,
            Self::Window(OptionsWindow {
                page: OptionsPage::Pause
            })
        )
    }
}
/// SPEC-014 §3.18 the preferences as the rest of the game applies them:
/// the pointer's motion scale, whether combat messages are verbose, and the
/// SPEC-014 §3.46 floating-text lifetime, `base + lines x line`
/// milliseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct AppliedPreferences {
    /// Relative pointer motion is multiplied by this (1.0 through 2.5).
    pub mouse_sensitivity: f64,
    /// `combat_messages` 1, "Verbose"; 0 is "Brief".
    pub verbose_combat_messages: bool,
    /// `trunc(text_base_delay x 1000)`.
    pub floating_base_ms: u64,
    /// `trunc(min(max((text_base_delay - 1) x 0.4, 0), 2) x 1000)`.
    pub floating_line_ms: u64,
}
#[cfg_attr(not(test), allow(dead_code))]
impl AppliedPreferences {
    pub fn of(values: &Preferences) -> Self {
        let base = values.text_base_delay.clamp(1.0, 6.0);
        Self {
            mouse_sensitivity: values.mouse_sensitivity.clamp(1.0, 2.5),
            verbose_combat_messages: values.combat_messages != 0,
            floating_base_ms: vault_core::floating::lifetime(base, 0),
            floating_line_ms: vault_core::floating::lifetime(base, 1)
                - vault_core::floating::lifetime(base, 0),
        }
    }
    /// A floating text of `lines` lines is removed once strictly more than
    /// this many milliseconds have passed since its creation.
    pub fn floating_lifetime_ms(&self, lines: u64) -> u64 {
        self.floating_base_ms
            .saturating_add(lines.saturating_mul(self.floating_line_ms))
    }
}

/// The effects slider's sample sound; the speech slider's narration.
const EFFECTS_SAMPLE: &[u8] = b"butin1";
const SPEECH_SAMPLE: &str = "sound/speech/narrator/options.acm";

/// An `options.msg` label's width in font 101 (the knob and switch hot
/// regions follow their labels).
fn label_width(painter: &Painter, id: i32) -> i32 {
    painter.width(101, &painter.message("options.msg", id))
}
fn menu_key(key: KeyCode, control: bool) -> Option<MenuKey> {
    Some(match key {
        KeyCode::Escape => MenuKey::Escape,
        KeyCode::Enter | KeyCode::NumpadEnter => MenuKey::Enter,
        KeyCode::KeyQ if control => MenuKey::CtrlQ,
        KeyCode::KeyX if control => MenuKey::CtrlX,
        KeyCode::F10 => MenuKey::F10,
        _ => MenuKey::Char(key_text(key, false).filter(u8::is_ascii_lowercase)?.into()),
    })
}
fn preference_key(key: KeyCode, control: bool) -> Option<PreferenceKey> {
    Some(match key {
        KeyCode::Escape => PreferenceKey::Escape,
        KeyCode::KeyQ if control => PreferenceKey::CtrlQ,
        KeyCode::KeyX if control => PreferenceKey::CtrlX,
        KeyCode::F10 => PreferenceKey::F10,
        KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::KeyP => PreferenceKey::Done,
        _ => return None,
    })
}
/// The knob values, read for drawing.
fn knob_values(values: &Preferences) -> [i32; 5] {
    [
        values.game_difficulty,
        values.combat_difficulty,
        values.violence_level,
        values.target_highlight,
        values.running_burning_guy,
    ]
}
/// The switch values, read for drawing.
fn switch_values(values: &Preferences) -> [i32; 6] {
    [
        values.combat_messages,
        values.combat_taunts,
        values.language_filter,
        values.running,
        values.subtitles,
        values.item_highlight,
    ]
}

impl MapPreview {
    pub(crate) fn open_options(&mut self) {
        if matches!(self.screen, Screen::Map) {
            self.screen = Screen::Options(OptionsScreen::page(OptionsPage::Menu));
        }
    }
    pub(crate) fn open_pause(&mut self) {
        if matches!(self.screen, Screen::Map) {
            self.screen = Screen::Options(OptionsScreen::page(OptionsPage::Pause));
        }
    }
    pub(crate) fn open_quit_confirm(&mut self) {
        if matches!(self.screen, Screen::Map | Screen::Options(_)) {
            self.screen = Screen::Options(OptionsScreen::page(OptionsPage::Quit));
        }
    }
    fn open_preferences(&mut self) {
        let values = self.preferences.clone();
        self.screen = Screen::Options(OptionsScreen::Preferences(Box::new(
            PreferencesScreen::open(values),
        )));
    }
    /// M-I6: the applied preferences, for the consumers outside this
    /// window: the shell's pointer-motion scale, the combat message
    /// verbosity and the floating-text layer's lifetimes.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn applied_preferences(&self) -> AppliedPreferences {
        AppliedPreferences::of(&self.preferences)
    }
    /// Apply preferences to the world (the save words included) and the
    /// mixer, and write the configuration file when one is configured.
    pub(crate) fn apply_preferences(&mut self) {
        self.store_preferences();
        // SPEC-009 §2.5 / SPEC-002 §3.7: reapplying preferences
        // re-evaluates the language filter and discards the per-script
        // catalogs.
        if let Err(error) = self.host.reevaluate_language_filter() {
            self.status = format!("language filter: {error}").into_bytes();
        }
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
        // §3.18 / §3.46: reapplying derives the line delay from the base
        // delay, and the derived value is what is saved.
        self.preferences.text_line_delay = options::line_delay(self.preferences.text_base_delay);
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
        world.verbose_combat_messages = values.combat_messages != 0;
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
        let mut view = self.map_view_image()?;
        // §3.17: before the pause window opens, every pixel of the map
        // window (rows 0-380) is converted through the grey lookup table.
        // The bar (and the indicator boxes) are separate windows drawn on
        // top unconverted, so screen rows 0-378 show grey. (The `enhanced`
        // backdrop's flat view stays flat: its world is greyed instead.)
        if matches!(&self.screen, Screen::Options(screen) if screen.paused())
            && self.view_fill.is_none()
        {
            let rows = view.height().min(MAP_WINDOW_ROWS) as usize;
            let width = view.width() as usize;
            for pixel in &mut view.pixels_mut()[..width * rows] {
                *pixel = self.grey[usize::from(*pixel)];
            }
        }
        let mut image = self.compose_world(view);
        let Screen::Options(screen) = &self.screen else {
            return Ok(image);
        };
        let painter = self.painter();
        match screen {
            OptionsScreen::Window(window) => {
                self.draw_options_window(&painter, &mut image, window);
            }
            OptionsScreen::Preferences(state) => {
                self.draw_preferences(&painter, &mut image, state);
            }
        }
        Ok(image)
    }
    /// The options menu, the pause window or the quit confirmation.
    fn draw_options_window(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        window: &OptionsWindow,
    ) {
        match window.page {
            OptionsPage::Menu => {
                let origin = options::OPTIONS_ORIGIN;
                painter.blit_opaque(image, "opbase.frm", 0, origin);
                for (i, (_, rect)) in window.layout().controls().iter().enumerate() {
                    let rect = *rect;
                    let art = ("opbtnoff.frm", "opbtnon.frm");
                    let art = self.draw_button_opaque(painter, image, rect, art);
                    let key = if art == "opbtnon.frm" {
                        ColorKey::LABEL_PRESSED
                    } else {
                        ColorKey::LABEL
                    };
                    let caption = painter.message("options.msg", i as i32);
                    painter.centered(image, 103, key, &caption, [rect.x, rect.y + 8], rect.w);
                }
            }
            OptionsPage::Pause => draw_pause(painter, image, options::PAUSE_ORIGIN),
            OptionsPage::Quit => {
                // §3.17 the large Yes/No box with `misc.msg` 0 in key 32328.
                let title = painter.message("misc.msg", 0);
                let quit = OptionsWindow::quit_box();
                self.draw_dialog_box(
                    painter,
                    image,
                    quit.origin,
                    (&title, &[]),
                    ColorKey::ERROR,
                    quit.yes_no,
                );
            }
            OptionsPage::Preferences => {}
        }
    }
    fn draw_preferences(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        state: &PreferencesScreen,
    ) {
        painter.blit_opaque(image, "prefscrn.frm", 0, [0, 0]);
        let title = painter.message("options.msg", 100);
        painter.text(image, 104, ColorKey::LABEL, &title, [74, 10], 300);
        let covered = self.preferences_covered();
        for (index, (knob, value)) in options::KNOBS
            .iter()
            .zip(knob_values(&state.values))
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
                Rect::new(0, 47 * value, 46, 47),
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
            .zip(switch_values(&state.values))
            .enumerate()
        {
            let caption = painter.message("options.msg", 106 + index as i32);
            let y = [49, 116, 181, 247, 313, 380][index];
            // Switch captions start at x = 206 (knob captions are centered).
            painter.text(image, 103, ColorKey::LABEL, &caption, [206, y], 160);
            let [kx, ky] = switch.origin;
            let frame = switch.frame(value);
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
                _ => ("prfsldof.frm", options::slider_knob(&state.values, index)),
            };
            painter.blit(image, art, 0, [x, *ky]);
        }
        let checkbox = if state.values.player_speedup != 0 {
            "prfxin.frm"
        } else {
            "prfxout.frm"
        };
        painter.blit(image, checkbox, 0, options::CHECKBOX.origin());
        let caption = painter.message("options.msg", 122);
        painter.text(image, 103, ColorKey::LABEL, &caption, [405, 72], 200);
        if covered {
            painter.blit_opaque(image, "prefcvr.frm", 0, [0, 356]);
        }
        for (_, button, caption, id) in options::PREFERENCE_BUTTONS {
            let rect = vault_core::ui::red_button(button);
            let message = ("options.msg", id);
            self.draw_captioned_red_button(painter, image, rect, message, caption, 100);
        }
    }
    /// A slider drag is under way, or a knob or switch change blocks.
    fn preferences_busy(&self) -> bool {
        matches!(&self.screen, Screen::Options(OptionsScreen::Preferences(state))
            if state.busy(self.clock))
    }
    /// The knob and switch labels' widths.
    fn label_widths(&self) -> LabelWidths {
        let painter = self.painter();
        LabelWidths::measure(|id| label_width(&painter, id))
    }
    /// A left release over the preferences: the armed knob or switch acts
    /// when the release is inside its rectangle (any release there marks
    /// "changed", even in a dead zone); a change plays the press cue, then
    /// blocks input for 70 ms and plays the release cue. A drag notes its
    /// release for its next iteration.
    pub(crate) fn options_release(&mut self, point: Option<[i32; 2]>) -> bool {
        let now = self.clock;
        let widths = self.label_widths();
        let Screen::Options(OptionsScreen::Preferences(state)) = &mut self.screen else {
            return false;
        };
        // A drag or a blocking change swallows the release; otherwise only
        // an armed control's release in the frame is handled.
        let handled = state.busy(now) || state.armed().is_some() && point.is_some();
        if let PreferenceOutcome::Changed { cues } = state.release(point, &widths, now) {
            self.play_effect(cues.0.as_bytes(), None);
            self.cue_later(options::CONTROL_CUE_MS, cues.1.as_bytes());
        }
        handled
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
        let Some(step) = state.drag_tick(now, pointer.map(|[x, _]| x)) else {
            return false;
        };
        let (index, iteration) = match step {
            DragStep::Ended { index } => {
                // §3.18 mouse sensitivity takes effect on release.
                if index == 7 {
                    self.preferences.mouse_sensitivity = state.values.mouse_sensitivity;
                }
                self.play_effect(vault_core::ui::BUTTON_RELEASE.as_bytes(), None);
                return true;
            }
            DragStep::Moved {
                index, iteration, ..
            } => (index, iteration),
        };
        let values = state.values.clone();
        // The live preview: brightness and the volumes take effect now
        // (Cancel restores the backup).
        match index {
            // §3.18: the text delay is stored as it changes.
            1 => {
                self.preferences.text_base_delay = values.text_base_delay;
                self.preferences.text_line_delay = values.text_line_delay;
            }
            2 => self.preferences.master_volume = values.master_volume,
            3 => self.preferences.music_volume = values.music_volume,
            4 => self.preferences.sndfx_volume = values.sndfx_volume,
            5 => self.preferences.speech_volume = values.speech_volume,
            6 => self.preferences.brightness = values.brightness,
            _ => {}
        }
        if index == 4 && iteration.is_multiple_of(options::EFFECTS_SAMPLE_EVERY) {
            self.play_effect(EFFECTS_SAMPLE, None);
        }
        // The options narrator is narration (SPEC-010 §3.13): it plays at
        // `trunc(0.69 x speech_volume)` and follows the slider while it
        // plays.
        if index == 5
            && iteration.is_multiple_of(options::SPEECH_SAMPLE_EVERY)
            && let Some(samples) = self.sounds.load(&*self.host.assets.vfs, SPEECH_SAMPLE)
        {
            self.sync_volumes();
            self.audio.play_narration(samples, self.volumes.speech);
        }
        true
    }
    /// Global 603 at its initial 0 covers the fifth knob; SPEC-019 §2.7:
    /// never under Fallout 2.
    fn preferences_covered(&self) -> bool {
        self.host.world.fallout2.is_none()
            && self
                .host
                .world
                .gvars
                .as_ref()
                .and_then(|globals| globals.get(options::COVER_GLOBAL))
                .is_none_or(|value| *value == 0)
    }
    /// A menu action, as its button or key would send it.
    #[cfg(test)]
    fn options_action(&mut self, action: options::OptionsAction) {
        let outcome = OptionsWindow::new(OptionsPage::Menu).dispatch(action);
        self.options_outcome(outcome);
    }
    /// Act on an options window's outcome (SPEC-020 §2.1 rule 4).
    fn options_outcome(&mut self, outcome: OptionsOutcome) {
        match outcome {
            OptionsOutcome::None => {}
            // §3.17: a save or load closes options; a cancel returns here.
            OptionsOutcome::Save => self.save_screen_from_options(true),
            OptionsOutcome::Load => self.save_screen_from_options(false),
            OptionsOutcome::Page(OptionsPage::Preferences) => self.open_preferences(),
            OptionsOutcome::Page(page) => self.screen = Screen::Options(OptionsScreen::page(page)),
            OptionsOutcome::Close => self.screen = Screen::Map,
            OptionsOutcome::Quit(yes) => self.answer_quit(yes),
        }
    }
    fn save_screen_from_options(&mut self, save: bool) {
        use vault_core::ui::saveload::Mode;
        self.screen = Screen::Map;
        self.open_save_screen(if save { Mode::Save } else { Mode::LoadInGame });
        if let Screen::SaveLoad(screen) = &mut self.screen {
            screen.from_options = true;
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
                state.brightness(self.preferences.brightness);
            }
            return true;
        }
        // §3.17 F12 saves a screenshot (and is not an options shortcut).
        if key == KeyCode::F12
            && !matches!(&self.screen, Screen::Options(screen) if screen.paused())
        {
            self.screenshot();
            return true;
        }
        let now = self.clock;
        let Screen::Options(screen) = &mut self.screen else {
            return false;
        };
        match screen {
            OptionsScreen::Window(window) => {
                let page = window.page;
                // The pause window closes on any key, an unmapped one too:
                // it reaches the window as Esc.
                let input = match (menu_key(key, control), page) {
                    (Some(input), _) => input,
                    (None, OptionsPage::Pause) => MenuKey::Escape,
                    (None, _) => return false,
                };
                let (outcome, sound) = window.key(input);
                if outcome == OptionsOutcome::None {
                    return false;
                }
                if sound {
                    self.play_effect(BUTTON_PRESS.as_bytes(), None);
                }
                self.options_outcome(outcome);
            }
            OptionsScreen::Preferences(state) => {
                let Some(input) = preference_key(key, control) else {
                    return false;
                };
                match state.key(input, now) {
                    PreferenceOutcome::Close { keep } => {
                        // Enter or `p` is Done, after `ib1p1xx1`.
                        if keep {
                            self.play_effect(BUTTON_PRESS.as_bytes(), None);
                        }
                        self.close_preferences(keep);
                    }
                    // §3.18 Ctrl+Q, Ctrl+X and F10: the quit confirmation,
                    // whose Yes acts as Cancel.
                    PreferenceOutcome::QuitConfirm => self.confirm_quit(),
                    _ => {}
                }
            }
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
    /// SPEC-019 §3.2: the main menu's Options opens the preferences
    /// directly; closing them leaves the session.
    pub(crate) fn open_preferences_from_menu(&mut self) {
        self.open_preferences();
        self.menu_preferences = true;
    }
    /// This session was opened for the main menu's preferences.
    pub(crate) fn opened_for_preferences(&self) -> bool {
        self.menu_preferences
    }
    /// The preferences opened from the main menu are still up (or their
    /// quit confirmation is).
    pub(crate) fn menu_preferences_open(&self) -> bool {
        self.menu_preferences
            && !self.quit_requested
            && matches!(
                self.screen,
                Screen::Options(
                    OptionsScreen::Preferences(_)
                        | OptionsScreen::Window(OptionsWindow {
                            page: OptionsPage::Quit
                        })
                )
            )
    }
    pub(super) fn close_preferences(&mut self, keep: bool) {
        let after = if self.menu_preferences {
            Screen::Map
        } else {
            Screen::Options(OptionsScreen::page(OptionsPage::Menu))
        };
        let Screen::Options(OptionsScreen::Preferences(state)) =
            std::mem::replace(&mut self.screen, after)
        else {
            return;
        };
        let (values, apply) = state.close(keep);
        self.preferences = values;
        if apply {
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
            OptionsScreen::Window(window) => {
                let page = window.page;
                let layout = window.layout();
                let held = layout.hit(point).and_then(|action| layout.rect(action));
                let outcome = window.click(point);
                // The menu's buttons and the quit box's red buttons show
                // their down art; any press closes the pause window.
                match (page, held) {
                    (OptionsPage::Menu, Some(rect)) => {
                        let (press, release) = options::OPTIONS_SOUNDS;
                        self.press_button_cue(press.as_bytes(), release.as_bytes(), rect);
                    }
                    (OptionsPage::Quit, Some(rect)) => self.press_button(rect),
                    _ => {}
                }
                self.options_outcome(outcome);
            }
            OptionsScreen::Preferences(_) => self.preferences_click(point),
        }
        true
    }
    /// §3.18 DEFAULT reset every preference, game difficulty included; the
    /// volumes, gamma, mouse sensitivity and text delays apply at once
    /// (the live preview a slider drag gives; Cancel still restores the
    /// backup).
    fn preview_defaults(&mut self) {
        let Screen::Options(OptionsScreen::Preferences(state)) = &self.screen else {
            return;
        };
        let values = state.values.clone();
        let live = &mut self.preferences;
        live.master_volume = values.master_volume;
        live.music_volume = values.music_volume;
        live.sndfx_volume = values.sndfx_volume;
        live.speech_volume = values.speech_volume;
        live.brightness = values.brightness;
        live.mouse_sensitivity = values.mouse_sensitivity;
        live.text_base_delay = values.text_base_delay;
        live.text_line_delay = values.text_line_delay;
    }
    /// A press on the preferences: a red button acts at once; G-23 /
    /// §3.18 a knob or switch only arms for its release; a slider strip
    /// starts its modal drag loop with `ib1p1xx1` (the first iteration
    /// runs on the next frame); the checkbox toggles with `ib2p1xx1` (the
    /// reference plays it on the release too, the safe rule `ib2lu1x1`).
    fn preferences_click(&mut self, point: [i32; 2]) {
        let covered = self.preferences_covered();
        let widths = self.label_widths();
        let now = self.clock;
        let Screen::Options(OptionsScreen::Preferences(state)) = &mut self.screen else {
            return;
        };
        let layout = state.layout(&widths, covered);
        let button = match layout.hit(point) {
            Some(control @ PreferenceControl::Button(_)) => layout.rect(control),
            _ => None,
        };
        let outcome = state.press(point, &widths, covered, now);
        if let Some(rect) = button {
            self.press_button(rect);
        }
        match outcome {
            PreferenceOutcome::Defaults => self.preview_defaults(),
            PreferenceOutcome::Close { keep } => self.close_preferences(keep),
            PreferenceOutcome::DragStarted(_) => {
                self.play_effect(BUTTON_PRESS.as_bytes(), None);
            }
            PreferenceOutcome::Toggled => {
                let (press, release) = options::CHECKBOX_SOUNDS;
                self.press_cue(press.as_bytes(), release.as_bytes());
            }
            _ => {}
        }
    }
}

/// §3.17 the map window is 640 by 381; the pause converts rows 0-380.
const MAP_WINDOW_ROWS: u32 = 381;

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

#[cfg(test)]
mod tests;
