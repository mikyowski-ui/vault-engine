//! Shared modal chrome, hit-testing and frame adapters (Wave 1, Agent 2).
//!
//! The modal screens over the map (inventory, loot, barter, character,
//! Pip-Boy, save/load, options, Skilldex, elevator, called shot, dialog
//! boxes and the conversation's windows) share their window-relative
//! rectangle math, their red and two-state buttons, the SPEC-014 §3.13
//! large dialog box and the held-button repeat here. The item windows'
//! shared parts (the arrow cursor's press and menu, the entry grid, the
//! portraits, the counter and the engine-drawn buttons) are in `items`.
//!
//! Everything here draws or hit-tests exactly as the screens did on their
//! own; only where the code lives changed.
use super::MapPreview;
use crate::interface::Painter;
use vault_core::ui::{
    ColorKey, HoldRepeat, RED_BUTTON, Rect,
    flow::{self, DialogButton},
};
use vault_render::IndexedImage;

mod items;

pub(crate) use items::{ArrowPress, ItemMenu, clamp_offsets, hover, scroll};

/// A window-relative rectangle on the screen, for the window at `origin`.
pub(crate) const fn at(origin: [i32; 2], rect: Rect) -> Rect {
    Rect::new(origin[0] + rect.x, origin[1] + rect.y, rect.w, rect.h)
}

/// A window-relative point on the screen, for the window at `origin`.
pub(crate) const fn offset(origin: [i32; 2], point: [i32; 2]) -> [i32; 2] {
    [origin[0] + point[0], origin[1] + point[1]]
}

/// A screen point relative to the window at `origin`.
pub(crate) const fn local(point: [i32; 2], origin: [i32; 2]) -> [i32; 2] {
    [point[0] - origin[0], point[1] - origin[1]]
}

/// The red button's fixed 15-by-16 rectangle with its top-left at `origin`.
pub(crate) const fn red_button(origin: [i32; 2]) -> Rect {
    Rect::new(origin[0], origin[1], RED_BUTTON.2[0], RED_BUTTON.2[1])
}

/// The red button at `origin`, sized by its up art (the fixed size when
/// the art is missing).
pub(crate) fn red_button_sized(painter: &Painter, origin: [i32; 2]) -> Rect {
    let [w, h] = painter.size(RED_BUTTON.0).unwrap_or(RED_BUTTON.2);
    Rect::new(origin[0], origin[1], w, h)
}

/// One frame of a held, repeating button: the repeat ends when `over`
/// says the pointer has left its rectangle, else its action comes back
/// when the repeat fires.
pub(crate) fn poll_repeat<K: Copy>(
    held: &mut Option<(K, Rect, HoldRepeat)>,
    now: u64,
    over: impl FnOnce(Rect) -> bool,
) -> Option<K> {
    let (action, rect, repeat) = held.as_mut()?;
    if !over(*rect) {
        *held = None;
        return None;
    }
    repeat.poll(now).then_some(*action)
}

/// A large dialog box's red buttons on the screen, for the box at
/// `origin`: the Done button, or Yes and No.
pub(crate) fn dialog_buttons(origin: [i32; 2], yes_no: bool) -> Vec<(DialogButton, Rect)> {
    flow::dialog_buttons(yes_no)
        .into_iter()
        .map(|button| (button, red_button(offset(origin, button.button))))
        .collect()
}

impl MapPreview {
    /// A two-state button at screen `rect`, drawn transparently at its
    /// top-left: `down` while it is held with the pointer over it, else
    /// `up`. Returns the art drawn.
    pub(crate) fn draw_button(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        rect: Rect,
        (up, down): (&'static str, &'static str),
    ) -> &'static str {
        let art = self.button_art(rect, up, down);
        painter.blit(image, art, 0, [rect.x, rect.y]);
        art
    }
    /// A two-state button drawn opaquely (index 0 included).
    pub(crate) fn draw_button_opaque(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        rect: Rect,
        (up, down): (&'static str, &'static str),
    ) -> &'static str {
        let art = self.button_art(rect, up, down);
        painter.blit_opaque(image, art, 0, [rect.x, rect.y]);
        art
    }
    /// The red button at screen `rect`.
    pub(crate) fn draw_red_button(&self, painter: &Painter, image: &mut IndexedImage, rect: Rect) {
        self.draw_button(painter, image, rect, (RED_BUTTON.0, RED_BUTTON.1));
    }
    /// The red button at screen `rect` with its caption: the `message`
    /// in font 103, key 18979, at `caption`, clipped to `clip` pixels.
    pub(crate) fn draw_captioned_red_button(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        rect: Rect,
        (file, id): (&str, i32),
        caption: [i32; 2],
        clip: i32,
    ) {
        self.draw_red_button(painter, image, rect);
        let text = painter.message(file, id);
        painter.text(image, 103, ColorKey::LABEL, &text, caption, clip);
    }
    /// SPEC-014 §3.13 the large dialog box at `origin`: `lgdialog.frm`,
    /// the title and body lines in font 101 and `key`, centered and 10
    /// pixels apart from the title's line, then the Done box or the Yes
    /// and No boxes with their captions and red buttons.
    pub(crate) fn draw_dialog_box(
        &self,
        painter: &Painter,
        image: &mut IndexedImage,
        origin: [i32; 2],
        (title, body): (&[u8], &[Vec<u8>]),
        key: ColorKey,
        yes_no: bool,
    ) {
        let [ox, oy] = origin;
        let (art, [width, _]) = flow::DialogSize::Large.art();
        painter.blit_opaque(image, art, 0, origin);
        let mut y = oy + flow::dialog_title_y(body.len());
        for line in std::iter::once(title).chain(body.iter().map(Vec::as_slice)) {
            painter.centered(image, 101, key, line, [ox, y], width);
            y += 10;
        }
        for (button, rect) in dialog_buttons(origin, yes_no) {
            let box_origin = offset(origin, button.box_origin);
            if button.transparent_box {
                painter.blit(image, flow::DONE_BOX.0, 0, box_origin);
            } else {
                painter.blit_opaque(image, flow::DONE_BOX.0, 0, box_origin);
            }
            let caption = painter.message("dbox.msg", button.caption);
            let at = offset(origin, button.caption_origin);
            painter.text(image, 103, ColorKey::LABEL, &caption, at, 60);
            self.draw_red_button(painter, image, rect);
        }
    }
}
