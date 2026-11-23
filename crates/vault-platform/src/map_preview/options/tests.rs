//! M-I6 preferences (SPEC-014 §3.17-§3.18, §3.46), and the H12 worn-armor
//! guard and elevator keypad over the synthetic corpus.
use super::{
    AppliedPreferences, BADGE, ENHANCED_ENTRY, EnhancedRow, OptionsScreen, PanelHit, SaveOutcome,
    panel_hit, preset_rect,
};
use crate::{
    config::{Config, Preset},
    enhanced::{BuiltinPreset, EnhancedSettings, GameSpeed, RoofPeek, ViewportMode},
    map_preview::{
        MapPreview, Screen, fixture,
        test_support::{interactive, synthetic_corpus},
    },
    preview::PreviewArgs,
    replay::{Replay, interactive_world},
    session::{SessionConfig, SessionRuntime},
};
use std::{path::Path, sync::Arc};
use vault_core::{
    EnginePreset, WorldEffect,
    movie::MoviePolicy,
    ui::{
        Rect,
        options::{self, DEFAULT_VOLUME, OptionsAction, OptionsPage, Preferences, set_slider},
        red_button,
    },
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
        Screen::Options(OptionsScreen::Window(options::OptionsWindow {
            page: OptionsPage::Menu
        }))
    ));
    preview.options_action(OptionsAction::LoadGame);
    assert!(matches!(preview.screen, Screen::SaveLoad(_)));
    // The Cancel red button.
    let [x, y] = vault_core::ui::saveload::CANCEL.0;
    assert!(preview.click([x + 2, y + 2], false));
    preview.release(Some([x + 2, y + 2]), false);
    assert!(matches!(
        preview.screen,
        Screen::Options(OptionsScreen::Window(options::OptionsWindow {
            page: OptionsPage::Menu
        }))
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
    assert!(matches!(&preview.screen, Screen::Elevator(screen) if !screen.moving()));
    assert!(preview.step(KeyCode::Numpad1));
    assert!(matches!(&preview.screen, Screen::Elevator(screen) if screen.moving()));
}

/// The synthetic world under the `enhanced` preset, in the fixed viewport so
/// the frame's pixels are the Vanilla ones.
fn enhanced(root: &Path) -> MapPreview {
    let mut preview = preview(root);
    preview.host.set_preset(EnginePreset::Enhanced);
    preview.set_enhanced(Some(EnhancedSettings {
        viewport: ViewportMode::Fixed,
        ..Default::default()
    }));
    preview
}
fn center(rect: Rect) -> [i32; 2] {
    [rect.x + rect.w / 2, rect.y + rect.h / 2]
}
/// `preview` with the Enhanced window open, through the menu's button.
fn open_panel(preview: &mut MapPreview) {
    preview.open_options();
    assert!(preview.options_click(center(ENHANCED_ENTRY)));
    preview.release(Some(center(ENHANCED_ENTRY)), false);
    assert!(matches!(
        preview.screen,
        Screen::Options(OptionsScreen::Enhanced(_))
    ));
}
fn click_row(preview: &mut MapPreview, row: EnhancedRow) {
    assert_eq!(
        panel_hit(center(row.value_rect())),
        Some(PanelHit::Row(row))
    );
    assert!(preview.options_click(center(row.value_rect())));
}
fn note(preview: &MapPreview) -> Option<(String, bool)> {
    match &preview.screen {
        Screen::Options(OptionsScreen::Enhanced(panel)) => panel.note.clone(),
        _ => None,
    }
}
/// The frame's pixels inside `rect`.
fn region(image: &vault_render::IndexedImage, rect: Rect) -> Vec<u8> {
    let width = image.width() as usize;
    (rect.y..rect.y + rect.h)
        .flat_map(|row| {
            let start = row as usize * width + rect.x as usize;
            image.pixels()[start..start + rect.w as usize].to_vec()
        })
        .collect()
}

const SETTINGS: &str = "# My Fallout setup. Hands off.
game = \"fallout1\"
a_key_from_tomorrow = \"stay\"

[profiles.fallout1]
smooth_camera = true # the first profile's own note

[profiles.fallout2]
# Fallout 2 stays as I left it.
default_zoom = 2.5
";

fn settings_file(dir: &Path, text: &str) -> std::path::PathBuf {
    let path = dir.join("vault-engine.toml");
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn a_built_in_preset_applies_to_the_session_at_once() {
    let dir = synthetic_corpus();
    let mut preview = enhanced(dir.path());
    open_panel(&mut preview);
    for (index, preset) in BuiltinPreset::ALL.into_iter().enumerate() {
        assert_eq!(
            panel_hit(center(preset_rect(index))),
            Some(PanelHit::Preset(preset))
        );
        assert!(preview.options_click(center(preset_rect(index))));
        let want = preset.settings();
        let settings = preview.enhanced().unwrap();
        assert_eq!(
            want.viewport_mode.unwrap_or(settings.viewport),
            settings.viewport
        );
        assert_eq!(
            want.default_zoom.unwrap_or(settings.default_zoom),
            settings.default_zoom
        );
        assert_eq!(
            want.roof_peek.unwrap_or(settings.roof_peek),
            settings.roof_peek
        );
        assert_eq!(
            want.smooth_camera.unwrap_or(settings.smooth_camera),
            settings.smooth_camera
        );
        let speed = preview.session_game_speed();
        assert_eq!(want.game_speed.unwrap_or(speed), speed);
        let overlay = preview.qol_state.overlay;
        assert_eq!(want.tactical_overlay.unwrap_or(overlay), overlay);
        // The window lights the preset now in force, and still draws.
        let values = preview.enhanced_values();
        assert_eq!(values.with_profile(want), values);
        preview.image().unwrap();
    }
    // The rules a world loaded with are not a thing a preset can change.
    assert_eq!(preview.host.preset(), EnginePreset::Enhanced);
}

#[test]
fn each_setting_applies_to_the_session_at_once() {
    let dir = synthetic_corpus();
    let mut preview = enhanced(dir.path());
    open_panel(&mut preview);
    let world = preview.host.world.state_hash();

    click_row(&mut preview, EnhancedRow::Viewport);
    assert_eq!(preview.enhanced().unwrap().viewport, ViewportMode::Unlocked);
    click_row(&mut preview, EnhancedRow::Zoom);
    assert_eq!(preview.enhanced().unwrap().default_zoom, 1.5);
    let camera = &preview.enhanced.as_ref().unwrap().camera;
    assert_eq!((camera.default_zoom, camera.zoom_goal()), (1.5, 1.5));
    click_row(&mut preview, EnhancedRow::Smooth);
    assert!(!preview.enhanced.as_ref().unwrap().camera.smooth);
    click_row(&mut preview, EnhancedRow::Roof);
    assert_eq!(preview.enhanced().unwrap().roof_peek, RoofPeek::CursorXray);
    click_row(&mut preview, EnhancedRow::UiScale);
    assert_eq!(preview.enhanced().unwrap().ui_scale, Some(1.0));
    click_row(&mut preview, EnhancedRow::Speed);
    assert_eq!(preview.game_speed(), GameSpeed::Fast);
    click_row(&mut preview, EnhancedRow::Overlay);
    assert!(!preview.qol_state.overlay);
    assert_eq!(
        preview.host.world.state_hash(),
        world,
        "presentation settings leave the world's state alone"
    );
}

#[test]
fn the_window_says_what_cannot_change_mid_session() {
    let dir = synthetic_corpus();
    let mut preview = enhanced(dir.path());
    open_panel(&mut preview);
    let values = preview.enhanced_values();
    assert_eq!(values.tag(EnhancedRow::Rules).0, "NEXT START");
    assert_eq!(values.tag(EnhancedRow::Speed).0, "LIVE");
    // The camera's rows show nothing in the fixed viewport, and say so.
    assert_eq!(values.tag(EnhancedRow::Zoom).0, "NOT IN FIXED");
    click_row(&mut preview, EnhancedRow::Viewport);
    assert_eq!(preview.enhanced_values().tag(EnhancedRow::Zoom).0, "LIVE");

    click_row(&mut preview, EnhancedRow::Rules);
    assert_eq!(preview.enhanced_values().rules, Preset::Vanilla);
    assert_eq!(
        preview.host.preset(),
        EnginePreset::Enhanced,
        "not this session"
    );
    let file = dir.path().join("rules");
    std::fs::create_dir(&file).unwrap();
    let path = settings_file(&file, SETTINGS);
    assert!(matches!(
        preview.save_enhanced_to(Some(&path)),
        SaveOutcome::Saved { .. }
    ));
    assert_eq!(
        Config::load(&path).unwrap().enhanced.preset,
        Some(Preset::Vanilla)
    );
}

#[test]
fn each_badge_shows_its_setting() {
    let dir = synthetic_corpus();
    let mut preview = enhanced(dir.path());
    open_panel(&mut preview);
    for row in EnhancedRow::ALL {
        let rect = row.badge_rect();
        assert!(rect.x + rect.w <= super::PANEL.x + super::PANEL.w);
        assert_eq!((rect.w, rect.h), (BADGE.w, BADGE.h));
        let before = region(&preview.image().unwrap(), rect);
        click_row(&mut preview, row);
        let after = region(&preview.image().unwrap(), rect);
        assert_ne!(before, after, "{row:?}");
    }
}

#[test]
fn a_save_keeps_the_other_profiles_and_the_comments_byte_for_byte() {
    let dir = synthetic_corpus();
    let path = settings_file(dir.path(), SETTINGS);
    let mut preview = enhanced(dir.path());
    open_panel(&mut preview);
    click_row(&mut preview, EnhancedRow::Speed);
    click_row(&mut preview, EnhancedRow::Overlay);
    assert_eq!(
        preview.save_enhanced_to(Some(&path)),
        SaveOutcome::Saved {
            profile: "fallout1".into()
        }
    );
    let written = std::fs::read_to_string(&path).unwrap();
    let head = "# My Fallout setup. Hands off.\ngame = \"fallout1\"\n\
                a_key_from_tomorrow = \"stay\"\n\n[profiles.fallout1]\n";
    assert!(written.starts_with(head), "{written}");
    assert!(
        written.ends_with(
            "\n[profiles.fallout2]\n# Fallout 2 stays as I left it.\ndefault_zoom = 2.5\n"
        ),
        "{written}"
    );
    assert!(
        written.contains("# the first profile's own note"),
        "{written}"
    );
    let read = Config::load(&path).unwrap();
    assert_eq!(read.enhanced.game_speed, Some(GameSpeed::Fast));
    assert_eq!(read.enhanced.tactical_overlay, Some(false));
    assert_eq!(read.enhanced.preset, None, "no rules were chosen");
    assert_eq!(
        read.scope(vault_core::GameProfile::Fallout2)
            .enhanced
            .default_zoom,
        Some(2.5)
    );
    let (text, failed) = note(&preview).unwrap();
    assert_eq!(
        (text.as_str(), failed),
        ("Saved to the fallout1 profile.", false)
    );
}

#[test]
fn a_replay_session_writes_nothing() {
    let dir = synthetic_corpus();
    let path = settings_file(dir.path(), SETTINGS);
    let args = PreviewArgs {
        corpus_dir: Some(dir.path().into()),
        preview_map: Some("maps/synthetic.map".into()),
        player_gcd: Some("premade/synthetic.gcd".into()),
        preset: EnginePreset::Enhanced,
        enhanced: Some(EnhancedSettings {
            viewport: ViewportMode::Fixed,
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut session = SessionRuntime::start(SessionConfig::replay(args))
        .unwrap()
        .unwrap();
    let world = interactive_world(&mut session).unwrap();
    open_panel(world);
    click_row(world, EnhancedRow::Speed);
    assert_eq!(world.save_enhanced_to(Some(&path)), SaveOutcome::Headless);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), SETTINGS);
    assert!(note(world).unwrap().1, "the window says so");

    // Any headless world, by the same mark.
    let mut headless = enhanced(dir.path());
    headless.host.movie_policy = MoviePolicy::CompleteImmediately;
    assert_eq!(
        headless.save_enhanced_to(Some(&path)),
        SaveOutcome::Headless
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), SETTINGS);
}

#[test]
fn a_logged_click_on_the_window_replays_as_the_desktop_ran_it() {
    /// The fixed frame's pixel in the logged 960 by 640 surface.
    fn press(id: &mut u32, [x, y]: [i32; 2]) -> String {
        let [x, y] = [x + 160, y + 80];
        let lines = format!(
            r#"{{"command_id":{},"kind":"pointer_moved","x":{x}.5,"y":{y}.5}}
{{"command_id":{},"kind":"pointer_button","button":"Left","pressed":true}}
{{"command_id":{},"kind":"pointer_button","button":"Left","pressed":false}}
"#,
            *id,
            *id + 1,
            *id + 2
        );
        *id += 3;
        lines
    }
    let dir = synthetic_corpus();
    let run = |log: &str| {
        let args = PreviewArgs {
            corpus_dir: Some(dir.path().into()),
            preview_map: Some("maps/synthetic.map".into()),
            player_gcd: Some("premade/synthetic.gcd".into()),
            preset: EnginePreset::Enhanced,
            enhanced: Some(EnhancedSettings {
                viewport: ViewportMode::Fixed,
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut session = SessionRuntime::start(SessionConfig::replay(args))
            .unwrap()
            .unwrap();
        let world = interactive_world(&mut session).unwrap();
        world.open_options();
        let mut replay = Replay::new(session).unwrap();
        replay.run(log.as_bytes()).unwrap();
        replay.finish(None).unwrap().state_sha256
    };
    let mut id = 1;
    let mut opened = press(&mut id, center(ENHANCED_ENTRY));
    let plain = run(&opened);
    opened += &press(&mut id, center(EnhancedRow::Speed.value_rect()));
    let changed = run(&opened);
    assert_ne!(plain, changed, "the click reached the session");
    assert_eq!(changed, run(&opened), "and it replays alike");
}

#[test]
fn the_vanilla_options_window_is_unchanged() {
    let dir = synthetic_corpus();
    // The synthetic corpus has no menu art: the captions' font shows the frame.
    let font = crate::map_preview::state_fixture::preview_aaf();
    std::fs::write(dir.path().join("font3.aaf"), font).unwrap();
    let mut preview = preview(dir.path());
    preview.open_options();
    let vanilla = preview.image().unwrap();
    // The button is no target: not a press, not a key, and not a pixel.
    assert!(!preview.enhanced_entry_shown());
    assert!(preview.options_click(center(ENHANCED_ENTRY)));
    assert!(!preview.step(KeyCode::KeyH));
    assert!(matches!(
        preview.screen,
        Screen::Options(OptionsScreen::Window(options::OptionsWindow {
            page: OptionsPage::Menu
        }))
    ));
    assert_eq!(preview.image().unwrap().pixels(), vanilla.pixels());

    // The Enhanced button is the only change the preset makes to the frame.
    preview.host.set_preset(EnginePreset::Enhanced);
    preview.set_enhanced(Some(EnhancedSettings {
        viewport: ViewportMode::Fixed,
        ..Default::default()
    }));
    assert!(preview.enhanced_entry_shown());
    let enhanced = preview.image().unwrap();
    assert_ne!(
        region(&enhanced, ENHANCED_ENTRY),
        region(&vanilla, ENHANCED_ENTRY)
    );
    let width = vanilla.width() as i32;
    for (index, (old, new)) in vanilla.pixels().iter().zip(enhanced.pixels()).enumerate() {
        let point = [index as i32 % width, index as i32 / width];
        assert!(old == new || ENHANCED_ENTRY.contains(point), "{point:?}");
    }
    // Vanilla's touch camera has an enhanced view, and still no window.
    preview.host.set_preset(EnginePreset::Vanilla);
    assert!(!preview.enhanced_entry_shown());
}

#[test]
fn a_failed_save_is_reported_and_the_session_goes_on() {
    let dir = synthetic_corpus();
    let mut preview = enhanced(dir.path());
    open_panel(&mut preview);
    click_row(&mut preview, EnhancedRow::Speed);

    assert_eq!(preview.save_enhanced_to(None), SaveOutcome::NoFile);
    let (text, failed) = note(&preview).unwrap();
    assert!(failed && text.contains("vault-engine.toml"), "{text}");

    // A path no file can be read from.
    let folder = dir.path().join("a-folder");
    std::fs::create_dir(&folder).unwrap();
    assert!(matches!(
        preview.save_enhanced_to(Some(&folder)),
        SaveOutcome::Failed(_)
    ));
    assert!(note(&preview).unwrap().1);

    // A file that reads and cannot be replaced.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        let path = settings_file(&locked, SETTINGS);
        let mode = |bits| std::fs::Permissions::from_mode(bits);
        std::fs::set_permissions(&locked, mode(0o555)).unwrap();
        // A superuser's write goes through: that run proves nothing here.
        let writable = std::fs::write(locked.join("probe"), b"").is_ok();
        let outcome = preview.save_enhanced_to(Some(&path));
        std::fs::set_permissions(&locked, mode(0o755)).unwrap();
        if !writable {
            assert!(matches!(outcome, SaveOutcome::Failed(_)), "{outcome:?}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), SETTINGS);
            assert!(note(&preview).unwrap().1);
            assert!(!preview.status.is_empty());
        }
    }
    preview.image().unwrap();
    assert_eq!(preview.session_game_speed(), GameSpeed::Fast);
    assert!(matches!(
        preview.screen,
        Screen::Options(OptionsScreen::Enhanced(_))
    ));
}

#[test]
fn a_file_running_another_profile_is_not_clobbered() {
    let dir = synthetic_corpus();
    // A `--game-profile fallout1` start over a file whose active game is
    // Fallout 2: the save would land in Fallout 2's table.
    let text = SETTINGS.replace("game = \"fallout1\"", "game = \"fallout2\"");
    let path = settings_file(dir.path(), &text);
    let mut preview = enhanced(dir.path());
    open_panel(&mut preview);
    click_row(&mut preview, EnhancedRow::Speed);
    assert_eq!(
        preview.save_enhanced_to(Some(&path)),
        SaveOutcome::OtherProfile {
            file: "fallout2".into(),
            session: "fallout1".into()
        }
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    let (message, failed) = note(&preview).unwrap();
    assert!(failed && message.contains("fallout2"), "{message}");
}

#[test]
fn the_window_closes_to_the_menu_and_keeps_what_it_applied() {
    let dir = synthetic_corpus();
    let mut preview = enhanced(dir.path());
    open_panel(&mut preview);
    click_row(&mut preview, EnhancedRow::Speed);
    assert!(preview.step(KeyCode::Escape));
    assert!(preview.enhanced_entry_shown());
    assert_eq!(preview.session_game_speed(), GameSpeed::Fast);
    assert!(preview.step(KeyCode::KeyH), "the menu's key opens it again");
    assert!(preview.options_click(center(red_button(super::DONE_AT))));
    assert!(preview.enhanced_entry_shown());
    // SPEC-014 §3.17's keys keep their meaning: E is still Exit Game.
    assert!(preview.step(KeyCode::KeyE));
    assert!(matches!(
        preview.screen,
        Screen::Options(OptionsScreen::Window(options::OptionsWindow {
            page: OptionsPage::Quit
        }))
    ));
}
