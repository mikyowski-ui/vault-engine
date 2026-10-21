//! SPEC-008 `.DAT` archives: byte-zero directory tables, raw/LZSS/block
//! payloads, and bounded decoding that rejects malformed streams.
//!
//! An archive opened from a file or other seekable stream reads only its
//! directory tables up front; each extraction seeks to one member and reads
//! just its stored bytes.
use crate::{MAX_ASSET_BYTES, ParseError, Vfs, VfsError, VfsPath};
use std::{
    collections::BTreeMap,
    fmt,
    fs::File,
    io::{self, BufReader, Read, Seek, SeekFrom},
    path::Path,
    sync::{Arc, Mutex, PoisonError},
};

/// Logical archive block size; every non-final block must produce exactly this.
pub const BLOCK_SIZE: usize = 16384;
const RING: usize = 4096;
/// Initial LZSS write cursor, `0xFEE`.
pub const RING_START: usize = 4078;
/// Entries reserved up front per directory table, whatever its count says.
const RESERVED_ENTRIES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Storage {
    /// `0x20`: logical bytes stored directly at the offset.
    Raw,
    /// `0x40`: two-byte headers selecting raw or LZSS block bodies.
    Blocks,
    /// `0x10` (and exact attribute zero): one LZSS stream of packed length.
    Legacy,
}
impl Storage {
    /// Exact zero aliases legacy `0x10`; otherwise bits `0xF0` select the mode.
    pub fn from_attributes(attributes: u32) -> Option<Self> {
        match if attributes == 0 {
            0x10
        } else {
            attributes & 0xf0
        } {
            0x10 => Some(Self::Legacy),
            0x20 => Some(Self::Raw),
            0x40 => Some(Self::Blocks),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatEntry {
    pub name: Vec<u8>,
    pub attributes: u32,
    pub offset: u32,
    pub length: u32,
    pub packed: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatTablePrefix {
    pub count: i32,
    pub capacity: i32,
    pub data_size: i32,
    /// Legacy word; not a usable file offset.
    pub opaque: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatDirectory {
    pub name: Vec<u8>,
    pub prefix: DatTablePrefix,
    pub files: Vec<DatEntry>,
}

/// A seekable byte source an archive streams its members from.
pub trait ArchiveSource: Read + Seek + Send {}
impl<T: Read + Seek + Send> ArchiveSource for T {}

/// Where member payloads come from.
#[derive(Clone)]
enum Payloads {
    /// The whole archive, already in memory.
    Memory(Arc<Vec<u8>>),
    /// A shared stream; extractions seek to one member at a time.
    Stream(Arc<Mutex<Box<dyn ArchiveSource>>>),
}
impl fmt::Debug for Payloads {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Memory(bytes) => write!(f, "Memory({} bytes)", bytes.len()),
            Self::Stream(_) => f.write_str("Stream"),
        }
    }
}

/// A parsed archive: root prefix, directories in root order, and the source
/// of its payloads (in-memory bytes or a lazily read stream).
#[derive(Debug, Clone)]
pub struct DatArchive {
    pub root: DatTablePrefix,
    pub directories: Vec<DatDirectory>,
    payloads: Payloads,
    index: BTreeMap<(Vec<u8>, Vec<u8>), (usize, usize)>,
}

/// The directory tables read from the archive start: whole archive bytes in
/// memory, or a file streamed without loading its payloads.
struct TableReader<R> {
    input: R,
    pos: usize,
}
impl<R: Read> TableReader<R> {
    fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError::new("DAT", self.pos, message)
    }
    fn truncated(&self, count: usize) -> ParseError {
        self.error(format!("truncated input: need {count} bytes"))
    }
    fn take(&mut self, count: usize) -> Result<Vec<u8>, ParseError> {
        let mut bytes = vec![0; count];
        self.input
            .read_exact(&mut bytes)
            .map_err(|_| self.truncated(count))?;
        self.pos += count;
        Ok(bytes)
    }
    /// Discard `count` bytes without buffering them.
    fn skip(&mut self, count: usize) -> Result<(), ParseError> {
        let skipped = io::copy(&mut (&mut self.input).take(count as u64), &mut io::sink())
            .map_err(|_| self.truncated(count))?;
        if skipped != count as u64 {
            return Err(self.truncated(count));
        }
        self.pos += count;
        Ok(())
    }
    fn word(&mut self) -> Result<u32, ParseError> {
        let mut bytes = [0; 4];
        self.input
            .read_exact(&mut bytes)
            .map_err(|_| self.truncated(4))?;
        self.pos += 4;
        Ok(u32::from_be_bytes(bytes))
    }
    fn name(&mut self) -> Result<Vec<u8>, ParseError> {
        let length = self.take(1)?[0] as usize;
        self.take(length)
    }
    fn prefix(&mut self) -> Result<DatTablePrefix, ParseError> {
        let count = self.word()? as i32;
        let capacity = self.word()? as i32;
        let data_size = self.word()? as i32;
        let opaque = self.word()?;
        if count < 0 || data_size < 0 {
            return Err(self.error("negative table count/associated size"));
        }
        if count as usize > 1_000_000 {
            return Err(self.error("table count exceeds resource limit"));
        }
        Ok(DatTablePrefix {
            count,
            capacity,
            data_size,
            opaque,
        })
    }
}

fn key(name: &[u8]) -> Vec<u8> {
    name.iter().map(u8::to_ascii_uppercase).collect()
}

/// An archive's directory tables, without its payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatTables {
    pub root: DatTablePrefix,
    pub directories: Vec<DatDirectory>,
}
impl DatTables {
    /// Read the root table and every file table from the archive start,
    /// consuming nothing past the last table.
    pub fn read(input: impl Read) -> Result<Self, ParseError> {
        let mut r = TableReader { input, pos: 0 };
        let root = r.prefix()?;
        // Table counts are untrusted: reserve at most a small fixed amount
        // and let the vectors grow with the entries actually read.
        let mut names = Vec::with_capacity((root.count as usize).min(RESERVED_ENTRIES));
        for _ in 0..root.count {
            names.push(r.name()?);
            r.skip(root.data_size as usize)?;
        }
        let mut directories = Vec::with_capacity(names.len());
        for name in names {
            let table = r.prefix()?;
            if (table.data_size as usize) < 16 {
                return Err(r.error("file table associated size is below 16"));
            }
            let mut files = Vec::with_capacity((table.count as usize).min(RESERVED_ENTRIES));
            for _ in 0..table.count {
                let file = r.name()?;
                let attributes = r.word()?;
                let offset = r.word()?;
                let size = r.word()?;
                let packed = r.word()?;
                r.skip(table.data_size as usize - 16)?;
                files.push(DatEntry {
                    name: file,
                    attributes,
                    offset,
                    length: size,
                    packed,
                });
            }
            directories.push(DatDirectory {
                name,
                prefix: table,
                files,
            });
        }
        Ok(Self { root, directories })
    }
}

impl DatDirectory {
    /// A member's stored path, `directory\\file` (root `.` files bare).
    pub fn path(&self, file: &DatEntry) -> Vec<u8> {
        if self.name == b"." {
            file.name.clone()
        } else {
            [self.name.as_slice(), b"\\", file.name.as_slice()].concat()
        }
    }
}

impl DatEntry {
    /// Bytes stored at `offset`: the logical length for raw members,
    /// otherwise the packed length.
    pub fn stored_len(&self) -> Result<usize, ParseError> {
        let storage = Storage::from_attributes(self.attributes).ok_or_else(|| {
            ParseError::new(
                "DAT",
                self.offset as usize,
                "unsupported attribute storage mode",
            )
        })?;
        Ok(if storage == Storage::Raw {
            self.length
        } else {
            self.packed
        } as usize)
    }
    /// Decode this member's exact logical bytes from its stored bytes (the
    /// `stored_len` bytes at its offset, or fewer if the archive ends).
    pub fn decode(&self, stored: &[u8]) -> Result<Vec<u8>, ParseError> {
        let start = self.offset as usize;
        let error = |message: &str| ParseError::new("DAT", start, message);
        let length = self.length as usize;
        if length > MAX_ASSET_BYTES {
            return Err(error("extracted file exceeds 64 MiB limit"));
        }
        let storage = Storage::from_attributes(self.attributes)
            .ok_or_else(|| error("unsupported attribute storage mode"))?;
        let body = stored
            .get(..self.stored_len()?)
            .ok_or_else(|| match storage {
                Storage::Raw => error("raw payload extends beyond the archive"),
                _ => error("packed payload extends beyond the archive"),
            })?;
        match storage {
            Storage::Raw => Ok(body.to_vec()),
            Storage::Legacy => {
                let mut ring = Ring::new();
                let output = ring.decode(body, length, false)?;
                if output.len() != length {
                    return Err(error("legacy stream length differs from the entry"));
                }
                Ok(output)
            }
            Storage::Blocks => decode_blocks(body, length).map_err(|mut failure| {
                failure.offset += start;
                failure
            }),
        }
    }
    /// Seek to this member in a streamed archive, read only its stored
    /// bytes, and decode them.
    pub fn read_from(&self, archive: &mut (impl Read + Seek)) -> Result<Vec<u8>, ParseError> {
        let start = self.offset as usize;
        let failed = |error: io::Error| ParseError::new("DAT", start, error.to_string());
        let stored = self.stored_len()?;
        if self.length as usize > MAX_ASSET_BYTES {
            return Err(ParseError::new(
                "DAT",
                start,
                "extracted file exceeds 64 MiB limit",
            ));
        }
        archive
            .seek(SeekFrom::Start(u64::from(self.offset)))
            .map_err(failed)?;
        // Growth follows the bytes actually present, so a bogus extent
        // cannot allocate more than the archive holds.
        let mut bytes = Vec::new();
        archive
            .take(stored as u64)
            .read_to_end(&mut bytes)
            .map_err(failed)?;
        self.decode(&bytes)
    }
}

impl DatArchive {
    /// Archives are not bounded by the 64 MiB per-asset limit, but each
    /// extracted file is.
    pub fn parse(bytes: Vec<u8>) -> Result<Self, ParseError> {
        let tables = DatTables::read(bytes.as_slice())?;
        Ok(Self::new(tables, Payloads::Memory(Arc::new(bytes))))
    }
    /// Open an archive file, reading only its directory tables. Members are
    /// read and decoded on demand by `extract`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ParseError> {
        let path = path.as_ref();
        let file = File::open(path)
            .map_err(|error| ParseError::new("DAT", 0, format!("{}: {error}", path.display())))?;
        Self::from_reader(file)
    }
    /// Read the directory tables from the start of a seekable stream and keep
    /// the stream for on-demand member reads; no payload is read here.
    pub fn from_reader(mut source: impl ArchiveSource + 'static) -> Result<Self, ParseError> {
        source
            .seek(SeekFrom::Start(0))
            .map_err(|error| ParseError::new("DAT", 0, error.to_string()))?;
        // Buffered, since the tables are read in one- to four-byte fields.
        let tables = DatTables::read(BufReader::new(&mut source))?;
        Ok(Self::new(
            tables,
            Payloads::Stream(Arc::new(Mutex::new(Box::new(source)))),
        ))
    }
    fn new(DatTables { root, directories }: DatTables, payloads: Payloads) -> Self {
        let mut index = BTreeMap::new();
        for (d, directory) in directories.iter().enumerate() {
            for (f, file) in directory.files.iter().enumerate() {
                index
                    .entry((key(&directory.name), key(&file.name)))
                    .or_insert((d, f));
            }
        }
        Self {
            root,
            directories,
            payloads,
            index,
        }
    }
    /// Whether members are read from a stream on demand rather than from
    /// archive bytes held in memory.
    pub fn is_streamed(&self) -> bool {
        matches!(self.payloads, Payloads::Stream(_))
    }
    /// Lookup: uppercase, split at the final backslash into directory and
    /// file keys, stripping a leading `.\\`/`.`; a path without a directory
    /// selects directory-table index zero (normally `.`).
    pub fn entry(&self, path: &[u8]) -> Option<&DatEntry> {
        let path = path.strip_prefix(b".\\").unwrap_or(path);
        let (directory, file) = match path.iter().rposition(|byte| *byte == b'\\') {
            Some(split) => (key(&path[..split]), key(&path[split + 1..])),
            None => (key(self.directories.first()?.name.as_slice()), key(path)),
        };
        let directory = if directory == b"." {
            key(&self.directories.first()?.name)
        } else {
            directory
        };
        let (d, f) = self.index.get(&(directory, file))?;
        self.directories.get(*d)?.files.get(*f)
    }
    /// Extract a file's exact logical bytes.
    pub fn extract(&self, entry: &DatEntry) -> Result<Vec<u8>, ParseError> {
        match &self.payloads {
            Payloads::Memory(bytes) => {
                entry.decode(bytes.get(entry.offset as usize..).unwrap_or_default())
            }
            // Every read seeks first, so a panic mid-read leaves no state
            // that matters and the poisoned lock is safe to reuse.
            Payloads::Stream(source) => {
                entry.read_from(&mut *source.lock().unwrap_or_else(PoisonError::into_inner))
            }
        }
    }
    /// Every stored path as `directory\\file` (root `.` files bare).
    pub fn paths(&self) -> impl Iterator<Item = Vec<u8>> + '_ {
        self.directories
            .iter()
            .flat_map(|directory| directory.files.iter().map(move |file| directory.path(file)))
    }
}

