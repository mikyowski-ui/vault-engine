//! The `enhanced` camera: the requests `0x8210`-`0x8213` under both presets,
//! the scroll region, and the synthetic camera demo.
use crate::{
    CharacterSheet, EnginePreset, WorldEffect, WorldHost, WorldState,
    assets::WorldAssets,
    camera::{CameraCommand, CameraRegion, SCROLL_BLOCKER_PID, flood},
    fixture_state as fixture,
    test_support::{TestAssets, dir_assets},
    ui::cursor,
    world::{WorldObject, empty_map, object_record},
};
use vault_formats::HexGrid;
use vault_vm::{IntProgram, RunOutcome, Vm, VmError, VmStatus};

fn camera<A: WorldAssets>(host: &WorldHost<A>) -> Vec<CameraCommand> {
    host.world
        .effects
        .iter()
        .filter_map(|effect| match effect {
            WorldEffect::Camera(command) => Some(*command),
            _ => None,
        })
        .collect()
}
fn diagnostics<A: WorldAssets>(host: &WorldHost<A>) -> Vec<String> {
    host.world
        .effects
        .iter()
        .filter_map(|effect| match effect {
            WorldEffect::Diagnostic(message) => Some(message.clone()),
            _ => None,
        })
        .collect()
}
fn host(preset: EnginePreset) -> WorldHost<TestAssets> {
    let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let mut host = WorldHost::new(world, TestAssets::default());
    host.set_preset(preset);
    host
}
/// `code` as a fragment of an enhanced parse of the SPEC-004 fixture.
fn fragment(code: &[u8]) -> Vm {
    let program = IntProgram::parse_enhanced(&crate::fixture_vm::program()).unwrap();
    Vm::fragment(program, code).unwrap()
}
/// `code` as a fragment of a plain SPEC-004 parse, as Vanilla loads scripts.
fn vanilla_fragment(code: &[u8]) -> Vm {
    let program = IntProgram::parse(&crate::fixture_vm::program()).unwrap();
    Vm::fragment(program, code).unwrap()
}
/// The pushes and word of one request, operands in the order written.
fn request(word: u16, operands: &[i32]) -> Vec<u8> {
    let mut code: Vec<u8> = operands
        .iter()
        .flat_map(|value| fixture::push(*value))
        .collect();
    code.extend(fixture::instruction(word));
    code
}
const DEMO: [CameraCommand; 4] = [
    CameraCommand::PanTo {
        tile: fixture::CAM_DEMO_INTERIOR,
        duration_ms: 1500,
    },
    CameraCommand::SetZoom {
        percent: 200,
        duration_ms: 1000,
    },
    CameraCommand::Shake {
        intensity: 6,
        duration_ms: 800,
    },
    CameraCommand::Reset { duration_ms: 1500 },
];

#[test]
fn vanilla_rejects_the_camera_words_as_unknown_words() {
    // With valid operands below each word, Vanilla fails exactly as for a
    // word outside every range: the same error and machine state, no
    // effect and no diagnostic.
    let outcome = |word: u16| {
        let mut host = host(EnginePreset::Vanilla);
        let mut vm = vanilla_fragment(&request(word, &[20100, 0]));
        let result = vm.run(&mut host, 10);
        assert!(camera(&host).is_empty());
        assert!(diagnostics(&host).is_empty());
        assert!(host.calls.is_empty());
        // An enhanced parse reaching a Vanilla host fails as unknown too.
        let mut host = self::host(EnginePreset::Vanilla);
        let mut enhanced = fragment(&request(word, &[20100, 0]));
        assert_eq!(enhanced.run(&mut host, 10), Err(VmError::Opcode(word)));
        assert!(camera(&host).is_empty() && host.calls.is_empty());
        (result, vm.status, vm.pc, vm.value_stack.byte_len())
    };
    let (other, other_status, other_pc, other_depth) = outcome(0x8156);
    assert_eq!(other, Err(VmError::Opcode(0x8156)));
    for word in 0x8210..=0x8213 {
        let (result, status, pc, depth) = outcome(word);
        assert_eq!(result, Err(VmError::Opcode(word)));
        assert_eq!((status, pc, depth), (other_status, other_pc, other_depth));
    }
    // A Vanilla parse refuses the demo script as it refuses any undefined
    // word.
    let error = IntProgram::parse(&fixture::cam_demo_program())
        .unwrap_err()
        .to_string();
    assert!(error.contains("invalid instruction 0x8210"), "{error}");
}

