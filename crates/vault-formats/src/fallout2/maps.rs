//! SPEC-018 §4.2 `data/maps.txt`, the map table, and §5.1's map numbers
//! and name matching.
use super::text::{self, Diagnostics, SectionedText, TableError};

pub const FILE: &str = "data/maps.txt";
/// The highest map number a record may have.
pub const MAX_MAP: u32 = 999;

/// One random start point (§4.2 `random_start_point_N`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartPoint {
    pub elevation: u8,
    pub tile: i32,
}

/// One `[Map NNN]` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapRecord {
    pub number: u32,
    /// The stem as written (its case kept).
    pub stem: String,
    pub lookup_name: Option<String>,
    pub music: Option<String>,
    /// `ambient_sfx` choices; parsed, with no effect in this revision.
    pub ambient: Vec<(String, i32)>,
    pub saved: bool,
    pub dead_bodies_age: bool,
    /// Per elevation 0, 1, 2.
    pub can_rest: [bool; 3],
    pub pipboy_active: bool,
    /// The valid start points in ascending N, renumbered from 0.
    pub start_points: Vec<StartPoint>,
}

impl MapRecord {
    /// The canonical lower-case path of the map file.
    pub fn path(&self) -> String {
        format!("maps/{}.map", self.stem.to_ascii_lowercase())
    }
    /// §5.1 the match text: the stem upper-cased, then `.MAP`.
    pub fn match_text(&self) -> String {
        format!("{}.MAP", self.stem.to_ascii_uppercase())
    }
}

/// The parsed map table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapsTable {
    /// Index = map number, `0..M_T`; `None` is an absent map.
    records: Vec<Option<MapRecord>>,
}

