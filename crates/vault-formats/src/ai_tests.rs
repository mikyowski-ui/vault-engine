//! SPEC-009 §4.1 Fixture A: `ai.txt` grammar, sorting, and resolution.
use crate::ai::*;

const ZETA: &str = "[Zeta Guard]
packet_num=0
max_dist=8
min_to_hit=40
min_hp=12 ; low-hp flee
aggression=50
hurt_too_much=Blind, crippled_legs
secondary_freq=4
called_freq=3
font=101
color=145
outline_color=-1
chance=10
chance=35
run_start=1000
move_start=1010
attack_start=1020
miss_start=1030
hit_head_start=1040
hit_left_arm_start=1050
hit_right_arm_start=1060
hit_torso_start=1070
hit_right_leg_start=1080
hit_left_leg_start=1090
hit_eyes_start=1100
hit_groin_start=1110
last_msg=1119";

const ALPHA: &str = "[alpha brute]
packet_num=7
max_dist=6 hexes
min_to_hit=25
min_hp=0
aggression=0
hurt_too_much=blind ,crippled
secondary_freq=
called_freq=0
font=101
color=0x50
outline_color=0
chance=100
run_start=2000
move_start=2001
attack_start=2002
miss_start=2003
hit_head_start=2004
hit_left_arm_start=2004
hit_right_arm_start=2004
hit_torso_start=2004
hit_right_leg_start=2004
hit_left_leg_start=2004
hit_eyes_start=2004
hit_groin_start=2004
last_msg=2003";

/// Fixture A with CRLF physical line endings.
fn fixture(zeta: &str, empty: &str, alpha: &str) -> Vec<u8> {
    ["; synthetic packets", zeta, empty, alpha]
        .iter()
        .flat_map(|block| block.lines())
        .map(|line| format!("{line}\r\n"))
        .collect::<String>()
        .into_bytes()
}
fn standard() -> Vec<u8> {
    fixture(ZETA, "[Empty Header]", ALPHA)
}

#[test]
fn spec009_fixture_a_grammar_sorting_and_resolution() {
    let packets = Packets::parse(&standard()).unwrap();
    // `Empty Header` holds no keys, so it creates no packet.
    assert_eq!(packets.len(), 2);
    let names: Vec<_> = packets.packets.iter().map(|p| p.name.as_slice()).collect();
    assert_eq!(names, [b"alpha brute".as_slice(), b"Zeta Guard"]);
    let (alpha, zeta) = (&packets.packets[0], &packets.packets[1]);
    assert_eq!((alpha.packet_num, zeta.packet_num), (7, 0));
    assert_eq!(zeta.chance, 35, "last duplicate wins");
    assert_eq!(zeta.min_hp, 12, "comment stripped, value trimmed");
    assert_eq!(zeta.hurt_too_much, 0x4c);
    assert_eq!(zeta.outline_color, -1);
    assert_eq!(
        zeta.hit_start,
        [1040, 1050, 1060, 1070, 1080, 1090, 1100, 1110]
    );
    assert_eq!(zeta.last_msg, 1119);
    assert_eq!(alpha.max_dist, 6, "decimal prefix stops at the space");
    assert_eq!(alpha.hurt_too_much, 0x3c);
    assert_eq!((alpha.secondary_freq, alpha.called_freq), (0, 0));
    assert_eq!(alpha.color, 0, "parsing 0x50 stops at x");
    assert_eq!(alpha.aggression, 0);
    // The trailing space keeps `blind ` from matching: one diagnostic.
    assert_eq!(packets.diagnostics.len(), 1);
    assert!(packets.diagnostics[0].contains("\"blind \""));
    // Resolution: the first sorted match, else table entry 0.
    assert_eq!(packets.resolve(0), (zeta, true));
    assert_eq!(packets.resolve(3), (alpha, false));
    assert_eq!(packets.resolve(7), (alpha, true));
    // The name query's guard compares with the table length.
    assert_eq!(packets.name(0), Some(b"Zeta Guard".as_slice()));
    assert_eq!(packets.name(7), None);
    assert_eq!(packets.name(-1), None);
    assert_eq!(packets.name(1), None);
}

#[test]
fn spec009_fixture_a_exact_line_bytes() {
    let config = Config::parse(b"\x63\x68\x61\x6E\x63\x65\x3D\x33\x35\x0D\x0A").unwrap();
    assert_eq!(config.sections.len(), 1);
    assert_eq!(config.sections[0].name, b"unknown");
    assert_eq!(config.sections[0].get(b"chance"), Some(b"35".as_slice()));
    let config =
        Config::parse(b"\x5B\x45\x6D\x70\x74\x79\x20\x48\x65\x61\x64\x65\x72\x5D\x0D\x0Ax=1\r\n")
            .unwrap();
    assert_eq!(config.sections[0].name, b"Empty Header");
}

