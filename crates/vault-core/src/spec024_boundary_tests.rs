//! SPEC-024b §6.19: the sixteen hooks Revision 4 promoted, each at its
//! boundary, on vectors H18 to H33. A row names its arguments by symbol;
//! [`expected`] resolves them, so what the member saw is the row's own.
use crate::{
    LootRequest, ObjectAction, WorldHost,
    combat::Resolution,
    combat_state::{CombatAi, CombatProgress},
    critical::effect,
    host_choreography::Playback,
    host_rest::{RestOutcome, RestRequest},
    host_sfall_boundaries::{
        HOOK_ADJUSTFID, HOOK_ADJUSTRADS, HOOK_AMMOCOST, HOOK_CANUSEWEAPON, HOOK_CARTRAVEL,
        HOOK_COMBATTURN, HOOK_DEATHANIM2, HOOK_DESCRIPTIONOBJ, HOOK_INVENWIELD, HOOK_MOUSECLICK,
        HOOK_RESTTIMER, HOOK_SETLIGHTING, HOOK_STEAL, HOOK_USEANIMOBJ, HOOK_USESKILLON,
        HOOK_WITHINPERCEPTION, perception,
    },
    host_sfall_hooks::Preview,
    loot::{LootMove, LootSession},
    motion_tests::labels,
    spec024_hook_tests::{
        REGISTER_PROC, RIGHT, SET_RETURN, attack_host, global, install, item, member, registered,
        rows, saw, script, script_with, set_up,
    },
    spec024_tests::{Op, diagnostics, run, sfall_host},
    test_support::{TestAssets, request},
    world::ObjectRef,
};
use Op::{I, W};
use vault_formats::pro::BasicFlags;

const GET_ARG: Op = W(0x81e4);
const EQUAL: u16 = 0x8033;
const AND: u16 = 0x803e;
const ADD: u16 = 0x8039;
const DISCARD: u16 = 0x801a;
const GLOBAL_INT: u16 = 0x819e;
const SET_GLOBAL: u16 = 0x819d;

/// A row's `args=` as ints, each symbol resolved by `names`.
fn expected(cells: &[&str], names: &[(&str, ObjectRef)]) -> Vec<i32> {
    let (_, args) = cells[2].split_once("args=").expect("a row with arguments");
    args.split(',')
        .map(|arg| {
            if let Some(hex) = arg.strip_prefix("0x") {
                return i32::from_str_radix(hex, 16).unwrap();
            }
            if let Ok(value) = arg.parse() {
                return value;
            }
            let found = names.iter().find(|(name, _)| *name == arg);
            found.unwrap_or_else(|| panic!("{arg}")).1 as i32
        })
        .collect()
}
fn final_state<'a>(cells: &[&'a str]) -> &'a str {
    cells[4]
        .strip_prefix("final=")
        .and_then(|state| state.strip_suffix("; order=m"))
        .unwrap_or(cells[4])
}
fn no_chain(cells: &[&str]) -> bool {
    cells[4].starts_with("final=no chain runs")
}
fn messages(host: &WorldHost<TestAssets>) -> Vec<Vec<u8>> {
    let lines = host.world.effects.iter().filter_map(|effect| match effect {
        crate::WorldEffect::Message(text) => Some(text.clone()),
        _ => None,
    });
    lines.collect()
}

pub(crate) fn h18_row(cells: &[&str]) {
    let (mut host, [attacker, _, target, _]) = attack_host(HOOK_DEATHANIM2, &member(5, cells[3]));
    if no_chain(cells) {
        let state = host.world.critter_state_mut(target).unwrap();
        state.results.0 |= effect::KNOCKED_DOWN;
        host.present_script_damage(target, 50, 0, false, true)
            .unwrap();
        assert_eq!(saw(&host, HOOK_DEATHANIM2).1, 0, "{cells:?}");
        return;
    }
    let chosen = host.hook_death(attacker, target, None, 50, 20);
    let names = [("A", attacker), ("T", target)];
    let args = (expected(cells, &names), 1);
    assert_eq!(saw(&host, HOOK_DEATHANIM2), args, "{cells:?}");
    assert_eq!(format!("animation {chosen}"), final_state(cells));
}
#[test]
fn hook_4_death_animation_by_vector_h18() {
    rows("H18").iter().for_each(|row| h18_row(row));
}
#[test]
fn hook_4_a_critter_dmg_death_names_no_weapon_and_plays_what_the_chain_chose() {
    let (mut host, [_, _, target, _]) = attack_host(HOOK_DEATHANIM2, &member(5, "rets=[23]"));
    host.present_script_damage(target, 50, 0, false, true)
        .unwrap();
    let (args, runs) = saw(&host, HOOK_DEATHANIM2);
    assert_eq!(runs, 1);
    assert_eq!((args[0], args[2], args[3]), (-1, target as i32, 50));
    let art = host.world.get(target).unwrap().raw.art_id;
    assert_eq!(vault_formats::Fid(art).animation(), 23, "the fall it shows");
}
#[test]
fn hook_4_a_lethal_attack_names_the_weapon_pid() {
    let (mut host, [attacker, _, target, _]) = attack_host(HOOK_DEATHANIM2, &member(5, "rets=[]"));
    let gun = host.world.wielded(attacker, RIGHT).unwrap();
    let pid = host.world.get(gun).unwrap().raw.pid as i32;
    host.world.critter_state_mut(target).unwrap().hp = 1;
    host.world.rng.script(&[]);
    for _ in 0..40 {
        if host.world.is_dead(target) {
            break;
        }
        host.world.critter_state_mut(attacker).unwrap().ap = 8;
        let _ = host
            .perform_attack(attacker, target, RIGHT, 3, false)
            .unwrap();
        host.motions.clear();
        host.world.get_mut(target).unwrap().raw.flags.0 &= !BasicFlags::HIDDEN.0;
    }
    let (args, runs) = saw(&host, HOOK_DEATHANIM2);
    assert_eq!(runs, 1, "the target dies once");
    assert_eq!(
        (args[0], args[1], args[2]),
        (pid, attacker as i32, target as i32)
    );
}
#[test]
fn hook_4_an_animation_with_no_art_leaves_the_engines_with_a_diagnostic() {
    let (mut host, [attacker, _, target, _]) =
        attack_host(HOOK_DEATHANIM2, &member(5, "rets=[23]"));
    host.assets.missing.insert(23);
    diagnostics(&mut host);
    assert_eq!(host.hook_death(attacker, target, None, 50, 20), 20);
    assert_eq!(diagnostics(&mut host).len(), 1);
}

