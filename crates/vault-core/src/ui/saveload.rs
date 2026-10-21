//! SPEC-014 §3.14-3.16: the save/load slot screen (captions, slot states,
//! navigation, clicks, previews, the thumbnail scaler, the info box), the
//! description box, and quick save/load.
use super::{ColorKey, Rect};

pub const SLOTS: usize = 10;
pub const BACKGROUND: &str = "lsgame.frm";
pub const COVER: (&str, [i32; 2]) = ("lscover.frm", [340, 39]);
pub const TITLE_ORIGIN: [i32; 2] = [48, 27];
pub const LIST_AREA: Rect = Rect::new(55, 87, 230, 353);
pub const PREVIEW_ORIGIN: [i32; 2] = [366, 58];
pub const INFO_BOX: Rect = Rect::new(396, 254, 164, 60);
/// Done and Cancel red buttons and their captions (`lsgame.msg` 104, 105).
pub const DONE: ([i32; 2], [i32; 2]) = ([391, 349], [410, 348]);
pub const CANCEL: ([i32; 2], [i32; 2]) = ([495, 349], [515, 348]);
pub const ARROW_UP: Rect = Rect::new(35, 58, 11, 14);
pub const ARROW_DOWN: Rect = Rect::new(35, 72, 11, 14);

/// The five modes sharing the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Save,
    PickQuickSave,
    LoadInGame,
    LoadFromMenu,
    PickQuickLoad,
}
impl Mode {
    /// `lsgame.msg` title.
    pub fn title(self) -> i32 {
        match self {
            Self::Save => 102,
            Self::PickQuickSave => 103,
            Self::LoadInGame | Self::LoadFromMenu => 100,
            Self::PickQuickLoad => 101,
        }
    }
    /// `lsgame.msg` row caption (SAVE GAME or LOAD GAME).
    pub fn caption(self) -> i32 {
        if self.saving() { 109 } else { 110 }
    }
    pub fn saving(self) -> bool {
        matches!(self, Self::Save | Self::PickQuickSave)
    }
}

/// A slot's state, from its `SAVE.DAT` header.
///
/// §3.14 `[May-improve]`: the reference's "old version" state (`lsgame.msg`
/// 113, 116 and 136) is unreachable in R1, because the header reader reports
/// only short reads and bad signatures; it is omitted here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotState {
    Occupied(SlotInfo),
    /// No `SAVE.DAT`.
    Empty,
    /// A short header or a bad signature.
    Corrupt,
}
/// The header fields the screen shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SlotInfo {
    pub name: Vec<u8>,
    pub description: Vec<u8>,
    /// Month, day, year as stored.
    pub game_date: [u16; 3],
    pub game_ticks: u32,
    pub map: u16,
    pub elevation: u16,
}

/// Caption line for zero-based slot `i`: `[`, three spaces, the caption, a
/// space, the two-digit slot number, `:`, three spaces, `]`.
pub fn slot_caption(caption: &[u8], slot: usize) -> Vec<u8> {
    let mut text = b"[   ".to_vec();
    text.extend_from_slice(caption);
    text.extend(format!(" {:02}:   ]", slot + 1).bytes());
    text
}
pub fn slot_origin(slot: usize) -> [i32; 2] {
    [55, 87 + 34 * slot as i32]
}
pub fn second_line_origin(slot: usize) -> [i32; 2] {
    [55, 97 + 34 * slot as i32]
}
/// Second line text and whether it uses the error color, with
/// `lsgame.msg` texts supplied by `text`.
pub fn second_line(state: &SlotState, text: impl Fn(i32) -> Vec<u8>) -> (Vec<u8>, bool) {
    match state {
        SlotState::Occupied(info) => (info.description.clone(), false),
        SlotState::Empty => ([b"       ".to_vec(), text(111)].concat(), false),
        SlotState::Corrupt => (text(112), true),
    }
}
pub fn caption_color(selected: bool) -> ColorKey {
    if selected {
        ColorKey::SELECTED
    } else {
        ColorKey::GREEN
    }
}

/// What the preview area shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preview {
    /// The slot's stored thumbnail.
    Stored,
    /// The freshly captured thumbnail of the current game.
    Current,
    /// `lscover.frm`.
    Cover,
}
/// Save modes show the stored thumbnail or the current capture; load modes
/// the stored thumbnail or the cover. Safe rule: a failed thumbnail read
/// shows the cover (load) or the current capture (save).
pub fn preview(mode: Mode, state: &SlotState, thumbnail_ok: bool) -> Preview {
    let occupied = matches!(state, SlotState::Occupied(_)) && thumbnail_ok;
    match (mode.saving(), occupied) {
        (_, true) => Preview::Stored,
        (true, false) => Preview::Current,
        (false, false) => Preview::Cover,
    }
}

