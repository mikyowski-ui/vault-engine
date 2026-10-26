//! SPEC-017 §7 vectors C-M under the Fallout 2 profile (and their Fallout 1
//! counterparts), run as SPEC-005 §4 runs its fixtures: a fragment from an
//! empty stack, with the script context bound and SPEC-018's world state
//! built from synthetic tables. Also the §8.5 gate's script path.
use crate::{
    CharacterSheet, ScriptEvent, WorldEffect, WorldHost, WorldState,
    fallout2::{Fallout2Tables, Fallout2World},
    fixture_items as items, fixture_state as fixture,
    queue::EventKind,
    test_support::{TestAssets, dir_assets, npc_prototype, patched, program, requested},
    world::ObjectRef,
};
use std::sync::Arc;
use vault_formats::{
    map::{ObjectDetail, ObjectState},
    pro::BasicFlags,
};
use vault_vm::{IntProgram, RunOutcome, Vm, VmError, WordSet};

fn hex(text: &str) -> Vec<u8> {
    text.split_whitespace()
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect()
}

/// §7.8's world: areas 0-9 and maps 0-9, with a 14 by 18 subtile grid
/// (700 by 900 pixels); area 2 at (100, 100); area 1 with three entrances
/// to maps 3 and 4.
pub(crate) fn tables() -> Arc<Fallout2Tables> {
    let mut city = String::new();
    for area in 0..10 {
        let position = if area == 2 { 100 } else { 10 + area * 20 };
        city.push_str(&format!(
            "[Area {area:02}]\narea_name=Area {area}\nworld_pos={position},{position}\n"
        ));
        if area == 1 {
            city.push_str(
                "entrance_0=Off,1,1,Map 3,-1,-1,0\n\
                 entrance_1=Off,1,1,Map 3,1,100,0\n\
                 entrance_2=Off,1,1,Map 4,0,100,0\n",
            );
        }
    }
    let mut maps = String::new();
    for map in 0..10 {
        maps.push_str(&format!(
            "[Map {map:03}]\nlookup_name=Map {map}\nmap_name=map{map}\nmusic=track{map}\n"
        ));
    }
    let mut worldmap =
        String::from("[Data]\nterrain_types=Desert:1\n[Tile Data]\nnum_horizontal_tiles=2\n");
    for tile in 0..6 {
        worldmap.push_str(&format!("[Tile {tile}]\nart_idx=1\n"));
    }
    let files: Vec<(&str, String)> = vec![
        ("data/city.txt", city),
        ("data/maps.txt", maps),
        ("data/worldmap.txt", worldmap),
        (
            "data/party.txt",
            vault_fixtures::fallout2::PARTY.to_string(),
        ),
    ];
    let mut read = move |path: &str| -> Result<Option<Vec<u8>>, String> {
        Ok(files
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, text)| text.as_bytes().to_vec()))
    };
    Arc::new(Fallout2Tables::load(&mut read).unwrap())
}

/// A Fallout 2 host whose areas and maps all start unknown, on map 3, with
/// a non-player critter `self` (at hex 20000) holding script instance
/// `script`; and the base program the fragments run from (static string
/// offset 6 is `testtrk`).
struct Harness {
    host: WorldHost<TestAssets>,
    base: IntProgram,
    me: ObjectRef,
    script: u32,
}
fn harness(fallout2: bool) -> Harness {
    let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let mut host = WorldHost::new(world, TestAssets::all_art());
    if fallout2 {
        let mut state = Fallout2World::new_game(tables(), -1);
        for area in &mut state.areas {
            area.known = 0;
        }
        state.map_flags.fill(0);
        host.world.fallout2 = Some(Box::new(state));
    }
    host.world.map.header.map_number = 3;
    let me = host.world.spawn(npc_prototype(), 20000, 0).unwrap();
    let script = host.world.add_script(me, 0, 4, 0).unwrap();
    let words = if fallout2 {
        WordSet::FALLOUT2
    } else {
        WordSet::FALLOUT1
    };
    let base = IntProgram::parse_in(&fixture::program(&[b"testtrk"], &[]), &words, false).unwrap();
    host.world.effects.clear();
    Harness {
        host,
        base,
        me,
        script,
    }
}
impl Harness {
    fn run(&mut self, bytes: &[u8]) -> Result<Vm, VmError> {
        let mut vm = Vm::fragment(self.base.clone(), bytes).unwrap();
        vm.id = 50;
        self.host.bind_program(&vm, self.script);
        for _ in 0..100 {
            match vm.run(&mut self.host, 1000)? {
                RunOutcome::Budget => {}
                _ => return Ok(vm),
            }
        }
        Err(VmError::InstructionLimit)
    }
    fn state(&self) -> &Fallout2World {
        self.host.world.fallout2.as_deref().unwrap()
    }
    fn diagnostics(&mut self) -> Vec<String> {
        self.host
            .world
            .effects
            .drain(..)
            .filter_map(|effect| match effect {
                WorldEffect::Diagnostic(text) => Some(text),
                _ => None,
            })
            .collect()
    }
}
/// The value stack's integers, bottom to top.
fn integers(vm: &Vm) -> Vec<i32> {
    vm.value_stack
        .bytes()
        .chunks(6)
        .map(|slot| {
            assert_eq!(&slot[4..], [0xc0, 0x01]);
            i32::from_be_bytes(slot[..4].try_into().unwrap())
        })
        .collect()
}

