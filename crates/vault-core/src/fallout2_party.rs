//! SPEC-019 §2.5 and §3.14-§3.18 party control under the Fallout 2
//! profile, as rules that do not depend on the live world: the control
//! record's fields and presets (§3.15.1), the custom panel's cycling
//! (§3.15.3), the combat-AI deltas against SPEC-009 (§3.16) and the
//! party-member level schedule (§3.17). None of them makes a random draw.
//!
//! A control record is SPEC-018 §7.5's [`MemberRecord`]: its seven fields
//! are vocabulary indexes in SPEC-019 §2.5's order, which is also each
//! panel's order. The host applies these rules only to a party member other
//! than the player that has a record (`host_fallout2_party`); every rule
//! here reduces to SPEC-009 under the neutral record.
use crate::ai::{WeaponChoice, subtype};
use vault_formats::fallout2::{
    party::{ControlField, PartyMember},
    save::MemberRecord,
};

/// §2.6 the party list's capacity, the player included.
pub const CAPACITY: usize = 32;
/// §2.6 the stay-close leash: hexes from the player.
pub const STAY_CLOSE_LEASH: i32 = 5;
/// §2.6 the snipe minimum range.
pub const SNIPE_RANGE: i32 = 4;

/// `disposition` indexes (SPEC-018 §4.5).
pub mod disposition {
    pub const CUSTOM: i32 = 0;
    pub const COWARD: i32 = 1;
    pub const DEFENSIVE: i32 = 2;
    pub const AGGRESSIVE: i32 = 3;
    pub const BERSERK: i32 = 4;
}
/// `area_attack_mode` indexes.
pub mod area_attack {
    pub const ALWAYS: i32 = 0;
    pub const SOMETIMES: i32 = 1;
    pub const BE_SURE: i32 = 2;
    pub const BE_CAREFUL: i32 = 3;
    pub const BE_ABSOLUTELY_SURE: i32 = 4;
}
/// `run_away_mode` indexes.
pub mod run_away {
    pub const NONE: i32 = 0;
    pub const COWARD: i32 = 1;
    pub const FINGER_HURTS: i32 = 2;
    pub const BLEEDING: i32 = 3;
    pub const NOT_FEELING_GOOD: i32 = 4;
    pub const TOURNIQUET: i32 = 5;
    pub const NEVER: i32 = 6;
}
/// `best_weapon` indexes.
pub mod best_weapon {
    pub const NO_PREF: i32 = 0;
    pub const MELEE: i32 = 1;
    pub const MELEE_OVER_RANGED: i32 = 2;
    pub const RANGED_OVER_MELEE: i32 = 3;
    pub const RANGED: i32 = 4;
    pub const UNARMED: i32 = 5;
    pub const UNARMED_OVER_THROWN: i32 = 6;
    pub const RANDOM: i32 = 7;
}
/// `distance` indexes.
pub mod distance {
    pub const STAY_CLOSE: i32 = 0;
    pub const CHARGE: i32 = 1;
    pub const SNIPE: i32 = 2;
    pub const ON_YOUR_OWN: i32 = 3;
    pub const STAY: i32 = 4;
}
/// `attack_who` indexes.
pub mod attack_who {
    pub const WHOMEVER_ATTACKING_ME: i32 = 0;
    pub const STRONGEST: i32 = 1;
    pub const WEAKEST: i32 = 2;
    pub const WHOMEVER: i32 = 3;
    pub const CLOSEST: i32 = 4;
}
/// `chem_use` indexes.
pub mod chem_use {
    pub const CLEAN: i32 = 0;
    pub const STIMS_WHEN_HURT_LITTLE: i32 = 1;
    pub const STIMS_WHEN_HURT_LOTS: i32 = 2;
    pub const SOMETIMES: i32 = 3;
    pub const ANYTIME: i32 = 4;
    pub const ALWAYS: i32 = 5;
}

