//! SPEC-014 §3.14-3.16: the save/load slot screen (captions, slot states,
//! navigation, clicks, previews, the thumbnail scaler, the info box), the
//! description box, and quick save/load.
use super::{ColorKey, Layout, Rect};

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
/// §3.14 `[May-improve]`: under Fallout 1 the reference's "old version"
/// state (`lsgame.msg` 113, 116 and 136) is unreachable in R1, because the
/// header reader reports only short reads and bad signatures. SPEC-019
/// §3.11 reaches it under Fallout 2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotState {
    Occupied(SlotInfo),
    /// No `SAVE.DAT`.
    Empty,
    /// A short header or a bad signature.
    Corrupt,
    /// SPEC-019 §3.11, Fallout 2 only: a full header with a good signature
    /// that fails SPEC-018 §7.2's other checks (a Fallout 1 slot, another
    /// layout tag or campaign count, or a gated save once the gate opens).
    OldVersion,
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
        SlotState::OldVersion => (text(113), true),
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

// SPEC-020 §2.1: the save/load screen's layout and dispatch. Rectangles are
// frame (screen) pixels.

/// §3.15 the description box's Done and Cancel red buttons (screen).
pub const DESCRIPTION_DONE: Rect = super::red_button([169 + 34, 116 + 58]);
pub const DESCRIPTION_CANCEL: Rect = super::red_button([169 + 160, 116 + 58]);
/// §3.15 the overwrite question (a large Yes/No box, `lsgame.msg` 131, key
/// 32328) and the §3.14 notices (a large Done box, key 32328).
pub const OVERWRITE_ORIGIN: [i32; 2] = [169, 131];
pub const NOTICE_ORIGIN: [i32; 2] = [169, 116];

