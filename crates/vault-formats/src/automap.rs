//! SPEC-008 §2.8 automap database (`MAPS/AUTOMAP.DB`, slot `AUTOMAP.SAV`) and
//! its 200 x 200 two-bit raster.
use crate::{
    ParseError,
    binary::{put_i32, put_u32},
    map::MapObject,
    pro::BasicFlags,
};

pub const MAPS: usize = 66;
pub const RASTER_LEN: usize = 10000;
/// Version byte, length word, and the 66 x 3 offset table: the length of a
/// fresh database.
pub const EMPTY_LEN: usize = 5 + 4 * MAPS * 3;
/// Map numbers excluded on every elevation in a fresh database.
pub const EXCLUDED: [usize; 24] = [
    0, 1, 2, 19, 20, 21, 22, 23, 49, 50, 51, 52, 53, 54, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65,
];
/// Scenery prototype that never contributes to the raster.
pub const IGNORED_SCENERY: u32 = 0x02000158;
/// Display-flag masks (save payload order 17).
pub const DISPLAY_IN_GAME: u32 = 0x01;
pub const DISPLAY_HIGH_DETAIL: u32 = 0x02;
pub const DISPLAY_SCANNER: u32 = 0x04;

/// The database bytes. Entries are validated when read, so one damaged entry
/// does not hide the others.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomapDb {
    bytes: Vec<u8>,
}

fn slot(map: usize, elevation: usize) -> Option<usize> {
    (map < MAPS && elevation < 3).then(|| 5 + 4 * (3 * map + elevation))
}

fn error(at: usize, message: &str) -> ParseError {
    ParseError::new("AUTOMAP", at, message)
}

impl Default for AutomapDb {
    /// A fresh 797-byte database: excluded maps -1, others recordable (0).
    fn default() -> Self {
        let mut bytes = vec![1];
        put_u32(&mut bytes, EMPTY_LEN as u32);
        for map in 0..MAPS {
            for _ in 0..3 {
                put_i32(&mut bytes, if EXCLUDED.contains(&map) { -1 } else { 0 });
            }
        }
        Self { bytes }
    }
}

