//! SPEC-014 §3.19-3.25: the interface bar (window, buttons, hotkeys), the
//! message monitor, HP/AC counters, AP pips, the active item slot, the
//! indicator boxes and the combat panel.
use super::{ColorKey, Rect};

/// The bar window: screen (0, 379), 640 by 100, over `iface.frm`.
pub const BAR_ORIGIN: [i32; 2] = [0, 379];
pub const BAR_SIZE: [i32; 2] = [640, 100];
pub const BAR_BACKGROUND: &str = "iface.frm";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarButton {
    Inventory,
    Options,
    Skilldex,
    Map,
    Character,
    PipBoy,
    SwapHands,
    ItemSlot,
    EndTurn,
    EndCombat,
}
/// Rectangle, up and down art, and the key the release sends.
pub const BAR_BUTTONS: [(BarButton, Rect, &str, &str, Key); 10] = [
    (
        BarButton::Inventory,
        Rect::new(211, 41, 32, 21),
        "invbutup.frm",
        "invbutdn.frm",
        Key::Char('i'),
    ),
    (
        BarButton::Options,
        Rect::new(210, 62, 34, 34),
        "optiup.frm",
        "optidn.frm",
        Key::Char('o'),
    ),
    (
        BarButton::Skilldex,
        Rect::new(523, 7, 22, 21),
        "bigredup.frm",
        "bigreddn.frm",
        Key::Char('s'),
    ),
    (
        BarButton::Map,
        Rect::new(526, 40, 41, 19),
        "mapup.frm",
        "mapdn.frm",
        Key::Tab,
    ),
    (
        BarButton::Character,
        Rect::new(526, 59, 41, 19),
        "chaup.frm",
        "chadn.frm",
        Key::Char('c'),
    ),
    (
        BarButton::PipBoy,
        Rect::new(526, 78, 41, 19),
        "pipup.frm",
        "pipdn.frm",
        Key::Char('p'),
    ),
    (
        BarButton::SwapHands,
        Rect::new(218, 6, 22, 21),
        "bigredup.frm",
        "bigreddn.frm",
        Key::Char('b'),
    ),
    (
        BarButton::ItemSlot,
        Rect::new(267, 26, 188, 67),
        "sattkbup.frm",
        "sattkbdn.frm",
        Key::Char('n'),
    ),
    (
        BarButton::EndTurn,
        Rect::new(590, 43, 38, 22),
        "endturnu.frm",
        "endturnd.frm",
        Key::Space,
    ),
    (
        BarButton::EndCombat,
        Rect::new(590, 65, 38, 22),
        "endcmbtu.frm",
        "endcmbtd.frm",
        Key::Enter,
    ),
];
/// Bar buttons press and release sounds (the item slot uses the large pair).
pub const BUTTON_SOUNDS: (&str, &str) = ("ib2p1xx1", "ib2lu1x1");
pub const SLOT_SOUNDS: (&str, &str) = ("ib3p1xx1", "ib3lu1x1");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Tab,
    Space,
    Enter,
    Escape,
    Home,
    CtrlQ,
    CtrlX,
    CtrlP,
    CtrlS,
    CtrlL,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F10,
    F12,
}
/// §3.19 game-loop actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Inventory,
    Options,
    Skilldex,
    Character,
    SwapHands,
    CycleItemMode,
    Automap,
    PipBoy,
    AlarmClock,
    CycleCursor,
    StartCombat,
    /// Hotkeys `1`-`8`: SPEC-013 §3.1 (0 is Sneak).
    Skill(u8),
    CenterPlayer,
    Rotate(bool),
    Brightness(i32),
    DateTime,
    QuitConfirm,
    Pause,
    Save,
    Load,
    QuickSave,
    QuickLoad,
    MasterVolume(i32),
    Screenshot,
}
/// The shifted characters equivalent to `1`-`8`.
const SHIFTED_DIGITS: &str = "!@#$%^&*";
/// A key in the game loop. `enabled` is the bar state; most actions need
/// it. Every listed action plays `ib1p1xx1` first.
pub fn game_key(key: Key, enabled: bool, alt: bool) -> Option<Action> {
    let always = match key {
        Key::CtrlQ | Key::CtrlX | Key::F10 => Some(Action::QuitConfirm),
        Key::CtrlP => Some(Action::Pause),
        Key::CtrlS | Key::F4 => Some(Action::Save),
        Key::CtrlL | Key::F5 => Some(Action::Load),
        Key::F6 => Some(Action::QuickSave),
        Key::F7 => Some(Action::QuickLoad),
        Key::F2 => Some(Action::MasterVolume(-2047)),
        Key::F3 => Some(Action::MasterVolume(2047)),
        Key::F12 => Some(Action::Screenshot),
        _ => None,
    };
    if always.is_some() || !enabled {
        return always;
    }
    Some(match key {
        Key::Char('i' | 'I') => Action::Inventory,
        Key::Char('o' | 'O') | Key::Escape => Action::Options,
        Key::Char('s' | 'S') => Action::Skilldex,
        Key::Char('c' | 'C') => Action::Character,
        Key::Char('b' | 'B') => Action::SwapHands,
        Key::Char('n' | 'N') => Action::CycleItemMode,
        Key::Tab if !alt => Action::Automap,
        Key::Char('p' | 'P') => Action::PipBoy,
        Key::Char('z' | 'Z') => Action::AlarmClock,
        Key::Char('m' | 'M') => Action::CycleCursor,
        Key::Char('a' | 'A') => Action::StartCombat,
        Key::Char(c @ '1'..='8') => Action::Skill(c as u8 - b'1'),
        Key::Char(c) if SHIFTED_DIGITS.contains(c) => {
            Action::Skill(SHIFTED_DIGITS.find(c).unwrap_or(0) as u8)
        }
        Key::Home => Action::CenterPlayer,
        Key::Char(',') => Action::Rotate(false),
        Key::Char('.') => Action::Rotate(true),
        Key::Char('=' | '+') => Action::Brightness(1),
        Key::Char('-' | '_') => Action::Brightness(-1),
        Key::Char('/') => Action::DateTime,
        _ => return None,
    })
}

