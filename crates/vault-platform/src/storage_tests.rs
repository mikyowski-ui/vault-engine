//! SPEC-023 T8-T12 and T15 on the vectors in `tests/fixtures/spec023/`.
//! The same scripts run on every backend, whose results must be the same at
//! every step.
use super::*;
use std::collections::VecDeque;

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!("../../../tests/fixtures/spec023/", $name))
    };
}

fn rows(text: &str) -> Vec<Vec<&str>> {
    text.lines()
        .skip(1)
        .map(|line| line.split('\t').collect())
        .collect()
}

fn namespace(name: &str) -> Namespace {
    match name {
        "config" => Namespace::Config,
        "savegame" => Namespace::Savegame,
        "mods" => Namespace::Mods,
        other => panic!("no namespace {other}"),
    }
}

fn kind(error: &StorageError) -> &'static str {
    match error {
        StorageError::InvalidKey { .. } => "InvalidKey",
        StorageError::TooLarge { .. } => "TooLarge",
        StorageError::QuotaExceeded => "QuotaExceeded",
        StorageError::Collision { .. } => "Collision",
        StorageError::LayerNotFound { .. } => "LayerNotFound",
        StorageError::Unavailable(_) => "Unavailable",
        StorageError::Io(_) => "Io",
    }
}

/// `put KEY=VALUE;del KEY`, the value running from the first `=`.
fn batch(data: &str) -> Vec<StorageOp> {
    data.split(';')
        .map(|op| match op.split_once(' ') {
            Some(("put", rest)) => {
                let (key, value) = rest.split_once('=').unwrap();
                StorageOp::put(key, value)
            }
            Some(("del", key)) => StorageOp::delete(key),
            _ => panic!("{op:?}"),
        })
        .collect()
}

fn commit_result(backend: &dyn StorageBackend, space: Namespace, data: &str) -> String {
    match backend.commit(space, &batch(data)) {
        Ok(()) => "ok".to_owned(),
        Err(error) => format!("error {}", kind(&error)),
    }
}

fn joined(list: Result<Vec<String>, StorageError>) -> String {
    let list = list.unwrap();
    if list.is_empty() {
        "-".to_owned()
    } else {
        list.join(",")
    }
}

/// `storage-ops.tsv` on `backend`: every step's result, as text.
fn conformance(backend: &dyn StorageBackend) -> Vec<String> {
    rows(fixture!("storage-ops.tsv"))
        .iter()
        .map(|row| {
            let [step, op, space, key, data, expect] = row[..] else {
                panic!("{row:?}");
            };
            let space = namespace(space);
            let found = match op {
                "commit" => commit_result(backend, space, data),
                "read" => match backend.read(space, key).unwrap() {
                    Some(bytes) => String::from_utf8(bytes).unwrap(),
                    None => "none".to_owned(),
                },
                "list" => joined(backend.list(space, (key != "-").then_some(key))),
                "slots" => joined(backend.slots()),
                other => panic!("no op {other}"),
            };
            assert_eq!(found, expect, "step {step}");
            found
        })
        .collect()
}

/// The three roots of an [`FsStorage`] in `dir`; the settings folder is one
/// the shell found, so it exists.
fn fs_storage(dir: &Path) -> FsStorage {
    let config = dir.join("config");
    fs::create_dir(&config).unwrap();
    FsStorage::new(
        Some(config),
        Some(dir.join("saves/SAVEGAME")),
        Some(dir.join("mods")),
    )
}

/// Every file below `dir`, relative, sorted.
fn files_below(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(folder) = pending.pop() {
        for entry in fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path.strip_prefix(dir).unwrap();
                found.push(relative.to_str().unwrap().replace('\\', "/"));
            }
        }
    }
    found.sort();
    found
}

/// A flat store for [`KvStorage`]. Transactions wait in `queue` until
/// `complete` (or a `flush`) applies them, in order; `fail_next` fails the
/// next one to complete.
#[derive(Debug, Default)]
struct TestStore {
    records: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    queue: Arc<Mutex<VecDeque<Vec<KvWrite>>>>,
    fail_next: Arc<Mutex<bool>>,
    /// Apply each transaction as it is written.
    immediate: bool,
}

