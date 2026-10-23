//! SPEC-007 §3.11 non-combat animation in the world: the world's own
//! sequences and their playback, the walk and run step model (§3.11.1),
//! the interaction sequences (§3.11.3), and doors and containers
//! (§3.11.4).
//!
//! Unlike an attack's show, these sequences act on world objects as their
//! entries start and step: a walker changes hexes as it crosses them, a
//! door blocks until its animation ends, and a pickup happens on the reach
//! animation's action frame.
use crate::{
    LootRequest, ObjectAction, ObjectRef, ScriptEvent, WorldEffect, WorldError,
    assets::WorldAssets,
    choreography::{Begin, Driver, Entry, Frames, Refused, Stepped},
    clicks::{Click, ClickState, Policy, busy_policy},
    critical::{effect, hit_mode},
    host::{DispatchResult, WorldHost},
    host_choreography::Playback,
    host_drugs::Arg,
    motion::{Call, Goal, HEX_STEPS, Motion, MotionSequence, Play, Walk, crosses},
    routing::{rotation, step},
    stats::Perk,
    world::ScriptRequest,
};
use vault_formats::{
    Fid, FrmArchive,
    pro::{ContainerFlags, ItemData, LockFlags, PrototypeData, SceneryData},
};

/// Animation codes (SPEC-007 §2.2).
mod animation {
    pub const STAND: u8 = 0;
    pub const WALK: u8 = 1;
    pub const CLIMB: u8 = 4;
    pub const GROUND_REACH: u8 = 10;
    pub const MIDDLE_REACH: u8 = 11;
    pub const RUN: u8 = 19;
    pub const FALL_BACK: u8 = 20;
    pub const PRONE_UP: u8 = 36;
    pub const BACK_UP: u8 = 37;
    pub const TAKE_OUT: u8 = 38;
    pub const PUT_AWAY: u8 = 39;
}
/// SPEC-010 §3.8 open/close name actions.
mod sound {
    pub const OPEN: u8 = 0;
    pub const CLOSE: u8 = 1;
    pub const LOCKED: u8 = 2;
}

/// §3.11.4 the open-door flag group: movement-nonblocking `0x10`,
/// light-through `0x20000000` and projectile-through `0x80000000`.
const OPEN_DOOR_FLAGS: u32 = 0xA000_0010;
/// SPEC-006 §3.8 the combat interaction check's cost.
const INTERACTION_AP: i32 = 3;
/// §3.11.1 an interaction approach runs out of combat from this object
/// distance; a talk approach from [`TALK_RUN_DISTANCE`].
pub(crate) const RUN_DISTANCE: i32 = 5;
pub(crate) const TALK_RUN_DISTANCE: i32 = 9;
/// Item PID 213, the Remains of Gizmo: no container use, always looted.
const GIZMO_REMAINS: u32 = 213;
/// §3.11.3 scenery prototype extended flag `0x01`: the ground reach.
const SCENERY_GROUND_REACH: u32 = 0x01;

/// A sequence being played (or halted), as its callbacks see it: which
/// objects it holds under §3.10.1, whether something cancelled it
/// meanwhile, and the entries a callback registered into it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Running {
    id: u64,
    /// The owners of its non-callback entries.
    owners: Vec<ObjectRef>,
    insignificant: bool,
    /// Cancelled from inside its own playback, or being halted: its
    /// remaining entries do nothing, but its forced ones.
    cancelled: bool,
    /// Entries a callback registered as it started, to follow it
    /// (§3.11.4 the container use).
    appends: Vec<(ObjectRef, i32, Motion)>,
}
impl Running {
    fn of(sequence: &MotionSequence, cancelled: bool) -> Self {
        Self {
            id: sequence.id,
            owners: sequence.owned(),
            insignificant: sequence.insignificant,
            cancelled,
            appends: Vec::new(),
        }
    }
}

