//! SPEC-023 §2.1-§3.5: game data from byte buffers, with no OS path. A
//! [`ByteSource`] is one immutable shared buffer, a [`MemoryDir`] a loose
//! tree of them, and a [`MemoryMount`] assembles both into the provider
//! structure a file mount builds, so the same SPEC-008 and SPEC-015 rules
//! answer every request. [`classify_drop`] turns a dropped file list into a
//! mount.
use super::{LayeredVfs, ModLayer, ServedBy, Vfs, VfsError, VfsPath};
use crate::{
    Dat2Archive, Dat2Provider, DatArchive, DatProvider, Database, GameProfile, MAX_ASSET_BYTES,
    fallout2,
};
use std::{collections::BTreeMap, fmt, path::PathBuf, sync::Arc};

/// SPEC-023 §2.1: both archive formats address members with 32-bit offsets
/// and sizes.
const MAX_SOURCE_BYTES: u64 = u32::MAX as u64;

/// The members a Fallout 1 master database must hold, as mounted: the
/// palette, the campaign initializer and the new-game map (SPEC-014).
pub const FALLOUT1_MEMBERS: [&str; 3] = ["color.pal", "data/vault13.gam", "maps/v13ent.map"];

/// An immutable, shared byte buffer. Cloning and slicing share it.
#[derive(Clone)]
pub struct ByteSource {
    owner: Arc<dyn AsRef<[u8]> + Send + Sync>,
    start: usize,
    end: usize,
}

impl ByteSource {
    /// A source over any owner of immutable bytes (a vector, a `'static`
    /// slice, a memory map). A buffer over `2^32 - 1` bytes is refused.
    pub fn new(owner: impl AsRef<[u8]> + Send + Sync + 'static) -> Result<Self, VfsError> {
        let end = owner.as_ref().len();
        check_length(end as u64)?;
        Ok(Self {
            owner: Arc::new(owner),
            start: 0,
            end,
        })
    }
    pub fn as_slice(&self) -> &[u8] {
        let bytes: &[u8] = (*self.owner).as_ref();
        bytes.get(self.start..self.end).unwrap_or_default()
    }
    pub fn len(&self) -> usize {
        self.end - self.start
    }
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
    /// The `length` bytes at `offset`, sharing this buffer; `None` unless
    /// they lie wholly inside it. Done in `u64`, so that a 32-bit offset
    /// and size never wrap on a 32-bit target.
    pub fn slice(&self, offset: u64, length: u64) -> Option<Self> {
        let end = offset.checked_add(length)?;
        (end <= self.len() as u64).then(|| Self {
            owner: self.owner.clone(),
            start: self.start + offset as usize,
            end: self.start + end as usize,
        })
    }
}

impl fmt::Debug for ByteSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "ByteSource({} bytes)", self.len())
    }
}

fn check_length(length: u64) -> Result<(), VfsError> {
    if length > MAX_SOURCE_BYTES {
        return Err(VfsError::Archive(format!(
            "a byte source of {length} bytes is over the {MAX_SOURCE_BYTES}-byte limit"
        )));
    }
    Ok(())
}

/// SPEC-023 §3.3: a loose tree in memory. Unlike a loose folder it never
/// changes after it is built, so nothing resolves against a live directory.
#[derive(Debug, Clone, Default)]
pub struct MemoryDir {
    files: BTreeMap<VfsPath, ByteSource>,
}

