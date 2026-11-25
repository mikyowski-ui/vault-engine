use super::*;
use crate::enhanced::BuiltinPreset;
use sha2::{Digest, Sha256};
use std::fs;

use vault_fixtures::dat::{archive, critter_archive};

/// The SPEC-008 synthetic archive fixture: two text members only.
fn fixture_archive() -> Vec<u8> {
    crate::fixture_assets::sparse(include_str!(
        "../../../../tests/fixtures/archive/synthetic.dat.hex"
    ))
}

/// A game folder with both databases.
fn install(dir: &Path) {
    vault_fixtures::dat::fallout1_install(dir);
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

/// Each game keeps its own load order: the Mods tab follows the Game toggle,
/// a new list never lands in the other game's file, and unsaved edits stop
/// the toggle rather than vanish or cross over.
#[test]
fn each_game_keeps_its_own_load_order() {
    use GameProfile::{Fallout1, Fallout2};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let settings = dir.join(config::FILE_NAME);
    let config = Config {
        game: Some(Fallout2),
        ..Config::default()
    };
    let mut launcher = Launcher::new(Setup {
        settings_path: Some(settings.clone()),
        ..setup(dir, config)
    });
    let layer = |path: &str| LayerEntry {
        path: Some(path.to_owned()),
        ..LayerEntry::default()
    };

    launcher.mods.entries.push(layer("mods/two"));
    launcher.layers_changed();
    launcher.switch_game(Fallout1);
    assert_eq!(launcher.config.active_game(), Fallout2);
    assert_eq!(launcher.mods.entries, [layer("mods/two")]);
    assert!(launcher.status.contains("Save the Mods tab"));
    launcher.save().unwrap();

    launcher.switch_game(Fallout1);
    assert_eq!(launcher.config.active_game(), Fallout1);
    assert!(launcher.mods.entries.is_empty());
    assert_eq!(launcher.mods.file, None);
    launcher.mods.entries.push(layer("mods/one"));
    launcher.layers_changed();
    launcher.save().unwrap();

    let read = |name: &str| {
        vault_formats::parse_load_order(&fs::read_to_string(dir.join(name)).unwrap()).unwrap()
    };
    assert_eq!(read("load_order.toml"), [layer("mods/two")]);
    assert_eq!(read("load_order_fallout1.toml"), [layer("mods/one")]);
    let saved = Config::load(&settings).unwrap();
    assert_eq!(
        saved.scope(Fallout1).mod_list,
        Some(ModList::File(dir.join("load_order_fallout1.toml")))
    );
    assert_eq!(
        saved.scope(Fallout2).mod_list,
        Some(ModList::File(dir.join("load_order.toml")))
    );

    launcher.switch_game(Fallout2);
    assert_eq!(launcher.mods.entries, [layer("mods/two")]);
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
    // The preset is the active profile's, in its table.
    assert_eq!(saved.enhanced.preset, Some(Preset::Enhanced));
    assert_eq!(saved.preset, None);
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
    // An inline path list is shown as it stands, and the first edit makes it
    // a load order: `load_order.toml` is there already, so it gets its own.
    let inline = Config {
        mod_list: Some(ModList::Paths(vec![dir.join("mods/one")])),
        ..reloaded.config.clone()
    };
    let mut launcher = Launcher::new(setup(dir, inline));
    launcher.tab = Tab::Mods;
    assert_eq!(launcher.mods.read_only, None);
    assert!(launcher.mods.check.stack.is_ok());
    launcher.handle(LauncherInput::Dropped(dir.join("mods/two")));
    assert_eq!(paths(&launcher), ["mods/one", "mods/two"]);
    assert!(launcher.mods.check.layers.iter().all(Check::is_ok));
    launcher.save().unwrap();
    assert_eq!(
        launcher.config.mod_list,
        Some(ModList::File(dir.join("load_order_fallout1.toml")))
    );
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
    vault_fixtures::dat2::launcher_install(dir, &vault_formats::fallout2::REQUIRED_MEMBERS);
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
        // A path as the settings file writes it: the platform's own
        // separators, a backslash escaped as TOML's basic strings need.
        let toml = |path: &Path| path.display().to_string().replace('\\', "\\\\");
        for (name, master) in [
            ("fallout1", Path::new("fo1").join("MASTER.DAT")),
            ("fallout2", Path::new("fo2").join("master.dat")),
        ] {
            let table = format!("[profiles.{name}]\nmaster_dat = \"{}\"\n", toml(&master));
            assert!(text.contains(&table), "{text}");
        }
        let top = format!(
            "master_dat = \"{}\"\n",
            toml(paths[0].strip_prefix(dir).unwrap())
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
    for _ in 0..24 {
        key(&mut launcher, KeyCode::Tab);
        seen.push(launcher.focus);
    }
    assert_eq!(
        seen,
        [
            W::Tab(T::Data),
            W::Tab(T::Display),
            W::Tab(T::Audio),
            W::Tab(T::Debug),
            W::Field(F::Master),
            W::Browse(F::Master),
            W::Field(F::Critter),
            W::Browse(F::Critter),
            W::Field(F::DataRoot),
            W::Browse(F::DataRoot),
            W::Game,
            W::AutoDetect,
            W::Layers,
            W::LayerUp,
            W::LayerDown,
            W::LayerToggle,
            W::LayerRemove,
            W::NewLayer,
            W::AddLayer,
            W::BrowseLayer,
            W::Save,
            W::Play,
            W::PlayDebug,
            W::Quit,
            W::ProfileCard(0),
        ]
    );
    launcher.handle(LauncherInput::Key {
        key: KeyCode::Tab,
        shift: true,
    });
    assert_eq!(launcher.focus, W::Quit);
    // Arrows on the pills switch tabs, wrapping, and leave a tab for its rows.
    launcher.focus = W::Tab(T::Data);
    key(&mut launcher, KeyCode::ArrowLeft);
    assert_eq!((launcher.tab, launcher.focus), (T::Debug, W::Tab(T::Debug)));
    key(&mut launcher, KeyCode::ArrowDown);
    assert_eq!(launcher.focus, W::Debug);
    // The Mods tab shares the first pill.
    launcher.tab = T::Mods;
    launcher.focus = W::Tab(T::Data);
    key(&mut launcher, KeyCode::ArrowRight);
    assert_eq!(launcher.tab, T::Display);
    // The browser is modal, and Esc backs out of it to its button.
    launcher.tab = T::Data;
    launcher.focus = W::Browse(F::Critter);
    key(&mut launcher, KeyCode::Space);
    assert_eq!(launcher.focus, W::BrowseList);
    key(&mut launcher, KeyCode::Tab);
    assert_eq!(launcher.focus, W::BrowseUse);
    key(&mut launcher, KeyCode::Tab);
    assert_eq!(launcher.focus, W::BrowseCancel);
    key(&mut launcher, KeyCode::Tab);
    assert_eq!(launcher.focus, W::KnownPath(0));
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
    let inside = |launcher: &Launcher, widget: W| {
        let rect = launcher.rect(widget).unwrap();
        [rect.x + 2, rect.y + 2]
    };
    let play = inside(&launcher, W::Play);
    launcher.handle(LauncherInput::Pointer(play));
    assert_eq!(launcher.hover, Some(W::Play));
    let quit = inside(&launcher, W::Quit);
    let quit = launcher.handle(LauncherInput::Press(quit));
    assert_eq!(quit, Some(LauncherAction::Quit));
    let pill = inside(&launcher, W::Tab(T::Display));
    launcher.handle(LauncherInput::Press(pill));
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
        Widget::CameraFollow,
        Widget::RecenterMargin,
        Widget::RoofPeek,
        Widget::UiScale,
        Widget::GameSpeed,
        Widget::TacticalOverlay,
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
    assert_eq!(launcher.config.enhanced.preset, Some(Preset::Enhanced));
    // Enhanced: the rows follow the general ones in the focus order.
    let order = launcher.order();
    let last = order
        .iter()
        .position(|widget| *widget == Widget::Fullscreen)
        .unwrap();
    assert_eq!(order[last - 1], Widget::Scaling);
    assert_eq!(&order[last + 1..last + 10], &enhanced);
    assert_eq!(order[last + 10], Widget::Save);
    launcher.focus = Widget::Fullscreen;
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
    // The defaults (a configured follow is `off`), then each choice.
    assert_eq!(
        launcher.active_enhanced().settings,
        crate::enhanced::EnhancedSettings {
            camera_follow: crate::enhanced::CameraFollow::CONFIGURED,
            ..Default::default()
        }
    );
    // Each goes in the active profile's table.
    launcher.focus = Widget::Viewport;
    key(&mut launcher, KeyCode::ArrowRight);
    assert_eq!(
        launcher.config.enhanced.viewport_mode,
        Some(ViewportMode::Fixed)
    );
    // The zoom steps through its stops, both ways, wrapping at the ends.
    launcher.focus = Widget::Zoom;
    let mut zooms = Vec::new();
    for _ in 0..8 {
        key(&mut launcher, KeyCode::ArrowRight);
        zooms.push(launcher.config.enhanced.default_zoom.unwrap());
    }
    assert_eq!(zooms, [1.25, 1.5, 2.0, 2.5, 3.0, 0.5, 0.75, 1.0]);
    key(&mut launcher, KeyCode::ArrowLeft);
    assert_eq!(launcher.config.enhanced.default_zoom, Some(0.75));
    // A value from the file between stops moves from the nearest one.
    launcher.config.enhanced.default_zoom = Some(1.1);
    key(&mut launcher, KeyCode::ArrowRight);
    assert_eq!(launcher.config.enhanced.default_zoom, Some(1.25));
    launcher.focus = Widget::SmoothCamera;
    key(&mut launcher, KeyCode::Enter);
    assert_eq!(launcher.config.enhanced.smooth_camera, Some(false));
    launcher.focus = Widget::RoofPeek;
    key(&mut launcher, KeyCode::Space);
    assert_eq!(
        launcher.config.enhanced.roof_peek,
        Some(RoofPeek::CursorXray)
    );
    key(&mut launcher, KeyCode::ArrowLeft);
    key(&mut launcher, KeyCode::ArrowLeft);
    assert_eq!(launcher.config.enhanced.roof_peek, Some(RoofPeek::Hidden));
    launcher.focus = Widget::UiScale;
    let mut scales = Vec::new();
    for _ in 0..6 {
        key(&mut launcher, KeyCode::ArrowRight);
        scales.push(launcher.config.enhanced.ui_scale.unwrap());
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
    assert_eq!(launcher.config.enhanced.ui_scale, Some(UiScale::Scale(3.0)));
    // The game speed cycles 1.0, 1.5, 2.0 both ways, as Alt+F does.
    use crate::enhanced::GameSpeed;
    launcher.focus = Widget::GameSpeed;
    assert_eq!(launcher.active_enhanced().game_speed, GameSpeed::Normal);
    let mut speeds = Vec::new();
    for _ in 0..3 {
        key(&mut launcher, KeyCode::ArrowRight);
        speeds.push(launcher.config.enhanced.game_speed.unwrap());
    }
    assert_eq!(
        speeds,
        [GameSpeed::Fast, GameSpeed::Faster, GameSpeed::Normal]
    );
    key(&mut launcher, KeyCode::ArrowLeft);
    assert_eq!(launcher.config.enhanced.game_speed, Some(GameSpeed::Faster));
    key(&mut launcher, KeyCode::Enter);
    assert_eq!(launcher.config.enhanced.game_speed, Some(GameSpeed::Normal));
    let rect = launcher.rect(Widget::GameSpeed).unwrap();
    launcher.handle(LauncherInput::Press([rect.x + 4, rect.y + 4]));
    assert_eq!(launcher.config.enhanced.game_speed, Some(GameSpeed::Fast));
    // A click on a row steps it too.
    let rect = launcher.rect(Widget::Zoom).unwrap();
    launcher.handle(LauncherInput::Press([rect.x + 4, rect.y + 4]));
    assert_eq!(launcher.config.enhanced.default_zoom, Some(1.5));
    // A click on a slider's track takes the stop under it.
    let track = view::slider_track(rect);
    launcher.handle(LauncherInput::Press([track.x, rect.y + 4]));
    assert_eq!(launcher.config.enhanced.default_zoom, Some(0.5));
    launcher.handle(LauncherInput::Press([rect.x + 4, rect.y + 4]));
    assert_eq!(launcher.config.enhanced.default_zoom, Some(0.75));
    launcher.handle(LauncherInput::Press([rect.x + 4, rect.y + 4]));
    assert_eq!(launcher.config.enhanced.default_zoom, Some(1.0));
    launcher.handle(LauncherInput::Press([rect.x + 4, rect.y + 4]));
    launcher.handle(LauncherInput::Press([rect.x + 4, rect.y + 4]));
    assert_eq!(launcher.config.enhanced.default_zoom, Some(1.5));
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
        "game_speed = 1.5\n",
    ] {
        assert!(text.contains(line), "{line}: {text}");
    }
    let saved = Config::parse(&text).unwrap();
    // A launch reads them, and a launcher from the saved file shows them.
    let launch = saved.scoped_to(saved.active_game());
    assert_eq!(launch.game_speed(), GameSpeed::Fast);
    let reloaded = Launcher::new(setup(dir, saved.clone()));
    assert_eq!(reloaded.active_enhanced().game_speed, GameSpeed::Fast);
    assert_eq!(
        launch.enhanced_settings(),
        crate::enhanced::EnhancedSettings {
            viewport: ViewportMode::Fixed,
            default_zoom: 1.5,
            smooth_camera: false,
            roof_peek: RoofPeek::Hidden,
            ui_scale: Some(3.0),
            navigation_only: false,
            tactical_overlay: None,
            camera_follow: crate::enhanced::CameraFollow::CONFIGURED,
            camera_recenter_margin: crate::enhanced::CameraFollow::MARGIN,
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
    // Vanilla on the fixed viewport is the purist preset, which lights its
    // button; on the viewport it started with, the tab draws as it did.
    assert_eq!(
        launcher.active_preset(),
        Some(PresetMatch::Builtin(BuiltinPreset::VanillaPurist))
    );
    launcher.config.enhanced.viewport_mode = None;
    assert_eq!(launcher.image().pixels(), vanilla.pixels());
}

/// The camera follow row steps Off, Step, Smooth both ways, and the margin
/// row through its stops, by key and by a click on its track; both save
/// to the active profile and read back. Under Vanilla they hide.
#[test]
fn the_camera_follow_rows_step_and_save_with_the_enhanced_preset() {
    use crate::enhanced::CameraFollow;
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let mut launcher = Launcher::new(setup(dir, Config::default()));
    launcher.tab = Tab::Display;
    let rows = [Widget::CameraFollow, Widget::RecenterMargin];
    assert!(rows.iter().all(|widget| !launcher.order().contains(widget)));
    launcher.focus = Widget::Preset;
    key(&mut launcher, KeyCode::Space);
    assert!(rows.iter().all(|widget| launcher.order().contains(widget)));
    let settings = |launcher: &Launcher| launcher.active_enhanced().settings;
    assert_eq!(settings(&launcher).camera_follow, CameraFollow::Off);
    assert_eq!(settings(&launcher).camera_recenter_margin, 96);

    launcher.focus = Widget::CameraFollow;
    let mut modes = Vec::new();
    for _ in 0..3 {
        key(&mut launcher, KeyCode::ArrowRight);
        modes.push(launcher.config.enhanced.camera_follow.unwrap());
    }
    assert_eq!(
        modes,
        [CameraFollow::Step, CameraFollow::Smooth, CameraFollow::Off]
    );
    key(&mut launcher, KeyCode::ArrowLeft);
    assert_eq!(
        launcher.config.enhanced.camera_follow,
        Some(CameraFollow::Smooth)
    );
    let shown = |launcher: &Launcher, widget| view::setting(launcher, widget).unwrap().1;
    assert_eq!(
        shown(&launcher, Widget::CameraFollow),
        view::Control::Choice("Smooth")
    );
    // A click on the row steps it.
    let rect = launcher.rect(Widget::CameraFollow).unwrap();
    launcher.handle(LauncherInput::Press([rect.x + 4, rect.y + 4]));
    assert_eq!(settings(&launcher).camera_follow, CameraFollow::Off);

    launcher.focus = Widget::RecenterMargin;
    let mut margins = Vec::new();
    for _ in 0..5 {
        key(&mut launcher, KeyCode::ArrowRight);
        margins.push(launcher.config.enhanced.camera_recenter_margin.unwrap());
    }
    assert_eq!(margins, [144, 192, 0, 48, 96]);
    key(&mut launcher, KeyCode::ArrowLeft);
    assert_eq!(launcher.config.enhanced.camera_recenter_margin, Some(48));
    assert!(matches!(
        shown(&launcher, Widget::RecenterMargin),
        view::Control::Slider { stop: 1, stops: 5, value } if value == "48px"
    ));
    // A margin from the file between stops moves from the nearest one.
    launcher.config.enhanced.camera_recenter_margin = Some(100);
    key(&mut launcher, KeyCode::ArrowRight);
    assert_eq!(launcher.config.enhanced.camera_recenter_margin, Some(144));
    // A click on the track takes the stop under it: the first is off.
    let rect = launcher.rect(Widget::RecenterMargin).unwrap();
    let track = view::slider_track(rect);
    launcher.handle(LauncherInput::Press([track.x, rect.y + 4]));
    assert_eq!(launcher.config.enhanced.camera_recenter_margin, Some(0));
    assert!(matches!(
        shown(&launcher, Widget::RecenterMargin),
        view::Control::Slider { value, .. } if value == "Off"
    ));
    launcher.handle(LauncherInput::Press([track.right() - 1, rect.y + 4]));
    assert_eq!(launcher.config.enhanced.camera_recenter_margin, Some(192));

    // Save writes them, and they read back.
    launcher.focus = Widget::CameraFollow;
    key(&mut launcher, KeyCode::Enter);
    assert_eq!(settings(&launcher).camera_follow, CameraFollow::Step);
    assert_eq!(launcher.save(), Ok(dir.join(config::FILE_NAME)));
    let text = fs::read_to_string(dir.join(config::FILE_NAME)).unwrap();
    for line in [
        "camera_follow = \"step\"\n",
        "camera_recenter_margin = 192\n",
    ] {
        assert!(text.contains(line), "{line}: {text}");
    }
    let saved = Config::parse(&text).unwrap();
    let reloaded = Launcher::new(setup(dir, saved));
    assert_eq!(settings(&reloaded).camera_follow, CameraFollow::Step);
    assert_eq!(settings(&reloaded).camera_recenter_margin, 192);
    // Back to Vanilla, the rows hide again.
    launcher.focus = Widget::Preset;
    key(&mut launcher, KeyCode::Space);
    assert!(rows.iter().all(|widget| !launcher.order().contains(widget)));
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
            "9af742bf683c34f096233c0202e375e6",
            "06d03285cb823c7473fc9b4b69ce8a73"
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
    launcher.tab = Tab::Audio;
    // In the focus order right after Audio.
    let order = launcher.order();
    let at = order
        .iter()
        .position(|widget| *widget == Widget::Audio)
        .unwrap();
    assert_eq!(
        &order[at + 1..at + 3],
        &[Widget::TouchUi, Widget::HotkeyHints]
    );
    launcher.focus = Widget::Audio;
    key(&mut launcher, KeyCode::Tab);
    assert_eq!(launcher.focus, Widget::TouchUi);
    // Off by default; Space turns it on, and the row draws it.
    assert_eq!(launcher.config.touch_ui, None);
    let off = launcher.image();
    key(&mut launcher, KeyCode::Space);
    assert_eq!(launcher.config.touch_ui, Some(true));
    assert_ne!(launcher.image().pixels(), off.pixels());
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

/// SPEC-020 §2.6: the `Hotkey hints` row, on the Audio & Input tab under both
/// presets, steps through its three values and saves like any row.
#[test]
fn the_hotkey_hints_row_steps_and_saves_under_both_presets() {
    let dir = tempfile::tempdir().unwrap();
    for preset in [Preset::Vanilla, Preset::Enhanced] {
        let config = Config {
            preset: Some(preset),
            ..Config::default()
        };
        let mut launcher = Launcher::new(setup(dir.path(), config));
        launcher.tab = Tab::Audio;
        assert!(launcher.order().contains(&Widget::HotkeyHints));
        launcher.focus = Widget::HotkeyHints;
        let shown = |launcher: &Launcher| launcher.config.hotkey_hints;
        for expected in [HotkeyHints::Always, HotkeyHints::Off, HotkeyHints::Once] {
            let before = launcher.image();
            key(&mut launcher, KeyCode::Enter);
            assert_eq!(shown(&launcher), Some(expected));
            assert_ne!(
                launcher.image().pixels(),
                before.pixels(),
                "the row shows its value"
            );
        }
        key(&mut launcher, KeyCode::ArrowLeft);
        assert_eq!(shown(&launcher), Some(HotkeyHints::Off), "wraps backward");
        key(&mut launcher, KeyCode::ArrowRight);
        assert_eq!(shown(&launcher), Some(HotkeyHints::Once));
        key(&mut launcher, KeyCode::ArrowRight);
        let path = launcher.save().unwrap();
        let read = Config::load(&path).unwrap();
        assert_eq!(read.hotkey_hints, Some(HotkeyHints::Always));
    }
}

/// The row stays clear of the help line below the column.
#[test]
fn the_hotkey_hints_row_is_on_the_tab_and_clear_of_the_help_line() {
    let dir = tempfile::tempdir().unwrap();
    let mut launcher = Launcher::new(setup(dir.path(), Config::default()));
    launcher.tab = Tab::Audio;
    let rect = launcher.rect(Widget::HotkeyHints).unwrap();
    assert!(rect.y + rect.h < view::HELP_TOP - 8);
    let press = LauncherInput::Press([rect.x + 2, rect.y + 2]);
    assert_eq!(launcher.handle(press), None);
    assert_eq!(launcher.config.hotkey_hints, Some(HotkeyHints::Always));
}

/// `switch-cases-rev4.tsv` S21, S22 and S24: from Setup, Quit reads `Back`
/// and asks for the selector, on the Game data tab with the edition it was
/// opened for. Save writes the settings and asks nothing of the shell.
#[test]
fn from_the_selector_quit_is_back_and_save_stays() {
    let dir = tempfile::tempdir().unwrap();
    let plain = Launcher::new(setup(dir.path(), Config::default()));
    let same_game =
        Launcher::new(setup(dir.path(), Config::default())).opened_by_selector(plain.game());
    assert_ne!(
        same_game.image().pixels(),
        plain.image().pixels(),
        "only its button differs: it reads Back"
    );
    let mut launcher = Launcher::new(setup(dir.path(), Config::default()))
        .opened_by_selector(GameProfile::Fallout2);
    assert_eq!(launcher.tab, Tab::Data);
    assert_eq!(launcher.game(), GameProfile::Fallout2);
    launcher.focus = Widget::Save;
    assert_eq!(key(&mut launcher, KeyCode::Enter), None);
    let path = dir.path().join(config::FILE_NAME);
    assert_eq!(
        Config::load(&path).unwrap().game,
        Some(GameProfile::Fallout2)
    );
    assert_eq!(
        Config::load(&path).unwrap().last_played,
        None,
        "Save is no choice"
    );
    launcher.focus = Widget::Quit;
    assert_eq!(
        key(&mut launcher, KeyCode::Enter),
        Some(LauncherAction::Back)
    );

    let mut plain = plain;
    plain.focus = Widget::Quit;
    assert_eq!(key(&mut plain, KeyCode::Enter), Some(LauncherAction::Quit));
}

/// Decision D-36: Play from Setup is a choice, and sets `last_played` with
/// the rest of the settings. A launcher that was not opened from it does not.
#[test]
fn play_from_setup_sets_last_played() {
    for selector in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path());
        let config = Config {
            master_dat: Some(dir.path().join("MASTER.DAT")),
            critter_dat: Some(dir.path().join("CRITTER.DAT")),
            ..Config::default()
        };
        let mut launcher = Launcher::new(setup(dir.path(), config));
        if selector {
            launcher = launcher.opened_by_selector(GameProfile::Fallout1);
        }
        launcher.focus = Widget::Play;
        assert_eq!(
            key(&mut launcher, KeyCode::Enter),
            Some(LauncherAction::Play)
        );
        let path = dir.path().join(config::FILE_NAME);
        let read = Config::load(&path).unwrap();
        assert_eq!(read.last_played.is_some(), selector);
    }
}

/// `dir`'s data for each edition, in folders of their own, and a third folder
/// of Fallout 2 data, for a custom profile that runs on its own install.
struct Installs {
    fallout1: PathBuf,
    fallout2: PathBuf,
    ettu: PathBuf,
}

impl Installs {
    fn new(dir: &Path) -> Self {
        let folder = |name: &str| {
            let folder = dir.join(name);
            fs::create_dir_all(&folder).unwrap();
            folder
        };
        let installs = Self {
            fallout1: folder("fo1"),
            fallout2: folder("fo2"),
            ettu: folder("ettu"),
        };
        install(&installs.fallout1);
        fallout2_install(&installs.fallout2);
        fallout2_install(&installs.ettu);
        installs
    }
    /// Fallout 1 active, each edition on its folder, and the custom profile
    /// `ettu` on its own, with `sfall` and its own Enhanced settings.
    fn config(&self) -> Config {
        use GameProfile::{Fallout1, Fallout2};
        let found =
            |dir: &PathBuf, game| config::discover_game(std::slice::from_ref(dir), game).unwrap();
        let profiles = PerGame {
            fallout1: found(&self.fallout1, Fallout1),
            fallout2: found(&self.fallout2, Fallout2),
        };
        let mut config = Config::with_profiles(Fallout1, profiles);
        config.add_custom_profile("ettu", Fallout2).unwrap();
        let scope = &mut config.custom_profiles[0].scope;
        scope.install_dir = Some(self.ettu.clone());
        scope.sfall = Some(true);
        scope.enhanced = BuiltinPreset::ModernTactical.settings();
        config
    }
}

fn layer(path: &str) -> LayerEntry {
    LayerEntry {
        path: Some(path.to_owned()),
        ..LayerEntry::default()
    }
}

/// A folder layer that mounts.
fn mod_folder(dir: &Path) {
    fs::create_dir_all(dir.join("data")).unwrap();
    fs::write(dir.join("data/x.txt"), "x").unwrap();
}

const FALLOUT1: usize = 0;
const FALLOUT2: usize = 1;
const ETTU: usize = 2;

#[test]
fn each_card_is_a_profile_with_its_own_scope() {
    use GameProfile::{Fallout1, Fallout2};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let installs = Installs::new(dir);
    let mut config = installs.config();
    config.last_played = Some("fallout2".into());
    config.custom_profiles[0].scope.mod_list = Some(ModList::Paths(vec![dir.join("one")]));
    mod_folder(&dir.join("one"));
    let settings = dir.join(config::FILE_NAME);
    let mut launcher = Launcher::new(Setup {
        settings_path: Some(settings.clone()),
        ..setup(dir, config)
    });

    let cards = launcher.profile_cards();
    let names: Vec<&str> = cards.iter().map(|card| card.name.as_str()).collect();
    assert_eq!(names, ["fallout1", "fallout2", "ettu"]);
    let titles: Vec<&str> = cards.iter().map(|card| card.title.as_str()).collect();
    assert_eq!(titles, ["Fallout 1", "Fallout 2", "Fallout: Et Tu"]);
    let editions: Vec<_> = cards.iter().map(|card| card.edition).collect();
    assert_eq!(editions, [Fallout1, Fallout2, Fallout2]);
    assert!(cards.iter().all(|card| card.ready), "{cards:?}");
    let active: Vec<bool> = cards.iter().map(|card| card.active).collect();
    assert_eq!(active, [true, false, false]);
    assert!(cards[ETTU].sfall && !cards[FALLOUT2].sfall);
    assert_eq!(cards[ETTU].mods, 1);
    assert!(cards[FALLOUT2].last_played && !cards[ETTU].last_played);

    // The card shows the profile: its folder, its mods, its settings.
    launcher.activate(Widget::ProfileCard(ETTU));
    assert_eq!(launcher.config.active_profile(), "ettu");
    assert_eq!(launcher.game(), Fallout2);
    assert_eq!(
        launcher.path(Field::Master),
        Some(installs.ettu.join("master.dat"))
    );
    assert_eq!(launcher.config.sfall, Some(true));
    assert_eq!(launcher.mods.entries.len(), 1);
    assert_eq!(
        launcher.active_preset(),
        Some(PresetMatch::Builtin(BuiltinPreset::ModernTactical))
    );
    let cards = launcher.profile_cards();
    assert!(cards[ETTU].active && cards[ETTU].ready);
    // `last_played` names the edition, which runs as this profile now.
    assert!(cards[ETTU].last_played && !cards[FALLOUT2].last_played);

    // An edit stays with the profile it was made in.
    launcher.activate(Widget::Zoom);
    assert_eq!(launcher.config.enhanced.default_zoom, Some(1.25));
    launcher.activate(Widget::ProfileCard(FALLOUT1));
    assert_eq!(launcher.active_enhanced().settings.default_zoom, 1.0);
    assert_eq!(launcher.config.sfall, None);
    assert!(launcher.mods.entries.is_empty());

    // Its folder never lands on its edition's paths, whatever Save does.
    launcher.activate(Widget::ProfileCard(ETTU));
    launcher.activate(Widget::Save);
    assert_eq!(launcher.status_state, State::Ok, "{}", launcher.status);
    let saved = Config::load(&settings).unwrap();
    assert_eq!(saved.active_profile(), "ettu");
    assert_eq!(saved.install_dir, Some(installs.ettu.clone()));
    assert_eq!(saved.enhanced.default_zoom, Some(1.25));
    assert_eq!(
        saved.profile(Fallout2).master_dat,
        Some(installs.fallout2.join("master.dat"))
    );
    assert_eq!(
        saved.launch_paths(Fallout2).master_dat,
        Some(installs.ettu.join("master.dat"))
    );
    let mut reloaded = Launcher::new(Setup {
        settings_path: Some(settings.clone()),
        ..setup(dir, saved)
    });
    assert_eq!(
        reloaded.path(Field::Master),
        Some(installs.ettu.join("master.dat"))
    );
    reloaded.activate(Widget::ProfileCard(FALLOUT2));
    assert_eq!(
        reloaded.path(Field::Master),
        Some(installs.fallout2.join("master.dat"))
    );
    assert_eq!(reloaded.config.install_dir, None);
    assert_eq!(reloaded.config.custom_profiles[0].scope.sfall, Some(true));
}

/// A custom profile with no folder reads its edition's, and a folder typed in
/// becomes its own. Paths that are not one folder cannot be kept, and the
/// launcher says so, but a launch still runs what the fields say.
#[test]
fn a_custom_profiles_paths_never_reach_its_edition() {
    use GameProfile::Fallout1;
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let installs = Installs::new(dir);
    let other = dir.join("other");
    fs::create_dir_all(&other).unwrap();
    install(&other);
    let edition = installs.fallout1.join("MASTER.DAT");
    let mut launcher = Launcher::new(setup(dir, installs.config()));

    launcher.activate(Widget::NewProfile);
    assert_eq!(launcher.config.active_profile(), "profile-1");
    assert_eq!(launcher.game(), Fallout1);
    assert_eq!(launcher.path(Field::Master), Some(edition.clone()));
    assert_eq!(launcher.ready(), Ok(()));
    launcher.activate(Widget::Save);
    assert_eq!(launcher.config.install_dir, None);

    launcher.set_field(Field::Master, &other.join("MASTER.DAT"));
    launcher.set_field(Field::Critter, &other.join("CRITTER.DAT"));
    assert_eq!(launcher.ready(), Ok(()));
    launcher.activate(Widget::Save);
    assert_eq!(launcher.status_state, State::Ok, "{}", launcher.status);
    assert_eq!(launcher.config.install_dir, Some(other.clone()));
    assert_eq!(launcher.config.master_dat, Some(edition.clone()));
    assert_eq!(
        launcher.launch_config().launch_paths(Fallout1).master_dat,
        Some(other.join("MASTER.DAT"))
    );

    // MASTER.DAT here and CRITTER.DAT there.
    launcher.set_field(Field::Critter, &installs.fallout1.join("CRITTER.DAT"));
    assert_eq!(launcher.ready(), Ok(()));
    launcher.activate(Widget::Save);
    assert_eq!(launcher.status_state, State::Bad);
    assert!(
        launcher.status.contains("one game folder"),
        "{}",
        launcher.status
    );
    assert_eq!(launcher.config.master_dat, Some(edition.clone()));
    let launch = launcher.launch_config();
    let run = launch.scoped_to(Fallout1);
    assert_eq!(run.master_dat, Some(other.join("MASTER.DAT")));
    assert_eq!(run.critter_dat, Some(installs.fallout1.join("CRITTER.DAT")));
    assert_eq!(launcher.config.master_dat, Some(edition.clone()));

    // An edition keeps what it is given, and a folder of its own that has
    // gone out of step with its fields goes.
    launcher.activate(Widget::ProfileCard(FALLOUT1));
    assert_eq!(launcher.path(Field::Master), Some(edition));
    launcher.config.install_dir = Some(installs.fallout1.clone());
    launcher.set_field(Field::Master, &other.join("MASTER.DAT"));
    launcher.set_field(Field::Critter, &other.join("CRITTER.DAT"));
    assert!(launcher.apply_fields());
    assert_eq!(launcher.config.install_dir, None);
    assert_eq!(
        launcher.config.launch_paths(Fallout1).master_dat,
        Some(other.join("MASTER.DAT"))
    );
}

/// A frame asks for the cards: another profile's check is kept, not repeated.
#[test]
fn a_cards_check_is_kept_until_something_changes() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let installs = Installs::new(dir);
    let mut launcher = Launcher::new(setup(dir, installs.config()));
    assert!(launcher.profile_cards()[FALLOUT2].ready);
    fs::remove_file(installs.fallout2.join("master.dat")).unwrap();
    assert!(launcher.profile_cards()[FALLOUT2].ready, "not read again");
    launcher.save().unwrap();
    assert!(!launcher.profile_cards()[FALLOUT2].ready);
    // The active card follows the fields as they are edited.
    assert!(launcher.profile_cards()[FALLOUT1].ready);
    launcher.set_field(Field::Master, &dir.join("gone.dat"));
    assert!(!launcher.profile_cards()[FALLOUT1].ready);
}

#[test]
fn a_card_with_unsaved_mods_stays_and_starts_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let installs = Installs::new(dir);
    let mut launcher = Launcher::new(Setup {
        settings_path: Some(dir.join(config::FILE_NAME)),
        ..setup(dir, installs.config())
    });
    mod_folder(&dir.join("one"));
    assert!(launcher.add_layer("one".into()));
    for widget in [
        Widget::ProfileCard(ETTU),
        Widget::ProfilePlay(ETTU),
        Widget::ProfileResume(ETTU),
        Widget::ProfileAutoplay(ETTU),
        Widget::NewProfile,
    ] {
        assert_eq!(launcher.activate(widget), None, "{widget:?}");
        assert_eq!(launcher.config.active_profile(), "fallout1");
        assert_eq!(launcher.mods.entries, [layer("one")]);
        assert!(launcher.status.contains("Save the Mods tab"), "{widget:?}");
    }
    assert_eq!(launcher.config.custom_profiles.len(), 1);
    // The active profile's own card needs nothing saved.
    assert_eq!(
        launcher.activate(Widget::ProfilePlay(FALLOUT1)),
        Some(LauncherAction::Play)
    );
    // Play saved them, so the card can go.
    assert_eq!(launcher.activate(Widget::ProfileCard(ETTU)), None);
    assert_eq!(launcher.config.active_profile(), "ettu");
    assert!(launcher.mods.entries.is_empty());
}

#[test]
fn a_cards_buttons_play_that_profile() {
    use GameProfile::Fallout2;
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let installs = Installs::new(dir);
    let mut launcher = Launcher::new(setup(dir, installs.config()));
    for (widget, action) in [
        (Widget::ProfilePlay(ETTU), LauncherAction::Play),
        (Widget::ProfileResume(FALLOUT1), LauncherAction::ResumeLast),
        (Widget::ProfileAutoplay(ETTU), LauncherAction::AutoplayTest),
    ] {
        assert_eq!(launcher.activate(widget), Some(action));
    }
    let launch = launcher.launch_config();
    assert_eq!(launch.active_profile(), "ettu");
    assert_eq!(launch.scope_name(Fallout2), "ettu");
    assert_eq!(launch.game, Some(Fallout2));
    // What the shell's start reads from it.
    let run = launch.scoped_to(Fallout2);
    assert_eq!(run.master_dat, Some(installs.ettu.join("master.dat")));
    assert_eq!(run.sfall, Some(true));
    assert_eq!(run.preset, Some(Preset::Enhanced));
    assert_eq!(run.enhanced.tactical_overlay, Some(true));
    // A card with no data says why, and nothing starts.
    fs::remove_file(installs.fallout1.join("MASTER.DAT")).unwrap();
    assert_eq!(launcher.activate(Widget::ProfilePlay(FALLOUT1)), None);
    assert_eq!(launcher.config.active_profile(), "fallout1");
    assert!(
        launcher.status.starts_with("Not ready."),
        "{}",
        launcher.status
    );
}

#[test]
fn new_profiles_take_the_next_free_name_and_read_back() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let mut launcher = Launcher::new(setup(dir, Config::default()));
    launcher.activate(Widget::NewProfile);
    launcher.activate(Widget::ProfileCard(FALLOUT1));
    launcher.activate(Widget::NewProfile);
    let names: Vec<String> = launcher
        .profile_cards()
        .into_iter()
        .map(|card| card.name)
        .collect();
    assert_eq!(
        names,
        ["fallout1", "fallout2", "ettu", "profile-1", "profile-2"]
    );
    assert_eq!(launcher.config.active_profile(), "profile-2");
    let path = launcher.save().unwrap();
    let saved = Config::load(&path).unwrap();
    assert_eq!(saved.active_profile(), "profile-2");
    assert_eq!(
        saved.custom_profiles.len(),
        3,
        "Et Tu's and the two new ones"
    );
}

