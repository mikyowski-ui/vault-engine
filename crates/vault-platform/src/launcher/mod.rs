//! The pre-game launcher: game data, display and audio settings and the
//! mod load order, validated live, then Play. It draws with its own font
//! and palette only, so it works before any game data is found.
//!
//! The state machine is headless: [`Launcher::handle`] takes one input and
//! may answer with an action, and [`Launcher::image`] draws the frame.
//! File-system access goes through `validate`, `browse`, the settings
//! writers and discovery in [`Setup::data_dirs`], so tests point it at
//! temporary directories.
//!
//! Each game keeps its own paths (`[profiles.<game>]` in the settings):
//! the Game toggle keeps the fields as the old game's profile and fills
//! them from the new game's, or, when it has none yet, with that game's
//! data discovered in its own directories.
//!
//! The sidebar's profiles are both editions and each custom profile, one of
//! them active. A profile's `sfall`, load order and Enhanced settings are its
//! own. The editable paths are an edition's; a custom profile shares them
//! until it has an `install_dir` of its own, which then wins, and Save never
//! writes a custom profile's folder over its edition's paths. The rows of the
//! Enhanced settings write the active profile's table, so read them through
//! [`Launcher::active_enhanced`]: the top-level keys are only what its table
//! leaves open.
//!
//! This is engine infrastructure, not part of any specification.
mod browse;
pub(crate) mod font;
pub(crate) mod validate;
mod view;
mod widgets;

#[cfg(test)]
mod tests;

use crate::config::{
    self, Config, EnhancedProfile, GamePaths, HotkeyHints, KnownKind, ModList, PerGame, Preset,
    ProfileScope,
};
use crate::enhanced::{ActiveEnhanced, PresetMatch, ViewportMode};
use crate::gallery::settings_preview::{PreviewSettings, SettingsPreviewCanvas};
use browse::{Browser, Shortcut, Target};
use std::path::{Path, PathBuf};
pub(crate) use validate::Empty;
use validate::{Check, State};
use vault_core::GameProfile;
use vault_formats::{LayerEntry, LoadOrderPolicy};
use vault_render::{IndexedImage, ScalingMode};
pub(crate) use view::color;
pub use view::palette;
use widgets::{List, TextField};
use winit::keyboard::KeyCode;

/// The load order a new mod list is written to, beside the settings.
const LOAD_ORDER: &str = "load_order.toml";

/// Where a new load order for `config`'s active profile goes, in the folder of
/// `beside`. Each profile has its own list, so when [`LOAD_ORDER`] is taken
/// this one gets a file named for its profile. A file that exists but no
/// profile lists is taken too: this list must not write over it.
fn new_load_order(config: &Config, beside: &Path) -> PathBuf {
    let taken = |file: &Path| file.exists() || listed_elsewhere(config, file);
    let shared = beside.with_file_name(LOAD_ORDER);
    if !taken(&shared) {
        return shared;
    }
    let profile = config.active_profile();
    let mut file = beside.with_file_name(format!("load_order_{profile}.toml"));
    let mut number = 1;
    while taken(&file) {
        number += 1;
        file = beside.with_file_name(format!("load_order_{profile}_{number}.toml"));
    }
    file
}

/// Whether a profile other than the active one lists the load order `file`.
fn listed_elsewhere(config: &Config, file: &Path) -> bool {
    let active = config.active_profile();
    config
        .profile_names()
        .iter()
        .filter(|(name, _)| name != active)
        .filter_map(|(name, _)| stored_scope(config, name))
        .any(|scope| matches!(&scope.mod_list, Some(ModList::File(listed)) if listed == file))
}

/// Where `name` keeps its scope. That is empty while it is the active profile,
/// whose scope is the top-level fields.
fn stored_scope<'a>(config: &'a Config, name: &str) -> Option<&'a ProfileScope> {
    match config::parse_game(name) {
        Some(game) => Some(config.scopes.get(game)),
        None => config
            .custom_profiles
            .iter()
            .find(|custom| custom.name == name)
            .map(|custom| &custom.scope),
    }
}

/// The list of folders `game`'s data is remembered in.
fn known_kind(game: GameProfile) -> KnownKind {
    match game {
        GameProfile::Fallout1 => KnownKind::Fallout1,
        GameProfile::Fallout2 => KnownKind::Fallout2,
    }
}

/// The enhanced preset's zoom stepper.
pub const ZOOM_STEPS: [f32; 8] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 2.5, 3.0];
/// The enhanced preset's interface scales (`None`: auto).
pub const UI_SCALES: [Option<f32>; 6] =
    [None, Some(1.0), Some(1.5), Some(2.0), Some(2.5), Some(3.0)];

/// One input the shell passes on: frame pixels for the pointer.
#[derive(Debug, Clone, PartialEq)]
pub enum LauncherInput {
    /// A key press or repeat, with Shift held or not.
    Key {
        key: KeyCode,
        shift: bool,
    },
    Pointer([i32; 2]),
    /// A left-button press.
    Press([i32; 2]),
    /// Wheel lines, positive away from the user.
    Wheel(f64),
    /// A file or folder dropped on the window.
    Dropped(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherAction {
    Play,
    PlayDebug,
    /// Play, loading the active profile's most recent save.
    ResumeLast,
    /// Play with the bot in Watch Mode.
    AutoplayTest,
    Quit,
    /// Back to the selector, from Setup (SPEC-020 §2.4).
    Back,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Data,
    Display,
    Mods,
    Audio,
    Debug,
}

impl Tab {
    #[cfg(test)]
    const ALL: [Self; 5] = [
        Self::Data,
        Self::Display,
        Self::Mods,
        Self::Audio,
        Self::Debug,
    ];
}

/// A profile in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileCard {
    /// As [`Config::profile_names`] gives it.
    pub name: String,
    pub title: String,
    pub edition: GameProfile,
    pub active: bool,
    /// Its data validates.
    pub ready: bool,
    pub sfall: bool,
    /// Enabled mod layers.
    pub mods: usize,
    pub last_played: bool,
}

/// The game-data path fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Master,
    Critter,
    DataRoot,
}

impl Field {
    const ALL: [Self; 3] = [Self::Master, Self::Critter, Self::DataRoot];
    fn index(self) -> usize {
        self as usize
    }
}

/// Everything that takes the focus or a click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Widget {
    Tab(Tab),
    Field(Field),
    Browse(Field),
    /// The game whose data this is (`game`).
    Game,
    Scaling,
    Fullscreen,
    /// The touch UI: gestures and the on-screen pill bar (`touch_ui`).
    TouchUi,
    Preset,
    Audio,
    Debug,
    /// The `enhanced` preset's presentation, shown with that preset only.
    Viewport,
    Zoom,
    SmoothCamera,
    RoofPeek,
    UiScale,
    /// The game speed (`game_speed`), with the enhanced preset only.
    GameSpeed,
    /// The hotkey toasts (`hotkey_hints`), under both presets.
    HotkeyHints,
    Layers,
    LayerUp,
    LayerDown,
    LayerToggle,
    LayerRemove,
    NewLayer,
    AddLayer,
    BrowseLayer,
    Save,
    Play,
    PlayDebug,
    Quit,
    BrowseList,
    BrowseUse,
    BrowseCancel,
    /// A sidebar card, by its place in [`Launcher::profile_cards`].
    ProfileCard(usize),
    ProfilePlay(usize),
    ProfileResume(usize),
    ProfileAutoplay(usize),
    NewProfile,
    /// The tactical overlay's toggle (`tactical_overlay`).
    TacticalOverlay,
    BuiltinPreset(crate::enhanced::BuiltinPreset),
    SaveCustomPreset,
    /// A saved custom preset, by its place in `custom_presets`.
    CustomPreset(usize),
    /// Fill the paths from the folder named, Et Tu and mods included.
    AutoDetect,
    /// A shortcut in the browser, by its place in [`browse::Browser::shortcuts`].
    KnownPath(usize),
}

