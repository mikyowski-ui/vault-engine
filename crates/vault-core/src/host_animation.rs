//! SPEC-005 animation registrations. A script registers a sequence of
//! entries, or one entry outside any registration; once registered it runs
//! as one of the world's sequences under the SPEC-007 §3.10.1 start rule:
//! delay 0 starts with the entry before it, -1 once every earlier entry has
//! finished, and N > 0 after N examinations. Movement takes the SPEC-007
//! §3.11.1 walk steps with unlimited action points.
use crate::{
    ObjectRef,
    assets::WorldAssets,
    choreography::{CAPACITY, Frames, Refused},
    host::{AnimationSequence, WorldHost},
    motion::{Goal, Motion, MotionSequence, Play, Walk},
};

/// SPEC-007 §2.1 the take-out animation.
const TAKE_OUT: u8 = 38;

/// The animated object is each registration's last operand.
fn animated_object(command: &(u16, Vec<i32>)) -> Option<ObjectRef> {
    command.1.last().map(|object| *object as ObjectRef)
}

impl<A: WorldAssets> WorldHost<A> {
    /// `anim_busy`: a world sequence with an entry of the object's still to
    /// finish, other than a sound or a callback; a lone standing action
    /// does not count. A full construction list forces zero.
    pub fn animation_busy(&self, object: ObjectRef) -> bool {
        if object == 0
            || self
                .building_animation
                .as_ref()
                .is_some_and(|sequence| sequence.commands.len() >= CAPACITY)
        {
            return false;
        }
        self.motions.iter().any(|sequence| sequence.busy(object))
    }
    /// A registered script sequence joins the world's sequences. One that
    /// cannot register (more than [`CAPACITY`] entries, or an animation
    /// whose art cannot be loaded) is discarded whole (§3.10.1).
    pub(crate) fn register_script_sequence(&mut self, sequence: &AnimationSequence) {
        match self.script_motion(sequence) {
            Ok(motion) => {
                self.register_motion(motion);
            }
            Err(refused) => self
                .world
                .diagnostic(format!("animation sequence refused: {refused:?}")),
        }
    }
    /// The entries of a script sequence. Request flag 4 suppresses the
    /// standing reset at its end and 512 makes it insignificant.
    fn script_motion(&self, sequence: &AnimationSequence) -> Result<MotionSequence, Refused> {
        let mut motion = MotionSequence {
            reset: sequence.flags & 4 == 0,
            insignificant: sequence.flags & 512 != 0,
            ..MotionSequence::new()
        };
        let walk = |run: bool, goal: Goal| Motion::Walk(Box::new(Walk::new(goal, run, usize::MAX)));
        for (index, command) in sequence.commands.iter().enumerate() {
            let (op, values) = command;
            let Some(object) = animated_object(command) else {
                continue;
            };
            let delay = values.first().copied().unwrap_or(0);
            match op {
                0x810f | 0x8110 | 0x8126 => {
                    let animation = values[values.len() - 2] as u8;
                    let art = self.script_art(object, animation);
                    if !self.assets.art_exists(art) {
                        return Err(Refused::Art(art));
                    }
                    let (play, delay) = match op {
                        0x810f => (Play::Forward, delay),
                        0x8110 => (Play::Reverse, delay),
                        _ => (Play::Forever, -1),
                    };
                    let entry = Motion::Play {
                        animation,
                        play,
                        art,
                        frames: Frames::default(),
                    };
                    motion.push(object, delay, entry)?;
                }
                0x8111 | 0x8112 => {
                    let goal = Goal::Object(values[1] as ObjectRef);
                    motion.push(object, delay, walk(*op == 0x8112, goal))?;
                }
                0x8113 | 0x8114 => {
                    let goal = Goal::Hex(values[1]);
                    motion.push(object, delay, walk(*op == 0x8114, goal))?;
                }
                0x80ce => motion.push(object, 0, walk(values[0] != 0, Goal::Hex(values[1])))?,
                0x813b => {
                    let name = sequence.sounds.get(&index).cloned().unwrap_or_default();
                    motion.push(object, delay, Motion::Sound { name })?;
                }
                0x80cc | 0x80cd => motion.push(object, 0, Motion::Stand)?,
                // `anim`: forward for control 0, reverse otherwise.
                0x810c => {
                    let animation = values[1] as u8;
                    let art = self.script_art(object, animation);
                    if !self.assets.art_exists(art) {
                        return Err(Refused::Art(art));
                    }
                    let play = if values[0] == 0 {
                        Play::Forward
                    } else {
                        Play::Reverse
                    };
                    let entry = Motion::Play {
                        animation,
                        play,
                        art,
                        frames: Frames::default(),
                    };
                    motion.push(object, 0, entry)?;
                }
                // SPEC-017 §4.1, Fallout 2 only: the complete art
                // identifier, used as given (instant); art that cannot be
                // loaded fails the whole sequence.
                0x8157 => {
                    let art = values[1] as u32;
                    if !self.assets.art_exists(art) {
                        return Err(Refused::Art(art));
                    }
                    motion.push(object, delay, Motion::SetArt { art })?;
                }
                // The take-out animation for the weapon code (timed); the
                // art then keeps that code.
                0x8158 => {
                    let code = values[1] as u8;
                    let facing = self.world.get(object).map_or(0, |o| o.raw.orientation);
                    let art = self.critter_fid(object, TAKE_OUT, Some(code), facing);
                    if !self.assets.art_exists(art) {
                        return Err(Refused::Art(art));
                    }
                    let entry = Motion::Animate {
                        animation: TAKE_OUT,
                        weapon: Some(code),
                        art,
                        frames: Frames::default(),
                    };
                    motion.push(object, delay, entry)?;
                }
                // Facing the hex, as `rotation_to_tile` from its own hex.
                0x8159 => {
                    let toward = Goal::Hex(values[1]);
                    motion.push(object, delay, Motion::Turn { toward })?;
                }
                _ => {}
            }
        }
        Ok(motion)
    }
    /// The art a script animation of `object` shows: a critter's animation
    /// for its base and weapon code, or any other object's own art.
    fn script_art(&self, object: ObjectRef, animation: u8) -> u32 {
        let Some(found) = self.world.get(object) else {
            return 0;
        };
        if self.world.critter_art(object) {
            self.critter_fid(object, animation, None, found.raw.orientation)
        } else {
            found.raw.art_id
        }
    }
}
