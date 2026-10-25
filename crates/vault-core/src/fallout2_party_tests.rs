//! SPEC-019 §4 vectors D-K on the pure party rules (the live-world halves
//! are in `host_ai_tests` and `spec019_tests`).
use super::*;
use crate::ai::{InventoryWeapon, WeaponChoice, inventory_best, inventory_best_by};
use vault_formats::fallout2::{party::PartyMember, save::MemberRecord};

fn neutral_record() -> MemberRecord {
    MemberRecord {
        record: 0,
        stage: 0,
        fields: neutral(),
    }
}
fn member(allowed: [Vec<i32>; 7]) -> PartyMember {
    PartyMember {
        number: 0,
        pid: 0x0100_0050,
        allowed,
        level_minimum: 3,
        level_up_every: 3,
        level_pids: vec![0x0100_0051, 0x0100_0052, 0x0100_0053],
    }
}

#[test]
fn spec019_neutral_record_is_the_spec009_record() {
    // custom, always, none, no_pref, on_your_own, whomever, anytime.
    assert_eq!(neutral(), [0, 0, 0, 0, 3, 3, 4]);
    let record = neutral_record();
    let test = flee_test(value(&record, ControlField::RunAwayMode), 60, 10);
    assert_eq!(
        test,
        FleeTest {
            hp_below: Some(10),
            hurt_too_much: true
        }
    );
    assert_eq!(
        healing(value(&record, ControlField::ChemUse), 61),
        Some(Healing {
            below: 30,
            limit: None
        })
    );
    assert_eq!(
        follow_threshold(value(&record, ControlField::Distance)),
        Some(5)
    );
    assert_eq!(choose_target(attack_who::WHOMEVER, &[]), None);
    assert!(burst_allowed(
        area_attack::ALWAYS,
        Exposure {
            at_risk: 9,
            nearest: Some(0)
        }
    ));
}

#[test]
fn spec019_vector_d_run_away_thresholds() {
    // Max HP 60, HP 44, packet min_hp 10, hurt_too_much crippled legs.
    let flees = |mode: i32, crippled: bool| {
        let test = flee_test(mode, 60, 10);
        (test.hurt_too_much && crippled) || test.hp_below.is_some_and(|below| 44 < below)
    };
    assert_eq!(flee_test(run_away::NONE, 60, 10).hp_below, Some(10));
    assert_eq!(flee_test(run_away::BLEEDING, 60, 10).hp_below, Some(45));
    assert_eq!(
        flee_test(run_away::NOT_FEELING_GOOD, 60, 10).hp_below,
        Some(30)
    );
    assert_eq!(
        flee_test(run_away::NEVER, 60, 10),
        FleeTest {
            hp_below: None,
            hurt_too_much: false
        }
    );
    let modes = [
        run_away::NONE,
        run_away::BLEEDING,
        run_away::NOT_FEELING_GOOD,
        run_away::NEVER,
    ];
    assert_eq!(
        modes.map(|mode| flees(mode, true)),
        [true, true, true, false]
    );
    assert_eq!(
        modes.map(|mode| flees(mode, false)),
        [false, true, false, false]
    );
    // The other fractions of §2.6.
    assert_eq!(flee_test(run_away::COWARD, 60, 10).hp_below, Some(60));
    assert_eq!(flee_test(run_away::FINGER_HURTS, 60, 10).hp_below, Some(54));
    assert_eq!(flee_test(run_away::TOURNIQUET, 60, 10).hp_below, Some(15));
}

#[test]
fn spec019_vector_e_chem_thresholds() {
    assert_eq!(healing(chem_use::CLEAN, 61), None);
    assert_eq!(healing(chem_use::ANYTIME, 61).unwrap().below, 30);
    assert_eq!(
        healing(chem_use::STIMS_WHEN_HURT_LITTLE, 61),
        Some(Healing {
            below: 45,
            limit: None
        })
    );
    assert_eq!(
        healing(chem_use::STIMS_WHEN_HURT_LOTS, 61).unwrap().below,
        30
    );
    assert_eq!(
        healing(chem_use::SOMETIMES, 61),
        Some(Healing {
            below: 30,
            limit: Some(1)
        })
    );
    assert_eq!(healing(chem_use::ALWAYS, 61).unwrap().below, 45);
}

