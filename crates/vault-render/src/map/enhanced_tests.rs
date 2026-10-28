//! The `enhanced` preset's map layers: the roof cutaway, the cutaway-aware
//! picking and the shroud, over synthetic scenes.
use super::tests::{Keyed, grey_palette, object, preview, scene, view};
use super::*;
use crate::outline::OutlineInks;
use vault_formats::{FrmArchive, map::TilePair};

/// A `width` by `height` frame whose pixel (x, y) is `color(x, y)`.
fn pattern(width: u16, height: u16, color: impl Fn(u16, u16) -> u8) -> FrmArchive {
    let mut bytes = fixture::frm(width, height, 1);
    let start = bytes.len() - usize::from(width) * usize::from(height);
    for y in 0..height {
        for x in 0..width {
            bytes[start + usize::from(y) * usize::from(width) + usize::from(x)] = color(x, y);
        }
    }
    FrmArchive::parse(&bytes).unwrap()
}

/// FNV-1a over the pixels: a short digest of a frame.
fn digest(image: &IndexedImage) -> String {
    let hash = image
        .pixels()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
        });
    format!("{:016x}", hash)
}

/// The fixture map with patterned floors on every square of elevation 0
/// and a block of patterned roofs (id 3) over the view's center.
fn roofed_map() -> MapFile {
    let mut map = preview();
    let tiles = (0..10000)
        .map(|square: u32| {
            let (column, row) = (square % 100, square / 100);
            let roof = if (46..55).contains(&column) && (46..55).contains(&row) {
                3
            } else {
                1
            };
            let word = (roof << 16) | 4;
            TilePair { stored: word, word }
        })
        .collect();
    map.tiles[0] = Some(tiles);
    map
}

/// The art the rich scenes draw: patterned floors and roofs with holes,
/// and patterned objects of every drawn kind.
fn rich_art() -> Keyed {
    let mut art = std::collections::BTreeMap::new();
    art.insert(
        (true, 1),
        FrmArchive::parse(&fixture::frm(80, 36, 0)).unwrap(),
    );
    art.insert(
        (true, 4),
        pattern(80, 36, |x, y| 30 + ((x * 3 + y) % 60) as u8),
    );
    art.insert(
        (true, 3),
        pattern(80, 36, |x, y| {
            if (x + y) % 17 == 0 {
                0
            } else {
                100 + ((x + 2 * y) % 50) as u8
            }
        }),
    );
    for (id, base) in [
        (0x0000_0001, 140),
        (0x0100_0001, 160),
        (0x0200_0001, 180),
        (0x0300_0001, 200),
    ] {
        art.insert(
            (false, id),
            pattern(20, 30, move |x, y| base + ((x + y) % 20) as u8),
        );
    }
    Keyed(art)
}

/// The rich scene's objects: scenery, a wall the cutout cuts, an outlined
/// critter, a flat item, a translucent object and one without a hex.
fn rich_objects(map: &MapFile) -> MapObjects {
    let scenery = object(map, 1, 0x0200_0001, 20100);
    let wall = object(map, 2, 0x0300_0001, 20300);
    let mut critter = object(map, 3, 0x0100_0001, 19900);
    critter.outline = 3;
    let mut item = object(map, 4, 0x0000_0001, 20102);
    item.flags.0 |= BasicFlags::FLAT.0;
    let mut steam = object(map, 5, 0x0200_0001, 20500);
    steam.flags.0 |= 0x4000;
    let mut free = object(map, 6, 0x0200_0001, 0);
    free.hex = None;
    free.screen = [600, 10];
    scene(vec![scenery, wall, critter, item, steam, free])
}

/// The fixture view with roofs shown.
fn roofed_view() -> MapView {
    MapView {
        roofs: true,
        ..view()
    }
}

/// Everything a refresh can draw, lit or unlit, with the given `enhanced`
/// layers.
fn rich_render(
    lit: bool,
    roof_cutaway: Option<RoofCutaway>,
    shroud: Option<Shroud>,
) -> IndexedImage {
    render(roofed_view(), lit, true, roof_cutaway, shroud)
}

