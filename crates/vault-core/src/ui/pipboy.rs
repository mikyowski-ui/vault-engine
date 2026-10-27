//! SPEC-014 §3.41 the Pip-Boy 2000 screen: its art, window controls and
//! sounds, the date, time and vault-water note, the screen area's lines,
//! alignments and row zones, the shared word wrapper, the Status page's
//! columns, the holodisk reading controls, the Alarm Clock options, the
//! holidays, the `pipboy.msg` IDs and the screen saver. Coordinates are
//! window coordinates, which equal screen coordinates (the window is at
//! (0, 0), 640 by 480).
use super::{ColorKey, Layout, Rect};
use crate::host_rest::RestRequest;

/// All art is under `art/intrface/`; there is no `pipboy.frm`.
pub const BACKGROUND: &str = "pip.frm";
pub const NUMBERS: &str = "numbers.frm";
pub const NOTE: &str = "pip2.frm";
pub const MONTHS: &str = "months.frm";
pub const NOTE_DIGITS: &str = "notenums.frm";
pub const ALARM_UP: &str = "alarmout.frm";
pub const ALARM_DOWN: &str = "alarmin.frm";
pub const LOGO: &str = "pipx.frm";
pub const BOMB: &str = "bomb1.frm";
/// The eleven art files opening loads; any failure keeps the Pip-Boy shut.
pub const ART: [&str; 11] = [
    BACKGROUND,
    super::RED_BUTTON.0,
    super::RED_BUTTON.1,
    NUMBERS,
    NOTE,
    MONTHS,
    NOTE_DIGITS,
    ALARM_UP,
    ALARM_DOWN,
    LOGO,
    BOMB,
];
pub const MESSAGES: &str = "pipboy.msg";
/// Requested on opening; the corpus has no `pipon.acm` (SPEC-010 §3.9).
pub const OPEN_SOUND: &str = "pipon";
pub const ALARM_PRESS: &str = "ib2p1xx1";
pub const ALARM_RELEASE: &str = "ib2lu1x1";
/// The rest refusal: a large dialog box at screen (192, 135), key 32328.
pub const REST_DIALOG: [i32; 2] = [192, 135];

/// `pipboy.msg` IDs (§3.41.9).
pub mod msg {
    pub const MORE: i32 = 200;
    pub const BACK: i32 = 201;
    pub const STATUS: i32 = 202;
    pub const NO_TASKS: i32 = 203;
    pub const AUTOMAPS: i32 = 205;
    pub const ARCHIVES: i32 = 206;
    pub const STATUS_SUFFIX: i32 = 210;
    pub const DATA: i32 = 211;
    pub const OF: i32 = 212;
    pub const DONE: i32 = 214;
    pub const CANNOT_REST: i32 = 215;
    pub const ALARM_CLOCK: i32 = 300;
    pub const HIT_POINTS: i32 = 301;
}

/// The window controls. The page buttons, labels included, are part of
/// `pip.frm`; the scratched-out second label at (53, 368) has no button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Status,
    Automaps,
    Archives,
    Close,
    Alarm,
}
impl Button {
    pub const ALL: [Self; 5] = [
        Self::Status,
        Self::Automaps,
        Self::Archives,
        Self::Close,
        Self::Alarm,
    ];
    /// The whole rectangle is clickable; no mask is used.
    pub const fn rect(self) -> Rect {
        match self {
            Self::Status => Rect::new(53, 341, 15, 16),
            Self::Automaps => Rect::new(53, 395, 15, 16),
            Self::Archives => Rect::new(53, 422, 15, 16),
            Self::Close => Rect::new(53, 449, 15, 16),
            Self::Alarm => Rect::new(124, 13, 28, 24),
        }
    }
    /// Up and down art, both drawn transparently.
    pub const fn art(self) -> (&'static str, &'static str) {
        match self {
            Self::Alarm => (ALARM_UP, ALARM_DOWN),
            _ => (super::RED_BUTTON.0, super::RED_BUTTON.1),
        }
    }
    /// Press and release sounds.
    pub const fn sounds(self) -> (&'static str, &'static str) {
        match self {
            Self::Alarm => (ALARM_PRESS, ALARM_RELEASE),
            _ => (super::BUTTON_PRESS, super::BUTTON_RELEASE),
        }
    }
    pub fn at(point: [i32; 2]) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|button| button.rect().contains(point))
    }
}

pub use crate::host_save::{CALENDAR_DAY, CALENDAR_YEAR};
/// Month, day and year of a game tick (tick 0 is 5 December 2161).
pub fn date(ticks: u64) -> [u16; 3] {
    crate::host_save::game_date(ticks)
}

