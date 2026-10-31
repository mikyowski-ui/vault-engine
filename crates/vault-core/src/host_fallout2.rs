//! SPEC-018 in the host: parsing the tables for Play or a load (§6.6 step
//! 1), the new-game world state (§6.5), the script gate's Play rules
//! (§6.7, SPEC-017 §8.7), and the world-map rules of §5 (travel,
//! discovery, encounters and spawning, entering areas and entrances).
use crate::{
    ObjectRef, Perk, RandomSource, WorldEffect, WorldError,
    assets::WorldAssets,
    fallout2::{self, AreaEntry, Conditions, Fallout2Tables, Fallout2World, Selected, Step},
    host::WorldHost,
    sfall::encounter_flag,
    worldmap::TravelSpeed,
};
use std::sync::Arc;
use vault_formats::{
    AutomapDb, HexGrid,
    fallout2::{TableError, worldmap::CritterType},
};

/// One Fallout 2 movement-loop iteration's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fallout2Travel {
    Idle,
    Moving,
    Arrived,
    Blocked,
    /// A queue handler returned nonzero.
    Interrupted(i32),
    /// §5.6 an encounter was selected; travel stopped.
    Encounter(Selected),
    /// SPEC-019 §3.19.3 the iteration emptied the car's tank: travel
    /// stopped at rest, and the screen shows E-fuel-out.
    OutOfFuel,
}

/// SPEC-019 §2.3 E-fuel-out.
pub const FUEL_OUT: &str = "Your car is out of fuel.";

/// SPEC-018 §4.8: the world tables whose Fallout 1 overrides are not read
/// under Fallout 2.
const WORLD_TABLES: [crate::tables::Table; 6] = [
    crate::tables::Table::Terrain,
    crate::tables::Table::Frequency,
    crate::tables::Table::Region,
    crate::tables::Table::Towns,
    crate::tables::Table::Hotspots,
    crate::tables::Table::Maps,
];

