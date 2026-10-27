//! SPEC-014 §3.13 modal dialog boxes over the current screen: Done boxes
//! (the save/load errors at (169, 116), the Pip-Boy refusal in combat at
//! (192, 116), the automap's motion-sensor refusals at (165, 140)), the
//! save screen's Yes/No overwrite question at (169, 131), and the Yes/No
//! quit confirmation at (169, 117) over the windows that offer it.
//!
//! Every box shown here is large, `lgdialog.frm` 302 by 127: the save/load
//! boxes request it, a Yes/No box always is, and the other titles are wider
//! than 168 pixels. Text is font 101 in one color key, centered, the title
//! at `y0 = 27 + 30 - trunc((body lines + 1) * 10 / 2)` and body lines
//! (wrapped at 276 pixels) every 10 pixels below it.
use super::{MapPreview, Screen, modal_kit};
use vault_core::ui::{BUTTON_PRESS, ColorKey, flow};
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
    /// Yes continues the save over an occupied slot (§3.15 step 1).
    overwrite: bool,
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
            overwrite: false,
        }
    }
    /// §3.15 the save screen's overwrite question: a large Yes/No box with
    /// the title only.
    pub fn overwrite(origin: [i32; 2], title: Vec<u8>, key: ColorKey) -> Self {
        Self {
            yes_no: true,
            overwrite: true,
            ..Self::done(origin, title, Vec::new(), key)
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
    /// §3.17 the quit confirmation over the current window: a large Yes/No
    /// box at (169, 117) with `misc.msg` 0 in key 32328.
    pub(crate) fn confirm_quit(&mut self) {
        let title = self.message("misc.msg", 0);
        self.show_dialog(Dialog::quit_confirmation(
            [169, 117],
            title,
            ColorKey::ERROR,
        ));
    }
    /// Close the box with the OK/Yes (`true`) or No/cancel answer. A Done
    /// box's procedure goes on whichever way it is closed.
    fn answer_dialog(&mut self, yes: bool) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        if dialog.overwrite {
            if yes {
                self.overwrite_accepted();
            }
            return;
        }
        if dialog.quits && (yes || !dialog.yes_no) {
            self.leave_for_quit();
            self.quit_requested = true;
        }
    }
    /// A quit closes the window in front the way its own exit does: the
    /// loot window restores the player's equipment, preferences act as
    /// Cancel (§3.18), the inventory gives the player the figure's art, the
    /// save/load screen closes as cancelled; then the map is in front.
    fn leave_for_quit(&mut self) {
        match &self.screen {
            Screen::Loot(_) => self.close_loot_screen(),
            Screen::Options(_) => self.close_preferences(false),
            Screen::Inventory(_) => self.close_inventory(),
            Screen::PipBoy(_) => self.close_pipboy(),
            // SPEC-014 §3.42: Yes cancels the character screen.
            Screen::Character(_) => self.close_character(false),
            _ => {}
        }
        self.screen = Screen::Map;
    }
    pub(crate) fn draw_dialog(&self, image: &mut IndexedImage) {
        let Some(dialog) = &self.dialog else {
            return;
        };
        let painter = self.painter();
        let body = wrap(&dialog.body, |line| painter.width(101, line));
        let text = (dialog.title.as_slice(), body.as_slice());
        self.draw_dialog_box(
            &painter,
            image,
            dialog.origin,
            text,
            dialog.key,
            dialog.yes_no,
        );
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
    /// swallowed (the box is modal). H14: the press's release is swallowed
    /// too, so a box answered over the world map (the quit confirmation's
    /// No button at (351, 219) lies inside the view) never reaches the
    /// screen below as a view click.
    pub(crate) fn dialog_click(&mut self, point: [i32; 2]) -> bool {
        let Some(dialog) = &self.dialog else {
            return false;
        };
        match &mut self.screen {
            Screen::World(world) => world.swallow_release(),
            Screen::World2(world) => world.swallow_release(),
            _ => {}
        }
        let pressed = modal_kit::dialog_button_at(dialog.origin, dialog.yes_no, point);
        if let Some((button, rect)) = pressed {
            self.press_button(rect);
            // `dbox.msg` 100 is Done, 101 Yes, 102 No.
            self.answer_dialog(button.caption != 102);
        }
        true
    }
}
