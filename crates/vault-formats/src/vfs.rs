use std::{collections::BTreeSet, fmt, io, path::PathBuf, sync::Arc};

#[cfg(feature = "host-fs")]
mod host;
mod memory;

#[cfg(feature = "host-fs")]
pub use host::{LocalDirProvider, PrefixMount};
pub use memory::{
    ByteSource, DropError, DropIgnored, DropReport, FALLOUT1_MEMBERS, MemoryDir, MemoryMount,
    MemoryProvider, classify_drop,
};

/// A canonical, relative asset path: lowercase ASCII with `/` separators.
///
/// Both slash styles are accepted on input. Empty components, `.`/`..`,
/// absolute/drive/UNC paths, control characters, Windows special characters,
/// device names, trailing dots/spaces, and paths over [`MAX_PATH_BYTES`] are
/// rejected rather than repaired. Restricting names to ASCII gives identical
/// case folding on every platform.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VfsPath(String);

/// SPEC-023 §2.2 rule 1: the longest path the validator accepts, in bytes.
pub const MAX_PATH_BYTES: usize = 1024;

impl VfsPath {
    pub fn new(path: &str) -> Result<Self, VfsError> {
        let invalid = |reason| VfsError::InvalidPath {
            path: path.to_owned(),
            reason,
        };
        if path.is_empty() || !path.is_ascii() {
            return Err(invalid("expected a nonempty ASCII path"));
        }
        if path.len() > MAX_PATH_BYTES {
            return Err(invalid("longer than 1024 bytes"));
        }
        if path
            .bytes()
            .any(|byte| byte.is_ascii_control() || b"<>:\"|?*".contains(&byte))
        {
            return Err(invalid("control or reserved character"));
        }
        let normalized = path.replace('\\', "/").to_ascii_lowercase();
        for component in normalized.split('/') {
            if component.is_empty() || component == "." || component == ".." {
                return Err(invalid("empty, absolute, or traversal component"));
            }
            if component.ends_with(['.', ' ']) {
                return Err(invalid("component ends in a dot or space"));
            }
            let stem = component.split('.').next().expect("nonempty component");
            if matches!(stem, "con" | "prn" | "aux" | "nul")
                || ((stem.starts_with("com") || stem.starts_with("lpt"))
                    && stem.len() == 4
                    && matches!(stem.as_bytes()[3], b'1'..=b'9'))
            {
                return Err(invalid("reserved device name"));
            }
        }
        Ok(Self(normalized))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Lowercase extension without a leading dot.
    pub fn extension(&self) -> Option<&str> {
        self.0
            .rsplit('/')
            .next()?
            .rsplit_once('.')
            .map(|(_, ext)| ext)
    }
}

impl fmt::Display for VfsPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum VfsError {
    #[error("invalid VFS path {path:?}: {reason}")]
    InvalidPath { path: String, reason: &'static str },
    #[error("asset not found: {0}")]
    NotFound(VfsPath),
    #[error("case-insensitive path collision at {path}: {first:?} and {second:?}")]
    CaseCollision {
        path: VfsPath,
        first: PathBuf,
        second: PathBuf,
    },
    #[error("symlinks are not allowed inside a loose mount: {0:?}")]
    Symlink(PathBuf),
    #[error("unsupported filesystem entry: {0:?}")]
    UnsupportedEntry(PathBuf),
    #[error("non-UTF-8 filename: {0:?}")]
    NonUtf8Name(PathBuf),
    #[error("asset {path:?} exceeds the {max_bytes}-byte read limit")]
    FileTooLarge { path: PathBuf, max_bytes: usize },
    #[error("archive error: {0}")]
    Archive(String),
    #[error("{file:?} is a file and also the folder of {inside:?}")]
    FileIsFolder { file: String, inside: String },
    #[error("load order: {0}")]
    LoadOrder(String),
    #[error("filesystem error at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Read-only, object-safe asset access shared by loose and future archive providers.
pub trait Vfs: Send + Sync {
    /// Read a file using the same strict normalization as `VfsPath::new`.
    fn read(&self, path: &str) -> Result<Vec<u8>, VfsError>;

    /// Read with a size limit. Providers that can check sizes before reading
    /// override this; the default bounds the returned length.
    fn read_bounded(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, VfsError> {
        let bytes = self.read(path)?;
        if bytes.len() > max_bytes {
            return Err(VfsError::FileTooLarge {
                path: PathBuf::from(path),
                max_bytes,
            });
        }
        Ok(bytes)
    }

    /// [`Vfs::read_bounded`], with the name of the mod layer
    /// ([`LayeredVfs`]) that served the file: `None` when the base data
    /// served it, and always for a plain provider. One read, so the name is
    /// the bytes' own layer.
    fn read_with_layer(
        &self,
        path: &str,
        max_bytes: usize,
    ) -> Result<(Vec<u8>, Option<String>), VfsError> {
        self.read_bounded(path, max_bytes)
            .map(|bytes| (bytes, None))
    }

    /// Walk all mounted files in canonical lexical order (directories excluded).
    fn files(&self) -> Box<dyn Iterator<Item = &VfsPath> + '_>;

    /// How many mod layers ([`LayeredVfs`]) sit above the base data; 0 for
    /// a plain provider.
    fn mod_layers(&self) -> usize {
        0
    }

    /// Each mod layer's own `vault-mod.toml`
    /// ([`crate::mod_manifest::MANIFEST`]), lowest layer first, as (layer
    /// name, bytes). A layer above another hides its file from
    /// [`Vfs::read`], so the manifests are read layer by layer here. None
    /// for a plain provider.
    fn layer_manifests(&self) -> Result<Vec<(String, Vec<u8>)>, VfsError> {
        Ok(Vec::new())
    }
}

impl fmt::Debug for dyn Vfs {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Vfs({} files)", self.files().count())
    }
}

/// Which part of a mount served a read (SPEC-023 §3.2 rule 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServedBy {
    Loose,
    Patch,
    Master,
    Critter,
    /// A mod layer, by the name its load order gave it.
    Layer(String),
}

impl ServedBy {
    pub fn label(&self) -> &str {
        match self {
            Self::Loose => "loose",
            Self::Patch => "patch",
            Self::Master => "master",
            Self::Critter => "critter",
            Self::Layer(name) => name,
        }
    }
}

/// One mod layer above the base data: a loose folder and/or a master
/// `.DAT` and an optional critter `.DAT`. Within the layer the SPEC-008
/// §3.2 rules apply as they do to the base: the loose folder is consulted
/// first, and critter art (`art/critters/`) selects the layer's critter
/// archive while every other path selects its master archive, with no
/// cross-archive fallback inside the layer.
pub struct ModLayer {
    name: String,
    provider: crate::DatProvider,
}

impl ModLayer {
    /// A layer from already-mounted parts, its loose root any provider.
    pub fn from_parts(
        name: impl Into<String>,
        loose: Option<Arc<dyn Vfs>>,
        master: Option<crate::DatArchive>,
        critter: Option<crate::DatArchive>,
    ) -> Self {
        let provider = crate::DatProvider::new(
            crate::Database {
                loose: loose.clone(),
                archive: master.map(Arc::new),
            },
            crate::Database {
                loose,
                archive: critter.map(Arc::new),
            },
        );
        Self {
            name: name.into(),
            provider,
        }
    }

