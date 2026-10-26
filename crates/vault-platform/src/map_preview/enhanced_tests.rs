//! The `enhanced` presentation over the synthetic corpus: Vanilla identity,
//! frame sizes, pointer mapping, zoom, panning, roof peeking and the
//! scripted camera.
use super::super::test_support::{interactive, npc, synthetic_corpus};
use super::*;
use vault_core::{WorldEffect, ui::cursor::Mode};
use vault_render::map::{pick_candidates, pick_candidates_cut};

/// The synthetic world in the unlocked view, without smoothing.
fn unlocked(dir: &std::path::Path) -> MapPreview {
    let mut preview = interactive(dir);
    preview.set_enhanced(Some(EnhancedSettings {
        smooth_camera: false,
        ..Default::default()
    }));
    preview.tick(16).unwrap();
    preview
}
/// The layout for `surface` at the preview's zoom, and its frame.
fn frame(preview: &mut MapPreview, surface: [u32; 2]) -> (EnhancedLayout, EnhancedFrame) {
    let layout = EnhancedLayout::compute(surface, None, preview.enhanced_zoom()).unwrap();
    let frame = preview.enhanced_frame(&layout).unwrap().unwrap();
    (layout, frame)
}
/// The pointer at surface point `point`, as the shell reports it.
fn at(layout: &EnhancedLayout, point: [f64; 2]) -> PointerAt {
    PointerAt {
        ui: layout.ui_pixel(point),
        world: layout.world_point(point),
        side: layout.in_side_panel(point),
    }
}
/// The surface point at the middle of canvas pixel `pixel`.
fn surface(layout: &EnhancedLayout, pixel: [i64; 2]) -> [f64; 2] {
    let per_world = f64::from(layout.ui_scale * layout.zoom);
    [0, 1].map(|axis| f64::from(layout.world_rect[axis]) + (pixel[axis] as f64 + 0.5) * per_world)
}
fn set_zoom(preview: &mut MapPreview, zoom: f64) {
    preview.enhanced.as_mut().unwrap().camera.zoom = zoom;
}
fn camera(preview: &MapPreview) -> &Camera {
    &preview.enhanced.as_ref().unwrap().camera
}

#[test]
fn vanilla_frames_are_unchanged_without_the_enhanced_view() {
    let dir = synthetic_corpus();
    let mut plain = interactive(dir.path());
    let mut reset = interactive(dir.path());
    reset.set_enhanced(Some(EnhancedSettings::default()));
    reset.set_enhanced(None);
    assert!(reset.enhanced().is_none());
    let layout = EnhancedLayout::compute([1920, 1080], None, 1.0).unwrap();
    assert!(reset.enhanced_frame(&layout).is_none());
    assert!(!reset.enhanced_pointer(at(&layout, [960.0, 400.0])));
    let mut now = 16;
    type Step<'a> = &'a dyn Fn(&mut MapPreview, u64);
    let steps: [Step; 6] = [
        &|preview, _| {
            preview.hover([336, 170]);
        },
        &|preview, now| {
            preview.tick(now).unwrap();
        },
        &|preview, _| {
            preview.click([336, 170], true);
        },
        &|preview, now| {
            preview.hover([300, 200]);
            preview.tick(now).unwrap();
        },
        &|preview, _| {
            preview.step(KeyCode::ArrowRight);
            preview.step(KeyCode::Equal);
        },
        &|preview, _| {
            preview.tap([320, 185]);
        },
    ];
    for step in steps {
        now += 300;
        step(&mut plain, now);
        step(&mut reset, now);
        assert_eq!(
            plain.image().unwrap().pixels(),
            reset.image().unwrap().pixels()
        );
        assert_eq!(plain.host.world.camera_hex, reset.host.world.camera_hex);
    }
}

#[test]
fn the_fixed_viewport_keeps_the_vanilla_frame() {
    let dir = synthetic_corpus();
    let mut preview = interactive(dir.path());
    preview.set_enhanced(Some(EnhancedSettings {
        viewport: ViewportMode::Fixed,
        ..Default::default()
    }));
    let layout = EnhancedLayout::compute([1920, 1080], None, 1.0).unwrap();
    assert!(preview.enhanced_frame(&layout).is_none());
    // Idempotent, and it reports the current settings.
    let settings = preview.enhanced().unwrap();
    preview.set_enhanced(Some(settings));
    assert_eq!(preview.enhanced(), Some(settings));
}

