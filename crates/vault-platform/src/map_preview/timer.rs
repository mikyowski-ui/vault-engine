//! SPEC-013 §3.15 the Set Timer window for an explosive being armed
//! (SPEC-013 §3.10), opened by the inventory's "use" action or by the item
//! slot (SPEC-014 §3.23): `movemult.frm` at screen (140, 80), the title, the
//! item picture, `timer.frm`, the minutes and seconds digits, plus, minus,
//! Done and Cancel (all four drawn transparently). The title is in key
//! 21091. It is modal over whichever window opened it.
use super::MapPreview;
use crate::interface::Painter;
use vault_core::{
    ObjectRef,
    assets::WorldAssets,
    skills::SetTimer,
    ui::{BUTTON_PRESS, ColorKey, ERROR_SOUND, Rect},
};
use vault_formats::pro::PrototypeData;
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// §3.15 layout (window-relative).
const ORIGIN: [i32; 2] = [140, 80];
const PLUS: Rect = Rect::new(194, 64, 16, 12);
const MINUS: Rect = Rect::new(194, 76, 17, 12);
const DONE: Rect = Rect::new(98, 128, 15, 16);
const CANCEL: Rect = Rect::new(148, 128, 15, 16);
/// A button's screen rectangle.
fn button(rect: Rect) -> Rect {
    Rect::new(ORIGIN[0] + rect.x, ORIGIN[1] + rect.y, rect.w, rect.h)
}

impl MapPreview {
    /// Open the window for `item`, starting at 60 seconds.
    pub(crate) fn open_timer(&mut self, item: ObjectRef) {
        self.hud.timer = Some((item, SetTimer::default()));
    }
    pub(crate) fn timer_open(&self) -> bool {
        self.hud.timer.is_some()
    }
    pub(crate) fn draw_timer(&self, image: &mut IndexedImage) {
        let Some((item, timer)) = &self.hud.timer else {
            return;
        };
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        let [ox, oy] = ORIGIN;
        painter.blit_opaque(image, "movemult.frm", 0, [ox, oy]);
        let title = painter.message("inventry.msg", 23);
        painter.centered(image, 103, ColorKey::AMBER, &title, [ox, oy + 9], 259);
        // The item's inventory art scaled into 90 by 61 at (16, 46).
        let fid =
            self.host
                .world
                .get(*item)
                .and_then(|object| match &object.prototype.as_ref()?.data {
                    PrototypeData::Item(data) => Some(data.inventory_art),
                    _ => None,
                });
        if let Some(fid) = fid
            && let Ok(art) = self.host.assets.object_art(fid)
            && let Some(frame) = art.frame(0, 0)
        {
            image.draw_frame(frame, [(ox + 16) as u32, (oy + 46) as u32], [90, 61]);
        }
        painter.blit(image, "timer.frm", 0, [ox + 113, oy + 34]);
        for (x, digit) in timer.digits() {
            painter.big_digit(image, digit, [ox + x, oy + 64], false);
        }
        for (rect, up, down) in [
            (PLUS, "splsoff.frm", "splson.frm"),
            (MINUS, "snegoff.frm", "snegon.frm"),
            (DONE, "lilredup.frm", "lilreddn.frm"),
            (CANCEL, "lilredup.frm", "lilreddn.frm"),
        ] {
            let rect = button(rect);
            painter.blit(image, self.button_art(rect, up, down), 0, [rect.x, rect.y]);
        }
    }
    /// Enter (which plays `ib1p1xx1` for an accepted value) arms the
    /// explosive; any other value would play `iisxxxx1`. Esc cancels.
    fn finish_timer(&mut self, accept: bool) {
        let Some((item, timer)) = self.hud.timer.take() else {
            return;
        };
        if !accept {
            return;
        }
        if !timer.accepts() {
            self.play_effect(ERROR_SOUND.as_bytes(), None);
            return;
        }
        self.play_effect(BUTTON_PRESS.as_bytes(), None);
        if let Err(error) = self.host.arm_explosive(item, timer.seconds) {
            tracing::warn!(%error, "explosive not armed");
            self.status = error.to_string().into_bytes();
        }
        self.drain_effects();
    }
    /// Keys while the window is open: Enter and Esc; digit keys and
    /// Backspace are ignored, and every other key is swallowed (modal).
    pub(crate) fn timer_key(&mut self, key: KeyCode) -> bool {
        if self.hud.timer.is_none() {
            return false;
        }
        match key {
            KeyCode::Enter | KeyCode::NumpadEnter => self.finish_timer(true),
            KeyCode::Escape => self.finish_timer(false),
            _ => {}
        }
        true
    }
    /// A press on a button: every button, plus and minus included, plays
    /// `ib1p1xx1` now and `ib1lu1x1` on its release. Plus and minus act on
    /// the press, once (holding them does not repeat); Done and Cancel act
    /// on the release (`timer_release`).
    pub(crate) fn timer_click(&mut self, point: [i32; 2]) -> bool {
        let Some((_, timer)) = &mut self.hud.timer else {
            return false;
        };
        let Some(pressed) = [PLUS, MINUS, DONE, CANCEL]
            .into_iter()
            .find(|rect| button(*rect).contains(point))
        else {
            return true;
        };
        if pressed == PLUS {
            timer.plus();
        } else if pressed == MINUS {
            timer.minus();
        }
        self.press_button(button(pressed));
        true
    }
    /// A left release while the window is open (it is modal): Done and
    /// Cancel act as Enter and Esc when the release is over the button
    /// pressed. `over` is that button's screen rectangle.
    pub(crate) fn timer_release(&mut self, over: Option<Rect>) -> bool {
        if self.hud.timer.is_none() {
            return false;
        }
        // The release cue sounds before the button acts.
        if let Some(cue) = self.pressed.take() {
            self.play_effect(cue, None);
        }
        if over == Some(button(DONE)) {
            self.finish_timer(true);
        } else if over == Some(button(CANCEL)) {
            self.finish_timer(false);
        }
        true
    }
}
