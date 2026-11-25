//! SPEC-020 §2.6: the hotkey toasts. Short hints that teach the controls, one
//! at a time, in an amber plate like the enhanced preset's HUD messages.
//!
//! A toast is shell state: it reads the session and writes nothing to it.
//! The shell draws the plate over a copy of the composed frame, in the
//! launcher's font, so no frame, screenshot or digest ever holds one. Only
//! the desktop shell owns a [`HotkeyToasts`], so a replay, a smoke test and a
//! gallery capture have none.
use crate::{
    launcher::font,
    map_preview::{BAYER, MESSAGE_FADE_MS},
};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// The first hint, after the map screen first appears.
const FIRST_MS: u64 = 2000;
/// How long a hint shows whole, and the gap after it.
const SHOW_MS: u64 = 4000;
const GAP_MS: u64 = 1000;
/// A hint: the key, whether it needs Alt, and the exact text.
pub type Hint = (KeyCode, bool, &'static str);

/// §2.6's hints in order, with neither conditional row's condition: the
/// last, `Alt+L`, shows only with both editions found.
pub const HINTS: [Hint; 7] = [
    (KeyCode::F1, false, "F1: Help"),
    (KeyCode::KeyI, false, "I: Inventory"),
    (KeyCode::KeyS, false, "S: Skilldex"),
    (KeyCode::KeyP, false, "P: Pip-Boy"),
    (KeyCode::KeyZ, false, "Z: Wait (Pip-Boy clock)"),
    (KeyCode::F9, false, "F9: Debugger"),
    (KeyCode::KeyL, true, "Alt+L: Switch game"),
];

/// §2.6's recentre hint: after row 6 (`F9`) and before `Alt+L`, shown only
/// while the session's `camera_follow` is `off`.
pub const CENTER_VIEW: Hint = (KeyCode::Home, false, "Home: Center view");

/// The rows unconditionally shown, before the two conditional ones.
const ALWAYS: usize = 6;

/// The plate's height, and its top edge, as a HUD message's.
const PLATE_HEIGHT: i32 = 16;
const PLATE_TOP: i64 = 62;
/// The first palette-cycling index: a plate stays below it.
const CYCLING: usize = 229;
/// The dissolve's steps, as a HUD message's: 0 whole, 16 gone.
const STEPS: u64 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// The time left before the next hint.
    Waiting(u64),
    /// How long this hint has shown.
    Showing(u64),
    Done,
}

/// What shows now: the hint's index and its dissolve step.
pub type Shown = (usize, u8);

/// The list a session works through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyToasts {
    /// This session's rows in order: the six of [`HINTS`] always shown,
    /// [`CENTER_VIEW`], then `Alt+L` with both editions found, so `Alt+L`
    /// is last whichever rows the session has.
    rows: Vec<Hint>,
    /// The session's `camera_follow` is `off`: [`CENTER_VIEW`] shows. It
    /// is read when the row comes up, so Alt+C before then decides it.
    center_view: bool,
    at: usize,
    phase: Phase,
    /// The shell clock at the last tick.
    last: Option<u64>,
    /// The timer ran at the last tick, so the time since it counts.
    running: bool,
    drawn: Option<Shown>,
    /// Finished and not yet reported.
    finished: bool,
}