#[test]
fn spec017_vector_c_the_profile_decides_0x80e1() {
    let bytes = hex("C0 01 00 00 00 00 C0 01 00 00 00 00 C0 01 00 00 \
         00 00 C0 01 00 00 00 00 80 E1");
    // Fallout 2: metarule3 sub-function 0, unknown: I 0 and a diagnostic.
    let mut f2 = harness(true);
    let vm = f2.run(&bytes).unwrap();
    assert_eq!(vm.value_stack.bytes(), hex("00 00 00 00 C0 01"));
    assert!(
        f2.diagnostics()
            .iter()
            .any(|note| note.contains("sub-function 0"))
    );
    // Fallout 1: turn_off_objs_in_area over elevation 0, hex 0: no object
    // there, nothing changes, nothing is pushed.
    let mut f1 = harness(false);
    let hidden: Vec<_> = f1.host.world.iter().map(|(_, o)| o.raw.flags.0).collect();
    let vm = f1.run(&bytes).unwrap();
    assert!(vm.value_stack.bytes().is_empty());
    let after: Vec<_> = f1.host.world.iter().map(|(_, o)| o.raw.flags.0).collect();
    assert_eq!(hidden, after);
}

#[test]
fn spec017_vector_d_metarule3_100_and_an_unknown_sub_function() {
    let mut h = harness(true);
    let other = h.host.world.spawn(npc_prototype(), 20002, 0).unwrap();
    let nine = h.host.world.add_script(other, 0, 4, 0).unwrap();
    let me = h.me;
    let timer = |script, parameter| EventKind::Script { script, parameter };
    let drug = EventKind::Drug {
        stats: [0; 3],
        modifiers: [0; 3],
    };
    for (delay, object, kind) in [
        (1, me, timer(h.script, 1)),
        (2, me, timer(h.script, 2)),
        (3, me, drug),
        (4, me, timer(h.script, 1)),
        (5, other, timer(nine, 1)),
    ] {
        h.host.world.queue.insert(0, delay, object, kind).unwrap();
    }
    let vm = h
        .run(&hex("C0 01 00 00 00 64 80 BC C0 01 00 00 00 01 C0 01 \
             00 00 00 00 80 E1 C0 01 00 00 00 63 C0 01 00 00 \
             00 07 C0 01 00 00 00 08 C0 01 00 00 00 09 80 E1"))
        .unwrap();
    assert_eq!(
        vm.value_stack.bytes(),
        hex("00 00 00 00 C0 01 00 00 00 00 C0 01")
    );
    let left: Vec<_> = h
        .host
        .world
        .queue
        .events()
        .map(|event| event.kind)
        .collect();
    assert_eq!(left, [timer(h.script, 2), drug, timer(nine, 1)]);
    assert!(
        h.diagnostics()
            .iter()
            .any(|note| note.contains("sub-function 99"))
    );
}

#[test]
fn spec017_vector_e_metarule3_106_walks_a_hex() {
    let mut h = harness(true);
    let world = &mut h.host.world;
    let item = world
        .spawn(items::misc(0x0000_0099, 1, 1, 1), 12345, 0)
        .unwrap();
    let a = world.spawn(npc_prototype(), 12345, 0).unwrap();
    let b = world.spawn(npc_prototype(), 12345, 0).unwrap();
    let c = world.spawn(npc_prototype(), 12345, 1).unwrap();
    assert!(item < a && a < b && b < c);
    let walk = hex("C0 01 00 00 00 6A C0 01 00 00 30 39 C0 01 00 00 \
         00 00 C0 01 00 00 00 6A C0 01 00 00 30 39 C0 01 \
         00 00 00 00 C0 01 00 00 00 6A C0 01 00 00 30 39 \
         C0 01 00 00 00 00 C0 01 00 00 00 00 80 E1 80 E1 \
         80 E1");
    let vm = h.run(&walk).unwrap();
    assert_eq!(vm.value_stack.bytes(), hex("00 00 00 00 C0 01"));
    // The first two calls give A, then B.
    let vm = h.run(&walk[..walk.len() - 4]).unwrap();
    assert_eq!(integers(&vm)[6], a as i32);
    let vm = h.run(&walk[..walk.len() - 2]).unwrap();
    assert_eq!(integers(&vm)[3], b as i32);
    // Elevation 1 holds C; outside 0-2 or 0-39999 there is none.
    let call = |hex_number: i32, elevation: i32| {
        [
            fixture::push(106),
            fixture::push(hex_number),
            fixture::push(elevation),
            fixture::push(0),
            fixture::instruction(0x80e1),
        ]
        .concat()
    };
    assert_eq!(integers(&h.run(&call(12345, 1)).unwrap()), [c as i32]);
    assert_eq!(integers(&h.run(&call(12345, 3)).unwrap()), [0]);
    assert_eq!(integers(&h.run(&call(40000, 0)).unwrap()), [0]);
    // A previous critter not among them gives 0.
    let stray = [
        fixture::push(106),
        fixture::push(12345),
        fixture::push(0),
        fixture::push(c as i32),
        fixture::instruction(0x80e1),
    ]
    .concat();
    assert_eq!(integers(&h.run(&stray).unwrap()), [0]);
}

