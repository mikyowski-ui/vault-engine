//! SPEC-013 §4 fixtures and the rules behind them: steal checks, First Aid
//! and Doctor with usage slots and kits, explosives, poison and radiation,
//! elevators and transitions, the move-items counter, sneak, skill gating
//! and defaults, and the loot window.
use crate::{
    LootRequest, ObjectRef, RandomSource, Transition, WorldEffect, WorldError, WorldHost,
    fixture_items::{self as items, ItemAssets},
    loot::LootMove,
    queue::EventKind,
    skills::{self, Counter, SetTimer, StealExperience, StealInputs},
};
use std::{collections::VecDeque, sync::Arc};
use vault_formats::{
    HexGrid, MsgCatalog,
    map::{ObjectDetail, ObjectState},
    pro::BasicFlags,
};

/// Scripted outcomes for pure checks; each must lie in its range.
struct Draws(VecDeque<i32>);
impl RandomSource for Draws {
    fn range(&mut self, first: i32, last: i32) -> Result<i32, WorldError> {
        let value = self.0.pop_front().expect("an outcome for every draw");
        assert!(
            (first..=last).contains(&value),
            "{value} outside {first}..={last}"
        );
        Ok(value)
    }
}
fn draws(values: &[i32]) -> Draws {
    Draws(values.iter().copied().collect())
}

const CRITTER_PID: u32 = 0x0100_0100;
const ITEM_PID: u32 = 0x600;
const KIT_PID: u32 = 47;
const BAG_PID: u32 = 91;

/// The SPEC-011 item host with SPEC-013 placeholder catalogs, past the
/// first day so critical-upgrade draws are made.
fn host() -> WorldHost<ItemAssets> {
    let mut host = items::host();
    let mut skill = String::from(
        "{500}{}{heal %d}\n{501}{}{you look healthy}\n{502}{}{%s looks healthy}\n{503}{}{no healing}\n{505}{}{xp %d}\n\
         {512}{}{dead 0}\n{513}{}{dead 1}\n{514}{}{dead 2}\n{520}{}{heal your %s}\n{521}{}{heal the %s}\n\
         {525}{}{fail your %s}\n{526}{}{fail the %s}\n{551}{}{no traps}\n{552}{}{learn nothing}\n{553}{}{cannot repair}\n\
         {570}{}{caught taking %s}\n{571}{}{take %s}\n{572}{}{caught planting %s}\n{573}{}{plant %s}\n\
         {590}{}{taxed 0}\n{591}{}{taxed 1}\n{592}{}{taxed 2}\n",
    );
    for (index, part) in ["eye", "left arm", "right arm", "right leg", "left leg"]
        .iter()
        .enumerate()
    {
        skill.push_str(&format!("{{{}}}{{}}{{{part}}}\n", 530 + index));
    }
    let catalogs = [
        ("skill.msg", skill),
        (
            "proto.msg",
            "{485}{}{search %s}\n{486}{}{close %s}\n{487}{}{locked}\n{582}{}{no use}\n{589}{}{timer set}\n\
             {590}{}{ticking}\n{700}{}{no ap}\n{900}{}{bag out}\n{901}{}{kit out}\n{902}{}{not in combat}\n{903}{}{no barter}\n\
             {800}{}{read}\n{801}{}{nothing new}\n{802}{}{science}\n{803}{}{repair}\n{804}{}{heal}\n{805}{}{guns}\n{806}{}{outdoors}\n"
                .into(),
        ),
        (
            "misc.msg",
            "{1000}{}{rad 1}\n{1001}{}{rad 2}\n{1002}{}{rad 3}\n{1003}{}{rad 4}\n{1004}{}{rad 5}\n{1005}{}{rad 6}\n\
             {1006}{}{rad death}\n{1007}{}{big dose}\n{1008}{}{clicking}\n{1009}{}{clicking wildly}\n{2000}{}{cannot get there}\n\
             {2001}{}{jammed}\n{3000}{}{poisoned}\n{3001}{}{poison damage}\n{4000}{}{premature}\n"
                .into(),
        ),
        (
            "inventry.msg",
            "{25}{}{too heavy}\n{26}{}{no space}\n{29}{}{steal xp %d}\n".into(),
        ),
    ];
    for (file, text) in catalogs {
        host.assets
            .catalogs
            .insert(file, Arc::new(MsgCatalog::parse(text.as_bytes()).unwrap()));
    }
    for pid in [
        KIT_PID,
        BAG_PID,
        ITEM_PID,
        skills::DYNAMITE,
        skills::DYNAMITE_ARMED,
    ] {
        host.assets
            .prototypes
            .insert(pid, items::misc(pid, 0, 1, 10));
    }
    host.world.clock.ticks = 900_000;
    host.world.effects.clear();
    host
}
fn messages(host: &mut WorldHost<ItemAssets>) -> Vec<String> {
    let messages = items::messages(host);
    host.world.effects.clear();
    messages
}
fn adjacent(direction: u8) -> i32 {
    HexGrid::step(20100, direction, 1).unwrap()
}
/// A critter (SPECIAL 5, max HP 30) at `hex` with stat overrides.
fn critter(host: &mut WorldHost<ItemAssets>, hex: i32, stats: &[(usize, i32)]) -> ObjectRef {
    let critter = host
        .world
        .spawn(items::critter(CRITTER_PID, stats, 0, &[]), hex, 0)
        .unwrap();
    set_hp(host, critter, 30);
    critter
}
fn set_hp(host: &mut WorldHost<ItemAssets>, critter: ObjectRef, hp: i32) {
    let object = host.world.get_mut(critter).unwrap();
    object.character.as_mut().unwrap().current_hp = hp;
    if let ObjectState::Critter(state) = &mut object.raw.state {
        state.hp = hp;
    }
}
fn hp(host: &WorldHost<ItemAssets>, critter: ObjectRef) -> i32 {
    host.world.hit_points(critter).unwrap()
}
fn skill(host: &WorldHost<ItemAssets>, index: usize) -> i32 {
    let player = host.world.player;
    host.world
        .player_sheet()
        .unwrap()
        .skill(index, host.world.context(player))
}
fn critical(host: &WorldHost<ItemAssets>) -> i32 {
    host.world.player_stat(15)
}
fn assert_script_used(host: &WorldHost<ItemAssets>) {
    assert!(
        host.world.rng.unused_script().is_empty(),
        "unused {:?}",
        host.world.rng.unused_script()
    );
}

