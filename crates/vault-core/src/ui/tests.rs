//! SPEC-014 §4 fixtures and the rules behind them.
use super::{
    bar, cursor,
    editor::{self, Editor, NameField, Refusal},
    endgame, flow, options, saveload,
};
use crate::stats::{StatContext, Trait};
use vault_formats::credits;

#[test]
fn spec014_fixture_a_splash_names_and_stored_value() {
    let tried = std::cell::RefCell::new(Vec::new());
    let shown = flow::choose_splash(9, |path| {
        tried.borrow_mut().push(path.to_owned());
        path == "art/splash/splash0.rix"
    });
    assert_eq!(shown, Some(("art/splash/splash0.rix".into(), 1)));
    assert_eq!(
        *tried.borrow(),
        ["art/splash/splash9.rix", "art/splash/splash0.rix"]
    );
    // Ten misses show nothing.
    assert_eq!(flow::choose_splash(0, |_| false), None);
    // The stored value is not wrapped.
    assert_eq!(
        flow::choose_splash(9, |p| p.ends_with("splash9.rix"))
            .unwrap()
            .1,
        10
    );
}

#[test]
fn spec014_fixture_b_layout_arithmetic() {
    let button = flow::MenuButton::Credits;
    assert_eq!(button.rect(), super::Rect::new(425, 168, 26, 26));
    assert_eq!(button.label_origin(60), [490, 169]);
    // Save/load list clicks at y = 112, 113, 121.
    let reference: Vec<_> = [112, 113, 121].map(saveload::reference_slot_at).into();
    let safe: Vec<_> = [112, 113, 121].map(saveload::slot_at).into();
    assert_eq!((reference, safe), (vec![0, 1, 1], vec![0, 0, 1]));
    // Thumbnail columns and rows.
    assert_eq!(saveload::thumbnail_source(0), Some(2));
    assert_eq!(saveload::thumbnail_source(1), Some(5));
    assert_eq!(saveload::thumbnail_source(222), Some(637));
    assert_eq!(saveload::thumbnail_source(223), None);
    assert_eq!(saveload::thumbnail_source(131), Some(377));
    let info = saveload::SlotInfo {
        game_date: [12, 5, 2161],
        game_ticks: 264_600,
        ..Default::default()
    };
    assert_eq!(saveload::date_line(&info, b"DEC"), b"05 DEC 2161   0721");
    // HP bands at maximum 50: thresholds 12 and 25.
    assert_eq!(
        [11, 12, 24, 25].map(|hp| bar::hp_band(hp, 50)),
        [
            bar::Band::Red,
            bar::Band::Yellow,
            bar::Band::Yellow,
            bar::Band::White
        ]
    );
    // AP 13 on the player's turn: 10 green pips at 316 through 397.
    assert_eq!(bar::pips(Some(13), true), [Some(bar::Pip::Green); 10]);
    assert_eq!((bar::pip_x(0), bar::pip_x(9)), (316, 397));
    assert_eq!(options::delay_position(3.5), 493);
    assert_eq!(options::slider_position(22281.0, 0.0, 32767.0), 532);
    let mut brightness = options::BRIGHTNESS_MIN;
    for _ in 0..16 {
        brightness = options::brightness_step(brightness, true);
    }
    assert_eq!(brightness, options::BRIGHTNESS_MAX);
    assert_eq!(options::brightness_step(brightness, true), brightness);
    // Indicator boxes CHEM, SNEAK, LEVEL.
    let boxes = bar::indicators(true, true, true);
    assert_eq!(
        boxes,
        [
            bar::Indicator::Chem,
            bar::Indicator::Sneak,
            bar::Indicator::Level
        ]
    );
    assert_eq!([0, 1, 2].map(bar::indicator_x), [0, 124, 251]);
    assert_eq!(bar::indicator_window_width(boxes.len()), 381);
    assert_eq!(cursor::pick_art(572, 100).0, "actarrom.frm");
    assert_eq!(cursor::pick_art(571, 100).0, "actpick.frm");
    assert_eq!(cursor::menu_shift(300, 5), 120);
}

#[test]
fn spec014_fixture_c_editor_rules() {
    let context = StatContext::default();
    let mut editor = Editor::open(editor::blank_sheet(context), true, context);
    assert_eq!((editor.character_points, editor.tag_picks), (5, 3));
    for _ in 0..5 {
        assert!(editor.increment(0));
    }
    assert_eq!((editor.shown(0), editor.character_points), (10, 0));
    assert!(!editor.increment(0), "no points and shown 10");
    assert_eq!(
        editor.validate(),
        Err(Refusal {
            title: 142,
            body: 143
        })
    );
    for skill in [0, 9, 8] {
        editor.toggle_tag(skill).unwrap();
    }
    assert_eq!(
        editor.toggle_tag(1),
        Err(Refusal {
            title: 140,
            body: 141
        })
    );
    // Untagging shifts later tags down and returns the pick.
    editor.toggle_tag(9).unwrap();
    assert_eq!(editor.sheet.tags, [0, 8, -1, -1]);
    editor.toggle_tag(9).unwrap();
    let sheet = editor.done().unwrap();
    assert_eq!(sheet.name, b"None");
    assert_eq!(sheet.current_hp, sheet.stat(7, context));
    // Bruiser (+2 ST) with base 8 and 2 points left shows 10: plus refused.
    let mut editor = Editor::open(editor::blank_sheet(context), true, context);
    editor.toggle_trait(Trait::Bruiser as i32).unwrap();
    for _ in 0..3 {
        assert!(editor.increment(0));
    }
    assert_eq!(
        (
            editor.sheet.base[0],
            editor.character_points,
            editor.shown(0)
        ),
        (8, 2, 10)
    );
    assert!(!editor.increment(0));
    // A shown 11 from bonuses fails the upper-bound check.
    editor.sheet.bonus[0] = 1;
    editor.character_points = 0;
    editor.tag_picks = 0;
    assert_eq!(
        editor.validate(),
        Err(Refusal {
            title: 157,
            body: 158
        })
    );
    // Two traits at most.
    editor.toggle_trait(Trait::Finesse as i32).unwrap();
    assert_eq!(
        editor.toggle_trait(Trait::Gifted as i32),
        Err(Refusal {
            title: 148,
            body: 149
        })
    );
    assert_eq!(editor::age_step(35, true), 35);
    assert_eq!(editor::age_step(16, false), 16);
    // Cancel restores the sheet and (safe rule) the points.
    let (sheet, points) = editor.cancel();
    assert_eq!((sheet.base[0], points), (5, 5));
    // A premade opens with no picks; four tags never go negative, and
    // removing one of four leaves the first three full (Done passes).
    let mut premade = editor::blank_sheet(context);
    premade.tags = [0, 1, 2, 3];
    premade.remaining_creation = 0;
    let mut four = Editor::open(premade, false, context);
    assert_eq!(four.tag_picks, 0);
    four.toggle_tag(0).unwrap();
    assert_eq!((four.sheet.tags, four.tag_picks), ([1, 2, 3, -1], 0));
    assert_eq!(four.validate(), Ok(()));
}

#[test]
fn spec014_name_field_caption_and_info_regions() {
    let mut field = NameField::open(b"None");
    assert!(field.text.is_empty());
    for code in b"Abcdefghijklm" {
        field.key(*code);
    }
    assert_eq!(field.accept().unwrap(), b"Abcdefghijk");
    field.key(b'{');
    assert_eq!(field.text.len(), 11);
    assert_eq!(NameField::open(b"").accept(), None);
    // 10 pixels per character: "Vault Dweller Prime" overflows 100.
    let width = |text: &[u8]| 10 * text.len() as i32;
    assert_eq!(editor::caption_name(b"Short", width), b"Short");
    assert_eq!(
        editor::caption_name(b"Vault Dweller Prime", width),
        b"Vault "
    );
    assert_eq!(
        editor::caption_name(b"Abcdefghijklmno", width),
        b"Abcdefghijk"
    );
    assert_eq!(
        editor::info_hit([50, 40]),
        Some(editor::InfoLine::Special(0))
    );
    assert_eq!(
        editor::info_hit([50, 250]),
        Some(editor::InfoLine::Special(6))
    );
    assert_eq!(
        editor::info_hit([200, 70]),
        Some(editor::InfoLine::Condition(2))
    );
    assert_eq!(
        editor::info_hit([400, 27]),
        Some(editor::InfoLine::Skill(0))
    );
    assert_eq!(
        editor::info_hit([400, 221]),
        Some(editor::InfoLine::Skill(17))
    );
    assert_eq!(
        editor::info_hit([200, 148]),
        Some(editor::InfoLine::Condition(7))
    );
    assert_eq!(
        editor::info_hit([200, 307]),
        Some(editor::InfoLine::Derived(9))
    );
    assert_eq!(
        editor::info_hit([180, 354]),
        Some(editor::InfoLine::Trait(8))
    );
    assert_eq!(editor::big_number_cells(7, 7), [0, 7]);
    assert_eq!(editor::big_number_cells(100, 7), [9, 9]);
}

#[test]
fn spec014_fixture_d_endgame_slides() {
    let names = |globals: &[(usize, i32)]| {
        let mut table = vec![0; 700];
        for (index, value) in globals {
            table[*index] = *value;
        }
        endgame::slides(&table)
            .iter()
            .map(|slide| (slide.art, slide.narration))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(&[]),
        [
            ("dp", "nar_10"),
            ("seq4abc", "nar_19"),
            ("seq5b", "nar_25"),
            ("seq6a", "nar_28"),
            ("seq8abc", "nar_37"),
            ("dp", "nar_40"),
        ]
    );
    assert_eq!(
        names(&[
            (51, 1),
            (30, 1),
            (31, 2),
            (132, 1),
            (604, 1),
            (26, 1),
            (38, 1),
            (108, 2),
            (250, 1),
            (606, 1),
            (69, 2),
            (115, 6),
        ]),
        [
            ("dp", "nar_11"),
            ("seq2b", "nar_13"),
            ("seq3a", "nar_16"),
            ("seq4de", "nar_22"),
            ("seq6b", "nar_29"),
            ("seq7a", "nar_32"),
            ("seq8abc", "nar_36"),
            ("dp", "nar_40"),
        ]
    );
    let third = names(&[(104, 2), (69, 2), (114, 1), (115, 7)]);
    assert!(third.contains(&("seq5a", "nar_24")));
    assert!(third.contains(&("seq8abc", "nar_35")));
    let slides = endgame::slides(&[]);
    assert_eq!(slides[0].pan, Some(true));
    assert_eq!(slides.last().unwrap().pan, Some(false));
    assert_eq!(slides[1].art_path(), "art/intrface/seq4abc.frm");
    assert_eq!(slides[1].palette_path(), "art/intrface/seq4abc.pal");
    // Pan timing and fades.
    assert_eq!(endgame::pan_step_ms(None), 16);
    assert_eq!(
        endgame::pan_step_ms(Some(5)),
        16,
        "5000 ms does not exceed 5120"
    );
    assert_eq!(endgame::pan_step_ms(Some(20)), (20_000 + 16 * 320) / 640);
    assert_eq!(endgame::pan_scale(0), 0.0);
    assert_eq!(endgame::pan_scale(80), 0.5);
    assert_eq!(endgame::pan_scale(300), 1.0);
    assert_eq!(endgame::pan_scale(560), 0.5);
    assert_eq!(
        (
            endgame::pan_narration_offset(true),
            endgame::pan_narration_offset(false)
        ),
        (160, 480)
    );
    assert_eq!(endgame::subtitle_top(2), 450);
}

#[test]
fn spec014_fixture_e_credits_scroll_and_easter_egg() {
    let rows =
        credits::parse_credits(b";comment line\n@PRODUCER\nJane Example\n#Grey Name\n\n@DESIGN\n");
    let style = endgame::CreditStyle::Normal;
    assert_eq!(style.font(rows[0].kind), (103, super::ColorKey(13673)));
    assert_eq!(style.font(rows[1].kind), (104, super::ColorKey(18917)));
    assert_eq!(style.font(rows[2].kind), (104, endgame::GREY_NAME));
    assert_eq!(endgame::CreditStyle::Reversed.font(rows[0].kind).0, 104);
    let mut scroll = endgame::Credits::new(rows.clone(), |_| 100);
    assert_eq!(scroll.scrolling_ticks(), 130);
    // b, o, x ends the credits at x.
    assert!(!scroll.input(endgame::CreditInput::Char('b'), 3));
    assert!(!scroll.input(endgame::CreditInput::Char('o'), 3));
    assert!(scroll.input(endgame::CreditInput::Char('x'), 3));
    // b, o, o, m with violence above 0 starts the animation, which can be
    // triggered again.
    let mut scroll = endgame::Credits::new(rows.clone(), |_| 100);
    for c in "boom".chars() {
        assert!(!scroll.input(endgame::CreditInput::Char(c), 1));
    }
    assert_eq!(scroll.egg_frame(), Some(0));
    for _ in 0..4 {
        scroll.advance();
    }
    assert_eq!(scroll.egg_frame(), Some(2));
    assert!(scroll.input(endgame::CreditInput::Other, 1), "a click ends");
    // During the trailing blank ticks any input ends the credits.
    while scroll.tick < scroll.scrolling_ticks() {
        scroll.advance();
    }
    assert!(scroll.input(endgame::CreditInput::Char('b'), 1));
    while !scroll.finished() {
        scroll.advance();
    }
    assert_eq!(scroll.tick, 130 + 480);
    // Rows 640 pixels or wider are skipped.
    assert_eq!(
        endgame::Credits::new(rows, |row| if row.text.is_empty() { 0 } else { 640 })
            .rows
            .len(),
        1
    );
}

