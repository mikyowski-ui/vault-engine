//! `vault-mod.toml`: the optional manifest at the root of a mod layer's
//! loose folder, and the writer of a self-contained mod folder (the
//! manifest, the layer's files and a `README.txt`). Engine extension, not
//! part of any specification.
//!
//! The manifest names the mod (`name`, `version`, `author`,
//! `description`) and may carry an `[enhanced]` table with the `enhanced`
//! preset's launch settings, which an enabled layer applies on launch
//! below the command line's flags:
//!
//! ```toml
//! name = "Tactical camera"
//! version = "1.0.0"
//!
//! [enhanced]
//! preset = "enhanced"
//! viewport_mode = "unlocked_widescreen"
//! default_zoom = 1.25
//! smooth_camera = true
//! roof_peek = "cursor_xray"
//! ```
//!
//! It may also register additions without replacing whole files
//! ([`ModRegistry`]): scripts beyond `scripts.lst`, maps beyond the
//! SPEC-008 §3.10 table, and record patches of the campaign tables:
//!
//! ```toml
//! [[scripts]]
//! file = "camdemo.int"   # under scripts/
//! index = 4096           # past the base scripts.lst, unique across mods
//! local_vars = 0
//!
//! [[maps]]
//! file = "cam_demo.map"  # under maps/
//! number = 100           # 66 or more, unique across mods
//!
//! [[patches]]
//! table = "towns"
//! file = "data/mymod/towns.patch.txt"
//! ```
//!
//! The file is the same TOML subset as `load_order.toml`: strings, `true`
//! and `false`, numbers, `#` comments, the `[enhanced]` table and the
//! three array tables. Unknown keys and tables are errors (a misspelt
//! setting would otherwise do nothing).
use crate::vfs::{Vfs, VfsError, VfsPath, strip_comment, toml_string};
use std::{collections::BTreeMap, fmt, io, path::Path};

/// The manifest's file name at a layer's root.
pub const MANIFEST: &str = "vault-mod.toml";
/// The exported folder's instructions.
pub const README: &str = "README.txt";

/// `preset`'s values.
pub const PRESETS: [&str; 2] = ["vanilla", "enhanced"];
/// `viewport_mode`'s values.
pub const VIEWPORT_MODES: [&str; 2] = ["fixed_640x480", "unlocked_widescreen"];
/// `roof_peek`'s values.
pub const ROOF_PEEKS: [&str; 3] = ["vanilla", "cursor_xray", "hidden"];
/// `default_zoom`'s range.
pub const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.5..=3.0;
/// A registered script's `index` range (a script list row number).
pub const SCRIPT_INDEXES: std::ops::RangeInclusive<u32> = 0..=0x00ff_ffff;
/// A registered script's `local_vars` limit.
pub const MAX_LOCALS: u32 = 1_000_000;
/// A registered map's `number` range: past SPEC-008 §3.10's table of 66
/// maps, within a save's 16-bit map number.
pub const MAP_NUMBERS: std::ops::RangeInclusive<u32> = 66..=0xffff;
/// The largest manifest read from a layer.
const MANIFEST_BYTES: usize = 1 << 20;

/// Which part of the manifest a line is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Top,
    Enhanced,
    Script,
    Map,
    Patch,
}

/// A `[[scripts]]` entry: `scripts/<file>` answers script list row
/// `index`, which the base `scripts.lst` must not have.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptRegistration {
    /// The file name under `scripts/`, lowercase.
    pub file: String,
    pub index: u32,
    pub local_vars: u32,
}

/// A `[[maps]]` entry: `maps/<file>` is map `number`, reachable by its
/// name through `load_map` and by number from transitions and saves.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapRegistration {
    /// The file name under `maps/`, lowercase.
    pub file: String,
    pub number: u32,
}

/// A `[[patches]]` entry: `file` replaces records of campaign table
/// `table` (`<record index>|<fields>` lines).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatchRegistration {
    pub table: String,
    /// A VFS path.
    pub file: String,
}

/// A parsed `vault-mod.toml`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModManifest {
    /// Shown in the launcher's mod list; required and not empty.
    pub name: String,
    pub version: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    /// The `[enhanced]` table, when present.
    pub enhanced: Option<EnhancedOverrides>,
    pub scripts: Vec<ScriptRegistration>,
    pub maps: Vec<MapRegistration>,
    pub patches: Vec<PatchRegistration>,
}

/// The `[enhanced]` table: launch settings, each validated against its
/// values (an absent key leaves the setting as the settings file has it).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EnhancedOverrides {
    /// One of [`PRESETS`].
    pub preset: Option<String>,
    /// One of [`VIEWPORT_MODES`].
    pub viewport_mode: Option<String>,
    /// In [`ZOOM_RANGE`].
    pub default_zoom: Option<f32>,
    pub smooth_camera: Option<bool>,
    /// One of [`ROOF_PEEKS`].
    pub roof_peek: Option<String>,
}