impl MapsTable {
    pub fn parse(bytes: &[u8], notes: &mut Diagnostics) -> Result<Self, TableError> {
        let file = FILE;
        let table = SectionedText::parse(file, bytes, notes)?;
        let mut records: Vec<Option<MapRecord>> = Vec::new();
        for (number, section) in table.numbered("map") {
            if number > MAX_MAP {
                notes.push(file, section.line, format!("map {number} is outside 0-999"));
                continue;
            }
            let Some(stem) = section
                .get("map_name")
                .and_then(|value| text::stem(&value.text))
            else {
                notes.push(
                    file,
                    section.line,
                    format!("[{}] has no valid map_name; dropped", section.name),
                );
                continue;
            };
            let optional_bool = |key: &str, notes: &mut Diagnostics| -> bool {
                match section.get(key) {
                    None => true,
                    Some(value) => text::boolean(&value.text).unwrap_or_else(|| {
                        notes.push(file, value.line, format!("{key}: not a Boolean; Yes"));
                        true
                    }),
                }
            };
            let lookup_name = section.get("lookup_name").and_then(|value| {
                let name = text::name(&value.text);
                if name.is_none() {
                    notes.push(file, value.line, "lookup_name: not a Name; none");
                }
                name
            });
            let music = section.get("music").and_then(|value| {
                if value.text.is_empty() {
                    return None;
                }
                let stem = text::stem(&value.text);
                if stem.is_none() {
                    notes.push(file, value.line, "music: not a Stem; none");
                }
                stem
            });
            let mut ambient = Vec::new();
            if let Some(value) = section.get("ambient_sfx") {
                for item in text::list(&value.text) {
                    let parsed = item.split_once(':').and_then(|(sound, weight)| {
                        Some((
                            text::stem(sound.trim())?,
                            text::integer_in(weight.trim(), 0..=100)?,
                        ))
                    });
                    match parsed {
                        Some(choice) => ambient.push(choice),
                        None => {
                            notes.push(file, value.line, format!("ambient_sfx: {item:?} dropped"))
                        }
                    }
                }
            }
            let can_rest = match section.get("can_rest_here") {
                None => [true; 3],
                Some(value) => {
                    let items: Vec<Option<bool>> = text::list(&value.text)
                        .iter()
                        .map(|item| text::boolean(item))
                        .collect();
                    match items.as_slice() {
                        [Some(a), Some(b), Some(c)] => [*a, *b, *c],
                        _ => {
                            notes.push(
                                file,
                                value.line,
                                "can_rest_here: not 3 Booleans; Yes,Yes,Yes",
                            );
                            [true; 3]
                        }
                    }
                }
            };
            let mut start_points = Vec::new();
            for (n, value) in section.indexed("random_start_point", 0..=99) {
                match start_point(&value.text) {
                    Some(point) => start_points.push(point),
                    None => notes.push(
                        file,
                        value.line,
                        format!("random_start_point_{n} is invalid; dropped"),
                    ),
                }
            }
            let record = MapRecord {
                number,
                stem,
                lookup_name,
                music,
                ambient,
                saved: optional_bool("saved", notes),
                dead_bodies_age: optional_bool("dead_bodies_age", notes),
                can_rest,
                pipboy_active: optional_bool("pipboy_active", notes),
                start_points,
            };
            let index = number as usize;
            if records.len() <= index {
                records.resize(index + 1, None);
            }
            records[index] = Some(record);
        }
        if records.iter().all(Option::is_none) {
            return Err(TableError::new(file, 0, "no valid map record"));
        }
        let out = Self { records };
        // §4.2 duplicate stems and lookup names exist; lookups take the
        // lowest number.
        let mut stems = std::collections::BTreeMap::new();
        let mut names = std::collections::BTreeMap::new();
        for record in out.records() {
            if let Some(first) = stems.insert(text::fold(&record.stem), record.number) {
                stems.insert(text::fold(&record.stem), first);
                notes.push(
                    file,
                    0,
                    format!(
                        "map {} repeats map {first}'s file {}",
                        record.number, record.stem
                    ),
                );
            }
            if let Some(name) = &record.lookup_name
                && let Some(first) = names.insert(text::fold(name), record.number)
            {
                names.insert(text::fold(name), first);
                notes.push(
                    file,
                    0,
                    format!(
                        "map {} repeats map {first}'s lookup name {name}",
                        record.number
                    ),
                );
            }
        }
        Ok(out)
    }
    /// `M_T`: the highest record number plus 1.
    pub fn count(&self) -> usize {
        self.records.len()
    }
    /// The record of map number `number`, if present.
    pub fn get(&self, number: i32) -> Option<&MapRecord> {
        usize::try_from(number)
            .ok()
            .and_then(|number| self.records.get(number))
            .and_then(Option::as_ref)
    }
    /// The present records, in ascending number.
    pub fn records(&self) -> impl Iterator<Item = &MapRecord> {
        self.records.iter().flatten()
    }
    /// The lowest map whose `lookup_name` is `name` (case folded).
    pub fn by_lookup_name(&self, name: &str) -> Option<u32> {
        self.records()
            .find(|record| {
                record
                    .lookup_name
                    .as_deref()
                    .is_some_and(|own| own.eq_ignore_ascii_case(name))
            })
            .map(|record| record.number)
    }
    /// The lowest map whose match text is exactly `text`.
    fn by_match_text(&self, text: &[u8]) -> i32 {
        self.records()
            .find(|record| record.match_text().as_bytes() == text)
            .map_or(-1, |record| record.number as i32)
    }
    /// §5.1 matching a map name (`load_map` with a string): uppercased,
    /// its first `.SAV` read as `.MAP`, compared exactly with the match
    /// texts; a string of 16 bytes or more matches nothing.
    pub fn match_name(&self, name: &[u8]) -> i32 {
        let end = name.iter().position(|b| *b == 0).unwrap_or(name.len());
        if end >= 16 {
            return -1;
        }
        self.by_match_text(&canonical(&name[..end]))
    }
    /// §5.1 assigning the current map number after a map stream loads: as
    /// SPEC-008 §3.10, a header name containing the lower-case `.sav`, or
    /// exactly `TMP$MAP#.MAP`, keeps the stored word; any other is matched
    /// with the records' match texts in ascending number.
    pub fn header_number(&self, name: &[u8], stored: i32) -> i32 {
        let end = name.iter().position(|b| *b == 0).unwrap_or(name.len());
        let name = &name[..end];
        if name.windows(4).any(|window| window == b".sav") || name == b"TMP$MAP#.MAP" {
            return stored;
        }
        self.by_match_text(&canonical(name))
    }
    /// §5.2 the excluded set: `saved=No`, an absent number, or -1.
    pub fn excluded(&self, number: i32) -> bool {
        self.get(number).is_none_or(|record| !record.saved)
    }
}

/// Uppercased, with its first `.SAV` replaced by `.MAP`.
fn canonical(name: &[u8]) -> Vec<u8> {
    let mut upper = name.to_ascii_uppercase();
    if let Some(at) = upper.windows(4).position(|window| window == b".SAV") {
        upper[at..at + 4].copy_from_slice(b".MAP");
    }
    upper
}

/// `elev:E, tile_num:T` in either order.
fn start_point(value: &str) -> Option<StartPoint> {
    let items = text::list(value);
    if items.len() != 2 {
        return None;
    }
    let (mut elevation, mut tile) = (None, None);
    for item in &items {
        let (keyword, value) = text::field(item)?;
        match keyword.as_str() {
            "elev" if elevation.is_none() => elevation = Some(text::integer_in(value, 0..=2)?),
            "tile_num" if tile.is_none() => tile = Some(text::integer_in(value, 0..=39999)?),
            _ => return None,
        }
    }
    Some(StartPoint {
        elevation: elevation? as u8,
        tile: tile?,
    })
}
