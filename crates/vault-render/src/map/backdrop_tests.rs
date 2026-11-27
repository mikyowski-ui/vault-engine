//! Pre-rendered backdrops: the owner brief's synthetic vectors A-D, the
//! amended sort-key domains, parity of the reference and instanced paths
//! (records, CPU and GPU canvases, the renderer) over the full fixture
//! scene with a backdrop, picking, suppression and the safe rules for
//! malformed tiles. Synthetic data only.
use super::instanced_tests::{
    CRITTER, Fixture, ITEM, Options, SCENERY, WALL, assert_canvas, gpu_executor, parity, pattern,
    reference_cutaway, scene_of, tile_of, view_at,
};
use super::tests::{Keyed, object, preview, scene};
use super::*;
use crate::layers::{
    BatchPlan, DrawMode, EXEMPT, InstanceError, LookupTables, SELECT, gpu, sort_by_key,
};
use std::collections::BTreeMap;

/// One owned tile's planes.
struct OwnedTile {
    origin: [u32; 2],
    size: [u16; 2],
    colour: Vec<u8>,
    depth: Option<Vec<u16>>,
    fade: Option<Vec<u8>>,
}

/// A whole image's planes, cut into tiles.
struct Planes {
    size: [u32; 2],
    tiles: Vec<OwnedTile>,
}

impl Planes {
    /// An image of `size`, in `chunk`-square tiles (row by row), whose
    /// pixel (x, y) is `paint(x, y)` = (colour, depth, fade bit); the depth
    /// and fade planes are present when asked for.
    fn paint(
        size: [u32; 2],
        chunk: u32,
        planes: (bool, bool),
        paint: impl Fn(u32, u32) -> (u8, u16, bool),
    ) -> Self {
        let mut tiles = Vec::new();
        for top in (0..size[1]).step_by(chunk as usize) {
            for left in (0..size[0]).step_by(chunk as usize) {
                let width = chunk.min(size[0] - left);
                let height = chunk.min(size[1] - top);
                let stride = width.div_ceil(8) as usize;
                let mut colour = Vec::new();
                let mut depth = Vec::new();
                let mut fade = vec![0_u8; stride * height as usize];
                for y in 0..height {
                    for x in 0..width {
                        let (c, d, f) = paint(left + x, top + y);
                        colour.push(c);
                        depth.push(d);
                        if f {
                            fade[y as usize * stride + x as usize / 8] |= 0x80 >> (x % 8);
                        }
                    }
                }
                tiles.push(OwnedTile {
                    origin: [left, top],
                    size: [width as u16, height as u16],
                    colour,
                    depth: planes.0.then_some(depth),
                    fade: planes.1.then_some(fade),
                });
            }
        }
        Self { size, tiles }
    }
    fn tiles(&self) -> Vec<BackdropTile<'_>> {
        self.tiles
            .iter()
            .map(|tile| BackdropTile {
                origin: tile.origin,
                size: tile.size,
                colour: &tile.colour,
                depth: tile.depth.as_deref(),
                fade: tile.fade.as_deref(),
            })
            .collect()
    }
}

/// The origin pixel that places image pixel (0, 0) at canvas `at` of
/// `view`, with origin hex 20100.
fn origin_px(view: MapView, at: [i64; 2]) -> [i32; 2] {
    let reference = view.grid.hex_reference(20100).unwrap();
    [0, 1].map(|axis| (reference[axis] - at[axis]) as i32)
}

/// A backdrop view of `tiles` placed by [`origin_px`].
fn backdrop<'a>(
    tiles: &'a [BackdropTile<'a>],
    size: [u32; 2],
    origin: [i32; 2],
    pids: &'a [u32],
    walls: bool,
) -> BackdropView<'a> {
    BackdropView {
        elevation: 0,
        origin_hex: 20100,
        origin_px: origin,
        size,
        chunks: tiles,
        suppress_walls: walls,
        suppress_pids: pids,
    }
}

// ---------------------------------------------------------------------
// The brief's vectors A-D.

/// A palette whose supplied intensity table is the identity at level 128
/// (so ambient light 65536 leaves every index unchanged) and scrambled at
/// every other level, with a scrambled supplied mix table.
fn vector_palette() -> Palette {
    let mut bytes = Vec::with_capacity(0x8300 + 4 + 3 * 65536);
    for index in 0..256_u32 {
        bytes.extend(
            [
                (index * 7) % 32,
                (index * 13 + 5) % 32,
                (index * 29 + 11) % 32,
            ]
            .map(|channel| channel as u8),
        );
    }
    for packed in 0..32768_u32 {
        let (r, g, b) = (packed >> 10, (packed >> 5) & 31, packed & 31);
        bytes.push(((r * 3 + g * 5 + b * 7 + 1) % 255 + 1) as u8);
    }
    bytes.extend(b"CWEN");
    for index in 0..256_u32 {
        for level in 0..256_u32 {
            bytes.push(if level == 128 {
                index as u8
            } else {
                ((index * 31 + level * 7 + 3) % 251 + 1) as u8
            });
        }
    }
    for a in 0..256_u32 {
        for b in 0..256_u32 {
            bytes.push(((a * 5 + b * 11 + 1) % 253 + 2) as u8);
        }
    }
    bytes.extend((0..65536_u32).map(|at| (at % 250 + 1) as u8));
    Palette::parse(&bytes).unwrap()
}

/// The vectors' art: a 4 × 1 and a 2 × 2 sprite of index 9 (critter art
/// ids 1 and 2), a 2 × 2 wall and nothing for the floor.
fn vector_art() -> Keyed {
    let mut art = BTreeMap::new();
    art.insert((false, CRITTER), pattern(4, 1, |_, _| 9));
    art.insert((false, CRITTER + 1), pattern(2, 2, |_, _| 9));
    art.insert((false, WALL), pattern(2, 2, |_, _| 9));
    art.insert((false, ITEM), pattern(2, 2, |_, _| 9));
    Keyed(art)
}

