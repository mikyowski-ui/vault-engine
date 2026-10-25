//! The `enhanced` preset's tactical camera: the view center and zoom of the
//! unlocked world, following the player, panned by the pointer and keys,
//! zoomed about a world point, and driven by the scripted camera requests
//! (`vault_core::camera::CameraCommand`). Engine extension, not part of any
//! specification; the Vanilla preset never builds one.
//!
//! This is presentation state only: it never enters the world simulation.
//! Every update is a function of the logged shell clock passed to `tick`
//! (and of logged input), so a replay sees the same camera. The shake is a
//! hash of that clock, never the world's random numbers.
use vault_formats::HexGrid;

/// The zoom range (`EnhancedLayout::MIN_ZOOM` / `MAX_ZOOM`).
pub(crate) const MIN_ZOOM: f64 = 0.5;
pub(crate) const MAX_ZOOM: f64 = 3.0;
/// Edge and arrow-key panning, in world pixels per second at zoom 1 (scaled
/// by `1 / zoom`, so the screen speed stays the same).
pub(crate) const PAN_SPEED: f64 = 900.0;
/// One wheel line or zoom key multiplies the zoom by this.
pub(crate) const ZOOM_STEP: f64 = 1.15;
/// Exponential easing time constants, in milliseconds: the center toward
/// the player while following, toward a pan target, and back inside the
/// region (the soft clamp).
const FOLLOW_TAU_MS: f64 = 120.0;
const PAN_TAU_MS: f64 = 60.0;
const CLAMP_TAU_MS: f64 = 150.0;
/// A smooth user zoom (wheel or keys) runs over this long.
const ZOOM_MS: u64 = 150;
/// A followed player jumping farther than this in one tick (a map change,
/// a load or a teleport) is snapped to, not eased to.
const SNAP_DISTANCE: f64 = 640.0;

/// A hex's center in default-grid pixels, the camera's coordinates.
pub(crate) fn hex_point(hex: i32) -> Option<[f64; 2]> {
    let [x, y] = HexGrid::default().hex_center(hex).ok()?;
    Some([x as f64, y as f64])
}

/// The hex under a default-grid point.
pub(crate) fn point_hex(point: [f64; 2]) -> Option<i32> {
    let [x, y] = point.map(|value| value.round());
    if !(f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&x)
        || !(f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&y)
    {
        return None;
    }
    HexGrid::default()
        .pick_hex([x as i32, y as i32])
        .filter(|hex| (0..40000).contains(hex))
}

/// `t` in 0-1 eased in and out.
fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How far a timed animation from `start` over `duration` has come, 0-1.
fn progress(now: u64, start: u64, duration: u64) -> f64 {
    if duration == 0 {
        return 1.0;
    }
    (now.saturating_sub(start) as f64 / duration as f64).clamp(0.0, 1.0)
}

