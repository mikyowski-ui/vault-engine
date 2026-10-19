//! SPEC-007 §3.4-3.8 attack arithmetic: AP costs, ranges, hit chance, rolls,
//! critical effects, damage, and knockback. Inputs are explicit effective
//! values so the rules can be checked independently of world bookkeeping.
use crate::{
    RandomSource, WorldError,
    critical::{self, PLAYER_MATRIX, effect, location},
    world::WeaponPerk,
};

/// Weapon attack-mode nibble values (SPEC-003 §2.6).
pub mod attack_mode {
    pub const NONE: u8 = 0;
    pub const PUNCH: u8 = 1;
    pub const KICK: u8 = 2;
    pub const SWING: u8 = 3;
    pub const THRUST: u8 = 4;
    pub const THROW: u8 = 5;
    pub const SINGLE: u8 = 6;
    pub const BURST: u8 = 7;
    pub const CONTINUOUS: u8 = 8;
}

/// Damage types, in DT/DR order.
pub mod damage_type {
    pub const NORMAL: i32 = 0;
    pub const LASER: i32 = 1;
    pub const FIRE: i32 = 2;
    pub const PLASMA: i32 = 3;
    pub const ELECTRICAL: i32 = 4;
    pub const EMP: i32 = 5;
    pub const EXPLOSION: i32 = 6;
}

/// The literal protected PID: no victim critical augmentation or ordinary
/// death/status/HP application, and exempt from critical-failure effects.
pub const PROTECTED_PID: u32 = 0x01000008;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subtype {
    Unarmed,
    Melee,
    Throwing,
    Ranged,
}
impl Subtype {
    /// Modes 1/2 unarmed, 3/4 melee, 5 throwing, 6-8 ranged; 0 and 9-15
    /// have no valid skill/mode-table entry.
    pub fn of(mode: u8) -> Option<Self> {
        match mode {
            1 | 2 => Some(Self::Unarmed),
            3 | 4 => Some(Self::Melee),
            5 => Some(Self::Throwing),
            6..=8 => Some(Self::Ranged),
            _ => None,
        }
    }
    pub fn close(self) -> bool {
        matches!(self, Self::Unarmed | Self::Melee)
    }
}

