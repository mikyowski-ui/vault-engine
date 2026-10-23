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
use vault_formats::{LocalDirProvider, Vfs, VfsError, VfsPath, map::ObjectState, pro::Category};
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
    // Bar buttons: inventory (211, 41) and the Skilldex (523, 7) under y
    // 379, sending their keys on the release.
    assert!(preview.tap([220, 379 + 50]));
    assert!(matches!(preview.screen, Screen::Inventory(_)));
    assert!(preview.step(KeyCode::Escape));
    assert!(preview.tap([530, 379 + 15]));
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
    // Row 1 (Lockpick) at (451 + 15, 5 + 45 + 36). The window waits
    // trunc(1000 / 9) ms after the press, discarding input, then closes.
    let now = preview.clock;
    assert!(preview.click([451 + 20, 5 + 45 + 36 + 5], false));
    assert!(preview.step(KeyCode::Escape));
    preview.tick(now + 110).unwrap();
    assert!(matches!(preview.screen, Screen::Skilldex(_)));
    preview.tick(now + 111).unwrap();
    assert!(matches!(preview.screen, Screen::Map));
    assert_eq!(preview.hud.mode, Mode::Skill(9));
    // Row 0 toggles Sneak and leaves the mode alone.
    assert!(preview.step(KeyCode::KeyS));
    assert!(preview.click([451 + 20, 5 + 45 + 5], false));
    preview.tick(now + 222).unwrap();
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
    finish_elevator(&mut preview);
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
    // G-23: knobs and switches act on the release.
    assert!(preview.click([299 + 11, 338 + 12], false));
    assert!(preview.hud.cues.is_empty());
    assert!(preview.release(Some([299 + 11, 338 + 12]), false));
    // The change plays `ib2p1xx1` now and `ib2lu1x1` 70 ms later.
    assert_eq!(preview.hud.cues.len(), 1);
    // The game-difficulty label 1 (the current value) still turns the knob
    // downward, so the next center click steps to 0.
    assert!(preview.tap([76 + 20, 71 - 2]));
    assert!(preview.tap([76 + 23, 71 + 21]));
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
    assert!(matches!(&preview.screen, Screen::Inventory(_)) && preview.hud.timer.is_some());
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
    assert!(matches!(&preview.screen, Screen::Inventory(_)) && preview.hud.timer.is_none());
}

