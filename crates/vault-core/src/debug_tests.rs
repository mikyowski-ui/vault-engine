//! Debugger commands through `WorldHost::apply_debug`.
use crate::{
    CharacterSheet, ScriptEvent, WorldHost, WorldState,
    assets::VfsWorldAssets,
    debug::{DebugCommand, MAX_SPAWN, MAX_VM_STEP},
    fixture_state as fixture,
    host_choreography::Playback,
    queue::EventKind,
    routing::step,
    test_support::{TestAssets, count, dir_assets, patched},
};
use std::{
    fs,
    sync::{Arc, Mutex},
};
use vault_fixtures::campaign::{self, Edition};
use vault_formats::{
    FrmArchive,
    map::{ObjectDetail, ObjectState},
    pro::{BasicFlags, LockFlags},
};
use vault_vm::{Vm, VmStatus};

const ITEM_PID: u32 = 0x0000_0001;
const NPC_PID: u32 = 0x0100_0001;
const SCENERY_PID: u32 = 0x0200_0001;
/// SPEC-004 §3.7 event numbers.
const USE: i32 = 6;
const TALK: i32 = 11;
const DAMAGE: i32 = 14;
const MAP_ENTER: i32 = 15;
const DESTROY: i32 = 18;

fn host() -> (tempfile::TempDir, WorldHost<VfsWorldAssets>) {
    host_with(None)
}
/// The NPC's script file is `program`, if there is one.
fn host_with(program: Option<Vec<u8>>) -> (tempfile::TempDir, WorldHost<VfsWorldAssets>) {
    let dir = tempfile::tempdir().unwrap();
    fixture::write_corpus(dir.path());
    if let Some(program) = program {
        fs::write(dir.path().join("scripts/synthetic.int"), program).unwrap();
    }
    // `map.msg` lists the fixture map as number 7.
    fs::copy(
        dir.path().join("maps/synthetic.map"),
        dir.path().join("maps/synth.map"),
    )
    .unwrap();
    let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let mut host = WorldHost::new(world, dir_assets(dir.path()));
    host.load_campaign().unwrap();
    host.load_map("maps/synthetic.map").unwrap();
    (dir, host)
}
/// The map object id of the first object with `pid` (the fixture NPC, a
/// critter with script 0x04000001 and four local variables, is
/// 0x0100_0001).
fn id_of(host: &WorldHost<VfsWorldAssets>, pid: u32) -> u32 {
    host.world
        .iter()
        .find(|(_, object)| object.raw.pid == pid)
        .map(|(_, object)| object.raw.id)
        .unwrap()
}
fn npc(host: &WorldHost<VfsWorldAssets>) -> crate::ObjectRef {
    host.world.reference_for_id(id_of(host, NPC_PID)).unwrap()
}
fn gvars(host: &WorldHost<VfsWorldAssets>) -> Vec<i32> {
    host.world.gvars.clone().unwrap()
}
/// `use_p_proc`: a critical section counting gvar 0. Every other procedure
/// counts its own global.
fn counting_program() -> Vec<u8> {
    let counted = |global| fixture::returning(count(global));
    let critical = [
        fixture::instruction(0x8002),
        count(0),
        fixture::instruction(0x8003),
    ]
    .concat();
    fixture::program(
        &[],
        &[
            (b"start", fixture::returning(Vec::new())),
            (b"use_p_proc", fixture::returning(critical)),
            (b"talk_p_proc", counted(1)),
            (b"damage_p_proc", counted(2)),
            (b"map_enter_p_proc", counted(3)),
            (b"destroy_p_proc", counted(4)),
            (b"timed_event_p_proc", counted(5)),
        ],
    )
}
/// The fixture with [`counting_program`] on the NPC and eight globals at 0.
fn counting() -> (tempfile::TempDir, WorldHost<VfsWorldAssets>) {
    let (dir, mut host) = host_with(Some(counting_program()));
    host.world.gvars = Some(vec![0; 8]);
    (dir, host)
}
/// The Fallout 1 synthetic campaign's first map.
fn campaign_host() -> (tempfile::TempDir, WorldHost<VfsWorldAssets>) {
    let dir = tempfile::tempdir().unwrap();
    fixture::write_corpus(dir.path());
    campaign::write_campaign(dir.path(), Edition::Fallout1);
    let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let mut host = WorldHost::new(world, dir_assets(dir.path()));
    host.load_campaign().unwrap();
    host.load_map(&campaign::start(Edition::Fallout1)).unwrap();
    (dir, host)
}
fn refused<A: crate::assets::WorldAssets>(host: &mut WorldHost<A>, commands: &[DebugCommand]) {
    let before = host.world.state_hash();
    for command in commands {
        assert!(host.apply_debug(command).is_err(), "{command}");
        assert_eq!(host.world.state_hash(), before, "{command}");
    }
}
/// Run the stepped procedure to its end, one instruction at a time.
fn step_to_end<A: crate::assets::WorldAssets>(host: &mut WorldHost<A>) -> u32 {
    let mut steps = 0;
    while host.debug_vm.is_some() {
        host.apply_debug(&DebugCommand::VmStep { count: 1 })
            .unwrap();
        steps += 1;
        assert!(steps < 1000, "the procedure never ended");
    }
    steps
}

