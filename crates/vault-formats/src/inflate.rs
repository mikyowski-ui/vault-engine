//! SPEC-015 §5: a bounded DEFLATE (RFC 1951) decoder and the zlib (RFC 1950)
//! wrapper that Fallout 2 archive members use. Written from the two RFCs;
//! every case they leave undefined is refused, and the output can never
//! grow past the size the caller expects.
use crate::ParseError;

/// RFC 1951 §3.2.5: the base lengths and extra bits of length symbols
/// 257-285.
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
/// RFC 1951 §3.2.5: the base distances and extra bits of distance symbols
/// 0-29.
const DISTANCE_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DISTANCE_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
/// RFC 1951 §3.2.7: the order code-length code lengths are stored in.
const CODE_LENGTH_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];
const MAX_BITS: usize = 15;

fn error(offset: usize, message: impl Into<String>) -> ParseError {
    ParseError::new("zlib", offset, message)
}

/// The input, read least-significant bit first (RFC 1951 §3.1.1).
struct Bits<'a> {
    input: &'a [u8],
    /// The next byte to load.
    pos: usize,
    buffer: u32,
    count: u32,
}

impl<'a> Bits<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            pos: 0,
            buffer: 0,
            count: 0,
        }
    }
    fn bits(&mut self, need: u32) -> Result<u32, ParseError> {
        while self.count < need {
            let byte = *self
                .input
                .get(self.pos)
                .ok_or_else(|| error(self.pos, "the stream ends early"))?;
            self.pos += 1;
            self.buffer |= u32::from(byte) << self.count;
            self.count += 8;
        }
        let value = self.buffer & ((1u32 << need) - 1);
        self.buffer = if need == 32 { 0 } else { self.buffer >> need };
        self.count -= need;
        Ok(value)
    }
    /// Drop the bits left in the current byte (a stored block, the end).
    fn align(&mut self) {
        self.buffer = 0;
        self.count = 0;
    }
}

/// A canonical Huffman code (RFC 1951 §3.2.2): the number of codes of each
/// length, and the symbols in code order.
struct Huffman {
    counts: [u16; MAX_BITS + 1],
    symbols: Vec<u16>,
}

