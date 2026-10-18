//! SPEC-011 §3.12 and §3.20-3.22 character progression: the experience
//! table, level-up HP, the player-stat setter, skill-point and perk-timing
//! reconciliation, the perk table with its eligibility test, the perk-effect
//! mechanism, and the chooser's immediate extras.
use crate::{CharacterSheet, Perk, StatContext, Trait, stats::saturated};

pub const MAX_LEVEL: i32 = 21;
/// SPEC-006 §2.2 player flag set by each level advancement.
pub const LEVEL_UP_FLAG: u32 = 0x08;
/// SPEC-006 §2.2 player flag set by a successful addiction roll.
pub const ADDICTED_FLAG: u32 = 0x10;
/// Skill points above this are lost at reconciliation.
pub const SKILL_POINT_CAP: i64 = 99;
/// A perk pick is granted only while fewer distinct perks are held.
pub const DISTINCT_PERK_LIMIT: usize = 7;

/// Total experience required to leave `level` (1-20); -1 at level 21 and
/// for any level outside the table.
pub fn xp_threshold(level: i32) -> i32 {
    if (1..MAX_LEVEL).contains(&level) {
        1000 * level * (level + 1) / 2
    } else {
        -1
    }
}

/// §3.20 step 1: `x + trunc(Swift Learner rank × 5 × x / 100)`, with the
/// product formed first so that a negative award truncates toward zero.
pub fn experience_award(x: i32, swift_learner: u32) -> i64 {
    let x = i64::from(x);
    x + i64::from(swift_learner) * 5 * x / 100
}

/// One perk-table row. For the non-selectable effect perks 53-63 the
/// requirement columns hold per-attribute SPECIAL bonuses instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerkRow {
    pub max_rank: u32,
    pub min_level: i32,
    /// Effective SPECIAL minimums in ST, PE, EN, CH, IN, AG, LK order.
    pub special: [i32; 7],
    /// Skill index and minimum effective percentage.
    pub skill: Option<(usize, i32)>,
    /// Stat index and modifier applied per rank or effect.
    pub effect: Option<(usize, i32)>,
}
const fn row(
    max_rank: u32,
    min_level: i32,
    special: [i32; 7],
    skill: Option<(usize, i32)>,
    effect: Option<(usize, i32)>,
) -> PerkRow {
    PerkRow {
        max_rank,
        min_level,
        special,
        skill,
        effect,
    }
}
/// Requirement or SPECIAL-effect columns, ST through LK.
const fn s(st: i32, pe: i32, en: i32, ch: i32, int: i32, ag: i32, lk: i32) -> [i32; 7] {
    [st, pe, en, ch, int, ag, lk]
}
const NONE: [i32; 7] = [0; 7];
const SNEAK: usize = 8;
const OUTDOORSMAN: usize = 17;
/// The effect-only rows: no rank, level, or skill columns apply.
const fn effect(special: [i32; 7], effect: Option<(usize, i32)>) -> PerkRow {
    row(0, 0, special, None, effect)
}

