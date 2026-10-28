//! SPEC-021 §4 tests T1-T6, T8-T10 and T12 (T7 and T11 run on the
//! synthetic corpus in `tools/asset-audit/tests/spec021.rs`).
use crate::{
    CharacterSheet, ObjectRef, ParkMiller, WorldState,
    state_hash::{
        CombatRecord, EntityRecord, Fnv1a64, Handle, InventoryEntry, LvarSlice, Participant,
        ParticipantState, Position, RngRecord, StateSnapshot, fnv1a64,
    },
    traces::{
        CombatEvent, CombatTraceRow, VmTraceEnd, parse_hex_dump, read_combat_trace, trace_vm,
        write_combat_trace,
    },
    world::{WorldObject, object_record},
};
use std::collections::BTreeSet;
use vault_formats::map::{CritterState, ObjectState};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/spec021");

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{FIXTURES}/{name}")).unwrap()
}
fn hex_fixture(name: &str) -> Vec<u8> {
    parse_hex_dump(&fixture(name)).unwrap()
}
fn hex(value: u64) -> String {
    format!("{value:016X}")
}
#[track_caller]
fn assert_bytes(actual: &[u8], expected: &[u8], what: &str) {
    if actual != expected {
        let at = actual
            .iter()
            .zip(expected)
            .position(|(a, b)| a != b)
            .unwrap_or(actual.len().min(expected.len()));
        panic!(
            "{what}: {} bytes vs {} expected, first difference at {at:#05x}",
            actual.len(),
            expected.len()
        );
    }
}

/// SPEC-021 §5.2 the RNG entries of a zeroed generator seeded with 1.
const W0_ENTRIES: [i32; 32] = [
    0, 784575628, 2128236580, 1624379149, 1358580979, 704877633, 1580723810, 101929267, 563613512,
    1636807826, 1954899097, 1505795335, 893351816, 1404280278, 197493099, 1131570933, 1817129560,
    1998097157, 1264817709, 1474833169, 896544303, 143542612, 823378840, 16531729, 1441282327,
    1137522503, 114807987, 74243042, 1784484492, 1115438165, 823564440, 2007237709,
];
fn w0_rng() -> RngRecord {
    RngRecord {
        state: 784575628,
        selector: 0,
        entries: W0_ENTRIES,
    }
}
fn w3_rng() -> RngRecord {
    let mut rng = w0_rng();
    rng.state = 812987216;
    rng.entries[0] = 812987216;
    rng
}

