use super::*;
use sha2::{Digest, Sha256};
use std::fs;

/// An archive member: its name and bytes.
type Member<'a> = (&'a str, &'a [u8]);

/// A raw (uncompressed) synthetic DAT archive: directories with members.
fn archive(directories: &[(&str, &[Member])]) -> Vec<u8> {
    let count = directories.len() as u32;
    let mut tables: Vec<u8> = [count, count, 0, 0]
        .iter()
        .flat_map(|word| word.to_be_bytes())
        .collect();
    for (name, _) in directories {
        tables.push(name.len() as u8);
        tables.extend(name.as_bytes());
    }
    let table_len = tables.len()
        + directories
            .iter()
            .map(|(_, files)| 16 + files.iter().map(|(name, _)| 17 + name.len()).sum::<usize>())
            .sum::<usize>();
    let mut offset = table_len as u32;
    let mut payload: Vec<u8> = Vec::new();
    for (_, files) in directories {
        let count = files.len() as u32;
        for word in [count, count, 16, 0] {
            tables.extend(word.to_be_bytes());
        }
        for (name, body) in *files {
            tables.push(name.len() as u8);
            tables.extend(name.as_bytes());
            for word in [0x20, offset, body.len() as u32, body.len() as u32] {
                tables.extend(word.to_be_bytes());
            }
            offset += body.len() as u32;
            payload.extend(*body);
        }
    }
    tables.extend(payload);
    tables
}

fn master_archive() -> Vec<u8> {
    archive(&[
        (".", &[("COLOR.PAL", b"pal")]),
        ("DATA", &[("VAULT13.GAM", b"gam")]),
        ("MAPS", &[("V13ENT.MAP", b"map")]),
    ])
}

fn critter_archive() -> Vec<u8> {
    archive(&[("ART\\CRITTERS", &[("HERO.FRM", b"frm")])])
}

/// The SPEC-008 synthetic archive fixture: two text members only.
fn fixture_archive() -> Vec<u8> {
    crate::fixture_assets::sparse(include_str!(
        "../../../../tests/fixtures/archive/synthetic.dat.hex"
    ))
}

/// A game folder with both databases (and the given extra files).
fn install(dir: &Path) {
    fs::write(dir.join("MASTER.DAT"), master_archive()).unwrap();
    fs::write(dir.join("CRITTER.DAT"), critter_archive()).unwrap();
}

fn setup(dir: &Path, config: Config) -> Setup {
    Setup {
        config,
        settings_path: None,
        save_dirs: vec![dir.to_path_buf()],
        start_dir: dir.to_path_buf(),
        problem: None,
    }
}

fn key(launcher: &mut Launcher, key: KeyCode) -> Option<LauncherAction> {
    launcher.handle(LauncherInput::Key { key, shift: false })
}

fn type_text(launcher: &mut Launcher, text: &str) {
    for byte in text.bytes() {
        let found = KEYS.iter().find_map(|code| {
            [false, true]
                .into_iter()
                .find(|shift| field_text(*code, *shift) == Some(byte))
                .map(|shift| (*code, shift))
        });
        let (key, shift) = found.unwrap_or_else(|| panic!("no key types {:?}", byte as char));
        launcher.handle(LauncherInput::Key { key, shift });
    }
}

/// The keys `type_text` searches.
const KEYS: [KeyCode; 48] = {
    use KeyCode::*;
    [
        KeyA,
        KeyB,
        KeyC,
        KeyD,
        KeyE,
        KeyF,
        KeyG,
        KeyH,
        KeyI,
        KeyJ,
        KeyK,
        KeyL,
        KeyM,
        KeyN,
        KeyO,
        KeyP,
        KeyQ,
        KeyR,
        KeyS,
        KeyT,
        KeyU,
        KeyV,
        KeyW,
        KeyX,
        KeyY,
        KeyZ,
        Digit0,
        Digit1,
        Digit2,
        Digit3,
        Digit4,
        Digit5,
        Digit6,
        Digit7,
        Digit8,
        Digit9,
        Space,
        Minus,
        Equal,
        BracketLeft,
        BracketRight,
        Backslash,
        Semicolon,
        Quote,
        Comma,
        Period,
        Slash,
        Backquote,
    ]
};

