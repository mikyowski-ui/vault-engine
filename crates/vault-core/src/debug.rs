//! Debugger mutations of a running world, as logged commands.
//!
//! A [`DebugCommand`] names its object by the map object id (`raw.id`,
//! saved with the map and identical on every run of the same log), never by
//! a slot handle. [`WorldHost::apply_debug`] runs each command through the
//! ordinary engine paths: the same variable stores the script opcodes use,
//! the normal map-transition request, ordinary object creation into the
//! player's inventory, and the heal the Doctor skill applies. Values out of
//! range are refused with an error and change nothing.
//!
//! [`DebugCommand::VmBegin`] steps a copy of the object's program. The
//! program list keeps the copy from before the call until the procedure
//! ends, when the stepped one replaces it, so [`DebugCommand::VmAbort`] leaves
//! the program as it was, though what the procedure already did to the world
//! stays. While a procedure is being stepped nothing else may change: every
//! command but [`DebugCommand::VmStep`] and [`DebugCommand::VmAbort`] is
//! refused, and `debug_paused` stays set, so the frame loop does not tick.
use crate::{
    ObjectRef, ScriptEvent, VmObserver, WorldError, WorldHost,
    assets::WorldAssets,
    world::{ScriptRequest, Transition},
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use vault_formats::{
    Prototype,
    map::{ObjectDetail, ObjectState},
    pro::{BasicFlags, Category, ItemSubtype, LockFlags},
};
use vault_vm::{RunOutcome, VmStatus};

/// The largest stack [`DebugCommand::SpawnItem`] creates at once, and the most
/// of an item [`DebugCommand::SetItemCount`] sets.
pub const MAX_SPAWN: u32 = 100_000;
/// The most instructions one [`DebugCommand::VmStep`] runs: the budget of a
/// whole synchronous script run.
pub const MAX_VM_STEP: u32 = 1_000_000;

/// The procedure a [`DebugCommand::FireProc`] or [`DebugCommand::VmBegin`]
/// names: `event` is the SPEC-004 §3.7 event number,
/// [`ScriptEvent::code`]. Only 6 `use_p_proc`, 11 `talk_p_proc`, 14
/// `damage_p_proc`, 15 `map_enter_p_proc` and 18 `destroy_p_proc` may be
/// fired.
pub fn proc_event(event: i32) -> Option<ScriptEvent> {
    [
        ScriptEvent::Use,
        ScriptEvent::Talk,
        ScriptEvent::Damage,
        ScriptEvent::MapEnter,
        ScriptEvent::Destroy,
    ]
    .into_iter()
    .find(|candidate| candidate.code() == event)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DebugCommand {
    /// `set_global_var` on a campaign variable.
    SetGlobalVar {
        index: usize,
        value: i32,
    },
    /// `set_map_var` on the current map's variable.
    SetMapVar {
        index: usize,
        value: i32,
    },
    /// `set_local_var` in the script attached to the object with map
    /// object id `object`.
    SetObjectLocalVar {
        object: u32,
        index: usize,
        value: i32,
    },
    /// A map transition to a numbered map (SPEC-008 §3.10), on the given
    /// elevation and hex, or the map's default entrance without a tile.
    TeleportMap {
        map_number: i32,
        elevation: u32,
        tile: Option<u32>,
    },
    /// Create `quantity` of an item prototype in the player's inventory.
    SpawnItem {
        pid: u32,
        quantity: u32,
    },
    /// Restore a living critter to its maximum HP and clear its crippled
    /// limbs and blindness.
    HealCritter {
        object: u32,
    },
    /// A door's state; `None` leaves that part as it is.
    SetDoor {
        object: u32,
        open: Option<bool>,
        locked: Option<bool>,
        jammed: Option<bool>,
    },
    SetHitPoints {
        object: u32,
        value: i32,
    },
    SetActionPoints {
        object: u32,
        value: i32,
    },
    /// How many of `pid` `owner` carries.
    SetItemCount {
        owner: u32,
        pid: u32,
        count: u32,
    },
    /// Run the object's script procedure for `event` (`use_p_proc` and the
    /// like), with `fixed` as its fixed parameter.
    FireProc {
        object: u32,
        event: i32,
        fixed: i32,
    },
    /// Advance the clock to the next queued event, through `advance_time`.
    /// Refused in combat, where the queue does not run.
    FastForwardTimer,
    /// Move a critter or item to a hex on this map.
    TeleportObject {
        object: u32,
        hex: u32,
        elevation: u32,
    },
    /// Create an object of `pid` on a hex.
    SpawnObject {
        pid: u32,
        hex: u32,
        elevation: u32,
    },
    /// Hold or release the world between frames.
    SetPaused {
        on: bool,
    },
    /// Load the object's procedure for `event` in a debugger VM, to step it
    /// from a frame boundary. The world stays frozen until it ends.
    VmBegin {
        object: u32,
        event: i32,
    },
    /// `count` instructions of the debugger VM, inside critical sections too.
    VmStep {
        count: u32,
    },
    VmAbort,
}

impl std::fmt::Display for DebugCommand {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SetGlobalVar { index, value } => write!(formatter, "GVAR {index} = {value}"),
            Self::SetMapVar { index, value } => write!(formatter, "MVAR {index} = {value}"),
            Self::SetObjectLocalVar {
                object,
                index,
                value,
            } => write!(formatter, "object {object:#x} LVAR {index} = {value}"),
            Self::TeleportMap {
                map_number,
                elevation,
                tile,
            } => match tile {
                Some(tile) => write!(
                    formatter,
                    "teleport to map {map_number} elevation {elevation} hex {tile}"
                ),
                None => write!(formatter, "teleport to map {map_number} (entrance)"),
            },
            Self::SpawnItem { pid, quantity } => {
                write!(formatter, "spawn {quantity} x PID {pid:#010x}")
            }
            Self::HealCritter { object } => write!(formatter, "heal object {object:#x}"),
            Self::SetDoor {
                object,
                open,
                locked,
                jammed,
            } => {
                write!(formatter, "door {object:#x}")?;
                for (name, part) in [("open", open), ("locked", locked), ("jammed", jammed)] {
                    if let Some(on) = part {
                        write!(formatter, " {name} = {on}")?;
                    }
                }
                Ok(())
            }
            Self::SetHitPoints { object, value } => {
                write!(formatter, "object {object:#x} HP = {value}")
            }
            Self::SetActionPoints { object, value } => {
                write!(formatter, "object {object:#x} AP = {value}")
            }
            Self::SetItemCount { owner, pid, count } => {
                write!(
                    formatter,
                    "object {owner:#x} carries {count} x PID {pid:#010x}"
                )
            }
            Self::FireProc {
                object,
                event,
                fixed,
            } => write!(
                formatter,
                "fire {} on object {object:#x} (fixed {fixed})",
                proc_name(*event)
            ),
            Self::FastForwardTimer => write!(formatter, "fast-forward to the next queued event"),
            Self::TeleportObject {
                object,
                hex,
                elevation,
            } => write!(
                formatter,
                "teleport object {object:#x} to elevation {elevation} hex {hex}"
            ),
            Self::SpawnObject {
                pid,
                hex,
                elevation,
            } => write!(
                formatter,
                "spawn PID {pid:#010x} at elevation {elevation} hex {hex}"
            ),
            Self::SetPaused { on: true } => write!(formatter, "pause the world"),
            Self::SetPaused { on: false } => write!(formatter, "resume the world"),
            Self::VmBegin { object, event } => {
                write!(
                    formatter,
                    "step {} of object {object:#x}",
                    proc_name(*event)
                )
            }
            Self::VmStep { count } => write!(formatter, "run {count} stepped instructions"),
            Self::VmAbort => write!(formatter, "abort the stepped procedure"),
        }
    }
}

