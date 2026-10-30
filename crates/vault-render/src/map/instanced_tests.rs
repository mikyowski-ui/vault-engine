//! SPEC-022 §4 tests T7-T19 over the map view: the engine's light buffer,
//! the roof peek against the reference X-ray, order parity with the
//! reference path's draw log, canvas parity (CPU and GPU), the fallback,
//! no state change, the square order, the gray bound, the draws revision 3
//! lifted from the refusals, and the refusals that stay.
use super::tests::{Keyed, object, preview, scene};
use super::*;
use crate::{
    floating::FloatingSprite,
    layers::{
        BatchPlan, DrawMode, EXEMPT, FrameCounts, HexLightBuffer, InstanceError, LightSource,
        LookupTables, PEEK, RenderPath,
        gpu::{self, GpuExecutor},
    },
    outline::OutlineInks,
};
use std::{collections::BTreeMap, sync::Arc};
use vault_formats::{
    FrmArchive,
    map::{TilePair, outline},
};

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

/// A palette whose six-bit channels stay below 32, with a scrambled
/// RGB555 lookup. (Every palette's gray levels are 0-7, the blend tables'
/// rows: they are taken over the five-bit components.)
fn palette() -> Palette {
    let mut bytes = Vec::with_capacity(0x8300);
    for index in 0..256_u32 {
        bytes.extend(
            [
                (index * 7) % 32,
                (index * 13 + 5) % 32,
                (index * 29 + 11) % 32,
            ]
            .map(|c| c as u8),
        );
    }
    for packed in 0..32768_u32 {
        let (r, g, b) = (packed >> 10, (packed >> 5) & 31, packed & 31);
        bytes.push(((r * 3 + g * 5 + b * 7 + 1) % 255 + 1) as u8);
    }
    Palette::parse(&bytes).unwrap()
}

/// The map: patterned floors (with a hidden one), and a block of roofs over
/// the view's center whose top-left group is hidden.
fn map() -> MapFile {
    let mut map = preview();
    let tiles = (0..10000)
        .map(|square: u32| {
            let (column, row) = (square % 100, square / 100);
            let roofed = (46..56).contains(&column) && (46..56).contains(&row);
            let roof: u32 = if roofed { 3 } else { 1 };
            let hidden_roof = roofed && column < 49 && row < 49;
            let floor: u32 = if square.is_multiple_of(37) { 5 } else { 4 };
            let mut word = (roof << 16) | floor;
            if hidden_roof {
                word |= 0x1000_0000;
            }
            if square % 101 == 7 {
                word |= 0x1000;
            }
            TilePair { stored: word, word }
        })
        .collect();
    map.tiles[0] = Some(tiles);
    map
}

/// Object art ids.
const ITEM: u32 = 0x0000_0001;
const CRITTER: u32 = 0x0100_0001;
const SCENERY: u32 = 0x0200_0001;
const WALL: u32 = 0x0300_0001;

/// Patterned tiles and objects: holes (index 0) and indexes 229 and up.
fn art() -> Keyed {
    let mut art = BTreeMap::new();
    art.insert(
        (true, 1),
        FrmArchive::parse(&fixture::frm(80, 36, 0)).unwrap(),
    );
    art.insert(
        (true, 4),
        pattern(80, 36, |x, y| match (x * 7 + y * 3) % 61 {
            0 => 0,
            1 => 233,
            value => 20 + value as u8,
        }),
    );
    art.insert(
        (true, 5),
        pattern(80, 36, |x, y| 150 + ((x + y) % 90) as u8),
    );
    art.insert(
        (true, 3),
        pattern(80, 36, |x, y| {
            if (x + y) % 17 == 0 {
                0
            } else if (x * y) % 23 == 1 {
                240
            } else {
                100 + ((x + 2 * y) % 50) as u8
            }
        }),
    );
    for (id, base, size) in [
        (ITEM, 140, [16, 12]),
        (CRITTER, 160, [20, 40]),
        (SCENERY, 180, [24, 30]),
        (WALL, 30, [32, 60]),
    ] {
        art.insert(
            (false, id),
            pattern(size[0], size[1], move |x, y| match (x * 5 + y * 3) % 23 {
                0 => 0,
                1 => 230 + (x % 20) as u8,
                value => base + value as u8,
            }),
        );
    }
    Keyed(art)
}

/// The scene's objects: the player (a critter on 20100) with scenery on
/// its hex, walls the cutout cuts, two objects on one hex, a flat item,
/// one object per translucency bit (and "explicitly none", and two bits),
/// outlined critters of every ink, a hidden object, and two objects without
/// a hex.
fn objects(map: &MapFile) -> MapObjects {
    let mut list = vec![
        object(map, 1, SCENERY, 20100),
        object(map, 2, CRITTER, 20100),
        object(map, 3, WALL, 20300),
        object(map, 4, WALL, 20302),
        object(map, 5, WALL, 20500),
        object(map, 6, ITEM, 19902),
        object(map, 7, CRITTER, 19902),
    ];
    let mut flat = object(map, 8, ITEM, 20102);
    flat.flags.0 |= BasicFlags::FLAT.0;
    list.push(flat);
    for (id, (bits, hex)) in [
        (0x4000, 20704),
        (0x10000, 20706),
        (0x20000, 20708),
        (0x40000, 20710),
        (0x80000, 20712),
        (0x8000, 20714),
        (0x4000 | 0x20000, 20716),
    ]
    .into_iter()
    .enumerate()
    {
        let mut translucent = object(map, 20 + id as u32, SCENERY, hex);
        translucent.flags.0 |= bits;
        list.push(translucent);
    }
    for (id, (word, hex)) in [
        (outline::RED, 19500),
        (outline::RED | outline::TRANSLUCENT, 19504),
        (outline::GREY | outline::TRANSLUCENT, 19508),
        (outline::ITEM | outline::TRANSLUCENT, 19512),
        (outline::HOSTILE, 19700),
        (outline::FRIENDLY, 19704),
        (3, 19708),
    ]
    .into_iter()
    .enumerate()
    {
        let mut critter = object(map, 40 + id as u32, CRITTER, hex);
        critter.outline = word;
        list.push(critter);
    }
    let mut hidden = object(map, 60, CRITTER, 20104);
    hidden.flags.0 |= BasicFlags::HIDDEN.0;
    list.push(hidden);
    for (id, (screen, bits)) in [([600, 10], 0), ([300, 200], 0x40000)]
        .into_iter()
        .enumerate()
    {
        let mut free = object(map, 70 + id as u32, SCENERY, 0);
        free.hex = None;
        free.screen = screen;
        free.flags.0 |= bits;
        list.push(free);
    }
    scene(list)
}

/// The hex light: 655, plus lights on the player and on 19700 (SPEC-006
/// §3.6's falloff without blockers).
fn tiles() -> Vec<i64> {
    let mut tiles = vec![655_i64; 40000];
    for (center, radius, intensity) in [(20100, 4, 65536), (19700, 3, 40000), (20708, 2, 30000)] {
        let step = (intensity - 655) / (radius + 1);
        for (hex, tile) in tiles.iter_mut().enumerate() {
            let distance = HexGrid::lattice_distance(center, hex as i32).unwrap();
            if distance <= radius {
                *tile += i64::from(intensity - distance * step);
            }
        }
    }
    tiles
}

/// Everything a scene borrows.
struct Fixture {
    map: MapFile,
    objects: MapObjects,
    art: Keyed,
    palette: Palette,
    tiles: Vec<i64>,
    ambient: i32,
    mask: FrmArchive,
    inks: OutlineInks,
    floating: Vec<FloatingSprite>,
    cursor_art: IndexedImage,
    cursor_text: IndexedImage,
    ring: FrmArchive,
}

