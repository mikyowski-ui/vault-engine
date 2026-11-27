//! Pre-rendered map backdrops in the shell (owner brief
//! `docs/prerendered-backdrop-cutaway.md`), over the synthetic corpus with
//! `.bkd`/`.bkc` files built by `vault_fixtures::backdrop`: where the
//! pixels land in the Vanilla view and the `enhanced` canvas at several
//! zooms, on both render paths; the player fade; suppression; the refused
//! files; the chunk cache; picking; and that no world value changes.
use super::*;
use crate::map_preview::{
    MapPreview, SharedRenderer,
    test_support::{cpu_renderer, gpu_lock, gpu_renderer, load, synthetic_corpus},
};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use vault_core::ObjectRef;
use vault_fixtures::{
    backdrop::{Backdrop as Files, Elevation, build},
    world as fixture,
};
use vault_formats::pro::Category;
use vault_render::{IndexedImage, map::object_sprite};
use winit::keyboard::KeyCode;

/// An unlit index (229-255 never go through the intensity table, brief
/// §3.7), so the expected canvas bytes need no light.
const FLOOR: u8 = 240;
const GROUP: u8 = 241;

/// The synthetic corpus with its map also as `maps/synth.map`, whose name
/// fits a save slot; the backdrop files are named after that stem.
fn corpus() -> tempfile::TempDir {
    let corpus = synthetic_corpus();
    std::fs::copy(
        corpus.path().join("maps/synthetic.map"),
        corpus.path().join("maps/synth.map"),
    )
    .unwrap();
    corpus
}

fn open(root: &Path) -> MapPreview {
    load(root, "maps/synth.map")
}

/// The SPEC-021 §2.1 hash of the corpus's raw `color.pal`.
fn palette_hash(root: &Path) -> u64 {
    vault_core::state_hash::fnv1a64(&std::fs::read(root.join("color.pal")).unwrap())
}

/// Write `files` as `maps/synth.bkd` and its chunk files.
fn write(root: &Path, files: &Files) {
    std::fs::write(root.join("maps/synth.bkd"), &files.index).unwrap();
    for (elevation, column, row, bytes) in &files.chunks {
        let dir = root.join(format!("maps/synth/e{elevation}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{column}_{row}.bkc")), bytes).unwrap();
    }
}

/// One elevation of `size` pixels of `colour`, placed with image pixel
/// `origin_px` at hex `origin_hex`'s reference corner; every pixel has
/// depth `depth` (none: no depth plane) and, with `fade`, its fade bit.
fn uniform(
    size: [u32; 2],
    origin_hex: i32,
    origin_px: [i32; 2],
    colour: u8,
    depth: Option<u16>,
    fade: bool,
) -> Elevation {
    let pixels = (size[0] * size[1]) as usize;
    Elevation {
        elevation: 0,
        width: size[0],
        height: size[1],
        origin_px,
        origin_hex: origin_hex as u32,
        colour: vec![colour; pixels],
        depth: depth.map(|depth| vec![depth; pixels]),
        fade: fade.then(|| vec![true; pixels]),
        suppression: 0,
        pids: Vec::new(),
    }
}

/// The player and its hex.
fn player(preview: &MapPreview) -> (ObjectRef, i32) {
    let world = &preview.host.world;
    let hex = world.get(world.player).unwrap().raw.hex.unwrap();
    (world.player, hex)
}

/// The first object of `category`.
fn first(preview: &MapPreview, category: Category) -> ObjectRef {
    preview
        .host
        .world
        .iter()
        .find(|(_, object)| object.category() == category)
        .unwrap()
        .0
}

/// `object`'s sprite in `view`: its corner and size.
fn sprite(preview: &MapPreview, view: MapView, object: ObjectRef) -> ([i64; 2], [i64; 2]) {
    let raw = &preview.host.world.get(object).unwrap().raw;
    let (frame, origin) = object_sprite(view, &preview.art, raw).unwrap();
    (origin, [frame.width, frame.height].map(i64::from))
}

/// The canvas position of image pixel (0, 0) in `view`.
fn offset(view: &MapView, origin_hex: i32, origin_px: [i32; 2]) -> [i64; 2] {
    let reference = view.grid.hex_reference(origin_hex).unwrap();
    [
        reference[0] - i64::from(origin_px[0]),
        reference[1] - i64::from(origin_px[1]),
    ]
}