impl TestStore {
    fn complete(records: &Mutex<BTreeMap<String, Vec<u8>>>, batch: Vec<KvWrite>) {
        let mut records = records.lock().unwrap();
        for write in batch {
            match write {
                KvWrite::Put(key, bytes) => records.insert(key, bytes),
                KvWrite::Delete(key) => records.remove(&key),
            };
        }
    }
}

impl KvStore for TestStore {
    fn scan(&self, prefix: &str) -> Result<Vec<(String, Vec<u8>)>, KvError> {
        let records = self.records.lock().unwrap();
        Ok(records
            .iter()
            .filter(|(key, _)| key.starts_with(prefix))
            .map(|(key, bytes)| (key.clone(), bytes.clone()))
            .collect())
    }
    fn write(&mut self, batch: Vec<KvWrite>) -> Result<(), KvError> {
        if self.immediate {
            Self::complete(&self.records, batch);
        } else {
            self.queue.lock().unwrap().push_back(batch);
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<(), KvError> {
        loop {
            let Some(batch) = self.queue.lock().unwrap().pop_front() else {
                return Ok(());
            };
            if std::mem::take(&mut *self.fail_next.lock().unwrap()) {
                self.queue.lock().unwrap().clear();
                return Err(KvError::Failed("the host store failed".into()));
            }
            Self::complete(&self.records, batch);
        }
    }
}

fn immediate_store() -> KvStorage<TestStore> {
    let store = TestStore {
        immediate: true,
        ..TestStore::default()
    };
    KvStorage::open(store, GameProfile::Fallout1).unwrap()
}

#[test]
fn t8_every_key_gives_its_canonical_form_and_record_or_its_refusal() {
    let cases = rows(fixture!("keys.tsv"));
    assert_eq!(cases.len(), 15);
    for row in cases {
        let [space, scope, input, canonical, record] = row[..] else {
            panic!("{row:?}");
        };
        let space = namespace(space);
        // `-` in this table means "not used": the empty key.
        let input = if input == "-" { "" } else { input };
        match canonical.strip_prefix("ERROR ") {
            Some(rule) => {
                let error = canonical_key(space, input).unwrap_err();
                let StorageError::InvalidKey { rule: found, .. } = error else {
                    panic!("{input:?}: {error}");
                };
                assert_eq!(found, rule, "{input:?}");
                assert_eq!(record, "-");
            }
            None => {
                assert_eq!(canonical_key(space, input).unwrap(), canonical, "{input:?}");
                let profile = match scope {
                    "fallout2" => GameProfile::Fallout2,
                    _ => GameProfile::Fallout1,
                };
                assert_eq!(record_key(profile, space, canonical), record, "{input:?}");
            }
        }
    }
}

#[test]
fn a_namespace_limits_the_shape_of_its_keys() {
    use Namespace::{Config, Mods, Savegame};
    let refused = |space, key| canonical_key(space, key).is_err();
    assert!(refused(Config, "dir/vault-engine.toml"));
    assert!(refused(Config, "settings.ini"));
    assert!(refused(Savegame, "SAVE.DAT"));
    assert!(refused(Savegame, "SLOT01/MAPS/A.SAV"));
    assert!(refused(Savegame, "-"));
    // `.tmp` and `.bak` are reserved only where the staged write uses them.
    assert_eq!(canonical_key(Mods, "A/B.TMP").unwrap(), "a/b.tmp");
}

#[test]
fn t9_every_backend_gives_the_same_results_at_every_step() {
    let memory = conformance(&MemoryStorage::new());
    let temp = tempfile::tempdir().unwrap();
    assert_eq!(conformance(&fs_storage(temp.path())), memory);
    assert_eq!(conformance(&immediate_store()), memory);
    // Keys and bytes are all that differ from a backend to a backend's disk:
    // nothing staged or backed up stays behind.
    let files = files_below(temp.path());
    assert!(
        files
            .iter()
            .any(|file| file.ends_with("SLOT03/SAVE.DAT") || file.ends_with("slot03/save.dat"))
    );
    let leftover = |file: &&String| {
        let file = file.to_ascii_lowercase();
        file.ends_with(".tmp") || file.ends_with(".bak")
    };
    assert_eq!(files.iter().find(leftover), None);
}

#[test]
fn t10_a_quota_counts_keys_and_values_over_the_net_effect() {
    let backend = MemoryStorage::with_quota(20);
    for row in rows(fixture!("quota.tsv")) {
        let [step, op, data, expect, usage] = row[..] else {
            panic!("{row:?}");
        };
        match op {
            "commit" => {
                let found = commit_result(&backend, Namespace::Config, data);
                assert_eq!(found, expect, "step {step}");
            }
            _ => {
                let found = backend.read(Namespace::Config, data).unwrap().unwrap();
                assert_eq!(found, expect.as_bytes(), "step {step}");
            }
        }
        assert_eq!(backend.usage().to_string(), usage, "step {step}");
    }
}

/// `mods-expected.tsv` on a backend holding `mods-entries.tsv`.
fn mods_conformance(backend: &dyn StorageBackend) {
    let ops: Vec<StorageOp> = rows(fixture!("mods-entries.tsv"))
        .iter()
        .map(|row| StorageOp::put(row[0], row[1]))
        .collect();
    backend.commit(Namespace::Mods, &ops).unwrap();
    for row in rows(fixture!("mods-expected.tsv")) {
        let [query, input, result] = row[..] else {
            panic!("{row:?}");
        };
        match query {
            "manifests" => {
                let found: Vec<String> = input
                    .split(", ")
                    .map(|layer| {
                        let (layer, _) = layer.split_once(' ').unwrap();
                        let manifest = match backend.layer_manifest(layer).unwrap() {
                            Some(bytes) => String::from_utf8(bytes).unwrap(),
                            None => "none".to_owned(),
                        };
                        format!("{layer}: {manifest}")
                    })
                    .collect();
                assert_eq!(found.join("; "), result);
            }
            "find_layers" => assert_eq!(backend.find_layers().unwrap().join(", "), result),
            "layer_files" => match backend.mount_layer(input) {
                Ok(layer) => {
                    let files: Vec<&str> = layer.files().map(VfsPath::as_str).collect();
                    assert_eq!(files.join(", "), result, "{input}");
                }
                Err(error) => assert_eq!(format!("error {}", kind(&error)), result, "{input}"),
            },
            other => panic!("no query {other}"),
        }
    }
}

#[test]
fn t11_mod_layers_are_found_and_mounted_on_every_backend() {
    mods_conformance(&MemoryStorage::new());
    let temp = tempfile::tempdir().unwrap();
    mods_conformance(&FsStorage::mods_at(temp.path()));
    mods_conformance(&immediate_store());
}

#[test]
fn an_empty_folder_is_no_layer_on_any_backend() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("mods/empty")).unwrap();
    let storage = FsStorage::mods_at(temp.path());
    assert!(matches!(
        storage.mount_layer("mods/empty"),
        Err(StorageError::LayerNotFound { .. })
    ));
    assert!(matches!(
        MemoryStorage::new().mount_layer("mods/empty"),
        Err(StorageError::LayerNotFound { .. })
    ));
}