impl Huffman {
    /// The code for `lengths` (0: the symbol is unused). An over-subscribed
    /// code is refused; an incomplete one is kept (its unused codes fail
    /// when read).
    fn new(lengths: &[u8], offset: usize) -> Result<Self, ParseError> {
        let mut counts = [0u16; MAX_BITS + 1];
        for &length in lengths {
            counts[usize::from(length)] += 1;
        }
        counts[0] = 0;
        let mut left: i32 = 1;
        for &count in &counts[1..] {
            left = left * 2 - i32::from(count);
            if left < 0 {
                return Err(error(offset, "an over-subscribed Huffman code"));
            }
        }
        let mut starts = [0u16; MAX_BITS + 2];
        for length in 1..=MAX_BITS {
            starts[length + 1] = starts[length] + counts[length];
        }
        let mut symbols = vec![0u16; usize::from(starts[MAX_BITS + 1])];
        for (symbol, &length) in lengths.iter().enumerate() {
            if length != 0 {
                let slot = &mut starts[usize::from(length)];
                symbols[usize::from(*slot)] = symbol as u16;
                *slot += 1;
            }
        }
        Ok(Self { counts, symbols })
    }
    /// The next symbol, bit by bit through the code lengths.
    fn decode(&self, bits: &mut Bits) -> Result<u16, ParseError> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for length in 1..=MAX_BITS {
            code |= bits.bits(1)? as i32;
            let count = i32::from(self.counts[length]);
            if code - first < count {
                return Ok(self.symbols[(index + code - first) as usize]);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(error(bits.pos, "a code that is not in the Huffman table"))
    }
}

/// RFC 1951 §3.2.6: the fixed literal/length and distance codes.
fn fixed() -> (Huffman, Huffman) {
    let mut lengths = [0u8; 288];
    lengths[..144].fill(8);
    lengths[144..256].fill(9);
    lengths[256..280].fill(7);
    lengths[280..].fill(8);
    let literal = Huffman::new(&lengths, 0).expect("the fixed code is complete");
    let distance = Huffman::new(&[5u8; 30], 0).expect("the fixed code is complete");
    (literal, distance)
}

/// RFC 1951 §3.2.7: a dynamic block's two codes.
fn dynamic(bits: &mut Bits) -> Result<(Huffman, Huffman), ParseError> {
    let literals = bits.bits(5)? as usize + 257;
    let distances = bits.bits(5)? as usize + 1;
    let code_lengths = bits.bits(4)? as usize + 4;
    if literals > 286 || distances > 30 {
        return Err(error(bits.pos, "too many literal/length or distance codes"));
    }
    let mut lengths = [0u8; 19];
    for &symbol in &CODE_LENGTH_ORDER[..code_lengths] {
        lengths[symbol] = bits.bits(3)? as u8;
    }
    let code = Huffman::new(&lengths, bits.pos)?;
    let mut lengths = vec![0u8; literals + distances];
    let mut index = 0;
    while index < lengths.len() {
        let symbol = code.decode(bits)?;
        let (value, repeat) = match symbol {
            0..=15 => (symbol as u8, 1),
            16 => {
                let previous = index
                    .checked_sub(1)
                    .map(|previous| lengths[previous])
                    .ok_or_else(|| error(bits.pos, "a repeat with no previous length"))?;
                (previous, 3 + bits.bits(2)? as usize)
            }
            17 => (0, 3 + bits.bits(3)? as usize),
            _ => (0, 11 + bits.bits(7)? as usize),
        };
        if index + repeat > lengths.len() {
            return Err(error(bits.pos, "code lengths run past HLIT + HDIST"));
        }
        lengths[index..index + repeat].fill(value);
        index += repeat;
    }
    if lengths[256] == 0 {
        return Err(error(bits.pos, "a code with no end-of-block symbol"));
    }
    let literal = Huffman::new(&lengths[..literals], bits.pos)?;
    let distance = Huffman::new(&lengths[literals..], bits.pos)?;
    Ok((literal, distance))
}

/// Decode a raw DEFLATE stream (RFC 1951) of exactly `size` output bytes.
/// Returns the output and how many input bytes the stream used (up to the
/// byte holding its last bit).
pub fn inflate(input: &[u8], size: usize) -> Result<(Vec<u8>, usize), ParseError> {
    let mut bits = Bits::new(input);
    // Growth follows the output actually produced, never past `size`.
    let mut output: Vec<u8> = Vec::with_capacity(size.min(1 << 16));
    let too_long = |pos: usize| error(pos, format!("the data is longer than {size} bytes"));
    loop {
        let last = bits.bits(1)? == 1;
        match bits.bits(2)? {
            0 => {
                bits.align();
                let header = input
                    .get(bits.pos..bits.pos + 4)
                    .ok_or_else(|| error(bits.pos, "the stream ends early"))?;
                let length = u16::from_le_bytes([header[0], header[1]]);
                let complement = u16::from_le_bytes([header[2], header[3]]);
                if length != !complement {
                    return Err(error(bits.pos, "a stored block's LEN and NLEN disagree"));
                }
                bits.pos += 4;
                let body = input
                    .get(bits.pos..bits.pos + usize::from(length))
                    .ok_or_else(|| error(bits.pos, "the stream ends early"))?;
                if output.len() + body.len() > size {
                    return Err(too_long(bits.pos));
                }
                output.extend_from_slice(body);
                bits.pos += usize::from(length);
            }
            kind @ (1 | 2) => {
                let (literal, distance) = if kind == 1 {
                    fixed()
                } else {
                    dynamic(&mut bits)?
                };
                loop {
                    let symbol = usize::from(literal.decode(&mut bits)?);
                    match symbol {
                        0..=255 => {
                            if output.len() >= size {
                                return Err(too_long(bits.pos));
                            }
                            output.push(symbol as u8);
                        }
                        256 => break,
                        257..=285 => {
                            let index = symbol - 257;
                            let length = usize::from(LENGTH_BASE[index])
                                + bits.bits(u32::from(LENGTH_EXTRA[index]))? as usize;
                            let code = usize::from(distance.decode(&mut bits)?);
                            if code >= 30 {
                                return Err(error(bits.pos, "distance symbol 30 or 31"));
                            }
                            let back = usize::from(DISTANCE_BASE[code])
                                + bits.bits(u32::from(DISTANCE_EXTRA[code]))? as usize;
                            if back > output.len() {
                                return Err(error(bits.pos, "a distance before the start"));
                            }
                            if output.len() + length > size {
                                return Err(too_long(bits.pos));
                            }
                            let start = output.len() - back;
                            // Byte by byte: a copy may overlap its own output.
                            for offset in 0..length {
                                output.push(output[start + offset]);
                            }
                        }
                        _ => return Err(error(bits.pos, "length symbol 286 or 287")),
                    }
                }
            }
            _ => return Err(error(bits.pos, "block type 3")),
        }
        if last {
            break;
        }
    }
    if output.len() != size {
        return Err(error(
            bits.pos,
            format!("the data is {} bytes, not {size}", output.len()),
        ));
    }
    Ok((output, bits.pos))
}

/// RFC 1950 §9: the Adler-32 checksum of `data`.
pub fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let (mut a, mut b) = (1u32, 0u32);
    // 5552 bytes keep the sums below 2^32 between reductions.
    for chunk in data.chunks(5552) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    (b << 16) | a
}

/// SPEC-015 §5.1: a whole zlib stream (RFC 1950) of exactly `size` output
/// bytes: its header checked, the DEFLATE data decoded, the Adler-32
/// checksum verified, and nothing after it.
pub fn zlib_decompress(input: &[u8], size: usize) -> Result<Vec<u8>, ParseError> {
    let [cmf, flg, ..] = *input else {
        return Err(error(0, "the stream ends early"));
    };
    if cmf & 0x0f != 8 {
        return Err(error(0, "the compression method is not DEFLATE"));
    }
    if cmf >> 4 > 7 {
        return Err(error(0, "a window larger than 32 KiB"));
    }
    if !(u16::from(cmf) * 256 + u16::from(flg)).is_multiple_of(31) {
        return Err(error(1, "the header check fails"));
    }
    if flg & 0x20 != 0 {
        return Err(error(1, "a preset dictionary"));
    }
    let (output, used) = inflate(&input[2..], size)?;
    let end = 2 + used;
    let checksum = input
        .get(end..end + 4)
        .ok_or_else(|| error(end, "the checksum is missing"))?;
    if u32::from_be_bytes([checksum[0], checksum[1], checksum[2], checksum[3]]) != adler32(&output)
    {
        return Err(error(end, "the Adler-32 checksum does not match"));
    }
    if input.len() != end + 4 {
        return Err(error(end + 4, "bytes after the checksum"));
    }
    Ok(output)
}

#[cfg(test)]
#[path = "inflate_tests.rs"]
mod tests;