/// SPEC-011 §3.22, indexed by perk number. Skills use SPEC-003 order (0
/// Small Guns, 3 Unarmed, 6 First Aid, 8 Sneak, 15 Barter, 17 Outdoorsman).
pub const PERKS: [PerkRow; 64] = [
    row(1, 3, s(0, 5, 0, 0, 0, 0, 0), None, None),
    row(1, 6, s(0, 0, 0, 0, 0, 6, 0), None, None),
    row(3, 3, s(6, 0, 0, 0, 0, 6, 0), None, Some((11, 2))),
    row(3, 6, s(0, 0, 0, 0, 0, 5, 0), None, None),
    row(2, 6, s(0, 0, 0, 0, 0, 6, 6), None, None),
    row(1, 9, s(0, 6, 0, 0, 6, 7, 0), None, None),
    row(3, 3, s(0, 6, 0, 0, 0, 0, 0), None, Some((13, 2))),
    row(3, 3, s(0, 0, 6, 0, 0, 0, 0), None, Some((14, 1))),
    row(3, 6, s(0, 0, 0, 0, 0, 0, 6), None, Some((15, 5))),
    row(3, 3, s(0, 6, 0, 0, 0, 0, 0), None, None),
    row(3, 3, s(0, 0, 0, 6, 0, 0, 0), None, None),
    row(3, 6, s(0, 0, 6, 0, 4, 0, 0), None, Some((31, 15))),
    row(3, 3, s(0, 0, 6, 0, 0, 0, 6), None, Some((24, 10))),
    row(3, 3, s(6, 0, 6, 0, 0, 0, 0), None, Some((12, 50))),
    row(2, 6, s(0, 7, 0, 0, 6, 0, 0), None, None),
    row(1, 6, s(0, 0, 0, 0, 0, 6, 0), Some((SNEAK, 50)), None),
    row(3, 3, s(0, 0, 6, 0, 6, 0, 0), Some((OUTDOORSMAN, 40)), None),
    row(1, 9, s(0, 0, 0, 7, 0, 0, 0), Some((15, 60)), None),
    row(3, 6, s(0, 0, 0, 0, 6, 0, 0), None, None),
    row(3, 3, s(0, 7, 0, 0, 5, 6, 0), Some((6, 40)), None),
    row(1, 6, s(0, 0, 0, 0, 0, 0, 8), None, None),
    row(1, 9, s(0, 6, 0, 0, 0, 4, 6), None, Some((16, 20))),
    row(1, 6, s(0, 7, 0, 0, 5, 0, 0), None, None),
    row(1, 18, s(8, 0, 0, 0, 0, 8, 0), Some((3, 80)), None),
    row(1, 18, s(0, 8, 0, 0, 0, 8, 0), Some((0, 80)), None),
    row(1, 18, s(0, 0, 0, 0, 0, 10, 0), Some((SNEAK, 80)), None),
    row(3, 12, s(0, 0, 0, 0, 0, 5, 0), None, Some((8, 1))),
    row(1, 15, NONE, None, None),
    row(3, 12, s(0, 0, 4, 0, 0, 0, 0), None, None),
    row(2, 9, s(0, 0, 0, 0, 0, 4, 0), None, Some((9, 5))),
    row(1, 6, s(0, 0, 3, 0, 0, 0, 0), None, Some((32, 25))),
    row(1, 12, NONE, None, None),
    row(1, 12, NONE, None, None),
    row(1, 12, NONE, None, None),
    row(1, 12, NONE, None, None),
    row(3, 6, NONE, None, None),
    row(1, 6, s(0, 4, 0, 0, 0, 0, 0), None, None),
    // Pickpocket requires Sneak, not Steal.
    row(1, 9, s(0, 0, 0, 0, 0, 8, 0), Some((SNEAK, 80)), None),
    row(1, 6, NONE, Some((SNEAK, 60)), None),
    row(1, 12, s(0, 0, 0, 10, 0, 0, 0), None, None),
    row(1, 9, s(0, 0, 0, 0, 0, 0, 8), None, None),
    row(1, 9, NONE, None, None),
    row(1, 9, s(0, 0, 5, 0, 0, 0, 0), None, None),
    row(2, 6, s(0, 0, 6, 0, 0, 0, 0), Some((OUTDOORSMAN, 40)), None),
    row(1, 9, s(0, 0, 0, 0, 5, 0, 0), Some((OUTDOORSMAN, 25)), None),
    row(1, 3, s(0, 8, 0, 0, 0, 0, 0), None, None),
    row(1, 6, s(0, 0, 0, 0, 0, 0, 7), None, None),
    row(3, 6, s(0, 6, 0, 0, 0, 0, 0), None, None),
    row(3, 3, s(0, 0, 0, 0, 0, 5, 0), None, None),
    row(3, 3, s(0, 0, 0, 0, 4, 0, 0), None, None),
    row(3, 3, s(0, 0, 0, 0, 4, 0, 0), None, None),
    row(1, 12, NONE, None, None),
    row(1, 9, NONE, None, None),
    // 53 Nuka-Cola Craving has a description only.
    effect(NONE, None),
    effect(s(-2, 0, -2, 0, 0, -3, 0), None),
    effect(s(0, 0, 0, 0, -3, -2, 0), None),
    effect(s(0, 0, 0, 0, -2, 0, 0), None),
    effect(NONE, Some((31, -20))),
    // 58-61 weapon perks act through the SPEC-007 attack rules.
    effect(NONE, None),
    effect(NONE, None),
    effect(NONE, None),
    effect(NONE, None),
    effect(s(3, 0, 0, 0, 0, 0, 0), Some((31, 30))),
    effect(NONE, Some((31, 20))),
];