/// G-18 / SPEC-013 §3.21: a chosen level other than the start runs the
/// travel animation first: the gauge steps every 23, 46 or 69 ms and a
/// 200 ms pause follows; keys are swallowed meanwhile. Then the player
/// moves.
fn finish_elevator(preview: &mut MapPreview) {
    let Screen::Elevator(screen) = &preview.screen else {
        panic!("the elevator closed before its travel animation");
    };
    assert!(screen.travel.is_some());
    let mut now = preview.clock;
    assert!(preview.step(KeyCode::Escape));
    assert!(matches!(preview.screen, Screen::Elevator(_)));
    for frame in 0..1000 {
        now += 16;
        preview.tick(now).unwrap();
        if matches!(preview.screen, Screen::Map) {
            // Three buttons from level 2 to 0: 72 steps of 1/6 cell, 46 ms
            // each, then the pause.
            assert!(now >= 200, "{now}");
            return;
        }
        if frame % 16 == 0 {
            preview.image().unwrap();
        }
    }
    panic!("the elevator travel did not end");
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
fn spec014_arrow_hover_outlines_an_item_until_the_pointer_moves() {
    let dir = corpus();
    let mut preview = world(dir.path());
    let player = preview.host.world.player;
    let (player_hex, elevation) = preview
        .host
        .world
        .get(player)
        .map(|object| (object.raw.hex.unwrap(), object.raw.elevation))
        .unwrap();
    let hex = vault_formats::HexGrid::step(player_hex, 2, 3).unwrap();
    let prototype =
        Arc::new(vault_formats::Prototype::parse(&super::fixture::prototype(0, 5)).unwrap());
    let item = preview.host.world.spawn(prototype, hex, elevation).unwrap();
    preview.tick(16).unwrap();
    preview.image().unwrap();
    let point = on_hex(&preview, hex);
    // S-22 / SPEC-014 §3.36: roofs stay drawn. The player stands inside
    // the synthetic roofed area, whose roof is hidden from the entrance
    // placement on, so the item under it can be picked.
    assert!(preview.view.roofs);
    assert_eq!(preview.object_at(point), Some(item));
    let color = vault_core::ui::cursor::ITEM_OUTLINE.resolve(&preview.palette);
    // The pixel just left of the item's 12-by-20 sprite, halfway down.
    let (_, origin) = vault_render::map::object_sprite(
        preview.view,
        &preview.art,
        &preview.host.world.get(item).unwrap().raw,
    )
    .unwrap();
    let ring = (origin[1] as usize + 10) * 640 + origin[0] as usize - 1;
    // A move, then a 300 ms rest (a mode change restarts the rest on the
    // next poll, so the hover runs on the one after).
    let rest = |preview: &mut MapPreview, at: [i32; 2], now: &mut u64| {
        preview.hover(at);
        *now += 16;
        preview.tick(*now).unwrap();
        *now += 300;
        preview.tick(*now).unwrap();
    };
    let mut now = 16;
    preview.hud.mode = Mode::Arrow;
    rest(&mut preview, point, &mut now);
    assert_eq!(preview.pointer.outlined, Some(item));
    preview.image().unwrap();
    let plain = preview.frame.as_ref().unwrap().pixels()[ring];
    assert_ne!(plain, color);
    assert_eq!(preview.pointer_frame().unwrap().pixels()[ring], color);
    // A one-pixel move removes the outline at once (§3.27).
    preview.hover([point[0] + 1, point[1]]);
    assert_eq!(preview.pointer.outlined, None);
    assert_eq!(preview.pointer_frame().unwrap().pixels()[ring], plain);
    // Item highlight off, or the no-highlight flag, skips the outline.
    preview.preferences.item_highlight = 0;
    rest(&mut preview, point, &mut now);
    assert!(preview.pointer.feedback.is_some());
    assert_eq!(preview.pointer.outlined, None);
    preview.preferences.item_highlight = 1;
    preview.host.world.get_mut(item).unwrap().raw.flags.0 |=
        vault_formats::pro::BasicFlags::NO_HIGHLIGHT.0;
    rest(&mut preview, [point[0] + 1, point[1]], &mut now);
    assert_eq!(preview.pointer.outlined, None);
}

#[test]
fn spec014_movement_hover_plans_only_when_its_inputs_change() {
    let dir = corpus();
    let mut preview = world(dir.path());
    preview.tick(16).unwrap();
    preview.image().unwrap();
    let player = preview.host.world.player;
    let own = preview.host.world.get(player).unwrap().raw.hex.unwrap();
    let center = |preview: &MapPreview, hex: i32| {
        let [x, y] = preview.view.grid.hex_center(hex).unwrap();
        [x as i32, y as i32]
    };
    let mut now = 16;
    let mut rest = |preview: &mut MapPreview, at: [i32; 2]| {
        preview.hover(at);
        now += 300;
        preview.tick(now).unwrap();
    };
    let blocked = Some(super::pointer::Feedback::Cost(
        b"X".to_vec(),
        vault_core::ui::ColorKey::RED,
    ));
    // The player's own hex has no path: a red X, planned once.
    let at = center(&preview, own);
    rest(&mut preview, at);
    assert_eq!(preview.pointer.feedback, blocked);
    assert_eq!(preview.pointer.routes_planned, 1);
    // Resting again on another pixel of the same hex reuses it.
    rest(&mut preview, [at[0] + 1, at[1]]);
    assert_eq!(preview.pointer.feedback, blocked);
    assert_eq!(preview.pointer.routes_planned, 1);
    // Another hex plans again, and so does a return to the first.
    let other = vault_formats::HexGrid::step(own, 2, 3).unwrap();
    let there = center(&preview, other);
    rest(&mut preview, there);
    assert_eq!(preview.pointer.routes_planned, 2);
    rest(&mut preview, at);
    assert_eq!(preview.pointer.routes_planned, 3);
    // A blocker moving on the elevation plans again.
    let blocker = npc(&preview);
    let elevation = preview.host.world.get(blocker).unwrap().raw.elevation;
    let spot = vault_formats::HexGrid::step(own, 4, 2).unwrap();
    preview
        .host
        .world
        .move_object(blocker, spot, elevation, false)
        .unwrap();
    rest(&mut preview, [at[0] + 1, at[1]]);
    assert_eq!(preview.pointer.routes_planned, 4);
    assert_eq!(preview.pointer.feedback, blocked);
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
fn spec014_scrolling_stops_at_the_map_border_and_home_walks_back() {
    let dir = corpus();
    let mut preview = world(dir.path());
    preview.tick(16).unwrap();
    // The player near the top of the border: row 50, column 100.
    let player = preview.host.world.player;
    let elevation = preview.host.world.get(player).unwrap().raw.elevation;
    let near = 50 * 200 + 100;
    preview
        .host
        .world
        .move_object(player, near, elevation, false)
        .unwrap();
    assert_eq!(preview.host.world.camera_hex, near);
    // Arrow-key steps north stop on the border's first row, 45.
    let mut now = 16;
    let mut blocked = false;
    for _ in 0..20 {
        now += 40;
        preview.tick(now).unwrap();
        if preview.scroll_step(1) == Some(true) {
            blocked = true;
            break;
        }
    }
    assert!(blocked);
    let top = preview.host.world.camera_hex;
    assert!(vault_core::ui::cursor::inside_border(top));
    assert!((45..=46).contains(&(top / 200)), "row {}", top / 200);
    // Home walks hex by hex back to the player.
    assert!(preview.step(KeyCode::Home));
    assert_eq!(preview.host.world.camera_hex, near);
    // A player outside the border: the walk stops on it, and moving the
    // player there leaves the view where it was.
    let outside = 30 * 200 + 100;
    preview
        .host
        .world
        .move_object(player, outside, elevation, false)
        .unwrap();
    assert_eq!(preview.host.world.camera_hex, near);
    assert!(preview.step(KeyCode::Home));
    let stopped = preview.host.world.camera_hex;
    assert_ne!(stopped, near);
    assert!(vault_core::ui::cursor::inside_border(stopped));
    assert_eq!(stopped / 200, 45);
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
    // Done is the red button at (517, 329).
    assert!(preview.click([520, 333], false));
    assert!(matches!(preview.screen, Screen::Map));
}

#[test]
fn spec014_inventory_figure_turns_and_nested_containers_open() {
    use vault_formats::pro::BasicFlags;
    let dir = corpus();
    let mut preview = world(dir.path());
    let player = preview.host.world.player;
    let spawn = |preview: &mut MapPreview, bytes: &[u8], owner, quantity| {
        let prototype = Arc::new(vault_formats::Prototype::parse(bytes).unwrap());
        let item = preview.host.world.spawn(prototype, 0, 0).unwrap();
        preview
            .host
            .world
            .add_item(owner, item, quantity, true)
            .unwrap();
        item
    };
    // A bag of size 100 holding a rock and a pouch with a pebble.
    let container = |pid| {
        let mut bytes = super::fixture::prototype(0, 1);
        super::fixture::word(&mut bytes, 0, pid);
        super::fixture::word(&mut bytes, 0x39, 100);
        bytes
    };
    let misc = |pid| {
        let mut bytes = super::fixture::prototype(0, 5);
        super::fixture::word(&mut bytes, 0, pid);
        bytes
    };
    let bag = spawn(&mut preview, &container(0x20), player, 1);
    let rock = spawn(&mut preview, &misc(0x21), bag, 1);
    let pouch = spawn(&mut preview, &container(0x22), bag, 1);
    let pebble = spawn(&mut preview, &misc(0x23), pouch, 1);
    let owner = |preview: &MapPreview, item| preview.host.world.get(item).unwrap().owner;
    let screen = |preview: &MapPreview| match &preview.screen {
        Screen::Inventory(screen) => (preview.figure.0, screen.path.clone()),
        _ => panic!("the inventory is open"),
    };
    preview.image().unwrap();
    assert!(preview.step(KeyCode::KeyI));
    // The figure turns one facing at most every 166 ms (SPEC-014 §3.34).
    let mut now = preview.clock;
    let start = screen(&preview).0;
    now += 166;
    preview.tick(now).unwrap();
    assert_eq!(screen(&preview).0, (start + 1) % 6);
    now += 100;
    preview.tick(now).unwrap();
    assert_eq!(screen(&preview).0, (start + 1) % 6);
    // Use on the bag (arrow cursor, second menu entry) opens it; the list
    // shows its contents.
    let at = list_point(&preview, bag);
    assert!(preview.click(at, true));
    menu_second_entry(&mut preview, at, &mut now);
    assert_eq!(screen(&preview).1, [bag]);
    preview.image().unwrap();
    // And the pouch inside it, a level deeper.
    let row = |row: i32| [134, 45 + 48 * row];
    menu_second_entry(&mut preview, row(1), &mut now);
    assert_eq!(screen(&preview).1, [bag, pouch]);
    assert!(preview.click(at, true));
    // Dropping the pebble on the figure moves it up into the bag; the
    // figure keeps turning during the drag.
    let figure = [280, 80];
    assert!(preview.click(row(0), false));
    let turning = screen(&preview).0;
    now += 166;
    preview.tick(now).unwrap();
    assert_eq!(screen(&preview).0, (turning + 1) % 6);
    assert!(preview.release(Some(figure), false));
    assert_eq!(owner(&preview, pebble), Some(bag));
    // A press on the figure goes up a level, to the bag (the release
    // does nothing more).
    assert!(preview.click(figure, false));
    assert!(!preview.release(Some(figure), false));
    assert_eq!(screen(&preview).1, [bag]);
    // The rock goes straight from the bag to the right hand.
    assert!(preview.click(row(0), false));
    assert!(preview.release(Some([330, 300]), false));
    assert_eq!(owner(&preview, rock), Some(player));
    assert!(
        preview
            .host
            .world
            .get(rock)
            .unwrap()
            .raw
            .flags
            .contains(BasicFlags::RIGHT_HAND)
    );
    // From the hand, dropped on the list, it goes back into the open bag.
    assert!(preview.click([330, 300], false));
    assert!(preview.release(Some(row(2)), false));
    assert_eq!(owner(&preview, rock), Some(bag));
    assert_eq!(
        preview.host.world.get(rock).unwrap().raw.flags.0 & vault_core::items::EQUIPPED,
        0
    );
    // Up again to the player's own list; the rock moves up with it.
    let index = |preview: &MapPreview, item| {
        let world = &preview.host.world;
        world
            .get(world.get(item).unwrap().owner.unwrap())
            .unwrap()
            .inventory
            .iter()
            .position(|(entry, _)| *entry == item)
            .unwrap() as i32
    };
    let rock_row = row(index(&preview, rock));
    assert!(preview.click(rock_row, false));
    assert!(preview.release(Some(figure), false));
    assert_eq!(owner(&preview, rock), Some(player));
    assert!(preview.click(figure, false));
    assert!(screen(&preview).1.is_empty());
}

#[test]
fn spec013_skill_books_read_from_the_inventory_and_the_item_slot() {
    use vault_formats::pro::BasicFlags;
    let dir = corpus();
    let mut preview = world(dir.path());
    let player = preview.host.world.player;
    let carry = |preview: &mut MapPreview, pid: i32| {
        let mut bytes = super::fixture::prototype(0, 5);
        super::fixture::word(&mut bytes, 0, pid);
        let prototype = Arc::new(vault_formats::Prototype::parse(&bytes).unwrap());
        let item = preview.host.world.spawn(prototype, 0, 0).unwrap();
        preview.host.world.add_item(player, item, 1, true).unwrap();
        item
    };
    let skill = |preview: &MapPreview, index: usize| {
        let world = &preview.host.world;
        world
            .player_sheet()
            .unwrap()
            .skill(index, world.context(player))
    };
    // The inventory's Use (Look, Use, Drop, Cancel) reads the book.
    let book = carry(&mut preview, 73);
    let (science, clock) = (skill(&preview, 12), preview.host.world.clock.ticks);
    preview.image().unwrap();
    assert!(preview.step(KeyCode::KeyI));
    let at = list_point(&preview, book);
    assert!(preview.click(at, true));
    let mut now = preview.clock;
    menu_second_entry(&mut preview, at, &mut now);
    assert!(preview.host.world.get(book).is_none());
    assert!(skill(&preview, 12) > science);
    assert!(preview.host.world.clock.ticks > clock);
    // SPEC-014 §3.2: the book's fades block input until they end.
    preview.finish_fades();
    assert!(preview.step(KeyCode::Escape));
    // From the item slot in combat: 2 AP are spent and the book is kept.
    let book = carry(&mut preview, 76);
    preview
        .host
        .world
        .equip_slot(player, book, BasicFlags::LEFT_HAND.0)
        .unwrap();
    preview.host.world.active_hand = 0;
    preview.host.world.combat = true;
    // SPEC-014 §3.27: the player's turn, or the combat lock discards input.
    preview.host.world.combat_state.player_turn = true;
    preview.host.world.critter_state_mut(player).unwrap().ap = 5;
    let repair = skill(&preview, 13);
    preview.tap([360, 440]);
    assert_eq!(preview.host.world.critter_state(player).unwrap().ap, 3);
    assert!(preview.host.world.get(book).is_some());
    assert_eq!(skill(&preview, 13), repair);
    // Out of combat the slot reads it at once.
    preview.host.world.combat = false;
    preview.tap([360, 440]);
    assert!(preview.host.world.get(book).is_none());
    assert!(skill(&preview, 13) > repair);
}

#[test]
fn spec014_inventory_and_use_on_window_layout() {
    use super::inventory::Slot;
    let dir = corpus();
    let mut preview = world(dir.path());
    let player = preview.host.world.player;
    for pid in 0x40..0x4e {
        let mut bytes = super::fixture::prototype(0, 5);
        super::fixture::word(&mut bytes, 0, pid);
        let prototype = Arc::new(vault_formats::Prototype::parse(&bytes).unwrap());
        let item = preview.host.world.spawn(prototype, 0, 0).unwrap();
        preview.host.world.add_item(player, item, 1, true).unwrap();
    }
    let count = preview.list_entries_len();
    assert!(count >= 14);
    let offset = |preview: &MapPreview| match &preview.screen {
        Screen::Inventory(screen) => screen.offset,
        _ => panic!("the inventory is open"),
    };
    preview.image().unwrap();
    assert!(preview.step(KeyCode::KeyI));
    // The scroll buttons at (208, 39) and (208, 62) move one row each.
    assert!(preview.click([215, 70], false));
    assert_eq!(offset(&preview), 1);
    assert!(preview.click([215, 45], false));
    assert!(preview.click([215, 45], false));
    assert_eq!(offset(&preview), 0, "up needs a positive offset");
    // The stats panel's empty-hand block: 1 to melee damage + 2.
    let melee = preview.host.world.player_stat(11);
    let painter = crate::interface::Painter {
        assets: &preview.host.assets,
        palette: &preview.palette,
    };
    let lines = preview.hand_lines(&painter, Slot::LeftHand);
    assert_eq!(lines.len(), 2);
    assert!(lines[1].0.ends_with(format!(" 1-{}", melee + 2).as_bytes()));
    preview.image().unwrap();
    // A held arrow press opens the menu where the pointer is at the pulse:
    // flipped against the window's right edge (x + 68 reaching 579). The
    // release returns the pointer to where the menu opened.
    let mut now = preview.clock;
    assert!(preview.click([134, 45], true));
    assert!(preview.click([134, 45], false));
    preview.hover([560, 45]);
    now += 300;
    preview.tick(now).unwrap();
    let menu = match &preview.screen {
        Screen::Inventory(screen) => screen.menu.clone().unwrap(),
        _ => panic!("the inventory is open"),
    };
    assert!(menu.flipped);
    assert_eq!((menu.origin, menu.top), ([560, 45], 45));
    preview.image().unwrap();
    // A move under 10 pixels keeps Look highlighted.
    preview.hover([560, 52]);
    preview.release(Some([560, 52]), false);
    assert_eq!(preview.pointer.at, Some([560, 45]));
    assert_eq!(preview.list_entries_len(), count);
    // The facing counter persists between openings.
    now += 400;
    preview.tick(now).unwrap();
    let facing = preview.figure.0;
    assert!(preview.step(KeyCode::Escape));
    assert!(preview.step(KeyCode::KeyI));
    assert_eq!(preview.figure.0, facing);
    assert!(preview.step(KeyCode::Escape));
    // The use-on window: Home, End, PgUp and PgDn, and Cancel at (313, 328).
    let target = npc(&preview);
    preview.open_inventory_on(Some(target));
    preview.image().unwrap();
    assert!(preview.step(KeyCode::PageDown));
    assert_eq!(offset(&preview), 6);
    assert!(preview.step(KeyCode::PageDown));
    assert_eq!(offset(&preview), count - 6, "clamped to the last rows");
    assert!(preview.step(KeyCode::PageUp));
    assert_eq!(offset(&preview), count - 12);
    assert!(preview.step(KeyCode::Home));
    assert_eq!(offset(&preview), 0);
    assert!(preview.step(KeyCode::End));
    assert_eq!(offset(&preview), count - 6);
    assert!(preview.click([316, 332], false));
    assert!(matches!(preview.screen, Screen::Map));
}

#[test]
fn spec014_reloading_a_held_weapon_keeps_it_in_its_hand() {
    use vault_formats::pro::BasicFlags;
    let dir = corpus();
    let mut preview = world(dir.path());
    let player = preview.host.world.player;
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
    preview.host.world.get_mut(gun).unwrap().raw.state = ObjectState::Other {
        word: 0,
        detail: vault_formats::map::ObjectDetail::Weapon {
            rounds: 0,
            ammo_pid: -1,
        },
    };
    preview
        .host
        .world
        .equip_slot(player, gun, BasicFlags::LEFT_HAND.0)
        .unwrap();
    preview.image().unwrap();
    assert!(preview.step(KeyCode::KeyI));
    // The stack dropped on the left hand loads both packs; the gun stays
    // in the hand (SPEC-014 §3.34 target 2).
    let from = list_point(&preview, rounds);
    assert!(preview.click(from, false));
    assert!(preview.release(Some([240, 300]), false));
    assert!(preview.step(KeyCode::Digit2));
    assert!(preview.step(KeyCode::Enter));
    assert_eq!(preview.host.world.loaded(gun).0, 10);
    let object = preview.host.world.get(gun).unwrap();
    assert!(object.raw.flags.contains(BasicFlags::LEFT_HAND));
    assert_eq!(object.owner, Some(player));
}

#[test]
fn spec007_player_punch_upgrades_to_kick_through_the_crosshair() {
    // Fixture D through the interface: a bare-handed player attacks an
    // adjacent critter with the crosshair; the generator is positioned so
    // the kick draw D(1, 4) gives `first`.
    let attack = |kick_art: bool, first: i32| {
        let dir = corpus();
        // SPEC-007 §3.10.1: an attack whose sequence art is missing does
        // not register, so the punch and every reaction get art.
        let suffixes = ["aq", "an", "ao", "ap", "ba", "bb", "bo", "bp", "ch", "cj"];
        for suffix in suffixes.iter().chain(kick_art.then_some(&"ar")) {
            std::fs::copy(
                dir.path().join("art/critters/sampleaa.frm"),
                dir.path().join(format!("art/critters/sample{suffix}.frm")),
            )
            .unwrap();
        }
        let mut preview = world(dir.path());
        // The show plays at once, so its sounds follow the tap.
        preview.host.playback = vault_core::host_choreography::Playback::Instant;
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
        // SPEC-014 §3.27: the player's turn, or the combat lock discards input.
        preview.host.world.combat_state.player_turn = true;
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

/// The corpus with every read counted by path.
struct CountingVfs {
    inner: LocalDirProvider,
    reads: std::sync::Mutex<std::collections::BTreeMap<String, usize>>,
}
impl CountingVfs {
    fn count(&self, path: &str) {
        *self.reads.lock().unwrap().entry(path.into()).or_default() += 1;
    }
    fn take(&self) -> std::collections::BTreeMap<String, usize> {
        std::mem::take(&mut *self.reads.lock().unwrap())
    }
}
impl Vfs for CountingVfs {
    fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        self.count(path);
        self.inner.read(path)
    }
    fn read_bounded(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>, VfsError> {
        self.count(path);
        self.inner.read_bounded(path, max_bytes)
    }
    fn files(&self) -> Box<dyn Iterator<Item = &VfsPath> + '_> {
        self.inner.files()
    }
}

#[test]
fn decoded_assets_are_read_once_across_frames_hovers_and_windows() {
    let dir = corpus();
    // Scenery art index 1 names a file that does not exist.
    std::fs::write(
        dir.path().join("art/scenery/scenery.lst"),
        b"sample.frm\nghost.frm\n",
    )
    .unwrap();
    let counting = Arc::new(CountingVfs {
        inner: LocalDirProvider::mount(dir.path()).unwrap(),
        reads: Default::default(),
    });
    let vfs: Arc<dyn Vfs> = counting.clone();
    let mut preview = MapPreview::load(
        vfs,
        "maps/synth.map",
        "english",
        Some("premade/synthetic.gcd"),
        1,
    )
    .unwrap();
    preview.save_root = dir.path().join("saves");
    let ghost = npc(&preview);
    preview.host.world.get_mut(ghost).unwrap().raw.art_id =
        vault_formats::Fid::new(2, 1, 0, 0, 0).0;
    let player = preview.host.world.player;
    let own = preview.host.world.get(player).unwrap().raw.hex.unwrap();
    let points = [
        on_hex(&preview, own),
        on_hex(&preview, vault_formats::HexGrid::step(own, 2, 3).unwrap()),
        [320, 440],
    ];
    let mut now = 0;
    let mut cycle = |preview: &mut MapPreview| {
        for mode in [Mode::Movement, Mode::Arrow, Mode::Crosshair] {
            preview.hud.mode = mode;
            for point in points {
                preview.hover(point);
                for _ in 0..3 {
                    now += 150;
                    preview.tick(now).unwrap();
                    preview.image().unwrap();
                    preview.pointer_frame();
                }
            }
        }
        assert!(preview.step(KeyCode::KeyI));
        preview.image().unwrap();
        assert!(preview.step(KeyCode::Escape));
        preview.image().unwrap();
    };
    cycle(&mut preview);
    let first = counting.take();
    assert_eq!(first.get("art/scenery/ghost.frm"), Some(&1), "{first:?}");
    // Every decoded resource is cached, failures included: the same frames,
    // hovers and windows again read nothing.
    cycle(&mut preview);
    let again = counting.take();
    assert!(again.is_empty(), "{again:?}");
}

/// Solid one-frame interface art for each `(name, width, height, color)`,
/// so a frame pixel tells which image was drawn.
fn write_art(dir: &std::path::Path, art: &[(&str, u16, u16, u8)]) {
    for (name, width, height, color) in art {
        std::fs::write(
            dir.join("art/intrface").join(name),
            super::fixture::frm(*width, *height, *color),
        )
        .unwrap();
    }
}
/// The last composed frame's pixel (without the cursor).
fn frame_pixel(preview: &MapPreview, [x, y]: [i32; 2]) -> u8 {
    preview.frame.as_ref().unwrap().pixels()[(y * 640 + x) as usize]
}
/// The newest monitor line, without its bullet.
fn monitor_line(preview: &MapPreview) -> Vec<u8> {
    preview
        .hud
        .monitor
        .visible()
        .last()
        .map(|line| line[1..].to_vec())
        .unwrap_or_default()
}
/// A marker on the monitor, so the next line is known to be new.
fn mark_monitor(preview: &mut MapPreview) {
    preview
        .host
        .world
        .effects
        .push_back(WorldEffect::Message(b"mark".to_vec()));
    preview.image().unwrap();
    assert_eq!(monitor_line(preview), b"mark");
}
/// An item named `Widget`, with an examine text, at the end of `owner`'s
/// inventory.
fn widget(preview: &mut MapPreview, owner: vault_core::ObjectRef) -> vault_core::ObjectRef {
    let mut bytes = super::fixture::prototype(0, 5);
    super::fixture::word(&mut bytes, 0, 0x40);
    let prototype = Arc::new(vault_formats::Prototype::parse(&bytes).unwrap());
    let item = preview.host.world.spawn(prototype, 0, 0).unwrap();
    let object = preview.host.world.get_mut(item).unwrap();
    object.name = b"Widget".to_vec();
    object.description = b"A synthetic widget.".to_vec();
    preview.host.world.add_item(owner, item, 1, true).unwrap();
    item
}
fn contains(text: &[u8], part: &[u8]) -> bool {
    text.windows(part.len()).any(|window| window == part)
}

#[test]
fn spec014_bar_buttons_draw_down_while_held_and_send_on_release() {
    use vault_core::ui::Rect;
    let dir = corpus();
    write_art(
        dir.path(),
        &[
            ("invbutup.frm", 32, 21, 1),
            ("invbutdn.frm", 32, 21, 2),
            ("sattkbup.frm", 188, 67, 3),
            ("sattkbdn.frm", 188, 67, 4),
        ],
    );
    let mut preview = world(dir.path());
    // The Inventory button is screen (211, 420) 32 by 21; the item slot
    // (267, 405) 188 by 67. Their bottom-right pixels show the image.
    let (inventory, slot) = ([242, 440], [454, 471]);
    preview.image().unwrap();
    assert_eq!(frame_pixel(&preview, inventory), 1);
    assert_eq!(frame_pixel(&preview, slot), 3);
    // The press draws the down image and sends nothing yet.
    let button = [220, 429];
    preview.hover(button);
    assert!(preview.click(button, false));
    assert!(matches!(preview.screen, Screen::Map));
    preview.image().unwrap();
    assert_eq!(frame_pixel(&preview, inventory), 2);
    assert_eq!(preview.held_button(), Some(Rect::new(211, 420, 32, 21)));
    // Off the button it is up again, back on it down; each crossing asks
    // for a new frame.
    preview.hover([100, 429]);
    assert!(preview.held_moved());
    preview.image().unwrap();
    assert_eq!(frame_pixel(&preview, inventory), 1);
    assert!(!preview.held_moved());
    preview.hover(button);
    assert!(preview.held_moved());
    preview.image().unwrap();
    assert_eq!(frame_pixel(&preview, inventory), 2);
    // The release over it sends `i`.
    assert!(preview.release(Some(button), false));
    assert!(matches!(preview.screen, Screen::Inventory(_)));
    assert_eq!(preview.held_button(), None);
    assert!(preview.step(KeyCode::Escape));
    // A release off the button sends nothing and draws it up.
    assert!(preview.click(button, false));
    preview.hover([100, 429]);
    assert!(preview.release(Some([100, 429]), false));
    assert!(matches!(preview.screen, Screen::Map));
    preview.image().unwrap();
    assert_eq!(frame_pixel(&preview, inventory), 1);
    // §3.23 the item slot: `sattkbdn.frm` while held; the release uses the
    // (empty-handed) slot, setting the crosshair.
    let at = [361, 438];
    preview.hover(at);
    assert!(preview.click(at, false));
    preview.image().unwrap();
    assert_eq!(frame_pixel(&preview, slot), 4);
    assert_eq!(preview.hud.mode, Mode::Movement);
    assert!(preview.release(Some(at), false));
    assert_eq!(preview.hud.mode, Mode::Crosshair);
    preview.image().unwrap();
    assert_eq!(frame_pixel(&preview, slot), 3);
}

#[test]
fn spec013_elevator_buttons_draw_down_and_report_on_release() {
    let dir = corpus();
    write_art(
        dir.path(),
        &[("ebut_out.frm", 48, 50, 1), ("ebut_in.frm", 48, 50, 2)],
    );
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
    assert!(matches!(preview.screen, Screen::Elevator(_)));
    // Button 0 at (13, 40) in the window, 48 by 50.
    let size = crate::interface::Painter {
        assets: &preview.host.assets,
        palette: &preview.palette,
    }
    .size("el_vault.frm")
    .unwrap_or([230, 284]);
    let [ox, oy] = vault_core::skills::elevator_origin(size);
    let [bx, by] = vault_core::skills::elevator_button(0);
    let (button, corner) = ([ox + bx + 20, oy + by + 20], [ox + bx + 47, oy + by + 49]);
    assert_eq!(frame_pixel(&preview, corner), 1);
    // The press plays its sound and draws `ebut_in.frm`; the level waits
    // for the release.
    preview.hover(button);
    assert!(preview.click(button, false));
    preview.image().unwrap();
    assert_eq!(frame_pixel(&preview, corner), 2);
    assert!(matches!(preview.screen, Screen::Elevator(_)));
    // Released off the button: nothing, and the button is up.
    preview.hover([10, 10]);
    assert!(preview.release(Some([10, 10]), false));
    assert!(matches!(preview.screen, Screen::Elevator(_)));
    preview.image().unwrap();
    assert_eq!(frame_pixel(&preview, corner), 1);
    // Released over it: level 1 (vault 6/0/14104).
    preview.hover(button);
    assert!(preview.tap(button));
    finish_elevator(&mut preview);
    let object = preview.host.world.get(player).unwrap();
    assert_eq!((object.raw.hex, object.raw.elevation), (Some(14104), 0));
}

#[test]
fn spec014_main_menu_buttons_draw_down_and_select_on_release() {
    let dir = corpus();
    write_art(
        dir.path(),
        &[("menuup.frm", 26, 26, 1), ("menudown.frm", 26, 26, 2)],
    );
    let vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(dir.path()).unwrap());
    let mut game = Game::new(vfs, "english", 1, dir.path().join("saves"), None).unwrap();
    assert_eq!(game.title(), "Vault — main menu");
    // Button 1 (New Game) at (425, 86), 26 by 26.
    let pixel = |game: &mut Game| game.image().unwrap().pixels()[(86 + 25) * 640 + 425 + 25];
    assert_eq!(pixel(&mut game), 1);
    let button = [430, 90];
    game.hover(button);
    assert!(game.click(button, false));
    assert_eq!(game.title(), "Vault — main menu");
    assert_eq!(pixel(&mut game), 2);
    // Off the button: a new frame with the up image; a release there
    // selects nothing.
    game.hover([300, 300]);
    assert!(game.tick(16).unwrap());
    assert_eq!(pixel(&mut game), 1);
    assert!(game.release(Some([300, 300]), false));
    assert_eq!(game.title(), "Vault — main menu");
    // A release over the button selects it.
    game.hover(button);
    assert!(game.click(button, false));
    assert!(game.release(Some(button), false));
    assert!(game.title().contains("character selection"));
}

#[test]
fn spec013_counter_all_button_is_allbon_up_and_allboff_down() {
    use vault_core::ui::Rect;
    let dir = corpus();
    write_art(
        dir.path(),
        &[("allbon.frm", 94, 33, 1), ("allboff.frm", 94, 33, 2)],
    );
    let preview = world(dir.path());
    let painter = crate::interface::Painter {
        assets: &preview.host.assets,
        palette: &preview.palette,
    };
    let counter = vault_core::skills::Counter::new(5);
    let [cx, cy] = vault_core::skills::COUNTER_ORIGIN;
    let all = Rect::new(cx + 120, cy + 80, 94, 33);
    let corner = ((all.y + 32) * 640 + all.x + 93) as usize;
    for (held, color) in [(None, 1), (Some(all), 2)] {
        let mut image = vault_render::IndexedImage::new(640, 480).unwrap();
        super::counter::draw_counter(&painter, &mut image, &counter, None, held);
        assert_eq!(image.pixels()[corner], color);
    }
}

#[test]
fn spec013_loot_arrow_cursor_looks_and_runs_the_item_menu() {
    use vault_core::{skills::ItemEntry, ui::inventory as layout};
    let dir = corpus();
    let mut preview = world(dir.path());
    let player = preview.host.world.player;
    let npc = npc(&preview);
    if let Some(ObjectState::Critter(state)) =
        preview.host.world.get_mut(npc).map(|o| &mut o.raw.state)
    {
        state.results.0 |= effect::DEAD;
    }
    widget(&mut preview, npc);
    let own = widget(&mut preview, player);
    preview.host.world.pending_loot = Some(LootRequest {
        looter: player,
        target: npc,
        steal: false,
    });
    preview.image().unwrap();
    let menu = |preview: &MapPreview| match &preview.screen {
        Screen::Loot(screen) => screen.menu.as_ref().map(|(_, menu)| menu.clone()),
        _ => panic!("the loot window is open"),
    };
    // The right press switches to the arrow cursor; hovering the target's
    // entry prints its look line.
    let entry = [80 + 430, 35 + 10];
    assert!(preview.click(entry, true));
    assert!(matches!(&preview.screen, Screen::Loot(screen) if screen.arrow));
    preview.hover(entry);
    assert!(contains(&monitor_line(&preview), b"Widget"));
    // A quick click prints it again.
    mark_monitor(&mut preview);
    assert!(preview.click(entry, false));
    assert!(preview.release(Some(entry), false));
    assert!(contains(&monitor_line(&preview), b"Widget"));
    // A hold past 250 ms opens the target entry's menu (Look, Cancel) at
    // the pointer, flipped against x 617 and shifted up from y 376.
    let mut now = preview.clock;
    assert!(preview.click(entry, false));
    let at = [600, 360];
    preview.hover(at);
    now += 300;
    preview.tick(now).unwrap();
    let opened = menu(&preview).expect("the item menu");
    assert_eq!(opened.entries, [ItemEntry::Look, ItemEntry::Cancel]);
    assert_eq!(
        (opened.top, opened.flipped),
        layout::menu_place(at, 2, layout::LOOT_EDGES)
    );
    assert_eq!((opened.top, opened.flipped), (295, true));
    preview.image().unwrap();
    // The release on Look prints the examine text; the pointer returns to
    // where the menu opened.
    mark_monitor(&mut preview);
    preview.hover([at[0], at[1] + 5]);
    assert!(preview.release(Some([at[0], at[1] + 5]), false));
    assert!(menu(&preview).is_none());
    assert_eq!(preview.pointer.at, Some(at));
    assert_eq!(monitor_line(&preview), b"A synthetic widget.");
    // The player's own entry offers Drop: one move down and the release
    // drops it at the player's position.
    let row = match &preview.screen {
        Screen::Loot(screen) => preview
            .host
            .loot_items(&screen.session, false)
            .iter()
            .position(|(entry, _)| *entry == own)
            .unwrap(),
        _ => unreachable!(),
    };
    assert!(row < vault_core::skills::LOOT_ROWS);
    let [sx, sy] = vault_core::skills::loot_player_slot(row);
    let [ox, oy] = vault_core::skills::LOOT_ORIGIN;
    let at = [ox + sx + 20, oy + sy + 20];
    preview.hover(at);
    assert!(preview.click(at, false));
    now += 300;
    preview.tick(now).unwrap();
    assert_eq!(
        menu(&preview).unwrap().entries,
        [ItemEntry::Look, ItemEntry::Drop, ItemEntry::Cancel]
    );
    preview.hover([at[0], at[1] + 11]);
    assert!(preview.release(Some([at[0], at[1] + 11]), false));
    let dropped = preview.host.world.get(own).unwrap();
    assert_eq!(dropped.owner, None);
    assert_eq!(
        dropped.raw.hex,
        preview.host.world.get(player).unwrap().raw.hex
    );
    assert!(preview.step(KeyCode::Escape));
}

#[test]
fn spec013_barter_arrow_cursor_looks_and_drops_through_the_item_menu() {
    use vault_core::{skills::ItemEntry, ui::inventory as layout};
    let dir = corpus();
    let mut preview = world(dir.path());
    let player = preview.host.world.player;
    // Only the widget in the player's inventory column.
    let carried: Vec<_> = preview.host.world.get(player).unwrap().inventory.clone();
    for (item, quantity) in carried {
        preview
            .host
            .world
            .drop_item(player, item, quantity)
            .unwrap();
    }
    let item = widget(&mut preview, player);
    preview.image().unwrap();
    preview.hud.mode = Mode::Arrow;
    assert!(preview.tap([336, 170]));
    assert!(preview.host.world.dialogue.open);
    preview.host.world.dialogue.pending_barter = true;
    preview.image().unwrap();
    let reply = |preview: &mut MapPreview| match &mut preview.screen {
        Screen::Barter(screen) => screen.reply.take().unwrap_or_default(),
        _ => panic!("the barter window is open"),
    };
    let menu = |preview: &MapPreview| match &preview.screen {
        Screen::Barter(screen) => screen.menu.as_ref().map(|(_, menu)| menu.clone()),
        _ => panic!("the barter window is open"),
    };
    // The arrow cursor: hovering and a quick click show the look line in
    // the reply area (the window covers the monitor).
    let entry = [120, 340];
    assert!(preview.click(entry, true));
    preview.hover(entry);
    assert!(contains(&reply(&mut preview), b"Widget"));
    assert!(preview.click(entry, false));
    assert!(preview.release(Some(entry), false));
    assert!(contains(&reply(&mut preview), b"Widget"));
    // A hold opens Look, Drop, Cancel for the player's own column, shifted
    // up from the window's bottom (470).
    let mut now = preview.clock;
    assert!(preview.click(entry, false));
    let at = [120, 400];
    preview.hover(at);
    now += 300;
    preview.tick(now).unwrap();
    let opened = menu(&preview).expect("the item menu");
    assert_eq!(
        opened.entries,
        [ItemEntry::Look, ItemEntry::Drop, ItemEntry::Cancel]
    );
    assert_eq!(
        (opened.top, opened.flipped),
        layout::menu_place(at, 3, layout::BARTER_EDGES)
    );
    assert_eq!((opened.top, opened.flipped), (349, false));
    preview.image().unwrap();
    // Look shows the examine text.
    assert!(preview.release(Some(at), false));
    assert_eq!(reply(&mut preview), b"A synthetic widget.");
    assert_eq!(preview.pointer.at, Some(at));
    // Drop: one entry down, and the widget lies at the player's feet.
    assert!(preview.click(entry, false));
    preview.hover(entry);
    now += 300;
    preview.tick(now).unwrap();
    assert!(menu(&preview).is_some());
    preview.hover([entry[0], entry[1] + 11]);
    assert!(preview.release(Some([entry[0], entry[1] + 11]), false));
    assert_eq!(preview.host.world.get(item).unwrap().owner, None);
    assert!(preview.step(KeyCode::Escape));
}

#[test]
fn spec014_use_on_window_arrow_menu_and_quick_look() {
    use vault_core::{skills::ItemEntry, ui::inventory as layout};
    let dir = corpus();
    let mut preview = world(dir.path());
    let player = preview.host.world.player;
    let item = widget(&mut preview, player);
    let target = npc(&preview);
    preview.open_inventory_on(Some(target));
    preview.image().unwrap();
    let entry = list_point(&preview, item);
    assert!(preview.click(entry, true));
    // A quick click prints the look line on the monitor.
    mark_monitor(&mut preview);
    preview.hover(entry);
    assert!(preview.click(entry, false));
    assert!(preview.release(Some(entry), false));
    assert!(contains(&monitor_line(&preview), b"Widget"));
    // A hold opens the menu against the use-on window's own right edge
    // (372, where the inventory window's 579 would not flip it).
    let mut now = preview.clock;
    assert!(preview.click(entry, false));
    let at = [330, entry[1]];
    preview.hover(at);
    now += 300;
    preview.tick(now).unwrap();
    let opened = match &preview.screen {
        Screen::Inventory(screen) => screen.menu.clone().expect("the item menu"),
        _ => panic!("the use-on window is open"),
    };
    assert_eq!(opened.entries[0], ItemEntry::Look);
    assert!(opened.flipped);
    assert_eq!(
        (opened.top, opened.flipped),
        layout::menu_place(at, opened.entries.len(), layout::USE_ON_EDGES)
    );
    assert!(!layout::menu_place(at, opened.entries.len(), layout::INVENTORY_EDGES).1);
    preview.image().unwrap();
    // Look sends the examine text to the monitor (no stats panel here).
    mark_monitor(&mut preview);
    assert!(preview.release(Some(at), false));
    assert_eq!(monitor_line(&preview), b"A synthetic widget.");
    assert!(preview.step(KeyCode::Escape));
}

#[test]
fn spec014_hotkeys_work_over_the_bar_but_drop_while_edge_scrolling() {
    let dir = corpus();
    let mut preview = world(dir.path());
    preview.tick(16).unwrap();
    preview.image().unwrap();
    // Resting over the interface bar, `i` opens the inventory.
    preview.hover([320, 430]);
    preview.tick(100).unwrap();
    assert!(preview.pointer.edge.is_none());
    assert!(!preview.pointer_swallows_keys());
    assert!(preview.step(KeyCode::KeyI));
    assert!(matches!(preview.screen, Screen::Inventory(_)));
    assert!(preview.step(KeyCode::Escape));
    // On an edge pixel (row 479, below the bar, or the corner a letterboxed
    // window's border clamps to) the view scrolls and §3.29 drops keys.
    for (point, now) in [([320, 479], 200), ([639, 479], 300)] {
        preview.hover(point);
        preview.tick(now).unwrap();
        assert!(preview.pointer.edge.is_some());
        assert!(preview.pointer_swallows_keys());
        assert!(preview.step(KeyCode::KeyI));
        assert!(matches!(preview.screen, Screen::Map));
    }
    // Back over the bar, keys work again.
    preview.hover([320, 430]);
    preview.tick(400).unwrap();
    assert!(preview.step(KeyCode::KeyI));
    assert!(matches!(preview.screen, Screen::Inventory(_)));
}

#[test]
fn spec005_player_art_follows_the_gender_and_the_inventory_window() {
    use vault_formats::{Fid, pro::BasicFlags};
    let dir = corpus();
    std::fs::write(
        dir.path().join("art/critters/critters.lst"),
        b"sample,0\nhfjmps,0\nhmjmps,0\n",
    )
    .unwrap();
    let mut preview = world(dir.path());
    let player = preview.host.world.player;
    let base =
        |preview: &MapPreview| Fid(preview.host.world.get(player).unwrap().raw.art_id).base();
    // Loading the player sheet ran the gender update: the jumpsuit row.
    let context = preview.host.world.context(player);
    let female = preview.host.world.player_sheet().unwrap().stat(34, context) == 1;
    let jumpsuit = if female { 1 } else { 2 };
    assert_eq!(preview.host.world.default_player_art, jumpsuit);
    assert_eq!(base(&preview), jumpsuit);
    // Armor worn through the window shows once the window closes.
    let mut bytes = super::fixture::prototype(0, 0);
    super::fixture::word(&mut bytes, 0, 0x30);
    super::fixture::word(&mut bytes, 0x79, 0x0100_0007);
    super::fixture::word(&mut bytes, 0x7d, 0x0100_0008);
    let prototype = Arc::new(vault_formats::Prototype::parse(&bytes).unwrap());
    let armor = preview.host.world.spawn(prototype, 0, 0).unwrap();
    preview.host.world.add_item(player, armor, 1, true).unwrap();
    preview.image().unwrap();
    assert!(preview.step(KeyCode::KeyI));
    preview
        .host
        .world
        .equip(player, armor, BasicFlags::WORN.0)
        .unwrap();
    preview.image().unwrap();
    assert_eq!(base(&preview), jumpsuit, "unchanged while open");
    assert!(preview.step(KeyCode::Escape));
    assert_eq!(base(&preview), if female { 8 } else { 7 });
    // The use-on window never applies the figure art.
    let target = npc(&preview);
    preview.open_inventory_on(Some(target));
    preview.host.world.unequip(player, armor).unwrap();
    assert!(preview.step(KeyCode::Escape));
    assert_eq!(base(&preview), if female { 8 } else { 7 });
    // The inventory window does: the jumpsuit again.
    assert!(preview.step(KeyCode::KeyI));
    assert!(preview.step(KeyCode::Escape));
    assert_eq!(base(&preview), jumpsuit);
}

#[test]
fn g01_g09_game_worlds_keep_escape_and_leave_the_preview_keys_to_debug() {
    let dir = corpus();
    let vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(dir.path()).unwrap());
    let start = |debug: bool| {
        let mut game =
            Game::new(vfs.clone(), "english", 1, dir.path().join("saves"), None).unwrap();
        game.debug_keys = debug;
        let mut preview = crate::preview::AssetPreview::Game(Box::new(game));
        // G-01: the game never exits on a key; the standalone tools do.
        assert!(!preview.escape_exits());
        assert!(preview.step(KeyCode::KeyN));
        assert!(preview.step(KeyCode::KeyT));
        preview.tick(16).unwrap();
        let crate::preview::AssetPreview::Game(game) = preview else {
            unreachable!()
        };
        game
    };
    assert!(crate::preview::AssetPreview::Map(Box::new(world(dir.path()))).escape_exits());
    let mut game = start(false);
    let world = game.world().unwrap();
    world.finish_fades();
    // A script disabled the interface (SPEC-005 0x8133): Escape opens
    // nothing, and it is consumed rather than left to quit the program.
    world.host.world.ui_disabled = true;
    assert!(world.step(KeyCode::Escape));
    assert!(matches!(world.screen, Screen::Map));
    world.host.world.ui_disabled = false;
    // G-09: the map preview tool's keys do nothing in the game.
    let (elevation, roofs) = (world.view.elevation, world.view.roofs);
    for key in [
        KeyCode::KeyW,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::KeyR,
        KeyCode::KeyL,
        KeyCode::KeyD,
        KeyCode::KeyU,
        KeyCode::KeyG,
        KeyCode::KeyT,
    ] {
        assert!(!world.step(key), "{key:?}");
    }
    assert!(matches!(world.screen, Screen::Map));
    assert_eq!((world.view.elevation, world.view.roofs), (elevation, roofs));
    assert!(!world.title().contains("world map") && !world.title().contains("roofs"));
    // With --debug they work, but no key turns the roofs off (§3.36).
    let mut game = start(true);
    let world = game.world().unwrap();
    world.finish_fades();
    assert!(!world.step(KeyCode::KeyR));
    assert_eq!(world.view.roofs, roofs);
    assert!(world.step(KeyCode::PageUp));
    assert_eq!(world.view.elevation, elevation + 1);
    assert!(world.step(KeyCode::KeyW));
    assert!(matches!(world.screen, Screen::World(_)));
}

/// The player beside the synthetic NPC, in a world with a frame drawn;
/// returns the NPC's hex.
fn beside_npc(preview: &mut MapPreview) -> i32 {
    let player = preview.host.world.player;
    let target = npc(preview);
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
    preview.tick(16).unwrap();
    preview.image().unwrap();
    hex
}

#[test]
fn spec014_item_slot_cycles_the_empty_hand_and_aims_the_crosshair_attack() {
    use vault_core::{critical::hit_mode, ui::bar::WeaponMode};
    let dir = corpus();
    let mut preview = world(dir.path());
    let hex = beside_npc(&mut preview);
    let player = preview.host.world.player;
    assert!(preview.active_item().is_none(), "bare hands");
    // §3.23: an empty hand punches, with no secondary mode and nothing to
    // reload: `n` runs primary, primary aimed, primary.
    assert_eq!(preview.slot_mode(), WeaponMode::Primary);
    assert!(preview.step(KeyCode::KeyN));
    assert_eq!(preview.slot_mode(), WeaponMode::PrimaryAimed);
    assert_eq!(preview.slot_attack(), Some((hit_mode::PUNCH, true)));
    assert!(preview.step(KeyCode::KeyN));
    assert_eq!(preview.slot_mode(), WeaponMode::Primary);
    assert!(preview.step(KeyCode::KeyN));
    // The aimed punch costs 3 + 1 AP (SPEC-007 §3.4): with 3 AP the
    // attack is refused before any draw.
    preview.host.world.combat = true;
    // SPEC-014 §3.27: the player's turn, or the combat lock discards input.
    preview.host.world.combat_state.player_turn = true;
    preview.host.world.critter_state_mut(player).unwrap().ap = 3;
    preview.hud.mode = Mode::Crosshair;
    let before = preview.host.world.rng.clone();
    assert!(preview.tap(on_hex(&preview, hex)));
    assert!(
        preview.host.world.rng == before,
        "refused: {}",
        preview.status.escape_ascii()
    );
    // A right-click on the slot cycles back to the plain punch, which
    // goes ahead (its kick draw comes first).
    assert!(preview.click([360, 440], true));
    assert_eq!(preview.slot_mode(), WeaponMode::Primary);
    assert!(preview.tap(on_hex(&preview, hex)));
    assert!(preview.host.world.rng != before, "the attack drew");
}

#[test]
fn spec014_item_slot_weapon_modes_labels_and_reload() {
    use vault_core::ui::bar::WeaponMode;
    use vault_formats::pro::BasicFlags;
    let dir = corpus();
    write_art(
        dir.path(),
        &[
            ("sattkbup.frm", 188, 67, 3),
            ("single.frm", 20, 10, 5),
            ("burst.frm", 20, 10, 6),
            ("reload.frm", 20, 10, 7),
            ("bullseye.frm", 18, 18, 8),
        ],
    );
    let mut preview = world(dir.path());
    let player = preview.host.world.player;
    // A rifle: primary single fire (6), secondary burst (7), 4 and 5 AP,
    // caliber 1, 10 rounds with 5 loaded, no inventory art.
    let mut weapon = super::fixture::prototype(0, 3);
    super::fixture::word(&mut weapon, 0, 0x0000_0010);
    super::fixture::word(&mut weapon, 0x18, 0x76);
    super::fixture::word(&mut weapon, 0x34, -1);
    super::fixture::word(&mut weapon, 0x39 + 32, 4);
    super::fixture::word(&mut weapon, 0x39 + 36, 5);
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
    spawn(&mut preview, &ammo, 2);
    preview.host.world.get_mut(gun).unwrap().raw.state = ObjectState::Other {
        word: 0,
        detail: vault_formats::map::ObjectDetail::Weapon {
            rounds: 5,
            ammo_pid: 0x11,
        },
    };
    preview
        .host
        .world
        .equip_slot(player, gun, BasicFlags::RIGHT_HAND.0)
        .unwrap();
    preview.host.world.active_hand = 1;
    // The label (right edge at slot x 181, y 7) and the bullseye (163, 42)
    // in the slot at screen (267, 405).
    let shown = |preview: &mut MapPreview| {
        preview.image().unwrap();
        (
            frame_pixel(preview, [430, 414]),
            frame_pixel(preview, [435, 452]),
        )
    };
    assert_eq!(shown(&mut preview), (5, 3), "single, not aimed");
    let mut cycle = Vec::new();
    for _ in 0..4 {
        assert!(preview.step(KeyCode::KeyN));
        cycle.push((preview.slot_mode(), shown(&mut preview)));
    }
    // Burst fire cannot aim (SPEC-007 §3.4), so secondary aimed is skipped.
    assert_eq!(
        cycle,
        [
            (WeaponMode::PrimaryAimed, (5, 8)),
            (WeaponMode::Secondary, (6, 3)),
            (WeaponMode::Reload, (7, 3)),
            (WeaponMode::Primary, (5, 3)),
        ]
    );
    // The hand keeps its object's mode; the other (empty) hand has its own.
    assert!(preview.step(KeyCode::KeyN));
    assert!(preview.step(KeyCode::KeyN));
    assert_eq!(preview.slot_mode(), WeaponMode::Secondary);
    assert!(preview.step(KeyCode::KeyB));
    assert_eq!(preview.slot_mode(), WeaponMode::Primary);
    assert!(preview.step(KeyCode::KeyB));
    assert_eq!(preview.slot_mode(), WeaponMode::Secondary);
    assert!(preview.step(KeyCode::KeyN));
    assert_eq!(preview.slot_mode(), WeaponMode::Reload);
    // A left-click in reload mode reloads until full; in combat 2 AP are
    // checked first (1 AP does nothing) and spent once something loaded.
    // The mode then advances to primary.
    preview.host.world.combat = true;
    // SPEC-014 §3.27: the player's turn, or the combat lock discards input.
    preview.host.world.combat_state.player_turn = true;
    preview.host.world.critter_state_mut(player).unwrap().ap = 1;
    preview.tap([360, 440]);
    assert_eq!(preview.host.world.loaded(gun).0, 5);
    assert_eq!(preview.slot_mode(), WeaponMode::Reload);
    preview.host.world.critter_state_mut(player).unwrap().ap = 5;
    preview.tap([360, 440]);
    assert_eq!(preview.host.world.loaded(gun).0, 10);
    assert_eq!(preview.host.world.critter_state(player).unwrap().ap, 3);
    assert_eq!(preview.slot_mode(), WeaponMode::Primary);
    // Full, the weapon has no reload mode.
    for mode in [
        WeaponMode::PrimaryAimed,
        WeaponMode::Secondary,
        WeaponMode::Primary,
    ] {
        assert!(preview.step(KeyCode::KeyN));
        assert_eq!(preview.slot_mode(), mode);
    }
    // Another weapon mode sets the crosshair and, out of combat, asks for
    // combat.
    preview.host.world.combat = false;
    preview.tap([360, 440]);
    assert_eq!(preview.hud.mode, Mode::Crosshair);
    assert!(preview.host.world.pending_combat.is_some());
}

#[test]
fn g03_bar_buttons_do_nothing_while_the_interface_is_disabled() {
    let dir = corpus();
    let mut preview = world(dir.path());
    preview.image().unwrap();
    let button = [220, 429];
    preview.hover(button);
    preview.host.world.ui_disabled = true;
    assert!(preview.click(button, false));
    assert_eq!(preview.held_button(), None);
    assert!(!preview.release(Some(button), false));
    assert!(matches!(preview.screen, Screen::Map));
    // Neither `n` nor the slot's right-click cycles the item mode.
    assert!(!preview.step(KeyCode::KeyN));
    assert!(preview.click([360, 440], true));
    assert_eq!(
        preview.slot_mode(),
        vault_core::ui::bar::WeaponMode::Primary
    );
    // A button pressed before the interface was disabled sends nothing.
    preview.host.world.ui_disabled = false;
    assert!(preview.click(button, false));
    preview.host.world.ui_disabled = true;
    preview.release(Some(button), false);
    assert!(matches!(preview.screen, Screen::Map));
    preview.host.world.ui_disabled = false;
    assert!(preview.tap(button));
    assert!(matches!(preview.screen, Screen::Inventory(_)));
}

#[test]
fn spec014_game_fades_hold_the_world_input_and_the_frame() {
    let dir = corpus();
    let mut preview = world(dir.path());
    preview.tick(16).unwrap();
    let before = preview.image().unwrap();
    let ticks = preview.host.world.clock.ticks;
    // A script's `gfade_out; move_to; gfade_in` in one pass: both fades
    // are requested, then the player moves.
    for out in [true, false] {
        preview.host.world.emit(WorldEffect::Fade {
            out,
            duration: 0,
            from_black: false,
        });
    }
    let player = preview.host.world.player;
    let object = preview.host.world.get(player).unwrap();
    let (hex, elevation) = (object.raw.hex.unwrap(), object.raw.elevation);
    preview
        .host
        .world
        .move_object(player, hex + 400, elevation, false)
        .unwrap();
    preview.tick(32).unwrap();
    assert!(preview.fade_blocks());
    // The frame stays while the screen goes black: the move is hidden.
    assert_eq!(preview.image().unwrap().pixels(), before.pixels());
    // Input is swallowed and the world waits.
    assert!(preview.step(KeyCode::KeyI));
    assert!(preview.tap([220, 429]));
    preview.hover([100, 100]);
    assert!(matches!(preview.screen, Screen::Map));
    assert_eq!(preview.pointer.at, None);
    let mut now = 32;
    while preview.fade_blocks() {
        now += 16;
        preview.tick(now).unwrap();
        assert!(now < 3000, "the fades end");
    }
    assert!(now >= 32 + 2 * super::FADE_MS);
    assert_eq!(preview.host.world.clock.ticks, ticks, "the clock stood");
    // Afterwards the pointer's move applies, the moved player shows under
    // the game palette, and the world runs again.
    assert_eq!(preview.pointer.at, Some([100, 100]));
    assert_eq!(preview.display_override(), None);
    assert_ne!(preview.image().unwrap().pixels(), before.pixels());
    preview.tick(now + 200).unwrap();
    assert!(preview.host.world.clock.ticks > ticks);
    // A library fade services background work: it never blocks.
    preview.host.world.emit(WorldEffect::Fade {
        out: true,
        duration: 300,
        from_black: false,
    });
    preview.tick(now + 216).unwrap();
    assert!(!preview.fade_blocks());
    assert!(preview.step(KeyCode::KeyI));
    assert!(matches!(preview.screen, Screen::Inventory(_)));
}

#[test]
fn g22_a_window_opened_over_a_held_map_press_drops_it() {
    let dir = corpus();
    let mut preview = world(dir.path());
    let target = npc(&preview);
    let hex = preview.host.world.get(target).unwrap().raw.hex.unwrap();
    preview.tick(16).unwrap();
    preview.image().unwrap();
    let at = on_hex(&preview, hex);
    preview.hud.mode = Mode::Arrow;
    preview.hover(at);
    assert!(preview.click(at, false));
    assert!(preview.pointer.held.is_some());
    // `i` opens the inventory with the button still down; the release
    // lands in the inventory.
    assert!(preview.step(KeyCode::KeyI));
    assert!(preview.pointer.held.is_none());
    preview.release(Some(at), false);
    assert!(preview.step(KeyCode::Escape));
    assert!(matches!(preview.screen, Screen::Map));
    // Back on the map no hold pulse opens the action menu.
    let mut now = preview.clock;
    for _ in 0..40 {
        now += 16;
        preview.tick(now).unwrap();
    }
    assert!(preview.pointer.menu.is_none());
}

/// An item of misc subtype `pid` carried by the player.
fn carried(preview: &mut MapPreview, pid: i32) -> vault_core::ObjectRef {
    let player = preview.host.world.player;
    let mut bytes = super::fixture::prototype(0, 5);
    super::fixture::word(&mut bytes, 0, pid);
    let prototype = Arc::new(vault_formats::Prototype::parse(&bytes).unwrap());
    let item = preview.host.world.spawn(prototype, 0, 0).unwrap();
    preview.host.world.add_item(player, item, 1, true).unwrap();
    item
}

#[test]
fn g15_a_use_only_explosive_in_the_slot_opens_set_timer() {
    use vault_formats::pro::BasicFlags;
    let dir = corpus();
    let mut preview = world(dir.path());
    let player = preview.host.world.player;
    let dynamite = carried(&mut preview, vault_core::skills::DYNAMITE as i32);
    preview
        .host
        .world
        .equip_slot(player, dynamite, BasicFlags::RIGHT_HAND.0)
        .unwrap();
    preview.host.world.active_hand = 1;
    preview.image().unwrap();
    // SPEC-014 §3.23: a use-only item is used at once (no use crosshair);
    // in combat it needs and spends 2 AP. SPEC-013 §3.10: the use opens
    // the Set Timer window over the map.
    preview.host.world.combat = true;
    // SPEC-014 §3.27: the player's turn, or the combat lock discards input.
    preview.host.world.combat_state.player_turn = true;
    preview.host.world.critter_state_mut(player).unwrap().ap = 5;
    assert!(preview.tap([360, 440]));
    assert_eq!(preview.host.world.critter_state(player).unwrap().ap, 3);
    assert_eq!(preview.hud.mode, Mode::Movement);
    assert!(preview.hud.timer.is_some());
    preview.image().unwrap();
    // Plus makes it 70 seconds (with the red-button sounds); Enter arms.
    assert!(preview.click([140 + 196, 80 + 66], false));
    assert!(preview.step(KeyCode::Enter));
    assert!(preview.hud.timer.is_none());
    let object = preview.host.world.get(dynamite).unwrap();
    assert_eq!(object.raw.pid, vault_core::skills::DYNAMITE_ARMED);
}

#[test]
fn g17_combat_panel_animates_lights_and_closes() {
    let dir = corpus();
    let mut preview = world(dir.path());
    preview.image().unwrap();
    assert_eq!(preview.hud.panel.frame, None);
    // Entering combat: `iciboxx1`, frames 0-4 one every 200 ms, the bar
    // blocking meanwhile; then the buttons appear with the red light.
    preview.host.world.combat = true;
    preview.host.world.combat_state.player_turn = false;
    assert!(preview.panel_tick(1000));
    assert_eq!(preview.hud.panel.frame, Some(0));
    assert!(preview.bar_blocks());
    assert!(!preview.hud.panel.buttons());
    assert!(preview.panel_tick(1200));
    assert_eq!(preview.hud.panel.frame, Some(1));
    assert!(preview.panel_tick(1800));
    assert_eq!(preview.hud.panel.frame, Some(4));
    assert!(!preview.bar_blocks());
    assert!(preview.hud.panel.buttons());
    assert_eq!(preview.hud.panel.light, Some(false));
    // Disabled (red), End turn takes no press.
    assert!(preview.click([590 + 5, 379 + 43 + 5], false));
    assert!(preview.hud.held.is_none());
    // The player's turn lights it green; then the button presses.
    preview.host.world.combat_state.player_turn = true;
    assert!(preview.panel_tick(1900));
    assert_eq!(preview.hud.panel.light, Some(true));
    preview.image().unwrap();
    // The combat end removes the buttons and runs frames 4-0.
    preview.host.world.combat = false;
    assert!(preview.panel_tick(2000));
    assert_eq!(preview.hud.panel.frame, Some(4));
    assert!(!preview.hud.panel.buttons());
    assert!(preview.panel_tick(2800));
    assert_eq!(preview.hud.panel.frame, Some(0));
    assert!(!preview.bar_blocks());
    // A load settles the panel without animation.
    preview.host.world.combat = true;
    preview.settle_panel();
    assert!(preview.hud.panel.buttons());
    assert_eq!(preview.hud.panel.frame, Some(4));
}

#[test]
fn g17_crippled_arms_disable_the_slot_and_the_ammo_bar_is_drawn() {
    let dir = corpus();
    let mut preview = world(dir.path());
    let player = preview.host.world.player;
    preview.image().unwrap();
    // The empty hand holds no ammunition: the bar is raw index 14 top to
    // bottom at (463, 26) in the bar.
    for y in [26, 60, 95] {
        assert_eq!(frame_pixel(&preview, [463, 379 + y]), 14);
    }
    // Both arms crippled: the empty hand (a weapon) is disabled and its
    // button takes no press.
    preview
        .host
        .world
        .critter_state_mut(player)
        .unwrap()
        .results
        .0 |= effect::CRIPPLED_ARMS;
    assert!(preview.slot_disabled());
    assert!(preview.tap([360, 440]));
    assert_eq!(preview.hud.mode, Mode::Movement);
    assert!(preview.host.world.pending_combat.is_none());
}

#[test]
fn g20_take_all_is_shift_a_only() {
    let dir = corpus();
    let mut preview = world(dir.path());
    let player = preview.host.world.player;
    let npc = npc(&preview);
    let item = widget(&mut preview, npc);
    preview.host.world.pending_loot = Some(LootRequest {
        looter: player,
        target: npc,
        steal: false,
    });
    preview.image().unwrap();
    assert!(matches!(preview.screen, Screen::Loot(_)));
    assert!(!preview.step(KeyCode::KeyA));
    assert_eq!(preview.host.world.get(item).unwrap().owner, Some(npc));
    preview.set_modifiers([true, false, false]);
    assert!(preview.step(KeyCode::KeyA));
    preview.set_modifiers([false, false, false]);
    assert_eq!(preview.host.world.get(item).unwrap().owner, Some(player));
    // Ctrl+Q opens the quit confirmation; Yes closes the window.
    preview.set_modifiers([false, true, false]);
    assert!(preview.step(KeyCode::KeyQ));
    preview.set_modifiers([false, false, false]);
    assert!(preview.step(KeyCode::KeyY));
    assert!(matches!(preview.screen, Screen::Map));
    assert!(preview.quit_requested);
}

#[test]
fn g21_overwrite_question_and_description_box_take_the_mouse() {
    let dir = corpus();
    let mut preview = world(dir.path());
    preview.image().unwrap();
    assert!(preview.step(KeyCode::F4));
    preview.image().unwrap();
    // An empty slot: the description box; its Cancel returns to the list.
    assert!(preview.step(KeyCode::Enter));
    let description = |preview: &MapPreview| matches!(&preview.screen, Screen::SaveLoad(screen) if screen.description_open());
    assert!(description(&preview));
    preview.image().unwrap();
    assert!(preview.tap([169 + 160 + 2, 116 + 58 + 2]));
    assert!(!description(&preview));
    // Again, then Done saves.
    assert!(preview.step(KeyCode::Enter));
    preview.set_modifiers([true, false, false]);
    assert!(preview.step(KeyCode::KeyB));
    preview.set_modifiers([false, false, false]);
    assert!(preview.tap([169 + 34 + 2, 116 + 58 + 2]));
    assert!(matches!(preview.screen, Screen::Map));
    // The occupied slot asks first: No returns to the list, Yes opens the
    // description prefilled.
    assert!(preview.step(KeyCode::F4));
    assert!(preview.step(KeyCode::Enter));
    assert!(preview.dialog.is_some());
    preview.image().unwrap();
    assert!(preview.tap([169 + 182 + 2, 131 + 102 + 2]));
    assert!(preview.dialog.is_none() && !description(&preview));
    assert!(preview.step(KeyCode::Enter));
    assert!(preview.tap([169 + 50 + 2, 131 + 102 + 2]));
    assert!(description(&preview));
    // F10 in the list opens the quit confirmation (after Esc leaves the
    // description); its Yes closes the screen as cancelled.
    assert!(preview.step(KeyCode::Escape));
    assert!(preview.step(KeyCode::F10));
    assert!(preview.step(KeyCode::KeyY));
    assert!(matches!(preview.screen, Screen::Map));
    assert!(preview.quit_requested);
}

#[test]
fn g21_a_held_save_list_arrow_repeats_after_13_frames() {
    let dir = corpus();
    let mut preview = world(dir.path());
    preview.image().unwrap();
    assert!(preview.step(KeyCode::F4));
    preview.image().unwrap();
    let arrow = [36, 73];
    preview.hover(arrow);
    let start = preview.clock;
    assert!(preview.click(arrow, false));
    assert_eq!(preview.slot_session().0.selected, 1);
    // Other input is discarded while the arrow is held.
    assert!(preview.step(KeyCode::Escape));
    assert!(matches!(preview.screen, Screen::SaveLoad(_)));
    preview.tick(start + 13 * 41 - 1).unwrap();
    assert_eq!(preview.slot_session().0.selected, 1);
    preview.tick(start + 13 * 41).unwrap();
    assert_eq!(preview.slot_session().0.selected, 2);
    preview.tick(start + 13 * 41 + 200).unwrap();
    assert_eq!(preview.slot_session().0.selected, 3);
    // The release ends the repeat.
    assert!(preview.release(Some(arrow), false));
    preview.tick(start + 5000).unwrap();
    assert_eq!(preview.slot_session().0.selected, 3);
}

#[test]
fn g24_pause_windows_grey_the_view() {
    let dir = corpus();
    let mut preview = world(dir.path());
    let before = preview.image().unwrap();
    preview.set_modifiers([false, true, false]);
    assert!(preview.step(KeyCode::KeyP));
    preview.set_modifiers([false, false, false]);
    assert!(matches!(preview.screen, Screen::Options(_)));
    let paused = preview.image().unwrap();
    let grey = vault_core::ui::grey_table(&preview.palette);
    for at in [10 * 640 + 10, 300 * 640 + 600] {
        assert_eq!(paused.pixels()[at], grey[usize::from(before.pixels()[at])]);
    }
    // The brightness keys keep it open; any other key closes it.
    assert!(preview.step(KeyCode::Equal));
    assert!(matches!(preview.screen, Screen::Options(_)));
    assert!(preview.step(KeyCode::Space));
    assert!(matches!(preview.screen, Screen::Map));
    // The world map's `p` opens its own pause window.
    preview.open_world_map();
    assert!(preview.step(KeyCode::KeyP));
    assert!(matches!(&preview.screen, Screen::World(world) if world.paused));
    preview.image().unwrap();
    assert!(preview.click([300, 300], false));
    assert!(matches!(&preview.screen, Screen::World(world) if !world.paused));
}

#[test]
fn g26_z_opens_the_alarm_clock_and_f12_saves_a_screenshot() {
    let dir = corpus();
    let mut preview = world(dir.path());
    preview.image().unwrap();
    assert!(preview.step(KeyCode::F12));
    assert!(dir.path().join("saves/scr00000.ppm").exists());
    assert!(preview.step(KeyCode::KeyZ));
    assert!(matches!(
        &preview.screen,
        Screen::PipBoy(pipboy) if pipboy.tab() == Some(super::pipboy::Tab::Alarm)
    ));
}

#[test]
fn g31_diagnostics_never_reach_the_monitor() {
    let dir = corpus();
    let mut preview = world(dir.path());
    preview.image().unwrap();
    mark_monitor(&mut preview);
    preview.status = b"invalid object reference".to_vec();
    preview.image().unwrap();
    assert_eq!(monitor_line(&preview), b"mark");
}

#[test]
fn g23_preference_controls_act_on_release_and_sliders_drag() {
    let dir = corpus();
    let mut preview = world(dir.path());
    preview.image().unwrap();
    assert!(preview.step(KeyCode::Escape));
    assert!(preview.step(KeyCode::KeyP));
    preview.image().unwrap();
    // A brightness hotkey inside preferences joins the cancellable state:
    // Done keeps it (G-31).
    assert!(preview.step(KeyCode::Equal));
    let raised = preview.preferences.brightness;
    assert!(raised > vault_core::ui::options::BRIGHTNESS_MIN);
    // A slider press released before the first iteration changes nothing.
    let strip = [600, 400];
    preview.hover(strip);
    assert!(preview.click(strip, false));
    assert!(preview.release(Some(strip), false));
    let now = preview.clock;
    preview.tick(now + 40).unwrap();
    assert_eq!(preview.preferences.brightness, raised);
    // Held, the drag follows the pointer and applies the gamma at once.
    assert!(preview.click(strip, false));
    preview.tick(now + 80).unwrap();
    assert_eq!(
        preview.preferences.brightness,
        vault_core::ui::options::slider_value(
            594,
            vault_core::ui::options::BRIGHTNESS_MIN,
            vault_core::ui::options::BRIGHTNESS_MAX
        )
    );
    assert!(preview.release(Some(strip), false));
    preview.tick(now + 120).unwrap();
    // Cancel restores the brightness on opening.
    assert!(preview.step(KeyCode::Escape));
    assert_eq!(
        preview.preferences.brightness,
        vault_core::ui::options::BRIGHTNESS_MIN
    );
}

#[test]
fn g31_menu_buttons_need_an_opaque_up_pixel_and_the_left_button() {
    let dir = corpus();
    write_art(
        dir.path(),
        &[("menuup.frm", 26, 26, 0), ("menudown.frm", 26, 26, 2)],
    );
    let vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(dir.path()).unwrap());
    let mut game = Game::new(vfs, "english", 1, dir.path().join("saves"), None).unwrap();
    let button = [430, 90];
    game.hover(button);
    // A transparent up image has no clickable pixel.
    game.click(button, false);
    game.release(Some(button), false);
    assert_eq!(game.title(), "Vault — main menu");
    // A right press never presses a button.
    assert!(!game.click(button, true));
}

