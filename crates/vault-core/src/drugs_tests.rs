//! SPEC-011 fixtures A-D, J, and K and the §3.6 use paths: drug stages and
//! their timeline, the random-range form, addiction, withdrawal, re-dosing,
//! the SPEC-009 healing hook, map exit, and prototype-shared bonuses.
use crate::{
    ObjectRef, Perk, RandomSource, Trait, WorldEffect, WorldError, WorldHost,
    ai::intent,
    critical::effect,
    drugs::{Sensitivity, Stage},
    fixture_items::{self as support, DRUG_A, DRUG_B, ItemAssets, messages},
    progression::ADDICTED_FLAG,
    queue::EventKind,
    test_support::hp,
};
use std::collections::VecDeque;

/// Replays explicit draws, checking each requested range.
struct Draws(VecDeque<(i32, i32, i32)>);
impl RandomSource for Draws {
    fn range(&mut self, first: i32, last: i32) -> Result<i32, WorldError> {
        let (low, high, value) = self.0.pop_front().expect("scripted draw available");
        assert_eq!((first, last), (low, high), "requested range");
        Ok(value)
    }
}

fn bonus(host: &WorldHost<ItemAssets>, critter: ObjectRef, stats: [usize; 2]) -> [i32; 2] {
    let sheet = host.world.get(critter).unwrap().character.as_ref().unwrap();
    stats.map(|stat| sheet.bonus[stat])
}
fn set_hp(host: &mut WorldHost<ItemAssets>, critter: ObjectRef, value: i32) {
    host.world.set_hit_points(critter, value);
}
/// Queued events as (due, owner, kind).
fn queued(host: &WorldHost<ItemAssets>) -> Vec<(u64, ObjectRef, EventKind)> {
    host.world
        .queue
        .events()
        .map(|event| (event.due, event.object, event.kind))
        .collect()
}
fn npc(host: &mut WorldHost<ItemAssets>, pid: u32, stats: &[(usize, i32)], body: i32) -> ObjectRef {
    let prototype = support::critter(pid, stats, body, &[]);
    host.assets.prototypes.insert(pid, prototype.clone());
    host.world.spawn(prototype, 20300, 0).unwrap()
}
const IN_AG: [usize; 2] = [4, 5];
const STAGE_1: EventKind = EventKind::Drug {
    stats: [4, 5, -1],
    modifiers: [-1, -1, 0],
};
const STAGE_2: EventKind = EventKind::Drug {
    stats: [4, 5, -1],
    modifiers: [-1, 0, 0],
};

/// The player takes one Fixture A dose with the given traits and perks.
fn dose_a(
    traits: [i32; 2],
    flower_child: bool,
    seed: impl Fn(&[i32]) -> bool,
) -> (WorldHost<ItemAssets>, u64) {
    let mut host = support::host();
    host.world.rng = support::seed_where(&[(1, 100), (1, 100)], seed);
    let player = host.world.player;
    let sheet = host.world.player_sheet_mut().unwrap();
    sheet.traits = traits;
    if flower_child {
        sheet.perks.insert(Perk::FlowerChild, 1);
    }
    let drug = support::carry(&mut host, player, DRUG_A, 1);
    host.world.effects.clear();
    let start = host.world.clock.ticks;
    assert_eq!(host.use_drug(player, drug).unwrap(), 1);
    assert!(
        host.world.get(drug).is_none(),
        "a successful use destroys the unit"
    );
    (host, start)
}

