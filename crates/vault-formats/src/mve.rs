//! SPEC-012 `.MVE` movies: the 26-byte little-endian header, chunk records
//! with the read-ahead rule, opcode framing, the 8-bit block video codec
//! (two buffers, swap, decoding map, block types 0-15 and their motion
//! tables), the palette table, and the DPCM audio codec.
//!
//! `Movie::next_event` processes opcodes in stream order and reports what a
//! player schedules: timer creation, sync points, audio, and shown frames.
//! Pacing, dropping, palette pushes, and presentation belong to the player.
//! The reference's unchecked reads are stream errors here (the SPEC-012
//! `[May-improve]` safe rules); an error ends the movie.
use crate::{MAX_ASSET_BYTES, ParseError};

pub const SIGNATURE: [u8; 20] = *b"Interplay MVE File\x1a\x00";
/// The file header; the first chunk header follows it.
pub const HEADER_BYTES: usize = 26;
const VERSION: u16 = 0x0100;
const CHECK_SUM: u16 = 0x1233;
/// Frames must fit the 640 x 480 output surface.
pub const SURFACE: [usize; 2] = [640, 480];

/// §2.8 compressed-audio deltas, added to a 16-bit predictor (engine data,
/// reproduced exactly, including the out-of-order codes 237 and 238).
#[rustfmt::skip]
pub const AUDIO_DELTAS: [u16; 256] = [
    0x0000, 0x0001, 0x0002, 0x0003, 0x0004, 0x0005, 0x0006, 0x0007, 0x0008, 0x0009, 0x000A, 0x000B, 0x000C, 0x000D, 0x000E, 0x000F,
    0x0010, 0x0011, 0x0012, 0x0013, 0x0014, 0x0015, 0x0016, 0x0017, 0x0018, 0x0019, 0x001A, 0x001B, 0x001C, 0x001D, 0x001E, 0x001F,
    0x0020, 0x0021, 0x0022, 0x0023, 0x0024, 0x0025, 0x0026, 0x0027, 0x0028, 0x0029, 0x002A, 0x002B, 0x002F, 0x0033, 0x0038, 0x003D,
    0x0042, 0x0048, 0x004F, 0x0056, 0x005E, 0x0066, 0x0070, 0x007A, 0x0085, 0x0091, 0x009E, 0x00AD, 0x00BD, 0x00CE, 0x00E1, 0x00F5,
    0x010B, 0x0124, 0x013E, 0x015C, 0x017B, 0x019E, 0x01C4, 0x01ED, 0x021A, 0x024B, 0x0280, 0x02BB, 0x02FB, 0x0340, 0x038C, 0x03DF,
    0x0439, 0x049C, 0x0508, 0x057D, 0x05FE, 0x0689, 0x0722, 0x07C9, 0x087F, 0x0945, 0x0A1E, 0x0B0A, 0x0C0C, 0x0D25, 0x0E58, 0x0FA8,
    0x1115, 0x12A4, 0x1458, 0x1633, 0x183A, 0x1A6F, 0x1CD9, 0x1F7B, 0x225A, 0x257D, 0x28E8, 0x2CA4, 0x30B7, 0x3529, 0x3A03, 0x3F4E,
    0x4515, 0x4B62, 0x5244, 0x59C5, 0x61F6, 0x6AE7, 0x74A8, 0x7F4D, 0x8AEB, 0x9798, 0xA56E, 0xB486, 0xC4FF, 0xD6F9, 0xEA97, 0xFFFF,
    0x0001, 0x0001, 0x1569, 0x2907, 0x3B01, 0x4B7A, 0x5A92, 0x6868, 0x7515, 0x80B3, 0x8B58, 0x9519, 0x9E0A, 0xA63B, 0xADBC, 0xB49E,
    0xBAEB, 0xC0B2, 0xC5FD, 0xCAD7, 0xCF49, 0xD35C, 0xD718, 0xDA83, 0xDDA6, 0xE085, 0xE327, 0xE591, 0xE7C6, 0xE9CD, 0xEBA8, 0xED5C,
    0xEEEB, 0xF058, 0xF1A8, 0xF2DB, 0xF3F4, 0xF4F6, 0xF5E2, 0xF6BB, 0xF781, 0xF837, 0xF8DE, 0xF977, 0xFA02, 0xFA83, 0xFAF8, 0xFB64,
    0xFBC7, 0xFC21, 0xFC74, 0xFCC0, 0xFD05, 0xFD45, 0xFD80, 0xFDB5, 0xFDE6, 0xFE13, 0xFE3C, 0xFE62, 0xFE85, 0xFEA4, 0xFEC2, 0xFEDC,
    0xFEF5, 0xFF0B, 0xFF1F, 0xFF32, 0xFF43, 0xFF53, 0xFF62, 0xFF6F, 0xFF7B, 0xFF86, 0xFF90, 0xFF9A, 0xFFA2, 0xFFAA, 0xFFB1, 0xFFB8,
    0xFFBE, 0xFFC3, 0xFFC8, 0xFFCD, 0xFFD1, 0xFFD5, 0xFFD6, 0xFFD7, 0xFFD8, 0xFFD9, 0xFFDA, 0xFFDB, 0xFFDC, 0xFFDD, 0xFFDE, 0xFFDF,
    0xFFE0, 0xFFE1, 0xFFE2, 0xFFE3, 0xFFE4, 0xFFE5, 0xFFE6, 0xFFE7, 0xFFE8, 0xFFE9, 0xFFEA, 0xFFEB, 0xFFEC, 0xFFEE, 0xFFED, 0xFFEF,
    0xFFF0, 0xFFF1, 0xFFF2, 0xFFF3, 0xFFF4, 0xFFF5, 0xFFF6, 0xFFF7, 0xFFF8, 0xFFF9, 0xFFFA, 0xFFFB, 0xFFFC, 0xFFFD, 0xFFFE, 0xFFFF,
];