fn vector_view() -> MapView {
    MapView {
        roofs: false,
        ..view_at(20100, [640, 380])
    }
}

/// An object of `art` on `hex`, its `width` × `height` frame drawn with its
/// top-left corner at canvas `at` (through its displacement).
fn placed(map: &MapFile, id: u32, art: u32, hex: i32, at: [i64; 2], size: [i64; 2]) -> MapObject {
    let mut object = object(map, id, art, hex);
    let center = vector_view().grid.hex_center(hex).unwrap();
    object.displacement = [
        (at[0] + size[0] / 2 - center[0]) as i32,
        (at[1] + size[1] - 1 - center[1]) as i32,
    ];
    object
}

/// The ambient-65536 light of the vectors.
fn vector_light<'a>(palette: &'a Palette, tiles: &'a [i64]) -> MapLight<'a> {
    MapLight {
        palette,
        ambient: 65536,
        tiles,
    }
}

/// Vector B's backdrop: 4 × 1 at canvas (100, 100), colour 07, depth
/// 0, 0, 20509, 20509, with the fade bit on every pixel when `faded`.
fn vector_b(faded: bool) -> Planes {
    let depth = [0, 0, 0x501D, 0x501D];
    Planes::paint([4, 1], 64, (true, faded), |x, _| {
        (7, depth[x as usize], faded)
    })
}

/// The four canvas bytes at (100, 100)-(103, 100).
fn row(image: &IndexedImage) -> [u8; 4] {
    let at = 100 * image.width() as usize + 100;
    image.pixels()[at..at + 4].try_into().unwrap()
}

#[test]
fn backdrop_vector_b_exact_bytes_on_both_paths() {
    let _gpu = gpu::test_lock();
    let mut gpu = gpu_executor();
    let (map, art, palette) = (preview(), vector_art(), vector_palette());
    let light_tiles = vec![0_i64; 40000];
    let view = vector_view();
    let planes = vector_b(false);
    let tiles = planes.tiles();
    let view_b = backdrop(&tiles, planes.size, origin_px(view, [100, 100]), &[], false);
    for (hex, want) in [
        (Some(20100), [9, 9, 7, 7]),
        (Some(20508), [9, 9, 7, 7]),
        (Some(20509), [9, 9, 9, 9]),
        (None, [7, 7, 7, 7]),
    ] {
        let objects = scene(
            hex.map(|hex| placed(&map, 1, CRITTER, hex, [100, 100], [4, 1]))
                .into_iter()
                .collect(),
        );
        let scene = MapScene {
            light: Some(vector_light(&palette, &light_tiles)),
            backdrop: Some(&view_b),
            ..MapScene::default()
        };
        let image = render_map_scene(&map, &objects, view, &art, scene).unwrap();
        assert_eq!(row(&image), want, "object on {hex:?}");
        parity(
            &format!("vector B, {hex:?}"),
            &map,
            &objects,
            view,
            &art,
            scene,
            &mut gpu,
        );
    }
}

#[test]
fn backdrop_vector_c_sort_keys() {
    let group = SortKey::backdrop_group(20508).unwrap();
    assert_eq!(group.0, 0x4000_501C_FFFF_FFFF);
    assert_eq!(group, SortKey::new(2, 20508, 0xFFFF_FFFF).unwrap());
    let on_hex = SortKey::object(FrameLayer::Upright, 20508, 0, 0).unwrap();
    let next = SortKey::object(FrameLayer::Upright, 20509, 0, 0).unwrap();
    assert_eq!(on_hex.0, 0x4000_501C_0000_0000);
    assert_eq!(next.0, 0x4000_501D_0000_0000);
    let mut keys = vec![next, group, on_hex];
    keys.sort();
    assert_eq!(keys, [on_hex, group, next]);
    // The last list index and part of a hex sort before its group.
    let last = SortKey::object(FrameLayer::Upright, 20508, (1 << 29) - 1, 4).unwrap();
    assert!(last < group);
    // The records: a distinct backdrop record, not list index 0x1FFFFFFF
    // part 7.
    let record = DrawRecord::of_key(group).unwrap();
    assert_eq!(record, DrawRecord::backdrop_group(20508));
    assert_eq!(
        record,
        DrawRecord::new(FrameLayer::Upright, 20508, u32::MAX, 0)
    );
    assert!(record.is_backdrop_group());
    assert_ne!(
        record,
        DrawRecord::new(FrameLayer::Upright, 20508, 0x1FFF_FFFF, 7)
    );
    assert!(!DrawRecord::of_key(last).unwrap().is_backdrop_group());
    // The floor pass: after the last square, before every flat object.
    let floor = SortKey::backdrop_floor();
    assert_eq!(floor.0, 0x0000_2710_0000_0000);
    assert_eq!(floor, SortKey::new(0, 10000, 0).unwrap());
    assert!(SortKey::floor(9999).unwrap() < floor);
    assert!(floor < SortKey::object(FrameLayer::Flat, 0, 0, 0).unwrap());
    assert_eq!(
        DrawRecord::of_key(floor).unwrap(),
        DrawRecord::backdrop_floor()
    );
    assert_eq!(
        DrawRecord::backdrop_floor(),
        DrawRecord::new(FrameLayer::Floor, 10000, 0, 0)
    );
}