fn magazine(host: &WorldHost<TestAssets>, gun: ObjectRef) -> i32 {
    host.world.loaded(gun).0
}
pub(crate) fn h19_row(cells: &[&str]) {
    let (mut host, [attacker, _, defender, _]) = attack_host(HOOK_AMMOCOST, &member(4, cells[3]));
    let gun = host.world.wielded(attacker, RIGHT).unwrap();
    let names = [("W", gun)];
    let args = expected(cells, &names);
    let kind = args[3];
    let attack = host
        .world
        .attack_profile(attacker, defender, RIGHT, 8, false)
        .unwrap();
    let outcome = match kind {
        2 => {
            let mut rng = crate::ParkMiller::seeded(1).unwrap();
            let cost = host
                .hook_burst_cost(attacker, &attack, 10, &mut rng)
                .unwrap();
            format!("{cost} rounds each")
        }
        3 => {
            let burst = Resolution {
                ammo: 5,
                burst_cost: Some(1),
                ..Resolution::default()
            };
            format!("{} rounds spent", host.hook_ammo_spent(gun, &burst))
        }
        _ => {
            host.world.get_mut(gun).unwrap();
            let before = magazine(&host, gun);
            host.perform_attack(attacker, defender, RIGHT, 8, false)
                .unwrap()
                .unwrap();
            let spent = before - magazine(&host, gun);
            let unit = if spent == 1 { "round" } else { "rounds" };
            format!("{spent} {unit} spent")
        }
    };
    assert_eq!(saw(&host, HOOK_AMMOCOST), (args, 1), "{cells:?}");
    assert_eq!(outcome, final_state(cells));
}
#[test]
fn hook_17_ammo_cost_by_vector_h19() {
    rows("H19").iter().for_each(|row| h19_row(row));
}
#[test]
fn hook_17_a_burst_counts_then_charges_and_keeps_its_damage() {
    let burst = |rets: Option<&str>| {
        let (mut host, defender, _) =
            crate::combat_tests::spray_world(crate::combat::attack_mode::BURST);
        host.sfall = Some(Default::default());
        host.hook_host = Some(Default::default());
        host.world.sfall = Some(Default::default());
        if let Some(rets) = rets {
            registered(&mut host, HOOK_AMMOCOST, &member(4, rets));
        }
        let player = host.world.player;
        host.world.get_mut(player).unwrap().raw.hex = Some(20100);
        let gun = host.world.wielded(player, RIGHT).unwrap();
        let rounds = 5;
        let attack = host
            .world
            .attack_profile(player, defender, RIGHT, 8, false)
            .unwrap();
        let mut rng = crate::ParkMiller::seeded(7).unwrap();
        let spray = host
            .resolve_spray(player, defender, &attack, &mut rng)
            .unwrap();
        let spent = host.hook_ammo_spent(gun, &spray.main);
        let seen = saw(&host, HOOK_AMMOCOST);
        (spray.main.damage, spray.main.ammo, spent, seen, rounds, gun)
    };
    let (plain, ammo, spent, _, _, gun) = burst(None);
    assert_eq!(spent, ammo, "the cost per round is 1 with no chain");
    let (damage, hooked_ammo, spent, (args, runs), _, _) = burst(Some("rets=[2]"));
    assert_eq!(damage, plain, "the damage does not change");
    assert_eq!(hooked_ammo, ammo, "nor do the rounds fired");
    assert_eq!(runs, 2, "type 2, then type 3");
    assert_eq!(
        args,
        [gun as i32, ammo, ammo * 2, 3],
        "the cost reached type 3"
    );
    assert_eq!(spent, 2, "type 3 has the last word");
    let (_, _, spent, _, _, _) = burst(Some("rets=[-1]"));
    assert_eq!(
        spent, ammo,
        "a negative cost leaves 1, and a negative total the engine's"
    );
}
#[test]
fn hook_17_type_1_never_runs() {
    // D-53: the engine has no query of that type to run.
    let (mut host, [attacker, _, defender, _]) = attack_host(HOOK_AMMOCOST, &member(4, "rets=[]"));
    host.perform_attack(attacker, defender, RIGHT, 8, false)
        .unwrap()
        .unwrap();
    let (args, runs) = saw(&host, HOOK_AMMOCOST);
    assert_eq!((args[3], runs), (0, 1));
}

pub(crate) fn h20_row(cells: &[&str]) {
    let (mut host, _) = sfall_host();
    registered(&mut host, HOOK_MOUSECLICK, &member(2, cells[3]));
    let args = expected(cells, &[]);
    assert_eq!(cells[3], "rets=[]", "it has no returns");
    host.sfall_mouse(args[0] == 1, args[1]);
    assert_eq!(saw(&host, HOOK_MOUSECLICK), (args, 1), "{cells:?}");
}
#[test]
fn hook_20_mouse_click_by_vector_h20() {
    rows("H20").iter().for_each(|row| h20_row(row));
}

