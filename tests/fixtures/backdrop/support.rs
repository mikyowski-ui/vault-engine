//! Synthetic `.bkd` / `.bkc` builders (`docs/prerendered-backdrop-cutaway.md`
//! sections 3.1-3.3), written by hand so fixtures need no `vault-formats`.
//! All numbers are big-endian; zlib chunks use stored DEFLATE blocks.

/// One elevation's in-memory planes, `width * height` values each.
pub struct Elevation {
    pub elevation: u8,
    pub width: u32,
    pub height: u32,
    /// Image pixel of the origin hex's reference corner.
    pub origin_px: [i32; 2],
    pub origin_hex: u32,
    pub colour: Vec<u8>,
    pub depth: Option<Vec<u16>>,
    /// One flag per pixel.
    pub fade: Option<Vec<bool>>,
    /// 0 none, 1 walls, 2 PIDs, 3 both.
    pub suppression: u8,
    pub pids: Vec<u32>,
}

/// An index and its chunk files: `(elevation, column, row, bytes)`.
pub struct Backdrop {
    pub index: Vec<u8>,
    pub chunks: Vec<(u8, u16, u16, Vec<u8>)>,
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = data.chunks(0xFFFF).collect();
    for (i, block) in blocks.iter().enumerate() {
        out.push(u8::from(i + 1 == blocks.len()));
        let len = block.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(block);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// Build the `.bkd` and every chunk file of `elevations` (ascending, all
/// chunks present) at chunk size `chunk_size` (64, 128, 256 or 512);
/// `zlib` selects compression 1.
pub fn build(chunk_size: u16, zlib: bool, palette_hash: u64, elevations: &[Elevation]) -> Backdrop {
    let s = u32::from(chunk_size);
    let mut index = Vec::new();
    index.extend_from_slice(b"VBKD");
    index.extend_from_slice(&1u16.to_be_bytes());
    index.push(elevations.iter().fold(0, |mask, e| mask | 1 << e.elevation));
    index.push(u8::from(zlib));
    index.extend_from_slice(&chunk_size.to_be_bytes());
    index.extend_from_slice(&[0; 6]);
    index.extend_from_slice(&palette_hash.to_be_bytes());
    let mut chunks = Vec::new();
    for e in elevations {
        let planes = 1 | u8::from(e.depth.is_some()) << 1 | u8::from(e.fade.is_some()) << 2;
        let (columns, rows) = (e.width.div_ceil(s), e.height.div_ceil(s));
        index.extend_from_slice(&[e.elevation, planes, e.suppression, 0]);
        index.extend_from_slice(&e.origin_hex.to_be_bytes());
        for value in e.origin_px {
            index.extend_from_slice(&(value as u32).to_be_bytes());
        }
        index.extend_from_slice(&e.width.to_be_bytes());
        index.extend_from_slice(&e.height.to_be_bytes());
        index.extend_from_slice(&(columns as u16).to_be_bytes());
        index.extend_from_slice(&(rows as u16).to_be_bytes());
        index.extend_from_slice(&(e.pids.len() as u32).to_be_bytes());
        for row in 0..rows {
            for column in 0..columns {
                chunks.push((
                    e.elevation,
                    column as u16,
                    row as u16,
                    chunk(e, s, zlib, planes, column, row),
                ));
            }
        }
    }
    for e in elevations {
        let cells = e.width.div_ceil(s) * e.height.div_ceil(s);
        index.extend(std::iter::repeat_n(1u8, cells as usize));
        for pid in &e.pids {
            index.extend_from_slice(&pid.to_be_bytes());
        }
    }
    Backdrop { index, chunks }
}

fn chunk(e: &Elevation, s: u32, zlib: bool, planes: u8, column: u32, row: u32) -> Vec<u8> {
    let (x0, y0) = (column * s, row * s);
    let (w, h) = ((e.width - x0).min(s), (e.height - y0).min(s));
    let pixel = |x: u32, y: u32| ((y0 + y) * e.width + x0 + x) as usize;
    let mut payload = Vec::new();
    for y in 0..h {
        for x in 0..w {
            payload.push(e.colour[pixel(x, y)]);
        }
    }
    if let Some(depth) = &e.depth {
        for y in 0..h {
            for x in 0..w {
                payload.extend_from_slice(&depth[pixel(x, y)].to_be_bytes());
            }
        }
    }
    if let Some(fade) = &e.fade {
        for y in 0..h {
            for byte in 0..w.div_ceil(8) {
                let mut bits = 0u8;
                for bit in 0..8 {
                    let x = byte * 8 + bit;
                    if x < w && fade[pixel(x, y)] {
                        bits |= 0x80 >> bit;
                    }
                }
                payload.push(bits);
            }
        }
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"VBKC");
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&[e.elevation, planes]);
    out.extend_from_slice(&(column as u16).to_be_bytes());
    out.extend_from_slice(&(row as u16).to_be_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(&if zlib { zlib_stored(&payload) } else { payload });
    out
}

/// Vector B of the brief: elevation 0, 4 by 1 pixels at canvas (100, 100),
/// colour `07 07 07 07`, depth `0000 0000 501D 501D`, fade bits all set,
/// chunk size 64, stored.
pub fn vector_b() -> Backdrop {
    build(
        64,
        false,
        0,
        &[Elevation {
            elevation: 0,
            width: 4,
            height: 1,
            origin_px: [0, 0],
            origin_hex: 0,
            colour: vec![7; 4],
            depth: Some(vec![0, 0, 0x501D, 0x501D]),
            fade: Some(vec![true; 4]),
            suppression: 0,
            pids: Vec::new(),
        }],
    )
}
