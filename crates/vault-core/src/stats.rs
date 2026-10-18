use std::collections::BTreeMap;
use vault_formats::{GcdFile, pro::Critter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Trait {
    FastMetabolism,
    Bruiser,
    SmallFrame,
    OneHander,
    Finesse,
    Kamikaze,
    HeavyHanded,
    FastShot,
    BloodyMess,
    Jinxed,
    GoodNatured,
    ChemReliant,
    ChemResistant,
    NightPerson,
    Skilled,
    Gifted,
}
/// SPEC-011 §3.22 perk indices 0-63: the save-component, script, and
/// `perk.msg` numbering. Indices 53-63 are not selectable; drugs, armor,
/// and weapons apply them as effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Perk {
    Awareness,
    BonusHthAttacks,
    BonusHthDamage,
    BonusMove,
    BonusRangedDamage,
    BonusRateOfFire,
    EarlierSequence,
    FasterHealing,
    MoreCriticals,
    NightVision,
    Presence,
    RadResistance,
    Toughness,
    StrongBack,
    Sharpshooter,
    SilentRunning,
    Survivalist,
    MasterTrader,
    Educated,
    Healer,
    FortuneFinder,
    BetterCriticals,
    Empathy,
    Slayer,
    Sniper,
    SilentDeath,
    ActionBoy,
    MentalBlock,
    Lifegiver,
    Dodger,
    Snakeater,
    MrFixit,
    Medic,
    MasterThief,
    Speaker,
    HeaveHo,
    FriendlyFoe,
    Pickpocket,
    Ghost,
    CultOfPersonality,
    Scrounger,
    Explorer,
    FlowerChild,
    Pathfinder,
    AnimalFriend,
    Scout,
    MysteriousStranger,
    Ranger,
    QuickPockets,
    SmoothTalker,
    SwiftLearner,
    Tag,
    Mutate,
    NukaColaCraving,
    BuffoutCraving,
    MentatsCraving,
    PsychoCraving,
    RadawayCraving,
    WeaponLongRange,
    WeaponAccurate,
    WeaponPenetrate,
    WeaponKnockback,
    PoweredArmor,
    CombatArmor,
}
impl Perk {
    pub const ALL: [Self; 64] = [
        Self::Awareness,
        Self::BonusHthAttacks,
        Self::BonusHthDamage,
        Self::BonusMove,
        Self::BonusRangedDamage,
        Self::BonusRateOfFire,
        Self::EarlierSequence,
        Self::FasterHealing,
        Self::MoreCriticals,
        Self::NightVision,
        Self::Presence,
        Self::RadResistance,
        Self::Toughness,
        Self::StrongBack,
        Self::Sharpshooter,
        Self::SilentRunning,
        Self::Survivalist,
        Self::MasterTrader,
        Self::Educated,
        Self::Healer,
        Self::FortuneFinder,
        Self::BetterCriticals,
        Self::Empathy,
        Self::Slayer,
        Self::Sniper,
        Self::SilentDeath,
        Self::ActionBoy,
        Self::MentalBlock,
        Self::Lifegiver,
        Self::Dodger,
        Self::Snakeater,
        Self::MrFixit,
        Self::Medic,
        Self::MasterThief,
        Self::Speaker,
        Self::HeaveHo,
        Self::FriendlyFoe,
        Self::Pickpocket,
        Self::Ghost,
        Self::CultOfPersonality,
        Self::Scrounger,
        Self::Explorer,
        Self::FlowerChild,
        Self::Pathfinder,
        Self::AnimalFriend,
        Self::Scout,
        Self::MysteriousStranger,
        Self::Ranger,
        Self::QuickPockets,
        Self::SmoothTalker,
        Self::SwiftLearner,
        Self::Tag,
        Self::Mutate,
        Self::NukaColaCraving,
        Self::BuffoutCraving,
        Self::MentatsCraving,
        Self::PsychoCraving,
        Self::RadawayCraving,
        Self::WeaponLongRange,
        Self::WeaponAccurate,
        Self::WeaponPenetrate,
        Self::WeaponKnockback,
        Self::PoweredArmor,
        Self::CombatArmor,
    ];
    /// The perk with a numeric index; anything outside 0-63 is none.
    pub fn from_index(index: i32) -> Option<Self> {
        usize::try_from(index)
            .ok()
            .and_then(|index| Self::ALL.get(index).copied())
    }
    pub const fn index(self) -> usize {
        self as usize
    }
    /// Indices 0-52 can be chosen; 53-63 are effect-only.
    pub const fn selectable(self) -> bool {
        (self as u8) < 53
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatContext {
    pub ticks: u64,
    pub blinded: bool,
    pub combat: bool,
    pub taking_turn: bool,
    pub remaining_ap: i32,
    pub visible_light: i32,
    pub difficulty: i32,
}
impl Default for StatContext {
    fn default() -> Self {
        Self {
            ticks: 0,
            blinded: false,
            combat: false,
            taking_turn: true,
            remaining_ap: 0,
            visible_light: 65536,
            difficulty: 1,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterSheet {
    pub player: bool,
    pub flags: u32,
    pub name: Vec<u8>,
    pub base: [i32; 35],
    pub bonus: [i32; 35],
    pub invested: [i32; 18],
    pub tags: [i32; 4],
    pub traits: [i32; 2],
    pub perks: BTreeMap<Perk, u32>,
    pub current_hp: i32,
    pub poison: i32,
    pub radiation: i32,
    pub level: i32,
    pub experience: i32,
    pub reputation: i32,
    pub karma: i32,
    pub unspent_skills: i32,
    pub reconciled_level: i32,
    /// SPEC-011 §3.21 pending-perk byte (a flag, not a counter).
    pub pending_perk: bool,
    pub remaining_creation: i32,
    pub body: i32,
    /// Critter experience reward (not the player's accumulated experience).
    pub experience_reward: i32,
    pub kill_category: i32,
}
impl Default for CharacterSheet {
    fn default() -> Self {
        let mut base = [0; 35];
        base[..7].fill(5);
        base[29] = 100;
        base[33] = 25;
        let mut sheet = Self {
            player: true,
            flags: 0,
            name: b"Player".to_vec(),
            base,
            bonus: [0; 35],
            invested: [0; 18],
            tags: [-1; 4],
            traits: [-1; 2],
            perks: BTreeMap::new(),
            current_hp: 0,
            poison: 0,
            radiation: 0,
            level: 1,
            experience: 0,
            reputation: 0,
            karma: 0,
            unspent_skills: 0,
            reconciled_level: 1,
            pending_perk: false,
            remaining_creation: 0,
            body: 0,
            experience_reward: 0,
            kill_category: 0,
        };
        sheet.recompute(StatContext::default());
        sheet.heal(10000, StatContext::default());
        sheet
    }
}
pub(crate) fn saturated(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}
impl CharacterSheet {
    pub fn from_gcd(gcd: &GcdFile, context: StatContext) -> Self {
        let gcd = gcd.player_normalized();
        let mut result = Self {
            flags: gcd.flags,
            name: gcd.name_bytes().to_vec(),
            base: gcd.base,
            bonus: gcd.bonus,
            invested: gcd.skills,
            tags: gcd.tags,
            traits: gcd.traits,
            remaining_creation: gcd.remaining_points,
            current_hp: 0,
            ..Self::default()
        };
        result.recompute(context);
        result.heal(10000, context);
        result
    }
    pub fn from_critter(critter: &Critter) -> Self {
        let mut result = Self {
            player: false,
            flags: critter.flags.0,
            name: Vec::new(),
            base: critter.base_stats,
            bonus: critter.bonus_stats,
            invested: critter.skills,
            body: critter.body,
            experience_reward: critter.experience,
            kill_category: critter.kill_category,
            ..Self::default()
        };
        result.current_hp = result.stat(7, StatContext::default());
        result
    }
    pub fn has_trait(&self, value: Trait) -> bool {
        self.player && self.traits.contains(&(value as i32))
    }
    pub fn rank(&self, perk: Perk) -> u32 {
        if self.player {
            *self.perks.get(&perk).unwrap_or(&0)
        } else {
            0
        }
    }
    pub fn raw(&self, index: usize, context: StatContext) -> i64 {
        if index >= 7 {
            return i64::from(self.base.get(index).copied().unwrap_or(0));
        }
        let mut value = i64::from(self.base[index]);
        if self.has_trait(Trait::Gifted) {
            value += 1;
        }
        if index == 0 && self.has_trait(Trait::Bruiser) {
            value += 2;
        }
        if index == 5 && self.has_trait(Trait::SmallFrame) {
            value += 1;
        }
        if matches!(index, 1 | 4) && self.has_trait(Trait::NightPerson) {
            let hours = (context.ticks / 36000) % 24;
            value += if hours >= 18 { 1 } else { -1 };
        }
        value
    }
    pub fn stat(&self, index: usize, context: StatContext) -> i32 {
        if index >= 35 {
            return 0;
        }
        let mut value = self.raw(index, context) + i64::from(self.bonus[index]);
        if index < 7 {
            if index == 1 && context.blinded {
                value -= 5;
            }
            return value.clamp(1, 10) as i32;
        }
        if self.player {
            match index {
                8 if self.has_trait(Trait::Bruiser) => value -= 2,
                9 if self.has_trait(Trait::Kamikaze) => value -= i64::from(self.base[9]),
                11 if self.has_trait(Trait::HeavyHanded) => value += 4,
                12 if self.has_trait(Trait::SmallFrame) => value -= 10 * i64::from(self.base[0]),
                13 if self.has_trait(Trait::Kamikaze) => value += 5,
                14 if self.has_trait(Trait::FastMetabolism) => value += 2,
                15 if self.has_trait(Trait::Finesse) => value += 10,
                16 if self.has_trait(Trait::HeavyHanded) => value -= 30,
                31 | 32 if self.has_trait(Trait::FastMetabolism) => {
                    value += i64::from(self.base[index])
                }
                _ => {}
            }
        }
        if index == 9 && context.combat && !context.taking_turn {
            value += i64::from(context.remaining_ap);
        }
        if index == 33 {
            value += (context.ticks / (365 * 864000)) as i64;
        }
        let (low, high) = match index {
            7..=14 => (0, i32::MAX),
            15..=23 | 29 | 31 | 32 => (0, 100),
            24..=28 | 30 => (0, 90),
            33 => (16, 35),
            34 => (0, 1),
            _ => (0, i32::MAX),
        };
        value.clamp(i64::from(low), i64::from(high)) as i32
    }
    pub fn recompute(&mut self, context: StatContext) {
        let e: Vec<_> = (0..7)
            .map(|index| i64::from(self.stat(index, context)))
            .collect();
        for (index, value) in [
            (7, 15 + self.raw(0, context) + 2 * self.raw(2, context)),
            (8, 5 + e[5] / 2),
            (9, e[5]),
            (11, (e[0] - 5).max(1)),
            (12, 25 + 25 * e[0]),
            (13, 2 * e[1]),
            (14, (e[2] / 3).max(1)),
            (15, e[6]),
            (16, 0),
            (31, 2 * e[2]),
            (32, 5 * e[2]),
        ] {
            self.base[index] = saturated(value);
        }
    }
    pub fn set_base(&mut self, index: usize, value: i32, context: StatContext) -> bool {
        if index >= 35 || (index < 7 && !(1..=10).contains(&value)) {
            return false;
        }
        self.base[index] = value;
        if index < 7 {
            self.recompute(context);
        }
        true
    }
    pub fn adjust_bonus(&mut self, index: usize, delta: i32, context: StatContext) {
        if index >= 35 {
            return;
        }
        self.bonus[index] = self.bonus[index].saturating_add(delta);
        if index < 7 {
            self.recompute(context);
        }
    }
    /// Set a perk rank, applying the §3.12 effect once per gained rank and
    /// removing it once per lost rank.
    pub fn set_perk(&mut self, perk: Perk, rank: u32, context: StatContext) {
        let previous = self.perks.get(&perk).copied().unwrap_or(0);
        if rank == 0 {
            self.perks.remove(&perk);
        } else {
            self.perks.insert(perk, rank);
        }
        let ranks = i64::from(rank) - i64::from(previous);
        for (stat, amount) in crate::progression::perk_changes(perk) {
            self.adjust_bonus(stat, saturated(ranks * i64::from(amount)), context);
        }
    }
    /// SPEC-011 §3.12 perk effect on this sheet's bonus array; removal
    /// subtracts exactly the applied amounts, and effects stack.
    pub fn perk_effect(&mut self, perk: Perk, apply: bool, context: StatContext) {
        for (stat, amount) in crate::progression::perk_changes(perk) {
            self.adjust_bonus(stat, if apply { amount } else { -amount }, context);
        }
    }
    pub fn skill(&self, index: usize, context: StatContext) -> i32 {
        let Some(&points) = self.invested.get(index) else {
            return 0;
        };
        if points < 0 {
            return points;
        }
        let e: Vec<_> = (0..7).map(|i| i64::from(self.stat(i, context))).collect();
        let base = match index {
            0 => 35 + e[5],
            1 | 2 => 10 + e[5],
            3 => 65 + (e[5] + e[0]) / 2,
            4 => 55 + (e[5] + e[0]) / 2,
            5 => 40 + e[5],
            6 => 30 + (e[1] + e[4]) / 2,
            7 => 15 + (e[1] + e[4]) / 2,
            8 => 25 + e[5],
            9 | 11 => 20 + (e[1] + e[5]) / 2,
            10 => 20 + e[5],
            12 => 25 + 2 * e[4],
            13 => 20 + e[4],
            14 => 25 + 2 * e[3],
            15 => 20 + 2 * e[3],
            16 => 20 + 3 * e[6],
            17 => 5 + (e[2] + e[4]) / 2,
            _ => 0,
        };
        let mut value = base
            + if self.player && self.tags.contains(&(index as i32)) {
                20 + 2 * i64::from(points)
            } else {
                i64::from(points)
            };
        if self.player {
            if self.has_trait(Trait::Gifted) {
                value -= 10;
            }
            if self.has_trait(Trait::Skilled) {
                value += 10;
            }
            if self.has_trait(Trait::GoodNatured) {
                if index < 6 {
                    value -= 10;
                } else if matches!(index, 6 | 7 | 14 | 15) {
                    value += 15;
                }
            }
            for (perk, condition, bonus) in [
                (Perk::Medic, matches!(index, 6 | 7), 20),
                (
                    Perk::Ghost,
                    index == 8 && context.visible_light <= 45875,
                    20,
                ),
                (Perk::MasterThief, matches!(index, 8..=11), 10),
                (Perk::MrFixit, matches!(index, 12 | 13), 20),
                (Perk::Speaker, matches!(index, 14 | 15), 20),
            ] {
                if condition && self.rank(perk) > 0 {
                    value += bonus;
                }
            }
            if index >= 6 {
                value += match context.difficulty {
                    0 => 20,
                    2 => -10,
                    _ => 0,
                };
            }
        }
        saturated(value.min(200))
    }
    pub fn buy_skill(&mut self, index: usize) -> bool {
        if index >= 18 || self.unspent_skills <= 0 {
            return false;
        }
        self.invested[index] = self.invested[index].saturating_add(1);
        self.unspent_skills -= 1;
        true
    }
    pub fn heal(&mut self, amount: i32, context: StatContext) {
        self.current_hp = self
            .current_hp
            .saturating_add(amount)
            .clamp(0, self.stat(7, context));
    }
}
