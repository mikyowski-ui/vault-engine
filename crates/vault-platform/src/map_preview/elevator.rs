//! SPEC-013 §3.21 the elevator window: background, optional panel, the
//! level buttons and the gauge. A level button plays `ib1p1xx1` on the
//! press and reports on the release over it; a chosen level plays the
//! travel sound and moves the player (or requests the map change); Esc
//! cancels and, under the safe rule, changes nothing.
//!
//! SPEC-020 §2.1: the window's placement, level buttons, held button and
//! travel animation are `vault_core::ui::elevator`'s; this draws it, plays
//! its sounds and performs the choice.
use super::{MapPreview, Screen};
use crate::{
    AppResult,
    input::{key_digit, key_text},
};
use vault_core::{
    skills,
    ui::{
        BUTTON_PRESS, Rect,
        elevator::{BUTTON_ART, ElevatorKey, ElevatorOutcome, ElevatorWindow},
    },
};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

impl MapPreview {
    /// A deferred elevator request opens the window over the world.
    pub(crate) fn open_elevator(&mut self, kind: i32) {
        if let Some(view) = self.host.elevator_view(kind) {
            let start = self.host.elevator_start_level(kind);
            let background = self.painter().size(&view.background);
            self.screen = Screen::Elevator(Box::new(ElevatorWindow::open(
                kind, view, start, background,
            )));
        }
    }
    pub(crate) fn elevator_image(&mut self) -> AppResult<IndexedImage> {
        if !matches!(self.screen, Screen::Elevator(_)) {
            return Err("no elevator".into());
        }
        let view = self.map_view_image()?;
        let mut image = self.compose_world(view);
        let Screen::Elevator(window) = &self.screen else {
            return Err("no elevator".into());
        };
        let painter = self.painter();
        let elevator = &window.view;
        let origin = window.origin();
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
        for (_, rect) in window.layout().controls() {
            self.draw_button(&painter, &mut image, *rect, BUTTON_ART);
        }
        let (gauge, [gx, gy], [w, h]) = skills::GAUGE;
        let cell = window.gauge_cell(self.clock);
        painter.blit_region(
            &mut image,
            gauge,
            Rect::new(0, h * cell, w, h),
            [origin[0] + gx, origin[1] + gy],
            true,
        );
        Ok(image)
    }
    /// Act on the core window's outcome: the travel sound, or the window
    /// closes and the player moves (or the map change is requested).
    /// Returns whether the input was taken.
    fn elevator_outcome(&mut self, outcome: ElevatorOutcome) -> bool {
        match outcome {
            ElevatorOutcome::None => false,
            ElevatorOutcome::Busy | ElevatorOutcome::Pressed(_) | ElevatorOutcome::Redraw => true,
            ElevatorOutcome::Travel { sound } => {
                if let Some(sound) = sound {
                    self.play_effect(sound.as_bytes(), None);
                }
                true
            }
            ElevatorOutcome::Perform(choice) => {
                if let Screen::Elevator(window) = std::mem::replace(&mut self.screen, Screen::Map)
                    && let Err(error) = self.host.perform_elevator(window.kind, choice)
                {
                    self.status = error.to_string().into_bytes();
                }
                true
            }
        }
    }
    /// One frame of the travel animation; it busy-waits, so the window
    /// takes no input meanwhile. Returns whether the frame changed.
    pub(crate) fn elevator_tick(&mut self, now: u64) -> bool {
        let Screen::Elevator(window) = &mut self.screen else {
            return false;
        };
        let outcome = window.tick(now);
        self.elevator_outcome(outcome)
    }
    /// A press on a level button holds it (`ib1p1xx1`, its down art).
    pub(crate) fn elevator_click(&mut self, point: [i32; 2]) -> bool {
        let Screen::Elevator(window) = &mut self.screen else {
            return false;
        };
        if let ElevatorOutcome::Pressed(level) = window.press(point) {
            self.held = window.layout().rect(level);
            self.play_effect(BUTTON_PRESS.as_bytes(), None);
        }
        true
    }
    /// A left release at `point` over the held level button (`over`, the
    /// held rectangle under the release) chooses it.
    pub(crate) fn elevator_release(&mut self, point: Option<[i32; 2]>, over: Option<Rect>) -> bool {
        let clock = self.clock;
        let Screen::Elevator(window) = &mut self.screen else {
            return false;
        };
        let outcome = window.release(over.and(point), clock);
        self.elevator_outcome(outcome)
    }
    pub(crate) fn elevator_key(&mut self, key: KeyCode) -> bool {
        let clock = self.clock;
        let Screen::Elevator(window) = &mut self.screen else {
            return false;
        };
        // The number row, the numeric keypad's digits and the letters (upper
        // case) all type their character (the keypad's operators type none).
        let input = if key == KeyCode::Escape {
            ElevatorKey::Escape
        } else {
            key_digit(key)
                .or_else(|| key_text(key, true).filter(u8::is_ascii_uppercase))
                .map_or(ElevatorKey::Other, ElevatorKey::Char)
        };
        let outcome = window.key(input, clock);
        self.elevator_outcome(outcome)
    }
}

#[cfg(test)]
impl MapPreview {
    /// Whether the elevator window is open.
    pub(crate) fn elevator_open(&self) -> bool {
        matches!(self.screen, Screen::Elevator(_))
    }
}