impl<A: WorldAssets> WorldHost<A> {
    /// SPEC-017 §8.5: whether a Fallout 2 game runs with its scripts gated
    /// (map preview and the VM harness are not gated).
    pub fn scripts_gated(&self) -> bool {
        self.world.fallout2.is_some() && !fallout2::SCRIPT_GATE_OPEN
    }
    /// §6.6 step 1 (and every load): parse the §4 tables through the master
    /// database as mounted. A refusal names the file and line.
    pub fn load_fallout2_tables(&self) -> Result<Arc<Fallout2Tables>, TableError> {
        let mut read = |path: &str| -> Result<Option<Vec<u8>>, String> {
            if !self.assets.available(path.as_bytes()) {
                return Ok(None);
            }
            self.assets
                .resource(path)
                .map(Some)
                .map_err(|error| error.to_string())
        };
        Fallout2Tables::load(&mut read).map(Arc::new)
    }
    /// §6.5-§6.6 step 3: install the new game's world state, after the
    /// reset and before the start map loads: the clock at tick 264600, the
    /// world state from the tables, an automap database of 1000 map
    /// numbers, and the table diagnostics.
    pub fn begin_fallout2_game(&mut self, tables: Arc<Fallout2Tables>) {
        self.set_game_profile(crate::GameProfile::Fallout2);
        let start = tables.maps.match_name(
            fallout2::START_MAP
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .as_bytes(),
        );
        if start < 0 {
            self.world
                .diagnostic("maps/artemple.map has no data/maps.txt record; its map number is -1");
        }
        for note in &tables.diagnostics {
            self.world.diagnostic(note.to_string());
        }
        self.world.clock.set(fallout2::START_TICKS);
        self.world.persistence.automap =
            AutomapDb::fallout2(|map| tables.maps.excluded(map as i32));
        self.world.persistence.campaign_status = 0;
        self.world.fallout2 = Some(Box::new(Fallout2World::new_game(tables, start)));
        // §6.5 (Revision 3): the player's record gets the slice that
        // `scripts.lst` entry 0 declares, all 0.
        let count = self.assets.local_count(crate::world::PLAYER_SCRIPT_FILE);
        if let Some(sid) = self.world.script_for(self.world.player) {
            self.world.set_player_locals(sid, count, None);
        }
        self.note_world_table_overrides();
    }
    /// §4.8: a present Fallout 1 world-table override gets a diagnostic;
    /// it is not read.
    pub(crate) fn note_world_table_overrides(&mut self) {
        for table in WORLD_TABLES {
            let path = table.override_path();
            if self.assets.available(path.as_bytes()) {
                self.world.diagnostic(format!(
                    "{path} is a Fallout 1 table and is not read under Fallout 2"
                ));
            }
        }
    }
    /// SPEC-017 §8.7 rule 6: after a completed new-game hand-off or a
    /// successful load, the monitor prints the gate notice, once.
    pub fn fallout2_notice(&mut self) {
        if self.scripts_gated() {
            self.world.emit(WorldEffect::Message(
                fallout2::GATE_NOTICE.as_bytes().to_vec(),
            ));
        }
    }
    /// The SPEC-018 §5.1 map file of map number `number`, under Fallout 2.
    pub(crate) fn fallout2_map_path(&self, number: i32) -> Option<String> {
        let fallout2 = self.world.fallout2.as_ref()?;
        fallout2.tables.maps.get(number).map(|record| record.path())
    }
    /// The world-map state, or an error outside a Fallout 2 game.
    fn fallout2_state(&mut self) -> Result<&mut Fallout2World, WorldError> {
        self.world
            .fallout2
            .as_deref_mut()
            .ok_or_else(|| WorldError::Invalid("no Fallout 2 game is running".into()))
    }
    /// SPEC-019 §3.12 building the world map (on opening, which also takes
    /// W and Q, and on each return from a town map): the reveal at the
    /// party's cell, and the travel-dot count from 0. Plays `03wrldmp`.
    pub fn fallout2_build_world_map(&mut self, opening: bool) -> Result<(), WorldError> {
        let speed = if opening {
            Some(self.travel_speed()?)
        } else {
            None
        };
        let scout = self.world.perk(Perk::Scout) > 0;
        let state = self.fallout2_state()?;
        if let Some(speed) = speed {
            state.speed = Some(speed);
            // SPEC-019 §3.19.2: driving is decided at every opening.
            let left_area = state.tables.city.area_of_map(state.last_map);
            state.driving = state.car.drives(state.last_map, left_area);
        }
        state.reveal(scout);
        state.iterations = 0;
        state.mark = None;
        state.world_map_open = true;
        if opening {
            // `03wrldmp`, or SPEC-017 §5.2's `metarule3` 102 override.
            let track = self.world_map_track();
            self.request_music(&track);
        }
        Ok(())
    }
    /// Begin travel toward a world pixel.
    pub fn fallout2_travel_to(&mut self, destination: [i32; 2]) -> Result<(), WorldError> {
        self.fallout2_state()?.travel_to(destination);
        Ok(())
    }
    /// SPEC-018 §5.4-§5.6 one movement-loop iteration on foot: the terrain
    /// schedule, `Q` charged every iteration, the discovery reveal and
    /// travel dot on iterations 1, 4, 7, ..., and every `W` iterations the
    /// 24-hour healing and the encounter check. In the car with fuel
    /// (SPEC-019 §3.19.3) the iteration makes the car's steps instead, and
    /// one that moved uses 100 fuel; one that empties the tank stops
    /// travel.
    pub fn fallout2_travel_iteration(&mut self) -> Result<Fallout2Travel, WorldError> {
        if self.fallout2_state()?.travel.is_none() {
            return Ok(Fallout2Travel::Idle);
        }
        let speed: TravelSpeed = match self.fallout2_state()?.speed {
            Some(speed) => speed,
            None => self.travel_speed()?,
        };
        let state = self.fallout2_state()?;
        state.mark = None;
        let (step, emptied) = if state.car_travel() {
            let (step, made) = state.drive();
            if made > 0 {
                state.car.fuel = (state.car.fuel - fallout2::FUEL_PER_ITERATION).max(0);
            }
            (step, made > 0 && state.car.fuel == 0)
        } else {
            (state.step(), false)
        };
        let result = self.charge_travel_time(speed.ticks)?;
        if result != 0 {
            self.fallout2_state()?.travel = None;
            return Ok(Fallout2Travel::Interrupted(result));
        }
        let scout = self.world.perk(Perk::Scout) > 0;
        let state = self.fallout2_state()?;
        if state.iterations.is_multiple_of(3) {
            state.reveal(scout);
            state.mark = Some(state.position);
        }
        state.iterations = state.iterations.wrapping_add(1);
        state.interval += 1;
        if state.interval >= speed.interval {
            state.interval = 0;
            self.heal_party(24);
            if let Some(selected) = self.fallout2_check_encounter()? {
                self.fallout2_state()?.travel = None;
                return Ok(Fallout2Travel::Encounter(selected));
            }
        }
        if emptied {
            self.fallout2_state()?.travel = None;
            return Ok(Fallout2Travel::OutOfFuel);
        }
        Ok(match step {
            Step::Moving => Fallout2Travel::Moving,
            Step::Arrived => Fallout2Travel::Arrived,
            Step::Blocked => Fallout2Travel::Blocked,
        })
    }
    /// §5.6 the encounter check at the party's cell.
    pub fn fallout2_check_encounter(&mut self) -> Result<Option<Selected>, WorldError> {
        if let Some(forced) = self.take_forced_encounter() {
            return Ok(Some(forced));
        }
        let luck = self.world.player_stat(6);
        let explorer = self.world.perk(Perk::Explorer);
        let level = self.world.player_sheet().map_or(1, |sheet| sheet.level);
        let ticks = self.world.clock.ticks;
        let globals = self.world.gvars.clone().unwrap_or_default();
        let world = &mut self.world;
        let Some(state) = world.fallout2.as_deref_mut() else {
            return Ok(None);
        };
        let mut notes = Vec::new();
        let mut conditions = Conditions {
            globals: &globals,
            level,
            ticks,
            rng: &mut world.rng,
            failure: None,
        };
        let selected = state.check_encounter(&mut conditions, luck, explorer, &mut notes)?;
        for note in notes {
            world.diagnostic(note);
        }
        Ok(selected)
    }
    /// §5.7 entering a selected encounter: its map at a start point (or the
    /// header's entrance), then its groups spawned in expression order.
    /// No campaign global is written (§6.2).
    pub fn fallout2_enter_encounter(&mut self, selected: Selected) -> Result<(), WorldError> {
        if let Some(flags) = selected.forced {
            return self.fallout2_enter_forced(selected.map, flags);
        }
        self.fallout2_load_encounter_map(selected.map)?;
        let tables = self.fallout2_state()?.tables.clone();
        let Some(table) = tables.worldmap.tables.get(&selected.table) else {
            return Ok(());
        };
        let Some(entry) = table.entries.iter().find(|e| e.number == selected.entry) else {
            return Ok(());
        };
        for group in &entry.expression.groups {
            let Some(definition) = tables.worldmap.group(&group.name) else {
                self.world.diagnostic(format!(
                    "encounter group {} is not defined; skipped",
                    group.name
                ));
                continue;
            };
            self.fallout2_spawn_group(group.min, group.max, definition)?;
        }
        self.world_map_closed();
        Ok(())
    }
    /// SPEC-024b §4.11 a forced encounter: the map through §5.7's step 1,
    /// with no group, and the flags' effects.
    fn fallout2_enter_forced(&mut self, map: u32, flags: u32) -> Result<(), WorldError> {
        if flags & encounter_flag::FADEOUT != 0 {
            self.library.faded_out = true;
            self.world.emit(WorldEffect::Fade {
                out: true,
                duration: 0,
                from_black: false,
            });
        }
        self.fallout2_load_encounter_map(map)?;
        if flags & encounter_flag::NO_CAR != 0 {
            self.fallout2_state()?.driving = false;
        }
        self.world_map_closed();
        Ok(())
    }
    /// §5.7 "Dropping to terrain": a random map of the party cell's
    /// terrain, entered as an encounter map with nothing spawned. Returns
    /// whether a map loaded (an empty list leaves the world map open).
    pub fn fallout2_drop_to_terrain(&mut self) -> Result<bool, WorldError> {
        let world = &mut self.world;
        let state = world
            .fallout2
            .as_deref()
            .ok_or_else(|| WorldError::Invalid("no Fallout 2 game is running".into()))?;
        let Some(map) = state.terrain_map(&mut world.rng)? else {
            return Ok(false);
        };
        self.fallout2_load_encounter_map(map)?;
        self.world_map_closed();
        Ok(true)
    }
    /// §5.7 step 1: an encounter map's entrance, then the load.
    fn fallout2_load_encounter_map(&mut self, map: u32) -> Result<(), WorldError> {
        let world = &mut self.world;
        let state = world
            .fallout2
            .as_deref()
            .ok_or_else(|| WorldError::Invalid("no Fallout 2 game is running".into()))?;
        let entrance = state.encounter_entrance(map, &mut world.rng)?;
        let path = self
            .fallout2_map_path(map as i32)
            .ok_or_else(|| WorldError::Invalid(format!("map {map} is not in data/maps.txt")))?;
        self.load_map(&path)?;
        if let Some((hex, elevation)) = entrance {
            let facing = self.world.map.header.entry_orientation;
            self.place_player(hex, elevation, facing)?;
            self.world.place_party()?;
        }
        Ok(())
    }
    /// §5.7 step 2: one group of `min` to `max` critters around the player.
    fn fallout2_spawn_group(
        &mut self,
        min: i32,
        max: i32,
        group: &vault_formats::fallout2::worldmap::Group,
    ) -> Result<(), WorldError> {
        let count = fallout2::random(&mut self.world.rng, min, max)?;
        let direction = fallout2::random(&mut self.world.rng, 0, 5)?;
        let ticks = self.world.clock.ticks;
        let level = self.world.player_sheet().map_or(1, |sheet| sheet.level);
        let globals = self.world.gvars.clone().unwrap_or_default();
        let mut eligible: Vec<&CritterType> = Vec::new();
        {
            let mut conditions = Conditions {
                globals: &globals,
                level,
                ticks,
                rng: &mut self.world.rng,
                failure: None,
            };
            for critter in &group.types {
                let holds = critter.condition.holds(&mut conditions);
                if let Some(error) = conditions.failure.take() {
                    return Err(error);
                }
                if holds {
                    eligible.push(critter);
                }
            }
        }
        if eligible.is_empty() {
            return Ok(());
        }
        let any_ratio = eligible.iter().any(|critter| critter.ratio.is_some());
        let mut weights: Vec<i32> = eligible
            .iter()
            .map(|critter| match critter.ratio {
                Some(ratio) => ratio,
                None if any_ratio => 0,
                None => 1,
            })
            .collect();
        if weights.iter().sum::<i32>() == 0 {
            weights.fill(1);
        }
        let total: i32 = weights.iter().sum();
        // §4.4.5 (Revision 2): `Player(Perception)` is read as the group
        // spawns.
        let distance = group.distance.hexes(self.world.player_stat(1));
        let player = self.world.player;
        let (origin, elevation) = self
            .world
            .get(player)
            .map(|object| (object.raw.hex.unwrap_or(0), object.raw.elevation))
            .ok_or_else(|| WorldError::Invalid("no player".into()))?;
        for k in 0..count.max(0) {
            let critter = if eligible.len() == 1 {
                eligible[0]
            } else {
                let draw = fallout2::random(&mut self.world.rng, 1, total)?;
                let mut running = 0;
                let index = weights
                    .iter()
                    .position(|weight| {
                        running += weight;
                        running >= draw
                    })
                    .unwrap_or(0);
                eligible[index]
            };
            // §5.7 step 6 (Revision 2): a type's own distance replaces the
            // group's.
            let steps = critter.distance.unwrap_or(distance) + group.spacing * (k / 6);
            let toward = ((direction + k) % 6) as u8;
            let start = HexGrid::step(origin, toward, steps)
                .map_err(|_| WorldError::Invalid("player hex outside the grid".into()))?;
            let Some(hex) = self.fallout2_free_hex(start, elevation) else {
                self.world.diagnostic(format!(
                    "no free hex near {start} for encounter critter {:#010x}; not created",
                    critter.pid
                ));
                continue;
            };
            if let Err(error) = self.fallout2_create_critter(critter, hex, elevation, origin) {
                self.world.diagnostic(format!(
                    "encounter critter {:#010x} not created: {error}",
                    critter.pid
                ));
            }
        }
        Ok(())
    }
    /// §5.7 placement: the first hex, ring by ring up to radius 5 in
    /// SPEC-003's neighbor order, that no blocking object occupies.
    fn fallout2_free_hex(&self, center: i32, elevation: u8) -> Option<i32> {
        let free = |hex: i32| self.world.movement_blocker(hex, elevation, 0).is_none();
        if free(center) {
            return Some(center);
        }
        for radius in 1..=5 {
            for hex in ring(center, radius) {
                if free(hex) {
                    return Some(hex);
                }
            }
        }
        None
    }
    /// §5.7 creation: the critter at `hex`, facing the player at `player`,
    /// with its items, held weapon, script and dead state. An item or
    /// scenery type is that object at `hex`, with nothing else applied
    /// (Revision 2).
    fn fallout2_create_critter(
        &mut self,
        critter: &CritterType,
        hex: i32,
        elevation: u8,
        player: i32,
    ) -> Result<ObjectRef, WorldError> {
        let prototype = self.assets.prototype(critter.pid)?;
        let object = self.spawn(prototype.clone(), hex, elevation)?;
        if !critter.is_critter() {
            return Ok(object);
        }
        let name = self.assets.prototype_text(&prototype, false);
        let description = self.assets.prototype_text(&prototype, true);
        if let Some(target) = self.world.get_mut(object) {
            if let Some(name) = name {
                target.name = name;
            }
            target.description = description.unwrap_or_default();
            if let Some(facing) = crate::routing::rotation(hex, player) {
                target.raw.orientation = facing;
            }
        }
        for item in &critter.items {
            let quantity = fallout2::random(&mut self.world.rng, item.min, item.max)?;
            if quantity <= 0 {
                continue;
            }
            let prototype = match self.assets.prototype(item.pid) {
                Ok(prototype) => prototype,
                Err(error) => {
                    self.world
                        .diagnostic(format!("encounter item {}: {error}", item.pid));
                    continue;
                }
            };
            let spawned = self.spawn(prototype, hex, elevation)?;
            if let Err(error) = self.world.add_item(object, spawned, quantity, true) {
                self.world.erase(spawned);
                self.world
                    .diagnostic(format!("encounter item {}: {error}", item.pid));
                continue;
            }
            if item.wielded
                && let Err(error) = self.wield_object(object, spawned)
            {
                self.world
                    .diagnostic(format!("encounter weapon {}: {error}", item.pid));
            }
        }
        let file = if critter.script >= 0 {
            Some(critter.script)
        } else {
            prototype
                .script
                .filter(|script| *script != -1)
                .map(|script| script & 0xffffff)
        };
        if let Some(file) = file {
            let locals = self.assets.local_count(file);
            match self.world.add_script(object, file, 4, locals) {
                Ok(sid) => {
                    if let Some(target) = self.world.get_mut(object) {
                        target.raw.script_id = sid as i32;
                    }
                }
                Err(error) => self.world.diagnostic(error.to_string()),
            }
        }
        if critter.dead {
            self.kill_critter(object, 20)?;
        }
        Ok(object)
    }
    /// SPEC-018 §5.8 / SPEC-019 §3.12.4 entering area `area`: the first
    /// entry loads its lowest-numbered known entrance directly.
    pub fn fallout2_enter_area(&mut self, area: usize) -> Result<AreaEntry, WorldError> {
        let entry = self.fallout2_state()?.enter_area(area);
        if let AreaEntry::Direct(slot) = entry {
            self.fallout2_enter_entrance(area, slot)?;
        }
        Ok(entry)
    }
    /// SPEC-018 §4.3 entering entrance `slot` of `area`: its map, at the
    /// entrance's hex, elevation and orientation, each -1 taking the
    /// header's (a hex of -1 takes the header's whole entrance). No
    /// campaign global is written.
    pub fn fallout2_enter_entrance(&mut self, area: usize, slot: usize) -> Result<(), WorldError> {
        let state = self.fallout2_state()?;
        let entrance = state
            .entrance_of(area, slot)
            .ok_or_else(|| WorldError::Invalid(format!("area {area} has no entrance {slot}")))?;
        state.area = area as i32;
        state.entrance = slot as i32;
        let path = self
            .fallout2_map_path(entrance.map as i32)
            .ok_or_else(|| WorldError::Invalid(format!("map {} is not available", entrance.map)))?;
        self.load_map(&path)?;
        if entrance.tile >= 0 {
            let header = &self.world.map.header;
            let elevation = if entrance.elevation >= 0 {
                entrance.elevation as u8
            } else {
                header.entry_elevation
            };
            let facing = if entrance.orientation >= 0 {
                entrance.orientation as u8
            } else {
                header.entry_orientation
            };
            self.place_player(entrance.tile, elevation, facing)?;
            self.world.place_party()?;
        }
        self.world_map_closed();
        Ok(())
    }
    /// SPEC-018 §4.9 the elevator of `metarule` rule 15's operand. A type
    /// without a record does nothing, with one diagnostic.
    pub fn fallout2_elevator(
        &mut self,
        kind: i32,
    ) -> Option<vault_formats::fallout2::elevators::Elevator> {
        let found = self
            .world
            .fallout2
            .as_ref()
            .and_then(|state| state.tables.elevators.get(kind).cloned());
        if found.is_none() {
            self.world.diagnostic(format!(
                "elevator type {kind} has no data/vault-engine/elevators.txt record"
            ));
        }
        found
    }
    /// SPEC-017 §8.7 rule 2: a request that would load a script, while
    /// gated, is handled as one whose script cannot be loaded, without a
    /// per-request diagnostic; the session's one diagnostic is recorded.
    pub(crate) fn note_gated_request(&mut self) {
        if let Some(state) = self.world.fallout2.as_deref_mut()
            && !std::mem::replace(&mut state.gate_noted, true)
        {
            self.world
                .diagnostic("Fallout 2 scripts are gated (SPEC-017 §8.5): no script runs");
        }
    }
}

/// The hexes at lattice distance `radius` from `center`, from the corner in
/// direction 0 and around through directions 2, 3, 4, 5, 0, 1 (SPEC-003's
/// neighbor order); off-grid hexes are skipped.
fn ring(center: i32, radius: i32) -> Vec<i32> {
    let mut out = Vec::new();
    let mut hex = Some(center);
    for _ in 0..radius {
        hex = hex.and_then(|hex| HexGrid::neighbor(hex, 0).ok().flatten());
    }
    let Some(mut hex) = hex else {
        return out;
    };
    for direction in [2u8, 3, 4, 5, 0, 1] {
        for _ in 0..radius {
            out.push(hex);
            match HexGrid::neighbor(hex, direction).ok().flatten() {
                Some(next) => hex = next,
                None => return out,
            }
        }
    }
    out
}

impl<R: RandomSource> std::fmt::Debug for Conditions<'_, R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Conditions")
            .field("level", &self.level)
            .field("ticks", &self.ticks)
            .finish_non_exhaustive()
    }
}
