//! SPEC-020 §2.6's hints: their list, timing and text, and that a plate
//! never reaches a frame. `tests/fixtures/spec020/hotkey-hints.tsv` is the
//! list. The session's side is in `warm_switch_tests`.
use super::*;

/// The list run with `ready` always true, sampled every 50 ms from `start`
/// to `end`: the texts in the order they first show.
fn texts(toasts: &mut HotkeyToasts, start: u64, end: u64) -> Vec<&'static str> {
    let mut out = Vec::new();
    for now in (start..end).step_by(50) {
        toasts.tick(now, true);
        if let Some(text) = toasts.text()
            && out.last() != Some(&text)
        {
            out.push(text);
        }
    }
    out
}

/// Every row of `hotkey-hints.tsv`, with the seventh's condition.
#[test]
fn spec020_the_hints_are_the_fixtures_in_order() {
    let table = include_str!("../../../tests/fixtures/spec020/hotkey-hints.tsv");
    let rows: Vec<Vec<&str>> = table
        .lines()
        .skip(1)
        .map(|row| row.split('\t').collect())
        .collect();
    assert_eq!(rows.len(), HINTS.len());
    for (row, (key, alt, text)) in rows.iter().zip(HINTS) {
        assert_eq!(row[2], text);
        assert_eq!(row[1].starts_with("Alt+"), alt);
        assert!(format!("{key:?}").ends_with(row[1].rsplit('+').next().unwrap()));
    }
    let list = texts(&mut HotkeyToasts::new(true), 0, 60_000);
    assert_eq!(list, HINTS.map(|(_, _, text)| text));
    let list = texts(&mut HotkeyToasts::new(false), 0, 60_000);
    assert_eq!(list.len(), 6);
    assert!(!list.contains(&"Alt+L: Switch game"), "both editions only");
}

#[test]
fn spec020_a_hint_shows_at_two_seconds_for_four_and_a_half() {
    let mut toasts = HotkeyToasts::new(true);
    assert!(!toasts.tick(1000, true), "the first tick starts the timer");
    assert!(!toasts.tick(2999, true));
    assert_eq!(toasts.shown(), None, "not before 2 seconds");
    assert!(toasts.tick(3000, true));
    assert_eq!(toasts.shown(), Some((0, 0)));
    assert_eq!(toasts.text(), Some("F1: Help"));
    // Whole for 4 seconds, then the dissolve's 16 steps in 0.5 seconds.
    assert!(!toasts.tick(6999, true));
    assert_eq!(toasts.shown(), Some((0, 0)));
    assert!(toasts.tick(7250, true));
    assert_eq!(toasts.shown(), Some((0, 8)));
    assert!(toasts.tick(7500, true));
    assert_eq!(toasts.shown(), None, "gone at 4.5 seconds");
    // A 1-second gap.
    assert!(!toasts.tick(8499, true));
    assert_eq!(toasts.shown(), None);
    assert!(toasts.tick(8500, true));
    assert_eq!(toasts.text(), Some("I: Inventory"));
}

/// The timer is stopped, not reset, and the hint hides while it is.
#[test]
fn spec020_the_timer_stops_while_the_map_is_not_free() {
    let mut toasts = HotkeyToasts::new(false);
    toasts.tick(0, false);
    // Combat, a window or the main menu for a minute: nothing shows.
    for now in (0..60_000).step_by(100) {
        assert!(!toasts.tick(now, false));
    }
    assert_eq!(toasts.shown(), None);
    toasts.tick(60_000, true);
    toasts.tick(61_999, true);
    assert_eq!(toasts.shown(), None, "the 2 seconds start now");
    assert!(toasts.tick(62_000, true));
    assert_eq!(toasts.text(), Some("F1: Help"));
    // A window opens over the hint: it hides and keeps its place.
    toasts.tick(63_000, true);
    assert!(toasts.tick(63_001, false));
    assert_eq!(toasts.shown(), None);
    assert!(!toasts.tick(90_000, false));
    assert!(toasts.tick(90_001, true));
    assert_eq!(toasts.shown(), Some((0, 0)), "the same hint comes back");
    // It has 3 seconds of its 4 left.
    toasts.tick(92_500, true);
    assert_eq!(toasts.shown(), Some((0, 0)));
    toasts.tick(93_100, true);
    assert_ne!(toasts.shown(), Some((0, 0)));
}

