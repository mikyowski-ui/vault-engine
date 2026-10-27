//! SPEC-014 §3.41 and §4.10 Fixture J: the Pip-Boy's layout rules.
use super::*;
use crate::host_rest::minutes_until_hour;

/// Font 101 widths from Fixture J.
const STATUS: i32 = 47;
const DATA: i32 = 33;
const BACK: i32 = 32;
const MORE: i32 = 31;
const OF: i32 = 13;

#[test]
fn spec014_fixture_j_date_time_and_holidays() {
    // Tick 264600 is 07:21 on 5 December 2161, the new-game clock.
    assert_eq!(date(264_600), [12, 5, 2161]);
    assert_eq!(date(864_000), [12, 6, 2161]);
    assert_eq!(date(27 * 864_000), [1, 1, 2162]);
    let (digits, month) = clock_cells(264_600);
    let expected: Vec<(i32, [i32; 2])> = [
        (0, [20, 17]),
        (5, [29, 17]),
        (2, [83, 17]),
        (1, [92, 17]),
        (6, [101, 17]),
        (1, [110, 17]),
        (0, [155, 17]),
        (7, [164, 17]),
        (2, [173, 17]),
        (1, [182, 17]),
    ]
    .into();
    assert_eq!(
        digits
            .iter()
            .map(|cell| (cell.source.x / 9, cell.at))
            .collect::<Vec<_>>(),
        expected
    );
    assert!(
        digits
            .iter()
            .all(|cell| cell.source.w == 9 && cell.source.h == 17)
    );
    // DEC: months.frm rows 165-178 at (46, 18).
    assert_eq!(
        month,
        Cell {
            source: Rect::new(0, 165, 29, 14),
            at: [46, 18]
        }
    );
    // 25 December: "Merry Christmas!" (101 wide) at (381, 315).
    assert_eq!(holiday(12, 25), Some(107));
    assert_eq!(holiday(10, 6), Some(103));
    assert_eq!(holiday(12, 5), None);
    assert_eq!([holiday_x(101), HOLIDAY_Y], [381, 315]);
    assert_eq!(LOGO_AT, [323, 156]);
}

#[test]
fn spec014_fixture_j_vault_water_note() {
    let cells = |days| {
        note_digits(days)
            .iter()
            .map(|cell| (cell.source.x / 12, cell.at))
            .collect::<Vec<_>>()
    };
    assert!(note_shown(0, 150));
    assert_eq!(cells(150), [(0, [92, 180]), (5, [80, 182]), (1, [68, 184])]);
    // After a digit 1 the next moves only 6 pixels left.
    assert_eq!(cells(11), [(1, [92, 180]), (1, [86, 182])]);
    assert_eq!(cells(7), [(7, [92, 180])]);
    assert!(!note_shown(2, 150), "the water chip is home");
    assert!(!note_shown(0, 0), "no days left");
    assert!(!note_shown(1, -5), "safe rule: a negative count hides it");
    assert_eq!(NOTE_RECT, Rect::new(32, 83, 148, 227));
}

