//! SPEC-008 §2.4/§2.9/§3.3: working saved maps, automap capture, and
//! save-slot round trips through the synthetic world corpus.
use crate::{
    CharacterSheet, Perk, WorldHost, WorldState,
    assets::{VfsWorldAssets, WorldAssets},
    combat_state::{CombatProgress, NoAi},
    fixture_state as fixture,
    host_save::{SaveInfo, slot_directory},
    queue::EventKind,
};
use std::{fs, path::Path, sync::Arc};
use vault_formats::{
    LocalDirProvider, MapFile, ParseError, map::ObjectState, pro::BasicFlags, savegame::SaveHeader,
};

/// The interactive fixture map as fresh `synth.map` (map script, number 7)
/// and `second.map` (no map script, number 8), both with 8.3 names.
fn corpus() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fixture::write_corpus(dir.path());
    for (name, script, number) in [("synth", 1, 7), ("second", 0, 8)] {
        let mut map = fixture::interactive_map();
        fixture::world::word(&mut map, 0x24, script);
        fixture::world::word(&mut map, 0x28, 0);
        fixture::world::word(&mut map, 0x34, number);
        fs::write(dir.path().join(format!("maps/{name}.map")), map).unwrap();
    }
    dir
}
fn host(dir: &Path) -> WorldHost<VfsWorldAssets> {
    let assets =
        VfsWorldAssets::new(Arc::new(LocalDirProvider::mount(dir).unwrap()), "english").unwrap();
    let mut host = WorldHost::new(
        WorldState::new(CharacterSheet::default(), 1).unwrap(),
        assets,
    );
    host.load_campaign().unwrap();
    host
}
fn parse_saved(host: &WorldHost<VfsWorldAssets>, name: &str) -> MapFile {
    let lookup = |pid| {
        host.assets
            .prototype(pid)
            .map(|p| p.info())
            .map_err(|error| ParseError {
                format: "PRO",
                offset: 0,
                message: error.to_string(),
            })
    };
    MapFile::parse(&host.world.persistence.maps[name], &lookup).unwrap()
}
fn queue_summary(host: &WorldHost<VfsWorldAssets>) -> Vec<(u64, i32, i64, EventKind)> {
    host.world
        .queue
        .events()
        .map(|event| {
            let owner = host
                .world
                .get(event.object)
                .map_or(-2, |o| i64::from(o.raw.id));
            (event.due, event.kind.type_number(), owner, event.kind)
        })
        .collect()
}
/// Fixture objects by prototype and hex (identities shift on import when
/// they collide with the player's).
fn find(host: &WorldHost<VfsWorldAssets>, pid: u32, hex: Option<i32>) -> crate::ObjectRef {
    host.world
        .iter()
        .find(|(_, o)| o.raw.pid == pid && (hex.is_none() || o.raw.hex == hex))
        .map(|(reference, _)| reference)
        .unwrap()
}
const ITEM: u32 = 0x0000_0001;
const CRITTER: u32 = 0x0100_0001;
const SCENERY: u32 = 0x0200_0001;
const WALL: u32 = 0x0300_0001;

fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn spec008_left_maps_join_the_working_saved_set_with_automap_capture() {
    let dir = corpus();
    let mut host = host(dir.path());
    host.load_map("maps/synth.map").unwrap();
    assert_eq!(
        host.world.persistence.current_map.as_deref(),
        Some("SYNTH.SAV")
    );
    assert_eq!(host.world.mvars, [4, -1], "fresh maps take the .GAM values");
    host.world.mvars[0] = 99;
    let wall = find(&host, WALL, None);
    host.world.get_mut(wall).unwrap().raw.flags.0 |= BasicFlags::SEEN.0;
    let scenery = find(&host, SCENERY, None);
    let scenery_id = host.world.get(scenery).unwrap().raw.id;
    host.world.move_object(scenery, 20500, 0, false).unwrap();
    host.world.clock.set(5000);
    host.load_map("maps/second.map").unwrap();

    let saved = parse_saved(&host, "SYNTH.SAV");
    assert_eq!(saved.header.flags.0 & 1, 1, "saved-state bit");
    assert_eq!(saved.header.last_visit, 5000);
    assert_eq!(saved.mvars, [99, -1]);
    // Placed map objects only: no player, no hidden temporary script owner.
    assert_eq!(saved.objects.total, 5);
    let moved = saved.objects.elevations[0]
        .iter()
        .find(|object| object.id == scenery_id)
        .unwrap();
    assert_eq!(moved.hex, Some(20500));
    // The critter script persists; the per-load map script does not.
    assert_eq!(saved.scripts[4].active().count(), 1);
    assert_eq!(saved.scripts[0].active().count(), 0);
    // Seen wall at 20101: X = 99, byte 24 + 50 x 100, shift 0.
    let raster = host
        .world
        .persistence
        .automap
        .raster(7, 0)
        .unwrap()
        .unwrap();
    assert_eq!(raster[5024] & 3, 1);
    assert_eq!(raster.iter().filter(|byte| **byte != 0).count(), 1);
    // SPEC-014 §3.35: leaving captures only the player's elevation.
    assert!(
        host.world
            .persistence
            .automap
            .raster(7, 1)
            .unwrap()
            .is_none()
    );

    // Returning restores from the working save: embedded values are kept.
    host.load_map("maps/synth.map").unwrap();
    assert!(host.world.persistence.maps.contains_key("SECOND.SAV"));
    assert_eq!(host.world.mvars, [99, -1]);
    let scenery = host.world.reference_for_id(scenery_id).unwrap();
    assert_eq!(host.world.get(scenery).unwrap().raw.pid, SCENERY);
    assert_eq!(host.world.get(scenery).unwrap().raw.hex, Some(20500));
    let system = host.world.scripts.keys().filter(|sid| **sid >> 24 == 0);
    assert_eq!(system.count(), 1, "one map script after re-entry");
    assert!(host.world.scripts.contains_key(&0x0400_0001));
}

