//! SPEC-014 §3.17-3.18 in game: the options menu, the pause window, the
//! quit confirmation and the preferences screen (knobs, switches, sliders,
//! the speed-up checkbox, Default, Done and Cancel), with preferences applied
//! to the world and written to `fallout.cfg` when a path is configured.
//!
//! **The Enhanced window** (engine extension, not part of any specification).
//! Under the `enhanced` preset the options menu gains one button below its
//! plate, which opens a window of built-in presets and single settings. Under
//! Vanilla ([`MapPreview::enhanced_entry_shown`] is false) none of it draws or
//! takes input, so SPEC-014's windows are as they were.
//! - **Applying.** A click applies at once: the viewport, zoom, smooth camera,
//!   roof peek, interface scale, tactical overlay and game speed all reach the
//!   session. The zoom, camera, roof and scale rows do nothing in the fixed
//!   viewport, and the window says so. The Vanilla or Enhanced rules cannot
//!   change here: they decide tables, scripts and save sidecars as a world
//!   loads, so that row only names what the next launch runs.
//! - **Replays.** Clicks are logged input, and a replay's clicks reach this
//!   window as a preferences change does, so nothing here is a record of its
//!   own. What a frame shows comes from the session, never from a file.
//! - **Saving.** Save writes the active profile's table of `vault-engine.toml`
//!   ([`crate::config::save_profile_enhanced`]), a different file from
//!   `fallout.cfg`. A headless or replay session writes nothing, and a file
//!   whose active profile is another game's is refused: that save would go
//!   into the wrong table.
use super::{MapPreview, Screen};
use crate::{
    AppResult,
    audio::Volumes,
    config::{self, Config, EnhancedProfile, Preset, UiScale},
    enhanced::{BuiltinPreset, CameraFollow, EnhancedSettings, GameSpeed, RoofPeek, ViewportMode},
    input::key_text,
    interface::Painter,
};
use std::path::{Path, PathBuf};
use vault_core::{
    GameProfile,
    movie::MoviePolicy,
    ui::{
        BUTTON_PRESS, ColorKey, Rect,
        options::{
            self, DragStep, LabelWidths, MenuKey, OptionsOutcome, OptionsPage, OptionsWindow,
            PreferenceControl, PreferenceKey, PreferenceOutcome, Preferences, PreferencesScreen,
        },
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
    /// The `enhanced` preset's window of presets and settings.
    Enhanced(EnhancedPanel),
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
                if self.enhanced_entry_shown() {
                    self.draw_enhanced_entry(&painter, &mut image);
                }
            }
            OptionsScreen::Preferences(state) => {
                self.draw_preferences(&painter, &mut image, state);
            }
            OptionsScreen::Enhanced(panel) => {
                self.draw_enhanced_panel(&painter, &mut image, panel);
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
        // E is Exit Game (§3.17), so the window takes the word's next letter.
        if key == KeyCode::KeyH && self.enhanced_entry_shown() {
            self.play_effect(BUTTON_PRESS.as_bytes(), None);
            self.open_enhanced_panel();
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
            OptionsScreen::Enhanced(_) => return self.enhanced_panel_key(key),
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
        if self.enhanced_entry_shown() && ENHANCED_ENTRY.contains(point) {
            let (press, release) = options::OPTIONS_SOUNDS;
            self.press_button_cue(press.as_bytes(), release.as_bytes(), ENHANCED_ENTRY);
            self.open_enhanced_panel();
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
            OptionsScreen::Enhanced(_) => self.enhanced_panel_click(point),
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

/// The options menu's Enhanced button, below its plate: the size and art of
/// the five buttons on it.
const ENHANCED_ENTRY: Rect = Rect::new(251, 290, 137, 34);
/// The Enhanced window, over the map rows. A dimmed backdrop keeps the bar
/// below it, so it stops short of row 380.
const PANEL: Rect = Rect::new(54, 6, 532, 368);
const PRESET_Y: i32 = 34;
const PRESET_SIZE: [i32; 2] = [164, 22];
const PRESET_GAP: i32 = 8;
const ROW_Y: i32 = 62;
const ROW_HEIGHT: i32 = 25;
const ROW_X: i32 = 66;
const VALUE: Rect = Rect::new(260, 0, 130, 21);
const TAG_X: i32 = 398;
const BADGE: Rect = Rect::new(482, 0, 92, 22);
const NOTE_Y: i32 = 318;
const BUTTON_Y: i32 = 340;
const SAVE_AT: [i32; 2] = [66, BUTTON_Y];
const DONE_AT: [i32; 2] = [176, BUTTON_Y];
/// A red button's caption, right of the button.
const CAPTION_DX: i32 = 20;
/// The stops a click on the zoom and interface scale rows steps through.
const ZOOM_STOPS: [f32; 6] = [0.5, 0.75, 1.0, 1.5, 2.0, 3.0];
const UI_SCALE_STOPS: [f32; 4] = [1.0, 1.5, 2.0, 3.0];
/// The recentre margins a click steps through, as the launcher's row.
const MARGIN_STOPS: [u16; 5] = crate::launcher::MARGIN_STEPS;

/// A row of the Enhanced window, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnhancedRow {
    Viewport,
    Zoom,
    Smooth,
    /// `camera_follow`.
    Follow,
    /// `camera_recenter_margin`.
    Margin,
    Roof,
    UiScale,
    Speed,
    Overlay,
    Rules,
}

impl EnhancedRow {
    const ALL: [Self; 10] = [
        Self::Viewport,
        Self::Zoom,
        Self::Smooth,
        Self::Follow,
        Self::Margin,
        Self::Roof,
        Self::UiScale,
        Self::Speed,
        Self::Overlay,
        Self::Rules,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Viewport => "Viewport",
            Self::Zoom => "Default zoom",
            Self::Smooth => "Smooth camera",
            Self::Follow => "Camera follow",
            Self::Margin => "Recentre margin",
            Self::Roof => "Roof peek",
            Self::UiScale => "Interface scale",
            Self::Speed => "Game speed",
            Self::Overlay => "Tactical overlay",
            Self::Rules => "Game rules",
        }
    }
    fn hint(self) -> &'static str {
        match self {
            Self::Viewport => "The world across the window",
            Self::Zoom => "The zoom a map starts at",
            Self::Smooth => "Camera moves ease in and out",
            Self::Follow => "How the view follows you",
            Self::Margin => "Edge room before it recentres",
            Self::Roof => "Roofs open at the cursor",
            Self::UiScale => "How large the interface draws",
            Self::Speed => "World time against the clock",
            Self::Overlay => "HP bars and to-hit in combat",
            Self::Rules => "Vanilla or Enhanced play",
        }
    }
    /// The row needs the unlocked view to show anything.
    fn unlocked_only(self) -> bool {
        matches!(self, Self::Zoom | Self::Smooth | Self::Roof | Self::UiScale)
    }
    /// The screen rectangle of the row's value, which a click steps.
    fn value_rect(self) -> Rect {
        let index = Self::ALL.iter().position(|row| *row == self).unwrap_or(0);
        VALUE.offset([0, ROW_Y + ROW_HEIGHT * index as i32 + 2])
    }
    /// The screen rectangle of the row's preview badge.
    fn badge_rect(self) -> Rect {
        let index = Self::ALL.iter().position(|row| *row == self).unwrap_or(0);
        BADGE.offset([0, ROW_Y + ROW_HEIGHT * index as i32 + 2])
    }
}

/// What the session runs, as the window shows and saves it. It comes from the
/// session alone, so a replay draws the frame the desktop drew.
#[derive(Debug, Clone, Copy, PartialEq)]
struct EnhancedValues {
    settings: EnhancedSettings,
    speed: GameSpeed,
    overlay: bool,
    /// The rules the next launch runs.
    rules: Preset,
}

impl EnhancedValues {
    /// These values with what `profile` names over them.
    fn with_profile(self, profile: EnhancedProfile) -> Self {
        let settings = self.settings;
        Self {
            settings: EnhancedSettings {
                viewport: profile.viewport_mode.unwrap_or(settings.viewport),
                default_zoom: profile.default_zoom.unwrap_or(settings.default_zoom),
                smooth_camera: profile.smooth_camera.unwrap_or(settings.smooth_camera),
                roof_peek: profile.roof_peek.unwrap_or(settings.roof_peek),
                ui_scale: profile.ui_scale.map_or(settings.ui_scale, UiScale::factor),
                camera_follow: profile.camera_follow.unwrap_or(settings.camera_follow),
                camera_recenter_margin: profile
                    .camera_recenter_margin
                    .unwrap_or(settings.camera_recenter_margin),
                ..settings
            },
            speed: profile.game_speed.unwrap_or(self.speed),
            overlay: profile.tactical_overlay.unwrap_or(self.overlay),
            rules: profile.preset.unwrap_or(self.rules),
        }
    }
    /// These values with `row` stepped to its next choice.
    fn stepped(mut self, row: EnhancedRow) -> Self {
        let settings = &mut self.settings;
        match row {
            EnhancedRow::Viewport => *settings = crate::replay::toggled_viewport(*settings),
            EnhancedRow::Zoom => {
                settings.default_zoom =
                    next_stop(&ZOOM_STOPS, settings.default_zoom).unwrap_or(ZOOM_STOPS[0]);
            }
            EnhancedRow::Smooth => settings.smooth_camera = !settings.smooth_camera,
            EnhancedRow::Follow => settings.camera_follow = settings.camera_follow.next(),
            EnhancedRow::Margin => {
                let margin = settings.camera_recenter_margin;
                settings.camera_recenter_margin = MARGIN_STOPS
                    .into_iter()
                    .find(|stop| *stop > margin)
                    .unwrap_or(MARGIN_STOPS[0]);
            }
            EnhancedRow::Roof => settings.roof_peek = settings.roof_peek.next(),
            EnhancedRow::UiScale => {
                settings.ui_scale = match settings.ui_scale {
                    None => Some(UI_SCALE_STOPS[0]),
                    Some(scale) => next_stop(&UI_SCALE_STOPS, scale),
                };
            }
            EnhancedRow::Speed => self.speed = self.speed.next(),
            EnhancedRow::Overlay => self.overlay = !self.overlay,
            EnhancedRow::Rules => {
                self.rules = match self.rules {
                    Preset::Vanilla => Preset::Enhanced,
                    Preset::Enhanced => Preset::Vanilla,
                };
            }
        }
        self
    }
    /// `row`'s value as shown.
    fn text(&self, row: EnhancedRow) -> String {
        let settings = &self.settings;
        let on = |flag: bool| if flag { "On" } else { "Off" };
        match row {
            EnhancedRow::Viewport => match settings.viewport {
                ViewportMode::Fixed => "Fixed 640x480".into(),
                ViewportMode::Unlocked => "Unlocked".into(),
            },
            EnhancedRow::Zoom => format!("{:.2}x", settings.default_zoom),
            EnhancedRow::Smooth => on(settings.smooth_camera).into(),
            EnhancedRow::Follow => settings.camera_follow.label().into(),
            EnhancedRow::Margin => match settings.camera_recenter_margin {
                0 => "Off".into(),
                margin => format!("{margin} px"),
            },
            EnhancedRow::Roof => match settings.roof_peek {
                RoofPeek::Vanilla => "Vanilla".into(),
                RoofPeek::CursorXray => "Cursor X-ray".into(),
                RoofPeek::Hidden => "Hidden".into(),
            },
            EnhancedRow::UiScale => settings
                .ui_scale
                .map_or_else(|| "Fit window".into(), |scale| format!("{scale:.1}x")),
            EnhancedRow::Speed => self.speed.label().into(),
            EnhancedRow::Overlay => on(self.overlay).into(),
            EnhancedRow::Rules => match self.rules {
                Preset::Vanilla => "Vanilla".into(),
                Preset::Enhanced => "Enhanced".into(),
            },
        }
    }
    /// How `row` takes effect: its tag and color.
    fn tag(&self, row: EnhancedRow) -> (&'static str, ColorKey) {
        if row == EnhancedRow::Rules {
            ("NEXT START", ColorKey::AMBER)
        } else if row.unlocked_only() && self.settings.viewport == ViewportMode::Fixed {
            ("NOT IN FIXED", ColorKey::GREY)
        } else if row == EnhancedRow::Margin && self.settings.camera_follow != CameraFollow::Off {
            ("FOLLOW OFF", ColorKey::GREY)
        } else {
            ("LIVE", ColorKey::GREEN)
        }
    }
    /// The settings to save: these values over the file's table. The rules
    /// stay as the file has them until the player chose some.
    fn profile(&self, chosen: Option<Preset>, file: EnhancedProfile) -> EnhancedProfile {
        let settings = &self.settings;
        EnhancedProfile {
            preset: chosen.or(file.preset),
            viewport_mode: Some(settings.viewport),
            default_zoom: Some(settings.default_zoom),
            smooth_camera: Some(settings.smooth_camera),
            roof_peek: Some(settings.roof_peek),
            ui_scale: Some(UiScale::from_factor(settings.ui_scale)),
            game_speed: Some(self.speed),
            tactical_overlay: Some(self.overlay),
            camera_follow: Some(settings.camera_follow),
            camera_recenter_margin: Some(settings.camera_recenter_margin),
        }
    }
}

/// The first of `stops` past `value`.
fn next_stop(stops: &[f32], value: f32) -> Option<f32> {
    stops.iter().copied().find(|stop| *stop > value + 1e-3)
}

/// The Enhanced window's own state: what the session's settings do not hold.
#[derive(Debug, Default)]
pub(crate) struct EnhancedPanel {
    /// The rules the player chose for the next launch; `None` leaves the
    /// file's.
    rules: Option<Preset>,
    /// A change since the window opened or last saved.
    unsaved: bool,
    /// The last save's result, and whether it failed.
    note: Option<(String, bool)>,
}

/// What a save did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SaveOutcome {
    Saved {
        profile: String,
    },
    /// A replay or a headless session: nothing was read or written.
    Headless,
    /// No settings file was found.
    NoFile,
    /// The file marks another game's profile active, so
    /// [`config::save_profile_enhanced`] would write this session's settings
    /// into that profile's table.
    OtherProfile {
        file: String,
        session: String,
    },
    /// The file could not be read or written.
    Failed(String),
}

