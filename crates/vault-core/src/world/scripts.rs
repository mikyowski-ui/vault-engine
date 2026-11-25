use super::{ObjectRef, WorldState};
use crate::{WorldError, assets::WorldAssets, queue::EventKind};
use std::collections::BTreeSet;
use vault_formats::{MapFile, map::ScriptPrefix, pro::BasicFlags};

/// SPEC-006 §3.5 the party marks of a script record: "not saved with the
/// map" (script-instance flag `0x08`, SPEC-003 §2.9) and "kept by script
/// removal" (`0x10`).
pub mod party_marks {
    pub const NOT_SAVED: u32 = 0x08;
    pub const KEPT: u32 = 0x10;
    pub const BOTH: u32 = NOT_SAVED | KEPT;
}
/// SPEC-006 §3.5 step 7: a recovered party record's identifier is
/// `(PID & 0xFFFFFF) + 18000` in the critter category.
pub const PARTY_SCRIPT_BASE: u32 = 18000;
/// SPEC-006 §3.5 the player script: `scripts.lst` entry 0, no locals.
pub const PLAYER_SCRIPT_FILE: i32 = 0;

/// A party member's script record copied when a map is left (SPEC-006
/// §3.5 step 1), with its local-variable values, until step 7 recovers it.
#[derive(Debug, Clone)]
pub(crate) struct CarriedScript {
    pub member: ObjectRef,
    pub record: ScriptInstance,
    pub locals: Option<Vec<i32>>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScriptEvent {
    Start,
    MapEnter,
    MapUpdate,
    MapExit,
    Spatial,
    Timed,
    LookAt,
    Description,
    Use,
    UseSkill,
    UseObject,
    Pickup,
    Talk,
    /// The critter-script cycle step's request (SPEC-006 §3.7, see
    /// `WorldHost::critter_cycle_step`).
    Critter,
    /// SPEC-007 combat invocations (fixed parameter 4 turn, 5 join check).
    Combat,
    Damage,
    Destroy,
    /// An item dropped from inventory to the map.
    Drop,
    /// Never requested by the host (SPEC-004 §3.7).
    Create,
    /// SPEC-017 §2.4, Fallout 2 only: the player's Push action on a party
    /// member (SPEC-019 §3.6); an override suppresses the default push.
    Push,
    /// SPEC-017 §2.4, Fallout 2 only; never requested in this revision.
    IsDropping,
    /// SPEC-017 §2.4, Fallout 2 only; never requested: SPEC-019 defines no
    /// path.
    CombatIsStarting,
    /// SPEC-017 §2.4, Fallout 2 only; never requested either.
    CombatIsOver,
}
impl ScriptEvent {
    pub fn name(self) -> &'static [u8] {
        match self {
            Self::Start => b"start",
            Self::MapEnter => b"map_enter_p_proc",
            Self::MapUpdate => b"map_update_p_proc",
            Self::MapExit => b"map_exit_p_proc",
            Self::Spatial => b"spatial_p_proc",
            Self::Timed => b"timed_event_p_proc",
            Self::LookAt => b"look_at_p_proc",
            Self::Description => b"description_p_proc",
            Self::Use => b"use_p_proc",
            Self::UseSkill => b"use_skill_on_p_proc",
            Self::UseObject => b"use_obj_on_p_proc",
            Self::Pickup => b"pickup_p_proc",
            Self::Talk => b"talk_p_proc",
            Self::Critter => b"critter_p_proc",
            Self::Combat => b"combat_p_proc",
            Self::Damage => b"damage_p_proc",
            Self::Destroy => b"destroy_p_proc",
            Self::Drop => b"drop_p_proc",
            Self::Create => b"create_p_proc",
            Self::Push => b"push_p_proc",
            Self::IsDropping => b"is_dropping_p_proc",
            Self::CombatIsStarting => b"combat_is_starting_p_proc",
            Self::CombatIsOver => b"combat_is_over_p_proc",
        }
    }
    /// SPEC-004 §3.7 the host event number a request stores as the
    /// instance's action code (`script_action`, SPEC-005 0x80C7).
    pub fn code(self) -> i32 {
        match self {
            Self::Start => 1,
            Self::Spatial => 2,
            Self::Description => 3,
            Self::Pickup => 4,
            Self::Drop => 5,
            Self::Use => 6,
            Self::UseObject => 7,
            Self::UseSkill => 8,
            Self::Talk => 11,
            Self::Critter => 12,
            Self::Combat => 13,
            Self::Damage => 14,
            Self::MapEnter => 15,
            Self::MapExit => 16,
            Self::Create => 17,
            Self::Destroy => 18,
            Self::LookAt => 21,
            Self::Timed => 22,
            Self::MapUpdate => 23,
            Self::Push => 24,
            Self::IsDropping => 25,
            Self::CombatIsStarting => 26,
            Self::CombatIsOver => 27,
        }
    }
}
#[derive(Debug, Clone)]
pub struct ScriptRequest {
    pub script: u32,
    pub event: ScriptEvent,
    pub source: ObjectRef,
    pub target: ObjectRef,
    pub fixed: i32,
    pub action: i32,
    pub only_present: bool,
}
#[derive(Debug, Clone)]
pub struct ScriptInstance {
    pub id: u32,
    pub owner: ObjectRef,
    pub file: i32,
    pub flags: u32,
    pub local_base: i32,
    pub local_count: usize,
    pub spatial: Option<(i32, u8, i32)>,
    pub initialized: bool,
    pub program: u32,
    pub source: ObjectRef,
    pub target: ObjectRef,
    pub event: i32,
    pub fixed: i32,
    pub action: i32,
    pub result: i32,
    pub overridden: bool,
    pub margin: i32,
}
/// One active script record in stored list order (SPEC-003 §2.9): its
/// category stream (0 system to 4 critter) and identifier. Lookup by
/// identifier searches only the stream the identifier's top byte names and
/// binds the first equal record there (SPEC-003 §3.3), so a later duplicate
/// or a record in another category's stream is never found: it keeps its
/// stored record, which spatial checks read and saves write back unchanged.
/// Passes that walk the list still visit it and request its identifier.
#[derive(Debug, Clone)]
pub struct ScriptSlot {
    pub stream: u8,
    pub id: u32,
    pub inert: Option<Box<vault_formats::map::MapScript>>,
}
impl ScriptSlot {
    fn live(id: u32) -> Self {
        Self {
            stream: (id >> 24).min(4) as u8,
            id,
            inert: None,
        }
    }
}
/// A script instance from its stored record.
fn script_instance(script: &vault_formats::map::MapScript, local_count: usize) -> ScriptInstance {
    let spatial = if let ScriptPrefix::Spatial { location, radius } = script.prefix {
        Some((location.hex() as i32, location.elevation(), radius))
    } else {
        None
    };
    ScriptInstance {
        id: script.id,
        owner: 0,
        file: script.file_index,
        flags: script.flags.0,
        local_base: script.local_base,
        local_count,
        spatial,
        initialized: false,
        program: 0,
        source: 0,
        target: 0,
        event: script.event,
        fixed: script.parameter,
        action: script.action,
        result: script.result,
        overridden: false,
        margin: 0,
    }
}
impl WorldState {
    /// SPEC-006 §3.5 the player's script record, created from nothing (at
    /// program start, for a new game, at a game reset, and for a loaded
    /// player): a new critter-category instance of `scripts.lst` entry 0
    /// with no loaded program, action code 0, no local slice and no timers,
    /// carrying both party marks.
    pub fn create_player_script(&mut self) -> Result<u32, WorldError> {
        let sid = self.add_script(self.player, PLAYER_SCRIPT_FILE, 4, 0)?;
        let script = self.scripts.get_mut(&sid).expect("added script");
        script.flags |= party_marks::BOTH;
        Ok(sid)
    }
    /// SPEC-018 §6.5 (Revision 3): the player's record `sid` gets a local
    /// slice of `count` values: `values` when given, else zeros allocated on
    /// first use.
    pub(crate) fn set_player_locals(&mut self, sid: u32, count: usize, values: Option<Vec<i32>>) {
        let base = self.lvars.len();
        let Some(script) = self.scripts.get_mut(&sid) else {
            return;
        };
        script.local_count = count;
        script.local_base = -1;
        if let Some(values) = values.filter(|values| values.len() == count) {
            script.local_base = base as i32;
            self.lvars.extend(values);
        }
    }
    /// Tests about other critter records leave the player's out of the
    /// critter stream and the cycle.
    #[cfg(test)]
    pub(crate) fn drop_player_script(&mut self) {
        if let Some(sid) = self.script_for(self.player) {
            self.remove_script(sid);
        }
        if let Some(player) = self.get_mut(self.player) {
            player.raw.script_id = -1;
        }
    }
    /// SPEC-006 §3.5 saving a game: the party records lose their marks, so
    /// they are written with the map's other scripts, and get them back
    /// afterwards (`marked` false, then true).
    pub fn set_party_marks(&mut self, marked: bool) {
        for sid in self.party_records() {
            if let Some(script) = self.scripts.get_mut(&sid) {
                if marked {
                    script.flags |= party_marks::BOTH;
                } else {
                    script.flags &= !party_marks::BOTH;
                }
            }
        }
    }
    /// SPEC-006 §3.5 leaving a map (step 1): each party member's record and
    /// local values are copied, its marks cleared, and the instance removed
    /// in the ordinary way, cancelling its owner's script timers. The
    /// member keeps no script reference until step 7.
    fn carry_party_scripts(&mut self) {
        for member in self.party_members() {
            let Some(sid) = self.script_for(member) else {
                continue;
            };
            let record = self.scripts.get(&sid).cloned().expect("member script");
            let locals = usize::try_from(record.local_base)
                .ok()
                .and_then(|base| self.lvars.get(base..base.checked_add(record.local_count)?))
                .map(<[i32]>::to_vec);
            if let Some(script) = self.scripts.get_mut(&sid) {
                script.flags &= !party_marks::BOTH;
            }
            self.remove_script(sid);
            self.queue.retain(|event| {
                !matches!(event.kind, EventKind::Script { script, .. }
                    if script == sid || event.object == member)
            });
            if let Some(object) = self.get_mut(member) {
                object.raw.script_id = -1;
            }
            self.party_scripts.push(CarriedScript {
                member,
                record,
                locals,
            });
        }
    }
    /// SPEC-006 §3.5 step 7: each copied party record becomes a new
    /// critter-category instance with identifier `(PID & 0xFFFFFF) + 18000`
    /// (18000 for the player), keeping its action code, fixed parameter,
    /// flags and other words. It has no loaded program and has not run,
    /// carries both marks again and gets a fresh local slice for the saved
    /// values; it joins the end of the critter stream, after the map's own
    /// records. A taken identifier (two members of one PID, or a game
    /// load's saved copy of the player record, which the load then removes)
    /// moves on to the next free one, so records never alias.
    pub(crate) fn recover_party_scripts(&mut self) -> Result<(), WorldError> {
        for carried in std::mem::take(&mut self.party_scripts) {
            let CarriedScript {
                member,
                mut record,
                locals,
            } = carried;
            let Some(pid) = self.get(member).map(|object| object.raw.pid) else {
                continue;
            };
            let number = ((pid & 0x00ff_ffff) + PARTY_SCRIPT_BASE) & 0x00ff_ffff;
            let wanted = (4 << 24) | number;
            let id = if self.scripts.contains_key(&wanted) {
                (number..0x0100_0000)
                    .chain(1..number)
                    .map(|n| (4 << 24) | n)
                    .find(|id| !self.scripts.contains_key(id))
                    .ok_or(WorldError::Arithmetic)?
            } else {
                wanted
            };
            record.id = id;
            record.owner = member;
            record.initialized = false;
            record.program = 0;
            record.source = 0;
            record.target = member;
            record.flags |= party_marks::BOTH;
            match locals {
                Some(values) => {
                    record.local_base = self.lvars.len() as i32;
                    record.local_count = values.len();
                    self.lvars.extend(values);
                }
                None => record.local_base = -1,
            }
            self.scripts.insert(id, record);
            self.append_script(id);
            if let Some(object) = self.get_mut(member) {
                object.raw.script_id = id as i32;
            }
        }
        Ok(())
    }
    /// SPEC-006 §3.5 loading a game: the record the loaded player names
    /// (`saved`; `SAVE.DAT` stores -1 there, SPEC-008 §2.5), the copy the
    /// saved map restored (its record names the player's object and no map
    /// object bound it) and every record owned by the player (the one the
    /// reset created and the map load recovered) are removed, with their
    /// script timers, and the player gets a newly created record.
    ///
    /// Under Fallout 2, `player_locals` is `scripts.lst` entry 0's declared
    /// count (SPEC-018 §6.5, Revision 3): the new record has a slice of that
    /// size, holding the saved map copy's values when it has a slice of that
    /// size, and zeros otherwise. The copy's slice is handed over in place,
    /// so a load does not leave the old values behind in the pool.
    ///
    /// The old records are removed before the new one is created, so the new
    /// record takes the first free critter identifier of the restored world.
    /// It then depends only on the map's own records, never on the identifier
    /// the saved copy held, and a load of a load settles on the same
    /// identifier (SPEC-021 §2.8 keys local slices by it). No record of the
    /// restored world holds it at creation, and it comes last in the critter
    /// stream, as at creation.
    pub(crate) fn replace_player_script(
        &mut self,
        saved: i32,
        player_locals: Option<usize>,
    ) -> Result<u32, WorldError> {
        let player = self.player;
        let player_id = self.get(player).map_or(-1, |object| object.raw.id as i32);
        let copies: Vec<u32> = self
            .record_objects
            .iter()
            .filter(|(sid, object)| {
                *object == player_id && self.scripts.get(sid).is_some_and(|s| s.owner == 0)
            })
            .map(|(sid, _)| *sid)
            .collect();
        let kept = player_locals.and_then(|count| {
            copies.iter().find_map(|sid| {
                let copy = self.scripts.get(sid)?;
                let base = usize::try_from(copy.local_base).ok()?;
                let end = base.checked_add(count)?;
                (copy.local_count == count && end <= self.lvars.len()).then_some(base)
            })
        });
        let mut old: Vec<u32> = self
            .scripts
            .values()
            .filter(|script| script.owner == player)
            .map(|script| script.id)
            .chain(copies)
            .collect();
        if let Ok(saved) = u32::try_from(saved)
            && self.scripts.contains_key(&saved)
        {
            old.push(saved);
        }
        old.sort_unstable();
        old.dedup();
        let mut freed: Vec<(usize, usize)> = old
            .iter()
            .filter_map(|id| self.scripts.get(id))
            .filter_map(|script| {
                Some((usize::try_from(script.local_base).ok()?, script.local_count))
            })
            .filter(|(base, count)| *count > 0 && Some(*base) != kept)
            .collect();
        for id in &old {
            self.remove_script(*id);
        }
        self.reclaim_trailing_locals(&mut freed);
        self.queue.retain(
            |event| !matches!(event.kind, EventKind::Script { script, .. } if old.contains(&script)),
        );
        self.record_objects.retain(|(sid, _)| !old.contains(sid));
        let sid = self.create_player_script()?;
        if let Some(count) = player_locals {
            self.set_player_locals(sid, count, None);
            if let (Some(base), Some(script)) = (kept, self.scripts.get_mut(&sid)) {
                script.local_base = base as i32;
            }
        }
        Ok(sid)
    }
    /// The slices (`base`, `count`) of removed records that end the pool are
    /// cut from it, latest first, while no remaining record's slice (live or
    /// inert) reaches into them. A load's old player records (the saved copy
    /// runs the map-entry requests before the load removes it, and may take
    /// a fresh slice there) then leave no values behind, so a chain of loads
    /// of loads keeps one pool length. A slice in the middle of the pool
    /// stays, as removing it would move other records' bases.
    fn reclaim_trailing_locals(&mut self, freed: &mut [(usize, usize)]) {
        freed.sort_unstable_by(|a, b| b.cmp(a));
        let reaches = |base: i32, count: usize, from: usize| {
            usize::try_from(base).is_ok_and(|base| count > 0 && base.saturating_add(count) > from)
        };
        for (base, count) in freed.iter().copied() {
            if base.checked_add(count) != Some(self.lvars.len()) {
                continue;
            }
            let held = self
                .scripts
                .values()
                .any(|script| reaches(script.local_base, script.local_count, base))
                || self.script_order.iter().any(|slot| {
                    slot.inert.as_ref().is_some_and(|record| {
                        let count = usize::try_from(record.local_count).unwrap_or(0);
                        reaches(record.local_base, count, base)
                    })
                });
            if held {
                break;
            }
            self.lvars.truncate(base);
        }
    }
    pub fn import_map(
        &mut self,
        map: MapFile,
        assets: &impl WorldAssets,
    ) -> Result<(), WorldError> {
        // SPEC-006 §3.5 step 1: the party records leave with their members.
        self.carry_party_scripts();
        let keep: BTreeSet<_> = self
            .iter()
            .filter(|(reference, _)| {
                *reference == self.player
                    || self.party.contains(reference)
                    || self.owned_by_kept(*reference)
            })
            .map(|(r, _)| r)
            .collect();
        let remove: Vec<_> = self
            .iter()
            .filter(|(r, _)| !keep.contains(r))
            .map(|(r, _)| r)
            .collect();
        for reference in remove {
            self.erase(reference);
        }
        // SPEC-014 §3.36: removing all objects resets the roof tracking to
        // "none"; the entrance placement then hides the roof over the player.
        self.roof = crate::roof::RoofTracker::NONE;
        // SPEC-014 §3.33: the visited hexes belong to the map that is left
        // (a map saved on leaving has already folded them into its seen
        // flags; an excluded or unnumbered map drops them).
        self.visited.fill(0);
        // The kept items' script instances and local variables travel with
        // them, under ids that cannot alias the new map's scripts; a kept
        // object whose script instance is gone loses its reference instead
        // of dangling. (The party members' records were copied above, for
        // step 7.)
        let mut carried = Vec::new();
        for reference in &keep {
            let Some(sid) = self
                .get(*reference)
                .map(|object| object.raw.script_id)
                .filter(|sid| *sid != -1)
            else {
                continue;
            };
            let Some(instance) = self.scripts.get(&(sid as u32)).cloned() else {
                if let Some(object) = self.get_mut(*reference) {
                    object.raw.script_id = -1;
                }
                continue;
            };
            let locals = usize::try_from(instance.local_base)
                .ok()
                .and_then(|base| self.lvars.get(base..base + instance.local_count))
                .map(<[i32]>::to_vec);
            carried.push((*reference, instance, locals));
        }
        self.scripts.clear();
        self.script_order.clear();
        self.record_objects.clear();
        self.map_script = None;
        self.mvars = map.mvars.clone();
        self.lvars = map.lvars.clone();
        let saved = map
            .header
            .flags
            .contains(vault_formats::pro::MapFlags::SAVED);
        // SPEC-003 §3.3: the first record of an identifier in the stream
        // its top byte names binds; a later duplicate, or a record in
        // another category's stream, is kept inert.
        for (stream, category) in map.scripts.iter().enumerate() {
            for script in category.active() {
                let found =
                    script.id >> 24 == stream as u32 && !self.scripts.contains_key(&script.id);
                self.script_order.push(ScriptSlot {
                    stream: stream as u8,
                    id: script.id,
                    inert: (!found).then(|| Box::new(script.clone())),
                });
                if found {
                    if script.object_id != -1 {
                        self.record_objects.push((script.id, script.object_id));
                    }
                    let locals = if script.local_count > 0 {
                        script.local_count as usize
                    } else {
                        assets.local_count(script.file_index)
                    };
                    let mut instance = script_instance(script, locals);
                    // SPEC-003 §3.2 safe rule: a saved record with no
                    // locals owns no pool values, whatever base its first
                    // lazy request left. The count its file declares gets
                    // a fresh slice on first use, not a window over values
                    // past its base that later slices are handed.
                    if saved && script.local_count == 0 {
                        instance.local_base = -1;
                    }
                    self.scripts.insert(script.id, instance);
                }
            }
        }
        let roots: Vec<_> = map.objects.elevations.iter().flatten().cloned().collect();
        self.map = map;
        for root in roots {
            self.import_object(root, None, assets)?;
        }
        for (reference, mut instance, locals) in carried {
            let category = (instance.id >> 24).min(4);
            let id = (1..0x1000000)
                .map(|n| (category << 24) | n)
                .find(|id| !self.scripts.contains_key(id))
                .ok_or(WorldError::Arithmetic)?;
            instance.id = id;
            // The left map's programs were purged: the script runs its
            // initial stream again on its next request.
            instance.initialized = false;
            instance.program = 0;
            instance.source = 0;
            instance.target = instance.owner;
            match locals {
                Some(values) => {
                    instance.local_base = self.lvars.len() as i32;
                    instance.local_count = values.len();
                    self.lvars.extend(values);
                }
                None => instance.local_base = -1,
            }
            self.scripts.insert(id, instance);
            self.append_script(id);
            if let Some(object) = self.get_mut(reference) {
                object.raw.script_id = id as i32;
            }
        }
        self.rebuild_lighting();
        Ok(())
    }
    fn owned_by_kept(&self, mut reference: ObjectRef) -> bool {
        for _ in 0..64 {
            let Some(parent) = self.get(reference).and_then(|o| o.owner) else {
                return false;
            };
            if parent == self.player || self.party.contains(&parent) {
                return true;
            }
            reference = parent;
        }
        false
    }
    pub fn script_for(&self, object: ObjectRef) -> Option<u32> {
        let sid = self.get(object)?.raw.script_id;
        (sid != -1 && self.scripts.contains_key(&(sid as u32))).then_some(sid as u32)
    }
    /// SPEC-024b §4.12 `create_spatial`: an ordinary spatial record, with no
    /// object, saved with the map like any other.
    pub(crate) fn add_spatial_script(
        &mut self,
        file: i32,
        tile: i32,
        elevation: u8,
        radius: i32,
    ) -> Result<u32, WorldError> {
        let id = self.add_script(0, file, 1, 0)?;
        if let Some(script) = self.scripts.get_mut(&id) {
            script.spatial = Some((tile, elevation, radius));
        }
        Ok(id)
    }
    pub fn add_script(
        &mut self,
        owner: ObjectRef,
        file: i32,
        category: u8,
        locals: usize,
    ) -> Result<u32, WorldError> {
        if self.scripts.len() >= 100_000 || category > 4 || locals > 1_000_000 {
            return Err(WorldError::Invalid("script limit/domain".into()));
        }
        let id = (1..0x1000000)
            .map(|n| ((category as u32) << 24) | n)
            .find(|id| !self.scripts.contains_key(id))
            .ok_or(WorldError::Arithmetic)?;
        self.scripts.insert(
            id,
            ScriptInstance {
                id,
                owner,
                file,
                flags: 0,
                local_base: -1,
                local_count: locals,
                spatial: None,
                initialized: false,
                program: 0,
                source: 0,
                target: owner,
                event: 0,
                fixed: 0,
                action: -1,
                result: 0,
                overridden: false,
                margin: 0,
            },
        );
        self.append_script(id);
        if let Some(object) = self.get_mut(owner) {
            object.raw.script_id = id as i32;
            object.raw.cached_script_file = file;
        }
        Ok(id)
    }
    /// A new record joins the end of its category's stream.
    fn append_script(&mut self, id: u32) {
        let slot = ScriptSlot::live(id);
        let position = self
            .script_order
            .iter()
            .position(|other| other.stream > slot.stream)
            .unwrap_or(self.script_order.len());
        self.script_order.insert(position, slot);
    }
    /// Remove the record an identifier finds (SPEC-006 §3.7): its stream's
    /// last record moves into the freed slot. A later record of the same
    /// identifier in that stream is found from then on.
    pub fn remove_script(&mut self, id: u32) {
        self.scripts.remove(&id);
        let Some(at) = self
            .script_order
            .iter()
            .position(|slot| slot.id == id && slot.inert.is_none())
        else {
            return;
        };
        let stream = self.script_order[at].stream;
        let last = self
            .script_order
            .iter()
            .rposition(|slot| slot.stream == stream)
            .unwrap_or(at);
        self.script_order.swap(at, last);
        self.script_order.remove(last);
        let promoted = self.script_order.iter_mut().find(|slot| {
            slot.id == id && u32::from(slot.stream) == id >> 24 && slot.inert.is_some()
        });
        if let Some(slot) = promoted
            && let Some(record) = slot.inert.take()
        {
            let locals = usize::try_from(record.local_count).unwrap_or(0);
            self.scripts.insert(id, script_instance(&record, locals));
        }
    }
    /// The identifiers of the script records, in stored list order.
    pub fn script_ids(&self) -> Vec<u32> {
        self.script_order.iter().map(|slot| slot.id).collect()
    }
    pub fn ensure_locals(&mut self, sid: u32) -> Result<usize, WorldError> {
        let script = self
            .scripts
            .get_mut(&sid)
            .ok_or_else(|| WorldError::Invalid("missing script context".into()))?;
        if script.local_base < 0 {
            let end = self
                .lvars
                .len()
                .checked_add(script.local_count)
                .filter(|end| *end <= 1_000_000)
                .ok_or(WorldError::Arithmetic)?;
            // Allocation failure leaves the pool unchanged (§3.2 safe rule).
            self.lvars
                .try_reserve(script.local_count)
                .map_err(|_| WorldError::Invalid("LVAR pool allocation failed".into()))?;
            script.local_base = self.lvars.len() as i32;
            self.lvars.resize(end, 0);
        }
        Ok(script.local_base as usize)
    }
    /// The pool position of local `index`. Under the SPEC-003 §3.2 safe
    /// rule an index outside the script's own slice is out of range, rather
    /// than reaching another script's slice.
    fn local_slot(&mut self, sid: u32, index: i32) -> Result<Option<usize>, WorldError> {
        let base = self.ensure_locals(sid)?;
        let count = self
            .scripts
            .get(&sid)
            .map_or(0, |script| script.local_count);
        Ok(usize::try_from(index)
            .ok()
            .filter(|index| *index < count)
            .map(|index| base + index)
            .filter(|at| *at < self.lvars.len()))
    }
    /// System/map scripts read -1; an unallocatable slice reads -1 with a
    /// diagnostic, and an index outside the own slice reads 0.
    pub fn local(&mut self, sid: Option<u32>, index: i32) -> i32 {
        let Some(sid) = sid.filter(|sid| sid >> 24 != 0) else {
            return -1;
        };
        match self.local_slot(sid, index) {
            Ok(Some(at)) => self.lvars[at],
            Ok(None) => {
                self.diagnostic(format!("LVAR read {index} outside the script's slice"));
                0
            }
            Err(error) => {
                self.diagnostic(format!("LVAR read {index}: {error}"));
                -1
            }
        }
    }
    /// Writes outside the own slice are ignored; under the SPEC-003 §3.2 safe
    /// rule system/map scripts are denied local writes as well as reads.
    pub fn set_local(&mut self, sid: Option<u32>, index: i32, value: i32) -> bool {
        let Some(sid) = sid.filter(|sid| sid >> 24 != 0) else {
            return false;
        };
        match self.local_slot(sid, index) {
            Ok(Some(at)) => {
                self.lvars[at] = value;
                true
            }
            _ => false,
        }
    }
    pub fn request(&mut self, request: ScriptRequest) {
        if self.requests.len() < 100_000 {
            self.requests.push_back(request);
        }
    }
    pub fn broadcast(&mut self, event: ScriptEvent, fixed: i32) {
        if let Some(script) = self.map_script {
            self.request(ScriptRequest {
                script,
                event,
                source: 0,
                target: 0,
                fixed,
                action: -1,
                only_present: false,
            });
        }
        for script in self.script_ids() {
            if Some(script) != self.map_script {
                self.request(ScriptRequest {
                    script,
                    event,
                    source: 0,
                    target: 0,
                    fixed,
                    action: -1,
                    only_present: true,
                });
            }
        }
    }
    pub fn check_spatials(&mut self, mover: ObjectRef) {
        if !self.spatial_enabled || self.cursor_objects.contains(&mover) {
            return;
        }
        let Some(object) = self.get(mover) else {
            return;
        };
        let Some(hex) = object.raw.hex.filter(|hex| *hex >= 10) else {
            return;
        };
        if object.raw.flags.0 & (BasicFlags::HIDDEN.0 | BasicFlags::FLAT.0) != 0 {
            return;
        }
        let elevation = object.raw.elevation;
        self.spatial_enabled = false;
        // SPEC-003 §3.3: an inert record's own location and radius request
        // the procedure of the record its identifier finds.
        let triggered = |flags: u32, spatial: Option<(i32, u8, i32)>| {
            flags & 2 == 0
                && spatial.is_some_and(|(center, layer, radius)| {
                    layer == elevation
                        && (hex == center
                            || (radius != 0
                                && vault_formats::HexGrid::lattice_distance(hex, center)
                                    .is_ok_and(|distance| distance <= radius)))
                })
        };
        let ids: Vec<u32> = self
            .script_order
            .iter()
            .filter(|slot| match &slot.inert {
                Some(record) => triggered(record.flags.0, script_instance(record, 0).spatial),
                None => self
                    .scripts
                    .get(&slot.id)
                    .is_some_and(|script| triggered(script.flags, script.spatial)),
            })
            .map(|slot| slot.id)
            .collect();
        for id in ids {
            self.request(ScriptRequest {
                script: id,
                event: ScriptEvent::Spatial,
                source: mover,
                target: 0,
                fixed: 0,
                action: -1,
                only_present: false,
            });
        }
        self.spatial_enabled = true;
    }
}
