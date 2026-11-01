//! SPEC-020 §2.5, §2.6 and §3 criterion 5, headless: a session stopped under
//! the selector and resumed, a warm switch against a cold start, and what a
//! start's flags decide. The selector's own rows are in `boot_tests`.
use super::*;
use crate::{
    config::{Config, GamePaths, HotkeyHints, PerGame},
    input::{InputCommand, write_command},
    map_preview::test_support::synthetic_corpus,
    replay::{CheckpointOptions, Replay},
    session::{SessionConfig, SessionOrigin, SessionRuntime},
    storage::MemoryStorage,
};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use vault_core::GameProfile::{self, Fallout1, Fallout2};
use vault_fixtures::fallout2 as tables;

fn parse(args: &[&str]) -> Args {
    Args::try_parse_from(std::iter::once("vault-platform").chain(args.iter().copied())).unwrap()
}
fn dirs(one: &Path, two: &Path) -> PerGame<Vec<PathBuf>> {
    PerGame {
        fallout1: vec![one.to_path_buf()],
        fallout2: vec![two.to_path_buf()],
    }
}
/// Two synthetic folders, one of each edition's format.
fn folders() -> (tempfile::TempDir, tempfile::TempDir) {
    let (one, two) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    vault_fixtures::dat::fallout1_install(one.path());
    vault_fixtures::dat2::launcher_install(two.path(), &vault_formats::fallout2::REQUIRED_MEMBERS);
    (one, two)
}

/// SPEC-020 §2.4: every flag that names a start bypasses the boot screens at
/// once, so a headless or CI run is what it always was.
#[test]
fn spec020_a_named_start_bypasses_the_boot_screens() {
    let (one, two) = folders();
    let dirs = dirs(one.path(), two.path());
    let plan = |flags: &[&str]| boot_plan(&parse(flags), None, true, &dirs);
    for flags in [
        &["--game-profile", "fallout2"][..],
        &["--launcher"],
        &["--game"],
        &["--preview-map", "maps/a.map"],
        &["--preview-frm", "art/a.frm"],
        &["--play-movie", "art/cuts/a.mve"],
        &["--smoke-test"],
        &[
            "--replay-inputs",
            "log.jsonl",
            "--preview-map",
            "maps/a.map",
        ],
        &["--corpus-dir", "data", "--preview-map", "maps/a.map"],
        &[
            "--master-dat",
            "m.dat",
            "--critter-dat",
            "c.dat",
            "--preview-map",
            "maps/a.map",
        ],
    ] {
        assert!(plan(flags).is_none(), "{flags:?}");
    }
    // Display flags do not stop a zero-argument start.
    for flags in [
        &[][..],
        &["--scaling", "integer"],
        &["--preset", "enhanced"],
        &["--fullscreen"],
        &["--no-audio"],
    ] {
        let (decision, _) = plan(flags).unwrap_or_else(|| panic!("{flags:?}"));
        assert_eq!(decision, boot::Decision::Selector, "{flags:?}");
    }
}

/// B2, B3 and B17 through the plan, from settings as a file gives them.
#[test]
fn spec020_a_zero_argument_start_follows_the_settings() {
    let (one, two) = folders();
    let dirs = dirs(one.path(), two.path());
    let args = parse(&[]);
    let settings = Config {
        last_played: Some("fallout2".to_owned()),
        ..Config::default()
    };
    let (decision, found) = boot_plan(&args, Some(&settings), true, &dirs).unwrap();
    assert_eq!(decision, boot::Decision::Banner(Fallout2));
    assert!(found.get(Fallout2).is_ok(), "the banner boots this");
    let (decision, _) = boot_plan(&args, None, false, &dirs).unwrap();
    assert_eq!(decision, boot::Decision::Launcher, "settings do not parse");
}

