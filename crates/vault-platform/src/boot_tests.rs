//! SPEC-020 §2.4 on the boot decision, discovery, and the two screens,
//! driven directly. `tests/fixtures/spec020/` holds the rows.
use super::*;
use crate::config::Preset;

fn per<T>(fallout1: T, fallout2: T) -> PerGame<T> {
    PerGame { fallout1, fallout2 }
}
fn paths(name: &str) -> GamePaths {
    GamePaths {
        master_dat: Some(PathBuf::from(name)),
        ..GamePaths::default()
    }
}
fn both() -> Installs {
    per(Ok(paths("one")), Ok(paths("two")))
}
fn only_first() -> Installs {
    per(Ok(paths("one")), Err(NOT_FOUND.to_owned()))
}
fn key(key: KeyCode) -> LauncherInput {
    LauncherInput::Key { key, shift: false }
}
/// A folder with `master.dat` and `critter.dat` of each edition's format.
fn installs() -> (tempfile::TempDir, tempfile::TempDir) {
    let (one, two) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    vault_fixtures::dat::fallout1_install(one.path());
    vault_fixtures::dat2::launcher_install(two.path(), &vault_formats::fallout2::REQUIRED_MEMBERS);
    (one, two)
}
fn dirs(one: &Path, two: &Path) -> PerGame<Vec<PathBuf>> {
    per(vec![one.to_path_buf()], vec![two.to_path_buf()])
}

/// Every row of `boot-decision.tsv`.
#[test]
fn spec020_boot_decision_table() {
    let table = include_str!("../../../tests/fixtures/spec020/boot-decision.tsv");
    let mut rows = 0;
    for row in table.lines().skip(1) {
        let cells: Vec<&str> = row.split('\t').collect();
        let [case, parses, one, two, last, outcome, edition] = cells[..] else {
            panic!("{row}");
        };
        let yes = |cell: &str| cell == "yes";
        let game = || config::parse_game(edition).unwrap();
        let want = match outcome {
            "selector" => Decision::Selector,
            "banner" => Decision::Banner(game()),
            "boot" => Decision::Start(game()),
            "launcher" => Decision::Launcher,
            other => panic!("{other}"),
        };
        let last = (last != "-").then_some(last);
        let got = decide(yes(parses), per(yes(one), yes(two)), last);
        assert_eq!(got, want, "{case}");
        rows += 1;
    }
    assert_eq!(rows, 17);
}

/// One install boots at once; both, on a first run, stop and ask.
#[test]
fn spec020_discovery_finds_each_edition_by_its_format_alone() {
    let (one, two) = installs();
    let empty = tempfile::tempdir().unwrap();
    let config = Config::default();
    let found = |dirs: &PerGame<Vec<PathBuf>>| {
        let found = discover(&config, dirs);
        per(found.fallout1.is_ok(), found.fallout2.is_ok())
    };
    let both = dirs(one.path(), two.path());
    let shown = found(&both);
    assert!(shown.fallout1 && shown.fallout2);
    assert_eq!(decide(true, shown, None), Decision::Selector);
    assert!(both_found(&config, &both));
    // Each folder counts only for its own edition's format.
    let single = dirs(one.path(), one.path());
    let shown = found(&single);
    assert!(shown.fallout1 && !shown.fallout2);
    assert_eq!(
        decide(true, shown, None),
        Decision::Start(GameProfile::Fallout1)
    );
    assert!(!both_found(&config, &single));
    let single = dirs(two.path(), two.path());
    let shown = found(&single);
    assert!(!shown.fallout1 && shown.fallout2);
    assert_eq!(
        decide(true, shown, Some("fallout1")),
        Decision::Start(GameProfile::Fallout2)
    );
    let none = dirs(empty.path(), empty.path());
    assert_eq!(decide(true, found(&none), None), Decision::Launcher);
}