#[test]
fn backdrop_sort_key_domains_accept_exactly_the_two_new_keys() {
    let refused = |result: Result<SortKey, InstanceError>| {
        matches!(result, Err(InstanceError::Domain { .. }))
    };
    // Layer 2: only secondary 0xFFFFFFFF beyond part 4, below hex 40000.
    assert!(SortKey::new(2, 0, 0xFFFF_FFFF).is_ok());
    assert!(SortKey::new(2, 39999, 0xFFFF_FFFF).is_ok());
    assert!(SortKey::backdrop_group(39999).is_ok());
    for secondary in [0xFFFF_FFFE, 0xFFFF_FFFD, 0xFFFF_FFF7, 0xFFFF_FFF5, 5, 6, 7] {
        assert!(refused(SortKey::new(2, 20508, secondary)), "{secondary:X}");
    }
    // List index 2^29 - 1, part 4 and below: object keys, as before.
    assert!(SortKey::new(2, 20508, 0xFFFF_FFFC).is_ok());
    assert!(SortKey::new(2, 20508, 0xFFFF_FFF8).is_ok());
    assert!(refused(SortKey::new(2, 40000, 0xFFFF_FFFF)));
    assert!(refused(SortKey::backdrop_group(40000)));
    assert!(refused(SortKey::backdrop_group(u32::MAX)));
    // Not in the flat layer, nor through the object constructor.
    assert!(refused(SortKey::new(1, 20508, 0xFFFF_FFFF)));
    for layer in [FrameLayer::Flat, FrameLayer::Upright] {
        assert!(refused(SortKey::object(layer, 20508, (1 << 29) - 1, 7)));
        for part in 5..=7 {
            assert!(refused(SortKey::object(layer, 0, 0, part)), "{part}");
        }
    }
    // Layer 0: only (10000, 0) past the squares.
    assert!(refused(SortKey::floor(10000)));
    assert!(refused(SortKey::new(0, 10000, 1)));
    assert!(refused(SortKey::new(0, 10001, 0)));
    assert!(refused(SortKey::new(0, 0, 1)));
    // The other layers keep their domains.
    assert!(refused(SortKey::new(3, 0, 0xFFFF_FFFF)));
    assert!(refused(SortKey::new(4, 0, 0xFFFF_FFFF)));
    assert!(refused(SortKey::new(7, 0, 0xFFFF_FFFF)));
    assert!(refused(SortKey::new(3, 10001, 0)));
    assert!(
        SortKey::new(5, 0, 0xFFFF_FFFF).is_ok(),
        "creation ranks as before"
    );
}

#[test]
fn backdrop_vector_d_fade_through_the_player_mask() {
    let _gpu = gpu::test_lock();
    let mut gpu = gpu_executor();
    let (map, art, palette) = (preview(), vector_art(), vector_palette());
    let light_tiles = vec![0_i64; 40000];
    let view = vector_view();
    let planes = vector_b(true);
    let tiles = planes.tiles();
    let view_d = backdrop(&tiles, planes.size, origin_px(view, [100, 100]), &[], false);
    let mask = pattern(129, 98, |_, _| 1);
    // The player's sprite on 20100 at (100, 100).
    let objects = scene(vec![placed(&map, 1, CRITTER, 20100, [100, 100], [4, 1])]);
    let blended = palette.additive(palette.intensity(7, 1), palette.intensity(9, 127));
    assert_ne!(blended, 7);
    for (player, want) in [(20100, [9, 9, blended, blended]), (20600, [9, 9, 7, 7])] {
        // The mask's corner at (90, 90): it covers the four pixels.
        let center = view.grid.hex_center(player).unwrap();
        let cutout = Cutout {
            mask: mask.frame(0, 0).unwrap(),
            hex: player,
            displacement: [(90 + 64 - center[0]) as i32, (90 + 72 - center[1]) as i32],
            hidden: false,
            extended: &|_| 0,
            palette: &palette,
        };
        let scene = MapScene {
            light: Some(vector_light(&palette, &light_tiles)),
            cutout: Some(&cutout),
            backdrop: Some(&view_d),
            ..MapScene::default()
        };
        let image = render_map_scene(&map, &objects, view, &art, scene).unwrap();
        assert_eq!(row(&image), want, "player on {player}");
        parity(
            &format!("vector D, player on {player}"),
            &map,
            &objects,
            view,
            &art,
            scene,
            &mut gpu,
        );
        // The faded group is one masked instance with a selector.
        let tables = LookupTables::from_palette(&palette);
        let frame = gather_map_frame(&map, &objects, view, &art, scene, Some(&tables)).unwrap();
        let group = frame
            .instances
            .iter()
            .find(|instance| instance.key == SortKey::backdrop_group(20508).unwrap())
            .unwrap();
        assert_eq!(group.mode == DrawMode::Egg, player == 20100);
        assert_eq!(group.flags & SELECT != 0, player == 20100);
        // A hidden player fades nothing.
        let hidden = Cutout {
            hidden: true,
            ..cutout
        };
        let image = render_map_scene(
            &map,
            &objects,
            view,
            &art,
            MapScene {
                cutout: Some(&hidden),
                ..scene
            },
        )
        .unwrap();
        assert_eq!(row(&image)[2..], [7, 7], "hidden player on {player}");
    }
}

/// Vector A's 4 × 4-hex patch (columns 100-103, rows 100-103), the whole
/// canvas as the image: colour 7 over the patch, depth 0 over rows 100-101
/// and 20504 (group 20503) over rows 102-103.
fn vector_a(view: MapView) -> Planes {
    Planes::paint([640, 380], 64, (true, false), |x, y| {
        match view.grid.pick_hex([x as i32, y as i32]) {
            Some(hex) if (100..104).contains(&(hex % 200)) && (100..104).contains(&(hex / 200)) => {
                (7, if hex / 200 >= 102 { 20504 } else { 0 }, false)
            }
            _ => (0, 0, false),
        }
    })
}

