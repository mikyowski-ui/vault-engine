//! SPEC-005 §2.2 exported-registry lifetime, SPEC-004 §3.7 event numbers
//! and the stored action code, SPEC-006 §3.5 first requests and map entry,
//! and SPEC-005 0x80F9 dialogue entry.
use crate::fixture_state as fixture;
use crate::{
    assets::WorldAssets,
    world::{ScriptRequest, WorldTrace, empty_map, object_record},
    *,
};
use std::sync::Arc;
use vault_formats::{
    MapFile, Prototype,
    map::{MapScript, ScriptBlock, ScriptCategory, ScriptPrefix},
    pro::ScriptFlags,
};
use vault_vm::{IntProgram, VmHost, VmStatus};

fn table(strings: &[&[u8]]) -> (Vec<u8>, Vec<u32>) {
    let mut payload = Vec::new();
    let mut offsets = Vec::new();
    for text in strings {
        offsets.push(payload.len() as u32 + 6);
        let size = (text.len() + 1).div_ceil(2) * 2;
        payload.extend((size as u16).to_be_bytes());
        payload.extend(*text);
        payload.resize(payload.len() + size - text.len(), 0);
    }
    let mut bytes = (payload.len() as u32).to_be_bytes().to_vec();
    bytes.extend(payload);
    bytes.extend([255; 4]);
    (bytes, offsets)
}
/// A corpus-shaped `.INT` (SPEC-004 §2.2): the initial stream exports
/// `exports` with `0x8016`, then jumps into the body of the first listed
/// procedure (procedure index 1, normally `start`), which returns to the
/// bootstrap's end-of-execution trampoline.
fn program(exports: &[&[u8]], procedures: &[(&[u8], Vec<u8>)]) -> IntProgram {
    let bootstrap =
        fixture::assets::sparse(include_str!("../../../tests/fixtures/vm/program.int.hex"));
    let count = procedures.len() + 1;
    let mut bytes = bootstrap[..42].to_vec();
    bytes.extend((count as u32).to_be_bytes());
    bytes.resize(46 + 24 * count, 0);
    let mut names = vec![b"main".as_slice()];
    names.extend(procedures.iter().map(|(name, _)| *name));
    names.extend(exports.iter().copied());
    let (identifiers, offsets) = table(&names);
    bytes.extend(identifiers);
    bytes.extend(table(&[]).0);
    let init = bytes.len();
    fixture::world::word(&mut bytes, 12, init as i32);
    let mut stream = fixture::instruction(0x802c);
    for index in 0..exports.len() {
        stream.extend(fixture::push(offsets[count + index] as i32));
        stream.extend(fixture::instruction(0x8016));
    }
    stream.extend(fixture::push(0));
    stream.extend(fixture::instruction(0x8003));
    let jump = init + stream.len() + 2;
    stream.extend(fixture::push(0));
    stream.extend(fixture::instruction(0x8004));
    bytes.extend(stream);
    let mut addresses = vec![bytes.len() as u32];
    for op in [0x802b, 0x802a, 0x8029, 0x801c] {
        bytes.extend(fixture::instruction(op));
    }
    for (_, body) in procedures {
        addresses.push(bytes.len() as u32);
        bytes.extend(fixture::returning(body.clone()));
    }
    for index in 0..count {
        fixture::world::word(&mut bytes, 46 + 24 * index, offsets[index] as i32);
        fixture::world::word(&mut bytes, 46 + 24 * index + 16, addresses[index] as i32);
    }
    fixture::world::word(&mut bytes, jump, addresses[1] as i32);
    IntProgram::parse(&bytes).unwrap()
}
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
/// `gvar[script_action] += 1`: which requests reached the body, by number.
fn count_action() -> Vec<u8> {
    [
        fixture::instruction(0x80c7),
        fixture::instruction(0x80c7),
        fixture::instruction(0x80c5),
        fixture::push(1),
        fixture::instruction(0x8039),
        fixture::instruction(0x80c6),
    ]
    .concat()
}

