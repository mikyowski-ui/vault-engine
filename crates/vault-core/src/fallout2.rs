//! SPEC-018 under the Fallout 2 profile: the world state its tables drive
//! (areas, entrances, the discovery grid, map flags, encounter counters,
//! the car and the party records it saves), the world-map rules of §5, the
//! new-game data of §6 and the script gate of SPEC-017 §8.5.
//!
//! The tables themselves are `vault_formats::fallout2`'s. The host wires
//! these rules in where a Fallout 2 game is running
//! ([`crate::WorldState::fallout2`] is set).
use crate::{RandomSource, WorldError, worldmap::Stepper};
use std::{collections::BTreeMap, sync::Arc};
pub use vault_formats::fallout2::Fallout2Tables;
use vault_formats::fallout2::{
    city::Entrance,
    save::{AreaSave, Fallout2Shape, MemberRecord, WorldMapSave2},
    worldmap::{ConditionContext, Fill, Subtile},
};

/// SPEC-017 §8.5: Fallout 2 script execution stays gated until a spec
/// revision records the corpus audit. This is the one switch; it is not a
/// setting, and no configuration key, flag or preset opens it.
pub const SCRIPT_GATE_OPEN: bool = false;
/// SPEC-017 §8.7 rule 6: the monitor's notice while scripts are gated.
pub const GATE_NOTICE: &str = "Scripts are off: the Fallout 2 script audit is not recorded yet.";
/// §6.5 the new-game map, loaded by this fixed name.
pub const START_MAP: &str = "maps/artemple.map";
/// §6.1 the reset tick: 07:21:00 of the first game day.
pub const START_TICKS: u64 = 264600;
/// §6.1 tick 0 is 00:00 on 25 July 2241: day 205 (from 0) of the fixed
/// non-leap calendar.
pub const CALENDAR_YEAR: u16 = 2241;
pub const CALENDAR_DAY: u64 = 205;
/// §5.2 the world-map track.
pub const WORLD_MAP_MUSIC: &str = "03wrldmp";
/// §6.4 the game movie table, by `play_gmovie` index, under `art/cuts/`.
pub const MOVIES: [&str; 17] = [
    "iplogo.mve",
    "intro.mve",
    "elder.mve",
    "vsuit.mve",
    "afailed.mve",
    "adestroy.mve",
    "car.mve",
    "cartucci.mve",
    "timeout.mve",
    "tanker.mve",
    "enclave.mve",
    "derrick.mve",
    "artimer1.mve",
    "artimer2.mve",
    "artimer3.mve",
    "artimer4.mve",
    "credits.mve",
];
/// §6.4: the hand-off plays `elder.mve` (index 2) with flag `0x04`.
pub const NEW_GAME_MOVIE: (&str, u32) = ("art/cuts/elder.mve", 0x04);
/// §6.4: startup plays `iplogo.mve` and then `intro.mve`.
pub const STARTUP_MOVIES: [&str; 2] = ["art/cuts/iplogo.mve", "art/cuts/intro.mve"];
/// SPEC-019 §2.6 the car's fuel maximum.
pub const FUEL_MAX: i32 = vault_formats::fallout2::save::FUEL_MAX;

/// SPEC-017 §2.1 the instruction-word set of a game profile's scripts.
pub fn word_set(game: crate::GameProfile) -> vault_vm::WordSet {
    match game {
        crate::GameProfile::Fallout1 => vault_vm::WordSet::FALLOUT1,
        crate::GameProfile::Fallout2 => vault_vm::WordSet::FALLOUT2,
    }
}

/// §6.1 month, day and year of a game tick.
pub fn game_date(ticks: u64) -> [u16; 3] {
    crate::host_save::calendar_date(ticks, CALENDAR_YEAR, CALENDAR_DAY)
}

