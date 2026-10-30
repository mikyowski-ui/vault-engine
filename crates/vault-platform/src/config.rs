//! `vault-engine.toml`: the launcher settings a zero-argument start reads,
//! and the game-data discovery that writes them the first time.
//!
//! The file is a small TOML subset read and written by hand: one
//! `key = value` per line, where a value is a basic `"string"` (with the
//! `\\ \" \n \t \r \uXXXX \UXXXXXXXX` escapes), a literal `'string'`,
//! `true` or `false`, a decimal integer or float (`2`, `-1_000`, `1.25`,
//! `5e-1`), or a one-line array of strings; `#` starts a comment.
//! Relative paths are relative to the file's directory.
//!
//! The only tables are `[profiles.fallout1]` and `[profiles.fallout2]`,
//! after the top-level keys: each game's last-used `master_dat`,
//! `critter_dat` and `data_root`, so switching games keeps both sets. The
//! top-level keys stay the active game's paths (a file from before
//! profiles is read as it always was) and win over its table when both
//! name paths.
//!
//! This is engine infrastructure, not part of any specification: it only
//! decides which game data and display options the shell starts with.
use crate::enhanced::{EnhancedSettings, GameSpeed, RoofPeek, ViewportMode};
use std::{
    fmt,
    path::{Path, PathBuf},
};
use vault_core::GameProfile;
use vault_render::{RenderPath, ScalingMode};

/// The settings file's name, looked up beside the executable first and
/// then in the working directory.
pub const FILE_NAME: &str = "vault-engine.toml";

/// The engine preset a configuration selects (the engine maps it to its
/// own rule set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Preset {
    #[default]
    Vanilla,
    Enhanced,
}

impl Preset {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "vanilla" => Some(Self::Vanilla),
            "enhanced" => Some(Self::Enhanced),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Vanilla => "vanilla",
            Self::Enhanced => "enhanced",
        }
    }
}

/// Both games, in profile order.
pub const GAMES: [GameProfile; 2] = [GameProfile::Fallout1, GameProfile::Fallout2];

/// `fallout1` or `fallout2` (the `game` key and `--game-profile`).
pub fn parse_game(text: &str) -> Option<GameProfile> {
    match text {
        "fallout1" => Some(GameProfile::Fallout1),
        "fallout2" => Some(GameProfile::Fallout2),
        _ => None,
    }
}
pub fn game_name(game: GameProfile) -> &'static str {
    match game {
        GameProfile::Fallout1 => "fallout1",
        GameProfile::Fallout2 => "fallout2",
    }
}

/// `aspect_fit` or `integer`.
pub fn parse_scaling(text: &str) -> Option<ScalingMode> {
    match text {
        "aspect_fit" => Some(ScalingMode::AspectFit),
        "integer" => Some(ScalingMode::Integer),
        _ => None,
    }
}
pub fn scaling_name(mode: ScalingMode) -> &'static str {
    match mode {
        ScalingMode::AspectFit => "aspect_fit",
        ScalingMode::Integer => "integer",
    }
}

/// `fixed_640x480` or `unlocked_widescreen` (the `enhanced` preset's
/// `viewport_mode`).
pub fn parse_viewport(text: &str) -> Option<ViewportMode> {
    match text {
        "fixed_640x480" => Some(ViewportMode::Fixed),
        "unlocked_widescreen" => Some(ViewportMode::Unlocked),
        _ => None,
    }
}
pub fn viewport_name(mode: ViewportMode) -> &'static str {
    match mode {
        ViewportMode::Fixed => "fixed_640x480",
        ViewportMode::Unlocked => "unlocked_widescreen",
    }
}

/// `vanilla`, `cursor_xray` or `hidden` (the `enhanced` preset's
/// `roof_peek`).
pub fn parse_roof_peek(text: &str) -> Option<RoofPeek> {
    match text {
        "vanilla" => Some(RoofPeek::Vanilla),
        "cursor_xray" => Some(RoofPeek::CursorXray),
        "hidden" => Some(RoofPeek::Hidden),
        _ => None,
    }
}
pub fn roof_peek_name(mode: RoofPeek) -> &'static str {
    match mode {
        RoofPeek::Vanilla => "vanilla",
        RoofPeek::CursorXray => "cursor_xray",
        RoofPeek::Hidden => "hidden",
    }
}

/// `default_zoom`'s range (the enhanced view's zoom range).
pub const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.5..=3.0;
/// A numeric `ui_scale`'s range.
pub const UI_SCALE_RANGE: std::ops::RangeInclusive<f32> = 0.5..=8.0;

/// `default_zoom` on the command line: a number from 0.5 to 3.0.
pub fn parse_zoom(text: &str) -> Option<f32> {
    text.parse::<f32>()
        .ok()
        .filter(|zoom| ZOOM_RANGE.contains(zoom))
}

/// `ui_scale`: `auto` fits the interface to the window's height, else
/// surface pixels per interface pixel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UiScale {
    Auto,
    Scale(f32),
}
impl UiScale {
    /// The enhanced settings' form: `None` is `auto`.
    pub fn factor(self) -> Option<f32> {
        match self {
            Self::Auto => None,
            Self::Scale(scale) => Some(scale),
        }
    }
    pub fn from_factor(factor: Option<f32>) -> Self {
        factor.map_or(Self::Auto, Self::Scale)
    }
}

/// A value of the TOML subset.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    String(String),
    Bool(bool),
    Integer(i64),
    Float(f64),
    Array(Vec<String>),
}

impl Value {
    /// An integer or a float, as a number.
    fn number(&self) -> Option<f64> {
        match self {
            Self::Integer(value) => Some(*value as f64),
            Self::Float(value) => Some(*value),
            _ => None,
        }
    }
}

/// `mod_list` (alias `load_order`): the mods in load order, or the path of
/// a `load_order.toml` that lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModList {
    Paths(Vec<PathBuf>),
    File(PathBuf),
}

/// One game's data paths: the top-level keys for the active game, a
/// `[profiles.<game>]` table for each game.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GamePaths {
    pub master_dat: Option<PathBuf>,
    pub critter_dat: Option<PathBuf>,
    /// The loose data directory both databases consult first.
    pub data_root: Option<PathBuf>,
}

impl GamePaths {
    pub fn is_empty(&self) -> bool {
        self.entries().iter().all(|(_, path)| path.is_none())
    }
    /// Whether the paths name game data: both databases, or a loose data
    /// directory.
    pub fn has_game_data(&self) -> bool {
        (self.master_dat.is_some() && self.critter_dat.is_some()) || self.data_root.is_some()
    }
    fn entries(&self) -> [(&'static str, &Option<PathBuf>); 3] {
        [
            ("master_dat", &self.master_dat),
            ("critter_dat", &self.critter_dat),
            ("data_root", &self.data_root),
        ]
    }
    fn entries_mut(&mut self) -> [(&'static str, &mut Option<PathBuf>); 3] {
        [
            ("master_dat", &mut self.master_dat),
            ("critter_dat", &mut self.critter_dat),
            ("data_root", &mut self.data_root),
        ]
    }
}

/// One value for each game.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PerGame<T> {
    pub fallout1: T,
    pub fallout2: T,
}

impl<T> PerGame<T> {
    pub fn get(&self, game: GameProfile) -> &T {
        match game {
            GameProfile::Fallout1 => &self.fallout1,
            GameProfile::Fallout2 => &self.fallout2,
        }
    }
    pub fn get_mut(&mut self, game: GameProfile) -> &mut T {
        match game {
            GameProfile::Fallout1 => &mut self.fallout1,
            GameProfile::Fallout2 => &mut self.fallout2,
        }
    }
}

/// The settings in `vault-engine.toml`; an absent key is `None`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Config {
    pub master_dat: Option<PathBuf>,
    pub critter_dat: Option<PathBuf>,
    /// The loose data directory both databases consult first.
    pub data_root: Option<PathBuf>,
    pub mod_list: Option<ModList>,
    /// The game whose data this is; only Fallout 1 runs today.
    pub game: Option<GameProfile>,
    pub preset: Option<Preset>,
    pub scaling: Option<ScalingMode>,
    pub fullscreen: Option<bool>,
    /// The touch UI: touch gestures and the on-screen pill bar.
    pub touch_ui: Option<bool>,
    pub debug: Option<bool>,
    /// `false` keeps the audio device closed, as `--no-audio` does.
    pub audio: Option<bool>,
    /// The `enhanced` preset's presentation (unused under Vanilla): the
    /// viewport, the zoom a map starts at, camera smoothing, the roof peek
    /// mode and the interface scale.
    pub viewport_mode: Option<ViewportMode>,
    pub default_zoom: Option<f32>,
    pub smooth_camera: Option<bool>,
    pub roof_peek: Option<RoofPeek>,
    pub ui_scale: Option<UiScale>,
    /// The `enhanced` preset's game speed (1.0, 1.5 or 2.0; unused under
    /// Vanilla).
    pub game_speed: Option<GameSpeed>,
    /// SPEC-022 §3.1 the world canvas's render path, `reference` or
    /// `instanced`, under every preset and game. The engine only reads it:
    /// a rewrite keeps the file's own line, and nothing the engine chose
    /// (such as `--render-path`) is ever written here, logged or saved.
    pub render_path: Option<RenderPath>,
    /// `sfall` under `[scripting]` (SPEC-024b §3.1): opt-in sfall mode.
    pub sfall: Option<bool>,
    /// Keys this engine does not know, in file order: ignored, but kept
    /// so that a rewrite does not lose them.
    pub unknown: Vec<(String, Value)>,
    /// The other game's last-used paths (`[profiles.<game>]`). The active
    /// game's are the top-level fields, so its entry here stays empty;
    /// [`Config::switch_game`] moves them between the two.
    pub profiles: PerGame<GamePaths>,
}