#[test]
fn enhanced_frames_fit_their_layouts() {
    let dir = synthetic_corpus();
    let mut preview = unlocked(dir.path());
    for surface in [[1920, 1080], [2560, 1080], [1280, 800]] {
        for zoom in [0.5, 1.0, 2.0] {
            set_zoom(&mut preview, zoom);
            let (layout, frame) = frame(&mut preview, surface);
            assert_eq!(layout.zoom, zoom as f32);
            let size = [frame.world.width(), frame.world.height()];
            assert_eq!(size, layout.world_size, "{surface:?} at {zoom}");
            assert_eq!([frame.ui.width(), frame.ui.height()], [640, 480]);
            // The map rows see through to the world; the bar does not.
            let clear = frame.ui_transparent.expect("a see-through index");
            assert_eq!(frame.ui.pixels()[100 * 640 + 320], clear);
            assert!(
                frame.ui.pixels()[440 * 640..]
                    .iter()
                    .all(|pixel| *pixel != clear)
            );
            let panel = frame.side_panel.as_ref().unwrap();
            assert_eq!([panel.width(), panel.height()], [32, 100]);
        }
    }
    // A full-screen window is opaque over the world it froze.
    assert!(preview.step(KeyCode::KeyI));
    let (layout, frame) = frame(&mut preview, [1920, 1080]);
    assert!(frame.ui_transparent.is_none());
    assert_eq!(
        [frame.world.width(), frame.world.height()],
        layout.world_size
    );
}

#[test]
fn the_pointer_hovers_the_hex_drawn_under_it_at_every_zoom() {
    let dir = synthetic_corpus();
    let mut preview = unlocked(dir.path());
    let player = preview
        .host
        .world
        .get(preview.host.world.player)
        .unwrap()
        .raw
        .hex
        .unwrap();
    let hexes = [
        player,
        HexGrid::step(player, 0, 2).unwrap(),
        HexGrid::step(player, 2, 3).unwrap(),
        HexGrid::step(player, 4, 1).unwrap(),
        HexGrid::step(player, 5, 4).unwrap(),
    ];
    for zoom in [0.5, 1.0, 2.0] {
        set_zoom(&mut preview, zoom);
        let (layout, _) = frame(&mut preview, [1920, 1080]);
        let view = preview.enhanced_view().unwrap();
        for hex in hexes {
            let center = view.grid.hex_center(hex).unwrap();
            let point = surface(&layout, center);
            assert!(preview.enhanced_pointer(at(&layout, point)));
            assert!(preview.pointer.world);
            assert_eq!(preview.last_hover.unwrap().hex, Some(hex), "zoom {zoom}");
        }
        // The bar rows take interface pixels.
        let bar = [960.0, 1080.0 - 20.0];
        preview.enhanced_pointer(at(&layout, bar));
        assert!(!preview.pointer.world);
        assert!(preview.pointer.at.unwrap()[1] > 379);
    }
}

#[test]
fn the_wheel_zooms_about_the_world_point_under_the_pointer() {
    let dir = synthetic_corpus();
    let mut preview = unlocked(dir.path());
    let (layout, _) = frame(&mut preview, [1920, 1080]);
    let point = [700.0, 300.0];
    let grid_point = |preview: &MapPreview, layout: &EnhancedLayout| {
        let world = layout.world_point(point).unwrap();
        let origin = preview.enhanced_view().unwrap().grid.origin;
        [world[0] - origin[0] as f64, world[1] - origin[1] as f64]
    };
    let before = grid_point(&preview, &layout);
    preview.enhanced_pointer(at(&layout, point));
    assert!(preview.enhanced_wheel(at(&layout, point), 2.0));
    assert!((camera(&preview).zoom - 1.15_f64.powi(2)).abs() < 1e-9);
    assert!(
        !camera(&preview).follow,
        "an off-center anchor stops following"
    );
    preview.tick(32).unwrap();
    let (layout, _) = frame(&mut preview, [1920, 1080]);
    let after = grid_point(&preview, &layout);
    for axis in 0..2 {
        assert!(
            (after[axis] - before[axis]).abs() <= 1.5,
            "{before:?} -> {after:?}"
        );
    }
    // `-` zooms back out about the same point; `0` resets; Ctrl+= is
    // brightness.
    assert!(preview.step(KeyCode::Minus));
    assert!((camera(&preview).zoom - 1.15).abs() < 1e-9);
    assert!(preview.step(KeyCode::Digit0));
    assert_eq!(camera(&preview).zoom, 1.0);
    let brightness = preview.preferences.brightness;
    preview.set_modifiers([false, true, false]);
    preview.step(KeyCode::Equal);
    assert_eq!(camera(&preview).zoom, 1.0);
    assert_ne!(preview.preferences.brightness, brightness);
}

/// The smooth camera as the shell drives it: a tick and a frame every
/// 16 ms for `ms` milliseconds; returns the clock.
fn run(preview: &mut MapPreview, mut now: u64, ms: u64) -> u64 {
    for _ in 0..ms / 16 {
        now += 16;
        preview.tick(now).unwrap();
        frame(preview, [1920, 1080]);
    }
    now
}