/// §3.20 the message monitor: a ring of 100 lines of up to 79 characters,
/// 6 visible, in font 101.
pub const MONITOR: Rect = Rect::new(23, 24, 167, 60);
pub const MONITOR_LINES: usize = 100;
pub const MONITOR_VISIBLE: usize = 6;
pub const MONITOR_LINE_CHARS: usize = 79;
pub const MONITOR_COLOR: ColorKey = ColorKey::GREEN;
pub const BULLET: u8 = 0x95;
pub const MONITOR_SOUND: &str = "monitor";
pub const MONITOR_SOUND_MS: u64 = 500;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Monitor {
    lines: std::collections::VecDeque<Vec<u8>>,
    /// Lines scrolled back from the newest.
    back: usize,
    last_sound: Option<u64>,
}
impl Default for Monitor {
    fn default() -> Self {
        Self {
            lines: std::collections::VecDeque::with_capacity(MONITOR_LINES),
            back: 0,
            last_sound: None,
        }
    }
}
impl Monitor {
    /// Add a message wrapped with `width` (font 101 with its spacing); the
    /// view snaps to the newest line. Returns whether `monitor` plays (out
    /// of combat, at most once per 500 ms).
    pub fn push(
        &mut self,
        text: &[u8],
        width: impl Fn(&[u8]) -> i32,
        now: u64,
        combat: bool,
    ) -> bool {
        for line in wrap_message(text, &width) {
            if self.lines.len() == MONITOR_LINES {
                self.lines.pop_front();
            }
            self.lines.push_back(line);
        }
        self.back = 0;
        let sound = !combat
            && self
                .last_sound
                .is_none_or(|last| now >= last + MONITOR_SOUND_MS);
        if sound {
            self.last_sound = Some(now);
        }
        sound
    }
    /// The six visible rows, oldest at top (line k is drawn at x = 23 + k);
    /// the newest line is on the bottom row, with empty rows above it while
    /// fewer than six lines exist.
    pub fn visible(&self) -> Vec<&[u8]> {
        let end = self.lines.len().saturating_sub(self.back);
        let start = end.saturating_sub(MONITOR_VISIBLE);
        let mut rows = vec![[].as_slice(); MONITOR_VISIBLE - (end - start)];
        rows.extend(self.lines.range(start..end).map(Vec::as_slice));
        rows
    }
    /// Scroll one line older (safe rule: stop at the oldest stored line).
    pub fn older(&mut self) {
        if self.back + MONITOR_VISIBLE < self.lines.len() {
            self.back += 1;
        }
    }
    /// Scroll one line newer, stopping at the newest.
    pub fn newer(&mut self) {
        self.back = self.back.saturating_sub(1);
    }
}
/// The upper and lower scroll halves of the monitor.
pub const MONITOR_OLDER: Rect = Rect::new(23, 24, 167, 30);
pub const MONITOR_NEWER: Rect = Rect::new(23, 54, 167, 30);
/// Line x for visible line `k`.
pub fn monitor_line_x(k: usize) -> i32 {
    23 + k as i32
}
/// Wrap a message: the first line starts with the bullet and holds text
/// narrower than 155 pixels, continuation lines narrower than 161; breaks
/// are at the last space. A word too long for a line is stored truncated.
pub fn wrap_message(text: &[u8], width: impl Fn(&[u8]) -> i32) -> Vec<Vec<u8>> {
    let mut lines = Vec::new();
    let mut rest = text;
    let mut first = true;
    loop {
        let limit = if first { 155 } else { 161 };
        let fitting = (0..=rest.len())
            .rev()
            .find(|end| width(&rest[..*end]) < limit)
            .unwrap_or(0);
        let (line, next) = if fitting == rest.len() {
            (rest, &rest[rest.len()..])
        } else if let Some(space) = rest[..fitting].iter().rposition(|b| *b == b' ') {
            (&rest[..space], &rest[space + 1..])
        } else {
            let word_end = rest.iter().position(|b| *b == b' ').unwrap_or(rest.len());
            (&rest[..word_end], &rest[(word_end + 1).min(rest.len())..])
        };
        let mut stored = if first { vec![BULLET] } else { Vec::new() };
        stored.extend_from_slice(line);
        stored.truncate(MONITOR_LINE_CHARS);
        lines.push(stored);
        first = false;
        rest = next;
        if rest.is_empty() {
            return lines;
        }
    }
}

