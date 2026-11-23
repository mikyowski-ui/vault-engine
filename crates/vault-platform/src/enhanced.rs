//! The `enhanced` preset's presentation settings and the frame and pointer
//! types the desktop shell and the interactive world exchange (the dual-
//! layer viewport, tactical camera and roof peeking). Engine extension, not
//! part of any specification: with the Vanilla preset none of it is used.

use crate::config::{self, Config, EnhancedProfile, Preset, UiScale};
use vault_render::IndexedImage;

/// `viewport_mode`: the fixed 640x480 frame, or the world unlocked across
/// the window behind a separately scaled interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewportMode {
    Fixed,
    #[default]
    Unlocked,
}

/// `roof_peek`: Vanilla roof rules only, a cutaway around the cursor, or no
/// roofs at all. Alt+R cycles them in play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RoofPeek {
    #[default]
    Vanilla,
    CursorXray,
    Hidden,
}

impl RoofPeek {
    /// The next mode in the Alt+R cycle.
    pub fn next(self) -> Self {
        match self {
            Self::Vanilla => Self::CursorXray,
            Self::CursorXray => Self::Hidden,
            Self::Hidden => Self::Vanilla,
        }
    }
}

/// The enhanced presentation settings (from `vault-engine.toml`, the
/// launcher or the command line).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnhancedSettings {
    pub viewport: ViewportMode,
    /// The zoom a map starts at and `0` resets to (0.5-3.0).
    pub default_zoom: f32,
    /// Interpolate camera moves (else they snap).
    pub smooth_camera: bool,
    pub roof_peek: RoofPeek,
    /// Surface pixels per interface pixel; `None` fits the window's height.
    pub ui_scale: Option<f32>,
    /// The touch UI's camera alone, under the Vanilla preset: the unlocked
    /// view pans and zooms (a drag, a pinch, the wheel), and none of the
    /// enhanced extras run: no roof peek (Alt, Alt+R, the PEEK button), no
    /// `-`/`=`/`0` zoom keys (they stay the brightness keys), no Shift
    /// item highlight and no Take All.
    pub navigation_only: bool,
    /// The tactical overlay a session starts with; `None` leaves it as it is.
    pub tactical_overlay: Option<bool>,
}

impl Default for EnhancedSettings {
    fn default() -> Self {
        Self {
            viewport: ViewportMode::Unlocked,
            default_zoom: 1.0,
            smooth_camera: true,
            roof_peek: RoofPeek::Vanilla,
            ui_scale: None,
            navigation_only: false,
            tactical_overlay: None,
        }
    }
}

/// The built-in presets a profile applies with one click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinPreset {
    VanillaPurist,
    WidescreenClassic,
    ModernTactical,
}

impl BuiltinPreset {
    pub const ALL: [Self; 3] = [
        Self::VanillaPurist,
        Self::WidescreenClassic,
        Self::ModernTactical,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::VanillaPurist => "1:1 Vanilla Purist",
            Self::WidescreenClassic => "Widescreen Classic",
            Self::ModernTactical => "Modern Tactical",
        }
    }

    /// What it sets. A key it leaves out stays with the top-level settings.
    pub fn settings(self) -> EnhancedProfile {
        match self {
            Self::VanillaPurist => EnhancedProfile {
                preset: Some(Preset::Vanilla),
                viewport_mode: Some(ViewportMode::Fixed),
                tactical_overlay: Some(false),
                ..EnhancedProfile::default()
            },
            Self::WidescreenClassic => EnhancedProfile {
                preset: Some(Preset::Enhanced),
                viewport_mode: Some(ViewportMode::Unlocked),
                default_zoom: Some(1.0),
                roof_peek: Some(RoofPeek::Vanilla),
                tactical_overlay: Some(false),
                ..EnhancedProfile::default()
            },
            Self::ModernTactical => EnhancedProfile {
                preset: Some(Preset::Enhanced),
                viewport_mode: Some(ViewportMode::Unlocked),
                roof_peek: Some(RoofPeek::CursorXray),
                tactical_overlay: Some(true),
                ..EnhancedProfile::default()
            },
        }
    }
}

/// The preset a profile's settings match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetMatch {
    Builtin(BuiltinPreset),
    /// By its place in `custom_presets`.
    Custom(usize),
}

/// A profile's Enhanced settings as a launch reads them: its own, over the
/// top-level ones, over the defaults. Nothing is left open.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActiveEnhanced {
    pub preset: Preset,
    pub settings: EnhancedSettings,
    pub game_speed: GameSpeed,
    pub tactical_overlay: bool,
}

impl ActiveEnhanced {
    /// `own`, a profile's table, under the top-level settings of `config`.
    pub fn new(config: &Config, own: EnhancedProfile) -> Self {
        let base = config.enhanced_settings();
        Self {
            preset: own.preset.or(config.preset).unwrap_or_default(),
            settings: EnhancedSettings {
                viewport: own.viewport_mode.unwrap_or(base.viewport),
                default_zoom: own.default_zoom.unwrap_or(base.default_zoom),
                smooth_camera: own.smooth_camera.unwrap_or(base.smooth_camera),
                roof_peek: own.roof_peek.unwrap_or(base.roof_peek),
                ui_scale: own.ui_scale.map_or(base.ui_scale, UiScale::factor),
                navigation_only: base.navigation_only,
                tactical_overlay: own.tactical_overlay,
            },
            game_speed: own.game_speed.unwrap_or(config.game_speed()),
            tactical_overlay: own.tactical_overlay.unwrap_or(false),
        }
    }

