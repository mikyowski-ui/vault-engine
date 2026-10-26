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
        data_dirs: PerGame::default(),
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
    let check = validate::master(&dir.join("fixture.dat"), None, GameProfile::Fallout1);
    assert_eq!(check.state, State::Bad);
    assert_eq!(
        check.text,
        "2 members; missing color.pal, data/vault13.gam, maps/v13ent.map"
    );
    let check = validate::critter(&dir.join("fixture.dat"), GameProfile::Fallout1);
    assert_eq!(
        (check.state, check.text.as_str()),
        (State::Bad, "2 members; none under art/critters/")
    );
    // Archives shaped like the real ones.
    install(dir);
    let check = validate::master(&dir.join("MASTER.DAT"), None, GameProfile::Fallout1);
    assert_eq!((check.state, check.text.as_str()), (State::Ok, "3 members"));
    let check = validate::critter(&dir.join("CRITTER.DAT"), GameProfile::Fallout1);
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
    assert!(!validate::master(&dir.join("partial.dat"), None, GameProfile::Fallout1).is_ok());
    fs::create_dir_all(dir.join("DATA")).unwrap();
    fs::write(dir.join("DATA/Color.Pal"), b"pal").unwrap();
    assert!(
        validate::master(
            &dir.join("partial.dat"),
            Some(&dir.join("DATA")),
            GameProfile::Fallout1,
        )
        .is_ok()
    );
    // Missing, wrong-kind and invalid paths.
    for (path, reason) in [
        (dir.join("none.dat"), "file not found"),
        (dir.join("DATA"), "not a file"),
    ] {
        assert_eq!(
            validate::master(&path, None, GameProfile::Fallout1).text,
            reason
        );
        assert_eq!(validate::critter(&path, GameProfile::Fallout1).text, reason);
    }
    fs::write(dir.join("junk.dat"), b"not an archive").unwrap();
    let check = validate::master(&dir.join("junk.dat"), None, GameProfile::Fallout1);
    assert!(
        check.state == State::Bad && check.text.starts_with("not a readable DAT archive"),
        "{check:?}"
    );
    assert_eq!(
        validate::data_root(&dir.join("missing"), false, GameProfile::Fallout1).text,
        "directory not found"
    );
    assert_eq!(
        validate::data_root(&dir.join("MASTER.DAT"), false, GameProfile::Fallout1).text,
        "not a directory"
    );
    assert!(validate::data_root(&dir.join("DATA"), false, GameProfile::Fallout1).is_ok());
    // Alone, the loose folder must hold everything a new game reads.
    let alone = validate::data_root(&dir.join("DATA"), true, GameProfile::Fallout1);
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
fn the_game_row_reads_each_games_archives() {
    use vault_fixtures::dat2::{Member, archive, zlib_vector};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    install(dir);
    // Fallout 2 archives (SPEC-015) in their own folder.
    let fallout2 = dir.join("fallout2");
    fs::create_dir_all(&fallout2).unwrap();
    fs::write(
        fallout2.join("master.dat"),
        archive(&[
            Member::stored("COLOR.PAL", b"pal"),
            Member::zlib("TEXT\\ENGLISH\\GAME\\NOTE.MSG", &zlib_vector("text")),
            Member::zlib("DATA\\WORDS.TXT", &zlib_vector("words")),
        ]),
    )
    .unwrap();
    fs::write(
        fallout2.join("critter.dat"),
        archive(&[Member::zlib(
            "ART\\CRITTERS\\HERO.FRM",
            &zlib_vector("repeat"),
        )]),
    )
    .unwrap();
    let config = Config {
        master_dat: Some(dir.join("MASTER.DAT")),
        critter_dat: Some(dir.join("CRITTER.DAT")),
        ..Config::default()
    };
    let mut launcher = Launcher::new(setup(dir, config));
    assert_eq!(launcher.ready(), Ok(()));
    // On the Game data tab, after the fields.
    let order = launcher.order();
    let browse = order
        .iter()
        .position(|widget| *widget == Widget::Browse(Field::DataRoot))
        .unwrap();
    assert_eq!(order[browse + 1], Widget::Game);
    // Fallout 2 has no paths of its own yet, and nothing to discover: its
    // fields are empty. It reads the archives as version 2, so Fallout 1's
    // are refused there, with a hint.
    launcher.focus = Widget::Game;
    key(&mut launcher, KeyCode::ArrowRight);
    assert_eq!(launcher.config.game, Some(GameProfile::Fallout2));
    assert_eq!(launcher.path(Field::Master), None);
    assert!(
        launcher.status.starts_with("No Fallout 2 data found. "),
        "{}",
        launcher.status
    );
    launcher.set_field(Field::Master, &dir.join("MASTER.DAT"));
    launcher.set_field(Field::Critter, &dir.join("CRITTER.DAT"));
    assert_eq!(launcher.checks[0].state, State::Bad);
    assert!(
        launcher.checks[0].text.contains("set Game to fallout1"),
        "{:?}",
        launcher.checks[0]
    );
    assert!(launcher.ready().unwrap_err().starts_with("MASTER.DAT:"));
    // Version 2 archives open, but SPEC-018 §2.2's members are missing.
    launcher.set_field(Field::Master, &fallout2.join("master.dat"));
    launcher.set_field(Field::Critter, &fallout2.join("critter.dat"));
    assert_eq!(launcher.checks[0].state, State::Bad);
    assert!(
        launcher.checks[0].text.starts_with(
            "3 members (Fallout 2 archive); missing data/vault13.gam, maps/artemple.map"
        ),
        "{:?}",
        launcher.checks[0]
    );
    assert_eq!(launcher.checks[1].state, State::Ok);
    let refused = launcher.ready().unwrap_err();
    assert!(refused.contains("data/party.txt"), "{refused}");
    // With every required member, Play unlocks under Fallout 2.
    let mut members = vec![Member::zlib("DATA\\WORDS.TXT", &zlib_vector("words"))];
    for path in vault_formats::fallout2::REQUIRED_MEMBERS {
        members.push(Member::stored(
            &path.to_ascii_uppercase().replace('/', "\\"),
            b"x",
        ));
    }
    fs::write(fallout2.join("master.dat"), archive(&members)).unwrap();
    launcher.revalidate();
    assert_eq!(
        (launcher.checks[0].state, launcher.checks[0].text.as_str()),
        (State::Ok, "8 members (Fallout 2 archive)")
    );
    assert_eq!(launcher.ready(), Ok(()));
    // A patch000.dat that does not open refuses the install.
    fs::write(fallout2.join("PATCH000.DAT"), b"not an archive").unwrap();
    launcher.revalidate();
    assert!(
        launcher.checks[0].text.contains("PATCH000.DAT"),
        "{:?}",
        launcher.checks[0]
    );
    fs::remove_file(fallout2.join("PATCH000.DAT")).unwrap();
    launcher.revalidate();
    // Back to Fallout 1: its own archives come back and validate. Fallout
    // 2's are refused there, with the other hint.
    key(&mut launcher, KeyCode::Space);
    assert_eq!(launcher.config.game, Some(GameProfile::Fallout1));
    assert_eq!(launcher.path(Field::Master), Some(dir.join("MASTER.DAT")));
    assert_eq!(launcher.ready(), Ok(()));
    launcher.set_field(Field::Master, &fallout2.join("master.dat"));
    assert!(
        launcher.checks[0].text.contains("set Game to fallout2"),
        "{:?}",
        launcher.checks[0]
    );
}