impl SaveOutcome {
    /// The window's line for it, and whether it is a failure.
    fn note(&self) -> (String, bool) {
        match self {
            Self::Saved { profile } => (format!("Saved to the {profile} profile."), false),
            Self::Headless => ("A replay or headless session saves nothing.".into(), true),
            Self::NoFile => (
                "No vault-engine.toml found: changes last this session.".into(),
                true,
            ),
            Self::OtherProfile { file, session } => (
                format!("Not saved: the file's profile is {file}, this session is {session}."),
                true,
            ),
            Self::Failed(_) => (
                "Not saved: the settings file could not be read or written.".into(),
                true,
            ),
        }
    }
}

/// A place in the Enhanced window a press acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelHit {
    Preset(BuiltinPreset),
    Row(EnhancedRow),
    Save,
    Done,
}

/// A built-in preset's button.
fn preset_rect(index: usize) -> Rect {
    let x = ROW_X + (PRESET_SIZE[0] + PRESET_GAP) * index as i32;
    Rect::at([x, PRESET_Y], PRESET_SIZE)
}

/// What the Enhanced window has at `point`.
fn panel_hit(point: [i32; 2]) -> Option<PanelHit> {
    let preset = BuiltinPreset::ALL
        .into_iter()
        .enumerate()
        .find(|(index, _)| preset_rect(*index).contains(point))
        .map(|(_, preset)| PanelHit::Preset(preset));
    let row = EnhancedRow::ALL
        .into_iter()
        .find(|row| row.value_rect().contains(point))
        .map(PanelHit::Row);
    preset.or(row).or_else(|| {
        if vault_core::ui::red_button(SAVE_AT).contains(point) {
            Some(PanelHit::Save)
        } else if vault_core::ui::red_button(DONE_AT).contains(point) {
            Some(PanelHit::Done)
        } else {
            None
        }
    })
}

