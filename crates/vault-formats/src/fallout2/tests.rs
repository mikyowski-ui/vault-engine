//! SPEC-018 §8 test vectors for the formats: install discovery and
//! mounting (§8.1), the grammar and tables (§8.2-§8.7, §8.12, §8.14), the
//! `SAVE.DAT` header (§8.8), the world-map and party components (§8.9,
//! §8.10) and the automap index (§8.11).
use super::{
    city::{AreaSize, CityTable},
    lines::{self, QuestStatus, SlideArt},
    maps::MapsTable,
    party::{ControlField, PartyTable},
    save::{AreaSave, Fallout2Shape, MemberRecord, PartySave2, WorldMapSave2},
    text::Diagnostics,
    worldmap::{Condition, ConditionContext, Distance, Fill, Relation, WorldmapTable},
    *,
};
use crate::{
    GameProfile::Fallout2,
    LocalDirProvider, Vfs, VfsError,
    automap::{AutomapDb, FALLOUT2_EMPTY_LEN},
    binary::Reader,
    save_tests::{fixture_header, lookup, synthetic_save},
    savegame::*,
};
use std::sync::Arc;
use vault_fixtures::{
    dat2::{Member, archive},
    fallout2 as fixture,
    formats::hex,
};

fn maps() -> MapsTable {
    MapsTable::parse(fixture::MAPS.as_bytes(), &mut Diagnostics::default()).unwrap()
}
fn worldmap() -> WorldmapTable {
    WorldmapTable::parse(
        fixture::WORLDMAP.as_bytes(),
        &maps(),
        &mut Diagnostics::default(),
    )
    .unwrap()
}

// §8.1 -----------------------------------------------------------------

fn members() -> Vec<Member> {
    REQUIRED_MEMBERS
        .iter()
        .map(|path| Member::stored(&path.to_ascii_uppercase().replace('/', "\\"), b"x"))
        .collect()
}

fn write(dir: &std::path::Path, name: &str, members: &[Member]) {
    std::fs::write(dir.join(name), archive(members)).unwrap();
}

fn validate(
    dir: &std::path::Path,
    loose: Option<&std::path::Path>,
) -> Result<Fallout2Install, String> {
    let install = Fallout2Install::discover(dir)?;
    let opened = install.open()?;
    let loose = loose.map(|root| Arc::new(LocalDirProvider::mount(root).unwrap()));
    let missing = missing_members(&opened.provider(loose));
    if let Some(member) = missing.first() {
        return Err(format!("missing {member}"));
    }
    Ok(install)
}

#[test]
fn spec018_install_discovery_and_validation() {
    let case = |names: &[&str]| {
        let dir = tempfile::tempdir().unwrap();
        for name in names {
            write(dir.path(), name, &members());
        }
        dir
    };
    // Valid; no patch.
    let dir = case(&["MASTER.DAT", "CRITTER.DAT"]);
    assert_eq!(validate(dir.path(), None).unwrap().patch, None);
    // Names fold case; PATCH000.DAT is mounted.
    let dir = case(&["master.dat", "Critter.Dat", "PATCH000.DAT"]);
    let install = validate(dir.path(), None).unwrap();
    assert_eq!(install.patch, Some(dir.path().join("PATCH000.DAT")));
    assert_eq!(install.master, dir.path().join("master.dat"));
    // Two entries that differ only in case refuse, naming both.
    let dir = case(&["MASTER.DAT", "CRITTER.DAT", "patch000.dat", "PATCH000.DAT"]);
    let error = validate(dir.path(), None).unwrap_err();
    assert!(
        error.contains("PATCH000.DAT") && error.contains("patch000.dat"),
        "{error}"
    );
    // A folder named patch000.dat is skipped.
    let dir = case(&["MASTER.DAT", "CRITTER.DAT"]);
    std::fs::create_dir(dir.path().join("patch000.dat")).unwrap();
    assert_eq!(validate(dir.path(), None).unwrap().patch, None);
    // patch001.dat is not mounted.
    let dir = case(&["MASTER.DAT", "CRITTER.DAT", "patch001.dat"]);
    assert_eq!(validate(dir.path(), None).unwrap().patch, None);
    // A patch000.dat with a bad footer refuses, naming it and the rule.
    let dir = case(&["MASTER.DAT", "CRITTER.DAT"]);
    let mut bad = archive(&members());
    let end = bad.len();
    bad[end - 1] ^= 0x40;
    std::fs::write(dir.path().join("patch000.dat"), bad).unwrap();
    let error = validate(dir.path(), None).unwrap_err();
    assert!(
        error.contains("patch000.dat") && error.contains("SPEC-015 §2"),
        "{error}"
    );
    // data/party.txt missing everywhere refuses, naming it; data_root may
    // hold it.
    let dir = tempfile::tempdir().unwrap();
    let without: Vec<Member> = REQUIRED_MEMBERS
        .iter()
        .filter(|path| **path != "data/party.txt")
        .map(|path| Member::stored(&path.to_ascii_uppercase().replace('/', "\\"), b"x"))
        .collect();
    write(dir.path(), "MASTER.DAT", &without);
    write(dir.path(), "CRITTER.DAT", &[]);
    assert_eq!(
        validate(dir.path(), None).unwrap_err(),
        "missing data/party.txt"
    );
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("data")).unwrap();
    std::fs::write(root.path().join("data/party.txt"), b"x").unwrap();
    assert!(validate(dir.path(), Some(root.path())).is_ok());
    // CRITTER.DAT missing refuses, naming it.
    let dir = case(&["MASTER.DAT"]);
    let error = validate(dir.path(), None).unwrap_err();
    assert!(error.contains("CRITTER.DAT"), "{error}");
    // A Fallout 1 MASTER.DAT is named as Fallout 1 data.
    let dir = case(&["CRITTER.DAT"]);
    std::fs::write(
        dir.path().join("MASTER.DAT"),
        vault_fixtures::formats::sparse(include_str!(
            "../../../../tests/fixtures/archive/synthetic.dat.hex"
        )),
    )
    .unwrap();
    assert!(crate::DatArchive::open(dir.path().join("MASTER.DAT")).is_ok());
    let error = validate(dir.path(), None).unwrap_err();
    assert!(error.contains("Fallout 1"), "{error}");
}

