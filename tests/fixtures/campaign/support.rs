//! A small synthetic campaign for the autoplay bot: three maps joined by exit
//! grids, the last leading to the world map. Everything here is authored for
//! Vault Engine and named for the fixture. It holds no game data.
//!
//! The first map has a plain door, a locked door, a jammed door and a door
//! whose `damage_p_proc` destroys it; a container; a critter to talk to and
//! one to fight; a pistol with ammunition, dynamite, plastic explosive and a
//! drug on the floor. The second holds a critter and a door. The third holds
//! the exit to the world map.
use crate::{fallout2, state, world};

pub const LOCKED: i32 = 0x0200_0000;
pub const JAMMED: i32 = 0x0400_0000;

/// The entrance hex of every map.
pub const ENTRY: i32 = 20100;
/// The map object ids of the first map.
pub const PLAIN_DOOR: u32 = 10;
pub const LOCKED_DOOR: u32 = 11;
pub const JAMMED_DOOR: u32 = 12;
pub const SCRIPTED_DOOR: u32 = 13;
pub const CONTAINER: u32 = 14;
pub const FRIEND: u32 = 15;
pub const FOE: u32 = 16;
pub const PISTOL: u32 = 17;
pub const AMMUNITION: u32 = 18;
pub const DYNAMITE: u32 = 19;
pub const PLASTIC: u32 = 20;
pub const DRUG: u32 = 21;
pub const EXIT_GRID: u32 = 22;
/// The hex of the first map's exit grid.
pub const EXIT_HEX: i32 = 20700;

/// The item prototype rows, by their row in `items.lst`.
const PISTOL_ROW: i32 = 8;
const AMMUNITION_ROW: i32 = 31;
const DRUG_ROW: i32 = 40;
const DYNAMITE_ROW: i32 = 51;
const PLASTIC_ROW: i32 = 85;
const EXIT_PID: i32 = 0x0500_0010;
/// World map, as a map number.
const WORLD_MAP: i32 = -2;

/// The door script's row in `scripts.lst`.
const DOOR_SCRIPT: &str = "campdoor.int # local_vars=4";

/// Which game's tables the campaign is written for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edition {
    Fallout1,
    Fallout2,
}

/// The maps in campaign order, as (file stem, header name, map number). Each
/// map's exit grid leads to the next, and the last one's to the world map.
/// Fallout 2 has two: its table's other usable map is number 0, which no
/// transition can name.
pub fn maps(edition: Edition) -> Vec<(&'static str, &'static str, i32)> {
    match edition {
        Edition::Fallout1 => vec![
            ("campa", "CAMPA.MAP", 10),
            ("campb", "CAMPB.MAP", 11),
            ("campc", "CAMPC.MAP", 12),
        ],
        Edition::Fallout2 => vec![
            ("artemple", "artemple.map", 2),
            ("testvil", "testvil.map", 1),
        ],
    }
}

/// The first map's path.
pub fn start(edition: Edition) -> String {
    format!("maps/{}.map", maps(edition)[0].0)
}

struct Script {
    index: i32,
    id: u32,
}

/// One map object: its record, and the script it carries.
fn placed(mut record: Vec<u8>, script: Option<Script>) -> (Vec<u8>, Option<Script>) {
    if let Some(script) = &script {
        world::word(&mut record, 0x40, 0x0400_0000 | (script.id as i32));
        world::word(&mut record, 0x44, script.index);
    }
    (record, script)
}

fn door(id: u32, hex: i32, lock: i32, script: Option<i32>) -> (Vec<u8>, Option<Script>) {
    let record = world::object(2, hex, id, &[lock]);
    placed(record, script.map(|index| Script { index, id }))
}

fn item(id: u32, hex: i32, row: i32, extra: &[i32]) -> (Vec<u8>, Option<Script>) {
    let mut record = world::object(0, hex, id, extra);
    world::word(&mut record, 0x24, 16);
    world::word(&mut record, 0x2c, row);
    (record, None)
}

fn critter(id: u32, hex: i32, team: i32, script: Option<i32>) -> (Vec<u8>, Option<Script>) {
    let mut record = world::object(1, hex, id, &[]);
    world::word(&mut record, 0x6c, team);
    world::word(&mut record, 0x74, 30);
    placed(record, script.map(|index| Script { index, id }))
}

