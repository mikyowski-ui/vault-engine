//! SPEC-018 §8 vectors for the Fallout 2 world rules: the new-game world
//! state (§8.4, §6.5), encounter selection (§8.5, §5.6), travel and fill
//! (§8.5, §5.4-§5.5), the calendar (§8.8, §6.1), the world-map component
//! from live state (§8.9), Pip-Boy lists (§8.7), party records (§8.10),
//! elevators (§8.12) and the script gate (§6.7).
use crate::{
    CharacterSheet, ScriptEvent, WorldEffect, WorldHost, WorldState,
    fallout2::{self, AreaEntry, Conditions, Fallout2Tables, Fallout2World, Selected, Step},
    test_support::{TestAssets, program},
    world::ScriptRequest,
};
use std::sync::Arc;
use vault_fixtures::fallout2 as fixture;
use vault_formats::fallout2::{lines::QuestStatus, save::MemberRecord};

fn tables_with(extra: Vec<(&'static str, &'static str)>) -> Arc<Fallout2Tables> {
    let mut files = fixture::tables();
    files.extend(extra);
    Arc::new(Fallout2Tables::load(&mut fixture::reader(files)).unwrap())
}
fn tables() -> Arc<Fallout2Tables> {
    tables_with(Vec::new())
}
/// The new game of §8.4: the start map is map 2 (`artemple`).
fn new_game() -> Fallout2World {
    let tables = tables();
    let start = tables.maps.match_name(b"artemple.map");
    assert_eq!(start, 2);
    Fallout2World::new_game(tables, start)
}

#[test]
fn spec018_new_game_world_state() {
    let world = new_game();
    // §8.4: the start area is area 0; its current entrance is slot 2.
    assert_eq!((world.area, world.entrance), (0, 2));
    assert_eq!(world.position, [175, 125]);
    assert_eq!(world.areas.len(), 1);
    let area = &world.areas[0];
    assert_eq!(
        (area.known, area.locked, area.position),
        (2, false, [175, 125])
    );
    assert_eq!(area.entrances, [true, false, false]);
    // Discovery: the start area's cell is 2, everything else 0.
    assert_eq!(world.grid(), [7, 6]);
    let visited: Vec<usize> = (0..42).filter(|at| world.discovery[*at] != 0).collect();
    assert_eq!(visited, [2 * 7 + 3]);
    assert_eq!(world.discovery([3, 2]), 2);
    assert_eq!(world.map_flags, [0, 0, 0]);
    assert_eq!(world.counter(0, 1), 1);
    assert_eq!(world.counter(0, 3), 1);
    assert_eq!(world.counter(0, 0), -1);
    assert_eq!(world.car, fallout2::Car::default());
    assert_eq!(world.movies, [0; 17]);
    assert!(world.control.is_empty() && world.retained.is_empty());
    // Without a matching record the start area is area 0.
    let tables = tables();
    assert_eq!(Fallout2World::start_area(&tables, -1), 0);
    let unmatched = Fallout2World::new_game(tables, -1);
    assert_eq!((unmatched.area, unmatched.entrance), (0, -1));
    // §5.2 a map load sets its flags and moves the party to its area.
    let mut world = new_game();
    world.position = [10, 10];
    world.map_loaded(1);
    assert_eq!(world.map_flags, [0, 3, 0]);
    assert_eq!(world.position, [175, 125]);
    world.position = [10, 10];
    world.map_loaded(0);
    assert_eq!(world.position, [10, 10], "map 0 has area -1");
    assert_eq!(world.location_name(2), "Test Village");
    assert_eq!(world.location_name(0), "Desert Encounter 1");
    assert!(!world.can_rest(2, 0) && world.can_rest(2, 2) && world.can_rest(1, 0));
}

#[test]
fn spec018_world_map_component_from_live_state() {
    let mut world = new_game();
    world.entrance = 0;
    world.discovery.fill(0);
    world.discovery[0] = 2;
    for at in [1, 7, 8] {
        world.discovery[at] = 1;
    }
    world.map_flags[2] = 3;
    world.counters.insert((0, 1), 0);
    let mut bytes = Vec::new();
    world.save().write(&mut bytes);
    let expected = vault_fixtures::formats::hex(
        "00 00 00 83 00 00 00 AF 00 00 00 7D 00 00 00 00
         00 00 00 00 00 00 00 00 FF FF FF FF FF FF FF FF
         00 00 00 00 00 07 00 06 02 01 00 00 00 00 00 01
         01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
         00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
         00 00 00 00 00 01 02 00 00 00 00 AF 00 00 00 7D
         03 01 00 00 00 00 00 03 00 00 03 00 00 00 02 00
         00 00 00 00 00 00 01 00 00 00 00 00 00 00 00 00
         00 00 03 00 00 00 01",
    );
    assert_eq!(bytes, expected);
    // Restored, it gives the same component; a counter record of no
    // limited entry is ignored, and an outside position is repaired.
    let mut saved = world.save();
    saved.counters.push([0, 0, 5]);
    saved.position = [400, 10];
    saved.areas[0].position = [-1, 0];
    let mut notes = Vec::new();
    let restored = Fallout2World::restore(world.tables.clone(), &saved, [0; 17], &mut notes);
    assert_eq!(notes.len(), 3, "{notes:?}");
    assert_eq!(restored.position, [349, 10]);
    assert_eq!(restored.areas[0].position, [175, 125]);
    assert_eq!(restored.counter(0, 1), 0);
    assert_eq!(restored.counter(0, 0), -1);
    assert_eq!(restored.discovery, world.discovery);
}