/// An LZSS ring: at each compressed block start positions 0-4077 refill with
/// spaces and, under the SPEC-008 §3.1 safe rule, the final eighteen with
/// zero (the reference's first-use state; later blocks retain prior bytes).
struct Ring {
    data: [u8; RING],
}
impl Ring {
    fn new() -> Self {
        Self { data: [0; RING] }
    }
    /// Decode an entire compressed input, bounding output to `limit`. With
    /// `truncate`, output past the bound is discarded and decoding stops
    /// (the final block of a `0x40` stream, SPEC-008 §2.3); otherwise
    /// expanding past the bound is an error.
    fn decode(
        &mut self,
        input: &[u8],
        limit: usize,
        truncate: bool,
    ) -> Result<Vec<u8>, ParseError> {
        self.data[..RING_START].fill(b' ');
        self.data[RING_START..].fill(0);
        let mut cursor = RING_START;
        let mut output = Vec::new();
        let mut position = 0;
        let error = |at: usize, message: &str| ParseError::new("LZSS", at, message);
        while position < input.len() {
            let control = input[position];
            position += 1;
            for bit in 0..8 {
                if position >= input.len() {
                    break;
                }
                if control & (1 << bit) != 0 {
                    let byte = input[position];
                    position += 1;
                    if output.len() == limit {
                        if truncate {
                            return Ok(output);
                        }
                        return Err(error(
                            position - 1,
                            "literal expands beyond the block bound",
                        ));
                    }
                    output.push(byte);
                    self.data[cursor] = byte;
                    cursor = (cursor + 1) % RING;
                } else {
                    let (Some(&a), Some(&b)) = (input.get(position), input.get(position + 1))
                    else {
                        return Err(error(position, "truncated match token"));
                    };
                    position += 2;
                    let start = usize::from(a) + 256 * usize::from(b >> 4);
                    let count = usize::from(b & 15) + 3;
                    if output.len() + count > limit && !truncate {
                        return Err(error(position - 2, "match expands beyond the block bound"));
                    }
                    for k in 0..count {
                        if output.len() == limit {
                            return Ok(output);
                        }
                        let byte = self.data[(start + k) % RING];
                        output.push(byte);
                        self.data[cursor] = byte;
                        cursor = (cursor + 1) % RING;
                    }
                }
            }
        }
        Ok(output)
    }
}