/// A press ends the shown hint's key only, and says so to the shell.
#[test]
fn spec020_the_shown_hints_key_ends_it() {
    let mut toasts = HotkeyToasts::new(true);
    toasts.tick(0, true);
    assert!(!toasts.key(KeyCode::F1, false), "nothing shows yet");
    toasts.tick(2000, true);
    assert!(!toasts.key(KeyCode::KeyI, false), "another hint's key");
    assert!(!toasts.key(KeyCode::F1, true), "F1 with Alt is not its key");
    assert!(toasts.key(KeyCode::F1, false));
    assert_eq!(toasts.shown(), None);
    toasts.tick(2999, true);
    assert_eq!(toasts.shown(), None, "the gap");
    toasts.tick(3000, true);
    assert_eq!(toasts.text(), Some("I: Inventory"));
    // Walk to the seventh, which needs Alt.
    for (index, key) in [
        (2, KeyCode::KeyI),
        (3, KeyCode::KeyS),
        (4, KeyCode::KeyP),
        (5, KeyCode::KeyZ),
        (6, KeyCode::F9),
    ] {
        assert!(toasts.key(key, false));
        toasts.tick(3000 + index * 1000, true);
    }
    assert_eq!(toasts.text(), Some("Alt+L: Switch game"));
    assert!(!toasts.key(KeyCode::KeyL, false), "L alone is the game's");
    assert!(toasts.key(KeyCode::KeyL, true));
    assert!(toasts.take_finished());
}

#[test]
fn spec020_finishing_is_reported_once() {
    let mut toasts = HotkeyToasts::new(false);
    assert!(!toasts.take_finished());
    texts(&mut toasts, 0, 40_000);
    assert!(toasts.take_finished());
    assert!(!toasts.take_finished());
    assert!(!toasts.tick(50_000, true), "nothing shows after the list");
}

/// A long stall on the clock does not lose a hint, or the order.
#[test]
fn spec020_a_stalled_clock_walks_the_list() {
    let mut toasts = HotkeyToasts::new(false);
    toasts.tick(0, true);
    toasts.tick(9000, true);
    assert_eq!(toasts.text(), Some("I: Inventory"));
    toasts.tick(1_000_000, true);
    assert_eq!(toasts.shown(), None);
    assert!(toasts.take_finished());
}

fn palette() -> [[u8; 4]; 256] {
    let mut palette = [[0, 0, 0, 255]; 256];
    for (index, entry) in palette.iter_mut().enumerate() {
        *entry = [index as u8, index as u8 / 2, 64, 255];
    }
    palette
}

/// The shell draws on a copy, so the frame a screenshot or a digest reads
/// is the same with toasts on and off.
#[test]
fn spec020_a_plate_is_drawn_over_a_copy() {
    let inks = Inks::nearest(&palette());
    let frame = IndexedImage::from_pixels(640, 480, vec![7; 640 * 480]).unwrap();
    let before = frame.pixels().to_vec();
    let plate = plate("F1: Help", 0, inks);
    let shown = over(&frame, &plate);
    assert_eq!(frame.pixels(), before, "the frame is left as it was");
    assert_ne!(shown.pixels(), before);
    // Near the top, centered, in the same place as a HUD message.
    let [x, y] = plate_origin(640, &plate);
    assert_eq!(y, 62);
    assert_eq!(x, (640 - i64::from(plate.width())) / 2);
    let at = |x: i64, y: i64| shown.pixels()[(y * 640 + x) as usize];
    assert_eq!(at(x + 1, y), inks.amber, "the edge");
    assert_eq!(at(x + 1, y + 1), inks.black, "the face");
    assert_eq!(at(x, y), 7, "a rounded corner shows the frame");
}

#[test]
fn spec020_a_plate_dissolves_to_nothing_and_stays_below_the_cycling_range() {
    let inks = Inks::nearest(&palette());
    assert!(usize::from(inks.amber) < CYCLING && inks.amber != 0);
    assert!(usize::from(inks.black) < CYCLING && inks.black != 0);
    let opaque = |step: u8| {
        let plate = plate("Z: Wait (Pip-Boy clock)", step, inks);
        plate.pixels().iter().filter(|pixel| **pixel != 0).count()
    };
    assert!(opaque(0) > opaque(8));
    assert!(opaque(8) > opaque(15));
}
