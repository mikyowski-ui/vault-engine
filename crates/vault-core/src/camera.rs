//! The `enhanced` preset's camera: commands that scripts give through the
//! enhanced opcodes `0x8210`-`0x8213`, and the region the camera center may
//! roam under the scroll blockers. This is engine extension, not part of
//! any specification; the Vanilla preset never produces or reads either.
//!
//! Camera state itself is presentation (the desktop's view), so commands
//! leave the world as [`crate::WorldEffect::Camera`] effects and never
//! change the simulation.

use crate::{WorldState, ui::cursor};
use std::collections::VecDeque;
use vault_formats::HexGrid;
use vault_vm::extensions::HostCall;

/// The scroll blocker's PID (SPEC-014 §3.29): an unowned one on a hex of
/// the current elevation refuses a view center there.
pub const SCROLL_BLOCKER_PID: u32 = 0x0500_000c;
/// The enhanced camera request words, `op_cam_pan_to` to `op_cam_reset`.
pub const CAMERA_WORDS: std::ops::RangeInclusive<u16> = 0x8210..=0x8213;

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
impl CameraCommand {
    /// The request a decoded camera word makes
    /// ([`vault_vm::extensions::decode_enhanced`], so `args[0]` is the last
    /// operand), or the diagnostic refusing it: a hex outside 0-39999, a
    /// negative duration, a zoom percent of 0 or less, or a negative shake
    /// intensity. A refused request changes nothing and the script goes on.
    pub fn from_call(call: &HostCall) -> Result<Self, String> {
        let name = call.spec.name;
        let duration = |index: usize| {
            u32::try_from(call.int(index))
                .map_err(|_| format!("{name}: negative duration {}", call.int(index)))
        };
        match call.spec.word {
            0x8210 => {
                let tile = call.int(1);
                if !(0..40000).contains(&tile) {
                    return Err(format!("{name}: hex {tile} outside 0-39999"));
                }
                Ok(Self::PanTo {
                    tile,
                    duration_ms: duration(0)?,
                })
            }
            0x8211 => {
                let percent = u32::try_from(call.int(1))
                    .ok()
                    .filter(|percent| *percent > 0)
                    .ok_or_else(|| format!("{name}: zoom {}% is not positive", call.int(1)))?;
                Ok(Self::SetZoom {
                    percent,
                    duration_ms: duration(0)?,
                })
            }
            0x8212 => {
                let intensity = u32::try_from(call.int(1))
                    .map_err(|_| format!("{name}: negative intensity {}", call.int(1)))?;
                Ok(Self::Shake {
                    intensity,
                    duration_ms: duration(0)?,
                })
            }
            0x8213 => Ok(Self::Reset {
                duration_ms: duration(0)?,
            }),
            word => Err(format!("{word:#06x} is not a camera request")),
        }
    }
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
    /// The outer region of `world`'s current elevation: a breadth-first
    /// fill from the player's hex (from [`WorldState::camera_hex`] while the
    /// player is not placed on that elevation) through the hexes that pass
    /// the map-dependent §3.29 scroll tests: inside the map border
    /// ([`cursor::inside_border`]) with no unowned [`SCROLL_BLOCKER_PID`]
    /// object on the hex on that elevation. Neighbours are visited in
    /// [`HexGrid::neighbor`] direction order 0-5, so the result depends on
    /// the world alone.
    ///
    /// The player-distance limit ([`cursor::distance_refused`]) is not
    /// applied: it moves with the player, and this is every hex the view
    /// may ever reach. When the start hex fails the tests (a player outside
    /// the border, say), the fill starts from the view center instead; when
    /// that fails too, the region is the start hex alone, so the view holds
    /// still rather than jumping.
    pub fn compute(world: &WorldState) -> Self {
        let elevation = world.elevation;
        let mut open: Vec<bool> = (0..40000).map(cursor::inside_border).collect();
        for (_, object) in world.iter() {
            if object.raw.pid == SCROLL_BLOCKER_PID
                && object.raw.elevation == elevation
                && object.owner.is_none()
                && let Some(hex) = object.raw.hex.and_then(|hex| usize::try_from(hex).ok())
                && let Some(flag) = open.get_mut(hex)
            {
                *flag = false;
            }
        }
        let passes = |hex: i32| usize::try_from(hex).is_ok_and(|hex| open.get(hex) == Some(&true));
        let player = world
            .get(world.player)
            .filter(|object| object.raw.elevation == elevation)
            .and_then(|object| object.raw.hex);
        let start = player.unwrap_or(world.camera_hex);
        let Some(seed) = [start, world.camera_hex]
            .into_iter()
            .find(|hex| passes(*hex))
        else {
            let mut allowed = vec![false; 40000];
            if let Some(flag) = usize::try_from(start)
                .ok()
                .and_then(|hex| allowed.get_mut(hex))
            {
                *flag = true;
            }
            return Self::from_allowed(allowed);
        };
        Self::from_allowed(flood(seed, passes))
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
/// The hexes reached from `seed` (which must pass) through hexes that
/// pass, breadth first in [`HexGrid::neighbor`] direction order: one flag
/// per hex 0-39999.
pub(crate) fn flood(seed: i32, passes: impl Fn(i32) -> bool) -> Vec<bool> {
    let mut reached = vec![false; 40000];
    let Some(flag) = usize::try_from(seed)
        .ok()
        .and_then(|hex| reached.get_mut(hex))
    else {
        return reached;
    };
    *flag = true;
    let mut queue = VecDeque::from([seed]);
    while let Some(hex) = queue.pop_front() {
        for direction in 0..6 {
            if let Ok(Some(next)) = HexGrid::neighbor(hex, direction)
                && !reached[next as usize]
                && passes(next)
            {
                reached[next as usize] = true;
                queue.push_back(next);
            }
        }
    }
    reached
}
