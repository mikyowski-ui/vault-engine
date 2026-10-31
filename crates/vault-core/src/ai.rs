//! SPEC-009 combat-AI rules that do not depend on the live world: the
//! perception limit, retaliation rating, danger-source selection over a
//! snapshot view, weapon preference and inventory search, hit-mode and
//! called-shot selection (with their exact random draws), flee candidates,
//! taunt ranges, and the healing and bounded attack loops. The live world
//! supplies the environments (`host_ai`).
use crate::{ObjectRef, RandomSource, WorldError, rng::stat_check};
use std::sync::Arc;
use vault_formats::{
    MsgCatalog,
    ai::{Packet, Packets},
};

/// Runtime AI state. The packet table, snapshot, and taunt buffers are never
/// serialized (the combat-AI save component has no bytes).
#[derive(Debug, Clone)]
pub struct AiState {
    pub packets: Option<Arc<Packets>>,
    /// `combatai.msg`.
    pub messages: Option<Arc<MsgCatalog>>,
    /// The combat-wide critter list captured at combat start.
    pub snapshot: Vec<ObjectRef>,
    /// Taunt text buffers: run/move/attack, then miss/hit.
    pub taunts: [Vec<u8>; 2],
    /// Configured preferences; unset ones use SPEC-009's defaults.
    pub combat_taunts: Option<i32>,
    pub language_filter: Option<i32>,
    /// The remembered previous filter state (initially -1).
    pub filter_state: i32,
    /// Whether the loaded taunt catalog went through the language filter.
    pub filtered: bool,
    /// The accuracy bonus added to the AI's hit-chance evaluations (the
    /// scripted request's, during its first turn only).
    pub accuracy: i32,
    /// A scripted request's defender and accuracy, awaiting the first turn.
    pub requested: Option<(ObjectRef, i32)>,
    /// Taunt display callbacks registered by the current animation sequence.
    pub callbacks: Vec<TauntCallback>,
}
impl Default for AiState {
    fn default() -> Self {
        Self {
            packets: None,
            messages: None,
            snapshot: Vec::new(),
            taunts: Default::default(),
            combat_taunts: None,
            language_filter: None,
            filter_state: -1,
            filtered: false,
            accuracy: 0,
            requested: None,
            callbacks: Vec::new(),
        }
    }
}

/// A registered taunt display: the speaker, its text buffer (read at
/// callback time), and the packet's font, text-color word, and outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TauntCallback {
    pub speaker: ObjectRef,
    pub buffer: usize,
    pub font: i32,
    pub color: i32,
    pub outline: i32,
}

/// Intent bits (the SPEC-005 maneuver word).
pub mod intent {
    pub const ALERT: u32 = 0x01;
    pub const STOP: u32 = 0x02;
    pub const FLEE: u32 = 0x04;
}
/// Attack subtypes: 0 none, 1 unarmed, 2 melee, 3 throw, 4 ranged.
pub mod subtype {
    pub const NONE: u8 = 0;
    pub const UNARMED: u8 = 1;
    pub const MELEE: u8 = 2;
    pub const THROW: u8 = 3;
    pub const RANGED: u8 = 4;
}
pub const STIMPAK: u32 = 40;
pub const SUPER_STIMPAK: u32 = 144;
/// Objects with smaller identifiers are never party members.
pub const PARTY_ID_FLOOR: u32 = 18000;
/// Hit locations and modes used by the AI.
pub const TORSO: u8 = 3;
pub const UNCALLED: u8 = 8;
pub const RIGHT_PRIMARY: u8 = 2;
pub const RIGHT_SECONDARY: u8 = 3;
pub const PUNCH: u8 = 4;
/// The bounded attack loop's attempt limit.
pub const ATTACK_ATTEMPTS: usize = 10;
/// Critter animations the AI plays: hands (drug use, reloading), take out
/// and put away (wield and unwield).
pub const HANDS: u8 = 11;
pub const TAKE_OUT: u8 = 38;
pub const PUT_AWAY: u8 = 39;
/// Weapon sound effect types: ready and out of ammunition.
pub const READY_SOUND: u8 = 0;
pub const OUT_OF_AMMO_SOUND: u8 = 2;
/// Taunt text buffers hold 80 bytes: copies keep at most 79.
pub const TAUNT_BYTES: usize = 79;
/// Message-monitor lines are formatted into 200 bytes: at most 199 kept.
pub const MESSAGE_BYTES: usize = 199;