/// A copy of `source` from an art file to window `at`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub source: Rect,
    pub at: [i32; 2],
}
/// `numbers.frm` white-band digit d: the 9-by-17 cell at source x = 9d.
fn number_cell(digit: u32, at: [i32; 2]) -> Cell {
    Cell {
        source: Rect::new(9 * digit as i32, 0, 9, 17),
        at,
    }
}
/// `digits` decimal places of `value` with leading zeros, 9 pixels apart.
fn number_cells(value: u32, digits: u32, [x, y]: [i32; 2]) -> impl Iterator<Item = Cell> {
    (0..digits).map(move |place| {
        let digit = value / 10_u32.pow(digits - 1 - place) % 10;
        number_cell(digit, [x + 9 * place as i32, y])
    })
}
/// The date and time, copied opaquely: the day at (20, 17), the month's
/// 29-by-14 `months.frm` block (source rows `15(m - 1)` on) at (46, 18), the
/// year at (83, 17) and the 24-hour time at (155, 17), with leading zeros.
pub fn clock_cells(ticks: u64) -> (Vec<Cell>, Cell) {
    let [month, day, year] = date(ticks);
    let time = (ticks / 36000 % 24 * 100 + ticks / 600 % 60) as u32;
    let digits = number_cells(u32::from(day), 2, [20, 17])
        .chain(number_cells(u32::from(year), 4, [83, 17]))
        .chain(number_cells(time, 4, [155, 17]))
        .collect();
    let month = Cell {
        source: Rect::new(0, 15 * (i32::from(month) - 1), 29, 14),
        at: [46, 18],
    };
    (digits, month)
}

/// The vault-water note: campaign global 101 is the water chip, 10 the
/// vault-water days.
pub const WATER_CHIP_GLOBAL: usize = 101;
pub const WATER_DAYS_GLOBAL: usize = 10;
pub const NOTE_RECT: Rect = Rect::new(32, 83, 148, 227);
/// The note shows unless the chip is 2 or no days remain. Safe rule: a
/// negative day count (whose digits would come from outside the art) hides
/// it too.
pub fn note_shown(chip: i32, days: i32) -> bool {
    chip != 2 && days > 0
}
/// The days left in `notenums.frm` cells (12 by 13 at source x = 12d),
/// drawn transparently from the ones digit up: the ones digit at (92, 180),
/// each next one 12 pixels left (6 after a digit 1) and 2 lower.
pub fn note_digits(days: i32) -> Vec<Cell> {
    let mut cells = Vec::new();
    let (mut value, mut x, mut y) = (days.max(0), 92, 180);
    loop {
        let digit = value % 10;
        cells.push(Cell {
            source: Rect::new(12 * digit, 0, 12, 13),
            at: [x, y],
        });
        value /= 10;
        if value == 0 {
            return cells;
        }
        x -= if digit == 1 { 6 } else { 12 };
        y += 2;
    }
}

/// The start screen: `pipx.frm` transparently at (323, 156), and on a
/// holiday its greeting centered on x = 431 at y = 315.
pub const LOGO_AT: [i32; 2] = [323, 156];
pub const HOLIDAY_Y: i32 = 315;
pub const HOLIDAYS: [([u16; 2], i32); 8] = [
    ([1, 1], 100),
    ([2, 14], 101),
    ([4, 1], 102),
    ([7, 4], 104),
    ([10, 6], 103),
    ([10, 31], 105),
    ([11, 28], 106),
    ([12, 25], 107),
];
pub fn holiday(month: u16, day: u16) -> Option<i32> {
    HOLIDAYS
        .iter()
        .find(|(date, _)| *date == [month, day])
        .map(|(_, id)| *id)
}
pub fn holiday_x(width: i32) -> i32 {
    431 - width / 2
}

/// The page text area, restored from `pip.frm` before a page draws.
pub const SCREEN: Rect = Rect::new(254, 46, 374, 410);
pub const LINE_HEIGHT: i32 = 10;
/// Line 40, at y = 446, is the bottom line; later lines overprint it.
pub const LAST_LINE: i32 = 40;
pub fn line_y(line: i32) -> i32 {
    SCREEN.y + LINE_HEIGHT * line.clamp(0, LAST_LINE)
}
/// Text is drawn with a 640-pixel limit, so it is not clipped at x = 627.
pub const TEXT_LIMIT: i32 = 640;
/// Clicks left of x = 429 are the left column, right of it the right.
pub const DIVIDER_X: i32 = 429;
pub const TEXT: ColorKey = ColorKey::GREEN;
pub const SELECTED: ColorKey = ColorKey::SELECTED;
pub const COMPLETED: ColorKey = ColorKey(8804);
/// A struck-through line is 1 pixel at the text origin y + 3.
pub const STRIKE_OFFSET: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Normal,
    /// Holodisk text.
    NoIndent,
    Centered,
    RightColumn,
    /// Ends at x = 364.
    LeftHeader,
    /// Ends at x = 522.
    RightHeader,
}
/// Where text of `width` starts.
pub fn text_x(align: Align, width: i32) -> i32 {
    match align {
        Align::Normal => 262,
        Align::NoIndent => 255,
        Align::Centered => 254 + (350 - width) / 2,
        Align::RightColumn => 437,
        Align::LeftHeader => 364 - width,
        Align::RightHeader => 522 - width,
    }
}

