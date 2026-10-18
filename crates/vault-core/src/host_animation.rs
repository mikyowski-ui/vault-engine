//! Executing SPEC-005 animation registrations. Sequences run their commands
//! in order, one frame step at a time (at the art's frame interval, or 10 FPS
//! without art), and retire when complete. Movement uses the SPEC-007 routes
//! with unlimited action points.
use crate::{
    Movement, ObjectRef, StepOutcome, WorldEffect, WorldError, assets::WorldAssets, host::WorldHost,
};
use vault_formats::{Fid, map::ObjectState};

/// Per-command execution state.
#[derive(Debug, Clone, Default)]
pub struct CommandProgress {
    pub started: bool,
    /// The command's art/route is set up (after any delay).
    pub prepared: bool,
    /// Remaining frame steps before the command starts.
    pub wait: i32,
    pub frame: usize,
    pub frames: usize,
    pub interval_ms: u64,
    pub next_ms: u64,
    pub movement: Option<Movement>,
}

/// The animated object is each registration's last operand.
fn animated_object(command: &(u16, Vec<i32>)) -> Option<ObjectRef> {
    command.1.last().map(|object| *object as ObjectRef)
}

impl<A: WorldAssets> WorldHost<A> {
    /// `anim_busy`: an active, nontrivial registration for the object. Sound
    /// cues and a lone standing action do not count; neither does the
    /// sequence under construction. A full construction list forces zero.
    pub fn animation_busy(&self, object: ObjectRef) -> bool {
        if object == 0
            || self
                .building_animation
                .as_ref()
                .is_some_and(|sequence| sequence.commands.len() >= 64)
        {
            return false;
        }
        self.animations.iter().any(|sequence| {
            let lone_stand =
                sequence.commands.len() == 1 && matches!(sequence.commands[0].0, 0x80cc | 0x80cd);
            !lone_stand
                && sequence.commands[sequence.cursor.min(sequence.commands.len())..]
                    .iter()
                    .any(|command| command.0 != 0x813b && animated_object(command) == Some(object))
        })
    }
    /// The art identifier for an animation of a critter, keeping its base and
    /// weapon code; non-critters keep their art.
    fn animation_fid(&self, object: ObjectRef, animation: u8) -> Option<u32> {
        let current = Fid(self.world.get(object)?.raw.art_id);
        if current.category() != 1 {
            return Some(current.0);
        }
        let orientation = self.world.get(object)?.raw.orientation;
        Some(self.assets.construct_fid(
            1,
            current.base(),
            animation,
            current.weapon(),
            Fid::selector_for_orientation(orientation),
        ))
    }
    /// Advance every sequence by at most one frame step whose time is due.
    pub fn step_animations(&mut self, now_ms: u64) -> Result<bool, WorldError> {
        let mut changed = false;
        let mut index = 0;
        while index < self.animations.len() {
            let finished = self.step_sequence(index, now_ms, &mut changed)?;
            if finished {
                let sequence = self.animations.remove(index);
                // Unless suppressed, animated critters return to standing.
                if sequence.flags & 4 == 0 {
                    for object in sequence.commands.iter().filter_map(animated_object) {
                        self.stand(object);
                    }
                }
                changed = true;
            } else {
                index += 1;
            }
        }
        Ok(changed)
    }
    fn stand(&mut self, object: ObjectRef) {
        let dead = matches!(
            self.world.get(object).map(|o| &o.raw.state),
            Some(ObjectState::Critter(state)) if state.results.0 & crate::critical::effect::DEAD != 0
        );
        if !dead
            && let Some(fid) = self.animation_fid(object, 0)
            && let Some(found) = self.world.get_mut(object)
        {
            found.raw.art_id = fid;
            found.raw.frame = 0;
        }
    }
    /// Returns whether the sequence has completed all commands.
    fn step_sequence(
        &mut self,
        index: usize,
        now_ms: u64,
        changed: &mut bool,
    ) -> Result<bool, WorldError> {
        let sequence = &self.animations[index];
        let Some(command) = sequence.commands.get(sequence.cursor).cloned() else {
            return Ok(true);
        };
        let mut progress = sequence.progress.clone();
        if progress.next_ms > now_ms {
            return Ok(false);
        }
        let (op, values) = &command;
        let object = animated_object(&command).unwrap_or(0);
        if self.world.get(object).is_none() {
            return self.next_command(index);
        }
        if !progress.started {
            progress.started = true;
            // Delays count frame steps; a negative delay waits on the previous
            // command, which sequential execution already satisfies.
            progress.wait = match op {
                0x810f..=0x8114 | 0x813b => values.first().copied().unwrap_or(0).max(0),
                _ => 0,
            };
            progress.interval_ms = 100;
        }
        if progress.wait > 0 {
            progress.wait -= 1;
            progress.next_ms = now_ms + progress.interval_ms;
            self.animations[index].progress = progress;
            return Ok(false);
        }
        if !progress.prepared {
            progress.prepared = true;
            match op {
                0x810f | 0x8110 | 0x8126 => {
                    let animation = values[values.len() - 2] as u8;
                    let fid = self.animation_fid(object, animation).unwrap_or(0);
                    let art = self.assets.object_art(fid).ok();
                    progress.frames = art.as_ref().map_or(0, |art| art.frame_count());
                    progress.interval_ms = art
                        .as_ref()
                        .and_then(|art| art.frame_interval_ms())
                        .unwrap_or(100);
                    if progress.frames > 0 {
                        let found = self.world.get_mut(object).expect("checked object");
                        found.raw.art_id = fid;
                        found.raw.frame = if *op == 0x8110 {
                            progress.frames - 1
                        } else {
                            0
                        };
                        progress.frame = 0;
                    }
                }
                0x8111..=0x8114 | 0x80ce => {
                    let run = matches!(op, 0x8112 | 0x8114) || (*op == 0x80ce && values[0] != 0);
                    let fid = self.animation_fid(object, if run { 19 } else { 1 });
                    if let (Some(fid), Some(found)) = (fid, self.world.get_mut(object)) {
                        found.raw.art_id = fid;
                    }
                    progress.movement = match op {
                        0x8111 | 0x8112 => self.plan_to_object(object, values[1] as ObjectRef),
                        _ => self.plan_to_tile(object, values[1], usize::MAX),
                    };
                    self.emit_movement_sound(object, run);
                }
                _ => {}
            }
            // The first frame (or the walk art) shows for this frame step.
            if matches!(op, 0x810f | 0x8110 | 0x8126 | 0x8111..=0x8114 | 0x80ce)
                && (progress.frames > 0 || progress.movement.is_some())
            {
                *changed = true;
                progress.next_ms = now_ms + progress.interval_ms;
                self.animations[index].progress = progress;
                return Ok(false);
            }
        }
        let done = match op {
            0x810f | 0x8110 | 0x8126 => {
                if progress.frames == 0 {
                    true
                } else {
                    progress.frame += 1;
                    *changed = true;
                    let last = progress.frame >= progress.frames;
                    let found = self.world.get_mut(object).expect("checked object");
                    if *op == 0x8126 {
                        found.raw.frame = progress.frame % progress.frames;
                        false
                    } else if last {
                        // Forward death animations end on their static art.
                        let animation = Fid(found.raw.art_id).animation();
                        if *op == 0x810f && (20..=35).contains(&animation) {
                            let still = self.animation_fid(object, animation + 28);
                            if let (Some(still), Some(found)) = (still, self.world.get_mut(object))
                            {
                                found.raw.art_id = still;
                                found.raw.frame = 0;
                            }
                        }
                        true
                    } else {
                        found.raw.frame = if *op == 0x8110 {
                            progress.frames - 1 - progress.frame
                        } else {
                            progress.frame
                        };
                        false
                    }
                }
            }
            0x8111..=0x8114 | 0x80ce => match progress.movement.as_mut() {
                Some(movement) => {
                    let outcome = self.step_movement(movement)?;
                    *changed |= outcome == StepOutcome::Moved;
                    matches!(outcome, StepOutcome::Finished | StepOutcome::Stopped)
                        || movement.finished()
                }
                None => true,
            },
            0x813b => {
                let sequence = &self.animations[index];
                if let Some(sound) = sequence.sounds.get(&sequence.cursor).cloned() {
                    // Animation-registered effects supply their owner.
                    self.world.emit(WorldEffect::SoundEffect {
                        name: sound,
                        owner: Some(object),
                    });
                }
                true
            }
            0x80cc | 0x80cd => {
                self.stand(object);
                true
            }
            _ => true,
        };
        progress.next_ms = now_ms + progress.interval_ms;
        self.animations[index].progress = progress;
        if done {
            return self.next_command(index);
        }
        Ok(false)
    }
    fn next_command(&mut self, index: usize) -> Result<bool, WorldError> {
        let sequence = &mut self.animations[index];
        sequence.cursor += 1;
        sequence.progress = Default::default();
        Ok(sequence.cursor >= sequence.commands.len())
    }
}