/// Where the pointer rests in the zoom test (surface pixels).
const STILL: [f64; 2] = [960.0, 420.0];
/// Wheel `lines` at the resting pointer, mapped through the layout of the
/// moment as the shell maps it.
fn wheel(preview: &mut MapPreview, lines: f64) -> bool {
    let at = at(&frame(preview, [1920, 1080]).0, STILL);
    preview.enhanced_wheel(at, lines)
}

#[test]
fn the_zoom_leaves_both_limits_by_wheel_and_keys() {
    let dir = synthetic_corpus();
    let mut preview = interactive(dir.path());
    preview.set_enhanced(Some(EnhancedSettings::default()));
    let mut now = run(&mut preview, 16, 64);
    let pointer = at(&frame(&mut preview, [1920, 1080]).0, STILL);
    preview.enhanced_pointer(pointer);
    // Up to the 3.0 limit in several wheel steps (and past it).
    for _ in 0..12 {
        assert!(wheel(&mut preview, 1.0));
        now = run(&mut preview, now, 48);
    }
    now = run(&mut preview, now, 300);
    assert_eq!(camera(&preview).zoom, camera::MAX_ZOOM);
    // A negative wheel line leaves the limit, and more reach 1.0.
    wheel(&mut preview, -1.0);
    now = run(&mut preview, now, 300);
    let zoom = camera(&preview).zoom;
    assert!(
        (zoom - camera::MAX_ZOOM / camera::ZOOM_STEP).abs() < 1e-9,
        "{zoom}"
    );
    for _ in 0..7 {
        wheel(&mut preview, -1.0);
        now = run(&mut preview, now, 48);
    }
    now = run(&mut preview, now, 300);
    let zoom = camera(&preview).zoom;
    assert!(zoom < 1.0 && zoom > 0.9, "{zoom}");
    // `-` from 3.0, then `0` back to 1.0 from the limit.
    for _ in 0..12 {
        wheel(&mut preview, 1.0);
    }
    now = run(&mut preview, now, 300);
    assert_eq!(camera(&preview).zoom, camera::MAX_ZOOM);
    assert!(preview.step(KeyCode::Minus));
    assert!(
        preview.pointer.edge.is_none(),
        "a still pointer never edge-scrolls"
    );
    now = run(&mut preview, now, 300);
    let zoom = camera(&preview).zoom;
    assert!(
        (zoom - camera::MAX_ZOOM / camera::ZOOM_STEP).abs() < 1e-9,
        "{zoom}"
    );
    for _ in 0..2 {
        wheel(&mut preview, 1.0);
    }
    now = run(&mut preview, now, 300);
    assert_eq!(camera(&preview).zoom, camera::MAX_ZOOM);
    assert!(preview.step(KeyCode::Digit0));
    now = run(&mut preview, now, 300);
    assert_eq!(camera(&preview).zoom, 1.0);
    // Down to the 0.5 limit with `-` (a pixel-delta wheel's fractional
    // lines too), and back up from it.
    wheel(&mut preview, -0.4);
    for _ in 0..8 {
        preview.step(KeyCode::Minus);
        now = run(&mut preview, now, 48);
    }
    now = run(&mut preview, now, 300);
    assert_eq!(camera(&preview).zoom, camera::MIN_ZOOM);
    assert!(wheel(&mut preview, 1.0));
    now = run(&mut preview, now, 300);
    let zoom = camera(&preview).zoom;
    assert!(
        (zoom - camera::MIN_ZOOM * camera::ZOOM_STEP).abs() < 1e-9,
        "{zoom}"
    );
    preview.step(KeyCode::Minus);
    now = run(&mut preview, now, 300);
    assert_eq!(camera(&preview).zoom, camera::MIN_ZOOM);
    assert!(preview.step(KeyCode::Digit0));
    run(&mut preview, now, 300);
    assert_eq!(camera(&preview).zoom, 1.0);
}

#[test]
fn a_middle_drag_pans_the_view() {
    let dir = synthetic_corpus();
    let mut preview = unlocked(dir.path());
    let (layout, _) = frame(&mut preview, [1920, 1080]);
    let start = camera(&preview).center;
    let from = [900.0, 400.0];
    let to = [990.0, 445.0];
    assert!(preview.enhanced_button(at(&layout, from), PointerButton::Middle, true));
    preview.enhanced_pointer(at(&layout, to));
    let per_world = f64::from(layout.ui_scale * layout.zoom);
    let moved = camera(&preview).center;
    assert!((moved[0] - (start[0] - 90.0 / per_world)).abs() < 1e-6);
    assert!((moved[1] - (start[1] - 45.0 / per_world)).abs() < 1e-6);
    assert!(!camera(&preview).follow);
    assert!(preview.enhanced_button(at(&layout, to), PointerButton::Middle, false));
    preview.enhanced_pointer(at(&layout, from));
    assert_eq!(camera(&preview).center, moved);
    // Home follows the player again.
    assert!(preview.step(KeyCode::Home));
    assert!(camera(&preview).follow);
}