#[test]
fn spec017_vector_f_metarule3_107_sets_the_art_base() {
    let fragment = hex("C0 01 00 00 00 6B 80 BC C0 01 01 00 00 05 C0 01 \
         00 00 00 00 80 E1");
    let mut h = harness(true);
    h.host.world.get_mut(h.me).unwrap().raw.art_id = 0x0100_4003;
    let vm = h.run(&fragment).unwrap();
    assert_eq!(integers(&vm), [0]);
    assert_eq!(h.host.world.get(h.me).unwrap().raw.art_id, 0x0100_4005);
    // The player, wearing no armor: the default player art and the art.
    for worn in [false, true] {
        let mut h = harness(true);
        let player = h.host.world.player;
        h.host.world.get_mut(player).unwrap().raw.art_id = 0x0100_400b;
        if worn {
            let armor = h
                .host
                .world
                .spawn(items::armor(0x0000_0011, 1, 2), 1, 0)
                .unwrap();
            h.host.world.add_item(player, armor, 1, true).unwrap();
            h.host.world.get_mut(armor).unwrap().raw.flags.0 |= BasicFlags::WORN.0;
        }
        let script = h.host.world.add_script(player, 0, 4, 0).unwrap();
        h.script = script;
        h.run(&fragment).unwrap();
        assert_eq!(h.host.world.default_player_art, 5);
        let art = h.host.world.get(player).unwrap().raw.art_id;
        assert_eq!(art, if worn { 0x0100_400b } else { 0x0100_4005 });
    }
}

#[test]
fn spec017_vector_g_tile_in_tile_rect() {
    let fragment = hex("C0 01 00 00 07 E4 C0 01 00 00 07 DA C0 01 00 00 \
         0F B4 C0 01 00 00 0F AA C0 01 00 00 0B C7 80 CF \
         C0 01 00 00 07 E4 C0 01 00 00 07 DA C0 01 00 00 \
         0F B4 C0 01 00 00 0F AA C0 01 00 00 0B D1 80 CF");
    let mut h = harness(true);
    let vm = h.run(&fragment).unwrap();
    assert_eq!(
        vm.value_stack.bytes(),
        hex("00 00 00 01 C0 01 00 00 00 00 C0 01")
    );
    // The first test hex out of range gives 0.
    let mut mutated = fragment.clone();
    mutated[0x1a..0x1e].copy_from_slice(&hex("00 00 9C 40"));
    assert_eq!(integers(&h.run(&mutated).unwrap()), [0, 0]);
    // Under Fallout 1, 0x80CF is animate_jump: nothing is popped.
    let mut f1 = harness(false);
    assert_eq!(f1.run(&fragment).unwrap().value_stack.byte_len(), 60);
    // Corner order does not matter.
    use crate::host_fallout2_words::tile_in_tile_rect;
    assert!(tile_in_tile_rect([4010, 2020, 4020, 2010], 3015));
    assert!(!tile_in_tile_rect([4010, 2020, 4020, 2010], 3025));
    assert!(!tile_in_tile_rect([-1, 2020, 4020, 2010], 3015));
}

/// §7.8 Vector H's fragment.
const VECTOR_H: &str = "C0 01 00 00 00 03 90 01 00 00 00 06 80 E2 C0 01 \
     00 00 00 02 C0 01 00 00 01 5E C0 01 00 00 02 58 \
     80 E5 C0 01 00 00 00 00 C0 01 00 00 00 04 C0 01 \
     00 00 00 01 80 B2 C0 01 00 00 00 01 C0 01 00 00 \
     00 07 C0 01 00 00 00 02 80 B2 C0 01 00 00 00 02 \
     C0 01 00 00 00 00 C0 01 00 00 00 01 80 B2";

#[test]
fn spec017_vector_h_music_area_position_and_area_knowledge() {
    let mut h = harness(true);
    assert_eq!(h.state().areas[2].position, [100, 100]);
    let vm = h.run(&hex(VECTOR_H)).unwrap();
    assert!(vm.value_stack.bytes().is_empty() && vm.return_stack.bytes().is_empty());
    let state = h.state();
    assert_eq!(state.map_music.get(&3).map(String::as_str), Some("testtrk"));
    assert_eq!(state.areas[2].position, [350, 600]);
    assert_eq!(state.areas[4].known, 1);
    assert_eq!(state.map_flags[7], 0x03);
    assert!(
        state
            .areas
            .iter()
            .enumerate()
            .all(|(n, a)| (a.known == 0) == (n != 4))
    );
    assert_eq!(h.host.last_music.as_deref(), Some("testtrk"));
    let notes = h.diagnostics();
    assert_eq!(
        notes
            .iter()
            .filter(|note| note.contains("mark type 2"))
            .count(),
        1,
        "{notes:?}"
    );
    // The override is the city track from now on; an empty name restores
    // the table's. A map without a record changes nothing.
    h.host
        .world
        .fallout2
        .as_deref_mut()
        .unwrap()
        .map_music
        .remove(&3);
    let set = |map: i32, offset: u32| {
        [
            fixture::push(map),
            vec![0x90, 0x01],
            offset.to_be_bytes().to_vec(),
            fixture::instruction(0x80e2),
        ]
        .concat()
    };
    h.run(&set(12, 6)).unwrap();
    assert!(h.state().map_music.is_empty());
    assert!(h.diagnostics().iter().any(|note| note.contains("map 12")));
    // A point outside the world, or an absent area, changes nothing.
    for (area, x, y) in [(2, 700, 0), (2, 0, 900), (10, 1, 1)] {
        let bytes = [
            fixture::push(area),
            fixture::push(x),
            fixture::push(y),
            fixture::instruction(0x80e5),
        ]
        .concat();
        h.run(&bytes).unwrap();
        assert_eq!(h.state().areas[2].position, [350, 600]);
        assert!(!h.diagnostics().is_empty());
    }
    // Mark type 1 state 0 clears both flags; state 1 sets only known.
    let mark = |kind: i32, index: i32, state: i32| {
        [
            fixture::push(kind),
            fixture::push(index),
            fixture::push(state),
            fixture::instruction(0x80b2),
        ]
        .concat()
    };
    h.run(&mark(1, 7, 0)).unwrap();
    assert_eq!(h.state().map_flags[7], 0);
    h.run(&mark(1, 7, 1)).unwrap();
    assert_eq!(h.state().map_flags[7], 0x01);
    h.run(&mark(0, 4, 2)).unwrap();
    assert_eq!(h.state().areas[4].known, 2);
    h.run(&mark(0, 4, 3)).unwrap();
    assert_eq!(h.state().areas[4].known, 2);
}