#[test]
fn spec008_save_slot_round_trip_restores_the_world() {
    let dir = corpus();
    let root = tempfile::tempdir().unwrap();
    let mut host = host(dir.path());
    host.load_map("maps/second.map").unwrap();
    host.load_map("maps/synth.map").unwrap();
    let player = host.world.player;
    host.world.move_object(player, 20400, 0, false).unwrap();
    let sheet = host.world.player_sheet_mut().unwrap();
    sheet.name = b"Saver".to_vec();
    sheet.invested[3] = 12;
    sheet.tags = [3, 8, -1, -1];
    sheet.traits = [1, -1];
    sheet.level = 3;
    sheet.experience = 3100;
    sheet.karma = 25;
    sheet.perks.insert(Perk::Awareness, 1);
    sheet.current_hp = 17;
    host.world.kills[4] = 2;
    host.world.gvars.as_mut().unwrap()[0] = 1234;
    host.world.clock.set(3 * 864000 + 600);
    let critter = find(&host, CRITTER, None);
    let critter_id = host.world.get(critter).unwrap().raw.id;
    host.world.party.insert(critter);
    let item = find(&host, ITEM, Some(20300));
    let item_id = host.world.get(item).unwrap().raw.id;
    host.world.add_item(player, item, 1, false).unwrap();
    let now = host.world.clock.ticks;
    for (delay, object, kind) in [
        (600, critter, EventKind::Knockout),
        (
            900,
            0,
            EventKind::Drug {
                stats: [1, 2, 3],
                modifiers: [-1, -2, -3],
            },
        ),
        (
            1200,
            player,
            EventKind::Radiation {
                level: 3,
                healing: 1,
            },
        ),
        (1500, 0, EventKind::Other(9)),
    ] {
        host.world.queue.insert(now, delay, object, kind).unwrap();
    }
    host.world.world_map.set_position([300, 400]);
    host.world.world_map.saved.discovery[300] = 2;
    host.world.persistence.automap_flags = 0x5;
    host.world.persistence.movies[3] = 1;
    host.world.difficulty = 2;
    let queue = queue_summary(&host);
    let info = SaveInfo {
        description: b"Round trip".to_vec(),
        host_date: [17, 10, 2026],
        host_time: 12 + 34,
        thumbnail: None,
    };
    host.save_game(root.path(), 1, &info).unwrap();
    let slot = slot_directory(root.path(), 1).unwrap();
    assert!(slot.ends_with("SAVEGAME/SLOT01"));
    assert_eq!(
        files(&slot),
        ["AUTOMAP.SAV", "SAVE.DAT", "SECOND.SAV", "SYNTH.SAV"]
    );
    let central = fs::read(slot.join("SAVE.DAT")).unwrap();
    let header = SaveHeader::parse(&central).unwrap();
    assert_eq!(header.name_text(), b"Saver");
    assert_eq!(header.description_text(), b"Round trip");
    assert_eq!(header.map_file_text(), b"SYNTH.SAV");
    assert_eq!(
        (header.game_ticks, header.map, header.host_time),
        (now as u32, 7, 46)
    );
    assert_eq!(header.game_date[..2], [1, 4]);
    // The automap companion length is recorded in the manifest component.
    assert_eq!(
        fs::read(slot.join("AUTOMAP.SAV")).unwrap(),
        host.world.persistence.automap.bytes()
    );

    // Scramble live state, then restore.
    host.world.gvars.as_mut().unwrap()[0] = 0;
    host.load_map("maps/second.map").unwrap();
    host.world.clock.set(5);
    host.world.kills = [0; 15];
    host.load_game(root.path(), 1).unwrap();

    let world = &host.world;
    assert_eq!(world.persistence.current_map.as_deref(), Some("SYNTH.SAV"));
    assert_eq!(world.gvars.as_ref().unwrap()[0], 1234);
    assert_eq!(world.clock.ticks, now);
    assert_eq!(world.kills[4], 2);
    assert_eq!(world.difficulty, 2);
    assert_eq!(world.persistence.automap_flags, 5);
    assert_eq!(world.persistence.movies[3], 1);
    assert_eq!(world.world_map.position(), [300, 400]);
    assert_eq!(world.world_map.saved.discovery[300], 2);
    let player = world.player;
    let object = world.get(player).unwrap();
    assert_eq!(object.raw.hex, Some(20400));
    assert_eq!(object.name, b"Saver");
    let sheet = object.character.as_ref().unwrap();
    assert_eq!(sheet.invested[3], 12);
    assert_eq!((sheet.tags, sheet.traits), ([3, 8, -1, -1], [1, -1]));
    assert_eq!((sheet.level, sheet.experience, sheet.karma), (3, 3100, 25));
    assert_eq!(sheet.rank(Perk::Awareness), 1);
    assert_eq!(sheet.current_hp, 17);
    let carried: Vec<_> = object
        .inventory
        .iter()
        .map(|(child, quantity)| (world.get(*child).unwrap().raw.id, *quantity))
        .collect();
    assert_eq!(carried, [(item_id, 1)]);
    let carried = world.get(object.inventory[0].0).unwrap();
    assert_eq!((carried.raw.pid, carried.owner), (ITEM, Some(player)));
    let critter = world.reference_for_id(critter_id).unwrap();
    assert_eq!(world.get(critter).unwrap().raw.pid, CRITTER);
    assert!(world.party.contains(&critter));
    assert!(!world.combat);
    assert_eq!(queue_summary(&host), queue);

    // A second save replaces the slot without leaving backups.
    host.load_map("maps/second.map").unwrap();
    host.save_game(root.path(), 1, &info).unwrap();
    assert_eq!(
        files(&slot),
        ["AUTOMAP.SAV", "SAVE.DAT", "SECOND.SAV", "SYNTH.SAV"]
    );
    let header = SaveHeader::parse(&fs::read(slot.join("SAVE.DAT")).unwrap()).unwrap();
    assert_eq!(header.map_file_text(), b"SECOND.SAV");
}