/// A steal session on `victim`, who carries `loot`.
fn steal_session(host: &mut WorldHost<TestAssets>, victim: ObjectRef) -> (LootSession, ObjectRef) {
    let loot = item(host, 0);
    host.world.add_item(victim, loot, 1, true).unwrap();
    let request = LootRequest {
        looter: host.world.player,
        target: victim,
        steal: true,
    };
    let mut session = host.open_loot(request).unwrap().unwrap();
    session.press();
    (session, loot)
}
pub(crate) fn h21_row(cells: &[&str]) {
    let (mut host, victim) = sfall_host();
    let player = host.world.player;
    host.world.get_mut(player).unwrap().raw.hex = Some(19900);
    let (mut session, loot) = steal_session(&mut host, victim);
    let args = expected(cells, &[("T", player), ("V", victim), ("I", loot)]);
    let planting = args[3] == 1;
    if planting {
        host.world.remove_item(victim, loot, 1).unwrap();
        host.world.add_item(player, loot, 1, true).unwrap();
    }
    registered(&mut host, HOOK_STEAL, &member(5, cells[3]));
    host.world.effects.clear();
    let engine = final_state(cells).starts_with("section 3.9.3");
    if engine {
        // Not caught: a success, and the catch's failure.
        host.world.rng.script(&[1, 100, 100, 100]);
    }
    let state = host.world.rng.state().clone();
    let outcome = host.loot_move(&mut session, !planting, loot, 1).unwrap();
    assert_eq!(saw(&host, HOOK_STEAL), (args, 1), "{cells:?}");
    let holder = if planting { victim } else { player };
    let moved = host
        .world
        .get(holder)
        .unwrap()
        .inventory
        .iter()
        .any(|(held, _)| *held == loot);
    let pending = session.experience.pending;
    if engine {
        assert!(host.world.rng.unused_script().len() < 4, "the check drew");
        assert_eq!(outcome, LootMove::Moved);
        return;
    }
    assert_eq!(host.world.rng.state(), &state, "no number is drawn");
    assert!(messages(&host).is_empty(), "nothing prints");
    match final_state(cells) {
        "moved, not caught, no draw, pending XP +40" => {
            assert_eq!((outcome, moved, pending), (LootMove::Moved, true, 40));
        }
        "caught, no draw, window closes" => {
            assert_eq!((outcome, moved), (LootMove::Caught, false));
            assert!(session.caught);
        }
        "not moved, no draw, window stays open" => {
            assert_eq!((outcome, moved), (LootMove::Refused, false));
            assert!(!session.caught);
        }
        "moved, pending XP by section 3.9.5" => {
            assert_eq!((outcome, moved, pending), (LootMove::Moved, true, 10));
        }
        other => panic!("{other}"),
    }
}
#[test]
fn hook_22_steal_by_vector_h21() {
    rows("H21").iter().for_each(|row| h21_row(row));
}

pub(crate) fn h22_row(cells: &[&str]) {
    let (mut host, [watcher, blocker, target, far]) =
        attack_host(HOOK_WITHINPERCEPTION, &member(4, cells[3]));
    let args = expected(cells, &[("W", watcher), ("T", target)]);
    host.world.erase(far);
    if args[2] == 0 {
        // Out of range, on the same line.
        host.world.get_mut(target).unwrap().raw.hex = Some(20100 + 60 * 200);
    }
    if args[3] == perception::CAN_SEE && !final_state(cells).contains("no blocker ray") {
        host.world.erase(blocker);
    }
    let result = if args[3] == perception::CAN_SEE {
        host.hooked_can_see(watcher, target)
    } else {
        host.perceives(watcher, target, args[3])
    };
    assert_eq!(saw(&host, HOOK_WITHINPERCEPTION), (args, 1), "{cells:?}");
    let shown = match final_state(cells) {
        "in range" | "sees unless blocked" | "sees, no blocker ray" => true,
        "cannot see" => false,
        other => panic!("{other}"),
    };
    assert_eq!(result, shown, "{cells:?}");
}
#[test]
fn hook_23_within_perception_by_vector_h22() {
    rows("H22").iter().for_each(|row| h22_row(row));
}
#[test]
fn hook_23_in_range_still_needs_a_clear_ray() {
    let (mut host, [watcher, _, target, _]) =
        attack_host(HOOK_WITHINPERCEPTION, &member(4, "rets=[1]"));
    // Target out of range, with a critter on the line.
    host.world.get_mut(target).unwrap().raw.hex = Some(20100 + 60 * 200);
    assert!(!host.hooked_can_see(watcher, target));
}
#[test]
fn hook_23_the_word_reaches_the_chain_with_its_type() {
    let (mut host, [watcher, _, target, _]) =
        attack_host(HOOK_WITHINPERCEPTION, &member(4, "rets=[2]"));
    host.world.get_mut(target).unwrap().raw.hex = Some(20100 + 60 * 200);
    let ran = run(&mut host, &[I(watcher as i32), I(target as i32), W(0x80dc)]);
    assert_eq!(ran.ints(), [1]);
    assert_eq!(saw(&host, HOOK_WITHINPERCEPTION).0[3], 1);
    // obj_can_hear_obj is any other caller: 2 acts as 1.
    run(&mut host, &[I(watcher as i32), I(target as i32), W(0x80f5)]);
    assert_eq!(saw(&host, HOOK_WITHINPERCEPTION).0[3], 0);
}
#[test]
fn hook_23_ai_target_selection_runs_type_3_and_can_refuse() {
    let danger = |rets: &str| {
        let (mut host, [critter, _, target, _]) =
            attack_host(HOOK_WITHINPERCEPTION, &member(4, rets));
        host.world.get_mut(target).unwrap().opponent = critter;
        host.world.ai.snapshot = vec![target];
        (
            host.danger(critter),
            saw(&host, HOOK_WITHINPERCEPTION),
            target,
        )
    };
    let (found, (args, runs), target) = danger("rets=[]");
    assert_eq!(found, Some(target));
    assert_eq!((args[3], runs), (perception::AI_TARGET, 1));
    assert_eq!(danger("rets=[0]").0, None);
}
#[test]
fn hook_23_type_2_never_runs() {
    // D-53: nothing here asks with type 2. Onlookers are "otherwise".
    let (mut host, [watcher, _, target, _]) =
        attack_host(HOOK_WITHINPERCEPTION, &member(4, "rets=[]"));
    host.world.ai.snapshot = vec![watcher];
    host.alert_onlookers(target);
    assert_eq!(saw(&host, HOOK_WITHINPERCEPTION).0[3], perception::OTHER);
}