#[test]
fn spec014_fixture_f_monitor_wrapping() {
    let width = |text: &[u8]| text.len() as i32;
    let fits = vec![b'a'; 154];
    assert_eq!(bar::wrap_message(&fits, width).len(), 1);
    let mut long = vec![b'a'; 150];
    long.push(b' ');
    long.extend([b'b'; 4]);
    let lines = bar::wrap_message(&long, width);
    assert_eq!(lines.len(), 2, "a 155-pixel chunk breaks at its last space");
    assert_eq!(lines[0][0], bar::BULLET);
    assert_eq!(&lines[1], b"bbbb");
    // Seven one-line messages: six visible, oldest out of view.
    let mut monitor = bar::Monitor::default();
    for (i, message) in [b"m0", b"m1", b"m2", b"m3", b"m4", b"m5", b"m6"]
        .iter()
        .enumerate()
    {
        monitor.push(*message, width, i as u64 * 100, false);
    }
    let visible = monitor.visible();
    assert_eq!(visible.len(), 6);
    assert_eq!(&visible[0][1..], b"m1");
    assert_eq!(&visible[5][1..], b"m6");
    assert_eq!((bar::monitor_line_x(0), bar::monitor_line_x(5)), (23, 28));
    monitor.older();
    assert_eq!(&monitor.visible()[0][1..], b"m0");
    monitor.older();
    assert_eq!(
        &monitor.visible()[0][1..],
        b"m0",
        "stops at the oldest line"
    );
    monitor.newer();
    assert_eq!(&monitor.visible()[5][1..], b"m6");
    // Fewer than six lines sit at the bottom under empty rows.
    let mut early = bar::Monitor::default();
    early.push(b"first", width, 0, false);
    let rows = early.visible();
    assert_eq!(rows.len(), 6);
    assert!(rows[..5].iter().all(|row| row.is_empty()));
    assert_eq!(&rows[5][1..], b"first");
    // The monitor sound plays at most once per 500 ms, never in combat.
    let mut monitor = bar::Monitor::default();
    assert!(monitor.push(b"a", width, 1000, false));
    assert!(!monitor.push(b"b", width, 1400, false));
    assert!(monitor.push(b"c", width, 1500, false));
    assert!(!monitor.push(b"d", width, 9000, true));
}

#[test]
fn spec014_menu_loop_attract_dialogs_and_fades() {
    use flow::{MenuChoice, MenuInput, menu_poll};
    let poll = menu_poll(MenuInput::Key('N'), false, 0);
    assert_eq!(
        (poll.choice, poll.sounds),
        (Some(MenuChoice::NewGame), vec!["nmselec1"])
    );
    let credits = flow::MenuButton::Credits;
    assert_eq!(
        menu_poll(MenuInput::Release(credits), true, 0).choice,
        Some(MenuChoice::Quotes)
    );
    assert_eq!(
        menu_poll(MenuInput::Press(credits), false, 0).sounds,
        ["nmselec0"]
    );
    assert_eq!(
        menu_poll(MenuInput::Escape, false, 0).choice,
        Some(MenuChoice::Exit)
    );
    assert_eq!(
        menu_poll(MenuInput::Key('d'), false, 0).choice,
        Some(MenuChoice::Timeout)
    );
    assert_eq!(menu_poll(MenuInput::Key('='), false, 0).brightness, 1);
    assert_eq!(menu_poll(MenuInput::None, false, 119_999).choice, None);
    assert_eq!(
        menu_poll(MenuInput::None, false, 120_000).choice,
        Some(MenuChoice::Timeout)
    );
    // Safe rule: a selection on the timeout poll wins.
    assert_eq!(
        menu_poll(MenuInput::Key('l'), false, 200_000).choice,
        Some(MenuChoice::LoadGame)
    );
    // Attract: demo turn first; without demos every trigger plays the intro.
    let mut attract = flow::Attract::default();
    assert_eq!(attract.trigger(0), flow::AttractAction::Intro);
    assert_eq!(attract.trigger(0), flow::AttractAction::Intro);
    let mut attract = flow::Attract::default();
    assert_eq!(attract.trigger(2), flow::AttractAction::Demo(0));
    assert_eq!(attract.trigger(2), flow::AttractAction::Intro);
    assert_eq!(attract.trigger(2), flow::AttractAction::Demo(1));
    assert_eq!(flow::death_narration(1), "sound/speech/narrator/nar_3.acm");
    assert_eq!(flow::cycle(0, -1, 3), 2);
    assert_eq!(flow::portrait_origin(212, 187), [44, 53]);
    assert_eq!(flow::bio_line_y(21), Some(250));
    assert_eq!(flow::bio_line_y(22), None);
    assert_eq!(flow::level_word(0), 301);
    // Dialog boxes.
    assert_eq!(
        flow::dialog_size(true, Some(flow::DialogSize::Medium), 10, 1),
        flow::DialogSize::Large
    );
    assert_eq!(
        flow::dialog_size(false, None, 169, 2),
        flow::DialogSize::Large
    );
    assert_eq!(
        flow::dialog_size(false, None, 168, 5),
        flow::DialogSize::Medium
    );
    assert_eq!(flow::dialog_title_y(0), 52);
    assert_eq!(flow::dialog_title_y(2), 42);
    assert_eq!(flow::dialog_answer(flow::DialogKey::Char('y'), false), None);
    assert_eq!(
        flow::dialog_answer(flow::DialogKey::Char('Y'), true),
        Some(true)
    );
    // Fades: about 700 ms; step k of n; the target exactly at the end.
    assert_eq!(flow::fade_steps(0), 913);
    assert_eq!(flow::fade_steps(700), 60);
    // §3.2 the exact `trunc(42000 / M)`: 42000 / 7 = 6000 and 42000 / 701
    // = 59, and a measurement above 42000 ms makes every fade instant.
    assert_eq!(flow::fade_steps(7), 6000);
    assert_eq!(flow::fade_steps(701), 59);
    assert_eq!(flow::fade_steps(42_001), 0);
    // §3.13 the medium box lays out five lines: 23 + 25 - trunc(n * 10 / 2).
    assert_eq!(flow::DialogSize::Medium.title_y(0), 43);
    assert_eq!(flow::DialogSize::Medium.title_y(2), 33);
    assert_eq!(flow::DialogSize::Large.title_y(1), 47);
    assert_eq!(flow::fade_level(63, 0, 30, 60), 32);
    assert_eq!(flow::fade_level(10, 20, 1, 3), 13);
    assert_eq!(flow::fade_level(63, 0, 60, 60), 0);
}

#[test]
fn spec014_preferences_knobs_switches_and_sliders() {
    use vault_formats::cfg::ConfigFile;
    let defaults = options::Preferences::default();
    assert_eq!(
        options::Preferences::from_config(&ConfigFile::default()),
        defaults
    );
    let config = ConfigFile::parse(
        "[preferences]\ngame_difficulty=7\ncombat_looks=0\nbrightness=9\n[sound]\nmaster_volume=-4\n",
    );
    let read = options::Preferences::from_config(&config);
    assert_eq!(read.game_difficulty, 2, "clamped");
    assert_eq!(read.running_burning_guy, 0, "combat_looks accepted");
    assert_eq!(read.brightness, options::BRIGHTNESS_MAX);
    assert_eq!(read.master_volume, 0);
    let mut written = ConfigFile::default();
    read.write_config(&mut written);
    assert_eq!(written.get("preferences", "running_burning_guy"), Some("0"));
    assert_eq!(options::Preferences::from_config(&written), read);
    // The 20 SPEC-008 save words round-trip.
    let restored = options::Preferences::from_words(&defaults.to_words());
    assert_eq!(restored.text_line_delay, 1.0, "derived from the base delay");
    assert_eq!(
        options::Preferences {
            text_line_delay: defaults.text_line_delay,
            ..restored
        },
        defaults
    );
    assert_eq!(defaults.to_words()[13], 3.5f32.to_bits());
    assert_eq!(options::line_delay(3.5), 1.0);
    // Knob clicks: the ping-pong center, label zones, and dead zones.
    let knob = options::KNOBS[0];
    let widths = [30, 30, 30];
    let center = [knob.origin[0] + 23, knob.origin[1] + 21];
    assert_eq!(
        options::knob_click(&knob, &widths, center, 1, true),
        Some((2, false))
    );
    assert_eq!(
        options::knob_click(&knob, &widths, center, 2, false),
        Some((1, false))
    );
    assert_eq!(
        options::knob_click(&knob, &widths, center, 1, false),
        Some((0, true))
    );
    let [kx, ky] = knob.origin;
    assert_eq!(
        options::knob_click(&knob, &widths, [kx - 10, ky + 15], 2, false),
        Some((0, true))
    );
    assert_eq!(
        options::knob_click(&knob, &widths, [kx + 50, ky + 15], 0, false),
        Some((2, true))
    );
    assert_eq!(
        options::knob_click(&knob, &widths, [kx + 20, ky - 2], 2, true),
        Some((1, false))
    );
    assert_eq!(options::knob_click(&knob, &widths, [0, 0], 2, true), None);
    let violence = options::KNOBS[2];
    let [vx, vy] = violence.origin;
    assert_eq!(
        options::knob_click(&violence, &[30; 4], [vx + 50, vy + 40], 0, true),
        Some((3, false))
    );
    // Switches: toggle at the center; left and right zones.
    let switch = options::SWITCHES[1];
    let [sx, sy] = switch.origin;
    assert_eq!(
        options::switch_click(&switch, [20, 20], [sx + 11, sy + 12], 1),
        Some(0)
    );
    assert_eq!(
        options::switch_click(&switch, [20, 20], [sx - 5, sy], 1),
        Some(0)
    );
    assert_eq!(
        options::switch_click(&switch, [20, 20], [sx + 30, sy], 0),
        Some(1)
    );
    // Combat messages: the left label (Verbose) is value 1.
    let messages = options::SWITCHES[0];
    assert_eq!(
        options::switch_click(
            &messages,
            [40, 30],
            [messages.origin[0] - 5, messages.origin[1]],
            0
        ),
        Some(1)
    );
    assert_eq!(messages.frame(1), 0);
    // Slider hysteresis and values.
    assert_eq!(options::slider_drag(400, 413), 400);
    assert_eq!(options::slider_drag(400, 414), 408);
    assert_eq!(options::slider_drag(400, 390), 384);
    assert_eq!(options::slider_drag(600, 700), 603);
    assert_eq!(options::slider_value(603, 0.0, 50.0), 50.0);
    assert!((options::delay_value(493) - 3.5).abs() < 0.03);
    assert_eq!(
        options::options_key(options::MenuKey::Char('p')),
        Some((options::OptionsAction::Preferences, true))
    );
    assert_eq!(options::paused_title_x(39), 52);
}

#[test]
fn spec014_cursor_modes_hover_and_menu() {
    use cursor::Mode;
    assert_eq!(cursor::cycle_mode(Mode::Movement, false, true), Mode::Arrow);
    assert_eq!(cursor::cycle_mode(Mode::Arrow, false, true), Mode::Movement);
    assert_eq!(cursor::cycle_mode(Mode::Arrow, true, true), Mode::Crosshair);
    assert_eq!(cursor::cycle_mode(Mode::Arrow, true, false), Mode::Movement);
    assert_eq!(
        cursor::cycle_mode(Mode::Crosshair, true, true),
        Mode::Movement
    );
    assert_eq!(
        cursor::cycle_mode(Mode::Skill(9), true, true),
        Mode::Movement
    );
    // The reference transitions by numeric value.
    assert_eq!(cursor::reference_cycle(Mode::UseCrosshair), 1);
    for (skill, next) in [(6, 2), (10, 2), (13, 2), (7, 0), (11, 0), (9, 1), (12, 1)] {
        assert_eq!(
            cursor::reference_cycle(Mode::Skill(skill)),
            next,
            "skill {skill}"
        );
    }
    assert_eq!(
        cursor::move_hover(None, false, 0, 0, 10),
        cursor::MoveHover::Blocked
    );
    assert_eq!(
        cursor::move_hover(Some(5), false, 0, 0, 10),
        cursor::MoveHover::Nothing
    );
    assert_eq!(
        cursor::move_hover(Some(3), true, 1, 2, 10),
        cursor::MoveHover::Cost(10)
    );
    assert_eq!(
        cursor::move_hover(Some(3), true, 2, 0, 10),
        cursor::MoveHover::Blocked
    );
    use cursor::{Entry, Target};
    assert_eq!(
        cursor::menu_entries(Target::Critter { talkable: true }, true),
        [Entry::Look, Entry::Inventory, Entry::Skill, Entry::Cancel]
    );
    assert_eq!(
        cursor::menu_entries(Target::Item { container: true }, false),
        [
            Entry::Use,
            Entry::Look,
            Entry::Inventory,
            Entry::Skill,
            Entry::Cancel
        ]
    );
    assert_eq!(
        cursor::menu_entries(Target::Wall { usable: false }, false),
        [Entry::Look, Entry::Cancel]
    );
    assert_eq!(
        cursor::primary_action(Target::Critter { talkable: true }, true),
        cursor::Primary::Look
    );
    let mut tracker = cursor::MenuTracker::new(100, 3);
    tracker.moved(111);
    tracker.moved(150);
    tracker.moved(200);
    assert_eq!(tracker.highlight, 2, "clamped to the last entry");
    assert_eq!(cursor::edge_direction(0, 0), Some(0));
    assert_eq!(cursor::edge_direction(639, 200), Some(3));
    assert_eq!(cursor::edge_direction(300, 479), Some(5));
    assert_eq!(cursor::edge_direction(300, 200), None);
    assert_eq!(cursor::edge_step(4), [32, 24]);
    assert_eq!(cursor::animated_frame(650, 7), 6);
    assert_eq!(cursor::animated_frame(700, 7), 0);
}

