//! SPEC-014 §3.45 the dialogue control panel's Review and Tell Me About
//! controls: the panel and its three buttons, the Review window (layout,
//! scrolling, the shared review wrapping and the drawing order of names and
//! texts), and the Ask About window (layout, input field, cursor blink and
//! entry rules). SPEC-006 §3.9 owns the review history and the Ask About
//! lookup (`host_talk`); SPEC-013 §3.18 owns barter. Coordinates are
//! window-relative unless named "screen".
use super::{Armed, ColorKey, Layout, Rect};
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

// SPEC-020 §2.1: the dialogue panel's and its windows' layouts and
// dispatch. Rectangles are frame (screen) pixels.

/// A control of the dialogue control panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelControl {
    Barter,
    /// Fallout 1's Tell Me About.
    TellMeAbout,
    /// SPEC-019 §3.8 Fallout 2's Combat Control, in Tell Me About's place,
    /// for a party member with a control record.
    CombatControl,
    Review,
}
impl PanelControl {
    /// Press and release sounds: the small buttons' or Review's.
    pub fn sounds(self) -> (&'static str, &'static str) {
        match self {
            Self::Review => (super::BUTTON_PRESS, super::BUTTON_RELEASE),
            _ => (SMALL_PRESS, SMALL_RELEASE),
        }
    }
}
/// The panel's buttons on screen: Barter, then Tell Me About (Fallout 1)
/// or Combat Control (a Fallout 2 party member's; none for anyone else),
/// then Review.
pub fn panel_layout(fallout2: bool, control_member: bool) -> Layout<PanelControl> {
    let mut layout = Layout::new().with(PanelControl::Barter, panel_button(BARTER));
    if !fallout2 {
        layout.push(PanelControl::TellMeAbout, panel_button(TELL_ME_ABOUT));
    } else if control_member {
        layout.push(
            PanelControl::CombatControl,
            panel_button(super::fallout2::COMBAT_CONTROL_BUTTON),
        );
    }
    layout.with(PanelControl::Review, panel_button(REVIEW))
}
/// The panel's keys in the reply/choice loop: `a` asks about (Fallout 1
/// only), `b` barters, `c` opens Combat Control for a Fallout 2 party
/// member with a control record; no key opens Review.
pub fn panel_key(key: char, fallout2: bool, control_member: bool) -> Option<PanelControl> {
    match key.to_ascii_lowercase() {
        'a' if !fallout2 => Some(PanelControl::TellMeAbout),
        'c' if control_member => Some(PanelControl::CombatControl),
        'b' => Some(PanelControl::Barter),
        _ => None,
    }
}
/// The panel's controls, held from press to release: a release over the
/// held button acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panel {
    pub fallout2: bool,
    pub control_member: bool,
    armed: Armed<PanelControl>,
}
impl Panel {
    pub fn new(fallout2: bool, control_member: bool) -> Self {
        Self {
            fallout2,
            control_member,
            armed: Armed::new(),
        }
    }
    pub fn layout(&self) -> Layout<PanelControl> {
        panel_layout(self.fallout2, self.control_member)
    }
    pub fn pressed(&self) -> Option<PanelControl> {
        self.armed.pressed()
    }
    pub fn press(&mut self, point: [i32; 2]) -> Option<PanelControl> {
        self.armed.press(self.layout().hit(point))
    }
    pub fn release(&mut self, point: Option<[i32; 2]>) -> Option<PanelControl> {
        let over = point.and_then(|point| self.layout().hit(point));
        self.armed.release(over)
    }
}

/// A control of the Review window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewControl {
    ScrollUp,
    ScrollDown,
    Done,
}
impl ReviewControl {
    /// The arrows use the small buttons' sounds, Done the red button's.
    pub fn sounds(self) -> (&'static str, &'static str) {
        match self {
            Self::Done => (super::BUTTON_PRESS, super::BUTTON_RELEASE),
            _ => (SMALL_PRESS, SMALL_RELEASE),
        }
    }
}
/// The Review window's buttons (the window is at (0, 0)).
pub fn review_layout() -> Layout<ReviewControl> {
    Layout::new()
        .with(ReviewControl::ScrollUp, SCROLL_UP.0)
        .with(ReviewControl::ScrollDown, SCROLL_DOWN.0)
        .with(ReviewControl::Done, DONE.0)
}
/// What a Review input leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewOutcome {
    None,
    /// The top entry moved.
    Scrolled(usize),
    /// Esc or Done: the window closes (the head updater resumes).
    Close,
}
/// A key in the Review window: Esc, Up and Down act; every other key
/// (Ctrl+Q and F10 included) is ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewKey {
    Escape,
    Up,
    Down,
    Other,
}
/// §3.45 the Review window: the entry at the top of the transcript and the
/// held button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReviewWindow {
    pub top: usize,
    armed: Armed<ReviewControl>,
}
impl ReviewWindow {
    /// Opened with the oldest entry at the top.
    pub fn new() -> Self {
        Self::default()
    }
    pub fn layout(&self) -> Layout<ReviewControl> {
        review_layout()
    }
    pub fn pressed(&self) -> Option<ReviewControl> {
        self.armed.pressed()
    }
    /// A press holds the button under the pointer (every press is taken:
    /// the window is modal).
    pub fn press(&mut self, point: [i32; 2]) -> Option<ReviewControl> {
        self.armed.press(self.layout().hit(point))
    }
    /// A release over the held button acts, over `entries` history entries.
    pub fn release(&mut self, point: Option<[i32; 2]>, entries: usize) -> ReviewOutcome {
        let over = point.and_then(|point| self.layout().hit(point));
        match self.armed.release(over) {
            Some(control) => self.dispatch(control, entries),
            None => ReviewOutcome::None,
        }
    }
    pub fn key(&mut self, key: ReviewKey, entries: usize) -> ReviewOutcome {
        match key {
            ReviewKey::Escape => self.dispatch(ReviewControl::Done, entries),
            ReviewKey::Up => self.dispatch(ReviewControl::ScrollUp, entries),
            ReviewKey::Down => self.dispatch(ReviewControl::ScrollDown, entries),
            ReviewKey::Other => ReviewOutcome::None,
        }
    }
    /// Act: the arrows move the top by one whole entry (`scroll`), Done
    /// closes.
    pub fn dispatch(&mut self, control: ReviewControl, entries: usize) -> ReviewOutcome {
        let down = match control {
            ReviewControl::Done => return ReviewOutcome::Close,
            ReviewControl::ScrollUp => false,
            ReviewControl::ScrollDown => true,
        };
        match scroll(self.top, entries, down) {
            Some(top) => {
                self.top = top;
                ReviewOutcome::Scrolled(top)
            }
            None => ReviewOutcome::None,
        }
    }
}