impl<A: WorldAssets> WorldHost<A> {
    /// Register a world sequence (§3.10.1): it is examined once now, and
    /// instant playback plays it to its end at once. Returns whether it
    /// registered.
    ///
    /// "One sequence per owner": an object that has a non-callback entry,
    /// started or not, in a sequence that has not ended cannot own an
    /// entry of a new one. The new sequence is then discarded whole, with
    /// a diagnostic; the request is dropped, not queued. When the other
    /// sequence is an idle fidget (§3.11.2, an insignificant sequence) it
    /// is ended at once instead and registration continues. Callback
    /// entries are not checked, so a sequence of callbacks always
    /// registers. Attack shows are never involved: they are not world
    /// sequences.
    pub(crate) fn register_motion(&mut self, mut sequence: MotionSequence) -> bool {
        let owners = sequence.owned();
        let held = owners.iter().copied().find(|owner| {
            self.running_motions.iter().any(|running| {
                !running.cancelled && !running.insignificant && running.owners.contains(owner)
            }) || self
                .motions
                .iter()
                .any(|playing| !playing.insignificant && playing.owns(*owner))
        });
        if let Some(owner) = held {
            self.world.diagnostic(format!(
                "animation sequence discarded: object {owner} is in a sequence that has not ended"
            ));
            return false;
        }
        for running in &mut self.running_motions {
            if running.insignificant && owners.iter().any(|owner| running.owners.contains(owner)) {
                running.cancelled = true;
            }
        }
        let (ended, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.motions)
            .into_iter()
            .partition(|playing| {
                playing.insignificant && owners.iter().any(|owner| playing.owns(*owner))
            });
        self.motions = kept;
        for playing in ended {
            self.halt_motion(playing);
        }
        self.next_motion += 1;
        sequence.id = self.next_motion;
        sequence.combat = self.world.combat;
        let now = self.motion_time.max(self.world.elapsed_ms);
        self.running_motions.push(Running::of(&sequence, false));
        match self.playback {
            Playback::Instant => {
                sequence.schedule.run_to_end(now, self);
            }
            Playback::RealTime => sequence.schedule.register(now, self),
        }
        self.settle_running(&mut sequence, now);
        if sequence.schedule.finished() {
            self.end_motion(&sequence);
        } else {
            self.motions.push(sequence);
        }
        self.sync_combat_lock();
        true
    }
    /// SPEC-014 §3.27 whether the player's turn is under way for the combat
    /// lock: from its start (before the combat script and any stand-up)
    /// until it ends.
    pub(crate) fn lock_turn(&self) -> bool {
        self.combat_lock.turn || self.world.combat_state.player_turn
    }
    /// SPEC-014 §3.27 recount the running sequences registered in combat:
    /// the world's sequences and the attack shows queued to play.
    pub(crate) fn sync_combat_lock(&mut self) {
        let count = self.motions.iter().filter(|m| m.combat).count()
            + self.world.shows.iter().filter(|show| show.combat).count();
        let turn = self.lock_turn();
        self.combat_lock.counted(count, turn);
    }
    /// After a sequence's registration or pass: a sequence cancelled
    /// meanwhile is halted while its record is still the running one, so
    /// its forced entries run under the halt; then the record leaves.
    fn settle_running(&mut self, sequence: &mut MotionSequence, now_ms: u64) {
        if self.halted() {
            sequence.schedule.halt(now_ms, self);
        }
        self.running_motions.pop();
    }
    /// Play the world's sequences up to `now_ms`: one scheduler pass at
    /// each due frame step in turn, so a late update still keeps every
    /// entry's timing. Returns whether anything stepped.
    pub(crate) fn step_motions(&mut self, now_ms: u64) -> bool {
        let mut changed = false;
        for _ in 0..10_000 {
            let Some(due) = self
                .motions
                .iter()
                .filter_map(|sequence| sequence.schedule.due_ms())
                .min()
                .filter(|due| *due <= now_ms)
            else {
                break;
            };
            self.motion_time = due;
            self.motion_pass(due);
            changed = true;
        }
        self.motion_time = now_ms;
        self.sync_combat_lock();
        changed
    }
    /// One scheduler pass over the sequences registered before it, in
    /// registration order. A sequence registered during the pass was
    /// examined at once; its first steps come in the next pass at the same
    /// time.
    fn motion_pass(&mut self, now_ms: u64) {
        let ids: Vec<u64> = self.motions.iter().map(|sequence| sequence.id).collect();
        for id in ids {
            let Some(index) = self.motions.iter().position(|sequence| sequence.id == id) else {
                continue;
            };
            if self.motions[index]
                .schedule
                .due_ms()
                .is_none_or(|due| due > now_ms)
            {
                continue;
            }
            let placeholder = MotionSequence {
                id,
                combat: self.motions[index].combat,
                ..MotionSequence::default()
            };
            let mut sequence = std::mem::replace(&mut self.motions[index], placeholder);
            self.running_motions.push(Running::of(&sequence, false));
            sequence.schedule.pass(now_ms, self);
            self.settle_running(&mut sequence, now_ms);
            let slot = self.motions.iter().position(|sequence| sequence.id == id);
            if sequence.schedule.finished() {
                if let Some(slot) = slot {
                    self.motions.remove(slot);
                }
                self.end_motion(&sequence);
            } else if let Some(slot) = slot {
                self.motions[slot] = sequence;
            }
        }
    }
    /// §3.10.1 a sequence's end: unless it was registered without the
    /// reset, each critter owner that is neither prone nor dead stands in
    /// its pose for its current weapon code.
    fn end_motion(&mut self, sequence: &MotionSequence) {
        if !sequence.reset {
            return;
        }
        for owner in sequence.owners() {
            self.stand_still(owner);
        }
    }
    /// A critter's standing pose for its current weapon code and facing.
    fn stand_still(&mut self, critter: ObjectRef) {
        if !self.world.critter_art(critter)
            || self.world.is_prone(critter)
            || self.world.is_dead(critter)
        {
            return;
        }
        let orientation = self.facing(critter);
        let art = self.critter_fid(critter, animation::STAND, None, orientation);
        if let Some(object) = self.world.get_mut(critter) {
            object.raw.art_id = art;
            object.raw.frame = 0;
            object.raw.displacement = [0; 2];
        }
    }
    /// Stop every world sequence as a failing entry would (§3.10.1): forced
    /// callbacks, such as a door's finishing callback, run at once, the
    /// rest are dropped, and critters return to standing.
    pub(crate) fn stop_motions(&mut self) {
        for sequence in std::mem::take(&mut self.motions) {
            self.halt_motion(sequence);
        }
    }
    /// End a sequence taken out of the world's list, the §3.10.1 way: its
    /// forced entries run under the halt (so a forced adjacency check
    /// reports nothing, §3.11.3), the rest are dropped, and its critters
    /// return to standing.
    fn halt_motion(&mut self, mut sequence: MotionSequence) {
        let now = self.motion_time.max(self.world.elapsed_ms);
        self.running_motions.push(Running::of(&sequence, true));
        sequence.schedule.halt(now, self);
        self.running_motions.pop();
        self.end_motion(&sequence);
        self.sync_combat_lock();
    }
    /// §3.11.5 getting up at once: animation 37 after a fall back (20), 36
    /// otherwise, then the standing pose; without that art, the standing
    /// pose at once.
    pub(crate) fn stand_up(&mut self, critter: ObjectRef) {
        let current = self
            .world
            .get(critter)
            .map_or(0, |object| Fid(object.raw.art_id).animation());
        let up = if current == animation::FALL_BACK {
            animation::BACK_UP
        } else {
            animation::PRONE_UP
        };
        let art = self.critter_fid(critter, up, None, self.facing(critter));
        if self.world.critter_art(critter) && self.assets.art_exists(art) {
            let mut sequence = MotionSequence::new();
            let play = Motion::Play {
                animation: up,
                play: Play::Forward,
                art,
                frames: Frames::default(),
            };
            if sequence.push(critter, 0, play).is_ok() {
                self.register_motion(sequence);
                return;
            }
        }
        let standing = self.critter_fid(critter, animation::STAND, None, self.facing(critter));
        if let Some(object) = self.world.get_mut(critter) {
            object.raw.art_id = standing;
            object.raw.frame = 0;
            object.raw.displacement = [0; 2];
        }
    }
    /// §3.10.1 cancelling an object's sequence (SPEC-014 §3.27, SPEC-005
    /// `reg_anim_func` command 2): the first sequence that has not ended
    /// and holds a non-callback entry of `object`'s ends. Its unstarted
    /// entries are dropped except the forced ones, which run at once, and
    /// its critters return to standing. One being played now stops where
    /// it stands, running its forced entries as its pass ends. Attack
    /// shows are prioritized and never cancelled this way.
    pub(crate) fn cancel_motions(&mut self, object: ObjectRef) {
        let listed = self
            .motions
            .iter()
            .position(|sequence| sequence.owns(object));
        let running = self
            .running_motions
            .iter()
            .position(|running| !running.cancelled && running.owners.contains(&object));
        let first_listed = match (listed, running) {
            (Some(index), Some(playing)) => {
                self.motions[index].id < self.running_motions[playing].id
            }
            (Some(_), None) => true,
            _ => false,
        };
        if first_listed {
            let sequence = self.motions.remove(listed.unwrap_or_default());
            self.halt_motion(sequence);
        } else if let Some(playing) = running {
            self.running_motions[playing].cancelled = true;
        }
    }
    /// The sequence being played was cancelled, or is being halted: its
    /// entries do nothing, but its forced ones.
    fn halted(&self) -> bool {
        self.running_motions
            .last()
            .is_some_and(|running| running.cancelled)
    }
    /// Whether a world sequence still has an entry of `object`'s to run.
    pub fn moving(&self, object: ObjectRef) -> bool {
        self.motions.iter().any(|sequence| sequence.names(object))
    }
    /// §3.10.1 whether a sequence that has not ended holds `object`: a
    /// non-callback entry of its, started or not, in a sequence other than
    /// an idle fidget (which a new sequence ends). While this holds, no
    /// new sequence can own an entry of the object's (`register_motion`),
    /// and the player is busy for SPEC-014 §3.27.
    pub fn owns_motion(&self, object: ObjectRef) -> bool {
        self.motions
            .iter()
            .any(|sequence| !sequence.insignificant && sequence.owns(object))
            || self.running_motions.iter().any(|running| {
                !running.cancelled && !running.insignificant && running.owners.contains(&object)
            })
    }
    /// SPEC-014 §3.27 what a player's click finds: combat and whose turn it
    /// is, whether the combat lock is engaged by running sequences, whether
    /// a sequence holds the player, and the player's current pose by its
    /// art (walk 1 or run 19; standing 0).
    pub fn click_state(&self) -> ClickState {
        let player = self.world.player;
        let animation = self
            .world
            .get(player)
            .map(|object| Fid(object.raw.art_id).animation());
        ClickState {
            combat: self.world.combat,
            player_turn: self.lock_turn(),
            engaged: self.combat_lock.engaged,
            busy: self.owns_motion(player),
            walking: matches!(animation, Some(animation::WALK | animation::RUN)),
            standing: animation == Some(animation::STAND),
        }
    }
    /// SPEC-014 §3.27 whether the player's talk with `target` is a near
    /// talk: out of combat, at object distance below 9 with an unblocked
    /// line (SPEC-007 §3.11.1), so it has no approach entry and registers
    /// even while the player is busy (`Click::NearTalk`).
    pub fn near_talk(&self, target: ObjectRef) -> bool {
        let player = self.world.player;
        !self.world.combat
            && self.world.object_distance(player, target) < TALK_RUN_DISTANCE
            && !self.world.shot(player, target).hard_obstruction
    }
    pub(crate) fn facing(&self, object: ObjectRef) -> u8 {
        self.world
            .get(object)
            .map_or(0, |object| object.raw.orientation)
    }
    fn place(&self, object: ObjectRef) -> Option<(i32, u8)> {
        let found = self.world.get(object)?;
        Some((found.raw.hex?, found.raw.elevation))
    }
    /// The frame-step interval of `fid` at its own rate (stored 0 is 10
    /// fps) plus `speed`, or 100 ms without the art.
    fn art_interval(&self, fid: u32, speed: i32) -> u64 {
        let fps = self
            .assets
            .object_art(fid)
            .map_or(10, |art| i32::from(art.effective_fps()))
            .saturating_add(speed);
        1000 / u64::from(if fps > 0 { fps.unsigned_abs() } else { 10 })
    }
    /// A frame's per-frame displacement in `direction` (SPEC-001 §3.5),
    /// falling back to the art's first direction.
    fn shift(art: &FrmArchive, direction: u8, frame: usize) -> [i32; 2] {
        art.frame(usize::from(direction), frame)
            .or_else(|| art.frame(0, frame))
            .map_or([0; 2], |frame| frame.displacement.map(i32::from))
    }

    // §3.11.1 walking and running.