/// A Fallout 2 install whose MASTER.DAT holds every SPEC-018 §2.2
/// required member (SPEC-015 archives).
fn fallout2_install(dir: &Path) {
    use vault_fixtures::dat2::{Member, archive, zlib_vector};
    let members: Vec<Member> = vault_formats::fallout2::REQUIRED_MEMBERS
        .iter()
        .map(|path| Member::stored(&path.to_ascii_uppercase().replace('/', "\\"), b"x"))
        .collect();
    fs::write(dir.join("master.dat"), archive(&members)).unwrap();
    fs::write(
        dir.join("critter.dat"),
        archive(&[Member::zlib(
            "ART\\CRITTERS\\HERO.FRM",
            &zlib_vector("repeat"),
        )]),
    )
    .unwrap();
}

#[test]
fn each_game_keeps_its_own_paths_through_the_toggle_and_the_file() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let (fallout1, fallout2) = (dir.join("fo1"), dir.join("fo2"));
    for (folder, make) in [
        (&fallout1, install as fn(&Path)),
        (&fallout2, fallout2_install),
    ] {
        fs::create_dir_all(folder).unwrap();
        make(folder);
    }
    let fo1 = [fallout1.join("MASTER.DAT"), fallout1.join("CRITTER.DAT")];
    let fo2 = [fallout2.join("master.dat"), fallout2.join("critter.dat")];
    let shown = |launcher: &Launcher| [Field::Master, Field::Critter].map(|f| launcher.path(f));
    let named = |paths: &[PathBuf; 2]| paths.clone().map(Some);
    // Each game's directories hold both games' installs: discovery takes
    // only the archives in the game's own format.
    let both = vec![fallout2.clone(), fallout1.clone()];
    let path = dir.join(config::FILE_NAME);
    let mut launcher = Launcher::new(Setup {
        data_dirs: PerGame {
            fallout1: both.clone(),
            fallout2: both,
        },
        ..setup(dir, Config::default())
    });
    // No profile yet: Fallout 1's data is discovered at the start.
    assert!(launcher.discovered());
    assert_eq!(shown(&launcher), named(&fo1));
    assert_eq!(launcher.ready(), Ok(()));
    assert_eq!(
        launcher.status,
        format!(
            "Fallout 1 data found in {}: Enter plays.",
            fallout1.display()
        )
    );
    // One click on Game: Fallout 2's data is discovered for its profile.
    let game = launcher.rect(Widget::Game).unwrap();
    let click = [game.x + 4, game.y + 4];
    launcher.handle(LauncherInput::Press(click));
    assert_eq!(launcher.config.game, Some(GameProfile::Fallout2));
    assert_eq!(shown(&launcher), named(&fo2));
    assert_eq!(launcher.ready(), Ok(()));
    assert!(launcher.checks[..2].iter().all(Check::is_ok));
    assert!(
        launcher.status.starts_with("Fallout 2 data found in "),
        "{}",
        launcher.status
    );
    // From here on nothing is discovered: each click restores the other
    // game's own validated archives.
    launcher.data_dirs = PerGame::default();
    for (game, paths) in [
        (GameProfile::Fallout1, &fo1),
        (GameProfile::Fallout2, &fo2),
        (GameProfile::Fallout1, &fo1),
    ] {
        launcher.handle(LauncherInput::Press(click));
        assert_eq!(launcher.config.game, Some(game));
        assert_eq!(shown(&launcher), named(paths));
        assert!(launcher.checks[..2].iter().all(Check::is_ok));
        assert_eq!(
            launcher.status,
            format!("{} paths restored: ready to play.", title(game))
        );
    }
    // Saved and loaded, then toggled and saved again, round after round:
    // the active game's paths are the top-level keys and each game keeps
    // its own table.
    assert_eq!(launcher.save(), Ok(path.clone()));
    for (active, paths, other, other_paths) in [
        (GameProfile::Fallout1, &fo1, GameProfile::Fallout2, &fo2),
        (GameProfile::Fallout2, &fo2, GameProfile::Fallout1, &fo1),
        (GameProfile::Fallout1, &fo1, GameProfile::Fallout2, &fo2),
    ] {
        let text = fs::read_to_string(&path).unwrap();
        for (name, master) in [
            ("fallout1", "fo1/MASTER.DAT"),
            ("fallout2", "fo2/master.dat"),
        ] {
            let table = format!("[profiles.{name}]\nmaster_dat = \"{master}\"\n");
            assert!(text.contains(&table), "{text}");
        }
        let top = format!(
            "master_dat = \"{}\"\n",
            paths[0].strip_prefix(dir).unwrap().display()
        );
        assert!(
            text.starts_with(&format!("# Vault Engine launcher settings.\n{top}")),
            "{text}"
        );
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.game, Some(active));
        assert_eq!(loaded.master_dat.as_ref(), Some(&paths[0]));
        assert_eq!(
            loaded.profile(other).master_dat.as_ref(),
            Some(&other_paths[0])
        );
        assert_eq!(
            loaded.profile(other).critter_dat.as_ref(),
            Some(&other_paths[1])
        );
        let mut reloaded = Launcher::new(Setup {
            settings_path: Some(path.clone()),
            ..setup(dir, loaded)
        });
        assert!(!reloaded.discovered());
        assert_eq!(shown(&reloaded), named(paths));
        assert_eq!(reloaded.ready(), Ok(()));
        reloaded.handle(LauncherInput::Press(click));
        assert_eq!(shown(&reloaded), named(other_paths));
        assert_eq!(reloaded.ready(), Ok(()));
        assert_eq!(reloaded.save(), Ok(path.clone()));
    }
}

