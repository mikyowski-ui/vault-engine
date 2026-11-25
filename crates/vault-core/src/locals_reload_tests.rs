//! SPEC-006 §3.5 / SPEC-018 §6.5 / SPEC-021 §2.8: a slot load of a slot
//! load holds the same local slices, under the same script identifiers, as
//! the first load, and the local pool settles instead of growing.
use crate::{
    CharacterSheet, GameProfile, WorldHost, WorldState, assets::VfsWorldAssets,
    fallout2::Fallout2World, host_save::SaveInfo, test_support::dir_assets,
};
use vault_fixtures::campaign::{self, Edition};
use vault_formats::AutomapDb;

type Host = WorldHost<VfsWorldAssets>;

/// A host over the synthetic state corpus and campaign, without a map.
fn bare_host(dir: &std::path::Path, edition: Edition) -> Host {
    let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let mut host = WorldHost::new(world, dir_assets(dir));
    host.load_campaign().unwrap();
    if edition == Edition::Fallout2 {
        host.set_game_profile(GameProfile::Fallout2);
        let tables = host.load_fallout2_tables().unwrap();
        let start = tables.maps.match_name(b"artemple.map");
        host.world.persistence.automap =
            AutomapDb::fallout2(|map| tables.maps.excluded(map as i32));
        host.world.fallout2 = Some(Box::new(Fallout2World::new_game(tables, start)));
    }
    host
}

/// The campaign's first map, loaded.
fn campaign_host(edition: Edition) -> (tempfile::TempDir, Host) {
    let dir = tempfile::tempdir().unwrap();
    vault_fixtures::state::write_corpus(dir.path());
    campaign::write_campaign(dir.path(), edition);
    let mut host = bare_host(dir.path(), edition);
    host.load_map(&campaign::start(edition)).unwrap();
    (dir, host)
}

/// The player's script record and its local slice, as the hash sees them.
fn player_locals(host: &Host) -> (u32, Option<Vec<i32>>) {
    let sid = host
        .world
        .script_for(host.world.player)
        .expect("player record");
    let slice = host
        .world
        .state_snapshot()
        .lvars
        .into_iter()
        .find(|slice| slice.script == sid)
        .map(|slice| slice.values);
    (sid, slice)
}

/// Fallout 2, as a new game leaves the player (SPEC-018 §6.5): the record
/// has `scripts.lst` entry 0's slice; the player script then writes local 1.
fn fallout2_player_with_locals() -> (tempfile::TempDir, Host) {
    let (dir, mut host) = campaign_host(Edition::Fallout2);
    let count =
        crate::assets::WorldAssets::local_count(&host.assets, crate::world::PLAYER_SCRIPT_FILE);
    assert!(count > 0, "the corpus player script declares locals");
    let sid = host.world.script_for(host.world.player).unwrap();
    host.world.set_player_locals(sid, count, None);
    let base = host.world.ensure_locals(sid).unwrap();
    host.world.lvars[base + 1] = 7;
    (dir, host)
}

/// A slot image of `live`, loaded into a fresh host of the same edition.
fn loaded(dir: &std::path::Path, live: &mut Host, edition: Edition) -> Host {
    let mut second = bare_host(dir, edition);
    let mut image = live.slot_image(&SaveInfo::default()).unwrap();
    second.world.rng = live.world.rng.clone();
    second.load_slot(&mut image).unwrap();
    second
}

/// SPEC-003 §3.2 (lazy slices, and the safe rule that a script's locals
/// never reach another script's slice): a record with no locals whose
/// script asked for one owns no pool values. When a load gives it its
/// file's declared count, that slice is its own, not a window over values
/// a later allocation hands to another record.
#[test]
fn spec003_a_record_saved_without_locals_never_aliases_a_later_slice() {
    for edition in [Edition::Fallout1, Edition::Fallout2] {
        let (dir, mut live) = campaign_host(edition);
        // `create_spatial` (SPEC-024b §4.12) makes a record with no locals
        // for a file that declares 4.
        let spatial = live.world.add_spatial_script(0, 20100, 0, 1).unwrap();
        assert_eq!(live.world.scripts[&spatial].local_count, 0);
        assert_eq!(live.world.local(Some(spatial), 0), 0, "outside its slice");
        let mut second = loaded(dir.path(), &mut live, edition);
        let other = second.world.add_script(0, 0, 1, 4).unwrap();
        for index in 0..4 {
            assert!(second.world.set_local(Some(other), index, 90 + index));
        }
        let values: Vec<i32> = (0..4)
            .map(|index| second.world.local(Some(spatial), index))
            .collect();
        assert_eq!(values, [0; 4], "{edition:?}: the spatial record's locals");
        let slices = second.world.state_snapshot().lvars;
        let held: Vec<_> = slices
            .iter()
            .filter(|slice| slice.values.contains(&90))
            .map(|slice| slice.script)
            .collect();
        assert_eq!(
            held,
            [other],
            "{edition:?}: only its owner holds the values"
        );
    }
}

