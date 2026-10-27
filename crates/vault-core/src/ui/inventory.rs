//! SPEC-014 §3.34 the player inventory and use-on windows: art and layout,
//! the slots, the drop targets in their test order, the stats panel's
//! examine wrapping, the rotating figure, the cursor art, and the arrow
//! cursor's item menu. Rectangles are screen coordinates; both windows are
//! at (80, 0).
use super::{Layout, Rect};
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

// SPEC-020 §2.1 (Decision D-2): the inventory, use-on, loot and barter
// windows' layouts and list state. Rectangles are frame (screen) pixels.
// Barter's trade rules stay in `crate::barter`.

/// A list scrolls one row at a time: up from a positive offset, down while
/// `offset + rows` is below `count`. Returns whether it moved.
pub fn scroll_list(offset: &mut usize, up: bool, count: usize, rows: usize) -> bool {
    let before = *offset;
    if up {
        *offset = offset.saturating_sub(1);
    } else if *offset + rows < count {
        *offset += 1;
    }
    *offset != before
}
/// The use-on window's extra keys over `rows` rows: Home, End, PageUp and
/// PageDown (which stops at the last full page).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListStep {
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}
pub fn list_step(offset: usize, step: ListStep, count: usize, rows: usize) -> usize {
    match step {
        ListStep::Up => offset.saturating_sub(1),
        ListStep::Down if offset + rows < count => offset + 1,
        ListStep::Down => offset,
        ListStep::Home => 0,
        ListStep::End => count.saturating_sub(rows),
        ListStep::PageUp => offset.saturating_sub(rows),
        ListStep::PageDown => {
            let next = offset + rows;
            if next + rows >= count {
                count.saturating_sub(rows)
            } else {
                next
            }
        }
    }
}

/// A control of the inventory and use-on windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryControl {
    /// Done (inventory) or Cancel (use-on): a red button sending Esc.
    Close,
    ScrollUp,
    ScrollDown,
    /// Visible list row 0-5.
    Row(usize),
    LeftHand,
    RightHand,
    Armor,
    /// The figure box: up one container level (hand cursor only).
    Figure,
}
/// §3.34 the window's controls in press order: the red button (sized by
/// its up art, else 15 by 16), the scroll buttons, the list rows, the
/// inventory window's hand and armor slots, and the figure.
pub fn inventory_layout(use_on: bool, red_size: Option<[i32; 2]>) -> Layout<InventoryControl> {
    let red = if use_on { CANCEL } else { DONE };
    let mut layout = Layout::new()
        .with(
            InventoryControl::Close,
            Rect::at(red, red_size.unwrap_or(super::RED_BUTTON.2)),
        )
        .with(InventoryControl::ScrollUp, SCROLL_UP.0)
        .with(InventoryControl::ScrollDown, SCROLL_DOWN.0);
    layout.extend((0..ROWS).map(|row| (InventoryControl::Row(row), row_rect(row))));
    if !use_on {
        layout.extend([
            (InventoryControl::LeftHand, LEFT_HAND),
            (InventoryControl::RightHand, RIGHT_HAND),
            (InventoryControl::Armor, ARMOR),
        ]);
    }
    layout.with(InventoryControl::Figure, FIGURE)
}
/// What an inventory press or key leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryOutcome {
    /// Nothing (the input is still swallowed).
    None,
    /// Esc, Done or Cancel: the window closes.
    Close,
    /// The list moved to this offset.
    Scrolled(usize),
    /// A press on list entry `index` (offset plus row): the hand lifts it
    /// (in the use-on window, uses it on the target); the arrow waits for
    /// the release or the hold pulse.
    Entry(usize),
    /// A press on a hand or armor slot (inventory window only); it acts
    /// only when the slot holds an item.
    Slot(InventoryControl),
    /// The figure, in the hand cursor: up one container level.
    UpLevel,
    /// A right press switched the cursor: `true` is the arrow.
    Cursor(bool),
    /// Ctrl+Q or Ctrl+X: the quit confirmation.
    QuitConfirm,
}
/// A key in the inventory or use-on window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryKey {
    Escape,
    Step(ListStep),
    Quit,
}
/// §3.34 the inventory or use-on window's list state: the scroll offset
/// and the cursor (hand or arrow). The lifted item, the item menu and the
/// move counter are the caller's (they hold world objects).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InventoryWindow {
    pub use_on: bool,
    pub offset: usize,
    /// The arrow cursor (else the hand).
    pub arrow: bool,
}
impl InventoryWindow {
    pub fn new(use_on: bool) -> Self {
        Self {
            use_on,
            ..Self::default()
        }
    }
    pub fn layout(&self, red_size: Option<[i32; 2]>) -> Layout<InventoryControl> {
        inventory_layout(self.use_on, red_size)
    }
    /// A right press switches the cursor.
    pub fn right_press(&mut self) -> InventoryOutcome {
        self.arrow = !self.arrow;
        InventoryOutcome::Cursor(self.arrow)
    }
    /// A left press over a list of `count` entries.
    pub fn press(
        &mut self,
        point: [i32; 2],
        count: usize,
        red_size: Option<[i32; 2]>,
    ) -> InventoryOutcome {
        match self.layout(red_size).hit(point) {
            Some(control) => self.dispatch(control, count),
            None => InventoryOutcome::None,
        }
    }
    /// Act on a control.
    pub fn dispatch(&mut self, control: InventoryControl, count: usize) -> InventoryOutcome {
        match control {
            InventoryControl::Close => InventoryOutcome::Close,
            InventoryControl::ScrollUp | InventoryControl::ScrollDown => {
                let up = control == InventoryControl::ScrollUp;
                scroll_list(&mut self.offset, up, count, ROWS);
                InventoryOutcome::Scrolled(self.offset)
            }
            InventoryControl::Row(row) => {
                let index = self.offset + row;
                if index < count {
                    InventoryOutcome::Entry(index)
                } else {
                    InventoryOutcome::None
                }
            }
            InventoryControl::LeftHand | InventoryControl::RightHand | InventoryControl::Armor => {
                if self.use_on {
                    InventoryOutcome::None
                } else {
                    InventoryOutcome::Slot(control)
                }
            }
            InventoryControl::Figure if !self.arrow => InventoryOutcome::UpLevel,
            InventoryControl::Figure => InventoryOutcome::None,
        }
    }
    /// Keys: Esc, Up, Down and the quit keys in both windows; the use-on
    /// window adds Home, End, PageUp and PageDown.
    pub fn key(&mut self, key: InventoryKey, count: usize) -> InventoryOutcome {
        match key {
            InventoryKey::Escape => InventoryOutcome::Close,
            InventoryKey::Quit => InventoryOutcome::QuitConfirm,
            InventoryKey::Step(step @ (ListStep::Up | ListStep::Down)) => {
                self.offset = list_step(self.offset, step, count, ROWS);
                InventoryOutcome::Scrolled(self.offset)
            }
            InventoryKey::Step(step) if self.use_on => {
                self.offset = list_step(self.offset, step, count, ROWS);
                InventoryOutcome::Scrolled(self.offset)
            }
            InventoryKey::Step(_) => InventoryOutcome::None,
        }
    }
}

