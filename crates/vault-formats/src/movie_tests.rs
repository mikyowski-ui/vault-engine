//! SPEC-012 §4 vectors A-H: header validation, the chunk walk and its
//! read-ahead rule, motion codes, pattern blocks, audio, subtitles, fades,
//! and the synthetic movies against their expected hashes and frames.
use crate::{
    moviefx::{self, FadeHook},
    mve::{
        self, AudioFormat, Event, Movie, VideoBuffers, decode_audio, far_motion, frame_period,
        near_motion,
    },
    sve::{self, SubtitleTrack},
};
use sha2::{Digest, Sha256};

#[allow(dead_code)]
#[path = "../../../tests/fixtures/movie/support.rs"]
mod movie;

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A movie whose single chunk holds `opcodes`, then an end-of-stream chunk.
fn stream(opcodes: &[(u8, u8, Vec<u8>)]) -> Vec<u8> {
    let mut body = Vec::new();
    for (kind, version, payload) in opcodes {
        body.extend((payload.len() as u16).to_le_bytes());
        body.extend([*kind, *version]);
        body.extend(payload);
    }
    body.extend([0, 0, 1, 0]);
    let mut bytes = movie::header_a()[..26].to_vec();
    bytes.extend((body.len() as u16).to_le_bytes());
    bytes.extend([0, 0]);
    bytes.extend(body);
    bytes.extend([4, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}
fn words(values: &[u16]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

#[test]
fn spec012_fixture_a_header_validation() {
    let first = mve::header(&movie::header_a()).unwrap();
    assert_eq!((first.length, first.kind), (8, 3));
    for (name, bytes) in movie::header_variants() {
        assert!(mve::header(&bytes).is_err(), "{name} must be rejected");
    }
    // Only 30 header bytes, without the first record: the header is valid,
    // but the record read fails before any frame.
    assert!(Movie::open(movie::header_a()).is_err());
    assert!(mve::header(&movie::header_a()[..29]).is_err());
}

#[test]
fn spec012_fixture_b_chunk_walk_and_read_ahead() {
    let mut walk = Movie::open(movie::chunk_walk()).unwrap();
    assert_eq!(
        walk.next_event().unwrap(),
        Event::Timer {
            rate: 8333,
            subdivision: 8
        }
    );
    assert_eq!(frame_period(8333, 8), 66668);
    assert_eq!(walk.next_event().unwrap(), Event::Finished);
    assert_eq!(walk.stats().chunks, 2);
    assert_eq!(walk.stats().opcode_counts[..3], [1, 1, 1]);
    assert_eq!(walk.next_event().unwrap(), Event::Finished, "closed");
    // Without the trailing header, record 2 cannot be read.
    let mut bytes = movie::chunk_walk();
    bytes.truncate(bytes.len() - 4);
    let mut truncated = Movie::open(bytes).unwrap();
    assert!(matches!(truncated.next_event(), Ok(Event::Timer { .. })));
    let error = truncated.next_event().unwrap_err();
    assert!(error.to_string().contains("truncated"), "{error}");
    assert_eq!(truncated.next_event().unwrap(), Event::Finished);
    // The chunk type word is ignored.
    let mut retyped = movie::chunk_walk();
    retyped[28] = 0x77;
    let mut retyped = Movie::open(retyped).unwrap();
    assert!(matches!(retyped.next_event(), Ok(Event::Timer { .. })));
    assert_eq!(retyped.next_event().unwrap(), Event::Finished);
    // Safe rule: an opcode running past its chunk body is a stream error.
    let mut overrun = movie::chunk_walk();
    overrun[30] = 0x20;
    assert!(Movie::open(overrun).unwrap().next_event().is_err());
    // All 32 rate bits count (safe rule).
    assert_eq!(frame_period(0x1_0000, 8), 0x8_0000 + 4);
}

#[test]
fn spec012_fixture_c_motion_codes_and_linear_wrap() {
    for (code, expected) in [
        (0x00, (8, 0)),
        (55, (14, 7)),
        (56, (-14, 8)),
        (0xff, (11, 14)),
    ] {
        assert_eq!(near_motion(code), expected);
    }
    let (dx, dy) = near_motion(0xff);
    assert_eq!((-dx, -dy), (-11, -14), "type 3 negates");
    assert_eq!(far_motion(0x00), (-8, -8));
    assert_eq!(far_motion(0x88), (0, 0));
    assert_eq!(far_motion(0xff), (7, 7));
    assert_eq!((0xfd_u8 as i8, 0x02_u8 as i8), (-3, 2));
    // A type-2 block at x = 424 of a 432-wide frame reads columns 432..439,
    // which are columns 0..7 of the next row.
    let mut video = VideoBuffers::new(54, 2);
    for row in 1..=8 {
        for col in 0..8 {
            video.current_mut()[row * 432 + col] = (row * 8 + col + 1) as u8;
        }
    }
    let mut counts = [0; 16];
    assert_eq!(
        video.decode(&[0x21], &[0x00], [52, 0, 2, 1], &mut counts),
        Ok(1)
    );
    for row in 0..8 {
        for col in 0..8 {
            assert_eq!(
                video.current()[row * 432 + 424 + col],
                ((row + 1) * 8 + col + 1) as u8
            );
        }
    }
    assert_eq!((counts[1], counts[2]), (1, 1));
    // Type 5 copies from the other buffer; a source outside the buffer is a
    // stream error (safe rule).
    video.other_mut()[3] = 0xab;
    assert!(
        video
            .decode(&[0x15], &[0, 0], [0, 1, 2, 1], &mut counts)
            .is_ok()
    );
    assert!(
        video
            .decode(&[0x15], &[0xfd, 0x00], [0, 0, 2, 1], &mut counts)
            .is_err()
    );
    let mut video = VideoBuffers::new(2, 1);
    video.other_mut()[3] = 0xab;
    assert!(
        video
            .decode(&[0x15], &[3, 0], [0, 0, 2, 1], &mut counts)
            .is_ok()
    );
    assert_eq!(video.current()[0], 0xab);
}

/// Decode one pattern block into the left of a 16 x 8 frame; the right
/// block is type 1 (unchanged). Returns the left block's rows.
fn left_block(kind: u8, params: &[u8]) -> ([[u8; 8]; 8], usize) {
    let mut video = VideoBuffers::new(2, 1);
    let consumed = video
        .decode(&[0x10 | kind], params, [0, 0, 2, 1], &mut [0; 16])
        .unwrap();
    let rows = std::array::from_fn(|j| video.current()[j * 16..j * 16 + 8].try_into().unwrap());
    (rows, consumed)
}

#[test]
fn spec012_fixture_d_pattern_blocks() {
    // 1. Type 7, P0 <= P1: the diagonal is P1.
    let (rows, consumed) = left_block(7, &[0x10, 0x20, 1, 2, 4, 8, 0x10, 0x20, 0x40, 0x80]);
    assert_eq!(consumed, 10);
    for (j, row) in rows.iter().enumerate() {
        for (i, pixel) in row.iter().enumerate() {
            assert_eq!(*pixel, if i == j { 0x20 } else { 0x10 });
        }
    }
    // 2. Type 7, P0 > P1: 2 x 2 cells on the diagonal are P1.
    let (rows, consumed) = left_block(7, &[0x20, 0x10, 0x21, 0x84]);
    assert_eq!(consumed, 4);
    for (j, row) in rows.iter().enumerate() {
        for (i, pixel) in row.iter().enumerate() {
            assert_eq!(*pixel, if i / 2 == j / 2 { 0x10 } else { 0x20 });
        }
    }
    // 3. Type 9 (P0 <= P1, P2 > P3): 2 x 2 cells.
    let (rows, consumed) = left_block(9, &[0, 1, 3, 2, 0xe4, 0xe4, 0xe4, 0xe4]);
    assert_eq!(consumed, 8);
    assert!(rows.iter().all(|row| *row == [0, 0, 1, 1, 3, 3, 2, 2]));
    // 4. Type 8 quadrants: top-left, then bottom-left (second group),
    // top-right, bottom-right.
    let params = [
        0x05, 0x09, 0x0f, 0x00, 0x01, 0x02, 0xff, 0xff, 0x03, 0x04, 0, 0, 0x05, 0x06, 0, 0,
    ];
    let (rows, consumed) = left_block(8, &params);
    assert_eq!(consumed, 16);
    assert_eq!(rows[0][..4], [9; 4]);
    for row in &rows[1..4] {
        assert_eq!(row[..4], [5; 4]);
        assert_eq!(row[4..], [3; 4]);
    }
    for row in &rows[4..] {
        assert_eq!(row[..4], [2; 4], "second group: bottom-left");
        assert_eq!(row[4..], [5; 4]);
    }
    // 5. Type 15 checkerboard.
    let (rows, consumed) = left_block(15, &[5, 9]);
    assert_eq!(consumed, 2);
    assert_eq!(rows[0], [5, 9, 5, 9, 5, 9, 5, 9]);
    assert_eq!(rows[1], [9, 5, 9, 5, 9, 5, 9, 5]);
    // 6. Type 13 quadrant colors.
    let (rows, _) = left_block(13, &[1, 2, 3, 4]);
    assert!(rows[..4].iter().all(|row| *row == [1, 1, 1, 1, 2, 2, 2, 2]));
    assert!(rows[4..].iter().all(|row| *row == [3, 3, 3, 3, 4, 4, 4, 4]));
    // 7. Map byte E7: type 7 on the left, type 14 on the right.
    let mut video = VideoBuffers::new(2, 1);
    let mut counts = [0; 16];
    let params = [0x10, 0x20, 1, 2, 4, 8, 0x10, 0x20, 0x40, 0x80, 0x33];
    assert_eq!(
        video.decode(&[0xe7], &params, [0, 0, 2, 1], &mut counts),
        Ok(11)
    );
    assert_eq!((counts[7], counts[14]), (1, 1));
    assert!(video.current()[8..16].iter().all(|pixel| *pixel == 0x33));
    // Safe rules: type 6, an over-read, an odd width, and a short map.
    assert!(
        video
            .decode(&[0x16], &[], [0, 0, 2, 1], &mut counts)
            .is_err()
    );
    assert!(
        video
            .decode(&[0x1b], &[0; 63], [0, 0, 2, 1], &mut counts)
            .is_err()
    );
    assert!(
        video
            .decode(&[0x11], &[], [0, 0, 1, 1], &mut counts)
            .is_err()
    );
    assert!(video.decode(&[], &[], [0, 0, 2, 1], &mut counts).is_err());
}

#[test]
fn spec012_fixture_e_audio() {
    let mono = AudioFormat {
        rate: 22050,
        stereo: false,
        sixteen_bit: true,
        compressed: true,
        buffer: 0,
    };
    assert_eq!(
        decode_audio(&mono, 8, &[0x00, 0x01, 0x05, 0xff, 0x82]).unwrap(),
        [0x00, 0x01, 0x05, 0x01, 0x04, 0x01, 0x6d, 0x16]
    );
    let stereo = AudioFormat {
        stereo: true,
        ..mono
    };
    assert_eq!(
        decode_audio(
            &stereo,
            12,
            &[0x9c, 0xff, 0x64, 0x00, 0x01, 0xff, 0x82, 0x81]
        )
        .unwrap(),
        [
            0x9c, 0xff, 0x64, 0x00, 0x9d, 0xff, 0x63, 0x00, 0x06, 0x15, 0x64, 0x00
        ]
    );
    // Wraparound: 32000 + 0x3F4E gives -17330, never a clamped 32767.
    let pcm = decode_audio(&mono, 4, &[0x00, 0x7d, 0x6f]).unwrap();
    assert_eq!(i16::from_le_bytes([pcm[2], pcm[3]]), -17330);
    assert_eq!(pcm[2..], [0x4e, 0xbc]);
    // The table puts -29973 at code 120 (§2.8's prose says 121); codes
    // 120-127 are negative and 128-129 are both +1.
    assert_eq!(mve::AUDIO_DELTAS[120] as i16, -29973);
    assert!(
        mve::AUDIO_DELTAS[120..128]
            .iter()
            .all(|delta| (*delta as i16) < 0)
    );
    assert_eq!(mve::AUDIO_DELTAS[127] as i16, -1);
    assert_eq!(mve::AUDIO_DELTAS[128..130], [1, 1]);
    assert_eq!(mve::AUDIO_DELTAS[137] as i16, -32589);
    assert_eq!(mve::AUDIO_DELTAS[237..239], [0xffee, 0xffed]);
    // Uncompressed data is the payload unchanged (safe rule).
    let raw = AudioFormat {
        compressed: false,
        ..mono
    };
    assert_eq!(decode_audio(&raw, 3, &[1, 2, 3, 4]).unwrap(), [1, 2, 3]);
    assert!(decode_audio(&mono, 8, &[0, 1, 5]).is_err(), "short codes");
    // Track mask: only bit 0x0001 selects track 0.
    let audio =
        |mask: u16, length: u16, codec: &[u8]| [words(&[0, mask, length]), codec.to_vec()].concat();
    let mut movie = Movie::open(stream(&[
        (3, 1, words(&[0, 0b110, 22050, 0x5000, 0])),
        (8, 0, audio(0x0002, 8, &[0x00, 0x01, 0x05, 0xff, 0x82])),
        (8, 0, audio(0x0001, 8, &[0x00, 0x01, 0x05, 0xff, 0x82])),
        (9, 0, words(&[0, 0xfffe, 16])),
        (9, 0, words(&[0, 0xffff, 16])),
    ]))
    .unwrap();
    let format = match movie.next_event().unwrap() {
        Event::AudioFormat(format) => format,
        other => panic!("{other:?}"),
    };
    assert!(format.compressed && format.sixteen_bit && !format.stereo);
    assert_eq!(
        (format.buffer, format.device_buffer_bytes()),
        (0x5000, 0x7800)
    );
    assert_eq!(
        movie.next_event().unwrap(),
        Event::Audio {
            silence: false,
            pcm: vec![0x00, 0x01, 0x05, 0x01, 0x04, 0x01, 0x6d, 0x16]
        }
    );
    assert_eq!(
        movie.next_event().unwrap(),
        Event::Audio {
            silence: true,
            pcm: vec![0; 16]
        }
    );
    assert_eq!(movie.next_event().unwrap(), Event::Finished);
    // Version 0 ignores the compressed bit; 8-bit silence is 0x80.
    let mut movie = Movie::open(stream(&[
        (3, 0, words(&[0, 0b100, 11025, 0x1000])),
        (9, 0, words(&[0, 1, 2])),
    ]))
    .unwrap();
    assert!(matches!(
        movie.next_event().unwrap(),
        Event::AudioFormat(AudioFormat {
            compressed: false,
            buffer: 0x1000,
            ..
        })
    ));
    assert_eq!(
        movie.next_event().unwrap(),
        Event::Audio {
            silence: true,
            pcm: vec![0x80; 2]
        }
    );
}

#[test]
fn spec012_video_configuration_and_stream_errors() {
    let video = |version: u8, payload: Vec<u8>| {
        Movie::open(stream(&[(5, version, payload)]))
            .unwrap()
            .next_event()
    };
    assert_eq!(video(2, words(&[54, 40, 1, 0])), Ok(Event::Finished));
    assert!(video(2, words(&[54, 40, 1, 1])).is_err(), "high color");
    assert!(video(1, words(&[54, 20, 2])).is_err(), "multiplier");
    assert!(video(0, words(&[81, 40])).is_err(), "wider than 640");
    assert!(video(0, words(&[80, 61])).is_err(), "taller than 480");
    assert_eq!(video(0, words(&[80, 60])), Ok(Event::Finished));
    // Video data: version 3 or later, with a map in the same chunk.
    let data = |version: u8| (17, version, words(&[0, 0, 0, 0, 2, 1, 0]));
    let configure = (5, 0, words(&[2, 1]));
    let map = (15, 0, vec![0x11]);
    let result = |opcodes: &[(u8, u8, Vec<u8>)]| Movie::open(stream(opcodes)).unwrap().next_event();
    assert_eq!(
        result(&[configure.clone(), map.clone(), data(3)]),
        Ok(Event::Finished)
    );
    assert!(result(&[configure.clone(), map.clone(), data(2)]).is_err());
    assert!(result(&[configure.clone(), data(3)]).is_err(), "no map");
    assert!(result(&[map.clone(), data(3)]).is_err(), "no buffers");
    // Palette loads within 256 entries.
    let mut loaded =
        Movie::open(stream(&[(12, 0, [words(&[254, 2]), vec![1; 6]].concat())])).unwrap();
    assert_eq!(loaded.next_event(), Ok(Event::Finished));
    assert_eq!(loaded.palette_table()[762..], [1; 6]);
    assert!(result(&[(12, 0, [words(&[255, 2]), vec![1; 6]].concat())]).is_err());
    assert!(result(&[(12, 0, [words(&[0, 2]), vec![1; 5]].concat())]).is_err());
    // Show frame counts frames and reports the push range and markers.
    let mut shown = Movie::open(stream(&[
        (14, 0, Vec::new()),
        (7, 1, words(&[1, 254, 1])),
        (7, 0, words(&[0, 0])),
    ]))
    .unwrap();
    let Ok(Event::Frame(first)) = shown.next_event() else {
        panic!("frame");
    };
    assert_eq!(
        (first.number, first.palette_start, first.palette_count),
        (1, 1, 254)
    );
    assert!(first.suppressed && first.field_mode && !first.video);
    let Ok(Event::Frame(second)) = shown.next_event() else {
        panic!("frame");
    };
    assert_eq!(second.number, 2);
    assert!(result(&[(7, 0, words(&[200, 57]))]).is_err());
}

#[test]
fn spec012_fixture_f_subtitles() {
    let entries = sve::parse(movie::subtitles());
    let expected: [(i32, &[u8]); 4] = [
        (10, b"First line"),
        (25, b"Second: with colon"),
        (5, b"Earlier number, later in file"),
        (0, b"Zero"),
    ];
    assert_eq!(entries.len(), 4);
    for (entry, (frame, text)) in entries.iter().zip(expected) {
        assert_eq!((entry.frame, entry.text.as_slice()), (frame, text));
    }
    let mut track = SubtitleTrack::new(entries);
    for frame in 1..10 {
        assert!(track.update(frame).is_empty());
    }
    assert_eq!(track.update(10).len(), 1);
    assert_eq!(track.shown(), Some(b"First line".as_slice()));
    for frame in 11..25 {
        assert!(track.update(frame).is_empty());
    }
    let drawn = track.update(25);
    assert_eq!(drawn.len(), 3, "entries 2, 3 and 4 in one step");
    assert_eq!(track.shown(), Some(b"Zero".as_slice()));
    assert_eq!(track.remaining(), 0);
    // Signs and leading-integer rules; LF-only and lone-CR lines.
    let entries = sve::parse(b"  -3x:a\n+7:b\rc\n12\n:\n");
    assert_eq!(entries.len(), 3);
    assert_eq!(
        (entries[0].frame, entries[0].text.as_slice()),
        (-3, b"a".as_slice())
    );
    assert_eq!(
        (entries[1].frame, entries[1].text.as_slice()),
        (7, b"b".as_slice())
    );
    assert_eq!(
        (entries[2].frame, entries[2].text.as_slice()),
        (0, b"".as_slice())
    );
}

#[test]
fn spec012_fixture_g_fades() {
    let effects = moviefx::parse(movie::fades()).unwrap();
    assert_eq!(effects.len(), 2);
    assert!(!effects[0].out && effects[1].out);
    assert_eq!((effects[1].start, effects[1].steps), (35, 35));
    let mut hook = FadeHook::new(effects).unwrap();
    assert!(hook.inside(), "an effect starts at frame 0");
    let table: [u8; 768] = std::array::from_fn(|index| [63, 40, 1][index % 3]);
    let mut device = [0_u8; 768];
    hook.push(&table, &mut device, 0, 256);
    assert_eq!(device, [0; 768], "inside a fade the push is held back");
    let levels = |device: &[u8; 768]| [device[0], device[1], device[2]];
    for (frame, expected) in [(1, [12, 8, 0]), (5, [37, 24, 0]), (9, [63, 40, 1])] {
        assert!(hook.update(frame, &mut device));
        assert_eq!(levels(&device), expected, "frame {frame}");
        assert!(device.chunks(3).all(|entry| entry == expected));
    }
    assert!(!hook.update(10, &mut device));
    // Outside a fade the device follows palette pushes.
    let other = [7_u8; 768];
    hook.push(&other, &mut device, 3, 1);
    assert_eq!(device[9..12], [7; 3]);
    hook.push(&table, &mut device, 3, 1);
    for (frame, expected) in [
        (35, [62, 39, 1]),
        (36, [60, 38, 1]),
        (52, [31, 20, 1]),
        (69, [0, 0, 0]),
    ] {
        assert!(hook.update(frame, &mut device));
        assert_eq!(levels(&device), expected, "frame {frame}");
    }
    assert!(!hook.update(70, &mut device));
    assert_eq!(levels(&device), [0, 0, 0], "stays at the fade color");
    // Too few commas load nothing; newest-loaded effects win overlaps.
    let none = b"[info]\ntotal_effects=2\neffect_frames=0\n[0]\nfade_type=in\nfade_color=0,0,0\nfade_steps=5\n";
    assert!(moviefx::parse(none).unwrap().is_empty());
    assert!(FadeHook::new(Vec::new()).is_none());
    let skipped = b"[info]\ntotal_effects=4\neffect_frames=1,2,3,4 junk\n\
        [1]\nfade_type=sideways\nfade_color=0,0,0\nfade_steps=5\n\
        [2]\nfade_type=in\nfade_color=0,0\nfade_steps=5\n\
        [3]\nfade_type=in\nfade_color=1,2,3\n\
        [4]\nfade_type=In\nfade_color=300,-1,2\nfade_steps=5\n";
    let loaded = moviefx::parse(skipped).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].color, [44, 255, 2], "stored modulo 256");
    let overlap = b"[info]\ntotal_effects=2\neffect_frames=10,12\n\
        [10]\nfade_type=out\nfade_color=63,63,63\nfade_steps=10\n\
        [12]\nfade_type=out\nfade_color=0,0,0\nfade_steps=2\n";
    let mut hook = FadeHook::new(moviefx::parse(overlap).unwrap()).unwrap();
    assert!(!hook.inside());
    let mut device = [0; 768];
    hook.update(12, &mut device);
    assert_eq!(
        levels(&device),
        [0, 0, 0],
        "frame 12 belongs to the later effect"
    );
    hook.update(14, &mut device);
    assert_eq!(
        levels(&device),
        [31, 31, 31],
        "frame 14: k = 5 of the first"
    );
}

/// §4.8 Fixture H: every expected-table row in stream order, every frame
/// buffer, and the whole PCM stream.
#[test]
fn spec012_fixture_h_synthetic_movies() {
    for name in ['A', 'B'] {
        let (bytes, expected, frames) = movie::synthetic(name);
        let frames = movie::frames(frames);
        let mut rows = expected.lines().filter(|line| !line.is_empty());
        let mut movie = Movie::open(bytes.to_vec()).unwrap();
        let mut device = [0_u8; 768];
        let mut pcm = Vec::new();
        let mut shown = 0;
        loop {
            match movie.next_event().unwrap() {
                Event::Audio { silence, pcm: out } => {
                    let row = rows.next().unwrap();
                    let opcode = if silence { 9 } else { 8 };
                    assert_eq!(
                        row,
                        format!(
                            "audio\top={opcode}\tbytes={}\tpcm_sha256={}",
                            out.len(),
                            sha(&out)
                        ),
                        "{name}"
                    );
                    pcm.extend(out);
                }
                Event::Frame(frame) => {
                    mve::push_palette(
                        movie.palette_table(),
                        &mut device,
                        frame.palette_start,
                        frame.palette_count,
                    );
                    let pixels = movie.video().unwrap().current();
                    assert_eq!(
                        pixels,
                        frames[shown].as_slice(),
                        "{name} frame {}",
                        frame.number
                    );
                    shown += 1;
                    assert_eq!(
                        rows.next().unwrap(),
                        format!(
                            "frame\t{}\tvideo={}\tsuppressed={}\tpal={}+{}\tdevice_pal_sha256={}\tpixels_sha256={}",
                            frame.number,
                            u8::from(frame.video),
                            u8::from(frame.suppressed),
                            frame.palette_start,
                            frame.palette_count,
                            sha(&device),
                            sha(pixels)
                        ),
                        "{name}"
                    );
                }
                Event::Finished => break,
                Event::Timer { .. } | Event::AudioFormat(_) | Event::Sync => {}
            }
        }
        assert_eq!(shown, frames.len());
        let total = rows.next().unwrap();
        let samples: Vec<i16> = pcm
            .chunks(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let list = |samples: &[i16]| {
            samples
                .iter()
                .map(i16::to_string)
                .collect::<Vec<_>>()
                .join(",")
        };
        assert_eq!(
            total,
            format!(
                "pcm_total\tbytes={}\tsha256={}\tfirst_samples={}\tlast_samples={}",
                pcm.len(),
                sha(&pcm),
                list(&samples[..8]),
                list(&samples[samples.len() - 8..])
            ),
            "{name}"
        );
        assert!(rows.next().is_none());
        // Every type except 6 occurs, as do both buffer roles.
        let counts = movie.stats().block_counts;
        assert!(
            counts
                .iter()
                .enumerate()
                .all(|(kind, n)| (*n == 0) == (kind == 6))
        );
    }
}

/// Deterministic mutations and truncations never panic or loop.
#[test]
fn spec012_mutated_movies_fail_cleanly() {
    let (bytes, _, _) = movie::synthetic('A');
    let mut state = 0x2545_f491_u32;
    for round in 0..400 {
        let mut mutated = bytes.to_vec();
        for _ in 0..1 + round % 4 {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let at = (state >> 8) as usize % mutated.len();
            mutated[at] = (state >> 3) as u8;
        }
        if round % 7 == 0 {
            mutated.truncate((state as usize >> 4) % mutated.len());
        }
        if let Ok(mut movie) = Movie::open(mutated) {
            for _ in 0..10_000 {
                match movie.next_event() {
                    Ok(Event::Finished) | Err(_) => break,
                    Ok(_) => {}
                }
            }
            assert!(movie.finished());
        }
    }
}