impl crate::WorldState {
    /// The clock's month, day and year: SPEC-018 §6.1's calendar in a
    /// Fallout 2 game, SPEC-005 §3.5's otherwise.
    pub fn game_date(&self) -> [u16; 3] {
        if self.fallout2.is_some() {
            game_date(self.clock.ticks)
        } else {
            crate::host_save::game_date(self.clock.ticks)
        }
    }
}

/// `random(a, b)` of §3.2: no draw when a equals b.
pub fn random(rng: &mut impl RandomSource, first: i32, last: i32) -> Result<i32, WorldError> {
    if first == last {
        Ok(first)
    } else {
        rng.range(first, last)
    }
}

/// §5.6 step 1 the period of a game time: 0 morning (06:00-11:59), 1
/// afternoon (12:00-17:59), 2 night (18:00-05:59).
pub fn period(ticks: u64) -> usize {
    match (ticks % 864000) / 36000 {
        6..=11 => 0,
        12..=17 => 1,
        _ => 2,
    }
}

/// §5.8 one area's state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AreaState {
    /// 0 unknown, 1 known, 2 visited.
    pub known: u8,
    pub locked: bool,
    /// The current world position.
    pub position: [i32; 2],
    /// Per entrance slot.
    pub entrances: Vec<bool>,
}

/// SPEC-019 §2.4's four car values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Car {
    pub has_car: bool,
    pub area: i32,
    pub map: i32,
    pub fuel: i32,
}

impl Default for Car {
    fn default() -> Self {
        Self {
            has_car: false,
            area: -1,
            map: -1,
            fuel: 0,
        }
    }
}

/// Interactive travel in progress (not saved, as SPEC-008 §3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Travel {
    pub destination: [i32; 2],
    pub stepper: Stepper,
}

/// One movement-loop iteration's step result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Moving,
    Arrived,
    /// The walk mask or the world's edge stopped movement.
    Blocked,
}

/// §5.6 a selected encounter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selected {
    pub table: u32,
    pub entry: u32,
    pub map: u32,
    pub special: bool,
}

/// §5.8 entering an area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AreaEntry {
    /// First entry: load this entrance slot's map directly.
    Direct(usize),
    /// A later entry: show the town map.
    TownMap,
    /// Locked, unknown, absent, or (first entry) without a known entrance.
    Refused,
}

/// What a condition reads, from the host.
pub struct Conditions<'a, R: RandomSource> {
    pub globals: &'a [i32],
    pub level: i32,
    pub ticks: u64,
    pub rng: &'a mut R,
    /// The first draw failure, if any (the draw then reads 100).
    pub failure: Option<WorldError>,
}

impl<R: RandomSource> ConditionContext for Conditions<'_, R> {
    fn global(&self, index: i32) -> i32 {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.globals.get(index))
            .copied()
            .unwrap_or(0)
    }
    fn player_level(&self) -> i32 {
        self.level
    }
    fn ticks(&self) -> u64 {
        self.ticks
    }
    fn time_of_day(&self) -> i32 {
        let minutes = (self.ticks % 864000) / 600;
        (minutes / 60 * 100 + minutes % 60) as i32
    }
    fn percent_draw(&mut self) -> i32 {
        match self.rng.range(1, 100) {
            Ok(draw) => draw,
            Err(error) => {
                self.failure.get_or_insert(error);
                100
            }
        }
    }
}

