//! SPEC-011 fixtures G and H and the perk mechanism: the experience table,
//! Swift Learner, level-up HP, reconciliation and the single-pick flag,
//! chooser extras, eligibility, and perk effects.
use crate::{
    CharacterSheet, Perk, StatContext, Trait, WorldEffect, WorldHost,
    fixture_items::{self as support, ItemAssets, messages},
    progression::{
        LEVEL_UP_FLAG, MAX_LEVEL, PERKS, PerkChoice, experience_award, perk_row, xp_threshold,
    },
};

fn context() -> StatContext {
    StatContext::default()
}
/// Fixture G's player: level 1, R(EN) 6, R(IN) 7, max HP 40 (no bonus).
fn fixture_g() -> WorldHost<ItemAssets> {
    let mut host = support::host();
    let sheet = host.world.player_sheet_mut().unwrap();
    sheet.base[2] = 6;
    sheet.base[4] = 7;
    sheet.recompute(context());
    sheet.base[7] = 40;
    sheet.current_hp = 40;
    sheet.perks.insert(Perk::SwiftLearner, 1);
    let player = host.world.player;
    host.world.sync_hit_points(player);
    host
}
fn sheet(host: &WorldHost<ItemAssets>) -> &CharacterSheet {
    host.world.player_sheet().unwrap()
}

#[test]
fn spec011_experience_table_and_award_rule() {
    let table: Vec<_> = (1..MAX_LEVEL).map(xp_threshold).collect();
    assert_eq!(
        table,
        [
            1000, 3000, 6000, 10000, 15000, 21000, 28000, 36000, 45000, 55000, 66000, 78000, 91000,
            105000, 120000, 136000, 153000, 171000, 190000, 210000
        ]
    );
    assert_eq!(xp_threshold(21), -1);
    assert_eq!(experience_award(2900, 1), 3045);
    assert_eq!(experience_award(-510, 1), -535, "the product truncates");
    assert_eq!(experience_award(99, 3), 113);
    let mut sheet = CharacterSheet::default();
    assert_eq!(sheet.add_experience(i32::MAX, context()), 20);
    assert_eq!((sheet.level, sheet.experience), (21, i32::MAX));
    assert_eq!(sheet.add_experience(-5, context()), 0);
    sheet.add_experience(i32::MIN, context());
    assert_eq!(
        (sheet.level, sheet.experience),
        (21, 0),
        "levels never drop"
    );
}

#[test]
fn spec011_fixture_g_experience_level_up_and_reconciliation() {
    let mut host = fixture_g();
    host.world.effects.clear();
    assert_eq!(host.give_experience(2900), 2);
    let player = host.world.player;
    let s = sheet(&host);
    assert_eq!((s.experience, s.level), (3045, 3));
    assert_eq!((s.current_hp, s.stat(7, context())), (45, 50));
    assert_eq!(host.world.hit_points(player), Some(45));
    assert_ne!(s.flags & LEVEL_UP_FLAG, 0);
    assert_eq!(messages(&host), ["level up", "level up"]);
    let sounds = host
        .world
        .effects
        .iter()
        .filter(
            |effect| matches!(effect, WorldEffect::SoundEffect { name, .. } if name == b"levelup"),
        )
        .count();
    assert_eq!(sounds, 2);

    // Skill points and the perk flag wait for the character screen.
    assert_eq!(sheet(&host).unspent_skills, 0);
    let sheet_mut = host.world.player_sheet_mut().unwrap();
    assert!(sheet_mut.reconcile_levels(context()), "3 mod 3 = 0");
    assert_eq!(
        (sheet_mut.unspent_skills, sheet_mut.reconciled_level),
        (38, 3)
    );
    assert!(
        sheet_mut.reconcile_levels(context()),
        "nothing new, but an unused pick stays pending"
    );
    assert_eq!(sheet_mut.unspent_skills, 38);

    // A negative award truncates toward zero and never lowers the level.
    assert_eq!(host.give_experience(-510), 0);
    assert_eq!((sheet(&host).experience, sheet(&host).level), (2510, 3));

    // Points above 99 are lost.
    let sheet_mut = host.world.player_sheet_mut().unwrap();
    sheet_mut.unspent_skills = 90;
    sheet_mut.level = 4;
    sheet_mut.reconcile_levels(context());
    assert_eq!(sheet_mut.unspent_skills, 99);
}