#[test]
fn the_builtin_presets_set_what_the_brief_says() {
    use crate::enhanced::{RoofPeek, ViewportMode};
    let settings = BuiltinPreset::VanillaPurist.settings();
    assert_eq!(settings.preset, Some(Preset::Vanilla));
    assert_eq!(settings.viewport_mode, Some(ViewportMode::Fixed));
    assert_eq!(settings.tactical_overlay, Some(false));
    let settings = BuiltinPreset::WidescreenClassic.settings();
    assert_eq!(settings.preset, Some(Preset::Enhanced));
    assert_eq!(settings.viewport_mode, Some(ViewportMode::Unlocked));
    assert_eq!(settings.default_zoom, Some(1.0));
    assert_eq!(settings.roof_peek, Some(RoofPeek::Vanilla));
    assert_eq!(settings.tactical_overlay, Some(false));
    let settings = BuiltinPreset::ModernTactical.settings();
    assert_eq!(settings.preset, Some(Preset::Enhanced));
    assert_eq!(settings.viewport_mode, Some(ViewportMode::Unlocked));
    assert_eq!(settings.roof_peek, Some(RoofPeek::CursorXray));
    assert_eq!(settings.tactical_overlay, Some(true));
}

#[test]
fn a_preset_applies_to_the_active_profile_and_is_recognized() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let installs = Installs::new(dir);
    let mut launcher = Launcher::new(setup(dir, installs.config()));
    // A plain profile's settings are no preset's: its viewport is unlocked.
    assert_eq!(launcher.active_preset(), None);
    for preset in BuiltinPreset::ALL {
        launcher.activate(Widget::BuiltinPreset(preset));
        assert_eq!(launcher.config.enhanced, preset.settings());
        assert_eq!(launcher.active_preset(), Some(PresetMatch::Builtin(preset)));
    }
    // A value from the top level matches too: a table need not repeat it.
    launcher.config.enhanced.default_zoom = None;
    launcher.config.default_zoom = Some(1.0);
    launcher.activate(Widget::BuiltinPreset(BuiltinPreset::WidescreenClassic));
    launcher.config.enhanced.default_zoom = None;
    assert_eq!(
        launcher.active_preset(),
        Some(PresetMatch::Builtin(BuiltinPreset::WidescreenClassic))
    );
    // Off the preset by one row.
    launcher.activate(Widget::TacticalOverlay);
    assert_eq!(launcher.config.enhanced.tactical_overlay, Some(true));
    assert_eq!(launcher.active_preset(), None);
    // The other profile did not move.
    launcher.activate(Widget::ProfileCard(FALLOUT2));
    assert!(!launcher.active_enhanced().tactical_overlay);
    assert_eq!(launcher.active_enhanced().preset, Preset::Vanilla);
    assert_eq!(launcher.config.enhanced, Default::default());
}