/// §3.21 counters in `numbers.frm`: three 120-pixel bands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    White,
    Yellow,
    Red,
}
impl Band {
    pub fn offset(self) -> i32 {
        match self {
            Self::White => 0,
            Self::Yellow => 120,
            Self::Red => 240,
        }
    }
}
pub const HP_COUNTER: [i32; 2] = [473, 40];
pub const AC_COUNTER: [i32; 2] = [473, 75];
/// HP color: red below a quarter of maximum, yellow below half.
pub fn hp_band(hp: i32, max: i32) -> Band {
    if hp < max / 4 {
        Band::Red
    } else if hp < max / 2 {
        Band::Yellow
    } else {
        Band::White
    }
}
/// One counter cell's glyph: x within the band and its width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Digit(u8),
    RollDown,
    RollUp,
    Minus,
    Blank,
}
impl Glyph {
    pub fn x(self) -> i32 {
        match self {
            Self::Digit(d) => 9 * i32::from(d),
            Self::RollDown => 90,
            Self::RollUp => 99,
            Self::Minus => 108,
            Self::Blank => 114,
        }
    }
    pub fn width(self) -> i32 {
        match self {
            Self::Minus | Self::Blank => 6,
            _ => 9,
        }
    }
}
/// The four cells (sign, hundreds, tens, ones) at offsets 0, 6, 15, 24.
pub const COUNTER_CELLS: [i32; 4] = [0, 6, 15, 24];
pub fn counter_glyphs(value: i32) -> [Glyph; 4] {
    let value = value.clamp(-999, 999);
    let digits = value.unsigned_abs();
    [
        if value < 0 {
            Glyph::Minus
        } else {
            Glyph::Blank
        },
        Glyph::Digit((digits / 100) as u8),
        Glyph::Digit((digits / 10 % 10) as u8),
        Glyph::Digit((digits % 10) as u8),
    ]
}
/// The delay between roll frames for a change from `old` to `new`.
pub fn roll_delay(old: i32, new: i32) -> u64 {
    250 / (u64::from(old.abs_diff(new)) + 1)
}
/// The frames of one unit step from `value` to `value ± 1`: "up" in the
/// ones cell; on a carry "up" in the tens cell, on a further carry "up"
/// then "down" in the hundreds cell, then "down" in the tens cell; "down" in
/// the ones cell; then the true digits. Each frame is followed by a wait.
pub fn roll_frames(value: i32, up: bool) -> Vec<[Glyph; 4]> {
    let next = if up { value + 1 } else { value - 1 };
    let mut cells = counter_glyphs(value);
    let mut frames = Vec::new();
    let show = |cells: &mut [Glyph; 4], cell: usize, glyph: Glyph, frames: &mut Vec<[Glyph; 4]>| {
        cells[cell] = glyph;
        frames.push(*cells);
    };
    let tens_carry = value.abs() / 10 != next.abs() / 10;
    let hundreds_carry = value.abs() / 100 != next.abs() / 100;
    show(&mut cells, 3, Glyph::RollUp, &mut frames);
    if tens_carry {
        show(&mut cells, 2, Glyph::RollUp, &mut frames);
        if hundreds_carry {
            show(&mut cells, 1, Glyph::RollUp, &mut frames);
            show(&mut cells, 1, Glyph::RollDown, &mut frames);
        }
        show(&mut cells, 2, Glyph::RollDown, &mut frames);
    }
    show(&mut cells, 3, Glyph::RollDown, &mut frames);
    frames.push(counter_glyphs(next));
    frames
}
/// A roll crossing a color threshold is split at it; each segment uses the
/// band of the value it starts in.
pub fn roll_segments(old: i32, new: i32, max: i32) -> Vec<(i32, i32, Band)> {
    let mut segments = Vec::new();
    let mut start = old;
    let mut value = old;
    while value != new {
        let next = if new > value { value + 1 } else { value - 1 };
        if hp_band(next, max) != hp_band(start, max) {
            segments.push((start, next, hp_band(start, max)));
            start = next;
        }
        value = next;
    }
    if start != new || segments.is_empty() {
        segments.push((start, new, hp_band(start, max)));
    }
    segments
}