fn exit(id: u32, hex: i32, map: i32) -> (Vec<u8>, Option<Script>) {
    let mut record = world::object(5, hex, id, &[map, ENTRY, 0, 0]);
    world::word(&mut record, 0x24, 0x1);
    world::word(&mut record, 0x2c, EXIT_PID);
    (record, None)
}

/// A map of `objects` over the synthetic floor, entered at [`ENTRY`].
fn map(name: &str, mut objects: Vec<(Vec<u8>, Option<Script>)>) -> Vec<u8> {
    let hex = |record: &[u8]| i32::from_be_bytes(record[4..8].try_into().unwrap());
    objects.sort_by_key(|(record, _)| hex(record));
    let scripts: Vec<&Script> = objects.iter().filter_map(|(_, s)| s.as_ref()).collect();
    assert!(scripts.len() <= 16);
    let mut bytes = world::preview_map();
    bytes.truncate(248 + 120000);
    bytes.extend(world::words(&[0, 0, 0, 0, scripts.len() as i32]));
    if !scripts.is_empty() {
        for index in 0..16 {
            let mut slot = match scripts.get(index) {
                Some(script) => {
                    let mut slot = world::script_slot(4, script.id);
                    world::word(&mut slot, 12, script.index);
                    world::word(&mut slot, 20, script.id as i32);
                    world::word(&mut slot, 28, 4);
                    slot
                }
                None => world::script_slot(0, 0),
            };
            bytes.append(&mut slot);
        }
        bytes.extend(world::words(&[scripts.len() as i32, 0]));
    }
    let count = objects.len() as i32;
    bytes.extend(world::words(&[count, count]));
    for (record, _) in &objects {
        bytes.extend(record);
    }
    bytes.extend(world::words(&[0, 0]));
    fallout2::named_map(bytes, name, 0, ENTRY, 0)
}

/// `rows` rows of `file` in `folder`'s list, with `named` over them.
fn write_list(root: &std::path::Path, folder: &str, rows: usize, named: &[(i32, &str)]) {
    let mut list = vec!["synthetic.pro"; rows];
    for (row, file) in named {
        list[*row as usize - 1] = file;
    }
    std::fs::write(
        root.join(format!("proto/{folder}/{folder}.lst")),
        list.join("\n") + "\n",
    )
    .unwrap();
}

fn item_prototype(row: i32, subtype: usize) -> Vec<u8> {
    let mut bytes = world::prototype(0, subtype);
    world::word(&mut bytes, 0, row);
    bytes
}