#[test]
fn spec013_fixture_a_steal_checks() {
    let inputs = |thief: u8, victim: u8| StealInputs {
        attempts: 1,
        item_size: 2,
        thief_steal: 60,
        thief_critical: 5,
        pickpocket: false,
        victim_critter: true,
        in_front: skills::in_front(thief, victim),
        victim_prone: false,
        party_member: false,
        victim_steal: Some(40),
    };
    let check = |inputs: StealInputs, outcomes: &[i32]| {
        let mut rng = draws(outcomes);
        let result = skills::steal_check(&inputs, 864_000, &mut rng).unwrap();
        assert!(rng.0.is_empty(), "unused outcomes");
        result
    };
    // A1: in front, M = -33, S = 27; the catch at 73 succeeds: caught.
    assert_eq!(skills::steal_modifier(&inputs(1, 4)), -33);
    let a1 = check(inputs(1, 4), &[20, 50, 70, 90]);
    assert!(a1.caught);
    // A2: behind, M = -8, S = 52; the catch at 48 fails: not caught.
    let a2 = check(inputs(4, 4), &[20, 50, 70, 30]);
    assert_eq!((a2.roll, a2.caught), (2, false));
    // A3: the upgrade draw makes a critical success: no catch draws.
    let a3 = check(inputs(4, 4), &[2, 5]);
    assert_eq!((a3.roll, a3.caught), (3, false));
    // A4: a party member: critical success with no draws.
    let a4 = check(
        StealInputs {
            party_member: true,
            ..inputs(1, 4)
        },
        &[],
    );
    assert_eq!((a4.roll, a4.caught), (3, false));
    // A critical failure is caught with no catch draws.
    assert!(check(inputs(4, 4), &[99, 1]).caught);
    // Four successes give 100, capped by 300 - Steal.
    let mut experience = StealExperience::default();
    for _ in 0..4 {
        experience.success();
    }
    assert_eq!(experience.pending, 100);
    assert_eq!(experience.award(60), Some(100));
    assert_eq!(experience.award(250), Some(50));
    assert_eq!(experience.award(300), None, "safe rule: no award below 1");
    // Pickpocket drops the size and facing penalties, not the prone bonus.
    let mut pick = inputs(1, 4);
    pick.pickpocket = true;
    pick.victim_prone = true;
    assert_eq!(skills::steal_modifier(&pick), 20);
}

#[test]
fn spec013_fixture_b_first_aid_through_a_kit() {
    let mut host = host();
    let player = host.world.player;
    let target = critter(&mut host, adjacent(1), &[]);
    set_hp(&mut host, target, 10);
    let kit = items::carry(&mut host, player, KIT_PID, 1);
    let chance = skill(&host, 6) + critical(&host) + 20;
    assert!(chance >= 1 && chance - 1 < 990, "{chance}");
    let before = (
        host.world.clock.ticks,
        host.world.player_sheet().unwrap().experience,
    );
    host.world.rng.script(&[1, 100, 4, 7]);
    host.use_kit(player, kit, target).unwrap();
    assert_script_used(&host);
    assert_eq!(hp(&host, target), 14);
    assert_eq!(messages(&mut host), ["heal 4", "xp 25"]);
    assert_eq!(
        host.world.persistence.skill_uses[6],
        [before.0 as i32, 0, 0]
    );
    assert_eq!(host.world.clock.ticks, before.0 + 18_000);
    assert_eq!(host.world.player_sheet().unwrap().experience, before.1 + 25);
    assert!(host.world.get(kit).is_some(), "draw 7 keeps the kit");
    // A kit draw of 1 uses it up.
    set_hp(&mut host, target, 10);
    host.world.rng.script(&[1, 100, 2, 1]);
    host.use_kit(player, kit, target).unwrap();
    assert!(messages(&mut host).contains(&"kit out".to_string()));
    assert!(host.world.get(kit).is_none());
    // A healthy target: no roll, 502 with its name, no experience; time
    // still passes, and the kit draw follows.
    let kit = items::carry(&mut host, player, KIT_PID, 1);
    set_hp(&mut host, target, 30);
    host.world.get_mut(target).unwrap().name = b"Ian".to_vec();
    let ticks = host.world.clock.ticks;
    host.world.rng.script(&[5]);
    host.use_kit(player, kit, target).unwrap();
    assert_script_used(&host);
    assert_eq!(messages(&mut host), ["Ian looks healthy"]);
    assert_eq!(host.world.clock.ticks, ticks + 18_000);
    // A non-critter target (safe rule): 582, no draw, nothing consumed.
    let other = items::spawn(&mut host, ITEM_PID);
    host.use_kit(player, kit, other).unwrap();
    assert_eq!(messages(&mut host), ["no use"]);
    // In combat: 902.
    host.world.combat = true;
    host.use_kit(player, kit, target).unwrap();
    assert_eq!(messages(&mut host), ["not in combat"]);
}

#[test]
fn spec013_fixture_b_doctor_limbs_hp_roll_and_time() {
    let mut host = host();
    let player = host.world.player;
    let target = critter(&mut host, adjacent(2), &[]);
    if let Some(ObjectState::Critter(state)) = host.world.get_mut(target).map(|o| &mut o.raw.state)
    {
        state.results.0 |= 0x40 | 0x04;
    }
    let bag = items::carry(&mut host, player, BAG_PID, 1);
    let doctor = skill(&host, 7);
    assert!(doctor + critical(&host) + 20 < 100 && doctor >= 1);
    let (ticks, experience) = (
        host.world.clock.ticks,
        host.world.player_sheet().unwrap().experience,
    );
    // Blind succeeds, the left leg fails, the HP roll succeeds; the bag stays.
    host.world.rng.script(&[1, 100, 100, 100, 1, 100, 6, 3]);
    host.use_kit(player, bag, target).unwrap();
    assert_script_used(&host);
    assert_eq!(
        messages(&mut host),
        [
            "heal the eye",
            "xp 50",
            "fail the left leg",
            "heal 0",
            "xp 50"
        ]
    );
    let results = host.world.critter_state(target).unwrap().results.0;
    assert_eq!(
        results & 0x7c,
        0x04,
        "blindness cleared, left leg still crippled"
    );
    assert_eq!(
        host.world.clock.ticks,
        ticks + 108_000,
        "3 attempts of an hour"
    );
    // Safe rule: the failed limb awards nothing (100 rather than 150).
    assert_eq!(
        host.world.player_sheet().unwrap().experience,
        experience + 100
    );
    // One slot, recorded by the limb; the HP roll records none.
    assert_eq!(host.world.persistence.skill_uses[7], [ticks as i32, 0, 0]);
    assert!(host.world.get(bag).is_some());
}

#[test]
fn spec013_fixture_b_usage_slots_and_the_limit() {
    let mut slots = [100_000, 200_000, 300_000];
    assert_eq!(skills::usage_slot(&slots, 999_999), None, "H = 24");
    assert_eq!(skills::usage_slot(&slots, 1_000_000), Some(2), "H = 25");
    assert!(!skills::record_use(&mut slots, 999_999));
    assert!(skills::record_use(&mut slots, 1_000_000));
    assert_eq!(slots, [200_000, 300_000, 1_000_000]);
    let mut empty = [0, 5, 0];
    assert!(skills::record_use(&mut empty, 7));
    assert_eq!(empty, [7, 5, 0]);
    // Unavailable: 590 plus D(0, 2), failure, and no kit draw after it.
    let mut host = host();
    let player = host.world.player;
    let target = critter(&mut host, adjacent(1), &[]);
    set_hp(&mut host, target, 10);
    let kit = items::carry(&mut host, player, KIT_PID, 1);
    host.world.persistence.skill_uses[6] = [100_000, 200_000, 300_000];
    host.world.clock.ticks = 999_999;
    host.world.rng.script(&[2]);
    host.use_kit(player, kit, target).unwrap();
    assert_script_used(&host);
    assert_eq!(messages(&mut host), ["taxed 2"]);
    assert_eq!(hp(&host, target), 10);
    // A dead target: D(0, 2) and its message (safe rule: displayed).
    host.world.clock.ticks = 2_000_000;
    if let Some(ObjectState::Critter(state)) = host.world.get_mut(target).map(|o| &mut o.raw.state)
    {
        state.results.0 |= 0x80;
    }
    host.world.rng.script(&[1, 9]);
    host.use_kit(player, kit, target).unwrap();
    assert_script_used(&host);
    assert_eq!(messages(&mut host), ["dead 1"]);
}

