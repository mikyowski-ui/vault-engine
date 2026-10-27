//! SPEC-014 §2.3 and §3.17-3.18: preferences (defaults, ranges, the
//! `fallout.cfg` keys and the SPEC-008 save words), the options menu, the
//! pause window, the quit confirmation, and the preferences screen's knobs,
//! switches, sliders and brightness steps.
use super::{Layout, Rect};
use vault_formats::cfg::ConfigFile;

/// Every preference value, at its §2.3 default.
#[derive(Debug, Clone, PartialEq)]
pub struct Preferences {
    pub game_difficulty: i32,
    pub combat_difficulty: i32,
    pub violence_level: i32,
    pub target_highlight: i32,
    pub running_burning_guy: i32,
    pub combat_messages: i32,
    pub combat_taunts: i32,
    pub language_filter: i32,
    pub running: i32,
    pub subtitles: i32,
    pub item_highlight: i32,
    pub combat_speed: i32,
    pub text_base_delay: f64,
    pub text_line_delay: f64,
    pub player_speedup: i32,
    pub brightness: f64,
    pub mouse_sensitivity: f64,
    pub master_volume: i32,
    pub music_volume: i32,
    pub sndfx_volume: i32,
    pub speech_volume: i32,
}
pub const DEFAULT_VOLUME: i32 = 22281;
pub const BRIGHTNESS_MIN: f64 = 1.0;
pub const BRIGHTNESS_MAX: f64 = 1.179_992_675_781_25;
/// One brightness hotkey step (16 steps span the range).
pub const BRIGHTNESS_STEP: f64 = 0.011_249_542_236_328_12;
impl Default for Preferences {
    fn default() -> Self {
        Self {
            game_difficulty: 1,
            combat_difficulty: 1,
            violence_level: 3,
            target_highlight: 2,
            running_burning_guy: 1,
            combat_messages: 1,
            combat_taunts: 1,
            language_filter: 0,
            running: 0,
            subtitles: 0,
            item_highlight: 1,
            combat_speed: 0,
            text_base_delay: 3.5,
            text_line_delay: 1.399_994,
            player_speedup: 0,
            brightness: BRIGHTNESS_MIN,
            mouse_sensitivity: 1.0,
            master_volume: DEFAULT_VOLUME,
            music_volume: DEFAULT_VOLUME,
            sndfx_volume: DEFAULT_VOLUME,
            speech_volume: DEFAULT_VOLUME,
        }
    }
}

const INTEGER_KEYS: [(&str, &str, i32, i32); 17] = [
    ("preferences", "game_difficulty", 0, 2),
    ("preferences", "combat_difficulty", 0, 2),
    ("preferences", "violence_level", 0, 3),
    ("preferences", "target_highlight", 0, 2),
    ("preferences", "running_burning_guy", 0, 1),
    ("preferences", "combat_messages", 0, 1),
    ("preferences", "combat_taunts", 0, 1),
    ("preferences", "language_filter", 0, 1),
    ("preferences", "running", 0, 1),
    ("preferences", "subtitles", 0, 1),
    ("preferences", "item_highlight", 0, 1),
    ("preferences", "combat_speed", 0, 50),
    ("preferences", "player_speedup", 0, 1),
    ("sound", "master_volume", 0, 32767),
    ("sound", "music_volume", 0, 32767),
    ("sound", "sndfx_volume", 0, 32767),
    ("sound", "speech_volume", 0, 32767),
];
const REAL_KEYS: [(&str, f64, f64); 4] = [
    ("text_base_delay", 1.0, 6.0),
    ("text_line_delay", 0.0, 2.0),
    ("brightness", BRIGHTNESS_MIN, BRIGHTNESS_MAX),
    ("mouse_sensitivity", 1.0, 2.5),
];

