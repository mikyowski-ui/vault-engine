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
//! The file is the same TOML subset as `load_order.toml`: strings, `true`
//! and `false`, numbers, `#` comments and the one table. Unknown keys and
//! tables are errors (a misspelt setting would otherwise do nothing).
use crate::vfs::{VfsPath, strip_comment, toml_string};
use std::{fmt, io, path::Path};

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
        let mut seen: Vec<(bool, String)> = Vec::new();
        let mut in_enhanced = false;
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
                in_enhanced = true;
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
            if seen.contains(&(in_enhanced, key.to_owned())) {
                return Err(error(format!("duplicate key `{key}`")));
            }
            seen.push((in_enhanced, key.to_owned()));
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
            if let Some(enhanced) = manifest.enhanced.as_mut().filter(|_| in_enhanced) {
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
                continue;
            }
            match key {
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
            }
        }
        if !named {
            return Err(ManifestError {
                line: 0,
                message: "`name` is missing".into(),
            });
        }
        Ok(manifest)
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
        if files.iter().any(|file| file.starts_with("scripts/")) {
            text += "\nThe scripts use the enhanced opcode range, so they run with the enhanced\n\
                     preset only. A script runs when a map or scripts.lst row names it; this\n\
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