#[test]
fn spec014_fixture_j_lines_alignment_and_zones() {
    assert_eq!(line_y(0), 46);
    assert_eq!(line_y(40), 446);
    assert_eq!(line_y(41), 446, "later lines overprint line 40");
    // STATUS centered at x = 405; with holodisks right-aligned at 317 and
    // DATA at 489; BACK centered at 413 and right-aligned at 332; MORE at 491.
    assert_eq!(text_x(Align::Centered, STATUS), 405);
    assert_eq!(text_x(Align::LeftHeader, STATUS), 317);
    assert_eq!(text_x(Align::RightHeader, DATA), 489);
    assert_eq!(text_x(Align::Centered, BACK), 413);
    assert_eq!(text_x(Align::LeftHeader, BACK), 332);
    assert_eq!(text_x(Align::RightHeader, MORE), 491);
    assert_eq!(text_x(Align::Centered, 127), 365, "FEV Experiment Tape");
    assert_eq!(text_x(Align::Centered, 83), 387, "ALARM CLOCK");
    assert_eq!(text_x(Align::Centered, 103), 377, "Hit Points 25/30");
    assert_eq!(text_x(Align::Centered, 22), 418, "Hub");
    assert_eq!(text_x(Align::Normal, 0), 262);
    assert_eq!(text_x(Align::NoIndent, 0), 255);
    assert_eq!(text_x(Align::RightColumn, 0), 437);
    assert_eq!(counter_x(OF), 567);
    // The water-chip-only Status list: two zones from line 2.
    let status = zones(2, status_zone_count(1, 0));
    assert_eq!(
        status,
        [
            (Rect::new(254, 66, 350, 10), 1),
            (Rect::new(254, 86, 350, 10), 2)
        ]
    );
    assert_eq!(zone_row(2, 2, [300, 70]), Some(1));
    assert_eq!(zone_row(2, 2, [300, 90]), Some(2));
    assert_eq!(zone_row(2, 2, [300, 80]), None, "between lines");
    assert_eq!(zone_row(2, 2, [604, 70]), None);
    // At most 18 zones from line 2.
    assert_eq!(zones(2, 40).len(), 18);
    // The elevation view with two entries: rows -1, 0, 1, 2.
    let rows: Vec<_> = zones(0, 2 + 2)
        .iter()
        .map(|(rect, row)| (rect.y, *row))
        .collect();
    assert_eq!(rows, [(46, -1), (66, 0), (86, 1), (106, 2)]);
    // Alarm options from line 5: option j at y = 96 + 20j, row j + 4.
    let options = zones(OPTION_LINE, ALARM_OPTIONS.len());
    assert_eq!(options.len(), 13);
    assert_eq!(options[0], (Rect::new(254, 96, 350, 10), 4));
    assert_eq!(options[12].0.y, 336, "Rest until healed");
    assert_eq!(alarm_option(4), Some(0));
    assert_eq!(alarm_option(16), Some(12));
    assert_eq!(alarm_option(3), None);
    assert_eq!(alarm_option(17), None);
    assert_eq!(BACK_ZONE, Rect::new(254, 446, 350, 10));
}

#[test]
fn spec014_fixture_j_window_controls() {
    assert_eq!(Button::at([53, 341]), Some(Button::Status));
    assert_eq!(Button::at([67, 356]), Some(Button::Status));
    assert_eq!(Button::at([60, 370]), None, "the scratched-out label");
    assert_eq!(Button::at([60, 400]), Some(Button::Automaps));
    assert_eq!(Button::at([60, 430]), Some(Button::Archives));
    assert_eq!(Button::at([60, 455]), Some(Button::Close));
    assert_eq!(Button::at([130, 20]), Some(Button::Alarm));
    assert_eq!(Button::Alarm.sounds(), (ALARM_PRESS, ALARM_RELEASE));
    assert_eq!(Button::Close.art(), ("lilredup.frm", "lilreddn.frm"));
    assert_eq!(ART.len(), 11);
    assert!(!ART.contains(&"pipboy.frm"));
}

#[test]
fn spec014_fixture_j_status_clicks_and_reading_controls() {
    // Only the water-chip quest: row 1 opens Vault 13; row 2 (past the
    // last location) is ignored under the safe rule.
    assert_eq!(status_click(300, 1, 1, 0), Some(StatusClick::Location(1)));
    assert_eq!(status_click(300, 2, 1, 0), None);
    assert_eq!(status_click(500, 1, 1, 1), Some(StatusClick::Disk(1)));
    assert_eq!(status_click(500, 2, 1, 1), None);
    assert_eq!(status_click(DIVIDER_X, 1, 1, 1), None);
    // A three-page disk.
    use ReadingAction::*;
    let action = |input, page| reading_action(input, page, 2);
    assert_eq!(action(ReadingInput::Release(500), 0), Some(Next));
    assert_eq!(action(ReadingInput::Release(460), 1), Some(Next));
    assert_eq!(action(ReadingInput::PageDown, 2), None);
    assert_eq!(action(ReadingInput::Release(420), 2), None);
    assert_eq!(action(ReadingInput::Release(380), 2), Some(Previous));
    assert_eq!(action(ReadingInput::Release(500), 2), Some(List));
    assert_eq!(action(ReadingInput::Release(395), 0), Some(List));
    assert_eq!(action(ReadingInput::PageUp, 0), Some(List));
    assert_eq!(action(ReadingInput::PageUp, 1), Some(Previous));
    assert_eq!(action(ReadingInput::Release(459), 1), None);
}

