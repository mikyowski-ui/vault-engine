//! SPEC-009 §4.2-§4.10 Fixtures B-J over independent state models.
use crate::{RandomSource, WorldError, ai::*};
use std::collections::{BTreeMap, VecDeque};
use vault_formats::ai::Packets;

/// Replays scripted draws, checking each requested range.
struct Draws(VecDeque<i32>);
impl Draws {
    fn new(values: &[i32]) -> Self {
        Self(values.iter().copied().collect())
    }
}
impl RandomSource for Draws {
    fn range(&mut self, first: i32, last: i32) -> Result<i32, WorldError> {
        let value = self.0.pop_front().expect("scripted draw available");
        assert!(
            (first.min(last)..=first.max(last)).contains(&value),
            "{value} outside {first}..={last}"
        );
        Ok(value)
    }
}

#[test]
fn spec009_fixture_b_perception_limits() {
    let p = 6;
    // (forward, combat, glass, sneaking player, limit, [d=3, d=7, d=15])
    let rows = [
        (true, false, false, false, 30, [true, true, true]),
        (true, true, false, false, 30, [true, true, true]),
        (true, false, true, false, 15, [true, true, true]),
        (true, false, false, true, 7, [true, true, false]),
        (true, false, true, true, 3, [true, false, false]),
        (false, false, false, false, 6, [true, false, false]),
        (false, true, false, false, 12, [true, true, false]),
        (false, true, true, false, 12, [true, true, false]),
        (false, true, false, true, 3, [true, false, false]),
    ];
    for (forward, combat, glass, sneaking, limit, expected) in rows {
        let computed = perception_limit(p, forward, glass, sneaking, combat);
        assert_eq!(computed, limit, "{forward} {combat} {glass} {sneaking}");
        let seen = [3, 7, 15].map(|d| d <= computed);
        assert_eq!(seen, expected);
    }
}

/// Fixture C's snapshot: (team, dead, last attacker, dist from A).
struct Table {
    rows: BTreeMap<u32, (i32, bool, Option<u32>, i32)>,
    limit: i32,
}
impl ThreatView for Table {
    fn last_attacker(&self, critter: u32) -> Option<u32> {
        self.rows[&critter].2
    }
    fn dead(&self, critter: u32) -> bool {
        self.rows[&critter].1
    }
    fn team(&self, critter: u32) -> i32 {
        self.rows[&critter].0
    }
    fn dist(&self, a: u32, b: u32) -> i32 {
        // Distances are listed from the actor A (1).
        assert_eq!(a, 1);
        self.rows[&b].3
    }
    fn perceives(&self, _observer: u32, target: u32) -> bool {
        self.dist(1, target) <= self.limit
    }
}
const A: u32 = 1;
const B: u32 = 2;
const C: u32 = 3;
const D: u32 = 4;
const E: u32 = 5;
const F: u32 = 6;
const X: u32 = 7;
fn fixture_c() -> Table {
    Table {
        rows: BTreeMap::from([
            (A, (1, false, Some(X), 0)),
            (B, (2, false, Some(A), 5)),
            (C, (1, false, Some(D), 2)),
            (D, (2, false, None, 12)),
            (E, (2, false, Some(F), 3)),
            (F, (1, false, None, 9)),
            (X, (2, true, None, 4)),
        ]),
        // Perception 5, in combat, every entry in the rear sector.
        limit: perception_limit(5, false, false, false, true),
    }
}

