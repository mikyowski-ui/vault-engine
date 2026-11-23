use super::*;
use vault_formats::{FrmArchive, Prototype};
struct Art {
    tile: FrmArchive,
    /// Tile index 1, the ordinary blank tile.
    blank: FrmArchive,
    object: FrmArchive,
}
impl MapArt for Art {
    fn frame(
        &self,
        category: Category,
        id: u32,
        direction: u8,
        frame: usize,
    ) -> Option<(&FrmFrame, [i16; 2])> {
        let archive = match (category, id) {
            (Category::Tile, 1) => &self.blank,
            (Category::Tile, _) => &self.tile,
            _ => &self.object,
        };
        Some((archive.frame(direction as usize, frame)?, [0, 0]))
    }
}
/// A grey palette: index i is grey `i / 4` (six-bit), and five-bit grey
/// g looks up index `8 g`.
pub(super) fn grey_palette() -> Palette {
    let mut bytes = Vec::with_capacity(0x8300);
    for index in 0..256_u32 {
        let grey = (index / 4) as u8;
        bytes.extend([grey; 3]);
    }
    for red in 0..32_u32 {
        bytes.extend(std::iter::repeat_n((red * 8) as u8, 1024));
    }
    Palette::parse(&bytes).unwrap()
}
pub(super) fn preview() -> MapFile {
    MapFile::parse(&fixture::preview_map(), &|pid| {
        let category = (pid >> 24) as u8;
        Ok(Prototype::parse(&fixture::prototype(
            category,
            if category == 0 { 1 } else { 5 },
        ))?
        .info())
    })
    .unwrap()
}
/// Art by identifier: tiles and objects keyed by their art id.
pub(super) struct Keyed(pub(super) std::collections::BTreeMap<(bool, u32), FrmArchive>);
impl MapArt for Keyed {
    fn frame(
        &self,
        category: Category,
        id: u32,
        direction: u8,
        frame: usize,
    ) -> Option<(&FrmFrame, [i16; 2])> {
        let archive = self.0.get(&(category == Category::Tile, id))?;
        Some((archive.frame(direction as usize, frame)?, [0, 0]))
    }
}
/// An object of `art` on `hex`, from the fixture map's first object.
pub(super) fn object(map: &MapFile, id: u32, art: u32, hex: i32) -> MapObject {
    let mut object = map.objects.elevations[0][0].clone();
    object.id = id;
    object.pid = art & 0x0f00_0000 | 1;
    object.art_id = art;
    object.hex = Some(hex);
    object.flags = BasicFlags(0);
    object.displacement = [0; 2];
    object.frame = 0;
    object.orientation = 0;
    object
}
pub(super) fn scene(objects: Vec<MapObject>) -> MapObjects {
    MapObjects {
        total: objects.len(),
        elevations: [objects, Vec::new(), Vec::new()],
        consumed_len: 0,
    }
}
pub(super) fn view() -> MapView {
    MapView {
        grid: HexGrid::centered(20100, 640, 380).unwrap(),
        elevation: 0,
        roofs: false,
        width: 640,
        height: 380,
    }
}

#[test]
fn spec014_cutout_orientation_table_uses_the_exact_constant() {
    let map = preview();
    let mask = FrmArchive::parse(&fixture::frm(129, 98, 64)).unwrap();
    let palette = grey_palette();
    let cut = |extended: &'static dyn Fn(u32) -> u32| Cutout {
        mask: mask.frame(0, 0).unwrap(),
        hex: 20100,
        displacement: [0; 2],
        hidden: false,
        extended,
        palette: &palette,
    };
    let view = view();
    let wall = |hex: i32, end: bool| {
        let mut wall = object(&map, 1, 0x0300_0001, hex);
        if end {
            wall.flags.0 |= TRANSPARENCY_END;
        }
        wall
    };
    // Rows on and back (+-16, +-12) are exact equalities (12 K rounds
    // to 16), so R and R' both hold. Three rows back (-48, -36) is where
    // K decides: 36 K exceeds 48, so R fails there (it would hold with
    // the nearest double to 4/3).
    let plain = cut(&|_| 0);
    assert_eq!(12.0 * K, 16.0);
    const { assert!(36.0 * K > 48.0 && 36.0 * (4.0_f64 / 3.0) == 48.0) };
    assert!(plain.oriented(view, &wall(20300, false), true));
    assert!(plain.oriented(view, &wall(19900, false), true));
    assert!(!plain.oriented(view, &wall(19500, false), true));
    // Two columns on (-48, +12): R, F and F' all hold, so the end marker
    // decides the default row.
    assert!(plain.oriented(view, &wall(20102, false), true));
    assert!(!plain.oriented(view, &wall(20102, true), true));
    assert!(plain.oriented(view, &wall(20102, true), false));
    // The other rows of the table.
    let east = cut(&|_| 0x0800_0000);
    assert!(east.oriented(view, &wall(20102, true), true));
    assert!(!east.oriented(view, &wall(19500, false), true));
    let either = cut(&|_| 0x1000_0000);
    assert!(either.oriented(view, &wall(20300, false), true));
    assert!(!either.oriented(view, &wall(19500, false), true));
    let both = cut(&|_| 0x2000_0000);
    assert!(both.oriented(view, &wall(20300, false), true));
    // Three rows back (-48, -36): F' only.
    assert!(!both.oriented(view, &wall(19500, false), true));
}