#[test]
fn t12_a_fault_while_staging_leaves_the_slot_and_nothing_else() {
    let temp = tempfile::tempdir().unwrap();
    let mut storage = FsStorage::at_save_root(temp.path());
    let old = [
        StorageOp::put("SLOT01/SAVE.DAT", "OLD-SAVE"),
        StorageOp::put("SLOT01/A.SAV", "OLD-A"),
        StorageOp::put("SLOT01/VAULTENG.TOML", "OLD-SIDECAR"),
    ];
    storage.commit(Namespace::Savegame, &old).unwrap();
    let before = files_below(temp.path());

    storage.fail_staging = Some(1);
    let new = [
        StorageOp::put("SLOT01/SAVE.DAT", "NEW-SAVE"),
        StorageOp::put("SLOT01/A.SAV", "NEW-A"),
        StorageOp::delete("SLOT01/VAULTENG.TOML"),
    ];
    let error = storage.commit(Namespace::Savegame, &new).unwrap_err();
    assert_eq!(kind(&error), "Io");
    assert!(error.to_string().contains("injected fault"), "{error}");

    assert_eq!(files_below(temp.path()), before);
    for (key, value) in [
        ("SLOT01/SAVE.DAT", "OLD-SAVE"),
        ("SLOT01/A.SAV", "OLD-A"),
        ("SLOT01/VAULTENG.TOML", "OLD-SIDECAR"),
    ] {
        let found = storage.read(Namespace::Savegame, key).unwrap().unwrap();
        assert_eq!(found, value.as_bytes(), "{key}");
    }
    storage.fail_staging = None;
    storage.commit(Namespace::Savegame, &new).unwrap();
    assert_eq!(
        files_below(temp.path()),
        ["SAVEGAME/SLOT01/A.SAV", "SAVEGAME/SLOT01/SAVE.DAT"]
    );
}