impl MemoryDir {
    /// Build a tree from (path, bytes) pairs in any order. An invalid path,
    /// two spellings of one path, a path that is also another's folder and a
    /// value over 64 MiB each refuse the whole tree, naming the input.
    pub fn new<P: AsRef<str>>(
        entries: impl IntoIterator<Item = (P, ByteSource)>,
    ) -> Result<Self, VfsError> {
        let mut entries: Vec<(P, ByteSource)> = entries.into_iter().collect();
        // The input's order never decides which refusal is reported.
        entries.sort_by(|a, b| a.0.as_ref().cmp(b.0.as_ref()));
        let mut spelled: BTreeMap<VfsPath, String> = BTreeMap::new();
        let mut files = BTreeMap::new();
        for (spelling, bytes) in entries {
            let spelling = spelling.as_ref();
            let path = VfsPath::new(spelling)?;
            if bytes.len() > MAX_ASSET_BYTES {
                return Err(VfsError::FileTooLarge {
                    path: PathBuf::from(spelling),
                    max_bytes: MAX_ASSET_BYTES,
                });
            }
            if let Some(first) = spelled.insert(path.clone(), spelling.to_owned()) {
                return Err(VfsError::CaseCollision {
                    path,
                    first: first.into(),
                    second: spelling.into(),
                });
            }
            files.insert(path, bytes);
        }
        for (path, spelling) in &spelled {
            let path = path.as_str();
            for (at, _) in path.match_indices('/') {
                let folder = VfsPath::new(&path[..at])?;
                if let Some(file) = spelled.get(&folder) {
                    return Err(VfsError::FileIsFolder {
                        file: file.clone(),
                        inside: spelling.clone(),
                    });
                }
            }
        }
        Ok(Self { files })
    }
    fn holds(&self, path: &str) -> bool {
        VfsPath::new(path).is_ok_and(|path| self.files.contains_key(&path))
    }
}

impl Vfs for MemoryDir {
    fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        self.read_bounded(path, usize::MAX)
    }
    fn read_bounded(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, VfsError> {
        let path = VfsPath::new(path)?;
        let bytes = self
            .files
            .get(&path)
            .ok_or_else(|| VfsError::NotFound(path.clone()))?;
        if bytes.len() > max_bytes {
            return Err(VfsError::FileTooLarge {
                path: PathBuf::from(path.as_str()),
                max_bytes,
            });
        }
        Ok(bytes.as_slice().to_vec())
    }
    fn files(&self) -> Box<dyn Iterator<Item = &VfsPath> + '_> {
        Box::new(self.files.keys())
    }
}

/// SPEC-023 §2.3: one game's data from byte sources. With no archive at all
/// it is a loose corpus, the memory form of `--corpus-dir` alone.
#[derive(Debug, Clone, Default)]
pub struct MemoryMount {
    pub profile: GameProfile,
    pub master: Option<ByteSource>,
    pub critter: Option<ByteSource>,
    /// `patch000.dat`: Fallout 2 only.
    pub patch: Option<ByteSource>,
    /// The loose override tree, the role of `data_root`.
    pub loose: Option<MemoryDir>,
    /// Mod layers, lowest first, as a load order lists them.
    pub layers: Vec<(String, MemoryDir)>,
}

/// What a memory mount's game data is.
enum MemoryBase {
    Corpus(Arc<MemoryDir>),
    Dat(Arc<DatProvider>),
    Dat2(Arc<Dat2Provider>),
}

impl MemoryBase {
    fn vfs(&self) -> Arc<dyn Vfs> {
        match self {
            Self::Corpus(tree) => tree.clone(),
            Self::Dat(provider) => provider.clone(),
            Self::Dat2(provider) => provider.clone(),
        }
    }
    fn read_served(&self, path: &str, max_bytes: usize) -> Result<(Vec<u8>, ServedBy), VfsError> {
        match self {
            Self::Corpus(tree) => tree
                .read_bounded(path, max_bytes)
                .map(|bytes| (bytes, ServedBy::Loose)),
            Self::Dat(provider) => provider.read_served(path, max_bytes),
            Self::Dat2(provider) => provider.read_served(path, max_bytes),
        }
    }
    /// SPEC-018 §2.2: whether the master database as mounted holds `path`.
    fn in_master(&self, path: &str) -> bool {
        match self {
            Self::Corpus(tree) => tree.holds(path),
            Self::Dat(provider) => provider.master.holds(path),
            Self::Dat2(provider) => provider.in_master(path),
        }
    }
}