/// A malformed line of the settings file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    /// One-based.
    pub line: usize,
    pub message: String,
}
impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}
impl std::error::Error for ConfigError {}

impl Config {
    /// Parse the TOML subset; the first malformed line is an error that
    /// names it.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let mut config = Self::default();
        let mut seen: Vec<&'static str> = Vec::new();
        // The profile table being read, and the tables read.
        let mut table: Option<GameProfile> = None;
        let mut tables: Vec<GameProfile> = Vec::new();
        let mut scripting = false;
        let mut in_scripting = false;
        for (index, raw) in text.lines().enumerate() {
            let line = index + 1;
            let error = |message: String| ConfigError { line, message };
            let raw = if index == 0 {
                raw.strip_prefix('\u{feff}').unwrap_or(raw)
            } else {
                raw
            };
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some(header) = trimmed.strip_prefix('[') {
                let (name, rest) = header
                    .split_once(']')
                    .ok_or_else(|| error("unterminated table header".into()))?;
                let rest = rest.trim_start();
                if !(rest.is_empty() || rest.starts_with('#')) {
                    return Err(error(format!("unexpected `{rest}` after the table header")));
                }
                let name = name.trim();
                if name == "scripting" {
                    if scripting {
                        return Err(error(format!("duplicate table `[{name}]`")));
                    }
                    scripting = true;
                    in_scripting = true;
                    table = None;
                    continue;
                }
                in_scripting = false;
                let game = name
                    .strip_prefix("profiles.")
                    .and_then(parse_game)
                    .ok_or_else(|| {
                        error(format!(
                            "unsupported table `[{name}]` (only [profiles.fallout1], \
                             [profiles.fallout2] and [scripting])"
                        ))
                    })?;
                if tables.contains(&game) {
                    return Err(error(format!("duplicate table `[{name}]`")));
                }
                tables.push(game);
                table = Some(game);
                continue;
            }
            let (key, rest) = trimmed
                .split_once('=')
                .ok_or_else(|| error("expected `key = value`".into()))?;
            let key = key.trim();
            if key.is_empty()
                || !key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            {
                return Err(error(format!("invalid key `{key}`")));
            }
            let (value, rest) = parse_value(rest.trim_start()).map_err(error)?;
            let rest = rest.trim_start();
            if !(rest.is_empty() || rest.starts_with('#')) {
                return Err(error(format!("unexpected `{rest}` after the value")));
            }
            if in_scripting {
                if key != "sfall" {
                    return Err(error(format!("`{key}` is not a scripting setting (sfall)")));
                }
                if config.sfall.is_some() {
                    return Err(error(format!("duplicate key `{key}`")));
                }
                match value {
                    Value::Bool(flag) => config.sfall = Some(flag),
                    _ => return Err(error("`sfall` expects true or false".into())),
                }
                continue;
            }
            if let Some(game) = table {
                let paths = config.profiles.get_mut(game);
                let (_, slot) = paths
                    .entries_mut()
                    .into_iter()
                    .find(|(name, _)| *name == key)
                    .ok_or_else(|| {
                        error(format!(
                            "`{key}` is not a profile setting (master_dat, critter_dat or \
                             data_root)"
                        ))
                    })?;
                if slot.is_some() {
                    return Err(error(format!("duplicate key `{key}`")));
                }
                match value {
                    Value::String(text) => *slot = Some(text.into()),
                    _ => return Err(error(format!("`{key}` expects a string"))),
                }
                continue;
            }
            let canonical = match key {
                "load_order" => "mod_list",
                other => KNOWN
                    .iter()
                    .copied()
                    .find(|known| *known == other)
                    .unwrap_or(""),
            };
            if !canonical.is_empty() {
                if seen.contains(&canonical) {
                    return Err(error(format!("duplicate key `{key}`")));
                }
                seen.push(canonical);
            } else if config.unknown.iter().any(|(name, _)| name == key) {
                return Err(error(format!("duplicate key `{key}`")));
            }
            config.assign(canonical, key, value).map_err(error)?;
        }
        // The active game's table fills the top-level paths only when they
        // name none; either way the top level holds them from here on.
        let stored = std::mem::take(config.profiles.get_mut(config.active_game()));
        if config.paths().is_empty() {
            config.set_paths(stored);
        }
        Ok(config)
    }

    fn assign(&mut self, canonical: &str, key: &str, value: Value) -> Result<(), String> {
        let string = |value: Value| match value {
            Value::String(text) => Ok(text),
            _ => Err(format!("`{key}` expects a string")),
        };
        let boolean = |value: Value| match value {
            Value::Bool(flag) => Ok(flag),
            _ => Err(format!("`{key}` expects true or false")),
        };
        match canonical {
            "master_dat" => self.master_dat = Some(string(value)?.into()),
            "critter_dat" => self.critter_dat = Some(string(value)?.into()),
            "data_root" => self.data_root = Some(string(value)?.into()),
            "mod_list" => {
                self.mod_list = Some(match value {
                    Value::Array(paths) => {
                        ModList::Paths(paths.into_iter().map(Into::into).collect())
                    }
                    Value::String(path) => ModList::File(path.into()),
                    Value::Bool(_) | Value::Integer(_) | Value::Float(_) => {
                        return Err(format!("`{key}` expects an array of paths or a path"));
                    }
                })
            }
            "game" => {
                let text = string(value)?;
                self.game = Some(parse_game(&text).ok_or_else(|| {
                    format!("`game` is \"fallout1\" or \"fallout2\", not \"{text}\"")
                })?);
            }
            "preset" => {
                let text = string(value)?;
                self.preset = Some(Preset::parse(&text).ok_or_else(|| {
                    format!("`preset` is \"vanilla\" or \"enhanced\", not \"{text}\"")
                })?);
            }
            "scaling" => {
                let text = string(value)?;
                self.scaling = Some(parse_scaling(&text).ok_or_else(|| {
                    format!("`scaling` is \"aspect_fit\" or \"integer\", not \"{text}\"")
                })?);
            }
            "fullscreen" => self.fullscreen = Some(boolean(value)?),
            "touch_ui" => self.touch_ui = Some(boolean(value)?),
            "debug" => self.debug = Some(boolean(value)?),
            "audio" => self.audio = Some(boolean(value)?),
            "viewport_mode" => {
                let text = string(value)?;
                self.viewport_mode = Some(parse_viewport(&text).ok_or_else(|| {
                    format!(
                        "`viewport_mode` is \"fixed_640x480\" or \"unlocked_widescreen\", not \"{text}\""
                    )
                })?);
            }
            "default_zoom" => {
                let expects = "`default_zoom` is a number from 0.5 to 3.0";
                let zoom = value.number().ok_or_else(|| expects.to_owned())?;
                self.default_zoom = Some(
                    Some(zoom as f32)
                        .filter(|zoom| ZOOM_RANGE.contains(zoom))
                        .ok_or_else(|| format!("{expects}, not {zoom}"))?,
                );
            }
            "smooth_camera" => self.smooth_camera = Some(boolean(value)?),
            "roof_peek" => {
                let text = string(value)?;
                self.roof_peek = Some(parse_roof_peek(&text).ok_or_else(|| {
                    format!(
                        "`roof_peek` is \"vanilla\", \"cursor_xray\" or \"hidden\", not \"{text}\""
                    )
                })?);
            }
            "ui_scale" => {
                let expects = "`ui_scale` is \"auto\" or a number from 0.5 to 8";
                self.ui_scale = Some(match value {
                    Value::String(text) if text == "auto" => UiScale::Auto,
                    Value::String(text) => return Err(format!("{expects}, not \"{text}\"")),
                    value => {
                        let scale = value.number().ok_or_else(|| expects.to_owned())?;
                        UiScale::Scale(
                            Some(scale as f32)
                                .filter(|scale| UI_SCALE_RANGE.contains(scale))
                                .ok_or_else(|| format!("{expects}, not {scale}"))?,
                        )
                    }
                });
            }
            "render_path" => {
                let text = string(value)?;
                self.render_path = Some(RenderPath::parse(&text).ok_or_else(|| {
                    format!("`render_path` is \"reference\" or \"instanced\", not \"{text}\"")
                })?);
            }
            "game_speed" => {
                let expects = "`game_speed` is 1.0, 1.5 or 2.0";
                let factor = value.number().ok_or_else(|| expects.to_owned())?;
                self.game_speed = Some(
                    GameSpeed::from_factor(factor)
                        .ok_or_else(|| format!("{expects}, not {factor}"))?,
                );
            }
            _ => self.unknown.push((key.to_owned(), value)),
        }
        Ok(())
    }

    /// The file text: the known keys in a fixed order, then the unknown
    /// ones, then each game's profile table that names a path (the active
    /// game's repeating the top-level paths). A path that is not valid
    /// UTF-8 cannot be written.
    pub fn to_toml(&self) -> std::io::Result<String> {
        let mut out = String::from("# Vault Engine launcher settings.\n");
        let path = |path: &Path| {
            path.to_str().map(quote).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("{} is not valid UTF-8", path.display()),
                )
            })
        };
        let mut tables = String::new();
        if let Some(sfall) = self.sfall {
            tables.push_str(&format!("\n[scripting]\nsfall = {sfall}\n"));
        }
        for game in GAMES {
            let paths = self.profile(game);
            if paths.is_empty() {
                continue;
            }
            tables.push_str(&format!("\n[profiles.{}]\n", game_name(game)));
            for (key, value) in paths.entries() {
                if let Some(value) = value {
                    tables.push_str(&format!("{key} = {}\n", path(value)?));
                }
            }
        }
        let mut line = |key: &str, value: String| {
            out.push_str(key);
            out.push_str(" = ");
            out.push_str(&value);
            out.push('\n');
        };
        for (key, value) in [
            ("master_dat", &self.master_dat),
            ("critter_dat", &self.critter_dat),
            ("data_root", &self.data_root),
        ] {
            if let Some(value) = value {
                line(key, path(value)?);
            }
        }
        match &self.mod_list {
            Some(ModList::Paths(paths)) => {
                let items = paths
                    .iter()
                    .map(|item| path(item))
                    .collect::<std::io::Result<Vec<_>>>()?;
                line("mod_list", format!("[{}]", items.join(", ")));
            }
            Some(ModList::File(file)) => line("mod_list", path(file)?),
            None => {}
        }
        if let Some(game) = self.game {
            line("game", quote(game_name(game)));
        }
        if let Some(preset) = self.preset {
            line("preset", quote(preset.name()));
        }
        if let Some(scaling) = self.scaling {
            line("scaling", quote(scaling_name(scaling)));
        }
        for (key, value) in [
            ("fullscreen", self.fullscreen),
            ("touch_ui", self.touch_ui),
            ("debug", self.debug),
            ("audio", self.audio),
        ] {
            if let Some(value) = value {
                line(key, value.to_string());
            }
        }
        if let Some(mode) = self.viewport_mode {
            line("viewport_mode", quote(viewport_name(mode)));
        }
        if let Some(zoom) = self.default_zoom {
            line("default_zoom", float(f64::from(zoom)));
        }
        if let Some(smooth) = self.smooth_camera {
            line("smooth_camera", smooth.to_string());
        }
        if let Some(mode) = self.roof_peek {
            line("roof_peek", quote(roof_peek_name(mode)));
        }
        match self.ui_scale {
            Some(UiScale::Auto) => line("ui_scale", quote("auto")),
            Some(UiScale::Scale(scale)) => line("ui_scale", float(f64::from(scale))),
            None => {}
        }
        if let Some(speed) = self.game_speed {
            line("game_speed", float(f64::from(speed.factor())));
        }
        if let Some(path) = self.render_path {
            line("render_path", quote(path.name()));
        }
        for (key, value) in &self.unknown {
            line(
                key,
                match value {
                    Value::String(text) => quote(text),
                    Value::Bool(flag) => flag.to_string(),
                    Value::Integer(number) => number.to_string(),
                    Value::Float(number) => float(*number),
                    Value::Array(items) => format!(
                        "[{}]",
                        items
                            .iter()
                            .map(|item| quote(item))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                },
            );
        }
        out.push_str(&tables);
        Ok(out)
    }

    /// Relative paths taken relative to `base` (the file's directory).
    pub fn relative_to(mut self, base: &Path) -> Self {
        self.each_path(|path| {
            if path.is_relative() {
                *path = base.join(&*path);
            }
        });
        self
    }

    /// Paths below `base` made relative to it again (the inverse of
    /// [`Config::relative_to`]), so a file kept beside the game data stays
    /// valid when the folder moves; other paths are kept as they are.
    pub fn relative_from(mut self, base: &Path) -> Self {
        self.each_path(|path| {
            if let Ok(rest) = path.strip_prefix(base)
                && !rest.as_os_str().is_empty()
            {
                *path = rest.to_path_buf();
            }
        });
        self
    }

    fn each_path(&mut self, mut visit: impl FnMut(&mut PathBuf)) {
        let profiles = &mut self.profiles;
        for path in [
            &mut self.master_dat,
            &mut self.critter_dat,
            &mut self.data_root,
        ]
        .into_iter()
        .chain(profiles.fallout1.entries_mut().map(|(_, path)| path))
        .chain(profiles.fallout2.entries_mut().map(|(_, path)| path))
        .flatten()
        {
            visit(path);
        }
        match &mut self.mod_list {
            Some(ModList::Paths(paths)) => paths.iter_mut().for_each(visit),
            Some(ModList::File(file)) => visit(file),
            None => {}
        }
    }

    /// Write the settings to `path`, replacing any file there; paths below
    /// its directory are written relative to it, and unknown keys are kept.
    pub fn save(&self, path: &Path) -> crate::AppResult<()> {
        let base = path.parent().unwrap_or(Path::new(""));
        let text = self
            .clone()
            .relative_from(base)
            .to_toml()
            .map_err(|error| format!("{}: {error}", path.display()))?;
        write_replacing(path, text.as_bytes())
    }

    /// Read a settings file, its relative paths resolved against its
    /// directory; unknown keys are reported and ignored.
    pub fn load(path: &Path) -> crate::AppResult<Self> {
        let text = crate::storage::read_config_file(path)?
            .ok_or_else(|| format!("{}: file not found", path.display()))?;
        let config = Self::parse(&text).map_err(|error| format!("{}: {error}", path.display()))?;
        for (key, _) in &config.unknown {
            tracing::warn!(file = %path.display(), key, "unknown setting ignored");
        }
        let base = path.parent().unwrap_or(Path::new(""));
        Ok(config.relative_to(base))
    }

    /// The `enhanced` preset's presentation settings; missing keys take
    /// [`EnhancedSettings::default`].
    pub fn enhanced_settings(&self) -> EnhancedSettings {
        let default = EnhancedSettings::default();
        EnhancedSettings {
            viewport: self.viewport_mode.unwrap_or(default.viewport),
            default_zoom: self.default_zoom.unwrap_or(default.default_zoom),
            smooth_camera: self.smooth_camera.unwrap_or(default.smooth_camera),
            roof_peek: self.roof_peek.unwrap_or(default.roof_peek),
            ui_scale: self.ui_scale.map_or(default.ui_scale, UiScale::factor),
            navigation_only: false,
        }
    }

    /// The `enhanced` preset's game speed; missing, 1.0x.
    pub fn game_speed(&self) -> GameSpeed {
        self.game_speed.unwrap_or_default()
    }

    /// Whether the settings name game data: both databases, or a loose
    /// data directory.
    pub fn has_game_data(&self) -> bool {
        self.paths().has_game_data()
    }

    /// Settings for `game` from each game's paths: `game`'s become the
    /// top-level keys.
    pub fn with_profiles(game: GameProfile, mut profiles: PerGame<GamePaths>) -> Self {
        let mut config = Self {
            game: Some(game),
            ..Self::default()
        };
        config.set_paths(std::mem::take(profiles.get_mut(game)));
        config.profiles = profiles;
        config
    }

    /// The game the top-level paths are for.
    pub fn active_game(&self) -> GameProfile {
        self.game.unwrap_or_default()
    }

    /// The top-level paths: the active game's.
    pub fn paths(&self) -> GamePaths {
        GamePaths {
            master_dat: self.master_dat.clone(),
            critter_dat: self.critter_dat.clone(),
            data_root: self.data_root.clone(),
        }
    }

    pub fn set_paths(&mut self, paths: GamePaths) {
        self.master_dat = paths.master_dat;
        self.critter_dat = paths.critter_dat;
        self.data_root = paths.data_root;
    }

    /// `game`'s last-used paths: the top-level ones for the active game.
    pub fn profile(&self, game: GameProfile) -> GamePaths {
        if game == self.active_game() {
            self.paths()
        } else {
            self.profiles.get(game).clone()
        }
    }

    /// The paths a launch of `game` reads: its profile, or the top-level
    /// paths when it has none (a file from before profiles, launched with
    /// `--game-profile`).
    pub fn launch_paths(&self, game: GameProfile) -> GamePaths {
        let paths = self.profile(game);
        if paths.is_empty() {
            self.paths()
        } else {
            paths
        }
    }

    /// Make `game` the active game: the top-level paths are kept as the
    /// old game's profile, and `game`'s profile becomes the top-level
    /// paths (none when it has none yet).
    pub fn switch_game(&mut self, game: GameProfile) {
        let active = self.active_game();
        if game != active {
            *self.profiles.get_mut(active) = self.paths();
            let paths = std::mem::take(self.profiles.get_mut(game));
            self.set_paths(paths);
        }
        self.game = Some(game);
    }
}