#[test]
fn a_custom_preset_keeps_every_setting_and_reads_back() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let mut launcher = Launcher::new(setup(dir, Config::default()));
    launcher.activate(Widget::BuiltinPreset(BuiltinPreset::ModernTactical));
    launcher.activate(Widget::Zoom);
    let wanted = launcher.active_enhanced();
    launcher.activate(Widget::SaveCustomPreset);
    assert_eq!(launcher.active_preset(), Some(PresetMatch::Custom(0)));
    let preset = &launcher.config.custom_presets[0];
    assert_eq!(preset.name, "custom-1");
    assert_eq!(preset.settings, wanted.profile());
    // A second press keeps no second copy; a change makes a second preset.
    launcher.activate(Widget::SaveCustomPreset);
    assert_eq!(launcher.config.custom_presets.len(), 1);
    launcher.activate(Widget::Zoom);
    launcher.activate(Widget::SaveCustomPreset);
    assert_eq!(launcher.config.custom_presets[1].name, "custom-2");
    // It holds its values when the top level moves under it.
    launcher.activate(Widget::CustomPreset(0));
    assert_eq!(launcher.active_enhanced(), wanted);
    assert_eq!(launcher.active_preset(), Some(PresetMatch::Custom(0)));
    launcher.activate(Widget::CustomPreset(9));
    assert_eq!(launcher.active_enhanced(), wanted);
    let path = launcher.save().unwrap();
    let saved = Config::load(&path).unwrap();
    assert_eq!(saved.custom_presets, launcher.config.custom_presets);
    // A preset that names nothing is no match for anything.
    launcher.config.custom_presets = vec![config::CustomPreset {
        name: "empty".into(),
        settings: EnhancedProfile::default(),
    }];
    assert_eq!(
        launcher.active_preset(),
        Some(PresetMatch::Builtin(BuiltinPreset::ModernTactical))
    );
}