#[test]
fn spec014_word_wrapper() {
    // One pixel per character plus nothing: a 10-wide line.
    let advance = |_: u8| 1;
    assert_eq!(wrap(b"short", 10, advance).unwrap(), [b"short".to_vec()]);
    // Breaks just after the last space or hyphen that fits.
    assert_eq!(
        wrap(b"aaaa bbbb-cccc dd", 10, advance).unwrap(),
        [b"aaaa bbbb-".to_vec(), b"cccc dd".to_vec()]
    );
    // With nothing remembered the line ends before the overflowing one.
    assert_eq!(
        wrap(b"abcdefghijkl", 10, advance).unwrap(),
        [b"abcdefghij".to_vec(), b"kl".to_vec()]
    );
    // A string of exactly the width is still one line.
    assert_eq!(wrap(b"abcdefghij", 10, advance).unwrap().len(), 1);
    // More than 63 lines fail.
    assert!(wrap(&[b'x'; 640], 10, advance).is_none());
    assert_eq!(wrap(&[b'x'; 630], 10, advance).unwrap().len(), 63);
}

#[test]
fn spec014_fixture_j_alarm_until_options() {
    // 07:21: until morning 22 h 39 min, until noon 4 h 39 min.
    assert_eq!(until(6, 7, 21), (22, 39));
    assert_eq!(until(12, 7, 21), (4, 39));
    // 06:00, 12:00 and 06:30, until morning.
    assert_eq!(until(6, 6, 0), (24, 0));
    assert_eq!(until(6, 12, 0), (18, 0));
    assert_eq!(until(6, 6, 30), (23, 30));
    // The core's wake-up preset agrees.
    for (hour, minute) in [(7, 21), (6, 0), (12, 0), (6, 30), (23, 59), (0, 0)] {
        for wake in [0, 6, 12, 18] {
            let ticks = (hour * 36000 + minute * 600) as u64;
            let (h, m) = until(wake, hour, minute);
            assert_eq!(minutes_until_hour(ticks, wake), 60 * h + m);
        }
    }
    assert_eq!(ALARM_OPTIONS[0], (302, RestRequest::Minutes(10)));
    assert_eq!(ALARM_OPTIONS[7], (309, RestRequest::Hours(6)));
    assert_eq!(ALARM_OPTIONS[11], (313, RestRequest::UntilHour(0)));
    assert_eq!(ALARM_OPTIONS[12], (314, RestRequest::UntilHealed));
}