/// The §8.5 selection state: the morning at cell (0, 0), Luck 5, no
/// Explorer, global 3 = 0, level 1.
fn select(world: &mut Fallout2World, draws: &[i32]) -> (Option<Selected>, Vec<i32>) {
    let mut rng = crate::ParkMiller::seeded(1).unwrap();
    rng.script(draws);
    let globals = [0; 8];
    let mut conditions = Conditions {
        globals: &globals,
        level: 1,
        ticks: fallout2::START_TICKS,
        rng: &mut rng,
        failure: None,
    };
    let mut notes = Vec::new();
    let selected = world
        .check_encounter(&mut conditions, 5, 0, &mut notes)
        .unwrap();
    (selected, rng.unused_script())
}

#[test]
fn spec018_encounter_selection() {
    let mut world = new_game();
    world.position = [10, 10];
    assert_eq!(fallout2::period(fallout2::START_TICKS), 0);
    // 22 <= 22 is an encounter; enc_03's special gate draws 3, 3, 3 (9 -
    // 5 + 5 = 9, below 18); random(1, 40) = 30 picks enc_01; one map in
    // the table's list, so no map draw.
    let (selected, unused) = select(&mut world, &[22, 3, 3, 3, 30]);
    assert_eq!(
        selected,
        Some(Selected {
            table: 0,
            entry: 1,
            map: 0,
            special: false
        })
    );
    assert!(unused.is_empty());
    assert_eq!(world.counter(0, 1), 0);
    // Repeated: enc_01's counter is 0, so the pick draws random(1, 25).
    let (selected, unused) = select(&mut world, &[22, 3, 3, 3, 25, 99]);
    assert_eq!(selected.map(|s| s.entry), Some(0));
    assert_eq!(unused, [99]);
    // 23 gives no encounter, and nothing else is drawn.
    let (selected, unused) = select(&mut world, &[23, 99]);
    assert_eq!((selected, unused), (None, vec![99]));
    // A passed special gate keeps enc_03, which forces map 2.
    let (selected, _) = select(&mut world, &[22, 6, 6, 6, 30]);
    assert_eq!(
        selected,
        Some(Selected {
            table: 0,
            entry: 3,
            map: 2,
            special: true
        })
    );
    assert_eq!(world.counter(0, 3), 0);
    // Inside an area (area 0's Large circle) the draw is still made, and
    // there is no encounter.
    world.position = [175, 125];
    let (selected, unused) = select(&mut world, &[1, 99]);
    assert_eq!((selected, unused), (None, vec![99]));
    // A cell without a table makes the draw too.
    world.position = [120, 160];
    let (selected, unused) = select(&mut world, &[1, 99]);
    assert_eq!((selected, unused), (None, vec![99]));
}

#[test]
fn spec018_travel_schedule_reveal_and_fill() {
    let mut world = new_game();
    // Cell (1, 0) is Mountain, difficulty 2: steps on iterations 1, 3, 5.
    world.position = [60, 10];
    world.travel_to([90, 10]);
    let mut moved = Vec::new();
    for iteration in 1..=6 {
        let before = world.position;
        assert_eq!(world.step(), Step::Moving);
        if world.position != before {
            moved.push(iteration);
        }
    }
    assert_eq!(moved, [1, 3, 5]);
    // Fill: the reveal sets cell (1, 0) to 2 and its Fill_W cell (0, 0).
    world.discovery.fill(0);
    world.reveal(false);
    assert_eq!(world.discovery([1, 0]), 2);
    assert_eq!(world.discovery([0, 0]), 2);
    assert_eq!(world.discovery([2, 0]), 1);
    assert_eq!(world.discovery([2, 1]), 1);
    assert_eq!(world.discovery([3, 0]), 0);
    // A step out of the world is blocked and stops movement.
    world.position = [0, 10];
    world.travel_to([-5, 10]);
    world.terrain_counter = 0;
    assert_eq!(world.step(), Step::Blocked);
    assert_eq!(world.position, [0, 10]);
    assert!(world.travel.is_none());
}