/// The rich scene in `view`; without `extras`, no outlines, floating
/// text, in-world cursor or hexless object (the layers drawn after the
/// shroud).
fn render(
    view: MapView,
    lit: bool,
    extras: bool,
    roof_cutaway: Option<RoofCutaway>,
    shroud: Option<Shroud>,
) -> IndexedImage {
    let map = roofed_map();
    let art = rich_art();
    let mut objects = rich_objects(&map);
    if !extras {
        objects.elevations[0].retain(|object| object.hex.is_some());
    }
    let palette = grey_palette();
    let mut tiles = vec![0; 40000];
    tiles[20100] = 65536;
    tiles[19900] = 50000;
    let light = MapLight {
        palette: &palette,
        ambient: 30000,
        tiles: &tiles,
    };
    let mask = FrmArchive::parse(&fixture::frm(129, 98, 64)).unwrap();
    let none = |_| 0;
    let cutout = Cutout {
        mask: mask.frame(0, 0).unwrap(),
        hex: 20100,
        displacement: [0; 2],
        hidden: false,
        extended: &none,
        palette: &palette,
    };
    let inks = OutlineInks::new(&palette);
    let mut text = IndexedImage::new(5, 3).unwrap();
    text.pixels_mut()[6] = 77;
    text.pixels_mut()[8] = 78;
    let floating = [crate::floating::FloatingSprite {
        hex: 20300,
        offset: [0, -20],
        image: text,
    }];
    let cursor_art = IndexedImage::from_pixels(6, 4, vec![77; 24]).unwrap();
    let ring = FrmArchive::parse(&fixture::frm(4, 4, 99)).unwrap();
    let ring = ring.frame(0, 0).unwrap();
    let [cx, cy] = view.grid.hex_center(20104).unwrap();
    let cursor = SceneCursor {
        hex: 20104,
        order: objects.elevations[0].len(),
        art: (&cursor_art, [cx - 3, cy - 2]),
        outline: Some((ring, [cx - 2, cy - 2], 3)),
        text: None,
    };
    let mut image = if lit {
        render_floor_lit(&map, view, &art, light).unwrap()
    } else {
        render_floor(&map, view, &art).unwrap()
    };
    let layers = MapScene {
        cutout: lit.then_some(&cutout),
        light: lit.then_some(light),
        outlines: extras.then_some(&inks),
        floating: if extras { &floating } else { &[] },
        cursor: extras.then_some(cursor),
        roof_cutaway,
        shroud,
        overscan: false,
        highlight: None,
    };
    draw_map_scene(&mut image, &map, &objects, view, &art, layers).unwrap();
    image
}

#[test]
fn vanilla_scenes_are_unchanged_without_the_enhanced_layers() {
    // Digests recorded from the renderer before the enhanced layers
    // existed: with both new fields `None` every pixel is the same.
    assert_eq!(digest(&rich_render(true, None, None)), "b53c03a4aa8a2574");
    assert_eq!(digest(&rich_render(false, None, None)), "9c9fc06ef3a314ba");
}

#[test]
fn far_cutaways_and_open_shrouds_leave_vanilla_scenes_unchanged() {
    let far = RoofCutaway {
        center: [5000, -5000],
        inner: 40,
        ring: 24,
    };
    let all = vec![true; 40000];
    let open = Shroud {
        allowed: &all,
        dark: 3,
    };
    for (lit, expected) in [(true, "b53c03a4aa8a2574"), (false, "9c9fc06ef3a314ba")] {
        assert_eq!(digest(&rich_render(lit, Some(far), None)), expected);
        assert_eq!(digest(&rich_render(lit, None, Some(open))), expected);
    }
}