#[test]
fn spec014_fixture_j_screen_saver() {
    let mut saver = ScreenSaver {
        warm_up: 0,
        ..ScreenSaver::default()
    };
    // Draws 100, 16383, 16383: a bomb at (602, 409) at speed 1.524958,
    // clipped to 2 by 43 in the same pass and moved to (600, 410).
    let mut draws = [100, 16383, 16383].into_iter();
    let cells = saver.pass(|| draws.next().unwrap()).unwrap();
    assert_eq!(
        cells,
        [Cell {
            source: Rect::new(0, 0, 2, 43),
            at: [602, 409]
        }]
    );
    let bomb = saver.bombs[0].unwrap();
    assert_eq!((bomb.x, bomb.y, bomb.progress), (600, 410, 0.0));
    assert_eq!(bomb.speed, 1.524_958_f32);
    // Draw 3048: no spawn, one draw.
    let mut count = 0;
    saver.pass(|| {
        count += 1;
        3048
    });
    assert_eq!(count, 1);
    assert!(saver.bombs[1].is_none());
    // Draws 100, 20000, 0: P = 441, start (317, -17) at speed 0.15; seven
    // passes reach progress 1.05 and move it to (315, -15).
    let mut saver = ScreenSaver {
        warm_up: 0,
        ..ScreenSaver::default()
    };
    let mut draws = [100, 20000, 0].into_iter();
    let cells = saver.pass(|| draws.next().unwrap()).unwrap();
    // Above the area: 63 rows skipped, 2 drawn from y = 46.
    assert_eq!(
        cells,
        [Cell {
            source: Rect::new(0, 63, 65, 2),
            at: [317, 46]
        }]
    );
    assert_eq!(saver.bombs[0].unwrap().speed, 0.15_f32);
    for _ in 1..7 {
        saver.pass(|| 32767);
    }
    let bomb = saver.bombs[0].unwrap();
    assert_eq!((bomb.x, bomb.y, bomb.progress), (315, -15, 0.0));
    // The warm-up draws nothing for 50 passes.
    let mut saver = ScreenSaver::default();
    for _ in 0..SAVER_WARM_UP {
        assert!(saver.pass(|| 32767).is_none());
    }
    assert!(saver.pass(|| 32767).is_some());
    // A bomb left of the area frees its slot once 65 columns are skipped.
    let mut saver = ScreenSaver {
        warm_up: 0,
        ..ScreenSaver::default()
    };
    saver.bombs[3] = Some(Bomb {
        x: 189,
        y: 100,
        speed: 0.0,
        progress: 0.0,
    });
    assert!(saver.pass(|| 32767).unwrap().is_empty());
    assert!(saver.bombs[3].is_none());
}

/// SPEC-020 §2.1: the Pip-Boy's layout and page state machine.
mod modal {
    use super::*;

    fn counts() -> PageCounts {
        PageCounts {
            locations: 2,
            disks: 1,
            areas: 3,
            entries: 2,
            archives: 2,
        }
    }
    /// A press and release at one point.
    fn click(pipboy: &mut PipBoyModal, point: [i32; 2], now: u64) -> PipBoyOutcome {
        pipboy.press(point, &counts(), now);
        pipboy.release(Some(point), &counts(), now)
    }

    #[test]
    fn buttons_and_zones_layout() {
        let pipboy = PipBoyModal::open(0);
        // The start screen has the window buttons only.
        let start = pipboy.layout(&counts());
        assert_eq!(start.len(), 5);
        assert_eq!(
            start.rect(PipBoyControl::Button(Button::Status)),
            Some(Rect::new(53, 341, 15, 16))
        );
        assert_eq!(
            start.rect(PipBoyControl::Button(Button::Alarm)),
            Some(Rect::new(124, 13, 28, 24))
        );
        let mut status = pipboy.clone();
        status.show_tab(Tab::Status);
        assert_eq!(status.zone_layout(&counts()), (2, 4, false));
        let layout = status.layout(&counts());
        assert_eq!(layout.len(), 9);
        assert_eq!(
            layout.rect(PipBoyControl::Zone(Zone::Row(1))),
            Some(Rect::new(254, 66, 350, 10))
        );
        assert_eq!(
            layout.rect(PipBoyControl::Zone(Zone::Row(4))),
            Some(Rect::new(254, 126, 350, 10))
        );
        assert_eq!(layout.hit([300, 76]), None, "lines between zones");
        let mut detail = pipboy;
        detail.page = Some(Page::Status(StatusView::Detail(7)));
        assert_eq!(
            detail
                .layout(&counts())
                .rect(PipBoyControl::Zone(Zone::Back)),
            Some(BACK_ZONE)
        );
        assert_eq!(detail.zone_at([300, 450], &counts()), Some(Zone::Back));
    }