#[test]
fn spec018_overlay_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut master = members();
    master.retain(|member| member.path != "DATA\\MAPS.TXT");
    master.push(Member::stored("DATA\\MAPS.TXT", b"B"));
    write(dir.path(), "MASTER.DAT", &master);
    write(
        dir.path(),
        "patch000.dat",
        &[
            Member::stored("DATA\\MAPS.TXT", b"A"),
            Member::stored("ART\\CRITTERS\\TEST.FRM", b"patched"),
        ],
    );
    write(dir.path(), "CRITTER.DAT", &[]);
    let opened = Fallout2Install::discover(dir.path())
        .unwrap()
        .open()
        .unwrap();
    let provider = opened.provider(None);
    assert_eq!(provider.read("data/maps.txt").unwrap(), b"A");
    // The critter database has no patch.
    assert!(matches!(
        provider.read("art/critters/test.frm"),
        Err(VfsError::NotFound(_))
    ));
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("data")).unwrap();
    std::fs::write(root.path().join("data/maps.txt"), b"C").unwrap();
    let loose = Arc::new(LocalDirProvider::mount(root.path()).unwrap());
    assert_eq!(
        opened.provider(Some(loose)).read("data/maps.txt").unwrap(),
        b"C"
    );
}

// §8.2-§8.7 -------------------------------------------------------------

#[test]
fn spec018_text_grammar() {
    let mut notes = Diagnostics::default();
    let table = MapsTable::parse(fixture::GRAMMAR.as_bytes(), &mut notes).unwrap();
    assert_eq!(table.count(), 2);
    assert!(table.get(0).is_none());
    let map = table.get(1).unwrap();
    assert_eq!(map.lookup_name.as_deref(), Some("Test Village"));
    assert_eq!(map.stem, "TESTVIL");
    assert_eq!(map.path(), "maps/testvil.map");
    assert_eq!(map.match_text(), "TESTVIL.MAP");
    assert!(map.saved);
    assert_eq!(map.music.as_deref(), Some("05raider"));
    // The stray line and the repeated `saved`; bogus_key has no effect.
    assert_eq!(notes.0.len(), 2, "{:?}", notes.0);
    assert_eq!(notes.0[0].line, 2);
    assert!(
        notes.0[1].message.to_ascii_lowercase().contains("saved"),
        "{:?}",
        notes.0[1]
    );
    assert_eq!(notes.0[1].line, 7);
    // Structural faults refuse the file, naming it and the line.
    let error = MapsTable::parse(b"[Map 0]\nmap_name=a\0b\n", &mut notes).unwrap_err();
    assert_eq!((error.file.as_str(), error.line), ("data/maps.txt", 2));
    let long = format!("[Map 0]\nmap_name={}\n", "a".repeat(4096));
    assert_eq!(
        MapsTable::parse(long.as_bytes(), &mut notes)
            .unwrap_err()
            .line,
        2
    );
    assert!(MapsTable::parse(b"[Map 0]\nlookup_name=x\n", &mut notes).is_err());
}

#[test]
fn spec018_value_forms() {
    use super::text::*;
    assert_eq!(integer("-12"), Some(-12));
    assert_eq!(integer("+7"), Some(7));
    for bad in ["12tail", "0x10", "", "-", "12345678901", "2147483648"] {
        assert_eq!(integer(bad), None, "{bad}");
    }
    assert_eq!(integer("-2147483648"), Some(i32::MIN));
    assert_eq!(percent("22%"), Some(22));
    assert_eq!(percent("101"), None);
    assert_eq!(boolean("TRUE"), Some(true));
    assert_eq!(boolean("off"), Some(false));
    assert_eq!(list("a, (b, c) ,, d"), ["a", "(b, c)", "d"]);
    assert_eq!(stem("TESTVIL.MAP").as_deref(), Some("TESTVIL"));
    assert_eq!(stem("toolongname"), None);
    assert_eq!(name("Test Village").as_deref(), Some("Test Village"));
    assert_eq!(name("a,b"), None);
}

#[test]
fn spec018_maps_table() {
    let table = maps();
    assert_eq!(table.count(), 3);
    let map0 = table.get(0).unwrap();
    assert_eq!(map0.stem, "desert1");
    assert_eq!(map0.music.as_deref(), Some("07desert"));
    assert!(table.excluded(0) && !table.excluded(1) && !table.excluded(2));
    assert_eq!(map0.can_rest, [true; 3]);
    assert!(map0.pipboy_active && map0.dead_bodies_age);
    assert_eq!(map0.start_points.len(), 1);
    assert_eq!(
        (map0.start_points[0].elevation, map0.start_points[0].tile),
        (0, 19086)
    );
    let map1 = table.get(1).unwrap();
    assert_eq!(map1.music.as_deref(), Some("05raider"));
    assert!(map1.start_points.is_empty());
    let map2 = table.get(2).unwrap();
    assert_eq!(map2.music, None);
    assert_eq!(map2.can_rest, [false, false, true]);
    assert!(!map2.pipboy_active && !map2.dead_bodies_age);
    assert!(table.get(3).is_none());
    // Name matching.
    assert_eq!(table.match_name(b"ARTEMPLE.MAP"), 2);
    assert_eq!(table.match_name(b"artemple.sav"), 2);
    assert_eq!(table.match_name(b"artemple.map"), 2);
    assert_eq!(table.match_name(b"ARTEMPLE"), -1);
    assert_eq!(table.match_name(b"MAPS\\ARTEMPLE.MAP"), -1);
    assert_eq!(table.match_name(b"ARTEMPLE.MAP____"), -1);
    // Current-map assignment: by name, not the stored word 7.
    assert_eq!(table.header_number(b"artemple.map", 7), 2);
    assert_eq!(table.header_number(b"ARTEMPLE.sav", 7), 7);
    assert!(table.excluded(-1) && table.excluded(3));
}