impl AutomapDb {
    /// Exactly version 1 is accepted, and the offset table must be complete.
    pub fn parse(bytes: Vec<u8>) -> Result<Self, ParseError> {
        match bytes.first() {
            Some(1) if bytes.len() >= EMPTY_LEN => Ok(Self { bytes }),
            Some(1) => Err(error(bytes.len(), "truncated automap offset table")),
            Some(_) => Err(error(0, "automap database version must be 1")),
            None => Err(error(0, "empty automap database")),
        }
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// The recorded database length word (not necessarily the actual length).
    pub fn recorded_len(&self) -> u32 {
        u32::from_be_bytes(self.bytes[1..5].try_into().expect("word"))
    }
    fn set_recorded_len(&mut self, length: u32) {
        self.bytes[1..5].copy_from_slice(&length.to_be_bytes());
    }
    /// The offset word: 0 recordable/not captured, -1 excluded, else an entry.
    pub fn offset(&self, map: usize, elevation: usize) -> Option<i32> {
        let at = slot(map, elevation)?;
        Some(i32::from_be_bytes(
            self.bytes[at..at + 4].try_into().expect("word"),
        ))
    }
    fn set_offset(&mut self, map: usize, elevation: usize, value: i32) {
        let at = slot(map, elevation).expect("valid slot");
        self.bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
    }
    /// The stored entry `(compression byte, payload)` at a positive offset.
    fn entry(&self, map: usize, elevation: usize) -> Result<Option<(u8, &[u8])>, ParseError> {
        let offset = self
            .offset(map, elevation)
            .ok_or_else(|| error(0, "map/elevation outside the offset table"))?;
        if offset <= 0 {
            return Ok(None);
        }
        let start = offset as usize;
        if start < EMPTY_LEN {
            return Err(error(start, "entry offset points into the offset table"));
        }
        let header = self
            .bytes
            .get(start..start + 5)
            .ok_or_else(|| error(start, "entry header beyond the database"))?;
        let length = u32::from_be_bytes(header[..4].try_into().expect("word")) as usize;
        let payload = self
            .bytes
            .get(start + 5..start + 5 + length)
            .ok_or_else(|| error(start, "entry payload beyond the database"))?;
        Ok(Some((header[4], payload)))
    }
    /// The expanded 10000-byte raster, or `None` when not captured/excluded.
    /// Compression byte 1 is LZSS (no outer block header, stopping at 10000
    /// output bytes); other values take the raw path. Input running out of
    /// the stored payload is an error rather than a read past the entry.
    pub fn raster(&self, map: usize, elevation: usize) -> Result<Option<Vec<u8>>, ParseError> {
        let Some((compression, payload)) = self.entry(map, elevation)? else {
            return Ok(None);
        };
        if compression == 1 {
            return crate::dat::decode_until(payload, RASTER_LEN).map(Some);
        }
        payload
            .get(..RASTER_LEN)
            .map(|raster| Some(raster.to_vec()))
            .ok_or_else(|| error(0, "raw raster is shorter than 10000 bytes"))
    }
    /// Capture a raster as a raw entry. An uncaptured slot appends at EOF,
    /// which requires the actual length to equal the recorded one; a captured
    /// slot is rewritten in place, shifting every later positive offset and
    /// the recorded length by the payload-length difference. Excluded slots
    /// are not recorded (`Ok(false)`).
    pub fn store(
        &mut self,
        map: usize,
        elevation: usize,
        raster: &[u8],
    ) -> Result<bool, ParseError> {
        if raster.len() != RASTER_LEN {
            return Err(error(0, "a raster has exactly 10000 bytes"));
        }
        let offset = self
            .offset(map, elevation)
            .ok_or_else(|| error(0, "map/elevation outside the offset table"))?;
        let mut entry = Vec::with_capacity(5 + RASTER_LEN);
        put_u32(&mut entry, RASTER_LEN as u32);
        entry.push(0);
        entry.extend_from_slice(raster);
        match offset {
            -1 => return Ok(false),
            0 => {
                let at = self.bytes.len();
                if self.recorded_len() as usize != at {
                    return Err(error(at, "actual EOF differs from the recorded length"));
                }
                self.bytes.extend_from_slice(&entry);
                self.set_offset(map, elevation, at as i32);
                self.set_recorded_len(self.bytes.len() as u32);
            }
            start if start < 0 => return Err(error(0, "negative entry offset")),
            start => {
                let (_, payload) = self.entry(map, elevation)?.expect("positive offset");
                let old = payload.len();
                let begin = start as usize;
                self.bytes
                    .splice(begin..begin + 5 + old, entry.iter().copied());
                let delta = RASTER_LEN as i64 - old as i64;
                for other in 0..MAPS {
                    for layer in 0..3 {
                        let value = self.offset(other, layer).expect("slot");
                        if value > start {
                            self.set_offset(other, layer, (i64::from(value) + delta) as i32);
                        }
                    }
                }
                let length = i64::from(self.recorded_len()) + delta;
                self.set_recorded_len(length as u32);
            }
        }
        Ok(true)
    }
}

/// Two-bit display cell: four cells per byte, most-significant pair first.
/// Values 1 (wall) and 2 (scenery) draw; 0 and 3 do not.
pub fn cell(raster: &[u8], row: usize, column: usize) -> u8 {
    raster
        .get(row * 50 + column / 4)
        .map_or(0, |byte| (byte >> (2 * (3 - column % 4))) & 3)
}

/// The recovered raster writer for one elevation: seen walls write 1, seen
/// scenery (except the ignored prototype) writes 2, at `X = 200 - column`, so
/// column zero spills into the following row and the last row's column-zero
/// write falls outside the payload. Later objects overwrite earlier ones.
pub fn raster_from_objects<'a>(
    objects: impl IntoIterator<Item = &'a MapObject>,
    elevation: u8,
) -> Vec<u8> {
    let mut raster = vec![0; RASTER_LEN];
    for object in objects {
        if object.elevation != elevation || !object.flags.contains(BasicFlags::SEEN) {
            continue;
        }
        let Some(tile) = object.hex.filter(|hex| (0..40000).contains(hex)) else {
            continue;
        };
        let value = match object.pid >> 24 {
            3 => 1,
            2 if object.pid != IGNORED_SCENERY => 2,
            _ => continue,
        };
        let x = 200 - (tile % 200) as usize;
        let at = x / 4 + 50 * (tile / 200) as usize;
        let shift = 2 * (3 - x % 4);
        if let Some(byte) = raster.get_mut(at) {
            *byte = (*byte & !(3 << shift)) | (value << shift);
        }
    }
    raster
}
