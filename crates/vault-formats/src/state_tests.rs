use crate::{GamFile, GamSection, GcdFile, fixture_assets};

#[test]
fn spec006_gam_order_markers_chunks_and_mutations() {
    let campaign = include_str!("../../../tests/fixtures/state/campaign.gam");
    for text in [
        campaign.to_owned(),
        campaign.replace('\n', "\r\n"),
        campaign.replace("GAME_GLOBAL_VARS:", "GAME_GLOBAL_VARSX"),
    ] {
        assert_eq!(
            GamFile::parse(text.as_bytes(), GamSection::Campaign)
                .unwrap()
                .values,
            [7, -2, 9, 0, 0, 0, 0, 12, 0, 3]
        );
    }
    let text = include_str!("../../../tests/fixtures/state/map.gam");
    assert_eq!(
        GamFile::parse(text.as_bytes(), GamSection::Map)
            .unwrap()
            .values,
        [4, -1]
    );
    for marker in [
        " MAP_GLOBAL_VARS:",
        "map_global_vars:",
        "MAP_GLOBAL_VARSX",
        "\u{feff}MAP_GLOBAL_VARS:",
    ] {
        assert!(
            GamFile::parse(
                text.replace("MAP_GLOBAL_VARS:", marker).as_bytes(),
                GamSection::Map
            )
            .unwrap()
            .values
            .is_empty()
        );
    }
    for (line, expected) in [
        (b" \n".as_slice(), vec![0]),
        (b";\n", vec![0]),
        (b"//=9\n", vec![]),
    ] {
        let bytes = [b"MAP_GLOBAL_VARS:\n".as_slice(), line].concat();
        assert_eq!(
            GamFile::parse(&bytes, GamSection::Map).unwrap().values,
            expected
        );
    }
    for (length, count) in [(257, 1), (258, 2)] {
        let mut bytes = b"MAP_GLOBAL_VARS:\n".to_vec();
        bytes.extend(std::iter::repeat_n(b'X', length));
        bytes.push(b'\n');
        assert_eq!(
            GamFile::parse(&bytes, GamSection::Map)
                .unwrap()
                .values
                .len(),
            count
        );
    }
    // Safe rule: no convertible number stores zero; overflow clamps.
    for (text, value) in [
        (&b"bad = ;"[..], 0),
        (b"x=-;", 0),
        (b"x=2147483648;", i32::MAX),
        (b"x=-2147483649;", i32::MIN),
        (b"x=\x0b7;", 7),
    ] {
        let bytes = [&b"MAP_GLOBAL_VARS:\n"[..], text].concat();
        assert_eq!(
            GamFile::parse(&bytes, GamSection::Map).unwrap().values,
            [value]
        );
    }
}
#[test]
fn spec006_gcd_fields_normalization_and_bounds() {
    let bytes =
        fixture_assets::sparse(include_str!("../../../tests/fixtures/state/player.gcd.hex"));
    assert_eq!(bytes.len(), 428);
    let sheet = GcdFile::parse(&bytes).unwrap();
    assert_eq!(&sheet.base[..7], &[5; 7]);
    assert_eq!(sheet.bonus[0], 2);
    assert_eq!((sheet.skills[0], sheet.skills[14]), (4, 2));
    assert_eq!(sheet.tags, [0, 14, -1, -1]);
    assert_eq!(sheet.traits, [-1, -1]);
    assert_eq!(sheet.name_bytes(), b"Fixture Six");
    // Age 25, gender 0, no remaining points; body/reward/kill and flags zero.
    assert_eq!(
        (sheet.base[33], sheet.base[34], sheet.remaining_points),
        (25, 0, 0)
    );
    assert_eq!(
        (
            sheet.body,
            sheet.experience_reward,
            sheet.kill_category,
            sheet.flags
        ),
        (0, 0, 0, 0)
    );
    for end in 0..bytes.len() {
        assert!(GcdFile::parse(&bytes[..end]).is_err());
    }
    let mut changed = bytes.clone();
    for offset in [0x164, 0x168, 0x16c, 0x78] {
        changed[offset..offset + 4].copy_from_slice(&7_i32.to_be_bytes());
    }
    let raw = GcdFile::parse(&changed).unwrap();
    assert_eq!(raw.body, 7);
    let player = raw.player_normalized();
    assert_eq!(
        (
            player.body,
            player.experience_reward,
            player.kill_category,
            player.base[29]
        ),
        (0, 0, 0, 100)
    );
    changed[0x170..0x190].fill(b'x');
    assert!(GcdFile::parse(&changed).is_err());
    let mut bad = bytes;
    bad[0x190..0x194].copy_from_slice(&18_i32.to_be_bytes());
    assert!(GcdFile::parse(&bad).is_err());
}
/// SPEC-018 §8.15 (Revision 4, Team A): S428 is SPEC-006 §4.2's sheet with
/// tags (3, 8, 12, -1) and traits (1, 9); S432 is S428 with the extra word
/// 3 inserted at `0x170`.
#[test]
fn spec018_fallout2_gcd_has_an_extra_word_before_the_name() {
    use vault_fixtures::state::sheet_gcd;
    let (tags, traits) = ([3, 8, 12, -1], [1, 9]);
    let s428 = sheet_gcd(b"Fixture Six", tags, traits, None);
    assert_eq!(s428.len(), crate::gcd::GCD_LEN);
    assert_eq!(&s428[0x190..0x194], &3_i32.to_be_bytes());
    assert_eq!(&s428[0x1A0..0x1A4], &1_i32.to_be_bytes());
    let read = GcdFile::parse(&s428).unwrap();
    assert_eq!(read.extra_word, None);
    assert_eq!((read.tags, read.traits), (tags, traits));
    let s432 = sheet_gcd(b"Fixture Six", tags, traits, Some(3));
    assert_eq!(s432.len(), crate::gcd::GCD2_LEN);
    assert_eq!(&s432[0x170..0x174], &[0, 0, 0, 3]);
    assert_eq!(&s432[0x174..0x17F], b"Fixture Six");
    assert_eq!(s432[0x17F], 0);
    let read = GcdFile::parse(&s432).unwrap();
    assert_eq!(read.extra_word, Some(3));
    assert_eq!(read.name_bytes(), b"Fixture Six");
    assert_eq!((read.tags, read.traits), (tags, traits));
    assert_eq!(read.remaining_points, 0);
    assert_eq!(read.base, GcdFile::parse(&s428).unwrap().base);
    // The defect ruled out: S432 read at the 428-byte offsets would give
    // an empty name, tags (0, 3, 8, 12), traits (-1, 1) and 9 points.
    let word = |at: usize| i32::from_be_bytes(s432[at..at + 4].try_into().unwrap());
    assert_eq!(s432[0x170], 0);
    assert_eq!([0x190, 0x194, 0x198, 0x19C].map(word), [0, 3, 8, 12]);
    assert_eq!([0x1A0, 0x1A4].map(word), [-1, 1]);
    assert_eq!(word(0x1A8), 9);
    assert_ne!((read.tags, read.remaining_points), ([0, 3, 8, 12], 9));
    // Each layout writes back byte for byte.
    assert_eq!(GcdFile::parse(&s428).unwrap().write(), s428);
    assert_eq!(read.write(), s432);
    // Refused: 430 bytes, 433 bytes, and every prefix of S432 but its
    // 428-byte one (which SPEC-006 §2.3's rules judge).
    let mut s430 = s428.clone();
    s430.extend([0; 2]);
    assert!(GcdFile::parse(&s430).is_err());
    let mut s433 = s432.clone();
    s433.push(0);
    assert!(GcdFile::parse(&s433).is_err());
    for end in (0..s432.len()).filter(|end| *end != 428) {
        assert!(GcdFile::parse(&s432[..end]).is_err());
    }
}
