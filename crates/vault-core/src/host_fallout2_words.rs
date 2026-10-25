//! SPEC-017 host words under the Fallout 2 profile: the six SPEC-005 words
//! §3.2 reassigns (`mark_area_known`, `tile_in_tile_rect`, `metarule3`,
//! `set_map_music`, `wm_area_set_pos`, `set_exit_grids`), the four words of
//! §4.1, `metarule`'s Fallout 2 rules (§5.1) and `metarule3`'s
//! sub-functions 100-111 (§5.2). They act on SPEC-018's world state
//! ([`crate::WorldState::fallout2`]). Every other host word keeps its
//! SPEC-005 row (§3.1), and under Fallout 1 nothing here is reached.
//!
//! A nonzero reference that names no live object behaves exactly as null,
//! with a diagnostic (§1, Decision 11).
use crate::{
    WorldEffect,
    assets::WorldAssets,
    fallout2::Fallout2World,
    host::{HostResult, WorldHost},
    queue::EventKind,
    world::ObjectRef,
};
use vault_formats::pro::{BasicFlags, Category, ItemData, PrototypeData};
use vault_vm::{
    ExternalValue, Value, Vm, VmError, WordSet,
    extensions::{FALLOUT2, HostCall, base_tag},
};

fn integer(value: i32) -> Option<ExternalValue> {
    Some(ExternalValue::Value(Value::integer(value)))
}
/// The map hexes, 0-39999.
fn hex_valid(hex: i32) -> bool {
    (0..40000).contains(&hex)
}
/// The `metarule` rules SPEC-017 §5.1 gives a Fallout 2 meaning: rule 15
/// with the engine-owned elevator table, and the new rules 17-51.
fn fallout2_rule(rule: i32) -> bool {
    matches!(rule, 15 | 17..=19 | 30..=32 | 40 | 42..=51)
}
/// SPEC-005's track-name limit, which SPEC-017 §4.4 and §5.2 keep.
const TRACK_BYTES: usize = 15;
fn track_name(text: &[u8]) -> String {
    String::from_utf8_lossy(&text[..text.len().min(TRACK_BYTES)]).into_owned()
}
/// SPEC-017 §4.1 command 13 of `inven_cmds`.
const INVEN_CMD_INDEX_PTR: i32 = 13;

