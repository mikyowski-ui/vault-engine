//! The touch UI's action pill bar (`--touch-ui`, `touch_ui = true`): a row
//! of thumb-sized amber-and-black buttons for the keys a touch screen has
//! no other way to press. Engine extension, not part of any specification;
//! with the touch UI off (the default) nothing draws and no click is taken,
//! so frames, input and replays are what they were.
//!
//! The bar sits in the interface frame's top-right corner (frame pixels):
//! [`SIZE`] buttons [`GAP`] apart, right-aligned to x 636 at y 12, so it
//! ends at y 52 and starts at x 246 or further right. That keeps it clear
//! of the party pills (x 4-156), the bar and the indicator boxes (y 358
//! down), the floating text, which is in the world, and the unlocked
//! view's edge-scrolling band (2% of the canvas, under 10 frame rows when
//! the interface fits the window's height), whose scrolling would discard
//! the buttons' keys. It shows on the map
//! screen only, outside a conversation and while no dialog box, Set Timer
//! or called-shot window is open; it hides while the debug overlay's panel
//! (x 352 right) shows, and every full window covers it.
//!
//! Each button acts on its release over it, through the key's own path (the
//! press and release of [`MapPreview::step`] and [`MapPreview::key_up`]),
//! so it does exactly what the key does, the combat lock's held keys
//! included:
//! - `MODE`: a right click at the button (the cursor-mode toggle);
//! - `PEEK`: Alt+R, the roof peek's cycle (the enhanced unlocked view only;
//!   hidden elsewhere, where Alt+R does nothing);
//! - `TURN`: Space (end the turn); held [`LONG_PRESS_MS`] or longer, Enter
//!   (end combat), timed on the logged frame clock;
//! - `INV`: I, the inventory; `PIP`: P, the Pip-Boy; `ESC`: Escape, the
//!   menu.
//!
//! A held button draws inverted (black on amber), as an engine button
//! shows its down art, and plays the button cues.
use super::{MapPreview, Screen};
use crate::interface::Painter;
use crate::touch::LONG_PRESS_MS;
use vault_core::ui::{ColorKey, Rect};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// A button's size, the space between buttons, the bar's right edge and
/// its top, in frame pixels.
pub(crate) const SIZE: [i32; 2] = [60, 40];
pub(crate) const GAP: i32 = 6;
const RIGHT: i32 = 636;
const TOP: i32 = 12;
/// The smallest touch target allowed, in frame pixels on each side.
pub(crate) const MIN_TARGET: i32 = 40;
const _: () = assert!(SIZE[0] >= MIN_TARGET && SIZE[1] >= MIN_TARGET);

/// A pill bar button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BarButton {
    Mode,
    Peek,
    Turn,
    Inventory,
    PipBoy,
    Escape,
}
impl BarButton {
    pub(crate) const ALL: [Self; 6] = [
        Self::Mode,
        Self::Peek,
        Self::Turn,
        Self::Inventory,
        Self::PipBoy,
        Self::Escape,
    ];
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Mode => "MODE",
            Self::Peek => "PEEK",
            Self::Turn => "TURN",
            Self::Inventory => "INV",
            Self::PipBoy => "PIP",
            Self::Escape => "ESC",
        }
    }
}

/// The bar's buttons and their rectangles, left to right and
/// right-aligned; `PEEK` only with `peek`.
pub(crate) fn layout(peek: bool) -> Vec<(BarButton, Rect)> {
    let shown: Vec<BarButton> = BarButton::ALL
        .into_iter()
        .filter(|button| peek || *button != BarButton::Peek)
        .collect();
    let count = shown.len() as i32;
    let left = RIGHT - count * SIZE[0] - (count - 1) * GAP;
    (0..)
        .zip(shown)
        .map(|(k, button)| {
            let x = left + k * (SIZE[0] + GAP);
            (button, Rect::new(x, TOP, SIZE[0], SIZE[1]))
        })
        .collect()
}

/// The touch UI's state in a world.
#[derive(Debug, Clone, Default)]
pub(crate) struct TouchBar {
    pub(crate) on: bool,
    /// The button pressed and still held, and the frame clock it went down
    /// at.
    pub(crate) pressed: Option<(BarButton, u64)>,
}

