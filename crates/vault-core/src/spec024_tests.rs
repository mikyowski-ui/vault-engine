//! SPEC-024b section 7 and SPEC-017 §7.14: sfall mode under the host, on the
//! oracle tables and the stack vectors.
use crate::{
    CharacterSheet, GameProfile, WorldEffect, WorldHost, WorldState,
    fallout2::Fallout2World,
    fixture_state as fixture,
    sfall_ini::IniFiles,
    spec017_tests::tables,
    test_support::{TestAssets, npc_prototype},
    world::ObjectRef,
};
use std::sync::{Arc, Mutex};
use vault_formats::map::{ObjectDetail, ObjectState};
use vault_vm::{IntProgram, RunOutcome, Value, Vm, VmError, WordSet, sfall};

const ORACLE: &str = include_str!("../../../oracle/SPEC-024/sfall-opcodes.tsv");

pub(crate) fn sfall_host() -> (WorldHost<TestAssets>, ObjectRef) {
    let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let assets = TestAssets {
        profile: GameProfile::Fallout2,
        ..TestAssets::all_art()
    };
    let mut host = WorldHost::new(world, assets);
    host.world.fallout2 = Some(Box::new(Fallout2World::new_game(tables(), -1)));
    host.enable_sfall().unwrap();
    let me = host.world.spawn(npc_prototype(), 20000, 0).unwrap();
    host.world.effects.clear();
    (host, me)
}