#[test]
fn spec013_fixture_c_explosive_timers_and_detonation() {
    let traps = |host: &WorldHost<ItemAssets>| skill(host, 11);
    let arm = |outcomes: &dyn Fn(i32, i32) -> [i32; 2]| {
        let mut host = host();
        let player = host.world.player;
        let dynamite = items::carry(&mut host, player, skills::DYNAMITE, 1);
        assert!(host.explosive_armable(dynamite));
        let draws = outcomes(traps(&host), critical(&host));
        host.world.rng.script(&draws);
        host.arm_explosive(dynamite, 60).unwrap();
        assert_script_used(&host);
        let event = host
            .world
            .queue
            .events()
            .find(|event| event.object == dynamite)
            .cloned()
            .unwrap();
        (host, dynamite, event)
    };
    // Failure without upgrade: type 11 after 300 ticks.
    let (mut host, dynamite, event) = arm(&|traps, _| [traps + 10, 95]);
    assert_eq!(event.kind, EventKind::Other(11));
    assert_eq!(event.due, host.world.clock.ticks + 300);
    assert_eq!(messages(&mut host), ["timer set"]);
    let object = host.world.get(dynamite).unwrap();
    assert_eq!(object.raw.pid, skills::DYNAMITE_ARMED, "armed permanently");
    assert!(object.raw.flags.contains(BasicFlags::USED));
    assert!(!host.explosive_armable(dynamite));
    assert_eq!(messages(&mut host), ["ticking"]);
    // Success: type 8 after 600 ticks.
    let (armed, _, event) = arm(&|_, _| [1, 100]);
    assert_eq!(
        (event.kind, event.due - armed.world.clock.ticks),
        (EventKind::Other(8), 600)
    );
    // Critical failure: type 11 at once.
    let (armed, _, event) = arm(&|traps, _| [100, (100 - traps) / 10]);
    assert_eq!(
        (event.kind, event.due - armed.world.clock.ticks),
        (EventKind::Other(11), 0)
    );

    // Detonation: draw 40 against explosion DT 4 and DR 20 gives 29 and
    // knockback 2; a critter on an adjacent hex takes nothing.
    assert_eq!(skills::explosion_damage(40, 4, 20, false), (29, 2));
    assert_eq!(skills::explosion_damage(40, 4, 20, true), (29, 0));
    assert_eq!(skills::explosion_damage(3, 4, 20, false), (0, 0));
    let mut host = self::host();
    let center = HexGrid::step(20100, 1, 3).unwrap();
    let victim = critter(&mut host, center, &[(23, 4), (30, 20)]);
    let bystander = critter(&mut host, HexGrid::step(center, 1, 1).unwrap(), &[]);
    let dynamite = items::spawn(&mut host, skills::DYNAMITE_ARMED);
    host.world.move_object(dynamite, center, 0, false).unwrap();
    host.world
        .queue
        .insert(host.world.clock.ticks, 0, dynamite, EventKind::Other(11))
        .unwrap();
    host.world.rng.script(&[40]);
    host.process_queue().unwrap();
    assert_script_used(&host);
    assert_eq!(messages(&mut host), ["premature"]);
    assert!(
        host.world.get(dynamite).is_none(),
        "the explosive is destroyed"
    );
    // The animated explosion is in progress: the damage, computed with the
    // one draw, is applied after the art has played (here there is no art,
    // so it plays out on the first poll).
    assert!(host.world.explosion.is_some());
    assert_eq!(hp(&host, victim), 30);
    assert!(host.step_explosions(0).unwrap());
    assert!(host.world.explosion.is_none());
    assert_eq!(hp(&host, victim), 1);
    assert_eq!(hp(&host, bystander), 30);
    assert_eq!(host.world.get(victim).unwrap().opponent, host.world.player);
    assert!(host.world.pending_combat.is_some());
    // Map exit detonates a carried explosive at once, without 4000.
    let player = host.world.player;
    let carried = items::carry(&mut host, player, skills::DYNAMITE_ARMED, 1);
    host.world
        .queue
        .insert(host.world.clock.ticks, 6000, carried, EventKind::Other(8))
        .unwrap();
    host.skill_map_exit().unwrap();
    assert!(host.world.get(carried).is_none());
    assert!(messages(&mut host).is_empty());
}

