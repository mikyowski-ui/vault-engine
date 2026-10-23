//! SPEC-008 §2.7 and §3.4-§3.7 world travel: pixel coordinates over the
//! fixed 28 x 30 cell grids, the asymmetric stepper, binary32 travel speed,
//! terrain schedules, discovery, encounter dice, and town knowledge.
use crate::{RandomSource, WorldError};
use std::sync::Arc;
use vault_formats::{MsgCatalog, savegame::WorldMapSave};

/// One row of the fixed Section 3.7 town table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Town {
    pub name: &'static str,
    /// `[column, row]` of the town's 50-pixel cell.
    pub cell: [usize; 2],
    pub known_global: usize,
    pub entrance_globals: &'static [usize],
}

/// One compiled town-map hotspot; missing slots are all zero/empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotspot {
    pub x: i32,
    pub y: i32,
    pub entry: i32,
    pub map: &'static str,
}
impl Hotspot {
    pub const EMPTY: Self = Self {
        x: 0,
        y: 0,
        entry: 0,
        map: "",
    };
}
/// One row of the fixed SPEC-008 §3.10 map table: the `map.msg` text, the
/// town and the city music track of a map number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapEntry {
    pub name: &'static str,
    pub town: usize,
    pub music: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/world_tables.rs"));

/// SPEC-008 §3.10: there are exactly 66 map numbers, `map.msg` 0-65.
pub const MAP_COUNT: usize = 66;
/// SPEC-008 §3.10 the track the world map plays.
pub const WORLD_MAP_MUSIC: &str = "03wrldmp";
/// SPEC-008 §3.10 the looping background track a map load starts first.
pub const LOADING_MUSIC: &str = "wind2";

pub const WORLD_SIZE: [i32; 2] = [1400, 1500];
pub const CELL: i32 = 50;
pub const COLUMNS: usize = 28;
pub const ROWS: usize = 30;
/// The saved discovery array's larger stride and row count.
pub const STRIDE: usize = 29;
pub const SAVED_ROWS: usize = 31;
pub const KNOWLEDGE_ROWS: usize = 15;
pub const ENTRANCE_SLOTS: usize = 7;
/// The center of Vault 13's cell (16,1).
pub const START: [i32; 2] = [825, 75];
pub const VIEWPORT_ORIGIN: [i32; 2] = [22, 21];
pub const VIEWPORT_SIZE: [i32; 2] = [450, 442];
pub const SCROLL_LIMIT: [i32; 2] = [950, 1058];
/// West/ocean reveal thresholds for rows 0-29.
pub const WEST_REVEAL: [usize; ROWS] = [
    0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 2, 3, 4, 5, 5, 5, 5, 8, 11, 12, 14, 14, 17, 18, 19, 19, 20, 20,
    20, 21,
];
/// A 3d6 sum strictly below this selects an encounter, by frequency class.
pub const ENCOUNTER_BELOW: [i32; 4] = [6, 7, 9, 10];
/// Inclusive 1-100 upper bounds, maps, and consumed bits.
pub const SPECIAL_ENCOUNTERS: [(i32, &str, u32); 6] = [
    (30, "FOOT.MAP", 0x01),
    (50, "TALKCOW.MAP", 0x02),
    (70, "USEDCAR.MAP", 0x04),
    (80, "TARDIS.MAP", 0x08),
    (90, "FSAUSER.MAP", 0x10),
    (100, "COLATRUK.MAP", 0x20),
];
pub const ALL_SPECIALS: u32 = 0x3f;
/// Desert, mountain, city, coast choices; empty strings are unpopulated.
pub const TERRAIN_MAPS: [[&str; 3]; 4] = [
    ["DESERT1.MAP", "DESERT2.MAP", "DESERT3.MAP"],
    ["MOUNTN1.MAP", "MOUNTN2.MAP", ""],
    ["CITY1.MAP", "", ""],
    ["COAST1.MAP", "COAST2.MAP", ""],
];
/// Campaign globals written before an encounter/town/dropout map load.
pub const REGION_GLOBAL: usize = 65;
pub const ENTRY_GLOBAL: usize = 32;
pub const BASE_DESTROYED_GLOBAL: usize = 17;
pub const CATHEDRAL_DESTROYED_GLOBAL: usize = 18;
pub const MILITARY_BASE: usize = 8;
pub const CATHEDRAL: usize = 11;
pub const BROTHERHOOD: usize = 7;
/// Vault 13 exposes all four entrances at/after this game tick.
pub const VAULT13_OPEN_TICK: u64 = 864000;
/// Town-map presentation: picture art `156 + town`, buttons from event 514.
pub const TOWN_PICTURE_BASE: i32 = 156;
pub const TOWN_BUTTON_EVENT: i32 = 514;
pub const TOWN_BUTTON_SIZE: [i32; 2] = [164, 25];
/// Safe bound on the reference's unbounded redraw loops.
const DRAW_LIMIT: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terrain {
    Desert,
    Mountain,
    City,
    Coast,
}