impl Preferences {
    fn integers(&mut self) -> [&mut i32; 17] {
        [
            &mut self.game_difficulty,
            &mut self.combat_difficulty,
            &mut self.violence_level,
            &mut self.target_highlight,
            &mut self.running_burning_guy,
            &mut self.combat_messages,
            &mut self.combat_taunts,
            &mut self.language_filter,
            &mut self.running,
            &mut self.subtitles,
            &mut self.item_highlight,
            &mut self.combat_speed,
            &mut self.player_speedup,
            &mut self.master_volume,
            &mut self.music_volume,
            &mut self.sndfx_volume,
            &mut self.speech_volume,
        ]
    }
    fn reals(&mut self) -> [&mut f64; 4] {
        [
            &mut self.text_base_delay,
            &mut self.text_line_delay,
            &mut self.brightness,
            &mut self.mouse_sensitivity,
        ]
    }
    /// Read the configuration, clamping every value to its range. Safe rule:
    /// the burning-guy value is read from `running_burning_guy`, accepting
    /// the reference's written key `combat_looks`.
    pub fn from_config(config: &ConfigFile) -> Self {
        let mut preferences = Self::default();
        for ((section, key, low, high), value) in INTEGER_KEYS.iter().zip(preferences.integers()) {
            let read = config.get_int(section, key).or_else(|| {
                (*key == "running_burning_guy")
                    .then(|| config.get_int("preferences", "combat_looks"))
                    .flatten()
            });
            if let Some(read) = read {
                *value = read.clamp(i64::from(*low), i64::from(*high)) as i32;
            }
        }
        for ((key, low, high), value) in REAL_KEYS.iter().zip(preferences.reals()) {
            // R-05: a non-finite real (`brightness=nan`) would survive the
            // clamp, so it keeps the default as a missing key does.
            if let Some(read) = config
                .get_real("preferences", key)
                .filter(|read| read.is_finite())
            {
                *value = read.clamp(*low, *high);
            }
        }
        preferences
    }
    /// Write every value (the burning-guy key under its read name).
    pub fn write_config(&self, config: &mut ConfigFile) {
        let mut copy = self.clone();
        for ((section, key, ..), value) in INTEGER_KEYS.iter().zip(copy.integers()) {
            config.set(section, key, value.to_string());
        }
        for ((key, ..), value) in REAL_KEYS.iter().zip(copy.reals()) {
            config.set("preferences", key, format!("{value:.6}"));
        }
    }
    /// The 20 SPEC-008 save words (text delay, brightness and mouse
    /// sensitivity as binary32).
    pub fn to_words(&self) -> [u32; 20] {
        let float = |value: f64| (value as f32).to_bits();
        [
            self.game_difficulty as u32,
            self.combat_difficulty as u32,
            self.violence_level as u32,
            self.target_highlight as u32,
            self.running_burning_guy as u32,
            self.combat_messages as u32,
            self.combat_taunts as u32,
            self.language_filter as u32,
            self.running as u32,
            self.subtitles as u32,
            self.item_highlight as u32,
            self.combat_speed as u32,
            self.player_speedup as u32,
            float(self.text_base_delay),
            self.master_volume as u32,
            self.music_volume as u32,
            self.sndfx_volume as u32,
            self.speech_volume as u32,
            float(self.brightness),
            float(self.mouse_sensitivity),
        ]
    }
    /// Values from save words, each clamped to its range on application;
    /// the line delay (not saved) is derived as on reapplication.
    pub fn from_words(words: &[u32; 20]) -> Self {
        let mut preferences = Self::default();
        let integer_words = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 14, 15, 16, 17];
        for (((_, _, low, high), value), word) in INTEGER_KEYS
            .iter()
            .zip(preferences.integers())
            .zip(integer_words)
        {
            *value = (words[word] as i32).clamp(*low, *high);
        }
        let real = |word: usize, low: f64, high: f64| {
            let value = f64::from(f32::from_bits(words[word]));
            if value.is_finite() {
                value.clamp(low, high)
            } else {
                low
            }
        };
        preferences.text_base_delay = real(13, 1.0, 6.0);
        preferences.brightness = real(18, BRIGHTNESS_MIN, BRIGHTNESS_MAX);
        preferences.mouse_sensitivity = real(19, 1.0, 2.5);
        preferences.text_line_delay = line_delay(preferences.text_base_delay);
        preferences
    }
}
/// Reapplication: line delay `(base - 1) / 5 * 2`, clamped to 0-2.
pub fn line_delay(base: f64) -> f64 {
    ((base - 1.0) / 5.0 * 2.0).clamp(0.0, 2.0)
}
/// A brightness hotkey: one step within range.
pub fn brightness_step(value: f64, up: bool) -> f64 {
    let next = if up {
        value + BRIGHTNESS_STEP
    } else {
        value - BRIGHTNESS_STEP
    };
    if (BRIGHTNESS_MIN..=BRIGHTNESS_MAX).contains(&next) {
        next
    } else {
        value
    }
}