/// SplitMix64: the shake's deterministic noise.
fn mix(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

/// Where a timed pan ends.
#[derive(Debug, Clone, Copy, PartialEq)]
enum PanGoal {
    Point([f64; 2]),
    /// The player's drawn position as it moves (the reset).
    Player,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Pan {
    from: [f64; 2],
    goal: PanGoal,
    start: u64,
    duration: u64,
    /// Following resumes when it ends (the reset).
    then_follow: bool,
}

/// A zoom change over time. With an anchor the world point `anchor` keeps
/// its place on screen: the center is `anchor - (anchor - center0) * zoom0
/// / zoom` throughout.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Zoom {
    from: f64,
    to: f64,
    center0: [f64; 2],
    anchor: Option<[f64; 2]>,
    start: u64,
    duration: u64,
}
impl Zoom {
    fn center(&self, zoom: f64) -> Option<[f64; 2]> {
        let anchor = self.anchor?;
        let ratio = self.from / zoom;
        Some([0, 1].map(|axis| anchor[axis] - (anchor[axis] - self.center0[axis]) * ratio))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Shake {
    intensity: f64,
    start: u64,
    duration: u64,
}

/// The camera: where the view is centered, how far it is zoomed, and what
/// moves it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Camera {
    /// The view center, in default-grid pixels (a hex's center is its
    /// `HexGrid::default().hex_center`).
    pub center: [f64; 2],
    /// Where the center heads; the center eases toward it when smooth.
    target: [f64; 2],
    pub zoom: f64,
    /// The center tracks the player's drawn position.
    pub follow: bool,
    /// Interpolate moves (else they snap).
    pub smooth: bool,
    pub default_zoom: f64,
    pan: Option<Pan>,
    zoom_anim: Option<Zoom>,
    shake: Option<Shake>,
    /// The last tick's clock.
    last: Option<u64>,
    /// The player's position the last tick saw.
    player: Option<[f64; 2]>,
}

/// One tick's inputs.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TickInput {
    /// The player's drawn position (its hex center plus its displacement).
    pub player: Option<[f64; 2]>,
    /// Manual panning (edges, arrow keys) as unit directions; scaled by
    /// [`PAN_SPEED`] and `1 / zoom`.
    pub pan: [f64; 2],
    /// The center's soft bounds `[min_x, min_y, max_x, max_y]`, or `None`
    /// (the free camera).
    pub bounds: Option<[f64; 4]>,
}

impl Camera {
    pub fn new(center: [f64; 2], default_zoom: f64, smooth: bool) -> Self {
        let zoom = default_zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        Self {
            center,
            target: center,
            zoom,
            follow: true,
            smooth,
            default_zoom: zoom,
            pan: None,
            zoom_anim: None,
            shake: None,
            last: None,
            player: None,
        }
    }
    /// Jump to `center` at once, ending any move toward elsewhere.
    pub fn snap(&mut self, center: [f64; 2]) {
        self.center = center;
        self.target = center;
        self.pan = None;
        if let Some(zoom) = &mut self.zoom_anim {
            zoom.anchor = None;
        }
    }
    /// A manual pan by `delta` default-grid pixels (a pointer drag): it
    /// moves the view at once and stops following.
    pub fn drag(&mut self, delta: [f64; 2]) {
        self.follow = false;
        self.pan = None;
        self.zoom_anim = None;
        self.center = [self.center[0] + delta[0], self.center[1] + delta[1]];
        self.target = self.center;
    }
    /// Follow the player again (Home).
    pub fn resume_follow(&mut self) {
        self.follow = true;
        self.pan = None;
    }
    /// Pan to `point` over `duration` milliseconds (eased; 0 is at once),
    /// which stops following.
    pub fn pan_to(&mut self, point: [f64; 2], now: u64, duration: u64) {
        self.follow = false;
        self.pan = Some(Pan {
            from: self.center,
            goal: PanGoal::Point(point),
            start: now,
            duration,
            then_follow: false,
        });
        self.update_pan(now);
    }
    /// Zoom to `zoom` (clamped) over `duration` milliseconds, keeping the
    /// world point `anchor` in its place on screen (`None`: the center).
    /// An anchor away from the center stops following.
    pub fn zoom_to(&mut self, zoom: f64, anchor: Option<[f64; 2]>, now: u64, duration: u64) {
        let to = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let anchor = anchor.filter(|anchor| {
            (anchor[0] - self.center[0]).abs() > 0.5 || (anchor[1] - self.center[1]).abs() > 0.5
        });
        if anchor.is_some() {
            self.follow = false;
            self.pan = None;
        }
        self.zoom_anim = Some(Zoom {
            from: self.zoom,
            to,
            center0: self.center,
            anchor,
            start: now,
            duration,
        });
        self.update_zoom(now);
    }
    /// A user zoom (wheel or key) about `anchor`: smooth or at once.
    pub fn zoom_user(&mut self, zoom: f64, anchor: Option<[f64; 2]>, now: u64) {
        let duration = if self.smooth { ZOOM_MS } else { 0 };
        self.zoom_to(zoom, anchor, now, duration);
    }
    /// The zoom the camera is heading for: the running zoom's goal, else
    /// the current zoom (a user zoom step multiplies it).
    pub fn zoom_goal(&self) -> f64 {
        self.zoom_anim.map_or(self.zoom, |zoom| zoom.to)
    }
    /// Keep the zoom at `floor` at least (a layout that cannot show more).
    pub fn raise_zoom(&mut self, floor: f64) {
        if self.zoom < floor - 1e-6 {
            self.zoom = floor;
        }
        if let Some(zoom) = &mut self.zoom_anim {
            zoom.to = zoom.to.max(floor);
        }
    }
    /// Shake by up to `intensity` pixels, decaying over `duration`.
    pub fn shake(&mut self, intensity: f64, now: u64, duration: u64) {
        self.shake = (intensity > 0.0 && duration > 0).then_some(Shake {
            intensity,
            start: now,
            duration,
        });
    }
    /// Back to following the player at the default zoom with no shake,
    /// over `duration` milliseconds.
    pub fn reset(&mut self, now: u64, duration: u64) {
        self.shake = None;
        self.follow = false;
        self.pan = Some(Pan {
            from: self.center,
            goal: PanGoal::Player,
            start: now,
            duration,
            then_follow: true,
        });
        self.zoom_anim = Some(Zoom {
            from: self.zoom,
            to: self.default_zoom,
            center0: self.center,
            anchor: None,
            start: now,
            duration,
        });
        self.update_zoom(now);
        self.update_pan(now);
    }
    /// The shake's offset now, in default-grid pixels.
    pub fn shake_offset(&self, now: u64) -> [f64; 2] {
        let Some(shake) = self.shake else {
            return [0.0; 2];
        };
        let t = progress(now, shake.start, shake.duration);
        if t >= 1.0 {
            return [0.0; 2];
        }
        let amplitude = shake.intensity * (1.0 - t);
        // A new offset every 16 ms of presentation time.
        let step = now / 16;
        [0, 1].map(|axis| {
            let noise = mix(step.wrapping_mul(2).wrapping_add(axis));
            let unit = (noise >> 11) as f64 / (1u64 << 53) as f64;
            (unit * 2.0 - 1.0) * amplitude
        })
    }
    /// The drawn center: the center plus the shake.
    pub fn drawn_center(&self, now: u64) -> [f64; 2] {
        let shake = self.shake_offset(now);
        [self.center[0] + shake[0], self.center[1] + shake[1]]
    }
    /// Whether anything still moves on its own (an animation, a shake, or
    /// the center easing toward its target).
    pub fn moving(&self, now: u64) -> bool {
        self.pan.is_some()
            || self.zoom_anim.is_some()
            || self
                .shake
                .is_some_and(|shake| progress(now, shake.start, shake.duration) < 1.0)
            || (self.center[0] - self.target[0]).abs() > 0.01
            || (self.center[1] - self.target[1]).abs() > 0.01
    }
    fn update_pan(&mut self, now: u64) {
        let Some(pan) = self.pan else {
            return;
        };
        let goal = match pan.goal {
            PanGoal::Point(point) => Some(point),
            PanGoal::Player => self.player,
        };
        let Some(goal) = goal else {
            self.pan = None;
            self.follow |= pan.then_follow;
            return;
        };
        let s = smoothstep(progress(now, pan.start, pan.duration));
        let point = [0, 1].map(|axis| pan.from[axis] + (goal[axis] - pan.from[axis]) * s);
        self.center = point;
        self.target = point;
        if progress(now, pan.start, pan.duration) >= 1.0 {
            self.pan = None;
            if pan.then_follow {
                self.follow = true;
            }
        }
    }
    fn update_zoom(&mut self, now: u64) {
        let Some(zoom) = self.zoom_anim else {
            return;
        };
        let t = progress(now, zoom.start, zoom.duration);
        // Interpolated in log space, so each step feels the same.
        let value = (zoom.from.ln() + (zoom.to.ln() - zoom.from.ln()) * smoothstep(t)).exp();
        self.zoom = if t >= 1.0 { zoom.to } else { value };
        if let Some(center) = zoom.center(self.zoom) {
            self.center = center;
            self.target = center;
        }
        if t >= 1.0 {
            self.zoom_anim = None;
        }
    }
    /// One presentation tick at `now`. Returns whether the view changed.
    pub fn tick(&mut self, now: u64, input: TickInput) -> bool {
        let before = (self.center, self.zoom);
        let dt = self
            .last
            .map_or(0.0, |last| now.saturating_sub(last).min(250) as f64);
        self.last = Some(now);
        let jumped = match (self.player, input.player) {
            (Some(old), Some(new)) => {
                (old[0] - new[0]).abs() > SNAP_DISTANCE || (old[1] - new[1]).abs() > SNAP_DISTANCE
            }
            _ => false,
        };
        self.player = input.player;
        // Manual panning takes over from following and scripted pans.
        let pan = input.pan;
        let panning = pan[0] != 0.0 || pan[1] != 0.0;
        if panning {
            self.follow = false;
            self.pan = None;
        }
        self.update_pan(now);
        self.update_zoom(now);
        let animated = self.pan.is_some() || self.zoom_anim.is_some_and(|z| z.anchor.is_some());
        if self.follow
            && !animated
            && let Some(player) = self.player
        {
            self.target = player;
            if jumped || !self.smooth {
                self.center = player;
            }
        }
        if panning && !animated {
            let speed = PAN_SPEED / self.zoom * dt / 1000.0;
            let mut moved = [0, 1].map(|axis| self.target[axis] + pan[axis] * speed);
            // The soft clamp: panning reaches the bounds, and makes no
            // further outward motion once outside them.
            if let Some([x0, y0, x1, y1]) = input.bounds {
                let (low, high) = ([x0, y0], [x1, y1.max(y0)]);
                let high = [high[0].max(low[0]), high[1]];
                for axis in 0..2 {
                    let excess = |value: f64| (low[axis] - value).max(value - high[axis]).max(0.0);
                    if excess(moved[axis]) > excess(self.target[axis]) {
                        moved[axis] = if excess(self.target[axis]) > 0.0 {
                            self.target[axis]
                        } else {
                            moved[axis].clamp(low[axis], high[axis])
                        };
                    }
                }
            }
            self.target = moved;
        }
        // The soft clamp eases the target back inside the bounds (not
        // during a scripted pan, which goes where it was sent).
        if self.pan.is_none()
            && let Some([x0, y0, x1, y1]) = input.bounds
        {
            let keep = (-dt / CLAMP_TAU_MS).exp();
            let (low, high) = ([x0, y0], [x1, y1]);
            for axis in 0..2 {
                let value = self.target[axis];
                let inside = value.clamp(low[axis], high[axis].max(low[axis]));
                if value != inside {
                    let eased = inside + (value - inside) * keep;
                    self.target[axis] = if (eased - inside).abs() < 0.05 {
                        inside
                    } else {
                        eased
                    };
                }
            }
        }
        if !self.smooth || animated {
            if !animated {
                self.center = self.target;
            }
        } else {
            let tau = if self.follow {
                FOLLOW_TAU_MS
            } else {
                PAN_TAU_MS
            };
            let keep = (-dt / tau).exp();
            for axis in 0..2 {
                let gap = self.target[axis] - self.center[axis];
                self.center[axis] = if gap.abs() < 0.05 {
                    self.target[axis]
                } else {
                    self.target[axis] - gap * keep
                };
            }
        }
        if self
            .shake
            .is_some_and(|shake| progress(now, shake.start, shake.duration) >= 1.0)
        {
            self.shake = None;
            return true;
        }
        before != (self.center, self.zoom) || self.shake.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(player: [f64; 2]) -> TickInput {
        TickInput {
            player: Some(player),
            ..Default::default()
        }
    }

    #[test]
    fn following_eases_to_the_player_and_snaps_without_smoothing() {
        let mut camera = Camera::new([0.0, 0.0], 1.0, true);
        camera.tick(0, input([100.0, 50.0]));
        let mut now = 0;
        for _ in 0..200 {
            now += 16;
            camera.tick(now, input([100.0, 50.0]));
        }
        assert_eq!(camera.center, [100.0, 50.0]);
        let mut snapping = Camera::new([0.0, 0.0], 1.0, false);
        snapping.tick(0, input([100.0, 50.0]));
        assert_eq!(snapping.center, [100.0, 50.0]);
    }

    #[test]
    fn anchored_zoom_keeps_the_anchor_on_screen() {
        let mut camera = Camera::new([0.0, 0.0], 1.0, true);
        camera.follow = false;
        let anchor = [200.0, -100.0];
        camera.zoom_to(2.0, Some(anchor), 0, 100);
        for now in [0, 30, 60, 100] {
            camera.tick(now, TickInput::default());
            // The anchor's offset from the center, in screen units, stays.
            let screen = [0, 1].map(|axis| (anchor[axis] - camera.center[axis]) * camera.zoom);
            assert!((screen[0] - 200.0).abs() < 1e-6 && (screen[1] + 100.0).abs() < 1e-6);
        }
        assert_eq!(camera.zoom, 2.0);
    }

    #[test]
    fn the_shake_is_deterministic_and_decays() {
        let mut camera = Camera::new([0.0, 0.0], 1.0, true);
        camera.shake(6.0, 1000, 800);
        let first = camera.shake_offset(1100);
        assert_eq!(first, camera.clone().shake_offset(1100));
        assert!(first.iter().all(|value| value.abs() <= 6.0));
        assert!(first.iter().any(|value| *value != 0.0));
        assert_eq!(camera.shake_offset(1800), [0.0; 2]);
    }
}
