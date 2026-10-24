//! Debugger mutations of a running world, as logged commands.
//!
//! A [`DebugCommand`] names its object by the map object id (`raw.id`,
//! saved with the map and identical on every run of the same log), never by
//! a slot handle. [`WorldHost::apply_debug`] runs each command through the
//! ordinary engine paths: the same variable stores the script opcodes use,
//! the normal map-transition request, ordinary object creation into the
//! player's inventory, and the heal the Doctor skill applies. Values out of
//! range are refused with an error and change nothing.
use crate::{WorldError, WorldHost, assets::WorldAssets, world::Transition};
use serde::{Deserialize, Serialize};
use vault_formats::{map::ObjectState, pro::Category};

/// The largest stack [`DebugCommand::SpawnItem`] creates at once.
pub const MAX_SPAWN: u32 = 100_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DebugCommand {
    /// `set_global_var` on a campaign variable.
    SetGlobalVar { index: usize, value: i32 },
    /// `set_map_var` on the current map's variable.
    SetMapVar { index: usize, value: i32 },
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
    SpawnItem { pid: u32, quantity: u32 },
    /// Restore a living critter to its maximum HP and clear its crippled
    /// limbs and blindness.
    HealCritter { object: u32 },
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
        }
    }
}

fn refuse<T>(message: impl Into<String>) -> Result<T, WorldError> {
    Err(WorldError::Invalid(message.into()))
}

impl<A: WorldAssets> WorldHost<A> {
    /// Apply one debugger command through the ordinary engine paths. An
    /// out-of-range or inapplicable command is an error and changes
    /// nothing. A teleport is a pending transition the next tick processes,
    /// like any other map change.
    pub fn apply_debug(&mut self, command: &DebugCommand) -> Result<(), WorldError> {
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
                let player = self.world.player;
                let (hex, elevation) = self
                    .world
                    .get(player)
                    .map(|o| (o.raw.hex.unwrap_or(0), o.raw.elevation))
                    .ok_or_else(|| WorldError::Invalid("no player".into()))?;
                // As `create_object_sid` then `add_mult_objs_to_inven`.
                let item = self.spawn(prototype.clone(), hex, elevation)?;
                if let Some(target) = self.world.get_mut(item) {
                    target.name = self
                        .assets
                        .prototype_text(&prototype, false)
                        .unwrap_or_else(|| target.name.clone());
                    target.description = self
                        .assets
                        .prototype_text(&prototype, true)
                        .unwrap_or_default();
                }
                if let Err(error) = self.world.add_item(player, item, quantity as i32, true) {
                    self.world.erase(item);
                    return Err(error);
                }
            }
            DebugCommand::HealCritter { object } => {
                let reference = self.debug_object(object)?;
                if !matches!(
                    self.world.get(reference).map(|o| &o.raw.state),
                    Some(ObjectState::Critter(_))
                ) {
                    return refuse(format!("object {object:#x} is not a critter"));
                }
                if self.world.is_dead(reference) {
                    return refuse(format!("object {object:#x} is dead"));
                }
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
        }
        Ok(())
    }
    /// The live object with map object id `id`.
    fn debug_object(&self, id: u32) -> Result<crate::ObjectRef, WorldError> {
        self.world
            .reference_for_id(id)
            .ok_or_else(|| WorldError::Invalid(format!("no object with id {id:#x}")))
    }
}

impl crate::WorldState {
    /// A critter's effective maximum HP (stat 7), as the heal paths cap it.
    pub fn max_hit_points(&self, critter: crate::ObjectRef) -> i32 {
        self.effective(critter, 7)
    }
}