/// The Fallout 2 game state SPEC-018 §5 and §7 define, with the tables it
/// was built from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fallout2World {
    pub tables: Arc<Fallout2Tables>,
    pub position: [i32; 2],
    /// The current area, or -1.
    pub area: i32,
    /// The current entrance slot, or -1.
    pub entrance: i32,
    pub car: Car,
    /// `R` rows of `C` bytes: 0 unknown, 1 seen, 2 visited.
    pub discovery: Vec<u8>,
    /// `A` areas (absent ones stay unknown).
    pub areas: Vec<AreaState>,
    /// `M_T` bytes: `0x01` known, `0x02` visited.
    pub map_flags: Vec<u8>,
    /// Remaining counts of the limited entries, by (table, entry).
    pub counters: BTreeMap<(u32, u32), i32>,
    /// One byte per [`MOVIES`] index.
    pub movies: [u8; 17],
    /// §7.3 the player's kill counters of kill types 15-18.
    pub kills: [i32; 4],
    /// SPEC-019 §2.5 control records of party members, by object id.
    pub control: BTreeMap<u32, MemberRecord>,
    /// SPEC-019 §3.14 the records kept for objects that left the party.
    pub retained: BTreeMap<u32, MemberRecord>,
    pub travel: Option<Travel>,
    /// §5.4 the terrain counter, 0 at each trip's start.
    pub terrain_counter: i32,
    /// Movement-loop iterations since the world map was built.
    pub iterations: u32,
    /// Iterations since the last encounter and healing check.
    pub interval: u32,
    pub speed: Option<crate::worldmap::TravelSpeed>,
    /// The pixel the last marking iteration marked.
    pub mark: Option<[i32; 2]>,
    /// The session's one gated-scripts diagnostic was recorded.
    pub gate_noted: bool,
    /// SPEC-017 §4.4 `set_map_music` overrides of the city track, by map
    /// number. Not saved: they last until the next full game reset.
    pub map_music: BTreeMap<i32, String>,
    /// SPEC-017 §5.2 `metarule3` 102's world-map track, when overridden;
    /// not saved either.
    pub world_map_music: Option<String>,
    /// The world-map screen is open (built and not yet closed).
    pub world_map_open: bool,
}