/// Words pushed by a fragment: integers, strings of the base program, and
/// raw words.
#[derive(Clone)]
pub(crate) enum Op {
    I(i32),
    F(f32),
    S(&'static [u8]),
    W(u16),
}
pub(crate) use Op::{F, I, S, W};

pub(crate) struct Run {
    pub vm: Vm,
    pub result: Result<RunOutcome, VmError>,
}
impl Run {
    pub fn stack(&self) -> Vec<Value> {
        let mut stack = self.vm.value_stack.clone();
        let mut values = Vec::new();
        while let Ok(value) = stack.pop() {
            values.push(value);
        }
        values.reverse();
        values
    }
    pub fn ints(&self) -> Vec<i32> {
        self.stack()
            .iter()
            .map(|value| value.payload as i32)
            .collect()
    }
    pub fn text(&self) -> Vec<u8> {
        let top = *self.stack().last().unwrap();
        self.vm.string(top).unwrap().to_vec()
    }
    pub fn float(&self) -> f32 {
        f32::from_bits(self.stack().last().unwrap().payload)
    }
}

/// The strings a fragment may push.
fn base(strings: &[&[u8]]) -> IntProgram {
    IntProgram::parse_in(
        &fixture::program(strings, &[]),
        &WordSet::FALLOUT2_SFALL,
        false,
    )
    .unwrap()
}
fn literal_offset(program: &IntProgram, text: &[u8]) -> u32 {
    (0..0x4000)
        .find(|offset| program.literal(*offset) == Some(text))
        .unwrap_or_else(|| panic!("no literal {text:?}"))
}
pub(crate) fn run(host: &mut WorldHost<TestAssets>, ops: &[Op]) -> Run {
    let strings: Vec<&[u8]> = ops
        .iter()
        .filter_map(|op| match op {
            Op::S(text) => Some(*text),
            _ => None,
        })
        .collect();
    let program = base(&strings);
    let mut bytes = Vec::new();
    for op in ops {
        match op {
            Op::I(value) => {
                bytes.extend([0xc0, 0x01]);
                bytes.extend(value.to_be_bytes());
            }
            Op::F(value) => {
                bytes.extend([0xa0, 0x01]);
                bytes.extend(value.to_bits().to_be_bytes());
            }
            Op::S(text) => {
                bytes.extend([0x90, 0x01]);
                bytes.extend(literal_offset(&program, text).to_be_bytes());
            }
            Op::W(word) => bytes.extend(word.to_be_bytes()),
        }
    }
    let mut vm = Vm::fragment(program, &bytes).unwrap();
    vm.id = 50;
    vm.intern(b"").unwrap();
    let result = vm.run(host, 100_000);
    Run { vm, result }
}
fn diagnostics(host: &mut WorldHost<TestAssets>) -> Vec<String> {
    host.world
        .effects
        .drain(..)
        .filter_map(|effect| match effect {
            WorldEffect::Diagnostic(text) => Some(text),
            _ => None,
        })
        .collect()
}

#[test]
fn every_row_pops_and_pushes_as_the_oracle_says_and_tier_r_fails_closed() {
    let mut rows = 0;
    for line in ORACLE.lines().skip(1) {
        let cells: Vec<&str> = line.split('\t').collect();
        let (kind, name, tier) = (cells[0], cells[2], cells[9]);
        let word = u16::from_str_radix(&cells[3][2..], 16).unwrap();
        let (mut host, _) = sfall_host();
        let mut ops = vec![I(7777)];
        if tier == "R" {
            ops.push(W(word));
            let done = run(&mut host, &ops);
            assert_eq!(done.result, Err(VmError::Opcode(word)), "{line}");
            assert_eq!(done.ints(), [7777], "{line}");
            rows += 1;
            continue;
        }
        let pops: usize = cells[4].parse().unwrap();
        let name: &'static [u8] = Box::leak(name.as_bytes().to_vec().into_boxed_slice());
        match kind {
            "word" => ops.extend(std::iter::repeat_n(I(1), pops)),
            "sfall_func" => {
                ops.push(S(name));
                ops.extend(std::iter::repeat_n(I(1), pops - 1));
            }
            // The sub-function operand comes first.
            _ => {
                ops.push(I(cells[1].parse().unwrap()));
                ops.extend(std::iter::repeat_n(I(0), pops - 1));
            }
        }
        ops.push(W(word));
        let done = run(&mut host, &ops);
        assert_eq!(done.result, Ok(RunOutcome::FragmentEnd), "{line}");
        let push: usize = cells[5].parse().unwrap();
        assert_eq!(done.vm.value_stack.byte_len(), 6 * (1 + push), "{line}");
        assert_eq!(done.ints()[0], 7777, "{line}");
        rows += 1;
    }
    assert_eq!(rows, 449);
}

#[test]
fn the_word_sets_load_as_vector_p_says() {
    let load = |word: u16, set: &WordSet, enhanced: bool| {
        let mut bytes = fixture::program(&[], &[(b"body", word.to_be_bytes().to_vec())]);
        bytes.extend(0x8000u16.to_be_bytes());
        IntProgram::parse_in(&bytes, set, enhanced).is_ok()
    };
    // Word, off, off enhanced, on, on enhanced.
    for (word, off, off_enhanced, on, on_enhanced) in [
        (0x8155, true, true, true, true),
        (0x8156, true, true, true, true),
        (0x8183, true, true, true, true),
        (0x8184, false, false, true, true),
        (0x81ff, false, false, true, true),
        (0x8200, false, true, true, true),
        (0x8213, false, true, true, true),
        (0x8281, false, true, true, true),
        (0x8282, false, true, false, true),
    ] {
        assert_eq!(load(word, &WordSet::FALLOUT2, false), off, "{word:#x}");
        assert_eq!(
            load(word, &WordSet::FALLOUT2, true),
            off_enhanced,
            "{word:#x}"
        );
        assert_eq!(load(word, &WordSet::FALLOUT2_SFALL, false), on, "{word:#x}");
        assert_eq!(
            load(word, &WordSet::FALLOUT2_SFALL, true),
            on_enhanced,
            "{word:#x}"
        );
    }
}

#[test]
fn sfall_mode_is_refused_under_fallout1() {
    let world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    let mut host = WorldHost::new(world, TestAssets::all_art());
    let error = host.enable_sfall().unwrap_err();
    assert!(error.to_string().contains("fallout2"), "{error}");
    assert!(!host.sfall_mode());
    assert!(host.world.sfall.is_none());
}

#[test]
fn vector_p_routes_the_overlap() {
    // Fragment 1: `read_byte` is tier X, popping I 1 and pushing I 0.
    let (mut host, me) = sfall_host();
    let mut vm_self = vec![W(0x80bc), I(13), I(1), W(0x8156)];
    let script = host.world.add_script(me, 0, 4, 0).unwrap();
    let mut vm = Vm::fragment(base(&[]), &[]).unwrap();
    vm.id = 50;
    host.bind_program(&vm, script);
    let done = run(&mut host, &vm_self);
    assert_eq!(done.ints(), [me as i32, 13, 0]);
    vm_self.clear();
    // Fragment 2: `fs_size` is tier P2.
    let done = run(&mut host, &[I(7), W(0x8200)]);
    assert_eq!(done.ints(), [0]);
    // Fragment 3: `sfall_ver_major` pops nothing.
    let done = run(&mut host, &[I(100), I(0), W(0x8210)]);
    assert_eq!(done.ints(), [100, 0, 4]);
    // The enhanced preset changes neither.
    host.world.rules.preset = crate::EnginePreset::Enhanced;
    assert_eq!(run(&mut host, &[I(7), W(0x8200)]).ints(), [0]);
    assert_eq!(
        run(&mut host, &[I(100), I(0), W(0x8210)]).ints(),
        [100, 0, 4]
    );
    assert_eq!(run(&mut host, &[W(0x8211), W(0x8212)]).ints(), [5, 1]);
}

#[test]
fn the_stack_vectors_of_section_7_1() {
    let (mut host, _) = sfall_host();
    assert_eq!(
        run(
            &mut host,
            &[S(b"string_find"), S(b"abc"), S(b"c"), W(0x8278)]
        )
        .ints(),
        [2]
    );
    assert_eq!(
        run(&mut host, &[S(b"no_such_fn"), I(1), W(0x8277)]).ints(),
        [0]
    );
    assert!(
        diagnostics(&mut host)
            .iter()
            .any(|line| line.contains("no_such_fn"))
    );
    assert_eq!(
        run(&mut host, &[S(b"set_flags"), I(0), I(1), W(0x8278)]).ints(),
        [0]
    );
    let done = run(&mut host, &[S(b"abc"), S(b"x"), I(0), W(0x824e)]);
    assert_eq!(done.text(), b"");
    assert_eq!(done.stack().len(), 1);
    // S9: a reserved word consumes nothing.
    let done = run(&mut host, &[I(5), W(0x8252)]);
    assert_eq!(done.ints(), [5]);
    assert_eq!(done.result, Err(VmError::Opcode(0x8252)));
    assert_eq!(run(&mut host, &[I(7), I(2), W(0x827f)]).ints(), [3]);
    assert_eq!(run(&mut host, &[I(-1), I(2), W(0x827f)]).ints(), [i32::MAX]);
}

#[test]
fn a_failed_operand_pops_everything_and_pushes_the_default() {
    let (mut host, _) = sfall_host();
    // Every class of return, with no VM error.
    assert_eq!(run(&mut host, &[S(b"x"), W(0x81ec)]).float(), 0.0);
    assert_eq!(run(&mut host, &[I(1), W(0x824f)]).ints(), [0]);
    assert_eq!(run(&mut host, &[I(1), I(2), I(3), W(0x824e)]).text(), b"");
    let done = run(&mut host, &[I(1), S(b"oops"), I(7), W(0x8160)]);
    assert_eq!(done.result, Ok(RunOutcome::FragmentEnd));
    assert_eq!(done.ints(), [1, 0], "`get_critter_base_stat` pops two");
    // Underflow stays a VM error.
    let done = run(&mut host, &[W(0x8160)]);
    assert_eq!(done.result, Err(VmError::Underflow));
    // A float for an int is truncated; an int for a float converts.
    let (mut host, me) = sfall_host();
    let done = run(&mut host, &[I(me as i32), F(4.9), I(77), W(0x815e)]);
    assert_eq!(done.result, Ok(RunOutcome::FragmentEnd));
    assert_eq!(
        host.world.get(me).unwrap().character.as_ref().unwrap().base[4],
        77
    );
    assert_eq!(run(&mut host, &[I(4), W(0x81ec)]).float(), 2.0);
}

#[test]
fn a_p2_word_diagnoses_once_per_program_and_x_words_never_touch_memory() {
    let (mut host, _) = sfall_host();
    run(
        &mut host,
        &[I(1), W(0x8162), I(1), W(0x8162), I(2), W(0x8171)],
    );
    assert_eq!(diagnostics(&mut host).len(), 2);
    let done = run(&mut host, &[I(0x7fff_0000), W(0x8158)]);
    assert_eq!(done.ints(), [0]);
    assert_eq!(run(&mut host, &[I(1), W(0x8159)]).text(), b"");
    let done = run(&mut host, &[I(1), I(2), W(0x81cf)]);
    assert_eq!(done.result, Ok(RunOutcome::FragmentEnd));
    assert!(done.ints().is_empty());
}

#[test]
fn the_environment_words_report_the_session() {
    let (mut host, _) = sfall_host();
    assert_eq!(
        run(&mut host, &[W(0x8210), W(0x8211), W(0x8212)]).ints(),
        [4, 5, 1]
    );
    assert_eq!(run(&mut host, &[W(0x81b3)]).ints(), [0]);
    host.sfall_end_of_step();
    host.sfall_end_of_step();
    assert_eq!(run(&mut host, &[W(0x81b3)]).ints(), [32]);
    assert_eq!(
        run(&mut host, &[W(0x8163)]).ints()[0],
        i32::from(host.world.game_date()[2])
    );
    // `game_loaded` is 1 once per script.
    assert_eq!(run(&mut host, &[W(0x8164), W(0x8164)]).ints(), [1, 0]);
    // The game mode: the world's bits and the screens'.
    use crate::host_sfall::game_mode;
    assert_eq!(
        run(&mut host, &[W(0x81af)]).ints(),
        [game_mode::LOCAL_MAP as i32]
    );
    host.set_sfall_screens(game_mode::WORLD_MAP | game_mode::BARTER);
    assert_eq!(
        run(&mut host, &[W(0x81af), W(0x8170)]).ints(),
        [(game_mode::WORLD_MAP | game_mode::BARTER) as i32, 1]
    );
    host.world.dialogue.open = true;
    host.set_sfall_screens(0);
    assert_eq!(
        run(&mut host, &[W(0x81af), W(0x8170)]).ints(),
        [(game_mode::LOCAL_MAP | game_mode::DIALOGUE) as i32, 0]
    );
}

#[test]
fn the_stat_words_store_without_perks_or_clamps() {
    let (mut host, me) = sfall_host();
    let player = host.world.player;
    let done = run(
        &mut host,
        &[
            I(me as i32),
            I(0),
            I(99),
            W(0x815e),
            I(me as i32),
            I(0),
            I(-5),
            W(0x815f),
            I(me as i32),
            I(0),
            W(0x8160),
            I(me as i32),
            I(0),
            W(0x8161),
            I(2),
            I(11),
            W(0x815a),
            I(2),
            I(12),
            W(0x815b),
            I(2),
            W(0x815c),
            I(2),
            W(0x815d),
            I(me as i32),
            I(35),
            W(0x8160),
        ],
    );
    assert_eq!(done.ints(), [99, -5, 11, 12, 0]);
    let sheet = host.world.get(player).unwrap().character.as_ref().unwrap();
    assert_eq!((sheet.base[2], sheet.bonus[2]), (11, 12));
    // A stale reference means null.
    assert_eq!(run(&mut host, &[I(0x7777), I(0), W(0x8160)]).ints(), [0]);
    assert!(
        diagnostics(&mut host)
            .iter()
            .any(|line| line.contains("no live object"))
    );
    let done = run(
        &mut host,
        &[
            I(me as i32),
            I(3),
            I(40),
            W(0x8242),
            I(me as i32),
            I(3),
            W(0x8243),
        ],
    );
    assert_eq!(done.ints(), [40]);
}

#[test]
fn the_math_and_string_words_run_from_scripts() {
    let (mut host, _) = sfall_host();
    let done = run(&mut host, &[I(2), I(3), W(0x8263)]);
    assert_eq!(done.ints(), [8]);
    assert_eq!(run(&mut host, &[F(-8.0), I(2), W(0x8263)]).float(), 64.0);
    assert!(
        run(&mut host, &[F(-8.0), F(0.5), W(0x8263)])
            .float()
            .is_nan()
    );
    assert_eq!(run(&mut host, &[I(i32::MIN), W(0x81ed)]).ints(), [i32::MIN]);
    assert_eq!(run(&mut host, &[F(-1.5), W(0x81ed)]).float(), 1.5);
    assert_eq!(run(&mut host, &[F(2.5), W(0x8267)]).ints(), [3]);
    assert_eq!(run(&mut host, &[F(1.5), W(0x8266)]).ints(), [2]);
    assert_eq!(
        run(&mut host, &[S(b"floor2"), F(-1.5), W(0x8277)]).ints(),
        [-2]
    );
    assert_eq!(
        run(&mut host, &[S(b"test"), I(-2), I(2), W(0x824e)]).text(),
        b"st"
    );
    assert_eq!(run(&mut host, &[S(b"test"), W(0x824f)]).ints(), [4]);
    assert_eq!(run(&mut host, &[S(b"A"), W(0x8251)]).ints(), [65]);
    assert_eq!(run(&mut host, &[S(b""), W(0x8251)]).ints(), [0]);
    assert_eq!(run(&mut host, &[I(1), W(0x8253)]).ints(), [1]);
    assert_eq!(run(&mut host, &[F(1.0), W(0x8253)]).ints(), [2]);
    assert_eq!(run(&mut host, &[S(b"x"), W(0x8253)]).ints(), [3]);
    assert_eq!(run(&mut host, &[S(b" 12x"), W(0x8237)]).ints(), [12]);
    assert_eq!(run(&mut host, &[S(b"1.5"), W(0x8238)]).float(), 1.5);
    assert_eq!(run(&mut host, &[S(b"%d!"), I(7), W(0x8250)]).text(), b"7!");
    assert_eq!(
        run(
            &mut host,
            &[S(b"string_format"), S(b"%s-%d"), S(b"a"), I(2), W(0x8279)]
        )
        .text(),
        b"a-2"
    );
    assert_eq!(
        run(
            &mut host,
            &[S(b"string_to_case"), S(b"aB"), I(1), W(0x8278)]
        )
        .text(),
        b"AB"
    );
    assert_eq!(
        run(
            &mut host,
            &[S(b"string_compare"), S(b"aB"), S(b"Ab"), W(0x8278)]
        )
        .ints(),
        [1]
    );
    assert_eq!(
        run(&mut host, &[S(b"metarule_exist"), S(b"floor2"), W(0x8277)]).ints(),
        [1]
    );
    assert_eq!(
        run(
            &mut host,
            &[S(b"metarule_exist"), S(b"message_box"), W(0x8277)]
        )
        .ints(),
        [0]
    );
    assert_eq!(
        run(&mut host, &[S(b"opcode_exists"), I(0x8210), W(0x8277)]).ints(),
        [1]
    );
    assert_eq!(
        run(&mut host, &[S(b"opcode_exists"), I(0x8200), W(0x8277)]).ints(),
        [0]
    );
    assert_eq!(
        run(&mut host, &[S(b"opcode_exists"), I(0x8252), W(0x8277)]).ints(),
        [0]
    );
}

#[test]
fn sfall_globals_keep_their_type_and_take_eight_byte_keys() {
    let (mut host, _) = sfall_host();
    let done = run(
        &mut host,
        &[
            S(b"a_myvar1"),
            I(1000),
            W(0x819d),
            I(7),
            F(0.5),
            W(0x819d),
            S(b"a_myvar1"),
            W(0x819e),
            I(7),
            W(0x819e),
            S(b"nope0000"),
            W(0x819e),
        ],
    );
    assert_eq!(done.ints(), [1000, 0, 0]);
    assert_eq!(run(&mut host, &[I(7), W(0x819f)]).float(), 0.5);
    assert_eq!(run(&mut host, &[S(b"a_myvar1"), W(0x819f)]).float(), 1000.0);
    // Another length makes the word a failed operand.
    run(&mut host, &[S(b"short"), I(1), W(0x819d)]);
    assert_eq!(run(&mut host, &[S(b"short"), W(0x819e)]).ints(), [0]);
    assert_eq!(host.world.sfall.as_ref().unwrap().globals.len(), 2);
}

#[test]
fn set_self_lasts_one_word_and_a_second_call_reaches_the_script() {
    let (mut host, me) = sfall_host();
    // A fragment's `self_obj`: the host has no script for it.
    let done = run(
        &mut host,
        &[W(0x80bc), I(me as i32), W(0x8206), W(0x80bc), W(0x80bc)],
    );
    assert_eq!(done.ints(), [0, me as i32, 0]);
    // `source_obj` and `target_obj` give nothing straight after it.
    let done = run(&mut host, &[I(me as i32), W(0x8206), W(0x80bd)]);
    assert_eq!(done.ints(), [0]);
    // Object 0 reverts at once.
    let done = run(
        &mut host,
        &[I(me as i32), W(0x8206), I(0), W(0x8206), W(0x80bc)],
    );
    assert_eq!(done.ints(), [0]);
    // A second consecutive call reaches the object's local variables.
    let script = host.world.add_script(me, 0, 4, 4).unwrap();
    host.world.get_mut(me).unwrap().raw.script_id = script as i32;
    let done = run(
        &mut host,
        &[
            I(me as i32),
            W(0x8206),
            I(me as i32),
            W(0x8206),
            I(1),
            I(55),
            W(0x80c2),
            I(me as i32),
            W(0x8206),
            I(me as i32),
            W(0x8206),
            I(1),
            W(0x80c1),
        ],
    );
    assert_eq!(done.ints(), [55]);
}

#[test]
fn prototype_data_addresses_fields_by_meaning() {
    let (mut host, me) = sfall_host();
    let pid = host.world.get(me).unwrap().raw.pid as i32;
    let prototype = Arc::new((*npc_prototype()).clone());
    host.assets.prototypes.insert(pid as u32, prototype.clone());
    let done = run(
        &mut host,
        &[
            I(pid),
            I(0),
            W(0x8204),
            I(pid),
            I(412),
            W(0x8204),
            I(pid),
            I(36),
            W(0x8204),
            I(pid),
            I(9999),
            W(0x8204),
            I(0x7700_0000),
            I(0),
            W(0x8204),
        ],
    );
    let PrototypeBase { team, strength } = PrototypeBase::of(&prototype);
    assert_eq!(done.ints(), [pid, team, strength, 0, -1]);
    run(&mut host, &[I(pid), I(412), I(9), W(0x8205)]);
    assert_eq!(run(&mut host, &[I(pid), I(412), W(0x8204)]).ints(), [9]);
    // The object in the world holds the edit too.
    let held = host.world.get(me).unwrap().prototype.clone().unwrap();
    assert_eq!(PrototypeBase::of(&held).team, 9);
}

struct PrototypeBase {
    team: i32,
    strength: i32,
}
impl PrototypeBase {
    fn of(prototype: &vault_formats::Prototype) -> Self {
        let vault_formats::pro::PrototypeData::Critter(critter) = &prototype.data else {
            panic!("a critter prototype");
        };
        Self {
            team: critter.team,
            strength: critter.base_stats[0],
        }
    }
}

#[test]
fn object_data_flags_and_the_inactive_hand() {
    let (mut host, me) = sfall_host();
    let hex = host.world.get(me).unwrap().raw.hex.unwrap();
    let done = run(
        &mut host,
        &[
            S(b"get_object_data"),
            I(me as i32),
            I(0x04),
            W(0x8278),
            S(b"get_object_data"),
            I(me as i32),
            I(0x999),
            W(0x8278),
            S(b"get_object_data"),
            I(0),
            I(0x04),
            W(0x8278),
        ],
    );
    assert_eq!(done.ints(), [hex, 0, 0]);
    run(
        &mut host,
        &[S(b"set_flags"), I(me as i32), I(0x11), W(0x8278)],
    );
    assert_eq!(
        run(&mut host, &[S(b"get_flags"), I(me as i32), W(0x8277)]).ints(),
        [0x11]
    );
    run(
        &mut host,
        &[
            S(b"set_object_data"),
            I(me as i32),
            I(0x40),
            I(6),
            W(0x8279),
        ],
    );
    assert_eq!(run(&mut host, &[I(me as i32), W(0x8191)]).ints(), [6]);
    // The ammunition words read the weapon's loaded state.
    let gun = host.world.spawn(npc_prototype(), 20001, 0).unwrap();
    host.world.get_mut(gun).unwrap().raw.state = ObjectState::Other {
        word: 0,
        detail: ObjectDetail::Weapon {
            rounds: 3,
            ammo_pid: 40,
        },
    };
    let done = run(
        &mut host,
        &[
            I(gun as i32),
            W(0x8217),
            I(gun as i32),
            W(0x8219),
            I(gun as i32),
            I(41),
            W(0x8218),
            I(gun as i32),
            I(9),
            W(0x821a),
            I(gun as i32),
            W(0x8217),
            I(gun as i32),
            W(0x8219),
        ],
    );
    assert_eq!(done.ints(), [40, 3, 41, 9]);
}

#[test]
fn lists_are_snapshots_in_entity_order() {
    let (mut host, me) = sfall_host();
    let other = host.world.spawn(npc_prototype(), 20002, 0).unwrap();
    let list = run(&mut host, &[I(0), W(0x820d)]).ints()[0];
    // A critter placed after the call is not in the snapshot.
    let late = host.world.spawn(npc_prototype(), 20003, 0).unwrap();
    let mut seen = Vec::new();
    loop {
        match run(&mut host, &[I(list), W(0x820e)]).ints()[0] {
            0 => break,
            object => seen.push(object as ObjectRef),
        }
    }
    assert!(seen.contains(&me) && seen.contains(&other));
    assert!(!seen.contains(&late));
    let mut ordered = seen.clone();
    ordered.sort_by_key(|object| host.world.state_handle(*object));
    assert_eq!(seen, ordered);
    // An open list is gone at the end of the Simulation phase.
    let list = run(&mut host, &[I(0), W(0x820d)]).ints()[0];
    host.sfall_end_of_step();
    assert_eq!(run(&mut host, &[I(list), W(0x820e)]).ints(), [0]);
    run(&mut host, &[I(list), W(0x820f)]);
}

#[derive(Clone, Default)]
struct MemoryFiles(Arc<Mutex<std::collections::BTreeMap<String, Vec<u8>>>>);
impl IniFiles for MemoryFiles {
    fn read(&self, file: &str) -> Option<Vec<u8>> {
        self.0.lock().unwrap().get(file).cloned()
    }
    fn write(&mut self, file: &str, text: &[u8]) -> Result<(), String> {
        self.0.lock().unwrap().insert(file.into(), text.to_vec());
        Ok(())
    }
}

#[test]
fn ini_settings_read_through_the_layers_and_write_to_the_folder() {
    let (mut host, _) = sfall_host();
    host.assets.resources.insert(
        "mod.ini".into(),
        b"; a comment\n[Main]\nCount= 12 ; note\nName=Vault\n[Other]\nx=1\n".to_vec(),
    );
    let files = MemoryFiles::default();
    files
        .0
        .lock()
        .unwrap()
        .insert("ddraw.ini".into(), b"[Misc]\nModifiedIni=3\n".to_vec());
    host.set_sfall_ini_files(Box::new(files.clone()));
    assert_eq!(
        run(&mut host, &[S(b"mod.ini|main|COUNT"), W(0x81ac)]).ints(),
        [12]
    );
    assert_eq!(
        run(&mut host, &[S(b"mod.ini|Main|Name"), W(0x81ac)]).ints(),
        [0]
    );
    assert_eq!(
        run(&mut host, &[S(b"mod.ini|Main|Name"), W(0x81eb)]).text(),
        b"Vault"
    );
    assert_eq!(run(&mut host, &[W(0x823b)]).ints(), [3]);
    // §7.2: malformed gives -1, and an absent key the empty string.
    assert_eq!(run(&mut host, &[S(b"a.ini"), W(0x81ac)]).ints(), [-1]);
    assert_eq!(run(&mut host, &[S(b"a.ini"), W(0x81eb)]).ints(), [-1]);
    assert_eq!(
        run(&mut host, &[S(b"absent.ini|s|k"), W(0x81eb)]).text(),
        b""
    );
    assert_eq!(
        run(&mut host, &[S(b"absent.ini|s|k"), W(0x81ac)]).ints(),
        [-1]
    );
    assert_eq!(
        run(&mut host, &[S(b"../up.ini|s|k"), W(0x81eb)]).ints(),
        [-1]
    );
    assert_eq!(
        run(&mut host, &[S(b"C:/up.ini|s|k"), W(0x81eb)]).ints(),
        [-1]
    );
    // A write reaches the cache and the folder, and creates the file.
    let done = run(
        &mut host,
        &[
            S(b"set_ini_setting"),
            S(b"new.ini|Sec|Key"),
            I(5),
            W(0x8278),
        ],
    );
    assert_eq!(done.ints(), [0]);
    assert_eq!(
        run(&mut host, &[S(b"new.ini|sec|key"), W(0x81ac)]).ints(),
        [5]
    );
    assert_eq!(
        files.0.lock().unwrap().get("new.ini").unwrap(),
        b"[Sec]\nKey=5\n"
    );
    // Without a `ModifiedIni`, `modified_ini` gives 0.
    let (mut other, _) = sfall_host();
    assert_eq!(run(&mut other, &[W(0x823b)]).ints(), [0]);
}

#[test]
fn metarule3_adds_sfall_sub_functions_only_with_the_mode_on() {
    let (mut host, _) = sfall_host();
    host.sfall.as_mut().unwrap().key_queue.push_back(30);
    assert_eq!(
        run(&mut host, &[I(201), I(0), I(0), I(0), W(0x80e1)]).ints(),
        [0]
    );
    assert!(host.sfall.as_ref().unwrap().key_queue.is_empty());
    assert_eq!(
        run(&mut host, &[I(210), I(0), I(0), I(0), W(0x80e1)]).ints(),
        [0]
    );
    assert!(
        diagnostics(&mut host)
            .iter()
            .any(|line| line.contains("get_current_save_slot"))
    );
    assert_eq!(
        run(&mut host, &[I(205), I(0), I(0), I(0), W(0x80e1)]).ints(),
        [0]
    );
    assert!(
        diagnostics(&mut host)
            .iter()
            .any(|line| line.contains("205 is not defined"))
    );
    // `metarule2_explosions` pops three and pushes one.
    assert_eq!(run(&mut host, &[I(1), I(2), I(3), W(0x8261)]).ints(), [0]);
    assert_eq!(sfall::SFALL_EXPLOSIONS.len(), 9);
}

#[test]
fn the_reg_anim_words_keep_their_effects_at_sfall_numbers() {
    let (mut host, me) = sfall_host();
    host.building_animation = Some(Default::default());
    run(&mut host, &[I(me as i32), I(0x1000), I(5), W(0x825e)]);
    run(&mut host, &[I(me as i32), I(20), I(5), W(0x8260)]);
    let sequence = host.building_animation.as_ref().unwrap();
    assert_eq!(sequence.commands.len(), 2);
    assert_eq!(sequence.commands[0].1, [5, 0x1000, me as i32]);
    assert_eq!(sequence.commands[0].0, 0x8157);
    assert_eq!(sequence.commands[1].0, 0x8159);
}