/// A malformed manifest: the line (one-based; 0 for the whole file) and
/// why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(formatter, "{MANIFEST}: {}", self.message)
        } else {
            write!(formatter, "{MANIFEST} line {}: {}", self.line, self.message)
        }
    }
}

impl std::error::Error for ManifestError {}

/// One value of the subset.
enum Value {
    String(String),
    Bool(bool),
    Number(f64),
}

fn value(text: &str) -> Result<(Value, &str), String> {
    let text = text.trim_start();
    if text.starts_with(['"', '\'']) {
        let (string, rest) = toml_string(text).map_err(|error| match error {
            crate::VfsError::LoadOrder(message) => message,
            other => other.to_string(),
        })?;
        return Ok((Value::String(string), rest));
    }
    for (word, flag) in [("true", true), ("false", false)] {
        if let Some(rest) = text.strip_prefix(word)
            && !rest.starts_with(|ch: char| ch.is_ascii_alphanumeric() || ch == '_')
        {
            return Ok((Value::Bool(flag), rest));
        }
    }
    let end = text
        .find(|ch: char| !(ch.is_ascii_digit() || matches!(ch, '+' | '-' | '.' | '_' | 'e' | 'E')))
        .unwrap_or(text.len());
    let number = text[..end].replace('_', "");
    match number.parse::<f64>() {
        Ok(number) if number.is_finite() && end > 0 => Ok((Value::Number(number), &text[end..])),
        _ => Err(format!(
            "expected a string, true, false or a number at {text:?}"
        )),
    }
}