/// The canvas pixel at `[x, y]`.
fn at(image: &IndexedImage, [x, y]: [i64; 2]) -> u8 {
    image.pixels()[y as usize * image.width() as usize + x as usize]
}

/// A floor-pass backdrop (depth 0) of `colour` drew at `corner` with
/// `size`: outside that rectangle `image` is `baseline`; inside, each pixel
/// is `colour` or what `baseline` drew there (an object over the floor),
/// and most are `colour`, which `baseline` never shows.
fn assert_floor_at(
    image: &IndexedImage,
    baseline: &IndexedImage,
    corner: [i64; 2],
    size: [u32; 2],
) {
    assert_eq!(
        (image.width(), image.height()),
        (baseline.width(), baseline.height())
    );
    let (width, height) = (i64::from(image.width()), i64::from(image.height()));
    let (mut inside, mut shown) = (0, 0);
    for y in 0..height {
        for x in 0..width {
            let (pixel, base) = (at(image, [x, y]), at(baseline, [x, y]));
            let within = (corner[0]..corner[0] + i64::from(size[0])).contains(&x)
                && (corner[1]..corner[1] + i64::from(size[1])).contains(&y);
            if within {
                inside += 1;
                assert_ne!(base, FLOOR, "the baseline shows the backdrop colour");
                assert!(pixel == FLOOR || pixel == base, "({x}, {y}): {pixel}");
                shown += usize::from(pixel == FLOOR);
            } else {
                assert_eq!(pixel, base, "({x}, {y}) outside the backdrop");
            }
        }
    }
    assert!(inside > 0, "the backdrop is off the canvas");
    assert!(shown * 10 >= inside * 8, "{shown} of {inside}");
}