#[test]
fn spec011_fixture_a_drug_timeline_and_trait_variants() {
    let (mut host, start) = dose_a([-1, -1], false, |draws| draws[0] > 20);
    let player = host.world.player;
    assert_eq!(bonus(&host, player, IN_AG), [2, 1]);
    assert_eq!(messages(&host), ["gained 2 stat4", "gained 1 stat5"]);
    assert_eq!(
        queued(&host),
        [
            (start + 36000, player, STAGE_1),
            (start + 72000, player, STAGE_2)
        ],
        "both delayed stages, and no withdrawal after the draw above 20"
    );
    host.world.effects.clear();
    host.advance_time(36000).unwrap();
    assert_eq!(bonus(&host, player, IN_AG), [1, 0]);
    assert_eq!(messages(&host), ["lost 1 stat4", "lost 1 stat5"]);
    host.world.effects.clear();
    host.advance_time(36000).unwrap();
    assert_eq!(bonus(&host, player, IN_AG), [0, 0]);
    assert_eq!(messages(&host), ["lost 1 stat4"]);
    assert!(queued(&host).is_empty());

    // A2: Chem Resistant halves the delays and the chance (P = 10), so a
    // draw of 11-20 queues no withdrawal.
    let resistant = Trait::ChemResistant as i32;
    let reliant = Trait::ChemReliant as i32;
    let teens = |draws: &[i32]| (11..=20).contains(&draws[0]);
    let (host, start) = dose_a([resistant, -1], false, teens);
    let player = host.world.player;
    assert_eq!(
        queued(&host),
        [
            (start + 18000, player, STAGE_1),
            (start + 36000, player, STAGE_2)
        ]
    );
    // A3: Chem Reliant doubles first (40), then Chem Resistant halves (20):
    // the same draw now addicts.
    let (host, start) = dose_a([reliant, resistant], false, teens);
    assert_eq!(queued(&host).len(), 3);
    assert_eq!(
        queued(&host)[2],
        (
            start + 864000,
            host.world.player,
            EventKind::Withdrawal([1, DRUG_A as i32, 55])
        )
    );
    // A4: Flower Child and Chem Resistant give P = 5; delays are halved.
    let (host, start) = dose_a([resistant, -1], true, |draws| (6..=10).contains(&draws[0]));
    assert_eq!(
        queued(&host),
        [
            (start + 18000, host.world.player, STAGE_1),
            (start + 36000, host.world.player, STAGE_2)
        ]
    );
    let scaled = |chem_reliant, chem_resistant, flower_child| {
        Sensitivity {
            chem_reliant,
            chem_resistant,
            flower_child,
        }
        .addiction_chance(20)
    };
    assert_eq!(
        [
            scaled(false, false, false),
            scaled(false, true, false),
            scaled(true, true, false),
            scaled(false, true, true)
        ],
        [20, 10, 20, 5]
    );
}

#[test]
fn spec011_fixture_b_range_form_and_npc_healing() {
    let b = Stage {
        selectors: [-2, 35, -1],
        amounts: [10, 20, 0],
    };
    let mut draws = Draws(VecDeque::from([(10, 20, 14)]));
    assert_eq!(b.slots(&mut draws).unwrap(), [(35, 14)]);
    // B3: with slot 1 unused, the range draw moves to slot 2 (not A2 = 0).
    let b3 = Stage {
        selectors: [-2, -1, 35],
        ..b
    };
    let mut draws = Draws(VecDeque::from([(10, 20, 14)]));
    assert_eq!(b3.slots(&mut draws).unwrap(), [(35, 14)]);
    assert!(draws.0.is_empty());
    // No processed slot, no draw; the plain form never draws.
    let mut none = Draws(VecDeque::new());
    let unused = Stage {
        selectors: [-2, -1, -1],
        ..b
    };
    assert!(unused.slots(&mut none).unwrap().is_empty());
    let plain = Stage {
        selectors: [4, 5, -1],
        amounts: [2, 1, 0],
    };
    assert_eq!(plain.slots(&mut none).unwrap(), [(4, 2), (5, 1)]);
    // A delayed stage with all-zero amounts is not scheduled; the range
    // form's low bound counts.
    assert!(
        !Stage {
            amounts: [0; 3],
            ..b
        }
        .scheduled()
    );
    assert!(
        Stage {
            amounts: [1, 0, 0],
            ..b
        }
        .scheduled()
    );

    for (start, expected_cap) in [(15, None), (39, Some(41))] {
        let mut host = support::host();
        let critter = npc(&mut host, 0x0100_0101, &[(7, 41)], 0);
        set_hp(&mut host, critter, start);
        host.world.critter_state_mut(critter).unwrap().maneuvers |= intent::FLEE;
        let drug = support::spawn(&mut host, DRUG_B);
        let mut expected = host.world.rng.clone();
        let range = expected.range(10, 20).unwrap();
        expected.range(1, 100).unwrap();
        host.world.effects.clear();
        assert_eq!(host.consume_drug(critter, drug).unwrap(), 1);
        let maneuvers = host.world.critter_state(critter).unwrap().maneuvers;
        assert_eq!(maneuvers & intent::FLEE, 0, "the fleeing bit is cleared");
        assert_eq!(hp(&host, critter), expected_cap.unwrap_or(start + range));
        assert_eq!(
            host.world.rng.state(),
            expected.state(),
            "two draws: the range, then the addiction roll even at 0%"
        );
        assert!(messages(&host).is_empty(), "NPCs see no stage text");
        assert!(queued(&host).is_empty(), "no delayed stage, no withdrawal");
    }
}