#[test]
fn spec008_slot_loading_validates_companions_before_changing_state() {
    let dir = corpus();
    let root = tempfile::tempdir().unwrap();
    let mut host = host(dir.path());
    assert!(slot_directory(root.path(), 0).is_err());
    assert!(slot_directory(root.path(), 11).is_err());
    assert_eq!(
        slot_directory(root.path(), 10).unwrap(),
        root.path().join("SAVEGAME/SLOT10")
    );
    host.load_map("maps/synth.map").unwrap();
    host.save_game(root.path(), 2, &SaveInfo::default())
        .unwrap();
    let slot = slot_directory(root.path(), 2).unwrap();
    host.load_map("maps/second.map").unwrap();
    host.world.gvars.as_mut().unwrap()[0] = 77;
    let check_unchanged = |host: &WorldHost<VfsWorldAssets>| {
        assert_eq!(host.world.gvars.as_ref().unwrap()[0], 77);
        assert_eq!(
            host.world.persistence.current_map.as_deref(),
            Some("SECOND.SAV")
        );
    };
    // A wrong automap length.
    let automap = fs::read(slot.join("AUTOMAP.SAV")).unwrap();
    fs::write(slot.join("AUTOMAP.SAV"), &automap[..automap.len() - 1]).unwrap();
    assert!(host.load_game(root.path(), 2).is_err());
    check_unchanged(&host);
    // A missing automap or map companion.
    fs::remove_file(slot.join("AUTOMAP.SAV")).unwrap();
    assert!(host.load_game(root.path(), 2).is_err());
    fs::write(slot.join("AUTOMAP.SAV"), &automap).unwrap();
    let map = fs::read(slot.join("SYNTH.SAV")).unwrap();
    fs::remove_file(slot.join("SYNTH.SAV")).unwrap();
    assert!(host.load_game(root.path(), 2).is_err());
    check_unchanged(&host);
    // A truncated companion map is rejected before any state changes.
    fs::write(slot.join("SYNTH.SAV"), &map[..map.len() - 4]).unwrap();
    assert!(host.load_game(root.path(), 2).is_err());
    check_unchanged(&host);
    fs::remove_file(slot.join("SYNTH.SAV")).unwrap();
    // Companion names match case-insensitively.
    fs::write(slot.join("synth.sav"), &map).unwrap();
    // A truncated central file.
    let central = fs::read(slot.join("SAVE.DAT")).unwrap();
    fs::write(slot.join("SAVE.DAT"), &central[..central.len() - 3]).unwrap();
    assert!(host.load_game(root.path(), 2).is_err());
    check_unchanged(&host);
    fs::write(slot.join("SAVE.DAT"), &central).unwrap();
    host.load_game(root.path(), 2).unwrap();
    assert_eq!(
        host.world.persistence.current_map.as_deref(),
        Some("SYNTH.SAV")
    );
    assert!(host.load_game(root.path(), 3).is_err());
}

