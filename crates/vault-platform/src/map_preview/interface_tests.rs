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
    assert!(preview.tap([5, 5]));
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
    // Lift the target pane's first entry (the press), then drop it on the
    // player pane (the release).
    assert!(preview.click([80 + 430, 35 + 10], false));
    assert!(preview.release(Some([130, 100]), false));
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
    assert!(preview.dialog.is_some());
    assert!(matches!(preview.screen, Screen::SaveLoad(_)));
    preview.image().unwrap();
    // Other keys are swallowed; Enter dismisses.
    assert!(preview.step(KeyCode::ArrowDown));
    assert!(preview.dialog.is_some());
    assert!(preview.step(KeyCode::Enter));
    assert!(preview.dialog.is_none() && !preview.quit_requested);
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
    assert!(preview.dialog.is_some());
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
    assert!(preview.dialog.is_some() && !preview.loaded);
    let [rx, ry] = vault_core::ui::flow::dialog_buttons(false)[0].button;
    assert!(preview.click([169 + rx + 2, 116 + ry + 2], false));
    assert!(preview.dialog.is_none());
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
    // §3.5: the session ends with a fade to white over the world, then the
    // menu fades in from white; Escape there exits.
    game.tick(32).unwrap();
    assert!(game.world().is_none());
    game.image().unwrap();
    game.tick(32 + 700).unwrap();
    assert_eq!(game.title(), "Vault — main menu");
    assert_eq!(game.display_override().unwrap()[1], [252, 252, 252, 255]);
    game.tick(32 + 1400).unwrap();
    assert_eq!(game.display_override(), None);
    assert!(game.step(KeyCode::Escape));
    assert!(game.finished());
}

#[test]
fn spec014_knockout_and_death_end_the_session_between_passes() {
    let dir = corpus();
    let vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(dir.path()).unwrap());
    let mut game = Game::new(vfs, "english", 1, dir.path().join("saves"), None).unwrap();
    assert!(game.step(KeyCode::KeyN));
    assert!(game.step(KeyCode::KeyT));
    game.tick(16).unwrap();
    // A knockout outside combat ends the session at the end of the pass
    // (in combat the check waits, and the cleanup normally wakes the
    // player): white, then the death scene fading from white, then black,
    // then the menu.
    let world = game.world().unwrap();
    let player = world.host.world.player;
    world
        .host
        .world
        .critter_state_mut(player)
        .unwrap()
        .results
        .0 |= 0x01;
    game.tick(48).unwrap();
    assert!(game.world().is_none());
    game.tick(48 + 700).unwrap();
    assert_eq!(game.display_override().unwrap()[1], [252, 252, 252, 255]);
    game.image().unwrap();
    let mut now = 48 + 700;
    while game.title() != "Vault — main menu" {
        now += 50;
        game.tick(now).unwrap();
        assert!(now < 20_000, "the death scene ends");
    }
    // The menu fades in from black.
    assert_eq!(game.display_override().unwrap()[1], [0, 0, 0, 255]);
}

/// The inventory list point of `item` (SPEC-014 §3.34 row (124, 35 +
/// 48i), unequipped entries in order).
fn list_point(preview: &MapPreview, item: vault_core::ObjectRef) -> [i32; 2] {
    let world = &preview.host.world;
    let row = world
        .get(world.player)
        .unwrap()
        .inventory
        .iter()
        .filter(|(entry, _)| {
            world.get(*entry).unwrap().raw.flags.0 & vault_core::items::EQUIPPED == 0
        })
        .position(|(entry, _)| *entry == item)
        .unwrap();
    [134, 45 + 48 * row as i32]
}
/// In the arrow cursor, hold a press on `at` past the 250 ms pulse to open
/// the item menu, move down one entry (Look, then Use) and release.
fn menu_second_entry(preview: &mut MapPreview, at: [i32; 2], now: &mut u64) {
    preview.hover(at);
    assert!(preview.click(at, false));
    *now += 300;
    preview.tick(*now).unwrap();
    assert!(matches!(&preview.screen, Screen::Inventory(screen) if screen.menu.is_some()));
    preview.image().unwrap();
    let below = [at[0], at[1] + 11];
    preview.hover(below);
    assert!(preview.release(Some(below), false));
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
    // SPEC-014 §3.34: a right press switches to the arrow cursor; held on
    // the dynamite it opens Look, Use, Drop, Cancel. Use opens the Set
    // Timer dialog at 60 seconds; plus makes it 70.
    let at = list_point(&preview, dynamite);
    assert!(preview.click(at, true));
    let mut now = 0;
    menu_second_entry(&mut preview, at, &mut now);
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
    menu_second_entry(&mut preview, at, &mut now);
    assert!(matches!(&preview.screen, Screen::Inventory(screen) if screen.timer.is_none()));
}

