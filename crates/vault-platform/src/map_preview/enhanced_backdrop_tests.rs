//! The unlocked view's backdrops (the dimmed world behind a window, the
//! bezel's place around a full-screen one) and its auto-fit zoom floor,
//! over the synthetic corpus.
use super::super::{
    barter::BarterScreen,
    test_support::{interactive, npc, synthetic_corpus},
};
use super::tests::{at, frame, loot_corpse, unlocked};
use super::*;

const WIDE: [u32; 2] = [1920, 1080];

/// A palette index's brightness: the sum of its 6-bit channels.
fn brightness(preview: &MapPreview, index: u8) -> u64 {
    preview
        .game_palette()
        .rgb6(index)
        .iter()
        .map(|channel| u64::from(*channel))
        .sum()
}

/// The total brightness of `image`'s pixels in `rect` (`[x, y, w, h]`).
fn total(preview: &MapPreview, image: &IndexedImage, [x, y, w, h]: [u32; 4]) -> u64 {
    let width = image.width() as usize;
    (y..y + h)
        .flat_map(|row| (x..x + w).map(move |column| (row as usize) * width + column as usize))
        .map(|index| brightness(preview, image.pixels()[index]))
        .sum()
}

/// The window's frame shows the world dimmed behind it: exactly the map
/// screen's canvas `plain` (the same camera and zoom; greyed under the
/// pause window) through the vignette, never brighter and darker overall;
/// the side panels only where the bar shows; no bezel.
fn assert_dimmed(
    preview: &mut MapPreview,
    plain: &IndexedImage,
    label: &str,
    bar: bool,
    grey: bool,
) {
    let (layout, shown) = frame(preview, WIDE);
    assert!(!shown.bezel, "{label}");
    assert_eq!(shown.side_panel.is_some(), bar, "{label}");
    let world = &shown.world;
    let height = if bar {
        layout.world_size[1]
    } else {
        layout.full_canvas_height()
    };
    assert_eq!(
        [world.width(), world.height()],
        [layout.world_size[0], height],
        "{label}"
    );
    // The map screen's canvas, greyed and dimmed by the vignette about the
    // window's middle, is what the window's backdrop shows above the bar.
    let mut expected = plain.clone();
    if grey {
        for pixel in expected.pixels_mut() {
            *pixel = preview.grey[usize::from(*pixel)];
        }
    }
    let middle = ui_to_world(&layout, if bar { [320, 190] } else { [320, 240] })
        .map(|value| value.round() as i64);
    let size = [i64::from(world.width()), i64::from(world.height())];
    let vignette = Vignette {
        center: middle,
        radii: [0, 1].map(|axis| middle[axis].max(size[axis] - middle[axis])),
        near: DIM_NEAR,
        far: DIM_FAR,
    };
    let grid = preview.enhanced_view().unwrap().grid;
    let ramp = preview.dim_ramp();
    draw_vignette(&mut expected, grid, vignette, &ramp);
    let rows = plain.pixels().len();
    assert!(world.pixels()[..rows] == *expected.pixels(), "{label}");
    let whole = [0, 0, layout.world_size[0], layout.world_size[1]];
    let (dimmed, lit) = (total(preview, world, whole), total(preview, plain, whole));
    assert!(dimmed < lit, "{label}: dimmed {dimmed} {lit}");
    // The frame sees through to it somewhere in the map rows.
    if let Some(clear) = shown.ui_transparent {
        assert!(shown.ui.pixels()[..380 * 640].contains(&clear), "{label}");
    }
}

#[test]
fn the_dimming_never_lightens_and_darkens_toward_the_window() {
    let dir = synthetic_corpus();
    let mut preview = unlocked(dir.path());
    let ramp = preview.dim_ramp();
    assert_eq!(ramp.len(), DIM_LEVELS.len());
    for table in ramp.iter() {
        for (index, dimmed) in table.iter().enumerate() {
            assert!(brightness(&preview, *dimmed) <= brightness(&preview, index as u8));
        }
    }
    // The window's middle takes a darker table than the canvas's edge.
    let vignette = Vignette {
        center: [400, 200],
        radii: [400, 200],
        near: DIM_NEAR,
        far: DIM_FAR,
    };
    let grid = HexGrid::default();
    let near = vignette.level(grid, [400, 200]);
    let edge = vignette.level(grid, [0, 200]);
    assert!(near > edge, "{near} {edge}");
    assert_eq!(near, (DIM_NEAR / 16) as usize);
}

