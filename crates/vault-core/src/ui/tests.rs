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
    assert_eq!(automap::pipboy_cell(3, 10), [258, 108]);
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