/// A settings path is taken first, and a broken one falls back to the
/// default locations with no selector row lost.
#[test]
fn spec020_settings_paths_come_before_the_default_locations() {
    let (one, two) = installs();
    let own = config::discover_game(&[one.path().to_path_buf()], GameProfile::Fallout1).unwrap();
    let config = Config::with_profiles(GameProfile::Fallout1, per(own, GamePaths::default()));
    let empty = tempfile::tempdir().unwrap();
    let none = dirs(empty.path(), empty.path());
    assert!(find_edition(&config, &none, GameProfile::Fallout1).is_ok());
    assert_eq!(
        find_edition(&config, &none, GameProfile::Fallout2),
        Err(NOT_FOUND.to_owned())
    );
    let broken = Config {
        master_dat: Some(empty.path().join("gone.dat")),
        critter_dat: Some(empty.path().join("gone2.dat")),
        ..Config::default()
    };
    assert!(find_edition(&broken, &none, GameProfile::Fallout1).is_err());
    let found = dirs(one.path(), two.path());
    assert!(find_edition(&broken, &found, GameProfile::Fallout1).is_ok());
}

#[test]
fn spec020_the_selector_says_what_it_must_and_starts_on_the_right_row() {
    let selector = Selector::new(&Config::default(), both(), None);
    assert_eq!(selector.title(), "Found Fallout 1 and Fallout 2. Pick one.");
    assert_eq!(selector.highlight(), GameProfile::Fallout1);
    let over = Selector::new(&Config::default(), both(), Some(GameProfile::Fallout1));
    assert_eq!(over.title(), "Switch game. Unsaved progress is lost.");
    assert_eq!(selector.row(GameProfile::Fallout1).label(1), "1  Fallout 1");
    assert_eq!(selector.row(GameProfile::Fallout2).label(2), "2  Fallout 2");
    // `last_played`, else `game`, else Fallout 1.
    let config = Config {
        game: Some(GameProfile::Fallout2),
        ..Config::default()
    };
    let start = |config: &Config| Selector::new(config, both(), None).highlight();
    assert_eq!(start(&config), GameProfile::Fallout2);
    let played = Config {
        last_played: Some("fallout1".to_owned()),
        ..config.clone()
    };
    assert_eq!(start(&played), GameProfile::Fallout1);
    let bad = Config {
        last_played: Some("bad".to_owned()),
        ..config
    };
    assert_eq!(start(&bad), GameProfile::Fallout2);
    // A row counts the enabled mods, and says when its edition is missing.
    let mods = Config {
        mod_list: Some(ModList::Paths(vec!["a".into(), "b".into()])),
        ..Config::default()
    };
    let selector = Selector::new(&mods, only_first(), None);
    assert_eq!(
        selector.row(GameProfile::Fallout1).label(1),
        "1  Fallout 1  (2 mods)"
    );
    assert_eq!(
        selector.row(GameProfile::Fallout2).label(2),
        "2  Fallout 2  not found"
    );
    // It draws something for each.
    let image = selector.image();
    assert_eq!([image.width(), image.height()], [640, 480]);
    assert!(image.pixels().contains(&color::ACCENT));
}

