//! SPEC-014 §3.38 the called-shot window over the map: `called.frm` at
//! screen (108, 20), the target's `na` picture, the eight body parts with
//! their chances and labels, the invisible part buttons with their hover
//! color, Cancel, and the window's input. Map background processing keeps
//! running while it is open (unlike the action menu); the in-world cursor
//! is hidden and the system cursor is the arrow.
use super::{
    MapPreview,
    modal_kit::{at, red_button_sized},
};
use crate::{input::key_text, interface::Painter};
use vault_core::{
    ObjectRef,
    assets::WorldAssets,
    critical::location,
    ui::{
        ColorKey, RED_BUTTON, Rect,
        called::{self, Part},
    },
};
use vault_formats::Fid;
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// The Cancel red button's art.
const CANCEL_ART: (&str, &str) = (RED_BUTTON.0, RED_BUTTON.1);

/// The open window: its target and hit mode, what it showed when it
/// opened (the chances are computed once), and the part button pressed and
/// not yet released.
pub(crate) struct CalledShot {
    pub target: ObjectRef,
    pub hit_mode: u8,
    /// The chance at each location 0-7.
    pub chances: [i32; 8],
    /// Each location's label and its width in font 101; `None` when the
    /// message is missing.
    pub labels: [Option<(Vec<u8>, i32)>; 8],
    /// The `na` picture's identifier.
    pub picture: u32,
    pub pressed: Option<u8>,
}
impl CalledShot {
    /// A part's button in screen pixels.
    fn button(&self, part: Part) -> Rect {
        let label = self.labels[usize::from(part.location)].as_ref();
        screen(part.button(label.map(|(_, width)| *width)))
    }
    /// The part whose button holds a screen pixel.
    fn part_at(&self, point: [i32; 2]) -> Option<Part> {
        called::parts().find(|part| self.button(*part).contains(point))
    }
}

/// A window rectangle in screen pixels.
const fn screen(rect: Rect) -> Rect {
    at(called::ORIGIN, rect)
}
/// The Cancel button's screen rectangle (the up art's size).
fn cancel_rect(painter: &Painter) -> Rect {
    screen(red_button_sized(painter, called::CANCEL))
}