#[test]
fn spec009_fixture_a_grammar_mutations() {
    // Deleting any required key fails the whole table.
    let missing = fixture(&ZETA.replace("last_msg=1119", ""), "[Empty Header]", ALPHA);
    assert!(Packets::parse(&missing).is_err());
    // `color=[x]` selects section x; later keys populate it, so it fails.
    let section = fixture(
        &ZETA.replace("color=145", "color=145\ncolor=[x]"),
        "",
        ALPHA,
    );
    let config = Config::parse(&section).unwrap();
    assert!(config.sections.iter().any(|s| s.name == b"x"));
    assert!(Packets::parse(&section).is_err());
    // Case-folded sorting is independent of file order and header case.
    let upper = fixture(ZETA, &ALPHA.replace("[alpha brute]", "[ALPHA BRUTE]"), "");
    let packets = Packets::parse(&upper).unwrap();
    assert_eq!(packets.packets[0].name, b"ALPHA BRUTE");
    assert_eq!(packets.packets[1].name, b"Zeta Guard");
    // A repeated header merges; the first occurrence keeps its name.
    let merged = fixture(ZETA, "[zeta guard]\nchance=1", ALPHA);
    let packets = Packets::parse(&merged).unwrap();
    assert_eq!(packets.len(), 2);
    assert_eq!(packets.packets[1].name, b"Zeta Guard");
    assert_eq!(packets.packets[1].chance, 1);
    // Keys before the first header form section `unknown`, which fails.
    let early = fixture("packet_num=3", ZETA, ALPHA);
    assert!(Packets::parse(&early).is_err());
    // An empty table is an initialization error, as is an embedded NUL.
    assert!(Packets::parse(b"[Empty]\r\n").is_err());
    assert!(Config::parse(b"a=1\0\r\n").is_err());
}

#[test]
fn spec009_reader_lines_keys_values_and_integers() {
    // A physical line longer than 255 bytes splits at byte 255.
    let long = format!("{}=v\nz=1", "k".repeat(300));
    let config = Config::parse(long.as_bytes()).unwrap();
    let section = &config.sections[0];
    assert_eq!(
        section.get("k".repeat(45).as_bytes()),
        Some(b"v".as_slice())
    );
    assert_eq!(section.get(b"z"), Some(b"1".as_slice()));
    // Empty keys are valid; a `[` without a later `]` stays in the text.
    let config =
        Config::parse(b"[s]\n=5\nkey[=a]b\n[s]\nname=x[y\n  spaced key  =  v v  \n").unwrap();
    // The `key[=a]b` header selects `=a`, which never holds a key and so
    // never exists; `[s]` then resumes the existing section.
    assert_eq!(config.sections.len(), 1);
    let section = &config.sections[0];
    assert_eq!(section.get(b""), Some(b"5".as_slice()));
    assert_eq!(section.get(b"key"), None, "[...] makes it a header line");
    assert_eq!(section.get(b"name"), Some(b"x[y".as_slice()));
    assert_eq!(section.get(b"SPACED KEY"), Some(b"v v".as_slice()));
    // `_` (0x5F) sorts before `a` (0x61); `Z` folds to `z`.
    let config = Config::parse(b"[b]\nk=1\n[Z]\nk=1\n[_u]\nk=1\n[a]\nk=1\n").unwrap();
    let order: Vec<_> = config.sections.iter().map(|s| s.name.clone()).collect();
    assert_eq!(
        order,
        [b"_u".to_vec(), b"a".to_vec(), b"b".to_vec(), b"Z".to_vec()]
    );
    // Decimal-prefix integers with saturation.
    assert_eq!(decimal_prefix(b"8 hexes"), (8, false));
    assert_eq!(decimal_prefix(b"-12"), (-12, false));
    assert_eq!(decimal_prefix(b"+7x"), (7, false));
    assert_eq!(decimal_prefix(b""), (0, false));
    assert_eq!(decimal_prefix(b"x9"), (0, false));
    assert_eq!(decimal_prefix(b"2147483647"), (i32::MAX, false));
    assert_eq!(decimal_prefix(b"-2147483648"), (i32::MIN, false));
    assert_eq!(decimal_prefix(b"99999999999999999999"), (i32::MAX, true));
    assert_eq!(decimal_prefix(b"-2147483649"), (i32::MIN, true));
    // hurt_too_much tokens.
    assert_eq!(hurt_mask(b""), (0, vec![]));
    assert_eq!(hurt_mask(b"CRIPPLED_ARMS"), (0x30, vec![]));
    assert_eq!(
        hurt_mask(b"blind,,crippled_legs, "),
        (0x4c, vec![vec![], vec![]])
    );
    assert_eq!(hurt_mask(b"\tblind"), (0, vec![b"\tblind".to_vec()]));
}