pub(crate) fn h23_row(cells: &[&str]) {
    let (mut host, critter) = sfall_host();
    let first = item(&mut host, 0);
    host.world.add_item(critter, first, 1, true).unwrap();
    let names = [("C", critter), ("I", first)];
    let state = final_state(cells);
    match state {
        "wielded" | "not wielded" => {
            registered(&mut host, HOOK_INVENWIELD, &member(5, cells[3]));
            host.wield_object(critter, first).unwrap();
            assert_eq!(saw(&host, HOOK_INVENWIELD), (expected(cells, &names), 1));
            let held = host.world.wielded(critter, RIGHT);
            assert_eq!(held.is_some(), state == "wielded", "{cells:?}");
        }
        "unwielded and removed" => {
            let hand = BasicFlags::RIGHT_HAND.0;
            host.world.equip(critter, first, hand).unwrap();
            registered(&mut host, HOOK_INVENWIELD, &member(5, cells[3]));
            run(&mut host, &[I(critter as i32), I(first as i32), W(0x80d9)]);
            assert_eq!(saw(&host, HOOK_INVENWIELD), (expected(cells, &names), 1));
            assert!(host.world.get(critter).unwrap().inventory.is_empty());
        }
        "one chain, for the wield" => {
            let hand = BasicFlags::RIGHT_HAND.0;
            host.world.equip(critter, first, hand).unwrap();
            let second = item(&mut host, 2);
            host.world.add_item(critter, second, 1, true).unwrap();
            registered(&mut host, HOOK_INVENWIELD, &member(5, cells[3]));
            host.wield_object(critter, second).unwrap();
            let names = [("C", critter), ("J", second)];
            assert_eq!(saw(&host, HOOK_INVENWIELD), (expected(cells, &names), 1));
            assert_eq!(host.world.wielded(critter, RIGHT), Some(second));
        }
        other => panic!("{other}"),
    }
}
#[test]
fn hook_25_inven_wield_by_vector_h23() {
    rows("H23").iter().for_each(|row| h23_row(row));
}
#[test]
fn hook_25_a_removal_goes_on_whatever_the_chain_returns() {
    let (mut host, critter) = sfall_host();
    let held = item(&mut host, 0);
    host.world.add_item(critter, held, 1, true).unwrap();
    host.world
        .equip(critter, held, BasicFlags::RIGHT_HAND.0)
        .unwrap();
    registered(&mut host, HOOK_INVENWIELD, &member(5, "rets=[0]"));
    run(&mut host, &[I(critter as i32), I(held as i32), W(0x80d9)]);
    assert!(host.world.get(critter).unwrap().inventory.is_empty());
    // An item nobody holds runs no chain.
    let loose = item(&mut host, 0);
    host.world.add_item(critter, loose, 1, true).unwrap();
    run(&mut host, &[I(critter as i32), I(loose as i32), W(0x80d9)]);
    assert_eq!(saw(&host, HOOK_INVENWIELD).1, 1);
}

pub(crate) fn h24_row(cells: &[&str]) {
    let (mut host, _) = sfall_host();
    if no_chain(cells) {
        registered(&mut host, HOOK_ADJUSTFID, &member(2, "rets=[]"));
        host.sfall_figure(0x0100_0042);
        host.sfall_figure(0x0100_0042);
        assert_eq!(saw(&host, HOOK_ADJUSTFID).1, 1, "{cells:?}");
        return;
    }
    let values = cells[3].replace('\'', "").replace("0x01000099", "16777369");
    registered(&mut host, HOOK_ADJUSTFID, &member(2, &values));
    host.sfall_figure(0x0100_0042);
    assert_eq!(saw(&host, HOOK_ADJUSTFID), (expected(cells, &[]), 1));
    let shown = host.sfall_figure_shown(0x0100_0042);
    assert_eq!(format!("figure {shown:#010x}"), final_state(cells));
}
#[test]
fn hook_26_adjust_fid_by_vector_h24() {
    rows("H24").iter().for_each(|row| h24_row(row));
}
#[test]
fn hook_26_a_figure_with_no_art_leaves_the_engines() {
    let (mut host, _) = sfall_host();
    registered(&mut host, HOOK_ADJUSTFID, &member(2, "rets=[16777369]"));
    host.assets.art_policy = crate::test_support::ArtPolicy::Loads;
    host.sfall_figure(0x0100_0042);
    assert_eq!(host.sfall_figure_shown(0x0100_0042), 0x0100_0042);
}

struct Turns(Vec<ObjectRef>);
impl CombatAi for Turns {
    fn danger_target(
        &mut self,
        _world: &crate::WorldState,
        _critter: ObjectRef,
    ) -> Option<ObjectRef> {
        None
    }
    fn take_turn(
        &mut self,
        _world: &mut crate::WorldState,
        critter: ObjectRef,
    ) -> Result<(), crate::WorldError> {
        self.0.push(critter);
        Ok(())
    }
}
pub(crate) fn h25_row(cells: &[&str]) {
    let (mut host, [critter, ..]) = attack_host(HOOK_COMBATTURN, &member(3, cells[3]));
    let player = host.world.player;
    let args = expected(cells, &[("C", critter), ("P", player)]);
    host.world.combat = true;
    host.world.combat_actor = critter;
    match final_state(cells) {
        "turn skipped" => assert!(host.hook_turn_start(critter, false)),
        "combat ends" => {
            host.hook_turn_end(critter);
            assert!(host.sfall_ends_combat());
        }
        "combat has ended" => {
            host.world.combat_quit = args[0] == -1;
            host.end_combat().unwrap();
            assert!(!host.world.combat);
        }
        "the turn runs" => assert!(!host.hook_turn_start(player, true)),
        other => panic!("{other}"),
    }
    assert_eq!(saw(&host, HOOK_COMBATTURN), (args, 1), "{cells:?}");
}
#[test]
fn hook_27_combat_turn_by_vector_h25() {
    rows("H25").iter().for_each(|row| h25_row(row));
}
#[test]
fn hook_27_a_skipped_turn_takes_no_action() {
    // A member that skips only the attacker's turn: skipping every turn
    // would never reach the player's.
    let (_, [attacker, ..]) = crate::host_ai_tests::fixture_e("critter", 2);
    let event = [
        GET_ARG,
        I(1),
        W(EQUAL),
        GET_ARG,
        I(attacker as i32),
        W(EQUAL),
    ];
    let mut body = event.to_vec();
    body.extend([W(AND), SET_RETURN]);
    let (mut host, [attacker, _, defender, _]) = attack_host(HOOK_COMBATTURN, &body);
    host.start_combat(attacker, defender).unwrap();
    let mut ai = Turns(Vec::new());
    assert_eq!(
        host.advance_combat(&mut ai).unwrap(),
        CombatProgress::PlayerTurn
    );
    assert!(!ai.0.contains(&attacker), "{:?}", ai.0);
    assert!(ai.0.contains(&defender), "{:?}", ai.0);
}
#[test]
fn hook_27_minus_1_at_a_turns_end_ends_combat_at_the_boundary() {
    let (mut host, [attacker, _, defender, _]) =
        attack_host(HOOK_COMBATTURN, &member(3, "rets=[-1]"));
    host.start_combat(attacker, defender).unwrap();
    let mut ai = Turns(Vec::new());
    assert_eq!(host.advance_combat(&mut ai).unwrap(), CombatProgress::Ended);
    assert_eq!(ai.0.len(), 1, "one turn, then the boundary");
    assert!(!host.world.combat);
    let (args, _) = saw(&host, HOOK_COMBATTURN);
    assert_eq!(
        args[..2],
        [-2, ai.0[0] as i32],
        "it ran for the end of combat"
    );
}
#[test]
fn hook_27_a_resumed_players_turn_says_so_at_both_ends() {
    let (mut host, [attacker, _, defender, _]) =
        attack_host(HOOK_COMBATTURN, &member(3, "rets=[]"));
    host.start_combat(attacker, defender).unwrap();
    let player = host.world.player;
    assert!(!host.hook_turn_start(player, true));
    host.hook_turn_end(player);
    assert_eq!(saw(&host, HOOK_COMBATTURN).0, [0, player as i32, 1]);
    host.hook_turn_end(player);
    assert_eq!(saw(&host, HOOK_COMBATTURN).0, [0, player as i32, 0]);
}