#[test]
fn spec014_counters_item_slot_and_keys() {
    use bar::Glyph;
    assert_eq!(
        bar::counter_glyphs(-1234),
        [
            Glyph::Minus,
            Glyph::Digit(9),
            Glyph::Digit(9),
            Glyph::Digit(9)
        ]
    );
    assert_eq!(bar::roll_delay(10, 5), 41);
    // 99 to 100: ones up, tens up, hundreds up and down, tens and ones down,
    // then the true digits.
    let frames = bar::roll_frames(99, true);
    assert_eq!(frames.len(), 7);
    assert_eq!(frames[0][3], Glyph::RollUp);
    assert_eq!(frames[2][1], Glyph::RollUp);
    assert_eq!(frames[3][1], Glyph::RollDown);
    assert_eq!(frames[6], bar::counter_glyphs(100));
    assert_eq!(bar::roll_frames(5, false).len(), 3);
    // A roll crossing a threshold splits at it: falling past y = 25 a
    // segment starts at 25 in yellow, past r = 12 one starts at 12 in red;
    // rising past them, yellow from 12 and white from 25. Without a color
    // change there is one segment.
    use bar::Band::{Red, White, Yellow};
    assert_eq!(
        bar::roll_segments(30, 20, 50, White),
        [(30, 25, White), (25, 20, Yellow)]
    );
    assert_eq!(
        bar::roll_segments(30, 5, 50, White),
        [(30, 25, White), (25, 12, Yellow), (12, 5, Red)]
    );
    assert_eq!(
        bar::roll_segments(5, 40, 50, Red),
        [(5, 12, Red), (12, 25, Yellow), (25, 40, White)]
    );
    assert_eq!(bar::roll_segments(40, 30, 50, White), [(40, 30, White)]);
    assert_eq!(bar::roll_segments(30, 25, 50, White), [(30, 25, White)]);
    // The whole roll: the old value first, then 3 frames per plain unit.
    let roll = bar::roll(3, &[(3, 1, White)]);
    assert_eq!(roll.len(), 1 + 3 + 3);
    assert_eq!(roll[0].0, bar::counter_glyphs(3));
    assert_eq!(roll[6].0, bar::counter_glyphs(1));
    assert_eq!(
        bar::slot_label(bar::SlotAction::Attack(2)),
        Some("unarmed.frm")
    );
    assert_eq!(bar::slot_label(bar::SlotAction::Attack(9)), None);
    assert_eq!(bar::slot_label_origin(40, false), [141, 7]);
    assert_eq!(bar::cost_digit(10), None);
    assert_eq!(bar::item_art_origin(88, 37), [50, 15]);
    // 5 of 10 fills 35, rounded down to 34: 36 top pixels, then stripes.
    let bar = bar::ammo_bar(5, 10);
    assert_eq!(bar[..36], [14; 36]);
    assert_eq!(bar[36..38], [196, 14]);
    assert_eq!(bar::ammo_bar(0, 0), [14; 70]);
    use bar::WeaponMode;
    let only_reload = |mode: WeaponMode| mode == WeaponMode::Reload;
    assert_eq!(
        bar::next_weapon_mode(WeaponMode::Primary, only_reload),
        WeaponMode::Reload
    );
    assert_eq!(
        bar::next_weapon_mode(WeaponMode::Reload, only_reload),
        WeaponMode::Primary
    );
    // §3.23 the full cycle skips what is unavailable: primary, primary
    // aimed, secondary, secondary aimed, reload, then primary again.
    let mut mode = WeaponMode::Primary;
    let mut seen = Vec::new();
    for _ in 0..5 {
        mode = bar::next_weapon_mode(mode, |_| true);
        seen.push(mode);
    }
    assert_eq!(
        seen,
        [
            WeaponMode::PrimaryAimed,
            WeaponMode::Secondary,
            WeaponMode::SecondaryAimed,
            WeaponMode::Reload,
            WeaponMode::Primary
        ]
    );
    let no_secondary = |mode: WeaponMode| !mode.secondary();
    assert_eq!(
        bar::next_weapon_mode(WeaponMode::PrimaryAimed, no_secondary),
        WeaponMode::Reload
    );
    assert!(WeaponMode::SecondaryAimed.aimed() && !WeaponMode::Secondary.aimed());
    // The same object keeps its action; another object, or an empty hand
    // after an item, starts again at the primary attack.
    let mut slot = bar::HandSlot::default();
    assert_eq!(slot.mode(None), WeaponMode::Primary);
    slot.set(Some(7), WeaponMode::Secondary);
    assert_eq!(slot.mode(Some(7)), WeaponMode::Secondary);
    assert_eq!(slot.mode(Some(8)), WeaponMode::Primary);
    assert_eq!(slot.mode(None), WeaponMode::Primary);
    slot.set(None, WeaponMode::PrimaryAimed);
    assert_eq!(slot.mode(None), WeaponMode::PrimaryAimed);
    assert!(bar::slot_disabled(true, false, true));
    assert!(!bar::slot_disabled(true, false, false));
    use bar::{Action, Key, game_key};
    assert_eq!(
        game_key(Key::Char('3'), true, false),
        Some(Action::Skill(2))
    );
    assert_eq!(
        game_key(Key::Char('#'), true, false),
        Some(Action::Skill(2))
    );
    assert_eq!(game_key(Key::Char('i'), false, false), None);
    assert_eq!(game_key(Key::F6, false, false), Some(Action::QuickSave));
    assert_eq!(game_key(Key::Tab, true, true), None);
    assert_eq!(bar::panel_frame(450, true), 2);
    assert_eq!(bar::panel_frame(0, false), 4);
}

#[test]
fn spec014_save_list_navigation_thumbnail_and_quick_slots() {
    let mut list = saveload::SlotList::default();
    list.key(saveload::ListKey::Up);
    assert_eq!(list.selected, 0);
    list.key(saveload::ListKey::End);
    list.key(saveload::ListKey::Down);
    assert_eq!(list.selected, 9);
    assert_eq!(list.click(130), saveload::Click::Select(1));
    assert_eq!(list.click(140), saveload::Click::Done(1));
    assert_eq!(list.click(140), saveload::Click::Select(1));
    for _ in 0..saveload::DOUBLE_CLICK_FRAMES {
        list.frame();
    }
    assert_eq!(
        list.click(140),
        saveload::Click::Select(1),
        "the window expired"
    );
    assert_eq!(
        saveload::slot_caption(b"SAVE GAME", 0),
        b"[   SAVE GAME 01:   ]"
    );
    let text = |id: i32| format!("<{id}>").into_bytes();
    assert_eq!(
        saveload::second_line(&saveload::SlotState::Empty, text),
        (b"       <111>".to_vec(), false)
    );
    assert_eq!(
        saveload::second_line(&saveload::SlotState::Corrupt, text),
        (b"<112>".to_vec(), true)
    );
    use saveload::{Mode, Preview, SlotState, preview};
    assert_eq!(
        preview(Mode::Save, &SlotState::Empty, true),
        Preview::Current
    );
    assert_eq!(
        preview(Mode::LoadInGame, &SlotState::Corrupt, true),
        Preview::Cover
    );
    let occupied = SlotState::Occupied(Default::default());
    assert_eq!(preview(Mode::LoadInGame, &occupied, true), Preview::Stored);
    assert_eq!(preview(Mode::LoadInGame, &occupied, false), Preview::Cover);
    // The scaler takes source columns 2, 5, ... and fills the edge.
    let view: Vec<u8> = (0..640 * 380).map(|i| (i % 640 / 3) as u8).collect();
    let thumbnail = saveload::thumbnail(&view, 640);
    assert_eq!(thumbnail.len(), 224 * 133);
    assert_eq!(thumbnail[..2], [(2 / 3) as u8, (5 / 3) as u8]);
    assert_eq!(thumbnail[223], thumbnail[222]);
    assert_eq!(thumbnail[132 * 224..], thumbnail[131 * 224..132 * 224]);
    assert_eq!(saveload::location_messages(6, 2), Some((500, 220)));
    assert_eq!(saveload::location_messages(66, 0), None);
    assert_eq!(saveload::location_messages(6, 3), None);
    let mut description = saveload::Description::open(b"Old save");
    description.backspace();
    assert!(description.text.is_empty(), "the first Backspace clears");
    description.key(b'A');
    description.backspace();
    assert!(description.text.is_empty());
    let mut quick = saveload::QuickSlot::default();
    assert_eq!(quick.f6(), saveload::Quick::Pick(Mode::PickQuickSave));
    quick.picked();
    assert_eq!(
        (quick.f6(), quick.f7()),
        (saveload::Quick::SaveDirect, saveload::Quick::LoadDirect)
    );
    quick.forget();
    assert_eq!(quick.f7(), saveload::Quick::Pick(Mode::PickQuickLoad));
}

#[test]
fn spec014_automap_colors_and_mirrored_pixels() {
    use crate::ui::automap::{self, Mark};
    // Hex t paints (449 - 2c, 8 + 2r): the map is mirrored left to right.
    assert_eq!(automap::pixel(0), Some([449, 8]));
    assert_eq!(automap::pixel(199), Some([51, 8]));
    assert_eq!(automap::pixel(200 * 3 + 5), Some([439, 14]));
    assert_eq!(automap::pixel(40000), None);
    assert_eq!(
        automap::player_cross([10, 20]),
        [[9, 20], [10, 19], [10, 21]]
    );
    // The scanner shows living, visible critters seen or not.
    let critter = Mark::Critter {
        hidden: false,
        dead: false,
    };
    assert_eq!(
        automap::color(critter, false, true, false),
        Some(automap::BRIGHT_RED)
    );
    assert_eq!(automap::color(critter, true, false, true), None);
    let dead = Mark::Critter {
        hidden: false,
        dead: true,
    };
    assert_eq!(automap::color(dead, true, true, true), None);
    // Seen objects: walls, the exit-grid marker, the player; scenery only
    // in high detail, never the undrawn PID; unseen objects are skipped.
    assert_eq!(
        automap::color(Mark::Wall, true, false, false),
        Some(automap::BRIGHT_GREEN)
    );
    assert_eq!(automap::color(Mark::Wall, false, false, true), None);
    let marker = Mark::Scenery {
        pid: automap::EXIT_GRID_MARKER,
    };
    assert_eq!(
        automap::color(marker, true, false, false),
        Some(automap::ORANGE)
    );
    let scenery = Mark::Scenery { pid: 0x0200_0001 };
    assert_eq!(automap::color(scenery, true, false, false), None);
    assert_eq!(
        automap::color(scenery, true, false, true),
        Some(automap::DARK_GREEN)
    );
    let undrawn = Mark::Scenery {
        pid: automap::UNDRAWN_SCENERY,
    };
    assert_eq!(automap::color(undrawn, true, false, true), None);
    assert_eq!(
        automap::color(Mark::Player, true, false, false),
        Some(automap::BRIGHT_RED)
    );
    assert_eq!(automap::color(Mark::Other, true, true, true), None);
    // The Pip-Boy page: 2 by 1 cells from (238, 105).
    assert_eq!(automap::pipboy_cell(0, 0), [238, 105]);
    // A two-row pitch: cell row 3 paints window row 111.
    assert_eq!(automap::pipboy_cell(3, 10), [258, 111]);
    assert_eq!(automap::pipboy_color(2), Some(automap::DARK_GREEN));
}

#[test]
fn spec014_inventory_drop_targets_and_item_menus() {
    use crate::{
        skills::ItemEntry,
        ui::inventory::{self, DropTarget},
    };
    // The list area under the safe row rule, then the slots in order.
    assert_eq!(inventory::drop_target([124, 35]), DropTarget::List(0));
    assert_eq!(inventory::drop_target([188, 82]), DropTarget::List(0));
    assert_eq!(inventory::drop_target([150, 83]), DropTarget::List(1));
    assert_eq!(inventory::drop_target([150, 322]), DropTarget::List(5));
    assert_eq!(inventory::drop_target([150, 323]), DropTarget::Nowhere);
    assert_eq!(inventory::drop_target([189, 100]), DropTarget::Nowhere);
    assert_eq!(inventory::drop_target([240, 300]), DropTarget::LeftHand);
    assert_eq!(inventory::drop_target([330, 300]), DropTarget::RightHand);
    assert_eq!(inventory::drop_target([240, 200]), DropTarget::Armor);
    assert_eq!(inventory::drop_target([256, 37]), DropTarget::Figure);
    assert_eq!(inventory::drop_target([316, 137]), DropTarget::Figure);
    assert_eq!(inventory::drop_target([317, 137]), DropTarget::Nowhere);
    use ItemEntry::*;
    assert_eq!(
        inventory::player_menu(true, false, true),
        [Look, Unload, Drop, Cancel]
    );
    assert_eq!(
        inventory::player_menu(false, true, true),
        [Look, Drop, Cancel]
    );
    assert_eq!(
        inventory::player_menu(false, false, true),
        [Look, Use, Drop, Cancel]
    );
    assert_eq!(
        inventory::player_menu(false, false, false),
        [Look, Drop, Cancel]
    );
}