#[test]
fn spec013_explosion_retry_scripted_requests_and_scenery_notice() {
    use crate::world::{ActiveExplosion, ExplosionRequest, WorldTrace};
    let mut host = host();
    let center = HexGrid::step(20100, 1, 3).unwrap();
    let victim = critter(&mut host, center, &[(23, 4), (30, 20)]);
    // Two explosives due together: the second, a type 11, shows 4000 once,
    // meets the first in progress, and is re-queued as type 8 after 50
    // ticks without being destroyed.
    let first = items::spawn(&mut host, skills::DYNAMITE_ARMED);
    host.world.move_object(first, center, 0, false).unwrap();
    let second = items::spawn(&mut host, skills::DYNAMITE_ARMED);
    let elsewhere = HexGrid::step(center, 4, 6).unwrap();
    host.world.move_object(second, elsewhere, 0, false).unwrap();
    let now = host.world.clock.ticks;
    for (explosive, kind) in [(first, 8), (second, 11)] {
        host.world
            .queue
            .insert(now, 0, explosive, EventKind::Other(kind))
            .unwrap();
    }
    host.world.rng.script(&[40]);
    host.process_queue().unwrap();
    assert_script_used(&host);
    assert_eq!(messages(&mut host), ["premature"]);
    assert!(host.world.get(first).is_none());
    assert!(host.world.get(second).is_some(), "kept for the retry");
    let retry = host
        .world
        .queue
        .events()
        .find(|event| event.object == second)
        .cloned()
        .unwrap();
    assert_eq!((retry.kind, retry.due - now), (EventKind::Other(8), 50));
    assert!(host.step_explosions(0).unwrap());
    assert_eq!(hp(&host, victim), 1);

    // Scripted requests: the combat request pass discards one; one meeting
    // an explosion in progress waits for the next pass (safe rule).
    // (The first blast's knockback moved the victim off the center.)
    host.world.move_object(victim, center, 0, false).unwrap();
    set_hp(&mut host, victim, 30);
    host.world.get_mut(victim).unwrap().opponent = 0;
    host.world.pending_combat = None;
    let request = ExplosionRequest {
        hex: center,
        elevation: 0,
        min: 10,
        max: 10,
    };
    host.world.pending_explosion = Some(request);
    host.world.combat = true;
    assert!(!host.service_explosion_request().unwrap());
    assert_eq!(host.world.pending_explosion, None);
    host.world.combat = false;
    host.world.pending_explosion = Some(request);
    host.world.explosion = Some(ActiveExplosion {
        hex: 0,
        elevation: 0,
        victim: None,
        source: None,
        art: host.explosion_art(0, 0, skills::explosion_art::ROCKET, 0),
        notified: false,
    });
    assert!(!host.service_explosion_request().unwrap());
    assert_eq!(host.world.pending_explosion, Some(request));
    host.world.explosion = None;

    // Out of combat it detonates with animation and no credited source. The
    // scenery object 3 hexes away whose script has a damage_p_proc is
    // notified once the art has played; one 4 hexes away is not.
    host.assets.program = Some(
        vault_vm::IntProgram::parse(&crate::fixture_state::program(
            &[],
            &[
                (b"start", crate::fixture_state::returning(Vec::new())),
                (
                    b"damage_p_proc",
                    crate::fixture_state::returning(Vec::new()),
                ),
            ],
        ))
        .unwrap(),
    );
    let scenery = |host: &mut WorldHost<ItemAssets>, hex| {
        let mut bytes = items::world::prototype(2, 0);
        items::world::word(&mut bytes, 0, 0x0200_0077_u32 as i32);
        let object = host.world.spawn(items::parse(&bytes), hex, 0).unwrap();
        host.world.add_script(object, 0, 3, 0).unwrap()
    };
    let near = scenery(&mut host, HexGrid::step(center, 2, 3).unwrap());
    let far = scenery(&mut host, HexGrid::step(center, 2, 4).unwrap());
    assert!(host.service_explosion_request().unwrap());
    assert_eq!(hp(&host, victim), 30, "damage waits for the art");
    host.world.trace.clear();
    assert!(host.step_explosions(0).unwrap());
    // 10 - DT 4 = 6, less trunc(6 * 20 / 100) = 1: 5 damage.
    assert_eq!(hp(&host, victim), 25);
    assert_eq!(host.world.get(victim).unwrap().opponent, 0);
    assert!(host.world.pending_combat.is_none());
    let notified = |host: &WorldHost<ItemAssets>, script| {
        host.world.trace.iter().any(|entry| {
            matches!(entry, WorldTrace::Script { id, event: crate::ScriptEvent::Damage, .. } if *id == script)
        })
    };
    assert!(notified(&host, near));
    assert!(!notified(&host, far));
}

#[test]
fn spec013_fixture_d_poison_timeline() {
    let mut host = host();
    let player = host.world.player;
    let context = host.world.context(player);
    let sheet = host.world.player_sheet_mut().unwrap();
    sheet.base[2] = 4;
    sheet.bonus[7] = 100;
    sheet.recompute(context);
    sheet.heal(1000, context);
    host.world.sync_hit_points(player);
    assert_eq!(host.world.player_stat(32), 20, "Poison Resistance 5 × EN");
    assert!(host.adjust_poison(player, 10).unwrap());
    assert_eq!(host.world.player_sheet().unwrap().poison, 8);
    assert_eq!(messages(&mut host), ["poisoned"]);
    let next = |host: &WorldHost<ItemAssets>| {
        host.world
            .queue
            .events()
            .find(|event| event.kind == EventKind::Poison)
            .map(|event| event.due - host.world.clock.ticks)
    };
    assert_eq!(next(&host), Some(4650));
    let start = hp(&host, player);
    for (level, delay, expected) in [
        (6, Some(4750), vec!["poisoned", "poison damage"]),
        (4, Some(4850), vec!["poisoned", "poison damage"]),
        (2, Some(4950), vec!["poisoned", "poison damage"]),
        (0, None, vec!["poison damage"]),
    ] {
        host.world.clock.ticks = host.world.queue.next_due().unwrap();
        host.process_queue().unwrap();
        assert_eq!(host.world.player_sheet().unwrap().poison, level);
        assert_eq!(next(&host), delay);
        assert_eq!(messages(&mut host), expected);
    }
    assert_eq!(hp(&host, player), start - 4);
    // Level 150: ticks repeat in one pass until the level is 100.
    host.world.player_sheet_mut().unwrap().poison = 148;
    host.adjust_poison(player, 2).unwrap();
    let start = hp(&host, player);
    host.process_queue().unwrap();
    assert_eq!(host.world.player_sheet().unwrap().poison, 100);
    assert_eq!(hp(&host, player), start - 25);
    // Another critter: no change (the opcode's diagnostic is the caller's).
    let npc = critter(&mut host, adjacent(1), &[]);
    assert!(!host.adjust_poison(npc, 10).unwrap());
    assert_eq!(skills::poison_delay(1), 5000);
    assert_eq!(skills::poison_delay(100), 50);
}

#[test]
fn spec013_fixture_d_radiation_check_onset_and_recovery() {
    let mut host = host();
    let player = host.world.player;
    let context = host.world.context(player);
    let sheet = host.world.player_sheet_mut().unwrap();
    sheet.base[2] = 6;
    sheet.bonus[7] = 100;
    sheet.recompute(context);
    sheet.heal(1000, context);
    sheet.radiation = 450;
    sheet.flags |= crate::host_skills::EXPOSED_FLAG;
    host.world.sync_hit_points(player);
    assert_eq!(skills::radiation_level(450), (3, -4));
    // D(1, 10) = 3 exceeds 6 - 4: level 4; D(4, 18) = 10 game hours.
    host.world.rng.script(&[3, 10]);
    host.radiation_check().unwrap();
    assert_script_used(&host);
    let onset = host.world.queue.events().next().cloned().unwrap();
    assert_eq!(
        onset.kind,
        EventKind::Radiation {
            level: 4,
            healing: 0
        }
    );
    assert_eq!(onset.due - host.world.clock.ticks, 360_000);
    assert_eq!(
        host.world.player_sheet().unwrap().flags & crate::host_skills::EXPOSED_FLAG,
        0
    );
    let bonus = |host: &WorldHost<ItemAssets>| {
        let sheet = host.world.player_sheet().unwrap();
        [
            sheet.bonus[0],
            sheet.bonus[2],
            sheet.bonus[5],
            sheet.bonus[14],
        ]
    };
    let hp_before = hp(&host, player);
    host.world.clock.ticks = onset.due;
    host.process_queue().unwrap();
    assert_eq!(messages(&mut host), ["rad 4"]);
    assert_eq!(bonus(&host), [-2, -1, -2, -5]);
    assert_eq!(hp(&host, player), hp_before - 5);
    let recovery = host.world.queue.events().next().cloned().unwrap();
    assert_eq!(
        recovery.kind,
        EventKind::Radiation {
            level: 4,
            healing: 1
        }
    );
    assert_eq!(
        recovery.due - host.world.clock.ticks,
        skills::RADIATION_RECOVERY_TICKS
    );
    host.world.clock.ticks = recovery.due;
    host.process_queue().unwrap();
    assert_eq!(
        messages(&mut host),
        ["rad 4"],
        "recovery repeats the stage text"
    );
    assert_eq!(bonus(&host), [0, 0, 0, 0]);
    assert_eq!(hp(&host, player), hp_before);
    // Without the exposed flag the midnight check draws nothing.
    host.radiation_check().unwrap();
    // Exposure: resistance, the flag, and the big-dose message.
    let resistance = host.world.player_stat(31);
    assert!(host.adjust_radiation(player, 20));
    let added = 20 - resistance * 20 / 100;
    assert_eq!(host.world.player_sheet().unwrap().radiation, 450 + added);
    assert_ne!(
        host.world.player_sheet().unwrap().flags & crate::host_skills::EXPOSED_FLAG,
        0
    );
    assert_eq!(
        messages(&mut host),
        if added >= 10 {
            vec!["big dose"]
        } else {
            vec![]
        }
    );
    let npc = critter(&mut host, adjacent(1), &[]);
    assert!(!host.adjust_radiation(npc, 50), "a silent no-op on others");
}

