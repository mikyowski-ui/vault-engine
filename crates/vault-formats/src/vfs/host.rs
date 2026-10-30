//! The file-backed side of [`crate::vfs`] (SPEC-023 section 3.6, `host-fs`):
//! loose folders opened through a directory capability, and load orders
//! resolved against a folder. Nothing outside this module opens an OS path
//! for the virtual file system.
use super::{LayerEntry, LayeredVfs, LoadOrderPolicy, ModLayer, Vfs, VfsError, VfsPath};
use super::{load_order_error, parse_load_order};
use cap_std::{ambient_authority, fs::Dir};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

fn io_error(path: &Path, source: io::Error) -> VfsError {
    VfsError::Io {
        path: path.to_owned(),
        source,
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
        self.read_with_layer(path, max_bytes)
            .map(|(bytes, _)| bytes)
    }

    fn read_with_layer(
        &self,
        path: &str,
        max_bytes: usize,
    ) -> Result<(Vec<u8>, Option<String>), VfsError> {
        match self.primary.read_with_layer(path, max_bytes) {
            // The mounted directory is base data.
            Err(VfsError::NotFound(missing)) => match missing.as_str().strip_prefix(&self.prefix) {
                Some(rest) => self
                    .dir
                    .read_bounded(rest, max_bytes)
                    .map(|bytes| (bytes, None))
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

    fn layer_manifests(&self) -> Result<Vec<(String, Vec<u8>)>, VfsError> {
        self.primary.layer_manifests()
    }
}

impl ModLayer {
    /// A layer from already-mounted parts.
    pub fn new(
        name: impl Into<String>,
        loose: Option<LocalDirProvider>,
        master: Option<crate::DatArchive>,
        critter: Option<crate::DatArchive>,
    ) -> Self {
        let loose = loose.map(|root| Arc::new(root) as Arc<dyn Vfs>);
        Self::from_parts(name, loose, master, critter)
    }
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

impl LayeredVfs {
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
        Self::from_entries(base, directory, &entries, policy)
    }

    /// Layers from already-parsed entries (lowest first), their paths
    /// resolved against `directory` as a load order there would resolve them.
    /// Disabled entries are skipped.
    pub fn from_entries(
        base: Arc<dyn Vfs>,
        directory: &Path,
        entries: &[LayerEntry],
        policy: LoadOrderPolicy,
    ) -> Result<Self, VfsError> {
        let dir = Dir::open_ambient_dir(directory, ambient_authority())
            .map_err(|error| io_error(directory, error))?;
        let layers = entries
            .iter()
            .filter(|entry| entry.enabled)
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
}
