//! SPEC-007 combat, routing, and ray tests (sections 2.4-3.8, fixtures B-C).
use crate::critical::{self, CriticalEffect, PLAYER_MATRIX, effect, location};

fn table(matrix: usize, place: u8, tier: usize) -> CriticalEffect {
    critical::success_effect(matrix, place, tier).unwrap()
}
fn values(entry: CriticalEffect) -> [i64; 7] {
    [
        entry.multiplier.into(),
        entry.flags.into(),
        entry.save_stat.into(),
        entry.save_adjustment.into(),
        entry.failed_flags.into(),
        entry.message.into(),
        entry.failed_message.into(),
    ]
}

#[test]
fn spec007_critical_matrices_compile_with_hex_fields_and_repeats() {
    // Fixture C's entry: victim 0, right leg, tier 4 = 4/80A/2/0/1/5025/5026.
    assert_eq!(
        values(table(0, location::RIGHT_LEG, 4)),
        [4, 0x80a, 2, 0, 1, 5025, 5026]
    );
    // `=` repeats the entry to its left: man/left arm tier 4 equals tier 3.
    assert_eq!(
        table(0, location::LEFT_ARM, 4),
        table(0, location::LEFT_ARM, 3)
    );
    assert_eq!(
        values(table(0, location::LEFT_ARM, 5)),
        [4, 0x810, -1, 0, 0, 5013, 5000]
    );
    // Only the second and fifth values are hexadecimal: 1460 is a hex mask.
    assert_eq!(
        values(table(2, location::RIGHT_LEG, 1)),
        [3, 0xa, -1, 0, 0x1460, 5000, 0]
    );
    assert_eq!(
        table(2, location::RIGHT_LEG, 2),
        table(2, location::RIGHT_LEG, 1)
    );
    // Rows made entirely of repeats, and the player matrix.
    for tier in 0..6 {
        assert_eq!(
            values(table(9, location::GROIN, tier)),
            [2, 0, -1, 0, 0, 5912, 5000]
        );
        assert_eq!(
            table(14, location::LEFT_ARM, tier),
            table(14, location::LEFT_ARM, 0)
        );
    }
    assert_eq!(
        values(table(PLAYER_MATRIX, location::HEAD, 5)),
        [6, 0x800, 2, -2, 0x80, 6501, 6505]
    );
    assert_eq!(
        values(table(PLAYER_MATRIX, location::UNCALLED, 5)),
        [4, 0x802, 6, 2, 0x80, 6503, 6513]
    );
    // Uncalled rows are stored separately and can differ from torso.
    assert_ne!(
        table(0, location::UNCALLED, 2),
        table(0, location::TORSO, 2)
    );
    assert_eq!(critical::CRITICAL_SUCCESS.len() * 9 * 6, 864);
    assert!(critical::success_effect(16, 0, 0).is_none());
    assert!(critical::success_effect(0, 9, 0).is_none());
    assert!(critical::success_effect(0, 0, 6).is_none());
}

#[test]
fn spec007_tier_columns_failure_masks_and_location_adjustments() {
    for (score, tier) in [
        (-5, 0),
        (20, 0),
        (21, 1),
        (45, 1),
        (70, 2),
        (90, 3),
        (100, 4),
        (101, 5),
    ] {
        assert_eq!(critical::success_tier(score), tier, "{score}");
    }
    for (score, column) in [(20, 0), (21, 1), (50, 1), (75, 2), (95, 3), (96, 4)] {
        assert_eq!(critical::failure_column(score), column, "{score}");
    }
    // Fixture C boundary: table 2, Luck 7, draw 60 -> score 50 -> column 1.
    let score = 60 - 5 * (7 - 5);
    assert_eq!(
        critical::failure_mask(2, critical::failure_column(score)),
        Some(effect::LOSE_AMMO)
    );
    assert_eq!(critical::failure_mask(0, 3), Some(0x80002));
    assert_eq!(critical::failure_mask(3, 2), Some(0xc000));
    assert_eq!(critical::failure_mask(6, 4), Some(0x9400));
    // Safe rule: a table number outside 0-6 uses table 0.
    assert_eq!(critical::failure_mask(7, 3), Some(0x80002));
    assert_eq!(critical::failure_mask(-1, 3), Some(0x80002));
    assert_eq!(critical::failure_mask(0, 5), None);
    // Half adjustments for melee truncate toward zero.
    assert_eq!(
        critical::location_adjustment(location::EYES, false),
        Some(-60)
    );
    assert_eq!(
        critical::location_adjustment(location::EYES, true),
        Some(-30)
    );
    assert_eq!(
        critical::location_adjustment(location::LEFT_ARM, true),
        Some(-15)
    );
    assert_eq!(
        critical::location_adjustment(location::UNCALLED, true),
        Some(0)
    );
    assert_eq!(critical::location_adjustment(9, false), None);
    assert_eq!(
        effect::PERSISTENT,
        0x1 | 0x2 | 0x4 | 0x8 | 0x10 | 0x20 | 0x40 | 0x80 | 0x8000
    );
}

use crate::{CharacterSheet, ObjectRef, WorldState, routing};
use std::sync::Arc;
use vault_formats::{
    Prototype,
    map::ObjectDetail,
    pro::{BasicFlags, LockFlags},
};

use crate::fixture_world as world_fixture;

fn prototype(category: u8, subtype: usize) -> Arc<Prototype> {
    Arc::new(Prototype::parse(&world_fixture::prototype(category, subtype)).unwrap())
}
fn critter(body: i32) -> Arc<Prototype> {
    let mut bytes = world_fixture::prototype(1, 0);
    world_fixture::word(&mut bytes, 0x190, body);
    Arc::new(Prototype::parse(&bytes).unwrap())
}
/// Player at 20100, facing 5, elevation 0, outside combat.
fn world() -> WorldState {
    let mut world = WorldState::new(CharacterSheet::default(), 1).unwrap();
    world.get_mut(world.player).unwrap().raw.orientation = 5;
    world
}
fn place(world: &mut WorldState, prototype: Arc<Prototype>, hex: i32) -> ObjectRef {
    world.spawn(prototype, hex, 0).unwrap()
}
fn wall(world: &mut WorldState, hex: i32) -> ObjectRef {
    let wall = place(world, prototype(3, 0), hex);
    world.get_mut(wall).unwrap().raw.flags = BasicFlags(0);
    wall
}
fn door(world: &mut WorldState, hex: i32, locked: bool) -> ObjectRef {
    let door = place(world, prototype(2, 0), hex);
    let object = world.get_mut(door).unwrap();
    object.raw.flags = BasicFlags(0);
    object.raw.state = vault_formats::map::ObjectState::Other {
        word: 0,
        detail: ObjectDetail::Door(LockFlags(if locked { LockFlags::LOCKED.0 } else { 0 })),
    };
    door
}
/// A non-player mover at the fixture source, with the player moved away.
fn npc(world: &mut WorldState, body: i32) -> ObjectRef {
    let player = world.player;
    world.get_mut(player).unwrap().raw.hex = Some(0);
    world.get_mut(player).unwrap().raw.elevation = 2;
    place(world, critter(body), 20100)
}

#[test]
fn spec007_long_routes_keep_their_first_800_steps_from_the_source() {
    // A one-hex-wide serpentine corridor: legs on even columns 10, 12, ...
    // from row 10 to row 190, joined by one odd-column hex at alternate ends.
    let mut corridor = std::collections::BTreeSet::new();
    for leg in 0..6 {
        let column = 10 + 2 * leg;
        for row in 10..=190 {
            corridor.insert(column + 200 * row);
        }
        let end = if leg % 2 == 0 { 190 } else { 10 };
        corridor.insert(column + 1 + 200 * end);
    }
    let (source, goal) = (10 + 200 * 10, 20 + 200 * 190);
    let route = routing::find_route(
        source,
        goal,
        true,
        true,
        |hex| (!corridor.contains(&hex)).then_some(()),
        |_| false,
    );
    assert_eq!(route.len(), 800);
    let hexes = routing::route_hexes(source, &route);
    assert!(hexes.iter().all(|hex| corridor.contains(hex)));
    assert!(!hexes.contains(&goal));
}

