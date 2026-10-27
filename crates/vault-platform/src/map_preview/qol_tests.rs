//! The `enhanced` preset's game speed, tactical overlay and quick-save slot
//! in the interactive world, over the synthetic corpus, under both games'
//! profiles; and that Vanilla keeps none of them.
use super::super::{
    Screen,
    pointer::Feedback,
    test_support::{npc, synthetic_corpus},
};
use super::*;
use crate::enhanced::{EnhancedSettings, ViewportMode};
use vault_core::{
    host_save::slot_directory,
    ui::saveload::{Mode as SaveMode, SlotState},
};
use vault_formats::HexGrid;
use winit::keyboard::KeyCode;

/// The synthetic corpus with its map also under a save-manifest-sized
/// name, so a slot can be written.
fn corpus() -> tempfile::TempDir {
    let dir = synthetic_corpus();
    let map = std::fs::read(dir.path().join("maps/synthetic.map")).unwrap();
    std::fs::write(dir.path().join("maps/synth.map"), map).unwrap();
    dir
}
/// The synthetic world (its 8.3-named copy) under `preset`, saving below
/// `saves`, in the Vanilla frame's coordinates (the enhanced preset in the
/// fixed viewport).
fn world(dir: &std::path::Path, preset: EnginePreset) -> MapPreview {
    let mut preview = super::super::test_support::load(dir, "maps/synth.map");
    preview.save_root = dir.join("saves");
    if preset == EnginePreset::Enhanced {
        preview.host.set_preset(EnginePreset::Enhanced);
        preview.set_enhanced(Some(EnhancedSettings {
            viewport: ViewportMode::Fixed,
            ..Default::default()
        }));
    }
    preview
}
fn message(preview: &MapPreview) -> Option<&[u8]> {
    preview
        .qol_state
        .message
        .as_ref()
        .map(|message| message.text.as_slice())
}
/// The synthetic NPC's frame pixel (the replay tests' hover point).
const NPC_PIXEL: [i32; 2] = [336, 170];

#[test]
fn world_time_runs_at_the_game_speed_and_is_the_shell_clock_at_1x() {
    let dir = corpus();
    let mut preview = world(dir.path(), EnginePreset::Enhanced);
    // 1.0x: the world sees the shell clock itself.
    for now in [0, 16, 32, 1000] {
        preview.tick(now).unwrap();
        assert_eq!(preview.host.world.elapsed_ms, now);
    }
    // 1.5x: each frame's delta counts half again, carried in half
    // milliseconds so odd deltas lose nothing.
    preview.set_game_speed(GameSpeed::Fast);
    assert_eq!(preview.game_speed(), GameSpeed::Fast);
    assert_eq!(message(&preview), Some(b"Game speed 1.5x".as_slice()));
    preview.tick(1100).unwrap();
    assert_eq!(preview.host.world.elapsed_ms, 1000 + 150);
    preview.tick(1101).unwrap();
    assert_eq!(preview.host.world.elapsed_ms, 1150 + 1, "1.5 ms: one whole");
    preview.tick(1102).unwrap();
    assert_eq!(
        preview.host.world.elapsed_ms,
        1150 + 3,
        "and the half carried"
    );
    // 2.0x: twice the delta.
    preview.set_game_speed(GameSpeed::Faster);
    preview.tick(1202).unwrap();
    assert_eq!(preview.host.world.elapsed_ms, 1153 + 200);
    // Back to 1.0x: world time keeps what it ran ahead, and follows the
    // shell clock again.
    preview.set_game_speed(GameSpeed::Normal);
    preview.tick(1302).unwrap();
    assert_eq!(preview.host.world.elapsed_ms, 1353 + 100);
    // An open conversation runs at 1.0x.
    preview.set_game_speed(GameSpeed::Faster);
    preview.host.world.dialogue.open = true;
    preview.tick(1402).unwrap();
    assert_eq!(
        preview.host.world.elapsed_ms,
        1453 + 100,
        "1.0x in dialogue"
    );
    preview.host.world.dialogue.open = false;
    preview.tick(1502).unwrap();
    assert_eq!(preview.host.world.elapsed_ms, 1553 + 200);
}

