//! SPEC-014 §3.34 the player inventory and use-on windows: art and layout,
//! the slots, the drop targets in their test order, the stats panel's
//! examine wrapping, the rotating figure, the cursor art, and the arrow
//! cursor's item menu. Rectangles are screen coordinates; both windows are
//! at (80, 0).
use super::Rect;
use crate::skills::ItemEntry;

pub const WINDOW: [i32; 2] = [80, 0];
/// The window art: `invbox.frm` (`intrface.lst` 48, 499 by 377) and the
/// use-on window's `use.frm` (113, 292 by 376).
pub const INVENTORY_ART: &str = "invbox.frm";
pub const USE_ON_ART: &str = "use.frm";
/// Visible list rows.
pub const ROWS: usize = 6;
/// Scroll buttons, 22 by 23, with their up and down art (49-52); keys Up
/// and Down.
pub const SCROLL_UP: (Rect, &str, &str) =
    (Rect::new(208, 39, 22, 23), "invupout.frm", "invupin.frm");
pub const SCROLL_DOWN: (Rect, &str, &str) =
    (Rect::new(208, 62, 22, 23), "invdnout.frm", "invdnin.frm");
/// The red buttons (`lilredup.frm` 8 / `lilreddn.frm` 9), sending Esc:
/// Done in the inventory window, Cancel in the use-on window.
pub const RED_BUTTON: (&str, &str) = ("lilredup.frm", "lilreddn.frm");
pub const DONE: [i32; 2] = [517, 329];
pub const CANCEL: [i32; 2] = [313, 328];
/// The rotating figure's 60-by-100 box; an invisible button over it goes
/// up one container level.
pub const FIGURE_BOX: Rect = Rect::new(256, 37, 60, 100);
/// The figure turns one facing at most every `trunc(1000 / 6)` ms.
pub const FIGURE_TURN_MS: u64 = 1000 / 6;
/// The stats panel (inventory window only).
pub const STATS: Rect = Rect::new(377, 44, 152, 188);
/// Under the safe rule an item held in both hands is drawn once into this
/// 180-by-61 area (the reference draws it at the right-hand position).
pub const BOTH_HANDS: Rect = Rect::new(234, 286, 180, 61);
/// The action menu's window edges, the right edge for the flip test and
/// the bottom for the upward shift: inventory, use-on, loot and barter.
pub const INVENTORY_EDGES: [i32; 2] = [579, 377];
pub const USE_ON_EDGES: [i32; 2] = [372, 376];
pub const LOOT_EDGES: [i32; 2] = [617, 376];
pub const BARTER_EDGES: [i32; 2] = [560, 470];

/// The item art in list row `row`: scaled into 56 by 40.
pub fn icon_rect(row: usize) -> Rect {
    Rect::new(128, 39 + 48 * row as i32, 56, 40)
}
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
/// §3.34 the action menu's placement for a press at `point` with `count`
/// entries in a window with `edges`: its top-left is at the pointer, or
/// its top-right when `x + 68` reaches the window's right edge (flipped);
/// when `y + 40 * count - 1` reaches the window's bottom the menu shifts up
/// by that amount less the bottom, plus 2. Returns the menu's top and
/// whether it is flipped.
pub fn menu_place(point: [i32; 2], count: usize, edges: [i32; 2]) -> (i32, bool) {
    let [x, y] = point;
    let last = y + 40 * count as i32 - 1;
    let shift = if last >= edges[1] {
        last - edges[1] + 2
    } else {
        0
    };
    (y - shift, x + 68 >= edges[0])
}
/// §3.34 the 40-by-40 normal and highlighted icons of an item menu entry.
pub fn entry_icons(entry: ItemEntry) -> (&'static str, &'static str) {
    match entry {
        ItemEntry::Look => ("lookn.frm", "lookh.frm"),
        ItemEntry::Use => ("usegetn.frm", "usegeth.frm"),
        ItemEntry::Drop => ("dropn.frm", "droph.frm"),
        ItemEntry::Unload => ("unloadn.frm", "unloadh.frm"),
        ItemEntry::Cancel => ("canceln.frm", "cancelh.frm"),
    }
}
/// §3.34 examine text wrapping at `width` pixels. A string no wider fits
/// on one line. A wider one is cut at a space, after its first word or at a
/// later space while the cut text stays narrower than `width`, and the rest
/// continues on the next line. A first word wider than `width` drops that
/// line and all remaining text; a string with no space after its first
/// character is drawn whole (clipped).
pub fn wrap_examine(text: &[u8], width: i32, measure: impl Fn(&[u8]) -> i32) -> Vec<Vec<u8>> {
    let mut lines = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        if measure(rest) <= width {
            lines.push(rest.to_vec());
            break;
        }
        let mut spaces = (1..rest.len()).filter(|index| rest[*index] == b' ');
        let Some(first) = spaces.next() else {
            lines.push(rest.to_vec());
            break;
        };
        if measure(&rest[..first]) > width {
            break;
        }
        let cut = spaces
            .take_while(|space| measure(&rest[..*space]) < width)
            .last()
            .unwrap_or(first);
        lines.push(rest[..cut].to_vec());
        rest = &rest[cut + 1..];
    }
    lines
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
