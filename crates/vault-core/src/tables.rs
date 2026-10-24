//! The campaign tables `build.rs` compiles from the approved specs (the
//! SPEC-007 §2.4-2.5 critical tables, the SPEC-008 §3.6 world grids, the
//! §3.7 town and hotspot catalogs and the §3.10 map table) as one owned,
//! replaceable set.
//!
//! `build.rs` also emits each table as canonical text, embedded here with
//! `include_str!`, in the format [`CampaignTables::with_override`] parses:
//! `#` comment lines, then one `|`-separated record per line. A mod may
//! replace a whole table by shipping that text at
//! [`Table::override_path`] (`data/vault-engine/<table>.txt`). The host
//! reads overrides only when the engine preset is
//! [`Enhanced`](crate::EnginePreset::Enhanced) or a mod layer is mounted;
//! a Vanilla world always runs on the compiled tables.
//!
//! An override must keep each table's fixed shape (864 critical entries,
//! 7 failure rows, 30 x 28 grids, 12 towns, 15 hotspot rows of 7 slots,
//! 66 maps): it changes values, never the shape the rules index by.
use crate::{
    WorldError,
    critical::{CRITICAL_FAILURE, CRITICAL_SUCCESS, CriticalEffect},
    worldmap::{self, COLUMNS, ENTRANCE_SLOTS, MAP_COUNT, ROWS, Terrain},
};
use std::sync::OnceLock;

/// The tables a mod can override, in canonical order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Table {
    Critical,
    CriticalFailure,
    Terrain,
    Frequency,
    Region,
    Towns,
    Hotspots,
    Maps,
}
impl Table {
    pub const ALL: [Self; 8] = [
        Self::Critical,
        Self::CriticalFailure,
        Self::Terrain,
        Self::Frequency,
        Self::Region,
        Self::Towns,
        Self::Hotspots,
        Self::Maps,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::CriticalFailure => "critical_failure",
            Self::Terrain => "terrain",
            Self::Frequency => "frequency",
            Self::Region => "region",
            Self::Towns => "towns",
            Self::Hotspots => "hotspots",
            Self::Maps => "maps",
        }
    }
    /// The VFS path an override of this table is read from.
    pub fn override_path(self) -> String {
        format!("data/vault-engine/{}.txt", self.name())
    }
    /// The canonical text `build.rs` emitted for the compiled table.
    pub fn canonical(self) -> &'static str {
        match self {
            Self::Critical => include_str!(concat!(env!("OUT_DIR"), "/campaign_critical.txt")),
            Self::CriticalFailure => {
                include_str!(concat!(env!("OUT_DIR"), "/campaign_critical_failure.txt"))
            }
            Self::Terrain => include_str!(concat!(env!("OUT_DIR"), "/campaign_terrain.txt")),
            Self::Frequency => include_str!(concat!(env!("OUT_DIR"), "/campaign_frequency.txt")),
            Self::Region => include_str!(concat!(env!("OUT_DIR"), "/campaign_region.txt")),
            Self::Towns => include_str!(concat!(env!("OUT_DIR"), "/campaign_towns.txt")),
            Self::Hotspots => include_str!(concat!(env!("OUT_DIR"), "/campaign_hotspots.txt")),
            Self::Maps => include_str!(concat!(env!("OUT_DIR"), "/campaign_maps.txt")),
        }
    }
}

/// One town row (SPEC-008 §3.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TownRow {
    pub name: String,
    pub cell: [usize; 2],
    pub known_global: usize,
    pub entrance_globals: Vec<usize>,
}
/// One town-map hotspot; an unlisted slot is all zero and empty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HotspotRow {
    pub x: i32,
    pub y: i32,
    pub entry: i32,
    pub map: String,
}
/// One map-number row (SPEC-008 §3.10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapRow {
    pub name: String,
    pub town: usize,
    pub music: String,
}

pub type Grid = [[u8; COLUMNS]; ROWS];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampaignTables {
    pub critical_success: Box<[[[CriticalEffect; 6]; 9]; 16]>,
    pub critical_failure: [[u32; 5]; 7],
    pub terrain: Grid,
    pub frequency: Grid,
    pub region: Grid,
    pub towns: Vec<TownRow>,
    pub hotspots: Vec<[HotspotRow; ENTRANCE_SLOTS]>,
    pub maps: Vec<MapRow>,
}

