//! SPEC-013 §3.21 the elevator window: background, optional panel, the
//! level buttons and the gauge. A level button plays `ib1p1xx1` on the
//! press and reports on the release over it; a chosen level plays the
//! travel sound and moves the player (or requests the map change); Esc
//! cancels and, under the safe rule, changes nothing.
use super::{MapPreview, Screen, modal_kit::at};
use crate::{
    AppResult,
    input::{key_digit, key_text},
    interface::Painter,
};
use vault_core::{
    skills,
    ui::{BUTTON_PRESS, Rect},
};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

pub(crate) struct ElevatorScreen {
    pub kind: i32,
    pub start: usize,
    /// G-18 the travel animation under way: the chosen level, the gauge
    /// cells it draws, when it started and the wait after each cell.
    pub travel: Option<Travel>,
}
pub(crate) struct Travel {
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

const BUTTON: [i32; 2] = [48, 50];

/// Level button `i`'s screen rectangle in the window at `origin`.
fn elevator_button(origin: [i32; 2], i: usize) -> Rect {
    let [x, y] = skills::elevator_button(i);
    at(origin, Rect::new(x, y, BUTTON[0], BUTTON[1]))
}

impl MapPreview {
    /// A deferred elevator request opens the window over the world.
    pub(crate) fn open_elevator(&mut self, kind: i32) {
        if self.host.elevator_view(kind).is_some() {
            let start = self.host.elevator_start_level(kind);
            self.screen = Screen::Elevator(ElevatorScreen {
                kind,
                start,
                travel: None,
            });
        }
    }
    fn elevator_origin(&self, painter: &Painter, kind: i32) -> [i32; 2] {
        let background = self
            .host
            .elevator_view(kind)
            .map_or_else(|| "el_mil1.frm".to_owned(), |e| e.background);
        skills::elevator_origin(painter.size(&background).unwrap_or([230, 284]))
    }
    /// The level buttons' screen rectangles, in level order.
    fn elevator_buttons(&self, kind: i32) -> Vec<Rect> {
        let origin = self.elevator_origin(&self.painter(), kind);
        let buttons = self.host.elevator_view(kind).map_or(0, |e| e.buttons());
        (0..buttons).map(|i| elevator_button(origin, i)).collect()
    }
    pub(crate) fn elevator_image(&mut self) -> AppResult<IndexedImage> {
        let Screen::Elevator(screen) = &self.screen else {
            return Err("no elevator".into());
        };
        let (kind, start) = (screen.kind, screen.start);
        let moving = screen
            .travel
            .as_ref()
            .and_then(|travel| travel.cell(self.clock));
        let elevator = self.host.elevator_view(kind).ok_or("invalid elevator")?;
        let view = self.map_view_image()?;
        let mut image = self.compose_world(view);
        let painter = self.painter();
        let origin = self.elevator_origin(&painter, kind);
        painter.blit_opaque(&mut image, &elevator.background, 0, origin);
        if let Some(panel) = &elevator.panel {
            let height = painter.size(&elevator.background).map_or(284, |[_, h]| h);
            let panel_height = painter.size(panel).map_or(248, |[_, h]| h);
            painter.blit_opaque(
                &mut image,
                panel,
                0,
                [origin[0], origin[1] + height - panel_height],
            );
        }
        for i in 0..elevator.buttons() {
            let rect = elevator_button(origin, i);
            self.draw_button(&painter, &mut image, rect, ("ebut_out.frm", "ebut_in.frm"));
        }
        let (gauge, [gx, gy], [w, h]) = skills::GAUGE;
        let cell = moving.unwrap_or_else(|| elevator.gauge_cell(start));
        painter.blit_region(
            &mut image,
            gauge,
            Rect::new(0, h * cell, w, h),
            [origin[0] + gx, origin[1] + gy],
            true,
        );
        Ok(image)
    }
    /// A chosen level (or Esc). G-18 / SPEC-013 §3.21: a level other than
    /// the start one plays the travel sound and runs the travel animation
    /// first (the gauge steps by `(n - 1) / 12` cells every 23, 46 or 69 ms,
    /// then a 200 ms pause; safe rule: the exact target cell ends it); the
    /// window then closes and the player moves.
    fn choose_level(&mut self, choice: Option<usize>) {
        let Screen::Elevator(screen) = &mut self.screen else {
            return;
        };
        let Some(elevator) = self.host.elevator_view(screen.kind) else {
            self.screen = Screen::Map;
            return;
        };
        if let Some(level) = choice
            && level != screen.start
        {
            screen.travel = Some(Travel {
                level,
                cells: elevator.gauge_path(screen.start, level),
                start: self.clock,
                step: elevator.step_ms(),
                shown: None,
            });
            if let Some(sound) = elevator.travel_sound(level.abs_diff(screen.start)) {
                self.play_effect(sound.as_bytes(), None);
            }
            return;
        }
        self.perform_level(choice);
    }
    fn perform_level(&mut self, choice: Option<usize>) {
        let Screen::Elevator(screen) = std::mem::replace(&mut self.screen, Screen::Map) else {
            return;
        };
        if let Err(error) = self.host.perform_elevator(screen.kind, choice) {
            self.status = error.to_string().into_bytes();
        }
    }
    /// One frame of the travel animation; it busy-waits, so the window
    /// takes no input meanwhile. Returns whether the frame changed.
    pub(crate) fn elevator_tick(&mut self, now: u64) -> bool {
        let Screen::Elevator(ElevatorScreen {
            travel: Some(travel),
            ..
        }) = &mut self.screen
        else {
            return false;
        };
        if now >= travel.end() {
            let level = travel.level;
            self.perform_level(Some(level));
            return true;
        }
        let cell = travel.cell(now);
        std::mem::replace(&mut travel.shown, cell) != cell
    }
    fn elevator_moving(&self) -> bool {
        matches!(
            &self.screen,
            Screen::Elevator(ElevatorScreen {
                travel: Some(_),
                ..
            })
        )
    }
    pub(crate) fn elevator_click(&mut self, point: [i32; 2]) -> bool {
        let Screen::Elevator(screen) = &self.screen else {
            return false;
        };
        if self.elevator_moving() {
            return true;
        }
        if let Some(rect) = self
            .elevator_buttons(screen.kind)
            .into_iter()
            .find(|rect| rect.contains(point))
        {
            self.held = Some(rect);
            self.play_effect(BUTTON_PRESS.as_bytes(), None);
        }
        true
    }
    /// A left release over the held level button (`over`) chooses it.
    pub(crate) fn elevator_release(&mut self, over: Option<Rect>) -> bool {
        let Screen::Elevator(screen) = &self.screen else {
            return false;
        };
        if self.elevator_moving() {
            return true;
        }
        let buttons = self.elevator_buttons(screen.kind);
        let Some(level) = buttons.iter().position(|rect| over == Some(*rect)) else {
            return false;
        };
        self.choose_level(Some(level));
        true
    }
    pub(crate) fn elevator_key(&mut self, key: KeyCode) -> bool {
        let Screen::Elevator(screen) = &self.screen else {
            return false;
        };
        if self.elevator_moving() {
            return true;
        }
        if key == KeyCode::Escape {
            self.choose_level(None);
            return true;
        }
        // The number row, the numeric keypad's digits and the letters (upper
        // case) all type their character (the keypad's operators type none).
        let character =
            key_digit(key).or_else(|| key_text(key, true).filter(u8::is_ascii_uppercase));
        let level = character.and_then(|c| self.host.elevator_view(screen.kind)?.button_for_key(c));
        match level {
            Some(level) => {
                self.choose_level(Some(level));
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
impl MapPreview {
    /// Whether the elevator window is open.
    pub(crate) fn elevator_open(&self) -> bool {
        matches!(self.screen, Screen::Elevator(_))
    }
}