pub(crate) fn h26_row(cells: &[&str]) {
    let (mut host, _) = sfall_host();
    registered(&mut host, HOOK_CARTRAVEL, &member(2, cells[3]));
    let (speed, fuel) = host.hook_car_travel(5, 100);
    assert_eq!(saw(&host, HOOK_CARTRAVEL), (expected(cells, &[]), 1));
    assert_eq!(format!("speed {speed}, fuel {fuel}"), final_state(cells));
}
#[test]
fn hook_28_car_travel_by_vector_h26() {
    rows("H26").iter().for_each(|row| h26_row(row));
}
#[test]
fn hook_28_limits_the_returns() {
    let (mut host, _) = sfall_host();
    registered(&mut host, HOOK_CARTRAVEL, &member(2, "rets=[-9, -9]"));
    assert_eq!(host.hook_car_travel(5, 100), (5, 0));
}
#[test]
fn hook_28_a_car_iteration_makes_the_chains_steps_and_uses_its_fuel() {
    let (mut host, _) = sfall_host();
    registered(&mut host, HOOK_CARTRAVEL, &member(2, "rets=[3, 50]"));
    let state = host.world.fallout2.as_deref_mut().unwrap();
    state.car = crate::fallout2::Car {
        has_car: true,
        area: 4,
        map: 20,
        fuel: 1000,
    };
    state.position = [10, 10];
    state.driving = true;
    host.fallout2_travel_to([45, 10]).unwrap();
    let steps = host.world.fallout2.as_deref().unwrap().car_speed();
    host.fallout2_travel_iteration().unwrap();
    assert_eq!(saw(&host, HOOK_CARTRAVEL), (vec![steps, 100], 1));
    let state = host.world.fallout2.as_deref().unwrap();
    assert_eq!((state.position[0], state.car.fuel), (13, 950));
}

pub(crate) fn h27_row(cells: &[&str]) {
    let (mut host, _) = sfall_host();
    registered(&mut host, HOOK_RESTTIMER, &member(4, cells[3]));
    let args = expected(cells, &[]);
    let mut session = host.begin_rest(RestRequest::Hours(6)).unwrap();
    if args[1] == 1 {
        let mut steps = 0;
        let outcome = loop {
            if let Some(outcome) = host.advance_rest(&mut session).unwrap() {
                break outcome;
            }
            assert!(!host.rest_step_over(&mut session, false));
            steps += 1;
        };
        assert_eq!(outcome, RestOutcome::Completed);
        let (seen, runs) = saw(&host, HOOK_RESTTIMER);
        assert_eq!(runs, steps + 1, "each step, and once at the end");
        assert_eq!(seen[1..], args[1..], "{cells:?}");
        assert_eq!(seen[0], host.world.clock.ticks as i32);
        return;
    }
    assert_eq!(host.advance_rest(&mut session).unwrap(), None);
    host.world.clock.set(args[0] as u64);
    let stops = host.rest_step_over(&mut session, args[1] == -1);
    assert_eq!(saw(&host, HOOK_RESTTIMER), (args, 1), "{cells:?}");
    assert_eq!(stops, final_state(cells) == "the rest stops", "{cells:?}");
    let finished = stops.then_some(RestOutcome::Cancelled);
    assert_eq!(session.outcome(), finished);
}
#[test]
fn hook_30_rest_timer_by_vector_h27() {
    rows("H27").iter().for_each(|row| h27_row(row));
}
#[test]
fn hook_30_a_rest_run_to_its_end_stops_where_the_chain_says() {
    let (mut host, _) = sfall_host();
    registered(&mut host, HOOK_RESTTIMER, &member(4, "rets=[1]"));
    let outcome = host.rest(RestRequest::Hours(6), &mut || false).unwrap();
    assert_eq!(outcome, RestOutcome::Cancelled);
    assert_eq!(saw(&host, HOOK_RESTTIMER).1, 1);
}
#[test]
fn hook_30_with_no_chain_esc_stops_a_rest() {
    let (mut host, _) = sfall_host();
    let outcome = host.rest(RestRequest::Hours(6), &mut || true).unwrap();
    assert_eq!(outcome, RestOutcome::Cancelled);
}

