//! SPEC-014 §2.2 splash images, §3.30 narration subtitles and §3.32 credit
//! text.
use crate::{
    credits::{self, CreditKind, CreditLine},
    rix::{self, Splash},
};

/// Fixture A: the first 16 bytes of a synthetic splash, the rest zero.
fn splash() -> Vec<u8> {
    let mut bytes = vec![0; rix::FILE_LEN];
    bytes[..16].copy_from_slice(&[
        0x52, 0x49, 0x58, 0x33, 0x80, 0x02, 0xE0, 0x01, 0xAF, 0x00, 0x3F, 0x00, 0x00, 0x00, 0x3F,
        0x00,
    ]);
    bytes
}

#[test]
fn spec014_fixture_a_splash_layout() {
    let mut bytes = splash();
    assert_eq!(bytes.len(), 307_978);
    bytes[0x030A] = 7;
    bytes[0x4B309] = 9;
    let image = Splash::parse(&bytes).unwrap();
    assert_eq!(image.palette[0], [63, 0, 0]);
    assert_eq!(image.palette[1][..2], [0, 63]);
    assert_eq!(
        (image.pixel(0, 0), image.pixel(639, 479)),
        (Some(7), Some(9))
    );
    assert_eq!(image.pixel(640, 0), None);
    // The header bytes are not read: any signature or size is accepted.
    bytes[..10].fill(0xFF);
    assert_eq!(Splash::parse(&bytes).unwrap().palette, image.palette);
    // Safe rule: a short splash is rejected; trailing bytes are ignored.
    assert!(Splash::parse(&bytes[..rix::FILE_LEN - 1]).is_err());
    bytes.push(1);
    assert!(Splash::parse(&bytes).is_ok());
}

#[test]
fn spec014_fixture_e_credit_rows() {
    let text = b";comment line\r\n@PRODUCER\r\nJane Example\r\n#Grey Name\r\n\r\n@DESIGN\r\n";
    let rows = credits::parse_credits(text);
    let row = |kind, text: &[u8]| CreditLine {
        kind,
        text: text.to_vec(),
    };
    assert_eq!(
        rows,
        [
            row(CreditKind::Title, b"PRODUCER"),
            row(CreditKind::Name, b"Jane Example"),
            row(CreditKind::GreyName, b"Grey Name"),
            row(CreditKind::Name, b""),
            row(CreditKind::Title, b"DESIGN"),
        ]
    );
    // LF-only files and a missing final terminator read the same.
    assert_eq!(
        credits::parse_credits(b";c\n@PRODUCER\nJane Example\n#Grey Name\n\n@DESIGN"),
        rows
    );
}

#[test]
fn spec014_narration_lines_and_timing() {
    let lines = credits::parse_narration(b"1:First line.\r\nno colon\r\n2:a:b\r\n");
    assert_eq!(lines, [b"First line.".to_vec(), b"a:b".to_vec()]);
    // Reads go through a 255-byte buffer: the 254-byte read keeps its
    // colon, and the colon-less remainder is dropped.
    let mut long = b"1:".to_vec();
    long.extend([b'x'; 300]);
    long.extend(b"\n2:next\n");
    let lines = credits::parse_narration(&long);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].len(), 252);
    assert_eq!(lines[1], b"next");
    // At most 50 lines.
    let many: Vec<u8> = (0..60)
        .flat_map(|i| format!("{i}:line\n").into_bytes())
        .collect();
    assert_eq!(credits::parse_narration(&many).len(), 50);
    // 20 characters over 4 seconds: 0.2 s each; 0.08 s without narration.
    let lines = [vec![b'a'; 5], vec![b'b'; 15]];
    assert_eq!(credits::narration_timing(&lines, Some(4)), [1000, 4000]);
    assert_eq!(credits::narration_timing(&lines, None), [400, 1600]);
}
