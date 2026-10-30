//! SPEC-023 §3.7-§3.14: where the engine's own state lives. One synchronous
//! [`StorageBackend`] holds three namespaces (settings, save slots and mod
//! layers) and answers every host the same way: [`FsStorage`] keeps the
//! desktop's files where and as they are, [`MemoryStorage`] is the reference
//! for tests and replays, and [`KvStorage`] adapts a flat key-value store,
//! such as a browser's. Game data never comes through here (SPEC-023 D-11).
use crate::config::game_name;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};
use vault_core::{GameProfile, host_save::QUICK_SLOT};
use vault_formats::{
    ByteSource, LocalDirProvider, MAX_ASSET_BYTES, MemoryDir, Vfs, VfsError, VfsPath,
    mod_manifest::MANIFEST,
    staged::{self, StagedOp, StagingNames},
};

/// A slot's central file (SPEC-008 §2.4), which makes a folder a slot.
const SLOT_MARK: &str = vault_core::host_save::CENTRAL_FILE;

/// §2.4: the three namespaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Namespace {
    Config,
    Savegame,
    Mods,
}

impl Namespace {
    pub fn name(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Savegame => "savegame",
            Self::Mods => "mods",
        }
    }
}

/// §3.7: the closed error type. Host details appear only in messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageError {
    InvalidKey {
        key: String,
        rule: &'static str,
    },
    TooLarge {
        key: String,
    },
    QuotaExceeded,
    /// Two stored entries share a canonical key.
    Collision {
        key: String,
    },
    LayerNotFound {
        layer: String,
    },
    /// The store failed or was closed.
    Unavailable(String),
    Io(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKey { key, rule } => write!(f, "storage key {key:?}: {rule}"),
            Self::TooLarge { key } => {
                write!(f, "{key}: over the {MAX_ASSET_BYTES}-byte value limit")
            }
            Self::QuotaExceeded => f.write_str("the storage quota is used up"),
            Self::Collision { key } => write!(f, "{key}: two entries differ only in case"),
            Self::LayerNotFound { layer } => write!(f, "mod layer {layer}: not found"),
            Self::Unavailable(message) => write!(f, "storage unavailable: {message}"),
            Self::Io(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<StorageError> for vault_core::WorldError {
    fn from(error: StorageError) -> Self {
        Self::Invalid(error.to_string())
    }
}

/// One operation of a batch ([`StorageBackend::commit`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageOp {
    Put { key: String, bytes: Vec<u8> },
    Delete { key: String },
}

impl StorageOp {
    pub fn put(key: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self::Put {
            key: key.into(),
            bytes: bytes.into(),
        }
    }
    pub fn delete(key: impl Into<String>) -> Self {
        Self::Delete { key: key.into() }
    }
    fn key(&self) -> &str {
        match self {
            Self::Put { key, .. } | Self::Delete { key } => key,
        }
    }
}

/// The key `key` of `namespace` in canonical form (§2.2, §2.4), through the
/// loose provider's own validator. `.tmp` and `.bak` names belong to the
/// staged write outside `mods`.
pub fn canonical_key(namespace: Namespace, key: &str) -> Result<String, StorageError> {
    let invalid = |rule| StorageError::InvalidKey {
        key: key.to_owned(),
        rule,
    };
    let path = VfsPath::new(key).map_err(|_| invalid("invalid path"))?;
    let canonical = path.as_str();
    let last = canonical.rsplit('/').next().unwrap_or(canonical);
    if namespace != Namespace::Mods && (last.ends_with(".tmp") || last.ends_with(".bak")) {
        return Err(invalid("reserved suffix"));
    }
    let components = canonical.split('/').count();
    match namespace {
        Namespace::Config if components != 1 || !last.ends_with(".toml") => {
            Err(invalid("a config key is one .toml name"))
        }
        Namespace::Savegame if components != 2 => Err(invalid("a savegame key is <slot>/<file>")),
        _ => Ok(canonical.to_owned()),
    }
}

/// §2.5: the record key of a canonical key in a flat key-value store.
pub fn record_key(profile: GameProfile, namespace: Namespace, canonical: &str) -> String {
    let scope = match namespace {
        Namespace::Savegame => game_name(profile),
        Namespace::Config | Namespace::Mods => "shared",
    };
    format!("{RECORD_ROOT}/{scope}/{}/{canonical}", namespace.name())
}

/// §2.5: every record of layout version 1 starts here.
const RECORD_ROOT: &str = "vault-engine/1";

/// A list's or layer's key prefix in canonical form.
fn canonical_prefix(prefix: &str) -> Result<String, StorageError> {
    VfsPath::new(prefix)
        .map(|path| path.as_str().to_owned())
        .map_err(|_| StorageError::InvalidKey {
            key: prefix.to_owned(),
            rule: "invalid path",
        })
}

/// The net effect of a batch on one key: its spelling, and the value of a
/// put (`None` for a delete).
struct NetOp<'a> {
    spelling: &'a str,
    bytes: Option<&'a [u8]>,
}

