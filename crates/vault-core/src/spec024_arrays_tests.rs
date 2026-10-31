//! SPEC-024b §7.3 tests 3 to 6: every array vector of
//! `oracle/SPEC-024/sfall-array-vectors.tsv`, step by step, and the sidecar.
use crate::{
    WorldState,
    sfall::{GlobalKey, SfallWorld},
    state_hash::fnv1a64,
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

#[test]
fn t3_every_array_vector_gives_its_push_and_its_state() {
    let mut store = ArrayStore::default();
    let mut vector = String::new();
    let mut sidecar = Vec::new();
    let mut rows = 0;
    for line in VECTORS.lines().skip(1) {
        let cells: Vec<&str> = line.split('\t').collect();
        let [name, step_name, operation, push, state_after, _] = cells[..] else {
            panic!("{line}");
        };
        if !matches!(name, "A" | "B" | "C" | "D" | "E" | "F" | "F2" | "F3") {
            continue;
        }
        if name != vector {
            vector = name.to_owned();
            store = ArrayStore::default();
        }
        rows += 1;
        let world = f_world(store.clone());
        match step_name {
            "sidecar" => {
                let bytes = hex(push);
                assert_eq!(world.sidecar(), bytes, "{line}");
                assert_eq!(state_after, format!("bytes={}", bytes.len()));
                sidecar = bytes;
                continue;
            }
            "sidecar-fnv" => {
                assert_eq!(format!("{:016X}", fnv1a64(&sidecar)), push, "{line}");
                continue;
            }
            "stream" => {
                let streamed = if name == "F3" {
                    SfallWorld::default().extension_stream()
                } else {
                    world.extension_stream()
                };
                assert_eq!(streamed, hex(push), "{line}");
                continue;
            }
            "stream-after-step" => {
                let mut after = world;
                after.arrays.end_of_step();
                assert_eq!(after.extension_stream(), hex(push), "{line}");
                continue;
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
                assert_eq!(format!("{hash:016X}"), push, "{line}");
                continue;
            }
            _ => {}
        }
        if operation == "load the slot" {
            store = SfallWorld::from_sidecar(&sidecar).unwrap().arrays;
        } else {
            assert_eq!(step(&mut store, operation), push, "{line}");
        }
        // Vector D's rows show the array they touch; the rest show all.
        if name == "D" {
            let id: u32 = state_after[1..].split(' ').next().unwrap().parse().unwrap();
            let array = (&id, &store.arrays()[&id]);
            assert_eq!(shown_array(array), state_after, "{line}");
        } else {
            assert_eq!(state(&store), state_after, "{line}");
        }
    }
    assert_eq!(rows, 74);
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