/// A combat-list member: an entity of the model, or a handle no entity has.
#[derive(Debug, Clone, Copy)]
enum Who {
    Entity(usize),
    Stale(Handle),
}
#[derive(Debug, Clone)]
struct Combat {
    round: u32,
    current: Option<u32>,
    active: Vec<Who>,
    waiting: Vec<Who>,
}
/// A synthetic world as SPEC-021 §5 describes it; combat participants
/// read their values from the entity they name.
#[derive(Debug, Clone)]
struct Model {
    entities: Vec<EntityRecord>,
    rng: RngRecord,
    combat: Option<Combat>,
    gvars: Option<Vec<i32>>,
    mvars: Option<Vec<i32>>,
    lvars: Vec<LvarSlice>,
}
impl Model {
    fn participant(&self, who: Who) -> Participant {
        match who {
            Who::Entity(index) => {
                let entity = &self.entities[index];
                let critter = entity.critter();
                Participant {
                    handle: entity.handle,
                    live: Some(ParticipantState {
                        hp: if critter { entity.hp } else { 0 },
                        ap: if critter { entity.ap } else { 0 },
                        position: entity.position,
                        orientation: entity.orientation,
                    }),
                }
            }
            Who::Stale(handle) => Participant { handle, live: None },
        }
    }
    fn snapshot(&self) -> StateSnapshot {
        StateSnapshot {
            entities: self.entities.clone(),
            rng: self.rng,
            combat: self.combat.as_ref().map(|combat| CombatRecord {
                round: combat.round,
                current: combat.current,
                active: combat.active.iter().map(|w| self.participant(*w)).collect(),
                waiting: combat
                    .waiting
                    .iter()
                    .map(|w| self.participant(*w))
                    .collect(),
            }),
            gvars: self.gvars.clone(),
            mvars: self.mvars.clone(),
            lvars: self.lvars.clone(),
        }
    }
}
fn w0() -> Model {
    Model {
        entities: vec![
            EntityRecord {
                handle: Handle::new(1, 1),
                pid: 0x01000001,
                position: Some(Position {
                    elevation: 0,
                    hex: 20100,
                }),
                orientation: 2,
                hp: 30,
                ap: 8,
                inventory: vec![InventoryEntry {
                    pid: 4,
                    quantity: 3,
                }],
            },
            EntityRecord {
                handle: Handle::new(2, 3),
                pid: 0x01000002,
                position: Some(Position {
                    elevation: 0,
                    hex: 20102,
                }),
                orientation: 5,
                hp: 12,
                ap: 6,
                inventory: Vec::new(),
            },
        ],
        rng: w0_rng(),
        combat: None,
        gvars: Some(vec![7]),
        mvars: None,
        lvars: Vec::new(),
    }
}
fn w1() -> Model {
    let mut model = w0();
    model.combat = Some(Combat {
        round: 0,
        current: Some(0),
        active: vec![Who::Entity(0), Who::Entity(1)],
        waiting: Vec::new(),
    });
    model
}
fn w2() -> Model {
    let mut model = w0();
    model.entities.reverse();
    model.mvars = Some(vec![1, -1]);
    model.lvars = vec![
        LvarSlice {
            script: 0x04000002,
            values: vec![5],
        },
        LvarSlice {
            script: 0x01000007,
            values: vec![0, 9],
        },
    ];
    model
}
fn w3() -> Model {
    let mut model = w0();
    model.rng = w3_rng();
    model
}
fn w4() -> Model {
    let mut model = w1();
    let combat = model.combat.as_mut().unwrap();
    combat.round = 1;
    combat.current = None;
    combat.waiting = vec![Who::Stale(Handle::new(5, 1))];
    model
}
/// The model a `hashes.tsv` vector names.
fn vector(name: &str) -> Model {
    match name {
        "W0" => w0(),
        "W1" => w1(),
        "W2" => w2(),
        "W3" => w3(),
        "W4" => w4(),
        "M-gvar" => {
            let mut m = w0();
            m.gvars = Some(vec![6]);
            m
        }
        "M-gen" => {
            let mut m = w0();
            m.entities[1].handle.generation = 2;
            m
        }
        "M-hp" => {
            let mut m = w0();
            m.entities[0].hp = 31;
            m
        }
        "M-hex" => {
            let mut m = w0();
            m.entities[0].position.as_mut().unwrap().hex = 20101;
            m
        }
        "M-inv" => {
            let mut m = w0();
            m.entities[0].inventory[0].quantity = 2;
            m
        }
        "M-rng" => {
            let mut m = w0();
            m.rng.entries[0] = 1;
            m
        }
        "M-turn" => {
            let mut m = w1();
            m.combat.as_mut().unwrap().current = Some(1);
            m
        }
        "M-order" => {
            let mut m = w1();
            m.combat.as_mut().unwrap().active.reverse();
            m
        }
        other => panic!("unknown vector {other}"),
    }
}