/// §2.5 the neutral record's seven fields.
pub fn neutral() -> [i32; 7] {
    ControlField::ALL.map(ControlField::neutral)
}
/// A field's value in a record.
pub fn value(record: &MemberRecord, field: ControlField) -> i32 {
    record.fields[field as usize]
}

/// §3.15.1 a disposition's preset of the six other fields, in §2.5 order
/// from `area_attack_mode`; `custom` (and any other value) has none.
pub fn preset(disposition: i32) -> Option<[i32; 6]> {
    use self::{area_attack as a, attack_who as w, best_weapon as b, chem_use as c};
    use self::{distance as d, run_away as r};
    Some(match disposition {
        disposition::COWARD => [
            a::BE_ABSOLUTELY_SURE,
            r::COWARD,
            b::RANGED,
            d::SNIPE,
            w::WHOMEVER_ATTACKING_ME,
            c::STIMS_WHEN_HURT_LITTLE,
        ],
        disposition::DEFENSIVE => [
            a::BE_SURE,
            r::BLEEDING,
            b::RANGED_OVER_MELEE,
            d::STAY_CLOSE,
            w::WHOMEVER_ATTACKING_ME,
            c::STIMS_WHEN_HURT_LITTLE,
        ],
        disposition::AGGRESSIVE => [
            a::SOMETIMES,
            r::NOT_FEELING_GOOD,
            b::NO_PREF,
            d::ON_YOUR_OWN,
            w::CLOSEST,
            c::STIMS_WHEN_HURT_LOTS,
        ],
        disposition::BERSERK => [
            a::ALWAYS,
            r::NEVER,
            b::MELEE_OVER_RANGED,
            d::CHARGE,
            w::STRONGEST,
            c::CLEAN,
        ],
        _ => return None,
    })
}

/// §3.15.2 choosing disposition row `disposition` on the control panel: a
/// token outside the member's allowed set is disabled and changes nothing.
/// Otherwise the disposition is set and, for any but `custom`, its preset
/// replaces the six other fields, whatever their allowed sets. Returns
/// whether the row was enabled.
pub fn choose_disposition(
    record: &mut MemberRecord,
    member: &PartyMember,
    disposition: i32,
) -> bool {
    if !member
        .allowed(ControlField::Disposition)
        .contains(&disposition)
    {
        return false;
    }
    record.fields[ControlField::Disposition as usize] = disposition;
    if let Some(values) = preset(disposition) {
        record.fields[1..].copy_from_slice(&values);
    }
    true
}

/// §3.15.3 one press of a custom-panel row's button: the field becomes the
/// first token of the allowed set after the current value in §2.5 order,
/// wrapping to the start of that order (a current value outside the set
/// counts by its own position). An empty set, or a search that wraps back
/// to the current value, changes nothing. Any change makes the disposition
/// `custom`. Returns whether the field changed.
pub fn cycle(record: &mut MemberRecord, member: &PartyMember, field: ControlField) -> bool {
    if field == ControlField::Disposition {
        return false;
    }
    let allowed = member.allowed(field);
    let count = field.words().len() as i32;
    let current = value(record, field);
    let start = current.clamp(-1, count - 1);
    let Some(next) = (1..=count)
        .map(|step| (start + step).rem_euclid(count))
        .find(|candidate| allowed.contains(candidate))
    else {
        return false;
    };
    if next == current {
        return false;
    }
    record.fields[field as usize] = next;
    record.fields[ControlField::Disposition as usize] = disposition::CUSTOM;
    true
}

/// §2.6 a run-away mode's fraction of maximum HP, in percent.
fn run_away_fraction(mode: i32) -> Option<i32> {
    match mode {
        run_away::COWARD => Some(100),
        run_away::FINGER_HURTS => Some(90),
        run_away::BLEEDING => Some(75),
        run_away::NOT_FEELING_GOOD => Some(50),
        run_away::TOURNIQUET => Some(25),
        _ => None,
    }
}

