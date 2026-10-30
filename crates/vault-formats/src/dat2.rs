//! SPEC-015: the Fallout 2 archive (version 2). Little-endian; member data
//! first, then a directory tree, then an 8-byte footer (`tree_size`,
//! `file_size`). Members are stored or zlib-compressed ([`crate::inflate`]).
//! The archive is a read-only [`Vfs`] provider over canonical paths.
//!
//! SPEC-008's version 1 archive ([`crate::DatArchive`]) is unchanged; the
//! game profile selects which format a data file is read as.
use crate::{
    ByteSource, MAX_ASSET_BYTES, ParseError, ServedBy,
    dat::ArchiveSource,
    inflate::zlib_decompress,
    vfs::{Vfs, VfsError, VfsPath},
};
use std::{
    collections::BTreeMap,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
};

/// SPEC-015 §4: the longest stored path.
pub const MAX_PATH: usize = 1024;
/// The smallest entry: a 4-byte length, a 1-byte path, the flags and three
/// 4-byte fields.
const MIN_ENTRY: u64 = 17;
/// SPEC-015 §3: the largest directory tree read.
pub const MAX_TREE: u64 = MAX_ASSET_BYTES as u64;

fn error(offset: u64, message: impl Into<String>) -> ParseError {
    ParseError::new("DAT2", offset as usize, message)
}

/// One member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dat2Entry {
    /// The canonical path (lowercase, `/`-separated).
    pub path: VfsPath,
    /// The path as stored.
    pub stored_name: Vec<u8>,
    pub compressed: bool,
    pub real_size: u32,
    pub packed_size: u32,
    pub offset: u32,
}

/// Where member bytes come from.
#[derive(Clone)]
enum Source {
    Memory(ByteSource),
    Stream(Arc<Mutex<Box<dyn ArchiveSource>>>),
}

impl std::fmt::Debug for Source {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Memory(bytes) => write!(formatter, "Memory({} bytes)", bytes.len()),
            Self::Stream(_) => formatter.write_str("Stream"),
        }
    }
}

/// A parsed version 2 archive.
#[derive(Debug, Clone)]
pub struct Dat2Archive {
    /// Every member, in canonical path order.
    entries: Vec<Dat2Entry>,
    paths: Vec<VfsPath>,
    index: BTreeMap<VfsPath, usize>,
    source: Source,
}

