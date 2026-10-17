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
    assert!(GamFile::parse(b"MAP_GLOBAL_VARS:\nbad = ;", GamSection::Map).is_err());
    assert!(GamFile::parse(b"MAP_GLOBAL_VARS:\nx=2147483648;", GamSection::Map).is_err());
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
