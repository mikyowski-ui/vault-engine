use super::*;

fn hex(text: &str) -> Vec<u8> {
    text.split_whitespace()
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect()
}

fn elevation(
    e: u8,
    planes: Planes,
    size: [u32; 2],
    s: u16,
    suppress: Suppression,
) -> BackdropElevation {
    let chunks = [
        size[0].div_ceil(u32::from(s)) as u16,
        size[1].div_ceil(u32::from(s)) as u16,
    ];
    let cells = usize::from(chunks[0]) * usize::from(chunks[1]);
    BackdropElevation {
        elevation: e,
        planes,
        origin_hex: 20100 + i32::from(e),
        origin_px: [-5 * i32::from(e) - 1, 7 * i32::from(e)],
        size,
        chunks,
        present: (0..cells).map(|i| i % 3 != 1).collect(),
        suppress,
    }
}

fn index(compression: u8, elevations: Vec<BackdropElevation>) -> BackdropIndex {
    BackdropIndex {
        version: 1,
        chunk_size: 64,
        palette_hash: 0x0123_4567_89AB_CDEF,
        compression,
        elevations,
    }
}

const COMBOS: [Planes; 4] = [
    Planes {
        depth: false,
        fade: false,
    },
    Planes {
        depth: true,
        fade: false,
    },
    Planes {
        depth: false,
        fade: true,
    },
    Planes {
        depth: true,
        fade: true,
    },
];

/// A patterned chunk for (`column`, `row`) of `e`.
fn chunk_for(idx: &BackdropIndex, e: &BackdropElevation, column: u16, row: u16) -> BackdropChunk {
    let (origin, size) = e.chunk_rect(idx.chunk_size, column, row).unwrap();
    let (w, h) = (usize::from(size[0]), usize::from(size[1]));
    let seed = usize::from(column) * 31 + usize::from(row) * 7 + usize::from(e.elevation);
    BackdropChunk {
        origin,
        size,
        colour: (0..w * h).map(|i| ((i + seed) % 256) as u8).collect(),
        depth: e.planes.depth.then(|| {
            (0..w * h)
                .map(|i| ((i * 13 + seed) % 40001) as u16)
                .collect()
        }),
        fade: e.planes.fade.then(|| {
            (0..h * w.div_ceil(8))
                .map(|i| ((i * 5 + seed) % 256) as u8)
                .collect()
        }),
    }
}