/// §3.8 rules 1-2: validate a whole batch, then reduce it to one operation
/// per canonical key, a later operation replacing an earlier one.
fn net_effect<'a>(
    namespace: Namespace,
    ops: &'a [StorageOp],
) -> Result<BTreeMap<String, NetOp<'a>>, StorageError> {
    let mut net = BTreeMap::new();
    for op in ops {
        let canonical = canonical_key(namespace, op.key())?;
        let bytes = match op {
            StorageOp::Put { bytes, .. } => {
                if bytes.len() > MAX_ASSET_BYTES {
                    return Err(StorageError::TooLarge {
                        key: op.key().to_owned(),
                    });
                }
                Some(bytes.as_slice())
            }
            StorageOp::Delete { .. } => None,
        };
        net.insert(
            canonical,
            NetOp {
                spelling: op.key(),
                bytes,
            },
        );
    }
    Ok(net)
}

/// In `mods`, where keys nest: a key that would also be another's folder is
/// refused by every backend, because a file system cannot hold both. A batch
/// that turns a file into a folder counts too, since a file system cannot
/// stage its way there: every backend takes that as two commits.
fn check_folders(
    net: &BTreeMap<String, NetOp<'_>>,
    stored: Vec<String>,
) -> Result<(), StorageError> {
    let mut keys: BTreeSet<String> = stored.into_iter().collect();
    let puts = net.iter().filter(|(_, op)| op.bytes.is_some());
    keys.extend(puts.map(|(key, _)| key.clone()));
    for key in &keys {
        for (at, _) in key.match_indices('/') {
            if keys.contains(&key[..at]) {
                return Err(StorageError::InvalidKey {
                    key: key.clone(),
                    rule: "a key is both a file and a folder",
                });
            }
        }
    }
    Ok(())
}

/// §3.7: the operations every backend shares. Reads and listings answer from
/// committed state; [`Self::commit`] is atomic (§3.8); keys keep their
/// caller's spelling and compare by their canonical form.
pub trait StorageBackend: Send + Sync {
    /// The entry's bytes, or `None` when it is absent.
    fn read(&self, namespace: Namespace, key: &str) -> Result<Option<Vec<u8>>, StorageError>;
    /// The canonical keys below `prefix`, or of the whole namespace, in
    /// canonical order. A prefix matches whole components only.
    fn list(&self, namespace: Namespace, prefix: Option<&str>)
    -> Result<Vec<String>, StorageError>;
    /// Apply a batch of puts and deletes atomically.
    fn commit(&self, namespace: Namespace, ops: &[StorageOp]) -> Result<(), StorageError>;
    /// Return when every committed batch is durable, or report the first
    /// that could not be made durable.
    fn flush(&self) -> Result<(), StorageError>;