#[test]
fn spec007_fixture_b_routes_around_blockers_doors_and_footprints() {
    let route = |world: &WorldState, actor: ObjectRef, goal: i32, strict: bool| {
        let directions = world.route(actor, 20100, goal, strict);
        (directions.clone(), routing::route_hexes(20100, &directions))
    };
    let mut clear = world();
    clear.combat = true;
    let player = clear.player;
    assert_eq!(
        route(&clear, player, 20298, true),
        (vec![1, 1], vec![20100, 20299, 20298])
    );
    // Wall: combat takes the low-slot tie, not the lexicographic NE route.
    let mut walled = world();
    let player = walled.player;
    wall(&mut walled, 20299);
    walled.combat = true;
    assert_eq!(
        route(&walled, player, 20298, true),
        (vec![2, 1, 0], vec![20100, 20300, 20499, 20298])
    );
    // Outside combat, initial incoming direction 0 and turn surcharges select NE.
    walled.combat = false;
    assert_eq!(
        route(&walled, player, 20298, true),
        (vec![0, 1, 2], vec![20100, 20099, 20098, 20298])
    );
    // Doors: the player never auto-opens; biped/robotic NPCs open unlocked doors.
    for (body, locked, npc_actor, expected) in [
        (0, false, false, vec![2, 1, 0]),
        (0, false, true, vec![1, 1]),
        (2, false, true, vec![1, 1]),
        (0, true, true, vec![2, 1, 0]),
        (1, false, true, vec![2, 1, 0]),
    ] {
        let mut doors = world();
        doors.combat = true;
        door(&mut doors, 20299, locked);
        let actor = if npc_actor {
            npc(&mut doors, body)
        } else {
            doors.player
        };
        assert_eq!(
            route(&doors, actor, 20298, true).0,
            expected,
            "body {body} locked {locked} npc {npc_actor}"
        );
    }
    // A strict blocked goal is rejected before search; permissive is not.
    let mut goal = world();
    let player = goal.player;
    goal.combat = true;
    wall(&mut goal, 20298);
    assert_eq!(route(&goal, player, 20298, true).0, Vec::<u8>::new());
    assert_eq!(
        route(&goal, player, 20298, false),
        (vec![1, 1], vec![20100, 20299, 20298])
    );
    // A multihex obstacle's footprint grows by one hex; no mover dilation.
    for (combat, mover_multihex, expected, hexes) in [
        (
            true,
            false,
            vec![2, 2, 1, 1, 0, 0],
            vec![20100, 20300, 20500, 20699, 20698, 20697, 20496],
        ),
        (
            false,
            false,
            vec![0, 0, 1, 1, 2, 2],
            vec![20100, 20099, 19898, 20097, 20096, 20296, 20496],
        ),
        (
            true,
            true,
            vec![2, 2, 1, 1, 0, 0],
            vec![20100, 20300, 20500, 20699, 20698, 20697, 20496],
        ),
    ] {
        let mut footprint = world();
        let player = footprint.player;
        footprint.combat = combat;
        let obstacle = place(&mut footprint, critter(0), 20298);
        footprint.get_mut(obstacle).unwrap().raw.flags = BasicFlags(BasicFlags::MULTIHEX.0);
        if mover_multihex {
            footprint.get_mut(player).unwrap().raw.flags = BasicFlags(BasicFlags::MULTIHEX.0);
        }
        assert_eq!(route(&footprint, player, 20496, true), (expected, hexes));
    }
    // The directional helper never leaves the border source 0.
    let border = world();
    assert_eq!(border.route(border.player, 0, 1, true), Vec::<u8>::new());
    // Already at the goal is also an empty route.
    assert_eq!(
        border.route(border.player, 20100, 20100, true),
        Vec::<u8>::new()
    );
}

#[test]
fn spec007_fixture_b_controlled_rays_los_and_cover() {
    let setup = || {
        let mut world = world();
        let defender = place(&mut world, critter(0), 20298);
        world.get_mut(defender).unwrap().raw.flags = BasicFlags(0);
        (world, defender)
    };
    let (mut world, defender) = setup();
    let player = world.player;
    let between = place(&mut world, critter(0), 20299);
    world.get_mut(between).unwrap().raw.flags = BasicFlags(0);
    // Stride-16 LOS (ignored reference explicitly null) reports the
    // intervening critter, not the defender.
    assert_eq!(
        world.ray(player, 20100, 20298, 16, None).obstacle,
        Some(between)
    );
    assert!(!world.can_see(player, defender));
    // Stride-32 shooting resumes through that critter: one cover body.
    assert_eq!(
        world.shot(player, defender),
        routing::Shot {
            hard_obstruction: false,
            cover: 1
        }
    );
    // An ordinary wall is a hard obstruction.
    let (mut world, defender) = setup();
    let blocking = wall(&mut world, 20299);
    assert!(world.shot(player, defender).hard_obstruction);
    // Projectile-through: stride 32 skips it, stride 16 still reports it.
    world.get_mut(blocking).unwrap().raw.flags = BasicFlags(BasicFlags::PROJECTILE_THROUGH.0);
    assert_eq!(
        world.shot(player, defender),
        routing::Shot {
            hard_obstruction: false,
            cover: 0
        }
    );
    assert_eq!(
        world.ray(player, 20100, 20298, 16, None).obstacle,
        Some(blocking)
    );
    // A nonblocking defender is never reported, so LOS is false despite a
    // clear line and perception.
    let (mut world, defender) = setup();
    assert!(world.can_see(player, defender));
    world.get_mut(defender).unwrap().raw.flags = BasicFlags(BasicFlags::NONBLOCKING.0);
    assert!(world.can_perceive(player, defender));
    assert!(!world.can_see(player, defender));
}

/// No resources, though every art identifier counts as existing, so that
/// attack sequences register (SPEC-007 §3.10.1).
struct NoAssets;
impl crate::assets::WorldAssets for NoAssets {
    fn prototype(&self, _pid: u32) -> Result<Arc<Prototype>, crate::WorldError> {
        Err(crate::WorldError::Invalid("no prototypes".into()))
    }
    fn art_exists(&self, _fid: u32) -> bool {
        true
    }
    fn program(&self, _index: i32) -> Result<(String, vault_vm::IntProgram), crate::WorldError> {
        Err(crate::WorldError::Invalid("no programs".into()))
    }
    fn message(&self, _list: i32, _id: i32) -> Option<vault_formats::msg::Message> {
        None
    }
}

fn set_results(world: &mut WorldState, object: ObjectRef, results: u32, ap: i32) {
    if let Some(vault_formats::map::ObjectState::Critter(state)) =
        world.get_mut(object).map(|o| &mut o.raw.state)
    {
        state.results = vault_formats::pro::CombatFlags(results);
        state.ap = ap;
    }
}
fn ap(world: &WorldState, object: ObjectRef) -> i32 {
    match &world.get(object).unwrap().raw.state {
        vault_formats::map::ObjectState::Critter(state) => state.ap,
        _ => unreachable!(),
    }
}

#[test]
fn spec007_move_to_object_shortening_doors_replans_and_ap() {
    use crate::{StepOutcome, WorldHost};
    // Move-to-object hides the target while planning, drops one step, and
    // saves a replan destination stepped forward past the target.
    let mut state = world();
    let target = place(&mut state, critter(0), 20298);
    let player = state.player;
    let mut host = WorldHost::new(state, NoAssets);
    let movement = host.plan_to_object(player, target).unwrap();
    assert_eq!(
        (movement.directions.clone(), movement.destination),
        (vec![1], 20497)
    );
    assert_eq!(
        host.world.get(target).unwrap().raw.flags.0 & 1,
        0,
        "hidden state restored"
    );
    // Only the mover's multihex flag doubles the subtraction.
    host.world.get_mut(player).unwrap().raw.flags.0 |= BasicFlags::MULTIHEX.0;
    let movement = host.plan_to_object(player, target).unwrap();
    assert_eq!(
        (movement.directions.clone(), movement.destination),
        (vec![], 20496)
    );
    host.world.get_mut(player).unwrap().raw.flags.0 &= !BasicFlags::MULTIHEX.0;

    // A biped NPC opens an unlocked door on its route and moves into it.
    let mut state = world();
    let gate = door(&mut state, 20299, false);
    let mover = npc(&mut state, 0);
    let mut host = WorldHost::new(state, NoAssets);
    let mut movement = host.plan_to_tile(mover, 20298, 10).unwrap();
    assert_eq!(movement.directions, [1, 1]);
    assert_eq!(
        host.step_movement(&mut movement).unwrap(),
        StepOutcome::Moved
    );
    assert_eq!(host.world.get(mover).unwrap().raw.hex, Some(20299));
    let opened = host.world.get(gate).unwrap();
    assert_eq!(opened.raw.frame, 1);
    assert!(opened.lock().unwrap().contains(LockFlags::OPEN));
    assert_eq!(
        host.step_movement(&mut movement).unwrap(),
        StepOutcome::Moved
    );
    assert_eq!(
        host.step_movement(&mut movement).unwrap(),
        StepOutcome::Finished
    );

    // A newly blocking wall rebuilds the route from the current hex with a
    // strict goal and resets facing; a blocked destination stops movement.
    let mut state = world();
    let player = state.player;
    state.combat = true;
    let mut host = WorldHost::new(state, NoAssets);
    let mut movement = host.plan_to_tile(player, 20298, 10).unwrap();
    assert_eq!(movement.directions, [1, 1]);
    wall(&mut host.world, 20299);
    assert_eq!(
        host.step_movement(&mut movement).unwrap(),
        StepOutcome::Replanned
    );
    assert_eq!(movement.directions, [2, 1, 0]);
    assert_eq!(host.world.get(player).unwrap().raw.orientation, 2);
    wall(&mut host.world, 20298);
    let mut movement = host.plan_to_tile(player, 20298, 10).unwrap();
    // The occupied destination removed the final step.
    assert_eq!(
        (movement.directions.len(), movement.destination),
        (2, step_back(20298, 0))
    );
    // The saved destination 20297 is open, so a new blocker on the next hex
    // still rebuilds; once that destination is blocked too, the strict-goal
    // rebuild fails and movement ends.
    wall(&mut host.world, 20300);
    let mut retry = movement.clone();
    assert_eq!(
        host.step_movement(&mut retry).unwrap(),
        StepOutcome::Replanned
    );
    wall(&mut host.world, 20297);
    assert_eq!(
        host.step_movement(&mut movement).unwrap(),
        StepOutcome::Stopped
    );

    // Combat AP per committed hex: 1, 4 with one crippled leg, 8 with both;
    // the player's free-movement pool pays first; saturation at zero.
    let mut state = world();
    let player = state.player;
    state.combat = true;
    let mut host = WorldHost::new(state, NoAssets);
    for (results, free, start, expected_ap, expected_free) in [
        (0, 0, 8, 7, 0),
        (effect::CRIPPLED_LEFT_LEG, 0, 8, 4, 0),
        (effect::CRIPPLED_LEGS, 0, 8, 0, 0),
        (effect::CRIPPLED_LEGS, 0, 3, 0, 0),
        (effect::CRIPPLED_LEFT_LEG, 2, 8, 6, 0),
        (0, 4, 8, 8, 3),
    ] {
        set_results(&mut host.world, player, results, start);
        host.world.free_movement = free;
        host.charge_movement(player);
        assert_eq!(
            (ap(&host.world, player), host.world.free_movement),
            (expected_ap, expected_free)
        );
    }
    host.world.combat = false;
    host.charge_movement(player);
    assert_eq!(ap(&host.world, player), 8, "no AP debit outside combat");
}

/// The hex reached by stepping forward from `hex` along `direction`.
fn step_back(hex: i32, direction: u8) -> i32 {
    routing::step(hex, direction)
}

