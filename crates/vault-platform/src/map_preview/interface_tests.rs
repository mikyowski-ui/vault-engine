//! SPEC-013 and SPEC-014 windows in the interactive world, and the game
//! flow, over the synthetic corpus.
use super::{MapPreview, Screen, state_fixture};
use crate::game::Game;
use std::sync::Arc;
use vault_core::{
    LootRequest, WorldEffect,
    critical::effect,
    ui::{cursor::Mode, saveload::Mode as SaveMode},
};
use vault_formats::{LocalDirProvider, Vfs, map::ObjectState, pro::Category};
use winit::keyboard::KeyCode;

/// The synthetic corpus, with its map also under a save-manifest-sized
/// name and as the new-game map.
fn corpus() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    state_fixture::write_corpus(dir.path());
    let map = std::fs::read(dir.path().join("maps/synthetic.map")).unwrap();
    std::fs::write(dir.path().join("maps/synth.map"), &map).unwrap();
    std::fs::write(dir.path().join("maps/v13ent.map"), &map).unwrap();
    dir
}
fn world(dir: &std::path::Path) -> MapPreview {
    let vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(dir).unwrap());
    let mut preview = MapPreview::load(
        vfs,
        "maps/synth.map",
        "english",
        Some("premade/synthetic.gcd"),
        1,
    )
    .unwrap();
    preview.save_root = dir.join("saves");
    preview
}
fn npc(preview: &MapPreview) -> vault_core::ObjectRef {
    let player = preview.host.world.player;
    preview
        .host
        .world
        .iter()
        .find(|(reference, object)| object.category() == Category::Critter && *reference != player)
        .unwrap()
        .0
}

#[test]
fn spec014_bar_monitor_and_hotkeys() {
    let dir = corpus();
    let mut preview = world(dir.path());
    // The map frame carries the bar below the 640-by-380 view.
    assert_eq!(preview.image().unwrap().height(), 480);
    preview
        .host
        .world
        .effects
        .push_back(WorldEffect::Message(b"Hello".to_vec()));
    preview.image().unwrap();
    let visible = preview.hud.monitor.visible();
    assert_eq!(
        visible.last().map(|line| &line[1..]),
        Some(b"Hello".as_slice())
    );
    assert_eq!(visible.last().unwrap()[0], vault_core::ui::bar::BULLET);
    // Bar buttons: inventory (211, 41) and the Skilldex (523, 7) under y 379.
    assert!(preview.click([220, 379 + 50], false));
    assert!(matches!(preview.screen, Screen::Inventory(_)));
    assert!(preview.step(KeyCode::Escape));
    assert!(preview.click([530, 379 + 15], false));
    assert!(matches!(preview.screen, Screen::Skilldex(_)));
    assert!(preview.step(KeyCode::Escape));
    // Hotkey 2 selects Lockpick targeting; a right-click returns to movement.
    assert!(preview.step(KeyCode::Digit2));
    assert_eq!(preview.hud.mode, Mode::Skill(9));
    assert!(preview.click([336, 170], true));
    assert_eq!(preview.hud.mode, Mode::Movement);
    // Hotkey 1 toggles Sneak; the indicator follows the flag.
    assert!(preview.step(KeyCode::Digit1));
    assert!(preview.host.sneaking());
    // b swaps the active hand.
    let hand = preview.host.world.active_hand;
    assert!(preview.step(KeyCode::KeyB));
    assert_eq!(preview.host.world.active_hand, hand ^ 1);
}

#[test]
fn spec013_skilldex_rows_and_targeting() {
    let dir = corpus();
    let mut preview = world(dir.path());
    preview.image().unwrap();
    assert!(preview.step(KeyCode::KeyS));
    preview.image().unwrap();
    // Row 1 (Lockpick) at (451 + 15, 5 + 45 + 36).
    assert!(preview.click([451 + 20, 5 + 45 + 36 + 5], false));
    assert!(matches!(preview.screen, Screen::Map));
    assert_eq!(preview.hud.mode, Mode::Skill(9));
    // Row 0 toggles Sneak and leaves the mode alone.
    assert!(preview.step(KeyCode::KeyS));
    assert!(preview.click([451 + 20, 5 + 45 + 5], false));
    assert!(preview.host.sneaking());
    // A targeting click with no object returns to movement.
    assert!(preview.click([5, 5], false));
    assert_eq!(preview.hud.mode, Mode::Movement);
}

