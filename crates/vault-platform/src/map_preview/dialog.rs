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
use super::{MapPreview, Screen};
use vault_core::ui::{
    BUTTON_PRESS, ColorKey, Rect,
    flow::{self, DialogBox, DialogButton, DialogKey, DialogSize},
    red_button,
};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

pub(crate) struct Dialog {
    /// SPEC-020 §2.1: the core box owns the origin, the Yes/No kind and
    /// the red buttons' hit-testing.
    dialog: DialogBox,
    title: Vec<u8>,
    body: Vec<u8>,
    key: ColorKey,
    /// The OK (Done) or Yes answer closes the screen and requests the
    /// return to the main menu (a failed load, a confirmed quit).
    quits: bool,
    /// Yes continues the save over an occupied slot (§3.15 step 1).
    overwrite: bool,
    /// SPEC-024b §4.11: a script's `message_box`, which its answer resumes.
    script: Option<ScriptBox>,
}
/// How a script's message box differs from the engine's boxes.
struct ScriptBox {
    lines: Vec<Vec<u8>>,
    /// The first line's palette index, and the other lines'.
    colors: [u8; 2],
    size: DialogSize,
    left: bool,
    top: bool,
    buttons: bool,
}
/// `message_box`'s flags: `0x1` normal, `0x2` small, `0x4` align left, `0x8`
/// align top, `0x10` Yes and No, `0x20` no buttons.
mod box_flag {
    pub const NORMAL: i32 = 0x1;
    pub const SMALL: i32 = 0x2;
    pub const LEFT: i32 = 0x4;
    pub const TOP: i32 = 0x8;
    pub const YES_NO: i32 = 0x10;
    pub const NO_BUTTONS: i32 = 0x20;
}
/// SPEC-014 §3.13's left margin when centering is disabled.
const LEFT_MARGIN: i32 = 29;
/// The screen row of a script's box. Its column centers the box.
const SCRIPT_BOX_Y: i32 = 116;
impl Dialog {
    /// A Done box with a title, an optional body, in `key`.
    pub fn done(origin: [i32; 2], title: Vec<u8>, body: Vec<u8>, key: ColorKey) -> Self {
        Self {
            dialog: DialogBox::new(origin, false),
            title,
            body,
            key,
            quits: false,
            overwrite: false,
            script: None,
        }
    }
    /// SPEC-024b §4.11 a script's message box, whose widest line is `widest`
    /// pixels. `0x20` shows no buttons, and Enter and Escape still close it.
    fn message_box(open: &vault_core::host_sfall::MessageBox, widest: i32) -> Self {
        let flags = open.flags;
        let buttons = flags & box_flag::NO_BUTTONS == 0;
        let yes_no = buttons && flags & box_flag::YES_NO != 0;
        let requested = if flags & box_flag::SMALL != 0 {
            Some(DialogSize::Medium)
        } else if flags & box_flag::NORMAL != 0 {
            Some(DialogSize::Large)
        } else {
            None
        };
        let size = flow::dialog_size(yes_no, requested, widest, open.lines.len());
        let (_, [width, _]) = size.art();
        Self {
            dialog: DialogBox::new([(640 - width) / 2, SCRIPT_BOX_Y], yes_no),
            title: Vec::new(),
            body: Vec::new(),
            key: ColorKey::WHITE,
            quits: false,
            overwrite: false,
            script: Some(ScriptBox {
                lines: open.lines.clone(),
                colors: open.colors.map(|color| color as u8),
                size,
                left: flags & box_flag::LEFT != 0,
                top: flags & box_flag::TOP != 0,
                buttons,
            }),
        }
    }
    /// A script's box's red buttons on the screen.
    fn script_buttons(&self, script: &ScriptBox) -> Vec<(DialogButton, Rect)> {
        if !script.buttons {
            return Vec::new();
        }
        let origin = self.dialog.origin;
        flow::dialog_buttons_for(self.dialog.yes_no, script.size)
            .into_iter()
            .map(|button| {
                let at = [origin[0] + button.button[0], origin[1] + button.button[1]];
                (button, red_button(at))
            })
            .collect()
    }
    /// §3.15 the save screen's overwrite question: a large Yes/No box with
    /// the title only.
    pub fn overwrite(origin: [i32; 2], title: Vec<u8>, key: ColorKey) -> Self {
        Self {
            dialog: DialogBox::new(origin, true),
            overwrite: true,
            ..Self::done(origin, title, Vec::new(), key)
        }
    }
    /// A Yes/No box whose Yes requests the return to the main menu.
    pub fn quit_confirmation(origin: [i32; 2], title: Vec<u8>, key: ColorKey) -> Self {
        Self {
            dialog: DialogBox::new(origin, true),
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
    /// SPEC-024b §4.11: a box a script opened shows as a modal screen.
    pub(crate) fn show_message_box(&mut self) {
        if self.dialog.is_some() {
            return;
        }
        let Some(open) = self.host.sfall_open_message_box() else {
            return;
        };
        let widest = {
            let painter = self.painter();
            let widths = open.lines.iter().map(|line| painter.width(101, line));
            widths.max().unwrap_or(0)
        };
        let dialog = Dialog::message_box(open, widest);
        self.show_dialog(dialog);
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
        if dialog.script.is_some() {
            if let Err(error) = self.host.sfall_close_message_box(i32::from(yes)) {
                self.status = error.to_string().into_bytes();
            }
            return;
        }
        if dialog.overwrite {
            if yes {
                self.overwrite_accepted();
            }
            return;
        }
        if dialog.quits && (yes || !dialog.dialog.yes_no) {
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
            // The save/load screen closes as cancelled, keeping its
            // selection.
            Screen::SaveLoad(_) => self.close_save_screen(),
            _ => {}
        }
        self.screen = Screen::Map;
    }
    pub(crate) fn draw_dialog(&self, image: &mut IndexedImage) {
        let Some(dialog) = &self.dialog else {
            return;
        };
        let painter = self.painter();
        if let Some(script) = &dialog.script {
            self.draw_script_box(&painter, image, dialog, script);
            return;
        }
        let body = wrap(&dialog.body, |line| painter.width(101, line));
        let text = (dialog.title.as_slice(), body.as_slice());
        self.draw_dialog_box(
            &painter,
            image,
            dialog.dialog.origin,
            text,
            dialog.key,
            dialog.dialog.yes_no,
        );
    }
    /// SPEC-024b §4.11 a script's box: the first line in color 1 and the
    /// others in color 2, at §3.13's layout.
    fn draw_script_box(
        &self,
        painter: &crate::interface::Painter,
        image: &mut IndexedImage,
        dialog: &Dialog,
        script: &ScriptBox,
    ) {
        let [ox, oy] = dialog.dialog.origin;
        let (art, [width, _]) = script.size.art();
        painter.blit_opaque(image, art, 0, [ox, oy]);
        let body = script.lines.len().saturating_sub(1);
        let mut y = oy
            + if script.top {
                script.size.title_y(4)
            } else {
                script.size.title_y(body)
            };
        for (index, line) in script.lines.iter().enumerate() {
            let shown = painter.width(101, line);
            let x = if script.left {
                LEFT_MARGIN
            } else {
                (width - shown) / 2
            };
            let color = script.colors[usize::from(index > 0)];
            painter.text_indexed(image, 101, color, line, [ox + x, y], width - x);
            y += 10;
        }
        for (button, rect) in dialog.script_buttons(script) {
            let box_origin = [ox + button.box_origin[0], oy + button.box_origin[1]];
            if button.transparent_box {
                painter.blit(image, flow::DONE_BOX.0, 0, box_origin);
            } else {
                painter.blit_opaque(image, flow::DONE_BOX.0, 0, box_origin);
            }
            let caption = painter.message("dbox.msg", button.caption);
            let at = [ox + button.caption_origin[0], oy + button.caption_origin[1]];
            painter.text(image, 103, ColorKey::LABEL, &caption, at, 60);
            self.draw_red_button(painter, image, rect);
        }
    }
    /// Enter means OK/yes (with `ib1p1xx1`), Esc no/cancel; `y` and `n`
    /// work in Yes/No boxes only. A pending quit request forces yes. Other
    /// keys are swallowed.
    pub(crate) fn dialog_key(&mut self, key: KeyCode) -> bool {
        let Some(dialog) = self.dialog.as_ref().map(|dialog| dialog.dialog) else {
            return false;
        };
        let input = match key {
            KeyCode::Enter | KeyCode::NumpadEnter => {
                self.play_effect(BUTTON_PRESS.as_bytes(), None);
                Some(DialogKey::Enter)
            }
            KeyCode::Escape => Some(DialogKey::Escape),
            KeyCode::KeyY => Some(DialogKey::Char('y')),
            KeyCode::KeyN => Some(DialogKey::Char('n')),
            _ => None,
        };
        let answer = input.and_then(|input| dialog.key(input));
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
        let Some(dialog) = self.dialog.as_ref().map(|dialog| dialog.dialog) else {
            return false;
        };
        match &mut self.screen {
            Screen::World(world) => world.swallow_release(),
            Screen::World2(world) => world.swallow_release(),
            _ => {}
        }
        // A script's box has its own size, and may have no buttons.
        if let Some(buttons) = self.dialog.as_ref().and_then(|dialog| {
            let script = dialog.script.as_ref()?;
            Some(dialog.script_buttons(script))
        }) {
            let hit = buttons.iter().find(|(_, rect)| rect.contains(point));
            if let Some((button, rect)) = hit {
                self.press_button(*rect);
                self.answer_dialog(button.caption != 102);
            }
            return true;
        }
        let layout = dialog.layout();
        if let Some(yes) = dialog.click(point) {
            if let Some(rect) = layout.rect(yes) {
                self.press_button(rect);
            }
            self.answer_dialog(yes);
        }
        true
    }
}