    /// SPEC-023 §3.4: a layer that is one memory tree.
    pub fn from_tree(name: impl Into<String>, tree: MemoryDir) -> Self {
        Self::from_parts(name, Some(Arc::new(tree)), None, None)
    }

    /// The layer's name as its load order wrote it.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Debug for ModLayer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ModLayer")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// One `load_order.toml` layer entry, with paths as written (relative to
/// the load order's own directory).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerEntry {
    /// A loose folder, or a `.dat` file used as the layer's master archive.
    pub path: Option<String>,
    /// The layer's master archive, beside a loose `path` folder.
    pub master_dat: Option<String>,
    /// The layer's critter archive (`art/critters/` requests).
    pub critter_dat: Option<String>,
    /// `enabled = false` keeps the layer listed but unmounted. On by default.
    pub enabled: bool,
}

impl Default for LayerEntry {
    fn default() -> Self {
        Self {
            path: None,
            master_dat: None,
            critter_dat: None,
            enabled: true,
        }
    }
}

/// How a load order resolves its layer paths against its folder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LoadOrderPolicy {
    /// Accept absolute paths and `..` components that leave the load
    /// order's directory. Off by default: every layer must lie inside it.
    pub allow_outside: bool,
}

pub(crate) fn load_order_error(message: impl Into<String>) -> VfsError {
    VfsError::LoadOrder(message.into())
}

/// Strip a `#` comment that is not inside a string.
pub(crate) fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        match quote {
            Some('"') if escaped => escaped = false,
            Some('"') if ch == '\\' => escaped = true,
            Some(open) if ch == open => quote = None,
            Some(_) => {}
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch == '#' => return &line[..index],
            None => {}
        }
    }
    line
}