#[test]
fn spec013_elevator_window_moves_the_player() {
    let dir = corpus();
    let mut preview = world(dir.path());
    preview.host.world.map.header.map_number = 6;
    let player = preview.host.world.player;
    preview
        .host
        .world
        .move_object(player, 20100, 2, false)
        .unwrap();
    preview
        .host
        .world
        .effects
        .push_back(WorldEffect::Elevator(8));
    preview.image().unwrap();
    assert!(matches!(&preview.screen, Screen::Elevator(screen) if screen.start == 2));
    assert!(preview.step(KeyCode::Digit1));
    assert!(matches!(preview.screen, Screen::Map));
    let object = preview.host.world.get(player).unwrap();
    assert_eq!((object.raw.hex, object.raw.elevation), (Some(14104), 0));
    // Escape cancels without moving (the safe rule).
    preview
        .host
        .world
        .effects
        .push_back(WorldEffect::Elevator(8));
    preview.image().unwrap();
    assert!(preview.step(KeyCode::Escape));
    assert_eq!(preview.host.world.get(player).unwrap().raw.hex, Some(14104));
}

#[test]
fn spec013_loot_window_moves_items_between_panes() {
    let dir = corpus();
    let mut preview = world(dir.path());
    let player = preview.host.world.player;
    let npc = npc(&preview);
    if let Some(ObjectState::Critter(state)) =
        preview.host.world.get_mut(npc).map(|o| &mut o.raw.state)
    {
        state.results.0 |= effect::DEAD;
    }
    let item = preview
        .host
        .world
        .iter()
        .find(|(_, object)| object.category() == Category::Item)
        .unwrap()
        .0;
    preview.host.world.add_item(npc, item, 1, true).unwrap();
    preview.host.world.pending_loot = Some(LootRequest {
        looter: player,
        target: npc,
        steal: false,
    });
    preview.image().unwrap();
    assert!(matches!(preview.screen, Screen::Loot(_)));
    // Lift the target pane's first entry, then drop it on the player pane.
    assert!(preview.click([80 + 430, 35 + 10], false));
    assert!(preview.click([130, 100], false));
    assert_eq!(preview.host.world.get(item).unwrap().owner, Some(player));
    assert!(preview.step(KeyCode::Escape));
    assert!(matches!(preview.screen, Screen::Map));
}

#[test]
fn spec014_options_quit_and_preferences() {
    let dir = corpus();
    let mut preview = world(dir.path());
    preview.config_path = Some(dir.path().join("fallout.cfg"));
    preview.image().unwrap();
    assert!(preview.step(KeyCode::Escape));
    assert!(matches!(preview.screen, Screen::Options(_)));
    preview.image().unwrap();
    // PREFERENCES (button 2), then a click on the subtitles switch's center
    // (it toggles) and Done write the configuration.
    let [ox, oy] = vault_core::ui::options::OPTIONS_ORIGIN;
    assert!(preview.click([ox + 20, oy + 17 + 2 * 37 + 5], false));
    preview.image().unwrap();
    assert!(preview.click([299 + 11, 338 + 12], false));
    // The game-difficulty label 1 (the current value) still turns the knob
    // downward, so the next center click steps to 0.
    assert!(preview.click([76 + 20, 71 - 2], false));
    assert!(preview.click([76 + 23, 71 + 21], false));
    assert!(preview.step(KeyCode::Enter));
    assert_eq!(preview.preferences.subtitles, 1);
    assert_eq!(preview.preferences.game_difficulty, 0);
    let config = std::fs::read_to_string(dir.path().join("fallout.cfg")).unwrap();
    assert!(config.contains("subtitles=1"), "{config}");
    // EXIT GAME, then Yes, requests the return to the main menu.
    assert!(preview.click([ox + 20, oy + 17 + 3 * 37 + 5], false));
    assert!(preview.step(KeyCode::KeyY));
    assert!(preview.quit_requested);
}