/// Scenery of its own PID, next to the player.
fn scenery(host: &mut WorldHost<TestAssets>) -> ObjectRef {
    let mut bytes = vault_fixtures::world::prototype(2, 5);
    vault_fixtures::world::word(&mut bytes, 0, 0x0200_0045);
    vault_fixtures::world::word(&mut bytes, 0x18, 0x0800);
    let prototype = std::sync::Arc::new(vault_formats::Prototype::parse(&bytes).unwrap());
    host.world.spawn(prototype, 20100, 0).unwrap()
}
pub(crate) fn h28_row(cells: &[&str]) {
    let (mut host, critter) = sfall_host();
    let object = scenery(&mut host);
    registered(&mut host, HOOK_USEANIMOBJ, &member(3, cells[3]));
    let chosen = host.hook_use_animation(critter, object, 12);
    let names = [("C", critter), ("O", object)];
    assert_eq!(saw(&host, HOOK_USEANIMOBJ), (expected(cells, &names), 1));
    let state = final_state(cells);
    match chosen {
        None => assert_eq!(state, "animation 12"),
        Some(Some(code)) => assert_eq!(state, format!("animation {code}")),
        Some(None) => assert_eq!(state, "no animation; the use still follows"),
    }
}
#[test]
fn hook_32_use_anim_obj_by_vector_h28() {
    rows("H28").iter().for_each(|row| h28_row(row));
}
#[test]
fn hook_32_a_use_of_scenery_plays_the_chains_animation_and_the_use_follows() {
    let use_labels = |rets: &str| {
        let (mut host, _) = sfall_host();
        let player = host.world.player;
        // Far enough that the walk holds the sequence open.
        host.world.get_mut(player).unwrap().raw.hex = Some(19500);
        let object = scenery(&mut host);
        registered(&mut host, HOOK_USEANIMOBJ, &member(3, rets));
        host.playback = Playback::RealTime;
        host.action(player, object, ObjectAction::Use).unwrap();
        let (seen, runs) = saw(&host, HOOK_USEANIMOBJ);
        assert_eq!((seen[0], seen[1], runs), (player as i32, object as i32, 1));
        let shown: Vec<String> = labels(&host.motions[0])
            .into_iter()
            .map(|(_, label)| label)
            .collect();
        shown
    };
    let engine = use_labels("rets=[]");
    assert!(engine.contains(&"animate 11".to_owned()), "{engine:?}");
    let chosen = use_labels("rets=[13]");
    assert!(chosen.contains(&"animate 13".to_owned()), "{chosen:?}");
    assert!(!chosen.contains(&"animate 11".to_owned()), "{chosen:?}");
    let skipped = use_labels("rets=[-1]");
    assert!(
        !skipped.iter().any(|label| label.starts_with("animate")),
        "{skipped:?}"
    );
    assert!(skipped.contains(&"use".to_owned()), "{skipped:?}");
}
#[test]
fn hook_32_an_item_used_on_scenery_is_not_a_use_of_it() {
    let (mut host, _) = sfall_host();
    let player = host.world.player;
    host.world.get_mut(player).unwrap().raw.hex = Some(19900);
    let object = scenery(&mut host);
    registered(&mut host, HOOK_USEANIMOBJ, &member(3, "rets=[]"));
    let tool = item(&mut host, 0);
    host.use_sequence(
        player,
        object,
        crate::motion::Call::UseOn {
            item: tool,
            target: object,
        },
    )
    .unwrap();
    assert_eq!(saw(&host, HOOK_USEANIMOBJ).1, 0);
}

pub(crate) fn h29_row(cells: &[&str]) {
    let (mut host, object) = sfall_host();
    host.world.get_mut(object).unwrap().description = b"Base".to_vec();
    let player = host.world.player;
    if no_chain(cells) {
        let overriding = [W(0x80b9)];
        let program = script(&[], &[("description_p_proc", &overriding)]);
        host.assets.programs.push(("obj.int".to_owned(), program));
        let file = host.assets.programs.len() as i32 - 1;
        let id = host.world.add_script(object, file, 1, 0).unwrap();
        host.dispatch(request(id, crate::ScriptEvent::Critter))
            .unwrap();
        registered(&mut host, HOOK_DESCRIPTIONOBJ, &member(1, "rets=[]"));
        host.world.effects.clear();
        host.action(player, object, ObjectAction::Description)
            .unwrap();
        assert_eq!(saw(&host, HOOK_DESCRIPTIONOBJ).1, 0, "{cells:?}");
        assert!(!messages(&host).contains(&b"Base".to_vec()));
        return;
    }
    let text: &[u8] = b"New";
    let (strings, body): (&[&[u8]], Vec<Op>) = if cells[2].contains("ret0=\"New\"") {
        let mut body = member(1, "rets=[]");
        body.extend([Op::S(text), SET_RETURN]);
        (&[text], body)
    } else {
        (
            &[],
            member(
                1,
                if cells[2].contains("ret0=7") {
                    "rets=[7]"
                } else {
                    "rets=[]"
                },
            ),
        )
    };
    let register = [I(HOOK_DESCRIPTIONOBJ), I(2), REGISTER_PROC];
    install(
        &mut host,
        "gl_0.int",
        script_with(strings, &register, &[("m", &body)]),
    );
    set_up(&mut host);
    diagnostics(&mut host);
    host.world.effects.clear();
    host.action(player, object, ObjectAction::Description)
        .unwrap();
    assert_eq!(
        saw(&host, HOOK_DESCRIPTIONOBJ).0,
        [object as i32],
        "{cells:?}"
    );
    let shown: &[u8] = match final_state(cells) {
        "the engine text" => b"Base",
        "text \"New\"" => b"New",
        other => panic!("{other}"),
    };
    assert_eq!(messages(&host)[0], shown, "{cells:?}");
    let refused = i32::from(cells[2].contains("ret0=7"));
    assert_eq!(diagnostics(&mut host).len() as i32, refused, "{cells:?}");
}
#[test]
fn hook_34_description_by_vector_h29() {
    rows("H29").iter().for_each(|row| h29_row(row));
}

