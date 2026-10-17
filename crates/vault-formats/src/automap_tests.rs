//! SPEC-008 §2.8 automap database and §4.4 raster vectors.
use crate::{
    automap::*,
    map::{MapObject, ObjectDetail, ObjectState},
    pro::{BasicFlags, Light},
};

fn object(pid: u32, hex: i32, seen: bool, elevation: u8) -> MapObject {
    MapObject {
        id: 1,
        hex: Some(hex),
        displacement: [0; 2],
        screen: [0; 2],
        frame: 0,
        orientation: 0,
        art_id: 0,
        flags: BasicFlags(if seen { BasicFlags::SEEN.0 } else { 0 }),
        stored_elevation: i32::from(elevation),
        elevation,
        pid,
        combat_id: -1,
        light: Light {
            radius: 0,
            intensity: 0,
        },
        outline: 0,
        script_id: -1,
        cached_script_file: -1,
        inventory_capacity: 0,
        state: ObjectState::Other {
            word: 0,
            detail: ObjectDetail::Plain,
        },
        inventory: Vec::new(),
        owner: None,
        file_offset: 0,
    }
}

const WALL: u32 = 0x0300_0001;
const SCENERY: u32 = 0x0200_0001;

#[test]
fn spec008_fixture_automap_raster_vectors() {
    // Tile 199: X = 1, byte 0, shift 4.
    let raster = raster_from_objects(&[object(WALL, 199, true, 0)], 0);
    assert_eq!(raster.len(), RASTER_LEN);
    assert_eq!(raster[0], 0x10);
    assert_eq!(cell(&raster, 0, 1), 1);
    // Tile 0: X = 200 spills into the following serialized row.
    let raster = raster_from_objects(&[object(WALL, 0, true, 0)], 0);
    assert_eq!(raster[50], 0x40);
    assert_eq!(cell(&raster, 1, 0), 1);
    assert_eq!(raster.iter().filter(|byte| **byte != 0).count(), 1);
    // Tile 39800 computes byte 10000, outside the persisted payload.
    let raster = raster_from_objects(&[object(WALL, 39800, true, 0)], 0);
    assert!(raster.iter().all(|byte| *byte == 0));
}

#[test]
fn spec008_automap_raster_contributions() {
    let objects = [
        object(SCENERY, 198, true, 0),
        object(IGNORED_SCENERY, 197, true, 0),
        object(WALL, 196, false, 0),
        object(0x0000_0001, 195, true, 0),
        object(0x0100_0001, 194, true, 0),
        object(WALL, 193, true, 1),
        // Later objects overwrite earlier ones in traversal order.
        object(WALL, 1, true, 0),
        object(SCENERY, 1, true, 0),
    ];
    let raster = raster_from_objects(&objects, 0);
    // Tile 198 -> X 2 -> byte 0, shift 2 -> scenery 2.
    assert_eq!(raster[0], 0b0000_1000);
    assert_eq!(cell(&raster, 0, 2), 2);
    // Tile 1 -> X 199 -> byte 49, shift 0.
    assert_eq!(raster[49], 0b0000_0010);
    assert_eq!(raster.iter().filter(|byte| **byte != 0).count(), 2);
    // The other elevation's wall appears only in its own raster.
    let upper = raster_from_objects(&objects, 1);
    assert_eq!(cell(&upper, 0, 7), 1);
    assert_eq!(upper.iter().filter(|byte| **byte != 0).count(), 1);
}

fn word(db: &[u8], at: usize) -> i32 {
    i32::from_be_bytes(db[at..at + 4].try_into().unwrap())
}

/// A 10000-byte all-space raster: 556 length-18 matches from ring start 0
/// (the space-filled prefix), the final match truncated at 10000 bytes.
fn compressed_spaces() -> Vec<u8> {
    let mut payload = Vec::new();
    for group in 0..70 {
        payload.push(0);
        for _ in 0..if group == 69 { 4 } else { 8 } {
            payload.extend_from_slice(&[0x00, 0x0F]);
        }
    }
    payload
}