#[test]
fn spec011_fixture_c_addiction_and_withdrawal_timeline() {
    let (mut host, start) = dose_a([-1, -1], false, |draws| draws[0] <= 20);
    let player = host.world.player;
    let onset = start + 864000;
    assert!(queued(&host).contains(&(
        onset,
        player,
        EventKind::Withdrawal([1, DRUG_A as i32, 55])
    )));
    assert_ne!(host.world.player_sheet().unwrap().flags & ADDICTED_FLAG, 0);
    assert!(
        host.world
            .gvars
            .as_ref()
            .unwrap()
            .iter()
            .all(|value| *value == 0),
        "PID 0x400 maps to no campaign variable"
    );
    // Both stages wear off before the onset; neither stops a pass.
    host.advance_time(863999).unwrap();
    assert_eq!(bonus(&host, player, IN_AG), [0, 0]);
    host.world.clock.set(onset);
    host.world.effects.clear();
    assert_eq!(host.process_queue().unwrap(), 1, "the onset stops the pass");
    assert_eq!(bonus(&host, player, IN_AG), [-3, -2]);
    assert_eq!(messages(&host), ["about 55"]);
    let end = onset + 6048000;
    assert_eq!(
        queued(&host),
        [(end, player, EventKind::Withdrawal([0, DRUG_A as i32, 55]))]
    );
    host.world.clock.set(end);
    host.world.effects.clear();
    assert_eq!(host.process_queue().unwrap(), 1, "the end stops the pass");
    assert_eq!(bonus(&host, player, IN_AG), [0, 0]);
    assert_eq!(messages(&host), ["withdrawal ends"]);
    assert_eq!(host.world.player_sheet().unwrap().flags & ADDICTED_FLAG, 0);

    // C2: Chem Reliant plus Flower Child: 10080 / 2 / 2 minutes.
    let reliant = Trait::ChemReliant as i32;
    let (mut host, start) = dose_a([reliant, -1], true, |draws| draws[0] <= 20);
    host.world.clock.set(start + 864000);
    while host.process_queue().unwrap() == 0 && host.world.queue.next_due().is_some() {}
    let (due, ..) = *queued(&host).last().unwrap();
    assert_eq!(due, start + 864000 + 1512000);
    assert_eq!(
        Sensitivity {
            chem_reliant: true,
            flower_child: true,
            ..Default::default()
        }
        .withdrawal_ticks(),
        1512000
    );
}

