//! SPEC-014 game flow and user interface rules, independent of windowing:
//! layouts, state machines, timings and text rules that a presentation layer
//! draws and drives. Coordinates are window-relative pixels unless named
//! "screen"; durations are milliseconds.
pub mod automap;
pub mod bar;
pub mod called;
pub mod character;
pub mod cursor;
pub mod editor;
pub mod endgame;
pub mod flow;
pub mod inventory;
pub mod options;
pub mod pipboy;
pub mod saveload;
pub mod sheet;
pub mod talk;

use vault_formats::Palette;

/// One modal UI frame lasts at least `trunc(1000 / 24)` milliseconds.
pub const FRAME_MS: u64 = 1000 / 24;

/// A held button's repeat (SPEC-014 §3.9 SPECIAL plus and minus from the
/// 20th frame, §3.10 the age arrows from the 15th, §3.14 the save/load
/// arrows after 13 idle frames): the press steps once at once; after the
/// idle frames of `FRAME_MS` the steps come at 5 per second, rising by 1
/// per step to 24 per second, each step waiting `trunc(1000 / rate)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoldRepeat {
    next: u64,
    steps: u32,
}
impl HoldRepeat {
    /// A button pressed at `pressed_ms`, repeating after `idle_frames`.
    pub fn new(pressed_ms: u64, idle_frames: u32) -> Self {
        Self {
            next: pressed_ms + u64::from(idle_frames) * FRAME_MS,
            steps: 0,
        }
    }
    /// Whether a repeated step is due at `now`; a due step schedules the
    /// next one. At most one step per poll, as one per loop pass.
    pub fn poll(&mut self, now: u64) -> bool {
        if now < self.next {
            return false;
        }
        // The step happens now, then waits for the rate's delay.
        self.next = now + editor::repeat_delay(self.steps);
        self.steps += 1;
        true
    }
}

/// SPEC-014 §3.17 the grey lookup table the pause window converts the view
/// through, built from the `color.pal` palette (not the brightness-adjusted
/// or cycled display): for each entry, s is the largest plus the smallest of
/// its five-bit components (an unmapped entry counts as zero), `v =
/// trunc(s × 240 / 510)` (0-29), and the grey index is the palette's RGB555
/// lookup of (v, v, v), key `1057 v`. Unmapped entries therefore map to the
/// lookup of key 0 (index 228 in the corpus).
pub fn grey_table(palette: &Palette) -> [u8; 256] {
    std::array::from_fn(|index| {
        let index = index as u8;
        let components = if palette.is_mapped(index) {
            palette.rgb6(index).map(|level| u16::from(level / 2))
        } else {
            [0; 3]
        };
        let largest = components.into_iter().max().unwrap_or(0);
        let smallest = components.into_iter().min().unwrap_or(0);
        let v = ((largest + smallest) * 240 / 510) as u8;
        // v is at most 29, always a valid five-bit level.
        palette.lookup_rgb5([v; 3]).unwrap_or(0)
    })
}
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
