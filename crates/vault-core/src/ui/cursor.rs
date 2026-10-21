//! SPEC-014 §3.26-3.29: cursor shapes, in-world cursor modes, hover
//! feedback, clicks, the action menu and edge scrolling.
use super::ColorKey;

/// §3.26 cursor art, size and hotspot (computed from the corpus art by the
/// SPEC-001 §3.5 rule; `None` for a 1-by-1 or rule-only cursor).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorArt {
    pub art: &'static str,
    pub size: [i32; 2],
    pub hotspot: Option<[i32; 2]>,
}
const fn art(art: &'static str, size: [i32; 2], hotspot: Option<[i32; 2]>) -> CursorArt {
    CursorArt { art, size, hotspot }
}
pub const ARROW: CursorArt = art("stdarrow.frm", [18, 18], Some([0, 0]));
pub const BLANK: CursorArt = art("blank.frm", [1, 1], None);
pub const SMALL_UP: CursorArt = art("suparrow.frm", [11, 10], Some([5, 0]));
pub const SMALL_DOWN: CursorArt = art("sdnarrow.frm", [11, 10], Some([5, 9]));
pub const CROSSHAIR: CursorArt = art("crsshair.frm", [21, 21], Some([10, 10]));
pub const PLUS: CursorArt = art("plus.frm", [17, 17], Some([8, 8]));
pub const DESTROY: CursorArt = art("destroy.frm", [33, 30], Some([0, 0]));
pub const USE_CROSSHAIR: CursorArt = art("crossuse.frm", [21, 21], Some([10, 10]));
pub const WATCH: CursorArt = art("watch.frm", [12, 15], None);
pub const PLANET: CursorArt = art("wait.frm", [64, 64], Some([32, 32]));
pub const ANIMATED_WATCH: CursorArt = art("wait2.frm", [12, 15], None);
/// Scroll cursors in direction order NW, N, NE, E, SE, S, SW, W.
pub const SCROLL: [CursorArt; 8] = [
    art("scrnwest.frm", [27, 28], Some([0, 0])),
    art("scrnorth.frm", [31, 26], Some([15, 0])),
    art("scrneast.frm", [28, 28], Some([27, 0])),
    art("screast.frm", [25, 32], Some([24, 16])),
    art("scrseast.frm", [28, 28], Some([27, 27])),
    art("scrsouth.frm", [32, 26], Some([16, 25])),
    art("scrswest.frm", [27, 28], Some([0, 27])),
    art("scrwest.frm", [25, 32], Some([0, 16])),
];
/// Blocked scroll cursors in the same order.
pub const SCROLL_BLOCKED: [CursorArt; 8] = [
    art("scrnwx.frm", [37, 38], Some([0, 0])),
    art("scrnx.frm", [31, 46], Some([15, 0])),
    art("scrnex.frm", [37, 39], Some([36, 0])),
    art("screx.frm", [40, 33], Some([39, 16])),
    art("scrsex.frm", [35, 37], Some([34, 36])),
    art("scrsx.frm", [32, 41], Some([16, 40])),
    art("scrswx.frm", [34, 38], Some([0, 37])),
    art("scrwx.frm", [40, 34], Some([0, 16])),
];
/// Animated cursors: art frames at 10 fps, looping.
pub const ANIMATED: [(&str, u32); 2] = [("wait.frm", 7), ("wait2.frm", 6)];
pub fn animated_frame(elapsed_ms: u64, frames: u32) -> u32 {
    (elapsed_ms / 100 % u64::from(frames.max(1))) as u32
}

/// §3.27 in-world cursor modes by numeric value: movement 0, arrow 1,
/// crosshair 2, use crosshair 3, then the seven skill modes 4-10 (First
/// Aid, Doctor, Lockpick, Steal, Traps, Science, Repair).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Movement,
    Arrow,
    Crosshair,
    UseCrosshair,
    /// A targeted skill by its SPEC-006 skill index.
    Skill(u8),
}
pub const SKILL_MODES: [u8; 7] = [6, 7, 9, 10, 11, 12, 13];
impl Mode {
    pub fn value(self) -> i32 {
        match self {
            Self::Movement => 0,
            Self::Arrow => 1,
            Self::Crosshair => 2,
            Self::UseCrosshair => 3,
            Self::Skill(skill) => {
                4 + SKILL_MODES.iter().position(|s| *s == skill).unwrap_or(0) as i32
            }
        }
    }
    /// In-world art over the map.
    pub fn art(self) -> &'static str {
        match self {
            Self::Movement => "msef003.frm",
            Self::Arrow => "actarrow.frm",
            Self::Crosshair => "acrshair.frm",
            Self::UseCrosshair | Self::Skill(_) => "crossuse.frm",
        }
    }
}
/// A right-click in the map area or `m`: `(current + 1) mod 3` over
/// movement, arrow and crosshair. Out of combat a crosshair becomes
/// movement; in combat it is skipped only for a non-weapon in the active
/// hand. Safe rule: use and skill modes return to movement.
pub fn cycle_mode(current: Mode, combat: bool, weapon_in_hand: bool) -> Mode {
    let next = match current {
        Mode::UseCrosshair | Mode::Skill(_) => return Mode::Movement,
        Mode::Movement => Mode::Arrow,
        Mode::Arrow => Mode::Crosshair,
        Mode::Crosshair => Mode::Movement,
    };
    match next {
        Mode::Crosshair if !combat => Mode::Movement,
        Mode::Crosshair if !weapon_in_hand => Mode::Movement,
        other => other,
    }
}
/// The reference transition by numeric value, for every mode (P-08: kept
/// for the tests that pin the `[May-improve]` difference).
#[cfg(test)]
pub fn reference_cycle(current: Mode) -> i32 {
    (current.value() + 1) % 3
}