impl Dat2Archive {
    /// An archive held in memory.
    pub fn parse(bytes: Vec<u8>) -> Result<Self, ParseError> {
        let source = ByteSource::new(bytes).map_err(|failure| error(0, failure.to_string()))?;
        Self::from_bytes(source)
    }
    /// SPEC-023 §3.1: an archive over a byte source, reading only its
    /// footer and tree. Refusals and members match a file of the same
    /// bytes; the source's length is the file's.
    pub fn from_bytes(source: ByteSource) -> Result<Self, ParseError> {
        let bytes = source.as_slice();
        let length = bytes.len() as u64;
        let footer = bytes
            .get(bytes.len().saturating_sub(8)..)
            .filter(|footer| footer.len() == 8)
            .ok_or_else(|| error(0, "shorter than its 8-byte footer"))?;
        let (start, size) = Self::footer(footer, length)?;
        let tree = source
            .slice(start, size)
            .ok_or_else(|| error(start, "the tree ends early"))?;
        let entries = Self::tree(tree.as_slice(), start)?;
        Self::new(entries, Source::Memory(source))
    }
    /// Open an archive file, reading only its footer and tree; members are
    /// read on demand.
    #[cfg(feature = "host-fs")]
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self, ParseError> {
        let path = path.as_ref();
        let file = std::fs::File::open(path)
            .map_err(|failure| error(0, format!("{}: {failure}", path.display())))?;
        Self::from_reader(file)
    }
    /// Read the footer and tree of a seekable stream and keep the stream
    /// for member reads.
    pub fn from_reader(mut source: impl ArchiveSource + 'static) -> Result<Self, ParseError> {
        let io = |failure: std::io::Error| error(0, failure.to_string());
        let length = source.seek(SeekFrom::End(0)).map_err(io)?;
        if length < 8 {
            return Err(error(0, "shorter than its 8-byte footer"));
        }
        source.seek(SeekFrom::Start(length - 8)).map_err(io)?;
        let mut footer = [0u8; 8];
        source.read_exact(&mut footer).map_err(io)?;
        let (start, size) = Self::footer(&footer, length)?;
        source.seek(SeekFrom::Start(start)).map_err(io)?;
        let mut tree = Vec::new();
        source
            .by_ref()
            .take(size)
            .read_to_end(&mut tree)
            .map_err(io)?;
        if tree.len() as u64 != size {
            return Err(error(start, "the tree ends early"));
        }
        let entries = Self::tree(&tree, start)?;
        Self::new(
            entries,
            Source::Stream(Arc::new(Mutex::new(Box::new(source)))),
        )
    }
    /// SPEC-015 §2: the tree's start and size from the footer.
    fn footer(footer: &[u8], length: u64) -> Result<(u64, u64), ParseError> {
        let word =
            |at: usize| u64::from(u32::from_le_bytes(footer[at..at + 4].try_into().unwrap()));
        let (tree_size, file_size) = (word(0), word(4));
        let at = length - 8;
        if file_size != length {
            return Err(error(
                at,
                format!("the footer's file size {file_size} is not the length {length}"),
            ));
        }
        if tree_size + 8 > file_size {
            return Err(error(
                at,
                format!("a tree of {tree_size} bytes does not fit"),
            ));
        }
        if tree_size > MAX_TREE {
            return Err(error(
                at,
                format!("a tree of {tree_size} bytes is over the limit"),
            ));
        }
        Ok((file_size - 8 - tree_size, tree_size))
    }
    /// SPEC-015 §3-§4: the entries of a tree that starts at `start` (the
    /// end of the member data).
    fn tree(tree: &[u8], start: u64) -> Result<Vec<Dat2Entry>, ParseError> {
        let mut at = 0usize;
        let here = |at: usize| start + at as u64;
        let take = |at: &mut usize, count: usize| -> Result<&[u8], ParseError> {
            let bytes = tree
                .get(*at..*at + count)
                .ok_or_else(|| error(here(*at), "the tree ends early"))?;
            *at += count;
            Ok(bytes)
        };
        let word = |bytes: &[u8]| u32::from_le_bytes(bytes.try_into().unwrap());
        let count = u64::from(word(take(&mut at, 4)?));
        if count * MIN_ENTRY + 4 > tree.len() as u64 {
            return Err(error(start, format!("{count} entries cannot fit the tree")));
        }
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let entry_at = here(at);
            let length = word(take(&mut at, 4)?) as usize;
            if !(1..=MAX_PATH).contains(&length) {
                return Err(error(entry_at, format!("a path of {length} bytes")));
            }
            let name = take(&mut at, length)?.to_vec();
            if !name.iter().all(|byte| (0x20..=0x7e).contains(byte)) {
                return Err(error(entry_at, "a path that is not printable ASCII"));
            }
            let flags = take(&mut at, 1)?[0];
            let compressed = match flags {
                0 => false,
                1 => true,
                other => return Err(error(entry_at, format!("flags {other}"))),
            };
            let real_size = word(take(&mut at, 4)?);
            let packed_size = word(take(&mut at, 4)?);
            let offset = word(take(&mut at, 4)?);
            let text = String::from_utf8_lossy(&name);
            if u64::from(offset) + u64::from(packed_size) > start {
                return Err(error(entry_at, format!("{text} lies past the member data")));
            }
            if !compressed && real_size != packed_size {
                return Err(error(
                    entry_at,
                    format!("{text} is stored with two different sizes"),
                ));
            }
            let path = VfsPath::new(&text.replace('\\', "/"))
                .map_err(|failure| error(entry_at, format!("{text}: {failure}")))?;
            entries.push(Dat2Entry {
                path,
                stored_name: name,
                compressed,
                real_size,
                packed_size,
                offset,
            });
        }
        if at != tree.len() {
            return Err(error(here(at), "bytes after the last entry"));
        }
        Ok(entries)
    }
    fn new(mut entries: Vec<Dat2Entry>, source: Source) -> Result<Self, ParseError> {
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        if let Some(pair) = entries.windows(2).find(|pair| pair[0].path == pair[1].path) {
            return Err(error(
                u64::from(pair[1].offset),
                format!(
                    "{} and {} are the same path",
                    String::from_utf8_lossy(&pair[0].stored_name),
                    String::from_utf8_lossy(&pair[1].stored_name)
                ),
            ));
        }
        let paths: Vec<VfsPath> = entries.iter().map(|entry| entry.path.clone()).collect();
        let index = paths
            .iter()
            .enumerate()
            .map(|(at, path)| (path.clone(), at))
            .collect();
        Ok(Self {
            entries,
            paths,
            index,
            source,
        })
    }
    /// Every member, in canonical path order.
    pub fn entries(&self) -> &[Dat2Entry] {
        &self.entries
    }
    /// The member at a path in either slash style and any case.
    pub fn entry(&self, path: &str) -> Option<&Dat2Entry> {
        let path = VfsPath::new(&path.replace('\\', "/")).ok()?;
        self.index.get(&path).map(|at| &self.entries[*at])
    }
    /// SPEC-015 §5: a member's logical bytes.
    pub fn extract(&self, entry: &Dat2Entry) -> Result<Vec<u8>, ParseError> {
        let at = u64::from(entry.offset);
        if entry.real_size as usize > MAX_ASSET_BYTES {
            return Err(error(at, "the member is over the 64 MiB limit"));
        }
        let size = entry.packed_size as usize;
        let stored = match &self.source {
            Source::Memory(bytes) => bytes
                .slice(at, u64::from(entry.packed_size))
                .ok_or_else(|| error(at, "the member lies past the archive"))?
                .as_slice()
                .to_vec(),
            // Every read seeks first, so a poisoned lock holds no state that
            // matters.
            Source::Stream(source) => {
                let mut source = source.lock().unwrap_or_else(PoisonError::into_inner);
                let io = |failure: std::io::Error| error(at, failure.to_string());
                source.seek(SeekFrom::Start(at)).map_err(io)?;
                let mut bytes = Vec::new();
                source
                    .by_ref()
                    .take(size as u64)
                    .read_to_end(&mut bytes)
                    .map_err(io)?;
                if bytes.len() != size {
                    return Err(error(at, "the member lies past the archive"));
                }
                bytes
            }
        };
        if !entry.compressed {
            return Ok(stored);
        }
        zlib_decompress(&stored, entry.real_size as usize).map_err(|failure| {
            error(
                at,
                format!("{}: {failure}", String::from_utf8_lossy(&entry.stored_name)),
            )
        })
    }
}