/// The selection and double-click state; the selection persists for the
/// program run (slot 1 at start).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlotList {
    pub selected: usize,
    remembered: Option<usize>,
    window: u32,
}
/// Frames a click stays remembered for a double click.
pub const DOUBLE_CLICK_FRAMES: u32 = 24;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKey {
    Up,
    Down,
    Home,
    End,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    Select(usize),
    /// A click on the remembered slot acts as Done.
    Done(usize),
}
impl SlotList {
    /// Up/Down move by one without wrap; Home/End select slots 1 and 10.
    /// Keys forget the remembered slot.
    pub fn key(&mut self, key: ListKey) {
        self.selected = match key {
            ListKey::Up => self.selected.saturating_sub(1),
            ListKey::Down => (self.selected + 1).min(SLOTS - 1),
            ListKey::Home => 0,
            ListKey::End => SLOTS - 1,
        };
        self.remembered = None;
    }
    /// A list click at screen y. Safe rules: rows from y = 87, and a fresh
    /// 24-frame double-click window on each click.
    pub fn click(&mut self, y: i32) -> Click {
        let slot = slot_at(y);
        self.selected = slot;
        if self.remembered == Some(slot) {
            self.remembered = None;
            return Click::Done(slot);
        }
        self.remembered = Some(slot);
        self.window = DOUBLE_CLICK_FRAMES;
        Click::Select(slot)
    }
    /// One modal frame: the remembered slot expires.
    pub fn frame(&mut self) {
        self.window = self.window.saturating_sub(1);
        if self.window == 0 {
            self.remembered = None;
        }
    }
}
/// Safe rule `trunc((y - 87) / 34)`, clamped to 0-9.
pub fn slot_at(y: i32) -> usize {
    ((y - 87).max(0) / 34).min(SLOTS as i32 - 1) as usize
}
/// The reference mapping `trunc((y - 79) / 34)`, clamped to 0-9 (P-08:
/// kept for the tests that pin the difference from the safe rule).
#[cfg(test)]
pub fn reference_slot_at(y: i32) -> usize {
    ((y - 79).max(0) / 34).min(SLOTS as i32 - 1) as usize
}
/// Held arrows: after this many idle frames, steps every frame.
pub const ARROW_IDLE_FRAMES: u32 = 13;

/// §3.14 thumbnail capture: the top 640 by 380 pixels scaled to 224 by 133
/// with ratio `R = 22937`. Source column x covers destination columns
/// `trunc(R x / 65536)` up to `trunc(R (x + 1) / 65536)`, rows likewise.
/// Safe rule: the never-written column 223 and row 132 repeat their nearest
/// written neighbour.
pub const THUMBNAIL: [usize; 2] = [224, 133];
pub const SCALE: u64 = 22937;
pub fn thumbnail(view: &[u8], stride: usize) -> Vec<u8> {
    let [width, height] = THUMBNAIL;
    let mut out = vec![0; width * height];
    let span = |i: u64| (SCALE * i / 65536) as usize..(SCALE * (i + 1) / 65536) as usize;
    for sy in 0..380 {
        for dy in span(sy as u64) {
            for sx in 0..640 {
                let pixel = view.get(sy * stride + sx).copied().unwrap_or(0);
                for dx in span(sx as u64) {
                    out[dy * width + dx] = pixel;
                }
            }
        }
    }
    for row in 0..height - 1 {
        out[row * width + width - 1] = out[row * width + width - 2];
    }
    out.copy_within(
        (height - 2) * width..(height - 1) * width,
        (height - 1) * width,
    );
    out
}
/// The source column or row a destination column or row takes (P-08: the
/// tests' statement of the scaler's mapping).
#[cfg(test)]
pub fn thumbnail_source(destination: usize) -> Option<usize> {
    (0..640).rev().find(|x| {
        let start = (SCALE * *x as u64 / 65536) as usize;
        let end = (SCALE * (*x as u64 + 1) / 65536) as usize;
        (start..end).contains(&destination)
    })
}