const KNOWN: [&str; 19] = [
    "master_dat",
    "critter_dat",
    "data_root",
    "mod_list",
    "game",
    "preset",
    "scaling",
    "fullscreen",
    "touch_ui",
    "debug",
    "audio",
    "viewport_mode",
    "default_zoom",
    "smooth_camera",
    "roof_peek",
    "ui_scale",
    "game_speed",
    "render_path",
    "load_order",
];

/// A float as TOML writes it: always with a fraction or an exponent, so it
/// reads back as a float. A value that is an `f32` is written at that
/// precision (`1.1`, not `1.100000023841858`).
fn float(value: f64) -> String {
    // `Debug` keeps the fraction (`2.0`) and uses an exponent for very
    // large and small values (`1e300`).
    let text = if value == f64::from(value as f32) {
        format!("{:?}", value as f32)
    } else {
        format!("{value:?}")
    };
    if text.contains(['.', 'e', 'E']) {
        text
    } else {
        format!("{text}.0")
    }
}

/// A decimal integer or float at the start of `text`, and the text after
/// it; `None` when the text does not start like a number. TOML's decimal
/// forms: an optional sign, digits with single underscores between them
/// and no leading zero, then for a float a fraction, an exponent or both.
/// Infinities, NaN and the hexadecimal, octal and binary forms are not
/// part of the subset.
fn parse_number(text: &str) -> Option<Result<(Value, &str), String>> {
    let end = text
        .find(|c: char| c.is_whitespace() || matches!(c, '#' | ',' | ']'))
        .unwrap_or(text.len());
    let (word, rest) = text.split_at(end);
    let unsigned = word.strip_prefix(['+', '-']).unwrap_or(word);
    if !unsigned.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let invalid = || Some(Err(format!("invalid number `{word}`")));
    let digits = |part: &str| {
        !part.is_empty()
            && !part.starts_with('_')
            && !part.ends_with('_')
            && !part.contains("__")
            && part
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'_')
    };
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(at) => (&unsigned[..at], Some(&unsigned[at + 1..])),
        None => (unsigned, None),
    };
    let (whole, fraction) = match mantissa.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (mantissa, None),
    };
    let leading_zero = whole.len() > 1 && whole.starts_with('0');
    let exponent_ok = exponent
        .is_none_or(|exponent| digits(exponent.strip_prefix(['+', '-']).unwrap_or(exponent)));
    if !digits(whole) || leading_zero || !fraction.is_none_or(digits) || !exponent_ok {
        return invalid();
    }
    let plain: String = word.chars().filter(|c| *c != '_').collect();
    let value = if fraction.is_some() || exponent.is_some() {
        match plain.parse::<f64>() {
            Ok(value) if value.is_finite() => Value::Float(value),
            _ => return invalid(),
        }
    } else {
        match plain.parse::<i64>() {
            Ok(value) => Value::Integer(value),
            Err(_) => return Some(Err(format!("number `{word}` is out of range"))),
        }
    };
    Some(Ok((value, rest)))
}