/// SPEC-013 §3.16-3.17 the loot window (at `crate::skills::LOOT_ORIGIN`),
/// window-relative: Done, the engine-drawn Take All, the companion gear
/// window's Distance and WIELD with its label, the body-cycling arrows
/// (forward is the right one), the portraits' hot zones and the scroll
/// buttons (pane, up).
pub const LOOT_DONE: Rect = Rect::new(288, 328, 15, 16);
pub const LOOT_TAKE_ALL: Rect = Rect::new(214, 236, 108, 20);
pub const LOOT_DISTANCE: Rect = Rect::new(214, 210, 108, 20);
pub const LOOT_WIELD: Rect = Rect::new(214, 184, 108, 20);
pub const LOOT_WIELD_LABEL: Rect = Rect::new(214, 168, 108, 12);
pub const LOOT_CYCLE: [(Rect, bool, &str, &str); 2] = [
    (Rect::new(436, 162, 20, 18), false, "slu.frm", "sld.frm"),
    (Rect::new(456, 162, 20, 18), true, "sru.frm", "srd.frm"),
];
pub const LOOT_PORTRAITS: [(bool, Rect); 2] = [
    (false, Rect::new(176, 37, 60, 100)),
    (true, Rect::new(297, 37, 60, 100)),
];
pub const LOOT_SCROLL: [(bool, bool, Rect); 4] = [
    (false, true, Rect::new(128, 39, 22, 23)),
    (false, false, Rect::new(128, 62, 22, 23)),
    (true, true, Rect::new(379, 39, 22, 23)),
    (true, false, Rect::new(379, 62, 22, 23)),
];
/// An item slot's size in the loot and barter windows.
pub const SLOT_SIZE: [i32; 2] = [64, 48];
/// A loot pane's slot `row` (window-relative; `target` is the right pane).
pub fn loot_slot(target: bool, row: usize) -> Rect {
    let at = if target {
        crate::skills::loot_target_slot(row)
    } else {
        crate::skills::loot_player_slot(row)
    };
    Rect::at(at, SLOT_SIZE)
}
/// A control of the loot window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LootControl {
    Done,
    Distance,
    Wield,
    TakeAll,
    Scroll {
        target: bool,
        up: bool,
    },
    Cycle {
        forward: bool,
    },
    /// A filled entry's slot.
    Slot {
        target: bool,
        row: usize,
    },
    Portrait {
        target: bool,
    },
}
/// What the loot window shows, for its layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LootView {
    /// A companion's gear window (Distance and WIELD).
    pub companion: bool,
    /// Take All is shown (the enhanced preset or Fallout 2, not stealing),
    /// with its art's size under Fallout 2 (else 108 by 20).
    pub take_all: Option<Option<[i32; 2]>>,
    /// The bodies a corpse pile cycles through.
    pub bodies: usize,
    /// Each pane's entry count and scroll offset (player, target).
    pub counts: [usize; 2],
    pub offsets: [usize; 2],
}
/// The loot window's controls on screen, in press order: Done, Distance
/// and WIELD (a companion's), Take All (when shown), the scroll buttons,
/// the cycling arrows (with more than one body), the filled entries
/// (player pane first) and the portraits.
pub fn loot_layout(view: &LootView) -> Layout<LootControl> {
    let mut layout = Layout::new().with(LootControl::Done, LOOT_DONE);
    if view.companion {
        layout.push(LootControl::Distance, LOOT_DISTANCE);
        layout.push(LootControl::Wield, LOOT_WIELD);
    }
    if let Some(size) = view.take_all {
        let rect = size.map_or(LOOT_TAKE_ALL, |size| Rect::at(LOOT_TAKE_ALL.origin(), size));
        layout.push(LootControl::TakeAll, rect);
    }
    layout.extend(LOOT_SCROLL.iter().map(|(target, up, rect)| {
        (
            LootControl::Scroll {
                target: *target,
                up: *up,
            },
            *rect,
        )
    }));
    if view.bodies > 1 {
        layout.extend(
            LOOT_CYCLE
                .iter()
                .map(|(rect, forward, ..)| (LootControl::Cycle { forward: *forward }, *rect)),
        );
    }
    for target in [false, true] {
        let pane = usize::from(target);
        let filled = view.counts[pane].saturating_sub(view.offsets[pane]);
        layout.extend(
            (0..crate::skills::LOOT_ROWS.min(filled))
                .map(|row| (LootControl::Slot { target, row }, loot_slot(target, row))),
        );
    }
    layout.extend(
        LOOT_PORTRAITS
            .iter()
            .map(|(target, rect)| (LootControl::Portrait { target: *target }, *rect)),
    );
    layout.offset(crate::skills::LOOT_ORIGIN)
}