#[test]
fn spec017_vector_i_the_fallout2_metarule_rules() {
    let rules = hex("C0 01 00 00 00 11 C0 01 00 00 00 04 81 0B C0 01 \
         00 00 00 13 C0 01 00 00 00 07 81 0B C0 01 00 00 \
         00 11 C0 01 00 00 00 05 81 0B C0 01 00 00 00 63 \
         C0 01 00 00 00 00 81 0B");
    let mut h = harness(true);
    h.run(&hex(VECTOR_H)).unwrap();
    assert_eq!(integers(&h.run(&rules).unwrap()), [1, 1, 0, 0]);
    let mut f1 = harness(false);
    assert_eq!(integers(&f1.run(&rules).unwrap()), [0, 0, 0, 0]);
}

/// One `metarule` call.
fn metarule(rule: i32, parameter: i32) -> Vec<u8> {
    [
        fixture::push(rule),
        fixture::push(parameter),
        fixture::instruction(0x810b),
    ]
    .concat()
}
/// One `metarule3` call with integer parameters.
fn metarule3(sub: i32, [p1, p2, p3]: [i32; 3]) -> Vec<u8> {
    [
        fixture::push(sub),
        fixture::push(p1),
        fixture::push(p2),
        fixture::push(p3),
        fixture::instruction(0x80e1),
    ]
    .concat()
}

#[test]
fn spec017_metarule_rules_read_and_write_spec018_state() {
    let mut h = harness(true);
    let me = h.me as i32;
    // The car: none, then given (map 3 is area 1's), fueled and clamped.
    assert_eq!(integers(&h.run(&metarule(30, 0)).unwrap()), [-1]);
    assert_eq!(integers(&h.run(&metarule(32, 500)).unwrap()), [0]);
    assert_eq!(h.state().car.fuel, 0);
    assert_eq!(integers(&h.run(&metarule3(110, [0; 3])).unwrap()), [0]);
    h.run(&metarule(31, 0)).unwrap();
    assert_eq!(integers(&h.run(&metarule(30, 0)).unwrap()), [1]);
    assert_eq!(h.state().car.map, 3);
    assert_eq!(integers(&h.run(&metarule3(110, [0; 3])).unwrap()), [1]);
    h.run(&metarule(32, 90000)).unwrap();
    assert_eq!(h.state().car.fuel, 80000);
    h.run(&metarule(32, -100000)).unwrap();
    assert_eq!(h.state().car.fuel, 0);
    // Position and current area.
    let state = h.host.world.fallout2.as_deref_mut().unwrap();
    state.position = [123, 456];
    state.area = 6;
    assert_eq!(integers(&h.run(&metarule(44, 0)).unwrap()), [123]);
    assert_eq!(integers(&h.run(&metarule(45, 0)).unwrap()), [456]);
    assert_eq!(integers(&h.run(&metarule(46, 0)).unwrap()), [6]);
    // Preferences: SPEC-014 defaults 0 and 3.
    assert_eq!(integers(&h.run(&metarule(47, 0)).unwrap()), [0]);
    assert_eq!(integers(&h.run(&metarule(48, 0)).unwrap()), [3]);
    h.host.world.violence = Some(1);
    assert_eq!(integers(&h.run(&metarule(48, 0)).unwrap()), [1]);
    // Tagged skills.
    h.host.world.player_sheet_mut().unwrap().tags = [3, 7, -1, -1];
    assert_eq!(integers(&h.run(&metarule(40, 7)).unwrap()), [1]);
    assert_eq!(integers(&h.run(&metarule(40, 8)).unwrap()), [0]);
    assert_eq!(integers(&h.run(&metarule(40, -1)).unwrap()), [0]);
    // Drugs in the queue.
    assert_eq!(integers(&h.run(&metarule(18, me)).unwrap()), [0]);
    let drug = EventKind::Drug {
        stats: [0; 3],
        modifiers: [0; 3],
    };
    h.host.world.queue.insert(0, 5, h.me, drug).unwrap();
    assert_eq!(integers(&h.run(&metarule(18, me)).unwrap()), [1]);
    assert_eq!(integers(&h.run(&metarule(18, 0)).unwrap()), [0]);
    // Critter prototype rules; a non-critter gives -1 with a diagnostic.
    let item = h
        .host
        .world
        .spawn(items::misc(0x0000_0098, 1, 1, 1), 1, 0)
        .unwrap() as i32;
    let kill = match &h
        .host
        .world
        .get(h.me)
        .unwrap()
        .prototype
        .as_deref()
        .unwrap()
        .data
    {
        vault_formats::pro::PrototypeData::Critter(critter) => critter.kill_category,
        _ => unreachable!(),
    };
    assert_eq!(integers(&h.run(&metarule(51, me)).unwrap()), [kill]);
    assert_eq!(integers(&h.run(&metarule(51, item)).unwrap()), [-1]);
    assert_eq!(integers(&h.run(&metarule(49, item)).unwrap()), [-1]);
    assert_eq!(integers(&h.run(&metarule(50, item)).unwrap()), [0]);
    // A stale reference behaves as null, with a diagnostic.
    h.diagnostics();
    assert_eq!(integers(&h.run(&metarule(51, 0x7fff_0000)).unwrap()), [-1]);
    assert!(
        h.diagnostics()
            .iter()
            .any(|note| note.contains("names no live object"))
    );
    // Rule 15 without an elevator record: nothing, one diagnostic.
    assert_eq!(integers(&h.run(&metarule(15, 4)).unwrap()), [0]);
    let elevator = h
        .host
        .world
        .effects
        .iter()
        .any(|effect| matches!(effect, WorldEffect::Elevator(_)));
    assert!(!elevator);
    assert!(
        h.diagnostics()
            .iter()
            .any(|note| note.contains("elevator type 4"))
    );
    // SPEC-005's rules keep their rows; 16 counts every party entry.
    h.host.world.party.insert(h.host.world.player);
    assert_eq!(integers(&h.run(&metarule(16, 0)).unwrap()), [1]);
    assert_eq!(integers(&h.run(&metarule(20, 0)).unwrap()), [0]);
}

