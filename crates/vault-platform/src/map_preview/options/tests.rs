//! M-I6 preferences (SPEC-014 §3.17-§3.18, §3.46), and the H12 worn-armor
//! guard and elevator keypad over the synthetic corpus.
use super::{AppliedPreferences, OptionsScreen, set_slider};
use crate::map_preview::{
    MapPreview, Screen, fixture,
    test_support::{interactive, synthetic_corpus},
};
use std::sync::Arc;
use vault_core::{
    WorldEffect,
    ui::options::{self, DEFAULT_VOLUME, OptionsAction, Preferences},
};
use vault_formats::pro::BasicFlags;
use winit::keyboard::KeyCode;

fn preview(root: &std::path::Path) -> MapPreview {
    let mut preview = interactive(root);
    preview.save_root = root.join("saves");
    preview
}

#[test]
fn m_i6_the_line_delay_follows_the_base_delay_and_the_hooks_apply_it() {
    // §3.18: line delay (base - 1) / 5 x 2, clamped to 0-2.
    let mut values = Preferences::default();
    set_slider(&mut values, 1, 6.0);
    assert_eq!((values.text_base_delay, values.text_line_delay), (6.0, 2.0));
    set_slider(&mut values, 1, 1.0);
    assert_eq!(values.text_line_delay, 0.0);
    // §3.46 lifetimes: 3500 + 1000 per line at the default 3.5; 1000 flat
    // at 1.0; 6000 + 2000 per line at 6.0.
    let applied = AppliedPreferences::of(&Preferences::default());
    assert_eq!(
        (applied.floating_base_ms, applied.floating_line_ms),
        (3500, 1000)
    );
    assert_eq!(applied.floating_lifetime_ms(2), 5500);
    assert!(applied.verbose_combat_messages);
    assert_eq!(applied.mouse_sensitivity, 1.0);
    let brief = AppliedPreferences::of(&Preferences {
        text_base_delay: 1.0,
        combat_messages: 0,
        mouse_sensitivity: 2.5,
        ..Default::default()
    });
    assert_eq!((brief.floating_base_ms, brief.floating_line_ms), (1000, 0));
    assert!(!brief.verbose_combat_messages);
    assert_eq!(brief.mouse_sensitivity, 2.5);
    let slow = AppliedPreferences::of(&Preferences {
        text_base_delay: 6.0,
        ..Default::default()
    });
    assert_eq!((slow.floating_base_ms, slow.floating_line_ms), (6000, 2000));
    // Storing derives the line delay (the stale 1.399994 is never kept).
    let dir = synthetic_corpus();
    let mut preview = preview(dir.path());
    preview.preferences.text_base_delay = 3.5;
    preview.preferences.text_line_delay = 1.399_994;
    preview.store_preferences();
    assert_eq!(
        preview.preferences.text_line_delay,
        options::line_delay(3.5)
    );
    assert_eq!(preview.applied_preferences().floating_line_ms, 1000);
}

#[test]
fn m_i6_default_applies_at_once_and_cancel_restores_the_backup() {
    let dir = synthetic_corpus();
    let mut preview = preview(dir.path());
    preview.preferences.master_volume = 100;
    preview.preferences.mouse_sensitivity = 2.0;
    preview.preferences.text_base_delay = 6.0;
    preview.preferences.game_difficulty = 2;
    preview.open_options();
    preview.options_action(OptionsAction::Preferences);
    // DEFAULT, the red button at (23, 450).
    assert!(preview.options_click([25, 452]));
    assert_eq!(preview.preferences.master_volume, DEFAULT_VOLUME);
    assert_eq!(preview.preferences.mouse_sensitivity, 1.0);
    assert_eq!(
        (
            preview.preferences.text_base_delay,
            preview.preferences.text_line_delay
        ),
        (3.5, 1.0)
    );
    let Screen::Options(OptionsScreen::Preferences(state)) = &preview.screen else {
        panic!("preferences stay open");
    };
    assert_eq!(state.values.game_difficulty, 1);
    // Cancel (263, 450) restores the values on opening.
    preview.release(Some([25, 452]), false);
    assert!(preview.options_click([265, 452]));
    assert_eq!(preview.preferences.master_volume, 100);
    assert_eq!(preview.preferences.mouse_sensitivity, 2.0);
    assert_eq!(preview.preferences.game_difficulty, 2);
    // DEFAULT then DONE (148, 450) keeps the defaults.
    preview.release(Some([265, 452]), false);
    preview.options_action(OptionsAction::Preferences);
    assert!(preview.options_click([25, 452]));
    preview.release(Some([25, 452]), false);
    assert!(preview.options_click([150, 452]));
    assert_eq!(preview.preferences.game_difficulty, 1);
    assert_eq!(preview.preferences.master_volume, DEFAULT_VOLUME);
}

