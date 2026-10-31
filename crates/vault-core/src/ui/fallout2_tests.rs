//! SPEC-019 §4 vectors A, B, C, L (the gauge), P and Q on the interface
//! rules, and the panel geometry of §3.12.3 and §3.15.
use super::*;
use crate::ui::{bar, flow::MenuChoice};
use vault_formats::fallout2::lines::{EndgameSlide, SlideArt};

#[test]
fn spec019_vector_a_bar_geometry() {
    let full = BarGeometry::new(100);
    assert_eq!(full.origin(), bar::BAR_ORIGIN);
    assert_eq!(full.map_size(), [640, 381]);
    assert_eq!(full.map_bottom(), 379);
    assert_eq!(full.indicator_origin(), bar::INDICATOR_ORIGIN);
    for (_, rect, ..) in bar::BAR_BUTTONS {
        assert_eq!(full.rect(rect), rect);
    }
    let short = BarGeometry::new(99);
    assert_eq!((short.origin(), short.clamped), ([0, 380], false));
    let inventory = short.rect(bar::BAR_BUTTONS[0].1);
    assert_eq!([inventory.x, inventory.y], [211, 40]);
    assert_eq!(short.origin()[1] + inventory.y, 420);
    assert_eq!(short.element(bar::HP_COUNTER), [473, 39]);
    assert_eq!(short.map_size(), [640, 382]);
    assert_eq!(short.map_bottom(), 380);
    assert_eq!(short.indicator_origin(), [0, 359]);
    // Height 50 (and 101) is taken as 100, with a diagnostic.
    for height in [50, 101, 0] {
        let geometry = BarGeometry::new(height);
        assert_eq!((geometry.height, geometry.clamped), (100, true));
    }
    assert!(!BarGeometry::new(60).clamped);
}

#[test]
fn spec024_custom_boxes_follow_the_named_ones_up_to_five_slots() {
    let named = indicators(true, true, false, 3, 0);
    assert_eq!(
        with_custom(named.clone(), &[5, 6]),
        [
            Indicator::Addict,
            Indicator::Sneak,
            Indicator::Poisoned,
            Indicator::Custom(5),
            Indicator::Custom(6)
        ]
    );
    assert_eq!(with_custom(named, &[5, 6, 7]).len(), 5);
    assert_eq!(Indicator::Custom(7).label().0, 107);
}

#[test]
fn spec019_vector_b_indicator_boxes() {
    let lit = indicators(true, true, false, 3, 0);
    assert_eq!(
        lit,
        [Indicator::Addict, Indicator::Sneak, Indicator::Poisoned]
    );
    let x: Vec<i32> = (0..lit.len()).map(bar::indicator_x).collect();
    assert_eq!(x, [0, 124, 251]);
    assert_eq!(bar::indicator_window_width(lit.len()), 381);
    assert_eq!(Indicator::Addict.label().2, ColorKey(31744));
    assert_eq!(Indicator::Poisoned.label().2, ColorKey(31744));
    assert_eq!(Indicator::Sneak.label().2, ColorKey(992));
    assert_eq!(
        indicators(true, true, true, 1, 1),
        [
            Indicator::Addict,
            Indicator::Sneak,
            Indicator::Level,
            Indicator::Poisoned,
            Indicator::Radiated
        ]
    );
    assert_eq!(Indicator::Radiated.label().0, 104);
}