impl ModManifest {
    /// Parse a manifest; the first malformed line is an error naming it.
    pub fn parse(text: &str) -> Result<Self, ManifestError> {
        let mut manifest = Self::default();
        let mut named = false;
        // Keys seen in the current section (each `[[...]]` entry is one).
        let mut seen: Vec<String> = Vec::new();
        let mut section = Section::Top;
        // Where the current array entry began, for its missing-key error.
        let mut entry_line = 0;
        for (index, raw) in text.lines().enumerate() {
            let line = index + 1;
            let error = |message: String| ManifestError { line, message };
            let raw = if index == 0 {
                raw.strip_prefix('\u{feff}').unwrap_or(raw)
            } else {
                raw
            };
            let trimmed = strip_comment(raw).trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(header) = trimmed.strip_prefix('[') {
                Self::finish(section, entry_line, &seen)?;
                seen.clear();
                entry_line = line;
                if let Some(array) = header.strip_prefix('[') {
                    let name = array
                        .strip_suffix("]]")
                        .map(str::trim)
                        .ok_or_else(|| error("expected `[[table]]`".into()))?;
                    section = match name {
                        "scripts" => {
                            manifest.scripts.push(ScriptRegistration::default());
                            Section::Script
                        }
                        "maps" => {
                            manifest.maps.push(MapRegistration::default());
                            Section::Map
                        }
                        "patches" => {
                            manifest.patches.push(PatchRegistration::default());
                            Section::Patch
                        }
                        _ => {
                            return Err(error(format!(
                                "unknown table `[[{name}]]` (`[[scripts]]`, `[[maps]]` or \
                                 `[[patches]]`)"
                            )));
                        }
                    };
                    continue;
                }
                let table = header
                    .strip_suffix(']')
                    .map(str::trim)
                    .ok_or_else(|| error("expected `[table]`".into()))?;
                if table != "enhanced" {
                    return Err(error(format!(
                        "unknown table `[{table}]` (only `[enhanced]`)"
                    )));
                }
                if manifest.enhanced.is_some() {
                    return Err(error("duplicate table `[enhanced]`".into()));
                }
                manifest.enhanced = Some(EnhancedOverrides::default());
                section = Section::Enhanced;
                continue;
            }
            let (key, rest) = trimmed
                .split_once('=')
                .ok_or_else(|| error("expected `key = value`".into()))?;
            let key = key.trim();
            let (value, rest) = value(rest).map_err(error)?;
            if !rest.trim().is_empty() {
                return Err(error(format!(
                    "unexpected `{}` after the value",
                    rest.trim()
                )));
            }
            if seen.iter().any(|seen| seen == key) {
                return Err(error(format!("duplicate key `{key}`")));
            }
            seen.push(key.to_owned());
            let string = |value: Value| match value {
                Value::String(text) => Ok(text),
                _ => Err(error(format!("`{key}` expects a string"))),
            };
            let one_of = |value: Value, allowed: &[&str]| {
                let text = string(value)?;
                if allowed.contains(&text.as_str()) {
                    Ok(text)
                } else {
                    Err(error(format!(
                        "`{key}` is one of {}, not \"{text}\"",
                        allowed
                            .iter()
                            .map(|allowed| format!("\"{allowed}\""))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )))
                }
            };
            // A whole number in `range`.
            let integer = |value: Value, range: std::ops::RangeInclusive<u32>| {
                let expects = format!(
                    "`{key}` is a whole number from {} to {}",
                    range.start(),
                    range.end()
                );
                match value {
                    Value::Number(number)
                        if number.fract() == 0.0
                            && (f64::from(*range.start())..=f64::from(*range.end()))
                                .contains(&number) =>
                    {
                        Ok(number as u32)
                    }
                    Value::Number(number) => Err(error(format!("{expects}, not {number}"))),
                    _ => Err(error(expects)),
                }
            };
            // A file name with `extension`, directly in its folder.
            let file_name = |value: Value, extension: &str| {
                let name = string(value)?;
                let valid = !name.contains(['/', '\\'])
                    && name.to_ascii_lowercase().ends_with(extension)
                    && VfsPath::new(&name).is_ok();
                if valid {
                    Ok(name.to_ascii_lowercase())
                } else {
                    Err(error(format!(
                        "`{key}` is a {extension} file name with no folder, not \"{name}\""
                    )))
                }
            };
            match section {
                Section::Enhanced => {
                    let enhanced = manifest.enhanced.as_mut().expect("the table exists");
                    match key {
                        "preset" => enhanced.preset = Some(one_of(value, &PRESETS)?),
                        "viewport_mode" => {
                            enhanced.viewport_mode = Some(one_of(value, &VIEWPORT_MODES)?);
                        }
                        "default_zoom" => {
                            let expects = "`default_zoom` is a number from 0.5 to 3.0";
                            let Value::Number(zoom) = value else {
                                return Err(error(expects.into()));
                            };
                            let zoom = zoom as f32;
                            if !ZOOM_RANGE.contains(&zoom) {
                                return Err(error(format!("{expects}, not {zoom}")));
                            }
                            enhanced.default_zoom = Some(zoom);
                        }
                        "smooth_camera" => {
                            let Value::Bool(flag) = value else {
                                return Err(error("`smooth_camera` expects true or false".into()));
                            };
                            enhanced.smooth_camera = Some(flag);
                        }
                        "roof_peek" => enhanced.roof_peek = Some(one_of(value, &ROOF_PEEKS)?),
                        _ => return Err(error(format!("unknown key `{key}` in `[enhanced]`"))),
                    }
                }
                Section::Script => {
                    let script = manifest.scripts.last_mut().expect("the entry exists");
                    match key {
                        "file" => script.file = file_name(value, ".int")?,
                        "index" => script.index = integer(value, SCRIPT_INDEXES)?,
                        "local_vars" => script.local_vars = integer(value, 0..=MAX_LOCALS)?,
                        _ => return Err(error(format!("unknown key `{key}` in `[[scripts]]`"))),
                    }
                }
                Section::Map => {
                    let map = manifest.maps.last_mut().expect("the entry exists");
                    match key {
                        "file" => map.file = file_name(value, ".map")?,
                        "number" => map.number = integer(value, MAP_NUMBERS)?,
                        _ => return Err(error(format!("unknown key `{key}` in `[[maps]]`"))),
                    }
                }
                Section::Patch => {
                    let patch = manifest.patches.last_mut().expect("the entry exists");
                    match key {
                        "table" => {
                            let table = string(value)?;
                            if table.is_empty()
                                || !table
                                    .bytes()
                                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
                            {
                                return Err(error(format!(
                                    "`table` is a table name, not \"{table}\""
                                )));
                            }
                            patch.table = table;
                        }
                        "file" => {
                            let file = string(value)?;
                            let path = VfsPath::new(&file).map_err(|reason| {
                                error(format!("`file` is a relative path: {reason}"))
                            })?;
                            patch.file = path.as_str().to_owned();
                        }
                        _ => return Err(error(format!("unknown key `{key}` in `[[patches]]`"))),
                    }
                }
                Section::Top => match key {
                    "name" => {
                        let name = string(value)?;
                        if name.trim().is_empty() {
                            return Err(error("`name` is empty".into()));
                        }
                        manifest.name = name;
                        named = true;
                    }
                    "version" => manifest.version = Some(string(value)?),
                    "author" => manifest.author = Some(string(value)?),
                    "description" => manifest.description = Some(string(value)?),
                    _ => return Err(error(format!("unknown key `{key}`"))),
                },
            }
        }
        Self::finish(section, entry_line, &seen)?;
        if !named {
            return Err(ManifestError {
                line: 0,
                message: "`name` is missing".into(),
            });
        }
        Ok(manifest)
    }
    /// The array entry `section` ended (its keys `seen`): its required
    /// keys must be there.
    fn finish(section: Section, line: usize, seen: &[String]) -> Result<(), ManifestError> {
        let (table, required): (&str, &[&str]) = match section {
            Section::Script => ("scripts", &["file", "index"]),
            Section::Map => ("maps", &["file", "number"]),
            Section::Patch => ("patches", &["table", "file"]),
            Section::Top | Section::Enhanced => return Ok(()),
        };
        match required
            .iter()
            .find(|key| !seen.iter().any(|seen| seen == *key))
        {
            Some(key) => Err(ManifestError {
                line,
                message: format!("`[[{table}]]` needs `{key}`"),
            }),
            None => Ok(()),
        }
    }