/// A capturing subscriber's output while `run` runs: the warnings about
/// backdrops, one per line.
fn backdrop_warnings<T>(run: impl FnOnce() -> T) -> (T, Vec<String>) {
    #[derive(Clone, Default)]
    struct Logs(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Logs {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let logs = Logs::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .finish();
    let value = tracing::subscriber::with_default(subscriber, run);
    let text = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    let lines = text
        .lines()
        .filter(|line| line.contains("map backdrop"))
        .map(str::to_owned)
        .collect();
    (value, lines)
}

/// A backdrop of 300 by 200 floor pixels around the player, in chunks of
/// 64 (5 by 4 chunks, zlib).
fn floor_backdrop(root: &Path) -> (i32, [i32; 2], [u32; 2]) {
    let hex = player(&open(root)).1;
    let (origin_px, size) = ([150, 100], [300, 200]);
    let files = build(
        64,
        true,
        palette_hash(root),
        &[uniform(size, hex, origin_px, FLOOR, None, false)],
    );
    write(root, &files);
    (hex, origin_px, size)
}

/// Scroll the Vanilla view right and down a few steps through the arrow
/// keys' gate.
fn scroll(preview: &mut MapPreview, now: &mut u64) {
    for key in [KeyCode::ArrowRight, KeyCode::ArrowDown] {
        for _ in 0..3 {
            *now += 40;
            preview.tick(*now).unwrap();
            preview.step(key);
        }
    }
}

/// The Vanilla view's frames at the start and after a scroll, with the
/// view each was drawn in.
fn vanilla_frames(root: &Path, renderer: Option<SharedRenderer>) -> Vec<(MapView, IndexedImage)> {
    let mut preview = open(root);
    if let Some(renderer) = renderer {
        preview.set_scene_renderer(renderer);
    }
    let mut now = 16;
    preview.tick(now).unwrap();
    let first = (preview.view, preview.map_view_image().unwrap());
    scroll(&mut preview, &mut now);
    let second = (preview.view, preview.map_view_image().unwrap());
    assert_ne!(
        first.0.grid.origin, second.0.grid.origin,
        "the arrow keys scrolled the view"
    );
    vec![first, second]
}

/// Brief §3.5 item 1 in the Vanilla view: the floor pixels show where the
/// index places them, in the classic view and after a scroll, the same on
/// the reference path and the instanced path's CPU executor (and the GPU
/// when there is one).
#[test]
fn the_backdrop_shows_in_place_in_the_vanilla_view_on_both_paths() {
    let plain = corpus();
    let baseline = vanilla_frames(plain.path(), None);
    let dir = corpus();
    let (hex, origin_px, size) = floor_backdrop(dir.path());
    let reference = vanilla_frames(dir.path(), None);
    for ((view, image), (base_view, base)) in reference.iter().zip(&baseline) {
        assert_eq!(view.grid.origin, base_view.grid.origin);
        assert_floor_at(image, base, offset(view, hex, origin_px), size);
    }
    let renderer = cpu_renderer();
    let instanced = vanilla_frames(dir.path(), Some(renderer.clone()));
    for (index, ((_, expected), (_, image))) in reference.iter().zip(&instanced).enumerate() {
        assert!(
            expected.pixels() == image.pixels(),
            "frame {index} differs between the paths"
        );
    }
    let totals = renderer.session_totals();
    assert_eq!(
        totals.refused,
        0,
        "{totals:?} {:?}",
        renderer.logged_causes()
    );
    assert!(totals.instanced >= 2, "{totals:?}");
    let _gpu = gpu_lock();
    if let Some(gpu) = gpu_renderer() {
        let renderer = SharedRenderer::from_renderer(gpu);
        let frames = vanilla_frames(dir.path(), Some(renderer.clone()));
        for (index, ((_, expected), (_, image))) in reference.iter().zip(&frames).enumerate() {
            assert!(
                expected.pixels() == image.pixels(),
                "GPU frame {index} differs from the reference"
            );
        }
        println!("GPU backdrop frames: {:?}", renderer.session_totals());
        drop(frames);
        drop(renderer);
    }
}

/// The unlocked `enhanced` canvas (1920 by 1080 surface) at `zoom`, and
/// the view it was drawn in.
fn enhanced_canvas(
    root: &Path,
    zoom: f64,
    renderer: Option<SharedRenderer>,
) -> (MapView, IndexedImage) {
    let mut preview = open(root);
    if let Some(renderer) = renderer {
        preview.set_scene_renderer(renderer);
    }
    preview.set_enhanced(Some(crate::enhanced::EnhancedSettings {
        smooth_camera: false,
        ..Default::default()
    }));
    preview.tick(16).unwrap();
    preview.enhanced.as_mut().unwrap().camera.zoom = zoom;
    let layout =
        vault_render::EnhancedLayout::compute([1920, 1080], None, preview.enhanced_zoom()).unwrap();
    let frame = preview.enhanced_frame(&layout).unwrap().unwrap();
    let view = preview.enhanced_view().unwrap();
    assert_eq!(
        [view.width, view.height],
        [frame.world.width(), frame.world.height()]
    );
    (view, frame.world)
}

/// Every tile of the unlocked canvas draws the backdrop, at zoom 1, 0.5
/// and 2: the zoom changes the canvas and its tiles, not the backdrop. The
/// instanced path's canvases are the reference path's.
#[test]
fn the_backdrop_shows_in_place_on_the_enhanced_canvas_at_every_zoom() {
    let plain = corpus();
    let dir = corpus();
    let (hex, origin_px, size) = floor_backdrop(dir.path());
    let mut sizes = Vec::new();
    for zoom in [1.0, 0.5, 2.0] {
        let (base_view, base) = enhanced_canvas(plain.path(), zoom, None);
        let (view, image) = enhanced_canvas(dir.path(), zoom, None);
        assert_eq!(view.grid.origin, base_view.grid.origin, "zoom {zoom}");
        assert_floor_at(&image, &base, offset(&view, hex, origin_px), size);
        let (_, instanced) = enhanced_canvas(dir.path(), zoom, Some(cpu_renderer()));
        assert!(
            instanced.pixels() == image.pixels(),
            "zoom {zoom}: the paths differ"
        );
        sizes.push([view.width, view.height]);
    }
    assert!(
        sizes[1][0] > sizes[0][0] && sizes[2][0] < sizes[0][0],
        "{sizes:?}"
    );
}

/// The corpus with the see-through cutout's mask: `egg.frm`, interface
/// art row 2, 129 by 98 at weight 61 throughout.
fn with_egg(root: &Path) {
    let dir = root.join("art/intrface");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("intrface.lst"), b"none0.frm\nnone1.frm\negg.frm\n").unwrap();
    std::fs::write(dir.join("egg.frm"), fixture::frm(129, 98, 61)).unwrap();
}

/// The Vanilla view of a group of hex `group` over the player (400 by 300
/// pixels around them), with or without fade bits; and the player's
/// sprite centre.
fn group_over_player(group: i32, fade: bool) -> (IndexedImage, IndexedImage, [i64; 2]) {
    let plain = corpus();
    with_egg(plain.path());
    let mut baseline = open(plain.path());
    baseline.tick(16).unwrap();
    let base = baseline.map_view_image().unwrap();
    let (who, hex) = player(&baseline);
    let (corner, size) = sprite(&baseline, baseline.view, who);
    let centre = [corner[0] + size[0] / 2, corner[1] + size[1] / 2];
    let dir = corpus();
    with_egg(dir.path());
    let files = build(
        128,
        false,
        palette_hash(dir.path()),
        &[uniform(
            [400, 300],
            hex,
            [200, 150],
            GROUP,
            Some(group as u16 + 1),
            fade,
        )],
    );
    write(dir.path(), &files);
    let mut preview = open(dir.path());
    preview.tick(16).unwrap();
    (preview.map_view_image().unwrap(), base, centre)
}

/// Brief §3.6: a foreground group (fade bits, group hex past the
/// player's) over the player's sprite draws through the player's mask, so
/// neither its colour nor the player's alone shows there; without fade
/// bits it covers the player; a group behind the player never fades and
/// the player draws over it.
#[test]
fn a_foreground_group_fades_through_the_player_mask_and_one_behind_does_not() {
    let hex = {
        let dir = corpus();
        player(&open(dir.path())).1
    };
    let (front, base, centre) = group_over_player(hex + 1000, true);
    let (pixel, player_pixel) = (at(&front, centre), at(&base, centre));
    assert_ne!(player_pixel, GROUP);
    assert!(
        pixel != GROUP && pixel != player_pixel,
        "the fade blends: {pixel} (player {player_pixel})"
    );
    // Far from the player (outside the mask) the group is opaque.
    assert_eq!(at(&front, [centre[0] - 180, centre[1] - 130]), GROUP);
    let (opaque, _, _) = group_over_player(hex + 1000, false);
    assert_eq!(at(&opaque, centre), GROUP, "without fade bits it covers");
    let (behind, base, centre) = group_over_player(hex - 1000, true);
    assert_eq!(
        at(&behind, centre),
        at(&base, centre),
        "the player draws over it"
    );
    assert_eq!(at(&behind, [centre[0] - 180, centre[1] - 130]), GROUP);
}

/// Brief §3.8: suppression `1` stops drawing the wall, which keeps every
/// world role (it still blocks, and no world value changes), and is no
/// pick hit; elsewhere the canvas is unchanged.
#[test]
fn suppression_hides_a_wall_that_still_blocks() {
    let plain = corpus();
    let mut baseline = open(plain.path());
    baseline.tick(16).unwrap();
    let base = baseline.map_view_image().unwrap();
    let wall = first(&baseline, Category::Wall);
    let (corner, size) = sprite(&baseline, baseline.view, wall);
    let dir = corpus();
    let mut transparent = uniform([4, 4], 20100, [0, 0], 0, None, false);
    transparent.suppression = 1;
    write(
        dir.path(),
        &build(64, false, palette_hash(dir.path()), &[transparent]),
    );
    let mut preview = open(dir.path());
    preview.tick(16).unwrap();
    let image = preview.map_view_image().unwrap();
    let mut changed = 0;
    for y in 0..i64::from(image.height()) {
        for x in 0..i64::from(image.width()) {
            let within = (corner[0]..corner[0] + size[0]).contains(&x)
                && (corner[1]..corner[1] + size[1]).contains(&y);
            if at(&image, [x, y]) != at(&base, [x, y]) {
                assert!(within, "({x}, {y}) changed outside the wall");
                changed += 1;
            }
        }
    }
    assert!(changed > 0, "the wall is not drawn");
    let hex = preview.host.world.get(wall).unwrap().raw.hex.unwrap();
    let mover = player(&preview).0;
    assert_eq!(
        preview.host.world.sight_blocker(hex, 0),
        Some(wall),
        "it still stops sight"
    );
    assert_eq!(
        preview.host.world.movement_blocker(hex, 0, mover),
        baseline.host.world.movement_blocker(hex, 0, mover),
        "it blocks movement as before"
    );
    assert_eq!(
        preview.host.world.state_hash(),
        baseline.host.world.state_hash()
    );
    let point = [corner[0] + size[0] / 2, corner[1] + size[1] / 2].map(|v| v as i32);
    assert_eq!(baseline.object_at(point), Some(wall));
    assert_ne!(preview.object_at(point), Some(wall), "never a pick hit");
}

/// Brief §3.8.5: a sprite pixel a later group covers in the final canvas
/// is no pick hit for that sprite.
#[test]
fn a_covered_sprite_pixel_is_no_pick_hit() {
    let plain = corpus();
    let mut baseline = open(plain.path());
    baseline.tick(16).unwrap();
    let wall = first(&baseline, Category::Wall);
    let (corner, size) = sprite(&baseline, baseline.view, wall);
    let point = [corner[0] + size[0] / 2, corner[1] + size[1] / 2].map(|v| v as i32);
    assert_eq!(baseline.object_at(point), Some(wall));
    let hex = baseline.host.world.get(wall).unwrap().raw.hex.unwrap();
    let dir = corpus();
    let files = build(
        64,
        false,
        palette_hash(dir.path()),
        &[uniform(
            [200, 200],
            hex,
            [100, 100],
            GROUP,
            Some(40000),
            false,
        )],
    );
    write(dir.path(), &files);
    let mut preview = open(dir.path());
    preview.tick(16).unwrap();
    let image = preview.map_view_image().unwrap();
    assert_eq!(
        at(&image, point.map(i64::from)),
        GROUP,
        "the group covers it"
    );
    assert_ne!(preview.object_at(point), Some(wall));
}

/// Brief §4.8: a palette hash that differs and a malformed index each
/// draw the map as without a backdrop, with one warning however many
/// refreshes follow.
#[test]
fn a_refused_index_draws_without_the_backdrop_and_warns_once() {
    let plain = corpus();
    let mut baseline = open(plain.path());
    baseline.tick(16).unwrap();
    let base = baseline.map_view_image().unwrap();
    let hex = player(&baseline).1;
    let mismatch = corpus();
    let mut walls = uniform([300, 200], hex, [150, 100], FLOOR, None, false);
    walls.suppression = 1;
    write(
        mismatch.path(),
        &build(64, false, palette_hash(mismatch.path()) ^ 1, &[walls]),
    );
    let malformed = corpus();
    std::fs::write(
        malformed.path().join("maps/synth.bkd"),
        b"VBKE not an index",
    )
    .unwrap();
    for dir in [mismatch, malformed] {
        let (images, warnings) = backdrop_warnings(|| {
            let mut preview = open(dir.path());
            preview.tick(16).unwrap();
            let images: Vec<_> = (0..3)
                .map(|_| {
                    preview.thaw_view();
                    preview.map_view_image().unwrap()
                })
                .collect();
            assert!(!preview.backdrops.borrow().loaded());
            images
        });
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        for image in images {
            assert!(image.pixels() == base.pixels(), "drawn as without it");
        }
    }
}

/// Brief §4.9: a chunk the index lists but the layer lacks, or one that
/// does not decode, disables the elevation's backdrop for the session with
/// one warning, so it draws without the backdrop and without suppression.
#[test]
fn a_missing_or_malformed_chunk_disables_the_elevation_and_warns_once() {
    let plain = corpus();
    let mut baseline = open(plain.path());
    baseline.tick(16).unwrap();
    let base = baseline.map_view_image().unwrap();
    let hex = player(&baseline).1;
    for broken in ["missing", "malformed"] {
        let dir = corpus();
        let mut walls = uniform([300, 200], hex, [150, 100], FLOOR, None, false);
        walls.suppression = 1;
        write(
            dir.path(),
            &build(64, false, palette_hash(dir.path()), &[walls]),
        );
        let chunk = dir.path().join("maps/synth/e0/2_1.bkc");
        if broken == "missing" {
            std::fs::remove_file(&chunk).unwrap();
        } else {
            let mut bytes = std::fs::read(&chunk).unwrap();
            bytes.pop();
            std::fs::write(&chunk, bytes).unwrap();
        }
        let (images, warnings) = backdrop_warnings(|| {
            let mut preview = open(dir.path());
            preview.tick(16).unwrap();
            (0..3)
                .map(|_| {
                    preview.thaw_view();
                    preview.map_view_image().unwrap()
                })
                .collect::<Vec<_>>()
        });
        assert_eq!(warnings.len(), 1, "{broken}: {warnings:?}");
        for image in images {
            assert!(
                image.pixels() == base.pixels(),
                "{broken}: drawn as without it"
            );
        }
    }
}

/// Brief question 6: a listed PID whose prototype is a door (the
/// corpus's scenery prototype is one) is dropped from the suppression
/// list, with one warning, so the door still draws; the other listed PIDs
/// stay, sorted, and are suppressed.
#[test]
fn a_door_pid_is_never_suppressed() {
    let plain = corpus();
    let mut baseline = open(plain.path());
    baseline.tick(16).unwrap();
    let base = baseline.map_view_image().unwrap();
    let door = first(&baseline, Category::Scenery);
    let wall = first(&baseline, Category::Wall);
    let centre = |object| {
        let (corner, size) = sprite(&baseline, baseline.view, object);
        [corner[0] + size[0] / 2, corner[1] + size[1] / 2]
    };
    let (door_at, wall_at) = (centre(door), centre(wall));
    let dir = corpus();
    let mut listed = uniform([4, 4], 20100, [0, 0], 0, None, false);
    listed.suppression = 2;
    listed.pids = vec![0x0300_0001, 0x0200_0001, 0x0300_0001];
    write(
        dir.path(),
        &build(64, false, palette_hash(dir.path()), &[listed]),
    );
    let ((pids, image), warnings) = backdrop_warnings(|| {
        let mut preview = open(dir.path());
        preview.tick(16).unwrap();
        let image = preview.map_view_image().unwrap();
        preview.thaw_view();
        preview.map_view_image().unwrap();
        let view = preview.view;
        let backdrops = preview.backdrops.get_mut();
        let prepared = backdrops.prepare(&*preview.host.assets.vfs, &view).unwrap();
        let tiles = prepared.tiles();
        (prepared.view(&tiles).suppress_pids.to_vec(), image)
    });
    assert_eq!(pids, [0x0300_0001]);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("0x02000001"), "{warnings:?}");
    assert_eq!(at(&image, door_at), at(&base, door_at), "the door draws");
    assert_ne!(at(&image, wall_at), at(&base, wall_at), "the wall does not");
}