impl Fixture {
    fn new() -> Self {
        let map = map();
        let objects = objects(&map);
        let palette = palette();
        let text = |color: u8| {
            let mut text = IndexedImage::new(9, 5).unwrap();
            for (index, pixel) in text.pixels_mut().iter_mut().enumerate() {
                if index % 3 != 0 {
                    *pixel = color + (index % 5) as u8;
                }
            }
            text
        };
        Self {
            inks: OutlineInks::new(&palette),
            map,
            objects,
            art: art(),
            palette,
            tiles: tiles(),
            ambient: 16384,
            // Weights 0 to 139: 0 draws the source, above 128 is 128.
            mask: pattern(129, 98, |x, y| ((x + 2 * y) % 140) as u8),
            floating: vec![
                FloatingSprite {
                    hex: 20300,
                    offset: [0, -30],
                    image: text(60),
                },
                FloatingSprite {
                    hex: 19708,
                    offset: [-4, -50],
                    image: text(90),
                },
                FloatingSprite {
                    hex: -5,
                    offset: [0, 0],
                    image: text(10),
                },
            ],
            cursor_art: IndexedImage::from_pixels(6, 4, (0..24).map(|i| 200 + i).collect())
                .unwrap(),
            cursor_text: text(120),
            ring: FrmArchive::parse(&fixture::frm(32, 16, 99)).unwrap(),
        }
    }
    fn light(&self) -> MapLight<'_> {
        MapLight {
            palette: &self.palette,
            ambient: self.ambient,
            tiles: &self.tiles,
        }
    }
    /// The cutout on the player's hex, with the palette.
    fn cutout(&self) -> Cutout<'_> {
        Cutout {
            mask: self.mask.frame(0, 0).unwrap(),
            hex: 20100,
            displacement: [3, -2],
            hidden: false,
            extended: &|_| 0,
            palette: &self.palette,
        }
    }
}

/// What a scenario's scene includes.
#[derive(Clone, Copy)]
struct Options {
    lit: bool,
    cutout: bool,
    outlines: bool,
    cursor: bool,
    overscan: bool,
}

impl Options {
    const FULL: Self = Self {
        lit: true,
        cutout: true,
        outlines: true,
        cursor: true,
        overscan: false,
    };
}

/// The scenario's scene in `view`.
fn scene_of<'a>(
    fixture: &'a Fixture,
    cutout: &'a Cutout<'a>,
    view: MapView,
    options: Options,
) -> MapScene<'a> {
    let [cx, cy] = view.grid.hex_center(20104).unwrap();
    let ring = fixture.ring.frame(0, 0).unwrap();
    let cursor = SceneCursor {
        hex: 20104,
        order: 3,
        art: (&fixture.cursor_art, [cx - 3, cy - 2]),
        outline: Some((ring, [cx - 16, cy - 8], outline::RED | outline::TRANSLUCENT)),
        text: Some((&fixture.cursor_text, [cx + 10, cy - 20])),
    };
    MapScene {
        cutout: options.cutout.then_some(cutout),
        light: options.lit.then(|| fixture.light()),
        outlines: options.outlines.then_some(&fixture.inks),
        floating: &fixture.floating,
        cursor: options.cursor.then_some(cursor),
        roof_cutaway: None,
        shroud: None,
        overscan: options.overscan,
        highlight: None,
    }
}

/// The view centered on hex `center`, `size` pixels, roofs shown.
fn view_at(center: i32, size: [u32; 2]) -> MapView {
    MapView {
        grid: HexGrid::centered(center, size[0], size[1]).unwrap(),
        elevation: 0,
        roofs: true,
        width: size[0],
        height: size[1],
    }
}

/// T9, T10 and T11 for one refresh: the reference draw log equals the
/// sorted instance list, the CPU executor's canvas equals the reference
/// canvas, and so does the GPU executor's (when a device exists). Returns
/// the plan's counts.
fn parity<A: MapArt>(
    label: &str,
    map: &MapFile,
    objects: &MapObjects,
    view: MapView,
    art: &A,
    scene: MapScene,
    gpu: &mut Option<GpuExecutor>,
) -> FrameCounts {
    let mut log = Vec::new();
    let mut record = |entry: DrawRecord| log.push(entry);
    let reference =
        render_map_scene_recorded(map, objects, view, art, scene, Some(&mut record)).unwrap();
    // The hook changes nothing.
    assert_eq!(
        reference.pixels(),
        render_map_scene(map, objects, view, art, scene)
            .unwrap()
            .pixels(),
        "{label}: recording"
    );
    let palette = scene
        .light
        .map(|light| light.palette)
        .or(scene.cutout.map(|cut| cut.palette));
    let tables = palette.map(LookupTables::from_palette);
    let frame = gather_map_frame(map, objects, view, art, scene, tables.as_ref())
        .unwrap_or_else(|error| panic!("{label}: {error}"));
    // T9: order parity, item for item.
    let records = frame.records().unwrap();
    assert_eq!(records.len(), log.len(), "{label}: draw count");
    for (position, (got, want)) in records.iter().zip(&log).enumerate() {
        assert_eq!(got, want, "{label}: draw {position}");
    }
    // T10: the CPU executor's canvas.
    let tables = tables.unwrap_or_else(|| LookupTables::from_palette(&palette_for_unlit()));
    let cpu = frame.draw_cpu(&tables).unwrap();
    assert_canvas(label, &cpu, &reference);
    // T11: the GPU executor's canvas.
    let plan = BatchPlan::new(&frame.instances).unwrap();
    if let Some(executor) = gpu {
        let mut canvas = frame.base_canvas().unwrap();
        executor
            .execute(&mut canvas, &frame.instances, &plan, &frame.inputs(&tables))
            .unwrap_or_else(|error| panic!("{label}: GPU {error}"));
        assert_canvas(&format!("{label} (GPU)"), &canvas, &reference);
    }
    // T19: the renderer draws it, with no refusal counted.
    let mut renderer = SceneRenderer::cpu();
    let image = renderer.render(map, objects, view, art, scene).unwrap();
    assert_canvas(&format!("{label} (renderer)"), &image, &reference);
    assert_no_refusal(label, &renderer);
    plan.counts()
}

/// T19: the renderer's last refresh was drawn by the instanced path, and
/// nothing was refused for any of §3.16's causes or anything else.
fn assert_no_refusal(label: &str, renderer: &SceneRenderer) {
    assert!(
        matches!(renderer.last_refresh(), Some(RefreshReport::Instanced(_))),
        "{label}: {:?}",
        renderer.last_refresh()
    );
    for cause in [
        "unexpressible draw",
        "peek bound",
        "field domain",
        "table range",
        "unknown light source",
    ] {
        assert_eq!(renderer.refusals(cause), 0, "{label}: {cause}");
    }
    assert_eq!(renderer.logged_causes().count(), 0, "{label}");
}

/// The tables an unlit scene's executor gets (no instance reads them).
fn palette_for_unlit() -> Palette {
    palette()
}

fn assert_canvas(label: &str, got: &IndexedImage, want: &IndexedImage) {
    assert_eq!(
        [got.width(), got.height()],
        [want.width(), want.height()],
        "{label}"
    );
    if let Some(index) = got
        .pixels()
        .iter()
        .zip(want.pixels())
        .position(|(a, b)| a != b)
    {
        let width = got.width() as usize;
        let differing = got
            .pixels()
            .iter()
            .zip(want.pixels())
            .filter(|(a, b)| a != b)
            .count();
        panic!(
            "{label}: {differing} pixels differ, first at ({}, {}): {} != {}",
            index % width,
            index / width,
            got.pixels()[index],
            want.pixels()[index]
        );
    }
}

/// The GPU executor of a GPU test, if a device exists.
fn gpu_executor() -> Option<GpuExecutor> {
    gpu::test_device().map(GpuExecutor::new)
}

