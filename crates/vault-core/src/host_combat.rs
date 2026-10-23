//! SPEC-007 world integration: attack profiles from live state, the §3.2
//! weapon-attack preflight, and §3.6/§3.8 result application.
use crate::{
    ObjectRef, RandomSource, ScriptEvent, Trait, WorldError, WorldState,
    assets::WorldAssets,
    combat::{
        Attack, DeathChoice, PROTECTED_PID, PlayerModifiers, Resolution, Subtype, WeaponProfile,
        corpse_flattens, knockback_displacement, knockout_delay, weapon_skill,
    },
    critical::{effect, hit_mode},
    host::WorldHost,
    host_ai::Victim,
    host_drugs::Arg,
    queue::EventKind,
    routing::{extend_line, rotation, step},
    stats::Perk,
    world::ScriptRequest,
};
use std::collections::BTreeSet;
use vault_formats::{
    Fid,
    map::{CritterState, ObjectDetail, ObjectState},
    pro::{BasicFlags, ExtendedFlags, ItemData, PrototypeData},
};

/// Preflight refusals, in the order they are tested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    DeadDefender,
    CrippledArms,
    CrippledArmTwoHanded,
    ActionPoints,
    Range,
    NoAmmunition,
    Obstructed,
    /// SPEC-007 §3.10.1: the attack's animation sequence cannot register
    /// (the attack then applies nothing).
    Sequence,
}