/// Row zones: invisible 350-by-10 rectangles at x = 254, one line tall.
pub const ZONE_X: i32 = 254;
pub const ZONE_WIDTH: i32 = 350;
pub const BACK_ZONE: Rect = Rect::new(254, 446, 350, 10);
fn zone_rect(line: i32) -> Rect {
    Rect::new(ZONE_X, line_y(line), ZONE_WIDTH, LINE_HEIGHT)
}
/// A page's zones from first line `first`, on every other line, as
/// (rectangle, row): zone k reports row `first + k - 1` and exists only
/// while `first + k` is below 20. Safe rule: only created zones exist.
pub fn zones(first: i32, count: usize) -> Vec<(Rect, i32)> {
    (0..count as i32)
        .take_while(|k| first + k < 20)
        .map(|k| (zone_rect(first + 2 * k), first + k - 1))
        .collect()
}
/// The row a point is on, if any.
pub fn zone_row(first: i32, count: usize, point: [i32; 2]) -> Option<i32> {
    zones(first, count)
        .into_iter()
        .find(|(rect, _)| rect.contains(point))
        .map(|(_, row)| row)
}

/// The shared word wrapper at width 350.
pub const WRAP_WIDTH: i32 = 350;
pub const WRAP_LINES: usize = 63;
fn breaks_after(code: u8) -> bool {
    matches!(code, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r' | b'-')
}
/// Wrap `text` to `width` with per-character advances (width plus letter
/// spacing; a space the font's space width). A string narrower than the
/// width is one line. Otherwise a line ends just after the last space-class
/// character or hyphen that fit, or, with none, before the character that
/// overflows. `None` is a failure: more than 63 lines.
pub fn wrap(text: &[u8], width: i32, advance: impl Fn(u8) -> i32) -> Option<Vec<Vec<u8>>> {
    if text.iter().map(|code| advance(*code)).sum::<i32>() < width {
        return Some(vec![text.to_vec()]);
    }
    let mut lines = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let (mut sum, mut fitted, mut end) = (0, None, text.len());
        for (index, code) in text.iter().enumerate().skip(start) {
            sum += advance(*code);
            if sum > width {
                // Safe bound: a line always takes at least one character.
                end = fitted.map_or(index.max(start + 1), |at| at + 1);
                break;
            }
            if breaks_after(*code) {
                fitted = Some(index);
            }
        }
        if lines.len() == WRAP_LINES {
            return None;
        }
        lines.push(text[start..end].to_vec());
        start = end;
    }
    Some(lines)
}

/// What a Status-list release does: the r-th location (left of x = 429)
/// or the r-th holodisk (right of it); x = 429 exactly does nothing.
/// Safe rule: a left click below the last location is ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusClick {
    Location(usize),
    Disk(usize),
}
pub fn status_click(x: i32, row: i32, locations: usize, disks: usize) -> Option<StatusClick> {
    let row = usize::try_from(row).ok().filter(|row| *row >= 1)?;
    match x {
        _ if x < DIVIDER_X && row <= locations => Some(StatusClick::Location(row)),
        _ if x > DIVIDER_X && row <= disks => Some(StatusClick::Disk(row)),
        _ => None,
    }
}
/// The Status list asks for one zone per location, one per holodisk and
/// one more; at most 18 exist (from line 2).
pub fn status_zone_count(locations: usize, disks: usize) -> usize {
    locations + disks + 1
}

