//! SPEC-011 §2.2-2.3 and §3.2-3.8 pure drug rules: the addiction table,
//! stage slots with the random-range form, delayed-stage scheduling, the
//! player's trait scaling of addiction chance and stage delay, and the
//! withdrawal duration. `host_drugs` applies them to the world.
use crate::{RandomSource, WorldError};
use vault_formats::pro::Drug;

/// Game ticks per minute.
pub const TICKS_PER_MINUTE: i64 = 600;
/// Withdrawal lasts 10080 minutes (7 days) before trait scaling.
pub const WITHDRAWAL_MINUTES: i64 = 10080;
/// Stat selectors 35-37: current HP, poison level, radiation level.
pub const CURRENT_HP: i32 = 35;
pub const POISON: i32 = 36;
pub const RADIATION: i32 = 37;
/// S0 = -2 selects the random-range form; -1 is an unused slot.
pub const RANGE_FORM: i32 = -2;
/// Withdrawal payload kinds.
pub const ONSET: i32 = 1;
pub const END: i32 = 0;

/// §2.3 drug PID → campaign variable written on a player addiction.
pub const ADDICTION_TABLE: [(u32, usize); 7] = [
    (106, 189),
    (87, 190),
    (53, 191),
    (110, 192),
    (48, 193),
    (124, 194),
    (125, 194),
];
/// The campaign variables of the addiction table, 189-194.
pub fn addiction_variables() -> std::ops::RangeInclusive<usize> {
    189..=194
}

/// The mapped addiction variable of a drug PID; none ("-1") outside the
/// table, so two unlisted PIDs compare equal.
pub fn addiction_variable(pid: i32) -> Option<usize> {
    ADDICTION_TABLE
        .iter()
        .find(|(drug, _)| i64::from(*drug) == i64::from(pid))
        .map(|(_, variable)| *variable)
}

/// One effect group: the drug's shared selectors with one stage's amounts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stage {
    pub selectors: [i32; 3],
    pub amounts: [i32; 3],
}
impl Stage {
    pub fn immediate(drug: &Drug) -> Self {
        Self {
            selectors: drug.stats,
            amounts: drug.immediate,
        }
    }
    /// Delayed stages 1 and 2 with their delays in minutes from consumption.
    pub fn delayed(drug: &Drug) -> [(i32, Self); 2] {
        [(drug.delay1, drug.changes1), (drug.delay2, drug.changes2)].map(|(delay, amounts)| {
            (
                delay,
                Self {
                    selectors: drug.stats,
                    amounts,
                },
            )
        })
    }
    /// §3.3 step 1: a delayed stage with all three amounts zero is not
    /// scheduled; the range form's low bound counts as an amount.
    pub fn scheduled(&self) -> bool {
        self.amounts.iter().any(|amount| *amount != 0)
    }
    /// §3.2 slot amounts in processing order. With S0 = -2 slot 0 is
    /// skipped and the first processed slot draws `roll(A0, A1)` (a skipped
    /// -1 slot moves the draw on); other slots use their amounts. Slots
    /// with a selector outside 0-37 are skipped and draw nothing.
    pub fn slots(&self, rng: &mut impl RandomSource) -> Result<Vec<(i32, i32)>, WorldError> {
        let range = self.selectors[0] == RANGE_FORM;
        let mut pending = range;
        let mut slots = Vec::with_capacity(3);
        for slot in usize::from(range)..3 {
            let selector = self.selectors[slot];
            if !(0..=RADIATION).contains(&selector) {
                continue;
            }
            let amount = if pending {
                pending = false;
                rng.range(self.amounts[0], self.amounts[1])?
            } else {
                self.amounts[slot]
            };
            slots.push((selector, amount));
        }
        Ok(slots)
    }
}

/// The player's trait and perk modifiers of the drug rules.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sensitivity {
    pub chem_reliant: bool,
    pub chem_resistant: bool,
    pub flower_child: bool,
}
impl Sensitivity {
    /// §3.4 step 3: Chem Reliant doubles, then Chem Resistant halves, then
    /// Flower Child halves the addiction chance (integer division).
    pub fn addiction_chance(self, chance: i32) -> i32 {
        let mut chance = i64::from(chance);
        if self.chem_reliant {
            chance *= 2;
        }
        if self.chem_resistant {
            chance /= 2;
        }
        if self.flower_child {
            chance /= 2;
        }
        chance.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    }
    /// §3.3 step 2: `600 × D` ticks, halved for Chem Resistant.
    pub fn stage_delay(self, minutes: i32) -> i64 {
        let ticks = TICKS_PER_MINUTE * i64::from(minutes);
        if self.chem_resistant {
            ticks / 2
        } else {
            ticks
        }
    }
    /// §3.8 step 3: 10080 minutes, halved for Chem Reliant and again for
    /// Flower Child.
    pub fn withdrawal_ticks(self) -> i64 {
        let mut minutes = WITHDRAWAL_MINUTES;
        if self.chem_reliant {
            minutes /= 2;
        }
        if self.flower_child {
            minutes /= 2;
        }
        TICKS_PER_MINUTE * minutes
    }
}