/// SPEC-006 §3.5 "Loading a game": load A of the live world's slot, then
/// load B of A's slot, then load C of B's: each hashes the same, and the
/// player's record keeps its saved locals under one identifier.
#[test]
fn spec006_fallout2_player_locals_settle_across_a_load_of_a_load() {
    let (dir, mut live) = fallout2_player_with_locals();
    let mut second = bare_host(dir.path(), Edition::Fallout2);
    let mut image = live.slot_image(&SaveInfo::default()).unwrap();
    second.world.rng = live.world.rng.clone();
    second.load_slot(&mut image).unwrap();
    let (sid_a, slice_a) = player_locals(&second);
    assert_eq!(slice_a, Some(vec![0, 7, 0, 0]), "load A keeps the locals");
    let hash_a = second.world.state_hash();
    let pool_a = second.world.lvars.len();
    let mut ids = vec![sid_a];
    for round in 0..6 {
        let mut again = second.slot_image(&SaveInfo::default()).unwrap();
        second.world.rng = live.world.rng.clone();
        second.load_slot(&mut again).unwrap();
        let (sid, slice) = player_locals(&second);
        ids.push(sid);
        assert_eq!(
            second.world.state_hash(),
            hash_a,
            "round {round}: the hash; player identifiers {ids:x?}"
        );
        assert_eq!(slice, slice_a, "round {round}: the locals");
        assert_eq!(sid, sid_a, "round {round}: identifiers {ids:x?}");
        assert_eq!(second.world.lvars.len(), pool_a, "round {round}: pool");
    }
}

/// `scripts.lst` entry 0 (the corpus' `synthetic.int`, 4 locals, which the
/// campaign's friendly critter runs and, under Fallout 2, the player's
/// record): `map_enter_p_proc` reads local 0 and writes `local 0 * 0 + 5`
/// back, and `map_update_p_proc` writes `local 0 + 1` to local 1. A load
/// runs them, so each load writes what any other would.
fn locals_program() -> Vec<u8> {
    use vault_fixtures::state::{instruction, program, push, returning};
    let enter = [
        push(0),
        push(0),
        instruction(0x80c1),
        push(0),
        instruction(0x803b),
        push(5),
        instruction(0x8039),
        instruction(0x80c2),
    ]
    .concat();
    let update = [
        push(1),
        push(0),
        instruction(0x80c1),
        push(1),
        instruction(0x8039),
        instruction(0x80c2),
    ]
    .concat();
    program(
        &[],
        &[
            (b"start", returning(Vec::new())),
            (b"map_enter_p_proc", returning(enter)),
            (b"map_update_p_proc", returning(update)),
        ],
    )
}

/// One save-and-load of `from` into `into`, with `rng` copied in before and
/// after, as the audit does.
fn load_into(from: &mut Host, into: &mut Host, rng: &crate::ParkMiller) {
    let mut image = from.slot_image(&SaveInfo::default()).unwrap();
    into.world.rng = rng.clone();
    into.load_slot(&mut image).unwrap();
    into.world.rng = rng.clone();
}

/// The campaign friendly critter's local `index` in `host`.
fn friend_local(host: &mut Host, index: i32) -> i32 {
    let friend = host
        .world
        .reference_for_id(campaign::FRIEND)
        .and_then(|object| host.world.script_for(object))
        .expect("the friend's record");
    host.world.local(Some(friend), index)
}