#[test]
fn round_trip_every_plane_combination_and_elevation_count() {
    let masks: [&[u8]; 3] = [&[1], &[0, 2], &[0, 1, 2]];
    for compression in [COMPRESSION_STORED, COMPRESSION_ZLIB] {
        for elevations in masks {
            for planes in COMBOS {
                let list = elevations
                    .iter()
                    .map(|&e| {
                        let suppress = match e {
                            0 => Suppression::None,
                            1 => Suppression::WallsAndPids(vec![5, 0xFFFF_FFFF, 5]),
                            _ => Suppression::Pids(vec![]),
                        };
                        // 130 by 70: a partial last column and row at S = 64.
                        elevation(e, planes, [130, 70], 64, suppress)
                    })
                    .collect();
                let idx = index(compression, list);
                let bytes = idx.to_bytes();
                assert_eq!(BackdropIndex::parse(&bytes).unwrap(), idx);
                for e in &idx.elevations {
                    for row in 0..e.chunks[1] {
                        for column in 0..e.chunks[0] {
                            let chunk = chunk_for(&idx, e, column, row);
                            let bytes = chunk.to_bytes(&idx, e.elevation, column, row);
                            let parsed = BackdropChunk::parse(&bytes, &idx, e.elevation);
                            if e.is_present(column, row) {
                                assert_eq!(parsed.unwrap(), chunk);
                            } else {
                                let err = parsed.unwrap_err();
                                assert_eq!((err.format, err.offset), ("BKC", 8));
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn partial_last_chunk_geometry() {
    let e = elevation(0, Planes::default(), [130, 70], 64, Suppression::None);
    assert_eq!(e.chunks, [3, 2]);
    assert_eq!(e.chunk_rect(64, 0, 0), Some(([0, 0], [64, 64])));
    assert_eq!(e.chunk_rect(64, 2, 0), Some(([128, 0], [2, 64])));
    assert_eq!(e.chunk_rect(64, 2, 1), Some(([128, 64], [2, 6])));
    assert_eq!(e.chunk_rect(64, 3, 0), None);
    assert_eq!(e.chunk_rect(64, 0, 2), None);
    assert_eq!(e.chunk_rect(0, 0, 0), None);
}

#[test]
fn minimal_index_bytes() {
    let bytes = hex("56 42 4B 44 00 01 01 00 00 40 00 00 00 00 00 00 \
         01 02 03 04 05 06 07 08 \
         00 01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 \
         00 00 00 40 00 00 00 40 00 01 00 01 00 00 00 00 \
         01");
    assert_eq!(bytes.len(), 57);
    let idx = BackdropIndex::parse(&bytes).unwrap();
    assert_eq!(
        idx,
        BackdropIndex {
            version: 1,
            chunk_size: 64,
            palette_hash: 0x0102_0304_0506_0708,
            compression: 0,
            elevations: vec![BackdropElevation {
                elevation: 0,
                planes: Planes::default(),
                origin_hex: 0,
                origin_px: [0, 0],
                size: [64, 64],
                chunks: [1, 1],
                present: vec![true],
                suppress: Suppression::None,
            }],
        }
    );
    assert_eq!(idx.to_bytes(), bytes);
}

/// Elevation 1 with all planes, both suppressions and one PID.
const INDEX_E1: &str = "56 42 4B 44 00 01 02 00 00 40 00 00 00 00 00 00 \
    00 00 00 00 00 00 00 00 \
    01 07 03 00 00 00 4E 1F FF FF FF F6 00 00 00 14 \
    00 00 00 03 00 00 00 02 00 01 00 01 00 00 00 01 \
    01 \
    00 00 00 05";

#[test]
fn chunk_bytes_checked_against_the_tables() {
    let idx_bytes = hex(INDEX_E1);
    assert_eq!(idx_bytes.len(), 61);
    let idx = BackdropIndex::parse(&idx_bytes).unwrap();
    let e = &idx.elevations[0];
    assert_eq!(e.elevation, 1);
    assert_eq!(
        e.planes,
        Planes {
            depth: true,
            fade: true
        }
    );
    assert_eq!(e.origin_hex, 19999);
    assert_eq!(e.origin_px, [-10, 20]);
    assert_eq!(e.size, [3, 2]);
    assert_eq!(e.suppress, Suppression::WallsAndPids(vec![5]));
    assert_eq!(idx.to_bytes(), idx_bytes);

    // 3 by 2: colour 6, depth 12, fade 2 rows of one byte = 20 = 0x14.
    let bytes = hex("56 42 4B 43 00 01 01 07 00 00 00 00 00 00 00 14 \
         01 02 03 00 05 06 \
         00 00 00 01 00 02 9C 40 00 00 00 05 \
         A0 40");
    let chunk = BackdropChunk::parse(&bytes, &idx, 1).unwrap();
    assert_eq!(chunk.origin, [0, 0]);
    assert_eq!(chunk.size, [3, 2]);
    assert_eq!(chunk.colour, [1, 2, 3, 0, 5, 6]);
    assert_eq!(chunk.depth.as_deref(), Some(&[0, 1, 2, 40000, 0, 5][..]));
    assert_eq!(chunk.fade.as_deref(), Some(&[0xA0, 0x40][..]));
    let fade: Vec<bool> = (0..2)
        .flat_map(|y| (0..4).map(move |x| (x, y)))
        .map(|(x, y)| chunk.fade_at(x, y))
        .collect();
    // Bit 7 first; x = 3 is outside the chunk.
    assert_eq!(fade, [true, false, true, false, false, true, false, false]);
    assert!(!chunk.fade_at(0, 2));
    assert_eq!(chunk.to_bytes(&idx, 1, 0, 0), bytes);
}

#[test]
fn fade_at_without_plane_and_with_padding() {
    let none = BackdropChunk {
        origin: [0, 0],
        size: [2, 1],
        colour: vec![0; 2],
        depth: None,
        fade: None,
    };
    assert!(!none.fade_at(0, 0));
    // 9 wide: two bytes per row, bit 7 of the second byte is x = 8.
    let wide = BackdropChunk {
        origin: [0, 0],
        size: [9, 2],
        colour: vec![0; 18],
        depth: None,
        fade: Some(vec![0x01, 0x80, 0x80, 0x00]),
    };
    assert!(wide.fade_at(7, 0) && wide.fade_at(8, 0) && wide.fade_at(0, 1));
    assert!(!wide.fade_at(6, 0) && !wide.fade_at(8, 1) && !wide.fade_at(9, 0));
}

#[test]
fn helpers() {
    assert_eq!(depth_group(0), None);
    assert_eq!(depth_group(1), Some(0));
    assert_eq!(depth_group(20504), Some(20503));
    assert_eq!(depth_group(40000), Some(39999));
    assert_eq!(depth_group(40001), None);
    assert_eq!(depth_group(u16::MAX), None);
    assert_eq!(index_path("Arroyo"), "maps/arroyo.bkd");
    assert_eq!(chunk_path("ARroyo", 2, 10, 0), "maps/arroyo/e2/10_0.bkc");
    let e = elevation(0, Planes::default(), [130, 70], 64, Suppression::None);
    // present = [true, false, true, true, false, true] for 3 by 2.
    assert!(e.is_present(0, 0) && !e.is_present(1, 0) && e.is_present(2, 0));
    assert!(e.is_present(0, 1) && !e.is_present(1, 1) && e.is_present(2, 1));
    assert!(!e.is_present(3, 0) && !e.is_present(0, 2));
}

fn err(result: Result<impl std::fmt::Debug, ParseError>) -> (&'static str, usize) {
    let e = result.unwrap_err();
    (e.format, e.offset)
}

fn minimal() -> Vec<u8> {
    hex("56 42 4B 44 00 01 01 00 00 40 00 00 00 00 00 00 \
         01 02 03 04 05 06 07 08 \
         00 01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 \
         00 00 00 40 00 00 00 40 00 01 00 01 00 00 00 00 \
         01")
}

fn with(mut bytes: Vec<u8>, at: usize, value: &[u8]) -> Vec<u8> {
    bytes[at..at + value.len()].copy_from_slice(value);
    bytes
}

#[test]
fn index_refusals() {
    let ok = minimal();
    // 1. magic and version.
    assert_eq!(
        err(BackdropIndex::parse(&with(ok.clone(), 3, b"E"))),
        ("BKD", 0)
    );
    assert_eq!(
        err(BackdropIndex::parse(&with(ok.clone(), 5, &[2]))),
        ("BKD", 4)
    );
    // 2. mask, elevation, colour bit.
    assert_eq!(
        err(BackdropIndex::parse(&with(ok.clone(), 6, &[0]))),
        ("BKD", 6)
    );
    assert_eq!(
        err(BackdropIndex::parse(&with(ok.clone(), 6, &[8]))),
        ("BKD", 6)
    );
    assert_eq!(
        err(BackdropIndex::parse(&with(ok.clone(), 24, &[1]))),
        ("BKD", 24)
    );
    assert_eq!(
        err(BackdropIndex::parse(&with(ok.clone(), 25, &[6]))),
        ("BKD", 25)
    );
    assert_eq!(
        err(BackdropIndex::parse(&with(ok.clone(), 25, &[9]))),
        ("BKD", 25)
    );
    // 3. chunk size, dimensions, chunk counts.
    assert_eq!(
        err(BackdropIndex::parse(&with(ok.clone(), 8, &[0, 100]))),
        ("BKD", 8)
    );
    assert_eq!(
        err(BackdropIndex::parse(&with(ok.clone(), 8, &[1, 1]))),
        ("BKD", 8)
    );
    assert_eq!(
        err(BackdropIndex::parse(&with(
            ok.clone(),
            24 + 0x10,
            &[0, 0, 0, 0]
        ))),
        ("BKD", 40)
    );
    assert_eq!(
        err(BackdropIndex::parse(&with(
            ok.clone(),
            24 + 0x14,
            &[0, 0, 0x40, 1]
        ))),
        ("BKD", 44)
    );
    assert_eq!(
        err(BackdropIndex::parse(&with(ok.clone(), 24 + 0x18, &[0, 2]))),
        ("BKD", 48)
    );
    assert_eq!(
        err(BackdropIndex::parse(&with(ok.clone(), 24 + 0x1A, &[0, 0]))),
        ("BKD", 50)
    );
    // 4. origin hex, PID count, compression, reserved bytes.
    assert_eq!(
        err(BackdropIndex::parse(&with(
            ok.clone(),
            24 + 4,
            &[0, 0, 0x9C, 0x40]
        ))),
        ("BKD", 28)
    );
    assert_eq!(
        err(BackdropIndex::parse(&with(
            ok.clone(),
            24 + 0x1C,
            &[0, 0, 0x10, 1]
        ))),
        ("BKD", 52)
    );
    assert_eq!(
        err(BackdropIndex::parse(&with(ok.clone(), 7, &[2]))),
        ("BKD", 7)
    );
    assert_eq!(
        err(BackdropIndex::parse(&with(ok.clone(), 0x0A, &[0, 1]))),
        ("BKD", 0x0A)
    );
    assert_eq!(
        err(BackdropIndex::parse(&with(ok.clone(), 0x0F, &[1]))),
        ("BKD", 0x0C)
    );
    assert_eq!(
        err(BackdropIndex::parse(&with(ok.clone(), 24 + 3, &[1]))),
        ("BKD", 27)
    );
    // Other: suppression code, presence byte, truncation, trailing bytes.
    assert_eq!(
        err(BackdropIndex::parse(&with(ok.clone(), 26, &[4]))),
        ("BKD", 26)
    );
    assert_eq!(
        err(BackdropIndex::parse(&with(ok.clone(), 56, &[2]))),
        ("BKD", 56)
    );
    assert_eq!(err(BackdropIndex::parse(&ok[..56])), ("BKD", 56));
    assert_eq!(err(BackdropIndex::parse(&ok[..10])), ("BKD", 10));
    assert_eq!(err(BackdropIndex::parse(&[])), ("BKD", 0));
    let mut long = ok.clone();
    long.push(0);
    assert_eq!(err(BackdropIndex::parse(&long)), ("BKD", 57));
}

#[test]
fn index_tables_are_checked_before_allocation() {
    // 16384 by 16384 at S = 64 declares 65536 chunks and 4096 PIDs, with no
    // table bytes behind the record.
    let mut bytes = minimal();
    bytes.truncate(24 + 32);
    bytes = with(
        bytes,
        24 + 0x10,
        &[0, 0, 0x40, 0, 0, 0, 0x40, 0, 1, 0, 1, 0, 0, 0, 0x10, 0],
    );
    bytes[26] = 2;
    // Offset 24 + 0x1C holds the PID count 4096.
    assert_eq!(err(BackdropIndex::parse(&bytes)), ("BKD", 56));
    // PIDs without a PID suppression code are refused at the count field.
    let listed = with(with(minimal(), 24 + 0x1C, &[0, 0, 0, 1]), 26, &[1]);
    assert_eq!(err(BackdropIndex::parse(&listed)), ("BKD", 52));
}

#[test]
fn index_with_hostile_bytes_never_panics() {
    let base = hex(INDEX_E1);
    for at in 0..base.len() {
        for value in [0u8, 1, 2, 0x7F, 0x80, 0xFF] {
            let mut bytes = base.clone();
            bytes[at] = value;
            let _ = BackdropIndex::parse(&bytes);
        }
        let _ = BackdropIndex::parse(&base[..at]);
    }
}

fn chunk_fixture(compression: u8) -> (BackdropIndex, Vec<u8>) {
    let e = elevation(
        0,
        Planes {
            depth: true,
            fade: true,
        },
        [70, 3],
        64,
        Suppression::None,
    );
    let mut idx = index(compression, vec![e]);
    idx.elevations[0].present = vec![true, true];
    let chunk = chunk_for(&idx, &idx.elevations[0], 1, 0);
    let bytes = chunk.to_bytes(&idx, 0, 1, 0);
    (idx, bytes)
}

#[test]
fn chunk_refusals() {
    for compression in [COMPRESSION_STORED, COMPRESSION_ZLIB] {
        let (idx, ok) = chunk_fixture(compression);
        BackdropChunk::parse(&ok, &idx, 0).unwrap();
        let go = |bytes: &[u8]| err(BackdropChunk::parse(bytes, &idx, 0));
        // Vector E: magic VBKE, then the version.
        assert_eq!(go(&with(ok.clone(), 3, b"E")), ("BKC", 0));
        assert_eq!(go(&with(ok.clone(), 5, &[2])), ("BKC", 4));
        // Elevation disagrees with the file and with the index.
        assert_eq!(go(&with(ok.clone(), 6, &[1])), ("BKC", 6));
        assert_eq!(err(BackdropChunk::parse(&ok, &idx, 1)), ("BKC", 6));
        // Planes, column and row.
        assert_eq!(go(&with(ok.clone(), 7, &[3])), ("BKC", 7));
        assert_eq!(go(&with(ok.clone(), 8, &[0, 2])), ("BKC", 8));
        assert_eq!(go(&with(ok.clone(), 0x0A, &[0, 5])), ("BKC", 0x0A));
        // Column 0 is a full 64 by 3 chunk: the declared length is wrong.
        assert_eq!(go(&with(ok.clone(), 8, &[0, 0])), ("BKC", 0x0C));
        assert_eq!(go(&with(ok.clone(), 0x0F, &[1])), ("BKC", 0x0C));
        // Truncated header.
        assert_eq!(go(&ok[..9]), ("BKC", 8));
        assert_eq!(go(&[]), ("BKC", 0));
        // Trailing bytes after the payload (stored) or the stream (zlib).
        let mut long = ok.clone();
        long.push(0);
        assert_eq!(go(&long), ("BKC", ok.len()));
        // A truncated payload.
        let short = go(&ok[..ok.len() - 1]);
        assert_eq!(short.0, "BKC");
        if compression == COMPRESSION_STORED {
            assert_eq!(short.1, ok.len() - 1);
        }
    }
}

#[test]
fn chunk_absent_in_the_index_is_refused() {
    let (mut idx, ok) = chunk_fixture(COMPRESSION_STORED);
    idx.elevations[0].present[1] = false;
    assert_eq!(err(BackdropChunk::parse(&ok, &idx, 0)), ("BKC", 8));
}

#[test]
fn vector_e_depth_40001() {
    for compression in [COMPRESSION_STORED, COMPRESSION_ZLIB] {
        let (idx, _) = chunk_fixture(compression);
        let mut chunk = chunk_for(&idx, &idx.elevations[0], 1, 0);
        chunk.depth.as_mut().unwrap()[4] = 40000;
        let bytes = chunk.to_bytes(&idx, 0, 1, 0);
        BackdropChunk::parse(&bytes, &idx, 0).unwrap();
        chunk.depth.as_mut().unwrap()[4] = 40001;
        let bytes = chunk.to_bytes(&idx, 0, 1, 0);
        // 6 by 3 chunk: 18 colour bytes, then depth value 4 at +8.
        assert_eq!(
            err(BackdropChunk::parse(&bytes, &idx, 0)),
            ("BKC", 16 + 18 + 8)
        );
        chunk.depth.as_mut().unwrap()[4] = u16::MAX;
        let bytes = chunk.to_bytes(&idx, 0, 1, 0);
        assert_eq!(
            err(BackdropChunk::parse(&bytes, &idx, 0)),
            ("BKC", 16 + 18 + 8)
        );
    }
}

#[test]
fn vector_e_payload_one_byte_short_and_stream_framing() {
    let (idx, ok) = chunk_fixture(COMPRESSION_ZLIB);
    let n = 6 * 3 + 2 * 18 + 3;
    assert_eq!(&ok[0x0C..0x10], &(n as u32).to_be_bytes());
    // A well-framed stream whose data is one byte short.
    let payload: Vec<u8> = (0..n as u8 - 1).collect();
    let mut short = ok[..0x10].to_vec();
    short.extend_from_slice(&zlib_stored(&payload));
    let found = err(BackdropChunk::parse(&short, &idx, 0));
    assert_eq!(found.0, "BKC");
    assert!(found.1 > 0x10);
    // Bytes after the stream end exactly there.
    let mut long = ok.clone();
    long.extend_from_slice(&[0, 0]);
    assert_eq!(err(BackdropChunk::parse(&long, &idx, 0)), ("BKC", ok.len()));
    // A corrupted checksum is reported at the checksum.
    let mut bad = ok.clone();
    let last = bad.len() - 1;
    bad[last] ^= 1;
    assert_eq!(
        err(BackdropChunk::parse(&bad, &idx, 0)),
        ("BKC", ok.len() - 4)
    );
}

#[test]
fn zlib_writer_frames_large_payloads() {
    // Over 65535 bytes needs several stored blocks: 512 by 512, all planes.
    let mut e = elevation(0, COMBOS[3], [512, 512], 512, Suppression::None);
    e.present = vec![true];
    let mut idx = index(COMPRESSION_ZLIB, vec![e]);
    idx.chunk_size = 512;
    let chunk = chunk_for(&idx, &idx.elevations[0], 0, 0);
    let bytes = chunk.to_bytes(&idx, 0, 0, 0);
    assert_eq!(BackdropChunk::parse(&bytes, &idx, 0).unwrap(), chunk);
}

#[test]
fn chunk_with_hostile_bytes_never_panics() {
    for compression in [COMPRESSION_STORED, COMPRESSION_ZLIB] {
        let (idx, base) = chunk_fixture(compression);
        for at in 0..base.len().min(80) {
            for value in [0u8, 1, 0x7F, 0xFF] {
                let mut bytes = base.clone();
                bytes[at] = value;
                let _ = BackdropChunk::parse(&bytes, &idx, 0);
            }
            let _ = BackdropChunk::parse(&base[..at], &idx, 0);
        }
    }
}

#[test]
fn fixture_builders_parse_and_match_the_writer() {
    use vault_fixtures::backdrop::{Elevation, build, vector_b};
    let b = vector_b();
    let idx = BackdropIndex::parse(&b.index).unwrap();
    assert_eq!(idx.elevations[0].size, [4, 1]);
    let (e, c, r, bytes) = &b.chunks[0];
    let chunk = BackdropChunk::parse(bytes, &idx, *e).unwrap();
    assert_eq!((*c, *r), (0, 0));
    assert_eq!(chunk.colour, [7; 4]);
    assert_eq!(chunk.depth.as_deref(), Some(&[0, 0, 0x501D, 0x501D][..]));
    assert_eq!(chunk.fade.as_deref(), Some(&[0xF0][..]));
    assert_eq!(idx.to_bytes(), b.index);
    assert_eq!(chunk.to_bytes(&idx, *e, *c, *r), *bytes);

    // Two chunks across, zlib, with suppressed PIDs.
    let (w, h) = (70u32, 3u32);
    let n = (w * h) as usize;
    let built = build(
        64,
        true,
        9,
        &[Elevation {
            elevation: 2,
            width: w,
            height: h,
            origin_px: [-3, 4],
            origin_hex: 100,
            colour: (0..n).map(|i| i as u8).collect(),
            depth: Some((0..n).map(|i| (i * 3) as u16).collect()),
            fade: Some((0..n).map(|i| i % 5 == 0).collect()),
            suppression: 3,
            pids: vec![1, 2],
        }],
    );
    let idx = BackdropIndex::parse(&built.index).unwrap();
    assert_eq!(
        idx.elevations[0].suppress,
        Suppression::WallsAndPids(vec![1, 2])
    );
    assert_eq!(built.chunks.len(), 2);
    let right = built.chunks.iter().find(|c| c.1 == 1).unwrap();
    let chunk = BackdropChunk::parse(&right.3, &idx, 2).unwrap();
    assert_eq!((chunk.origin, chunk.size), ([64, 0], [6, 3]));
    assert_eq!(chunk.colour[0], 64);
    assert!(chunk.fade_at(1, 0) && !chunk.fade_at(0, 0));
    assert_eq!(chunk.to_bytes(&idx, 2, 1, 0), right.3);
}