#[test]
fn enhanced_requests_leave_the_world_as_camera_effects_in_order() {
    let mut host = host(EnginePreset::Enhanced);
    let code: Vec<u8> = fixture::CAM_DEMO_REQUESTS
        .iter()
        .flat_map(|(word, operands)| request(*word, operands))
        .collect();
    let mut vm = fragment(&[fixture::push(77), code].concat());
    assert_eq!(vm.run(&mut host, 100).unwrap(), RunOutcome::FragmentEnd);
    assert_eq!(camera(&host), DEMO);
    assert!(diagnostics(&host).is_empty());
    // They push nothing: the sentinel below them is on top.
    assert_eq!(vm.value_stack.pop().unwrap().payload, 77);
    assert!(vm.value_stack.pop().is_err());
    let traced: Vec<u16> = host.calls.iter().map(|call| call.opcode).collect();
    assert_eq!(traced, [0x8210, 0x8211, 0x8212, 0x8213]);
    // Bounds and zero durations are accepted.
    let mut host = self::host(EnginePreset::Enhanced);
    let code = [
        request(0x8210, &[0, 0]),
        request(0x8210, &[39999, 0]),
        request(0x8211, &[1, 0]),
        request(0x8212, &[0, 0]),
        request(0x8213, &[0]),
    ]
    .concat();
    fragment(&code).run(&mut host, 100).unwrap();
    assert_eq!(
        camera(&host),
        [
            CameraCommand::PanTo {
                tile: 0,
                duration_ms: 0
            },
            CameraCommand::PanTo {
                tile: 39999,
                duration_ms: 0
            },
            CameraCommand::SetZoom {
                percent: 1,
                duration_ms: 0
            },
            CameraCommand::Shake {
                intensity: 0,
                duration_ms: 0
            },
            CameraCommand::Reset { duration_ms: 0 },
        ]
    );
}

#[test]
fn enhanced_refuses_invalid_operands_with_diagnostics_and_runs_on() {
    let mut host = host(EnginePreset::Enhanced);
    let code = [
        request(0x8210, &[-1, 100]),
        request(0x8210, &[40000, 100]),
        request(0x8210, &[20100, -1]),
        request(0x8211, &[0, 100]),
        request(0x8211, &[-50, 100]),
        request(0x8211, &[150, -1]),
        request(0x8212, &[-1, 100]),
        request(0x8212, &[4, -1]),
        request(0x8213, &[-1]),
        request(0x8213, &[250]),
    ]
    .concat();
    let mut vm = fragment(&code);
    assert_eq!(vm.run(&mut host, 100).unwrap(), RunOutcome::FragmentEnd);
    assert!(!vm.status.contains(VmStatus::ERROR));
    assert_eq!(vm.value_stack.byte_len(), 0);
    // Only the last request is made.
    assert_eq!(camera(&host), [CameraCommand::Reset { duration_ms: 250 }]);
    assert_eq!(
        diagnostics(&host),
        [
            "op_cam_pan_to: hex -1 outside 0-39999",
            "op_cam_pan_to: hex 40000 outside 0-39999",
            "op_cam_pan_to: negative duration -1",
            "op_cam_set_zoom: zoom 0% is not positive",
            "op_cam_set_zoom: zoom -50% is not positive",
            "op_cam_set_zoom: negative duration -1",
            "op_cam_shake: negative intensity -1",
            "op_cam_shake: negative duration -1",
            "op_cam_reset: negative duration -1",
        ]
    );
}

#[test]
fn a_hand_built_map_script_makes_its_requests_once_at_map_entry() {
    // The demo program through `TestAssets`: `start` runs with 15 on the
    // map script's first request (step 6) and makes the requests; the
    // startup (step 8) and update (step 9) requests skip them.
    let program = IntProgram::parse_enhanced(&fixture::cam_demo_program()).unwrap();
    let mut map = empty_map();
    map.header.script_reference = 1;
    let assets = TestAssets {
        programs: vec![("scripts/camdemo.int".into(), program)],
        any_map: Some(map),
        ..TestAssets::default()
    };
    let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let mut host = WorldHost::new(world, assets);
    host.set_preset(EnginePreset::Enhanced);
    host.load_map("maps/a.map").unwrap();
    assert_eq!(camera(&host), DEMO);
    assert!(
        diagnostics(&host)
            .iter()
            .all(|message| !message.contains("op_cam")),
        "{:?}",
        diagnostics(&host)
    );
}