    /// A run request walks instead with a crippled leg, for a sneaking
    /// player without Silent Running, or without art for running.
    pub(crate) fn walk_animation(&self, mover: ObjectRef, run: bool) -> u8 {
        if !run {
            return animation::WALK;
        }
        let crippled = self
            .world
            .critter_state(mover)
            .is_some_and(|state| state.results.0 & effect::CRIPPLED_LEGS != 0);
        let sneaking = mover == self.world.player
            && self.sneaking()
            && self.world.perk(Perk::SilentRunning) == 0;
        let art = self.critter_fid(mover, animation::RUN, None, self.facing(mover));
        if crippled || sneaking || !self.assets.art_exists(art) {
            animation::WALK
        } else {
            animation::RUN
        }
    }
    /// A walk entry starts: it plans its route (§3.1). Without a route it
    /// fails, unless the mover is already where it was going: on the hex,
    /// or for an object, a one-step route shortened to nothing (the mover
    /// on the target's hex or a neighbor of it) moves nothing.
    pub(crate) fn start_walk(&mut self, mover: ObjectRef, walk: &mut Walk) -> Begin {
        let planned = match walk.goal {
            Goal::Hex(hex) => self.plan_to_tile(mover, hex, walk.limit),
            Goal::Object(target) => self.plan_to_object(mover, target).map(|mut movement| {
                movement.directions.truncate(walk.limit);
                movement
            }),
        };
        let Some(movement) = planned else {
            return Begin::Failed;
        };
        if movement.directions.is_empty() {
            let there = match walk.goal {
                Goal::Hex(hex) => self.place(mover).is_some_and(|(at, _)| at == hex),
                Goal::Object(target) => self.world.distance(mover, target) <= 1,
            };
            return if there { Begin::Done } else { Begin::Failed };
        }
        walk.route = movement.directions;
        walk.cursor = 0;
        walk.destination = movement.destination;
        walk.animation = self.walk_animation(mover, walk.run);
        let speed = if walk.animation == animation::WALK {
            self.walk_speed(mover)
        } else {
            0
        };
        let fid = self.critter_fid(mover, walk.animation, None, walk.route[0]);
        walk.interval_ms = self.art_interval(fid, speed);
        Begin::Timed
    }
    /// One frame step of a walk (§3.11.1). The first step re-places the
    /// mover on its hex at frame 0, facing the route; later steps advance
    /// a frame, wrapping. The shown frame's displacement accumulates until
    /// it reaches the next hex's pixel step: then the mover enters that
    /// hex with the residual (or replans around a blocker, or uses a door
    /// it may open). Safe rule: without walk art, or with art whose frames
    /// never move in the facing direction, every step enters the next hex.
    pub(crate) fn walk_step(
        &mut self,
        mover: ObjectRef,
        walk: &mut Walk,
        step_number: usize,
    ) -> Stepped {
        let Some((hex, elevation)) = self.place(mover) else {
            return Stepped::Done;
        };
        // The main loop stops every animation for a pending transition.
        let leaving = mover == self.world.player && self.world.pending_transition.is_some();
        if walk.cursor >= walk.route.len() || leaving {
            self.stand_on_hex(mover);
            return Stepped::Done;
        }
        let facing = walk.route[walk.cursor];
        let fid = self.critter_fid(mover, walk.animation, None, facing);
        let art = self.assets.object_art(fid).ok().filter(|art| {
            (0..art.frame_count()).any(|frame| Self::shift(art, facing, frame) != [0; 2])
        });
        walk.frame = match &art {
            Some(art) if step_number > 1 => (walk.frame + 1) % art.frame_count().max(1),
            _ => 0,
        };
        let shift = art
            .as_deref()
            .map_or([0; 2], |art| Self::shift(art, facing, walk.frame));
        let Some(object) = self.world.get_mut(mover) else {
            return Stepped::Done;
        };
        if step_number == 1 {
            object.raw.displacement = [0; 2];
        }
        object.raw.art_id = fid;
        object.raw.frame = walk.frame;
        object.raw.orientation = facing;
        let displacement = [
            object.raw.displacement[0] + shift[0],
            object.raw.displacement[1] + shift[1],
        ];
        if art.is_some() && !crosses(displacement, facing) {
            object.raw.displacement = displacement;
            return Stepped::After(walk.interval_ms);
        }
        let pixel = HEX_STEPS[usize::from(facing % 6)];
        let residual = if art.is_some() {
            [displacement[0] - pixel[0], displacement[1] - pixel[1]]
        } else {
            [0; 2]
        };
        let next = step(hex, facing);
        if let Some(blocker) = self.world.movement_blocker(next, elevation, mover) {
            if self.world.may_auto_open(mover, blocker) {
                if let Err(error) = self.use_door(mover, blocker) {
                    self.world.diagnostic(error.to_string());
                }
            } else {
                let rebuilt = self.world.route(mover, hex, walk.destination, true);
                let Some(&first) = rebuilt.first() else {
                    self.stand_on_hex(mover);
                    return Stepped::Done;
                };
                walk.route = rebuilt;
                walk.cursor = 0;
                walk.frame = 0;
                let fid = self.critter_fid(mover, walk.animation, None, first);
                if let Some(object) = self.world.get_mut(mover) {
                    object.raw.displacement = [0; 2];
                    object.raw.frame = 0;
                    object.raw.orientation = first;
                    object.raw.art_id = fid;
                }
                return Stepped::After(walk.interval_ms);
            }
        }
        if let Some(object) = self.world.get_mut(mover) {
            object.raw.displacement = [0; 2];
        }
        if let Err(error) = self.world.move_object(mover, next, elevation, true) {
            self.world.diagnostic(error.to_string());
            return Stepped::Done;
        }
        self.charge_movement(mover);
        walk.cursor += 1;
        let free = if mover == self.world.player {
            self.world.free_movement
        } else {
            0
        };
        let spent = self.world.combat && self.world.action_points(mover) + free <= 0;
        if walk.cursor >= walk.route.len() || spent {
            return Stepped::Done;
        }
        let facing = walk.route[walk.cursor];
        let fid = self.critter_fid(mover, walk.animation, None, facing);
        if let Some(object) = self.world.get_mut(mover) {
            object.raw.orientation = facing;
            object.raw.art_id = fid;
            object.raw.displacement = residual;
        }
        Stepped::After(walk.interval_ms)
    }
    /// The walk ends: the last residual is dropped.
    fn stand_on_hex(&mut self, mover: ObjectRef) {
        if let Some(object) = self.world.get_mut(mover) {
            object.raw.displacement = [0; 2];
        }
    }

    // Entries.