#[test]
fn spec013_fixture_e_elevators() {
    // E2 under the safe rule: type 8 on map 6, elevation 2 starts at level 2;
    // three buttons travelling two levels plays elv1_1; the gauge runs from
    // cell 12 to cell 0.
    let vault = skills::elevator(8).unwrap();
    assert_eq!(vault.start_level(6, 2), 2);
    assert_eq!(vault.travel_sound(2), Some("elv1_1"));
    assert_eq!(vault.travel_sound(1), Some("elv1_2"));
    let path = vault.gauge_path(2, 0);
    assert!(path[0] < 12 && path.windows(2).all(|w| w[0] >= w[1]));
    assert_eq!(*path.last().unwrap(), 0);
    assert_eq!(vault.step_ms(), 46);
    let four = skills::elevator(0).unwrap();
    assert_eq!(
        [0, 1, 2, 3].map(|level| four.gauge_cell(level)),
        [0, 4, 8, 12]
    );
    assert_eq!(four.step_ms(), 69);
    assert_eq!(skills::elevator(1).unwrap().step_ms(), 23);
    // E4: lowercase g selects button 0 under the safe rule.
    assert_eq!(skills::elevator(1).unwrap().button_for_key(b'g'), Some(0));
    // E5: type 12 does not exist.
    assert!(skills::elevator(12).is_none());
    assert_eq!(skills::elevator_origin([230, 284]), [205, 47]);
    assert_eq!(skills::elevator_origin([231, 285]), [204, 47]);

    // E1 (safe start): pressing 1 on Vault 13 elevation 2 places the player
    // at 14104 on elevation 0 facing southeast and closes a nearby door.
    let mut host = host();
    let player = host.world.player;
    host.world.map.header.map_number = 6;
    host.world.move_object(player, 20100, 2, false).unwrap();
    let mut door_bytes = items::world::prototype(2, 0);
    items::world::word(&mut door_bytes, 0, 0x0200_0099_u32 as i32);
    let door = host
        .world
        .spawn(items::parse(&door_bytes), adjacent(1), 2)
        .unwrap();
    {
        let door = host.world.get_mut(door).unwrap();
        door.raw.frame = 1;
        door.raw.flags.0 |= 0x10 | BasicFlags::PROJECTILE_THROUGH.0;
    }
    assert_eq!(host.elevator_start_level(8), 2);
    host.perform_elevator(8, Some(0)).unwrap();
    let object = host.world.get(player).unwrap();
    assert_eq!(
        (object.raw.hex, object.raw.elevation, object.raw.orientation),
        (Some(14104), 0, 2)
    );
    let door = host.world.get(door).unwrap();
    assert_eq!(door.raw.frame, 0);
    assert_eq!(
        door.raw.flags.0 & (0x10 | BasicFlags::PROJECTILE_THROUGH.0),
        0
    );
    // Cancel changes nothing (safe rule).
    host.perform_elevator(8, None).unwrap();
    assert_eq!(host.world.get(player).unwrap().raw.hex, Some(14104));
    // E3: type 0 on map 14, pressing 3, requests a transition.
    host.world.map.header.map_number = 14;
    host.perform_elevator(0, Some(2)).unwrap();
    assert_eq!(
        host.world.pending_transition,
        Some(Transition {
            map: 15,
            elevation: 0,
            hex: 21340,
            facing: 2
        })
    );
}

#[test]
fn spec013_exit_grids_and_world_map_transitions() {
    let mut host = host();
    let player = host.world.player;
    let mut grid_bytes = items::world::prototype(5, 0);
    items::world::word(&mut grid_bytes, 0, 0x0500_0010);
    let grid = host
        .world
        .spawn(items::parse(&grid_bytes), adjacent(0), 0)
        .unwrap();
    host.world.get_mut(grid).unwrap().raw.state = ObjectState::Other {
        word: 0,
        detail: ObjectDetail::ExitGrid {
            map: 0,
            hex: 12000,
            elevation: 1,
            orientation: 3,
        },
    };
    host.world
        .move_object(player, adjacent(0), 0, true)
        .unwrap();
    assert_eq!(
        host.world.pending_transition,
        Some(Transition {
            map: 0,
            elevation: 1,
            hex: 12000,
            facing: 3
        })
    );
    // Map 0 is the world map; transitions wait while combat is active.
    host.world.combat = true;
    assert!(!host.process_transition().unwrap());
    host.world.combat = false;
    host.world.effects.clear();
    assert!(host.process_transition().unwrap());
    assert!(host.world.effects.contains(&WorldEffect::WorldMap));
    assert!(host.world.pending_transition.is_none());
}

#[test]
fn spec013_fixture_f_counter_and_set_timer() {
    let mut counter = Counter::new(25);
    assert_eq!(counter.value, 1);
    counter.digit(3);
    counter.digit(7);
    assert_eq!(counter.value, 37);
    assert_eq!(counter.enter(), None, "37 exceeds 25: iisxxxx1");
    counter.minus();
    assert_eq!((counter.value, counter.enter()), (36, None));
    counter.all();
    assert_eq!(counter.enter(), Some(25));
    let mut fresh = Counter::new(999);
    for digit in [1, 2, 3, 4] {
        fresh.digit(digit);
    }
    assert_eq!(fresh.value, 234);
    assert_eq!(fresh.digits(), [2, 3, 4]);
    fresh.backspace();
    assert_eq!(fresh.value, 23);
    fresh.plus();
    fresh.backspace();
    assert_eq!(
        fresh.value, 0,
        "Backspace starts fresh after another action"
    );
    assert_eq!(Counter::new(5000).max, 999);
    let mut timer = SetTimer::default();
    assert_eq!(timer.seconds, 60);
    for _ in 0..20 {
        timer.plus();
    }
    assert_eq!(timer.seconds, 180);
    assert_eq!(timer.digits(), [(133, 3), (161, 0), (175, 0)]);
    for _ in 0..20 {
        timer.minus();
    }
    assert!(timer.accepts() && timer.seconds == 10);
    assert_eq!(skills::explosive_timer(1, 60), (11, 300));
    assert_eq!(skills::quantity_label(1, None), None);
    assert_eq!(skills::quantity_label(3, Some((10, 4))).unwrap(), b"x24");
    assert_eq!(skills::quantity_label(200_000, None).unwrap(), b"x99999");
    assert_eq!(skills::loot_drop_target([510, 100]), Some(true));
    assert_eq!(skills::loot_drop_target([130, 323]), Some(false));
    assert_eq!(skills::loot_drop_target([300, 100]), None);
    assert_eq!(skills::skilldex_digits(-1), [0, 0, 0]);
    assert_eq!(skills::skilldex_label(40, 10), [24, 12]);
    use skills::ItemEntry::*;
    assert_eq!(
        skills::item_menu(true, true, false),
        [Look, Unload, Drop, Cancel]
    );
    assert_eq!(skills::item_menu(false, false, true), [Look, Use, Cancel]);
    assert!(skills::BarterRoute::TableToMerchant.accepts([500, 400]));
    assert!(!skills::BarterRoute::TableToPlayer.accepts([160, 400]));
}