/// Tick the world in 50 ms steps until the player's sequence ends.
fn settle_player(preview: &mut MapPreview, now: &mut u64) {
    let player = preview.host.world.player;
    for _ in 0..400 {
        if !preview.host.owns_motion(player) && !preview.host.presenting() {
            return;
        }
        *now += 50;
        preview.tick(*now).unwrap();
    }
    panic!("the player's sequence ends");
}

#[test]
fn s04_a_combat_walk_locks_clicks_bar_and_keys_under_the_watch() {
    let dir = corpus();
    let mut preview = world(dir.path());
    let player = preview.host.world.player;
    let mut now = 16;
    preview.tick(now).unwrap();
    preview.image().unwrap();
    let start = preview.host.world.get(player).unwrap().raw.hex.unwrap();
    // SPEC-014 §3.27: the player's turn with AP to walk.
    preview.host.world.combat = true;
    preview.host.world.combat_state.player_turn = true;
    preview.host.world.critter_state_mut(player).unwrap().ap = 8;
    preview.hud.panel = super::hud::Panel::settled(true, true);
    let first = vault_formats::HexGrid::step(start, 1, 4).unwrap();
    let second = vault_formats::HexGrid::step(start, 4, 3).unwrap();
    assert!(preview.tap(on_hex(&preview, first)));
    assert!(preview.host.owns_motion(player), "the walk registered");
    // The walk locks the game mouse, the keyboard and the bar under the
    // animated watch; clicks made meanwhile are discarded, never queued.
    assert!(preview.combat_locked());
    now += 16;
    preview.tick(now).unwrap();
    assert!(preview.hud.watch.is_some());
    assert!(preview.tap(on_hex(&preview, second)));
    assert!(preview.pointer.held.is_none());
    // The Inventory button opens nothing; `i` is not read but held.
    assert!(preview.tap([211 + 5, 379 + 40 + 5]));
    assert!(preview.step(KeyCode::KeyI));
    assert!(matches!(preview.screen, Screen::Map));
    assert_eq!(preview.hud.held_keys.len(), 1);
    settle_player(&mut preview, &mut now);
    let end = preview.host.world.get(player).unwrap().raw.hex.unwrap();
    assert_ne!(end, second, "the second click did not re-route the walk");
    // A key typed during the player's own sequence takes effect when it
    // ends.
    assert!(matches!(preview.screen, Screen::Inventory(_)));
    assert!(preview.hud.held_keys.is_empty());
    // Once every sequence has ended the watch goes and input is back.
    now += 16;
    preview.tick(now).unwrap();
    assert!(!preview.combat_locked());
    assert!(preview.hud.watch.is_none());
}