/// §3.17 options menu: `opbase.frm` at screen (238, 71), five buttons.
pub const OPTIONS_ORIGIN: [i32; 2] = [238, 71];
pub const OPTIONS_SIZE: [i32; 2] = [164, 217];
pub fn options_button(i: usize) -> Rect {
    Rect::new(13, 17 + 37 * i as i32, 137, 34)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsAction {
    SaveGame,
    LoadGame,
    Preferences,
    ExitGame,
    Done,
}
pub const OPTIONS_BUTTONS: [OptionsAction; 5] = [
    OptionsAction::SaveGame,
    OptionsAction::LoadGame,
    OptionsAction::Preferences,
    OptionsAction::ExitGame,
    OptionsAction::Done,
];
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKey {
    Char(char),
    Escape,
    Enter,
    CtrlQ,
    CtrlX,
    F10,
}
/// Options keys: the action, and whether `ib1p1xx1` plays first.
pub fn options_key(key: MenuKey) -> Option<(OptionsAction, bool)> {
    Some(match key {
        MenuKey::Escape => (OptionsAction::Done, false),
        MenuKey::Enter | MenuKey::Char('o' | 'O' | 'd' | 'D') => (OptionsAction::Done, true),
        MenuKey::Char('s' | 'S') => (OptionsAction::SaveGame, false),
        MenuKey::Char('l' | 'L') => (OptionsAction::LoadGame, false),
        MenuKey::Char('p' | 'P') => (OptionsAction::Preferences, true),
        MenuKey::Char('e' | 'E') | MenuKey::CtrlQ | MenuKey::CtrlX | MenuKey::F10 => {
            (OptionsAction::ExitGame, false)
        }
        MenuKey::Char(_) => return None,
    })
}

/// The pause window's screen origin in game and on the world map.
pub const PAUSE_ORIGIN: [i32; 2] = [250, 150];
pub const PAUSE_WORLD_ORIGIN: [i32; 2] = [185, 180];
/// "PAUSED" x for a title `width` pixels wide (font 104).
pub fn paused_title_x(width: i32) -> i32 {
    2 + (139 - width) / 2
}
/// The quit confirmation box (`misc.msg` 0, error color).
pub const QUIT_ORIGIN: [i32; 2] = [169, 117];

/// §3.18 knobs: position, value count and value label messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Knob {
    pub origin: [i32; 2],
    pub labels: &'static [i32],
    /// The row restored before drawing: (23, Y) 160 by 54.
    pub row_y: i32,
}
pub const KNOBS: [Knob; 5] = [
    Knob {
        origin: [76, 71],
        labels: &[203, 204, 205],
        row_y: 66,
    },
    Knob {
        origin: [76, 149],
        labels: &[206, 204, 208],
        row_y: 143,
    },
    Knob {
        origin: [76, 226],
        labels: &[214, 215, 204, 216],
        row_y: 222,
    },
    Knob {
        origin: [76, 309],
        labels: &[202, 201, 213],
        row_y: 304,
    },
    Knob {
        origin: [76, 387],
        labels: &[202, 201],
        row_y: 382,
    },
];
impl Knob {
    pub fn top(&self) -> i32 {
        self.labels.len() as i32 - 1
    }
    /// Label `i`'s anchor for a label `width` wide: (x, y) of its top-left.
    pub fn label_origin(&self, i: usize, width: i32) -> [i32; 2] {
        let [kx, ky] = self.origin;
        match i {
            0 => [kx + 2 - width, ky + 10],
            1 => [kx + 25 - width / 2, ky - 4],
            2 => [kx + 46, ky + 10],
            _ => [kx + 46, ky + 31],
        }
    }
    /// The click rectangle from label 0's left edge to the last label's
    /// right edge (full unsplit widths).
    pub fn rect(&self, widths: &[i32]) -> Rect {
        let [_, ky] = self.origin;
        let left = self.label_origin(0, widths[0])[0];
        let last = self.labels.len() - 1;
        let right = self.label_origin(last, widths[last])[0] + widths[last];
        Rect::new(left, ky - 4, right - left, 48)
    }
}
/// A knob release at `point`: the new value and direction (true is up).
/// Clicks outside every zone change nothing (but still mark "changed").
pub fn knob_click(
    knob: &Knob,
    widths: &[i32],
    point: [i32; 2],
    value: i32,
    up: bool,
) -> Option<(i32, bool)> {
    let rect = knob.rect(widths);
    if !rect.contains(point) {
        return None;
    }
    let [kx, ky] = knob.origin;
    let [x, y] = point;
    let (dx, dy) = (x - (kx + 23), y - (ky + 21));
    if dx * dx + dy * dy <= 16 * 16 {
        // Ping-pong: up to the top value, then down to 0, and so on.
        let top = knob.top();
        let mut up = if up { value < top } else { value <= 0 };
        let value = value + if up { 1 } else { -1 };
        if value >= top {
            up = false;
        } else if value <= 0 {
            up = true;
        }
        return Some((value, up));
    }
    let right = rect.x + rect.w;
    if y > ky && (ky + 10..=ky + 20).contains(&y) {
        if (rect.x..=kx).contains(&x) {
            return Some((0, true));
        }
        if knob.labels.len() >= 3 && (kx + 46..=right).contains(&x) {
            return Some((2, true));
        }
    }
    if y <= ky && (kx + 9..=kx + 37).contains(&x) {
        return Some((1, value == 0));
    }
    if knob.labels.len() == 4 && (ky + 31..=ky + 51).contains(&y) && (kx + 46..=right).contains(&x)
    {
        return Some((3, false));
    }
    Some((value, up))
}

