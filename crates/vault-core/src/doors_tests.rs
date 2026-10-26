//! The `enhanced` auto-open doors: the player's routes through unscripted
//! and remembered doors, a script override stopping the walk, locks and
//! combat, Vanilla unchanged, both game profiles, and the sidecar table.
use super::*;
use crate::{
    CharacterSheet, GameProfile, ObjectAction, ScriptEvent, WorldHost, WorldState,
    fallout2::{Fallout2Tables, Fallout2World},
    fixture_state as fixture,
    host_choreography::Playback,
    routing::{find_route, step},
    test_support::{TestAssets as Assets, count, gvar, patched, program, request},
};
use std::sync::Arc;
use vault_formats::{
    Fid, FrmArchive,
    map::{ObjectDetail, ObjectState},
    pro::BasicFlags,
};

/// SPEC-005 `script_overrides`.
const SCRIPT_OVERRIDES: u16 = 0x80b9;

/// Four frames per direction at 10 fps, each moving one pixel right.
fn door_art() -> Arc<FrmArchive> {
    let mut bytes = Vec::new();
    bytes.extend(4_u32.to_be_bytes());
    bytes.extend(10_u16.to_be_bytes());
    bytes.extend(0_u16.to_be_bytes());
    bytes.extend(4_u16.to_be_bytes());
    bytes.extend([0; 24]);
    for index in 0..7_u32 {
        bytes.extend((index * 4 * 13).to_be_bytes());
    }
    for _ in 0..6 * 4 {
        bytes.extend(1_u16.to_be_bytes());
        bytes.extend(1_u16.to_be_bytes());
        bytes.extend(1_u32.to_be_bytes());
        bytes.extend(1_i16.to_be_bytes());
        bytes.extend(0_i16.to_be_bytes());
        bytes.push(1);
    }
    Arc::new(FrmArchive::parse(&bytes).unwrap())
}

