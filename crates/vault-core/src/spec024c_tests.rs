//! SPEC-024c §6.1: the oracle's sheets and their effect, startup keys, and
//! tables. Everything here is `oracle/SPEC-024c/`'s synthetic data.
use crate::{
    CharacterSheet, GameProfile, StatContext, WorldHost, WorldState, sfall::GlobalKey,
    sfall_ini::IniFiles, spec024_tests::sfall_host, state_hash::fnv1a64, test_support::TestAssets,
    ui::sheet::sheet_gcd,
};
use vault_formats::{GcdFile, SheetAppearance};
use vault_vm::sfall::SfallValue;

const VECTORS: &str = include_str!("../../../oracle/SPEC-024c/gcd-vectors.tsv");
const EFFECT: &str = include_str!("../../../oracle/SPEC-024c/gcd-effect.tsv");
const LAYOUT: &str = include_str!("../../../oracle/SPEC-024c/gcd-layout.tsv");

/// `gcd-gN.hex`: spaced bytes, in lines.
fn hex(text: &str) -> Vec<u8> {
    text.split_whitespace()
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect()
}
fn sheets() -> [Vec<u8>; 5] {
    [
        hex(include_str!("../../../oracle/SPEC-024c/gcd-g1.hex")),
        hex(include_str!("../../../oracle/SPEC-024c/gcd-g2.hex")),
        hex(include_str!("../../../oracle/SPEC-024c/gcd-g3.hex")),
        hex(include_str!("../../../oracle/SPEC-024c/gcd-g4.hex")),
        hex(include_str!("../../../oracle/SPEC-024c/gcd-g5.hex")),
    ]
}
/// A cell of ints, or `None` for `-`.
fn word(cell: &str) -> Option<i32> {
    cell.parse().ok()
}
fn words(cell: &str) -> Vec<i32> {
    cell.split(',')
        .map(|value| value.parse().unwrap())
        .collect()
}

/// §6.1 test 1: G1 to G5 decode as the table says.
#[test]
fn t1_each_sheet_decodes_as_its_vector_says() {
    let sheets = sheets();
    for (index, line) in VECTORS.lines().skip(1).take(5).enumerate() {
        let cells: Vec<&str> = line.split('\t').collect();
        let bytes = &sheets[index];
        assert_eq!(bytes.len().to_string(), cells[1], "{line}");
        assert_eq!(format!("{:016X}", fnv1a64(bytes)), cells[2], "{line}");
        let gcd = GcdFile::parse(bytes).unwrap();
        assert_eq!(gcd.name_bytes(), cells[3].as_bytes(), "{line}");
        assert_eq!(gcd.tags.to_vec(), words(cells[4]), "{line}");
        assert_eq!(gcd.traits.to_vec(), words(cells[5]), "{line}");
        assert_eq!(gcd.remaining_points.to_string(), cells[6], "{line}");
        assert_eq!(gcd.extra_word, word(cells[7]), "{line}");
        assert_eq!(gcd.appearance.map(|a| a.race), word(cells[8]), "{line}");
        assert_eq!(gcd.appearance.map(|a| a.style), word(cells[9]), "{line}");
    }
}
/// §6.1 test 1: G6 to G8, the lengths no layout has.
#[test]
fn t1_the_other_lengths_are_refused_with_no_player_state_changed() {
    let g3 = sheets()[2].clone();
    let mut g7 = g3.clone();
    g7.extend([0; 4]);
    let refused: Vec<&str> = VECTORS.lines().skip(6).collect();
    let bytes = [g3[..436].to_vec(), g7, Vec::new()];
    assert_eq!(refused.len(), 3);
    for (line, bytes) in refused.iter().zip(&bytes) {
        let cells: Vec<&str> = line.split('\t').collect();
        assert_eq!(bytes.len().to_string(), cells[1], "{line}");
        assert_eq!(format!("{:016X}", fnv1a64(bytes)), cells[2], "{line}");
        assert_eq!(cells[3], "refused");
        assert!(GcdFile::parse(bytes).is_err(), "{line}");
    }
}
/// `gcd-layout.tsv`: a field at its offset, in all three layouts.
#[test]
fn the_layout_table_gives_each_fields_offset() {
    let sheets = sheets();
    let (g1, g2, g5) = (&sheets[0], &sheets[1], &sheets[4]);
    let at = |bytes: &[u8], cell: &str| {
        let offset = usize::from_str_radix(&cell[2..], 16).unwrap();
        i32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
    };
    for line in LAYOUT.lines().skip(1) {
        let cells: Vec<&str> = line.split('\t').collect();
        match cells[2] {
            "Four tagged-skill indices" | "Two trait indices" | "Unspent character points" => {
                let read = at(g5, cells[5]);
                assert_eq!(read, at(g2, cells[4]), "{line}");
                assert_eq!(read, at(g1, cells[3]), "{line}");
            }
            "Player name" => {
                let name = usize::from_str_radix(&cells[5][2..], 16).unwrap();
                assert_eq!(&g5[name..name + 5], b"Synth", "{line}");
            }
            "Appearance race word" => assert_eq!(at(g5, cells[5]), 3, "{line}"),
            "Appearance style word" => assert_eq!(at(g5, cells[5]), 2, "{line}"),
            _ => {}
        }
    }
}
/// §6.1 test 2, on the file: parse and write are inverses.
#[test]
fn t2_a_load_and_a_save_give_identical_bytes() {
    for bytes in &sheets() {
        assert_eq!(&GcdFile::parse(bytes).unwrap().write(), bytes);
    }
}
/// §2.5 through the editor's SAVE, which writes the player's sheet: a sheet
/// keeps its length, and everything from the extra word on, under either
/// profile. SPEC-006 §2.3's load recomputes the derived stats, which the
/// synthetic sheets leave at 0, so those words of the 368 can change once;
/// after that a load and a save give identical bytes.
#[test]
fn the_editors_save_keeps_a_sheets_length_and_its_words() {
    let saved = |bytes: &[u8], game| {
        let gcd = GcdFile::parse(bytes).unwrap();
        let sheet = CharacterSheet::from_gcd(&gcd, StatContext::default());
        sheet_gcd(&sheet, game).write()
    };
    let sheets = sheets();
    for (index, game) in [
        (4, GameProfile::Fallout2),
        (4, GameProfile::Fallout1),
        (1, GameProfile::Fallout2),
        (0, GameProfile::Fallout1),
    ] {
        let once = saved(&sheets[index], game);
        assert_eq!(once.len(), sheets[index].len(), "G{}", index + 1);
        assert_eq!(once[0x170..], sheets[index][0x170..], "G{}", index + 1);
        assert_eq!(saved(&once, game), once, "G{}", index + 1);
    }
    // A sheet with no words under Fallout 2 is SPEC-018's 432 bytes.
    let plain = saved(&sheets[0], GameProfile::Fallout2);
    assert_eq!(plain.len(), 432);
}
/// §2.3: the words are not the player's: nothing else reads them.
#[test]
fn the_words_change_no_stat_and_no_hash() {
    let context = StatContext::default();
    let sheets = sheets();
    let of = |bytes: &[u8]| CharacterSheet::from_gcd(&GcdFile::parse(bytes).unwrap(), context);
    let (mut g5, g3) = (of(&sheets[4]), of(&sheets[2]));
    assert_eq!(
        g5.read_words,
        Some((0, SheetAppearance { race: 3, style: 2 }))
    );
    g5.read_words = g3.read_words;
    assert_eq!(g5, g3);
    let hash = |sheet: CharacterSheet| WorldState::new(sheet, 1).unwrap().state_hash();
    assert_eq!(hash(of(&sheets[4])), hash(of(&sheets[2])));
}