#[test]
fn a_file_system_key_keeps_the_spelling_it_has_and_reads_in_any_case() {
    let temp = tempfile::tempdir().unwrap();
    let storage = FsStorage::at_save_root(temp.path());
    let slot = temp.path().join("SAVEGAME/Slot01");
    fs::create_dir_all(&slot).unwrap();
    fs::write(slot.join("Save.Dat"), b"old").unwrap();
    let ops = [
        StorageOp::put("SLOT01/SAVE.DAT", "new"),
        StorageOp::put("slot01/AUTOMAP.SAV", "map"),
    ];
    storage.commit(Namespace::Savegame, &ops).unwrap();
    assert_eq!(
        files_below(temp.path()),
        ["SAVEGAME/Slot01/AUTOMAP.SAV", "SAVEGAME/Slot01/Save.Dat"]
    );
    let read = storage.read(Namespace::Savegame, "slot01/save.dat");
    assert_eq!(read.unwrap().unwrap(), b"new");
    // Two spellings of a new folder in one batch make one folder.
    let ops = [
        StorageOp::put("NEW01/A.SAV", "a"),
        StorageOp::put("new01/B.SAV", "b"),
    ];
    storage.commit(Namespace::Savegame, &ops).unwrap();
    let keys = storage.list(Namespace::Savegame, Some("New01"));
    assert_eq!(keys.unwrap(), ["new01/a.sav", "new01/b.sav"]);
}

#[test]
fn staging_leftovers_are_invisible_and_replaced() {
    let temp = tempfile::tempdir().unwrap();
    let storage = FsStorage::at_save_root(temp.path());
    let slot = temp.path().join("SAVEGAME/SLOT01");
    fs::create_dir_all(&slot).unwrap();
    fs::write(slot.join("SAVE.DAT"), b"current").unwrap();
    fs::write(slot.join("SAVE.BAK"), b"crashed").unwrap();
    fs::write(slot.join("SAVE.DAT.TMP"), b"crashed").unwrap();
    assert_eq!(
        storage.list(Namespace::Savegame, None).unwrap(),
        ["slot01/save.dat"]
    );
    let refused = storage.read(Namespace::Savegame, "SLOT01/SAVE.BAK");
    assert!(matches!(refused, Err(StorageError::InvalidKey { .. })));
    let ops = [StorageOp::put("SLOT01/SAVE.DAT", "next")];
    storage.commit(Namespace::Savegame, &ops).unwrap();
    assert_eq!(files_below(&slot), ["SAVE.DAT"]);
}

#[test]
fn in_mods_the_staged_write_never_touches_a_real_bak_or_tmp_file() {
    let temp = tempfile::tempdir().unwrap();
    let storage = FsStorage::mods_at(temp.path());
    let ops = [
        StorageOp::put("old/notes.txt", "text"),
        StorageOp::put("old/notes.BAK", "a real file"),
        StorageOp::put("old/notes.txt.TMP", "another"),
    ];
    storage.commit(Namespace::Mods, &ops).unwrap();
    let ops = [StorageOp::put("old/notes.txt", "new text")];
    storage.commit(Namespace::Mods, &ops).unwrap();
    assert_eq!(
        storage.list(Namespace::Mods, None).unwrap(),
        ["old/notes.bak", "old/notes.txt", "old/notes.txt.tmp"]
    );
    let read = |key| storage.read(Namespace::Mods, key).unwrap().unwrap();
    assert_eq!(read("old/notes.txt"), b"new text");
    assert_eq!(read("old/notes.bak"), b"a real file");
    assert_eq!(read("old/notes.txt.tmp"), b"another");
}

