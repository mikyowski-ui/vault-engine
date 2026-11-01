//! SPEC-024c on a whole session: a synthetic Fallout 2 install, a loose folder
//! over it as a mod's data would be, and `--game` under sfall mode.
use crate::{
    game::Game,
    map_preview::test_support::synthetic_corpus,
    preview::{AssetPreview, PreviewArgs},
};
use std::path::{Path, PathBuf};
use vault_core::{
    GameProfile::Fallout2,
    sfall::{GlobalKey, SfallValue},
};
use winit::keyboard::KeyCode;

fn hex(text: &str) -> Vec<u8> {
    text.split_whitespace()
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect()
}
/// A style word of 1 (G4), and both words set with style 2 (G5).
fn g4() -> Vec<u8> {
    hex(include_str!("../../../oracle/SPEC-024c/gcd-g4.hex"))
}
fn g5() -> Vec<u8> {
    hex(include_str!("../../../oracle/SPEC-024c/gcd-g5.hex"))
}

/// The session's folders: the install's archives, and the loose folder over
/// them.
struct Install {
    master: PathBuf,
    critter: PathBuf,
    loose: tempfile::TempDir,
    saves: tempfile::TempDir,
    _corpus: tempfile::TempDir,
    _install: tempfile::TempDir,
}
fn install() -> Install {
    let corpus = synthetic_corpus();
    vault_fixtures::fallout2::write_tables(corpus.path());
    let map = std::fs::read(corpus.path().join("maps/synthetic.map")).unwrap();
    std::fs::write(
        corpus.path().join("maps/artemple.map"),
        vault_fixtures::fallout2::named_map(map, "artemple.map", 3, 20100, 2),
    )
    .unwrap();
    let folder = tempfile::tempdir().unwrap();
    let (master, critter) = vault_fixtures::dat2::fallout2_install(corpus.path(), folder.path(), 2);
    Install {
        master,
        critter,
        loose: tempfile::tempdir().unwrap(),
        saves: tempfile::tempdir().unwrap(),
        _corpus: corpus,
        _install: folder,
    }
}
impl Install {
    fn write(&self, path: &str, bytes: &[u8]) {
        let path = self.loose.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    fn args(&self, sfall: bool) -> PreviewArgs {
        PreviewArgs {
            game_profile: Fallout2,
            master_dat: Some(self.master.clone()),
            critter_dat: Some(self.critter.clone()),
            corpus_dir: Some(self.loose.path().to_path_buf()),
            game: true,
            headless: true,
            sfall,
            save_dir: Some(self.saves.path().to_path_buf()),
            ..Default::default()
        }
    }
    fn game(&self, sfall: bool) -> Box<Game> {
        match self.args(sfall).load().unwrap() {
            Some(AssetPreview::Game(game)) => game,
            _ => panic!("the game"),
        }
    }
}
fn style_global(game: &mut Game) -> Option<SfallValue> {
    let world = game.world().expect("the world");
    let state = world.host.world.sfall.as_ref()?;
    state.globals.get(&GlobalKey::Text(*b"HApStyle")).cloned()
}
/// New game, then take the premade the screen shows.
fn take(game: &mut Game) {
    assert!(game.step(KeyCode::KeyN));
    assert_eq!(game.last_error, None);
    assert!(game.step(KeyCode::KeyT));
}

/// §2.1 and §2.4 on a session: the mod's `combat.GCD`, whose name differs from
/// the table's by case, is read at 440 bytes, and taking it sets the global.
#[test]
fn taking_a_440_byte_premade_gives_hapstyle_its_style_word() {
    let install = install();
    install.write("premade/combat.GCD", &g5());
    install.write("ddraw.ini", b"[Misc]\nEnableHeroAppearanceMod=1\n");
    let mut game = install.game(true);
    take(&mut game);
    assert_eq!(style_global(&mut game), Some(SfallValue::Int(2)));
    let world = game.world().unwrap();
    assert_eq!(world.host.world.player_sheet().unwrap().name, b"Synth");
}
/// §2.4: the same sheet with the option off, or sfall mode off, writes nothing.
#[test]
fn the_style_word_needs_the_option_and_sfall_mode() {
    let install_off = install();
    install_off.write("premade/combat.gcd", &g4());
    install_off.write("ddraw.ini", b"[Misc]\nEnableHeroAppearanceMod=0\n");
    let mut game = install_off.game(true);
    take(&mut game);
    assert_eq!(style_global(&mut game), None, "the option is off");
    let mut game = install_off.game(false);
    take(&mut game);
    assert_eq!(style_global(&mut game), None, "sfall mode is off");
    // A premade with no words writes nothing, whatever the option says.
    let plain = install();
    plain.write("ddraw.ini", b"[Misc]\nEnableHeroAppearanceMod=1\n");
    let mut game = plain.game(true);
    take(&mut game);
    assert_eq!(style_global(&mut game), None, "no words");
}

impl Install {
    /// A map of the install's shape under another name.
    fn map(&self, name: &str) {
        let map = self.args(false).source().unwrap();
        let map = map.read("maps/artemple.map").unwrap();
        let map = vault_fixtures::fallout2::named_map(map, name, 3, 20100, 2);
        self.write(&format!("maps/{name}"), &map);
    }
    /// A `critters.lst` base name of the synthetic data.
    fn critter_name(&self) -> String {
        let list = self._corpus.path().join("art/critters/critters.lst");
        let text = std::fs::read_to_string(list).unwrap();
        let first = text.lines().next().unwrap();
        first.split([',', '.']).next().unwrap().to_owned()
    }
    /// The menu's art, for a test that presses a button.
    fn menu_art(&self) {
        let frm = vault_fixtures::world::frm;
        self.write("art/intrface/mainmenu.frm", &frm(640, 480, 3));
        self.write("art/intrface/menuup.frm", &frm(26, 26, 90));
        self.write("art/intrface/menudown.frm", &frm(26, 26, 91));
    }
}
/// §6.1 test 10's shape on synthetic data: the file says where a new game
/// starts, and when.
#[test]
fn a_new_game_starts_where_and_when_the_file_says() {
    let install = install();
    install.map("v13ent.map");
    install.write(
        "ddraw.ini",
        b"[Misc]\nStartingMap=V13ent.map\nStartYear=2161\nStartMonth=11\nStartDay=4\n\
          StartTime=1530\nStartXPos=300\nStartYPos=200\n",
    );
    let mut game = install.game(true);
    take(&mut game);
    let world = game.world().expect("the world");
    let host = &world.host;
    assert_eq!(&host.world.map.header.name[..6], b"v13ent");
    assert_eq!(host.world.clock.ticks, (60 * 15 + 30) * 600);
    assert_eq!(host.world.game_date(), [12, 5, 2161]);
    assert_eq!(host.world.fallout2.as_deref().unwrap().position, [300, 200]);
}
/// §3.4: an absent map falls back to `artemple`, and §3.7: a point outside
/// the world is dropped. Neither stops the start.
#[test]
fn a_bad_start_falls_back_cleanly_with_diagnostics() {
    let install = install();
    install.write(
        "ddraw.ini",
        b"[Misc]\nStartingMap=absent.map\nStartXPos=99999\nStartYPos=1\n",
    );
    let mut game = install.game(true);
    take(&mut game);
    let world = game.world().expect("the world");
    assert_eq!(&world.host.world.map.header.name[..8], b"artemple");
    assert_eq!(
        world.host.world.fallout2.as_deref().unwrap().position,
        [175, 125]
    );
}
/// §6.1 test 5, and §3.1: with sfall mode off the file is never opened.
#[test]
fn with_sfall_mode_off_the_file_is_never_read() {
    let install = install();
    install.map("v13ent.map");
    install.write(
        "ddraw.ini",
        b"[Misc]\nStartingMap=V13ent.map\nStartTime=1530\n",
    );
    let mut game = install.game(false);
    assert!(game.startup.is_none());
    take(&mut game);
    let world = game.world().expect("the world");
    let host = &world.host;
    assert_eq!(&host.world.map.header.name[..8], b"artemple");
    assert_eq!(host.world.clock.ticks, vault_core::fallout2::START_TICKS);
    assert_eq!(host.world.game_date(), [7, 25, 2241]);
}
/// §3.1's lookup: the `Vfs` layers, the writable folder, the install folder,
/// with the name compared without case. No file gives every default.
#[test]
fn the_file_is_found_in_its_order_without_case() {
    use crate::session::read_startup;
    use vault_formats::LocalDirProvider;
    let ini = |year: i32| format!("[Misc]\nStartYear={year}\n");
    let (layer, writable, folder) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let year =
        |startup: std::sync::Arc<vault_core::sfall_startup::StartupOptions>| startup.calendar().0;
    let read = |vfs: &LocalDirProvider| {
        year(read_startup(
            vfs,
            Some(writable.path()),
            Some(folder.path()),
        ))
    };
    let empty = LocalDirProvider::mount(layer.path()).unwrap();
    assert_eq!(read(&empty), 2241, "no file");
    std::fs::write(folder.path().join("DDRAW.INI"), ini(2001)).unwrap();
    assert_eq!(read(&empty), 2001, "the install folder");
    std::fs::write(writable.path().join("Ddraw.ini"), ini(2002)).unwrap();
    assert_eq!(read(&empty), 2002, "the writable folder first");
    std::fs::write(layer.path().join("ddraw.INI"), ini(2003)).unwrap();
    let mounted = LocalDirProvider::mount(layer.path()).unwrap();
    assert_eq!(read(&mounted), 2003, "the layers first");
    // Only that name: an input to a mod's own build is never read.
    let (none, other) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    std::fs::write(other.path().join("ddraw.fo1in2.ini"), ini(2004)).unwrap();
    let none = LocalDirProvider::mount(none.path()).unwrap();
    assert_eq!(year(read_startup(&none, None, Some(other.path()))), 2241);
}
/// §3.9: the title, and the buttons where the file moved them.
#[test]
fn the_main_menu_takes_the_files_title_and_positions() {
    let install = install();
    install.menu_art();
    install.write("font0.aaf", &vault_fixtures::state::preview_aaf());
    let before = install.game(true).image().unwrap();
    install.write("ddraw.ini", b"[Misc]\nVersionString=VAULT %d.%d\n");
    let titled = install.game(true).image().unwrap();
    install.write(
        "ddraw.ini",
        b"[Misc]\nVersionString=VAULT %d.%d\nMainMenuOffsetX=100\nMainMenuOffsetY=10\n\
          MainMenuFontColour=0x000055\n",
    );
    let mut game = install.game(true);
    let image = game.image().unwrap();
    let at = |x: i32, y: i32| (y * 640 + x) as usize;
    // Options, button 3: (30, 142), now 100 right and 10 down.
    let (old, new) = ([30 + 13, 142 + 13], [130 + 13, 152 + 13]);
    assert_eq!(before.pixels()[at(old[0], old[1])], 90);
    assert_eq!(image.pixels()[at(new[0], new[1])], 90);
    assert_eq!(image.pixels()[at(old[0], old[1])], 3, "not where it was");
    // The version line: the file's text, then its colour.
    let line = |image: &vault_render::IndexedImage| image.pixels()[at(0, 460)..at(0, 472)].to_vec();
    assert_ne!(line(&titled), line(&before), "the version text");
    assert_ne!(line(&image), line(&titled), "the version's colour");
    // The old place presses nothing, and the new one the button.
    assert!(!game.click(old, false) || !game.world_shown());
    game.release(Some(old), false);
    assert!(!game.world_shown());
    assert!(game.click(new, false));
    assert!(game.release(Some(new), false));
    assert!(game.world_shown(), "Options opened");
}
/// §3.10: the listed premades replace the three, in order.
#[test]
fn the_premade_list_replaces_the_three() {
    let install = install();
    install.write("premade/hero.gcd", &g5());
    install.write("premade/rival.gcd", &g4());
    install.write(
        "ddraw.ini",
        b"[Misc]\nEnableHeroAppearanceMod=1\nPremadePaths=hero,rival\nPremadeFIDs=1,2\n",
    );
    let mut game = install.game(true);
    assert!(game.step(KeyCode::KeyN));
    // The second of two is `rival`, with style 1: right once, then take.
    assert!(game.step(KeyCode::ArrowRight));
    assert!(game.step(KeyCode::KeyT));
    assert_eq!(style_global(&mut game), Some(SfallValue::Int(1)));
    assert_eq!(game.premade_count(), 2);
}
/// §3.8: the start model is the player's art at a new game, and an unknown
/// name changes nothing.
#[test]
fn the_start_model_names_the_players_art() {
    let install = install();
    let name = install.critter_name();
    install.write(
        "ddraw.ini",
        format!("[Misc]\nMaleStartModel={name}\n").as_bytes(),
    );
    let mut game = install.game(true);
    take(&mut game);
    let world = game.world().unwrap();
    let player = world.host.world.player;
    let base = vault_formats::Fid(world.host.world.get(player).unwrap().raw.art_id).base();
    let row = vault_core::assets::WorldAssets::art_row(&world.host.assets, 1, name.as_bytes());
    assert_eq!(Some(base), row);
    let bad = self::install();
    bad.write("ddraw.ini", b"[Misc]\nMaleStartModel=nosuch\n");
    let mut plain = self::install().game(true);
    take(&mut plain);
    let mut game = bad.game(true);
    take(&mut game);
    let art = |game: &mut Game| {
        let world = game.world().unwrap();
        world
            .host
            .world
            .get(world.host.world.player)
            .unwrap()
            .raw
            .art_id
    };
    assert_eq!(art(&mut game), art(&mut plain), "the existing rule stands");
}
/// §3.7: the first view, once, and only drawing changes.
#[test]
fn the_first_world_map_view_is_the_files() {
    let install = install();
    // Six tiles, so a view has somewhere to be.
    let tiles: String = (1..6)
        .map(|tile| format!("[Tile {tile}]\nart_idx=339\n"))
        .collect();
    let table = vault_fixtures::fallout2::WORLDMAP
        .replace("num_horizontal_tiles=1", "num_horizontal_tiles=3")
        .replace(
            "[Random Maps: Desert]",
            &format!("{tiles}[Random Maps: Desert]"),
        );
    install.write("data/worldmap.txt", table.as_bytes());
    install.write("ddraw.ini", b"[Misc]\nViewXPos=40\nViewYPos=8\n");
    let mut game = install.game(true);
    take(&mut game);
    let world = game.world().unwrap();
    let position = world.host.world.fallout2.as_deref().unwrap().position;
    world.open_world2();
    assert_eq!(world.world2_view(), Some([40, 8]));
    assert_eq!(
        world.host.world.fallout2.as_deref().unwrap().position,
        position
    );
    assert_eq!(world.host.take_first_view(), None, "the first view only");
}
