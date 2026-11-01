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
/// §2.6's hints in order: the key, whether it needs Alt, and the exact text.
pub const HINTS: [(KeyCode, bool, &str); 7] = [
    (KeyCode::F1, false, "F1: Help"),
    (KeyCode::KeyI, false, "I: Inventory"),
    (KeyCode::KeyS, false, "S: Skilldex"),
    (KeyCode::KeyP, false, "P: Pip-Boy"),
    (KeyCode::KeyZ, false, "Z: Wait (Pip-Boy clock)"),
    (KeyCode::F9, false, "F9: Debugger"),
    (KeyCode::KeyL, true, "Alt+L: Switch game"),
];

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
    /// How many of [`HINTS`] this session has.
    count: usize,
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
    /// The list, with the seventh hint when both editions were found.
    pub fn new(both_editions: bool) -> Self {
        Self {
            count: HINTS.len() - usize::from(!both_editions),
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
    /// This hint is over.
    fn next(&mut self) {
        self.at += 1;
        if self.at >= self.count {
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
        self.drawn.map(|(index, _)| HINTS[index].2)
    }
    /// A key press: the shown hint's key ends it at once. No key's action
    /// changes. Returns whether what shows changed.
    pub fn key(&mut self, key: KeyCode, alt: bool) -> bool {
        let matches = self.drawn.is_some_and(|(index, _)| {
            let (hint, needs_alt, _) = HINTS[index];
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