#[test]
fn spec009_fixture_c_danger_source_selection() {
    let view = fixture_c();
    assert_eq!(view.limit, 10);
    let mut snapshot = vec![A, B, C, D, E, F, X];
    // X is dead on team 2, so t0 is the nearest living team-2 entry: E.
    assert_eq!(
        nearest_team_member(&mut snapshot.clone(), A, X, &view),
        Some(E)
    );
    // C fills t2 = D; E fills t3 = E; X nothing; B fills t1 = B (3 fills).
    assert_eq!(
        attacker_search(&mut snapshot.clone(), A, &view),
        [Some(B), Some(D), Some(E)]
    );
    assert_eq!(danger(&mut snapshot, A, &view), Some(E));
    // The distance sort persists in the snapshot.
    assert_eq!(snapshot, [A, C, E, X, B, F, D]);
    // X alive: returned immediately, without a perception test.
    let mut alive = fixture_c();
    alive.rows.get_mut(&X).unwrap().1 = false;
    alive.rows.get_mut(&X).unwrap().0 = 1;
    assert_eq!(danger(&mut snapshot.clone(), A, &alive), Some(X));
    // Perception 1 (limit 2): E 3, B 5, D 12 all exceed it.
    let mut dim = fixture_c();
    dim.limit = 2;
    assert_eq!(danger(&mut snapshot.clone(), A, &dim), None);
    // Without E: t0 = B, t2 = D, t1 = B, t3 = B, so B.
    let without = fixture_c();
    let mut snapshot: Vec<_> = vec![A, B, C, D, F, X];
    assert_eq!(
        attacker_search(&mut snapshot.clone(), A, &without),
        [Some(B), Some(D), Some(B)]
    );
    assert_eq!(danger(&mut snapshot, A, &without), Some(B));
    // A self reference is no last attacker.
    let mut own = fixture_c();
    own.rows.get_mut(&A).unwrap().2 = Some(A);
    assert_eq!(danger(&mut [A, B, C, D, E, F, X], A, &own), Some(E));
}

fn weapon(subtype: u8, ap: i32, loaded: i32, ammo: bool, cost: i32) -> InventoryWeapon {
    InventoryWeapon {
        right_hand: false,
        raw_primary_ap: ap,
        usable_primary: true,
        loaded,
        ammo_available: ammo,
        choice: WeaponChoice {
            primary_subtype: subtype,
            cost,
        },
    }
}

#[test]
fn spec009_fixture_d_weapon_preference_and_inventory_search() {
    use subtype::*;
    let inventory = [
        weapon(MELEE, 3, 0, false, 40),     // knife
        weapon(RANGED, 4, 0, false, 250),   // pistol, empty, no ammo
        weapon(THROW, 3, 0, false, 0),      // rock
        weapon(RANGED, 7, 5, false, 1000),  // rifle
        weapon(RANGED, 5, 10, false, 1000), // SMG-A
        weapon(RANGED, 5, 10, false, 1000), // SMG-B
    ];
    assert_eq!(inventory_best(&inventory, Some(5)), Some(4), "SMG-A");
    assert_eq!(inventory_best(&inventory, None), Some(3), "rifle");
    // The right-hand entry is skipped; the pistol becomes eligible with ammo.
    let mut held = inventory;
    held[4].right_hand = true;
    assert_eq!(inventory_best(&held, Some(5)), Some(5));
    let mut supplied = inventory;
    supplied[1].ammo_available = true;
    supplied[3..]
        .iter_mut()
        .for_each(|w| w.usable_primary = false);
    assert_eq!(inventory_best(&supplied, Some(5)), Some(1));
    assert_eq!(inventory_best(&[], None), None);
    // Preference: ranged, then throw, then strictly higher cost.
    let c = |primary_subtype, cost| WeaponChoice {
        primary_subtype,
        cost,
    };
    assert!(prefers_challenger(c(MELEE, 40), c(THROW, 0)));
    assert!(!prefers_challenger(c(RANGED, 0), c(THROW, 99)));
    assert!(
        !prefers_challenger(c(UNARMED, 50), c(MELEE, 50)),
        "cost tie keeps incumbent"
    );
    assert!(prefers_challenger(c(UNARMED, 50), c(MELEE, 51)));
    // Cost with loaded ammunition: 200 + floor(10 x 50 / 20) = 225.
    assert_eq!(weapon_cost(200, 10, 50, 20), (225, false));
    assert_eq!(weapon_cost(200, 0, 50, 20), (200, false));
    assert_eq!(weapon_cost(200, 10, 50, 0), (200, true));
    // Usability: a two-handed rifle with the left arm crippled; skill 39.
    let crippled_left = 0x10;
    assert!(!weapon_usable(crippled_left, true, 40, || true, || 80));
    assert!(weapon_usable(crippled_left, false, 40, || true, || 80));
    assert!(!weapon_usable(0x30, false, 40, || true, || 80));
    assert!(!weapon_usable(0, false, 40, || true, || 39));
    assert!(!weapon_usable(0, false, 40, || false, || 80));
    // Attack-mode nibbles to subtypes; 9-15 are reported.
    assert_eq!(
        (0..=9).map(|n| subtype_of(n).0).collect::<Vec<_>>(),
        [0, 1, 1, 2, 2, 3, 4, 4, 4, 0]
    );
    assert!(subtype_of(9).1 && !subtype_of(8).1);
}