#[test]
fn spec018_city_table() {
    let maps = maps();
    let world = worldmap().world_size();
    assert_eq!(world, [350, 300]);
    let city = CityTable::parse(
        fixture::CITY.as_bytes(),
        &maps,
        world,
        &mut Diagnostics::default(),
    )
    .unwrap();
    assert_eq!(city.count(), 1);
    let area = city.get(0).unwrap();
    assert_eq!(area.name, "Test Village");
    assert!(area.start_known && !area.start_locked);
    assert_eq!(area.size, AreaSize::Large);
    assert_eq!((area.townmap_art, area.townmap_label_art), (156, -1));
    assert_eq!(area.entrances.len(), 3);
    let slot0 = area.entrances[0].unwrap();
    assert!(slot0.known);
    assert_eq!(
        (slot0.map, slot0.elevation, slot0.tile, slot0.orientation),
        (1, -1, -1, 0)
    );
    assert!(area.entrances[1].is_none());
    let slot2 = area.entrances[2].unwrap();
    assert!(!slot2.known);
    assert_eq!(
        (slot2.map, slot2.elevation, slot2.tile, slot2.orientation),
        (2, 1, 12345, -1)
    );
    assert_eq!(city.area_of_map(1), 0);
    assert_eq!(city.area_of_map(2), 0);
    assert_eq!(city.area_of_map(0), -1);
    assert_eq!(city.area_of_map(-1), -1);
    // Entrances by map (metarule3 104).
    assert_eq!(city.entrances_by_map(2, 1), [(0, 2)]);
    assert_eq!(city.entrances_by_map(2, 0), []);
    assert_eq!(city.entrances_by_map(1, 2), [(0, 0)]);
    assert_eq!(AreaSize::Medium.radius(), 13);
    // An orientation of 8 drops the entrance; no valid area refuses.
    let city = CityTable::parse(
        b"[Area 0]\nworld_pos=1,1\nentrance_0=On,1,1,Temple,-1,-1,8\n",
        &maps,
        world,
        &mut Diagnostics::default(),
    )
    .unwrap();
    assert!(city.get(0).unwrap().entrances.is_empty());
    assert!(
        CityTable::parse(
            b"[Area 1]\nworld_pos=999,1\n",
            &maps,
            world,
            &mut Diagnostics::default()
        )
        .is_err()
    );
}

struct Context {
    globals: Vec<i32>,
    level: i32,
    draws: Vec<i32>,
    drawn: usize,
}
impl ConditionContext for Context {
    fn global(&self, index: i32) -> i32 {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.globals.get(index))
            .copied()
            .unwrap_or(0)
    }
    fn player_level(&self) -> i32 {
        self.level
    }
    fn ticks(&self) -> u64 {
        264600
    }
    fn time_of_day(&self) -> i32 {
        721
    }
    fn percent_draw(&mut self) -> i32 {
        self.drawn += 1;
        self.draws[self.drawn - 1]
    }
}

