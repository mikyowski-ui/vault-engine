//! Extended art identifiers and the mod layers' additive art, prototype and
//! tile-remap rows: SPEC-007 §2.1.1 and §2.1.2, SPEC-024a §2.4 and word
//! `0x830A` `mod_art_fid`, and SPEC-017 §5.2 rule 107 (owner brief
//! `docs/extended-art-namespaces.md`). All data is synthetic: the state
//! corpus as the base, and a layer `mods/tc` built here.
use crate::{
    CharacterSheet, WorldHost, WorldState,
    assets::{VfsWorldAssets, WorldAssets},
    fallout2::Fallout2World,
    fixture_state as fixture,
    host_save::SaveInfo,
    spec024_tests::{
        Op::{I, W},
        diagnostics, run_as,
    },
    world::ObjectRef,
};
use std::{path::Path, sync::Arc};
use vault_fixtures::{art, world::word};
use vault_formats::{Fid, LayeredVfs, LocalDirProvider, ModLayer, Vfs};
use vault_vm::{IntProgram, WordSet, stack_check};

type Host = WorldHost<VfsWorldAssets>;

/// The registered scenery row, and its extended identifier.
const ROW: u32 = 5000;
const EXTENDED: u32 = 0x8200_1388;
/// What the low 12 bits of row 5000 address in a plain identifier.
const WRAPPED: u32 = 0x0200_0388;
/// Scenery prototype entry 5000, which the layer registers.
const TC_PID: u32 = 0x0200_1388;
/// The base corpus's scenery prototype.
const BASE_SCENERY: u32 = 0x0200_0001;
const ART_FID: u16 = 0x830a;
/// The layer frame's action frame (SPEC-002 FRM header bytes 6-7).
const ACTION_FRAME: i16 = 3;

