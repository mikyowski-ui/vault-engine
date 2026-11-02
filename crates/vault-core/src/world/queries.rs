use super::{EXPLOSION_ART_ID, ObjectRef, WorldObject, WorldState, object_record};
use crate::{CharacterSheet, EntityId, Perk, StatContext};
use std::collections::BTreeMap;
use vault_formats::{
    MapFile,
    map::{MapObject, MapObjects, ObjectState},
    pro::BasicFlags,
};

/// A record drawn as a presentation display shows it; `None` when the
/// display is off the map or hidden.
fn displayed(mut record: MapObject, display: &crate::choreography::Display) -> Option<MapObject> {
    if display.hidden {
        return None;
    }
    record.hex = Some(display.hex?);
    record.art_id = display.art;
    record.frame = display.frame;
    record.elevation = display.elevation;
    record.orientation = display.orientation;
    record.displacement = display.displacement;
    if display.flat {
        record.flags.0 |= BasicFlags::FLAT.0;
    }
    Some(record)
}
impl WorldState {
    /// The party: the player (party member 0) and the other members.
    pub fn party_members(&self) -> Vec<ObjectRef> {
        std::iter::once(self.player)
            .chain(self.party.iter().copied().filter(|m| *m != self.player))
            .collect()
    }
    /// The script records of the party members, in member order.
    pub fn party_records(&self) -> Vec<u32> {
        self.party_members()
            .into_iter()
            .filter_map(|member| self.script_for(member))
            .collect()
    }
    pub fn get(&self, reference: ObjectRef) -> Option<&WorldObject> {
        self.entities.get(*self.references.get(&reference)?)
    }
    /// The entity-store key a reference names, if it still names one.
    pub fn entity_key(&self, reference: ObjectRef) -> Option<EntityId> {
        self.references.get(&reference).copied()
    }
    pub fn get_mut(&mut self, reference: ObjectRef) -> Option<&mut WorldObject> {
        let key = *self.references.get(&reference)?;
        self.entities.get_mut(key)
    }
    pub fn iter(&self) -> impl Iterator<Item = (ObjectRef, &WorldObject)> {
        self.references.iter().filter_map(|(reference, key)| {
            self.entities.get(*key).map(|object| (*reference, object))
        })
    }
    pub fn reference_for_id(&self, id: u32) -> Option<ObjectRef> {
        self.iter()
            .find(|(_, object)| object.raw.id == id)
            .map(|(reference, _)| reference)
    }
    pub fn context(&self, object: ObjectRef) -> StatContext {
        let (ap, blinded) = match self.get(object).map(|o| &o.raw.state) {
            Some(ObjectState::Critter(state)) => (state.ap, state.results.0 & 0x40 != 0),
            _ => (0, false),
        };
        StatContext {
            ticks: self.clock.ticks,
            blinded,
            combat: self.combat,
            taking_turn: self.combat_actor == object,
            remaining_ap: ap,
            visible_light: self.visible_light(object),
            difficulty: self.difficulty,
            skill_cap: self.session.skill_cap,
            stat_max: self.session.stat_max.filter(|_| object == self.player),
            level_points: self.session.level_points,
            perk_interval: self.session.perk_interval,
        }
    }
    pub fn player_sheet(&self) -> Option<&CharacterSheet> {
        self.get(self.player)?.character.as_ref()
    }
    pub fn player_sheet_mut(&mut self) -> Option<&mut CharacterSheet> {
        self.get_mut(self.player)?.character.as_mut()
    }
    pub fn player_stat(&self, index: usize) -> i32 {
        self.player_sheet()
            .map_or(0, |sheet| sheet.stat(index, self.context(self.player)))
    }
    pub fn perk(&self, perk: Perk) -> u32 {
        self.player_sheet().map_or(0, |sheet| sheet.rank(perk))
    }
    /// SPEC-017 §5.1 rule 49 during an explosion: `reference` is the object
    /// in a notice, or the explosive being resolved.
    pub(crate) fn in_explosion(&self, reference: ObjectRef) -> bool {
        self.notified == Some(reference) || self.resolving == Some(reference)
    }
    /// The map with its object table replaced by `render_objects`. This
    /// clones the whole map file; a per-frame caller should draw from
    /// `map` by reference and `render_objects` instead.
    pub fn render_snapshot(&self) -> MapFile {
        let mut map = self.map.clone();
        let consumed_len = map.objects.consumed_len;
        map.objects = self.render_objects();
        map.objects.consumed_len = consumed_len;
        map
    }
    /// The object table to draw: the active, unowned world objects on the
    /// three elevations plus the records the attack shows and explosions
    /// present, without cloning the map's tiles, scripts or variables.
    /// Each elevation is in SPEC-014 §3.44 insertion order, oldest first.
    pub fn render_objects(&self) -> MapObjects {
        let mut objects = MapObjects {
            total: 0,
            elevations: std::array::from_fn(|_| Vec::new()),
            consumed_len: 0,
        };
        // SPEC-007 §3.10: an object an attack presentation owns is drawn as
        // the earliest one still to finish draws it. Owners only the
        // presentation knows (a created projectile, explosion art, an
        // erased grenade) are drawn from their displays alone.
        use crate::choreography::{Display, Owner};
        let mut owned: BTreeMap<Owner, &Display> = BTreeMap::new();
        for (owner, display) in self.shows.iter().flat_map(|show| show.displays()) {
            owned.entry(*owner).or_insert(display);
        }
        // SPEC-014 §3.44: each elevation lists its objects by their latest
        // insertion. A display that has stepped since its show began keeps
        // its own; records only an explosion draws come after the others.
        let mut drawn: Vec<(u64, MapObject)> = Vec::new();
        let mut push = |stamp: u64, record: MapObject| {
            if record.elevation < 3 {
                drawn.push((stamp, record));
            }
        };
        for (reference, object) in self.iter() {
            if object.owner.is_some() || !object.active || object.raw.elevation >= 3 {
                continue;
            }
            let stamp = self.insertions.get(&reference).copied().unwrap_or(0);
            match owned.remove(&Owner::World(reference)) {
                Some(display) => {
                    if let Some(record) = displayed(object.raw.clone(), display) {
                        push(display.inserted.unwrap_or(stamp), record);
                    }
                }
                None => push(stamp, object.raw.clone()),
            }
        }
        for display in owned.into_values() {
            let record = object_record(display.pid, EXPLOSION_ART_ID);
            if let Some(record) = displayed(record, display) {
                push(display.inserted.unwrap_or(0), record);
            }
        }
        // SPEC-013 §3.10 the timed or scripted explosion's art playing now.
        for record in self
            .explosion
            .iter()
            .flat_map(|explosion| explosion.art.records())
        {
            push(u64::MAX, record);
        }
        // A stable sort keeps equal stamps in drawing order.
        drawn.sort_by_key(|(stamp, _)| *stamp);
        for (_, record) in drawn {
            objects.elevations[record.elevation as usize].push(record);
            objects.total += 1;
        }
        objects
    }
    /// SPEC-024b §4.12 `tile_get_objs`: every object on the hex, hidden, dead
    /// and system objects too, in SPEC-014 §3.44's per-hex order.
    pub(crate) fn objects_on_hex(&self, hex: i32, elevation: u8) -> Vec<ObjectRef> {
        let mut objects: Vec<ObjectRef> = self
            .iter()
            .filter(|(_, object)| {
                object.owner.is_none()
                    && object.raw.hex == Some(hex)
                    && object.raw.elevation == elevation
            })
            .map(|(reference, _)| reference)
            .collect();
        let flat = |object: &ObjectRef| {
            let flags = self.get(*object).map_or(0, |found| found.raw.flags.0);
            flags & BasicFlags::FLAT.0 == 0
        };
        objects.sort_by_key(|object| (flat(object), self.insertions.get(object).copied()));
        objects
    }
    pub fn distance(&self, a: ObjectRef, b: ObjectRef) -> i32 {
        match (self.get(a), self.get(b)) {
            (Some(a), Some(b)) if a.raw.elevation == b.raw.elevation => {
                vault_formats::HexGrid::angular_distance(
                    a.raw.hex.unwrap_or(-1),
                    b.raw.hex.unwrap_or(-1),
                )
                .unwrap_or(9999)
            }
            _ => 9999,
        }
    }
    pub fn inventory_count(&self, owner: ObjectRef, pid: u32) -> i32 {
        let Some(object) = self.get(owner) else {
            return 0;
        };
        object
            .inventory
            .iter()
            .fold(0_i32, |sum, (reference, quantity)| {
                let Some(item) = self.get(*reference) else {
                    return sum;
                };
                sum.saturating_add(if item.raw.pid == pid { *quantity } else { 0 })
                    .saturating_add(self.inventory_count(*reference, pid))
            })
    }
    pub fn carried(&self, owner: ObjectRef, pid: u32) -> Option<ObjectRef> {
        for (item, _) in &self.get(owner)?.inventory {
            if self.get(*item)?.raw.pid == pid {
                return Some(*item);
            }
            if let Some(found) = self.carried(*item, pid) {
                return Some(found);
            }
        }
        None
    }
}