#[test]
fn spec018_worldmap_table() {
    let table = worldmap();
    let frequency = |name: &str| table.frequencies[name];
    assert_eq!(
        ["forced", "frequent", "common", "uncommon", "rare", "none"].map(frequency),
        [100, 38, 22, 12, 4, 0]
    );
    assert_eq!(table.terrains.len(), 2);
    assert_eq!(
        (
            table.terrains[1].name.as_str(),
            table.terrains[1].difficulty
        ),
        ("Mountain", 2)
    );
    assert_eq!((table.across, table.tiles.len()), (1, 1));
    assert_eq!(table.grid(), [7, 6]);
    assert_eq!(table.tiles[0].art, 339);
    let cell = |c, r| table.subtile([c, r]);
    let c00 = cell(0, 0);
    assert_eq!(
        (c00.terrain, c00.fill, c00.frequency, c00.table),
        (0, Fill::None, [22, 4, 0], Some(0))
    );
    let c10 = cell(1, 0);
    assert_eq!(
        (c10.terrain, c10.fill, c10.frequency, c10.table),
        (1, Fill::W, [4, 4, 4], Some(0))
    );
    assert_eq!(table.cell_of([75, 10]), Some([1, 0]));
    let c65 = cell(6, 5);
    assert_eq!(
        (c65.terrain, c65.fill, c65.frequency, c65.table),
        (0, Fill::None, [0, 22, 22], None)
    );
    let c23 = cell(2, 3);
    assert_eq!(
        (c23.terrain, c23.fill, c23.frequency, c23.table),
        (0, Fill::None, [0, 0, 0], None)
    );
    assert_eq!(table.random_maps[0], [0]);
    let encounters = &table.tables[&0];
    assert_eq!(encounters.maps, [0]);
    let numbers: Vec<u32> = encounters.entries.iter().map(|e| e.number).collect();
    assert_eq!(numbers, [0, 1, 2, 3], "enc_04 (3 > 1) is dropped");
    let e0 = &encounters.entries[0];
    assert_eq!((e0.chance, e0.counter), (25, -1));
    assert_eq!(e0.expression.groups.len(), 1);
    assert_eq!(
        (e0.expression.groups[0].min, e0.expression.groups[0].max),
        (2, 4)
    );
    assert_eq!(e0.expression.groups[0].name, "test_geckos");
    let e1 = &encounters.entries[1];
    assert_eq!(e1.counter, 1);
    assert_eq!(e1.expression.relations, [Relation::Fighting]);
    assert_eq!(
        (e1.expression.groups[1].min, e1.expression.groups[1].max),
        (1, 2)
    );
    let e2 = &encounters.entries[2];
    assert_eq!(e2.expression.relations, [Relation::Ambush]);
    assert_eq!(e2.expression.groups.len(), 1);
    assert_ne!(e2.condition, Condition::Always);
    let e3 = &encounters.entries[3];
    assert!(e3.special);
    assert_eq!((e3.counter, e3.map), (1, Some(2)));
    assert!(e3.expression.groups.is_empty());
    let geckos = table.group("TEST_Geckos").unwrap();
    assert_eq!((geckos.spacing, geckos.distance), (2, Distance::Hexes(6)));
    assert_eq!(geckos.types[1].items.len(), 2);
    let item0 = geckos.types[1].items[0];
    assert_eq!(
        (item0.min, item0.max, item0.pid, item0.wielded),
        (1, 3, 41, false)
    );
    let item1 = geckos.types[1].items[1];
    assert_eq!(
        (item1.min, item1.max, item1.pid, item1.wielded),
        (1, 1, 4, true)
    );
    let hunters = table.group("test_hunters").unwrap();
    assert_eq!(hunters.types[0].script, 12);
    assert!(hunters.types[1].dead);
    // Conditions: no short-circuit, so Rand draws even when the left side
    // is false.
    let condition = worldmap::condition("Global(3) > 1 And Rand(50%)").unwrap();
    let mut context = Context {
        globals: vec![0; 4],
        level: 1,
        draws: vec![10],
        drawn: 0,
    };
    assert!(!condition.holds(&mut context));
    assert_eq!(context.drawn, 1);
    context.globals[3] = 2;
    context.draws = vec![51, 50];
    context.drawn = 0;
    assert!(!condition.holds(&mut context));
    assert!(condition.holds(&mut context));
    assert_eq!(worldmap::condition("Global(3) >"), None);
    let level = worldmap::condition("Player(Level) >= 5 Or Days_Played = 0").unwrap();
    assert!(level.holds(&mut context));
    // A missing num_horizontal_tiles refuses.
    assert!(WorldmapTable::parse(b"[Tile 0]\n", &maps(), &mut Diagnostics::default()).is_err());
}