/// The ring rule as specified, in floating point: with t = (d − inner) /
/// ring, a roof pixel is drawn iff `bayer < floor(16 t)` at its world
/// pixel.
fn ring_rule(cutaway: RoofCutaway, grid: HexGrid, [x, y]: [i64; 2]) -> bool {
    let [dx, dy] = [x - cutaway.center[0], y - cutaway.center[1]];
    let d = ((dx * dx + dy * dy) as f64).sqrt();
    let t = (d - f64::from(cutaway.inner)) / f64::from(cutaway.ring);
    let [wx, wy] = [x - grid.origin[0], y - grid.origin[1]];
    let bayer = BAYER[wy.rem_euclid(4) as usize][wx.rem_euclid(4) as usize];
    f64::from(bayer) < (t * 16.0).floor()
}

#[test]
fn the_cutaway_removes_roofs_inside_keeps_them_beyond_and_dithers_the_ring() {
    let cutaway = RoofCutaway {
        center: [290, 110],
        inner: 40,
        ring: 24,
    };
    let view = roofed_view();
    for lit in [true, false] {
        let full = render(view, lit, true, None, None);
        let bare = render(
            MapView {
                roofs: false,
                ..view
            },
            lit,
            true,
            None,
            None,
        );
        let cut = render(view, lit, true, Some(cutaway), None);
        let (mut kept, mut removed, mut inside) = (0, 0, 0);
        for y in 0..380_i64 {
            for x in 0..640_i64 {
                let at = (y * 640 + x) as usize;
                let [dx, dy] = [x - 290, y - 110];
                let d2 = dx * dx + dy * dy;
                let roofed = full.pixels()[at] != bare.pixels()[at];
                let expected = if d2 <= 40 * 40 {
                    inside += usize::from(roofed);
                    bare.pixels()[at]
                } else if d2 > 64 * 64 {
                    full.pixels()[at]
                } else if ring_rule(cutaway, view.grid, [x, y]) {
                    assert!(cutaway.keeps(view.grid, [x, y]));
                    kept += usize::from(roofed);
                    full.pixels()[at]
                } else {
                    assert!(!cutaway.keeps(view.grid, [x, y]));
                    removed += usize::from(roofed);
                    bare.pixels()[at]
                };
                assert_eq!(cut.pixels()[at], expected, "lit {lit} at {x}, {y}");
            }
        }
        // The roofs cover the whole cutaway, so each part is exercised.
        assert!(inside > 3000, "{inside}");
        assert!(kept > 1000 && removed > 1000, "{kept} {removed}");
    }
    // A cutaway with no ring has a hard edge; the ring's outer edge
    // (t = 1) draws every pixel.
    let hard = RoofCutaway { ring: 0, ..cutaway };
    assert!(!hard.keeps(view.grid, [330, 110]));
    assert!(hard.keeps(view.grid, [331, 110]));
    for x in 0..4 {
        assert!(cutaway.keeps(view.grid, [290 + 64 + x, 110]));
        assert!(cutaway.keeps(view.grid, [290, 110 + 64 + x]));
    }
}

#[test]
fn the_cutaway_dither_is_anchored_to_the_world_as_the_camera_pans() {
    let cutaway = RoofCutaway {
        center: [290, 110],
        inner: 40,
        ring: 24,
    };
    let view = roofed_view();
    let before = render(view, false, false, Some(cutaway), None);
    for delta in [[7, -5], [1, 0], [0, 3], [-32, 24]] {
        let mut panned = view;
        panned.pan(delta[0], delta[1]);
        let moved = RoofCutaway {
            center: [290 + i64::from(delta[0]), 110 + i64::from(delta[1])],
            ..cutaway
        };
        let after = render(panned, false, false, Some(moved), None);
        for y in 40..180_i64 {
            for x in 220..360_i64 {
                let shifted = [x + i64::from(delta[0]), y + i64::from(delta[1])];
                assert_eq!(
                    cutaway.keeps(view.grid, [x, y]),
                    moved.keeps(panned.grid, shifted)
                );
                assert_eq!(
                    after.pixels()[(shifted[1] * 640 + shifted[0]) as usize],
                    before.pixels()[(y * 640 + x) as usize],
                    "{delta:?} at {x}, {y}"
                );
            }
        }
    }
    // The pattern is the matrix at world pixels: a camera moved by one
    // pixel moves the pattern with the map.
    let world = |grid: HexGrid, x: i64| bayer([x - grid.origin[0], 7 - grid.origin[1]]);
    let mut panned = view;
    panned.pan(1, 0);
    assert_eq!(world(view.grid, 10), world(panned.grid, 11));
    assert_eq!(bayer([-1, -1]), BAYER[3][3]);
    assert_eq!(bayer([4, 5]), BAYER[1][0]);
}