use crate::combat::{Attack, PlayerModifiers, WeaponProfile, attack_mode, knockback_displacement};
use std::collections::VecDeque;

/// Supplies exact random outcomes in order, checking each is in range.
struct Outcomes(VecDeque<i32>);
impl Outcomes {
    fn new(values: &[i32]) -> Self {
        Self(values.iter().copied().collect())
    }
}
impl crate::RandomSource for Outcomes {
    fn range(&mut self, first: i32, last: i32) -> Result<i32, crate::WorldError> {
        let value = self.0.pop_front().expect("scripted random outcome");
        assert!(
            (first..=last).contains(&value),
            "{value} outside {first}..={last}"
        );
        Ok(value)
    }
}

/// SPEC-007 §4.3: aimed right-leg rifle shot by the player at a man-category
/// NPC fifteen hexes east, one intervening critter, normal difficulty.
fn fixture_c() -> Attack {
    let mut thresholds = [0; 7];
    thresholds[0] = 4;
    let mut resistances = [0; 7];
    resistances[0] = 25;
    Attack {
        attacker_player: true,
        attacker_pid: 0x01000000,
        attacker_team: 0,
        player_team: 0,
        skill: 80,
        perception: 6,
        strength: 5,
        luck: 5,
        critical_chance: 5,
        better_criticals: 20,
        melee_damage: 1,
        blind: false,
        player: PlayerModifiers::default(),
        jinxed: false,
        weapon: Some(WeaponProfile {
            pid: 0x00000009,
            mode: attack_mode::SINGLE,
            damage: [8, 20],
            damage_type: 0,
            range: 20,
            min_strength: 4,
            ap_cost: 5,
            failure_table: 0,
            burst: 0,
            two_handed: false,
            big_gun: false,
            perk: None,
            rounds: 6,
            capacity: 6,
        }),
        hit_mode: critical::hit_mode::RIGHT_PRIMARY,
        location: location::RIGHT_LEG,
        aimed: true,
        distance: 15,
        cover: 1,
        defender_player: false,
        defender_pid: 0x01000002,
        kill_category: 0,
        armor_class: 12,
        thresholds,
        resistances,
        attacker_thresholds: [0; 7],
        attacker_resistances: [0; 7],
        defender_special: [5; 7],
        defender_multihex: false,
        defender_prone: false,
        defender_light: 60000,
        defender_hp: 60,
        ticks: 864000,
        combat_difficulty: 1,
        accuracy_bonus: 0,
        tables: None,
    }
}

#[test]
fn spec007_fixture_c_hit_chance_cost_and_normal_damage() {
    let attack = fixture_c();
    // Range -12, cover -10, AC -12, leg -20, bright light 0: 26 (the ignored
    // ammunition AC modifier would have produced 36).
    assert_eq!(attack.hit_chance(true).unwrap(), 26);
    assert_eq!(attack.ap_cost(), 6);
    assert_eq!(attack.range(), 20);
    // D100 20 (margin 6), upgrade D100 50 > 25, base damage 18:
    // 18 - DT 4 = 14; 14 - trunc(14 * 25 / 100) = 11.
    let resolution = attack.resolve(&mut Outcomes::new(&[20, 50, 18])).unwrap();
    assert_eq!(
        (
            resolution.code,
            resolution.damage,
            resolution.rounds,
            resolution.ammo
        ),
        (2, 11, 1, 1)
    );
    assert_eq!(resolution.defender_flags, effect::HIT);
    assert_eq!((resolution.knockback, resolution.multiplier), (0, 2));
}

#[test]
fn spec007_fixture_c_critical_variant_and_boundaries() {
    let attack = fixture_c();
    // Upgrade D100 1 <= 25; severity 80 + 20 = 100 -> tier 4, entry
    // 4/80A/2/0/1/5025/5026; saving D10 8 > Endurance 5 fails; base 18.
    let resolution = attack
        .resolve(&mut Outcomes::new(&[20, 1, 80, 8, 18]))
        .unwrap();
    assert_eq!(resolution.code, 3);
    assert_eq!(
        resolution.defender_flags,
        effect::HIT | effect::CRITICAL | 0x80b
    );
    assert_eq!(resolution.defender_flags & effect::PERSISTENT, 0x0b);
    assert_eq!(resolution.message, Some(5026));
    // Armor bypass removes DT and DR: 18 * 4 / 2 = 36. Knockdown still gives
    // a rifle no knockback.
    assert_eq!(
        (
            resolution.damage,
            resolution.multiplier,
            resolution.knockback
        ),
        (36, 4, 0)
    );
    assert_eq!(crate::combat::knockout_delay(5), 200);
    // A passed save keeps the ordinary mask and message.
    let saved = attack
        .resolve(&mut Outcomes::new(&[20, 1, 80, 5, 18]))
        .unwrap();
    assert_eq!(saved.defender_flags & effect::PERSISTENT, 0x0a);
    assert_eq!(saved.message, Some(5025));

    // Critical failure: table 2, Luck 7, failure draw 60 -> column 1, mask
    // 0x20000; ranged failure schedules the whole six-round magazine.
    let mut failing = fixture_c();
    failing.luck = 7;
    failing.weapon.as_mut().unwrap().failure_table = 2;
    let resolution = failing.resolve(&mut Outcomes::new(&[100, 1, 60])).unwrap();
    assert_eq!(resolution.code, 0);
    assert_eq!(
        resolution.attacker_flags,
        effect::CRITICAL | effect::LOSE_AMMO
    );
    assert_eq!((resolution.ammo, resolution.damage), (6, 0));

    // Melee damage 35 gives computed knockback 3; under the §3.8 safe rule
    // a blocker at distance j permits j - 1 (reference values 0 and 1).
    let mut melee = fixture_c();
    melee.weapon.as_mut().unwrap().mode = attack_mode::SWING;
    assert_eq!(melee.knockback(35), 3);
    assert_eq!(knockback_displacement(3, |_| false), 3);
    assert_eq!(knockback_displacement(3, |distance| distance == 2), 1);
    assert_eq!(knockback_displacement(3, |distance| distance == 3), 2);
    assert_eq!(knockback_displacement(3, |distance| distance == 1), 0);
    assert_eq!(knockback_displacement(0, |_| true), 0);

    // Before tick 864000 no upgrade draw is made, but Sniper still promotes
    // a ranged success on D10 <= Luck.
    let mut early = fixture_c();
    early.ticks = 863999;
    let resolution = early.resolve(&mut Outcomes::new(&[20, 18])).unwrap();
    assert_eq!((resolution.code, resolution.damage), (2, 11));
    early.player.sniper = true;
    let resolution = early
        .resolve(&mut Outcomes::new(&[20, 5, 80, 8, 18]))
        .unwrap();
    assert_eq!(resolution.code, 3);
}

use crate::combat::{DeathChoice, blood_variant, corpse_flattens, final_animation_drops_inventory};

#[test]
fn spec007_death_animation_choice_tree_and_fallbacks() {
    let base = DeathChoice {
        violence: 3,
        attacker_player: true,
        attacker_pid: 0x01000000,
        attacker_art: 0x01000000,
        victim_pid: 0x01000002,
        orientations: [1, 4],
        damage_type: 0,
        mode: attack_mode::BURST,
        damage: 10,
        bloody_mess: false,
        attack_animation: 45,
    };
    let all = |_: u8| true;
    assert!(base.from_front());
    assert!(
        !DeathChoice {
            orientations: [1, 2],
            ..base
        }
        .from_front()
    );
    assert!(
        !DeathChoice {
            orientations: [0, 5],
            ..base
        }
        .from_front()
    );
    // Low damage, front: plain fall-back; rear: fall-front.
    assert_eq!(base.animation(all), 20);
    assert_eq!(
        DeathChoice {
            orientations: [2, 2],
            ..base
        }
        .animation(all),
        21
    );
    // Damage thresholds pick the moderate (>=15) and maximum (>=45) columns.
    assert_eq!(DeathChoice { damage: 15, ..base }.animation(all), 26);
    assert_eq!(DeathChoice { damage: 45, ..base }.animation(all), 25);
    assert_eq!(
        DeathChoice {
            damage: 45,
            damage_type: 2,
            ..base
        }
        .animation(all),
        33
    );
    assert_eq!(
        DeathChoice {
            damage: 45,
            violence: 2,
            ..base
        }
        .animation(all),
        26
    );
    assert_eq!(
        DeathChoice {
            damage: 45,
            violence: 1,
            ..base
        }
        .animation(all),
        20
    );
    // Missing maximum art falls back to the moderate column.
    assert_eq!(
        DeathChoice { damage: 45, ..base }.animation(|code| code != 25),
        26
    );
    // Bloody Mess (player only) at maximum violence: close attacks request 23.
    assert_eq!(
        DeathChoice {
            bloody_mess: true,
            mode: attack_mode::SWING,
            ..base
        }
        .animation(all),
        23
    );
    assert_eq!(
        DeathChoice {
            bloody_mess: true,
            mode: attack_mode::SWING,
            attacker_player: false,
            ..base
        }
        .animation(all),
        20
    );
    // Single normal-damage fire with damage >= 45 also requests 23.
    assert_eq!(
        DeathChoice {
            mode: attack_mode::SINGLE,
            damage: 45,
            ..base
        }
        .animation(all),
        23
    );
    // Special victims request 31 under minimum violence 2.
    assert_eq!(
        DeathChoice {
            victim_pid: 0x01000031,
            violence: 2,
            ..base
        }
        .animation(all),
        31
    );
    assert_eq!(
        DeathChoice {
            victim_pid: 0x01000031,
            violence: 1,
            ..base
        }
        .animation(all),
        20
    );
    // Special attackers, under maximum-violence validation.
    assert_eq!(
        DeathChoice {
            attacker_art: 0x0500000a,
            ..base
        }
        .animation(all),
        31
    );
    assert_eq!(
        DeathChoice {
            attacker_pid: 0x020001eb,
            ..base
        }
        .animation(all),
        30
    );
    assert_eq!(
        DeathChoice {
            attacker_art: 0x020001f5,
            ..base
        }
        .animation(all),
        45
    );
    // Safe-rule rear fallback: fall-front art present returns 21, absent 20.
    let rear = DeathChoice {
        orientations: [3, 3],
        damage: 15,
        ..base
    };
    assert_eq!(rear.animation(|code| code == 21), 21);
    assert_eq!(rear.animation(|_| false), 20);
    // A front fallback is always 20, whether or not its art exists.
    assert_eq!(DeathChoice { damage: 15, ..base }.animation(|_| false), 20);
    // Blood presentation and corpse rules.
    assert_eq!(blood_variant(20, 2, all), 34);
    assert_eq!(blood_variant(21, 2, all), 35);
    assert_eq!(blood_variant(20, 0, all), 20);
    assert_eq!(blood_variant(21, 2, |code| code != 35), 21);
    assert!(corpse_flattens(0x01000002) && !corpse_flattens(0x01000008));
    assert!(final_animation_drops_inventory(31, 0x01000002));
    assert!(!final_animation_drops_inventory(31, 0x01000017));
    assert!(!final_animation_drops_inventory(25, 0x01000002));
}