impl WorldState {
    pub fn critter_state(&self, object: ObjectRef) -> Option<&CritterState> {
        match &self.get(object)?.raw.state {
            ObjectState::Critter(state) => Some(state),
            _ => None,
        }
    }
    pub fn critter_state_mut(&mut self, object: ObjectRef) -> Option<&mut CritterState> {
        match &mut self.get_mut(object)?.raw.state {
            ObjectState::Critter(state) => Some(state),
            _ => None,
        }
    }
    /// The item equipped in the hand a hit mode selects (none for punch/kick).
    pub fn wielded(&self, critter: ObjectRef, mode: u8) -> Option<ObjectRef> {
        let flag = match mode {
            hit_mode::LEFT_PRIMARY | hit_mode::LEFT_SECONDARY | hit_mode::LEFT_RELOAD => {
                BasicFlags::LEFT_HAND
            }
            hit_mode::RIGHT_PRIMARY | hit_mode::RIGHT_SECONDARY | hit_mode::RIGHT_RELOAD => {
                BasicFlags::RIGHT_HAND
            }
            _ => return None,
        };
        self.get(critter)?
            .inventory
            .iter()
            .map(|(item, _)| *item)
            .find(|item| self.get(*item).is_some_and(|o| o.raw.flags.contains(flag)))
    }
    /// Weapon data for a hit mode: primary uses the low extended-flag nibble,
    /// secondary the next nibble, with matching AP cost and range.
    pub fn weapon_profile(&self, item: ObjectRef, mode: u8) -> Option<WeaponProfile> {
        let object = self.get(item)?;
        let prototype = object.prototype.as_deref()?;
        let PrototypeData::Item(vault_formats::pro::Item {
            data: ItemData::Weapon(weapon),
            ..
        }) = &prototype.data
        else {
            return None;
        };
        let secondary = usize::from(matches!(
            mode,
            hit_mode::LEFT_SECONDARY | hit_mode::RIGHT_SECONDARY
        ));
        let rounds = match &object.raw.state {
            ObjectState::Other {
                detail: ObjectDetail::Weapon { rounds, .. },
                ..
            } => *rounds,
            _ => 0,
        };
        Some(WeaponProfile {
            pid: prototype.pid,
            mode: ((prototype.extended.0 >> (4 * secondary)) & 15) as u8,
            damage: weapon.damage,
            damage_type: weapon.damage_type,
            range: weapon.ranges[secondary],
            min_strength: weapon.strength,
            ap_cost: weapon.ap_cost[secondary],
            failure_table: weapon.critical_failure,
            burst: weapon.burst,
            two_handed: prototype.extended.contains(ExtendedFlags::TWO_HANDED),
            big_gun: prototype.extended.contains(ExtendedFlags::BIG_GUN),
            perk: self.rules.weapon_perk(weapon.perk),
            rounds,
            capacity: weapon.capacity,
        })
    }
    fn team(&self, object: ObjectRef) -> i32 {
        self.critter_state(object).map_or(0, |state| state.team)
    }
    /// Behind when the stored orientations differ by 0, 1, or 5.
    fn behind(&self, attacker: ObjectRef, defender: ObjectRef) -> bool {
        match (self.get(attacker), self.get(defender)) {
            (Some(a), Some(d)) => !matches!(a.raw.orientation.abs_diff(d.raw.orientation), 2..=4),
            _ => false,
        }
    }
    /// Build explicit attack inputs from live effective state.
    pub fn attack_profile(
        &self,
        attacker: ObjectRef,
        defender: ObjectRef,
        mode: u8,
        place: u8,
        aimed: bool,
    ) -> Result<Attack, WorldError> {
        let missing = || WorldError::Invalid("attack requires two critters".into());
        let a = self.get(attacker).ok_or_else(missing)?;
        let d = self.get(defender).ok_or_else(missing)?;
        let (sheet, target) = (
            a.character.as_ref().ok_or_else(missing)?,
            d.character.as_ref().ok_or_else(missing)?,
        );
        let (ac, dc) = (self.context(attacker), self.context(defender));
        let player = attacker == self.player;
        let weapon = self
            .wielded(attacker, mode)
            .and_then(|item| self.weapon_profile(item, mode));
        let skill_index = weapon
            .as_ref()
            .map_or(Some(3), |w| weapon_skill(w.mode, w.damage_type, w.big_gun));
        let rank = |perk| sheet.rank(perk);
        let results = |object| self.critter_state(object).map_or(0, |s| s.results.0);
        let player_modifiers = if player {
            PlayerModifiers {
                one_hander: sheet.has_trait(Trait::OneHander),
                fast_shot: sheet.has_trait(Trait::FastShot),
                finesse: sheet.has_trait(Trait::Finesse),
                bloody_mess: sheet.has_trait(Trait::BloodyMess),
                sharpshooter: rank(Perk::Sharpshooter),
                bonus_ranged_damage: rank(Perk::BonusRangedDamage),
                heave_ho: rank(Perk::HeaveHo),
                bonus_hth_attacks: rank(Perk::BonusHthAttacks) > 0,
                bonus_rate_of_fire: rank(Perk::BonusRateOfFire) > 0,
                slayer: rank(Perk::Slayer) > 0,
                sniper: rank(Perk::Sniper) > 0,
                // SPEC-013 §3.7: Silent Death tests the sneak flag.
                silent_death_strike: rank(Perk::SilentDeath) > 0
                    && self
                        .player_sheet()
                        .is_some_and(|sheet| sheet.flags & 1 != 0)
                    && self.behind(attacker, defender)
                    && d.opponent != self.player,
            }
        } else {
            PlayerModifiers::default()
        };
        let ranged = weapon
            .as_ref()
            .and_then(|w| Subtype::of(w.mode))
            .is_some_and(|subtype| matches!(subtype, Subtype::Throwing | Subtype::Ranged));
        Ok(Attack {
            attacker_player: player,
            attacker_pid: a.raw.pid,
            attacker_team: self.team(attacker),
            player_team: self.team(self.player),
            skill: skill_index.map_or(0, |index| sheet.skill(index, ac)),
            perception: sheet.stat(1, ac),
            strength: sheet.stat(0, ac),
            luck: sheet.stat(6, ac),
            critical_chance: sheet.stat(15, ac),
            better_criticals: sheet.stat(16, ac),
            melee_damage: sheet.stat(11, ac),
            blind: results(attacker) & effect::BLIND != 0,
            player: player_modifiers,
            jinxed: self
                .player_sheet()
                .is_some_and(|sheet| sheet.has_trait(Trait::Jinxed)),
            weapon,
            hit_mode: mode,
            location: place,
            aimed,
            distance: self.object_distance(attacker, defender),
            cover: if ranged {
                self.shot(attacker, defender).cover
            } else {
                0
            },
            defender_player: defender == self.player,
            defender_pid: d.raw.pid,
            kill_category: target.kill_category,
            armor_class: target.stat(9, dc),
            thresholds: std::array::from_fn(|index| target.stat(17 + index, dc)),
            resistances: std::array::from_fn(|index| target.stat(24 + index, dc)),
            attacker_thresholds: std::array::from_fn(|index| sheet.stat(17 + index, ac)),
            attacker_resistances: std::array::from_fn(|index| sheet.stat(24 + index, ac)),
            defender_special: std::array::from_fn(|index| target.stat(index, dc)),
            defender_multihex: d.raw.flags.contains(BasicFlags::MULTIHEX),
            defender_prone: results(defender) & (effect::KNOCKED_OUT | effect::KNOCKED_DOWN) != 0,
            defender_light: self.visible_light(defender),
            defender_hp: self.hit_points(defender).unwrap_or(target.current_hp),
            ticks: self.clock.ticks,
            // SPEC-014 §2.3: combat_difficulty defaults to 1 (normal).
            combat_difficulty: self.combat_difficulty.unwrap_or(1),
            accuracy_bonus: 0,
        })
    }
    /// SPEC-007 §3.2 weapon-attack preflight. A high accuracy score alone
    /// does not prove shootability.
    pub fn preflight(
        &self,
        attacker: ObjectRef,
        defender: ObjectRef,
        attack: &Attack,
    ) -> Result<(), Refusal> {
        let dead = self
            .critter_state(defender)
            .is_none_or(|state| state.results.0 & effect::DEAD != 0 || state.hp <= 0);
        if dead {
            return Err(Refusal::DeadDefender);
        }
        let state = self.critter_state(attacker).ok_or(Refusal::DeadDefender)?;
        if let Some(weapon) = &attack.weapon {
            match (state.results.0 & effect::CRIPPLED_ARMS).count_ones() {
                2 => return Err(Refusal::CrippledArms),
                1 if weapon.two_handed => return Err(Refusal::CrippledArmTwoHanded),
                _ => {}
            }
        }
        if state.ap < attack.ap_cost() {
            return Err(Refusal::ActionPoints);
        }
        if attack.distance > attack.range() {
            return Err(Refusal::Range);
        }
        if attack
            .weapon
            .as_ref()
            .is_some_and(|weapon| weapon.capacity > 0 && weapon.rounds == 0)
        {
            return Err(Refusal::NoAmmunition);
        }
        if attack
            .subtype()
            .is_some_and(|subtype| matches!(subtype, Subtype::Throwing | Subtype::Ranged))
            && self.shot(attacker, defender).hard_obstruction
        {
            return Err(Refusal::Obstructed);
        }
        Ok(())
    }
}