/// Attack subtype for a prototype attack-mode nibble. Nibbles 9-15 index
/// past the reference table: subtype none, reported by the flag.
pub fn subtype_of(nibble: u8) -> (u8, bool) {
    match nibble {
        0 => (subtype::NONE, false),
        1 | 2 => (subtype::UNARMED, false),
        3 | 4 => (subtype::MELEE, false),
        5 => (subtype::THROW, false),
        6..=8 => (subtype::RANGED, false),
        _ => (subtype::NONE, true),
    }
}

/// The SPEC-007 attack animation for an attack-mode nibble: punch 16, kick
/// 17, swing 42, thrust 41, throw 18, single 45, burst 46, continuous 47.
pub fn attack_animation(nibble: u8) -> u8 {
    match nibble {
        1 => 16,
        2 => 17,
        3 => 42,
        4 => 41,
        5 => 18,
        6 => 45,
        7 => 46,
        8 => 47,
        _ => 0,
    }
}
/// Melee-form attacks (animation below 45 and not throw) taunt the hit or
/// miss from the attacker; ranged and thrown attacks from the defender.
pub fn melee_form(animation: u8) -> bool {
    animation < 45 && animation != 18
}

/// §3.3 perception limit: `5P` in the forward sector (halved for a glass
/// target), else `2P` in combat or `P` outside; quartered for a sneaking
/// player target. Integer divisions truncate.
pub fn perception_limit(
    perception: i32,
    forward: bool,
    glass: bool,
    sneaking_player: bool,
    combat: bool,
) -> i32 {
    let mut limit = if forward {
        let limit = perception.saturating_mul(5);
        if glass { limit / 2 } else { limit }
    } else if combat {
        perception.saturating_mul(2)
    } else {
        perception
    };
    if sneaking_player {
        limit /= 4;
    }
    limit
}

/// Inputs to the §3.4 retaliation rating of one critter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rating {
    /// False for a non-critter, dead, or knocked-out critter.
    pub rateable: bool,
    pub melee_damage: i32,
    /// Maximum damage of the right-hand, then left-hand weapon.
    pub weapons: [Option<i32>; 2],
    pub armor_class: i32,
}
/// Melee Damage raised to any greater weapon maximum, plus effective AC.
pub fn retaliation_rating(rating: Option<Rating>) -> i32 {
    let Some(rating) = rating.filter(|r| r.rateable) else {
        return 0;
    };
    let mut best = rating.melee_damage;
    for maximum in rating.weapons.into_iter().flatten() {
        if maximum > best {
            best = maximum;
        }
    }
    best.saturating_add(rating.armor_class)
}

/// The world queries danger-source selection needs.
pub trait ThreatView {
    fn last_attacker(&self, critter: ObjectRef) -> Option<ObjectRef>;
    fn dead(&self, critter: ObjectRef) -> bool;
    fn team(&self, critter: ObjectRef) -> i32;
    fn dist(&self, a: ObjectRef, b: ObjectRef) -> i32;
    fn perceives(&self, observer: ObjectRef, target: ObjectRef) -> bool;
}

/// Ascending `dist` from `from`, nulls last. Stable, so equal distances keep
/// their existing order (the reference's unstable sort has none defined).
pub fn sort_by_distance(list: &mut [Option<ObjectRef>], from: ObjectRef, view: &impl ThreatView) {
    list.sort_by_key(|entry| entry.map_or((1, 0), |e| (0, view.dist(from, e))));
}
fn sort_snapshot(snapshot: &mut [ObjectRef], from: ObjectRef, view: &impl ThreatView) {
    snapshot.sort_by_key(|entry| view.dist(from, *entry));
}

/// The first living entry (not `critter`) on `of`'s team, after sorting the
/// snapshot from `critter` in place.
pub fn nearest_team_member(
    snapshot: &mut [ObjectRef],
    critter: ObjectRef,
    of: ObjectRef,
    view: &impl ThreatView,
) -> Option<ObjectRef> {
    if snapshot.is_empty() {
        return None;
    }
    sort_snapshot(snapshot, critter, view);
    let team = view.team(of);
    snapshot
        .iter()
        .copied()
        .find(|e| *e != critter && !view.dead(*e) && view.team(*e) == team)
}