#[test]
fn spec011_fixture_d_redose_during_withdrawal() {
    let both = |draws: &[i32]| draws[0] <= 20 && draws[1] <= 20;
    let (mut host, start) = dose_a([-1, -1], false, both);
    let player = host.world.player;
    host.world.clock.set(start + 864000);
    while host.process_queue().unwrap() == 0 {}
    assert_eq!(
        bonus(&host, player, IN_AG),
        [-3, -2],
        "withdrawal is active"
    );
    // The second dose at T + 1000000, with the end event pending.
    let now = start + 1_000_000;
    host.world.clock.set(now);
    let drug = support::carry(&mut host, player, DRUG_A, 1);
    host.world.effects.clear();
    assert_eq!(host.use_drug(player, drug).unwrap(), 1);
    assert_eq!(
        messages(&host),
        ["withdrawal ends", "gained 2 stat4", "gained 1 stat5"],
        "the end event's effect is removed before the immediate stage"
    );
    assert_eq!(bonus(&host, player, IN_AG), [2, 1]);
    let onsets: Vec<_> = queued(&host)
        .into_iter()
        .filter(|(_, _, kind)| matches!(kind, EventKind::Withdrawal(_)))
        .collect();
    let onset = (
        now + 864000,
        player,
        EventKind::Withdrawal([1, DRUG_A as i32, 55]),
    );
    assert_eq!(
        onsets,
        [onset, onset],
        "the re-dose onset and a second roll"
    );
    // Both onsets fire; each stops its own pass and schedules an end.
    host.world.clock.set(now + 864000);
    assert_eq!(host.process_queue().unwrap(), 1);
    assert_eq!(host.process_queue().unwrap(), 1);
    assert_eq!(
        bonus(&host, player, IN_AG),
        [-6, -4],
        "perk 55 applied twice"
    );
    let ends = queued(&host)
        .into_iter()
        .filter(|(_, _, kind)| *kind == EventKind::Withdrawal([0, DRUG_A as i32, 55]))
        .count();
    assert_eq!(ends, 2);
}

#[test]
fn spec011_fixture_j_spec009_healing_hook_trace() {
    let mut host = support::host();
    // Effective max HP 41 (B = 20), HP 15, AP 5; Fixture B as the stimpak.
    let critter = npc(&mut host, 0x0100_0102, &[(7, 41)], 0);
    set_hp(&mut host, critter, 15);
    host.world.critter_state_mut(critter).unwrap().ap = 5;
    host.assets.prototypes.insert(
        crate::ai::STIMPAK,
        support::renumbered(DRUG_B, crate::ai::STIMPAK),
    );
    let stimpak = support::carry(&mut host, critter, crate::ai::STIMPAK, 2);
    let mut expected = host.world.rng.clone();
    let range = expected.range(10, 20).unwrap();
    expected.range(1, 100).unwrap();
    host.world.effects.clear();
    host.ai_heal(critter).unwrap();
    let state = host.world.critter_state(critter).unwrap();
    assert_eq!((state.hp, state.ap), (15 + range, 3), "one dose, 2 AP");
    assert_eq!(
        host.world.rng.state(),
        expected.state(),
        "range then addiction"
    );
    let inventory = &host.world.get(critter).unwrap().inventory;
    assert_eq!(inventory.len(), 1);
    assert_eq!(inventory[0].1, 1, "the used unit is destroyed");
    assert_ne!(inventory[0].0, stimpak, "a copy represents the rest");
    let name = String::from_utf8(host.world.get(critter).unwrap().name.clone()).unwrap();
    assert_eq!(messages(&host), [format!("{name} uses Object 00000028.")]);
    assert!(
        host.world
            .effects
            .iter()
            .any(|effect| matches!(effect, WorldEffect::CritterAnimation { animation: 11, .. }))
    );

    // Failure: a robotic or dead consumer gives -1 with no draw or change.
    let robot = npc(&mut host, 0x0100_0103, &[(7, 41)], 2);
    set_hp(&mut host, robot, 15);
    let unit = support::spawn(&mut host, crate::ai::STIMPAK);
    let before = host.world.rng.clone();
    assert_eq!(host.consume_drug(robot, unit).unwrap(), -1);
    assert_eq!(hp(&host, robot), 15);
    host.world.critter_state_mut(critter).unwrap().results.0 |= effect::DEAD;
    assert_eq!(host.consume_drug(critter, unit).unwrap(), -1);
    assert_eq!(host.world.rng, before);
}

