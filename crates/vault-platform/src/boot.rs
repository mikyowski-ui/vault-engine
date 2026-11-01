//! SPEC-020 §2.4 and §2.5: which edition a zero-argument start boots, and
//! the two pre-boot screens that ask: the selector and the countdown banner.
//!
//! Everything here is headless, as the launcher's state machine is: a screen
//! takes one input and may answer with an action, and draws its own frame in
//! the launcher's font and palette, so it works before any game data is
//! mounted. Discovery reads directory tables only. It never mounts anything
//! and enables no mod. The shell wires this up, and a test drives it directly.
use crate::{
    config::{self, Config, GAMES, GamePaths, HotkeyHints, ModList, PerGame},
    launcher::{LauncherInput, color, font, validate},
};
use std::path::{Path, PathBuf};
use vault_core::GameProfile;
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// The pre-boot title (Decision D-12).
pub const PICK_TITLE: &str = "Found Fallout 1 and Fallout 2. Pick one.";
/// The title over a running session.
pub const SWITCH_TITLE: &str = "Switch game. Unsaved progress is lost.";
/// How long the banner waits, in shell milliseconds.
pub const BANNER_MS: u64 = 3000;
/// What a row says for an edition that was not found.
const NOT_FOUND: &str = "not found";

/// Each edition's install, or why it has none.
pub type Installs = PerGame<Result<GamePaths, String>>;

/// The frame the screens draw, the launcher's.
const FRAME: [i32; 2] = [640, 480];
/// The two rows: their left, the first's top, and their size.
const ROW_LEFT: i32 = 120;
const ROW_TOP: i32 = 200;
const ROW_SIZE: [i32; 2] = [400, 32];
const ROW_GAP: i32 = 8;

/// A game's name as the screens write it.
pub fn edition_name(game: GameProfile) -> &'static str {
    match game {
        GameProfile::Fallout1 => "Fallout 1",
        GameProfile::Fallout2 => "Fallout 2",
    }
}

/// The edition a `last_played` text names, if it names one.
fn named(last_played: Option<&str>) -> Option<GameProfile> {
    last_played.and_then(config::parse_game)
}

/// What a zero-argument start does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// The launcher, as a start with no usable data has always shown.
    Launcher,
    /// One edition: it boots at once.
    Start(GameProfile),
    Selector,
    Banner(GameProfile),
}

/// §2.4's table: `tests/fixtures/spec020/boot-decision.tsv` is every row.
pub fn decide(parses: bool, found: PerGame<bool>, last_played: Option<&str>) -> Decision {
    if !parses {
        return Decision::Launcher;
    }
    match (found.fallout1, found.fallout2) {
        (false, false) => Decision::Launcher,
        (true, false) => Decision::Start(GameProfile::Fallout1),
        (false, true) => Decision::Start(GameProfile::Fallout2),
        (true, true) => named(last_played).map_or(Decision::Selector, Decision::Banner),
    }
}

/// `paths` as the launcher validates them for `game`, or why not.
pub fn check_paths(paths: &GamePaths, game: GameProfile) -> Result<(), String> {
    let checks = validate::install(
        paths.master_dat.as_deref(),
        paths.critter_dat.as_deref(),
        paths.data_root.as_deref(),
        game,
    );
    validate::install_ready(&checks)
}

/// `game`'s install: its paths in `config`, then the default locations in
/// `dirs`, each validated. The error is why the first one failed.
pub fn find_edition(
    config: &Config,
    dirs: &PerGame<Vec<PathBuf>>,
    game: GameProfile,
) -> Result<GamePaths, String> {
    let own = config.profile(game);
    let mut reason = None;
    if !own.is_empty() {
        match check_paths(&own, game) {
            Ok(()) => return Ok(own),
            Err(error) => reason = Some(error),
        }
    }
    if let Some(found) = config::discover_game(dirs.get(game), game) {
        match check_paths(&found, game) {
            Ok(()) => return Ok(found),
            Err(error) => reason = reason.or(Some(error)),
        }
    }
    Err(reason.unwrap_or_else(|| NOT_FOUND.to_owned()))
}

/// Both editions' installs.
pub fn discover(config: &Config, dirs: &PerGame<Vec<PathBuf>>) -> Installs {
    PerGame {
        fallout1: find_edition(config, dirs, GameProfile::Fallout1),
        fallout2: find_edition(config, dirs, GameProfile::Fallout2),
    }
}

/// Whether both editions are installed (the seventh hint's condition).
pub fn both_found(config: &Config, dirs: &PerGame<Vec<PathBuf>>) -> bool {
    GAMES
        .iter()
        .all(|game| find_edition(config, dirs, *game).is_ok())
}