/// `vault-mod.toml` of layer `tc`.
fn tc_manifest() -> String {
    art::manifest(
        "TC",
        &[("scenery", "tc_rock.frm", ROW)],
        &[("scenery", "tc_rock.pro", ROW)],
        &[],
    )
}
/// The interactive fixture map with header `name`, no map script, number
/// `number`, and its scenery object (id 3 at hex 20301) given `pid` and
/// art `fid`.
fn map_with(name: &str, number: i32, pid: u32, fid: u32) -> Vec<u8> {
    let mut map = fixture::interactive_map();
    let header = format!("{}.MAP", name.to_ascii_uppercase());
    map[4..20].fill(0);
    map[4..4 + header.len()].copy_from_slice(header.as_bytes());
    word(&mut map, 0x24, 0);
    word(&mut map, 0x28, 0);
    word(&mut map, 0x34, number);
    let key = [3u32.to_be_bytes(), 20301u32.to_be_bytes()].concat();
    let at = map
        .windows(8)
        .position(|window| window == key.as_slice())
        .expect("the scenery object");
    word(&mut map, at + 0x20, fid as i32);
    word(&mut map, at + 0x2c, pid as i32);
    map
}
/// The corpus `map.msg`: the 66 table names, with `tc.map` as 8 and
/// `plain.map` as 9.
fn write_map_msg(base: &Path) {
    let text: String = crate::worldmap::MAP_TABLE
        .iter()
        .enumerate()
        .map(|(number, entry)| {
            let name = match number {
                7 => "SYNTH.MAP",
                8 => "TC.MAP",
                9 => "PLAIN.MAP",
                _ => entry.name,
            };
            format!("{{{number}}}{{}}{{{name}}}\n")
        })
        .collect();
    std::fs::write(base.join("text/english/game/map.msg"), text).unwrap();
}
/// A base corpus under `dir/base` holding `maps/plain.map` (its scenery
/// object of the base prototype with art [`EXTENDED`]), and a layer
/// `dir/mods/tc` that registers scenery art row 5000 (`tc_rock.frm`) and
/// scenery prototype 5000 (`tc_rock.pro`, art [`EXTENDED`]), and holds
/// `maps/tc.map` with an object of that prototype.
fn corpus() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base");
    fixture::write_corpus(&base);
    write_map_msg(&base);
    std::fs::write(
        base.join("maps/plain.map"),
        map_with("plain", 9, BASE_SCENERY, EXTENDED),
    )
    .unwrap();
    let layer = dir.path().join("mods/tc");
    for folder in ["art/scenery", "proto/scenery", "maps"] {
        std::fs::create_dir_all(layer.join(folder)).unwrap();
    }
    std::fs::write(layer.join("vault-mod.toml"), tc_manifest()).unwrap();
    let mut frame = vault_fixtures::world::frm(12, 20, 40);
    frame[6..8].copy_from_slice(&ACTION_FRAME.to_be_bytes());
    std::fs::write(layer.join("art/scenery/tc_rock.frm"), frame).unwrap();
    let mut prototype = vault_fixtures::world::prototype(2, 0);
    word(&mut prototype, 0, TC_PID as i32);
    word(&mut prototype, 8, EXTENDED as i32);
    std::fs::write(layer.join("proto/scenery/tc_rock.pro"), prototype).unwrap();
    std::fs::write(
        layer.join("maps/tc.map"),
        map_with("tc", 8, TC_PID, EXTENDED),
    )
    .unwrap();
    dir
}
/// The base alone, or the base under layer `mods/<layer>`.
fn mount(dir: &Path, layer: Option<&str>) -> Arc<dyn Vfs> {
    let base: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(dir.join("base")).unwrap());
    let Some(layer) = layer else {
        return base;
    };
    let path = format!("mods/{layer}");
    let loose = LocalDirProvider::mount(dir.join(&path)).unwrap();
    Arc::new(LayeredVfs::new(
        base,
        vec![ModLayer::new(&path, Some(loose), None, None)],
    ))
}
/// A host over `vfs` with the campaign loaded, and `map` if any.
fn host(vfs: Arc<dyn Vfs>, map: Option<&str>) -> Host {
    let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let mut host = WorldHost::new(world, VfsWorldAssets::new(vfs, "english").unwrap());
    host.load_campaign().unwrap();
    if let Some(map) = map {
        host.load_map(map).unwrap();
    }
    host
}
fn find(host: &Host, pid: u32) -> ObjectRef {
    host.world
        .iter()
        .find(|(_, object)| object.raw.pid == pid)
        .map(|(reference, _)| reference)
        .expect("the object")
}
fn art_of(host: &Host, object: ObjectRef) -> u32 {
    host.world.get(object).unwrap().raw.art_id
}
/// A slot image of `from` loaded into `into`, with `from`'s draws.
fn load_into(from: &mut Host, into: &mut Host) {
    let rng = from.world.rng.clone();
    let mut image = from.slot_image(&SaveInfo::default()).unwrap();
    into.world.rng = rng.clone();
    into.load_slot(&mut image).unwrap();
    into.world.rng = rng;
}

#[test]
fn extended_art_a_registered_row_and_prototype_resolve_from_the_layer() {
    let dir = corpus();
    let mut host = host(mount(dir.path(), Some("tc")), Some("maps/tc.map"));
    // The prototype loads by its registered PID, with its stored art.
    let prototype = host.assets.prototype(TC_PID).unwrap();
    assert_eq!((prototype.pid, prototype.art_id), (TC_PID, EXTENDED));
    let object = find(&host, TC_PID);
    assert_eq!(art_of(&host, object), EXTENDED, "the map's art, unchanged");
    // The extended identifier names the layer's file.
    let catalog = host.assets.art_catalog().unwrap();
    assert!(catalog.is_extended(Fid(EXTENDED)));
    assert_eq!(
        catalog.path(Fid(EXTENDED)).unwrap(),
        "art/scenery/tc_rock.frm"
    );
    assert!(host.assets.art_exists(EXTENDED));
    let frame = host.assets.object_art(EXTENDED).unwrap();
    assert_eq!(frame.action_frame, ACTION_FRAME);
    // Its low 12 bits alone address no row of the resolved list's file.
    assert!(!host.assets.art_exists(WRAPPED));
    // `obj_art_fid` gives the identifier unchanged.
    let run = run_as(
        &mut host,
        &[I(object as i32), W(0x8149)],
        &WordSet::FALLOUT1,
        90,
    );
    assert_eq!(run.ints(), [EXTENDED as i32]);
    // The trait's view of the same row.
    assert_eq!(host.assets.art_fid(2, ROW), Some(EXTENDED));
    assert!(host.assets.art_extended(EXTENDED));
    assert_eq!(
        host.assets.art_row_token(2, ROW).as_deref(),
        Some(&b"tc_rock.frm"[..])
    );
    // The list past 4,096 rows is noted once, for the shell's log, and
    // never as a world effect.
    let noted = host.assets.take_art_diagnostics();
    assert_eq!(noted.len(), 1, "{noted:?}");
    assert!(
        noted[0].starts_with("art/scenery/scenery.lst: 5,001 rows"),
        "{noted:?}"
    );
    assert!(host.assets.take_art_diagnostics().is_empty());
    assert!(
        diagnostics(&mut host)
            .iter()
            .all(|line| !line.contains("scenery.lst"))
    );
}

