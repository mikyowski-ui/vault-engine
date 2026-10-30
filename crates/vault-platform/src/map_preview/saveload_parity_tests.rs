//! SPEC-023 T13: a save, a quick save and a settings save through
//! `SessionRuntime` write the paths and bytes they wrote before the storage
//! moved behind `StorageBackend`. `RECORDED` was printed by this test on the
//! code before that move. T14 and the session's other storage rules follow.
use crate::{
    config::Config,
    map_preview::{MapPreview, test_support::synthetic_corpus},
    preview::{AssetPreview, PreviewArgs},
    session::{SessionConfig, SessionOrigin, SessionRuntime},
    storage::{FsStorage, MemoryStorage, Namespace, StorageBackend, StorageOp},
};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};
use vault_core::{
    EnginePreset,
    companion::{Distance, SIDECAR},
    host_save::QUICK_SLOT,
    ui::saveload::SlotState,
};
use vault_formats::{ByteSource, MemoryDir, MemoryMount, Vfs};

const RECORDED: &[(&str, &str)] = &[
    (
        "SAVEGAME/QUICKSAV/AUTOMAP.SAV",
        "3ab6cdda5729d36bf26a151f1ea89128",
    ),
    (
        "SAVEGAME/QUICKSAV/SAVE.DAT",
        "059d97515c47198181808cf842298229",
    ),
    (
        "SAVEGAME/QUICKSAV/SYNTH.SAV",
        "cb5c2e95e8c73ff654abb436ca8f90df",
    ),
    (
        "SAVEGAME/QUICKSAV/VAULTENG.TOML",
        "00f6915987735241bfcd2f427f0dcb49",
    ),
    (
        "SAVEGAME/SLOT01/AUTOMAP.SAV",
        "3ab6cdda5729d36bf26a151f1ea89128",
    ),
    (
        "SAVEGAME/SLOT01/SAVE.DAT",
        "16096b12ebc9182a96bdc6071a6fe013",
    ),
    (
        "SAVEGAME/SLOT01/SYNTH.SAV",
        "cb5c2e95e8c73ff654abb436ca8f90df",
    ),
    ("vault-engine.toml", "349f0e7d7f655c1106eef15ab8a74d8d"),
];

/// Every file below `root`: its `/`-separated path and the first half
/// of its SHA-256, sorted.
fn digests(root: &Path) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let relative = path.strip_prefix(root).unwrap().to_str().unwrap();
            let digest = Sha256::digest(std::fs::read(&path).unwrap());
            let hex: String = digest[..16]
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            files.push((relative.replace('\\', "/"), hex));
        }
    }
    files.sort();
    files
}

/// The synthetic corpus with a map whose name fits a slot: a slot's map
/// names are at most twelve bytes (SPEC-008 §2.5).
fn short_map_corpus() -> tempfile::TempDir {
    let corpus = synthetic_corpus();
    std::fs::copy(
        corpus.path().join("maps/synthetic.map"),
        corpus.path().join("maps/synth.map"),
    )
    .unwrap();
    corpus
}

fn session_args(corpus: &Path) -> PreviewArgs {
    PreviewArgs {
        corpus_dir: Some(corpus.to_path_buf()),
        preview_map: Some("maps/synth.map".into()),
        player_gcd: Some("premade/synthetic.gcd".into()),
        headless: true,
        ..Default::default()
    }
}

/// A world of its own over `corpus`, keeping its slots in `storage`.
fn world_on(corpus: &Path, storage: Arc<dyn StorageBackend>) -> MapPreview {
    let mut world = crate::map_preview::test_support::load(corpus, "maps/synth.map");
    world.storage = Some(storage);
    world
}