impl MapPreview {
    /// Show the touch UI's pill bar or not (the shell's `--touch-ui`, or a
    /// replayed `touch_ui` record).
    pub fn set_touch_ui(&mut self, on: bool) {
        self.touch.on = on;
        if !on {
            self.touch.pressed = None;
        }
    }
    /// The bar's buttons shown now, with their rectangles (none when it
    /// does not show).
    pub(crate) fn touch_bar(&self) -> Vec<(BarButton, Rect)> {
        let shown = self.touch.on
            && matches!(self.screen, Screen::Map)
            && !self.host.world.dialogue.open
            && !self.debug_enabled()
            && self.dialog.is_none()
            && self.hud.timer.is_none()
            && self.hud.called.is_none();
        if !shown {
            return Vec::new();
        }
        layout(self.unlocked().is_some())
    }
    /// The button under a frame pixel.
    pub(crate) fn touch_button_at(&self, point: [i32; 2]) -> Option<BarButton> {
        self.touch_bar()
            .into_iter()
            .find(|(_, rect)| rect.contains(point))
            .map(|(button, _)| button)
    }
    /// Draw the bar: each button an amber frame with rounded corners, its
    /// label amber on black, or black on amber while it is held.
    pub(crate) fn draw_touch_bar(&self, painter: &Painter, image: &mut IndexedImage) {
        let amber = painter.color(ColorKey::AMBER);
        let black = painter.color(ColorKey::BLACK);
        let held = self.touch.pressed.map(|(button, _)| button);
        for (button, rect) in self.touch_bar() {
            let Rect { x, y, w, h } = rect;
            image.fill_rect([x + 1, y, w - 2, h], amber);
            image.fill_rect([x, y + 1, w, h - 2], amber);
            let down = held == Some(button);
            if !down {
                image.fill_rect([x + 2, y + 2, w - 4, h - 4], black);
            }
            let ink = if down {
                ColorKey::BLACK
            } else {
                ColorKey::AMBER
            };
            painter.centered(
                image,
                101,
                ink,
                button.label().as_bytes(),
                [x, y + (h - 8) / 2],
                w,
            );
        }
    }
    /// A left press at frame pixel `point`; returns whether a bar button
    /// took it (it then shows held until the release).
    pub(crate) fn touch_bar_press(&mut self, point: [i32; 2]) -> bool {
        let Some(button) = self.touch_button_at(point) else {
            return false;
        };
        self.touch.pressed = Some((button, self.clock));
        self.press();
        true
    }
    /// A bar button is held.
    pub(crate) fn touch_bar_held(&self) -> bool {
        self.touch.pressed.is_some()
    }
    /// The left release of a bar press at frame pixel `point` (`None`:
    /// outside the frame or the window): the release cue, and the button's
    /// action when the release is over the button pressed. Always taken.
    pub(crate) fn touch_bar_release(&mut self, point: Option<[i32; 2]>) -> bool {
        let Some((button, since)) = self.touch.pressed.take() else {
            return false;
        };
        if let Some(cue) = self.pressed.take() {
            self.play_effect(cue, None);
        }
        let Some(point) = point.filter(|point| self.touch_button_at(*point) == Some(button)) else {
            return true;
        };
        let held = self.clock.saturating_sub(since);
        match button {
            BarButton::Mode => {
                self.click(point, true);
                self.release(Some(point), true);
            }
            BarButton::Peek => {
                // Alt+R, with Alt held for that key only.
                let modifiers = self.hud.modifiers;
                self.hud.modifiers = [false, false, true];
                self.touch_key(KeyCode::KeyR);
                self.hud.modifiers = modifiers;
            }
            BarButton::Turn if held >= LONG_PRESS_MS => self.touch_key(KeyCode::Enter),
            BarButton::Turn => self.touch_key(KeyCode::Space),
            BarButton::Inventory => self.touch_key(KeyCode::KeyI),
            BarButton::PipBoy => self.touch_key(KeyCode::KeyP),
            BarButton::Escape => self.touch_key(KeyCode::Escape),
        }
        true
    }
    /// A key pressed and released, as the keyboard sends it.
    fn touch_key(&mut self, key: KeyCode) {
        self.step(key);
        self.key_up(key);
    }
}

#[cfg(test)]
#[path = "touch_tests.rs"]
mod tests;
