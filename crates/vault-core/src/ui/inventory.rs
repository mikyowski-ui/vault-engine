//! SPEC-014 §3.34 the player inventory window: its slots, the drop targets
//! in their test order, the cursor art, and the arrow cursor's item menu.
//! Rectangles are screen coordinates; the window is at (80, 0).
use super::Rect;
use crate::skills::ItemEntry;

pub const WINDOW: [i32; 2] = [80, 0];
/// Visible list rows.
pub const ROWS: usize = 6;
pub const LEFT_HAND: Rect = Rect::new(234, 286, 90, 61);
pub const RIGHT_HAND: Rect = Rect::new(325, 286, 90, 61);
pub const ARMOR: Rect = Rect::new(234, 183, 90, 61);
/// The player figure, x 256-316 and y 37-137.
pub const FIGURE: Rect = Rect::new(256, 37, 61, 101);
/// The cursors: the hand (list index 286) and the arrow (250).
pub const HAND_CURSOR: &str = "hand.frm";
pub const ARROW_CURSOR: &str = "actarrow.frm";

/// List row `i`: (124, 35 + 48i), 64 by 48.
pub fn row_rect(row: usize) -> Rect {
    Rect::new(124, 35 + 48 * row as i32, 64, 48)
}
/// The list row under a point of the list area (x 124-188, y 35 through
/// the bottom of the visible rows). The reference computes `trunc((y -
/// 39) / 48)`, so a row's top 4 pixels map to the row above; under the
/// safe rule (SPEC-013 §3.16) the row is `trunc((y - 35) / 48)`.
pub fn list_row([x, y]: [i32; 2]) -> Option<usize> {
    let bottom = 35 + 48 * ROWS as i32 - 1;
    ((124..=188).contains(&x) && (35..=bottom).contains(&y)).then(|| ((y - 35) / 48) as usize)
}

/// Where a drag ends, tested in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropTarget {
    List(usize),
    LeftHand,
    RightHand,
    Armor,
    Figure,
    Nowhere,
}
pub fn drop_target(point: [i32; 2]) -> DropTarget {
    if let Some(row) = list_row(point) {
        DropTarget::List(row)
    } else if LEFT_HAND.contains(point) {
        DropTarget::LeftHand
    } else if RIGHT_HAND.contains(point) {
        DropTarget::RightHand
    } else if ARMOR.contains(point) {
        DropTarget::Armor
    } else if FIGURE.contains(point) {
        DropTarget::Figure
    } else {
        DropTarget::Nowhere
    }
}
/// The arrow cursor's menu in the player window: an unloadable weapon has
/// Look, Unload, Drop, Cancel; a container held in a hand or worn Look,
/// Drop, Cancel; a usable or use-on item Look, Use, Drop, Cancel; anything
/// else Look, Drop, Cancel.
pub fn player_menu(unloadable: bool, held_container: bool, usable: bool) -> Vec<ItemEntry> {
    let middle = if unloadable {
        Some(ItemEntry::Unload)
    } else if held_container {
        None
    } else if usable {
        Some(ItemEntry::Use)
    } else {
        None
    };
    [
        Some(ItemEntry::Look),
        middle,
        Some(ItemEntry::Drop),
        Some(ItemEntry::Cancel),
    ]
    .into_iter()
    .flatten()
    .collect()
}
