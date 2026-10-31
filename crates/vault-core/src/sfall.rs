//! SPEC-024b: what sfall mode adds to the world. It exists only with the
//! mode on ([`crate::WorldState::sfall`]); with it off none of this does,
//! and nothing here reaches SPEC-021's `state_hash`.
use crate::state_hash::fnv1a64;
use std::{collections::BTreeMap, ops::RangeInclusive};
pub use vault_vm::sfall::SfallValue;
use vault_vm::sfall_arrays::{self, ArrayStore, Header, SidecarError};

/// The save sidecar, beside the slot's `SAVE.DAT` (§5.8).
pub const SIDECAR: &str = "VAULTSFL.DAT";

/// §4.11 the object IDs `set_unique_id` assigns.
pub const UNIQUE_IDS: RangeInclusive<u32> = 0x1000_0000..=0x7fff_ffff;

/// §4.11 a forced encounter's flags.
pub mod encounter_flag {
    pub const NO_CAR: u32 = 0x1;
    pub const LOCK: u32 = 0x2;
    pub const NO_ICON: u32 = 0x4;
    pub const ICON_SP: u32 = 0x8;
    pub const FADEOUT: u32 = 0x10;
}

/// The maxima `set_pc_stat_max` set: [`NO_STAT_MAX`] leaves a stat's own.
pub type StatMaxima = [i32; 35];
pub const NO_STAT_MAX: i32 = i32::MIN;

/// §4.12 the session settings. They start at their defaults on a new game and
/// on each load, are not saved, and are outside `sfall_hash` (Decision D-48).
/// The `Default` is the engine with the mode off, so nothing here changes a
/// formula then.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionSettings {
    pub skill_cap: i32,
    pub stat_max: Option<StatMaxima>,
    /// `set_map_time_multi`, kept as binary32.
    pub map_time: f32,
    pub level_points: i32,
    /// `set_perk_freq`: 0 for the default.
    pub perk_interval: i32,
    pub encounter_detection: bool,
    pub npc_level_up: bool,
    pub town_names: bool,
    pub fo1_hit_chance: bool,
    /// The reaction bands: neutral above the first, good above the second.
    pub reaction: [i32; 2],
    /// `set_terrain_name`, by cell.
    pub terrain_names: BTreeMap<[usize; 2], Vec<u8>>,
    pub rest_mode: i32,
    pub car_art: i32,
    /// `set_cursor_mode`, waiting for the shell.
    pub cursor_mode: Option<i32>,
}
impl Default for SessionSettings {
    fn default() -> Self {
        Self {
            skill_cap: 200,
            stat_max: None,
            map_time: 1.0,
            level_points: 0,
            perk_interval: 0,
            encounter_detection: true,
            npc_level_up: true,
            town_names: true,
            fo1_hit_chance: false,
            reaction: [25, 75],
            terrain_names: BTreeMap::new(),
            rest_mode: 0,
            car_art: 433,
            cursor_mode: None,
        }
    }
}
impl SessionSettings {
    /// §4.12's defaults, which sfall mode starts from.
    pub fn sfall() -> Self {
        Self {
            skill_cap: 300,
            reaction: [-10, 10],
            ..Self::default()
        }
    }
}

/// A sfall global's key (§4.9): any int, or a string of exactly 8 bytes.
/// Numbers order before strings, as §5.6 asks of the sidecar.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum GlobalKey {
    Int(i32),
    Text([u8; 8]),
}
impl GlobalKey {
    /// The key of a class-`K` operand; a string of another length is a
    /// failed operand.
    pub fn of(value: &SfallValue) -> Option<Self> {
        match value {
            SfallValue::Int(key) => Some(Self::Int(*key)),
            SfallValue::Text(text) => text.as_slice().try_into().ok().map(Self::Text),
            SfallValue::Float(_) => None,
        }
    }
    fn value(&self) -> SfallValue {
        match self {
            Self::Int(key) => SfallValue::Int(*key),
            Self::Text(text) => SfallValue::text(text.as_slice()),
        }
    }
}

/// The world's sfall state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SfallWorld {
    /// §4.9 each value keeps its type.
    pub globals: BTreeMap<GlobalKey, SfallValue>,
    /// §5: shared by all scripts.
    pub arrays: ArrayStore,
    /// §4.11 the next ID `set_unique_id` assigns.
    pub next_unique: u32,
    /// §4.11 the pending forced encounter's map, or -1 for none.
    pub forced_map: i32,
    pub forced_flags: u32,
}
impl Default for SfallWorld {
    fn default() -> Self {
        let header = Header::default();
        Self {
            globals: BTreeMap::new(),
            arrays: ArrayStore::default(),
            next_unique: header.next_unique,
            forced_map: header.forced_map,
            forced_flags: header.forced_flags,
        }
    }
}
impl SfallWorld {
    fn header(&self) -> Header {
        Header {
            next_unique: self.next_unique,
            forced_map: self.forced_map,
            forced_flags: self.forced_flags,
        }
    }
    fn global_records(&self) -> Vec<(SfallValue, SfallValue)> {
        self.globals
            .iter()
            .map(|(key, value)| (key.value(), value.clone()))
            .collect()
    }
    /// `VAULTSFL.DAT`: the globals and the saved arrays (§5.8).
    pub fn sidecar(&self) -> Vec<u8> {
        sfall_arrays::write_stream(
            sfall_arrays::SIDECAR_TAG,
            &self.arrays,
            self.header(),
            &self.global_records(),
            true,
        )
    }
    /// The extension stream: the same layout with every live array (§5.9).
    pub fn extension_stream(&self) -> Vec<u8> {
        sfall_arrays::write_stream(
            sfall_arrays::STREAM_TAG,
            &self.arrays,
            self.header(),
            &self.global_records(),
            false,
        )
    }
    /// A sidecar's state, parsed whole so that a bad file changes nothing. A
    /// revision 1 sidecar migrates: what it lacks takes its default, and the
    /// next save writes revision 2 (Decision D-32).
    pub fn from_sidecar(bytes: &[u8]) -> Result<Self, SidecarError> {
        let (arrays, header, records) = sfall_arrays::parse_sidecar(bytes)?;
        let mut globals = BTreeMap::new();
        for (key, value) in records {
            let parsed =
                GlobalKey::of(&key).ok_or_else(|| SidecarError::GlobalKey(format!("{key:?}")))?;
            globals.insert(parsed, value);
        }
        Ok(Self {
            globals,
            arrays,
            next_unique: header.next_unique,
            forced_map: header.forced_map,
            forced_flags: header.forced_flags,
        })
    }
}
impl crate::WorldState {
    /// SPEC-024b §5.9 the companion to `state_hash`: FNV-1a 64 of the
    /// extension stream. It takes `&self`, draws nothing and changes nothing,
    /// and with the mode off it is the empty stream's value (vector F3).
    pub fn sfall_hash(&self) -> u64 {
        fnv1a64(
            &self
                .sfall
                .as_deref()
                .cloned()
                .unwrap_or_default()
                .extension_stream(),
        )
    }
}