/// A frame pixel on the critter or item at `hex` (sprites stand on the
/// hex center).
fn on_hex(preview: &MapPreview, hex: i32) -> [i32; 2] {
    let [x, y] = preview.view.grid.hex_center(hex).unwrap();
    [x as i32, y as i32 - 5]
}

#[test]
fn spec014_hover_feedback_waits_for_a_rest_and_draws_the_cursor() {
    let dir = corpus();
    for (name, size) in [("msef003.frm", [32, 16]), ("stdarrow.frm", [18, 18])] {
        std::fs::write(
            dir.path().join("art/intrface").join(name),
            super::fixture::frm(size[0], size[1], 9),
        )
        .unwrap();
    }
    let mut preview = world(dir.path());
    let player = preview.host.world.player;
    let target = npc(&preview);
    preview.tick(16).unwrap();
    let base = preview.image().unwrap();
    // No pointer, no cursor; with one, the drawn cursor changes the frame.
    let player_hex = preview.host.world.get(player).unwrap().raw.hex.unwrap();
    let own = on_hex(&preview, player_hex);
    preview.hover([own[0], own[1] + 5]);
    let drawn = preview.pointer_frame().unwrap();
    assert_ne!(drawn.pixels(), base.pixels());
    // Movement: nothing before 250 ms of rest; then the player's own hex
    // has no path, a red X.
    preview.tick(200).unwrap();
    assert_eq!(preview.pointer.feedback, None);
    preview.tick(300).unwrap();
    assert_eq!(
        preview.pointer.feedback,
        Some(super::pointer::Feedback::Cost(
            b"X".to_vec(),
            vault_core::ui::ColorKey::RED
        ))
    );
    // Arrow over the talkable NPC: the talk icon, and its look-at text
    // reaches the monitor once. Moving resets the art.
    // The mode change restarts the rest (§3.27).
    preview.hud.mode = Mode::Arrow;
    let hex = preview.host.world.get(target).unwrap().raw.hex.unwrap();
    let point = on_hex(&preview, hex);
    preview.hover(point);
    preview.tick(600).unwrap();
    assert_eq!(preview.pointer.feedback, None);
    preview.tick(900).unwrap();
    assert_eq!(
        preview.pointer.feedback,
        Some(super::pointer::Feedback::Pick("talkn.frm"))
    );
    let name = preview.host.world.get(target).unwrap().name.clone();
    assert!(preview.status.ends_with(&name), "{:?}", preview.status);
    preview.image().unwrap();
    preview.hover([point[0] + 1, point[1]]);
    assert_eq!(preview.pointer.feedback, None);
    // Out of the map area the arrow is drawn and no hover runs.
    preview.hover([320, 440]);
    preview.tick(1300).unwrap();
    assert_eq!(preview.pointer.feedback, None);
    assert!(preview.pointer_frame().is_some());
}