#[test]
fn variable_commands_use_the_script_stores_and_refuse_bad_indices() {
    let (_dir, mut host) = host();
    let count = host.world.gvars.as_ref().unwrap().len();
    host.apply_debug(&DebugCommand::SetGlobalVar {
        index: count - 1,
        value: 77,
    })
    .unwrap();
    assert_eq!(host.world.gvars.as_ref().unwrap()[count - 1], 77);
    let before = host.world.gvars.clone();
    assert!(
        host.apply_debug(&DebugCommand::SetGlobalVar {
            index: count,
            value: 1
        })
        .is_err()
    );
    assert!(
        host.apply_debug(&DebugCommand::SetGlobalVar {
            index: usize::MAX,
            value: 1
        })
        .is_err()
    );
    assert_eq!(host.world.gvars, before);

    if host.world.mvars.is_empty() {
        host.world.mvars.push(0);
    }
    host.apply_debug(&DebugCommand::SetMapVar {
        index: 0,
        value: -5,
    })
    .unwrap();
    assert_eq!(host.world.mvars[0], -5);
    let index = host.world.mvars.len();
    assert!(
        host.apply_debug(&DebugCommand::SetMapVar { index, value: 1 })
            .is_err()
    );

    // LVARs through the object's own script slice, by map object id.
    let npc_id = id_of(&host, 0x0100_0001);
    host.apply_debug(&DebugCommand::SetObjectLocalVar {
        object: npc_id,
        index: 3,
        value: 42,
    })
    .unwrap();
    let sid = host.world.get(npc(&host)).unwrap().raw.script_id as u32;
    assert_eq!(host.world.local(Some(sid), 3), 42);
    for command in [
        DebugCommand::SetObjectLocalVar {
            object: npc_id,
            index: 4,
            value: 1,
        },
        DebugCommand::SetObjectLocalVar {
            object: npc_id,
            index: usize::MAX,
            value: 1,
        },
        // The player has no object script; an unknown id has no object.
        DebugCommand::SetObjectLocalVar {
            object: host.world.get(host.world.player).unwrap().raw.id,
            index: 0,
            value: 1,
        },
        DebugCommand::SetObjectLocalVar {
            object: 0xdead,
            index: 0,
            value: 1,
        },
    ] {
        assert!(host.apply_debug(&command).is_err(), "{command}");
    }
    assert_eq!(host.world.local(Some(sid), 3), 42);
}

#[test]
fn teleport_requests_an_ordinary_transition() {
    let (_dir, mut host) = host();
    for bad in [
        (0, 0, None),
        (66, 0, None),
        (-1, 0, None),
        (7, 3, None),
        (7, 0, Some(40000)),
        (7, 0, Some(0)),
        // Listed in the table, but not in this corpus's `map.msg`.
        (8, 0, None),
    ] {
        let command = DebugCommand::TeleportMap {
            map_number: bad.0,
            elevation: bad.1,
            tile: bad.2,
        };
        assert!(host.apply_debug(&command).is_err(), "{command}");
        assert!(host.world.pending_transition.is_none());
    }
    host.apply_debug(&DebugCommand::TeleportMap {
        map_number: 7,
        elevation: 0,
        tile: Some(20500),
    })
    .unwrap();
    let pending = host.world.pending_transition.unwrap();
    assert_eq!(
        (pending.map, pending.elevation, pending.hex, pending.facing),
        (7, 0, 20500, -1)
    );
    assert!(host.process_transition().unwrap());
    assert_eq!(
        host.world.persistence.current_map.as_deref(),
        Some("SYNTH.SAV")
    );
    let player = host.world.get(host.world.player).unwrap();
    assert_eq!(player.raw.hex, Some(20500));
    // Without a tile the map's own entrance applies.
    host.apply_debug(&DebugCommand::TeleportMap {
        map_number: 7,
        elevation: 2,
        tile: None,
    })
    .unwrap();
    let pending = host.world.pending_transition.unwrap();
    assert_eq!((pending.elevation, pending.hex), (-1, -1));
}

#[test]
fn spawned_items_join_the_player_inventory_and_heals_restore_critters() {
    let (_dir, mut host) = host();
    let player = host.world.player;
    let before = host.world.inventory_count(player, ITEM_PID);
    host.apply_debug(&DebugCommand::SpawnItem {
        pid: ITEM_PID,
        quantity: 3,
    })
    .unwrap();
    assert_eq!(host.world.inventory_count(player, ITEM_PID), before + 3);
    let objects = host.world.iter().count();
    for command in [
        DebugCommand::SpawnItem {
            pid: ITEM_PID,
            quantity: 0,
        },
        DebugCommand::SpawnItem {
            pid: ITEM_PID,
            quantity: crate::debug::MAX_SPAWN + 1,
        },
        // A critter PID is not an item; a missing prototype fails cleanly.
        DebugCommand::SpawnItem {
            pid: 0x0100_0001,
            quantity: 1,
        },
        DebugCommand::SpawnItem {
            pid: 0x0000_0999,
            quantity: 1,
        },
    ] {
        assert!(host.apply_debug(&command).is_err(), "{command}");
    }
    assert_eq!(host.world.iter().count(), objects, "nothing left behind");

    // Hurt and cripple the NPC, then heal it fully.
    let npc_id = id_of(&host, 0x0100_0001);
    let target = npc(&host);
    let max = host.world.effective(target, 7);
    let object = host.world.get_mut(target).unwrap();
    object.character.as_mut().unwrap().current_hp = 1;
    if let ObjectState::Critter(state) = &mut object.raw.state {
        state.hp = 1;
        state.results.0 |= 0x40 | 0x04;
    }
    host.apply_debug(&DebugCommand::HealCritter { object: npc_id })
        .unwrap();
    assert_eq!(host.world.hit_points(target), Some(max));
    let state = host.world.critter_state(target).unwrap();
    assert_eq!((state.hp, state.results.0 & 0x7c), (max, 0));
    // Not a critter, or no such object.
    let scenery = id_of(&host, 0x0200_0001);
    assert!(
        host.apply_debug(&DebugCommand::HealCritter { object: scenery })
            .is_err()
    );
    assert!(
        host.apply_debug(&DebugCommand::HealCritter { object: 0xdead })
            .is_err()
    );
}