pub(crate) fn h30_row(cells: &[&str]) {
    let (mut host, critter) = sfall_host();
    let player = host.world.player;
    host.world.get_mut(player).unwrap().raw.hex = Some(19900);
    host.world.get_mut(critter).unwrap().raw.hex = Some(19500);
    let object = item(&mut host, 0);
    let names = [("P", player), ("O", object), ("C", critter)];
    let rets = cells[3].replace("'C'", &(critter as i32).to_string());
    registered(&mut host, HOOK_USESKILLON, &member(3, &rets));
    let args = expected(cells, &names);
    host.world.combat = cells[2].contains("in combat");
    host.playback = Playback::RealTime;
    let accepted = host.use_skill_on(player, object, args[2]).unwrap();
    assert_eq!(saw(&host, HOOK_USESKILLON), (args, 1), "{cells:?}");
    let state = final_state(cells);
    assert_eq!(accepted, state != "request cancelled", "{cells:?}");
    let mover = host
        .motions
        .first()
        .map(|motion| motion.schedule.entries()[0].owner);
    match state {
        "P uses the skill" | "gating skips the combat refusal" => {
            assert_eq!(mover, Some(player), "{cells:?}");
        }
        "C uses the skill" => assert_eq!(mover, Some(critter), "{cells:?}"),
        "request cancelled" => assert_eq!(mover, None, "{cells:?}"),
        other => panic!("{other}"),
    }
}
#[test]
fn hook_35_use_skill_on_by_vector_h30() {
    rows("H30").iter().for_each(|row| h30_row(row));
}
#[test]
fn hook_35_the_combat_pass_is_for_the_player_or_a_controlled_critter() {
    let (mut host, critter) = sfall_host();
    let object = item(&mut host, 0);
    registered(&mut host, HOOK_USESKILLON, &member(3, "rets=[0, 1]"));
    host.world.combat = true;
    assert!(!host.use_skill_on(critter, object, 9).unwrap());
    let player = host.world.player;
    assert!(host.use_skill_on(player, object, 9).unwrap());
}
#[test]
fn hook_35_with_no_pass_combat_refuses() {
    let (mut host, _) = sfall_host();
    let object = item(&mut host, 0);
    registered(&mut host, HOOK_USESKILLON, &member(3, "rets=[]"));
    host.world.combat = true;
    let player = host.world.player;
    assert!(!host.use_skill_on(player, object, 9).unwrap());
}

pub(crate) fn h31_row(cells: &[&str]) {
    let (mut host, object) = sfall_host();
    registered(&mut host, HOOK_SETLIGHTING, &member(3, cells[3]));
    let args = expected(cells, &[("O", object)]);
    if args[0] == -1 {
        let (ambient, _) = host.hook_light(-1, args[1], args[2]);
        assert_eq!(format!("ambient {ambient}"), final_state(cells));
    } else {
        host.hooked_projectile_light(object, args[2], args[1]);
        let light = host.world.get(object).unwrap().raw.light;
        let state = format!("intensity {}, radius {}", light.intensity, light.radius);
        assert_eq!(state, final_state(cells));
    }
    assert_eq!(saw(&host, HOOK_SETLIGHTING), (args, 1), "{cells:?}");
}
#[test]
fn hook_38_set_lighting_by_vector_h31() {
    rows("H31").iter().for_each(|row| h31_row(row));
}
#[test]
fn hook_38_the_words_run_on_the_computed_intensity() {
    let (mut host, object) = sfall_host();
    registered(&mut host, HOOK_SETLIGHTING, &member(3, "rets=[20000, 3]"));
    run(&mut host, &[I(object as i32), I(50), I(4), W(0x8107)]);
    assert_eq!(saw(&host, HOOK_SETLIGHTING).0, [object as i32, 32818, 4]);
    let light = host.world.get(object).unwrap().raw.light;
    assert_eq!((light.intensity, light.radius), (20000, 3));
    run(&mut host, &[I(25), W(0x80e9)]);
    assert_eq!(saw(&host, HOOK_SETLIGHTING).0, [-1, 22528, -1]);
    assert_eq!(host.world.ambient, 20000);
}

pub(crate) fn h32_row(cells: &[&str]) {
    let (mut host, npc) = sfall_host();
    let player = host.world.player;
    registered(&mut host, HOOK_ADJUSTRADS, &member(2, cells[3]));
    if no_chain(cells) {
        assert!(!host.adjust_radiation(npc, 100));
        assert_eq!(saw(&host, HOOK_ADJUSTRADS).1, 0, "{cells:?}");
        return;
    }
    let args = expected(cells, &[("P", player)]);
    let before = host.world.player_sheet().unwrap().radiation;
    // The chain sees what resistance leaves.
    let resistance = host.world.player_stat(31);
    let exposure = (args[1]..)
        .find(|amount| amount - resistance * amount / 100 == args[1])
        .unwrap();
    assert!(host.adjust_radiation(player, exposure));
    assert_eq!(saw(&host, HOOK_ADJUSTRADS), (args, 1), "{cells:?}");
    let added = host.world.player_sheet().unwrap().radiation - before;
    assert_eq!(format!("{added} added"), final_state(cells));
}
#[test]
fn hook_45_adjust_rads_by_vector_h32() {
    rows("H32").iter().for_each(|row| h32_row(row));
}

