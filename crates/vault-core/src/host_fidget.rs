//! SPEC-007 §3.11.2 idle fidgets: outside combat, a background task now and
//! then plays the standing animation once for one standing critter on the
//! screen, drawing from the shared generator.
use crate::{
    ObjectRef, RandomSource, WorldError,
    assets::WorldAssets,
    choreography::Frames,
    host::WorldHost,
    host_choreography::Playback,
    motion::{Motion, MotionSequence, Play},
};
use vault_formats::{Fid, HexGrid, pro::BasicFlags};

/// The most candidates one run collects.
const CANDIDATES: usize = 100;
/// The screen a candidate's bounding rectangle must reach.
const SCREEN: [i64; 2] = [640, 480];
/// The map view the camera centers.
const VIEW: [u32; 2] = [640, 380];

/// The task's clock: when it last ran, and how long it waits.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Fidget {
    last_ms: u64,
    wait_ms: u64,
}

impl<A: WorldAssets> WorldHost<A> {
    /// The background task. It is removed in combat and added again, with
    /// no wait, once combat ends; it runs while a map is loaded and its
    /// background processing is on, and here only with real-time playback.
    /// It does nothing while the player is hidden. Once the time since its
    /// last run exceeds its wait it collects up to 100 visible, living,
    /// standing critters on the player's elevation whose bounding
    /// rectangles reach the 640 by 480 screen, in object order; draws
    /// `random(0, count - 1)` to pick one and plays its fidget; and then
    /// waits `random(0, 3000) + 1000 * k` ms, where k is `trunc(20 /
    /// count)`, or 7 without candidates, clamped to 1-7. Returns whether
    /// it ran.
    pub(crate) fn idle_fidget(&mut self, now_ms: u64) -> Result<bool, WorldError> {
        if self.world.combat
            || self.world.unloaded
            || !self.world.background_enabled
            || self.playback != Playback::RealTime
        {
            if self.world.combat {
                self.fidget = None;
            }
            return Ok(false);
        }
        let task = *self.fidget.get_or_insert(Fidget {
            last_ms: now_ms,
            wait_ms: 0,
        });
        let hidden = self
            .world
            .get(self.world.player)
            .is_none_or(|player| player.raw.flags.contains(BasicFlags::HIDDEN));
        if hidden || now_ms.saturating_sub(task.last_ms) <= task.wait_ms {
            return Ok(false);
        }
        let candidates = self.fidget_candidates();
        let count = candidates.len();
        if count > 0 {
            let pick = RandomSource::range(&mut self.world.rng, 0, count as i32 - 1)?;
            self.fidget_critter(candidates[usize::try_from(pick).unwrap_or(0)]);
        }
        let k = 20_usize.checked_div(count).map_or(7, |k| k.clamp(1, 7)) as u64;
        let wait = RandomSource::range(&mut self.world.rng, 0, 3000)?;
        self.fidget = Some(Fidget {
            last_ms: now_ms,
            wait_ms: u64::try_from(wait).unwrap_or(0) + 1000 * k,
        });
        Ok(true)
    }
    /// The task's candidates, in object order.
    fn fidget_candidates(&self) -> Vec<ObjectRef> {
        let Some(elevation) = self.world.get(self.world.player).map(|p| p.raw.elevation) else {
            return Vec::new();
        };
        let Ok(grid) = HexGrid::centered(self.world.camera_hex, VIEW[0], VIEW[1]) else {
            return Vec::new();
        };
        self.world
            .iter()
            .filter(|(reference, object)| {
                object.owner.is_none()
                    && object.raw.elevation == elevation
                    && !object.raw.flags.contains(BasicFlags::HIDDEN)
                    && self.world.critter_art(*reference)
                    && Fid(object.raw.art_id).animation() == 0
                    && !self.world.is_dead(*reference)
                    && self.on_screen(grid, *reference)
            })
            .map(|(reference, _)| reference)
            .take(CANDIDATES)
            .collect()
    }
    /// Whether an object's bounding rectangle (its shown frame at its hex,
    /// shift and displacement) reaches the screen.
    fn on_screen(&self, grid: HexGrid, object: ObjectRef) -> bool {
        let Some(found) = self.world.get(object) else {
            return false;
        };
        let (Some(hex), Ok(art)) = (found.raw.hex, self.assets.object_art(found.raw.art_id)) else {
            return false;
        };
        let direction = usize::from(found.raw.orientation % 6);
        let (Some(frame), Ok(center)) = (
            art.frame(direction, found.raw.frame)
                .or_else(|| art.frame(0, found.raw.frame)),
            grid.hex_center(hex),
        ) else {
            return false;
        };
        let anchor =
            center.map(|value| value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32);
        let rect = frame.world_rect(anchor, art.shifts[direction], found.raw.displacement);
        rect.right >= 0 && rect.left < SCREEN[0] && rect.bottom >= 0 && rect.top < SCREEN[1]
    }
    /// A fidget: an unreserved, insignificant sequence playing animation 0
    /// once, after the character sound for animation 0 when the critter is
    /// the player, or its art stem starts with `m` and it stands closer to
    /// the player than twice the player's Perception.
    fn fidget_critter(&mut self, critter: ObjectRef) {
        let player = self.world.player;
        let speaks = critter == player || {
            let stem = self
                .world
                .get(critter)
                .and_then(|object| self.assets.art_token(1, Fid(object.raw.art_id).base()));
            stem.is_some_and(|stem| matches!(stem.first(), Some(b'm' | b'M')))
                && self.world.distance(critter, player) < 2 * self.world.player_stat(1)
        };
        let art = self.critter_fid(critter, 0, None, self.facing(critter));
        if !self.assets.art_exists(art) {
            return;
        }
        let mut sequence = MotionSequence {
            insignificant: true,
            ..MotionSequence::new()
        };
        let mut built = Ok(());
        if speaks {
            let name = self.character_sound_name(critter, 0, 0).unwrap_or_default();
            built = sequence.push(critter, 0, Motion::Sound { name });
        }
        let play = Motion::Play {
            animation: 0,
            play: Play::Forward,
            art,
            frames: Frames::default(),
        };
        if built.and_then(|()| sequence.push(critter, 0, play)).is_ok() {
            self.register_motion(sequence);
        }
    }
}
