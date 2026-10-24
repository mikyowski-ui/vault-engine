use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

use cap_std::{ambient_authority, fs::Dir};

/// A canonical, relative asset path: lowercase ASCII with `/` separators.
///
/// Both slash styles are accepted on input. Empty components, `.`/`..`,
/// absolute/drive/UNC paths, control characters, Windows special characters,
/// device names, and trailing dots/spaces are rejected rather than repaired.
/// Restricting names to ASCII gives identical case folding on every platform.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VfsPath(String);

impl VfsPath {
    pub fn new(path: &str) -> Result<Self, VfsError> {
        let invalid = |reason| VfsError::InvalidPath {
            path: path.to_owned(),
            reason,
        };
        if path.is_empty() || !path.is_ascii() {
            return Err(invalid("expected a nonempty ASCII path"));
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
    #[error("load order: {0}")]
    LoadOrder(String),
    #[error("filesystem error at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

fn io_error(path: &Path, source: io::Error) -> VfsError {
    VfsError::Io {
        path: path.to_owned(),
        source,
    }
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

    /// Walk all mounted files in canonical lexical order (directories excluded).
    fn files(&self) -> Box<dyn Iterator<Item = &VfsPath> + '_>;

    /// How many mod layers ([`LayeredVfs`]) sit above the base data; 0 for
    /// a plain provider.
    fn mod_layers(&self) -> usize {
        0
    }
}

/// A snapshot of the namespace of an unpacked game directory.
///
/// Root files and arbitrary subdirectories are indexed; no extension or folder
/// allowlist is applied. [`Vfs::files`] lists the mount-time snapshot (remount
/// to list added or renamed files), but under the SPEC-008 §3.2 safe rule a
/// read that misses the snapshot consults the live directory, so a stale
/// index never hides a file added later. Contents are read on demand. Files/directories that collide after case folding, links,
/// and special files cause mount failure instead of ambiguous or partial results.
///
/// All filesystem operations after mounting are relative to an open directory
/// capability. Even if the tree changes between checking and opening a path,
/// the operation cannot follow a symlink outside that capability. The selected
/// root stays anchored if its ambient path is renamed or replaced.
#[derive(Debug)]
pub struct LocalDirProvider {
    root: Dir,
    files: BTreeMap<VfsPath, PathBuf>,
}

impl LocalDirProvider {
    pub fn mount(root: impl AsRef<Path>) -> Result<Self, VfsError> {
        let root_path = root.as_ref();
        let root = Dir::open_ambient_dir(root_path, ambient_authority())
            .map_err(|error| io_error(root_path, error))?;
        Self::mount_dir(root)
    }

    /// Mount an already-open directory capability (for example a mod
    /// layer opened through its load order's directory).
    pub fn mount_dir(root: Dir) -> Result<Self, VfsError> {
        let mut files = BTreeMap::new();
        let mut seen = BTreeMap::new();
        let mut pending = vec![PathBuf::new()];

        while let Some(directory) = pending.pop() {
            let entries = if directory.as_os_str().is_empty() {
                root.entries()
            } else {
                root.read_dir(&directory)
            }
            .map_err(|error| io_error(&directory, error))?;
            let mut entries = entries
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| io_error(&directory, error))?;
            entries.sort_by_key(|entry| entry.file_name());

            for entry in entries {
                let name = entry.file_name();
                let native_path = directory.join(&name);
                let name = name
                    .to_str()
                    .ok_or_else(|| VfsError::NonUtf8Name(native_path.clone()))?;
                // A literal backslash in a Unix filename must not become a
                // virtual directory separator.
                if name.contains('\\') {
                    return Err(VfsError::InvalidPath {
                        path: name.to_owned(),
                        reason: "filename contains a separator",
                    });
                }
                let path = VfsPath::new(
                    native_path
                        .to_str()
                        .ok_or_else(|| VfsError::NonUtf8Name(native_path.clone()))?,
                )?;
                if let Some(first) = seen.insert(path.clone(), native_path.clone()) {
                    return Err(VfsError::CaseCollision {
                        path,
                        first,
                        second: native_path,
                    });
                }
                let kind = root
                    .symlink_metadata(&native_path)
                    .map_err(|error| io_error(&native_path, error))?
                    .file_type();
                if kind.is_symlink() {
                    return Err(VfsError::Symlink(native_path));
                } else if kind.is_dir() {
                    pending.push(native_path);
                } else if kind.is_file() {
                    files.insert(path, native_path);
                } else {
                    return Err(VfsError::UnsupportedEntry(native_path));
                }
            }
        }
        Ok(Self { root, files })
    }

    fn check_components(&self, path: &Path) -> Result<(), VfsError> {
        let mut prefix = PathBuf::new();
        for component in path.components() {
            prefix.push(component);
            let kind = self
                .root
                .symlink_metadata(&prefix)
                .map_err(|error| io_error(&prefix, error))?
                .file_type();
            if kind.is_symlink() {
                return Err(VfsError::Symlink(prefix));
            }
            if !(kind.is_file() || kind.is_dir()) {
                return Err(VfsError::UnsupportedEntry(prefix));
            }
        }
        Ok(())
    }

    /// Resolve a path missing from the snapshot against the live directory,
    /// one case-insensitive component at a time through the capability.
    /// Ambiguous case-folded names are rejected as at mount time.
    fn resolve_live(&self, path: &VfsPath) -> Result<Option<PathBuf>, VfsError> {
        let mut native = PathBuf::new();
        for part in path.as_str().split('/') {
            let entries = if native.as_os_str().is_empty() {
                self.root.entries()
            } else {
                self.root.read_dir(&native)
            };
            let Ok(entries) = entries else {
                return Ok(None);
            };
            let mut found: Option<PathBuf> = None;
            for entry in entries {
                let entry = entry.map_err(|error| io_error(&native, error))?;
                let name = entry.file_name();
                if !name
                    .to_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case(part))
                {
                    continue;
                }
                let candidate = native.join(&name);
                if let Some(first) = found {
                    return Err(VfsError::CaseCollision {
                        path: path.clone(),
                        first,
                        second: candidate,
                    });
                }
                found = Some(candidate);
            }
            let Some(next) = found else {
                return Ok(None);
            };
            native = next;
        }
        Ok(Some(native))
    }

    /// Bound allocation before reading, and enforce the limit if the file grows.
    pub fn read_bounded(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, VfsError> {
        let path = VfsPath::new(path)?;
        let native_path = match self.files.get(&path) {
            Some(native) => native.clone(),
            None => self
                .resolve_live(&path)?
                .ok_or_else(|| VfsError::NotFound(path.clone()))?,
        };
        let native_path = &native_path;
        match self.check_components(native_path) {
            // A snapshot entry removed since the mount is simply absent
            // (SPEC-008 §3.2: the archive is consulted next).
            Err(VfsError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                return Err(VfsError::NotFound(path.clone()));
            }
            result => result?,
        }
        // Use the capability for the actual open, not an ambient/canonical path.
        let file = self.root.open(native_path).map_err(|error| {
            // A snapshot entry removed since the mount is simply absent
            // (SPEC-008 §3.2: the archive is consulted next).
            if error.kind() == io::ErrorKind::NotFound {
                VfsError::NotFound(path.clone())
            } else {
                io_error(native_path, error)
            }
        })?;
        let metadata = file
            .metadata()
            .map_err(|error| io_error(native_path, error))?;
        if !metadata.is_file() {
            return Err(VfsError::UnsupportedEntry(native_path.clone()));
        }
        let too_large = || VfsError::FileTooLarge {
            path: native_path.clone(),
            max_bytes,
        };
        if metadata.len() > max_bytes as u64 {
            return Err(too_large());
        }
        let mut contents = Vec::new();
        file.take((max_bytes as u64).saturating_add(1))
            .read_to_end(&mut contents)
            .map_err(|error| io_error(native_path, error))?;
        if contents.len() > max_bytes {
            return Err(too_large());
        }
        Ok(contents)
    }
}

impl Vfs for LocalDirProvider {
    fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        LocalDirProvider::read_bounded(self, path, usize::MAX)
    }

    fn read_bounded(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, VfsError> {
        LocalDirProvider::read_bounded(self, path, max_bytes)
    }

    fn files(&self) -> Box<dyn Iterator<Item = &VfsPath> + '_> {
        Box::new(self.files.keys())
    }
}

/// A loose directory mounted under a fixed VFS prefix behind another
/// provider. A read under the prefix that the primary provider does not
/// find falls back to the directory, so the primary (and its own loose
/// root) always wins. Used for loose `.ACM` music kept beside the DAT
/// archives rather than inside them.
pub struct PrefixMount {
    primary: Arc<dyn Vfs>,
    prefix: String,
    dir: LocalDirProvider,
    files: Vec<VfsPath>,
}

impl PrefixMount {
    /// Mount `dir` as `prefix/...` (a relative VFS directory path).
    pub fn new(
        primary: Arc<dyn Vfs>,
        prefix: &str,
        dir: LocalDirProvider,
    ) -> Result<Self, VfsError> {
        let prefix = format!("{}/", VfsPath::new(prefix)?.as_str());
        let mut files: BTreeSet<VfsPath> = primary.files().cloned().collect();
        for file in dir.files() {
            files.insert(VfsPath::new(&format!("{prefix}{file}"))?);
        }
        Ok(Self {
            primary,
            prefix,
            dir,
            files: files.into_iter().collect(),
        })
    }

