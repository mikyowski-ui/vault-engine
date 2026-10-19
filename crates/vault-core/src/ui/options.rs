//! SPEC-014 §2.3 and §3.17-3.18: preferences (defaults, ranges, the
//! `fallout.cfg` keys and the SPEC-008 save words), the options menu, the
//! pause window, the quit confirmation, and the preferences screen's knobs,
//! switches, sliders and brightness steps.
use super::Rect;
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
            if let Some(read) = config.get_real("preferences", key) {
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