#[test]
fn spec011_fixture_k_map_exit_handlers() {
    let mut host = support::host();
    let player = host.world.player;
    let stranger = npc(&mut host, 0x0100_0104, &[], 0);
    let companion = npc(&mut host, 0x0100_0105, &[], 0);
    host.world.party.insert(companion);
    let stage = EventKind::Drug {
        stats: [5, -1, -1],
        modifiers: [-1, 0, 0],
    };
    let end = EventKind::Withdrawal([0, 87, 56]);
    for owner in [stranger, companion, player] {
        for kind in [stage, end] {
            host.world
                .queue
                .insert(host.world.clock.ticks, 6000, owner, kind)
                .unwrap();
        }
    }
    host.world
        .queue
        .insert(
            host.world.clock.ticks,
            6000,
            stranger,
            EventKind::Withdrawal([1, 87, 56]),
        )
        .unwrap();
    let leaving = host.world.map_exit_cleanup(|_| true);
    assert_eq!(leaving.len(), 3);
    host.world.effects.clear();
    host.leave_map_events(leaving).unwrap();
    // The stage applies at once; the end removes perk 56 (IN +2).
    assert_eq!(bonus(&host, stranger, IN_AG), [2, -1]);
    for owner in [companion, player] {
        assert_eq!(bonus(&host, owner, IN_AG), [0, 0]);
    }
    let owners: Vec<_> = queued(&host)
        .into_iter()
        .map(|(_, owner, _)| owner)
        .collect();
    assert_eq!(owners.len(), 4);
    assert!(!owners.contains(&stranger));
    assert!(messages(&host).is_empty());
}

#[test]
fn spec011_use_paths_destroy_rules_and_messages() {
    let mut host = support::host();
    let player = host.world.player;
    // Inventory use by a robotic consumer: -1, and (safe rule) the unit stays.
    let robot = npc(&mut host, 0x0100_0106, &[], 2);
    let kept = support::carry(&mut host, robot, DRUG_B, 1);
    assert_eq!(host.use_drug(robot, kept).unwrap(), -1);
    assert_eq!(host.world.get(kept).unwrap().owner, Some(robot));
    // Use on a non-critter: "no use", -1, kept.
    let scenery = support::spawn(&mut host, support::CONTAINER);
    let drug = support::carry(&mut host, player, DRUG_B, 2);
    host.world.effects.clear();
    assert_eq!(host.use_drug_on(player, drug, scenery).unwrap(), -1);
    assert_eq!(messages(&host), ["no use"]);
    // Use on a dead critter: one roll(0, 3) picks the message; kept.
    let corpse = npc(&mut host, 0x0100_0107, &[], 0);
    host.world.critter_state_mut(corpse).unwrap().results.0 |= effect::DEAD;
    let mut expected = host.world.rng.clone();
    let pick = expected.range(0, 3).unwrap();
    host.world.effects.clear();
    assert_eq!(host.use_drug_on(player, drug, corpse).unwrap(), -1);
    assert_eq!(messages(&host), [format!("dead {pick}")]);
    assert_eq!(host.world.rng, expected);
    assert_eq!(host.world.get(player).unwrap().inventory[0].1, 2);
    // Use on a living critter: the names first, then the consumption; one
    // unit is destroyed on exactly 1.
    let patient = npc(&mut host, 0x0100_0108, &[(7, 41)], 0);
    set_hp(&mut host, patient, 5);
    host.world.effects.clear();
    assert_eq!(host.use_drug_on(player, drug, patient).unwrap(), 1);
    let name = String::from_utf8(host.world.get(patient).unwrap().name.clone()).unwrap();
    assert_eq!(messages(&host), [format!("use Object 00000401 on {name}")]);
    assert!(hp(&host, patient) >= 15);
    assert_eq!(host.world.get(player).unwrap().inventory[0].1, 1);
}