fn open_v1(name: &str, source: ByteSource) -> Result<Arc<DatArchive>, VfsError> {
    DatArchive::from_bytes(source)
        .map(Arc::new)
        .map_err(|error| VfsError::Archive(format!("{name}: {error} (SPEC-008 section 2)")))
}

fn open_v2(name: &str, source: ByteSource) -> Result<Arc<Dat2Archive>, VfsError> {
    Dat2Archive::from_bytes(source)
        .map(Arc::new)
        .map_err(|error| VfsError::Archive(format!("{name}: {error} (SPEC-015 section 2)")))
}

impl MemoryMount {
    /// SPEC-023 §3.2: assemble the mount. The profile alone selects the
    /// archive format; the other format is tried only to word a refused
    /// master database. Required members are a separate step
    /// ([`MemoryProvider::validate`]), as they are for a file mount.
    pub fn mount(self) -> Result<MemoryProvider, VfsError> {
        let Self {
            profile,
            master,
            critter,
            patch,
            loose,
            layers,
        } = self;
        let loose = loose.map(Arc::new);
        let base = match (master, critter) {
            (None, None) if patch.is_none() => MemoryBase::Corpus(loose.unwrap_or_default()),
            (Some(master), Some(critter)) => {
                let root = loose.map(|tree| tree as Arc<dyn Vfs>);
                match profile {
                    GameProfile::Fallout1 => {
                        if patch.is_some() {
                            return Err(VfsError::Archive(
                                "patch000.dat: Fallout 1 has no patch archive".into(),
                            ));
                        }
                        let opened = open_v1("master.dat", master.clone()).map_err(|error| {
                            if Dat2Archive::from_bytes(master).is_ok() {
                                VfsError::Archive(
                                    "master.dat: this is Fallout 2 data (a SPEC-015 archive; \
                                     set Game to fallout2)"
                                        .into(),
                                )
                            } else {
                                error
                            }
                        })?;
                        MemoryBase::Dat(Arc::new(DatProvider::new(
                            Database {
                                loose: root.clone(),
                                archive: Some(opened),
                            },
                            Database {
                                loose: root,
                                archive: Some(open_v1("critter.dat", critter)?),
                            },
                        )))
                    }
                    GameProfile::Fallout2 => {
                        let opened = open_v2("master.dat", master.clone()).map_err(|error| {
                            if DatArchive::from_bytes(master).is_ok() {
                                VfsError::Archive(
                                    "master.dat: this is Fallout 1 data (a SPEC-008 archive; \
                                     set Game to fallout1)"
                                        .into(),
                                )
                            } else {
                                error
                            }
                        })?;
                        let mut archives = vec![opened];
                        if let Some(patch) = patch {
                            archives.push(open_v2("patch000.dat", patch)?);
                        }
                        let critter = open_v2("critter.dat", critter)?;
                        MemoryBase::Dat2(Arc::new(Dat2Provider::new(root, archives, Some(critter))))
                    }
                }
            }
            (master, _) => {
                let missing = if master.is_none() {
                    "master.dat"
                } else {
                    "critter.dat"
                };
                return Err(VfsError::Archive(format!("{missing} is missing")));
            }
        };
        let layers = layers
            .into_iter()
            .map(|(name, tree)| ModLayer::from_tree(name, tree))
            .collect();
        Ok(MemoryProvider {
            profile,
            layered: LayeredVfs::new(base.vfs(), layers),
            base,
        })
    }
}

/// A mounted [`MemoryMount`]: the file mount's providers over byte sources.
pub struct MemoryProvider {
    profile: GameProfile,
    base: MemoryBase,
    layered: LayeredVfs,
}

