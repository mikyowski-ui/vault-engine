//! Zero-dependency image writers for the gallery: indexed PNG (RFC 1950 /
//! 1951 compression with fixed Huffman codes and greedy LZ77) and looping
//! GIF89a animations (LZW, each frame only the rectangle that changed).
//! Both keep the frames' own 256-entry palettes, so nothing is dithered or
//! quantized.

/// An indexed picture: `width` by `height` palette indexes, row-major.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Indexed {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// LSB-first bit packing, as DEFLATE and GIF's LZW both store codes.
#[derive(Default)]
struct Bits {
    out: Vec<u8>,
    word: u32,
    count: u32,
}
impl Bits {
    fn put(&mut self, value: u32, count: u32) {
        self.word |= value << self.count;
        self.count += count;
        while self.count >= 8 {
            self.out.push(self.word as u8);
            self.word >>= 8;
            self.count -= 8;
        }
    }
    /// A Huffman code, stored most significant bit first.
    fn put_code(&mut self, code: u32, length: u32) {
        self.put(code.reverse_bits() >> (32 - length), length);
    }
    fn finish(mut self) -> Vec<u8> {
        if self.count > 0 {
            self.out.push(self.word as u8);
        }
        self.out
    }
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DISTANCE_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DISTANCE_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const WINDOW: usize = 32768;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
/// How many earlier positions a match search tries.
const CHAIN: usize = 128;

/// RFC 1951 §3.2.6's fixed literal/length code.
fn literal(bits: &mut Bits, symbol: u32) {
    match symbol {
        0..=143 => bits.put_code(0x30 + symbol, 8),
        144..=255 => bits.put_code(0x190 + symbol - 144, 9),
        256..=279 => bits.put_code(symbol - 256, 7),
        _ => bits.put_code(0xC0 + symbol - 280, 8),
    }
}

/// The last table entry whose base is at most `value`.
fn bucket(bases: &[u16], value: usize) -> usize {
    bases
        .iter()
        .rposition(|base| usize::from(*base) <= value)
        .unwrap_or(0)
}

/// `data` as one final fixed-Huffman DEFLATE block (RFC 1951), matched
/// greedily over a 32 KiB window through hash chains of three bytes.
pub fn deflate(data: &[u8]) -> Vec<u8> {
    let mut bits = Bits::default();
    bits.put(1, 1);
    bits.put(1, 2);
    let hash = |at: usize| {
        (usize::from(data[at]) << 10 ^ usize::from(data[at + 1]) << 5 ^ usize::from(data[at + 2]))
            & 0x7fff
    };
    let mut head = vec![usize::MAX; 0x8000];
    let mut prev = vec![usize::MAX; data.len()];
    let insert = |head: &mut [usize], prev: &mut [usize], at: usize| {
        if at + MIN_MATCH <= data.len() {
            let key = hash(at);
            prev[at] = head[key];
            head[key] = at;
        }
    };
    let mut at = 0;
    while at < data.len() {
        let mut best = (0, 0);
        if at + MIN_MATCH <= data.len() {
            let mut candidate = head[hash(at)];
            let limit = (data.len() - at).min(MAX_MATCH);
            for _ in 0..CHAIN {
                if candidate == usize::MAX || at - candidate > WINDOW {
                    break;
                }
                let length = data[candidate..]
                    .iter()
                    .zip(&data[at..at + limit])
                    .take_while(|(a, b)| a == b)
                    .count();
                if length > best.0 {
                    best = (length, at - candidate);
                    if length == limit {
                        break;
                    }
                }
                candidate = prev[candidate];
            }
        }
        if best.0 >= MIN_MATCH {
            let (length, distance) = best;
            let code = bucket(&LENGTH_BASE, length);
            literal(&mut bits, 257 + code as u32);
            bits.put(
                (length - usize::from(LENGTH_BASE[code])) as u32,
                u32::from(LENGTH_EXTRA[code]),
            );
            let code = bucket(&DISTANCE_BASE, distance);
            bits.put_code(code as u32, 5);
            bits.put(
                (distance - usize::from(DISTANCE_BASE[code])) as u32,
                u32::from(DISTANCE_EXTRA[code]),
            );
            for position in at..at + length {
                insert(&mut head, &mut prev, position);
            }
            at += length;
        } else {
            literal(&mut bits, u32::from(data[at]));
            insert(&mut head, &mut prev, at);
            at += 1;
        }
    }
    literal(&mut bits, 256);
    bits.finish()
}

/// `data` in a zlib stream (RFC 1950): the header, [`deflate`] and the
/// Adler-32 checksum.
pub fn zlib(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    out.extend(deflate(data));
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    out.extend(((b << 16) | a).to_be_bytes());
    out
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// An indexed-colour PNG of `image` under `palette` (8-bit RGB entries).
pub fn png(image: &Indexed, palette: &[[u8; 3]; 256]) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend(kind);
        out.extend(data);
        let crc = crc32(&out[start..]);
        out.extend(crc.to_be_bytes());
    };
    let mut header = Vec::with_capacity(13);
    header.extend(image.width.to_be_bytes());
    header.extend(image.height.to_be_bytes());
    // Bit depth 8, colour type 3 (indexed), deflate, filter 0, no interlace.
    header.extend([8, 3, 0, 0, 0]);
    chunk(b"IHDR", &header);
    chunk(b"PLTE", palette.as_flattened());
    let width = image.width as usize;
    let mut rows = Vec::with_capacity((width + 1) * image.height as usize);
    for row in image.pixels.chunks_exact(width) {
        // Filter type 0: indexed rows compress best unfiltered.
        rows.push(0);
        rows.extend(row);
    }
    chunk(b"IDAT", &zlib(&rows));
    chunk(b"IEND", &[]);
    out
}