/// Attacker search: `[my victim, teammate's attacker, team's victim]`, each
/// slot filled once by the first qualifying entry of the sorted snapshot;
/// the scan stops after three fills.
pub fn attacker_search(
    snapshot: &mut [ObjectRef],
    critter: ObjectRef,
    view: &impl ThreatView,
) -> [Option<ObjectRef>; 3] {
    sort_snapshot(snapshot, critter, view);
    let team = view.team(critter);
    let mut slots = [None; 3];
    let mut fills = 0;
    for entry in snapshot.iter().copied().filter(|e| *e != critter) {
        if fills == 3 {
            break;
        }
        let attacker = view.last_attacker(entry);
        if slots[0].is_none() && !view.dead(entry) && attacker == Some(critter) {
            slots[0] = Some(entry);
            fills += 1;
        }
        if slots[1].is_none()
            && view.team(entry) == team
            && let Some(other) = attacker
            && other != critter
            && view.team(other) != team
            && !view.dead(other)
        {
            slots[1] = Some(other);
            fills += 1;
        }
        if slots[2].is_none()
            && view.team(entry) != team
            && !view.dead(entry)
            && attacker.is_some_and(|other| view.team(other) == team)
        {
            slots[2] = Some(entry);
            fills += 1;
        }
    }
    slots
}

/// §3.5 `danger(C)`: a living last attacker returns at once; otherwise the
/// nearest perceived of `[t0, t1, t2, t3]`.
pub fn danger(
    snapshot: &mut [ObjectRef],
    critter: ObjectRef,
    view: &impl ThreatView,
) -> Option<ObjectRef> {
    let t0 = match view.last_attacker(critter).filter(|h| *h != critter) {
        Some(attacker) if !view.dead(attacker) => return Some(attacker),
        Some(attacker) if view.team(attacker) != view.team(critter) => {
            nearest_team_member(snapshot, critter, attacker, view)
        }
        _ => None,
    };
    let [t1, t2, t3] = attacker_search(snapshot, critter, view);
    let mut candidates = [t0, t1, t2, t3];
    sort_by_distance(&mut candidates, critter, view);
    candidates
        .into_iter()
        .flatten()
        .find(|target| view.perceives(critter, *target))
}

/// §3.9 usability of a weapon in a hit mode: not with both arms crippled,
/// not two-handed with either arm crippled, only with the critter art for
/// the attack, and only at a weapon skill of at least `min_to_hit` (a skill
/// percentage, not a hit chance).
pub fn weapon_usable(
    damage_flags: u32,
    two_handed: bool,
    min_to_hit: i32,
    has_art: impl FnOnce() -> bool,
    skill: impl FnOnce() -> i32,
) -> bool {
    let arms = damage_flags & crate::critical::effect::CRIPPLED_ARMS;
    if arms == crate::critical::effect::CRIPPLED_ARMS || (arms != 0 && two_handed) {
        return false;
    }
    has_art() && skill() >= min_to_hit
}

/// What weapon preference compares: primary subtype and item cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeaponChoice {
    pub primary_subtype: u8,
    pub cost: i32,
}
/// §3.9: differing primary subtypes prefer ranged, then throw; otherwise
/// the challenger wins only with a strictly higher cost.
pub fn prefers_challenger(incumbent: WeaponChoice, challenger: WeaponChoice) -> bool {
    let (held, offered) = (incumbent.primary_subtype, challenger.primary_subtype);
    if held != offered {
        if held == subtype::RANGED || offered == subtype::RANGED {
            return offered == subtype::RANGED;
        }
        if held == subtype::THROW || offered == subtype::THROW {
            return offered == subtype::THROW;
        }
    }
    challenger.cost > incumbent.cost
}
/// Base cost plus `floor(loaded x ammo cost / pack quantity)` with
/// truncating, wrapping 32-bit arithmetic. A zero pack quantity contributes
/// nothing and is reported by the flag.
pub fn weapon_cost(base: i32, loaded: i32, ammo_cost: i32, pack: i32) -> (i32, bool) {
    if loaded <= 0 {
        return (base, false);
    }
    if pack == 0 {
        return (base, true);
    }
    (
        base.wrapping_add(loaded.wrapping_mul(ammo_cost).wrapping_div(pack)),
        false,
    )
}

