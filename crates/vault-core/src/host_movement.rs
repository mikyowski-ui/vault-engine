//! SPEC-007 §3.1/§3.4 movement: move-to-object/tile planning, per-hex
//! commits with door auto-opening and strict-goal replanning, and AP debits.
use crate::{
    ObjectRef, WorldError, assets::WorldAssets, critical::effect, host::WorldHost, routing::step,
};
use vault_formats::{map::ObjectState, pro::BasicFlags};

/// A scheduled route. `destination` is the saved replan destination, which
/// the reference does not always set to the intended stopping hex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Movement {
    pub actor: ObjectRef,
    pub directions: Vec<u8>,
    pub cursor: usize,
    pub destination: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
    /// One hex transition committed.
    Moved,
    /// A newly blocking non-door rebuilt the route; facing/cursor were reset.
    Replanned,
    /// The route is complete.
    Finished,
    /// Rebuilding failed (or the actor vanished): movement ends.
    Stopped,
}

impl Movement {
    pub fn finished(&self) -> bool {
        self.cursor >= self.directions.len()
    }
}

/// Remove `count` final steps, deriving the saved replan destination by
/// stepping forward from `target` along the removed final direction (and, for
/// two, also along the preceding removed direction).
fn shorten(mut directions: Vec<u8>, target: i32, count: usize) -> (Vec<u8>, i32) {
    let mut destination = target;
    for _ in 0..count {
        match directions.pop() {
            Some(direction) => destination = step(destination, direction),
            None => break,
        }
    }
    (directions, destination)
}

impl<A: WorldAssets> WorldHost<A> {
    /// Move-to-object: plan with the destination object temporarily hidden,
    /// then drop one step (two for a multihex mover; only the mover's flag
    /// matters). The core search does not dilate obstacles for the mover.
    /// The goal is permissive (SPEC-007 §3.1 "Move-to-object goal"): the
    /// target's own hex is never tested for a blocker, so a container
    /// sharing its hex with a wall (as corpus lockers do) is reached from
    /// the route's next-to-last hex.
    pub fn plan_to_object(&mut self, actor: ObjectRef, target: ObjectRef) -> Option<Movement> {
        let start = self.world.get(actor)?.raw.hex?;
        let goal = self.world.get(target)?.raw.hex?;
        let hidden = self.world.get(target)?.raw.flags.0 & BasicFlags::HIDDEN.0;
        self.world.get_mut(target)?.raw.flags.0 |= BasicFlags::HIDDEN.0;
        let directions = self.world.route(actor, start, goal, false);
        let object = self.world.get_mut(target)?;
        object.raw.flags.0 = (object.raw.flags.0 & !BasicFlags::HIDDEN.0) | hidden;
        let count = if self
            .world
            .get(actor)?
            .raw
            .flags
            .contains(BasicFlags::MULTIHEX)
        {
            2
        } else {
            1
        };
        let (directions, destination) = shorten(directions, goal, count);
        Some(Movement {
            actor,
            directions,
            cursor: 0,
            destination,
        })
    }
    /// Move-to-tile: remove the last step when the destination is occupied,
    /// updating the saved destination the same way. `limit` truncates the
    /// scheduled route length.
    pub fn plan_to_tile(&self, actor: ObjectRef, tile: i32, limit: usize) -> Option<Movement> {
        let object = self.world.get(actor)?;
        let start = object.raw.hex?;
        let mut directions = self.world.route(actor, start, tile, false);
        let mut destination = tile;
        if self
            .world
            .movement_blocker(tile, object.raw.elevation, actor)
            .is_some()
        {
            (directions, destination) = shorten(directions, tile, 1);
        }
        directions.truncate(limit);
        Some(Movement {
            actor,
            directions,
            cursor: 0,
            destination,
        })
    }
    /// Commit the next hex: a usable door gets an open/use request and the
    /// move proceeds; any other new blocker triggers a strict-goal rebuild from
    /// the current hex toward the saved destination.
    pub fn step_movement(&mut self, movement: &mut Movement) -> Result<StepOutcome, WorldError> {
        if movement.finished() {
            return Ok(StepOutcome::Finished);
        }
        let Some((current, elevation)) = self
            .world
            .get(movement.actor)
            .and_then(|o| Some((o.raw.hex?, o.raw.elevation)))
        else {
            return Ok(StepOutcome::Stopped);
        };
        let direction = movement.directions[movement.cursor];
        let next = step(current, direction);
        if let Some(blocker) = self.world.movement_blocker(next, elevation, movement.actor) {
            if self.world.may_auto_open(movement.actor, blocker) {
                // The door action's result is not checked before moving on.
                self.request_door_open(movement.actor, blocker)?;
            } else {
                let rebuilt = self
                    .world
                    .route(movement.actor, current, movement.destination, true);
                if rebuilt.is_empty() {
                    return Ok(StepOutcome::Stopped);
                }
                if let Some(object) = self.world.get_mut(movement.actor) {
                    object.raw.orientation = rebuilt[0];
                    object.raw.frame = 0;
                }
                self.world.set_offset(movement.actor, [0, 0]);
                movement.directions = rebuilt;
                movement.cursor = 0;
                return Ok(StepOutcome::Replanned);
            }
        }
        if let Some(object) = self.world.get_mut(movement.actor) {
            object.raw.orientation = direction;
        }
        self.world
            .move_object(movement.actor, next, elevation, true)?;
        self.charge_movement(movement.actor);
        movement.cursor += 1;
        Ok(StepOutcome::Moved)
    }
    /// The door's use (SPEC-007 §3.11.4): its script runs first; unless
    /// overridden, the door opens or closes in its own sequence.
    fn request_door_open(&mut self, actor: ObjectRef, door: ObjectRef) -> Result<(), WorldError> {
        self.use_door(actor, door)
    }
    /// SPEC-007 §3.4: in combat each committed hex costs 1 AP with sound legs,
    /// 4 with one crippled leg, 8 with both; the player's free-movement pool
    /// pays first, then ordinary AP saturating at zero.
    pub fn charge_movement(&mut self, actor: ObjectRef) {
        if !self.world.combat {
            return;
        }
        let player = actor == self.world.player;
        let Some(ObjectState::Critter(state)) = self.world.get_mut(actor).map(|o| &mut o.raw.state)
        else {
            return;
        };
        let cost = match (state.results.0 & effect::CRIPPLED_LEGS).count_ones() {
            0 => 1,
            1 => 4,
            _ => 8,
        };
        let mut remaining = cost;
        if player {
            let free = self.world.free_movement.min(remaining);
            self.world.free_movement -= free;
            remaining -= free;
        }
        if let Some(ObjectState::Critter(state)) =
            self.world.get_mut(actor).map(|o| &mut o.raw.state)
        {
            state.ap = state.ap.saturating_sub(remaining).max(0);
        }
    }
}