#[test]
fn a_walk_takes_half_the_time_at_2x_and_two_thirds_at_1_5x() {
    let dir = corpus();
    // Shell milliseconds until the player's walk of a few hexes ends, and
    // where it ends.
    let walk = |speed: GameSpeed| {
        let mut preview = world(dir.path(), EnginePreset::Enhanced);
        preview.tick(0).unwrap();
        preview.set_game_speed(speed);
        let player = preview.host.world.player;
        let start = preview.host.world.get(player).unwrap().raw.hex.unwrap();
        let target = (0..6)
            .filter_map(|direction| HexGrid::step(start, direction, 4).ok())
            .find(|hex| {
                preview
                    .host
                    .plan_to_tile(player, *hex, usize::MAX)
                    .is_some_and(|path| path.directions.len() == 4)
            })
            .expect("a free four-hex walk");
        assert!(preview.host.walk_to(player, target).unwrap());
        let mut now = 0;
        while preview.host.owns_motion(player) {
            now += 16;
            preview.tick(now).unwrap();
            assert!(now < 60_000, "the walk ends");
        }
        let at = preview.host.world.get(player).unwrap().raw.hex;
        (now, at, target)
    };
    let (normal, at, target) = walk(GameSpeed::Normal);
    assert_eq!(at, Some(target));
    let (fast, fast_at, _) = walk(GameSpeed::Fast);
    let (faster, faster_at, _) = walk(GameSpeed::Faster);
    assert_eq!((fast_at, faster_at), (at, at), "the same walk");
    // Within a frame or two of the exact ratio.
    assert!(
        faster.abs_diff(normal / 2) <= 32,
        "2.0x: {faster} ms against {normal} ms"
    );
    assert!(
        fast.abs_diff(normal * 2 / 3) <= 32,
        "1.5x: {fast} ms against {normal} ms"
    );
}

#[test]
fn vanilla_ignores_the_game_speed_and_keeps_the_shell_clock() {
    let dir = corpus();
    let mut preview = world(dir.path(), EnginePreset::Vanilla);
    preview.set_game_speed(GameSpeed::Faster);
    assert_eq!(preview.game_speed(), GameSpeed::Normal);
    assert_eq!(message(&preview), None);
    preview.init_game_speed(GameSpeed::Faster);
    assert_eq!(preview.game_speed(), GameSpeed::Normal);
    for now in [0, 16, 500, 2000] {
        preview.tick(now).unwrap();
        assert_eq!(preview.host.world.elapsed_ms, now);
    }
}

/// The NPC injured (one HP below its maximum) with combat under way on the
/// player's turn (set after the last tick, as the interface tests do), and
/// the frame composed.
fn injured_in_combat(preview: &mut MapPreview) -> ObjectRef {
    preview.tick(16).unwrap();
    let critter = npc(preview);
    let world = &mut preview.host.world;
    let max = world
        .get(critter)
        .unwrap()
        .character
        .as_ref()
        .unwrap()
        .stat(7, world.context(critter));
    assert!(max > 1, "the fixture critter has hit points");
    world.critter_state_mut(critter).unwrap().hp = max - 1;
    world.combat = true;
    world.combat_state.player_turn = true;
    preview.image().unwrap();
    critter
}