#[test]
fn spec014_cutout_blends_walls_and_roofs_through_the_mask() {
    let map = preview();
    let mask = FrmArchive::parse(&fixture::frm(129, 98, 64)).unwrap();
    let palette = grey_palette();
    let none = |_| 0;
    let mut art = std::collections::BTreeMap::new();
    art.insert(
        (true, 1),
        FrmArchive::parse(&fixture::frm(80, 36, 0)).unwrap(),
    );
    art.insert(
        (true, 2),
        FrmArchive::parse(&fixture::frm(80, 36, 200)).unwrap(),
    );
    art.insert(
        (false, 0x0300_0001),
        FrmArchive::parse(&fixture::frm(2, 2, 20)).unwrap(),
    );
    art.insert(
        (false, 0x0200_0001),
        FrmArchive::parse(&fixture::frm(2, 2, 20)).unwrap(),
    );
    let art = Keyed(art);
    let mut view = view();
    let at = |hex: i32| view.grid.hex_center(hex).unwrap();
    let pixel =
        |image: &IndexedImage, [x, y]: [i64; 2]| image.pixels()[y as usize * 640 + x as usize];
    let cutout = Cutout {
        mask: mask.frame(0, 0).unwrap(),
        hex: 20100,
        displacement: [0; 2],
        hidden: false,
        extended: &none,
        palette: &palette,
    };
    // A wall one row in front of the player is cut; scenery three rows
    // behind is not. The floor under the wall is index 0 (black).
    let wall = object(&map, 1, 0x0300_0001, 20300);
    let behind = object(&map, 2, 0x0200_0001, 19500);
    let objects = scene(vec![wall.clone(), behind]);
    let image = render_map_cut(&map, &objects, view, &art, Some(&cutout)).unwrap();
    // 20 is five-bit grey 2: at weight 64 it halves to 1 (index 8), and
    // black adds nothing.
    assert_eq!(pixel(&image, at(20300)), 8);
    assert_eq!(pixel(&image, at(19500)), 20);
    assert_eq!(palette.mask_blend(20, 0, 64), 8);
    assert_eq!(palette.mask_blend(20, 0, 0), 20);
    // Without the cutout, for a hidden player, or with a translucency
    // bit the wall draws normally.
    let image = render_map_cut(&map, &objects, view, &art, None).unwrap();
    assert_eq!(pixel(&image, at(20300)), 20);
    let hidden = Cutout {
        hidden: true,
        ..cutout
    };
    let image = render_map_cut(&map, &objects, view, &art, Some(&hidden)).unwrap();
    assert_eq!(pixel(&image, at(20300)), 20);
    let mut glass = wall.clone();
    glass.flags.0 |= 0x8000;
    let image = render_map_cut(&map, &scene(vec![glass]), view, &art, Some(&cutout)).unwrap();
    assert_eq!(pixel(&image, at(20300)), 20);
    // The mask moves with the player's displacement: shifted well away,
    // the wall is outside it.
    let away = Cutout {
        displacement: [200, 0],
        ..cutout
    };
    let image = render_map_cut(&map, &objects, view, &art, Some(&away)).unwrap();
    assert_eq!(pixel(&image, at(20300)), 20);
    // Every roof square goes through the mask, even for a hidden player:
    // grey 25 at weight 64 over the blank floor gives grey 12.
    view.roofs = true;
    let roof = [256, 84];
    let player = view.grid.pick_hex([256, 124]).unwrap();
    let over = Cutout {
        hex: player,
        hidden: true,
        ..cutout
    };
    let image = render_map_cut(&map, &scene(Vec::new()), view, &art, Some(&over)).unwrap();
    assert_eq!(pixel(&image, roof), 96);
    let image = render_map_cut(&map, &scene(Vec::new()), view, &art, None).unwrap();
    assert_eq!(pixel(&image, roof), 200);
    // Picking: over the roof only inside the mask; there the wall in
    // front is "inside the cutout" and not solid, a translucent object
    // never is, and a plain one is.
    view.roofs = false;
    let point = at(20300).map(|value| value as i32);
    let candidates = pick_candidates(&map, &objects, view, &art, point, Some(&cutout)).unwrap();
    assert_eq!(candidates, [(1, false)]);
    assert_eq!(
        pick_candidates(&map, &objects, view, &art, point, None).unwrap(),
        [(1, true)]
    );
    let mut tinted = object(&map, 3, 0x0200_0001, 20300);
    tinted.flags.0 |= 0x4000;
    assert_eq!(
        pick_candidates(&map, &scene(vec![tinted]), view, &art, point, None).unwrap(),
        [(3, false)]
    );
}

#[test]
fn objects_draw_flat_first_then_in_hex_order() {
    let map = preview();
    let mut art = std::collections::BTreeMap::new();
    art.insert(
        (false, 0x0200_0001),
        FrmArchive::parse(&fixture::frm(200, 40, 30)).unwrap(),
    );
    art.insert(
        (false, 0x0200_0002),
        FrmArchive::parse(&fixture::frm(200, 40, 40)).unwrap(),
    );
    let art = Keyed(art);
    let view = view();
    // Hex 20104 sits 96 pixels left of and 24 below 20100; hex 20300
    // sits 16 right of and 12 below it. By screen height 20104 would
    // draw last; in object order 20300 does.
    let left = object(&map, 1, 0x0200_0001, 20104);
    let right = object(&map, 2, 0x0200_0002, 20300);
    let [x, y] = view.grid.hex_center(20300).unwrap();
    let image = render_map_cut(
        &map,
        &scene(vec![right.clone(), left.clone()]),
        view,
        &art,
        None,
    )
    .unwrap();
    let pixel = image.pixels()[(y - 5) as usize * 640 + (x - 40) as usize];
    assert_eq!(pixel, 40);
    // A flat object draws before every non-flat one.
    let mut flat = right;
    flat.flags.0 |= BasicFlags::FLAT.0;
    let image = render_map_cut(&map, &scene(vec![flat, left]), view, &art, None).unwrap();
    let pixel = image.pixels()[(y - 5) as usize * 640 + (x - 40) as usize];
    assert_eq!(pixel, 30);
}