#[test]
fn spec009_fixture_e_hit_mode_called_shot_and_draws() {
    let thrown = ModeWeapon {
        secondary_subtype: subtype::THROW,
        usable_secondary: true,
    };
    let spear = Some(thrown);
    // Secondary gate 1, D10 4 against Int 6 succeeds: primary kept.
    let mut draws = Draws::new(&[1, 4]);
    assert_eq!(
        select_hit_mode(spear, 4, 6, &mut draws, || false),
        Ok(RIGHT_PRIMARY)
    );
    assert!(draws.0.is_empty());
    // D10 7 fails the check: throw.
    let mut draws = Draws::new(&[1, 7]);
    assert_eq!(
        select_hit_mode(spear, 4, 6, &mut draws, || false),
        Ok(RIGHT_SECONDARY)
    );
    // Another inventory weapon: no D10 draw, throw.
    let mut draws = Draws::new(&[1]);
    assert_eq!(
        select_hit_mode(spear, 4, 6, &mut draws, || true),
        Ok(RIGHT_SECONDARY)
    );
    assert!(draws.0.is_empty());
    // A gate result other than 1, no secondary, unusable secondary, none.
    let mut draws = Draws::new(&[3]);
    assert_eq!(
        select_hit_mode(spear, 4, 6, &mut draws, || false),
        Ok(RIGHT_PRIMARY)
    );
    let plain = ModeWeapon {
        secondary_subtype: subtype::NONE,
        usable_secondary: true,
    };
    let mut none = Draws::new(&[]);
    assert_eq!(
        select_hit_mode(Some(plain), 4, 6, &mut none, || false),
        Ok(RIGHT_PRIMARY)
    );
    let blocked = ModeWeapon {
        usable_secondary: false,
        ..thrown
    };
    assert_eq!(
        select_hit_mode(Some(blocked), 4, 6, &mut none, || false),
        Ok(RIGHT_PRIMARY)
    );
    assert_eq!(select_hit_mode(None, 4, 6, &mut none, || false), Ok(PUNCH));
    // Called shot: gate 1, location 0 at hit chance 32 < 40, so torso.
    let inputs = CalledShot {
        aimed_affordable: true,
        aiming_available: true,
        called_freq: 3,
        difficulty: 1,
        intelligence: 6,
        min_to_hit: 40,
    };
    let chance = |location: u8| {
        Ok(match location {
            0 => 32,
            1 => 41,
            _ => 60,
        })
    };
    let mut draws = Draws::new(&[1, 0]);
    assert_eq!(select_location(inputs, &mut draws, chance), Ok(TORSO));
    let mut draws = Draws::new(&[1, 8]);
    assert_eq!(select_location(inputs, &mut draws, chance), Ok(UNCALLED));
    let mut draws = Draws::new(&[1, 1]);
    assert_eq!(select_location(inputs, &mut draws, chance), Ok(1));
    // No draws when aimed AP is unaffordable or aiming is unavailable.
    for blocked in [
        CalledShot {
            aimed_affordable: false,
            ..inputs
        },
        CalledShot {
            aiming_available: false,
            ..inputs
        },
    ] {
        let mut none = Draws::new(&[]);
        assert_eq!(select_location(blocked, &mut none, chance), Ok(TORSO));
    }
    // A failed gate or a low Intelligence draws once.
    let mut draws = Draws::new(&[2]);
    assert_eq!(select_location(inputs, &mut draws, chance), Ok(TORSO));
    let mut draws = Draws::new(&[1]);
    let dull = CalledShot {
        intelligence: 4,
        ..inputs
    };
    assert_eq!(select_location(dull, &mut draws, chance), Ok(TORSO));
    assert!(draws.0.is_empty());
    assert_eq!(
        [0, 1, 2, 7, -1].map(called_shot_intelligence),
        [7, 5, 3, 5, 5]
    );
}