/// Fixture C's world: rifle-armed player, intervening critter, defender.
fn fixture_c_world() -> (crate::WorldHost<NoAssets>, ObjectRef) {
    let mut sheet = CharacterSheet::default();
    sheet.base[1] = 6;
    sheet.bonus[16] = 20;
    sheet.invested[0] = 40;
    sheet.recompute(Default::default());
    let mut state = WorldState::new(sheet, 1).unwrap();
    let player = state.player;
    state.get_mut(player).unwrap().raw.orientation = 1;
    set_results(&mut state, player, 0, 8);
    let mut rifle = world_fixture::prototype(0, 3);
    world_fixture::word(&mut rifle, 0x18, 0x06);
    for (offset, value) in [
        (61, 8),
        (65, 20),
        (73, 20),
        (85, 4),
        (89, 5),
        (101, -1),
        (117, 6),
    ] {
        world_fixture::word(&mut rifle, offset, value);
    }
    let rifle = place(&mut state, Arc::new(Prototype::parse(&rifle).unwrap()), 1);
    state.add_item(player, rifle, 1, true).unwrap();
    state
        .equip(player, rifle, BasicFlags::RIGHT_HAND.0)
        .unwrap();
    let mut man = world_fixture::prototype(1, 0);
    for (stat, value) in [
        (0, 5),
        (1, 5),
        (2, 5),
        (3, 5),
        (4, 5),
        (5, 5),
        (6, 5),
        (7, 60),
        (9, 10),
        (17, 4),
        (24, 25),
    ] {
        world_fixture::word(&mut man, 48 + 4 * stat, value);
    }
    let man = Arc::new(Prototype::parse(&man).unwrap());
    let defender = place(&mut state, man.clone(), 21685);
    let between = place(&mut state, man, 20892);
    for object in [defender, between] {
        state.get_mut(object).unwrap().raw.flags = BasicFlags(0);
    }
    state.get_mut(defender).unwrap().raw.orientation = 4;
    set_results(&mut state, defender, 0, 2);
    state.ambient = 50000;
    state.rebuild_lighting();
    state.tile_light[0][21685] = 60000;
    state.combat = true;
    state.combat_actor = player;
    state.clock.set(864000);
    (crate::WorldHost::new(state, NoAssets), defender)
}

fn field_bytes(host: &crate::WorldHost<NoAssets>, defender: ObjectRef) -> Vec<u8> {
    let world = &host.world;
    let player = world.player;
    let rifle = world
        .wielded(player, critical::hit_mode::RIGHT_PRIMARY)
        .unwrap();
    let rounds = match &world.get(rifle).unwrap().raw.state {
        vault_formats::map::ObjectState::Other {
            detail: ObjectDetail::Weapon { rounds, .. },
            ..
        } => *rounds,
        _ => unreachable!(),
    };
    let state = world.critter_state(defender).unwrap();
    [
        ap(world, player),
        rounds,
        state.hp,
        (state.results.0 & effect::PERSISTENT) as i32,
    ]
    .iter()
    .flat_map(|value| value.to_be_bytes())
    .collect()
}

#[test]
fn spec007_fixture_c_world_profile_preflight_and_application() {
    let (mut host, defender) = fixture_c_world();
    let player = host.world.player;
    assert_eq!(host.world.object_distance(player, defender), 15);
    let attack = host
        .world
        .attack_profile(
            player,
            defender,
            critical::hit_mode::RIGHT_PRIMARY,
            location::RIGHT_LEG,
            true,
        )
        .unwrap();
    assert_eq!(
        (
            attack.skill,
            attack.perception,
            attack.strength,
            attack.critical_chance,
            attack.better_criticals
        ),
        (80, 6, 5, 5, 20)
    );
    assert_eq!(
        (attack.cover, attack.armor_class, attack.defender_light),
        (1, 12, 60000)
    );
    assert_eq!(attack.hit_chance(true).unwrap(), 26);
    assert_eq!(host.world.preflight(player, defender, &attack), Ok(()));
    let resolution = attack.resolve(&mut Outcomes::new(&[20, 50, 18])).unwrap();
    host.apply_attack(player, defender, &attack, &resolution)
        .unwrap();
    assert_eq!(
        field_bytes(&host, defender),
        [0, 0, 0, 2, 0, 0, 0, 5, 0, 0, 0, 0x31, 0, 0, 0, 0]
    );

    let (mut host, defender) = fixture_c_world();
    let attack = host
        .world
        .attack_profile(
            player,
            defender,
            critical::hit_mode::RIGHT_PRIMARY,
            location::RIGHT_LEG,
            true,
        )
        .unwrap();
    let resolution = attack
        .resolve(&mut Outcomes::new(&[20, 1, 80, 8, 18]))
        .unwrap();
    host.apply_attack(player, defender, &attack, &resolution)
        .unwrap();
    assert_eq!(
        field_bytes(&host, defender),
        [0, 0, 0, 2, 0, 0, 0, 5, 0, 0, 0, 0x18, 0, 0, 0, 0x0b]
    );
    // Knockout wake-up is queued 10 * (35 - 3 * 5) = 200 ticks later; no
    // knockback displacement for a rifle hit.
    let wake = host
        .world
        .queue
        .events()
        .find(|e| e.object == defender)
        .unwrap();
    assert_eq!(
        (wake.due, wake.kind),
        (864200, crate::queue::EventKind::Knockout)
    );
    assert_eq!(host.world.get(defender).unwrap().raw.hex, Some(21685));
    host.world.clock.set(864200);
    host.process_queue().unwrap();
    assert_eq!(
        host.world.critter_state(defender).unwrap().results.0 & 0x1,
        0
    );

    // Preflight order: AP, range, empty magazine, hard obstruction, dead.
    let (mut host, defender) = fixture_c_world();
    let attack = |host: &crate::WorldHost<NoAssets>| {
        host.world
            .attack_profile(
                player,
                defender,
                critical::hit_mode::RIGHT_PRIMARY,
                location::RIGHT_LEG,
                true,
            )
            .unwrap()
    };
    set_results(&mut host.world, player, 0, 5);
    assert_eq!(
        host.world.preflight(player, defender, &attack(&host)),
        Err(crate::Refusal::ActionPoints)
    );
    set_results(&mut host.world, player, effect::CRIPPLED_ARMS, 8);
    assert_eq!(
        host.world.preflight(player, defender, &attack(&host)),
        Err(crate::Refusal::CrippledArms)
    );
    set_results(&mut host.world, player, 0, 8);
    let mut far = attack(&host);
    far.distance = 21;
    assert_eq!(
        host.world.preflight(player, defender, &far),
        Err(crate::Refusal::Range)
    );
    let mut empty = attack(&host);
    empty.weapon.as_mut().unwrap().rounds = 0;
    assert_eq!(
        host.world.preflight(player, defender, &empty),
        Err(crate::Refusal::NoAmmunition)
    );
    // A stacked wall would be masked by the critter (the first eligible
    // object on a hex hides the others), so replace the critter with a wall.
    let between = host.world.occupancy(0)[&20892][0];
    host.world.erase(between);
    wall(&mut host.world, 20892);
    let blocked = attack(&host);
    assert!(host.world.shot(player, defender).hard_obstruction);
    assert_eq!(
        host.world.preflight(player, defender, &blocked),
        Err(crate::Refusal::Obstructed)
    );
    set_results(&mut host.world, defender, effect::DEAD, 0);
    assert_eq!(
        host.world.preflight(player, defender, &blocked),
        Err(crate::Refusal::DeadDefender)
    );
}

/// Always returns the lowest bound: every D100 hits; damage is the minimum.
struct Lowest;
impl crate::RandomSource for Lowest {
    fn range(&mut self, first: i32, _last: i32) -> Result<i32, crate::WorldError> {
        Ok(first)
    }
}

/// Player with a 10-round spray weapon; defender 5 east; extra critter 2 east.
fn spray_world(mode: u8) -> (crate::WorldHost<NoAssets>, ObjectRef, ObjectRef) {
    let mut sheet = CharacterSheet::default();
    sheet.invested[0] = 200;
    sheet.recompute(Default::default());
    let mut state = WorldState::new(sheet, 1).unwrap();
    let player = state.player;
    state.get_mut(player).unwrap().raw.orientation = 1;
    set_results(&mut state, player, 0, 10);
    let mut gun = world_fixture::prototype(0, 3);
    world_fixture::word(&mut gun, 0x18, i32::from(mode));
    for (offset, value) in [
        (61, 5),
        (65, 5),
        (73, 20),
        (89, 5),
        (101, -1),
        (105, 10),
        (117, 10),
    ] {
        world_fixture::word(&mut gun, offset, value);
    }
    let gun = place(&mut state, Arc::new(Prototype::parse(&gun).unwrap()), 1);
    state.add_item(player, gun, 1, true).unwrap();
    state.equip(player, gun, BasicFlags::RIGHT_HAND.0).unwrap();
    let mut body = world_fixture::prototype(1, 0);
    for (stat, value) in [
        (0, 5),
        (1, 5),
        (2, 5),
        (3, 5),
        (4, 5),
        (5, 5),
        (6, 5),
        (7, 100),
    ] {
        world_fixture::word(&mut body, 48 + 4 * stat, value);
    }
    let body = Arc::new(Prototype::parse(&body).unwrap());
    let defender = place(&mut state, body.clone(), 20695);
    let extra = place(&mut state, body, 20298);
    for object in [defender, extra] {
        state.get_mut(object).unwrap().raw.flags = BasicFlags(0);
    }
    state.combat = true;
    state.combat_actor = player;
    (crate::WorldHost::new(state, NoAssets), defender, extra)
}