impl<A: WorldAssets> WorldHost<A> {
    /// Apply a resolved attack after successful sequencing: debit attack AP
    /// (saturating), consume scheduled ammunition from the hit-mode weapon,
    /// then apply target damage/status, knockback, and death presentation.
    pub fn apply_attack(
        &mut self,
        attacker: ObjectRef,
        defender: ObjectRef,
        attack: &Attack,
        resolution: &Resolution,
    ) -> Result<(), WorldError> {
        self.apply_attack_to(attacker, defender, defender, attack, resolution)
    }
    /// `apply_attack` once the recorded defender may differ from the
    /// intended target (SPEC-007 §3.6): a missed-shot bystander, a random
    /// target, or a non-critter impact. Onlookers are alerted for an
    /// intended target that was not the final victim; only a critter not
    /// already marked dead then takes status, damage and the §3.6 writers.
    pub(crate) fn apply_attack_to(
        &mut self,
        attacker: ObjectRef,
        intended: ObjectRef,
        defender: ObjectRef,
        attack: &Attack,
        resolution: &Resolution,
    ) -> Result<(), WorldError> {
        self.apply_resolved(
            attacker,
            intended,
            defender,
            attack,
            resolution,
            attack.ap_cost(),
            false,
        )
    }
    /// `apply_attack_to`; a `sequenced` attack leaves the knockback and the
    /// death presentation to its show (SPEC-007 §3.10). `cost` is the AP
    /// debit: the initiating request's own cost (SPEC-007 §3.4, aiming
    /// included), which an unintended victim's profile does not replace.
    #[allow(clippy::too_many_arguments)]
    fn apply_resolved(
        &mut self,
        attacker: ObjectRef,
        intended: ObjectRef,
        defender: ObjectRef,
        attack: &Attack,
        resolution: &Resolution,
        cost: i32,
        sequenced: bool,
    ) -> Result<(), WorldError> {
        if let Some(state) = self.world.critter_state_mut(attacker) {
            state.ap = state.ap.saturating_sub(cost).max(0);
        }
        // SPEC-009 §3.4: the player's attack debit references the defender
        // named when the attack began.
        if attacker == self.world.player {
            self.set_last_attacker(attacker, intended);
        }
        // SPEC-007 §3.6 `[Must-match]`: cleanup reselects the current weapon
        // by hit mode before consuming the scheduled ammunition, so a weapon
        // swapped in between is the one debited. The `[May-improve]`
        // alternative (consume from the weapon that made the attack) is not
        // taken: the reference-visible magazine counts are kept.
        if resolution.ammo > 0
            && let Some(item) = self.world.wielded(attacker, attack.hit_mode)
            && let Some(ObjectState::Other {
                detail: ObjectDetail::Weapon { rounds, .. },
                ..
            }) = self.world.get_mut(item).map(|o| &mut o.raw.state)
        {
            *rounds = rounds.saturating_sub(resolution.ammo).max(0);
        }
        // SPEC-007 §3.6 safe rule: attacker self-damage and its flags are
        // committed when the attacker is not marked dead (the reference tests
        // the opposite condition).
        let living = self
            .world
            .critter_state(attacker)
            .is_some_and(|state| state.results.0 & effect::DEAD == 0);
        if resolution.attacker_flags != 0 && attack.attacker_pid != PROTECTED_PID && living {
            let lose_turn = resolution.attacker_flags & effect::LOSE_TURN != 0;
            self.damage_critter(
                attacker,
                None,
                resolution.self_damage,
                resolution.attacker_flags,
            )?;
            if lose_turn && let Some(state) = self.world.critter_state_mut(attacker) {
                state.ap = 0;
            }
        }
        if intended != 0 && intended != defender {
            self.alert_onlookers(intended);
        }
        let living_critter = matches!(
            self.world.get(defender).map(|o| &o.raw.state),
            Some(ObjectState::Critter(state)) if state.results.0 & effect::DEAD == 0
        );
        if attack.defender_pid != PROTECTED_PID && living_critter {
            self.damage_critter(
                defender,
                Some(attacker),
                resolution.damage,
                resolution.defender_flags,
            )?;
            self.after_damage(attacker, defender, defender == intended);
            if sequenced {
                return Ok(());
            }
            // The fall (lethal or not) carries the computed knockback; a
            // nonpositive displacement animates without movement.
            if resolution.knockback > 0 {
                self.knock_back(attacker, defender, resolution.knockback)?;
            }
            let dead = self
                .world
                .critter_state(defender)
                .is_some_and(|state| state.results.0 & effect::DEAD != 0);
            if dead {
                self.present_death(attacker, defender, attack, resolution)?;
            }
        }
        Ok(())
    }
    /// HP/status application: record damage received, persistent status,
    /// knockout wake-up, death (queue removal, kill credit unless the destroy
    /// script overrides), and party removal.
    pub fn damage_critter(
        &mut self,
        victim: ObjectRef,
        source: Option<ObjectRef>,
        damage: i32,
        flags: u32,
    ) -> Result<(), WorldError> {
        let context = self.world.context(victim);
        let Some(object) = self.world.get_mut(victim) else {
            return Ok(());
        };
        let endurance = object.character.as_ref().map_or(5, |s| s.stat(2, context));
        let category = object.character.as_ref().map_or(0, |s| s.kill_category);
        let experience = match object.prototype.as_deref().map(|p| &p.data) {
            Some(PrototypeData::Critter(critter)) => critter.experience,
            _ => 0,
        };
        let ObjectState::Critter(state) = &mut object.raw.state else {
            return Ok(());
        };
        let was_dead = state.results.0 & effect::DEAD != 0;
        let mut flags = flags;
        if damage > 0 {
            if damage >= state.hp {
                flags |= effect::DEAD;
            }
            state.hp = state.hp.saturating_sub(damage);
            state.damage = state.damage.saturating_add(damage);
        }
        state.results.0 |= flags & effect::PERSISTENT;
        let hp = state.hp;
        let dead = !was_dead && state.results.0 & effect::DEAD != 0;
        if let Some(sheet) = &mut object.character {
            sheet.current_hp = hp;
        }
        if dead {
            self.world.queue.remove_object(victim);
        } else if flags & effect::KNOCKED_OUT != 0 {
            self.world.queue.insert(
                self.world.clock.ticks,
                knockout_delay(endurance),
                victim,
                EventKind::Knockout,
            )?;
        }
        let script = self.world.script_for(victim);
        // Script failures here are reported; they do not undo the damage.
        let notify = |host: &mut Self, event: ScriptEvent| {
            let Some(script) = script else {
                return false;
            };
            match host.dispatch(ScriptRequest {
                script,
                event,
                source: source.unwrap_or(0),
                target: victim,
                fixed: 0,
                action: -1,
                only_present: true,
            }) {
                Ok(result) => result.overridden,
                Err(error) => {
                    host.world.diagnostic(format!("{event:?} script: {error}"));
                    false
                }
            }
        };
        if damage > 0 {
            notify(self, ScriptEvent::Damage);
        }
        if dead {
            let overridden = notify(self, ScriptEvent::Destroy);
            let player_team = self.world.critter_state(self.world.player).map(|s| s.team);
            let credited = source.is_some_and(|source| {
                source == self.world.player
                    || self.world.critter_state(source).map(|s| s.team) == player_team
            });
            if credited && !overridden {
                if let Some(count) = usize::try_from(category)
                    .ok()
                    .and_then(|index| self.world.kills.get_mut(index))
                {
                    *count = count.saturating_add(1);
                }
                self.world.combat_experience =
                    self.world.combat_experience.saturating_add(experience);
            }
            self.world.party.remove(&victim);
        }
        Ok(())
    }
    /// Knockback along the attacker-to-defender orientation; a blocker at
    /// distance j limits displacement to `j - 1` (the §3.8 safe rule).
    pub(crate) fn knock_back(
        &mut self,
        attacker: ObjectRef,
        defender: ObjectRef,
        distance: i32,
    ) -> Result<(), WorldError> {
        let (Some(from), Some((to, elevation))) = (
            self.world.get(attacker).and_then(|o| o.raw.hex),
            self.world
                .get(defender)
                .and_then(|o| Some((o.raw.hex?, o.raw.elevation))),
        ) else {
            return Ok(());
        };
        let Some(direction) = rotation(from, to) else {
            return Ok(());
        };
        let path: Vec<i32> = (0..distance.max(0))
            .scan(to, |hex, _| {
                *hex = step(*hex, direction);
                Some(*hex)
            })
            .collect();
        let moved = knockback_displacement(distance, |j| {
            self.world
                .movement_blocker(path[(j - 1) as usize], elevation, defender)
                .is_some()
        });
        if moved > 0 {
            self.world
                .move_object(defender, path[(moved - 1) as usize], elevation, false)?;
        }
        Ok(())
    }
    /// Death presentation: choose the lethal animation, show its static art
    /// (28 higher for 20-35), and make the corpse nonblocking and flat.
    fn present_death(
        &mut self,
        attacker: ObjectRef,
        defender: ObjectRef,
        attack: &Attack,
        resolution: &Resolution,
    ) -> Result<(), WorldError> {
        let (Some(a), Some(d)) = (self.world.get(attacker), self.world.get(defender)) else {
            return Ok(());
        };
        let art = Fid(d.raw.art_id);
        let choice = DeathChoice {
            violence: self.world.violence.unwrap_or(2),
            attacker_player: attack.attacker_player,
            attacker_pid: a.raw.pid,
            attacker_art: a.raw.art_id,
            victim_pid: d.raw.pid,
            orientations: [a.raw.orientation, d.raw.orientation],
            damage_type: attack
                .weapon
                .as_ref()
                .map_or(crate::combat::damage_type::NORMAL, |w| w.damage_type),
            mode: attack.mode(),
            damage: resolution.damage,
            bloody_mess: attack.player.bloody_mess,
            attack_animation: Fid(a.raw.art_id).animation(),
        };
        let orientation = d.raw.orientation;
        let probe = |animation: u8| {
            let fid = self.assets.construct_fid(
                1,
                art.base(),
                animation,
                art.weapon(),
                Fid::selector_for_orientation(orientation),
            );
            self.assets.object_art(fid).is_ok()
        };
        let animation =
            crate::combat::blood_variant(choice.animation(probe), choice.violence, probe);
        let still = if (20..=35).contains(&animation) {
            animation + 28
        } else {
            animation
        };
        let fid = self.assets.construct_fid(
            1,
            art.base(),
            still,
            art.weapon(),
            Fid::selector_for_orientation(orientation),
        );
        if let Some(object) = self.world.get_mut(defender) {
            object.raw.art_id = fid;
            object.raw.frame = 0;
        }
        self.present_corpse(defender, animation)
    }
    /// Corpse presentation: nonblocking and flat (except listed victims),
    /// outline removed; final animations 30/31 drop the inventory onto the
    /// corpse's hex (except listed victims).
    pub fn present_corpse(&mut self, victim: ObjectRef, animation: u8) -> Result<(), WorldError> {
        let Some(object) = self.world.get_mut(victim) else {
            return Ok(());
        };
        object.raw.outline = 0;
        object.raw.flags.0 |= BasicFlags::NONBLOCKING.0;
        if corpse_flattens(object.raw.pid) {
            object.raw.flags.0 |= BasicFlags::FLAT.0;
        }
        let (pid, place) = (
            object.raw.pid,
            object.raw.hex.map(|hex| (hex, object.raw.elevation)),
        );
        if crate::combat::final_animation_drops_inventory(animation, pid)
            && let Some((hex, elevation)) = place
        {
            let items: Vec<_> = self
                .world
                .get(victim)
                .map(|o| o.inventory.clone())
                .unwrap_or_default();
            for (item, quantity) in items {
                self.world.remove_item(victim, item, quantity)?;
                self.world.move_object(item, hex, elevation, false)?;
            }
        }
        Ok(())
    }
}