/// A 4-byte chunk header: body length L and the (ignored) chunk type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkHeader {
    pub length: u16,
    pub kind: u16,
}

/// §2.2: validate the file header (all twenty signature bytes, header size
/// 26, version 256, `version + check = 0x1233`) and return the first chunk
/// header at offset `0x1A`.
pub fn header(bytes: &[u8]) -> Result<ChunkHeader, ParseError> {
    let header = bytes
        .get(..HEADER_BYTES + 4)
        .ok_or_else(|| ParseError::new("MVE", bytes.len(), "header is shorter than 30 bytes"))?;
    if header[..20] != SIGNATURE {
        return Err(ParseError::new("MVE", 0, "bad signature"));
    }
    let word = |at: usize| u16::from_le_bytes([header[at], header[at + 1]]);
    if usize::from(word(0x14)) != HEADER_BYTES {
        return Err(ParseError::new("MVE", 0x14, "header size must be 26"));
    }
    if word(0x16) != VERSION {
        return Err(ParseError::new("MVE", 0x16, "version must be 256"));
    }
    if word(0x16).wrapping_add(word(0x18)) != CHECK_SUM {
        return Err(ParseError::new("MVE", 0x18, "check value mismatch"));
    }
    Ok(ChunkHeader {
        length: word(0x1a),
        kind: word(0x1c),
    })
}

/// §3.3 frame period `T = rate × subdivision + (subdivision >> 1)` µs.
/// Safe rule: all 32 bits of the rate are used (the reference keeps only
/// the low 16 bits, which no corpus rate exceeds).
pub fn frame_period(rate: u32, subdivision: u16) -> u64 {
    u64::from(rate) * u64::from(subdivision) + u64::from(subdivision >> 1)
}

/// Opcode 3 audio parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormat {
    pub rate: u16,
    pub stereo: bool,
    pub sixteen_bit: bool,
    /// Flag bit 2, honored only for opcode version 1 or later.
    pub compressed: bool,
    /// The declared buffer size (u32 from version 1, else u16).
    pub buffer: u32,
}
impl AudioFormat {
    /// The device buffer: `floor((size + size/2) / 4) × 4` bytes.
    pub fn device_buffer_bytes(&self) -> u64 {
        let size = u64::from(self.buffer);
        (size + size / 2) / 4 * 4
    }
    pub fn channels(&self) -> usize {
        if self.stereo { 2 } else { 1 }
    }
}