#[test]
fn spec017_metarule_42_and_43_drop_and_unwield() {
    let mut h = harness(true);
    let me = h.me;
    let first = h
        .host
        .world
        .spawn(items::misc(0x0000_0097, 1, 1, 1), 1, 0)
        .unwrap();
    let second = h
        .host
        .world
        .spawn(items::misc(0x0000_0096, 1, 1, 1), 1, 0)
        .unwrap();
    h.host.world.add_item(me, first, 1, true).unwrap();
    h.host.world.add_item(me, second, 1, true).unwrap();
    h.host.world.get_mut(second).unwrap().raw.flags.0 |= BasicFlags::RIGHT_HAND.0;
    h.run(&metarule(43, me as i32)).unwrap();
    assert!(
        !h.host
            .world
            .get(second)
            .unwrap()
            .raw
            .flags
            .contains(BasicFlags::RIGHT_HAND)
    );
    h.run(&metarule(42, me as i32)).unwrap();
    assert!(h.host.world.get(me).unwrap().inventory.is_empty());
    for item in [first, second] {
        let object = h.host.world.get(item).unwrap();
        assert_eq!((object.raw.hex, object.owner), (Some(20000), None));
    }
    h.diagnostics();
    h.run(&metarule(42, 0)).unwrap();
    assert!(
        h.diagnostics()
            .iter()
            .any(|note| note.contains("null critter"))
    );
}