#[test]
fn spec011_reconciliation_grants_one_pick_and_respects_skilled_gifted() {
    // Levels 3 and 6 both qualify from last = 2 to level 7: one pick.
    let mut sheet = CharacterSheet {
        level: 7,
        reconciled_level: 2,
        ..CharacterSheet::default()
    };
    assert!(sheet.reconcile_levels(context()));
    assert_eq!(
        sheet.choose_perk(Perk::Awareness, context()),
        PerkChoice::Granted
    );
    assert!(!sheet.pending_perk);
    assert_eq!(
        sheet.choose_perk(Perk::Toughness, context()),
        PerkChoice::Refused
    );
    // Skilled: only multiples of four; n = 3 grants nothing, n = 4 does.
    let mut skilled = CharacterSheet {
        level: 3,
        reconciled_level: 2,
        traits: [Trait::Skilled as i32, -1],
        ..CharacterSheet::default()
    };
    assert!(!skilled.reconcile_levels(context()));
    skilled.level = 4;
    assert!(skilled.reconcile_levels(context()));
    // Gifted subtracts 5 (floored at zero); Skilled adds no points.
    let mut gifted = CharacterSheet {
        level: 2,
        traits: [Trait::Gifted as i32, Trait::Skilled as i32],
        ..CharacterSheet::default()
    };
    gifted.reconcile_levels(context());
    assert_eq!(gifted.unspent_skills, 5 + 2 * 6 - 5);
    // Seven distinct perks: no further pick.
    let mut collector = CharacterSheet {
        level: 3,
        reconciled_level: 2,
        ..CharacterSheet::default()
    };
    for perk in &Perk::ALL[..7] {
        collector.perks.insert(*perk, 1);
    }
    assert!(!collector.reconcile_levels(context()));
}

#[test]
fn spec011_chooser_extras_and_player_stat_setter() {
    let mut host = fixture_g();
    let player = host.world.player;
    let sheet_mut = host.world.player_sheet_mut().unwrap();
    sheet_mut.level = 12;
    sheet_mut.pending_perk = true;
    sheet_mut.current_hp = 30;
    let before = sheet_mut.stat(7, context());
    assert_eq!(host.choose_perk(Perk::Lifegiver), PerkChoice::Granted);
    assert_eq!(sheet(&host).stat(7, context()), before + 4);
    assert_eq!(host.world.hit_points(player), Some(34), "heal 4");
    // Educated: +2 without the 99 cap.
    let sheet_mut = host.world.player_sheet_mut().unwrap();
    sheet_mut.pending_perk = true;
    sheet_mut.unspent_skills = 99;
    assert_eq!(host.choose_perk(Perk::Educated), PerkChoice::Granted);
    assert_eq!(sheet(&host).unspent_skills, 101);
    // Tag! opens the fourth tag; cancelling removes the rank again and
    // keeps the pick.
    host.world.player_sheet_mut().unwrap().pending_perk = true;
    assert_eq!(host.choose_perk(Perk::Tag), PerkChoice::TagSelection);
    let sheet_mut = host.world.player_sheet_mut().unwrap();
    sheet_mut.cancel_perk_choice(Perk::Tag, context());
    assert_eq!(
        (sheet_mut.rank(Perk::Tag), sheet_mut.pending_perk),
        (0, true)
    );
    assert_eq!(host.choose_perk(Perk::Mutate), PerkChoice::TraitSelection);

    // The player-stat setter: selectors 0-4 within bounds; XP re-runs the
    // level loop; anything else is rejected.
    let mut host = fixture_g();
    assert!(!host.set_pc_stat(5, 1));
    assert!(host.set_pc_stat(3, 50));
    assert_eq!(sheet(&host).reputation, 20);
    host.world.effects.clear();
    assert!(host.set_pc_stat(2, 10000));
    assert_eq!(sheet(&host).level, 5);
    assert_eq!(messages(&host).len(), 4);
    assert!(host.set_pc_stat(1, -3));
    assert_eq!(sheet(&host).level, 1);
}

#[test]
fn spec011_fixture_h_perk_eligibility() {
    let mut sheet = CharacterSheet {
        level: 6,
        ..CharacterSheet::default()
    };
    sheet.base[..7].copy_from_slice(&[5, 7, 5, 5, 6, 6, 5]);
    sheet.invested[8] = 24;
    sheet.invested[17] = 35;
    sheet.invested[15] = 28;
    sheet.recompute(context());
    let c = context();
    assert_eq!([8, 17, 15].map(|skill| sheet.skill(skill, c)), [55, 45, 58]);
    for (perk, eligible) in [
        (Perk::Awareness, true),
        (Perk::SilentRunning, true),
        (Perk::Sharpshooter, true),
        (Perk::Survivalist, false),
        (Perk::Ghost, false),
        (Perk::MasterTrader, false),
        (Perk::Pickpocket, false),
        (Perk::Educated, true),
        (Perk::NukaColaCraving, false),
    ] {
        assert_eq!(sheet.perk_eligible(perk, c), eligible, "{perk:?}");
    }
    // Effective IN 5 fails both IN 6 perks; a drug's IN +1 restores them.
    sheet.base[4] = 5;
    assert!(!sheet.perk_eligible(Perk::Sharpshooter, c) && !sheet.perk_eligible(Perk::Educated, c));
    sheet.adjust_bonus(4, 1, c);
    assert!(sheet.perk_eligible(Perk::Sharpshooter, c) && sheet.perk_eligible(Perk::Educated, c));
    // The script path uses the same test and fails silently.
    assert!(!sheet.add_perk_rank(Perk::Ghost, c));
    assert!(sheet.add_perk_rank(Perk::Educated, c));
    sheet.set_perk(Perk::Educated, 3, c);
    assert!(!sheet.perk_eligible(Perk::Educated, c), "maximum rank");
    // Choices skip ineligible perks and the effect-only indices.
    let choices = sheet.perk_choices(c);
    assert!(choices.contains(&Perk::Awareness) && !choices.contains(&Perk::Ghost));
    assert!(choices.iter().all(|perk| perk.selectable()));
}