#[test]
fn s04_hotkeys_are_refused_on_the_enemy_turn() {
    let dir = corpus();
    let mut preview = world(dir.path());
    preview.tick(16).unwrap();
    preview.image().unwrap();
    preview.host.world.combat = true;
    preview.host.world.combat_state.player_turn = false;
    preview.hud.panel = super::hud::Panel::settled(true, false);
    assert!(preview.combat_locked());
    // SPEC-014 §3.27: outside the player's turn the keyboard is disabled:
    // no window opens, no end of combat is requested and no screenshot is
    // taken; the keys are held.
    for key in [
        KeyCode::F12,
        KeyCode::KeyI,
        KeyCode::KeyC,
        KeyCode::KeyP,
        KeyCode::KeyS,
        KeyCode::Tab,
        KeyCode::Escape,
        KeyCode::Enter,
    ] {
        assert!(preview.step(key), "{key:?} is swallowed");
        assert!(matches!(preview.screen, Screen::Map), "{key:?}");
    }
    assert!(preview.host.world.combat);
    // The bar takes no press either.
    assert!(preview.tap([211 + 5, 379 + 40 + 5]));
    assert!(matches!(preview.screen, Screen::Map));
    assert!(preview.watch_shown());
    assert_eq!(preview.hud.held_keys.len(), 8);
    assert!(!preview.save_root.exists(), "no screenshot");
    // The player's turn gives the keyboard back and flushes the keys typed
    // on the enemy turn.
    preview.host.combat_lock.begin_turn();
    preview.host.world.combat_state.player_turn = true;
    preview
        .host
        .world
        .critter_state_mut(preview.host.world.player)
        .unwrap()
        .ap = 10;
    assert!(!preview.combat_locked());
    preview.tick(32).unwrap();
    assert!(preview.hud.held_keys.is_empty());
    assert!(matches!(preview.screen, Screen::Map));
    assert!(preview.host.world.combat);
    assert!(preview.step(KeyCode::KeyI));
    assert!(matches!(preview.screen, Screen::Inventory(_)));
}