#[test]
fn spec014_lattice_visits_every_shown_hex_in_ascending_order() {
    // 41 columns by 72 rows, each point a distinct hex.
    for table in lattice() {
        assert_eq!(table.len(), 41 * 72);
        assert!(table.windows(2).all(|pair| pair[0].0 < pair[1].0));
    }
    // Off-grid numbering agrees with picking on the grid.
    let grid = HexGrid::centered(20100, 640, 380).unwrap();
    for point in [[0, 0], [320, 190], [17, 333], [639, 379], [-5, 100]] {
        let picked = grid.pick_hex(point).map(i64::from);
        assert_eq!(Some(hex_under(grid, point.map(i64::from))), picked);
    }
    // Every hex whose center is in the view is visited, for views
    // centered on hexes of both parities.
    for center in [20100, 20101, 15333, 24688] {
        let mut view = view();
        view.grid = HexGrid::centered(center, 640, 380).unwrap();
        let visited: Vec<i32> = visited_hexes(view).collect();
        assert!(visited.windows(2).all(|pair| pair[0] < pair[1]));
        for hex in 0..40000 {
            let [x, y] = view.grid.hex_center(hex).unwrap();
            if (0..640).contains(&x) && (0..380).contains(&y) {
                assert!(visited.binary_search(&hex).is_ok(), "{center}: {hex}");
            }
        }
    }
}

#[test]
fn spec014_fixture_q_draw_order() {
    let map = preview();
    let flat = |id: u32, hex: i32| {
        let mut item = object(&map, id, 0x0000_0001, hex);
        item.flags.0 |= BasicFlags::FLAT.0;
        item
    };
    let [a, b] = [flat(1, 20100), flat(2, 20100)];
    let c = object(&map, 3, 0x0100_0001, 20100);
    let d = object(&map, 4, 0x0200_0001, 20100);
    let e = flat(5, 20101);
    let f = object(&map, 6, 0x0300_0001, 20101);
    let order = |objects: Vec<MapObject>| {
        let (placed, hexless) = draw_sequence(view(), &objects, &[]);
        assert!(hexless.is_empty());
        placed
            .iter()
            .map(|index| objects[*index].id)
            .collect::<Vec<_>>()
    };
    // After load: A, B, E, then C, D, F.
    let loaded = vec![
        a.clone(),
        b.clone(),
        c.clone(),
        d.clone(),
        e.clone(),
        f.clone(),
    ];
    assert_eq!(order(loaded), [1, 2, 5, 3, 4, 6]);
    // C takes a walk step on 20100: it moves to the end of its group.
    let mut stepped = c.clone();
    stepped.displacement = [4, -2];
    let walked = vec![
        a.clone(),
        b.clone(),
        d.clone(),
        e.clone(),
        f.clone(),
        stepped,
    ];
    assert_eq!(order(walked), [1, 2, 5, 4, 3, 6]);
    // C steps onto 20101, after E and F there.
    let mut moved = c;
    moved.hex = Some(20101);
    let crossed = vec![a, b, d, e, f, moved];
    assert_eq!(order(crossed.clone()), [1, 2, 5, 4, 6, 3]);
    // Hidden objects are skipped, and an object without a hex is last.
    let mut hidden = crossed;
    hidden[2].flags.0 |= BasicFlags::HIDDEN.0;
    hidden[0].hex = None;
    let (placed, hexless) = draw_sequence(view(), &hidden, &[]);
    assert_eq!(placed, [1, 3, 4, 5]);
    assert_eq!(hexless, [0]);
}

/// One object of `color` on hex 20100 with `flags`, drawn over a floor
/// of `floor` with `light`; the pixel at the hex center.
fn lit_pixel(color: u8, floor: u8, flags: u32, light: MapLight) -> u8 {
    let map = preview();
    let mut art = std::collections::BTreeMap::new();
    art.insert(
        (false, 0x0200_0001),
        FrmArchive::parse(&fixture::frm(8, 8, color)).unwrap(),
    );
    let art = Keyed(art);
    let view = view();
    let mut item = object(&map, 1, 0x0200_0001, 20100);
    item.flags.0 = flags;
    let mut image = IndexedImage::new(640, 380).unwrap();
    image.pixels_mut().fill(floor);
    let lit = MapScene {
        light: Some(light),
        ..MapScene::default()
    };
    draw_map_scene(&mut image, &map, &scene(vec![item]), view, &art, lit).unwrap();
    let [x, y] = view.grid.hex_center(20100).unwrap();
    image.pixels()[(y - 3) as usize * 640 + x as usize]
}