#[test]
fn t13_saves_and_settings_keep_their_paths_and_bytes() {
    let corpus = short_map_corpus();
    let saves = tempfile::tempdir().unwrap();
    let mut session = SessionRuntime::start(SessionConfig::new(
        SessionOrigin::CommandLine,
        PreviewArgs {
            corpus_dir: Some(corpus.path().to_path_buf()),
            preview_map: Some("maps/synth.map".into()),
            player_gcd: Some("premade/synthetic.gcd".into()),
            save_dir: Some(saves.path().to_path_buf()),
            preset: EnginePreset::Enhanced,
            headless: true,
            ..Default::default()
        },
    ))
    .unwrap()
    .unwrap();
    let AssetPreview::Map(world) = &mut *session else {
        panic!("a map session");
    };
    let saved = world.save_slot(0, b"Parity".to_vec());
    assert!(saved, "{}", world.status.escape_ascii());
    world
        .host
        .world
        .companion_distance
        .insert(18000, Distance::Hold);
    world.enhanced_quick_save();

    let settings = saves.path().join(crate::config::FILE_NAME);
    let config = Config {
        master_dat: Some(saves.path().join("MASTER.DAT")),
        ..Config::default()
    };
    config.save(&settings).unwrap();

    let found = digests(saves.path());
    let shown: Vec<(&str, &str)> = found
        .iter()
        .map(|(path, digest)| (path.as_str(), digest.as_str()))
        .collect();
    assert_eq!(shown, RECORDED, "{shown:#?}");
}

#[test]
fn t14_a_slot_copied_between_backends_loads_to_the_same_state_hash() {
    let corpus = short_map_corpus();
    let memory: Arc<dyn StorageBackend> = Arc::new(MemoryStorage::new());
    let mut saved = world_on(corpus.path(), memory.clone());
    saved.host.world.clock.set(4321);
    assert!(saved.save_slot(0, b"Hash".to_vec()));
    let keys = memory.list(Namespace::Savegame, None).unwrap();
    assert!(keys.contains(&"slot01/save.dat".to_owned()), "{keys:?}");

    let temp = tempfile::tempdir().unwrap();
    let file_system: Arc<dyn StorageBackend> = Arc::new(FsStorage::at_save_root(temp.path()));
    for key in &keys {
        let bytes = memory.read(Namespace::Savegame, key).unwrap().unwrap();
        let put = [StorageOp::put(key.as_str(), bytes)];
        file_system.commit(Namespace::Savegame, &put).unwrap();
    }

    let loaded_hash = |storage: Arc<dyn StorageBackend>| {
        let mut world = world_on(corpus.path(), storage);
        world.read_game_slot("SLOT01").unwrap();
        assert_eq!(world.host.world.clock.ticks, 4321);
        world.host.world.state_hash()
    };
    assert_eq!(loaded_hash(file_system), loaded_hash(memory));
}

#[test]
fn a_save_drops_stale_maps_and_a_stale_sidecar_and_keeps_other_files() {
    let corpus = short_map_corpus();
    let storage: Arc<dyn StorageBackend> = Arc::new(MemoryStorage::new());
    let old = [
        StorageOp::put("SLOT01/OLDMAP.SAV", "stale"),
        StorageOp::put("SLOT01/VAULTSFL.DAT", "kept"),
        StorageOp::put("SLOT01/NOTES.TXT", "kept"),
        StorageOp::put(format!("SLOT01/{SIDECAR}"), "stale"),
    ];
    storage.commit(Namespace::Savegame, &old).unwrap();
    let mut world = world_on(corpus.path(), storage.clone());
    assert!(world.save_slot(0, b"Vanilla".to_vec()));
    let read = |key: &str| storage.read(Namespace::Savegame, key).unwrap();
    assert_eq!(read("SLOT01/OLDMAP.SAV"), None);
    assert_eq!(read(&format!("SLOT01/{SIDECAR}")), None);
    assert_eq!(read("SLOT01/VAULTSFL.DAT").unwrap(), b"kept");
    assert_eq!(read("SLOT01/NOTES.TXT").unwrap(), b"kept");
    assert!(read("SLOT01/SAVE.DAT").is_some());
}

