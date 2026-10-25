//! The `enhanced` preset's presentation settings and the frame and pointer
//! types the desktop shell and the interactive world exchange (the dual-
//! layer viewport, tactical camera and roof peeking). Engine extension, not
//! part of any specification: with the Vanilla preset none of it is used.

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
}

impl Default for EnhancedSettings {
    fn default() -> Self {
        Self {
            viewport: ViewportMode::Unlocked,
            default_zoom: 1.0,
            smooth_camera: true,
            roof_peek: RoofPeek::Vanilla,
            ui_scale: None,
        }
    }
}

/// One enhanced frame: the layers the shell draws, back to front.
pub struct EnhancedFrame {
    /// The world canvas at the layout's `world_size`.
    pub world: IndexedImage,
    /// The 640x480 interface frame.
    pub ui: IndexedImage,
    /// The interface frame's see-through index over the world (the map
    /// screen), or `None` for an opaque full-screen window.
    pub ui_transparent: Option<u8>,
    /// The side-panel tile drawn left and right of the bar, if any.
    pub side_panel: Option<IndexedImage>,
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
