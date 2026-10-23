//! SPEC-010 §2.6 `.LIP` layout and the §4.7 LIP vectors.
use crate::lip::{self, LipError, LipFile, LipMarker, LipWarning};

/// SPEC-010 §4.7 LIP-A: stem `SYNTH01`, P = 5, M = 6.
pub(crate) const LIP_A: [u8; 97] = [
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x58, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x03, 0xE9, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x06,
    0x53, 0x59, 0x4E, 0x54, 0x48, 0x30, 0x31, 0x00, 0x56, 0x4F, 0x43, 0x00, 0x00, 0x09, 0x13, 0x17,
    0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x2C, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01,
    0x2D, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x02, 0x58, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x03,
    0xE8,
];

fn marker(kind: i32, position: i32) -> LipMarker {
    LipMarker { kind, position }
}

#[test]
fn spec010_lip_a_layout() {
    let lip = LipFile::parse(&LIP_A).unwrap();
    assert_eq!(lip.version, 2);
    assert_eq!((lip.flags, lip.length), (0, 1001));
    assert_eq!(lip.stem, b"SYNTH01");
    assert_eq!(lip.phonemes, [0, 9, 19, 23, 0]);
    assert_eq!(
        lip.markers,
        [
            marker(1, 0),
            marker(1, 100),
            marker(0, 300),
            marker(1, 301),
            marker(1, 600),
            marker(1, 1000)
        ]
    );
    assert!(!lip.repeat());
    assert!(lip.check().is_empty());
    // Bytes after the marker table are not read.
    let mut longer = LIP_A.to_vec();
    longer.extend_from_slice(b"trailing");
    assert_eq!(LipFile::parse(&longer).unwrap(), lip);
}

#[test]
fn spec010_lip_b_and_stem_bytes() {
    // LIP-B: the final marker moves to 900.
    let mut bytes = LIP_A;
    bytes[0x5D..0x61].copy_from_slice(&[0x00, 0x00, 0x03, 0x84]);
    let lip = LipFile::parse(&bytes).unwrap();
    assert_eq!(lip.markers[5], marker(1, 900));
    // All 8 stem bytes may be name characters, with no terminator.
    bytes[0x20..0x28].copy_from_slice(b"ABCDEFGH");
    assert_eq!(LipFile::parse(&bytes).unwrap().stem, b"ABCDEFGH");
    // The repeat flag is bit 0; the corpus value 4 is not it.
    bytes[0x0B] = 4;
    assert!(!LipFile::parse(&bytes).unwrap().repeat());
    bytes[0x0B] = 5;
    assert!(LipFile::parse(&bytes).unwrap().repeat());
}

#[test]
fn spec010_lip_loading_checks_and_failures() {
    // A file shorter than its header and counts require.
    for len in [0, 3, 43, 96] {
        assert!(
            matches!(
                LipFile::parse(&LIP_A[..len]),
                Err(LipError::Truncated { .. })
            ),
            "{len} bytes"
        );
    }
    // Any version other than 1 or 2.
    let mut bytes = LIP_A;
    bytes[3] = 3;
    assert_eq!(LipFile::parse(&bytes), Err(LipError::Version(3)));
    // A negative count is abandoned like a short file.
    let mut bytes = LIP_A;
    bytes[0x14..0x18].copy_from_slice(&(-1_i32).to_be_bytes());
    assert!(matches!(
        LipFile::parse(&bytes),
        Err(LipError::Count { .. })
    ));
    // Zero counts parse (the speech is then abandoned by the player).
    let mut bytes = LIP_A[..0x2C].to_vec();
    bytes[0x17] = 0;
    bytes[0x1F] = 0;
    let empty = LipFile::parse(&bytes).unwrap();
    assert!(empty.phonemes.is_empty() && empty.markers.is_empty());
    // Loading checks only log.
    let mut bytes = LIP_A;
    bytes[0x2D] = 42; // phoneme 1
    bytes[0x31 + 3] = 7; // marker 0 type
    bytes[0x31 + 7] = 5; // marker 0 position
    bytes[0x31 + 8 * 4 + 6] = 0; // marker 4 position 600 -> 88
    let lip = LipFile::parse(&bytes).unwrap();
    assert_eq!(
        lip.check(),
        [
            LipWarning::Phoneme {
                index: 1,
                value: 42
            },
            LipWarning::MarkerKind { index: 0, kind: 7 },
            LipWarning::FirstPosition(5),
            LipWarning::Decreasing {
                index: 4,
                position: 88
            },
        ]
    );
    // Safe rule: a phoneme of 42 or more, or past the table, is phoneme 0.
    assert_eq!((lip.phoneme(1), lip.phoneme(5), lip.phoneme(2)), (0, 0, 19));
}