#[test]
fn spec008_failed_slot_write_leaves_the_old_slot_untouched() {
    let dir = corpus();
    let root = tempfile::tempdir().unwrap();
    let mut host = host(dir.path());
    host.load_map("maps/second.map").unwrap();
    host.load_map("maps/synth.map").unwrap();
    host.save_game(root.path(), 4, &SaveInfo::default())
        .unwrap();
    let slot = slot_directory(root.path(), 4).unwrap();
    let snapshot = |slot: &std::path::Path| {
        let mut files: Vec<_> = fs::read_dir(slot)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_file())
            .map(|path| (path.clone(), fs::read(path).unwrap()))
            .collect();
        files.sort();
        files
    };
    let before = snapshot(&slot);
    // SECOND.SAV.TMP is a directory, so staging that companion fails; the
    // safe-rule writer then leaves every committed file byte-identical.
    fs::create_dir_all(slot.join("SECOND.SAV.TMP/blocker")).unwrap();
    host.world.clock.set(999_999);
    assert!(
        host.save_game(root.path(), 4, &SaveInfo::default())
            .is_err()
    );
    assert_eq!(snapshot(&slot), before);
    // Once staging succeeds, the slot is replaced and no backups remain.
    fs::remove_dir_all(slot.join("SECOND.SAV.TMP")).unwrap();
    host.save_game(root.path(), 4, &SaveInfo::default())
        .unwrap();
    let central = before
        .iter()
        .find(|(path, _)| path.ends_with("SAVE.DAT"))
        .unwrap();
    assert_ne!(fs::read(slot.join("SAVE.DAT")).unwrap(), central.1);
    assert!(fs::read_dir(&slot).unwrap().all(|entry| {
        entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|e| e != "BAK" && e != "TMP")
    }));
}

#[test]
fn spec008_saved_active_combat_resumes_the_player_turn() {
    let dir = corpus();
    let root = tempfile::tempdir().unwrap();
    let mut host = host(dir.path());
    host.load_map("maps/synth.map").unwrap();
    let player = host.world.player;
    let critter = find(&host, CRITTER, None);
    host.start_combat(player, critter).unwrap();
    assert_eq!(
        host.advance_combat(&mut NoAi).unwrap(),
        CombatProgress::PlayerTurn
    );
    if let Some(ObjectState::Critter(state)) = host.world.get_mut(player).map(|o| &mut o.raw.state)
    {
        state.ap = 3;
    }
    host.world.free_movement = 1;
    host.save_game(root.path(), 5, &SaveInfo::default())
        .unwrap();
    host.end_combat().unwrap();
    host.load_game(root.path(), 5).unwrap();
    let critter = find(&host, CRITTER, None);
    let world = &host.world;
    assert!(world.combat && world.combat_state.resumed);
    assert_eq!(world.combat_state.list, [world.player, critter]);
    assert_eq!(
        (world.combat_state.active, world.combat_state.cursor),
        (2, 0)
    );
    assert_eq!(world.free_movement, 1);
    // The resumed player turn does not refill AP.
    assert_eq!(
        host.advance_combat(&mut NoAi).unwrap(),
        CombatProgress::PlayerTurn
    );
    let ap = host.world.critter_state(host.world.player).unwrap().ap;
    assert_eq!(ap, 3);
}

#[test]
fn spec008_missing_map_script_does_not_trap_map_transitions() {
    let dir = corpus();
    // Map script index 9 has no scripts.lst entry or compiled program.
    let mut map = fixture::interactive_map();
    fixture::world::word(&mut map, 0x24, 10);
    fixture::world::word(&mut map, 0x28, 0);
    fs::write(dir.path().join("maps/broken.map"), map).unwrap();
    let mut host = host(dir.path());
    host.load_map("maps/broken.map").unwrap();
    host.world.effects.clear();
    host.load_map("maps/second.map").unwrap();
    assert_eq!(
        host.world.persistence.current_map.as_deref(),
        Some("SECOND.SAV")
    );
    assert!(host.world.persistence.maps.contains_key("BROKEN.SAV"));
    assert!(host.world.effects.iter().any(|effect| matches!(
        effect,
        crate::WorldEffect::Diagnostic(text) if text.starts_with("map-load script request")
    )));
}