/// SPEC-006 §3.5 / SPEC-013 §3.22 / SPEC-021: on the campaign's two-elevation
/// first map, whose scripts read and write their locals on map enter and map
/// update, the player changes elevation by the ladders' `move_to` and by a
/// transition to the map's own number (an exit grid's request), 40 times.
/// After each change the audit's load A (of the live world) and load B (of
/// A) hash the same and keep the friend's locals, and a chain of loads of
/// loads keeps the local pool at one length.
#[test]
fn spec006_elevation_changes_and_own_map_transitions_keep_two_loads_equal() {
    use crate::{ScriptEvent, Transition, world::ScriptRequest};
    for edition in [Edition::Fallout1, Edition::Fallout2] {
        let dir = tempfile::tempdir().unwrap();
        vault_fixtures::state::write_corpus(dir.path());
        campaign::write_campaign(dir.path(), edition);
        std::fs::write(dir.path().join("scripts/synthetic.int"), locals_program()).unwrap();
        let mut live = bare_host(dir.path(), edition);
        live.load_map(&campaign::start(edition)).unwrap();
        if edition == Edition::Fallout2 {
            // SPEC-018 §6.5: a new game's player record has entry 0's slice.
            let sid = live.world.script_for(live.world.player).unwrap();
            live.world.set_player_locals(sid, 4, None);
        }
        live.map_update().unwrap();
        let number = live.world.map.header.map_number;
        let ladder = |host: &Host, id: u32| {
            let object = host.world.reference_for_id(id).unwrap();
            (object, host.world.script_for(object).unwrap())
        };
        let mut second = bare_host(dir.path(), edition);
        let mut pools = Vec::new();
        for round in 0..40 {
            let player = live.world.player;
            let up = live.world.get(player).unwrap().raw.elevation == 0;
            if round % 2 == 0 {
                // A scripted move: the ladder's `use_p_proc` runs `move_to`.
                let id = if up {
                    campaign::LADDER_UP
                } else {
                    campaign::LADDER_DOWN
                };
                let (object, script) = ladder(&live, id);
                live.dispatch(ScriptRequest {
                    script,
                    event: ScriptEvent::Use,
                    source: player,
                    target: object,
                    fixed: 0,
                    action: -1,
                    only_present: false,
                })
                .unwrap();
                live.drain_requests().unwrap();
            } else {
                // An exit grid to the map's own number, at the other landing.
                let (hex, elevation) = if up {
                    (campaign::UPPER_LANDING, 1)
                } else {
                    (campaign::LOWER_LANDING, 0)
                };
                live.request_transition(Transition {
                    map: number,
                    elevation,
                    hex,
                    facing: 0,
                });
                assert!(live.process_transition().unwrap());
            }
            live.map_update().unwrap();
            let now = live.world.get(player).unwrap().raw.elevation;
            assert_eq!(now == 1, up, "{edition:?} round {round}: the elevation");
            // A value that changes from change to change, as play leaves it.
            let friend = live
                .world
                .reference_for_id(campaign::FRIEND)
                .and_then(|object| live.world.script_for(object))
                .unwrap();
            assert!(live.world.set_local(Some(friend), 2, 100 + round));
            let rng = live.world.rng.clone();
            load_into(&mut live, &mut second, &rng);
            let hash_a = second.world.state_hash();
            let (sid_a, _) = player_locals(&second);
            let mut middle = std::mem::replace(&mut second, bare_host(dir.path(), edition));
            load_into(&mut middle, &mut second, &rng);
            assert_eq!(
                second.world.state_hash(),
                hash_a,
                "{edition:?} round {round}: state_hash(A) and state_hash(B)"
            );
            assert_eq!(player_locals(&second).0, sid_a, "{edition:?} round {round}");
            assert_eq!(friend_local(&mut second, 2), 100 + round, "{edition:?}");
            assert_eq!(friend_local(&mut second, 0), 5, "{edition:?}");
            pools.push(live.world.lvars.len());
        }
        assert!(
            pools.iter().all(|pool| *pool == pools[0]),
            "{edition:?}: live pool {pools:?}"
        );
        // Loads of loads, as a player saving and loading over and over.
        let rng = live.world.rng.clone();
        let mut chain = Vec::new();
        for _ in 0..30 {
            let mut middle = std::mem::replace(&mut second, bare_host(dir.path(), edition));
            load_into(&mut middle, &mut second, &rng);
            chain.push((second.world.state_hash(), second.world.lvars.len()));
        }
        assert!(
            chain.iter().all(|link| *link == chain[0]),
            "{edition:?}: {chain:x?}"
        );
    }
}