#[test]
fn commands_serialize_as_tagged_records() {
    let command = DebugCommand::TeleportMap {
        map_number: 7,
        elevation: 1,
        tile: None,
    };
    let text = serde_json::to_string(&command).unwrap();
    assert_eq!(
        text,
        r#"{"op":"teleport_map","map_number":7,"elevation":1,"tile":null}"#
    );
    assert_eq!(
        serde_json::from_str::<DebugCommand>(&text).unwrap(),
        command
    );
    assert_eq!(
        serde_json::to_string(&DebugCommand::HealCritter { object: 2 }).unwrap(),
        r#"{"op":"heal_critter","object":2}"#
    );
}

/// Four frames per direction, each moving one pixel right.
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

#[test]
fn doors_change_at_once_and_routes_follow() {
    let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let mut host = WorldHost::new(world, TestAssets::default());
    host.assets.art.insert(0x0200_0000, door_art());
    let player = host.world.player;
    let goal = (0..4).fold(20100, |hex, _| step(hex, 1));
    for direction in [0, 1, 2, 3, 5] {
        let rock = patched(2, 5, &[(0x18, 0x800)], &[]);
        let rock = host.world.spawn(rock, step(goal, direction), 0).unwrap();
        host.world.get_mut(rock).unwrap().raw.flags = BasicFlags(0);
    }
    // The only way to the goal is through this door.
    let prototype = patched(2, 0, &[(0x18, 0x800)], &[(0x28, b'A')]);
    let door = host.world.spawn(prototype, step(goal, 4), 0).unwrap();
    let object = host.world.get_mut(door).unwrap();
    object.raw.flags = BasicFlags(0);
    object.raw.state = ObjectState::Other {
        word: 0,
        detail: ObjectDetail::Door(LockFlags(0)),
    };
    let id = object.raw.id;
    let reach =
        |host: &WorldHost<TestAssets>| !host.world.route(player, 20100, goal, false).is_empty();
    let set = |open, locked, jammed| DebugCommand::SetDoor {
        object: id,
        open,
        locked,
        jammed,
    };
    assert!(!reach(&host));

    host.apply_debug(&set(Some(true), None, None)).unwrap();
    let object = host.world.get(door).unwrap();
    assert!(object.lock().unwrap().contains(LockFlags::OPEN));
    assert_eq!(object.raw.frame, 3);
    assert_eq!(object.raw.displacement, [3, 0]);
    assert_eq!(object.raw.flags.0 & 0xA000_0010, 0xA000_0010);
    assert!(reach(&host));

    // The locks change alone, and opening twice moves nothing.
    host.apply_debug(&set(Some(true), Some(true), Some(true)))
        .unwrap();
    let object = host.world.get(door).unwrap();
    let lock = object.lock().unwrap();
    assert!(lock.contains(LockFlags::LOCKED) && lock.contains(LockFlags::JAMMED));
    assert_eq!(object.raw.displacement, [3, 0]);
    host.apply_debug(&set(None, Some(false), None)).unwrap();
    let lock = host.world.get(door).unwrap().lock().unwrap();
    assert!(!lock.contains(LockFlags::LOCKED) && lock.contains(LockFlags::JAMMED));

    host.apply_debug(&set(Some(false), None, Some(false)))
        .unwrap();
    let object = host.world.get(door).unwrap();
    assert!(!object.lock().unwrap().contains(LockFlags::OPEN));
    assert!(!object.lock().unwrap().contains(LockFlags::JAMMED));
    assert_eq!(object.raw.frame, 0);
    assert_eq!(object.raw.displacement, [0, 0]);
    assert_eq!(object.raw.flags.0 & 0xA000_0010, 0);
    assert!(!reach(&host));
}

#[test]
fn a_door_swing_under_way_cannot_undo_a_set_door() {
    let (_dir, mut host) = campaign_host();
    host.playback = Playback::RealTime;
    let door = host.world.reference_for_id(campaign::PLAIN_DOOR).unwrap();
    host.script_open(door, true);
    assert!(host.moving(door));
    host.apply_debug(&DebugCommand::SetDoor {
        object: campaign::PLAIN_DOOR,
        open: Some(false),
        locked: None,
        jammed: None,
    })
    .unwrap();
    assert!(!host.moving(door));
    let player = host.world.player;
    assert_eq!(host.world.movement_blocker(20301, 0, player), Some(door));
}

#[test]
fn doors_refuse_anything_else() {
    let (_dir, mut host) = campaign_host();
    let set = |object| DebugCommand::SetDoor {
        object,
        open: Some(true),
        locked: Some(true),
        jammed: Some(true),
    };
    // A container has a lock too, but it is not a door.
    refused(
        &mut host,
        &[
            set(campaign::FRIEND),
            set(campaign::CONTAINER),
            set(campaign::PISTOL),
            set(0xdead),
        ],
    );
}

#[test]
fn hit_and_action_points_stay_in_bounds() {
    let (_dir, mut host) = host();
    let id = id_of(&host, NPC_PID);
    let target = npc(&host);
    let max = host.world.max_hit_points(target);
    host.apply_debug(&DebugCommand::SetHitPoints {
        object: id,
        value: 3,
    })
    .unwrap();
    assert_eq!(host.world.hit_points(target), Some(3));
    assert_eq!(host.world.critter_state(target).unwrap().hp, 3);
    assert_eq!(
        host.world
            .get(target)
            .unwrap()
            .character
            .as_ref()
            .unwrap()
            .current_hp,
        3
    );
    let top = host.world.effective(target, 8);
    assert!(top > 0);
    host.apply_debug(&DebugCommand::SetActionPoints {
        object: id,
        value: top,
    })
    .unwrap();
    assert_eq!(host.world.action_points(target), top);

    let scenery = id_of(&host, SCENERY_PID);
    let hp = |object, value| DebugCommand::SetHitPoints { object, value };
    let ap = |object, value| DebugCommand::SetActionPoints { object, value };
    refused(
        &mut host,
        &[
            hp(id, -1),
            hp(id, max + 1),
            hp(scenery, 1),
            hp(0xdead, 1),
            ap(id, -1),
            ap(id, top + 1),
            ap(scenery, 1),
            ap(0xdead, 1),
        ],
    );
    // The dead stay dead.
    if let ObjectState::Critter(state) = &mut host.world.get_mut(target).unwrap().raw.state {
        state.results.0 |= 0x80;
    }
    refused(&mut host, &[hp(id, 1), ap(id, 1)]);
    assert_eq!(host.world.hit_points(target), Some(3));
}