#[test]
fn the_font_covers_printable_ascii_in_at_most_a_kilobyte() {
    assert!(std::mem::size_of_val(&font::GLYPHS) <= 1024);
    assert_eq!(font::GLYPHS.len(), 95 + 2);
    let mut seen = std::collections::BTreeSet::new();
    for c in (' '..='~').chain([font::CHECK, font::CROSS]) {
        let rows = font::glyph(c);
        // Five columns wide; every glyph but the space draws something,
        // and no two glyphs look alike.
        assert!(rows.iter().all(|row| row & !0x1F == 0), "{c:?}");
        assert_eq!(rows.iter().all(|row| *row == 0), c == ' ', "{c:?}");
        assert!(seen.insert(*rows), "{c:?} repeats another glyph");
    }
    // Outside the font: the question mark.
    assert_eq!(font::glyph('é'), font::glyph('?'));
    assert_eq!(font::width("Play"), 24);
    assert_eq!(
        font::clip_start("/a/long/path/MASTER.DAT", 12),
        "..MASTER.DAT"
    );
    assert_eq!(font::clip_end("a long reason", 8), "a long..");
}

#[test]
fn text_keys_follow_the_us_layout() {
    assert_eq!(field_text(KeyCode::KeyA, false), Some(b'a'));
    assert_eq!(field_text(KeyCode::KeyZ, true), Some(b'Z'));
    assert_eq!(field_text(KeyCode::Digit2, true), Some(b'@'));
    assert_eq!(field_text(KeyCode::Semicolon, true), Some(b':'));
    assert_eq!(field_text(KeyCode::Backslash, false), Some(b'\\'));
    assert_eq!(field_text(KeyCode::Numpad7, true), Some(b'7'));
    assert_eq!(field_text(KeyCode::Enter, false), None);
    assert_eq!(field_text(KeyCode::ShiftLeft, true), None);
}

#[test]
fn the_databases_validate_with_counts_and_reasons() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    // The SPEC-008 fixture opens, but holds none of what a game reads.
    fs::write(dir.join("fixture.dat"), fixture_archive()).unwrap();
    let check = validate::master(&dir.join("fixture.dat"), None);
    assert_eq!(check.state, State::Bad);
    assert_eq!(
        check.text,
        "2 members; missing color.pal, data/vault13.gam, maps/v13ent.map"
    );
    let check = validate::critter(&dir.join("fixture.dat"));
    assert_eq!(
        (check.state, check.text.as_str()),
        (State::Bad, "2 members; none under art/critters/")
    );
    // Archives shaped like the real ones.
    install(dir);
    let check = validate::master(&dir.join("MASTER.DAT"), None);
    assert_eq!((check.state, check.text.as_str()), (State::Ok, "3 members"));
    let check = validate::critter(&dir.join("CRITTER.DAT"));
    assert_eq!(
        (check.state, check.text.as_str()),
        (State::Ok, "1 member, 1 critter art")
    );
    // A member missing from the archive may sit in the loose data folder.
    let partial = archive(&[
        ("DATA", &[("VAULT13.GAM", b"gam")]),
        ("MAPS", &[("V13ENT.MAP", b"map")]),
    ]);
    fs::write(dir.join("partial.dat"), partial).unwrap();
    assert!(!validate::master(&dir.join("partial.dat"), None).is_ok());
    fs::create_dir_all(dir.join("DATA")).unwrap();
    fs::write(dir.join("DATA/Color.Pal"), b"pal").unwrap();
    assert!(validate::master(&dir.join("partial.dat"), Some(&dir.join("DATA"))).is_ok());
    // Missing, wrong-kind and invalid paths.
    for (path, reason) in [
        (dir.join("none.dat"), "file not found"),
        (dir.join("DATA"), "not a file"),
    ] {
        assert_eq!(validate::master(&path, None).text, reason);
        assert_eq!(validate::critter(&path).text, reason);
    }
    fs::write(dir.join("junk.dat"), b"not an archive").unwrap();
    let check = validate::master(&dir.join("junk.dat"), None);
    assert!(
        check.state == State::Bad && check.text.starts_with("not a readable DAT archive"),
        "{check:?}"
    );
    assert_eq!(
        validate::data_root(&dir.join("missing"), false).text,
        "directory not found"
    );
    assert_eq!(
        validate::data_root(&dir.join("MASTER.DAT"), false).text,
        "not a directory"
    );
    assert!(validate::data_root(&dir.join("DATA"), false).is_ok());
    // Alone, the loose folder must hold everything a new game reads.
    let alone = validate::data_root(&dir.join("DATA"), true);
    assert_eq!(
        alone.text,
        "no databases, and missing data/vault13.gam, maps/v13ent.map"
    );
}