/// Where the launcher starts from.
pub struct Setup {
    /// The settings read (or discovered), unknown keys included.
    pub config: Config,
    /// The settings file they came from, rewritten on Save.
    pub settings_path: Option<PathBuf>,
    /// Without one, where a new file is tried, in order.
    pub save_dirs: Vec<PathBuf>,
    /// The folder the browser opens in when a field names none.
    pub start_dir: PathBuf,
    /// Why the settings could not be used (a malformed file), if they
    /// could not.
    pub problem: Option<String>,
    /// Where each game's data is looked for when its profile names none
    /// ([`config::data_dirs`]).
    pub data_dirs: PerGame<Vec<PathBuf>>,
}

/// Where a dropped path goes: a `.DAT` to the field its name matches
/// (`critter` in the name: CRITTER.DAT, else MASTER.DAT), a folder to the
/// data folder (or, holding both archives, to all three fields), and on
/// the Mods tab a folder or `.DAT` to a new layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropTarget {
    Field(Field),
    Layer,
    Rejected,
}

pub fn route_drop(path: &Path, is_dir: bool, tab: Tab) -> DropTarget {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let dat = !is_dir && name.ends_with(".dat");
    match tab {
        Tab::Mods if is_dir || dat => DropTarget::Layer,
        _ if is_dir => DropTarget::Field(Field::DataRoot),
        _ if dat && name.contains("critter") => DropTarget::Field(Field::Critter),
        _ if dat => DropTarget::Field(Field::Master),
        _ => DropTarget::Rejected,
    }
}

/// The mod load order being edited.
struct Mods {
    /// The `load_order.toml` read and rewritten; `None` until Save
    /// creates one beside the settings.
    file: Option<PathBuf>,
    entries: Vec<LayerEntry>,
    /// Why the list cannot be edited here.
    read_only: Option<String>,
    /// The list is the settings' inline `mod_list`, which cannot hold a
    /// disabled layer: the first edit makes it a load order.
    inline: bool,
    /// The load order could not be read: the game would not start.
    broken: bool,
    dirty: bool,
    list: List,
    new_layer: TextField,
    check: validate::Mods,
}

impl Mods {
    fn load(mod_list: Option<&ModList>) -> Self {
        let mut mods = Self {
            file: None,
            entries: Vec::new(),
            read_only: None,
            inline: false,
            broken: false,
            dirty: false,
            list: List::default(),
            new_layer: TextField::default(),
            check: validate::Mods {
                layers: Vec::new(),
                manifests: Vec::new(),
                stack: Check::unset("no mod layers"),
            },
        };
        match mod_list {
            None => {}
            Some(ModList::Paths(paths)) => {
                mods.entries = paths
                    .iter()
                    .map(|path| LayerEntry {
                        path: Some(path.to_string_lossy().into_owned()),
                        ..LayerEntry::default()
                    })
                    .collect();
                mods.inline = true;
            }
            Some(ModList::File(file)) => {
                mods.file = Some(file.clone());
                match crate::storage::read_config_file(file) {
                    // Save creates it.
                    Ok(None) => {}
                    Err(error) => mods.fail(error),
                    Ok(Some(text)) => match vault_formats::parse_load_order(&text) {
                        Ok(entries) => mods.entries = entries,
                        Err(error) => mods.fail(format!("{}: {error}", file.display())),
                    },
                }
            }
        }
        mods
    }
    fn fail(&mut self, reason: String) {
        self.read_only = Some(format!("{reason} (fix it by hand; Save leaves it as is)"));
        self.broken = true;
    }
    /// Whether the stack can start the game, or the first reason it cannot.
    fn ready(&self) -> Result<(), String> {
        if self.broken {
            return Err("Mods: the load order cannot be read.".into());
        }
        if self.check.stack.state == State::Bad {
            return Err(format!("Mods: {}.", self.check.stack.text));
        }
        // A layer that mounts can still fail on its `vault-mod.toml`.
        if let Some(check) = self
            .check
            .layers
            .iter()
            .find(|check| check.state == State::Bad)
        {
            return Err(format!("Mods: {}.", check.text));
        }
        Ok(())
    }
    fn enabled(&self) -> usize {
        self.entries.iter().filter(|entry| entry.enabled).count()
    }
}

/// What a card costs to find out, kept between the changes that move it.
struct CardCheck {
    name: String,
    ready: bool,
    mods: usize,
}

pub struct Launcher {
    config: Config,
    settings_path: Option<PathBuf>,
    save_dirs: Vec<PathBuf>,
    start_dir: PathBuf,
    tab: Tab,
    focus: Widget,
    hover: Option<Widget>,
    fields: [TextField; 3],
    checks: [Check; 3],
    mods: Mods,
    browser: Option<Browser>,
    status: String,
    status_state: State,
    data_dirs: PerGame<Vec<PathBuf>>,
    /// The start found the fields' data on disk (the active game's profile
    /// named none).
    discovered: bool,
    /// The selector's Setup opened it: Quit reads `Back`, and Play is a
    /// choice (SPEC-020 §2.4).
    from_selector: bool,
    /// Each profile but the active one, whose card reads the live state.
    card_checks: Vec<CardCheck>,
    /// The per-user folder, whose load order may reach any folder.
    user_root: Option<PathBuf>,
    preview: SettingsPreviewCanvas,
    /// What `preview` was last given.
    previewed: PreviewSettings,
    /// The widget the pointer came to, until a press takes its next value.
    preview_hover: Option<Widget>,
}

/// A game's name in the launcher's messages.
fn title(game: GameProfile) -> &'static str {
    match game {
        GameProfile::Fallout1 => "Fallout 1",
        GameProfile::Fallout2 => "Fallout 2",
    }
}

/// A profile's name in the launcher's messages and on its card.
fn profile_title(name: &str, edition: GameProfile) -> String {
    match config::parse_game(name) {
        Some(_) => title(edition).to_owned(),
        None if name == config::ETTU_PROFILE => "Fallout: Et Tu".to_owned(),
        None => name.to_owned(),
    }
}

impl Launcher {
    pub fn new(setup: Setup) -> Self {
        let mut config = setup.config;
        // Et Tu has its own card from the start: `--profile ettu` and the
        // desktop menu find it once its paths are set. An error means the
        // settings hold one of that name already.
        let _ = config.add_custom_profile(config::ETTU_PROFILE, GameProfile::Fallout2);
        let mut launcher = Self {
            fields: Default::default(),
            checks: [Check::unset(""), Check::unset(""), Check::unset("")],
            mods: Mods::load(config.mod_list.as_ref()),
            config,
            settings_path: setup.settings_path,
            save_dirs: setup.save_dirs,
            start_dir: setup.start_dir,
            tab: Tab::Data,
            focus: Widget::Field(Field::Master),
            hover: None,
            browser: None,
            status: String::new(),
            status_state: State::Unset,
            data_dirs: setup.data_dirs,
            discovered: false,
            from_selector: false,
            card_checks: Vec::new(),
            user_root: crate::storage::user_config_root(),
            preview: Default::default(),
            previewed: PreviewSettings::default(),
            preview_hover: None,
        };
        let paths = launcher.config.launch_paths(launcher.config.active_game());
        launcher.load_fields(&paths);
        let found = if paths.is_empty() {
            launcher.discover_fields()
        } else {
            None
        };
        launcher.discovered = found.is_some();
        launcher.revalidate();
        launcher.revalidate_mods();
        launcher.refresh_cards();
        launcher.show_preview();
        match (setup.problem, launcher.ready()) {
            (Some(problem), _) => launcher.report(State::Bad, problem),
            (None, Ok(())) => {
                launcher.focus = Widget::Play;
                let text = match found {
                    Some(dir) => format!(
                        "{} data found in {}: Enter plays.",
                        title(launcher.config.active_game()),
                        dir.display()
                    ),
                    None => "Ready: Enter plays.".into(),
                };
                launcher.report(State::Ok, text);
            }
            (None, Err(reason)) => launcher.report(State::Unset, reason),
        }
        launcher
    }

    pub fn image(&self) -> IndexedImage {
        view::draw(self)
    }