#[test]
fn spec014_held_keys_keep_the_first_32_and_survive_the_end_of_combat() {
    let dir = corpus();
    let mut preview = world(dir.path());
    preview.tick(16).unwrap();
    preview.image().unwrap();
    preview.host.world.combat = true;
    preview.hud.panel = super::hud::Panel::settled(true, false);
    assert!(preview.keyboard_locked());
    // Presses and releases both count toward the 32 held events.
    for _ in 0..20 {
        assert!(preview.step(KeyCode::KeyM));
        preview.key_up(KeyCode::KeyM);
    }
    assert_eq!(preview.hud.held_keys.len(), 32);
    // The end of combat re-enables the keyboard without a flush: the 16
    // held `m` presses cycle the cursor mode in order.
    preview.host.world.combat = false;
    preview.host.combat_lock.end_turn();
    preview.hud.panel = super::hud::Panel::settled(false, false);
    let before = preview.hud.mode;
    preview.tick(32).unwrap();
    assert!(preview.hud.held_keys.is_empty());
    let mut expected = before;
    for _ in 0..16 {
        expected = vault_core::ui::cursor::cycle_mode(expected, false, true);
    }
    assert_eq!(preview.hud.mode, expected);
}

#[test]
fn s18_rotating_needs_the_standing_art() {
    let dir = corpus();
    let mut preview = world(dir.path());
    let player = preview.host.world.player;
    let mut now = 16;
    preview.tick(now).unwrap();
    preview.image().unwrap();
    let facing = |preview: &MapPreview| preview.host.world.get(player).unwrap().raw.orientation;
    // Standing: `.` rotates the player.
    let before = facing(&preview);
    assert!(preview.game_action(vault_core::ui::bar::Action::Rotate(true)));
    assert_eq!(facing(&preview), (before + 1) % 6);
    // Walking: SPEC-014 §3.27 rotate works only while standing.
    let start = preview.host.world.get(player).unwrap().raw.hex.unwrap();
    let target = vault_formats::HexGrid::step(start, 1, 6).unwrap();
    assert!(preview.tap(on_hex(&preview, target)));
    now += 100;
    preview.tick(now).unwrap();
    let state = preview.host.click_state();
    assert!(state.busy && !state.standing, "{state:?}");
    let walking = facing(&preview);
    assert!(!preview.game_action(vault_core::ui::bar::Action::Rotate(true)));
    assert_eq!(facing(&preview), walking);
    settle_player(&mut preview, &mut now);
    assert!(preview.game_action(vault_core::ui::bar::Action::Rotate(true)));
}