    /// §3.12 rule 4: the `mods` layer at key prefix `layer`, as a provider
    /// of its files. A layer with no file is [`StorageError::LayerNotFound`].
    fn mount_layer(&self, layer: &str) -> Result<Arc<dyn Vfs>, StorageError> {
        let prefix = canonical_prefix(layer)?;
        let keys = self.list(Namespace::Mods, Some(&prefix))?;
        let mut entries = Vec::with_capacity(keys.len());
        for key in keys {
            let bytes = self
                .read(Namespace::Mods, &key)?
                .ok_or_else(|| StorageError::Unavailable(format!("{key} vanished")))?;
            let bytes = ByteSource::new(bytes).map_err(vfs_failure)?;
            let relative = key[prefix.len() + 1..].to_owned();
            entries.push((relative, bytes));
        }
        if entries.is_empty() {
            return Err(StorageError::LayerNotFound { layer: prefix });
        }
        Ok(Arc::new(MemoryDir::new(entries).map_err(vfs_failure)?))
    }

    /// §3.11 rule 4: `SLOT01`-`SLOT10`, then `QUICKSAV`, keeping those whose
    /// `SAVE.DAT` exists.
    fn slots(&self) -> Result<Vec<String>, StorageError> {
        let mut slots = Vec::new();
        let names = (1..=10)
            .map(|slot| format!("SLOT{slot:02}"))
            .chain([QUICK_SLOT.to_owned()]);
        for name in names {
            let mark = format!("{name}/{SLOT_MARK}").to_ascii_lowercase();
            if self.list(Namespace::Savegame, Some(&name))?.contains(&mark) {
                slots.push(name);
            }
        }
        Ok(slots)
    }

    /// §3.12 rule 3: every prefix P such that `P/vault-mod.toml` exists, in
    /// canonical order. A manifest at the namespace root is not a layer.
    fn find_layers(&self) -> Result<Vec<String>, StorageError> {
        let suffix = format!("/{MANIFEST}");
        let layers: BTreeSet<String> = self
            .list(Namespace::Mods, None)?
            .iter()
            .filter_map(|key| key.strip_suffix(&suffix))
            .map(str::to_owned)
            .collect();
        Ok(layers.into_iter().collect())
    }

    /// §3.12 rule 2: the manifest of `layer`, or `None`.
    fn layer_manifest(&self, layer: &str) -> Result<Option<Vec<u8>>, StorageError> {
        self.read(Namespace::Mods, &format!("{layer}/{MANIFEST}"))
    }
}

impl fmt::Debug for dyn StorageBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StorageBackend")
    }
}

fn vfs_failure(error: VfsError) -> StorageError {
    match error {
        VfsError::CaseCollision { path, .. } => StorageError::Collision {
            key: path.as_str().to_owned(),
        },
        other => StorageError::Io(other.to_string()),
    }
}

/// The entries of a backend that holds them in memory: the reference
/// behavior of §3.7, shared by [`MemoryStorage`] and [`KvStorage`].
#[derive(Debug, Default)]
struct Entries {
    entries: BTreeMap<(Namespace, String), Vec<u8>>,
}