#[test]
fn spec011_npc_bonuses_live_on_the_prototype() {
    let mut host = support::host();
    let first = npc(&mut host, 0x0100_0110, &[], 0);
    let second = npc(&mut host, 0x0100_0110, &[], 0);
    let other = npc(&mut host, 0x0100_0111, &[], 0);
    let drug = support::spawn(&mut host, DRUG_A);
    host.world.effects.clear();
    assert_eq!(host.consume_drug(first, drug).unwrap(), 1);
    assert!(messages(&host).is_empty());
    assert_eq!(bonus(&host, first, IN_AG), [2, 1]);
    assert_eq!(
        bonus(&host, second, IN_AG),
        [2, 1],
        "shared prototype array"
    );
    assert_eq!(bonus(&host, other, IN_AG), [0, 0]);
    let later = npc(&mut host, 0x0100_0110, &[], 0);
    assert_eq!(bonus(&host, later, IN_AG), [2, 1], "a later instance too");
    // The AG bonus recomputes derived stats on every instance.
    let armor_class = |host: &WorldHost<ItemAssets>, critter| {
        host.world
            .get(critter)
            .unwrap()
            .character
            .as_ref()
            .unwrap()
            .base[9]
    };
    assert_eq!(
        armor_class(&host, second),
        6,
        "AC base follows effective AG"
    );

    // Poison and radiation slots act on the player only, resistance first.
    let toxin = support::drug(
        0x420,
        [36, 37, -1, 10, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, -1, 0],
    );
    host.assets.prototypes.insert(0x420, toxin);
    let dose = support::spawn(&mut host, 0x420);
    host.consume_drug(other, dose).unwrap();
    let sheet = host.world.get(other).unwrap().character.as_ref().unwrap();
    assert_eq!((sheet.poison, sheet.radiation), (0, 0));
    let player = host.world.player;
    let resist = [32, 31].map(|stat| host.world.player_stat(stat));
    let dose = support::spawn(&mut host, 0x420);
    host.world.effects.clear();
    host.consume_drug(player, dose).unwrap();
    let sheet = host.world.player_sheet().unwrap();
    assert_eq!(
        (sheet.poison, sheet.radiation),
        (10 - 10 * resist[0] / 100, 10 - 10 * resist[1] / 100)
    );
    assert_eq!(messages(&host).len(), 2);

    // An immediate stage that changes nothing tells the player so.
    let inert = support::drug(
        0x421,
        [4, -1, -1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, -1, 0],
    );
    host.assets.prototypes.insert(0x421, inert);
    let dose = support::spawn(&mut host, 0x421);
    host.world.effects.clear();
    host.consume_drug(player, dose).unwrap();
    assert_eq!(messages(&host), ["nothing"]);

    // An overdose kills through the kill path; the result is still 1 and
    // no overdose text is shown.
    let poison = support::drug(
        0x422,
        [35, -1, -1, -100, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, -1, 0],
    );
    host.assets.prototypes.insert(0x422, poison);
    let dose = support::spawn(&mut host, 0x422);
    host.world.effects.clear();
    assert_eq!(host.consume_drug(other, dose).unwrap(), 1);
    assert!(host.world.is_dead(other));
    assert!(messages(&host).is_empty());
}

#[test]
fn spec011_queue_handlers_stop_only_for_the_player() {
    let mut host = support::host();
    let player = host.world.player;
    let critter = npc(&mut host, 0x0100_0112, &[], 0);
    let wear_off = EventKind::Drug {
        stats: [4, -1, -1],
        modifiers: [-1, 0, 0],
    };
    let now = host.world.clock.ticks;
    host.world.queue.insert(now, 10, player, wear_off).unwrap();
    host.world
        .queue
        .insert(now, 10, critter, EventKind::Withdrawal([1, 87, 56]))
        .unwrap();
    host.world.clock.set(now + 10);
    assert_eq!(host.process_queue().unwrap(), 0, "neither stops the pass");
    assert_eq!(bonus(&host, critter, IN_AG), [-2, 0]);
    assert_eq!(queued(&host).len(), 1, "the NPC's end event");
    // A stage that leaves the player dead stops the pass.
    let fatal = EventKind::Drug {
        stats: [35, -1, -1],
        modifiers: [-1000, 0, 0],
    };
    host.world.queue.insert(now + 10, 5, player, fatal).unwrap();
    host.world.clock.set(now + 15);
    assert_eq!(host.process_queue().unwrap(), 1);
    assert!(host.world.is_dead(player));
}