#[test]
fn picks_reach_objects_under_a_cut_away_roof_and_not_under_an_intact_one() {
    let map = roofed_map();
    let art = rich_art();
    let view = roofed_view();
    let (hex, point) = (19900..20300)
        .find_map(|hex| {
            let [x, y] = view.grid.hex_center(hex).ok()?;
            let point = [x as i32, y as i32 - 5];
            let roofed = (0..4)
                .flat_map(|dy| (0..4).map(move |dx| [point[0] + dx, point[1] + dy]))
                .all(|at| roof_hides(&map, view, &art, at, None));
            roofed.then_some((hex, point))
        })
        .expect("a hex under the roofs");
    let objects = scene(vec![object(&map, 9, 0x0200_0001, hex)]);
    let pick =
        |at: [i32; 2], cutaway| pick_candidates_cut(&map, &objects, view, &art, at, None, cutaway);
    // An intact roof refuses the pick, as in Vanilla.
    assert_eq!(
        pick_candidates(&map, &objects, view, &art, point, None),
        None
    );
    assert_eq!(pick(point, None), None);
    let far = RoofCutaway {
        center: [5000, 5000],
        inner: 40,
        ring: 24,
    };
    assert_eq!(pick(point, Some(far)), None);
    assert!(roof_hides(&map, view, &art, point, Some(far)));
    // Cut away over the pointer, the object is picked.
    let over = RoofCutaway {
        center: point.map(i64::from),
        inner: 30,
        ring: 16,
    };
    assert_eq!(pick(point, Some(over)), Some(vec![(9, true)]));
    assert!(!roof_hides(&map, view, &art, point, Some(over)));
    // In the ring the pick follows the dither at each pixel.
    let ring = RoofCutaway {
        center: [i64::from(point[0]) - 38, i64::from(point[1])],
        inner: 30,
        ring: 16,
    };
    let mut seen = [false; 2];
    for dy in 0..4 {
        for dx in 0..4 {
            let at = [point[0] + dx, point[1] + dy];
            let keeps = ring.keeps(view.grid, at.map(i64::from));
            seen[usize::from(keeps)] = true;
            assert_eq!(pick(at, Some(ring)).is_none(), keeps, "{at:?}");
            assert_eq!(roof_hides(&map, view, &art, at, Some(ring)), keeps);
        }
    }
    assert_eq!(seen, [true, true]);
    // Without roofs nothing hides; off the elevations everything does.
    let open = MapView {
        roofs: false,
        ..view
    };
    assert!(!roof_hides(&map, open, &art, point, None));
    let high = MapView {
        elevation: 3,
        ..view
    };
    assert!(roof_hides(&map, high, &art, point, Some(over)));
}

/// A block of allowed hexes around the view's center hex (20100).
fn region() -> Vec<bool> {
    (0..40000)
        .map(|hex| (95..106).contains(&(hex % 200)) && (96..104).contains(&(hex / 200)))
        .collect()
}