/// GIF's variable-length LZW (codes 9 to 12 bits over 8-bit pixels, with a
/// clear code first and whenever the 4096-entry table fills).
fn lzw(pixels: &[u8]) -> Vec<u8> {
    const CLEAR: u32 = 256;
    const END: u32 = 257;
    let mut bits = Bits::default();
    let mut table = vec![0u16; 4096 * 256];
    let mut size = 9;
    let mut next = 258;
    bits.put(CLEAR, size);
    let Some((&first, rest)) = pixels.split_first() else {
        bits.put(END, size);
        return bits.finish();
    };
    let mut prefix = u32::from(first);
    for &byte in rest {
        let key = prefix as usize * 256 + usize::from(byte);
        if table[key] != 0 {
            prefix = u32::from(table[key]);
            continue;
        }
        bits.put(prefix, size);
        if next >= 1 << size && size < 12 {
            size += 1;
        }
        if next >= 4095 {
            bits.put(CLEAR, size);
            table.fill(0);
            size = 9;
            next = 258;
        } else {
            table[key] = next as u16;
            next += 1;
        }
        prefix = u32::from(byte);
    }
    bits.put(prefix, size);
    if next >= 1 << size && size < 12 {
        size += 1;
    }
    bits.put(END, size);
    bits.finish()
}

/// A looping GIF89a of `frames` (all one size), each shown for its delay in
/// hundredths of a second, under one global `palette`. After the first, a
/// frame stores only the rectangle where it differs from the one before
/// (drawn over it), and a frame identical to the one before lengthens that
/// one's delay instead.
pub fn gif(frames: &[(Indexed, u16)], palette: &[[u8; 3]; 256]) -> Vec<u8> {
    let Some((first, _)) = frames.first() else {
        return Vec::new();
    };
    let (width, height) = (first.width, first.height);
    let mut out = b"GIF89a".to_vec();
    out.extend((width as u16).to_le_bytes());
    out.extend((height as u16).to_le_bytes());
    // A 256-entry global colour table, 8 bits of colour resolution.
    out.extend([0xF7, 0, 0]);
    out.extend(palette.as_flattened());
    // NETSCAPE2.0: loop forever.
    out.extend([0x21, 0xFF, 0x0B]);
    out.extend(b"NETSCAPE2.0");
    out.extend([0x03, 0x01, 0x00, 0x00, 0x00]);
    // Each stored frame: its rectangle, its pixels and its delay.
    let mut stored: Vec<([u32; 4], Vec<u8>, u16)> = Vec::new();
    let mut previous: Option<&Indexed> = None;
    for (frame, delay) in frames {
        assert_eq!((frame.width, frame.height), (width, height));
        let rect = match previous {
            None => Some([0, 0, width, height]),
            Some(before) => changed(before, frame),
        };
        match (rect, stored.last_mut()) {
            (None, Some(last)) => last.2 = last.2.saturating_add(*delay),
            (rect, _) => {
                let [x, y, w, h] = rect.unwrap_or([0, 0, 1, 1]);
                let mut pixels = Vec::with_capacity((w * h) as usize);
                for row in y..y + h {
                    let start = (row * width + x) as usize;
                    pixels.extend(&frame.pixels[start..start + w as usize]);
                }
                stored.push(([x, y, w, h], pixels, *delay));
            }
        }
        previous = Some(frame);
    }
    for ([x, y, w, h], pixels, delay) in stored {
        // Graphic control: leave the frame in place, no transparency.
        out.extend([0x21, 0xF9, 0x04, 0x04]);
        out.extend(delay.to_le_bytes());
        out.extend([0x00, 0x00]);
        out.push(0x2C);
        for value in [x, y, w, h] {
            out.extend((value as u16).to_le_bytes());
        }
        out.extend([0x00, 0x08]);
        for block in lzw(&pixels).chunks(255) {
            out.push(block.len() as u8);
            out.extend(block);
        }
        out.push(0x00);
    }
    out.push(0x3B);
    out
}