#[test]
fn spec007_spray_budgets_rays_extras_and_continuous_fire() {
    use crate::{combat::attack_mode, spray_budgets};
    assert_eq!(spray_budgets(10, false), [3, 3, 4, 1]);
    assert_eq!(spray_budgets(6, false), [2, 2, 2, 1]);
    // Direct count raised to 1 takes one from the center budget.
    assert_eq!(spray_budgets(2, false), [0, 0, 1, 1]);
    assert_eq!(spray_budgets(1, false), [0, 0, 0, 1]);
    assert_eq!(spray_budgets(9, true), [0, 1, 1, 1]);

    let profile = |host: &crate::WorldHost<NoAssets>, defender| {
        host.world
            .attack_profile(
                host.world.player,
                defender,
                critical::hit_mode::RIGHT_PRIMARY,
                location::TORSO,
                false,
            )
            .unwrap()
    };
    // Burst: center ray budget 3 - 1 direct hit = 2, left 3, right 4; the
    // extra critter on all three lines absorbs 9 rounds, the target 1.
    let (host, defender, extra) = spray_world(attack_mode::BURST);
    let attack = profile(&host, defender);
    let spray = host
        .resolve_spray(host.world.player, defender, &attack, &mut Lowest)
        .unwrap();
    assert_eq!(
        (spray.main.code, spray.main.rounds, spray.main.ammo),
        (2, 1, 10)
    );
    assert_eq!(spray.main.damage, 5);
    assert_eq!(spray.extras.len(), 1);
    let (object, _, outcome) = &spray.extras[0];
    assert_eq!((*object, outcome.rounds, outcome.damage), (extra, 9, 45));
    // Application: AP debit, whole scheduled magazine, both targets' damage.
    let (mut host, defender, extra) = spray_world(attack_mode::BURST);
    let attack = profile(&host, defender);
    let spray = host
        .resolve_spray(host.world.player, defender, &attack, &mut Lowest)
        .unwrap();
    host.apply_spray(host.world.player, defender, &attack, &spray)
        .unwrap();
    assert_eq!(host.world.hit_points(defender), Some(95));
    assert_eq!(host.world.hit_points(extra), Some(55));
    assert_eq!(ap(&host.world, host.world.player), 5);

    // Continuous: budgets reset to 1 per critter, later duplicate extras are
    // ignored, and ray hits on the target do not count.
    let (host, defender, extra) = spray_world(attack_mode::CONTINUOUS);
    let attack = profile(&host, defender);
    let spray = host
        .resolve_spray(host.world.player, defender, &attack, &mut Lowest)
        .unwrap();
    assert_eq!((spray.main.rounds, spray.main.damage), (1, 5));
    let (object, _, outcome) = &spray.extras[0];
    assert_eq!((*object, outcome.rounds), (extra, 1));

    // A hard non-critter obstruction stops every ray at once.
    let (mut host, defender, _) = spray_world(attack_mode::BURST);
    wall(&mut host.world, 20299);
    let attack = profile(&host, defender);
    let spray = host
        .resolve_spray(host.world.player, defender, &attack, &mut Lowest)
        .unwrap();
    assert_eq!((spray.main.rounds, spray.extras.len()), (1, 0));

    // A critical spray: extras keep the normal numerator 2 (computed before
    // the main augmentation); the target uses its critical multiplier 3.
    let (mut host, defender, _) = spray_world(attack_mode::BURST);
    host.world.clock.set(864000);
    let attack = profile(&host, defender);
    let spray = host
        .resolve_spray(host.world.player, defender, &attack, &mut Lowest)
        .unwrap();
    assert_eq!((spray.main.code, spray.main.multiplier), (3, 3));
    assert_eq!(spray.main.damage, 7);
    assert_eq!(spray.extras[0].2.damage, 45);
}

/// Scripted draws, then the lowest (or highest) bound; records every range.
struct Traced {
    script: std::collections::VecDeque<i32>,
    high: bool,
    ranges: Vec<(i32, i32)>,
}
impl Traced {
    fn new(script: &[i32], high: bool) -> Self {
        Self {
            script: script.iter().copied().collect(),
            high,
            ranges: Vec::new(),
        }
    }
}
impl crate::RandomSource for Traced {
    fn range(&mut self, first: i32, last: i32) -> Result<i32, crate::WorldError> {
        self.ranges.push((first, last));
        Ok(self
            .script
            .pop_front()
            .unwrap_or(if self.high { last } else { first }))
    }
}

#[test]
fn spec007_spray_stage_a_extra_damage_order_and_post_processing() {
    use crate::combat::attack_mode;
    let (host, defender, extra) = spray_world(attack_mode::BURST);
    let mut attack = host
        .world
        .attack_profile(
            host.world.player,
            defender,
            critical::hit_mode::RIGHT_PRIMARY,
            location::TORSO,
            false,
        )
        .unwrap();
    attack.attacker_player = true;
    attack.player.sniper = true;
    attack.luck = 5;
    // Spray roll succeeds, the one direct trial misses, and the in-spray
    // Stage A pass draws once for the extra critter in the line and does not
    // select it. Every later D100 hits.
    let mut rng = Traced::new(&[1, 100, 100], false);
    let spray = host
        .resolve_spray(host.world.player, defender, &attack, &mut rng)
        .unwrap();
    // Roll, direct trial, Stage A, then the center ray's trials at the
    // extra (hits plus one) and at once its three damage draws.
    assert_eq!(&rng.ranges[..3], &[(1, 100); 3]);
    assert_eq!(&rng.ranges[3..7], &[(1, 100); 4]);
    assert_eq!(&rng.ranges[7..10], &[(5, 5); 3]);
    // Only the extra was hit, so the spray stays a success, and the Sniper
    // D10 is the final draw; under the safe rule the target gets no hit bit
    // and no critical processing.
    assert_eq!(rng.ranges.last(), Some(&(1, 10)));
    assert_eq!(spray.main.code, 3);
    assert_eq!(spray.main.defender_flags & effect::HIT, 0);
    assert_eq!((spray.extras[0].0, spray.extras[0].2.rounds), (extra, 10));

    // Every D100 misses: the final ordinary failure draws the Jinxed coin
    // once, and a heads makes it a critical failure.
    let mut jinxed = attack.clone();
    jinxed.player.sniper = false;
    jinxed.jinxed = true;
    let mut rng = Traced::new(&[], true);
    let spray = host
        .resolve_spray(host.world.player, defender, &jinxed, &mut rng)
        .unwrap();
    assert_eq!(
        rng.ranges.iter().filter(|range| **range == (0, 1)).count(),
        1
    );
    assert_eq!(spray.main.code, 0);
    assert!(spray.extras.is_empty());
}