/// Brief §5.1: `state_hash` and a slot's every file are the same with and
/// without the backdrop files (the slot written with a fixed thumbnail:
/// the thumbnail is a picture of the canvas, which a backdrop changes).
#[test]
fn a_backdrop_changes_no_world_value_or_save_byte() {
    use crate::storage::{MemoryStorage, Namespace, StorageBackend};
    let slot = |root: &Path| {
        let storage: Arc<dyn StorageBackend> = Arc::new(MemoryStorage::new());
        let mut preview = open(root);
        preview.storage = Some(storage.clone());
        let mut now = 16;
        preview.tick(now).unwrap();
        preview.map_view_image().unwrap();
        scroll(&mut preview, &mut now);
        preview.map_view_image().unwrap();
        let info = vault_core::host_save::SaveInfo {
            description: b"Backdrop".to_vec(),
            host_date: [0; 3],
            host_time: 0,
            thumbnail: Some(vec![0; 224 * 133]),
        };
        preview.write_game_slot("SLOT01", &info).unwrap();
        let mut files = Vec::new();
        for key in storage.list(Namespace::Savegame, None).unwrap() {
            let bytes = storage.read(Namespace::Savegame, &key).unwrap().unwrap();
            files.push((key, bytes));
        }
        (
            preview.host.world.state_hash(),
            format!("{:?}", preview.host.world.rng.state()),
            files,
        )
    };
    let plain = corpus();
    let dir = corpus();
    floor_backdrop(dir.path());
    let (without, with) = (slot(plain.path()), slot(dir.path()));
    assert!(!with.2.is_empty());
    assert!(without == with, "a world value or a save byte differs");
}