#[test]
fn spec009_secondary_and_called_frequencies_at_most_one_still_draw() {
    // The reversed/equal-range wrapper returns its first bound: always 1.
    let mut world_rng = crate::ParkMiller::seeded(7).unwrap();
    let before = world_rng.state().clone();
    let spear = Some(ModeWeapon {
        secondary_subtype: subtype::MELEE,
        usable_secondary: true,
    });
    for frequency in [0, 1] {
        assert_eq!(
            select_hit_mode(spear, frequency, 5, &mut world_rng, || false),
            Ok(RIGHT_SECONDARY)
        );
    }
    assert_ne!(world_rng.state(), &before, "each call consumed one draw");
}

#[test]
fn spec009_fixture_f_flee_candidate_order() {
    let candidates: Vec<_> = flee_candidates(1, 4).collect();
    assert_eq!(
        candidates[..6],
        [(4, 1), (4, 2), (4, 0), (3, 1), (3, 2), (3, 0)]
    );
    assert_eq!(candidates.len(), 12);
    assert_eq!(candidates[11], (1, 0));
    assert_eq!(flee_candidates(1, 0).count(), 0);
    assert_eq!(
        flee_candidates(5, 1).collect::<Vec<_>>(),
        [(1, 5), (1, 0), (1, 4)]
    );
    // A crafted AP word enumerates lazily: no allocation of 3 × AP tuples.
    assert_eq!(flee_candidates(0, i32::MAX).next(), Some((i32::MAX, 0)));
}

fn fixture_a() -> Packets {
    let text = "[Zeta Guard]\npacket_num=0\nmax_dist=8\nmin_to_hit=40\nmin_hp=12\naggression=50\nhurt_too_much=blind\nsecondary_freq=4\ncalled_freq=3\nfont=101\ncolor=145\noutline_color=-1\nchance=35\nrun_start=1000\nmove_start=1010\nattack_start=1020\nmiss_start=1030\nhit_head_start=1040\nhit_left_arm_start=1050\nhit_right_arm_start=1060\nhit_torso_start=1070\nhit_right_leg_start=1080\nhit_left_leg_start=1090\nhit_eyes_start=1100\nhit_groin_start=1110\nlast_msg=1119\n[alpha brute]\npacket_num=7\nmax_dist=6\nmin_to_hit=25\nmin_hp=0\naggression=0\nhurt_too_much=\nsecondary_freq=\ncalled_freq=0\nfont=101\ncolor=0\noutline_color=0\nchance=100\nrun_start=2000\nmove_start=2001\nattack_start=2002\nmiss_start=2003\nhit_head_start=2004\nhit_left_arm_start=2004\nhit_right_arm_start=2004\nhit_torso_start=2004\nhit_right_leg_start=2004\nhit_left_leg_start=2004\nhit_eyes_start=2004\nhit_groin_start=2004\nlast_msg=2003\n";
    Packets::parse(text.as_bytes()).unwrap()
}