impl HotkeyToasts {
    /// The list, with `Alt+L` when both editions were found. The recentre
    /// row waits for [`Self::set_center_view`]: until then it is skipped.
    pub fn new(both_editions: bool) -> Self {
        let mut rows = HINTS[..ALWAYS].to_vec();
        rows.push(CENTER_VIEW);
        if both_editions {
            rows.push(HINTS[ALWAYS]);
        }
        Self {
            rows,
            center_view: false,
            at: 0,
            phase: Phase::Waiting(FIRST_MS),
            last: None,
            running: false,
            drawn: None,
            finished: false,
        }
    }
    /// One tick of the shell clock: the timer runs while `ready` and stops
    /// otherwise, hiding the hint. Returns whether what shows changed.
    pub fn tick(&mut self, now: u64, ready: bool) -> bool {
        let last = self.last.replace(now).unwrap_or(now);
        if std::mem::replace(&mut self.running, ready) && ready {
            self.advance(now.saturating_sub(last));
        }
        let drawn = if ready { self.current() } else { None };
        std::mem::replace(&mut self.drawn, drawn) != drawn
    }
    fn advance(&mut self, mut elapsed: u64) {
        while elapsed > 0 {
            match self.phase {
                Phase::Waiting(left) if left > elapsed => {
                    self.phase = Phase::Waiting(left - elapsed);
                    return;
                }
                Phase::Waiting(left) => {
                    elapsed -= left;
                    self.phase = Phase::Showing(0);
                }
                Phase::Showing(shown) if shown + elapsed < SHOW_MS + MESSAGE_FADE_MS => {
                    self.phase = Phase::Showing(shown + elapsed);
                    return;
                }
                Phase::Showing(shown) => {
                    elapsed -= SHOW_MS + MESSAGE_FADE_MS - shown;
                    self.next();
                }
                Phase::Done => return,
            }
        }
    }
    /// Whether the session's `camera_follow` is `off`, so that the recentre
    /// row shows. Turned off while that row waits or shows, it is passed
    /// over, as if it had shown.
    pub fn set_center_view(&mut self, on: bool) {
        self.center_view = on;
        if !on && self.phase != Phase::Done && self.rows.get(self.at) == Some(&CENTER_VIEW) {
            self.next();
        }
    }
    /// The rows this session shows now, in order.
    #[cfg(test)]
    fn rows(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.rows
            .iter()
            .filter(|row| self.center_view || **row != CENTER_VIEW)
            .map(|row| row.2)
    }
    /// This hint is over: the next one this session shows.
    fn next(&mut self) {
        self.at += 1;
        while !self.center_view && self.rows.get(self.at) == Some(&CENTER_VIEW) {
            self.at += 1;
        }
        if self.at >= self.rows.len() {
            self.phase = Phase::Done;
            self.finished = true;
        } else {
            self.phase = Phase::Waiting(GAP_MS);
        }
    }
    fn current(&self) -> Option<Shown> {
        let Phase::Showing(shown) = self.phase else {
            return None;
        };
        let fading = shown.saturating_sub(SHOW_MS);
        Some((
            self.at,
            (fading * STEPS / MESSAGE_FADE_MS).min(STEPS - 1) as u8,
        ))
    }
    /// What the last tick showed.
    pub fn shown(&self) -> Option<Shown> {
        self.drawn
    }
    pub fn text(&self) -> Option<&'static str> {
        self.drawn.map(|(index, _)| self.rows[index].2)
    }
    /// A key press: the shown hint's key ends it at once. No key's action
    /// changes. Returns whether what shows changed.
    pub fn key(&mut self, key: KeyCode, alt: bool) -> bool {
        let matches = self.drawn.is_some_and(|(index, _)| {
            let (hint, needs_alt, _) = self.rows[index];
            hint == key && needs_alt == alt
        });
        if matches {
            self.next();
            self.drawn = None;
        }
        matches
    }
    /// Once, when the list has been finished.
    pub fn take_finished(&mut self) -> bool {
        std::mem::take(&mut self.finished)
    }
}

/// The plate's two inks in the palette a frame shows through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Inks {
    amber: u8,
    black: u8,
}

impl Inks {
    /// The nearest entries below the cycling range, index 0 excluded,
    /// because a plate's 0 is see-through.
    pub fn nearest(palette: &[[u8; 4]; 256]) -> Self {
        let near = |target: [i32; 3]| {
            (1..CYCLING)
                .min_by_key(|index| {
                    let color = palette[*index];
                    (0..3)
                        .map(|axis| (i32::from(color[axis]) - target[axis]).pow(2))
                        .sum::<i32>()
                })
                .unwrap_or(1) as u8
        };
        Self {
            amber: near([255, 192, 64]),
            black: near([0, 0, 0]),
        }
    }
}

/// The plate of `text`: amber edge, black face and amber text, as a HUD
/// message's, dissolving through the same dither. Index 0 is see-through.
pub fn plate(text: &str, step: u8, inks: Inks) -> IndexedImage {
    let width = font::width(text) + 16;
    let mut image =
        IndexedImage::new(width as u32, PLATE_HEIGHT as u32).expect("a plate's size is valid");
    image.fill_rect([1, 0, width - 2, PLATE_HEIGHT], inks.amber);
    image.fill_rect([0, 1, width, PLATE_HEIGHT - 2], inks.amber);
    image.fill_rect([1, 1, width - 2, PLATE_HEIGHT - 2], inks.black);
    font::draw(&mut image, [8, 4], text, inks.amber);
    if step > 0 {
        let stride = width as usize;
        for (at, pixel) in image.pixels_mut().iter_mut().enumerate() {
            if BAYER[(at / stride) % 4][(at % stride) % 4] < step {
                *pixel = 0;
            }
        }
    }
    image
}

