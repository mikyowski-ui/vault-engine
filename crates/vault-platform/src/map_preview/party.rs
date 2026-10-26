//! The `enhanced` HUD's party status pills: one compact amber-and-black
//! plate per active companion ([`vault_core::WorldHost::party_status`]),
//! with its name, current and maximum hit points and distance mode. Engine
//! extension, not part of any specification; under Vanilla, or with no
//! companion, nothing draws and no click is taken.
//!
//! The pills are a fixed stack in the interface frame's top-left corner
//! (frame pixels): pill k is [`pill`]`(k)`, 152 by 16 at (4, 4 + 18k), for
//! at most [`MAX_PILLS`], so the stack ends at y 111. That keeps it clear of
//! the bar and the indicator boxes (y 358 down), the debug overlay's panel
//! (x 352 right) and the floating text, which is in the world; the modal
//! windows draw over it, and it shows on the map screen only. Each pill's
//! first 118 pixels are its name/HP region, which opens the companion's
//! window (Gear under Fallout 1, the party Trade window under Fallout 2),
//! and the last 34 its mode region, which cycles the distance.
use super::{MapPreview, Screen};
use crate::interface::Painter;
use vault_core::{
    ObjectRef,
    companion::PartyStatus,
    ui::{ColorKey, Rect},
};
use vault_render::IndexedImage;

/// The first pill's top-left corner, a pill's size and the stack's step.
const ORIGIN: [i32; 2] = [4, 4];
const SIZE: [i32; 2] = [152, 16];
const STRIDE: i32 = 18;
/// The mode region's width, at a pill's right end.
const MODE_WIDTH: i32 = 34;
/// The most pills drawn; further companions have none.
pub(crate) const MAX_PILLS: usize = 6;
/// The status line when a pill's window or mode is refused (in combat, or
/// a member that cannot take it).
pub(crate) const REFUSED: &[u8] = b"You can't do that now.";

/// Pill `k`'s rectangle.
pub(crate) fn pill(k: usize) -> Rect {
    Rect::new(ORIGIN[0], ORIGIN[1] + STRIDE * k as i32, SIZE[0], SIZE[1])
}
/// Pill `k`'s name/HP region and mode region.
pub(crate) fn regions(k: usize) -> [Rect; 2] {
    let Rect { x, y, w, h } = pill(k);
    [
        Rect::new(x, y, w - MODE_WIDTH, h),
        Rect::new(x + w - MODE_WIDTH, y, MODE_WIDTH, h),
    ]
}

/// What a press on a pill reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PillRegion {
    /// The name and hit points: the companion's window.
    Window,
    /// The distance mode.
    Mode,
}

impl MapPreview {
    /// The pills shown: on the map screen outside a conversation, the
    /// host's active companions (none without the enhanced preset), at
    /// most [`MAX_PILLS`].
    pub(crate) fn party_pills(&self) -> Vec<PartyStatus> {
        if !matches!(self.screen, Screen::Map) || self.host.world.dialogue.open {
            return Vec::new();
        }
        let mut pills = self.host.party_status();
        pills.truncate(MAX_PILLS);
        pills
    }
    /// Draw the pills: an amber frame with rounded corners, the name and
    /// `hp/max` amber on black, and the mode label black on amber. The
    /// colors are the frame palette's nearest to the two keys.
    pub(crate) fn draw_party_pills(&self, painter: &Painter, image: &mut IndexedImage) {
        let amber = painter.color(ColorKey::AMBER);
        let black = painter.color(ColorKey::BLACK);
        for (k, status) in self.party_pills().iter().enumerate() {
            let Rect { x, y, w, h } = pill(k);
            image.fill_rect([x + 1, y, w - 2, h], amber);
            image.fill_rect([x, y + 1, w, h - 2], amber);
            let [name, mode] = regions(k);
            image.fill_rect([x + 1, y + 1, name.w - 1, h - 2], black);
            let text_y = y + (h - 8) / 2;
            let hp = format!("{}/{}", status.hp, status.max_hp);
            let hp_width = painter.width(101, hp.as_bytes());
            let hp_x = name.x + name.w - 4 - hp_width;
            painter.text(
                image,
                101,
                ColorKey::AMBER,
                hp.as_bytes(),
                [hp_x, text_y],
                hp_width,
            );
            painter.text(
                image,
                101,
                ColorKey::AMBER,
                &status.name,
                [x + 4, text_y],
                hp_x - x - 10,
            );
            let label = status.mode.unwrap_or("---");
            painter.centered(
                image,
                101,
                ColorKey::BLACK,
                label.as_bytes(),
                [mode.x, text_y],
                mode.w,
            );
        }
    }
    /// The pill and region under a frame pixel.
    pub(crate) fn party_pill_at(&self, point: [i32; 2]) -> Option<(ObjectRef, PillRegion)> {
        self.party_pills()
            .iter()
            .enumerate()
            .find_map(|(k, status)| {
                let [name, mode] = regions(k);
                if name.contains(point) {
                    Some((status.critter, PillRegion::Window))
                } else if mode.contains(point) {
                    Some((status.critter, PillRegion::Mode))
                } else {
                    None
                }
            })
    }
    /// A left press at frame pixel `point`; returns whether a pill took
    /// it. The name/HP region opens the companion's window as its action
    /// menu entry does (Gear under Fallout 1, the party Trade window under
    /// Fallout 2); the mode region cycles the distance. A refusal shows
    /// [`REFUSED`] on the status line, or the talk exception's message
    /// when the Fallout 2 member is out of reach.
    pub(crate) fn party_pill_click(&mut self, point: [i32; 2]) -> bool {
        let Some((critter, region)) = self.party_pill_at(point) else {
            return false;
        };
        if self.host.world.ui_disabled {
            return true;
        }
        self.press();
        let allowed = match region {
            PillRegion::Window if self.host.world.fallout2.is_some() => {
                let allowed = self.host.can_open_party_trade(critter);
                if allowed {
                    self.open_party_trade(critter);
                }
                allowed
            }
            PillRegion::Window => {
                let allowed = self.host.is_companion(critter);
                if allowed {
                    self.open_companion_gear(critter);
                }
                allowed
            }
            // A Fallout 2 value its allowed set cannot leave stays, silently.
            PillRegion::Mode => {
                !self.host.world.combat && {
                    self.host.cycle_party_distance(critter);
                    true
                }
            }
        };
        if !allowed {
            self.status = REFUSED.to_vec();
        }
        true
    }
}

#[cfg(test)]
#[path = "party_tests.rs"]
mod tests;