/// The Ask About window's red buttons on screen: DONE sends Enter, CANCEL
/// Esc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskControl {
    Done,
    Cancel,
}
pub fn ask_layout() -> Layout<AskControl> {
    Layout::new()
        .with(
            AskControl::Done,
            super::red_button(ASK_DONE).offset(ASK_ORIGIN),
        )
        .with(
            AskControl::Cancel,
            super::red_button(ASK_CANCEL).offset(ASK_ORIGIN),
        )
}
/// What an Ask About input leads to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskOutcome {
    None,
    /// The field changed (redraw).
    Changed,
    /// Look this text up (the field cleared; the window stays open).
    Submit(Vec<u8>),
    /// Esc or CANCEL: speech stops and the window closes, restoring the
    /// saved reply.
    Cancel,
}
/// A key in the Ask About window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskKey {
    Enter,
    Escape,
    Backspace,
    /// A typed input code.
    Code(i32),
}
/// §3.45 the Ask About window: its field, whether it is still opening (the
/// reply and option windows are hidden and it opens on the next tick),
/// whether an answer shows, and the held button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskAboutWindow {
    pub field: AskField,
    pub opening: bool,
    pub answered: bool,
    armed: Armed<AskControl>,
}
impl AskAboutWindow {
    /// Tell Me About passed its tests at `now`.
    pub fn new(now: u64) -> Self {
        Self {
            field: AskField::new(now),
            opening: true,
            answered: false,
            armed: Armed::new(),
        }
    }
    pub fn layout(&self) -> Layout<AskControl> {
        ask_layout()
    }
    pub fn pressed(&self) -> Option<AskControl> {
        self.armed.pressed()
    }
    /// Which of the reply and option windows it hides: the reply until an
    /// answer shows, the options while it is open.
    pub fn hides(&self) -> (bool, bool) {
        (self.opening || !self.answered, true)
    }
    /// A press holds a red button (nothing while opening).
    pub fn press(&mut self, point: [i32; 2]) -> Option<AskControl> {
        if self.opening {
            return None;
        }
        self.armed.press(self.layout().hit(point))
    }
    /// A release over the held button acts.
    pub fn release(&mut self, point: Option<[i32; 2]>) -> AskOutcome {
        let over = point.and_then(|point| self.layout().hit(point));
        match self.armed.release(over) {
            Some(control) if !self.opening => self.dispatch(control),
            _ => AskOutcome::None,
        }
    }
    /// A key (nothing while opening); typed codes follow
    /// `AskField::type_code`.
    pub fn key(
        &mut self,
        key: AskKey,
        measure: impl Fn(&[u8]) -> i32,
        glyph: impl Fn(u8) -> i32,
    ) -> AskOutcome {
        if self.opening {
            return AskOutcome::None;
        }
        match key {
            AskKey::Enter => self.dispatch(AskControl::Done),
            AskKey::Escape => self.dispatch(AskControl::Cancel),
            AskKey::Backspace => {
                self.field.backspace();
                AskOutcome::Changed
            }
            AskKey::Code(code) => {
                if self.field.type_code(code, measure, glyph) {
                    AskOutcome::Changed
                } else {
                    AskOutcome::None
                }
            }
        }
    }
    /// DONE submits the typed text (an empty field submits nothing);
    /// CANCEL closes.
    pub fn dispatch(&mut self, control: AskControl) -> AskOutcome {
        match control {
            AskControl::Done => self
                .field
                .submit()
                .map_or(AskOutcome::None, AskOutcome::Submit),
            AskControl::Cancel => AskOutcome::Cancel,
        }
    }
    /// The lookup found an answer: the reply window shows again.
    pub fn answer_shown(&mut self) {
        self.answered = true;
    }
    /// An input poll at `now`: the window finishes opening, else the
    /// cursor blinks. Returns whether it redraws.
    pub fn tick(&mut self, now: u64) -> bool {
        if self.opening {
            self.opening = false;
            return true;
        }
        self.field.blink(now)
    }
}

#[cfg(test)]
#[path = "talk_tests.rs"]
mod tests;
