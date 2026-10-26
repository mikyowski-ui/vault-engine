//! SPEC-018 §4.3 `data/city.txt`: areas (SPEC-019's cities) and their
//! entrances.
use super::{
    maps::MapsTable,
    text::{self, Diagnostics, SectionedText, TableError},
};

pub const FILE: &str = "data/city.txt";
pub const MAX_AREA: u32 = 999;
/// Entrance slots 0-31.
pub const MAX_ENTRANCES: usize = 32;

/// An area's circle on the world map (SPEC-019 §2.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AreaSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl AreaSize {
    /// SPEC-019 §2.6 the circle radius in world pixels.
    pub fn radius(self) -> i32 {
        match self {
            Self::Small => 7,
            Self::Medium => 13,
            Self::Large => 20,
        }
    }
}

/// One entrance slot's seven items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entrance {
    pub known: bool,
    /// The marker on the town map picture; `None` (stored as -1, -1) is an
    /// entrance with no marker, never drawn (Revision 2, R12).
    pub marker: Option<[i32; 2]>,
    /// The destination's map number (from its lookup name).
    pub map: u32,
    /// -1 takes the header's elevation.
    pub elevation: i32,
    /// -1 takes the header's whole default entrance.
    pub tile: i32,
    /// -1 takes the header's orientation.
    pub orientation: i32,
}

/// One `[Area NN]` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Area {
    pub number: u32,
    /// The display name (single-byte text, possibly empty).
    pub name: String,
    pub world_pos: [i32; 2],
    pub start_known: bool,
    pub start_locked: bool,
    pub size: AreaSize,
    pub townmap_art: i32,
    pub townmap_label_art: i32,
    /// `E` slots; `None` is an absent slot.
    pub entrances: Vec<Option<Entrance>>,
}

/// The parsed area table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CityTable {
    /// Index = area number, `0..A`; `None` is an absent area.
    areas: Vec<Option<Area>>,
}