#[test]
fn spec014_save_and_load_screens() {
    let dir = corpus();
    let mut preview = world(dir.path());
    preview.image().unwrap();
    // Ctrl+S opens Save; Enter on the empty slot 1 asks for a description.
    preview.set_modifiers([false, true, false]);
    assert!(preview.step(KeyCode::KeyS));
    preview.set_modifiers([false, false, false]);
    assert!(matches!(&preview.screen, Screen::SaveLoad(screen) if screen.mode == SaveMode::Save));
    preview.image().unwrap();
    assert!(preview.step(KeyCode::Enter));
    preview.set_modifiers([true, false, false]);
    assert!(preview.step(KeyCode::KeyA));
    preview.set_modifiers([false, false, false]);
    assert!(preview.step(KeyCode::Enter));
    assert!(matches!(preview.screen, Screen::Map));
    let slot = dir.path().join("saves/SAVEGAME/SLOT01/SAVE.DAT");
    assert!(slot.exists(), "{}", preview.status.escape_ascii());
    // F5 opens Load; the occupied slot loads.
    let ticks = preview.host.world.clock.ticks;
    preview.host.world.clock.ticks += 5000;
    assert!(preview.step(KeyCode::F5));
    preview.image().unwrap();
    assert!(preview.step(KeyCode::Enter));
    assert!(preview.loaded);
    assert_eq!(preview.host.world.clock.ticks, ticks);
    // F6 picks a quick-save slot first.
    assert!(preview.step(KeyCode::F6));
    assert!(
        matches!(&preview.screen, Screen::SaveLoad(screen) if screen.mode == SaveMode::PickQuickSave)
    );
}

#[test]
fn spec014_save_and_load_failures_show_done_boxes() {
    let dir = corpus();
    let mut preview = world(dir.path());
    let save = |preview: &mut MapPreview| {
        preview.set_modifiers([false, true, false]);
        assert!(preview.step(KeyCode::KeyS));
        preview.set_modifiers([false, false, false]);
        assert!(preview.step(KeyCode::Enter));
        assert!(preview.step(KeyCode::KeyA));
        assert!(preview.step(KeyCode::Enter));
    };
    // A save root that is a file: 132 / 133, the screen stays open.
    let file = dir.path().join("not-a-directory");
    std::fs::write(&file, b"x").unwrap();
    preview.save_root = file;
    save(&mut preview);
    assert!(preview.notice.is_some());
    assert!(matches!(preview.screen, Screen::SaveLoad(_)));
    preview.image().unwrap();
    // Other keys are swallowed; Enter dismisses.
    assert!(preview.step(KeyCode::ArrowDown));
    assert!(preview.notice.is_some());
    assert!(preview.step(KeyCode::Enter));
    assert!(preview.notice.is_none() && !preview.quit_requested);
    assert!(matches!(preview.screen, Screen::SaveLoad(_)));
    assert!(preview.step(KeyCode::Escape));
    // A corrupt slot: 134 with the safe-rule body; nothing loads.
    preview.save_root = dir.path().join("saves");
    let slot2 = dir.path().join("saves/SAVEGAME/SLOT02");
    std::fs::create_dir_all(&slot2).unwrap();
    std::fs::write(slot2.join("SAVE.DAT"), b"short").unwrap();
    assert!(preview.step(KeyCode::F5));
    assert!(preview.step(KeyCode::ArrowDown));
    assert!(preview.step(KeyCode::Enter));
    assert!(preview.notice.is_some());
    assert!(!preview.loaded);
    assert!(preview.step(KeyCode::Escape));
    assert!(matches!(preview.screen, Screen::SaveLoad(_)) && !preview.quit_requested);
    assert!(preview.step(KeyCode::Escape));
    // An occupied slot that fails to load: 134 / 135; dismissing closes
    // the screen and requests the main menu. The selection stays on slot
    // 2, which the save overwrites.
    save(&mut preview);
    assert!(matches!(preview.screen, Screen::Map));
    let path = slot2.join("SAVE.DAT");
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &bytes[..vault_formats::savegame::HEADER_LEN]).unwrap();
    assert!(preview.step(KeyCode::F5));
    assert!(preview.step(KeyCode::Enter));
    assert!(preview.notice.is_some() && !preview.loaded);
    let [rx, ry] = vault_core::ui::flow::dialog_buttons(false)[0].button;
    assert!(preview.click([169 + rx + 2, 116 + ry + 2], false));
    assert!(preview.notice.is_none());
    assert!(matches!(preview.screen, Screen::Map) && preview.quit_requested);
}

