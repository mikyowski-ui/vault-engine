//! SPEC-013 §3.15 the Skilldex window over the world: eight skill rows
//! with their effective values, and Cancel. A row's press holds the window
//! open `trunc(1000 / 9)` ms; then Sneak toggles, another row enters that
//! skill's targeting mode, or, opened from the action menu's Skill entry
//! (SPEC-014 §3.28), uses the skill on that target.
//!
//! SPEC-020 §2.1: the window's layout, the chosen row's wait and the
//! action it leads to are `vault_core::ui::skilldex`'s; this draws it and
//! applies the action.
use super::{MapPreview, Screen};
use crate::AppResult;
use vault_core::{
    ObjectRef, skills,
    ui::{
        BUTTON_PRESS, ColorKey,
        skilldex::{
            self, ROW_ART, ROW_SIZE, ROW_SOUNDS, SkilldexAction, SkilldexControl, SkilldexKey,
            SkilldexOutcome, SkilldexWindow,
        },
    },
};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

impl MapPreview {
    pub(crate) fn open_skilldex(&mut self) {
        self.open_skilldex_on(None);
    }
    pub(crate) fn open_skilldex_on(&mut self, target: Option<ObjectRef>) {
        if matches!(self.screen, Screen::Map) {
            self.screen = Screen::Skilldex(SkilldexWindow::new(target));
        }
    }
    pub(crate) fn skilldex_image(&mut self) -> AppResult<IndexedImage> {
        let view = self.map_view_image()?;
        let mut image = self.compose_world(view);
        let painter = self.painter();
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
        let layout = skilldex::layout();
        for (row, skill) in skills::SKILLDEX_SKILLS.iter().enumerate() {
            let rect = skilldex::row_rect(row);
            let art = self.draw_button_opaque(&painter, &mut image, rect, ROW_ART);
            // The label is (18, 17, 3) on the up image, (14, 12, 3) down.
            let key = if art == ROW_ART.1 {
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
                ROW_SIZE[0],
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
        if let Some(cancel) = layout.rect(SkilldexControl::Cancel) {
            self.draw_red_button(&painter, &mut image, cancel);
        }
        Ok(image)
    }
    /// The window closes; a chosen row's action applies.
    fn close_skilldex(&mut self, action: Option<SkilldexAction>) {
        if !matches!(self.screen, Screen::Skilldex(_)) {
            return;
        }
        self.screen = Screen::Map;
        match action {
            None => {}
            Some(SkilldexAction::UseOn { target, skill }) => {
                let player = self.host.world.player;
                if let Err(error) = self.host.use_skill_on(player, target, skill) {
                    self.status = error.to_string().into_bytes();
                }
                self.drain_effects();
            }
            Some(SkilldexAction::Choose(skill)) => self.choose_skill(skill),
        }
    }
    pub(crate) fn skilldex_click(&mut self, point: [i32; 2]) -> bool {
        let clock = self.clock;
        let Screen::Skilldex(window) = &mut self.screen else {
            return false;
        };
        match window.press(point, clock) {
            SkilldexOutcome::Chosen { row, .. } => {
                let (press, release) = ROW_SOUNDS;
                let rect = skilldex::row_rect(row);
                self.press_button_cue(press.as_bytes(), release.as_bytes(), rect);
            }
            SkilldexOutcome::Cancel => {
                if let Some(cancel) = skilldex::layout().rect(SkilldexControl::Cancel) {
                    self.press_button(cancel);
                }
                self.close_skilldex(None);
            }
            SkilldexOutcome::None | SkilldexOutcome::Busy => {}
        }
        true
    }
    /// The chosen row's wait: the window closes `SKILLDEX_CLOSE_MS` after
    /// the press.
    pub(crate) fn skilldex_tick(&mut self) -> bool {
        let Screen::Skilldex(window) = &self.screen else {
            return false;
        };
        let Some(action) = window.due(self.clock) else {
            return false;
        };
        self.close_skilldex(Some(action));
        true
    }
    /// Esc cancels; Enter also cancels after `ib1p1xx1`.
    pub(crate) fn skilldex_key(&mut self, key: KeyCode) -> bool {
        let Screen::Skilldex(window) = &self.screen else {
            return false;
        };
        let input = match key {
            KeyCode::Escape => SkilldexKey::Escape,
            KeyCode::Enter | KeyCode::NumpadEnter => SkilldexKey::Enter,
            _ => SkilldexKey::Other,
        };
        match window.key(input) {
            SkilldexOutcome::None => false,
            SkilldexOutcome::Cancel => {
                if input == SkilldexKey::Enter {
                    self.play_effect(BUTTON_PRESS.as_bytes(), None);
                }
                self.close_skilldex(None);
                true
            }
            SkilldexOutcome::Busy | SkilldexOutcome::Chosen { .. } => true,
        }
    }
}