impl Vfs for Dat2Archive {
    fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        self.read_bounded(path, usize::MAX)
    }
    fn read_bounded(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, VfsError> {
        let normalized = VfsPath::new(path)?;
        let entry = self
            .index
            .get(&normalized)
            .map(|at| &self.entries[*at])
            .ok_or_else(|| VfsError::NotFound(normalized.clone()))?;
        if entry.real_size as usize > max_bytes {
            return Err(VfsError::FileTooLarge {
                path: PathBuf::from(normalized.as_str()),
                max_bytes,
            });
        }
        self.extract(entry)
            .map_err(|failure| VfsError::Archive(failure.to_string()))
    }
    fn files(&self) -> Box<dyn Iterator<Item = &VfsPath> + '_> {
        Box::new(self.paths.iter())
    }
}

/// SPEC-018 §1: a Fallout 2 install's two databases over SPEC-015
/// archives, in SPEC-008 §3.2's model. The loose data folder is read first.
/// `art/critters/` requests then read the critter archive alone; every other
/// request reads the master layers, `patch000.dat` above `MASTER.DAT`. Only
/// an absent file falls through to the next source.
#[derive(Debug, Clone)]
pub struct Dat2Provider {
    loose: Option<Arc<dyn Vfs>>,
    /// Highest first: `patch000.dat`, then `MASTER.DAT`.
    master: Vec<Arc<Dat2Archive>>,
    critter: Option<Arc<Dat2Archive>>,
    files: Vec<VfsPath>,
}