#[test]
fn backdrop_vector_a_a_two_level_depth_step() {
    let _gpu = gpu::test_lock();
    let mut gpu = gpu_executor();
    let (map, art, palette) = (preview(), vector_art(), vector_palette());
    let light_tiles = vec![0_i64; 40000];
    let view = vector_view();
    let planes = vector_a(view);
    let tiles = planes.tiles();
    let view_a = backdrop(&tiles, planes.size, origin_px(view, [0, 0]), &[], false);
    // A point in each half: the center of hex 20702 (row 103) and 20101
    // (row 100); the 2 × 2 sprite covers it and the pixels right and below.
    let low = view.grid.hex_center(20702).unwrap();
    let high = view.grid.hex_center(20101).unwrap();
    for (hex, in_low, in_high) in [(20301, 7, 9), (20701, 9, 9), (20503, 7, 9)] {
        for (point, want) in [(low, in_low), (high, in_high)] {
            let objects = scene(vec![placed(&map, 1, CRITTER + 1, hex, point, [2, 2])]);
            let scene = MapScene {
                light: Some(vector_light(&palette, &light_tiles)),
                backdrop: Some(&view_a),
                ..MapScene::default()
            };
            let image = render_map_scene(&map, &objects, view, &art, scene).unwrap();
            for [dx, dy] in [[0, 0], [1, 0], [0, 1], [1, 1]] {
                let at = (point[1] + dy) as usize * 640 + (point[0] + dx) as usize;
                assert_eq!(image.pixels()[at], want, "sprite on {hex} at {point:?}");
            }
            parity(
                &format!("vector A, {hex} at {point:?}"),
                &map,
                &objects,
                view,
                &art,
                scene,
                &mut gpu,
            );
        }
    }
}

// ---------------------------------------------------------------------
// Parity over the full fixture scene.

/// A scenic backdrop over `base`'s canvas, image pixel = canvas pixel +
/// `at`: patterned colours with holes and indexes 229 and up; depth by
/// 24 × 16 blocks: floor, the hex under the pixel, two rows lower, or one
/// row higher (so groups interleave with objects on lower, equal and
/// higher hexes), and one block of group 20502 across a tile seam; the fade
/// bit on four pixels in five.
fn scenic(base: MapView, at: [i64; 2], size: [u32; 2], chunk: u32, planes: (bool, bool)) -> Planes {
    Planes::paint(size, chunk, planes, |x, y| {
        let colour = match (x * 7 + y * 5) % 29 {
            0 => 0,
            1 => 235,
            value => 60 + value as u8,
        };
        let canvas = [i64::from(x) + at[0], i64::from(y) + at[1]];
        let hex = base.grid.pick_hex([canvas[0] as i32, canvas[1] as i32]);
        let group = if (50..80).contains(&x) && (100..140).contains(&y) {
            Some(20502)
        } else {
            hex.and_then(|hex| match (x / 24 + y / 16) % 4 {
                1 => Some(hex),
                2 => Some(hex - 401),
                3 => Some(hex + 199),
                _ => None,
            })
        };
        let depth = group
            .filter(|group| (0..40000).contains(group))
            .map_or(0, |group| group as u16 + 1);
        (colour, depth, (x + 3 * y) % 5 != 0)
    })
}

/// The whole-canvas scenic backdrop of `view` (image pixel = canvas pixel)
/// in 64-pixel tiles.
fn scenic_of(view: MapView, planes: (bool, bool)) -> Planes {
    scenic(view, [0, 0], [view.width, view.height], 64, planes)
}

#[test]
fn backdrop_parity_floor_groups_and_fade_over_the_full_scene() {
    let _gpu = gpu::test_lock();
    let fixture = Fixture::new();
    let cutout = fixture.cutout();
    let mut gpu = gpu_executor();
    let view = view_at(20100, [640, 380]);
    let tables = LookupTables::from_palette(&fixture.palette);
    for (label, planes, options) in [
        ("floor only", (false, false), Options::FULL),
        ("groups", (true, false), Options::FULL),
        ("groups faded", (true, true), Options::FULL),
        (
            "groups faded, no cutout",
            (true, true),
            Options {
                cutout: false,
                ..Options::FULL
            },
        ),
        (
            "unlit",
            (true, true),
            Options {
                lit: false,
                cutout: false,
                outlines: false,
                ..Options::FULL
            },
        ),
        (
            "unlit with the cutout",
            (true, true),
            Options {
                lit: false,
                outlines: false,
                ..Options::FULL
            },
        ),
    ] {
        let image = scenic_of(view, planes);
        let tiles = image.tiles();
        let back = backdrop(&tiles, image.size, origin_px(view, [0, 0]), &[], false);
        let scene = MapScene {
            backdrop: Some(&back),
            ..scene_of(&fixture, &cutout, view, options)
        };
        parity(
            label,
            &fixture.map,
            &fixture.objects,
            view,
            &fixture.art,
            scene,
            &mut gpu,
        );
        // The backdrop shows, its holes keep the floor squares, and the
        // draw items are the ones the brief names.
        let bare = render_map_scene(
            &fixture.map,
            &fixture.objects,
            view,
            &fixture.art,
            MapScene {
                backdrop: None,
                ..scene
            },
        )
        .unwrap();
        let drawn =
            render_map_scene(&fixture.map, &fixture.objects, view, &fixture.art, scene).unwrap();
        assert_ne!(drawn.pixels(), bare.pixels(), "{label}");
        let frame = gather_map_frame(
            &fixture.map,
            &fixture.objects,
            view,
            &fixture.art,
            scene,
            Some(&tables),
        )
        .unwrap();
        let keys: Vec<SortKey> = frame.instances.iter().map(|item| item.key).collect();
        assert!(keys.contains(&SortKey::backdrop_floor()), "{label}");
        let groups = keys.iter().filter(|key| key.is_backdrop_group()).count();
        assert_eq!(groups > 0, planes.0, "{label}");
        let selected = frame
            .instances
            .iter()
            .filter(|item| item.flags & SELECT != 0)
            .count();
        assert_eq!(
            selected > 0,
            planes.1 && options.cutout,
            "{label}: masked groups"
        );
        // A hole in the backdrop over a floor square shows the square.
        let hole = (0..640 * 380)
            .find(|at| {
                let (x, y) = (at % 640, at / 640);
                (x * 7 + y * 5) % 29 == 0 && bare.pixels()[*at as usize] != 0
            })
            .unwrap() as usize;
        assert_eq!(drawn.pixels()[hole], bare.pixels()[hole], "{label}: hole");
    }
}