#[test]
fn spec009_fixture_g_taunt_ranges() {
    let packets = fixture_a();
    let zeta = packets.resolve(0).0;
    for (taunt, range) in [
        (Taunt::Run, (1000, 1010)),
        (Taunt::Move, (1010, 1020)),
        (Taunt::Attack, (1020, 1030)),
        (Taunt::Miss, (1030, 1040)),
        (Taunt::Hit(0), (1040, 1050)),
        (Taunt::Hit(3), (1070, 1080)),
        (Taunt::Hit(4), (1080, 1090)),
        (Taunt::Hit(7), (1110, 1119)),
        // Location 8 is treated as torso.
        (Taunt::Hit(8), (1070, 1080)),
    ] {
        assert_eq!(taunt_range(zeta, taunt), range, "{taunt:?}");
    }
    let alpha = packets.resolve(7).0;
    let (start, end) = taunt_range(alpha, Taunt::Hit(7));
    assert!(end < start, "2004..2003 gives no line");
    assert_eq!(taunt_range(alpha, Taunt::Hit(0)), (2004, 2004));
    assert_eq!(
        [
            Taunt::Run,
            Taunt::Move,
            Taunt::Attack,
            Taunt::Miss,
            Taunt::Hit(2)
        ]
        .map(Taunt::buffer),
        [0, 0, 0, 1, 1]
    );
}

#[test]
fn spec009_fixture_h_retaliation_rating() {
    let rating = |melee_damage, weapons, armor_class, rateable| {
        retaliation_rating(Some(Rating {
            rateable,
            melee_damage,
            weapons,
            armor_class,
        }))
    };
    let q = rating(3, [Some(12), None], 10, true);
    let r1 = rating(5, [None, None], 20, true);
    // R1-rev3: R2 has AC 2, so max(2, 20) + 2 = 22, a tie with Q.
    let r2 = rating(2, [None, Some(20)], 2, true);
    let r3 = rating(9, [Some(30), None], 40, false);
    assert_eq!((q, r1, r2, r3), (22, 25, 22, 0));
    // The check switches only on a strictly greater rating, so the exact
    // tie keeps the current reference.
    assert!(r1 > q && r2 == q && r3 <= q);
    assert_eq!(retaliation_rating(None), 0);
    // A weapon maximum must exceed Melee Damage to replace it.
    assert_eq!(rating(12, [Some(12), Some(4)], 1, true), 13);
}

/// Fixture I's inventory: (PID, quantity) stacks, items of type drug.
struct Stacks {
    entries: Vec<(u32, u32)>,
    hp: i32,
    ap: i32,
    consume_ok: bool,
    log: Vec<&'static str>,
}
impl HealingInventory for Stacks {
    type Unit = u32;
    fn hp(&self) -> i32 {
        self.hp
    }
    fn ap(&self) -> i32 {
        self.ap
    }
    fn set_ap(&mut self, ap: i32) {
        self.ap = ap;
    }
    fn entry(&self, position: usize) -> Option<Option<u32>> {
        self.entries.get(position).map(|(pid, _)| Some(*pid))
    }
    fn take_one(&mut self, position: usize) -> Option<u32> {
        let (pid, count) = self.entries.get_mut(position)?;
        let pid = *pid;
        *count -= 1;
        if *count == 0 {
            self.entries.remove(position);
        }
        Some(pid)
    }
    fn consume(&mut self, _unit: &u32) -> Result<bool, WorldError> {
        if self.consume_ok {
            self.hp += 8;
        }
        Ok(self.consume_ok)
    }
    fn restore(&mut self, unit: u32) -> Result<(), WorldError> {
        self.log.push("forced back");
        match self.entries.iter_mut().find(|(pid, _)| *pid == unit) {
            Some((_, count)) => *count += 1,
            None => self.entries.push((unit, 1)),
        }
        Ok(())
    }
    fn used(&mut self, _unit: u32) -> Result<(), WorldError> {
        self.log.push("animate, print 5000, destroy");
        Ok(())
    }
}