/// Parse one TOML string (basic `"..."` with `\\` `\"` escapes, or
/// literal `'...'`) at the start of `text`; returns it and the rest.
pub(crate) fn toml_string(text: &str) -> Result<(String, &str), VfsError> {
    let text = text.trim_start();
    let mut chars = text.char_indices();
    let open = match chars.next() {
        Some((_, ch @ ('"' | '\''))) => ch,
        _ => return Err(load_order_error(format!("expected a string at {text:?}"))),
    };
    let mut value = String::new();
    let mut escaped = false;
    for (index, ch) in chars {
        if open == '"' && escaped {
            match ch {
                '\\' | '"' => value.push(ch),
                _ => return Err(load_order_error(format!("unsupported escape \\{ch}"))),
            }
            escaped = false;
        } else if open == '"' && ch == '\\' {
            escaped = true;
        } else if ch == open {
            return Ok((value, &text[index + 1..]));
        } else {
            value.push(ch);
        }
    }
    Err(load_order_error("unterminated string"))
}

/// Parse the `load_order.toml` subset: either `layers = ["path", ...]`
/// (one array, which may span lines) or one `[[layer]]` table per layer
/// with `path = "..."`, the optional `master_dat` / `critter_dat` keys and
/// an optional `enabled = true | false` (a disabled layer stays listed but
/// is not mounted). `#` starts a comment. Entries are listed lowest first:
/// the last layer wins. Unknown keys, sections, or a mix of both forms are
/// errors.
pub fn parse_load_order(text: &str) -> Result<Vec<LayerEntry>, VfsError> {
    let mut entries: Vec<LayerEntry> = Vec::new();
    let mut array_form = false;
    let mut in_table = false;
    // The layer number whose `enabled` key was last seen.
    let mut enabled_seen = None;
    let mut lines = text.lines().enumerate();
    while let Some((number, line)) = lines.next() {
        let at = |message: String| load_order_error(format!("line {}: {message}", number + 1));
        let line = strip_comment(line).trim();
        if line.is_empty() {
            continue;
        }
        if line == "[[layer]]" {
            if array_form {
                return Err(at("`[[layer]]` after a `layers` array".into()));
            }
            in_table = true;
            entries.push(LayerEntry::default());
            continue;
        }
        if line.starts_with('[') {
            return Err(at(format!("unsupported section {line:?}")));
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| at(format!("expected `key = value`, found {line:?}")))?;
        let (key, value) = (key.trim(), value.trim());
        if key == "layers" && !in_table {
            if array_form {
                return Err(at("a second `layers` array".into()));
            }
            array_form = true;
            // Gather the array, which may continue over later lines.
            let mut array = value.to_string();
            while !strip_comment(&array).contains(']') {
                let Some((_, next)) = lines.next() else {
                    return Err(at("unterminated `layers` array".into()));
                };
                array.push(' ');
                array.push_str(strip_comment(next));
            }
            let mut rest = strip_comment(&array)
                .trim()
                .strip_prefix('[')
                .ok_or_else(|| at("`layers` must be an array".into()))?
                .trim_start();
            loop {
                if let Some(after) = rest.strip_prefix(']') {
                    if !after.trim().is_empty() {
                        return Err(at(format!("text after the array: {after:?}")));
                    }
                    break;
                }
                let (path, after) = toml_string(rest)?;
                entries.push(LayerEntry {
                    path: Some(path),
                    ..LayerEntry::default()
                });
                let after = after.trim_start();
                rest = match after.strip_prefix(',') {
                    Some(after) => after.trim_start(),
                    None if after.starts_with(']') => after,
                    None => return Err(at(format!("expected `,` or `]` at {after:?}"))),
                };
            }
            continue;
        }
        let layer = entries.len();
        let Some(entry) = entries.last_mut().filter(|_| in_table) else {
            return Err(at(format!("unknown key {key:?}")));
        };
        if key == "enabled" {
            if enabled_seen.replace(layer) == Some(layer) {
                return Err(at(format!("duplicate key {key:?}")));
            }
            entry.enabled = match value {
                "true" => true,
                "false" => false,
                _ => return Err(at(format!("`enabled` is true or false, not {value:?}"))),
            };
            continue;
        }
        let (text, rest) = toml_string(value)?;
        if !rest.trim().is_empty() {
            return Err(at(format!("text after the value: {rest:?}")));
        }
        let slot = match key {
            "path" => &mut entry.path,
            "master_dat" => &mut entry.master_dat,
            "critter_dat" => &mut entry.critter_dat,
            _ => return Err(at(format!("unknown layer key {key:?}"))),
        };
        if slot.replace(text).is_some() {
            return Err(at(format!("duplicate key {key:?}")));
        }
    }
    for (index, entry) in entries.iter().enumerate() {
        if entry.path.is_none() && entry.master_dat.is_none() && entry.critter_dat.is_none() {
            return Err(load_order_error(format!(
                "layer {} names nothing",
                index + 1
            )));
        }
    }
    Ok(entries)
}

