//! SPEC-006 §3.7 background passes and the critter-script cycle step,
//! SPEC-003 §3.3 duplicate script identifiers and SPEC-013 §3.22 exit
//! grids at a map's entrance.
use crate::fixture_state as fixture;
use crate::{
    assets::WorldAssets,
    world::{ScriptRequest, Transition, WorldTrace, empty_map, object_record},
    *,
};
use std::sync::Arc;
use vault_formats::{
    MapFile, Prototype,
    map::{MapScript, ObjectDetail, ObjectState, ScriptBlock, ScriptCategory, ScriptPrefix},
    pro::{BasicFlags, PackedLocation, ScriptFlags},
};
use vault_vm::IntProgram;

/// `gvar[index] += 1`.
fn count(index: i32) -> Vec<u8> {
    [
        fixture::push(index),
        fixture::push(index),
        fixture::instruction(0x80c5),
        fixture::push(1),
        fixture::instruction(0x8039),
        fixture::instruction(0x80c6),
    ]
    .concat()
}
fn program(procedures: &[(&[u8], Vec<u8>)]) -> IntProgram {
    let procedures: Vec<(&[u8], Vec<u8>)> = procedures
        .iter()
        .map(|(name, body)| (*name, fixture::returning(body.clone())))
        .collect();
    IntProgram::parse(&fixture::program(&[], &procedures)).unwrap()
}

/// Script files: 0 has `start` (gvar 0) and `critter_p_proc` (gvar 1); 1
/// has only `start` (gvar 0); 2's `critter_p_proc` divides by zero; any
/// other file cannot be loaded.
struct Scripts {
    critter: Arc<Prototype>,
    programs: Vec<IntProgram>,
    map: MapFile,
}
impl Scripts {
    fn new(map: MapFile) -> Self {
        Self {
            critter: Arc::new(Prototype::parse(&fixture::npc_prototype()).unwrap()),
            programs: vec![
                program(&[(b"start", count(0)), (b"critter_p_proc", count(1))]),
                program(&[(b"start", count(0))]),
                program(&[
                    (b"start", Vec::new()),
                    (
                        b"critter_p_proc",
                        [
                            fixture::push(1),
                            fixture::push(0),
                            fixture::instruction(0x803c),
                        ]
                        .concat(),
                    ),
                ]),
            ],
            map,
        }
    }
}
impl WorldAssets for Scripts {
    fn prototype(&self, pid: u32) -> Result<Arc<Prototype>, WorldError> {
        if pid >> 24 == 1 {
            Ok(self.critter.clone())
        } else {
            Err(WorldError::Invalid("no prototype".into()))
        }
    }
    fn program(&self, index: i32) -> Result<(String, IntProgram), WorldError> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.programs.get(index))
            .map(|program| ("synthetic.int".into(), program.clone()))
            .ok_or_else(|| WorldError::Invalid(format!("script {index} does not load")))
    }
    fn message(&self, _list: i32, _id: i32) -> Option<vault_formats::msg::Message> {
        None
    }
    fn map(&self, _path: &str) -> Result<MapFile, WorldError> {
        Ok(self.map.clone())
    }
    fn map_path(&self, number: i32) -> Option<String> {
        (number == 7).then(|| "maps/synth.map".into())
    }
}