#[test]
fn mod_layers_show_their_manifest_name_and_enhanced_badge() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    install(dir);
    let manifest = |name: &str, text: &str| {
        fs::create_dir_all(dir.join("mods").join(name)).unwrap();
        fs::write(dir.join("mods").join(name).join("vault-mod.toml"), text).unwrap();
    };
    manifest(
        "camera",
        "name = \"Tactical camera\"\n[enhanced]\nroof_peek = \"cursor_xray\"",
    );
    manifest("maps", "name = \"New maps\"");
    manifest("broken", "name = \"Broken\"\n[enhanced]\nzoom = 9");
    fs::create_dir_all(dir.join("mods/plain/data")).unwrap();
    fs::write(dir.join("mods/plain/data/x.txt"), "x").unwrap();
    fs::write(
        dir.join("load_order.toml"),
        "layers = [\"mods/camera\", \"mods/maps\", \"mods/plain\", \"mods/broken\"]",
    )
    .unwrap();
    let config = Config {
        master_dat: Some(dir.join("MASTER.DAT")),
        critter_dat: Some(dir.join("CRITTER.DAT")),
        mod_list: Some(ModList::File(dir.join("load_order.toml"))),
        ..Config::default()
    };
    let launcher = Launcher::new(setup(dir, config));
    let check = &launcher.mods.check;
    let names: Vec<String> = launcher
        .mods
        .entries
        .iter()
        .zip(&check.manifests)
        .map(|(entry, manifest)| view::layer_name(entry, manifest.as_ref()))
        .collect();
    assert_eq!(
        names,
        [
            "Tactical camera [enhanced] (mods/camera)",
            "New maps (mods/maps)",
            "mods/plain",
            "mods/broken",
        ]
    );
    // The malformed manifest fails its layer (it would stop the launch).
    let states: Vec<State> = check.layers.iter().map(|check| check.state).collect();
    assert_eq!(states, [State::Ok, State::Ok, State::Ok, State::Bad]);
    assert!(
        check.layers[3].text.contains("zoom"),
        "{:?}",
        check.layers[3]
    );
    assert!(launcher.ready().unwrap_err().starts_with("Mods:"));
}