/// T1.
#[test]
fn fnv1a64_reproduces_the_known_answers_and_the_step_table() {
    let table = fixture("fnv1a64.tsv");
    let mut lines = table.lines();
    assert_eq!(lines.next(), Some("input_hex\tfnv1a64"));
    let mut rows = 0;
    for line in lines {
        let (input, expected) = line.split_once('\t').unwrap();
        let bytes: Vec<u8> = if input == "-" {
            Vec::new()
        } else {
            (0..input.len())
                .step_by(2)
                .map(|at| u8::from_str_radix(&input[at..at + 2], 16).unwrap())
                .collect()
        };
        assert_eq!(hex(fnv1a64(&bytes)), expected, "input {input}");
        rows += 1;
    }
    assert_eq!(rows, 4);
    // §5.1: after XOR, then after the multiplication, byte by byte.
    let steps: [(u8, u64, u64); 9] = [
        (0x56, 0xCBF29CE484222373, 0xAF640B4C86023C69),
        (0x43, 0xAF640B4C86023C2A, 0x093B5D07B5CC3B5E),
        (0x48, 0x093B5D07B5CC3B16, 0x7C1A2A19EA086662),
        (0x31, 0x7C1A2A19EA086653, 0xE8DBDD08AC45DF09),
        (0x43, 0xE8DBDD08AC45DF4A, 0xF377DFBCBABA6ABE),
        (0x4D, 0xF377DFBCBABA6AF3, 0x6F1C20B14AC3BAE9),
        (0x42, 0x6F1C20B14AC3BAAB, 0x908638420A963091),
        (0x54, 0x908638420A9630C5, 0x2A425D37FD34DEBF),
        (0x00, 0x2A425D37FD34DEBF, 0x03A3252340D67E8D),
    ];
    let mut hash = Fnv1a64::new();
    assert_eq!(hash.finish(), 0xCBF29CE484222325);
    for (byte, xored, multiplied) in steps {
        assert_eq!(hash.finish() ^ u64::from(byte), xored);
        hash.write(&[byte]);
        assert_eq!(hash.finish(), multiplied);
    }
}

/// T2: every stream file and every `hashes.tsv` row.
#[test]
fn the_synthetic_vectors_encode_and_hash_as_recorded() {
    for (name, model) in [
        ("w0", w0()),
        ("w1", w1()),
        ("w2", w2()),
        ("w3", w3()),
        ("w4", w4()),
    ] {
        let snapshot = model.snapshot();
        assert_bytes(
            &snapshot.canonical_bytes(),
            &hex_fixture(&format!("{name}.stream.hex")),
            name,
        );
        assert_eq!(
            fnv1a64(&snapshot.canonical_bytes()),
            snapshot.state_hash(),
            "{name}"
        );
        assert_eq!(
            fnv1a64(&snapshot.canonical_combat_bytes()),
            snapshot.combat_hash(),
            "{name}"
        );
    }
    for (name, model) in [("w1", w1()), ("w4", w4())] {
        assert_bytes(
            &model.snapshot().canonical_combat_bytes(),
            &hex_fixture(&format!("{name}.combat.hex")),
            name,
        );
    }
    assert_eq!(
        w0().snapshot().canonical_combat_bytes(),
        [0x56, 0x43, 0x48, 0x31, 0x43, 0x4D, 0x42, 0x54, 0x00]
    );
    let table = fixture("hashes.tsv");
    let mut lines = table.lines();
    assert_eq!(
        lines.next(),
        Some("vector\tbase\tchange\tstream_bytes\tstate_hash\tcombat_bytes\tcombat_hash")
    );
    let mut rows = 0;
    for line in lines {
        let fields: Vec<&str> = line.split('\t').collect();
        let snapshot = vector(fields[0]).snapshot();
        assert_eq!(
            snapshot.canonical_bytes().len().to_string(),
            fields[3],
            "{line}"
        );
        assert_eq!(hex(snapshot.state_hash()), fields[4], "{line}");
        assert_eq!(
            snapshot.canonical_combat_bytes().len().to_string(),
            fields[5],
            "{line}"
        );
        assert_eq!(hex(snapshot.combat_hash()), fields[6], "{line}");
        rows += 1;
    }
    assert_eq!(rows, 13);
}