/// Zero is the Kill button: the death path, not a critter left at 0 HP.
#[test]
fn zero_hit_points_kill_through_the_death_path() {
    let (_dir, mut host) = host();
    let id = id_of(&host, NPC_PID);
    let target = npc(&host);
    host.apply_debug(&DebugCommand::SetHitPoints {
        object: id,
        value: 0,
    })
    .unwrap();
    assert!(host.world.is_dead(target));
}

#[test]
fn item_counts_follow_add_and_remove() {
    let (_dir, mut host) = campaign_host();
    let player = host.world.player;
    let id = host.world.get(player).unwrap().raw.id;
    let ammunition = 31;
    let set = |owner, pid, count| DebugCommand::SetItemCount { owner, pid, count };
    for count in [7, 2, 2, 0] {
        host.apply_debug(&set(id, ammunition, count)).unwrap();
        assert_eq!(host.world.inventory_count(player, ammunition), count as i32);
    }
    assert!(host.world.carried(player, ammunition).is_none());

    // Removal takes from every stack, and from inside a container.
    let bag = host.world.reference_for_id(campaign::CONTAINER).unwrap();
    host.apply_debug(&set(campaign::CONTAINER, 8, 2)).unwrap();
    assert_eq!(host.world.inventory_count(bag, 8), 2);
    host.apply_debug(&set(id, 8, 3)).unwrap();
    host.world.get_mut(bag).unwrap().raw.hex = None;
    host.world.add_item(player, bag, 1, true).unwrap();
    assert_eq!(host.world.inventory_count(player, 8), 5);
    host.apply_debug(&set(id, 8, 1)).unwrap();
    assert_eq!(host.world.inventory_count(player, 8), 1);

    let objects = host.world.iter().count();
    refused(
        &mut host,
        &[
            set(id, 8, MAX_SPAWN + 1),
            // Not an item, no such prototype, nobody to carry it.
            set(id, NPC_PID, 1),
            set(id, 0x0000_0999, 1),
            set(campaign::PLAIN_DOOR, 8, 1),
            set(0xdead, 8, 1),
        ],
    );
    assert_eq!(host.world.iter().count(), objects);
}

#[test]
fn a_worn_item_comes_off_as_it_goes() {
    let (_dir, mut host) = campaign_host();
    let player = host.world.player;
    let id = host.world.get(player).unwrap().raw.id;
    let pistol = 8;
    let set = |count| DebugCommand::SetItemCount {
        owner: id,
        pid: pistol,
        count,
    };
    host.apply_debug(&set(2)).unwrap();
    let carried = host.world.carried(player, pistol).unwrap();
    host.world.get_mut(carried).unwrap().raw.flags.0 |= BasicFlags::WORN.0;
    host.apply_debug(&set(1)).unwrap();
    assert_eq!(host.world.inventory_count(player, pistol), 1);
    let left = host.world.carried(player, pistol).unwrap();
    assert!(
        !host
            .world
            .get(left)
            .unwrap()
            .raw
            .flags
            .contains(BasicFlags::WORN)
    );
}

#[test]
fn every_named_procedure_fires_once_through_the_dispatcher() {
    let (_dir, mut host) = counting();
    let id = id_of(&host, NPC_PID);
    for (event, global) in [
        (USE, 0),
        (TALK, 1),
        (DAMAGE, 2),
        (MAP_ENTER, 3),
        (DESTROY, 4),
    ] {
        host.apply_debug(&DebugCommand::FireProc {
            object: id,
            event,
            fixed: 9,
        })
        .unwrap();
        assert_eq!(gvars(&host)[global], 1, "event {event}");
        let sid = host.world.script_for(npc(&host)).unwrap();
        let script = host.world.scripts.get(&sid).unwrap();
        assert_eq!((script.event, script.fixed, script.source), (event, 9, 0));
    }
    // A talk that opened no dialogue left no session behind.
    assert!(!host.world.dialogue.talking);
    assert!(host.world.background_enabled);
}

#[test]
fn a_script_that_never_ran_is_loaded_before_its_procedure_fires() {
    let (_dir, mut host) = counting();
    let scenery = id_of(&host, SCENERY_PID);
    let reference = host.world.reference_for_id(scenery).unwrap();
    let sid = host.world.add_script(reference, 0, 3, 4).unwrap();
    assert!(!host.world.scripts.get(&sid).unwrap().initialized);
    // It cannot be stepped until it has run.
    refused(
        &mut host,
        &[DebugCommand::VmBegin {
            object: scenery,
            event: USE,
        }],
    );
    assert!(host.debug_vm.is_none());
    assert!(!host.debug_paused);
    host.apply_debug(&DebugCommand::FireProc {
        object: scenery,
        event: USE,
        fixed: 0,
    })
    .unwrap();
    assert_eq!(gvars(&host)[0], 1);
}