fn host(map: MapFile) -> WorldHost<Scripts> {
    let mut world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    world.gvars = Some(vec![0; 4]);
    // Only the records these tests make are in the critter stream.
    world.drop_player_script();
    WorldHost::new(world, Scripts::new(map))
}
/// A scripted critter at `hex` on `elevation`.
fn critter(host: &mut WorldHost<Scripts>, hex: i32, elevation: u8, file: i32) -> u32 {
    let prototype = host.assets.critter.clone();
    let object = host.world.spawn(prototype, hex, elevation).unwrap();
    host.world.add_script(object, file, 4, 0).unwrap()
}
fn request(script: u32, event: ScriptEvent) -> ScriptRequest {
    ScriptRequest {
        script,
        event,
        source: 0,
        target: 0,
        fixed: 0,
        action: -1,
        only_present: false,
    }
}
/// The scripts that received a request of `event`, in order.
fn requested(host: &WorldHost<Scripts>, event: ScriptEvent) -> Vec<u32> {
    host.world
        .trace
        .iter()
        .filter_map(|entry| match entry {
            WorldTrace::Script { id, event: e, .. } if *e == event => Some(*id),
            _ => None,
        })
        .collect()
}
fn gvar(host: &WorldHost<Scripts>, index: usize) -> i32 {
    host.world.gvars.as_ref().unwrap()[index]
}

#[test]
fn spec006_critter_cycle_serves_one_record_per_step_in_list_order() {
    let mut host = host(empty_map());
    // Every critter-stream record counts: another elevation, a hidden
    // owner, a dead owner whose script cannot be loaded.
    let a = critter(&mut host, 20100, 0, 0);
    let b = critter(&mut host, 20102, 1, 1);
    let c = critter(&mut host, 20104, 0, 2);
    let d = critter(&mut host, 20106, 0, 9);
    let owner = |host: &WorldHost<Scripts>, script: u32| host.world.scripts[&script].owner;
    let hidden = owner(&host, c);
    host.world.get_mut(hidden).unwrap().raw.flags.0 |= BasicFlags::HIDDEN.0;
    let dead = owner(&host, d);
    host.world.critter_state_mut(dead).unwrap().results.0 |= critical::effect::DEAD;
    // The startup pass loads each program (its first request runs the
    // initial stream, not a procedure).
    for script in [a, b, c, d] {
        let _ = host.dispatch(request(script, ScriptEvent::Start));
    }
    host.world.trace.clear();
    host.world.effects.clear();
    // The counter starts at 0, so the first step serves position 1. A
    // script without `critter_p_proc` runs index 1 (`start`); a VM error
    // stops c's program; d's file does not load, so its turn does nothing.
    for _ in 0..6 {
        host.critter_cycle_step().unwrap();
    }
    assert_eq!(requested(&host, ScriptEvent::Critter), [b, c, a, b]);
    assert_eq!((gvar(&host, 0), gvar(&host, 1)), (2, 1));
    assert_eq!(host.critter_cycle, 2);
    let diagnostics = host
        .world
        .effects
        .iter()
        .filter(|effect| matches!(effect, WorldEffect::Diagnostic(_)))
        .count();
    assert_eq!(diagnostics, 1, "only c's VM error is reported");

    // Nothing happens in combat or during dialogue, not even the counter.
    host.world.trace.clear();
    host.world.combat = true;
    host.critter_cycle_step().unwrap();
    host.world.combat = false;
    host.world.dialogue.open = true;
    host.critter_cycle_step().unwrap();
    host.world.dialogue.open = false;
    assert!(requested(&host, ScriptEvent::Critter).is_empty());
    assert_eq!(host.critter_cycle, 2);

    // Removing a record moves its stream's last record into the slot.
    let ids = |host: &WorldHost<Scripts>| host.world.script_ids();
    assert_eq!(ids(&host), [a, b, c, d]);
    host.world.remove_script(a);
    assert_eq!(ids(&host), [d, b, c]);
    let e = critter(&mut host, 20108, 0, 0);
    assert_eq!(ids(&host), [d, b, c, e]);

    // A map change keeps the counter; at or above the new N it wraps to 0.
    host.critter_cycle = 3;
    host.world.import_map(empty_map(), &host.assets).unwrap();
    assert_eq!(host.critter_cycle, 3);
    let f = critter(&mut host, 20100, 0, 0);
    let _ = host.dispatch(request(f, ScriptEvent::Start));
    host.world.trace.clear();
    host.critter_cycle_step().unwrap();
    host.critter_cycle_step().unwrap();
    // N = 1: every step serves position 0.
    assert_eq!(host.critter_cycle, 0);
    assert_eq!(requested(&host, ScriptEvent::Critter), [f, f]);
}