/// T3: the encoder sorts entities, inventory entries and LVAR slices.
#[test]
fn keyed_collections_encode_in_sorted_order_whatever_the_input_order() {
    let expected = hex_fixture("w2.stream.hex");
    let mut model = w2();
    assert_bytes(&model.snapshot().canonical_bytes(), &expected, "w2");
    model.entities.reverse();
    model.lvars.reverse();
    assert_bytes(&model.snapshot().canonical_bytes(), &expected, "w2 sorted");
    let mut held = w0();
    held.entities[0].inventory = vec![
        InventoryEntry {
            pid: 9,
            quantity: 1,
        },
        InventoryEntry {
            pid: 4,
            quantity: 3,
        },
        InventoryEntry {
            pid: 4,
            quantity: 2,
        },
    ];
    let first = held.snapshot().state_hash();
    held.entities[0].inventory.rotate_left(1);
    assert_eq!(held.snapshot().state_hash(), first);
    held.entities[0].inventory.swap(0, 2);
    assert_eq!(held.snapshot().state_hash(), first);
    // Duplicate script identifiers are ordered by their elements.
    let mut duplicates = w0();
    duplicates.lvars = vec![
        LvarSlice {
            script: 0x01000001,
            values: vec![2],
        },
        LvarSlice {
            script: 0x01000001,
            values: vec![1, 5],
        },
        LvarSlice {
            script: 0x01000001,
            values: vec![1],
        },
    ];
    let first = duplicates.snapshot().canonical_bytes();
    duplicates.lvars.reverse();
    assert_eq!(duplicates.snapshot().canonical_bytes(), first);
    let tail = &first[first.len() - 44..];
    assert_eq!(
        tail,
        &[
            3_u8, 0, 0, 0, // three slices
            1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, // [1]
            1, 0, 0, 1, 2, 0, 0, 0, 1, 0, 0, 0, 5, 0, 0, 0, // [1, 5]
            1, 0, 0, 1, 1, 0, 0, 0, 2, 0, 0, 0, // [2]
        ]
    );
}

fn flip_u32(value: &mut u32, bit: u32) {
    *value ^= 1 << bit;
}
fn flip_i32(value: &mut i32, bit: u32) {
    *value ^= (1_u32 << bit) as i32;
}
/// One flip: its name, whether it is a combat field (or a participant's
/// HP, AP, elevation, hex or orientation), and the flipped model.
type Flip = (String, bool, Model);
/// T4's flips of every fixed-width covered scalar.
fn flips(base: &Model) -> Vec<Flip> {
    let mut out: Vec<Flip> = Vec::new();
    let in_combat = |model: &Model, entity: usize| {
        model.combat.as_ref().is_some_and(|combat| {
            combat
                .active
                .iter()
                .chain(&combat.waiting)
                .any(|who| matches!(who, Who::Entity(e) if *e == entity))
        })
    };
    for entity in 0..base.entities.len() {
        let participant = in_combat(base, entity);
        for bit in 0..32 {
            let mut push = |field: &str, combat: bool, change: &dyn Fn(&mut EntityRecord)| {
                let mut model = base.clone();
                change(&mut model.entities[entity]);
                out.push((format!("entity {entity} {field} bit {bit}"), combat, model));
            };
            push("index", false, &|e| flip_u32(&mut e.handle.index, bit));
            push("generation", false, &|e| {
                flip_u32(&mut e.handle.generation, bit)
            });
            if bit < 24 {
                push("pid", false, &|e| flip_u32(&mut e.pid, bit));
            }
            push("elevation", participant, &|e| {
                flip_u32(&mut e.position.as_mut().unwrap().elevation, bit)
            });
            push("hex", participant, &|e| {
                flip_u32(&mut e.position.as_mut().unwrap().hex, bit)
            });
            push("orientation", participant, &|e| {
                flip_u32(&mut e.orientation, bit)
            });
            push("hp", participant, &|e| flip_i32(&mut e.hp, bit));
            push("ap", participant, &|e| flip_i32(&mut e.ap, bit));
            for item in 0..base.entities[entity].inventory.len() {
                push(&format!("item {item} pid"), false, &|e| {
                    flip_u32(&mut e.inventory[item].pid, bit)
                });
                push(&format!("item {item} quantity"), false, &|e| {
                    flip_u32(&mut e.inventory[item].quantity, bit)
                });
            }
        }
    }
    for bit in 0..32 {
        let mut push = |field: String, combat: bool, change: &dyn Fn(&mut Model)| {
            let mut model = base.clone();
            change(&mut model);
            out.push((format!("{field} bit {bit}"), combat, model));
        };
        push("rng state".into(), false, &|m| {
            flip_i32(&mut m.rng.state, bit)
        });
        push("rng selector".into(), false, &|m| {
            flip_i32(&mut m.rng.selector, bit)
        });
        for entry in 0..32 {
            push(format!("rng entry {entry}"), false, &|m| {
                flip_i32(&mut m.rng.entries[entry], bit)
            });
        }
        for index in 0..base.gvars.as_ref().map_or(0, Vec::len) {
            push(format!("gvar {index}"), false, &|m| {
                flip_i32(&mut m.gvars.as_mut().unwrap()[index], bit)
            });
        }
        if base.combat.is_some() {
            push("round".into(), true, &|m| {
                flip_u32(&mut m.combat.as_mut().unwrap().round, bit)
            });
            push("current turn".into(), true, &|m| {
                flip_u32(m.combat.as_mut().unwrap().current.as_mut().unwrap(), bit)
            });
        }
    }
    out
}
/// T4: every single-bit flip changes the state hash, all differently;
/// combat flips change the combat hash; every opt toggle changes the hash.
#[test]
fn every_single_bit_flip_of_a_covered_scalar_changes_the_hash() {
    for (name, base, expected) in [("W0", w0(), 1680), ("W1", w1(), 1744)] {
        let snapshot = base.snapshot();
        let (state, combat) = (snapshot.state_hash(), snapshot.combat_hash());
        let flips = flips(&base);
        assert_eq!(flips.len(), expected, "{name}");
        let mut seen = BTreeSet::from([state]);
        for (field, combat_field, model) in &flips {
            let flipped = model.snapshot();
            assert_eq!(
                flipped.canonical_bytes().len(),
                snapshot.canonical_bytes().len(),
                "{name} {field}"
            );
            assert!(
                seen.insert(flipped.state_hash()),
                "{name} {field}: state hash repeats"
            );
            if *combat_field {
                assert_ne!(flipped.combat_hash(), combat, "{name} {field}");
            }
        }
        // Each opt field toggled once.
        let mut toggles: Vec<(String, Model)> = Vec::new();
        for entity in 0..base.entities.len() {
            let mut model = base.clone();
            model.entities[entity].position = None;
            toggles.push((format!("entity {entity} position"), model));
            // HP and AP are present exactly for critters.
            let mut model = base.clone();
            model.entities[entity].pid &= 0x00FF_FFFF;
            toggles.push((format!("entity {entity} critter"), model));
        }
        let mut model = base.clone();
        model.gvars = None;
        toggles.push(("gvar".into(), model));
        let mut model = base.clone();
        model.mvars = Some(Vec::new());
        toggles.push(("mvar".into(), model));
        let mut model = base.clone();
        if base.combat.is_some() {
            model.combat = None;
            toggles.push(("combat".into(), model));
            let mut model = base.clone();
            model.combat.as_mut().unwrap().current = None;
            toggles.push(("current turn".into(), model));
        } else {
            model.combat = w1().combat;
            toggles.push(("combat".into(), model));
        }
        for (field, model) in toggles {
            let toggled = model.snapshot();
            assert_ne!(toggled.state_hash(), state, "{name} {field}");
            if field == "combat" || field == "current turn" {
                assert_ne!(toggled.combat_hash(), combat, "{name} {field}");
            }
        }
    }
}