    /// Files served from the mounted directory.
    pub fn mounted(&self) -> usize {
        self.dir.files().count()
    }
}

impl Vfs for PrefixMount {
    fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        self.read_bounded(path, usize::MAX)
    }

    fn read_bounded(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, VfsError> {
        match self.primary.read_bounded(path, max_bytes) {
            Err(VfsError::NotFound(missing)) => match missing.as_str().strip_prefix(&self.prefix) {
                Some(rest) => self
                    .dir
                    .read_bounded(rest, max_bytes)
                    .map_err(|error| match error {
                        VfsError::NotFound(_) => VfsError::NotFound(missing),
                        other => other,
                    }),
                None => Err(VfsError::NotFound(missing)),
            },
            result => result,
        }
    }

    fn files(&self) -> Box<dyn Iterator<Item = &VfsPath> + '_> {
        Box::new(self.files.iter())
    }

    fn mod_layers(&self) -> usize {
        self.primary.mod_layers()
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
    /// A layer from already-mounted parts.
    pub fn new(
        name: impl Into<String>,
        loose: Option<LocalDirProvider>,
        master: Option<crate::DatArchive>,
        critter: Option<crate::DatArchive>,
    ) -> Self {
        let loose = loose.map(Arc::new);
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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayerEntry {
    /// A loose folder, or a `.dat` file used as the layer's master archive.
    pub path: Option<String>,
    /// The layer's master archive, beside a loose `path` folder.
    pub master_dat: Option<String>,
    /// The layer's critter archive (`art/critters/` requests).
    pub critter_dat: Option<String>,
}

/// How [`LayeredVfs::from_load_order_with`] resolves layer paths.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LoadOrderPolicy {
    /// Accept absolute paths and `..` components that leave the load
    /// order's directory. Off by default: every layer must lie inside it.
    pub allow_outside: bool,
}

fn load_order_error(message: impl Into<String>) -> VfsError {
    VfsError::LoadOrder(message.into())
}

/// Strip a `#` comment that is not inside a string.
fn strip_comment(line: &str) -> &str {
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
fn toml_string(text: &str) -> Result<(String, &str), VfsError> {
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
/// with `path = "..."` and the optional `master_dat` / `critter_dat` keys.
/// `#` starts a comment. Entries are listed lowest first: the last layer
/// wins. Unknown keys, sections, or a mix of both forms are errors.
pub fn parse_load_order(text: &str) -> Result<Vec<LayerEntry>, VfsError> {
    let mut entries: Vec<LayerEntry> = Vec::new();
    let mut array_form = false;
    let mut in_table = false;
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
        let Some(entry) = entries.last_mut().filter(|_| in_table) else {
            return Err(at(format!("unknown key {key:?}")));
        };
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

/// Whether `path` is absolute or has a drive or root prefix, in either
/// slash style, independent of the host platform.
fn is_rooted(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with(['/', '\\'])
        || (bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic())
        || Path::new(path).is_absolute()
}

/// A layer path resolved against the load order's directory.
enum Located {
    /// Relative, inside the directory: opened through its capability.
    Inside(PathBuf),
    /// Outside, allowed by the policy: opened ambiently.
    Outside(PathBuf),
}

fn locate(path: &str, policy: LoadOrderPolicy, base: &Path) -> Result<Located, VfsError> {
    if path.trim().is_empty() {
        return Err(load_order_error("empty layer path"));
    }
    let traversal = path.split(['/', '\\']).any(|part| part == "..");
    if is_rooted(path) || traversal {
        if !policy.allow_outside {
            return Err(load_order_error(format!(
                "layer path {path:?} leaves the load order's directory"
            )));
        }
        return Ok(Located::Outside(base.join(path)));
    }
    let relative: PathBuf = path
        .split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    if relative.as_os_str().is_empty() {
        return Err(load_order_error(format!(
            "layer path {path:?} names the directory itself"
        )));
    }
    Ok(Located::Inside(relative))
}

/// Reject a symlink anywhere along a relative path inside `dir`.
fn no_links(dir: &Dir, relative: &Path) -> Result<(), VfsError> {
    let mut prefix = PathBuf::new();
    for component in relative.components() {
        prefix.push(component);
        let kind = dir
            .symlink_metadata(&prefix)
            .map_err(|error| io_error(&prefix, error))?
            .file_type();
        if kind.is_symlink() {
            return Err(VfsError::Symlink(prefix));
        }
    }
    Ok(())
}

fn open_folder(dir: &Dir, located: &Located) -> Result<LocalDirProvider, VfsError> {
    match located {
        Located::Inside(relative) => {
            no_links(dir, relative)?;
            let folder = dir
                .open_dir(relative)
                .map_err(|error| io_error(relative, error))?;
            LocalDirProvider::mount_dir(folder)
        }
        Located::Outside(path) => LocalDirProvider::mount(path),
    }
}

fn open_archive(dir: &Dir, located: &Located) -> Result<crate::DatArchive, VfsError> {
    let archive = |file: std::fs::File, path: &Path| {
        crate::DatArchive::from_reader(file)
            .map_err(|error| VfsError::Archive(format!("{}: {error}", path.display())))
    };
    match located {
        Located::Inside(relative) => {
            no_links(dir, relative)?;
            let file = dir
                .open(relative)
                .map_err(|error| io_error(relative, error))?
                .into_std();
            if !file
                .metadata()
                .map_err(|e| io_error(relative, e))?
                .is_file()
            {
                return Err(VfsError::UnsupportedEntry(relative.clone()));
            }
            archive(file, relative)
        }
        Located::Outside(path) => {
            let file = std::fs::File::open(path).map_err(|error| io_error(path, error))?;
            archive(file, path)
        }
    }
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

    /// Read `load_order.toml` at `path` and mount its layers above `base`.
    /// Layer paths are relative to the file's directory and may not leave
    /// it ([`LoadOrderPolicy::default`]). A missing layer is an error.
    pub fn from_load_order(base: Arc<dyn Vfs>, path: impl AsRef<Path>) -> Result<Self, VfsError> {
        Self::from_load_order_with(base, path, LoadOrderPolicy::default())
    }

    pub fn from_load_order_with(
        base: Arc<dyn Vfs>,
        path: impl AsRef<Path>,
        policy: LoadOrderPolicy,
    ) -> Result<Self, VfsError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|error| io_error(path, error))?;
        let entries = parse_load_order(&text)?;
        let directory = match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        };
        let dir = Dir::open_ambient_dir(directory, ambient_authority())
            .map_err(|error| io_error(directory, error))?;
        let layers = entries
            .iter()
            .map(|entry| Self::open_layer(&dir, directory, entry, policy))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self::new(base, layers))
    }

    fn open_layer(
        dir: &Dir,
        directory: &Path,
        entry: &LayerEntry,
        policy: LoadOrderPolicy,
    ) -> Result<ModLayer, VfsError> {
        let mut loose = None;
        let mut master = None;
        if let Some(path) = &entry.path {
            let located = locate(path, policy, directory)?;
            if path.to_ascii_lowercase().ends_with(".dat") {
                master = Some(open_archive(dir, &located)?);
            } else {
                loose = Some(open_folder(dir, &located)?);
            }
        }
        if let Some(path) = &entry.master_dat {
            if master.is_some() {
                return Err(load_order_error(format!(
                    "layer {path:?} names two master archives"
                )));
            }
            master = Some(open_archive(dir, &locate(path, policy, directory)?)?);
        }
        let critter = entry
            .critter_dat
            .as_ref()
            .map(|path| open_archive(dir, &locate(path, policy, directory)?))
            .transpose()?;
        let name = entry
            .path
            .clone()
            .or_else(|| entry.master_dat.clone())
            .or_else(|| entry.critter_dat.clone())
            .unwrap_or_default();
        Ok(ModLayer::new(name, loose, master, critter))
    }

    /// The mounted layers, lowest first.
    pub fn layers(&self) -> &[ModLayer] {
        &self.layers
    }
}

impl Vfs for LayeredVfs {
    fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        self.read_bounded(path, usize::MAX)
    }

    fn read_bounded(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, VfsError> {
        let normalized = VfsPath::new(path)?;
        for layer in self.layers.iter().rev() {
            match layer.provider.read_bounded(normalized.as_str(), max_bytes) {
                Err(VfsError::NotFound(_)) => {}
                result => return result,
            }
        }
        self.base.read_bounded(normalized.as_str(), max_bytes)
    }

    fn files(&self) -> Box<dyn Iterator<Item = &VfsPath> + '_> {
        Box::new(self.files.iter())
    }

    fn mod_layers(&self) -> usize {
        self.layers.len() + self.base.mod_layers()
    }
}