#[test]
fn spec006_each_background_pass_runs_one_cycle_step() {
    let mut host = host(empty_map());
    let a = critter(&mut host, 20100, 0, 0);
    let b = critter(&mut host, 20102, 0, 0);
    for script in [a, b] {
        let _ = host.dispatch(request(script, ScriptEvent::Start));
    }
    host.world.trace.clear();
    // Passes run at the polling rate, not on the 100 ms clock.
    for now in [1000, 1001, 1002] {
        host.tick(now).unwrap();
    }
    assert_eq!(requested(&host, ScriptEvent::Critter), [b, a, b]);
    // Map background processing off (a suspending window, a map load):
    // the pass skips the critter step and the clock checker.
    host.world.trace.clear();
    host.world.background_enabled = false;
    let ticks = host.world.clock.ticks;
    host.tick(5000).unwrap();
    assert!(requested(&host, ScriptEvent::Critter).is_empty());
    assert_eq!(host.world.clock.ticks, ticks);
    host.world.background_enabled = true;
    host.tick(5001).unwrap();
    assert_eq!(requested(&host, ScriptEvent::Critter), [a]);
    assert_eq!(host.world.clock.ticks, ticks + 1);
}

fn record(id: u32, file: i32, prefix: ScriptPrefix) -> MapScript {
    MapScript {
        id,
        link: 0,
        prefix,
        stored_flags: ScriptFlags(0),
        flags: ScriptFlags(0),
        file_index: file,
        object_id: -1,
        local_base: -1,
        stored_local_count: 0,
        local_count: 0,
        result: 0,
        event: 0,
        parameter: 0,
        action: 0,
        override_default: 0,
        execution_state: 0,
        quantity: 0,
        annotations: 0,
        file_offset: 0,
    }
}
fn stream(records: Vec<MapScript>) -> ScriptCategory {
    ScriptCategory {
        declared_count: records.len(),
        blocks: vec![ScriptBlock {
            active: records.len(),
            slots: records,
        }],
    }
}
fn spatial(hex: u32) -> ScriptPrefix {
    ScriptPrefix::Spatial {
        location: PackedLocation(hex),
        radius: 0,
    }
}

#[test]
fn spec003_duplicate_identifiers_bind_the_first_record() {
    const A: u32 = 0x0400_0001;
    const B: u32 = 0x0400_0002;
    const S: u32 = 0x0100_0001;
    let mut map = empty_map();
    // Stream 0 holds an item-category identifier: lookup never finds it.
    map.scripts[0] = stream(vec![record(0x0300_0005, 1, ScriptPrefix::None)]);
    map.scripts[1] = stream(vec![
        record(S, 1, spatial(2010)),
        record(S, 1, spatial(3010)),
    ]);
    map.scripts[4] = stream(vec![
        record(A, 0, ScriptPrefix::None),
        record(B, 1, ScriptPrefix::None),
        record(A, 1, ScriptPrefix::None),
    ]);
    // Two objects name A: the last one read owns it.
    for (index, hex) in [(1, 20200), (2, 20202)] {
        let mut object = object_record(0x0200_0001, index);
        object.hex = Some(hex);
        object.script_id = A as i32;
        map.objects.elevations[0].push(object);
    }
    let mut host = host(map);
    host.load_map("maps/synth.map").unwrap();
    // The first record binds (file 0), and every record keeps its slot.
    assert_eq!(host.world.scripts[&A].file, 0);
    assert!(!host.world.scripts.contains_key(&0x0300_0005));
    assert_eq!(host.world.script_ids(), [0x0300_0005, S, S, A, B, A]);
    let owner = host.world.scripts[&A].owner;
    assert_eq!(host.world.get(owner).unwrap().raw.hex, Some(20202));
    // The startup pass requests the first record once per record.
    let starts = requested(&host, ScriptEvent::Start);
    assert_eq!(starts.iter().filter(|id| **id == A).count(), 2);
    assert_eq!(starts.iter().filter(|id| **id == S).count(), 2);

    // The cycle gives the later record's turn to the first.
    host.world.trace.clear();
    for _ in 0..3 {
        host.critter_cycle_step().unwrap();
    }
    assert_eq!(requested(&host, ScriptEvent::Critter), [B, A, A]);

    // The later record's own location triggers the first one's spatial.
    host.world.trace.clear();
    let player = host.world.player;
    for hex in [2010, 3010, 3012] {
        host.world.move_object(player, hex, 0, true).unwrap();
        host.drain_requests().unwrap();
    }
    assert_eq!(requested(&host, ScriptEvent::Spatial), [S, S]);

    // A save writes the later and the mismatched records back unchanged.
    let saved = host.world.export_map(false);
    let ids: Vec<Vec<(u32, i32)>> = saved
        .scripts
        .iter()
        .map(|category| category.active().map(|s| (s.id, s.file_index)).collect())
        .collect();
    assert_eq!(ids[0], [(0x0300_0005, 1)]);
    assert_eq!(ids[4], [(A, 0), (B, 1), (A, 1)]);

    // Removing A moves the stream's last record (the later A) into its
    // slot, and lookup now finds it.
    host.world.remove_script(A);
    assert_eq!(host.world.scripts[&A].file, 1);
    assert_eq!(host.world.script_ids(), [0x0300_0005, S, S, A, B]);
}