impl Entries {
    fn read(&self, namespace: Namespace, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let key = canonical_key(namespace, key)?;
        Ok(self.entries.get(&(namespace, key)).cloned())
    }
    fn list(
        &self,
        namespace: Namespace,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, StorageError> {
        let below = prefix
            .map(canonical_prefix)
            .transpose()?
            .map(|prefix| prefix + "/");
        Ok(self
            .entries
            .keys()
            .filter(|(space, key)| {
                *space == namespace
                    && below
                        .as_ref()
                        .is_none_or(|below| key.starts_with(below.as_str()))
            })
            .map(|(_, key)| key.clone())
            .collect())
    }
    /// The batch's net effect, refused here if it cannot apply.
    fn prepare<'a>(
        &self,
        namespace: Namespace,
        ops: &'a [StorageOp],
    ) -> Result<BTreeMap<String, NetOp<'a>>, StorageError> {
        let net = net_effect(namespace, ops)?;
        if namespace == Namespace::Mods {
            check_folders(&net, self.list(namespace, None)?)?;
        }
        Ok(net)
    }
    fn apply(&mut self, namespace: Namespace, net: BTreeMap<String, NetOp<'_>>) {
        for (key, op) in net {
            match op.bytes {
                Some(bytes) => self.entries.insert((namespace, key), bytes.to_vec()),
                None => self.entries.remove(&(namespace, key)),
            };
        }
    }
    /// §3.8 rule 5: canonical key bytes plus value bytes, over all namespaces.
    fn usage(&self) -> u64 {
        self.entries
            .iter()
            .map(|((_, key), value)| (key.len() + value.len()) as u64)
            .sum()
    }
    /// [`Self::usage`] after `net` applied to `namespace`.
    fn usage_after(&self, namespace: Namespace, net: &BTreeMap<String, NetOp<'_>>) -> u64 {
        net.iter().fold(self.usage(), |usage, (key, op)| {
            let entry = |value: usize| (key.len() + value) as u64;
            let old = self
                .entries
                .get(&(namespace, key.clone()))
                .map_or(0, |value| entry(value.len()));
            let new = op.bytes.map_or(0, |value| entry(value.len()));
            usage - old + new
        })
    }
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A commit applies to memory in one step after its checks, so a panic
    // elsewhere leaves nothing half done.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The settings or load-order file at `path` as a `config` key of its
/// folder; `None` for a name that is no such key, which keeps its plain
/// meaning on the desktop (§3.10 rule 3).
pub fn config_file(path: &Path) -> Option<(FsStorage, String)> {
    let name = path.file_name()?.to_str()?;
    canonical_key(Namespace::Config, name).ok()?;
    let folder = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    Some((FsStorage::config_at(folder), name.to_owned()))
}

/// The settings or load-order text at `path`, or `None` when it is absent.
pub fn read_config_file(path: &Path) -> Result<Option<String>, String> {
    let failed = |error: &dyn fmt::Display| format!("{}: {error}", path.display());
    let bytes = match config_file(path) {
        Some((storage, key)) => storage
            .read(Namespace::Config, &key)
            .map_err(|error| failed(&error))?,
        None => match fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(failed(&error)),
        },
    };
    bytes
        .map(|bytes| String::from_utf8(bytes).map_err(|error| failed(&error)))
        .transpose()
}

/// §3.14: the three namespaces in memory, with an optional quota. The
/// reference for the conformance fixtures, and what a replay runs on.
#[derive(Debug, Default)]
pub struct MemoryStorage {
    quota: Option<u64>,
    entries: Mutex<Entries>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }
    /// A backend that refuses to hold more than `quota` bytes (§3.8 rule 5).
    pub fn with_quota(quota: u64) -> Self {
        Self {
            quota: Some(quota),
            ..Self::default()
        }
    }
    #[cfg(test)]
    pub fn usage(&self) -> u64 {
        locked(&self.entries).usage()
    }
}

