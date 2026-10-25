//! The interactive world's `enhanced` preset presentation: the camera, the
//! dual-layer frame and the pointer in world and interface coordinates.
//! With the Vanilla preset (`set_enhanced(None)`, the default) every method
//! answers `None`/`false` and the shell keeps the Vanilla path.
// Scaffold: removed once the shell and the world use every item.
#![allow(dead_code)]

use super::MapPreview;
use crate::AppResult;
use crate::enhanced::{EnhancedFrame, EnhancedSettings, PointerAt, PointerButton};
use vault_render::EnhancedLayout;

impl MapPreview {
    /// Select the enhanced presentation (`None` is Vanilla).
    pub fn set_enhanced(&mut self, settings: Option<EnhancedSettings>) {
        let _ = settings;
    }
    /// The enhanced presentation settings, when it is selected.
    pub fn enhanced(&self) -> Option<EnhancedSettings> {
        None
    }
    /// The world zoom the shell lays the next frame out with.
    pub fn enhanced_zoom(&self) -> f32 {
        1.0
    }
    /// The enhanced frame for `layout`, or `None` to show the Vanilla
    /// frame (Vanilla preset, or the fixed viewport).
    pub fn enhanced_frame(&mut self, layout: &EnhancedLayout) -> Option<AppResult<EnhancedFrame>> {
        let _ = layout;
        None
    }
    /// The pointer moved; returns whether the frame changed.
    pub fn enhanced_pointer(&mut self, at: PointerAt) -> bool {
        let _ = at;
        false
    }
    /// A pointer button; returns whether the frame changed.
    pub fn enhanced_button(&mut self, at: PointerAt, button: PointerButton, pressed: bool) -> bool {
        let _ = (at, button, pressed);
        false
    }
    /// Wheel lines (positive away from the user); returns whether the
    /// frame changed.
    pub fn enhanced_wheel(&mut self, at: PointerAt, lines: f64) -> bool {
        let _ = (at, lines);
        false
    }
}