/// The playable 50-pixel cell `[column, row]` holding a world pixel.
pub fn cell_of(position: [i32; 2]) -> Option<[usize; 2]> {
    let [column, row] = position.map(|value| value.div_euclid(CELL));
    ((0..COLUMNS as i32).contains(&column) && (0..ROWS as i32).contains(&row))
        .then_some([column as usize, row as usize])
}
pub fn terrain(cell: [usize; 2]) -> Terrain {
    match TERRAIN[cell[1]][cell[0]] {
        0 => Terrain::Desert,
        1 => Terrain::Mountain,
        2 => Terrain::City,
        _ => Terrain::Coast,
    }
}
pub fn frequency(cell: [usize; 2]) -> u8 {
    FREQUENCY[cell[1]][cell[0]]
}
pub fn region(cell: [usize; 2]) -> u8 {
    REGION[cell[1]][cell[0]]
}
/// Town detection compares the cell with the fixed town locations.
pub fn town_at(cell: [usize; 2]) -> Option<usize> {
    TOWNS.iter().position(|town| town.cell == cell)
}
pub fn clamp_scroll(origin: [i32; 2]) -> [i32; 2] {
    [
        origin[0].clamp(0, SCROLL_LIMIT[0]),
        origin[1].clamp(0, SCROLL_LIMIT[1]),
    ]
}
/// A town destination near the cell center with 0-16 pixels of jitter per
/// axis: four draws, X magnitude 0-16, X sign 0-1, Y magnitude 0-16, Y sign
/// 0-1, where sign 1 adds and 0 subtracts (SPEC-008 §3.7).
pub fn town_target(town: usize, rng: &mut impl RandomSource) -> Result<[i32; 2], WorldError> {
    let town = TOWNS
        .get(town)
        .ok_or_else(|| WorldError::Invalid(format!("town {town} outside 0-11")))?;
    let [column, row] = town.cell.map(|value| value as i32);
    let mut jitter = || -> Result<i32, WorldError> {
        let magnitude = rng.range(0, 16)?;
        Ok(if rng.range(0, 1)? == 1 {
            magnitude
        } else {
            -magnitude
        })
    };
    let x = CELL * column + 25 + jitter()?;
    let y = CELL * row + 25 + jitter()?;
    Ok([x, y])
}
/// A knowledge row's hotspots in slot order, stopping at the first zero X
/// or Y.
pub fn hotspots(row: usize) -> impl Iterator<Item = (usize, &'static Hotspot)> {
    HOTSPOTS
        .get(row)
        .into_iter()
        .flatten()
        .enumerate()
        .take_while(|(_, hotspot)| hotspot.x != 0 && hotspot.y != 0)
}

/// The center of a town's cell.
pub fn town_center(town: usize) -> Option<[i32; 2]> {
    let [column, row] = TOWNS.get(town)?.cell;
    Some([column as i32 * CELL + 25, row as i32 * CELL + 25])
}
/// SPEC-008 §3.10 the fixed table row of a map number; none for an
/// out-of-range number, including -1.
pub fn map_entry(number: i32) -> Option<&'static MapEntry> {
    usize::try_from(number)
        .ok()
        .and_then(|number| MAP_TABLE.get(number))
}
/// SPEC-008 §3.10 town lookup: every map number has one fixed town id, a
/// plain table value even for the encounter and caravan maps; an
/// out-of-range number, including -1, gives town -1.
pub fn map_town(number: i32) -> i32 {
    map_entry(number).map_or(-1, |entry| entry.town as i32)
}
/// SPEC-008 §3.10 city music: the track `sound/music/<name>.acm` that
/// replaces `wind2` once a map's number is known; none for -1, where
/// `wind2` keeps looping.
pub fn city_music(number: i32) -> Option<&'static str> {
    map_entry(number).map(|entry| entry.music)
}

