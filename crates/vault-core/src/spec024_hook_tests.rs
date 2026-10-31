//! SPEC-024b part 3: registration, chains and composition on the H vectors
//! of `sfall-array-vectors.tsv`, the eight hook boundaries, and §7.3 tests 7
//! and 8.
use crate::{
    ScriptEvent, WorldEffect, WorldHost, fixture_state as fixture,
    host_ai_tests::fixture_e,
    host_sfall::game_mode,
    host_sfall_hooks::{
        HOOK_AFTERHITROLL, HOOK_BARTERPRICE, HOOK_CALCAPCOST, HOOK_COMBATDAMAGE, HOOK_ENCOUNTER,
        HOOK_GAMEMODECHANGE, HOOK_INVENTORYMOVE, HOOK_KEYPRESS, HOOK_MOVECOST, HOOK_SETGLOBALVAR,
        HOOK_STDPROCEDURE, HOOK_TOHIT, HOOK_USEOBJ, HOOK_USEOBJON, Preview,
    },
    spec024_arrays_tests::VECTORS,
    spec024_tests::{Op, base, diagnostics, literal_offset, run, sfall_host},
    test_support::{TestAssets, request},
    world::ObjectRef,
};
use Op::{F, I, S, W};
use vault_vm::{
    IntProgram, Vm, WordSet,
    sfall::{self, SfallValue},
    sfall_arrays::Storage,
};

const GET_ARG: Op = W(0x81e4);
const SET_RETURN: Op = W(0x81e5);
const SET_ARG: Op = W(0x823d);
const INIT_HOOK: Op = W(0x81ea);
const REGISTER: Op = W(0x8207);
const REGISTER_PROC: Op = W(0x8262);
const REGISTER_SPEC: Op = W(0x827d);
const DUPLICATE: Op = W(0x801b);
const SWAP: Op = W(0x8018);
const DISCARD: Op = W(0x801a);
const ADD: Op = W(0x8039);
const MULTIPLY: Op = W(0x803b);
const DIVIDE: Op = W(0x803c);
const GLOBAL_INT: Op = W(0x819e);
const SET_GLOBAL: Op = W(0x819d);
const RANDOM: Op = W(0x80b4);
/// The right hand's primary attack: `fixture_e`'s attacker holds its gun
/// there.
const RIGHT: u8 = crate::critical::hit_mode::RIGHT_PRIMARY;