#[test]
fn the_fields_validate_live_and_gate_play() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    install(dir);
    let mut launcher = Launcher::new(setup(dir, Config::default()));
    assert_eq!(launcher.focus, Widget::Field(Field::Master));
    assert!(launcher.ready().is_err());
    // Typing a path validates it at each key.
    type_text(&mut launcher, &dir.join("MASTER.DA").to_string_lossy());
    assert_eq!(launcher.checks[0].text, "file not found");
    type_text(&mut launcher, "T");
    assert!(launcher.checks[0].is_ok());
    // MASTER.DAT alone is not enough.
    assert_eq!(launcher.checks[1].text, "required with MASTER.DAT");
    assert_eq!(key(&mut launcher, KeyCode::Enter), None);
    assert_eq!(
        launcher.ready(),
        Err("CRITTER.DAT: required with MASTER.DAT.".into())
    );
    launcher.focus = Widget::Play;
    assert_eq!(key(&mut launcher, KeyCode::Enter), None);
    assert!(
        launcher.status.starts_with("Not ready."),
        "{}",
        launcher.status
    );
    launcher.focus = Widget::Field(Field::Critter);
    type_text(&mut launcher, &dir.join("CRITTER.DAT").to_string_lossy());
    assert!(launcher.checks[1].is_ok());
    assert_eq!(launcher.ready(), Ok(()));
    // Backspace edits, and the check follows.
    key(&mut launcher, KeyCode::Backspace);
    assert!(!launcher.checks[1].is_ok());
}