    /// The manifest text, which [`Self::parse`] reads back to the same
    /// manifest. A string with a line break or control character, an
    /// empty name, or a setting outside its values cannot be written.
    pub fn to_toml(&self) -> Result<String, ManifestError> {
        let bad = |message: String| ManifestError { line: 0, message };
        let quote = |key: &str, text: &str| {
            if text.chars().any(char::is_control) {
                return Err(bad(format!("`{key}` has a control character")));
            }
            Ok(format!(
                "{key} = \"{}\"\n",
                text.replace('\\', "\\\\").replace('"', "\\\"")
            ))
        };
        if self.name.trim().is_empty() {
            return Err(bad("`name` is empty".into()));
        }
        let mut text = String::from("# A Vault Engine mod layer's manifest (see README.txt).\n");
        text += &quote("name", &self.name)?;
        for (key, value) in [
            ("version", &self.version),
            ("author", &self.author),
            ("description", &self.description),
        ] {
            if let Some(value) = value {
                text += &quote(key, value)?;
            }
        }
        if let Some(enhanced) = &self.enhanced {
            text += "\n[enhanced]\n";
            for (key, value, allowed) in [
                ("preset", &enhanced.preset, &PRESETS[..]),
                ("viewport_mode", &enhanced.viewport_mode, &VIEWPORT_MODES),
                ("roof_peek", &enhanced.roof_peek, &ROOF_PEEKS),
            ] {
                if let Some(value) = value {
                    if !allowed.contains(&value.as_str()) {
                        return Err(bad(format!("`{key}` cannot be \"{value}\"")));
                    }
                    text += &quote(key, value)?;
                }
            }
            if let Some(zoom) = enhanced.default_zoom {
                if !ZOOM_RANGE.contains(&zoom) {
                    return Err(bad(format!("`default_zoom` cannot be {zoom}")));
                }
                text += &format!("default_zoom = {zoom:?}\n");
            }
            if let Some(smooth) = enhanced.smooth_camera {
                text += &format!("smooth_camera = {smooth}\n");
            }
        }
        for script in &self.scripts {
            text += "\n[[scripts]]\n";
            text += &quote("file", &script.file)?;
            text += &format!(
                "index = {}\nlocal_vars = {}\n",
                script.index, script.local_vars
            );
        }
        for map in &self.maps {
            text += "\n[[maps]]\n";
            text += &quote("file", &map.file)?;
            text += &format!("number = {}\n", map.number);
        }
        for patch in &self.patches {
            text += "\n[[patches]]\n";
            text += &quote("table", &patch.table)?;
            text += &quote("file", &patch.file)?;
        }
        // What is written must read back.
        Self::parse(&text).map_err(|error| bad(error.message))?;
        Ok(text)
    }

    /// The folder's `README.txt`: what the mod is, what it holds, and how
    /// to load it.
    pub fn readme(&self, files: &[String]) -> String {
        let mut text = format!("{}\n", self.name);
        text += &"=".repeat(self.name.chars().count().max(3));
        text += "\n\n";
        if let Some(version) = &self.version {
            text += &format!("Version: {version}\n");
        }
        if let Some(author) = &self.author {
            text += &format!("Author: {author}\n");
        }
        if let Some(description) = &self.description {
            text += &format!("\n{description}\n");
        }
        text += "\nA Vault Engine mod layer: a folder the engine mounts above the game data.\n";
        if let Some(enhanced) = &self.enhanced {
            text += "\nOn launch, while the layer is enabled, its vault-mod.toml applies these\n\
                     [enhanced] settings (command-line flags still win):\n";
            for (key, value) in [
                ("preset", enhanced.preset.clone()),
                ("viewport_mode", enhanced.viewport_mode.clone()),
                (
                    "default_zoom",
                    enhanced.default_zoom.map(|zoom| format!("{zoom:?}")),
                ),
                (
                    "smooth_camera",
                    enhanced.smooth_camera.map(|flag| flag.to_string()),
                ),
                ("roof_peek", enhanced.roof_peek.clone()),
            ] {
                if let Some(value) = value {
                    text += &format!("  {key} = {value}\n");
                }
            }
        }
        if files.is_empty() {
            text += "\nIt carries no game files: only the settings above.\n";
        } else {
            text += "\nFiles it adds or replaces:\n";
            for file in files {
                text += &format!("  {file}\n");
            }
        }
        if !self.scripts.is_empty() || !self.maps.is_empty() || !self.patches.is_empty() {
            text += "\nIt registers, without replacing scripts.lst or whole tables:\n";
            for script in &self.scripts {
                text += &format!("  script row {}: scripts/{}\n", script.index, script.file);
            }
            for map in &self.maps {
                text += &format!("  map {}: maps/{}\n", map.number, map.file);
            }
            for patch in &self.patches {
                text += &format!("  {} table patch: {}\n", patch.table, patch.file);
            }
        }
        let registered = |file: &String| {
            self.scripts
                .iter()
                .any(|script| *file == format!("scripts/{}", script.file))
        };
        if files
            .iter()
            .any(|file| file.starts_with("scripts/") && !registered(file))
        {
            text += "\nIts unregistered scripts use the enhanced opcode range, so they run with\n\
                     the enhanced preset only, when a map or scripts.lst row names them; this\n\
                     folder does not replace scripts.lst.\n";
        }
        text += "\nTo load it:\n\
                 1. Put this folder beside your load_order.toml (or vault-engine.toml),\n\
                 \x20  for example in mods/.\n\
                 2. Start the engine with --launcher and open the Mods tab.\n\
                 3. Type the folder's path (relative to the load order, such as\n\
                 \x20  mods/<this folder>), press Add, then Save.\n\
                 4. Press Play. The list shows the mod's name, and [enhanced] when it\n\
                 \x20  carries launch settings. Disable or remove the layer to stop using it.\n\
                 \nOr add it to load_order.toml by hand:\n\
                 \n  [[layer]]\n  path = \"mods/<this folder>\"\n";
        text
    }
}