impl CityTable {
    /// `world` is the world's size in pixels (§5.3), which bounds
    /// `world_pos`.
    pub fn parse(
        bytes: &[u8],
        maps: &MapsTable,
        world: [i32; 2],
        notes: &mut Diagnostics,
    ) -> Result<Self, TableError> {
        let file = FILE;
        let table = SectionedText::parse(file, bytes, notes)?;
        let mut areas: Vec<Option<Area>> = Vec::new();
        for (number, section) in table.numbered("area") {
            if number > MAX_AREA {
                notes.push(
                    file,
                    section.line,
                    format!("area {number} is outside 0-999"),
                );
                continue;
            }
            let position = section.get("world_pos").and_then(|value| {
                let items = text::list(&value.text);
                let [x, y] = items.as_slice() else {
                    return None;
                };
                let x = text::integer_in(x, 0..=world[0] - 1)?;
                let y = text::integer_in(y, 0..=world[1] - 1)?;
                Some([x, y])
            });
            let Some(world_pos) = position else {
                notes.push(
                    file,
                    section.line,
                    format!("[{}] has no valid world_pos; dropped", section.name),
                );
                continue;
            };
            let flag = |key: &str, notes: &mut Diagnostics| match section.get(key) {
                None => false,
                Some(value) => text::boolean(&value.text).unwrap_or_else(|| {
                    notes.push(file, value.line, format!("{key}: not a Boolean; Off"));
                    false
                }),
            };
            let art = |key: &str, notes: &mut Diagnostics| match section.get(key) {
                None => -1,
                Some(value) => text::integer_in(&value.text, -1..=i32::MAX).unwrap_or_else(|| {
                    notes.push(file, value.line, format!("{key}: invalid; -1"));
                    -1
                }),
            };
            let size = match section.get("size") {
                None => AreaSize::Medium,
                Some(value) => match text::fold(&value.text).as_str() {
                    "small" => AreaSize::Small,
                    "medium" => AreaSize::Medium,
                    "large" => AreaSize::Large,
                    _ => {
                        notes.push(file, value.line, "size: not Small, Medium or Large; Medium");
                        AreaSize::Medium
                    }
                },
            };
            let mut entrances: Vec<Option<Entrance>> = Vec::new();
            for (slot, value) in section.indexed("entrance", 0..=(MAX_ENTRANCES as u32 - 1)) {
                // Revision 3 (R16): an eighth item is ignored, with a
                // diagnostic.
                if text::list(&value.text).len() == 8 {
                    notes.push(
                        file,
                        value.line,
                        format!(
                            "[{}] entrance_{slot} has 8 items; read from the first seven",
                            section.name
                        ),
                    );
                }
                match entrance(&value.text, maps) {
                    Some(entrance) => {
                        let slot = slot as usize;
                        if entrances.len() <= slot {
                            entrances.resize(slot + 1, None);
                        }
                        entrances[slot] = Some(entrance);
                    }
                    None => notes.push(
                        file,
                        value.line,
                        format!("[{}] entrance_{slot} is invalid; dropped", section.name),
                    ),
                }
            }
            let area = Area {
                number,
                name: section
                    .get("area_name")
                    .map(|value| value.text.clone())
                    .unwrap_or_default(),
                world_pos,
                start_known: flag("start_state", notes),
                start_locked: flag("lock_state", notes),
                size,
                townmap_art: art("townmap_art_idx", notes),
                townmap_label_art: art("townmap_label_art_idx", notes),
                entrances,
            };
            let index = number as usize;
            if areas.len() <= index {
                areas.resize(index + 1, None);
            }
            areas[index] = Some(area);
        }
        if areas.iter().all(Option::is_none) {
            return Err(TableError::new(file, 0, "no valid area"));
        }
        Ok(Self { areas })
    }
    /// `A`: the highest valid area number plus 1.
    pub fn count(&self) -> usize {
        self.areas.len()
    }
    pub fn get(&self, number: i32) -> Option<&Area> {
        usize::try_from(number)
            .ok()
            .and_then(|number| self.areas.get(number))
            .and_then(Option::as_ref)
    }
    /// The valid areas, in ascending number.
    pub fn areas(&self) -> impl Iterator<Item = &Area> {
        self.areas.iter().flatten()
    }
    /// The entrance count `E` of area `number` (0 for an absent area).
    pub fn entrance_count(&self, number: usize) -> usize {
        self.areas
            .get(number)
            .and_then(Option::as_ref)
            .map_or(0, |area| area.entrances.len())
    }
    /// §5.2 the area of map `map`: the lowest-numbered valid area with a
    /// valid entrance naming it, in any slot; -1 for none and for -1.
    pub fn area_of_map(&self, map: i32) -> i32 {
        let Ok(map) = u32::try_from(map) else {
            return -1;
        };
        self.areas()
            .find(|area| area.entrances.iter().flatten().any(|e| e.map == map))
            .map_or(-1, |area| area.number as i32)
    }
    /// §5.8 entrances by map (`metarule3` 104): every valid slot, in every
    /// area, whose destination is `map` and where `elevation` is -1, or
    /// item 5 equals it, or item 5 is -1. `(area, slot)` pairs in order.
    pub fn entrances_by_map(&self, map: i32, elevation: i32) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for area in self.areas() {
            for (slot, entrance) in area.entrances.iter().enumerate() {
                let Some(entrance) = entrance else { continue };
                if i64::from(entrance.map) == i64::from(map)
                    && (elevation == -1
                        || entrance.elevation == elevation
                        || entrance.elevation == -1)
                {
                    out.push((area.number as usize, slot));
                }
            }
        }
        out
    }
}

/// The seven entrance items; of eight, the first seven (Revision 3, R16).
fn entrance(value: &str, maps: &MapsTable) -> Option<Entrance> {
    let items = text::list(value);
    let ([known, x, y, name, elevation, tile, orientation]
    | [known, x, y, name, elevation, tile, orientation, _]) = items.as_slice()
    else {
        return None;
    };
    // Revision 2 (R12): -1, -1 is an entrance with no marker; only one of
    // the two at -1 is invalid.
    let marker = match [text::integer(x)?, text::integer(y)?] {
        [-1, -1] => None,
        marker if marker.iter().all(|value| (0..=1023).contains(value)) => Some(marker),
        _ => return None,
    };
    Some(Entrance {
        known: text::boolean(known)?,
        marker,
        map: maps.by_lookup_name(&text::name(name)?)?,
        elevation: text::integer_in(elevation, -1..=2)?,
        tile: text::integer_in(tile, -1..=39999)?,
        orientation: text::integer_in(orientation, -1..=5)?,
    })
}