#[test]
fn alt_r_cycles_the_roof_peek_and_alt_peeks_while_held() {
    let dir = synthetic_corpus();
    let mut preview = unlocked(dir.path());
    let (layout, _) = frame(&mut preview, [1920, 1080]);
    preview.enhanced_pointer(at(&layout, [800.0, 300.0]));
    assert_eq!(preview.peek_cutaway(), None);
    preview.set_modifiers([false, false, true]);
    let cut = preview.peek_cutaway().expect("Alt peeks");
    assert_eq!(cut.center, preview.pointer.at.unwrap().map(i64::from));
    assert_eq!((cut.inner, cut.ring), (96, 48));
    for peek in [RoofPeek::CursorXray, RoofPeek::Hidden, RoofPeek::Vanilla] {
        assert!(preview.step(KeyCode::KeyR));
        assert_eq!(preview.enhanced().unwrap().roof_peek, peek);
    }
    preview.set_modifiers([false, false, false]);
    assert_eq!(preview.peek_cutaway(), None);
    // The X-ray keeps its on-screen size; Hidden draws no roofs.
    preview.set_modifiers([false, false, true]);
    preview.step(KeyCode::KeyR);
    preview.set_modifiers([false, false, false]);
    assert_eq!(preview.enhanced().unwrap().roof_peek, RoofPeek::CursorXray);
    set_zoom(&mut preview, 2.0);
    let (layout, _) = frame(&mut preview, [1920, 1080]);
    preview.enhanced_pointer(at(&layout, [800.0, 300.0]));
    let cut = preview.peek_cutaway().unwrap();
    assert_eq!((cut.inner, cut.ring), (48, 24));
    preview.set_modifiers([false, false, true]);
    preview.step(KeyCode::KeyR);
    preview.set_modifiers([false, false, false]);
    assert!(!preview.enhanced_view().unwrap().roofs);
    assert_eq!(preview.peek_cutaway(), None);
    let (_, frame) = frame(&mut preview, [1920, 1080]);
    assert_eq!(frame.world.width(), layout.world_size[0]);
}

#[test]
fn a_click_through_a_peeked_roof_reaches_the_interior() {
    let dir = synthetic_corpus();
    let mut preview = unlocked(dir.path());
    // Roofs over the middle of the map (roof index 2, not hidden).
    let tiles = preview.host.world.map.tiles[0].as_mut().unwrap();
    for row in 40..60 {
        for column in 40..60 {
            let square = &mut tiles[row * 100 + column];
            square.word = (square.word & 0xffff) | 0x0002_0000;
        }
    }
    preview.image().unwrap();
    let (layout, _) = frame(&mut preview, [1920, 1080]);
    // The NPC's pixel, from the Vanilla view onto the canvas.
    let vanilla = preview.view.grid.origin;
    let canvas = preview.enhanced_view().unwrap().grid.origin;
    let pixel = [336 - vanilla[0] + canvas[0], 170 - vanilla[1] + canvas[1]];
    let point = surface(&layout, pixel);
    preview.enhanced_pointer(at(&layout, point));
    assert!(preview.pointer.world);
    let npc = npc(&preview);
    // Under the roof nothing is picked.
    assert_eq!(preview.object_at(preview.pointer.at.unwrap()), None);
    preview.enhanced.as_mut().unwrap().settings.roof_peek = RoofPeek::CursorXray;
    let at_pixel = preview.pointer.at.unwrap();
    let (tile, local, cutaway) = preview.enhanced_pick(at_pixel).unwrap();
    let cutaway = cutaway.expect("the pick takes the cutaway");
    assert_eq!(cutaway.center, local.map(i64::from));
    // Once the renderer honours the cutaway the NPC is hit; until then the
    // cut pick is the plain one.
    let objects = preview.host.world.render_objects();
    let plain = pick_candidates(
        &preview.host.world.map,
        &objects,
        tile,
        &preview.art,
        local,
        None,
    );
    let cut = pick_candidates_cut(
        &preview.host.world.map,
        &objects,
        tile,
        &preview.art,
        local,
        None,
        Some(cutaway),
    );
    let picked = preview.object_at(at_pixel);
    assert!(picked == Some(npc) || cut == plain, "{picked:?}");
    // The Hidden mode drops the roofs: the click reaches the NPC.
    preview.enhanced.as_mut().unwrap().settings.roof_peek = RoofPeek::Hidden;
    assert_eq!(preview.object_at(at_pixel), Some(npc));
    preview.hud.mode = Mode::Arrow;
    let (layout, _) = frame(&mut preview, [1920, 1080]);
    let point = surface(&layout, at_pixel.map(i64::from));
    assert!(preview.enhanced_button(at(&layout, point), PointerButton::Left, true));
    assert!(preview.enhanced_button(at(&layout, point), PointerButton::Left, false));
    assert_eq!(preview.selected, Some(npc));
}