/// The additions the enabled mod layers register, merged in load order
/// (scripts by row, maps by number, and every table patch in order).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModRegistry {
    pub scripts: BTreeMap<u32, ScriptRegistration>,
    pub maps: BTreeMap<u32, MapRegistration>,
    /// Lowest layer first: a later patch wins a record.
    pub patches: Vec<PatchRegistration>,
}

impl ModRegistry {
    /// The registrations of `mods` (lowest layer first). Two mods
    /// registering one script row, one map number or one map file name is
    /// an error naming both.
    pub fn build<'a>(mods: impl IntoIterator<Item = &'a ModManifest>) -> Result<Self, String> {
        let mut registry = Self::default();
        let mut script_owner: BTreeMap<u32, &str> = BTreeMap::new();
        let mut map_owner: BTreeMap<u32, &str> = BTreeMap::new();
        let mut map_files: BTreeMap<&str, &str> = BTreeMap::new();
        for manifest in mods {
            let name = manifest.name.as_str();
            for script in &manifest.scripts {
                if let Some(other) = script_owner.insert(script.index, name) {
                    return Err(format!(
                        "script index {} is registered by both \"{other}\" and \"{name}\"",
                        script.index
                    ));
                }
                registry.scripts.insert(script.index, script.clone());
            }
            for map in &manifest.maps {
                if let Some(other) = map_owner.insert(map.number, name) {
                    return Err(format!(
                        "map number {} is registered by both \"{other}\" and \"{name}\"",
                        map.number
                    ));
                }
                if let Some(other) = map_files.insert(map.file.as_str(), name) {
                    return Err(format!(
                        "map {} is registered by both \"{other}\" and \"{name}\"",
                        map.file
                    ));
                }
                registry.maps.insert(map.number, map.clone());
            }
            registry.patches.extend(manifest.patches.iter().cloned());
        }
        Ok(registry)
    }
    /// The registrations of the manifests `vfs`'s mod layers carry
    /// ([`Vfs::layer_manifests`]). A malformed manifest or a conflict is an
    /// error naming it.
    pub fn from_vfs(vfs: &dyn Vfs) -> Result<Self, String> {
        let manifests = vfs
            .layer_manifests()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|(layer, bytes)| {
                let text = String::from_utf8(bytes)
                    .map_err(|_| format!("{layer}: {MANIFEST} is not UTF-8 text"))?;
                ModManifest::parse(&text).map_err(|error| format!("{layer}: {error}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::build(&manifests)
    }
    /// The bytes of a layer's manifest, bounded.
    pub(crate) fn read_layer_manifest(vfs: &dyn Vfs) -> Result<Option<Vec<u8>>, VfsError> {
        match vfs.read_bounded(MANIFEST, MANIFEST_BYTES) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(VfsError::NotFound(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.scripts.is_empty() && self.maps.is_empty() && self.patches.is_empty()
    }
    /// The registered map a `load_map` name names: the name uppercased with
    /// its first `.SAV` replaced by `.MAP` (SPEC-008 §3.10's normalization),
    /// compared with the registered file names uppercased.
    pub fn map_number(&self, name: &[u8]) -> Option<u32> {
        let name = std::str::from_utf8(name).ok()?.to_ascii_uppercase();
        let name = name.replacen(".SAV", ".MAP", 1);
        self.maps
            .values()
            .find(|map| map.file.to_ascii_uppercase() == name)
            .map(|map| map.number)
    }
}

/// The text of a `[[patches]]` file applied to a table's `text` (`#`
/// comment lines, then one `|`-separated record per line): each
/// `<record index>|<fields>` line replaces that record with `<fields>`.
/// Blank and `#` lines are skipped; an index past the records, or a line
/// without one, is an error naming the line.
pub fn apply_patch(text: &str, patch: &str) -> Result<String, String> {
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let records: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| {
            let line = line.trim();
            !line.is_empty() && !line.starts_with('#')
        })
        .map(|(index, _)| index)
        .collect();
    for (number, line) in patch.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let at = |message: String| format!("patch line {}: {message}", number + 1);
        let (index, fields) = trimmed
            .split_once('|')
            .ok_or_else(|| at("expected `<record index>|<fields>`".into()))?;
        let index: usize = index
            .trim()
            .parse()
            .map_err(|_| at(format!("`{}` is not a record index", index.trim())))?;
        let &line_index = records.get(index).ok_or_else(|| {
            at(format!(
                "record {index} is past the table's {} records",
                records.len()
            ))
        })?;
        lines[line_index] = fields.trim().to_owned();
    }
    let mut patched = lines.join("\n");
    if text.ends_with('\n') {
        patched.push('\n');
    }
    Ok(patched)
}

/// Write a mod folder at `output`: `vault-mod.toml`, each of `files` (a
/// VFS path, which must be relative and canonical-safe, and its bytes) and
/// `README.txt`. `output` must not exist or be an empty directory: an
/// export never overwrites.
pub fn write_mod_folder(
    output: &Path,
    manifest: &ModManifest,
    files: &[(String, Vec<u8>)],
) -> io::Result<()> {
    let invalid = |message: String| io::Error::new(io::ErrorKind::InvalidInput, message);
    if output.exists() && output.read_dir()?.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} is not empty", output.display()),
        ));
    }
    let toml = manifest
        .to_toml()
        .map_err(|error| invalid(error.to_string()))?;
    let mut paths = Vec::with_capacity(files.len());
    for (path, _) in files {
        let path = VfsPath::new(path).map_err(|error| invalid(error.to_string()))?;
        if [MANIFEST, README].contains(&path.as_str()) {
            return Err(invalid(format!("{path} is the folder's own file")));
        }
        paths.push(path);
    }
    std::fs::create_dir_all(output)?;
    std::fs::write(output.join(MANIFEST), toml)?;
    for (path, (_, bytes)) in paths.iter().zip(files) {
        let target = path
            .as_str()
            .split('/')
            .fold(output.to_path_buf(), |target, part| target.join(part));
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(target, bytes)?;
    }
    let listed: Vec<String> = paths.iter().map(|path| path.as_str().to_owned()).collect();
    std::fs::write(output.join(README), manifest.readme(&listed))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full() -> ModManifest {
        ModManifest {
            name: "Tactical \"camera\"".into(),
            version: Some("1.2.0".into()),
            author: Some("someone".into()),
            description: Some("Zoom and peek".into()),
            enhanced: Some(EnhancedOverrides {
                preset: Some("enhanced".into()),
                viewport_mode: Some("unlocked_widescreen".into()),
                default_zoom: Some(1.25),
                smooth_camera: Some(true),
                roof_peek: Some("cursor_xray".into()),
            }),
            ..ModManifest::default()
        }
    }

    #[test]
    fn manifests_parse_and_write_back() {
        let text = "\u{feff}# a mod\nname = 'Camera' # trailing\nversion = \"1.0\"\n\n\
                    [ enhanced ]\npreset = \"enhanced\"\ndefault_zoom = 2\n\
                    smooth_camera = false\nroof_peek = \"hidden\"\n";
        let manifest = ModManifest::parse(text).unwrap();
        assert_eq!(manifest.name, "Camera");
        assert_eq!(manifest.version.as_deref(), Some("1.0"));
        let enhanced = manifest.enhanced.clone().unwrap();
        assert_eq!(enhanced.default_zoom, Some(2.0));
        assert_eq!(enhanced.smooth_camera, Some(false));
        assert_eq!(enhanced.viewport_mode, None);
        for manifest in [
            manifest,
            full(),
            ModManifest {
                name: "plain".into(),
                ..Default::default()
            },
        ] {
            let text = manifest.to_toml().unwrap();
            assert_eq!(ModManifest::parse(&text).unwrap(), manifest, "{text}");
        }
        // A manifest without `[enhanced]` carries no settings.
        let plain = ModManifest::parse("name = \"Maps only\"").unwrap();
        assert_eq!(plain.enhanced, None);
    }

    #[test]
    fn malformed_manifests_name_the_line() {
        for (text, line, needle) in [
            ("version = \"1\"", 0, "`name` is missing"),
            ("name = \"\"", 1, "empty"),
            ("name = \"a\"\n[camera]", 2, "unknown table"),
            ("name = \"a\"\nnmae = \"b\"", 2, "unknown key `nmae`"),
            (
                "name = \"a\"\n[enhanced]\nzoom = 2",
                3,
                "unknown key `zoom`",
            ),
            (
                "name = \"a\"\n[enhanced]\ndefault_zoom = 4",
                3,
                "0.5 to 3.0",
            ),
            (
                "name = \"a\"\n[enhanced]\nroof_peek = \"xray\"",
                3,
                "\"cursor_xray\"",
            ),
            (
                "name = \"a\"\n[enhanced]\nsmooth_camera = \"yes\"",
                3,
                "true or false",
            ),
            (
                "name = \"a\"\n[enhanced]\npreset = \"modern\"",
                3,
                "\"enhanced\"",
            ),
            ("name = \"a\"\nname = \"b\"", 2, "duplicate key"),
            ("name = \"a\"\n[enhanced]\n[enhanced]", 3, "duplicate table"),
            ("name = \"a\" extra", 1, "unexpected"),
            ("name = \"a", 1, "unterminated"),
            ("name", 1, "key = value"),
        ] {
            let error = ModManifest::parse(text).unwrap_err();
            assert_eq!(error.line, line, "{text}: {error}");
            assert!(error.message.contains(needle), "{text}: {error}");
        }
        // The same key in both places is not a duplicate.
        ModManifest::parse("name = \"a\"\n[enhanced]\npreset = \"vanilla\"").unwrap();
        let mut bad = full();
        bad.description = Some("two\nlines".into());
        assert!(bad.to_toml().is_err());
    }

    #[test]
    fn registrations_parse_write_back_and_name_missing_keys() {
        let text = "name = \"Demo\"\n[enhanced]\npreset = \"enhanced\"\n\
                    [[scripts]]\nfile = \"CamDemo.int\"\nindex = 4096\n\
                    [[maps]]\nfile = \"cam_demo.map\"\nnumber = 100\n\
                    [[scripts]] # a second\nfile = \"other.int\"\nindex = 4097\nlocal_vars = 3\n\
                    [[patches]]\ntable = \"towns\"\nfile = \"data/demo/towns.patch.txt\"\n";
        let manifest = ModManifest::parse(text).unwrap();
        assert_eq!(
            manifest.scripts,
            [
                ScriptRegistration {
                    file: "camdemo.int".into(),
                    index: 4096,
                    local_vars: 0
                },
                ScriptRegistration {
                    file: "other.int".into(),
                    index: 4097,
                    local_vars: 3
                },
            ]
        );
        assert_eq!(manifest.maps[0].number, 100);
        assert_eq!(manifest.patches[0].table, "towns");
        let written = manifest.to_toml().unwrap();
        assert_eq!(ModManifest::parse(&written).unwrap(), manifest, "{written}");
        for (text, line, needle) in [
            (
                "name = \"a\"\n[[scripts]]\nfile = \"a.int\"",
                2,
                "needs `index`",
            ),
            (
                "name = \"a\"\n[[maps]]\nnumber = 70\n[enhanced]",
                2,
                "needs `file`",
            ),
            (
                "name = \"a\"\n[[maps]]\nfile = \"a.map\"\nnumber = 65",
                4,
                "66 to 65535",
            ),
            (
                "name = \"a\"\n[[maps]]\nfile = \"maps/a.map\"",
                3,
                "no folder",
            ),
            (
                "name = \"a\"\n[[scripts]]\nfile = \"a.ssl\"",
                3,
                ".int file",
            ),
            ("name = \"a\"\n[[scripts]]\nindex = 1.5", 3, "whole number"),
            (
                "name = \"a\"\n[[patches]]\ntable = \"Towns\"",
                3,
                "table name",
            ),
            (
                "name = \"a\"\n[[patches]]\nfile = \"../x\"",
                3,
                "relative path",
            ),
            ("name = \"a\"\n[[mods]]", 2, "unknown table"),
            (
                "name = \"a\"\n[[scripts]]\nfile = \"a.int\"\nfile = \"b.int\"",
                4,
                "duplicate",
            ),
        ] {
            let error = ModManifest::parse(text).unwrap_err();
            assert_eq!(error.line, line, "{text}: {error}");
            assert!(error.message.contains(needle), "{text}: {error}");
        }
        // Each entry has its own keys: two entries may both name `file`.
        ModManifest::parse(
            "name = \"a\"\n[[maps]]\nfile = \"a.map\"\nnumber = 70\n\
             [[maps]]\nfile = \"b.map\"\nnumber = 71",
        )
        .unwrap();
    }

    #[test]
    fn registries_merge_in_load_order_and_refuse_conflicts() {
        let manifest = |name: &str, text: &str| {
            ModManifest::parse(&format!("name = \"{name}\"\n{text}")).unwrap()
        };
        let one = manifest(
            "One",
            "[[scripts]]\nfile = \"a.int\"\nindex = 5000\n[[maps]]\nfile = \"a.map\"\nnumber = 70\n\
             [[patches]]\ntable = \"towns\"\nfile = \"one/towns.txt\"",
        );
        let two = manifest(
            "Two",
            "[[scripts]]\nfile = \"b.int\"\nindex = 5001\n[[patches]]\ntable = \"towns\"\nfile = \"two/towns.txt\"",
        );
        let registry = ModRegistry::build([&one, &two]).unwrap();
        assert_eq!(
            registry.scripts.keys().copied().collect::<Vec<_>>(),
            [5000, 5001]
        );
        assert_eq!(registry.patches[1].file, "two/towns.txt");
        assert_eq!(registry.map_number(b"a.map"), Some(70));
        assert_eq!(registry.map_number(b"A.SAV"), Some(70));
        assert_eq!(registry.map_number(b"maps/a.map"), None);
        for (text, needle) in [
            (
                "[[scripts]]\nfile = \"c.int\"\nindex = 5000",
                "script index 5000",
            ),
            ("[[maps]]\nfile = \"c.map\"\nnumber = 70", "map number 70"),
            ("[[maps]]\nfile = \"a.map\"\nnumber = 71", "map a.map"),
        ] {
            let three = manifest("Three", text);
            let error = ModRegistry::build([&one, &three]).unwrap_err();
            assert!(
                error.contains(needle) && error.contains("\"One\" and \"Three\""),
                "{error}"
            );
        }
    }

    #[test]
    fn patches_replace_records_by_index() {
        let table = "# header\n0|a|b\n\n  # note\n1|c|d\n2|e|f\n";
        let patched = apply_patch(table, "# mine\n2|2|x|y\n\n0 | 0|z|z\n").unwrap();
        assert_eq!(patched, "# header\n0|z|z\n\n  # note\n1|c|d\n2|x|y\n");
        assert!(
            apply_patch(table, "3|x")
                .unwrap_err()
                .contains("past the table's 3")
        );
        assert!(apply_patch(table, "x|y").unwrap_err().contains("line 1"));
        assert!(apply_patch(table, "nothing").is_err());
    }

    #[test]
    fn layered_vfs_reads_each_layers_own_manifest() {
        let dir = tempfile::tempdir().unwrap();
        for (layer, name) in [("low", "Low"), ("high", "High")] {
            std::fs::create_dir_all(dir.path().join(layer)).unwrap();
            std::fs::write(
                dir.path().join(layer).join(MANIFEST),
                format!(
                    "name = \"{name}\"\n[[maps]]\nfile = \"{layer}.map\"\nnumber = {}",
                    if layer == "low" { 70 } else { 71 }
                ),
            )
            .unwrap();
        }
        std::fs::create_dir_all(dir.path().join("plain")).unwrap();
        std::fs::create_dir_all(dir.path().join("base")).unwrap();
        let base: std::sync::Arc<dyn Vfs> =
            std::sync::Arc::new(crate::LocalDirProvider::mount(dir.path().join("base")).unwrap());
        let entry = |path: &str| crate::LayerEntry {
            path: Some(path.into()),
            ..Default::default()
        };
        let vfs = crate::LayeredVfs::from_entries(
            base,
            dir.path(),
            &[entry("low"), entry("plain"), entry("high")],
            crate::LoadOrderPolicy::default(),
        )
        .unwrap();
        let names: Vec<String> = vfs
            .layer_manifests()
            .unwrap()
            .into_iter()
            .map(|(layer, _)| layer)
            .collect();
        assert_eq!(names, ["low", "high"]);
        let registry = ModRegistry::from_vfs(&vfs).unwrap();
        assert_eq!(registry.maps.keys().copied().collect::<Vec<_>>(), [70, 71]);
        // A plain provider has none.
        let plain = crate::LocalDirProvider::mount(dir.path().join("low")).unwrap();
        assert!(plain.layer_manifests().unwrap().is_empty());
        assert!(ModRegistry::from_vfs(&plain).unwrap().is_empty());
    }

    #[test]
    fn mod_folders_hold_the_manifest_files_and_readme() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("mods/camera");
        let files = vec![
            ("data/vault-engine/towns.txt".to_owned(), b"towns".to_vec()),
            ("Scripts\\CamDemo.int".to_owned(), vec![1, 2, 3]),
        ];
        write_mod_folder(&output, &full(), &files).unwrap();
        let manifest = std::fs::read_to_string(output.join(MANIFEST)).unwrap();
        assert_eq!(ModManifest::parse(&manifest).unwrap(), full());
        assert_eq!(
            std::fs::read(output.join("data/vault-engine/towns.txt")).unwrap(),
            b"towns"
        );
        assert_eq!(
            std::fs::read(output.join("scripts/camdemo.int")).unwrap(),
            [1, 2, 3]
        );
        let readme = std::fs::read_to_string(output.join(README)).unwrap();
        for needle in [
            "Mods tab",
            "roof_peek = cursor_xray",
            "scripts/camdemo.int",
            "scripts.lst",
        ] {
            assert!(readme.contains(needle), "{needle}: {readme}");
        }
        // Never over an existing export, nor outside the folder.
        let error = write_mod_folder(&output, &full(), &[]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        for path in ["../escape.txt", "/etc/passwd", "vault-mod.toml"] {
            let other = dir.path().join("other");
            let error = write_mod_folder(&other, &full(), &[(path.into(), vec![])]).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{path}");
            assert!(!other.exists(), "{path}");
        }
    }
}
