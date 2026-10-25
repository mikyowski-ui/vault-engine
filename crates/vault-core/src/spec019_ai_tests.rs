//! SPEC-019 §3.16 through the live SPEC-009 AI: vectors D-I on a Fallout 2
//! party member with a control record, and the neutral record and a
//! member without one reducing to SPEC-009.
use super::*;
use crate::{
    fallout2::{Fallout2Tables, Fallout2World},
    fallout2_party::{
        self as party, area_attack, attack_who, best_weapon, chem_use, distance, run_away,
    },
    host_fallout2_party::ray_between,
};
use vault_formats::fallout2::save::MemberRecord;

/// `ai.txt` with SPEC-009's packets and `Hurt` (5): min_hp 10, hurt too
/// much on crippled legs.
fn party_assets() -> TestAssets {
    let mut assets = assets();
    let mut text = packets();
    text.extend(
        section(
            "Hurt",
            5,
            &[("min_hp", "10"), ("hurt_too_much", "crippled_legs")],
        )
        .into_bytes(),
    );
    assets.resources.insert("data/ai.txt".into(), text);
    assets
}
fn tables() -> Arc<Fallout2Tables> {
    let mut files: Vec<(&str, &str)> = vault_fixtures::fallout2::tables()
        .into_iter()
        .filter(|(path, _)| *path != "data/party.txt")
        .collect();
    files.push((
        "data/party.txt",
        "[Party Member 0]\nparty_member_pid=16777300\nlevel_minimum=1\nlevel_up_every=1\n",
    ));
    Arc::new(Fallout2Tables::load(&mut vault_fixtures::fallout2::reader(files)).unwrap())
}
/// Make `host` a Fallout 2 game and `member` a party member (id 18000 + n)
/// with control record `fields`.
fn join(host: &mut WorldHost<TestAssets>, member: ObjectRef, fields: [i32; 7]) {
    if host.world.fallout2.is_none() {
        host.world.fallout2 = Some(Box::new(Fallout2World::new_game(tables(), -1)));
    }
    let id = 18000 + host.world.party.len() as u32;
    host.world.get_mut(member).unwrap().raw.id = id;
    host.world.party.insert(member);
    host.world.fallout2.as_deref_mut().unwrap().control.insert(
        id,
        MemberRecord {
            record: 0,
            stage: 0,
            fields,
        },
    );
}
fn with(field: party_field::Field, value: i32) -> [i32; 7] {
    let mut fields = party::neutral();
    fields[field as usize] = value;
    fields
}
mod party_field {
    pub use vault_formats::fallout2::party::ControlField as Field;
}
use party_field::Field;

#[test]
fn spec019_vector_d_run_away_through_the_turn() {
    let flee = |mode: i32, crippled: bool| {
        let mut world = world();
        let member = npc(&mut world, 20300, 0, 5, &[(7, 60)]);
        set_hp(&mut world, member, 44);
        if crippled {
            state(&mut world, member).results.0 = effect::CRIPPLED_LEFT_LEG;
        }
        let mut host = ai_host(world, party_assets());
        join(&mut host, member, with(Field::RunAwayMode, mode));
        let packet = host.packet(member).unwrap();
        host.party_forced_flee(member, &packet).unwrap()
    };
    let modes = [
        run_away::NONE,
        run_away::BLEEDING,
        run_away::NOT_FEELING_GOOD,
        run_away::NEVER,
    ];
    assert_eq!(
        modes.map(|mode| flee(mode, true)),
        [true, true, true, false]
    );
    assert_eq!(
        modes.map(|mode| flee(mode, false)),
        [false, true, false, false]
    );
    // Without a control record SPEC-009's test applies (`None`).
    let mut world = world();
    let member = npc(&mut world, 20300, 0, 5, &[]);
    let host = ai_host(world, party_assets());
    let packet = {
        let mut host = host;
        let packet = host.packet(member).unwrap();
        assert_eq!(host.party_forced_flee(member, &packet), None);
        packet
    };
    assert_eq!(packet.min_hp, 10);
}

#[test]
fn spec019_vector_e_chem_use_through_the_healing_loop() {
    let heal = |mode: i32, hp: i32| {
        let mut world = world();
        let member = npc(&mut world, 20300, 0, 0, &[(7, 61)]);
        set_hp(&mut world, member, hp);
        state(&mut world, member).ap = 6;
        carry(
            &mut world,
            member,
            drug(crate::ai::STIMPAK, [35, -1, -1], [10, 0, 0]),
            2,
            ObjectDetail::Plain,
        );
        let mut host = ai_host(world, party_assets());
        join(&mut host, member, with(Field::ChemUse, mode));
        host.ai_heal(member).unwrap();
        let state = host.world.critter_state(member).unwrap();
        (state.hp, state.ap, stimpak_count(&host.world, member))
    };
    assert_eq!(heal(chem_use::CLEAN, 40), (40, 6, 2));
    assert_eq!(heal(chem_use::ANYTIME, 40), (40, 6, 2));
    assert_eq!(heal(chem_use::STIMS_WHEN_HURT_LITTLE, 40), (50, 4, 1));
    assert_eq!(heal(chem_use::ALWAYS, 20), (40, 2, 0));
    assert_eq!(heal(chem_use::SOMETIMES, 20), (30, 4, 1));
    // The neutral record (`anytime`) is SPEC-009's half-HP threshold.
    assert_eq!(heal(chem_use::ANYTIME, 29), (39, 4, 1));
}