#[test]
fn extended_art_a_save_keeps_the_identifier_and_loads_without_the_layer() {
    let dir = corpus();
    for map in ["maps/tc.map", "maps/plain.map"] {
        let pid = if map == "maps/tc.map" {
            TC_PID
        } else {
            BASE_SCENERY
        };
        let mut live = host(mount(dir.path(), Some("tc")), Some(map));
        // Load A of the live world, then load B of load A and load C of
        // load B: the identifier bit for bit, and one hash. (A live world
        // and its first load differ in hash in every session, stock ones
        // included; SPEC-021's audit compares loads.)
        let mut first = host(mount(dir.path(), Some("tc")), None);
        load_into(&mut live, &mut first);
        let object = find(&first, pid);
        assert_eq!(art_of(&first, object), EXTENDED, "{map}");
        assert!(first.assets.art_exists(EXTENDED), "{map}");
        let hash = first.world.state_hash();
        let mut second = host(mount(dir.path(), Some("tc")), None);
        load_into(&mut first, &mut second);
        assert_eq!(art_of(&second, find(&second, pid)), EXTENDED, "{map}");
        assert_eq!(second.world.state_hash(), hash, "{map}: load B");
        let mut third = host(mount(dir.path(), Some("tc")), None);
        load_into(&mut second, &mut third);
        assert_eq!(third.world.state_hash(), hash, "{map}: load C");
    }
    // The base alone: a save whose object of a base prototype holds an
    // extended identifier loads; with no list past 4,096 rows bit 31 counts
    // for nothing, the identifier stays as saved, and it has no file.
    let mut live = host(mount(dir.path(), Some("tc")), Some("maps/plain.map"));
    let mut bare = host(mount(dir.path(), None), None);
    load_into(&mut live, &mut bare);
    let object = find(&bare, BASE_SCENERY);
    assert_eq!(art_of(&bare, object), EXTENDED);
    assert!(!bare.assets.art_extended(EXTENDED));
    assert!(!bare.assets.art_exists(EXTENDED));
    assert!(bare.assets.object_art(EXTENDED).is_err());
    assert_eq!(bare.assets.art_fid(2, ROW), None);
    // A save of the layer's own map holds objects of a prototype only the
    // layer registers: without it the load fails cleanly, never panics.
    let mut live = host(mount(dir.path(), Some("tc")), Some("maps/tc.map"));
    let mut bare = host(mount(dir.path(), None), None);
    let mut image = live.slot_image(&SaveInfo::default()).unwrap();
    let error = bare.load_slot(&mut image).unwrap_err();
    assert!(error.to_string().contains("0x2001388"), "{error}");
}

/// A base of three item rows and one critter row, under a layer `vc` that
/// registers item row 4100 (brief vector C).
fn vector_c() -> (tempfile::TempDir, Host) {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base");
    for folder in ["art/items", "art/critters"] {
        std::fs::create_dir_all(base.join(folder)).unwrap();
    }
    std::fs::write(base.join("art/items/items.lst"), art::art_list("i", 3)).unwrap();
    std::fs::write(base.join("art/critters/critters.lst"), b"sample,0\n").unwrap();
    let layer = dir.path().join("mods/vc");
    std::fs::create_dir_all(&layer).unwrap();
    std::fs::write(
        layer.join("vault-mod.toml"),
        art::manifest("VC", &[("items", "tc_a.frm", 4100)], &[], &[]),
    )
    .unwrap();
    let assets = VfsWorldAssets::new(mount(dir.path(), Some("vc")), "english").unwrap();
    let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    (dir, WorldHost::new(world, assets))
}