/// Where a plate sits on a frame `frame_width` wide: centered, near the top.
pub fn plate_origin(frame_width: u32, plate: &IndexedImage) -> [i64; 2] {
    [
        (i64::from(frame_width) - i64::from(plate.width())) / 2,
        PLATE_TOP,
    ]
}

/// `frame` with `plate` over it. The frame itself is left as it was.
pub fn over(frame: &IndexedImage, plate: &IndexedImage) -> IndexedImage {
    let mut shown = frame.clone();
    shown.blit_image(plate, plate_origin(frame.width(), plate));
    shown
}

#[cfg(test)]
#[path = "toast_tests.rs"]
mod tests;

/// SPEC-020 §2.6's recentre row, under each condition of the list.
#[cfg(test)]
mod center_view_tests {
    use super::*;

    /// The texts in the order they first show, `ready` always true.
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

    fn list(both_editions: bool, center_view: bool) -> HotkeyToasts {
        let mut toasts = HotkeyToasts::new(both_editions);
        toasts.set_center_view(center_view);
        toasts
    }

    #[test]
    fn spec020_the_center_view_row_shows_under_off_only() {
        let six: Vec<_> = HINTS[..6].iter().map(|hint| hint.2).collect();
        for both in [false, true] {
            // Not `off`: the list as before the row was added.
            let shown = texts(&mut list(both, false), 0, 80_000);
            let mut want = six.clone();
            if both {
                want.push("Alt+L: Switch game");
            }
            assert_eq!(shown, want, "both editions: {both}");
            assert_eq!(list(both, false).rows().collect::<Vec<_>>(), want);
            // `off`: the row after row 6, and `Alt+L` still last.
            let shown = texts(&mut list(both, true), 0, 80_000);
            let mut want = six.clone();
            want.push("Home: Center view");
            if both {
                want.push("Alt+L: Switch game");
            }
            assert_eq!(shown, want, "both editions: {both}");
            assert_eq!(list(both, true).rows().collect::<Vec<_>>(), want);
        }
    }

    #[test]
    fn spec020_the_lists_end_is_reported_under_each_condition() {
        for both in [false, true] {
            for center in [false, true] {
                let mut toasts = list(both, center);
                texts(&mut toasts, 0, 80_000);
                assert!(toasts.take_finished(), "{both} {center}");
                assert!(!toasts.tick(90_000, true));
            }
        }
    }

    /// Home ends its own hint, and a mode change while the row waits passes
    /// over it.
    #[test]
    fn spec020_the_center_view_row_follows_the_mode_in_play() {
        let mut toasts = list(false, true);
        toasts.tick(0, true);
        for key in [
            KeyCode::F1,
            KeyCode::KeyI,
            KeyCode::KeyS,
            KeyCode::KeyP,
            KeyCode::KeyZ,
        ] {
            toasts.tick(toasts.last.unwrap() + 2000, true);
            assert!(toasts.key(key, false));
        }
        toasts.tick(toasts.last.unwrap() + 2000, true);
        assert_eq!(toasts.text(), Some("F9: Debugger"));
        assert!(toasts.key(KeyCode::F9, false));
        toasts.tick(toasts.last.unwrap() + 1000, true);
        assert_eq!(toasts.text(), Some("Home: Center view"));
        assert!(toasts.key(KeyCode::Home, false));
        assert!(toasts.take_finished(), "one edition: the row is the last");

        // Alt+C to `smooth` while the row waits: it is passed over.
        let mut toasts = list(true, true);
        // F9, the sixth, shows from 29.5 to 34 seconds; the gap follows.
        texts(&mut toasts, 0, 34_500);
        assert_eq!(toasts.text(), None, "the gap after F9");
        assert_eq!(toasts.rows[toasts.at], CENTER_VIEW);
        toasts.set_center_view(false);
        let rest = texts(&mut toasts, 34_500, 60_000);
        assert_eq!(rest, ["Alt+L: Switch game"]);
    }
}