/// The preview follows the saved settings, and the pointer: the widget under it
/// stands in with its next value, until a press makes that the saved one.
#[test]
fn the_preview_answers_on_hover_and_keeps_what_a_press_chose() {
    use crate::enhanced::{RoofPeek, ViewportMode};
    let temp = tempfile::tempdir().unwrap();
    let mut launcher = Launcher::new(setup(temp.path(), Config::default()));
    assert_eq!(
        launcher.previewed,
        PreviewSettings {
            enhanced: false,
            viewport: ViewportMode::Unlocked,
            zoom: 1.0,
            roof_peek: RoofPeek::Vanilla,
            tactical_overlay: false,
        }
    );
    launcher.config.preset = Some(Preset::Enhanced);
    launcher.tab = Tab::Display;
    let pointer = |launcher: &mut Launcher, widget: Option<Widget>| {
        let point = widget.map_or([0, 0], |it| {
            let rect = launcher.rect(it).unwrap();
            [rect.x + 2, rect.y + 2]
        });
        launcher.handle(LauncherInput::Pointer(point));
    };
    let center = |launcher: &mut Launcher, widget: Widget| {
        let rect = launcher.rect(widget).unwrap();
        [rect.x + 2, rect.y + 2]
    };
    pointer(&mut launcher, None);
    assert!(launcher.previewed.enhanced);

    let tactical = Widget::BuiltinPreset(BuiltinPreset::ModernTactical);
    pointer(&mut launcher, Some(tactical));
    assert_eq!(launcher.previewed.roof_peek, RoofPeek::CursorXray);
    assert!(launcher.previewed.tactical_overlay);
    assert_eq!(
        launcher.config.enhanced,
        Default::default(),
        "nothing saved"
    );
    pointer(&mut launcher, None);
    assert_eq!(launcher.previewed.roof_peek, RoofPeek::Vanilla);
    assert!(!launcher.previewed.tactical_overlay);

    pointer(&mut launcher, Some(Widget::Zoom));
    assert_eq!(launcher.previewed.zoom, 1.25);
    pointer(&mut launcher, None);
    pointer(&mut launcher, Some(Widget::RoofPeek));
    assert_eq!(launcher.previewed.roof_peek, RoofPeek::CursorXray);
    pointer(&mut launcher, Some(Widget::Viewport));
    assert_eq!(launcher.previewed.roof_peek, RoofPeek::Vanilla);
    assert_eq!(launcher.previewed.viewport, ViewportMode::Fixed);
    pointer(&mut launcher, Some(Widget::TacticalOverlay));
    assert!(launcher.previewed.tactical_overlay);

    // A press: the value hovered is the value kept, and the pointer that
    // stays over it does not show the one after.
    let point = center(&mut launcher, Widget::TacticalOverlay);
    launcher.handle(LauncherInput::Press(point));
    assert_eq!(launcher.config.enhanced.tactical_overlay, Some(true));
    assert!(launcher.previewed.tactical_overlay);
    launcher.handle(LauncherInput::Pointer(point));
    assert!(launcher.previewed.tactical_overlay);
    // It answers again when the pointer comes back.
    pointer(&mut launcher, None);
    pointer(&mut launcher, Some(Widget::TacticalOverlay));
    assert!(!launcher.previewed.tactical_overlay);
}