#[test]
fn the_shroud_darkens_outside_the_region_through_a_dithered_band() {
    let allowed = region();
    let field = ShroudField::new(&allowed);
    let view = roofed_view();
    let base = render(view, true, false, None, None);
    let shroud = Shroud {
        allowed: &allowed,
        dark: 3,
    };
    let shrouded = render(view, true, false, None, Some(shroud));
    let [ox, oy] = view.grid.origin;
    let (mut open, mut band_dark, mut band_open, mut far) = (0, 0, 0, 0);
    for y in 0..380_i64 {
        for x in 0..640_i64 {
            let at = (y * 640 + x) as usize;
            let (pixel, under) = (shrouded.pixels()[at], base.pixels()[at]);
            let hex = view.grid.pick_hex([x as i32, y as i32]);
            match hex.and_then(|hex| field.cell(hex)) {
                Some(ShroudCell::Allowed) => {
                    assert!(allowed[hex.unwrap() as usize]);
                    assert_eq!(pixel, under, "allowed at {x}, {y}");
                    open += 1;
                }
                None | Some(ShroudCell::Far) => {
                    assert_eq!(pixel, 3, "far at {x}, {y}");
                    far += 1;
                }
                Some(ShroudCell::Near([cx, cy])) => {
                    let [dx, dy] = [x - ox - i64::from(cx), y - oy - i64::from(cy)];
                    let d = ((dx * dx + dy * dy) as f64).sqrt();
                    if d >= 80.0 {
                        assert_eq!(pixel, 3, "beyond the band at {x}, {y}");
                        far += 1;
                        continue;
                    }
                    // t = (d - 16) / 64: dark iff bayer < floor(16 t).
                    let level = ((d - 16.0) / 64.0 * 16.0).floor();
                    let dark = f64::from(bayer([x - ox, y - oy])) < level;
                    assert_eq!(pixel, if dark { 3 } else { under }, "band at {x}, {y}");
                    if dark {
                        band_dark += 1;
                    } else {
                        band_open += 1;
                    }
                }
            }
        }
    }
    assert!(open > 10000 && far > 10000, "{open} {far}");
    assert!(
        band_dark > 2000 && band_open > 2000,
        "{band_dark} {band_open}"
    );
    // The view's corners are more than two hexes from the region.
    for corner in [0, 639, 380 * 640 - 640, 380 * 640 - 1] {
        assert_eq!(shrouded.pixels()[corner], 3);
    }
}

#[test]
fn the_shroud_field_is_the_nearest_allowed_center_within_reach() {
    // A blob, a block against the grid's corner, and a lone hex.
    let allowed: Vec<bool> = (0..40000_i32)
        .map(|hex| {
            let (column, row) = (hex % 200, hex / 200);
            ((column - 100).pow(2) + (row - 100).pow(2) * 4 < 40)
                || (column < 4 && row < 6)
                || hex == 23055
        })
        .collect();
    let field = ShroudField::new(&allowed);
    let inside: Vec<i32> = (0..40000).filter(|hex| allowed[*hex as usize]).collect();
    let center = |hex: i32| HexGrid::default().hex_center(hex).unwrap();
    let reach = enhanced::FIELD_REACH;
    let sample = (80..125)
        .flat_map(|row| (75..125).map(move |column| column + 200 * row))
        .chain((0..20).flat_map(|row| (0..20).map(move |column| column + 200 * row)));
    for hex in sample {
        let expected = if allowed[hex as usize] {
            ShroudCell::Allowed
        } else {
            let [x, y] = center(hex);
            let best = inside
                .iter()
                .map(|other| {
                    let [ax, ay] = center(*other);
                    ((ax - x).pow(2) + (ay - y).pow(2), *other)
                })
                .min()
                .unwrap();
            if best.0 < reach * reach {
                ShroudCell::Near(center(best.1).map(|value| value as i32))
            } else {
                ShroudCell::Far
            }
        };
        assert_eq!(field.cell(hex), Some(expected), "{hex}");
    }
    assert_eq!(field.cell(40000), None);
    // A short allowed set treats the missing hexes as outside.
    let short = ShroudField::new(&[true; 3]);
    assert_eq!(short.cell(2), Some(ShroudCell::Allowed));
    assert!(matches!(short.cell(3), Some(ShroudCell::Near(_))));
    assert_eq!(short.cell(39999), Some(ShroudCell::Far));
}