#[test]
fn firing_refuses_what_the_dispatcher_would_not_run() {
    // This script defines `start` alone.
    let start = fixture::program(&[], &[(b"start", fixture::returning(Vec::new()))]);
    let (_dir, mut host) = host_with(Some(start));
    let id = id_of(&host, NPC_PID);
    let scenery = id_of(&host, SCENERY_PID);
    let fire = |object, event| DebugCommand::FireProc {
        object,
        event,
        fixed: 0,
    };
    let begin = |object, event| DebugCommand::VmBegin { object, event };
    refused(
        &mut host,
        &[
            fire(id, USE),
            begin(id, USE),
            fire(id, 1),
            fire(id, 0),
            begin(id, -1),
            fire(scenery, USE),
            begin(scenery, USE),
            fire(0xdead, USE),
        ],
    );
    let (_dir, mut host) = counting();
    let id = id_of(&host, NPC_PID);
    host.world.scripts_enabled = false;
    refused(&mut host, &[fire(id, USE), begin(id, USE)]);
    host.world.scripts_enabled = true;
    host.world.combat = true;
    refused(&mut host, &[fire(id, TALK), begin(id, TALK)]);
    assert!(!host.world.dialogue.talking);
    host.world.combat = false;
    let sid = host.world.script_for(npc(&host)).unwrap();
    let program = host.world.scripts.get(&sid).unwrap().program;
    host.programs.get_mut(&program).unwrap().status.0 |= VmStatus::ERROR;
    refused(&mut host, &[fire(id, USE), begin(id, USE)]);
    assert_eq!(gvars(&host), vec![0; 8]);
}

#[test]
fn firing_runs_a_door_script_that_destroys_its_door() {
    let (_dir, mut host) = campaign_host();
    host.apply_debug(&DebugCommand::FireProc {
        object: campaign::SCRIPTED_DOOR,
        event: DAMAGE,
        fixed: 0,
    })
    .unwrap();
    assert!(
        host.world
            .reference_for_id(campaign::SCRIPTED_DOOR)
            .is_none()
    );
}

#[test]
fn fast_forward_fires_the_next_event_through_advance_time() {
    let (_dir, mut host) = counting();
    let sid = host.world.script_for(npc(&host)).unwrap();
    host.world.queue.retain(|_| false);
    refused(&mut host, &[DebugCommand::FastForwardTimer]);
    let start = host.world.clock.ticks;
    let kind = EventKind::Script {
        script: sid,
        parameter: 9,
    };
    host.world.queue.insert(start, 5000, 0, kind).unwrap();
    for frozen in [true, false] {
        host.world.combat = frozen;
        host.world.dialogue.open = !frozen;
        refused(&mut host, &[DebugCommand::FastForwardTimer]);
        assert_eq!(host.world.clock.ticks, start);
        assert_eq!(host.world.queue.events().count(), 1);
    }
    host.world.combat = false;
    host.world.dialogue.open = false;
    host.apply_debug(&DebugCommand::FastForwardTimer).unwrap();
    assert_eq!(host.world.clock.ticks, start + 5000);
    assert_eq!(gvars(&host)[5], 1);
    assert_eq!(host.world.scripts.get(&sid).unwrap().fixed, 9);
    assert_eq!(host.world.queue.events().count(), 0);

    // Past what the 32-bit clock reaches.
    let kind = EventKind::Other(7);
    host.world
        .queue
        .insert(host.world.clock.ticks, 1 << 32, 0, kind)
        .unwrap();
    let now = host.world.clock.ticks;
    refused(&mut host, &[DebugCommand::FastForwardTimer]);
    assert_eq!(host.world.clock.ticks, now);
}

#[test]
fn teleports_stay_on_the_map_and_keep_the_id() {
    let (_dir, mut host) = campaign_host();
    let friend = host.world.reference_for_id(campaign::FRIEND).unwrap();
    let move_to = |object, hex, elevation| DebugCommand::TeleportObject {
        object,
        hex,
        elevation,
    };
    host.apply_debug(&move_to(campaign::FRIEND, 21000, 1))
        .unwrap();
    let object = host.world.get(friend).unwrap();
    assert_eq!((object.raw.hex, object.raw.elevation), (Some(21000), 1));
    assert_eq!(host.world.reference_for_id(campaign::FRIEND), Some(friend));
    assert_eq!(host.world.movement_blocker(21000, 1, 0), Some(friend));
    assert_eq!(host.world.movement_blocker(20099, 0, 0), None);

    let pistol = host.world.reference_for_id(campaign::PISTOL).unwrap();
    host.apply_debug(&move_to(campaign::PISTOL, 21002, 0))
        .unwrap();
    assert_eq!(host.world.get(pistol).unwrap().raw.hex, Some(21002));

    let player = host.world.player;
    let id = host.world.get(player).unwrap().raw.id;
    host.apply_debug(&move_to(id, 21004, 0)).unwrap();
    assert_eq!(host.world.get(player).unwrap().raw.hex, Some(21004));

    host.world.get_mut(pistol).unwrap().raw.hex = None;
    host.world.add_item(player, pistol, 1, true).unwrap();
    refused(
        &mut host,
        &[
            move_to(campaign::FOE, 0, 0),
            move_to(campaign::FOE, 40000, 0),
            move_to(campaign::FOE, u32::MAX, 0),
            move_to(campaign::FOE, 21000, 3),
            // Scenery stays, a carried item has no hex, and a critter cannot
            // share a hex.
            move_to(campaign::PLAIN_DOOR, 21006, 0),
            move_to(campaign::PISTOL, 21006, 0),
            move_to(campaign::FOE, 21004, 0),
            move_to(0xdead, 21006, 0),
        ],
    );
}

#[test]
fn a_teleport_stops_a_walk_under_way() {
    let (_dir, mut host) = campaign_host();
    host.playback = Playback::RealTime;
    let foe = host.world.reference_for_id(campaign::FOE).unwrap();
    assert!(host.walk_toward(foe, 20904, false, 0).unwrap());
    assert!(host.moving(foe));
    host.apply_debug(&DebugCommand::TeleportObject {
        object: campaign::FOE,
        hex: 21000,
        elevation: 0,
    })
    .unwrap();
    assert!(!host.moving(foe));
}