/// §3.16 rule 1, the forced-flee thresholds of SPEC-009 §3.6 step 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FleeTest {
    /// HP strictly below this flees; `None` disables the HP test.
    pub hp_below: Option<i32>,
    /// Whether the packet's `hurt_too_much` test applies.
    pub hurt_too_much: bool,
}
/// The flee test under `mode`: `none` keeps the packet's `min_hp`, the
/// fractional modes use `trunc(max HP × f / 100)`, and `never` disables
/// both the HP and the `hurt_too_much` test.
pub fn flee_test(mode: i32, max_hp: i32, min_hp: i32) -> FleeTest {
    match mode {
        run_away::NEVER => FleeTest {
            hp_below: None,
            hurt_too_much: false,
        },
        _ => FleeTest {
            hp_below: Some(match run_away_fraction(mode) {
                Some(fraction) => percent_of(max_hp, fraction),
                None => min_hp,
            }),
            hurt_too_much: true,
        },
    }
}
/// `trunc(max × p / 100)` (§2.1), in 64 bits.
pub fn percent_of(max: i32, percent: i32) -> i32 {
    (i64::from(max) * i64::from(percent) / 100) as i32
}

/// §3.16 rule 2 the healing step under `chem_use`: the threshold B below
/// which drugs are used and the per-turn limit of successful uses; `None`
/// is `clean`, with no healing step at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Healing {
    pub below: i32,
    pub limit: Option<u32>,
}
pub fn healing(chem_use: i32, max_hp: i32) -> Option<Healing> {
    let little = percent_of(max_hp, 75);
    let half = max_hp / 2;
    let (below, limit) = match chem_use {
        chem_use::CLEAN => return None,
        chem_use::STIMS_WHEN_HURT_LITTLE | chem_use::ALWAYS => (little, None),
        chem_use::SOMETIMES => (half, Some(1)),
        _ => (half, None),
    };
    Some(Healing { below, limit })
}

/// One hostile candidate of §3.16 rule 3, in snapshot order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Candidate {
    pub attacked_by_player: bool,
    /// SPEC-009 §3.4's retaliation rating.
    pub rating: i32,
    pub hp: i32,
}
/// §3.16 rule 3: the candidate index `attack_who` picks, or `None` for
/// SPEC-009's `danger(self)` (always under `whomever`, and whenever the
/// mode finds no candidate). Ties go to the first in order.
pub fn choose_target(attack_who: i32, candidates: &[Candidate]) -> Option<usize> {
    let first_best = |better: &dyn Fn(&Candidate, &Candidate) -> bool| {
        let mut best: Option<usize> = None;
        for (index, candidate) in candidates.iter().enumerate() {
            if best.is_none_or(|held| better(candidate, &candidates[held])) {
                best = Some(index);
            }
        }
        best
    };
    match attack_who {
        attack_who::WHOMEVER_ATTACKING_ME => candidates
            .iter()
            .position(|candidate| candidate.attacked_by_player),
        attack_who::CLOSEST => (!candidates.is_empty()).then_some(0),
        attack_who::STRONGEST => first_best(&|a, b| a.rating > b.rating),
        attack_who::WEAKEST => first_best(&|a, b| a.hp < b.hp),
        _ => None,
    }
}

/// §2.6 / §3.16 rule 5 the combat follow threshold: the member approaches
/// the player when farther than this; `stay` never follows.
pub fn follow_threshold(distance: i32) -> Option<i32> {
    match distance {
        distance::STAY_CLOSE => Some(3),
        distance::STAY => None,
        _ => Some(5),
    }
}