#[test]
fn valid_folders_are_remembered_on_save_and_on_browse() {
    use GameProfile::Fallout1;
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let installs = Installs::new(dir);
    mod_folder(&dir.join("mods/one"));
    fs::write(dir.join("mods/two.dat"), critter_archive()).unwrap();
    let known =
        |launcher: &Launcher, kind: KnownKind| launcher.config.known_paths.list(kind).to_vec();
    let config = Config::with_profiles(Fallout1, PerGame::default());
    let mut launcher = Launcher::new(setup(dir, config));

    // Browse: only what validates.
    launcher.pick_folder(Target::Field(Field::Master), installs.fallout2.clone());
    assert!(
        known(&launcher, KnownKind::Fallout1).is_empty(),
        "the other game's"
    );
    launcher.pick_folder(Target::Field(Field::DataRoot), dir.join("gone"));
    assert!(known(&launcher, KnownKind::Fallout1).is_empty());
    launcher.set_field(Field::DataRoot, Path::new(""));
    launcher.fields = Default::default();
    launcher.pick_folder(Target::Field(Field::Master), installs.fallout1.clone());
    assert_eq!(
        known(&launcher, KnownKind::Fallout1),
        std::slice::from_ref(&installs.fallout1)
    );
    launcher.pick_folder(Target::Layer, dir.join("mods/one"));
    assert_eq!(known(&launcher, KnownKind::Mods), [dir.join("mods/one")]);
    launcher.pick_folder(Target::Layer, dir.join("mods/missing"));
    assert_eq!(known(&launcher, KnownKind::Mods), [dir.join("mods/one")]);
    assert_eq!(launcher.mods.entries.len(), 2);
    launcher.activate(Widget::LayerRemove);
    launcher.pick_file(Target::Layer, dir.join("mods/two.dat"));
    assert_eq!(
        known(&launcher, KnownKind::Mods),
        [dir.join("mods"), dir.join("mods/one")]
    );

    // Save: everything valid now, under its game, and it reads back.
    launcher.config.known_paths = Default::default();
    let path = launcher.save().unwrap();
    assert_eq!(
        known(&launcher, KnownKind::Fallout1),
        std::slice::from_ref(&installs.fallout1)
    );
    assert_eq!(
        known(&launcher, KnownKind::Mods),
        [dir.join("mods"), dir.join("mods/one")]
    );
    assert!(known(&launcher, KnownKind::Fallout2).is_empty());
    assert_eq!(
        Config::load(&path).unwrap().known_paths,
        launcher.config.known_paths
    );
    launcher.activate(Widget::ProfileCard(FALLOUT2));
    launcher.pick_folder(Target::Field(Field::Master), installs.fallout2.clone());
    launcher.activate(Widget::Save);
    assert_eq!(
        known(&launcher, KnownKind::Fallout2),
        std::slice::from_ref(&installs.fallout2)
    );

    // The browser offers them, and a shortcut goes there.
    launcher.open_browser(Target::Field(Field::Master));
    let browser = launcher.browser.as_ref().unwrap();
    let at = (browser.shortcuts().iter())
        .position(|shortcut| shortcut.path == installs.fallout1)
        .unwrap();
    assert_eq!(launcher.activate(Widget::KnownPath(at)), None);
    assert_eq!(launcher.focus, Widget::BrowseList);
    assert_eq!(launcher.browser.as_ref().unwrap().dir, installs.fallout1);
    launcher.activate(Widget::KnownPath(999));
    assert_eq!(launcher.browser.as_ref().unwrap().dir, installs.fallout1);
}

