//! SPEC-008 `.DAT` archives: byte-zero directory tables, raw/LZSS/block
//! payloads, and bounded decoding that rejects malformed streams.
use crate::{MAX_ASSET_BYTES, ParseError, Vfs, VfsError, VfsPath, binary::Reader};
use std::collections::BTreeMap;

/// Logical archive block size; every non-final block must produce exactly this.
pub const BLOCK_SIZE: usize = 16384;
const RING: usize = 4096;
/// Initial LZSS write cursor, `0xFEE`.
pub const RING_START: usize = 4078;

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

/// A parsed archive: root prefix, directories in root order, and the bytes.
#[derive(Debug, Clone)]
pub struct DatArchive {
    pub root: DatTablePrefix,
    pub directories: Vec<DatDirectory>,
    bytes: std::sync::Arc<Vec<u8>>,
    index: BTreeMap<(Vec<u8>, Vec<u8>), (usize, usize)>,
}

fn prefix(r: &mut Reader<'_>) -> Result<DatTablePrefix, ParseError> {
    let count = r.be_i32()?;
    let capacity = r.be_i32()?;
    let data_size = r.be_i32()?;
    let opaque = r.word()?;
    if count < 0 || data_size < 0 {
        return Err(r.error("negative table count/associated size"));
    }
    if count as usize > 1_000_000 {
        return Err(r.error("table count exceeds resource limit"));
    }
    Ok(DatTablePrefix {
        count,
        capacity,
        data_size,
        opaque,
    })
}

fn key(name: &[u8]) -> Vec<u8> {
    name.iter().map(u8::to_ascii_uppercase).collect()
}

impl DatArchive {
    /// Archives are not bounded by the 64 MiB per-asset limit, but each
    /// extracted file is.
    pub fn parse(bytes: Vec<u8>) -> Result<Self, ParseError> {
        let mut r = Reader::unbounded(&bytes, "DAT");
        let root = prefix(&mut r)?;
        let mut names = Vec::with_capacity(root.count as usize);
        for _ in 0..root.count {
            let length = r.take(1)?[0] as usize;
            names.push(r.take(length)?.to_vec());
            r.take(root.data_size as usize)?;
        }
        let mut directories = Vec::with_capacity(names.len());
        let mut index = BTreeMap::new();
        for name in names {
            let table = prefix(&mut r)?;
            if (table.data_size as usize) < 16 {
                return Err(r.error("file table associated size is below 16"));
            }
            let mut files = Vec::with_capacity(table.count as usize);
            for _ in 0..table.count {
                let length = r.take(1)?[0] as usize;
                let file = r.take(length)?.to_vec();
                let attributes = r.word()?;
                let offset = r.word()?;
                let size = r.word()?;
                let packed = r.word()?;
                r.take(table.data_size as usize - 16)?;
                index
                    .entry((key(&name), key(&file)))
                    .or_insert((directories.len(), files.len()));
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
        Ok(Self {
            root,
            directories,
            bytes: std::sync::Arc::new(bytes),
            index,
        })
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
        let error = |message: &str| ParseError::new("DAT", entry.offset as usize, message);
        let length = entry.length as usize;
        if length > MAX_ASSET_BYTES {
            return Err(error("extracted file exceeds 64 MiB limit"));
        }
        let start = entry.offset as usize;
        let storage = Storage::from_attributes(entry.attributes)
            .ok_or_else(|| error("unsupported attribute storage mode"))?;
        match storage {
            Storage::Raw => self
                .bytes
                .get(
                    start
                        ..start
                            .checked_add(length)
                            .ok_or_else(|| error("offset overflow"))?,
                )
                .map(<[u8]>::to_vec)
                .ok_or_else(|| error("raw payload extends beyond the archive")),
            Storage::Legacy => {
                let body = self.packed(entry)?;
                let mut ring = Ring::new();
                let output = ring.decode(body, length)?;
                if output.len() != length {
                    return Err(error("legacy stream length differs from the entry"));
                }
                Ok(output)
            }
            Storage::Blocks => {
                let body = self.packed(entry)?;
                decode_blocks(body, length).map_err(|mut failure| {
                    failure.offset += start;
                    failure
                })
            }
        }
    }
    fn packed(&self, entry: &DatEntry) -> Result<&[u8], ParseError> {
        let start = entry.offset as usize;
        let end = start
            .checked_add(entry.packed as usize)
            .ok_or_else(|| ParseError::new("DAT", start, "packed extent overflow"))?;
        self.bytes.get(start..end).ok_or_else(|| {
            ParseError::new("DAT", start, "packed payload extends beyond the archive")
        })
    }
    /// Every stored path as `directory\\file` (root `.` files bare).
    pub fn paths(&self) -> impl Iterator<Item = Vec<u8>> + '_ {
        self.directories.iter().flat_map(|directory| {
            directory.files.iter().map(move |file| {
                if directory.name == b"." {
                    file.name.clone()
                } else {
                    [directory.name.as_slice(), b"\\", file.name.as_slice()].concat()
                }
            })
        })
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
    /// Decode an entire compressed input, bounding output to `limit`.
    fn decode(&mut self, input: &[u8], limit: usize) -> Result<Vec<u8>, ParseError> {
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
                    if output.len() + count > limit {
                        return Err(error(position - 2, "match expands beyond the block bound"));
                    }
                    for k in 0..count {
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

/// Decode a `0x40` block stream to exactly `length` bytes. Each block must
/// produce `min(16384, remaining)` bytes; a zero-length or nonprogressing
/// block or a truncated body is rejected, and bytes after the last needed
/// block are not read.
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
        let expected = BLOCK_SIZE.min(length - output.len());
        let produced = if header & 0x8000 != 0 {
            block.to_vec()
        } else {
            ring.decode(block, expected).map_err(|mut failure| {
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
        let normalized = VfsPath::new(path)?;
        if let Some(loose) = &self.loose
            && let Ok(bytes) = loose.read(normalized.as_str())
        {
            return Ok(bytes);
        }
        let archive = self
            .archive
            .as_ref()
            .ok_or_else(|| VfsError::NotFound(normalized.clone()))?;
        let request = normalized.as_str().replace('/', "\\");
        let entry = archive
            .entry(request.as_bytes())
            .ok_or_else(|| VfsError::NotFound(normalized.clone()))?;
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
        let normalized = VfsPath::new(path)?;
        if normalized.as_str().starts_with("art/critters/") {
            self.critter.read(normalized.as_str())
        } else {
            self.master.read(normalized.as_str())
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