#[test]
fn spec009_fixture_i_healing_loop() {
    let fresh = |consume_ok| Stacks {
        entries: vec![(0x35, 1), (STIMPAK, 2)],
        hp: 15,
        ap: 5,
        consume_ok,
        log: Vec::new(),
    };
    // Max HP 41: B = 20.
    let mut stacks = fresh(true);
    heal(&mut stacks, 41 / 2).unwrap();
    assert_eq!((stacks.hp, stacks.ap), (23, 3));
    assert_eq!(stacks.entries, [(0x35, 1), (STIMPAK, 1)]);
    assert_eq!(stacks.log, ["animate, print 5000, destroy"]);
    // Failing consumption spends AP until AP < 2; both stimpaks remain.
    let mut stacks = fresh(false);
    heal(&mut stacks, 20).unwrap();
    assert_eq!((stacks.hp, stacks.ap), (15, 1));
    assert_eq!(stacks.entries, [(0x35, 1), (STIMPAK, 2)]);
    assert_eq!(stacks.log, ["forced back", "forced back"]);
    // Super stimpaks qualify; at half HP or above nothing happens.
    let mut stacks = fresh(true);
    stacks.entries = vec![(SUPER_STIMPAK, 1)];
    heal(&mut stacks, 20).unwrap();
    assert_eq!(stacks.hp, 23);
    let mut stacks = fresh(true);
    stacks.hp = 20;
    heal(&mut stacks, 20).unwrap();
    assert_eq!(stacks.ap, 5);
}

/// Fixture J's attack-loop environment.
struct Loop {
    ap: i32,
    cost: i32,
    preflight: VecDeque<u8>,
    ranged: i32,
    no_range: i32,
    weapon: bool,
    approach_ok: bool,
    log: Vec<String>,
}
impl AttackEnv for Loop {
    fn disabled(&self) -> bool {
        false
    }
    fn preflight(&mut self, _mode: u8) -> Result<u8, WorldError> {
        Ok(self.preflight.pop_front().unwrap_or(0))
    }
    fn hit_chance(&mut self, _mode: u8, range_aware: bool) -> Result<i32, WorldError> {
        Ok(if range_aware {
            self.ranged
        } else {
            self.no_range
        })
    }
    fn attack(&mut self, _mode: u8) -> Result<bool, WorldError> {
        self.ap = (self.ap - self.cost).max(0);
        self.log.push(format!("attack, AP {}", self.ap));
        Ok(true)
    }
    fn attack_cost(&mut self, _mode: u8) -> Result<i32, WorldError> {
        Ok(self.cost)
    }
    fn ap(&self) -> i32 {
        self.ap
    }
    fn approach(&mut self, taunt: bool) -> Result<bool, WorldError> {
        self.log.push(format!("approach, taunt {taunt}"));
        Ok(self.approach_ok)
    }
    fn flee(&mut self) -> Result<(), WorldError> {
        self.log.push("flee".into());
        Ok(())
    }
    fn has_weapon(&self) -> bool {
        self.weapon
    }
    fn switch_weapon(&mut self) -> Result<Switched, WorldError> {
        self.log.push("switch".into());
        Ok(Switched {
            success: false,
            weapon: false,
            mode: PUNCH,
        })
    }
    fn reload(&mut self) -> Result<Option<Switched>, WorldError> {
        self.log.push("reload".into());
        Ok(None)
    }
}
fn pistol(ranged: i32, no_range: i32) -> Loop {
    Loop {
        ap: 8,
        cost: 4,
        preflight: VecDeque::new(),
        ranged,
        no_range,
        weapon: true,
        approach_ok: true,
        log: Vec::new(),
    }
}