/// §3.22 AP pips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pip {
    Green,
    Red,
}
pub const PIP_STRIP: Rect = Rect::new(316, 14, 90, 5);
pub const PIP_ART: (&str, &str) = ("hlgrn.frm", "hlred.frm");
pub fn pip_x(i: usize) -> i32 {
    316 + 9 * i as i32
}
/// `None` draws the empty strip (out of combat); "not your turn" is all
/// red; otherwise AP green pips, clamped to 0-10.
pub fn pips(ap: Option<i32>, your_turn: bool) -> [Option<Pip>; 10] {
    match ap {
        None => [None; 10],
        Some(_) if !your_turn => [Some(Pip::Red); 10],
        Some(ap) => {
            let count = ap.clamp(0, 10) as usize;
            std::array::from_fn(|i| (i < count).then_some(Pip::Green))
        }
    }
}

/// §3.23 the item slot: the action shown and its label art.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotAction {
    UseOn,
    Use,
    Reload,
    /// A weapon attack by its animation nibble.
    Attack(u8),
}
pub const SLOT: Rect = Rect::new(267, 26, 188, 67);
pub const SLOT_DISABLED: &str = "saturbup.frm";
/// The action label art; `None` draws no label (safe rule for an attack
/// animation outside the list).
pub fn slot_label(action: SlotAction) -> Option<&'static str> {
    use crate::combat::attack_mode as mode;
    Some(match action {
        SlotAction::UseOn => "useon.frm",
        SlotAction::Use => "uset.frm",
        SlotAction::Reload => "reload.frm",
        SlotAction::Attack(mode::PUNCH) => "punch.frm",
        SlotAction::Attack(mode::KICK) => "unarmed.frm",
        SlotAction::Attack(mode::THROW) => "throw.frm",
        SlotAction::Attack(mode::THRUST) => "thrust.frm",
        SlotAction::Attack(mode::SWING) => "swing.frm",
        SlotAction::Attack(mode::SINGLE) => "single.frm",
        SlotAction::Attack(mode::BURST | mode::CONTINUOUS) => "burst.frm",
        SlotAction::Attack(_) => return None,
    })
}
/// Label origin (right edge at 181) for a label `width` wide; the pressed
/// image draws it darkened one right and two up.
pub fn slot_label_origin(width: i32, pressed: bool) -> [i32; 2] {
    if pressed {
        [182 - width, 5]
    } else {
        [181 - width, 7]
    }
}
pub const BULLSEYE: (&str, [i32; 2], [i32; 2]) = ("bullseye.frm", [163, 42], [164, 40]);
pub const COST_LABEL: (&str, [i32; 2], [i32; 2]) = ("mvepnt.frm", [7, 48], [8, 46]);
pub const COST_DIGIT: (&str, [i32; 2], [i32; 2]) = ("mvenum.frm", [31, 48], [32, 46]);
/// Light levels of the pressed image's label and item art.
pub const PRESSED_LABEL_LIGHT: u32 = 59641;
pub const PRESSED_ITEM_LIGHT: u32 = 63571;
/// The AP cost cell (digit cells 10 wide); costs outside 0-9 replace both
/// images with `saturbup.frm`.
pub fn cost_digit(cost: i32) -> Option<u8> {
    (0..=9).contains(&cost).then_some(cost as u8)
}
/// Item art origin, centered unscaled in 188 by 67.
pub fn item_art_origin(w: i32, h: i32) -> [i32; 2] {
    [(188 - w) / 2, (67 - h) / 2]
}
/// The ammo bar: x = 463 from y = 26, 70 tall; the fill is
/// `trunc(current / max * 70)` rounded down to even. Pixels from the top:
/// raw index 14, then alternately 196 and 14.
pub fn ammo_bar(current: i32, max: i32) -> [u8; 70] {
    let fill = if max > 0 {
        ((f64::from(current) / f64::from(max) * 70.0) as i32).clamp(0, 70) & !1
    } else {
        0
    } as usize;
    std::array::from_fn(|y| {
        if y < 70 - fill || (y - (70 - fill)) % 2 == 1 {
            14
        } else {
            196
        }
    })
}
/// Weapon modes cycled by right-click or `n`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaponMode {
    Primary,
    PrimaryAimed,
    Secondary,
    SecondaryAimed,
    Reload,
}
const CYCLE: [WeaponMode; 5] = [
    WeaponMode::Primary,
    WeaponMode::PrimaryAimed,
    WeaponMode::Secondary,
    WeaponMode::SecondaryAimed,
    WeaponMode::Reload,
];
/// The next available mode after `current`, wrapping to primary.
pub fn next_weapon_mode(current: WeaponMode, available: impl Fn(WeaponMode) -> bool) -> WeaponMode {
    let start = CYCLE.iter().position(|m| *m == current).unwrap_or(0);
    (1..=CYCLE.len())
        .map(|step| CYCLE[(start + step) % CYCLE.len()])
        .find(|mode| *mode == WeaponMode::Primary || available(*mode))
        .unwrap_or(WeaponMode::Primary)
}
/// A weapon is disabled with both arms crippled, or one with a two-handed
/// weapon.
pub fn slot_disabled(left_crippled: bool, right_crippled: bool, two_handed: bool) -> bool {
    (left_crippled && right_crippled) || ((left_crippled || right_crippled) && two_handed)
}