    /// A turn toward a hex or an object: the facing and, for a critter,
    /// the standing pose for its current weapon code, at once.
    fn turn_toward(&mut self, owner: ObjectRef, toward: Goal) {
        let hex = match toward {
            Goal::Hex(hex) => Some(hex),
            Goal::Object(target) => self.place(target).map(|(hex, _)| hex),
        };
        let Some(orientation) = self
            .place(owner)
            .zip(hex)
            .and_then(|((from, _), to)| rotation(from, to))
        else {
            return;
        };
        let art = self
            .world
            .critter_art(owner)
            .then(|| self.critter_fid(owner, animation::STAND, None, orientation));
        if let Some(object) = self.world.get_mut(owner) {
            object.raw.orientation = orientation;
            if let Some(art) = art {
                object.raw.art_id = art;
                object.raw.frame = 0;
                object.raw.displacement = [0; 2];
            }
        }
    }
    /// One frame step of a critter animation: step k shows frame k - 1,
    /// adding its displacement; the step after the last frame finishes.
    fn animate_step(
        &mut self,
        owner: ObjectRef,
        art: u32,
        frames: &Frames,
        step_number: usize,
    ) -> Stepped {
        let Some(object) = self.world.get_mut(owner) else {
            return Stepped::Done;
        };
        match frames.shifts.get(step_number - 1) {
            Some(shift) => {
                object.raw.art_id = art;
                object.raw.frame = step_number - 1;
                object.raw.displacement[0] += shift[0];
                object.raw.displacement[1] += shift[1];
                Stepped::After(frames.interval_ms)
            }
            None => {
                // Art that could not be loaded still takes effect.
                if frames.shifts.is_empty() {
                    object.raw.art_id = art;
                    object.raw.frame = 0;
                }
                Stepped::Done
            }
        }
    }
    /// One frame step of a script animation (SPEC-005). Forward, step k
    /// shows frame k - 1, adding its displacement, and a critter's death
    /// animation (20-35) ends on its static art 28 higher. In reverse, step
    /// k shows the k-th frame from the last, leaving each later frame's
    /// displacement. Repeating, the frames cycle without end.
    fn play_step(
        &mut self,
        owner: ObjectRef,
        animation: u8,
        play: Play,
        art: u32,
        frames: &Frames,
        step_number: usize,
    ) -> Stepped {
        let count = frames.shifts.len();
        let still = (play == Play::Forward
            && step_number > count
            && self.world.critter_art(owner)
            && (20..=35).contains(&animation))
        .then(|| self.critter_fid(owner, animation + 28, None, self.facing(owner)));
        let Some(object) = self.world.get_mut(owner) else {
            return Stepped::Done;
        };
        object.raw.art_id = art;
        match play {
            Play::Forward => {
                let Some(shift) = frames.shifts.get(step_number - 1) else {
                    object.raw.frame = object.raw.frame.min(count.saturating_sub(1));
                    if let Some(still) = still {
                        object.raw.art_id = still;
                        object.raw.frame = 0;
                    }
                    return Stepped::Done;
                };
                object.raw.frame = step_number - 1;
                object.raw.displacement[0] += shift[0];
                object.raw.displacement[1] += shift[1];
            }
            Play::Reverse => {
                if step_number > count {
                    object.raw.frame = 0;
                    return Stepped::Done;
                }
                let frame = count - step_number;
                if let Some(left) = frames.shifts.get(frame + 1).filter(|_| step_number > 1) {
                    object.raw.displacement[0] -= left[0];
                    object.raw.displacement[1] -= left[1];
                }
                object.raw.frame = frame;
            }
            Play::Forever => object.raw.frame = (step_number - 1) % count.max(1),
        }
        Stepped::After(frames.interval_ms)
    }
    /// The frame count of an object's own art; without the art, two
    /// frames stand for closed and open.
    fn frame_count(&self, object: ObjectRef) -> (usize, Option<std::sync::Arc<FrmArchive>>) {
        let art = self
            .world
            .get(object)
            .and_then(|found| self.assets.object_art(found.raw.art_id).ok());
        let count = art.as_ref().map_or(2, |art| art.frame_count().max(1));
        (count, art)
    }
    /// §3.11.4 one frame step of a door's or container's animation 0: the
    /// first step shows the current frame; each later one enters the next
    /// frame forward (adding its displacement) or leaves the current one in
    /// reverse (subtracting it), until the last frame or frame 0.
    fn swing_step(
        &mut self,
        object: ObjectRef,
        forward: bool,
        interval_ms: u64,
        step_number: usize,
    ) -> Stepped {
        if step_number == 1 {
            return Stepped::After(interval_ms);
        }
        let (count, art) = self.frame_count(object);
        let Some(found) = self.world.get_mut(object) else {
            return Stepped::Done;
        };
        let direction = found.raw.orientation;
        let frame = found.raw.frame;
        let shift = |frame: usize| {
            art.as_deref()
                .map_or([0; 2], |art| Self::shift(art, direction, frame))
        };
        if forward {
            if frame + 1 >= count {
                return Stepped::Done;
            }
            let [x, y] = shift(frame + 1);
            found.raw.frame = frame + 1;
            found.raw.displacement[0] += x;
            found.raw.displacement[1] += y;
        } else {
            if frame == 0 {
                return Stepped::Done;
            }
            let [x, y] = shift(frame);
            found.raw.frame = frame - 1;
            found.raw.displacement[0] -= x;
            found.raw.displacement[1] -= y;
        }
        Stepped::After(interval_ms)
    }
    /// §3.11.4 the finishing callback. Closed (open bit clear): the
    /// open-door flags clear and the object returns to frame 0, subtracting
    /// the displacements of the current frame down to frame 1 under the
    /// safe rule. Open: the flags are set and the object goes to its last
    /// frame, adding each later frame's displacement. Lighting follows.
    fn finish_swing(&mut self, object: ObjectRef, open: bool) {
        let (count, art) = self.frame_count(object);
        let Some(found) = self.world.get_mut(object) else {
            return;
        };
        let open = match found.raw.state {
            vault_formats::map::ObjectState::Other {
                detail: vault_formats::map::ObjectDetail::Door(flags),
                ..
            } => flags.contains(LockFlags::OPEN),
            _ => open,
        };
        let direction = found.raw.orientation;
        let shift = |frame: usize| {
            art.as_deref()
                .map_or([0; 2], |art| Self::shift(art, direction, frame))
        };
        let frame = found.raw.frame;
        if open {
            found.raw.flags.0 |= OPEN_DOOR_FLAGS;
            for later in frame + 1..count {
                let [x, y] = shift(later);
                found.raw.displacement[0] += x;
                found.raw.displacement[1] += y;
            }
            found.raw.frame = count - 1;
        } else {
            found.raw.flags.0 &= !OPEN_DOOR_FLAGS;
            for current in (1..=frame).rev() {
                let [x, y] = shift(current);
                found.raw.displacement[0] -= x;
                found.raw.displacement[1] -= y;
            }
            found.raw.frame = 0;
        }
        self.world.rebuild_lighting();
    }
    /// The object distance between two placed objects on one elevation is
    /// at most 1.
    fn adjacent(&self, a: ObjectRef, b: ObjectRef) -> bool {
        self.place(a)
            .zip(self.place(b))
            .is_some_and(|((_, x), (_, y))| x == y)
            && self.world.object_distance(a, b) <= 1
    }
    /// A callback entry.
    fn call(&mut self, owner: ObjectRef, call: Call) -> Result<Begin, WorldError> {
        let player = owner == self.world.player;
        match call {
            Call::Adjacent { target } => {
                if self.adjacent(owner, target) {
                    return Ok(Begin::Done);
                }
                // SPEC-007 §3.11.3 adjacency check, safe rule: a player
                // actor whose approach failed is told `misc.msg` 2000. Not
                // when an external stop (combat starting, leaving the map)
                // or a cancel (SPEC-014 §3.27) runs this forced check: the
                // approach did not fail, and the reference prints nothing.
                if player && !self.halted() {
                    self.print_game("misc.msg", 2000, &[]);
                }
                return Ok(Begin::Failed);
            }
            Call::CombatPoints => {
                if !self.world.combat {
                    return Ok(Begin::Done);
                }
                if self.world.action_points(owner) < INTERACTION_AP {
                    if player {
                        self.print_game("proto.msg", 700, &[]);
                    }
                    return Ok(Begin::Failed);
                }
                if let Some(state) = self.world.critter_state_mut(owner) {
                    state.ap -= INTERACTION_AP;
                }
            }
            Call::Pickup { item } => self.pick_up(owner, item)?,
            Call::Use { target } => self.use_object(owner, target)?,
            Call::UseOn { item, target } => self.use_item_stage(owner, item, target)?,
            Call::UseContainer { container } => self.use_container(owner, container)?,
            Call::Loot { target } => self.post_loot(owner, target),
            Call::Arrive { target, action } => {
                self.arriving = true;
                let result = self.action(owner, target, action);
                self.arriving = false;
                if let Err(error) = result {
                    self.world.diagnostic(error.to_string());
                }
            }
            Call::SkillStage { target, skill } => self.skill_stage(owner, target, skill)?,
            Call::OpenBit { open } => {
                if let Some(object) = self.world.get_mut(owner)
                    && let vault_formats::map::ObjectState::Other {
                        detail: vault_formats::map::ObjectDetail::Door(flags),
                        ..
                    } = &mut object.raw.state
                {
                    if open {
                        flags.0 |= LockFlags::OPEN.0;
                    } else {
                        flags.0 &= !LockFlags::OPEN.0;
                    }
                }
            }
            Call::Finish { open } => self.finish_swing(owner, open),
        }
        Ok(Begin::Done)
    }

    // §3.11.3 the interaction sequences.

