//! SPEC-024a §2.3 and §10 tests 9 to 11: a session with scripted layers, its
//! `mod_stack` record, and a replay held to it. The layers are real folders
//! on a load order, and §9.1's program goes through the mod-script check.
use super::*;
use crate::{
    config::ModList,
    input::mod_stack_record,
    preview::PreviewArgs,
    session::{SessionConfig, SessionOrigin, SessionRuntime},
    storage::{MemoryStorage, Namespace, StorageBackend, StorageOp},
};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use vault_core::sfall::SfallValue;
use vault_fixtures::{gallery, state, state as fixture, vm};

const N1: &str = include_str!("../../../tests/fixtures/spec024a/vault-mod.toml");
const N2: &str = include_str!("../../../tests/fixtures/spec024a/vault-mod-row.toml");
const PROGRAM: &str = include_str!("../../../tests/fixtures/spec024a/gl_cheapap.int.hex");
const SET_SETTING: u16 = 0x8302;

fn fixture_program() -> Vec<u8> {
    PROGRAM
        .lines()
        .filter_map(|line| line.split_once(": "))
        .flat_map(|(_, bytes)| bytes.split(' '))
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect()
}
/// A folder of game data with no layers yet.
fn corpus() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    state::write_corpus(dir.path());
    gallery::write_hud(dir.path());
    std::fs::write(dir.path().join("scripts/core-fixture.int"), vm::program()).unwrap();
    dir
}
/// A layer `name` under `mods/`, with `manifest` and `script` as `file`.
fn layer(root: &Path, name: &str, manifest: &str, file: &str, script: &[u8]) {
    let dir = root.join("mods").join(name);
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(dir.join("vault-mod.toml"), manifest).unwrap();
    std::fs::write(dir.join("scripts").join(file), script).unwrap();
}
/// A load order of `layers`, lowest first.
fn order(root: &Path, layers: &[&str]) -> ModList {
    let text: String = layers
        .iter()
        .map(|name| format!("[[layer]]\npath = \"mods/{name}\"\n"))
        .collect();
    let path = root.join(format!("{}.toml", layers.join("-")));
    std::fs::write(&path, text).unwrap();
    ModList::File(path)
}
fn args(root: &Path, mods: ModList, headless: bool) -> PreviewArgs {
    PreviewArgs {
        corpus_dir: Some(root.into()),
        preview_map: Some("maps/synthetic.map".into()),
        seed: Some(1),
        headless,
        mods: Some(mods),
        ..Default::default()
    }
}
/// A replay's session over `mods`.
fn session(root: &Path, mods: ModList) -> SessionRuntime {
    SessionRuntime::start(SessionConfig::replay(args(root, mods, true)))
        .unwrap()
        .unwrap()
}
fn log_line(id: u64, command: &InputCommand) -> String {
    let mut ids = vault_core::SequenceIds::from_last(id - 1, 0);
    let mut line = Vec::new();
    crate::input::write_command(&mut line, &mut ids, command).unwrap();
    String::from_utf8(line).unwrap()
}
/// The leading record, a `mod_stack` when `stack` has one, and two ticks.
fn log(stack: Option<&InputCommand>) -> String {
    let surface = InputCommand::Surface {
        width: 640,
        height: 480,
    };
    let mut text = log_line(1, &surface);
    let mut next = 2;
    if let Some(stack) = stack {
        text += &log_line(next, stack);
        next += 1;
    }
    text += &log_line(next, &InputCommand::Tick { now_ms: 16 });
    text += &log_line(next + 1, &InputCommand::Tick { now_ms: 32 });
    text
}
fn replay_of(root: &Path, mods: ModList) -> Replay {
    Replay::with_checkpoints(session(root, mods), CheckpointOptions::default()).unwrap()
}
fn stack_of(root: &Path, mods: ModList) -> InputCommand {
    mod_stack_record(&session(root, mods).mod_session().unwrap())
}
fn refusal(replay: &mut Replay, text: &str) -> String {
    let error = replay.run(std::io::Cursor::new(text)).unwrap_err();
    error.to_string()
}