#[test]
fn dropped_paths_go_to_the_matching_field() {
    let mods = Tab::Mods;
    for (path, dir, tab, target) in [
        (
            "/g/MASTER.DAT",
            false,
            Tab::Data,
            DropTarget::Field(Field::Master),
        ),
        (
            "/g/critter.dat",
            false,
            Tab::Display,
            DropTarget::Field(Field::Critter),
        ),
        (
            "/g/patch000.dat",
            false,
            Tab::Data,
            DropTarget::Field(Field::Master),
        ),
        (
            "/g/data",
            true,
            Tab::Data,
            DropTarget::Field(Field::DataRoot),
        ),
        ("/g/readme.txt", false, Tab::Data, DropTarget::Rejected),
        ("/g/mods/one", true, mods, DropTarget::Layer),
        ("/g/mods/two.DAT", false, mods, DropTarget::Layer),
        ("/g/mods/notes.txt", false, mods, DropTarget::Rejected),
    ] {
        assert_eq!(route_drop(Path::new(path), dir, tab), target, "{path}");
    }
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let mut launcher = Launcher::new(setup(dir, Config::default()));
    // A dropped game folder fills all three fields.
    let game = dir.join("game");
    fs::create_dir_all(game.join("DATA")).unwrap();
    install(&game);
    launcher.handle(LauncherInput::Dropped(game.clone()));
    assert_eq!(
        launcher.fields[0].text,
        game.join("MASTER.DAT").to_string_lossy()
    );
    assert_eq!(
        launcher.fields[1].text,
        game.join("CRITTER.DAT").to_string_lossy()
    );
    assert_eq!(launcher.fields[2].text, game.join("DATA").to_string_lossy());
    assert_eq!(launcher.ready(), Ok(()));
    // A plain folder is the data folder; a file goes by its name.
    let loose = dir.join("loose");
    fs::create_dir_all(&loose).unwrap();
    launcher.handle(LauncherInput::Dropped(loose.clone()));
    assert_eq!(launcher.fields[2].text, loose.to_string_lossy());
    launcher.handle(LauncherInput::Dropped(dir.join("other/critter.dat")));
    assert_eq!(
        launcher.fields[1].text,
        dir.join("other/critter.dat").to_string_lossy()
    );
    assert_eq!(launcher.checks[1].text, "file not found");
    launcher.handle(LauncherInput::Dropped(dir.join("notes.txt")));
    assert_eq!(launcher.status_state, State::Bad);
}

#[test]
fn save_rewrites_the_settings_and_keeps_unknown_keys() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    install(dir);
    let path = dir.join(config::FILE_NAME);
    fs::write(
        &path,
        "master_dat = \"MASTER.DAT\"\ncritter_dat = \"CRITTER.DAT\"\nfuture_key = \"kept\"\n\
         scaling = \"integer\"\n",
    )
    .unwrap();
    let config = Config::load(&path).unwrap();
    let mut launcher = Launcher::new(Setup {
        settings_path: Some(path.clone()),
        ..setup(dir, config)
    });
    assert_eq!(launcher.ready(), Ok(()));
    assert_eq!(launcher.focus, Widget::Play);
    // Display & audio: scaling, fullscreen, audio and the preset change.
    launcher.focus = Widget::Tab(Tab::Data);
    key(&mut launcher, KeyCode::ArrowRight);
    assert_eq!(launcher.tab, Tab::Display);
    for widget in [
        Widget::Scaling,
        Widget::Fullscreen,
        Widget::Audio,
        Widget::Preset,
    ] {
        launcher.focus = widget;
        key(&mut launcher, KeyCode::Space);
    }
    launcher.focus = Widget::Save;
    key(&mut launcher, KeyCode::Enter);
    assert_eq!(launcher.status_state, State::Ok, "{}", launcher.status);
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("master_dat = \"MASTER.DAT\"\n"), "{text}");
    assert!(text.contains("future_key = \"kept\"\n"), "{text}");
    let saved = Config::parse(&text).unwrap();
    assert_eq!(saved.scaling, Some(ScalingMode::AspectFit));
    assert_eq!(saved.fullscreen, Some(true));
    assert_eq!(saved.audio, Some(false));
    assert_eq!(saved.preset, Some(Preset::Enhanced));
    assert_eq!(saved.mod_list, None);
    // A launcher started from the saved file shows the same settings.
    let reloaded = Launcher::new(Setup {
        settings_path: Some(path.clone()),
        ..setup(dir, Config::load(&path).unwrap())
    });
    assert_eq!(reloaded.config, launcher.config);
    // Without a settings file, the first save directory that takes one.
    let fresh = tempfile::tempdir().unwrap();
    let mut launcher = Launcher::new(Setup {
        save_dirs: vec![fresh.path().join("missing"), fresh.path().into()],
        ..setup(fresh.path(), Config::default())
    });
    assert_eq!(launcher.save(), Ok(fresh.path().join(config::FILE_NAME)));
}