/// A world on elevation 0 with the player at `player` (unplaced if `None`)
/// and a scroll blocker on each of `blockers` (hex, elevation, owned).
fn region_world(player: Option<i32>, blockers: &[(i32, u8, bool)]) -> WorldState {
    let mut world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let reference = world.player;
    match player {
        Some(hex) => world.move_object(reference, hex, 0, false).unwrap(),
        None => world.get_mut(reference).unwrap().raw.hex = None,
    }
    let mut owner = None;
    for (hex, elevation, owned) in blockers {
        let mut raw = object_record(SCROLL_BLOCKER_PID, 0);
        raw.hex = Some(*hex);
        raw.elevation = *elevation;
        let reference = world
            .insert(WorldObject {
                raw,
                prototype: None,
                character: None,
                name: Vec::new(),
                description: Vec::new(),
                owner: if *owned { owner } else { None },
                inventory: Vec::new(),
                money: None,
                active: true,
                opponent: 0,
            })
            .unwrap();
        owner.get_or_insert(reference);
    }
    world
}
/// The hexes of the ring of columns and rows `first` and `last`.
fn ring(first: i32, last: i32) -> Vec<i32> {
    (first..=last)
        .flat_map(|index| {
            [
                index + 200 * first,
                index + 200 * last,
                first + 200 * index,
                last + 200 * index,
            ]
        })
        .collect()
}
fn hex(column: i32, row: i32) -> i32 {
    column + 200 * row
}

#[test]
fn a_ring_of_scroll_blockers_encloses_the_region() {
    let blockers: Vec<_> = ring(80, 120)
        .into_iter()
        .map(|hex| (hex, 0, false))
        .collect();
    let world = region_world(Some(hex(100, 100)), &blockers);
    let region = CameraRegion::compute(&world);
    let inside = |hex: i32| {
        let (column, row) = (hex % 200, hex / 200);
        (81..=119).contains(&column) && (81..=119).contains(&row)
    };
    for hex in 0..40000 {
        assert_eq!(region.contains(hex), inside(hex), "hex {hex}");
    }
    assert_eq!(region.allowed().iter().filter(|ok| **ok).count(), 39 * 39);
    // The bounds are the interior's references.
    let grid = HexGrid::default();
    let references: Vec<[i64; 2]> = (0..40000)
        .filter(|hex| inside(*hex))
        .map(|hex| grid.hex_reference(hex).unwrap())
        .collect();
    let min = |axis: usize| references.iter().map(|r| r[axis]).min().unwrap();
    let max = |axis: usize| references.iter().map(|r| r[axis]).max().unwrap();
    assert_eq!(region.bounds, [min(0), min(1), max(0), max(1)]);
    assert_eq!(region.clamp([-1e9, 1e9]), [min(0) as f64, max(1) as f64]);
    // A gap in the ring lets the fill out; then only the border bounds it.
    let mut open = blockers.clone();
    open.retain(|(hex, _, _)| *hex != self::hex(100, 80));
    let leaking = CameraRegion::compute(&region_world(Some(hex(100, 100)), &open));
    assert!(leaking.contains(hex(100, 60)));
    // Owned blockers, and blockers on another elevation, do not count.
    for (elevation, owned) in [(1, false), (0, true)] {
        let mut others = blockers.clone();
        for blocker in others
            .iter_mut()
            .filter(|(hex, _, _)| *hex == self::hex(100, 80))
        {
            *blocker = (blocker.0, elevation, owned);
        }
        // An owned blocker is owned by the first one, at (80, 80).
        let region = CameraRegion::compute(&region_world(Some(hex(100, 100)), &others));
        assert!(region.contains(hex(100, 60)), "{elevation} {owned}");
    }
    // The player outside the ring: the region is the outside.
    let outside = CameraRegion::compute(&region_world(Some(hex(60, 60)), &blockers));
    assert!(outside.contains(hex(60, 60)) && !outside.contains(hex(100, 100)));
}