/// §3.8 decoding of one audio data opcode's codec bytes to `length` output
/// bytes. Compressed data restarts its predictors from its own first u16
/// values (one per channel, also the first output sample or pair), then
/// adds `AUDIO_DELTAS[code]` per code byte with wrapping 16-bit arithmetic,
/// channels interleaved left then right. Safe rule: uncompressed data is
/// the payload's `length` bytes unchanged.
pub fn decode_audio(format: &AudioFormat, length: u16, codec: &[u8]) -> Result<Vec<u8>, String> {
    let length = usize::from(length);
    if !format.compressed {
        return codec
            .get(..length)
            .map(<[u8]>::to_vec)
            .ok_or_else(|| "uncompressed audio is shorter than its decoded length".into());
    }
    let channels = format.channels();
    let head = 2 * channels;
    if length < head {
        return Err("decoded length is shorter than the initial predictors".into());
    }
    let codes = (length - head) / head * channels;
    let input = codec
        .get(..head + codes)
        .ok_or("audio codes run past the opcode payload")?;
    let mut predictors = [0_u16; 2];
    let mut out = Vec::with_capacity(head + 2 * codes);
    for (channel, predictor) in predictors.iter_mut().take(channels).enumerate() {
        *predictor = u16::from_le_bytes([input[2 * channel], input[2 * channel + 1]]);
        out.extend(predictor.to_le_bytes());
    }
    for (index, code) in input[head..].iter().enumerate() {
        let predictor = &mut predictors[index % channels];
        *predictor = predictor.wrapping_add(AUDIO_DELTAS[usize::from(*code)]);
        out.extend(predictor.to_le_bytes());
    }
    Ok(out)
}

/// §3.5 types 2 and 3: the near motion offset coded by B.
pub fn near_motion(code: u8) -> (i32, i32) {
    let code = i32::from(code);
    if code < 56 {
        (8 + code % 7, code / 7)
    } else {
        (-14 + (code - 56) % 29, 8 + (code - 56) / 29)
    }
}
/// §3.5 type 4: the motion offset into the other buffer.
pub fn far_motion(code: u8) -> (i32, i32) {
    (i32::from(code & 15) - 8, i32::from(code >> 4) - 8)
}

/// Bit `k` of a byte sequence, least-significant first.
fn bit(bytes: &[u8], k: usize) -> bool {
    (bytes[k / 8] >> (k % 8)) & 1 != 0
}
/// The 2-bit value of cell `c`: bits `2c` and `2c + 1`.
fn pair(bytes: &[u8], c: usize) -> usize {
    usize::from((bytes[c / 4] >> (2 * (c % 4))) & 3)
}

/// The block parameter stream shared by one video data opcode.
struct Params<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Params<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let data = self
            .bytes
            .get(self.pos..self.pos + count)
            .ok_or("block parameters run past the video payload")?;
        self.pos += count;
        Ok(data)
    }
    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn peek(&self, count: usize) -> Result<&'a [u8], String> {
        self.bytes
            .get(self.pos..self.pos + count)
            .ok_or_else(|| "block parameters run past the video payload".into())
    }
}

/// Two-color bitmap: a set bit selects `colors[1]`.
fn two(colors: &[u8], set: bool) -> u8 {
    colors[usize::from(set)]
}