#[test]
fn spec019_vector_c_main_menu() {
    // Button 3 is Options, at (30, 142) 26 by 26.
    let options = menu_button(3);
    assert_eq!(options, Rect::new(30, 142, 26, 26));
    assert_eq!(menu_button_at([40, 147]), Some(3));
    assert_eq!(MENU_CHOICES[3], MenuChoice::Options);
    let press = menu_poll(MenuInput::Press(3), false, 0);
    assert_eq!((press.choice, press.sounds), (None, vec!["nmselec0"]));
    let release = menu_poll(MenuInput::Release(3), false, 0);
    assert_eq!(
        (release.choice, release.sounds),
        (Some(MenuChoice::Options), vec!["nmselec1"])
    );
    // The `O` key, either case: Options with `nmselec1` only.
    for key in ['O', 'o'] {
        let poll = menu_poll(MenuInput::Key(key), false, 0);
        assert_eq!(
            (poll.choice, poll.sounds),
            (Some(MenuChoice::Options), vec!["nmselec1"])
        );
    }
    assert_eq!(menu_label(3, 60), [96, 143]);
    // Shift+Credits, Esc, `d` and the timeout as SPEC-014.
    assert_eq!(
        menu_poll(MenuInput::Release(4), true, 0).choice,
        Some(MenuChoice::Quotes)
    );
    assert_eq!(
        menu_poll(MenuInput::Escape, false, 0).choice,
        Some(MenuChoice::Exit)
    );
    assert_eq!(
        menu_poll(MenuInput::Key('d'), false, 0).choice,
        Some(MenuChoice::Timeout)
    );
    assert_eq!(
        menu_poll(MenuInput::None, false, 120_000).choice,
        Some(MenuChoice::Timeout)
    );
    assert_eq!(menu_hotkey('E'), Some(5));
    assert_eq!(menu_button_at([29, 147]), None);
}

#[test]
fn spec019_side_panel_and_town_list() {
    assert_eq!(panel::row_button(0), Rect::new(508, 138, 15, 15));
    assert_eq!(panel::row_label(6), [531, 300]);
    assert_eq!(panel::dial_frame(12, 24), 12);
    assert_eq!(panel::dial_frame(23, 8), 7);
    assert_eq!(panel::dial_frame(0, 1), 0);
    let mut list = TownList::default();
    assert!(!list.up());
    assert!(!list.down(7));
    assert!(list.down(9));
    assert!(list.down(9));
    assert!(!list.down(9));
    assert_eq!(list.entry(0, 9), Some(2));
    assert_eq!(list.entry(6, 9), Some(8));
    assert_eq!(list.entry(7, 9), None);
    list.clamp(8);
    assert_eq!(list.offset, 1);
    list.clamp(3);
    assert_eq!(list.offset, 0);
    assert!(list.entry(3, 3).is_none());
}

#[test]
fn spec019_city_circles_and_entrance_markers() {
    let points = circle([100, 100], 13);
    assert!(points.contains(&[113, 100]) && points.contains(&[100, 87]));
    assert!(points.iter().all(|[x, y]| {
        let d2 = (x - 100).pow(2) + (y - 100).pow(2);
        (12 * 12..=14 * 14).contains(&d2)
    }));
    assert_eq!(city_label([500, 400], 13, 40), [480, 415]);
    assert_eq!(entrance_button([100, 200]), Rect::new(43, 194, 164, 25));
    assert_eq!(entrance_triangle([100, 200]), [112, 206]);
}

#[test]
fn spec019_vector_l_gauge() {
    let car = crate::fallout2::Car {
        has_car: true,
        area: 0,
        map: 0,
        fuel: 40000,
    };
    assert_eq!(car.gauge_rows(), 94);
    assert_eq!(gauge_fill(94), Rect::new(493, 233, 6, 94));
    assert_eq!(gauge_fill(0).h, 0);
    let full = crate::fallout2::Car { fuel: 80000, ..car };
    assert_eq!(gauge_fill(full.gauge_rows()), panel::GAUGE);
}

#[test]
fn spec019_control_and_custom_panels() {
    assert_eq!(control::disposition_button(4), [380, 108]);
    assert_eq!(control::disposition_label(0), [400, 20]);
    assert_eq!(
        control::button_rect(control::BEST_BUTTON),
        Rect::new(150, 100, 15, 16)
    );
    assert_eq!(custom::button(5), [150, 130]);
    // Revision 2: the window takes its background's size.
    assert_eq!(custom::window(None), Rect::new(170, 145, 300, 190));
    assert_eq!(custom::window(Some([300, 190])), custom::window(None));
    assert_eq!(
        custom::window(Some([640, 190])),
        Rect::new(0, 290, 640, 190)
    );
    assert_eq!(custom::FULL_ORIGIN, [0, 290]);
    assert_eq!(custom::value(0), [170, 20]);
    assert_eq!(text::hp(12, 40), "HP 12/40");
    assert_eq!(text::ac(5), "AC 5");
    assert_eq!(text::level(2), "Level 2");
    assert_eq!(
        value_text(CUSTOM_FIELDS[0], 4),
        "Be absolutely sure".to_string()
    );
    assert_eq!(weight_color(101, 100), ColorKey::RED);
    assert_eq!(weight_color(100, 100), ColorKey::GREEN);
}