/// SPEC-018 Revision 2 (R7-R11): the encounter grammar the retail file
/// uses, on a synthetic table.
#[test]
fn spec018_revision2_encounter_grammar() {
    let text = "[Data]\nterrain_types=Desert:1\n[Tile Data]\nnum_horizontal_tiles=1\n\
        [Tile 0]\nart_idx=1\n\
        [Encounter Table 0]\nlookup_name=Test_Desert\n\
        enc_00=Chance:5%, If(Global(3) > 1) And If (Rand(50%)), Enc:(2) TEST_Geckos\n\
        enc_01=Chance:5%, Special, Enc:TEST_Hunters, If(time_of_day >= 600) Or if(GLOBAL(1)==2)\n\
        enc_02=Chance:5%, Enc:(1) TEST_Geckos FIGHTING Player\n\
        enc_03=Chance:5%, Enc:TEST_Geckos AND (1-2) TEST_Hunters AMBUSH Player\n\
        enc_04=Chance:5%, Enc:(1) TEST_Geckos AND Player\n\
        enc_05=Chance:5%, If(Global(3) > 1) And, Enc:(1) TEST_Geckos\n\
        [Encounter: TEST_Geckos]\n\
        position=wedge, spacing:2, distance:Player(Perception)\nteam_num=4\n\
        type_00=pid:16777217, Distance:3, Item:(1-2)7{Wielded}, Item:8 {wielded}\n\
        type_01=pid:5, Ratio:10%, Dead, Script:4, Item:9\n\
        type_02=pid:33554440\n\
        type_03=pid:50331649\n\
        type_04=pid:16777218, Distance:31, If (Player(Level) >= 2) And If(Days_Played = 0)\n\
        [Encounter: TEST_Hunters]\n\
        position=surrounding, distance:12\n\
        type_00=pid:16777219\n";
    let mut notes = Diagnostics::default();
    let table = WorldmapTable::parse(text.as_bytes(), &maps(), &mut notes).unwrap();
    let encounters = &table.tables[&0];
    let numbers: Vec<u32> = encounters.entries.iter().map(|e| e.number).collect();
    // `AND Player` stays invalid, as does a field with a dangling join.
    assert_eq!(numbers, [0, 1, 2, 3, 5]);
    // R7: two `If(...)` read as one condition, joined at their boundary.
    let e0 = &encounters.entries[0];
    let mut context = Context {
        globals: vec![0; 4],
        level: 1,
        draws: vec![50, 51, 10],
        drawn: 0,
    };
    context.globals[3] = 2;
    assert!(e0.condition.holds(&mut context));
    assert!(!e0.condition.holds(&mut context));
    // No short-circuit: the Rand draws with the left side false.
    context.globals[3] = 0;
    assert!(!e0.condition.holds(&mut context));
    assert_eq!(context.drawn, 3);
    // R7: spaces after `If`, case folding, and `Or` between fields.
    let e1 = &encounters.entries[1];
    assert!(e1.special);
    assert!(e1.condition.holds(&mut context), "time 721 >= 600");
    let either = worldmap::condition_field("If(time_of_day >= 800) Or if(GLOBAL(1)==2)").unwrap();
    assert!(either.holds(&mut Context {
        globals: vec![0, 2],
        level: 1,
        draws: Vec::new(),
        drawn: 0,
    }));
    // An invalid field never holds, with a diagnostic.
    let e5 = &encounters.entries[4];
    assert_eq!((e5.number, &e5.condition), (5, &Condition::Never));
    assert!(
        notes
            .0
            .iter()
            .any(|note| note.message.contains("enc_05: condition"))
    );
    // R8: a group without a count is `(1)`.
    let hunters = &e1.expression.groups[0];
    assert_eq!(
        (hunters.min, hunters.max, hunters.name.as_str()),
        (1, 1, "test_hunters")
    );
    // R8: `FIGHTING Player`, and each relation records a Player target.
    let e2 = &encounters.entries[2];
    assert_eq!(e2.expression.relations, [Relation::Fighting]);
    assert_eq!(e2.expression.player_targets, [true]);
    assert_eq!(e2.expression.groups.len(), 1);
    let e3 = &encounters.entries[3];
    assert_eq!(e3.expression.relations, [Relation::And, Relation::Ambush]);
    assert_eq!(e3.expression.player_targets, [false, true]);
    assert_eq!(
        e3.expression
            .groups
            .iter()
            .map(|group| (group.min, group.max))
            .collect::<Vec<_>>(),
        [(1, 1), (1, 2)]
    );
    // R10: `Player(Perception)` in `position`, clamped when spawning;
    // `team_num` stored with no diagnostic.
    let geckos = table.group("test_geckos").unwrap();
    assert_eq!(geckos.distance, Distance::PlayerPerception);
    assert_eq!(
        [3, 40, -1].map(|perception| geckos.distance.hexes(perception)),
        [3, 30, 0]
    );
    assert_eq!(geckos.team, Some(4));
    assert!(!notes.0.iter().any(|note| note.message.contains("team_num")));
    let other = table.group("test_hunters").unwrap();
    assert_eq!((other.distance, other.team), (Distance::Hexes(12), None));
    // R10: a type's own Distance; an invalid one is ignored with a
    // diagnostic. R7 applies to types' conditions too.
    let numbers: Vec<u32> = geckos.types.iter().map(|t| t.number).collect();
    assert_eq!(numbers, [0, 1, 2, 4], "a misc PID (category 3) is dropped");
    let [critter, item, scenery, last] = &geckos.types[..] else {
        panic!()
    };
    assert_eq!(critter.distance, Some(3));
    assert_eq!(last.distance, None);
    assert!(
        notes
            .0
            .iter()
            .any(|note| note.message.contains("type_04: distance"))
    );
    assert!(last.condition.holds(&mut Context {
        globals: Vec::new(),
        level: 2,
        draws: Vec::new(),
        drawn: 0,
    }));
    // R11: `{Wielded}` is `(wielded)`, with or without a space.
    assert_eq!(
        critter
            .items
            .iter()
            .map(|item| (item.min, item.max, item.pid, item.wielded))
            .collect::<Vec<_>>(),
        [(1, 2, 7, true), (1, 1, 8, true)]
    );
    // R9: item and scenery types are kept; their Dead, Script and Item
    // fields are ignored with a diagnostic.
    assert!(critter.is_critter() && !item.is_critter() && !scenery.is_critter());
    assert_eq!((item.pid, item.ratio), (5, Some(10)));
    assert!(!item.dead && item.script == -1 && item.items.is_empty());
    assert_eq!(scenery.pid, 0x0200_0008);
    let ignored: Vec<_> = notes
        .0
        .iter()
        .filter(|note| note.message.contains("is not a critter's"))
        .collect();
    assert_eq!(ignored.len(), 1, "{ignored:?}");
    assert!(ignored[0].message.starts_with("type_01"));
}

/// SPEC-018 Revision 2 (R12, R13): entrances with no marker, a non-critter
/// party record and `level_pids=-1`.
#[test]
fn spec018_revision2_entrances_and_party_records() {
    let maps = maps();
    let world = worldmap().world_size();
    let mut notes = Diagnostics::default();
    let city = CityTable::parse(
        b"[Area 0]\nworld_pos=1,1\nentrance_0=On,-1,-1,Temple,-1,-1,0\n\
          entrance_1=On,5,-1,Temple,-1,-1,0\nentrance_2=Off,-1,7,Temple,-1,-1,0\n\
          entrance_3=On,1023,0,Temple,-1,-1,0\n",
        &maps,
        world,
        &mut notes,
    )
    .unwrap();
    let area = city.get(0).unwrap();
    let markers: Vec<_> = area
        .entrances
        .iter()
        .map(|entrance| entrance.map(|entrance| entrance.marker))
        .collect();
    // Only one of the two at -1 is invalid, and drops the entrance.
    assert_eq!(markers, [Some(None), None, None, Some(Some([1023, 0]))]);
    assert_eq!(area.entrances[0].unwrap().map, 2);
    // The unmarked entrance still serves scripts (metarule3 104).
    assert_eq!(city.entrances_by_map(2, 0), [(0, 0), (0, 3)]);
    let mut notes = Diagnostics::default();
    let party = PartyTable::parse(
        b"[Party Member 0]\nparty_member_pid=16777300\nlevel_pids=-1\n\
          [Party Member 1]\nparty_member_pid=33554500\nlevel_pids = -1\n\
          [Party Member 2]\nparty_member_pid=10\n\
          [Party Member 3]\nparty_member_pid=50331649\n\
          [Party Member 4]\nparty_member_pid=16777301\nlevel_pids=-1, 16777302\n",
        &mut notes,
    )
    .unwrap();
    assert!(party.get(0).unwrap().level_pids.is_empty());
    assert_eq!(party.get(1).unwrap().pid, 0x0200_0044);
    assert_eq!(party.get(2).unwrap().pid, 10);
    assert!(party.get(3).is_none(), "a misc PID is not a member's");
    // `-1` is the empty list only alone; in a longer list it is dropped.
    assert_eq!(party.get(4).unwrap().level_pids, [16777302]);
    let messages: Vec<&str> = notes.0.iter().map(|note| note.message.as_str()).collect();
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages[0].contains("[Party Member 3]"));
    assert!(messages[1].contains("level_pids: \"-1\" dropped"));
}