impl StorageBackend for MemoryStorage {
    fn read(&self, namespace: Namespace, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        locked(&self.entries).read(namespace, key)
    }
    fn list(
        &self,
        namespace: Namespace,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, StorageError> {
        locked(&self.entries).list(namespace, prefix)
    }
    fn commit(&self, namespace: Namespace, ops: &[StorageOp]) -> Result<(), StorageError> {
        let mut entries = locked(&self.entries);
        let net = entries.prepare(namespace, ops)?;
        if self
            .quota
            .is_some_and(|quota| entries.usage_after(namespace, &net) > quota)
        {
            return Err(StorageError::QuotaExceeded);
        }
        entries.apply(namespace, net);
        Ok(())
    }
    fn flush(&self) -> Result<(), StorageError> {
        Ok(())
    }
}

/// §3.9: the desktop's folders. Each namespace maps to one folder (§2.4);
/// a namespace with no folder is unavailable.
#[derive(Debug, Default)]
pub struct FsStorage {
    config: Option<PathBuf>,
    savegame: Option<PathBuf>,
    mods: Option<PathBuf>,
    /// T12: fail while staging the file at this index.
    #[cfg(test)]
    fail_staging: Option<usize>,
}

fn io_failure(path: &Path, error: impl fmt::Display) -> StorageError {
    StorageError::Io(format!("{}: {error}", path.display()))
}

/// The entry of `dir` named `name` with case folded, by the loose provider's
/// rules: two matches are a collision, and links and special files are
/// refused. `None` when there is none, or `dir` is not a folder.
fn lookup(
    dir: &Path,
    name: &str,
    key: &str,
) -> Result<Option<(PathBuf, fs::FileType)>, StorageError> {
    use std::io::ErrorKind;
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            return Ok(None);
        }
        Err(error) => return Err(io_failure(dir, error)),
    };
    let mut found = None;
    for entry in entries {
        let entry = entry.map_err(|error| io_failure(dir, error))?;
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|text| text.eq_ignore_ascii_case(name))
        {
            continue;
        }
        if found.replace(entry.path()).is_some() {
            return Err(StorageError::Collision {
                key: key.to_owned(),
            });
        }
    }
    let Some(path) = found else {
        return Ok(None);
    };
    let kind = checked_kind(&path)?;
    Ok(Some((path, kind)))
}

/// A regular file's or folder's type; anything else is refused.
fn checked_kind(path: &Path) -> Result<fs::FileType, StorageError> {
    let kind = fs::symlink_metadata(path)
        .map_err(|error| io_failure(path, error))?
        .file_type();
    if kind.is_symlink() {
        return Err(io_failure(path, "symbolic links are refused"));
    }
    if !(kind.is_file() || kind.is_dir()) {
        return Err(io_failure(path, "not a regular file or folder"));
    }
    Ok(kind)
}