/// Ordinary host failures in rows: `tile_distance` of an off-map hex into
/// gvar 50, `override_map_start` with orientation 9, then gvar 51 += 1.
fn rows() -> Vec<u8> {
    [
        fixture::push(50),
        fixture::push(1),
        fixture::push(50000),
        fixture::instruction(0x80d2),
        fixture::instruction(0x80c6),
        fixture::push(100),
        fixture::push(100),
        fixture::push(0),
        fixture::push(9),
        fixture::instruction(0x80a9),
        count(51),
    ]
    .concat()
}
/// Script files by index; index 9 and above never load.
/// - 0 `mapa.int` and 1 `mapb.int`: map scripts exporting `Ian_ptr`; `start`
///   counts its action, `map_enter_p_proc` counts gvar 31.
/// - 2 `npc.int`: `start` counts its action, `map_enter_p_proc` gvar 30,
///   `talk_p_proc` gvar 40; no `critter_p_proc`.
/// - 3 `talker.int`: `critter_p_proc` enters dialogue (0x80F9); `talk_p_proc`
///   counts gvar 40.
/// - 4 `rows.int`: `use_p_proc` runs [`rows`].
struct Requests {
    critter: Arc<Prototype>,
    programs: Vec<(String, IntProgram)>,
    maps: Vec<(String, MapFile)>,
}
impl WorldAssets for Requests {
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
            .cloned()
            .ok_or_else(|| WorldError::Invalid(format!("script {index} does not load")))
    }
    fn message(&self, _list: i32, _id: i32) -> Option<vault_formats::msg::Message> {
        None
    }
    fn map(&self, path: &str) -> Result<MapFile, WorldError> {
        self.maps
            .iter()
            .find(|(name, _)| name == path)
            .map(|(_, map)| map.clone())
            .ok_or_else(|| WorldError::Invalid(format!("{path} is missing")))
    }
}
fn record(id: u32, file: i32) -> MapScript {
    MapScript {
        id,
        link: 0,
        prefix: ScriptPrefix::None,
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
/// A map whose map script is file `map_script`, with one critter per
/// `(hex, file)` (critter script identifiers `0x04000001` on).
fn map(map_script: i32, critters: &[(i32, i32)]) -> MapFile {
    let mut map = empty_map();
    map.header.script_reference = map_script + 1;
    let mut records = Vec::new();
    for (index, (hex, file)) in critters.iter().enumerate() {
        let id = 0x0400_0001 + index as u32;
        let mut object = object_record(0x0100_0001, index as u32 + 1);
        object.hex = Some(*hex);
        object.script_id = id as i32;
        map.objects.elevations[0].push(object);
        records.push(record(id, *file));
    }
    map.scripts[4] = ScriptCategory {
        declared_count: records.len(),
        blocks: vec![ScriptBlock {
            active: records.len(),
            slots: records,
        }],
    };
    map
}
fn host(maps: Vec<(&str, MapFile)>) -> WorldHost<Requests> {
    let exporting = |name: &str| {
        (
            format!("scripts/{name}"),
            program(
                &[b"Ian_ptr"],
                &[(b"start", count_action()), (b"map_enter_p_proc", count(31))],
            ),
        )
    };
    let programs = vec![
        exporting("mapa.int"),
        exporting("mapb.int"),
        (
            "scripts/npc.int".into(),
            program(
                &[],
                &[
                    (b"start", count_action()),
                    (b"map_enter_p_proc", count(30)),
                    (b"talk_p_proc", count(40)),
                ],
            ),
        ),
        (
            "scripts/talker.int".into(),
            program(
                &[],
                &[
                    (b"start", Vec::new()),
                    (b"critter_p_proc", fixture::instruction(0x80f9)),
                    (b"talk_p_proc", count(40)),
                ],
            ),
        ),
        (
            "scripts/rows.int".into(),
            program(&[], &[(b"start", Vec::new()), (b"use_p_proc", rows())]),
        ),
    ];
    let mut world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    world.gvars = Some(vec![0; 64]);
    WorldHost::new(
        world,
        Requests {
            critter: Arc::new(Prototype::parse(&fixture::npc_prototype()).unwrap()),
            programs,
            maps: maps
                .into_iter()
                .map(|(name, map)| (name.to_string(), map))
                .collect(),
        },
    )
}
fn gvar(host: &WorldHost<Requests>, index: usize) -> i32 {
    host.world.gvars.as_ref().unwrap()[index]
}
fn status(host: &WorldHost<Requests>, script: u32) -> VmStatus {
    host.programs[&host.world.scripts[&script].program].status
}
fn symbol_faults(host: &WorldHost<Requests>) -> usize {
    host.world
        .effects
        .iter()
        .filter(|effect| matches!(effect, WorldEffect::Diagnostic(message) if message.contains("Ian_ptr")))
        .count()
}

#[test]
fn spec005_a_map_change_clears_the_exported_variable_registry() {
    let mut host = host(vec![
        ("maps/a.map", map(0, &[])),
        ("maps/b.map", map(1, &[])),
    ]);
    // Two map scripts from different files export the same name: each map
    // load starts with an empty registry, so neither faults.
    for path in ["maps/a.map", "maps/b.map", "maps/a.map"] {
        host.load_map(path).unwrap();
        let map_script = host.world.map_script.unwrap();
        assert!(!status(&host, map_script).contains(VmStatus::ERROR));
        assert_eq!(symbol_faults(&host), 0, "{path}");
        assert!(host.registry.fetch_variable(b"Ian_ptr").is_ok());
    }
}

#[test]
fn spec005_a_same_map_export_by_another_file_is_a_vm_error() {
    // The map script (mapa) exports `Ian_ptr`; a critter running mapb then
    // exports it too and faults; a second mapa instance is the same owner
    // filename, so its export only resets the value.
    let mut host = host(vec![("maps/a.map", map(0, &[(20104, 1), (20106, 0)]))]);
    host.load_map("maps/a.map").unwrap();
    let [conflict, same] = [0x0400_0001, 0x0400_0002];
    assert!(status(&host, conflict).contains(VmStatus::ERROR));
    assert!(!status(&host, same).contains(VmStatus::ERROR));
    assert_eq!(symbol_faults(&host), 1);
    // The faulted script runs no further requests on this map.
    let before = gvar(&host, 12);
    host.dispatch(ScriptRequest {
        script: conflict,
        event: ScriptEvent::Critter,
        source: 0,
        target: 0,
        fixed: 0,
        action: -1,
        only_present: false,
    })
    .unwrap();
    assert_eq!(gvar(&host, 12), before);
}

#[test]
fn spec006_first_requests_run_the_start_body_with_the_requested_number() {
    let mut host = host(vec![("maps/a.map", map(0, &[(20104, 2)]))]);
    host.load_map("maps/a.map").unwrap();
    // Step 6, the map script's first request: its initial stream ran the
    // `start` body with `script_action` 15. Its `map_enter_p_proc` is not
    // called separately, and step 8's broadcast skips the map script.
    assert_eq!(gvar(&host, 15), 1);
    assert_eq!(gvar(&host, 31), 0);
    // Step 8: the map script's startup request is a later request (start
    // again with 1); the critter's is its first request (start with 1), and
    // its `map_enter_p_proc` body then runs through the broadcast.
    assert_eq!(gvar(&host, 1), 2);
    assert_eq!(gvar(&host, 30), 1);
    // Step 9: neither defines `map_update_p_proc`; the map script falls
    // back to procedure index 1 with 23, the broadcast skips the critter.
    assert_eq!(gvar(&host, 23), 1);
    // The critter cycle: no `critter_p_proc`, so `start` runs with 12.
    host.critter_cycle_step().unwrap();
    assert_eq!(gvar(&host, 12), 1);
    // The stored action code is the last request's and stays after it; it
    // is what a save writes (SPEC-003 script-record offset 0x1C).
    let critter = 0x0400_0001;
    assert_eq!(host.world.scripts[&critter].event, 12);
    let saved = host.world.export_map(false);
    let stored = saved.scripts[4].active().find(|s| s.id == critter).unwrap();
    assert_eq!(stored.event, 12);
    // A timer fires `timed_event_p_proc`: missing, so `start` runs with 22.
    let owner = host.world.scripts[&critter].owner;
    let now = host.world.clock.ticks;
    host.world
        .queue
        .insert(
            now,
            0,
            owner,
            crate::queue::EventKind::Script {
                script: critter,
                parameter: 0,
            },
        )
        .unwrap();
    host.process_queue().unwrap();
    assert_eq!(gvar(&host, 22), 1);
}

#[test]
fn spec006_a_failed_first_request_runs_nothing_and_is_retried() {
    let mut host = host(vec![("maps/a.map", map(0, &[]))]);
    host.load_map("maps/a.map").unwrap();
    let owner = host.world.player;
    let script = host.world.add_script(owner, 9, 4, 0).unwrap();
    // A new instance starts with action code 0.
    assert_eq!(host.world.scripts[&script].event, 0);
    let request = ScriptRequest {
        script,
        event: ScriptEvent::Critter,
        source: 0,
        target: 0,
        fixed: 0,
        action: -1,
        only_present: false,
    };
    for _ in 0..2 {
        assert!(host.dispatch(request.clone()).is_err());
        let instance = &host.world.scripts[&script];
        assert!(!instance.initialized);
        // The number was stored before the load was attempted.
        assert_eq!(instance.event, 12);
    }
}

#[test]
fn spec005_dialogue_system_enter_opens_the_talk_entry_on_the_next_tick() {
    let mut host = host(vec![("maps/a.map", map(0, &[(20102, 3)]))]);
    host.load_map("maps/a.map").unwrap();
    let talker = 0x0400_0001;
    let owner = host.world.scripts[&talker].owner;
    host.world.trace.clear();
    // The critter cycle's `critter_p_proc` requests the dialogue state; the
    // same tick enters it outside that program: `talk_p_proc` runs with 11.
    host.tick(1).unwrap();
    assert_eq!(gvar(&host, 40), 1);
    assert_eq!(host.world.scripts[&talker].event, 11);
    assert_eq!(host.world.dialogue.target, owner);
    assert!(host.world.trace.iter().any(|entry| matches!(
        entry,
        WorldTrace::Script { id, event: ScriptEvent::Talk, .. } if *id == talker
    )));
    // No dialogue opened: the talk session ended and background resumed.
    assert!(!host.world.dialogue.talking);
    assert!(host.world.background_enabled);
    // In combat the request is refused.
    host.world.combat = true;
    host.pending_dialogue = Some(owner);
    assert!(!host.service_dialogue_request().unwrap());
    assert_eq!(gvar(&host, 40), 1);
}

#[test]
fn spec004_event_numbers_follow_the_complete_table() {
    let table = [
        (ScriptEvent::Start, 1, b"start".as_slice()),
        (ScriptEvent::Spatial, 2, b"spatial_p_proc"),
        (ScriptEvent::Description, 3, b"description_p_proc"),
        (ScriptEvent::Pickup, 4, b"pickup_p_proc"),
        (ScriptEvent::Drop, 5, b"drop_p_proc"),
        (ScriptEvent::Use, 6, b"use_p_proc"),
        (ScriptEvent::UseObject, 7, b"use_obj_on_p_proc"),
        (ScriptEvent::UseSkill, 8, b"use_skill_on_p_proc"),
        (ScriptEvent::Talk, 11, b"talk_p_proc"),
        (ScriptEvent::Critter, 12, b"critter_p_proc"),
        (ScriptEvent::Combat, 13, b"combat_p_proc"),
        (ScriptEvent::Damage, 14, b"damage_p_proc"),
        (ScriptEvent::MapEnter, 15, b"map_enter_p_proc"),
        (ScriptEvent::MapExit, 16, b"map_exit_p_proc"),
        (ScriptEvent::Create, 17, b"create_p_proc"),
        (ScriptEvent::Destroy, 18, b"destroy_p_proc"),
        (ScriptEvent::LookAt, 21, b"look_at_p_proc"),
        (ScriptEvent::Timed, 22, b"timed_event_p_proc"),
        (ScriptEvent::MapUpdate, 23, b"map_update_p_proc"),
    ];
    for (event, code, name) in table {
        assert_eq!((event.code(), event.name()), (code, name));
    }
}

#[test]
fn spec005_ordinary_host_failures_are_diagnosed_with_the_null_result() {
    let mut host = host(vec![("maps/a.map", map(0, &[(20104, 4)]))]);
    host.load_map("maps/a.map").unwrap();
    let script = 0x0400_0001;
    let player = host.world.player;
    let hex = host.world.get(player).unwrap().raw.hex;
    host.world.effects.clear();
    host.dispatch(ScriptRequest {
        script,
        event: ScriptEvent::Use,
        source: player,
        target: 0,
        fixed: 0,
        action: -1,
        only_present: false,
    })
    .unwrap();
    // The off-map distance pushes 9999, the bad orientation moves nothing,
    // and the procedure runs on: neither failure faults the program.
    assert_eq!(gvar(&host, 50), 9999);
    assert_eq!(host.world.get(player).unwrap().raw.hex, hex);
    assert_eq!(gvar(&host, 51), 1);
    assert!(!status(&host, script).contains(VmStatus::ERROR));
    let diagnostics = host
        .world
        .effects
        .iter()
        .filter(|effect| matches!(effect, WorldEffect::Diagnostic(message) if message.contains("host failure")))
        .count();
    assert_eq!(diagnostics, 2);
}

#[test]
fn spec005_a_nonpositive_dialogue_list_is_missing_text() {
    let mut host = host(vec![]);
    for list in [0, -1, -5] {
        let text = crate::dialogue::DialogueText::Message { list, message: 3 };
        assert_eq!(host.text_resource(&text, false, false).unwrap().0, b"Error");
    }
}

#[test]
fn spec006_leaving_a_map_closes_an_open_dialogue() {
    let mut host = host(vec![("maps/a.map", map(0, &[]))]);
    host.load_map("maps/a.map").unwrap();
    host.world.dialogue.open = true;
    host.world.dialogue.target = host.world.player;
    host.load_map("maps/a.map").unwrap();
    assert!(!host.world.dialogue.open);
    assert_eq!(host.world.dialogue.target, 0);
}

#[test]
fn the_live_trace_observes_the_time_slice() {
    #[derive(Default)]
    struct Steps(usize);
    impl crate::host::VmObserver for Steps {
        fn before_step(&mut self, _vm: &vault_vm::Vm) {
            self.0 += 1;
        }
        fn after_step(
            &mut self,
            _vm: &vault_vm::Vm,
            _result: &Result<vault_vm::RunOutcome, vault_vm::VmError>,
        ) {
        }
    }
    let mut host = host(vec![]);
    let steps = Arc::new(std::sync::Mutex::new(Steps::default()));
    host.vm_observer = Some(steps.clone());
    // A program the interpreter pass resumes (its bootstrap not yet run).
    let mut vm = vault_vm::Vm::new(host.assets.programs[2].1.clone());
    vm.id = 77;
    host.programs.insert(77, vm);
    host.update_programs().unwrap();
    assert!(steps.lock().unwrap().0 > 10);
}