#[test]
fn spec019_vector_f_attack_who() {
    // Candidates A and B (C unperceived, D dead are not candidates).
    let candidates = [
        Candidate {
            attacked_by_player: false,
            rating: 12,
            hp: 10,
        },
        Candidate {
            attacked_by_player: true,
            rating: 25,
            hp: 30,
        },
    ];
    assert_eq!(
        choose_target(attack_who::WHOMEVER_ATTACKING_ME, &candidates),
        Some(1)
    );
    assert_eq!(choose_target(attack_who::CLOSEST, &candidates), Some(0));
    assert_eq!(choose_target(attack_who::STRONGEST, &candidates), Some(1));
    assert_eq!(choose_target(attack_who::WEAKEST, &candidates), Some(0));
    assert_eq!(choose_target(attack_who::WHOMEVER, &candidates), None);
    // No candidate: danger(self) in every mode; ties go to the first.
    assert_eq!(choose_target(attack_who::CLOSEST, &[]), None);
    let tied = [candidates[0], candidates[0]];
    assert_eq!(choose_target(attack_who::STRONGEST, &tied), Some(0));
    assert_eq!(choose_target(attack_who::WEAKEST, &tied), Some(0));
}

#[test]
fn spec019_vector_g_weapon_ranking() {
    let entry = |subtype, cost| InventoryWeapon {
        right_hand: false,
        raw_primary_ap: 0,
        usable_primary: true,
        loaded: 1,
        ammo_available: true,
        choice: WeaponChoice {
            primary_subtype: subtype,
            cost,
        },
    };
    // Knife (melee, 40), pistol (ranged, 250, loaded), throwing knife (50).
    let entries = [
        entry(crate::ai::subtype::MELEE, 40),
        entry(crate::ai::subtype::RANGED, 250),
        entry(crate::ai::subtype::THROW, 50),
    ];
    let best = |mode: i32| {
        inventory_best_by(&entries, None, |held, offered| {
            prefers_ranked(mode, held, offered)
        })
    };
    assert_eq!(best(best_weapon::NO_PREF), Some(1), "pistol");
    assert_eq!(best(best_weapon::MELEE), Some(0), "knife");
    assert_eq!(best(best_weapon::RANGED), Some(1), "pistol");
    assert_eq!(
        best(best_weapon::UNARMED_OVER_THROWN),
        Some(2),
        "throwing knife"
    );
    assert_eq!(best(best_weapon::MELEE_OVER_RANGED), Some(0), "knife");
    // `no_pref` reproduces SPEC-009 over every pair of subtypes and costs.
    for held in 0..=4 {
        for offered in 0..=4 {
            for (held_cost, offered_cost) in [(10, 20), (20, 10), (10, 10)] {
                let held = WeaponChoice {
                    primary_subtype: held,
                    cost: held_cost,
                };
                let offered = WeaponChoice {
                    primary_subtype: offered,
                    cost: offered_cost,
                };
                assert_eq!(
                    prefers_ranked(best_weapon::NO_PREF, held, offered),
                    crate::ai::prefers_challenger(held, offered),
                    "{held:?} {offered:?}"
                );
                assert_eq!(
                    prefers_ranked(best_weapon::RANDOM, held, offered),
                    crate::ai::prefers_challenger(held, offered)
                );
            }
        }
    }
    assert_eq!(inventory_best(&entries, None), best(best_weapon::NO_PREF));
}

#[test]
fn spec019_vector_h_burst_gate() {
    // F next to the ray's third hex and 3 hexes from T: one at risk.
    let exposure = Exposure {
        at_risk: 1,
        nearest: Some(3),
    };
    assert!(burst_allowed(area_attack::ALWAYS, exposure));
    assert!(burst_allowed(area_attack::SOMETIMES, exposure));
    assert!(!burst_allowed(area_attack::BE_SURE, exposure));
    assert!(!burst_allowed(area_attack::BE_CAREFUL, exposure));
    assert!(!burst_allowed(area_attack::BE_ABSOLUTELY_SURE, exposure));
    // Nobody at risk: be_careful and be_absolutely_sure test the distance.
    let clear = |nearest| Exposure {
        at_risk: 0,
        nearest,
    };
    assert!(burst_allowed(area_attack::BE_SURE, clear(Some(1))));
    assert!(!burst_allowed(area_attack::BE_CAREFUL, clear(Some(3))));
    assert!(burst_allowed(area_attack::BE_CAREFUL, clear(Some(4))));
    assert!(!burst_allowed(
        area_attack::BE_ABSOLUTELY_SURE,
        clear(Some(5))
    ));
    assert!(burst_allowed(
        area_attack::BE_ABSOLUTELY_SURE,
        clear(Some(6))
    ));
    assert!(burst_allowed(area_attack::BE_ABSOLUTELY_SURE, clear(None)));
    assert!(!burst_allowed(
        area_attack::SOMETIMES,
        Exposure {
            at_risk: 2,
            nearest: Some(1)
        }
    ));
    assert!(area_nibble(7) && area_nibble(8) && !area_nibble(6));
}

