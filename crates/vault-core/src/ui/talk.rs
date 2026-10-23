//! SPEC-014 §3.45 the dialogue control panel's Review and Tell Me About
//! controls: the panel and its three buttons, the Review window (layout,
//! scrolling, the shared review wrapping and the drawing order of names and
//! texts), and the Ask About window (layout, input field, cursor blink and
//! entry rules). SPEC-006 §3.9 owns the review history and the Ask About
//! lookup (`host_talk`); SPEC-013 §3.18 owns barter. Coordinates are
//! window-relative unless named "screen".
use super::{ColorKey, Rect};
use crate::host_talk::ReviewText;

/// The control panel `di_talk.frm` (list index 99, 640 by 190) at screen
/// (0, 290); its art carries the REVIEW, BARTER and TELL ME ABOUT labels.
pub const PANEL: &str = "di_talk.frm";
pub const PANEL_ORIGIN: [i32; 2] = [0, 290];
/// The panel buttons, each acting on mouse release.
pub const BARTER: Rect = Rect::new(593, 41, 14, 14);
pub const TELL_ME_ABOUT: Rect = Rect::new(593, 116, 14, 14);
pub const REVIEW: Rect = Rect::new(13, 154, 51, 29);
/// Both 14-by-14 buttons show `di_rdbt2.frm` normally and `di_rdbt1.frm`
/// while pressed, drawn transparently; Review shows `di_rest1.frm` /
/// `di_rest2.frm`, drawn opaquely. Each pair is (normal, pressed).
pub const SMALL_ART: (&str, &str) = ("di_rdbt2.frm", "di_rdbt1.frm");
pub const REVIEW_ART: (&str, &str) = ("di_rest1.frm", "di_rest2.frm");
/// The 14-by-14 buttons' and the Review scroll buttons' sounds; Review and
/// Done use `ui::BUTTON_PRESS` / `ui::BUTTON_RELEASE`.
pub const SMALL_PRESS: &str = "ib2p1xx1";
pub const SMALL_RELEASE: &str = "ib2lu1x1";

/// A panel button's screen rectangle.
pub const fn panel_button(rect: Rect) -> Rect {
    Rect::new(
        rect.x + PANEL_ORIGIN[0],
        rect.y + PANEL_ORIGIN[1],
        rect.w,
        rect.h,
    )
}

/// The Review window: modal at screen (0, 0), 640 by 480, filled opaquely
/// with `review.frm` (list index 102); font 101.
pub const REVIEW_BACKGROUND: &str = "review.frm";
pub const REVIEW_FONT: i32 = super::FONT_SMALL;
/// Its buttons, drawn transparently, each acting on release: (rectangle,
/// (normal, pressed) art).
pub const SCROLL_UP: (Rect, (&str, &str)) = (
    Rect::new(475, 152, 35, 35),
    ("di_bgup1.frm", "di_bgup2.frm"),
);
pub const SCROLL_DOWN: (Rect, (&str, &str)) = (
    Rect::new(475, 191, 35, 37),
    ("di_bgdn1.frm", "di_bgdn2.frm"),
);
pub const DONE: (Rect, (&str, &str)) = (
    Rect::new(499, 398, 82, 46),
    ("di_done1.frm", "di_done2.frm"),
);

/// The transcript: names at x = 88 (drawing limit 180), texts wrapped in
/// x 113-422 (width 309) with each text's first line at x = 123, from
/// y = 76. Drawing stops after the first entry that leaves y at 407 or
/// more, and a line is drawn only while its top is at most 408.
pub const NAME_X: i32 = 88;
pub const NAME_LIMIT: i32 = 180;
pub const TEXT_LEFT: i32 = 113;
pub const TEXT_WIDTH: i32 = 309;
pub const FIRST_LINE_X: i32 = 123;
pub const TRANSCRIPT_TOP: i32 = 76;
pub const STOP_Y: i32 = 407;
pub const LAST_LINE_TOP: i32 = 408;
/// The target's name, the reply, the player's name and the option.
pub const TARGET_NAME_KEY: ColorKey = ColorKey(992);
pub const REPLY_KEY: ColorKey = ColorKey(768);
pub const PLAYER_NAME_KEY: ColorKey = ColorKey(21140);
pub const OPTION_KEY: ColorKey = ColorKey(15855);