/// A raw-member (`0x20`) archive of the corpus files selected by `keep`, with
/// case-insensitively sorted directory and file keys.
fn raw_dat(root: &Path, keep: impl Fn(&str) -> bool) -> Vec<u8> {
    use std::collections::BTreeMap;
    fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, base, out);
            } else {
                let relative = path.strip_prefix(base).unwrap().to_str().unwrap();
                out.push((relative.replace('\\', "/"), fs::read(&path).unwrap()));
            }
        }
    }
    let mut files = Vec::new();
    walk(root, root, &mut files);
    let mut directories: BTreeMap<String, BTreeMap<String, Vec<u8>>> = BTreeMap::new();
    directories.entry(".".into()).or_default();
    for (path, bytes) in files.into_iter().filter(|(path, _)| keep(path)) {
        let upper = path.to_ascii_uppercase();
        let (directory, name) = upper.rsplit_once('/').unwrap_or((".", &upper));
        directories
            .entry(directory.replace('/', "\\"))
            .or_default()
            .insert(name.to_string(), bytes);
    }
    let word = |out: &mut Vec<u8>, value: usize| out.extend((value as u32).to_be_bytes());
    let tables: usize = 16
        + directories.keys().map(|d| 1 + d.len()).sum::<usize>()
        + directories
            .values()
            .map(|files| 16 + files.keys().map(|n| 17 + n.len()).sum::<usize>())
            .sum::<usize>();
    let mut out = Vec::new();
    for value in [directories.len(), directories.len(), 0, 0] {
        word(&mut out, value);
    }
    for directory in directories.keys() {
        out.push(directory.len() as u8);
        out.extend(directory.as_bytes());
    }
    let mut offset = tables;
    let mut payload: Vec<u8> = Vec::new();
    for files in directories.values() {
        for value in [files.len(), files.len(), 16, 0] {
            word(&mut out, value);
        }
        for (name, bytes) in files {
            out.push(name.len() as u8);
            out.extend(name.as_bytes());
            for value in [0x20, offset, bytes.len(), bytes.len()] {
                word(&mut out, value);
            }
            offset += bytes.len();
            payload.extend(bytes);
        }
    }
    assert_eq!(out.len(), tables);
    out.extend(payload);
    out
}

#[test]
fn spec008_world_host_runs_from_selected_dat_databases() {
    use vault_formats::{DatArchive, DatProvider, Database, Vfs};
    let dir = corpus();
    let archive = |critters: bool| {
        let bytes = raw_dat(dir.path(), |path| {
            path.starts_with("art/critters/") == critters
        });
        Database {
            loose: None,
            archive: Some(Arc::new(DatArchive::parse(bytes).unwrap())),
        }
    };
    let provider = DatProvider::new(archive(false), archive(true));
    // Critter art resolves only through the critter database.
    assert!(provider.read("art/critters/sampleaa.frm").is_ok());
    assert!(provider.master.read("art/critters/sampleaa.frm").is_err());
    assert!(provider.critter.read("maps/synth.map").is_err());
    assert!(
        provider
            .files()
            .any(|path| path.as_str() == "maps/synth.map")
    );
    let assets = VfsWorldAssets::new(Arc::new(provider), "english").unwrap();
    let mut host = WorldHost::new(
        WorldState::new(CharacterSheet::default(), 1).unwrap(),
        assets,
    );
    host.load_campaign().unwrap();
    host.load_map("maps/synth.map").unwrap();
    assert_eq!(host.world.mvars, [4, -1]);
    let critter = find(&host, CRITTER, None);
    let art = host.world.get(critter).unwrap().raw.art_id;
    assert!(host.assets.object_art(art).is_ok());
    host.load_map("maps/second.map").unwrap();
    assert!(host.world.persistence.maps.contains_key("SYNTH.SAV"));
}