#[test]
fn a_slot_in_memory_is_listed_and_loaded_as_on_disk() {
    let corpus = short_map_corpus();
    let storage: Arc<dyn StorageBackend> = Arc::new(MemoryStorage::new());
    let mut world = world_on(corpus.path(), storage.clone());
    assert_eq!(storage.slots().unwrap(), Vec::<String>::new());
    assert!(world.save_slot(2, b"Third".to_vec()));
    let SlotState::Occupied(info) = &world.slot_states()[2] else {
        panic!("slot 3 is occupied");
    };
    assert_eq!(info.description, b"Third");
    world.enhanced_quick_save();
    assert_eq!(storage.slots().unwrap(), ["SLOT03", QUICK_SLOT]);
    world.host.world.clock.set(77);
    world.enhanced_quick_load();
    assert_ne!(world.host.world.clock.ticks, 77);
    // An image with a missing companion is refused before any state changes.
    let ticks = world.host.world.clock.ticks;
    let drop_map = [StorageOp::delete("SLOT03/SYNTH.SAV")];
    storage.commit(Namespace::Savegame, &drop_map).unwrap();
    assert!(world.read_game_slot("SLOT03").is_err());
    assert_eq!(world.host.world.clock.ticks, ticks);
}

/// SPEC-023 §3.15: a whole session with no OS path: its game data is a
/// memory mount, its slots a `MemoryStorage`. Only the fixture is on disk.
#[test]
fn a_session_runs_from_a_memory_mount_and_memory_storage() {
    let corpus = short_map_corpus();
    let provider = LocalFiles::read(corpus.path());
    let tree = MemoryDir::new(provider).unwrap();
    let mount = MemoryMount {
        loose: Some(tree),
        ..Default::default()
    }
    .mount()
    .unwrap();
    let data: Arc<dyn Vfs> = Arc::new(mount);
    let storage: Arc<dyn StorageBackend> = Arc::new(MemoryStorage::new());
    let mut session = SessionRuntime::start(SessionConfig::new(
        SessionOrigin::CommandLine,
        PreviewArgs {
            data: Some(data),
            storage: Some(storage.clone()),
            corpus_dir: None,
            ..session_args(corpus.path())
        },
    ))
    .unwrap()
    .unwrap();
    let AssetPreview::Map(world) = &mut *session else {
        panic!("a map session");
    };
    assert!(world.save_slot(0, b"Memory".to_vec()));
    assert_eq!(storage.slots().unwrap(), ["SLOT01"]);
    world.read_game_slot("SLOT01").unwrap();
}

/// Every file below a folder as (path, bytes), for a memory tree.
struct LocalFiles;

impl LocalFiles {
    fn read(root: &Path) -> Vec<(String, ByteSource)> {
        let mut found = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                    continue;
                }
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned();
                let bytes = ByteSource::new(std::fs::read(&path).unwrap()).unwrap();
                found.push((relative, bytes));
            }
        }
        found
    }
}

#[test]
fn a_replay_never_touches_the_users_real_storage() {
    let corpus = short_map_corpus();
    let saves = tempfile::tempdir().unwrap();
    let mut session = SessionRuntime::start(SessionConfig::replay(PreviewArgs {
        save_dir: Some(saves.path().to_path_buf()),
        ..session_args(corpus.path())
    }))
    .unwrap()
    .unwrap();
    let AssetPreview::Map(world) = &mut *session else {
        panic!("a map session");
    };
    assert!(world.save_slot(0, b"Replay".to_vec()));
    world.read_game_slot("SLOT01").unwrap();
    assert_eq!(std::fs::read_dir(saves.path()).unwrap().count(), 0);
}

#[test]
fn the_required_members_name_the_new_game_map() {
    let members = vault_formats::vfs::FALLOUT1_MEMBERS;
    assert!(members.contains(&vault_core::ui::flow::NEW_GAME_MAP));
}