#[test]
fn spec014_map_border_and_player_distance_test() {
    use super::cursor::{distance_refused, inside_border};
    // §3.29 border under the safe rule: columns 45-155, rows 45-154 (the
    // reference also takes column 44 and 156 through its parity slip).
    let hex = |column: i32, row: i32| row * 200 + column;
    assert!(inside_border(20100));
    for (column, row, inside) in [
        (45, 45, true),
        (155, 154, true),
        (44, 100, false),
        (156, 100, false),
        (100, 44, false),
        (100, 155, false),
    ] {
        assert_eq!(inside_border(hex(column, row)), inside, "{column},{row}");
    }
    assert!(!inside_border(-1) && !inside_border(40000));
    // Player-distance test with the odd-column reference (304, 182).
    let odd = hex(101, 100);
    let player = [304 + 500, 182];
    // Farther from the player than the reference and 600 pixels away.
    assert!(distance_refused([304 - 100, 182], player, odd));
    // Toward the player: never refused, however far the player is.
    assert!(!distance_refused([304 + 32, 182], [304 + 1000, 182], odd));
    // Farther, but under 480 across and 400 down.
    assert!(!distance_refused([304 - 100, 182], [304 + 300, 182], odd));
    // Vertically: 400 down is the limit.
    assert!(distance_refused([304, 182 - 24], [304, 182 + 380], odd));
    assert!(!distance_refused([304, 182 - 12], [304, 182 + 380], odd));
    // An even center column moves the reference to (272, 182): a target
    // 490 from the player is refused against 304 but not against 272.
    let even = hex(100, 100);
    let player = [272 + 500, 182];
    assert!(distance_refused([player[0] - 490, 182], player, odd));
    assert!(!distance_refused([player[0] - 490, 182], player, even));
}

#[test]
fn spec014_inventory_examine_wrapping_and_menu_placement() {
    use super::inventory::{INVENTORY_EDGES, USE_ON_EDGES, menu_place, wrap_examine};
    // Six pixels per byte; the panel width is 152 in the window, 30 here.
    let measure = |text: &[u8]| 6 * text.len() as i32;
    let wrap = |text: &str| -> Vec<String> {
        wrap_examine(text.as_bytes(), 30, measure)
            .into_iter()
            .map(|line| String::from_utf8(line).unwrap())
            .collect()
    };
    // No wider than the width: one line.
    assert_eq!(wrap("aa bb"), ["aa bb"]);
    // Cut at the last space that keeps the line narrower than the width:
    // "cc dd" is exactly 30 wide, so it is cut after "cc".
    assert_eq!(wrap("a b cc dd ee"), ["a b", "cc", "dd ee"]);
    // A first word of exactly the width still makes a line.
    assert_eq!(wrap("aaaaa bb"), ["aaaaa", "bb"]);
    // A first word wider than the width drops it and the rest.
    assert_eq!(wrap("aa bb cccccc dd"), ["aa", "bb"]);
    assert!(wrap("aaaaaa bb").is_empty());
    // No space after the first character: drawn whole, clipped.
    assert_eq!(wrap("aaaaaaaa"), ["aaaaaaaa"]);
    // The menu's top-left is at the pointer; at the inventory's right edge
    // (x + 68 reaching 579) it flips, and it shifts up when its last icon
    // would reach the window's bottom (377), by that amount less it plus 2.
    assert_eq!(menu_place([300, 100], 4, INVENTORY_EDGES), (100, false));
    assert_eq!(menu_place([511, 100], 4, INVENTORY_EDGES), (100, true));
    assert_eq!(menu_place([510, 100], 4, INVENTORY_EDGES), (100, false));
    let y = 300;
    assert_eq!(
        menu_place([300, y], 4, INVENTORY_EDGES),
        (y - (y + 159 - 377 + 2), false)
    );
    // The use-on window's edges are 372 and 376.
    assert_eq!(menu_place([304, 100], 3, USE_ON_EDGES), (100, true));
    assert_eq!(
        menu_place([300, 260], 3, USE_ON_EDGES),
        (260 - (260 + 119 - 376 + 2), false)
    );
}

#[test]
fn held_button_repeat() {
    // §3.14: after 13 idle frames (13 * 41 ms) the steps come at 5, 6, 7 ...
    // per second; each waits trunc(1000 / rate).
    let mut repeat = super::HoldRepeat::new(1000, 13);
    assert!(!repeat.poll(1000 + 13 * 41 - 1));
    assert!(repeat.poll(1533));
    assert!(!repeat.poll(1533 + 199));
    assert!(repeat.poll(1733));
    assert!(!repeat.poll(1733 + 165));
    assert!(repeat.poll(1899));
}

/// A synthetic `.pal`: entry levels as given (others black), and an RGB555
/// lookup whose entry for key k is `k % 251`.
fn lookup_palette(entries: &[(usize, [u8; 3])]) -> vault_formats::Palette {
    let mut bytes = vec![0u8; 0x8300];
    for (index, levels) in entries {
        bytes[3 * index..3 * index + 3].copy_from_slice(levels);
    }
    for key in 0..32768usize {
        bytes[768 + key] = (key % 251) as u8;
    }
    vault_formats::Palette::parse(&bytes).unwrap()
}

#[test]
fn spec014_grey_table_uses_max_plus_min_of_the_five_bit_components() {
    // §3.17: s = max + min of P(i), v = trunc(s * 240 / 510), and the grey
    // index is the RGB555 lookup of key 1057 v.
    let palette = lookup_palette(&[
        (1, [59, 59, 59]),
        (57, [63, 63, 31]),
        (145, [63, 39, 18]),
        (215, [15, 62, 0]),
        (220, [63, 63, 63]),
        // An unmapped entry (a level above 63) counts as zero.
        (229, [64, 0, 0]),
    ]);
    let grey = super::grey_table(&palette);
    let key = |v: usize| ((1057 * v) % 251) as u8;
    for (index, v) in [
        (0, 0),
        (1, 27),
        (57, 21),
        (145, 18),
        (215, 14),
        (220, 29),
        (229, 0),
    ] {
        assert_eq!(grey[index], key(v), "index {index}");
    }
}

/// §4.20 Fixture T over the corpus `color.pal`.
#[test]
#[ignore = "needs the local corpus"]
fn fixture_t_grey_table_over_the_corpus_palette() {
    let dir = std::env::var_os("VAULT_CORPUS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../local-corpus")
        });
    let Ok(bytes) = std::fs::read(dir.join("fallout1-unpacked/color.pal")) else {
        eprintln!("skipping: no local corpus at {}", dir.display());
        return;
    };
    let palette = vault_formats::Palette::parse(&bytes).unwrap();
    let grey = super::grey_table(&palette);
    for (index, expected) in [(0, 228), (1, 2), (57, 5), (145, 7), (215, 9), (220, 1)] {
        assert_eq!(grey[index], expected, "index {index}");
    }
    for (index, entry) in grey.iter().enumerate().skip(229) {
        assert!(!palette.is_mapped(index as u8));
        assert_eq!(*entry, 228, "index {index}");
    }
}

#[test]
fn editor_steps_refused_by_the_sheet_spend_and_refund_nothing() {
    // R-10: a loaded base of 12 cannot step to 11, so it refunds no point.
    let context = crate::stats::StatContext::default();
    let mut sheet = editor::blank_sheet(context);
    sheet.base[0] = 12;
    let mut edit = editor::Editor::open(sheet, false, context);
    let points = edit.character_points;
    for _ in 0..5 {
        assert!(!edit.decrement(0));
    }
    assert_eq!(edit.character_points, points);
}

#[test]
fn non_finite_configuration_reals_keep_their_defaults() {
    // R-05: `brightness=nan` used to survive the clamp.
    let config = vault_formats::cfg::ConfigFile::parse(
        "[preferences]\nbrightness=nan\ntext_base_delay=inf\n",
    );
    let preferences = options::Preferences::from_config(&config);
    assert_eq!(preferences.brightness, options::BRIGHTNESS_MIN);
    assert_eq!(preferences.text_base_delay, 3.5);
}

#[test]
fn location_line_wraps_to_three_lines() {
    // Five-pixel glyphs: 32 characters fit in 164 pixels.
    let width = |text: &[u8]| 5 * text.len() as i32;
    let lines = saveload::location_lines(
        b"VAULT 13 LIVING QUARTERS AND THE OVERSEER OFFICE ON THE LOWEST LEVEL OF ALL",
        width,
    );
    assert_eq!(lines.len(), 3);
    assert!(
        lines
            .iter()
            .all(|line| width(line) <= saveload::LOCATION_WIDTH)
    );
    assert_eq!(lines[0], b"VAULT 13 LIVING QUARTERS AND THE");
}

#[test]
fn fixture_s_date_line_and_dialog_done_boxes() {
    // §4.19 Fixture S: `/` at tick 264600, then 39 minutes later.
    let line = |ticks: u64| {
        let [month, day, year] = crate::host_save::game_date(ticks);
        assert_eq!(month, 12);
        let clock = crate::queue::WorldClock {
            ticks,
            ..Default::default()
        };
        bar::date_line(b"December", [day, year], clock.hhmm())
    };
    assert_eq!(line(264_600), b"December: 5/2161 7:21");
    assert_eq!(line(264_600 + 23_400), b"December: 5/2161 8:00");
    // `/` and `?` print it also while the bar is disabled.
    use bar::{Action, Key, game_key};
    for key in ['/', '?'] {
        assert_eq!(
            game_key(Key::Char(key), false, false),
            Some(Action::DateTime)
        );
        assert_eq!(
            game_key(Key::Char(key), true, false),
            Some(Action::DateTime)
        );
    }
    // §3.13: a medium Done box at screen (200, 150).
    use flow::{DialogSize, dialog_buttons_for};
    let [done] = dialog_buttons_for(false, DialogSize::Medium)[..] else {
        panic!("one Done button");
    };
    assert_eq!(done.box_origin, [53, 81]);
    assert_eq!([200 + 53, 150 + 81], [253, 231]);
    assert_eq!(done.caption_origin, [88, 84]);
    assert_eq!(done.button, [66, 85]);
    assert_eq!(
        dialog_buttons_for(false, DialogSize::Large),
        flow::dialog_buttons(false)
    );
    let [large] = flow::dialog_buttons(false)[..] else {
        panic!("one Done button");
    };
    assert_eq!(
        (large.box_origin, large.caption_origin, large.button),
        ([97, 98], [132, 101], [110, 102])
    );
    // Medium requested with a second button: forced large.
    let size = flow::dialog_size(true, Some(DialogSize::Medium), 0, 1);
    assert_eq!(size, DialogSize::Large);
    let boxes: Vec<_> = dialog_buttons_for(true, size)
        .iter()
        .map(|button| button.box_origin)
        .collect();
    assert_eq!(boxes, [[37, 98], [169, 98]]);
}

/// SPEC-020 §2.1: the modal layout and dispatch API, screen by screen.
mod modal_api {
    use crate::stats::StatContext;
    use crate::ui::{
        Armed, FRAME_SIZE, Layout, Pointer, Rect, automap, bar, called, character, editor,
        flow::{self, MainMenuInput, MenuChoice, SelectionAction, SelectionKey, SelectionOutcome},
        frame_point, inventory, options, red_button, saveload, sheet,
    };

    #[test]
    fn rects_layouts_and_arming() {
        assert_eq!(Rect::at([1, 2], [3, 4]), Rect::new(1, 2, 3, 4));
        assert_eq!(
            Rect::new(1, 2, 3, 4).offset([10, 20]),
            Rect::new(11, 22, 3, 4)
        );
        assert_eq!(Rect::new(1, 2, 3, 4).origin(), [1, 2]);
        assert_eq!(red_button([10, 20]), Rect::new(10, 20, 15, 16));
        // A 640-wide frame placed 160 pixels in on a wider surface.
        assert_eq!(FRAME_SIZE, [640, 480]);
        assert_eq!(frame_point([700, 100], [160, 0]), Some([540, 100]));
        assert_eq!(frame_point([799, 479], [160, 0]), Some([639, 479]));
        assert_eq!(frame_point([800, 0], [160, 0]), None);
        assert_eq!(frame_point([100, 0], [160, 0]), None);
        // The first control holding a point wins.
        let layout = Layout::new()
            .with('a', Rect::new(0, 0, 10, 10))
            .with('b', Rect::new(5, 5, 10, 10));
        assert_eq!(layout.hit([6, 6]), Some('a'));
        assert_eq!(layout.hit([12, 12]), Some('b'));
        assert_eq!(layout.hit([20, 20]), None);
        assert_eq!(layout.rect('b'), Some(Rect::new(5, 5, 10, 10)));
        assert!(layout.contains('a') && !layout.contains('c'));
        assert_eq!(layout.len(), 2);
        let moved = layout.clone().offset([100, 0]);
        assert_eq!(moved.hit([106, 6]), Some('a'));
        assert_eq!(moved.hit([6, 6]), None);
        let collected: Layout<char> = layout.controls().iter().copied().collect();
        assert_eq!(collected, layout);
        // Press then release over the same control fires it; any release
        // disarms.
        let mut armed = Armed::new();
        assert_eq!(armed.release(Some('a')), None);
        assert_eq!(armed.press(Some('a')), Some('a'));
        assert_eq!(armed.pressed(), Some('a'));
        assert_eq!(armed.release(Some('b')), None);
        assert_eq!(armed.pressed(), None);
        armed.press(Some('a'));
        assert_eq!(armed.release(Some('a')), Some('a'));
        armed.press(Some('a'));
        assert_eq!(armed.release(None), None);
        assert_eq!(armed.press(None), None);
        assert_eq!(armed.pressed(), None);
    }