#[test]
fn extended_art_mod_art_fid_gives_the_canonical_identifier() {
    let (_dir, mut host) = vector_c();
    let words = WordSet::FALLOUT1.with_mods(true);
    let mut art_fid = |category: i32, row: i32| {
        run_as(&mut host, &[I(category), I(row), W(ART_FID)], &words, 70).ints()
    };
    assert_eq!(art_fid(0, 4100), [0x8000_1004_u32 as i32], "vector C");
    assert_eq!(art_fid(0, 2), [0x0000_0002], "a row below 4096: plain");
    assert_eq!(art_fid(0, 3), [0x0000_0003], "an empty gap row");
    assert_eq!(art_fid(0, 4101), [-1], "past the resolved list");
    assert_eq!(art_fid(1, 0), [0x0100_0000], "a critter row");
    assert_eq!(art_fid(1, 4096), [-1], "critters stay capped");
    assert_eq!(art_fid(11, 0), [-1], "no such category");
    assert_eq!(art_fid(256, 0), [-1]);
    assert_eq!(art_fid(0, -1), [-1]);
    assert_eq!(art_fid(2, 0), [-1], "a category without a list");
    // Vector C's resolved list: 4,101 rows, rows 3-4099 empty.
    let catalog = host.assets.art_catalog().unwrap();
    assert_eq!(catalog.list_len(0), 4101);
    assert_eq!(catalog.row_name(0, 4100), Some("tc_a.frm"));
    assert_eq!(catalog.row_name(0, 3), None);
}

#[test]
fn extended_art_mod_art_fid_is_in_the_set_only_with_manifest_scripting() {
    let load = |set: &WordSet| {
        let mut bytes = fixture::program(&[], &[(b"body", ART_FID.to_be_bytes().to_vec())]);
        bytes.extend(0x8000u16.to_be_bytes());
        IntProgram::parse_in(&bytes, set, false).is_ok()
    };
    for (set, loads) in [
        (WordSet::FALLOUT1, false),
        (WordSet::FALLOUT2, false),
        (WordSet::FALLOUT2_SFALL, false),
        (WordSet::FALLOUT1.with_mods(true), true),
        (WordSet::FALLOUT2.with_mods(true), true),
        (WordSet::FALLOUT2_SFALL.with_mods(true), true),
    ] {
        assert_eq!(load(&set), loads, "{set:?}");
    }
    // The stack model's row: two pops, one push.
    let table = stack_check::Table::new(WordSet::FALLOUT2.with_mods(true));
    let row = table.row(ART_FID).unwrap();
    assert_eq!((row.name, row.operands.len()), ("mod_art_fid", 2));
    assert!(
        stack_check::Table::new(WordSet::FALLOUT2)
            .row(ART_FID)
            .is_none()
    );
}