#[test]
fn spec019_vector_f_attack_who_through_the_snapshot() {
    let mut world = world();
    let player = world.player;
    // Self S on team 0 facing down the column; Perception 1 sees 5 hexes
    // ahead (out of combat it would see 1 behind).
    let s = npc(&mut world, 20100, 0, 0, &[(1, 1)]);
    let m = npc(&mut world, 20500, 0, 0, &[]);
    let a = npc(&mut world, 20100 + 2 * 200, 1, 0, &[(9, 2)]);
    let b = npc(&mut world, 20100 + 4 * 200, 1, 0, &[(9, 15)]);
    let c = npc(&mut world, 20100 + 6 * 200, 1, 0, &[(9, 30)]);
    let d = npc(&mut world, 20100 + 7 * 200, 1, 0, &[]);
    world.get_mut(s).unwrap().raw.orientation = crate::routing::rotation(20100, 20500).unwrap();
    set_hp(&mut world, a, 10);
    set_hp(&mut world, b, 30);
    set_hp(&mut world, c, 5);
    world.get_mut(a).unwrap().opponent = m;
    world.get_mut(b).unwrap().opponent = player;
    world.get_mut(c).unwrap().opponent = player;
    world.get_mut(d).unwrap().opponent = player;
    state(&mut world, d).results.0 = effect::DEAD;
    world.ai.snapshot = vec![d, c, b, a, player, m, s];
    let mut host = ai_host(world, party_assets());
    join(&mut host, m, party::neutral());
    assert!(host.world.retaliation_rating(b) > host.world.retaliation_rating(a));
    assert!(!host.world.can_perceive(s, c));
    let target = |host: &mut WorldHost<TestAssets>, mode: i32| {
        if let Some(record) = host
            .world
            .fallout2
            .as_deref_mut()
            .unwrap()
            .control
            .get_mut(&18001)
        {
            record.fields[Field::AttackWho as usize] = mode;
        }
        host.party_target(s)
    };
    join(&mut host, s, party::neutral());
    assert_eq!(
        target(&mut host, attack_who::WHOMEVER_ATTACKING_ME),
        Some(b)
    );
    assert_eq!(target(&mut host, attack_who::CLOSEST), Some(a));
    assert_eq!(target(&mut host, attack_who::STRONGEST), Some(b));
    assert_eq!(target(&mut host, attack_who::WEAKEST), Some(a));
    assert_eq!(target(&mut host, attack_who::WHOMEVER), None, "danger(S)");
    // No draw was made, and the snapshot keeps its entries.
    assert_eq!(host.world.ai.snapshot.len(), 7);
}

#[test]
fn spec019_vector_g_weapon_ranking_through_the_inventory() {
    let search = |mode: i32| {
        let mut world = world();
        let member = npc(&mut world, 20300, 0, 0, &[]);
        set_skill(&mut world, member, 5, 100);
        set_skill(&mut world, member, 4, 100);
        let knife = carry(
            &mut world,
            member,
            weapon(40, 0x03, 3, 4, 1, 0, 40),
            1,
            loaded(0, -1),
        );
        let pistol = carry(
            &mut world,
            member,
            weapon(41, 0x06, 4, 8, 20, 10, 250),
            1,
            loaded(5, 30),
        );
        let throwing = carry(
            &mut world,
            member,
            weapon(42, 0x05, 3, 3, 10, 0, 50),
            1,
            loaded(0, -1),
        );
        state(&mut world, member).ap = 8;
        let mut assets = party_assets();
        assets.prototypes.insert(30, ammo(30, 0, 10));
        let mut host = ai_host(world, assets);
        join(&mut host, member, with(Field::BestWeapon, mode));
        let found = host.inventory_search(member, true);
        [knife, pistol, throwing]
            .iter()
            .position(|weapon| Some(*weapon) == found)
    };
    assert_eq!(search(best_weapon::NO_PREF), Some(1));
    assert_eq!(search(best_weapon::MELEE), Some(0));
    assert_eq!(search(best_weapon::RANGED), Some(1));
    assert_eq!(search(best_weapon::UNARMED_OVER_THROWN), Some(2));
    assert_eq!(search(best_weapon::MELEE_OVER_RANGED), Some(0));
}