    #[test]
    fn page_buttons_act_on_release_over_themselves() {
        let mut pipboy = PipBoyModal::open(0);
        assert_eq!(
            pipboy.press([54, 342], &counts(), 0),
            PipBoyOutcome::Pressed(Button::Status)
        );
        assert_eq!(pipboy.pressed(), Some(Button::Status));
        assert_eq!(
            pipboy.release(Some([0, 0]), &counts(), 0),
            PipBoyOutcome::None
        );
        assert_eq!((pipboy.page, pipboy.pressed()), (None, None));
        assert_eq!(click(&mut pipboy, [54, 342], 0), PipBoyOutcome::Redraw);
        assert_eq!(pipboy.page.map(|page| page.tab()), Some(Tab::Status));
        assert_eq!(click(&mut pipboy, [54, 396], 0), PipBoyOutcome::Redraw);
        assert_eq!(pipboy.page, Some(Page::Automaps(AutomapView::List)));
        assert_eq!(click(&mut pipboy, [54, 450], 0), PipBoyOutcome::Close);
        // The Alarm Clock waits for the eligibility test.
        assert_eq!(
            click(&mut pipboy, [125, 14], 0),
            PipBoyOutcome::AlarmRequested
        );
        assert_eq!(pipboy.page, Some(Page::Automaps(AutomapView::List)));
        pipboy.show_alarm();
        assert_eq!(pipboy.page, Some(Page::Alarm));
    }

    #[test]
    fn status_rows_flash_then_act() {
        let mut pipboy = PipBoyModal::open(0);
        pipboy.show_tab(Tab::Status);
        // A location row (left of x = 429) reports its row.
        assert_eq!(click(&mut pipboy, [300, 67], 0), PipBoyOutcome::Location(1));
        // A press and a release on different zones do nothing.
        pipboy.press([300, 67], &counts(), 0);
        assert_eq!(
            pipboy.release(Some([300, 87]), &counts(), 0),
            PipBoyOutcome::None
        );
        assert_eq!(
            pipboy.flash(1000, Highlight::Location(1), Deferred::Detail(7)),
            PipBoyOutcome::Flashing
        );
        // Input waits during the flash.
        assert_eq!(pipboy.key(PipBoyKey::Escape, 1100), PipBoyOutcome::None);
        assert_eq!(
            pipboy.press([54, 342], &counts(), 1100),
            PipBoyOutcome::None
        );
        assert!(!pipboy.tick(1199));
        assert!(pipboy.tick(1200));
        assert_eq!(pipboy.page, Some(Page::Status(StatusView::Detail(7))));
        // Back from the detail flashes, then returns to the list.
        assert_eq!(
            click(&mut pipboy, [300, 450], 2000),
            PipBoyOutcome::Flashing
        );
        assert_eq!(
            pipboy.flash.map(|flash| flash.highlight),
            Some(Highlight::Back)
        );
        assert!(pipboy.tick(2200));
        assert_eq!(pipboy.page, Some(Page::Status(StatusView::List)));
        // A holodisk row (right of x = 429), then its reading view.
        assert_eq!(click(&mut pipboy, [500, 67], 3000), PipBoyOutcome::Disk(1));
        pipboy.flash(
            3000,
            Highlight::Disk(1),
            Deferred::Reading {
                disk: 3,
                last_page: 1,
            },
        );
        pipboy.tick(3200);
        assert_eq!(
            pipboy.page,
            Some(Page::Status(StatusView::Reading {
                disk: 3,
                page: 0,
                last_page: 1
            }))
        );
        assert_eq!(
            pipboy.key(PipBoyKey::PageDown, 4000),
            PipBoyOutcome::Flashing
        );
        pipboy.tick(4200);
        assert!(matches!(
            pipboy.page,
            Some(Page::Status(StatusView::Reading { page: 1, .. }))
        ));
        // PageDown on the last page does nothing; MORE there returns to
        // the list.
        assert_eq!(pipboy.key(PipBoyKey::PageDown, 5000), PipBoyOutcome::None);
        assert_eq!(
            click(&mut pipboy, [470, 450], 5000),
            PipBoyOutcome::Flashing
        );
        pipboy.tick(5200);
        assert_eq!(pipboy.page, Some(Page::Status(StatusView::List)));
        // Page keys outside a holodisk do nothing.
        assert_eq!(pipboy.key(PipBoyKey::PageUp, 6000), PipBoyOutcome::None);
    }