#[test]
fn spec013_sneak_toggle_recheck_and_map_exit() {
    let mut host = host();
    let player = host.world.player;
    let rechecks = |host: &WorldHost<ItemAssets>| {
        host.world
            .queue
            .events()
            .filter(|event| event.object == player && event.kind == EventKind::Other(10))
            .count()
    };
    let target = items::spawn(&mut host, ITEM_PID);
    assert!(host.use_skill_on(player, target, 8).unwrap());
    assert!(host.sneaking());
    assert_eq!(host.world.sneak_working, skill(&host, 8) >= 2);
    assert_eq!(rechecks(&host), 1);
    let due = host.world.queue.next_due().unwrap();
    assert_eq!(due, host.world.clock.ticks + 600);
    host.world.clock.ticks = due;
    host.process_queue().unwrap();
    assert_eq!(rechecks(&host), 1, "the recheck queues the next one");
    host.toggle_sneak().unwrap();
    assert!(!host.sneaking());
    assert_eq!(rechecks(&host), 0);
    host.set_sneak(true).unwrap();
    host.skill_map_exit().unwrap();
    assert!(!host.sneaking(), "leaving a map turns sneak off");
}

#[test]
fn spec013_skill_gate_jam_check_and_defaults() {
    let mut host = host();
    let player = host.world.player;
    let npc = critter(&mut host, adjacent(1), &[]);
    let item = items::spawn(&mut host, ITEM_PID);
    host.world.move_object(item, adjacent(2), 0, false).unwrap();
    // Gating: Traps never on a critter; Science only on robots or brahmin;
    // Steal never on oneself; nothing but Sneak in combat (902).
    assert!(!host.use_skill_on(player, npc, 11).unwrap());
    assert!(!host.use_skill_on(player, npc, 12).unwrap());
    assert!(!host.use_skill_on(player, player, 10).unwrap());
    assert!(messages(&mut host).is_empty());
    host.world.combat = true;
    assert!(!host.use_skill_on(player, item, 9).unwrap());
    assert_eq!(messages(&mut host), ["not in combat"]);
    assert!(
        host.use_skill_on(player, item, 8).unwrap(),
        "Sneak passes in combat"
    );
    host.world.combat = false;
    // Defaults on an adjacent target.
    for (index, message) in [
        (11, "no traps"),
        (12, "learn nothing"),
        (13, "cannot repair"),
    ] {
        assert!(host.use_skill_on(player, item, index).unwrap());
        assert_eq!(messages(&mut host), [message]);
    }
    assert!(host.use_skill_on(player, item, 9).unwrap());
    assert!(
        messages(&mut host).is_empty(),
        "Lockpick does nothing by default"
    );
    assert!(host.use_skill_on(player, npc, 10).unwrap());
    assert_eq!(
        host.take_loot_request(),
        Some(LootRequest {
            looter: player,
            target: npc,
            steal: true
        })
    );
    // A jammed container stops every skill with misc.msg 2001.
    let container = items::spawn(&mut host, items::CONTAINER);
    host.world
        .move_object(container, adjacent(3), 0, false)
        .unwrap();
    let object = host.world.get_mut(container).unwrap();
    let mut lock = object.lock().unwrap();
    lock.0 |= vault_formats::pro::LockFlags::JAMMED.0;
    object.set_lock(lock);
    assert!(host.use_skill_on(player, container, 11).unwrap());
    assert_eq!(messages(&mut host), ["jammed"]);
}

#[test]
fn spec013_steal_and_loot_sessions() {
    let mut host = host();
    let player = host.world.player;
    let victim = critter(&mut host, adjacent(1), &[]);
    host.world.get_mut(victim).unwrap().name = b"Gizmo".to_vec();
    let loot = items::carry(&mut host, victim, ITEM_PID, 3);
    host.world.get_mut(loot).unwrap().name = b"Watch".to_vec();
    let armor = items::carry(&mut host, victim, items::ARMOR_A, 1);
    host.world.get_mut(armor).unwrap().raw.flags.0 |= BasicFlags::WORN.0;
    let request = LootRequest {
        looter: player,
        target: victim,
        steal: true,
    };
    let mut session = host.open_loot(request).unwrap().unwrap();
    assert!(session.steal);
    assert_eq!(
        host.loot_items(&session, true),
        [(loot, 3)],
        "equipment is hidden"
    );
    // Not caught: steal success, catch failure.
    session.press();
    host.world.rng.script(&[1, 100, 100, 100]);
    assert_eq!(
        host.loot_move(&mut session, true, loot, 1).unwrap(),
        LootMove::Moved
    );
    assert_script_used(&host);
    assert_eq!(messages(&mut host), ["take Watch"]);
    assert_eq!(host.world.inventory_count(player, ITEM_PID), 1);
    let experience = host.world.player_sheet().unwrap().experience;
    host.close_loot(session).unwrap();
    assert_eq!(messages(&mut host), ["steal xp 10"]);
    assert_eq!(
        host.world.player_sheet().unwrap().experience,
        experience + 10
    );
    // Caught: the catch succeeds, nothing moves, no experience.
    let mut session = host.open_loot(request).unwrap().unwrap();
    session.press();
    host.world.rng.script(&[1, 100, 1, 100]);
    assert_eq!(
        host.loot_move(&mut session, true, loot, 1).unwrap(),
        LootMove::Caught
    );
    assert_eq!(messages(&mut host), ["caught taking Watch"]);
    assert_eq!(host.world.inventory_count(victim, ITEM_PID), 2);
    host.close_loot(session).unwrap();
    assert!(messages(&mut host).is_empty());
    // A dead victim opens the ordinary window: equipment shows, no checks,
    // and Take All moves everything that fits.
    if let Some(ObjectState::Critter(state)) = host.world.get_mut(victim).map(|o| &mut o.raw.state)
    {
        state.results.0 |= 0x80;
    }
    let session = host.open_loot(request).unwrap().unwrap();
    assert!(!session.steal);
    assert_eq!(host.loot_items(&session, true).len(), 2);
    assert!(host.loot_take_all(&session).unwrap());
    assert!(host.loot_items(&session, true).is_empty());
    host.close_loot(session).unwrap();
    // A party member is robbed with no draws and no experience.
    let friend = critter(&mut host, adjacent(4), &[]);
    host.world.party.insert(friend);
    let gift = items::carry(&mut host, friend, ITEM_PID, 1);
    let mut session = host
        .open_loot(LootRequest {
            looter: player,
            target: friend,
            steal: true,
        })
        .unwrap()
        .unwrap();
    session.press();
    assert_eq!(
        host.loot_move(&mut session, true, gift, 1).unwrap(),
        LootMove::Moved
    );
    host.close_loot(session).unwrap();
    assert!(messages(&mut host).is_empty());
}