fn invalid(table: Table, line: usize, message: impl std::fmt::Display) -> WorldError {
    WorldError::Invalid(format!("{} line {line}: {message}", table.override_path()))
}
/// The records of a canonical text: `(line number, fields)`, skipping
/// blank and `#` lines, each with exactly `width` fields.
fn records(table: Table, text: &str, width: usize) -> Result<Vec<(usize, Vec<&str>)>, WorldError> {
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('|').map(str::trim).collect();
        if fields.len() != width {
            return Err(invalid(
                table,
                index + 1,
                format!("expected {width} fields, found {}", fields.len()),
            ));
        }
        out.push((index + 1, fields));
    }
    Ok(out)
}
fn number<T: std::str::FromStr>(table: Table, line: usize, field: &str) -> Result<T, WorldError> {
    field
        .parse()
        .map_err(|_| invalid(table, line, format!("{field:?} is not a number")))
}
fn hex(table: Table, line: usize, field: &str) -> Result<u32, WorldError> {
    u32::from_str_radix(field, 16)
        .map_err(|_| invalid(table, line, format!("{field:?} is not hexadecimal")))
}
/// An index in `0..len` that has not been seen before.
fn slot(
    table: Table,
    line: usize,
    field: &str,
    len: usize,
    seen: &mut [bool],
) -> Result<usize, WorldError> {
    let index: usize = number(table, line, field)?;
    if index >= len {
        return Err(invalid(
            table,
            line,
            format!("index {index} outside 0-{}", len - 1),
        ));
    }
    if std::mem::replace(&mut seen[index], true) {
        return Err(invalid(table, line, format!("duplicate entry {index}")));
    }
    Ok(index)
}
fn complete(table: Table, seen: &[bool]) -> Result<(), WorldError> {
    match seen.iter().position(|seen| !seen) {
        Some(missing) => Err(invalid(table, 0, format!("entry {missing} is missing"))),
        None => Ok(()),
    }
}
/// A bare 8.3 file name with no path or traversal.
fn plain_name(table: Table, line: usize, name: &str, extension: &str) -> Result<(), WorldError> {
    let ok = name.len() <= 12
        && name.ends_with(extension)
        && name.len() > extension.len()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        && name.matches('.').count() == 1;
    if ok {
        Ok(())
    } else {
        Err(invalid(
            table,
            line,
            format!("{name:?} is not a plain 8.3 {extension} name"),
        ))
    }
}