/// SPEC-008 §3.10 the music on/off state, for the whole run (no key,
/// preference control or opcode toggles it, and no save stores it).
/// `configured` is the `fallout.cfg` `[sound] music` key (SPEC-014 §2.3,
/// default 1): with 0, music stays off whatever the volumes do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MusicSwitch {
    configured: bool,
    on: bool,
}
impl Default for MusicSwitch {
    /// The default configuration (`music=1`) at nonzero volumes: on.
    fn default() -> Self {
        Self {
            configured: true,
            on: true,
        }
    }
}
impl MusicSwitch {
    /// Rule 1: music starts off and a nonzero `music` key turns it on; the
    /// startup volumes then apply as any other setting does (rule 2).
    pub fn startup(configured: bool, master_volume: i32, music_volume: i32) -> Self {
        let mut switch = Self {
            configured,
            on: configured,
        };
        switch.set_volumes(master_volume, music_volume);
        switch
    }
    pub fn on(self) -> bool {
        self.on
    }
    pub fn configured(self) -> bool {
        self.configured
    }
    /// Master and music volume were set: by the preferences sliders, a
    /// save's preference words, the startup configuration, or a game reset.
    /// Returns the new state when it changed.
    ///
    /// - Rule 2: music that is on turns off by volume when either is 0.
    /// - Rule 3: music off by volume turns back on when music volume is set
    ///   again, when master volume is set nonzero while music volume is
    ///   nonzero, or at a game reset, and turns off again at once when
    ///   either is still 0. So after any setting it is on exactly when the
    ///   key allows it and both volumes are nonzero; an on-then-off-at-once
    ///   pair leaves nothing to hear and reports no change.
    /// - Rule 4: dialogue's halved music volume is just a setting here, so
    ///   halving a volume of 1 to 0 turns music off by volume.
    pub fn set_volumes(&mut self, master_volume: i32, music_volume: i32) -> Option<bool> {
        let on = self.configured && master_volume > 0 && music_volume > 0;
        (on != self.on).then(|| {
            self.on = on;
            on
        })
    }
}
/// SPEC-008 §3.10 the `map.msg` text of a map number in the fixed table
/// (the corpus `map.msg` lists the same names in the same order).
pub fn map_table_name(number: i32) -> Option<&'static str> {
    map_entry(number).map(|entry| entry.name)
}
/// SPEC-008 §3.10 the form a name is matched in: cut at the first NUL,
/// uppercased, with its first `.SAV` replaced by `.MAP`.
pub fn canonical_map_name(name: &[u8]) -> Vec<u8> {
    let end = name.iter().position(|b| *b == 0).unwrap_or(name.len());
    let mut upper = name[..end].to_ascii_uppercase();
    if let Some(at) = upper.windows(4).position(|window| window == b".SAV") {
        upper[at..at + 4].copy_from_slice(b".MAP");
    }
    upper
}
/// SPEC-008 §3.10 name matching, shared by the header rule and `load_map`
/// (SPEC-005 0x80E4): the canonical name is compared with the texts of
/// `map.msg` messages 0-65 in order; the first exact match gives the
/// number, and no match (or no `map.msg`) gives -1. Nothing else is
/// normalized: no extension is added and no directory is stripped.
pub fn map_number_of_name(name: &[u8], catalog: Option<&MsgCatalog>) -> i32 {
    let Some(catalog) = catalog else {
        return -1;
    };
    let wanted = canonical_map_name(name);
    (0..MAP_COUNT as i32)
        .find(|number| {
            catalog
                .get(*number)
                .is_some_and(|message| message.text == wanted)
        })
        .unwrap_or(-1)
}
/// SPEC-008 §3.10 the `load_map` string rule: [`map_number_of_name`], except
/// that under the `[May-improve]` rule a string of 16 or more bytes (which
/// overruns the reference's 16-byte work buffer) matches nothing.
pub fn load_map_number(name: &[u8], catalog: Option<&MsgCatalog>) -> i32 {
    let end = name.iter().position(|b| *b == 0).unwrap_or(name.len());
    if end >= 16 {
        return -1;
    }
    map_number_of_name(name, catalog)
}
/// SPEC-008 §3.10 the current map number assigned after a map stream has
/// loaded, from the header's name field rather than the requested file:
/// a name containing the lowercase text `.sav`, or exactly `TMP$MAP#.MAP`,
/// keeps the header's map-number word; any other name is matched against
/// `map.msg`, so a renamed copy of a map takes the number of its name.
pub fn header_map_number(name: &[u8], stored: i32, catalog: Option<&MsgCatalog>) -> i32 {
    let end = name.iter().position(|b| *b == 0).unwrap_or(name.len());
    let name = &name[..end];
    if name.windows(4).any(|window| window == b".sav") || name == b"TMP$MAP#.MAP" {
        return stored;
    }
    map_number_of_name(name, catalog)
}

/// SPEC-008 §3.4 movement: absolute differences, signs, a zero error
/// accumulator, and a zero step count. The X-major branch's strict
/// `error > A` comparison can finish one Y pixel short of the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stepper {
    a: i32,
    b: i32,
    sign: [i32; 2],
    error: i32,
    count: i32,
}
impl Stepper {
    pub fn new(from: [i32; 2], to: [i32; 2]) -> Self {
        let delta = [to[0] - from[0], to[1] - from[1]];
        Self {
            a: delta[0].abs(),
            b: delta[1].abs(),
            sign: delta.map(i32::signum),
            error: 0,
            count: 0,
        }
    }
    /// One actual movement step from `position`; `None` completes the path.
    pub fn step(&mut self, position: [i32; 2]) -> Option<[i32; 2]> {
        self.count += 1;
        let [mut x, mut y] = position;
        if self.a <= self.b {
            if self.count > self.b {
                return None;
            }
            self.error += self.a;
            if self.error > 0 {
                self.error -= self.b;
                x += self.sign[0];
            }
            y += self.sign[1];
        } else {
            if self.count > self.a {
                return None;
            }
            self.error += self.b;
            if self.error > self.a {
                self.error -= self.a;
                y += self.sign[1];
            }
            x += self.sign[0];
        }
        Some([x, y])
    }
}

/// W movement-loop iterations between encounter/healing checks and Q game
/// ticks per time-charging iteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TravelSpeed {
    pub interval: u32,
    pub ticks: u32,
}
/// Travel speed from displayed Outdoorsman (capped above at 100) and
/// Pathfinder ranks: W and the division in binary32, the Pathfinder factor in
/// binary64. Under the SPEC-008 §3.4 safe rule Outdoorsman is clamped below
/// at 0 and Pathfinder to 0-4, so W is at least 60 and Q at least 0.
pub fn travel_speed(outdoorsman: i32, pathfinder: u32) -> Result<TravelSpeed, WorldError> {
    let skill = outdoorsman.clamp(0, 100) as f32;
    let ranks = pathfinder.min(4);
    let interval = (60.0_f32 + 60.0_f32 * skill / 100.0_f32).trunc();
    let ticks = (864000.0_f32 / interval).trunc();
    let ticks = (f64::from(ticks) * (1.0 - 0.25 * f64::from(ranks))).trunc();
    Ok(TravelSpeed {
        interval: interval as u32,
        ticks: ticks as u32,
    })
}