#[test]
fn camera_commands_animate_and_reset_returns_to_the_player() {
    let dir = synthetic_corpus();
    let mut preview = unlocked(dir.path());
    let player = preview.player_point().unwrap();
    assert_eq!(camera(&preview).center, player);
    let tile = HexGrid::step(20100, 2, 6).unwrap();
    let goal = camera::hex_point(tile).unwrap();
    preview
        .host
        .world
        .effects
        .push_back(WorldEffect::Camera(CameraCommand::PanTo {
            tile,
            duration_ms: 1000,
        }));
    preview.tick(1000).unwrap();
    preview.tick(1500).unwrap();
    let middle = camera(&preview).center;
    assert!(middle != player && middle != goal);
    preview.tick(2000).unwrap();
    assert_eq!(camera(&preview).center, goal);
    assert_eq!(preview.host.world.camera_hex, tile);
    preview
        .host
        .world
        .effects
        .push_back(WorldEffect::Camera(CameraCommand::SetZoom {
            percent: 200,
            duration_ms: 500,
        }));
    preview.tick(2100).unwrap();
    preview.tick(2700).unwrap();
    assert_eq!(camera(&preview).zoom, 2.0);
    preview
        .host
        .world
        .effects
        .push_back(WorldEffect::Camera(CameraCommand::Shake {
            intensity: 6,
            duration_ms: 800,
        }));
    preview.tick(2800).unwrap();
    preview.tick(3000).unwrap();
    assert_ne!(camera(&preview).shake_offset(3000), [0.0; 2]);
    preview
        .host
        .world
        .effects
        .push_back(WorldEffect::Camera(CameraCommand::Reset {
            duration_ms: 500,
        }));
    // The reset waits for the shake (to 3600), then runs 500 ms.
    preview.tick(3500).unwrap();
    assert_eq!(camera(&preview).zoom, 2.0);
    for now in (3600..4200).step_by(50) {
        preview.tick(now).unwrap();
    }
    let camera = camera(&preview);
    assert_eq!(camera.center, player);
    assert_eq!(camera.zoom, 1.0);
    assert!(camera.follow);
    assert_eq!(camera.shake_offset(4200), [0.0; 2]);
}

#[test]
fn camera_commands_given_together_run_one_after_another() {
    let dir = synthetic_corpus();
    let mut preview = unlocked(dir.path());
    let player = preview.player_point().unwrap();
    let tile = HexGrid::step(20100, 2, 6).unwrap();
    let goal = camera::hex_point(tile).unwrap();
    // One script call's worth: a pan, a zoom, a shake and a reset at once.
    preview.host.world.effects.extend([
        WorldEffect::Camera(CameraCommand::PanTo {
            tile,
            duration_ms: 1000,
        }),
        WorldEffect::Camera(CameraCommand::SetZoom {
            percent: 200,
            duration_ms: 500,
        }),
        WorldEffect::Camera(CameraCommand::Shake {
            intensity: 6,
            duration_ms: 400,
        }),
        WorldEffect::Camera(CameraCommand::Reset { duration_ms: 500 }),
    ]);
    preview.tick(1000).unwrap();
    // The pan runs first, alone: the zoom has not started.
    preview.tick(1500).unwrap();
    assert_eq!(camera(&preview).zoom, 1.0);
    assert!(camera(&preview).center != player);
    preview.tick(2000).unwrap();
    assert_eq!(camera(&preview).center, goal);
    // Then the zoom, at the goal.
    preview.tick(2500).unwrap();
    assert_eq!(camera(&preview).zoom, 2.0);
    assert_eq!(camera(&preview).center, goal);
    // Then the shake.
    preview.tick(2700).unwrap();
    assert_ne!(camera(&preview).shake_offset(2700), [0.0; 2]);
    // Then the reset, back to the player.
    for now in (2900..3600).step_by(50) {
        preview.tick(now).unwrap();
    }
    let camera = camera(&preview);
    assert_eq!(camera.center, player);
    assert_eq!(camera.zoom, 1.0);
    assert!(camera.follow);
}