impl<A: WorldAssets> WorldHost<A> {
    /// The active profile's word set (SPEC-017 §2.3): Fallout 2's while a
    /// Fallout 2 game is running, Fallout 1's otherwise. The profile, not
    /// the script, decides a word's meaning.
    pub fn word_set(&self) -> WordSet {
        if self.world.fallout2.is_some() {
            WordSet::FALLOUT2
        } else {
            WordSet::FALLOUT1
        }
    }
    /// Whether `call` takes its SPEC-017 row rather than its SPEC-005 one:
    /// under Fallout 2, a reassigned or new word, and `metarule` with one of
    /// the §5.1 rules.
    pub(crate) fn fallout2_row(&self, call: &HostCall) -> bool {
        let word = call.spec.word;
        self.world.fallout2.is_some()
            && (FALLOUT2.iter().any(|spec| spec.word == word)
                || (word == 0x810b && fallout2_rule(call.int(1))))
    }
    /// A reference operand: `None` for null, and (Decision 11) for a
    /// nonzero reference that names no live object, with a diagnostic.
    fn live(&mut self, name: &str, reference: u32) -> Option<ObjectRef> {
        if reference == 0 {
            return None;
        }
        if self.world.get(reference).is_some() {
            return Some(reference);
        }
        self.world.diagnostic(format!(
            "{name}: reference {reference:#x} names no live object; treated as null"
        ));
        None
    }
    fn fallout2_world(&self) -> Result<&Fallout2World, VmError> {
        self.world
            .fallout2
            .as_deref()
            .ok_or_else(|| VmError::Host("no Fallout 2 game is running".into()))
    }
    fn fallout2_world_mut(&mut self) -> Result<&mut Fallout2World, VmError> {
        self.world
            .fallout2
            .as_deref_mut()
            .ok_or_else(|| VmError::Host("no Fallout 2 game is running".into()))
    }
    /// One SPEC-017 row. Operands are in pop order: `call.int(0)` is the
    /// public call's last argument.
    pub(crate) fn fallout2_call(&mut self, vm: &mut Vm, call: &HostCall) -> HostResult {
        let name = call.spec.name;
        match call.spec.word {
            0x80b2 => {
                self.mark_area_known(call.int(2), call.int(1), call.int(0))?;
                Ok(None)
            }
            0x80cf => {
                let [test, lower_right, lower_left, upper_right, upper_left] =
                    std::array::from_fn(|index| call.int(index));
                Ok(integer(i32::from(tile_in_tile_rect(
                    [upper_left, upper_right, lower_left, lower_right],
                    test,
                ))))
            }
            0x80e1 => self.metarule3(vm, call),
            0x80e2 => {
                let track = call.text(0).unwrap_or_default().to_vec();
                self.set_map_music(call.int(1), &track)?;
                Ok(None)
            }
            0x80e5 => {
                let (area, x, y) = (call.int(2), call.int(1), call.int(0));
                let state = self.fallout2_world_mut()?;
                let [width, height] = state.world_size();
                let present = state.tables.city.get(area).is_some();
                match usize::try_from(area)
                    .ok()
                    .and_then(|area| state.areas.get_mut(area))
                {
                    Some(target)
                        if present && (0..width).contains(&x) && (0..height).contains(&y) =>
                    {
                        target.position = [x, y];
                    }
                    _ => self.world.diagnostic(format!(
                        "{name}: area {area} at ({x}, {y}) is outside the table or the world"
                    )),
                }
                Ok(None)
            }
            0x80e6 => {
                let [orientation, hex, elevation, map, marked] =
                    std::array::from_fn(|index| call.int(index));
                self.set_exit_grids(marked, [map, hex, elevation, orientation]);
                Ok(None)
            }
            0x8156 => {
                let (index, command) = (call.int(0), call.int(1));
                let Some(owner) = self.live(name, call.uint(2)) else {
                    self.world.diagnostic(format!("{name}: null owner"));
                    return Ok(integer(0));
                };
                if command != INVEN_CMD_INDEX_PTR {
                    self.world
                        .diagnostic(format!("{name}: command {command} is not defined"));
                    return Ok(integer(0));
                }
                let entry = usize::try_from(index).ok().and_then(|index| {
                    self.world
                        .get(owner)
                        .and_then(|object| object.inventory.get(index))
                        .map(|(item, _)| *item)
                });
                Ok(integer(entry.map_or(0, |item| item as i32)))
            }
            0x8157..=0x8159 => {
                self.fallout2_registration(call);
                Ok(None)
            }
            0x810b => self.fallout2_metarule(call.int(1), call.int(0)),
            word => Err(VmError::Opcode(word)),
        }
    }
    /// §4.2 `mark_area_known`: type 0 sets an area's known state, type 1 a
    /// map's known and visited flags. Anything else changes nothing, with a
    /// diagnostic. Subtiles and the party's position never change.
    fn mark_area_known(&mut self, kind: i32, index: i32, state: i32) -> Result<(), VmError> {
        let world = self.fallout2_world_mut()?;
        let done = match (kind, u8::try_from(state)) {
            (0, Ok(known @ 0..=2)) => {
                let present = world.tables.city.get(index).is_some();
                match usize::try_from(index)
                    .ok()
                    .and_then(|index| world.areas.get_mut(index))
                {
                    Some(area) if present => {
                        area.known = known;
                        true
                    }
                    _ => false,
                }
            }
            (1, Ok(known @ 0..=2)) => {
                let present = world.tables.maps.get(index).is_some();
                match usize::try_from(index)
                    .ok()
                    .and_then(|index| world.map_flags.get_mut(index))
                {
                    Some(flags) if present => {
                        match known {
                            0 => *flags &= !0x03,
                            1 => *flags |= 0x01,
                            _ => *flags |= 0x03,
                        }
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        };
        if !done {
            self.world.diagnostic(format!(
                "mark_area_known: mark type {kind}, index {index}, state {state} changes nothing"
            ));
        }
        Ok(())
    }
    /// §4.4 `set_map_music`: the map's city track becomes `track` (cut to 15
    /// bytes, empty removing the override), restarting at once on the
    /// current map. A map without a `maps.txt` record changes nothing.
    fn set_map_music(&mut self, map: i32, track: &[u8]) -> Result<(), VmError> {
        let current = self.world.map.header.map_number;
        let state = self.fallout2_world_mut()?;
        let Some(record) = state.tables.maps.get(map) else {
            self.world
                .diagnostic(format!("set_map_music: map {map} has no maps.txt record"));
            return Ok(());
        };
        let table = record.music.clone();
        let track = track_name(track);
        let playing = if track.is_empty() {
            state.map_music.remove(&map);
            table
        } else {
            state.map_music.insert(map, track.clone());
            Some(track)
        };
        if map == current
            && let Some(track) = playing
        {
            self.request_music(&track);
        }
        Ok(())
    }
    /// §4.6 `set_exit_grids`: every exit grid on the current map at the
    /// marked elevation, hidden or not, takes the four destination words
    /// as given. Its art is kept.
    fn set_exit_grids(&mut self, marked: i32, destination: [i32; 4]) {
        let Ok(marked @ 0..=2) = u8::try_from(marked) else {
            return;
        };
        let grids: Vec<ObjectRef> = self
            .world
            .iter()
            .filter(|(_, object)| {
                crate::skills::is_exit_grid(object.raw.pid)
                    && object.owner.is_none()
                    && object.raw.hex.is_some()
                    && object.raw.elevation == marked
            })
            .map(|(reference, _)| reference)
            .collect();
        let [map, hex, elevation, orientation] = destination;
        for grid in grids {
            if let Some(object) = self.world.get_mut(grid)
                && let vault_formats::map::ObjectState::Other { detail, .. } = &mut object.raw.state
            {
                *detail = vault_formats::map::ObjectDetail::ExitGrid {
                    map,
                    hex,
                    elevation,
                    orientation,
                };
            }
        }
    }
    /// §4.1 `reg_anim_change_fid`, `reg_anim_take_out` and
    /// `reg_anim_turn_towards`: outside combat, one entry of the sequence
    /// being registered (SPEC-005 §2.6). In combat the operands are
    /// consumed and nothing happens; a null object only diagnoses.
    fn fallout2_registration(&mut self, call: &HostCall) {
        let name = call.spec.name;
        let op = call.spec.word;
        if self.world.combat {
            return;
        }
        let Some(object) = self.live(name, call.uint(2)) else {
            self.world.diagnostic(format!("{name}: null object"));
            return;
        };
        let value = call.int(1);
        match op {
            0x8158 if !(1..=15).contains(&value) => {
                self.world.diagnostic(format!(
                    "{name}: weapon code {value} is outside 1-15; no entry"
                ));
                return;
            }
            // A take-out animation is a critter's (an interpretation: the
            // spec names no non-critter case).
            0x8158 if !self.world.critter_art(object) => {
                self.world
                    .diagnostic(format!("{name}: {object:#x} has no critter art; no entry"));
                return;
            }
            0x8159 if !hex_valid(value) => {
                self.world
                    .diagnostic(format!("{name}: hex {value} is outside 0-39999; no entry"));
                return;
            }
            _ => {}
        }
        let values = call.args.iter().map(|arg| arg.int()).collect();
        self.queue_animation(op, values, None);
    }
    /// SPEC-017 §2.4 event 24: before the player pushes party member
    /// `critter` (SPEC-019 §3.6 step 1), its `push_p_proc` is requested
    /// with the player as source and the critter as target. Returns whether
    /// the script overrode the default push. Under Fallout 1 no event
    /// above 23 exists, so nothing is requested.
    pub fn request_push(&mut self, critter: ObjectRef) -> Result<bool, crate::WorldError> {
        if self.world.fallout2.is_none() {
            return Ok(false);
        }
        let Some(script) = self.world.script_for(critter) else {
            return Ok(false);
        };
        let result = self.dispatch(crate::world::ScriptRequest {
            script,
            event: crate::ScriptEvent::Push,
            source: self.world.player,
            target: critter,
            fixed: 0,
            action: -1,
            only_present: false,
        })?;
        Ok(result.overridden)
    }
    /// SPEC-017 §3.1 `party_add` under Fallout 2 (SPEC-019 §3.14): the
    /// object joins unless the list already holds 32 entries; a critter
    /// switches to team 0; a member with a `party.txt` record gets its
    /// retained control record back, or a new one.
    pub(crate) fn fallout2_party_add(&mut self, member: ObjectRef) {
        if self.world.party.contains(&member) {
            return;
        }
        if self.world.party.len() >= 32 {
            self.world
                .diagnostic("party_add: the party list already holds 32 entries");
            return;
        }
        self.world.party.insert(member);
        self.set_team(member, 0);
        let Some((id, pid)) = self
            .world
            .get(member)
            .map(|object| (object.raw.id, object.raw.pid))
        else {
            return;
        };
        if let Some(state) = self.world.fallout2.as_deref_mut()
            && !state.control.contains_key(&id)
        {
            let record = state
                .retained
                .remove(&id)
                .unwrap_or_else(|| state.joining_record(pid));
            if record.record != -1 {
                state.control.insert(id, record);
            }
        }
    }
    /// `party_remove` under Fallout 2 (SPEC-019 §3.14): the control record
    /// is kept as a retained record. Nothing happens under Fallout 1.
    pub(crate) fn fallout2_party_left(&mut self, member: ObjectRef) {
        let Some(id) = self.world.get(member).map(|object| object.raw.id) else {
            return;
        };
        if let Some(state) = self.world.fallout2.as_deref_mut()
            && let Some(record) = state.control.remove(&id)
        {
            state.retained.insert(id, record);
        }
    }
    /// The SPEC-019 §2.5 control record of `critter`: its stored one, or
    /// for a party member without one the record joining creates.
    fn control_record(
        &self,
        critter: ObjectRef,
    ) -> Option<vault_formats::fallout2::save::MemberRecord> {
        let state = self.world.fallout2.as_deref()?;
        let object = self.world.get(critter)?;
        let record = match state.control.get(&object.raw.id) {
            Some(record) => *record,
            None if critter != self.world.player && self.world.party_member(critter) => {
                state.joining_record(object.raw.pid)
            }
            None => return None,
        };
        (record.record != -1).then_some(record)
    }
    /// §5.1 the Fallout 2 `metarule` rules ([`fallout2_rule`]); the rest
    /// keep SPEC-005's rows.
    fn fallout2_metarule(&mut self, rule: i32, parameter: i32) -> HostResult {
        let name = "metarule";
        let number = self.world.map.header.map_number;
        let value = match rule {
            // SPEC-018 §4.9: a type without a record does nothing, with a
            // diagnostic.
            15 => {
                if self.fallout2_elevator(parameter).is_some() {
                    self.world.emit(WorldEffect::Elevator(parameter));
                }
                0
            }
            17 => {
                let state = self.fallout2_world()?;
                i32::from(
                    state.tables.city.get(parameter).is_some()
                        && usize::try_from(parameter)
                            .ok()
                            .and_then(|area| state.areas.get(area))
                            .is_some_and(|area| area.known >= 1),
                )
            }
            18 => match self.live(name, parameter as u32) {
                Some(critter) => i32::from(self.world.queue.events().any(|event| {
                    event.object == critter && matches!(event.kind, EventKind::Drug { .. })
                })),
                None => 0,
            },
            19 => {
                let state = self.fallout2_world()?;
                i32::from(
                    state.tables.maps.get(parameter).is_some()
                        && usize::try_from(parameter)
                            .ok()
                            .and_then(|map| state.map_flags.get(map))
                            .is_some_and(|flags| flags & 0x01 != 0),
                )
            }
            30 => {
                let car = self.fallout2_world()?.car;
                if car.has_car { car.area } else { -1 }
            }
            31 => {
                let area = self.fallout2_world()?.tables.city.area_of_map(number);
                let car = &mut self.fallout2_world_mut()?.car;
                car.has_car = true;
                car.area = area;
                car.map = number;
                0
            }
            32 => {
                let car = &mut self.fallout2_world_mut()?.car;
                if car.has_car {
                    car.fuel = (i64::from(car.fuel) + i64::from(parameter))
                        .clamp(0, i64::from(crate::fallout2::FUEL_MAX))
                        as i32;
                }
                0
            }
            40 => i32::from(
                (0..18).contains(&parameter)
                    && self
                        .world
                        .player_sheet()
                        .is_some_and(|sheet| sheet.tags.contains(&parameter)),
            ),
            42 => {
                match self.live(name, parameter as u32) {
                    Some(critter) => self.drop_everything(critter),
                    None => self
                        .world
                        .diagnostic("metarule 42 (drop all inventory): null critter"),
                }
                0
            }
            43 => {
                match self.live(name, parameter as u32) {
                    Some(critter) => self.unwield_hand(critter),
                    None => self.world.diagnostic("metarule 43 (unwield): null critter"),
                }
                0
            }
            44 => self.fallout2_world()?.position[0],
            45 => self.fallout2_world()?.position[1],
            46 => self.fallout2_world()?.area,
            47 => self.world.ai.language_filter.unwrap_or(0).clamp(0, 1),
            // SPEC-014 §2.3: `violence_level` 0-3, default 3.
            48 => self.world.violence.unwrap_or(3).clamp(0, 3),
            49 => {
                let damage = self
                    .live(name, parameter as u32)
                    .and_then(|item| self.world.item_data(item))
                    .and_then(|item| match &item.data {
                        ItemData::Weapon(weapon) => Some(weapon.damage_type),
                        _ => None,
                    });
                damage.unwrap_or_else(|| {
                    self.world
                        .diagnostic("metarule 49 (weapon damage type): not a weapon");
                    -1
                })
            }
            50 | 51 => {
                let critter = self
                    .live(name, parameter as u32)
                    .and_then(|critter| self.world.get(critter))
                    .and_then(
                        |object| match object.prototype.as_deref().map(|p| &p.data) {
                            Some(PrototypeData::Critter(critter)) => {
                                Some((critter.flags.0, critter.kill_category))
                            }
                            _ => None,
                        },
                    );
                match (rule, critter) {
                    (50, Some((flags, _))) => i32::from(flags & 0x0000_0002 != 0),
                    (50, None) => 0,
                    (_, Some((_, kill))) => kill,
                    (_, None) => {
                        self.world
                            .diagnostic("metarule 51 (critter kill type): not a critter");
                        -1
                    }
                }
            }
            _ => 0,
        };
        Ok(integer(value))
    }
    /// §5.1 rule 42: every inventory entry, in inventory order, is removed
    /// and placed on the critter's own hex and elevation, with its worn and
    /// held flags cleared. Taking the player's armor off removes its effects
    /// and leaves the default player art, as `rm_obj_from_inven` does.
    fn drop_everything(&mut self, critter: ObjectRef) {
        let entries = self
            .world
            .get(critter)
            .map(|object| object.inventory.clone())
            .unwrap_or_default();
        let mut worn = false;
        for (item, count) in entries {
            worn |= self
                .world
                .get(item)
                .is_some_and(|object| object.raw.flags.contains(BasicFlags::WORN));
            if let Err(error) = self.world.drop_item(critter, item, count) {
                self.world
                    .diagnostic(format!("metarule 42 (drop all inventory): {error}"));
            }
        }
        if worn {
            self.unworn_art(critter);
        }
    }
    /// §5.1 rule 43: unwield as `inven_unwield` does for its owner: the
    /// critter's right hand, or the player's active hand.
    fn unwield_hand(&mut self, critter: ObjectRef) {
        let flag = if critter == self.world.player && self.world.active_hand == 0 {
            BasicFlags::LEFT_HAND.0
        } else {
            BasicFlags::RIGHT_HAND.0
        };
        let inventory = self
            .world
            .get(critter)
            .map(|object| object.inventory.clone())
            .unwrap_or_default();
        for (item, _) in inventory {
            if let Some(object) = self.world.get_mut(item) {
                object.raw.flags.0 &= !flag;
            }
        }
    }
    /// §5.2 `metarule3`: only the sub-function's tag is checked; each
    /// sub-function reads the parameters it uses, and a used parameter of
    /// another tag makes it do nothing, with a diagnostic, and give I 0.
    fn metarule3(&mut self, vm: &Vm, call: &HostCall) -> HostResult {
        let sub = call.int(3);
        // p1, p2, p3 as integers when their masked tag is I.
        let value = |index: usize| {
            let value = call.args[index].value;
            (base_tag(value) == Value::INTEGER).then_some(value.payload as i32)
        };
        let [p1, p2, p3] = [value(2), value(1), value(0)];
        let uses: &[Option<i32>] = match sub {
            100 => &[p1, p2],
            101 | 104 | 106 => &[p1, p2, p3],
            103 | 108 | 109 => &[p1],
            105 | 107 => &[p1, p2],
            102 | 110 | 111 => &[],
            _ => {
                self.world
                    .diagnostic(format!("metarule3: sub-function {sub} is not defined"));
                return Ok(integer(0));
            }
        };
        if uses.iter().any(Option::is_none) {
            self.world.diagnostic(format!(
                "metarule3 {sub}: a parameter it uses is not an integer"
            ));
            return Ok(integer(0));
        }
        let [p1, p2, p3] = [p1, p2, p3].map(|value| value.unwrap_or(0));
        let result = match sub {
            100 => {
                if let Some(object) = self.live("metarule3 100", p1 as u32)
                    && let Some(script) = self.world.script_for(object)
                {
                    self.world.queue.retain(|event| {
                        !matches!(event.kind, EventKind::Script { script: s, parameter }
                            if s == script && parameter == p2)
                    });
                }
                0
            }
            101 => {
                let state = self.fallout2_world_mut()?;
                if p3 >= 0
                    && let Some(cell) = state.tables.worldmap.cell_of([p1, p2])
                {
                    state.mark_subtiles(cell, p3 as usize);
                }
                0
            }
            102 => {
                let value = call.args[2].value;
                if base_tag(value) != Value::STATIC_STRING {
                    self.world
                        .diagnostic("metarule3 102: the track name is not a string");
                    return Ok(integer(0));
                }
                let track = track_name(vm.host_string(value)?.unwrap_or_default());
                let state = self.fallout2_world_mut()?;
                state.world_map_music = (!track.is_empty()).then_some(track);
                if state.world_map_open {
                    let track = self.world_map_track();
                    self.request_music(&track);
                }
                0
            }
            103 => match usize::try_from(p1) {
                Ok(category @ 0..=14) => self.world.kills[category],
                Ok(category @ 15..=18) => self.fallout2_world()?.kills[category - 15],
                _ => 0,
            },
            104 => {
                let state = self.fallout2_world_mut()?;
                if state.tables.maps.get(p1).is_none() {
                    -1
                } else {
                    state.mark_entrances(p1, p2 != 0, p3);
                    0
                }
            }
            105 => {
                let state = self.fallout2_world()?;
                state
                    .tables
                    .worldmap
                    .cell_of([p1, p2])
                    .map_or(-1, |cell| i32::from(state.discovery(cell)))
            }
            106 => {
                let previous = self.live("metarule3 106", p3 as u32);
                self.next_critter(p1, p2, previous)
                    .map_or(0, |critter| critter as i32)
            }
            107 => {
                if let Some(object) = self.live("metarule3 107", p1 as u32) {
                    self.set_base_art(object, (p2 as u32 & 0xfff) as u16);
                }
                0
            }
            108 => {
                if hex_valid(p1) {
                    self.world.set_view_center(p1);
                }
                0
            }
            109 => self
                .live("metarule3 109", p1 as u32)
                .and_then(|critter| self.control_record(critter))
                .map_or(-1, |record| record.fields[6]),
            110 => {
                let car = self.fallout2_world()?.car;
                i32::from(car.has_car && car.fuel == 0)
            }
            111 => {
                let number = self.world.map.header.map_number;
                self.fallout2_world()?.tables.city.area_of_map(number)
            }
            _ => 0,
        };
        Ok(integer(result))
    }
    /// §5.2 sub-function 106: the critter after `previous` (or the first)
    /// among the critters placed at the hex and elevation, in object order,
    /// the player and dead or hidden critters included.
    fn next_critter(
        &self,
        hex: i32,
        elevation: i32,
        previous: Option<ObjectRef>,
    ) -> Option<ObjectRef> {
        if !hex_valid(hex) || !(0..=2).contains(&elevation) {
            return None;
        }
        let mut critters = self.world.iter().filter(|(_, object)| {
            object.category() == Category::Critter
                && object.owner.is_none()
                && object.raw.hex == Some(hex)
                && i32::from(object.raw.elevation) == elevation
        });
        match previous {
            None => critters.next().map(|(reference, _)| reference),
            Some(previous) => {
                critters.find(|(reference, _)| *reference == previous)?;
                critters.next().map(|(reference, _)| reference)
            }
        }
    }
    /// §5.2 sub-function 107: base `base` for `object`'s art at once,
    /// keeping every other field. For the player it is the default player
    /// art, shown at once only while no armor is worn.
    fn set_base_art(&mut self, object: ObjectRef, base: u16) {
        let show = if object == self.world.player {
            self.world.default_player_art = base;
            self.worn_armor(object).is_none()
        } else {
            true
        };
        if show && let Some(target) = self.world.get_mut(object) {
            target.raw.art_id = (target.raw.art_id & !0xfff) | u32::from(base);
        }
    }
}

/// §4.3 `tile_in_tile_rect`: whether `test` lies in the column and row
/// bounding box of the four corners (hex = 200 x row + column), bounds
/// included. Any hex outside 0-39999 gives `false`.
pub fn tile_in_tile_rect(corners: [i32; 4], test: i32) -> bool {
    if !corners.iter().chain([&test]).all(|hex| hex_valid(*hex)) {
        return false;
    }
    let split = |hex: i32| [hex % 200, hex / 200];
    let [column, row] = split(test);
    let points = corners.map(split);
    let range = |axis: usize| {
        let values = points.map(|point| point[axis]);
        values.iter().min().copied().unwrap_or(0)..=values.iter().max().copied().unwrap_or(0)
    };
    range(0).contains(&column) && range(1).contains(&row)
}

impl Fallout2World {
    /// SPEC-017 §5.2 sub-function 101: every cell whose column and row each
    /// differ from `cell`'s by at most `radius`, clipped to the grid, rises
    /// from unknown to known. No cell is lowered.
    pub fn mark_subtiles(&mut self, [column, row]: [usize; 2], radius: usize) {
        let [columns, rows] = self.grid();
        for y in row.saturating_sub(radius)..=row.saturating_add(radius).min(rows - 1) {
            for x in column.saturating_sub(radius)..=column.saturating_add(radius).min(columns - 1)
            {
                let value = &mut self.discovery[y * columns + x];
                if *value == 0 {
                    *value = 1;
                }
            }
        }
    }
    /// SPEC-017 §5.2 sub-function 104 over SPEC-018 §5.8's "Entrances by
    /// map": every valid slot whose destination is `map` and whose entry
    /// elevation is `elevation`, or -1, or any when `elevation` is -1.
    pub fn mark_entrances(&mut self, map: i32, known: bool, elevation: i32) {
        let tables = self.tables.clone();
        for area in tables.city.areas() {
            let Some(state) = self.areas.get_mut(area.number as usize) else {
                continue;
            };
            for (slot, entrance) in area.entrances.iter().enumerate() {
                let Some(entrance) = entrance else {
                    continue;
                };
                if i64::from(entrance.map) == i64::from(map)
                    && (elevation == -1
                        || entrance.elevation == -1
                        || entrance.elevation == elevation)
                    && let Some(flag) = state.entrances.get_mut(slot)
                {
                    *flag = known;
                }
            }
        }
    }
}