/// A critter object at `hex` with the given facing, HP and AP.
fn critter(pid: u32, hex: i32, orientation: u8, hp: i32, ap: i32) -> WorldObject {
    let mut raw = object_record(pid, 0);
    raw.hex = Some(hex);
    raw.orientation = orientation;
    raw.state = ObjectState::Critter(CritterState {
        reaction: 0,
        damage: 0,
        maneuvers: 0,
        ap,
        results: vault_formats::pro::CombatFlags(0),
        ai: 0,
        team: 0,
        attacker: -1,
        hp,
        radiation: 0,
        poison: 0,
    });
    let sheet = CharacterSheet {
        current_hp: hp,
        ..CharacterSheet::default()
    };
    WorldObject {
        raw,
        prototype: None,
        character: Some(sheet),
        name: Vec::new(),
        description: Vec::new(),
        owner: None,
        inventory: Vec::new(),
        money: None,
        active: true,
        opponent: 0,
    }
}
fn item(pid: u32) -> WorldObject {
    WorldObject {
        raw: object_record(pid, 0),
        prototype: None,
        character: None,
        name: Vec::new(),
        description: Vec::new(),
        owner: None,
        inventory: Vec::new(),
        money: None,
        active: true,
        opponent: 0,
    }
}
/// T5's world: critters `1:1` and `2:3`, the first holding three of an
/// item (`3:1`), one GVAR, two MVARs on a loaded map, two scripts with
/// locals.
struct Built {
    world: WorldState,
    first: ObjectRef,
    second: ObjectRef,
    held: ObjectRef,
    scripts: [u32; 2],
}
fn built() -> Built {
    let mut world = WorldState::empty(1).unwrap();
    let first = world.insert(critter(0x01000001, 20100, 2, 30, 8)).unwrap();
    let spare = world.insert(item(0x00000009)).unwrap();
    world.erase(spare);
    let second = world.insert(critter(0x01000002, 20102, 5, 12, 6)).unwrap();
    let held = world.insert(item(0x00000004)).unwrap();
    world.get_mut(held).unwrap().owner = Some(first);
    world.get_mut(first).unwrap().inventory.push((held, 3));
    world.gvars = Some(vec![7]);
    world.persistence.current_map = Some("SYNTH.SAV".into());
    world.mvars = vec![1, -1];
    let a = world.add_script(first, 1, 1, 2).unwrap();
    let b = world.add_script(second, 2, 4, 1).unwrap();
    assert!(world.set_local(Some(b), 0, 5));
    assert!(world.set_local(Some(a), 1, 9));
    Built {
        world,
        first,
        second,
        held,
        scripts: [a, b],
    }
}
/// T5.
#[test]
fn the_snapshot_extracts_the_values_set_through_the_engine() {
    let Built {
        world,
        first,
        second,
        held,
        scripts: [a, b],
    } = built();
    let handle = |reference| crate::state_hash::Handle::of(world.entity_key(reference).unwrap());
    assert_eq!(handle(first), Handle::new(1, 1));
    assert_eq!(handle(second), Handle::new(2, 3));
    assert_eq!(handle(held), Handle::new(3, 1));
    assert_eq!(a, 0x01000001);
    assert_eq!(b, 0x04000001);
    let mut expected = w0().snapshot();
    expected.mvars = Some(vec![1, -1]);
    expected.entities.push(EntityRecord {
        handle: Handle::new(3, 1),
        pid: 4,
        position: None,
        orientation: 0,
        hp: 0,
        ap: 0,
        inventory: Vec::new(),
    });
    expected.lvars = vec![
        LvarSlice {
            script: a,
            values: vec![0, 9],
        },
        LvarSlice {
            script: b,
            values: vec![5],
        },
    ];
    let snapshot = world.state_snapshot();
    assert_eq!(snapshot, expected);
    assert_eq!(world.state_hash(), expected.state_hash());
    assert_eq!(world.state_hash(), fnv1a64(&snapshot.canonical_bytes()));
    assert_eq!(world.combat_hash(), 0x03A3252340D67E8D);
    // Hashing changes nothing and repeats.
    assert_eq!(world.state_hash(), world.state_hash());
    assert_eq!(world.rng.state(), ParkMiller::seeded(1).unwrap().state());
    // No map loaded: the MVAR part is absent.
    let mut unloaded = built().world;
    unloaded.persistence.current_map = None;
    assert_eq!(unloaded.state_snapshot().mvars, None);
    unloaded.persistence.current_map = Some("SYNTH.SAV".into());
    unloaded.unloaded = true;
    assert_eq!(unloaded.state_snapshot().mvars, None);
}
/// T3: LVAR slices allocated in two different orders hash the same.
#[test]
fn script_insertion_order_does_not_change_the_hash() {
    let Built {
        world: forward,
        scripts: [a, b],
        ..
    } = built();
    let mut reversed = built().world;
    let (first, second) = (
        reversed.scripts.remove(&a).unwrap(),
        reversed.scripts.remove(&b).unwrap(),
    );
    reversed.lvars.clear();
    // The forward world allocated `b`'s slice first; this one `a`'s.
    for mut script in [first, second] {
        let id = script.id;
        script.local_base = -1;
        reversed.scripts.insert(id, script);
        reversed.ensure_locals(id).unwrap();
    }
    assert!(reversed.set_local(Some(b), 0, 5));
    assert!(reversed.set_local(Some(a), 1, 9));
    assert_ne!(
        reversed.scripts[&a].local_base,
        forward.scripts[&a].local_base
    );
    assert_eq!(reversed.state_snapshot(), forward.state_snapshot());
    assert_eq!(reversed.state_hash(), forward.state_hash());
}
/// T6.
#[test]
fn a_generator_seeded_with_one_snapshots_to_the_w0_and_w3_records() {
    let mut rng = ParkMiller::seeded(1).unwrap();
    assert_eq!(RngRecord::from(rng.state()), w0_rng());
    let stream = hex_fixture("w0.stream.hex");
    let mut world = WorldState::empty(1).unwrap();
    world.gvars = Some(vec![7]);
    let bytes = world.state_snapshot().canonical_bytes();
    // No entities here: the RNG part starts right after an empty ENTS.
    assert_eq!(&bytes[12..16], b"RNGS");
    assert_eq!(&bytes[16..16 + 136], &stream[0x66..0xEE]);
    assert_eq!(rng.draw(), 0);
    assert_eq!(RngRecord::from(rng.state()), w3_rng());
    world.rng.draw();
    let bytes = world.state_snapshot().canonical_bytes();
    assert_eq!(
        &bytes[16..16 + 136],
        &hex_fixture("w3.stream.hex")[0x66..0xEE]
    );
}