    #[test]
    fn automaps_archives_and_rest() {
        let mut pipboy = PipBoyModal::open(0);
        pipboy.show_tab(Tab::Automaps);
        assert_eq!(click(&mut pipboy, [300, 67], 0), PipBoyOutcome::Selected);
        assert_eq!(
            pipboy.page,
            Some(Page::Automaps(AutomapView::Elevations {
                area: 0,
                entry: 0
            }))
        );
        assert_eq!(pipboy.zone_layout(&counts()), (0, 4, true));
        assert_eq!(click(&mut pipboy, [300, 107], 0), PipBoyOutcome::Selected);
        assert_eq!(
            pipboy.page,
            Some(Page::Automaps(AutomapView::Elevations {
                area: 0,
                entry: 1
            }))
        );
        // The title row (-1) goes back to the area list.
        assert_eq!(click(&mut pipboy, [300, 47], 0), PipBoyOutcome::Selected);
        assert_eq!(pipboy.page, Some(Page::Automaps(AutomapView::List)));
        // A row past the last area does nothing.
        let few = PageCounts {
            areas: 1,
            ..counts()
        };
        pipboy.press([300, 87], &few, 0);
        assert_eq!(
            pipboy.release(Some([300, 87]), &few, 0),
            PipBoyOutcome::None
        );
        pipboy.show_tab(Tab::Archives);
        assert_eq!(click(&mut pipboy, [300, 67], 0), PipBoyOutcome::Archive(1));
        // The Alarm Clock's first option is on line 5 and reports row 4.
        pipboy.show_alarm();
        assert_eq!(pipboy.zone_layout(&counts()), (OPTION_LINE, 13, false));
        assert_eq!(click(&mut pipboy, [300, 97], 0), PipBoyOutcome::Rest(0));
        pipboy.begin_rest(0);
        assert_eq!(pipboy.press([54, 342], &counts(), 0), PipBoyOutcome::None);
        assert_eq!(pipboy.key(PipBoyKey::Close, 0), PipBoyOutcome::None);
        assert!(!pipboy.rest_due(49));
        assert!(pipboy.rest_due(50));
        assert!(!pipboy.rest_due(60));
        assert!(pipboy.rest_due(100));
        assert_eq!(pipboy.key(PipBoyKey::Escape, 120), PipBoyOutcome::StopRest);
        pipboy.end_rest();
        assert!(!pipboy.rest_due(1000));
        assert_eq!(
            pipboy.key(PipBoyKey::Quit, 2000),
            PipBoyOutcome::QuitConfirm
        );
        assert!(pipboy.close_after_dialog);
        assert_eq!(pipboy.key(PipBoyKey::Escape, 2000), PipBoyOutcome::Close);
        assert_eq!(pipboy.key(PipBoyKey::F12, 2000), PipBoyOutcome::Screenshot);
    }

    #[test]
    fn screen_saver_starts_idle_and_input_ends_it() {
        let mut pipboy = PipBoyModal::open(0);
        assert!(!pipboy.saver_due(IDLE_MS, false));
        assert!(pipboy.saver_due(IDLE_MS + 1, false));
        assert!(!pipboy.saver_due(IDLE_MS + 1, true), "not under a dialog");
        pipboy.saver_pass(IDLE_MS + 1, || 32767);
        let saver = pipboy.saver.as_ref().unwrap();
        assert_eq!((saver.state.warm_up, saver.last_pass), (0, IDLE_MS + 1));
        assert!(!pipboy.saver_due(IDLE_MS + 50, false));
        assert!(pipboy.saver_due(IDLE_MS + 51, false));
        // The input that ends the saver is consumed.
        let now = IDLE_MS + 60;
        assert_eq!(
            pipboy.press([54, 342], &counts(), now),
            PipBoyOutcome::Redraw
        );
        assert!(pipboy.saver.is_none() && pipboy.pressed().is_none());
        assert_eq!(pipboy.idle_since, now);
        assert_eq!(
            pipboy.press([54, 342], &counts(), now),
            PipBoyOutcome::Pressed(Button::Status)
        );
    }
}