/// An event number as its procedure's name, for a log line.
fn proc_name(event: i32) -> String {
    proc_event(event).map_or_else(
        || format!("event {event}"),
        |event| String::from_utf8_lossy(event.name()).into_owned(),
    )
}

fn refuse<T>(message: impl Into<String>) -> Result<T, WorldError> {
    Err(WorldError::Invalid(message.into()))
}

type Observer = dyn VmObserver + 'static;
fn watching(observer: &Mutex<Observer>) -> MutexGuard<'_, Observer> {
    observer.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A hex and elevation an object can be placed on.
fn placement(hex: u32, elevation: u32) -> Result<(i32, u8), WorldError> {
    if !(1..40000).contains(&hex) {
        return refuse(format!("hex {hex} outside 1-39999"));
    }
    if elevation > 2 {
        return refuse(format!("elevation {elevation} outside 0-2"));
    }
    Ok((hex as i32, elevation as u8))
}

impl<A: WorldAssets> WorldHost<A> {
    /// Apply one debugger command through the ordinary engine paths. An
    /// out-of-range or inapplicable command is an error and changes
    /// nothing. A teleport is a pending transition the next tick processes,
    /// like any other map change.
    pub fn apply_debug(&mut self, command: &DebugCommand) -> Result<(), WorldError> {
        let stepping = matches!(
            command,
            DebugCommand::VmStep { .. }
                | DebugCommand::VmAbort
                | DebugCommand::SetPaused { on: true }
        );
        if self.debug_vm.is_some() && !stepping {
            return refuse(format!("a stepped procedure holds the world: {command}"));
        }
        match *command {
            DebugCommand::SetGlobalVar { index, value } => {
                let Some(slot) = self
                    .world
                    .gvars
                    .as_mut()
                    .and_then(|array| array.get_mut(index))
                else {
                    return refuse(format!("GVAR {index} does not exist"));
                };
                *slot = value;
            }
            DebugCommand::SetMapVar { index, value } => {
                let Some(slot) = self.world.mvars.get_mut(index) else {
                    return refuse(format!("MVAR {index} does not exist on this map"));
                };
                *slot = value;
            }
            DebugCommand::SetObjectLocalVar {
                object,
                index,
                value,
            } => {
                let reference = self.debug_object(object)?;
                let sid = self.world.get(reference).map_or(-1, |o| o.raw.script_id);
                let sid = u32::try_from(sid)
                    .ok()
                    .filter(|sid| sid >> 24 != 0 && self.world.scripts.contains_key(sid));
                let Some(sid) = sid else {
                    return refuse(format!("object {object:#x} has no object script"));
                };
                let index = i32::try_from(index)
                    .map_err(|_| WorldError::Invalid(format!("LVAR {index} out of range")))?;
                if !self.world.set_local(Some(sid), index, value) {
                    return refuse(format!(
                        "LVAR {index} is outside object {object:#x}'s script slice"
                    ));
                }
            }
            DebugCommand::TeleportMap {
                map_number,
                elevation,
                tile,
            } => {
                // Map 0's transition opens the world map (SPEC-013 §3.22),
                // so it cannot be reached by number.
                if !(1..crate::worldmap::MAP_COUNT as i32).contains(&map_number) {
                    return refuse(format!(
                        "map {map_number} outside 1-{} (0 opens the world map)",
                        crate::worldmap::MAP_COUNT - 1
                    ));
                }
                if elevation > 2 {
                    return refuse(format!("elevation {elevation} outside 0-2"));
                }
                if self.assets.map_path(map_number).is_none() {
                    return refuse(format!("map {map_number} is not available"));
                }
                let hex = match tile {
                    None => -1,
                    Some(tile) if (1..40000).contains(&tile) => tile as i32,
                    Some(tile) => return refuse(format!("hex {tile} outside 1-39999")),
                };
                self.request_transition(Transition {
                    map: map_number,
                    elevation: if tile.is_some() { elevation as i32 } else { -1 },
                    hex,
                    facing: -1,
                });
            }
            DebugCommand::SpawnItem { pid, quantity } => {
                if !(1..=MAX_SPAWN).contains(&quantity) {
                    return refuse(format!("quantity {quantity} outside 1-{MAX_SPAWN}"));
                }
                if Category::from_pid(pid).ok() != Some(Category::Item) {
                    return refuse(format!("PID {pid:#010x} is not an item"));
                }
                let prototype = self.assets.prototype(pid)?;
                self.debug_give(self.world.player, prototype, quantity as i32)?;
            }
            DebugCommand::HealCritter { object } => {
                let reference = self.debug_living_critter(object)?;
                let max = self.world.effective(reference, 7);
                let hp = self.world.hit_points(reference).unwrap_or(0);
                self.heal_critter(reference, max.saturating_sub(hp).max(0));
                // The Doctor skill's limb heal clears these bits.
                if let Some(ObjectState::Critter(state)) =
                    self.world.get_mut(reference).map(|o| &mut o.raw.state)
                {
                    state.results.0 &= !crate::host_skills::CRIPPLED;
                }
            }
            DebugCommand::SetDoor {
                object,
                open,
                locked,
                jammed,
            } => self.debug_set_door(object, open, locked, jammed)?,
            DebugCommand::SetHitPoints { object, value } => {
                let reference = self.debug_living_critter(object)?;
                let max = self.world.max_hit_points(reference);
                if !(0..=max).contains(&value) {
                    return refuse(format!("HP {value} outside 0-{max}"));
                }
                if value == 0 {
                    // Through the death path, as `kill_critter` scripts go.
                    self.kill_critter(reference, 20)?;
                } else {
                    self.world.set_hit_points(reference, value);
                }
            }
            DebugCommand::SetActionPoints { object, value } => {
                let reference = self.debug_living_critter(object)?;
                let max = self.world.effective(reference, 8);
                if !(0..=max).contains(&value) {
                    return refuse(format!("AP {value} outside 0-{max}"));
                }
                if let Some(state) = self.world.critter_state_mut(reference) {
                    state.ap = value;
                }
            }
            DebugCommand::SetItemCount { owner, pid, count } => {
                self.debug_set_item_count(owner, pid, count)?;
            }
            DebugCommand::FireProc {
                object,
                event,
                fixed,
            } => self.debug_fire(object, event, fixed)?,
            DebugCommand::FastForwardTimer => {
                if self.world.combat || self.world.dialogue.open {
                    return refuse("the queue does not run in combat or dialogue");
                }
                let Some(due) = self.world.queue.next_due() else {
                    return refuse("the queue is empty");
                };
                // `advance_time` keeps the clock in 32 bits.
                let now = self.world.clock.ticks;
                let delay = i32::try_from(due.saturating_sub(now))
                    .ok()
                    .filter(|_| now.max(due) <= u64::from(u32::MAX));
                let Some(delay) = delay else {
                    return refuse(format!("tick {due} is out of the clock's reach"));
                };
                self.advance_time(delay)?;
            }
            DebugCommand::TeleportObject {
                object,
                hex,
                elevation,
            } => {
                let (hex, elevation) = placement(hex, elevation)?;
                self.debug_teleport(object, hex, elevation)?;
            }
            DebugCommand::SpawnObject {
                pid,
                hex,
                elevation,
            } => {
                let (hex, elevation) = placement(hex, elevation)?;
                self.debug_spawn(pid, hex, elevation)?;
            }
            DebugCommand::SetPaused { on } => self.debug_paused = on,
            DebugCommand::VmBegin { object, event } => self.debug_vm_begin(object, event)?,
            DebugCommand::VmStep { count } => {
                if !(1..=MAX_VM_STEP).contains(&count) {
                    return refuse(format!("count {count} outside 1-{MAX_VM_STEP}"));
                }
                self.debug_vm_step(count)?;
            }
            DebugCommand::VmAbort => {
                let Some(vm) = self.debug_vm.take() else {
                    return refuse("no procedure is being stepped");
                };
                // As a faulted run: a registration it left open is dropped.
                if self.building_program == Some(vm.id) {
                    self.building_program = None;
                    self.building_animation = None;
                }
                // A dialogue it opened has no script left to run it.
                if self.world.dialogue.open && self.world.dialogue.program == vm.id {
                    self.end_session();
                }
                self.debug_vm_end(self.sid(&vm))?;
            }
        }
        Ok(())
    }
    /// The live object with map object id `id`.
    fn debug_object(&self, id: u32) -> Result<crate::ObjectRef, WorldError> {
        self.world
            .reference_for_id(id)
            .ok_or_else(|| WorldError::Invalid(format!("no object with id {id:#x}")))
    }
    /// The living critter with map object id `id`.
    fn debug_living_critter(&self, id: u32) -> Result<ObjectRef, WorldError> {
        let reference = self.debug_object(id)?;
        if self.world.critter_state(reference).is_none() {
            return refuse(format!("object {id:#x} is not a critter"));
        }
        if self.world.is_dead(reference) {
            return refuse(format!("object {id:#x} is dead"));
        }
        Ok(reference)
    }
    /// A new object of `prototype` on a hex, named as `create_object_sid`
    /// names it.
    fn debug_create(
        &mut self,
        prototype: &Arc<Prototype>,
        hex: i32,
        elevation: u8,
    ) -> Result<ObjectRef, WorldError> {
        let object = self.spawn(prototype.clone(), hex, elevation)?;
        if let Some(target) = self.world.get_mut(object) {
            target.name = self
                .assets
                .prototype_text(prototype, false)
                .unwrap_or_else(|| target.name.clone());
            target.description = self
                .assets
                .prototype_text(prototype, true)
                .unwrap_or_default();
        }
        Ok(object)
    }
    /// `quantity` new items in `owner`'s inventory, as `create_object_sid`
    /// then `add_mult_objs_to_inven`.
    fn debug_give(
        &mut self,
        owner: ObjectRef,
        prototype: Arc<Prototype>,
        quantity: i32,
    ) -> Result<(), WorldError> {
        let (hex, elevation) = self
            .world
            .get(owner)
            .map(|o| (o.raw.hex.unwrap_or(0), o.raw.elevation))
            .ok_or_else(|| WorldError::Invalid("no inventory owner".into()))?;
        let item = self.debug_create(&prototype, hex, elevation)?;
        if let Err(error) = self.world.add_item(owner, item, quantity, true) {
            self.world.erase(item);
            return Err(error);
        }
        Ok(())
    }
    /// Destroy `excess` of `pid` from `owner`'s inventory, as
    /// `rm_mult_objs_from_inven` then `destroy_object`.
    fn debug_take(&mut self, owner: ObjectRef, pid: u32, excess: i32) -> Result<(), WorldError> {
        let mut excess = excess;
        while excess > 0 {
            let carried = self.world.carried(owner, pid);
            let parent = carried.and_then(|item| self.world.get(item)?.owner);
            let (Some(item), Some(parent)) = (carried, parent) else {
                return refuse(format!("PID {pid:#010x} is not carried"));
            };
            let held = self
                .world
                .get(parent)
                .and_then(|o| o.inventory.iter().find(|(entry, _)| *entry == item))
                .map_or(0, |(_, quantity)| *quantity);
            let quantity = held.min(excess);
            let worn = self
                .world
                .get(item)
                .is_some_and(|o| o.raw.flags.contains(BasicFlags::WORN));
            self.hook_removal(owner, item);
            self.world.remove_item(owner, item, quantity)?;
            if worn {
                self.unworn_art(owner);
            }
            self.world.erase(item);
            excess -= quantity;
        }
        Ok(())
    }
    fn debug_set_item_count(&mut self, owner: u32, pid: u32, count: u32) -> Result<(), WorldError> {
        let reference = self.debug_object(owner)?;
        if self.world.critter_state(reference).is_none()
            && self.world.item_subtype(reference) != Some(ItemSubtype::Container)
        {
            return refuse(format!("object {owner:#x} carries nothing"));
        }
        if count > MAX_SPAWN {
            return refuse(format!("count {count} outside 0-{MAX_SPAWN}"));
        }
        if Category::from_pid(pid).ok() != Some(Category::Item) {
            return refuse(format!("PID {pid:#010x} is not an item"));
        }
        let wanted = count as i32;
        // One pass lands short when an empty ammunition pack merges away, and
        // long when a removed container held more of the same PID.
        for _ in 0..8 {
            let held = self.world.inventory_count(reference, pid);
            if held < wanted {
                let prototype = self.assets.prototype(pid)?;
                self.debug_give(reference, prototype, wanted - held)?;
            } else if held > wanted {
                self.debug_take(reference, pid, held - wanted)?;
            } else {
                return Ok(());
            }
        }
        Err(WorldError::Invalid(format!(
            "object {owner:#x} never reached {count} x PID {pid:#010x}"
        )))
    }
    fn debug_set_door(
        &mut self,
        object: u32,
        open: Option<bool>,
        locked: Option<bool>,
        jammed: Option<bool>,
    ) -> Result<(), WorldError> {
        let door = self.debug_object(object)?;
        let target = self.world.get_mut(door);
        let Some(target) = target.filter(|target| {
            matches!(
                target.raw.state,
                ObjectState::Other {
                    detail: ObjectDetail::Door(_),
                    ..
                }
            )
        }) else {
            return refuse(format!("object {object:#x} is not a door"));
        };
        if let Some(mut lock) = target.lock() {
            for (part, flag) in [(locked, LockFlags::LOCKED), (jammed, LockFlags::JAMMED)] {
                match part {
                    Some(true) => lock.0 |= flag.0,
                    Some(false) => lock.0 &= !flag.0,
                    None => {}
                }
            }
            target.set_lock(lock);
        }
        if let Some(open) = open {
            self.debug_swing_door(door, open);
        }
        Ok(())
    }
    /// A door opened or closed at once: the SPEC-007 §3.11.4 finishing
    /// callback with no sequence. Routes and sight read the flags it sets.
    fn debug_swing_door(&mut self, door: ObjectRef, open: bool) {
        // A swing under way ends first: its finishing callback would undo this.
        self.clear_animation(door);
        let Some(found) = self.world.get(door) else {
            return;
        };
        let art = self.assets.object_art(found.raw.art_id).ok();
        let count = art.as_ref().map_or(2, |art| art.frame_count().max(1));
        let (direction, frame) = (usize::from(found.raw.orientation), found.raw.frame);
        let shift = |frame: usize| {
            art.as_deref()
                .and_then(|art| art.frame(direction, frame).or_else(|| art.frame(0, frame)))
                .map_or([0; 2], |frame| frame.displacement.map(i32::from))
        };
        let passed = if open { frame + 1..count } else { 1..frame + 1 };
        let sign = if open { 1 } else { -1 };
        let total = passed.clone().fold([0; 2], |[x, y], frame| {
            let [dx, dy] = shift(frame);
            [x + sign * dx, y + sign * dy]
        });
        if !passed.is_empty() {
            self.world.shift_offset(door, total);
        }
        if !open {
            self.close_door_instantly(door);
            return;
        }
        if let Some(object) = self.world.get_mut(door) {
            object.raw.frame = count - 1;
            object.raw.flags.0 |= BasicFlags::NONBLOCKING.0
                | BasicFlags::LIGHT_THROUGH.0
                | BasicFlags::PROJECTILE_THROUGH.0;
            if let Some(mut lock) = object.lock() {
                lock.0 |= LockFlags::OPEN.0;
                object.set_lock(lock);
            }
        }
        self.world.rebuild_lighting();
    }
    /// Move a placed critter or item, as `move_to`. The player goes through
    /// its own placement, which ends combat on a change of elevation.
    fn debug_teleport(&mut self, id: u32, hex: i32, elevation: u8) -> Result<(), WorldError> {
        let reference = self.debug_object(id)?;
        let Some(object) = self.world.get(reference) else {
            return refuse(format!("no object with id {id:#x}"));
        };
        let category = object.category();
        if !matches!(category, Category::Critter | Category::Item) {
            return refuse(format!("object {id:#x} is not a critter or an item"));
        }
        if object.owner.is_some() || object.raw.hex.is_none() {
            return refuse(format!("object {id:#x} is not on the map"));
        }
        let facing = object.raw.orientation;
        if category == Category::Critter
            && self
                .world
                .movement_blocker(hex, elevation, reference)
                .is_some()
        {
            return refuse(format!("hex {hex} is blocked"));
        }
        if reference == self.world.player {
            return self.place_player(hex, elevation, facing);
        }
        self.clear_animation(reference);
        self.world.move_object(reference, hex, elevation, false)
    }
    /// A new object on a hex with its prototype's script, as
    /// `create_object_sid`.
    fn debug_spawn(&mut self, pid: u32, hex: i32, elevation: u8) -> Result<(), WorldError> {
        let category = Category::from_pid(pid).ok();
        if category.is_none() || category == Some(Category::Tile) {
            return refuse(format!("PID {pid:#010x} is not an object"));
        }
        let prototype = self.assets.prototype(pid)?;
        if category == Some(Category::Critter)
            && self.world.movement_blocker(hex, elevation, 0).is_some()
        {
            return refuse(format!("hex {hex} is blocked"));
        }
        let object = self.debug_create(&prototype, hex, elevation)?;
        if let Some(script) = prototype.script.filter(|script| *script != -1) {
            let file = script & 0xff_ffff;
            let stream = match prototype.category {
                Category::Critter => 4,
                Category::Item | Category::Scenery => 3,
                _ => 0,
            };
            let locals = self.assets.local_count(file);
            if let Err(error) = self.world.add_script(object, file, stream, locals) {
                self.world.erase(object);
                return Err(error);
            }
        }
        Ok(())
    }
    /// The event and script a debugger request names, when the dispatcher
    /// would run that procedure: nothing here is a request it would skip, or
    /// one that falls back to another procedure.
    fn debug_script(&self, object: u32, event: i32) -> Result<(ScriptEvent, u32), WorldError> {
        let Some(event) = proc_event(event) else {
            return refuse(format!("event {event} cannot be fired"));
        };
        let reference = self.debug_object(object)?;
        if !self.world.scripts_enabled {
            return refuse("scripts are disabled");
        }
        let script = self.world.script_for(reference);
        let Some((sid, script)) = script.and_then(|sid| Some((sid, self.world.scripts.get(&sid)?)))
        else {
            return refuse(format!("object {object:#x} has no script"));
        };
        let name = event.name();
        let present = if script.initialized {
            let Some(vm) = self.programs.get(&script.program) else {
                return refuse(format!("object {object:#x}'s program is unavailable"));
            };
            let held =
                VmStatus::ERROR | VmStatus::WAITING | VmStatus::ATTACHED | VmStatus::SPAWN_BLOCKED;
            if vm.status.contains(held)
                || self.active.contains(&vm.id)
                || self.host_busy.contains(&vm.id)
            {
                return refuse(format!("object {object:#x}'s program cannot run now"));
            }
            vm.program.find_procedure(name, true).is_some()
        } else {
            if self.scripts_gated() {
                return refuse("scripts are gated");
            }
            let (_, program) = self.assets.program(script.file)?;
            program.find_procedure(name, true).is_some()
        };
        if !present {
            return refuse(format!(
                "object {object:#x}'s script has no {}",
                String::from_utf8_lossy(name)
            ));
        }
        Ok((event, sid))
    }
    /// SPEC-006 §3.8: a talk needs its session before `talk_p_proc` runs.
    fn debug_talk_begin(&mut self, sid: u32) -> Result<(), WorldError> {
        if self.world.combat || self.world.dialogue.open {
            return refuse("dialogue entry refused in combat or in a dialogue");
        }
        self.world.background_enabled = false;
        self.world.dialogue.target = self.world.scripts.get(&sid).map_or(0, |s| s.owner);
        self.world.dialogue.talking = true;
        Ok(())
    }
    /// SPEC-006 §3.8: the talk returns at once when no dialogue opened.
    fn debug_talk_end(&mut self) -> Result<(), WorldError> {
        if !self.world.dialogue.talking || self.world.dialogue.open {
            return Ok(());
        }
        self.world.dialogue.talking = false;
        self.world.background_enabled = true;
        self.map_update()
    }
    /// A use comes from the player, as a click's; the other procedures' host
    /// requests name no source the debugger has.
    fn debug_source(&self, event: ScriptEvent) -> ObjectRef {
        if event == ScriptEvent::Use {
            self.world.player
        } else {
            0
        }
    }
    fn debug_fire(&mut self, object: u32, event: i32, fixed: i32) -> Result<(), WorldError> {
        let (event, script) = self.debug_script(object, event)?;
        let talk = event == ScriptEvent::Talk;
        if talk {
            self.debug_talk_begin(script)?;
        }
        let request = ScriptRequest {
            script,
            event,
            source: self.debug_source(event),
            target: 0,
            fixed,
            action: -1,
            only_present: false,
        };
        // SPEC-006 §3.5: a first request only runs the initial stream, so
        // the procedure takes a second one.
        let first = !self
            .world
            .scripts
            .get(&script)
            .is_some_and(|s| s.initialized);
        let mut result = self.dispatch(request.clone());
        if first && result.as_ref().is_ok_and(|run| !run.pending) {
            result = self.dispatch(request);
        }
        let ended = if talk { self.debug_talk_end() } else { Ok(()) };
        result?;
        ended?;
        self.drain_requests()
    }
    /// SPEC-004 §3.3: the procedure starts as a synchronous host call, which
    /// does not set critical state from its procedure flag.
    fn debug_vm_begin(&mut self, object: u32, event: i32) -> Result<(), WorldError> {
        let (event, sid) = self.debug_script(object, event)?;
        let Some(script) = self.world.scripts.get(&sid).filter(|s| s.initialized) else {
            return refuse(format!("object {object:#x}'s script has not run yet"));
        };
        let owner = script.owner;
        let Some(mut vm) = self.programs.get(&script.program).cloned() else {
            return refuse(format!("object {object:#x}'s program is unavailable"));
        };
        let procedure = vm.program.find_procedure(event.name(), true);
        let index = procedure.filter(|index| *index != 0).unwrap_or(1);
        vm.prepare_host_call(index, true)
            .map_err(|error| WorldError::Invalid(format!("procedure not entered: {error}")))?;
        if event == ScriptEvent::Talk {
            self.debug_talk_begin(sid)?;
        }
        let source = self.debug_source(event);
        if let Some(script) = self.world.scripts.get_mut(&sid) {
            script.overridden = false;
            script.event = event.code();
            script.source = source;
            script.target = owner;
            script.fixed = 0;
            script.action = -1;
        }
        self.debug_vm = Some(vm);
        self.debug_paused = true;
        Ok(())
    }
    /// `count` instructions, each through `Vm::step` so a critical section
    /// stops between them too, and each shown to an attached observer. Any
    /// outcome but a spent budget ends the session as it ends a synchronous
    /// run. A program that now waits or is blocked goes back with the rest of
    /// its procedure to run in the world, because a frozen world could never
    /// release it.
    fn debug_vm_step(&mut self, count: u32) -> Result<(), WorldError> {
        let Some(mut vm) = self.debug_vm.take() else {
            return refuse("no procedure is being stepped");
        };
        // Out of the list while it runs, as in `dispatch`.
        let Some(saved) = self.programs.remove(&vm.id) else {
            self.debug_paused = false;
            return refuse("the stepped program is gone");
        };
        let observer = self.vm_observer.clone();
        self.active.insert(vm.id);
        let mut outcome = Ok(RunOutcome::Budget);
        for _ in 0..count {
            if let Some(observer) = &observer {
                watching(observer).before_step(&vm);
            }
            outcome = vm.step(self);
            if let Some(observer) = &observer {
                watching(observer).after_step(&vm, &outcome);
            }
            if !matches!(outcome, Ok(RunOutcome::Budget)) {
                break;
            }
        }
        self.active.remove(&vm.id);
        if matches!(outcome, Ok(RunOutcome::Budget)) {
            self.programs.insert(saved.id, saved);
            self.debug_vm = Some(vm);
            return Ok(());
        }
        let sid = self.sid(&vm);
        if let Err(error) = &outcome {
            self.program_faulted(&mut vm, error);
            self.world.diagnostic(format!("stepped procedure: {error}"));
        }
        self.programs.insert(vm.id, vm);
        self.debug_vm_end(sid)
    }
    /// The session is over: the world runs again, and what a request's end
    /// does in `dispatch` and its callers is done.
    fn debug_vm_end(&mut self, sid: Option<u32>) -> Result<(), WorldError> {
        self.debug_paused = false;
        if let Some(sid) = sid {
            self.departed.remove(&sid);
            if let Some(script) = self.world.scripts.get_mut(&sid) {
                script.source = 0;
            }
        }
        self.debug_talk_end()?;
        self.drain_requests()
    }
}

impl crate::WorldState {
    /// A critter's effective maximum HP (stat 7), as the heal paths cap it.
    pub fn max_hit_points(&self, critter: crate::ObjectRef) -> i32 {
        self.effective(critter, 7)
    }
}
