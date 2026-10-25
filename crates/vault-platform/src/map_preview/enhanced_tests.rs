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
