//! SPEC-014 §3.2 palette fades in the interactive world: the library
//! `fadein` / `fadeout` and `gfade_in` / `gfade_out` requests, the First
//! Aid and Doctor fades (SPEC-013 §3.13), and the SPEC-012 §3.12 game-movie
//! fade flags. Fades run one after another, each from the levels shown when
//! it starts; step k of n sets each level to `old - trunc((old - new) * k /
//! n)` and the target is installed exactly at the end. Under the safe rule
//! a fade lasts a fixed 700 ms instead of a calibrated step count; a library
//! fade with a positive duration lasts that many milliseconds.
//!
//! A game fade blocks completely (§3.2): while one runs, or waits to run
//! next, the world does not update and input is swallowed, and while it
//! fades to black the frame on screen stays the one shown when it was
//! requested. Library fades service background work, so they do not block.
use std::collections::VecDeque;
use vault_core::ui::flow;
use vault_formats::{Palette, palette::display_rgb6};

/// The standard fade's length.
pub const FADE_MS: u64 = 700;
/// Steps of a fade (the reference's uncalibrated count).
const STEPS: u32 = 60;
use flow::BLACK;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Request {
    to_black: bool,
    from_black: bool,
    duration: u64,
    /// A game fade, which blocks.
    blocking: bool,
}

#[derive(Debug, Clone, Copy)]
struct Active {
    from: [[u8; 3]; 256],
    to_black: bool,
    start: u64,
    duration: u64,
    blocking: bool,
}

/// Queued and running fades, and whether black replaces the game palette.
#[derive(Debug, Default)]
pub(crate) struct Fades {
    queue: VecDeque<Request>,
    active: Option<Active>,
    /// A finished fade to black (or a movie) left the display black.
    black: bool,
    /// The shell clock at the last update.
    now: u64,
}

fn levels(palette: &Palette) -> [[u8; 3]; 256] {
    std::array::from_fn(|index| palette.rgb6(index as u8))
}

impl Fades {
    /// Queue a game fade (`gfade_out` / `gfade_in`, First Aid, Doctor and
    /// skill books, the movie fade flags): the standard fade, which blocks.
    pub fn push_game(&mut self, out: bool) {
        self.queue.push_back(Request {
            to_black: out,
            from_black: false,
            duration: FADE_MS,
            blocking: true,
        });
    }
    /// Queue a library `fadein` (from black) or `fadeout` over `duration`
    /// ms, 0 or less being the standard length; it does not block.
    pub fn push_library(&mut self, out: bool, duration: i32, from_black: bool) {
        self.queue.push_back(Request {
            to_black: out,
            from_black,
            duration: u64::try_from(duration)
                .ok()
                .filter(|ms| *ms > 0)
                .unwrap_or(FADE_MS),
            blocking: false,
        });
    }
    /// The fade running now, or else the one that starts next.
    fn current(&self) -> Option<(bool, bool)> {
        match (&self.active, self.queue.front()) {
            (Some(active), _) => Some((active.blocking, active.to_black)),
            (None, Some(request)) => Some((request.blocking, request.to_black)),
            (None, None) => None,
        }
    }
    /// A game fade runs, or starts next: the world waits and input is
    /// swallowed.
    pub fn blocking(&self) -> bool {
        self.current().is_some_and(|(blocking, _)| blocking)
    }
    /// A blocking fade to black runs or starts next: the frame stays as it
    /// was when the fade was requested.
    pub fn freezes(&self) -> bool {
        self.current() == Some((true, true))
    }
    /// Show black at once (a movie's end sets the palette to black).
    pub fn set_black(&mut self) {
        self.queue.clear();
        self.active = None;
        self.black = true;
    }
    #[cfg(test)]
    pub fn running(&self) -> bool {
        self.active.is_some() || !self.queue.is_empty()
    }
    /// Advance to `now`: start the next queued fade, finish the running one.
    /// Returns whether the shown levels changed.
    pub fn tick(&mut self, now: u64, game: &Palette) -> bool {
        self.now = now;
        if self.active.is_none()
            && let Some(request) = self.queue.pop_front()
        {
            let from = if request.from_black {
                BLACK
            } else {
                self.shown(game)
            };
            self.active = Some(Active {
                from,
                to_black: request.to_black,
                start: now,
                duration: request.duration,
                blocking: request.blocking,
            });
            return true;
        }
        let Some(active) = self.active else {
            return false;
        };
        if now.saturating_sub(active.start) >= active.duration {
            self.black = active.to_black;
            self.active = None;
        }
        true
    }
    /// The six-bit levels shown now.
    fn shown(&self, game: &Palette) -> [[u8; 3]; 256] {
        if let Some(active) = &self.active {
            let target = if active.to_black { BLACK } else { levels(game) };
            let elapsed = self.now.saturating_sub(active.start).min(active.duration);
            let step = (elapsed * u64::from(STEPS) / active.duration.max(1)) as u32;
            return flow::fade_palette(&active.from, &target, step, STEPS);
        }
        if self.black { BLACK } else { levels(game) }
    }
    /// The display palette while a fade runs or black is shown; `None` when
    /// the game palette shows as usual (and palette cycling may run).
    pub fn display(&self, game: &Palette, gamma: f64) -> Option<[[u8; 4]; 256]> {
        if self.active.is_none() && !self.black {
            return None;
        }
        Some(self.shown(game).map(|rgb6| display_rgb6(rgb6, gamma)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette {
        let mut bytes = vec![0; 768 + 32768];
        for entry in bytes[..768].chunks_mut(3) {
            entry.copy_from_slice(&[63, 40, 1]);
        }
        Palette::parse(&bytes).unwrap()
    }

    #[test]
    fn fades_step_from_the_shown_levels_and_install_the_target() {
        let game = palette();
        let mut fades = Fades::default();
        assert_eq!(fades.display(&game, 1.0), None);
        fades.push_game(true);
        fades.push_game(false);
        // Game fades block from their request to their end, and the frame
        // stays while the screen goes black.
        assert!(fades.blocking() && fades.freezes());
        assert!(fades.tick(1000, &game));
        // Halfway (step 30 of 60): 63 - trunc(63 * 30 / 60) = 32.
        fades.tick(1350, &game);
        assert_eq!(fades.shown(&game)[5], [32, 20, 1]);
        // The end installs black, the next tick starts the fade back.
        assert!(fades.tick(1700, &game));
        assert_eq!(fades.shown(&game)[5], [0, 0, 0]);
        assert!(fades.display(&game, 1.0).is_some());
        // The fade back still blocks, but the frame may change under black.
        assert!(fades.blocking() && !fades.freezes());
        assert!(fades.tick(1716, &game));
        assert!(fades.tick(2416, &game));
        assert!(!fades.running() && !fades.blocking());
        assert_eq!(fades.display(&game, 1.0), None);
        // `fadein` starts from black even when the game palette shows, and
        // a library duration replaces the standard length; it never blocks.
        fades.push_library(false, 100, true);
        assert!(!fades.blocking());
        fades.tick(3000, &game);
        fades.tick(3050, &game);
        assert_eq!(fades.shown(&game)[0], [31, 20, 0]);
        fades.tick(3100, &game);
        assert_eq!(fades.display(&game, 1.0), None);
    }
}