#[test]
fn spec019_vector_p_archives() {
    let mut played = [0u8; 17];
    for index in [1, 2, 5, 12] {
        played[index] = 1;
    }
    assert_eq!(archive_movies(&played), [2, 5, 12]);
    let titles: Vec<i32> = archive_movies(&played)
        .into_iter()
        .map(archive_title)
        .collect();
    assert_eq!(titles, [502, 505, 512]);
    assert_eq!(archive_selection(&played, 1), Some(2));
    assert_eq!(archive_selection(&played, 4), None);
    assert_eq!(archive_selection(&played, 0), None);
    played[16] = 1;
    played[0] = 1;
    assert_eq!(archive_movies(&played), [2, 5, 12]);
}

#[test]
fn spec019_vector_q_endgame_slides() {
    let slide = |global, value, art: SlideArt, narration: &str, direction| EndgameSlide {
        global,
        value,
        art,
        narration: narration.into(),
        direction,
    };
    let table = [
        slide(5, 1, SlideArt::Stem("endA".into()), "nar_a", 0),
        slide(5, 2, SlideArt::Stem("endB".into()), "nar_b", 0),
        slide(9, 0, SlideArt::Row(7), "nar_c", -1),
    ];
    let mut globals = vec![0; 10];
    globals[5] = 2;
    let before = globals.clone();
    let mut notes = Vec::new();
    let slides = endgame_slides(
        &table,
        &globals,
        |row| (row == 7).then(|| "DP.FRM".to_string()),
        &mut notes,
    );
    assert_eq!(slides.len(), 2);
    assert_eq!(slides[0].art, "art/intrface/endb.frm");
    assert_eq!(slides[0].palette, "art/intrface/endb.pal");
    assert_eq!(slides[0].pan, None);
    assert_eq!(slides[1].art, "art/intrface/dp.frm");
    assert_eq!(slides[1].palette, "art/intrface/dp.pal");
    assert_eq!(slides[1].pan, Some(false));
    assert_eq!(slides[1].narration, "sound/speech/narrator/nar_c.acm");
    assert!(notes.is_empty());
    assert_eq!(globals, before, "no global is written");
    // An unnamed row is skipped with a note; without a table nothing shows.
    let missing = endgame_slides(&table[2..], &globals, |_| None, &mut notes);
    assert!(missing.is_empty() && notes.len() == 1);
    assert!(endgame_slides(&[], &globals, |_| None, &mut notes).is_empty());
}

#[test]
fn spec019_art_resolution() {
    let rows: Vec<&[u8]> = vec![b"IFACE.FRM", b"takeallu.frm", b"INVMADN.FRM"];
    assert_eq!(IFACE.resolve(rows.iter().copied()), Some(("iface.frm", 0)));
    // The first candidate present wins.
    assert_eq!(
        TAKE_ALL[0].resolve(rows.iter().copied()),
        Some(("takeallu.frm", 1))
    );
    assert_eq!(
        TAKE_ALL[1].resolve(rows.iter().copied()),
        Some(("invmadn.frm", 2))
    );
    assert_eq!(DIAL.resolve(rows.iter().copied()), None);
    assert_eq!(DIAL.stand_in, StandIn::None);
}

#[test]
fn spec019_kill_names() {
    assert_eq!(kill_messages(0), (1450, 1469));
    assert_eq!(kill_messages(18), (1468, 1487));
    assert_eq!(KILL_FALLBACKS[15], "Gecko");
    assert_eq!(KILL_FALLBACKS[18], "Big Bad Boss");
}

/// SPEC-020 §2.1: the combat control panels', the world map side panel's
/// and the town map entrances' layouts and dispatch.
mod modal {
    use super::*;