#[test]
fn the_soft_clamp_keeps_the_center_inside_and_the_free_camera_bypasses_it() {
    let dir = synthetic_corpus();
    let mut preview = unlocked(dir.path());
    let (layout, _) = frame(&mut preview, [1920, 1080]);
    let bounds = preview.unlocked().unwrap().bounds().unwrap();
    // Edge-scroll west for ten seconds: the center reaches the bound and
    // stops there.
    let edge = [1.0, 400.0];
    preview.enhanced_pointer(at(&layout, edge));
    let mut now = 16;
    for _ in 0..600 {
        now += 16;
        preview.tick(now).unwrap();
        assert!(camera(&preview).center[0] >= bounds[0] - 1e-6);
    }
    assert!(preview.pointer.edge.is_some_and(|edge| edge.blocked));
    assert!((camera(&preview).center[0] - bounds[0]).abs() < 1e-6);
    // A drag past the bound eases back inside.
    preview.enhanced_pointer(at(&layout, [960.0, 400.0]));
    preview
        .enhanced
        .as_mut()
        .unwrap()
        .camera
        .drag([-2000.0, 0.0]);
    for _ in 0..200 {
        now += 16;
        preview.tick(now).unwrap();
    }
    assert!((camera(&preview).center[0] - bounds[0]).abs() < 1e-6);
    // The free camera (the debug overlay's camera page) has no clamp, and
    // cuts the roofs away.
    preview.set_debug(true);
    for _ in 0..6 {
        assert!(preview.step(KeyCode::F8));
    }
    assert_eq!(
        preview.debug.as_ref().unwrap().page,
        super::super::debug::Page::Camera
    );
    assert!(preview.step(KeyCode::KeyF));
    assert!(preview.unlocked().unwrap().free_camera);
    assert!(!preview.enhanced_view().unwrap().roofs);
    preview
        .enhanced
        .as_mut()
        .unwrap()
        .camera
        .drag([-2000.0, 0.0]);
    let outside = camera(&preview).center;
    for _ in 0..200 {
        now += 16;
        preview.tick(now).unwrap();
    }
    assert_eq!(camera(&preview).center, outside);
    // The camera demo runs from the same page.
    assert!(preview.step(KeyCode::KeyC));
    assert!(!preview.unlocked().unwrap().demo.is_empty());
}

#[test]
fn scroll_blockers_bound_the_camera_but_never_shroud_the_grid() {
    let dir = synthetic_corpus();
    let mut preview = unlocked(dir.path());
    let world = &preview.host.world;
    let player = world.get(world.player).unwrap().raw.hex.unwrap();
    // A narrow strip, as artemple.map's blockers make: the player's column
    // and two either side, every row.
    let column = player % 200;
    let strip: Vec<bool> = (0..40000)
        .map(|hex| (hex % 200 - column).abs() <= 2)
        .collect();
    let strip = Arc::new(CameraRegion::from_allowed(strip));
    let wide = [1920, 1080];
    let open = frame(&mut preview, wide).1.world;
    preview.unlocked_mut().unwrap().region = strip.clone();
    // The same view draws the same: floor, walls and objects past the
    // blockers are not shrouded.
    let narrowed = frame(&mut preview, wide).1.world;
    let differing = (narrowed.pixels().iter())
        .zip(open.pixels())
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(differing, 0, "pixels shrouded past the blockers");
    // A view at least 640 by 380 keeps the strip's bounds (hex centers).
    let [x0, y0, x1, y1] = strip.bounds.map(|value| value as f64);
    let held = [x0 + 16.0, y0 + 8.0, x1 + 16.0, y1 + 8.0];
    assert_eq!(preview.unlocked().unwrap().bounds(), Some(held));
    set_zoom(&mut preview, 0.5);
    frame(&mut preview, wide);
    assert_eq!(preview.unlocked().unwrap().bounds(), Some(held));
    // Zoomed in, a smaller view may go as far as a 640-by-380 view's
    // edges reach, so the player at the strip's edge stays in sight.
    set_zoom(&mut preview, 3.0);
    frame(&mut preview, [960, 720]);
    let size = preview.unlocked().unwrap().world_size().map(f64::from);
    let slack = [(640.0 - size[0]) / 2.0, (380.0 - size[1]) / 2.0];
    assert!(slack.iter().all(|value| *value > 0.0), "{size:?}");
    assert_eq!(
        preview.unlocked().unwrap().bounds(),
        Some([
            held[0] - slack[0],
            held[1] - slack[1],
            held[2] + slack[0],
            held[3] + slack[1]
        ])
    );
}