fn append(bytes: &mut Vec<u8>, map: usize, elevation: usize, compression: u8, payload: &[u8]) {
    let at = bytes.len() as i32;
    let slot = 5 + 4 * (3 * map + elevation);
    bytes[slot..slot + 4].copy_from_slice(&at.to_be_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    bytes.push(compression);
    bytes.extend_from_slice(payload);
    let length = bytes.len() as u32;
    bytes[1..5].copy_from_slice(&length.to_be_bytes());
}

#[test]
fn spec008_automap_database_layout_and_capture() {
    let fresh = AutomapDb::default();
    let bytes = fresh.bytes();
    assert_eq!((bytes.len(), EMPTY_LEN, bytes[0]), (797, 797, 1));
    assert_eq!(fresh.recorded_len(), 797);
    for map in 0..MAPS {
        for elevation in 0..3 {
            let expected = if EXCLUDED.contains(&map) { -1 } else { 0 };
            assert_eq!(word(bytes, 5 + 4 * (3 * map + elevation)), expected);
            assert_eq!(fresh.offset(map, elevation), Some(expected));
        }
    }
    assert_eq!(fresh.offset(66, 0), None);
    assert_eq!(fresh.raster(3, 0).unwrap(), None);
    assert_eq!(AutomapDb::parse(bytes.to_vec()).unwrap(), fresh);

    // Excluded slots are not recorded; recordable slots append at EOF.
    let mut db = fresh.clone();
    let raster: Vec<u8> = (0..RASTER_LEN).map(|i| i as u8).collect();
    assert!(!db.store(0, 0, &raster).unwrap());
    assert_eq!(db, fresh);
    assert!(db.store(3, 1, &raster).unwrap());
    assert!(db.store(4, 0, &raster).unwrap());
    assert_eq!(db.offset(3, 1), Some(797));
    assert_eq!(db.offset(4, 0), Some(797 + 10005));
    assert_eq!(db.recorded_len() as usize, db.bytes().len());
    assert_eq!(&db.bytes()[797..802], &[0, 0, 0x27, 0x10, 0]);
    assert_eq!(db.raster(3, 1).unwrap(), Some(raster.clone()));
    // Rewriting an equal-size entry leaves the later offset in place.
    let blank = vec![0; RASTER_LEN];
    assert!(db.store(3, 1, &blank).unwrap());
    assert_eq!(db.offset(4, 0), Some(797 + 10005));
    assert_eq!(db.raster(3, 1).unwrap(), Some(blank.clone()));
    assert_eq!(db.raster(4, 0).unwrap(), Some(raster.clone()));
    assert!(db.store(3, 1, &raster[..9999]).is_err());
}

#[test]
fn spec008_automap_compressed_entries_and_offset_shifts() {
    let spaces = compressed_spaces();
    let mut bytes = AutomapDb::default().bytes().to_vec();
    append(&mut bytes, 3, 0, 1, &spaces);
    let raw: Vec<u8> = (0..RASTER_LEN).map(|i| (i % 7) as u8).collect();
    append(&mut bytes, 4, 2, 0, &raw);
    let mut db = AutomapDb::parse(bytes.clone()).unwrap();
    assert_eq!(db.raster(3, 0).unwrap(), Some(vec![b' '; RASTER_LEN]));
    assert_eq!(db.raster(4, 2).unwrap(), Some(raw.clone()));
    let later = 797 + 5 + spaces.len() as i32;
    assert_eq!(db.offset(4, 2), Some(later));
    // Replacing the compressed entry with a raw one shifts every later
    // positive offset and the recorded length by the payload difference.
    let delta = (RASTER_LEN - spaces.len()) as i32;
    let captured = vec![0x55; RASTER_LEN];
    assert!(db.store(3, 0, &captured).unwrap());
    assert_eq!(db.offset(3, 0), Some(797));
    assert_eq!(db.offset(4, 2), Some(later + delta));
    assert_eq!(db.recorded_len(), bytes.len() as u32 + delta as u32);
    assert_eq!(db.recorded_len() as usize, db.bytes().len());
    assert_eq!(db.raster(3, 0).unwrap(), Some(captured));
    assert_eq!(db.raster(4, 2).unwrap(), Some(raw));
    assert_eq!(db.offset(5, 0), Some(0));
}

#[test]
fn spec008_automap_malformed_databases_are_rejected() {
    let fresh = AutomapDb::default().bytes().to_vec();
    let mut version = fresh.clone();
    version[0] = 2;
    assert!(AutomapDb::parse(version).is_err());
    assert!(AutomapDb::parse(fresh[..796].to_vec()).is_err());
    assert!(AutomapDb::parse(Vec::new()).is_err());

    // A stream that runs out of input before 10000 output bytes.
    let mut short = fresh.clone();
    let spaces = compressed_spaces();
    append(&mut short, 3, 0, 1, &spaces[..spaces.len() - 2]);
    assert!(
        AutomapDb::parse(short.clone())
            .unwrap()
            .raster(3, 0)
            .is_err()
    );
    // A raw entry shorter than the raster.
    let mut raw = fresh.clone();
    append(&mut raw, 3, 0, 0, &[0; 100]);
    assert!(AutomapDb::parse(raw).unwrap().raster(3, 0).is_err());
    // Entries pointing into the table or past EOF.
    // Map 3 elevation 0's offset word is at 5 + 4 x 9.
    let mut into_table = fresh.clone();
    into_table[41..45].copy_from_slice(&40_i32.to_be_bytes());
    assert!(AutomapDb::parse(into_table).unwrap().raster(3, 0).is_err());
    let mut past = fresh.clone();
    past[41..45].copy_from_slice(&900_i32.to_be_bytes());
    assert!(AutomapDb::parse(past).unwrap().raster(3, 0).is_err());
    // A damaged entry does not hide intact ones.
    let mut mixed = fresh.clone();
    append(&mut mixed, 4, 0, 0, &[0; RASTER_LEN]);
    mixed[41..45].copy_from_slice(&20000_i32.to_be_bytes());
    let mixed = AutomapDb::parse(mixed).unwrap();
    assert!(mixed.raster(3, 0).is_err());
    assert_eq!(mixed.raster(4, 0).unwrap(), Some(vec![0; RASTER_LEN]));

    // Appending requires actual EOF to match the recorded length.
    let mut padded = fresh.clone();
    padded.push(0);
    let mut padded = AutomapDb::parse(padded).unwrap();
    assert_eq!(padded.recorded_len(), 797);
    assert!(padded.store(3, 0, &[0; RASTER_LEN]).is_err());
}