/// Only flags a zero-argument start may carry cross a warm switch.
#[test]
fn spec020_a_warm_switch_keeps_display_flags_only() {
    let shown = parse(&[
        "--game",
        "--sfall",
        "--corpus-dir",
        "data",
        "--scaling",
        "integer",
        "--preset",
        "enhanced",
        "--fullscreen",
        "--no-audio",
        "--record-inputs",
        "log.jsonl",
        "--mod-list",
        "order.toml",
        "--seed",
        "7",
        "--debug",
    ]);
    let kept = switch_args(&shown);
    assert_eq!(kept.scaling, Some(ScalingMode::Integer));
    assert_eq!(kept.preset, Some(config::Preset::Enhanced));
    assert!(kept.fullscreen && kept.no_audio);
    assert!(kept.zero_argument(), "it is a zero-argument start");
    assert!(!kept.preview.sfall && !kept.preview.game && !kept.preview.debug);
    assert_eq!(kept.preview.corpus_dir, None);
    assert_eq!(kept.preview.seed, None);
    assert_eq!(kept.mod_list, None);
    assert_eq!(kept.record_inputs, None);
}

/// Alt+L is a shell key under both presets, and the game never receives it.
#[test]
fn spec020_alt_l_is_a_shell_key_and_l_alone_is_not() {
    for enhanced in [false, true] {
        assert_eq!(
            shell_hotkey(KeyCode::KeyL, true, enhanced),
            Some(ShellHotkey::Switch)
        );
        assert_eq!(shell_hotkey(KeyCode::KeyL, false, enhanced), None);
    }
}

/// A shared log, and a session on the synthetic corpus.
#[derive(Clone, Default)]
struct SharedLog(Arc<Mutex<Vec<u8>>>);
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
fn map_args(root: &Path) -> preview::PreviewArgs {
    preview::PreviewArgs {
        corpus_dir: Some(root.into()),
        preview_map: Some("maps/synthetic.map".into()),
        seed: Some(1),
        headless: true,
        storage: Some(Arc::new(MemoryStorage::new())),
        ..Default::default()
    }
}
fn map_session(root: &Path) -> SessionRuntime {
    SessionRuntime::start(SessionConfig::new(
        SessionOrigin::CommandLine,
        map_args(root),
    ))
    .unwrap()
    .unwrap()
}
fn selector(running: GameProfile) -> boot::Selector {
    let found = PerGame {
        fallout1: Ok(GamePaths::default()),
        fallout2: Ok(GamePaths::default()),
    };
    boot::Selector::new(&Config::default(), found, Some(running))
}
fn state_hash(session: &mut SessionRuntime) -> u64 {
    crate::replay::interactive_world(session)
        .unwrap()
        .host
        .world
        .state_hash()
}

/// S1, S2, S8 and S10: while the selector is open no step runs, no record
/// is written and the clock stops, and the session resumes at the same step.
#[test]
fn spec020_a_cancelled_selector_changes_nothing() {
    let dir = synthetic_corpus();
    let written = SharedLog::default();
    let mut shell = Shell {
        input_log: InputLog(Some(Box::new(written.clone()))),
        session: Some(map_session(dir.path())),
        started: Some(Instant::now()),
        ..Default::default()
    };
    shell.record(InputCommand::Tick { now_ms: 16 });
    let log = written.text();
    let hash = state_hash(shell.session.as_mut().unwrap());
    let now = shell.now();

    shell.stop_session(selector(Fallout1));
    // Nothing a shell does while open reaches the log.
    shell.record(InputCommand::Tick { now_ms: 32 });
    shell.record(InputCommand::Scaling { aspect_fit: true });
    assert_eq!(written.text(), log, "no record");
    std::thread::sleep(Duration::from_millis(60));
    let stopped = shell.resume_session().unwrap();
    assert!(shell.boot.is_none());
    assert!(stopped.held_back.is_empty());

    assert_eq!(state_hash(shell.session.as_mut().unwrap()), hash);
    assert!(
        shell.now() < now + 50,
        "the clock skipped the selector: {} from {now}",
        shell.now()
    );
    // It runs on, and logs, exactly as before.
    shell.record(InputCommand::Tick { now_ms: 32 });
    assert_ne!(written.text(), log);
    assert!(shell.resume_session().is_none(), "already running");
}