#[test]
fn spec018_walk_masks_block_travel() {
    const MASKED: &str = "[Data]\nterrain_types=Desert:1\n[Tile Data]\nnum_horizontal_tiles=1\n\
        [Tile 0]\nwalk_mask_name=testmask\n";
    let mut files = fixture::tables();
    files.retain(|(path, _)| *path != "data/worldmap.txt");
    files.push(("data/worldmap.txt", MASKED));
    let mut mask = vec![0u8; 13200];
    // Tile-local pixel (10, 20): row 20, byte 1, bit 0x20.
    mask[20 * 44 + 1] = 0x20;
    let mut read = {
        let mut inner = fixture::reader(files);
        move |path: &str| {
            if path == "data/testmask.msk" {
                Ok(Some(mask.clone()))
            } else {
                inner(path)
            }
        }
    };
    let tables = Arc::new(Fallout2Tables::load(&mut read).unwrap());
    let mut world = Fallout2World::new_game(tables, -1);
    assert!(world.blocks([10, 20]));
    assert!(!world.blocks([11, 20]));
    world.position = [9, 20];
    world.travel_to([20, 20]);
    assert_eq!(world.step(), Step::Blocked);
    assert_eq!(world.position, [9, 20]);
    // A missing mask blocks nothing, with a diagnostic.
    let mut files = fixture::tables();
    files.retain(|(path, _)| *path != "data/worldmap.txt");
    files.push(("data/worldmap.txt", MASKED));
    let tables = Fallout2Tables::load(&mut fixture::reader(files)).unwrap();
    assert!(tables.mask(0).is_none());
    assert!(
        tables
            .diagnostics
            .iter()
            .any(|note| note.file == "data/testmask.msk")
    );
}

#[test]
fn spec018_calendar() {
    assert_eq!(fallout2::game_date(fallout2::START_TICKS), [7, 25, 2241]);
    assert_eq!(fallout2::game_date(264600 + 7 * 864000), [8, 1, 2241]);
    assert_eq!(fallout2::game_date(160 * 864000), [1, 1, 2242]);
    assert_eq!(fallout2::game_date(0), [7, 25, 2241]);
    // Fallout 1's calendar is unchanged.
    assert_eq!(crate::host_save::game_date(864000), [12, 6, 2161]);
}

#[test]
fn spec018_pipboy_quests_and_holodisks() {
    let world = new_game();
    let mut globals = vec![0; 16];
    (globals[7], globals[8], globals[9]) = (2, 1, 5);
    assert_eq!(
        world.quest_list(&globals),
        [
            (1500, vec![(0, QuestStatus::Active)]),
            (1501, vec![(2, QuestStatus::Completed)]),
        ]
    );
    globals[12] = 1;
    assert_eq!(world.holodisk_list(&globals), [0]);
    globals[12] = -1;
    assert!(world.holodisk_list(&globals).is_empty());
}

#[test]
fn spec018_party_records() {
    let world = new_game();
    // A member of a `party.txt` record joins with the neutral record.
    let joined = world.joining_record(16777301);
    assert_eq!(
        (joined.record, joined.stage, joined.fields),
        (0, 0, [0, 0, 0, 0, 3, 3, 4])
    );
    assert_eq!(world.joining_record(16777999), MemberRecord::NONE);
    // A field outside its vocabulary reads as its neutral value.
    let mut notes = Vec::new();
    let record = Fallout2World::sanitize(
        MemberRecord {
            record: 0,
            stage: 1,
            fields: [2, 2, 3, 8, 0, 0, 1],
        },
        &mut notes,
    );
    assert_eq!(record.fields, [2, 2, 3, 0, 0, 0, 1]);
    assert_eq!(notes.len(), 1);
}

#[test]
fn spec018_area_entry() {
    let mut world = new_game();
    // Area 0 is visited (the start area): a later entry shows the town map.
    assert_eq!(world.enter_area(0), AreaEntry::TownMap);
    world.areas[0].known = 1;
    assert_eq!(world.enter_area(0), AreaEntry::Direct(0));
    assert_eq!(world.areas[0].known, 2);
    world.areas[0].locked = true;
    assert_eq!(world.enter_area(0), AreaEntry::Refused);
    assert_eq!(world.enter_area(5), AreaEntry::Refused);
    assert_eq!(world.area_at([175 + 20, 125]), Some(0));
    assert_eq!(world.area_at([175 + 21, 125]), None);
}

fn fallout2_host() -> WorldHost<TestAssets> {
    let assets =
        TestAssets::default().with_program("gated.int", program(&[(b"start", Vec::new())]));
    let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let mut host = WorldHost::new(world, assets);
    host.world.fallout2 = Some(Box::new(new_game()));
    host
}