/// §2.3's example, byte for byte.
#[test]
fn the_record_of_n1_is_the_specs_example() {
    let dir = corpus();
    layer(
        dir.path(),
        "cheap",
        N1,
        "gl_cheapap.int",
        &fixture_program(),
    );
    let record = stack_of(dir.path(), order(dir.path(), &["cheap"]));
    assert_eq!(
        log_line(7, &record).trim_end(),
        r#"{"command_id":7,"kind":"mod_stack","api":1,"mods":[{"id":"cheap-ap","version":"1.0.0","manifest":"EE7C38513A262548","settings":{"ap_cost":"i:1"}}]}"#
    );
    let parsed: LoggedCommand = serde_json::from_str(&log_line(7, &record)).unwrap();
    assert_eq!(parsed.command, record);
}
/// The script word on a real session: `HOOK_CALCAPCOST` gives the setting.
#[test]
fn n1_and_n2_cost_the_setting_on_a_real_session() {
    for manifest in [N1, N2] {
        let dir = corpus();
        layer(
            dir.path(),
            "cheap",
            manifest,
            "gl_cheapap.int",
            &fixture_program(),
        );
        let mods = order(dir.path(), &["cheap"]);
        let mut replay = replay_of(dir.path(), mods.clone());
        replay
            .run(std::io::Cursor::new(log(Some(&stack_of(dir.path(), mods)))))
            .unwrap();
        let world = interactive_world(&mut replay.session).unwrap();
        let player = world.host.world.player;
        assert_eq!(world.host.hook_fixed_cost(player, 5, 0), 1);
    }
}
/// §10 test 10.
#[test]
fn a_changed_manifest_order_or_layer_is_refused_before_the_first_step() {
    let dir = corpus();
    let root = dir.path();
    let beta = N1
        .replace("cheap-ap", "beta")
        .replace("gl_cheapap", "gl_beta");
    layer(root, "alpha", N1, "gl_cheapap.int", &fixture_program());
    layer(root, "beta", &beta, "gl_beta.int", &fixture_program());
    let both = order(root, &["alpha", "beta"]);
    let record = stack_of(root, both.clone());
    let text = log(Some(&record));
    let mut same = replay_of(root, both.clone());
    same.run(std::io::Cursor::new(&text)).unwrap();
    assert_eq!(same.now, 32, "the same stack runs");
    let refused = |mods: ModList, text: &str| {
        let mut replay = replay_of(root, mods);
        let message = refusal(&mut replay, text);
        assert_eq!(replay.now, 0, "before the first step");
        message
    };
    // The layer order.
    let swapped = order(root, &["beta", "alpha"]);
    assert_eq!(refused(swapped, &text), "mod stack mismatch: cheap-ap");
    // A missing layer.
    let missing = order(root, &["alpha"]);
    assert_eq!(refused(missing, &text), "mod stack mismatch: beta");
    // A changed manifest byte.
    let edited = format!("{beta}\n# one more line\n");
    layer(root, "beta", &edited, "gl_beta.int", &fixture_program());
    assert_eq!(refused(both.clone(), &text), "mod stack mismatch: beta");
    // A log with no record means no scripted layer.
    assert_eq!(refused(both, &log(None)), "mod stack mismatch: cheap-ap");
}
/// A record with no scripted layer here is as much a difference.
#[test]
fn a_record_with_no_layers_mounted_is_refused() {
    let dir = corpus();
    layer(
        dir.path(),
        "cheap",
        N1,
        "gl_cheapap.int",
        &fixture_program(),
    );
    let record = stack_of(dir.path(), order(dir.path(), &["cheap"]));
    let plain = SessionRuntime::start(SessionConfig::replay(PreviewArgs {
        corpus_dir: Some(dir.path().into()),
        preview_map: Some("maps/synthetic.map".into()),
        seed: Some(1),
        headless: true,
        ..Default::default()
    }))
    .unwrap()
    .unwrap();
    let mut replay = Replay::with_checkpoints(plain, CheckpointOptions::default()).unwrap();
    assert_eq!(
        refusal(&mut replay, &log(Some(&record))),
        "mod stack mismatch: cheap-ap"
    );
}
/// A script that writes `value` to `ap_cost` in its `start`.
fn writing_script(value: i32) -> Vec<u8> {
    let body = [
        fixture::literal(6),
        fixture::push(value),
        fixture::instruction(SET_SETTING),
    ]
    .concat();
    fixture::corpus_program(&[b"ap_cost"], &[(b"start", fixture::returning(body))])
}
const SETTINGS: &str = "mod-settings-cheap-ap.toml";
/// §10 test 11: the record's settings, no settings file, and no write.
#[test]
fn a_replay_takes_its_settings_from_the_record_and_writes_nothing() {
    let dir = corpus();
    let root = dir.path();
    layer(root, "cheap", N1, "gl_cheapap.int", &fixture_program());
    let mods = order(root, &["cheap"]);
    let InputCommand::ModStack {
        api,
        mods: mut kept,
    } = stack_of(root, mods.clone())
    else {
        panic!("a record");
    };
    kept[0].settings.insert("ap_cost".into(), "i:3".into());
    let record = InputCommand::ModStack { api, mods: kept };
    let mut replay = replay_of(root, mods.clone());
    replay
        .run(std::io::Cursor::new(log(Some(&record))))
        .unwrap();
    let world = interactive_world(&mut replay.session).unwrap();
    let player = world.host.world.player;
    assert_eq!(
        world.host.hook_fixed_cost(player, 5, 0),
        3,
        "from the record"
    );
    // Its script's write reaches the session's table, and commits nothing.
    layer(root, "cheap", N1, "gl_cheapap.int", &writing_script(5));
    let mut replay = replay_of(root, mods);
    replay
        .run(std::io::Cursor::new(log(Some(&record))))
        .unwrap();
    let world = interactive_world(&mut replay.session).unwrap();
    let session = world.host.mod_session.clone().unwrap();
    assert_eq!(session.settings(0)["ap_cost"], SfallValue::Int(5));
    let storage = world.storage.clone().unwrap();
    assert_eq!(storage.read(Namespace::Config, SETTINGS).unwrap(), None);
}
fn live(root: &Path, mods: ModList, storage: Arc<MemoryStorage>) -> SessionRuntime {
    let mut args = args(root, mods, false);
    args.storage = Some(storage);
    SessionRuntime::start(SessionConfig::new(SessionOrigin::CommandLine, args))
        .unwrap()
        .unwrap()
}
/// §7.4: a live session commits at the end of phase 2, and the next one
/// starts from what it stored, dropping what the manifest does not declare.
#[test]
fn settings_commit_and_come_back_and_a_bad_line_is_dropped() {
    let dir = corpus();
    let root = dir.path();
    layer(root, "cheap", N1, "gl_cheapap.int", &writing_script(5));
    let mods = order(root, &["cheap"]);
    let storage = Arc::new(MemoryStorage::new());
    let mut first = live(root, mods.clone(), storage.clone());
    first.tick(16).unwrap();
    let text = storage.read(Namespace::Config, SETTINGS).unwrap().unwrap();
    assert_eq!(text, b"ap_cost = 5\n");
    layer(root, "cheap", N1, "gl_cheapap.int", &fixture_program());
    let ops = [StorageOp::put(
        SETTINGS,
        "ap_cost = 7\nother = 1\nap_cost2 = \"x\"\n",
    )];
    storage.commit(Namespace::Config, &ops).unwrap();
    let mut second = live(root, mods, storage.clone());
    second.tick(16).unwrap();
    let world = interactive_world(&mut second).unwrap();
    let session = world.host.mod_session.clone().unwrap();
    assert_eq!(session.settings(0)["ap_cost"], SfallValue::Int(7));
}
/// A log the test can read back while the shell holds its writer.
#[derive(Clone, Default)]
struct SharedLog(Arc<std::sync::Mutex<Vec<u8>>>);
impl std::io::Write for SharedLog {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl SharedLog {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}
/// The desktop's start of a log over `session`: its surface, then what both
/// surface sites call.
fn desktop_log(session: SessionRuntime, set: impl FnOnce(&mut crate::Shell)) -> SharedLog {
    let written = SharedLog::default();
    let mut shell = crate::Shell {
        input_log: crate::InputLog(Some(Box::new(written.clone()))),
        session: Some(session),
        ..Default::default()
    };
    set(&mut shell);
    shell.record(InputCommand::Surface {
        width: 640,
        height: 480,
    });
    shell.log_session_records();
    shell.record(InputCommand::Tick { now_ms: 16 });
    shell.record(InputCommand::Tick { now_ms: 32 });
    written
}
fn kinds(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            value["kind"].as_str().unwrap().to_owned()
        })
        .collect()
}
/// §2.3 on the desktop path: a live session's stack is logged last of the
/// session records, its stored settings with it, and a replay of the very
/// log it wrote takes the same layers and those settings from the record.
#[test]
fn the_desktop_logs_its_stack_last_and_a_replay_of_that_log_runs() {
    let dir = corpus();
    let root = dir.path();
    layer(root, "cheap", N1, "gl_cheapap.int", &fixture_program());
    let mods = order(root, &["cheap"]);
    let storage = Arc::new(MemoryStorage::new());
    let ops = [StorageOp::put(SETTINGS, "ap_cost = 7\n")];
    storage.commit(Namespace::Config, &ops).unwrap();
    let written = desktop_log(live(root, mods.clone(), storage), |shell| {
        shell.touch_ui = true;
        shell.game_speed = Some(crate::enhanced::GameSpeed::Fast);
    });
    let text = written.text();
    assert_eq!(
        kinds(&text),
        [
            "surface",
            "scaling",
            "touch_ui",
            "game_speed",
            "mod_stack",
            "tick",
            "tick"
        ]
    );
    assert!(text.contains(r#""settings":{"ap_cost":"i:7"}"#), "{text}");
    let mut replay = replay_of(root, mods);
    replay.run(std::io::Cursor::new(&text)).unwrap();
    assert_eq!(replay.now, 32);
    let world = interactive_world(&mut replay.session).unwrap();
    let player = world.host.world.player;
    assert_eq!(world.host.hook_fixed_cost(player, 5, 0), 7);
    let storage = world.storage.clone().unwrap();
    assert_eq!(storage.read(Namespace::Config, SETTINGS).unwrap(), None);
}
/// §2.3: a session with no scripted layer logs no record.
#[test]
fn the_desktop_logs_no_stack_without_a_scripted_layer() {
    let dir = corpus();
    let root = dir.path();
    let plain = PreviewArgs {
        corpus_dir: Some(root.into()),
        preview_map: Some("maps/synthetic.map".into()),
        seed: Some(1),
        headless: true,
        storage: Some(Arc::new(MemoryStorage::new())),
        ..Default::default()
    };
    let session = SessionRuntime::start(SessionConfig::new(SessionOrigin::CommandLine, plain))
        .unwrap()
        .unwrap();
    let written = desktop_log(session, |_| {});
    assert_eq!(
        kinds(&written.text()),
        ["surface", "scaling", "tick", "tick"]
    );
}

/// §10 test 9: a chain never starts while a checkpoint is taken or a frame
/// composed, with every point attached.
#[test]
fn no_chain_starts_in_the_checkpoint_or_presentation_phases() {
    let dir = corpus();
    let root = dir.path();
    let rows: String = include_str!("../../../tests/fixtures/spec024a/hook-points.tsv")
        .lines()
        .skip(1)
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .filter(|cells| cells[3] == "yes")
        .map(|cells| {
            format!(
                "[[hooks]]\npoint = \"{}\"\nfile = \"gl_cheapap.int\"\nprocedure = \"ap_cost\"\n",
                cells[1]
            )
        })
        .collect();
    let manifest =
        format!("name = \"All\"\nid = \"all\"\napi = 1\n[settings]\nap_cost = 1\n{rows}");
    layer(root, "all", &manifest, "gl_cheapap.int", &fixture_program());
    let mods = order(root, &["all"]);
    let record = stack_of(root, mods.clone());
    let mut replay = Replay::with_checkpoints(
        session(root, mods),
        CheckpointOptions {
            out: Some(root.join("stream.tsv")),
            expected: None,
        },
    )
    .unwrap();
    let (started, wrong) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let observer = (started.clone(), wrong.clone());
    interactive_world(&mut replay.session)
        .unwrap()
        .host
        .chain_observer = Some(Box::new(move |_| {
        observer.0.fetch_add(1, Ordering::Relaxed);
        if OUTSIDE_SIMULATION.get() {
            observer.1.fetch_add(1, Ordering::Relaxed);
        }
    }));
    let key = |pressed| InputCommand::Key {
        physical_key: "KeyA".into(),
        pressed,
        repeat: false,
    };
    let button = |pressed| InputCommand::PointerButton {
        button: "Left".into(),
        pressed,
    };
    let lines = [
        InputCommand::Surface {
            width: 640,
            height: 480,
        },
        record,
        InputCommand::Tick { now_ms: 16 },
        InputCommand::PointerMoved { x: 100.5, y: 100.5 },
        button(true),
        button(false),
        key(true),
        key(false),
        InputCommand::Tick { now_ms: 32 },
        InputCommand::Tick { now_ms: 48 },
    ];
    let text: String = lines
        .iter()
        .enumerate()
        .map(|(index, command)| log_line(index as u64 + 1, command))
        .collect();
    replay.run(std::io::Cursor::new(text)).unwrap();
    assert!(started.load(Ordering::Relaxed) >= 4, "chains ran");
    assert_eq!(wrong.load(Ordering::Relaxed), 0);
}
/// §4.4: a script the check refuses runs none of its rows, and the session
/// goes on. `0x830A` is in no set (§7.1).
#[test]
fn a_script_using_a_word_past_the_ten_is_refused_and_its_rows_run_nothing() {
    let dir = corpus();
    let root = dir.path();
    let body = fixture::instruction(0x830a);
    let script = fixture::corpus_program(&[], &[(b"start", fixture::returning(body))]);
    let row = "name = \"R\"\nid = \"r\"\napi = 1\n[[hooks]]\npoint = \"HOOK_CALCAPCOST\"\n\
               file = \"gl_refused.int\"\n";
    layer(root, "refused", row, "gl_refused.int", &script);
    let mods = order(root, &["refused"]);
    let record = stack_of(root, mods.clone());
    let mut replay = replay_of(root, mods);
    replay
        .run(std::io::Cursor::new(log(Some(&record))))
        .unwrap();
    let world = interactive_world(&mut replay.session).unwrap();
    let player = world.host.world.player;
    assert_eq!(world.host.hook_fixed_cost(player, 5, 0), 5);
    assert!(!world.host.hook_has_members(2));
}
/// §7.4: a stored key the manifest does not declare, or of the wrong type,
/// is dropped, and its one line says so.
#[test]
fn stored_settings_are_checked_against_the_manifest() {
    let dir = corpus();
    let root = dir.path();
    layer(root, "cheap", N1, "gl_cheapap.int", &fixture_program());
    let ops = [StorageOp::put(SETTINGS, "other = 1\nap_cost = \"x\"\n")];
    let storage = MemoryStorage::new();
    storage.commit(Namespace::Config, &ops).unwrap();
    let dirs = [root.join("mods/cheap")];
    let session = crate::mods::mod_session(&dirs, Some(&storage), true)
        .unwrap()
        .unwrap();
    assert_eq!(session.settings(0)["ap_cost"], SfallValue::Int(1));
    assert_eq!(session.take_notes().len(), 2);
}