#[test]
fn two_files_of_a_batch_never_share_a_backup() {
    let temp = tempfile::tempdir().unwrap();
    let storage = FsStorage::at_save_root(temp.path());
    let old = [
        StorageOp::put("SLOT01/SAVE.DAT", "central"),
        StorageOp::put("SLOT01/SAVE.SAV", "a map named SAVE"),
    ];
    storage.commit(Namespace::Savegame, &old).unwrap();
    let new = [
        StorageOp::put("SLOT01/SAVE.DAT", "central 2"),
        StorageOp::put("SLOT01/SAVE.SAV", "map 2"),
    ];
    storage.commit(Namespace::Savegame, &new).unwrap();
    assert_eq!(
        files_below(temp.path()),
        ["SAVEGAME/SLOT01/SAVE.DAT", "SAVEGAME/SLOT01/SAVE.SAV"]
    );
}

#[cfg(unix)]
#[test]
fn links_and_collisions_are_refused_and_nothing_outside_the_root_is_read() {
    let temp = tempfile::tempdir().unwrap();
    let outside = temp.path().join("outside.txt");
    fs::write(&outside, b"secret").unwrap();
    let root = temp.path().join("root");
    fs::create_dir(&root).unwrap();
    let storage = FsStorage::mods_at(&root);
    std::os::unix::fs::symlink(&outside, root.join("link.txt")).unwrap();
    let error = storage.read(Namespace::Mods, "link.txt").unwrap_err();
    assert_eq!(kind(&error), "Io");
    assert_eq!(
        kind(&storage.list(Namespace::Mods, None).unwrap_err()),
        "Io"
    );
    let put = [StorageOp::put("link.txt", "new")];
    assert!(storage.commit(Namespace::Mods, &put).is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"secret");
    fs::remove_file(root.join("link.txt")).unwrap();

    fs::write(root.join("Twice.txt"), b"1").unwrap();
    fs::write(root.join("twice.txt"), b"2").unwrap();
    let error = storage.read(Namespace::Mods, "twice.txt").unwrap_err();
    assert_eq!(kind(&error), "Collision");
    assert_eq!(
        kind(&storage.list(Namespace::Mods, None).unwrap_err()),
        "Collision"
    );
}

#[test]
fn a_settings_folder_is_found_never_made_and_only_its_toml_is_listed() {
    let temp = tempfile::tempdir().unwrap();
    let missing = FsStorage::config_at(&temp.path().join("missing"));
    let put = [StorageOp::put("vault-engine.toml", "x")];
    assert!(missing.commit(Namespace::Config, &put).is_err());
    assert!(!temp.path().join("missing").exists());

    let storage = FsStorage::config_at(temp.path());
    fs::write(temp.path().join("notes.txt"), b"x").unwrap();
    fs::create_dir(temp.path().join("sub.toml")).unwrap();
    storage.commit(Namespace::Config, &put).unwrap();
    assert_eq!(
        storage.list(Namespace::Config, None).unwrap(),
        ["vault-engine.toml"]
    );
    assert_eq!(storage.read(Namespace::Config, "sub.toml").unwrap(), None);
}

#[test]
fn a_namespace_with_no_folder_is_unavailable() {
    let temp = tempfile::tempdir().unwrap();
    let storage = FsStorage::config_at(temp.path());
    let error = storage.read(Namespace::Savegame, "SLOT01/SAVE.DAT");
    assert_eq!(kind(&error.unwrap_err()), "Unavailable");
}

#[test]
fn every_backend_refuses_a_value_over_the_limit_and_keeps_the_batch_out() {
    let temp = tempfile::tempdir().unwrap();
    let file_system = fs_storage(temp.path());
    let memory = MemoryStorage::new();
    let key_value = immediate_store();
    let backends: [&dyn StorageBackend; 3] = [&memory, &file_system, &key_value];
    for backend in backends {
        let ops = [
            StorageOp::put("SLOT01/A.SAV", "fits"),
            StorageOp::put("SLOT01/B.SAV", vec![0; MAX_ASSET_BYTES + 1]),
        ];
        let error = backend.commit(Namespace::Savegame, &ops).unwrap_err();
        assert_eq!(kind(&error), "TooLarge");
        assert_eq!(backend.read(Namespace::Savegame, "SLOT01/A.SAV"), Ok(None));
    }
}