#[test]
fn spec014_3_17_save_and_load_from_options_return_there_on_cancel() {
    let dir = synthetic_corpus();
    let mut preview = preview(dir.path());
    preview.open_options();
    preview.options_action(OptionsAction::SaveGame);
    assert!(matches!(&preview.screen, Screen::SaveLoad(screen) if screen.from_options));
    assert!(preview.step(KeyCode::Escape));
    assert!(matches!(
        preview.screen,
        Screen::Options(OptionsScreen::Menu)
    ));
    preview.options_action(OptionsAction::LoadGame);
    assert!(matches!(preview.screen, Screen::SaveLoad(_)));
    // The Cancel red button.
    let [x, y] = vault_core::ui::saveload::CANCEL.0;
    assert!(preview.click([x + 2, y + 2], false));
    preview.release(Some([x + 2, y + 2]), false);
    assert!(matches!(
        preview.screen,
        Screen::Options(OptionsScreen::Menu)
    ));
    // Opened from the map, Esc returns to the map.
    preview.options_action(OptionsAction::Done);
    preview.open_save_screen(vault_core::ui::saveload::Mode::Save);
    assert!(preview.step(KeyCode::Escape));
    assert!(matches!(preview.screen, Screen::Map));
}

#[test]
fn h12_a_quick_click_on_worn_armor_changes_nothing() {
    let dir = synthetic_corpus();
    let mut preview = preview(dir.path());
    let player = preview.host.world.player;
    // Armor class 5, damage resistance 10 and threshold 2 for normal damage.
    let mut bytes = fixture::prototype(0, 0);
    fixture::word(&mut bytes, 0, 0x30);
    fixture::word(&mut bytes, 0x39, 5);
    fixture::word(&mut bytes, 0x39 + 4, 10);
    fixture::word(&mut bytes, 0x39 + 32, 2);
    fixture::word(&mut bytes, 0x75, -1);
    fixture::word(&mut bytes, 0x79, -1);
    fixture::word(&mut bytes, 0x7d, -1);
    let prototype = Arc::new(vault_formats::Prototype::parse(&bytes).unwrap());
    let armor = preview.host.world.spawn(prototype, 0, 0).unwrap();
    preview.host.world.add_item(player, armor, 1, true).unwrap();
    preview
        .host
        .world
        .equip(player, armor, BasicFlags::WORN.0)
        .unwrap();
    let stats = |preview: &MapPreview| {
        let context = preview.host.world.context(player);
        let sheet = preview.host.world.player_sheet().unwrap();
        [9, 17, 24].map(|stat| sheet.stat(stat, context))
    };
    let worn = stats(&preview);
    assert!(preview.step(KeyCode::KeyI));
    preview.image().unwrap();
    // The armor slot is (234, 183) 90 by 61.
    for _ in 0..3 {
        assert!(preview.click([279, 213], false));
        assert!(preview.release(Some([279, 213]), false));
    }
    assert_eq!(stats(&preview), worn, "AC, DT and DR unchanged");
    assert!(
        preview
            .host
            .world
            .get(armor)
            .unwrap()
            .raw
            .flags
            .contains(BasicFlags::WORN)
    );
    assert!(preview.step(KeyCode::Escape));
}

#[test]
fn spec013_elevator_keys_accept_the_numeric_keypad() {
    let dir = synthetic_corpus();
    let mut preview = preview(dir.path());
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
    assert!(matches!(&preview.screen, Screen::Elevator(screen) if screen.travel.is_none()));
    assert!(preview.step(KeyCode::Numpad1));
    assert!(matches!(&preview.screen, Screen::Elevator(screen) if screen.travel.is_some()));
}