/// The holodisk reading view: records from line 3 at x = 255, the page
/// counter at y = 48 from x = 580 minus the width of "of", and line 40's
/// BACK and MORE or DONE.
pub const RECORD_LINE: i32 = 3;
pub const COUNTER_Y: i32 = 48;
pub fn counter_x(of_width: i32) -> i32 {
    580 - of_width
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadingInput {
    /// A release on line 40 at this x.
    Release(i32),
    PageDown,
    PageUp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadingAction {
    Next,
    Previous,
    List,
}
/// The reading view's controls on page `page` of a disk whose last page
/// is `last_page`: x ≥ 460 or PageDown goes on (a release on the last page
/// returns to the list; PageDown there does nothing), x ≤ 395 or PageUp
/// goes back (to the list from page 0), and x 396-459 does nothing.
pub fn reading_action(input: ReadingInput, page: usize, last_page: usize) -> Option<ReadingAction> {
    let back = || {
        Some(if page == 0 {
            ReadingAction::List
        } else {
            ReadingAction::Previous
        })
    };
    let last = page >= last_page;
    match input {
        ReadingInput::Release(x) if x >= 460 && last => Some(ReadingAction::List),
        ReadingInput::Release(x) if x >= 460 => Some(ReadingAction::Next),
        ReadingInput::PageDown if last => None,
        ReadingInput::PageDown => Some(ReadingAction::Next),
        ReadingInput::Release(x) if x <= 395 => back(),
        ReadingInput::PageUp => back(),
        ReadingInput::Release(_) => None,
    }
}

/// The Automaps elevation view: zones from line 0 (the title reports row
/// -1, the description row 0, entry r row r), and the raster from (238,
/// 105) (section 3.33).
pub const ENTRY_LINE: i32 = 4;

/// The Alarm Clock: the Hit Points strip at y = 66 and the options from
/// line 5 on every other line; option j reports row j + 4.
pub const HP_STRIP: Rect = Rect::new(254, 66, 350, 10);
pub const OPTION_LINE: i32 = 5;
pub const ALARM_OPTIONS: [(i32, RestRequest); 13] = [
    (302, RestRequest::Minutes(10)),
    (303, RestRequest::Minutes(30)),
    (304, RestRequest::Hours(1)),
    (305, RestRequest::Hours(2)),
    (306, RestRequest::Hours(3)),
    (307, RestRequest::Hours(4)),
    (308, RestRequest::Hours(5)),
    (309, RestRequest::Hours(6)),
    (310, RestRequest::UntilHour(6)),
    (311, RestRequest::UntilHour(12)),
    (312, RestRequest::UntilHour(18)),
    (313, RestRequest::UntilHour(0)),
    (314, RestRequest::UntilHealed),
];
/// The option a row reports.
pub fn alarm_option(row: i32) -> Option<usize> {
    usize::try_from(row - 4)
        .ok()
        .filter(|option| *option < ALARM_OPTIONS.len())
}
/// An "until" option's rest, `(hours, minutes)`, from 24-hour `h:m`:
/// d = (W - h) mod 24; with m = 0, d hours (24 when d = 0), else (d - 1)
/// mod 24 hours and 60 - m minutes, ending at the next W:00 after now.
pub fn until(wake: i32, hour: i32, minute: i32) -> (i32, i32) {
    let d = (wake - hour).rem_euclid(24);
    match minute {
        0 if d == 0 => (24, 0),
        0 => (d, 0),
        _ => ((d - 1).rem_euclid(24), 60 - minute),
    }
}

/// The screen saver starts after more than 120 seconds without input or
/// pointer movement; once warmed up each pass lasts at least 50 ms.
pub const IDLE_MS: u64 = 120_000;
pub const SAVER_PASS_MS: u64 = 50;
pub const SAVER_WARM_UP: u32 = 50;
pub const BOMB_SLOTS: usize = 16;
/// A draw below this (0 through 3047) spawns a bomb in a free slot.
pub const SPAWN_BELOW: f64 = 3047.3311;
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bomb {
    pub x: i32,
    pub y: i32,
    pub speed: f32,
    pub progress: f32,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenSaver {
    pub bombs: [Option<Bomb>; BOMB_SLOTS],
    pub warm_up: u32,
}
impl Default for ScreenSaver {
    fn default() -> Self {
        Self {
            bombs: [None; BOMB_SLOTS],
            warm_up: SAVER_WARM_UP,
        }
    }
}
impl ScreenSaver {
    /// One pass after the input check. `draw` is one game random draw in 0
    /// through 32767 (SPEC-006 §3.4). Returns the `bomb1.frm` cells this
    /// pass draws, or `None` during the warm-up (nothing drawn, no wait).
    pub fn pass(&mut self, mut draw: impl FnMut() -> i32) -> Option<Vec<Cell>> {
        if f64::from(draw()) < SPAWN_BELOW
            && let Some(slot) = self.bombs.iter_mut().find(|slot| slot.is_none())
        {
            // 724 = (350 - 16) + (406 - 16): the right edge, then the top.
            let position = (f64::from(draw()) / 32767.0 * 724.0) as i32;
            let (x, y) = if position <= 394 {
                (602, position + 48)
            } else {
                (position - 124, -17)
            };
            let speed = (f64::from(draw()) * 2.75 / 32767.0 + 0.15) as f32;
            *slot = Some(Bomb {
                x,
                y,
                speed,
                progress: 0.0,
            });
        }
        let cells = (self.warm_up == 0).then(|| self.clip());
        for bomb in self.bombs.iter_mut().flatten() {
            bomb.progress += bomb.speed;
            if bomb.progress >= 1.0 {
                bomb.x = (bomb.x as f32 - bomb.progress) as i32;
                bomb.y = (bomb.y as f32 + bomb.progress) as i32;
                bomb.progress = 0.0;
            }
        }
        self.warm_up = self.warm_up.saturating_sub(1);
        cells
    }
    /// Clip each active bomb to x 254-603 and y 46-451, in slot order;
    /// one that leaves the area frees its slot.
    fn clip(&mut self) -> Vec<Cell> {
        let mut cells = Vec::new();
        for slot in &mut self.bombs {
            let Some(bomb) = *slot else {
                continue;
            };
            let (sx, x, w) = if bomb.x >= 254 {
                let w = if bomb.x + 65 >= 604 { 604 - bomb.x } else { 65 };
                (0, bomb.x, w)
            } else {
                let skip = 254 - bomb.x;
                (skip, 254, 65 - skip)
            };
            let (sy, y, h) = if bomb.y >= 46 {
                let h = if bomb.y + 65 >= 452 { 452 - bomb.y } else { 65 };
                (0, bomb.y, h)
            } else {
                let skip = 46 - bomb.y;
                (skip, 46, 65 - skip)
            };
            // Above the area the slot is freed only when `y + 65 < 46`.
            let freed = w < 1 || (bomb.y >= 46 && h < 1) || bomb.y + 65 < 46;
            if freed {
                *slot = None;
            } else if h > 0 {
                cells.push(Cell {
                    source: Rect::new(sx, sy, w, h),
                    at: [x, y],
                });
            }
        }
        cells
    }
}

// SPEC-020 §2.1: the Pip-Boy's layout and page state machine. Rectangles
// are frame (screen) pixels; the window is at (0, 0).

/// An accepted row shows yellow this long before it acts.
pub const FLASH_MS: u64 = 200;

/// The pages, by the button that shows them (`z` opens on the Alarm Clock).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Status,
    Automaps,
    Archives,
    Alarm,
}
/// The Status page's views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusView {
    List,
    /// A location's quests, by the presentation layer's location key.
    Detail(usize),
    /// A holodisk being read: its key, the page shown and the last page.
    Reading {
        disk: usize,
        page: usize,
        last_page: usize,
    },
}
/// The Automaps page's views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomapView {
    List,
    /// An area's elevation list: the area's index in the area list, and
    /// the selected entry (from 0).
    Elevations {
        area: usize,
        entry: usize,
    },
}
/// The page shown (none on the start screen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Status(StatusView),
    Automaps(AutomapView),
    Archives,
    Alarm,
}
impl Page {
    pub fn tab(&self) -> Tab {
        match self {
            Self::Status(_) => Tab::Status,
            Self::Automaps(_) => Tab::Automaps,
            Self::Archives => Tab::Archives,
            Self::Alarm => Tab::Alarm,
        }
    }
}
/// A row zone, or the Back zone on line 40.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Row(i32),
    Back,
}
/// What shows yellow while a flash waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Highlight {
    None,
    Location(usize),
    Disk(usize),
    Back,
}
/// What a flash does when it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deferred {
    Detail(usize),
    Reading { disk: usize, last_page: usize },
    StatusList,
    Page(ReadingAction),
}
/// A selected row's yellow flash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flash {
    pub until: u64,
    pub highlight: Highlight,
    pub then: Deferred,
}
/// The list lengths the current page's zones depend on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PageCounts {
    pub locations: usize,
    pub disks: usize,
    pub areas: usize,
    /// The shown area's elevation entries.
    pub entries: usize,
    pub archives: usize,
}
/// A control of the Pip-Boy window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipBoyControl {
    Button(Button),
    Zone(Zone),
}
/// A key in the Pip-Boy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipBoyKey {
    /// Esc (it also stops a rest).
    Escape,
    /// Enter or `p`.
    Close,
    /// Ctrl+Q, Ctrl+X or F10.
    Quit,
    F12,
    PageDown,
    PageUp,
    Other,
}
/// What a Pip-Boy input leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipBoyOutcome {
    /// Nothing changed (the input is still swallowed).
    None,
    /// The window changed (a page shown, the screen saver ended).
    Redraw,
    /// A window button is held: play its `Button::sounds`.
    Pressed(Button),
    /// Close the Pip-Boy (the map-update scripts run).
    Close,
    /// The quit confirmation; the Pip-Boy closes after it, whatever the
    /// answer.
    QuitConfirm,
    Screenshot,
    /// The Alarm Clock: test the SPEC-008 §3.9 rest eligibility, then call
    /// [`PipBoyModal::show_alarm`] or show the refusal at `REST_DIALOG`.
    AlarmRequested,
    /// The r-th location (from 1) was chosen: flash it, then its detail
    /// ([`PipBoyModal::flash`] with `Highlight::Location(r)` and
    /// `Deferred::Detail(key)`).
    Location(usize),
    /// The r-th holodisk (from 1) was chosen: read it, then flash it with
    /// `Highlight::Disk(r)` and `Deferred::Reading`.
    Disk(usize),
    /// A flash began (`ib1p1xx1`).
    Flashing,
    /// An automap area or entry was selected (`ib1p1xx1`).
    Selected,
    /// Play archive row r (from 1), after `ib1p1xx1`.
    Archive(i32),
    /// Rest with Alarm Clock option j; on success call
    /// [`PipBoyModal::begin_rest`].
    Rest(usize),
    /// Esc during a rest: it stops.
    StopRest,
}
/// The screen saver's run: its bombs, the cells drawn last and the last
/// pass's time.
#[derive(Debug, Clone, PartialEq)]
pub struct SaverRun {
    pub state: ScreenSaver,
    pub cells: Vec<Cell>,
    pub last_pass: u64,
}
/// §3.41 the Pip-Boy's modal state: the page, the held button or zone, a
/// row's flash, a rest's option, the idle timer and the screen saver.
#[derive(Debug, Clone, PartialEq)]
pub struct PipBoyModal {
    /// No page is current on the start screen.
    pub page: Option<Page>,
    pub flash: Option<Flash>,
    /// The Alarm Clock option of the running rest.
    pub resting: Option<usize>,
    pressed: Option<Button>,
    zone: Option<Zone>,
    pub idle_since: u64,
    /// The quit confirmation closes the Pip-Boy whatever the answer.
    pub close_after_dialog: bool,
    pub saver: Option<SaverRun>,
    last_step: u64,
}
impl PipBoyModal {
    /// Opened at `now` on the start screen.
    pub fn open(now: u64) -> Self {
        Self {
            page: None,
            flash: None,
            resting: None,
            pressed: None,
            zone: None,
            idle_since: now,
            close_after_dialog: false,
            saver: None,
            last_step: 0,
        }
    }
    /// The held window button (drawn with its down art).
    pub fn pressed(&self) -> Option<Button> {
        self.pressed
    }
    /// The current page's zones: (first line, count, Back zone).
    pub fn zone_layout(&self, counts: &PageCounts) -> (i32, usize, bool) {
        match &self.page {
            None => (0, 0, false),
            Some(Page::Status(StatusView::List)) => {
                (2, status_zone_count(counts.locations, counts.disks), false)
            }
            Some(Page::Status(_)) => (0, 0, true),
            Some(Page::Automaps(AutomapView::List)) => (2, counts.areas, false),
            Some(Page::Automaps(AutomapView::Elevations { .. })) => (0, counts.entries + 2, true),
            Some(Page::Archives) => (2, counts.archives, false),
            Some(Page::Alarm) => (OPTION_LINE, ALARM_OPTIONS.len(), false),
        }
    }
    /// The zone under `point` on the current page.
    pub fn zone_at(&self, point: [i32; 2], counts: &PageCounts) -> Option<Zone> {
        let (first, count, back) = self.zone_layout(counts);
        if back && BACK_ZONE.contains(point) {
            return Some(Zone::Back);
        }
        zone_row(first, count, point).map(Zone::Row)
    }
    /// The window buttons, then the current page's Back zone and row
    /// zones.
    pub fn layout(&self, counts: &PageCounts) -> Layout<PipBoyControl> {
        let (first, count, back) = self.zone_layout(counts);
        let mut layout: Layout<PipBoyControl> = Button::ALL
            .into_iter()
            .map(|button| (PipBoyControl::Button(button), button.rect()))
            .collect();
        if back {
            layout.push(PipBoyControl::Zone(Zone::Back), BACK_ZONE);
        }
        layout.extend(
            zones(first, count)
                .into_iter()
                .map(|(rect, row)| (PipBoyControl::Zone(Zone::Row(row)), rect)),
        );
        layout
    }
    /// Any input or pointer movement restarts the idle timer; one that
    /// ends the screen saver is consumed (`true`).
    pub fn input(&mut self, now: u64) -> bool {
        self.idle_since = now;
        self.saver.take().is_some()
    }
    /// A left press: a window button is held; a row zone waits for its
    /// release. Nothing is taken during a rest or a flash.
    pub fn press(&mut self, point: [i32; 2], counts: &PageCounts, now: u64) -> PipBoyOutcome {
        if self.input(now) {
            return PipBoyOutcome::Redraw;
        }
        if self.resting.is_some() || self.flash.is_some() {
            return PipBoyOutcome::None;
        }
        if let Some(button) = Button::at(point) {
            self.pressed = Some(button);
            return PipBoyOutcome::Pressed(button);
        }
        self.zone = self.zone_at(point, counts);
        PipBoyOutcome::None
    }
    /// A left release: over the held button it acts; over the held zone
    /// its row acts.
    pub fn release(
        &mut self,
        point: Option<[i32; 2]>,
        counts: &PageCounts,
        now: u64,
    ) -> PipBoyOutcome {
        let zone = point.and_then(|point| self.zone_at(point, counts));
        let (button, pressed_zone) = (self.pressed.take(), self.zone.take());
        let Some(point) = point else {
            return PipBoyOutcome::None;
        };
        if let Some(button) = button {
            if !button.rect().contains(point) {
                return PipBoyOutcome::None;
            }
            return match button {
                Button::Status => self.show_tab(Tab::Status),
                Button::Automaps => self.show_tab(Tab::Automaps),
                Button::Archives => self.show_tab(Tab::Archives),
                Button::Close => PipBoyOutcome::Close,
                Button::Alarm => self.show_tab(Tab::Alarm),
            };
        }
        match (pressed_zone, zone) {
            (Some(pressed), Some(zone)) if pressed == zone => {
                self.zone_release(zone, point[0], counts, now)
            }
            _ => PipBoyOutcome::None,
        }
    }
    /// A page button: its page from the initial state (also when it is
    /// already shown). The Alarm Clock needs the eligibility test first.
    pub fn show_tab(&mut self, tab: Tab) -> PipBoyOutcome {
        self.page = Some(match tab {
            Tab::Status => Page::Status(StatusView::List),
            Tab::Automaps => Page::Automaps(AutomapView::List),
            Tab::Archives => Page::Archives,
            Tab::Alarm => return PipBoyOutcome::AlarmRequested,
        });
        PipBoyOutcome::Redraw
    }
    /// Resting is allowed: the Alarm Clock page.
    pub fn show_alarm(&mut self) {
        self.page = Some(Page::Alarm);
    }
    /// A released row zone on the current page.
    pub fn zone_release(
        &mut self,
        zone: Zone,
        x: i32,
        counts: &PageCounts,
        now: u64,
    ) -> PipBoyOutcome {
        match (self.page, zone) {
            (Some(Page::Status(StatusView::List)), Zone::Row(row)) => {
                match status_click(x, row, counts.locations, counts.disks) {
                    Some(StatusClick::Location(row)) => PipBoyOutcome::Location(row),
                    Some(StatusClick::Disk(row)) => PipBoyOutcome::Disk(row),
                    None => PipBoyOutcome::None,
                }
            }
            (Some(Page::Status(StatusView::Detail(_))), Zone::Back) => {
                self.flash(now, Highlight::Back, Deferred::StatusList)
            }
            (Some(Page::Status(StatusView::Reading { .. })), Zone::Back) => {
                self.reading_input(ReadingInput::Release(x), now)
            }
            (Some(Page::Automaps(AutomapView::List)), Zone::Row(row)) => {
                match usize::try_from(row - 1)
                    .ok()
                    .filter(|area| *area < counts.areas)
                {
                    Some(area) => {
                        self.page =
                            Some(Page::Automaps(AutomapView::Elevations { area, entry: 0 }));
                        PipBoyOutcome::Selected
                    }
                    None => PipBoyOutcome::None,
                }
            }
            (Some(Page::Automaps(AutomapView::Elevations { area, .. })), zone) => match zone {
                Zone::Back | Zone::Row(-1) => {
                    self.page = Some(Page::Automaps(AutomapView::List));
                    PipBoyOutcome::Selected
                }
                Zone::Row(row) if row >= 1 => {
                    let entry = row as usize - 1;
                    self.page = Some(Page::Automaps(AutomapView::Elevations { area, entry }));
                    PipBoyOutcome::Selected
                }
                Zone::Row(_) => PipBoyOutcome::None,
            },
            (Some(Page::Archives), Zone::Row(row)) if row >= 1 => PipBoyOutcome::Archive(row),
            (Some(Page::Alarm), Zone::Row(row)) => {
                alarm_option(row).map_or(PipBoyOutcome::None, PipBoyOutcome::Rest)
            }
            _ => PipBoyOutcome::None,
        }
    }
    /// The reading view's line-40 controls and page keys; elsewhere page
    /// keys do nothing.
    pub fn reading_input(&mut self, input: ReadingInput, now: u64) -> PipBoyOutcome {
        let Some(Page::Status(StatusView::Reading {
            page, last_page, ..
        })) = self.page
        else {
            return PipBoyOutcome::None;
        };
        match reading_action(input, page, last_page) {
            Some(action) => self.flash(now, Highlight::None, Deferred::Page(action)),
            None => PipBoyOutcome::None,
        }
    }
    /// Show `highlight` for `FLASH_MS`, then do `then`.
    pub fn flash(&mut self, now: u64, highlight: Highlight, then: Deferred) -> PipBoyOutcome {
        self.flash = Some(Flash {
            until: now + FLASH_MS,
            highlight,
            then,
        });
        PipBoyOutcome::Flashing
    }
    /// A flash's deferred step.
    pub fn apply(&mut self, then: Deferred) {
        let view = match then {
            Deferred::Detail(key) => StatusView::Detail(key),
            Deferred::Reading { disk, last_page } => StatusView::Reading {
                disk,
                page: 0,
                last_page,
            },
            Deferred::StatusList | Deferred::Page(ReadingAction::List) => StatusView::List,
            Deferred::Page(action) => {
                if let Some(Page::Status(StatusView::Reading { page, .. })) = &mut self.page {
                    *page = match action {
                        ReadingAction::Next => *page + 1,
                        _ => page.saturating_sub(1),
                    };
                }
                return;
            }
        };
        self.page = Some(Page::Status(view));
    }
    /// A key. One that ends the screen saver is consumed; during a rest
    /// only Esc acts (it stops the rest); a flash swallows keys.
    pub fn key(&mut self, key: PipBoyKey, now: u64) -> PipBoyOutcome {
        if self.input(now) {
            return PipBoyOutcome::Redraw;
        }
        if self.resting.is_some() {
            return if key == PipBoyKey::Escape {
                PipBoyOutcome::StopRest
            } else {
                PipBoyOutcome::None
            };
        }
        if self.flash.is_some() {
            return PipBoyOutcome::None;
        }
        match key {
            PipBoyKey::Escape | PipBoyKey::Close => PipBoyOutcome::Close,
            PipBoyKey::Quit => {
                self.close_after_dialog = true;
                PipBoyOutcome::QuitConfirm
            }
            PipBoyKey::F12 => PipBoyOutcome::Screenshot,
            PipBoyKey::PageDown => self.reading_input(ReadingInput::PageDown, now),
            PipBoyKey::PageUp => self.reading_input(ReadingInput::PageUp, now),
            PipBoyKey::Other => PipBoyOutcome::None,
        }
    }
    /// One loop pass: a due flash ends and its step applies (`true`).
    pub fn tick(&mut self, now: u64) -> bool {
        match self.flash.take_if(|flash| now >= flash.until) {
            Some(flash) => {
                self.apply(flash.then);
                true
            }
            None => false,
        }
    }
    /// A rest began with Alarm Clock option `option`.
    pub fn begin_rest(&mut self, option: usize) {
        self.resting = Some(option);
        self.last_step = 0;
    }
    /// The rest ended.
    pub fn end_rest(&mut self) {
        self.resting = None;
    }
    /// Whether a rest step is due (at least `SAVER_PASS_MS` apart); a due
    /// step is taken.
    pub fn rest_due(&mut self, now: u64) -> bool {
        if self.resting.is_none() || now.saturating_sub(self.last_step) < SAVER_PASS_MS {
            return false;
        }
        self.last_step = now;
        true
    }
    /// Whether a screen-saver pass is due: the next pass of a running
    /// saver, or the start after more than `IDLE_MS` idle with no dialog
    /// or flash up.
    pub fn saver_due(&self, now: u64, dialog: bool) -> bool {
        match &self.saver {
            Some(saver) => now.saturating_sub(saver.last_pass) >= SAVER_PASS_MS,
            None => {
                now.saturating_sub(self.idle_since) > IDLE_MS && !dialog && self.flash.is_none()
            }
        }
    }
    /// One screen-saver pass with game random draws in 0-32767; starting
    /// runs the warm-up passes, which draw nothing, first.
    pub fn saver_pass(&mut self, now: u64, mut draw: impl FnMut() -> i32) {
        let saver = self.saver.get_or_insert_with(|| SaverRun {
            state: ScreenSaver::default(),
            cells: Vec::new(),
            last_pass: now,
        });
        loop {
            if let Some(cells) = saver.state.pass(&mut draw) {
                saver.cells = cells;
                saver.last_pass = now;
                return;
            }
        }
    }
}

#[cfg(test)]
#[path = "pipboy_tests.rs"]
mod tests;