/// A launch over the state corpus and a layer `bad` with `manifest` and
/// `files`: the error, if the launch is refused.
fn launch(manifest: &str, files: &[(&str, &[u8])]) -> Option<String> {
    let dir = tempfile::tempdir().unwrap();
    fixture::write_corpus(&dir.path().join("base"));
    let layer = dir.path().join("mods/bad");
    std::fs::create_dir_all(&layer).unwrap();
    std::fs::write(layer.join("vault-mod.toml"), manifest).unwrap();
    for (path, bytes) in files {
        let path = layer.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    VfsWorldAssets::new(mount(dir.path(), Some("bad")), "english")
        .err()
        .map(|error| error.to_string())
}

#[test]
fn extended_art_vector_d_registrations_inside_the_served_lists_stop_the_launch() {
    let refused = |manifest: String, files: &[(&str, &[u8])], needle: &str| {
        let error = launch(&manifest, files).unwrap_or_else(|| panic!("accepted: {manifest}"));
        assert!(error.contains("mod registration: \"Bad\""), "{error}");
        assert!(error.contains(needle), "{needle}: {error}");
    };
    let pro = vault_fixtures::world::prototype(2, 0);
    // `[[art]] index = 2` over a three-row list.
    let three = art::art_list("i", 3);
    refused(
        art::manifest("Bad", &[("items", "a.frm", 2)], &[], &[]),
        &[("art/items/items.lst", three.as_bytes())],
        "[[art]] items row 2 is inside art/items/items.lst's 3 rows",
    );
    // The corpus prototype lists have one entry: index 1 is inside, and a
    // file the VFS does not serve is refused too.
    refused(
        art::manifest("Bad", &[], &[("scenery", "a.pro", 1)], &[]),
        &[("proto/scenery/a.pro", &pro)],
        "[[protos]] scenery prototype 1 is at or inside proto/scenery/scenery.lst's 1 entries",
    );
    refused(
        art::manifest("Bad", &[], &[("scenery", "absent.pro", 2)], &[]),
        &[],
        "[[protos]] scenery prototype 2: proto/scenery/absent.pro is not served",
    );
    // A tile remap: a missing sidecar, a bad one, and a map not served.
    let remap = |file: &str| art::manifest("Bad", &[], &[], &[("bad.map", file)]);
    let map = fixture::interactive_map();
    refused(
        remap("maps/bad.tlx"),
        &[("maps/bad.map", &map)],
        "[[tile_remap]] of map bad.map: sidecar maps/bad.tlx is not served",
    );
    let mut wrong = art::tile_remap(&[(4000, 5000)]);
    wrong[0] = b'X';
    refused(
        remap("maps/bad.tlx"),
        &[("maps/bad.map", &map), ("maps/bad.tlx", &wrong)],
        "[[tile_remap]] of map bad.map: sidecar maps/bad.tlx:",
    );
    let blank = art::tile_remap(&[(1, 5000)]);
    refused(
        remap("maps/bad.tlx"),
        &[("maps/bad.map", &map), ("maps/bad.tlx", &blank)],
        "sidecar maps/bad.tlx:",
    );
    let good = art::tile_remap(&[(4000, 5000)]);
    refused(
        remap("maps/bad.tlx"),
        &[("maps/bad.tlx", &good)],
        "[[tile_remap]] of map bad.map: maps/bad.map is not served",
    );
    // A registered art row needs no file (a row with a name and no file
    // is not drawn), and a row past the served lists is accepted.
    let accepted = art::manifest(
        "Bad",
        &[("scenery", "nofile.frm", 4096)],
        &[("scenery", "a.pro", 2)],
        &[],
    );
    assert_eq!(launch(&accepted, &[("proto/scenery/a.pro", &pro)]), None);
}

#[test]
fn extended_art_vector_e_a_registered_tile_remap_and_its_rows() {
    let dir = tempfile::tempdir().unwrap();
    fixture::write_corpus(&dir.path().join("base"));
    let layer = dir.path().join("mods/tiles");
    std::fs::create_dir_all(layer.join("maps")).unwrap();
    std::fs::create_dir_all(layer.join("art/tiles")).unwrap();
    std::fs::write(
        layer.join("vault-mod.toml"),
        art::manifest(
            "Tiles",
            &[("tiles", "tc_tile.frm", 5000)],
            &[],
            &[("TC.MAP", "maps/tc.tlx")],
        ),
    )
    .unwrap();
    std::fs::write(layer.join("maps/tc.map"), fixture::interactive_map()).unwrap();
    std::fs::write(layer.join("maps/tc.tlx"), art::tile_remap(&[(4000, 5000)])).unwrap();
    let assets = VfsWorldAssets::new(mount(dir.path(), Some("tiles")), "english").unwrap();
    let remap = assets.tile_remap("maps/TC.map").unwrap().unwrap();
    assert_eq!(remap.entries(), [(4000, 5000)]);
    assert_eq!((remap.row(0x0fa0), remap.row(0x0fa1)), (5000, 4001));
    assert_eq!(assets.tile_remap("tc.map").unwrap(), Some(remap));
    assert_eq!(assets.tile_remap("synthetic.map").unwrap(), None);
    assert_eq!(
        assets.tile_row_path(5000).as_deref(),
        Some("art/tiles/tc_tile.frm")
    );
    assert_eq!(
        assets.tile_row_path(1).as_deref(),
        Some("art/tiles/blank.frm")
    );
    assert_eq!(assets.tile_row_path(4000), None, "an empty gap row");
    assert_eq!(assets.tile_row_path(5001), None, "past the list");
    // Without the layer: no remap, and rows only from the file.
    let base = VfsWorldAssets::new(mount(dir.path(), None), "english").unwrap();
    assert_eq!(base.tile_remap("tc.map").unwrap(), None);
    assert_eq!(base.tile_row_path(5000), None);
}

/// A Fallout 2 world over the `tc` layer on `maps/tc.map`.
fn fallout2_host(dir: &Path) -> Host {
    let mut host = host(mount(dir, Some("tc")), Some("maps/tc.map"));
    host.world.fallout2 = Some(Box::new(Fallout2World::new_game(
        crate::spec017_tests::tables(),
        -1,
    )));
    host
}
/// `metarule3(107, object, number, 0)`, `art_change_fid_num`.
fn change_art(host: &mut Host, object: ObjectRef, number: u32) {
    let ops = [I(107), I(object as i32), I(number as i32), I(0), W(0x80e1)];
    assert_eq!(run_as(host, &ops, &WordSet::FALLOUT2, 60).ints(), [0]);
}

#[test]
fn extended_art_metarule3_107_takes_an_extended_row_only_for_non_critters() {
    let dir = corpus();
    let mut host = fallout2_host(dir.path());
    let object = find(&host, TC_PID);
    // A plain argument keeps today's low-12-bit result, every other field
    // kept.
    host.world.get_mut(object).unwrap().raw.art_id = 0x0203_0000;
    change_art(&mut host, object, 0x0200_0005);
    assert_eq!(art_of(&host, object), 0x0203_0005);
    // An extended argument gives a scenery object the row it addresses.
    change_art(&mut host, object, EXTENDED);
    assert_eq!(art_of(&host, object), EXTENDED);
    assert!(host.assets.art_exists(art_of(&host, object)));
    // An extended form of a row below 4096 gives that row's plain form.
    change_art(&mut host, object, 0x8200_0000);
    assert_eq!(art_of(&host, object), 0x0200_0000);
    // A row the object's own list lacks leaves the art, with a diagnostic.
    diagnostics(&mut host);
    change_art(&mut host, object, 0x8200_1389);
    assert_eq!(art_of(&host, object), 0x0200_0000);
    assert_eq!(diagnostics(&mut host).len(), 1);
    // A critter keeps the low 12 bits, exactly as before.
    let player = host.world.player;
    let critter = host
        .world
        .iter()
        .find(|(reference, object)| {
            *reference != player && object.category() == vault_formats::pro::Category::Critter
        })
        .map(|(reference, _)| reference)
        .expect("the map's critter");
    host.world.get_mut(critter).unwrap().raw.art_id = 0x0100_4003;
    change_art(&mut host, critter, EXTENDED);
    assert_eq!(art_of(&host, critter), 0x0100_4388);
    // So does the player, through the default player art.
    host.world.get_mut(player).unwrap().raw.art_id = 0x0100_400b;
    change_art(&mut host, player, EXTENDED);
    assert_eq!(host.world.default_player_art, 0x388);
    assert_eq!(art_of(&host, player), 0x0100_4388);
}

#[test]
fn extended_art_action_frames_and_death_art_keep_an_extended_identifier() {
    let dir = corpus();
    let mut host = host(mount(dir.path(), Some("tc")), Some("maps/tc.map"));
    let object = find(&host, TC_PID);
    // 0x813A `anim_action_frame(object, animation)` reads the object's own
    // art: the layer frame's action frame, not row 904's.
    let ops = [I(object as i32), I(0), W(0x813a)];
    let run = run_as(&mut host, &ops, &WordSet::FALLOUT1, 61);
    assert_eq!(run.ints(), [i32::from(ACTION_FRAME)]);
    assert_eq!(art_of(&host, object), EXTENDED);
    // The kill path on a non-critter: no animation bits to change.
    host.kill_critter(object, 20).unwrap();
    assert_eq!(art_of(&host, object), EXTENDED);
    // A plain identifier still takes the animation.
    let plain = find(&host, 0x0300_0001);
    let before = art_of(&host, plain);
    host.kill_critter(plain, 20).unwrap();
    assert_eq!(art_of(&host, plain), (before & !0x00ff_0000) | (20 << 16));
}

#[test]
fn extended_art_sound_names_read_the_wide_row() {
    let dir = corpus();
    let host = host(mount(dir.path(), Some("tc")), Some("maps/tc.map"));
    let object = find(&host, TC_PID);
    let name = host.character_sound_name(object, 0, 0).unwrap();
    assert!(
        name.to_ascii_uppercase().starts_with(b"TC_ROCK"),
        "{}",
        String::from_utf8_lossy(&name)
    );
}

/// A host over the state corpus whose critter list has 5,000 rows, with
/// both jumpsuits at row 4500, past what a critter identifier addresses.
fn long_critter_list() -> (tempfile::TempDir, Host) {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base");
    fixture::write_corpus(&base);
    let mut rows = vec!["sample,0"; 5000];
    rows[4500] = "hmjmps,0";
    rows[4501] = "hfjmps,0";
    std::fs::write(
        base.join("art/critters/critters.lst"),
        rows.join("\n") + "\n",
    )
    .unwrap();
    let host = host(mount(dir.path(), None), None);
    (dir, host)
}

#[test]
fn extended_art_a_critter_row_past_4095_is_refused_not_wrapped() {
    let (_dir, mut host) = long_critter_list();
    let player = host.world.player;
    assert_eq!(host.assets.art_row(1, b"hmjmps"), Some(4500));
    host.world.get_mut(player).unwrap().raw.art_id = 0x0100_0000;
    let default = host.world.default_player_art;
    diagnostics(&mut host);
    // The gender update: the art and the default player art stay.
    host.update_player_gender();
    assert_eq!(art_of(&host, player), 0x0100_0000);
    assert_eq!(host.world.default_player_art, default);
    let lines = diagnostics(&mut host);
    assert!(
        lines
            .iter()
            .any(|line| line.contains("critter row 4500 does not fit")),
        "{lines:?}"
    );
    // The start model of SPEC-024c §3.8: refused, not ORed past the base.
    host.models.start = [Some("hfjmps".into()), Some("hfjmps".into())];
    host.apply_start_model();
    assert_eq!(art_of(&host, player), 0x0100_0000);
    let lines = diagnostics(&mut host);
    assert!(
        lines
            .iter()
            .any(|line| line.contains("critter row 4501 of hfjmps does not fit")),
        "{lines:?}"
    );
    // A default player art past 4095 is never ORed in either.
    host.world.default_player_art = 4500;
    host.unworn_art(player);
    assert_eq!(art_of(&host, player), 0x0100_0000);
}

#[test]
fn extended_art_a_stock_session_has_no_extended_identifiers() {
    // No rows and no list past 4,096: bit 31 counts for nothing, and
    // `mod_art_fid` gives the plain form of the rows the lists hold.
    let dir = tempfile::tempdir().unwrap();
    fixture::write_corpus(&dir.path().join("base"));
    let mut host = host(mount(dir.path(), None), Some("maps/synthetic.map"));
    assert!(host.assets.take_art_diagnostics().is_empty());
    assert!(!host.assets.art_extended(0x8200_0000));
    assert_eq!(host.assets.art_fid(2, 0), Some(0x0200_0000));
    assert_eq!(host.assets.art_fid(2, 1), None);
    let words = WordSet::FALLOUT1.with_mods(true);
    let run = run_as(&mut host, &[I(2), I(0), W(ART_FID)], &words, 71);
    assert_eq!(run.ints(), [0x0200_0000]);
    assert_eq!(host.assets.art_row_token(2, 0), host.assets.art_token(2, 0));
    assert!(host.assets.tile_remap("synthetic.map").unwrap().is_none());
}