/// The enabled layers of `mod_list`. It reads the list and mounts nothing.
pub fn enabled_mods(config: &Config) -> usize {
    match &config.mod_list {
        None => 0,
        Some(ModList::Paths(paths)) => paths.len(),
        Some(ModList::File(file)) => crate::storage::read_config_file(file)
            .ok()
            .flatten()
            .and_then(|text| vault_formats::parse_load_order(&text).ok())
            .map_or(0, |entries| {
                entries.iter().filter(|entry| entry.enabled).count()
            }),
    }
}

/// The toast setting: unset is `once`.
pub fn hint_setting(config: Option<&Config>) -> HotkeyHints {
    config
        .and_then(|config| config.hotkey_hints)
        .unwrap_or_default()
}

/// One selector row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub game: GameProfile,
    /// The install, or why there is none.
    pub install: Result<GamePaths, String>,
    pub mods: usize,
}

impl Row {
    /// §2.4: `1  Fallout 1`, its mods when above 0, or why it cannot run.
    pub fn label(&self, number: usize) -> String {
        let name = edition_name(self.game);
        match &self.install {
            Err(reason) => format!("{number}  {name}  {reason}"),
            Ok(_) if self.mods == 1 => format!("{number}  {name}  (1 mod)"),
            Ok(_) if self.mods > 0 => format!("{number}  {name}  ({} mods)", self.mods),
            Ok(_) => format!("{number}  {name}"),
        }
    }
}

/// What a screen asks of the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootAction {
    /// Esc: the process ends, or the running session resumes.
    Leave,
    /// This edition.
    Choose(GameProfile),
}

/// The selector: two rows, over nothing or over a stopped session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selector {
    rows: [Row; 2],
    highlight: usize,
    /// The edition running under it, which is what changes the title.
    pub running: Option<GameProfile>,
}

impl Selector {
    /// §2.4: the highlight starts on `last_played`, else on the `game`
    /// setting, else on Fallout 1.
    pub fn new(config: &Config, found: Installs, running: Option<GameProfile>) -> Self {
        let mods = enabled_mods(config);
        let row = |game, install| Row {
            game,
            install,
            mods,
        };
        let rows = [
            row(GameProfile::Fallout1, found.fallout1),
            row(GameProfile::Fallout2, found.fallout2),
        ];
        let start = named(config.last_played.as_deref())
            .or(config.game)
            .unwrap_or(GameProfile::Fallout1);
        Self {
            highlight: rows.iter().position(|row| row.game == start).unwrap_or(0),
            rows,
            running,
        }
    }
    pub fn title(&self) -> &'static str {
        if self.running.is_some() {
            SWITCH_TITLE
        } else {
            PICK_TITLE
        }
    }
    #[cfg(test)]
    pub fn highlight(&self) -> GameProfile {
        self.rows[self.highlight].game
    }
    #[cfg(test)]
    pub fn row(&self, game: GameProfile) -> &Row {
        self.rows
            .iter()
            .find(|row| row.game == game)
            .expect("a row per edition")
    }
    /// A validation that failed at the last moment shows on its row
    /// (§2.5 step 1).
    pub fn fail(&mut self, game: GameProfile, reason: String) {
        if let Some(row) = self.rows.iter_mut().find(|row| row.game == game) {
            row.install = Err(reason);
        }
    }
    fn rect(index: usize) -> [i32; 4] {
        let top = ROW_TOP + index as i32 * (ROW_SIZE[1] + ROW_GAP);
        [ROW_LEFT, top, ROW_SIZE[0], ROW_SIZE[1]]
    }
    fn row_at([x, y]: [i32; 2]) -> Option<usize> {
        (0..2).find(|index| {
            let [left, top, width, height] = Self::rect(*index);
            (left..left + width).contains(&x) && (top..top + height).contains(&y)
        })
    }
    /// A row chooses its edition; a disabled one does nothing (case S6),
    /// and the edition already running just resumes (Decision D-23).
    fn choose(&self, index: usize) -> Option<BootAction> {
        let row = &self.rows[index];
        if self.running == Some(row.game) {
            return Some(BootAction::Leave);
        }
        row.install.is_ok().then_some(BootAction::Choose(row.game))
    }
    /// One input, and what it asks for.
    pub fn handle(&mut self, input: &LauncherInput) -> Option<BootAction> {
        match input {
            LauncherInput::Key { key, .. } => match key {
                KeyCode::Digit1 | KeyCode::Numpad1 => self.choose(0),
                KeyCode::Digit2 | KeyCode::Numpad2 => self.choose(1),
                KeyCode::ArrowUp | KeyCode::ArrowLeft => {
                    self.highlight = 0;
                    None
                }
                KeyCode::ArrowDown | KeyCode::ArrowRight => {
                    self.highlight = 1;
                    None
                }
                KeyCode::Enter | KeyCode::NumpadEnter => self.choose(self.highlight),
                KeyCode::Escape => Some(BootAction::Leave),
                _ => None,
            },
            LauncherInput::Pointer(point) => {
                if let Some(row) = Self::row_at(*point) {
                    self.highlight = row;
                }
                None
            }
            LauncherInput::Press(point) => Self::row_at(*point).and_then(|row| self.choose(row)),
            LauncherInput::Wheel(_) | LauncherInput::Dropped(_) => None,
        }
    }
    pub fn image(&self) -> IndexedImage {
        let mut image = frame();
        let width = font::width(self.title());
        font::draw(
            &mut image,
            [(FRAME[0] - width) / 2, ROW_TOP - 48],
            self.title(),
            color::ACCENT,
        );
        for (index, row) in self.rows.iter().enumerate() {
            let [x, y, w, h] = Self::rect(index);
            let lit = index == self.highlight;
            image.fill_rect(
                [x, y, w, h],
                if lit { color::SELECTED } else { color::PANEL },
            );
            let ink = match (&row.install, lit) {
                (Err(_), _) => color::DIM,
                (Ok(_), true) => color::FOCUS,
                (Ok(_), false) => color::TEXT,
            };
            let label = font::clip_end(&row.label(index + 1), (w as usize - 16) / 6);
            font::draw(&mut image, [x + 8, y + 12], &label, ink);
        }
        image
    }
}