/// Back to the map screen, and its canvas.
fn map_canvas(preview: &mut MapPreview) -> IndexedImage {
    preview.screen = Screen::Map;
    preview.thaw_view();
    let (_, shown) = frame(preview, WIDE);
    shown.world
}

#[test]
fn each_window_over_the_map_shows_the_world_dimmed_behind_it() {
    let dir = synthetic_corpus();
    let mut preview = unlocked(dir.path());
    let plain = map_canvas(&mut preview);
    // Inventory, Skilldex and the options menu, by their keys.
    for (key, label) in [
        (KeyCode::KeyI, "inventory"),
        (KeyCode::KeyS, "Skilldex"),
        (KeyCode::Escape, "options"),
    ] {
        assert!(preview.step(key), "{label}");
        assert!(!matches!(preview.screen, Screen::Map), "{label}");
        assert_dimmed(&mut preview, &plain, label, true, false);
        assert_eq!(map_canvas(&mut preview).pixels(), plain.pixels());
    }
    // The pause window greys the world first, as Vanilla greys its map.
    preview.open_options();
    let (_, menu) = frame(&mut preview, WIDE);
    map_canvas(&mut preview);
    preview.open_pause();
    assert_dimmed(&mut preview, &plain, "pause", true, true);
    let (_, paused) = frame(&mut preview, WIDE);
    let tint = |image: &IndexedImage| {
        image
            .pixels()
            .iter()
            .map(|index| {
                let [r, g, b] = preview.game_palette().rgb6(*index).map(i64::from);
                (r - g).abs() + (g - b).abs()
            })
            .sum::<i64>()
    };
    assert!(tint(&paused.world) < tint(&menu.world), "the pause greys");
    map_canvas(&mut preview);
    // The elevator.
    preview.open_elevator(0);
    assert!(matches!(preview.screen, Screen::Elevator(_)));
    assert_dimmed(&mut preview, &plain, "elevator", true, false);
    map_canvas(&mut preview);
    // Loot, on a corpse.
    loot_corpse(&mut preview);
    assert_dimmed(&mut preview, &plain, "loot", true, false);
    let plain = map_canvas(&mut preview);
    // Barter: the conversation's window covers the bar, so the canvas
    // runs down to the surface's bottom and no side panel shows.
    let merchant = npc(&preview);
    let session = preview.host.begin_barter(merchant).unwrap();
    preview.screen = Screen::Barter(Box::new(BarterScreen::new(session, false)));
    assert_dimmed(&mut preview, &plain, "barter", false, false);
}

#[test]
fn a_paused_window_keeps_its_backdrop_until_the_world_changes() {
    let dir = synthetic_corpus();
    let mut preview = unlocked(dir.path());
    assert!(preview.step(KeyCode::KeyI));
    let (_, first) = frame(&mut preview, WIDE);
    assert!(preview.unlocked().unwrap().frozen.is_some());
    // A pointer move over the window does not redraw the world.
    let (layout, _) = frame(&mut preview, WIDE);
    preview.enhanced_pointer(at(&layout, [960.0, 300.0]));
    let (_, again) = frame(&mut preview, WIDE);
    assert_eq!(again.world.pixels(), first.world.pixels());
    // A camera move does (the world is drawn as it is now), and so does
    // a world change.
    preview.enhanced.as_mut().unwrap().camera.drag([64.0, 0.0]);
    let (_, moved) = frame(&mut preview, WIDE);
    assert_ne!(moved.world.pixels(), first.world.pixels());
    preview
        .host
        .world
        .effects
        .push_back(vault_core::WorldEffect::Camera(
            vault_core::camera::CameraCommand::Shake {
                intensity: 0,
                duration_ms: 0,
            },
        ));
    preview.drain_effects();
    assert!(preview.unlocked().unwrap().frozen.is_none());
}

#[test]
fn full_screen_windows_take_the_bezel_and_movies_black() {
    let dir = synthetic_corpus();
    let mut preview = unlocked(dir.path());
    for (key, label) in [(KeyCode::KeyP, "Pip-Boy"), (KeyCode::KeyC, "character")] {
        assert!(preview.step(key), "{label}");
        let (_, shown) = frame(&mut preview, WIDE);
        assert!(shown.bezel, "{label}");
        assert!(shown.side_panel.is_none(), "{label}");
        assert!(shown.ui_transparent.is_none(), "{label}");
        assert!(preview.step(KeyCode::Escape));
        assert!(matches!(preview.screen, Screen::Map), "{label}");
    }
    // The map screen and its windows never do.
    let (_, map) = frame(&mut preview, WIDE);
    assert!(!map.bezel && map.side_panel.is_some());
    assert_eq!(preview.backdrop(), Backdrop::Map);
}