    #[test]
    fn main_menu_press_release_and_keys() {
        let layout = flow::menu_layout();
        assert_eq!(
            layout.rect(flow::MenuButton::Intro),
            Some(Rect::new(425, 45, 26, 26))
        );
        assert_eq!(
            flow::MenuButton::at([430, 170]),
            Some(flow::MenuButton::Credits)
        );
        assert_eq!(flow::MenuButton::at([400, 170]), None);
        let mut menu = flow::MainMenu::new(false);
        assert_eq!(menu.layout().rect(4), Some(Rect::new(425, 209, 26, 26)));
        let opaque = |_: usize, _: [i32; 2]| true;
        // A press sounds `nmselec0` and holds; the release selects.
        let press = MainMenuInput::Pointer(Pointer::Press([430, 50]));
        let poll = menu.poll(press, false, 0, opaque);
        assert_eq!((poll.choice, poll.sounds), (None, vec!["nmselec0"]));
        assert_eq!(menu.pressed(), Some(0));
        let release = MainMenuInput::Pointer(Pointer::Release(Some([430, 50])));
        let poll = menu.poll(release, false, 0, opaque);
        assert_eq!(
            (poll.choice, poll.sounds),
            (Some(MenuChoice::Intro), vec!["nmselec1"])
        );
        assert_eq!(menu.pressed(), None);
        // The pixel mask refuses a press; a release elsewhere selects
        // nothing.
        let poll = menu.poll(press, false, 0, |_, _| false);
        assert!(poll.sounds.is_empty() && menu.pressed().is_none());
        menu.poll(
            MainMenuInput::Pointer(Pointer::Press([430, 170])),
            false,
            0,
            opaque,
        );
        let off = MainMenuInput::Pointer(Pointer::Release(Some([430, 210])));
        assert_eq!(menu.poll(off, false, 0, opaque).choice, None);
        // Shift+Credits is Quotes; keys and the timeout as `menu_poll`.
        menu.poll(
            MainMenuInput::Pointer(Pointer::Press([430, 170])),
            true,
            0,
            opaque,
        );
        let credits = MainMenuInput::Pointer(Pointer::Release(Some([430, 170])));
        assert_eq!(
            menu.poll(credits, true, 0, opaque).choice,
            Some(MenuChoice::Quotes)
        );
        assert_eq!(
            menu.poll(MainMenuInput::Key('n'), false, 0, opaque).choice,
            Some(MenuChoice::NewGame)
        );
        assert_eq!(
            menu.poll(MainMenuInput::None, false, 120_000, opaque)
                .choice,
            Some(MenuChoice::Timeout)
        );
        // Fallout 2's six buttons: Options is button 3.
        let mut menu2 = flow::MainMenu::new(true);
        assert_eq!(menu2.layout().len(), 6);
        assert_eq!(menu2.layout().rect(3), Some(Rect::new(30, 142, 26, 26)));
        menu2.poll(
            MainMenuInput::Pointer(Pointer::Press([31, 143])),
            false,
            0,
            opaque,
        );
        let options = MainMenuInput::Pointer(Pointer::Release(Some([31, 143])));
        assert_eq!(
            menu2.poll(options, false, 0, opaque).choice,
            Some(MenuChoice::Options)
        );
        assert_eq!(
            menu2.poll(MainMenuInput::Escape, false, 0, opaque).choice,
            Some(MenuChoice::Exit)
        );
    }

    #[test]
    fn selection_layout_and_transitions() {
        let layout = flow::selection_layout();
        assert_eq!(
            layout.rect(SelectionAction::Take),
            Some(Rect::new(81, 323, 15, 16))
        );
        assert_eq!(
            layout.rect(SelectionAction::Back),
            Some(Rect::new(461, 425, 15, 16))
        );
        assert_eq!(
            layout.rect(SelectionAction::Next),
            Some(Rect::new(318, 320, 20, 18))
        );
        assert_eq!(layout.len(), 6);
        assert_eq!(
            flow::selection_sounds(SelectionAction::Next),
            ("ib2p1xx1", "ib2lu1x1")
        );
        assert_eq!(
            flow::selection_sounds(SelectionAction::Take),
            ("ib1p1xx1", "ib1lu1x1")
        );
        let mut screen = flow::SelectionScreen::new(0);
        assert_eq!(screen.press([320, 325]), Some(SelectionAction::Next));
        assert_eq!(screen.pressed(), Some(SelectionAction::Next));
        assert_eq!(
            screen.release(Some([320, 325])),
            Some(SelectionOutcome::Moved(1))
        );
        assert_eq!(screen.index, 1);
        // A release away from the held arrow does nothing.
        screen.press([300, 325]);
        assert_eq!(screen.release(Some([0, 0])), None);
        assert_eq!(screen.index, 1);
        // The arrows' keys wrap through the three premades.
        assert_eq!(
            screen.key(SelectionKey::Left),
            Some(SelectionOutcome::Moved(0))
        );
        assert_eq!(
            screen.key(SelectionKey::Left),
            Some(SelectionOutcome::Moved(2))
        );
        screen.press([82, 324]);
        assert_eq!(
            screen.release(Some([82, 324])),
            Some(SelectionOutcome::Take(2))
        );
        assert_eq!(
            screen.key(SelectionKey::Char('m')),
            Some(SelectionOutcome::Modify(2))
        );
        assert_eq!(
            screen.key(SelectionKey::Escape),
            Some(SelectionOutcome::Back)
        );
        assert_eq!(
            screen.key(SelectionKey::F10),
            Some(SelectionOutcome::QuitConfirm)
        );
        assert_eq!(screen.key(SelectionKey::Char('x')), None);
        assert_eq!(flow::SelectionScreen::new(4).index, 1);
    }

    #[test]
    fn dialog_box_buttons_and_answers() {
        // The quit confirmation at (169, 117): its No button at (351, 219).
        let mut quit = flow::DialogBox::new([169, 117], true);
        let layout = quit.layout();
        assert_eq!(layout.rect(true), Some(Rect::new(219, 219, 15, 16)));
        assert_eq!(layout.rect(false), Some(Rect::new(351, 219, 15, 16)));
        assert_eq!(quit.click([352, 220]), Some(false));
        assert_eq!(quit.click([0, 0]), None);
        assert_eq!(quit.press([220, 220]), Some(true));
        assert_eq!(quit.pressed(), Some(true));
        assert_eq!(quit.release(Some([352, 220])), None);
        quit.press([220, 220]);
        assert_eq!(quit.release(Some([220, 220])), Some(true));
        assert_eq!(quit.key(flow::DialogKey::Char('y')), Some(true));
        // A Yes/No box is always large; a medium Done box centres its button.
        assert_eq!(
            flow::DialogBox::sized([0, 0], true, flow::DialogSize::Medium).size,
            flow::DialogSize::Large
        );
        let done = flow::DialogBox::sized([0, 0], false, flow::DialogSize::Medium);
        assert_eq!(done.layout().rect(true), Some(Rect::new(66, 85, 15, 16)));
        assert_eq!(done.layout().len(), 1);
        assert_eq!(done.key(flow::DialogKey::Char('y')), None);
        assert_eq!(done.key(flow::DialogKey::Enter), Some(true));
    }

    #[test]
    fn options_menu_pause_and_quit() {
        use options::{MenuKey, OptionsAction, OptionsOutcome, OptionsPage, OptionsWindow};
        let layout = options::options_layout();
        assert_eq!(
            layout.rect(OptionsAction::SaveGame),
            Some(Rect::new(251, 88, 137, 34))
        );
        assert_eq!(
            layout.rect(OptionsAction::Done),
            Some(Rect::new(251, 236, 137, 34))
        );
        assert_eq!(
            options::pause_button(options::PAUSE_ORIGIN),
            Rect::new(276, 196, 15, 16)
        );
        let mut window = OptionsWindow::new(OptionsPage::Menu);
        assert_eq!(window.click([260, 90]), OptionsOutcome::Save);
        assert_eq!(window.click([260, 125]), OptionsOutcome::Load);
        assert_eq!(window.click([0, 0]), OptionsOutcome::None);
        assert_eq!(
            window.click([260, 200]),
            OptionsOutcome::Page(OptionsPage::Quit)
        );
        assert_eq!(window.page, OptionsPage::Quit);
        assert_eq!(
            window.layout().rect(OptionsAction::Done),
            Some(Rect::new(351, 219, 15, 16))
        );
        assert_eq!(window.click([352, 220]), OptionsOutcome::Quit(false));
        assert_eq!(
            window.key(MenuKey::Char('y')),
            (OptionsOutcome::Quit(true), false)
        );
        assert_eq!(window.key(MenuKey::F10), (OptionsOutcome::None, false));
        let mut menu = OptionsWindow::new(OptionsPage::Menu);
        assert_eq!(menu.key(MenuKey::Escape), (OptionsOutcome::Close, false));
        assert_eq!(menu.key(MenuKey::Enter), (OptionsOutcome::Close, true));
        assert_eq!(
            menu.key(MenuKey::Char('p')),
            (OptionsOutcome::Page(OptionsPage::Preferences), true)
        );
        assert_eq!(menu.page, OptionsPage::Preferences);
        assert!(menu.layout().is_empty());
        let mut pause = OptionsWindow::new(OptionsPage::Pause);
        assert_eq!(pause.click([0, 0]), OptionsOutcome::Close);
        assert_eq!(
            pause.key(MenuKey::Char('q')),
            (OptionsOutcome::Close, false)
        );
    }

    #[test]
    fn preferences_controls_drag_and_close() {
        use options::{
            DragStep, LabelWidths, PreferenceButton, PreferenceControl, PreferenceKey,
            PreferenceOutcome, Preferences, PreferencesScreen,
        };
        let widths = LabelWidths::measure(|_| 20);
        let mut screen = PreferencesScreen::open(Preferences::default());
        let layout = screen.layout(&widths, false);
        assert_eq!(
            layout.rect(PreferenceControl::Button(PreferenceButton::Default)),
            Some(Rect::new(23, 450, 15, 16))
        );
        assert_eq!(
            layout.rect(PreferenceControl::Knob(0)),
            Some(Rect::new(58, 67, 84, 48))
        );
        assert_eq!(
            layout.rect(PreferenceControl::Switch(0)),
            Some(Rect::new(283, 69, 57, 28))
        );
        assert_eq!(
            layout.rect(PreferenceControl::Slider(2)),
            Some(Rect::new(384, 184, 240, 23))
        );
        assert_eq!(
            layout.rect(PreferenceControl::Checkbox),
            Some(options::CHECKBOX)
        );
        assert!(
            !screen
                .layout(&widths, true)
                .contains(PreferenceControl::Knob(4))
        );
        // A knob arms on the press and acts on the release: its centre
        // steps game difficulty 1 up to 2, then blocks for the cues.
        assert_eq!(
            screen.press([99, 92], &widths, false, 0),
            PreferenceOutcome::Armed(PreferenceControl::Knob(0))
        );
        assert_eq!(
            screen.release(Some([99, 92]), &widths, 0),
            PreferenceOutcome::Changed {
                cues: options::KNOB_SOUNDS
            }
        );
        assert_eq!(
            (screen.values.game_difficulty, screen.knob_up[0]),
            (2, false)
        );
        assert!(screen.changed && screen.busy(69) && !screen.busy(70));
        assert_eq!(
            screen.press([99, 92], &widths, false, 10),
            PreferenceOutcome::None
        );
        // A switch's centre flips combat messages.
        screen.press([310, 86], &widths, false, 100);
        assert_eq!(
            screen.release(Some([310, 86]), &widths, 100),
            PreferenceOutcome::Changed {
                cues: options::SWITCH_SOUNDS
            }
        );
        assert_eq!(screen.values.combat_messages, 0);
        // A slider press starts the drag; the knob follows the pointer
        // once per 35 ms, and the noted release ends it.
        assert_eq!(
            screen.press([500, 190], &widths, false, 200),
            PreferenceOutcome::DragStarted(2)
        );
        assert_eq!(screen.drag.map(|drag| drag.position), Some(532));
        assert_eq!(
            screen.drag_tick(200, Some(600)),
            Some(DragStep::Moved {
                index: 2,
                iteration: 0,
                moved: true
            })
        );
        assert_eq!(screen.values.master_volume, 31420);
        assert_eq!(screen.drag_tick(210, Some(600)), None);
        assert_eq!(
            screen.release(Some([600, 190]), &widths, 220),
            PreferenceOutcome::None
        );
        assert_eq!(
            screen.drag_tick(235, None),
            Some(DragStep::Ended { index: 2 })
        );
        assert!(screen.drag.is_none());
        // The checkbox toggles on the press.
        assert_eq!(
            screen.press([390, 75], &widths, false, 300),
            PreferenceOutcome::Toggled
        );
        assert_eq!(screen.values.player_speedup, 1);
        // Default, Done and Cancel; keys.
        assert_eq!(
            screen.press([24, 451], &widths, false, 300),
            PreferenceOutcome::Defaults
        );
        assert_eq!(screen.values.game_difficulty, 1);
        assert_eq!(
            screen.press([149, 451], &widths, false, 300),
            PreferenceOutcome::Close { keep: true }
        );
        assert_eq!(
            screen.press([264, 451], &widths, false, 300),
            PreferenceOutcome::Close { keep: false }
        );
        assert_eq!(
            screen.key(PreferenceKey::F10, 300),
            PreferenceOutcome::QuitConfirm
        );
        assert_eq!(
            screen.key(PreferenceKey::Escape, 300),
            PreferenceOutcome::Close { keep: false }
        );
        let backup = screen.backup.clone();
        assert_eq!(screen.clone().close(false), (backup, true));
        let (kept, apply) = screen.close(true);
        assert!(apply && kept.player_speedup == 0);
        let untouched = PreferencesScreen::open(Preferences::default());
        assert!(!untouched.close(true).1);
    }

