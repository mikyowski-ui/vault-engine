//! SPEC-013 §3.15 the Skilldex window over the world: eight skill rows
//! with their effective values, and Cancel. A row's press holds the window
//! open `trunc(1000 / 9)` ms; then Sneak toggles, another row enters that
//! skill's targeting mode, or, opened from the action menu's Skill entry
//! (SPEC-014 §3.28), uses the skill on that target.
use super::{MapPreview, Screen};
use crate::{AppResult, interface::Painter};
use vault_core::{
    ObjectRef, skills,
    ui::{BUTTON_PRESS, ColorKey, Rect},
};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

pub(crate) struct SkilldexScreen {
    /// The action menu's target.
    target: Option<ObjectRef>,
    /// The chosen skill and the clock at its press: the window closes
    /// `CLOSE_DELAY_MS` later, discarding input meanwhile.
    chosen: Option<(i32, u64)>,
}

const ROW: [i32; 2] = [86, 33];
const CANCEL: Rect = Rect::new(48, 338, 15, 16);
/// A row button waits `trunc(1000 / 9)` ms before the window closes.
const CLOSE_DELAY_MS: u64 = 1000 / 9;

/// Row button `row`'s screen rectangle.
fn row_rect(row: usize) -> Rect {
    let [x, y] = skills::skilldex_button(row);
    let [ox, oy] = skills::SKILLDEX_ORIGIN;
    Rect::new(ox + x, oy + y, ROW[0], ROW[1])
}
/// Cancel's screen rectangle.
fn cancel_rect() -> Rect {
    let [ox, oy] = skills::SKILLDEX_ORIGIN;
    Rect::new(ox + CANCEL.x, oy + CANCEL.y, CANCEL.w, CANCEL.h)
}

impl MapPreview {
    pub(crate) fn open_skilldex(&mut self) {
        self.open_skilldex_on(None);
    }
    pub(crate) fn open_skilldex_on(&mut self, target: Option<ObjectRef>) {
        if matches!(self.screen, Screen::Map) {
            self.screen = Screen::Skilldex(SkilldexScreen {
                target,
                chosen: None,
            });
        }
    }
    pub(crate) fn skilldex_image(&mut self) -> AppResult<IndexedImage> {
        let view = self.map_view_image()?;
        let mut image = self.compose_world(view);
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        let [ox, oy] = skills::SKILLDEX_ORIGIN;
        painter.blit_opaque(&mut image, "skldxbox.frm", 0, [ox, oy]);
        let title = painter.message("skilldex.msg", 100);
        painter.text(
            &mut image,
            103,
            ColorKey::LABEL,
            &title,
            [ox + 55, oy + 14],
            120,
        );
        let player = self.host.world.player;
        let context = self.host.world.context(player);
        let font_height = painter.line_height(103);
        for (row, skill) in skills::SKILLDEX_SKILLS.iter().enumerate() {
            let rect = row_rect(row);
            let art = self.button_art(rect, "skldxoff.frm", "skldxon.frm");
            painter.blit_opaque(&mut image, art, 0, [rect.x, rect.y]);
            // The label is (18, 17, 3) on the up image, (14, 12, 3) down.
            let key = if art == "skldxon.frm" {
                ColorKey::LABEL_PRESSED
            } else {
                ColorKey::LABEL
            };
            let label = painter.message("skilldex.msg", 102 + row as i32);
            let [lx, ly] = skills::skilldex_label(painter.width(103, &label), font_height);
            painter.text(
                &mut image,
                103,
                key,
                &label,
                [rect.x + lx, rect.y + ly],
                ROW[0],
            );
            let value = self
                .host
                .world
                .player_sheet()
                .map_or(0, |sheet| sheet.skill(*skill as usize, context));
            for (digit, x) in skills::skilldex_digits(value).iter().zip([110, 124, 138]) {
                painter.big_digit(
                    &mut image,
                    *digit,
                    [ox + x, oy + skills::skilldex_counter_y(row)],
                    false,
                );
            }
        }
        let cancel = painter.message("skilldex.msg", 101);
        painter.text(
            &mut image,
            103,
            ColorKey::LABEL,
            &cancel,
            [ox + 72, oy + 337],
            80,
        );
        let cancel = cancel_rect();
        let art = self.button_art(cancel, "lilredup.frm", "lilreddn.frm");
        painter.blit(&mut image, art, 0, [cancel.x, cancel.y]);
        Ok(image)
    }
    fn close_skilldex(&mut self, skill: Option<i32>) {
        let Screen::Skilldex(screen) = std::mem::replace(&mut self.screen, Screen::Map) else {
            return;
        };
        let Some(skill) = skill else {
            return;
        };
        match screen.target {
            Some(target) if skill != skills::skill::SNEAK => {
                let player = self.host.world.player;
                if let Err(error) = self.host.use_skill_on(player, target, skill) {
                    self.status = error.to_string().into_bytes();
                }
                self.drain_effects();
            }
            _ => self.choose_skill(skill),
        }
    }
    pub(crate) fn skilldex_click(&mut self, point: [i32; 2]) -> bool {
        let clock = self.clock;
        let Screen::Skilldex(screen) = &mut self.screen else {
            return false;
        };
        if screen.chosen.is_some() {
            return true;
        }
        if let Some(row) = (0..8).find(|row| row_rect(*row).contains(point)) {
            screen.chosen = Some((skills::SKILLDEX_SKILLS[row], clock));
            self.press_button_cue(b"ib3p1xx1", b"ib3lu1x1", row_rect(row));
            return true;
        }
        if cancel_rect().contains(point) {
            self.press_button(cancel_rect());
            self.close_skilldex(None);
        }
        true
    }
    /// The chosen row's wait: the window closes `CLOSE_DELAY_MS` after the
    /// press.
    pub(crate) fn skilldex_tick(&mut self) -> bool {
        let Screen::Skilldex(SkilldexScreen {
            chosen: Some((skill, at)),
            ..
        }) = self.screen
        else {
            return false;
        };
        if self.clock < at + CLOSE_DELAY_MS {
            return false;
        }
        self.close_skilldex(Some(skill));
        true
    }
    /// Esc cancels; Enter also cancels after `ib1p1xx1`.
    pub(crate) fn skilldex_key(&mut self, key: KeyCode) -> bool {
        if matches!(&self.screen, Screen::Skilldex(screen) if screen.chosen.is_some()) {
            return true;
        }
        match key {
            KeyCode::Escape => self.close_skilldex(None),
            KeyCode::Enter | KeyCode::NumpadEnter => {
                self.play_effect(BUTTON_PRESS.as_bytes(), None);
                self.close_skilldex(None);
            }
            _ => return false,
        }
        true
    }
}
