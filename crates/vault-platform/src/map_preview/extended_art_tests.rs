//! Extended art on the platform (owner brief
//! `docs/extended-art-namespaces.md`): a mod layer whose tile list has
//! 4,002 rows plus an `[[art]]` tile row at 5000, a map whose floor uses
//! local indexes 4000 and 4001, and a `[[tile_remap]]` sidecar mapping 4000
//! to 5000 (vector E); and an object drawn with the extended identifier of
//! a registered scenery row.
use super::test_support::{cpu_renderer, synthetic_corpus};
use super::*;
use crate::{config::ModList, preview::PreviewArgs};
use std::path::Path;
use vault_fixtures::{art, world as fixture};
use vault_formats::{LocalDirProvider, pro::Category};
use vault_render::map::object_sprite;

/// Unlit indexes (229-255), so the expected bytes need no light: the art
/// of tile row 5000, row 4001, row 4000, and scenery row 5000.
const ROW_5000: u8 = 242;
const ROW_4001: u8 = 243;
const ROW_4000: u8 = 244;
const SCENERY_5000: u8 = 245;
/// The two view points whose floor squares the layer's map changes.
const POINTS: [[i32; 2]; 2] = [[60, 60], [580, 320]];

/// The synthetic corpus with its map also as `maps/synth.map`.
fn base() -> tempfile::TempDir {
    let corpus = synthetic_corpus();
    std::fs::copy(
        corpus.path().join("maps/synthetic.map"),
        corpus.path().join("maps/synth.map"),
    )
    .unwrap();
    corpus
}

/// `maps/synth.map` over `base`, with `layer` mounted over it.
fn open(base: &Path, layer: Option<&Path>) -> MapPreview {
    let mut vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(base).unwrap());
    if let Some(layer) = layer {
        vfs = PreviewArgs::mount_mods(vfs, &ModList::Paths(vec![layer.to_path_buf()])).unwrap();
    }
    MapPreview::load(
        vfs,
        "maps/synth.map",
        "english",
        Some("premade/synthetic.gcd"),
        1,
    )
    .unwrap()
}

/// The floor squares under [`POINTS`] in the plain map's first view.
fn squares(base: &Path) -> [usize; 2] {
    let preview = open(base, None);
    POINTS.map(|point| preview.view.grid.pick_square(point, false).unwrap() as usize)
}