impl FsStorage {
    /// Folders for every namespace (§2.4, table).
    pub fn new(config: Option<PathBuf>, savegame: Option<PathBuf>, mods: Option<PathBuf>) -> Self {
        Self {
            config,
            savegame,
            mods,
            #[cfg(test)]
            fail_staging: None,
        }
    }
    /// The save slots of the writable data folder `root`: `root/SAVEGAME`.
    pub fn at_save_root(root: &Path) -> Self {
        Self::new(None, Some(root.join("SAVEGAME")), None)
    }
    /// The settings folder `dir`.
    pub fn config_at(dir: &Path) -> Self {
        Self::new(Some(dir.to_path_buf()), None, None)
    }
    /// The folder `dir` that holds mod layers.
    pub fn mods_at(dir: &Path) -> Self {
        Self::new(None, None, Some(dir.to_path_buf()))
    }
    fn root(&self, namespace: Namespace) -> Result<&Path, StorageError> {
        let root = match namespace {
            Namespace::Config => &self.config,
            Namespace::Savegame => &self.savegame,
            Namespace::Mods => &self.mods,
        };
        root.as_deref()
            .ok_or_else(|| StorageError::Unavailable(format!("no {} folder", namespace.name())))
    }
    /// The file or folder a canonical path names below `root`, if any.
    fn resolve(
        root: &Path,
        canonical: &str,
    ) -> Result<Option<(PathBuf, fs::FileType)>, StorageError> {
        let mut at = None;
        let mut folder = root.to_path_buf();
        for part in canonical.split('/') {
            let Some((path, kind)) = lookup(&folder, part, canonical)? else {
                return Ok(None);
            };
            folder.clone_from(&path);
            at = Some((path, kind));
        }
        Ok(at)
    }
    /// Where a commit writes `spelling`: the stored spelling of whatever
    /// exists, and for the rest the caller's, one folder per canonical name
    /// within the batch.
    fn target(
        root: &Path,
        canonical: &str,
        spelling: &str,
        folders: &mut BTreeMap<String, PathBuf>,
    ) -> Result<PathBuf, StorageError> {
        let mut at = root.to_path_buf();
        let parts: Vec<&str> = canonical.split('/').collect();
        let spelled = spelling.split(['/', '\\']);
        for (index, (part, spelled)) in parts.iter().zip(spelled).enumerate() {
            let name = parts[..=index].join("/");
            if let Some(known) = folders.get(&name) {
                at.clone_from(known);
                continue;
            }
            at = match lookup(&at, part, canonical)? {
                Some((path, _)) => path,
                None => at.join(spelled),
            };
            if index + 1 < parts.len() {
                folders.insert(name, at.clone());
            }
        }
        Ok(at)
    }
    fn staging_fault(&self, index: usize) -> std::io::Result<()> {
        #[cfg(test)]
        if self.fail_staging == Some(index) {
            return Err(std::io::Error::other("injected fault"));
        }
        let _ = index;
        Ok(())
    }
    /// The keys below `folder`, whose key prefix is `base`, up to `depth`
    /// components deep. Only names the namespace accepts are entries.
    fn walk(
        namespace: Namespace,
        folder: &Path,
        base: &str,
        depth: usize,
        keys: &mut BTreeSet<String>,
    ) -> Result<(), StorageError> {
        let entries = match fs::read_dir(folder) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(io_failure(folder, error)),
        };
        let mut seen = BTreeSet::new();
        for entry in entries {
            let entry = entry.map_err(|error| io_failure(folder, error))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let folded = name.to_ascii_lowercase();
            // §3.9 rule 3: the settings folder is shared with other files,
            // and only its `*.toml` names concern this namespace.
            if namespace == Namespace::Config && !folded.ends_with(".toml") {
                continue;
            }
            let key = format!("{base}{folded}");
            let path = entry.path();
            let folder = checked_kind(&path)?.is_dir();
            let holds = if folder {
                depth > 1
            } else {
                canonical_key(namespace, &key).is_ok()
            };
            if !holds {
                continue;
            }
            if !seen.insert(folded) {
                return Err(StorageError::Collision { key });
            }
            if folder {
                Self::walk(namespace, &path, &format!("{key}/"), depth - 1, keys)?;
            } else {
                keys.insert(key);
            }
        }
        Ok(())
    }
}