#[test]
fn spec019_vector_i_follow_thresholds() {
    // Follow at dist(S, player) 4: stay_close (4 > 3) approaches,
    // on_your_own (4 <= 5) does not, stay never does.
    let follows = |mode: i32| follow_threshold(mode).is_some_and(|threshold| 4 > threshold);
    assert!(follows(distance::STAY_CLOSE));
    assert!(!follows(distance::ON_YOUR_OWN));
    assert!(!follows(distance::STAY));
    assert_eq!(follow_threshold(distance::CHARGE), Some(5));
    assert_eq!(follow_threshold(distance::SNIPE), Some(5));
    assert_eq!(STAY_CLOSE_LEASH, 5);
    assert_eq!(SNIPE_RANGE, 4);
}

#[test]
fn spec019_vector_j_dispositions_and_the_custom_panel() {
    let mut allowed: [Vec<i32>; 7] = Default::default();
    allowed[ControlField::Disposition as usize] = vec![
        disposition::CUSTOM,
        disposition::DEFENSIVE,
        disposition::AGGRESSIVE,
    ];
    allowed[ControlField::Distance as usize] = vec![
        distance::STAY_CLOSE,
        distance::ON_YOUR_OWN,
        distance::CHARGE,
    ];
    let member = member(allowed);
    let mut record = neutral_record();
    assert!(choose_disposition(
        &mut record,
        &member,
        disposition::DEFENSIVE
    ));
    assert_eq!(
        record.fields,
        [
            disposition::DEFENSIVE,
            area_attack::BE_SURE,
            run_away::BLEEDING,
            best_weapon::RANGED_OVER_MELEE,
            distance::STAY_CLOSE,
            attack_who::WHOMEVER_ATTACKING_ME,
            chem_use::STIMS_WHEN_HURT_LITTLE,
        ]
    );
    // Coward (key 2) is not allowed: the row is disabled.
    let before = record;
    assert!(!choose_disposition(
        &mut record,
        &member,
        disposition::COWARD
    ));
    assert_eq!(record, before);
    // The custom panel's Distance button: stay_close, charge, on_your_own,
    // then wrapping to stay_close; the disposition becomes custom.
    assert!(cycle(&mut record, &member, ControlField::Distance));
    assert_eq!(value(&record, ControlField::Distance), distance::CHARGE);
    assert_eq!(
        value(&record, ControlField::Disposition),
        disposition::CUSTOM
    );
    assert!(cycle(&mut record, &member, ControlField::Distance));
    assert_eq!(
        value(&record, ControlField::Distance),
        distance::ON_YOUR_OWN
    );
    assert!(cycle(&mut record, &member, ControlField::Distance));
    assert_eq!(value(&record, ControlField::Distance), distance::STAY_CLOSE);
    // An empty allowed set changes nothing; so does a set holding only the
    // current value.
    assert!(!cycle(&mut record, &member, ControlField::ChemUse));
    let mut single: [Vec<i32>; 7] = Default::default();
    single[ControlField::Distance as usize] = vec![distance::STAY_CLOSE];
    assert!(!cycle(
        &mut record,
        &self::member(single),
        ControlField::Distance
    ));
    // Presets ignore the allowed sets; custom changes nothing else.
    let mut berserk: [Vec<i32>; 7] = Default::default();
    berserk[0] = vec![disposition::BERSERK, disposition::CUSTOM];
    let berserk = self::member(berserk);
    assert!(choose_disposition(
        &mut record,
        &berserk,
        disposition::BERSERK
    ));
    assert_eq!(value(&record, ControlField::Distance), distance::CHARGE);
    let fields = record.fields;
    assert!(choose_disposition(
        &mut record,
        &berserk,
        disposition::CUSTOM
    ));
    assert_eq!(record.fields[1..], fields[1..]);
}

#[test]
fn spec019_vector_k_level_schedule_and_hp() {
    let member = member(Default::default());
    // L0 = 3, E = 3, three level PIDs.
    assert!(advances(&member, 0, 3));
    assert!(!advances(&member, 1, 4));
    assert!(advances(&member, 1, 6));
    // Joining at player level 12, one stage per gain, up to n = 3.
    let mut stage = 0;
    for level in 13..=16 {
        if advances(&member, stage, level) {
            stage += 1;
        }
    }
    assert_eq!(stage, 3);
    // E = 0 never advances.
    let still = PartyMember {
        level_up_every: 0,
        ..member.clone()
    };
    assert!(!advances(&still, 0, 99));
    assert_eq!(advanced_hp(40, 30, 48), 38);
    assert_eq!(advanced_hp(40, 1, 35), 1);
    assert_eq!(advanced_hp(40, 40, 30), 30);
}

#[test]
fn spec019_engine_value_text() {
    assert_eq!(
        value_text(
            ControlField::AreaAttackMode,
            area_attack::BE_ABSOLUTELY_SURE
        ),
        "Be absolutely sure"
    );
    assert_eq!(
        value_text(ControlField::Distance, distance::ON_YOUR_OWN),
        "On your own"
    );
    assert_eq!(value_text(ControlField::ChemUse, 99), "");
}