#[test]
fn spec018_party_table() {
    let table = PartyTable::parse(fixture::PARTY.as_bytes(), &mut Diagnostics::default()).unwrap();
    assert_eq!(table.count(), 1);
    let member = table.get(0).unwrap();
    assert_eq!(member.pid, 16777300);
    assert_eq!(member.allowed(ControlField::AreaAttackMode), [0, 2]);
    assert_eq!(member.allowed(ControlField::AttackWho), [4]);
    for field in [
        ControlField::Disposition,
        ControlField::RunAwayMode,
        ControlField::BestWeapon,
        ControlField::Distance,
        ControlField::ChemUse,
    ] {
        assert!(member.allowed(field).is_empty());
    }
    assert_eq!((member.level_minimum, member.level_up_every), (2, 3));
    assert_eq!(member.level_pids, [16777301, 16777302]);
    for pid in [16777300, 16777301, 16777302] {
        assert_eq!(table.record_of(pid).unwrap().number, 0);
    }
    assert!(table.get(1).is_none());
    // The neutral record.
    let neutral: Vec<&str> = ControlField::ALL
        .iter()
        .map(|field| field.words()[field.neutral() as usize])
        .collect();
    assert_eq!(
        neutral,
        [
            "custom",
            "always",
            "none",
            "no_pref",
            "on_your_own",
            "whomever",
            "anytime"
        ]
    );
}

#[test]
fn spec018_line_tables() {
    let mut notes = Diagnostics::default();
    let quests = lines::quests(fixture::QUESTS.as_bytes(), &mut notes).unwrap();
    assert_eq!(quests.len(), 3);
    assert_eq!(notes.0.len(), 1, "the fourth line is dropped");
    let globals = [(7, 2), (8, 1), (9, 5)];
    let status: Vec<QuestStatus> = quests
        .iter()
        .zip(globals)
        .map(|(quest, (global, value))| {
            assert_eq!(quest.global, global);
            quest.status(value)
        })
        .collect();
    assert_eq!(
        status,
        [
            QuestStatus::Active,
            QuestStatus::Hidden,
            QuestStatus::Completed
        ]
    );
    assert_eq!(
        lines::quest_groups(&quests),
        [(1500, vec![0, 1]), (1501, vec![2])]
    );
    let disks = lines::holodisks(fixture::HOLODISKS.as_bytes(), &mut notes).unwrap();
    assert_eq!(disks.len(), 2);
    assert_eq!(
        (disks[0].global, disks[0].title, disks[0].text),
        (12, 400, 1000)
    );
    let genrep = lines::genrep(fixture::GENREP.as_bytes(), &mut notes).unwrap();
    assert_eq!(lines::reputation_title(&genrep, -60), None);
    assert_eq!(lines::reputation_title(&genrep, -50), Some(1001));
    assert_eq!(lines::reputation_title(&genrep, -1), Some(1001));
    assert_eq!(lines::reputation_title(&genrep, 0), Some(1002));
    assert_eq!(lines::reputation_title(&genrep, 99), Some(1002));
    assert_eq!(lines::reputation_title(&genrep, 100), Some(1003));
    let slides = lines::endgame(fixture::ENDGAME.as_bytes(), &mut notes).unwrap();
    assert_eq!(slides.len(), 2);
    assert_eq!(
        (
            slides[0].global,
            slides[0].value,
            &slides[0].art,
            slides[0].direction
        ),
        (5, 1, &SlideArt::Row(327), 0)
    );
    assert_eq!(
        slides[0].narration_path(),
        "sound/speech/narrator/nar_a1.acm"
    );
    assert_eq!(
        slides[1].art_path().as_deref(),
        Some("art/intrface/endslid2.frm")
    );
    assert_eq!(
        slides[1].palette_path().as_deref(),
        Some("art/intrface/endslid2.pal")
    );
    assert_eq!(slides[1].direction, -1);
    assert_eq!(lines::row_palette("endslid1.frm"), "endslid1.pal");
}

#[test]
fn spec018_elevator_table() {
    let maps = maps();
    let mut notes = Diagnostics::default();
    let table =
        elevators::ElevatorTable::parse(fixture::ELEVATORS.as_bytes(), &maps, &mut notes).unwrap();
    let elevator = table.get(0).unwrap();
    assert_eq!(elevator.stops.len(), 3);
    let keys: Vec<u8> = elevator.stops.iter().map(|stop| stop.key).collect();
    assert_eq!(keys, b"12G");
    assert!(elevator.stops[2].matches(b'g'));
    assert_eq!(elevator.doors, [0x02000099]);
    assert_eq!(elevator.start_level(2, 1), 1);
    assert_eq!(elevator.start_level(5, 0), 0);
    assert!(table.get(1).is_none(), "map 7 has no record");
    assert!(table.get(5).is_none());
    let empty = elevators::ElevatorTable::parse(b"", &maps, &mut notes).unwrap();
    assert!(empty.get(0).is_none());
}