/// The Wave 3 report: a bright color's gray level reaches 15 and
/// overruns the 8-row blend tables. Refuted: SPEC-014 §3.44 computes
/// the gray level over the five-bit components (six-bit channel / 2),
/// so the brightest entry, six-bit (63, 63, 63), has
/// `trunc(trunc(310 / 10) / 4) = 7` in both gray tables; only six-bit
/// channels would give `trunc(630 / 10 / 4) = 15`.
#[test]
fn spec014_bright_colors_stay_in_the_blend_tables_rows() {
    let palette_of = |channel: &dyn Fn(u32) -> [u8; 3]| {
        let mut bytes = Vec::with_capacity(0x8300);
        for index in 0..256 {
            bytes.extend(channel(index));
        }
        bytes.resize(0x8300, 1);
        Palette::parse(&bytes).unwrap()
    };
    let palettes = [
        grey_palette(),
        palette_of(&|_| [63; 3]),
        palette_of(&|i| [(i % 64) as u8, (i * 7 % 64) as u8, (i * 13 % 64) as u8]),
        // Channels above 63 leave the entry unmapped (gray level 0).
        palette_of(&|i| [i as u8, 255 - i as u8, 63]),
    ];
    for (number, palette) in palettes.iter().enumerate() {
        for glass in [false, true] {
            let levels: Vec<u8> = (0..=255).map(|i| palette.gray_level(i, glass)).collect();
            assert!(levels.iter().all(|level| *level <= 7), "palette {number}");
        }
    }
    assert_eq!(grey_palette().rgb6(255), [63; 3]);
    assert_eq!(grey_palette().gray_level(255, false), 7);
    assert_eq!(grey_palette().gray_level(255, true), 7);
    // The translucent draws of the brightest indexes, over the darkest
    // and the brightest floor, at full light: blend_F[7][D] relit at 128.
    let palette = grey_palette();
    let tiles = vec![65536; 40000];
    let light = MapLight {
        palette: &palette,
        ambient: 65536,
        tiles: &tiles,
    };
    for (bit, key, ..) in TRANSLUCENT {
        let blend = crate::outline::blend_table(&palette, key);
        for color in 252..=255 {
            for floor in [1, 128, 255] {
                let want = palette.intensity(blend[7][usize::from(floor)], 128);
                assert_eq!(
                    lit_pixel(color, floor, bit, light),
                    want,
                    "{bit:#x} {color}"
                );
            }
        }
    }
}

#[test]
fn spec014_lit_and_translucent_draws() {
    // In the grey palette index i has five-bit grey i / 8, and five-bit
    // grey g looks up index 8 g.
    let palette = grey_palette();
    let mut tiles = vec![0; 40000];
    tiles[20100] = 32768;
    let dim = MapLight {
        palette: &palette,
        ambient: 16384,
        tiles: &tiles,
    };
    // Lit draw: the hex light (above the ambient) halves grey 25 to 12;
    // 229 and up are unchanged.
    assert_eq!(lit_pixel(200, 80, 0, dim), 96);
    assert_eq!(lit_pixel(230, 80, 0, dim), 230);
    // Full ambient light leaves grey 25 at index 200.
    let bright = MapLight {
        ambient: 65536,
        ..dim
    };
    assert_eq!(lit_pixel(200, 80, 0, bright), 200);
    // Wall translucency: key 25439 has red 24 (index 192); grey 25 has
    // common gray level 6, so the mix is (6 × 24 + 10) / 7 = 22 over
    // grey 10, at full light whatever the hex light (index 176).
    assert_eq!(lit_pixel(200, 80, 0x10000, dim), 176);
    // Steam (key 32767, grey 31) uses the hex light: (6 × 31 + 10) / 7
    // = 28, halved to 14.
    assert_eq!(lit_pixel(200, 80, 0x40000, dim), 112);
    // Gray level 0 keeps the destination but relights it.
    assert_eq!(lit_pixel(8, 80, 0x40000, dim), 40);
    // Indexes 229 and up get no exemption: 240 (grey 30, gray level 7)
    // draws the foreground itself.
    assert_eq!(lit_pixel(240, 80, 0x40000, bright), 248);
    // Explicitly no translucency, or two bits, is the lit draw.
    assert_eq!(lit_pixel(200, 80, 0x8000, dim), 96);
    assert_eq!(lit_pixel(200, 80, 0x30000, dim), 96);
    // Unlit, everything is drawn as stored.
    let map = preview();
    let mut art = std::collections::BTreeMap::new();
    art.insert(
        (false, 0x0200_0001),
        FrmArchive::parse(&fixture::frm(8, 8, 200)).unwrap(),
    );
    let mut glass = object(&map, 1, 0x0200_0001, 20100);
    glass.flags.0 = 0x20000;
    let image = render_map_with(&map, &scene(vec![glass]), view(), &Keyed(art)).unwrap();
    let [x, y] = view().grid.hex_center(20100).unwrap();
    assert_eq!(image.pixels()[(y - 3) as usize * 640 + x as usize], 200);
}

#[test]
fn spec014_floating_text_draws_over_roofs_at_its_hex_and_m_r2_picks_past_bad_objects() {
    let map = preview();
    let mut art = std::collections::BTreeMap::new();
    art.insert(
        (true, 2),
        FrmArchive::parse(&fixture::frm(80, 36, 200)).unwrap(),
    );
    art.insert(
        (false, 0x0200_0001),
        FrmArchive::parse(&fixture::frm(2, 2, 20)).unwrap(),
    );
    let art = Keyed(art);
    let mut view = view();
    view.roofs = true;
    let roof = [256_i64, 84];
    let [hx, hy] = view.grid.hex_reference(20100).unwrap();
    // A 3-by-3 box with one glyph pixel at (1, 1), placed over the roof.
    let mut image = IndexedImage::new(3, 3).unwrap();
    image.pixels_mut()[4] = 77;
    let text = FloatingSprite {
        hex: 20100,
        offset: [(roof[0] - hx - 1) as i32, (roof[1] - hy - 1) as i32],
        image,
    };
    let draw = |texts: &[FloatingSprite]| {
        let mut image = render_floor(&map, view, &art).unwrap();
        let layers = MapScene {
            floating: texts,
            ..MapScene::default()
        };
        draw_map_scene(&mut image, &map, &scene(Vec::new()), view, &art, layers).unwrap();
        image
    };
    let pixel =
        |image: &IndexedImage, [x, y]: [i64; 2]| image.pixels()[y as usize * 640 + x as usize];
    let bare = draw(&[]);
    assert_eq!(pixel(&bare, roof), 200);
    let image = draw(std::slice::from_ref(&text));
    assert_eq!(pixel(&image, roof), 77, "not hidden by the roof");
    for x in [roof[0] - 1, roof[0] + 1] {
        let beside = [x, roof[1]];
        assert_eq!(
            pixel(&image, beside),
            pixel(&bare, beside),
            "index 0 skipped"
        );
    }
    // M-R2: an object whose PID has no category is skipped by picking
    // as by drawing; it does not hide the objects under the pointer.
    view.roofs = false;
    let good = object(&map, 1, 0x0200_0001, 20300);
    let mut bad = object(&map, 2, 0x0200_0001, 20500);
    bad.pid = 0x0f00_0001;
    let objects = scene(vec![bad, good]);
    let point = view
        .grid
        .hex_center(20300)
        .unwrap()
        .map(|value| value as i32);
    let hits = pick_candidates(&map, &objects, view, &art, point, None).unwrap();
    assert_eq!(hits, [(1, true)]);
}