/// Hover feedback waits until the pointer rests 250 ms on one pixel.
pub const HOVER_REST_MS: u64 = 250;
/// Movement hover in combat: path length times 1, 4 with one crippled leg
/// or 8 with both, less free movement, at least 0.
pub fn movement_cost(path: i32, crippled_legs: u8, free_move: i32) -> i32 {
    let factor = match crippled_legs {
        0 => 1,
        1 => 4,
        _ => 8,
    };
    (path * factor - free_move).max(0)
}
/// The movement hover text: `None` without a path is a red `X`; out of
/// combat nothing; in combat the cost in white when affordable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveHover {
    Nothing,
    Cost(i32),
    Blocked,
}
pub fn move_hover(
    path: Option<i32>,
    combat: bool,
    crippled_legs: u8,
    free_move: i32,
    ap: i32,
) -> MoveHover {
    let Some(path) = path else {
        return MoveHover::Blocked;
    };
    if !combat {
        return MoveHover::Nothing;
    }
    let cost = movement_cost(path, crippled_legs, free_move);
    if cost <= ap {
        MoveHover::Cost(cost)
    } else {
        MoveHover::Blocked
    }
}
pub const COST_COLOR: ColorKey = ColorKey::WHITE;
pub const BLOCKED_COLOR: ColorKey = ColorKey::RED;
/// §3.27 the arrow hover's opaque one-color item outline.
pub const ITEM_OUTLINE: ColorKey = ColorKey(30632);

/// What the pointer is over in arrow mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Item { container: bool },
    Player,
    Critter { talkable: bool },
    Scenery { usable: bool },
    Wall { usable: bool },
}
/// The primary action of a hover or arrow click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Primary {
    Use,
    Rotate,
    Talk,
    Look,
}
pub fn primary_action(target: Target, combat: bool) -> Primary {
    match target {
        Target::Item { .. } => Primary::Use,
        Target::Player => Primary::Rotate,
        Target::Critter { talkable: true } if !combat => Primary::Talk,
        Target::Critter { talkable: true } => Primary::Look,
        Target::Critter { talkable: false } => Primary::Use,
        Target::Scenery { usable: true } => Primary::Use,
        Target::Scenery { usable: false } | Target::Wall { .. } => Primary::Look,
    }
}
/// The action-pick art (69 by 62): mirrored when `x + 68` reaches 640, and
/// shifted up by `y + 39 - 381 + 2` when `y + 39` reaches 381.
pub fn pick_art(x: i32, y: i32) -> (&'static str, i32) {
    let art = if x + 68 >= 640 {
        "actarrom.frm"
    } else {
        "actpick.frm"
    };
    let shift = if y + 39 >= 381 { y + 39 - 381 + 2 } else { 0 };
    (art, shift)
}
/// The crosshair hover text for a critter: `NN%`, or ` X ` when the attack
/// is impossible.
pub fn hit_text(chance: Option<i32>) -> Vec<u8> {
    match chance {
        Some(chance) => format!("{chance}%").into_bytes(),
        None => b" X ".to_vec(),
    }
}