/// A headless replay of `log` over `root`: the final `state_hash` and the
/// replay digest (which covers the frame).
fn replayed(root: &Path, log: &str) -> (u64, String) {
    let args = crate::preview::PreviewArgs {
        corpus_dir: Some(root.into()),
        preview_map: Some("maps/synth.map".into()),
        player_gcd: Some("premade/synthetic.gcd".into()),
        headless: true,
        ..Default::default()
    };
    let session =
        crate::session::SessionRuntime::start(crate::session::SessionConfig::replay(args))
            .unwrap()
            .unwrap();
    let mut replay = crate::replay::Replay::new(session).unwrap();
    replay.run(log.as_bytes()).unwrap();
    let hash = replay.world().unwrap().host.world.state_hash();
    (hash, replay.finish(None).unwrap().state_sha256)
}

/// Brief §5.1-§5.3: a replay of one input log (a walk ordered by a click,
/// and the clock running) ends in the same `state_hash` with and without
/// the backdrop; with it the digest, which covers the frame, differs from
/// the plain map's and is the same on every run.
#[test]
fn a_replay_keeps_its_world_and_a_stable_frame_digest_with_a_backdrop() {
    const LOG: &str = r#"{"command_id":1,"kind":"focus","focused":true}
{"command_id":2,"kind":"tick","now_ms":100}
{"command_id":3,"kind":"pointer_moved","x":420.5,"y":300.5}
{"command_id":4,"kind":"tick","now_ms":132}
{"command_id":5,"kind":"pointer_button","button":"Left","pressed":true}
{"command_id":6,"kind":"pointer_button","button":"Left","pressed":false}
{"command_id":7,"kind":"tick","now_ms":400}
{"command_id":8,"kind":"tick","now_ms":900}
{"command_id":9,"kind":"pointer_moved","x":300.5,"y":200.5}
{"command_id":10,"kind":"tick","now_ms":1500}
"#;
    let plain = corpus();
    let dir = corpus();
    floor_backdrop(dir.path());
    let without = replayed(plain.path(), LOG);
    let with = replayed(dir.path(), LOG);
    assert_eq!(with.0, without.0, "the backdrop changed the world");
    assert_ne!(with.1, without.1, "the digest covers the backdrop's frame");
    assert_eq!(replayed(dir.path(), LOG), with, "a stable digest");
}