    fn slots() -> Vec<saveload::SlotState> {
        use saveload::{SlotInfo, SlotState};
        let mut slots = vec![SlotState::Empty; saveload::SLOTS];
        slots[0] = SlotState::Occupied(SlotInfo {
            description: b"OLD".to_vec(),
            ..SlotInfo::default()
        });
        slots[2] = SlotState::Corrupt;
        slots[3] = SlotState::OldVersion;
        slots
    }

    #[test]
    fn save_screen_commit_description_and_arrows() {
        use saveload::{ListKey, Mode, SaveLoadControl, SaveLoadKey, SaveLoadOutcome, SlotList};
        let layout = saveload::layout(false);
        assert_eq!(
            layout.rect(SaveLoadControl::Done),
            Some(Rect::new(391, 349, 15, 16))
        );
        assert_eq!(
            layout.rect(SaveLoadControl::Cancel),
            Some(Rect::new(495, 349, 15, 16))
        );
        assert_eq!(
            layout.rect(SaveLoadControl::List),
            Some(saveload::LIST_AREA)
        );
        let description = saveload::layout(true);
        assert_eq!(description.len(), 2);
        assert_eq!(
            description.rect(SaveLoadControl::DescriptionDone),
            Some(Rect::new(203, 174, 15, 16))
        );
        assert_eq!(
            description.rect(SaveLoadControl::DescriptionCancel),
            Some(Rect::new(329, 174, 15, 16))
        );
        let mut screen = saveload::SaveLoadScreen::open(Mode::Save, slots(), SlotList::default());
        // Saving over an occupied slot asks; Yes prefills the description.
        assert_eq!(
            screen.press([392, 350], 0),
            SaveLoadOutcome::AskOverwrite(0)
        );
        assert_eq!(
            screen.overwrite_accepted(),
            SaveLoadOutcome::DescriptionOpened
        );
        assert_eq!(screen.layout().len(), 2);
        assert_eq!(
            screen.key(SaveLoadKey::Char(b'X')),
            SaveLoadOutcome::Changed
        );
        assert_eq!(
            screen.key(SaveLoadKey::Enter),
            SaveLoadOutcome::Save {
                slot: 0,
                description: b"OLDX".to_vec()
            }
        );
        assert!(screen.description.is_none());
        // An empty slot opens an empty description; its Cancel closes it.
        assert_eq!(
            screen.key(SaveLoadKey::List(ListKey::Down)),
            SaveLoadOutcome::Changed
        );
        assert_eq!(
            screen.press([392, 350], 0),
            SaveLoadOutcome::DescriptionOpened
        );
        assert_eq!(screen.press([330, 175], 0), SaveLoadOutcome::Changed);
        assert!(screen.description.is_none());
        // A held arrow steps, repeats after 13 idle frames, and swallows
        // every other input until it stops.
        assert_eq!(screen.press([36, 73], 0), SaveLoadOutcome::Changed);
        assert_eq!(screen.list.selected, 2);
        assert_eq!(screen.arrow_held(), Some(SaveLoadControl::ArrowDown));
        assert_eq!(screen.key(SaveLoadKey::Escape), SaveLoadOutcome::None);
        assert_eq!(screen.press([392, 350], 0), SaveLoadOutcome::None);
        assert!(!screen.frame(532, true));
        assert!(screen.frame(533, true));
        assert_eq!(screen.list.selected, 3);
        assert!(!screen.frame(600, false));
        assert_eq!(screen.arrow_held(), None);
        // A second click on the remembered slot commits it.
        assert_eq!(screen.press([60, 228], 0), SaveLoadOutcome::Changed);
        assert_eq!(screen.list.selected, 4);
        assert_eq!(
            screen.press([60, 228], 0),
            SaveLoadOutcome::DescriptionOpened
        );
        assert_eq!(screen.key(SaveLoadKey::Escape), SaveLoadOutcome::Changed);
        assert_eq!(
            screen.key(SaveLoadKey::Escape),
            SaveLoadOutcome::Cancel { to_options: false }
        );
        assert_eq!(
            screen.key(SaveLoadKey::Brightness(true)),
            SaveLoadOutcome::Brightness(true)
        );
        assert_eq!(screen.key(SaveLoadKey::Quit), SaveLoadOutcome::QuitConfirm);
        assert_eq!(screen.into_list().selected, 4);
    }

    #[test]
    fn load_screen_commits_by_slot_state() {
        use saveload::{ListKey, Mode, SaveLoadKey, SaveLoadOutcome, SlotList};
        let mut screen =
            saveload::SaveLoadScreen::open(Mode::LoadInGame, slots(), SlotList::default());
        assert_eq!(screen.key(SaveLoadKey::Enter), SaveLoadOutcome::Load(0));
        screen.key(SaveLoadKey::List(ListKey::Down));
        assert_eq!(screen.key(SaveLoadKey::Enter), SaveLoadOutcome::None);
        screen.key(SaveLoadKey::List(ListKey::Down));
        assert_eq!(
            screen.key(SaveLoadKey::Enter),
            SaveLoadOutcome::Notice {
                title: 134,
                body: 135
            }
        );
        screen.key(SaveLoadKey::List(ListKey::Down));
        assert_eq!(
            screen.commit(),
            SaveLoadOutcome::Notice {
                title: 134,
                body: 136
            }
        );
        // Description keys do nothing outside the box; Cancel returns to
        // the options menu that opened the screen.
        assert_eq!(screen.key(SaveLoadKey::Char(b'a')), SaveLoadOutcome::None);
        screen.from_options = true;
        assert_eq!(
            screen.press([496, 350], 0),
            SaveLoadOutcome::Cancel { to_options: true }
        );
        // A failed save's rescan replaces the states.
        screen.rescan(vec![saveload::SlotState::Empty; saveload::SLOTS]);
        assert_eq!(screen.selected_state(), &saveload::SlotState::Empty);
        assert_eq!(screen.overwrite_accepted(), SaveLoadOutcome::None);
    }

    #[test]
    fn inventory_window_layout_and_list() {
        use inventory::{
            InventoryControl, InventoryKey, InventoryOutcome, InventoryWindow, ListStep,
        };
        let layout = inventory::inventory_layout(false, None);
        assert_eq!(
            layout.rect(InventoryControl::Close),
            Some(Rect::new(517, 329, 15, 16))
        );
        assert_eq!(
            layout.rect(InventoryControl::ScrollUp),
            Some(Rect::new(208, 39, 22, 23))
        );
        assert_eq!(
            layout.rect(InventoryControl::Row(0)),
            Some(Rect::new(124, 35, 64, 48))
        );
        assert_eq!(
            layout.rect(InventoryControl::Row(5)),
            Some(Rect::new(124, 275, 64, 48))
        );
        assert_eq!(
            layout.rect(InventoryControl::LeftHand),
            Some(inventory::LEFT_HAND)
        );
        assert_eq!(
            layout.rect(InventoryControl::Figure),
            Some(inventory::FIGURE)
        );
        let use_on = inventory::inventory_layout(true, Some([16, 17]));
        assert_eq!(
            use_on.rect(InventoryControl::Close),
            Some(Rect::new(313, 328, 16, 17))
        );
        assert!(!use_on.contains(InventoryControl::LeftHand));
        let mut window = InventoryWindow::new(false);
        assert_eq!(
            window.press([130, 40], 10, None),
            InventoryOutcome::Entry(0)
        );
        assert_eq!(
            window.press([210, 65], 10, None),
            InventoryOutcome::Scrolled(1)
        );
        assert_eq!(
            window.press([130, 40], 10, None),
            InventoryOutcome::Entry(1)
        );
        assert_eq!(
            window.press([210, 65], 7, None),
            InventoryOutcome::Scrolled(1)
        );
        assert_eq!(window.press([130, 280], 6, None), InventoryOutcome::None);
        assert_eq!(
            window.press([240, 290], 10, None),
            InventoryOutcome::Slot(InventoryControl::LeftHand)
        );
        assert_eq!(window.press([260, 40], 10, None), InventoryOutcome::UpLevel);
        assert_eq!(window.right_press(), InventoryOutcome::Cursor(true));
        assert_eq!(window.press([260, 40], 10, None), InventoryOutcome::None);
        assert_eq!(window.press([518, 330], 10, None), InventoryOutcome::Close);
        assert_eq!(
            window.key(InventoryKey::Step(ListStep::End), 10),
            InventoryOutcome::None
        );
        assert_eq!(
            window.key(InventoryKey::Quit, 10),
            InventoryOutcome::QuitConfirm
        );
        let mut use_on = InventoryWindow::new(true);
        assert_eq!(use_on.press([240, 290], 10, None), InventoryOutcome::None);
        assert_eq!(
            use_on.key(InventoryKey::Step(ListStep::End), 10),
            InventoryOutcome::Scrolled(4)
        );
        assert_eq!(
            use_on.key(InventoryKey::Step(ListStep::Up), 10),
            InventoryOutcome::Scrolled(3)
        );
        assert_eq!(
            use_on.key(InventoryKey::Escape, 10),
            InventoryOutcome::Close
        );
        // The shared scroll rules.
        assert_eq!(inventory::list_step(0, ListStep::PageDown, 20, 6), 6);
        assert_eq!(inventory::list_step(10, ListStep::PageDown, 20, 6), 14);
        assert_eq!(inventory::list_step(3, ListStep::PageUp, 20, 6), 0);
        assert_eq!(inventory::list_step(0, ListStep::Up, 20, 6), 0);
        let mut offset = 0;
        assert!(!inventory::scroll_list(&mut offset, true, 9, 6));
        assert!(inventory::scroll_list(&mut offset, false, 9, 6));
        assert_eq!(offset, 1);
    }

    #[test]
    fn loot_and_barter_layouts() {
        use inventory::{BarterControl, BarterView, LootControl, LootView};
        let view = LootView {
            bodies: 1,
            counts: [3, 0],
            ..LootView::default()
        };
        let layout = inventory::loot_layout(&view);
        assert_eq!(
            layout.rect(LootControl::Done),
            Some(Rect::new(368, 328, 15, 16))
        );
        assert_eq!(
            layout.rect(LootControl::Slot {
                target: false,
                row: 0
            }),
            Some(Rect::new(124, 35, 64, 48))
        );
        assert_eq!(
            layout.rect(LootControl::Scroll {
                target: true,
                up: true
            }),
            Some(Rect::new(459, 39, 22, 23))
        );
        assert_eq!(
            layout.rect(LootControl::Portrait { target: true }),
            Some(Rect::new(377, 37, 60, 100))
        );
        assert_eq!(
            layout.hit([130, 40]),
            Some(LootControl::Slot {
                target: false,
                row: 0
            })
        );
        assert_eq!(layout.hit([130, 185]), None, "row 3 holds no entry");
        assert!(!layout.contains(LootControl::Distance));
        assert!(!layout.contains(LootControl::TakeAll));
        assert!(!layout.contains(LootControl::Cycle { forward: true }));
        let full = inventory::loot_layout(&LootView {
            companion: true,
            take_all: Some(Some([90, 25])),
            bodies: 2,
            counts: [0, 8],
            offsets: [0, 4],
        });
        assert_eq!(
            full.rect(LootControl::Distance),
            Some(Rect::new(294, 210, 108, 20))
        );
        assert_eq!(
            full.rect(LootControl::TakeAll),
            Some(Rect::new(294, 236, 90, 25))
        );
        assert_eq!(
            full.rect(LootControl::Cycle { forward: true }),
            Some(Rect::new(536, 162, 20, 18))
        );
        assert!(full.contains(LootControl::Slot {
            target: true,
            row: 3
        }));
        assert!(!full.contains(LootControl::Slot {
            target: true,
            row: 4
        }));
        let default_take_all = inventory::loot_layout(&LootView {
            take_all: Some(None),
            ..LootView::default()
        });
        assert_eq!(
            default_take_all.rect(LootControl::TakeAll),
            Some(Rect::new(294, 236, 108, 20))
        );

        let barter = inventory::barter_layout(&BarterView {
            wield: true,
            counts: [4, 1, 0, 2],
            offsets: [1, 0, 0, 0],
        });
        assert_eq!(
            barter.rect(BarterControl::Offer),
            Some(Rect::new(41, 452, 14, 14))
        );
        assert_eq!(
            barter.rect(BarterControl::Talk),
            Some(Rect::new(584, 451, 14, 14))
        );
        assert_eq!(
            barter.rect(BarterControl::TradeWield),
            Some(Rect::new(332, 293, 66, 15))
        );
        assert_eq!(
            barter.rect(BarterControl::Scroll {
                column: 0,
                up: true
            }),
            Some(Rect::new(189, 346, 23, 24))
        );
        assert_eq!(
            barter.rect(BarterControl::Slot { column: 0, row: 0 }),
            Some(Rect::new(109, 325, 64, 48))
        );
        assert!(barter.contains(BarterControl::Slot { column: 0, row: 2 }));
        assert!(!barter.contains(BarterControl::Slot { column: 2, row: 0 }));
        assert!(barter.contains(BarterControl::Slot { column: 3, row: 1 }));
        assert_eq!(
            barter.rect(BarterControl::Portrait { merchant: true }),
            Some(Rect::new(560, 314, 60, 100))
        );
        assert_eq!(
            barter.hit([110, 326]),
            Some(BarterControl::Slot { column: 0, row: 0 })
        );
        assert!(
            !inventory::barter_layout(&BarterView::default()).contains(BarterControl::TradeWield)
        );
        // Drag routes: a player item drops on the player table, and back.
        assert_eq!(inventory::barter_drop(0, [250, 320]), Some(1));
        assert_eq!(inventory::barter_drop(0, [250, 300]), None);
        assert_eq!(inventory::barter_drop(1, [120, 400]), Some(0));
        assert_eq!(inventory::barter_drop(2, [480, 400]), Some(3));
        assert_eq!(inventory::barter_drop(3, [400, 400]), None);
        assert_eq!(inventory::barter_route(3).2, 25);
    }