/// SPEC-014 §3.45 review wrapping (also the dialogue reply area's): the
/// lines of `text` for width `width`, under `measure`.
///
/// 1. A rest at most `width` wide is one line.
/// 2. Otherwise the line runs to the first space after its first
///    character, then extends to each later space while the text up to it
///    stays narrower than `width`; the space at the break is dropped.
/// 3. A rest with no space after its first character is one line, which
///    the font's drawing limit cuts at `width`; the text ends there.
/// 4. A chosen line still wider than `width` (a first word wider than it)
///    stops the text. The reference then leaves the stored message cut at
///    that word for the session; `[May-improve]`: the text is never
///    modified (these are borrowed slices).
pub fn wrap(text: &[u8], width: i32, measure: impl Fn(&[u8]) -> i32) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        if measure(rest) <= width {
            lines.push(rest);
            break;
        }
        let space = |from: usize| {
            rest.iter()
                .skip(from)
                .position(|byte| *byte == b' ')
                .map(|at| at + from)
        };
        let Some(mut end) = space(1) else {
            lines.push(rest);
            break;
        };
        while let Some(next) = space(end + 1) {
            if measure(&rest[..next]) >= width {
                break;
            }
            end = next;
        }
        if measure(&rest[..end]) > width {
            break;
        }
        lines.push(&rest[..end]);
        rest = &rest[end + 1..];
    }
    lines
}

/// One text line of the Review transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewLine {
    pub text: Vec<u8>,
    pub origin: [i32; 2],
    pub key: ColorKey,
    /// The font's drawing limit in pixels.
    pub limit: i32,
}

/// The Review transcript from entry `top` on. Each entry draws the
/// target's name and `:`; the text area's top then moves down by the line
/// step `s` = text height + 2 (12 for font 101), and the reply wraps from
/// there, its lines `height` apart. A chosen option adds the player's name
/// and `:` and its wrapped text the same way. Names obey the same 408 top
/// limit as text lines, so a name can be left at the bottom with no text.
pub fn review_lines(
    entries: &[ReviewText],
    top: usize,
    target: &[u8],
    player: &[u8],
    height: i32,
    measure: impl Fn(&[u8]) -> i32,
) -> Vec<ReviewLine> {
    let step = height + 2;
    let mut lines = Vec::new();
    let mut y = TRANSCRIPT_TOP;
    let body = |text: &[u8], top: i32, key: ColorKey, lines: &mut Vec<ReviewLine>| {
        let mut y = top;
        for (index, line) in wrap(text, TEXT_WIDTH, &measure).into_iter().enumerate() {
            if y > LAST_LINE_TOP {
                break;
            }
            lines.push(ReviewLine {
                text: line.to_vec(),
                origin: [if index == 0 { FIRST_LINE_X } else { TEXT_LEFT }, y],
                key,
                limit: TEXT_WIDTH,
            });
            y += height;
        }
        y
    };
    let name = |name: &[u8], y: i32, key: ColorKey, lines: &mut Vec<ReviewLine>| {
        if y <= LAST_LINE_TOP {
            lines.push(ReviewLine {
                text: [name, b":"].concat(),
                origin: [NAME_X, y],
                key,
                limit: NAME_LIMIT,
            });
        }
    };
    for entry in entries.iter().skip(top) {
        name(target, y, TARGET_NAME_KEY, &mut lines);
        y = body(&entry.reply, y + step, REPLY_KEY, &mut lines);
        if let Some(choice) = &entry.choice {
            name(player, y, PLAYER_NAME_KEY, &mut lines);
            y = body(choice, y + step, OPTION_KEY, &mut lines);
        }
        if y >= STOP_Y {
            break;
        }
    }
    lines
}

/// Up or Down in the Review window: the new top entry, or `None` when the
/// press cannot move (no redraw). Up stops at entry 0; Down stops at the
/// last entry, which can then be the only one shown.
pub fn scroll(top: usize, entries: usize, down: bool) -> Option<usize> {
    if down {
        (top + 1 < entries).then_some(top + 1)
    } else {
        top.checked_sub(1)
    }
}