/// The player at 20100 with critter art, outside combat, with instant
/// playback, on the map `SYNTH`, under `preset` and the `game` profile.
fn host(preset: EnginePreset, game: GameProfile) -> WorldHost<Assets> {
    let mut world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    world.gvars = Some(vec![0; 8]);
    let assets = Assets {
        art_policy: crate::test_support::ArtPolicy::Critters,
        ..Assets::default()
    }
    .with_program("synthetic.int", program(&[(b"start", Vec::new())]));
    let mut host = WorldHost::new(world, assets);
    host.playback = Playback::Instant;
    host.set_preset(preset);
    if game == GameProfile::Fallout2 {
        let files = vault_fixtures::fallout2::tables();
        let tables =
            Arc::new(Fallout2Tables::load(&mut vault_fixtures::fallout2::reader(files)).unwrap());
        let start = tables.maps.match_name(b"artemple.map");
        host.world.fallout2 = Some(Box::new(Fallout2World::new_game(tables, start)));
    }
    host.assets.tokens.insert((1, 0), b"hmjmps".to_vec());
    host.assets.art.insert(0x0200_0000, door_art());
    let player = host.world.player;
    host.world.get_mut(player).unwrap().raw.art_id = Fid::new(1, 0, 0, 0, 0).0;
    host.world.persistence.current_map = Some("SYNTH.SAV".into());
    host.world.effects.clear();
    host
}
fn hex(host: &WorldHost<Assets>, object: ObjectRef) -> i32 {
    host.world.get(object).unwrap().raw.hex.unwrap()
}
/// `count` hexes from `from` in `direction`.
fn hexes(from: i32, direction: u8, count: usize) -> i32 {
    (0..count).fold(from, |hex, _| step(hex, direction))
}
fn place(host: &mut WorldHost<Assets>, prototype: Arc<vault_formats::Prototype>, at: i32) -> u32 {
    let object = host.world.spawn(prototype, at, 0).unwrap();
    host.world.get_mut(object).unwrap().raw.flags = BasicFlags(0);
    object
}
/// A closed, unlocked door at `at`.
fn door(host: &mut WorldHost<Assets>, at: i32) -> ObjectRef {
    let door = place(host, patched(2, 0, &[(0x18, 0x800)], &[(0x28, b'A')]), at);
    host.world.get_mut(door).unwrap().raw.state = ObjectState::Other {
        word: 0,
        detail: ObjectDetail::Door(LockFlags(0)),
    };
    door
}
/// The goal four hexes northeast of the player, ringed by rocks but for
/// the hex toward the player, which holds a door: the only way in. Returns
/// the goal, the door and the hex before the door.
fn ringed(host: &mut WorldHost<Assets>) -> (i32, ObjectRef, i32) {
    let goal = hexes(20100, 1, 4);
    for direction in [0, 1, 2, 3, 5] {
        place(
            host,
            patched(2, 5, &[(0x18, 0x800)], &[]),
            step(goal, direction),
        );
    }
    let door = door(host, step(goal, 4));
    (goal, door, hexes(20100, 1, 2))
}
/// A script on `object` whose `use_p_proc` counts gvar 1, overriding first
/// when `overrides`.
fn script(host: &mut WorldHost<Assets>, object: ObjectRef, overrides: bool) {
    let mut body = Vec::new();
    if overrides {
        body.extend(fixture::instruction(SCRIPT_OVERRIDES));
    }
    body.extend(count(1));
    let program = program(&[(b"start", Vec::new()), (b"use_p_proc", body)]);
    host.assets.programs.push(("door.int".into(), program));
    let file = host.assets.programs.len() as i32 - 1;
    let sid = host.world.add_script(object, file, 3, 0).unwrap();
    host.dispatch(request(sid, ScriptEvent::Start)).unwrap();
}
fn open(host: &WorldHost<Assets>, door: ObjectRef) -> bool {
    let object = host.world.get(door).unwrap();
    object.lock().unwrap().contains(LockFlags::OPEN) && object.raw.flags.0 & 0x10 == 0x10
}
fn remembered(host: &WorldHost<Assets>, door: ObjectRef) -> bool {
    let key = host.world.door_key(door).unwrap();
    host.world.enhanced.opened_doors.contains(&key)
}
const GAMES: [GameProfile; 2] = [GameProfile::Fallout1, GameProfile::Fallout2];

#[test]
fn enhanced_player_walks_through_an_unscripted_unlocked_door() {
    for game in GAMES {
        let mut host = host(EnginePreset::Enhanced, game);
        let player = host.world.player;
        let (goal, door, _) = ringed(&mut host);
        assert!(host.world.may_auto_open(player, door));
        assert!(host.walk_toward(player, goal, false, 0).unwrap());
        assert_eq!(hex(&host, player), goal, "{game:?}");
        assert!(open(&host, door));
        // Not a click on the door: nothing is remembered.
        assert!(host.world.enhanced.is_empty());
    }
}

#[test]
fn enhanced_player_routes_through_a_scripted_door_only_after_opening_it_by_hand() {
    for game in GAMES {
        let mut host = host(EnginePreset::Enhanced, game);
        let player = host.world.player;
        let (goal, door, _) = ringed(&mut host);
        script(&mut host, door, false);
        // Never opened: the door blocks the player's route as in Vanilla.
        assert!(!host.world.may_auto_open(player, door));
        assert!(host.walk_toward(player, goal, false, 0).unwrap());
        assert_eq!(hex(&host, player), 20100);
        // A script's use of the door for the player is not by hand.
        host.script_action = true;
        host.action(player, door, ObjectAction::Use).unwrap();
        host.script_action = false;
        assert!(open(&host, door));
        assert!(!remembered(&host, door));
        host.action(player, door, ObjectAction::Use).unwrap();
        assert!(!open(&host, door));
        // The player's own click opens it: it is remembered (the close
        // that follows changes nothing).
        host.action(player, door, ObjectAction::Use).unwrap();
        assert!(open(&host, door) && remembered(&host, door));
        host.action(player, door, ObjectAction::Use).unwrap();
        assert!(!open(&host, door));
        assert_eq!(gvar(&host, 1), 4);
        // Now the walk passes it, its `use_p_proc` running on the way.
        let start = hex(&host, player);
        assert_ne!(start, goal);
        assert!(host.world.may_auto_open(player, door));
        assert!(host.walk_toward(player, goal, false, 0).unwrap());
        assert_eq!(hex(&host, player), goal, "{game:?}");
        assert!(open(&host, door));
        assert_eq!(gvar(&host, 1), 5);
    }
}