/// A writable folder that holds one `ddraw.ini`.
struct Folder(Vec<u8>);
impl IniFiles for Folder {
    fn read(&self, file: &str) -> Option<Vec<u8>> {
        (file == "ddraw.ini").then(|| self.0.clone())
    }
    fn write(&mut self, _file: &str, _text: &[u8]) -> Result<(), String> {
        Ok(())
    }
}
/// §6.1 test 3: A1 to A6.
#[test]
fn t3_each_effect_vector_gives_its_global_and_its_stored_style() {
    for line in EFFECT.lines().skip(1) {
        let cells: Vec<&str> = line.split('\t').collect();
        let (sfall, mode, bytes, style) = (cells[1] == "on", cells[2], cells[3], word(cells[4]));
        let appearance = (bytes == "440").then(|| SheetAppearance {
            race: 0,
            style: style.unwrap(),
        });
        let (mut host, _) = sfall_host();
        let ini = format!("[Misc]\nEnableHeroAppearanceMod={mode}\n").into_bytes();
        host.set_sfall_ini_files(Box::new(Folder(ini)));
        if !sfall {
            host.sfall = None;
            host.world.sfall = None;
        }
        host.sfall_take_sheet(appearance);
        let global = host
            .world
            .sfall
            .as_ref()
            .and_then(|state| state.globals.get(&GlobalKey::Text(*b"HApStyle")));
        match cells[5] {
            "no write" => assert_eq!(global, None, "{line}"),
            value => {
                let value = SfallValue::Int(value.parse().unwrap());
                assert_eq!(global, Some(&value), "{line}");
            }
        }
        if sfall {
            assert_eq!(host.hero_style_for_test().to_string(), cells[6], "{line}");
        }
    }
}
/// The global reaches the sidecar like any other (§2.4), and `get_sfall_global`
/// reads the eight-byte key.
#[test]
fn the_global_is_in_the_sidecar_and_in_the_hash() {
    let (mut host, _): (WorldHost<TestAssets>, _) = sfall_host();
    host.set_sfall_ini_files(Box::new(Folder(
        b"[Misc]\nEnableHeroAppearanceMod=1\n".to_vec(),
    )));
    let before = host.world.sfall_hash();
    host.sfall_take_sheet(Some(SheetAppearance { race: 0, style: 2 }));
    assert_ne!(host.world.sfall_hash(), before);
    let sidecar = host.world.sfall.as_ref().unwrap().sidecar();
    let read = crate::sfall::SfallWorld::from_sidecar(&sidecar).unwrap();
    let key = GlobalKey::Text(*b"HApStyle");
    assert_eq!(read.globals.get(&key), Some(&SfallValue::Int(2)));
}