/// The settings file a save writes: the one a launch reads. A test gives
/// [`MapPreview::save_enhanced_to`] its own, and never finds the player's.
fn settings_file() -> Option<PathBuf> {
    if cfg!(test) {
        None
    } else {
        config::find(&config::search_dirs())
    }
}

/// A one-pixel outline just inside `rect`.
fn outline(image: &mut IndexedImage, [x, y, w, h]: [i32; 4], color: u8) {
    image.fill_rect([x, y, w, 1], color);
    image.fill_rect([x, y + h - 1, w, 1], color);
    image.fill_rect([x, y, 1, h], color);
    image.fill_rect([x + w - 1, y, 1, h], color);
}

/// `row`'s badge in `rect`: a small picture of what the setting does in play,
/// drawn with the interface's own colors, as there is no game art for it.
fn draw_badge(
    painter: &Painter,
    image: &mut IndexedImage,
    row: EnhancedRow,
    values: &EnhancedValues,
    rect: Rect,
) {
    let [amber, green, red, grey, black] = [
        ColorKey::AMBER,
        ColorKey::GREEN,
        ColorKey::RED,
        ColorKey::GREY,
        ColorKey::BLACK,
    ]
    .map(|key| painter.color(key));
    let Rect { x, y, w, h } = rect;
    image.fill_rect([x, y, w, h], black);
    outline(image, [x, y, w, h], grey);
    let [cx, cy] = [x + w / 2, y + h / 2];
    let settings = &values.settings;
    match row {
        // The 4:3 frame alone, or the world running past it on both sides.
        EnhancedRow::Viewport => {
            if settings.viewport == ViewportMode::Unlocked {
                image.fill_rect([x + 8, y + 5, w - 16, h - 10], green);
            }
            image.fill_rect([cx - 13, y + 4, 26, h - 8], black);
            outline(image, [cx - 13, y + 4, 26, h - 8], amber);
        }
        // Floor tiles at the zoom.
        EnhancedRow::Zoom => {
            let side = ((settings.default_zoom * 7.0).round() as i32).clamp(3, h - 6);
            let mut at = x + 6;
            let mut tile = 0;
            while at + side < x + w - 5 {
                let color = if tile % 2 == 0 { amber } else { grey };
                image.fill_rect([at, cy - side / 2, side, side], color);
                at += side + 1;
                tile += 1;
            }
        }
        // A camera path: one even line, or a few jumps.
        EnhancedRow::Smooth => {
            let steps = if settings.smooth_camera { 64 } else { 4 };
            for step in 0..steps {
                let along = step * 64 / steps;
                let [px, py] = [x + 14 + along, y + h - 7 - along * (h - 13) / 64];
                let size = if settings.smooth_camera { 2 } else { 6 };
                image.fill_rect([px, py, size, 2], amber);
            }
        }
        // A house: the walls, a critter inside, and the roof over them, cut
        // around the cursor or gone.
        EnhancedRow::Roof => {
            outline(image, [cx - 24, y + 8, 48, h - 11], grey);
            image.fill_rect([cx - 2, y + 11, 5, 7], green);
            if settings.roof_peek != RoofPeek::Hidden {
                image.fill_rect([cx - 28, y + 3, 56, 11], amber);
            }
            if settings.roof_peek == RoofPeek::CursorXray {
                image.fill_rect([cx - 9, y + 3, 19, 11], black);
                image.fill_rect([cx - 12, y + 5, 25, 7], black);
                image.fill_rect([cx - 2, y + 11, 5, 3], green);
            }
        }
        // The view's frame over a walk: left behind (off), centred on the
        // player's hex (step), or on the player and its trail (smooth).
        EnhancedRow::Follow => {
            let player = [cx + 18, cy - 2];
            for along in 0..4 {
                image.fill_rect([cx - 30 + along * 12, cy + 4 - along * 2, 4, 2], grey);
            }
            let frame = match settings.camera_follow {
                CameraFollow::Off => [x + 6, y + 3, 40, h - 6],
                CameraFollow::Step | CameraFollow::Smooth => [player[0] - 20, y + 3, 40, h - 6],
            };
            outline(image, frame, amber);
            if settings.camera_follow == CameraFollow::Smooth {
                image.fill_rect([player[0] - 16, cy, 14, 1], amber);
            }
            image.fill_rect([player[0], player[1], 4, 5], green);
        }
        // The view, and inside it the box the player may walk in before it
        // recentres; at 0 there is none.
        EnhancedRow::Margin => {
            outline(image, [x + 6, y + 3, w - 12, h - 6], amber);
            let margin = i32::from(settings.camera_recenter_margin);
            if margin > 0 {
                let [dx, dy] = [margin / 8, margin / 32];
                outline(
                    image,
                    [x + 6 + dx, y + 3 + dy, w - 12 - 2 * dx, h - 6 - 2 * dy],
                    grey,
                );
            }
            image.fill_rect([cx - 2, cy - 2, 4, 5], green);
        }
        // The interface bar at its scale.
        EnhancedRow::UiScale => {
            let scale = settings.ui_scale.unwrap_or(2.0);
            let width = ((scale * 24.0).round() as i32).clamp(12, w - 8);
            let height = ((scale * 4.0).round() as i32).clamp(2, h - 8);
            image.fill_rect([cx - width / 2, y + h - 4 - height, width, height], amber);
            if settings.ui_scale.is_none() {
                outline(image, [x + 3, y + 3, w - 6, h - 6], grey);
            }
        }
        // One chevron for each step above 1.0x, and one for 1.0x itself.
        EnhancedRow::Speed => {
            let count = GameSpeed::ALL
                .iter()
                .position(|speed| *speed == values.speed)
                .unwrap_or(0) as i32
                + 1;
            let left = cx - 7 * count;
            for chevron in 0..count {
                for slant in 0_i32..7 {
                    let at = left + chevron * 14 + (3 - (3 - slant).abs()) * 2;
                    image.fill_rect([at, cy - 7 + slant * 2, 4, 2], amber);
                }
            }
        }
        // The overlay in combat: a hurt critter's HP bar, and the to-hit
        // chance beside it. Off, the bare critter.
        EnhancedRow::Overlay => {
            let critter = if values.overlay { green } else { grey };
            image.fill_rect([x + 22, y + 10, 7, 10], critter);
            if values.overlay {
                image.fill_rect([x + 12, y + 4, 28, 5], grey);
                image.fill_rect([x + 13, y + 5, 26, 3], black);
                image.fill_rect([x + 13, y + 5, 12, 3], red);
                painter.text(image, 101, ColorKey::WHITE, b"75%", [x + 48, y + 8], 40);
            }
        }
        // A switch: the rules the next launch runs.
        EnhancedRow::Rules => {
            outline(image, [cx - 26, y + 6, 52, h - 12], grey);
            let (at, color) = match values.rules {
                Preset::Vanilla => (cx - 25, grey),
                Preset::Enhanced => (cx + 1, amber),
            };
            image.fill_rect([at, y + 7, 24, h - 14], color);
        }
    }
}