#[test]
fn spec020_the_selector_takes_keys_and_the_mouse() {
    use GameProfile::{Fallout1, Fallout2};
    let mut selector = Selector::new(&Config::default(), both(), None);
    // 1 and 2 choose at once.
    assert_eq!(
        selector.handle(&key(KeyCode::Digit1)),
        Some(BootAction::Choose(Fallout1))
    );
    assert_eq!(
        selector.handle(&key(KeyCode::Digit2)),
        Some(BootAction::Choose(Fallout2))
    );
    // Up, Down, Left and Right move the highlight, and Enter chooses it.
    assert_eq!(selector.handle(&key(KeyCode::ArrowDown)), None);
    assert_eq!(selector.highlight(), Fallout2);
    assert_eq!(selector.handle(&key(KeyCode::ArrowLeft)), None);
    assert_eq!(selector.highlight(), Fallout1);
    assert_eq!(selector.handle(&key(KeyCode::ArrowRight)), None);
    assert_eq!(selector.handle(&key(KeyCode::ArrowUp)), None);
    assert_eq!(selector.highlight(), Fallout1);
    assert_eq!(
        selector.handle(&key(KeyCode::Enter)),
        Some(BootAction::Choose(Fallout1))
    );
    assert_eq!(
        selector.handle(&key(KeyCode::Escape)),
        Some(BootAction::Leave)
    );
    // A pointer over a row highlights it, and a click chooses it.
    let [x, y, _, _] = Selector::rect(1);
    let on_second = [x + 4, y + 4];
    assert_eq!(selector.handle(&LauncherInput::Pointer(on_second)), None);
    assert_eq!(selector.highlight(), Fallout2);
    assert_eq!(
        selector.handle(&LauncherInput::Press(on_second)),
        Some(BootAction::Choose(Fallout2))
    );
    assert_eq!(selector.handle(&LauncherInput::Press([0, 0])), None);
    // A disabled row does nothing, however it is chosen.
    let mut selector = Selector::new(&Config::default(), only_first(), None);
    assert_eq!(selector.handle(&key(KeyCode::Digit2)), None);
    assert_eq!(selector.handle(&LauncherInput::Press(on_second)), None);
    selector.handle(&key(KeyCode::ArrowDown));
    assert_eq!(selector.handle(&key(KeyCode::Enter)), None);
}

/// §4.1's cases S3, S4, S5, S6 and S7, on the selector over a session.
#[test]
fn spec020_switch_cases_on_the_selector() {
    use GameProfile::{Fallout1, Fallout2};
    let table = include_str!("../../../tests/fixtures/spec020/switch-cases.tsv");
    assert_eq!(table.lines().count(), 14, "S1 to S13");
    let mut selector = Selector::new(&Config::default(), both(), Some(Fallout1));
    // S2: Esc. S4: the running edition, by its key or by Enter.
    assert_eq!(
        selector.handle(&key(KeyCode::Escape)),
        Some(BootAction::Leave)
    );
    assert_eq!(
        selector.handle(&key(KeyCode::Digit1)),
        Some(BootAction::Leave)
    );
    assert_eq!(
        selector.handle(&key(KeyCode::Enter)),
        Some(BootAction::Leave)
    );
    // S5: the other one.
    assert_eq!(
        selector.handle(&key(KeyCode::Digit2)),
        Some(BootAction::Choose(Fallout2))
    );
    // S7: it failed validation at that moment: the row says why, and is off.
    selector.fail(Fallout2, "MASTER.DAT: file not found.".to_owned());
    assert_eq!(
        selector.row(Fallout2).label(2),
        "2  Fallout 2  MASTER.DAT: file not found."
    );
    assert_eq!(selector.handle(&key(KeyCode::Digit2)), None);
    // S6: not found.
    let mut selector = Selector::new(&Config::default(), only_first(), Some(Fallout1));
    assert_eq!(selector.handle(&key(KeyCode::Digit2)), None);
}

#[test]
fn spec020_the_banner_counts_three_seconds_of_shell_time() {
    let mut banner = Banner::new(GameProfile::Fallout2);
    // The clock starts at its first frame, not at zero.
    assert!(!banner.tick(5000));
    assert_eq!(
        banner.text(5000),
        "Starting Fallout 2 in 3. Press any key to switch."
    );
    assert_eq!(banner.seconds(6000), 2);
    assert_eq!(banner.seconds(6001), 2);
    assert_eq!(banner.seconds(7000), 1);
    assert_eq!(
        banner.text(7999),
        "Starting Fallout 2 in 1. Press any key to switch."
    );
    assert!(!banner.tick(7999));
    assert!(banner.tick(8000));
    assert_eq!(
        Banner::new(GameProfile::Fallout1).text(0),
        "Starting Fallout 1 in 3. Press any key to switch."
    );
    assert_eq!(
        [banner.image(0).width(), banner.image(0).height()],
        [640, 480]
    );
}

