//! The pre-game launcher: game data, display and audio settings and the
//! mod load order, validated live, then Play. It draws with its own font
//! and palette only, so it works before any game data is found.
//!
//! The state machine is headless: [`Launcher::handle`] takes one input and
//! may answer with an action, and [`Launcher::image`] draws the frame.
//! File-system access goes through `validate`, `browse` and the settings
//! writers, so tests point it at temporary directories.
//!
//! This is engine infrastructure, not part of any specification.
mod browse;
mod font;
mod validate;
mod view;
mod widgets;

#[cfg(test)]
mod tests;

use crate::config::{self, Config, ModList, Preset};
use crate::enhanced::ViewportMode;
use browse::{Browser, Target};
use std::path::{Path, PathBuf};
pub(crate) use validate::Empty;
use validate::{Check, State};
use vault_core::GameProfile;
use vault_formats::{LayerEntry, LoadOrderPolicy};
use vault_render::{IndexedImage, ScalingMode};
pub use view::palette;
use widgets::{List, TextField};
use winit::keyboard::KeyCode;

/// The load order a new mod list is written to, beside the settings.
const LOAD_ORDER: &str = "load_order.toml";

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
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Data,
    Display,
    Mods,
}

impl Tab {
    const ALL: [Self; 3] = [Self::Data, Self::Display, Self::Mods];
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
    Preset,
    Audio,
    Debug,
    /// The `enhanced` preset's presentation, shown with that preset only.
    Viewport,
    Zoom,
    SmoothCamera,
    RoofPeek,
    UiScale,
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
                mods.read_only = Some(
                    "mod_list is a path list in vault-engine.toml: edit it there, or remove it \
                     to keep a load_order.toml here"
                        .into(),
                );
            }
            Some(ModList::File(file)) => {
                mods.file = Some(file.clone());
                match std::fs::read_to_string(file) {
                    // Save creates it.
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => mods.fail(format!("{}: {error}", file.display())),
                    Ok(text) => match vault_formats::parse_load_order(&text) {
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
}

impl Launcher {
    pub fn new(setup: Setup) -> Self {
        let text = |path: &Option<PathBuf>| {
            TextField::new(
                path.as_ref()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            )
        };
        let config = setup.config;
        let mut launcher = Self {
            fields: [
                text(&config.master_dat),
                text(&config.critter_dat),
                text(&config.data_root),
            ],
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
        };
        launcher.revalidate();
        launcher.revalidate_mods();
        match (setup.problem, launcher.ready()) {
            (Some(problem), _) => launcher.report(State::Bad, problem),
            (None, Ok(())) => {
                launcher.focus = Widget::Play;
                launcher.report(State::Ok, "Ready: Enter plays.");
            }
            (None, Err(reason)) => launcher.report(State::Unset, reason),
        }
        launcher
    }

    pub fn image(&self) -> IndexedImage {
        view::draw(self)
    }

    pub fn title(&self) -> String {
        "Vault Engine — Launcher".into()
    }

    /// Whether the game data and the mod stack can start the game, or the
    /// first reason they cannot.
    pub fn ready(&self) -> Result<(), String> {
        const NAMES: [&str; 3] = ["MASTER.DAT", "CRITTER.DAT", "Data folder"];
        if let Some((name, check)) = NAMES
            .iter()
            .zip(&self.checks)
            .find(|(_, check)| check.state == State::Bad)
        {
            return Err(format!("{name}: {}.", check.text));
        }
        if self.checks.iter().all(|check| check.state == State::Unset) {
            return Err("Name MASTER.DAT and CRITTER.DAT, or a data folder.".into());
        }
        if self.mods.broken {
            return Err("Mods: the load order cannot be read.".into());
        }
        if self.mods.check.stack.state == State::Bad {
            return Err(format!("Mods: {}.", self.mods.check.stack.text));
        }
        // A layer that mounts can still fail on its `vault-mod.toml`.
        if let Some(check) = self
            .mods
            .check
            .layers
            .iter()
            .find(|check| check.state == State::Bad)
        {
            return Err(format!("Mods: {}.", check.text));
        }
        Ok(())
    }

    /// The settings to launch with: the edited ones, the fields' paths
    /// made absolute.
    pub fn launch_config(&mut self) -> Config {
        self.apply_fields();
        self.config.clone()
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
        match input {
            LauncherInput::Key { key, shift } if self.browser.is_some() => {
                self.browser_key(key, shift)
            }
            LauncherInput::Key { key, shift } => return self.key(key, shift),
            LauncherInput::Pointer(point) => self.hover = self.hit(point),
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
                Widget::Zoom | Widget::RoofPeek | Widget::UiScale => self.choose(focus, delta),
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

    fn switch_tab(&mut self, delta: isize) {
        let at = Tab::ALL
            .iter()
            .position(|tab| *tab == self.tab)
            .unwrap_or(0);
        self.tab = Tab::ALL[(at as isize + delta).rem_euclid(3) as usize];
        self.focus = Widget::Tab(self.tab);
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
        } else if self.tab == Tab::Mods {
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
                config.game = Some(match config.game.unwrap_or_default() {
                    GameProfile::Fallout1 => GameProfile::Fallout2,
                    GameProfile::Fallout2 => GameProfile::Fallout1,
                });
                // The archives are read as the other game's format now.
                self.revalidate();
            }
            Widget::Preset => {
                config.preset = Some(match config.preset.unwrap_or_default() {
                    Preset::Vanilla => Preset::Enhanced,
                    Preset::Enhanced => Preset::Vanilla,
                });
            }
            Widget::Fullscreen => config.fullscreen = Some(!config.fullscreen.unwrap_or(false)),
            Widget::Audio => config.audio = Some(!config.audio.unwrap_or(true)),
            Widget::Debug => config.debug = Some(!config.debug.unwrap_or(false)),
            Widget::Viewport
            | Widget::Zoom
            | Widget::SmoothCamera
            | Widget::RoofPeek
            | Widget::UiScale => self.choose(widget, 1),
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
                Ok(path) => self.report(State::Ok, format!("Saved {}.", path.display())),
                Err(error) => self.report(State::Bad, format!("Not saved: {error}")),
            },
            Widget::Play => return self.play(LauncherAction::Play),
            Widget::PlayDebug => return self.play(LauncherAction::PlayDebug),
            Widget::Quit => return Some(LauncherAction::Quit),
            Widget::BrowseList => self.browse_enter(),
            Widget::BrowseUse => {
                let browser = self.browser.as_ref()?;
                let (target, dir) = (browser.target, browser.dir.clone());
                self.pick_folder(target, dir);
            }
            Widget::BrowseCancel => self.close_browser(),
        }
        None
    }

    /// The next (`delta` 1) or previous (-1) choice of an enhanced
    /// presentation widget, wrapping: the viewport and camera smoothing
    /// toggle, the zoom steps through [`ZOOM_STEPS`], the roof peek cycles
    /// as Alt+R does, and the interface scale steps through [`UI_SCALES`].
    /// A value from the file between two steps moves from the nearest.
    fn choose(&mut self, widget: Widget, delta: isize) {
        let settings = self.config.enhanced_settings();
        let step = |at: usize, len: usize| (at as isize + delta).rem_euclid(len as isize) as usize;
        let nearest = |values: &mut dyn Iterator<Item = f32>, value: f32| {
            values
                .enumerate()
                .min_by(|(_, a), (_, b)| (a - value).abs().total_cmp(&(b - value).abs()))
                .map_or(0, |(at, _)| at)
        };
        let config = &mut self.config;
        match widget {
            Widget::Viewport => {
                config.viewport_mode = Some(match settings.viewport {
                    ViewportMode::Fixed => ViewportMode::Unlocked,
                    ViewportMode::Unlocked => ViewportMode::Fixed,
                });
            }
            Widget::SmoothCamera => config.smooth_camera = Some(!settings.smooth_camera),
            Widget::Zoom => {
                let at = nearest(&mut ZOOM_STEPS.into_iter(), settings.default_zoom);
                config.default_zoom = Some(ZOOM_STEPS[step(at, ZOOM_STEPS.len())]);
            }
            Widget::RoofPeek => {
                let mut mode = settings.roof_peek;
                // Backwards is two steps forwards in the three-mode cycle.
                for _ in 0..step(0, 3) {
                    mode = mode.next();
                }
                config.roof_peek = Some(mode);
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
                config.ui_scale = Some(config::UiScale::from_factor(next));
            }
            _ => {}
        }
    }

    /// Play: the settings are saved, then the shell starts the game; a
    /// launcher that is not ready says why instead.
    fn play(&mut self, action: LauncherAction) -> Option<LauncherAction> {
        if let Err(reason) = self.ready() {
            self.report(State::Bad, format!("Not ready. {reason}"));
            return None;
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
        let game = self.config.game.unwrap_or_default();
        let databases = master.is_some() || critter.is_some();
        self.checks[2] = match &root {
            None => Check::unset("optional: loose files read before the archives"),
            Some(root) => validate::data_root(root, !databases, game),
        };
        let root = root.filter(|_| self.checks[2].is_ok());
        let required = |other: &str| Check {
            state: State::Bad,
            text: format!("required with {other}"),
        };
        if game == GameProfile::Fallout2 {
            // SPEC-018 §2: the install folder holding MASTER.DAT is listed
            // for its archives, which are opened, and the master database
            // as mounted must hold the required members.
            [self.checks[0], self.checks[1]] = match &master {
                Some(master) => validate::fallout2(master, critter.as_deref(), root.as_deref()),
                None => [
                    Check {
                        state: State::Bad,
                        text: "required: Fallout 2 reads MASTER.DAT and CRITTER.DAT".into(),
                    },
                    Check::unset("found beside MASTER.DAT"),
                ],
            };
            return;
        }
        self.checks[0] = match (&master, &critter) {
            (Some(master), _) => validate::master(master, root.as_deref(), game),
            (None, Some(_)) => required("CRITTER.DAT"),
            (None, None) => Check::unset("not set: the data folder alone is the game data"),
        };
        self.checks[1] = match (&critter, &master) {
            (Some(critter), _) => validate::critter(critter, game),
            (None, Some(_)) => required("MASTER.DAT"),
            (None, None) => Check::unset("not set"),
        };
    }

    fn set_field(&mut self, field: Field, path: &Path) {
        self.fields[field.index()] = TextField::new(path.to_string_lossy());
        self.revalidate();
    }

    /// The fields' paths into the settings, made absolute (relative ones
    /// are relative to the working directory, as they were validated).
    fn apply_fields(&mut self) {
        let [master, critter, root] = Field::ALL.map(|field| {
            self.path(field)
                .map(|path| std::path::absolute(&path).unwrap_or(path))
        });
        self.config.master_dat = master;
        self.config.critter_dat = critter;
        self.config.data_root = root;
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
        match route_drop(path, path.is_dir(), self.tab) {
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
        self.browser = Some(Browser::open(start, target));
        self.focus = Widget::BrowseList;
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
            }
            Target::Layer
                if file
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .ends_with(".dat") =>
            {
                self.close_browser();
                let text = self.layer_text(&file);
                self.add_layer(text);
            }
            Target::Layer => self.report(State::Bad, "A mod layer is a folder or a .DAT file."),
        }
    }

    fn pick_folder(&mut self, target: Target, dir: PathBuf) {
        match target {
            Target::Field(Field::DataRoot) => {
                self.close_browser();
                self.set_field(Field::DataRoot, &dir);
            }
            Target::Field(_) => {
                if self.fill_install(&dir) {
                    self.close_browser();
                } else {
                    self.report(State::Bad, "No MASTER.DAT and CRITTER.DAT in this folder.");
                }
            }
            Target::Layer => {
                self.close_browser();
                let text = self.layer_text(&dir);
                self.add_layer(text);
            }
        }
    }

    // --- Mods ----------------------------------------------------------

    /// The directory the load order's relative paths are relative to.
    fn mods_dir(&self) -> PathBuf {
        let parent = |path: &Path| match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
            _ => PathBuf::from("."),
        };
        match (&self.mods.file, &self.settings_path) {
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

    fn editable(&mut self) -> bool {
        match self.mods.read_only.clone() {
            Some(reason) => {
                self.report(State::Bad, format!("Read only: {reason}"));
                false
            }
            None => true,
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
        let policy = LoadOrderPolicy {
            // An inline list's paths are the user's own, as the game reads them.
            allow_outside: matches!(self.config.mod_list, Some(ModList::Paths(_))),
        };
        self.mods.check = validate::mods(&self.mods_dir(), &self.mods.entries, policy);
        let rows = List::rows(view::LAYERS);
        self.mods.list.show(self.mods.entries.len(), rows);
    }

    // --- Saving --------------------------------------------------------

    /// Write the settings (and an edited load order); an existing settings
    /// file is rewritten in place, else the first save directory that
    /// takes a new one is used.
    pub fn save(&mut self) -> Result<PathBuf, String> {
        self.apply_fields();
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
        let write_mods = mods.dirty
            && mods.read_only.is_none()
            && (mods.file.is_some() || !mods.entries.is_empty());
        if write_mods {
            let file = mods
                .file
                .clone()
                .unwrap_or_else(|| path.with_file_name(LOAD_ORDER));
            let text = vault_formats::vfs::write_load_order(&mods.entries)
                .map_err(|error| error.to_string())?;
            config::write_replacing(&file, text.as_bytes()).map_err(|error| error.to_string())?;
            config.mod_list = Some(ModList::File(file.clone()));
            self.mods.file = Some(file);
            self.mods.dirty = false;
        }
        config.save(path).map_err(|error| error.to_string())?;
        self.config = config;
        Ok(())
    }
}

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