/// A basic string with its escapes.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// One value at the start of `text`, and the text after it.
fn parse_value(text: &str) -> Result<(Value, &str), String> {
    if let Some(rest) = text.strip_prefix('[') {
        let mut items = Vec::new();
        let mut rest = rest.trim_start();
        loop {
            if let Some(after) = rest.strip_prefix(']') {
                return Ok((Value::Array(items), after));
            }
            if rest.is_empty() || rest.starts_with('#') {
                return Err("unterminated array (arrays stay on one line)".into());
            }
            let (item, after) =
                parse_string(rest)?.ok_or_else(|| "arrays hold strings only".to_owned())?;
            items.push(item);
            rest = after.trim_start();
            if let Some(after) = rest.strip_prefix(',') {
                rest = after.trim_start();
            } else if !rest.starts_with(']') {
                return Err("expected `,` or `]` in the array".into());
            }
        }
    }
    if let Some((text, rest)) = parse_string(text)? {
        return Ok((Value::String(text), rest));
    }
    if let Some(number) = parse_number(text) {
        return number;
    }
    for (word, flag) in [("true", true), ("false", false)] {
        if let Some(rest) = text.strip_prefix(word)
            && rest
                .chars()
                .next()
                .is_none_or(|c| c.is_whitespace() || c == '#')
        {
            return Ok((Value::Bool(flag), rest));
        }
    }
    if text.is_empty() {
        Err("missing value".into())
    } else {
        Err(format!(
            "unsupported value `{text}` (use a quoted string, true, false, a number or an array)"
        ))
    }
}

/// A basic or literal string at the start of `text`, or `None` when the
/// text starts with neither quote.
fn parse_string(text: &str) -> Result<Option<(String, &str)>, String> {
    if let Some(rest) = text.strip_prefix('\'') {
        let end = rest.find('\'').ok_or("unterminated string")?;
        return Ok(Some((rest[..end].to_owned(), &rest[end + 1..])));
    }
    let Some(rest) = text.strip_prefix('"') else {
        return Ok(None);
    };
    let mut out = String::new();
    let mut chars = rest.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '"' => return Ok(Some((out, &rest[at + 1..]))),
            '\\' => {
                let (_, escape) = chars.next().ok_or("unterminated string")?;
                match escape {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    'u' | 'U' => {
                        let digits = if escape == 'u' { 4 } else { 8 };
                        let hex: String = (0..digits)
                            .filter_map(|_| chars.next())
                            .map(|(_, c)| c)
                            .collect();
                        let code = (hex.len() == digits)
                            .then(|| u32::from_str_radix(&hex, 16).ok())
                            .flatten()
                            .and_then(char::from_u32)
                            .ok_or_else(|| format!("invalid escape `\\{escape}{hex}`"))?;
                        out.push(code);
                    }
                    other => {
                        return Err(format!(
                            "invalid escape `\\{other}` (write a backslash as `\\\\` or use 'single quotes')"
                        ));
                    }
                }
            }
            c => out.push(c),
        }
    }
    Err("unterminated string".into())
}

/// The directories the settings file is looked up in: beside the
/// executable, then the working directory.
pub fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        dirs.push(dir);
    }
    if let Ok(dir) = std::env::current_dir()
        && !dirs.contains(&dir)
    {
        dirs.push(dir);
    }
    dirs
}

/// The first settings file in `dirs`.
pub fn find(dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter()
        .map(|dir| dir.join(FILE_NAME))
        .find(|path| path.is_file())
}

/// The directories `game`'s data is looked for in, in order: the working
/// directory, the executable's, then for Fallout 1 `../local-corpus`, and
/// the game's usual install directories under `home` (`games/fallout1` or
/// `games/fallout2`, Steam and GOG).
pub fn data_dirs(game: GameProfile, exe_dir: Option<&Path>, home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from(".")];
    dirs.extend(exe_dir.map(Path::to_path_buf));
    let install = match game {
        GameProfile::Fallout1 => {
            dirs.push(PathBuf::from("../local-corpus"));
            "Fallout"
        }
        GameProfile::Fallout2 => "Fallout 2",
    };
    if let Some(home) = home {
        dirs.push(home.join("games").join(game_name(game)));
        #[cfg(not(windows))]
        dirs.extend([
            home.join(".steam/steam/steamapps/common").join(install),
            home.join(".local/share/Steam/steamapps/common")
                .join(install),
            home.join("GOG Games").join(install),
        ]);
        #[cfg(windows)]
        dirs.push(home.join("GOG Games").join(install));
    }
    #[cfg(windows)]
    {
        let x86 = std::env::var_os("ProgramFiles(x86)")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"));
        dirs.extend([
            x86.join(r"Steam\steamapps\common").join(install),
            x86.join(r"GOG Galaxy\Games").join(install),
            x86.join("GOG.com").join(install),
            PathBuf::from(r"C:\GOG Games").join(install),
        ]);
    }
    dirs
}

