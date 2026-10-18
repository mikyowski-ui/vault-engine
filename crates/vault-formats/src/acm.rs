//! SPEC-010 `.ACM` audio: the 14-byte little-endian header and the exact
//! decoding of its LSB-first block bitstream to signed 16-bit PCM (step
//! table, column codes, persistent lifting carries, and wrapping output).
use crate::{MAX_ASSET_BYTES, ParseError};

pub const SIGNATURE: [u8; 3] = [0x97, 0x28, 0x03];
pub const VERSION: u8 = 1;
pub const HEADER_BYTES: usize = 14;
/// Effects and speech play as mono 16-bit samples at this mixer rate,
/// whatever the header declares; background music plays as interleaved
/// stereo at the same rate.
pub const MIXER_RATE: u32 = 22050;
/// Safe bounds: decoded PCM is limited like any other asset, and one block
/// (up to 4095 x 32768 elements in the format) to 4 Mi elements.
pub const MAX_SAMPLES: u32 = (MAX_ASSET_BYTES / 2) as u32;
pub const MAX_BLOCK_ELEMENTS: usize = 1 << 22;

/// The ACM header. `rows` and `levels` come from the packed word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcmHeader {
    /// Total 16-bit samples to emit, all channels together.
    pub samples: u32,
    pub channels: u16,
    pub rate: u16,
    pub levels: u8,
    pub rows: u16,
}
impl AcmHeader {
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let header = bytes.get(..HEADER_BYTES).ok_or_else(|| {
            ParseError::new("ACM", bytes.len(), "header is shorter than 14 bytes")
        })?;
        if header[..3] != SIGNATURE {
            return Err(ParseError::new("ACM", 0, "bad signature"));
        }
        if header[3] != VERSION {
            return Err(ParseError::new("ACM", 3, "unsupported version"));
        }
        let word = |at: usize| u16::from_le_bytes([header[at], header[at + 1]]);
        let packed = word(12);
        Ok(Self {
            samples: u32::from_le_bytes([header[4], header[5], header[6], header[7]]),
            channels: word(8),
            rate: word(10),
            levels: (packed & 0x0f) as u8,
            rows: packed >> 4,
        })
    }
    /// Column count `2^L`.
    pub fn columns(&self) -> usize {
        1 << self.levels
    }
}

/// A decoded stream and the decoder's corpus-observable counters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub header: AcmHeader,
    pub samples: Vec<i16>,
    /// Blocks decoded (the last one may be partly discarded).
    pub blocks: u32,
    /// Zero bytes the bit reader supplied past the end of the file.
    pub past_end: u32,
    /// `(sample index, value after the level shift)` for every sample that
    /// wrapped to 16 bits.
    pub wrapped: Vec<(usize, i32)>,
}
impl Decoded {
    /// The samples as signed 16-bit little-endian bytes.
    pub fn pcm(&self) -> Vec<u8> {
        self.samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect()
    }
}

/// `x mod 65536` reinterpreted as signed 16-bit.
pub fn wrap16(value: i64) -> i32 {
    i32::from(value as u16 as i16)
}

/// LSB-first reader: bytes are appended above the bits already held and
/// fields are taken from the low end. Past the end it supplies zero bytes.
struct Bits<'a> {
    bytes: &'a [u8],
    next: usize,
    value: u64,
    count: u32,
    past_end: u32,
}
impl<'a> Bits<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            next: 0,
            value: 0,
            count: 0,
            past_end: 0,
        }
    }
    /// The next `width` bits without consuming them (fetching on demand).
    fn peek(&mut self, width: u32) -> u32 {
        while self.count < width {
            let byte = match self.bytes.get(self.next) {
                Some(byte) => *byte,
                None => {
                    self.past_end += 1;
                    0
                }
            };
            self.next += 1;
            self.value |= u64::from(byte) << self.count;
            self.count += 8;
        }
        (self.value & ((1 << width) - 1)) as u32
    }
    fn skip(&mut self, width: u32) {
        self.value >>= width;
        self.count -= width;
    }
    fn read(&mut self, width: u32) -> u32 {
        let value = self.peek(width);
        self.skip(width);
        value
    }
    /// Bits consumed so far.
    fn position(&self) -> u64 {
        self.next as u64 * 8 - u64::from(self.count)
    }
}

/// The per-block step table `T[i] = wrap16(i x s)` for `i` in
/// `-2^k .. 2^k - 1`; other indexes are decode errors.
struct Table {
    values: Vec<i32>,
    half: i32,
}
impl Table {
    fn new(k: u32, step: u32) -> Self {
        let half = 1_i32 << k;
        Self {
            values: (-half..half)
                .map(|index| wrap16(i64::from(index) * i64::from(step)))
                .collect(),
            half,
        }
    }
    fn get(&self, index: i32) -> Result<i32, ParseError> {
        if index < -self.half || index >= self.half {
            return Err(ParseError::new(
                "ACM",
                0,
                format!("step-table index {index} is outside -{0}..{0}", self.half),
            ));
        }
        Ok(self.values[(index + self.half) as usize])
    }
}

