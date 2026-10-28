//! SPEC-013 §3.21 the elevator window: its placement, the level buttons'
//! layout, the press-then-release choice, the keys, and the G-18 travel
//! animation's pacing (SPEC-020 §2.1). A level button plays `BUTTON_PRESS`
//! on the press and reports on the release over it; a chosen level other
//! than the start one plays the travel sound and runs the gauge animation
//! (it busy-waits, so the window takes no input meanwhile), then the
//! caller performs the move; Esc cancels and, under the safe rule,
//! changes nothing.
use super::{Armed, Layout, Rect};
use crate::skills::{self, ElevatorView};

/// A level button's size.
pub const BUTTON_SIZE: [i32; 2] = [48, 50];
/// The level buttons' up and down art.
pub const BUTTON_ART: (&str, &str) = ("ebut_out.frm", "ebut_in.frm");
/// The window's size when its background art is missing.
pub const DEFAULT_SIZE: [i32; 2] = [230, 284];

/// Level button `level`'s screen rectangle in the window at `origin`.
pub fn button_rect(origin: [i32; 2], level: usize) -> Rect {
    Rect::at(skills::elevator_button(level), BUTTON_SIZE).offset(origin)
}
/// The level buttons on screen by level, for the window at `origin`.
pub fn layout(origin: [i32; 2], buttons: usize) -> Layout<usize> {
    (0..buttons)
        .map(|level| (level, button_rect(origin, level)))
        .collect()
}

/// What an elevator input leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevatorOutcome {
    /// Not accepted: the caller passes it on.
    None,
    /// Swallowed: the travel animation is under way.
    Busy,
    /// A level button is held (`BUTTON_PRESS`, its down art).
    Pressed(usize),
    /// The travel animation started: play `sound` when there is one.
    Travel { sound: Option<&'static str> },
    /// Close the window and perform the choice (`None`: cancel).
    Perform(Option<usize>),
    /// The travel animation drew a new gauge cell.
    Redraw,
}
/// A key in the elevator window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevatorKey {
    Escape,
    /// The character a key types: the digits (number row or keypad) and
    /// the upper-case letters.
    Char(u8),
    Other,
}

/// G-18 the travel animation under way: the chosen level, the gauge cells
/// it draws, when it started and the wait after each cell.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Travel {
    level: usize,
    cells: Vec<i32>,
    start: u64,
    step: u64,
    /// The cell the last frame drew.
    shown: Option<i32>,
}
impl Travel {
    /// The cell drawn at `now`: one per step, the last held to the end.
    fn cell(&self, now: u64) -> Option<i32> {
        let index = (now.saturating_sub(self.start) / self.step.max(1)) as usize;
        self.cells
            .get(index.min(self.cells.len().saturating_sub(1)))
            .copied()
    }
    /// SPEC-013 §3.21: each drawn cell is followed by its busy-wait, and
    /// the last by the 200 ms pause.
    fn end(&self) -> u64 {
        self.start + self.cells.len() as u64 * self.step + skills::ELEVATOR_PAUSE_MS
    }
}

/// §3.21 the open window: the elevator, its start level and placement,
/// the held level button and the travel animation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElevatorWindow {
    /// The elevator type the caller performs the choice with.
    pub kind: i32,
    pub view: ElevatorView,
    /// The level the gauge shows at the opening.
    pub start: usize,
    origin: [i32; 2],
    armed: Armed<usize>,
    travel: Option<Travel>,
}
impl ElevatorWindow {
    /// Open elevator `kind` at level `start`; `background` is the
    /// background art's size (`None`: [`DEFAULT_SIZE`]).
    pub fn open(kind: i32, view: ElevatorView, start: usize, background: Option<[i32; 2]>) -> Self {
        Self {
            kind,
            view,
            start,
            origin: skills::elevator_origin(background.unwrap_or(DEFAULT_SIZE)),
            armed: Armed::new(),
            travel: None,
        }
    }
    /// The window's top-left on screen.
    pub fn origin(&self) -> [i32; 2] {
        self.origin
    }
    /// The level buttons on screen, by level.
    pub fn layout(&self) -> Layout<usize> {
        layout(self.origin, self.view.buttons())
    }
    /// The held level button.
    pub fn pressed(&self) -> Option<usize> {
        self.armed.pressed()
    }
    /// The travel animation is under way.
    pub fn moving(&self) -> bool {
        self.travel.is_some()
    }
    /// The gauge cell drawn at `now`: the travel animation's, else the
    /// start level's.
    pub fn gauge_cell(&self, now: u64) -> i32 {
        self.travel
            .as_ref()
            .and_then(|travel| travel.cell(now))
            .unwrap_or_else(|| self.view.gauge_cell(self.start))
    }
    /// A press holds the level button under it.
    pub fn press(&mut self, point: [i32; 2]) -> ElevatorOutcome {
        if self.moving() {
            return ElevatorOutcome::Busy;
        }
        match self.armed.press(self.layout().hit(point)) {
            Some(level) => ElevatorOutcome::Pressed(level),
            None => ElevatorOutcome::Busy,
        }
    }
    /// A release over the held level button chooses it at `now`.
    pub fn release(&mut self, point: Option<[i32; 2]>, now: u64) -> ElevatorOutcome {
        if self.moving() {
            return ElevatorOutcome::Busy;
        }
        let over = point.and_then(|point| self.layout().hit(point));
        match self.armed.release(over) {
            Some(level) => self.choose(Some(level), now),
            None => ElevatorOutcome::None,
        }
    }
    /// Esc cancels; a key whose character is a button's chooses it.
    pub fn key(&mut self, key: ElevatorKey, now: u64) -> ElevatorOutcome {
        if self.moving() {
            return ElevatorOutcome::Busy;
        }
        match key {
            ElevatorKey::Escape => self.choose(None, now),
            ElevatorKey::Char(c) => match self.view.button_for_key(c) {
                Some(level) => self.choose(Some(level), now),
                None => ElevatorOutcome::None,
            },
            ElevatorKey::Other => ElevatorOutcome::None,
        }
    }
    /// A chosen level (or Esc) at `now`. G-18 / SPEC-013 §3.21: a level
    /// other than the start one runs the travel animation first (the
    /// gauge steps by `(n - 1) / 12` cells every 23, 46 or 69 ms, then a
    /// 200 ms pause; safe rule: the exact target cell ends it); the start
    /// level and Esc are performed at once.
    pub fn choose(&mut self, choice: Option<usize>, now: u64) -> ElevatorOutcome {
        match choice {
            Some(level) if level != self.start => {
                self.travel = Some(Travel {
                    level,
                    cells: self.view.gauge_path(self.start, level),
                    start: now,
                    step: self.view.step_ms(),
                    shown: None,
                });
                ElevatorOutcome::Travel {
                    sound: self.view.travel_sound(level.abs_diff(self.start)),
                }
            }
            choice => ElevatorOutcome::Perform(choice),
        }
    }
    /// One frame of the travel animation at `now`: `Perform` once it has
    /// ended, `Redraw` when the drawn cell changed.
    pub fn tick(&mut self, now: u64) -> ElevatorOutcome {
        let Some(travel) = &mut self.travel else {
            return ElevatorOutcome::None;
        };
        if now >= travel.end() {
            return ElevatorOutcome::Perform(Some(travel.level));
        }
        let cell = travel.cell(now);
        if std::mem::replace(&mut travel.shown, cell) != cell {
            ElevatorOutcome::Redraw
        } else {
            ElevatorOutcome::None
        }
    }
}