/// [`data_dirs`] for each game, from this executable's directory and the
/// user's home.
pub fn standard_data_dirs() -> PerGame<Vec<PathBuf>> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    let home = home_dir();
    let dirs = |game| data_dirs(game, exe_dir.as_deref(), home.as_deref());
    PerGame {
        fallout1: dirs(GameProfile::Fallout1),
        fallout2: dirs(GameProfile::Fallout2),
    }
}

/// The user's home directory from `HOME` (or `USERPROFILE`).
pub fn home_dir() -> Option<PathBuf> {
    ["HOME", "USERPROFILE"]
        .into_iter()
        .filter_map(std::env::var_os)
        .find(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// An entry of `dir` named `name` without regard to ASCII case, a
/// directory or a file as asked.
fn entry(dir: &Path, name: &str, directory: bool) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|entry| entry.eq_ignore_ascii_case(name))
        })
        .map(|entry| entry.path())
        .filter(|path| {
            if directory {
                path.is_dir()
            } else {
                path.is_file()
            }
        })
        .collect();
    // Directory order is the file system's: the choice must not be.
    found.sort();
    found.into_iter().next()
}

/// Game data in the first of `dirs` holding both `MASTER.DAT` and
/// `CRITTER.DAT` (in any case), with its sibling `DATA` directory as the
/// loose data root when there is one; the music folder beside the
/// archives is found when the databases are mounted. Paths are absolute.
pub fn discover(dirs: &[PathBuf]) -> Option<Config> {
    dirs.iter().find_map(|dir| {
        let master = entry(dir, "master.dat", false)?;
        let critter = entry(dir, "critter.dat", false)?;
        let absolute = |path: PathBuf| std::path::absolute(&path).unwrap_or(path);
        Some(Config {
            master_dat: Some(absolute(master)),
            critter_dat: Some(absolute(critter)),
            data_root: entry(dir, "data", true).map(absolute),
            ..Default::default()
        })
    })
}

/// [`discover`] for one game: the first of `dirs` whose MASTER.DAT opens
/// in that game's archive format (SPEC-008 for Fallout 1, SPEC-015 for
/// Fallout 2; directory tables only), so neither game's folder is taken
/// for the other's.
pub fn discover_game(dirs: &[PathBuf], game: GameProfile) -> Option<GamePaths> {
    dirs.iter().find_map(|dir| {
        let found = discover(std::slice::from_ref(dir))?.paths();
        let master = found.master_dat.as_deref()?;
        let opens = match game {
            GameProfile::Fallout1 => vault_formats::DatArchive::open(master).is_ok(),
            GameProfile::Fallout2 => vault_formats::Dat2Archive::open(master).is_ok(),
        };
        opens.then_some(found)
    })
}

/// Each game's data, discovered in its own directories.
pub fn discover_games(dirs: &PerGame<Vec<PathBuf>>) -> PerGame<GamePaths> {
    let found = |game| discover_game(dirs.get(game), game).unwrap_or_default();
    PerGame {
        fallout1: found(GameProfile::Fallout1),
        fallout2: found(GameProfile::Fallout2),
    }
}

/// Replace the file at `path` with `bytes`: a temporary sibling is written
/// and renamed over it, so a failed write leaves the old file whole.
pub fn write_replacing(path: &Path, bytes: &[u8]) -> crate::AppResult<()> {
    use crate::storage::{Namespace, StorageBackend, StorageOp};
    // SPEC-023 §3.10: one batch with one put, so a failed write leaves the
    // old file whole. A name that is no `config` key is written as it was.
    if let Some((storage, key)) = crate::storage::config_file(path) {
        let put = [StorageOp::put(key, bytes)];
        return storage
            .commit(Namespace::Config, &put)
            .map_err(|error| format!("{}: {error}", path.display()).into());
    }
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} names no file", path.display()))?;
    let mut temporary = std::ffi::OsString::from(".");
    temporary.push(name);
    temporary.push(".tmp");
    let temporary = path.with_file_name(temporary);
    std::fs::write(&temporary, bytes)
        .and_then(|()| std::fs::rename(&temporary, path))
        .map_err(|error| {
            let _ = std::fs::remove_file(&temporary);
            format!("{}: {error}", path.display()).into()
        })
}