#[test]
fn backdrop_parity_cursor_fade_and_view_positions() {
    let _gpu = gpu::test_lock();
    let fixture = Fixture::new();
    let cutout = fixture.cutout();
    let mut gpu = gpu_executor();
    let base = view_at(20100, [640, 380]);
    let image = scenic_of(base, (true, true));
    let tiles = image.tiles();
    let back = backdrop(&tiles, image.size, origin_px(base, [0, 0]), &[], false);
    // The cursor fade around the player, with roofs shown and hidden (the
    // backdrop's cursor fade follows the cutaway either way).
    let center = base.grid.hex_center(20104).unwrap();
    for roofs in [true, false] {
        let view = MapView { roofs, ..base };
        let scene = MapScene {
            backdrop: Some(&back),
            roof_cutaway: Some(reference_cutaway(center, 1.0)),
            ..scene_of(&fixture, &cutout, view, Options::FULL)
        };
        parity(
            &format!("cursor fade, roofs {roofs}"),
            &fixture.map,
            &fixture.objects,
            view,
            &fixture.art,
            scene,
            &mut gpu,
        );
        let unfaded = render_map_scene(
            &fixture.map,
            &fixture.objects,
            view,
            &fixture.art,
            MapScene {
                roof_cutaway: None,
                ..scene
            },
        )
        .unwrap();
        let faded =
            render_map_scene(&fixture.map, &fixture.objects, view, &fixture.art, scene).unwrap();
        assert_ne!(faded.pixels(), unfaded.pixels(), "roofs {roofs}");
    }
    // Other view positions, a wider view, the overscan, and refresh tiles
    // of a two-tile canvas: the backdrop moves with the view.
    let wide = view_at(20100, [1280, 380]);
    let enhanced = Options {
        overscan: true,
        ..Options::FULL
    };
    let mut views = vec![
        ("panned", view_at(19905, [640, 380]), Options::FULL),
        ("lower right", view_at(20520, [640, 380]), Options::FULL),
        ("overscan", base, enhanced),
        ("wide", wide, Options::FULL),
        ("small", view_at(20302, [200, 120]), Options::FULL),
    ];
    for corner in [[0, 0], [640, 0]] {
        views.push(("wide tile", tile_of(wide, corner, [640, 380]), enhanced));
    }
    for (label, view, options) in views {
        let scene = MapScene {
            backdrop: Some(&back),
            ..scene_of(&fixture, &cutout, view, options)
        };
        parity(
            &format!("{label} at {:?}", view.grid.origin),
            &fixture.map,
            &fixture.objects,
            view,
            &fixture.art,
            scene,
            &mut gpu,
        );
    }
}

#[test]
fn backdrop_parity_smaller_than_the_view_and_partly_outside_it() {
    let _gpu = gpu::test_lock();
    let fixture = Fixture::new();
    let cutout = fixture.cutout();
    let mut gpu = gpu_executor();
    let view = view_at(20100, [640, 380]);
    for (label, at, size, chunk) in [
        ("inside", [200, 120], [180, 100], 64),
        ("past the bottom left", [-50, 300], [200, 150], 64),
        ("past the top right", [560, -40], [300, 120], 128),
        ("off the view", [700, 0], [100, 100], 64),
    ] {
        let image = scenic(view, at, size, chunk, (true, true));
        let tiles = image.tiles();
        let back = backdrop(&tiles, image.size, origin_px(view, at), &[], false);
        let scene = MapScene {
            backdrop: Some(&back),
            ..scene_of(&fixture, &cutout, view, Options::FULL)
        };
        parity(
            label,
            &fixture.map,
            &fixture.objects,
            view,
            &fixture.art,
            scene,
            &mut gpu,
        );
        let drawn =
            render_map_scene(&fixture.map, &fixture.objects, view, &fixture.art, scene).unwrap();
        let bare = render_map_scene(
            &fixture.map,
            &fixture.objects,
            view,
            &fixture.art,
            MapScene {
                backdrop: None,
                ..scene
            },
        )
        .unwrap();
        // Outside the image the canvas is as without a backdrop.
        for (index, (got, want)) in drawn.pixels().iter().zip(bare.pixels()).enumerate() {
            let (x, y) = ((index % 640) as i64, (index / 640) as i64);
            let inside = (at[0]..at[0] + i64::from(size[0])).contains(&x)
                && (at[1]..at[1] + i64::from(size[1])).contains(&y);
            if !inside {
                assert_eq!(got, want, "{label}: ({x}, {y})");
            }
        }
        // The rectangle the cache decodes.
        let rect = backdrop_rect(&view, 20100, origin_px(view, at), size);
        let want = {
            let x0 = (-at[0]).max(0);
            let y0 = (-at[1]).max(0);
            let x1 = (640 - at[0]).min(i64::from(size[0]));
            let y1 = (380 - at[1]).min(i64::from(size[1]));
            (x0 < x1 && y0 < y1).then(|| [x0, y0, x1, y1].map(|value| value as u32))
        };
        assert_eq!(rect, want, "{label}");
    }
}