/// The Ask About window: modal at screen `(trunc((640 - 290) / 2), 356)`,
/// 290 by 85, `lsgbox.frm` (list index 238) copied opaquely.
pub const ASK_BACKGROUND: &str = "lsgbox.frm";
pub const ASK_SIZE: [i32; 2] = [290, 85];
pub const ASK_ORIGIN: [i32; 2] = [(640 - ASK_SIZE[0]) / 2, 356];
/// Texts in font 103, key 18979: `misc.msg` 6000 "Ask about what?"
/// centered at y = 7, `dbox.msg` 100 DONE and 103 CANCEL.
pub const ASK_FONT: i32 = super::FONT_CAPTION;
pub const ASK_KEY: ColorKey = ColorKey::LABEL;
pub const ASK_TITLE: (&str, i32) = ("misc.msg", 6000);
pub const ASK_TITLE_Y: i32 = 7;
pub const ASK_DONE_LABEL: (i32, [i32; 2]) = (100, [56, 57]);
pub const ASK_CANCEL_LABEL: (i32, [i32; 2]) = (103, [181, 57]);
/// The red buttons, 15 by 16 (`lilredup.frm` / `lilreddn.frm`, drawn
/// transparently, Red-button sounds): DONE sends Enter, CANCEL Esc.
pub const ASK_DONE: [i32; 2] = [34, 58];
pub const ASK_CANCEL: [i32; 2] = [160, 58];
/// The input field: filled with key 0 at each redraw, then the typed text
/// and the cursor character in font 101, key 992, limit 244.
pub const FIELD: Rect = Rect::new(22, 32, 244, 14);
pub const FIELD_FILL: ColorKey = ColorKey::BLACK;
pub const FIELD_FONT: i32 = super::FONT_SMALL;
pub const FIELD_KEY: ColorKey = ColorKey::GREEN;
/// The cursor toggles on an input poll more than 333 ms after the last
/// toggle.
pub const BLINK_MS: u64 = 333;
/// A code is appended only while fewer than 126 characters are typed.
pub const TYPED_LIMIT: usize = 126;

/// The Ask About input field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskField {
    pub text: Vec<u8>,
    /// The cursor shows `_` (else a space).
    pub cursor: bool,
    toggled: u64,
}
impl AskField {
    /// An empty field opened at `now`, its cursor `_`.
    pub fn new(now: u64) -> Self {
        Self {
            text: Vec::new(),
            cursor: true,
            toggled: now,
        }
    }
    /// The typed text followed by the cursor character.
    pub fn shown(&self) -> Vec<u8> {
        let mut shown = self.text.clone();
        shown.push(if self.cursor { b'_' } else { b' ' });
        shown
    }
    /// An input poll at `now`: past 333 ms since the last toggle the
    /// cursor alternates and the field redraws (`true`).
    pub fn blink(&mut self, now: u64) -> bool {
        if now.saturating_sub(self.toggled) <= BLINK_MS {
            return false;
        }
        self.cursor = !self.cursor;
        self.toggled = now;
        true
    }
    /// Backspace removes the last character.
    pub fn backspace(&mut self) {
        self.text.pop();
    }
    /// Enter or DONE: the text to look up, and the field clears; an empty
    /// field submits nothing.
    pub fn submit(&mut self) -> Option<Vec<u8>> {
        (!self.text.is_empty()).then(|| std::mem::take(&mut self.text))
    }
    /// Append input code `code` when fewer than 126 characters are typed
    /// and `width(text + "_") + glyph(code)` (the glyph width without
    /// spacing) is below 244. The reference stores any code of 0 or more
    /// as one byte, so extended keys enter characters (Up arrow 328 enters
    /// `H`); `[May-improve]`: only codes 32 through 255 are accepted.
    /// Returns whether the code was appended.
    pub fn type_code(
        &mut self,
        code: i32,
        measure: impl Fn(&[u8]) -> i32,
        glyph: impl Fn(u8) -> i32,
    ) -> bool {
        let Ok(byte) = u8::try_from(code) else {
            return false;
        };
        if byte < 32 || self.text.len() >= TYPED_LIMIT {
            return false;
        }
        let typed = measure(&[self.text.as_slice(), b"_"].concat());
        if typed + glyph(byte) >= FIELD.w {
            return false;
        }
        self.text.push(byte);
        true
    }
}

#[cfg(test)]
#[path = "talk_tests.rs"]
mod tests;