impl MemoryProvider {
    /// A read with the part that served it. [`Vfs::read_with_layer`] names
    /// only mod layers, as it does for a file mount.
    pub fn read_served(
        &self,
        path: &str,
        max_bytes: usize,
    ) -> Result<(Vec<u8>, ServedBy), VfsError> {
        let path = VfsPath::new(path)?;
        match self.layered.read_layers(path.as_str(), max_bytes) {
            Some(found) => found.map(|(bytes, name)| (bytes, ServedBy::Layer(name))),
            None => self.base.read_served(path.as_str(), max_bytes),
        }
    }
    /// SPEC-023 §3.2 rule 6: the required members the master database, as
    /// mounted, lacks. Mod layers do not count, as for a file mount.
    pub fn missing_members(&self) -> Vec<&'static str> {
        let required: &[&'static str] = match self.profile {
            GameProfile::Fallout1 => &FALLOUT1_MEMBERS,
            GameProfile::Fallout2 => &fallout2::REQUIRED_MEMBERS,
        };
        required
            .iter()
            .copied()
            .filter(|member| !self.base.in_master(member))
            .collect()
    }
    /// Refuse a mount that lacks a required member, naming it.
    pub fn validate(&self) -> Result<(), VfsError> {
        match self.missing_members().first() {
            Some(member) => Err(VfsError::Archive(format!(
                "the game data is missing {member}"
            ))),
            None => Ok(()),
        }
    }
}

impl Vfs for MemoryProvider {
    fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        self.layered.read(path)
    }
    fn read_bounded(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, VfsError> {
        self.layered.read_bounded(path, max_bytes)
    }
    fn read_with_layer(
        &self,
        path: &str,
        max_bytes: usize,
    ) -> Result<(Vec<u8>, Option<String>), VfsError> {
        self.layered.read_with_layer(path, max_bytes)
    }
    fn files(&self) -> Box<dyn Iterator<Item = &VfsPath> + '_> {
        self.layered.files()
    }
    fn mod_layers(&self) -> usize {
        self.layered.mod_layers()
    }
    fn layer_manifests(&self) -> Result<Vec<(String, Vec<u8>)>, VfsError> {
        self.layered.layer_manifests()
    }
}

/// The three archive names of a drop.
const MASTER: &str = "master.dat";
const CRITTER: &str = "critter.dat";
const PATCH: &str = "patch000.dat";

/// A dropped entry that is not mounted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropIgnored {
    /// Its canonical path, the common folder removed.
    pub path: String,
    pub reason: &'static str,
}

/// SPEC-023 §3.5: what a drop held, the same for any order of its entries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DropReport {
    /// The common top folder removed, in canonical form.
    pub stripped: Option<String>,
    /// The archives mounted, in canonical order.
    pub archives: Vec<String>,
    /// How many entries form the loose tree.
    pub loose_files: usize,
    /// Every other entry, in canonical order.
    pub ignored: Vec<DropIgnored>,
}