#[test]
fn a_remembered_doors_override_stops_the_walk_beside_it() {
    for game in GAMES {
        let mut host = host(EnginePreset::Enhanced, game);
        let player = host.world.player;
        let (goal, door, before) = ringed(&mut host);
        script(&mut host, door, true);
        // A click whose use the script overrides does not open the door,
        // so it is not remembered.
        host.action(player, door, ObjectAction::Use).unwrap();
        assert!(!open(&host, door) && !remembered(&host, door));
        assert_eq!(gvar(&host, 1), 1);
        // Remembered from an earlier open: the route passes the door, and
        // on reaching it the override stops the walk beside it.
        let key = host.world.door_key(door).unwrap();
        host.world.enhanced.opened_doors.insert(key);
        assert_eq!(hex(&host, player), before);
        assert!(host.walk_toward(player, 20100, false, 0).unwrap());
        assert_eq!(hex(&host, player), 20100);
        assert!(host.walk_toward(player, goal, false, 0).unwrap());
        assert_eq!(hex(&host, player), before, "{game:?}");
        assert!(!open(&host, door));
        assert_eq!(gvar(&host, 1), 2);
        assert!(!host.moving(player));
    }
}

#[test]
fn locked_and_jammed_doors_and_combat_keep_blocking_the_player() {
    for game in GAMES {
        for bits in [LockFlags::LOCKED.0, LockFlags::JAMMED.0] {
            let mut host = host(EnginePreset::Enhanced, game);
            let player = host.world.player;
            let (goal, door, _) = ringed(&mut host);
            host.world.get_mut(door).unwrap().set_lock(LockFlags(bits));
            assert!(!host.world.may_auto_open(player, door));
            assert!(host.world.route(player, 20100, goal, false).is_empty());
            assert!(host.walk_toward(player, goal, false, 0).unwrap());
            assert_eq!(hex(&host, player), 20100);
            let sound = |effect: &crate::WorldEffect| {
                matches!(effect, crate::WorldEffect::SoundEffect { .. })
            };
            assert!(!host.world.effects.iter().any(sound), "no locked sound");
        }
        // In combat the player's routes keep today's rules.
        let mut host = host(EnginePreset::Enhanced, game);
        let player = host.world.player;
        let (goal, door, _) = ringed(&mut host);
        host.world.combat = true;
        assert!(!host.world.may_auto_open(player, door));
        assert!(host.world.route(player, 20100, goal, false).is_empty());
        // A door without its prototype is never opened.
        host.world.combat = false;
        host.world.get_mut(door).unwrap().prototype = None;
        assert!(!host.world.may_auto_open(player, door));
    }
}