#[test]
fn spec018_tables_load_through_a_reader() {
    let mut read = fixture::reader(fixture::tables());
    let tables = Fallout2Tables::load(&mut read).unwrap();
    assert_eq!(tables.maps.count(), 3);
    assert_eq!(tables.city.count(), 1);
    assert_eq!(tables.quests.len(), 3);
    assert!(tables.elevators.elevators.is_empty());
    assert!(!tables.diagnostics.is_empty());
    // A missing required table refuses, naming it.
    let mut files = fixture::tables();
    files.retain(|(path, _)| *path != "data/party.txt");
    let error = Fallout2Tables::load(&mut fixture::reader(files)).unwrap_err();
    assert_eq!(error.file, "data/party.txt");
}

// §8.8-§8.11 ------------------------------------------------------------

/// §8.8: SPEC-008 §4.3's header with the Fallout 2 ranges.
fn fallout2_header() -> Vec<u8> {
    let mut bytes = fixture_header();
    for (offset, text) in [
        (0x0018, "00 01 00 02 52"),
        (0x0065, "00 07 00 19 08 C1 00 04 09 98 00 00 00 02"),
        (0x0073, "41 52 54 45 4D 50 4C 45 2E 73 61 76 00 00 00 00"),
        (0x74E3, "56 41 55 4C 54 45 4E 47 20 46 32 53 41 56 45 31"),
        (0x74F3, "00 00 02 BC"),
    ] {
        let replacement = hex(text);
        bytes[offset..offset + replacement.len()].copy_from_slice(&replacement);
    }
    bytes
}

#[test]
fn spec018_save_header() {
    let bytes = fallout2_header();
    let header = SaveHeader::parse_for(&bytes, Fallout2).unwrap();
    assert_eq!((header.version, header.release), (FALLOUT2_VERSION, b'R'));
    assert_eq!(header.game_date, [7, 25, 2241]);
    assert_eq!(header.game_ticks, 264600);
    assert_eq!((header.elevation, header.map), (0, 2));
    assert_eq!(header.map_file_text(), b"ARTEMPLE.sav");
    assert_eq!((header.campaign_count(), header.gated_mark()), (700, 0));
    header.check_fallout2(700, true).unwrap();
    header.check_fallout2(700, false).unwrap();
    assert_eq!(header.write().unwrap(), bytes);
    // A Fallout 1 header and a Fallout 2 one never cross.
    assert!(SaveHeader::parse(&bytes).is_err());
    assert!(SaveHeader::parse_for(&fixture_header(), Fallout2).is_err());
    let mut minor = bytes.clone();
    minor[0x1B] = 1;
    assert!(SaveHeader::parse_for(&minor, Fallout2).is_err());
    for at in 0x74E3..0x74F3 {
        let mut tag = bytes.clone();
        tag[at] ^= 0x01;
        assert!(SaveHeader::parse_for(&tag, Fallout2).is_err(), "{at:#x}");
    }
    let mut g = bytes.clone();
    g[0x74F6] = 0xBD;
    assert!(
        SaveHeader::parse_for(&g, Fallout2)
            .unwrap()
            .check_fallout2(700, true)
            .is_err()
    );
    let mut two = bytes.clone();
    two[0x74F7] = 2;
    assert!(
        SaveHeader::parse_for(&two, Fallout2)
            .unwrap()
            .check_fallout2(700, true)
            .is_err()
    );
    let mut one = bytes.clone();
    one[0x74F7] = 1;
    let gated = SaveHeader::parse_for(&one, Fallout2).unwrap();
    assert!(gated.check_fallout2(700, true).is_ok());
    assert!(gated.check_fallout2(700, false).is_err());
    assert!(SaveHeader::parse_for(&bytes[..HEADER_LEN - 1], Fallout2).is_err());
    // The writer sets the same reserved bytes.
    let mut written = SaveHeader::parse(&fixture_header()).unwrap();
    written.set_fallout2(700, false);
    assert_eq!(written.version, [1, 2]);
    assert_eq!(&written.write().unwrap()[0x74E3..], &bytes[0x74E3..]);
}

/// §8.9's installed shape, and §8.10's records: 0 (two level PIDs) and,
/// with `two_records`, 1 (two level PIDs).
fn shape(two_records: bool) -> Fallout2Shape {
    Fallout2Shape {
        grid: [7, 6],
        entrances: vec![3],
        areas_present: vec![true],
        maps: 3,
        maps_present: vec![true; 3],
        party_levels: if two_records {
            vec![Some(2), Some(2)]
        } else {
            vec![Some(2)]
        },
    }
}

fn world_map_state() -> WorldMapSave2 {
    let mut discovery = vec![0; 42];
    discovery[0] = 2;
    for at in [1, 7, 8] {
        discovery[at] = 1;
    }
    WorldMapSave2 {
        position: [175, 125],
        area: 0,
        entrance: 0,
        has_car: 0,
        car_area: -1,
        car_map: -1,
        car_fuel: 0,
        grid: [7, 6],
        discovery,
        areas: vec![AreaSave {
            known: 2,
            locked: 0,
            position: [175, 125],
            entrances: vec![1, 0, 0],
        }],
        map_flags: vec![0, 0, 3],
        counters: vec![[0, 1, 0], [0, 3, 1]],
    }
}

const WORLD_MAP_COMPONENT: &str = "
    00 00 00 83 00 00 00 AF 00 00 00 7D 00 00 00 00
    00 00 00 00 00 00 00 00 FF FF FF FF FF FF FF FF
    00 00 00 00 00 07 00 06 02 01 00 00 00 00 00 01
    01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    00 00 00 00 00 01 02 00 00 00 00 AF 00 00 00 7D
    03 01 00 00 00 00 00 03 00 00 03 00 00 00 02 00
    00 00 00 00 00 00 01 00 00 00 00 00 00 00 00 00
    00 00 03 00 00 00 01";