/// T8.
#[test]
fn a_one_bit_change_through_the_engine_changes_the_hash() {
    type Change = fn(&mut Built);
    let changes: [(&str, Change); 9] = [
        ("gvar", |b| b.world.gvars.as_mut().unwrap()[0] ^= 1),
        ("mvar", |b| b.world.mvars[1] ^= 1),
        ("lvar", |b| {
            let value = b.world.local(Some(b.scripts[0]), 1);
            assert!(b.world.set_local(Some(b.scripts[0]), 1, value ^ 1));
        }),
        ("hp", |b| {
            let hp = b.world.hit_points(b.first).unwrap();
            b.world.set_hit_points(b.first, hp ^ 1);
        }),
        ("ap", |b| {
            let ap = b.world.critter_state(b.second).unwrap().ap;
            b.world.set_ap(b.second, ap ^ 1);
        }),
        ("hex", |b| {
            let object = b.world.get_mut(b.first).unwrap();
            object.raw.hex = object.raw.hex.map(|hex| hex ^ 1);
        }),
        ("orientation", |b| {
            b.world.get_mut(b.second).unwrap().raw.orientation ^= 1;
        }),
        ("inventory", |b| {
            b.world.get_mut(b.first).unwrap().inventory[0].1 ^= 1;
        }),
        ("rng", |b| {
            b.world.rng.draw();
        }),
    ];
    let base = built().world.state_hash();
    for (name, change) in changes {
        let mut world = built();
        change(&mut world);
        assert_ne!(world.world.state_hash(), base, "{name}");
    }
    let mut world = built();
    world.world.clock.advance(600).unwrap();
    world.world.camera_hex += 1;
    world.world.elapsed_ms += 16;
    world.world.combat_speed = 50;
    assert_eq!(world.world.state_hash(), base);
}