impl StorageBackend for FsStorage {
    fn read(&self, namespace: Namespace, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let canonical = canonical_key(namespace, key)?;
        let root = self.root(namespace)?;
        let Some((path, kind)) = Self::resolve(root, &canonical)? else {
            return Ok(None);
        };
        if !kind.is_file() {
            return Ok(None);
        }
        let metadata = fs::metadata(&path).map_err(|error| io_failure(&path, error))?;
        if metadata.len() > MAX_ASSET_BYTES as u64 {
            return Err(StorageError::TooLarge { key: canonical });
        }
        fs::read(&path)
            .map(Some)
            .map_err(|error| io_failure(&path, error))
    }
    fn list(
        &self,
        namespace: Namespace,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, StorageError> {
        let root = self.root(namespace)?;
        // A folder's keys go one level per component the namespace allows.
        let depth = match namespace {
            Namespace::Config => 1,
            Namespace::Savegame => 2,
            Namespace::Mods => usize::MAX,
        };
        let mut keys = BTreeSet::new();
        match prefix.map(canonical_prefix).transpose()? {
            None => Self::walk(namespace, root, "", depth, &mut keys)?,
            Some(prefix) => {
                let used = prefix.split('/').count();
                if let Some((folder, kind)) = Self::resolve(root, &prefix)?
                    && kind.is_dir()
                    && used < depth
                {
                    let base = format!("{prefix}/");
                    Self::walk(namespace, &folder, &base, depth - used, &mut keys)?;
                }
            }
        }
        Ok(keys.into_iter().collect())
    }
    fn commit(&self, namespace: Namespace, ops: &[StorageOp]) -> Result<(), StorageError> {
        let net = net_effect(namespace, ops)?;
        let root = self.root(namespace)?;
        if namespace == Namespace::Mods {
            check_folders(&net, self.list(namespace, None)?)?;
        }
        // The settings folder is found, never made: a missing one tells the
        // shell to try the next (§3.10 rule 2).
        let puts = net.values().any(|op| op.bytes.is_some());
        if namespace == Namespace::Config && puts && !root.is_dir() {
            return Err(io_failure(root, "not a folder"));
        }
        let mut folders = BTreeMap::new();
        let mut targets = Vec::with_capacity(net.len());
        for (canonical, op) in &net {
            let target = Self::target(root, canonical, op.spelling, &mut folders)?;
            // A folder is no entry: deleting one is deleting an absent key,
            // and a put replaces one that deletes left empty (§3.13 rule 1).
            if target.is_dir() {
                if op.bytes.is_none() {
                    continue;
                }
                fs::remove_dir(&target).map_err(|error| io_failure(&target, error))?;
            }
            targets.push((target, op.bytes));
        }
        let ops: Vec<StagedOp> = targets
            .iter()
            .map(|(target, bytes)| match bytes {
                Some(bytes) => StagedOp::Put { target, bytes },
                None => StagedOp::Delete { target },
            })
            .collect();
        let names = match namespace {
            Namespace::Mods => StagingNames::Ordinary,
            Namespace::Config | Namespace::Savegame => StagingNames::Reserved,
        };
        staged::commit(&ops, names, |index| self.staging_fault(index))
            .map_err(|error| StorageError::Io(error.to_string()))
    }
    /// §3.9 rule 6: a commit's renames are its durability here.
    fn flush(&self) -> Result<(), StorageError> {
        Ok(())
    }
    /// §3.12 rule 4: the loose folder provider, as a folder layer has today.
    fn mount_layer(&self, layer: &str) -> Result<Arc<dyn Vfs>, StorageError> {
        let prefix = canonical_prefix(layer)?;
        let missing = || StorageError::LayerNotFound {
            layer: prefix.clone(),
        };
        let root = self.root(Namespace::Mods)?;
        let (folder, kind) = Self::resolve(root, &prefix)?.ok_or_else(missing)?;
        if !kind.is_dir() {
            return Err(missing());
        }
        let provider = LocalDirProvider::mount(&folder).map_err(vfs_failure)?;
        if provider.files().next().is_none() {
            return Err(missing());
        }
        Ok(Arc::new(provider))
    }
}

/// One record write of a host transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KvWrite {
    Put(String, Vec<u8>),
    Delete(String),
}

/// A failure the host store reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KvError {
    /// The host's own quota, which maps to [`StorageError::QuotaExceeded`].
    Quota,
    Failed(String),
}

/// §3.14 rule 2: what a browser or mobile host implements: a flat store with
/// a prefix scan and an atomic multi-record write. `write` may only queue a
/// transaction, in order; `flush` waits for the queue, and reports the first
/// transaction that failed.
pub trait KvStore: Send {
    fn scan(&self, prefix: &str) -> Result<Vec<(String, Vec<u8>)>, KvError>;
    fn write(&mut self, batch: Vec<KvWrite>) -> Result<(), KvError>;
    fn flush(&mut self) -> Result<(), KvError>;
}

#[derive(Debug)]
struct KvState<S> {
    store: S,
    entries: Entries,
    /// §3.13 rule 4: the first failed transaction. Commits stop until the
    /// store is reopened, so a later save is never reported as kept.
    failed: Option<String>,
}

/// §3.13: a [`KvStore`] behind the synchronous trait. Opening loads every
/// record into memory; reads and lists answer from there; each commit
/// applies to memory at once and becomes one host transaction, in order.
#[derive(Debug)]
pub struct KvStorage<S> {
    profile: GameProfile,
    state: Mutex<KvState<S>>,
    ignored: Vec<String>,
}