/// The terrain counter shared across terrain transitions. A desert/coast
/// iteration clears it; mountain and city schedules count it down and step
/// when it is exhausted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Schedule {
    pub counter: i32,
}
impl Schedule {
    /// One iteration's terrain schedule. `step` attempts one pixel step and
    /// reports whether movement remains active. Returns whether interactive
    /// travel charges Q this iteration.
    pub fn iterate(&mut self, terrain: Terrain, mut step: impl FnMut() -> bool) -> bool {
        match terrain {
            Terrain::Desert | Terrain::Coast => {
                step();
                self.counter = 0;
                true
            }
            Terrain::Mountain => {
                self.counter -= 1;
                if self.counter <= 0 {
                    step();
                    self.counter = 2;
                }
                true
            }
            Terrain::City => {
                let active = step();
                self.counter -= 1;
                if self.counter <= 0 && active {
                    step();
                    self.counter = 4;
                    false
                } else {
                    true
                }
            }
        }
    }
}

/// The fixed 1500-row, 133-byte-per-row travel walking mask (not the fog
/// of war). Its contents are not part of the approved specification, so a
/// host supplies it; without one nothing blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkMask(Vec<u8>);
impl WalkMask {
    pub const ROWS: usize = 1500;
    pub const ROW_BYTES: usize = 133;
    pub fn new(bytes: Vec<u8>) -> Result<Self, WorldError> {
        if bytes.len() != Self::ROWS * Self::ROW_BYTES {
            return Err(WorldError::Invalid(
                "walking mask must be 1500 rows of 133 bytes".into(),
            ));
        }
        Ok(Self(bytes))
    }
    /// Tested only when X is below 1064 and Y is positive; coordinates
    /// outside the mask's rows/columns do not block.
    pub fn blocks(&self, position: [i32; 2]) -> bool {
        let [x, y] = position;
        if !(0..1064).contains(&x) || y <= 0 || y as usize >= Self::ROWS {
            return false;
        }
        let byte = self.0[y as usize * Self::ROW_BYTES + x as usize / 8];
        byte & (1 << (7 - x % 8)) != 0
    }
}

/// Interactive world-map travel. Script-directed town travel (SPEC-008
/// §3.4) runs to completion inside one call and keeps no travel state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Travel {
    pub destination: [i32; 2],
    pub stepper: Stepper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encounter {
    /// A terrain map; entry context 2 (random encounter) or 1 (dropout).
    Terrain {
        map: &'static str,
        region: u8,
        context: i32,
    },
    /// A special map whose consumed bit is already marked; entry context 0.
    Special {
        map: &'static str,
        bit: u32,
        region: u8,
    },
}
impl Encounter {
    pub fn map(&self) -> &'static str {
        match self {
            Self::Terrain { map, .. } | Self::Special { map, .. } => map,
        }
    }
    pub fn region(&self) -> u8 {
        match self {
            Self::Terrain { region, .. } | Self::Special { region, .. } => *region,
        }
    }
    pub fn context(&self) -> i32 {
        match self {
            Self::Terrain { context, .. } => *context,
            Self::Special { .. } => 0,
        }
    }
}

/// Three independent inclusive d6 values, summed.
pub fn roll_3d6(rng: &mut impl RandomSource) -> Result<i32, WorldError> {
    Ok(rng.range(1, 6)? + rng.range(1, 6)? + rng.range(1, 6)?)
}

/// Ordinary terrain/dropdown map selection: redraw 0-2 until a populated
/// choice is found.
pub fn terrain_map(
    terrain: Terrain,
    rng: &mut impl RandomSource,
) -> Result<&'static str, WorldError> {
    let choices = &TERRAIN_MAPS[terrain as usize];
    for _ in 0..DRAW_LIMIT {
        let choice = choices[rng.range(0, 2)? as usize];
        if !choice.is_empty() {
            return Ok(choice);
        }
    }
    Ok(choices[0])
}

/// One map choice in a town's knowledge row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TownButton {
    /// The original hotspot slot, stored as the current section.
    pub slot: usize,
    pub event: i32,
    /// `[x, y, width, height]`.
    pub rect: [i32; 4],
    pub label: i32,
    pub map: &'static str,
    pub entry: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TownEntry {
    /// First entry: load the knowledge row's slot-zero map directly.
    Direct(TownButton),
    /// Later entry: display the town map.
    TownMap {
        picture: i32,
        buttons: Vec<TownButton>,
    },
}

fn global(gvars: &[i32], index: usize) -> i32 {
    gvars.get(index).copied().unwrap_or(0)
}