#[test]
fn the_map_border_bounds_an_open_map() {
    let world = region_world(Some(hex(100, 100)), &[]);
    let region = CameraRegion::compute(&world);
    for hex in 0..40000 {
        assert_eq!(region.contains(hex), cursor::inside_border(hex), "{hex}");
    }
    // Columns 45-155 and rows 45-154.
    assert_eq!(region.allowed().iter().filter(|ok| **ok).count(), 111 * 110);
    // An unplaced player: the fill starts from the view center.
    let mut world = region_world(
        None,
        &ring(80, 120)
            .into_iter()
            .map(|h| (h, 0, false))
            .collect::<Vec<_>>(),
    );
    assert!(world.set_view_center(hex(70, 70)));
    let region = CameraRegion::compute(&world);
    assert!(region.contains(hex(70, 70)) && !region.contains(hex(100, 100)));
    // A player outside the border: the view center again.
    let mut world = region_world(Some(hex(20, 20)), &[]);
    assert!(world.set_view_center(hex(100, 100)));
    assert_eq!(CameraRegion::compute(&world), region_for_border());
    // Neither passes: the start hex alone, so the view holds still.
    let mut world = region_world(Some(hex(20, 20)), &[(hex(100, 100), 0, false)]);
    world.camera_hex = hex(100, 100);
    let region = CameraRegion::compute(&world);
    assert_eq!(region.allowed().iter().filter(|ok| **ok).count(), 1);
    assert!(region.contains(hex(20, 20)));
    let reference = HexGrid::default().hex_reference(hex(20, 20)).unwrap();
    assert_eq!(
        region.bounds,
        [reference[0], reference[1], reference[0], reference[1]]
    );
}
fn region_for_border() -> CameraRegion {
    CameraRegion::from_allowed((0..40000).map(cursor::inside_border).collect())
}

#[test]
fn the_unbounded_region_holds_every_hex() {
    let region = CameraRegion::unbounded();
    assert!((0..40000).all(|hex| region.contains(hex)));
    assert!(!region.contains(-1) && !region.contains(40000));
    let grid = HexGrid::default();
    let corners = [0, 199, 39800, 39999].map(|hex| grid.hex_reference(hex).unwrap());
    let min = |axis: usize| corners.iter().map(|r| r[axis]).min().unwrap();
    let max = |axis: usize| corners.iter().map(|r| r[axis]).max().unwrap();
    assert!(region.bounds[0] <= min(0) && region.bounds[1] <= min(1));
    assert!(region.bounds[2] >= max(0) && region.bounds[3] >= max(1));
    let inside = [region.bounds[0] as f64 + 1.0, region.bounds[3] as f64 - 1.0];
    assert_eq!(region.clamp(inside), inside);
    // An empty region has zero bounds.
    assert_eq!(
        CameraRegion::from_allowed(vec![false; 40000]).bounds,
        [0; 4]
    );
}

#[test]
fn the_region_depends_on_the_world_alone() {
    let blockers: Vec<_> = ring(70, 130)
        .into_iter()
        .map(|hex| (hex, 0, false))
        .collect();
    let mut reversed = blockers.clone();
    reversed.reverse();
    let first = CameraRegion::compute(&region_world(Some(hex(100, 100)), &blockers));
    let again = CameraRegion::compute(&region_world(Some(hex(100, 100)), &blockers));
    let other_order = CameraRegion::compute(&region_world(Some(hex(100, 100)), &reversed));
    // Any start inside the enclosure gives the same region.
    let elsewhere = CameraRegion::compute(&region_world(Some(hex(75, 125)), &blockers));
    assert_eq!(first, again);
    assert_eq!(first, other_order);
    assert_eq!(first, elsewhere);
}

#[test]
fn a_whole_grid_fill_is_fast() {
    // Every hex passes: the fill visits all 40000 hexes.
    let mut best = std::time::Duration::MAX;
    for _ in 0..5 {
        let started = std::time::Instant::now();
        let reached = flood(hex(100, 100), |_| true);
        best = best.min(started.elapsed());
        assert!(reached.iter().all(|ok| *ok));
    }
    // Well under 5 ms optimized; unoptimized builds get more room.
    let limit = if cfg!(debug_assertions) { 50 } else { 5 };
    assert!(best.as_millis() < limit, "{best:?}");
    let world = region_world(Some(hex(100, 100)), &[]);
    let started = std::time::Instant::now();
    CameraRegion::compute(&world);
    assert!(started.elapsed().as_millis() < limit * 4);
}

