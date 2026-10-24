//! Debugger commands through `WorldHost::apply_debug`.
use crate::{
    CharacterSheet, WorldHost, WorldState, assets::VfsWorldAssets, debug::DebugCommand,
    fixture_state as fixture,
};
use std::{fs, sync::Arc};
use vault_formats::{LocalDirProvider, map::ObjectState};

const ITEM_PID: u32 = 0x0000_0001;

fn host() -> (tempfile::TempDir, WorldHost<VfsWorldAssets>) {
    let dir = tempfile::tempdir().unwrap();
    fixture::write_corpus(dir.path());
    // `map.msg` lists the fixture map as number 7.
    fs::copy(
        dir.path().join("maps/synthetic.map"),
        dir.path().join("maps/synth.map"),
    )
    .unwrap();
    let assets = VfsWorldAssets::new(
        Arc::new(LocalDirProvider::mount(dir.path()).unwrap()),
        "english",
    )
    .unwrap();
    let mut host = WorldHost::new(
        WorldState::new(CharacterSheet::default(), 1).unwrap(),
        assets,
    );
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
    host.world
        .reference_for_id(id_of(host, 0x0100_0001))
        .unwrap()
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