/// §3.24 indicator boxes, always ordered CHEM, SNEAK, LEVEL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indicator {
    Chem,
    Sneak,
    Level,
}
pub const INDICATOR_ORIGIN: [i32; 2] = [0, 358];
pub const INDICATOR_BOX: (&str, [i32; 2]) = ("warnbox.frm", [130, 21]);
impl Indicator {
    /// `intrface.msg` label and color.
    pub fn label(self) -> (i32, ColorKey) {
        match self {
            Self::Sneak => (100, ColorKey::GREEN),
            Self::Level => (101, ColorKey::GREEN),
            Self::Chem => (102, ColorKey::RED),
        }
    }
}
pub fn indicators(chem: bool, sneak: bool, level: bool) -> Vec<Indicator> {
    [
        (chem, Indicator::Chem),
        (sneak, Indicator::Sneak),
        (level, Indicator::Level),
    ]
    .into_iter()
    .filter_map(|(on, indicator)| on.then_some(indicator))
    .collect()
}
/// Box k's x: the first is drawn 127 wide (left 3 pixels cut) at 0.
pub fn indicator_x(k: usize) -> i32 {
    if k == 0 {
        0
    } else {
        130 * k as i32 - 3 * (k as i32 + 1)
    }
}
/// Labels center horizontally at y = 7 (a box height of 24 is assumed).
pub const INDICATOR_LABEL_Y: i32 = 7;
pub fn indicator_window_width(count: usize) -> i32 {
    127 * count as i32
}

/// §3.25 combat panel: `endanim.frm` (5 frames, 200 ms each) at (580, 38).
pub const PANEL: (&str, [i32; 2]) = ("endanim.frm", [580, 38]);
pub const PANEL_FRAME_MS: u64 = 200;
pub const PANEL_OPEN_SOUND: &str = "iciboxx1";
pub const PANEL_CLOSE_SOUND: &str = "icibcxx1";
/// The lights at the player's turn start (green) and end (red).
pub const LIGHTS: [(&str, &str); 2] = [("endltgrn.frm", "icombat2"), ("endltred.frm", "icombat1")];
/// The panel frame shown `elapsed` ms into an open (or close) animation.
pub fn panel_frame(elapsed: u64, opening: bool) -> u8 {
    let step = (elapsed / PANEL_FRAME_MS).min(4) as u8;
    if opening { step } else { 4 - step }
}
