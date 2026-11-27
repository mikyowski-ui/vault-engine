//! Pre-rendered map backdrops: the `.bkd` index and `.bkc` chunk files
//! (`docs/prerendered-backdrop-cutaway.md` sections 2-4).
//!
//! All multi-byte numbers are big-endian. The module is portable (no host
//! access) and every size is validated before anything is allocated.
use crate::{
    ParseError,
    binary::Reader,
    inflate::{adler32, zlib_decompress},
};

/// `.bkd` magic, `VBKD`.
pub const INDEX_MAGIC: [u8; 4] = *b"VBKD";
/// `.bkc` magic, `VBKC`.
pub const CHUNK_MAGIC: [u8; 4] = *b"VBKC";
pub const VERSION: u16 = 1;
/// The `.bkd` header length.
pub const INDEX_HEADER_LEN: usize = 24;
/// One elevation record.
pub const ELEVATION_RECORD_LEN: usize = 32;
/// The `.bkc` header length.
pub const CHUNK_HEADER_LEN: usize = 16;
pub const MAX_DIMENSION: u32 = 16384;
pub const MAX_PIDS: u32 = 4096;
/// Origin hexes are 0-39999.
pub const HEX_COUNT: u32 = 40000;
/// The largest valid depth value (group hex 39999).
pub const MAX_DEPTH: u16 = 40000;
pub const CHUNK_SIZES: [u16; 4] = [64, 128, 256, 512];

/// Chunk file compression, stored in the index.
pub const COMPRESSION_STORED: u8 = 0;
pub const COMPRESSION_ZLIB: u8 = 1;

/// Which planes a chunk carries; the colour plane is always present.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Planes {
    pub depth: bool,
    pub fade: bool,
}

impl Planes {
    /// The plane byte: bit 0 colour, bit 1 depth, bit 2 fade.
    pub fn bits(self) -> u8 {
        1 | (u8::from(self.depth) << 1) | (u8::from(self.fade) << 2)
    }

    fn from_bits(bits: u8) -> Option<Self> {
        (bits & 1 == 1 && bits < 8).then_some(Self {
            depth: bits & 2 != 0,
            fade: bits & 4 != 0,
        })
    }
}

/// Which objects of an elevation the compositor stops drawing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Suppression {
    None,
    /// Every wall-category object.
    Walls,
    Pids(Vec<u32>),
    WallsAndPids(Vec<u32>),
}

impl Suppression {
    fn code(&self) -> u8 {
        match self {
            Self::None => 0,
            Self::Walls => 1,
            Self::Pids(_) => 2,
            Self::WallsAndPids(_) => 3,
        }
    }

    /// The listed PIDs (empty for `None` and `Walls`).
    pub fn pids(&self) -> &[u32] {
        match self {
            Self::Pids(pids) | Self::WallsAndPids(pids) => pids,
            _ => &[],
        }
    }