#[test]
fn spec008_timed_event_with_a_missing_owner_restores_without_one() {
    let dir = corpus();
    let mut host = host(dir.path());
    host.load_map("maps/synth.map").unwrap();
    let player = host.world.player;
    let now = host.world.clock.ticks;
    host.world
        .queue
        .insert(now, 600, player, EventKind::Other(9))
        .unwrap();
    let mut save = host.build_save(&SaveInfo::default()).unwrap();
    let record = save
        .queue
        .iter_mut()
        .find(|record| record.kind == 9)
        .unwrap();
    record.owner = 999_999;
    let maps = host.world.persistence.maps.clone();
    let automap = host.world.persistence.automap.clone();
    host.apply_save(save, maps, automap).unwrap();
    let restored: Vec<_> = host
        .world
        .queue
        .events()
        .filter(|event| event.kind == EventKind::Other(9))
        .map(|event| (event.due, event.object))
        .collect();
    assert_eq!(restored, [(now + 600, 0)]);
}

#[test]
fn spec014_seen_state_marks_a_window_around_visited_hexes() {
    // SPEC-014 §3.33: a visited hex at the start of its byte (hex 20000,
    // byte 2500) marks bytes 2500 + 25k + d for k -16..=15, d -2..=2 on the
    // player's elevation; the set is then cleared.
    let mut world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let player = world.player;
    world.move_object(player, 20000, 0, false).unwrap();
    let place = |world: &mut WorldState, hex: i32, elevation: u8| {
        let prototype =
            Arc::new(vault_formats::Prototype::parse(&fixture::world::prototype(3, 0)).unwrap());
        world.spawn(prototype, hex, elevation).unwrap()
    };
    let inside = [
        place(&mut world, 20000 + 16, 0),
        place(&mut world, 20000 + 15 * 200, 0),
        place(&mut world, 20000 - 16 * 200, 0),
    ];
    let outside = [
        place(&mut world, 20000 + 24, 0),
        place(&mut world, 20000 + 16 * 200, 0),
        place(&mut world, 20000 - 17 * 200, 0),
        place(&mut world, 20000, 1),
    ];
    assert!(world.visited.iter().any(|byte| *byte != 0));
    world.process_seen();
    let seen = |world: &WorldState, object| {
        world
            .get(object)
            .unwrap()
            .raw
            .flags
            .contains(BasicFlags::SEEN)
    };
    assert!(inside.iter().all(|object| seen(&world, *object)));
    assert!(outside.iter().all(|object| !seen(&world, *object)));
    assert!(world.visited.iter().all(|byte| *byte == 0));
    // A capture processes first, so the flags reach the raster.
    world.mark_visited(39999);
    world.process_seen();
    assert!(world.visited.iter().all(|byte| *byte == 0));
}

/// The corpus plus `desert.map`, map 19: an excluded encounter map.
fn corpus_with_desert() -> tempfile::TempDir {
    let dir = corpus();
    let mut map = fixture::interactive_map();
    fixture::world::word(&mut map, 0x24, 0);
    fixture::world::word(&mut map, 0x28, 0);
    fixture::world::word(&mut map, 0x34, 19);
    fs::write(dir.path().join("maps/desert.map"), map).unwrap();
    dir
}
fn save_info() -> SaveInfo {
    SaveInfo {
        description: b"Working set".to_vec(),
        host_date: [20, 10, 2026],
        host_time: 1,
        thumbnail: None,
    }
}

#[test]
fn spec014_excluded_maps_are_rebuilt_but_a_saved_game_keeps_the_current_one() {
    let dir = corpus_with_desert();
    let root = tempfile::tempdir().unwrap();
    let mut host = host(dir.path());
    host.load_map("maps/synth.map").unwrap();
    host.load_map("maps/desert.map").unwrap();
    assert!(host.world.persistence.maps.contains_key("SYNTH.SAV"));
    // Leaving the desert writes nothing: it comes back from its `.MAP`.
    let scenery = find(&host, SCENERY, None);
    host.world.move_object(scenery, 20500, 0, false).unwrap();
    host.load_map("maps/second.map").unwrap();
    assert!(!host.world.persistence.maps.contains_key("DESERT.SAV"));
    host.load_map("maps/desert.map").unwrap();
    let scenery = find(&host, SCENERY, None);
    assert_eq!(host.world.get(scenery).unwrap().raw.hex, Some(20301));
    // Saving a game writes the current map without leaving it, marked
    // saved with the current time, even though it is excluded.
    host.world.clock.set(7000);
    host.save_game(root.path(), 1, &save_info()).unwrap();
    let saved = parse_saved(&host, "DESERT.SAV");
    assert_eq!(
        (saved.header.flags.0 & 1, saved.header.last_visit),
        (1, 7000)
    );
    let slot = slot_directory(root.path(), 1).unwrap();
    assert!(files(&slot).contains(&"DESERT.SAV".to_string()));
    // Leaving it then deletes that working copy again.
    host.load_map("maps/second.map").unwrap();
    assert!(!host.world.persistence.maps.contains_key("DESERT.SAV"));
    assert!(host.world.persistence.maps.contains_key("SECOND.SAV"));
}