#[test]
fn spawned_objects_get_fresh_ids_on_their_hex() {
    let (_dir, mut host) = host();
    let spawn = |pid, hex, elevation| DebugCommand::SpawnObject {
        pid,
        hex,
        elevation,
    };
    let before: Vec<u32> = host.world.iter().map(|(_, o)| o.raw.id).collect();
    host.apply_debug(&spawn(NPC_PID, 21000, 1)).unwrap();
    host.apply_debug(&spawn(ITEM_PID, 21000, 1)).unwrap();
    host.apply_debug(&spawn(SCENERY_PID, 21002, 2)).unwrap();
    let fresh: Vec<_> = host
        .world
        .iter()
        .filter(|(_, o)| !before.contains(&o.raw.id))
        .map(|(reference, object)| (reference, object.raw.id, object.raw.pid))
        .collect();
    assert_eq!(fresh.len(), 3);
    for (reference, id, pid) in fresh {
        assert_ne!(id, 0);
        assert_eq!(host.world.reference_for_id(id), Some(reference));
        let object = host.world.get(reference).unwrap();
        assert_eq!(object.raw.pid, pid);
        assert_eq!(object.owner, None);
    }
    let objects = host.world.iter().count();
    refused(
        &mut host,
        &[
            spawn(ITEM_PID, 0, 0),
            spawn(ITEM_PID, 40000, 0),
            spawn(ITEM_PID, 21000, 3),
            // A tile is no object, the prototype is missing, and the critter
            // already there blocks another.
            spawn(0x0400_0001, 21000, 0),
            spawn(0x0000_0999, 21000, 0),
            spawn(NPC_PID, 21000, 1),
        ],
    );
    assert_eq!(host.world.iter().count(), objects);
}

#[test]
fn stepping_stops_between_instructions_inside_a_critical_section() {
    let (_dir, mut host) = counting();
    let id = id_of(&host, NPC_PID);
    let begin = DebugCommand::VmBegin {
        object: id,
        event: USE,
    };
    let one = DebugCommand::VmStep { count: 1 };
    host.apply_debug(&begin).unwrap();
    assert!(host.debug_paused);
    let vm = |host: &WorldHost<VfsWorldAssets>| host.debug_vm.clone().unwrap();
    // A procedure's own frame comes first, then 0x8002.
    host.apply_debug(&one).unwrap();
    assert!(!vm(&host).status.contains(VmStatus::CRITICAL));
    host.apply_debug(&one).unwrap();
    assert!(vm(&host).status.contains(VmStatus::CRITICAL));
    // A push is six bytes; `Vm::run` would have run to the section's end.
    let pc = vm(&host).pc;
    host.apply_debug(&one).unwrap();
    assert_eq!(vm(&host).pc, pc + 6);
    assert!(vm(&host).status.contains(VmStatus::CRITICAL));
    assert_eq!(gvars(&host)[0], 0);
    assert!(host.debug_paused);

    step_to_end(&mut host);
    assert!(!host.debug_paused);
    assert_eq!(gvars(&host)[0], 1);

    // The whole run ends where the dispatcher's own run ends.
    let (_dir, mut fired) = counting();
    fired
        .apply_debug(&DebugCommand::FireProc {
            object: id,
            event: USE,
            fixed: 0,
        })
        .unwrap();
    assert_eq!(fired.world.state_hash(), host.world.state_hash());
    let program = |host: &WorldHost<VfsWorldAssets>| {
        let sid = host.world.script_for(npc(host)).unwrap();
        let vm: Vm = host.programs[&host.world.scripts.get(&sid).unwrap().program].clone();
        (
            vm.pc,
            vm.status,
            vm.value_stack.byte_len(),
            vm.return_stack.byte_len(),
        )
    };
    assert_eq!(program(&fired), program(&host));
}