#[test]
fn spec018_world_map_component() {
    let expected = hex(WORLD_MAP_COMPONENT);
    assert_eq!(expected.len(), 135);
    let mut bytes = Vec::new();
    world_map_state().write(&mut bytes);
    assert_eq!(bytes, expected);
    let installed = shape(false);
    let read =
        |bytes: &[u8]| WorldMapSave2::read(&mut Reader::unbounded(bytes, "SAVE"), &installed);
    assert_eq!(read(&expected).unwrap(), world_map_state());
    let mutated = |at: usize, value: u8| {
        let mut bytes = expected.clone();
        bytes[at] = value;
        bytes
    };
    for (what, bytes) in [
        ("L 132", mutated(3, 0x84)),
        ("R 5", mutated(0x27, 5)),
        ("E 2", mutated(0x60, 2)),
        ("M 4", mutated(0x67, 4)),
        ("known 3", mutated(0x56, 3)),
        ("has_car 2", mutated(0x17, 2)),
    ] {
        assert!(read(&bytes).is_err(), "{what}");
    }
    let mut fuel = expected.clone();
    fuel[0x20..0x24].copy_from_slice(&80001i32.to_be_bytes());
    assert!(read(&fuel).is_err(), "car_fuel 80001");
    fuel[0x20..0x24].copy_from_slice(&80000i32.to_be_bytes());
    assert!(read(&fuel).is_ok(), "car_fuel 80000");
}

#[test]
fn spec018_party_record() {
    let defensive = MemberRecord {
        record: 1,
        stage: 2,
        fields: [2, 2, 3, 3, 0, 0, 1],
    };
    let party = PartySave2 {
        count: 2,
        next_item_id: 4000,
        members: vec![77],
        records: vec![defensive],
        retained: Vec::new(),
    };
    let mut bytes = Vec::new();
    party.write(&mut bytes).unwrap();
    let expected = hex("00 00 00 01 00 00 00 02 00 00 00 02 00 00 00 02 00 00 00 03
         00 00 00 03 00 00 00 00 00 00 00 00 00 00 00 01");
    assert_eq!(&bytes[12..48], &expected[..]);
    assert_eq!(&bytes[48..], &[0, 0, 0, 0]);
    let read = |bytes: &[u8], two: bool| {
        PartySave2::read(&mut Reader::unbounded(bytes, "SAVE"), &shape(two))
    };
    assert!(read(&bytes, false).is_err(), "only record 0 is installed");
    assert_eq!(read(&bytes, true).unwrap(), party);
    let mut eight = bytes.clone();
    eight[12 + 4 * 5..12 + 4 * 6].copy_from_slice(&8i32.to_be_bytes());
    assert_eq!(read(&eight, true).unwrap().records[0].fields[3], 8);
    let mut stage = bytes.clone();
    stage[12 + 7] = 3;
    assert!(read(&stage, true).is_err());
    let mut big = bytes.clone();
    big[3] = 33;
    assert!(read(&big, true).is_err());
}

#[test]
fn spec018_fallout2_save_round_trips() {
    let mut save = synthetic_save(5);
    save.header.set_fallout2(5, true);
    save.party_count = 2;
    save.party = vec![11];
    save.fallout2 = Some(Box::new(Fallout2Payload {
        kills: [7, 8, 9, 10],
        world_map: world_map_state(),
        movies: [1; 17],
        records: vec![MemberRecord::NONE],
        retained: vec![(
            12,
            MemberRecord {
                record: 0,
                stage: 1,
                fields: [0, 0, 0, 0, 3, 3, 4],
            },
        )],
    }));
    let bytes = save.write().unwrap();
    let installed = shape(false);
    let parsed = SaveGame::parse_fallout2(&bytes, 5, &lookup, &installed).unwrap();
    assert_eq!(parsed.write().unwrap(), bytes);
    assert_eq!(parsed.fallout2, save.fallout2);
    assert_eq!(parsed.kills, save.kills);
    assert!(SaveGame::parse(&bytes, 5, &lookup).is_err());
    // A slot from other installed data is refused.
    let mut other = installed.clone();
    other.maps = 4;
    other.maps_present.push(true);
    assert!(SaveGame::parse_fallout2(&bytes, 5, &lookup, &other).is_err());
}

#[test]
fn spec018_automap_index() {
    let db = AutomapDb::fallout2(|map| map == 0 || map >= 3);
    assert_eq!((db.bytes().len(), FALLOUT2_EMPTY_LEN), (12005, 12005));
    assert_eq!(&db.bytes()[..5], &[1, 0, 0, 0x2E, 0xE5]);
    for elevation in 0..3 {
        assert_eq!(db.offset(0, elevation), Some(-1));
        assert_eq!(db.offset(1, elevation), Some(0));
        assert_eq!(db.offset(2, elevation), Some(0));
        assert_eq!(db.offset(999, elevation), Some(-1));
    }
    // The word for map 2, elevation 2 is at offset 37.
    assert_eq!(&db.bytes()[37..41], &[0, 0, 0, 0]);
    let mut stored = db.clone();
    assert!(stored.store(2, 2, &[0; 10000]).unwrap());
    let reparsed = AutomapDb::parse_fallout2(stored.bytes().to_vec()).unwrap();
    assert_eq!(reparsed.raster(2, 2).unwrap(), Some(vec![0; 10000]));
    assert!(AutomapDb::parse_fallout2(db.bytes()[..12004].to_vec()).is_err());
    let mut inside = db.bytes().to_vec();
    inside[37..41].copy_from_slice(&100i32.to_be_bytes());
    assert!(AutomapDb::parse_fallout2(inside).is_err());
    let mut past = db.bytes().to_vec();
    past[37..41].copy_from_slice(&20000i32.to_be_bytes());
    assert!(AutomapDb::parse_fallout2(past).is_err());
    assert_eq!(AutomapDb::default().bytes().len(), 797);
    assert_eq!(AutomapDb::default().map_count(), 66);
}