#[test]
fn spec014_outlines_follow_the_roofs_at_most_100() {
    let map = preview();
    let mut art = std::collections::BTreeMap::new();
    art.insert(
        (false, 0x0200_0001),
        FrmArchive::parse(&fixture::frm(2, 2, 20)).unwrap(),
    );
    let art = Keyed(art);
    let view = view();
    // 101 objects two hexes apart, all outlined in the other key (type
    // 3), and a disabled hostile one that is never collected.
    let hexes: Vec<i32> = (95..106)
        .flat_map(|row| (90..110).step_by(2).map(move |column| row * 200 + column))
        .take(101)
        .collect();
    let mut objects: Vec<_> = hexes
        .iter()
        .enumerate()
        .map(|(id, hex)| {
            let mut object = object(&map, id as u32 + 1, 0x0200_0001, *hex);
            object.outline = 3;
            object
        })
        .collect();
    let mut disabled = object(&map, 0, 0x0200_0001, 20111);
    disabled.outline = 0x8000_0001;
    objects.insert(0, disabled);
    let palette = grey_palette();
    let inks = OutlineInks::new(&palette);
    let other = crate::outline::key_index(&palette, crate::outline::OTHER_KEY);
    assert_ne!(other, 0);
    let mut image = render_floor(&map, view, &art).unwrap();
    let mut sorted = objects.clone();
    sorted.sort_by_key(|object| object.hex);
    draw_map_scene(
        &mut image,
        &map,
        &scene(objects),
        view,
        &art,
        MapScene {
            outlines: Some(&inks),
            ..MapScene::default()
        },
    )
    .unwrap();
    // The ring left of each sprite: the 100 first in drawing order are
    // outlined, the last is not; the disabled one never is.
    let ring = |object: &MapObject| {
        let (_, origin) = object_sprite(view, &art, object).unwrap();
        image.pixels()[origin[1] as usize * 640 + origin[0] as usize - 1]
    };
    let (disabled, sorted): (Vec<_>, Vec<_>) =
        sorted.into_iter().partition(|object| object.id == 0);
    assert_ne!(ring(&disabled[0]), other);
    assert!(sorted[..100].iter().all(|object| ring(object) == other));
    assert_ne!(ring(&sorted[100]), other);
}

