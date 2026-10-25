//! SPEC-018 §7.4 the Fallout 2 world-map component (save order 20) and
//! §7.5 the party component (order 24). `[Engine layout]`: all words
//! big-endian.
use crate::{
    ParseError,
    binary::{Reader, put_i32, put_u32},
};

/// SPEC-019 §2.6 the car's fuel maximum.
pub const FUEL_MAX: i32 = 80000;
/// SPEC-019 §2.6 the party list's capacity, the player included.
pub const PARTY_CAPACITY: i32 = 32;
/// §7.5 the most retained records a load accepts.
pub const MAX_RETAINED: usize = 4096;

/// What the installed tables fix in a slot's shape; a slot from other
/// installed data is refused.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Fallout2Shape {
    /// `C` and `R`.
    pub grid: [usize; 2],
    /// `E` per area, `A` entries.
    pub entrances: Vec<usize>,
    /// Whether each area number below `A` is a valid area.
    pub areas_present: Vec<bool>,
    /// `M_T`.
    pub maps: usize,
    /// Whether each map number below `M_T` is present.
    pub maps_present: Vec<bool>,
    /// Per `party.txt` record below `P_T`: its level-PID count, `None` for
    /// an absent record.
    pub party_levels: Vec<Option<usize>>,
}

/// One area's saved state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AreaSave {
    /// 0 unknown, 1 known, 2 visited.
    pub known: u8,
    pub locked: u8,
    /// The current world position (SPEC-018 §5.8).
    pub position: [i32; 2],
    /// Per entrance slot: 0 or 1.
    pub entrances: Vec<u8>,
}

/// The §7.4 component.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorldMapSave2 {
    pub position: [i32; 2],
    pub area: i32,
    pub entrance: i32,
    pub has_car: i32,
    pub car_area: i32,
    pub car_map: i32,
    pub car_fuel: i32,
    pub grid: [u16; 2],
    /// `R` rows of `C` bytes, row-major.
    pub discovery: Vec<u8>,
    pub areas: Vec<AreaSave>,
    /// Per map number: bit `0x01` known, `0x02` visited.
    pub map_flags: Vec<u8>,
    /// (table, entry, remaining).
    pub counters: Vec<[i32; 3]>,
}

fn invalid(r: &Reader<'_>, message: impl Into<String>) -> ParseError {
    r.error(message)
}

