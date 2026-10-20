//! SPEC-014 §3.35 the revisit rules. Once a map's saved copy has loaded,
//! the whole game hours since its last visit unjam its locks (a day or
//! more), calm and heal its living critters (an hour or more), and turn
//! its old corpses into remains (more than six days).
use crate::{
    ObjectRef, RandomSource, WorldError, assets::WorldAssets, critical::maneuver, host::WorldHost,
};
use vault_formats::pro::{Category, CritterFlags, LockFlags, PrototypeData};

/// Game ticks in an hour.
const HOUR: u64 = 36_000;
/// HP healed for every three whole hours.
const HEALING: i32 = 14;
/// Kill types (SPEC-003 §2.5).
const RAT: i32 = 7;
const ROBOT: i32 = 10;
const MANTIS: i32 = 12;
/// Gizmo's corpse becomes item PID 213, the Remains of Gizmo, and takes
/// its inventory with it.
const GIZMO: u32 = 0x0100_0031;
const GIZMO_REMAINS: u32 = 213;
/// Every other corpse becomes miscellaneous PID 4 (`rscorpbl.frm`).
const REMAINS: u32 = 0x0500_0004;

impl<A: WorldAssets> WorldHost<A> {
    /// The revisit rules, when the current game time is not earlier than
    /// the map's recorded last visit.
    pub(crate) fn revisit(&mut self) -> Result<(), WorldError> {
        let now = self.world.clock.ticks;
        let last = u64::from(self.world.map.header.last_visit);
        let Some(hours) = now.checked_sub(last).map(|elapsed| elapsed / HOUR) else {
            return Ok(());
        };
        if hours >= 24 {
            self.unjam_locks();
        }
        if hours >= 1 {
            self.calm_and_heal(hours);
        }
        // The reference's later test for more than 14 days, which would
        // remove miscellaneous PID 0x0500000B, is never reached.
        if hours > 144 {
            self.leave_remains()?;
        }
        Ok(())
    }
    /// A day or more: every lock on the map is unjammed.
    fn unjam_locks(&mut self) {
        let objects: Vec<_> = self.world.iter().map(|(reference, _)| reference).collect();
        for object in objects {
            if let Some(found) = self.world.get_mut(object)
                && let Some(mut lock) = found.lock()
                && lock.contains(LockFlags::JAMMED)
            {
                lock.0 &= !LockFlags::JAMMED.0;
                found.set_lock(lock);
            }
        }
    }
    /// The kill type of a critter.
    fn kill_type(&self, critter: ObjectRef) -> Option<i32> {
        self.world
            .get(critter)?
            .character
            .as_ref()
            .map(|sheet| sheet.kill_category)
    }
    /// The critters the rules visit: neither the player nor a party member.
    fn residents(&self) -> Vec<ObjectRef> {
        self.world
            .iter()
            .filter(|(reference, object)| {
                object.category() == Category::Critter
                    && *reference != self.world.player
                    && !self.world.party.contains(reference)
            })
            .map(|(reference, _)| reference)
            .collect()
    }
    /// An hour or more: each living critter stops fleeing, and unless it
    /// is a robot, below its maximum HP it heals `14 * trunc(h / 3)` HP, up
    /// to that maximum.
    fn calm_and_heal(&mut self, hours: u64) {
        let amount = i32::try_from(hours / 3)
            .unwrap_or(i32::MAX)
            .saturating_mul(HEALING);
        for critter in self.residents() {
            if self.world.is_dead(critter) {
                continue;
            }
            if let Some(state) = self.world.critter_state_mut(critter) {
                state.maneuvers &= !maneuver::FLEEING;
            }
            if self.kill_type(critter) == Some(ROBOT) {
                continue;
            }
            let context = self.world.context(critter);
            let Some(object) = self.world.get_mut(critter) else {
                continue;
            };
            let Some(sheet) = object.character.as_mut() else {
                continue;
            };
            if sheet.current_hp < sheet.stat(7, context) {
                sheet.heal(amount, context);
            }
            let hp = sheet.current_hp;
            if let vault_formats::map::ObjectState::Critter(state) = &mut object.raw.state {
                state.hp = hp;
            }
        }
    }
    /// More than six days: each dead critter other than the player that is
    /// not a robot becomes remains on its hex, in object order. Gizmo's
    /// corpse becomes the Remains of Gizmo and keeps its inventory, which
    /// goes with it; every other corpse first drops its inventory. One
    /// draw picks the remains' frame: under the safe rule `random(0, 2)`,
    /// which stays within its group of three, plus 6 for a critter whose
    /// prototype has behavioral flag `0x800`, else plus 3 except for rats
    /// and mantises. A frame past the art's last keeps frame 0.
    fn leave_remains(&mut self) -> Result<(), WorldError> {
        let corpses: Vec<_> =
            self.residents()
                .into_iter()
                .filter(|critter| {
                    self.world.is_dead(*critter)
                        && self.kill_type(*critter) != Some(ROBOT)
                        && self.world.get(*critter).is_some_and(|object| {
                            object.owner.is_none() && object.raw.hex.is_some()
                        })
                })
                .collect();
        for corpse in corpses {
            let Some(object) = self.world.get(corpse) else {
                continue;
            };
            let (Some(hex), elevation, pid) =
                (object.raw.hex, object.raw.elevation, object.raw.pid)
            else {
                continue;
            };
            let flat = object.prototype.as_deref().is_some_and(|prototype| {
                matches!(&prototype.data, PrototypeData::Critter(critter)
                    if critter.flags.contains(CritterFlags::FLAT))
            });
            let gizmo = pid == GIZMO;
            let prototype = match self
                .assets
                .prototype(if gizmo { GIZMO_REMAINS } else { REMAINS })
            {
                Ok(prototype) => prototype,
                Err(error) => {
                    self.world
                        .diagnostic(format!("no remains for corpse {corpse}: {error}"));
                    continue;
                }
            };
            if !gizmo {
                let items = object.inventory.clone();
                for (item, quantity) in items {
                    self.world.remove_item(corpse, item, quantity)?;
                    self.world.move_object(item, hex, elevation, false)?;
                }
            }
            let remains = self.world.spawn(prototype, hex, elevation)?;
            let draw =
                usize::try_from(RandomSource::range(&mut self.world.rng, 0, 2)?).unwrap_or(0);
            let frame = if flat {
                draw + 6
            } else if matches!(self.kill_type(corpse), Some(RAT | MANTIS)) {
                draw
            } else {
                draw + 3
            };
            let frames = self
                .world
                .get(remains)
                .and_then(|object| self.assets.object_art(object.raw.art_id).ok())
                .map_or(0, |art| art.frame_count());
            if frame < frames
                && let Some(object) = self.world.get_mut(remains)
            {
                object.raw.frame = frame;
            }
            self.clear_animation(corpse);
            self.world.erase(corpse);
        }
        Ok(())
    }
}