/// Weapon skill for an attack mode: Unarmed 3, Melee 4, Throwing 5; ranged
/// uses Energy Weapons 2 for laser/plasma/electrical, else Big Guns 1 with the
/// big-gun flag, else Small Guns 0.
pub fn weapon_skill(mode: u8, damage_type: i32, big_gun: bool) -> Option<usize> {
    Some(match Subtype::of(mode)? {
        Subtype::Unarmed => 3,
        Subtype::Melee => 4,
        Subtype::Throwing => 5,
        Subtype::Ranged if matches!(damage_type, 1 | 3 | 4) => 2,
        Subtype::Ranged if big_gun => 1,
        Subtype::Ranged => 0,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeaponProfile {
    pub pid: u32,
    /// Attack-mode nibble of the selected hit mode.
    pub mode: u8,
    pub damage: [i32; 2],
    pub damage_type: i32,
    pub range: i32,
    pub min_strength: i32,
    pub ap_cost: i32,
    pub failure_table: i32,
    pub burst: i32,
    pub two_handed: bool,
    pub big_gun: bool,
    pub perk: Option<WeaponPerk>,
    pub rounds: i32,
    pub capacity: i32,
}

/// Player-only attacker modifiers; all default for NPC attackers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerModifiers {
    pub one_hander: bool,
    pub fast_shot: bool,
    pub finesse: bool,
    pub bloody_mess: bool,
    pub sharpshooter: u32,
    pub bonus_ranged_damage: u32,
    pub heave_ho: u32,
    pub bonus_hth_attacks: bool,
    pub bonus_rate_of_fire: bool,
    pub slayer: bool,
    pub sniper: bool,
    /// Silent Death applies: a sneaking player strike from behind against a
    /// defender not already targeting the player.
    pub silent_death_strike: bool,
}

/// Effective, explicit inputs for one attack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attack {
    pub attacker_player: bool,
    pub attacker_pid: u32,
    pub attacker_team: i32,
    pub player_team: i32,
    /// Effective selected weapon skill, or Unarmed without a weapon.
    pub skill: i32,
    pub perception: i32,
    pub strength: i32,
    pub luck: i32,
    pub critical_chance: i32,
    pub better_criticals: i32,
    pub melee_damage: i32,
    pub blind: bool,
    pub player: PlayerModifiers,
    /// The player's Jinxed trait, which affects every attacker.
    pub jinxed: bool,
    pub weapon: Option<WeaponProfile>,
    pub hit_mode: u8,
    pub location: u8,
    /// The player's UI aiming flag (ignored for NPC attackers).
    pub aimed: bool,
    /// Object distance (multihex-adjusted).
    pub distance: i32,
    /// Intervening critters from the shot query.
    pub cover: u32,
    pub defender_player: bool,
    pub defender_pid: u32,
    pub kill_category: i32,
    /// Effective AC, including unused AP when the defender is not acting.
    pub armor_class: i32,
    pub thresholds: [i32; 7],
    pub resistances: [i32; 7],
    /// The attacker's own DT/DR, for critical-failure self damage.
    pub attacker_thresholds: [i32; 7],
    pub attacker_resistances: [i32; 7],
    /// Effective defender SPECIAL, for critical saving throws.
    pub defender_special: [i32; 7],
    pub defender_multihex: bool,
    /// Knocked out or knocked down.
    pub defender_prone: bool,
    /// Visible light at the defender (SPEC-006), for player attackers.
    pub defender_light: i32,
    pub defender_hp: i32,
    pub ticks: u64,
    /// Combat difficulty preference: 0 easy, 1 normal, 2 hard.
    pub combat_difficulty: i32,
    /// Initial scripted combat-request accuracy adjustment.
    pub accuracy_bonus: i32,
}

/// Player darkness penalty by the defender's visible light (full 65536).
pub fn light_penalty(light: i32) -> i32 {
    match light {
        ..=26214 => -40,
        26215..=39321 => -25,
        39322..=52428 => -10,
        _ => 0,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resolution {
    /// 0 critical failure, 1 failure, 2 success, 3 critical success.
    pub code: i32,
    pub defender_flags: u32,
    pub attacker_flags: u32,
    /// Damage scale numerator (normal 2, Silent Death 4, or critical entry).
    pub multiplier: i32,
    pub message: Option<i32>,
    pub damage: i32,
    /// Rounds that hit the main target.
    pub rounds: i32,
    /// Ammunition scheduled for consumption at cleanup.
    pub ammo: i32,
    /// Computed critical-failure self damage (committed to a living attacker
    /// under the SPEC-007 §3.6 safe rule).
    pub self_damage: i32,
    pub knockback: i32,
    /// A random-target critical failure asks the caller for another target.
    pub retarget: bool,
}

impl Attack {
    /// The selected attack mode: the weapon's nibble, or bare punch/kick.
    pub fn mode(&self) -> u8 {
        match &self.weapon {
            Some(weapon) => weapon.mode,
            None if self.hit_mode == crate::critical::hit_mode::KICK => attack_mode::KICK,
            None => attack_mode::PUNCH,
        }
    }
    pub fn subtype(&self) -> Option<Subtype> {
        Subtype::of(self.mode())
    }
    /// SPEC-007 §3.4 effective range.
    pub fn range(&self) -> i32 {
        match (&self.weapon, self.subtype()) {
            (None, _) => 1,
            (Some(weapon), Some(Subtype::Throwing)) => {
                let heave = if self.attacker_player {
                    2 * self.player.heave_ho as i32
                } else {
                    0
                };
                weapon.range.min(3 * (self.strength + heave))
            }
            (Some(weapon), _) => weapon.range,
        }
    }
    /// Non-torso/non-uncalled locations imply aiming; torso/uncalled use the
    /// player's UI flag, or false for NPCs.
    pub fn aiming(&self) -> bool {
        !matches!(self.location, location::TORSO | location::UNCALLED)
            || (self.attacker_player && self.aimed)
    }
    /// SPEC-007 §3.4 attack cost: base cost, presence-tested player
    /// reductions, then +1 aiming, clamped to at least 1.
    pub fn ap_cost(&self) -> i32 {
        let subtype = self.subtype();
        let mut cost = match &self.weapon {
            None => 3,
            Some(weapon) => {
                let mut cost = weapon.ap_cost;
                if self.attacker_player && self.range() > 2 && self.player.fast_shot {
                    cost -= 1;
                }
                cost
            }
        };
        if self.attacker_player {
            if subtype.is_some_and(Subtype::close) && self.player.bonus_hth_attacks {
                cost -= 1;
            }
            if subtype == Some(Subtype::Ranged)
                && self.weapon.is_some()
                && self.player.bonus_rate_of_fire
            {
                cost -= 1;
            }
        }
        if self.aiming() {
            cost += 1;
        }
        cost.max(1)
    }
    fn perk(&self) -> Option<WeaponPerk> {
        self.weapon.as_ref().and_then(|weapon| weapon.perk)
    }
    /// SPEC-007 §3.5 hit chance. `range_aware` includes the R term; both
    /// variants count cover for ranged/throw attacks. Capped above at 95 only.
    pub fn hit_chance(&self, range_aware: bool) -> Result<i32, WorldError> {
        let subtype = self
            .subtype()
            .ok_or_else(|| WorldError::Invalid("attack mode has no valid skill".into()))?;
        let mut chance = i64::from(self.skill);
        let (perception, distance) = (i64::from(self.perception), i64::from(self.distance));
        if matches!(subtype, Subtype::Throwing | Subtype::Ranged) {
            if range_aware {
                let factor = if self.perk() == Some(WeaponPerk::LongRange) {
                    4
                } else {
                    2
                };
                let mut r = (distance - factor * perception).max(-2 * perception);
                if self.attacker_player {
                    r -= 2 * i64::from(self.player.sharpshooter);
                }
                chance += if r >= 0 && self.blind {
                    -12 * r
                } else {
                    -4 * r
                };
            }
            chance -= 10 * i64::from(self.cover);
        }
        if let Some(weapon) = &self.weapon {
            if self.attacker_player && self.player.one_hander {
                chance += if weapon.two_handed { -40 } else { 20 };
            }
            chance -= 20 * i64::from((weapon.min_strength - self.strength).max(0));
            if weapon.perk == Some(WeaponPerk::Accurate) {
                chance += 20;
            }
        }
        chance -= i64::from(self.armor_class);
        chance += i64::from(
            critical::location_adjustment(self.location, subtype.close())
                .ok_or_else(|| WorldError::Invalid("hit location must be 0-8".into()))?,
        );
        if self.defender_multihex {
            chance += 15;
        }
        if self.attacker_player {
            chance += i64::from(light_penalty(self.defender_light));
        }
        chance += i64::from(self.accuracy_bonus);
        if self.blind {
            chance -= 25;
        }
        if self.defender_prone {
            chance += 40;
        }
        if self.attacker_team != self.player_team {
            chance += match self.combat_difficulty {
                0 => -20,
                2 => 20,
                _ => 0,
            };
        }
        Ok(chance.min(95).max(i64::from(i32::MIN)) as i32)
    }
    /// D100 roll against `chance`, with the first-day-gated critical upgrade.
    pub(crate) fn roll(
        &self,
        chance: i32,
        critical_modifier: i32,
        rng: &mut impl RandomSource,
    ) -> Result<i32, WorldError> {
        let margin = chance - rng.range(1, 100)?;
        let mut code = if margin >= 0 { 2 } else { 1 };
        if self.ticks >= 864000 {
            let upgrade = rng.range(1, 100)?;
            if code == 1 {
                if upgrade <= -margin / 10 {
                    code = 0;
                }
            } else if upgrade <= margin / 10 + critical_modifier {
                code = 3;
            }
        }
        Ok(code)
    }
    /// The difficulty damage percentage for this attacker's team.
    fn difficulty(&self) -> i64 {
        if self.attacker_team != self.player_team {
            match self.combat_difficulty {
                0 => 75,
                2 => 125,
                _ => 100,
            }
        } else {
            100
        }
    }
    /// DT/DR by weapon damage type. Penetrate zeroes DT; a player attacker's
    /// Finesse adds 30 DR; armor bypass zeroes both except for EMP.
    fn armor(&self, bypass: bool, victim_is_attacker: bool) -> (i64, i64) {
        let kind = self
            .weapon
            .as_ref()
            .map_or(damage_type::NORMAL, |w| w.damage_type);
        if bypass && kind != damage_type::EMP {
            return (0, 0);
        }
        let (thresholds, resistances) = if victim_is_attacker {
            (&self.attacker_thresholds, &self.attacker_resistances)
        } else {
            (&self.thresholds, &self.resistances)
        };
        let index = usize::try_from(kind).unwrap_or(usize::MAX);
        let threshold = if self.perk() == Some(WeaponPerk::Penetrate) {
            0
        } else {
            thresholds.get(index).copied().unwrap_or(0)
        };
        let finesse = if self.attacker_player && self.player.finesse {
            30
        } else {
            0
        };
        let resistance = resistances.get(index).copied().unwrap_or(0) + finesse;
        (threshold.into(), resistance.into())
    }
    /// One hitting round: base damage (melee bonus widens the upper bound),
    /// ranged bonus, scale, difficulty, then DT and DR applied to this round.
    pub(crate) fn round_damage(
        &self,
        multiplier: i32,
        bypass: bool,
        victim_is_attacker: bool,
        rng: &mut impl RandomSource,
    ) -> Result<i64, WorldError> {
        let subtype = self.subtype();
        let mut base = i64::from(match &self.weapon {
            None => rng.range(1, self.melee_damage + 2)?,
            Some(weapon) if subtype.is_some_and(Subtype::close) => {
                rng.range(weapon.damage[0], weapon.damage[1] + self.melee_damage)?
            }
            Some(weapon) => rng.range(weapon.damage[0], weapon.damage[1])?,
        });
        if self.attacker_player && subtype == Some(Subtype::Ranged) {
            base += 2 * i64::from(self.player.bonus_ranged_damage);
        }
        let (threshold, resistance) = self.armor(bypass, victim_is_attacker);
        let mut value = base * i64::from(multiplier) / 2 * self.difficulty() / 100 - threshold;
        if value > 0 {
            value -= value * resistance / 100;
        }
        Ok(value.max(0))
    }
    /// Computed knockback distance for `total` damage.
    pub fn knockback(&self, total: i32) -> i32 {
        if self.defender_multihex {
            return 0;
        }
        let kind = self
            .weapon
            .as_ref()
            .map_or(damage_type::NORMAL, |w| w.damage_type);
        if kind == damage_type::EXPLOSION
            || self.weapon.is_none()
            || self.subtype() == Some(Subtype::Melee)
        {
            if self.perk() == Some(WeaponPerk::Knockback) {
                total / 5
            } else {
                total / 10
            }
        } else {
            0
        }
    }
    /// Normal aimed-shot availability: not for a player with Fast Shot, not
    /// for burst/continuous animation, not for explosion/fire/EMP damage,
    /// and (SPEC-007 §3.4 safe rule) not for a plasma throw weapon.
    pub fn aimed_shot_available(&self) -> bool {
        let kind = self
            .weapon
            .as_ref()
            .map_or(damage_type::NORMAL, |w| w.damage_type);
        !(self.attacker_player && self.player.fast_shot)
            && !matches!(self.mode(), attack_mode::BURST | attack_mode::CONTINUOUS)
            && !matches!(
                kind,
                damage_type::EXPLOSION | damage_type::FIRE | damage_type::EMP
            )
            && !(self.mode() == attack_mode::THROW && kind == damage_type::PLASMA)
    }
    /// Single-shot or ordinary melee/unarmed attack (burst/continuous fire
    /// uses [`Attack::resolve_spray`]).
    pub fn resolve(&self, rng: &mut impl RandomSource) -> Result<Resolution, WorldError> {
        let subtype = self
            .subtype()
            .ok_or_else(|| WorldError::Invalid("attack mode has no valid skill".into()))?;
        let chance = self.hit_chance(true)?;
        let full_location = critical::location_adjustment(self.location, false)
            .ok_or_else(|| WorldError::Invalid("hit location must be 0-8".into()))?;
        let mut code = self.roll(chance, self.critical_chance - full_location, rng)?;
        code = self.postprocess(code, subtype, rng)?;
        let mut resolution = Resolution {
            code,
            multiplier: 2,
            ammo: if subtype == Subtype::Ranged { 1 } else { 0 },
            ..Resolution::default()
        };
        if code >= 2 {
            if self.player.silent_death_strike && subtype.close() {
                resolution.multiplier = 4;
            }
            resolution.defender_flags |= effect::HIT;
            resolution.rounds = 1;
            if code == 3 {
                self.critical_success(&mut resolution, rng)?;
            }
            let bypass = resolution.defender_flags & effect::BYPASS != 0;
            resolution.damage =
                self.round_damage(resolution.multiplier, bypass, false, rng)? as i32;
        } else if code == 0 {
            self.critical_failure(&mut resolution, subtype, rng)?;
        }
        self.finish(&mut resolution);
        Ok(resolution)
    }
    /// Forced upgrades after the roll: global Jinxed, then player Slayer and
    /// Sniper. These are not disabled by the first-day critical gate.
    fn postprocess(
        &self,
        mut code: i32,
        subtype: Subtype,
        rng: &mut impl RandomSource,
    ) -> Result<i32, WorldError> {
        if self.jinxed && code == 1 && rng.range(0, 1)? == 1 {
            code = 0;
        }
        // Slayer (melee/unarmed) is tested first; Sniper's D10 is drawn only
        // for a ranged success that Slayer did not promote.
        if self.attacker_player
            && code == 2
            && ((self.player.slayer && subtype.close())
                || (self.player.sniper
                    && subtype == Subtype::Ranged
                    && rng.range(1, 10)? <= self.luck))
        {
            code = 3;
        }
        Ok(code)
    }
    pub(crate) fn critical_success(
        &self,
        resolution: &mut Resolution,
        rng: &mut impl RandomSource,
    ) -> Result<(), WorldError> {
        resolution.defender_flags |= effect::CRITICAL;
        if self.defender_pid == PROTECTED_PID {
            return Ok(());
        }
        let matrix = if self.defender_player {
            PLAYER_MATRIX
        } else {
            usize::try_from(self.kill_category)
                .ok()
                .filter(|category| *category <= 14)
                .ok_or_else(|| WorldError::Invalid("kill category must be 0-14".into()))?
        };
        let tier = critical::success_tier(rng.range(1, 100)? + self.better_criticals);
        // Uncalled is normalized to torso before the normal effect lookup.
        let place = if self.location == location::UNCALLED {
            location::TORSO
        } else {
            self.location
        };
        let entry = critical::success_effect(matrix, place, tier)
            .ok_or_else(|| WorldError::Invalid("critical entry out of range".into()))?;
        resolution.defender_flags |= entry.flags;
        resolution.message = Some(entry.message);
        resolution.multiplier = entry.multiplier;
        if entry.save_stat != -1 {
            let stat = usize::try_from(entry.save_stat)
                .ok()
                .and_then(|index| self.defender_special.get(index))
                .copied()
                .ok_or_else(|| WorldError::Invalid("critical saving stat out of range".into()))?;
            if rng.range(1, 10)? > stat + entry.save_adjustment {
                resolution.defender_flags |= entry.failed_flags;
                resolution.message = Some(entry.failed_message);
            }
        }
        resolution.defender_flags = random_cripple(resolution.defender_flags, rng)?;
        Ok(())
    }
    pub(crate) fn critical_failure(
        &self,
        resolution: &mut Resolution,
        subtype: Subtype,
        rng: &mut impl RandomSource,
    ) -> Result<(), WorldError> {
        resolution.defender_flags &= !effect::HIT;
        if self.attacker_pid == PROTECTED_PID {
            return Ok(());
        }
        let table = self
            .weapon
            .as_ref()
            .map_or(0, |weapon| weapon.failure_table);
        let column = critical::failure_column(rng.range(1, 100)? - 5 * (self.luck - 5));
        let mask = critical::failure_mask(table, column)
            .ok_or_else(|| WorldError::Invalid("critical-failure column must be 0-4".into()))?;
        if mask == 0 {
            return Ok(());
        }
        let mut flags = effect::CRITICAL | mask;
        let mut self_damage = 0;
        if flags & effect::HIT_SELF != 0 {
            let rounds = if subtype == Subtype::Ranged {
                resolution.ammo.max(1)
            } else {
                1
            };
            for _ in 0..rounds {
                self_damage += self.round_damage(2, false, true, rng)?;
            }
        }
        if flags & effect::EXPLODE != 0 {
            self_damage += self.round_damage(2, false, true, rng)?;
        }
        if flags & effect::HURT_SELF != 0 {
            self_damage += i64::from(rng.range(1, 5)?);
        }
        if flags & effect::LOSE_AMMO != 0 {
            if subtype == Subtype::Ranged {
                resolution.ammo = self.weapon.as_ref().map_or(0, |weapon| weapon.rounds);
            } else {
                flags &= !effect::LOSE_AMMO;
            }
        }
        flags = random_cripple(flags, rng)?;
        if flags & effect::RANDOM_HIT != 0 {
            resolution.retarget = true;
        }
        resolution.attacker_flags = flags;
        resolution.self_damage = self_damage.min(i64::from(i32::MAX)) as i32;
        Ok(())
    }
    /// Lethality and knockback after damage is known.
    pub(crate) fn finish(&self, resolution: &mut Resolution) {
        if resolution.damage > 0
            && resolution.damage >= self.defender_hp
            && self.defender_pid != PROTECTED_PID
        {
            resolution.defender_flags |= effect::DEAD;
        }
        resolution.knockback = self.knockback(resolution.damage);
    }
}

/// Replace a random-cripple flag with one uniform choice of the four limbs,
/// without rerolling an already crippled limb.
fn random_cripple(flags: u32, rng: &mut impl RandomSource) -> Result<u32, WorldError> {
    if flags & effect::CRIPPLED_RANDOM == 0 {
        return Ok(flags);
    }
    let limb = effect::RANDOM_LIMBS[rng.range(0, 3)? as usize];
    Ok((flags & !effect::CRIPPLED_RANDOM) | limb)
}

/// Reload through the normal combat/interface path costs 2 AP.
pub const RELOAD_COST: i32 = 2;
/// Nonweapon generic use, and using an inventory item on a target in combat
/// (charged on success, in addition to the inventory entry cost).
pub const ITEM_USE_COST: i32 = 2;
/// Standing up; below 3 AP the remaining AP drop to zero.
pub const STAND_UP_COST: i32 = 3;
/// Opening the player inventory in combat, charged once on entry; under the
/// SPEC-007 §3.4 safe rule the cost is clamped at 0.
pub fn inventory_entry_cost(quick_pockets: u32) -> i32 {
    (4 - quick_pockets as i32).max(0)
}
/// Knockout wake-up delay in ticks for an effective Endurance.
pub fn knockout_delay(endurance: i32) -> i64 {
    10 * (35 - 3 * i64::from(endurance))
}

/// Knockback displacement with blockers checked at increasing distance.
/// The §3.8 safe rule stops one hex short of the first blocker, so a
/// blocker at distance j permits `j - 1` (the reference's double decrement
/// permits `j - 2`); with no blocker the computed distance is used.
pub fn knockback_displacement(computed: i32, mut blocked: impl FnMut(i32) -> bool) -> i32 {
    (1..=computed)
        .find(|distance| blocked(*distance))
        .map_or(computed.max(0), |distance| distance - 1)
}

/// Inputs to the attack-driven death-animation choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeathChoice {
    /// Violence preference: 0 none, 1 minimal, 2 normal, 3 maximum.
    pub violence: i32,
    pub attacker_player: bool,
    pub attacker_pid: u32,
    pub attacker_art: u32,
    pub victim_pid: u32,
    /// Stored attacker and defender orientations.
    pub orientations: [u8; 2],
    pub damage_type: i32,
    pub mode: u8,
    pub damage: i32,
    pub bloody_mess: bool,
    /// The supplied attack-animation selector (for the special attacker art).
    pub attack_animation: u8,
}

/// (moderate, maximum) severe-death animations by damage type.
pub const SEVERE_DEATH: [(u8, u8); 7] = [
    (26, 25),
    (28, 28),
    (24, 33),
    (24, 32),
    (27, 30),
    (20, 20),
    (23, 31),
];

impl DeathChoice {
    /// From front when the stored orientations differ by 2, 3, or 4.
    pub fn from_front(&self) -> bool {
        matches!(self.orientations[0].abs_diff(self.orientations[1]), 2..=4)
    }
    /// Minimum-violence/art validation. A front hit falls back to 20; under
    /// the SPEC-007 §3.8 safe rule a rear hit falls back to 21 when
    /// fall-front art exists and 20 otherwise (the reference inverts this).
    fn validate(&self, desired: u8, minimum: i32, art: &impl Fn(u8) -> bool) -> u8 {
        if self.violence >= minimum && art(desired) {
            desired
        } else if self.from_front() || !art(21) {
            20
        } else {
            21
        }
    }
    /// SPEC-007 §3.8 ordinary lethal animation. `art` reports whether the
    /// victim has art for a death animation code.
    pub fn animation(&self, art: impl Fn(u8) -> bool) -> u8 {
        if self.attacker_art == 0x0500000a {
            return self.validate(31, 3, &art);
        }
        if self.attacker_pid == 0x020001eb {
            return self.validate(30, 3, &art);
        }
        if self.attacker_art == 0x020001f5 {
            return self.validate(self.attack_animation, 3, &art);
        }
        if matches!(self.victim_pid, 0x01000017 | 0x01000032 | 0x01000031) {
            return self.validate(31, 2, &art);
        }
        let bloody = self.attacker_player && self.bloody_mess;
        let (moderate, maximum) = usize::try_from(self.damage_type)
            .ok()
            .and_then(|kind| SEVERE_DEATH.get(kind))
            .copied()
            .unwrap_or((20, 20));
        let mut choice = 20;
        if self.violence == 3
            && ((bloody && matches!(self.mode, 1..=5))
                || (self.mode == attack_mode::SINGLE
                    && self.damage_type == damage_type::NORMAL
                    && (bloody || self.damage >= 45)))
        {
            return self.validate(23, 3, &art);
        } else if self.violence > 2 && (bloody || self.damage >= 45) {
            choice = if self.violence >= 3 && art(maximum) {
                maximum
            } else {
                moderate
            };
            return self.validate(choice, if choice == maximum { 3 } else { 2 }, &art);
        } else if self.violence > 1 && (bloody || self.damage >= 15) {
            return self.validate(moderate, 2, &art);
        }
        if !self.from_front() {
            choice = 21;
        }
        self.validate(choice, 0, &art)
    }
}

/// Blood presentation: fall-back/front become 34/35 when violence is nonzero
/// and that art exists.
pub fn blood_variant(animation: u8, violence: i32, art: impl Fn(u8) -> bool) -> u8 {
    match animation {
        20 if violence != 0 && art(34) => 34,
        21 if violence != 0 && art(35) => 35,
        other => other,
    }
}

/// Corpse presentation: nonblocking and flat, except these victims.
pub fn corpse_flattens(victim_pid: u32) -> bool {
    !matches!(victim_pid, 0x01000032 | 0x01000031 | 0x01000008)
}
/// Final animations 30/31 drop inventory, except these victims.
pub fn final_animation_drops_inventory(animation: u8, victim_pid: u32) -> bool {
    matches!(animation, 30 | 31) && !matches!(victim_pid, 0x01000031 | 0x01000032 | 0x01000017)
}

/// Grenade scatter (SPEC-007 §3.7.1 Stage B step 1): the nominal distance
/// D(half target distance) first, using the generator's reversed-bound
/// fallback and displacing at least 1 hex, then a direction 0-5
/// (border-stopped stepping).
pub fn grenade_scatter(
    target: i32,
    distance: i32,
    rng: &mut impl RandomSource,
) -> Result<i32, WorldError> {
    let displacement = rng.range(1, distance / 2)?.max(1);
    let direction = rng.range(0, 5)? as u8;
    vault_formats::HexGrid::step(target, direction, displacement)
        .map_err(|error| WorldError::Invalid(error.to_string()))
}
