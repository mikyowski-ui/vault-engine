//! SPEC-013 §3.15 the Skilldex window over the world: eight skill rows
//! with their effective values, and Cancel. Sneak toggles at once; another
//! row enters that skill's targeting mode, or, opened from the action
//! menu's Skill entry (SPEC-014 §3.28), uses the skill on that target.
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
}

const ROW: [i32; 2] = [86, 33];
const CANCEL: Rect = Rect::new(48, 338, 15, 16);

fn row_rect(row: usize) -> Rect {
    let [x, y] = skills::skilldex_button(row);
    Rect::new(x, y, ROW[0], ROW[1])
}

impl MapPreview {
    pub(crate) fn open_skilldex(&mut self) {
        self.open_skilldex_on(None);
    }
    pub(crate) fn open_skilldex_on(&mut self, target: Option<ObjectRef>) {
        if matches!(self.screen, Screen::Map) {
            self.screen = Screen::Skilldex(SkilldexScreen { target });
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
            let [x, y] = skills::skilldex_button(row);
            painter.blit_opaque(&mut image, "skldxoff.frm", 0, [ox + x, oy + y]);
            let label = painter.message("skilldex.msg", 102 + row as i32);
            let [lx, ly] = skills::skilldex_label(painter.width(103, &label), font_height);
            painter.text(
                &mut image,
                103,
                ColorKey::LABEL,
                &label,
                [ox + x + lx, oy + y + ly],
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
        painter.blit(
            &mut image,
            "lilredup.frm",
            0,
            [ox + CANCEL.x, oy + CANCEL.y],
        );
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
        let [ox, oy] = skills::SKILLDEX_ORIGIN;
        let local = [point[0] - ox, point[1] - oy];
        if let Some(row) = (0..8).find(|row| row_rect(*row).contains(local)) {
            self.play_effect(b"ib3p1xx1", None);
            self.play_effect(b"ib3lu1x1", None);
            self.close_skilldex(Some(skills::SKILLDEX_SKILLS[row]));
            return true;
        }
        if CANCEL.contains(local) {
            self.press();
            self.close_skilldex(None);
        }
        true
    }
    /// Esc cancels; Enter also cancels after `ib1p1xx1`.
    pub(crate) fn skilldex_key(&mut self, key: KeyCode) -> bool {
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