/// An inline list becomes a load order on its first edit. Moved beside the
/// settings it could not reach a folder outside them, as the settings can, so
/// it goes in the per-user folder, and the game reads every layer it did.
#[test]
fn an_inline_list_becomes_a_load_order_that_still_reaches_its_layers() {
    let temp = tempfile::tempdir().unwrap();
    let (dir, root) = (temp.path().join("game"), temp.path().join("user"));
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(&root).unwrap();
    let (inside, outside) = (dir.join("mods/one"), temp.path().join("elsewhere"));
    mod_folder(&inside);
    mod_folder(&outside);
    let inline = Config {
        mod_list: Some(ModList::Paths(vec![inside.clone(), outside.clone()])),
        ..Config::default()
    };
    let opened = |user_root: Option<PathBuf>| {
        let mut launcher = Launcher::new(setup(&dir, inline.clone()));
        launcher.user_root = user_root;
        launcher.revalidate_mods();
        launcher
    };

    let mut launcher = opened(Some(root.clone()));
    assert_eq!(launcher.mods.read_only, None);
    assert!(launcher.mods.check.layers.iter().all(Check::is_ok));
    // Unedited, it stays as the settings wrote it.
    launcher.save().unwrap();
    assert_eq!(launcher.config.mod_list, inline.mod_list);
    assert_eq!(launcher.mods.file, None);

    launcher.mods.list.selected = 1;
    launcher.activate(Widget::LayerToggle);
    assert!(!launcher.mods.entries[1].enabled);
    let file = root.join("load_order.toml");
    assert_eq!(launcher.mods.file, Some(file.clone()));
    launcher.activate(Widget::LayerToggle);
    launcher.mods.list.selected = 0;
    launcher.activate(Widget::LayerDown);
    assert!(launcher.mods.check.layers.iter().all(Check::is_ok));
    assert_eq!(launcher.mods.check.stack.state, State::Ok);
    launcher.save().unwrap();
    assert_eq!(launcher.config.mod_list, Some(ModList::File(file.clone())));
    let written = vault_formats::parse_load_order(&fs::read_to_string(&file).unwrap()).unwrap();
    let paths: Vec<_> = written
        .iter()
        .flat_map(|entry| entry.path.clone())
        .collect();
    assert_eq!(paths, [outside.to_string_lossy(), inside.to_string_lossy()]);
    assert!(!dir.join("load_order.toml").exists());

    // With no per-user folder the refusal shows, and Play will not hide it.
    let mut launcher = opened(None);
    launcher.activate(Widget::LayerToggle);
    assert_eq!(launcher.mods.file, None);
    assert_eq!(
        launcher.mods.entries[1].path,
        Some(outside.to_string_lossy().into())
    );
    assert_eq!(launcher.mods.check.layers[1].state, State::Bad);
    assert!(launcher.mods.ready().unwrap_err().starts_with("Mods:"));
}

