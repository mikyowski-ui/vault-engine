//! SPEC-018: the Fallout 2 install, its data tables, and the Fallout 2
//! save layout's components.
//!
//! - [`text`]: §3.3-§3.5 the table grammar, value forms and severities;
//! - [`maps`], [`city`], [`worldmap`], [`party`]: the four required tables
//!   (§4.2-§4.5);
//! - [`lines`]: §4.6 the line tables; [`elevators`]: §4.9;
//! - [`install`]: §2's archive discovery, mounting and required members;
//! - [`save`]: §7's world-map and party components.
pub mod city;
pub mod elevators;
pub mod install;
pub mod lines;
pub mod maps;
pub mod party;
pub mod save;
pub mod text;
pub mod worldmap;

pub use install::{Fallout2Install, OpenedInstall, REQUIRED_MEMBERS, missing_members};
pub use text::{Diagnostic, Diagnostics, TableError};

use std::collections::BTreeMap;

/// Every SPEC-018 table of an install, parsed once per Play or load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fallout2Tables {
    pub maps: maps::MapsTable,
    pub city: city::CityTable,
    pub worldmap: worldmap::WorldmapTable,
    pub party: party::PartyTable,
    pub quests: Vec<lines::Quest>,
    pub holodisks: Vec<lines::Holodisk>,
    pub genrep: Vec<lines::Reputation>,
    pub karmavar: Vec<lines::KarmaVar>,
    pub endgame: Vec<lines::EndgameSlide>,
    pub elevators: elevators::ElevatorTable,
    /// The tiles' walk masks that loaded, by folded stem.
    pub masks: BTreeMap<String, worldmap::WalkMask>,
    /// Everything dropped or defaulted (§3.5); never fatal.
    pub diagnostics: Vec<Diagnostic>,
}

/// A table reader: `Ok(None)` for an absent file, `Err` for one that could
/// not be read.
pub type Reader<'a> = dyn FnMut(&str) -> Result<Option<Vec<u8>>, String> + 'a;

impl Fallout2Tables {
    /// Parse every table through `read` (the master database as mounted):
    /// the four required tables of §2.2 refuse Play when absent or refused;
    /// the line tables and the elevator table are optional.
    pub fn load(read: &mut Reader<'_>) -> Result<Self, TableError> {
        let mut notes = Diagnostics::default();
        let mut fetch = |file: &str, required: bool| -> Result<Option<Vec<u8>>, TableError> {
            match read(file) {
                Ok(Some(bytes)) => Ok(Some(bytes)),
                Ok(None) if required => Err(TableError::new(file, 0, "missing")),
                Ok(None) => Ok(None),
                Err(error) => Err(TableError::new(file, 0, error)),
            }
        };
        let required = |bytes: Option<Vec<u8>>| bytes.expect("required tables are present");
        let maps_bytes = required(fetch(maps::FILE, true)?);
        let worldmap_bytes = required(fetch(worldmap::FILE, true)?);
        let city_bytes = required(fetch(city::FILE, true)?);
        let party_bytes = required(fetch(party::FILE, true)?);
        let maps = maps::MapsTable::parse(&maps_bytes, &mut notes)?;
        let worldmap = worldmap::WorldmapTable::parse(&worldmap_bytes, &maps, &mut notes)?;
        let city = city::CityTable::parse(&city_bytes, &maps, worldmap.world_size(), &mut notes)?;
        let party = party::PartyTable::parse(&party_bytes, &mut notes)?;
        // An absent optional table is an empty one.
        let mut optional = |file: &str| -> Result<Vec<u8>, TableError> {
            Ok(fetch(file, false)?.unwrap_or_default())
        };
        let quests = lines::quests(&optional(lines::QUESTS)?, &mut notes)?;
        let holodisks = lines::holodisks(&optional(lines::HOLODISKS)?, &mut notes)?;
        let genrep = lines::genrep(&optional(lines::GENREP)?, &mut notes)?;
        let karmavar = lines::karmavar(&optional(lines::KARMAVAR)?, &mut notes)?;
        let endgame = lines::endgame(&optional(lines::ENDGAME)?, &mut notes)?;
        let elevators =
            elevators::ElevatorTable::parse(&optional(elevators::FILE)?, &maps, &mut notes)?;
        let mut masks = BTreeMap::new();
        for stem in worldmap.mask_names() {
            let path = format!("data/{}.msk", stem.to_ascii_lowercase());
            match read(&path) {
                Ok(Some(bytes)) => match worldmap::WalkMask::parse(bytes) {
                    Some(mask) => {
                        masks.insert(text::fold(&stem), mask);
                    }
                    None => notes.push(&path, 0, "a walk mask is 13200 bytes; it blocks nothing"),
                },
                Ok(None) => notes.push(&path, 0, "missing; the walk mask blocks nothing"),
                Err(error) => {
                    notes.push(&path, 0, format!("{error}; the walk mask blocks nothing"))
                }
            }
        }
        Ok(Self {
            maps,
            city,
            worldmap,
            party,
            quests,
            holodisks,
            genrep,
            karmavar,
            endgame,
            elevators,
            masks,
            diagnostics: notes.0,
        })
    }
    /// The walk mask of tile `tile`, when it has one that loaded.
    pub fn mask(&self, tile: usize) -> Option<&worldmap::WalkMask> {
        let stem = self.worldmap.tiles.get(tile)?.walk_mask.as_ref()?;
        self.masks.get(&text::fold(stem))
    }
}

#[cfg(test)]
mod tests;