/// Write a discovered configuration into the first of `dirs` that takes
/// it (an existing file is never replaced); `None`, with a warning, when
/// none does.
pub fn persist(config: &Config, dirs: &[PathBuf]) -> Option<PathBuf> {
    use crate::storage::{FsStorage, Namespace, StorageBackend, StorageError, StorageOp};
    let text = match config.to_toml() {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!(%error, "discovered settings not saved");
            return None;
        }
    };
    for dir in dirs {
        let path = dir.join(FILE_NAME);
        let storage = FsStorage::config_at(dir);
        let put = [StorageOp::put(FILE_NAME, text.as_bytes())];
        let written = match storage.read(Namespace::Config, FILE_NAME) {
            Ok(None) => storage.commit(Namespace::Config, &put),
            Ok(Some(_)) => Err(StorageError::Io("the file exists".into())),
            Err(error) => Err(error),
        };
        match written {
            Ok(()) => {
                tracing::info!(file = %path.display(), "discovered game data saved");
                return Some(path);
            }
            Err(error) => {
                tracing::debug!(file = %path.display(), %error, "settings not writable here");
            }
        }
    }
    tracing::warn!("discovered game data could not be saved; continuing without a settings file");
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_round_trip_through_the_file_text() {
        let text = r#"
# Paths with escapes and literal strings.
master_dat = "C:\\Games\\Fallout\\MASTER.DAT"   # the master database
critter_dat = 'C:\Games\Fallout\CRITTER.DAT'
data_root = "/games/fallout/data"
load_order = ["mods/one", 'mods/two',]
game = "fallout1"
preset = "enhanced"
scaling = "aspect_fit"
fullscreen = true
touch_ui = true
debug = false
future_key = "kept"
future_list = []
"#;
        let config = Config::parse(text).unwrap();
        assert_eq!(
            config.master_dat,
            Some(PathBuf::from(r"C:\Games\Fallout\MASTER.DAT"))
        );
        assert_eq!(
            config.critter_dat,
            Some(PathBuf::from(r"C:\Games\Fallout\CRITTER.DAT"))
        );
        assert_eq!(
            config.mod_list,
            Some(ModList::Paths(vec!["mods/one".into(), "mods/two".into()]))
        );
        assert_eq!(config.game, Some(GameProfile::Fallout1));
        assert_eq!(config.preset, Some(Preset::Enhanced));
        assert_eq!(config.scaling, Some(ScalingMode::AspectFit));
        assert_eq!((config.fullscreen, config.debug), (Some(true), Some(false)));
        assert_eq!(config.touch_ui, Some(true));
        assert_eq!(
            Config::parse("touch_ui = false").unwrap().touch_ui,
            Some(false)
        );
        assert_eq!(Config::parse("").unwrap().touch_ui, None);
        assert_eq!(
            Config::parse("game = \"fallout2\"").unwrap().game,
            Some(GameProfile::Fallout2)
        );
        let error = Config::parse("game = \"fallout3\"").unwrap_err();
        assert!(
            error.message.contains("\"fallout1\" or \"fallout2\""),
            "{error}"
        );
        assert_eq!(
            config.unknown,
            vec![
                ("future_key".to_owned(), Value::String("kept".into())),
                ("future_list".to_owned(), Value::Array(Vec::new())),
            ]
        );
        let written = config.to_toml().unwrap();
        assert_eq!(Config::parse(&written).unwrap(), config);
        // A load-order file instead of a list, and an empty file.
        let file = Config::parse("mod_list = \"load_order.toml\"\nscaling = \"integer\"").unwrap();
        assert_eq!(file.mod_list, Some(ModList::File("load_order.toml".into())));
        assert_eq!(file.scaling, Some(ScalingMode::Integer));
        assert_eq!(Config::parse(&file.to_toml().unwrap()).unwrap(), file);
        assert_eq!(
            Config::parse("\u{feff}# only a comment\n\n").unwrap(),
            Config::default()
        );
        assert_eq!(
            Config::parse(r#"x = "tab\there \u00e9 \"q\"""#)
                .unwrap()
                .unknown[0]
                .1,
            Value::String("tab\there \u{e9} \"q\"".into())
        );
    }

    #[test]
    fn save_replaces_the_file_with_relative_paths_and_unknown_keys() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(FILE_NAME);
        std::fs::write(
            &path,
            "audio = false\nmaster_dat = \"MASTER.DAT\"\nnew = [\"x\"]\n",
        )
        .unwrap();
        let mut config = Config::load(&path).unwrap();
        assert_eq!(config.audio, Some(false));
        assert_eq!(config.master_dat, Some(temp.path().join("MASTER.DAT")));
        config.audio = Some(true);
        // An absolute path outside the file's folder (on Windows a
        // root-relative one would take the file's drive on reading).
        let (elsewhere, written) = if cfg!(windows) {
            (r"C:\elsewhere\data", r"C:\\elsewhere\\data")
        } else {
            ("/elsewhere/data", "/elsewhere/data")
        };
        config.data_root = Some(elsewhere.into());
        config.mod_list = Some(ModList::File(temp.path().join("load_order.toml")));
        config.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("master_dat = \"MASTER.DAT\"\n"), "{text}");
        assert!(
            text.contains(&format!("data_root = \"{written}\"\n")),
            "{text}"
        );
        assert!(text.contains("mod_list = \"load_order.toml\"\n"), "{text}");
        assert!(text.contains("audio = true\nnew = [\"x\"]\n"), "{text}");
        assert_eq!(Config::load(&path).unwrap(), config);
        // No temporary file is left behind, and a missing directory fails.
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
        assert!(
            config
                .save(&temp.path().join("missing").join(FILE_NAME))
                .is_err()
        );
        assert!(matches!(
            Config::parse("audio = \"off\""),
            Err(ConfigError { line: 1, .. })
        ));
    }

    #[test]
    fn malformed_lines_are_errors_that_name_the_line() {
        let line = |text: &str| Config::parse(text).unwrap_err().line;
        let message = |text: &str| Config::parse(text).unwrap_err().to_string();
        assert_eq!(line("debug = true\nfullscreen = yes"), 2);
        assert!(message("debug = true\nfullscreen = yes").starts_with("line 2: unsupported value"));
        assert_eq!(line("\n\nmaster_dat"), 3);
        assert_eq!(line("[section]"), 1);
        assert_eq!(line("master_dat = \"unterminated"), 1);
        assert_eq!(line("master_dat = \"C:\\Games\""), 1);
        assert!(message("master_dat = \"C:\\Games\"").contains("invalid escape"));
        assert_eq!(line("a = 01\n"), 1);
        assert_eq!(line("debug = \"true\""), 1);
        assert!(message("fullscreen = \"yes\"").contains("`fullscreen` expects true or false"));
        assert!(message("touch_ui = 1").contains("`touch_ui` expects true or false"));
        assert_eq!(line("preset = \"modern\""), 1);
        assert_eq!(line("scaling = \"stretch\""), 1);
        assert_eq!(line("master_dat = \"a\" trailing"), 1);
        assert_eq!(line("mod_list = [\"a\" \"b\"]"), 1);
        assert_eq!(line("mod_list = [\"a\", true]"), 1);
        assert_eq!(line("mod_list = [\"a\""), 1);
        assert_eq!(line("bad key = true"), 1);
        assert_eq!(line("= true"), 1);
        assert_eq!(line("debug = true\ndebug = false"), 2);
        assert_eq!(line("mod_list = []\nload_order = []"), 2);
        assert_eq!(line("x = true\nx = true"), 2);
        assert_eq!(line("debug = truer"), 1);
        assert_eq!(line("x = \"\\u12\""), 1);
    }

    #[test]
    fn the_enhanced_settings_round_trip_and_default_when_missing() {
        let text = r#"
preset = "enhanced"
viewport_mode = "fixed_640x480"
default_zoom = 1.25
smooth_camera = false
roof_peek = "cursor_xray"
ui_scale = 2
future_number = -1_000
future_float = 5e-1
"#;
        let config = Config::parse(text).unwrap();
        assert_eq!(config.viewport_mode, Some(ViewportMode::Fixed));
        assert_eq!(config.default_zoom, Some(1.25));
        assert_eq!(config.smooth_camera, Some(false));
        assert_eq!(config.roof_peek, Some(RoofPeek::CursorXray));
        assert_eq!(config.ui_scale, Some(UiScale::Scale(2.0)));
        assert_eq!(
            config.unknown,
            vec![
                ("future_number".to_owned(), Value::Integer(-1000)),
                ("future_float".to_owned(), Value::Float(0.5)),
            ]
        );
        assert_eq!(
            config.enhanced_settings(),
            EnhancedSettings {
                viewport: ViewportMode::Fixed,
                default_zoom: 1.25,
                smooth_camera: false,
                roof_peek: RoofPeek::CursorXray,
                ui_scale: Some(2.0),
                navigation_only: false,
            }
        );
        // The writer keeps every key, numbers as numbers (floats with a
        // fraction), and reads back the same settings.
        let written = config.to_toml().unwrap();
        for line in [
            "viewport_mode = \"fixed_640x480\"\n",
            "default_zoom = 1.25\n",
            "smooth_camera = false\n",
            "roof_peek = \"cursor_xray\"\n",
            "ui_scale = 2.0\n",
            "future_number = -1000\n",
            "future_float = 0.5\n",
        ] {
            assert!(written.contains(line), "{line}: {written}");
        }
        assert_eq!(Config::parse(&written).unwrap(), config);
        // `auto`, the other names, and a zoom of 1.1 written at its own
        // precision.
        let other = Config::parse(
            "viewport_mode = \"unlocked_widescreen\"\nroof_peek = 'hidden'\nui_scale = \"auto\"\n\
             default_zoom = 1.1\n",
        )
        .unwrap();
        assert_eq!(other.ui_scale, Some(UiScale::Auto));
        assert_eq!(other.enhanced_settings().ui_scale, None);
        assert_eq!(other.roof_peek, Some(RoofPeek::Hidden));
        let written = other.to_toml().unwrap();
        assert!(written.contains("ui_scale = \"auto\"\n"), "{written}");
        assert!(written.contains("default_zoom = 1.1\n"), "{written}");
        assert_eq!(Config::parse(&written).unwrap(), other);
        // Missing keys take the defaults; whole numbers are numbers too.
        assert_eq!(
            Config::default().enhanced_settings(),
            EnhancedSettings::default()
        );
        let zoom = Config::parse("default_zoom = 3\nsmooth_camera = true").unwrap();
        assert_eq!(
            zoom.enhanced_settings(),
            EnhancedSettings {
                default_zoom: 3.0,
                ..EnhancedSettings::default()
            }
        );
        // A Vanilla file writes none of them.
        let vanilla = Config::parse("preset = \"vanilla\"\n").unwrap();
        assert_eq!(
            vanilla.to_toml().unwrap(),
            "# Vault Engine launcher settings.\npreset = \"vanilla\"\n"
        );
    }

    #[test]
    fn the_game_speed_parses_writes_round_trips_and_names_bad_values() {
        // Numbers, whole or not; missing is 1.0x.
        assert_eq!(Config::default().game_speed(), GameSpeed::Normal);
        for (text, speed) in [
            ("game_speed = 1", GameSpeed::Normal),
            ("game_speed = 1.0", GameSpeed::Normal),
            ("game_speed = 1.5", GameSpeed::Fast),
            ("game_speed = 15e-1", GameSpeed::Fast),
            ("game_speed = 2", GameSpeed::Faster),
        ] {
            let config = Config::parse(text).unwrap();
            assert_eq!(config.game_speed, Some(speed), "{text}");
            assert_eq!(config.game_speed(), speed);
            // Written as a float, and read back the same.
            let written = config.to_toml().unwrap();
            let line = format!("game_speed = {}\n", float(f64::from(speed.factor())));
            assert!(written.contains(&line), "{written}");
            assert_eq!(Config::parse(&written).unwrap(), config);
        }
        let written = Config::parse("preset = \"enhanced\"\ngame_speed = 1.5\n")
            .unwrap()
            .to_toml()
            .unwrap();
        assert!(written.contains("game_speed = 1.5\n"), "{written}");
        // A known key: not kept as unknown, and only once.
        assert!(Config::parse("game_speed = 2").unwrap().unknown.is_empty());
        assert_eq!(
            Config::parse("game_speed = 2\ngame_speed = 1")
                .unwrap_err()
                .to_string(),
            "line 2: duplicate key `game_speed`"
        );
        // Anything else names its line and the choices.
        for (text, message) in [
            ("game_speed = 3", "`game_speed` is 1.0, 1.5 or 2.0, not 3"),
            ("game_speed = 1.25", "not 1.25"),
            ("game_speed = 0", "not 0"),
            ("game_speed = \"2x\"", "`game_speed` is 1.0, 1.5 or 2.0"),
            ("game_speed = true", "`game_speed` is 1.0, 1.5 or 2.0"),
        ] {
            let error = Config::parse(&format!("audio = true\n{text}")).unwrap_err();
            assert_eq!(error.line, 2, "{text}");
            assert!(error.to_string().contains(message), "{text}: {error}");
        }
    }

    /// SPEC-022 §3.1 `render_path`: either path parses, the file's own
    /// line survives a rewrite, a file without one gets none, and anything
    /// else names its line.
    #[test]
    fn the_render_path_parses_round_trips_and_names_bad_values() {
        assert_eq!(Config::default().render_path, None);
        for (text, path) in [
            ("render_path = \"reference\"", RenderPath::Reference),
            ("render_path = 'instanced'", RenderPath::Instanced),
        ] {
            let config = Config::parse(text).unwrap();
            assert_eq!(config.render_path, Some(path), "{text}");
            assert!(config.unknown.is_empty());
            let written = config.to_toml().unwrap();
            assert!(
                written.contains(&format!("render_path = \"{}\"\n", path.name())),
                "{written}"
            );
            assert_eq!(Config::parse(&written).unwrap(), config);
        }
        let written = Config::parse("preset = \"enhanced\"\n")
            .unwrap()
            .to_toml()
            .unwrap();
        assert!(!written.contains("render_path"), "{written}");
        assert_eq!(
            Config::parse("render_path = \"reference\"\nrender_path = \"instanced\"")
                .unwrap_err()
                .to_string(),
            "line 2: duplicate key `render_path`"
        );
        for (text, message) in [
            (
                "render_path = \"gpu\"",
                "`render_path` is \"reference\" or \"instanced\", not \"gpu\"",
            ),
            ("render_path = true", "`render_path` expects a string"),
        ] {
            let error = Config::parse(&format!("audio = true\n{text}")).unwrap_err();
            assert_eq!(error.line, 2, "{text}");
            assert!(error.to_string().contains(message), "{text}: {error}");
        }
    }

    #[test]
    fn bad_enhanced_values_and_numbers_name_their_line() {
        let error = |text: &str| Config::parse(text).unwrap_err();
        for (text, message) in [
            (
                "default_zoom = 4",
                "`default_zoom` is a number from 0.5 to 3.0, not 4",
            ),
            ("default_zoom = 0.25", "not 0.25"),
            ("default_zoom = \"2\"", "`default_zoom` is a number"),
            (
                "ui_scale = 0",
                "`ui_scale` is \"auto\" or a number from 0.5 to 8, not 0",
            ),
            ("ui_scale = \"big\"", "not \"big\""),
            ("ui_scale = true", "`ui_scale` is \"auto\""),
            (
                "viewport_mode = \"wide\"",
                "`viewport_mode` is \"fixed_640x480\"",
            ),
            ("roof_peek = \"xray\"", "`roof_peek` is \"vanilla\""),
            ("smooth_camera = 1", "`smooth_camera` expects true or false"),
            ("mod_list = 2", "expects an array of paths or a path"),
            ("x = 1__0", "invalid number `1__0`"),
            ("x = 1.", "invalid number `1.`"),
            ("x = 1.e3", "invalid number"),
            ("x = 1e", "invalid number"),
            ("x = 007", "invalid number"),
            ("x = 1_", "invalid number"),
            ("x = 0x10", "invalid number"),
            ("x = 1e999", "invalid number"),
            ("x = 99999999999999999999", "out of range"),
        ] {
            let error = error(&format!("audio = true\n{text}"));
            assert_eq!(error.line, 2, "{text}");
            assert!(error.to_string().contains(message), "{text}: {error}");
        }
        assert_eq!(
            error("default_zoom = 1\ndefault_zoom = 2").to_string(),
            "line 2: duplicate key `default_zoom`"
        );
        // A number then a comment or nothing; signs and exponents.
        let config = Config::parse("a = +3 # three\nb = -0.5e+1\nc = 0\nd = 1_000.000_1").unwrap();
        assert_eq!(
            config.unknown,
            vec![
                ("a".to_owned(), Value::Integer(3)),
                ("b".to_owned(), Value::Float(-5.0)),
                ("c".to_owned(), Value::Integer(0)),
                ("d".to_owned(), Value::Float(1000.0001)),
            ]
        );
        assert_eq!(float(-5.0), "-5.0");
        assert_eq!(float(1e300), "1e300");
        assert_eq!(parse_zoom("0.75"), Some(0.75));
        assert_eq!(parse_zoom("3.5"), None);
    }

    #[test]
    fn relative_paths_follow_the_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(FILE_NAME);
        std::fs::write(
            &path,
            "master_dat = \"MASTER.DAT\"\nmod_list = [\"mods/a\"]\nnew = true\n",
        )
        .unwrap();
        let config = Config::load(&path).unwrap();
        assert_eq!(config.master_dat, Some(temp.path().join("MASTER.DAT")));
        assert_eq!(
            config.mod_list,
            Some(ModList::Paths(vec![temp.path().join("mods/a")]))
        );
        assert_eq!(config.unknown.len(), 1);
        std::fs::write(&path, "debug = 1\n").unwrap();
        let error = Config::load(&path).unwrap_err().to_string();
        assert!(
            error.contains(FILE_NAME) && error.contains("line 1"),
            "{error}"
        );
        assert_eq!(
            find(&[temp.path().join("none"), temp.path().into()]),
            Some(path)
        );
    }

    #[test]
    fn discovery_finds_the_databases_in_any_case_and_saves_them_once() {
        let temp = tempfile::tempdir().unwrap();
        let empty = temp.path().join("empty");
        let half = temp.path().join("half");
        let install = temp.path().join("install");
        for dir in [&empty, &half, &install] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(half.join("MASTER.DAT"), b"").unwrap();
        std::fs::write(install.join("master.dat"), b"").unwrap();
        std::fs::write(install.join("Critter.Dat"), b"").unwrap();
        // A directory named like an archive is not one.
        std::fs::create_dir_all(empty.join("MASTER.DAT")).unwrap();
        std::fs::write(empty.join("CRITTER.DAT"), b"").unwrap();
        let dirs = [
            temp.path().join("missing"),
            empty.clone(),
            half.clone(),
            install.clone(),
        ];
        let found = discover(&dirs).unwrap();
        assert_eq!(found.master_dat, Some(install.join("master.dat")));
        assert_eq!(found.critter_dat, Some(install.join("Critter.Dat")));
        assert_eq!(found.data_root, None);
        std::fs::create_dir_all(install.join("DATA/SOUND/MUSIC")).unwrap();
        assert_eq!(
            discover(&dirs).unwrap().data_root,
            Some(install.join("DATA"))
        );
        assert!(discover(&dirs[..3]).is_none());
        // The first writable directory takes the file; an existing one is
        // never replaced, so the next save goes on to the next directory.
        let found = discover(&dirs).unwrap();
        let saved = persist(
            &found,
            &[temp.path().join("missing"), half.clone(), install.clone()],
        );
        assert_eq!(saved, Some(half.join(FILE_NAME)));
        assert_eq!(Config::load(&half.join(FILE_NAME)).unwrap(), found);
        assert_eq!(
            persist(&found, &[half.clone(), install.clone()]),
            Some(install.join(FILE_NAME))
        );
        assert_eq!(persist(&found, &[temp.path().join("missing")]), None);
    }

    #[test]
    fn each_game_keeps_its_profile_through_switches_and_the_file() {
        let paths = |root: &str| GamePaths {
            master_dat: Some(format!("{root}/MASTER.DAT").into()),
            critter_dat: Some(format!("{root}/CRITTER.DAT").into()),
            data_root: None,
        };
        // A file from before profiles: its top-level paths are Fallout
        // 1's, and a launch of Fallout 2 still reads them.
        let legacy =
            Config::parse("master_dat = \"/fo1/MASTER.DAT\"\ncritter_dat = \"/fo1/CRITTER.DAT\"\n")
                .unwrap();
        assert_eq!(legacy.profile(GameProfile::Fallout1), paths("/fo1"));
        assert!(legacy.profile(GameProfile::Fallout2).is_empty());
        assert_eq!(legacy.launch_paths(GameProfile::Fallout2), paths("/fo1"));
        // Switching keeps the old game's paths and brings up the other's
        // (none yet).
        let mut config = legacy.clone();
        config.switch_game(GameProfile::Fallout2);
        assert_eq!(config.game, Some(GameProfile::Fallout2));
        assert!(config.paths().is_empty());
        assert_eq!(config.profile(GameProfile::Fallout1), paths("/fo1"));
        config.set_paths(paths("/fo2"));
        let text = config.to_toml().unwrap();
        assert!(
            text.ends_with(
                "game = \"fallout2\"\n\n\
                 [profiles.fallout1]\nmaster_dat = \"/fo1/MASTER.DAT\"\n\
                 critter_dat = \"/fo1/CRITTER.DAT\"\n\n\
                 [profiles.fallout2]\nmaster_dat = \"/fo2/MASTER.DAT\"\n\
                 critter_dat = \"/fo2/CRITTER.DAT\"\n"
            ),
            "{text}"
        );
        assert!(
            text.starts_with(
                "# Vault Engine launcher settings.\nmaster_dat = \"/fo2/MASTER.DAT\"\n"
            )
        );
        let read = Config::parse(&text).unwrap();
        assert_eq!(read, config);
        assert_eq!(read.launch_paths(GameProfile::Fallout1), paths("/fo1"));
        assert_eq!(read.launch_paths(GameProfile::Fallout2), paths("/fo2"));
        // And back, through the file again.
        let mut back = read;
        back.switch_game(GameProfile::Fallout1);
        assert_eq!(back.paths(), paths("/fo1"));
        let back = Config::parse(&back.to_toml().unwrap()).unwrap();
        assert_eq!(
            (back.paths(), back.profile(GameProfile::Fallout2)),
            (paths("/fo1"), paths("/fo2"))
        );
        // The active game's table fills absent top-level keys, which win
        // when both are there.
        let tables_only = "game = \"fallout2\"\n[profiles.fallout2] # its own\n\
             master_dat = \"/fo2/MASTER.DAT\"\ncritter_dat = \"/fo2/CRITTER.DAT\"\n";
        assert_eq!(Config::parse(tables_only).unwrap().paths(), paths("/fo2"));
        let both = format!("master_dat = \"/top/MASTER.DAT\"\n{tables_only}");
        let both = Config::parse(&both).unwrap();
        assert_eq!(both.master_dat, Some("/top/MASTER.DAT".into()));
        assert_eq!(both.critter_dat, None);
        assert!(both.profiles.fallout2.is_empty());
        // Only the two profile tables, each once, with path keys.
        let error = |text: &str| Config::parse(text).unwrap_err().to_string();
        for (text, message) in [
            (
                "[profiles.fallout3]",
                "line 1: unsupported table `[profiles.fallout3]`",
            ),
            ("[profiles]", "unsupported table"),
            ("[profiles.fallout1", "unterminated table header"),
            (
                "[profiles.fallout1] x",
                "unexpected `x` after the table header",
            ),
            (
                "[profiles.fallout1]\n[profiles.fallout1]",
                "line 2: duplicate table",
            ),
            (
                "[profiles.fallout1]\ngame = \"fallout1\"",
                "line 2: `game` is not a profile setting",
            ),
            (
                "[profiles.fallout2]\ndata_root = true",
                "`data_root` expects a string",
            ),
            (
                "[profiles.fallout2]\ndata_root = \"a\"\ndata_root = \"b\"",
                "line 3: duplicate key",
            ),
        ] {
            assert!(error(text).contains(message), "{text}: {}", error(text));
        }
        // Table paths are relative to the file like the others, and are
        // written back relative.
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(FILE_NAME);
        std::fs::write(
            &path,
            "[profiles.fallout2]\nmaster_dat = \"fo2/master.dat\"\n",
        )
        .unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(
            loaded.profile(GameProfile::Fallout2).master_dat,
            Some(temp.path().join("fo2/master.dat"))
        );
        loaded.save(&path).unwrap();
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("[profiles.fallout2]\nmaster_dat = \"fo2/master.dat\"\n")
        );
    }

    #[test]
    fn discovery_takes_each_games_own_archive_format() {
        let temp = tempfile::tempdir().unwrap();
        let (fallout1, fallout2) = (temp.path().join("fo1"), temp.path().join("fo2"));
        for dir in [&fallout1, &fallout2] {
            std::fs::create_dir_all(dir).unwrap();
        }
        // Empty archives of each format (SPEC-008: four zero words;
        // SPEC-015: an empty tree and its two trailing words).
        std::fs::write(fallout1.join("MASTER.DAT"), [0u8; 16]).unwrap();
        std::fs::write(fallout1.join("CRITTER.DAT"), [0u8; 16]).unwrap();
        let dat2 = vault_fixtures::dat2::archive(&[]);
        std::fs::write(fallout2.join("master.dat"), &dat2).unwrap();
        std::fs::write(fallout2.join("critter.dat"), &dat2).unwrap();
        std::fs::create_dir_all(fallout2.join("data")).unwrap();
        let dirs = PerGame {
            fallout1: vec![fallout2.clone(), fallout1.clone()],
            fallout2: vec![fallout1.clone(), fallout2.clone()],
        };
        let found = discover_games(&dirs);
        assert_eq!(found.fallout1.master_dat, Some(fallout1.join("MASTER.DAT")));
        assert_eq!(found.fallout2.master_dat, Some(fallout2.join("master.dat")));
        assert_eq!(found.fallout2.data_root, Some(fallout2.join("data")));
        let config = Config::with_profiles(GameProfile::Fallout2, found.clone());
        assert_eq!(config.paths(), found.fallout2);
        assert_eq!(config.profile(GameProfile::Fallout1), found.fallout1);
        assert!(config.profiles.fallout2.is_empty());
        assert!(discover_game(&[fallout1], GameProfile::Fallout2).is_none());
    }

    #[test]
    fn install_directories_come_after_the_local_ones() {
        let home = Path::new("/home/someone");
        let exe = Some(Path::new("/opt/vault"));
        let dirs = data_dirs(GameProfile::Fallout1, exe, Some(home));
        assert_eq!(dirs[0], PathBuf::from("."));
        assert_eq!(dirs[1], PathBuf::from("/opt/vault"));
        assert_eq!(dirs[2], PathBuf::from("../local-corpus"));
        assert_eq!(dirs[3], home.join("games/fallout1"));
        let fallout2 = data_dirs(GameProfile::Fallout2, exe, Some(home));
        assert_eq!(&fallout2[..2], &dirs[..2]);
        assert_eq!(fallout2[2], home.join("games/fallout2"));
        assert!(!fallout2.contains(&PathBuf::from("../local-corpus")));
        #[cfg(not(windows))]
        {
            assert!(dirs.contains(&home.join(".steam/steam/steamapps/common/Fallout")));
            assert!(dirs.contains(&home.join(".local/share/Steam/steamapps/common/Fallout")));
            assert!(dirs.contains(&home.join("GOG Games/Fallout")));
            assert!(fallout2.contains(&home.join(".steam/steam/steamapps/common/Fallout 2")));
            assert!(fallout2.contains(&home.join("GOG Games/Fallout 2")));
        }
        for game in GAMES {
            assert_eq!(
                data_dirs(game, None, None).len(),
                1 + usize::from(game == GameProfile::Fallout1) + usize::from(cfg!(windows)) * 4
            );
        }
    }

    #[test]
    #[ignore = "needs the local corpus"]
    fn discovery_finds_the_local_corpus_databases() {
        let root = std::env::var_os("VAULT_CORPUS")
            .map(PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../local-corpus"));
        if !root.join("MASTER.DAT").is_file() {
            eprintln!("skipping: no MASTER.DAT in {}", root.display());
            return;
        }
        let found = discover(&[root.join("missing"), root.clone()]).unwrap();
        assert!(found.has_game_data());
        assert!(found.master_dat.as_ref().unwrap().is_file());
        assert!(found.critter_dat.as_ref().unwrap().is_file());
        assert!(found.master_dat.unwrap().is_absolute());
        // What discovery finds is what a saved file reads back.
        assert_eq!(
            Config::parse(&found_text(&root)).unwrap(),
            discover(&[root]).unwrap()
        );
    }

    fn found_text(root: &Path) -> String {
        discover(&[root.to_path_buf()]).unwrap().to_toml().unwrap()
    }

    #[test]
    fn sfall_sits_in_its_own_table_and_round_trips() {
        let config = Config::parse("game = \"fallout2\"\n[scripting]\nsfall = true\n").unwrap();
        assert_eq!(config.sfall, Some(true));
        assert_eq!(Config::parse("").unwrap().sfall, None);
        let written = config.to_toml().unwrap();
        assert!(written.contains("[scripting]\nsfall = true\n"), "{written}");
        assert_eq!(Config::parse(&written).unwrap().sfall, Some(true));
        let message = |text: &str| Config::parse(text).unwrap_err().to_string();
        assert!(message("[scripting]\nsfall = 1").contains("expects true or false"));
        assert!(message("[scripting]\nother = true").contains("not a scripting setting"));
        assert!(message("[scripting]\n[scripting]").contains("duplicate table"));
        assert!(message("[scripting]\nsfall = true\nsfall = false").contains("duplicate key"));
    }
}
