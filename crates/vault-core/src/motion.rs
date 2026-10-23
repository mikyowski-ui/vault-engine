//! SPEC-007 §3.11 non-combat animation: the sequences the world itself
//! starts outside an attack. Walking and running (§3.11.1), interaction
//! reaches (§3.11.3) and doors and containers (§3.11.4) act on world
//! objects as their entries start and step, under the §3.10.1 start rule.
use crate::{
    ObjectAction, ObjectRef,
    choreography::{Frames, Refused, Schedule},
};

/// The pixel step to the next hex in each direction, NE through NW
/// (§3.11.1).
pub const HEX_STEPS: [[i32; 2]; 6] = [
    [16, -12],
    [32, 0],
    [16, 12],
    [-16, 12],
    [-32, 0],
    [-16, -12],
];

/// §3.11.1 a mover crosses into the next hex in `direction` once its
/// displacement reaches that hex's pixel step on either axis: at least a
/// positive step, at most a negative one. A zero step is never reached.
pub fn crosses(displacement: [i32; 2], direction: u8) -> bool {
    let step = HEX_STEPS[usize::from(direction % 6)];
    (0..2).any(|axis| match step[axis].signum() {
        1 => displacement[axis] >= step[axis],
        -1 => displacement[axis] <= step[axis],
        _ => false,
    })
}

/// Where a walk goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Goal {
    Hex(i32),
    /// Next to an object (the route's last step is dropped).
    Object(ObjectRef),
}

/// A walk or run entry (§3.11.1): the request, then the route it plans
/// when it starts and the frame it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Walk {
    pub goal: Goal,
    /// A run request; the mover may walk instead.
    pub run: bool,
    /// The most hexes the route takes (the action points in combat).
    pub limit: usize,
    pub route: Vec<u8>,
    pub cursor: usize,
    /// The hex a blocked route is rebuilt toward.
    pub destination: i32,
    /// The walk (1) or run (19) animation the entry shows.
    pub animation: u8,
    pub frame: usize,
    pub interval_ms: u64,
}
impl Walk {
    pub fn new(goal: Goal, run: bool, limit: usize) -> Self {
        Self {
            goal,
            run,
            limit,
            route: Vec::new(),
            cursor: 0,
            destination: -1,
            animation: 1,
            frame: 0,
            interval_ms: 100,
        }
    }
}

/// A callback entry (instant): the checks and the interaction steps of
/// §3.11.3, and the door and container steps of §3.11.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    /// The owner must stand next to `target`; the sequence stops if not.
    Adjacent { target: ObjectRef },
    /// In combat the owner pays 3 AP; the sequence stops without them.
    CombatPoints,
    /// SPEC-006 §3.8 the pickup.
    Pickup { item: ObjectRef },
    /// §3.11.4 the use of a door or scenery, or SPEC-006 §3.8 the use of
    /// another object.
    Use { target: ObjectRef },
    /// SPEC-006 §3.8 the use of `item` on `target`.
    UseOn { item: ObjectRef, target: ObjectRef },
    /// §3.11.4 the container use.
    UseContainer { container: ObjectRef },
    /// SPEC-013 §3.16 the loot request.
    Loot { target: ObjectRef },
    /// An approach ended: the request's own stage runs (talk).
    Arrive {
        target: ObjectRef,
        action: ObjectAction,
    },
    /// SPEC-013 §3.2 step 5: the skill-on stage of §3.3, after the
    /// manipulation animation.
    SkillStage { target: ObjectRef, skill: i32 },
    /// §3.11.4 the open bit of a door, set or cleared.
    OpenBit { open: bool },
    /// §3.11.4 the finishing callback, for an object being opened or
    /// closed.
    Finish { open: bool },
}

/// How a script animation plays (SPEC-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Play {
    Forward,
    Reverse,
    /// Repeating without end, until something cancels it.
    Forever,
}