#[test]
fn the_camera_demo_loads_and_requests_its_camera_under_enhanced() {
    let dir = tempfile::tempdir().unwrap();
    fixture::write_corpus(dir.path());
    let list = std::fs::read(dir.path().join("scripts/scripts.lst")).unwrap();
    let index = fixture::write_cam_demo(dir.path());
    // Appended once, after the corpus's own rows.
    assert_eq!(index, 2);
    assert_eq!(fixture::write_cam_demo(dir.path()), 2);
    let appended = std::fs::read(dir.path().join("scripts/scripts.lst")).unwrap();
    assert_eq!(
        appended,
        [list, b"camdemo.int # local_vars=0\n".to_vec()].concat()
    );
    let load = |preset: EnginePreset| {
        let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
        let mut host = WorldHost::new(world, dir_assets(dir.path()));
        host.set_preset(preset);
        host.load_campaign().unwrap();
        host.load_map("maps/cam_demo.map").unwrap();
        host
    };
    let host = load(EnginePreset::Enhanced);
    assert_eq!(camera(&host), DEMO);
    assert!(
        diagnostics(&host)
            .iter()
            .all(|message| !message.contains("camdemo")
                && !message.contains("op_cam")
                && !message.contains("map-load")),
        "{:?}",
        diagnostics(&host)
    );
    let check_map = |host: &WorldHost<_>| {
        let world: &WorldState = &host.world;
        assert_eq!(&world.map.header.name[..12], b"CAM_DEMO.MAP");
        let player = world.get(world.player).unwrap();
        assert_eq!(player.raw.hex, Some(fixture::CAM_DEMO_ENTRY));
        let at = |pid: u32, hex: i32| {
            world
                .iter()
                .any(|(_, object)| object.raw.pid == pid && object.raw.hex == Some(hex))
        };
        assert!(at(0x0100_0001, fixture::CAM_DEMO_CRITTER));
        assert!(at(0x0000_0001, fixture::CAM_DEMO_ITEM));
        // The interior is roofed over floor; the doorway is open.
        let tiles = world.map.tiles[0].as_ref().unwrap();
        for hex in [fixture::CAM_DEMO_INTERIOR, fixture::CAM_DEMO_CRITTER] {
            let (column, row) = crate::roof::roof_square(hex);
            let tile = tiles[crate::roof::square_position(column, row).unwrap()];
            assert_eq!((tile.roof(), tile.floor()), (2, 0));
        }
        let (column, row) = crate::roof::roof_square(fixture::CAM_DEMO_ENTRY);
        let tile = tiles[crate::roof::square_position(column, row).unwrap()];
        assert_eq!((tile.roof(), tile.floor()), (1, 0));
        assert!(!at(0x0300_0001, fixture::CAM_DEMO_DOOR));
        assert!(at(0x0300_0001, fixture::CAM_DEMO_DOOR - 1));
        // The blockers enclose the hexes inside the ring.
        let region = CameraRegion::compute(world);
        let [first, last] = fixture::CAM_DEMO_RING;
        let inside = |hex: i32| {
            let (column, row) = (hex % 200, hex / 200);
            (first + 1..last).contains(&column) && (first + 1..last).contains(&row)
        };
        assert!((0..40000).all(|hex| region.contains(hex) == inside(hex)));
        assert!(region.contains(fixture::CAM_DEMO_INTERIOR));
    };
    check_map(&host);
    // Vanilla: the script fails to parse with the unknown-word error and
    // nothing runs; the map loads all the same.
    let host = load(EnginePreset::Vanilla);
    assert!(camera(&host).is_empty());
    assert!(
        diagnostics(&host)
            .iter()
            .any(|message| message.contains("invalid instruction 0x8210")),
        "{:?}",
        diagnostics(&host)
    );
    check_map(&host);
}

#[test]
fn the_camera_demo_map_is_a_well_formed_map() {
    // The demo map reads and writes back unchanged through vault-formats,
    // with the corpus's prototypes.
    let dir = tempfile::tempdir().unwrap();
    fixture::write_corpus(dir.path());
    fixture::write_cam_demo(dir.path());
    let assets = dir_assets(dir.path());
    let bytes = std::fs::read(dir.path().join("maps/cam_demo.map")).unwrap();
    let map = assets.map("maps/cam_demo.map").unwrap();
    assert!(map.diagnostics.is_empty(), "{:?}", map.diagnostics);
    assert_eq!(map.write().unwrap(), bytes);
    assert_eq!(map.header.script_reference, 3);
    assert_eq!(map.header.entry_hex, fixture::CAM_DEMO_ENTRY);
    assert!(map.tiles[1].is_none() && map.tiles[2].is_none());
    let blockers = map.objects.elevations[0]
        .iter()
        .filter(|object| object.pid == SCROLL_BLOCKER_PID)
        .count();
    assert_eq!(blockers, 4 * 80);
}