/// What the selector consumed stays with it, and what the session is owed
/// comes back in order.
#[test]
fn spec020_the_session_gets_what_it_missed_when_it_resumes() {
    let dir = synthetic_corpus();
    let mut shell = Shell {
        session: Some(map_session(dir.path())),
        started: Some(Instant::now()),
        ..Default::default()
    };
    shell.stop_session(selector(Fallout1));
    shell.hold_back(Some(WindowEvent::Focused(false)));
    shell.hold_back(Some(WindowEvent::Focused(true)));
    shell.hold_back(None);
    shell.boot_buttons.push(MouseButton::Left);
    assert!(shell.take_boot_button(MouseButton::Left));
    assert!(!shell.take_boot_button(MouseButton::Left), "once");
    assert!(!shell.take_boot_button(MouseButton::Right));
    let stopped = shell.resume_session().unwrap();
    assert_eq!(
        stopped.held_back,
        [WindowEvent::Focused(false), WindowEvent::Focused(true)]
    );
}

/// S11: the old log ends as at quit, and the new session is not recorded.
/// Only the window, the renderer and the storage carry over.
#[test]
fn spec020_teardown_ends_the_log_and_drops_the_session() {
    let dir = synthetic_corpus();
    let written = SharedLog::default();
    let mut shell = Shell {
        input_log: InputLog(Some(Box::new(written.clone()))),
        session: Some(map_session(dir.path())),
        started: Some(Instant::now()),
        frame: IndexedImage::new(4, 4).ok(),
        toasts: Some(toast::HotkeyToasts::new(true)),
        ..Default::default()
    };
    shell.stop_session(selector(Fallout1));
    crate::interface::note_once("a warm switch forgets this", "a test note");
    shell.end_session();
    assert!(
        written.text().trim_end().ends_with(r#""kind":"quit"}"#),
        "{}",
        written.text()
    );
    assert!(shell.session.is_none() && shell.boot.is_none() && shell.stopped.is_none());
    assert!(shell.frame.is_none() && shell.toasts.is_none() && shell.display.is_none());
    let log = written.text();
    shell.record(InputCommand::Tick { now_ms: 16 });
    assert_eq!(written.text(), log, "the log has ended");
}

/// S12: a memory-mounted session says so, and has nothing to switch to.
#[test]
fn spec020_a_memory_mounted_session_is_known() {
    let dir = synthetic_corpus();
    assert!(!map_session(dir.path()).memory_mounted());
    let provider = vault_formats::LocalDirProvider::mount(dir.path()).unwrap();
    let args = preview::PreviewArgs {
        corpus_dir: None,
        data: Some(Arc::new(provider)),
        ..map_args(dir.path())
    };
    let session = SessionRuntime::start(SessionConfig::new(SessionOrigin::CommandLine, args))
        .unwrap()
        .unwrap();
    assert!(session.memory_mounted());
}

/// S13: a replay has no shell key. On the main menu L means Load, so Alt+L
/// in a log is seen to do nothing there, where L alone reaches the game.
#[test]
fn spec020_alt_l_in_a_log_has_no_effect() {
    let installs = installs();
    let temp = tempfile::tempdir().unwrap();
    let line = |id: u64, command: &InputCommand| {
        let mut ids = vault_core::SequenceIds::from_last(id - 1, 0);
        let mut line = Vec::new();
        write_command(&mut line, &mut ids, command).unwrap();
        String::from_utf8(line).unwrap()
    };
    let title_after = |alt: Option<bool>| {
        let session = boot_session(&installs.settings, Fallout1, temp.path());
        let surface = InputCommand::Surface {
            width: 640,
            height: 480,
        };
        let mut log = line(1, &surface);
        if let Some(alt) = alt {
            let modifiers = InputCommand::Modifiers {
                shift: false,
                control: false,
                alt,
                super_key: false,
            };
            let key = InputCommand::Key {
                physical_key: "KeyL".to_owned(),
                pressed: true,
                repeat: false,
            };
            log += &line(2, &modifiers);
            log += &line(3, &key);
        }
        log += &line(4, &InputCommand::Tick { now_ms: 16 });
        let mut replay = Replay::new(session).unwrap();
        replay.run(std::io::Cursor::new(log)).unwrap();
        replay.stage().unwrap()
    };
    let menu = title_after(None);
    assert!(menu.contains("main menu"), "{menu}");
    assert_ne!(title_after(Some(false)), menu, "L reaches the menu");
    assert_eq!(title_after(Some(true)), menu, "Alt+L does not");
}

/// §2.6: asking whether a hint may show changes nothing, on a run with it
/// and a run without it, and a menu or a window never allows one.
#[test]
fn spec020_toasts_leave_the_state_hash_and_the_frame_alone() {
    let dir = synthetic_corpus();
    let run = |toasts: bool| {
        let mut session = map_session(dir.path());
        let mut repeat = input::KeyRepeat::default();
        let mut hints = toasts.then(|| toast::HotkeyToasts::new(true));
        let mut hashes = Vec::new();
        let mut shown = 0;
        for step in 1..=200_u64 {
            let now = step * 50;
            session.advance(now, &mut repeat).unwrap();
            if let Some(hints) = &mut hints {
                hints.tick(now, session.hints_ready());
                shown += usize::from(hints.shown().is_some());
            }
            hashes.push(state_hash(&mut session));
        }
        (hashes, session.image().unwrap().pixels().to_vec(), shown)
    };
    let (with_hashes, with_frame, shown) = run(true);
    let (without_hashes, without_frame, _) = run(false);
    assert!(shown > 0, "the hints really showed");
    assert_eq!(with_hashes, without_hashes);
    assert_eq!(with_frame, without_frame, "the frame a screenshot reads");
}

/// The gate: only the free map screen.
#[test]
fn spec020_hints_wait_for_the_free_map_screen() {
    let dir = synthetic_corpus();
    let mut session = map_session(dir.path());
    assert!(session.hints_ready());
    let world = crate::replay::interactive_world(&mut session).unwrap();
    world.host.world.combat = true;
    assert!(!session.hints_ready(), "combat");
    let world = crate::replay::interactive_world(&mut session).unwrap();
    world.host.world.combat = false;
    world.host.world.dialogue.open = true;
    assert!(!session.hints_ready(), "dialogue");
    let world = crate::replay::interactive_world(&mut session).unwrap();
    world.host.world.dialogue.open = false;
    assert!(session.press_key(KeyCode::KeyI, false) == session::KeyPress::Changed);
    assert!(!session.hints_ready(), "a window");
}

/// A synthetic install of each edition, each a real `--game` session.
struct Installs {
    settings: Config,
    _folders: Vec<tempfile::TempDir>,
}
fn installs() -> Installs {
    let corpus = synthetic_corpus();
    let one = synthetic_corpus();
    tables::write_tables(corpus.path());
    let map = std::fs::read(corpus.path().join("maps/synthetic.map")).unwrap();
    std::fs::write(
        corpus.path().join("maps/artemple.map"),
        tables::named_map(map.clone(), "artemple.map", 3, 20100, 2),
    )
    .unwrap();
    std::fs::write(one.path().join("maps/v13ent.map"), map).unwrap();
    let folder = tempfile::tempdir().unwrap();
    let (master, critter) = vault_fixtures::dat2::fallout2_install(corpus.path(), folder.path(), 2);
    let two = GamePaths {
        master_dat: Some(master),
        critter_dat: Some(critter),
        data_root: None,
    };
    let first = GamePaths {
        data_root: Some(one.path().to_path_buf()),
        ..GamePaths::default()
    };
    let settings = Config::with_profiles(
        Fallout1,
        PerGame {
            fallout1: first,
            fallout2: two,
        },
    );
    Installs {
        settings,
        _folders: vec![corpus, one, folder],
    }
}
/// A session of `game` from the settings, as a boot starts one.
fn boot_session(settings: &Config, game: GameProfile, seen: &Path) -> SessionRuntime {
    let mut settings = settings.clone();
    let paths = settings.profile(game);
    settings.choose_game(game, paths);
    let mut args = switch_args(&parse(&["--no-audio"]));
    args.preview.headless = true;
    args.preview.save_dir = Some(seen.to_path_buf());
    args.preview.storage = Some(Arc::new(MemoryStorage::new()));
    let (_, session, _) = start(&mut args, Some(&settings), SessionOrigin::Replay).unwrap();
    session.unwrap()
}
/// The session's checkpoint stream over the start of a new game: the
/// stream needs a world at every step, so it begins on the map.
fn stream(mut session: SessionRuntime, out: &Path) -> String {
    let mut repeat = input::KeyRepeat::default();
    session.press_key(KeyCode::KeyN, false);
    session.press_key(KeyCode::KeyT, false);
    for step in 1..=40 {
        if crate::replay::interactive_world(&mut session).is_some() {
            break;
        }
        session.advance(step * 16, &mut repeat).unwrap();
    }
    assert!(crate::replay::interactive_world(&mut session).is_some());
    let line = |id: u64, command: &InputCommand| {
        let mut ids = vault_core::SequenceIds::from_last(id - 1, 0);
        let mut line = Vec::new();
        write_command(&mut line, &mut ids, command).unwrap();
        String::from_utf8(line).unwrap()
    };
    let surface = InputCommand::Surface {
        width: 640,
        height: 480,
    };
    let mut log = line(1, &surface);
    for step in 0..40 {
        log += &line(
            2 + step,
            &InputCommand::Tick {
                now_ms: 16 * (step + 1),
            },
        );
    }
    let mut replay = Replay::with_checkpoints(
        session,
        CheckpointOptions {
            out: Some(out.to_path_buf()),
            expected: None,
        },
    )
    .unwrap();
    replay.run(std::io::Cursor::new(log)).unwrap();
    replay.finish(None).unwrap();
    std::fs::read_to_string(out).unwrap()
}

/// §3 criterion 5: a warm switch from each edition to the other, then a cold
/// start of it with the same settings and seed, give the same stream.
#[test]
fn spec020_a_warm_switch_is_a_cold_start() {
    let installs = installs();
    let temp = tempfile::tempdir().unwrap();
    // The comparison below can tell two sessions apart.
    let streams = [Fallout1, Fallout2].map(|game| {
        let session = boot_session(&installs.settings, game, temp.path());
        stream(session, &temp.path().join("edition.tsv"))
    });
    assert_ne!(streams[0], streams[1]);
    for (from, to) in [(Fallout1, Fallout2), (Fallout2, Fallout1)] {
        let cold = stream(
            boot_session(&installs.settings, to, temp.path()),
            &temp.path().join("cold.tsv"),
        );
        assert!(cold.lines().count() > 1, "{cold}");
        // A used session of the other edition, ended as a switch ends it.
        let mut running = boot_session(&installs.settings, from, temp.path());
        let mut repeat = input::KeyRepeat::default();
        for step in 1..=20 {
            running.advance(step * 16, &mut repeat).unwrap();
        }
        crate::interface::note_once("from the old session", "a test note");
        let mut shell = Shell {
            session: Some(running),
            started: Some(Instant::now()),
            ..Default::default()
        };
        shell.stop_session(selector(from));
        shell.end_session();
        let warm = stream(
            boot_session(&installs.settings, to, temp.path()),
            &temp.path().join("warm.tsv"),
        );
        assert_eq!(warm, cold, "{from:?} to {to:?}");
    }
}

/// `last_played` and `hotkey_hints` round trip, and a stranger's text in
/// `last_played` stays as it was written.
#[test]
fn spec023_the_phase_l1_keys_are_settings() {
    let config = Config::parse("last_played = \"fallout2\"\nhotkey_hints = \"always\"\n").unwrap();
    assert_eq!(config.last_played.as_deref(), Some("fallout2"));
    assert_eq!(config.hotkey_hints, Some(HotkeyHints::Always));
    let again = Config::parse(&config.to_toml().unwrap()).unwrap();
    assert_eq!(again, config);
    let odd = Config::parse("last_played = \"bad\"\n").unwrap();
    assert_eq!(odd.last_played.as_deref(), Some("bad"));
    assert!(odd.to_toml().unwrap().contains("last_played = \"bad\""));
    // Not a string is no choice either, and the file still parses.
    let number = Config::parse("last_played = 3\n").unwrap();
    assert_eq!(number.last_played, None);
    assert!(number.to_toml().unwrap().contains("last_played = 3"));
    assert!(Config::parse("hotkey_hints = \"sometimes\"\n").is_err());
    assert_eq!(Config::default().hotkey_hints, None);
}