    /// The sidebar's cards. Nothing here touches the disk, so a frame can ask:
    /// another profile's `ready` and `mods` are kept from when it last changed.
    pub fn profile_cards(&self) -> Vec<ProfileCard> {
        let config = &self.config;
        let active = config.active_profile();
        // `last_played` names an edition: its card is the profile it runs under.
        let last = config
            .last_played
            .as_deref()
            .and_then(config::parse_game)
            .map(|game| config.scope_name(game));
        config
            .profile_names()
            .into_iter()
            .map(|(name, edition)| {
                let current = name == active;
                let stored = stored_scope(config, &name);
                let check = self.card_checks.iter().find(|check| check.name == name);
                ProfileCard {
                    title: profile_title(&name, edition),
                    edition,
                    active: current,
                    ready: match current {
                        true => self.ready().is_ok(),
                        false => check.is_some_and(|check| check.ready),
                    },
                    sfall: match current {
                        true => config.sfall,
                        false => stored.and_then(|scope| scope.sfall),
                    }
                    .unwrap_or(false),
                    mods: match current {
                        true => self.mods.enabled(),
                        false => check.map_or(0, |check| check.mods),
                    },
                    last_played: last == Some(name.as_str()),
                    name,
                }
            })
            .collect()
    }

    /// Check every profile but the active one again.
    fn refresh_cards(&mut self) {
        let config = &self.config;
        let active = config.active_profile();
        let checks = config
            .profile_names()
            .into_iter()
            .filter(|(name, _)| name != active)
            .filter_map(|(name, edition)| {
                let scope = stored_scope(config, &name)?;
                let mut mods = Mods::load(scope.mod_list.as_ref());
                mods.check = self.check_mods(&mods);
                let own = scope
                    .install_dir
                    .as_ref()
                    .and_then(|dir| config::discover_game(std::slice::from_ref(dir), edition));
                let paths = own.unwrap_or_else(|| config.profile(edition));
                let checks = validate::install(
                    paths.master_dat.as_deref(),
                    paths.critter_dat.as_deref(),
                    paths.data_root.as_deref(),
                    edition,
                );
                Some(CardCheck {
                    ready: validate::install_ready(&checks).is_ok() && mods.ready().is_ok(),
                    mods: mods.enabled(),
                    name,
                })
            })
            .collect();
        self.card_checks = checks;
    }

    /// The active profile's Enhanced settings as a launch reads them. The view
    /// draws these, not the top-level keys.
    pub fn active_enhanced(&self) -> ActiveEnhanced {
        ActiveEnhanced::new(&self.config, self.config.enhanced)
    }

    /// The preset, built-in or custom, the active settings match.
    pub fn active_preset(&self) -> Option<PresetMatch> {
        self.active_enhanced().matching(&self.config.custom_presets)
    }

    /// The live preview, for the right pane.
    pub fn preview_canvas(&self) -> &SettingsPreviewCanvas {
        &self.preview
    }

    /// What the preview should show: the active settings, with the value the
    /// hovered widget would give in place of its own.
    fn preview_settings(&self) -> PreviewSettings {
        let own = self.config.enhanced;
        let own = match self.preview_hover {
            Some(Widget::BuiltinPreset(preset)) => preset.settings(),
            Some(Widget::CustomPreset(at)) => self
                .config
                .custom_presets
                .get(at)
                .map_or(own, |preset| preset.settings),
            Some(widget) => self.stepped(own, widget, 1),
            None => own,
        };
        let active = ActiveEnhanced::new(&self.config, own);
        PreviewSettings {
            enhanced: active.preset == Preset::Enhanced,
            viewport: active.settings.viewport,
            zoom: active.settings.default_zoom,
            roof_peek: active.settings.roof_peek,
            tactical_overlay: active.tactical_overlay,
        }
    }

    /// Tell the preview when what it shows has changed.
    fn show_preview(&mut self) {
        let settings = self.preview_settings();
        if settings != self.previewed {
            self.previewed = settings;
            self.preview.set(settings);
        }
    }

