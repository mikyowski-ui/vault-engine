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
}