#[test]
fn an_inline_list_below_the_settings_stays_beside_them() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    mod_folder(&dir.join("mods/one"));
    mod_folder(&dir.join("mods/two"));
    let lists = vec![dir.join("mods/one"), dir.join("mods/two")];
    let inline = Config {
        mod_list: Some(ModList::Paths(lists)),
        ..Config::default()
    };
    let mut launcher = Launcher::new(setup(dir, inline.clone()));
    launcher.activate(Widget::LayerToggle);
    assert_eq!(launcher.mods.entries, {
        let mut off = layer("mods/one");
        off.enabled = false;
        [off, layer("mods/two")]
    });
    assert!(launcher.mods.check.stack.is_ok());
    let path = launcher.save().unwrap();
    let saved = Config::load(&path).unwrap();
    assert_eq!(
        saved.mod_list,
        Some(ModList::File(dir.join("load_order.toml")))
    );
    let reloaded = Launcher::new(Setup {
        settings_path: Some(path),
        ..setup(dir, saved)
    });
    assert_eq!(reloaded.mods.entries, launcher.mods.entries);

    // Every layer removed: nothing is listed, nothing stays dirty, and the
    // inline list does not come back.
    let mut launcher = Launcher::new(setup(dir, inline));
    launcher.activate(Widget::LayerRemove);
    launcher.activate(Widget::LayerRemove);
    assert!(launcher.mods.entries.is_empty());
    let path = launcher.save().unwrap();
    assert!(!launcher.mods.dirty);
    assert_eq!(Config::load(&path).unwrap().mod_list, None);
}

#[test]
fn a_broken_load_order_stays_protected() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let file = dir.join("load_order.toml");
    fs::write(&file, "this is = not [a load order").unwrap();
    let config = Config {
        mod_list: Some(ModList::File(file.clone())),
        ..Config::default()
    };
    let mut launcher = Launcher::new(setup(dir, config));
    assert!(launcher.mods.broken);
    for widget in [Widget::LayerToggle, Widget::LayerUp, Widget::LayerRemove] {
        launcher.activate(widget);
    }
    assert!(!launcher.add_layer("mods/one".into()));
    assert!(
        launcher.status.starts_with("Read only"),
        "{}",
        launcher.status
    );
    assert!(!launcher.mods.dirty);
    launcher.save().unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "this is = not [a load order"
    );
    assert_eq!(launcher.config.mod_list, Some(ModList::File(file)));
}