#[test]
fn backdrop_parity_suppression_keeps_list_indexes() {
    let _gpu = gpu::test_lock();
    let fixture = Fixture::new();
    let cutout = fixture.cutout();
    let mut gpu = gpu_executor();
    let view = view_at(20100, [640, 380]);
    let image = scenic_of(view, (true, true));
    let tiles = image.tiles();
    let scenery_pid = SCENERY & 0x0f00_0000 | 1;
    let pids = [7_u32, scenery_pid, 0x0500_0000];
    for (label, walls, pids) in [
        ("walls", true, &[][..]),
        ("listed PIDs", false, &pids[..]),
        ("both", true, &pids[..]),
        ("unsorted PIDs", false, &[0x0500_0000, scenery_pid, 7][..]),
    ] {
        let back = backdrop(&tiles, image.size, origin_px(view, [0, 0]), pids, walls);
        let scene = MapScene {
            backdrop: Some(&back),
            ..scene_of(&fixture, &cutout, view, Options::FULL)
        };
        parity(
            label,
            &fixture.map,
            &fixture.objects,
            view,
            &fixture.art,
            scene,
            &mut gpu,
        );
        let mut log = Vec::new();
        let mut record = |entry: DrawRecord| log.push(entry);
        render_map_scene_recorded(
            &fixture.map,
            &fixture.objects,
            view,
            &fixture.art,
            scene,
            Some(&mut record),
        )
        .unwrap();
        let drawn = |hex: u32, index: u32| {
            log.iter().any(|entry| {
                matches!(entry.layer, FrameLayer::Upright)
                    && entry.primary == hex
                    && entry.index == index
            })
        };
        // The walls on 20300, 20302 and 20500 (list index 0).
        for hex in [20300, 20302, 20500] {
            assert_eq!(drawn(hex, 0), !walls, "{label}: wall on {hex}");
        }
        // The scenery on 20100 is list index 0, the player 1: with the
        // scenery suppressed the player keeps index 1.
        let scenery = pids.contains(&scenery_pid);
        assert_eq!(drawn(20100, 0), !scenery, "{label}: scenery");
        assert!(drawn(20100, 1), "{label}: the player keeps list index 1");
    }
    // Another elevation's backdrop draws and suppresses nothing.
    let mut other = backdrop(&tiles, image.size, origin_px(view, [0, 0]), &pids, true);
    other.elevation = 1;
    let base = scene_of(&fixture, &cutout, view, Options::FULL);
    let render = |scene| {
        render_map_scene(&fixture.map, &fixture.objects, view, &fixture.art, scene).unwrap()
    };
    assert_canvas(
        "another elevation",
        &render(MapScene {
            backdrop: Some(&other),
            ..base
        }),
        &render(base),
    );
}

/// No pixel, no list: a backdrop that draws nothing leaves the canvas and
/// the records as without one.
#[test]
fn backdrop_an_empty_backdrop_changes_nothing() {
    let _gpu = gpu::test_lock();
    let fixture = Fixture::new();
    let cutout = fixture.cutout();
    let mut gpu = gpu_executor();
    let view = view_at(20100, [640, 380]);
    let transparent = Planes::paint([640, 380], 64, (true, true), |_, _| (0, 5, true));
    let tiles = transparent.tiles();
    for chunks in [&[][..], &tiles[..]] {
        let back = backdrop(chunks, [640, 380], origin_px(view, [0, 0]), &[], false);
        let with = MapScene {
            backdrop: Some(&back),
            ..scene_of(&fixture, &cutout, view, Options::FULL)
        };
        let without = MapScene {
            backdrop: None,
            ..with
        };
        let records = |scene| {
            let mut log = Vec::new();
            let mut record = |entry: DrawRecord| log.push(entry);
            let image = render_map_scene_recorded(
                &fixture.map,
                &fixture.objects,
                view,
                &fixture.art,
                scene,
                Some(&mut record),
            )
            .unwrap();
            (image, log)
        };
        let (got, got_log) = records(with);
        let (want, want_log) = records(without);
        assert_canvas("empty backdrop", &got, &want);
        assert_eq!(got_log, want_log);
        parity(
            "empty backdrop",
            &fixture.map,
            &fixture.objects,
            view,
            &fixture.art,
            with,
            &mut gpu,
        );
    }
}