#[test]
fn every_pixel_is_within_reach_of_its_hex_center() {
    let reach = enhanced::CELL_REACH;
    for center in [20100, 20101, 15333] {
        let grid = HexGrid::centered(center, 640, 380).unwrap();
        for y in 0..380 {
            for x in 0..640 {
                let hex = grid.pick_hex([x, y]).unwrap();
                let [cx, cy] = grid.hex_center(hex).unwrap();
                let [dx, dy] = [i64::from(x) - cx, i64::from(y) - cy];
                assert!(dx * dx + dy * dy <= reach * reach, "{x}, {y}");
            }
        }
    }
    assert!(enhanced::FULLY_DARK + reach <= enhanced::FIELD_REACH);
}

#[test]
fn the_shroud_spans_darken_exactly_the_pixels_of_the_per_pixel_rule() {
    let allowed = region();
    let mut edge = allowed.clone();
    // A strip along the grid's first rows, so a view shows the grid's
    // edge with allowed hexes beside it.
    edge[..1200].fill(true);
    let views = [
        (HexGrid::centered(20100, 640, 380).unwrap(), [0, 0]),
        (HexGrid::centered(20100, 640, 380).unwrap(), [13, -7]),
        (HexGrid::centered(20100, 333, 211).unwrap(), [-3, 5]),
        (HexGrid::centered(400, 640, 380).unwrap(), [0, 0]),
        (HexGrid::centered(399, 640, 380).unwrap(), [5, 1]),
        (HexGrid::centered(39999, 500, 300).unwrap(), [0, 0]),
    ];
    for flags in [&allowed, &edge] {
        let field = ShroudField::new(flags);
        for (grid, [dx, dy]) in views {
            let grid = HexGrid {
                origin: [grid.origin[0] + dx, grid.origin[1] + dy],
            };
            let mut image = IndexedImage::new(640, 380).unwrap();
            image.pixels_mut().fill(9);
            draw_shroud(&mut image, grid, &field, 3);
            for y in 0..380_i64 {
                for x in 0..640_i64 {
                    let pixel = image.pixels()[(y * 640 + x) as usize];
                    let expected = if field.darkens(grid, [x, y]) { 3 } else { 9 };
                    assert_eq!(pixel, expected, "{grid:?} at {x}, {y}");
                }
            }
        }
    }
    // A grid origin past the coordinate range shows no hex: all dark.
    let mut image = IndexedImage::new(8, 8).unwrap();
    let field = ShroudField::new(&allowed);
    let far = HexGrid {
        origin: [i64::MAX, 0],
    };
    draw_shroud(&mut image, far, &field, 3);
    assert!(image.pixels().iter().all(|pixel| *pixel == 3));
    assert!(field.darkens(far, [0, 0]));
}

#[test]
fn the_overscan_draws_and_picks_a_sprite_reaching_in_from_beyond_the_window() {
    // A 700-pixel-tall scenery sprite on a hex 400 pixels below the view:
    // past the visited window's 240-pixel margin, it reaches up into the
    // view. A Vanilla refresh never visits its hex; an enhanced canvas
    // tile (the overscan) draws and picks it.
    let map = preview();
    let view = view();
    let mut art = rich_art();
    art.0
        .insert((false, 0x0200_0002), pattern(20, 700, |_, _| 222));
    let x = i64::from(view.width / 2);
    let below = i64::from(view.height) + 400;
    let hex = view.grid.pick_hex([x as i32, below as i32]).unwrap();
    let objects = scene(vec![object(&map, 7, 0x0200_0002, hex)]);
    let [cx, cy] = view.grid.hex_center(hex).unwrap();
    assert!(cy - i64::from(view.height) > 240, "{cy}");
    let point = [cx as i32, 100];
    let draw = |overscan: bool| {
        let mut image = render_floor(&map, view, &art).unwrap();
        let scene = MapScene {
            overscan,
            ..MapScene::default()
        };
        draw_map_scene(&mut image, &map, &objects, view, &art, scene).unwrap();
        image.pixels()[point[1] as usize * view.width as usize + point[0] as usize]
    };
    assert_ne!(draw(false), 222);
    assert_eq!(draw(true), 222);
    let vanilla = pick_candidates(&map, &objects, view, &art, point, None).unwrap();
    assert!(vanilla.is_empty(), "{vanilla:?}");
    let tile = pick_candidates_cut(&map, &objects, view, &art, point, None, None).unwrap();
    assert_eq!(tile, vec![(7, true)]);
    // A sprite wholly outside the view is not added.
    let mut far = view;
    far.grid.origin[0] += 2000;
    assert!(overscan_hexes(far, &objects.elevations[0], &art).is_empty());
}