impl CampaignTables {
    /// The tables compiled from the approved specs (shared, built once).
    pub fn compiled() -> &'static Self {
        static COMPILED: OnceLock<CampaignTables> = OnceLock::new();
        COMPILED.get_or_init(|| Self {
            critical_success: Box::new(CRITICAL_SUCCESS),
            critical_failure: CRITICAL_FAILURE,
            terrain: worldmap::TERRAIN,
            frequency: worldmap::FREQUENCY,
            region: worldmap::REGION,
            towns: worldmap::TOWNS
                .iter()
                .map(|town| TownRow {
                    name: town.name.to_string(),
                    cell: town.cell,
                    known_global: town.known_global,
                    entrance_globals: town.entrance_globals.to_vec(),
                })
                .collect(),
            hotspots: worldmap::HOTSPOTS
                .iter()
                .map(|row| {
                    row.map(|hotspot| HotspotRow {
                        x: hotspot.x,
                        y: hotspot.y,
                        entry: hotspot.entry,
                        map: hotspot.map.to_string(),
                    })
                })
                .collect(),
            maps: worldmap::MAP_TABLE
                .iter()
                .map(|entry| MapRow {
                    name: entry.name.to_string(),
                    town: entry.town,
                    music: entry.music.to_string(),
                })
                .collect(),
        })
    }
    /// A copy with one table replaced by the parsed text. The text is
    /// validated completely (shape, ranges, duplicates, plain file names)
    /// before anything is replaced.
    pub fn with_override(&self, table: Table, text: &str) -> Result<Self, WorldError> {
        let mut out = self.clone();
        match table {
            Table::Critical => {
                let mut seen = vec![false; 16 * 9 * 6];
                for (line, f) in records(table, text, 10)? {
                    let victim: usize = number(table, line, f[0])?;
                    let location: usize = number(table, line, f[1])?;
                    let tier: usize = number(table, line, f[2])?;
                    if victim >= 16 || location >= 9 || tier >= 6 {
                        return Err(invalid(table, line, "victim 0-15, location 0-8, tier 0-5"));
                    }
                    let index = (victim * 9 + location) * 6 + tier;
                    slot(table, line, &index.to_string(), seen.len(), &mut seen)?;
                    out.critical_success[victim][location][tier] = CriticalEffect {
                        multiplier: number(table, line, f[3])?,
                        flags: hex(table, line, f[4])?,
                        save_stat: number(table, line, f[5])?,
                        save_adjustment: number(table, line, f[6])?,
                        failed_flags: hex(table, line, f[7])?,
                        message: number(table, line, f[8])?,
                        failed_message: number(table, line, f[9])?,
                    };
                }
                complete(table, &seen)?;
            }
            Table::CriticalFailure => {
                let mut seen = [false; 7];
                for (line, f) in records(table, text, 6)? {
                    let row = slot(table, line, f[0], 7, &mut seen)?;
                    for column in 0..5 {
                        out.critical_failure[row][column] = hex(table, line, f[column + 1])?;
                    }
                }
                complete(table, &seen)?;
            }
            Table::Terrain | Table::Frequency | Table::Region => {
                let mut seen = [false; ROWS];
                let mut grid = [[0; COLUMNS]; ROWS];
                for (line, f) in records(table, text, 2)? {
                    let row = slot(table, line, f[0], ROWS, &mut seen)?;
                    let digits: Vec<u8> = f[1]
                        .chars()
                        .map(|digit| digit.to_digit(16).map(|value| value as u8))
                        .collect::<Option<_>>()
                        .ok_or_else(|| invalid(table, line, "cells must be hexadecimal digits"))?;
                    if digits.len() != COLUMNS {
                        return Err(invalid(table, line, format!("expected {COLUMNS} cells")));
                    }
                    let limit = match table {
                        Table::Terrain | Table::Frequency => 3,
                        _ => 15,
                    };
                    if digits.iter().any(|digit| *digit > limit) {
                        return Err(invalid(table, line, format!("cell above {limit}")));
                    }
                    grid[row].copy_from_slice(&digits);
                }
                complete(table, &seen)?;
                *match table {
                    Table::Terrain => &mut out.terrain,
                    Table::Frequency => &mut out.frequency,
                    _ => &mut out.region,
                } = grid;
            }
            Table::Towns => {
                let count = self.towns.len();
                let mut seen = vec![false; count];
                let mut towns = self.towns.clone();
                for (line, f) in records(table, text, 6)? {
                    let id = slot(table, line, f[0], count, &mut seen)?;
                    let cell = [number(table, line, f[2])?, number(table, line, f[3])?];
                    if cell[0] >= COLUMNS || cell[1] >= ROWS {
                        return Err(invalid(table, line, "cell outside the 28 x 30 grid"));
                    }
                    let entrance_globals = f[5]
                        .split(',')
                        .map(|global| number(table, line, global.trim()))
                        .collect::<Result<Vec<usize>, _>>()?;
                    if entrance_globals.is_empty() || entrance_globals.len() > ENTRANCE_SLOTS {
                        return Err(invalid(table, line, "1-7 entrance globals"));
                    }
                    if f[1].is_empty() {
                        return Err(invalid(table, line, "a town needs a name"));
                    }
                    towns[id] = TownRow {
                        name: f[1].to_string(),
                        cell,
                        known_global: number(table, line, f[4])?,
                        entrance_globals,
                    };
                }
                complete(table, &seen)?;
                out.towns = towns;
            }
            Table::Hotspots => {
                let rows = self.hotspots.len();
                let mut hotspots = vec![<[HotspotRow; ENTRANCE_SLOTS]>::default(); rows];
                let mut seen = vec![false; rows * ENTRANCE_SLOTS];
                for (line, f) in records(table, text, 6)? {
                    let row: usize = number(table, line, f[0])?;
                    let column: usize = number(table, line, f[1])?;
                    if row >= rows || column >= ENTRANCE_SLOTS {
                        return Err(invalid(table, line, "row 0-14, slot 0-6"));
                    }
                    let index = row * ENTRANCE_SLOTS + column;
                    slot(table, line, &index.to_string(), seen.len(), &mut seen)?;
                    plain_name(table, line, f[5], ".MAP")?;
                    hotspots[row][column] = HotspotRow {
                        x: number(table, line, f[2])?,
                        y: number(table, line, f[3])?,
                        entry: number(table, line, f[4])?,
                        map: f[5].to_string(),
                    };
                }
                out.hotspots = hotspots;
            }
            Table::Maps => {
                let mut seen = vec![false; MAP_COUNT];
                let mut maps = self.maps.clone();
                for (line, f) in records(table, text, 4)? {
                    let number_ = slot(table, line, f[0], MAP_COUNT, &mut seen)?;
                    plain_name(table, line, f[1], ".MAP")?;
                    if f[1] != f[1].to_ascii_uppercase() {
                        return Err(invalid(table, line, "map names are uppercase"));
                    }
                    let town: usize = number(table, line, f[2])?;
                    if town >= self.towns.len() {
                        return Err(invalid(table, line, format!("town {town} is not a town")));
                    }
                    let music = f[3];
                    if music.is_empty()
                        || music.len() > 8
                        || !music
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    {
                        return Err(invalid(table, line, format!("{music:?} is not a track")));
                    }
                    maps[number_] = MapRow {
                        name: f[1].to_string(),
                        town,
                        music: music.to_string(),
                    };
                }
                complete(table, &seen)?;
                out.maps = maps;
            }
        }
        Ok(out)
    }

    /// SPEC-007 critical-success effect (as [`crate::critical::success_effect`]).
    pub fn success_effect(
        &self,
        matrix: usize,
        location: u8,
        tier: usize,
    ) -> Option<CriticalEffect> {
        self.critical_success
            .get(matrix)?
            .get(usize::from(location))?
            .get(tier)
            .copied()
    }
    /// SPEC-007 §2.4 failure mask; a table outside 0-6 uses table 0.
    pub fn failure_mask(&self, table: i32, column: usize) -> Option<u32> {
        let row = usize::try_from(table)
            .ok()
            .and_then(|table| self.critical_failure.get(table))
            .unwrap_or(&self.critical_failure[0]);
        row.get(column).copied()
    }
    pub fn terrain(&self, cell: [usize; 2]) -> Terrain {
        match self.terrain[cell[1]][cell[0]] {
            0 => Terrain::Desert,
            1 => Terrain::Mountain,
            2 => Terrain::City,
            _ => Terrain::Coast,
        }
    }
    pub fn frequency(&self, cell: [usize; 2]) -> u8 {
        self.frequency[cell[1]][cell[0]]
    }
    pub fn region(&self, cell: [usize; 2]) -> u8 {
        self.region[cell[1]][cell[0]]
    }
    pub fn town_at(&self, cell: [usize; 2]) -> Option<usize> {
        self.towns.iter().position(|town| town.cell == cell)
    }
    pub fn map_row(&self, number: i32) -> Option<&MapRow> {
        usize::try_from(number)
            .ok()
            .and_then(|number| self.maps.get(number))
    }
    /// SPEC-008 §3.10 the town of a map number, -1 outside 0-65.
    pub fn map_town(&self, number: i32) -> i32 {
        self.map_row(number).map_or(-1, |row| row.town as i32)
    }
    pub fn city_music(&self, number: i32) -> Option<&str> {
        self.map_row(number).map(|row| row.music.as_str())
    }
    pub fn map_name(&self, number: i32) -> Option<&str> {
        self.map_row(number).map(|row| row.name.as_str())
    }
    /// Every table as canonical text, in the same format `build.rs` emits.
    pub fn to_text(&self, table: Table) -> String {
        let mut lines = Vec::new();
        match table {
            Table::Critical => {
                for (victim, locations) in self.critical_success.iter().enumerate() {
                    for (location, tiers) in locations.iter().enumerate() {
                        for (tier, e) in tiers.iter().enumerate() {
                            lines.push(format!(
                                "{victim}|{location}|{tier}|{}|{:x}|{}|{}|{:x}|{}|{}",
                                e.multiplier,
                                e.flags,
                                e.save_stat,
                                e.save_adjustment,
                                e.failed_flags,
                                e.message,
                                e.failed_message
                            ));
                        }
                    }
                }
            }
            Table::CriticalFailure => {
                for (row, masks) in self.critical_failure.iter().enumerate() {
                    let masks: Vec<_> = masks.iter().map(|mask| format!("{mask:x}")).collect();
                    lines.push(format!("{row}|{}", masks.join("|")));
                }
            }
            Table::Terrain | Table::Frequency | Table::Region => {
                let grid = match table {
                    Table::Terrain => &self.terrain,
                    Table::Frequency => &self.frequency,
                    _ => &self.region,
                };
                for (row, cells) in grid.iter().enumerate() {
                    let digits: String = cells.iter().map(|cell| format!("{cell:X}")).collect();
                    lines.push(format!("{row}|{digits}"));
                }
            }
            Table::Towns => {
                for (id, town) in self.towns.iter().enumerate() {
                    let globals: Vec<_> =
                        town.entrance_globals.iter().map(usize::to_string).collect();
                    lines.push(format!(
                        "{id}|{}|{}|{}|{}|{}",
                        town.name,
                        town.cell[0],
                        town.cell[1],
                        town.known_global,
                        globals.join(",")
                    ));
                }
            }
            Table::Hotspots => {
                for (row, slots) in self.hotspots.iter().enumerate() {
                    for (index, h) in slots.iter().enumerate() {
                        if h.map.is_empty() && h.x == 0 && h.y == 0 {
                            continue;
                        }
                        lines.push(format!(
                            "{row}|{index}|{}|{}|{}|{}",
                            h.x, h.y, h.entry, h.map
                        ));
                    }
                }
            }
            Table::Maps => {
                for (number, row) in self.maps.iter().enumerate() {
                    lines.push(format!("{number}|{}|{}|{}", row.name, row.town, row.music));
                }
            }
        }
        lines.join("\n")
    }
}
