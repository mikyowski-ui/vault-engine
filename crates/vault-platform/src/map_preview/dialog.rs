//! SPEC-014 §3.13 modal dialog boxes over the current screen: Done boxes
//! (the save/load errors at (169, 116), the Pip-Boy refusal in combat at
//! (192, 116), the automap's motion-sensor refusals at (165, 140)) and the
//! automap's Yes/No quit confirmation at (169, 117).
//!
//! Every box shown here is large, `lgdialog.frm` 302 by 127: the save/load
//! boxes request it, a Yes/No box always is, and the other titles are wider
//! than 168 pixels. Text is font 101 in one color key, centered, the title
//! at `y0 = 27 + 30 - trunc((body lines + 1) * 10 / 2)` and body lines
//! (wrapped at 276 pixels) every 10 pixels below it.
use super::{MapPreview, Screen};
use crate::interface::Painter;
use vault_core::ui::{BUTTON_PRESS, ColorKey, Rect, flow};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

pub(crate) struct Dialog {
    origin: [i32; 2],
    title: Vec<u8>,
    body: Vec<u8>,
    key: ColorKey,
    yes_no: bool,
    /// The OK (Done) or Yes answer closes the screen and requests the
    /// return to the main menu (a failed load, a confirmed quit).
    quits: bool,
}
impl Dialog {
    /// A Done box with a title, an optional body, in `key`.
    pub fn done(origin: [i32; 2], title: Vec<u8>, body: Vec<u8>, key: ColorKey) -> Self {
        Self {
            origin,
            title,
            body,
            key,
            yes_no: false,
            quits: false,
        }
    }
    /// A Yes/No box whose Yes requests the return to the main menu.
    pub fn quit_confirmation(origin: [i32; 2], title: Vec<u8>, key: ColorKey) -> Self {
        Self {
            yes_no: true,
            quits: true,
            ..Self::done(origin, title, Vec::new(), key)
        }
    }
    /// After the answer, close the screen and request the main menu.
    pub fn quitting(mut self) -> Self {
        self.quits = true;
        self
    }
}

/// Body lines wider than `flow::DIALOG_WRAP` wrap at spaces.
fn wrap(text: &[u8], width: impl Fn(&[u8]) -> i32) -> Vec<Vec<u8>> {
    let mut lines: Vec<Vec<u8>> = Vec::new();
    for word in text.split(|b| *b == b' ').filter(|w| !w.is_empty()) {
        match lines.last_mut() {
            Some(line) if width(&[line.as_slice(), b" ", word].concat()) <= flow::DIALOG_WRAP => {
                line.push(b' ');
                line.extend_from_slice(word);
            }
            _ => lines.push(word.to_vec()),
        }
    }
    lines
}

impl MapPreview {
    pub(crate) fn show_dialog(&mut self, dialog: Dialog) {
        self.dialog = Some(dialog);
    }
    /// Close the box with the OK/Yes (`true`) or No/cancel answer. A Done
    /// box's procedure goes on whichever way it is closed.
    fn answer_dialog(&mut self, yes: bool) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        if dialog.quits && (yes || !dialog.yes_no) {
            self.screen = Screen::Map;
            self.quit_requested = true;
        }
    }
    pub(crate) fn draw_dialog(&self, image: &mut IndexedImage) {
        let Some(dialog) = &self.dialog else {
            return;
        };
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        let [ox, oy] = dialog.origin;
        let (art, [width, _]) = flow::DialogSize::Large.art();
        painter.blit_opaque(image, art, 0, [ox, oy]);
        let body = wrap(&dialog.body, |line| painter.width(101, line));
        let mut y = oy + flow::dialog_title_y(body.len());
        for line in std::iter::once(&dialog.title).chain(&body) {
            painter.centered(image, 101, dialog.key, line, [ox, y], width);
            y += 10;
        }
        for button in flow::dialog_buttons(dialog.yes_no) {
            let [bx, by] = button.box_origin;
            if button.transparent_box {
                painter.blit(image, flow::DONE_BOX.0, 0, [ox + bx, oy + by]);
            } else {
                painter.blit_opaque(image, flow::DONE_BOX.0, 0, [ox + bx, oy + by]);
            }
            let caption = painter.message("dbox.msg", button.caption);
            let [cx, cy] = button.caption_origin;
            painter.text(
                image,
                103,
                ColorKey::LABEL,
                &caption,
                [ox + cx, oy + cy],
                60,
            );
            let [rx, ry] = button.button;
            let rect = Rect::new(ox + rx, oy + ry, 15, 16);
            let art = self.button_art(rect, "lilredup.frm", "lilreddn.frm");
            painter.blit(image, art, 0, [rect.x, rect.y]);
        }
    }
    /// Enter means OK/yes (with `ib1p1xx1`), Esc no/cancel; `y` and `n`
    /// work in Yes/No boxes only. A pending quit request forces yes. Other
    /// keys are swallowed.
    pub(crate) fn dialog_key(&mut self, key: KeyCode) -> bool {
        let Some(dialog) = &self.dialog else {
            return false;
        };
        let answer = match key {
            KeyCode::Enter | KeyCode::NumpadEnter => {
                self.play_effect(BUTTON_PRESS.as_bytes(), None);
                Some(true)
            }
            KeyCode::Escape => Some(false),
            KeyCode::KeyY if dialog.yes_no => Some(true),
            KeyCode::KeyN if dialog.yes_no => Some(false),
            _ => None,
        };
        if let Some(yes) = answer {
            let forced = self.quit_requested;
            self.answer_dialog(yes || forced);
        }
        true
    }
    /// A press on a red button answers the box; presses elsewhere are
    /// swallowed (the box is modal).
    pub(crate) fn dialog_click(&mut self, point: [i32; 2]) -> bool {
        let Some(dialog) = &self.dialog else {
            return false;
        };
        let [ox, oy] = dialog.origin;
        let pressed = flow::dialog_buttons(dialog.yes_no)
            .into_iter()
            .map(|button| {
                let [rx, ry] = button.button;
                (button, Rect::new(ox + rx, oy + ry, 15, 16))
            })
            .find(|(_, rect)| rect.contains(point));
        if let Some((button, rect)) = pressed {
            self.press_button(rect);
            // `dbox.msg` 100 is Done, 101 Yes, 102 No.
            self.answer_dialog(button.caption != 102);
        }
        true
    }
}