impl WorldMapSave2 {
    pub fn write(&self, out: &mut Vec<u8>) {
        let mut body = Vec::new();
        for value in [
            self.position[0],
            self.position[1],
            self.area,
            self.entrance,
            self.has_car,
            self.car_area,
            self.car_map,
            self.car_fuel,
        ] {
            put_i32(&mut body, value);
        }
        body.extend(self.grid[0].to_be_bytes());
        body.extend(self.grid[1].to_be_bytes());
        body.extend(&self.discovery);
        put_u32(&mut body, self.areas.len() as u32);
        for area in &self.areas {
            body.push(area.known);
            body.push(area.locked);
            put_i32(&mut body, area.position[0]);
            put_i32(&mut body, area.position[1]);
            body.push(area.entrances.len() as u8);
            body.extend(&area.entrances);
        }
        put_u32(&mut body, self.map_flags.len() as u32);
        body.extend(&self.map_flags);
        put_u32(&mut body, self.counters.len() as u32);
        for counter in &self.counters {
            for value in counter {
                put_i32(&mut body, *value);
            }
        }
        put_u32(out, body.len() as u32);
        out.extend(body);
    }
    /// §7.4 reading, with every check that refuses the load.
    pub(crate) fn read(r: &mut Reader<'_>, shape: &Fallout2Shape) -> Result<Self, ParseError> {
        let length = r.word()? as usize;
        let start = r.pos;
        let mut words = [0i32; 8];
        for word in &mut words {
            *word = r.be_i32()?;
        }
        let [x, y, area, entrance, has_car, car_area, car_map, car_fuel] = words;
        let grid = [r.be_u16()?, r.be_u16()?];
        if [usize::from(grid[0]), usize::from(grid[1])] != shape.grid {
            return Err(invalid(
                r,
                format!(
                    "the grid is {} by {}, not the installed {} by {}",
                    grid[0], grid[1], shape.grid[0], shape.grid[1]
                ),
            ));
        }
        let discovery = r.take(shape.grid[0] * shape.grid[1])?.to_vec();
        if discovery.iter().any(|value| *value > 2) {
            return Err(invalid(r, "a discovery byte outside 0-2"));
        }
        let count = r.word()? as usize;
        if count != shape.entrances.len() {
            return Err(invalid(
                r,
                format!("{count} areas, not the installed {}", shape.entrances.len()),
            ));
        }
        let mut areas = Vec::with_capacity(count);
        for expected in &shape.entrances {
            let known = r.take(1)?[0];
            let locked = r.take(1)?[0];
            if known > 2 || locked > 1 {
                return Err(invalid(r, "an area state outside its range"));
            }
            let position = [r.be_i32()?, r.be_i32()?];
            let entrances_count = usize::from(r.take(1)?[0]);
            if entrances_count != *expected {
                return Err(invalid(
                    r,
                    format!(
                        "an area with {entrances_count} entrances, not the installed {expected}"
                    ),
                ));
            }
            let entrances = r.take(entrances_count)?.to_vec();
            if entrances.iter().any(|value| *value > 1) {
                return Err(invalid(r, "an entrance state outside 0-1"));
            }
            areas.push(AreaSave {
                known,
                locked,
                position,
                entrances,
            });
        }
        let maps = r.word()? as usize;
        if maps != shape.maps {
            return Err(invalid(
                r,
                format!("{maps} map flags, not the installed {}", shape.maps),
            ));
        }
        let map_flags = r.take(maps)?.to_vec();
        if map_flags.iter().any(|flags| flags & !0x03 != 0) {
            return Err(invalid(r, "a map flag byte with bits other than 0x03"));
        }
        let counters_count = r.word()? as usize;
        if counters_count > 100_000 {
            return Err(invalid(r, "too many counter records"));
        }
        let mut counters = Vec::with_capacity(counters_count);
        for _ in 0..counters_count {
            let record = [r.be_i32()?, r.be_i32()?, r.be_i32()?];
            if !(-1..=1000).contains(&record[2]) {
                return Err(invalid(r, "a remaining count outside -1 to 1000"));
            }
            counters.push(record);
        }
        if r.pos - start != length {
            return Err(invalid(
                r,
                format!(
                    "the component's length word is {length}, but its fields take {}",
                    r.pos - start
                ),
            ));
        }
        if !matches!(has_car, 0 | 1) {
            return Err(invalid(r, format!("has_car {has_car}")));
        }
        if !(0..=FUEL_MAX).contains(&car_fuel) {
            return Err(invalid(r, format!("car_fuel {car_fuel} outside 0-80000")));
        }
        let area_ok = |area: i32| {
            area == -1
                || usize::try_from(area)
                    .ok()
                    .and_then(|area| shape.areas_present.get(area))
                    .copied()
                    .unwrap_or(false)
        };
        let map_ok = |map: i32| {
            map == -1
                || usize::try_from(map)
                    .ok()
                    .and_then(|map| shape.maps_present.get(map))
                    .copied()
                    .unwrap_or(false)
        };
        let entrance_ok = entrance == -1
            || (area >= 0
                && usize::try_from(entrance)
                    .is_ok_and(|slot| slot < shape.entrances[area as usize]));
        if !area_ok(area) || !entrance_ok || !area_ok(car_area) || !map_ok(car_map) {
            return Err(invalid(
                r,
                "an area, entrance or map outside the installed tables",
            ));
        }
        Ok(Self {
            position: [x, y],
            area,
            entrance,
            has_car,
            car_area,
            car_map,
            car_fuel,
            grid,
            discovery,
            areas,
            map_flags,
            counters,
        })
    }
}