#[test]
fn spec009_fixture_j_attack_loop_trace() {
    // 55 >= 40: attack (AP 4), cost 4 <= 4 continue; attack (AP 0), 4 > 0.
    let mut env = pistol(55, 55);
    assert_eq!(attack_loop(&mut env, RIGHT_PRIMARY, 40), Ok(false));
    assert_eq!(env.log, ["attack, AP 4", "attack, AP 0"]);
    assert_eq!(env.ap, 0);
    // Range-aware 30, no-range 45: approach, taunting only the first time.
    let mut env = pistol(30, 45);
    assert_eq!(attack_loop(&mut env, RIGHT_PRIMARY, 40), Ok(false));
    assert_eq!(env.log.len(), ATTACK_ATTEMPTS);
    assert_eq!(env.log[0], "approach, taunt true");
    assert!(
        env.log[1..]
            .iter()
            .all(|entry| entry == "approach, taunt false")
    );
    // No-range 35: flee immediately and return success.
    let mut env = pistol(30, 35);
    assert_eq!(attack_loop(&mut env, RIGHT_PRIMARY, 40), Ok(true));
    assert_eq!(env.log, ["flee"]);
    // A failed approach after a good no-range chance flees with success.
    let mut env = pistol(30, 45);
    env.approach_ok = false;
    assert_eq!(attack_loop(&mut env, RIGHT_PRIMARY, 40), Ok(true));
    assert_eq!(env.log, ["approach, taunt true", "flee"]);
    // Out of range with a weapon: approach; a failed approach fails.
    let mut env = pistol(55, 55);
    env.preflight = VecDeque::from([2]);
    env.approach_ok = false;
    assert_eq!(attack_loop(&mut env, RIGHT_PRIMARY, 40), Ok(false));
    assert_eq!(env.log, ["approach, taunt true"]);
    // Out of range unarmed: switching fails, so approach.
    let mut env = pistol(55, 55);
    env.weapon = false;
    env.preflight = VecDeque::from([2, 4, 4, 4, 4, 4, 4, 4, 4, 4]);
    assert_eq!(attack_loop(&mut env, PUNCH, 40), Ok(false));
    assert_eq!(env.log, ["switch", "approach, taunt true"]);
    // No ammo reloads; not enough AP switches and a failed switch fails.
    let mut env = pistol(55, 55);
    env.preflight = VecDeque::from([1, 3]);
    assert_eq!(attack_loop(&mut env, RIGHT_PRIMARY, 40), Ok(false));
    assert_eq!(env.log, ["reload", "switch"]);
    // A dead target consumes all ten attempts with no action.
    let mut env = pistol(55, 55);
    env.preflight = VecDeque::from([4; 10]);
    assert_eq!(attack_loop(&mut env, RIGHT_PRIMARY, 40), Ok(false));
    assert!(env.log.is_empty());
    // An aim-blocked attempt approaches.
    let mut env = pistol(55, 55);
    env.preflight = VecDeque::from([5]);
    env.approach_ok = false;
    assert_eq!(attack_loop(&mut env, RIGHT_PRIMARY, 40), Ok(false));
    assert_eq!(env.log, ["approach, taunt true"]);
}

/// SPEC-018 §4.7.1 (Revision 3): a Fallout 2 packet's taunt ranges end at
/// their own end keys; an end below its start is empty. Fallout 1's still
/// end at the next start.
#[test]
fn spec018_revision3_taunt_ranges_use_the_end_keys() {
    let mut packet = fixture_a().packets[1].clone();
    assert_eq!(taunt_range(&packet, Taunt::Run), (1000, 1010));
    assert_eq!(taunt_range(&packet, Taunt::Hit(7)), (1110, 1119));
    packet.ends = Some([
        1005, 1015, 1025, 1035, 1045, 1055, 1065, 1075, 1085, 1095, 1105, 1100,
    ]);
    assert_eq!(taunt_range(&packet, Taunt::Run), (1000, 1005));
    assert_eq!(taunt_range(&packet, Taunt::Move), (1010, 1015));
    assert_eq!(taunt_range(&packet, Taunt::Attack), (1020, 1025));
    assert_eq!(taunt_range(&packet, Taunt::Miss), (1030, 1035));
    assert_eq!(taunt_range(&packet, Taunt::Hit(0)), (1040, 1045));
    // An out-of-range location is the torso, as in SPEC-009.
    assert_eq!(taunt_range(&packet, Taunt::Hit(9)), (1070, 1075));
    let (start, end) = taunt_range(&packet, Taunt::Hit(7));
    assert!(end < start, "the groin range is empty");
}