#[test]
fn spec020_the_banner_stops_for_a_key_or_a_button_only() {
    for stops in [
        key(KeyCode::KeyA),
        key(KeyCode::Enter),
        key(KeyCode::Space),
        key(KeyCode::Escape),
        LauncherInput::Press([-1, -1]),
    ] {
        assert!(Banner::stops(&stops), "{stops:?}");
    }
    for ignored in [
        key(KeyCode::ShiftLeft),
        key(KeyCode::ControlRight),
        key(KeyCode::AltLeft),
        key(KeyCode::SuperLeft),
        LauncherInput::Pointer([10, 10]),
        LauncherInput::Wheel(1.0),
    ] {
        assert!(!Banner::stops(&ignored), "{ignored:?}");
    }
}

/// `game` and `last_played` go in one commit that keeps the rest of the
/// file; a failed commit says why and leaves the old file whole.
#[test]
fn spec020_a_choice_commits_game_and_last_played_together() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(config::FILE_NAME);
    let mut settings = Config {
        preset: Some(Preset::Enhanced),
        ..Config::default()
    };
    settings.commit(&path).unwrap();
    settings.choose_game(GameProfile::Fallout2, paths("/data/master.dat"));
    settings.last_played = Some("fallout2".to_owned());
    assert_eq!(
        commit_choice(&settings, &[dir.path().to_path_buf()]),
        Ok(path.clone())
    );
    let read = Config::load(&path).unwrap();
    assert_eq!(read.game, Some(GameProfile::Fallout2));
    assert_eq!(read.last_played.as_deref(), Some("fallout2"));
    assert_eq!(read.preset, Some(Preset::Enhanced));
    assert_eq!(
        read.master_dat,
        Some(PathBuf::from("/data/master.dat")),
        "the choice's paths"
    );
    // A first choice with no file writes a new one.
    let fresh = tempfile::tempdir().unwrap();
    let written = commit_choice(&settings, &[fresh.path().to_path_buf()]).unwrap();
    assert_eq!(written, fresh.path().join(config::FILE_NAME));
    // Nowhere to write is an error, not a stop.
    assert!(commit_choice(&settings, &[fresh.path().join("gone")]).is_err());
}

#[test]
fn spec020_hints_commit_keeps_the_file_and_defaults_to_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(config::FILE_NAME);
    assert_eq!(hint_setting(None), HotkeyHints::Once);
    let settings = Config {
        last_played: Some("bad".to_owned()),
        ..Config::default()
    };
    assert_eq!(hint_setting(Some(&settings)), HotkeyHints::Once);
    settings.commit(&path).unwrap();
    commit_hints(&path, HotkeyHints::Off).unwrap();
    let read = Config::load(&path).unwrap();
    assert_eq!(read.hotkey_hints, Some(HotkeyHints::Off));
    assert_eq!(read.last_played.as_deref(), Some("bad"), "kept as written");
    assert!(commit_hints(&dir.path().join("gone/vault-engine.toml"), HotkeyHints::Off).is_err());
}

#[test]
fn spec020_the_mod_count_reads_the_list_and_counts_the_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let list = dir.path().join("load_order.toml");
    std::fs::write(
        &list,
        "[[layer]]\npath = \"mods/a\"\n[[layer]]\npath = \"mods/b\"\nenabled = false\n",
    )
    .unwrap();
    let config = Config {
        mod_list: Some(ModList::File(list)),
        ..Config::default()
    };
    assert_eq!(enabled_mods(&config), 1);
    assert_eq!(enabled_mods(&Config::default()), 0);
    let missing = Config {
        mod_list: Some(ModList::File(dir.path().join("gone.toml"))),
        ..Config::default()
    };
    assert_eq!(enabled_mods(&missing), 0, "it never stops a boot");
}
