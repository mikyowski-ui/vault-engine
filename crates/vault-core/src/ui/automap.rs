//! SPEC-014 §3.33 the automap window: its geometry and controls, the
//! object color rule, the mirrored hex-to-pixel mapping, and the Pip-Boy
//! automap page's cell mapping.
use super::{Armed, ColorKey, Layout, Rect};

/// The window's screen origin and size; `automap.frm` (list index 171)
/// covers it.
pub const WINDOW: [i32; 2] = [75, 0];
pub const SIZE: [i32; 2] = [519, 480];
pub const BACKGROUND: &str = "automap.frm";
/// Red buttons, window coordinates: Scanner sends `s`, Cancel sends Esc,
/// each on release.
pub const SCANNER: Rect = Rect::new(111, 454, 15, 16);
pub const CANCEL: Rect = Rect::new(277, 454, 15, 16);
/// The detail switch (a two-state toggle): `autoup.frm` (172) up for high
/// detail, `autodwn.frm` (173) down for low, with `toggle` on press and
/// release.
pub const SWITCH: Rect = Rect::new(457, 340, 42, 74);
pub const SWITCH_UP: &str = "autoup.frm";
pub const SWITCH_DOWN: &str = "autodwn.frm";
pub const SWITCH_SOUND: &str = "toggle";
/// The area short name and elevation name labels (window coordinates),
/// width 240, key 992 on a transparent background.
pub const AREA_LABEL: [i32; 2] = [150, 380];
pub const ELEVATION_LABEL: [i32; 2] = [150, 396];
pub const LABEL_WIDTH: i32 = 240;
/// Item PID 59, the Motion Sensor.
pub const MOTION_SENSOR: u32 = 59;
/// The scanner's refusal box (screen coordinates), key 32328, with
/// `misc.msg` 17 (no sensor held) or 18 (the held sensor is empty).
pub const SCANNER_DIALOG: [i32; 2] = [165, 140];
/// `item.msg` 5 for an empty sensor used from the inventory or item slot.
pub const NO_CHARGES: i32 = 5;

/// The exit-grid marker (scenery PID `0x02000031`), and the scenery PID
/// never drawn.
pub const EXIT_GRID_MARKER: u32 = 0x0200_0031;
pub const UNDRAWN_SCENERY: u32 = 0x0200_0158;
pub const BRIGHT_RED: ColorKey = ColorKey::RED;
pub const ORANGE: ColorKey = ColorKey::ERROR;
pub const BRIGHT_GREEN: ColorKey = ColorKey::GREEN;
pub const DARK_GREEN: ColorKey = ColorKey(480);

/// What the color rule needs to know about one object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Player,
    Critter { hidden: bool, dead: bool },
    Wall,
    Scenery { pid: u32 },
    Other,
}
/// The color an object is drawn in, or `None` when it is not drawn:
/// 1. with the scanner on, a critter that is not hidden and not dead is
///    bright red, seen or not;
/// 2. otherwise an unseen object is skipped; a seen exit-grid marker is
///    orange, a wall bright green, other scenery (but `0x02000158`) dark
///    green in high detail only, and the player bright red.
pub fn color(mark: Mark, seen: bool, scanner: bool, high_detail: bool) -> Option<ColorKey> {
    let critter = match mark {
        Mark::Player => Some((false, false)),
        Mark::Critter { hidden, dead } => Some((hidden, dead)),
        _ => None,
    };
    if scanner && critter.is_some_and(|(hidden, dead)| !hidden && !dead) {
        return Some(BRIGHT_RED);
    }
    if !seen {
        return None;
    }
    match mark {
        Mark::Scenery {
            pid: EXIT_GRID_MARKER,
        } => Some(ORANGE),
        Mark::Wall => Some(BRIGHT_GREEN),
        Mark::Scenery { pid } if high_detail && pid != UNDRAWN_SCENERY => Some(DARK_GREEN),
        Mark::Player => Some(BRIGHT_RED),
        _ => None,
    }
}
/// The window pixel of hex t, `(449 - 2c, 8 + 2r)` for column c and row r
/// (the map is mirrored left to right); the object paints it and the pixel
/// to its right.
pub fn pixel(hex: i32) -> Option<[i32; 2]> {
    (0..40000)
        .contains(&hex)
        .then(|| [449 - 2 * (hex % 200), 8 + 2 * (hex / 200)])
}
/// The player's extra pixels around `pixel`: a small cross.
pub fn player_cross([x, y]: [i32; 2]) -> [[i32; 2]; 3] {
    [[x - 1, y], [x, y - 1], [x, y + 1]]
}
/// The Pip-Boy automap page draws the stored raster: each cell paints 2
/// pixels wide and 1 tall on a two-row pitch from (238, 105) of the Pip-Boy
/// window, so the row between two cell rows is never painted (SPEC-014
/// §3.33, SPEC-008 §2.8); walls in key 992 and scenery in key 480.
pub const PIPBOY_ORIGIN: [i32; 2] = [238, 105];
pub fn pipboy_cell(row: usize, column: usize) -> [i32; 2] {
    [
        PIPBOY_ORIGIN[0] + 2 * column as i32,
        PIPBOY_ORIGIN[1] + 2 * row as i32,
    ]
}
pub fn pipboy_color(cell: u8) -> Option<ColorKey> {
    match cell {
        1 => Some(BRIGHT_GREEN),
        2 => Some(DARK_GREEN),
        _ => None,
    }
}

