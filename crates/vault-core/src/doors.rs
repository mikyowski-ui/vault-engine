//! The `enhanced` preset's auto-open doors: the player's walk may pass an
//! unlocked door the way SPEC-007 §3.1 and §3.11.1 let other critters
//! pass one. Engine extension, not part of any specification; under
//! Vanilla nothing here is reachable, and `SAVE.DAT` never changes.
//!
//! A door the player may auto-open is a closed scenery door with a
//! prototype whose lock word is neither locked nor jammed (SPEC-003
//! §2.6, SPEC-013 §2.3), outside combat, and either without a script
//! instance or one the player has opened by hand in this game: a use from
//! the player's own click whose `use_p_proc` let it open. Traps are
//! script-driven, so a scripted door the player has never opened stays a
//! blocker. The doors opened by hand are kept in [`EnhancedState`] and
//! saved in the slot's [`SIDECAR`](crate::companion::SIDECAR).
use crate::{EnginePreset, ObjectRef, WorldHost, WorldState, assets::WorldAssets};
use std::collections::BTreeSet;
use vault_formats::pro::LockFlags;

/// The sidecar table of the doors opened by hand.
pub const TABLE: &str = "opened_doors";

/// A door the player opened by hand, keyed stably across map changes and
/// saves: the map's working `.SAV` name (SPEC-008 §3.10, the name its
/// saved copy has in every slot) and the door's unique object identifier
/// (SPEC-003 §2.10), which that saved copy keeps. The PID is checked too,
/// so an identifier given again to another object on a map rebuilt from
/// its `.MAP` (SPEC-014 §3.35 step 5) never matches.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DoorKey {
    pub map: String,
    pub id: u32,
    pub pid: u32,
}

/// The `enhanced` preset's game state beyond the companions' distances;
/// empty under Vanilla.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnhancedState {
    /// The doors the player opened by hand in this game.
    pub opened_doors: BTreeSet<DoorKey>,
}
impl EnhancedState {
    pub fn is_empty(&self) -> bool {
        self.opened_doors.is_empty()
    }
}

/// The sidecar's `[opened_doors]` table, one `"<map>:<object id>" = <PID>`
/// line per door; nothing when there are none.
pub fn write_table(doors: &BTreeSet<DoorKey>) -> String {
    if doors.is_empty() {
        return String::new();
    }
    let mut text = format!("[{TABLE}]\n");
    for door in doors {
        text += &format!("\"{}:{}\" = {}\n", door.map, door.id, door.pid);
    }
    text
}

/// The doors a sidecar's `[opened_doors]` table records (other tables are
/// [`crate::companion::parse_sidecar`]'s); a malformed line is an error
/// naming it.
pub fn parse_table(text: &str) -> Result<BTreeSet<DoorKey>, String> {
    let mut doors = BTreeSet::new();
    let mut inside = false;
    for (index, line) in text.lines().enumerate() {
        let at = |message: String| {
            format!(
                "{} line {}: {message}",
                crate::companion::SIDECAR,
                index + 1
            )
        };
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            inside = name.trim() == TABLE;
            continue;
        }
        if !inside {
            continue;
        }
        let expected = || at("expected `\"<map>:<object id>\" = <PID>`".into());
        let (key, pid) = line.split_once('=').ok_or_else(expected)?;
        let key = key
            .trim()
            .strip_prefix('"')
            .and_then(|key| key.strip_suffix('"'))
            .ok_or_else(expected)?;
        let (map, id) = key.rsplit_once(':').ok_or_else(expected)?;
        if !crate::host_save::valid_companion_name(map) {
            return Err(at(format!("`{map}` is not a working map name")));
        }
        let id: u32 = id
            .parse()
            .map_err(|_| at(format!("`{id}` is not an object id")))?;
        let pid: u32 = pid
            .trim()
            .parse()
            .map_err(|_| at(format!("`{}` is not a PID", pid.trim())))?;
        doors.insert(DoorKey {
            map: map.to_string(),
            id,
            pid,
        });
    }
    Ok(doors)
}

impl WorldState {
    /// The stable key of a placed object on the current map, when the map
    /// has a working name.
    pub fn door_key(&self, door: ObjectRef) -> Option<DoorKey> {
        let object = self.get(door)?;
        Some(DoorKey {
            map: self.current_map_name()?,
            id: object.raw.id,
            pid: object.prototype.as_ref()?.pid,
        })
    }
    /// The player's half of the auto-door permission
    /// ([`WorldState::may_auto_open`] checks that `door` is an unlocked
    /// scenery door): the enhanced preset, outside combat, and a closed
    /// door (frame 0 without the open bit, so its use opens it) that is
    /// neither locked nor jammed, with no script instance or opened by
    /// hand before.
    pub(crate) fn player_may_auto_open(&self, door: ObjectRef) -> bool {
        if self.rules.preset != EnginePreset::Enhanced || self.combat {
            return false;
        }
        let Some(object) = self.get(door) else {
            return false;
        };
        let shut = LockFlags(LockFlags::OPEN.0 | LockFlags::LOCKED.0 | LockFlags::JAMMED.0);
        object.raw.frame == 0
            && object.lock().is_some_and(|lock| lock.0 & shut.0 == 0)
            && (self.script_for(door).is_none()
                || self
                    .door_key(door)
                    .is_some_and(|key| self.enhanced.opened_doors.contains(&key)))
    }
}

impl<A: WorldAssets> WorldHost<A> {
    /// A door use from the player's own click opened `door`: remember it
    /// (the enhanced preset only).
    pub(crate) fn opened_by_hand(&mut self, door: ObjectRef) {
        if self.preset() != EnginePreset::Enhanced {
            return;
        }
        if let Some(key) = self.world.door_key(door) {
            self.world.enhanced.opened_doors.insert(key);
        }
    }
}

#[cfg(test)]
#[path = "doors_tests.rs"]
mod tests;