/// Decode a `0x40` block stream to exactly `length` bytes. Every non-final
/// block must produce exactly 16384 bytes; the final block is limited by
/// the remaining logical length (SPEC-008 §2.3), so output past it is
/// discarded as the reference's logical-length termination discards it,
/// while a final block that stops short still fails length validation. A
/// zero-length or nonprogressing block or a truncated body is rejected,
/// and bytes after the last needed block are not read.
pub fn decode_blocks(body: &[u8], length: usize) -> Result<Vec<u8>, ParseError> {
    let mut output = Vec::with_capacity(length.min(MAX_ASSET_BYTES));
    let mut position = 0;
    let mut ring = Ring::new();
    while output.len() < length {
        let header = body
            .get(position..position + 2)
            .ok_or_else(|| ParseError::new("DAT", position, "missing block header"))?;
        let header = u16::from_be_bytes([header[0], header[1]]);
        position += 2;
        let size = usize::from(header & 0x7fff);
        let block = body.get(position..position + size).ok_or_else(|| {
            ParseError::new("DAT", position, "block body exceeds the packed extent")
        })?;
        let remaining = length - output.len();
        let last = remaining <= BLOCK_SIZE;
        let expected = BLOCK_SIZE.min(remaining);
        let produced = if header & 0x8000 != 0 {
            block[..if last { size.min(expected) } else { size }].to_vec()
        } else {
            ring.decode(block, expected, last).map_err(|mut failure| {
                failure.offset += position;
                failure
            })?
        };
        if produced.len() != expected {
            return Err(ParseError::new(
                "DAT",
                position,
                format!(
                    "block produced {} bytes, expected {expected}",
                    produced.len()
                ),
            ));
        }
        output.extend_from_slice(&produced);
        position += size;
    }
    Ok(output)
}