#[test]
fn t9_t10_t11_full_refresh_parity_with_every_layer() {
    let _gpu = gpu::test_lock();
    let fixture = Fixture::new();
    let cutout = fixture.cutout();
    let mut gpu = gpu_executor();
    let view = view_at(20100, [640, 380]);
    let scene = scene_of(&fixture, &cutout, view, Options::FULL);
    let counts = parity(
        "full",
        &fixture.map,
        &fixture.objects,
        view,
        &fixture.art,
        scene,
        &mut gpu,
    );
    // Every layer drew: floors, objects split by the cutout, roofs,
    // outlines, texts and objects without a hex; the read modes split the
    // batches.
    assert!(counts.instances > 200, "{counts:?}");
    assert!(
        counts.read_batches > 0 && counts.batches < counts.instances,
        "{counts:?}"
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
    let modes: std::collections::BTreeSet<u8> = frame
        .instances
        .iter()
        .map(|instance| instance.mode as u8)
        .collect();
    assert_eq!(modes, (0..6).collect(), "every draw mode is used");
    let layers: std::collections::BTreeSet<u8> = frame
        .instances
        .iter()
        .map(|instance| instance.key.layer())
        .collect();
    assert_eq!(layers, (0..7).collect(), "every layer is drawn");
    // The hidden roof group draws no roof square over it.
    let hidden_rank = square_order()
        .iter()
        .position(|(index, _)| *index == 4747)
        .unwrap() as u32;
    assert!(
        frame
            .instances
            .iter()
            .all(|instance| instance.key.layer() != 3 || instance.key.primary() != hidden_rank)
    );
}

#[test]
fn t9_t10_t11_scene_variants_keep_parity() {
    let _gpu = gpu::test_lock();
    let fixture = Fixture::new();
    let cutout = fixture.cutout();
    let mut gpu = gpu_executor();
    let view = view_at(20100, [640, 380]);
    let variants = [
        (
            "no roofs",
            MapView {
                roofs: false,
                ..view
            },
            Options::FULL,
        ),
        (
            "no cutout",
            view,
            Options {
                cutout: false,
                ..Options::FULL
            },
        ),
        (
            "no outlines or cursor",
            view,
            Options {
                outlines: false,
                cursor: false,
                ..Options::FULL
            },
        ),
        (
            "unlit",
            view,
            Options {
                lit: false,
                cutout: false,
                outlines: false,
                ..Options::FULL
            },
        ),
        ("panned", view_at(19905, [640, 380]), Options::FULL),
        ("elsewhere", view_at(30150, [640, 380]), Options::FULL),
    ];
    for (label, view, options) in variants {
        let scene = scene_of(&fixture, &cutout, view, options);
        parity(
            label,
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
fn t9_t10_t11_enhanced_widescreen_views_and_tiles_keep_parity() {
    let _gpu = gpu::test_lock();
    let fixture = Fixture::new();
    let cutout = fixture.cutout();
    let mut gpu = gpu_executor();
    let enhanced = Options {
        overscan: true,
        ..Options::FULL
    };
    // The 1920x1080 window's world canvas at zoom 1.0 and 2.0
    // (`EnhancedLayout`), and two tiles of the zoom 1.0 canvas drawn as
    // separate views (the reference path's partial refreshes).
    for zoom in [1.0, 2.0] {
        let layout = crate::EnhancedLayout::compute([1920, 1080], None, zoom).unwrap();
        let size = layout.world_size;
        let view = view_at(20100, size);
        let scene = scene_of(&fixture, &cutout, view, enhanced);
        parity(
            &format!("widescreen zoom {zoom}"),
            &fixture.map,
            &fixture.objects,
            view,
            &fixture.art,
            scene,
            &mut gpu,
        );
    }
    let canvas = view_at(20100, [854, 380]);
    for corner in [[0_i64, 0_i64], [256, 128]] {
        let mut tile = canvas;
        tile.width = 256;
        tile.height = 256;
        tile.grid.origin = [
            canvas.grid.origin[0] - corner[0],
            canvas.grid.origin[1] - corner[1],
        ];
        let scene = scene_of(&fixture, &cutout, tile, enhanced);
        parity(
            &format!("tile {corner:?}"),
            &fixture.map,
            &fixture.objects,
            tile,
            &fixture.art,
            scene,
            &mut gpu,
        );
    }
}

#[test]
fn t9_t10_t11_walking_critter_reinserts_on_each_step() {
    let _gpu = gpu::test_lock();
    let fixture = Fixture::new();
    let cutout = fixture.cutout();
    let mut gpu = gpu_executor();
    let view = view_at(20100, [640, 380]);
    let mut objects = fixture.objects.clone();
    // A critter walks from 20098 along the row, past the scenery on 20100
    // and on; each step moves it and reinserts it at the list's end, so it
    // is listed after the objects already on its new hex.
    let mut walker = object(&fixture.map, 99, CRITTER, 20098);
    walker.outline = outline::HOSTILE;
    objects.elevations[0].insert(0, walker);
    for step in 0..6 {
        let list = &mut objects.elevations[0];
        let at = list.iter().position(|object| object.id == 99).unwrap();
        let mut walker = list.remove(at);
        walker.hex = Some(20098 + step);
        walker.displacement = [(step % 3) * 4, -(step % 2) * 2];
        list.push(walker);
        let scene = scene_of(&fixture, &cutout, view, Options::FULL);
        parity(
            &format!("walk step {step}"),
            &fixture.map,
            &objects,
            view,
            &fixture.art,
            scene,
            &mut gpu,
        );
        // On the player's hex the walker is the last entry: drawn after
        // the scenery and the player.
        if step == 2 {
            let lists = ListIndexes::new(&objects.elevations[0], None);
            let on_hex: Vec<u32> = objects.elevations[0]
                .iter()
                .enumerate()
                .filter(|(_, object)| object.hex == Some(20100))
                .map(|(index, _)| lists.objects[index])
                .collect();
            assert_eq!(on_hex, [0, 1, 2]);
        }
    }
}

#[test]
fn t9_engine_objects_with_each_translucency_bit_keep_parity() {
    let _gpu = gpu::test_lock();
    // Objects given each translucency bit through the engine's API, drawn
    // from the world's render list.
    use vault_core::{CharacterSheet, WorldState, world::WorldObject};
    let fixture = Fixture::new();
    let cutout = fixture.cutout();
    let mut gpu = gpu_executor();
    let mut world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    for (index, bits) in [0x4000_u32, 0x10000, 0x20000, 0x40000, 0x80000, 0x8000]
        .into_iter()
        .enumerate()
    {
        let mut raw =
            vault_core::world::object_record(SCENERY & 0x0f00_0000 | 1, 500 + index as u32);
        raw.art_id = SCENERY;
        raw.hex = Some(20096 + 2 * index as i32);
        raw.flags = BasicFlags(bits);
        world
            .insert(WorldObject {
                raw,
                prototype: None,
                character: None,
                name: Vec::new(),
                description: Vec::new(),
                owner: None,
                inventory: Vec::new(),
                money: None,
                active: true,
                opponent: 0,
            })
            .unwrap();
    }
    let objects = world.render_objects();
    let view = view_at(20100, [640, 380]);
    let scene = scene_of(&fixture, &cutout, view, Options::FULL);
    parity(
        "engine translucency",
        &fixture.map,
        &objects,
        view,
        &fixture.art,
        scene,
        &mut gpu,
    );
}

/// A corpus written by the synthetic fixture builders, its assets and
/// palette.
struct Corpus {
    root: std::path::PathBuf,
    assets: vault_core::assets::VfsWorldAssets,
}

impl Corpus {
    /// The synthetic corpus (`cargo xtask fixtures`), with the camera
    /// demo's map (no script) beside `maps/synthetic.map`, or with the
    /// gallery's showcase.
    fn new(name: &str, showcase: bool) -> Self {
        use vault_fixtures::{gallery, state};
        let root =
            std::env::temp_dir().join(format!("vault-spec022-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        state::write_corpus(&root);
        if showcase {
            gallery::write_showcase(&root);
        } else {
            std::fs::write(root.join("maps/cam_demo.map"), state::cam_demo_map(-1)).unwrap();
        }
        let vfs: Arc<dyn vault_formats::Vfs> =
            Arc::new(vault_formats::LocalDirProvider::mount(&root).unwrap());
        let assets = vault_core::assets::VfsWorldAssets::new(vfs, "english").unwrap();
        Self { root, assets }
    }
    fn map(&self, path: &str) -> MapFile {
        use vault_core::assets::WorldAssets;
        self.assets.map(path).unwrap()
    }
    fn palette(&self) -> Palette {
        use vault_core::assets::WorldAssets;
        self.assets.palette(b"color.pal").unwrap()
    }
    /// The art of every tile and object of `map`.
    fn art(&self, map: &MapFile) -> CorpusArt {
        use vault_core::assets::WorldAssets;
        let mut art = BTreeMap::new();
        for elevation in 0..3 {
            for square in 0..10000 {
                let Some(tile) = map.tile(elevation, square) else {
                    continue;
                };
                for index in [tile.floor(), tile.roof()] {
                    let index = u32::from(index);
                    if let Ok(archive) = self.assets.object_art(0x0400_0000 | index) {
                        art.insert((true, index), archive);
                    }
                }
            }
        }
        for object in map.objects.elevations.iter().flatten() {
            if let Ok(archive) = self.assets.object_art(object.art_id) {
                art.insert((false, object.art_id), archive);
            }
        }
        CorpusArt(art)
    }
}

impl Drop for Corpus {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A corpus's art by tile index and art identifier, with the art shifts.
struct CorpusArt(BTreeMap<(bool, u32), Arc<FrmArchive>>);

impl MapArt for CorpusArt {
    fn frame(
        &self,
        category: Category,
        art_id: u32,
        orientation: u8,
        frame: usize,
    ) -> Option<(&FrmFrame, [i16; 2])> {
        let archive = self.0.get(&(category == Category::Tile, art_id))?;
        let shift = archive
            .shifts
            .get(usize::from(orientation))
            .copied()
            .unwrap_or_default();
        Some((
            archive
                .frame(usize::from(orientation), frame)
                .or_else(|| archive.frame(usize::from(orientation), 0))?,
            shift,
        ))
    }
}

/// A corpus map's scene: lit around its entry hex, the cutout on the entry
/// hex, opaque and banded outlines on every critter, and floating text.
fn corpus_parity(label: &str, corpus: &Corpus, path: &str, gpu: &mut Option<GpuExecutor>) -> usize {
    let mut map = corpus.map(path);
    let art = corpus.art(&map);
    let palette = corpus.palette();
    let entry = map.header.entry_hex;
    let mut tiles = vec![655_i64; 40000];
    for (center, radius, intensity) in [(entry, 4, 60000), (entry + 404, 3, 45000)] {
        let step = (intensity - 655) / (radius + 1);
        for (hex, tile) in tiles.iter_mut().enumerate() {
            if let Ok(distance) = HexGrid::lattice_distance(center, hex as i32)
                && distance <= radius
            {
                *tile += i64::from(intensity - distance * step);
            }
        }
    }
    let mut critters = 0;
    for object in map.objects.elevations.iter_mut().flatten() {
        if (object.pid >> 24) == 1 {
            object.outline = [outline::RED, outline::HOSTILE, outline::FRIENDLY][critters % 3];
            critters += 1;
        }
    }
    let objects = map.objects.clone();
    let mask = pattern(129, 98, |x, y| ((x * 3 + y) % 135) as u8);
    let cutout = Cutout {
        mask: mask.frame(0, 0).unwrap(),
        hex: entry,
        displacement: [0, 0],
        hidden: false,
        extended: &|_| 0,
        palette: &palette,
    };
    let inks = OutlineInks::new(&palette);
    let mut text = IndexedImage::new(12, 6).unwrap();
    text.pixels_mut()
        .iter_mut()
        .step_by(2)
        .for_each(|pixel| *pixel = 77);
    let floating = [FloatingSprite {
        hex: entry,
        offset: [-6, -60],
        image: text,
    }];
    let mut drawn = 0;
    for (name, size, overscan) in [
        ("map view", [640, 380], false),
        ("widescreen", [1138, 380], true),
        ("zoom 2", [569, 190], true),
    ] {
        let view = MapView {
            grid: HexGrid::centered(entry, size[0], size[1]).unwrap(),
            elevation: map.header.entry_elevation,
            roofs: true,
            width: size[0],
            height: size[1],
        };
        let scene = MapScene {
            cutout: Some(&cutout),
            light: Some(MapLight {
                palette: &palette,
                ambient: 20000,
                tiles: &tiles,
            }),
            outlines: Some(&inks),
            floating: &floating,
            cursor: None,
            roof_cutaway: None,
            shroud: None,
            overscan,
            highlight: None,
        };
        drawn += parity(
            &format!("{label} {name}"),
            &map,
            &objects,
            view,
            &art,
            scene,
            gpu,
        )
        .instances;
    }
    drawn
}

#[test]
fn t9_t10_t11_synthetic_corpus_maps_keep_parity() {
    let _gpu = gpu::test_lock();
    let mut gpu = gpu_executor();
    let corpus = Corpus::new("corpus", false);
    assert!(corpus_parity("synthetic.map", &corpus, "maps/synthetic.map", &mut gpu) > 0);
    assert!(corpus_parity("cam_demo.map", &corpus, "maps/cam_demo.map", &mut gpu) > 0);
    let showcase = Corpus::new("showcase", true);
    assert!(
        corpus_parity(
            "showcase",
            &showcase,
            vault_fixtures::gallery::SHOWCASE_MAP,
            &mut gpu
        ) > 0
    );
}

/// The reference X-ray's cutaway for the cursor at canvas pixel `at` and
/// `zoom`, as the `enhanced` view builds it (96 and 48 interface pixels
/// over the zoom, rounded).
fn reference_cutaway(at: [i64; 2], zoom: f64) -> RoofCutaway {
    let [inner, ring] = RoofPeekParams::radii(zoom).unwrap();
    RoofCutaway {
        center: at,
        inner: inner as u32,
        ring: ring as u32,
    }
}

/// The tile of `canvas` with corner `corner`, as a separate view.
fn tile_of(canvas: MapView, corner: [i64; 2], size: [u32; 2]) -> MapView {
    let mut tile = canvas;
    tile.width = size[0];
    tile.height = size[1];
    tile.grid.origin = [
        canvas.grid.origin[0] - corner[0],
        canvas.grid.origin[1] - corner[1],
    ];
    tile
}

#[test]
fn t8_the_reference_xray_and_the_spec_peek_draw_the_same_canvas() {
    // SPEC-022 §3.10 parity rule: the showcase map with `cursor_xray` held,
    // zooms 0.5, 1.0, 1.25, 2.0 and 3.0, a canvas of two tiles whose seam
    // runs through the roofed building, and the pointer at three positions:
    // two over roofs, away from a pixel's middle, and one whose ring
    // crosses the seam.
    let _gpu = gpu::test_lock();
    let mut gpu = gpu_executor();
    let corpus = Corpus::new("peek", true);
    let map = corpus.map(vault_fixtures::gallery::SHOWCASE_MAP);
    let art = corpus.art(&map);
    let palette = corpus.palette();
    let tiles = vec![655_i64; 40000];
    // The roofed building's interior.
    let entry = vault_fixtures::state::CAM_DEMO_INTERIOR;
    let light = MapLight {
        palette: &palette,
        ambient: 30000,
        tiles: &tiles,
    };
    let lit = MapScene {
        light: Some(light),
        ..MapScene::default()
    };
    let canvas = MapView {
        grid: HexGrid::centered(entry, 1280, 380).unwrap(),
        elevation: map.header.entry_elevation,
        roofs: true,
        width: 1280,
        height: 380,
    };
    let roofs: Vec<[i64; 2]> = roof_squares(&map, canvas, &art)
        .map(|(_, frame, corner)| {
            [
                corner[0] + i64::from(frame.width / 2),
                corner[1] + i64::from(frame.height / 2),
            ]
        })
        .filter(|[x, y]| (0..1280).contains(x) && (0..380).contains(y))
        .collect();
    assert!(roofs.len() >= 3, "the showcase has roofs in view");
    let seam_y = roofs[roofs.len() / 2][1] as f64 + 0.25;
    for zoom in [0.5, 1.0, 1.25, 2.0, 3.0] {
        let [inner, _] = RoofPeekParams::radii(zoom).unwrap();
        let pointers = [
            [roofs[0][0] as f64 + 0.75, roofs[0][1] as f64 + 0.25],
            [
                roofs[roofs.len() - 1][0] as f64 + 0.125,
                roofs[roofs.len() - 1][1] as f64 + 0.875,
            ],
            [640.0 + inner as f64 / 2.0 + 0.75, seam_y],
        ];
        for pointer in pointers {
            let mut changed = false;
            for corner in [[0, 0], [640, 0]] {
                let tile = tile_of(canvas, corner, [640, 380]);
                let at = RoofPeekParams::center_of(pointer, corner);
                let scene = MapScene {
                    roof_cutaway: Some(reference_cutaway(at, zoom)),
                    ..lit
                };
                let label = format!("zoom {zoom} at {pointer:?} in tile {corner:?}");
                parity(&label, &map, &map.objects, tile, &art, scene, &mut gpu);
                let peeked = render_map_scene(&map, &map.objects, tile, &art, scene).unwrap();
                let bare = render_map_scene(&map, &map.objects, tile, &art, lit).unwrap();
                changed |= peeked.pixels() != bare.pixels();
            }
            assert!(changed, "zoom {zoom} at {pointer:?}: the X-ray cuts roofs");
        }
    }
    // The ring crosses the seam: both tiles lose roof pixels at zoom 1.0.
    let [inner, _] = RoofPeekParams::radii(1.0).unwrap();
    let pointer = [640.0 + inner as f64 / 2.0 + 0.75, seam_y];
    for corner in [[0, 0], [640, 0]] {
        let tile = tile_of(canvas, corner, [640, 380]);
        let scene = MapScene {
            roof_cutaway: Some(reference_cutaway(
                RoofPeekParams::center_of(pointer, corner),
                1.0,
            )),
            ..lit
        };
        assert_ne!(
            render_map_scene(&map, &map.objects, tile, &art, scene)
                .unwrap()
                .pixels(),
            render_map_scene(&map, &map.objects, tile, &art, lit)
                .unwrap()
                .pixels(),
            "tile {corner:?}"
        );
    }
    // A peek off the roofs' view is drawn the same, and marks the roofs.
    let tables = LookupTables::from_palette(&palette);
    let scene = MapScene {
        roof_cutaway: Some(reference_cutaway([320, 190], 1.0)),
        ..lit
    };
    let view = tile_of(canvas, [0, 0], [640, 380]);
    let frame = gather_map_frame(&map, &map.objects, view, &art, scene, Some(&tables)).unwrap();
    let peek = frame.uniforms.peek.unwrap();
    assert_eq!((peek.center, peek.inner, peek.ring), ([320, 190], 96, 48));
    assert!(frame.instances.iter().any(|item| item.key.layer() == 3));
    assert!(
        frame
            .instances
            .iter()
            .all(|item| (item.key.layer() == 3) == (item.flags & PEEK != 0))
    );
    // Roofs off in the view: no peek, as the reference draws none.
    let bare = MapView {
        roofs: false,
        ..view
    };
    let frame = gather_map_frame(&map, &map.objects, bare, &art, scene, Some(&tables)).unwrap();
    assert_eq!(frame.uniforms.peek, None);
}

#[test]
fn t8_t19_a_peek_a_safe_bound_changes_falls_back() {
    let fixture = Fixture::new();
    let cutout = fixture.cutout();
    let view = view_at(20100, [640, 380]);
    let base = scene_of(&fixture, &cutout, view, Options::FULL);
    let (map, objects, art) = (&fixture.map, &fixture.objects, &fixture.art);
    let tables = LookupTables::from_palette(&fixture.palette);
    for cutaway in [
        // Zoom 0.0625: the radii 1536 and 768, capped.
        reference_cutaway([320, 190], 0.0625),
        // A center past ±2^20.
        reference_cutaway([1 << 21, 190], 1.0),
    ] {
        let scene = MapScene {
            roof_cutaway: Some(cutaway),
            ..base
        };
        assert!(matches!(
            gather_map_frame(map, objects, view, art, scene, Some(&tables)),
            Err(instanced::GatherError::Refused(InstanceError::PeekBound))
        ));
        let mut renderer = SceneRenderer::cpu();
        let image = renderer.render(map, objects, view, art, scene).unwrap();
        assert_eq!(
            image.pixels(),
            render_map_scene(map, objects, view, art, scene)
                .unwrap()
                .pixels()
        );
        assert_eq!(
            renderer.last_refresh(),
            Some(&RefreshReport::Refused(InstanceError::PeekBound))
        );
        assert_eq!(renderer.refusals("peek bound"), 1);
    }
}

#[test]
fn t12_each_refusal_draws_the_reference_canvas_and_logs_its_cause_once() {
    let _gpu = gpu::test_lock();
    let fixture = Fixture::new();
    let cutout = fixture.cutout();
    let view = view_at(20100, [640, 380]);
    let scene = scene_of(&fixture, &cutout, view, Options::FULL);
    let (map, objects, art) = (&fixture.map, &fixture.objects, &fixture.art);
    let reference = render_map_scene(map, objects, view, art, scene).unwrap();
    let twice = |renderer: &mut SceneRenderer, scene: MapScene, cause: &str| {
        for _ in 0..2 {
            let image = renderer.render(map, objects, view, art, scene).unwrap();
            assert_eq!(
                image.pixels(),
                render_map_scene(map, objects, view, art, scene)
                    .unwrap()
                    .pixels(),
                "{cause}"
            );
            assert!(
                matches!(renderer.last_refresh(), Some(RefreshReport::Refused(error)) if error.cause() == cause),
                "{cause}: {:?}",
                renderer.last_refresh()
            );
        }
        assert_eq!(renderer.refusals(cause), 2, "{cause}");
        assert_eq!(
            renderer
                .logged_causes()
                .filter(|logged| *logged == cause)
                .count(),
            1,
            "{cause}"
        );
    };
    // A duplicate key.
    let mut renderer = SceneRenderer::cpu();
    renderer.tamper(|instances| {
        let key = instances[0].key;
        instances[1].key = key;
    });
    twice(&mut renderer, scene, "duplicate key");
    // An oversized canvas.
    let mut renderer = SceneRenderer::cpu();
    renderer.limit_canvas(512);
    twice(&mut renderer, scene, "texture limit");
    // A draw no mode expresses: an opaque ring paint of index 0.
    let zero_inks = OutlineInks::new(&fixture.palette).with_red(0);
    let zero_ring = MapScene {
        outlines: Some(&zero_inks),
        ..scene
    };
    twice(&mut SceneRenderer::cpu(), zero_ring, "unexpressible draw");
    // A peek a safe bound changes.
    let peek = MapScene {
        roof_cutaway: Some(reference_cutaway([320, 190], 0.0625)),
        ..scene
    };
    twice(&mut SceneRenderer::cpu(), peek, "peek bound");
    // A refresh the instanced path accepts draws the same canvas.
    let mut renderer = SceneRenderer::cpu();
    let image = renderer.render(map, objects, view, art, scene).unwrap();
    assert_eq!(image.pixels(), reference.pixels());
    assert!(matches!(
        renderer.last_refresh(),
        Some(RefreshReport::Instanced(counts)) if counts.instances > 0
    ));
    assert_eq!(renderer.logged_causes().count(), 0);
    // A lost device: the reference path to the session's end.
    if let Some(device) = gpu::test_device() {
        let mut renderer = SceneRenderer::with_device(device);
        let image = renderer.render(map, objects, view, art, scene).unwrap();
        assert_eq!(image.pixels(), reference.pixels());
        assert!(matches!(
            renderer.last_refresh(),
            Some(RefreshReport::Instanced(_))
        ));
        renderer.gpu().unwrap().device().destroy();
        twice(&mut renderer, scene, "device lost");
        assert!(
            renderer.gpu().is_none(),
            "the session keeps the reference path"
        );
    }
    // The reference path is the default, and never refuses.
    let mut renderer = SceneRenderer::new(RenderPath::default());
    let image = renderer.render(map, objects, view, art, scene).unwrap();
    assert_eq!(image.pixels(), reference.pixels());
    assert_eq!(renderer.last_refresh(), Some(&RefreshReport::Reference));
}

#[test]
fn t12_a_refresh_failing_on_both_paths_returns_the_reference_error() {
    let fixture = Fixture::new();
    let cutout = fixture.cutout();
    let mut view = view_at(20100, [640, 380]);
    let scene = scene_of(&fixture, &cutout, view, Options::FULL);
    view.elevation = 3;
    let mut renderer = SceneRenderer::cpu();
    assert!(matches!(
        renderer.render(&fixture.map, &fixture.objects, view, &fixture.art, scene),
        Err(MapRenderError::Elevation)
    ));
}

#[test]
fn t7_the_engine_falloff_world_packs_the_fixture_buffer() {
    // SPEC-022 §5.3 through the engine's API: two items that do not block
    // (0x10) and let light pass (0x20000000), on an otherwise empty
    // elevation 0 (the player moved to elevation 2).
    use vault_core::{CharacterSheet, WorldState, world::WorldObject};
    let mut world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let player = world.player;
    world.move_object(player, 0, 2, false).unwrap();
    for (index, (hex, radius, intensity)) in [(20100, 2, 65536), (20104, 3, 32768)]
        .into_iter()
        .enumerate()
    {
        let mut raw = vault_core::world::object_record(0x0000_0001, 900 + index as u32);
        raw.hex = Some(hex);
        raw.elevation = 0;
        raw.flags = BasicFlags(0x10 | 0x2000_0000);
        let item = world
            .insert(WorldObject {
                raw,
                prototype: None,
                character: None,
                name: Vec::new(),
                description: Vec::new(),
                owner: None,
                inventory: Vec::new(),
                money: None,
                active: true,
                opponent: 0,
            })
            .unwrap();
        world.set_light(item, radius, intensity);
    }
    let buffer = HexLightBuffer::pack(&world.tile_light[0]);
    let differing = buffer
        .values()
        .iter()
        .filter(|value| **value != 655)
        .count();
    assert_eq!(differing, 49, "SPEC-022 §5.3: 49 hexes differ from 655");
    for (hex, value) in [
        (20100, 65536),
        (20101, 53248),
        (20102, 39649),
        (20103, 25395),
        (20104, 33423),
        (19900, 44564),
        (19504, 9339),
    ] {
        assert_eq!(buffer.get(hex), value, "hex {hex}");
    }
    assert_eq!(buffer.digest(), 0xD450_03EF_0BA7_8613);
}

#[test]
fn t13_refreshes_change_no_world_state_on_either_path() {
    let _gpu = gpu::test_lock();
    use vault_core::{CharacterSheet, WorldState, world::WorldObject};
    let fixture = Fixture::new();
    let cutout = fixture.cutout();
    let mut world = WorldState::new(CharacterSheet::default(), 7).unwrap();
    for (index, (hex, bits)) in [(20100, 0), (20102, 0x20000), (19900, 0x4000)]
        .into_iter()
        .enumerate()
    {
        let mut raw =
            vault_core::world::object_record(SCENERY & 0x0f00_0000 | 1, 800 + index as u32);
        raw.art_id = SCENERY;
        raw.hex = Some(hex);
        raw.flags = BasicFlags(bits);
        world
            .insert(WorldObject {
                raw,
                prototype: None,
                character: None,
                name: Vec::new(),
                description: Vec::new(),
                owner: None,
                inventory: Vec::new(),
                money: None,
                active: true,
                opponent: 0,
            })
            .unwrap();
    }
    world.rebuild_lighting();
    let before = world.state_hash();
    let view = view_at(20100, [320, 240]);
    let mut renderers = vec![
        SceneRenderer::new(RenderPath::Reference),
        SceneRenderer::cpu(),
    ];
    if let Some(device) = gpu::test_device() {
        renderers.push(SceneRenderer::with_device(device));
    }
    let mut digests = Vec::new();
    for renderer in &mut renderers {
        let mut last = None;
        for _ in 0..100 {
            let objects = world.render_objects();
            let light = MapLight {
                palette: &fixture.palette,
                ambient: world.ambient,
                tiles: &world.tile_light[0],
            };
            let scene = MapScene {
                light: Some(light),
                cutout: Some(&cutout),
                ..MapScene::default()
            };
            let image = renderer
                .render(&fixture.map, &objects, view, &fixture.art, scene)
                .unwrap();
            last = Some(crate::layers::fnv1a64(image.pixels()));
        }
        assert_eq!(world.state_hash(), before);
        digests.push(last.unwrap());
        assert!(!matches!(
            renderer.last_refresh(),
            Some(RefreshReport::Refused(_))
        ));
    }
    assert!(
        digests.windows(2).all(|pair| pair[0] == pair[1]),
        "{digests:?}"
    );
}

/// The GPU canvas retention (`[May-improve]`): a fixed loop of whole
/// refreshes of the full scene on one executor stays byte-identical to
/// the reference canvas, and reports its per-refresh resource and
/// transfer counts and wall time (`--nocapture`).
#[test]
fn gpu_refresh_loop_keeps_parity_and_reports_its_transfers() {
    let _gpu = gpu::test_lock();
    let Some(mut executor) = gpu_executor() else {
        return;
    };
    const REFRESHES: u64 = 20;
    let fixture = Fixture::new();
    let cutout = fixture.cutout();
    let view = view_at(20100, [640, 380]);
    let scene = scene_of(&fixture, &cutout, view, Options::FULL);
    let reference =
        render_map_scene(&fixture.map, &fixture.objects, view, &fixture.art, scene).unwrap();
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
    let plan = BatchPlan::new(&frame.instances).unwrap();
    let inputs = frame.inputs(&tables);
    // The first refresh creates the resources; the loop measures the rest.
    let mut canvas = frame.base_canvas().unwrap();
    executor
        .execute(&mut canvas, &frame.instances, &plan, &inputs)
        .unwrap();
    assert_canvas("first", &canvas, &reference);
    let first = executor.stats();
    let start = std::time::Instant::now();
    for refresh in 0..REFRESHES {
        let mut canvas = frame.base_canvas().unwrap();
        executor
            .execute(&mut canvas, &frame.instances, &plan, &inputs)
            .unwrap();
        assert_canvas(&format!("refresh {refresh}"), &canvas, &reference);
    }
    let elapsed = start.elapsed();
    let stats = executor.stats();
    let per = |total: u64, before: u64| (total - before) as f64 / REFRESHES as f64;
    println!(
        "first refresh: {first:?}\nper refresh over {REFRESHES}: textures {}, buffers {}, \
         bind groups {}, canvas upload {} B, all uploads {} B, readback {} B, wall {:?}",
        per(stats.textures, first.textures),
        per(stats.buffers, first.buffers),
        per(stats.bind_groups, first.bind_groups),
        per(stats.canvas_upload_bytes, first.canvas_upload_bytes),
        per(stats.upload_bytes, first.upload_bytes),
        per(stats.readback_bytes, first.readback_bytes),
        elapsed / REFRESHES as u32,
    );
    assert_eq!(stats.refreshes, REFRESHES + 1);
    // Kept between refreshes: no texture, buffer or bind group is created
    // again, the zero base fill is a clear pass, nothing equal is
    // uploaded again, and at most the whole canvas is read back.
    assert_eq!(
        [stats.textures, stats.buffers, stats.bind_groups],
        [first.textures, first.buffers, first.bind_groups]
    );
    assert_eq!(stats.canvas_clears, REFRESHES + 1);
    assert_eq!(stats.upload_bytes, first.upload_bytes);
    assert!(stats.readback_bytes - first.readback_bytes <= REFRESHES * first.readback_bytes);
}

/// The fixture's region: a block of hexes around the player, smaller than
/// the grid, so the shroud's dither shows in the view.
fn region() -> Vec<bool> {
    (0..40000)
        .map(|hex| (92..108).contains(&(hex % 200)) && (96..104).contains(&(hex / 200)))
        .collect()
}

#[test]
fn t10_t11_tiles_with_the_shroud_a_highlight_and_the_peek_keep_parity() {
    let _gpu = gpu::test_lock();
    let fixture = Fixture::new();
    let cutout = fixture.cutout();
    let mut gpu = gpu_executor();
    let allowed = region();
    let enhanced = Options {
        overscan: true,
        ..Options::FULL
    };
    let shroud = Shroud {
        allowed: &allowed,
        dark: 3,
    };
    // A canvas two tiles wide and tall; the zoom sets the peek's radii.
    let size = [1280, 760];
    for zoom in [1.0, 2.0] {
        let canvas = view_at(20100, size);
        let [cx, cy] = canvas.grid.hex_center(20100).unwrap();
        // The first tile, the tile beside it, and the player's tile.
        let player = [cx - cx % 640, cy - cy % 380];
        let tile_size = |corner: [i64; 2]| {
            [
                (i64::from(size[0]) - corner[0]).min(640) as u32,
                (i64::from(size[1]) - corner[1]).min(380) as u32,
            ]
        };
        for corner in [[0, 0], [640, 0], [0, 380], player] {
            let view = tile_of(canvas, corner, tile_size(corner));
            let shrouded = MapScene {
                shroud: Some(shroud),
                highlight: Some(outline::ITEM),
                ..scene_of(&fixture, &cutout, view, enhanced)
            };
            parity(
                &format!("zoom {zoom}, tile {corner:?}"),
                &fixture.map,
                &fixture.objects,
                view,
                &fixture.art,
                shrouded,
                &mut gpu,
            );
            let [ax, ay] = [cx + 8, cy - 16];
            let peeked = MapScene {
                roof_cutaway: Some(reference_cutaway([ax - corner[0], ay - corner[1]], zoom)),
                ..shrouded
            };
            parity(
                &format!("zoom {zoom}, tile {corner:?}, peek"),
                &fixture.map,
                &fixture.objects,
                view,
                &fixture.art,
                peeked,
                &mut gpu,
            );
        }
        // The tile with the player has a shroud and a highlight to show.
        let view = tile_of(canvas, player, tile_size(player));
        let scene = MapScene {
            shroud: Some(shroud),
            highlight: Some(outline::ITEM),
            ..scene_of(&fixture, &cutout, view, enhanced)
        };
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
        let without = MapScene {
            highlight: None,
            ..scene
        };
        let plain = gather_map_frame(
            &fixture.map,
            &fixture.objects,
            view,
            &fixture.art,
            without,
            Some(&tables),
        )
        .unwrap();
        let outlines = |frame: &MapFrame| {
            frame
                .instances
                .iter()
                .filter(|item| item.key.layer() == 4)
                .count()
        };
        assert!(outlines(&frame) > outlines(&plain), "a highlight is drawn");
        let shroud_draw = frame
            .instances
            .iter()
            .find(|item| item.mode == DrawMode::Remap)
            .unwrap();
        let image = &frame.images[shroud_draw.image];
        assert!(image.pixels.contains(&1) && image.pixels.contains(&0));
    }
}

#[test]
fn t15_square_ranks_give_the_fixture_rows_and_the_reference_log() {
    let ranked = |roof: bool| -> Vec<i32> {
        let grid = HexGrid {
            origin: [-3680, -430],
        };
        let mut squares: Vec<(i64, i64, i32)> = (0..10000)
            .map(|index| {
                let [x, y] = grid.square_corner(index, roof).unwrap();
                (y, x, index)
            })
            .collect();
        squares.sort_unstable();
        squares.into_iter().map(|(.., index)| index).collect()
    };
    let (floors, roofs) = (ranked(false), ranked(true));
    assert_eq!(floors, roofs, "one rank on the floor and on the roof");
    let order: Vec<i32> = square_order().iter().map(|(index, _)| *index).collect();
    assert_eq!(floors, order);
    let text = include_str!("../../../../tests/fixtures/spec022/rev3/square-rank.tsv");
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap().split('\t').collect();
    let column = |name: &str| header.iter().position(|cell| *cell == name).unwrap();
    let (square, rank) = (column("square"), column("rank"));
    let mut rows = 0;
    for line in lines.filter(|line| !line.is_empty()) {
        let cells: Vec<&str> = line.split('\t').collect();
        let index: i32 = cells[square].parse().unwrap();
        let position = order.iter().position(|other| *other == index).unwrap();
        assert_eq!(position.to_string(), cells[rank], "square {index}");
        rows += 1;
    }
    assert_eq!(rows, 13);
    // The ranks do not change with the grid origin, and the log has gaps.
    let fixture = Fixture::new();
    let view = view_at(20100, [640, 380]);
    let scene = MapScene::default();
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
    let floor: Vec<u32> = log
        .iter()
        .filter(|entry| entry.layer == FrameLayer::Floor)
        .map(|entry| entry.primary)
        .collect();
    assert!(floor.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(floor.windows(2).any(|pair| pair[1] - pair[0] > 1));
    for rank in floor {
        let corner = view
            .grid
            .square_corner(order[rank as usize], false)
            .unwrap();
        assert!(corner[0] < 640 && corner[1] < 380, "rank {rank}");
    }
}

#[test]
fn t9_the_list_index_counts_only_the_current_elevation() {
    // The same hex on two elevations: each elevation's list starts from 0,
    // hidden entries count though they are not drawn, and the cursor and
    // its text take two places.
    let map = map();
    let mut objects = scene(Vec::new());
    let mut hidden = object(&map, 1, CRITTER, 20100);
    hidden.flags.0 |= BasicFlags::HIDDEN.0;
    objects.elevations[1] = vec![object(&map, 2, CRITTER, 20100); 3];
    objects.elevations[0] = vec![
        hidden,
        object(&map, 3, SCENERY, 20100),
        object(&map, 4, CRITTER, 20100),
    ];
    let lists = ListIndexes::new(&objects.elevations[0], Some((20100, 2)));
    assert_eq!(lists.objects, [0, 1, 4]);
    assert_eq!(lists.cursor, [2, 3]);
    let lists = ListIndexes::new(&objects.elevations[1], None);
    assert_eq!(lists.objects, [0, 1, 2]);
    // Both paths give those numbers.
    let art = art();
    let view = view_at(20100, [640, 380]);
    let mut gpu = None;
    let upright = |scene: MapScene| {
        let frame = gather_map_frame(&map, &objects, view, &art, scene, None).unwrap();
        frame
            .records()
            .unwrap()
            .into_iter()
            .filter(|record| record.layer == FrameLayer::Upright)
            .map(|record| record.index)
            .collect::<Vec<u32>>()
    };
    parity(
        "two elevations",
        &map,
        &objects,
        view,
        &art,
        MapScene::default(),
        &mut gpu,
    );
    assert_eq!(upright(MapScene::default()), [1, 2]);
}

#[test]
fn t16_a_hand_built_outline_gray_draws_with_row_7_on_both_paths() {
    let _gpu = gpu::test_lock();
    let fixture = Fixture::new();
    let cutout = fixture.cutout();
    let mut gpu = gpu_executor();
    let view = view_at(20100, [640, 380]);
    let top = OutlineInks::new(&fixture.palette).with_gray(7);
    let row_7 = MapScene {
        outlines: Some(&top),
        ..scene_of(&fixture, &cutout, view, Options::FULL)
    };
    let want = render_map_scene(&fixture.map, &fixture.objects, view, &fixture.art, row_7).unwrap();
    for gray in [8, 15] {
        let inks = OutlineInks::new(&fixture.palette).with_gray(gray);
        let scene = MapScene {
            outlines: Some(&inks),
            ..row_7
        };
        let label = format!("gray {gray}");
        parity(
            &label,
            &fixture.map,
            &fixture.objects,
            view,
            &fixture.art,
            scene,
            &mut gpu,
        );
        let got =
            render_map_scene(&fixture.map, &fixture.objects, view, &fixture.art, scene).unwrap();
        assert_canvas(&label, &got, &want);
    }
}

#[test]
fn t17_a_the_shroud_past_the_grid_is_drawn_instanced() {
    let _gpu = gpu::test_lock();
    let fixture = Fixture::new();
    let cutout = fixture.cutout();
    let mut gpu = gpu_executor();
    let allowed = vec![true; 40000];
    // The whole grid allowed, as the shell's unlocked view, from the
    // grid's top edge; and a region smaller than the grid.
    let edge = view_at(150, [1138, 380]);
    let scene = MapScene {
        shroud: Some(Shroud {
            allowed: &allowed,
            dark: 3,
        }),
        ..scene_of(&fixture, &cutout, edge, Options::FULL)
    };
    let counts = parity(
        "past the grid",
        &fixture.map,
        &fixture.objects,
        edge,
        &fixture.art,
        scene,
        &mut gpu,
    );
    assert!(counts.read_batches > 0, "{counts:?}");
    let reference =
        render_map_scene(&fixture.map, &fixture.objects, edge, &fixture.art, scene).unwrap();
    assert!(reference.pixels().contains(&3), "the shroud shows");
    // The shroud sits after every roof and before the first outline.
    let tables = LookupTables::from_palette(&fixture.palette);
    let view = view_at(20100, [640, 380]);
    let scene = MapScene {
        shroud: Some(Shroud {
            allowed: &allowed,
            dark: 3,
        }),
        ..scene_of(&fixture, &cutout, view, Options::FULL)
    };
    let frame = gather_map_frame(
        &fixture.map,
        &fixture.objects,
        view,
        &fixture.art,
        scene,
        Some(&tables),
    )
    .unwrap();
    let records = frame.records().unwrap();
    let at = records
        .iter()
        .position(|record| *record == DrawRecord::new(FrameLayer::Roof, 10000, 0, 0))
        .unwrap();
    assert!(records[..at].iter().all(|record| record.primary < 40000));
    assert_eq!(records[at - 1].layer, FrameLayer::Roof);
    assert_eq!(records[at + 1].layer, FrameLayer::Outline);
    let shroud = frame
        .instances
        .iter()
        .find(|item| item.mode == DrawMode::Remap)
        .unwrap();
    assert_eq!(shroud.light, LightSource::None);
    assert_eq!(shroud.key.to_string(), "6000271000000000");
    assert_eq!(shroud.dst, [0, 0]);
    assert_eq!(shroud.size, [640, 380]);
    let (table, at) = shroud.level.unwrap();
    assert_eq!(at, [0, 0]);
    assert_eq!(frame.images[table].pixels.as_ref(), &[3; 256]);
    // A dark index of 0 is still written, by the table.
    let allowed = region();
    let scene = MapScene {
        shroud: Some(Shroud {
            allowed: &allowed,
            dark: 0,
        }),
        ..scene
    };
    parity(
        "dark index 0",
        &fixture.map,
        &fixture.objects,
        view,
        &fixture.art,
        scene,
        &mut gpu,
    );
}

/// `outlined` outlined critters and two ground items.
fn highlight_objects(map: &MapFile, outlined: u32) -> MapObjects {
    let mut list = Vec::new();
    for id in 0..outlined {
        let hex = (95 + id % 10) + 200 * (95 + id / 10);
        let mut critter = object(map, id + 1, CRITTER, hex as i32);
        critter.outline = outline::RED;
        list.push(critter);
    }
    list.push(object(map, 900, ITEM, 20308));
    list.push(object(map, 901, ITEM, 20310));
    scene(list)
}

#[test]
fn t17_b_highlights_are_drawn_at_any_running_number() {
    let _gpu = gpu::test_lock();
    let fixture = Fixture::new();
    let cutout = fixture.cutout();
    let mut gpu = gpu_executor();
    let view = view_at(20100, [640, 380]);
    for (outlined, positions) in [(100, [100, 101]), (101, [100, 101]), (3, [3, 4])] {
        let objects = highlight_objects(&fixture.map, outlined);
        let scene = MapScene {
            highlight: Some(outline::ITEM),
            ..scene_of(&fixture, &cutout, view, Options::FULL)
        };
        let scene = MapScene {
            cursor: None,
            ..scene
        };
        parity(
            &format!("{outlined} outlined"),
            &fixture.map,
            &objects,
            view,
            &fixture.art,
            scene,
            &mut gpu,
        );
        let tables = LookupTables::from_palette(&fixture.palette);
        let frame = gather_map_frame(
            &fixture.map,
            &objects,
            view,
            &fixture.art,
            scene,
            Some(&tables),
        )
        .unwrap();
        let listed: Vec<u32> = frame
            .records()
            .unwrap()
            .iter()
            .filter(|record| record.layer == FrameLayer::Outline)
            .map(|record| record.index)
            .collect();
        let tail = &listed[listed.len() - 2..];
        assert_eq!(tail, positions, "{outlined} outlined");
    }
}

#[test]
fn t17_c_an_unlit_scene_draws_its_cutout_and_source_pixels() {
    let _gpu = gpu::test_lock();
    let fixture = Fixture::new();
    let cutout = fixture.cutout();
    let mut gpu = gpu_executor();
    let view = view_at(20100, [640, 380]);
    let options = Options {
        lit: false,
        ..Options::FULL
    };
    let scene = scene_of(&fixture, &cutout, view, options);
    let counts = parity(
        "unlit",
        &fixture.map,
        &fixture.objects,
        view,
        &fixture.art,
        scene,
        &mut gpu,
    );
    assert!(counts.read_batches > 0, "{counts:?}");
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
    // `rev3/unlit-modes.tsv`: only the cutout blends, without a light.
    let mut eggs = 0;
    for instance in &frame.instances {
        assert_eq!(instance.light, LightSource::None, "{:?}", instance.key);
        match instance.mode {
            DrawMode::Egg => eggs += 1,
            DrawMode::Plain => assert_eq!(instance.flags & EXEMPT, 0),
            DrawMode::OutlineBlend => assert_eq!(instance.key.layer(), 4),
            mode => panic!("{mode:?} in an unlit scene"),
        }
    }
    assert!(eggs > 0, "the player is under a roof");
    // A translucent object draws its source pixels, not a blend.
    let translucent = Options {
        cutout: false,
        cursor: false,
        outlines: false,
        ..options
    };
    let scene = scene_of(&fixture, &cutout, view, translucent);
    let mut objects = scene_objects_at(&fixture, 20708);
    let drawn = render_map_scene(&fixture.map, &objects, view, &fixture.art, scene).unwrap();
    parity(
        "translucent bit",
        &fixture.map,
        &objects,
        view,
        &fixture.art,
        scene,
        &mut gpu,
    );
    objects.elevations[0][0].flags.0 &= !0x20000;
    let opaque = render_map_scene(&fixture.map, &objects, view, &fixture.art, scene).unwrap();
    assert_canvas("translucency ignored", &drawn, &opaque);
}

/// One object with the glass translucency bit on `hex`.
fn scene_objects_at(fixture: &Fixture, hex: i32) -> MapObjects {
    let mut item = object(&fixture.map, 1, SCENERY, hex);
    item.flags.0 |= 0x20000;
    scene(vec![item])
}