/// §3.18 switches: position, left and right label messages; the frame is
/// the value except for combat messages (value XOR 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Switch {
    pub origin: [i32; 2],
    pub labels: [i32; 2],
    pub inverted: bool,
    pub row_y: i32,
}
pub const SWITCHES: [Switch; 6] = [
    Switch {
        origin: [299, 74],
        labels: [211, 212],
        inverted: true,
        row_y: 66,
    },
    Switch {
        origin: [299, 141],
        labels: [202, 201],
        inverted: false,
        row_y: 133,
    },
    Switch {
        origin: [299, 207],
        labels: [202, 201],
        inverted: false,
        row_y: 200,
    },
    Switch {
        origin: [299, 271],
        labels: [209, 219],
        inverted: false,
        row_y: 264,
    },
    Switch {
        origin: [299, 338],
        labels: [202, 201],
        inverted: false,
        row_y: 331,
    },
    Switch {
        origin: [299, 404],
        labels: [202, 201],
        inverted: false,
        row_y: 397,
    },
];
impl Switch {
    pub fn frame(&self, value: i32) -> i32 {
        if self.inverted { value ^ 1 } else { value }
    }
    /// The value a frame (0 left, 1 right) stands for.
    pub fn value(&self, frame: i32) -> i32 {
        self.frame(frame)
    }
    pub fn rect(&self, widths: [i32; 2]) -> Rect {
        let [kx, ky] = self.origin;
        let left = kx + 4 - widths[0];
        Rect::new(left, ky - 5, kx + 21 + widths[1] - left, 28)
    }
}
/// A switch release: the new value, or `None` outside the rectangle.
pub fn switch_click(switch: &Switch, widths: [i32; 2], point: [i32; 2], value: i32) -> Option<i32> {
    let rect = switch.rect(widths);
    if !rect.contains(point) {
        return None;
    }
    let [kx, ky] = switch.origin;
    let [x, y] = point;
    let (dx, dy) = (x - (kx + 11), y - (ky + 12));
    if dx * dx + dy * dy <= 10 * 10 {
        return Some(value ^ 1);
    }
    if (ky - 5..=ky + 7).contains(&y) {
        if (rect.x..=kx).contains(&x) {
            return Some(switch.value(0));
        }
        if (kx + 22..rect.x + rect.w).contains(&x) {
            return Some(switch.value(1));
        }
    }
    Some(value)
}

/// §3.18 sliders: the knob's left edge travels x = 384 through 603.
pub const SLIDER_LEFT: i32 = 384;
pub const SLIDER_RIGHT: i32 = 603;
pub fn slider_position(value: f64, min: f64, max: f64) -> i32 {
    ((value - min) * 219.0 / (max - min) + 384.0) as i32
}
/// Text delay runs right to left: 3.5 sits at x = 493.
pub fn delay_position(delay: f64) -> i32 {
    ((6.0 - delay) * 43.8 + 384.0) as i32
}
pub fn slider_value(position: i32, min: f64, max: f64) -> f64 {
    f64::from(position - 384) * (max - min) / 219.0 + min
}
pub fn delay_value(position: i32) -> f64 {
    6.0 - f64::from(position - 384) / 43.8
}
/// One drag iteration's hysteresis: follow the mouse when it is at least 14
/// right of the knob or below 6 right of it.
pub fn slider_drag(position: i32, mouse_x: i32) -> i32 {
    if mouse_x >= position + 14 {
        (mouse_x - 6).min(SLIDER_RIGHT)
    } else if mouse_x < position + 6 {
        (mouse_x - 6).max(SLIDER_LEFT)
    } else {
        position
    }
}
/// Slider knob rows (y) in caption order 112-119.
pub const SLIDER_KNOB_Y: [i32; 8] = [50, 125, 196, 247, 298, 349, 400, 451];
/// The hot rectangle for a slider at knob row `ky`.
pub fn slider_rect(ky: i32) -> Rect {
    Rect::new(SLIDER_LEFT, ky - 12, 240, 23)
}
/// Global 603 at 0 covers the fifth knob row with `prefcvr.frm`.
pub const COVER_GLOBAL: usize = 603;

// SPEC-020 §2.1: the options windows' layouts and dispatch. Rectangles are
// frame (screen) pixels.

/// §3.17 the options menu's buttons play `ib3p1xx1` and `ib3lu1x1`.
pub const OPTIONS_SOUNDS: (&str, &str) = ("ib3p1xx1", "ib3lu1x1");
/// §3.17 the options menu's five buttons on screen, in order.
pub fn options_layout() -> Layout<OptionsAction> {
    OPTIONS_BUTTONS
        .into_iter()
        .enumerate()
        .map(|(i, action)| (action, options_button(i).offset(OPTIONS_ORIGIN)))
        .collect()
}
/// §3.17 the pause window's red button (screen), at (26, 46) in the window.
pub fn pause_button(origin: [i32; 2]) -> Rect {
    super::red_button([26, 46]).offset(origin)
}