#[test]
fn spec007_fixture_i_spray_draw_order() {
    use crate::{
        combat::attack_mode,
        routing::{extend_line, rotation, step},
    };
    // Shared inputs: a burst with 3 rounds of a 10-round burst count
    // (budgets 0/1/1, one direct trial), spray accuracy 70, Critical Chance
    // 5, Luck 6, no Better Criticals, normal difficulty, first day over,
    // 10-20 normal damage. The extra critter is placed per case.
    let fixture = |sniper: bool, jinxed: bool| {
        let (mut host, defender, extra) = spray_world(attack_mode::BURST);
        host.world.clock.set(864000);
        let mut attack = host
            .world
            .attack_profile(
                host.world.player,
                defender,
                critical::hit_mode::RIGHT_PRIMARY,
                location::TORSO,
                false,
            )
            .unwrap();
        let weapon = attack.weapon.as_mut().unwrap();
        (weapon.rounds, weapon.burst, weapon.damage) = (3, 10, [10, 20]);
        attack.attacker_player = true;
        attack.player.sniper = sniper;
        attack.jinxed = jinxed;
        (attack.luck, attack.critical_chance, attack.better_criticals) = (6, 5, 0);
        attack.ticks = 864000;
        attack.combat_difficulty = 1;
        // The chance is capped at 95, so step the adjustment down to 70.
        for _ in 0..100 {
            attack.accuracy_bonus += 70 - attack.hit_chance(true).unwrap();
        }
        assert_eq!(attack.hit_chance(true).unwrap(), 70);
        (host, defender, extra, attack)
    };
    type Host = crate::WorldHost<NoAssets>;
    fn run(
        host: &Host,
        defender: ObjectRef,
        attack: &crate::combat::Attack,
        draws: &[i32],
    ) -> (crate::Spray, Vec<(i32, i32)>) {
        let mut rng = Traced::new(draws, false);
        let spray = host
            .resolve_spray(host.world.player, defender, attack, &mut rng)
            .unwrap();
        assert!(rng.script.is_empty(), "every listed outcome is drawn");
        (spray, rng.ranges)
    }
    let d100 = (1, 100);

    // I1, Sniper, no critter on any ray: 7 draws.
    let (mut host, defender, extra, attack) = fixture(true, false);
    host.world.erase(extra);
    let (spray, ranges) = run(&host, defender, &attack, &[40, 50, 20, 90, 4, 30, 15]);
    assert_eq!(&ranges[..5], &[d100, d100, d100, d100, (1, 10)]);
    assert_eq!(ranges[5], d100, "the critical tier draw");
    assert_eq!(ranges.last(), Some(&(10, 20)));
    assert_eq!(ranges.len(), 7);
    assert_eq!((spray.main.code, spray.main.rounds), (3, 1));

    // I2, Sniper, critter X on the left ray only: 8 draws, the extra-only
    // outcome (under the safe rule the target takes no hit bit).
    let (mut host, defender, x, attack) = fixture(true, false);
    let from = host.world.get(host.world.player).unwrap().raw.hex.unwrap();
    let to = host.world.get(defender).unwrap().raw.hex.unwrap();
    let range = usize::try_from(attack.range()).unwrap();
    let back = rotation(to, from).unwrap();
    let center = extend_line(from, to, range);
    let right = extend_line(from, step(to, (back + 5) % 6), range);
    let left = extend_line(from, step(to, (back + 1) % 6), range);
    let at = *left
        .iter()
        .find(|hex| !center.contains(hex) && !right.contains(hex))
        .expect("a hex on the left ray only");
    host.world.move_object(x, at, 0, false).unwrap();
    let (spray, ranges) = run(&host, defender, &attack, &[80, 50, 95, 60, 30, 10, 12, 9]);
    assert_eq!(
        ranges,
        [d100, d100, d100, d100, d100, d100, (10, 20), (1, 10)]
    );
    assert_eq!((spray.main.code, spray.main.rounds), (2, 0));
    assert_eq!(spray.main.defender_flags & effect::HIT, 0);
    assert_eq!((spray.extras[0].0, spray.extras[0].2.rounds), (x, 1));

    // I3, Jinxed without Sniper, no critter anywhere: the coin is the
    // fifth and last draw, and 0 keeps the failure.
    let (mut host, defender, extra, attack) = fixture(false, true);
    host.world.erase(extra);
    let (spray, ranges) = run(&host, defender, &attack, &[80, 50, 95, 60, 0]);
    assert_eq!(ranges, [d100, d100, d100, d100, (0, 1)]);
    assert_eq!(spray.main.code, 1);

    // I4, Jinxed: 100 then 2 is a critical failure; the spray returns at
    // once, and 40 is the critical-failure table draw.
    let (host, defender, _, attack) = fixture(false, true);
    let (spray, ranges) = run(&host, defender, &attack, &[100, 2, 40]);
    assert_eq!(&ranges[..2], &[d100, d100]);
    assert!(!ranges[2..].contains(&(0, 1)), "no Jinxed coin");
    assert_eq!(spray.main.code, 0);
    assert!(spray.extras.is_empty());
}

#[test]
fn spec007_aimed_shot_availability_and_grenade_scatter() {
    let attack = fixture_c();
    assert!(attack.aimed_shot_available());
    let mut fast = fixture_c();
    fast.player.fast_shot = true;
    assert!(!fast.aimed_shot_available());
    let mut burst = fixture_c();
    burst.weapon.as_mut().unwrap().mode = attack_mode::BURST;
    assert!(!burst.aimed_shot_available());
    for kind in [2, 5, 6] {
        let mut kinds = fixture_c();
        kinds.weapon.as_mut().unwrap().damage_type = kind;
        assert!(!kinds.aimed_shot_available(), "damage type {kind}");
    }
    // Safe rule: a plasma throw weapon cannot aim (the reference's check
    // never restricts).
    let mut plasma = fixture_c();
    plasma.weapon.as_mut().unwrap().mode = attack_mode::THROW;
    plasma.weapon.as_mut().unwrap().damage_type = 3;
    assert!(!plasma.aimed_shot_available());
    // Availability and cost are separate: forcing aim still adds 1 AP.
    let mut forced = fast.clone();
    forced.location = location::EYES;
    assert_eq!(forced.ap_cost(), 5, "Fast Shot -1, then forced aim +1");

    // Scatter (§3.7.1 Stage B): D(distance / 2) displacement, then the
    // direction draw.
    assert_eq!(
        crate::combat::grenade_scatter(20100, 8, &mut Outcomes::new(&[3, 2])).unwrap(),
        vault_formats::HexGrid::step(20100, 2, 3).unwrap()
    );
    // Half distance 0 is a reversed range: the generator returns its first
    // bound, so the displacement is still 1.
    let mut rng = crate::ParkMiller::seeded(1).unwrap();
    let scattered = crate::combat::grenade_scatter(20100, 1, &mut rng).unwrap();
    assert_eq!(
        vault_formats::HexGrid::lattice_distance(20100, scattered).unwrap(),
        1
    );
}

use crate::combat_state::{CombatAi, CombatProgress};

/// A hostile NPC (team 1) with SPECIAL 5, the given Sequence and max AP.
fn fighter(state: &mut WorldState, hex: i32, sequence: i32, elevation: u8) -> ObjectRef {
    let mut body = world_fixture::prototype(1, 0);
    for (stat, value) in [
        (0, 5),
        (1, 5),
        (2, 5),
        (3, 5),
        (4, 5),
        (5, 5),
        (6, 5),
        (7, 30),
        (8, 7),
        (13, sequence),
    ] {
        world_fixture::word(&mut body, 48 + 4 * stat, value);
    }
    world_fixture::word(&mut body, 40, 1);
    let critter = state
        .spawn(Arc::new(Prototype::parse(&body).unwrap()), hex, elevation)
        .unwrap();
    state.get_mut(critter).unwrap().raw.flags = BasicFlags(0);
    critter
}

/// Scripted AI: fixed danger targets and a log of NPC turns.
#[derive(Default)]
struct Scripted {
    danger: Vec<(ObjectRef, ObjectRef)>,
    turns: Vec<ObjectRef>,
}
impl CombatAi for Scripted {
    fn danger_target(&mut self, _world: &WorldState, critter: ObjectRef) -> Option<ObjectRef> {
        self.danger
            .iter()
            .find(|(who, _)| *who == critter)
            .map(|(_, danger)| *danger)
    }
    fn take_turn(
        &mut self,
        _world: &mut WorldState,
        critter: ObjectRef,
    ) -> Result<(), crate::WorldError> {
        self.turns.push(critter);
        Ok(())
    }
}

#[test]
fn spec007_combat_initialization_rounds_turns_and_cleanup() {
    use crate::critical::{maneuver, state};
    let mut world = world();
    let player = world.player;
    let a = fighter(&mut world, 20102, 12, 0);
    let b = fighter(&mut world, 20104, 6, 0);
    let c = fighter(&mut world, 20106, 8, 0);
    let hidden = fighter(&mut world, 20108, 8, 0);
    world.get_mut(hidden).unwrap().raw.flags = BasicFlags(BasicFlags::HIDDEN.0);
    let upstairs = fighter(&mut world, 20110, 8, 1);
    world.critter_state_mut(c).unwrap().maneuvers = maneuver::ALERTED | maneuver::FLEEING;
    let mut host = crate::WorldHost::new(world, NoAssets);
    assert_eq!(host.world.combat_state.flags, state::INPUT_GATE);

    // NPC attacker vs NPC defender: active order a, b, then the player.
    host.start_combat(a, b).unwrap();
    let combat = host.world.combat_state.clone();
    assert_eq!(combat.flags, state::ACTIVE | state::INPUT_GATE);
    assert_eq!(&combat.list[..combat.active], &[a, b, player]);
    assert_eq!(
        combat.list.len(),
        4,
        "hidden and other-elevation critters are not collected"
    );
    assert!(!combat.list.contains(&hidden) && !combat.list.contains(&upstairs));
    assert_eq!(
        host.world.critter_state(c).unwrap().maneuvers,
        maneuver::ALERTED
    );
    assert_eq!(
        (
            host.world.get(a).unwrap().opponent,
            host.world.get(b).unwrap().opponent
        ),
        (b, a)
    );

    // First pass in initiating order, unsorted: NPC turns, then the player.
    let mut ai = Scripted::default();
    assert_eq!(
        host.advance_combat(&mut ai).unwrap(),
        CombatProgress::PlayerTurn
    );
    assert_eq!(ai.turns, [a, b]);
    // AP refilled to effective maximum AP (+ request bonus 0).
    assert_eq!(ap(&host.world, a), 7);
    assert_eq!(ap(&host.world, player), 7);
    // Free movement does not keep player input open once ordinary AP is zero.
    host.world.free_movement = 4;
    set_results(&mut host.world, player, 0, 0);
    assert!(host.player_turn_over());
    set_results(&mut host.world, player, 0, 7);
    host.end_player_turn();

    // Round sweep: c (alerted) joins and takes an immediate turn; actors are
    // then sorted by descending Sequence; five game seconds pass.
    let before = host.world.clock.ticks;
    host.world.critter_state_mut(b).unwrap().maneuvers = maneuver::STOP_ATTACKING;
    ai.turns.clear();
    assert_eq!(
        host.advance_combat(&mut ai).unwrap(),
        CombatProgress::PlayerTurn
    );
    assert_eq!(host.world.clock.ticks, before + 50);
    let combat = host.world.combat_state.clone();
    // b (maneuver exactly 2) moved to waiting; c joined; sorted a(12), player(10), c(8).
    assert_eq!(&combat.list[..combat.active], &[a, player, c]);
    assert_eq!(
        ai.turns,
        [c, a],
        "c's admission turn precedes the sorted round"
    );

    // An explicit end request is refused while a relevant active opponent
    // does not want to stop.
    ai.danger = vec![(a, player), (c, player)];
    set_results(&mut host.world, a, 0, 3);
    host.world.critter_state_mut(c).unwrap().maneuvers = 0;
    set_results(&mut host.world, c, 0, 3);
    assert!(!host.request_end_combat(&mut ai));
    // Opponents that flee/stop allow it; a joining relevant waiter refuses.
    host.world.critter_state_mut(a).unwrap().maneuvers = maneuver::FLEEING;
    host.world.critter_state_mut(c).unwrap().maneuvers = maneuver::STOP_ATTACKING;
    host.world.critter_state_mut(b).unwrap().damage = 5;
    assert!(
        !host.request_end_combat(&mut ai),
        "b wants to rejoin after damage"
    );
    host.world.critter_state_mut(b).unwrap().damage = 0;
    assert!(host.request_end_combat(&mut ai));
    assert_ne!(host.world.combat_state.flags & state::END_REQUESTED, 0);
    // Player input consumes the request and exits combat; cleanup awards
    // accumulated kill experience to the living player.
    host.world.combat_experience = 25;
    assert_eq!(host.advance_combat(&mut ai).unwrap(), CombatProgress::Ended);
    assert!(!host.world.combat);
    assert_eq!(host.world.combat_state.flags, state::INPUT_GATE);
    assert_eq!(host.world.player_sheet().unwrap().experience, 25);
    assert_eq!(ap(&host.world, player), 7);
    assert_eq!(host.world.critter_state(a).unwrap().maneuvers, 0);
}