#[test]
fn spec014_action_menu_opens_on_a_held_press_and_runs_the_entry() {
    let dir = corpus();
    let mut preview = world(dir.path());
    let target = npc(&preview);
    preview.tick(16).unwrap();
    preview.image().unwrap();
    preview.hud.mode = Mode::Arrow;
    let hex = preview.host.world.get(target).unwrap().raw.hex.unwrap();
    let point = on_hex(&preview, hex);
    preview.hover(point);
    assert!(preview.click(point, false));
    // The first repeat event comes after more than 250 ms.
    preview.tick(266).unwrap();
    assert!(preview.pointer.menu.is_none());
    preview.tick(282).unwrap();
    let menu = preview.pointer.menu.as_ref().expect("the action menu");
    use vault_core::ui::cursor::Entry;
    assert_eq!(
        menu.entries,
        [
            Entry::Talk,
            Entry::Look,
            Entry::Inventory,
            Entry::Skill,
            Entry::Cancel
        ]
    );
    // The world pauses; each move of more than 10 pixels moves the
    // highlight one entry.
    let ticks = preview.host.world.clock.ticks;
    preview.tick(2000).unwrap();
    assert_eq!(preview.host.world.clock.ticks, ticks);
    preview.hover([point[0], point[1] + 11]);
    preview.hover([point[0] + 3, point[1] + 22]);
    assert_eq!(preview.pointer.menu.as_ref().unwrap().tracker.highlight, 2);
    preview.hover([point[0], point[1] + 12]);
    assert_eq!(preview.pointer.menu.as_ref().unwrap().tracker.highlight, 2);
    preview.hover([point[0] + 5, point[1] + 11]);
    assert_eq!(preview.pointer.menu.as_ref().unwrap().tracker.highlight, 1);
    preview.image().unwrap();
    // The release runs Look (examine) and warps the pointer to the menu's
    // column at the last anchor.
    assert!(preview.release(Some([point[0] + 5, point[1] + 11]), false));
    assert!(preview.pointer.menu.is_none());
    assert_eq!(preview.pointer.at, Some([point[0], point[1] + 11]));
    assert!(!preview.host.world.dialogue.open);
    // A second hold without moving runs Talk.
    preview.hover(point);
    assert!(preview.click(point, false));
    preview.tick(2400).unwrap();
    assert!(preview.pointer.menu.is_some());
    assert!(preview.release(Some(point), false));
    assert!(preview.host.world.dialogue.open);
}

#[test]
fn spec014_edge_scrolling_steps_at_most_every_33_ms_until_blocked() {
    let dir = corpus();
    let mut preview = world(dir.path());
    preview.tick(16).unwrap();
    preview.image().unwrap();
    let start = preview.host.world.camera_hex;
    preview.hover([639, 200]);
    assert!(preview.tick(100).unwrap());
    let first = preview.host.world.camera_hex;
    assert_ne!(first, start);
    let edge = preview.pointer.edge.unwrap();
    assert_eq!((edge.direction, edge.blocked), (3, false));
    // Throttled within 33 ms; the next step after.
    preview.tick(120).unwrap();
    assert_eq!(preview.host.world.camera_hex, first);
    preview.tick(133).unwrap();
    assert_ne!(preview.host.world.camera_hex, first);
    preview.image().unwrap();
    // A press on an edge pixel never clicks the map.
    assert!(preview.click([639, 200], false));
    assert!(preview.release(Some([639, 200]), false));
    // Far enough east the view's center leaves the grid: blocked.
    let mut now = 133;
    for _ in 0..400 {
        now += 40;
        preview.tick(now).unwrap();
    }
    assert!(preview.pointer.edge.unwrap().blocked);
    let stopped = preview.host.world.camera_hex;
    now += 40;
    preview.tick(now).unwrap();
    assert_eq!(preview.host.world.camera_hex, stopped);
    // Leaving the edge restores the cursor.
    preview.hover([320, 200]);
    preview.tick(now + 40).unwrap();
    assert!(preview.pointer.edge.is_none());
    preview.pointer_left();
    assert!(preview.pointer_frame().is_some());
}

#[test]
fn spec014_world_fades_run_in_order_over_700_ms() {
    let dir = corpus();
    let mut preview = world(dir.path());
    preview.tick(16).unwrap();
    assert_eq!(preview.display_override(), None);
    for out in [true, false] {
        preview.host.world.emit(WorldEffect::Fade {
            out,
            duration: 0,
            from_black: false,
        });
    }
    preview.tick(1000).unwrap();
    preview.tick(1350).unwrap();
    let halfway = preview.display_override().expect("fading");
    preview.tick(1700).unwrap();
    assert_eq!(preview.display_override(), Some([[0, 0, 0, 255]; 256]));
    assert_ne!(halfway, [[0, 0, 0, 255]; 256]);
    preview.tick(1716).unwrap();
    preview.tick(2416).unwrap();
    assert_eq!(preview.display_override(), None);
}

#[test]
fn spec014_help_screen_swaps_the_palette_until_any_input() {
    let dir = corpus();
    let mut help_palette = vec![0u8; 768 + 32768];
    help_palette[..3].copy_from_slice(&[63, 0, 0]);
    std::fs::write(dir.path().join("art/intrface/helpscrn.pal"), &help_palette).unwrap();
    let mut preview = world(dir.path());
    preview.tick(16).unwrap();
    assert_eq!(preview.display_override(), None);
    assert!(preview.step(KeyCode::F1));
    assert!(matches!(preview.screen, Screen::Help(_)));
    let shown = preview.display_override().expect("help palette");
    assert_eq!(shown[0], [252, 0, 0, 255]);
    assert_eq!(shown[1], [0, 0, 0, 255]);
    preview.image().unwrap();
    // Any key returns to the map under the game palette, without a fade.
    assert!(preview.step(KeyCode::KeyQ));
    assert!(matches!(preview.screen, Screen::Map));
    assert_eq!(preview.display_override(), None);
    // A click also returns.
    assert!(preview.step(KeyCode::F1));
    assert!(preview.click([320, 240], false));
    assert!(matches!(preview.screen, Screen::Map));
}