/// The Enhanced window.
impl MapPreview {
    /// The options menu shows the Enhanced button: the menu page of a session
    /// under the `enhanced` preset with its presentation on. Never under
    /// Vanilla.
    pub(crate) fn enhanced_entry_shown(&self) -> bool {
        self.qol()
            && self.enhanced.is_some()
            && matches!(
                self.screen,
                Screen::Options(OptionsScreen::Window(OptionsWindow {
                    page: OptionsPage::Menu
                }))
            )
    }
    fn open_enhanced_panel(&mut self) {
        self.screen = Screen::Options(OptionsScreen::Enhanced(EnhancedPanel::default()));
    }
    fn close_enhanced_panel(&mut self) {
        self.screen = Screen::Options(OptionsScreen::page(OptionsPage::Menu));
    }
    fn enhanced_panel(&mut self) -> Option<&mut EnhancedPanel> {
        match &mut self.screen {
            Screen::Options(OptionsScreen::Enhanced(panel)) => Some(panel),
            _ => None,
        }
    }
    /// The session's game speed, as the window left it. The shell keeps its
    /// own copy, for Alt+F and the worlds a game makes next.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn session_game_speed(&self) -> GameSpeed {
        self.qol_state.speed
    }
    /// This session's edition.
    fn session_edition(&self) -> GameProfile {
        if self.host.world.fallout2.is_some() {
            GameProfile::Fallout2
        } else {
            GameProfile::Fallout1
        }
    }
    /// A session no player runs: every headless world (a replay, a gallery
    /// capture, `--headless`) completes its movies.
    fn headless_session(&self) -> bool {
        self.host.movie_policy == MoviePolicy::CompleteImmediately
    }
    fn enhanced_values(&self) -> EnhancedValues {
        let rules = match &self.screen {
            Screen::Options(OptionsScreen::Enhanced(panel)) => panel.rules,
            _ => None,
        };
        EnhancedValues {
            settings: self.enhanced().unwrap_or_default(),
            speed: self.qol_state.speed,
            overlay: self.qol_state.overlay,
            rules: rules.unwrap_or(Preset::Enhanced),
        }
    }
    /// Give the session `values`. The world's clock and the logged input
    /// drive all of it, so a replay's click lands here on the same frame.
    fn apply_enhanced_values(&mut self, values: EnhancedValues, rules: Option<Preset>) {
        let old = self.enhanced_values();
        self.set_enhanced(Some(values.settings));
        if values.settings.default_zoom != old.settings.default_zoom {
            let (zoom, now) = (f64::from(values.settings.default_zoom), self.clock);
            if let Some(enhanced) = &mut self.enhanced {
                enhanced.camera.zoom_user(zoom, None, now);
            }
        }
        self.init_game_speed(values.speed);
        self.qol_state.overlay = values.overlay;
        if let Some(panel) = self.enhanced_panel() {
            panel.rules = rules.or(panel.rules);
            panel.unsaved = true;
            panel.note = None;
        }
        self.thaw_view();
        self.pointer.redraw = true;
    }
    /// A built-in preset: what it names, at once.
    fn apply_builtin(&mut self, preset: BuiltinPreset) {
        let profile = preset.settings();
        let values = self.enhanced_values().with_profile(profile);
        self.apply_enhanced_values(values, profile.preset);
    }
    fn enhanced_panel_click(&mut self, point: [i32; 2]) {
        match panel_hit(point) {
            Some(PanelHit::Preset(preset)) => {
                self.press();
                self.apply_builtin(preset);
            }
            Some(PanelHit::Row(row)) => {
                self.press();
                let values = self.enhanced_values().stepped(row);
                let rules = (row == EnhancedRow::Rules).then_some(values.rules);
                self.apply_enhanced_values(values, rules);
            }
            Some(PanelHit::Save) => {
                self.press_button(vault_core::ui::red_button(SAVE_AT));
                self.save_enhanced();
            }
            Some(PanelHit::Done) => {
                self.press_button(vault_core::ui::red_button(DONE_AT));
                self.close_enhanced_panel();
            }
            None => {}
        }
    }
    fn enhanced_panel_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Escape => self.close_enhanced_panel(),
            KeyCode::Enter | KeyCode::NumpadEnter => {
                self.play_effect(BUTTON_PRESS.as_bytes(), None);
                self.close_enhanced_panel();
            }
            KeyCode::KeyS => {
                self.play_effect(BUTTON_PRESS.as_bytes(), None);
                self.save_enhanced();
            }
            _ => return false,
        }
        true
    }
    /// Save to the settings file a launch reads, when the session may.
    fn save_enhanced(&mut self) {
        let path = if self.headless_session() {
            None
        } else {
            settings_file()
        };
        self.save_enhanced_to(path.as_deref());
    }
    /// Save the session's settings to the active profile of the settings file
    /// at `path`, and show what happened. A failure leaves the file alone and
    /// the session running on what it has.
    pub(crate) fn save_enhanced_to(&mut self, path: Option<&Path>) -> SaveOutcome {
        let outcome = self.write_enhanced(path);
        if let SaveOutcome::Failed(error) = &outcome {
            tracing::warn!(%error, "enhanced settings not saved");
            self.status = error.clone().into_bytes();
        }
        let note = outcome.note();
        let saved = matches!(outcome, SaveOutcome::Saved { .. });
        if let Some(panel) = self.enhanced_panel() {
            panel.note = Some(note);
            panel.unsaved &= !saved;
        }
        self.pointer.redraw = true;
        outcome
    }
    /// The file is read, and written, only here.
    fn write_enhanced(&self, path: Option<&Path>) -> SaveOutcome {
        if self.headless_session() {
            return SaveOutcome::Headless;
        }
        let Some(path) = path else {
            return SaveOutcome::NoFile;
        };
        let file = match Config::load(path) {
            Ok(file) => file,
            Err(error) => return SaveOutcome::Failed(error.to_string()),
        };
        let edition = self.session_edition();
        // A session that knows its profile saves there, whichever the file
        // has active. One that does not can only trust the file's own.
        let name = match &self.profile_name {
            Some(name) => name.clone(),
            None if file.active_game() == edition => file.active_profile().to_owned(),
            None => {
                return SaveOutcome::OtherProfile {
                    file: file.active_profile().to_owned(),
                    session: file.scope_name(edition).to_owned(),
                };
            }
        };
        let Some((owner, current)) = file.profile_enhanced(&name).filter(|it| it.0 == edition)
        else {
            return SaveOutcome::OtherProfile {
                file: file.active_profile().to_owned(),
                session: name,
            };
        };
        let _ = owner;
        let rules = match &self.screen {
            Screen::Options(OptionsScreen::Enhanced(panel)) => panel.rules,
            _ => None,
        };
        let settings = self.enhanced_values().profile(rules, current);
        match config::save_profile_enhanced(path, &name, settings) {
            Ok(()) => SaveOutcome::Saved { profile: name },
            Err(error) => SaveOutcome::Failed(error),
        }
    }
    /// The options menu's Enhanced button, in the art of the five above it.
    fn draw_enhanced_entry(&self, painter: &Painter, image: &mut IndexedImage) {
        let rect = ENHANCED_ENTRY;
        let art = self.draw_button_opaque(painter, image, rect, ("opbtnoff.frm", "opbtnon.frm"));
        let key = if art == "opbtnon.frm" {
            ColorKey::LABEL_PRESSED
        } else {
            ColorKey::LABEL
        };
        painter.centered(image, 103, key, b"ENHANCED", [rect.x, rect.y + 8], rect.w);
    }
    /// The window: the presets, then a row for each setting with its value,
    /// how it takes effect and its badge, then the last save's result.
    fn draw_enhanced_panel(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        panel: &EnhancedPanel,
    ) {
        let values = self.enhanced_values();
        let [amber, grey, black] =
            [ColorKey::AMBER, ColorKey::GREY, ColorKey::BLACK].map(|key| painter.color(key));
        let Rect { x, y, w, h } = PANEL;
        image.fill_rect([x, y, w, h], black);
        outline(image, [x, y, w, h], amber);
        outline(image, [x + 2, y + 2, w - 4, h - 4], grey);
        let title = b"ENHANCED OPTIONS";
        painter.centered(image, 103, ColorKey::LABEL, title, [x, y + 9], w);
        for (index, preset) in BuiltinPreset::ALL.into_iter().enumerate() {
            let rect = preset_rect(index);
            let on = values.with_profile(preset.settings()) == values;
            let area = [rect.x, rect.y, rect.w, rect.h];
            image.fill_rect(area, if on { amber } else { black });
            outline(image, area, amber);
            let ink = if on { ColorKey::BLACK } else { ColorKey::AMBER };
            let label = preset.label().as_bytes();
            painter.centered(image, 101, ink, label, [rect.x, rect.y + 7], rect.w);
        }
        for (index, row) in EnhancedRow::ALL.into_iter().enumerate() {
            let top = ROW_Y + ROW_HEIGHT * index as i32;
            let label = row.label().as_bytes();
            painter.text(image, 101, ColorKey::LABEL, label, [ROW_X, top + 2], 190);
            let hint = row.hint().as_bytes();
            painter.text(image, 101, ColorKey::GREY, hint, [ROW_X, top + 12], 190);
            let rect = row.value_rect();
            outline(image, [rect.x, rect.y, rect.w, rect.h], grey);
            let text = values.text(row);
            let origin = [rect.x, rect.y + 6];
            painter.centered(image, 101, ColorKey::AMBER, text.as_bytes(), origin, rect.w);
            let (tag, key) = values.tag(row);
            painter.text(image, 101, key, tag.as_bytes(), [TAG_X, top + 8], 82);
            draw_badge(painter, image, row, &values, row.badge_rect());
        }
        let (note, failed) = match &panel.note {
            Some((note, failed)) => (note.as_str(), *failed),
            None if values.rules == Preset::Vanilla => {
                ("Vanilla rules start at the next launch.", false)
            }
            None if panel.unsaved => ("Applied now. Save keeps it for the next launch.", false),
            None => ("", false),
        };
        let key = if failed {
            ColorKey::RED
        } else {
            ColorKey::GREEN
        };
        painter.text(image, 101, key, note.as_bytes(), [ROW_X, NOTE_Y], 508);
        for (at, caption) in [(SAVE_AT, b"SAVE"), (DONE_AT, b"DONE")] {
            self.draw_red_button(painter, image, vault_core::ui::red_button(at));
            let origin = [at[0] + CAPTION_DX, at[1] - 1];
            painter.text(image, 103, ColorKey::LABEL, caption, origin, 70);
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