/// Spray budgets `[center, left, right, direct trials]`. A burst's center is
/// ammo/3 (at least 1), left ammo/3, right the remainder; direct trials are
/// center/2, raised to 1 by taking one from the center. Continuous fire
/// initializes each budget to 1 before the same direct-trial rule.
pub fn spray_budgets(ammo: i32, continuous: bool) -> [i32; 4] {
    let (mut center, left, right) = if continuous {
        (1, 1, 1)
    } else {
        let center = (ammo / 3).max(1);
        (center, ammo / 3, ammo - center - ammo / 3)
    };
    let mut direct = center / 2;
    if direct == 0 {
        direct = 1;
        center -= 1;
    }
    [center, left, right, direct]
}

/// A resolved burst/continuous spray: the main target and up to six extras.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Spray {
    pub main: Resolution,
    pub extras: Vec<(ObjectRef, Attack, Resolution)>,
}

/// An extra spray target: its hits, torso profile and damage so far.
struct ExtraHit {
    object: ObjectRef,
    hits: i32,
    profile: Attack,
    damage: i32,
}

impl<A: WorldAssets> WorldHost<A> {
    /// SPEC-007 §3.7 burst/continuous fire against `defender`, in the spray
    /// draw order: the spray roll, the direct trials, the in-spray Stage A
    /// pass when no direct trial hit, the three rays (each extra's damage
    /// drawn where the ray meets it), Jinxed and Sniper once on the final
    /// result, then the main target's critical and damage draws.
    pub fn resolve_spray(
        &self,
        attacker: ObjectRef,
        defender: ObjectRef,
        attack: &Attack,
        rng: &mut impl crate::RandomSource,
    ) -> Result<Spray, WorldError> {
        let weapon = attack
            .weapon
            .as_ref()
            .ok_or_else(|| WorldError::Invalid("spray requires a weapon".into()))?;
        let continuous = weapon.mode == crate::combat::attack_mode::CONTINUOUS;
        let ammo = weapon.rounds.min(weapon.burst).max(0);
        let chance = attack.hit_chance(true)?;
        // One roll with effective Critical Chance and no location subtraction.
        let mut code = attack.roll(chance, attack.critical_chance, rng)?;
        let mut main = Resolution {
            code,
            multiplier: 2,
            ammo,
            ..Resolution::default()
        };
        if code == 0 {
            attack.critical_failure(&mut main, Subtype::Ranged, rng)?;
            return Ok(Spray {
                main,
                extras: Vec::new(),
            });
        }
        // A critical spray adds 20 without reapplying the 95 cap.
        let accuracy = if code == 3 { chance + 20 } else { chance };
        let [center, left, right, direct] = spray_budgets(ammo, continuous);
        let mut main_hits = 0;
        for _ in 0..direct {
            if attack.roll(accuracy, 0, rng)? >= 2 {
                main_hits += 1;
            }
        }
        let (Some(from), Some(to)) = (
            self.world.get(attacker).and_then(|o| o.raw.hex),
            self.world.get(defender).and_then(|o| o.raw.hex),
        ) else {
            return Err(WorldError::Invalid("spray requires placed critters".into()));
        };
        // §3.7 Stage A inside the spray: its draws are consumed, but it never
        // commits, so the recorded defender and the center budget stand.
        if main_hits == 0 {
            let extension = self.extension_hex(from, to, attack.range());
            self.line_of_fire(attacker, defender, attack, extension, rng)?;
        }
        let range = usize::try_from(attack.range().max(0)).unwrap_or(0);
        let spread = if attack.distance <= 3 {
            extend_line(from, to, 3).last().copied().unwrap_or(to)
        } else {
            to
        };
        let back = rotation(spread, from).unwrap_or(0);
        let sides = [step(spread, (back + 1) % 6), step(spread, (back + 5) % 6)];
        let mut extras: Vec<ExtraHit> = Vec::new();
        for (line, budget) in [
            (extend_line(from, to, range), center - main_hits),
            (extend_line(from, sides[0], range), left),
            (extend_line(from, sides[1], range), right),
        ] {
            let hits = self.spray_ray(
                attacker,
                defender,
                attack,
                &line,
                budget,
                continuous,
                &mut extras,
                rng,
            )?;
            if !continuous {
                main_hits += hits;
            }
        }
        let extra_hits: i32 = extras.iter().map(|extra| extra.hits).sum();
        if code == 1 && main_hits + extra_hits > 0 {
            code = 2;
        } else if code >= 2 && main_hits + extra_hits == 0 {
            code = 1;
        }
        // Jinxed and Sniper once, on the final result; a spray is ranged, so
        // Slayer and Silent Death never apply.
        code = attack.postprocess(code, Subtype::Ranged, rng)?;
        main.code = code;
        // Extra targets took normal-scale damage before any main critical.
        let resolved = extras
            .into_iter()
            .map(|extra| {
                let mut outcome = Resolution {
                    code: 2,
                    multiplier: 2,
                    rounds: extra.hits,
                    damage: extra.damage,
                    defender_flags: effect::HIT,
                    ..Resolution::default()
                };
                extra.profile.finish(&mut outcome);
                (extra.object, extra.profile, outcome)
            })
            .collect();
        if code == 0 {
            // A Jinxed final failure: the scheduled ammunition stays the
            // round count for self and random-target damage.
            attack.critical_failure(&mut main, Subtype::Ranged, rng)?;
        } else if code >= 2 && main_hits > 0 {
            // §3.7 extra hits only: under the safe rule the intended target
            // gets neither the hit bit nor critical processing.
            main.defender_flags |= effect::HIT;
            main.rounds = main_hits;
            if code == 3 {
                attack.critical_success(&mut main, rng)?;
            }
            let bypass = main.defender_flags & effect::BYPASS != 0;
            for _ in 0..main_hits {
                main.damage = main.damage.saturating_add(attack.round_damage(
                    main.multiplier,
                    bypass,
                    false,
                    rng,
                )? as i32);
            }
        }
        attack.finish(&mut main);
        Ok(Spray {
            main,
            extras: resolved,
        })
    }
    /// One spray ray: stops at a hard non-critter obstruction; at each critter
    /// recompute ordinary torso accuracy and take consecutive successful plain
    /// D100 trials until a miss or an exhausted budget (the comparison is
    /// drawn before the budget test). An extra target hit here takes its
    /// damage draws at once. Returns main-target hits.
    #[allow(clippy::too_many_arguments)]
    fn spray_ray(
        &self,
        attacker: ObjectRef,
        defender: ObjectRef,
        attack: &Attack,
        line: &[i32],
        mut budget: i32,
        continuous: bool,
        extras: &mut Vec<ExtraHit>,
        rng: &mut impl crate::RandomSource,
    ) -> Result<i32, WorldError> {
        // SPEC-007 Fixture I: a burst ray cast with no budget left (the
        // center after the direct hits) draws nothing.
        if budget <= 0 && !continuous {
            return Ok(0);
        }
        let elevation = self.world.get(attacker).map_or(0, |o| o.raw.elevation);
        let occupancy = self.world.occupancy(elevation);
        let mut visited = BTreeSet::new();
        let mut main = 0;
        for hex in line {
            let Some(object) = self.world.movement_blocker_in(&occupancy, *hex, attacker) else {
                continue;
            };
            // The occupancy is built from live objects; a vanished one
            // cannot block.
            let Some(found) = self.world.get(object) else {
                continue;
            };
            if Fid(found.raw.art_id).category() != 1 {
                if found.raw.flags.contains(BasicFlags::PROJECTILE_THROUGH) {
                    continue;
                }
                break;
            }
            if !visited.insert(object) {
                continue;
            }
            if continuous {
                budget = 1;
            }
            // SPEC-007 §3.7/§3.7.1: an extra or Stage B victim takes
            // normal-scale rounds through its own effective DT/DR, so its
            // profile is built at the torso (uncalled normalized, no aiming)
            // and serves damage, armor and knockback only; the AP debit stays
            // the request's own cost (§3.4).
            let mut profile = self.world.attack_profile(
                attacker,
                object,
                attack.hit_mode,
                crate::critical::location::TORSO,
                false,
            )?;
            profile.weapon = attack.weapon.clone();
            let accuracy = profile.hit_chance(true)?;
            let mut hits = 0;
            while rng.range(1, 100)? <= accuracy && budget > 0 {
                hits += 1;
                budget -= 1;
            }
            if object == defender {
                main += hits;
            } else if hits > 0 {
                // Continuous fire ignores later duplicate-extra accumulation.
                let slot = match extras.iter().position(|extra| extra.object == object) {
                    Some(slot) if !continuous => {
                        extras[slot].hits += hits;
                        Some(slot)
                    }
                    Some(_) => None,
                    None if extras.len() < 6 => {
                        extras.push(ExtraHit {
                            object,
                            hits,
                            profile,
                            damage: 0,
                        });
                        Some(extras.len() - 1)
                    }
                    None => None,
                };
                if let Some(slot) = slot {
                    let extra = &mut extras[slot];
                    for _ in 0..hits {
                        let round = extra.profile.round_damage(2, false, false, rng)?;
                        extra.damage = extra.damage.saturating_add(round as i32);
                    }
                }
            }
            if budget <= 0 && !continuous {
                break;
            }
        }
        Ok(main)
    }
    /// Apply a spray: the main target's resolution (with AP and the scheduled
    /// ammunition), then every extra target's damage.
    pub fn apply_spray(
        &mut self,
        attacker: ObjectRef,
        defender: ObjectRef,
        attack: &Attack,
        spray: &Spray,
    ) -> Result<(), WorldError> {
        self.apply_spray_to(attacker, defender, defender, attack, spray)
    }
    /// `apply_spray` with the intended target and the recorded defender
    /// apart (see `apply_attack_to`). Extras not already marked dead follow
    /// the same writers; an extra is never the intended target.
    pub(crate) fn apply_spray_to(
        &mut self,
        attacker: ObjectRef,
        intended: ObjectRef,
        defender: ObjectRef,
        attack: &Attack,
        spray: &Spray,
    ) -> Result<(), WorldError> {
        self.apply_sprayed(
            attacker,
            intended,
            defender,
            attack,
            spray,
            attack.ap_cost(),
            false,
        )
    }
    /// The §3.6 cleanup of a sequenced attack: its show commits the
    /// knockback, the death and the corpse presentation.
    /// `cost` is the initiating request's AP cost (see `apply_resolved`).
    pub(crate) fn apply_spray_sequenced(
        &mut self,
        attacker: ObjectRef,
        intended: ObjectRef,
        defender: ObjectRef,
        attack: &Attack,
        spray: &Spray,
        cost: i32,
    ) -> Result<(), WorldError> {
        self.apply_sprayed(attacker, intended, defender, attack, spray, cost, true)
    }
    #[allow(clippy::too_many_arguments)]
    fn apply_sprayed(
        &mut self,
        attacker: ObjectRef,
        intended: ObjectRef,
        defender: ObjectRef,
        attack: &Attack,
        spray: &Spray,
        cost: i32,
        sequenced: bool,
    ) -> Result<(), WorldError> {
        self.apply_resolved(
            attacker,
            intended,
            defender,
            attack,
            &spray.main,
            cost,
            sequenced,
        )?;
        for (object, profile, outcome) in &spray.extras {
            if profile.defender_pid != PROTECTED_PID && !self.world.is_dead(*object) {
                self.damage_critter(
                    *object,
                    Some(attacker),
                    outcome.damage,
                    outcome.defender_flags,
                )?;
                self.after_damage(attacker, *object, false);
                if outcome.knockback > 0 && !sequenced {
                    self.knock_back(attacker, *object, outcome.knockback)?;
                }
            }
        }
        Ok(())
    }
}