#[test]
fn spec010_lip_version_1_header() {
    let mut bytes = vec![0; lip::HEADER_V1];
    bytes[3] = 1;
    bytes[0x0B] = 1;
    // The handle and address words are ignored.
    for offset in [0x0C, 0x14, 0x18, 0x30] {
        bytes[offset..offset + 4].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
    }
    bytes[0x27] = 1;
    bytes[0x2F] = 2;
    bytes[0x50..0x55].copy_from_slice(b"OLDV1");
    bytes.push(13);
    for (kind, position) in [(1_i32, 0_i32), (1, 4410)] {
        bytes.extend_from_slice(&kind.to_be_bytes());
        bytes.extend_from_slice(&position.to_be_bytes());
    }
    let lip = LipFile::parse(&bytes).unwrap();
    assert_eq!((lip.version, lip.flags, lip.length), (1, 1, 0));
    assert_eq!(lip.stem, b"OLDV1");
    assert_eq!(lip.phonemes, [13]);
    assert_eq!(lip.markers, [marker(1, 0), marker(1, 4410)]);
    assert!(lip.repeat());
    assert!(matches!(
        LipFile::parse(&bytes[..lip::HEADER_V1 - 1]),
        Err(LipError::Truncated { .. })
    ));
}

#[test]
fn spec010_lip_paths_units_and_volume() {
    assert_eq!(
        lip::lip_path(b"aradesh", b"ara_1001.lip").as_deref(),
        Some("sound/speech/aradesh/ara_1001.LIP")
    );
    // At most 8 stem characters; a dotted head stem is rejected.
    assert_eq!(
        lip::lip_path(b"harry", b"harry_0abc").as_deref(),
        Some("sound/speech/harry/harry_0a.LIP")
    );
    assert_eq!(lip::lip_path(b"bad.stem", b"x"), None);
    // LIP-D: 44100 bytes is one second, sample 22050.
    assert_eq!(lip::BYTES_PER_SECOND, 2 * 22050);
    // §3.12 safe rule: floor(69 x V / 100).
    assert_eq!(lip::speech_volume(32767), 22609);
    assert_eq!(lip::speech_volume(22281), 15373);
    assert_eq!(lip::speech_volume(300), 207);
    assert_eq!(lip::speech_volume(0), 0);
    // The missing-file rule: one phoneme 0 that lasts past the audio end.
    let silent = LipFile::silent();
    assert_eq!(silent.phonemes, [0]);
    assert_eq!(silent.position(1), i32::MAX as u64);
    assert!(silent.check().is_empty());
}

/// Every corpus `.LIP` parses with the §2.6 layout and matches the §5 scan.
#[test]
#[ignore = "needs the local corpus"]
fn corpus_lip_files_parse() {
    let Some(corpus) = crate::tests::corpus_dir() else {
        return;
    };
    let root = corpus.join("fallout1-unpacked/sound/speech");
    let mut files = Vec::new();
    for head in std::fs::read_dir(&root).unwrap().flatten() {
        for entry in std::fs::read_dir(head.path()).unwrap().flatten() {
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("lip"))
            {
                files.push(path);
            }
        }
    }
    assert_eq!(files.len(), 1601);
    let (mut repeat, mut before_end) = (0, 0);
    for path in &files {
        let bytes = std::fs::read(path).unwrap();
        let lip = LipFile::parse(&bytes).unwrap_or_else(|error| panic!("{path:?}: {error}"));
        let (p, m) = (lip.phonemes.len(), lip.markers.len());
        assert_eq!(lip.version, 2, "{path:?}");
        assert_eq!(bytes.len(), 44 + p + 8 * m, "{path:?}");
        assert_eq!(p + 1, m, "{path:?}");
        assert!((5..=508).contains(&m), "{path:?}");
        assert!(lip.check().is_empty(), "{path:?}: {:?}", lip.check());
        assert!(matches!(lip.flags, 0 | 4), "{path:?}");
        repeat += usize::from(lip.repeat());
        assert_eq!(lip.markers[0], marker(1, 0), "{path:?}");
        assert!(
            lip.markers
                .windows(2)
                .all(|w| w[0].position < w[1].position),
            "{path:?}"
        );
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        assert!(
            lip.stem.eq_ignore_ascii_case(stem.as_bytes()),
            "{path:?}: stored stem {:?}",
            String::from_utf8_lossy(&lip.stem)
        );
        let acm = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .find(|candidate| {
                candidate.file_stem().is_some_and(|s| {
                    s.to_string_lossy()
                        .eq_ignore_ascii_case(&String::from_utf8_lossy(&lip.stem))
                }) && candidate
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("acm"))
            })
            .unwrap_or_else(|| panic!("{path:?}: no ACM"));
        let samples = i64::from(
            crate::acm::AcmHeader::parse(&std::fs::read(acm).unwrap())
                .unwrap()
                .samples,
        );
        let length = i64::from(lip.length);
        assert!(
            length == 2 * samples || length == 2 * samples + 1,
            "{path:?}"
        );
        let last = i64::from(lip.markers[m - 1].position);
        assert!((-3019..=24093).contains(&(last - 2 * samples)), "{path:?}");
        before_end += usize::from(last < 2 * samples);
    }
    assert_eq!((repeat, before_end), (0, 202));
}