    /// Advance the launcher's animations. Whether it needs drawing again.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        // Its clock runs either way; only a preview in view asks for a frame.
        self.preview.tick(now_ms) && view::preview_shown(self)
    }

    pub fn title(&self) -> String {
        "Vault Engine Studio".into()
    }

    /// Make `game` the active game, as the Game row does (the gallery's
    /// launcher picture).
    pub(crate) fn select_game(&mut self, game: GameProfile) {
        if self.config.active_game() != game {
            self.switch_game(game);
            self.show_preview();
        }
    }

    /// Opened by the selector's Setup, on `game`.
    pub fn opened_by_selector(mut self, game: GameProfile) -> Self {
        self.from_selector = true;
        self.select_game(game);
        self
    }

    /// The game its Game row shows.
    pub fn game(&self) -> GameProfile {
        self.config.active_game()
    }

    /// Whether the start found the game data on disk: the launcher shows
    /// it before it is played.
    pub fn discovered(&self) -> bool {
        self.discovered
    }

    /// Whether the game data and the mod stack can start the game, or the
    /// first reason they cannot.
    pub fn ready(&self) -> Result<(), String> {
        validate::install_ready(&self.checks)?;
        self.mods.ready()
    }

    /// The settings to launch with: the edited ones under the active profile,
    /// on the fields' paths made absolute. Those are what was validated, so
    /// they stand even where the settings could not keep them.
    pub fn launch_config(&mut self) -> Config {
        self.apply_fields();
        let mut config = self.config.clone();
        let paths = self.field_paths();
        if config.launch_paths(config.active_game()) != paths {
            config.install_dir = None;
        }
        config.set_paths(paths);
        config
    }

    /// The hand-off failed: the launcher shows why.
    pub fn launch_failed(&mut self, error: impl std::fmt::Display) {
        self.report(State::Bad, format!("The game did not start: {error}"));
    }

    fn report(&mut self, state: State, text: impl Into<String>) {
        self.status = text.into();
        self.status_state = state;
    }

    pub fn handle(&mut self, input: LauncherInput) -> Option<LauncherAction> {
        let action = self.input(input);
        self.show_preview();
        action
    }

    fn input(&mut self, input: LauncherInput) -> Option<LauncherAction> {
        if !matches!(input, LauncherInput::Pointer(_)) {
            // Its value may be the active one now: the next is not what was
            // asked for until the pointer comes back.
            self.preview_hover = None;
        }
        match input {
            LauncherInput::Key { key, shift } if self.browser.is_some() => {
                self.browser_key(key, shift)
            }
            LauncherInput::Key { key, shift } => return self.key(key, shift),
            LauncherInput::Pointer(point) => {
                let hover = self.hit(point);
                if hover != self.hover {
                    self.preview_hover = hover;
                }
                self.hover = hover;
            }
            LauncherInput::Press(point) => return self.press(point),
            LauncherInput::Wheel(lines) => self.wheel(lines),
            LauncherInput::Dropped(path) => self.dropped(&path),
        }
        None
    }

    fn key(&mut self, key: KeyCode, shift: bool) -> Option<LauncherAction> {
        let focus = self.focus;
        if let Some(field) = self.text_mut(focus)
            && let Some(changed) = field.edit(key, field_text(key, shift))
        {
            if changed && matches!(focus, Widget::Field(_)) {
                self.revalidate();
            }
            return None;
        }
        let delta = match key {
            KeyCode::ArrowUp | KeyCode::ArrowLeft => -1,
            _ => 1,
        };
        match key {
            KeyCode::Tab => self.move_focus(if shift { -1 } else { 1 }),
            KeyCode::ArrowUp | KeyCode::ArrowDown if focus == Widget::Layers => {
                if shift {
                    self.move_layer(delta);
                } else {
                    let rows = List::rows(view::LAYERS);
                    self.mods.list.step(delta, self.mods.entries.len(), rows);
                }
            }
            KeyCode::ArrowLeft | KeyCode::ArrowRight => match focus {
                Widget::Tab(_) => self.switch_tab(delta),
                Widget::Game | Widget::Scaling | Widget::Preset | Widget::Viewport => {
                    return self.activate(focus);
                }
                Widget::Zoom
                | Widget::RoofPeek
                | Widget::UiScale
                | Widget::GameSpeed
                | Widget::HotkeyHints => self.choose(focus, delta),
                _ => self.move_focus(delta),
            },
            KeyCode::ArrowUp | KeyCode::ArrowDown => self.move_focus(delta),
            KeyCode::Delete if focus == Widget::Layers => self.edit_layers(Edit::Remove),
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space => return self.activate(focus),
            _ => {}
        }
        None
    }

    fn browser_key(&mut self, key: KeyCode, shift: bool) {
        let Some(browser) = &mut self.browser else {
            return;
        };
        let len = browser.entries.len();
        let rows = List::rows(view::BROWSE_LIST);
        let step = match key {
            KeyCode::ArrowUp => -1,
            KeyCode::ArrowDown => 1,
            KeyCode::PageUp => -(rows as isize),
            KeyCode::PageDown => rows as isize,
            KeyCode::Home => -(len as isize),
            KeyCode::End => len as isize,
            _ => 0,
        };
        if step != 0 {
            browser.list.step(step, len, rows);
            self.focus = Widget::BrowseList;
            return;
        }
        match key {
            KeyCode::Escape => self.close_browser(),
            KeyCode::KeyP => self.toggle_pin(),
            KeyCode::Backspace => {
                browser.up();
                browser.list.show(browser.entries.len(), rows);
            }
            KeyCode::Tab => self.move_focus(if shift { -1 } else { 1 }),
            KeyCode::ArrowLeft => self.move_focus(-1),
            KeyCode::ArrowRight => self.move_focus(1),
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space => {
                self.activate(self.focus);
            }
            _ => {}
        }
    }

    fn text_mut(&mut self, widget: Widget) -> Option<&mut TextField> {
        match widget {
            Widget::Field(field) => Some(&mut self.fields[field.index()]),
            Widget::NewLayer => Some(&mut self.mods.new_layer),
            _ => None,
        }
    }

    /// The focusable widgets in focus order.
    fn order(&self) -> Vec<Widget> {
        view::layout(self)
            .into_iter()
            .map(|(widget, _)| widget)
            .collect()
    }

    fn move_focus(&mut self, delta: isize) {
        let order = self.order();
        let at = order.iter().position(|widget| *widget == self.focus);
        let next = match at {
            Some(at) => (at as isize + delta).rem_euclid(order.len() as isize) as usize,
            None => 0,
        };
        if let Some(widget) = order.get(next) {
            self.focus = *widget;
        }
    }

    /// The next tab the view shows.
    fn switch_tab(&mut self, delta: isize) {
        let tabs: Vec<Tab> = self
            .order()
            .into_iter()
            .filter_map(|widget| match widget {
                Widget::Tab(tab) => Some(tab),
                _ => None,
            })
            .collect();
        let at = tabs.iter().position(|tab| *tab == self.tab).unwrap_or(0);
        let next = (at as isize + delta).rem_euclid(tabs.len().max(1) as isize);
        if let Some(tab) = tabs.get(next as usize) {
            self.tab = *tab;
            self.focus = Widget::Tab(*tab);
        }
    }

    fn hit(&self, point: [i32; 2]) -> Option<Widget> {
        view::layout(self)
            .into_iter()
            .find(|(_, rect)| rect.contains(point))
            .map(|(widget, _)| widget)
    }

    fn rect(&self, widget: Widget) -> Option<widgets::Rect> {
        view::layout(self)
            .into_iter()
            .find(|(candidate, _)| *candidate == widget)
            .map(|(_, rect)| rect)
    }

    fn press(&mut self, point: [i32; 2]) -> Option<LauncherAction> {
        let widget = self.hit(point)?;
        self.focus = widget;
        let rect = self.rect(widget)?;
        match widget {
            Widget::Field(_) | Widget::NewLayer => {
                if let Some(field) = self.text_mut(widget) {
                    field.click(rect, point[0]);
                }
            }
            Widget::Layers => {
                let len = self.mods.entries.len();
                if let Some(row) = self.mods.list.row_at(rect, point[1], len) {
                    self.mods.list.selected = row;
                    // The `[x]` box toggles the layer.
                    if point[0] < rect.x + 26 {
                        self.edit_layers(Edit::Toggle);
                    }
                }
            }
            Widget::BrowseList => {
                let browser = self.browser.as_mut()?;
                let row = browser.list.row_at(rect, point[1], browser.entries.len())?;
                // A press on the selected entry opens it.
                if row == browser.list.selected {
                    self.browse_enter();
                } else {
                    browser.list.selected = row;
                }
            }
            Widget::Zoom | Widget::UiScale | Widget::GameSpeed
                if view::slider_track(rect).contains(point) =>
            {
                self.choose_stop(widget, view::slider_track(rect), point[0]);
            }
            _ => return self.activate(widget),
        }
        None
    }

    fn wheel(&mut self, lines: f64) {
        let step = if lines > 0.0 {
            -3
        } else if lines < 0.0 {
            3
        } else {
            return;
        };
        if let Some(browser) = &mut self.browser {
            let rows = List::rows(view::BROWSE_LIST);
            browser.list.step(step, browser.entries.len(), rows);
        } else if matches!(self.tab, Tab::Data | Tab::Mods) {
            let rows = List::rows(view::LAYERS);
            self.mods.list.step(step, self.mods.entries.len(), rows);
        }
    }

    fn activate(&mut self, widget: Widget) -> Option<LauncherAction> {
        let config = &mut self.config;
        match widget {
            Widget::Tab(tab) => self.tab = tab,
            Widget::Field(_) => self.move_focus(1),
            Widget::Browse(field) => self.open_browser(Target::Field(field)),
            Widget::Scaling => {
                config.scaling = Some(config.scaling.unwrap_or(ScalingMode::AspectFit).toggled());
            }
            Widget::Game => {
                let game = match config.active_game() {
                    GameProfile::Fallout1 => GameProfile::Fallout2,
                    GameProfile::Fallout2 => GameProfile::Fallout1,
                };
                self.switch_game(game);
            }
            Widget::Fullscreen => config.fullscreen = Some(!config.fullscreen.unwrap_or(false)),
            Widget::TouchUi => config.touch_ui = Some(!config.touch_ui.unwrap_or(false)),
            Widget::Audio => config.audio = Some(!config.audio.unwrap_or(true)),
            Widget::Debug => config.debug = Some(!config.debug.unwrap_or(false)),
            Widget::Preset
            | Widget::TacticalOverlay
            | Widget::Viewport
            | Widget::Zoom
            | Widget::SmoothCamera
            | Widget::RoofPeek
            | Widget::UiScale
            | Widget::GameSpeed
            | Widget::HotkeyHints => self.choose(widget, 1),
            Widget::Layers | Widget::LayerToggle => self.edit_layers(Edit::Toggle),
            Widget::LayerUp => self.move_layer(-1),
            Widget::LayerDown => self.move_layer(1),
            Widget::LayerRemove => self.edit_layers(Edit::Remove),
            Widget::NewLayer | Widget::AddLayer => {
                let text = self.mods.new_layer.text.trim().to_owned();
                if text.is_empty() {
                    self.report(State::Bad, "Type a folder or .DAT path to add it.");
                } else if self.add_layer(text) {
                    self.mods.new_layer = TextField::default();
                }
            }
            Widget::BrowseLayer => self.open_browser(Target::Layer),
            Widget::Save => match self.save() {
                Ok(path) if self.apply_fields() => {
                    self.report(State::Ok, format!("Saved {}.", path.display()));
                }
                Ok(path) => self.report(
                    State::Bad,
                    format!("Saved {}, without the paths: {UNKEPT}", path.display()),
                ),
                Err(error) => self.report(State::Bad, format!("Not saved: {error}")),
            },
            Widget::Play => return self.play(LauncherAction::Play),
            Widget::PlayDebug => return self.play(LauncherAction::PlayDebug),
            Widget::Quit if self.from_selector => return Some(LauncherAction::Back),
            Widget::Quit => return Some(LauncherAction::Quit),
            Widget::BrowseList => self.browse_enter(),
            Widget::BrowseUse => {
                let browser = self.browser.as_ref()?;
                let (target, dir) = (browser.target, browser.dir.clone());
                self.pick_folder(target, dir);
            }
            Widget::BrowseCancel => self.close_browser(),
            Widget::ProfileCard(at) => {
                self.select_card(at);
            }
            Widget::ProfilePlay(at) => return self.play_card(at, LauncherAction::Play),
            Widget::ProfileResume(at) => return self.play_card(at, LauncherAction::ResumeLast),
            Widget::ProfileAutoplay(at) => {
                return self.play_card(at, LauncherAction::AutoplayTest);
            }
            Widget::NewProfile => self.new_profile(),
            Widget::BuiltinPreset(preset) => {
                self.config.enhanced = preset.settings();
                self.report(State::Ok, format!("{}: Save keeps it.", preset.label()));
            }
            Widget::SaveCustomPreset => self.save_custom_preset(),
            Widget::CustomPreset(at) => {
                if let Some(preset) = self.config.custom_presets.get(at) {
                    let text = format!("{}: Save keeps it.", preset.name);
                    self.config.enhanced = preset.settings;
                    self.report(State::Ok, text);
                }
            }
            Widget::AutoDetect => self.auto_detect(),
            Widget::KnownPath(at) => {
                if self.browser.as_mut()?.open_shortcut(at) {
                    self.focus = Widget::BrowseList;
                }
            }
        }
        None
    }

    // --- Profiles ------------------------------------------------------

    /// Make the profile at `at` of [`Launcher::profile_cards`] active. Whether
    /// it now is: unsaved Mods edits belong to the profile they were made in,
    /// so they stop the change, as they stop the Game toggle.
    fn select_card(&mut self, at: usize) -> bool {
        match self.config.profile_names().into_iter().nth(at) {
            Some((name, _)) => self.select_named(&name),
            None => false,
        }
    }

    fn select_named(&mut self, name: &str) -> bool {
        if name == self.config.active_profile() {
            return true;
        }
        if self.mods_hold_profile() {
            return false;
        }
        let kept = self.apply_fields();
        if !self.config.select_profile(name) {
            return false;
        }
        self.show_profile(&profile_title(name, self.config.active_game()));
        if !kept {
            self.report(
                State::Bad,
                format!("The last profile's paths were not kept: {UNKEPT}"),
            );
        }
        true
    }

    /// Whether unsaved Mods edits stop a change of profile, which then says so.
    fn mods_hold_profile(&mut self) -> bool {
        if self.mods.dirty {
            self.report(
                State::Bad,
                "Save the Mods tab's changes before changing the profile.",
            );
        }
        self.mods.dirty
    }

    /// The card's button: its profile, then `action`. A card that could not be
    /// selected starts nothing, least of all the profile that stayed active.
    fn play_card(&mut self, at: usize, action: LauncherAction) -> Option<LauncherAction> {
        if self.select_card(at) {
            self.play(action)
        } else {
            None
        }
    }

    /// A new empty custom profile of the active edition, selected. It reads
    /// its edition's paths until it has its own.
    fn new_profile(&mut self) {
        if self.mods_hold_profile() {
            return;
        }
        let names = self.config.profile_names();
        let free = |number: &usize| {
            let name = format!("profile-{number}");
            !names.iter().any(|(known, _)| *known == name)
        };
        let number = (1..).find(free).unwrap_or_default();
        let name = format!("profile-{number}");
        let edition = self.config.active_game();
        match self.config.add_custom_profile(&name, edition) {
            Ok(()) => {
                self.select_named(&name);
            }
            Err(error) => self.report(State::Bad, error),
        }
    }

    /// Keep the active settings as a new preset, with every key named. A
    /// second press does not keep a second copy.
    fn save_custom_preset(&mut self) {
        let active = self.active_enhanced();
        if let Some(PresetMatch::Custom(at)) = self.active_preset() {
            let name = &self.config.custom_presets[at].name;
            let text = format!("These settings are the preset {name}.");
            return self.report(State::Unset, text);
        }
        let presets = &self.config.custom_presets;
        let free = |number: &usize| {
            let name = format!("custom-{number}");
            !presets.iter().any(|preset| preset.name == name)
        };
        let number = (1..).find(free).unwrap_or_default();
        let name = format!("custom-{number}");
        match self.config.save_custom_preset(&name, active.profile()) {
            Ok(()) => self.report(State::Ok, format!("Preset {name}: Save keeps it.")),
            Err(error) => self.report(State::Bad, error),
        }
    }

    /// The folder [`Widget::AutoDetect`] looks at: the one the browser is
    /// showing, or the first field's, or the profile's own.
    fn named_folder(&self) -> PathBuf {
        let field = Field::ALL.into_iter().find_map(|field| self.path(field));
        let browsed = self.browser.as_ref().map(|browser| browser.dir.clone());
        browsed
            .or_else(|| {
                field.map(|path| match path.parent() {
                    Some(parent) if !path.is_dir() => parent.to_path_buf(),
                    _ => path,
                })
            })
            .or_else(|| self.config.install_dir.clone())
            .unwrap_or_else(|| self.start_dir.clone())
    }

    /// A custom profile takes the edition of the data it is given, keeping
    /// its own scope. An edition's profile cannot: whether the active profile
    /// is of `edition` now.
    fn take_edition(&mut self, edition: GameProfile) -> bool {
        let old = self.config.active_game();
        if old == edition {
            return true;
        }
        let name = self.config.active_profile().to_owned();
        if config::parse_game(&name).is_some() {
            return false;
        }
        self.config.select_profile(config::game_name(old));
        for custom in &mut self.config.custom_profiles {
            if custom.name == name {
                custom.edition = edition;
            }
        }
        self.config.select_profile(&name)
    }

    /// Fill the active profile from the folder named: its paths, `sfall` by
    /// `ddraw.ini` standing there (a setting of Fallout 2's alone), its `mods`
    /// folder as a layer, and the folders it links.
    fn auto_detect(&mut self) {
        let dir = self.named_folder();
        let found = validate::detect_install(&dir, &self.config.known_paths);
        let notes = found.notes.join(" ");
        if found.master.is_none() && found.data_root.is_none() {
            let text = format!("No game data in {}. {notes}", dir.display());
            return self.report(State::Bad, text);
        }
        // Et Tu's folder goes to Et Tu's profile, whichever card was showing.
        // Unsaved Mods edits keep the card, and `select_named` says why.
        if found.fallout1in2 && !self.select_named(config::ETTU_PROFILE) {
            return;
        }
        if let Some(edition) = found.edition
            && !self.take_edition(edition)
        {
            let name = title(edition);
            let text = format!(
                "{} holds {name} data: select a {name} profile first.",
                dir.display()
            );
            return self.report(State::Bad, text);
        }
        self.browser = None;
        let game = self.config.active_game();
        self.config.install_dir = found
            .install_dir
            .as_ref()
            .map(|dir| std::path::absolute(dir).unwrap_or_else(|_| dir.clone()));
        if !found.secondary_dat_dirs.is_empty() {
            self.config.secondary_dat_dirs = found.secondary_dat_dirs;
        }
        if game == GameProfile::Fallout2 {
            self.config.sfall = Some(found.sfall_ini);
        }
        self.load_fields(&GamePaths {
            master_dat: found.master,
            critter_dat: found.critter,
            data_root: found.data_root,
        });
        self.revalidate();
        if let Some(dir) = &found.install_dir {
            self.remember(known_kind(game), dir);
        }
        if let Some(dir) = &found.mods_dir {
            self.remember(KnownKind::Mods, dir);
            self.add_mods_folder(dir);
        }
        self.refresh_cards();
        let state = match self.ready() {
            Ok(()) => State::Ok,
            Err(_) => State::Unset,
        };
        self.report(state, format!("Detected in {}. {notes}", dir.display()));
    }

    /// `dir` as a layer of the load order, once. A protected list stays as it
    /// is.
    fn add_mods_folder(&mut self, dir: &Path) {
        let base = self.mods_dir();
        let mut paths = self.mods.entries.iter().flat_map(|entry| &entry.path);
        let listed = paths.any(|path| base.join(path) == dir);
        if listed || self.mods.broken || !self.editable() {
            return;
        }
        let path = dir.to_string_lossy().into_owned();
        self.mods.entries.push(LayerEntry {
            path: Some(path),
            ..LayerEntry::default()
        });
        self.place_layers();
        self.layers_changed();
    }

    // --- Remembered folders --------------------------------------------

    /// Put `path` first among the folders of `kind`. One the settings could
    /// not write is left out, or it would stop every later Save.
    fn remember(&mut self, kind: KnownKind, path: &Path) {
        if let Ok(path) = std::path::absolute(path)
            && path.to_str().is_some()
        {
            self.config.known_paths.remember(kind, &path);
        }
    }

    /// The folder of `field`, when it validates.
    fn remember_field(&mut self, field: Field) {
        let Some(path) = self
            .path(field)
            .filter(|_| self.checks[field.index()].is_ok())
        else {
            return;
        };
        let path = std::path::absolute(&path).unwrap_or(path);
        let folder = match field {
            Field::DataRoot => Some(path.as_path()),
            Field::Master | Field::Critter => path.parent(),
        };
        if let Some(folder) = folder {
            self.remember(known_kind(self.config.active_game()), folder);
        }
    }

    /// The folder of the layer at `at`, a `.DAT`'s for a `.DAT`, when it
    /// validates.
    fn remember_layer(&mut self, at: usize) {
        let valid = self.mods.check.layers.get(at).is_some_and(Check::is_ok);
        let path = self
            .mods
            .entries
            .get(at)
            .and_then(|entry| entry.path.clone());
        let Some(path) = path.filter(|_| valid) else {
            return;
        };
        let full = self.mods_dir().join(&path);
        let folder = match path.to_ascii_lowercase().ends_with(".dat") {
            true => full.parent(),
            false => Some(full.as_path()),
        };
        if let Some(folder) = folder {
            self.remember(KnownKind::Mods, folder);
        }
    }

    /// Everything that validates now, the game's own folder first.
    fn remember_valid(&mut self) {
        for field in [Field::DataRoot, Field::Critter, Field::Master] {
            self.remember_field(field);
        }
        for at in 0..self.mods.entries.len() {
            self.remember_layer(at);
        }
    }

    /// The folders the browser offers.
    fn shortcuts(&self) -> Vec<Shortcut> {
        let env = |name: &str| std::env::var(name).ok();
        let common = browse::common_install_dirs(config::home_dir().as_deref(), &env);
        browse::shortcuts(&self.config.known_paths, &common, &browse::roots())
    }

    // --- Settings ------------------------------------------------------

    /// The next (`delta` 1) or previous (-1) choice of an enhanced
    /// presentation widget, wrapping: the viewport and camera smoothing
    /// toggle, the zoom steps through [`ZOOM_STEPS`], the roof peek cycles
    /// as Alt+R does, the interface scale steps through [`UI_SCALES`], and
    /// the game speed through 1.0, 1.5 and 2.0 as Alt+F does.
    /// A value from the file between two steps moves from the nearest.
    /// What is a profile's goes in its table, so that another profile's stays.
    fn choose(&mut self, widget: Widget, delta: isize) {
        if widget == Widget::HotkeyHints {
            let values = HotkeyHints::ALL;
            let shown = self.config.hotkey_hints.unwrap_or_default();
            let at = values.iter().position(|value| *value == shown).unwrap_or(0);
            let next = (at as isize + delta).rem_euclid(values.len() as isize);
            self.config.hotkey_hints = Some(values[next as usize]);
        } else {
            self.config.enhanced = self.stepped(self.config.enhanced, widget, delta);
        }
    }

    /// The stop of a slider's `track` nearest the frame x.
    fn choose_stop(&mut self, widget: Widget, track: widgets::Rect, x: i32) {
        let speeds = crate::enhanced::GameSpeed::ALL;
        let stop = |stops: usize| widgets::slider_stop(track, x, stops);
        let own = &mut self.config.enhanced;
        match widget {
            Widget::Zoom => own.default_zoom = Some(ZOOM_STEPS[stop(ZOOM_STEPS.len())]),
            Widget::UiScale => {
                let scale = UI_SCALES[stop(UI_SCALES.len())];
                own.ui_scale = Some(config::UiScale::from_factor(scale));
            }
            Widget::GameSpeed => own.game_speed = Some(speeds[stop(speeds.len())]),
            _ => {}
        }
    }

    /// `own`, a profile's table, with `widget` moved `delta` choices on. Any
    /// widget that holds none of it leaves it as it is.
    fn stepped(&self, mut own: EnhancedProfile, widget: Widget, delta: isize) -> EnhancedProfile {
        let active = ActiveEnhanced::new(&self.config, own);
        let settings = active.settings;
        let step = |at: usize, len: usize| (at as isize + delta).rem_euclid(len as isize) as usize;
        let nearest = |values: &mut dyn Iterator<Item = f32>, value: f32| {
            values
                .enumerate()
                .min_by(|(_, a), (_, b)| (a - value).abs().total_cmp(&(b - value).abs()))
                .map_or(0, |(at, _)| at)
        };
        match widget {
            Widget::Preset => {
                own.preset = Some(match active.preset {
                    Preset::Vanilla => Preset::Enhanced,
                    Preset::Enhanced => Preset::Vanilla,
                });
            }
            Widget::TacticalOverlay => own.tactical_overlay = Some(!active.tactical_overlay),
            Widget::Viewport => {
                own.viewport_mode = Some(match settings.viewport {
                    ViewportMode::Fixed => ViewportMode::Unlocked,
                    ViewportMode::Unlocked => ViewportMode::Fixed,
                });
            }
            Widget::SmoothCamera => own.smooth_camera = Some(!settings.smooth_camera),
            Widget::Zoom => {
                let at = nearest(&mut ZOOM_STEPS.into_iter(), settings.default_zoom);
                own.default_zoom = Some(ZOOM_STEPS[step(at, ZOOM_STEPS.len())]);
            }
            Widget::RoofPeek => {
                let mut mode = settings.roof_peek;
                // Backwards is two steps forwards in the three-mode cycle.
                for _ in 0..step(0, 3) {
                    mode = mode.next();
                }
                own.roof_peek = Some(mode);
            }
            Widget::UiScale => {
                // `auto` sorts before every number.
                let at = match settings.ui_scale {
                    None => 0,
                    Some(scale) => {
                        1 + nearest(&mut UI_SCALES[1..].iter().flatten().copied(), scale)
                    }
                };
                let next = UI_SCALES[step(at, UI_SCALES.len())];
                own.ui_scale = Some(config::UiScale::from_factor(next));
            }
            Widget::GameSpeed => {
                let speeds = crate::enhanced::GameSpeed::ALL;
                let at = speeds
                    .iter()
                    .position(|speed| *speed == active.game_speed)
                    .unwrap_or(0);
                own.game_speed = Some(speeds[step(at, speeds.len())]);
            }
            _ => {}
        }
        own
    }

    /// Play: the settings are saved, then the shell starts the game; a
    /// launcher that is not ready says why instead.
    fn play(&mut self, action: LauncherAction) -> Option<LauncherAction> {
        if let Err(reason) = self.ready() {
            self.report(State::Bad, format!("Not ready. {reason}"));
            return None;
        }
        // Decision D-36: Play from Setup is the selector's choice.
        if self.from_selector {
            let game = config::game_name(self.config.active_game());
            self.config.last_played = Some(game.to_owned());
        }
        let unsaved_mods = self.mods.dirty;
        match self.save() {
            Ok(path) => self.report(State::Ok, format!("Saved {}; starting.", path.display())),
            // The load order the game reads would be the old one.
            Err(error) if unsaved_mods => {
                self.report(State::Bad, format!("Not saved, so not started: {error}"));
                return None;
            }
            Err(error) => {
                tracing::warn!(%error, "launcher settings not saved");
                self.report(State::Bad, format!("Not saved ({error}); starting."));
            }
        }
        Some(action)
    }

    // --- Game data ---------------------------------------------------

    fn path(&self, field: Field) -> Option<PathBuf> {
        let text = self.fields[field.index()].text.trim();
        (!text.is_empty()).then(|| PathBuf::from(text))
    }

    /// Validate the three fields again.
    fn revalidate(&mut self) {
        let [master, critter, root] = Field::ALL.map(|field| self.path(field));
        let game = self.config.active_game();
        self.checks =
            validate::install(master.as_deref(), critter.as_deref(), root.as_deref(), game);
    }

    /// The Game toggle: the fields are kept as the old game's profile and
    /// show the new game's, or, when it has none yet, its data discovered
    /// in its own directories; then they validate in its format.
    ///
    /// Each game has its own load order, so the Mods tab follows the toggle.
    /// Unsaved Mods edits belong to the old game: the toggle waits for Save
    /// rather than dropping them or writing them into the new game's list.
    /// The profile's scope goes with it ([`Config::switch_game`]).
    fn switch_game(&mut self, game: GameProfile) {
        if self.mods.dirty {
            self.report(
                State::Bad,
                "Save the Mods tab's changes before changing the game.",
            );
            return;
        }
        self.apply_fields();
        self.config.switch_game(game);
        self.show_profile(title(game));
    }

    /// The Mods tab, the fields and the cards of the profile just made active,
    /// `name` in the messages.
    fn show_profile(&mut self, name: &str) {
        self.mods = Mods::load(self.config.mod_list.as_ref());
        self.revalidate_mods();
        let paths = self.config.launch_paths(self.config.active_game());
        self.load_fields(&paths);
        let found = if paths.is_empty() {
            self.discover_fields()
        } else {
            None
        };
        self.revalidate();
        self.refresh_cards();
        let source = match found {
            Some(dir) => format!("{name} data found in {}", dir.display()),
            None if paths.is_empty() => format!("No {name} data found"),
            None => format!("{name} paths restored"),
        };
        match self.ready() {
            Ok(()) => self.report(State::Ok, format!("{source}: ready to play.")),
            Err(reason) => self.report(State::Unset, format!("{source}. {reason}")),
        }
    }

    /// The active game's data discovered in its own directories fills the
    /// fields; returns the folder it was found in.
    fn discover_fields(&mut self) -> Option<PathBuf> {
        let game = self.config.active_game();
        let found = config::discover_game(self.data_dirs.get(game), game)?;
        self.load_fields(&found);
        found
            .master_dat
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
    }

    fn load_fields(&mut self, paths: &GamePaths) {
        for (field, path) in
            Field::ALL
                .into_iter()
                .zip([&paths.master_dat, &paths.critter_dat, &paths.data_root])
        {
            self.fields[field.index()] = TextField::new(
                path.as_ref()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            );
        }
    }

    fn set_field(&mut self, field: Field, path: &Path) {
        self.fields[field.index()] = TextField::new(path.to_string_lossy());
        self.revalidate();
    }

    /// The fields' paths, made absolute (relative ones are relative to the
    /// working directory, as they were validated).
    fn field_paths(&self) -> GamePaths {
        let [master_dat, critter_dat, data_root] = Field::ALL.map(|field| {
            self.path(field)
                .map(|path| std::path::absolute(&path).unwrap_or(path))
        });
        GamePaths {
            master_dat,
            critter_dat,
            data_root,
        }
    }

    /// The fields into the settings, so that a launch reads what they say.
    /// An `install_dir` wins at a launch, so one that says otherwise goes.
    ///
    /// The top-level paths are the edition's, so a custom profile leaves them
    /// alone and keeps a folder of its own instead. Whether the settings hold
    /// the fields now: a custom profile has no way to keep paths that are not
    /// one game folder.
    fn apply_fields(&mut self) -> bool {
        let paths = self.field_paths();
        let game = self.config.active_game();
        let found = |dir: &Path| config::discover_game(&[dir.to_path_buf()], game);
        let custom = config::parse_game(self.config.active_profile()).is_none();
        let installed = self.config.install_dir.as_deref().and_then(found);
        if installed.as_ref() == Some(&paths) {
            if !custom {
                self.config.set_paths(paths);
            }
            return true;
        }
        if !custom || paths.is_empty() || paths == self.config.paths() {
            // A folder that is gone for now stays named.
            if installed.is_some() {
                self.config.install_dir = None;
            }
            if !custom {
                self.config.set_paths(paths);
            }
            return true;
        }
        let folder = paths.master_dat.as_deref().and_then(Path::parent);
        match folder.filter(|dir| found(dir).as_ref() == Some(&paths)) {
            Some(dir) => {
                self.config.install_dir = Some(dir.to_path_buf());
                true
            }
            None => false,
        }
    }

    /// A game folder holding MASTER.DAT and CRITTER.DAT fills all three
    /// fields; returns whether `dir` is one.
    fn fill_install(&mut self, dir: &Path) -> bool {
        let Some(found) = config::discover(&[dir.to_path_buf()]) else {
            return false;
        };
        for (field, path) in [
            (Field::Master, found.master_dat),
            (Field::Critter, found.critter_dat),
            (Field::DataRoot, found.data_root),
        ] {
            if let Some(path) = path {
                self.fields[field.index()] = TextField::new(path.to_string_lossy());
            }
        }
        self.revalidate();
        self.report(State::Ok, format!("Game data found in {}.", dir.display()));
        true
    }

    fn dropped(&mut self, path: &Path) {
        self.browser = None;
        // One pane shows the paths and the load order: the focus says which.
        let layers = matches!(
            self.focus,
            Widget::Layers
                | Widget::LayerUp
                | Widget::LayerDown
                | Widget::LayerToggle
                | Widget::LayerRemove
                | Widget::NewLayer
                | Widget::AddLayer
                | Widget::BrowseLayer
        );
        let tab = if layers && self.tab == Tab::Data {
            Tab::Mods
        } else {
            self.tab
        };
        match route_drop(path, path.is_dir(), tab) {
            DropTarget::Layer => {
                let text = self.layer_text(path);
                self.add_layer(text);
            }
            DropTarget::Field(Field::DataRoot) if self.fill_install(path) => {}
            DropTarget::Field(field) => {
                self.set_field(field, path);
                self.tab = Tab::Data;
                self.focus = Widget::Field(field);
                self.report(State::Ok, format!("Dropped {}.", path.display()));
            }
            DropTarget::Rejected => self.report(
                State::Bad,
                "Drop a .DAT file or a folder (on the Mods tab: a mod layer).",
            ),
        }
    }

    // --- Browser -------------------------------------------------------

    fn open_browser(&mut self, target: Target) {
        let start = match target {
            Target::Field(field) => self.path(field).map(|path| {
                if path.is_dir() {
                    path
                } else {
                    path.parent().map(Path::to_path_buf).unwrap_or_default()
                }
            }),
            Target::Layer => Some(self.mods_dir()),
        }
        .filter(|dir| dir.is_dir())
        .unwrap_or_else(|| self.start_dir.clone());
        let mut browser = Browser::open(start, target);
        browser.set_shortcuts(self.shortcuts());
        self.browser = Some(browser);
        self.focus = Widget::BrowseList;
    }

    /// `P` in the browser: pin the folder on show, or unpin it.
    fn toggle_pin(&mut self) {
        let Some(dir) = self.browser.as_ref().map(|browser| browser.dir.clone()) else {
            return;
        };
        let known = &mut self.config.known_paths;
        let pinned = known.list(config::KnownKind::Pinned).contains(&dir);
        if pinned {
            known.forget(config::KnownKind::Pinned, &dir);
        } else {
            known.remember(config::KnownKind::Pinned, &dir);
        }
        let shortcuts = self.shortcuts();
        if let Some(browser) = &mut self.browser {
            browser.set_shortcuts(shortcuts);
        }
        let verb = if pinned { "Unpinned" } else { "Pinned" };
        self.report(State::Ok, format!("{verb} {}.", dir.display()));
    }

    fn close_browser(&mut self) {
        if let Some(browser) = self.browser.take() {
            self.focus = match browser.target {
                Target::Field(field) => Widget::Browse(field),
                Target::Layer => Widget::BrowseLayer,
            };
        }
    }

    fn browse_enter(&mut self) {
        let Some(browser) = &mut self.browser else {
            return;
        };
        let picked = browser.enter();
        browser
            .list
            .show(browser.entries.len(), List::rows(view::BROWSE_LIST));
        let target = browser.target;
        if let Some(file) = picked {
            self.pick_file(target, file);
        }
    }

    fn pick_file(&mut self, target: Target, file: PathBuf) {
        match target {
            Target::Field(Field::DataRoot) => {
                self.report(State::Bad, "Pick a folder: open it, then Use this folder.");
            }
            Target::Field(field) => {
                self.close_browser();
                self.set_field(field, &file);
                self.remember_field(field);
            }
            Target::Layer
                if file
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .ends_with(".dat") =>
            {
                self.close_browser();
                let text = self.layer_text(&file);
                if self.add_layer(text) {
                    self.remember_layer(self.mods.entries.len() - 1);
                }
            }
            Target::Layer => self.report(State::Bad, "A mod layer is a folder or a .DAT file."),
        }
    }

    fn pick_folder(&mut self, target: Target, dir: PathBuf) {
        match target {
            Target::Field(Field::DataRoot) => {
                self.close_browser();
                self.set_field(Field::DataRoot, &dir);
                self.remember_field(Field::DataRoot);
            }
            Target::Field(_) => {
                if self.fill_install(&dir) {
                    self.close_browser();
                    self.remember_field(Field::Master);
                } else {
                    self.report(State::Bad, "No MASTER.DAT and CRITTER.DAT in this folder.");
                }
            }
            Target::Layer => {
                self.close_browser();
                let text = self.layer_text(&dir);
                if self.add_layer(text) {
                    self.remember_layer(self.mods.entries.len() - 1);
                }
            }
        }
    }

    // --- Mods ----------------------------------------------------------

    /// The directory the load order's relative paths are relative to.
    fn mods_dir(&self) -> PathBuf {
        self.dir_of(&self.mods)
    }

    /// The directory `mods`' relative paths are relative to.
    fn dir_of(&self, mods: &Mods) -> PathBuf {
        let parent = |path: &Path| match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
            _ => PathBuf::from("."),
        };
        match (&mods.file, &self.settings_path) {
            (Some(file), _) => parent(file),
            (None, Some(settings)) => parent(settings),
            (None, None) => self
                .save_dirs
                .first()
                .cloned()
                .unwrap_or_else(|| PathBuf::from(".")),
        }
    }

    /// A picked or dropped path as a layer path: relative to the load
    /// order's directory when inside it (with `/` separators).
    fn layer_text(&self, path: &Path) -> String {
        let dir = self.mods_dir();
        let dir = std::path::absolute(&dir).unwrap_or(dir);
        match path.strip_prefix(&dir) {
            Ok(rest) if !rest.as_os_str().is_empty() => rest
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/"),
            _ => path.to_string_lossy().into_owned(),
        }
    }

    /// Whether the list can be edited: an inline `mod_list` becomes a load
    /// order here, and one that cannot be read stays as it is.
    fn editable(&mut self) -> bool {
        if self.mods.inline {
            self.mods.inline = false;
            self.place_layers();
        }
        match self.mods.read_only.clone() {
            Some(reason) => {
                self.report(State::Bad, format!("Read only: {reason}"));
                false
            }
            None => true,
        }
    }

    /// Write the layers as their load order will hold them. Beside the
    /// settings that reaches only the folders below it, which an inline list
    /// and a detected `mods` folder are not bound by: then the list goes in
    /// the per-user folder, whose load order may reach anywhere.
    fn place_layers(&mut self) {
        let dir = self.mods_dir();
        let dir = std::path::absolute(&dir).unwrap_or(dir);
        let full = |entry: &LayerEntry| entry.path.as_ref().map(|path| dir.join(path));
        let paths: Vec<Option<PathBuf>> = self.mods.entries.iter().map(full).collect();
        let outside = paths.iter().flatten().any(|path| {
            let climbs = path.components().any(|part| part.as_os_str() == "..");
            climbs || !path.starts_with(&dir)
        });
        if outside
            && self.mods.file.is_none()
            && let Some(root) = &self.user_root
        {
            self.mods.file = Some(new_load_order(&self.config, &root.join(LOAD_ORDER)));
        }
        let text = |path: &Option<PathBuf>| path.as_ref().map(|path| self.layer_text(path));
        let texts: Vec<Option<String>> = paths.iter().map(text).collect();
        for (entry, text) in self.mods.entries.iter_mut().zip(texts) {
            if text.is_some() {
                entry.path = text;
            }
        }
    }

    fn add_layer(&mut self, path: String) -> bool {
        if !self.editable() {
            return false;
        }
        self.mods.entries.push(LayerEntry {
            path: Some(path.clone()),
            ..LayerEntry::default()
        });
        self.mods.list.selected = self.mods.entries.len() - 1;
        self.tab = Tab::Mods;
        self.layers_changed();
        self.report(State::Ok, format!("Layer {path} added on top."));
        true
    }

    fn move_layer(&mut self, delta: isize) {
        let at = self.mods.list.selected;
        let to = at.wrapping_add_signed(delta);
        if to >= self.mods.entries.len() || at >= self.mods.entries.len() || !self.editable() {
            return;
        }
        self.mods.entries.swap(at, to);
        self.mods.list.selected = to;
        self.layers_changed();
    }

    fn edit_layers(&mut self, edit: Edit) {
        let at = self.mods.list.selected;
        if at >= self.mods.entries.len() || !self.editable() {
            return;
        }
        match edit {
            Edit::Toggle => self.mods.entries[at].enabled ^= true,
            Edit::Remove => {
                self.mods.entries.remove(at);
            }
        }
        self.layers_changed();
    }

    fn layers_changed(&mut self) {
        self.mods.dirty = true;
        self.revalidate_mods();
    }

    fn revalidate_mods(&mut self) {
        self.mods.check = self.check_mods(&self.mods);
        let rows = List::rows(view::LAYERS);
        self.mods.list.show(self.mods.entries.len(), rows);
    }

    fn check_mods(&self, mods: &Mods) -> validate::Mods {
        let directory = self.dir_of(mods);
        let policy = LoadOrderPolicy {
            // An inline list's paths are the user's own, as the game reads
            // them, and so are those of a load order in the per-user folder.
            allow_outside: mods.inline
                || crate::mods::load_order_policy(&directory, self.user_root.as_deref())
                    .allow_outside,
        };
        validate::mods(&directory, &mods.entries, policy)
    }

    // --- Saving --------------------------------------------------------

    /// Write the settings (and an edited load order); an existing settings
    /// file is rewritten in place, else the first save directory that
    /// takes a new one is used.
    pub fn save(&mut self) -> Result<PathBuf, String> {
        self.apply_fields();
        self.remember_valid();
        let candidates: Vec<PathBuf> = match &self.settings_path {
            Some(path) => vec![path.clone()],
            None => self
                .save_dirs
                .iter()
                .map(|dir| dir.join(config::FILE_NAME))
                .collect(),
        };
        let mut failure = String::from("no directory to save the settings in");
        for path in candidates {
            match self.save_to(&path) {
                Ok(()) => {
                    self.settings_path = Some(path.clone());
                    self.refresh_cards();
                    return Ok(path);
                }
                Err(error) => failure = error,
            }
        }
        Err(failure)
    }

    fn save_to(&mut self, path: &Path) -> Result<(), String> {
        let mut config = self.config.clone();
        let mods = &self.mods;
        let mut file = mods.file.clone();
        if mods.dirty && mods.read_only.is_none() {
            if file.is_some() || !mods.entries.is_empty() {
                // A file another profile lists stays that profile's.
                let beside = file.as_deref().unwrap_or(path);
                let own = file.as_deref().filter(|it| !listed_elsewhere(&config, it));
                let target = match own {
                    Some(own) => own.to_path_buf(),
                    None => new_load_order(&config, beside),
                };
                let text = vault_formats::vfs::write_load_order(&mods.entries)
                    .map_err(|error| error.to_string())?;
                config::write_replacing(&target, text.as_bytes())
                    .map_err(|error| error.to_string())?;
                config.mod_list = Some(ModList::File(target.clone()));
                file = Some(target);
            } else {
                // The last layer of an inline list went.
                config.mod_list = None;
            }
        }
        config.save(path).map_err(|error| error.to_string())?;
        self.config = config;
        self.mods.file = file;
        self.mods.dirty = false;
        Ok(())
    }
}

/// Why a custom profile's paths cannot all be kept.
const UNKEPT: &str = "a custom profile keeps one game folder, or its edition's paths.";

#[derive(Debug, Clone, Copy)]
enum Edit {
    Toggle,
    Remove,
}

/// The byte a key types into a launcher field: the shared US-layout text,
/// and the keypad digits.
fn field_text(key: winit::keyboard::KeyCode, shift: bool) -> Option<u8> {
    crate::input::key_text(key, shift).or_else(|| crate::input::key_digit(key))
}