/// A blank pre-boot frame.
fn frame() -> IndexedImage {
    let mut image =
        IndexedImage::new(FRAME[0] as u32, FRAME[1] as u32).expect("the frame size is valid");
    image.fill_rect([0, 0, FRAME[0], FRAME[1]], color::BACKGROUND);
    image
}

/// The countdown banner, `Starting Fallout 2 in 3.`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Banner {
    pub edition: GameProfile,
    /// The shell clock at its first frame.
    since: Option<u64>,
}

impl Banner {
    pub fn new(edition: GameProfile) -> Self {
        Self {
            edition,
            since: None,
        }
    }
    /// The whole seconds left at `now`: 3, 2 then 1.
    pub fn seconds(&self, now: u64) -> u64 {
        let spent = self.since.map_or(0, |since| now.saturating_sub(since));
        BANNER_MS.saturating_sub(spent).div_ceil(1000)
    }
    pub fn text(&self, now: u64) -> String {
        format!(
            "Starting {} in {}. Press any key to switch.",
            edition_name(self.edition),
            self.seconds(now).max(1)
        )
    }
    /// One tick: whether its time is up. The first tick is its first frame.
    pub fn tick(&mut self, now: u64) -> bool {
        let since = *self.since.get_or_insert(now);
        now.saturating_sub(since) >= BANNER_MS
    }
    /// Whether `input` stops it. Pointer movement and a lone modifier do
    /// not, and the shell has already kept F11, Alt+Enter and Alt+S.
    pub fn stops(input: &LauncherInput) -> bool {
        match input {
            LauncherInput::Key { key, .. } => !is_modifier(*key),
            LauncherInput::Press(_) => true,
            _ => false,
        }
    }
    pub fn image(&self, now: u64) -> IndexedImage {
        let mut image = frame();
        let text = self.text(now);
        let at = [(FRAME[0] - font::width(&text)) / 2, FRAME[1] / 2 - 4];
        font::draw(&mut image, at, &text, color::ACCENT);
        image
    }
}

fn is_modifier(key: KeyCode) -> bool {
    matches!(
        key,
        KeyCode::ShiftLeft
            | KeyCode::ShiftRight
            | KeyCode::ControlLeft
            | KeyCode::ControlRight
            | KeyCode::AltLeft
            | KeyCode::AltRight
            | KeyCode::SuperLeft
            | KeyCode::SuperRight
            | KeyCode::CapsLock
            | KeyCode::NumLock
            | KeyCode::ScrollLock
    )
}

/// The screen showing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Selector(Selector),
    Banner(Banner),
}

impl Screen {
    pub fn image(&self, now: u64) -> IndexedImage {
        match self {
            Self::Selector(selector) => selector.image(),
            Self::Banner(banner) => banner.image(now),
        }
    }
    pub fn title(&self) -> String {
        match self {
            Self::Selector(selector) => selector.title().to_owned(),
            Self::Banner(banner) => format!("Vault Engine — {}", edition_name(banner.edition)),
        }
    }
}

/// The settings file a choice commits to: the one found, else a new one in
/// the first folder that takes it, as the first start writes it.
pub fn commit_choice(config: &Config, dirs: &[PathBuf]) -> Result<PathBuf, String> {
    match config::find(dirs) {
        Some(path) => config.commit(&path).map(|()| path),
        None => config::persist(config, dirs)
            .ok_or_else(|| "no folder takes the settings file".to_owned()),
    }
}

/// The setting `hints` written over the settings at `path`, in one commit.
pub fn commit_hints(path: &Path, hints: HotkeyHints) -> Result<(), String> {
    let mut config = Config::load(path).map_err(|error| error.to_string())?;
    config.hotkey_hints = Some(hints);
    config.commit(path)
}

#[cfg(test)]
#[path = "boot_tests.rs"]
mod tests;