#[test]
fn the_keyboard_moves_the_focus_in_layout_order() {
    let temp = tempfile::tempdir().unwrap();
    let mut launcher = Launcher::new(setup(temp.path(), Config::default()));
    use {Field as F, Tab as T, Widget as W};
    launcher.focus = W::Tab(T::Data);
    let mut seen = vec![launcher.focus];
    for _ in 0..13 {
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
            W::Game,
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
fn the_enhanced_display_rows_show_step_and_save_with_the_enhanced_preset() {
    use crate::config::UiScale;
    use crate::enhanced::{RoofPeek, ViewportMode};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let mut launcher = Launcher::new(setup(dir, Config::default()));
    launcher.tab = Tab::Display;
    let enhanced = [
        Widget::Viewport,
        Widget::Zoom,
        Widget::SmoothCamera,
        Widget::RoofPeek,
        Widget::UiScale,
    ];
    // Vanilla: hidden, out of the focus order, and the tab draws as before.
    launcher.focus = Widget::Preset;
    let vanilla = launcher.image();
    assert!(
        enhanced
            .iter()
            .all(|widget| !launcher.order().contains(widget))
    );
    key(&mut launcher, KeyCode::Space);
    assert_eq!(launcher.config.preset, Some(Preset::Enhanced));
    // Enhanced: the rows follow Debug in the focus order.
    let order = launcher.order();
    let debug = order
        .iter()
        .position(|widget| *widget == Widget::Debug)
        .unwrap();
    assert_eq!(&order[debug + 1..debug + 6], &enhanced);
    assert_eq!(order[debug + 6], Widget::Save);
    launcher.focus = Widget::Debug;
    for widget in enhanced {
        key(&mut launcher, KeyCode::Tab);
        assert_eq!(launcher.focus, widget);
    }
    assert_ne!(launcher.image().pixels(), vanilla.pixels());
    assert!(
        launcher
            .image()
            .pixels()
            .iter()
            .all(|p| *p <= view::color::FIELD)
    );
    // The defaults, then each choice.
    assert_eq!(
        launcher.config.enhanced_settings(),
        crate::enhanced::EnhancedSettings::default()
    );
    launcher.focus = Widget::Viewport;
    key(&mut launcher, KeyCode::ArrowRight);
    assert_eq!(launcher.config.viewport_mode, Some(ViewportMode::Fixed));
    // The zoom steps through its stops, both ways, wrapping at the ends.
    launcher.focus = Widget::Zoom;
    let mut zooms = Vec::new();
    for _ in 0..8 {
        key(&mut launcher, KeyCode::ArrowRight);
        zooms.push(launcher.config.default_zoom.unwrap());
    }
    assert_eq!(zooms, [1.25, 1.5, 2.0, 2.5, 3.0, 0.5, 0.75, 1.0]);
    key(&mut launcher, KeyCode::ArrowLeft);
    assert_eq!(launcher.config.default_zoom, Some(0.75));
    // A value from the file between stops moves from the nearest one.
    launcher.config.default_zoom = Some(1.1);
    key(&mut launcher, KeyCode::ArrowRight);
    assert_eq!(launcher.config.default_zoom, Some(1.25));
    launcher.focus = Widget::SmoothCamera;
    key(&mut launcher, KeyCode::Enter);
    assert_eq!(launcher.config.smooth_camera, Some(false));
    launcher.focus = Widget::RoofPeek;
    key(&mut launcher, KeyCode::Space);
    assert_eq!(launcher.config.roof_peek, Some(RoofPeek::CursorXray));
    key(&mut launcher, KeyCode::ArrowLeft);
    key(&mut launcher, KeyCode::ArrowLeft);
    assert_eq!(launcher.config.roof_peek, Some(RoofPeek::Hidden));
    launcher.focus = Widget::UiScale;
    let mut scales = Vec::new();
    for _ in 0..6 {
        key(&mut launcher, KeyCode::ArrowRight);
        scales.push(launcher.config.ui_scale.unwrap());
    }
    assert_eq!(
        scales,
        [
            UiScale::Scale(1.0),
            UiScale::Scale(1.5),
            UiScale::Scale(2.0),
            UiScale::Scale(2.5),
            UiScale::Scale(3.0),
            UiScale::Auto,
        ]
    );
    key(&mut launcher, KeyCode::ArrowLeft);
    assert_eq!(launcher.config.ui_scale, Some(UiScale::Scale(3.0)));
    // A click on a row steps it too.
    let rect = launcher.rect(Widget::Zoom).unwrap();
    launcher.handle(LauncherInput::Press([rect.x + 4, rect.y + 4]));
    assert_eq!(launcher.config.default_zoom, Some(1.5));
    // Save writes them, and they read back.
    assert_eq!(launcher.save(), Ok(dir.join(config::FILE_NAME)));
    let text = fs::read_to_string(dir.join(config::FILE_NAME)).unwrap();
    for line in [
        "preset = \"enhanced\"\n",
        "viewport_mode = \"fixed_640x480\"\n",
        "default_zoom = 1.5\n",
        "smooth_camera = false\n",
        "roof_peek = \"hidden\"\n",
        "ui_scale = 3.0\n",
    ] {
        assert!(text.contains(line), "{line}: {text}");
    }
    let saved = Config::parse(&text).unwrap();
    assert_eq!(
        saved.enhanced_settings(),
        crate::enhanced::EnhancedSettings {
            viewport: ViewportMode::Fixed,
            default_zoom: 1.5,
            smooth_camera: false,
            roof_peek: RoofPeek::Hidden,
            ui_scale: Some(3.0),
            navigation_only: false,
        }
    );
    // Back to Vanilla, the rows hide again (their keys stay saved).
    launcher.focus = Widget::Preset;
    key(&mut launcher, KeyCode::Space);
    assert!(
        enhanced
            .iter()
            .all(|widget| !launcher.order().contains(widget))
    );
    assert_eq!(launcher.image().pixels(), vanilla.pixels());
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
        data_dirs: PerGame::default(),
    });
    let image = launcher.image();
    assert_eq!((image.width(), image.height()), (640, 480));
    let digest = format!("{:x}", Sha256::digest(image.pixels()));
    assert_eq!(
        digest,
        concat!(
            "e4eaed7d9bf7f9427f237d4722895d98",
            "bf5f118abd0d7f046b279560a38e6798"
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

#[test]
fn the_touch_ui_row_follows_fullscreen_toggles_and_saves() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let mut launcher = Launcher::new(setup(dir, Config::default()));
    launcher.tab = Tab::Display;
    // In the focus order right after Fullscreen.
    let order = launcher.order();
    let at = order
        .iter()
        .position(|widget| *widget == Widget::Fullscreen)
        .unwrap();
    assert_eq!(&order[at + 1..at + 3], &[Widget::TouchUi, Widget::Preset]);
    launcher.focus = Widget::Fullscreen;
    key(&mut launcher, KeyCode::Tab);
    assert_eq!(launcher.focus, Widget::TouchUi);
    // Off by default; Space turns it on, and the row draws it.
    assert_eq!(launcher.config.touch_ui, None);
    let off = launcher.image();
    key(&mut launcher, KeyCode::Space);
    assert_eq!(launcher.config.touch_ui, Some(true));
    assert_ne!(launcher.image().pixels(), off.pixels());
    // With the enhanced rows shown too, no two rows overlap, and the
    // display rows end above the enhanced section's title (y 238).
    launcher.config.preset = Some(Preset::Enhanced);
    let rows: Vec<_> = [
        Widget::Scaling,
        Widget::Fullscreen,
        Widget::TouchUi,
        Widget::Preset,
        Widget::Audio,
        Widget::Debug,
        Widget::Viewport,
        Widget::Zoom,
        Widget::SmoothCamera,
        Widget::RoofPeek,
        Widget::UiScale,
    ]
    .into_iter()
    .map(|widget| launcher.rect(widget).unwrap())
    .collect();
    for pair in rows.windows(2) {
        assert!(pair[0].y + pair[0].h < pair[1].y, "{pair:?}");
    }
    assert!(rows[5].y + rows[5].h < 238);
    // Save writes it, and a launcher from the file shows it.
    assert_eq!(launcher.save(), Ok(dir.join(config::FILE_NAME)));
    let text = fs::read_to_string(dir.join(config::FILE_NAME)).unwrap();
    assert!(text.contains("touch_ui = true\n"), "{text}");
    let saved = Config::parse(&text).unwrap();
    assert_eq!(saved.touch_ui, Some(true));
    let reloaded = Launcher::new(setup(dir, saved));
    assert_eq!(reloaded.config.touch_ui, Some(true));
    // A click on the row turns it off again.
    let rect = launcher.rect(Widget::TouchUi).unwrap();
    launcher.handle(LauncherInput::Press([rect.x + 4, rect.y + 4]));
    assert_eq!(launcher.config.touch_ui, Some(false));
}