#[test]
fn spec011_perk_table_and_effect_mechanism() {
    assert_eq!(PERKS.len(), 64);
    assert!(
        Perk::ALL
            .iter()
            .enumerate()
            .all(|(index, perk)| perk.index() == index && perk.selectable() == (index < 53))
    );
    let row = perk_row(Perk::Pickpocket);
    assert_eq!(
        (row.max_rank, row.min_level, row.skill),
        (1, 9, Some((8, 80)))
    );
    assert_eq!(perk_row(Perk::Slayer).skill, Some((3, 80)));
    assert_eq!(perk_row(Perk::Sniper).skill, Some((0, 80)));
    assert_eq!(perk_row(Perk::MasterTrader).skill, Some((15, 60)));
    assert_eq!(perk_row(Perk::MentalBlock).min_level, 15);
    assert_eq!(Perk::from_index(52), Some(Perk::Mutate));
    assert_eq!(Perk::from_index(64), None);
    assert_eq!(Perk::from_index(-1), None);
    assert_eq!(Perk::from_index(62), Some(Perk::PoweredArmor));

    let c = context();
    let mut sheet = CharacterSheet::default();
    let special = |sheet: &CharacterSheet| [0, 2, 4, 5].map(|stat| sheet.bonus[stat]);
    // Effects stack: two Buffout Craving applications double the penalty.
    sheet.perk_effect(Perk::BuffoutCraving, true, c);
    sheet.perk_effect(Perk::BuffoutCraving, true, c);
    assert_eq!(special(&sheet), [-4, -4, 0, -6]);
    sheet.perk_effect(Perk::BuffoutCraving, false, c);
    sheet.perk_effect(Perk::BuffoutCraving, false, c);
    assert_eq!(special(&sheet), [0; 4]);
    sheet.perk_effect(Perk::MentatsCraving, true, c);
    assert_eq!(special(&sheet), [0, 0, -3, -2]);
    sheet.perk_effect(Perk::PoweredArmor, true, c);
    sheet.perk_effect(Perk::RadawayCraving, true, c);
    assert_eq!((sheet.bonus[0], sheet.bonus[31]), (3, 10));
    let before = sheet.bonus;
    for perk in [
        Perk::NukaColaCraving,
        Perk::WeaponLongRange,
        Perk::WeaponKnockback,
    ] {
        sheet.perk_effect(perk, true, c);
    }
    assert_eq!(sheet.bonus, before, "description-only and weapon perks");
    // Ranks apply per rank: Bonus HtH Damage +2 melee damage each.
    sheet.set_perk(Perk::BonusHthDamage, 3, c);
    assert_eq!(sheet.bonus[11], 6);
    sheet.set_perk(Perk::BonusHthDamage, 1, c);
    assert_eq!(sheet.bonus[11], 2);
    sheet.set_perk(Perk::BonusHthDamage, 0, c);
    assert!(!sheet.perks.contains_key(&Perk::BonusHthDamage));
    // Out-of-range indices do nothing through the world entry point.
    let mut host = support::host();
    let player = host.world.player;
    let before = sheet_bonus(&host);
    host.world.perk_effect(player, 64, true);
    host.world.perk_effect(player, -1, true);
    assert_eq!(sheet_bonus(&host), before);
    // Weapon perk numbers 58-61 reach the SPEC-007 attack rules.
    use crate::world::WeaponPerk;
    let rules = &host.world.rules;
    assert_eq!(
        (58..62)
            .map(|perk| rules.weapon_perk(perk))
            .collect::<Vec<_>>(),
        [
            Some(WeaponPerk::LongRange),
            Some(WeaponPerk::Accurate),
            Some(WeaponPerk::Penetrate),
            Some(WeaponPerk::Knockback)
        ]
    );
    assert_eq!(rules.weapon_perk(57), None);
}
fn sheet_bonus(host: &WorldHost<ItemAssets>) -> [i32; 35] {
    host.world.player_sheet().unwrap().bonus
}

#[test]
fn spec011_chooser_lists_eligible_perks_by_display_name() {
    let mut host = support::host();
    host.world.player_sheet_mut().unwrap().level = 3;
    let choices = host.perk_choices();
    assert!(!choices.is_empty());
    let names: Vec<_> = choices.iter().map(|(_, name)| name.clone()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "display-name order");
    // The placeholder names run opposite to the perk numbers.
    let indices: Vec<_> = choices.iter().map(|(perk, _)| perk.index()).collect();
    assert!(indices.windows(2).all(|pair| pair[0] > pair[1]));
}