/// What a world sequence's entry does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Motion {
    /// Face a hex or an object, in the standing pose for the current
    /// weapon code (instant).
    Turn {
        toward: Goal,
    },
    /// A sound effect on the owner (instant); an empty name is silent.
    Sound {
        name: Vec<u8>,
    },
    /// A critter animation played once from frame 0 (timed). The art and
    /// frames are resolved for the owner's facing when it starts.
    Animate {
        animation: u8,
        /// The weapon code shown, when not the art's own (take out).
        weapon: Option<u8>,
        art: u32,
        frames: Frames,
    },
    /// A door's or container's animation 0 from its current frame,
    /// forward to the last frame or back to frame 0 (timed).
    Swing {
        forward: bool,
        interval_ms: u64,
    },
    /// A script animation (SPEC-005): a critter animation by code, or a
    /// non-critter's own art, resolved when it starts (timed).
    Play {
        animation: u8,
        play: Play,
        art: u32,
        frames: Frames,
    },
    /// The standing pose for the current weapon code (instant).
    Stand,
    /// New art at frame 0 (instant).
    SetArt {
        art: u32,
    },
    /// A walk or run (timed).
    Walk(Box<Walk>),
    Call(Call),
}

/// One of the world's sequences.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MotionSequence {
    pub schedule: Schedule<ObjectRef, Motion>,
    /// Critter owners return to standing when it ends (§3.10.1); ladder
    /// climbing is registered without this.
    pub reset: bool,
    /// SPEC-005 request flag 512: any later sequence naming one of its
    /// critters ends it (§3.11.2).
    pub insignificant: bool,
    pub id: u64,
    /// Registered while combat was active: it counts toward the SPEC-014
    /// §3.27 combat lock until it ends (`clicks::CombatLock`).
    pub combat: bool,
}
impl MotionSequence {
    pub fn new() -> Self {
        Self {
            reset: true,
            ..Self::default()
        }
    }
    pub fn push(&mut self, owner: ObjectRef, delay: i32, motion: Motion) -> Result<(), Refused> {
        self.schedule.push(owner, delay, motion, false)
    }
    /// A forced callback: it still runs when the sequence stops early.
    pub fn push_forced(
        &mut self,
        owner: ObjectRef,
        delay: i32,
        motion: Motion,
    ) -> Result<(), Refused> {
        self.schedule.push(owner, delay, motion, true)
    }
    /// Whether an entry of the sequence belongs to `object`.
    pub fn names(&self, object: ObjectRef) -> bool {
        self.schedule
            .entries()
            .iter()
            .any(|entry| entry.owner == object)
    }
    /// SPEC-007 §3.10.1 "One sequence per owner": whether a non-callback
    /// entry of the sequence, started or not, belongs to `object`.
    /// Callback entries are never checked, so a sequence made only of
    /// callbacks holds nothing.
    pub fn owns(&self, object: ObjectRef) -> bool {
        self.schedule
            .entries()
            .iter()
            .any(|entry| entry.owner == object && !matches!(entry.action, Motion::Call(_)))
    }
    /// The distinct owners, in entry order.
    pub fn owners(&self) -> Vec<ObjectRef> {
        let mut owners = Vec::new();
        for entry in self.schedule.entries() {
            if !owners.contains(&entry.owner) {
                owners.push(entry.owner);
            }
        }
        owners
    }
    /// The distinct owners of non-callback entries, in entry order (the
    /// objects the sequence holds under §3.10.1).
    pub fn owned(&self) -> Vec<ObjectRef> {
        let mut owners = Vec::new();
        for entry in self.schedule.entries() {
            if !matches!(entry.action, Motion::Call(_)) && !owners.contains(&entry.owner) {
                owners.push(entry.owner);
            }
        }
        owners
    }
    /// SPEC-005 `anim_busy`: an entry of `object`'s still to finish that is
    /// neither a sound nor a callback, unless the sequence is a lone
    /// standing action.
    pub fn busy(&self, object: ObjectRef) -> bool {
        let schedule = &self.schedule;
        let entries = schedule.entries();
        if entries.len() == 1 && entries[0].action == Motion::Stand {
            return false;
        }
        entries.iter().enumerate().any(|(index, entry)| {
            entry.owner == object
                && !matches!(entry.action, Motion::Sound { .. } | Motion::Call(_))
                && (!schedule.started(index) || schedule.running(index))
        })
    }
    /// Whether `object` walks or runs in an entry still to finish.
    pub fn walking(&self, object: ObjectRef) -> bool {
        let schedule = &self.schedule;
        schedule.entries().iter().enumerate().any(|(index, entry)| {
            entry.owner == object
                && matches!(entry.action, Motion::Walk(_))
                && (!schedule.started(index) || schedule.running(index))
        })
    }
}