/// §3.16 rule 8 the rank of a weapon's primary subtype under `best_weapon`;
/// `no_pref` and `random` reproduce SPEC-009 §3.9.
pub fn weapon_rank(best_weapon: i32, primary_subtype: u8) -> i32 {
    use self::best_weapon as b;
    // Columns: ranged, throw, melee, unarmed; none is always 1.
    let row: [i32; 4] = match best_weapon {
        b::MELEE => [1, 1, 2, 1],
        b::MELEE_OVER_RANGED => [2, 1, 3, 1],
        b::RANGED_OVER_MELEE => [3, 1, 2, 1],
        b::RANGED => [2, 1, 1, 1],
        b::UNARMED => [1, 1, 1, 2],
        b::UNARMED_OVER_THROWN => [1, 2, 1, 3],
        _ => [3, 2, 1, 1],
    };
    match primary_subtype {
        subtype::RANGED => row[0],
        subtype::THROW => row[1],
        subtype::MELEE => row[2],
        subtype::UNARMED => row[3],
        _ => 1,
    }
}

/// §3.16 rule 8 SPEC-009 §3.9's preference rule with step 2 ranked by
/// `best_weapon`: the higher rank wins, and equal ranks go to step 3, a
/// strictly higher cost.
pub fn prefers_ranked(best_weapon: i32, incumbent: WeaponChoice, challenger: WeaponChoice) -> bool {
    let held = weapon_rank(best_weapon, incumbent.primary_subtype);
    let offered = weapon_rank(best_weapon, challenger.primary_subtype);
    if held != offered {
        return offered > held;
    }
    challenger.cost > incumbent.cost
}

/// §3.16 rule 9 the friendlies' exposure to a burst at T.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Exposure {
    /// Friendlies at risk.
    pub at_risk: usize,
    /// The nearest friendly's distance from T, if any friendly exists.
    pub nearest: Option<i32>,
}
/// §3.16 rule 9 the burst gate under `area_attack_mode`.
pub fn burst_allowed(mode: i32, exposure: Exposure) -> bool {
    let within = |hexes: i32| exposure.nearest.is_some_and(|nearest| nearest <= hexes);
    match mode {
        area_attack::SOMETIMES => exposure.at_risk <= 1,
        area_attack::BE_SURE => exposure.at_risk == 0,
        area_attack::BE_CAREFUL => exposure.at_risk == 0 && !within(3),
        area_attack::BE_ABSOLUTELY_SURE => exposure.at_risk == 0 && !within(5),
        _ => true,
    }
}
/// Whether an attack-mode nibble is burst (7) or continuous (8).
pub fn area_nibble(nibble: u8) -> bool {
    matches!(nibble, 7 | 8)
}

/// §3.17 the player level at which a member at stage `stage` advances.
pub fn level_required(member: &PartyMember, stage: i32) -> i64 {
    i64::from(member.level_minimum) + i64::from(stage) * i64::from(member.level_up_every)
}
/// §3.17 whether a member at `stage` advances one stage at player level
/// `level`: it has level PIDs and a nonzero interval, a stage below their
/// count, and the level reaches `L0 + s × E`.
pub fn advances(member: &PartyMember, stage: i32, level: i32) -> bool {
    member.level_up_every > 0
        && usize::try_from(stage).is_ok_and(|stage| stage < member.level_pids.len())
        && i64::from(level) >= level_required(member, stage)
}
/// §3.17 current HP after advancing: raised by the maximum's increase,
/// then clamped to 1 through the new maximum.
pub fn advanced_hp(old_max: i32, current: i32, new_max: i32) -> i32 {
    let raised = i64::from(current) + i64::from(new_max) - i64::from(old_max);
    raised.clamp(1, i64::from(new_max.max(1))) as i32
}

/// §2.3 the panels' engine text for value `index` of `field`: the token
/// with `_` as a space and its first letter capitalized.
pub fn value_text(field: ControlField, index: i32) -> String {
    let Some(word) = usize::try_from(index)
        .ok()
        .and_then(|index| field.words().get(index))
    else {
        return String::new();
    };
    let mut text = word.replace('_', " ");
    if let Some(first) = text.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    text
}

#[cfg(test)]
#[path = "fallout2_party_tests.rs"]
mod tests;
