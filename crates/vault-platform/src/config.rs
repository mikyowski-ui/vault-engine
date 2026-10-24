//! `vault-engine.toml`: the launcher settings a zero-argument start reads,
//! and the game-data discovery that writes them the first time.
//!
//! The file is a small TOML subset read and written by hand: one
//! `key = value` per line, where a value is a basic `"string"` (with the
//! `\\ \" \n \t \r \uXXXX \UXXXXXXXX` escapes), a literal `'string'`,
//! `true` or `false`, or a one-line array of strings; `#` starts a comment.
//! Tables are not used. Relative paths are relative to the file's
//! directory.
//!
//! This is engine infrastructure, not part of any specification: it only
//! decides which game data and display options the shell starts with.
use std::{
    fmt,
    path::{Path, PathBuf},
};
use vault_render::ScalingMode;

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

/// A value of the TOML subset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    String(String),
    Bool(bool),
    Array(Vec<String>),
}

/// `mod_list` (alias `load_order`): the mods in load order, or the path of
/// a `load_order.toml` that lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModList {
    Paths(Vec<PathBuf>),
    File(PathBuf),
}

/// The settings in `vault-engine.toml`; an absent key is `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub master_dat: Option<PathBuf>,
    pub critter_dat: Option<PathBuf>,
    /// The loose data directory both databases consult first.
    pub data_root: Option<PathBuf>,
    pub mod_list: Option<ModList>,
    pub preset: Option<Preset>,
    pub scaling: Option<ScalingMode>,
    pub fullscreen: Option<bool>,
    pub debug: Option<bool>,
    /// Keys this engine does not know, in file order: ignored, but kept
    /// so that a rewrite does not lose them.
    pub unknown: Vec<(String, Value)>,
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
            if trimmed.starts_with('[') {
                return Err(error("tables are not supported".into()));
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
                    Value::Bool(_) => {
                        return Err(format!("`{key}` expects an array of paths or a path"));
                    }
                })
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
            "debug" => self.debug = Some(boolean(value)?),
            _ => self.unknown.push((key.to_owned(), value)),
        }
        Ok(())
    }

    /// The file text: the known keys in a fixed order, then the unknown
    /// ones. A path that is not valid UTF-8 cannot be written.
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
        if let Some(preset) = self.preset {
            line("preset", quote(preset.name()));
        }
        if let Some(scaling) = self.scaling {
            line("scaling", quote(scaling_name(scaling)));
        }
        for (key, value) in [("fullscreen", self.fullscreen), ("debug", self.debug)] {
            if let Some(value) = value {
                line(key, value.to_string());
            }
        }
        for (key, value) in &self.unknown {
            line(
                key,
                match value {
                    Value::String(text) => quote(text),
                    Value::Bool(flag) => flag.to_string(),
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
        Ok(out)
    }

    /// Relative paths taken relative to `base` (the file's directory).
    pub fn relative_to(mut self, base: &Path) -> Self {
        let join = |path: &mut PathBuf| {
            if path.is_relative() {
                *path = base.join(&*path);
            }
        };
        for path in [
            &mut self.master_dat,
            &mut self.critter_dat,
            &mut self.data_root,
        ]
        .into_iter()
        .flatten()
        {
            join(path);
        }
        match &mut self.mod_list {
            Some(ModList::Paths(paths)) => paths.iter_mut().for_each(join),
            Some(ModList::File(file)) => join(file),
            None => {}
        }
        self
    }

    /// Read a settings file, its relative paths resolved against its
    /// directory; unknown keys are reported and ignored.
    pub fn load(path: &Path) -> crate::AppResult<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let config = Self::parse(&text).map_err(|error| format!("{}: {error}", path.display()))?;
        for (key, _) in &config.unknown {
            tracing::warn!(file = %path.display(), key, "unknown setting ignored");
        }
        let base = path.parent().unwrap_or(Path::new(""));
        Ok(config.relative_to(base))
    }

    /// Whether the settings name game data: both databases, or a loose
    /// data directory.
    pub fn has_game_data(&self) -> bool {
        (self.master_dat.is_some() && self.critter_dat.is_some()) || self.data_root.is_some()
    }
}

const KNOWN: [&str; 9] = [
    "master_dat",
    "critter_dat",
    "data_root",
    "mod_list",
    "preset",
    "scaling",
    "fullscreen",
    "debug",
    "load_order",
];

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
            "unsupported value `{text}` (use a quoted string, true, false or an array)"
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

/// The directories game data is looked for in, in order: the working
/// directory, the executable's, `../local-corpus`, and the usual Steam and
/// GOG install directories under `home`.
pub fn data_dirs(exe_dir: Option<&Path>, home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from(".")];
    dirs.extend(exe_dir.map(Path::to_path_buf));
    dirs.push(PathBuf::from("../local-corpus"));
    if let Some(home) = home {
        #[cfg(not(windows))]
        dirs.extend([
            home.join(".steam/steam/steamapps/common/Fallout"),
            home.join(".local/share/Steam/steamapps/common/Fallout"),
            home.join("GOG Games/Fallout"),
        ]);
        #[cfg(windows)]
        dirs.push(home.join("GOG Games").join("Fallout"));
    }
    #[cfg(windows)]
    {
        let x86 = std::env::var_os("ProgramFiles(x86)")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"));
        dirs.extend([
            x86.join(r"Steam\steamapps\common\Fallout"),
            x86.join(r"GOG Galaxy\Games\Fallout"),
            x86.join(r"GOG.com\Fallout"),
            PathBuf::from(r"C:\GOG Games\Fallout"),
        ]);
    }
    dirs
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

/// Write a discovered configuration into the first of `dirs` that takes
/// it (an existing file is never replaced); `None`, with a warning, when
/// none does.
pub fn persist(config: &Config, dirs: &[PathBuf]) -> Option<PathBuf> {
    use std::io::Write;
    let text = match config.to_toml() {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!(%error, "discovered settings not saved");
            return None;
        }
    };
    for dir in dirs {
        let path = dir.join(FILE_NAME);
        let written = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .and_then(|mut file| file.write_all(text.as_bytes()));
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
preset = "enhanced"
scaling = "aspect_fit"
fullscreen = true
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
        assert_eq!(config.preset, Some(Preset::Enhanced));
        assert_eq!(config.scaling, Some(ScalingMode::AspectFit));
        assert_eq!((config.fullscreen, config.debug), (Some(true), Some(false)));
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
        assert_eq!(line("a = 1\n"), 1);
        assert_eq!(line("debug = \"true\""), 1);
        assert!(message("fullscreen = \"yes\"").contains("`fullscreen` expects true or false"));
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
    fn install_directories_come_after_the_local_ones() {
        let home = Path::new("/home/someone");
        let dirs = data_dirs(Some(Path::new("/opt/vault")), Some(home));
        assert_eq!(dirs[0], PathBuf::from("."));
        assert_eq!(dirs[1], PathBuf::from("/opt/vault"));
        assert_eq!(dirs[2], PathBuf::from("../local-corpus"));
        #[cfg(not(windows))]
        {
            assert!(dirs.contains(&home.join(".steam/steam/steamapps/common/Fallout")));
            assert!(dirs.contains(&home.join(".local/share/Steam/steamapps/common/Fallout")));
            assert!(dirs.contains(&home.join("GOG Games/Fallout")));
        }
        assert_eq!(
            data_dirs(None, None).len(),
            2 + usize::from(cfg!(windows)) * 4
        );
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
}