/// The mod layer `tc` at `root`: tile list rows `t0000.frm`-`t4001.frm`
/// with `[[art]]` tile row 5000 and scenery row 5000; with `tiles`, a map
/// whose floor squares `squares` use local indexes 4000 and 4001 (and with
/// `remap` the vector E sidecar for it); its scenery object drawn with the
/// extended identifier of scenery row 5000 (without `extended`, its plain
/// row 0, whose art the layer then replaces with row 5000's picture).
fn layer(root: &Path, base: &Path, squares: [usize; 2], [tiles, remap, extended]: [bool; 3]) {
    let remaps: &[(&str, &str)] = if remap {
        &[("synth.map", "maps/synth.tlx")]
    } else {
        &[]
    };
    let manifest = art::manifest(
        "tc",
        &[("tiles", "t5000.frm", 5000), ("scenery", "s5000.frm", 5000)],
        &[],
        remaps,
    );
    for dir in ["art/tiles", "art/scenery", "maps"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    std::fs::write(root.join("vault-mod.toml"), manifest).unwrap();
    std::fs::write(root.join("art/tiles/tiles.lst"), art::art_list("t", 4002)).unwrap();
    for (name, colour) in [
        ("t0000", 7),
        ("t0001", 0),
        ("t0002", 7),
        ("t4000", ROW_4000),
        ("t4001", ROW_4001),
        ("t5000", ROW_5000),
    ] {
        let path = root.join(format!("art/tiles/{name}.frm"));
        std::fs::write(path, fixture::frm(80, 36, colour)).unwrap();
    }
    std::fs::write(
        root.join("art/scenery/s5000.frm"),
        fixture::frm(12, 20, SCENERY_5000),
    )
    .unwrap();
    let mut map = std::fs::read(base.join("maps/synth.map")).unwrap();
    if tiles {
        for (square, local) in squares.into_iter().zip([4000, 4001]) {
            fixture::word(&mut map, 248 + 4 * square, 0x0001_0000 | local);
        }
    }
    // The scenery object's record: id 3 on hex 20301; its art at 0x20.
    let record = [3_i32, 20301].map(i32::to_be_bytes).concat();
    let at = (248 + 120_000..map.len() - 8)
        .find(|&at| map[at..at + 8] == record[..])
        .unwrap();
    let fid = Fid::extended(2, 5000).unwrap();
    assert_eq!(fid.0, 0x8200_1388);
    if extended {
        fixture::word(&mut map, at + 0x20, fid.0 as i32);
    } else {
        std::fs::write(
            root.join("art/scenery/sample.frm"),
            fixture::frm(12, 20, SCENERY_5000),
        )
        .unwrap();
    }
    std::fs::write(root.join("maps/synth.map"), map).unwrap();
    std::fs::write(
        root.join("maps/synth.tlx"),
        art::tile_remap(&[(4000, 5000)]),
    )
    .unwrap();
}

/// The canvas pixel at `[x, y]`.
fn at(image: &IndexedImage, [x, y]: [i32; 2]) -> u8 {
    image.pixels()[y as usize * image.width() as usize + x as usize]
}

/// The first view of `preview`, on its renderer.
fn first_view(preview: &mut MapPreview) -> IndexedImage {
    preview.tick(16).unwrap();
    preview.map_view_image().unwrap()
}

/// Vector E on both paths: local index 4000 draws row 5000 through the
/// remap and 4001 draws its own row; without the remap row 4000 draws
/// its own row 4000.
#[test]
fn a_remapped_floor_index_draws_its_row_past_4095_on_both_paths() {
    let base = base();
    let squares = squares(base.path());
    let mods = tempfile::tempdir().unwrap();
    let remapped = mods.path().join("remapped");
    layer(&remapped, base.path(), squares, [true, true, true]);
    let mut reference = open(base.path(), Some(&remapped));
    let image = first_view(&mut reference);
    assert_eq!(at(&image, POINTS[0]), ROW_5000);
    assert_eq!(at(&image, POINTS[1]), ROW_4001);
    assert!(!image.pixels().contains(&ROW_4000), "row 4000 is not drawn");
    let mut instanced = open(base.path(), Some(&remapped));
    instanced.set_scene_renderer(cpu_renderer());
    assert!(first_view(&mut instanced).pixels() == image.pixels());
    let plain = mods.path().join("plain");
    layer(&plain, base.path(), squares, [true, false, true]);
    let image = first_view(&mut open(base.path(), Some(&plain)));
    assert_eq!(at(&image, POINTS[0]), ROW_4000);
    assert_eq!(at(&image, POINTS[1]), ROW_4001);
    assert!(!image.pixels().contains(&ROW_5000));
}

/// The sprite centre of the map's scenery object in `preview`'s view.
fn scenery_centre(preview: &MapPreview) -> [i32; 2] {
    let world = &preview.host.world;
    let (_, object) = world
        .iter()
        .find(|(_, object)| object.category() == Category::Scenery)
        .unwrap();
    let (frame, origin) = object_sprite(preview.view, &preview.art, &object.raw).unwrap();
    [
        origin[0] as i32 + i32::from(frame.width) / 2,
        origin[1] as i32 + i32::from(frame.height) / 2,
    ]
}

/// §3.2 and §3.6.1: an object whose art is the extended identifier of a
/// registered scenery row draws that row; a slot save keeps the identifier
/// and loads to the same canvas; a session without the layer loads the
/// slot and draws the object nowhere, without failing.
#[test]
fn an_extended_scenery_identifier_draws_survives_a_save_and_is_skipped_without_its_layer() {
    use crate::storage::{MemoryStorage, StorageBackend};
    let base = base();
    let squares = squares(base.path());
    let mods = tempfile::tempdir().unwrap();
    let tc = mods.path().join("tc");
    layer(&tc, base.path(), squares, [false, false, true]);
    // The same picture through a plain identifier: what the object should
    // look like, lit as objects are.
    let control = mods.path().join("control");
    layer(&control, base.path(), squares, [false, false, false]);
    let expected = first_view(&mut open(base.path(), Some(&control)));
    let storage: Arc<dyn StorageBackend> = Arc::new(MemoryStorage::new());
    let mut saved = open(base.path(), Some(&tc));
    saved.storage = Some(storage.clone());
    let before = first_view(&mut saved);
    assert!(before.pixels() == expected.pixels(), "row 5000 is drawn");
    let drawn = at(&before, scenery_centre(&saved));
    let info = vault_core::host_save::SaveInfo {
        description: b"Extended".to_vec(),
        host_date: [0; 3],
        host_time: 0,
        thumbnail: None,
    };
    saved.write_game_slot("SLOT01", &info).unwrap();
    let mut loaded = open(base.path(), Some(&tc));
    loaded.storage = Some(storage.clone());
    loaded.read_game_slot("SLOT01").unwrap();
    let after = first_view(&mut loaded);
    assert!(
        after.pixels() == before.pixels(),
        "the same canvas after a load"
    );
    let mut stock = open(base.path(), None);
    stock.storage = Some(storage);
    stock.read_game_slot("SLOT01").unwrap();
    let image = first_view(&mut stock);
    assert!(!image.pixels().contains(&drawn), "not drawn");
}