/// `ops`' bytes; a string op is a literal of `strings`.
fn encode(ops: &[Op], strings: &IntProgram) -> Vec<u8> {
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
            Op::W(word) => bytes.extend(word.to_be_bytes()),
            Op::S(text) => {
                bytes.extend([0x90, 0x01]);
                bytes.extend(literal_offset(strings, text).to_be_bytes());
            }
        }
    }
    bytes
}
/// A compiled script's shape: the initial stream runs `start`, procedure 1,
/// and `procedures` follow it at indices 2 up.
fn script(start: &[Op], procedures: &[(&'static str, &[Op])]) -> IntProgram {
    script_with(&[], start, procedures)
}
/// [`script`], with `strings` for its literals.
fn script_with(
    strings: &[&[u8]],
    start: &[Op],
    procedures: &[(&'static str, &[Op])],
) -> IntProgram {
    let table = base(strings);
    let body = |ops: &[Op]| fixture::returning(encode(ops, &table));
    let mut listed: Vec<(&[u8], Vec<u8>)> = vec![(b"start", body(start))];
    for (name, ops) in procedures {
        listed.push((name.as_bytes(), body(ops)));
    }
    let mut bytes = fixture::program(strings, &listed);
    let words = WordSet::FALLOUT2_SFALL;
    let body = IntProgram::parse_in(&bytes, &words, false)
        .unwrap()
        .procedures()[1]
        .body;
    // The bootstrap's initial stream holds the root push at +12.
    let init = u32::from_be_bytes(bytes[12..16].try_into().unwrap()) as usize;
    bytes[init + 12..init + 16].copy_from_slice(&body.to_be_bytes());
    IntProgram::parse_in(&bytes, &words, false).unwrap()
}
fn install(host: &mut WorldHost<TestAssets>, name: &str, program: IntProgram) {
    host.assets.programs.push((name.to_owned(), program));
}
fn set_up(host: &mut WorldHost<TestAssets>) {
    host.sfall_setup_owed();
    host.sfall_setup_if_due();
}
fn ints(values: &[i32]) -> Vec<SfallValue> {
    values.iter().copied().map(SfallValue::Int).collect()
}
fn global(host: &WorldHost<TestAssets>, key: i32) -> i32 {
    host.world
        .sfall
        .as_ref()
        .unwrap()
        .globals
        .get(&crate::sfall::GlobalKey::Int(key))
        .map_or(0, SfallValue::int)
}
/// Append `digit` to the decimal trace in global 1.
fn trace(digit: i32) -> [Op; 8] {
    [I(1), GLOBAL_INT, I(10), MULTIPLY, I(digit), ADD, I(1), SWAP]
}
fn traced(digit: i32) -> Vec<Op> {
    let mut ops = trace(digit).to_vec();
    ops.push(SET_GLOBAL);
    ops
}
fn hook_args() -> Vec<SfallValue> {
    ints(&[100, 0, 0, 3, 0, 0, 1, 100])
}

#[test]
fn h1_h2_the_published_example_gives_75_in_either_order() {
    // "Divide by 2" and "multiply by 1.5", each also writing the argument.
    let halve: &[Op] = &[
        GET_ARG,
        I(2),
        DIVIDE,
        DUPLICATE,
        SET_RETURN,
        I(0),
        SWAP,
        SET_ARG,
    ];
    let raise: &[Op] = &[
        GET_ARG,
        F(1.5),
        MULTIPLY,
        DUPLICATE,
        SET_RETURN,
        I(0),
        SWAP,
        SET_ARG,
    ];
    for (first, second) in [(halve, raise), (raise, halve)] {
        let (mut host, _) = sfall_host();
        let register = [I(HOOK_TOHIT), I(2), REGISTER_PROC];
        install(&mut host, "gl_a.int", script(&register, &[("mod", first)]));
        install(&mut host, "gl_b.int", script(&register, &[("mod", second)]));
        set_up(&mut host);
        let rets = host.run_hook(HOOK_TOHIT, hook_args()).unwrap();
        assert_eq!(rets, [Some(SfallValue::Int(75))]);
    }
}

/// `I:7` or `S:"text"`.
fn shown(value: &SfallValue) -> String {
    match value {
        SfallValue::Int(value) => format!("I:{value}"),
        SfallValue::Text(text) => format!("S:\"{}\"", String::from_utf8_lossy(text)),
        other => format!("{other:?}"),
    }
}
/// `HOOK_COMBATDAMAGE`'s thirteen arguments, with `set` run on them first,
/// and each then read into element `index` of the list it returns, which
/// keeps a value's type. Argument 2 is 10.
fn damage_chain(host: &mut WorldHost<TestAssets>, strings: &[&[u8]], set: &[Op]) -> i32 {
    let state = host.world.sfall.as_mut().unwrap();
    let list = state.arrays.create(13, 0, Storage::Permanent);
    let mut body = set.to_vec();
    for index in 0..13 {
        body.extend([I(list), I(index), GET_ARG, W(0x822e)]);
    }
    let register = [I(HOOK_COMBATDAMAGE), I(2), REGISTER_PROC];
    install(
        host,
        "gl_0.int",
        script_with(strings, &register, &[("m", &body)]),
    );
    set_up(host);
    let mut args = ints(&[0; 13]);
    args[2] = SfallValue::Int(10);
    diagnostics(host);
    let rets = host.run_hook(HOOK_COMBATDAMAGE, args).unwrap();
    assert_eq!(rets, [None, None, None, None, None], "rets=[None]");
    list
}
fn argument(host: &WorldHost<TestAssets>, list: i32, index: i32) -> SfallValue {
    let state = host.world.sfall.as_ref().unwrap();
    state.arrays.get(list, &SfallValue::Int(index))
}

/// One row of SS2.
fn ss2_row(cells: &[&str]) {
    let call = cells[2]
        .split_once("set_sfall_arg(")
        .and_then(|(_, rest)| rest.split_once(')'))
        .unwrap()
        .0;
    let (index, value) = call.split_once(',').unwrap();
    let text: &'static [u8] = Box::leak(
        value
            .trim_matches('"')
            .as_bytes()
            .to_vec()
            .into_boxed_slice(),
    );
    let value = match value.strip_prefix("F:") {
        Some(number) => F(number.parse().unwrap()),
        None => S(text),
    };
    let (mut host, _) = sfall_host();
    let index: i32 = index.parse().unwrap();
    let set = [I(index), value, SET_ARG];
    let list = damage_chain(&mut host, &[text], &set);
    let lines = diagnostics(&mut host);
    let result = format!(
        "final=arg2={},arg12={},diagnostics={}; order=m",
        shown(&argument(&host, list, 2)),
        shown(&argument(&host, list, 12)),
        lines.len()
    );
    assert_eq!(result, cells[4], "{cells:?}");
}

#[test]
fn d_46_set_sfall_arg_converts_by_the_arguments_type_by_vector_ss2() {
    let rows = rows("SS2");
    assert_eq!(rows.len(), 4);
    for cells in &rows {
        ss2_row(cells);
    }
}

#[test]
fn d_46_a_mixed_argument_keeps_a_float_and_an_int_argument_truncates_it() {
    let (mut host, _) = sfall_host();
    let list = damage_chain(
        &mut host,
        &[],
        &[I(12), F(7.9), SET_ARG, I(2), F(-3.9), SET_ARG],
    );
    assert_eq!(argument(&host, list, 2), SfallValue::Int(-3));
    assert_eq!(argument(&host, list, 12), SfallValue::float(7.9));
    // An object type converts by class `I`, and an index out of range is
    // ignored, with no diagnostic.
    let (mut host, _) = sfall_host();
    let list = damage_chain(
        &mut host,
        &[],
        &[
            I(0),
            F(5.9),
            SET_ARG,
            I(13),
            I(1),
            SET_ARG,
            I(-1),
            I(1),
            SET_ARG,
        ],
    );
    assert_eq!(argument(&host, list, 0), SfallValue::Int(5));
    assert!(diagnostics(&mut host).is_empty());
}

#[test]
fn h3_the_chain_runs_newest_first_then_the_script_then_spec_oldest_first() {
    let (mut host, _) = sfall_host();
    let hook = HOOK_TOHIT;
    // Scripts load in name order, so r1 registers before r2.
    let proc = |digit| traced(digit);
    let (r1, r2, s1, s2) = (proc(1), proc(2), proc(4), proc(5));
    install(
        &mut host,
        "gl_0r1.int",
        script(&[I(hook), I(2), REGISTER_PROC], &[("m", &r1)]),
    );
    install(
        &mut host,
        "gl_1r2.int",
        script(&[I(hook), I(2), REGISTER_PROC], &[("m", &r2)]),
    );
    install(
        &mut host,
        "gl_2s1.int",
        script(&[I(hook), I(2), REGISTER_SPEC], &[("m", &s1)]),
    );
    install(
        &mut host,
        "gl_3s2.int",
        script(&[I(hook), I(2), REGISTER_SPEC], &[("m", &s2)]),
    );
    // `start` is the hs script's member; setup runs it once too.
    let body = traced(3);
    install(&mut host, "hs_tohit.int", script(&body, &[]));
    set_up(&mut host);
    host.world.sfall.as_mut().unwrap().globals.clear();
    host.run_hook(hook, hook_args());
    assert_eq!(global(&host, 1), 21_345);
    // Procedure 0 unregisters the calling script: r2.
    let r2 = host.global_for_test(1);
    host.unregister_for_test(hook, r2);
    host.world.sfall.as_mut().unwrap().globals.clear();
    host.run_hook(hook, hook_args());
    assert_eq!(global(&host, 1), 1_345);
}

#[test]
fn h4_each_member_restarts_at_ret0_and_the_last_writer_wins_per_index() {
    let (mut host, _) = sfall_host();
    let early: &[Op] = &[I(10), SET_RETURN, I(20), SET_RETURN];
    let late: &[Op] = &[I(30), SET_RETURN];
    // `early` registers first, and newest runs first, so it goes second.
    let hook = HOOK_COMBATDAMAGE;
    install(
        &mut host,
        "gl_0.int",
        script(&[I(hook), I(2), REGISTER_SPEC], &[("m", early)]),
    );
    install(
        &mut host,
        "gl_1.int",
        script(&[I(hook), I(2), REGISTER_SPEC], &[("m", late)]),
    );
    set_up(&mut host);
    let rets = host.run_hook(hook, ints(&[0; 13])).unwrap();
    assert_eq!(rets[0], Some(SfallValue::Int(30)));
    assert_eq!(rets[1], Some(SfallValue::Int(20)));
    assert_eq!(rets[2], None);
}

#[test]
fn h5_h6_a_write_past_the_end_does_nothing_and_a_read_past_it_gives_0() {
    let (mut host, _) = sfall_host();
    let hook = HOOK_MOVECOST;
    let body: &[Op] = &[I(1), SET_RETURN, I(2), SET_RETURN];
    install(
        &mut host,
        "gl_0.int",
        script(&[I(hook), I(2), REGISTER_PROC], &[("m", body)]),
    );
    set_up(&mut host);
    let rets = host.run_hook(hook, ints(&[0, 1, 1])).unwrap();
    assert_eq!(rets, [Some(SfallValue::Int(1))]);

    let (mut host, _) = sfall_host();
    // Three arguments are read, then a fourth, which gives 0.
    let body: &[Op] = &[
        GET_ARG, DISCARD, GET_ARG, DISCARD, GET_ARG, DISCARD, GET_ARG, SET_RETURN,
    ];
    install(
        &mut host,
        "gl_0.int",
        script(&[I(hook), I(2), REGISTER_PROC], &[("m", body)]),
    );
    set_up(&mut host);
    let rets = host.run_hook(hook, ints(&[11, 11, 11])).unwrap();
    assert_eq!(rets, [Some(SfallValue::Int(0))]);
}

/// A host whose hook returns what `body` writes.
fn hooked(hook: i32, body: &[Op]) -> WorldHost<TestAssets> {
    let (mut host, _) = sfall_host();
    let register = [I(hook), I(2), REGISTER_PROC];
    install(&mut host, "gl_0.int", script(&register, &[("m", body)]));
    set_up(&mut host);
    host
}
fn attack_host(hook: i32, body: &[Op]) -> (WorldHost<TestAssets>, [ObjectRef; 4]) {
    let (mut host, critters) = fixture_e("critter", 2);
    host.sfall = Some(Default::default());
    host.world.sfall = Some(Default::default());
    host.assets.programs.clear();
    let register = [I(hook), I(2), REGISTER_PROC];
    install(&mut host, "gl_0.int", script(&register, &[("m", body)]));
    set_up(&mut host);
    host.world.effects.clear();
    (host, critters)
}

#[test]
fn h7_to_hit_is_limited_and_an_unset_return_leaves_the_engine_value() {
    use crate::critical::location;
    let cases: [(&[Op], i32); 3] = [
        (&[I(1500), SET_RETURN], 999),
        (&[I(-500), SET_RETURN], -99),
        (&[I(40), SET_RETURN], 40),
    ];
    for (body, expected) in cases {
        let (mut host, [attacker, _, defender, _]) = attack_host(HOOK_TOHIT, body);
        let attack = host
            .world
            .attack_profile(attacker, defender, 0, location::TORSO, false)
            .unwrap();
        let chance = host
            .hook_hit_chance(attacker, defender, &attack, true, None)
            .unwrap();
        assert_eq!(chance, expected);
    }
    let (mut host, [attacker, _, defender, _]) = attack_host(HOOK_TOHIT, &[]);
    let attack = host
        .world
        .attack_profile(attacker, defender, 0, location::TORSO, false)
        .unwrap();
    assert_eq!(
        host.hook_hit_chance(attacker, defender, &attack, true, None)
            .unwrap(),
        attack.hit_chance(true).unwrap()
    );
}

#[test]
fn the_to_hit_arguments_are_the_chance_the_critters_and_the_query() {
    use crate::critical::location;
    // Writes each argument to a global, keyed by its position.
    let mut body = Vec::new();
    for index in 0..8 {
        body.extend([I(10 + index), GET_ARG, SET_GLOBAL]);
    }
    let (mut host, [attacker, _, defender, _]) = attack_host(HOOK_TOHIT, &body);
    let attack = host
        .world
        .attack_profile(attacker, defender, 0, location::HEAD, false)
        .unwrap();
    host.hook_hit_chance(attacker, defender, &attack, false, None)
        .unwrap();
    let (capped, raw) = attack.chance_parts(false).unwrap();
    let hex = host.world.get(attacker).unwrap().raw.hex.unwrap();
    let found: Vec<i32> = (10..18).map(|key| global(&host, key)).collect();
    assert_eq!(
        found,
        [
            capped,
            attacker as i32,
            defender as i32,
            i32::from(location::HEAD),
            hex,
            0,
            0,
            raw
        ]
    );
}

#[test]
fn h8_after_hit_roll_rejects_bad_returns_and_applies_the_roll() {
    let body: &[Op] = &[I(3), SET_RETURN, I(9), SET_RETURN, I(0), SET_RETURN];
    let (mut host, [attacker, _, defender, _]) = attack_host(HOOK_AFTERHITROLL, body);
    let mut attack = host
        .world
        .attack_profile(attacker, defender, 0, 3, false)
        .unwrap();
    let mut target = defender;
    let mut rng = host.world.rng.clone();
    let code = host
        .hook_after_roll(attacker, &mut target, &mut attack, 2, &mut rng)
        .unwrap();
    assert_eq!((code, attack.location, target), (3, 3, defender));
}

#[test]
fn after_hit_roll_changes_the_target_and_the_body_part_for_the_rest_of_the_attack() {
    let (host, [attacker, target, other, _]) = attack_host(HOOK_AFTERHITROLL, &[]);
    drop(host);
    let body = [
        I(2),
        SET_RETURN,
        I(1),
        SET_RETURN,
        I(other as i32),
        SET_RETURN,
    ];
    let (mut host, _) = attack_host(HOOK_AFTERHITROLL, &body);
    let hp = host.world.hit_points(other).unwrap();
    let kept = host.world.hit_points(target).unwrap();
    let result = host
        .perform_attack(attacker, target, RIGHT, 3, false)
        .unwrap();
    let resolution = result.unwrap();
    assert_eq!(resolution.code, 2);
    assert!(host.world.hit_points(other).unwrap() < hp, "the new target");
    assert_eq!(host.world.hit_points(target), Some(kept));
}

#[test]
fn combat_damage_overrides_the_amount_before_it_applies() {
    let body: &[Op] = &[I(7), SET_RETURN, I(0), SET_RETURN];
    // Any hit: force it with `HOOK_AFTERHITROLL`'s neighbor, a sure chance.
    let (mut host, [attacker, defender, ..]) = attack_host(HOOK_COMBATDAMAGE, body);
    let tohit = script(
        &[I(HOOK_TOHIT), I(2), REGISTER_PROC],
        &[("m", &[I(999), SET_RETURN])],
    );
    install(&mut host, "gl_1.int", tohit);
    set_up(&mut host);
    let hp = host.world.hit_points(defender).unwrap();
    let result = host
        .perform_attack(attacker, defender, RIGHT, 3, false)
        .unwrap()
        .unwrap();
    assert_eq!(result.damage, 7);
    assert_eq!(host.world.hit_points(defender), Some(hp - 7));
}

#[test]
fn calc_ap_cost_is_free_at_0_and_a_negative_return_becomes_0() {
    for body in [&[I(0), SET_RETURN][..], &[I(-5), SET_RETURN]] {
        let (mut host, [attacker, defender, ..]) = attack_host(HOOK_CALCAPCOST, body);
        let ap = host.world.action_points(attacker);
        host.perform_attack(attacker, defender, RIGHT, 3, false)
            .unwrap()
            .unwrap();
        assert_eq!(host.world.action_points(attacker), ap, "a free attack");
    }
    // The fixed costs reach the hook too.
    let mut host = hooked(HOOK_CALCAPCOST, &[I(5), SET_RETURN]);
    assert_eq!(host.hook_fixed_cost(0, 2, 0), 5);
    // A return of more than the critter has is refused by the preflight.
    let (mut host, [attacker, defender, ..]) = attack_host(HOOK_CALCAPCOST, &[I(99), SET_RETURN]);
    let result = host
        .perform_attack(attacker, defender, RIGHT, 3, false)
        .unwrap();
    assert_eq!(result.unwrap_err(), crate::Refusal::ActionPoints);
}

#[test]
fn move_cost_replaces_the_step_cost_before_the_pool_or_ap_are_spent() {
    let (mut host, [attacker, ..]) = attack_host(HOOK_MOVECOST, &[I(3), SET_RETURN]);
    host.world.combat = true;
    let ap = host.world.action_points(attacker);
    host.charge_movement(attacker);
    assert_eq!(host.world.action_points(attacker), ap - 3);
    assert_eq!(host.hook_move_cost(attacker, 5, 5), 3, "a planning query");
    let (mut host, [attacker, ..]) = attack_host(HOOK_MOVECOST, &[I(-3), SET_RETURN]);
    host.world.combat = true;
    let ap = host.world.action_points(attacker);
    host.charge_movement(attacker);
    assert_eq!(host.world.action_points(attacker), ap);
    // Out of combat movement costs no AP, and no chain runs.
    let (mut host, [attacker, ..]) = attack_host(HOOK_MOVECOST, &traced(1));
    host.charge_movement(attacker);
    assert_eq!(global(&host, 1), 0);
}

#[test]
fn h10_only_a_set_return_other_than_minus_1_prevents_a_move() {
    let hook = HOOK_INVENTORYMOVE;
    assert!(hooked(hook, &[]).sfall_inventory_move(1, 7, 0));
    assert!(hooked(hook, &[I(-1), SET_RETURN]).sfall_inventory_move(1, 7, 0));
    assert!(!hooked(hook, &[I(0), SET_RETURN]).sfall_inventory_move(1, 7, 0));
}

#[test]
fn h11_a_key_maps_and_its_release_ends_what_the_press_produced() {
    let hook = HOOK_KEYPRESS;
    assert_eq!(
        hooked(hook, &[I(0), SET_RETURN]).sfall_key(true, 30, 65),
        30
    );
    let mut host = hooked(hook, &[I(48), SET_RETURN]);
    assert_eq!(host.sfall_key(true, 30, 65), 48);
    let mut host_after = hooked(hook, &[I(0), SET_RETURN]);
    assert_eq!(host_after.sfall_key(false, 30, 65), 30, "no press was held");
    // The press's mapping stands, whatever the release's chain returns.
    assert_eq!(host.sfall_key(false, 30, 65), 48);
    assert_eq!(
        host.sfall_key(false, 30, 65),
        48,
        "the override still holds"
    );
    // With no hook the key passes through.
    let (mut plain, _) = sfall_host();
    assert_eq!(plain.sfall_key(true, 30, 65), 30);
}

#[test]
fn h9_the_barter_returns_apply_as_the_vector_says() {
    let body: &[Op] = &[I(-1), SET_RETURN, I(200), SET_RETURN];
    let (mut host, [_, merchant, ..]) = attack_host(HOOK_BARTERPRICE, body);
    let session = host.begin_barter(merchant).unwrap();
    let plain = host.barter_price(&session);
    assert_eq!(host.hooked_barter(&session, true), (plain, 200));
    host.end_barter(session).unwrap();

    let body: &[Op] = &[I(90), SET_RETURN];
    let (mut host, [_, merchant, ..]) = attack_host(HOOK_BARTERPRICE, body);
    let session = host.begin_barter(merchant).unwrap();
    let offer = host.world.inventory_value(session.offer);
    assert_eq!(host.hooked_barter(&session, true), (90, offer));
    // Any other negative price becomes 0, and a negative offer too.
    let body: &[Op] = &[I(-7), SET_RETURN, I(-3), SET_RETURN];
    let (mut host, [_, merchant, ..]) = attack_host(HOOK_BARTERPRICE, body);
    let session = host.begin_barter(merchant).unwrap();
    assert_eq!(host.hooked_barter(&session, true), (0, 0));
}

#[test]
fn the_barter_preview_runs_twice_on_entry_and_then_only_on_a_change() {
    let mut body = traced(1);
    body.extend([I(90), SET_RETURN]);
    let (mut host, [_, merchant, ..]) = attack_host(HOOK_BARTERPRICE, &body);
    let session = host.begin_barter(merchant).unwrap();
    assert_eq!(host.sfall_barter_shown(), None);
    host.sfall_barter_preview(&session);
    assert_eq!(global(&host, 1), 11, "the player, then the NPC");
    assert_eq!(host.sfall_barter_shown().unwrap().0, 90);
    host.sfall_barter_preview(&session);
    assert_eq!(global(&host, 1), 11, "the same inputs run nothing");
    host.end_barter(session).unwrap();
    assert_eq!(host.sfall_barter_shown(), None);
}

#[test]
fn a_member_that_errors_is_unloaded_and_the_chain_goes_on() {
    let (mut host, _) = sfall_host();
    let hook = HOOK_TOHIT;
    let bad: &[Op] = &[I(5), SET_RETURN, W(0x8252)];
    let good: &[Op] = &[I(6), SET_RETURN];
    install(
        &mut host,
        "gl_0.int",
        script(&[I(hook), I(2), REGISTER_SPEC], &[("m", bad)]),
    );
    install(
        &mut host,
        "gl_1.int",
        script(&[I(hook), I(2), REGISTER_SPEC], &[("m", good)]),
    );
    set_up(&mut host);
    let rets = host.run_hook(hook, hook_args()).unwrap();
    assert_eq!(rets, [Some(SfallValue::Int(6))], "the later member runs");
    host.world.effects.clear();
    // Only the good member is left, and the bad one's write is gone with it.
    let body_gone = host.run_hook(hook, hook_args()).unwrap();
    assert_eq!(body_gone, [Some(SfallValue::Int(6))]);
    assert!(!host.world.effects.iter().any(|effect| matches!(
        effect,
        crate::WorldEffect::Diagnostic(text) if text.contains("unloaded")
    )));
}

#[test]
fn a_member_stops_at_a_word_that_would_suspend_it_with_its_writes_kept() {
    let (mut host, _) = sfall_host();
    let hook = HOOK_TOHIT;
    let body: &[Op] = &[I(7), SET_RETURN, I(1), W(0x8047), I(8), SET_RETURN];
    install(
        &mut host,
        "gl_0.int",
        script(&[I(hook), I(2), REGISTER_PROC], &[("m", body)]),
    );
    set_up(&mut host);
    let rets = host.run_hook(hook, hook_args()).unwrap();
    assert_eq!(rets, [Some(SfallValue::Int(7))]);
    // It runs again next time: nothing was left suspended.
    let rets = host.run_hook(hook, hook_args()).unwrap();
    assert_eq!(rets, [Some(SfallValue::Int(7))]);
}

#[test]
fn only_global_and_hook_scripts_register_and_bad_ids_are_ignored() {
    use crate::spec024_tests::run;
    let (mut host, _) = sfall_host();
    let hook = HOOK_TOHIT;
    let body: &[Op] = &[I(6), SET_RETURN];
    install(
        &mut host,
        "gl_0.int",
        script(
            &[I(49), I(2), REGISTER_PROC, I(99), I(2), REGISTER_PROC],
            &[("m", body)],
        ),
    );
    set_up(&mut host);
    assert_eq!(host.run_hook(hook, hook_args()), None);
    host.world.effects.clear();
    let done = run(&mut host, &[I(hook), REGISTER]);
    assert!(done.ints().is_empty());
    assert!(host.world.effects.iter().any(|effect| matches!(
        effect,
        crate::WorldEffect::Diagnostic(text) if text.contains("neither")
    )));
    assert_eq!(host.run_hook(hook, hook_args()), None, "an empty chain");
}

#[test]
fn a_p2_hook_loads_and_runs_its_start_and_fires_no_chain() {
    let (mut host, _) = sfall_host();
    install(&mut host, "hs_ondeath.int", script(&traced(1), &[]));
    set_up(&mut host);
    assert_eq!(global(&host, 1), 1, "setup ran its start");
    assert_eq!(host.run_hook(6, ints(&[0])), None);
    assert_eq!(global(&host, 1), 1);
}

#[test]
fn a_shared_hook_file_loads_once() {
    let (mut host, _) = sfall_host();
    install(&mut host, "hs_stdprocedure.int", script(&traced(1), &[]));
    set_up(&mut host);
    assert_eq!(global(&host, 1), 1);
}

#[test]
fn init_hook_is_1_in_setup_and_setup_runs_again_after_a_load() {
    let (mut host, _) = sfall_host();
    let start = [I(2), INIT_HOOK, SET_GLOBAL];
    install(&mut host, "gl_0.int", script(&start, &[]));
    set_up(&mut host);
    assert_eq!(global(&host, 2), 1);
    // Outside setup and outside a chain it is 0.
    use crate::spec024_tests::run;
    assert_eq!(run(&mut host, &[INIT_HOOK]).ints(), [0]);
    // Outside a chain the hook words give their fixed results.
    assert_eq!(run(&mut host, &[GET_ARG]).ints(), [0]);
    let list = run(&mut host, &[W(0x823c)]).ints()[0];
    assert_eq!(run(&mut host, &[I(list), W(0x8231)]).ints(), [0]);
    run(&mut host, &[I(1), SET_RETURN, I(0), I(1), SET_ARG]);
    host.world.sfall.as_mut().unwrap().globals.clear();
    host.sfall_restored();
    // The fixture's critter would take file 0 as its own script.
    host.world.scripts_enabled = false;
    host.tick(16).unwrap();
    assert_eq!(global(&host, 2), 1, "the first Simulation phase set it up");
}

#[test]
fn a_global_script_repeats_by_steps_in_its_kind_of_screen() {
    let (mut host, _) = sfall_host();
    // `start` traces; in setup it also asks to repeat every 2 steps.
    let mut start = traced(1);
    start.extend([I(2), W(0x816a)]);
    install(&mut host, "gl_0.int", script(&start, &[]));
    set_up(&mut host);
    assert_eq!(global(&host, 1), 1);
    host.sfall_end_of_step();
    assert_eq!(global(&host, 1), 1);
    host.sfall_end_of_step();
    assert_eq!(global(&host, 1), 11);
    // Kind 0 is the local map: dialogue holds it.
    host.world.dialogue.open = true;
    host.sfall_end_of_step();
    host.sfall_end_of_step();
    assert_eq!(global(&host, 1), 11);
}

#[test]
fn self_obj_in_a_global_script_is_null_with_a_diagnostic() {
    let (mut host, _) = sfall_host();
    let start = [I(3), W(0x80bc), SET_GLOBAL];
    install(&mut host, "gl_0.int", script(&start, &[]));
    set_up(&mut host);
    assert_eq!(global(&host, 3), 0);
    assert!(host.world.effects.iter().any(|effect| matches!(
        effect,
        crate::WorldEffect::Diagnostic(text) if text.contains("self_obj")
    )));
}

#[test]
fn chains_nest_to_8_and_a_ninth_is_skipped() {
    let (mut host, _) = sfall_host();
    // Nothing in the words nests a chain, so the context stack is filled.
    let hook = HOOK_TOHIT;
    let body: &[Op] = &[I(6), SET_RETURN];
    install(
        &mut host,
        "gl_0.int",
        script(&[I(hook), I(2), REGISTER_PROC], &[("m", body)]),
    );
    set_up(&mut host);
    host.fill_contexts_for_test(8);
    assert_eq!(host.run_hook(hook, hook_args()), None);
    host.fill_contexts_for_test(0);
    assert!(host.run_hook(hook, hook_args()).is_some());
}

#[test]
fn a_preview_runs_only_when_its_inputs_change() {
    let (mut host, _) = sfall_host();
    let mut runs = 0;
    for inputs in [vec![1], vec![1], vec![2], vec![2]] {
        host.sfall_preview(Preview::SlotCost, inputs, |_| {
            runs += 1;
            runs
        });
    }
    assert_eq!(runs, 2);
    assert_eq!(host.sfall_previewed(Preview::SlotCost, &[2]), Some(2));
    assert_eq!(host.sfall_previewed(Preview::SlotCost, &[3]), None);
    // With the mode off nothing runs, and nothing is stored.
    let world = crate::WorldState::new(crate::CharacterSheet::default(), 1).unwrap();
    let mut off = WorldHost::new(world, TestAssets::all_art());
    assert_eq!(off.sfall_preview(Preview::SlotCost, vec![1], |_| 9), None);
}

/// The attack sequence of tests 7 and 8, with a chain on each hook.
fn sequence(body: &[Op], hooks: bool) -> (Vec<u64>, u64) {
    let (mut host, [attacker, defender, ..]) = fixture_e("critter", 2);
    host.sfall = Some(Default::default());
    host.world.sfall = Some(Default::default());
    host.assets.programs.clear();
    if hooks {
        let mut start = Vec::new();
        for hook in [
            HOOK_TOHIT,
            HOOK_AFTERHITROLL,
            HOOK_CALCAPCOST,
            HOOK_COMBATDAMAGE,
            HOOK_BARTERPRICE,
            HOOK_MOVECOST,
            HOOK_INVENTORYMOVE,
            HOOK_KEYPRESS,
            HOOK_USEOBJON,
            HOOK_USEOBJ,
            HOOK_SETGLOBALVAR,
            HOOK_GAMEMODECHANGE,
            HOOK_STDPROCEDURE,
            HOOK_ENCOUNTER,
        ] {
            start.extend([I(hook), I(2), REGISTER_PROC]);
        }
        install(&mut host, "gl_0.int", script(&start, &[("m", body)]));
    }
    set_up(&mut host);
    let mut hashes = Vec::new();
    for _ in 0..4 {
        host.sfall_key(true, 30, 65);
        host.sfall_key(false, 30, 65);
        host.world.combat = true;
        host.charge_movement(attacker);
        if host.world.hit_points(defender).is_some_and(|hp| hp > 0) {
            let _ = host
                .perform_attack(attacker, defender, RIGHT, 3, false)
                .unwrap();
        }
        host.sfall_inventory_move(0, 0, 0);
        // The new boundaries, each with one query a step: the rest of the
        // sequence reaches none of them.
        host.hook_use_on_held(attacker, attacker, defender);
        let _ = host.hook_use(attacker, 0);
        host.world.gvars.get_or_insert_with(|| vec![0]);
        run(&mut host, &[I(0), I(0), W(0x80c6)]);
        host.hook_std_procedure(6, attacker, defender, 0, 0);
        host.hook_map_entry(0);
        host.sfall_end_of_step();
        hashes.push(host.world.state_hash());
    }
    (hashes, host.world.sfall_hash())
}

#[test]
fn t7_two_runs_with_a_hook_that_draws_give_identical_streams() {
    let body: &[Op] = &[I(1), I(100), RANDOM, DISCARD];
    let first = sequence(body, true);
    assert_eq!(first, sequence(body, true));
    // The draws are real: they move the gameplay sequence.
    assert_ne!(first.0, sequence(&[], true).0);
}

#[test]
fn t8_hooks_that_return_nothing_change_no_hash() {
    let plain = sequence(&[], false);
    assert_eq!(sequence(&[], true), plain);
}

/// The rows of `vector`, as cells.
fn rows(vector: &str) -> Vec<Vec<&'static str>> {
    VECTORS
        .lines()
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .filter(|cells| cells[0] == vector)
        .collect()
}
/// A member that keeps its `count` arguments in globals `10..` and its run
/// count in global 1, then writes a row's returns: `rets=[41, None]`.
fn member(count: i32, rets: &str) -> Vec<Op> {
    let mut body = vec![I(1), I(1), GLOBAL_INT, I(1), ADD, SET_GLOBAL];
    for index in 0..count {
        body.extend([I(10 + index), GET_ARG, SET_GLOBAL]);
    }
    let values = rets
        .strip_prefix("rets=[")
        .and_then(|rets| rets.strip_suffix(']'))
        .unwrap_or("");
    for value in values.split(", ").filter_map(|value| value.parse().ok()) {
        body.extend([I(value), SET_RETURN]);
    }
    body
}
/// `body` registered on `hook` on `host`.
fn registered(host: &mut WorldHost<TestAssets>, hook: i32, body: &[Op]) {
    let start = [I(hook), I(2), REGISTER_PROC];
    install(host, "gl_0.int", script(&start, &[("m", body)]));
    set_up(host);
}
/// What a row of `hook`'s member saw, and how often it ran.
fn saw(host: &WorldHost<TestAssets>, hook: i32) -> (Vec<i32>, i32) {
    let count = sfall::hook_row(hook).unwrap().args.len() as i32;
    let args = (0..count).map(|index| global(host, 10 + index)).collect();
    (args, global(host, 1))
}
/// A carried item of its own PID, for the use hooks.
fn item(host: &mut WorldHost<TestAssets>, flags: u32) -> ObjectRef {
    let mut bytes = vault_fixtures::world::prototype(0, 5);
    vault_fixtures::world::word(&mut bytes, 0, 0x45);
    vault_fixtures::world::word(&mut bytes, 0x18, flags as i32);
    let prototype = std::sync::Arc::new(vault_formats::Prototype::parse(&bytes).unwrap());
    host.world.spawn(prototype, 20100, 0).unwrap()
}
fn default_ran(host: &WorldHost<TestAssets>) -> bool {
    host.world
        .effects
        .iter()
        .any(|effect| matches!(effect, WorldEffect::DefaultAction { .. }))
}

/// One row of H12.
fn h12_row(cells: &[&str]) {
    let (mut host, _) = sfall_host();
    registered(&mut host, HOOK_GAMEMODECHANGE, &member(2, cells[3]));
    // The first value is the one at the end of setup, which runs nothing.
    host.sfall_end_of_step();
    assert_eq!(saw(&host, HOOK_GAMEMODECHANGE).1, 0, "{cells:?}");
    host.world.combat = true;
    host.sfall_end_of_step();
    assert_eq!(saw(&host, HOOK_GAMEMODECHANGE).1, 1, "{cells:?}");
    let settled = (host.sfall_mode_bits()) as i32;
    match cells[1] {
        "1" => {
            let previous = game_mode::LOCAL_MAP as i32;
            assert_eq!(
                saw(&host, HOOK_GAMEMODECHANGE),
                (vec![0, previous], 1),
                "{cells:?}"
            );
            assert_ne!(settled as u32 & game_mode::COMBAT, 0, "{cells:?}");
            host.sfall_end_of_step();
            assert_eq!(saw(&host, HOOK_GAMEMODECHANGE).1, 1, "no change, no run");
        }
        "2" => {
            host.world.combat = false;
            host.world.combat = true;
            host.sfall_end_of_step();
            assert_eq!(saw(&host, HOOK_GAMEMODECHANGE).1, 1, "{cells:?}");
        }
        _ => {
            assert!(host.script_close_for_test());
            assert!(!host.sfall_session_closed());
            host.sfall_end_of_step();
            assert_eq!(
                saw(&host, HOOK_GAMEMODECHANGE),
                (vec![1, settled], 2),
                "{cells:?}"
            );
            assert!(host.sfall_session_closed());
        }
    }
}

#[test]
fn hook_31_game_mode_change_runs_once_a_step_when_the_bit_set_changed() {
    let row = sfall::hook_row(HOOK_GAMEMODECHANGE).unwrap();
    assert_eq!(
        (row.file, row.args.len(), row.rets.len()),
        ("hs_gamemodechange.int", 2, 0)
    );
    let vector = rows("H12");
    assert_eq!(vector.len(), 3);
    for cells in &vector {
        h12_row(cells);
    }
    // A close in a step that also changed the mode runs it once.
    let (mut host, _) = sfall_host();
    registered(&mut host, HOOK_GAMEMODECHANGE, &member(2, vector[2][3]));
    host.sfall_end_of_step();
    host.world.combat = true;
    host.script_close_for_test();
    host.sfall_end_of_step();
    assert_eq!(saw(&host, HOOK_GAMEMODECHANGE).1, 1);
}

/// One row of H13.
fn h13_row(cells: &[&str]) {
    let (mut host, target) = sfall_host();
    let user = host.world.player;
    let used = item(&mut host, 0);
    registered(&mut host, HOOK_USEOBJON, &member(3, cells[3]));
    host.world.effects.clear();
    host.use_item_stage(user, used, target).unwrap();
    let expected = vec![target as i32, user as i32, used as i32];
    assert_eq!(saw(&host, HOOK_USEOBJON), (expected, 1), "{cells:?}");
    let alive = host.world.get(used).is_some();
    let (routed, kept) = match cells[4] {
        "final=engine routing runs; order=m" => (true, true),
        "final=item kept; no routing; order=m" => (false, true),
        "final=item destroyed; no routing; order=m" => (false, false),
        other => panic!("{other}"),
    };
    assert_eq!((default_ran(&host), alive), (routed, kept), "{cells:?}");
}

#[test]
fn hook_8_use_obj_on_runs_before_routing_for_a_critter_target() {
    let row = sfall::hook_row(HOOK_USEOBJON).unwrap();
    assert_eq!(
        (row.file, row.args.len(), row.rets.len()),
        ("hs_useobjon.int", 3, 1)
    );
    let vector = rows("H13");
    assert_eq!(vector.len(), 5);
    for cells in &vector {
        h13_row(cells);
    }
    // A target that is not a critter runs no chain.
    let (mut host, _) = sfall_host();
    let user = host.world.player;
    let (used, other) = (item(&mut host, 0), item(&mut host, 0));
    registered(&mut host, HOOK_USEOBJON, &member(3, "rets=[0]"));
    host.use_item_stage(user, used, other).unwrap();
    assert_eq!(saw(&host, HOOK_USEOBJON).1, 0);
    // A member's destroy request for the used item is ignored: 1 is the way.
    let (mut host, target) = sfall_host();
    let used = item(&mut host, 0);
    let ops = [GET_ARG, DISCARD, GET_ARG, DISCARD, GET_ARG, W(0x80f4)];
    registered(&mut host, HOOK_USEOBJON, &ops);
    host.world.effects.clear();
    host.use_item_stage(user, used, target).unwrap();
    assert!(host.world.get(used).is_some());
    assert!(
        diagnostics(&mut host)
            .iter()
            .any(|line| line.contains("cannot destroy"))
    );
}

/// A use script on `object`: `use_p_proc` sets global 3, and the request
/// that loads it has run.
fn with_use_script(host: &mut WorldHost<TestAssets>, object: ObjectRef) -> u32 {
    let body = [I(3), I(1), SET_GLOBAL];
    let program = script(&[], &[("use_p_proc", &body)]);
    host.assets.programs.push(("obj.int".to_owned(), program));
    let file = host.assets.programs.len() as i32 - 1;
    let id = host.world.add_script(object, file, 1, 0).unwrap();
    host.dispatch(request(id, ScriptEvent::Critter)).unwrap();
    assert_eq!(global(host, 3), 0, "the first request runs only start");
    id
}

/// One row of H14.
fn h14_row(cells: &[&str]) {
    let (mut host, used) = sfall_host();
    let user = host.world.player;
    with_use_script(&mut host, used);
    registered(&mut host, HOOK_USEOBJ, &member(2, cells[3]));
    host.use_object(user, used).unwrap();
    assert_eq!(
        saw(&host, HOOK_USEOBJ),
        (vec![user as i32, used as i32], 1),
        "{cells:?}"
    );
    assert_eq!(global(&host, 3), 0, "use_p_proc not requested: {cells:?}");
    let destroyed = cells[4].starts_with("final=item destroyed");
    assert_eq!(host.world.get(used).is_none(), destroyed, "{cells:?}");
}

#[test]
fn hook_18_use_obj_runs_before_use_p_proc_and_before_the_drug_path() {
    let row = sfall::hook_row(HOOK_USEOBJ).unwrap();
    assert_eq!(
        (row.file, row.args.len(), row.rets.len()),
        ("hs_useobj.int", 2, 1)
    );
    let vector = rows("H14");
    assert_eq!(vector.len(), 2);
    for cells in &vector {
        h14_row(cells);
    }
    // Unset, the request runs, and a critter's own use of a map object
    // from outside the interface runs no chain.
    let (mut host, used) = sfall_host();
    let user = host.world.player;
    with_use_script(&mut host, used);
    registered(&mut host, HOOK_USEOBJ, &member(2, "rets=[None]"));
    host.use_object(user, used).unwrap();
    assert_eq!(global(&host, 3), 1);
    host.use_object(used, used).unwrap();
    assert_eq!(saw(&host, HOOK_USEOBJ).1, 1);
    // An inventory item with the Use flag runs it, and one without runs
    // HOOK_USEOBJON on the user.
    let (mut host, _) = sfall_host();
    let user = host.world.player;
    let (flagged, plain) = (item(&mut host, 0x800), item(&mut host, 0));
    let mut start = vec![I(HOOK_USEOBJ), I(2), REGISTER_PROC];
    start.extend([I(HOOK_USEOBJON), I(3), REGISTER_PROC]);
    let (first, second) = (member(2, "rets=[0]"), member(3, "rets=[0]"));
    let mut second = second;
    second.extend([I(4), I(1), SET_GLOBAL]);
    install(
        &mut host,
        "gl_0.int",
        script(&start, &[("a", &first), ("b", &second)]),
    );
    set_up(&mut host);
    assert!(host.hook_item_use(user, flagged).unwrap());
    assert_eq!(global(&host, 4), 0);
    assert!(host.hook_item_use(user, plain).unwrap());
    assert_eq!(global(&host, 4), 1);
    assert_eq!(
        saw(&host, HOOK_USEOBJON).0,
        [user as i32, user as i32, plain as i32]
    );
}

/// One row of H15.
fn h15_row(cells: &[&str]) {
    let (mut host, owner) = sfall_host();
    let id = with_use_script(&mut host, owner);
    if cells[2].contains("critter_p_proc") {
        // Events 1, 12, 22 and 23 never run it, and neither does a procedure
        // the script does not have.
        registered(&mut host, HOOK_STDPROCEDURE, &member(6, "rets=[-1]"));
        for event in [
            ScriptEvent::Start,
            ScriptEvent::Critter,
            ScriptEvent::Timed,
            ScriptEvent::MapUpdate,
            ScriptEvent::Talk,
        ] {
            host.dispatch(request(id, event)).unwrap();
        }
        assert_eq!(saw(&host, HOOK_STDPROCEDURE).1, 0, "{cells:?}");
        return;
    }
    registered(&mut host, HOOK_STDPROCEDURE, &member(6, cells[3]));
    let (caller, target) = (host.world.player, item(&mut host, 0));
    let mut use_request = request(id, ScriptEvent::Use);
    (use_request.source, use_request.target, use_request.fixed) = (caller, target, 0);
    let result = host.dispatch(use_request).unwrap();
    let expected = vec![6, owner as i32, caller as i32, 0, target as i32, 0];
    assert_eq!(saw(&host, HOOK_STDPROCEDURE), (expected, 1), "{cells:?}");
    let ran = cells[4].starts_with("final=use_p_proc runs");
    assert_eq!(global(&host, 3) == 1, ran, "{cells:?}");
    assert!(!result.overridden && !result.pending, "the default follows");
}

#[test]
fn hook_40_std_procedure_can_cancel_a_named_request() {
    let row = sfall::hook_row(HOOK_STDPROCEDURE).unwrap();
    assert_eq!(
        (row.file, row.args.len(), row.rets.len()),
        ("hs_stdprocedure.int", 6, 1)
    );
    let vector = rows("H15");
    assert_eq!(vector.len(), 4);
    for cells in &vector {
        h15_row(cells);
    }
}

/// A selected encounter on map 0 of the fixture tables, table 0 and entry
/// 1: the vector's map 40, table 5 and entry 2. Maps 1 and 2 stand for its
/// 41 and 42.
fn encounter_host(rets: &str) -> WorldHost<TestAssets> {
    let (mut host, _) = sfall_host();
    let mut state = crate::fallout2_tests::new_game();
    state.position = [10, 10];
    host.world.fallout2 = Some(Box::new(state));
    host.assets.any_map = Some(crate::world::empty_map());
    // The critters of the entry's groups.
    for pid in [0x0100_0001, 0x0100_0002, 0x0100_0003, 0x0100_0004] {
        let mut prototype = (*crate::test_support::npc_prototype()).clone();
        prototype.pid = pid;
        let prototype = std::sync::Arc::new(prototype);
        host.assets.prototypes.insert(pid, prototype);
    }
    host.world.gvars = Some(vec![0; 8]);
    host.world.clock.set(crate::fallout2::START_TICKS);
    let rets = rets.replace("41", "1").replace("42", "2");
    registered(&mut host, HOOK_ENCOUNTER, &member(5, &rets));
    host
}
/// §8.5's draws for entry 1: the check, the special gate, the pick.
const ENTRY_1: [i32; 5] = [22, 3, 3, 3, 30];

/// Whether groups spawned: objects beyond the map's own.
fn groups(host: &WorldHost<TestAssets>) -> usize {
    host.world
        .iter()
        .filter(|(reference, _)| *reference != host.world.player)
        .count()
}
/// One row of H16.
fn h16_row(cells: &[&str]) {
    let mut host = encounter_host(cells[3]);
    match cells[1] {
        "1" | "2" | "3" | "4" => {
            host.world.rng.script(&ENTRY_1);
            let selected = host.fallout2_check_encounter().unwrap();
            let expected = vec![0, 0, 0, 0, 1];
            assert_eq!(saw(&host, HOOK_ENCOUNTER), (expected, 1), "{cells:?}");
            // The counter stands when the hook cancels (§6.17).
            let state = host.world.fallout2.as_deref().unwrap();
            assert_eq!(state.counter(0, 1), 0, "{cells:?}");
            let final_cell = cells[4];
            let Some(selected) = selected else {
                assert!(
                    final_cell.starts_with("final=travel continues"),
                    "{cells:?}"
                );
                return;
            };
            let map = if final_cell.contains("41") { 1 } else { 0 };
            assert_eq!(selected.map, map, "{cells:?}");
            let spawned = !final_cell.contains("no groups");
            assert_eq!(selected.forced.is_none(), spawned, "{cells:?}");
            host.world.rng.script(&[0, 50, 2, 3]);
            host.fallout2_enter_encounter(selected).unwrap();
            assert_eq!(host.world.rng.unused_script().is_empty(), spawned);
            assert_eq!(groups(&host) > 0, spawned, "{cells:?}");
        }
        "5" | "6" => {
            let map = if cells[4].contains("42") { 2 } else { 0 };
            assert_eq!(host.hook_map_entry(0), map, "{cells:?}");
            let expected = vec![1, 0, 0, -1, -1];
            assert_eq!(saw(&host, HOOK_ENCOUNTER), (expected, 1), "{cells:?}");
        }
        _ => {
            // A forced encounter runs no chain.
            host.world.sfall.as_deref_mut().unwrap().forced_map = 2;
            let selected = host.fallout2_check_encounter().unwrap().unwrap();
            assert_eq!(selected.map, 2);
            assert_eq!(saw(&host, HOOK_ENCOUNTER).1, 0, "{cells:?}");
        }
    }
}

#[test]
fn hook_43_encounter_replaces_cancels_or_keeps_only_the_map() {
    let row = sfall::hook_row(HOOK_ENCOUNTER).unwrap();
    assert_eq!(
        (row.file, row.args.len(), row.rets.len()),
        ("hs_encounter.int", 5, 2)
    );
    let vector = rows("H16");
    assert_eq!(vector.len(), 7);
    for cells in &vector {
        h16_row(cells);
    }
    // Event 1 from an area's entrance: the chain sees the entrance's map,
    // and a replaced map loads, by the header's own entrance.
    let host = encounter_host("rets=[None]");
    let entrance = {
        let state = host.world.fallout2.as_deref().unwrap();
        state.entrance_of(0, 0).unwrap()
    };
    let wanted = if entrance.map == 1 { 2 } else { 1 };
    let mut host = encounter_host(&format!("rets=[{wanted}]"));
    let mut replaced = crate::world::empty_map();
    replaced.header.entry_orientation = 5;
    let path = host.fallout2_map_path(wanted).unwrap();
    host.assets.maps.insert(path, replaced);
    host.fallout2_enter_entrance(0, 0).unwrap();
    let expected = vec![1, entrance.map as i32, 0, -1, -1];
    assert_eq!(saw(&host, HOOK_ENCOUNTER), (expected, 1));
    assert_eq!(
        host.world.map.header.entry_orientation, 5,
        "the map replaced"
    );
    // Event 1 from the drop to terrain.
    let mut host = encounter_host("rets=[-1]");
    if host.fallout2_drop_to_terrain().unwrap() {
        assert_eq!(saw(&host, HOOK_ENCOUNTER).1, 1);
    }
    // A map with no row is ignored, with a diagnostic.
    let mut host = encounter_host("rets=[9999]");
    assert_eq!(host.hook_map_entry(0), 0);
    assert!(
        diagnostics(&mut host)
            .iter()
            .any(|line| line.contains("no row"))
    );
}

/// One row of H17.
fn h17_row(cells: &[&str]) {
    let (mut host, _) = sfall_host();
    host.world.gvars = Some(vec![0; 16]);
    if cells[4].starts_with("final=no chain runs") {
        // An invalid index runs nothing.
        registered(&mut host, HOOK_SETGLOBALVAR, &member(2, "rets=[None]"));
        run(&mut host, &[I(99), I(5), W(0x80c6)]);
        assert_eq!(saw(&host, HOOK_SETGLOBALVAR).1, 0, "{cells:?}");
        return;
    }
    registered(&mut host, HOOK_SETGLOBALVAR, &member(2, cells[3]));
    run(&mut host, &[I(12), I(5), W(0x80c6)]);
    assert_eq!(saw(&host, HOOK_SETGLOBALVAR), (vec![12, 5], 1), "{cells:?}");
    let stored = host.world.gvars.as_ref().unwrap()[12];
    assert_eq!(format!("final=GVAR 12 = {stored}; order=m"), cells[4]);
}

/// A global script's own member, while that script is still running.
#[test]
fn a_script_that_writes_a_global_in_its_start_runs_its_own_member() {
    let (mut host, _) = sfall_host();
    host.world.gvars = Some(vec![0; 16]);
    let start = [
        I(HOOK_SETGLOBALVAR),
        I(2),
        REGISTER_PROC,
        I(3),
        I(5),
        W(0x80c6),
    ];
    let mut body = member(2, "rets=[9]");
    body.extend([I(4), I(1), SET_GLOBAL]);
    install(&mut host, "gl_0.int", script(&start, &[("m", &body)]));
    set_up(&mut host);
    assert_eq!(saw(&host, HOOK_SETGLOBALVAR), (vec![3, 5], 1));
    assert_eq!(
        host.world.gvars.as_ref().unwrap()[3],
        9,
        "the return stands"
    );
    assert_eq!(global(&host, 4), 1, "the member ran to its end");
    assert_eq!(diagnostics(&mut host), Vec::<String>::new());
    // The script goes on after the word, and its stack is as it was.
    run(&mut host, &[I(6), I(1), W(0x80c6)]);
    assert_eq!(saw(&host, HOOK_SETGLOBALVAR), (vec![6, 1], 2));
}

/// §6.18: a member's own write runs a nested chain on that same member,
/// each with its own context, until §6.5's limit. Past it the write stores.
#[test]
fn a_member_that_writes_a_global_nests_its_own_chain_to_the_limit() {
    let (mut host, _) = sfall_host();
    host.world.gvars = Some(vec![0; 16]);
    let body = [
        I(1),
        I(1),
        GLOBAL_INT,
        I(1),
        ADD,
        SET_GLOBAL,
        GET_ARG,
        I(1),
        ADD,
        I(7),
        W(0x80c6),
        I(9),
        SET_RETURN,
    ];
    registered(&mut host, HOOK_SETGLOBALVAR, &body);
    host.world.effects.clear();
    run(&mut host, &[I(3), I(5), W(0x80c6)]);
    assert_eq!(global(&host, 1), 8, "eight nested chains");
    let stored = host.world.gvars.as_ref().unwrap();
    // Each outer chain's return was written after its nested one came back.
    assert_eq!(
        stored[3..=10],
        [9; 8],
        "the outer contexts resumed untouched"
    );
    assert_eq!(stored[11], 7, "the ninth write still stores its value");
    let lines = diagnostics(&mut host);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("past 8 nested chains"), "{lines:?}");
}

/// Two scripts write inside each other's chains: neither is skipped, and
/// §6.5's limit ends the walk.
#[test]
fn two_members_that_write_globals_reach_each_other() {
    let (mut host, _) = sfall_host();
    host.world.gvars = Some(vec![0; 16]);
    let register = [I(HOOK_SETGLOBALVAR), I(2), REGISTER_PROC];
    let stepper = |digit: i32| {
        let mut body = traced(digit);
        body.extend([GET_ARG, I(1), ADD, I(7), W(0x80c6)]);
        body
    };
    let (first, second) = (stepper(1), stepper(2));
    install(&mut host, "gl_0.int", script(&register, &[("m", &first)]));
    install(&mut host, "gl_1.int", script(&register, &[("m", &second)]));
    set_up(&mut host);
    host.world.effects.clear();
    run(&mut host, &[I(3), I(5), W(0x80c6)]);
    let lines = diagnostics(&mut host);
    assert!(
        !lines.iter().any(|line| line.contains("is running")),
        "{lines:?}"
    );
}

#[test]
fn hook_29_set_global_var_can_replace_the_value_stored() {
    let row = sfall::hook_row(HOOK_SETGLOBALVAR).unwrap();
    assert_eq!(
        (row.file, row.args.len(), row.rets.len()),
        ("hs_setglobalvar.int", 2, 1)
    );
    let vector = rows("H17");
    assert_eq!(vector.len(), 3);
    for (cells, expected) in vector[..2].iter().zip([5, 9]) {
        let (mut host, _) = sfall_host();
        host.world.gvars = Some(vec![0; 16]);
        registered(&mut host, HOOK_SETGLOBALVAR, &member(2, cells[3]));
        run(&mut host, &[I(12), I(5), W(0x80c6)]);
        assert_eq!(saw(&host, HOOK_SETGLOBALVAR), (vec![12, 5], 1), "{cells:?}");
        assert_eq!(
            host.world.gvars.as_ref().unwrap()[12],
            expected,
            "{cells:?}"
        );
    }
    // H17 step 3: an invalid index runs nothing.
    let (mut host, _) = sfall_host();
    host.world.gvars = Some(vec![0; 16]);
    registered(&mut host, HOOK_SETGLOBALVAR, &member(2, "rets=[None]"));
    run(&mut host, &[I(99), I(5), W(0x80c6)]);
    assert_eq!(saw(&host, HOOK_SETGLOBALVAR).1, 0);
    // A member that writes a global runs a nested chain, and past the depth
    // limit the write still stores its value.
    let (mut host, _) = sfall_host();
    host.world.gvars = Some(vec![0; 16]);
    registered(&mut host, HOOK_SETGLOBALVAR, &member(2, "rets=[None]"));
    host.fill_contexts_for_test(8);
    run(&mut host, &[I(3), I(7), W(0x80c6)]);
    assert_eq!(host.world.gvars.as_ref().unwrap()[3], 7);
    assert_eq!(saw(&host, HOOK_SETGLOBALVAR).1, 0);
}

/// Each member's trace digit in the vectors' `order=` lists.
fn trace_of(order: &str) -> i32 {
    order
        .trim_start_matches("order=")
        .split(',')
        .map(|member| match member {
            "ModA" | "r1" | "early" | "m" => 1,
            "ModB" | "r2" | "late" => 2,
            "hs_tohit" => 3,
            "s1" => 4,
            "s2" => 5,
            other => panic!("{other}"),
        })
        .fold(0, |trace, digit| trace * 10 + digit)
}
/// `final=...; order=...`: the two cells of a row's column.
fn after(cells: &[&str]) -> (&'static str, &'static str) {
    let cell: &'static str = Box::leak(cells[4].to_owned().into_boxed_str());
    let (value, order) = cell.split_once("; ").unwrap();
    (value.trim_start_matches("final="), order)
}
/// The returns `rets=[...]` lists, as a member writes them.
fn written(cells: &[&str]) -> Vec<Op> {
    member(0, cells[3])[6..].to_vec()
}
/// H1 and H2: the published example. `ModA` divides by 2 and `ModB`
/// multiplies by 1.5, so the last member's type shows in the return.
fn h1_h2_row(cells: &[&str]) {
    let halve = [
        GET_ARG,
        I(2),
        DIVIDE,
        DUPLICATE,
        SET_RETURN,
        I(0),
        SWAP,
        SET_ARG,
    ];
    let raise = [
        GET_ARG,
        F(1.5),
        MULTIPLY,
        DUPLICATE,
        SET_RETURN,
        I(0),
        SWAP,
        SET_ARG,
    ];
    let body = |digit, ops: &[Op]| {
        let mut body = traced(digit);
        body.extend_from_slice(ops);
        body
    };
    let (mod_a, mod_b) = (body(1, &halve), body(2, &raise));
    let (first, second) = if cells[2].contains("registered ModA,ModB") {
        (&mod_a, &mod_b)
    } else {
        (&mod_b, &mod_a)
    };
    let (mut host, _) = sfall_host();
    let register = [I(HOOK_TOHIT), I(2), REGISTER_PROC];
    install(&mut host, "gl_0.int", script(&register, &[("m", first)]));
    install(&mut host, "gl_1.int", script(&register, &[("m", second)]));
    set_up(&mut host);
    let rets = host.run_hook(HOOK_TOHIT, ints(&[100, 0, 0, 3, 0, 0, 1, 100]));
    let (value, order) = after(cells);
    assert_eq!(
        rets.unwrap()[0],
        Some(SfallValue::Int(value.parse().unwrap()))
    );
    assert_eq!(global(&host, 1), trace_of(order), "{cells:?}");
}
fn h3_row(cells: &[&str]) {
    let hook = HOOK_TOHIT;
    let (mut host, _) = sfall_host();
    let register = |word| [I(hook), I(2), word];
    for (name, digit, word) in [
        ("gl_0r1.int", 1, REGISTER_PROC),
        ("gl_1r2.int", 2, REGISTER_PROC),
        ("gl_2s1.int", 4, REGISTER_SPEC),
        ("gl_3s2.int", 5, REGISTER_SPEC),
    ] {
        let body = traced(digit);
        install(&mut host, name, script(&register(word), &[("m", &body)]));
    }
    let body = traced(3);
    install(&mut host, "hs_tohit.int", script(&body, &[]));
    set_up(&mut host);
    if cells[1] == "2" {
        let r2 = host.global_for_test(1);
        host.unregister_for_test(hook, r2);
    }
    host.world.sfall.as_mut().unwrap().globals.clear();
    let rets = host.run_hook(hook, ints(&[50])).unwrap();
    assert_eq!(rets, [None], "{cells:?}");
    let (value, order) = after(cells);
    assert_eq!(value, "50", "the engine's value stands");
    assert_eq!(global(&host, 1), trace_of(order), "{cells:?}");
}
/// H4's hook is tier P2, so no chain of it runs (§6.19). The composition it
/// pins is the chain's, and `HOOK_COMBATDAMAGE` has the returns to show it.
fn h4_row(cells: &[&str]) {
    let hook = HOOK_COMBATDAMAGE;
    let (mut host, _) = sfall_host();
    let mut early = traced(1);
    early.extend([I(10), SET_RETURN, I(20), SET_RETURN]);
    let mut late = traced(2);
    late.extend([I(30), SET_RETURN]);
    let register = [I(hook), I(2), REGISTER_SPEC];
    install(&mut host, "gl_0.int", script(&register, &[("m", &early)]));
    install(&mut host, "gl_1.int", script(&register, &[("m", &late)]));
    set_up(&mut host);
    let rets = host.run_hook(hook, ints(&[5, 6, 7, 8, 9])).unwrap();
    assert_eq!(after(cells).0, "[30, 20]");
    assert_eq!(
        rets[..2],
        [Some(SfallValue::Int(30)), Some(SfallValue::Int(20))]
    );
    assert_eq!(global(&host, 1), trace_of(after(cells).1), "{cells:?}");
}
fn h5_h6_row(cells: &[&str]) {
    let hook = HOOK_MOVECOST;
    let (body, args): (&[Op], i32) = if cells[0] == "H5" {
        (&[I(1), SET_RETURN, I(2), SET_RETURN], 0)
    } else {
        (&[GET_ARG, DISCARD, GET_ARG, SET_RETURN], 11)
    };
    let mut host = hooked(hook, body);
    let rets = host.run_hook(hook, ints(&[args])).unwrap();
    let expected: i32 = after(cells).0.trim_matches(['[', ']']).parse().unwrap();
    assert_eq!(rets, [Some(SfallValue::Int(expected))], "{cells:?}");
}
fn h7_row(cells: &[&str]) {
    let (mut host, [attacker, _, defender, _]) = attack_host(HOOK_TOHIT, &written(cells));
    let attack = host
        .world
        .attack_profile(
            attacker,
            defender,
            0,
            crate::critical::location::TORSO,
            false,
        )
        .unwrap();
    let chance = host
        .hook_hit_chance(attacker, defender, &attack, true, None)
        .unwrap();
    // With no return the engine's value stays, whatever the fixture's is.
    let expected = if cells[3] == "rets=[None]" {
        attack.hit_chance(true).unwrap()
    } else {
        after(cells).0.parse().unwrap()
    };
    assert_eq!(chance, expected, "{cells:?}");
}
fn h8_row(cells: &[&str]) {
    let (mut host, [attacker, _, defender, _]) = attack_host(HOOK_AFTERHITROLL, &written(cells));
    let mut attack = host
        .world
        .attack_profile(attacker, defender, 0, 3, false)
        .unwrap();
    let mut target = defender;
    let mut rng = host.world.rng.clone();
    let roll = host
        .hook_after_roll(attacker, &mut target, &mut attack, 2, &mut rng)
        .unwrap();
    assert_eq!(
        after(cells).0,
        format!("roll={roll},part={},target=T", attack.location)
    );
    assert_eq!(target, defender, "{cells:?}");
}
fn h9_row(cells: &[&str]) {
    let (mut host, [_, merchant, ..]) = attack_host(HOOK_BARTERPRICE, &written(cells));
    let session = host.begin_barter(merchant).unwrap();
    let plain = host.barter_price(&session);
    let offer = host.world.inventory_value(session.offer);
    let (price, value) = host.hooked_barter(&session, true);
    // The row's 150 and 120 stand for the engine's own values.
    let shown = format!(
        "price={},offer={}",
        if price == plain { 150 } else { price },
        if value == offer { 120 } else { value }
    );
    assert_eq!(after(cells).0, shown, "{cells:?}");
}
fn h10_row(cells: &[&str]) {
    let moves = hooked(HOOK_INVENTORYMOVE, &written(cells)).sfall_inventory_move(1, 7, 0);
    assert_eq!(after(cells).0 == "move proceeds", moves, "{cells:?}");
}
fn h11_row(cells: &[&str]) {
    // ret0 is the row's value on a press, and 0 on a release.
    let ret = cells[2]
        .split_once("ret0=")
        .and_then(|(_, rest)| rest.split(' ').next())
        .unwrap();
    let press: i32 = if cells[2].contains("release") {
        48
    } else {
        ret.parse().unwrap()
    };
    let body = [GET_ARG, I(press), MULTIPLY, SET_RETURN];
    let mut host = hooked(HOOK_KEYPRESS, &body);
    let pressed = host.sfall_key(true, 30, 65);
    let logical = if cells[2].contains("release") {
        host.sfall_key(false, 30, 65)
    } else {
        pressed
    };
    assert!(
        after(cells)
            .0
            .starts_with(&format!("logical key {logical}")),
        "{cells:?}"
    );
}
/// One row of an H vector, or of SS2, against the engine.
pub(crate) fn hook_vector_row(cells: &[&str]) {
    match cells[0] {
        "H1" | "H2" => h1_h2_row(cells),
        "H3" => h3_row(cells),
        "H4" => h4_row(cells),
        "H5" | "H6" => h5_h6_row(cells),
        "H7" => h7_row(cells),
        "H8" => h8_row(cells),
        "H9" => h9_row(cells),
        "H10" => h10_row(cells),
        "H11" => h11_row(cells),
        "H12" => h12_row(cells),
        "H13" => h13_row(cells),
        "H14" => h14_row(cells),
        "H15" => h15_row(cells),
        "H16" => h16_row(cells),
        "H17" => h17_row(cells),
        "SS2" => ss2_row(cells),
        other => panic!("no hook vector {other}"),
    }
}

#[test]
fn with_the_mode_off_the_hook_paths_return_the_engine_value() {
    let world = crate::WorldState::new(crate::CharacterSheet::default(), 1).unwrap();
    let mut off = WorldHost::new(world, TestAssets::all_art());
    assert_eq!(off.hook_fixed_cost(0, 2, 0), 2);
    assert_eq!(off.hook_move_cost(0, 1, 4), 4);
    assert!(off.sfall_inventory_move(1, 2, 3));
    assert_eq!(off.sfall_key(true, 30, 65), 30);
    assert_eq!(off.run_hook(HOOK_TOHIT, Vec::new()), None);
    assert_eq!(off.hook_global_var(&mut Vm::new(base(&[])), 1, 5), 5);
    assert_eq!(off.hook_map_entry(3), 3);
    assert!(!off.hook_std_procedure(6, 0, 0, 0, 0));
    assert!(!off.hook_item_use(0, 0).unwrap());
    off.sfall_end_of_step();
    off.sfall_setup_if_due();
}