#[test]
fn spec017_metarule3_sub_functions_101_to_111() {
    let mut h = harness(true);
    // 101: a radius-1 square around the subtile of (75, 75), raised to 1
    // only from 0; 105 reads it; outside the world gives -1.
    let state = h.host.world.fallout2.as_deref_mut().unwrap();
    state.discovery.fill(0);
    let [columns, _] = state.grid();
    state.discovery[columns + 1] = 2;
    h.run(&metarule3(101, [75, 75, 1])).unwrap();
    let state = h.state();
    for (cell, value) in [([0, 0], 1), ([2, 2], 1), ([1, 1], 2), ([3, 3], 0)] {
        assert_eq!(state.discovery(cell), value, "{cell:?}");
    }
    assert_eq!(integers(&h.run(&metarule3(105, [75, 75, 0])).unwrap()), [2]);
    assert_eq!(integers(&h.run(&metarule3(105, [10, 10, 0])).unwrap()), [1]);
    assert_eq!(
        integers(&h.run(&metarule3(105, [700, 0, 0])).unwrap()),
        [-1]
    );
    // A negative radius changes nothing.
    h.run(&metarule3(101, [325, 325, -1])).unwrap();
    assert_eq!(h.state().discovery([6, 6]), 0);
    // 103: kill counts over categories 0-18.
    h.host.world.kills[4] = 7;
    h.host.world.fallout2.as_deref_mut().unwrap().kills[2] = 9;
    assert_eq!(integers(&h.run(&metarule3(103, [4, 0, 0])).unwrap()), [7]);
    assert_eq!(integers(&h.run(&metarule3(103, [17, 0, 0])).unwrap()), [9]);
    assert_eq!(integers(&h.run(&metarule3(103, [19, 0, 0])).unwrap()), [0]);
    // 104: map 3's entrances of elevation 1 (slot 0 takes -1, slot 1 is
    // elevation 1); map 4 untouched; an absent map gives -1.
    assert_eq!(integers(&h.run(&metarule3(104, [3, 1, 1])).unwrap()), [0]);
    assert_eq!(h.state().areas[1].entrances, [true, true, false]);
    h.run(&metarule3(104, [3, 0, 0])).unwrap();
    assert_eq!(h.state().areas[1].entrances, [false, true, false]);
    h.run(&metarule3(104, [4, 1, -1])).unwrap();
    assert_eq!(h.state().areas[1].entrances, [false, true, true]);
    assert_eq!(
        integers(&h.run(&metarule3(104, [42, 1, -1])).unwrap()),
        [-1]
    );
    // 108: the view centers on the hex; out of range changes nothing.
    h.run(&metarule3(108, [20102, 0, 0])).unwrap();
    assert_eq!(h.host.world.camera_hex, 20102);
    h.run(&metarule3(108, [-5, 0, 0])).unwrap();
    assert_eq!(h.host.world.camera_hex, 20102);
    // 109: no control record gives -1.
    assert_eq!(
        integers(&h.run(&metarule3(109, [h.me as i32, 0, 0])).unwrap()),
        [-1]
    );
    // 111: the current map's area (map 3 is named by area 1).
    assert_eq!(integers(&h.run(&metarule3(111, [0; 3])).unwrap()), [1]);
    // A used parameter that is not an integer: nothing, I 0, diagnostic.
    let typed = [
        fixture::push(103),
        vec![0x90, 0x01, 0, 0, 0, 6],
        fixture::push(0),
        fixture::push(0),
        fixture::instruction(0x80e1),
    ]
    .concat();
    h.diagnostics();
    assert_eq!(integers(&h.run(&typed).unwrap()), [0]);
    assert!(
        h.diagnostics()
            .iter()
            .any(|note| note.contains("not an integer"))
    );
    // The sub-function itself must be I.
    let untyped = [
        vec![0x90, 0x01, 0, 0, 0, 6],
        fixture::push(0),
        fixture::push(0),
        fixture::push(0),
        fixture::instruction(0x80e1),
    ]
    .concat();
    assert!(matches!(h.run(&untyped), Err(VmError::Type(0x9001))));
    // 102: the world-map track, cut to 15 bytes; restarts while open.
    let music = [
        fixture::push(102),
        vec![0x90, 0x01, 0, 0, 0, 6],
        fixture::push(0),
        fixture::push(0),
        fixture::instruction(0x80e1),
    ]
    .concat();
    h.host.world.fallout2.as_deref_mut().unwrap().world_map_open = true;
    h.run(&music).unwrap();
    assert_eq!(h.state().world_map_music.as_deref(), Some("testtrk"));
    assert_eq!(h.host.last_music.as_deref(), Some("testtrk"));
    h.host.world_map_closed();
    assert!(!h.state().world_map_open);
}

#[test]
fn spec017_vector_j_set_exit_grids() {
    let mut h = harness(true);
    let grid = |h: &mut Harness, pid: u32, elevation: u8, hidden: bool| {
        let object = h
            .host
            .world
            .spawn(patched(5, 0, &[(0, pid as i32)], &[]), 30000, elevation)
            .unwrap();
        let target = h.host.world.get_mut(object).unwrap();
        target.raw.flags = BasicFlags(if hidden { BasicFlags::HIDDEN.0 } else { 0 });
        target.raw.state = ObjectState::Other {
            word: 0,
            detail: ObjectDetail::ExitGrid {
                map: 9,
                hex: 100,
                elevation: 0,
                orientation: 0,
            },
        };
        object
    };
    let g1 = grid(&mut h, 0x0500_0010, 0, true);
    let g2 = grid(&mut h, 0x0500_0011, 0, false);
    let g3 = grid(&mut h, 0x0500_0010, 1, false);
    let art: Vec<u32> = [g1, g2, g3]
        .iter()
        .map(|object| h.host.world.get(*object).unwrap().raw.art_id)
        .collect();
    let vm = h
        .run(&hex("C0 01 00 00 00 00 C0 01 00 00 00 05 C0 01 00 00 \
             00 01 C0 01 00 00 4E 84 C0 01 00 00 00 02 80 E6"))
        .unwrap();
    assert!(vm.value_stack.bytes().is_empty() && vm.return_stack.bytes().is_empty());
    let destination = |object: ObjectRef| match h.host.world.get(object).unwrap().raw.state {
        ObjectState::Other {
            detail:
                ObjectDetail::ExitGrid {
                    map,
                    hex,
                    elevation,
                    orientation,
                },
            ..
        } => [map, hex, elevation, orientation],
        _ => unreachable!(),
    };
    assert_eq!(destination(g1), [5, 20100, 1, 2]);
    assert_eq!(destination(g2), [5, 20100, 1, 2]);
    assert_eq!(destination(g3), [9, 100, 0, 0]);
    let after: Vec<u32> = [g1, g2, g3]
        .iter()
        .map(|object| h.host.world.get(*object).unwrap().raw.art_id)
        .collect();
    assert_eq!(art, after);
}