#[test]
fn the_overlay_marks_injured_critters_in_combat_only() {
    let dir = corpus();
    let mut preview = world(dir.path(), EnginePreset::Enhanced);
    preview.tick(0).unwrap();
    let critter = npc(&preview);
    let view = preview.view;
    // Out of combat, or uninjured: nothing.
    preview.image().unwrap();
    assert!(!preview.tactical_shown());
    assert!(preview.injured_critters(view).is_empty(), "nobody is hurt");
    let injured = injured_in_combat(&mut preview);
    assert_eq!(injured, critter);
    assert!(preview.tactical_shown());
    // The injured critter only: the player is at full HP.
    let marked: Vec<ObjectRef> = preview
        .injured_critters(view)
        .into_iter()
        .map(|(critter, _, _)| critter)
        .collect();
    assert_eq!(marked, [critter]);
    let [hp, max] = preview.injured_critters(view)[0].2;
    assert_eq!(hp, max - 1);
    // The bar draws above the critter's sprite: the frame differs from
    // the one with the overlay off (Alt+H), only there.
    let on = preview.image().unwrap();
    preview.set_modifiers([false, false, true]);
    assert!(preview.step(KeyCode::KeyH));
    assert!(!preview.qol_state.overlay);
    assert_eq!(message(&preview), Some(b"Tactical overlay off".as_slice()));
    preview.qol_state.message = None;
    let off = preview.image().unwrap();
    let (_, [cx, top], _) = preview.injured_critters(view)[0];
    let changed: Vec<usize> = (0..on.pixels().len())
        .filter(|at| on.pixels()[*at] != off.pixels()[*at])
        .collect();
    assert!(!changed.is_empty(), "the bar draws");
    for at in changed {
        let [x, y] = [(at % 640) as i64, (at / 640) as i64];
        assert!((x - cx).abs() <= 13 && y < top && y >= top - 6, "{x},{y}");
    }
    // Alt+H again turns it back on.
    assert!(preview.step(KeyCode::KeyH));
    assert!(preview.qol_state.overlay);
    preview.qol_state.message = None;
    assert_eq!(preview.image().unwrap().pixels(), on.pixels());
    // A healed critter has no bar; nor has a dead one.
    preview.set_modifiers([false; 3]);
    let world = &mut preview.host.world;
    world.critter_state_mut(critter).unwrap().hp = 0;
    world.critter_state_mut(critter).unwrap().results.0 |= vault_core::critical::effect::DEAD;
    assert!(preview.injured_critters(view).is_empty());
    // Combat over: no overlay at all.
    preview.host.world.combat = false;
    assert!(!preview.tactical_shown());
}

#[test]
fn the_overlay_to_hit_is_the_vanilla_hover_chance_at_once() {
    let dir = corpus();
    // The text's font (the synthetic corpus has none).
    std::fs::write(
        dir.path().join("font1.aaf"),
        super::super::state_fixture::preview_aaf(),
    )
    .unwrap();
    let mut preview = world(dir.path(), EnginePreset::Enhanced);
    preview.tick(0).unwrap();
    let critter = injured_in_combat(&mut preview);
    preview.hud.mode = vault_core::ui::cursor::Mode::Crosshair;
    preview.hover(NPC_PIXEL);
    // At once, before the hover's 250 ms rest.
    let Some((target, chance)) = preview.tactical_hit() else {
        panic!("the crosshair is over the critter");
    };
    assert_eq!(target, critter);
    assert_eq!(chance, preview.hover_hit_chance(critter));
    let before = preview.image().unwrap();
    // The Vanilla hover (after its rest) shows the same number; the
    // overlay's copy then gives way to it.
    let vanilla = preview.hover_feedback(NPC_PIXEL);
    let Some(Feedback::Hit(text, _)) = vanilla.clone() else {
        panic!("the crosshair hover's to-hit text");
    };
    assert_eq!(text, vault_core::ui::cursor::hit_text(chance));
    preview.pointer.feedback = vanilla;
    let rested = preview.image().unwrap();
    assert_ne!(before.pixels(), rested.pixels());
    // Without the overlay the frame before the rest has no number.
    preview.pointer.feedback = None;
    preview.qol_state.overlay = false;
    assert_ne!(preview.image().unwrap().pixels(), before.pixels());
    preview.qol_state.overlay = true;
    // Off the critter: nothing.
    preview.hover([20, 20]);
    assert!(preview.tactical_hit().is_none());
}

#[test]
fn vanilla_draws_no_overlay_and_alt_h_toggles_nothing() {
    let dir = corpus();
    let mut preview = world(dir.path(), EnginePreset::Vanilla);
    preview.tick(0).unwrap();
    injured_in_combat(&mut preview);
    assert!(!preview.tactical_shown());
    preview.hud.mode = vault_core::ui::cursor::Mode::Crosshair;
    preview.hover(NPC_PIXEL);
    let before = preview.image().unwrap();
    preview.set_modifiers([false, false, true]);
    preview.step(KeyCode::KeyH);
    assert!(preview.qol_state.overlay, "untouched");
    assert_eq!(message(&preview), None);
    assert_eq!(preview.image().unwrap().pixels(), before.pixels());
}