/// A §7.5 member record: nine signed words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemberRecord {
    /// The `party.txt` record, or -1 without a control record.
    pub record: i32,
    pub stage: i32,
    /// The seven control fields in SPEC-019 §2.5's order.
    pub fields: [i32; 7],
}

impl MemberRecord {
    /// A member without a control record: words 1-8 are -1.
    pub const NONE: Self = Self {
        record: -1,
        stage: -1,
        fields: [-1; 7],
    };
    fn write(&self, out: &mut Vec<u8>) {
        put_i32(out, self.record);
        put_i32(out, self.stage);
        for field in self.fields {
            put_i32(out, field);
        }
    }
    fn read(r: &mut Reader<'_>, shape: &Fallout2Shape) -> Result<Self, ParseError> {
        let words: [i32; 9] = r.words()?;
        let record = words[0];
        if record == -1 {
            return Ok(Self::NONE);
        }
        let levels = usize::try_from(record)
            .ok()
            .and_then(|record| shape.party_levels.get(record))
            .copied()
            .flatten()
            .ok_or_else(|| r.error(format!("party record {record} is not installed")))?;
        let stage = words[1];
        if usize::try_from(stage).map_or(true, |stage| stage > levels) {
            return Err(r.error(format!("level stage {stage} outside 0-{levels}")));
        }
        Ok(Self {
            record,
            stage,
            fields: words[2..].try_into().expect("seven fields"),
        })
    }
}

/// The §7.5 component: SPEC-008's order 24 and its extension.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PartySave2 {
    /// P, the player included.
    pub count: i32,
    pub next_item_id: i32,
    /// `max(P - 1, 0)` identifiers.
    pub members: Vec<i32>,
    /// One per member.
    pub records: Vec<MemberRecord>,
    /// (identifier, record), ascending identifiers.
    pub retained: Vec<(i32, MemberRecord)>,
}

impl PartySave2 {
    pub fn write(&self, out: &mut Vec<u8>) -> Result<(), ParseError> {
        let error = |message: &str| ParseError::new("SAVE", 0, message);
        if self.members.len() != (self.count - 1).max(0) as usize
            || self.records.len() != self.members.len()
        {
            return Err(error(
                "party identifiers and records must number max(P - 1, 0)",
            ));
        }
        put_i32(out, self.count);
        put_i32(out, self.next_item_id);
        for id in &self.members {
            put_i32(out, *id);
        }
        for record in &self.records {
            record.write(out);
        }
        put_u32(out, self.retained.len() as u32);
        for (id, record) in &self.retained {
            put_i32(out, *id);
            record.write(out);
        }
        Ok(())
    }
    pub(crate) fn read(r: &mut Reader<'_>, shape: &Fallout2Shape) -> Result<Self, ParseError> {
        let count = r.be_i32()?;
        let next_item_id = r.be_i32()?;
        if !(1..=PARTY_CAPACITY).contains(&count) {
            return Err(r.error(format!("party count {count} outside 1-32")));
        }
        let members = (0..count - 1)
            .map(|_| r.be_i32())
            .collect::<Result<Vec<_>, _>>()?;
        let records = (0..count - 1)
            .map(|_| MemberRecord::read(r, shape))
            .collect::<Result<Vec<_>, _>>()?;
        let retained_count = r.word()? as usize;
        if retained_count > MAX_RETAINED {
            return Err(r.error(format!("{retained_count} retained records, over 4096")));
        }
        let mut retained = Vec::with_capacity(retained_count);
        for _ in 0..retained_count {
            let id = r.be_i32()?;
            retained.push((id, MemberRecord::read(r, shape)?));
        }
        Ok(Self {
            count,
            next_item_id,
            members,
            records,
            retained,
        })
    }
}