/// The campaign's first map with `locals_program` as `scripts.lst` entry 0.
fn locals_host(edition: Edition) -> (tempfile::TempDir, Host) {
    let dir = tempfile::tempdir().unwrap();
    vault_fixtures::state::write_corpus(dir.path());
    campaign::write_campaign(dir.path(), edition);
    std::fs::write(dir.path().join("scripts/synthetic.int"), locals_program()).unwrap();
    let mut host = bare_host(dir.path(), edition);
    host.load_map(&campaign::start(edition)).unwrap();
    (dir, host)
}

/// SPEC-006 §3.5 / SPEC-008: an item the player holds keeps its script
/// record and locals through a load and a load of that load, bound to the
/// item again, and the chain settles.
#[test]
fn spec008_a_held_items_script_and_locals_survive_loads_of_loads() {
    for edition in [Edition::Fallout1, Edition::Fallout2] {
        let (dir, mut live) = locals_host(edition);
        let player = live.world.player;
        let pistol = live.world.reference_for_id(campaign::PISTOL).unwrap();
        live.world.add_item(player, pistol, 1, true).unwrap();
        let script = live.world.add_script(pistol, 0, 3, 4).unwrap();
        assert!(live.world.set_local(Some(script), 2, 77));
        let pid = live.world.get(pistol).unwrap().raw.pid;
        let held = |host: &mut Host| {
            let player = host.world.player;
            let item = host
                .world
                .get(player)
                .unwrap()
                .inventory
                .iter()
                .map(|(item, _)| *item)
                .find(|item| {
                    host.world
                        .get(*item)
                        .is_some_and(|object| object.raw.pid == pid)
                });
            let item = item.expect("the pistol is held");
            let sid = host.world.script_for(item).expect("the pistol's record");
            assert_eq!(host.world.scripts[&sid].owner, item, "{edition:?}: bound");
            host.world.local(Some(sid), 2)
        };
        let rng = live.world.rng.clone();
        let mut second = bare_host(dir.path(), edition);
        load_into(&mut live, &mut second, &rng);
        assert_eq!(held(&mut second), 77, "{edition:?}: load A");
        let mut chain = Vec::new();
        for _ in 0..12 {
            let mut middle = std::mem::replace(&mut second, bare_host(dir.path(), edition));
            load_into(&mut middle, &mut second, &rng);
            assert_eq!(held(&mut second), 77, "{edition:?}");
            chain.push((second.world.state_hash(), second.world.lvars.len()));
        }
        assert!(
            chain.iter().all(|link| *link == chain[0]),
            "{edition:?}: {chain:x?}"
        );
    }
}

/// SPEC-003 §3.3 / SPEC-006 §3.5 step 1: a temporary object is not saved,
/// and neither is its script record: a load holds no record for it, with no
/// owner, carrying its locals.
#[test]
fn spec008_a_temporary_objects_script_is_not_saved_as_an_orphan() {
    use vault_formats::pro::BasicFlags;
    for edition in [Edition::Fallout1, Edition::Fallout2] {
        let (dir, mut live) = locals_host(edition);
        let prototype = crate::assets::WorldAssets::prototype(&live.assets, 0x0100_0001).unwrap();
        let temporary = live.world.spawn(prototype, 20500, 0).unwrap();
        live.world.get_mut(temporary).unwrap().raw.flags.0 |= BasicFlags::TEMPORARY.0;
        let script = live.world.add_script(temporary, 0, 4, 4).unwrap();
        assert!(live.world.set_local(Some(script), 3, 55));
        let rng = live.world.rng.clone();
        let mut second = bare_host(dir.path(), edition);
        load_into(&mut live, &mut second, &rng);
        let orphans: Vec<_> = second
            .world
            .state_snapshot()
            .lvars
            .into_iter()
            .filter(|slice| slice.values.get(3) == Some(&55))
            .collect();
        assert!(orphans.is_empty(), "{edition:?}: {orphans:?}");
    }
}