#[test]
fn a_stepped_procedure_holds_the_world() {
    let (_dir, mut host) = counting();
    let id = id_of(&host, NPC_PID);
    let begin = DebugCommand::VmBegin {
        object: id,
        event: USE,
    };
    host.apply_debug(&begin).unwrap();
    refused(
        &mut host,
        &[
            DebugCommand::SetPaused { on: false },
            begin.clone(),
            DebugCommand::FireProc {
                object: id,
                event: TALK,
                fixed: 0,
            },
            DebugCommand::FastForwardTimer,
            DebugCommand::SetGlobalVar { index: 0, value: 1 },
            DebugCommand::SetHitPoints {
                object: id,
                value: 1,
            },
            DebugCommand::VmStep { count: 0 },
            DebugCommand::VmStep {
                count: MAX_VM_STEP + 1,
            },
        ],
    );
    assert!(host.debug_paused);
    assert!(host.debug_vm.is_some());
    host.apply_debug(&DebugCommand::SetPaused { on: true })
        .unwrap();
    assert!(host.debug_vm.is_some());
    // What was refused left the stepped run unharmed.
    step_to_end(&mut host);
    assert_eq!(gvars(&host), [1, 0, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn abort_leaves_the_program_as_it_was() {
    let (_dir, mut host) = counting();
    let id = id_of(&host, NPC_PID);
    let sid = host.world.script_for(npc(&host)).unwrap();
    let program = host.world.scripts.get(&sid).unwrap().program;
    let state = |vm: &Vm| {
        (
            vm.pc,
            vm.status,
            vm.value_stack.byte_len(),
            vm.return_stack.byte_len(),
        )
    };
    let before = state(&host.programs[&program]);
    host.apply_debug(&DebugCommand::VmBegin {
        object: id,
        event: USE,
    })
    .unwrap();
    // Inside the critical section, with values on the stack.
    host.apply_debug(&DebugCommand::VmStep { count: 3 })
        .unwrap();
    assert!(
        host.debug_vm
            .as_ref()
            .unwrap()
            .status
            .contains(VmStatus::CRITICAL)
    );
    host.apply_debug(&DebugCommand::VmAbort).unwrap();
    assert!(host.debug_vm.is_none());
    assert!(!host.debug_paused);
    assert_eq!(state(&host.programs[&program]), before);
    assert_eq!(host.world.scripts.get(&sid).unwrap().source, 0);

    // Nothing to abort or step now, and the program still runs.
    refused(
        &mut host,
        &[DebugCommand::VmAbort, DebugCommand::VmStep { count: 1 }],
    );
    host.apply_debug(&DebugCommand::FireProc {
        object: id,
        event: USE,
        fixed: 0,
    })
    .unwrap();
    assert_eq!(gvars(&host)[0], 1);
}

#[test]
fn a_fault_ends_the_session_as_it_ends_a_run() {
    let divide = [
        fixture::push(1),
        fixture::push(0),
        fixture::instruction(0x803c),
    ]
    .concat();
    let program = fixture::program(
        &[],
        &[
            (b"start", fixture::returning(Vec::new())),
            (b"use_p_proc", fixture::returning(divide)),
        ],
    );
    let (_dir, mut host) = host_with(Some(program));
    let id = id_of(&host, NPC_PID);
    host.apply_debug(&DebugCommand::VmBegin {
        object: id,
        event: USE,
    })
    .unwrap();
    host.world.effects.clear();
    step_to_end(&mut host);
    assert!(!host.debug_paused);
    let sid = host.world.script_for(npc(&host)).unwrap();
    let vm = &host.programs[&host.world.scripts.get(&sid).unwrap().program];
    assert!(vm.status.contains(VmStatus::ERROR));
    let told = |effect: &crate::WorldEffect| matches!(effect, crate::WorldEffect::Diagnostic(text) if text.contains("stepped procedure"));
    assert!(host.world.effects.iter().any(told));
}

#[test]
fn a_stepped_talk_hands_a_blocked_program_back_to_the_world() {
    // The fixture script's `talk_p_proc` opens a dialogue and waits in it.
    let (_dir, mut host) = host();
    host.interactive_dialogue = true;
    let id = id_of(&host, NPC_PID);
    host.apply_debug(&DebugCommand::VmBegin {
        object: id,
        event: TALK,
    })
    .unwrap();
    assert!(host.world.dialogue.talking);
    step_to_end(&mut host);
    assert!(!host.debug_paused);
    assert!(host.world.dialogue.open);
    assert!(host.world.dialogue.talking);
    assert!(!host.world.background_enabled);
    assert_eq!(host.world.dialogue.target, npc(&host));
    let sid = host.world.script_for(npc(&host)).unwrap();
    let vm = &host.programs[&host.world.scripts.get(&sid).unwrap().program];
    assert!(vm.status.contains(VmStatus::ATTACHED));
}

#[test]
fn abort_closes_a_dialogue_the_procedure_opened() {
    let (_dir, mut host) = host();
    host.interactive_dialogue = true;
    let id = id_of(&host, NPC_PID);
    host.apply_debug(&DebugCommand::VmBegin {
        object: id,
        event: TALK,
    })
    .unwrap();
    while !host.world.dialogue.open {
        host.apply_debug(&DebugCommand::VmStep { count: 1 })
            .unwrap();
    }
    host.apply_debug(&DebugCommand::VmAbort).unwrap();
    assert!(!host.world.dialogue.open && !host.world.dialogue.talking);
    assert!(host.world.background_enabled);
}

#[test]
fn a_stepped_procedure_may_destroy_its_own_object() {
    let (_dir, mut host) = campaign_host();
    host.apply_debug(&DebugCommand::VmBegin {
        object: campaign::SCRIPTED_DOOR,
        event: DAMAGE,
    })
    .unwrap();
    step_to_end(&mut host);
    assert!(
        host.world
            .reference_for_id(campaign::SCRIPTED_DOOR)
            .is_none()
    );
    assert!(!host.debug_paused);
    assert!(host.departed.is_empty());
}

#[test]
fn an_attached_observer_sees_every_stepped_instruction() {
    #[derive(Default)]
    struct Counter {
        before: u32,
        after: u32,
    }
    impl crate::VmObserver for Counter {
        fn before_step(&mut self, _: &Vm) {
            self.before += 1;
        }
        fn after_step(&mut self, _: &Vm, _: &Result<vault_vm::RunOutcome, vault_vm::VmError>) {
            self.after += 1;
        }
    }
    let (_dir, mut host) = counting();
    let id = id_of(&host, NPC_PID);
    let counter = Arc::new(Mutex::new(Counter::default()));
    host.vm_observer = Some(counter.clone());
    host.apply_debug(&DebugCommand::VmBegin {
        object: id,
        event: USE,
    })
    .unwrap();
    let steps = step_to_end(&mut host);
    let counter = counter.lock().unwrap();
    assert_eq!((counter.before, counter.after), (steps, steps));
}

#[test]
fn a_logged_session_replays_to_the_same_state_hash() {
    let (_dir, mut live) = campaign_host();
    let (_dir, mut replay) = campaign_host();
    let player = live.world.get(live.world.player).unwrap().raw.id;
    let start = live.world.state_hash();
    assert_eq!(start, replay.world.state_hash());
    let commands = [
        DebugCommand::SetDoor {
            object: campaign::PLAIN_DOOR,
            open: Some(true),
            locked: None,
            jammed: None,
        },
        DebugCommand::SetDoor {
            object: campaign::LOCKED_DOOR,
            open: None,
            locked: Some(false),
            jammed: Some(true),
        },
        DebugCommand::SetHitPoints {
            object: campaign::FOE,
            value: 4,
        },
        DebugCommand::SetActionPoints {
            object: campaign::FOE,
            value: 2,
        },
        // Refused in the log as it was live.
        DebugCommand::SetHitPoints {
            object: campaign::PISTOL,
            value: 4,
        },
        DebugCommand::SetItemCount {
            owner: player,
            pid: 31,
            count: 3,
        },
        DebugCommand::SetItemCount {
            owner: campaign::CONTAINER,
            pid: 8,
            count: 2,
        },
        DebugCommand::TeleportObject {
            object: campaign::FOE,
            hex: 21000,
            elevation: 0,
        },
        DebugCommand::SpawnObject {
            pid: 8,
            hex: 21002,
            elevation: 0,
        },
        DebugCommand::SpawnObject {
            pid: NPC_PID,
            hex: 21004,
            elevation: 0,
        },
        DebugCommand::FastForwardTimer,
        DebugCommand::SetPaused { on: true },
        DebugCommand::SetPaused { on: false },
        DebugCommand::VmBegin {
            object: campaign::SCRIPTED_DOOR,
            event: DAMAGE,
        },
        DebugCommand::VmStep { count: 1 },
        DebugCommand::VmAbort,
        DebugCommand::VmBegin {
            object: campaign::SCRIPTED_DOOR,
            event: DAMAGE,
        },
        DebugCommand::VmStep { count: 100 },
        DebugCommand::FireProc {
            object: campaign::FRIEND,
            event: TALK,
            fixed: 0,
        },
    ];
    let log: Vec<String> = commands
        .iter()
        .map(|command| serde_json::to_string(command).unwrap())
        .collect();
    let lived: Vec<bool> = commands
        .iter()
        .map(|command| live.apply_debug(command).is_ok())
        .collect();
    let replayed: Vec<bool> = log
        .iter()
        .map(|line| {
            let command = serde_json::from_str::<DebugCommand>(line).unwrap();
            replay.apply_debug(&command).is_ok()
        })
        .collect();
    assert_eq!(lived, replayed);
    assert_eq!(lived.iter().filter(|applied| !**applied).count(), 1);
    assert_ne!(live.world.state_hash(), start);
    assert_eq!(live.world.state_hash(), replay.world.state_hash());
    assert_eq!(live.world.clock.ticks, replay.world.clock.ticks);
    assert!(
        live.world
            .reference_for_id(campaign::SCRIPTED_DOOR)
            .is_none()
    );
}

#[test]
fn every_command_names_itself_and_keeps_its_record() {
    let commands = [
        (
            DebugCommand::SetDoor {
                object: 2,
                open: Some(true),
                locked: None,
                jammed: Some(false),
            },
            "door 0x2 open = true jammed = false",
            r#"{"op":"set_door","object":2,"open":true,"locked":null,"jammed":false}"#,
        ),
        (
            DebugCommand::SetHitPoints {
                object: 2,
                value: 3,
            },
            "object 0x2 HP = 3",
            r#"{"op":"set_hit_points","object":2,"value":3}"#,
        ),
        (
            DebugCommand::SetActionPoints {
                object: 2,
                value: 3,
            },
            "object 0x2 AP = 3",
            r#"{"op":"set_action_points","object":2,"value":3}"#,
        ),
        (
            DebugCommand::SetItemCount {
                owner: 2,
                pid: 8,
                count: 3,
            },
            "object 0x2 carries 3 x PID 0x00000008",
            r#"{"op":"set_item_count","owner":2,"pid":8,"count":3}"#,
        ),
        (
            DebugCommand::FireProc {
                object: 2,
                event: USE,
                fixed: 1,
            },
            "fire use_p_proc on object 0x2 (fixed 1)",
            r#"{"op":"fire_proc","object":2,"event":6,"fixed":1}"#,
        ),
        (
            DebugCommand::FireProc {
                object: 2,
                event: 99,
                fixed: 1,
            },
            "fire event 99 on object 0x2 (fixed 1)",
            r#"{"op":"fire_proc","object":2,"event":99,"fixed":1}"#,
        ),
        (
            DebugCommand::FastForwardTimer,
            "fast-forward to the next queued event",
            r#"{"op":"fast_forward_timer"}"#,
        ),
        (
            DebugCommand::TeleportObject {
                object: 2,
                hex: 3,
                elevation: 1,
            },
            "teleport object 0x2 to elevation 1 hex 3",
            r#"{"op":"teleport_object","object":2,"hex":3,"elevation":1}"#,
        ),
        (
            DebugCommand::SpawnObject {
                pid: 8,
                hex: 3,
                elevation: 1,
            },
            "spawn PID 0x00000008 at elevation 1 hex 3",
            r#"{"op":"spawn_object","pid":8,"hex":3,"elevation":1}"#,
        ),
        (
            DebugCommand::SetPaused { on: true },
            "pause the world",
            r#"{"op":"set_paused","on":true}"#,
        ),
        (
            DebugCommand::SetPaused { on: false },
            "resume the world",
            r#"{"op":"set_paused","on":false}"#,
        ),
        (
            DebugCommand::VmBegin {
                object: 2,
                event: DESTROY,
            },
            "step destroy_p_proc of object 0x2",
            r#"{"op":"vm_begin","object":2,"event":18}"#,
        ),
        (
            DebugCommand::VmStep { count: 5 },
            "run 5 stepped instructions",
            r#"{"op":"vm_step","count":5}"#,
        ),
        (
            DebugCommand::VmAbort,
            "abort the stepped procedure",
            r#"{"op":"vm_abort"}"#,
        ),
    ];
    for (command, line, record) in commands {
        assert_eq!(command.to_string(), line);
        assert_eq!(serde_json::to_string(&command).unwrap(), record);
        assert_eq!(
            serde_json::from_str::<DebugCommand>(record).unwrap(),
            command
        );
    }
    assert_eq!(ScriptEvent::Use.code(), USE);
    assert_eq!(
        crate::debug::proc_event(MAP_ENTER),
        Some(ScriptEvent::MapEnter)
    );
    assert_eq!(crate::debug::proc_event(1), None);
}