/// One database: an optional loose root consulted first, then its archive.
/// There is no fallback to another database.
#[derive(Debug, Clone, Default)]
pub struct Database {
    pub loose: Option<std::sync::Arc<crate::LocalDirProvider>>,
    pub archive: Option<std::sync::Arc<DatArchive>>,
}
impl Database {
    pub fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        self.read_bounded(path, usize::MAX)
    }
    /// `read` that stops at `max_bytes` without reading the whole file:
    /// the loose root checks the file size before reading, and an archive
    /// member is rejected from its table entry's logical length before any
    /// payload is read.
    pub fn read_bounded(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, VfsError> {
        let normalized = VfsPath::new(path)?;
        if let Some(loose) = &self.loose {
            match loose.read_bounded(normalized.as_str(), max_bytes) {
                Ok(bytes) => return Ok(bytes),
                // SPEC-008 §3.2: only an absent loose file falls through to
                // the archive; a collision, link, oversize or I/O failure on
                // the loose root is reported, not masked by the archive copy.
                Err(VfsError::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }
        let archive = self
            .archive
            .as_ref()
            .ok_or_else(|| VfsError::NotFound(normalized.clone()))?;
        let request = normalized.as_str().replace('/', "\\");
        let entry = archive
            .entry(request.as_bytes())
            .ok_or_else(|| VfsError::NotFound(normalized.clone()))?;
        if entry.length as usize > max_bytes {
            return Err(VfsError::FileTooLarge {
                path: std::path::PathBuf::from(normalized.as_str()),
                max_bytes,
            });
        }
        archive
            .extract(entry)
            .map_err(|error| VfsError::Archive(error.to_string()))
    }
}

/// The master and critter databases. Normal opens use the selected (master)
/// database; critter-art opens select the critter database for that request.
#[derive(Debug, Clone, Default)]
pub struct DatProvider {
    pub master: Database,
    pub critter: Database,
    files: Vec<VfsPath>,
}
impl DatProvider {
    pub fn new(master: Database, critter: Database) -> Self {
        let mut names = std::collections::BTreeSet::new();
        for database in [&master, &critter] {
            if let Some(loose) = &database.loose {
                names.extend(loose.files().cloned());
            }
            if let Some(archive) = &database.archive {
                for path in archive.paths() {
                    if let Ok(path) =
                        VfsPath::new(&String::from_utf8_lossy(&path).replace('\\', "/"))
                    {
                        names.insert(path);
                    }
                }
            }
        }
        Self {
            master,
            critter,
            files: names.into_iter().collect(),
        }
    }
    /// A critter-art request: the critter database's loose root, then
    /// `CRITTER.DAT`, never the master archive.
    pub fn read_critter(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        self.critter.read(path)
    }
}
impl Vfs for DatProvider {
    /// Ordinary opens use the master database only. Critter art (paths under
    /// `art/critters/`) selects the critter database for that request.
    fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        self.read_bounded(path, usize::MAX)
    }
    fn read_bounded(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, VfsError> {
        let normalized = VfsPath::new(path)?;
        if normalized.as_str().starts_with("art/critters/") {
            self.critter.read_bounded(normalized.as_str(), max_bytes)
        } else {
            self.master.read_bounded(normalized.as_str(), max_bytes)
        }
    }
    fn files(&self) -> Box<dyn Iterator<Item = &VfsPath> + '_> {
        Box::new(self.files.iter())
    }
}

/// SPEC-008 §2.8 automap LZSS: the same tokens without an outer block header,
/// terminating on exactly `count` output bytes (the final match is truncated
/// to that count). The separately allocated dictionary fills only positions
/// 0-4077 with spaces; the remainder starts at zero here.
pub fn decode_until(input: &[u8], count: usize) -> Result<Vec<u8>, ParseError> {
    let mut data = [0_u8; RING];
    data[..RING_START].fill(b' ');
    let mut cursor = RING_START;
    let mut output = Vec::with_capacity(count);
    let mut position = 0;
    let error = |at: usize, message: &str| ParseError::new("LZSS", at, message);
    while output.len() < count {
        let control = *input
            .get(position)
            .ok_or_else(|| error(position, "stream ends before the required output"))?;
        position += 1;
        for bit in 0..8 {
            if output.len() == count {
                break;
            }
            if control & (1 << bit) != 0 {
                let byte = *input
                    .get(position)
                    .ok_or_else(|| error(position, "truncated literal"))?;
                position += 1;
                output.push(byte);
                data[cursor] = byte;
                cursor = (cursor + 1) % RING;
            } else {
                let (Some(&a), Some(&b)) = (input.get(position), input.get(position + 1)) else {
                    return Err(error(position, "truncated match token"));
                };
                position += 2;
                let start = usize::from(a) + 256 * usize::from(b >> 4);
                for k in 0..usize::from(b & 15) + 3 {
                    if output.len() == count {
                        break;
                    }
                    let byte = data[(start + k) % RING];
                    output.push(byte);
                    data[cursor] = byte;
                    cursor = (cursor + 1) % RING;
                }
            }
        }
    }
    Ok(output)
}