#[test]
fn spec014_automap_window_detail_scanner_and_motion_sensor() {
    let dir = corpus();
    let mut preview = world(dir.path());
    preview.tick(16).unwrap();
    let wall = preview
        .host
        .world
        .iter()
        .find(|(_, object)| object.category() == Category::Wall && object.raw.hex.is_some())
        .map(|(reference, _)| reference)
        .expect("a wall in the synthetic map");
    let object = preview.host.world.get_mut(wall).unwrap();
    object.raw.flags.0 |= vault_formats::pro::BasicFlags::SEEN.0;
    let tile = object.raw.hex.unwrap();
    // Tab opens the window over the paused world with the scanner off; a
    // seen wall paints (449 - 2c, 8 + 2r) and the pixel to its right, from
    // the window at (75, 0).
    assert!(preview.step(KeyCode::Tab));
    assert!(matches!(&preview.screen, Screen::Automap(screen) if !screen.scanner));
    let image = preview.image().unwrap();
    let green = vault_core::ui::ColorKey::GREEN.resolve(&preview.palette);
    let [x, y] = [75 + 449 - 2 * (tile % 200), 8 + 2 * (tile / 200)];
    for x in [x, x + 1] {
        assert_eq!(image.pixels()[(y * 640 + x) as usize], green);
    }
    // h and l set the saved detail flag; the switch toggles on its press.
    let high = |preview: &MapPreview| preview.host.world.persistence.automap_flags & 0x02 != 0;
    assert!(preview.step(KeyCode::KeyH));
    assert!(high(&preview));
    assert!(preview.step(KeyCode::KeyL));
    assert!(!high(&preview));
    assert!(preview.click([75 + 457 + 10, 340 + 10], false));
    assert!(high(&preview));
    // The scanner without a sensor: the refusal box; Esc closes it.
    assert!(preview.step(KeyCode::KeyS));
    assert!(preview.dialog.is_some());
    preview.image().unwrap();
    assert!(preview.step(KeyCode::Escape));
    assert!(preview.dialog.is_none());
    assert!(matches!(preview.screen, Screen::Automap(_)));
    // Cancel acts on its release.
    let cancel = [75 + 277 + 5, 454 + 5];
    assert!(preview.click(cancel, false));
    assert!(matches!(preview.screen, Screen::Automap(_)));
    assert!(preview.release(Some(cancel), false));
    assert!(matches!(preview.screen, Screen::Map));
    // A Motion Sensor with one charge opens it with the scanner on; empty,
    // it opens nothing.
    let mut bytes = super::fixture::prototype(0, 5);
    super::fixture::word(&mut bytes, 0, vault_core::ui::automap::MOTION_SENSOR as i32);
    super::fixture::word(&mut bytes, 0x39 + 8, 1);
    let sensor = preview
        .host
        .world
        .spawn(
            Arc::new(vault_formats::Prototype::parse(&bytes).unwrap()),
            0,
            0,
        )
        .unwrap();
    let player = preview.host.world.player;
    preview
        .host
        .world
        .add_item(player, sensor, 1, true)
        .unwrap();
    preview.use_motion_sensor(sensor);
    assert!(matches!(&preview.screen, Screen::Automap(screen) if screen.scanner));
    assert_eq!(preview.host.world.charges(sensor), Some(0));
    preview.image().unwrap();
    assert!(preview.step(KeyCode::Escape));
    preview.use_motion_sensor(sensor);
    assert!(matches!(preview.screen, Screen::Map));
}