fn exit_grid(
    map: i32,
    hex: i32,
    elevation: i32,
    orientation: i32,
) -> vault_formats::map::MapObject {
    let mut grid = object_record(0x0500_0010, 9);
    grid.hex = Some(20100);
    grid.state = ObjectState::Other {
        word: 0,
        detail: ObjectDetail::ExitGrid {
            map,
            hex,
            elevation,
            orientation,
        },
    };
    grid
}

#[test]
fn spec013_exit_grids_at_the_entrance_request_a_transition() {
    // The default entrance, hex 20100 on elevation 0, holds an exit grid
    // to map 7, hex 5000, elevation 1, facing 2.
    let mut map = empty_map();
    map.objects.elevations[0].push(exit_grid(7, 5000, 1, 2));
    let mut host = host(map);
    let player = host.world.player;
    // A game load or a new game's first map: the request stays pending.
    host.load_map("maps/synth.map").unwrap();
    let grid = Transition {
        map: 7,
        elevation: 1,
        hex: 5000,
        facing: 2,
    };
    assert_eq!(host.world.pending_transition, Some(grid));
    assert_eq!(host.world.get(player).unwrap().raw.hex, Some(20100));

    // Transition processing: the request made during the load is cleared,
    // and steps 2 and 4 use the request saved when processing started.
    host.world.pending_transition = Some(Transition {
        map: 7,
        elevation: 0,
        hex: 12345,
        facing: 3,
    });
    assert!(host.process_transition().unwrap());
    assert_eq!(host.world.pending_transition, None);
    let object = host.world.get(player).unwrap();
    assert_eq!(
        (object.raw.hex, object.raw.elevation, object.raw.orientation),
        (Some(12345), 0, 3)
    );

    // A town or world map opened by map -1 or -2 discards the request
    // when it closes.
    host.world.pending_transition = Some(Transition {
        map: 0,
        elevation: -1,
        hex: -1,
        facing: -1,
    });
    assert!(host.process_transition().unwrap());
    host.load_map("maps/synth.map").unwrap();
    assert_eq!(host.world.pending_transition, Some(grid));
    host.world_map_closed();
    assert_eq!(host.world.pending_transition, None);
    // Without such a transition, closing leaves a request pending.
    host.load_map("maps/synth.map").unwrap();
    host.world_map_closed();
    assert_eq!(host.world.pending_transition, Some(grid));
}