#[test]
fn spec007_combat_turn_skips_standups_and_automatic_ending() {
    use crate::critical::state;
    let mut world = world();
    let player = world.player;
    let a = fighter(&mut world, 20102, 12, 0);
    let mut host = crate::WorldHost::new(world, NoAssets);
    host.start_combat(player, a).unwrap();
    // Lose-turn skips the action branch and is cleared.
    set_results(&mut host.world, a, effect::LOSE_TURN, 0);
    let mut ai = Scripted::default();
    assert_eq!(
        host.advance_combat(&mut ai).unwrap(),
        CombatProgress::PlayerTurn
    );
    host.end_player_turn();
    // The player acted first (initiating order); a then skipped its turn
    // (clearing lose-turn). Round two sorts a (Sequence 12) before the player
    // (10), so a acts normally before the player's next input.
    assert_eq!(
        host.advance_combat(&mut ai).unwrap(),
        CombatProgress::PlayerTurn
    );
    assert_eq!(ai.turns, [a]);
    assert_eq!(
        host.world.critter_state(a).unwrap().results.0 & effect::LOSE_TURN,
        0
    );
    assert_eq!(ap(&host.world, a), 7);
    // A prone actor stands for 3 AP (or to zero below 3) before acting.
    set_results(&mut host.world, a, effect::KNOCKED_DOWN, 0);
    host.end_player_turn();
    assert_eq!(
        host.advance_combat(&mut ai).unwrap(),
        CombatProgress::PlayerTurn
    );
    assert_eq!(ai.turns, [a, a]);
    assert_eq!(ap(&host.world, a), 4);
    assert_eq!(
        host.world.critter_state(a).unwrap().results.0 & effect::KNOCKED_DOWN,
        0
    );
    // Automatic ending once a's team leaves combat: only the player remains.
    set_results(&mut host.world, a, effect::DEAD, 0);
    host.end_player_turn();
    assert_eq!(
        host.advance_combat(&mut ai).unwrap(),
        CombatProgress::PlayerTurn
    );
    host.end_player_turn();
    assert_eq!(host.advance_combat(&mut ai).unwrap(), CombatProgress::Ended);
    assert_eq!(host.world.combat_state.flags, state::INPUT_GATE);
    // Same-team actors with no cross-team opponents end automatically.
    let mut allied = self::world();
    let player = allied.player;
    let ally = fighter(&mut allied, 20102, 12, 0);
    allied.critter_state_mut(ally).unwrap().team = 0;
    let mut host = crate::WorldHost::new(allied, NoAssets);
    host.start_combat(player, ally).unwrap();
    for critter in [player, ally] {
        host.world.get_mut(critter).unwrap().opponent = 0;
    }
    assert!(host.combat_should_end());
}

#[test]
fn spec007_scripted_request_starts_combat_and_adjusts_the_first_attack() {
    use crate::combat_state::CombatRequest;
    let (mut host, defender) = fixture_c_world();
    let player = host.world.player;
    host.world.combat = false;
    host.world.combat_actor = 0;
    host.world.pending_combat = Some(CombatRequest {
        attacker: player,
        defender,
        accuracy: 10,
        damage: [0, 5],
        flags: Some([0x100, 0x102]),
    });
    host.tick(0).unwrap();
    assert!(host.world.combat);
    assert!(host.world.combat_state.player_turn);
    assert!(host.world.combat_state.request.is_some());
    // Starting combat cleared the defender's unused AP (AC 12 -> 10), and the
    // request's accuracy adjustment adds 10: 26 + 2 + 10.
    let mut attack = host
        .world
        .attack_profile(
            player,
            defender,
            critical::hit_mode::RIGHT_PRIMARY,
            location::RIGHT_LEG,
            true,
        )
        .unwrap();
    let request = host.apply_request(player, defender, &mut attack).unwrap();
    assert_eq!(attack.hit_chance(true).unwrap(), 38);
    host.world.combat_state.request = Some(request);
    let hp = host.world.hit_points(defender).unwrap();
    let resolution = host
        .perform_attack(
            player,
            defender,
            critical::hit_mode::RIGHT_PRIMARY,
            location::RIGHT_LEG,
            true,
        )
        .unwrap()
        .unwrap();
    // Damage clamped to [0, 5]; under the safe rule the override words go
    // to the attacker and defender flags (the reference's last word wins).
    assert!(resolution.damage <= 5);
    assert_eq!(
        (resolution.attacker_flags, resolution.defender_flags),
        (0x100, 0x102)
    );
    assert_eq!(
        host.world.hit_points(defender).unwrap(),
        hp - resolution.damage
    );
    assert_ne!(
        host.world.critter_state(defender).unwrap().results.0 & effect::KNOCKED_DOWN,
        0
    );
    // The request is consumed by that first attack.
    assert!(host.world.combat_state.request.is_none());
    assert_eq!(ap(&host.world, player), 7 - 6);
}

#[test]
fn spec005_an_elevation_change_removes_combat_membership() {
    let mut world = world();
    let player = world.player;
    let a = fighter(&mut world, 20102, 12, 0);
    let b = fighter(&mut world, 20104, 6, 0);
    let mut host = crate::WorldHost::new(world, NoAssets);
    host.start_combat(a, b).unwrap();
    host.world.get_mut(a).unwrap().opponent = b;
    assert!(host.world.ai.snapshot.contains(&b));
    // SPEC-005 0x80B6/0x80FF: moving b to another elevation removes it from
    // the list and its partition, the SPEC-009 snapshot, and every
    // last-attacker reference to it.
    host.world.move_object(b, 20104, 1, false).unwrap();
    let combat = &host.world.combat_state;
    assert!(!combat.list.contains(&b));
    assert_eq!(&combat.list[..combat.active], &[a, player]);
    assert!(!host.world.ai.snapshot.contains(&b));
    assert_eq!(host.world.get(a).unwrap().opponent, 0);
    // A move on the combat elevation keeps membership.
    host.world.move_object(a, 20103, 0, false).unwrap();
    assert!(host.world.combat_state.list.contains(&a));
}

#[test]
fn spec007_the_ap_debit_is_the_requests_cost_not_the_victims() {
    let mut world = world();
    let attacker = fighter(&mut world, 20102, 12, 0);
    let intended = fighter(&mut world, 20104, 6, 0);
    let bystander = fighter(&mut world, 20106, 8, 0);
    world.critter_state_mut(attacker).unwrap().ap = 9;
    let mut host = crate::WorldHost::new(world, NoAssets);
    // SPEC-007 §3.4: fixture C's aimed leg shot costs 5 + 1. The missed
    // shot's bystander profile (§3.7.1, torso, unaimed) would cost 5, but it
    // serves damage, armor and knockback only.
    let request = fixture_c();
    assert_eq!(request.ap_cost(), 6);
    let mut victim = request.clone();
    victim.location = location::TORSO;
    victim.aimed = false;
    assert_eq!(victim.ap_cost(), 5);
    let spray = crate::host_combat::Spray {
        main: crate::combat::Resolution::default(),
        extras: Vec::new(),
    };
    host.apply_spray_sequenced(
        attacker,
        intended,
        bystander,
        &victim,
        &spray,
        request.ap_cost(),
    )
    .unwrap();
    assert_eq!(host.world.critter_state(attacker).unwrap().ap, 3);
}

#[test]
fn spec007_a_kill_category_outside_the_tables_uses_table_0() {
    // SPEC-007 §2.5 safe rule: an unvalidated category word selects victim
    // table 0 instead of refusing the attack after its draws.
    let mut attack = fixture_c();
    attack.kill_category = 15;
    let mut resolution = crate::combat::Resolution::default();
    attack
        .critical_success(&mut resolution, &mut Lowest)
        .unwrap();
    attack.kill_category = 0;
    let mut expected = crate::combat::Resolution::default();
    attack.critical_success(&mut expected, &mut Lowest).unwrap();
    assert_eq!(resolution, expected);
    assert_ne!(resolution.defender_flags & effect::CRITICAL, 0);
}

/// SPEC-014 §3.44 and Fixture T: the outline word's operations.
#[test]
fn spec014_outline_word_operations() {
    use vault_formats::map::outline;
    let mut world = world();
    let a = fighter(&mut world, 20102, 8, 0);
    // Fixture T: setting type 16 over 0x80000008 is refused.
    world.get_mut(a).unwrap().raw.outline = 0x8000_0008;
    assert!(!world.set_outline(a, outline::ITEM));
    assert_eq!(world.outline_word(a), 0x8000_0008);
    assert!(
        !world.outlined(a),
        "hostile or friendly, disabled: not drawn"
    );
    world.enable_outline(a);
    assert_eq!(world.outline_word(a), 8);
    assert!(world.outlined(a));
    world.remove_outline(a);
    assert_eq!(world.outline_word(a), 0);
    // Disabling needs a type.
    world.disable_outline(a);
    assert_eq!(world.outline_word(a), 0);
    // A hidden object's new type is set disabled; showing enables it and
    // hiding disables it again.
    world.set_hidden(a, true);
    assert!(world.set_outline(a, outline::RED | outline::TRANSLUCENT));
    assert_eq!(world.outline_word(a), 0xC000_0002);
    world.set_hidden(a, false);
    assert_eq!(world.outline_word(a), 0x4000_0002);
    world.set_hidden(a, true);
    assert_eq!(world.outline_word(a), 0xC000_0002);
    // The no-highlight flag refuses a type.
    let b = fighter(&mut world, 20104, 8, 0);
    world.get_mut(b).unwrap().raw.flags.0 |= BasicFlags::NO_HIGHLIGHT.0;
    assert!(!world.set_outline(b, outline::ITEM));
    assert_eq!(world.outline_word(b), 0);
}