/// The 64 pixels of a pattern block (types 7-15), row-major.
fn pattern(kind: u8, p: &mut Params) -> Result<[u8; 64], String> {
    let mut px = [0_u8; 64];
    // Quadrant origins in parameter order: top-left, bottom-left,
    // top-right, bottom-right.
    const QUADRANTS: [(usize, usize); 4] = [(0, 0), (0, 4), (4, 0), (4, 4)];
    match kind {
        7 => {
            let colors = p.take(2)?;
            if colors[0] <= colors[1] {
                let bits = p.take(8)?;
                for (k, pixel) in px.iter_mut().enumerate() {
                    *pixel = two(colors, bit(bits, k));
                }
            } else {
                let bits = p.take(2)?;
                for (k, pixel) in px.iter_mut().enumerate() {
                    let (i, j) = (k % 8, k / 8);
                    *pixel = two(colors, bit(bits, 4 * (j / 2) + i / 2));
                }
            }
        }
        8 => {
            let head = p.peek(2)?;
            if head[0] <= head[1] {
                for (qx, qy) in QUADRANTS {
                    let group = p.take(4)?;
                    for j in 0..4 {
                        for i in 0..4 {
                            px[(qy + j) * 8 + qx + i] = two(group, bit(&group[2..], 4 * j + i));
                        }
                    }
                }
            } else {
                let all = p.take(12)?;
                let halves = [&all[..6], &all[6..]];
                let horizontal = all[6] > all[7];
                for (h, half) in halves.iter().enumerate() {
                    let bits = &half[2..];
                    if horizontal {
                        for j in 0..4 {
                            for i in 0..8 {
                                px[(4 * h + j) * 8 + i] = two(half, bit(bits, 8 * j + i));
                            }
                        }
                    } else {
                        for j in 0..8 {
                            for i in 0..4 {
                                px[j * 8 + 4 * h + i] = two(half, bit(bits, 4 * j + i));
                            }
                        }
                    }
                }
            }
        }
        9 => {
            let colors = p.take(4)?;
            match (colors[0] <= colors[1], colors[2] <= colors[3]) {
                (true, true) => {
                    let bits = p.take(16)?;
                    for (k, pixel) in px.iter_mut().enumerate() {
                        *pixel = colors[pair(bits, k)];
                    }
                }
                (true, false) => {
                    let bits = p.take(4)?;
                    for (k, pixel) in px.iter_mut().enumerate() {
                        let (i, j) = (k % 8, k / 8);
                        *pixel = colors[pair(bits, 4 * (j / 2) + i / 2)];
                    }
                }
                (false, true) => {
                    let bits = p.take(8)?;
                    for (k, pixel) in px.iter_mut().enumerate() {
                        let (i, j) = (k % 8, k / 8);
                        *pixel = colors[pair(bits, 4 * j + i / 2)];
                    }
                }
                (false, false) => {
                    let bits = p.take(8)?;
                    for (k, pixel) in px.iter_mut().enumerate() {
                        let (i, j) = (k % 8, k / 8);
                        *pixel = colors[pair(bits, 8 * (j / 2) + i)];
                    }
                }
            }
        }
        10 => {
            let head = p.peek(2)?;
            if head[0] <= head[1] {
                for (qx, qy) in QUADRANTS {
                    let group = p.take(8)?;
                    for j in 0..4 {
                        for i in 0..4 {
                            px[(qy + j) * 8 + qx + i] = group[pair(&group[4..], 4 * j + i)];
                        }
                    }
                }
            } else {
                let all = p.take(24)?;
                let halves = [&all[..12], &all[12..]];
                let horizontal = all[12] > all[13];
                for (h, half) in halves.iter().enumerate() {
                    let bits = &half[4..];
                    if horizontal {
                        for j in 0..4 {
                            for i in 0..8 {
                                px[(4 * h + j) * 8 + i] = half[pair(bits, 8 * j + i)];
                            }
                        }
                    } else {
                        for j in 0..8 {
                            for i in 0..4 {
                                px[j * 8 + 4 * h + i] = half[pair(bits, 4 * j + i)];
                            }
                        }
                    }
                }
            }
        }
        11 => px.copy_from_slice(p.take(64)?),
        12 => {
            let cells = p.take(16)?;
            for (k, pixel) in px.iter_mut().enumerate() {
                let (i, j) = (k % 8, k / 8);
                *pixel = cells[4 * (j / 2) + i / 2];
            }
        }
        13 => {
            let quadrants = p.take(4)?;
            for (k, pixel) in px.iter_mut().enumerate() {
                let (i, j) = (k % 8, k / 8);
                *pixel = quadrants[(j / 4) * 2 + i / 4];
            }
        }
        14 => px.fill(p.byte()?),
        15 => {
            let colors = p.take(2)?;
            for (k, pixel) in px.iter_mut().enumerate() {
                *pixel = colors[(k % 8 + k / 8) % 2];
            }
        }
        _ => unreachable!("pattern types are 7-15"),
    }
    Ok(px)
}