/// The smallest rectangle `[x, y, w, h]` holding every pixel where `after`
/// differs from `before`, or `None` when they are the same.
fn changed(before: &Indexed, after: &Indexed) -> Option<[u32; 4]> {
    let width = before.width as usize;
    let mut bounds: Option<[usize; 4]> = None;
    for (row, (a, b)) in before
        .pixels
        .chunks_exact(width)
        .zip(after.pixels.chunks_exact(width))
        .enumerate()
    {
        let Some(left) = a.iter().zip(b).position(|(a, b)| a != b) else {
            continue;
        };
        let right = width
            - 1
            - a.iter()
                .rev()
                .zip(b.iter().rev())
                .position(|(a, b)| a != b)?;
        bounds = Some(match bounds {
            None => [left, row, right, row],
            Some([x0, y0, x1, _]) => [x0.min(left), y0, x1.max(right), row],
        });
    }
    bounds.map(|[x0, y0, x1, y1]| {
        [
            x0 as u32,
            y0 as u32,
            (x1 - x0 + 1) as u32,
            (y1 - y0 + 1) as u32,
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test pattern with runs, repeats and noise.
    fn pattern(length: usize, seed: u32) -> Vec<u8> {
        let mut state = seed;
        (0..length)
            .map(|at| {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                match at % 700 {
                    0..=299 => (at / 40) as u8,
                    300..=499 => b"vault engine "[at % 13],
                    _ => (state >> 16) as u8,
                }
            })
            .collect()
    }

    #[test]
    fn zlib_streams_inflate_back_through_the_engine_decoder() {
        for data in [
            Vec::new(),
            vec![7],
            vec![0; 70_000],
            pattern(200_000, 1),
            (0..=255).collect::<Vec<u8>>(),
        ] {
            let stream = zlib(&data);
            let back = vault_formats::inflate::zlib_decompress(&stream, data.len()).unwrap();
            assert_eq!(back, data);
        }
        // Repetitive data compresses.
        assert!(zlib(&vec![3; 100_000]).len() < 1000);
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }

    /// GIF's LZW decoding, for the tests.
    fn unlzw(data: &[u8]) -> Vec<u8> {
        let (mut word, mut count, mut at) = (0u32, 0u32, 0usize);
        let mut size = 9;
        let mut table: Vec<Vec<u8>> = (0..=255u8).map(|byte| vec![byte]).collect();
        table.extend([Vec::new(), Vec::new()]);
        let mut out = Vec::new();
        let mut last: Option<Vec<u8>> = None;
        loop {
            while count < size && at < data.len() {
                word |= u32::from(data[at]) << count;
                count += 8;
                at += 1;
            }
            let code = (word & ((1 << size) - 1)) as usize;
            word >>= size;
            count -= size;
            match code {
                256 => {
                    table.truncate(258);
                    size = 9;
                    last = None;
                    continue;
                }
                257 => return out,
                _ => {}
            }
            let entry = match (table.get(code), &last) {
                (Some(entry), _) => entry.clone(),
                (None, Some(last)) => {
                    let mut entry = last.clone();
                    entry.push(last[0]);
                    entry
                }
                (None, None) => panic!("code {code} before any entry"),
            };
            if let Some(mut last) = last.take() {
                last.push(entry[0]);
                table.push(last);
                if table.len() == 1 << size && size < 12 {
                    size += 1;
                }
            }
            out.extend(&entry);
            last = Some(entry);
        }
    }

    #[test]
    fn lzw_codes_decode_back_across_table_resets() {
        for data in [
            vec![1],
            vec![9; 5000],
            pattern(300_000, 7),
            (0..60_000).map(|at| (at % 251) as u8).collect(),
        ] {
            assert_eq!(unlzw(&lzw(&data)), data);
        }
    }

    #[test]
    fn png_and_gif_have_their_structure() {
        let mut palette = [[0u8; 3]; 256];
        palette[1] = [255, 192, 64];
        let image = Indexed {
            width: 3,
            height: 2,
            pixels: vec![0, 1, 0, 1, 0, 1],
        };
        let file = png(&image, &palette);
        assert_eq!(&file[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&file[12..16], b"IHDR");
        assert_eq!(&file[file.len() - 8..file.len() - 4], b"IEND");
        // A frame like the one before only lengthens its delay; a changed
        // pixel stores a one-pixel rectangle.
        let mut changed_pixel = image.clone();
        changed_pixel.pixels[4] = 1;
        let frames = [(image.clone(), 3), (image.clone(), 3), (changed_pixel, 4)];
        let file = gif(&frames, &palette);
        assert_eq!(&file[..6], b"GIF89a");
        assert_eq!(*file.last().unwrap(), 0x3B);
        let controls: Vec<usize> = file
            .windows(3)
            .enumerate()
            .filter(|(_, window)| *window == [0x21, 0xF9, 0x04])
            .map(|(at, _)| at)
            .collect();
        assert_eq!(controls.len(), 2);
        assert_eq!(
            u16::from_le_bytes([file[controls[0] + 4], file[controls[0] + 5]]),
            6
        );
        let descriptor = controls[1] + 8;
        assert_eq!(file[descriptor], 0x2C);
        assert_eq!(
            &file[descriptor + 1..descriptor + 9],
            &[1, 0, 1, 0, 1, 0, 1, 0]
        );
    }
}
