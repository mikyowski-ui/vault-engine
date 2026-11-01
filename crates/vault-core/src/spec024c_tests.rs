//! SPEC-024c §6.1: the oracle's sheets and their effect, startup keys, and
//! tables. Everything here is `oracle/SPEC-024c/`'s synthetic data.
use crate::{
    CharacterSheet, GameProfile, StatContext, WorldHost, WorldState, sfall::GlobalKey,
    sfall_startup::StartupOptions, spec024_tests::sfall_host, state_hash::fnv1a64,
    test_support::TestAssets, ui::sheet::sheet_gcd,
};
use std::sync::Arc;
use vault_formats::{GcdFile, SheetAppearance};
use vault_vm::sfall::SfallValue;

const VECTORS: &str = include_str!("../../../oracle/SPEC-024c/gcd-vectors.tsv");
const EFFECT: &str = include_str!("../../../oracle/SPEC-024c/gcd-effect.tsv");
const LAYOUT: &str = include_str!("../../../oracle/SPEC-024c/gcd-layout.tsv");
const DDRAW: &str = include_str!("../../../oracle/SPEC-024c/ddraw-vectors.tsv");
const DDRAW_FILE: &str = include_str!("../../../oracle/SPEC-024c/fixture-ddraw.ini");
const DDRAW_EXPECTED: &str = include_str!("../../../oracle/SPEC-024c/fixture-ddraw-expected.tsv");

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
        let ini = format!("[Misc]\nEnableHeroAppearanceMod={mode}\n");
        host.apply_startup(Arc::new(StartupOptions::parse(ini.as_bytes())), None);
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
    let ini = b"[Misc]\nEnableHeroAppearanceMod=1\n";
    host.apply_startup(Arc::new(StartupOptions::parse(ini)), None);
    let before = host.world.sfall_hash();
    host.sfall_take_sheet(Some(SheetAppearance { race: 0, style: 2 }));
    assert_ne!(host.world.sfall_hash(), before);
    let sidecar = host.world.sfall.as_ref().unwrap().sidecar();
    let read = crate::sfall::SfallWorld::from_sidecar(&sidecar).unwrap();
    let key = GlobalKey::Text(*b"HApStyle");
    assert_eq!(read.globals.get(&key), Some(&SfallValue::Int(2)));
}