/// One column's values, rows 0 to R-1, written at `row x C + column`.
struct Column<'b> {
    block: &'b mut [i32],
    columns: usize,
    column: usize,
    rows: usize,
    row: usize,
}
impl Column<'_> {
    fn remaining(&self) -> usize {
        self.rows - self.row
    }
    fn put(&mut self, value: i32) {
        if self.row < self.rows {
            self.block[self.row * self.columns + self.column] = value;
            self.row += 1;
        }
    }
    /// A two-zeros code stores a single zero when one row remains.
    fn zeros(&mut self, count: usize) {
        for _ in 0..count.min(self.remaining()) {
            self.put(0);
        }
    }
}

const SIGNED_TWO: [i32; 4] = [-2, -1, 1, 2];
const SIGNED_THREE: [i32; 4] = [-3, -2, 2, 3];
const SIGNED_FOUR: [i32; 8] = [-4, -3, -2, -1, 1, 2, 3, 4];

/// Decode one column of format `format` (section 2.4).
fn column(bits: &mut Bits, table: &Table, format: u32, out: &mut Column) -> Result<(), ParseError> {
    let t = |index: i32| table.get(index);
    match format {
        0 => out.zeros(out.rows),
        3..=16 => {
            let bias = 1_i32 << (format - 1);
            while out.remaining() > 0 {
                let value = bits.read(format) as i32;
                out.put(t(value - bias)?);
            }
        }
        17 => {
            while out.remaining() > 0 {
                let code = bits.peek(3);
                if code & 1 == 0 {
                    bits.skip(1);
                    out.zeros(2);
                } else if code & 2 == 0 {
                    bits.skip(2);
                    out.zeros(1);
                } else {
                    bits.skip(3);
                    out.put(t(if code & 4 != 0 { 1 } else { -1 })?);
                }
            }
        }
        18 => {
            while out.remaining() > 0 {
                let code = bits.peek(2);
                if code & 1 == 0 {
                    bits.skip(1);
                    out.zeros(1);
                } else {
                    bits.skip(2);
                    out.put(t(if code & 2 != 0 { 1 } else { -1 })?);
                }
            }
        }
        19 => grouped(bits, &t, out, 5, 3, 27, 1)?,
        20 => {
            while out.remaining() > 0 {
                let code = bits.peek(4);
                if code & 1 == 0 {
                    bits.skip(1);
                    out.zeros(2);
                } else if code & 2 == 0 {
                    bits.skip(2);
                    out.zeros(1);
                } else {
                    bits.skip(4);
                    out.put(t(SIGNED_TWO[(code >> 2 & 3) as usize])?);
                }
            }
        }
        21 => {
            while out.remaining() > 0 {
                let code = bits.peek(3);
                if code & 1 == 0 {
                    bits.skip(1);
                    out.zeros(1);
                } else {
                    bits.skip(3);
                    out.put(t(SIGNED_TWO[(code >> 1 & 3) as usize])?);
                }
            }
        }
        22 => grouped(bits, &t, out, 7, 5, 125, 2)?,
        23 => {
            while out.remaining() > 0 {
                let code = bits.peek(5);
                if code & 1 == 0 {
                    bits.skip(1);
                    out.zeros(2);
                } else if code & 2 == 0 {
                    bits.skip(2);
                    out.zeros(1);
                } else if code & 4 == 0 {
                    bits.skip(4);
                    out.put(t(if code & 8 != 0 { 1 } else { -1 })?);
                } else {
                    bits.skip(5);
                    out.put(t(SIGNED_THREE[(code >> 3 & 3) as usize])?);
                }
            }
        }
        24 => {
            while out.remaining() > 0 {
                let code = bits.peek(4);
                if code & 1 == 0 {
                    bits.skip(1);
                    out.zeros(1);
                } else if code & 2 == 0 {
                    bits.skip(3);
                    out.put(t(if code & 4 != 0 { 1 } else { -1 })?);
                } else {
                    bits.skip(4);
                    out.put(t(SIGNED_THREE[(code >> 2 & 3) as usize])?);
                }
            }
        }
        26 => {
            while out.remaining() > 0 {
                let code = bits.peek(5);
                if code & 1 == 0 {
                    bits.skip(1);
                    out.zeros(2);
                } else if code & 2 == 0 {
                    bits.skip(2);
                    out.zeros(1);
                } else {
                    bits.skip(5);
                    out.put(t(SIGNED_FOUR[(code >> 2 & 7) as usize])?);
                }
            }
        }
        27 => {
            while out.remaining() > 0 {
                let code = bits.peek(4);
                if code & 1 == 0 {
                    bits.skip(1);
                    out.zeros(1);
                } else {
                    bits.skip(4);
                    out.put(t(SIGNED_FOUR[(code >> 1 & 7) as usize])?);
                }
            }
        }
        29 => grouped(bits, &t, out, 7, 11, 121, 5)?,
        _ => {
            return Err(ParseError::new(
                "ACM",
                0,
                format!("reserved column format {format}"),
            ));
        }
    }
    Ok(())
}

