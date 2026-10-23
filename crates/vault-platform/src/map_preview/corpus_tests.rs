//! Live-corpus regressions for `--game` behavior, run with
//! `cargo test -p vault-platform -- --ignored` beside a `local-corpus`.
use super::MapPreview;
use std::sync::Arc;
use vault_core::{ObjectRef, WorldEffect};
use vault_formats::{
    LocalDirProvider, Vfs,
    map::{ObjectDetail, ObjectState},
    pro::Category,
};

/// The unpacked corpus: `$VAULT_CORPUS`, else `local-corpus` beside the
/// repository; `None` skips the test.
fn corpus() -> Option<Arc<dyn Vfs>> {
    let dir = std::env::var_os("VAULT_CORPUS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../local-corpus")
        })
        .join("fallout1-unpacked");
    if !dir.is_dir() {
        eprintln!("skipping: no local corpus at {}", dir.display());
        return None;
    }
    Some(Arc::new(LocalDirProvider::mount(dir).unwrap()))
}

fn map(name: &str) -> Option<MapPreview> {
    let vfs = corpus()?;
    let mut preview = MapPreview::load(vfs, &format!("maps/{name}"), "english", None, 1).unwrap();
    preview.host.world.effects.clear();
    Some(preview)
}

fn hex(preview: &MapPreview, object: ObjectRef) -> Option<i32> {
    preview.host.world.get(object)?.raw.hex
}

/// Tick the host in 50 ms frames until `done` or `limit_ms`; returns the
/// time reached.
fn run(preview: &mut MapPreview, limit_ms: u64, mut done: impl FnMut(&MapPreview) -> bool) -> u64 {
    let mut now = 1000;
    while now < 1000 + limit_ms && !done(preview) {
        preview.host.tick(now).unwrap();
        now += 50;
    }
    now - 1000
}

/// A hostile animal's critter_p_proc runs on the background tick and
/// attacks a player beside it, which starts combat.
fn animals_start_combat(name: &str) {
    let Some(mut preview) = map(name) else {
        return;
    };
    let player = preview.host.world.player;
    let animal = preview
        .host
        .world
        .iter()
        .filter(|(reference, object)| {
            *reference != player
                && object.raw.hex.is_some()
                && object.raw.elevation == preview.host.world.elevation
                && Category::from_pid(object.raw.pid).ok() == Some(Category::Critter)
        })
        .map(|(reference, _)| reference)
        .next()
        .expect("an animal");
    let beside = vault_core::routing::step(hex(&preview, animal).unwrap(), 0);
    let elevation = preview.host.world.elevation;
    preview
        .host
        .world
        .move_object(player, beside, elevation, false)
        .unwrap();
    let elapsed = run(&mut preview, 5000, |preview| preview.host.world.combat);
    assert!(
        preview.host.world.combat,
        "{name}: no combat after {elapsed} ms"
    );
}

#[test]
#[ignore = "needs the local corpus"]
fn v13ent_cave_rats_start_combat_when_the_player_comes_close() {
    animals_start_combat("v13ent.map");
}

#[test]
#[ignore = "needs the local corpus"]
fn v13ent_walking_across_the_cave_exit_grid_leaves_on_that_step() {
    let Some(mut preview) = map("v13ent.map") else {
        return;
    };
    let player = preview.host.world.player;
    let elevation = preview.host.world.elevation;
    // The cave mouth's grids lead to the world map (-2).
    let grids: Vec<i32> = preview
        .host
        .world
        .iter()
        .filter(|(_, object)| object.raw.elevation == elevation)
        .filter_map(|(_, object)| match object.raw.state {
            ObjectState::Other {
                detail: ObjectDetail::ExitGrid { map: -2, .. },
                ..
            } => object.raw.hex,
            _ => None,
        })
        .collect();
    assert!(!grids.is_empty());
    // Stand two hexes before a grid and walk to two hexes past it, so the
    // grid hex lies inside a multi-hex walk.
    let start = hex(&preview, player).unwrap();
    let step = vault_core::routing::step;
    let (before, grid, beyond) = grids
        .iter()
        .flat_map(|grid| (0u8..6).map(move |direction| (*grid, direction)))
        .find_map(|(grid, direction)| {
            let back = (direction + 3) % 6;
            let before = step(step(grid, back), back);
            let beyond = step(step(grid, direction), direction);
            let world = &preview.host.world;
            let clear = |at: i32| world.movement_blocker(at, elevation, player).is_none();
            let route = world.route(player, before, beyond, true);
            (clear(before)
                && !grids.contains(&before)
                && !world.route(player, start, before, true).is_empty()
                && route.len() == 4
                && route.iter().all(|turn| *turn == direction))
            .then_some((before, grid, beyond))
        })
        .expect("a straight walk across a grid");
    preview
        .host
        .world
        .move_object(player, before, elevation, false)
        .unwrap();
    preview.host.world.effects.clear();
    assert!(preview.host.walk_toward(player, beyond, false, 0).unwrap());
    let mut left = false;
    let elapsed = run(&mut preview, 20000, |preview| {
        left |= preview.host.world.effects.contains(&WorldEffect::WorldMap);
        left || !preview.host.moving(preview.host.world.player)
    });
    assert!(
        left,
        "no world map after {elapsed} ms walking {before} -> {beyond}"
    );
    // The walk stops on the first grid hex of its path.
    let direction = vault_core::routing::rotation(before, grid).unwrap();
    let first = std::iter::successors(Some(before), |at| Some(step(*at, direction)))
        .take(5)
        .find(|at| grids.contains(at));
    assert_eq!(hex(&preview, player), first, "the walk stops on the grid");
}

#[test]
#[ignore = "needs the local corpus"]
fn egg_mask_matches_spec014_and_the_cave_view_renders() {
    let Some(mut preview) = map("v13ent.map") else {
        return;
    };
    let egg = preview.egg_mask().expect("egg.frm");
    let frame = egg.frame(0, 0).unwrap();
    assert_eq!((frame.width, frame.height), (129, 98));
    let pixels = frame.pixels();
    assert_eq!(pixels.iter().filter(|weight| **weight == 1).count(), 4716);
    assert_eq!(pixels.iter().copied().max(), Some(122));
    assert_eq!(egg.shifts, [[0, 25]; 6]);
    // Inside the cave mouth the rock wall is drawn over the player (object
    // order) and the cutout reveals the player through it. `VAULT_VIEW_PPM`
    // saves the view for inspection.
    let player = preview.host.world.player;
    preview
        .host
        .world
        .move_object(player, 23353, 0, false)
        .unwrap();
    preview.view.grid = vault_formats::HexGrid::centered(23353, 640, 380).unwrap();
    let image = preview.map_view_image().unwrap();
    preview.egg = Some(None);
    let plain = preview.map_view_image().unwrap();
    let changed = image
        .pixels()
        .iter()
        .zip(plain.pixels())
        .filter(|(a, b)| a != b)
        .count();
    assert!(changed > 1000, "the cutout changed only {changed} pixels");
    if let Some(path) = std::env::var_os("VAULT_VIEW_PPM") {
        let rgba = preview.palette.rgba8();
        let mut ppm = format!("P6 {} {} 255\n", image.width(), image.height()).into_bytes();
        for index in image.pixels() {
            ppm.extend(&rgba[usize::from(*index)][..3]);
        }
        std::fs::write(path, ppm).unwrap();
    }
}