fn diagnostics(host: &mut WorldHost<TestAssets>) -> Vec<String> {
    host.world
        .effects
        .drain(..)
        .filter_map(|effect| match effect {
            WorldEffect::Diagnostic(text) => Some(text),
            _ => None,
        })
        .collect()
}

#[test]
fn spec018_gated_scripts_load_nothing() {
    for gated in [true, false] {
        let mut host = fallout2_host();
        if !gated {
            host.world.fallout2 = None;
        }
        assert_eq!(host.scripts_gated(), gated);
        host.world.scripts_enabled = true;
        let owner = host.world.player;
        let sid = host.world.add_script(owner, 0, 4, 0).unwrap();
        for _ in 0..2 {
            let request = ScriptRequest {
                script: sid,
                event: ScriptEvent::Use,
                source: 0,
                target: 0,
                fixed: 0,
                action: -1,
                only_present: false,
            };
            let result = host.dispatch(request).unwrap();
            assert!(!result.overridden);
        }
        // No program enters the VM run list while gated; the record stays.
        assert_eq!(host.programs.is_empty(), gated);
        assert!(host.world.scripts.contains_key(&sid));
        let notes = diagnostics(&mut host);
        let gate = notes.iter().filter(|note| note.contains("gated")).count();
        assert_eq!(gate, usize::from(gated), "{notes:?}");
    }
    // The notice is printed once per call, only while gated.
    let mut host = fallout2_host();
    host.fallout2_notice();
    let messages: Vec<_> = host
        .world
        .effects
        .drain(..)
        .filter_map(|effect| match effect {
            WorldEffect::Message(text) => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(messages, [fallout2::GATE_NOTICE.as_bytes().to_vec()]);
    const { assert!(!fallout2::SCRIPT_GATE_OPEN) };
}

#[test]
fn spec018_entering_an_encounter_spawns_its_groups() {
    let mut assets = TestAssets::all_art();
    for pid in [16777217u32, 16777218, 16777219, 16777220] {
        let mut prototype = (*crate::test_support::npc_prototype()).clone();
        prototype.pid = pid;
        assets.prototypes.insert(pid, Arc::new(prototype));
    }
    assets.any_map = Some(crate::world::empty_map());
    let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let mut host = WorldHost::new(world, assets);
    host.world.fallout2 = Some(Box::new(new_game()));
    // §8.5 steps 5-7 after enc_01 is picked: map 0's one start point needs
    // no draw; TEST_Geckos: count 1 (no draw), direction 0, type draw 50
    // (type 0); TEST_Hunters: count draw 2, direction 3, only type 1
    // eligible (no draw).
    host.world.rng.script(&[0, 50, 2, 3]);
    host.fallout2_enter_encounter(Selected {
        table: 0,
        entry: 1,
        map: 0,
        special: false,
    })
    .unwrap();
    assert!(host.world.rng.unused_script().is_empty());
    let player = host.world.player;
    let player_hex = host.world.get(player).unwrap().raw.hex;
    assert_eq!(player_hex, Some(19086));
    let spawned: Vec<(u32, i32, bool)> = host
        .world
        .iter()
        .filter(|(reference, object)| *reference != player && object.raw.pid >> 24 == 1)
        .map(|(reference, object)| {
            (
                object.raw.pid,
                object.raw.hex.unwrap(),
                host.world.is_dead(reference),
            )
        })
        .collect();
    let step =
        |direction: u8, count: i32| vault_formats::HexGrid::step(19086, direction, count).unwrap();
    let mut expected = vec![
        (16777217, step(0, 6), false),
        (16777220, step(3, 8), true),
        (16777220, step(4, 8), true),
    ];
    expected.sort();
    let mut spawned = spawned;
    spawned.sort();
    assert_eq!(spawned, expected);
    // No campaign global was written.
    assert!(host.world.gvars.is_none());
}

#[test]
fn spec018_elevator_records() {
    let mut host = fallout2_host();
    let tables = tables_with(vec![(
        "data/vault-engine/elevators.txt",
        fixture::ELEVATORS,
    )]);
    host.world.fallout2 = Some(Box::new(Fallout2World::new_game(tables, 2)));
    let elevator = host.fallout2_elevator(0).unwrap();
    assert_eq!(elevator.stops.len(), 3);
    assert_eq!(elevator.start_level(2, 1), 1);
    diagnostics(&mut host);
    assert!(host.fallout2_elevator(1).is_none());
    assert!(host.fallout2_elevator(5).is_none());
    assert_eq!(diagnostics(&mut host).len(), 2);
    // Without the file every type does nothing.
    let mut host = fallout2_host();
    assert!(host.fallout2_elevator(0).is_none());
}