#[test]
fn the_camera_page_exports_the_setup_as_a_mod() {
    use super::super::debug::{EXPORT_BUTTON, Page};
    use vault_formats::{
        ModManifest,
        mod_manifest::{MANIFEST, README},
    };
    let dir = synthetic_corpus();
    vault_fixtures::state::write_cam_demo(dir.path());
    let tables = dir.path().join("data/vault-engine");
    std::fs::create_dir_all(&tables).unwrap();
    let towns = vault_core::tables::Table::Towns.canonical();
    std::fs::write(tables.join("towns.txt"), towns).unwrap();
    let mut preview = unlocked(dir.path());
    preview.enhanced.as_mut().unwrap().settings.roof_peek = RoofPeek::CursorXray;
    set_zoom(&mut preview, 1.5);
    preview.set_debug(true);
    while preview.debug.as_ref().unwrap().page != Page::Camera {
        assert!(preview.step(KeyCode::F8));
    }
    let note = |preview: &MapPreview| preview.debug.as_ref().unwrap().note.clone();
    // Without an export root (a replay, a preview tool) it is refused.
    assert!(preview.step(KeyCode::KeyE));
    assert!(note(&preview).starts_with("refused"), "{}", note(&preview));
    let out = tempfile::tempdir().unwrap();
    preview.mod_export = Some(out.path().join("mods"));
    // A press on the button exports, as E does.
    let (layout, _) = frame(&mut preview, [1920, 1080]);
    let ui = [EXPORT_BUTTON[0] + 20, EXPORT_BUTTON[1] + 7];
    let scale = f64::from(layout.ui_scale);
    let point =
        [0, 1].map(|axis| f64::from(layout.ui_rect[axis]) + (f64::from(ui[axis]) + 0.5) * scale);
    let pointer = at(&layout, point);
    assert_eq!(pointer.ui, Some(ui));
    assert!(preview.enhanced_button(pointer, PointerButton::Left, true));
    let folder = out.path().join("mods/enhanced-setup");
    assert_eq!(note(&preview), format!("exported to {}", folder.display()));
    let manifest =
        ModManifest::parse(&std::fs::read_to_string(folder.join(MANIFEST)).unwrap()).unwrap();
    let enhanced = manifest.enhanced.unwrap();
    assert_eq!(enhanced.preset.as_deref(), Some("enhanced"));
    assert_eq!(
        enhanced.viewport_mode.as_deref(),
        Some("unlocked_widescreen")
    );
    assert_eq!(enhanced.default_zoom, Some(1.5));
    assert_eq!(enhanced.smooth_camera, Some(false));
    assert_eq!(enhanced.roof_peek.as_deref(), Some("cursor_xray"));
    assert_eq!(
        std::fs::read_to_string(folder.join("data/vault-engine/towns.txt")).unwrap(),
        towns
    );
    assert_eq!(
        std::fs::read(folder.join("scripts/camdemo.int")).unwrap(),
        vault_fixtures::state::cam_demo_program()
    );
    assert!(folder.join(README).is_file());
    // A press elsewhere on the panel does nothing; E exports again, to a
    // fresh folder.
    let below = [0, 1].map(|axis| {
        f64::from(layout.ui_rect[axis]) + (f64::from([ui[0], 200][axis]) + 0.5) * scale
    });
    assert!(preview.enhanced_button(at(&layout, below), PointerButton::Left, true));
    assert!(!out.path().join("mods/enhanced-setup-2").exists());
    assert!(preview.step(KeyCode::KeyE));
    assert!(
        out.path()
            .join("mods/enhanced-setup-2")
            .join(MANIFEST)
            .is_file()
    );
}

#[test]
fn a_registered_mod_layer_runs_the_camera_demo() {
    use std::sync::Arc;
    use vault_core::assets::WorldAssets;
    use vault_formats::{LayeredVfs, LocalDirProvider, Vfs};
    let dir = synthetic_corpus();
    let list = std::fs::read(dir.path().join("scripts/scripts.lst")).unwrap();
    let layer = vault_fixtures::state::write_cam_demo_mod(dir.path());
    assert_eq!(
        std::fs::read(dir.path().join("scripts/scripts.lst")).unwrap(),
        list,
        "scripts.lst is untouched"
    );
    let mount = || -> Arc<dyn Vfs> {
        let base: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(dir.path()).unwrap());
        Arc::new(LayeredVfs::from_load_order(base, &layer.load_order).unwrap())
    };
    let load = |vfs: Arc<dyn Vfs>| {
        MapPreview::load_with(
            vfs,
            "maps/cam_demo.map",
            "english",
            Some("premade/synthetic.gcd"),
            1,
            vault_core::EnginePreset::Enhanced,
        )
    };
    let mut preview = load(mount()).unwrap();
    // The registered map answers its name and number.
    let assets = &preview.host.assets;
    assert_eq!(assets.map_index(b"CAM_DEMO.MAP"), Some(layer.map as i32));
    assert_eq!(
        assets.map_path(layer.map as i32).as_deref(),
        Some("maps/cam_demo.map")
    );
    assert_eq!(preview.host.world.map.header.map_number, layer.map as i32);
    // The map script, a registered row, runs: the camera pans to the
    // interior (the first of its four requests).
    preview.set_enhanced(Some(EnhancedSettings {
        smooth_camera: false,
        ..Default::default()
    }));
    let mut now = 16;
    for _ in 0..120 {
        now += 16;
        preview.tick(now).unwrap();
    }
    assert_eq!(
        preview.host.world.camera_hex,
        vault_fixtures::state::CAM_DEMO_INTERIOR
    );
    // A registration on a scripts.lst row is refused at load.
    let manifest = layer.layer.join("vault-mod.toml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(&manifest, text.replace("index = 4096", "index = 0")).unwrap();
    let error = load(mount()).err().unwrap().to_string();
    assert!(error.contains("scripts.lst row"), "{error}");
}