#[test]
fn s03_a_movement_click_cancels_a_reach_with_interrupt_walk_on() {
    for interrupt in [true, false] {
        let dir = corpus();
        // Six-frame reaches at 10 fps, acting on the last frame, so the
        // reach is still in progress when the click comes.
        let mut reach = super::fixture::frm(12, 20, 16);
        let block = reach[62..].to_vec();
        reach[4..6].copy_from_slice(&10_u16.to_be_bytes());
        reach[6..8].copy_from_slice(&5_u16.to_be_bytes());
        reach[8..10].copy_from_slice(&6_u16.to_be_bytes());
        reach[58..62].copy_from_slice(&(6 * block.len() as u32).to_be_bytes());
        for _ in 1..6 {
            reach.extend_from_slice(&block);
        }
        for name in ["sampleak.frm", "sampleal.frm"] {
            std::fs::write(dir.path().join("art/critters").join(name), &reach).unwrap();
        }
        let mut preview = world(dir.path());
        preview.interrupt_walk = interrupt;
        let player = preview.host.world.player;
        let mut now = 16;
        preview.tick(now).unwrap();
        preview.image().unwrap();
        let (start, elevation) = preview
            .host
            .world
            .get(player)
            .map(|object| (object.raw.hex.unwrap(), object.raw.elevation))
            .unwrap();
        let prototype =
            Arc::new(vault_formats::Prototype::parse(&super::fixture::prototype(0, 5)).unwrap());
        let next = vault_formats::HexGrid::step(start, 1, 1).unwrap();
        let item = preview
            .host
            .world
            .spawn(prototype, next, elevation)
            .unwrap();
        preview
            .host
            .action(player, item, vault_core::ObjectAction::Pickup)
            .unwrap();
        now += 100;
        preview.tick(now).unwrap();
        let state = preview.host.click_state();
        assert!(state.busy && !state.walking && !state.standing, "{state:?}");
        assert_eq!(preview.host.world.get(item).unwrap().owner, None);
        // SPEC-014 §3.27: out of combat, a movement click with
        // `interrupt_walk` on cancels the reach (its pickup dropped) and
        // walks; off, it does nothing and the pickup completes.
        let away = vault_formats::HexGrid::step(start, 4, 3).unwrap();
        assert!(preview.tap(on_hex(&preview, away)));
        settle_player(&mut preview, &mut now);
        let owner = preview.host.world.get(item).unwrap().owner;
        let hex = preview.host.world.get(player).unwrap().raw.hex.unwrap();
        if interrupt {
            assert_eq!(owner, None, "the pickup was dropped");
            assert_ne!(hex, start, "the walk replaced the reach");
        } else {
            assert_eq!(owner, Some(player), "the pickup completed");
            assert_eq!(hex, start, "the click did nothing");
        }
    }
}