#[test]
fn load_order_edits_round_trip_through_the_writer() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    install(dir);
    for name in ["one", "two", "three"] {
        fs::create_dir_all(dir.join("mods").join(name).join("data")).unwrap();
        fs::write(dir.join("mods").join(name).join("data/x.txt"), name).unwrap();
    }
    fs::write(dir.join("mods/four.dat"), critter_archive()).unwrap();
    let config = Config {
        master_dat: Some(dir.join("MASTER.DAT")),
        critter_dat: Some(dir.join("CRITTER.DAT")),
        ..Config::default()
    };
    let mut launcher = Launcher::new(setup(dir, config));
    launcher.tab = Tab::Mods;
    // Typed, browsed and dropped layers.
    launcher.focus = Widget::NewLayer;
    type_text(&mut launcher, "mods/one");
    key(&mut launcher, KeyCode::Enter);
    launcher.handle(LauncherInput::Dropped(dir.join("mods/two")));
    launcher.focus = Widget::BrowseLayer;
    key(&mut launcher, KeyCode::Enter);
    let browser = launcher.browser.as_ref().unwrap();
    assert_eq!(browser.dir, std::path::absolute(dir).unwrap());
    let mods_at = browser
        .entries
        .iter()
        .position(|e| e.name == "mods")
        .unwrap();
    for _ in 0..mods_at {
        key(&mut launcher, KeyCode::ArrowDown);
    }
    key(&mut launcher, KeyCode::Enter);
    let names: Vec<_> = launcher
        .browser
        .as_ref()
        .unwrap()
        .entries
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(names, ["..", "one", "three", "two", "four.dat"]);
    key(&mut launcher, KeyCode::End);
    key(&mut launcher, KeyCode::Enter);
    assert!(launcher.browser.is_none());
    assert_eq!(launcher.focus, Widget::BrowseLayer);
    launcher.handle(LauncherInput::Dropped(dir.join("mods/three")));
    let paths = |launcher: &Launcher| -> Vec<String> {
        launcher
            .mods
            .entries
            .iter()
            .map(|e| e.path.clone().unwrap())
            .collect()
    };
    assert_eq!(
        paths(&launcher),
        ["mods/one", "mods/two", "mods/four.dat", "mods/three"]
    );
    assert!(
        launcher.mods.check.layers.iter().all(Check::is_ok),
        "{:?}",
        launcher.mods.check
    );
    // Reorder, disable and remove from the keyboard.
    launcher.focus = Widget::Layers;
    launcher.handle(LauncherInput::Key {
        key: KeyCode::ArrowUp,
        shift: true,
    });
    assert_eq!(
        paths(&launcher),
        ["mods/one", "mods/two", "mods/three", "mods/four.dat"]
    );
    key(&mut launcher, KeyCode::Space);
    assert!(!launcher.mods.entries[2].enabled);
    assert_eq!(launcher.mods.check.layers[2].state, State::Unset);
    key(&mut launcher, KeyCode::ArrowUp);
    key(&mut launcher, KeyCode::ArrowUp);
    key(&mut launcher, KeyCode::Delete);
    assert_eq!(
        paths(&launcher),
        ["mods/two", "mods/three", "mods/four.dat"]
    );
    // A layer that does not mount is reported and blocks Play.
    launcher.focus = Widget::NewLayer;
    type_text(&mut launcher, "mods/missing");
    key(&mut launcher, KeyCode::Enter);
    assert_eq!(launcher.mods.check.layers[3].state, State::Bad);
    assert!(launcher.ready().unwrap_err().starts_with("Mods:"));
    launcher.focus = Widget::LayerRemove;
    key(&mut launcher, KeyCode::Enter);
    assert_eq!(launcher.ready(), Ok(()));
    assert!(
        launcher
            .mods
            .check
            .stack
            .text
            .starts_with("2 of 3 layers mounted")
    );
    // Save writes the load order beside the settings, relative paths kept.
    let settings = launcher.save().unwrap();
    assert_eq!(settings, dir.join(config::FILE_NAME));
    let text = fs::read_to_string(dir.join("load_order.toml")).unwrap();
    assert_eq!(
        vault_formats::parse_load_order(&text).unwrap(),
        launcher.mods.entries
    );
    assert!(
        text.contains("path = \"mods/three\"\nenabled = false\n"),
        "{text}"
    );
    let saved = Config::load(&settings).unwrap();
    assert_eq!(
        saved.mod_list,
        Some(ModList::File(dir.join("load_order.toml")))
    );
    assert!(
        fs::read_to_string(&settings)
            .unwrap()
            .contains("mod_list = \"load_order.toml\"")
    );
    // A new launcher reads the same layers back.
    let reloaded = Launcher::new(Setup {
        settings_path: Some(settings.clone()),
        ..setup(dir, saved)
    });
    assert_eq!(reloaded.mods.entries, launcher.mods.entries);
    assert_eq!(reloaded.mods.check, launcher.mods.check);
    // An inline path list is shown but stays read-only.
    let inline = Config {
        mod_list: Some(ModList::Paths(vec![dir.join("mods/one")])),
        ..reloaded.config.clone()
    };
    let mut launcher = Launcher::new(setup(dir, inline));
    launcher.tab = Tab::Mods;
    assert!(launcher.mods.read_only.is_some());
    assert!(launcher.mods.check.stack.is_ok());
    launcher.handle(LauncherInput::Dropped(dir.join("mods/two")));
    assert_eq!(launcher.mods.entries.len(), 1);
    assert!(launcher.status.starts_with("Read only"));
}