    pub fn walls(&self) -> bool {
        matches!(self, Self::Walls | Self::WallsAndPids(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackdropElevation {
    pub elevation: u8,
    pub planes: Planes,
    pub origin_hex: i32,
    /// Image pixel of the origin hex's reference corner.
    pub origin_px: [i32; 2],
    pub size: [u32; 2],
    /// Chunk columns and rows.
    pub chunks: [u16; 2],
    /// Row-major, columns by rows.
    pub present: Vec<bool>,
    pub suppress: Suppression,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackdropIndex {
    pub version: u16,
    pub chunk_size: u16,
    /// The SPEC-021 64-bit hash of the palette the colour indexes target.
    pub palette_hash: u64,
    /// 0 stored, 1 zlib.
    pub compression: u8,
    pub elevations: Vec<BackdropElevation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackdropChunk {
    /// Image pixel of the chunk's top-left.
    pub origin: [u32; 2],
    pub size: [u16; 2],
    /// Palette indexes, 0 transparent; `size[0] * size[1]` bytes.
    pub colour: Vec<u8>,
    /// `size[0] * size[1]` values, each 0-40000.
    pub depth: Option<Vec<u16>>,
    /// One bit per pixel, rows padded to bytes, bit 7 first.
    pub fade: Option<Vec<u8>>,
}

fn fail(format: &'static str, offset: usize, message: impl Into<String>) -> ParseError {
    ParseError {
        format,
        offset,
        message: message.into(),
    }
}

fn byte(reader: &mut Reader<'_>) -> Result<u8, ParseError> {
    Ok(reader.take(1)?[0])
}

fn be_u32(reader: &mut Reader<'_>) -> Result<u32, ParseError> {
    let b = reader.take(4)?;
    Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn ceil_div(value: u32, by: u32) -> u32 {
    value.div_ceil(by)
}

/// The decoded payload length of a `w` by `h` chunk.
fn payload_len(planes: Planes, w: usize, h: usize) -> usize {
    let mut len = w * h;
    if planes.depth {
        len += 2 * w * h;
    }
    if planes.fade {
        len += h * w.div_ceil(8);
    }
    len
}

/// Depth value to group hex: 0 is the floor (`None`); 1-40000 is the
/// upright group hex `D - 1`. A value above 40000 is refused by the parser;
/// were one passed here it is also reported as `None`.
pub fn depth_group(depth: u16) -> Option<u32> {
    (1..=MAX_DEPTH)
        .contains(&depth)
        .then(|| u32::from(depth) - 1)
}

/// `maps/<stem>.bkd`, the stem lower-cased.
pub fn index_path(map_stem: &str) -> String {
    format!("maps/{}.bkd", map_stem.to_ascii_lowercase())
}

/// `maps/<stem>/e<E>/<CX>_<CY>.bkc`, the stem lower-cased, numbers decimal.
pub fn chunk_path(map_stem: &str, elevation: u8, column: u16, row: u16) -> String {
    format!(
        "maps/{}/e{elevation}/{column}_{row}.bkc",
        map_stem.to_ascii_lowercase()
    )
}

impl BackdropElevation {
    /// Image-pixel origin and size of chunk (`column`, `row`) for chunk size
    /// `chunk_size`; `None` outside the chunk table or the image.
    pub fn chunk_rect(
        &self,
        chunk_size: u16,
        column: u16,
        row: u16,
    ) -> Option<([u32; 2], [u16; 2])> {
        if chunk_size == 0 || column >= self.chunks[0] || row >= self.chunks[1] {
            return None;
        }
        let s = u32::from(chunk_size);
        let origin = [u32::from(column) * s, u32::from(row) * s];
        if origin[0] >= self.size[0] || origin[1] >= self.size[1] {
            return None;
        }
        let w = (self.size[0] - origin[0]).min(s);
        let h = (self.size[1] - origin[1]).min(s);
        Some((origin, [w as u16, h as u16]))
    }

    /// Whether the index lists a chunk file at (`column`, `row`).
    pub fn is_present(&self, column: u16, row: u16) -> bool {
        if column >= self.chunks[0] || row >= self.chunks[1] {
            return false;
        }
        let at = usize::from(row) * usize::from(self.chunks[0]) + usize::from(column);
        self.present.get(at).copied().unwrap_or(false)
    }
}

/// One validated elevation record, before its tables are read.
struct Record {
    at: usize,
    elevation: u8,
    planes: Planes,
    suppression: u8,
    origin_hex: i32,
    origin_px: [i32; 2],
    size: [u32; 2],
    chunks: [u16; 2],
    pid_count: usize,
}

impl BackdropIndex {
    /// The elevation record for `elevation`, if the index has one.
    pub fn elevation(&self, elevation: u8) -> Option<&BackdropElevation> {
        self.elevations.iter().find(|e| e.elevation == elevation)
    }

    /// Parse a `.bkd`. Layout choice for the brief's wording: all elevation
    /// records first, then, per elevation in the same order, its presence
    /// table and its PIDs. Bytes after the last PID are refused, as is a PID
    /// count over 0 with a suppression code that lists no PIDs.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        const F: &str = "BKD";
        let mut r = Reader::new(bytes, F)?;
        if r.take(4)? != INDEX_MAGIC {
            return Err(fail(F, 0, "bad magic, expected VBKD"));
        }
        let version = r.be_u16()?;
        if version != VERSION {
            return Err(fail(F, 4, format!("unsupported version {version}")));
        }
        let mask = byte(&mut r)?;
        if mask == 0 || mask > 7 {
            return Err(fail(F, 6, format!("bad elevation mask {mask:#04x}")));
        }
        let compression = byte(&mut r)?;
        if compression > COMPRESSION_ZLIB {
            return Err(fail(F, 7, format!("unknown compression {compression}")));
        }
        let chunk_size = r.be_u16()?;
        if !CHUNK_SIZES.contains(&chunk_size) {
            return Err(fail(F, 8, format!("bad chunk size {chunk_size}")));
        }
        if r.be_u16()? != 0 {
            return Err(fail(F, 0x0A, "reserved field is not 0"));
        }
        if be_u32(&mut r)? != 0 {
            return Err(fail(F, 0x0C, "reserved field is not 0"));
        }
        let hash = r.take(8)?;
        let palette_hash = u64::from_be_bytes(hash.try_into().expect("eight bytes"));

        let mut records = Vec::new();
        for bit in (0..3u8).filter(|bit| mask & (1 << bit) != 0) {
            let at = r.pos;
            let elevation = byte(&mut r)?;
            if elevation != bit {
                return Err(fail(
                    F,
                    at,
                    format!("elevation {elevation} is not mask bit {bit}"),
                ));
            }
            let plane_bits = byte(&mut r)?;
            let planes = Planes::from_bits(plane_bits)
                .ok_or_else(|| fail(F, at + 1, format!("bad planes {plane_bits:#04x}")))?;
            let suppression = byte(&mut r)?;
            if suppression > 3 {
                return Err(fail(F, at + 2, format!("bad suppression {suppression}")));
            }
            if byte(&mut r)? != 0 {
                return Err(fail(F, at + 3, "reserved byte is not 0"));
            }
            let origin_hex = be_u32(&mut r)?;
            if origin_hex >= HEX_COUNT {
                return Err(fail(
                    F,
                    at + 4,
                    format!("origin hex {origin_hex} out of range"),
                ));
            }
            let origin_px = [be_u32(&mut r)? as i32, be_u32(&mut r)? as i32];
            let mut size = [0u32; 2];
            for (i, value) in size.iter_mut().enumerate() {
                let field = at + 0x10 + 4 * i;
                *value = be_u32(&mut r)?;
                if *value == 0 || *value > MAX_DIMENSION {
                    return Err(fail(F, field, format!("dimension {value} out of range")));
                }
            }
            let mut chunks = [0u16; 2];
            for (i, value) in chunks.iter_mut().enumerate() {
                let field = at + 0x18 + 2 * i;
                *value = r.be_u16()?;
                let expected = ceil_div(size[i], u32::from(chunk_size));
                if u32::from(*value) != expected {
                    return Err(fail(
                        F,
                        field,
                        format!("chunk count {value}, expected {expected}"),
                    ));
                }
            }
            let pid_count = be_u32(&mut r)?;
            if pid_count > MAX_PIDS {
                return Err(fail(
                    F,
                    at + 0x1C,
                    format!("{pid_count} PIDs exceed {MAX_PIDS}"),
                ));
            }
            if pid_count > 0 && suppression < 2 {
                return Err(fail(F, at + 0x1C, "PIDs listed without a PID suppression"));
            }
            records.push(Record {
                at,
                elevation,
                planes,
                suppression,
                origin_hex: origin_hex as i32,
                origin_px,
                size,
                chunks,
                pid_count: pid_count as usize,
            });
        }

        let mut elevations = Vec::with_capacity(records.len());
        for record in records {
            let cells = usize::from(record.chunks[0]) * usize::from(record.chunks[1]);
            let table_at = r.pos;
            if cells + 4 * record.pid_count > bytes.len() - r.pos {
                return Err(fail(
                    F,
                    table_at,
                    format!(
                        "elevation record at {:#x}: tables run past the input",
                        record.at
                    ),
                ));
            }
            let mut present = Vec::with_capacity(cells);
            for (i, flag) in r.take(cells)?.iter().enumerate() {
                match flag {
                    0 => present.push(false),
                    1 => present.push(true),
                    _ => return Err(fail(F, table_at + i, "presence byte is not 0 or 1")),
                }
            }
            let pids: Vec<u32> = r
                .take(4 * record.pid_count)?
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| u32::from_be_bytes(*b))
                .collect();
            let suppress = match record.suppression {
                0 => Suppression::None,
                1 => Suppression::Walls,
                2 => Suppression::Pids(pids),
                _ => Suppression::WallsAndPids(pids),
            };
            elevations.push(BackdropElevation {
                elevation: record.elevation,
                planes: record.planes,
                origin_hex: record.origin_hex,
                origin_px: record.origin_px,
                size: record.size,
                chunks: record.chunks,
                present,
                suppress,
            });
        }
        if r.pos != bytes.len() {
            return Err(fail(F, r.pos, "bytes after the last table"));
        }
        Ok(Self {
            version,
            chunk_size,
            palette_hash,
            compression,
            elevations,
        })
    }

    /// The `.bkd` bytes; `parse(to_bytes())` returns this index when it is
    /// valid.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&INDEX_MAGIC);
        out.extend_from_slice(&self.version.to_be_bytes());
        let mask = self.elevations.iter().fold(0u8, |mask, e| {
            mask | 1u8.checked_shl(e.elevation.into()).unwrap_or(0)
        });
        out.push(mask);
        out.push(self.compression);
        out.extend_from_slice(&self.chunk_size.to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        out.extend_from_slice(&self.palette_hash.to_be_bytes());
        for e in &self.elevations {
            out.push(e.elevation);
            out.push(e.planes.bits());
            out.push(e.suppress.code());
            out.push(0);
            out.extend_from_slice(&(e.origin_hex as u32).to_be_bytes());
            for value in e.origin_px {
                out.extend_from_slice(&(value as u32).to_be_bytes());
            }
            for value in e.size {
                out.extend_from_slice(&value.to_be_bytes());
            }
            for value in e.chunks {
                out.extend_from_slice(&value.to_be_bytes());
            }
            out.extend_from_slice(&(e.suppress.pids().len() as u32).to_be_bytes());
        }
        for e in &self.elevations {
            out.extend(e.present.iter().map(|&flag| u8::from(flag)));
            for pid in e.suppress.pids() {
                out.extend_from_slice(&pid.to_be_bytes());
            }
        }
        out
    }
}

/// A zlib stream (RFC 1950) of stored DEFLATE blocks (RFC 1951 3.2.4).
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut blocks = data.chunks(0xFFFF).peekable();
    if blocks.peek().is_none() {
        out.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(block) = blocks.next() {
        out.push(u8::from(blocks.peek().is_none()));
        let len = block.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(block);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

impl BackdropChunk {
    /// Whether the fade bit is set at (`x`, `y`) in the chunk; false outside
    /// it and when there is no fade plane.
    pub fn fade_at(&self, x: u16, y: u16) -> bool {
        let [w, h] = self.size;
        if x >= w || y >= h {
            return false;
        }
        let Some(fade) = &self.fade else {
            return false;
        };
        let at = usize::from(y) * usize::from(w).div_ceil(8) + usize::from(x / 8);
        fade.get(at).is_some_and(|b| b & (0x80 >> (x % 8)) != 0)
    }

    /// Parse the `.bkc` of chunk (column, row in the file's own header) for
    /// `elevation` of `index`. Offsets inside a zlib payload are those the
    /// inflater reports, counted from the payload start (`0x10`); a depth
    /// refusal reports `0x10` plus the offset in the decoded payload.
    pub fn parse(bytes: &[u8], index: &BackdropIndex, elevation: u8) -> Result<Self, ParseError> {
        const F: &str = "BKC";
        let mut r = Reader::new(bytes, F)?;
        if r.take(4)? != CHUNK_MAGIC {
            return Err(fail(F, 0, "bad magic, expected VBKC"));
        }
        let version = r.be_u16()?;
        if version != VERSION {
            return Err(fail(F, 4, format!("unsupported version {version}")));
        }
        let record = index
            .elevation(elevation)
            .ok_or_else(|| fail(F, 6, format!("elevation {elevation} is not in the index")))?;
        let found = byte(&mut r)?;
        if found != elevation {
            return Err(fail(
                F,
                6,
                format!("elevation {found}, expected {elevation}"),
            ));
        }
        let plane_bits = byte(&mut r)?;
        if plane_bits != record.planes.bits() {
            return Err(fail(
                F,
                7,
                format!(
                    "planes {plane_bits:#04x}, index has {:#04x}",
                    record.planes.bits()
                ),
            ));
        }
        let column = r.be_u16()?;
        let row = r.be_u16()?;
        let (origin, size) = record
            .chunk_rect(index.chunk_size, column, row)
            .ok_or_else(|| {
                let (at, what) = if column >= record.chunks[0] {
                    (8, "column")
                } else {
                    (0x0A, "row")
                };
                fail(F, at, format!("chunk {what} outside the table"))
            })?;
        if !record.is_present(column, row) {
            return Err(fail(F, 8, "the index lists no file for this chunk"));
        }
        let expected = payload_len(record.planes, size[0].into(), size[1].into());
        let declared = be_u32(&mut r)?;
        if declared as usize != expected {
            return Err(fail(
                F,
                0x0C,
                format!("payload length {declared}, expected {expected}"),
            ));
        }
        let rest = &bytes[CHUNK_HEADER_LEN..];
        let payload = if index.compression == COMPRESSION_ZLIB {
            zlib_decompress(rest, expected)
                .map_err(|e| fail(F, CHUNK_HEADER_LEN + e.offset, e.message))?
        } else {
            if rest.len() != expected {
                let at = CHUNK_HEADER_LEN + rest.len().min(expected);
                return Err(fail(
                    F,
                    at,
                    format!("payload is {} bytes, expected {expected}", rest.len()),
                ));
            }
            rest.to_vec()
        };
        let pixels = usize::from(size[0]) * usize::from(size[1]);
        let (colour, tail) = payload.split_at(pixels);
        let (depth, tail) = if record.planes.depth {
            let (plane, tail) = tail.split_at(2 * pixels);
            let mut values = Vec::with_capacity(pixels);
            for (i, pair) in plane.as_chunks::<2>().0.iter().enumerate() {
                let value = u16::from_be_bytes(*pair);
                if value > MAX_DEPTH {
                    return Err(fail(
                        F,
                        CHUNK_HEADER_LEN + pixels + 2 * i,
                        format!("depth {value} above {MAX_DEPTH}"),
                    ));
                }
                values.push(value);
            }
            (Some(values), tail)
        } else {
            (None, tail)
        };
        let fade = record.planes.fade.then(|| tail.to_vec());
        Ok(Self {
            origin,
            size,
            colour: colour.to_vec(),
            depth,
            fade,
        })
    }

    /// The `.bkc` bytes for chunk (`column`, `row`) of `elevation`, with the
    /// index's planes and compression. A plane the index declares but this
    /// chunk lacks is written as zeros, and plane lengths are padded or cut
    /// to the chunk size, so the output is always well framed.
    pub fn to_bytes(&self, index: &BackdropIndex, elevation: u8, column: u16, row: u16) -> Vec<u8> {
        let planes = index
            .elevation(elevation)
            .map_or(Planes::default(), |e| e.planes);
        let (w, h) = (usize::from(self.size[0]), usize::from(self.size[1]));
        let mut payload = self.colour.clone();
        payload.resize(w * h, 0);
        if planes.depth {
            let mut depth = self.depth.clone().unwrap_or_default();
            depth.resize(w * h, 0);
            payload.extend(depth.iter().flat_map(|d| d.to_be_bytes()));
        }
        if planes.fade {
            let mut fade = self.fade.clone().unwrap_or_default();
            fade.resize(h * w.div_ceil(8), 0);
            payload.extend_from_slice(&fade);
        }
        let mut out = Vec::new();
        out.extend_from_slice(&CHUNK_MAGIC);
        out.extend_from_slice(&VERSION.to_be_bytes());
        out.push(elevation);
        out.push(planes.bits());
        out.extend_from_slice(&column.to_be_bytes());
        out.extend_from_slice(&row.to_be_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        if index.compression == COMPRESSION_ZLIB {
            out.extend_from_slice(&zlib_stored(&payload));
        } else {
            out.extend_from_slice(&payload);
        }
        out
    }
}

#[cfg(test)]
#[path = "backdrop_tests.rs"]
mod tests;
