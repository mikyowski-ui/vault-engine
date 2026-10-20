//! SPEC-013 §3.21 the elevator window: background, optional panel, the
//! level buttons and the gauge. A level button plays `ib1p1xx1` on the
//! press and reports on the release over it; a chosen level plays the
//! travel sound and moves the player (or requests the map change); Esc
//! cancels and, under the safe rule, changes nothing.
use super::{MapPreview, Screen};
use crate::{AppResult, interface::Painter};
use vault_core::{
    skills,
    ui::{BUTTON_PRESS, Rect},
};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

pub(crate) struct ElevatorScreen {
    pub kind: i32,
    pub start: usize,
}

const BUTTON: [i32; 2] = [48, 50];

/// Level button `i`'s screen rectangle in the window at `origin`.
fn elevator_button(origin: [i32; 2], i: usize) -> Rect {
    let [x, y] = skills::elevator_button(i);
    Rect::new(origin[0] + x, origin[1] + y, BUTTON[0], BUTTON[1])
}

impl MapPreview {
    /// A deferred elevator request opens the window over the world.
    pub(crate) fn open_elevator(&mut self, kind: i32) {
        if skills::elevator(kind).is_some() {
            let start = self.host.elevator_start_level(kind);
            self.screen = Screen::Elevator(ElevatorScreen { kind, start });
        }
    }
    fn elevator_origin(&self, painter: &Painter, kind: i32) -> [i32; 2] {
        let background = skills::elevator(kind).map_or("el_mil1.frm", |e| e.background);
        skills::elevator_origin(painter.size(background).unwrap_or([230, 284]))
    }
    pub(crate) fn elevator_image(&mut self) -> AppResult<IndexedImage> {
        let Screen::Elevator(screen) = &self.screen else {
            return Err("no elevator".into());
        };
        let (kind, start) = (screen.kind, screen.start);
        let elevator = skills::elevator(kind).ok_or("invalid elevator")?;
        let view = self.map_view_image()?;
        let mut image = self.compose_world(view);
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        let origin = self.elevator_origin(&painter, kind);
        painter.blit_opaque(&mut image, elevator.background, 0, origin);
        if let Some(panel) = elevator.panel {
            let height = painter.size(elevator.background).map_or(284, |[_, h]| h);
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
            let art = self.button_art(rect, "ebut_out.frm", "ebut_in.frm");
            painter.blit(&mut image, art, 0, [rect.x, rect.y]);
        }
        let (gauge, [gx, gy], [w, h]) = skills::GAUGE;
        let cell = elevator.gauge_cell(start);
        painter.blit_region(
            &mut image,
            gauge,
            Rect::new(0, h * cell, w, h),
            [origin[0] + gx, origin[1] + gy],
            true,
        );
        Ok(image)
    }
    fn choose_level(&mut self, choice: Option<usize>) {
        let Screen::Elevator(screen) = std::mem::replace(&mut self.screen, Screen::Map) else {
            return;
        };
        let Some(elevator) = skills::elevator(screen.kind) else {
            return;
        };
        if let Some(level) = choice
            && level != screen.start
            && let Some(sound) = elevator.travel_sound(level.abs_diff(screen.start))
        {
            self.play_effect(sound.as_bytes(), None);
        }
        if let Err(error) = self.host.perform_elevator(screen.kind, choice) {
            self.status = error.to_string().into_bytes();
        }
    }
    pub(crate) fn elevator_click(&mut self, point: [i32; 2]) -> bool {
        let Screen::Elevator(screen) = &self.screen else {
            return false;
        };
        let kind = screen.kind;
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        let origin = self.elevator_origin(&painter, kind);
        let buttons = skills::elevator(kind).map_or(0, |e| e.buttons());
        if let Some(rect) = (0..buttons)
            .map(|i| elevator_button(origin, i))
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
        let kind = screen.kind;
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        let origin = self.elevator_origin(&painter, kind);
        let buttons = skills::elevator(kind).map_or(0, |e| e.buttons());
        let Some(level) = (0..buttons).find(|i| over == Some(elevator_button(origin, *i))) else {
            return false;
        };
        self.choose_level(Some(level));
        true
    }
    pub(crate) fn elevator_key(&mut self, key: KeyCode) -> bool {
        let Screen::Elevator(screen) = &self.screen else {
            return false;
        };
        if key == KeyCode::Escape {
            self.choose_level(None);
            return true;
        }
        let name = format!("{key:?}");
        let character = name
            .strip_prefix("Digit")
            .or_else(|| name.strip_prefix("Key"))
            .and_then(|rest| rest.bytes().next());
        let level = character.and_then(|c| skills::elevator(screen.kind)?.button_for_key(c));
        match level {
            Some(level) => {
                self.choose_level(Some(level));
                true
            }
            None => false,
        }
    }
}