#[test]
fn spec014_r1_rev13_the_cursor_is_the_top_layer_with_its_outline_and_text() {
    let map = preview();
    let mut art = std::collections::BTreeMap::new();
    art.insert(
        (true, 2),
        FrmArchive::parse(&fixture::frm(80, 36, 200)).unwrap(),
    );
    // Flat items in 20, upright scenery in 30.
    art.insert(
        (false, 0x0000_0001),
        FrmArchive::parse(&fixture::frm(2, 2, 20)).unwrap(),
    );
    art.insert(
        (false, 0x0200_0001),
        FrmArchive::parse(&fixture::frm(2, 2, 30)).unwrap(),
    );
    let art = Keyed(art);
    let mut view = view();
    let flat = |id: u32, hex: i32| {
        let mut item = object(&map, id, 0x0000_0001, hex);
        item.flags.0 |= BasicFlags::FLAT.0;
        item
    };
    let upright = |id: u32, hex: i32| object(&map, id, 0x0200_0001, hex);
    // The cursor sits on 20100; its art covers the whole view in 77 and
    // the AP text, when present, in 88.
    let layer = |color: u8| IndexedImage::from_pixels(640, 380, vec![color; 640 * 380]).unwrap();
    let (cursor_art, text_art) = (layer(77), layer(88));
    let ring = FrmArchive::parse(&fixture::frm(4, 4, 99)).unwrap();
    let ring = ring.frame(0, 0).unwrap();
    let palette = grey_palette();
    let inks = OutlineInks::new(&palette);
    let other = crate::outline::key_index(&palette, crate::outline::OTHER_KEY);
    let draw = |view: MapView, objects: Vec<MapObject>, cursor: SceneCursor| {
        let mut image = render_floor(&map, view, &art).unwrap();
        let layers = MapScene {
            outlines: Some(&inks),
            cursor: Some(cursor),
            ..MapScene::default()
        };
        draw_map_scene(&mut image, &map, &scene(objects), view, &art, layers).unwrap();
        image
    };
    let at = |image: &IndexedImage, hex: i32| {
        let [x, y] = view.grid.hex_center(hex).unwrap();
        image.pixels()[y as usize * 640 + x as usize]
    };
    let objects = vec![
        flat(1, 20098),
        flat(2, 20102),
        upright(3, 20096),
        upright(4, 20104),
    ];
    let cursor = SceneCursor {
        hex: 20100,
        art: (&cursor_art, [0, 0]),
        outline: None,
        text: None,
    };
    let image = draw(view, objects.clone(), cursor);
    // Over every object, flat or upright, whatever its hex.
    for hex in [20098, 20102, 20096, 20104] {
        assert_eq!(at(&image, hex), 77, "over the object on {hex}");
    }
    // The AP text is last of all.
    let with_text = SceneCursor {
        text: Some((&text_art, [0, 0])),
        ..cursor
    };
    let image = draw(view, objects.clone(), with_text);
    for hex in [20098, 20102, 20096, 20104] {
        assert_eq!(at(&image, hex), 88, "the text on {hex}");
    }
    // On its own hex it takes no place in the list.
    let own = vec![flat(5, 20100), upright(6, 20100)];
    assert_eq!(at(&draw(view, own, cursor), 20100), 77);
    // Over a roof, with its outline right after its art.
    view.roofs = true;
    let roof = [256_i64, 84];
    let small = IndexedImage::from_pixels(4, 4, vec![77; 16]).unwrap();
    let origin = [roof[0] + 1, roof[1]];
    let outlined = SceneCursor {
        art: (&small, origin),
        outline: Some((ring, origin, 3)),
        ..cursor
    };
    let image = draw(view, Vec::new(), outlined);
    let pixel = |[x, y]: [i64; 2]| image.pixels()[y as usize * 640 + x as usize];
    assert_eq!(pixel(origin), 77, "the cursor covers the roof");
    assert_eq!(pixel(roof), other, "the ring is drawn over the roof");
    // Without the outline the roof shows there.
    let plain = SceneCursor {
        outline: None,
        ..outlined
    };
    let image = draw(view, Vec::new(), plain);
    assert_eq!(
        image.pixels()[roof[1] as usize * 640 + roof[0] as usize],
        200
    );
    // The art, its outline and the text draw in that order.
    let wide = SceneCursor {
        art: (&cursor_art, [0, 0]),
        outline: Some((ring, origin, 3)),
        ..cursor
    };
    let image = draw(view, Vec::new(), wide);
    let pixel = |[x, y]: [i64; 2]| image.pixels()[y as usize * 640 + x as usize];
    assert_eq!(pixel(roof), other, "the ring over the art");
    let image = draw(
        view,
        Vec::new(),
        SceneCursor {
            text: Some((&text_art, [0, 0])),
            ..wide
        },
    );
    let pixel = |[x, y]: [i64; 2]| image.pixels()[y as usize * 640 + x as usize];
    assert_eq!(pixel(roof), 88, "the text over the ring");
    // D-44: the outline takes no place among the 100, so with the list
    // full it is still drawn, and it costs no object its own outline.
    view.roofs = false;
    let shown = (0..40000).step_by(2).filter(|hex| {
        let [x, y] = view.grid.hex_center(*hex).unwrap_or([-1, -1]);
        (60..580).contains(&x) && (60..300).contains(&y)
    });
    let full: Vec<MapObject> = shown
        .take(100)
        .enumerate()
        .map(|(index, hex)| {
            let mut object = flat(10 + index as u32, hex);
            object.outline = 3;
            object
        })
        .collect();
    assert_eq!(full.len(), 100);
    let last = full[99].hex.unwrap();
    let [x, y] = view.grid.hex_center(last).unwrap();
    let ringed = |image: &IndexedImage| {
        let row = |y: i64| &image.pixels()[y as usize * 640..(y as usize + 1) * 640];
        (y - 4..y + 4).any(|y| row(y)[(x - 6) as usize..(x + 6) as usize].contains(&other))
    };
    let plain = SceneCursor {
        art: (&small, [600, 370]),
        ..cursor
    };
    assert!(ringed(&draw(view, full.clone(), plain)));
    let spare = [300_i64, 300];
    let with_outline = SceneCursor {
        outline: Some((ring, spare, 3)),
        ..plain
    };
    let image = draw(view, full, with_outline);
    assert!(ringed(&image), "the hundredth object keeps its outline");
    assert_eq!(
        image.pixels()[(spare[1] as usize - 1) * 640 + spare[0] as usize],
        other,
        "the cursor's outline is drawn"
    );
}

#[test]
fn spec014_objects_take_their_hex_light_and_roofs_the_ambient() {
    let map = preview();
    let palette = grey_palette();
    let mut art = std::collections::BTreeMap::new();
    art.insert(
        (true, 2),
        FrmArchive::parse(&fixture::frm(80, 36, 200)).unwrap(),
    );
    art.insert(
        (false, 0x0200_0001),
        FrmArchive::parse(&fixture::frm(2, 2, 200)).unwrap(),
    );
    let art = Keyed(art);
    let mut view = view();
    view.roofs = true;
    let mut tiles = vec![0; 40000];
    tiles[20300] = 65536;
    let light = MapLight {
        palette: &palette,
        ambient: 32768,
        tiles: &tiles,
    };
    let lit = object(&map, 1, 0x0200_0001, 20300);
    let dark = object(&map, 2, 0x0200_0001, 19500);
    let mut free = object(&map, 3, 0x0200_0001, 0);
    free.hex = None;
    free.screen = [600, 10];
    let mut image = render_floor(&map, view, &art).unwrap();
    let scene_light = MapScene {
        light: Some(light),
        ..MapScene::default()
    };
    draw_map_scene(
        &mut image,
        &map,
        &scene(vec![lit, dark, free]),
        view,
        &art,
        scene_light,
    )
    .unwrap();
    let pixel = |[x, y]: [i64; 2]| image.pixels()[y as usize * 640 + x as usize];
    let at = |hex: i32| {
        let [x, y] = view.grid.hex_center(hex).unwrap();
        [x, y - 1]
    };
    assert_eq!(pixel(at(20300)), 200);
    assert_eq!(pixel(at(19500)), 96);
    // Without a hex: full light.
    assert_eq!(pixel([600, 10]), 200);
    // The roof square of the fixture map at the ambient light.
    assert_eq!(pixel([256, 84]), 96);
}