#[test]
fn spec017_vector_k_inven_cmds() {
    let mut h = harness(true);
    let stack = h
        .host
        .world
        .spawn(items::misc(0x0000_0095, 1, 1, 1), 1, 0)
        .unwrap();
    let single = h
        .host
        .world
        .spawn(items::misc(0x0000_0094, 1, 1, 1), 1, 0)
        .unwrap();
    h.host.world.add_item(h.me, stack, 3, true).unwrap();
    h.host.world.add_item(h.me, single, 1, true).unwrap();
    let inventory = h.host.world.get(h.me).unwrap().inventory.clone();
    assert_eq!(inventory.len(), 2);
    let vm = h
        .run(&hex("80 BC C0 01 00 00 00 0D C0 01 00 00 00 01 81 56 \
             80 BC C0 01 00 00 00 0D C0 01 00 00 00 02 81 56 \
             80 BC C0 01 00 00 00 0C C0 01 00 00 00 00 81 56"))
        .unwrap();
    assert_eq!(integers(&vm), [inventory[1].0 as i32, 0, 0]);
    let notes = h.diagnostics();
    assert_eq!(notes.iter().filter(|n| n.contains("command 12")).count(), 1);
    // Index 0 is the stack's representative; a null owner gives 0.
    let call = |owner: i32, index: i32| {
        [
            fixture::push(owner),
            fixture::push(13),
            fixture::push(index),
            fixture::instruction(0x8156),
        ]
        .concat()
    };
    assert_eq!(
        integers(&h.run(&call(h.me as i32, 0)).unwrap()),
        [inventory[0].0 as i32]
    );
    assert_eq!(integers(&h.run(&call(h.me as i32, -1)).unwrap()), [0]);
    assert_eq!(integers(&h.run(&call(0, 0)).unwrap()), [0]);
    assert!(h.diagnostics().iter().any(|n| n.contains("null owner")));
}

/// §7.12 Vector L's fragment.
const VECTOR_L: &str = "C0 01 00 00 00 01 C0 01 00 00 00 01 81 0E 80 BC \
     C0 01 01 00 00 05 C0 01 00 00 00 00 81 57 80 BC \
     C0 01 00 00 30 39 C0 01 FF FF FF FF 81 59 80 BC \
     C0 01 00 00 00 04 C0 01 FF FF FF FF 81 58 C0 01 \
     00 00 00 03 C0 01 00 00 00 00 81 0E";