/// §3.4 the two 8-bit frame buffers. New buffers are zeroed (safe rule).
#[derive(Debug, Clone)]
pub struct VideoBuffers {
    /// Width and height in 8 x 8 blocks.
    pub blocks: [usize; 2],
    buffers: [Vec<u8>; 2],
    current: usize,
}
impl VideoBuffers {
    pub fn new(width_blocks: usize, height_blocks: usize) -> Self {
        let size = width_blocks * 8 * height_blocks * 8;
        Self {
            blocks: [width_blocks, height_blocks],
            buffers: [vec![0; size], vec![0; size]],
            current: 0,
        }
    }
    /// Frame size in pixels.
    pub fn size(&self) -> [usize; 2] {
        [self.blocks[0] * 8, self.blocks[1] * 8]
    }
    /// The buffer being decoded and displayed.
    pub fn current(&self) -> &[u8] {
        &self.buffers[self.current]
    }
    /// The previous frame.
    pub fn other(&self) -> &[u8] {
        &self.buffers[1 - self.current]
    }
    pub fn current_mut(&mut self) -> &mut [u8] {
        &mut self.buffers[self.current]
    }
    pub fn other_mut(&mut self) -> &mut [u8] {
        &mut self.buffers[1 - self.current]
    }
    /// Exchange the roles of the two buffers.
    pub fn swap(&mut self) {
        self.current = 1 - self.current;
    }
    /// Decode the region `[x, y, w, h]` (in blocks) into the current
    /// buffer: one map nibble per block in raster order (low nibble first),
    /// parameters consumed from one stream. Blocks outside the region keep
    /// their contents. Returns the parameter bytes consumed and counts each
    /// decoded block type.
    pub fn decode(
        &mut self,
        map: &[u8],
        params: &[u8],
        region: [usize; 4],
        counts: &mut [u64; 16],
    ) -> Result<usize, String> {
        let [x, y, w, h] = region;
        if w % 2 != 0 {
            return Err("odd region width (map bytes cover two blocks)".into());
        }
        if x + w > self.blocks[0] || y + h > self.blocks[1] {
            return Err("decode region lies outside the frame".into());
        }
        if map.len() < w * h / 2 {
            return Err("decoding map is shorter than the region".into());
        }
        let mut stream = Params {
            bytes: params,
            pos: 0,
        };
        for row in 0..h {
            for col in 0..w {
                let n = row * w + col;
                let kind = (map[n / 2] >> (4 * (n % 2))) & 0x0f;
                counts[usize::from(kind)] += 1;
                self.block(kind, x + col, y + row, &mut stream)?;
            }
        }
        Ok(stream.pos)
    }
    fn block(&mut self, kind: u8, bx: usize, by: usize, p: &mut Params) -> Result<(), String> {
        let width = self.blocks[0] * 8;
        let origin = by * 8 * width + bx * 8;
        match kind {
            0 => self.copy(false, origin, (0, 0)),
            1 => Ok(()),
            2 => self.copy(true, origin, near_motion(p.byte()?)),
            3 => {
                let (dx, dy) = near_motion(p.byte()?);
                self.copy(true, origin, (-dx, -dy))
            }
            4 => self.copy(false, origin, far_motion(p.byte()?)),
            5 => {
                let offset = p.take(2)?;
                self.copy(
                    false,
                    origin,
                    (i32::from(offset[0] as i8), i32::from(offset[1] as i8)),
                )
            }
            // Safe rule: the reference's skip counts can run past rows and
            // wrap; type 6 ends the frame with a stream error.
            6 => Err("block type 6 is not supported".into()),
            _ => {
                let pixels = pattern(kind, p)?;
                let current = self.current_mut();
                for j in 0..8 {
                    let at = origin + j * width;
                    current[at..at + 8].copy_from_slice(&pixels[j * 8..j * 8 + 8]);
                }
                Ok(())
            }
        }
    }
    /// An 8 x 8 motion copy with linear source addressing
    /// `(Y + dy) × width + (X + dx)`, row by row from the top, four bytes at
    /// a time. A source outside the buffer is a stream error (safe rule).
    fn copy(
        &mut self,
        from_current: bool,
        origin: usize,
        (dx, dy): (i32, i32),
    ) -> Result<(), String> {
        let width = self.blocks[0] * 8;
        let source = origin as i64 + i64::from(dy) * width as i64 + i64::from(dx);
        let last = source + 7 * width as i64 + 7;
        if source < 0 || last >= self.buffers[0].len() as i64 {
            return Err("motion source lies outside the frame buffer".into());
        }
        let source = source as usize;
        let (first, second) = self.buffers.split_at_mut(1);
        let (current, other) = if self.current == 0 {
            (&mut first[0], &second[0])
        } else {
            (&mut second[0], &first[0])
        };
        for j in 0..8 {
            for half in [0, 4] {
                let (from, to) = (source + j * width + half, origin + j * width + half);
                let chunk: [u8; 4] = if from_current {
                    current[from..from + 4].try_into().expect("four bytes")
                } else {
                    other[from..from + 4].try_into().expect("four bytes")
                };
                current[to..to + 4].copy_from_slice(&chunk);
            }
        }
        Ok(())
    }
}