#[test]
fn s01_s19_roofs_hidden_at_the_entrance_are_drawn_again_once_restored() {
    let dir = corpus();
    let mut preview = world(dir.path());
    let player = preview.host.world.player;
    let (start, elevation) = preview
        .host
        .world
        .get(player)
        .map(|object| (object.raw.hex.unwrap(), object.raw.elevation))
        .unwrap();
    // The fixture's one roofed square (5050) holds the entrance; the
    // entrance placement hid it (SPEC-014 §3.36).
    let roof = |preview: &MapPreview| {
        preview
            .host
            .world
            .map
            .tile(elevation as usize, 5050)
            .unwrap()
            .roof_hidden()
    };
    assert!(roof(&preview));
    let hex = vault_formats::HexGrid::step(start, 2, 3).unwrap();
    let prototype =
        Arc::new(vault_formats::Prototype::parse(&super::fixture::prototype(0, 5)).unwrap());
    let item = preview.host.world.spawn(prototype, hex, elevation).unwrap();
    preview.tick(16).unwrap();
    let point = on_hex(&preview, hex);
    let hidden = preview.map_view_image().unwrap().pixels()[(point[1] * 640 + point[0]) as usize];
    assert_eq!(preview.object_at(point), Some(item));
    // Stepping out onto an empty square restores the group: its art was
    // bound although hidden when the map bound (S-19), so it draws again
    // and blocks the pick (§3.27).
    let out = vault_formats::HexGrid::step(start, 4, 4).unwrap();
    preview
        .host
        .world
        .move_object(player, out, elevation, false)
        .unwrap();
    assert!(!roof(&preview));
    preview.tick(32).unwrap();
    let drawn = preview.map_view_image().unwrap().pixels()[(point[1] * 640 + point[0]) as usize];
    assert_ne!(drawn, hidden);
    assert_eq!(preview.object_at(point), None);
}

#[test]
fn g16_swap_hands_goes_through_the_world_and_returns_to_movement() {
    let dir = corpus();
    let mut preview = world(dir.path());
    let mut now = 16;
    preview.tick(now).unwrap();
    preview.image().unwrap();
    preview.hud.mode = Mode::Crosshair;
    let hand = preview.host.world.active_hand;
    // SPEC-014 §3.23: the hand flips at once; with both hands empty the
    // swap's sequence only returns the player to the standing art. While
    // a swap holds the player the bar and the mouse are disabled under the
    // watch, and afterwards a crosshair returns to movement.
    assert!(preview.step(KeyCode::KeyB));
    assert_eq!(preview.host.world.active_hand, hand ^ 1);
    if preview.hud.swapping {
        assert!(preview.watch_shown());
        assert!(preview.tap([211 + 5, 379 + 40 + 5]));
        assert!(matches!(preview.screen, Screen::Map));
    }
    settle_player(&mut preview, &mut now);
    now += 16;
    preview.tick(now).unwrap();
    assert!(!preview.hud.swapping && !preview.watch_shown());
    assert_eq!(preview.hud.mode, Mode::Movement);
    // A held swap (the player owning the swap's motion) locks the bar.
    preview.hud.swapping = true;
    let start = preview
        .host
        .world
        .get(preview.host.world.player)
        .unwrap()
        .raw
        .hex
        .unwrap();
    let target = vault_formats::HexGrid::step(start, 1, 6).unwrap();
    assert!(
        preview
            .host
            .walk_toward(preview.host.world.player, target, false, 0)
            .unwrap()
    );
    assert!(preview.watch_shown());
    // §3.23: every key read during the swap is discarded but F12, which
    // takes its screenshot; nothing is held.
    preview.step(KeyCode::KeyI);
    assert!(matches!(preview.screen, Screen::Map));
    assert!(preview.step(KeyCode::Escape));
    assert!(matches!(preview.screen, Screen::Map));
    assert!(preview.step(KeyCode::F12));
    assert!(preview.save_root.join("scr00000.ppm").exists());
    assert!(preview.hud.held_keys.is_empty());
    // The Swap hands button still presses (its art and sounds), but the
    // `b` it sends does nothing while the bar is disabled.
    let hand = preview.host.world.active_hand;
    let swap = [218 + 5, 379 + 6 + 5];
    preview.hover(swap);
    assert!(preview.click(swap, false));
    assert!(preview.held_button().is_some(), "pressed art");
    assert!(preview.release(Some(swap), false));
    assert_eq!(preview.host.world.active_hand, hand);
    settle_player(&mut preview, &mut now);
    now += 16;
    preview.tick(now).unwrap();
    assert!(!preview.hud.swapping);
}