/// One top-level weapon entry for the inventory search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InventoryWeapon {
    pub right_hand: bool,
    pub raw_primary_ap: i32,
    pub usable_primary: bool,
    pub loaded: i32,
    pub ammo_available: bool,
    pub choice: WeaponChoice,
}
/// §3.9 inventory search (body type already checked): skip the right-hand
/// entry, too-costly entries under the AP check, entries unusable in mode
/// 2, and empty ranged weapons without compatible ammunition; then fold the
/// preference rule in inventory order.
pub fn inventory_best(entries: &[InventoryWeapon], ap_check: Option<i32>) -> Option<usize> {
    inventory_best_by(entries, ap_check, prefers_challenger)
}
/// [`inventory_best`] with another preference rule in place of
/// [`prefers_challenger`] (SPEC-019 §3.16 rule 8's ranking).
pub fn inventory_best_by(
    entries: &[InventoryWeapon],
    ap_check: Option<i32>,
    prefers: impl Fn(WeaponChoice, WeaponChoice) -> bool,
) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (index, weapon) in entries.iter().enumerate() {
        if weapon.right_hand
            || ap_check.is_some_and(|ap| weapon.raw_primary_ap > ap)
            || !weapon.usable_primary
            || (weapon.choice.primary_subtype == subtype::RANGED
                && weapon.loaded == 0
                && !weapon.ammo_available)
        {
            continue;
        }
        best = match best {
            Some(held) if !prefers(entries[held].choice, weapon.choice) => Some(held),
            _ => Some(index),
        };
    }
    best
}

/// The weapon properties hit-mode selection consults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeWeapon {
    pub secondary_subtype: u8,
    pub usable_secondary: bool,
}
/// §3.10: punch without a weapon; right primary unless the secondary exists,
/// is usable, and `random(1, secondary_freq)` gives 1. A throwing secondary
/// with no other inventory weapon is kept as primary when an Intelligence
/// check (one D10) succeeds.
pub fn select_hit_mode(
    weapon: Option<ModeWeapon>,
    secondary_freq: i32,
    intelligence: i32,
    rng: &mut impl RandomSource,
    other_weapon: impl FnOnce() -> bool,
) -> Result<u8, WorldError> {
    let Some(weapon) = weapon else {
        return Ok(PUNCH);
    };
    if weapon.secondary_subtype == subtype::NONE || !weapon.usable_secondary {
        return Ok(RIGHT_PRIMARY);
    }
    if rng.range(1, secondary_freq)? != 1 {
        return Ok(RIGHT_PRIMARY);
    }
    if weapon.secondary_subtype == subtype::THROW
        && !other_weapon()
        && stat_check(intelligence, 0, rng)?.code >= 2
    {
        return Ok(RIGHT_PRIMARY);
    }
    Ok(RIGHT_SECONDARY)
}

/// Minimum Intelligence for called shots by combat difficulty (7 easy, 5
/// normal, 3 hard); other preference values use 5.
pub fn called_shot_intelligence(difficulty: i32) -> i32 {
    match difficulty {
        0 => 7,
        2 => 3,
        _ => 5,
    }
}
/// What called-shot selection consults besides draws and hit chances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalledShot {
    pub aimed_affordable: bool,
    pub aiming_available: bool,
    pub called_freq: i32,
    pub difficulty: i32,
    pub intelligence: i32,
    pub min_to_hit: i32,
}
/// §3.11: torso unless aimed AP is affordable, aiming is available,
/// `random(1, called_freq)` gives 1, Intelligence meets the difficulty
/// minimum, and the drawn location `random(0, 8)` keeps the range-aware
/// hit chance at `min_to_hit` or above. Location 8 (uncalled) can result.
pub fn select_location(
    inputs: CalledShot,
    rng: &mut impl RandomSource,
    hit_chance: impl FnOnce(u8) -> Result<i32, WorldError>,
) -> Result<u8, WorldError> {
    let Some(location) = draw_location(inputs, rng)? else {
        return Ok(TORSO);
    };
    if hit_chance(location)? < inputs.min_to_hit {
        return Ok(TORSO);
    }
    Ok(location)
}
/// [`select_location`]'s draws: the location to test, or none for the torso.
/// The test that follows draws nothing of its own, so a hook there may use
/// the world's generator again.
pub(crate) fn draw_location(
    inputs: CalledShot,
    rng: &mut impl RandomSource,
) -> Result<Option<u8>, WorldError> {
    if !inputs.aimed_affordable || !inputs.aiming_available {
        return Ok(None);
    }
    if rng.range(1, inputs.called_freq)? != 1
        || inputs.intelligence < called_shot_intelligence(inputs.difficulty)
    {
        return Ok(None);
    }
    Ok(Some(rng.range(0, 8)? as u8))
}