/// A refused drop.
#[derive(Debug, thiserror::Error)]
pub enum DropError {
    #[error("invalid path {0}")]
    InvalidPath(String),
    #[error("two entries match {name} ({first}, {second})")]
    TwoEntries {
        name: &'static str,
        first: String,
        second: String,
    },
    #[error("{0} is missing")]
    Missing(&'static str),
    #[error(transparent)]
    Tree(#[from] VfsError),
}

/// `path` without its first component, in either slash style.
fn without_folder(path: &str) -> &str {
    path.split_once(['/', '\\']).map_or(path, |(_, rest)| rest)
}

/// One dropped entry as its path is cut down, both spellings in step.
struct Dropped {
    canonical: String,
    spelling: String,
    bytes: ByteSource,
}

impl Dropped {
    fn strip(&mut self) {
        self.canonical = without_folder(&self.canonical).to_owned();
        self.spelling = without_folder(&self.spelling).to_owned();
    }
}

/// SPEC-023 §3.5: the mount a dropped file list describes, and its report.
/// Refused drops name the entry and the rule; archives are opened and
/// required members checked by [`MemoryMount::mount`] and
/// [`MemoryProvider::validate`], the last step of the rules.
pub fn classify_drop<P: AsRef<str>>(
    profile: GameProfile,
    entries: impl IntoIterator<Item = (P, ByteSource)>,
) -> Result<(MemoryMount, DropReport), DropError> {
    let mut entries: Vec<(P, ByteSource)> = entries.into_iter().collect();
    entries.sort_by(|a, b| a.0.as_ref().cmp(b.0.as_ref()));
    let mut dropped = Vec::with_capacity(entries.len());
    for (spelling, bytes) in entries {
        let spelling = spelling.as_ref().to_owned();
        let Ok(path) = VfsPath::new(&spelling) else {
            return Err(DropError::InvalidPath(spelling));
        };
        dropped.push(Dropped {
            canonical: path.as_str().to_owned(),
            spelling,
            bytes,
        });
    }

    // Dropping the game folder itself.
    let folder = |entry: &Dropped| {
        entry
            .canonical
            .split_once('/')
            .map(|(head, _)| head.to_owned())
    };
    let stripped = dropped
        .first()
        .and_then(folder)
        .filter(|head| dropped.iter().all(|e| folder(e).as_ref() == Some(head)));
    if stripped.is_some() {
        dropped.iter_mut().for_each(Dropped::strip);
    }

    let mut master: Option<Dropped> = None;
    let mut critter: Option<Dropped> = None;
    let mut patch: Option<Dropped> = None;
    let mut others = Vec::new();
    for entry in dropped {
        let (name, slot) = match entry.canonical.as_str() {
            MASTER => (MASTER, &mut master),
            CRITTER => (CRITTER, &mut critter),
            PATCH => (PATCH, &mut patch),
            _ => {
                others.push(entry);
                continue;
            }
        };
        if let Some(first) = slot {
            return Err(DropError::TwoEntries {
                name,
                first: first.spelling.clone(),
                second: entry.spelling,
            });
        }
        *slot = Some(entry);
    }

    let archive_found = master.is_some() || critter.is_some() || patch.is_some();
    let mut loose = Vec::new();
    let mut ignored = Vec::new();
    for mut entry in others {
        if !archive_found {
            loose.push((entry.spelling, entry.bytes));
            continue;
        }
        let in_data = entry.canonical.strip_prefix("data/");
        if in_data.is_some_and(|rest| !rest.starts_with("savegame/")) {
            entry.strip();
            loose.push((entry.spelling, entry.bytes));
        } else {
            ignored.push(DropIgnored {
                path: entry.canonical,
                reason: "not game data",
            });
        }
    }
    if archive_found && master.is_none() {
        return Err(DropError::Missing(MASTER));
    }
    if archive_found && critter.is_none() {
        return Err(DropError::Missing(CRITTER));
    }
    if profile == GameProfile::Fallout1
        && let Some(entry) = patch.take()
    {
        ignored.push(DropIgnored {
            path: entry.canonical,
            reason: "not mounted under fallout1",
        });
    }
    ignored.sort_by(|a, b| a.path.cmp(&b.path));

    let mut archives: Vec<String> = [&critter, &master, &patch]
        .into_iter()
        .flatten()
        .map(|entry| entry.canonical.clone())
        .collect();
    archives.sort();
    let report = DropReport {
        stripped,
        archives,
        loose_files: loose.len(),
        ignored,
    };
    let loose = (!loose.is_empty())
        .then(|| MemoryDir::new(loose))
        .transpose()?;
    let bytes = |entry: Option<Dropped>| entry.map(|entry| entry.bytes);
    Ok((
        MemoryMount {
            profile,
            master: bytes(master),
            critter: bytes(critter),
            patch: bytes(patch),
            loose,
            layers: Vec::new(),
        },
        report,
    ))
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;