/// The saved component plus runtime travel state that is not serialized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldMap {
    pub saved: WorldMapSave,
    pub travel: Option<Travel>,
    pub schedule: Schedule,
    /// Iterations since the last encounter/healing check.
    pub interval: u32,
    /// Movement-loop iterations since the world-map screen was built, for
    /// the every-third discovery update and travel dot (iterations 1, 4,
    /// 7, ...).
    pub iterations: u32,
    /// The pixel the last iteration marked with a travel dot, if it was a
    /// marking iteration (SPEC-014 §3.39).
    pub mark: Option<[i32; 2]>,
    /// W and Q, computed when the world-map screen opens and again by
    /// script-directed travel; computed per iteration until then.
    pub speed: Option<TravelSpeed>,
    pub walk_mask: Option<Arc<WalkMask>>,
}
impl Default for WorldMap {
    /// New-game state: Vault 13's center, town 0, section 1, town zero
    /// enabled, and only its entrance slot zero known.
    fn default() -> Self {
        let mut saved = WorldMapSave {
            towns: 1,
            town: 0,
            section: 1,
            position: START.map(|value| value as u32),
            ..WorldMapSave::default()
        };
        saved.entrances[0] = 1;
        Self::from_saved(saved)
    }
}
impl WorldMap {
    pub fn from_saved(saved: WorldMapSave) -> Self {
        Self {
            saved,
            travel: None,
            schedule: Schedule::default(),
            interval: 0,
            iterations: 0,
            mark: None,
            speed: None,
            walk_mask: None,
        }
    }
    pub fn position(&self) -> [i32; 2] {
        self.saved.position.map(|value| value as i32)
    }
    pub fn set_position(&mut self, position: [i32; 2]) {
        self.saved.position = position.map(|value| value as u32);
    }
    pub fn cell(&self) -> Option<[usize; 2]> {
        cell_of(self.position())
    }
    /// Discovery byte for a playable cell, at the saved 29-byte stride.
    pub fn discovery(&self, cell: [usize; 2]) -> u8 {
        self.saved.discovery[STRIDE * cell[1] + cell[0]]
    }
    fn discovery_mut(&mut self, cell: [usize; 2]) -> &mut u8 {
        &mut self.saved.discovery[STRIDE * cell[1] + cell[0]]
    }
    /// The current cell becomes visited (2); its 3 x 3 neighborhood (5 x 5
    /// with Scout) becomes seen (1) unless visited, clipped to the playable
    /// grid. The west/ocean threshold then reveals the current row's cells
    /// west of the threshold when the column is at most that threshold.
    pub fn reveal(&mut self, scout: bool) {
        let Some([column, row]) = self.cell() else {
            return;
        };
        let radius = if scout { 2 } else { 1 };
        for y in row.saturating_sub(radius)..=(row + radius).min(ROWS - 1) {
            for x in column.saturating_sub(radius)..=(column + radius).min(COLUMNS - 1) {
                let value = self.discovery_mut([x, y]);
                if *value != 2 {
                    *value = 1;
                }
            }
        }
        *self.discovery_mut([column, row]) = 2;
        let threshold = WEST_REVEAL[row];
        if column <= threshold {
            for x in 0..threshold {
                *self.discovery_mut([x, row]) = 2;
            }
        }
    }
    /// New-game reset: under the SPEC-008 §3.5 safe rule every byte of the
    /// 31 x 29 saved array is cleared (the reference visits only rows 0-28,
    /// columns 0-27).
    pub fn reset_discovery(&mut self) {
        self.saved.discovery.fill(0);
    }
    pub fn town_enabled(&self, town: usize) -> bool {
        town < 32 && self.saved.towns & (1 << town) != 0
    }
    pub fn entrance_known(&self, row: usize, slot: usize) -> bool {
        row < KNOWLEDGE_ROWS
            && slot < ENTRANCE_SLOTS
            && self.saved.entrances[ENTRANCE_SLOTS * row + slot] != 0
    }
    fn set_entrance(&mut self, row: usize, slot: usize, known: bool) {
        self.saved.entrances[ENTRANCE_SLOTS * row + slot] = u8::from(known);
    }
    /// Derive town and entrance knowledge from campaign globals. Under the
    /// SPEC-008 §3.7 safe rule an entrance global is tested as a full 32-bit
    /// value (the reference narrows it to a byte); a known-town value must
    /// equal 1 to enable the town, reveal its cell, and enable slot 0.
    pub fn refresh_knowledge(&mut self, gvars: &[i32], ticks: u64) {
        for (town, row) in TOWNS.iter().enumerate().skip(1) {
            for (slot, index) in row.entrance_globals.iter().enumerate() {
                self.set_entrance(town, slot, global(gvars, *index) != 0);
            }
            if global(gvars, row.known_global) == 1 {
                self.saved.towns |= 1 << town;
                if self.discovery(row.cell) == 0 {
                    *self.discovery_mut(row.cell) = 1;
                }
                self.set_entrance(town, 0, true);
            }
        }
        self.set_entrance(0, 0, true);
        for slot in 1..TOWNS[0].entrance_globals.len() {
            self.set_entrance(0, slot, ticks >= VAULT13_OPEN_TICK);
        }
        let any = |map: &Self, town: usize, slots: std::ops::Range<usize>| {
            slots.into_iter().any(|slot| map.entrance_known(town, slot))
        };
        let cathedral = global(gvars, CATHEDRAL_DESTROYED_GLOBAL) != 0
            || any(self, CATHEDRAL, 0..ENTRANCE_SLOTS);
        let base = global(gvars, BASE_DESTROYED_GLOBAL) != 0
            || any(self, MILITARY_BASE, 0..ENTRANCE_SLOTS);
        let brotherhood = any(self, BROTHERHOOD, 1..5);
        self.set_entrance(12, 0, cathedral);
        self.set_entrance(13, 0, base);
        self.set_entrance(14, 0, brotherhood);
    }
    /// The knowledge row used for a town: destroyed Cathedral (global 18)
    /// and Military Base (global 17) substitute alternate rows 12 and 13.
    pub fn knowledge_row(town: usize, gvars: &[i32]) -> usize {
        match town {
            CATHEDRAL if global(gvars, CATHEDRAL_DESTROYED_GLOBAL) != 0 => 12,
            MILITARY_BASE if global(gvars, BASE_DESTROYED_GLOBAL) != 0 => 13,
            _ => town,
        }
    }
    fn button(
        town: usize,
        row: usize,
        ordinal: usize,
        slot: usize,
        hotspot: &Hotspot,
    ) -> TownButton {
        TownButton {
            slot,
            event: TOWN_BUTTON_EVENT + ordinal as i32,
            rect: [
                hotspot.x - 57,
                hotspot.y - 6,
                TOWN_BUTTON_SIZE[0],
                TOWN_BUTTON_SIZE[1],
            ],
            label: match row {
                12 => 290,
                13 => 501,
                _ => 200 + 10 * town as i32 + slot as i32,
            },
            map: hotspot.map,
            entry: hotspot.entry,
        }
    }
    /// Buttons for known slots only; event numbers use the visible
    /// ordinal while each button keeps its original slot.
    pub fn town_buttons(&self, town: usize, gvars: &[i32]) -> Vec<TownButton> {
        let row = Self::knowledge_row(town, gvars);
        hotspots(row)
            .filter(|(slot, _)| self.entrance_known(row, *slot))
            .enumerate()
            .map(|(ordinal, (slot, hotspot))| Self::button(town, row, ordinal, slot, hotspot))
            .collect()
    }
    /// First entry to a town whose enabled/visited bit is clear loads its
    /// row's slot-zero map directly and sets the bit; later entry displays
    /// the town map.
    pub fn enter_town(&mut self, town: usize, gvars: &[i32]) -> Option<TownEntry> {
        if town >= TOWNS.len() {
            return None;
        }
        let row = Self::knowledge_row(town, gvars);
        if !self.town_enabled(town) {
            self.saved.towns |= 1 << town;
            let (slot, hotspot) = hotspots(row).next()?;
            return Some(TownEntry::Direct(Self::button(town, row, 0, slot, hotspot)));
        }
        Some(TownEntry::TownMap {
            picture: TOWN_PICTURE_BASE + town as i32,
            buttons: self.town_buttons(town, gvars),
        })
    }
    /// The W-iteration encounter test at the current cell. The 3d6 draw
    /// happens even in a town cell, which suppresses entry. A selected
    /// encounter then tests for a special map, restarting the whole special
    /// test when the drawn special was already consumed.
    pub fn check_encounter(
        &mut self,
        rng: &mut impl RandomSource,
        luck: i32,
        explorer: u32,
    ) -> Result<Option<Encounter>, WorldError> {
        let sum = roll_3d6(rng)?;
        let Some(cell) = self.cell() else {
            return Ok(None);
        };
        if town_at(cell).is_some() || sum >= ENCOUNTER_BELOW[usize::from(frequency(cell)) & 3] {
            return Ok(None);
        }
        let region = region(cell);
        for _ in 0..DRAW_LIMIT {
            let gate = roll_3d6(rng)? - 5 + luck + 2 * explorer.min(1 << 20) as i32;
            if gate < 18 || self.saved.specials & ALL_SPECIALS == ALL_SPECIALS {
                break;
            }
            let draw = rng.range(1, 100)?;
            // The table ends at 100; only a foreign generator can draw past it.
            let (_, map, bit) = SPECIAL_ENCOUNTERS
                .iter()
                .find(|(last, ..)| draw <= *last)
                .copied()
                .ok_or_else(|| {
                    WorldError::Invalid("special encounter draw outside 1-100".into())
                })?;
            if self.saved.specials & bit == 0 {
                self.saved.specials |= bit;
                return Ok(Some(Encounter::Special { map, bit, region }));
            }
        }
        Ok(Some(Encounter::Terrain {
            map: terrain_map(terrain(cell), rng)?,
            region,
            context: 2,
        }))
    }
}