#[test]
fn spec019_vector_h_area_attacks_through_the_ray() {
    let gate = |mode: i32, primary_burst: bool| {
        let mut world = world();
        let member = npc(&mut world, 19100, 0, 0, &[]);
        let target = npc(&mut world, HexGrid::step(19100, 1, 5).unwrap(), 1, 0, &[]);
        let (from, to) = (hex(&world, member), hex(&world, target));
        assert_eq!(world.object_distance(member, target), 5);
        let ray = ray_between(from, to, 0);
        let third = ray[2];
        let spot = (0..6)
            .filter_map(|direction| HexGrid::neighbor(third, direction).ok().flatten())
            .find(|hex| {
                !ray.contains(hex) && *hex != from && HexGrid::lattice_distance(*hex, to) == Ok(3)
            })
            .expect("a friendly spot next to the third hex");
        // Move the player (a friendly) there.
        let player = world.player;
        world.move_object(player, spot, 0, false).unwrap();
        // SMG: primary single (6), secondary burst (7); a minigun bursts both.
        let nibbles = if primary_burst { 0x77 } else { 0x76 };
        let smg = carry(
            &mut world,
            member,
            weapon(50, nibbles, 4, 8, 30, 30, 100),
            1,
            loaded(30, 30),
        );
        let mut host = ai_host(world, party_assets());
        join(&mut host, member, with(Field::AreaAttackMode, mode));
        let exposure = host.burst_exposure(member, target);
        (
            exposure.at_risk,
            host.party_burst_gate(member, target, Some(smg), hit_mode::RIGHT_SECONDARY),
        )
    };
    assert_eq!(gate(area_attack::ALWAYS, false), (1, (3, false)));
    assert_eq!(gate(area_attack::SOMETIMES, false), (1, (3, false)));
    assert_eq!(gate(area_attack::BE_SURE, false), (1, (2, false)));
    assert_eq!(gate(area_attack::BE_CAREFUL, false), (1, (2, false)));
    // Both modes burst: the attack step fails instead.
    assert_eq!(gate(area_attack::BE_SURE, true), (1, (3, true)));
}

#[test]
fn spec019_vector_i_distance_follow_and_approach() {
    // Follow with no target at all and dist(S, player) 4.
    let follow = |mode: i32| {
        let mut world = world();
        let player = world.player;
        let member = npc(&mut world, 20100 + 4 * 200, 0, 0, &[]);
        state(&mut world, member).ap = 3;
        let mut host = ai_host(world, party_assets());
        join(&mut host, member, with(Field::Distance, mode));
        host.ai_turn(member, None).unwrap();
        host.world.object_distance(member, player)
    };
    assert!(follow(distance::STAY_CLOSE) < 4);
    assert_eq!(follow(distance::ON_YOUR_OWN), 4);
    assert_eq!(follow(distance::STAY), 4);

    // An approach the attack loop asks for (range-aware chance below
    // min_to_hit, no-range chance above): under `stay` nothing moves and
    // the loop fails without fleeing; under `stay_close` past the leash the
    // member approaches the player instead.
    let approach = |mode: i32| {
        let mut world = world();
        let player = world.player;
        set_hp(&mut world, player, 1000);
        let member = npc(&mut world, 20100 + 7 * 200, 0, 0, &[]);
        set_skill(&mut world, member, 0, 100);
        let pistol = carry(
            &mut world,
            member,
            weapon(70, 0x06, 4, 1, 2, 10, 100),
            1,
            loaded(10, 30),
        );
        world
            .equip(member, pistol, BasicFlags::RIGHT_HAND.0)
            .unwrap();
        let enemy = npc(&mut world, 20100 + 11 * 200, 1, 0, &[]);
        state(&mut world, member).ap = 8;
        world.combat = true;
        world.ai.snapshot = vec![player, member, enemy];
        let mut host = ai_host(world, party_assets());
        join(&mut host, member, with(Field::Distance, mode));
        let before = (
            host.world.object_distance(member, enemy),
            host.world.object_distance(member, player),
        );
        host.ai_attack(member, enemy).unwrap();
        let after = (
            host.world.object_distance(member, enemy),
            host.world.object_distance(member, player),
        );
        let fled = host.world.critter_state(member).unwrap().maneuvers & intent::FLEE != 0;
        (before, after, fled)
    };
    let (before, after, fled) = approach(distance::STAY);
    assert_eq!((before, after, fled), (before, before, false));
    assert_eq!(before, (4, 7));
    let (before, after, fled) = approach(distance::STAY_CLOSE);
    assert!(after.1 < before.1 && !fled, "{before:?} {after:?}");
    // Otherwise the member approaches T.
    let (before, after, _) = approach(distance::ON_YOUR_OWN);
    assert!(after.0 < before.0, "{before:?} {after:?}");
}

#[test]
fn spec019_a_member_without_a_record_and_fallout1_keep_spec009() {
    // The same party follow as SPEC-009's fixture: beyond 5 hexes.
    let mut world = world();
    let player = world.player;
    let member = npc(&mut world, 20100 + 9 * 200, 0, 0, &[]);
    world.get_mut(member).unwrap().raw.id = 18000;
    world.party.insert(member);
    state(&mut world, member).ap = 3;
    let mut host = ai_host(world, party_assets());
    host.world.fallout2 = Some(Box::new(Fallout2World::new_game(tables(), -1)));
    assert_eq!(host.control_of(member), None);
    host.ai_turn(member, None).unwrap();
    assert_eq!(host.world.object_distance(member, player), 6);
}