    /// Every key named, so that a saved preset does not move with the
    /// top-level settings.
    pub fn profile(self) -> EnhancedProfile {
        EnhancedProfile {
            preset: Some(self.preset),
            viewport_mode: Some(self.settings.viewport),
            default_zoom: Some(self.settings.default_zoom),
            smooth_camera: Some(self.settings.smooth_camera),
            roof_peek: Some(self.settings.roof_peek),
            ui_scale: Some(UiScale::from_factor(self.settings.ui_scale)),
            game_speed: Some(self.game_speed),
            tactical_overlay: Some(self.tactical_overlay),
        }
    }

    /// Whether `preset` names something and every key it names has this value.
    fn matches(&self, preset: &EnhancedProfile) -> bool {
        let settings = &self.settings;
        *preset != EnhancedProfile::default()
            && preset.preset.is_none_or(|value| value == self.preset)
            && preset
                .viewport_mode
                .is_none_or(|value| value == settings.viewport)
            && preset
                .default_zoom
                .is_none_or(|value| value == settings.default_zoom)
            && preset
                .smooth_camera
                .is_none_or(|value| value == settings.smooth_camera)
            && preset
                .roof_peek
                .is_none_or(|value| value == settings.roof_peek)
            && preset
                .ui_scale
                .is_none_or(|value| value.factor() == settings.ui_scale)
            && preset
                .game_speed
                .is_none_or(|value| value == self.game_speed)
            && preset
                .tactical_overlay
                .is_none_or(|value| value == self.tactical_overlay)
    }

    /// The preset these settings match: a custom one, which the player made
    /// and which names more, before a built-in one. The built-in presets
    /// differ from each other, so at most one matches.
    pub fn matching(&self, custom: &[config::CustomPreset]) -> Option<PresetMatch> {
        let saved = custom.iter().position(|it| self.matches(&it.settings));
        saved.map(PresetMatch::Custom).or_else(|| {
            BuiltinPreset::ALL
                .into_iter()
                .find(|preset| self.matches(&preset.settings()))
                .map(PresetMatch::Builtin)
        })
    }
}

/// One enhanced frame: the layers the shell draws, back to front.
pub struct EnhancedFrame {
    /// The world canvas at the layout's `world_size`; behind a window
    /// without the interface bar it is taller, down to the surface's
    /// bottom (`EnhancedLayout::full_canvas_height`), and drawn from the
    /// same corner.
    pub world: IndexedImage,
    /// The 640x480 interface frame.
    pub ui: IndexedImage,
    /// The interface frame's see-through index over the world (the map
    /// screen, and the map rows around a window centered over it), or
    /// `None` for an opaque full-screen window.
    pub ui_transparent: Option<u8>,
    /// The side-panel tile drawn left and right of the bar, if any.
    pub side_panel: Option<IndexedImage>,
    /// A full-screen window: the shell draws the procedural bezel
    /// (`vault_render::bezel`) around the frame in place of the world.
    pub bezel: bool,
    /// The pointer art and its hotspot, drawn at the pointer at interface
    /// scale (index 0 transparent).
    pub cursor: Option<(IndexedImage, [i32; 2])>,
}

/// Where the pointer is, in both layers' coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerAt {
    /// The interface frame pixel, inside the frame.
    pub ui: Option<[i32; 2]>,
    /// The world-canvas position, inside the world's area.
    pub world: Option<[f64; 2]>,
    /// Over a side panel.
    pub side: bool,
}

/// The pointer buttons the enhanced world takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerButton {
    Left,
    Right,
    Middle,
}

/// `game_speed`: how fast world time runs against the shell clock, under
/// the `enhanced` preset only (Alt+F cycles it in play). The world's
/// animations, movement, combat turns, scripts and game clock see world
/// time; the interface (holds, fades, counters, the camera), movies and
/// audio keep the shell clock. 1.0x is the specification's timing exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GameSpeed {
    #[default]
    Normal,
    Fast,
    Faster,
}

impl GameSpeed {
    pub const ALL: [Self; 3] = [Self::Normal, Self::Fast, Self::Faster];
    /// The multiplier: 1.0, 1.5 or 2.0.
    pub fn factor(self) -> f32 {
        match self {
            Self::Normal => 1.0,
            Self::Fast => 1.5,
            Self::Faster => 2.0,
        }
    }
    /// The multiplier in halves (2, 3 or 4), for exact integer time.
    pub fn halves(self) -> u64 {
        match self {
            Self::Normal => 2,
            Self::Fast => 3,
            Self::Faster => 4,
        }
    }
    /// The speed whose multiplier is exactly `factor`.
    pub fn from_factor(factor: f64) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|speed| f64::from(speed.factor()) == factor)
    }
    /// The next speed in the Alt+F cycle: 1.0, 1.5, 2.0, then 1.0 again.
    pub fn next(self) -> Self {
        match self {
            Self::Normal => Self::Fast,
            Self::Fast => Self::Faster,
            Self::Faster => Self::Normal,
        }
    }
    /// The speed as shown: `1.0x`, `1.5x` or `2.0x`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "1.0x",
            Self::Fast => "1.5x",
            Self::Faster => "2.0x",
        }
    }
}