#[test]
fn spec014_revisits_unjam_heal_calm_and_leave_remains() {
    let dir = corpus();
    // Miscellaneous PID 4 is the remains, whose art has nine frames.
    let proto = dir.path().join("proto/misc");
    fs::write(
        proto.join("misc.lst"),
        b"synthetic.pro\nsynthetic.pro\nsynthetic.pro\nremains.pro\n",
    )
    .unwrap();
    let mut remains = fixture::world::prototype(5, 0);
    fixture::world::word(&mut remains, 0, 0x0500_0004);
    fixture::world::word(&mut remains, 8, 0x0500_0001);
    fs::write(proto.join("remains.pro"), remains).unwrap();
    fs::write(
        dir.path().join("art/misc/misc.lst"),
        b"sample.frm\nrscorpbl.frm\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("art/misc/rscorpbl.frm"),
        fixture::animated_frame(4, 4, &[1, 2, 3, 4, 5, 6, 7, 8, 9]),
    )
    .unwrap();
    let mut host = host(dir.path());
    host.load_map("maps/synth.map").unwrap();
    let chest = find(&host, ITEM, Some(20300));
    let jam = |host: &mut WorldHost<VfsWorldAssets>| {
        let object = host.world.get_mut(chest).unwrap();
        let mut lock = object.lock().unwrap();
        lock.0 |= vault_formats::pro::LockFlags::JAMMED.0;
        object.set_lock(lock);
    };
    let jammed = |host: &WorldHost<VfsWorldAssets>| {
        let chest = find(host, ITEM, Some(20300));
        host.world
            .get(chest)
            .and_then(|object| object.lock())
            .is_some_and(|lock| lock.contains(vault_formats::pro::LockFlags::JAMMED))
    };
    let critter = find(&host, CRITTER, None);
    let maximum = host
        .world
        .get(critter)
        .unwrap()
        .character
        .as_ref()
        .unwrap()
        .stat(7, host.world.context(critter));
    let wound = |host: &mut WorldHost<VfsWorldAssets>| {
        host.world.set_hit_points(critter, 2);
        host.world.critter_state_mut(critter).unwrap().maneuvers |=
            crate::critical::maneuver::FLEEING;
    };
    jam(&mut host);
    wound(&mut host);
    // 50 minutes away: nothing changes.
    host.world.clock.set(1000);
    host.load_map("maps/second.map").unwrap();
    host.world.clock.set(1000 + 30_000);
    host.load_map("maps/synth.map").unwrap();
    let critter = find(&host, CRITTER, None);
    assert!(jammed(&host));
    assert_eq!(host.world.hit_points(critter), Some(2));
    // Five hours: no longer fleeing, 14 * trunc(5 / 3) HP healed, still
    // jammed.
    host.load_map("maps/second.map").unwrap();
    let now = host.world.clock.ticks;
    host.world.clock.set(now + 5 * 36_000);
    host.load_map("maps/synth.map").unwrap();
    let critter = find(&host, CRITTER, None);
    assert_eq!(host.world.hit_points(critter), Some(2 + 14));
    assert_eq!(
        host.world.critter_state(critter).unwrap().maneuvers & crate::critical::maneuver::FLEEING,
        0
    );
    assert!(jammed(&host));
    // A day: the lock is unjammed, and healing stops at the maximum.
    host.load_map("maps/second.map").unwrap();
    let now = host.world.clock.ticks;
    host.world.clock.set(now + 24 * 36_000);
    host.load_map("maps/synth.map").unwrap();
    let critter = find(&host, CRITTER, None);
    assert!(!jammed(&host));
    assert_eq!(host.world.hit_points(critter), Some(maximum));
    // Dead for more than six days: remains replace the corpse, which
    // drops what it carried on its hex.
    let item = find(&host, ITEM, Some(20100));
    let item_id = host.world.get(item).unwrap().raw.id;
    host.world.add_item(critter, item, 1, true).unwrap();
    host.world.set_hit_points(critter, 0);
    host.world.critter_state_mut(critter).unwrap().results.0 |= crate::critical::effect::DEAD;
    let corpse_hex = host.world.get(critter).unwrap().raw.hex;
    host.load_map("maps/second.map").unwrap();
    let now = host.world.clock.ticks;
    host.world.clock.set(now + 145 * 36_000);
    host.load_map("maps/synth.map").unwrap();
    assert!(
        host.world
            .iter()
            .all(|(_, object)| object.raw.pid != CRITTER)
    );
    let remains = find(&host, 0x0500_0004, corpse_hex);
    assert!((3..=5).contains(&host.world.get(remains).unwrap().raw.frame));
    let dropped = host.world.reference_for_id(item_id).unwrap();
    let dropped = host.world.get(dropped).unwrap();
    assert_eq!((dropped.owner, dropped.raw.hex), (None, corpse_hex));
}

