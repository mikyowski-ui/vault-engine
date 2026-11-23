use super::{ObjectRef, WorldState};
use crate::{CharacterSheet, Perk, StatContext, WorldError, assets::WorldAssets};
use std::{collections::BTreeMap, sync::Arc};
use vault_formats::{
    map::{CritterState, MapObject, ObjectDetail, ObjectState},
    pro::{
        BasicFlags, Category, ItemData, Light, LockFlags, Prototype, PrototypeData, SceneryData,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectAction {
    LookAt,
    Description,
    Use,
    Pickup,
    Talk,
    UseSkill(i32),
    UseObject(ObjectRef),
    /// SPEC-019 §3.6, Fallout 2 only: the player pushes a party member,
    /// after the talk approach.
    Push,
}
#[derive(Debug, Clone)]
pub struct WorldObject {
    pub raw: MapObject,
    pub prototype: Option<Arc<Prototype>>,
    pub character: Option<CharacterSheet>,
    pub name: Vec<u8>,
    pub description: Vec<u8>,
    pub owner: Option<ObjectRef>,
    pub inventory: Vec<(ObjectRef, i32)>,
    pub money: Option<i32>,
    pub active: bool,
    pub opponent: ObjectRef,
}
impl WorldObject {
    pub fn category(&self) -> Category {
        Category::from_pid(self.raw.pid).unwrap_or(Category::Misc)
    }
    pub fn alive(&self) -> bool {
        self.active
            && self
                .character
                .as_ref()
                .is_none_or(|sheet| sheet.current_hp > 0)
    }
    pub fn lock(&self) -> Option<LockFlags> {
        match &self.raw.state {
            ObjectState::Other {
                word,
                detail: ObjectDetail::Plain,
            } if self.prototype.as_ref().is_some_and(|p| {
                matches!(
                    p.data,
                    PrototypeData::Item(vault_formats::pro::Item {
                        data: ItemData::Container { .. },
                        ..
                    })
                )
            }) =>
            {
                Some(LockFlags(*word))
            }
            ObjectState::Other {
                detail: ObjectDetail::Door(flags),
                ..
            } => Some(*flags),
            _ => None,
        }
    }
    pub fn set_lock(&mut self, flags: LockFlags) -> bool {
        if self.lock().is_none() {
            return false;
        }
        match &mut self.raw.state {
            ObjectState::Other {
                detail: ObjectDetail::Door(value),
                ..
            } => *value = flags,
            ObjectState::Other { word, .. } => *word = flags.0,
            _ => return false,
        }
        true
    }
    pub fn weight(&self) -> i32 {
        match self.prototype.as_deref().map(|p| &p.data) {
            Some(PrototypeData::Item(item)) => item.weight.max(0),
            _ => 0,
        }
    }
}
/// SPEC-014 §3.44 the next per-hex list insertion stamp. Within one group
/// of a hex list (elevation, then flat class) objects are drawn by
/// ascending stamp, so the latest insertion comes last. The sequence is
/// shared by the world's objects and the attack shows' displays, so a
/// display that steps a frame goes after the world objects inserted
/// before it; only the stamps' relative order matters.
pub fn next_insertion() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}
pub fn object_record(pid: u32, id: u32) -> MapObject {
    MapObject {
        id,
        hex: None,
        displacement: [0; 2],
        screen: [0; 2],
        frame: 0,
        orientation: 0,
        art_id: pid & 0xff000000,
        flags: BasicFlags(0),
        stored_elevation: 0,
        elevation: 0,
        pid,
        combat_id: -1,
        light: Light {
            radius: 0,
            intensity: 0,
        },
        outline: 0,
        script_id: -1,
        cached_script_file: -1,
        inventory_capacity: 0,
        state: ObjectState::Other {
            word: 0,
            detail: ObjectDetail::Plain,
        },
        inventory: Vec::new(),
        owner: None,
        file_offset: 0,
    }
}
impl WorldState {
    pub fn insert(&mut self, mut object: WorldObject) -> Result<ObjectRef, WorldError> {
        if self.references.len() >= 200_000 {
            return Err(WorldError::Invalid("world object limit exceeded".into()));
        }
        if object.raw.id == 0 || self.id_index().contains_key(&object.raw.id) {
            object.raw.id = self.allocate_object_id()?;
        }
        let reference = self.next_reference;
        self.next_reference = self
            .next_reference
            .checked_add(1)
            .ok_or(WorldError::Arithmetic)?;
        if let Some(prototype) = &object.prototype {
            self.prototypes
                .entry(prototype.pid)
                .or_insert_with(|| prototype.clone());
        }
        let id = object.raw.id;
        let key = self.entities.insert(object);
        self.references.insert(reference, key);
        self.unique_ids.insert(id, reference);
        // SPEC-014 §3.44: loading (in file order), creating and copying
        // insert the object at the end of its group.
        self.insertions.insert(reference, next_insertion());
        Ok(reference)
    }
    /// SPEC-014 §3.44 remove the object from its hex list and insert it
    /// again at the end of its group, whatever changed (even nothing).
    pub fn reinsert(&mut self, reference: ObjectRef) {
        if self.references.contains_key(&reference) {
            self.insertions.insert(reference, next_insertion());
        }
    }
    /// SPEC-014 §3.44 the object's latest insertion stamp, if it has one: a
    /// render-side object stamped later draws after it in its group.
    pub fn insertion(&self, reference: ObjectRef) -> Option<u64> {
        self.insertions.get(&reference).copied()
    }
    /// SPEC-014 §3.44 a pixel-offset change: it always reinserts the
    /// object, even when the offset is unchanged.
    pub fn set_offset(&mut self, reference: ObjectRef, offset: [i32; 2]) {
        if let Some(object) = self.get_mut(reference) {
            object.raw.displacement = offset;
            self.reinsert(reference);
        }
    }
    /// [`Self::set_offset`] to the current offset plus `shift` (an
    /// animation frame's shift, applied even when it is zero).
    pub fn shift_offset(&mut self, reference: ObjectRef, shift: [i32; 2]) {
        if let Some(object) = self.get(reference) {
            let [x, y] = object.raw.displacement;
            self.set_offset(
                reference,
                [x.wrapping_add(shift[0]), y.wrapping_add(shift[1])],
            );
        }
    }
    /// SPEC-014 §3.44 an object flipped between flat and not flat changes
    /// group and is reinserted.
    pub fn set_flat(&mut self, reference: ObjectRef, flat: bool) {
        if let Some(object) = self.get_mut(reference)
            && object.raw.flags.contains(BasicFlags::FLAT) != flat
        {
            if flat {
                object.raw.flags.0 |= BasicFlags::FLAT.0;
            } else {
                object.raw.flags.0 &= !BasicFlags::FLAT.0;
            }
            self.reinsert(reference);
        }
    }
    /// The unique-id index, rebuilt when it no longer mirrors the live
    /// objects (see `unique_ids`).
    /// The next object ID no object holds.
    pub(crate) fn allocate_object_id(&mut self) -> Result<u32, WorldError> {
        while {
            let next = self.next_uid;
            self.id_index().contains_key(&next)
        } {
            self.next_uid = self.next_uid.checked_add(1).ok_or(WorldError::Arithmetic)?;
        }
        let id = self.next_uid;
        self.next_uid = self.next_uid.checked_add(1).ok_or(WorldError::Arithmetic)?;
        Ok(id)
    }
    /// `reference` takes object ID `id`.
    pub(crate) fn set_object_id(&mut self, reference: ObjectRef, id: u32) {
        let Some(object) = self.get_mut(reference) else {
            return;
        };
        let old = std::mem::replace(&mut object.raw.id, id);
        self.unique_ids.remove(&old);
        self.unique_ids.insert(id, reference);
    }
    /// Ids the index holds for a retired object, or for one now under another
    /// id. Lookups rebuild the index only when its length is off, so an equal
    /// count can hide one of these.
    pub fn stale_object_ids(&self) -> Vec<u32> {
        self.unique_ids
            .iter()
            .filter(|(id, reference)| self.get(**reference).is_none_or(|o| o.raw.id != **id))
            .map(|(id, _)| *id)
            .collect()
    }
    fn id_index(&mut self) -> &BTreeMap<u32, ObjectRef> {
        let player_id = self.get(self.player).map(|object| object.raw.id);
        let stale = self.unique_ids.len() != self.references.len()
            || player_id.is_some_and(|id| self.unique_ids.get(&id) != Some(&self.player));
        if stale {
            self.unique_ids = self
                .iter()
                .map(|(reference, object)| (object.raw.id, reference))
                .collect();
        }
        &self.unique_ids
    }
    /// SPEC-011 §3.20 player experience award (Swift Learner, clamping,
    /// and level advancement); returns the levels gained. The host prints
    /// and plays each level-up.
    pub fn give_experience(&mut self, amount: i32) -> u32 {
        let context = self.context(self.player);
        let levels = self
            .player_sheet_mut()
            .map_or(0, |sheet| sheet.add_experience(amount, context));
        self.sync_hit_points(self.player);
        levels
    }
    /// Copy a sheet's current HP (after sheet-level healing) into the
    /// critter's instance record.
    pub fn sync_hit_points(&mut self, critter: ObjectRef) {
        if let Some(object) = self.get_mut(critter)
            && let Some(hp) = object.character.as_ref().map(|sheet| sheet.current_hp)
            && let ObjectState::Critter(state) = &mut object.raw.state
        {
            state.hp = hp;
        }
    }
    /// SPEC-006 §3.5 step 3 `[May-improve]` safe rule for a saved map:
    /// every placed critter but the player (PID -1 skipped) takes its last
    /// attacker from its stored word ([`Self::resolve_last_attacker`]).
    /// Intent bits and every other combat field are untouched.
    pub(crate) fn resolve_last_attackers(&mut self) {
        let critters: Vec<_> = self
            .iter()
            .filter(|(reference, object)| {
                *reference != self.player
                    && object.owner.is_none()
                    && object.raw.hex.is_some()
                    && object.raw.pid != u32::MAX
            })
            .map(|(reference, _)| reference)
            .collect();
        for critter in critters {
            self.resolve_last_attacker(critter);
        }
    }
    /// A critter's stored last-attacker word as its reference: a
    /// nonnegative combat identifier resolves to the placed critter with
    /// that identifier, or to nothing when none matches; -1 is nothing.
    pub(crate) fn resolve_last_attacker(&mut self, critter: ObjectRef) {
        let Some(ObjectState::Critter(state)) = self.get(critter).map(|o| &o.raw.state) else {
            return;
        };
        let attacker = state.attacker;
        let found = (attacker >= 0)
            .then(|| {
                self.iter()
                    .find(|(_, object)| {
                        object.owner.is_none()
                            && object.raw.hex.is_some()
                            && object.category() == Category::Critter
                            && object.raw.combat_id == attacker
                    })
                    .map(|(found, _)| found)
            })
            .flatten()
            .unwrap_or(0);
        if let Some(object) = self.get_mut(critter) {
            object.opponent = found;
        }
    }
    pub(crate) fn import_object(
        &mut self,
        mut raw: MapObject,
        owner: Option<ObjectRef>,
        assets: &impl WorldAssets,
    ) -> Result<ObjectRef, WorldError> {
        let children = std::mem::take(&mut raw.inventory);
        let prototype = assets.prototype(raw.pid).ok();
        let mut character = match prototype.as_deref().map(|p| &p.data) {
            Some(PrototypeData::Critter(critter)) => Some(self.critter_sheet(raw.pid, critter)),
            _ => None,
        };
        if let ObjectState::Other {
            detail: ObjectDetail::Weapon { ammo_pid, .. },
            ..
        } = raw.state
            && let Ok(ammo) = u32::try_from(ammo_pid)
            && !self.prototypes.contains_key(&ammo)
            && let Ok(prototype) = assets.prototype(ammo)
        {
            self.remember_prototype(prototype);
        }
        if let Some(sheet) = &mut character
            && let ObjectState::Critter(state) = &raw.state
        {
            sheet.current_hp = state.hp;
            sheet.radiation = state.radiation;
            sheet.poison = state.poison;
        }
        let name = prototype
            .as_deref()
            .and_then(|p| assets.prototype_text(p, false))
            .unwrap_or_else(|| format!("Object {:08X}", raw.pid).into_bytes());
        let description = prototype
            .as_deref()
            .and_then(|p| assets.prototype_text(p, true))
            .unwrap_or_default();
        let money = (self.rules.money_pid() == raw.pid).then_some(match raw.state {
            ObjectState::Other { word, .. } => word as i32,
            _ => 0,
        });
        let sid = raw.script_id as u32;
        let reference = self.insert(WorldObject {
            raw,
            prototype,
            character,
            name,
            description,
            owner,
            inventory: Vec::new(),
            money,
            active: true,
            opponent: 0,
        })?;
        if let Some(script) = self.scripts.get_mut(&sid) {
            script.owner = reference;
        }
        for child in children {
            let reference_child = self.import_object(child.object, Some(reference), assets)?;
            self.get_mut(reference)
                .expect("inserted parent")
                .inventory
                .push((reference_child, child.quantity as i32));
        }
        Ok(reference)
    }
    pub fn spawn(
        &mut self,
        prototype: Arc<Prototype>,
        hex: i32,
        elevation: u8,
    ) -> Result<ObjectRef, WorldError> {
        if !(0..40000).contains(&hex) || elevation > 2 {
            return Err(WorldError::Invalid("invalid placement".into()));
        }
        let reference = self.instantiate(prototype)?;
        let object = self.get_mut(reference).expect("instantiated object");
        object.raw.hex = Some(hex);
        object.raw.elevation = elevation;
        self.reinsert(reference);
        self.rebuild_lighting();
        Ok(reference)
    }
    /// A critter's sheet from its prototype, with any stat arrays this
    /// session changed on that prototype.
    pub(crate) fn critter_sheet(
        &self,
        pid: u32,
        critter: &vault_formats::pro::Critter,
    ) -> CharacterSheet {
        let mut sheet = CharacterSheet::from_critter(critter);
        if let Some((base, bonus)) = self.prototype_stats.get(&pid) {
            (sheet.base, sheet.bonus) = (*base, *bonus);
            sheet.current_hp = sheet.stat(7, StatContext::default());
        }
        sheet
    }
    /// A new unplaced, unowned instance of a prototype.
    pub fn instantiate(&mut self, prototype: Arc<Prototype>) -> Result<ObjectRef, WorldError> {
        let mut raw = object_record(prototype.pid, 0);
        raw.art_id = prototype.art_id;
        // Only persistent presentation/blocking bits propagate from a prototype.
        raw.flags = BasicFlags(prototype.flags.0 & 0xb00fd818);
        raw.light = prototype.light.unwrap_or(Light {
            radius: 0,
            intensity: 0,
        });
        let character = if let PrototypeData::Critter(critter) = &prototype.data {
            let sheet = self.critter_sheet(prototype.pid, critter);
            raw.state = ObjectState::Critter(CritterState {
                reaction: 0,
                damage: 0,
                maneuvers: 0,
                ap: 0,
                results: vault_formats::pro::CombatFlags(0),
                ai: critter.ai,
                team: critter.team,
                attacker: -1,
                hp: sheet.current_hp,
                radiation: 0,
                poison: 0,
            });
            Some(sheet)
        } else {
            let detail = match &prototype.data {
                PrototypeData::Item(item) => match &item.data {
                    ItemData::Weapon(weapon) => ObjectDetail::Weapon {
                        rounds: weapon.capacity,
                        ammo_pid: weapon.ammo_pid,
                    },
                    ItemData::Ammunition { quantity, .. } => ObjectDetail::Ammunition(*quantity),
                    ItemData::Misc { charges, .. } => ObjectDetail::Charges(*charges),
                    ItemData::Key { code } => ObjectDetail::Key(*code),
                    _ => ObjectDetail::Plain,
                },
                PrototypeData::Scenery(scenery) => match scenery.data {
                    SceneryData::Door { state, .. } => ObjectDetail::Door(state),
                    SceneryData::Stairs { map, destination } => ObjectDetail::Stairs {
                        map,
                        destination: vault_formats::pro::PackedLocation(destination),
                    },
                    SceneryData::Elevator { kind, level } => ObjectDetail::Elevator { kind, level },
                    SceneryData::LadderUp { destination }
                    | SceneryData::LadderDown { destination } => ObjectDetail::Ladder {
                        map: None,
                        destination: vault_formats::pro::PackedLocation(destination),
                    },
                    _ => ObjectDetail::Plain,
                },
                _ => ObjectDetail::Plain,
            };
            raw.state = ObjectState::Other { word: 0, detail };
            None
        };
        let money = (self.rules.money_pid() == prototype.pid).then_some(0);
        let reference = self.insert(WorldObject {
            name: format!("Object {:08X}", prototype.pid).into_bytes(),
            description: Vec::new(),
            raw,
            prototype: Some(prototype),
            character,
            owner: None,
            inventory: Vec::new(),
            money,
            active: true,
            opponent: 0,
        })?;
        Ok(reference)
    }
    pub fn erase(&mut self, reference: ObjectRef) {
        if let Some(owner) = self
            .get(reference)
            .filter(|o| o.raw.flags.contains(BasicFlags::WORN))
            .and_then(|o| o.owner)
        {
            self.armour_effect(owner, reference, false);
        }
        let Some(key) = self.references.remove(&reference) else {
            return;
        };
        self.insertions.remove(&reference);
        if let Some(object) = self.entities.remove(key) {
            if self.unique_ids.get(&object.raw.id) == Some(&reference) {
                self.unique_ids.remove(&object.raw.id);
            }
            if let Some(owner) = object.owner
                && let Some(owner) = self.get_mut(owner)
            {
                owner.inventory.retain(|(r, _)| *r != reference);
            }
            for (child, _) in object.inventory {
                self.erase(child);
            }
            if object.raw.script_id != -1 {
                self.remove_script(object.raw.script_id as u32);
            }
        }
        self.queue.remove_object(reference);
        self.party.remove(&reference);
        // SPEC-005 0x80F4: destruction clears combat membership.
        self.remove_from_combat(reference);
    }
    /// SPEC-011 §3.10 armor contribution, for the player only: AC, DR and
    /// DT added (or removed) unclamped, then the armor perk's effect. NPC
    /// protection never comes from worn armor.
    pub(crate) fn armour_effect(&mut self, owner: ObjectRef, item: ObjectRef, apply: bool) {
        if owner != self.player {
            return;
        }
        let Some(armour) = self.item_data(item).and_then(|data| match &data.data {
            ItemData::Armor(armour) => Some(armour.clone()),
            _ => None,
        }) else {
            return;
        };
        let sign = if apply { 1 } else { -1 };
        self.adjust_bonus(owner, 9, armour.armor_class.saturating_mul(sign));
        for index in 0..7 {
            self.adjust_bonus(
                owner,
                24 + index,
                armour.resistances[index].saturating_mul(sign),
            );
            self.adjust_bonus(
                owner,
                17 + index,
                armour.thresholds[index].saturating_mul(sign),
            );
        }
        self.perk_effect(owner, armour.perk, apply);
    }
    /// Add `delta` to a bonus-array stat. The player's array is its own;
    /// an NPC's lives on its prototype (SPEC-011 §3.2), so the change
    /// applies to every critter sharing the PID, including ones created
    /// later in this session.
    pub fn adjust_bonus(&mut self, critter: ObjectRef, index: usize, delta: i32) {
        let context = self.context(critter);
        let player = self.player;
        let Some(object) = self.get_mut(critter) else {
            return;
        };
        let pid = object.raw.pid;
        let Some(sheet) = object.character.as_mut() else {
            return;
        };
        sheet.adjust_bonus(index, delta, context);
        if critter == player {
            return;
        }
        let shared = (sheet.base, sheet.bonus);
        self.prototype_stats.insert(pid, shared);
        for (reference, key) in &self.references {
            if *reference == critter || *reference == player {
                continue;
            }
            if let Some(object) = self.entities.get_mut(*key)
                && object.raw.pid == pid
                && let Some(sheet) = object.character.as_mut()
            {
                (sheet.base, sheet.bonus) = shared;
            }
        }
    }
    /// SPEC-011 §3.12 perk effect (rank, armor, or withdrawal) on a
    /// critter's bonus array; an index outside 0-63 does nothing.
    pub fn perk_effect(&mut self, critter: ObjectRef, perk: i32, apply: bool) {
        let Some(perk) = Perk::from_index(perk) else {
            return;
        };
        for (stat, amount) in crate::progression::perk_changes(perk) {
            self.adjust_bonus(critter, stat, if apply { amount } else { -amount });
        }
    }
    pub fn equip(
        &mut self,
        owner: ObjectRef,
        item: ObjectRef,
        flag: u32,
    ) -> Result<(), WorldError> {
        if self.get(item).is_none_or(|o| o.owner != Some(owner)) {
            return Err(WorldError::Invalid(
                "equipment must be in the critter's inventory".into(),
            ));
        }
        let previous: Vec<_> = self
            .get(owner)
            .ok_or_else(|| WorldError::Invalid("equipment owner missing".into()))?
            .inventory
            .iter()
            .filter(|(reference, _)| {
                self.get(*reference)
                    .is_some_and(|o| o.raw.flags.0 & flag != 0)
            })
            .map(|(r, _)| *r)
            .collect();
        if previous.contains(&item) {
            return Ok(());
        }
        for old in previous {
            if flag == BasicFlags::WORN.0 {
                self.armour_effect(owner, old, false);
            }
            if let Some(object) = self.get_mut(old) {
                object.raw.flags.0 &= !flag;
            }
        }
        self.get_mut(item).expect("item").raw.flags.0 |= flag;
        if flag == BasicFlags::WORN.0 {
            self.armour_effect(owner, item, true);
        }
        Ok(())
    }
}