/// One processed show-frame opcode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShownFrame {
    /// The 1-based frame counter after this opcode.
    pub number: u32,
    pub palette_start: u16,
    pub palette_count: u16,
    /// A video data opcode was decoded since the previous shown frame.
    pub video: bool,
    /// The chunk carries a suppress-display opcode (14).
    pub suppressed: bool,
    /// The field-mode flag (safe rule: displayed normally).
    pub field_mode: bool,
}

/// What a processed opcode asks of the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Opcode 2: the frame timer's rate (µs) and subdivision.
    Timer { rate: u32, subdivision: u16 },
    /// Opcode 3.
    AudioFormat(AudioFormat),
    /// Opcode 4: wait for the frame deadline.
    Sync,
    /// Opcode 8 (`silence` false) or 9 selected by track-mask bit 0: the
    /// decoded output bytes (16-bit samples are little-endian).
    Audio { silence: bool, pcm: Vec<u8> },
    /// Opcode 7.
    Frame(ShownFrame),
    /// Opcode 0: playback finished normally.
    Finished,
}

/// Counters a corpus audit compares with the oracle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovieStats {
    /// Chunk records read, the first included.
    pub chunks: u32,
    pub video_frames: u32,
    pub block_counts: [u64; 16],
    pub opcode_counts: [u64; 256],
}

/// An open movie: the container cursor and the decoder state.
#[derive(Debug, Clone)]
pub struct Movie {
    data: Vec<u8>,
    /// The next opcode in the current chunk body.
    cursor: usize,
    /// The end of the current body, where the next chunk header begins.
    body_end: usize,
    video: Option<VideoBuffers>,
    palette: [u8; 768],
    /// The current chunk's decoding map (payload range).
    map: Option<(usize, usize)>,
    suppress: bool,
    /// The most recent decode region `[x, y, w, h]` in blocks.
    region: [usize; 4],
    video_since_frame: bool,
    frames: u32,
    audio: Option<AudioFormat>,
    finished: bool,
    stats: MovieStats,
}

