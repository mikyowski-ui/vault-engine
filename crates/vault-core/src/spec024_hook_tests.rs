//! SPEC-024b part 3: registration, chains and composition on the H vectors
//! of `sfall-array-vectors.tsv`, the eight hook boundaries, and §7.3 tests 7
//! and 8.
use crate::{
    WorldHost, fixture_state as fixture,
    host_ai_tests::fixture_e,
    host_sfall_hooks::{
        HOOK_AFTERHITROLL, HOOK_BARTERPRICE, HOOK_CALCAPCOST, HOOK_COMBATDAMAGE,
        HOOK_INVENTORYMOVE, HOOK_KEYPRESS, HOOK_MOVECOST, HOOK_TOHIT, Preview,
    },
    spec024_tests::{Op, sfall_host},
    test_support::TestAssets,
    world::ObjectRef,
};
use Op::{F, I, W};
use vault_vm::{IntProgram, WordSet, sfall::SfallValue};

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

fn encode(ops: &[Op]) -> Vec<u8> {
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
            Op::S(_) => panic!("hook fixtures hold no strings"),
        }
    }
    bytes
}
/// A compiled script's shape: the initial stream runs `start`, procedure 1,
/// and `procedures` follow it at indices 2 up.
fn script(start: &[Op], procedures: &[(&'static str, &[Op])]) -> IntProgram {
    let mut listed: Vec<(&[u8], Vec<u8>)> = vec![(b"start", fixture::returning(encode(start)))];
    for (name, body) in procedures {
        listed.push((name.as_bytes(), fixture::returning(encode(body))));
    }
    let mut bytes = fixture::program(&[], &listed);
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

#[test]
fn d_t1_set_sfall_arg_takes_an_int_whatever_the_argument_declares() {
    let hook = HOOK_COMBATDAMAGE;
    // A float truncates, and argument 12, `mixed`, takes the int too.
    let mut body = vec![I(12), F(7.9), SET_ARG, I(2), F(-3.9), SET_ARG];
    for index in 0..13 {
        body.extend([I(20 + index), GET_ARG, SET_GLOBAL]);
    }
    let (mut host, _) = sfall_host();
    install(
        &mut host,
        "gl_0.int",
        script(&[I(hook), I(2), REGISTER_PROC], &[("m", &body)]),
    );
    set_up(&mut host);
    let mut args = ints(&[0; 13]);
    args[12] = SfallValue::text("mixed");
    host.run_hook(hook, args);
    assert_eq!((global(&host, 32), global(&host, 22)), (7, -3));
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

#[test]
fn with_the_mode_off_the_hook_paths_return_the_engine_value() {
    let world = crate::WorldState::new(crate::CharacterSheet::default(), 1).unwrap();
    let mut off = WorldHost::new(world, TestAssets::all_art());
    assert_eq!(off.hook_fixed_cost(0, 2, 0), 2);
    assert_eq!(off.hook_move_cost(0, 1, 4), 4);
    assert!(off.sfall_inventory_move(1, 2, 3));
    assert_eq!(off.sfall_key(true, 30, 65), 30);
    assert_eq!(off.run_hook(HOOK_TOHIT, Vec::new()), None);
    off.sfall_end_of_step();
    off.sfall_setup_if_due();
}
