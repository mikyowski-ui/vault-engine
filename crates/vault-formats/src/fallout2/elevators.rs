//! SPEC-018 §4.9 the engine-owned elevator table,
//! `data/vault-engine/elevators.txt`.
use super::{
    maps::MapsTable,
    text::{self, Diagnostics, SectionedText, TableError},
};

pub const FILE: &str = "data/vault-engine/elevators.txt";

/// One button: a destination and its key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stop {
    pub map: u32,
    pub elevation: u8,
    pub tile: i32,
    /// One printable ASCII character; a letter matches either case.
    pub key: u8,
}

impl Stop {
    /// Whether key `pressed` selects this stop (SPEC-013 §3.21's safe rule:
    /// lowercase keys are accepted too).
    pub fn matches(&self, pressed: u8) -> bool {
        pressed.eq_ignore_ascii_case(&self.key)
    }
}

/// One `[Elevator N]` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Elevator {
    pub number: u32,
    pub background: String,
    pub panel: Option<String>,
    /// The valid stops in ascending K, renumbered from 0.
    pub stops: Vec<Stop>,
    pub doors: Vec<u32>,
}

impl Elevator {
    pub fn background_path(&self) -> String {
        format!("art/intrface/{}.frm", self.background.to_ascii_lowercase())
    }
    /// SPEC-013 §3.21's safe rule: the start level is the stop that matches
    /// both the current map and the player's elevation, else 0.
    pub fn start_level(&self, map: i32, elevation: u8) -> usize {
        self.stops
            .iter()
            .position(|stop| i64::from(stop.map) == i64::from(map) && stop.elevation == elevation)
            .unwrap_or(0)
    }
}

/// The parsed elevator table; an absent file is an empty one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ElevatorTable {
    pub elevators: std::collections::BTreeMap<u32, Elevator>,
}

impl ElevatorTable {
    pub fn parse(
        bytes: &[u8],
        maps: &MapsTable,
        notes: &mut Diagnostics,
    ) -> Result<Self, TableError> {
        let file = FILE;
        let table = SectionedText::parse(file, bytes, notes)?;
        let mut elevators = std::collections::BTreeMap::new();
        for (number, section) in table.numbered("elevator") {
            if number > 99 {
                notes.push(
                    file,
                    section.line,
                    format!("elevator {number} outside 0-99"),
                );
                continue;
            }
            let Some(background) = section
                .get("background")
                .and_then(|value| text::stem(&value.text))
            else {
                notes.push(
                    file,
                    section.line,
                    format!("[{}] has no background; dropped", section.name),
                );
                continue;
            };
            let panel = section
                .get("panel")
                .filter(|value| !value.text.is_empty())
                .and_then(|value| text::stem(&value.text));
            let mut stops = Vec::new();
            for (k, value) in section.indexed("stop", 0..=3) {
                match stop(&value.text, maps) {
                    Some(stop) => stops.push(stop),
                    None => notes.push(file, value.line, format!("stop_{k} is invalid; dropped")),
                }
            }
            if !(2..=4).contains(&stops.len()) {
                notes.push(
                    file,
                    section.line,
                    format!(
                        "[{}] has {} valid stops, not 2-4; dropped",
                        section.name,
                        stops.len()
                    ),
                );
                continue;
            }
            let mut doors = Vec::new();
            if let Some(value) = section.get("doors") {
                for item in text::list(&value.text) {
                    match text::integer(&item).filter(|pid| pid >> 24 == 2) {
                        Some(pid) => doors.push(pid as u32),
                        None => notes.push(file, value.line, format!("doors: {item:?} dropped")),
                    }
                }
            }
            elevators.insert(
                number,
                Elevator {
                    number,
                    background,
                    panel,
                    stops,
                    doors,
                },
            );
        }
        Ok(Self { elevators })
    }
    /// The record of `metarule` rule 15's operand, if any.
    pub fn get(&self, kind: i32) -> Option<&Elevator> {
        u32::try_from(kind)
            .ok()
            .and_then(|kind| self.elevators.get(&kind))
    }
}

fn stop(value: &str, maps: &MapsTable) -> Option<Stop> {
    let items = text::list(value);
    let [map, elevation, tile, key] = items.as_slice() else {
        return None;
    };
    let map = text::integer_in(map, 0..=999)?;
    maps.get(map)?;
    let key = key.as_bytes();
    if key.len() != 1 || !(0x21..=0x7e).contains(&key[0]) {
        return None;
    }
    Some(Stop {
        map: map as u32,
        elevation: text::integer_in(elevation, 0..=2)? as u8,
        tile: text::integer_in(tile, 0..=39999)?,
        key: key[0],
    })
}
