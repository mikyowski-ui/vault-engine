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
#[test]
fn spec018_fallout2_gcd_has_a_damage_type_word_before_the_name() {
    use vault_fixtures::state::sheet_gcd;
    let (tags, traits) = ([4, 0, 5, -1], [6, 15]);
    // SPEC-006's 428 bytes: no damage type.
    let one = sheet_gcd(b"Fixture Fighter", tags, traits, None);
    assert_eq!(one.len(), crate::gcd::GCD_LEN);
    let read = GcdFile::parse(&one).unwrap();
    assert_eq!(read.damage_type, None);
    assert_eq!((read.tags, read.traits), (tags, traits));
    // SPEC-018 §6.3 (Revision 4): 432 bytes, the word at 0x170 and the
    // player portion 4 bytes further on. Read with the 428-byte layout,
    // the first three tags would be [0, 4, 0]: Small Guns twice.
    let two = sheet_gcd(b"Fixture Fighter", tags, traits, Some(3));
    assert_eq!(two.len(), crate::gcd::GCD2_LEN);
    assert_eq!(&two[0x174..0x183], b"Fixture Fighter");
    assert_eq!(&two[0x194..0x198], &4_i32.to_be_bytes());
    let read = GcdFile::parse(&two).unwrap();
    assert_eq!(read.damage_type, Some(3));
    assert_eq!(read.name_bytes(), b"Fixture Fighter");
    assert_eq!((read.tags, read.traits), (tags, traits));
    assert_eq!(read.remaining_points, 0);
    assert_eq!(read.base, GcdFile::parse(&one).unwrap().base);
    // Each layout writes back byte for byte.
    assert_eq!(GcdFile::parse(&one).unwrap().write(), one);
    assert_eq!(read.write(), two);
    // Any other length is refused, including a 428-byte sheet with
    // trailing bytes and every prefix of the 432-byte one but 428.
    let mut longer = one.clone();
    longer.extend([0; 2]);
    assert!(GcdFile::parse(&longer).is_err());
    for end in (0..two.len()).filter(|end| *end != 428) {
        assert!(GcdFile::parse(&two[..end]).is_err());
    }
    let mut extra = two;
    extra.push(0);
    assert!(GcdFile::parse(&extra).is_err());
}