#[test]
fn spec013_the_world_and_town_maps_save_and_clear_the_outgoing_map() {
    let dir = corpus();
    let mut host = host(dir.path());
    host.load_map("maps/synth.map").unwrap();
    host.world.clock.set(4000);
    // An exit grid holding map 0: the world map.
    host.request_transition(crate::Transition {
        map: 0,
        elevation: 0,
        hex: 20100,
        facing: 2,
    });
    host.world.effects.clear();
    assert!(host.process_transition().unwrap());
    assert!(host.world.unloaded);
    assert_eq!(parse_saved(&host, "SYNTH.SAV").header.last_visit, 4000);
    assert!(host.world.effects.contains(&crate::WorldEffect::WorldMap));
    assert_eq!(host.world.current_map_name(), None);
    assert_eq!(host.world.iter().count(), 1, "only the player remains");
    // Entering a location loads its saved copy.
    host.load_map("maps/synth.map").unwrap();
    assert!(!host.world.unloaded);
    assert_eq!(host.world.current_map_name().as_deref(), Some("SYNTH.SAV"));
    // The town map (-1) saves first too.
    host.request_transition(crate::Transition {
        map: -1,
        elevation: -1,
        hex: -1,
        facing: -1,
    });
    host.world.effects.clear();
    assert!(host.process_transition().unwrap());
    assert!(host.world.unloaded);
    assert!(host.world.effects.contains(&crate::WorldEffect::TownMap));
}

/// A corpus map file with no map script and world-map map number `number`.
fn corpus_map(dir: &Path, name: &str, number: i32) {
    let mut map = fixture::interactive_map();
    fixture::world::word(&mut map, 0x24, 0);
    fixture::world::word(&mut map, 0x28, 0);
    fixture::world::word(&mut map, 0x34, number);
    fs::write(dir.join(format!("maps/{name}.map")), map).unwrap();
}

#[test]
fn spec013_transitions_move_the_world_map_to_the_destination_town() {
    let dir = corpus();
    // Junktown's slot-0 hotspot map (SPEC-008 §3.7).
    corpus_map(dir.path(), "junkent", 9);
    let mut host = host(dir.path());
    host.load_map("maps/synth.map").unwrap();
    let to = |map: i32| crate::Transition {
        map,
        elevation: -1,
        hex: -1,
        facing: -1,
    };
    host.request_transition(to(9));
    assert!(host.process_transition().unwrap());
    assert_eq!(host.world.world_map.saved.town, 3);
    assert_eq!(
        host.world.world_map.position(),
        [17 * 50 + 25, 10 * 50 + 25]
    );
    // A map outside the hotspot catalog keeps the town.
    host.request_transition(to(8));
    assert!(host.process_transition().unwrap());
    assert_eq!(host.world.world_map.saved.town, 3);
}

#[test]
fn spec014_leaving_requeues_the_game_time_event_restores_the_roof_and_starts_city_music() {
    let dir = corpus();
    corpus_map(dir.path(), "v13ent", 35);
    let mut host = host(dir.path());
    host.load_map("maps/synth.map").unwrap();
    let music = |host: &WorldHost<VfsWorldAssets>| -> Vec<String> {
        host.world
            .effects
            .iter()
            .filter_map(|effect| match effect {
                crate::WorldEffect::Music(name) => Some(name.clone()),
                _ => None,
            })
            .collect()
    };
    assert!(music(&host).is_empty(), "map 7 has no music the specs name");
    // A roof square over the player's elevation, hidden.
    host.world.map.tiles[0].as_mut().unwrap()[5050].word |= 0x1000_0000;
    host.world.effects.clear();
    host.load_map("maps/v13ent.map").unwrap();
    assert_eq!(music(&host), ["06vault"]);
    // The saved copy shows the roof again.
    let saved = parse_saved(&host, "SYNTH.SAV");
    assert_eq!(
        saved.tiles[0].as_ref().unwrap()[5050].stored & 0x1000_0000,
        0
    );
    // Leaving for the world map queues the game-time (midnight) event
    // again, so the days go on while the map is cleared.
    host.request_transition(crate::Transition {
        map: -2,
        elevation: -1,
        hex: -1,
        facing: -1,
    });
    assert!(host.process_transition().unwrap());
    assert!(host.world.unloaded);
    assert!(
        host.world
            .queue
            .events()
            .any(|event| event.kind == EventKind::Midnight)
    );
}