/// The combat record follows the world's combat state, with an
/// unresolved reference written as a stale participant.
#[test]
fn the_combat_record_reads_the_live_combat_state() {
    let Built {
        mut world,
        first,
        second,
        ..
    } = built();
    world.combat_state.flags |= crate::critical::state::ACTIVE;
    world.combat_state.list = vec![first, second, 0x0FFF_FFFF];
    world.combat_state.active = 2;
    world.combat_state.round = 1;
    world.combat_actor = second;
    let combat = world.state_snapshot().combat.unwrap();
    assert_eq!(combat.round, 1);
    assert_eq!(combat.current, Some(1));
    assert_eq!(combat.active[0].handle, Handle::new(1, 1));
    assert_eq!(
        combat.active[0].live,
        Some(ParticipantState {
            hp: 30,
            ap: 8,
            position: Some(Position {
                elevation: 0,
                hex: 20100
            }),
            orientation: 2,
        })
    );
    assert_eq!(
        combat.waiting,
        vec![Participant {
            handle: Handle::new(0x0FFF_FFFF, 0),
            live: None,
        }]
    );
    assert_eq!(
        world.combat_hash(),
        world.state_snapshot().combat_hash(),
        "the direct and the snapshot combat hash agree"
    );
    world.combat_actor = 0x0FFF_FFFF;
    assert_eq!(world.state_snapshot().combat.unwrap().current, None);
}