/// Which options window is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsPage {
    Menu,
    Pause,
    /// The quit confirmation at `QUIT_ORIGIN`.
    Quit,
    /// The preferences screen (its state is a [`PreferencesScreen`]).
    Preferences,
}
/// What an options input leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsOutcome {
    /// Nothing changed (the input is swallowed: the window is modal).
    None,
    /// The page changed (Preferences or the quit confirmation opened).
    Page(OptionsPage),
    /// Close the options and open the save screen, or the load screen;
    /// its cancel returns to the menu.
    Save,
    Load,
    /// Close the window back to the map.
    Close,
    /// The quit confirmation's answer; the window closes either way.
    Quit(bool),
}
/// §3.17 the options menu, the pause window and the quit confirmation.
/// Their controls act on the press; the presentation layer plays the press
/// sound of the control under the pointer and its release sound on release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptionsWindow {
    pub page: OptionsPage,
}
impl OptionsWindow {
    pub fn new(page: OptionsPage) -> Self {
        Self { page }
    }
    /// The quit confirmation box.
    pub fn quit_box() -> super::flow::DialogBox {
        super::flow::DialogBox::new(QUIT_ORIGIN, true)
    }
    /// The page's buttons: the menu's five, the pause window's red button
    /// (as Done), or the quit box's Yes and No (as `ExitGame` and `Done`).
    /// The preferences have their own layout.
    pub fn layout(&self) -> Layout<OptionsAction> {
        match self.page {
            OptionsPage::Menu => options_layout(),
            OptionsPage::Pause => {
                Layout::new().with(OptionsAction::Done, pause_button(PAUSE_ORIGIN))
            }
            OptionsPage::Quit => Self::quit_box()
                .layout()
                .controls()
                .iter()
                .map(|(yes, rect)| {
                    let action = if *yes {
                        OptionsAction::ExitGame
                    } else {
                        OptionsAction::Done
                    };
                    (action, *rect)
                })
                .collect(),
            OptionsPage::Preferences => Layout::new(),
        }
    }
    /// A left press: a menu button acts at once; any press closes the
    /// pause window; a quit-box button answers.
    pub fn click(&mut self, point: [i32; 2]) -> OptionsOutcome {
        match self.page {
            OptionsPage::Menu => match options_layout().hit(point) {
                Some(action) => self.dispatch(action),
                None => OptionsOutcome::None,
            },
            OptionsPage::Pause => OptionsOutcome::Close,
            OptionsPage::Quit => match Self::quit_box().click(point) {
                Some(yes) => OptionsOutcome::Quit(yes),
                None => OptionsOutcome::None,
            },
            OptionsPage::Preferences => OptionsOutcome::None,
        }
    }
    /// A key: the menu's `options_key` (with whether `ib1p1xx1` plays
    /// first), any key closes the pause window, and the quit box's
    /// `dialog_answer`.
    pub fn key(&mut self, key: MenuKey) -> (OptionsOutcome, bool) {
        match self.page {
            OptionsPage::Menu => match options_key(key) {
                Some((action, sound)) => (self.dispatch(action), sound),
                None => (OptionsOutcome::None, false),
            },
            OptionsPage::Pause => (OptionsOutcome::Close, false),
            OptionsPage::Quit => {
                let answer = match key {
                    MenuKey::Enter => Some(true),
                    MenuKey::Escape => Some(false),
                    MenuKey::Char(c) => Self::quit_box().key(super::flow::DialogKey::Char(c)),
                    _ => None,
                };
                (
                    answer.map_or(OptionsOutcome::None, OptionsOutcome::Quit),
                    false,
                )
            }
            OptionsPage::Preferences => (OptionsOutcome::None, false),
        }
    }
    /// A menu action: Save and Load leave for the save screen,
    /// Preferences and Exit Game change the page, Done closes.
    pub fn dispatch(&mut self, action: OptionsAction) -> OptionsOutcome {
        match action {
            OptionsAction::SaveGame => OptionsOutcome::Save,
            OptionsAction::LoadGame => OptionsOutcome::Load,
            OptionsAction::Preferences => {
                self.page = OptionsPage::Preferences;
                OptionsOutcome::Page(self.page)
            }
            OptionsAction::ExitGame => {
                self.page = OptionsPage::Quit;
                OptionsOutcome::Page(self.page)
            }
            OptionsAction::Done => OptionsOutcome::Close,
        }
    }
}