#[test]
fn vanilla_and_fixed_window_frames_are_untouched() {
    let dir = synthetic_corpus();
    let mut plain = interactive(dir.path());
    let mut visited = interactive(dir.path());
    let layout = EnhancedLayout::compute(WIDE, None, 1.0).unwrap();
    for key in [KeyCode::KeyI, KeyCode::KeyS, KeyCode::Escape] {
        plain.step(key);
        visited.step(key);
        // The unlocked view's backdrop, then Vanilla again: nothing it
        // drew stays behind in the Vanilla frame.
        visited.set_enhanced(Some(EnhancedSettings::default()));
        visited.enhanced_frame(&layout).unwrap().unwrap();
        visited.set_enhanced(None);
        assert!(visited.enhanced_frame(&layout).is_none());
        assert_eq!(
            plain.image().unwrap().pixels(),
            visited.image().unwrap().pixels()
        );
        // The fixed view has no enhanced frame over a window.
        visited.set_enhanced(Some(EnhancedSettings {
            viewport: ViewportMode::Fixed,
            ..Default::default()
        }));
        assert!(visited.enhanced_frame(&layout).is_none());
        visited.set_enhanced(None);
        plain.step(KeyCode::Escape);
        visited.step(KeyCode::Escape);
        assert!(matches!(plain.screen, Screen::Map));
    }
    plain.open_pause();
    visited.open_pause();
    assert_eq!(
        plain.image().unwrap().pixels(),
        visited.image().unwrap().pixels()
    );
}

#[test]
fn the_grid_and_the_drawn_floor_bound_the_map_area() {
    assert_eq!(grid_area(), [0.0, -1188.0, 7984.0, 2404.0]);
    let rest = [3000.0, 300.0, 4000.0, 900.0];
    // No floor drawn: the grid.
    assert_eq!(drawn_area(None, rest), grid_area());
    // A floor that holds Vanilla's 640-by-380 view everywhere the camera
    // rests frames the map.
    let framing = [2600.0, 100.0, 4400.0, 1100.0];
    assert_eq!(drawn_area(Some(framing), rest), framing);
    // One that does not (a few squares, or a floor ending at the
    // blockers) leaves the grid as the frame.
    let short = [2700.0, 100.0, 4400.0, 1100.0];
    assert_eq!(drawn_area(Some(short), rest), grid_area());
    // A floor past the grid is cut to it.
    let wide = [-500.0, -2000.0, 9000.0, 3000.0];
    assert_eq!(drawn_area(Some(wide), rest), grid_area());
    // The synthetic map draws one square of floor.
    let dir = synthetic_corpus();
    let preview = interactive(dir.path());
    let floor = floor_extent(&preview.host.world.map, 0).unwrap();
    assert_eq!([floor[2] - floor[0], floor[3] - floor[1]], [80.0, 36.0]);
}

#[test]
fn the_fit_zoom_follows_the_window_and_caps_at_three() {
    let rest = [3000.0, 300.0, 4000.0, 900.0];
    let area = [2600.0, 50.0, 4400.0, 1150.0];
    let at_scale = |surface, scale| EnhancedLayout::compute(surface, Some(scale), 1.0).unwrap();
    // 400 and 250 pixels of room: views up to 798 by 498 world pixels.
    let small = fit_zoom(rest, area, &at_scale([960, 600], 1.0));
    assert!((small - (960.0 / 798.0_f64).max(500.0 / 498.0)).abs() < 1e-9);
    let large = fit_zoom(rest, area, &at_scale([1920, 1080], 1.0));
    assert!((large - (1920.0 / 798.0_f64).max(980.0 / 498.0)).abs() < 1e-9);
    assert!(large > small);
    // A larger interface scale shows less world: a lower floor.
    assert!(fit_zoom(rest, area, &at_scale([1920, 1080], 2.0)) < large);
    // Less room than half the 640-by-380 view: no zoom fits, and the
    // floor is the 3.0 cap.
    let tight = [2700.0, 50.0, 4400.0, 1150.0];
    assert_eq!(
        fit_zoom(rest, tight, &at_scale([1920, 1080], 1.0)),
        camera::MAX_ZOOM
    );
    // Room for any view: the 0.5 floor.
    let open = [-10_000.0, -10_000.0, 20_000.0, 20_000.0];
    assert_eq!(
        fit_zoom(rest, open, &at_scale([1920, 1080], 1.0)),
        camera::MIN_ZOOM
    );
}