impl<A: WorldAssets> WorldHost<A> {
    /// One complete attack action: profile, the initiating request's accuracy,
    /// preflight, single-shot or spray resolution (world generator) with the
    /// random-target replacement, request damage bounds/override flags, the
    /// SPEC-009 taunt requests, application, then the taunt displays. A
    /// refusal performs nothing and is returned in the inner result.
    pub fn perform_attack(
        &mut self,
        attacker: ObjectRef,
        defender: ObjectRef,
        mode: u8,
        place: u8,
        aimed: bool,
    ) -> Result<Result<Resolution, Refusal>, WorldError> {
        let mode = self.kick_substitution(attacker, mode)?;
        let mut attack = self
            .world
            .attack_profile(attacker, defender, mode, place, aimed)?;
        let request = self.apply_request(attacker, defender, &mut attack);
        if let Err(refusal) = self.world.preflight(attacker, defender, &attack) {
            return Ok(Err(refusal));
        }
        // SPEC-007 §3.4: the debit is this request's cost (aiming included),
        // whichever victim profile the resolution ends on.
        let cost = attack.ap_cost();
        // SPEC-007 §2.5 safe rule (see `Attack::critical_success`).
        if !attack.defender_player && !(0..=14).contains(&attack.kill_category) {
            self.world.diagnostic(format!(
                "kill category {} is outside 0-14; victim table 0 is used",
                attack.kill_category
            ));
        }
        // SPEC-011 §3.11: the player is warned when the weapon's minimum
        // strength exceeds effective ST (the hit chance pays 20 per point).
        if attack.attacker_player
            && attack
                .weapon
                .as_ref()
                .is_some_and(|weapon| weapon.min_strength > attack.strength)
        {
            self.print_game("combat.msg", 107, &[]);
        }
        let mut rng = std::mem::take(&mut self.world.rng);
        let outcome = self.resolve_attack(attacker, defender, &attack, &mut rng);
        self.world.rng = rng;
        let (mut spray, victim, impact) = outcome?;
        if let Some(request) = request {
            adjust_for_request(&request, &mut spray.main);
        }
        let recorded = spray.main.clone();
        // A random target or a missed-shot bystander replaces the defender
        // as an unintended victim (SPEC-007 §3.6); a non-critter impact
        // becomes the recorded defender and is presented as the miss it is.
        let (target, applied) = match &victim {
            Some(Victim::Random(victim, profile) | Victim::Bystander(victim, profile)) => {
                (*victim, profile)
            }
            Some(Victim::Object(_)) | None => (defender, &attack),
        };
        let recorded_defender = match &victim {
            Some(Victim::Object(object)) => *object,
            _ => target,
        };
        // A struck wall or scenery object is the recorded defender with the
        // hit bit (§3.9): the projectile and N end on its hex, but it is
        // applied and reported as the miss it is.
        let struck = matches!(victim, Some(Victim::Object(_)));
        let hit = spray.main.defender_flags & effect::HIT != 0;
        if struck {
            spray.main.defender_flags &= !effect::HIT;
        }
        self.bystander_message(attacker, defender, victim.as_ref());
        self.world.ai.callbacks.clear();
        // SPEC-013 §3.10: an exploding weapon's explosion is where the
        // projectile ends.
        let explosion = self
            .weapon_explosion(attacker, defender, &attack, &spray.main, &victim, impact)
            .map(|(_, art)| art);
        // SPEC-007 §3.10: the attack's sequence is built, making the taunt
        // and fire-dance draws, before anything applies. A sequence that
        // cannot register fails the attack before its AP debit, and nothing
        // plays.
        let scene = crate::host_choreography::Scene {
            selector: None,
            attacker,
            defender: recorded_defender,
            attack: &attack,
            victim: applied,
            spray: &spray,
            hit,
            impact,
            explosion,
        };
        let built = match self.build_show(&scene) {
            Ok(built) => built,
            Err(refused) => {
                self.world
                    .diagnostic(format!("attack sequence refused: {refused:?}"));
                return Ok(Err(Refusal::Sequence));
            }
        };
        // The sequence's commitments, then the §3.6 cleanup: onlookers hear
        // of the intended target when the final victim differs, then of a
        // critter victim.
        if let Some(thrown) = built.thrown {
            self.consume_throw(attacker, thrown.item, thrown.landing)?;
        }
        self.apply_effects(&built.effects)?;
        let player = self.world.player;
        let before = self.world.hit_points(player);
        let application = self.apply_spray_sequenced(
            attacker,
            defender,
            recorded_defender,
            applied,
            &spray,
            cost,
        );
        // SPEC-014 §3.21: combat damage to the player rolls the HP counter.
        if self.world.hit_points(player) != before {
            self.world.emit(crate::WorldEffect::RollHitPoints);
        }
        application?;
        self.present(built.show);
        Ok(Ok(recorded))
    }
    /// SPEC-013 §3.10 a weapon explosion: the wielded weapon explodes when
    /// thrown with explosion, plasma or EMP damage or used without throwing
    /// with explosion damage. The projectile ends at the victim's hex on a
    /// hit or at the §3.7.1 impact hex on a miss, where the explosion art
    /// plays (presentation only: the attack has applied the damage). A
    /// critical failure launches no projectile, so nothing explodes.
    fn weapon_explosion(
        &self,
        attacker: ObjectRef,
        defender: ObjectRef,
        attack: &crate::combat::Attack,
        result: &crate::combat::Resolution,
        victim: &Option<Victim>,
        impact: Option<i32>,
    ) -> Option<(ObjectRef, crate::world::ExplosionArt)> {
        let data = attack.weapon.as_ref()?;
        let weapon = self.world.wielded(attacker, attack.hit_mode)?;
        let pid = self.world.get(weapon)?.raw.pid;
        let thrown = attack.mode() == crate::combat::attack_mode::THROW;
        let (fid, delay) = crate::skills::weapon_explosion(pid, data.damage_type, thrown)?;
        if result.code == 0 {
            return None;
        }
        let place = |object: ObjectRef| self.world.get(object).and_then(|o| o.raw.hex);
        let center = match victim {
            Some(
                Victim::Random(object, _) | Victim::Bystander(object, _) | Victim::Object(object),
            ) => place(*object),
            None if result.defender_flags & effect::HIT != 0 => place(defender),
            None => impact,
        }?;
        let elevation = self.world.get(attacker)?.raw.elevation;
        Some((weapon, self.explosion_art(center, elevation, fid, delay)))
    }
    /// SPEC-007 §3.6 punch-to-kick substitution: a punch request first draws
    /// D(1, 4); on 1, kick art for the attacker's current base index and
    /// weapon code (animation 17, selector 0) switches the request to kick.
    /// The probe draws nothing, and a drawn value stays consumed.
    pub(crate) fn kick_substitution(
        &mut self,
        attacker: ObjectRef,
        mode: u8,
    ) -> Result<u8, WorldError> {
        if mode != hit_mode::PUNCH || self.world.rng.range(1, 4)? != 1 {
            return Ok(mode);
        }
        let kick = self.world.get(attacker).is_some_and(|object| {
            let art = Fid(object.raw.art_id);
            self.assets
                .art_exists(Fid::new(1, art.base(), 17, art.weapon(), 0).0)
        });
        Ok(if kick { hit_mode::KICK } else { mode })
    }
    /// §3.7.1 messages: a critter victim other than the intended target is
    /// "hit instead" (`combat.msg` base 500 player, 600 male, 700 female, plus
    /// 8 when the intended target is the player, else 9); a non-critter
    /// impact shows the attacker's "missed" text (515, 615 or 715).
    fn bystander_message(
        &mut self,
        attacker: ObjectRef,
        intended: ObjectRef,
        victim: Option<&Victim>,
    ) {
        let base = |host: &Self, object: ObjectRef| {
            if object == host.world.player {
                500
            } else if host.world.effective(object, 34) == 1 {
                700
            } else {
                600
            }
        };
        let name = |host: &Self, object: ObjectRef| {
            if object == host.world.player {
                host.game_text("combat.msg", 506).unwrap_or_default()
            } else {
                host.world
                    .get(object)
                    .map(|o| o.name.clone())
                    .unwrap_or_default()
            }
        };
        match victim {
            Some(Victim::Bystander(victim, _)) if *victim != intended => {
                let (victim_name, intended_name) = (name(self, *victim), name(self, intended));
                if intended == self.world.player {
                    let id = base(self, *victim) + 8;
                    self.print_game("combat.msg", id, &[Arg::Text(&victim_name)]);
                } else {
                    let id = base(self, *victim) + 9;
                    self.print_game(
                        "combat.msg",
                        id,
                        &[Arg::Text(&victim_name), Arg::Text(&intended_name)],
                    );
                }
            }
            Some(Victim::Object(_)) => {
                let attacker_name = name(self, attacker);
                let id = base(self, attacker) + 15;
                self.print_game("combat.msg", id, &[Arg::Text(&attacker_name)]);
            }
            _ => {}
        }
    }
}

/// The scripted request's adjustments before sequencing: clamp defender
/// damage to the request bounds, then write the override words to the
/// attacker and defender flags (the SPEC-007 §3.6 safe rule; the reference
/// writes both to the defender, the last winning). Already computed
/// knockback/death flags are kept.
pub fn adjust_for_request(
    request: &crate::combat_state::CombatRequest,
    resolution: &mut Resolution,
) {
    let [minimum, maximum] = request.damage;
    if minimum <= maximum {
        resolution.damage = resolution.damage.clamp(minimum, maximum);
    }
    if let Some([attacker, defender]) = request.flags {
        resolution.attacker_flags = attacker as u32;
        resolution.defender_flags = defender as u32;
    }
}