#[test]
fn vanilla_never_lets_the_player_auto_open_and_routes_as_before() {
    for game in GAMES {
        let mut host = host(EnginePreset::Vanilla, game);
        let player = host.world.player;
        let (goal, door, _) = ringed(&mut host);
        // Even a remembered door: Vanilla never reads the set.
        let key = host.world.door_key(door).unwrap();
        host.world.enhanced.opened_doors.insert(key);
        assert!(!host.world.may_auto_open(player, door));
        // The route is the pre-extension search, in which the player opens
        // no door, for a goal behind the door and one in the open.
        let occupancy = host.world.occupancy(0);
        for target in [goal, hexes(20100, 4, 6), hexes(goal, 0, 3)] {
            let before = find_route(
                20100,
                target,
                false,
                false,
                |at| host.world.movement_blocker_in(&occupancy, at, player),
                |_| false,
            );
            assert_eq!(host.world.route(player, 20100, target, false), before);
        }
        assert!(host.walk_toward(player, goal, false, 0).unwrap());
        assert_eq!(hex(&host, player), 20100);
        assert!(!open(&host, door));
        // A Vanilla click on the door still opens it, and nothing is
        // remembered.
        host.world.enhanced = EnhancedState::default();
        host.action(player, door, ObjectAction::Use).unwrap();
        assert!(open(&host, door));
        assert!(host.world.enhanced.is_empty());
    }
}

#[test]
fn npc_auto_open_is_the_same_under_both_presets() {
    for game in GAMES {
        let mut routes = Vec::new();
        for preset in [EnginePreset::Vanilla, EnginePreset::Enhanced] {
            let mut host = host(preset, game);
            let (goal, door, _) = ringed(&mut host);
            // A scripted door no player opened: the NPC rule ignores that.
            script(&mut host, door, false);
            let walker = place(&mut host, patched(1, 0, &[], &[]), hexes(20100, 1, 1));
            host.world.get_mut(walker).unwrap().raw.art_id = Fid::new(1, 0, 0, 0, 0).0;
            assert!(host.world.may_auto_open(walker, door));
            let start = hex(&host, walker);
            routes.push(host.world.route(walker, start, goal, false));
            assert!(host.walk_toward(walker, goal, false, 0).unwrap());
            assert_eq!(hex(&host, walker), goal);
            assert!(open(&host, door));
            assert!(host.world.enhanced.is_empty());
        }
        assert_eq!(routes[0], routes[1]);
        assert!(!routes[0].is_empty());
    }
}

#[test]
fn the_opened_doors_table_round_trips_and_names_bad_lines() {
    let mut doors = BTreeSet::new();
    for (map, id, pid) in [("SYNTH.SAV", 18000, 0x0200_0001), ("V13ENT.SAV", 7, 33)] {
        doors.insert(DoorKey {
            map: map.into(),
            id,
            pid,
        });
    }
    let text = crate::companion::write_sidecar(&Default::default()) + &write_table(&doors);
    assert_eq!(parse_table(&text).unwrap(), doors);
    assert!(crate::companion::parse_sidecar(&text).unwrap().is_empty());
    assert!(write_table(&BTreeSet::new()).is_empty());
    assert!(parse_table("").unwrap().is_empty());
    for (text, needle) in [
        ("[opened_doors]\n\"SYNTH.SAV:x\" = 1", "not an object id"),
        ("[opened_doors]\n\"SYNTH.SAV:1\" = y", "not a PID"),
        (
            "[opened_doors]\n\"../X.SAV:1\" = 1",
            "not a working map name",
        ),
        ("[opened_doors]\nSYNTH.SAV:1 = 1", "expected"),
        ("[opened_doors]\n\"SYNTH.SAV\" = 1", "expected"),
    ] {
        let error = parse_table(text).unwrap_err();
        assert!(error.contains(needle), "{text}: {error}");
    }
    // A door's key is the current map's working name, its object
    // identifier and its PID; without a map name there is none.
    let mut host = host(EnginePreset::Enhanced, GameProfile::Fallout1);
    let door = door(&mut host, hexes(20100, 1, 3));
    let key = host.world.door_key(door).unwrap();
    assert_eq!(key.map, "SYNTH.SAV");
    assert_eq!(key.id, host.world.get(door).unwrap().raw.id);
    host.world.persistence.current_map = None;
    assert_eq!(host.world.door_key(door), None);
}