#[test]
fn the_vignette_darkens_toward_its_center_through_an_anchored_dither() {
    // A ramp whose table k paints index k, over a uniform canvas: each
    // pixel shows the table it took.
    let ramp: Vec<[u8; 256]> = (0..6).map(|level| [level as u8; 256]).collect();
    let vignette = Vignette {
        center: [200, 100],
        radii: [200, 100],
        near: 64,
        far: 8,
    };
    let grid = HexGrid { origin: [3, 7] };
    let mut image = IndexedImage::new(400, 200).unwrap();
    draw_vignette(&mut image, grid, vignette, &ramp);
    let at = |x: usize, y: usize| image.pixels()[y * 400 + x];
    // The center takes table 4 (64 sixteenths); the corner, past the
    // radii, 8 sixteenths: table 0, or table 1 where the Bayer threshold
    // is under 8 (half the pixels of every 4x4 cell).
    assert_eq!(at(200, 100), 4);
    let edge: Vec<u8> = (0..4)
        .flat_map(|y| (0..4).map(move |x| (x, y)))
        .map(|(x, y)| at(x, y))
        .collect();
    assert!(edge.iter().all(|level| *level <= 1), "{edge:?}");
    assert_eq!(edge.iter().filter(|level| **level == 1).count(), 8);
    // The mean level falls from the center outward along the middle row.
    let mean = |x0: usize| (x0..x0 + 8).map(|x| u32::from(at(x, 100))).sum::<u32>();
    let samples: Vec<u32> = [192, 150, 100, 50, 0].iter().map(|x| mean(*x)).collect();
    assert!(
        samples.windows(2).all(|pair| pair[0] >= pair[1]),
        "{samples:?}"
    );
    // The dither follows world pixels: the grid and the center moved by a
    // whole 4-pixel cell draw the same pattern, moved with them.
    let mut moved = IndexedImage::new(400, 200).unwrap();
    let shifted = Vignette {
        center: [204, 104],
        ..vignette
    };
    draw_vignette(&mut moved, HexGrid { origin: [7, 11] }, shifted, &ramp);
    for (x, y) in [(10, 10), (57, 33), (200, 100), (300, 150)] {
        assert_eq!(moved.pixels()[(y + 4) * 400 + x + 4], at(x, y), "{x},{y}");
    }
    // An empty ramp leaves the canvas as it is.
    let mut untouched = IndexedImage::new(4, 4).unwrap();
    draw_vignette(&mut untouched, grid, vignette, &[]);
    assert!(untouched.pixels().iter().all(|pixel| *pixel == 0));
}

/// The rows of a SPEC-022 TSV fixture after its header, by column name.
fn spec022_rows(text: &str) -> Vec<std::collections::BTreeMap<&str, &str>> {
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let header: Vec<&str> = lines.next().unwrap().split('\t').collect();
    lines
        .map(|line| header.iter().copied().zip(line.split('\t')).collect())
        .collect()
}

/// SPEC-022 §5.4 the scene's peek: center (88, 40), R0 = 32, Wr = 48.
fn scene_peek() -> RoofPeekParams {
    RoofPeekParams::new([88, 40], 32, 48)
}