/// Three profiles, one folder: each list is its own, and a file one profile
/// lists is never another's to write, even when the settings share it.
#[test]
fn a_new_load_order_never_lands_on_another_profiles_file() {
    use GameProfile::Fallout2;
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let settings = dir.join(config::FILE_NAME);
    let mut config = Config {
        game: Some(Fallout2),
        ..Config::default()
    };
    config.add_custom_profile("ettu", Fallout2).unwrap();
    let mut launcher = Launcher::new(Setup {
        settings_path: Some(settings.clone()),
        ..setup(dir, config)
    });
    let read = |name: &str| {
        vault_formats::parse_load_order(&fs::read_to_string(dir.join(name)).unwrap()).unwrap()
    };
    assert!(launcher.add_layer("mods/two".into()));
    launcher.save().unwrap();
    launcher.activate(Widget::ProfileCard(ETTU));
    assert!(launcher.mods.entries.is_empty());
    assert!(launcher.add_layer("mods/ettu".into()));
    launcher.save().unwrap();
    assert_eq!(read("load_order.toml"), [layer("mods/two")]);
    assert_eq!(read("load_order_ettu.toml"), [layer("mods/ettu")]);

    // An unlisted file of that name stays too.
    fs::write(dir.join("load_order_fallout1.toml"), "layers = []\n").unwrap();
    launcher.activate(Widget::ProfileCard(FALLOUT1));
    assert!(launcher.add_layer("mods/one".into()));
    launcher.save().unwrap();
    assert_eq!(
        launcher.mods.file,
        Some(dir.join("load_order_fallout1_2.toml"))
    );
    assert_eq!(read("load_order_fallout1.toml"), []);
    assert_eq!(read("load_order_fallout1_2.toml"), [layer("mods/one")]);

    // The settings give two profiles one file: an edit leaves the other's.
    let shared = ModList::File(dir.join("load_order.toml"));
    launcher.activate(Widget::ProfileCard(FALLOUT2));
    launcher.config.custom_profiles[0].scope.mod_list = Some(shared);
    launcher.activate(Widget::LayerRemove);
    launcher.save().unwrap();
    assert_eq!(
        launcher.mods.file,
        Some(dir.join("load_order_fallout2.toml"))
    );
    assert_eq!(read("load_order.toml"), [layer("mods/two")]);
    assert_eq!(read("load_order_fallout2.toml"), []);
}

/// An Et Tu folder, as far as names go.
fn et_tu_folder(dir: &Path) {
    fs::create_dir_all(dir.join("mods/fallout1in2")).unwrap();
    fs::write(dir.join("ddraw.ini"), "").unwrap();
}

#[test]
fn auto_detect_fills_a_profile_from_its_folder() {
    use GameProfile::{Fallout1, Fallout2};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let installs = Installs::new(dir);
    et_tu_folder(&installs.ettu);
    let user = dir.join("user");
    fs::create_dir_all(&user).unwrap();
    let mut config = installs.config();
    config.custom_profiles.clear();
    config.known_paths.fallout1 = vec![installs.fallout1.clone()];
    let mut launcher = Launcher::new(setup(dir, config));
    launcher.user_root = Some(user.clone());

    // An edition's own card cannot be given the other edition's data.
    launcher.set_field(Field::Master, &installs.fallout2.join("master.dat"));
    launcher.activate(Widget::AutoDetect);
    assert_eq!(launcher.status_state, State::Bad);
    assert!(
        launcher.status.contains("select a Fallout 2"),
        "{}",
        launcher.status
    );
    assert_eq!(launcher.config.install_dir, None);

    // A custom profile takes the edition of the data it is given.
    launcher.set_field(Field::Master, &installs.fallout1.join("MASTER.DAT"));
    launcher.activate(Widget::NewProfile);
    assert_eq!(launcher.game(), Fallout1);
    launcher.set_field(Field::Master, &installs.fallout2);
    launcher.activate(Widget::AutoDetect);
    assert_eq!(launcher.config.active_profile(), "profile-1");
    assert_eq!(launcher.game(), Fallout2);
    let edition = |launcher: &Launcher, name: &str| {
        let customs = &launcher.config.custom_profiles;
        customs.iter().find(|it| it.name == name).unwrap().edition
    };
    assert_eq!(edition(&launcher, "profile-1"), Fallout2);

    // Et Tu's folder goes to Et Tu's own card, from any other.
    launcher.set_field(Field::Master, &installs.ettu);
    launcher.activate(Widget::AutoDetect);
    assert_eq!(launcher.config.active_profile(), "ettu");
    assert_eq!(launcher.game(), Fallout2);
    assert_eq!(
        launcher.path(Field::Master),
        Some(installs.ettu.join("master.dat"))
    );
    assert_eq!(launcher.config.install_dir, Some(installs.ettu.clone()));
    assert_eq!(launcher.config.sfall, Some(true));
    assert_eq!(
        launcher.config.secondary_dat_dirs,
        std::slice::from_ref(&installs.fallout1)
    );
    let mods = installs.ettu.join("mods");
    assert_eq!(launcher.mods.entries, [layer("ettu/mods")]);
    assert_eq!(launcher.config.known_paths.list(KnownKind::Mods), [mods]);
    assert_eq!(
        launcher.config.known_paths.list(KnownKind::Fallout2),
        [installs.ettu.clone(), installs.fallout2.clone()]
    );
    assert_eq!(launcher.status_state, State::Ok, "{}", launcher.status);
    assert!(launcher.status.contains("Et Tu"), "{}", launcher.status);
    assert_eq!(launcher.ready(), Ok(()));
    // Again: the layer is listed once.
    launcher.activate(Widget::AutoDetect);
    assert_eq!(launcher.mods.entries.len(), 1);

    let path = launcher.save().unwrap();
    let saved = Config::load(&path).unwrap();
    assert_eq!(saved.active_profile(), "ettu");
    assert_eq!(saved.active_game(), Fallout2);
    assert_eq!(saved.install_dir, Some(installs.ettu.clone()));
    assert_eq!(
        saved.profile(Fallout2).master_dat,
        Some(installs.fallout2.join("master.dat"))
    );
    assert_eq!(
        saved.secondary_dat_dirs,
        std::slice::from_ref(&installs.fallout1)
    );

    // No ddraw.ini: `sfall` goes off, for the game that has it. In the
    // browser it looks where the browser is, and the browser closes.
    launcher.activate(Widget::ProfileCard(FALLOUT2));
    launcher.config.sfall = Some(true);
    launcher.open_browser(Target::Field(Field::Master));
    launcher
        .browser
        .as_mut()
        .unwrap()
        .go(installs.fallout2.clone());
    launcher.activate(Widget::AutoDetect);
    assert!(launcher.browser.is_none());
    assert_eq!(launcher.config.sfall, Some(false));
    assert_eq!(launcher.config.install_dir, Some(installs.fallout2.clone()));
    assert!(launcher.mods.entries.is_empty());

    // A folder with nothing in it changes nothing.
    let empty = dir.join("empty");
    fs::create_dir_all(&empty).unwrap();
    launcher.set_field(Field::Master, &empty);
    launcher.activate(Widget::AutoDetect);
    assert_eq!(launcher.status_state, State::Bad);
    assert!(
        launcher.status.starts_with("No game data"),
        "{}",
        launcher.status
    );
    assert_eq!(launcher.config.install_dir, Some(installs.fallout2.clone()));
}

/// Et Tu has a card before any setup, so the desktop menu's `--profile ettu`
/// has somewhere to go, and pointing Auto-Detect at Et Tu's folder lands there.
#[test]
fn et_tu_is_a_built_in_profile_that_auto_detect_reaches() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let launcher = Launcher::new(setup(dir, Config::default()));
    let cards = launcher.profile_cards();
    let card = cards.iter().find(|card| card.name == "ettu").unwrap();
    assert_eq!(card.title, "Fallout: Et Tu");
    assert_eq!(card.edition, GameProfile::Fallout2);
    assert!(!card.ready, "no data yet");
    // An `ettu` table the player wrote is theirs.
    let mut config = Config::default();
    config
        .add_custom_profile("ettu", GameProfile::Fallout2)
        .unwrap();
    let launcher = Launcher::new(setup(dir, config));
    let cards = launcher.profile_cards();
    assert_eq!(cards.iter().filter(|card| card.name == "ettu").count(), 1);
}
