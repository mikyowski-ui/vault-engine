//! SPEC-024b §7.3 tests 3 to 6: every array vector of
//! `oracle/SPEC-024/sfall-array-vectors.tsv`, step by step, and the sidecar.
use crate::{
    WorldState,
    sfall::{GlobalKey, SfallWorld},
    spec024_hook_tests::hook_vector_row,
    spec024_rows_tests::RowSession,
    state_hash::fnv1a64,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    panic::{AssertUnwindSafe, catch_unwind},
};
use vault_vm::{
    sfall::SfallValue,
    sfall_arrays::{ArrayStore, Elements, SfallArray, SidecarError, Storage, value_order},
};

pub(crate) const VECTORS: &str = include_str!("../../../oracle/SPEC-024/sfall-array-vectors.tsv");

fn text(value: &str) -> SfallValue {
    SfallValue::text(value)
}
fn int(value: i32) -> SfallValue {
    SfallValue::Int(value)
}
/// `I:5`, `F:0.5`, `S:"x"` or a bare `"x"`.
fn value(cell: &str) -> SfallValue {
    if let Some(number) = cell.strip_prefix("I:") {
        int(number.parse().unwrap())
    } else if let Some(number) = cell.strip_prefix("F:") {
        SfallValue::float(number.parse().unwrap())
    } else if let Some(quoted) = cell.strip_prefix("S:") {
        value(quoted)
    } else {
        text(cell.trim_matches('"'))
    }
}
/// A bare item of a `create list` or `create map`: a number when it reads
/// as one.
fn item(cell: &str) -> SfallValue {
    if let Ok(number) = cell.parse::<i32>() {
        int(number)
    } else if let Ok(number) = cell.parse::<f32>() {
        SfallValue::float(number)
    } else {
        text(cell)
    }
}
fn shown(value: &SfallValue) -> String {
    match value {
        SfallValue::Int(value) => format!("I:{value}"),
        SfallValue::Float(bits) => format!("F:{:?}", f32::from_bits(*bits)),
        SfallValue::Text(bytes) => format!("S:\"{}\"", String::from_utf8_lossy(bytes)),
    }
}
fn shown_array((id, array): (&u32, &SfallArray)) -> String {
    let storage = match array.storage {
        Storage::Temporary => "temp",
        Storage::Permanent => "perm",
        Storage::Saved => "saved",
    };
    let key = array
        .save_key
        .as_ref()
        .map_or(String::new(), |key| format!(" key={}", shown(key)));
    let (shape, elements) = match &array.elements {
        Elements::List(values) => ("list", values.iter().map(shown).collect::<Vec<_>>()),
        Elements::Map { lookup, pairs } => (
            if *lookup { "lookup" } else { "map" },
            pairs
                .iter()
                .map(|(key, value)| format!("{}=>{}", shown(key), shown(value)))
                .collect(),
        ),
    };
    format!("#{id} {shape} {storage}{key} [{}]", elements.join(","))
}
pub(crate) fn state(store: &ArrayStore) -> String {
    let arrays: Vec<String> = store.arrays().iter().map(shown_array).collect();
    arrays.join("; ")
}
/// The call in `operation`: its name and its operands.
pub(crate) fn call(operation: &str) -> (&str, Vec<&str>) {
    let (name, rest) = operation.split_once('(').unwrap();
    let inside = rest.split(')').next().unwrap();
    (name, inside.split(',').collect())
}
fn shuffle(store: &mut ArrayStore, id: i32, size: i32, draws: &[i32]) {
    let mut draws = draws.iter().copied();
    store
        .resize(id, size, |_| {
            Ok::<i32, ()>(draws.next().expect("the vector gives each draw"))
        })
        .unwrap();
}
fn step(store: &mut ArrayStore, operation: &str) -> String {
    let none = "-".to_owned();
    if operation == "end of Simulation phase" {
        store.end_of_step();
        return none;
    }
    if let Some(items) = operation.strip_prefix("create list [") {
        let items: Vec<SfallValue> = items.trim_end_matches(']').split(',').map(item).collect();
        let id = store.create(items.len() as i32, 0, Storage::Permanent);
        for (index, value) in items.into_iter().enumerate() {
            store.set(id, int(index as i32), value);
        }
        return shown(&int(id));
    }
    if let Some(items) = operation.strip_prefix("create map {") {
        let id = store.create(-1, 0, Storage::Permanent);
        for pair in items.trim_end_matches('}').split(',') {
            let (key, value) = pair.split_once(':').unwrap();
            store.set(id, item(key), item(value));
        }
        return shown(&int(id));
    }
    if operation == "create array 1 and lookup map 2" {
        let id = store.create(2, 0, Storage::Permanent);
        store.set(id, int(0), text("hi"));
        store.set(id, int(1), SfallValue::float(0.5));
        let map = store.create(-1, 2, Storage::Permanent);
        store.set(map, int(1), int(0));
        return shown(&int(map));
    }
    if let Some((first, second)) = operation.split_once(" then ") {
        store.create(0, 0, Storage::Permanent);
        store.create(0, 0, Storage::Permanent);
        step(store, first);
        return step(store, second);
    }
    if let Some((resize, draws)) = operation.split_once(" with draws ") {
        let (_, args) = call(resize);
        let draws: Vec<i32> = draws.split(',').map(|draw| draw.parse().unwrap()).collect();
        let (id, size) = (args[0].parse().unwrap(), args[1].parse().unwrap());
        shuffle(store, id, size, &draws);
        return none;
    }
    let (name, args) = call(operation);
    let id = || args[0].parse::<i32>().unwrap();
    let number = |index: usize| args[index].parse::<i32>().unwrap();
    match name {
        "create_array" => shown(&int(store.create(number(0), number(1), Storage::Permanent))),
        "temp_array" => shown(&int(store.create(number(0), number(1), Storage::Temporary))),
        "fix_array" => {
            store.fix(id());
            none
        }
        "free_array" => {
            store.free(id());
            none
        }
        "set_array" => {
            store.set(id(), value(args[1]), value(args[2]));
            none
        }
        "get_array" => shown(&store.get(id(), &value(args[1]))),
        "len_array" => shown(&int(store.len(id()))),
        "resize_array" => {
            shuffle(store, id(), number(1), &[]);
            none
        }
        "array_key" => shown(&store.key_at(id(), number(1))),
        "scan_array" => shown(&store.scan(id(), &value(args[1]))),
        "save_array" => {
            store.save(value(args[0]), number(1));
            none
        }
        "load_array" => shown(&int(store.load(&value(args[0])))),
        other => panic!("no operation {other}"),
    }
}
fn hex(cell: &str) -> Vec<u8> {
    cell.split_whitespace()
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect()
}
/// The bytes of `vector`'s row `step`.
pub(crate) fn vector_bytes(vector: &str, step: &str) -> Vec<u8> {
    hex(vector_cell(vector, step, 3))
}
/// Column `index` of `vector`'s row `step`.
pub(crate) fn vector_cell(vector: &str, step: &str, index: usize) -> &'static str {
    let row = VECTORS
        .lines()
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .find(|cells| cells[0] == vector && cells[1] == step)
        .unwrap_or_else(|| panic!("no row {step} in {vector}"));
    row[index]
}
/// Vector F's globals and header fields, as its sidecar bytes hold them.
fn f_world(store: ArrayStore) -> SfallWorld {
    let bytes = vector_bytes("F", "sidecar");
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let mut world = SfallWorld {
        arrays: store,
        next_unique: word(12),
        forced_map: word(16) as i32,
        forced_flags: word(20),
        ..Default::default()
    };
    world
        .globals
        .insert(GlobalKey::Int(7), SfallValue::float(0.5));
    world
        .globals
        .insert(GlobalKey::Text(*b"a_myvar1"), int(1000));
    world
}