/// §3.28 action menu entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Use,
    Look,
    Inventory,
    Skill,
    Rotate,
    Talk,
    Cancel,
}
impl Entry {
    /// Normal and highlighted icon art.
    pub fn icons(self) -> (&'static str, &'static str) {
        match self {
            Self::Use => ("usegetn.frm", "usegeth.frm"),
            Self::Look => ("lookn.frm", "lookh.frm"),
            Self::Inventory => ("invenn.frm", "invenh.frm"),
            Self::Skill => ("skilln.frm", "skillh.frm"),
            Self::Rotate => ("rotaten.frm", "rotateh.frm"),
            Self::Talk => ("talkn.frm", "talkh.frm"),
            Self::Cancel => ("canceln.frm", "cancelh.frm"),
        }
    }
}
pub fn menu_entries(target: Target, combat: bool) -> Vec<Entry> {
    let mut entries = Vec::new();
    match target {
        Target::Item { container } => {
            entries.extend([Entry::Use, Entry::Look]);
            if container {
                entries.extend([Entry::Inventory, Entry::Skill]);
            }
        }
        Target::Player | Target::Critter { .. } => {
            match target {
                Target::Player => entries.push(Entry::Rotate),
                Target::Critter { talkable: true } if !combat => entries.push(Entry::Talk),
                Target::Critter { talkable: false } => entries.push(Entry::Use),
                _ => {}
            }
            entries.extend([Entry::Look, Entry::Inventory, Entry::Skill]);
        }
        Target::Scenery { usable } => {
            if usable {
                entries.push(Entry::Use);
            }
            entries.extend([Entry::Look, Entry::Inventory, Entry::Skill]);
        }
        Target::Wall { usable } => {
            entries.push(Entry::Look);
            if usable {
                entries.push(Entry::Inventory);
            }
        }
    }
    entries.push(Entry::Cancel);
    entries
}
/// The menu's upward shift when `y + 40 * count - 1` reaches 381.
pub fn menu_shift(y: i32, count: usize) -> i32 {
    let bottom = y + 40 * count as i32 - 1;
    if bottom >= 381 { bottom - 381 + 2 } else { 0 }
}
/// The held-button repeat that opens the menu, and its period.
pub const MENU_HOLD_MS: u64 = 250;
pub const MENU_OPEN_SOUND: &str = "iaccuxx1";
/// Menu tracking: each vertical move of more than 10 pixels from the anchor
/// moves the highlight one entry. Safe rule: the index is clamped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuTracker {
    pub highlight: usize,
    pub anchor: i32,
    pub count: usize,
}
impl MenuTracker {
    pub fn new(y: i32, count: usize) -> Self {
        Self {
            highlight: 0,
            anchor: y,
            count,
        }
    }
    pub fn moved(&mut self, y: i32) {
        if y - self.anchor > 10 {
            self.highlight = (self.highlight + 1).min(self.count.saturating_sub(1));
            self.anchor = y;
        } else if self.anchor - y > 10 {
            self.highlight = self.highlight.saturating_sub(1);
            self.anchor = y;
        }
    }
}

/// §3.29 edge scrolling: the direction for a pointer on an edge pixel, in
/// the scroll-cursor order (NW, N, NE, E, SE, S, SW, W).
pub fn edge_direction(x: i32, y: i32) -> Option<usize> {
    let (left, right, top, bottom) = (x <= 0, x >= 639, y <= 0, y >= 479);
    Some(match (left, right, top, bottom) {
        (true, _, true, _) => 0,
        (_, true, true, _) => 2,
        (_, true, _, true) => 4,
        (true, _, _, true) => 6,
        (_, _, true, _) => 1,
        (_, true, _, _) => 3,
        (_, _, _, true) => 5,
        (true, _, _, _) => 7,
        _ => return None,
    })
}
/// Pixel step per scroll direction.
pub fn edge_step(direction: usize) -> [i32; 2] {
    let [dx, dy] = [
        [-1, -1],
        [0, -1],
        [1, -1],
        [1, 0],
        [1, 1],
        [0, 1],
        [-1, 1],
        [-1, 0],
    ][direction];
    [32 * dx, 24 * dy]
}
pub const EDGE_PERIOD_MS: u64 = 33;

/// §3.29 the map border in mirrored columns (`199 - column`) and rows, both
/// exclusive. The reference applies the parity correction meant for the
/// right bound (156) to the left one (43, made 42); under the safe rule the
/// bounds are 43 and 155, so centers lie in columns 45-155 and rows 45-154.
pub const BORDER_COLUMNS: (i32, i32) = (43, 155);
pub const BORDER_ROWS: (i32, i32) = (44, 155);
/// §3.29 whether a view center lies inside the map border. Every map is 200
/// by 200 hexes and the border does not depend on its contents.
pub fn inside_border(hex: i32) -> bool {
    if !(0..40000).contains(&hex) {
        return false;
    }
    let (mirrored, row) = (199 - hex % 200, hex / 200);
    BORDER_COLUMNS.0 < mirrored
        && mirrored < BORDER_COLUMNS.1
        && BORDER_ROWS.0 < row
        && row < BORDER_ROWS.1
}
/// §3.29 the player-distance test of scroll limiting. Positions are hex
/// cell corners in the current view's pixels: `target` is the new center,
/// `player` the player's hex. The view reference is (304, 182), or (272,
/// 182) when the current center hex's column is even. The target is refused
/// when it is farther from the player than the reference on either axis and
/// at least 480 pixels across or 400 down from the player, so a step toward
/// the player is never refused.
pub fn distance_refused(target: [i64; 2], player: [i64; 2], center: i32) -> bool {
    let reference: [i64; 2] = if (center % 200) % 2 == 0 {
        [272, 182]
    } else {
        [304, 182]
    };
    let [dx, dy] = [0, 1].map(|axis| (player[axis] - target[axis]).abs());
    (dx > (player[0] - reference[0]).abs() || dy > (player[1] - reference[1]).abs())
        && (dx >= 480 || dy >= 400)
}