#[test]
fn every_backend_refuses_a_key_that_is_also_a_folder() {
    let temp = tempfile::tempdir().unwrap();
    let file_system = FsStorage::mods_at(temp.path());
    let memory = MemoryStorage::new();
    let key_value = immediate_store();
    let backends: [&dyn StorageBackend; 3] = [&memory, &file_system, &key_value];
    for backend in backends {
        let both = [StorageOp::put("a/b", "1"), StorageOp::put("a/b/c", "2")];
        let error = backend.commit(Namespace::Mods, &both).unwrap_err();
        assert_eq!(kind(&error), "InvalidKey");
        backend
            .commit(Namespace::Mods, &[StorageOp::put("a/b", "1")])
            .unwrap();
        let below = [StorageOp::put("a/b/c", "2")];
        let error = backend.commit(Namespace::Mods, &below).unwrap_err();
        assert_eq!(kind(&error), "InvalidKey");
        // A file cannot become a folder in one batch, but it can in two.
        let swap = [StorageOp::delete("a/b"), StorageOp::put("a/b/c", "2")];
        let error = backend.commit(Namespace::Mods, &swap).unwrap_err();
        assert_eq!(kind(&error), "InvalidKey");
        backend
            .commit(Namespace::Mods, &[StorageOp::delete("a/b")])
            .unwrap();
        backend
            .commit(Namespace::Mods, &[StorageOp::put("a/b/c", "2")])
            .unwrap();
    }
}

#[test]
fn a_folder_is_no_entry_on_any_backend() {
    let temp = tempfile::tempdir().unwrap();
    let file_system = FsStorage::mods_at(temp.path());
    let memory = MemoryStorage::new();
    let key_value = immediate_store();
    let backends: [&dyn StorageBackend; 3] = [&memory, &file_system, &key_value];
    for backend in backends {
        let commit = |op| backend.commit(Namespace::Mods, &[op]);
        commit(StorageOp::put("a/b", "1")).unwrap();
        // Deleting a folder deletes an absent key: its files stay.
        commit(StorageOp::delete("a")).unwrap();
        assert_eq!(backend.read(Namespace::Mods, "a"), Ok(None));
        assert_eq!(backend.read(Namespace::Mods, "a/b").unwrap().unwrap(), b"1");
        // A folder that deletes left empty is no obstacle to a file.
        commit(StorageOp::delete("a/b")).unwrap();
        commit(StorageOp::put("a", "2")).unwrap();
        assert_eq!(backend.read(Namespace::Mods, "a").unwrap().unwrap(), b"2");
        assert_eq!(backend.list(Namespace::Mods, None).unwrap(), ["a"]);
    }
}

#[test]
fn an_empty_batch_and_a_delete_ask_nothing_of_a_missing_folder() {
    let temp = tempfile::tempdir().unwrap();
    let missing = FsStorage::config_at(&temp.path().join("missing"));
    missing.commit(Namespace::Config, &[]).unwrap();
    let delete = [StorageOp::delete("vault-engine.toml")];
    missing.commit(Namespace::Config, &delete).unwrap();
    assert!(!temp.path().join("missing").exists());
}