/// An AI that records whether each acting NPC is outlined during its turn.
#[derive(Default)]
struct OutlineWatch {
    seen: Vec<(ObjectRef, bool)>,
}
impl CombatAi for OutlineWatch {
    fn take_turn(
        &mut self,
        world: &mut WorldState,
        critter: ObjectRef,
    ) -> Result<(), crate::WorldError> {
        self.seen.push((critter, world.outlined(critter)));
        Ok(())
    }
}

/// SPEC-014 §3.44 combat outlines: types at combat start (Friendly Foe),
/// the player's turn under `target_highlight` and the crosshair, NPC turns,
/// team changes, death and the end of combat; then the outside-combat
/// steps.
#[test]
fn spec014_combat_sets_and_clears_target_outlines() {
    use crate::stats::Perk;
    use vault_formats::map::outline;
    let mut world = world();
    let player = world.player;
    let a = fighter(&mut world, 20102, 12, 0);
    let b = fighter(&mut world, 20104, 6, 0);
    world.critter_state_mut(player).unwrap().team = 0;
    world.critter_state_mut(a).unwrap().team = 1;
    world.critter_state_mut(b).unwrap().team = 0;
    let context = world.context(player);
    world
        .player_sheet_mut()
        .unwrap()
        .set_perk(Perk::FriendlyFoe, 1, context);
    world.target_highlight = 1;
    world.crosshair = true;
    let mut host = crate::WorldHost::new(world, NoAssets);
    host.start_combat(player, a).unwrap();
    assert_eq!(host.world.combat_state.highlight, 1);
    assert_eq!(host.world.outline_word(a), 0x8000_0001);
    assert_eq!(host.world.outline_word(b), 0x8000_0008, "Friendly Foe");
    // The player's turn enables every living combatant but the player.
    let mut ai = OutlineWatch::default();
    assert_eq!(
        host.advance_combat(&mut ai).unwrap(),
        CombatProgress::PlayerTurn
    );
    assert!(host.world.outlined(a) && host.world.outlined(b));
    assert!(!host.world.outlined(player));
    // Ending it disables them; the acting NPC is outlined for its turn.
    host.end_player_turn();
    host.world.crosshair = false;
    assert_eq!(
        host.advance_combat(&mut ai).unwrap(),
        CombatProgress::PlayerTurn
    );
    assert!(
        ai.seen.iter().all(|(_, outlined)| *outlined),
        "{:?}",
        ai.seen
    );
    assert!(!ai.seen.is_empty());
    // Without the crosshair the player's turn enables nothing.
    assert!(!host.world.outlined(a) && !host.world.outlined(b));
    // Entering crosshair mode enables them.
    host.world.crosshair = true;
    host.world.enable_target_outlines();
    assert!(host.world.outlined(a));
    // A team change rebuilds the type and keeps it enabled.
    host.set_team(a, 0);
    assert_eq!(host.world.outline_word(a), outline::FRIENDLY);
    // Death disables it.
    host.damage_critter(b, None, 1000, 0).unwrap();
    assert_eq!(host.world.outline_word(b), 0x8000_0008);
    // The end of combat removes every outline.
    host.end_combat().unwrap();
    assert_eq!(host.world.outline_word(a), 0);
    assert_eq!(host.world.outline_word(player), 0);
    // Outside combat: type 8 for the player's team, 1 for the others, no
    // Friendly Foe test, enabled; the disable step removes them.
    host.world.critter_state_mut(a).unwrap().team = 2;
    host.world.enable_target_outlines();
    assert_eq!(host.world.outline_word(a), outline::HOSTILE);
    assert_eq!(host.world.outline_word(player), 0);
    host.world.disable_target_outlines();
    assert_eq!(host.world.outline_word(a), 0);
    // With the setting 0 nothing is enabled.
    host.world.target_highlight = 0;
    host.world.enable_target_outlines();
    assert_eq!(host.world.outline_word(a), 0);
}

/// An AI whose NPC turns can request the script quit (as `terminate_combat`
/// does) or fail; every NPC sees the player as its danger source.
#[derive(Default)]
struct Quitting {
    quit_on: ObjectRef,
    fail: bool,
    turns: Vec<ObjectRef>,
}
impl CombatAi for Quitting {
    fn danger_target(&mut self, world: &WorldState, critter: ObjectRef) -> Option<ObjectRef> {
        (critter != world.player).then_some(world.player)
    }
    fn take_turn(
        &mut self,
        world: &mut WorldState,
        critter: ObjectRef,
    ) -> Result<(), crate::WorldError> {
        self.turns.push(critter);
        if critter == self.quit_on {
            world.combat_quit = true;
        }
        if self.fail {
            return Err(crate::WorldError::Invalid("scripted turn failure".into()));
        }
        Ok(())
    }
}

#[test]
fn spec005_terminate_combat_ends_combat_at_the_next_turn_boundary() {
    use crate::critical::state;
    let mut world = world();
    let player = world.player;
    let a = fighter(&mut world, 20102, 12, 0);
    let mut host = crate::WorldHost::new(world, NoAssets);
    // Requested during the player's turn: the turn's input stops, and
    // combat ends once it is over; a quit awards no kill experience.
    host.start_combat(player, a).unwrap();
    let mut ai = Quitting::default();
    assert_eq!(
        host.advance_combat(&mut ai).unwrap(),
        CombatProgress::PlayerTurn
    );
    host.world.combat_experience = 25;
    host.world.combat_quit = true;
    assert!(host.player_turn_over());
    assert_eq!(host.advance_combat(&mut ai).unwrap(), CombatProgress::Ended);
    assert!(!host.world.combat && !host.world.combat_quit);
    assert_eq!(host.world.combat_state.flags, state::INPUT_GATE);
    assert_eq!(host.world.player_sheet().unwrap().experience, 0);
    assert!(ai.turns.is_empty());
    // Requested during an NPC's turn: combat ends before the player's
    // next turn, and the player is back in control outside combat.
    host.start_combat(a, player).unwrap();
    let mut ai = Quitting {
        quit_on: a,
        ..Quitting::default()
    };
    assert_eq!(host.advance_combat(&mut ai).unwrap(), CombatProgress::Ended);
    assert_eq!(ai.turns, [a]);
    assert!(!host.world.combat && !host.world.combat_state.player_turn);
}

#[test]
fn spec014_a_knocked_out_player_wakes_between_combat_rounds() {
    use crate::queue::EventKind;
    let mut world = world();
    let player = world.player;
    let a = fighter(&mut world, 20102, 12, 0);
    let mut host = crate::WorldHost::new(world, NoAssets);
    host.start_combat(a, player).unwrap();
    set_results(&mut host.world, player, effect::KNOCKED_OUT, 0);
    let now = host.world.clock.ticks;
    host.world
        .queue
        .insert(now, 120, player, EventKind::Knockout)
        .unwrap();
    // The player's turns are skipped while out; each round adds 50 ticks
    // and processes the queue, so the wake-up runs and the player stands
    // at a turn start (3 AP) instead of combat spinning to its turn limit.
    let mut ai = Quitting::default();
    assert_eq!(
        host.advance_combat(&mut ai).unwrap(),
        CombatProgress::PlayerTurn
    );
    assert!(host.world.clock.ticks >= now + 120);
    let results = host.world.critter_state(player).unwrap().results.0;
    assert_eq!(results & (effect::KNOCKED_OUT | effect::KNOCKED_DOWN), 0);
    assert_eq!(ap(&host.world, player), 7 - 3);
    assert!(ai.turns.len() >= 3);
}

#[test]
fn a_failing_npc_turn_is_diagnosed_and_the_turn_passes() {
    let mut world = world();
    let player = world.player;
    let a = fighter(&mut world, 20102, 12, 0);
    let mut host = crate::WorldHost::new(world, NoAssets);
    host.start_combat(a, player).unwrap();
    let mut ai = Quitting {
        fail: true,
        ..Quitting::default()
    };
    assert_eq!(
        host.advance_combat(&mut ai).unwrap(),
        CombatProgress::PlayerTurn
    );
    assert_eq!(ai.turns, [a]);
    assert!(host.world.effects.iter().any(|effect| matches!(
        effect,
        crate::WorldEffect::Diagnostic(message) if message.contains("scripted turn failure")
    )));
}

#[test]
fn spec007_a_scripted_request_lasts_only_for_the_first_turn() {
    use crate::combat_state::CombatRequest;
    let mut world = world();
    let player = world.player;
    let a = fighter(&mut world, 20102, 12, 0);
    let mut host = crate::WorldHost::new(world, NoAssets);
    host.world.pending_combat = Some(CombatRequest {
        attacker: a,
        defender: player,
        accuracy: 10,
        damage: [0, 5],
        flags: None,
    });
    assert!(host.service_combat_request().unwrap());
    assert!(host.world.combat_state.request.is_some());
    // The attacker's first turn passes without the requested attack: the
    // window closes, so a later attack is not adjusted by the request.
    let mut ai = Quitting::default();
    assert_eq!(
        host.advance_combat(&mut ai).unwrap(),
        CombatProgress::PlayerTurn
    );
    assert_eq!(ai.turns, [a]);
    assert!(host.world.combat_state.request.is_none());
}