#[test]
fn backdrop_malformed_tiles_are_skipped_without_panicking() {
    let _gpu = gpu::test_lock();
    let fixture = Fixture::new();
    let cutout = fixture.cutout();
    let mut gpu = gpu_executor();
    let view = view_at(20100, [640, 380]);
    let image = scenic_of(view, (true, true));
    let good = image.tiles();
    let mut render = |chunks: &[BackdropTile], origin_hex: i32| {
        let mut back = backdrop(chunks, image.size, origin_px(view, [0, 0]), &[], true);
        back.origin_hex = origin_hex;
        let scene = MapScene {
            backdrop: Some(&back),
            ..scene_of(&fixture, &cutout, view, Options::FULL)
        };
        parity(
            "malformed",
            &fixture.map,
            &fixture.objects,
            view,
            &fixture.art,
            scene,
            &mut gpu,
        );
        render_map_scene(&fixture.map, &fixture.objects, view, &fixture.art, scene).unwrap()
    };
    // The bad tile replaces tile 3; the rest draw as before.
    let without: Vec<BackdropTile> = good
        .iter()
        .enumerate()
        .filter(|(at, _)| *at != 3)
        .map(|(_, tile)| *tile)
        .collect();
    let want = render(&without, 20100);
    let short = &good[3].colour[1..];
    let deep: Vec<u16> = (0..good[3].colour.len())
        .map(|at| if at == 77 { 40001 } else { 1 })
        .collect();
    let fade = &good[3].fade.unwrap()[1..];
    let depth = &good[3].depth.unwrap()[1..];
    for (label, tile) in [
        (
            "short colour",
            BackdropTile {
                colour: short,
                ..good[3]
            },
        ),
        (
            "depth 40001",
            BackdropTile {
                depth: Some(&deep),
                ..good[3]
            },
        ),
        (
            "short fade",
            BackdropTile {
                fade: Some(fade),
                ..good[3]
            },
        ),
        (
            "short depth",
            BackdropTile {
                depth: Some(depth),
                ..good[3]
            },
        ),
        (
            "past the image",
            BackdropTile {
                origin: [image.size[0] - 10, 0],
                ..good[3]
            },
        ),
        (
            "empty",
            BackdropTile {
                size: [0, 0],
                colour: &[],
                depth: Some(&[]),
                fade: Some(&[]),
                ..good[3]
            },
        ),
    ] {
        let mut chunks = good.clone();
        chunks[3] = tile;
        assert_canvas(label, &render(&chunks, 20100), &want);
    }
    // A tile overlapping an earlier one is skipped.
    let mut overlapping = good.clone();
    overlapping.push(BackdropTile {
        origin: [10, 10],
        ..good[0]
    });
    assert_canvas(
        "overlapping",
        &render(&overlapping, 20100),
        &render(&good, 20100),
    );
    // An origin hex off the grid draws nothing and suppresses nothing.
    let base = scene_of(&fixture, &cutout, view, Options::FULL);
    let bare = render_map_scene(&fixture.map, &fixture.objects, view, &fixture.art, base).unwrap();
    for hex in [-1, 40000] {
        assert_canvas("origin off the grid", &render(&good, hex), &bare);
        assert_eq!(backdrop_rect(&view, hex, [0, 0], [640, 380]), None);
    }
}

// ---------------------------------------------------------------------
// Picking.

#[test]
fn backdrop_picking_follows_the_drawing_order() {
    let (map, art) = (preview(), vector_art());
    let view = vector_view();
    let planes = vector_a(view);
    let tiles = planes.tiles();
    let low = view.grid.hex_center(20702).unwrap();
    let high = view.grid.hex_center(20101).unwrap();
    let point = |at: [i64; 2]| [at[0] as i32, at[1] as i32];
    let pick = |objects: &MapObjects, at: [i64; 2], back: Option<&BackdropView>| {
        let plain = pick_candidates_backdrop(&map, objects, view, &art, point(at), None, back);
        let cut =
            pick_candidates_cut_backdrop(&map, objects, view, &art, point(at), None, None, back);
        assert_eq!(plain, cut);
        plain
            .unwrap()
            .into_iter()
            .map(|(id, _)| id)
            .collect::<Vec<u32>>()
    };
    let back = backdrop(&tiles, planes.size, origin_px(view, [0, 0]), &[], false);
    for (hex, covered_low) in [(20301, true), (20503, true), (20701, false)] {
        for (at, covered) in [(low, covered_low), (high, false)] {
            let objects = scene(vec![placed(&map, 1, CRITTER + 1, hex, at, [2, 2])]);
            let hits = pick(&objects, at, Some(&back));
            assert_eq!(hits.is_empty(), covered, "sprite on {hex} at {at:?}");
            // Without a backdrop, as before.
            assert_eq!(pick(&objects, at, None), [1]);
            assert_eq!(
                pick_candidates(&map, &objects, view, &art, point(at), None)
                    .unwrap()
                    .len(),
                1
            );
        }
    }
    // A flat object draws before every group: covered at any group pixel.
    let mut flat = placed(&map, 2, ITEM, 20701, low, [2, 2]);
    flat.flags.0 |= BasicFlags::FLAT.0;
    assert!(pick(&scene(vec![flat.clone()]), low, Some(&back)).is_empty());
    let mut flat_high = placed(&map, 2, ITEM, 20701, high, [2, 2]);
    flat_high.flags.0 |= BasicFlags::FLAT.0;
    assert_eq!(pick(&scene(vec![flat_high]), high, Some(&back)), [2]);
    // The cursor fade never uncovers: the backdrop covers whatever fades.
    // A suppressed object is never hit, covered or not, and the others on
    // its hex still are.
    let wall = placed(&map, 3, WALL, 20701, high, [2, 2]);
    let critter = placed(&map, 4, CRITTER + 1, 20701, high, [2, 2]);
    let objects = scene(vec![wall, critter]);
    let suppressing = backdrop(&tiles, planes.size, origin_px(view, [0, 0]), &[], true);
    assert_eq!(pick(&objects, high, Some(&back)), [4, 3]);
    assert_eq!(pick(&objects, high, Some(&suppressing)), [4]);
    let pids = [WALL & 0x0f00_0000 | 1];
    let listed = backdrop(&tiles, planes.size, origin_px(view, [0, 0]), &pids, false);
    assert_eq!(pick(&objects, high, Some(&listed)), [4]);
}