/// Write the campaign over a corpus `state::write_corpus` wrote at `root`.
/// Under [`Edition::Fallout2`] the corpus also gets the SPEC-018 tables.
pub fn write_campaign(root: &std::path::Path, edition: Edition) {
    if edition == Edition::Fallout2 {
        fallout2::write_tables(root);
    }
    // A door a player can use, as the synthetic scenery prototype cannot be.
    let mut scenery = world::prototype(2, 0);
    world::word(&mut scenery, 0x18, 0x800);
    std::fs::write(root.join("proto/scenery/synthetic.pro"), scenery).unwrap();
    // A pistol of 10 rounds and range 10, 4 AP, with animation code 4, and its
    // ammunition.
    let mut pistol = item_prototype(PISTOL_ROW, 3);
    world::word(&mut pistol, 0x39, 4);
    world::word(&mut pistol, 0x39 + 16, 10);
    world::word(&mut pistol, 0x39 + 20, 10);
    world::word(&mut pistol, 0x18, 0x76);
    world::word(&mut pistol, 0x34, -1);
    world::word(&mut pistol, 0x39 + 32, 4);
    world::word(&mut pistol, 0x39 + 36, 5);
    world::word(&mut pistol, 0x39 + 52, 1);
    world::word(&mut pistol, 0x39 + 60, 10);
    let mut ammunition = item_prototype(AMMUNITION_ROW, 4);
    world::word(&mut ammunition, 0x39, 1);
    world::word(&mut ammunition, 0x39 + 4, 5);
    let items = root.join("proto/items");
    for (file, bytes) in [
        ("pistol.pro", pistol),
        ("ammo.pro", ammunition),
        ("drug.pro", item_prototype(DRUG_ROW, 2)),
        ("dynamite.pro", item_prototype(DYNAMITE_ROW, 5)),
        ("plastic.pro", item_prototype(PLASTIC_ROW, 5)),
    ] {
        std::fs::write(items.join(file), bytes).unwrap();
    }
    write_list(
        root,
        "items",
        100,
        &[
            (PISTOL_ROW, "pistol.pro"),
            (AMMUNITION_ROW, "ammo.pro"),
            (DRUG_ROW, "drug.pro"),
            (DYNAMITE_ROW, "dynamite.pro"),
            (PLASTIC_ROW, "plastic.pro"),
        ],
    );
    let mut grid = world::prototype(5, 0);
    world::word(&mut grid, 0, EXIT_PID);
    std::fs::write(root.join("proto/misc/exit.pro"), grid).unwrap();
    write_list(root, "misc", 32, &[(EXIT_PID & 0xff_ffff, "exit.pro")]);
    // SPEC-007 §3.10.1: an attack whose art is missing does not register.
    for suffix in [
        "aq", "an", "ao", "ap", "ar", "ba", "bb", "bo", "bp", "ch", "cj",
    ] {
        std::fs::copy(
            root.join("art/critters/sampleaa.frm"),
            root.join(format!("art/critters/sample{suffix}.frm")),
        )
        .unwrap();
    }
    // The pistol's animation code 4: its stand, walk and fire art.
    for letter in 'a'..='m' {
        std::fs::copy(
            root.join("art/critters/sampleaa.frm"),
            root.join(format!("art/critters/sampleg{letter}.frm")),
        )
        .unwrap();
    }
    // The door script: its `damage_p_proc` destroys it.
    let destroy = [state::instruction(0x80bc), state::instruction(0x80f4)].concat();
    let door_script = state::program(
        &[],
        &[
            (b"start", state::returning(Vec::new())),
            (b"damage_p_proc", state::returning(destroy)),
        ],
    );
    std::fs::write(root.join("scripts/campdoor.int"), door_script).unwrap();
    let list = root.join("scripts/scripts.lst");
    let mut text = std::fs::read_to_string(&list).unwrap();
    let door_index = text.lines().count() as i32;
    text.push_str(DOOR_SCRIPT);
    text.push('\n');
    std::fs::write(&list, text).unwrap();

    let list = maps(edition);
    let next = |at: usize| list.get(at + 1).map_or(WORLD_MAP, |map| map.2);
    let objects = [
        vec![
            door(PLAIN_DOOR, 20301, 0, None),
            door(LOCKED_DOOR, 20303, LOCKED, None),
            door(JAMMED_DOOR, 20305, JAMMED, None),
            door(SCRIPTED_DOOR, 20307, LOCKED, Some(door_index)),
            item(CONTAINER, 20500, 1, &[]),
            critter(FRIEND, 20099, 0, Some(0)),
            critter(FOE, 20900, 1, None),
            item(PISTOL, 20102, PISTOL_ROW, &[5, 0x11]),
            item(AMMUNITION, 20104, AMMUNITION_ROW, &[5]),
            item(DYNAMITE, 20106, DYNAMITE_ROW, &[0]),
            item(PLASTIC, 20108, PLASTIC_ROW, &[0]),
            item(DRUG, 20110, DRUG_ROW, &[]),
            exit(EXIT_GRID, EXIT_HEX, next(0)),
        ],
        vec![
            door(30, 20301, 0, None),
            critter(31, 20900, 1, None),
            exit(32, EXIT_HEX, next(1)),
        ],
        vec![exit(40, EXIT_HEX, next(2))],
    ];
    let files = list.iter().copied().zip(objects);
    let mut messages = std::fs::read_to_string(root.join("text/english/game/map.msg")).unwrap();
    for ((stem, name, number), objects) in files {
        std::fs::write(root.join(format!("maps/{stem}.map")), map(name, objects)).unwrap();
        std::fs::write(root.join(format!("maps/{stem}.gam")), b"MAP_GLOBAL_VARS:\n").unwrap();
        if edition == Edition::Fallout1 {
            messages.push_str(&format!("{{{number}}}{{}}{{{name}}}\n"));
        }
    }
    std::fs::write(root.join("text/english/game/map.msg"), messages).unwrap();
}