#[test]
fn the_keyboard_moves_the_focus_in_layout_order() {
    let temp = tempfile::tempdir().unwrap();
    let mut launcher = Launcher::new(setup(temp.path(), Config::default()));
    use {Field as F, Tab as T, Widget as W};
    launcher.focus = W::Tab(T::Data);
    let mut seen = vec![launcher.focus];
    for _ in 0..12 {
        key(&mut launcher, KeyCode::Tab);
        seen.push(launcher.focus);
    }
    assert_eq!(
        seen,
        [
            W::Tab(T::Data),
            W::Tab(T::Display),
            W::Tab(T::Mods),
            W::Field(F::Master),
            W::Browse(F::Master),
            W::Field(F::Critter),
            W::Browse(F::Critter),
            W::Field(F::DataRoot),
            W::Browse(F::DataRoot),
            W::Save,
            W::Play,
            W::PlayDebug,
            W::Quit,
        ]
    );
    key(&mut launcher, KeyCode::Tab);
    assert_eq!(launcher.focus, W::Tab(T::Data));
    launcher.handle(LauncherInput::Key {
        key: KeyCode::Tab,
        shift: true,
    });
    assert_eq!(launcher.focus, W::Quit);
    // Arrows on the tab row switch tabs.
    launcher.focus = W::Tab(T::Data);
    key(&mut launcher, KeyCode::ArrowLeft);
    assert_eq!((launcher.tab, launcher.focus), (T::Mods, W::Tab(T::Mods)));
    key(&mut launcher, KeyCode::ArrowDown);
    assert_eq!(launcher.focus, W::Layers);
    // The browser is modal, and Esc backs out of it to its button.
    launcher.tab = T::Data;
    launcher.focus = W::Browse(F::Critter);
    key(&mut launcher, KeyCode::Space);
    assert_eq!(launcher.focus, W::BrowseList);
    key(&mut launcher, KeyCode::Tab);
    assert_eq!(launcher.focus, W::BrowseUse);
    key(&mut launcher, KeyCode::Tab);
    key(&mut launcher, KeyCode::Tab);
    assert_eq!(launcher.focus, W::BrowseList);
    key(&mut launcher, KeyCode::Escape);
    assert!(launcher.browser.is_none());
    assert_eq!(launcher.focus, W::Browse(F::Critter));
    // A text field keeps its editing keys; Up and Down leave it.
    launcher.focus = W::Field(F::Master);
    type_text(&mut launcher, "ab");
    key(&mut launcher, KeyCode::ArrowLeft);
    type_text(&mut launcher, "X");
    assert_eq!(launcher.fields[0].text, "aXb");
    key(&mut launcher, KeyCode::ArrowDown);
    assert_eq!(launcher.focus, W::Browse(F::Master));
    // The pointer: hover, then a press focuses and activates.
    launcher.handle(LauncherInput::Pointer([350, 420]));
    assert_eq!(launcher.hover, Some(W::Play));
    let quit = launcher.handle(LauncherInput::Press([590, 420]));
    assert_eq!(quit, Some(LauncherAction::Quit));
    launcher.handle(LauncherInput::Press([120, 46]));
    assert_eq!(launcher.tab, T::Display);
}