    /// A sequence the actor starts. For the player, SPEC-014 §3.27 "Clicks
    /// while the player is busy" decides first ([`busy_policy`]): nothing
    /// is queued, the click either cancels the player's current sequence
    /// and starts its own, or does nothing and shows no message. Any actor
    /// then registers under §3.10.1. Returns whether the sequence
    /// registered.
    fn start_interaction(
        &mut self,
        actor: ObjectRef,
        sequence: MotionSequence,
        click: Click,
        interrupt_walk: bool,
    ) -> Result<bool, WorldError> {
        if actor == self.world.player {
            match busy_policy(click, &self.click_state(), interrupt_walk) {
                Policy::Ignore => return Ok(false),
                Policy::Cancel => self.cancel_motions(actor),
                Policy::Proceed => {}
            }
        }
        Ok(self.register_motion(sequence))
    }
    /// The route limit: the actor's action points in combat.
    fn move_limit(&self, actor: ObjectRef) -> usize {
        if self.world.combat {
            usize::try_from(self.world.action_points(actor)).unwrap_or(0)
        } else {
            usize::MAX
        }
    }
    /// The approach to `target` (§3.11.1): a walk, or a run out of combat
    /// at object distance `run_at` or more, limited in combat to the action
    /// points. Then a turn toward the target, delay -1. §3.1: the move
    /// entry is registered by route, not by distance; a one-step route
    /// (the mover already adjacent) moves nothing, and only a mover on the
    /// target's own hex and elevation registers no move entry at all.
    fn approach_entries(
        &self,
        sequence: &mut MotionSequence,
        actor: ObjectRef,
        target: ObjectRef,
        run_at: i32,
    ) -> Result<(), Refused> {
        let distance = self.world.object_distance(actor, target);
        let same_hex = self.place(actor).is_some() && self.place(actor) == self.place(target);
        if !same_hex {
            let run = !self.world.combat && distance >= run_at;
            let walk = Walk::new(Goal::Object(target), run, self.move_limit(actor));
            sequence.push(actor, 0, Motion::Walk(Box::new(walk)))?;
        }
        sequence.push(
            actor,
            -1,
            Motion::Turn {
                toward: Goal::Object(target),
            },
        )
    }
    /// An animation of `critter` whose art must exist (§3.10.1).
    fn animate_entry(
        &self,
        sequence: &mut MotionSequence,
        critter: ObjectRef,
        animation: u8,
        weapon: Option<u8>,
        delay: i32,
    ) -> Result<(), Refused> {
        let art = self.critter_fid(critter, animation, weapon, self.facing(critter));
        if !self.assets.art_exists(art) {
            return Err(Refused::Art(art));
        }
        sequence.push(
            critter,
            delay,
            Motion::Animate {
                animation,
                weapon,
                art,
                frames: Frames::default(),
            },
        )
    }
    /// The weapon code a critter's art shows (0 unarmed).
    fn weapon_code(&self, critter: ObjectRef) -> u8 {
        self.world
            .get(critter)
            .filter(|_| self.world.critter_art(critter))
            .map_or(0, |object| Fid(object.raw.art_id).weapon())
    }
    /// Putting away the held weapon: its sound (-1), then animation 39.
    fn put_away(
        &self,
        sequence: &mut MotionSequence,
        critter: ObjectRef,
        delay: i32,
    ) -> Result<(), Refused> {
        let name = self
            .character_sound_name(critter, animation::PUT_AWAY, 0)
            .unwrap_or_default();
        sequence.push(critter, -1, Motion::Sound { name })?;
        self.animate_entry(sequence, critter, animation::PUT_AWAY, None, delay)
    }
    /// Taking the weapon out again (animation 38 with its code), -1.
    fn take_out(
        &self,
        sequence: &mut MotionSequence,
        critter: ObjectRef,
        code: u8,
    ) -> Result<(), Refused> {
        self.animate_entry(sequence, critter, animation::TAKE_OUT, Some(code), -1)
    }
    /// §3.11.3 the reach: the ground reach (10) for a prone critter,
    /// scenery with extended flag `0x01`, or a container whose interaction
    /// word has bit 0 set; the middle reach (11) otherwise.
    fn reach(&self, target: ObjectRef) -> u8 {
        let Some(object) = self.world.get(target) else {
            return animation::MIDDLE_REACH;
        };
        let ground = match object.prototype.as_deref().map(|prototype| &prototype.data) {
            Some(PrototypeData::Critter(_)) => self.world.is_prone(target),
            Some(PrototypeData::Scenery(_)) => object
                .prototype
                .as_ref()
                .is_some_and(|prototype| prototype.extended.0 & SCENERY_GROUND_REACH != 0),
            Some(PrototypeData::Item(item)) => matches!(
                item.data,
                ItemData::Container { flags, .. } if flags.contains(ContainerFlags::GROUND_REACH)
            ),
            _ => false,
        };
        if ground {
            animation::GROUND_REACH
        } else {
            animation::MIDDLE_REACH
        }
    }
    /// The action frame A of a critter's reach art for its current weapon
    /// code, or for `weapon` (the container branch looks up with code 0,
    /// the weapon being put away by then; the art is the same, §2.2), or
    /// -1 when that art cannot be loaded.
    fn reach_frame(&self, critter: ObjectRef, reach: u8, weapon: Option<u8>) -> i32 {
        let art = self.critter_fid(critter, reach, weapon, self.facing(critter));
        self.assets
            .object_art(art)
            .map_or(-1, |art| i32::from(art.action_frame))
    }
    /// The checks every interaction makes after its approach: adjacency
    /// (forced, so a failed approach is still reported) and the combat AP.
    fn checks(
        &self,
        sequence: &mut MotionSequence,
        actor: ObjectRef,
        target: ObjectRef,
    ) -> Result<(), Refused> {
        sequence.push_forced(actor, -1, Motion::Call(Call::Adjacent { target }))?;
        sequence.push(actor, -1, Motion::Call(Call::CombatPoints))
    }
    /// §3.11.3 picking up an item: the approach and checks, the ground
    /// reach, then the item's sound and the pickup, each delay A.
    pub(crate) fn pickup_motion(
        &self,
        actor: ObjectRef,
        item: ObjectRef,
    ) -> Result<MotionSequence, Refused> {
        let mut sequence = MotionSequence::new();
        self.approach_entries(&mut sequence, actor, item, RUN_DISTANCE)?;
        self.checks(&mut sequence, actor, item)?;
        self.animate_entry(&mut sequence, actor, animation::GROUND_REACH, None, 0)?;
        let a = self.reach_frame(actor, animation::GROUND_REACH, None);
        let name = self.item_sound(item);
        sequence.push(actor, a, Motion::Sound { name })?;
        sequence.push(actor, a, Motion::Call(Call::Pickup { item }))?;
        Ok(sequence)
    }
    pub(crate) fn pickup_sequence(
        &mut self,
        actor: ObjectRef,
        item: ObjectRef,
    ) -> Result<(), WorldError> {
        let sequence = self.pickup_motion(actor, item).map_err(refused)?;
        self.start_interaction(actor, sequence, Click::Action, true)?;
        Ok(())
    }
    /// The pickup sound: the item's art stem (its list row without the
    /// extension), uppercased.
    fn item_sound(&self, item: ObjectRef) -> Vec<u8> {
        let Some(art) = self.world.get(item).map(|object| Fid(object.raw.art_id)) else {
            return Vec::new();
        };
        self.assets
            .art_token(art.category(), art.base())
            .map(|name| {
                let stem = name
                    .iter()
                    .rposition(|byte| *byte == b'.')
                    .map_or(name.as_slice(), |dot| &name[..dot]);
                stem.to_ascii_uppercase()
            })
            .unwrap_or_default()
    }
    /// §3.11.3 using a container that cannot be picked up: the approach
    /// and checks, putting away a held weapon, the reach, the container
    /// use (none for PID 213), taking the weapon out again, and the loot
    /// request when the container was closed now or is PID 213.
    ///
    /// Container reach timing (§3.11.3, SPEC-013 §3.16 step 3): the
    /// `[Must-match]` reference gives the container use delay -1 whenever
    /// the reach art loads, because its art test is inverted. This takes
    /// the `[May-improve]` rule instead: the delay is the action frame A of
    /// the reach art looked up with weapon code 0, so the container opens
    /// on the reach's action frame, and -1 only when that art does not
    /// load. The take-out and the loot request keep delay -1 under either
    /// rule, and the container's sound and animation join this sequence
    /// as the use runs (`use_container`), so the loot window opens only
    /// after the reach and the container animation have both finished.
    pub(crate) fn container_motion(
        &self,
        actor: ObjectRef,
        container: ObjectRef,
    ) -> Result<MotionSequence, Refused> {
        let (pid, closed) = self
            .world
            .get(container)
            .map_or((0, false), |object| (object.raw.pid, object.raw.frame == 0));
        let code = self.weapon_code(actor);
        let reach = self.reach(container);
        let mut sequence = MotionSequence::new();
        self.approach_entries(&mut sequence, actor, container, RUN_DISTANCE)?;
        self.checks(&mut sequence, actor, container)?;
        if code != 0 {
            self.put_away(&mut sequence, actor, -1)?;
        }
        self.animate_entry(&mut sequence, actor, reach, None, 0)?;
        if pid != GIZMO_REMAINS {
            let delay = self.reach_frame(actor, reach, Some(0));
            let call = Call::UseContainer { container };
            sequence.push(actor, delay, Motion::Call(call))?;
        }
        if code != 0 {
            self.take_out(&mut sequence, actor, code)?;
        }
        if closed || pid == GIZMO_REMAINS {
            let call = Call::Loot { target: container };
            sequence.push(actor, -1, Motion::Call(call))?;
        }
        Ok(sequence)
    }
    pub(crate) fn container_sequence(
        &mut self,
        actor: ObjectRef,
        container: ObjectRef,
    ) -> Result<(), WorldError> {
        let sequence = self.container_motion(actor, container).map_err(refused)?;
        self.start_interaction(actor, sequence, Click::Action, true)?;
        Ok(())
    }
    /// §3.11.3 using scenery or an object, or an item on one: the approach
    /// and checks, putting away a held weapon, the reach, the use, and
    /// taking the weapon out again.
    pub(crate) fn use_motion(
        &self,
        actor: ObjectRef,
        target: ObjectRef,
        call: Call,
    ) -> Result<MotionSequence, Refused> {
        let code = self.weapon_code(actor);
        let reach = self.reach(target);
        let mut sequence = MotionSequence::new();
        self.approach_entries(&mut sequence, actor, target, RUN_DISTANCE)?;
        self.checks(&mut sequence, actor, target)?;
        if code != 0 {
            self.put_away(&mut sequence, actor, 0)?;
        }
        self.animate_entry(&mut sequence, actor, reach, None, -1)?;
        sequence.push(actor, -1, Motion::Call(call))?;
        if code != 0 {
            self.take_out(&mut sequence, actor, code)?;
        }
        Ok(sequence)
    }
    pub(crate) fn use_sequence(
        &mut self,
        actor: ObjectRef,
        target: ObjectRef,
        call: Call,
    ) -> Result<(), WorldError> {
        let sequence = self.use_motion(actor, target, call).map_err(refused)?;
        self.start_interaction(actor, sequence, Click::Action, true)?;
        Ok(())
    }
    /// SPEC-013 §3.23 climbing a ladder up: a walk to the hex southeast of
    /// the ladder (a run out of combat at object distance 5 or more), the
    /// adjacency check, a turn toward the ladder, the combat AP, putting
    /// away a held weapon, the climb sound and animation 4, the use, and
    /// taking the weapon out again. The climber keeps its last pose.
    pub(crate) fn ladder_motion(
        &self,
        actor: ObjectRef,
        ladder: ObjectRef,
    ) -> Result<MotionSequence, Refused> {
        let mut sequence = MotionSequence {
            reset: false,
            ..MotionSequence::new()
        };
        if let Some((hex, _)) = self.place(ladder) {
            let spot = step(hex, 2);
            if self.place(actor).is_none_or(|(at, _)| at != spot) {
                let run =
                    !self.world.combat && self.world.object_distance(actor, ladder) >= RUN_DISTANCE;
                let walk = Walk::new(Goal::Hex(spot), run, self.move_limit(actor));
                sequence.push(actor, 0, Motion::Walk(Box::new(walk)))?;
            }
        }
        let code = self.weapon_code(actor);
        let call = Call::Adjacent { target: ladder };
        sequence.push_forced(actor, -1, Motion::Call(call))?;
        let toward = Goal::Object(ladder);
        sequence.push(actor, -1, Motion::Turn { toward })?;
        sequence.push(actor, -1, Motion::Call(Call::CombatPoints))?;
        if code != 0 {
            self.put_away(&mut sequence, actor, -1)?;
        }
        let name = self
            .character_sound_name(actor, animation::CLIMB, 0)
            .unwrap_or_default();
        sequence.push(actor, -1, Motion::Sound { name })?;
        self.animate_entry(&mut sequence, actor, animation::CLIMB, None, 0)?;
        let call = Call::Use { target: ladder };
        sequence.push(actor, -1, Motion::Call(call))?;
        if code != 0 {
            self.take_out(&mut sequence, actor, code)?;
        }
        Ok(sequence)
    }
    pub(crate) fn ladder_sequence(
        &mut self,
        actor: ObjectRef,
        ladder: ObjectRef,
    ) -> Result<(), WorldError> {
        let sequence = self.ladder_motion(actor, ladder).map_err(refused)?;
        self.start_interaction(actor, sequence, Click::Action, true)?;
        Ok(())
    }
    /// SPEC-013 §3.16 looting a critter: the approach, the checks and the
    /// loot request.
    pub(crate) fn loot_motion(
        &self,
        actor: ObjectRef,
        critter: ObjectRef,
    ) -> Result<MotionSequence, Refused> {
        let mut sequence = MotionSequence::new();
        self.approach_entries(&mut sequence, actor, critter, RUN_DISTANCE)?;
        self.checks(&mut sequence, actor, critter)?;
        let call = Call::Loot { target: critter };
        sequence.push(actor, -1, Motion::Call(call))?;
        Ok(sequence)
    }
    pub(crate) fn loot_sequence(
        &mut self,
        actor: ObjectRef,
        critter: ObjectRef,
    ) -> Result<(), WorldError> {
        let sequence = self.loot_motion(actor, critter).map_err(refused)?;
        self.start_interaction(actor, sequence, Click::Action, true)?;
        Ok(())
    }
    /// SPEC-013 §3.2 steps 2-5, an accepted skill-on request: out of
    /// combat the actor runs to the target at object distance 5 or more,
    /// otherwise walks (no movement when the target is the player), then
    /// turns; the forced adjacency check (step 3, the shared check of
    /// SPEC-007 §3.11.3); the manipulation animation, the ground reach
    /// (10) for a prone critter target and the middle reach (11)
    /// otherwise, whose action frame is not used; and the skill-on stage
    /// after the whole animation, delay -1. No action points are charged.
    pub(crate) fn skill_motion(
        &self,
        actor: ObjectRef,
        target: ObjectRef,
        skill: i32,
    ) -> Result<MotionSequence, Refused> {
        let mut sequence = MotionSequence::new();
        if target != self.world.player {
            self.approach_entries(&mut sequence, actor, target, RUN_DISTANCE)?;
        }
        sequence.push_forced(actor, -1, Motion::Call(Call::Adjacent { target }))?;
        let reach = if self.world.critter_art(target) && self.world.is_prone(target) {
            animation::GROUND_REACH
        } else {
            animation::MIDDLE_REACH
        };
        self.animate_entry(&mut sequence, actor, reach, None, -1)?;
        let call = Call::SkillStage { target, skill };
        sequence.push(actor, -1, Motion::Call(call))?;
        Ok(sequence)
    }
    pub(crate) fn skill_sequence(
        &mut self,
        actor: ObjectRef,
        target: ObjectRef,
        skill: i32,
    ) -> Result<(), WorldError> {
        let sequence = self.skill_motion(actor, target, skill).map_err(refused)?;
        self.start_interaction(actor, sequence, Click::Action, true)?;
        Ok(())
    }
    /// SPEC-014 §3.23 swap hands: the active hand flips at once. Then one
    /// sequence puts away the old weapon, its sound and animation 39, when
    /// the player's art shows a weapon code; and takes out the new hand's
    /// weapon by its animation code (38), or returns to the standing art
    /// with weapon code 0. The sequence's end leaves the player standing
    /// with the new code, so the player's art follows the hands. The
    /// section gives the order of the entries and no delays; each waits
    /// for the one before it (-1). Returns whether the animation
    /// registered: under §3.10.1 it is discarded while another sequence
    /// holds the player, and the hand stays flipped (step 1 comes first).
    /// The bar, the mouse and the `wait2.frm` cursor during the animation,
    /// and the cursor mode afterwards, are the interface's.
    pub fn swap_hands(&mut self) -> bool {
        let player = self.world.player;
        self.world.active_hand ^= 1;
        let old = self.weapon_code(player);
        let new = self.active_weapon_code();
        let mut sequence = MotionSequence::new();
        let built = (|| {
            if old != 0 {
                self.put_away(&mut sequence, player, -1)?;
            }
            if new != 0 {
                return self.take_out(&mut sequence, player, new);
            }
            let art = self.critter_fid(player, animation::STAND, Some(0), self.facing(player));
            sequence.push(player, -1, Motion::SetArt { art })
        })();
        match built {
            Ok(()) => self.register_motion(sequence),
            Err(refused) => {
                self.world
                    .diagnostic(format!("swap hands sequence refused: {refused:?}"));
                false
            }
        }
    }
    /// The weapon animation code of the item in the player's active hand
    /// (SPEC-003 weapon payload), 0 for an empty hand or a non-weapon.
    fn active_weapon_code(&self) -> u8 {
        let mode = if self.world.active_hand == 0 {
            hit_mode::LEFT_PRIMARY
        } else {
            hit_mode::RIGHT_PRIMARY
        };
        self.world
            .wielded(self.world.player, mode)
            .and_then(|item| self.world.item_data(item))
            .map_or(0, |item| match &item.data {
                ItemData::Weapon(weapon) => u8::try_from(weapon.animation).unwrap_or(0),
                _ => 0,
            })
    }
    /// An approach whose request then runs its own stage (talk, skills): a
    /// walk or run toward the target, a turn, and the arrival.
    pub(crate) fn approach_sequence(
        &mut self,
        actor: ObjectRef,
        target: ObjectRef,
        action: ObjectAction,
        run: bool,
    ) -> Result<(), WorldError> {
        let walk = Walk::new(Goal::Object(target), run, self.move_limit(actor));
        let mut sequence = MotionSequence::new();
        sequence
            .push(actor, 0, Motion::Walk(Box::new(walk)))
            .and_then(|()| {
                let toward = Goal::Object(target);
                sequence.push(actor, -1, Motion::Turn { toward })
            })
            .and_then(|()| {
                let call = Call::Arrive { target, action };
                sequence.push(actor, -1, Motion::Call(call))
            })
            .map_err(refused)?;
        self.start_interaction(actor, sequence, Click::Action, true)?;
        Ok(())
    }
    /// [`Self::walk_toward_with`] under the `fallout.cfg` default
    /// `[system] interrupt_walk` = 1.
    pub fn walk_toward(
        &mut self,
        actor: ObjectRef,
        tile: i32,
        run: bool,
        reserve: i32,
    ) -> Result<bool, WorldError> {
        self.walk_toward_with(actor, tile, run, reserve, true)
    }
    /// SPEC-014 §3.27 a movement click: a walk, or a run, toward `tile`,
    /// limited in combat to the action points and free movement less
    /// `reserve`. For a busy player (a sequence holding it is still
    /// playing) `interrupt_walk` decides: on, the click first cancels the
    /// player's current sequence, whatever it is doing, dropping its
    /// unstarted use, take-out and loot; off, the walk cannot register and
    /// the click does nothing. A player's run clears sneaking without
    /// Silent Running. Returns whether the walk registered: false when the
    /// click does nothing, without a route, or when the sequence cannot
    /// register (SPEC-007 §3.10.1).
    pub fn walk_toward_with(
        &mut self,
        actor: ObjectRef,
        tile: i32,
        run: bool,
        reserve: i32,
        interrupt_walk: bool,
    ) -> Result<bool, WorldError> {
        let player = actor == self.world.player;
        if player {
            match busy_policy(Click::Movement, &self.click_state(), interrupt_walk) {
                Policy::Ignore => return Ok(false),
                Policy::Cancel => self.cancel_motions(actor),
                Policy::Proceed => {}
            }
        }
        let limit = if self.world.combat {
            let free = if player { self.world.free_movement } else { 0 };
            let budget = self.world.action_points(actor) + free - reserve;
            if budget <= 0 {
                return Ok(false);
            }
            usize::try_from(budget).unwrap_or(0)
        } else {
            usize::MAX
        };
        let Some(movement) = self.plan_to_tile(actor, tile, limit) else {
            return Ok(false);
        };
        if movement.directions.is_empty() {
            return Ok(false);
        }
        if player && run && self.sneaking() && self.world.perk(Perk::SilentRunning) == 0 {
            self.set_sneak(false)?;
        }
        let mut sequence = MotionSequence::new();
        let walk = Walk::new(Goal::Hex(tile), run, limit);
        sequence
            .push(actor, 0, Motion::Walk(Box::new(walk)))
            .map_err(refused)?;
        Ok(self.register_motion(sequence))
    }