fn word(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

impl Movie {
    /// §3.1: validate the header and read the first chunk record. The frame
    /// counter starts at 0 and audio track 0 is selected.
    pub fn open(data: Vec<u8>) -> Result<Self, ParseError> {
        if data.len() > MAX_ASSET_BYTES {
            return Err(ParseError::new("MVE", 0, "asset exceeds 64 MiB limit"));
        }
        header(&data)?;
        let mut movie = Self {
            data,
            cursor: 0,
            body_end: 0,
            video: None,
            palette: [0; 768],
            map: None,
            suppress: false,
            region: [0; 4],
            video_since_frame: false,
            frames: 0,
            audio: None,
            finished: false,
            stats: MovieStats {
                chunks: 0,
                video_frames: 0,
                block_counts: [0; 16],
                opcode_counts: [0; 256],
            },
        };
        movie.fetch(HEADER_BYTES)?;
        Ok(movie)
    }
    /// Read the chunk record whose header is at `at`: its body plus the next
    /// chunk's header must be present. The map and suppress marker reset.
    fn fetch(&mut self, at: usize) -> Result<(), ParseError> {
        let length = usize::from(word(&self.data, at));
        let body = at + 4;
        if body + length + 4 > self.data.len() {
            return Err(ParseError::new(
                "MVE",
                body,
                format!("chunk record of {} bytes is truncated", length + 4),
            ));
        }
        self.cursor = body;
        self.body_end = body + length;
        self.map = None;
        self.suppress = false;
        self.stats.chunks += 1;
        Ok(())
    }
    pub fn frames(&self) -> u32 {
        self.frames
    }
    pub fn stats(&self) -> &MovieStats {
        &self.stats
    }
    pub fn audio_format(&self) -> Option<AudioFormat> {
        self.audio
    }
    /// The 256 x 3 table of 6-bit levels filled by palette opcodes.
    pub fn palette_table(&self) -> &[u8; 768] {
        &self.palette
    }
    pub fn video(&self) -> Option<&VideoBuffers> {
        self.video.as_ref()
    }
    /// The most recent decode region `[x, y, w, h]` in blocks.
    pub fn region(&self) -> [usize; 4] {
        self.region
    }
    pub fn finished(&self) -> bool {
        self.finished
    }
    /// Process opcodes in stream order up to the next event. Every error
    /// ends the movie (later calls report `Finished`).
    pub fn next_event(&mut self) -> Result<Event, ParseError> {
        if self.finished {
            return Ok(Event::Finished);
        }
        let result = self.process();
        if result.is_err() || matches!(result, Ok(Event::Finished)) {
            self.finished = true;
        }
        result
    }
    fn process(&mut self) -> Result<Event, ParseError> {
        loop {
            let at = self.cursor;
            if at + 4 > self.body_end {
                return Err(ParseError::new(
                    "MVE",
                    at,
                    "opcode header runs past the chunk body",
                ));
            }
            let length = usize::from(word(&self.data, at));
            let (kind, version) = (self.data[at + 2], self.data[at + 3]);
            let start = at + 4;
            let end = start + length;
            if end > self.body_end {
                return Err(ParseError::new(
                    "MVE",
                    at,
                    format!("opcode {kind} payload runs past the chunk body"),
                ));
            }
            self.cursor = end;
            self.stats.opcode_counts[usize::from(kind)] += 1;
            let fail = |message: &str| {
                ParseError::new(
                    "MVE",
                    at,
                    format!("opcode {kind} at version {version}: {message}"),
                )
            };
            let need = |count: usize| {
                if length < count {
                    Err(fail("payload is shorter than its fields"))
                } else {
                    Ok(())
                }
            };
            let field = |offset: usize| word(&self.data, start + offset);
            match kind {
                0 => return Ok(Event::Finished),
                1 => self.fetch(self.body_end)?,
                2 => {
                    need(6)?;
                    return Ok(Event::Timer {
                        rate: u32::from(field(0)) | (u32::from(field(2)) << 16),
                        subdivision: field(4),
                    });
                }
                3 => {
                    need(if version >= 1 { 10 } else { 8 })?;
                    let flags = field(2);
                    let format = AudioFormat {
                        rate: field(4),
                        stereo: flags & 1 != 0,
                        sixteen_bit: flags & 2 != 0,
                        compressed: version >= 1 && flags & 4 != 0,
                        buffer: if version >= 1 {
                            u32::from(field(6)) | (u32::from(field(8)) << 16)
                        } else {
                            u32::from(field(6))
                        },
                    };
                    self.audio = Some(format);
                    return Ok(Event::AudioFormat(format));
                }
                4 => return Ok(Event::Sync),
                5 => {
                    need(match version {
                        0 => 4,
                        1 => 6,
                        _ => 8,
                    })?;
                    let (width, height) = (usize::from(field(0)), usize::from(field(2)));
                    let multiplier = if version >= 1 { field(4) } else { 1 };
                    let high_color = version >= 2 && field(6) != 0;
                    if high_color {
                        return Err(fail("high-color movies are not supported"));
                    }
                    if multiplier != 1 {
                        return Err(fail("a height multiplier other than 1 is not supported"));
                    }
                    if width * 8 > SURFACE[0] || height * 8 > SURFACE[1] {
                        return Err(fail("frame exceeds the 640 x 480 output surface"));
                    }
                    self.video = Some(VideoBuffers::new(width, height));
                }
                7 => {
                    need(if version >= 1 { 6 } else { 4 })?;
                    let (palette_start, palette_count) = (field(0), field(2));
                    if usize::from(palette_start) + usize::from(palette_count) > 256 {
                        return Err(fail("palette push range exceeds 256 entries"));
                    }
                    self.frames += 1;
                    let frame = ShownFrame {
                        number: self.frames,
                        palette_start,
                        palette_count,
                        video: std::mem::take(&mut self.video_since_frame),
                        suppressed: self.suppress,
                        field_mode: version >= 1 && field(4) != 0,
                    };
                    return Ok(Event::Frame(frame));
                }
                8 | 9 => {
                    need(6)?;
                    let (mask, decoded) = (field(2), field(4));
                    // Without an audio configuration audio is ignored.
                    let Some(format) = self.audio else { continue };
                    if mask & 1 == 0 {
                        continue;
                    }
                    let pcm = if kind == 9 {
                        vec![if format.sixteen_bit { 0x00 } else { 0x80 }; usize::from(decoded)]
                    } else {
                        decode_audio(&format, decoded, &self.data[start + 6..end])
                            .map_err(|message| fail(&message))?
                    };
                    return Ok(Event::Audio {
                        silence: kind == 9,
                        pcm,
                    });
                }
                12 => {
                    need(4)?;
                    let (first, count) = (usize::from(field(0)), usize::from(field(2)));
                    if first + count > 256 {
                        return Err(fail("palette range exceeds 256 entries"));
                    }
                    need(4 + 3 * count)?;
                    self.palette[3 * first..3 * (first + count)]
                        .copy_from_slice(&self.data[start + 4..start + 4 + 3 * count]);
                }
                14 => self.suppress = true,
                15 => self.map = Some((start, end)),
                17 => {
                    if version < 3 {
                        return Err(fail("video data needs version 3 or later"));
                    }
                    need(14)?;
                    let region = [field(4), field(6), field(8), field(10)].map(usize::from);
                    let swap = field(12) & 1 != 0;
                    let (map_start, map_end) = self
                        .map
                        .ok_or_else(|| fail("no decoding map in this chunk"))?;
                    let video = self
                        .video
                        .as_mut()
                        .ok_or_else(|| fail("video data before the video configuration"))?;
                    if swap {
                        video.swap();
                    }
                    video
                        .decode(
                            &self.data[map_start..map_end],
                            &self.data[start + 14..end],
                            region,
                            &mut self.stats.block_counts,
                        )
                        .map_err(|message| fail(&message))?;
                    self.region = region;
                    self.video_since_frame = true;
                    self.stats.video_frames += 1;
                }
                // 10 video mode, 11 synthesized palette (safe rule: ignored),
                // and unknown types are skipped.
                _ => {}
            }
        }
    }
}

/// §3.7 palette push: entries `start .. start + count` of the table reach
/// the device palette; the others keep their values. Safe rule: the range
/// is clipped to the 256 entries, so a start at or past the end pushes
/// nothing instead of indexing outside the palettes.
pub fn push_palette(table: &[u8; 768], device: &mut [u8; 768], start: u16, count: u16) {
    let start = usize::from(start).min(256);
    let end = (start + usize::from(count)).min(256);
    let range = 3 * start..3 * end;
    device[range.clone()].copy_from_slice(&table[range]);
}