/// SPEC-013 §3.18 the barter window at screen (0, 289) and its subwindow
/// at (80, 290): three rows per column; the four columns (player
/// inventory, player table, merchant table, merchant inventory).
pub const BARTER_WINDOW: [i32; 2] = [0, 289];
pub const BARTER_SUBWINDOW: [i32; 2] = [80, 290];
pub const BARTER_ROWS: usize = 3;
/// Per column (subwindow-relative): the slot buttons and the icon boxes.
pub const BARTER_SLOTS: [[i32; 2]; 4] = [[29, 35], [165, 20], [250, 20], [395, 35]];
pub const BARTER_ICONS: [([i32; 2], [i32; 2]); 4] = [
    ([26, 39], [59, 40]),
    ([169, 24], [56, 40]),
    ([254, 24], [56, 40]),
    ([397, 39], [56, 40]),
];
/// The scroll buttons (subwindow-relative): column, up, rectangle, and
/// the up and down art.
pub const BARTER_SCROLL: [(usize, bool, Rect, &str, &str); 8] = [
    (
        0,
        true,
        Rect::new(109, 56, 23, 24),
        "di_up1.frm",
        "di_up2.frm",
    ),
    (
        0,
        false,
        Rect::new(109, 82, 24, 25),
        "di_down1.frm",
        "di_down2.frm",
    ),
    (
        3,
        true,
        Rect::new(342, 56, 23, 24),
        "di_up1.frm",
        "di_up2.frm",
    ),
    (
        3,
        false,
        Rect::new(342, 82, 24, 25),
        "di_down1.frm",
        "di_down2.frm",
    ),
    (
        1,
        true,
        Rect::new(128, 113, 22, 23),
        "invupout.frm",
        "invupin.frm",
    ),
    (
        1,
        false,
        Rect::new(128, 136, 22, 23),
        "invdnout.frm",
        "invdnin.frm",
    ),
    (
        2,
        true,
        Rect::new(333, 113, 22, 23),
        "invupout.frm",
        "invupin.frm",
    ),
    (
        2,
        false,
        Rect::new(333, 136, 22, 23),
        "invdnout.frm",
        "invdnin.frm",
    ),
];
/// Offer and Talk (window-relative), `di_rdbt2.frm` up and `di_rdbt1.frm`
/// down, with the small buttons' sounds.
pub const BARTER_OFFER: Rect = Rect::new(41, 163, 14, 14);
pub const BARTER_TALK: Rect = Rect::new(584, 162, 14, 14);
pub const BARTER_BUTTON_SOUNDS: (&str, &str) = ("ib2p1xx1", "ib2lu1x1");
/// The enhanced party Trade window's WIELD button and its weapon plate
/// (window-relative).
pub const TRADE_WIELD: Rect = Rect::new(332, 4, 66, 15);
pub const TRADE_WIELD_LABEL: Rect = Rect::new(240, 4, 88, 15);
/// The portraits (window-relative): the player's and the merchant's; their
/// invisible buttons go up one container level.
pub const BARTER_PORTRAITS: [(bool, Rect); 2] = [
    (false, Rect::new(15, 25, 60, 100)),
    (true, Rect::new(560, 25, 60, 100)),
];
/// Drop rectangles span screen y 310-454.
pub const BARTER_DROP_Y: std::ops::RangeInclusive<i32> = 310..=454;
/// A drag from `column`: the destination column, its drop ranges in
/// screen x, and the `inventry.msg` failure message. The player-table return
/// also takes the visible player column (x 109-172) under the safe rule.
pub fn barter_route(column: usize) -> (usize, &'static [(i32, i32)], i32) {
    match column {
        0 => (1, &[(245, 309)], 26),
        1 => (0, &[(80, 144), (109, 172)], 26),
        2 => (3, &[(475, 539)], 25),
        _ => (2, &[(330, 394)], 25),
    }
}
/// The column a drag from `column` drops into at a screen point, if any.
pub fn barter_drop(column: usize, point: [i32; 2]) -> Option<usize> {
    let (to, ranges, _) = barter_route(column);
    (BARTER_DROP_Y.contains(&point[1])
        && ranges
            .iter()
            .any(|(left, right)| (*left..=*right).contains(&point[0])))
    .then_some(to)
}
/// A column's slot `row` on screen.
pub fn barter_slot(column: usize, row: usize) -> Rect {
    let [x, y] = BARTER_SLOTS[column.min(3)];
    Rect::at([x, y + 48 * row as i32], SLOT_SIZE).offset(BARTER_SUBWINDOW)
}
/// A control of the barter window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarterControl {
    Offer,
    Talk,
    TradeWield,
    Scroll {
        column: usize,
        up: bool,
    },
    /// A filled entry's slot.
    Slot {
        column: usize,
        row: usize,
    },
    Portrait {
        merchant: bool,
    },
}
/// What the barter window shows, for its layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BarterView {
    /// The enhanced Trade window's WIELD is shown.
    pub wield: bool,
    /// Each column's entry count and scroll offset.
    pub counts: [usize; 4],
    pub offsets: [usize; 4],
}
/// The barter window's controls on screen, in press order: Offer, Talk,
/// WIELD (when shown), the scroll buttons, the filled entries (column by
/// column) and the portraits.
pub fn barter_layout(view: &BarterView) -> Layout<BarterControl> {
    let mut layout = Layout::new()
        .with(BarterControl::Offer, BARTER_OFFER.offset(BARTER_WINDOW))
        .with(BarterControl::Talk, BARTER_TALK.offset(BARTER_WINDOW));
    if view.wield {
        layout.push(BarterControl::TradeWield, TRADE_WIELD.offset(BARTER_WINDOW));
    }
    layout.extend(BARTER_SCROLL.iter().map(|(column, up, rect, ..)| {
        (
            BarterControl::Scroll {
                column: *column,
                up: *up,
            },
            rect.offset(BARTER_SUBWINDOW),
        )
    }));
    for column in 0..4 {
        let filled = view.counts[column].saturating_sub(view.offsets[column]);
        layout.extend((0..BARTER_ROWS.min(filled)).map(|row| {
            (
                BarterControl::Slot { column, row },
                barter_slot(column, row),
            )
        }));
    }
    layout.extend(BARTER_PORTRAITS.iter().map(|(merchant, rect)| {
        (
            BarterControl::Portrait {
                merchant: *merchant,
            },
            rect.offset(BARTER_WINDOW),
        )
    }));
    layout
}