#[test]
fn a_normal_map_keeps_the_half_zoom_floor() {
    let dir = synthetic_corpus();
    let mut preview = unlocked(dir.path());
    for surface in [[1920, 1080], [2560, 1080], [1280, 800], [3840, 2160]] {
        let (layout, _) = frame(&mut preview, surface);
        assert_eq!(
            preview.enhanced_floor(&layout),
            EnhancedLayout::MIN_ZOOM.max(layout.min_zoom()),
            "{surface:?}"
        );
    }
    assert_eq!(preview.unlocked().unwrap().camera.floor, camera::MIN_ZOOM);
}

#[test]
fn a_narrow_map_raises_the_floor_until_the_view_stays_on_it() {
    use vault_core::camera::CameraCommand;
    let dir = synthetic_corpus();
    let mut preview = unlocked(dir.path());
    let world = &preview.host.world;
    let player = world.get(world.player).unwrap().raw.hex.unwrap();
    // Scroll blockers around a small strip about the player, and floor
    // drawn a little past Vanilla's view of it (360 and 230 pixels).
    let [column, row] = [player % 200, player / 200];
    let strip: Vec<bool> = (0..40000)
        .map(|hex| (hex % 200 - column).abs() <= 2 && (hex / 200 - row).abs() <= 8)
        .collect();
    let enhanced = preview.unlocked_mut().unwrap();
    enhanced.region = Arc::new(CameraRegion::from_allowed(strip));
    let rest = enhanced.rest_box().unwrap();
    let floor_box = [
        rest[0] - 360.0,
        rest[1] - 230.0,
        rest[2] + 360.0,
        rest[3] + 230.0,
    ];
    enhanced.floor_area = Some(floor_box);
    let (layout, _) = frame(&mut preview, WIDE);
    let floor = f64::from(preview.enhanced_floor(&layout));
    let scale = f64::from(layout.ui_scale);
    let expected = (f64::from(layout.world_rect[2]) / (scale * 718.0))
        .max(f64::from(layout.world_rect[3]) / (scale * 458.0));
    assert!((floor - expected).abs() < 1e-5, "{floor} {expected}");
    assert!(floor > 1.0 && floor < camera::MAX_ZOOM);
    let camera = |preview: &MapPreview| preview.unlocked().unwrap().camera.clone();
    // The frame raised the camera to it.
    assert!((camera(&preview).zoom - floor).abs() < 1e-6);
    // The shell lays the frame out at it, and the view from the soft
    // bounds' corners stays on the drawn floor.
    let mut shell = crate::preview::AssetPreview::Map(Box::new(preview));
    let layout = crate::replay::enhanced_layout(&mut shell, WIDE).unwrap();
    let crate::preview::AssetPreview::Map(preview) = &mut shell else {
        unreachable!()
    };
    assert!(f64::from(layout.zoom) >= floor - 1e-6);
    preview.enhanced_frame(&layout).unwrap().unwrap();
    let bounds = preview.unlocked().unwrap().bounds().unwrap();
    let size = layout.world_size.map(f64::from);
    for axis in 0..2 {
        let half = size[axis] / 2.0 + 0.5;
        assert!(bounds[axis] - half >= floor_box[axis], "axis {axis}");
        assert!(
            bounds[axis + 2] + half <= floor_box[axis + 2],
            "axis {axis}"
        );
    }
    // The wheel (a pinch's lines too), `-`, a scripted SetZoom and the
    // default zoom all stop at the floor.
    let still = at(&layout, [960.0, 400.0]);
    preview.enhanced_pointer(still);
    assert!(preview.enhanced_wheel(still, -6.0));
    assert!((camera(preview).zoom - floor).abs() < 1e-6, "wheel");
    preview.enhanced_wheel(still, 2.0);
    assert!(camera(preview).zoom > floor + 0.1);
    for _ in 0..8 {
        preview.step(KeyCode::Minus);
    }
    assert!((camera(preview).zoom - floor).abs() < 1e-6, "minus");
    preview.enhanced_wheel(still, 2.0);
    preview.camera_command(CameraCommand::SetZoom {
        percent: 50,
        duration_ms: 0,
    });
    assert!((camera(preview).zoom - floor).abs() < 1e-6, "SetZoom");
    preview.set_enhanced(Some(EnhancedSettings {
        smooth_camera: false,
        default_zoom: 0.5,
        ..Default::default()
    }));
    preview.enhanced_wheel(still, 2.0);
    assert!(preview.step(KeyCode::Digit0));
    assert!((camera(preview).zoom - floor).abs() < 1e-6, "default zoom");
}