#[test]
fn the_hud_message_shows_dissolves_and_goes() {
    let dir = corpus();
    let mut preview = world(dir.path(), EnginePreset::Enhanced);
    preview.tick(0).unwrap();
    let plain = preview.image().unwrap();
    preview.post_message(b"Quick Saved.".to_vec(), false);
    let shown = preview.image().unwrap();
    assert_ne!(shown.pixels(), plain.pixels());
    // Whole until MESSAGE_MS, then dissolving: fewer of its pixels each
    // step, and gone after MESSAGE_FADE_MS.
    preview.tick(MESSAGE_MS - 16).unwrap();
    assert_eq!(preview.image().unwrap().pixels(), shown.pixels());
    let differs = |image: &IndexedImage| {
        image
            .pixels()
            .iter()
            .zip(plain.pixels())
            .filter(|(a, b)| a != b)
            .count()
    };
    assert!(preview.tick(MESSAGE_MS + MESSAGE_FADE_MS / 2).unwrap());
    let half = preview.image().unwrap();
    assert!(differs(&half) > 0 && differs(&half) < differs(&shown));
    assert!(preview.tick(MESSAGE_MS + MESSAGE_FADE_MS).unwrap());
    assert_eq!(message(&preview), None);
    assert_eq!(preview.image().unwrap().pixels(), plain.pixels());
}

/// A file in numbered slot 1, which no quick save may touch.
fn numbered_slot(preview: &MapPreview) -> std::path::PathBuf {
    let slot = slot_directory(&preview.save_root, 1).unwrap();
    std::fs::create_dir_all(&slot).unwrap();
    let marker = slot.join("MARKER.TXT");
    std::fs::write(&marker, b"mine").unwrap();
    marker
}

/// F6 then F7 under the enhanced preset: the quick slot round-trips the
/// game, the numbered slots stay as they were, and the screen lists none
/// of it.
fn quick_round_trip(preview: &mut MapPreview) {
    let marker = numbered_slot(preview);
    let quick = quick_slot_directory(&preview.save_root);
    preview.tick(16).unwrap();
    preview.image().unwrap();
    // Nothing to load yet.
    assert!(preview.step(KeyCode::F7));
    assert_eq!(message(preview), Some(NO_QUICK_SAVE));
    assert!(matches!(preview.screen, Screen::Map));
    // F6 saves at once: no pick screen, no description box.
    let ticks = preview.host.world.clock.ticks;
    assert!(preview.step(KeyCode::F6));
    assert!(matches!(preview.screen, Screen::Map));
    assert!(preview.dialog.is_none());
    assert_eq!(
        message(preview),
        Some(QUICK_SAVED),
        "{}",
        preview.status.escape_ascii()
    );
    assert!(quick.join("SAVE.DAT").is_file());
    assert!(quick.join("AUTOMAP.SAV").is_file());
    // The numbered slots: untouched, and the screen lists no quick slot.
    assert_eq!(std::fs::read(&marker).unwrap(), b"mine");
    let slot = marker.parent().unwrap();
    assert_eq!(std::fs::read_dir(slot).unwrap().count(), 1);
    assert!(
        preview
            .slot_states()
            .iter()
            .all(|state| matches!(state, SlotState::Empty | SlotState::Corrupt))
    );
    // F7 loads it back at once.
    preview.host.world.clock.ticks += 5000;
    assert!(preview.step(KeyCode::F7));
    assert!(matches!(preview.screen, Screen::Map));
    assert_eq!(message(preview), Some(QUICK_LOADED));
    assert_eq!(preview.host.world.clock.ticks, ticks);
    // A second quick save replaces the first in the same slot.
    preview.host.world.clock.ticks += 7;
    assert!(preview.step(KeyCode::F6));
    preview.host.world.clock.ticks += 9;
    assert!(preview.step(KeyCode::F7));
    assert_eq!(preview.host.world.clock.ticks, ticks + 7);
    assert_eq!(std::fs::read(&marker).unwrap(), b"mine");
}