/// Info box line 2: `DD MON YYYY   HHMM`, the month name from
/// `lsgame.msg` 116 + month.
pub fn date_line(info: &SlotInfo, month_name: &[u8]) -> Vec<u8> {
    let [_, day, year] = info.game_date;
    let hours = info.game_ticks / 36000 % 24;
    let minutes = info.game_ticks / 600 % 60;
    let mut line = format!("{day:02} ").into_bytes();
    line.extend_from_slice(month_name);
    line.extend(format!(" {year:04}   {hours:02}{minutes:02}").bytes());
    line
}
pub const INFO_LINES: [[i32; 2]; 3] = [[396, 254], [397, 266], [399, 279]];
/// Info line 3 (the location) wraps to the 164-pixel box with a 12-pixel
/// line step.
pub const LOCATION_WIDTH: i32 = 164;
pub const LOCATION_STEP: i32 = 12;
/// §3.14 info line 3: the location text word-wrapped to 164 pixels (breaks
/// at spaces; a word wider than a line stands alone and is clipped when
/// drawn). The reference shows only the first wrapped line, because its
/// wrapper ends the text at the next line start; `[May-improve]`: up to
/// three wrapped lines, at y = 279, 291 and 303.
pub fn location_lines(text: &[u8], width: impl Fn(&[u8]) -> i32) -> Vec<Vec<u8>> {
    let mut lines: Vec<Vec<u8>> = Vec::new();
    for word in text.split(|b| *b == b' ').filter(|word| !word.is_empty()) {
        match lines.last_mut() {
            Some(line) if width(&[line.as_slice(), b" ", word].concat()) <= LOCATION_WIDTH => {
                line.push(b' ');
                line.extend_from_slice(word);
            }
            _ => lines.push(word.to_vec()),
        }
    }
    lines.truncate(3);
    lines
}

/// The location short-name table: `map.msg` index by map number (66 maps).
pub const MAP_SHORT_NAMES: [i32; 66] = {
    let mut table = [0; 66];
    let mut map = 0;
    while map < 66 {
        table[map] = match map {
            0..=2 | 22 | 23 | 51..=54 | 56 | 57 | 60 | 62 => 100,
            3..=5 | 9 => 505,
            6 | 35 => 500,
            7 | 8 => 501,
            10..=12 => 503,
            13..=15 | 55 => 507,
            16 => 116,
            17 | 18 | 33 | 34 | 47 => 511,
            19 => 119,
            20 | 21 => 120,
            24 => 504,
            25 | 26 => 502,
            27 | 42 | 43 => 509,
            28 | 29 | 44..=46 => 510,
            30..=32 | 48 => 508,
            36..=41 | 65 => 506,
            49 | 50 | 58 | 59 | 61 | 63 => 149,
            _ => 160,
        };
        map += 1;
    }
    table
};
/// `map.msg` indexes of the short name and the elevation name; safe rule:
/// `None` for an out-of-range map or elevation.
pub fn location_messages(map: u16, elevation: u16) -> Option<(i32, i32)> {
    if elevation > 2 {
        return None;
    }
    let short = *MAP_SHORT_NAMES.get(usize::from(map))?;
    Some((short, 200 + 3 * i32::from(map) + i32::from(elevation)))
}

/// §3.15 description box: a 29-character field, codes 32-122. A Backspace
/// or Delete as the first key clears a prefilled description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Description {
    pub text: Vec<u8>,
    first_key: bool,
}
pub const DESCRIPTION_LIMIT: usize = 29;
pub const DESCRIPTION_BOX: (&str, [i32; 2]) = ("lsgbox.frm", [169, 116]);
impl Description {
    pub fn open(prefill: &[u8]) -> Self {
        Self {
            text: prefill[..prefill.len().min(DESCRIPTION_LIMIT)].to_vec(),
            first_key: true,
        }
    }
    pub fn key(&mut self, code: u8) {
        self.first_key = false;
        if (32..=122).contains(&code) && self.text.len() < DESCRIPTION_LIMIT {
            self.text.push(code);
        }
    }
    pub fn backspace(&mut self) {
        if std::mem::take(&mut self.first_key) {
            self.text.clear();
        } else {
            self.text.pop();
        }
    }
}

/// §3.16 quick save/load: one session flag for "a quick slot is known".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QuickSlot {
    known: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quick {
    /// Run the pick screen in this mode.
    Pick(Mode),
    /// Save to the selected slot directly, keeping its description.
    SaveDirect,
    /// Load the selected slot directly under a black window.
    LoadDirect,
}
impl QuickSlot {
    pub fn f6(&self) -> Quick {
        if self.known {
            Quick::SaveDirect
        } else {
            Quick::Pick(Mode::PickQuickSave)
        }
    }
    pub fn f7(&self) -> Quick {
        if self.known {
            Quick::LoadDirect
        } else {
            Quick::Pick(Mode::PickQuickLoad)
        }
    }
    /// A normal Save or Load screen opened.
    pub fn forget(&mut self) {
        self.known = false;
    }
    /// A pick-quick-slot screen completed successfully.
    pub fn picked(&mut self) {
        self.known = true;
    }
}