impl Fallout2World {
    /// §6.5 the world state at a new game, for start map number
    /// `start_map` (-1 when no record matches it).
    pub fn new_game(tables: Arc<Fallout2Tables>, start_map: i32) -> Self {
        let city = &tables.city;
        let [columns, rows] = tables.worldmap.grid();
        let mut areas = vec![AreaState::default(); city.count()];
        for area in city.areas() {
            areas[area.number as usize] = AreaState {
                known: u8::from(area.start_known),
                locked: area.start_locked,
                position: area.world_pos,
                entrances: area
                    .entrances
                    .iter()
                    .map(|entrance| entrance.is_some_and(|e| e.known))
                    .collect(),
            };
        }
        let start = Self::start_area(&tables, start_map);
        let mut world = Self {
            position: [0, 0],
            area: start,
            entrance: -1,
            car: Car::default(),
            discovery: vec![0; columns * rows],
            areas,
            map_flags: vec![0; tables.maps.count()],
            counters: Self::initial_counters(&tables),
            movies: [0; 17],
            kills: [0; 4],
            control: BTreeMap::new(),
            retained: BTreeMap::new(),
            travel: None,
            terrain_counter: 0,
            iterations: 0,
            interval: 0,
            speed: None,
            mark: None,
            gate_noted: false,
            map_music: BTreeMap::new(),
            world_map_music: None,
            world_map_open: false,
            tables,
        };
        let known: Vec<[i32; 2]> = world
            .areas
            .iter()
            .filter(|area| area.known > 0)
            .map(|area| area.position)
            .collect();
        for position in known {
            if let Some(cell) = world.tables.worldmap.cell_of(position) {
                *world.discovery_mut(cell) = 1;
            }
        }
        if let Ok(index) = usize::try_from(start)
            && let Some(area) = world.tables.city.get(start)
        {
            world.position = area.world_pos;
            world.areas[index].known = 2;
            world.entrance = area
                .entrances
                .iter()
                .position(|entrance| {
                    entrance.is_some_and(|e| i64::from(e.map) == i64::from(start_map))
                })
                .map_or(-1, |slot| slot as i32);
            if let Some(cell) = world.tables.worldmap.cell_of(area.world_pos) {
                *world.discovery_mut(cell) = 2;
            }
        }
        world
    }
    /// §6.5 the start area: the start map's area; else area 0; else the
    /// lowest valid area.
    pub fn start_area(tables: &Fallout2Tables, start_map: i32) -> i32 {
        let area = tables.city.area_of_map(start_map);
        if area >= 0 {
            return area;
        }
        if tables.city.get(0).is_some() {
            return 0;
        }
        tables
            .city
            .areas()
            .next()
            .map_or(-1, |area| area.number as i32)
    }
    fn initial_counters(tables: &Fallout2Tables) -> BTreeMap<(u32, u32), i32> {
        tables
            .worldmap
            .tables
            .values()
            .flat_map(|table| {
                table
                    .entries
                    .iter()
                    .filter(|entry| entry.counter != -1)
                    .map(|entry| ((table.number, entry.number), entry.counter))
            })
            .collect()
    }
    /// `[C, R]`.
    pub fn grid(&self) -> [usize; 2] {
        self.tables.worldmap.grid()
    }
    pub fn world_size(&self) -> [i32; 2] {
        self.tables.worldmap.world_size()
    }
    /// The party's grid cell.
    pub fn cell(&self) -> Option<[usize; 2]> {
        self.tables.worldmap.cell_of(self.position)
    }
    pub fn subtile(&self, cell: [usize; 2]) -> Subtile {
        self.tables.worldmap.subtile(cell)
    }
    pub fn discovery(&self, [c, r]: [usize; 2]) -> u8 {
        self.discovery[r * self.grid()[0] + c]
    }
    fn discovery_mut(&mut self, [c, r]: [usize; 2]) -> &mut u8 {
        let columns = self.grid()[0];
        &mut self.discovery[r * columns + c]
    }
    /// §5.5 the reveal: the current cell becomes 2 and its 3 by 3 (5 by 5
    /// with Scout) neighborhood 1 unless already 2, clipped to the grid;
    /// then the cell's fill sets every cell from it to the grid edge in each
    /// named direction to 2.
    pub fn reveal(&mut self, scout: bool) {
        let Some([column, row]) = self.cell() else {
            return;
        };
        let [columns, rows] = self.grid();
        let radius = if scout { 2 } else { 1 };
        for y in row.saturating_sub(radius)..=(row + radius).min(rows - 1) {
            for x in column.saturating_sub(radius)..=(column + radius).min(columns - 1) {
                let value = self.discovery_mut([x, y]);
                if *value != 2 {
                    *value = 1;
                }
            }
        }
        *self.discovery_mut([column, row]) = 2;
        let fill = self.subtile([column, row]).fill;
        if fill != Fill::None {
            for [dx, dy] in fill.directions() {
                let (mut x, mut y) = (column as i64 + i64::from(*dx), row as i64 + i64::from(*dy));
                while (0..columns as i64).contains(&x) && (0..rows as i64).contains(&y) {
                    *self.discovery_mut([x as usize, y as usize]) = 2;
                    x += i64::from(*dx);
                    y += i64::from(*dy);
                }
            }
        }
    }
    /// SPEC-019 §3.12.2 the first valid area, in table order, whose circle
    /// (at its current position) contains `position`, known or not.
    pub fn area_at(&self, position: [i32; 2]) -> Option<usize> {
        self.tables.city.areas().find_map(|area| {
            let state = self.areas.get(area.number as usize)?;
            let [dx, dy] = [
                i64::from(position[0] - state.position[0]),
                i64::from(position[1] - state.position[1]),
            ];
            let radius = i64::from(area.size.radius());
            (dx * dx + dy * dy <= radius * radius).then_some(area.number as usize)
        })
    }
    /// Begin travel toward `destination`; the terrain counter starts at 0.
    pub fn travel_to(&mut self, destination: [i32; 2]) {
        self.travel = Some(Travel {
            destination,
            stepper: Stepper::new(self.position, destination),
        });
        self.terrain_counter = 0;
    }
    /// §5.4 one iteration's terrain schedule: the counter drops by 1, and
    /// at 0 or less one pixel step is attempted and the counter becomes the
    /// difficulty of the terrain the iteration started on. A step out of the
    /// world, or onto a set walk-mask bit, restores the prior position and
    /// stops movement. `Q` is charged by the caller on every iteration.
    pub fn step(&mut self) -> Step {
        let Some(mut travel) = self.travel else {
            return Step::Arrived;
        };
        let terrain = self.cell().map_or(0, |cell| self.subtile(cell).terrain);
        let difficulty = self.tables.worldmap.difficulty(terrain);
        self.terrain_counter -= 1;
        let mut outcome = Step::Moving;
        if self.terrain_counter <= 0 {
            self.terrain_counter = difficulty;
            match travel.stepper.step(self.position) {
                None => outcome = Step::Arrived,
                Some(next) if self.blocks(next) => outcome = Step::Blocked,
                Some(next) => self.position = next,
            }
        }
        self.travel = (outcome == Step::Moving).then_some(travel);
        outcome
    }
    /// §5.4 whether a step to `next` is blocked: out of the world, or a set
    /// bit in its tile's walk mask.
    pub fn blocks(&self, next: [i32; 2]) -> bool {
        let Some(cell) = self.tables.worldmap.cell_of(next) else {
            return true;
        };
        let tile = self.tables.worldmap.tile_of(cell);
        self.tables.mask(tile).is_some_and(|mask| {
            mask.blocks([
                next[0].rem_euclid(vault_formats::fallout2::worldmap::TILE_SIZE[0]),
                next[1].rem_euclid(vault_formats::fallout2::worldmap::TILE_SIZE[1]),
            ])
        })
    }
    /// A limited entry's remaining count; -1 for an unlimited one.
    pub fn counter(&self, table: u32, entry: u32) -> i32 {
        self.counters.get(&(table, entry)).copied().unwrap_or(-1)
    }
    /// §5.6 the encounter check at the party's cell. `luck` is the player's
    /// effective Luck and `explorer` the Explorer ranks. Returns the
    /// selected encounter, or `None`; diagnostics go to `notes`.
    pub fn check_encounter<R: RandomSource>(
        &mut self,
        conditions: &mut Conditions<'_, R>,
        luck: i32,
        explorer: u32,
        notes: &mut Vec<String>,
    ) -> Result<Option<Selected>, WorldError> {
        let Some(cell) = self.cell() else {
            return Ok(None);
        };
        let subtile = self.subtile(cell);
        let chance = subtile.frequency[period(conditions.ticks)];
        let draw = conditions.rng.range(1, 100)?;
        let Some(table_number) = subtile.table else {
            return Ok(None);
        };
        if draw > chance || self.area_at(self.position).is_some() {
            return Ok(None);
        }
        let tables = self.tables.clone();
        let Some(table) = tables.worldmap.tables.get(&table_number) else {
            return Ok(None);
        };
        let mut eligible = Vec::new();
        for entry in &table.entries {
            let holds = entry.condition.holds(conditions);
            if let Some(error) = conditions.failure.take() {
                return Err(error);
            }
            if holds && self.counter(table.number, entry.number) != 0 {
                eligible.push(entry);
            }
        }
        if eligible.iter().any(|entry| entry.special) {
            let sum = conditions.rng.range(1, 6)?
                + conditions.rng.range(1, 6)?
                + conditions.rng.range(1, 6)?;
            let gate = sum - 5 + luck + 2 * explorer.min(1 << 20) as i32;
            if gate < 18 {
                eligible.retain(|entry| !entry.special);
            }
        }
        let total: i32 = eligible.iter().map(|entry| entry.chance).sum();
        if total == 0 {
            return Ok(None);
        }
        let draw = random(conditions.rng, 1, total)?;
        let mut running = 0;
        let Some(entry) = eligible.into_iter().find(|entry| {
            running += entry.chance;
            running >= draw
        }) else {
            return Ok(None);
        };
        if let Some(count) = self.counters.get_mut(&(table.number, entry.number))
            && *count > 0
        {
            *count -= 1;
        }
        let map = match entry.map {
            Some(map) => Some(map),
            None if !table.maps.is_empty() => {
                Some(table.maps[random(conditions.rng, 1, table.maps.len() as i32)? as usize - 1])
            }
            None => {
                let list = tables
                    .worldmap
                    .random_maps
                    .get(subtile.terrain)
                    .cloned()
                    .unwrap_or_default();
                if list.is_empty() {
                    None
                } else {
                    Some(list[random(conditions.rng, 1, list.len() as i32)? as usize - 1])
                }
            }
        };
        let Some(map) = map else {
            notes.push(format!(
                "encounter table {} entry {}: no map to load",
                table.number, entry.number
            ));
            return Ok(None);
        };
        Ok(Some(Selected {
            table: table.number,
            entry: entry.number,
            map,
            special: entry.special,
        }))
    }
    /// §5.7 "Dropping to terrain": one map of the party cell's terrain's
    /// random map list, or none (the world map stays open).
    pub fn terrain_map(&self, rng: &mut impl RandomSource) -> Result<Option<u32>, WorldError> {
        let terrain = self.cell().map_or(0, |cell| self.subtile(cell).terrain);
        let list = self
            .tables
            .worldmap
            .random_maps
            .get(terrain)
            .cloned()
            .unwrap_or_default();
        if list.is_empty() {
            return Ok(None);
        }
        Ok(Some(list[random(rng, 1, list.len() as i32)? as usize - 1]))
    }
    /// §5.7 step 1 an encounter map's entrance: one of its start points,
    /// drawn with `random(0, k - 1)`, as (hex, elevation); `None` for the
    /// header's default entrance.
    pub fn encounter_entrance(
        &self,
        map: u32,
        rng: &mut impl RandomSource,
    ) -> Result<Option<(i32, u8)>, WorldError> {
        let Some(record) = self.tables.maps.get(map as i32) else {
            return Ok(None);
        };
        let points = &record.start_points;
        if points.is_empty() {
            return Ok(None);
        }
        let point = points[random(rng, 0, points.len() as i32 - 1)? as usize];
        Ok(Some((point.tile, point.elevation)))
    }
    /// §5.8 entering area `area`: a known, unlocked area; on first entry
    /// (state 1) the state becomes 2 and its lowest-numbered known
    /// entrance is loaded directly; later entries show the town map.
    pub fn enter_area(&mut self, area: usize) -> AreaEntry {
        let Some(state) = self.areas.get_mut(area) else {
            return AreaEntry::Refused;
        };
        if state.known == 0 || state.locked || self.tables.city.get(area as i32).is_none() {
            return AreaEntry::Refused;
        }
        self.area = area as i32;
        if state.known == 1 {
            let Some(slot) = state.entrances.iter().position(|known| *known) else {
                return AreaEntry::Refused;
            };
            state.known = 2;
            self.entrance = slot as i32;
            return AreaEntry::Direct(slot);
        }
        AreaEntry::TownMap
    }
    /// The entrance record of `area`'s slot `slot`.
    pub fn entrance_of(&self, area: usize, slot: usize) -> Option<Entrance> {
        self.tables
            .city
            .get(area as i32)?
            .entrances
            .get(slot)
            .copied()
            .flatten()
    }
    /// §5.2 after a successful map load: the map's known and visited flags
    /// are set, and (§5.4) a map in an area moves the party to that area's
    /// current position at once.
    pub fn map_loaded(&mut self, map: i32) {
        if let Some(flags) = usize::try_from(map)
            .ok()
            .and_then(|map| self.map_flags.get_mut(map))
        {
            *flags |= 0x03;
        }
        let area = self.tables.city.area_of_map(map);
        if let Some(state) = usize::try_from(area)
            .ok()
            .and_then(|area| self.areas.get(area))
        {
            self.position = state.position;
        }
    }
    /// §5.2 a map's location name: its area's display name; for area -1,
    /// the map's lookup name; without either, empty.
    pub fn location_name(&self, map: i32) -> String {
        let area = self.tables.city.area_of_map(map);
        if let Some(area) = self.tables.city.get(area) {
            return area.name.clone();
        }
        self.tables
            .maps
            .get(map)
            .and_then(|record| record.lookup_name.clone())
            .unwrap_or_default()
    }
    /// §5.2 rest permission's first rule: `can_rest_here` for the
    /// elevation (`true` on an unnumbered map).
    pub fn can_rest(&self, map: i32, elevation: u8) -> bool {
        self.tables
            .maps
            .get(map)
            .is_none_or(|record| record.can_rest[usize::from(elevation.min(2))])
    }
    /// The installed tables' shape, which a save must match (§7.4, §7.5).
    pub fn shape(tables: &Fallout2Tables) -> Fallout2Shape {
        let city = &tables.city;
        Fallout2Shape {
            grid: tables.worldmap.grid(),
            entrances: (0..city.count())
                .map(|area| city.entrance_count(area))
                .collect(),
            areas_present: (0..city.count())
                .map(|area| city.get(area as i32).is_some())
                .collect(),
            maps: tables.maps.count(),
            maps_present: (0..tables.maps.count())
                .map(|map| tables.maps.get(map as i32).is_some())
                .collect(),
            party_levels: (0..tables.party.count())
                .map(|record| {
                    tables
                        .party
                        .get(record as i32)
                        .map(|member| member.level_pids.len())
                })
                .collect(),
        }
    }
    /// §7.4 the saved component. Absent areas, slots and maps are zero.
    pub fn save(&self) -> WorldMapSave2 {
        let [columns, rows] = self.grid();
        let mut counters: Vec<[i32; 3]> = self
            .counters
            .iter()
            .map(|((table, entry), remaining)| [*table as i32, *entry as i32, *remaining])
            .collect();
        counters.sort();
        WorldMapSave2 {
            position: self.position,
            area: self.area,
            entrance: self.entrance,
            has_car: i32::from(self.car.has_car),
            car_area: self.car.area,
            car_map: self.car.map,
            car_fuel: self.car.fuel,
            grid: [columns as u16, rows as u16],
            discovery: self.discovery.clone(),
            areas: self
                .areas
                .iter()
                .enumerate()
                .map(|(number, area)| {
                    let present = self.tables.city.get(number as i32).is_some();
                    AreaSave {
                        known: if present { area.known } else { 0 },
                        locked: u8::from(present && area.locked),
                        position: if present { area.position } else { [0, 0] },
                        entrances: (0..self.tables.city.entrance_count(number))
                            .map(|slot| {
                                u8::from(
                                    self.entrance_of(number, slot).is_some()
                                        && area.entrances.get(slot).copied().unwrap_or(false),
                                )
                            })
                            .collect(),
                    }
                })
                .collect(),
            map_flags: (0..self.tables.maps.count())
                .map(|map| {
                    if self.tables.maps.get(map as i32).is_some() {
                        self.map_flags[map]
                    } else {
                        0
                    }
                })
                .collect(),
            counters,
        }
    }
    /// §7.4 reading: the component was already checked against the shape;
    /// out-of-world positions are repaired with diagnostics, and counter
    /// records of no installed limited entry are ignored.
    pub fn restore(
        tables: Arc<Fallout2Tables>,
        saved: &WorldMapSave2,
        movies: [u8; 17],
        notes: &mut Vec<String>,
    ) -> Self {
        let mut world = Self::new_game(tables, -1);
        let [width, height] = world.world_size();
        let inside = |[x, y]: [i32; 2]| (0..width).contains(&x) && (0..height).contains(&y);
        world.position = saved.position;
        if !inside(world.position) {
            notes.push(format!(
                "world position ({}, {}) outside the world; clamped",
                saved.position[0], saved.position[1]
            ));
            world.position = [
                saved.position[0].clamp(0, width - 1),
                saved.position[1].clamp(0, height - 1),
            ];
        }
        world.area = saved.area;
        world.entrance = saved.entrance;
        world.car = Car {
            has_car: saved.has_car == 1,
            area: saved.car_area,
            map: saved.car_map,
            fuel: saved.car_fuel,
        };
        world.discovery = saved.discovery.clone();
        for (number, (state, area)) in world.areas.iter_mut().zip(&saved.areas).enumerate() {
            state.known = area.known;
            state.locked = area.locked != 0;
            state.entrances = area.entrances.iter().map(|known| *known != 0).collect();
            let default = world
                .tables
                .city
                .get(number as i32)
                .map_or([0, 0], |area| area.world_pos);
            state.position = if inside(area.position) {
                area.position
            } else {
                notes.push(format!("area {number}'s position outside the world; reset"));
                default
            };
        }
        world.map_flags = saved.map_flags.clone();
        for [table, entry, remaining] in &saved.counters {
            let key = (*table as u32, *entry as u32);
            match world.counters.get_mut(&key) {
                Some(count) if *table >= 0 && *entry >= 0 => *count = *remaining,
                _ => notes.push(format!(
                    "counter record ({table}, {entry}) names no limited entry; ignored"
                )),
            }
        }
        world.movies = movies;
        world
    }
    /// §7.5 a member record read back: values outside a vocabulary become
    /// the field's neutral value, with a diagnostic.
    pub fn sanitize(record: MemberRecord, notes: &mut Vec<String>) -> MemberRecord {
        use vault_formats::fallout2::party::ControlField;
        if record.record == -1 {
            return MemberRecord::NONE;
        }
        let mut out = record;
        for (index, field) in ControlField::ALL.into_iter().enumerate() {
            if !field.valid(out.fields[index]) {
                notes.push(format!(
                    "party record {}: {} {} is not an option; neutral",
                    record.record,
                    field.key(),
                    out.fields[index]
                ));
                out.fields[index] = field.neutral();
            }
        }
        out
    }
    /// SPEC-019 §2.5 a control record for a member of `pid`: the neutral
    /// record at level stage 0, or none without a `party.txt` record.
    pub fn joining_record(&self, pid: u32) -> MemberRecord {
        use vault_formats::fallout2::party::ControlField;
        match self.tables.party.record_of(pid) {
            Some(member) => MemberRecord {
                record: member.number as i32,
                stage: 0,
                fields: ControlField::ALL.map(ControlField::neutral),
            },
            None => MemberRecord::NONE,
        }
    }
    /// §5.9 the quests to list: groups by location in order of first
    /// appearance, each with its visible quests and their status.
    pub fn quest_list(
        &self,
        globals: &[i32],
    ) -> Vec<(
        i32,
        Vec<(usize, vault_formats::fallout2::lines::QuestStatus)>,
    )> {
        use vault_formats::fallout2::lines::{QuestStatus, quest_groups};
        let value = |global: i32| {
            usize::try_from(global)
                .ok()
                .and_then(|g| globals.get(g))
                .copied()
                .unwrap_or(0)
        };
        quest_groups(&self.tables.quests)
            .into_iter()
            .filter_map(|(location, ordinals)| {
                let visible: Vec<_> = ordinals
                    .into_iter()
                    .map(|ordinal| {
                        let quest = &self.tables.quests[ordinal];
                        (ordinal, quest.status(value(quest.global)))
                    })
                    .filter(|(_, status)| *status != QuestStatus::Hidden)
                    .collect();
                (!visible.is_empty()).then_some((location, visible))
            })
            .collect()
    }
    /// §5.9 the acquired holodisks' ordinals: those whose global is
    /// positive.
    pub fn holodisk_list(&self, globals: &[i32]) -> Vec<usize> {
        self.tables
            .holodisks
            .iter()
            .enumerate()
            .filter(|(_, disk)| {
                usize::try_from(disk.global)
                    .ok()
                    .and_then(|g| globals.get(g))
                    .is_some_and(|value| *value > 0)
            })
            .map(|(ordinal, _)| ordinal)
            .collect()
    }
}