#[test]
fn enhanced_f6_and_f7_round_trip_the_quick_slot_under_fallout1() {
    let dir = corpus();
    let mut preview = world(dir.path(), EnginePreset::Enhanced);
    quick_round_trip(&mut preview);
    // An enhanced save with settings to keep writes the sidecar there too.
    let critter = npc(&preview);
    let id = preview.host.world.get(critter).unwrap().raw.id;
    preview
        .host
        .world
        .companion_distance
        .insert(id, vault_core::companion::Distance::Hold);
    assert!(preview.step(KeyCode::F6));
    let quick = quick_slot_directory(&preview.save_root);
    assert!(quick.join(vault_core::companion::SIDECAR).is_file());
    preview.host.world.companion_distance.clear();
    assert!(preview.step(KeyCode::F7));
    assert_eq!(
        preview.host.world.companion_distance.get(&id),
        Some(&vault_core::companion::Distance::Hold)
    );
}

#[test]
fn a_failed_quick_save_says_so() {
    let dir = corpus();
    let mut preview = world(dir.path(), EnginePreset::Enhanced);
    // A save root that is a file cannot hold the slot.
    let file = dir.path().join("not-a-directory");
    std::fs::write(&file, b"x").unwrap();
    preview.save_root = file;
    preview.tick(16).unwrap();
    assert!(preview.step(KeyCode::F6));
    assert_eq!(message(&preview), Some(QUICK_SAVE_FAILED));
    assert!(preview.qol_state.message.as_ref().unwrap().error);
    assert!(preview.dialog.is_none());
    // A quick slot that does not load says so too, and changes nothing.
    preview.save_root = dir.path().join("saves");
    let quick = quick_slot_directory(&preview.save_root);
    std::fs::create_dir_all(&quick).unwrap();
    std::fs::write(quick.join("SAVE.DAT"), b"short").unwrap();
    let ticks = preview.host.world.clock.ticks;
    assert!(preview.step(KeyCode::F7));
    assert_eq!(message(&preview), Some(QUICK_LOAD_FAILED));
    assert_eq!(preview.host.world.clock.ticks, ticks);
    assert!(matches!(preview.screen, Screen::Map));
}

#[test]
fn vanilla_f6_and_f7_keep_the_spec014_pick_screen() {
    let dir = corpus();
    let mut preview = world(dir.path(), EnginePreset::Vanilla);
    preview.tick(16).unwrap();
    preview.image().unwrap();
    assert!(preview.step(KeyCode::F6));
    assert!(
        matches!(&preview.screen, Screen::SaveLoad(screen) if screen.mode == SaveMode::PickQuickSave)
    );
    assert!(preview.step(KeyCode::Escape));
    assert!(preview.step(KeyCode::F7));
    assert!(
        matches!(&preview.screen, Screen::SaveLoad(screen) if screen.mode == SaveMode::PickQuickLoad)
    );
    assert!(preview.step(KeyCode::Escape));
    assert_eq!(message(&preview), None);
    assert!(!quick_slot_directory(&preview.save_root).exists());
}

#[test]
fn enhanced_f6_and_f7_round_trip_the_quick_slot_under_fallout2() {
    let (_install, vfs) = super::super::fallout2_combat_tests::synthetic_install(
        super::super::fallout2_combat_tests::passive_script(),
    );
    let mut preview = super::super::fallout2_combat_tests::new_game_with(
        vfs,
        "premade/synthetic.gcd",
        EnginePreset::Enhanced,
    );
    assert!(preview.host.world.fallout2.is_some());
    let saves = tempfile::tempdir().unwrap();
    preview.save_root = saves.path().to_path_buf();
    quick_round_trip(&mut preview);
    // The slot holds Fallout 2's layout (SPEC-018 §7).
    let bytes = std::fs::read(quick_slot_directory(saves.path()).join("SAVE.DAT")).unwrap();
    let header = &bytes[..vault_formats::savegame::HEADER_LEN];
    assert!(
        vault_formats::savegame::SaveHeader::parse_for(header, vault_core::GameProfile::Fallout2)
            .is_ok()
    );
}

#[test]
fn the_overlay_scale_keeps_the_marks_readable_zoomed_out() {
    assert_eq!(overlay_scale(3.0), 1);
    assert_eq!(overlay_scale(1.0), 1);
    assert_eq!(overlay_scale(0.75), 1);
    assert_eq!(overlay_scale(0.6), 2);
    assert_eq!(overlay_scale(0.5), 2);
}