#[test]
fn spec022_t8_peek_thresholds_and_scales_give_the_fixture_rows() {
    let peek = scene_peek();
    assert_eq!(peek.outer(), 80);
    let squared = peek.thresholds_squared();
    let rows = spec022_rows(include_str!(
        "../../../../tests/fixtures/spec022/peek-thresholds.tsv"
    ));
    assert_eq!(rows.len(), 16);
    for row in rows {
        let t: usize = row["t"].parse().unwrap();
        assert_eq!(peek.threshold(t).to_string(), row["threshold_q4"], "t {t}");
        assert_eq!(squared[t].to_string(), row["threshold_sq"], "t {t}");
    }
    for row in spec022_rows(include_str!(
        "../../../../tests/fixtures/spec022/peek-scale.tsv"
    )) {
        let scale: f64 = row["canvas_scale"].parse().unwrap();
        let peek = RoofPeekParams::at_scale([0.0, 0.0], scale).unwrap();
        assert_eq!(peek.inner.to_string(), row["inner_q4"], "scale {scale}");
        assert_eq!(peek.ring.to_string(), row["ring_q4"], "scale {scale}");
        assert_eq!(peek.outer().to_string(), row["outer_q4"], "scale {scale}");
        assert_eq!(peek.threshold(0).to_string(), row["threshold_q4_t0"]);
        assert_eq!(peek.threshold(15).to_string(), row["threshold_q4_t15"]);
    }
    // The center of pixel (5, 2) is (88, 40) in 1/16 pixel.
    let centered = RoofPeekParams::at_scale([5.5, 2.5], 1.0).unwrap();
    assert_eq!(centered.center, [88, 40]);
}

#[test]
fn spec022_t8_peek_test_gives_the_fixture_samples() {
    let peek = scene_peek();
    let rows = spec022_rows(include_str!(
        "../../../../tests/fixtures/spec022/peek-samples.tsv"
    ));
    assert_eq!(rows.len(), 14);
    for row in rows {
        let (x, y): (i32, i32) = (row["x"].parse().unwrap(), row["y"].parse().unwrap());
        let label = format!("pixel ({x}, {y})");
        let world = [i64::from(x) + 4, i64::from(y) - 2];
        assert_eq!(bayer(world).to_string(), row["t"], "{label}");
        let (dx, dy) = (16 * x + 8 - 88, 16 * y + 8 - 40);
        assert_eq!(dx.to_string(), row["dx_q4"], "{label}");
        assert_eq!(dy.to_string(), row["dy_q4"], "{label}");
        assert_eq!((dx * dx + dy * dy).to_string(), row["dist_sq"], "{label}");
        let cut = peek.cuts([x, y], [-4, 2]);
        assert_eq!(u8::from(cut).to_string(), row["cut"], "{label}");
    }
}

#[test]
fn spec022_t8_peek_safe_rules() {
    // The outer radius is capped at 16384: R0 first, then Wr.
    let capped = RoofPeekParams::new([0, 0], 20000, 500);
    assert_eq!((capped.inner, capped.ring), (16384, 0));
    let capped = RoofPeekParams::new([0, 0], 16000, 1000);
    assert_eq!((capped.inner, capped.ring), (16000, 384));
    // A tiny scale caps; a scale that is not finite and positive refuses.
    let tiny = RoofPeekParams::at_scale([0.0, 0.0], 1e-9).unwrap();
    assert_eq!(tiny.outer(), 16384);
    for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(RoofPeekParams::at_scale([0.0, 0.0], scale).is_none());
    }
    assert!(RoofPeekParams::at_scale([f64::NAN, 0.0], 1.0).is_none());
    // The center is clamped to ±2^24, and the test stays in range there.
    let far = RoofPeekParams::at_scale([1e12, -1e12], 1.0).unwrap();
    assert_eq!(far.center, [1 << 24, -(1 << 24)]);
    assert!(!far.cuts([16383, 0], [0, 0]));
    // Every threshold is below the outer radius and its square fits.
    let wide = RoofPeekParams::new([0, 0], 0, 16384);
    assert!((0..16).all(|t| wide.threshold(t) < wide.outer()));
    assert!(wide.cuts([0, 0], [i32::MIN, i32::MAX]));
}