/// A vector's field of `options`, and every diagnostic reading it can give.
fn field(options: &StartupOptions, name: &str) -> (String, usize) {
    const WORLD: [i32; 2] = [1400, 1800];
    let exists = |path: &str| path == "maps/v13ent.map" || path == "maps/artemple.map";
    let start = options.new_game_start("maps/artemple.map", exists, WORLD);
    let holds = |name: &str| ["hmjmps", "hfjmps", "hmwarr"].contains(&name);
    let (models, model_notes) = options.checked_models(holds);
    let notes = options.notes.len() + start.notes.len() + model_notes.len();
    let or_unchanged =
        |model: &Option<String>| model.clone().unwrap_or_else(|| "unchanged".to_owned());
    let text = match name {
        "starting_map" => {
            let stem = start.map.strip_prefix("maps/").unwrap_or(&start.map);
            stem.strip_suffix(".map").unwrap_or(stem).to_owned()
        }
        "movie_2" => options
            .movie(2)
            .unwrap_or(crate::fallout2::MOVIES[2])
            .to_owned(),
        "start_tick" => options
            .start_ticks
            .unwrap_or(crate::fallout2::START_TICKS)
            .to_string(),
        "start_date" => {
            const MONTHS: [&str; 12] = [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ];
            let (year, day) = options.calendar();
            let [month, day, year] = crate::host_save::calendar_date(0, year, day);
            format!("{day} {} {year}", MONTHS[usize::from(month) - 1])
        }
        "world_pos" => start
            .world_pos
            .map_or("start area".to_owned(), |[x, y]| format!("{x},{y}")),
        "version" => String::from_utf8(
            options
                .version
                .clone()
                .unwrap_or_else(|| crate::ui::fallout2::MENU_VERSION.as_bytes().to_vec()),
        )
        .unwrap(),
        "premades" => {
            options
                .premades
                .as_ref()
                .map_or("combat,stealth,diplomat".to_owned(), |premades| {
                    let names: Vec<&str> = premades.iter().map(|(name, _)| name.as_str()).collect();
                    names.join(",")
                })
        }
        "male_start" => or_unchanged(&models.start[0]),
        "male_default" => or_unchanged(&models.default[0]),
        "menu_offset" => format!("{},{}", options.menu_offset[0], options.menu_offset[1]),
        "menu_colour" => options
            .menu_colour
            .map_or("unchanged".to_owned(), |colour| {
                let reach = if colour.version_only {
                    "version only"
                } else {
                    "version and copyright"
                };
                format!("palette index {}, {reach}", colour.index)
            }),
        other => panic!("{other}"),
    };
    (text, notes)
}
/// §6.1 test 4: every row, with its diagnostic count.
#[test]
fn t4_every_startup_vector_gives_its_value_and_its_diagnostics() {
    let mut rows = 0;
    for line in DDRAW.lines().skip(1) {
        let cells: Vec<&str> = line.split('\t').collect();
        let options = match cells[1] {
            "(no file)" => StartupOptions::default(),
            text => StartupOptions::parse(text.replace('|', "\n").as_bytes()),
        };
        let (value, notes) = field(&options, cells[2]);
        assert_eq!(value, cells[3], "{line}");
        assert_eq!(notes.to_string(), cells[4], "{line}");
        rows += 1;
    }
    assert_eq!(rows, 47);
}
/// §6.1 test 4: the whole block.
#[test]
fn t4_the_whole_startup_block_gives_everything_it_should() {
    let options = StartupOptions::parse(DDRAW_FILE.as_bytes());
    for line in DDRAW_EXPECTED.lines().skip(1) {
        let (name, expected) = line.split_once('\t').unwrap();
        if name == "diagnostics" {
            assert_eq!(field(&options, "world_pos").1.to_string(), expected);
        } else {
            assert_eq!(field(&options, name).0, expected, "{line}");
        }
    }
}
/// §3.2: the key and the section fold case, and -1 is unset, not invalid.
#[test]
fn numeric_keys_take_minus_1_as_unset_and_forms_are_checked() {
    let no_notes = [
        "[Misc]\nStartXPos=-1\nStartYPos=-1\n",
        "[Misc]\nStartYear=-1\nStartMonth=-1\nStartDay=-1\n",
        "[Misc]\nMainMenuFontColour=-1\n",
        "[Misc]\nMovie33=whatever.mve\n",
    ];
    for text in no_notes {
        let options = StartupOptions::parse(text.as_bytes());
        assert_eq!(options.notes, Vec::<String>::new(), "{text}");
        assert_eq!(options.calendar(), (2241, 205), "{text}");
    }
    for text in [
        "[Misc]\nStartYear=old\n",
        "[Misc]\nStartYear=-2\n",
        "[Misc]\nStartDay=31\nStartMonth=8\n",
        "[Misc]\nMovie3=elder\n",
        "[Misc]\nMainMenuFontColour=xyz\n",
        "[Misc]\nMainMenuOffsetX=left\n",
        "[Misc]\nStartXPos=x\nStartYPos=2\n",
        "[Misc]\nVersionString=100%\n",
        "[Misc]\nVersionString=1234567890123456789012345678901234567890123456789012345678901234\n",
        "[Misc]\nPremadePaths=a\n",
        "[Misc]\nPremadePaths=twelve_bytes,b\nPremadeFIDs=1,2\n",
    ] {
        let options = StartupOptions::parse(text.as_bytes());
        assert_eq!(options.notes.len(), 1, "{text}: {:?}", options.notes);
    }
}
/// §3.6: Et Tu's values, and a load keeps the file's calendar.
#[test]
fn the_calendar_counts_forward_from_the_files_date() {
    let (mut host, _) = sfall_host();
    let text = b"[Misc]\nStartYear=2161\nStartMonth=11\nStartDay=4\nStartTime=721\n";
    let startup = Arc::new(StartupOptions::parse(text));
    host.apply_startup(startup, None);
    assert_eq!(host.world.clock.ticks, 264600);
    assert_eq!(host.world.game_date(), [12, 5, 2161]);
    // It is the session's, so a load gets it back.
    let state = host.world.fallout2.as_deref_mut().unwrap();
    state.calendar = (
        crate::fallout2::CALENDAR_YEAR,
        crate::fallout2::CALENDAR_DAY,
    );
    host.sfall_restored();
    assert_eq!(host.world.game_date(), [12, 5, 2161]);
}
/// §3.5: `play_gmovie` uses the index's file, and its flags.
#[test]
fn play_gmovie_uses_the_files_movie() {
    let (mut host, _) = sfall_host();
    let text = b"[Misc]\nMovie3=ovrintro.mve\n";
    host.apply_startup(Arc::new(StartupOptions::parse(text)), None);
    host.world.effects.clear();
    host.play_gmovie(2);
    let played = host.world.effects.iter().find_map(|effect| match effect {
        crate::WorldEffect::Movie(request) => Some((request.path.clone(), request.flags)),
        _ => None,
    });
    assert_eq!(played, Some(("art/cuts/ovrintro.mve".to_owned(), 0x0b)));
}