    #[test]
    fn character_screen_controls_and_folders() {
        use character::{
            CharacterControl, CharacterKey, CharacterOutcome, CharacterScreen, Folder,
        };
        use editor::InfoLine;
        let layout = character::character_layout(2);
        assert_eq!(
            layout.rect(CharacterControl::Done),
            Some(Rect::new(455, 454, 15, 16))
        );
        assert_eq!(
            layout.rect(CharacterControl::Cancel),
            Some(Rect::new(552, 454, 15, 16))
        );
        assert_eq!(
            layout.rect(CharacterControl::Print),
            Some(Rect::new(343, 454, 15, 16))
        );
        assert_eq!(
            layout.rect(CharacterControl::SliderPlus),
            Some(Rect::new(614, 42, 16, 12))
        );
        assert_eq!(
            layout.rect(CharacterControl::SliderMinus),
            Some(Rect::new(614, 53, 17, 12))
        );
        assert_eq!(
            layout.rect(CharacterControl::Tabs),
            Some(character::FOLDER_TABS)
        );
        let mut screen = CharacterScreen::default();
        let mask = |_: [i32; 2]| true;
        assert_eq!(screen.press([456, 455], mask), CharacterOutcome::None);
        assert_eq!(screen.pressed(), Some(CharacterControl::Done));
        assert_eq!(
            screen.release(Some([456, 455])),
            CharacterOutcome::Close { keep: true }
        );
        screen.press([553, 455], mask);
        assert_eq!(screen.release(Some([456, 455])), CharacterOutcome::None);
        screen.press([344, 455], mask);
        assert_eq!(screen.release(Some([344, 455])), CharacterOutcome::Print);
        // The tabs act on the release by its x; the mask refuses a press.
        screen.press([150, 340], mask);
        assert_eq!(
            screen.release(Some([250, 340])),
            CharacterOutcome::Folder(Folder::Kills)
        );
        assert_eq!(
            (screen.folder, screen.selected),
            (Folder::Kills, InfoLine::Folder(31))
        );
        assert_eq!(screen.press([150, 340], |_| false), CharacterOutcome::None);
        assert_eq!(screen.pressed(), None);
        // The slider steps on the press and repeats from the 20th frame
        // while the pointer stays on it.
        screen.cursor = 2;
        assert_eq!(
            screen.press([615, 43], mask),
            CharacterOutcome::SkillStep { plus: true }
        );
        screen.hold_slider(true, 0);
        assert_eq!(screen.slider_held(), Some(true));
        assert_eq!(screen.tick(819, Some([615, 43])), None);
        assert_eq!(screen.tick(820, Some([615, 43])), Some(true));
        assert_eq!(screen.tick(900, Some([0, 0])), None);
        assert_eq!(screen.slider_held(), None);
        // The skill rows move the cursor; the info regions select.
        assert_eq!(
            screen.press([400, 84], mask),
            CharacterOutcome::Select(InfoLine::Skill(5))
        );
        assert_eq!(screen.cursor, 5);
        assert_eq!(
            screen.layout().rect(CharacterControl::SliderPlus),
            Some(Rect::new(614, 75, 16, 12))
        );
        assert_eq!(
            screen.press([20, 40], mask),
            CharacterOutcome::Select(InfoLine::Special(0))
        );
        assert_eq!(
            screen.key(CharacterKey::Minus),
            CharacterOutcome::SkillStep { plus: false }
        );
        assert_eq!(
            screen.key(CharacterKey::Escape),
            CharacterOutcome::Close { keep: false }
        );
        assert_eq!(
            screen.key(CharacterKey::Done),
            CharacterOutcome::Close { keep: true }
        );
    }

    #[test]
    fn perk_chooser_controls() {
        use character::{ChooserControl, ChooserKey, ChooserOutcome, PerkChooser};
        let layout = character::chooser_layout();
        assert_eq!(
            layout.rect(ChooserControl::Up),
            Some(Rect::new(58, 137, 11, 14))
        );
        assert_eq!(
            layout.rect(ChooserControl::Down),
            Some(Rect::new(58, 152, 11, 14))
        );
        assert_eq!(
            layout.rect(ChooserControl::Done),
            Some(Rect::new(81, 277, 15, 16))
        );
        assert_eq!(
            layout.rect(ChooserControl::Cancel),
            Some(Rect::new(186, 277, 15, 16))
        );
        assert_eq!(
            layout.rect(ChooserControl::List),
            Some(Rect::new(78, 134, 192, 129))
        );
        let mut chooser = PerkChooser::new(20, 0);
        assert_eq!(chooser.press([60, 155], 0), ChooserOutcome::Moved);
        assert_eq!(chooser.list.selected(), Some(1));
        assert_eq!(chooser.pressed(), Some(ChooserControl::Down));
        assert!(!chooser.tick(614, Some([60, 155])));
        assert!(chooser.tick(615, Some([60, 155])));
        assert_eq!(chooser.list.selected(), Some(2));
        assert!(!chooser.tick(700, Some([0, 0])));
        assert_eq!(chooser.pressed(), None);
        assert_eq!(chooser.release(Some([60, 155])), ChooserOutcome::None);
        chooser.press([82, 278], 0);
        assert_eq!(chooser.pressed(), Some(ChooserControl::Done));
        assert_eq!(chooser.release(Some([82, 278])), ChooserOutcome::Choose);
        chooser.press([187, 278], 0);
        assert_eq!(chooser.release(Some([82, 278])), ChooserOutcome::None);
        chooser.press([187, 278], 0);
        assert_eq!(chooser.release(Some([187, 278])), ChooserOutcome::Cancel);
        assert_eq!(chooser.key(ChooserKey::Home), ChooserOutcome::Moved);
        assert_eq!(chooser.list.selected(), Some(0));
        chooser.press([100, 140], 0);
        assert_eq!(chooser.release(Some([100, 140])), ChooserOutcome::Moved);
        chooser.press([100, 140], 0);
        assert_eq!(chooser.release(Some([100, 140])), ChooserOutcome::Choose);
        assert_eq!(chooser.key(ChooserKey::Escape), ChooserOutcome::Cancel);
        assert_eq!(chooser.key(ChooserKey::Enter), ChooserOutcome::Choose);
        assert_eq!(chooser.key(ChooserKey::Other), ChooserOutcome::None);
    }

    #[test]
    fn editor_layout_and_dispatch() {
        use editor::{EditorControl, EditorDialogControl, EditorDialogKind, EditorOutcome};
        let layout = editor::editor_layout();
        assert_eq!(
            layout.rect(EditorControl::Done),
            Some(Rect::new(455, 454, 15, 16))
        );
        assert_eq!(
            layout.rect(EditorControl::Options),
            Some(Rect::new(343, 454, 15, 16))
        );
        assert_eq!(
            layout.rect(EditorControl::SpecialPlus(0)),
            Some(Rect::new(149, 37, 16, 12))
        );
        assert_eq!(
            layout.rect(EditorControl::SpecialMinus(6)),
            Some(Rect::new(149, 246, 17, 12))
        );
        assert_eq!(
            layout.rect(EditorControl::Tag(17)),
            Some(Rect::new(347, 213, 17, 11))
        );
        assert_eq!(
            layout.rect(EditorControl::Trait(15)),
            Some(Rect::new(298, 443, 17, 11))
        );
        assert_eq!(editor::editor_hit([10, 1], |_, _| false), None);
        assert_eq!(
            editor::editor_hit([10, 1], |_, _| true),
            Some(EditorControl::Name)
        );
        assert_eq!(
            editor::editor_hit([150, 38], |_, _| false),
            Some(EditorControl::SpecialPlus(0))
        );
        let context = StatContext::default();
        let mut editor = editor::Editor::open(editor::blank_sheet(context), true, context);
        assert_eq!(
            editor.dispatch(EditorControl::SpecialPlus(0)),
            EditorOutcome::Stepped(true)
        );
        assert_eq!(editor.character_points, 4);
        assert_eq!(
            editor.dispatch(EditorControl::Tag(3)),
            EditorOutcome::Toggled
        );
        assert_eq!(
            editor.dispatch(EditorControl::Done),
            EditorOutcome::Refused(editor::Refusal {
                title: 118,
                body: 119
            })
        );
        assert_eq!(
            editor.dispatch(EditorControl::Options),
            EditorOutcome::Open(EditorDialogKind::Options)
        );
        assert_eq!(
            editor.dispatch(EditorControl::Cancel),
            EditorOutcome::Cancel
        );
        assert!(editor.step_age(true));
        assert_eq!(editor.sheet.base[33], 26);
        editor.sheet.base[33] = 35;
        assert!(!editor.step_age(true));
        editor.set_female(true);
        assert_eq!(editor.sheet.base[34], 1);
        let age = editor::editor_dialog_layout(EditorDialogKind::Age);
        assert_eq!(
            age.rect(EditorDialogControl::AgeDown),
            Some(Rect::new(173, 13, 20, 18))
        );
        assert_eq!(
            age.rect(EditorDialogControl::Done),
            Some(Rect::new(180, 44, 15, 16))
        );
        let sex = editor::editor_dialog_layout(EditorDialogKind::Sex);
        assert_eq!(
            sex.rect(EditorDialogControl::Done),
            Some(Rect::new(263, 48, 15, 16))
        );
        let options = editor::editor_dialog_layout(EditorDialogKind::Options);
        assert_eq!(
            options.rect(EditorDialogControl::Option(0)),
            Some(Rect::new(251, 107, 137, 34))
        );
        let erase = editor::editor_dialog_layout(EditorDialogKind::Erase);
        assert_eq!(
            erase.rect(EditorDialogControl::EraseYes),
            Some(Rect::new(219, 228, 15, 16))
        );
        assert_eq!(
            erase.rect(EditorDialogControl::EraseNo),
            Some(Rect::new(351, 228, 15, 16))
        );
    }

    #[test]
    fn automap_and_bar_controls() {
        use automap::{AutomapControl, AutomapKey, AutomapOutcome, AutomapWindow};
        let layout = automap::layout();
        assert_eq!(
            layout.rect(AutomapControl::Switch),
            Some(Rect::new(532, 340, 42, 74))
        );
        assert_eq!(
            layout.rect(AutomapControl::Scanner),
            Some(Rect::new(186, 454, 15, 16))
        );
        assert_eq!(
            layout.rect(AutomapControl::Cancel),
            Some(Rect::new(352, 454, 15, 16))
        );
        let mut window = AutomapWindow::open(false);
        assert_eq!(
            window.press([533, 341], false),
            AutomapOutcome::Detail(true)
        );
        assert_eq!(
            window.press([533, 341], true),
            AutomapOutcome::Detail(false)
        );
        assert_eq!(
            window.press([187, 455], false),
            AutomapOutcome::Pressed(AutomapControl::Scanner)
        );
        assert_eq!(window.pressed(), Some(AutomapControl::Scanner));
        assert_eq!(window.release(Some([187, 455])), AutomapOutcome::Scanner);
        window.scanner_on();
        assert_eq!(window.key(AutomapKey::Scanner), AutomapOutcome::None);
        window.press([353, 455], false);
        assert_eq!(window.release(Some([0, 0])), AutomapOutcome::None);
        window.press([353, 455], false);
        assert_eq!(window.release(Some([353, 455])), AutomapOutcome::Close);
        assert_eq!(window.key(AutomapKey::Low), AutomapOutcome::Detail(false));
        assert_eq!(window.key(AutomapKey::Quit), AutomapOutcome::QuitConfirm);

        let quiet = bar::bar_layout(false);
        assert_eq!(
            quiet.rect(bar::BarButton::Inventory),
            Some(Rect::new(211, 420, 32, 21))
        );
        assert!(!quiet.contains(bar::BarButton::EndTurn));
        let combat = bar::bar_layout(true);
        assert_eq!(
            combat.rect(bar::BarButton::EndTurn),
            Some(Rect::new(590, 422, 38, 22))
        );
        let mut buttons = bar::BarButtons::new(false);
        assert_eq!(buttons.press([212, 421]), Some(bar::BarButton::Inventory));
        assert_eq!(buttons.release(Some([212, 421])), Some(bar::Key::Char('i')));
        buttons.press([527, 420]);
        assert_eq!(buttons.pressed(), Some(bar::BarButton::Map));
        assert_eq!(buttons.release(Some([530, 425])), Some(bar::Key::Tab));
        buttons.press([527, 420]);
        assert_eq!(buttons.release(Some([212, 421])), None);
        assert_eq!(
            buttons.press([595, 430]),
            None,
            "no End Turn outside combat"
        );
        assert_eq!(bar::BarButton::ItemSlot.sounds(), bar::SLOT_SOUNDS);
        assert_eq!(bar::BarButton::EndCombat.key(), bar::Key::Enter);
        assert_eq!(bar::monitor_area([30, 410]), Some(true));
        assert_eq!(bar::monitor_area([30, 440]), Some(false));
        assert_eq!(bar::monitor_area([10, 410]), None);
    }