#[test]
fn t15_a_late_store_sees_commits_in_order_and_a_failure_stops_the_rest() {
    let store = TestStore::default();
    let records = store.records.clone();
    let queue = store.queue.clone();
    let fail_next = store.fail_next.clone();
    let storage = KvStorage::open(store, GameProfile::Fallout2).unwrap();
    let first = [StorageOp::put("SLOT01/SAVE.DAT", "one")];
    let second = [
        StorageOp::put("SLOT01/SAVE.DAT", "two"),
        StorageOp::put("SLOT01/A.SAV", "a"),
    ];
    storage.commit(Namespace::Savegame, &first).unwrap();
    storage.commit(Namespace::Savegame, &second).unwrap();

    // Reads see a commit at once, though nothing has reached the store.
    let read = storage.read(Namespace::Savegame, "SLOT01/SAVE.DAT");
    assert_eq!(read.unwrap().unwrap(), b"two");
    assert!(records.lock().unwrap().is_empty());
    // One transaction per commit, in commit order.
    let key = "vault-engine/1/fallout2/savegame/slot01/save.dat";
    {
        let queue = queue.lock().unwrap();
        assert_eq!(queue.len(), 2);
        assert_eq!(queue[0], [KvWrite::Put(key.to_owned(), b"one".to_vec())]);
        assert_eq!(queue[1].len(), 2);
    }
    storage.flush().unwrap();
    assert_eq!(records.lock().unwrap()[key], b"two");

    // A failed transaction makes `flush` report it, and later commits
    // return `Unavailable`, so a later save is never reported as kept.
    storage.commit(Namespace::Savegame, &first).unwrap();
    *fail_next.lock().unwrap() = true;
    assert_eq!(kind(&storage.flush().unwrap_err()), "Unavailable");
    assert_eq!(records.lock().unwrap()[key], b"two");
    let refused = storage.commit(Namespace::Savegame, &second).unwrap_err();
    assert_eq!(kind(&refused), "Unavailable");

    // Reopening the store clears it, with what reached the store.
    let store = TestStore {
        records: records.clone(),
        ..TestStore::default()
    };
    let reopened = KvStorage::open(store, GameProfile::Fallout2).unwrap();
    let read = reopened.read(Namespace::Savegame, "SLOT01/SAVE.DAT");
    assert_eq!(read.unwrap().unwrap(), b"two");
    reopened.commit(Namespace::Savegame, &first).unwrap();
}

#[test]
fn a_store_keeps_each_games_slots_and_reports_a_foreign_record_once() {
    let store = TestStore {
        immediate: true,
        ..TestStore::default()
    };
    let records = store.records.clone();
    {
        let mut records = records.lock().unwrap();
        for key in [
            "vault-engine/1/fallout1/savegame/slot01/save.dat",
            "vault-engine/1/fallout2/savegame/slot02/save.dat",
            "vault-engine/1/shared/config/vault-engine.toml",
            "vault-engine/1/shared/mods/mods/a/vault-mod.toml",
            // The adapter's own journal is no entry, and no finding.
            "vault-engine/1/journal/7",
            // Shapes this layout never writes.
            "vault-engine/1/shared/savegame/slot01/save.dat",
            "vault-engine/1/fallout1/config/vault-engine.toml",
            "vault-engine/1/shared/nothing/x",
            "vault-engine/1/shared/config",
            "vault-engine/1/fallout1/savegame/SLOT01/SAVE.DAT",
            // A later layout's record is not looked at.
            "vault-engine/2/shared/config/vault-engine.toml",
        ] {
            records.insert(key.to_owned(), b"x".to_vec());
        }
    }
    let storage = KvStorage::open(store, GameProfile::Fallout1).unwrap();
    assert_eq!(
        storage.list(Namespace::Savegame, None).unwrap(),
        ["slot01/save.dat"]
    );
    assert_eq!(
        storage.list(Namespace::Config, None).unwrap(),
        ["vault-engine.toml"]
    );
    assert_eq!(storage.find_layers().unwrap(), ["mods/a"]);
    assert_eq!(storage.ignored().len(), 5);
    // A key-value host stores only the canonical form.
    let put = [StorageOp::put("SLOT03/SAVE.DAT", "x")];
    storage.commit(Namespace::Savegame, &put).unwrap();
    let records = records.lock().unwrap();
    assert!(records.contains_key("vault-engine/1/fallout1/savegame/slot03/save.dat"));
}

#[test]
fn a_host_quota_error_refuses_the_commit() {
    #[derive(Debug)]
    struct Full;
    impl KvStore for Full {
        fn scan(&self, _: &str) -> Result<Vec<(String, Vec<u8>)>, KvError> {
            Ok(Vec::new())
        }
        fn write(&mut self, _: Vec<KvWrite>) -> Result<(), KvError> {
            Err(KvError::Quota)
        }
        fn flush(&mut self) -> Result<(), KvError> {
            Ok(())
        }
    }
    let storage = KvStorage::open(Full, GameProfile::Fallout1).unwrap();
    let put = [StorageOp::put("vault-engine.toml", "x")];
    let error = storage.commit(Namespace::Config, &put).unwrap_err();
    assert_eq!(kind(&error), "QuotaExceeded");
    assert_eq!(
        storage.read(Namespace::Config, "vault-engine.toml"),
        Ok(None)
    );
}