/// One array vector row, on the walk's store.
fn array_row(walk: &mut Walk, cells: &[&str]) {
    let [name, step_name, operation, push, state_after, _] = cells[..] else {
        panic!("{cells:?}");
    };
    let store = &mut walk.store;
    let world = f_world(store.clone());
    match step_name {
        "sidecar" => {
            let bytes = hex(push);
            assert_eq!(world.sidecar(), bytes, "{cells:?}");
            assert_eq!(state_after, format!("bytes={}", bytes.len()));
            walk.sidecar = bytes;
            return;
        }
        "sidecar-fnv" => {
            assert_eq!(
                format!("{:016X}", fnv1a64(&walk.sidecar)),
                push,
                "{cells:?}"
            );
            return;
        }
        "stream" => {
            let streamed = if name == "F3" {
                SfallWorld::default().extension_stream()
            } else {
                world.extension_stream()
            };
            assert_eq!(streamed, hex(push), "{cells:?}");
            return;
        }
        "stream-after-step" => {
            let mut after = world;
            after.arrays.end_of_step();
            assert_eq!(after.extension_stream(), hex(push), "{cells:?}");
            return;
        }
        "sfall_hash" | "sfall_hash-after-step" => {
            let mut hashed = if name == "F3" {
                SfallWorld::default()
            } else {
                world
            };
            if step_name != "sfall_hash" {
                hashed.arrays.end_of_step();
            }
            let hash = fnv1a64(&hashed.extension_stream());
            assert_eq!(format!("{hash:016X}"), push, "{cells:?}");
            return;
        }
        _ => {}
    }
    if operation == "load the slot" {
        *store = SfallWorld::from_sidecar(&walk.sidecar).unwrap().arrays;
    } else {
        assert_eq!(step(store, operation), push, "{cells:?}");
    }
    // Vector D's rows show the array they touch; the rest show all.
    if name == "D" {
        let id: u32 = state_after[1..].split(' ').next().unwrap().parse().unwrap();
        let array = (&id, &store.arrays()[&id]);
        assert_eq!(shown_array(array), state_after, "{cells:?}");
    } else {
        assert_eq!(state(store), state_after, "{cells:?}");
    }
}
/// One row of MG1, on the state the walk migrated.
fn migration_row(walk: &mut Walk, cells: &[&str]) {
    let [_, step_name, _, push, state_after, _] = cells[..] else {
        panic!("{cells:?}");
    };
    match step_name {
        "sidecar-r1" => {
            walk.migrated = Some(SfallWorld::from_sidecar(&hex(push)).unwrap());
            assert_eq!(state_after, format!("bytes={}", hex(push).len()));
        }
        "sidecar-r1-empty" => {
            walk.migrated = Some(SfallWorld::from_sidecar(&hex(push)).unwrap());
            assert_eq!(walk.migrated, Some(SfallWorld::default()));
        }
        step_name => {
            let world = walk.migrated.as_mut().expect("the row before it loaded");
            match step_name {
                "2" => assert_eq!(state(&world.arrays), state_after),
                "defaults" => assert_eq!(
                    format!(
                        "next unique ID {:#x}; forced map {}; forced flags {}",
                        world.next_unique, world.forced_map, world.forced_flags
                    ),
                    push
                ),
                "stream" => assert_eq!(world.extension_stream(), hex(push)),
                "sidecar" => {
                    walk.sidecar = world.sidecar();
                    assert_eq!(walk.sidecar, hex(push));
                    assert_eq!(state_after, format!("bytes={}", walk.sidecar.len()));
                }
                "sidecar-fnv" => {
                    assert_eq!(format!("{:016X}", fnv1a64(&walk.sidecar)), push);
                }
                "sfall_hash" | "sfall_hash-empty" => {
                    let hash = fnv1a64(&world.extension_stream());
                    assert_eq!(format!("{hash:016X}"), push);
                }
                other => {
                    assert_eq!(step(&mut world.arrays, cells[2]), push, "{other}");
                    assert_eq!(state(&world.arrays), state_after);
                }
            }
        }
    }
}
/// One row of MG2: its bytes are refused for its reason.
fn refusal_row(cells: &[&str]) {
    let error = SfallWorld::from_sidecar(&hex(cells[3])).unwrap_err();
    let reason = cells[4].strip_prefix("refused: ").unwrap();
    let expected = match error {
        SidecarError::Revision(revision) => format!("unknown revision {revision}"),
        SidecarError::Truncated => "truncated".to_owned(),
        SidecarError::Trailing(_) => "trailing bytes".to_owned(),
        other => panic!("{other:?}"),
    };
    assert_eq!(reason, expected, "{cells:?}");
}