/// §3.18 the preferences screen's red buttons: Default, Done and Cancel,
/// each with its caption's origin and `options.msg` ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferenceButton {
    Default,
    Done,
    Cancel,
}
pub const PREFERENCE_BUTTONS: [(PreferenceButton, [i32; 2], [i32; 2], i32); 3] = [
    (PreferenceButton::Default, [23, 450], [43, 449], 120),
    (PreferenceButton::Done, [148, 450], [169, 449], 4),
    (PreferenceButton::Cancel, [263, 450], [283, 449], 121),
];
/// §3.18 the player speed-up checkbox.
pub const CHECKBOX: Rect = Rect::new(383, 68, 18, 18);
/// One slider drag iteration lasts at least this long.
pub const DRAG_ITERATION_MS: u64 = 35;
/// A knob or switch's release cue follows its press cue after this long,
/// and the screen blocks meanwhile.
pub const CONTROL_CUE_MS: u64 = 70;
/// The effects slider samples its sound on the first and every 8th drag
/// iteration, the speech slider on the first and every 41st.
pub const EFFECTS_SAMPLE_EVERY: u32 = 8;
pub const SPEECH_SAMPLE_EVERY: u32 = 41;
/// The checkbox's cues (the safe rule's release cue).
pub const CHECKBOX_SOUNDS: (&str, &str) = ("ib2p1xx1", "ib2lu1x1");
/// A knob's and a switch's change cues.
pub const KNOB_SOUNDS: (&str, &str) = ("ib3p1xx1", "ib3lu1x1");
pub const SWITCH_SOUNDS: (&str, &str) = ("ib2p1xx1", "ib2lu1x1");