fn vm_trace(bytes: &[u8]) -> crate::traces::VmTrace {
    let program = vault_vm::IntProgram::parse(bytes).unwrap();
    let mut vm = vault_vm::Vm::new(program);
    let mut host = vault_vm::BasicHost::default();
    trace_vm(&mut vm, &mut host, "program.int", 100_000)
}
/// T9 in process (the binary is checked in `asset-audit`'s tests).
#[test]
fn the_vm_traces_of_the_spec004_program_match_the_goldens() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/vm/program.int.hex"
    );
    assert_eq!(crate::traces::trace_program_name(path), "program.int");
    assert_eq!(
        crate::traces::trace_program_name("scripts/Foo.INT"),
        "foo.int"
    );
    let mut bytes = parse_hex_dump(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(bytes.len(), 282);
    let trace = vm_trace(&bytes);
    assert_eq!(trace.end, VmTraceEnd::Ended);
    assert_eq!(trace.text(), fixture("vm-trace-program.tsv"));
    bytes[0xAE..0xB2].copy_from_slice(&[0, 0, 0, 1]);
    let trace = vm_trace(&bytes);
    assert_eq!(trace.end, VmTraceEnd::Ended);
    assert_eq!(trace.text(), fixture("vm-trace-program-branch.tsv"));
    // The step bound ends the trace without an extra row.
    let program = vault_vm::IntProgram::parse(&bytes).unwrap();
    let mut vm = vault_vm::Vm::new(program);
    let limited = trace_vm(&mut vm, &mut vault_vm::BasicHost::default(), "p", 5);
    assert_eq!(limited.rows.len(), 5);
    assert_eq!(limited.end, VmTraceEnd::StepLimit);
    assert_eq!(limited.end.exit_code(), 2);
}

/// T10.
#[test]
fn the_combat_trace_example_is_written_and_read_back() {
    let s0 = {
        let mut m = w1();
        m.combat.as_mut().unwrap().current = None;
        m
    };
    let s1 = {
        let mut m = w1();
        let e = &mut m.entities[0];
        e.position.as_mut().unwrap().hex = 20101;
        e.orientation = 1;
        e.ap = 0;
        m
    };
    let s2 = {
        let mut m = s1.clone();
        m.combat.as_mut().unwrap().current = Some(1);
        m.entities[0].hp = 26;
        m.entities[1].ap = 0;
        m.rng = w3_rng();
        m
    };
    let s3 = {
        let mut m = s2.clone();
        m.combat = None;
        m.entities[0].ap = 8;
        m
    };
    let rows = [
        CombatTraceRow::from_snapshot(0, CombatEvent::Start, None, &s0.snapshot()),
        CombatTraceRow::from_snapshot(
            1,
            CombatEvent::Turn,
            Some(Handle::new(1, 1)),
            &s1.snapshot(),
        ),
        CombatTraceRow::from_snapshot(
            2,
            CombatEvent::Turn,
            Some(Handle::new(2, 3)),
            &s2.snapshot(),
        ),
        CombatTraceRow::from_snapshot(3, CombatEvent::End, None, &s3.snapshot()),
    ];
    let text = write_combat_trace(&rows);
    assert_eq!(text, fixture("combat-trace-example.tsv"));
    assert_eq!(read_combat_trace(&text).unwrap(), rows);
    assert!(read_combat_trace(text.trim_end()).is_err());
    assert!(read_combat_trace(&text.replace("start", "begin")).is_err());
}

/// T12: the stream module uses no unstable hashing.
#[test]
fn the_stream_module_uses_no_std_hash_item_or_hash_collection() {
    let source = include_str!("state_hash.rs");
    let code: String = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for banned in [
        "std::hash",
        "HashMap",
        "HashSet",
        "DefaultHasher",
        "RandomState",
        "BuildHasher",
    ] {
        assert!(!code.contains(banned), "state_hash.rs uses {banned}");
    }
}