fn registered(h: &mut Harness) -> Vec<(u16, Vec<i32>)> {
    h.host
        .world
        .effects
        .iter()
        .filter_map(|effect| match effect {
            WorldEffect::Animation { opcode, values, .. } => Some((*opcode, values.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn spec017_vector_l_the_three_registration_words() {
    let mut h = harness(true);
    let me = h.me as i32;
    let vm = h.run(&hex(VECTOR_L)).unwrap();
    assert!(vm.value_stack.bytes().is_empty() && vm.return_stack.bytes().is_empty());
    assert_eq!(
        registered(&mut h),
        [
            (0x810e, vec![1, 1]),
            (0x8157, vec![0, 0x0100_0005, me]),
            (0x8159, vec![-1, 12345, me]),
            (0x8158, vec![-1, 4, me]),
            (0x810e, vec![0, 3]),
        ]
    );
    // The sequence played at once: the art keeps weapon code 4, facing
    // the hex.
    let object = h.host.world.get(h.me).unwrap();
    assert_eq!(vault_formats::Fid(object.raw.art_id).weapon(), 4);
    assert_eq!(
        Some(object.raw.orientation),
        crate::routing::rotation(20000, 12345)
    );
    // In combat every operand is consumed and nothing registers.
    let mut h = harness(true);
    h.host.world.combat = true;
    let vm = h.run(&hex(VECTOR_L)).unwrap();
    assert!(vm.value_stack.bytes().is_empty());
    assert!(registered(&mut h).is_empty());
    // Weapon code 0: only the set-art and turn entries, one diagnostic.
    let mut h = harness(true);
    let mut bytes = hex(VECTOR_L);
    bytes[0x30..0x36].copy_from_slice(&hex("C0 01 00 00 00 00"));
    h.run(&bytes).unwrap();
    let words: Vec<u16> = registered(&mut h).iter().map(|(word, _)| *word).collect();
    assert_eq!(words, [0x810e, 0x8157, 0x8159, 0x810e]);
    let notes = h.diagnostics();
    assert_eq!(
        notes.iter().filter(|n| n.contains("weapon code")).count(),
        1
    );
    // Art that cannot be loaded refuses the whole sequence.
    let mut h = harness(true);
    h.host.assets.art_policy = crate::test_support::ArtPolicy::Loads;
    h.run(&hex(VECTOR_L)).unwrap();
    assert!(h.diagnostics().iter().any(|n| n.contains("refused")));
}

#[test]
fn spec017_vector_m_lifecycle_names() {
    assert_eq!(ScriptEvent::Push.name(), b"push_p_proc");
    assert_eq!(
        [
            ScriptEvent::Push,
            ScriptEvent::IsDropping,
            ScriptEvent::CombatIsStarting,
            ScriptEvent::CombatIsOver,
        ]
        .map(ScriptEvent::code),
        [24, 25, 26, 27]
    );
    for fallout2 in [true, false] {
        let mut h = harness(fallout2);
        h.host.world.gvars = Some(vec![0; 8]);
        h.host.world.party.insert(h.me);
        // Push_P_Proc stores script_action in global 5 and overrides.
        let body = [
            fixture::push(5),
            fixture::instruction(0x80c7),
            fixture::instruction(0x80c6),
            fixture::instruction(0x80b9),
        ]
        .concat();
        let program = program(&[(b"start", Vec::new()), (b"Push_P_Proc", body)]);
        let mut vm = Vm::new(program);
        vm.id = 60;
        h.host.bind_program(&vm, h.script);
        h.host.programs.insert(vm.id, vm);
        let overridden = h.host.request_push(h.me).unwrap();
        assert_eq!(overridden, fallout2);
        assert_eq!(
            crate::test_support::gvar(&h.host, 5),
            if fallout2 { 24 } else { 0 }
        );
        let pushes = requested(&h.host, ScriptEvent::Push);
        assert_eq!(pushes.len(), usize::from(fallout2));
    }
}

#[test]
fn spec017_the_gate_is_open_and_fallout2_requests_run_their_scripts() {
    // A script using a Fallout 2 word: Fallout 1 refuses it at load; the
    // Fallout 2 profile parses it with the Fallout 2 word set.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("scripts/scripts.lst"),
        b"words2.int # local_vars=0\n",
    )
    .unwrap();
    let body = [
        fixture::instruction(0x80bc),
        fixture::push(13),
        fixture::push(0),
        fixture::instruction(0x8156),
        fixture::instruction(0x801a),
    ]
    .concat();
    let image = fixture::program(&[], &[(b"start".as_slice(), fixture::returning(body))]);
    std::fs::write(dir.path().join("scripts/words2.int"), image).unwrap();
    let assets = dir_assets(dir.path());
    use crate::assets::WorldAssets;
    assert!(assets.program(0).is_err());
    assets.set_game_profile(crate::GameProfile::Fallout2);
    let (_, program) = assets.program(0).unwrap();
    assert!(program.words().fallout2());
    assets.set_game_profile(crate::GameProfile::Fallout1);
    assert!(assets.program(0).is_err());
    // Revision 3 opens the gate: a Fallout 2 game is not gated, and runs
    // the program's words with the Fallout 2 rows.
    let mut h = harness(true);
    assert!(!h.host.scripts_gated());
    assert_eq!(h.host.word_set(), WordSet::FALLOUT2);
    let mut vm = Vm::new(program);
    vm.id = 70;
    h.host.bind_program(&vm, h.script);
    for _ in 0..100 {
        if vm.run(&mut h.host, 1000).unwrap() != RunOutcome::Budget {
            break;
        }
    }
    assert!(vm.last_error.is_none(), "{:?}", vm.last_error);
    // Under Fallout 1 the host has no row for the word.
    let mut f1 = harness(false);
    assert_eq!(f1.host.word_set(), WordSet::FALLOUT1);
    let mut vm = Vm::fragment(
        IntProgram::parse_in(&fixture::program(&[], &[]), &WordSet::FALLOUT2, false).unwrap(),
        &[
            fixture::push(0),
            fixture::push(13),
            fixture::push(0),
            fixture::instruction(0x8156),
        ]
        .concat(),
    )
    .unwrap();
    assert_eq!(vm.run(&mut f1.host, 100), Err(VmError::Opcode(0x8156)));
}

#[test]
fn spec017_party_add_keeps_control_records_and_a_32_entry_cap() {
    let mut h = harness(true);
    let player = h.host.world.player;
    h.host.world.party.insert(player);
    let add = |who: ObjectRef| [fixture::push(who as i32), fixture::instruction(0x8124)].concat();
    let remove =
        |who: ObjectRef| [fixture::push(who as i32), fixture::instruction(0x8125)].concat();
    h.run(&add(h.me)).unwrap();
    assert!(h.host.world.party.contains(&h.me));
    h.run(&remove(h.me)).unwrap();
    assert!(!h.host.world.party.contains(&h.me));
    for hex_number in 0..31 {
        let critter = h
            .host
            .world
            .spawn(npc_prototype(), 100 + hex_number, 0)
            .unwrap();
        h.run(&add(critter)).unwrap();
    }
    assert_eq!(h.host.world.party.len(), 32);
    h.diagnostics();
    h.run(&add(h.me)).unwrap();
    assert!(!h.host.world.party.contains(&h.me));
    assert!(h.diagnostics().iter().any(|n| n.contains("32 entries")));
}

/// Revision 3 (Decision 37): under Fallout 2, `0x8109` pops an unchecked
/// object, the integer 13 and an unchecked value, pushes I 0, changes
/// nothing, and gives one diagnostic per program.
#[test]
fn spec017_0x8109_pops_three_and_pushes_0_once_diagnosed() {
    let mut h = harness(true);
    let call = [
        fixture::instruction(0x80bc),
        fixture::push(13),
        fixture::push(4),
        fixture::instruction(0x8109),
    ]
    .concat();
    let ticks = h.host.world.clock.ticks;
    let vm = h.run(&[call.clone(), call.clone()].concat()).unwrap();
    assert_eq!(integers(&vm), [0, 0]);
    assert_eq!(h.host.world.clock.ticks, ticks);
    let notes = h.diagnostics();
    let reached: Vec<_> = notes.iter().filter(|n| n.contains("0x8109")).collect();
    assert_eq!(reached.len(), 1, "{notes:?}");
    // The same program again: no second diagnostic.
    h.run(&call).unwrap();
    assert!(!h.diagnostics().iter().any(|n| n.contains("0x8109")));
    // Fallout 1's town_map pops nothing.
    let mut f1 = harness(false);
    let vm = f1
        .run(&[fixture::push(7), fixture::instruction(0x8109)].concat())
        .unwrap();
    assert_eq!(integers(&vm), [7]);
}