/// A control of the preferences screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferenceControl {
    Button(PreferenceButton),
    Knob(usize),
    Switch(usize),
    /// A slider by its caption order (0 combat speed, 1 text delay, 2-5
    /// the volumes, 6 brightness, 7 mouse sensitivity).
    Slider(usize),
    Checkbox,
}
/// The label widths (font 101) the knob and switch rectangles depend on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelWidths {
    pub knobs: [Vec<i32>; 5],
    pub switches: [[i32; 2]; 6],
}
impl LabelWidths {
    /// The widths of every label, from its `options.msg` ID.
    pub fn measure(width: impl Fn(i32) -> i32) -> Self {
        Self {
            knobs: std::array::from_fn(|k| KNOBS[k].labels.iter().map(|id| width(*id)).collect()),
            switches: std::array::from_fn(|s| SWITCHES[s].labels.map(&width)),
        }
    }
}
/// A slider's value with its range: (value, min, max).
pub fn slider_reading(values: &Preferences, index: usize) -> (f64, f64, f64) {
    match index {
        0 => (f64::from(values.combat_speed), 0.0, 50.0),
        1 => (values.text_base_delay, 1.0, 6.0),
        2 => (f64::from(values.master_volume), 0.0, 32767.0),
        3 => (f64::from(values.music_volume), 0.0, 32767.0),
        4 => (f64::from(values.sndfx_volume), 0.0, 32767.0),
        5 => (f64::from(values.speech_volume), 0.0, 32767.0),
        6 => (values.brightness, BRIGHTNESS_MIN, BRIGHTNESS_MAX),
        _ => (values.mouse_sensitivity, 1.0, 2.5),
    }
}
/// Store a slider's value (the line delay follows the base delay).
pub fn set_slider(values: &mut Preferences, index: usize, value: f64) {
    match index {
        0 => values.combat_speed = value as i32,
        1 => {
            values.text_base_delay = value.clamp(1.0, 6.0);
            values.text_line_delay = line_delay(values.text_base_delay);
        }
        2 => values.master_volume = value as i32,
        3 => values.music_volume = value as i32,
        4 => values.sndfx_volume = value as i32,
        5 => values.speech_volume = value as i32,
        6 => values.brightness = value.clamp(BRIGHTNESS_MIN, BRIGHTNESS_MAX),
        _ => values.mouse_sensitivity = value.clamp(1.0, 2.5),
    }
}
/// A slider knob's left edge for the stored value.
pub fn slider_knob(values: &Preferences, index: usize) -> i32 {
    let (value, min, max) = slider_reading(values, index);
    if index == 1 {
        delay_position(value)
    } else {
        slider_position(value, min, max)
    }
}
fn knob_value(values: &mut Preferences, index: usize) -> &mut i32 {
    match index {
        0 => &mut values.game_difficulty,
        1 => &mut values.combat_difficulty,
        2 => &mut values.violence_level,
        3 => &mut values.target_highlight,
        _ => &mut values.running_burning_guy,
    }
}
fn switch_value(values: &mut Preferences, index: usize) -> &mut i32 {
    match index {
        0 => &mut values.combat_messages,
        1 => &mut values.combat_taunts,
        2 => &mut values.language_filter,
        3 => &mut values.running,
        4 => &mut values.subtitles,
        _ => &mut values.item_highlight,
    }
}
/// A slider's modal drag loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliderDrag {
    pub index: usize,
    /// The knob's left edge.
    pub position: i32,
    /// Iterations run so far.
    pub iteration: u32,
    next: u64,
    released: bool,
}
/// One slider drag iteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragStep {
    /// The knob followed the pointer: the iteration's number (for the
    /// samples) and whether the value changed. Brightness and the volumes
    /// take effect now, the text delay is stored as it changes.
    Moved {
        index: usize,
        iteration: u32,
        moved: bool,
    },
    /// The release was seen: the drag ended (`ib1lu1x1`; mouse
    /// sensitivity takes effect now).
    Ended { index: usize },
}
/// What a preferences input leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferenceOutcome {
    /// Nothing (a busy screen swallows its input).
    None,
    /// A knob or switch is armed for its release.
    Armed(PreferenceControl),
    /// Default reset every value (they preview at once).
    Defaults,
    /// Close: keep the values (Done) or restore the backup (Cancel).
    Close { keep: bool },
    /// A slider drag began (`ib1p1xx1`).
    DragStarted(usize),
    /// The checkbox toggled (its cues are `CHECKBOX_SOUNDS`).
    Toggled,
    /// A knob or switch changed: play the press cue now, the release cue
    /// `CONTROL_CUE_MS` later; input is blocked until then.
    Changed { cues: (&'static str, &'static str) },
    /// F10, Ctrl+Q or Ctrl+X: the quit confirmation, whose Yes cancels.
    QuitConfirm,
}
/// A key on the preferences screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferenceKey {
    Escape,
    /// Enter or `p`: Done, after `ib1p1xx1`.
    Done,
    CtrlQ,
    CtrlX,
    F10,
}
/// §3.18 the preferences screen's state: the edited values, the backup
/// Cancel restores, the knobs' ping-pong directions, the armed knob or
/// switch, a slider drag and the cue block.
#[derive(Debug, Clone, PartialEq)]
pub struct PreferencesScreen {
    pub values: Preferences,
    pub backup: Preferences,
    pub changed: bool,
    /// The five knobs' ping-pong directions (true is up).
    pub knob_up: [bool; 5],
    armed: Option<PreferenceControl>,
    pub drag: Option<SliderDrag>,
    pub blocked_until: u64,
}
impl PreferencesScreen {
    pub fn open(values: Preferences) -> Self {
        Self {
            backup: values.clone(),
            values,
            changed: false,
            knob_up: [true; 5],
            armed: None,
            drag: None,
            blocked_until: 0,
        }
    }
    /// The controls in their press order: the red buttons, the knobs (the
    /// fifth left out while `covered`), the switches, the sliders and the
    /// checkbox.
    pub fn layout(&self, widths: &LabelWidths, covered: bool) -> Layout<PreferenceControl> {
        let buttons = PREFERENCE_BUTTONS
            .iter()
            .map(|(button, at, ..)| (PreferenceControl::Button(*button), super::red_button(*at)));
        let knobs = KNOBS
            .iter()
            .enumerate()
            .filter(|(index, _)| !(*index == 4 && covered))
            .map(|(index, knob)| {
                (
                    PreferenceControl::Knob(index),
                    knob.rect(&widths.knobs[index]),
                )
            });
        let switches = SWITCHES.iter().enumerate().map(|(index, switch)| {
            (
                PreferenceControl::Switch(index),
                switch.rect(widths.switches[index]),
            )
        });
        let sliders = SLIDER_KNOB_Y
            .iter()
            .enumerate()
            .map(|(index, ky)| (PreferenceControl::Slider(index), slider_rect(*ky)));
        buttons
            .chain(knobs)
            .chain(switches)
            .chain(sliders)
            .chain([(PreferenceControl::Checkbox, CHECKBOX)])
            .collect()
    }
    /// A drag runs or a change's cues are still playing: input is
    /// swallowed.
    pub fn busy(&self, now: u64) -> bool {
        self.drag.is_some() || now < self.blocked_until
    }
    /// The armed knob or switch.
    pub fn armed(&self) -> Option<PreferenceControl> {
        self.armed
    }
    /// A left press: a red button acts at once; a knob or switch arms for
    /// its release; a slider starts its drag (the first iteration on the
    /// next tick); the checkbox toggles.
    pub fn press(
        &mut self,
        point: [i32; 2],
        widths: &LabelWidths,
        covered: bool,
        now: u64,
    ) -> PreferenceOutcome {
        if self.busy(now) {
            return PreferenceOutcome::None;
        }
        match self.layout(widths, covered).hit(point) {
            Some(PreferenceControl::Button(button)) => self.button(button),
            Some(control @ (PreferenceControl::Knob(_) | PreferenceControl::Switch(_))) => {
                self.armed = Some(control);
                PreferenceOutcome::Armed(control)
            }
            Some(PreferenceControl::Slider(index)) => {
                self.armed = None;
                self.drag = Some(SliderDrag {
                    index,
                    position: slider_knob(&self.values, index),
                    iteration: 0,
                    next: now,
                    released: false,
                });
                PreferenceOutcome::DragStarted(index)
            }
            Some(PreferenceControl::Checkbox) => {
                self.armed = None;
                self.values.player_speedup ^= 1;
                self.changed = true;
                PreferenceOutcome::Toggled
            }
            None => {
                self.armed = None;
                PreferenceOutcome::None
            }
        }
    }
    /// A red button: Default resets every value (game difficulty
    /// included); Done and Cancel close.
    pub fn button(&mut self, button: PreferenceButton) -> PreferenceOutcome {
        match button {
            PreferenceButton::Default => {
                self.defaults();
                PreferenceOutcome::Defaults
            }
            PreferenceButton::Done => PreferenceOutcome::Close { keep: true },
            PreferenceButton::Cancel => PreferenceOutcome::Close { keep: false },
        }
    }
    /// §3.18 DEFAULT: the default values, the line delay derived from the
    /// base delay; marks the screen changed.
    pub fn defaults(&mut self) {
        let defaults = Preferences::default();
        self.values = Preferences {
            text_line_delay: line_delay(defaults.text_base_delay),
            ..defaults
        };
        self.changed = true;
    }
    /// A left release: a drag notes it for its next iteration; the armed
    /// knob or switch acts when the release is in its rectangle (any
    /// release there marks the screen changed, even in a dead zone).
    pub fn release(
        &mut self,
        point: Option<[i32; 2]>,
        widths: &LabelWidths,
        now: u64,
    ) -> PreferenceOutcome {
        if let Some(drag) = &mut self.drag {
            drag.released = true;
            return PreferenceOutcome::None;
        }
        if now < self.blocked_until {
            return PreferenceOutcome::None;
        }
        let (Some(control), Some(point)) = (self.armed.take(), point) else {
            return PreferenceOutcome::None;
        };
        let cues = match control {
            PreferenceControl::Knob(index) => {
                let knob = &KNOBS[index];
                let value = *knob_value(&mut self.values, index);
                let Some((new, up)) = knob_click(
                    knob,
                    &widths.knobs[index],
                    point,
                    value,
                    self.knob_up[index],
                ) else {
                    return PreferenceOutcome::None;
                };
                self.changed = true;
                self.knob_up[index] = up;
                if new == value {
                    return PreferenceOutcome::None;
                }
                *knob_value(&mut self.values, index) = new;
                KNOB_SOUNDS
            }
            PreferenceControl::Switch(index) => {
                let switch = &SWITCHES[index];
                let value = *switch_value(&mut self.values, index);
                let Some(new) = switch_click(switch, widths.switches[index], point, value) else {
                    return PreferenceOutcome::None;
                };
                self.changed = true;
                if new == value {
                    return PreferenceOutcome::None;
                }
                *switch_value(&mut self.values, index) = new;
                SWITCH_SOUNDS
            }
            _ => return PreferenceOutcome::None,
        };
        self.blocked_until = now + CONTROL_CUE_MS;
        PreferenceOutcome::Changed { cues }
    }
    /// One tick of a slider drag, at most one iteration per
    /// `DRAG_ITERATION_MS`. The release is tested first; otherwise the
    /// knob follows the pointer's x with `slider_drag`'s hysteresis and
    /// the value follows the knob.
    pub fn drag_tick(&mut self, now: u64, pointer_x: Option<i32>) -> Option<DragStep> {
        let drag = self.drag.as_mut()?;
        if now < drag.next {
            return None;
        }
        if drag.released {
            let index = drag.index;
            self.drag = None;
            return Some(DragStep::Ended { index });
        }
        drag.next = now + DRAG_ITERATION_MS;
        let iteration = drag.iteration;
        drag.iteration += 1;
        if let Some(x) = pointer_x {
            drag.position = slider_drag(drag.position, x);
        }
        let (index, position) = (drag.index, drag.position);
        let (old, min, max) = slider_reading(&self.values, index);
        let value = if index == 1 {
            delay_value(position)
        } else {
            slider_value(position, min, max)
        };
        set_slider(&mut self.values, index, value);
        let moved = slider_reading(&self.values, index).0 != old;
        self.changed |= moved;
        Some(DragStep::Moved {
            index,
            iteration,
            moved,
        })
    }
    /// A brightness hotkey inside the screen: the slider follows and the
    /// change joins the cancellable state.
    pub fn brightness(&mut self, value: f64) {
        self.values.brightness = value;
        self.changed = true;
    }
    /// A key (ignored while busy).
    pub fn key(&mut self, key: PreferenceKey, now: u64) -> PreferenceOutcome {
        if self.busy(now) {
            return PreferenceOutcome::None;
        }
        match key {
            PreferenceKey::Escape => PreferenceOutcome::Close { keep: false },
            PreferenceKey::Done => PreferenceOutcome::Close { keep: true },
            PreferenceKey::CtrlQ | PreferenceKey::CtrlX | PreferenceKey::F10 => {
                PreferenceOutcome::QuitConfirm
            }
        }
    }
    /// Closing: the values kept (Done) or the backup (Cancel), and whether
    /// they must be applied (a kept change, or any cancel).
    pub fn close(self, keep: bool) -> (Preferences, bool) {
        if keep {
            let apply = self.changed;
            (self.values, apply)
        } else {
            (self.backup, true)
        }
    }
}