#[test]
fn spec011_inventory_drag_reloads_a_weapon_through_the_counter() {
    let dir = corpus();
    let mut preview = world(dir.path());
    let player = preview.host.world.player;
    // A 10-round weapon of caliber 1 and two packs of five caliber-1 rounds.
    let mut weapon = super::fixture::prototype(0, 3);
    super::fixture::word(&mut weapon, 0, 0x0000_0010);
    super::fixture::word(&mut weapon, 0x39 + 52, 1);
    super::fixture::word(&mut weapon, 0x39 + 60, 10);
    let mut ammo = super::fixture::prototype(0, 4);
    super::fixture::word(&mut ammo, 0, 0x0000_0011);
    super::fixture::word(&mut ammo, 0x39, 1);
    super::fixture::word(&mut ammo, 0x39 + 4, 5);
    let spawn = |preview: &mut MapPreview, bytes: &[u8], quantity| {
        let prototype = Arc::new(vault_formats::Prototype::parse(bytes).unwrap());
        let item = preview.host.world.spawn(prototype, 0, 0).unwrap();
        preview
            .host
            .world
            .add_item(player, item, quantity, true)
            .unwrap();
        item
    };
    let gun = spawn(&mut preview, &weapon, 1);
    let rounds = spawn(&mut preview, &ammo, 2);
    // The gun starts empty.
    preview.host.world.get_mut(gun).unwrap().raw.state = ObjectState::Other {
        word: 0,
        detail: vault_formats::map::ObjectDetail::Weapon {
            rounds: 0,
            ammo_pid: -1,
        },
    };
    assert_eq!(preview.host.world.loaded(gun).0, 0);
    preview.image().unwrap();
    assert!(preview.step(KeyCode::KeyI));
    // Dropping the stack on the gun's row asks for the count; 2 loads both
    // packs (SPEC-014 §3.34, SPEC-011 §3.15).
    let (from, to) = (list_point(&preview, rounds), list_point(&preview, gun));
    assert!(preview.click(from, false));
    preview.hover(to);
    assert!(preview.pointer_frame().is_some());
    assert!(preview.release(Some(to), false));
    assert!(matches!(&preview.screen, Screen::Inventory(screen) if screen.counter.is_some()));
    preview.image().unwrap();
    assert!(preview.step(KeyCode::Digit2));
    assert!(preview.step(KeyCode::Enter));
    assert!(matches!(&preview.screen, Screen::Inventory(screen) if screen.counter.is_none()));
    assert_eq!(preview.host.world.loaded(gun).0, 10);
    // A drop outside every target changes nothing; the left hand takes the
    // gun; the provisional Done button closes.
    let at = list_point(&preview, gun);
    assert!(preview.click(at, false));
    assert!(preview.release(Some([600, 400]), false));
    assert_eq!(list_point(&preview, gun), at);
    assert!(preview.click(at, false));
    assert!(preview.release(Some([240, 300]), false));
    assert!(
        preview
            .host
            .world
            .get(gun)
            .unwrap()
            .raw
            .flags
            .contains(vault_formats::pro::BasicFlags::LEFT_HAND)
    );
    preview.image().unwrap();
    assert!(preview.click([505, 345], false));
    assert!(matches!(preview.screen, Screen::Map));
}