    // §3.11.4 doors and containers, and the interaction callbacks.

    /// SPEC-006 §3.8 the pickup: `pickup_p_proc`, then unless it overrides,
    /// the item (money by its amount) joins the actor's inventory.
    fn pick_up(&mut self, actor: ObjectRef, item: ObjectRef) -> Result<(), WorldError> {
        if self
            .world
            .get(item)
            .is_none_or(|object| object.owner.is_some())
        {
            return Ok(());
        }
        let result = self.request(item, ScriptEvent::Pickup, actor, item)?;
        if result.pending {
            return Ok(());
        }
        if !result.overridden {
            let amount = self
                .world
                .get(item)
                .and_then(|object| object.money)
                .map_or(1, |amount| amount.max(1));
            self.world.add_item(actor, item, amount, false)?;
            if let Some(object) = self.world.get_mut(item)
                && object.money.is_some()
            {
                object.money = Some(0);
            }
            self.world.rebuild_lighting();
            self.world.emit(WorldEffect::DefaultAction {
                action: ObjectAction::Pickup,
                target: item,
            });
        }
        self.drain_requests()
    }
    /// A script request of `object`'s script, if it has one.
    fn request(
        &mut self,
        object: ObjectRef,
        event: ScriptEvent,
        source: ObjectRef,
        target: ObjectRef,
    ) -> Result<DispatchResult, WorldError> {
        match self.world.script_for(object) {
            Some(script) => self.dispatch(ScriptRequest {
                script,
                event,
                source,
                target,
                fixed: 0,
                action: -1,
                only_present: false,
            }),
            None => Ok(DispatchResult::default()),
        }
    }
    fn is_door(&self, object: ObjectRef) -> bool {
        self.world
            .get(object)
            .and_then(|found| found.prototype.as_deref())
            .is_some_and(|prototype| {
                matches!(
                    prototype.data,
                    PrototypeData::Scenery(vault_formats::pro::Scenery {
                        data: SceneryData::Door { .. },
                        ..
                    })
                )
            })
    }
    /// The use of a target: a door's use (§3.11.4); otherwise its
    /// `use_p_proc`, and without an override, `proto.msg` 480 naming it
    /// for a player using scenery; then the map update.
    fn use_object(&mut self, user: ObjectRef, target: ObjectRef) -> Result<(), WorldError> {
        if self.is_door(target) {
            return self.use_door(user, target);
        }
        let result = self.request(target, ScriptEvent::Use, user, target)?;
        if result.pending {
            return Ok(());
        }
        if !result.overridden {
            let object = self.world.get(target);
            let scenery = object
                .is_some_and(|object| object.category() == vault_formats::pro::Category::Scenery);
            let name = object.map(|object| object.name.clone()).unwrap_or_default();
            if scenery && user == self.world.player {
                self.print_game("proto.msg", 480, &[Arg::Text(&name)]);
            }
            self.world.emit(WorldEffect::DefaultAction {
                action: ObjectAction::Use,
                target,
            });
        }
        self.map_update()?;
        self.drain_requests()
    }
    /// §3.11.4 a door's use, by a click, a script or a walker opening it:
    /// a locked door's locked sound at once, then its `use_p_proc`; unless
    /// that overrides, a door not at frame 0 closes and a closed one opens
    /// (one already marked open stays as it is). A locked door is not
    /// refused here.
    pub(crate) fn use_door(&mut self, user: ObjectRef, door: ObjectRef) -> Result<(), WorldError> {
        let Some(lock) = self.world.get(door).and_then(|object| object.lock()) else {
            return Ok(());
        };
        if lock.contains(LockFlags::LOCKED) {
            self.emit_open_sound(door, sound::LOCKED);
        }
        let result = self.request(door, ScriptEvent::Use, user, door)?;
        if result.overridden || result.pending {
            return Ok(());
        }
        let Some(object) = self.world.get(door) else {
            return Ok(());
        };
        let open_bit = object
            .lock()
            .is_some_and(|lock| lock.contains(LockFlags::OPEN));
        if object.raw.frame != 0 {
            self.swing(door, false, false);
        } else if !open_bit {
            self.swing(door, true, false);
        }
        Ok(())
    }
    /// The open or close sequence of a door or container (§3.11.4): its
    /// open bit (a forced callback for a script's request), the open or
    /// close sound, animation 0 forward or in reverse, and the forced
    /// finishing callback. Under the safe rule an opening door plays the
    /// open sound rather than the close sound.
    fn swing(&mut self, object: ObjectRef, open: bool, scripted: bool) {
        let name = self
            .open_sound_name(object, if open { sound::OPEN } else { sound::CLOSE })
            .unwrap_or_default();
        let mut sequence = MotionSequence::new();
        let bit = Motion::Call(Call::OpenBit { open });
        let built = if scripted {
            sequence.push_forced(object, -1, bit)
        } else {
            sequence.push(object, -1, bit)
        }
        .and_then(|()| sequence.push(object, -1, Motion::Sound { name }))
        .and_then(|()| {
            let swing = Motion::Swing {
                forward: open,
                interval_ms: 100,
            };
            sequence.push(object, 0, swing)
        })
        .and_then(|()| {
            let finish = Motion::Call(Call::Finish { open });
            sequence.push_forced(object, -1, finish)
        });
        match built {
            Ok(()) => {
                self.register_motion(sequence);
            }
            Err(refused) => self
                .world
                .diagnostic(format!("door sequence refused: {refused:?}")),
        }
    }
    /// §3.11.4 a container's use: a locked one plays its locked sound and
    /// tells the player `proto.msg` 487; otherwise its `use_p_proc`, and
    /// unless that overrides, its open (from frame 0) or close sound and
    /// animation, with `proto.msg` 485 or 486 naming it for the player. No
    /// flag word changes.
    fn use_container(&mut self, user: ObjectRef, container: ObjectRef) -> Result<(), WorldError> {
        let Some(object) = self.world.get(container) else {
            return Ok(());
        };
        let player = user == self.world.player;
        if object
            .lock()
            .is_some_and(|lock| lock.contains(LockFlags::LOCKED))
        {
            self.emit_open_sound(container, sound::LOCKED);
            if player {
                self.print_game("proto.msg", 487, &[]);
            }
            return Ok(());
        }
        let result = self.request(container, ScriptEvent::Use, user, container)?;
        if result.overridden || result.pending {
            return Ok(());
        }
        let Some(object) = self.world.get(container) else {
            return Ok(());
        };
        let closed = object.raw.frame == 0;
        let name = object.name.clone();
        let effect = self
            .open_sound_name(container, if closed { sound::OPEN } else { sound::CLOSE })
            .unwrap_or_default();
        let swing = Motion::Swing {
            forward: closed,
            interval_ms: 100,
        };
        let entries = [
            (container, 0, Motion::Sound { name: effect }),
            (container, 0, swing),
        ];
        // §3.11.3: the container's sound and animation join the user's
        // sequence right after this use, so the take-out and the loot
        // request (-1) wait for the container animation too. §3.10.1: a
        // container another sequence still holds cannot register them.
        if self.owns_motion(container) {
            self.world.diagnostic(format!(
                "container animation discarded: object {container} is in a sequence that has not ended"
            ));
        } else if let Some(running) = self.running_motions.last_mut() {
            running.appends.extend(entries);
            if !running.owners.contains(&container) {
                running.owners.push(container);
            }
        } else {
            let mut sequence = MotionSequence::new();
            let mut built = Ok(());
            for (owner, delay, motion) in entries {
                built = built.and_then(|()| sequence.push(owner, delay, motion));
            }
            match built {
                Ok(()) => {
                    self.register_motion(sequence);
                }
                Err(refused) => self
                    .world
                    .diagnostic(format!("container sequence refused: {refused:?}")),
            }
        }
        if player {
            let id = if closed { 485 } else { 486 };
            self.print_game("proto.msg", id, &[Arg::Text(&name)]);
        }
        self.world.emit(WorldEffect::DefaultAction {
            action: ObjectAction::Use,
            target: container,
        });
        Ok(())
    }
    /// §3.11.4 `obj_open` and `obj_close`: a door or container in the
    /// opposite state (a door by its open bit, a container by its frame)
    /// opens or closes with no `use_p_proc`; a locked one is refused with
    /// no sound; the jam is cleared.
    pub(crate) fn script_open(&mut self, object: ObjectRef, open: bool) {
        let Some(found) = self.world.get(object) else {
            return;
        };
        let Some(mut lock) = found.lock() else {
            return;
        };
        let is_open = if self.is_door(object) {
            lock.contains(LockFlags::OPEN)
        } else {
            found.raw.frame != 0
        };
        if is_open == open || lock.contains(LockFlags::LOCKED) {
            return;
        }
        lock.0 &= !LockFlags::JAMMED.0;
        if let Some(found) = self.world.get_mut(object) {
            found.set_lock(lock);
        }
        self.swing(object, open, true);
    }
    /// SPEC-013 §3.16 the loot request: a critter's, or a container's.
    fn post_loot(&mut self, looter: ObjectRef, target: ObjectRef) {
        let critter = self
            .world
            .get(target)
            .is_some_and(|object| object.category() == vault_formats::pro::Category::Critter);
        if critter {
            self.world.pending_loot = Some(LootRequest {
                looter,
                target,
                steal: false,
            });
        } else {
            self.loot_container(looter, target, true);
        }
    }
}