    #[test]
    fn control_panel_layout_and_dispatch() {
        // Disposition 0 is also the custom value, so allowing it shows
        // Custom.
        let all = control_layout(&[0, 1, 2]);
        assert_eq!(
            all.rect(ControlButton::Disposition(0)),
            Some(Rect::new(380, 310, 15, 16))
        );
        assert_eq!(
            all.rect(ControlButton::Disposition(1)),
            Some(Rect::new(380, 332, 15, 16))
        );
        assert!(!all.contains(ControlButton::Disposition(3)));
        assert_eq!(
            all.rect(ControlButton::Custom),
            Some(Rect::new(380, 424, 15, 16))
        );
        assert_eq!(
            all.rect(ControlButton::Best),
            Some(Rect::new(150, 390, 15, 16))
        );
        assert_eq!(
            all.rect(ControlButton::Talk),
            Some(Rect::new(584, 452, 14, 14))
        );
        let stand_in = custom_layout(None);
        assert_eq!(
            stand_in.rect(ControlButton::Row(0)),
            Some(Rect::new(320, 165, 15, 16))
        );
        assert_eq!(
            stand_in.rect(ControlButton::Done),
            Some(Rect::new(280, 305, 15, 16))
        );
        assert_eq!(stand_in.len(), 7);
        let retail = custom_layout(Some([640, 190]));
        assert_eq!(
            retail.rect(ControlButton::Row(0)),
            Some(Rect::new(150, 310, 15, 16))
        );
        assert_eq!(ControlButton::Talk.sounds(), control::TALK_SOUNDS);

        let mut panel = ControlPanelWindow::new(vec![1, 2], None);
        assert_eq!(panel.press([381, 333]), Some(ControlButton::Disposition(1)));
        assert_eq!(
            panel.release(Some([381, 333])),
            ControlOutcome::Disposition(1)
        );
        assert_eq!(panel.press([381, 425]), None, "Custom is not allowed");
        assert_eq!(
            panel.dispatch(ControlButton::Custom),
            ControlOutcome::Custom(true)
        );
        assert!(panel.custom);
        // The custom panel is modal: its rows cycle their fields.
        assert_eq!(panel.press([321, 210]), Some(ControlButton::Row(2)));
        assert_eq!(
            panel.release(Some([321, 210])),
            ControlOutcome::Cycle(ControlField::BestWeapon)
        );
        assert_eq!(panel.key(ControlKey::Best), ControlOutcome::None);
        assert_eq!(panel.key(ControlKey::Escape), ControlOutcome::Custom(false));
        assert!(!panel.custom);
        assert_eq!(
            panel.key(ControlKey::Digit(4)),
            ControlOutcome::Disposition(4)
        );
        assert_eq!(panel.key(ControlKey::Digit(7)), ControlOutcome::None);
        assert_eq!(panel.key(ControlKey::Best), ControlOutcome::BestWeapon);
        assert_eq!(panel.key(ControlKey::Talk), ControlOutcome::Talk);
        assert_eq!(panel.key(ControlKey::Quit), ControlOutcome::QuitConfirm);
        panel.press([585, 453]);
        assert_eq!(panel.release(Some([585, 453])), ControlOutcome::Talk);
    }

    #[test]
    fn world_panel_and_entrances() {
        let list = TownList { offset: 0 };
        let layout = world_panel_layout(list, 3);
        assert_eq!(
            layout.rect(WorldPanelControl::Row(0)),
            Some(Rect::new(508, 138, 15, 15))
        );
        assert_eq!(
            layout.rect(WorldPanelControl::Row(2)),
            Some(Rect::new(508, 192, 15, 15))
        );
        assert!(!layout.contains(WorldPanelControl::Row(3)));
        assert_eq!(layout.rect(WorldPanelControl::ListUp), Some(panel::LIST_UP));
        assert_eq!(
            layout.rect(WorldPanelControl::TownWorld),
            Some(panel::TOWN_WORLD)
        );
        assert_eq!(
            world_panel_layout(TownList { offset: 2 }, 9).len(),
            3 + panel::ROWS
        );
        let entrances = entrance_layout(&[[100, 100], [300, 200]]);
        assert_eq!(entrances.hit([50, 100]), Some(0));
        assert_eq!(entrances.rect(1), Some(Rect::new(243, 194, 164, 25)));
        assert_eq!(entrances.hit([0, 0]), None);
    }
}