/// The synthetic world's first item, given to a dead `npc`, and a
/// non-steal loot window on that corpse.
fn loot_corpse(preview: &mut MapPreview) -> vault_core::ObjectRef {
    use vault_core::LootRequest;
    use vault_formats::map::ObjectState;
    let player = preview.host.world.player;
    let npc = npc(preview);
    if let Some(ObjectState::Critter(state)) =
        preview.host.world.get_mut(npc).map(|o| &mut o.raw.state)
    {
        state.results.0 |= vault_core::critical::effect::DEAD;
    }
    let item = preview
        .host
        .world
        .iter()
        .find(|(_, object)| object.category() == vault_formats::pro::Category::Item)
        .unwrap()
        .0;
    preview.host.world.add_item(npc, item, 1, true).unwrap();
    preview.host.world.pending_loot = Some(LootRequest {
        looter: player,
        target: npc,
        steal: false,
    });
    preview.image().unwrap();
    assert!(matches!(preview.screen, Screen::Loot(_)));
    item
}

#[test]
fn the_enhanced_loot_window_has_a_take_all_button() {
    use super::super::loot::TAKE_ALL;
    let [ox, oy] = vault_core::skills::LOOT_ORIGIN;
    let button = [ox + TAKE_ALL.x + 10, oy + TAKE_ALL.y + 10];
    let dir = synthetic_corpus();
    // Vanilla: no button (Shift+A only); the press takes nothing.
    let mut vanilla = interactive(dir.path());
    let item = loot_corpse(&mut vanilla);
    let plain = vanilla.image().unwrap();
    vanilla.click(button, false);
    vanilla.release(Some(button), false);
    assert_ne!(
        vanilla.host.world.get(item).unwrap().owner,
        Some(vanilla.host.world.player)
    );
    // Enhanced: the button is drawn and takes everything, as Shift+A does.
    let mut preview = unlocked(dir.path());
    let item = loot_corpse(&mut preview);
    let drawn = preview.image().unwrap();
    assert_ne!(drawn.pixels(), plain.pixels(), "the button is drawn");
    assert!(preview.click(button, false));
    preview.release(Some(button), false);
    assert_eq!(
        preview.host.world.get(item).unwrap().owner,
        Some(preview.host.world.player)
    );
}

#[test]
fn shift_highlights_the_ground_items_in_the_enhanced_world_only() {
    let dir = synthetic_corpus();
    let ground = |preview: &mut MapPreview| {
        let player = preview.host.world.player;
        let hex = preview.host.world.get(player).unwrap().raw.hex.unwrap();
        let item = preview
            .host
            .world
            .iter()
            .find(|(_, object)| {
                object.category() == vault_formats::pro::Category::Item && object.owner.is_none()
            })
            .map(|(item, _)| item)
            .expect("an item on the map");
        let next = HexGrid::step(hex, 1, 2).unwrap();
        preview
            .host
            .world
            .move_object(item, next, 0, false)
            .unwrap();
    };
    // Vanilla: Shift changes nothing.
    let mut vanilla = interactive(dir.path());
    ground(&mut vanilla);
    let before = vanilla.image().unwrap();
    vanilla.set_modifiers([true, false, false]);
    assert_eq!(vanilla.image().unwrap().pixels(), before.pixels());
    // Enhanced: the item is outlined while Shift is held.
    let mut preview = unlocked(dir.path());
    ground(&mut preview);
    let (_, plain) = frame(&mut preview, [1920, 1080]);
    preview.set_modifiers([true, false, false]);
    let (_, lit) = frame(&mut preview, [1920, 1080]);
    assert_ne!(lit.world.pixels(), plain.world.pixels());
    preview.set_modifiers([false, false, false]);
    let (_, again) = frame(&mut preview, [1920, 1080]);
    assert_eq!(again.world.pixels(), plain.world.pixels());
}