pub(crate) fn h33_row(cells: &[&str]) {
    let (mut host, [critter, ..]) = attack_host(HOOK_CANUSEWEAPON, &member(4, cells[3]));
    let gun = host.world.wielded(critter, RIGHT).unwrap();
    let player = host.world.player;
    let args = expected(cells, &[("C", critter), ("I", gun), ("P", player)]);
    let result = host.weapon_chain(args[0] as ObjectRef, gun, args[2], args[3] == 1);
    assert_eq!(saw(&host, HOOK_CANUSEWEAPON), (args, 1), "{cells:?}");
    let shown = matches!(final_state(cells), "can use");
    assert_eq!(result, shown, "{cells:?}");
}
#[test]
fn hook_48_can_use_weapon_by_vector_h33() {
    rows("H33").iter().for_each(|row| h33_row(row));
}
#[test]
fn hook_48_a_refused_weapon_is_out_of_the_ai_search() {
    let (mut host, [critter, ..]) = attack_host(HOOK_CANUSEWEAPON, &member(4, "rets=[]"));
    let spare = |host: &mut WorldHost<TestAssets>, critter| {
        let gun = host.world.wielded(critter, RIGHT).unwrap();
        host.world.get_mut(gun).unwrap().raw.flags.0 &= !BasicFlags::RIGHT_HAND.0;
        gun
    };
    let gun = spare(&mut host, critter);
    assert_eq!(host.inventory_search(critter, false), Some(gun));
    let (args, _) = saw(&host, HOOK_CANUSEWEAPON);
    assert_eq!(args[..2], [critter as i32, gun as i32]);
    let (mut host, [critter, ..]) = attack_host(HOOK_CANUSEWEAPON, &member(4, "rets=[0]"));
    spare(&mut host, critter);
    assert_eq!(host.inventory_search(critter, false), None);
}
#[test]
fn hook_48_the_slot_preview_stores_its_result_for_presentation() {
    let (mut host, [critter, ..]) = attack_host(HOOK_CANUSEWEAPON, &member(4, "rets=[0]"));
    let gun = host.world.wielded(critter, RIGHT).unwrap();
    assert!(!host.sfall_slot_refused(gun, RIGHT), "nothing stored yet");
    assert_eq!(host.sfall_slot_usable(gun, RIGHT), Some(0));
    assert!(host.sfall_slot_refused(gun, RIGHT));
    assert_eq!(saw(&host, HOOK_CANUSEWEAPON).0[2], -1, "for the player");
    host.sfall_slot_usable(gun, RIGHT);
    assert_eq!(
        saw(&host, HOOK_CANUSEWEAPON).1,
        1,
        "the inputs are the same"
    );
    assert!(
        host.sfall_previewed(Preview::SlotUsable, &[0, 0, 0])
            .is_none()
    );
}

/// Count this chain's runs in global 1, and the runs of type `kind`, the
/// last of the hook's four arguments, in global 5.
fn counting(kind: i32) -> Vec<Op> {
    let mut body = vec![I(1), I(1), W(GLOBAL_INT), I(1), W(ADD), W(SET_GLOBAL)];
    body.extend([I(5), I(5), W(GLOBAL_INT)]);
    for _ in 0..3 {
        body.extend([GET_ARG, W(DISCARD)]);
    }
    body.extend([GET_ARG, I(kind), W(EQUAL), W(ADD), W(SET_GLOBAL)]);
    body
}

/// §7.3 test 18's second half (D-53), over every query there is.
#[test]
fn t18_ammo_cost_type_1_and_within_perception_type_2_never_run() {
    let (mut host, [attacker, _, defender, _]) = attack_host(HOOK_AMMOCOST, &counting(1));
    host.perform_attack(attacker, defender, RIGHT, 8, false)
        .unwrap()
        .unwrap();
    assert_eq!(global(&host, 1), 1, "the single shot");
    let (mut burst, target, _) =
        crate::combat_tests::spray_world(crate::combat::attack_mode::BURST);
    burst.sfall = Some(Default::default());
    burst.hook_host = Some(Default::default());
    burst.world.sfall = Some(Default::default());
    registered(&mut burst, HOOK_AMMOCOST, &counting(1));
    let player = burst.world.player;
    let attack = burst
        .world
        .attack_profile(player, target, RIGHT, 8, false)
        .unwrap();
    let mut rng = crate::ParkMiller::seeded(7).unwrap();
    let spray = burst
        .resolve_spray(player, target, &attack, &mut rng)
        .unwrap();
    let gun = burst.world.wielded(player, RIGHT).unwrap();
    burst.hook_ammo_spent(gun, &spray.main);
    assert_eq!(global(&burst, 1), 2, "the burst's two types");
    assert_eq!((global(&host, 5), global(&burst, 5)), (0, 0), "type 1");

    let (mut host, [watcher, _, target, _]) = attack_host(HOOK_WITHINPERCEPTION, &counting(2));
    run(&mut host, &[I(watcher as i32), I(target as i32), W(0x80dc)]);
    run(&mut host, &[I(watcher as i32), I(target as i32), W(0x80f5)]);
    host.world.get_mut(target).unwrap().opponent = watcher;
    host.world.ai.snapshot = vec![target];
    host.danger(watcher);
    host.world.ai.snapshot = vec![watcher];
    host.alert_onlookers(target);
    host.world.get_mut(target).unwrap().opponent = 0;
    host.world.get_mut(watcher).unwrap().opponent = target;
    host.start_combat(watcher, target).unwrap();
    host.request_end_combat(&mut crate::combat_state::PacketAi);
    assert!(global(&host, 1) >= 4, "each caller ran it");
    assert_eq!(global(&host, 5), 0, "type 2");
}

#[test]
fn with_the_mode_off_the_promoted_hooks_return_the_engine_value() {
    let world = crate::WorldState::new(crate::CharacterSheet::default(), 1).unwrap();
    let mut off = WorldHost::new(world, TestAssets::all_art());
    let player = off.world.player;
    assert_eq!(off.hook_death(0, 0, None, 1, 20), 20);
    assert_eq!(off.hook_car_travel(5, 100), (5, 100));
    assert_eq!(off.hook_radiation(player, 7), 7);
    assert_eq!(off.hook_light(-1, 9, -1), (9, -1));
    assert_eq!(off.hook_use_animation(player, player, 10), None);
    assert_eq!(off.hook_skill_on(player, player, 9), Some((player, false)));
    assert_eq!(off.hook_description(player), None);
    assert!(off.hook_wield(player, 0, 1, true));
    assert!(!off.hook_turn_start(player, false));
    assert!(!off.sfall_ends_combat());
    assert_eq!(off.sfall_figure(5), None);
    assert_eq!(off.sfall_figure_shown(5), 5);
    off.sfall_mouse(true, 0);
    assert_eq!(global_is_absent(&off), None);
}
fn global_is_absent(host: &WorldHost<TestAssets>) -> Option<i32> {
    host.world.sfall.as_ref().map(|_| global(host, 1))
}