#[test]
fn spec013_containers_and_critters_post_loot_requests() {
    let mut host = host();
    let player = host.world.player;
    let container = items::spawn(&mut host, items::CONTAINER);
    host.world
        .move_object(container, adjacent(1), 0, false)
        .unwrap();
    host.world.get_mut(container).unwrap().name = b"locker".to_vec();
    host.action(player, container, crate::ObjectAction::Use)
        .unwrap();
    assert!(messages(&mut host).contains(&"search locker".to_string()));
    assert_eq!(host.world.get(container).unwrap().raw.frame, 1);
    assert_eq!(
        host.take_loot_request().map(|request| request.target),
        Some(container)
    );
    // Using it again (open) only closes it.
    host.action(player, container, crate::ObjectAction::Use)
        .unwrap();
    assert!(messages(&mut host).contains(&"close locker".to_string()));
    assert_eq!(host.take_loot_request(), None);
    // A locked (closed) container shows 487 and still posts its loot
    // request; the window then refuses the closed container.
    let object = host.world.get_mut(container).unwrap();
    let mut lock = object.lock().unwrap();
    lock.0 |= vault_formats::pro::LockFlags::LOCKED.0;
    object.set_lock(lock);
    host.action(player, container, crate::ObjectAction::Use)
        .unwrap();
    assert!(messages(&mut host).contains(&"locked".to_string()));
    assert_eq!(host.world.get(container).unwrap().raw.frame, 0);
    assert_eq!(
        host.take_loot_request().map(|request| request.target),
        Some(container)
    );
    // Using a critter loots it.
    let npc = critter(&mut host, adjacent(2), &[]);
    host.action(player, npc, crate::ObjectAction::Use).unwrap();
    assert_eq!(
        host.take_loot_request().map(|request| request.steal),
        Some(false)
    );
}

#[test]
fn spec014_corpse_look_at_leaves_the_world_generator_alone() {
    // SPEC-014 §3.27: a dead critter's look-at line is 491 or 492 at
    // random; the pick draws from the presentation generator, so looking
    // never moves the world's sequence.
    let mut host = host();
    let player = host.world.player;
    let corpse = critter(&mut host, adjacent(1), &[]);
    set_hp(&mut host, corpse, 0);
    host.world.get_mut(corpse).unwrap().name = b"rat".to_vec();
    let world = host.world.rng.state().clone();
    let presentation = host.presentation_rng.state().clone();
    let mut lines = Vec::new();
    for _ in 0..4 {
        host.action(player, corpse, crate::ObjectAction::LookAt)
            .unwrap();
        lines.extend(messages(&mut host));
    }
    assert_eq!(lines.len(), 4);
    assert!(lines.iter().all(|line| line.contains("rat")), "{lines:?}");
    assert_eq!(host.world.rng.state(), &world);
    assert_ne!(host.presentation_rng.state(), &presentation);
}

#[test]
fn spec013_fixture_g_skill_books() {
    use crate::{BookRoute, stats::Trait};
    // A host with the five books, effective Intelligence `intelligence`,
    // the given traits and tag, and `count` copies of book `pid` carried.
    let setup = |pid: u32, intelligence: i32, traits: [i32; 2], tag: i32, count: i32| {
        let mut host = host();
        for (book, _, _) in skills::SKILL_BOOKS {
            host.assets
                .prototypes
                .insert(book, items::misc(book, 0, 1, 10));
        }
        let gifted = traits.contains(&(Trait::Gifted as i32));
        let sheet = host.world.player_sheet_mut().unwrap();
        sheet.traits = traits;
        sheet.tags = [tag, -1, -1, -1];
        sheet.base[4] = intelligence - i32::from(gifted);
        let player = host.world.player;
        let book = items::carry(&mut host, player, pid, count);
        host.world.effects.clear();
        (host, book)
    };
    // The effective skill set to `target` through invested points.
    let set_skill = |host: &mut WorldHost<ItemAssets>, index: usize, target: i32| {
        host.world.player_sheet_mut().unwrap().invested[index] = 0;
        let start = skill(host, index);
        let tagged = host
            .world
            .player_sheet()
            .unwrap()
            .tags
            .contains(&(index as i32));
        let per = if tagged { 2 } else { 1 };
        assert!(
            target >= start && (target - start) % per == 0,
            "{start} to {target}"
        );
        host.world.player_sheet_mut().unwrap().invested[index] = (target - start) / per;
        assert_eq!(skill(host, index), target);
    };
    let unspent = |host: &WorldHost<ItemAssets>| host.world.player_sheet().unwrap().unspent_skills;
    let none = [-1, -1];
    // G1: Big Book of Science, Science 45 untagged, two copies carried.
    let (mut host, book) = setup(73, 6, none, -1, 2);
    set_skill(&mut host, 12, 45);
    let (points, clock) = (unspent(&host), host.world.clock.ticks);
    let rng = host.world.rng.state().clone();
    let experience = host.world.player_sheet().unwrap().experience;
    assert!(host.read_book(book, BookRoute::Inventory).unwrap());
    assert_eq!(skill(&host, 12), 50);
    assert_eq!(unspent(&host), points);
    assert_eq!(host.world.clock.ticks - clock, 180_000);
    assert_eq!(host.world.rng.state(), &rng, "no draws");
    assert_eq!(host.world.player_sheet().unwrap().experience, experience);
    let fades: Vec<bool> = host
        .world
        .effects
        .iter()
        .filter_map(|effect| match effect {
            WorldEffect::Fade { out, .. } => Some(*out),
            _ => None,
        })
        .collect();
    assert_eq!(fades, [true, false]);
    assert_eq!(messages(&mut host), ["read", "science"]);
    // One copy is read and destroyed; the other stays carried.
    assert!(host.world.get(book).is_none());
    let player = host.world.player;
    let carried: i32 = host
        .world
        .get(player)
        .unwrap()
        .inventory
        .iter()
        .map(|(_, n)| n)
        .sum();
    assert_eq!(carried, 1);
    // G2: Guns and Bullets, Small Guns 45 tagged (Gifted and Good Natured
    // bring the tagged skill down to 45): +2 per point.
    let traits = [Trait::Gifted as i32, Trait::GoodNatured as i32];
    let (mut host, book) = setup(102, 6, traits, 0, 1);
    set_skill(&mut host, 0, 45);
    assert!(host.read_book(book, BookRoute::Inventory).unwrap());
    assert_eq!(skill(&host, 0), 55);
    assert_eq!(messages(&mut host), ["read", "guns"]);
    // G3 and G4: Scout Handbook at Outdoorsman 90 gives one point; at 91,
    // none, but the time passes and the book is destroyed all the same.
    for (start, after, result) in [(90, 91, "outdoors"), (91, 91, "nothing new")] {
        let (mut host, book) = setup(86, 6, none, -1, 1);
        set_skill(&mut host, 17, start);
        let clock = host.world.clock.ticks;
        assert!(host.read_book(book, BookRoute::Inventory).unwrap());
        assert_eq!(skill(&host, 17), after);
        assert_eq!(messages(&mut host), ["read", result]);
        assert_eq!(host.world.clock.ticks - clock, 180_000);
        assert!(host.world.get(book).is_none());
    }
    // G5: Science 120 gives n = -2: unchanged, message 801.
    let (mut host, book) = setup(73, 6, none, -1, 1);
    set_skill(&mut host, 12, 120);
    assert!(host.read_book(book, BookRoute::Inventory).unwrap());
    assert_eq!(skill(&host, 12), 120);
    assert_eq!(messages(&mut host), ["read", "nothing new"]);
    // G6: the First Aid Book takes 1 hour at Intelligence 10, 10 at 1.
    for (intelligence, ticks) in [(10, 36_000), (1, 360_000)] {
        let (mut host, book) = setup(80, intelligence, none, -1, 1);
        let clock = host.world.clock.ticks;
        assert!(host.read_book(book, BookRoute::Inventory).unwrap());
        assert_eq!(host.world.clock.ticks - clock, ticks);
    }
    // G7: in combat message 902; the book is kept.
    let (mut host, book) = setup(76, 6, none, -1, 1);
    host.world.combat = true;
    let before = skill(&host, 13);
    assert!(host.read_book(book, BookRoute::ItemSlot).unwrap());
    assert_eq!(messages(&mut host), ["not in combat"]);
    assert!(host.world.get(book).is_some());
    assert_eq!(skill(&host, 13), before);
    // Anything else is not a book.
    let (mut host, _) = setup(73, 6, none, -1, 1);
    let player = host.world.player;
    let other = items::carry(&mut host, player, ITEM_PID, 1);
    assert!(!host.read_book(other, BookRoute::Inventory).unwrap());
}

