//! SPEC-014 game flow and user interface rules, independent of windowing:
//! layouts, state machines, timings and text rules that a presentation layer
//! draws and drives. Coordinates are window-relative pixels unless named
//! "screen"; durations are milliseconds.
pub mod automap;
pub mod bar;
pub mod cursor;
pub mod editor;
pub mod endgame;
pub mod flow;
pub mod inventory;
pub mod options;
pub mod saveload;

use vault_formats::Palette;

/// One modal UI frame lasts at least `trunc(1000 / 24)` milliseconds.
pub const FRAME_MS: u64 = 1000 / 24;
/// Red-button press and release sounds.
pub const BUTTON_PRESS: &str = "ib1p1xx1";
pub const BUTTON_RELEASE: &str = "ib1lu1x1";
/// The refusal / invalid-input sound.
pub const ERROR_SOUND: &str = "iisxxxx1";
/// Little red button art and size.
pub const RED_BUTTON: (&str, &str, [i32; 2]) = ("lilredup.frm", "lilreddn.frm", [15, 16]);

/// An RGB555 color key, `1024 r + 32 g + b` with 5-bit channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ColorKey(pub u16);
impl ColorKey {
    pub const LABEL: Self = Self(18979);
    pub const LABEL_PRESSED: Self = Self(14723);
    pub const GREEN: Self = Self(992);
    pub const SELECTED: Self = Self(32747);
    pub const ERROR: Self = Self(32328);
    pub const AMBER: Self = Self(21091);
    pub const GREY_GREEN: Self = Self(21204);
    pub const WHITE: Self = Self(32767);
    pub const GREY: Self = Self(21140);
    pub const DARK_GREEN: Self = Self(1313);
    pub const DARK_OLIVE: Self = Self(15845);
    pub const BLACK: Self = Self(0);
    pub const RED: Self = Self(31744);
    pub fn rgb5(self) -> [u8; 3] {
        let key = self.0;
        [
            (key >> 10 & 31) as u8,
            (key >> 5 & 31) as u8,
            (key & 31) as u8,
        ]
    }
    /// The palette index the color.pal RGB555 table returns for this key.
    pub fn resolve(self, palette: &Palette) -> u8 {
        palette.lookup_rgb5(self.rgb5()).unwrap_or(0)
    }
}

/// A rectangle `(x, y) w by h`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}
impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(&self, point: [i32; 2]) -> bool {
        (self.x..self.x + self.w).contains(&point[0])
            && (self.y..self.y + self.h).contains(&point[1])
    }
}

/// Font number `100 + n` selects `font<n>.aaf`.
pub const FONT_SMALL: i32 = 101;
pub const FONT_TITLE: i32 = 102;
pub const FONT_CAPTION: i32 = 103;
pub const FONT_LARGE: i32 = 104;
pub const FONT_PLAIN: i32 = 100;

/// Horizontal origin that centers text of `width` in `box_width`.
pub fn centered(box_width: i32, width: i32) -> i32 {
    (box_width - width) / 2
}

#[cfg(test)]
mod tests;