pub fn perk_row(perk: Perk) -> &'static PerkRow {
    &PERKS[perk.index()]
}

/// §3.12 bonus changes of one application: the effect stat's modifier,
/// then (non-selectable perks only) each nonzero per-attribute value.
pub fn perk_changes(perk: Perk) -> impl Iterator<Item = (usize, i32)> {
    let row = perk_row(perk);
    let special = if perk.selectable() {
        [0; 7]
    } else {
        row.special
    };
    row.effect.into_iter().chain(
        special
            .into_iter()
            .enumerate()
            .filter(|(_, amount)| *amount != 0),
    )
}

/// The immediate extra of a perk chosen in the character-screen chooser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerkChoice {
    /// Not pending, or the perk failed eligibility; nothing changed.
    Refused,
    /// The rank (and any Lifegiver/Educated extra) is granted.
    Granted,
    /// Tag! opens the fourth-tag selection; cancelling it removes the rank.
    TagSelection,
    /// Mutate! opens trait replacement; cancelling it removes the rank.
    TraitSelection,
}

impl CharacterSheet {
    /// Perks with a selected rank above zero.
    pub fn distinct_perks(&self) -> usize {
        self.perks.values().filter(|rank| **rank > 0).count()
    }
    /// §3.21 eligibility: selectable, below the maximum rank, at or above
    /// the minimum level, the skill percentage, and each effective SPECIAL
    /// (temporary drug and armor bonuses included) at its minimum.
    pub fn perk_eligible(&self, perk: Perk, context: StatContext) -> bool {
        let row = perk_row(perk);
        perk.selectable()
            && self.rank(perk) < row.max_rank
            && self.level >= row.min_level
            && row
                .skill
                .is_none_or(|(skill, percent)| self.skill(skill, context) >= percent)
            && (0..7).all(|index| self.stat(index, context) >= row.special[index])
    }
    /// Add one rank through the shared eligibility test (the chooser and
    /// `critter_add_trait` kind 0); a failing perk is silently refused.
    pub fn add_perk_rank(&mut self, perk: Perk, context: StatContext) -> bool {
        if !self.perk_eligible(perk, context) {
            return false;
        }
        self.set_perk(perk, self.rank(perk) + 1, context);
        true
    }
    /// One §3.20 level step: the level and pending-level-up flag, then
    /// `h = trunc(R(EN) / 2) + 2 + 4 × Lifegiver` healed up to the old
    /// maximum and added to the maximum-HP bonus.
    pub fn advance_level(&mut self, context: StatContext) -> bool {
        if self.level >= MAX_LEVEL {
            return false;
        }
        self.level += 1;
        self.flags |= LEVEL_UP_FLAG;
        let hp =
            saturated(self.raw(2, context) / 2 + 2 + 4 * i64::from(self.rank(Perk::Lifegiver)));
        self.heal(hp, context);
        self.adjust_bonus(7, hp, context);
        true
    }
    /// The level loop of §3.20 step 3; levels never decrease.
    fn advance_levels(&mut self, context: StatContext) -> u32 {
        let mut gained = 0;
        while (1..MAX_LEVEL).contains(&self.level) && self.experience >= xp_threshold(self.level) {
            self.advance_level(context);
            gained += 1;
        }
        gained
    }
    /// §3.20 experience award with Swift Learner and the 0..i32::MAX clamp;
    /// returns the number of levels gained.
    pub fn add_experience(&mut self, x: i32, context: StatContext) -> u32 {
        let award = experience_award(x, self.rank(Perk::SwiftLearner));
        self.experience = (i64::from(self.experience) + award).clamp(0, i64::from(i32::MAX)) as i32;
        self.advance_levels(context)
    }
    /// The player-stat setter for selectors 0-4 (unspent points, level,
    /// experience, reputation, karma) within the §2.5 bounds. Setting the
    /// experience re-runs the level loop. Safe rule: other selectors are
    /// rejected (`None`); otherwise the levels gained are returned.
    pub fn set_pc_stat(&mut self, index: i32, value: i32, context: StatContext) -> Option<u32> {
        match index {
            0 => self.unspent_skills = value.max(0),
            1 => self.level = value.max(1),
            2 => {
                self.experience = value.max(0);
                return Some(self.advance_levels(context));
            }
            3 => self.reputation = value.clamp(-20, 20),
            4 => self.karma = value.max(0),
            _ => return None,
        }
        Some(0)
    }
    /// §3.21 character-screen reconciliation from the last reconciled level
    /// to the current one: skill points per level (Gifted -5 floored at 0,
    /// capped at 99) and the pending-perk flag at every third (fourth with
    /// Skilled) level while fewer than seven distinct perks are held.
    /// Returns whether the perk chooser opens; the reconciled level then
    /// becomes the current level. Cancelling the screen restores a saved
    /// copy of the sheet, so the reconciliation repeats.
    pub fn reconcile_levels(&mut self, context: StatContext) -> bool {
        if self.level != self.reconciled_level && self.level <= MAX_LEVEL {
            let interval = if self.has_trait(Trait::Skilled) { 4 } else { 3 };
            for level in self.reconciled_level.saturating_add(1)..=self.level {
                let mut points = i64::from(self.unspent_skills)
                    + 5
                    + 2 * self.raw(4, context)
                    + 2 * i64::from(self.rank(Perk::Educated));
                if self.has_trait(Trait::Gifted) {
                    points = (points - 5).max(0);
                }
                self.unspent_skills = points.clamp(0, SKILL_POINT_CAP) as i32;
                if self.distinct_perks() < DISTINCT_PERK_LIMIT && level % interval == 0 {
                    self.pending_perk = true;
                }
            }
        }
        self.reconciled_level = self.level;
        self.pending_perk
    }
    /// Eligible perks for the chooser (the host orders them by display
    /// name).
    pub fn perk_choices(&self, context: StatContext) -> Vec<Perk> {
        Perk::ALL
            .into_iter()
            .filter(|perk| self.perk_eligible(*perk, context))
            .collect()
    }
    /// §3.21 choosing a perk: one rank, clearing the pending byte, then the
    /// first matching immediate extra.
    pub fn choose_perk(&mut self, perk: Perk, context: StatContext) -> PerkChoice {
        if !self.pending_perk || !self.add_perk_rank(perk, context) {
            return PerkChoice::Refused;
        }
        self.pending_perk = false;
        match perk {
            Perk::Tag => PerkChoice::TagSelection,
            Perk::Mutate => PerkChoice::TraitSelection,
            Perk::Lifegiver => {
                self.adjust_bonus(7, 4, context);
                self.heal(4, context);
                PerkChoice::Granted
            }
            Perk::Educated => {
                self.unspent_skills = self.unspent_skills.saturating_add(2);
                PerkChoice::Granted
            }
            _ => PerkChoice::Granted,
        }
    }
    /// A cancelled Tag!/Mutate! sub-selection removes the rank again; the
    /// pick was not made, so the pending byte stays for the next choice.
    pub fn cancel_perk_choice(&mut self, perk: Perk, context: StatContext) {
        if matches!(perk, Perk::Tag | Perk::Mutate) && self.rank(perk) > 0 {
            self.set_perk(perk, self.rank(perk) - 1, context);
            self.pending_perk = true;
        }
    }
}
