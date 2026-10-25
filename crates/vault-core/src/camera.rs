//! The `enhanced` preset's camera: commands that scripts give through the
//! enhanced opcodes `0x8210`-`0x8213`, and the region the camera center may
//! roam under the scroll blockers. This is engine extension, not part of
//! any specification; the Vanilla preset never produces or reads either.
//!
//! Camera state itself is presentation (the desktop's view), so commands
//! leave the world as [`crate::WorldEffect::Camera`] effects and never
//! change the simulation.

use vault_formats::HexGrid;

/// A scripted camera request (enhanced opcodes `0x8210`-`0x8213`). Durations
/// are presentation milliseconds; 0 applies at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraCommand {
    /// `0x8210 op_cam_pan_to(tile_num, duration_ms)`: move the view center
    /// to the center of `tile` (0-39999).
    PanTo { tile: i32, duration_ms: u32 },
    /// `0x8211 op_cam_set_zoom(zoom_pct, duration_ms)`: zoom to `percent`
    /// (100 is 1.0x), clamped by the view to its 50-300 range.
    SetZoom { percent: u32, duration_ms: u32 },
    /// `0x8212 op_cam_shake(intensity, duration_ms)`: shake the view by up
    /// to `intensity` pixels, decaying over the duration.
    Shake { intensity: u32, duration_ms: u32 },
    /// `0x8213 op_cam_reset(duration_ms)`: back to following the player at
    /// the configured zoom, with no shake.
    Reset { duration_ms: u32 },
}

/// The hexes the camera center may rest on, on one elevation: the hexes the
/// Vanilla scroll tests would ever let the view reach from the player (no
/// scroll blocker, inside the map border), and their pixel bounds in the
/// default grid's coordinates ([`HexGrid::hex_reference`] of a default grid).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraRegion {
    allowed: Vec<bool>,
    /// The allowed hexes' references: `[min_x, min_y, max_x, max_y]`.
    pub bounds: [i64; 4],
}

impl CameraRegion {
    /// Every hex of the 200-by-200 grid (the debug free camera).
    pub fn unbounded() -> Self {
        Self::from_allowed(vec![true; 40000])
    }
    /// The region from `allowed`, one flag per hex 0-39999.
    pub fn from_allowed(allowed: Vec<bool>) -> Self {
        let grid = HexGrid::default();
        let mut bounds = [i64::MAX, i64::MAX, i64::MIN, i64::MIN];
        for (hex, _) in allowed.iter().enumerate().filter(|(_, ok)| **ok) {
            if let Ok([x, y]) = grid.hex_reference(hex as i32) {
                bounds = [
                    bounds[0].min(x),
                    bounds[1].min(y),
                    bounds[2].max(x),
                    bounds[3].max(y),
                ];
            }
        }
        if bounds[0] > bounds[2] {
            bounds = [0; 4];
        }
        Self { allowed, bounds }
    }
    /// Whether the camera center may rest on `hex`.
    pub fn contains(&self, hex: i32) -> bool {
        usize::try_from(hex)
            .ok()
            .and_then(|hex| self.allowed.get(hex))
            .copied()
            .unwrap_or(false)
    }
    /// The per-hex flags, for drawing the shroud.
    pub fn allowed(&self) -> &[bool] {
        &self.allowed
    }
    /// `point` (default-grid pixels) moved inside [`Self::bounds`].
    pub fn clamp(&self, point: [f64; 2]) -> [f64; 2] {
        let [x0, y0, x1, y1] = self.bounds.map(|value| value as f64);
        [point[0].clamp(x0, x1), point[1].clamp(y0, y1)]
    }
}