#[test]
fn spec014_game_flow_menu_selection_editor_and_world() {
    let dir = corpus();
    let vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(dir.path()).unwrap());
    let mut game = Game::new(vfs, "english", 1, dir.path().join("saves"), None).unwrap();
    // No splash or start movies in the corpus: straight to the menu.
    assert_eq!(game.title(), "Vault — main menu");
    game.image().unwrap();
    // n: premade selection; c: the editor on a blank sheet.
    assert!(game.step(KeyCode::KeyN));
    assert!(game.title().contains("character selection"));
    game.image().unwrap();
    assert!(game.step(KeyCode::KeyC));
    assert!(game.title().contains("editor"));
    game.image().unwrap();
    // Done with tag picks remaining shows the refusal; Escape cancels back.
    assert!(game.step(KeyCode::Enter));
    game.image().unwrap();
    assert!(game.title().contains("editor"));
    assert!(game.step(KeyCode::Escape));
    assert!(game.step(KeyCode::Escape));
    assert!(game.title().contains("character selection"));
    // t takes the premade: ovrintro is missing, so the world starts.
    assert!(game.step(KeyCode::KeyT));
    let world = game.world().expect("the world");
    assert_eq!(
        world.host.world.clock.ticks,
        vault_core::ui::flow::START_TICKS
    );
    game.image().unwrap();
    game.tick(16).unwrap();
    // Quitting from the options returns to the menu; Escape there exits.
    assert!(game.step(KeyCode::Escape));
    let [ox, oy] = vault_core::ui::options::OPTIONS_ORIGIN;
    assert!(game.click([ox + 20, oy + 17 + 3 * 37 + 5], false));
    assert!(game.step(KeyCode::KeyY));
    game.tick(32).unwrap();
    assert_eq!(game.title(), "Vault — main menu");
    assert!(game.step(KeyCode::Escape));
    assert!(game.finished());
}

#[test]
fn spec013_set_timer_arms_an_explosive_from_the_inventory() {
    let dir = corpus();
    let mut preview = world(dir.path());
    let player = preview.host.world.player;
    let mut bytes = super::fixture::prototype(0, 5);
    super::fixture::word(&mut bytes, 0, vault_core::skills::DYNAMITE as i32);
    let prototype = Arc::new(vault_formats::Prototype::parse(&bytes).unwrap());
    let dynamite = preview.host.world.spawn(prototype, 0, 0).unwrap();
    preview
        .host
        .world
        .add_item(player, dynamite, 1, true)
        .unwrap();
    preview.image().unwrap();
    assert!(preview.step(KeyCode::KeyI));
    let row = preview
        .host
        .world
        .get(player)
        .unwrap()
        .inventory
        .iter()
        .position(|(item, _)| *item == dynamite)
        .unwrap();
    assert!(preview.click([30, 62 + 16 * row as i32], false));
    // Use opens the Set Timer dialog at 60 seconds; plus makes it 70.
    assert!(preview.click([450, 65], false));
    assert!(matches!(&preview.screen, Screen::Inventory(screen) if screen.timer.is_some()));
    preview.image().unwrap();
    assert!(preview.click([140 + 196, 80 + 66], false));
    assert!(preview.step(KeyCode::Enter));
    let object = preview.host.world.get(dynamite).unwrap();
    assert_eq!(object.raw.pid, vault_core::skills::DYNAMITE_ARMED);
    let event = preview
        .host
        .world
        .queue
        .events()
        .find(|event| event.object == dynamite)
        .unwrap();
    let delay = event.due - preview.host.world.clock.ticks;
    assert!(matches!(delay, 0 | 350 | 700), "70 seconds: {delay}");
    // A second use refuses: the timer is already ticking.
    assert!(preview.click([450, 65], false));
    assert!(matches!(&preview.screen, Screen::Inventory(screen) if screen.timer.is_none()));
}