#[test]
fn backdrop_chunks_under_a_rectangle() {
    let chunks: Vec<[u32; 2]> = backdrop_chunks([50, 0, 200, 80], 64).collect();
    assert_eq!(
        chunks,
        [
            [0, 0],
            [1, 0],
            [2, 0],
            [3, 0],
            [0, 1],
            [1, 1],
            [2, 1],
            [3, 1]
        ]
    );
    assert_eq!(backdrop_chunks([0, 0, 64, 64], 64).count(), 1);
    assert_eq!(backdrop_chunks([0, 0, 65, 64], 64).count(), 2);
    assert_eq!(backdrop_chunks([10, 10, 10, 20], 64).count(), 0);
    assert_eq!(backdrop_chunks([0, 0, 100, 100], 0).count(), 0);
    // The rectangle a refresh covers moves with the view.
    let view = view_at(20100, [640, 380]);
    let origin = origin_px(view, [0, 0]);
    assert_eq!(
        backdrop_rect(&view, 20100, origin, [4096, 4096]),
        Some([0, 0, 640, 380])
    );
    let mut panned = view;
    panned.pan(-100, 30);
    assert_eq!(
        backdrop_rect(&panned, 20100, origin, [4096, 4096]),
        Some([100, 0, 740, 350])
    );
    assert_eq!(backdrop_rect(&panned, 20100, origin, [50, 50]), None);
}

/// The instanced path's masked group: every pixel the selector leaves out
/// is the lit draw with the exemption, every selected one the masked blend
/// without it, on the CPU executor as on the reference (indexes 229 and up
/// inside the mask included).
#[test]
fn backdrop_selected_egg_pixels_relight_without_the_exemption() {
    let _gpu = gpu::test_lock();
    let mut gpu = gpu_executor();
    let (map, art, palette) = (preview(), vector_art(), vector_palette());
    // A dim, non-uniform light so the intensity table changes indexes.
    let light_tiles: Vec<i64> = (0..40000).map(|hex| (hex % 97) * 400).collect();
    let view = vector_view();
    let planes = Planes::paint([40, 12], 64, (true, true), |x, y| {
        let colour = if x % 3 == 0 { 240 } else { 100 + x as u8 };
        (colour, 20509, y % 2 == 0)
    });
    let tiles = planes.tiles();
    let back = backdrop(&tiles, planes.size, origin_px(view, [100, 100]), &[], false);
    let mask = pattern(129, 98, |x, y| ((x + 2 * y) % 140) as u8);
    let center = view.grid.hex_center(20100).unwrap();
    let cutout = Cutout {
        mask: mask.frame(0, 0).unwrap(),
        hex: 20100,
        displacement: [(90 + 64 - center[0]) as i32, (90 + 72 - center[1]) as i32],
        hidden: false,
        extended: &|_| 0,
        palette: &palette,
    };
    let objects = scene(vec![placed(&map, 1, CRITTER, 20100, [104, 104], [4, 1])]);
    let scene = MapScene {
        light: Some(MapLight {
            palette: &palette,
            ambient: 6000,
            tiles: &light_tiles,
        }),
        cutout: Some(&cutout),
        backdrop: Some(&back),
        ..MapScene::default()
    };
    parity(
        "selected egg pixels",
        &map,
        &objects,
        view,
        &art,
        scene,
        &mut gpu,
    );
    let tables = LookupTables::from_palette(&palette);
    let frame = gather_map_frame(&map, &objects, view, &art, scene, Some(&tables)).unwrap();
    let group = frame
        .instances
        .iter()
        .find(|instance| instance.key.is_backdrop_group())
        .unwrap();
    assert_eq!(group.mode, DrawMode::Egg);
    assert_eq!(group.flags, EXEMPT | SELECT);
    // The plan sorts the floor-less refresh with the group after the
    // player's hex.
    let order = sort_by_key(&frame.instances).unwrap();
    assert!(
        frame.instances[*order.last().unwrap()]
            .key
            .is_backdrop_group()
    );
    let _ = BatchPlan::new(&frame.instances).unwrap();
    // A selector-less masked instance is refused before any pixel.
    let mut broken = frame.instances.clone();
    for instance in &mut broken {
        if instance.flags & SELECT != 0 {
            instance.level = None;
        }
    }
    let inputs = frame.inputs(&tables);
    assert_eq!(inputs.validate(&broken), Err(InstanceError::SourceRange));
}

/// Translucent objects blend over the groups drawn before them, and the
/// art the fixture shares between the paths draws the same on both.
#[test]
fn backdrop_parity_translucent_objects_over_groups() {
    let _gpu = gpu::test_lock();
    let fixture = Fixture::new();
    let mut gpu = gpu_executor();
    let view = view_at(20708, [640, 380]);
    // Every pixel a group two rows below the hex under it: each
    // translucent scenery object (20704-20716) blends over groups.
    let image = Planes::paint([640, 380], 128, (true, false), |x, y| {
        let hex = view.grid.pick_hex([x as i32, y as i32]).unwrap_or(400);
        (50 + (x % 7) as u8, (hex - 400).max(0) as u16 + 1, false)
    });
    let tiles = image.tiles();
    let back = backdrop(&tiles, image.size, origin_px(view, [0, 0]), &[], false);
    let cutout = fixture.cutout();
    let scene = MapScene {
        backdrop: Some(&back),
        ..scene_of(
            &fixture,
            &cutout,
            view,
            Options {
                cutout: false,
                ..Options::FULL
            },
        )
    };
    parity(
        "translucent over groups",
        &fixture.map,
        &fixture.objects,
        view,
        &fixture.art,
        scene,
        &mut gpu,
    );
    let tables = LookupTables::from_palette(&fixture.palette);
    let frame = gather_map_frame(
        &fixture.map,
        &fixture.objects,
        view,
        &fixture.art,
        scene,
        Some(&tables),
    )
    .unwrap();
    assert!(
        frame
            .instances
            .iter()
            .any(|item| item.mode == DrawMode::Translucent)
    );
}