fn chunk(bytes: usize) -> Arc<BackdropChunk> {
    Arc::new(BackdropChunk {
        origin: [0, 0],
        size: [bytes as u16, 1],
        colour: vec![1; bytes],
        depth: None,
        fade: None,
    })
}

/// Brief §3.10.2: eviction is least-recently-drawn, ties by ascending
/// (elevation, row, column), and the current refresh's chunks are never
/// evicted, even past the budget.
#[test]
fn the_chunk_cache_evicts_least_recently_drawn_then_by_key() {
    let mut cache = ChunkCache::default();
    let budget = 300;
    // Refresh 1 draws three chunks: all fit.
    cache.stamp = 1;
    for key in [(0, 1, 0), (0, 0, 1), (0, 0, 0)] {
        cache.insert(key, chunk(100), budget);
    }
    assert_eq!(cache.entries.len(), 3);
    // Refresh 2 draws (0, 0, 1) again and one new chunk: of the two drawn
    // last in refresh 1, the lower key (0, 0, 0) goes first.
    cache.stamp = 2;
    assert!(cache.touch((0, 0, 1)).is_some());
    cache.insert((1, 0, 0), chunk(100), budget);
    let keys: Vec<_> = cache.entries.keys().copied().collect();
    assert_eq!(keys, [(0, 0, 1), (0, 1, 0), (1, 0, 0)]);
    // Refresh 3 needs five chunks: the older ones go, its own all stay.
    cache.stamp = 3;
    for column in 0..5 {
        cache.insert((2, 0, column), chunk(100), budget);
    }
    let keys: Vec<_> = cache.entries.keys().copied().collect();
    assert_eq!(
        keys,
        (0..5).map(|column| (2, 0, column)).collect::<Vec<_>>()
    );
    assert_eq!(cache.bytes, 500);
    // The next refresh trims back to the budget, lowest keys first.
    cache.stamp = 4;
    cache.evict(budget);
    let keys: Vec<_> = cache.entries.keys().copied().collect();
    assert_eq!(keys, [(2, 0, 2), (2, 0, 3), (2, 0, 4)]);
}

/// A budget smaller than one chunk still draws every chunk the refresh
/// needs, so the canvas is the same as under the default budget; between
/// refreshes the cache keeps only what fits.
#[test]
fn a_tiny_cache_budget_draws_the_same_canvas() {
    let dir = corpus();
    floor_backdrop(dir.path());
    let mut roomy = open(dir.path());
    roomy.tick(16).unwrap();
    let expected = roomy.map_view_image().unwrap();
    assert_eq!(roomy.backdrops.borrow().cached().len(), 20);
    let mut tiny = open(dir.path());
    tiny.backdrops.get_mut().set_budget(1);
    tiny.tick(16).unwrap();
    for _ in 0..3 {
        tiny.thaw_view();
        let image = tiny.map_view_image().unwrap();
        assert!(image.pixels() == expected.pixels());
    }
    tiny.backdrops.get_mut().set_budget(1);
    assert!(tiny.backdrops.borrow().cached().is_empty());
}
