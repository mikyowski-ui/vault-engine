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