/// Grouped codes: a `width`-bit field holds base-`base` digits (low digit
/// first) mapped to `T[d - offset]`; fields at or above `limit` give all
/// minimum digits. Digits past the last row are discarded.
fn grouped(
    bits: &mut Bits,
    t: &impl Fn(i32) -> Result<i32, ParseError>,
    out: &mut Column,
    width: u32,
    base: u32,
    limit: u32,
    offset: i32,
) -> Result<(), ParseError> {
    let digits = if base == 11 { 2 } else { 3 };
    while out.remaining() > 0 {
        let mut value = bits.read(width);
        if value >= limit {
            value = 0;
        }
        for _ in 0..digits {
            if out.remaining() == 0 {
                break;
            }
            out.put(t((value % base) as i32 - offset)?);
            value /= base;
        }
    }
    Ok(())
}

/// One lifting pass over `rows` x `width` contiguous elements, with one
/// persistent carry pair per column.
fn lift(elements: &mut [i32], rows: usize, width: usize, carries: &mut [(i32, i32)]) {
    for (x, carry) in carries.iter_mut().enumerate().take(width) {
        let (mut a, mut b) = *carry;
        for pair in 0..rows / 2 {
            let (first, second) = (2 * pair * width + x, (2 * pair + 1) * width + x);
            let (x0, x1) = (elements[first], elements[second]);
            elements[first] = x0.wrapping_add(b.wrapping_mul(2)).wrapping_add(a);
            elements[second] = x0.wrapping_mul(2).wrapping_sub(b).wrapping_sub(x1);
            a = x0;
            b = x1;
        }
        *carry = (a, b);
    }
}

/// Decode a complete file. Reserved column formats, out-of-range table
/// indexes, `R = 0` with samples to emit, and a block header that would
/// start at or past the end of the file are errors; so are streams beyond
/// the safe sample and block bounds.
pub fn decode(bytes: &[u8]) -> Result<Decoded, ParseError> {
    let header = AcmHeader::parse(bytes)?;
    let mut bits = Bits::new(bytes);
    for _ in 0..HEADER_BYTES {
        bits.read(8);
    }
    let total = header.samples as usize;
    let (rows, columns, levels) = (
        usize::from(header.rows),
        header.columns(),
        u32::from(header.levels),
    );
    if header.samples > MAX_SAMPLES {
        return Err(ParseError::new(
            "ACM",
            4,
            "sample count exceeds the safe bound",
        ));
    }
    if total > 0 && rows == 0 {
        return Err(ParseError::new(
            "ACM",
            12,
            "zero rows can never emit samples",
        ));
    }
    if rows * columns > MAX_BLOCK_ELEMENTS {
        return Err(ParseError::new("ACM", 12, "block exceeds the safe bound"));
    }
    let chunk = (2048 / columns as i64 - 2).max(1) as usize;
    let mut carries: Vec<Vec<(i32, i32)>> = (1..=levels)
        .map(|level| vec![(0, 0); columns >> level])
        .collect();
    let mut block = vec![0_i32; rows * columns];
    let mut decoded = Decoded {
        header,
        samples: Vec::with_capacity(total),
        blocks: 0,
        past_end: 0,
        wrapped: Vec::new(),
    };
    while decoded.samples.len() < total {
        if bits.position() >= bytes.len() as u64 * 8 {
            return Err(ParseError::new(
                "ACM",
                bytes.len(),
                "a block header starts at or past the end of the file",
            ));
        }
        let k = bits.read(4);
        let step = bits.read(16);
        let table = Table::new(k, step);
        for index in 0..columns {
            let format = bits.read(5);
            column(
                &mut bits,
                &table,
                format,
                &mut Column {
                    block: &mut block,
                    columns,
                    column: index,
                    rows,
                    row: 0,
                },
            )?;
        }
        if levels > 0 {
            let mut row = 0;
            while row < rows {
                let r = chunk.min(rows - row);
                let elements = &mut block[row * columns..(row + r) * columns];
                let width = columns / 2;
                lift(elements, 2 * r, width, &mut carries[0]);
                for carry in &mut carries[0] {
                    *carry = (wrap16(i64::from(carry.0)), wrap16(i64::from(carry.1)));
                }
                for line in 0..2 * r {
                    elements[line * width] = elements[line * width].wrapping_add(1);
                }
                for level in 2..=levels as usize {
                    lift(
                        elements,
                        r << level,
                        columns >> level,
                        &mut carries[level - 1],
                    );
                }
                row += r;
            }
        }
        let emit = (rows * columns).min(total - decoded.samples.len());
        for element in &block[..emit] {
            let shifted = element >> levels;
            let sample = wrap16(i64::from(shifted));
            if sample != shifted {
                decoded.wrapped.push((decoded.samples.len(), shifted));
            }
            decoded.samples.push(sample as i16);
        }
        decoded.blocks += 1;
    }
    decoded.past_end = bits.past_end;
    Ok(decoded)
}