/// A sequence that cannot register (§3.10.1).
fn refused(refused: Refused) -> WorldError {
    WorldError::Invalid(format!("animation sequence refused: {refused:?}"))
}

impl<A: WorldAssets> Driver<ObjectRef, Motion> for WorldHost<A> {
    fn start(&mut self, entry: &mut Entry<ObjectRef, Motion>, _now_ms: u64) -> Begin {
        let owner = entry.owner;
        // §3.10.1 a cancelled or halted sequence runs nothing more, but
        // its forced callbacks and erases, which run at once.
        if self.world.get(owner).is_none() || (self.halted() && !entry.forced) {
            return Begin::Done;
        }
        match &mut entry.action {
            Motion::Turn { toward } => {
                self.turn_toward(owner, *toward);
                Begin::Done
            }
            Motion::Sound { name } => {
                if !name.is_empty() {
                    self.world.emit(WorldEffect::SoundEffect {
                        name: name.clone(),
                        owner: Some(owner),
                    });
                }
                Begin::Done
            }
            Motion::Animate {
                animation,
                weapon,
                art,
                frames,
            } => {
                let facing = self.facing(owner);
                *art = self.critter_fid(owner, *animation, *weapon, facing);
                *frames = self.frames(*art, facing, 0);
                if let Some(object) = self.world.get_mut(owner) {
                    object.raw.displacement = [0; 2];
                }
                Begin::Timed
            }
            Motion::Swing { interval_ms, .. } => {
                // §3.10.1: a frame step every trunc(1000 / fps) ms at the
                // art's own rate, stored 0 meaning 10 fps. Safe bound: a
                // door or container whose art cannot be loaded steps at
                // that 10 fps default, 100 ms, rather than not at all.
                let art = self.world.get(owner).map_or(0, |object| object.raw.art_id);
                *interval_ms = self.art_interval(art, 0);
                Begin::Timed
            }
            Motion::Play {
                animation,
                art,
                frames,
                ..
            } => {
                let facing = self.facing(owner);
                *art = if self.world.critter_art(owner) {
                    self.critter_fid(owner, *animation, None, facing)
                } else {
                    self.world.get(owner).map_or(0, |object| object.raw.art_id)
                };
                *frames = self.frames(*art, facing, 0);
                Begin::Timed
            }
            Motion::Stand => {
                if self.world.critter_art(owner) && !self.world.is_dead(owner) {
                    let art = self.critter_fid(owner, animation::STAND, None, self.facing(owner));
                    if let Some(object) = self.world.get_mut(owner) {
                        object.raw.art_id = art;
                        object.raw.frame = 0;
                    }
                }
                Begin::Done
            }
            Motion::SetArt { art } => {
                if let Some(object) = self.world.get_mut(owner) {
                    object.raw.art_id = *art;
                    object.raw.frame = 0;
                }
                Begin::Done
            }
            Motion::Walk(walk) => self.start_walk(owner, walk),
            Motion::Call(call) => match self.call(owner, *call) {
                Ok(begin) => begin,
                Err(error) => {
                    self.world.diagnostic(error.to_string());
                    Begin::Failed
                }
            },
        }
    }
    fn step(&mut self, entry: &mut Entry<ObjectRef, Motion>, step: usize, _now_ms: u64) -> Stepped {
        let owner = entry.owner;
        if self.halted() {
            return Stepped::Done;
        }
        match &mut entry.action {
            Motion::Animate { art, frames, .. } => self.animate_step(owner, *art, frames, step),
            Motion::Swing {
                forward,
                interval_ms,
            } => self.swing_step(owner, *forward, *interval_ms, step),
            Motion::Walk(walk) => self.walk_step(owner, walk, step),
            Motion::Play {
                animation,
                play,
                art,
                frames,
            } => self.play_step(owner, *animation, *play, *art, frames, step),
            _ => Stepped::Done,
        }
    }
    fn appended(&mut self) -> Vec<(ObjectRef, i32, Motion)> {
        self.running_motions
            .last_mut()
            .map(|running| std::mem::take(&mut running.appends))
            .unwrap_or_default()
    }
    /// A repeating script animation (SPEC-005 `reg_anim_animate_forever`)
    /// runs until something cancels it.
    fn endless(&self, entry: &Entry<ObjectRef, Motion>) -> bool {
        matches!(
            entry.action,
            Motion::Play {
                play: Play::Forever,
                ..
            }
        )
    }
}