#[test]
fn spec013_explosion_art_schedule() {
    use crate::world::ExplosionArt;
    let frame = |art: &ExplosionArt, index: usize| art.objects[index].frame;
    // A weapon explosion: six frames at 100 ms, direction 0's art waiting
    // two frame steps while the center runs, the other five behind it.
    let mut art = ExplosionArt::new(20100, 0, skills::explosion_art::ROCKET, [6, 100], 2);
    assert_eq!(art.objects.len(), 7);
    assert!(art.poll(1000));
    assert_eq!(
        (frame(&art, 0), frame(&art, 1), frame(&art, 6)),
        (Some(0), None, None)
    );
    assert!(!art.poll(1099), "the next step is due at 1100");
    art.poll(1100);
    assert_eq!((frame(&art, 0), frame(&art, 1)), (Some(1), None));
    art.poll(1200);
    assert_eq!(
        (frame(&art, 0), frame(&art, 1), frame(&art, 6)),
        (Some(2), Some(0), Some(0))
    );
    art.poll(1600);
    assert!(art.center_done() && !art.finished());
    assert_eq!(frame(&art, 1), Some(4));
    art.poll(1800);
    assert!(art.finished());
    // Timed and scripted explosions: no delay, all seven play together.
    let mut art = ExplosionArt::new(20100, 0, skills::explosion_art::ROCKET, [6, 100], 0);
    art.poll(0);
    assert!(art.objects.iter().all(|object| object.frame == Some(0)));
    art.poll(500);
    assert!(art.objects.iter().all(|object| object.frame == Some(5)));
    art.poll(600);
    assert!(art.finished());
    // A delay is ignored once no earlier entry runs.
    let mut art = ExplosionArt::new(20100, 0, skills::explosion_art::GRENADE, [1, 100], 4);
    art.poll(0);
    art.poll(100);
    assert!(art.center_done());
    assert_eq!(frame(&art, 1), Some(0));
    // Art that cannot be loaded finishes on the first poll.
    let mut art = ExplosionArt::new(20100, 0, skills::explosion_art::ROCKET, [0, 100], 2);
    art.poll(0);
    assert!(art.finished());
}

#[test]
fn spec013_timed_explosion_art_precedes_the_damage_step() {
    use crate::world::{EXPLOSION_ART_ID, WorldTrace};
    let mut host = host();
    host.assets.art.insert(
        skills::explosion_art::ROCKET,
        Arc::new(
            vault_formats::FrmArchive::parse(&crate::fixture_state::animated_frame(
                12,
                20,
                &[1, 2, 3, 4, 5, 6],
            ))
            .unwrap(),
        ),
    );
    let center = HexGrid::step(20100, 1, 3).unwrap();
    let victim = critter(&mut host, center, &[(23, 4), (30, 20)]);
    let dynamite = items::spawn(&mut host, skills::DYNAMITE_ARMED);
    host.world.move_object(dynamite, center, 0, false).unwrap();
    host.world
        .queue
        .insert(host.world.clock.ticks, 0, dynamite, EventKind::Other(8))
        .unwrap();
    host.world.rng.script(&[40]);
    host.process_queue().unwrap();
    // roktxpd.frm on the center and the six adjacent hexes, frame 0 at the
    // first poll, with whn1xxx1 at the start.
    assert!(host.world.effects.iter().any(|effect| matches!(
        effect,
        WorldEffect::SoundEffect { name, .. } if name == b"whn1xxx1"
    )));
    host.step_explosions(1000).unwrap();
    let arts = |host: &WorldHost<ItemAssets>| -> Vec<(i32, usize)> {
        host.world.render_snapshot().objects.elevations[0]
            .iter()
            .filter(|object| object.id == EXPLOSION_ART_ID)
            .map(|object| (object.hex.unwrap(), object.frame))
            .collect()
    };
    let shown = arts(&host);
    assert_eq!(shown.len(), 7);
    assert_eq!(shown[0], (center, 0));
    assert!(shown.iter().all(|(_, frame)| *frame == 0));
    // Six frames at the art's rate; the damage waits for the last.
    let interval = host.world.explosion.as_ref().unwrap().art.interval_ms;
    for step in 1..6 {
        host.step_explosions(1000 + step * interval).unwrap();
        assert!(host.world.explosion.is_some());
        assert_eq!(hp(&host, victim), 30);
        assert!(arts(&host).iter().all(|(_, frame)| *frame == step as usize));
    }
    host.world.trace.clear();
    host.step_explosions(1000 + 6 * interval).unwrap();
    assert!(host.world.explosion.is_none());
    assert_eq!(hp(&host, victim), 1);
    assert!(arts(&host).is_empty());
    assert!(
        !host
            .world
            .trace
            .iter()
            .any(|entry| matches!(entry, WorldTrace::Script { .. }))
    );
}