#[test]
fn play_saves_and_asks_for_the_game() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    install(dir);
    let config = Config {
        master_dat: Some(dir.join("MASTER.DAT")),
        critter_dat: Some(dir.join("CRITTER.DAT")),
        ..Config::default()
    };
    let mut launcher = Launcher::new(setup(dir, config.clone()));
    assert_eq!(
        key(&mut launcher, KeyCode::Enter),
        Some(LauncherAction::Play)
    );
    assert!(dir.join(config::FILE_NAME).is_file());
    launcher.focus = Widget::PlayDebug;
    assert_eq!(
        key(&mut launcher, KeyCode::Space),
        Some(LauncherAction::PlayDebug)
    );
    let launch = launcher.launch_config();
    assert_eq!(launch.master_dat, config.master_dat);
    assert_eq!(launch.critter_dat, config.critter_dat);
    // The failed hand-off is shown.
    launcher.launch_failed("broken");
    assert_eq!(launcher.status, "The game did not start: broken");
}

#[test]
fn the_initial_screen_draws_the_same_frame() {
    let launcher = Launcher::new(Setup {
        config: Config::default(),
        settings_path: None,
        save_dirs: Vec::new(),
        start_dir: PathBuf::from("/nonexistent"),
        problem: None,
    });
    let image = launcher.image();
    assert_eq!((image.width(), image.height()), (640, 480));
    let digest = format!("{:x}", Sha256::digest(image.pixels()));
    assert_eq!(
        digest,
        concat!(
            "35d285d1e3ec2825720f7e021615ab94",
            "cd5db1d90a95af00eab8f74ca7a149ad"
        )
    );
    // Every tab and the browser draw within the palette's own entries.
    let mut launcher = launcher;
    let temp = tempfile::tempdir().unwrap();
    for tab in Tab::ALL {
        launcher.tab = tab;
        assert!(
            launcher
                .image()
                .pixels()
                .iter()
                .all(|p| *p <= view::color::FIELD)
        );
    }
    launcher.browser = Some(browse::Browser::open(
        temp.path().into(),
        browse::Target::Layer,
    ));
    assert!(
        launcher
            .image()
            .pixels()
            .iter()
            .all(|p| *p <= view::color::FIELD)
    );
    assert_eq!(
        palette()[usize::from(view::color::FIELD) + 1],
        [0, 0, 0, 255]
    );
}

#[test]
#[ignore = "needs the local corpus"]
fn the_local_corpus_databases_validate() {
    let root = std::env::var_os("VAULT_CORPUS")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../local-corpus"));
    let Some(found) = config::discover(std::slice::from_ref(&root)) else {
        eprintln!("skipping: no DAT archives in {}", root.display());
        return;
    };
    let launcher = Launcher::new(setup(&root, found));
    for check in &launcher.checks[..2] {
        assert!(check.is_ok(), "{check:?}");
    }
    assert_eq!(launcher.ready(), Ok(()));
    eprintln!("{:?}", launcher.checks);
}