/// SPEC-014 §3.39-§3.40 world-map and town-map screen geometry, in window
/// coordinates (the 640-by-480 window is at screen (0, 0)).
pub mod screen {
    use super::{SCROLL_LIMIT, VIEWPORT_ORIGIN, VIEWPORT_SIZE, clamp_scroll};

    /// The view's last window pixel, inclusive.
    pub const VIEW_END: [i32; 2] = [
        VIEWPORT_ORIGIN[0] + VIEWPORT_SIZE[0] - 1,
        VIEWPORT_ORIGIN[1] + VIEWPORT_SIZE[1] - 1,
    ];
    /// One scroll step on each requested axis.
    pub const SCROLL_STEP: i32 = 16;
    /// A pass that redraws lasts at least `trunc(1000 / 24)` milliseconds.
    pub const PASS_MS: u64 = 1000 / 24;
    /// The encounter flash: 14 images, each held 199 milliseconds.
    pub const FLASH_IMAGES: u64 = 14;
    pub const FLASH_MS: u64 = 199;
    /// The pressed triangle's wait before entering: 500 milliseconds for a
    /// town-button entry while travelling, else 300 after the release.
    pub const ENTER_MS: u64 = 300;
    pub const ENTER_TRAVELLING_MS: u64 = 500;
    /// The side panel: a 15-by-15 red button per town at (508, Y), its
    /// 82-by-18 name plate at (531, Y), and the Town/World button.
    pub const TOWN_BUTTON_X: i32 = 508;
    pub const TOWN_BUTTON_Y: [i32; 12] = [61, 88, 115, 143, 171, 200, 228, 256, 283, 310, 338, 367];
    pub const BUTTON_SIZE: [i32; 2] = [15, 15];
    pub const TOWN_NAME_X: i32 = 531;
    pub const TOWN_NAME_SIZE: [i32; 2] = [82, 18];
    pub const TOWN_WORLD_BUTTON: [i32; 2] = [520, 439];
    /// The date and time: 9-by-17 `numbers.frm` digits (white band, digit
    /// d at x = 9d), and 29-by-14 month rows 15 apart in `months.frm`.
    pub const DAY: [i32; 2] = [487, 12];
    pub const MONTH: [i32; 2] = [513, 12];
    pub const YEAR: [i32; 2] = [548, 12];
    pub const TIME: [i32; 2] = [593, 12];
    pub const DIGIT: [i32; 2] = [9, 17];
    pub const MONTH_SIZE: [i32; 2] = [29, 14];
    /// Hover and town-map labels: font 101, key 992, with shadow.
    pub const LABEL_FONT: i32 = 101;
    pub const LABEL_KEY: u16 = 992;
    /// Travel dots: key 27648 (dark red).
    pub const DOT_KEY: u16 = 27648;
    /// The town-map marker image within its 164-by-25 button.
    pub const MARKER_TRIANGLE: [i32; 2] = [69, 12];
    pub const MARKER_SIZE: [i32; 2] = [25, 13];
    /// The town picture's window position (453 by 444).
    pub const TOWN_PICTURE: [i32; 2] = [20, 20];