#[test]
fn spec014_cutout_relights_inside_the_mask() {
    let map = preview();
    let mask = FrmArchive::parse(&fixture::frm(129, 98, 64)).unwrap();
    let palette = grey_palette();
    let none = |_| 0;
    let mut art = std::collections::BTreeMap::new();
    art.insert(
        (false, 0x0300_0001),
        FrmArchive::parse(&fixture::frm(2, 2, 230)).unwrap(),
    );
    let art = Keyed(art);
    let view = view();
    let cutout = Cutout {
        mask: mask.frame(0, 0).unwrap(),
        hex: 20100,
        displacement: [0; 2],
        hidden: false,
        extended: &none,
        palette: &palette,
    };
    let tiles = vec![0; 40000];
    let light = MapLight {
        palette: &palette,
        ambient: 32768,
        tiles: &tiles,
    };
    let draw = |cut: Option<&Cutout>| {
        let mut image = IndexedImage::new(640, 380).unwrap();
        let wall = object(&map, 1, 0x0300_0001, 20300);
        let scene_cut = MapScene {
            cutout: cut,
            light: Some(light),
            outlines: None,
            floating: &[],
            cursor: None,
            roof_cutaway: None,
            shroud: None,
            overscan: false,
            highlight: None,
        };
        draw_map_scene(&mut image, &map, &scene(vec![wall]), view, &art, scene_cut).unwrap();
        let [x, y] = view.grid.hex_center(20300).unwrap();
        image.pixels()[y as usize * 640 + x as usize]
    };
    // Outside a cutout 230 is exempt from lighting; through the mask it
    // is relit (grey 28 to 14) and then blended at weight 64 over black
    // (grey 7, index 56).
    assert_eq!(draw(None), 230);
    assert_eq!(draw(Some(&cutout)), palette.mask_blend(112, 0, 64));
    assert_eq!(draw(Some(&cutout)), 56);
}

#[test]
#[ignore = "needs the local corpus"]
fn spec014_fixture_q_corpus_translucency() {
    let dir = std::env::var_os("VAULT_CORPUS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../local-corpus")
        })
        .join("fallout1-unpacked/color.pal");
    let Ok(bytes) = std::fs::read(&dir) else {
        eprintln!("skipping: no local corpus at {}", dir.display());
        return;
    };
    let palette = Palette::parse(&bytes).unwrap();
    // SPEC-001 §3.7: entries 0 and 229-255 unmapped; additive mixes.
    assert!((0..=255).all(|index| palette.is_mapped(index) == (1..229).contains(&index)));
    assert_eq!(palette.additive(215, 215), 196);
    assert_eq!(palette.additive(220, 220), 220);
    // SPEC-014 Fixture I seen cells: intensity column 64.
    assert_eq!(
        [50, 150, 100].map(|index| palette.intensity(index, 64)),
        [52, 153, 228]
    );
    // The five foreground colors.
    let foregrounds = TRANSLUCENT.map(|(_, key, ..)| {
        let key = u32::from(key);
        palette.lookup_rgb5([
            (key >> 10) as u8 & 31,
            (key >> 5) as u8 & 31,
            key as u8 & 31,
        ])
    });
    assert_eq!(foregrounds, [33, 35, 220, 58, 133].map(Some));
    // Source 215 over destination 100: 6 as wall translucency, 214 as
    // glass at full light.
    let tiles = vec![65536; 40000];
    let light = MapLight {
        palette: &palette,
        ambient: 65536,
        tiles: &tiles,
    };
    assert_eq!(lit_pixel(215, 100, 0x10000, light), 6);
    assert_eq!(lit_pixel(215, 100, 0x20000, light), 214);
    // SPEC-014 §3.44 the outline keys: red 133 (gray level 2), grey 9,
    // item 58 and other 49.
    use crate::outline::{GREY_KEY, ITEM_KEY, OTHER_KEY, RED_KEY, key_index};
    assert_eq!(
        [RED_KEY, GREY_KEY, ITEM_KEY, OTHER_KEY].map(|key| key_index(&palette, key)),
        [133, 9, 58, 49]
    );
    assert_eq!(palette.gray_level(133, false), 2);
}

#[test]
fn map_composition_uses_exact_floor_roof_and_object_coordinates() {
    let map = MapFile::parse(&fixture::preview_map(), &|pid| {
        let category = (pid >> 24) as u8;
        Ok(Prototype::parse(&fixture::prototype(
            category,
            if category == 0 { 1 } else { 5 },
        ))?
        .info())
    })
    .unwrap();
    // As in shipped data, the blank tile's pixels are all transparent index 0.
    let art = Art {
        tile: FrmArchive::parse(&fixture::frm(80, 36, 7)).unwrap(),
        blank: FrmArchive::parse(&fixture::frm(80, 36, 0)).unwrap(),
        object: FrmArchive::parse(&fixture::frm(2, 2, 20)).unwrap(),
    };
    let mut view = MapView::new(&map, 640, 380).unwrap();
    let image = render_map(&map, view, &art).unwrap();
    let pixel = |image: &IndexedImage, x: usize, y: usize| image.pixels()[y * 640 + x];
    assert_eq!(pixel(&image, 256, 180), 7);
    assert_eq!(pixel(&image, 256, 84), 7);
    assert_eq!(pixel(&image, 319, 189), 20); // Hex 20100 bottom-center, no frame increment.
    view.roofs = false;
    assert_eq!(pixel(&render_map(&map, view, &art).unwrap(), 256, 84), 0);
    view.pan(32, -12);
    assert_eq!(view.hover([352, 178]).hex, Some(20100));
    assert_eq!(pixel(&render_map(&map, view, &art).unwrap(), 351, 177), 20);
    view.elevation = 1;
    let image = render_map(&map, view, &art).unwrap();
    assert!(!image.pixels().contains(&20)); // No elevation-0 objects leak into other layers.
    // Floor index 1 is drawn like any other floor art (only roof index 1 is
    // exempt), so an opaque blank-tile image is visible under the view.
    let opaque = Art {
        blank: FrmArchive::parse(&fixture::frm(80, 36, 9)).unwrap(),
        ..art
    };
    let mut view = MapView::new(&map, 640, 380).unwrap();
    view.roofs = false;
    assert_eq!(pixel(&render_map(&map, view, &opaque).unwrap(), 256, 84), 9);
}