impl MapPreview {
    /// §3.38 step 4: open the window for an aimed attack on `target` with
    /// `hit_mode`. The chances (SPEC-007 §3.5, no draw), the labels
    /// (`combat.msg` 1000 + 10 × alias + location) and the picture follow
    /// the target art's critter-list alias. A window that cannot be built,
    /// or whose background or Cancel art is missing, closes at once, as
    /// Esc: nothing happens.
    pub(crate) fn open_called(&mut self, target: ObjectRef, hit_mode: u8) {
        let player = self.host.world.player;
        let chances = self
            .host
            .world
            .attack_profile(player, target, hit_mode, location::UNCALLED, true)
            .and_then(|attack| called::chances(&attack));
        let chances = match chances {
            Ok(chances) => chances,
            Err(error) => {
                self.status = error.to_string().into_bytes();
                return;
            }
        };
        let painter = self.painter();
        if [called::BACKGROUND, CANCEL_ART.0, CANCEL_ART.1]
            .iter()
            .any(|name| painter.art(name).is_none())
        {
            tracing::warn!("called-shot window art missing");
            return;
        }
        let base = self
            .host
            .world
            .get(target)
            .map_or(0, |object| Fid(object.raw.art_id).base());
        let alias = self
            .host
            .assets
            .art_catalog()
            .ok()
            .and_then(|catalog| catalog.critter_alias(base));
        let labels = std::array::from_fn(|location| {
            let id = called::label_message(alias?, location as u8)?;
            let catalog = self.host.assets.game_messages("combat.msg").ok()?;
            let text = catalog.get(id)?.text.clone();
            let width = painter.width(101, &text);
            Some((text, width))
        });
        let picture = Fid::new(1, base, called::PICTURE_ANIMATION, 0, 0).0;
        self.hud.called = Some(CalledShot {
            target,
            hit_mode,
            chances,
            labels,
            picture,
            pressed: None,
        });
        self.pointer_feedback_off();
    }
    /// Draw the window: the opaque background, the picture, each row's
    /// left chance and label then right chance and label, and Cancel. A
    /// label is green, or red while the pointer is over its button. Each
    /// frame is drawn from the background, so a hover's redraw never
    /// blends with the earlier color (the `[May-improve]` safe rule).
    pub(crate) fn draw_called(&self, image: &mut IndexedImage) {
        let Some(window) = &self.hud.called else {
            return;
        };
        let painter = self.painter();
        let [ox, oy] = called::ORIGIN;
        painter.blit_opaque(image, called::BACKGROUND, 0, [ox, oy]);
        self.draw_called_picture(image, window.picture);
        let hovered = self.pointer.at.and_then(|at| window.part_at(at));
        for part in called::parts() {
            let [cx, cy] = part.chance_origin();
            let chance = window.chances[usize::from(part.location)];
            for (source, width, dx) in called::chance_cells(chance) {
                painter.blit_region(
                    image,
                    "numbers.frm",
                    Rect::new(source, 0, width, called::DIGIT_HEIGHT),
                    [ox + cx + dx, oy + cy],
                    true,
                );
            }
            if let Some((text, width)) = &window.labels[usize::from(part.location)] {
                let [lx, ly] = part.label_origin(*width);
                let key = if hovered == Some(part) {
                    ColorKey::RED
                } else {
                    ColorKey::GREEN
                };
                painter.text(image, 101, key, text, [ox + lx, oy + ly], *width + 1);
            }
        }
        self.draw_button(&painter, image, cancel_rect(&painter), CANCEL_ART);
    }
    /// The `na` picture's frame 0, copied opaquely (index 0 included) at
    /// (128, 31). The reference copies 170 by 225 with a 170-pixel stride
    /// whatever the frame's size; the `[May-improve]` safe rule copies only
    /// the frame's own pixels, clipped to 170 by 225. A missing file keeps
    /// the background.
    fn draw_called_picture(&self, image: &mut IndexedImage, picture: u32) {
        let Ok(art) = self.host.assets.object_art(picture) else {
            return;
        };
        let Some(frame) = art.frame(0, 0) else {
            return;
        };
        let area = screen(called::PICTURE);
        let (width, height) = (i32::from(frame.width), i32::from(frame.height));
        let stride = image.width() as i32;
        let rows = image.height() as i32;
        for y in 0..height.min(area.h) {
            for x in 0..width.min(area.w) {
                let (dx, dy) = (area.x + x, area.y + y);
                if (0..stride).contains(&dx) && (0..rows).contains(&dy) {
                    image.pixels_mut()[(dy * stride + dx) as usize] =
                        frame.pixels()[(y * width + x) as usize];
                }
            }
        }
    }
    /// The hover feedback and outline go while the window is open.
    fn pointer_feedback_off(&mut self) {
        self.pointer.feedback = None;
        self.clear_item_outline();
        self.pointer.held = None;
    }
    /// A press while the window is open: the Cancel red button presses
    /// with its sounds; a left press on a part's button waits for its
    /// release; right clicks and every other press are discarded.
    pub(crate) fn called_click(&mut self, point: [i32; 2], right: bool) -> bool {
        let Some(window) = &self.hud.called else {
            return false;
        };
        if right {
            return true;
        }
        let cancel = cancel_rect(&self.painter());
        if cancel.contains(point) {
            self.press_button(cancel);
            return true;
        }
        let pressed = window.part_at(point).map(|part| part.code);
        if let Some(window) = &mut self.hud.called {
            window.pressed = pressed;
        }
        true
    }
    /// A left release while the window is open: Cancel's release returns
    /// Esc; a part pressed and released on its own button is chosen.
    pub(crate) fn called_release(&mut self, point: Option<[i32; 2]>, over: Option<Rect>) -> bool {
        let Some(window) = &mut self.hud.called else {
            return false;
        };
        let pressed = window.pressed.take();
        let cancel = cancel_rect(&self.painter());
        if over == Some(cancel) {
            self.close_called();
            return true;
        }
        let Some(window) = &self.hud.called else {
            return true;
        };
        let released = point.and_then(|point| window.part_at(point));
        if let Some(part) = released.filter(|part| Some(part.code) == pressed) {
            self.choose_called(part);
        }
        true
    }
    /// Keys while the window is open: Esc closes it; Ctrl+A-G choose by
    /// selection code (by physical letter); Backspace and Ctrl+H make code
    /// 8, which is ignored (`[May-improve]`: the reference reads past its
    /// table). F12 still takes its screenshot, as it is handled when read.
    /// The digit keys and every other key do nothing.
    pub(crate) fn called_key(&mut self, key: KeyCode) -> bool {
        if self.hud.called.is_none() {
            return false;
        }
        match key {
            KeyCode::Escape => self.close_called(),
            KeyCode::F12 => self.screenshot(),
            _ => {
                let letter = key_text(key, true)
                    .filter(u8::is_ascii_uppercase)
                    .map(char::from);
                let code =
                    called::key_code(letter, self.hud.modifiers[1], key == KeyCode::Backspace);
                if let Some(part) = code.and_then(called::choice) {
                    self.choose_called(part);
                }
            }
        }
        true
    }
    /// Esc, by key or Cancel: the window closes and nothing else happens:
    /// no attack, no AP, no message, no draw. The cursor stays in
    /// crosshair mode.
    pub(crate) fn close_called(&mut self) {
        self.hud.called = None;
        self.pointer.rested = true;
    }
    /// A part is chosen: the window closes, `icsxxxx1` plays, and the
    /// player's attack request runs with the slot's hit mode and the
    /// chosen location 0-7, debited at the aimed cost once it is done
    /// (SPEC-007 §3.4). Torso is location 3, not the uncalled 8, and still
    /// pays the aimed cost. Every draw comes after the choice.
    fn choose_called(&mut self, part: Part) {
        let Some(window) = self.hud.called.take() else {
            return;
        };
        self.play_effect(called::CHOSEN_SOUND.as_bytes(), None);
        let player = self.host.world.player;
        let mode = window.hit_mode;
        match self
            .host
            .perform_attack(player, window.target, mode, part.location, true)
        {
            Ok(Ok(_)) => {}
            Ok(Err(refusal)) => {
                let cost = self
                    .host
                    .world
                    .attack_profile(player, window.target, mode, part.location, true)
                    .map_or(0, |attack| attack.ap_cost());
                self.refuse_attack(refusal, cost, mode);
            }
            Err(error) => self.status = error.to_string().into_bytes(),
        }
        self.pointer.rested = true;
    }
    /// A pending quit request (a script's map or world-map request during
    /// combat) ends the window. The reference then takes the last input
    /// code, reading before its table; the `[May-improve]` safe rule
    /// treats it as Esc. Returns whether the window closed.
    pub(crate) fn called_tick(&mut self) -> bool {
        if self.hud.called.is_some()
            && (self.quit_requested
                || self.host.world.pending_transition.is_some()
                || !matches!(self.screen, super::Screen::Map))
        {
            self.close_called();
            return true;
        }
        false
    }
}