/// One basic TOML string with only the escapes [`parse_load_order`] reads.
fn toml_quote(text: &str) -> Result<String, VfsError> {
    if text.chars().any(char::is_control) {
        return Err(load_order_error(format!(
            "layer path {text:?} holds a control character"
        )));
    }
    Ok(format!(
        "\"{}\"",
        text.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

/// Write layer entries (lowest first) as a `load_order.toml` in the
/// `[[layer]]` form, with the paths exactly as given (relative paths stay
/// relative) and `enabled = false` for a disabled layer;
/// [`parse_load_order`] reads the text back to the same entries. A path
/// holding a control character cannot be written.
pub fn write_load_order(entries: &[LayerEntry]) -> Result<String, VfsError> {
    let mut out = String::from("# Mod layers, lowest first: the last layer wins.\n");
    for entry in entries {
        out.push_str("\n[[layer]]\n");
        for (key, value) in [
            ("path", &entry.path),
            ("master_dat", &entry.master_dat),
            ("critter_dat", &entry.critter_dat),
        ] {
            if let Some(value) = value {
                out.push_str(&format!("{key} = {}\n", toml_quote(value)?));
            }
        }
        if !entry.enabled {
            out.push_str("enabled = false\n");
        }
    }
    Ok(out)
}

/// N ordered mod layers above a base provider. A read tries the layers
/// from the highest down, then the base: the first layer holding the file
/// wins. Only an absent file falls through to the next layer; a
/// collision, link, oversize, or archive failure in a layer is reported,
/// not masked by a lower copy (the SPEC-008 §3.2 loose-root rule, applied
/// per layer). [`Vfs::files`] lists the union.
pub struct LayeredVfs {
    base: Arc<dyn Vfs>,
    /// Lowest first.
    layers: Vec<ModLayer>,
    files: Vec<VfsPath>,
}

impl LayeredVfs {
    /// Layers lowest first, so the last one wins.
    pub fn new(base: Arc<dyn Vfs>, layers: Vec<ModLayer>) -> Self {
        let mut files: BTreeSet<VfsPath> = base.files().cloned().collect();
        for layer in &layers {
            files.extend(layer.provider.files().cloned());
        }
        Self {
            base,
            layers,
            files: files.into_iter().collect(),
        }
    }

    /// The mounted layers, lowest first.
    pub fn layers(&self) -> &[ModLayer] {
        &self.layers
    }

    /// The highest layer's copy of `path`, with that layer's name; `None`
    /// when no layer holds it. Only an absent file falls through.
    pub(crate) fn read_layers(
        &self,
        path: &str,
        max_bytes: usize,
    ) -> Option<Result<(Vec<u8>, String), VfsError>> {
        self.layers.iter().rev().find_map(|layer| {
            match layer.provider.read_bounded(path, max_bytes) {
                Err(VfsError::NotFound(_)) => None,
                result => Some(result.map(|bytes| (bytes, layer.name.clone()))),
            }
        })
    }
}

impl Vfs for LayeredVfs {
    fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        self.read_bounded(path, usize::MAX)
    }

    fn read_bounded(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, VfsError> {
        self.read_with_layer(path, max_bytes)
            .map(|(bytes, _)| bytes)
    }

    fn read_with_layer(
        &self,
        path: &str,
        max_bytes: usize,
    ) -> Result<(Vec<u8>, Option<String>), VfsError> {
        let normalized = VfsPath::new(path)?;
        match self.read_layers(normalized.as_str(), max_bytes) {
            Some(found) => found.map(|(bytes, name)| (bytes, Some(name))),
            None => self.base.read_with_layer(normalized.as_str(), max_bytes),
        }
    }

    fn files(&self) -> Box<dyn Iterator<Item = &VfsPath> + '_> {
        Box::new(self.files.iter())
    }

    fn mod_layers(&self) -> usize {
        self.layers.len() + self.base.mod_layers()
    }

    fn layer_manifests(&self) -> Result<Vec<(String, Vec<u8>)>, VfsError> {
        let mut manifests = self.base.layer_manifests()?;
        for layer in &self.layers {
            if let Some(bytes) =
                crate::mod_manifest::ModRegistry::read_layer_manifest(&layer.provider)?
            {
                manifests.push((layer.name.clone(), bytes));
            }
        }
        Ok(manifests)
    }
}

#[cfg(all(test, feature = "host-fs"))]
mod tests {
    use super::*;

    fn layer(path: &str, enabled: bool) -> LayerEntry {
        LayerEntry {
            path: Some(path.into()),
            enabled,
            ..LayerEntry::default()
        }
    }

    #[test]
    fn a_disabled_layer_is_parsed_and_listed_but_not_mounted() {
        let text = "[[layer]]\npath = \"mods/one\"\nenabled = false # parked\n\
                    [[layer]]\npath = \"mods/two\"\nenabled = true\n";
        let entries = parse_load_order(text).unwrap();
        assert_eq!(entries, [layer("mods/one", false), layer("mods/two", true)]);
        for bad in [
            "[[layer]]\npath = \"a\"\nenabled = \"no\"",
            "[[layer]]\npath = \"a\"\nenabled = false\nenabled = true",
            "[[layer]]\nenabled = false",
            "enabled = false",
            "layers = [\"a\"]\nenabled = false",
        ] {
            assert!(
                matches!(parse_load_order(bad), Err(VfsError::LoadOrder(_))),
                "{bad:?}"
            );
        }
        // Each table has its own `enabled`.
        let two =
            "[[layer]]\npath = \"a\"\nenabled = false\n[[layer]]\npath = \"b\"\nenabled = false";
        assert_eq!(
            parse_load_order(two).unwrap(),
            [layer("a", false), layer("b", false)]
        );
        let dir = tempfile::tempdir().unwrap();
        for name in ["one", "two"] {
            let folder = dir.path().join("mods").join(name);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("a.txt"), name).unwrap();
        }
        let strict = LoadOrderPolicy::default();
        let base: Arc<dyn Vfs> =
            Arc::new(LocalDirProvider::mount(dir.path().join("mods/one")).unwrap());
        let vfs = LayeredVfs::from_entries(base, dir.path(), &entries, strict).unwrap();
        assert_eq!(vfs.mod_layers(), 1);
        assert_eq!(vfs.layers()[0].name(), "mods/two");
        assert_eq!(vfs.read("a.txt").unwrap(), b"two");
        // A disabled layer is never opened, so a missing one is no error.
        let base: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(dir.path()).unwrap());
        let missing = [layer("mods/missing", false)];
        assert!(LayeredVfs::from_entries(base.clone(), dir.path(), &missing, strict).is_ok());
        let missing = [layer("mods/missing", true)];
        assert!(LayeredVfs::from_entries(base, dir.path(), &missing, strict).is_err());
    }

    #[test]
    fn the_writer_round_trips_through_the_parser() {
        let entries = vec![
            layer("mods/one", true),
            LayerEntry {
                path: Some(r"mods\two".into()),
                master_dat: Some("mods/two/\"quoted\".dat".into()),
                critter_dat: Some("mods/two/critter.dat".into()),
                enabled: false,
            },
            LayerEntry {
                critter_dat: Some("critters.dat".into()),
                ..LayerEntry::default()
            },
        ];
        let text = write_load_order(&entries).unwrap();
        assert!(text.contains("[[layer]]\npath = \"mods/one\"\n"), "{text}");
        assert!(text.contains("path = \"mods\\\\two\"\n"), "{text}");
        assert_eq!(text.matches("enabled = false").count(), 1);
        assert_eq!(parse_load_order(&text).unwrap(), entries);
        assert_eq!(
            parse_load_order(&write_load_order(&[]).unwrap()).unwrap(),
            []
        );
        assert!(matches!(
            write_load_order(&[layer("bad\nname", true)]),
            Err(VfsError::LoadOrder(_))
        ));
    }

    #[test]
    fn a_layered_read_names_the_layer_that_served_it() {
        let dir = tempfile::tempdir().unwrap();
        for (folder, files) in [
            ("base", ["a.txt", "b.txt"].as_slice()),
            ("mods/one", &["a.txt", "c.txt"]),
            ("mods/two", &["c.txt"]),
            ("music", &["track.acm"]),
        ] {
            let folder = dir.path().join(folder);
            std::fs::create_dir_all(&folder).unwrap();
            for file in files {
                std::fs::write(folder.join(file), *file).unwrap();
            }
        }
        let base: Arc<dyn Vfs> =
            Arc::new(LocalDirProvider::mount(dir.path().join("base")).unwrap());
        let entries = [layer("mods/one", true), layer("mods/two", true)];
        let layered: Arc<dyn Vfs> = Arc::new(
            LayeredVfs::from_entries(
                base.clone(),
                dir.path(),
                &entries,
                LoadOrderPolicy::default(),
            )
            .unwrap(),
        );
        let served = |vfs: &dyn Vfs, path: &str| vfs.read_with_layer(path, 64).unwrap();
        assert_eq!(
            served(&*layered, "A.TXT"),
            (b"a.txt".to_vec(), Some("mods/one".into()))
        );
        assert_eq!(served(&*layered, "b.txt"), (b"b.txt".to_vec(), None));
        assert_eq!(
            served(&*layered, "c.txt"),
            (b"c.txt".to_vec(), Some("mods/two".into()))
        );
        assert!(matches!(
            layered.read_with_layer("d.txt", 64),
            Err(VfsError::NotFound(_))
        ));
        // A plain provider, and a prefix mount's own directory, are base data.
        assert_eq!(served(&*base, "a.txt").1, None);
        let music = LocalDirProvider::mount(dir.path().join("music")).unwrap();
        let mount = PrefixMount::new(layered, "sound/music", music).unwrap();
        assert_eq!(served(&mount, "sound/music/track.acm").1, None);
        assert_eq!(served(&mount, "a.txt").1.as_deref(), Some("mods/one"));
    }
}