/// SPEC-014 Fixture T floor light: an even base hex, every vertex
/// light 65536 except V3 = 0.
#[test]
fn spec014_fixture_t_floor_light_map() {
    let mut lights = [65536; 10];
    lights[3] = 0;
    let mut map = [0; FLOOR_MAP];
    let mut written = [false; FLOOR_MAP];
    fill_floor_map(&lights, &mut map, &mut written);
    let entry = |[px, py]: [usize; 2]| 160 + 80 * py + px;
    for (pixel, expected_entry, value, column) in [
        ([32, 10], 992, 0, 0),
        ([40, 10], 1000, 16384, 32),
        ([48, 10], 1008, 32768, 64),
        ([16, 4], 496, 49152, 96),
        ([32, 0], 192, 55454, 108),
        ([20, 20], 1780, 50410, 98),
        ([0, 10], 960, 65536, 128),
        ([64, 10], 1024, 65536, 128),
        ([48, 22], 1968, 65536, 128),
    ] {
        assert_eq!(entry(pixel), expected_entry, "{pixel:?}");
        assert_eq!(map[expected_entry], value, "{pixel:?}");
        assert_eq!(map[expected_entry] / 512, column, "{pixel:?}");
    }
    // (33, 35) reads entry 2993, which no triangle writes: light 0 in
    // R1. Under the safe rule it reads the nearest written entry above
    // in its column.
    assert_eq!(entry([33, 35]), 2993);
    assert!(!written[2993]);
    assert_eq!(map[2993], 0);
    let source = usize::from(floor_sources()[2993]);
    assert_eq!(source % 80, 33);
    assert!(source < 2993 && written[source]);
    assert!((source + 80..2993).step_by(80).all(|at| !written[at]));
    // Written entries read themselves.
    assert_eq!(floor_sources()[1000], 1000);
}

/// The vertex lights: the larger of the ambient and the hex light
/// (capped at 65536), with the even or odd offsets, off the grid 0.
#[test]
fn spec014_floor_vertices_take_the_base_hex_parity() {
    let palette = grey_palette();
    let mut tiles = vec![0_i64; 40000];
    for (hex, light) in tiles.iter_mut().enumerate() {
        *light = hex as i64 % 5000 * 10;
    }
    tiles[20100 + 599] = 100_000;
    let light = MapLight {
        palette: &palette,
        ambient: 500,
        tiles: &tiles,
    };
    let floor = FloorLight::new(light);
    let even = floor.vertices(20100);
    assert_eq!(even[0], 99 * 10);
    assert_eq!(even[3], 299 * 10);
    assert_eq!(even[9], 65536);
    let odd = floor.vertices(20101);
    assert_eq!(odd[0], (101 - 201 + 5000) * 10);
    assert_eq!(odd[3], 100 * 10);
    assert_eq!(odd[9], (101 + 399) * 10);
    // Off the grid: hex light 0, so the ambient.
    let edge = floor.vertices(1);
    assert_eq!(edge[0], 500);
}

/// A tile with all ten lights equal uses the lit draw (229 and up
/// unlit); an interpolated tile leaves 229-255 unlit too (the safe
/// rule) and lights the rest per pixel.
#[test]
fn spec014_floor_tiles_light_uniform_and_interpolated() {
    let map = preview();
    let palette = grey_palette();
    let mut art = std::collections::BTreeMap::new();
    art.insert(
        (true, 1),
        FrmArchive::parse(&fixture::frm(80, 36, 200)).unwrap(),
    );
    art.insert(
        (true, 2),
        FrmArchive::parse(&fixture::frm(80, 36, 240)).unwrap(),
    );
    let art = Keyed(art);
    let view = view();
    let tiles = vec![0_i64; 40000];
    let draw = |ambient: i32, tiles: &[i64]| {
        render_floor_lit(
            &map,
            view,
            &art,
            MapLight {
                palette: &palette,
                ambient,
                tiles,
            },
        )
        .unwrap()
    };
    let image = draw(40000, &tiles);
    // Fixture T: the lit draw at column trunc(40000 / 512) = 78.
    let lit = palette.intensity(200, 78);
    assert_ne!(lit, 200);
    assert!(image.pixels().contains(&lit));
    assert!(!image.pixels().contains(&200));
    // Square 5050 (tile 2, index 240) stays unlit.
    assert!(image.pixels().contains(&240));
    // A bright hex makes nearby tiles interpolated: some pixels of index
    // 200 get a level between ambient and full light.
    let mut bright = tiles.clone();
    bright[20100] = 65536;
    let image = draw(0, &bright);
    let levels: std::collections::BTreeSet<u8> = image
        .pixels()
        .iter()
        .copied()
        .filter(|pixel| *pixel != 0 && *pixel != 240)
        .collect();
    assert!(levels.len() > 2, "{levels:?}");
    assert!(image.pixels().contains(&240));
}