    /// Centering on world position (X, Y): `(X - 247, Y - 242)`, clamped.
    pub fn centered(position: [i32; 2]) -> [i32; 2] {
        clamp_scroll([position[0] - 247, position[1] - 242])
    }
    /// The window position of world pixel `world` under view origin `view`.
    pub fn window(world: [i32; 2], view: [i32; 2]) -> [i32; 2] {
        [
            world[0] - view[0] + VIEWPORT_ORIGIN[0],
            world[1] - view[1] + VIEWPORT_ORIGIN[1],
        ]
    }
    /// The world pixel under window pixel `point`, with no jitter.
    pub fn world_at(point: [i32; 2], view: [i32; 2]) -> [i32; 2] {
        [
            view[0] + point[0] - VIEWPORT_ORIGIN[0],
            view[1] + point[1] - VIEWPORT_ORIGIN[1],
        ]
    }
    /// A left release at x 23-471 and y 22-462 is a view click.
    pub fn view_click(point: [i32; 2]) -> bool {
        (VIEWPORT_ORIGIN[0] + 1..=VIEW_END[0]).contains(&point[0])
            && (VIEWPORT_ORIGIN[1] + 1..=VIEW_END[1]).contains(&point[1])
    }
    /// The hover test: `dx = |mx - px|`, `dy = |my - (py - 1)|`; the smaller
    /// is halved (dy when they are equal), and the sum must be at most 10.
    pub fn hovers(pointer: [i32; 2], party: [i32; 2]) -> bool {
        let dx = (pointer[0] - party[0]).abs();
        let dy = (pointer[1] - (party[1] - 1)).abs();
        let (dx, dy) = if dx < dy { (dx / 2, dy) } else { (dx, dy / 2) };
        dx + dy <= 10
    }
    /// The hover label's top-left corner for text width `width`:
    /// `(px - 12 + trunc((25 - w) / 2), py - 17)`, truncated toward zero.
    pub fn label_origin(party: [i32; 2], width: i32) -> [i32; 2] {
        [party[0] - 12 + (25 - width) / 2, party[1] - 17]
    }
    /// The label is drawn only when `x > 22 - w`, `x < 472` and
    /// `11 < y < 463`.
    pub fn label_shown(origin: [i32; 2], width: i32) -> bool {
        origin[0] > VIEWPORT_ORIGIN[0] - width
            && origin[0] <= VIEW_END[0]
            && 11 < origin[1]
            && origin[1] <= VIEW_END[1]
    }