/// A control of the save/load screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveLoadControl {
    Done,
    Cancel,
    ArrowUp,
    ArrowDown,
    /// The slot list area.
    List,
    DescriptionDone,
    DescriptionCancel,
}
/// The screen's controls: with the description box open only its Done and
/// Cancel; otherwise Done, Cancel, the arrows and the list.
pub fn layout(description: bool) -> Layout<SaveLoadControl> {
    if description {
        return Layout::new()
            .with(SaveLoadControl::DescriptionDone, DESCRIPTION_DONE)
            .with(SaveLoadControl::DescriptionCancel, DESCRIPTION_CANCEL);
    }
    Layout::new()
        .with(SaveLoadControl::Done, super::red_button(DONE.0))
        .with(SaveLoadControl::Cancel, super::red_button(CANCEL.0))
        .with(SaveLoadControl::ArrowUp, ARROW_UP)
        .with(SaveLoadControl::ArrowDown, ARROW_DOWN)
        .with(SaveLoadControl::List, LIST_AREA)
}
/// What a save/load input leads to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveLoadOutcome {
    /// Nothing changed (the input is still swallowed).
    None,
    /// The selection or the description text changed.
    Changed,
    /// Saving over an occupied slot: ask first (a Yes/No box at
    /// `OVERWRITE_ORIGIN`); Yes calls [`SaveLoadScreen::overwrite_accepted`].
    AskOverwrite(usize),
    /// The description box opened.
    DescriptionOpened,
    /// `iisxxxx1` and the `lsgame.msg` notice at `NOTICE_ORIGIN`.
    Notice { title: i32, body: i32 },
    /// Load this zero-based slot.
    Load(usize),
    /// Write this zero-based slot with the description.
    Save { slot: usize, description: Vec<u8> },
    /// Esc or Cancel: back to the options menu when it opened the screen,
    /// else to the map.
    Cancel { to_options: bool },
    /// A brightness hotkey step.
    Brightness(bool),
    /// Ctrl+Q, Ctrl+X or F10: the quit confirmation, whose Yes closes the
    /// screen as cancelled.
    QuitConfirm,
}
/// A key on the save/load screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveLoadKey {
    List(ListKey),
    Enter,
    Escape,
    Backspace,
    /// A description character.
    Char(u8),
    Brightness(bool),
    Quit,
}
/// §3.14-3.15 the save/load screen's state: its mode, the slot states read
/// from disk, the selection (which persists for the program run: take it
/// with [`SaveLoadScreen::into_list`]), the description box, a held scroll
/// arrow, and whether the options menu opened it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveLoadScreen {
    pub mode: Mode,
    pub slots: Vec<SlotState>,
    pub list: SlotList,
    pub description: Option<Description>,
    arrow: Option<(ListKey, super::HoldRepeat)>,
    pub from_options: bool,
}
impl SaveLoadScreen {
    pub fn open(mode: Mode, slots: Vec<SlotState>, list: SlotList) -> Self {
        Self {
            mode,
            slots,
            list,
            description: None,
            arrow: None,
            from_options: false,
        }
    }
    /// The selection to keep for the next opening.
    pub fn into_list(self) -> SlotList {
        self.list
    }
    pub fn layout(&self) -> Layout<SaveLoadControl> {
        layout(self.description.is_some())
    }
    /// The selected slot's state (a missing slot reads as empty).
    pub fn selected_state(&self) -> &SlotState {
        self.slots
            .get(self.list.selected)
            .unwrap_or(&SlotState::Empty)
    }
    /// The held scroll arrow's control, while one repeats.
    pub fn arrow_held(&self) -> Option<SaveLoadControl> {
        self.arrow.map(|(key, _)| match key {
            ListKey::Up => SaveLoadControl::ArrowUp,
            _ => SaveLoadControl::ArrowDown,
        })
    }
    /// A left press (the controls act on the press). While an arrow is
    /// held every input is swallowed. The description box's buttons
    /// finish it; Done commits; Cancel cancels; an arrow steps and starts
    /// its repeat (§3.14 "Held arrows"); the list selects, and a second
    /// click on the remembered slot commits.
    pub fn press(&mut self, point: [i32; 2], now: u64) -> SaveLoadOutcome {
        if self.arrow.is_some() {
            return SaveLoadOutcome::None;
        }
        match self.layout().hit(point) {
            Some(SaveLoadControl::DescriptionDone) => self.finish_description(true),
            Some(SaveLoadControl::DescriptionCancel) => self.finish_description(false),
            Some(SaveLoadControl::Done) => self.commit(),
            Some(SaveLoadControl::Cancel) => self.cancel(),
            Some(control @ (SaveLoadControl::ArrowUp | SaveLoadControl::ArrowDown)) => {
                let key = if control == SaveLoadControl::ArrowUp {
                    ListKey::Up
                } else {
                    ListKey::Down
                };
                self.list.key(key);
                self.arrow = Some((key, super::HoldRepeat::new(now, ARROW_IDLE_FRAMES)));
                SaveLoadOutcome::Changed
            }
            Some(SaveLoadControl::List) => match self.list.click(point[1]) {
                Click::Done(_) => self.commit(),
                Click::Select(_) => SaveLoadOutcome::Changed,
            },
            None => SaveLoadOutcome::None,
        }
    }
    /// A key. While an arrow is held every key, Esc included, is
    /// swallowed; the description box takes Enter, Esc, Backspace and
    /// characters.
    pub fn key(&mut self, key: SaveLoadKey) -> SaveLoadOutcome {
        if self.arrow.is_some() {
            return SaveLoadOutcome::None;
        }
        if let Some(description) = &mut self.description {
            return match key {
                SaveLoadKey::Enter => self.finish_description(true),
                SaveLoadKey::Escape => self.finish_description(false),
                SaveLoadKey::Backspace => {
                    description.backspace();
                    SaveLoadOutcome::Changed
                }
                SaveLoadKey::Char(code) => {
                    description.key(code);
                    SaveLoadOutcome::Changed
                }
                _ => SaveLoadOutcome::None,
            };
        }
        match key {
            SaveLoadKey::List(key) => {
                self.list.key(key);
                SaveLoadOutcome::Changed
            }
            SaveLoadKey::Enter => self.commit(),
            SaveLoadKey::Escape => self.cancel(),
            SaveLoadKey::Brightness(up) => SaveLoadOutcome::Brightness(up),
            SaveLoadKey::Quit => SaveLoadOutcome::QuitConfirm,
            SaveLoadKey::Backspace | SaveLoadKey::Char(_) => SaveLoadOutcome::None,
        }
    }
    /// Done, Enter or a double click on the selected slot. Saving asks
    /// before overwriting an occupied slot, else opens an empty
    /// description. Loading does nothing on an empty slot, refuses a
    /// corrupt one (`lsgame.msg` 134 and, under the safe rule, 135) or an
    /// old version (SPEC-019 §3.11: 134 and 136), and loads an occupied one.
    pub fn commit(&mut self) -> SaveLoadOutcome {
        let slot = self.list.selected;
        if self.mode.saving() {
            if matches!(self.selected_state(), SlotState::Occupied(_)) {
                return SaveLoadOutcome::AskOverwrite(slot);
            }
            self.description = Some(Description::open(b""));
            return SaveLoadOutcome::DescriptionOpened;
        }
        match self.selected_state() {
            SlotState::Empty => SaveLoadOutcome::None,
            SlotState::Corrupt => SaveLoadOutcome::Notice {
                title: 134,
                body: 135,
            },
            SlotState::OldVersion => SaveLoadOutcome::Notice {
                title: 134,
                body: 136,
            },
            SlotState::Occupied(_) => SaveLoadOutcome::Load(slot),
        }
    }
    /// Yes to the overwrite question: the description box, prefilled with
    /// the slot's description.
    pub fn overwrite_accepted(&mut self) -> SaveLoadOutcome {
        match self.selected_state() {
            SlotState::Occupied(info) => {
                self.description = Some(Description::open(&info.description));
                SaveLoadOutcome::DescriptionOpened
            }
            _ => SaveLoadOutcome::None,
        }
    }
    /// The description box closes; accepted, the slot is written.
    pub fn finish_description(&mut self, accept: bool) -> SaveLoadOutcome {
        match self.description.take() {
            Some(description) if accept => SaveLoadOutcome::Save {
                slot: self.list.selected,
                description: description.text,
            },
            Some(_) => SaveLoadOutcome::Changed,
            None => SaveLoadOutcome::None,
        }
    }
    /// Esc or Cancel.
    pub fn cancel(&mut self) -> SaveLoadOutcome {
        SaveLoadOutcome::Cancel {
            to_options: self.from_options,
        }
    }
    /// A failed save rescans: new slot states, the screen stays open.
    pub fn rescan(&mut self, slots: Vec<SlotState>) {
        self.slots = slots;
    }
    /// The left button came up: a held arrow stops.
    pub fn release(&mut self) {
        self.arrow = None;
    }
    /// One modal frame: the double-click window runs down, and a held
    /// arrow repeats while the pointer stays on it (`on_arrow`), else it
    /// stops. Returns whether the selection moved.
    pub fn frame(&mut self, now: u64, on_arrow: bool) -> bool {
        self.list.frame();
        let Some((key, repeat)) = &mut self.arrow else {
            return false;
        };
        if !on_arrow {
            self.arrow = None;
            return false;
        }
        if repeat.poll(now) {
            let key = *key;
            self.list.key(key);
            return true;
        }
        false
    }
}