#[test]
fn spec007_player_punch_upgrades_to_kick_through_the_crosshair() {
    // Fixture D through the interface: a bare-handed player attacks an
    // adjacent critter with the crosshair; the generator is positioned so
    // the kick draw D(1, 4) gives `first`.
    let attack = |kick_art: bool, first: i32| {
        let dir = corpus();
        if kick_art {
            std::fs::copy(
                dir.path().join("art/critters/sampleaa.frm"),
                dir.path().join("art/critters/samplear.frm"),
            )
            .unwrap();
        }
        let mut preview = world(dir.path());
        let player = preview.host.world.player;
        let target = npc(&preview);
        let object = preview.host.world.get(target).unwrap();
        let (hex, elevation) = (object.raw.hex.unwrap(), object.raw.elevation);
        let beside = (0..6)
            .filter_map(|direction| vault_formats::HexGrid::neighbor(hex, direction).ok()?)
            .find(|tile| !preview.host.world.occupancy(elevation).contains_key(tile))
            .unwrap();
        preview
            .host
            .world
            .move_object(player, beside, elevation, false)
            .unwrap();
        assert_eq!(preview.host.world.object_distance(player, target), 1);
        assert!(preview.active_item().is_none(), "bare hands");
        preview.tick(16).unwrap();
        preview.image().unwrap();
        preview.host.world.combat = true;
        preview.host.world.critter_state_mut(player).unwrap().ap = 8;
        let mut stream = vault_core::ParkMiller::seeded(12345).unwrap();
        while {
            let mut probe = stream.clone();
            vault_core::RandomSource::range(&mut probe, 1, 4).unwrap() != first
        } {
            vault_core::RandomSource::range(&mut stream, 1, 100).unwrap();
        }
        let before = stream.clone();
        preview.host.world.rng = stream;
        preview.host.world.effects.clear();
        preview.hud.mode = Mode::Crosshair;
        assert!(preview.tap(on_hex(&preview, hex)));
        assert!(
            preview.host.world.rng != before,
            "the attack drew: {}",
            preview.status.escape_ascii()
        );
        let swing = |animation| preview.host.character_sound_name(player, animation, 0);
        let names: Vec<_> = preview
            .host
            .world
            .effects
            .iter()
            .filter_map(|effect| match effect {
                WorldEffect::SoundEffect { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        (names, swing(17).unwrap(), swing(16).unwrap())
    };
    // D1: draw 1 with kick art: the swing sound is the kick's (17).
    let (names, kick, punch) = attack(true, 1);
    assert_ne!(kick, punch);
    assert!(
        names.contains(&kick) && !names.contains(&punch),
        "{names:?}"
    );
    // D2: no kick art keeps the punch; D3: results 2-4 keep it too.
    let (names, kick, punch) = attack(false, 1);
    assert!(
        names.contains(&punch) && !names.contains(&kick),
        "{names:?}"
    );
    let (names, kick, punch) = attack(true, 3);
    assert!(
        names.contains(&punch) && !names.contains(&kick),
        "{names:?}"
    );
}

#[test]
fn spec014_hp_counter_rolls_only_when_asked_and_blocks_the_world() {
    let dir = corpus();
    let mut preview = world(dir.path());
    let player = preview.host.world.player;
    preview.tick(16).unwrap();
    preview.image().unwrap();
    let hp = preview.host.world.hit_points(player).unwrap();
    // A change without a roll request is drawn at once.
    preview.host.world.set_hit_points(player, hp - 2);
    preview.tick(32).unwrap();
    assert!(!preview.counters_rolling());
    // Combat damage asks for a roll: one unit at a time, every frame
    // followed by trunc(250 / (|P - V| + 1)) ms, while the world and game
    // input wait.
    preview.host.world.set_hit_points(player, hp - 5);
    preview.host.world.emit(WorldEffect::RollHitPoints);
    preview.tick(48).unwrap();
    assert!(preview.counters_rolling());
    let ticks = preview.host.world.clock.ticks;
    assert!(preview.step(KeyCode::KeyI), "keys are swallowed");
    assert!(matches!(preview.screen, Screen::Map));
    let mut now = 48;
    while preview.counters_rolling() {
        now += 16;
        preview.tick(now).unwrap();
        preview.image().unwrap();
        assert!(now < 48 + 2000, "the roll ends");
    }
    assert_eq!(preview.host.world.clock.ticks, ticks, "the world waited");
    // 3 units of 3 frames plus the first, at 62 ms each.
    assert!(now >= 48 + 10 * 62, "{now}");
}

#[test]
fn debug_inspector_watches_variables_traces_scripts_and_toggles() {
    let dir = corpus();
    let mut preview = world(dir.path());
    preview.tick(16).unwrap();
    let plain = preview.image().unwrap();
    preview.set_debug(true);
    assert!(preview.debug_enabled() && preview.host.vm_observer.is_some());
    // A changed map variable reaches the overlay; scripts run observed.
    if preview.host.world.mvars.is_empty() {
        preview.host.world.mvars.push(0);
    }
    preview.host.world.mvars[0] += 7;
    preview.tick(32).unwrap();
    // Talking runs the NPC's script, one observed instruction at a time.
    let player = preview.host.world.player;
    let target = npc(&preview);
    preview
        .host
        .action(player, target, vault_core::ObjectAction::Talk)
        .unwrap();
    preview.tick(48).unwrap();
    let overlay = preview.debug.as_ref().unwrap();
    assert!(overlay.recent_changes().any(|line| line.contains("MVAR")));
    assert!(
        !overlay.trace_lines(4).is_empty(),
        "the talk script ran observed"
    );
    assert!(preview.host.world.dialogue.open, "with the same result");
    let shown = preview.image().unwrap();
    assert_ne!(shown.pixels(), plain.pixels());
    // F9 turns it off and detaches the trace.
    assert!(preview.step(KeyCode::F9));
    assert!(!preview.debug_enabled() && preview.host.vm_observer.is_none());
}