// SPEC-020 §2.1: the automap window's layout and dispatch. Rectangles are
// frame (screen) pixels.

/// A window rectangle on screen.
pub fn on_screen(rect: Rect) -> Rect {
    rect.offset(WINDOW)
}
/// The quit confirmation's screen position.
pub const QUIT_ORIGIN: [i32; 2] = [169, 117];
/// A control of the automap window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomapControl {
    /// The detail switch: toggles on its press (`SWITCH_SOUND`).
    Switch,
    /// The Scanner red button: sends `s` on its release.
    Scanner,
    /// The Cancel red button: sends Esc on its release.
    Cancel,
}
/// The window's controls on screen, in press order.
pub fn layout() -> Layout<AutomapControl> {
    Layout::new()
        .with(AutomapControl::Switch, on_screen(SWITCH))
        .with(AutomapControl::Scanner, on_screen(SCANNER))
        .with(AutomapControl::Cancel, on_screen(CANCEL))
}
/// What an automap input leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomapOutcome {
    None,
    /// A red button is held (`ib1p1xx1`).
    Pressed(AutomapControl),
    /// Set the saved detail flag: high or low.
    Detail(bool),
    /// `s` or Scanner: with the scanner off, the Motion Sensor test (a
    /// held sensor with charges spends one and turns the scanner on; else
    /// `iisxxxx1` and the refusal at `SCANNER_DIALOG`).
    Scanner,
    Close,
    /// Ctrl+Q, Alt+X or F10: the quit confirmation, whose Yes closes it.
    QuitConfirm,
    Screenshot,
}
/// A key in the automap window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomapKey {
    /// Tab, Esc or `a`.
    Close,
    /// `h` and `l`.
    High,
    Low,
    /// `s`.
    Scanner,
    Quit,
    F12,
    Other,
}
/// §3.33 the open window: the scanner state (cleared at every opening
/// unless a Motion Sensor opened it) and the held red button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutomapWindow {
    pub scanner: bool,
    armed: Armed<AutomapControl>,
}
impl AutomapWindow {
    pub fn open(scanner: bool) -> Self {
        Self {
            scanner,
            armed: Armed::new(),
        }
    }
    pub fn layout(&self) -> Layout<AutomapControl> {
        layout()
    }
    /// The held red button.
    pub fn pressed(&self) -> Option<AutomapControl> {
        self.armed.pressed()
    }
    /// A press: the switch toggles the detail at once (`high` is the
    /// current setting); a red button is held.
    pub fn press(&mut self, point: [i32; 2], high: bool) -> AutomapOutcome {
        match self.layout().hit(point) {
            Some(AutomapControl::Switch) => AutomapOutcome::Detail(!high),
            Some(control) => {
                self.armed.press(Some(control));
                AutomapOutcome::Pressed(control)
            }
            None => AutomapOutcome::None,
        }
    }
    /// A release over the held red button sends its key (`ib1lu1x1` plays
    /// whenever a held button is released).
    pub fn release(&mut self, point: Option<[i32; 2]>) -> AutomapOutcome {
        let over = point.and_then(|point| self.layout().hit(point));
        match self.armed.release(over) {
            Some(AutomapControl::Scanner) => self.key(AutomapKey::Scanner),
            Some(AutomapControl::Cancel) => AutomapOutcome::Close,
            _ => AutomapOutcome::None,
        }
    }
    pub fn key(&mut self, key: AutomapKey) -> AutomapOutcome {
        match key {
            AutomapKey::Close => AutomapOutcome::Close,
            AutomapKey::High => AutomapOutcome::Detail(true),
            AutomapKey::Low => AutomapOutcome::Detail(false),
            AutomapKey::Scanner if self.scanner => AutomapOutcome::None,
            AutomapKey::Scanner => AutomapOutcome::Scanner,
            AutomapKey::Quit => AutomapOutcome::QuitConfirm,
            AutomapKey::F12 => AutomapOutcome::Screenshot,
            AutomapKey::Other => AutomapOutcome::None,
        }
    }
    /// The Motion Sensor test passed.
    pub fn scanner_on(&mut self) {
        self.scanner = true;
    }
}