/// What reads a vector group's rows. A group with no reader fails
/// [`every_vector_group_has_a_reader`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reader {
    Arrays,
    Migration,
    Refusal,
    Hooks,
    Files,
    UniqueIds,
    Messages,
    Tags,
    Explosives,
}
fn reader_of(group: &str) -> Option<Reader> {
    Some(match group {
        "A" | "B" | "C" | "D" | "E" | "F" | "F2" | "F3" | "SS1" => Reader::Arrays,
        "MG1" => Reader::Migration,
        "MG2" => Reader::Refusal,
        "H1" | "H2" | "H3" | "H4" | "H5" | "H6" | "H7" | "H8" | "H9" | "H10" | "H11" | "H12"
        | "H13" | "H14" | "H15" | "H16" | "H17" | "SS2" => Reader::Hooks,
        "V1" | "V2" | "V3" | "V4" => Reader::Files,
        "U1" => Reader::UniqueIds,
        "M1" => Reader::Messages,
        "T1" => Reader::Tags,
        "E1" => Reader::Explosives,
        _ => return None,
    })
}
/// A walk's state, which a vector's rows carry from one to the next. V1 to
/// V4 are one session: V3 and V4 name V1's handle.
#[derive(Default)]
struct Walk {
    group: String,
    store: ArrayStore,
    sidecar: Vec<u8>,
    migrated: Option<SfallWorld>,
    session: Option<RowSession>,
}
impl Walk {
    fn row(&mut self, reader: Reader, cells: &[&str]) {
        let name = cells[0];
        if name != self.group {
            // V1 to V4 keep their one session.
            if !(name.starts_with('V') && self.group.starts_with('V')) {
                self.session = None;
            }
            self.group = name.to_owned();
            self.store = ArrayStore::default();
        }
        match reader {
            Reader::Arrays => array_row(self, cells),
            Reader::Migration => migration_row(self, cells),
            Reader::Refusal => refusal_row(cells),
            Reader::Hooks => hook_vector_row(cells),
            Reader::Files
            | Reader::UniqueIds
            | Reader::Messages
            | Reader::Tags
            | Reader::Explosives => self
                .session
                .get_or_insert_with(|| RowSession::for_group(cells[0]))
                .row(cells),
        }
    }
}
/// Every row of the groups `keep` keeps, in file order: the failing steps,
/// each by its vector and step, and the rows run per group. A failure does
/// not stop the walk, so one mismatch cannot hide a later one.
fn walk(keep: impl Fn(&str) -> bool) -> (Vec<String>, BTreeMap<String, usize>) {
    let mut state = Walk::default();
    let mut failures = Vec::new();
    let mut counts = BTreeMap::new();
    for line in VECTORS.lines().skip(1) {
        let cells: Vec<&str> = line.split('\t').collect();
        let name = cells[0];
        if !keep(name) {
            continue;
        }
        *counts.entry(name.to_owned()).or_insert(0) += 1;
        let Some(reader) = reader_of(name) else {
            failures.push(format!("{name}: no reader"));
            continue;
        };
        let ran = catch_unwind(AssertUnwindSafe(|| state.row(reader, &cells)));
        if let Err(panic) = ran {
            let text = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|text| (*text).to_owned()))
                .unwrap_or_default();
            failures.push(format!("{name} step {}: {text}", cells[1]));
        }
    }
    (failures, counts)
}
fn assert_walked(failures: Vec<String>) {
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn t3_every_array_vector_gives_its_push_and_its_state() {
    let (failures, counts) = walk(|_| true);
    assert_walked(failures);
    assert_eq!(counts.values().sum::<usize>(), 182, "{counts:?}");
    for (group, rows) in [
        ("A", 12),
        ("B", 18),
        ("C", 4),
        ("D", 10),
        ("E", 9),
        ("F", 16),
        ("F2", 3),
        ("F3", 2),
        ("V1", 5),
        ("V2", 1),
        ("V3", 3),
        ("V4", 1),
        ("U1", 5),
        ("M1", 8),
        ("T1", 6),
        ("E1", 6),
        ("MG1", 10),
        ("MG2", 4),
        ("SS1", 11),
        ("SS2", 4),
    ] {
        assert_eq!(counts.get(group), Some(&rows), "{group}");
    }
    let hooks: usize = (1..=17).map(|n| counts[&format!("H{n}")]).sum();
    assert_eq!(hooks, 44);
}

/// A new group in the TSV has no reader until a test reads it.
#[test]
fn every_vector_group_has_a_reader() {
    let groups: BTreeSet<&str> = VECTORS
        .lines()
        .skip(1)
        .map(|line| line.split('\t').next().unwrap())
        .collect();
    let unread: Vec<&&str> = groups
        .iter()
        .filter(|group| reader_of(group).is_none())
        .collect();
    assert_eq!(unread, Vec::<&&str>::new());
    assert!(groups.len() >= 37, "{groups:?}");
    // The walk says so for a row of such a group too.
    assert_eq!(reader_of("Z1"), None);
}

#[test]
fn t9_vectors_v_u_m_t_e_compare_the_push_and_the_state_step_by_step() {
    let (failures, counts) =
        walk(|group| matches!(group, "V1" | "V2" | "V3" | "V4" | "U1" | "M1" | "T1" | "E1"));
    assert_walked(failures);
    assert_eq!(counts.values().sum::<usize>(), 35);
}

#[test]
fn t15_mg1_loads_to_its_state_its_hash_and_its_next_save_and_mg2_refuses() {
    let (failures, counts) = walk(|group| matches!(group, "MG1" | "MG2"));
    assert_walked(failures);
    assert_eq!(counts.values().sum::<usize>(), 14);
    // The slot's side of this test is in `persistence_tests`.
    assert_eq!(
        vector_cell("MG1", "sfall_hash-empty", 3),
        vector_cell("F3", "sfall_hash", 3)
    );
}

#[test]
fn t16_ss1_and_ss2_compare_the_push_the_state_the_arguments_and_the_diagnostics() {
    let (failures, counts) = walk(|group| matches!(group, "SS1" | "SS2"));
    assert_walked(failures);
    assert_eq!(counts.values().sum::<usize>(), 15);
}

#[test]
fn t4_vector_f_round_trips_byte_for_byte_and_each_mutation_refuses_the_slot() {
    let bytes = vector_bytes("F", "sidecar");
    let world = SfallWorld::from_sidecar(&bytes).unwrap();
    assert_eq!(world.sidecar(), bytes);
    assert_eq!(world.arrays.next_id(), 4);
    assert_eq!(world.globals.len(), 2);
    assert_eq!(world.next_unique, 0x1000_0002);
    assert_eq!((world.forced_map, world.forced_flags), (161, 0x10));

    let mutated = |at: usize, byte: u8| {
        let mut copy = bytes.clone();
        copy[at] = byte;
        SfallWorld::from_sidecar(&copy)
    };
    assert_eq!(mutated(0, b'X'), Err(SidecarError::Tag));
    // A revision other than 1 and 2, as vector MG2's are.
    assert_eq!(mutated(4, 3), Err(SidecarError::Revision(3)));
    assert_eq!(mutated(4, 0), Err(SidecarError::Revision(0)));
    // The first global's key type byte, and the first array's shape.
    assert_eq!(mutated(28, 9), Err(SidecarError::Type(9)));
    assert_eq!(mutated(64, 2), Err(SidecarError::Shape(2)));
    assert_eq!(mutated(66, 3), Err(SidecarError::Storage(3)));
    assert_eq!(mutated(68, 2), Err(SidecarError::Option(2)));
    let mut longer = bytes.clone();
    longer.push(0);
    assert_eq!(
        SfallWorld::from_sidecar(&longer),
        Err(SidecarError::Trailing(1))
    );
    // Every truncation is refused, and none of them panics.
    for length in 0..bytes.len() {
        assert!(
            SfallWorld::from_sidecar(&bytes[..length]).is_err(),
            "{length}"
        );
    }
    // A duplicate ID: two copies of the only array record.
    let record = 60..bytes.len();
    let mut twice = bytes[..56].to_vec();
    twice.extend(2u32.to_le_bytes());
    twice.extend_from_slice(&bytes[record.clone()]);
    twice.extend_from_slice(&bytes[record]);
    assert_eq!(
        SfallWorld::from_sidecar(&twice),
        Err(SidecarError::DuplicateId(1))
    );
    // A revision 2 file cut to revision 1's header is no revision 1 file:
    // the revision chose the layout.
    let mut downgraded = bytes.clone();
    downgraded[4] = 1;
    assert!(SfallWorld::from_sidecar(&downgraded).is_err());
}

/// Vector MG1 (Decision D-32): a revision 1 sidecar's load, defaults, hash,
/// next save and the next array ID, row by row.
#[test]
fn mg1_a_revision_1_sidecar_migrates_to_its_state_defaults_and_hash() {
    let rows: Vec<&str> = VECTORS
        .lines()
        .filter(|line| line.starts_with("MG1\t"))
        .collect();
    assert_eq!(rows.len(), 10);
    let bytes = vector_bytes("MG1", "sidecar-r1");
    assert_eq!(
        vector_cell("MG1", "sidecar-r1", 4),
        format!("bytes={}", bytes.len())
    );
    let mut world = SfallWorld::from_sidecar(&bytes).unwrap();
    assert_eq!(state(&world.arrays), vector_cell("MG1", "2", 4));
    assert_eq!(world.globals.len(), 2);
    let defaults = format!(
        "next unique ID {:#x}; forced map {}; forced flags {}",
        world.next_unique, world.forced_map, world.forced_flags
    );
    assert_eq!(defaults, vector_cell("MG1", "defaults", 3));
    let stream = world.extension_stream();
    assert_eq!(stream, vector_bytes("MG1", "stream"));
    assert_eq!(
        format!("{:016X}", fnv1a64(&stream)),
        vector_cell("MG1", "sfall_hash", 3)
    );
    // Never a hash of the revision 1 bytes.
    assert_ne!(fnv1a64(&stream), fnv1a64(&bytes));
    let saved = world.sidecar();
    assert_eq!(saved, vector_bytes("MG1", "sidecar"));
    assert_eq!(
        vector_cell("MG1", "sidecar", 4),
        format!("bytes={}", saved.len())
    );
    assert_eq!(
        format!("{:016X}", fnv1a64(&saved)),
        vector_cell("MG1", "sidecar-fnv", 3)
    );
    // The revision 2 file and the migrated state are one state.
    assert_eq!(SfallWorld::from_sidecar(&saved).unwrap(), world);
    assert_eq!(
        step(&mut world.arrays, vector_cell("MG1", "8", 2)),
        vector_cell("MG1", "8", 3)
    );
    assert_eq!(state(&world.arrays), vector_cell("MG1", "8", 4));
    let empty = vector_bytes("MG1", "sidecar-r1-empty");
    let world = SfallWorld::from_sidecar(&empty).unwrap();
    assert_eq!(world, SfallWorld::default());
    let hash = format!("{:016X}", fnv1a64(&world.extension_stream()));
    assert_eq!(hash, vector_cell("MG1", "sfall_hash-empty", 3));
    assert_eq!(hash, vector_cell("F3", "sfall_hash", 3));
}

/// Vector MG2: revisions 0 and 3, and a revision 1 file cut short or left
/// long, are refused with the typed error.
#[test]
fn mg2_other_revisions_and_a_cut_or_long_revision_1_file_are_refused() {
    let rows: Vec<&str> = VECTORS
        .lines()
        .filter(|line| line.starts_with("MG2\t"))
        .collect();
    assert_eq!(rows.len(), 4);
    for (step_name, expected) in [
        ("refused-r0", SidecarError::Revision(0)),
        ("refused-r3", SidecarError::Revision(3)),
        ("refused-r1-truncated", SidecarError::Truncated),
        ("refused-r1-oversized", SidecarError::Trailing(1)),
    ] {
        let bytes = vector_bytes("MG2", step_name);
        assert_eq!(
            SfallWorld::from_sidecar(&bytes),
            Err(expected),
            "{step_name}"
        );
        assert!(vector_cell("MG2", step_name, 4).starts_with("refused: "));
    }
}

#[test]
fn t5_the_hash_is_the_vector_value_on_a_fresh_session() {
    let world = WorldState::new(crate::CharacterSheet::default(), 1).unwrap();
    let expected = u64::from_str_radix(vector_cell("F3", "sfall_hash", 3), 16).unwrap();
    assert_eq!(world.sfall_hash(), expected);
    let fresh = SfallWorld::default();
    assert_eq!(fresh.extension_stream(), vector_bytes("F3", "stream"));
    assert_eq!(fresh.extension_stream().len(), 32);
    // It takes `&self` and changes nothing.
    let before = world.state_hash();
    assert_eq!(world.sfall_hash(), world.sfall_hash());
    assert_eq!(world.state_hash(), before);
}

/// `count` pairs set, updated and removed through a store whose earlier life
/// is `noise`.
fn built(noise: usize) -> Vec<u8> {
    let mut held: Vec<Vec<u8>> = (0..noise).map(|size| vec![0; size * 64]).collect();
    let mut world = SfallWorld::default();
    for size in 0..noise {
        let id = world.arrays.create(size as i32, 0, Storage::Permanent);
        world.arrays.free(id);
    }
    let first = world.arrays.next_id();
    let map = world.arrays.create(-1, 0, Storage::Permanent);
    for key in 0..200 {
        world.arrays.set(map, int(199 - key), text("value"));
        world.arrays.set(map, text(&format!("k{key}")), int(key));
        held.push(vec![1; key as usize]);
    }
    for key in (0..200).step_by(3) {
        world.arrays.set(map, int(key), int(0));
    }
    world.arrays.save(text("map"), map);
    assert!(first as i32 == map && !held.is_empty());
    let mut bytes = world.sidecar();
    // The ID differs with the history; the elements must not.
    bytes.drain(..48);
    bytes
}

#[test]
fn t6_a_map_serializes_the_same_in_two_allocator_states() {
    let (quiet, noisy) = (built(0), built(50));
    assert_eq!(quiet.len(), noisy.len());
    assert_eq!(quiet, noisy);
}

#[test]
fn the_sort_order_is_total_and_descending_is_its_reverse() {
    use std::cmp::Ordering::{Equal, Greater, Less};
    let nan = SfallValue::float(f32::NAN);
    assert_eq!(value_order(&int(1), &SfallValue::float(1.0)), Less);
    assert_eq!(value_order(&SfallValue::float(1.0), &int(1)), Greater);
    assert_eq!(value_order(&int(5), &text("a")), Less);
    assert_eq!(value_order(&nan, &int(5)), Greater);
    assert_eq!(value_order(&nan, &text("a")), Less);
    assert_eq!(value_order(&nan, &nan), Equal);
    assert_eq!(value_order(&text("a"), &text("B")), Greater, "bytes");
    // Equality is by type and content: -0.0 and 0.0 are two keys.
    let mut store = ArrayStore::default();
    let map = store.create(-1, 0, Storage::Permanent);
    store.set(map, SfallValue::float(0.0), int(1));
    store.set(map, SfallValue::float(-0.0), int(2));
    store.set(map, nan.clone(), int(3));
    store.set(map, nan.clone(), int(4));
    assert_eq!(store.len(map), 3);
    assert_eq!(store.get(map, &nan), int(4));
}

#[test]
fn creation_flags_and_bad_sizes_follow_section_5_4() {
    let mut store = ArrayStore::default();
    assert_eq!(store.create(-2, 0, Storage::Permanent), 0);
    assert!(store.is_empty());
    let map = store.create(-1, 7, Storage::Permanent);
    store.set(map, int(1), int(0));
    assert_eq!(store.len(map), 0, "other flags give an ordinary map");
    let list = store.create(2, 2, Storage::Permanent);
    store.set(list, text("x"), int(1));
    store.set(list, int(0), int(0));
    assert_eq!(store.len(list), 2, "flags are ignored on a list");
    assert_eq!(store.key_at(list, -1), int(0));
    for size in [-1, -8, -100] {
        assert_eq!(store.resize(list, size, |_| Ok::<i32, ()>(0)), Ok(false));
    }
    // A bad ID never stops a script, and an ID is never reused.
    assert_eq!(store.len(99), -1);
    assert_eq!(store.scan(99, &int(1)), int(-1));
    assert_eq!(store.get(99, &int(1)), int(0));
    assert_eq!(store.key_at(99, 1), int(0));
    store.set(99, int(0), int(1));
    store.free(99);
    store.fix(99);
    store.save(text("k"), 99);
    assert_eq!(store.resize(99, 3, |_| Ok::<i32, ()>(0)), Ok(true));
    store.free(list);
    assert_eq!(store.create(0, 0, Storage::Permanent), list + 1);
    assert_eq!(store.len(0), -1, "0 is never an ID");
    // An expression on a freed array does nothing.
    let last = store.create(1, 0, Storage::Permanent);
    store.free(last);
    store.expression(int(0), int(9));
    assert_eq!(store.len(last), -1);
}

#[test]
fn a_shuffle_of_fewer_than_two_elements_draws_nothing() {
    let mut store = ArrayStore::default();
    let list = store.create(1, 0, Storage::Permanent);
    let mut draws = 0;
    store
        .resize(list, -5, |_| {
            draws += 1;
            Ok::<i32, ()>(0)
        })
        .unwrap();
    assert_eq!(draws, 0);
    let list = store.create(5, 0, Storage::Permanent);
    store
        .resize(list, -5, |_| {
            draws += 1;
            Ok::<i32, ()>(0)
        })
        .unwrap();
    assert_eq!(draws, 4, "length - 1 numbers");
}