/// A rifle in the active right hand with 5 of 10 rounds and ammunition
/// in the pack (SPEC-014 §3.23 weapon modes).
fn wield_rifle(preview: &mut MapPreview) -> vault_core::ObjectRef {
    use vault_formats::pro::BasicFlags;
    let player = preview.host.world.player;
    let mut weapon = super::fixture::prototype(0, 3);
    super::fixture::word(&mut weapon, 0, 0x0000_0010);
    super::fixture::word(&mut weapon, 0x18, 0x76);
    super::fixture::word(&mut weapon, 0x34, -1);
    super::fixture::word(&mut weapon, 0x39 + 32, 4);
    super::fixture::word(&mut weapon, 0x39 + 36, 5);
    super::fixture::word(&mut weapon, 0x39 + 52, 1);
    super::fixture::word(&mut weapon, 0x39 + 60, 10);
    let prototype = Arc::new(vault_formats::Prototype::parse(&weapon).unwrap());
    let gun = preview.host.world.spawn(prototype, 0, 0).unwrap();
    preview.host.world.add_item(player, gun, 1, true).unwrap();
    preview.host.world.get_mut(gun).unwrap().raw.state = ObjectState::Other {
        word: 0,
        detail: vault_formats::map::ObjectDetail::Weapon {
            rounds: 5,
            ammo_pid: 0x11,
        },
    };
    preview
        .host
        .world
        .equip_slot(player, gun, BasicFlags::RIGHT_HAND.0)
        .unwrap();
    preview.host.world.active_hand = 1;
    gun
}

#[test]
fn spec014_reload_mode_reports_no_attack() {
    use super::pointer::Feedback;
    use vault_core::ui::{ColorKey, bar::WeaponMode};
    let dir = corpus();
    let mut preview = world(dir.path());
    let hex = beside_npc(&mut preview);
    let player = preview.host.world.player;
    wield_rifle(&mut preview);
    while preview.slot_mode() != WeaponMode::Reload {
        assert!(preview.step(KeyCode::KeyN));
    }
    // §3.23: reload, use and use-on report no attack, so the aiming flag
    // is false too.
    assert_eq!(preview.slot_attack(), None);
    preview.hud.mode = Mode::Crosshair;
    // Out of combat the crosshair click starts no combat.
    assert!(preview.tap(on_hex(&preview, hex)));
    assert!(preview.host.world.pending_combat.is_none() && !preview.host.world.combat);
    assert!(preview.hud.replay.is_none());
    // In combat it does nothing: no attack, no message, no AP.
    preview.host.world.combat = true;
    preview.host.world.combat_state.player_turn = true;
    preview.host.world.critter_state_mut(player).unwrap().ap = 8;
    mark_monitor(&mut preview);
    let line = monitor_line(&preview);
    let before = preview.host.world.rng.clone();
    assert!(preview.tap(on_hex(&preview, hex)));
    assert!(preview.host.world.rng == before);
    assert_eq!(preview.host.world.critter_state(player).unwrap().ap, 8);
    assert!(preview.hud.called.is_none());
    preview.image().unwrap();
    assert_eq!(monitor_line(&preview), line);
    // The crosshair hover shows the red ` X ` over the critter.
    let feedback = preview.hover_feedback(on_hex(&preview, hex));
    assert!(
        matches!(&feedback, Some(Feedback::Hit(text, ColorKey::RED)) if text == b" X "),
        "{feedback:?}"
    );
}

#[test]
fn spec014_called_shot_window_opens_in_an_aimed_mode_and_chooses_a_part() {
    use vault_core::{
        critical::{hit_mode, location},
        ui::{bar::WeaponMode, called},
    };
    let dir = corpus();
    write_art(
        dir.path(),
        &[
            ("called.frm", 424, 309, 3),
            ("lilredup.frm", 15, 16, 4),
            ("lilreddn.frm", 15, 16, 5),
            ("numbers.frm", 360, 17, 6),
        ],
    );
    // The punch, kick and every reaction get art, so the attack's sequence
    // registers (SPEC-007 §3.10.1).
    for suffix in [
        "aq", "ar", "an", "ao", "ap", "ba", "bb", "bo", "bp", "ch", "cj",
    ] {
        std::fs::copy(
            dir.path().join("art/critters/sampleaa.frm"),
            dir.path().join(format!("art/critters/sample{suffix}.frm")),
        )
        .unwrap();
    }
    // Row 0 (`sample`) aliases itself: labels 1000-1007 and `samplena.frm`.
    std::fs::write(
        dir.path().join("art/critters/samplena.frm"),
        super::fixture::frm(170, 225, 9),
    )
    .unwrap();
    let labels: String = (0..8)
        .map(|part| format!("{{{}}}{{}}{{the part {part}}}\n", 1000 + part))
        .collect();
    std::fs::write(
        dir.path().join("text/english/game/combat.msg"),
        format!("{{100}}{{}}{{You need %d action points.}}\n{labels}"),
    )
    .unwrap();
    let mut preview = world(dir.path());
    preview.host.playback = vault_core::host_choreography::Playback::Instant;
    let hex = beside_npc(&mut preview);
    let target = npc(&preview);
    let player = preview.host.world.player;
    preview.host.world.critter_state_mut(target).unwrap().hp = 999;
    assert!(preview.step(KeyCode::KeyN));
    assert_eq!(preview.slot_mode(), WeaponMode::PrimaryAimed);
    preview.host.world.combat = true;
    preview.host.world.combat_state.player_turn = true;
    preview.hud.panel = super::hud::Panel::settled(true, true);
    let ap = |preview: &MapPreview| preview.host.world.critter_state(player).unwrap().ap;
    let reset = |preview: &mut MapPreview| {
        preview.host.world.shows.clear();
        preview.host.motions.clear();
        preview.host.world.critter_state_mut(player).unwrap().ap = 8;
    };
    reset(&mut preview);
    preview.hud.mode = Mode::Crosshair;
    let before = preview.host.world.rng.clone();
    // An aimed mode's crosshair click opens the window: no draw, no AP.
    assert!(preview.tap(on_hex(&preview, hex)));
    let window = preview.hud.called.as_ref().expect("the window opens");
    assert_eq!(window.target, target);
    let attack = preview
        .host
        .world
        .attack_profile(player, target, hit_mode::PUNCH, location::UNCALLED, true)
        .unwrap();
    assert_eq!(window.chances, called::chances(&attack).unwrap());
    assert_eq!(
        window.labels[usize::from(location::LEFT_LEG)]
            .as_ref()
            .map(|(text, _)| text.as_slice()),
        Some(b"the part 5".as_slice())
    );
    assert!(preview.host.world.rng == before);
    assert_eq!(ap(&preview), 8);
    // The opaque background and the `na` picture, 170 by 225 at (128, 31).
    preview.image().unwrap();
    assert_eq!(frame_pixel(&preview, [108 + 5, 20 + 5]), 3);
    assert_eq!(frame_pixel(&preview, [108 + 128, 20 + 31]), 9);
    assert_eq!(frame_pixel(&preview, [108 + 297, 20 + 255]), 9);
    assert_eq!(frame_pixel(&preview, [108 + 298, 20 + 256]), 3);
    // The digit keys do nothing; Esc closes it with nothing done, and the
    // cursor stays in crosshair mode.
    assert!(preview.step(KeyCode::Digit4));
    assert!(preview.hud.called.is_some());
    assert!(preview.step(KeyCode::Escape));
    assert!(preview.hud.called.is_none());
    assert_eq!(preview.hud.mode, Mode::Crosshair);
    assert!(preview.host.world.rng == before && ap(&preview) == 8);
    // Cancel's release is Esc too.
    assert!(preview.tap(on_hex(&preview, hex)));
    assert!(preview.tap([108 + 170 + 2, 20 + 268 + 2]));
    assert!(preview.hud.called.is_none());
    assert!(preview.host.world.rng == before && ap(&preview) == 8);
    // Ctrl+D chooses the torso: location 3, aimed, 3 + 1 AP.
    assert!(preview.tap(on_hex(&preview, hex)));
    preview.set_modifiers([false, true, false]);
    assert!(preview.step(KeyCode::KeyD));
    preview.set_modifiers([false; 3]);
    assert!(preview.hud.called.is_none());
    assert!(preview.host.world.rng != before, "the attack drew");
    assert_eq!(ap(&preview), 4);
    // A release at window (125, 236) is outside the right-leg button;
    // one pressed and released at (40, 236) chooses the right leg.
    reset(&mut preview);
    assert!(preview.tap(on_hex(&preview, hex)));
    assert!(preview.tap([108 + 125, 20 + 236]));
    assert!(preview.hud.called.is_some());
    let chosen = preview.host.world.rng.clone();
    assert!(preview.tap([108 + 40, 20 + 236]));
    assert!(preview.hud.called.is_none());
    assert!(preview.host.world.rng != chosen);
    assert_eq!(ap(&preview), 4);
    // A pending quit request (a script's world-map request) ends the
    // window as Esc (`[May-improve]`): no attack.
    reset(&mut preview);
    assert!(preview.tap(on_hex(&preview, hex)));
    let quiet = preview.host.world.rng.clone();
    preview.host.world.pending_transition = Some(vault_core::world::Transition {
        map: 0,
        elevation: 0,
        hex: 0,
        facing: 0,
    });
    let now = preview.clock + 16;
    preview.tick(now).unwrap();
    assert!(preview.hud.called.is_none());
    assert!(preview.host.world.rng == quiet && ap(&preview) == 8);
    preview.host.world.pending_transition = None;
    // Too few AP for the aimed cost: message 100 with the cost, no window.
    reset(&mut preview);
    preview.host.world.critter_state_mut(player).unwrap().ap = 3;
    assert!(preview.tap(on_hex(&preview, hex)));
    assert!(preview.hud.called.is_none());
    preview.image().unwrap();
    assert_eq!(monitor_line(&preview), b"You need 4 action points.");
}

#[test]
fn spec014_an_attack_click_out_of_combat_starts_combat_and_is_replayed() {
    use vault_core::ui::bar::WeaponMode;
    let dir = corpus();
    write_art(
        dir.path(),
        &[
            ("called.frm", 424, 309, 3),
            ("lilredup.frm", 15, 16, 4),
            ("lilreddn.frm", 15, 16, 5),
        ],
    );
    let mut preview = world(dir.path());
    let hex = beside_npc(&mut preview);
    let target = npc(&preview);
    let player = preview.host.world.player;
    preview.host.world.critter_state_mut(player).unwrap().ap = 8;
    assert!(preview.step(KeyCode::KeyN));
    assert_eq!(preview.slot_mode(), WeaponMode::PrimaryAimed);
    preview.hud.mode = Mode::Crosshair;
    // §3.38 step 3: a passing click starts combat; no window opens yet.
    assert!(preview.tap(on_hex(&preview, hex)));
    assert_eq!(
        preview
            .host
            .world
            .pending_combat
            .map(|request| request.defender),
        Some(target)
    );
    assert!(preview.hud.called.is_none());
    assert_eq!(preview.hud.replay, Some(target));
    // The player's first turn gives the input back; the click is then
    // replayed with the refilled AP and, aimed, opens the window.
    let mut now = 16;
    for _ in 0..200 {
        if preview.hud.called.is_some() {
            break;
        }
        now += 50;
        preview.tick(now).unwrap();
    }
    assert!(preview.host.world.combat);
    assert!(preview.hud.replay.is_none());
    assert!(preview.hud.called.is_some(), "the replay opened the window");
}