/// §3.8 flee candidates: for N from AP down to 1, directions D, D+1, D+5.
/// Lazily enumerated: a crafted AP word is not allocated for.
pub fn flee_candidates(direction: u8, ap: i32) -> impl Iterator<Item = (i32, u8)> {
    (1..=ap.max(0)).rev().flat_map(move |steps| {
        [direction % 6, (direction + 1) % 6, (direction + 5) % 6].map(|d| (steps, d))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taunt {
    Run,
    Move,
    Attack,
    Miss,
    /// Hit at a location 0-7 (8 is treated as torso).
    Hit(u8),
}
impl Taunt {
    /// Run/move/attack share one text buffer; miss/hit share the other.
    pub fn buffer(self) -> usize {
        match self {
            Self::Run | Self::Move | Self::Attack => 0,
            Self::Miss | Self::Hit(_) => 1,
        }
    }
}
/// §3.17 inclusive message-ID range for a taunt type. A Fallout 2 packet's
/// range ends at its own end key (SPEC-018 §4.7.1); an end below the start
/// is an empty range.
pub fn taunt_range(packet: &Packet, taunt: Taunt) -> (i32, i32) {
    if let Some(ends) = &packet.ends {
        return match taunt {
            Taunt::Run => (packet.run_start, ends[0]),
            Taunt::Move => (packet.move_start, ends[1]),
            Taunt::Attack => (packet.attack_start, ends[2]),
            Taunt::Miss => (packet.miss_start, ends[3]),
            Taunt::Hit(location) => {
                let location = if location > 7 {
                    3
                } else {
                    usize::from(location)
                };
                (packet.hit_start[location], ends[4 + location])
            }
        };
    }
    match taunt {
        Taunt::Run => (packet.run_start, packet.move_start),
        Taunt::Move => (packet.move_start, packet.attack_start),
        Taunt::Attack => (packet.attack_start, packet.miss_start),
        Taunt::Miss => (packet.miss_start, packet.hit_start[0]),
        Taunt::Hit(location) => {
            let location = if location > 7 {
                3
            } else {
                usize::from(location)
            };
            let end = if location == 7 {
                packet.last_msg
            } else {
                packet.hit_start[location + 1]
            };
            (packet.hit_start[location], end)
        }
    }
}

/// The inventory operations the healing loop performs.
pub trait HealingInventory {
    type Unit;
    fn hp(&self) -> i32;
    fn ap(&self) -> i32;
    fn set_ap(&mut self, ap: i32);
    /// SPEC-007 §3.4's generic use: 2 AP.
    fn use_cost(&mut self) -> i32 {
        2
    }
    /// `None` past the end; `Some(None)` for a non-drug entry; otherwise the
    /// drug's PID.
    fn entry(&self, position: usize) -> Option<Option<u32>>;
    /// Remove one unit from the stack at `position`.
    fn take_one(&mut self, position: usize) -> Option<Self::Unit>;
    /// SPEC-011 §3.7 drug consumption; false reports failure (-1).
    fn consume(&mut self, unit: &Self::Unit) -> Result<bool, WorldError>;
    /// Force the unit back into inventory after a failed consumption.
    fn restore(&mut self, unit: Self::Unit) -> Result<(), WorldError>;
    /// Hands animation, the 5000 message, and destruction of the unit.
    fn used(&mut self, unit: Self::Unit) -> Result<(), WorldError>;
}
/// §3.7 healing for a biped below half its maximum HP: stimpaks only, two AP
/// per consumption attempt (saturating), restarting from the first entry.
pub fn heal(inventory: &mut impl HealingInventory, half_maximum: i32) -> Result<(), WorldError> {
    heal_limited(inventory, half_maximum, None)
}
/// [`heal`] below `threshold`, stopping after `limit` successful uses
/// (SPEC-019 §3.16 rule 2); `None` is no limit.
pub fn heal_limited(
    inventory: &mut impl HealingInventory,
    threshold: i32,
    limit: Option<u32>,
) -> Result<(), WorldError> {
    let mut position = 0;
    let mut uses = 0;
    while inventory.hp() < threshold
        && inventory.ap() >= 2
        && limit.is_none_or(|limit| uses < limit)
    {
        let pid = loop {
            match inventory.entry(position) {
                None => return Ok(()),
                Some(Some(pid)) => break pid,
                Some(None) => position += 1,
            }
        };
        if pid != STIMPAK && pid != SUPER_STIMPAK {
            position += 1;
            continue;
        }
        let Some(unit) = inventory.take_one(position) else {
            position += 1;
            continue;
        };
        if inventory.consume(&unit)? {
            inventory.used(unit)?;
            uses += 1;
        } else {
            inventory.restore(unit)?;
        }
        let cost = inventory.use_cost();
        inventory.set_ap((inventory.ap() - cost).max(0));
        position = 0;
    }
    Ok(())
}

/// Weapon-switching outputs: success, whether a weapon is now held, and the
/// selected hit mode (outputs replace the caller's even on failure).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Switched {
    pub success: bool,
    pub weapon: bool,
    pub mode: u8,
}
/// The live operations of one attack loop against one target.
pub trait AttackEnv {
    /// Knocked out, dead, or losing the next turn.
    fn disabled(&self) -> bool;
    /// SPEC-007 preflight code 0-7 for the mode (non-aimed).
    fn preflight(&mut self, mode: u8) -> Result<u8, WorldError>;
    fn hit_chance(&mut self, mode: u8, range_aware: bool) -> Result<i32, WorldError>;
    fn attack(&mut self, mode: u8) -> Result<bool, WorldError>;
    fn attack_cost(&mut self, mode: u8) -> Result<i32, WorldError>;
    fn ap(&self) -> i32;
    fn approach(&mut self, taunt: bool) -> Result<bool, WorldError>;
    fn flee(&mut self) -> Result<(), WorldError>;
    fn has_weapon(&self) -> bool;
    fn switch_weapon(&mut self) -> Result<Switched, WorldError>;
    /// Reloading; `Some` when its out-of-ammo path ran weapon switching.
    fn reload(&mut self) -> Result<Option<Switched>, WorldError>;
    /// After an approach: whether the loop now returns failure instead of
    /// going on (SPEC-019 §3.16 rule 6, `stay` and the stay-close leash).
    /// SPEC-009 never halts.
    fn halted(&self) -> bool {
        false
    }
}
/// §3.12 bounded attack loop: at most ten attempts classified by preflight.
/// Returns the loop's (unconsulted) success value.
pub fn attack_loop(
    env: &mut impl AttackEnv,
    mode: u8,
    min_to_hit: i32,
) -> Result<bool, WorldError> {
    let mut mode = mode;
    let mut taunt = true;
    for _ in 0..ATTACK_ATTEMPTS {
        if env.disabled() {
            return Ok(false);
        }
        match env.preflight(mode)? {
            0 => {
                if env.hit_chance(mode, true)? >= min_to_hit {
                    if !env.attack(mode)? || env.attack_cost(mode)? > env.ap() {
                        return Ok(false);
                    }
                } else {
                    if env.hit_chance(mode, false)? < min_to_hit {
                        env.flee()?;
                        return Ok(true);
                    }
                    let approached = env.approach(taunt)?;
                    if env.halted() {
                        return Ok(false);
                    }
                    if !approached {
                        env.flee()?;
                        return Ok(true);
                    }
                    taunt = false;
                }
            }
            1 => {
                if let Some(switched) = env.reload()? {
                    mode = switched.mode;
                }
            }
            2 => {
                if env.hit_chance(mode, false)? < min_to_hit {
                    env.flee()?;
                    return Ok(true);
                }
                let approach = if env.has_weapon() {
                    true
                } else {
                    let switched = env.switch_weapon()?;
                    mode = switched.mode;
                    !switched.success || !switched.weapon
                };
                if approach {
                    if !env.approach(taunt)? || env.halted() {
                        return Ok(false);
                    }
                    taunt = false;
                }
            }
            3 | 6 | 7 => {
                let switched = env.switch_weapon()?;
                mode = switched.mode;
                if !switched.success {
                    return Ok(false);
                }
            }
            5 => {
                if !env.approach(taunt)? || env.halted() {
                    return Ok(false);
                }
                taunt = false;
            }
            // 4: the target is already dead; the attempt is consumed.
            _ => {}
        }
    }
    Ok(false)
}