    /// The world-map markers and their drawing windows.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Marker {
        /// `hotspot1.frm` / `hotspot2.frm`, 25 by 13, at (px - 12, py - 6).
        Triangle,
        /// `wmaploc.frm`, 5 by 5, at (px - 2, py - 2).
        Plus,
        /// `wmaptarg.frm` / `wmaptrg2.frm`, 11 by 11, at the point - (5, 5).
        Crosshair,
        /// `wmapfgt0.frm` / `wmapfgt1.frm`, 7 by 11, at (px - 3, py - 5).
        Bolt,
    }
    impl Marker {
        /// The top-left corner for a marker at window point `at`.
        pub fn origin(self, at: [i32; 2]) -> [i32; 2] {
            let [dx, dy] = match self {
                Self::Triangle => [12, 6],
                Self::Plus => [2, 2],
                Self::Crosshair => [5, 5],
                Self::Bolt => [3, 5],
            };
            [at[0] - dx, at[1] - dy]
        }
        /// A marker is skipped unless its top-left corner lies in these
        /// ranges.
        pub fn shown(self, origin: [i32; 2]) -> bool {
            let (x, y) = match self {
                Self::Triangle => (-2..=483, 9..=462),
                Self::Plus => (18..=473, 17..=462),
                Self::Crosshair => (12..=473, 11..=462),
                Self::Bolt => (16..=471, 11..=462),
            };
            x.contains(&origin[0]) && y.contains(&origin[1])
        }
    }

    /// The edge-scroll direction for a pointer at `point`: x 0-1 west, x
    /// 638-639 east, y 0-1 north, y 478-479 south; zero off the edges.
    pub fn edge(point: [i32; 2]) -> [i32; 2] {
        let axis = |value: i32, far: i32| {
            if value <= 1 {
                -1
            } else if value >= far - 1 {
                1
            } else {
                0
            }
        };
        [axis(point[0], 639), axis(point[1], 479)]
    }
    /// The scroll-cursor index (NW, N, NE, E, SE, S, SW, W) of a direction.
    pub fn cursor_index(direction: [i32; 2]) -> Option<usize> {
        Some(match direction {
            [-1, -1] => 0,
            [0, -1] => 1,
            [1, -1] => 2,
            [1, 0] => 3,
            [1, 1] => 4,
            [0, 1] => 5,
            [-1, 1] => 6,
            [-1, 0] => 7,
            _ => return None,
        })
    }
    /// One 16-pixel step on each requested axis, each clamped on its own.
    /// Also returns whether every requested axis was already at its limit.
    pub fn scroll(view: [i32; 2], direction: [i32; 2]) -> ([i32; 2], bool) {
        let mut next = view;
        let mut blocked = direction != [0, 0];
        for axis in 0..2 {
            let step = direction[axis];
            if step == 0 {
                continue;
            }
            let limit = if step < 0 { 0 } else { SCROLL_LIMIT[axis] };
            if view[axis] != limit {
                blocked = false;
            }
            next[axis] = (view[axis] + SCROLL_STEP * step).clamp(0, SCROLL_LIMIT[axis]);
        }
        (next, blocked)
    }
    /// Home: centers both axes on the party, then sets vy to 0. The
    /// reference then steps 16 pixels south, ending at vy 16; under the
    /// `[May-improve]` rule vy stays 0.
    pub fn home(position: [i32; 2]) -> [i32; 2] {
        [centered(position)[0], 0]
    }
    /// End: vy becomes 1058; vx is unchanged.
    pub fn end(view: [i32; 2]) -> [i32; 2] {
        [view[0], SCROLL_LIMIT[1]]
    }
    /// The window rectangle `[x, y, w, h]` of discovery cell `cell` under
    /// `view`, clipped to the view; none when nothing of it shows.
    pub fn cell_rect(cell: [usize; 2], view: [i32; 2]) -> Option<[i32; 4]> {
        let size = super::CELL;
        let [x, y] = window([cell[0] as i32 * size, cell[1] as i32 * size], view);
        let left = x.max(VIEWPORT_ORIGIN[0]);
        let top = y.max(VIEWPORT_ORIGIN[1]);
        let right = (x + size).min(VIEW_END[0] + 1);
        let bottom = (y + size).min(VIEW_END[1] + 1);
        (left < right && top < bottom).then_some([left, top, right - left, bottom - top])
    }
    /// The 15-by-15 red button of town `town` (0-11).
    pub fn town_button(town: usize) -> Option<[i32; 4]> {
        let y = *TOWN_BUTTON_Y.get(town)?;
        Some([TOWN_BUTTON_X, y, BUTTON_SIZE[0], BUTTON_SIZE[1]])
    }
    /// The Town/World button (event 512).
    pub fn town_world_button() -> [i32; 4] {
        [
            TOWN_WORLD_BUTTON[0],
            TOWN_WORLD_BUTTON[1],
            BUTTON_SIZE[0],
            BUTTON_SIZE[1],
        ]
    }
    /// A town-map marker's triangle (window position) for its button.
    pub fn marker_triangle(button: [i32; 4]) -> [i32; 2] {
        [
            button[0] + MARKER_TRIANGLE[0],
            button[1] + MARKER_TRIANGLE[1],
        ]
    }
    /// A town-map label's x offset in its button: `max(0, trunc((164 -
    /// w) / 2))`.
    pub fn marker_label_x(width: i32) -> i32 {
        ((super::TOWN_BUTTON_SIZE[0] - width) / 2).max(0)
    }
    /// The town picture `twnmapNN.frm` of a town 0-11 (list index 156 + t).
    pub fn town_picture(town: usize) -> Option<String> {
        (town < super::TOWNS.len()).then(|| format!("twnmap{town:02}.frm"))
    }
}