impl<S: KvStore> KvStorage<S> {
    /// Load the store for `profile`, whose scope holds the `savegame`
    /// namespace (§3.11 rule 5). Records of other shapes are ignored by
    /// listing and reported once; the journal is the adapter's own.
    pub fn open(store: S, profile: GameProfile) -> Result<Self, StorageError> {
        let unavailable = |error: KvError| StorageError::Unavailable(format!("{error:?}"));
        let root = format!("{RECORD_ROOT}/");
        let mut entries = Entries::default();
        let mut ignored = Vec::new();
        for (record, bytes) in store.scan(&root).map_err(unavailable)? {
            let rest = record.strip_prefix(&root).unwrap_or(&record);
            if rest.starts_with("journal") {
                continue;
            }
            match Self::entry_of(profile, rest) {
                Some(Some(key)) => {
                    entries.entries.insert(key, bytes);
                }
                // The other game's slots: well formed, and not this session's.
                Some(None) => {}
                None => ignored.push(record),
            }
        }
        for record in &ignored {
            tracing::warn!(record, "storage record ignored");
        }
        Ok(Self {
            profile,
            state: Mutex::new(KvState {
                store,
                entries,
                failed: None,
            }),
            ignored,
        })
    }
    /// A record's (namespace, key) after the layout root: `None` for a
    /// record of another shape, `Some(None)` for the other game's slot.
    fn entry_of(profile: GameProfile, record: &str) -> Option<Option<(Namespace, String)>> {
        let mut parts = record.splitn(3, '/');
        let (scope, space, key) = (parts.next()?, parts.next()?, parts.next()?);
        let namespace = [Namespace::Config, Namespace::Savegame, Namespace::Mods]
            .into_iter()
            .find(|namespace| namespace.name() == space)?;
        let canonical = canonical_key(namespace, key)
            .ok()
            .filter(|text| text == key)?;
        let own = match namespace {
            Namespace::Savegame => game_name(profile),
            _ => "shared",
        };
        if scope == own {
            return Some(Some((namespace, canonical)));
        }
        let other_game = namespace == Namespace::Savegame
            && crate::config::GAMES
                .iter()
                .any(|game| game_name(*game) == scope);
        other_game.then_some(None)
    }
    /// The records opening ignored.
    pub fn ignored(&self) -> &[String] {
        &self.ignored
    }
}

impl<S: KvStore> StorageBackend for KvStorage<S> {
    fn read(&self, namespace: Namespace, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        locked(&self.state).entries.read(namespace, key)
    }
    fn list(
        &self,
        namespace: Namespace,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, StorageError> {
        locked(&self.state).entries.list(namespace, prefix)
    }
    fn commit(&self, namespace: Namespace, ops: &[StorageOp]) -> Result<(), StorageError> {
        let mut state = locked(&self.state);
        if let Some(failure) = &state.failed {
            return Err(StorageError::Unavailable(failure.clone()));
        }
        let net = state.entries.prepare(namespace, ops)?;
        let batch = net
            .iter()
            .map(|(key, op)| {
                let record = record_key(self.profile, namespace, key);
                match op.bytes {
                    Some(bytes) => KvWrite::Put(record, bytes.to_vec()),
                    None => KvWrite::Delete(record),
                }
            })
            .collect();
        match state.store.write(batch) {
            Ok(()) => {}
            Err(KvError::Quota) => return Err(StorageError::QuotaExceeded),
            Err(KvError::Failed(message)) => {
                state.failed = Some(message.clone());
                return Err(StorageError::Unavailable(message));
            }
        }
        state.entries.apply(namespace, net);
        Ok(())
    }
    fn flush(&self) -> Result<(), StorageError> {
        let mut state = locked(&self.state);
        if let Some(failure) = &state.failed {
            return Err(StorageError::Unavailable(failure.clone()));
        }
        state.store.flush().map_err(|error| {
            let message = match error {
                KvError::Quota => "the host store's quota is used up".to_owned(),
                KvError::Failed(message) => message,
            };
            state.failed = Some(message.clone());
            StorageError::Unavailable(message)
        })
    }
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