impl Dat2Provider {
    /// The databases: `master` lowest first (`MASTER.DAT`, then an optional
    /// `patch000.dat` above it).
    pub fn new(
        loose: Option<Arc<dyn Vfs>>,
        master: Vec<Arc<Dat2Archive>>,
        critter: Option<Arc<Dat2Archive>>,
    ) -> Self {
        let mut files = std::collections::BTreeSet::new();
        if let Some(loose) = &loose {
            files.extend(loose.files().cloned());
        }
        for archive in master.iter().chain(&critter) {
            files.extend(archive.paths.iter().cloned());
        }
        Self {
            loose,
            master: master.into_iter().rev().collect(),
            critter,
            files: files.into_iter().collect(),
        }
    }
    /// SPEC-018 §2.2: whether the master database as mounted (the loose
    /// folder, then the master layers) holds `path`; the critter archive
    /// does not count.
    pub fn in_master(&self, path: &str) -> bool {
        let Ok(path) = VfsPath::new(path) else {
            return false;
        };
        self.loose
            .as_ref()
            .is_some_and(|loose| loose.files().any(|file| *file == path))
            || self
                .master
                .iter()
                .any(|archive| archive.index.contains_key(&path))
    }
    /// SPEC-018 §1: `patch000.dat` beside `master`, its name compared
    /// case-insensitively.
    #[cfg(feature = "host-fs")]
    pub fn find_patch(master: &std::path::Path) -> Option<PathBuf> {
        let dir = master.parent().filter(|dir| !dir.as_os_str().is_empty());
        let dir = dir.unwrap_or(std::path::Path::new("."));
        std::fs::read_dir(dir)
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.is_file()
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.eq_ignore_ascii_case("patch000.dat"))
            })
    }
    /// A read with the part that served it (SPEC-023 §3.2 rule 5).
    pub(crate) fn read_served(
        &self,
        path: &str,
        max_bytes: usize,
    ) -> Result<(Vec<u8>, ServedBy), VfsError> {
        let normalized = VfsPath::new(path)?;
        if let Some(loose) = &self.loose {
            match loose.read_bounded(normalized.as_str(), max_bytes) {
                Err(VfsError::NotFound(_)) => {}
                result => return result.map(|bytes| (bytes, ServedBy::Loose)),
            }
        }
        let critter = normalized.as_str().starts_with("art/critters/");
        let sources: Vec<&Arc<Dat2Archive>> = if critter {
            self.critter.iter().collect()
        } else {
            self.master.iter().collect()
        };
        let patched = self.master.len() > 1;
        for (index, archive) in sources.into_iter().enumerate() {
            match archive.read_bounded(normalized.as_str(), max_bytes) {
                Err(VfsError::NotFound(_)) => {}
                result => {
                    let part = match (critter, patched && index == 0) {
                        (true, _) => ServedBy::Critter,
                        (false, true) => ServedBy::Patch,
                        (false, false) => ServedBy::Master,
                    };
                    return result.map(|bytes| (bytes, part));
                }
            }
        }
        Err(VfsError::NotFound(normalized))
    }
}

impl Vfs for Dat2Provider {
    fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        self.read_bounded(path, usize::MAX)
    }
    fn read_bounded(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, VfsError> {
        self.read_served(path, max_bytes).map(|(bytes, _)| bytes)
    }
    fn files(&self) -> Box<dyn Iterator<Item = &VfsPath> + '_> {
        Box::new(self.files.iter())
    }
}

#[cfg(all(test, feature = "host-fs"))]
#[path = "dat2_tests.rs"]
mod tests;