    #[test]
    fn sheet_file_dialogs() {
        use sheet::{DialogKind, FileControl, FileEvent, FileKey, SheetDialog};
        let save = sheet::file_layout(DialogKind::Save);
        assert_eq!(
            save.rect(FileControl::Up),
            Some(Rect::new(204, 124, 11, 14))
        );
        assert_eq!(
            save.rect(FileControl::Done),
            Some(Rect::new(226, 294, 15, 16))
        );
        assert_eq!(
            save.rect(FileControl::Cancel),
            Some(Rect::new(331, 294, 15, 16))
        );
        assert_eq!(
            save.rect(FileControl::List),
            Some(Rect::new(223, 129, 190, 124))
        );
        let load = sheet::file_layout(DialogKind::Load);
        assert_eq!(
            load.rect(FileControl::Done),
            Some(Rect::new(226, 267, 15, 16))
        );
        let names = vec![b"A.GCD".to_vec(), b"B.GCD".to_vec(), b"C.GCD".to_vec()];
        // Load: a second click on a row chooses its file.
        let mut dialog = SheetDialog::new(DialogKind::Load, names.clone(), None, 0);
        assert_eq!(dialog.press([230, 130], 0), None);
        assert_eq!(dialog.release(Some([230, 130])), FileEvent::None);
        dialog.press([230, 130], 0);
        assert_eq!(
            dialog.release(Some([230, 130])),
            FileEvent::Chosen(b"A.GCD".to_vec())
        );
        assert_eq!(dialog.sounds, ["ib1p1xx1"]);
        assert_eq!(
            dialog.key(FileKey::Enter),
            FileEvent::None,
            "Return does nothing in the load dialog"
        );
        // An arrow steps on the press and repeats from the 15th frame.
        dialog.sounds.clear();
        assert_eq!(
            dialog.press([205, 139], 0),
            Some(Rect::new(204, 138, 11, 14))
        );
        assert_eq!(dialog.pressed(), Some(FileControl::Down));
        assert_eq!(dialog.list.selected(), Some(1));
        assert!(dialog.tick(615, Some([205, 139])));
        assert_eq!(dialog.list.selected(), Some(2));
        assert_eq!(dialog.release(None), FileEvent::None);
        assert_eq!(dialog.key(FileKey::Escape), FileEvent::Cancel);
        // Save: a single click copies the base name; typing and Return.
        let mut save = SheetDialog::new(DialogKind::Save, names, Some(b"PLAYER"), 0);
        save.press([230, 130], 0);
        save.release(Some([230, 130]));
        assert_eq!(
            save.field.as_ref().map(|field| field.text.clone()),
            Some(b"A".to_vec())
        );
        assert_eq!(save.key(FileKey::Char(b'X')), FileEvent::None);
        assert_eq!(save.press([227, 295], 0), Some(Rect::new(226, 294, 15, 16)));
        assert_eq!(
            save.release(Some([227, 295])),
            FileEvent::Chosen(b"AX".to_vec())
        );
        save.press([332, 295], 0);
        assert_eq!(save.release(Some([0, 0])), FileEvent::None);
        save.key(FileKey::Erase);
        save.key(FileKey::Erase);
        assert_eq!(
            save.key(FileKey::Enter),
            FileEvent::Cancel,
            "an empty name cancels"
        );
    }

    #[test]
    fn called_shot_window() {
        use crate::critical::location;
        use called::{CalledControl, CalledKey, CalledOutcome, CalledShotWindow};
        let widths = [Some(78); 8];
        let layout = called::layout(&widths, None);
        assert_eq!(
            layout.rect(CalledControl::Cancel),
            Some(Rect::new(278, 288, 15, 16))
        );
        assert_eq!(
            layout.rect(CalledControl::Part(0)),
            Some(Rect::new(141, 60, 88, 10))
        );
        assert_eq!(
            layout.rect(CalledControl::Part(4)),
            Some(Rect::new(381, 60, 88, 10))
        );
        assert_eq!(layout.controls()[0].0, CalledControl::Cancel);
        assert_eq!(
            called::layout(&widths, Some([20, 20])).rect(CalledControl::Cancel),
            Some(Rect::new(278, 288, 20, 20))
        );
        let mut window = CalledShotWindow::new(widths, None);
        assert_eq!(window.press([145, 62]), Some(CalledControl::Part(0)));
        match window.release(Some([145, 62])) {
            CalledOutcome::Choose(part) => assert_eq!(part.location, location::HEAD),
            other => panic!("expected the head, got {other:?}"),
        }
        window.press([145, 62]);
        assert_eq!(window.release(Some([385, 62])), CalledOutcome::None);
        window.press([279, 289]);
        assert_eq!(window.release(Some([279, 289])), CalledOutcome::Cancel);
        assert_eq!(window.key(CalledKey::Escape), CalledOutcome::Cancel);
        let torso = window.key(CalledKey::Code {
            letter: Some('d'),
            control: true,
            backspace: false,
        });
        assert!(matches!(torso, CalledOutcome::Choose(part) if part.location == location::TORSO));
        let ignored = window.key(CalledKey::Code {
            letter: Some('h'),
            control: true,
            backspace: false,
        });
        assert_eq!(ignored, CalledOutcome::None);
    }
}

/// SPEC-020 §2.1 / SPEC-013 §3.15: the Skilldex layout, the row's wait
/// and the action the closing window leads to.
#[test]
fn spec020_skilldex_layout_wait_and_action() {
    use super::{
        Rect,
        skilldex::{self, SkilldexAction, SkilldexControl, SkilldexKey, SkilldexOutcome},
    };
    use crate::skills::{self, skill};
    let layout = skilldex::layout();
    assert_eq!(layout.len(), 9);
    // Row k at (451 + 15, 5 + 45 + 36 k), 86 by 33; Cancel at (499, 343).
    assert_eq!(
        layout.rect(SkilldexControl::Row(1)),
        Some(Rect::new(466, 86, 86, 33))
    );
    assert_eq!(
        layout.rect(SkilldexControl::Cancel),
        Some(Rect::new(499, 343, 15, 16))
    );
    assert_eq!(
        layout.hit([466 + 85, 50 + 32]),
        Some(SkilldexControl::Row(0))
    );
    assert_eq!(layout.hit([466 + 86, 50]), None);
    // Opened from the bar: a row press waits trunc(1000 / 9) ms, swallowing
    // presses and keys, then chooses the skill.
    let mut window = skilldex::SkilldexWindow::new(None);
    assert_eq!(window.key(SkilldexKey::Other), SkilldexOutcome::None);
    assert_eq!(window.press([10, 10], 1000), SkilldexOutcome::Busy);
    assert_eq!(
        window.press([470, 90], 1000),
        SkilldexOutcome::Chosen {
            row: 1,
            skill: skill::LOCKPICK
        }
    );
    assert_eq!(window.chosen(), Some(skill::LOCKPICK));
    assert_eq!(window.press([500, 345], 1001), SkilldexOutcome::Busy);
    assert_eq!(window.key(SkilldexKey::Escape), SkilldexOutcome::Busy);
    assert_eq!(window.due(1000 + skills::SKILLDEX_CLOSE_MS - 1), None);
    assert_eq!(
        window.due(1111),
        Some(SkilldexAction::Choose(skill::LOCKPICK))
    );
    // Esc, Enter and Cancel close.
    let mut window = skilldex::SkilldexWindow::new(None);
    assert_eq!(window.key(SkilldexKey::Escape), SkilldexOutcome::Cancel);
    assert_eq!(window.key(SkilldexKey::Enter), SkilldexOutcome::Cancel);
    assert_eq!(window.press([500, 345], 0), SkilldexOutcome::Cancel);
    assert_eq!(window.chosen(), None);
    // From the action menu: a skill is used on the target; Sneak toggles.
    let mut window = skilldex::SkilldexWindow::new(Some(7));
    window.press([470, 50 + 36 * 4 + 5], 0);
    assert_eq!(
        window.due(111),
        Some(SkilldexAction::UseOn {
            target: 7,
            skill: skill::FIRST_AID
        })
    );
    let mut window = skilldex::SkilldexWindow::new(Some(7));
    window.press([470, 54], 0);
    assert_eq!(window.due(111), Some(SkilldexAction::Choose(skill::SNEAK)));
}

/// SPEC-020 §2.1 / SPEC-013 §3.21: the elevator layout, the press-then-
/// release choice, the keys and the travel animation's pacing.
#[test]
fn spec020_elevator_layout_choice_and_travel() {
    use super::{
        Rect,
        elevator::{self, ElevatorKey, ElevatorOutcome, ElevatorWindow},
    };
    use crate::skills::{self, ElevatorView};
    let view = ElevatorView::from(skills::elevator(8).unwrap());
    // A 230-by-284 background sits at ((640 - 230) / 2, (379 - 284) / 2).
    let mut window = ElevatorWindow::open(8, view.clone(), 2, None);
    assert_eq!(window.origin(), [205, 47]);
    let layout = window.layout();
    assert_eq!(layout.len(), 3);
    assert_eq!(layout.rect(1), Some(Rect::new(218, 147, 48, 50)));
    assert_eq!(
        elevator::layout([0, 0], 2).hit([13 + 47, 100 + 49]),
        Some(1)
    );
    assert_eq!(
        ElevatorWindow::open(8, view.clone(), 0, Some([200, 300])).origin(),
        [220, 39]
    );
    // The start level's gauge cell; a press holds, a release elsewhere does
    // nothing, a release over it chooses.
    assert_eq!(window.gauge_cell(0), 12);
    let button = [205 + 13 + 5, 47 + 40 + 5];
    assert_eq!(window.press([0, 0]), ElevatorOutcome::Busy);
    assert_eq!(window.press(button), ElevatorOutcome::Pressed(0));
    assert_eq!(window.pressed(), Some(0));
    assert_eq!(window.release(Some([0, 0]), 0), ElevatorOutcome::None);
    assert_eq!(window.pressed(), None);
    assert_eq!(window.release(Some(button), 0), ElevatorOutcome::None);
    window.press(button);
    assert_eq!(
        window.release(Some(button), 100),
        ElevatorOutcome::Travel {
            sound: view.travel_sound(2)
        }
    );
    // Moving: input is swallowed; the gauge steps every 46 ms from 12 down
    // to 0, then the 200 ms pause ends it.
    assert!(window.moving());
    assert_eq!(window.key(ElevatorKey::Escape, 100), ElevatorOutcome::Busy);
    assert_eq!(window.press(button), ElevatorOutcome::Busy);
    let cells = view.gauge_path(2, 0);
    let end = 100 + cells.len() as u64 * view.step_ms() + skills::ELEVATOR_PAUSE_MS;
    assert_eq!(window.tick(100), ElevatorOutcome::Redraw);
    assert_eq!(window.tick(101), ElevatorOutcome::None);
    assert_eq!(window.gauge_cell(100), cells[0]);
    assert_eq!(window.gauge_cell(end - 1), *cells.last().unwrap());
    assert_eq!(window.tick(end - 1), ElevatorOutcome::Redraw);
    assert_eq!(window.tick(end), ElevatorOutcome::Perform(Some(0)));
    // Keys: Esc cancels; the start level is performed at once; a key
    // without a button is not taken.
    let mut window = ElevatorWindow::open(8, view.clone(), 2, None);
    assert_eq!(window.key(ElevatorKey::Other, 0), ElevatorOutcome::None);
    assert_eq!(
        window.key(ElevatorKey::Char(b'9'), 0),
        ElevatorOutcome::None
    );
    assert_eq!(
        window.key(ElevatorKey::Escape, 0),
        ElevatorOutcome::Perform(None)
    );
    assert_eq!(
        window.key(ElevatorKey::Char(b'3'), 0),
        ElevatorOutcome::Perform(Some(2))
    );
    assert!(!window.moving());
    assert!(matches!(
        window.key(ElevatorKey::Char(b'2'), 0),
        ElevatorOutcome::Travel { .. }
    ));
}
