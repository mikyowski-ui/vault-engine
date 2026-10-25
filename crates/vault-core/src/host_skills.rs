//! SPEC-013 host behavior: the skill-on request and stage, the default
//! skill routines (First Aid, Doctor, the kit route, Steal, Traps, Science,
//! Repair), skill-use experience, Sneak, poison and radiation, explosives,
//! barter entry, elevators, exit grids and map transitions.
use crate::{
    ObjectRef, RandomSource, WorldEffect, WorldError,
    assets::WorldAssets,
    combat_state::CombatRequest,
    host::WorldHost,
    host_drugs::Arg,
    queue::EventKind,
    rng,
    skills::{self, Gate, SkillTarget, skill},
    stats::Perk,
    world::{LootRequest, ScriptEvent, ScriptRequest, Transition},
};
use vault_formats::{
    Fid,
    map::{ObjectDetail, ObjectState},
    pro::{BasicFlags, Category, LockFlags},
};

/// §3.13 the kit bonus (fixed) and the kit PIDs (the PIDs alone are
/// overridable through `WorldRules`).
const KIT_BONUS: i32 = 20;
pub const FIRST_AID_KIT: u32 = 47;
pub const DOCTORS_BAG: u32 = 91;
/// Geiger counters (off and on) and their drain event type.
const GEIGER: [u32; 2] = [52, 207];
const DRAIN_EVENT: u32 = 9;
/// Player prototype flag bits: sneaking, and radiation exposure.
pub const SNEAK_FLAG: u32 = 0x01;
pub const EXPOSED_FLAG: u32 = 0x02;
/// Doctor's limb order and the crippled bits.
const LIMBS: [u32; 5] = [0x40, 0x10, 0x20, 0x08, 0x04];
pub(crate) const CRIPPLED: u32 = 0x7c;
/// Explosion damage type's DT and DR stats.
const EXPLOSION_DT: usize = 17 + 6;
const EXPLOSION_DR: usize = 24 + 6;
/// §3.10 a timed explosive meeting an explosion in progress retries after
/// 50 ticks (5 game seconds), as type 8.
pub const EXPLOSION_RETRY_TICKS: i64 = 50;
/// The explosion sound.
const EXPLOSION_SOUND: &[u8] = b"whn1xxx1";

/// SPEC-013 §3.25 the item-use routes a skill book can be read through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookRoute {
    /// The inventory window's action menu.
    Inventory,
    /// The interface bar's item slot.
    ItemSlot,
}

impl<A: WorldAssets> WorldHost<A> {
    /// §3.1-3.2 the skill-on request. Sneak toggles at once. Otherwise the
    /// gate rejects (`proto.msg` 902 for a player in combat) or the request
    /// is accepted and scheduled as one animation sequence (charging no
    /// AP): the approach, the adjacency check, the manipulation animation
    /// and, after it, the stage of §3.3. Returns whether it was accepted.
    pub fn use_skill_on(
        &mut self,
        actor: ObjectRef,
        target: ObjectRef,
        index: i32,
    ) -> Result<bool, WorldError> {
        if index == skill::SNEAK {
            self.toggle_sneak()?;
            return Ok(true);
        }
        let object = self
            .world
            .get(target)
            .ok_or_else(|| WorldError::Invalid("skill target missing".into()))?;
        let info = SkillTarget {
            category: object.category(),
            kill_type: object.character.as_ref().map(|sheet| sheet.kill_category),
            pid: object.raw.pid,
            is_actor: actor == target,
        };
        if let Gate::Rejected { combat } = skills::gate(index, self.world.combat, &info) {
            if combat && actor == self.world.player {
                self.print_game("proto.msg", 902, &[]);
            }
            return Ok(false);
        }
        // §3.2 steps 1-5 (`skill_motion`): a walking player is cleared
        // first (SPEC-014 §3.27), the approach and its forced adjacency
        // check follow, and the stage runs only after the manipulation
        // animation has completed.
        self.skill_sequence(actor, target, index)?;
        Ok(true)
    }
    /// §3.3 the skill-on stage: a jammed lock stops every skill (`misc.msg`
    /// 2001 for the player); then `use_skill_on_p_proc`, and unless it
    /// overrides, the default routine.
    pub(crate) fn skill_stage(
        &mut self,
        actor: ObjectRef,
        target: ObjectRef,
        index: i32,
    ) -> Result<(), WorldError> {
        let Some(object) = self.world.get(target) else {
            return Ok(());
        };
        if object
            .lock()
            .is_some_and(|lock| lock.contains(LockFlags::JAMMED))
        {
            if actor == self.world.player {
                self.print_game("misc.msg", 2001, &[]);
            }
            return Ok(());
        }
        if object.prototype.is_none() {
            return Ok(());
        }
        if let Some(script) = self.world.script_for(target) {
            let result = self.dispatch(ScriptRequest {
                script,
                event: ScriptEvent::UseSkill,
                source: actor,
                target,
                fixed: 0,
                action: index,
                only_present: false,
            })?;
            if result.pending || result.overridden {
                return Ok(());
            }
        }
        self.default_skill(actor, target, index)?;
        self.drain_requests()
    }
    /// §3.4 default routines.
    fn default_skill(
        &mut self,
        actor: ObjectRef,
        target: ObjectRef,
        index: i32,
    ) -> Result<(), WorldError> {
        match index {
            skill::FIRST_AID => {
                self.first_aid(actor, target, 0)?;
            }
            skill::DOCTOR => {
                self.doctor(actor, target, 0)?;
            }
            skill::SNEAK | skill::LOCKPICK => {}
            skill::STEAL => {
                self.world.pending_loot = Some(LootRequest {
                    looter: actor,
                    target,
                    steal: true,
                });
            }
            skill::TRAPS => self.print_game("skill.msg", 551, &[]),
            skill::SCIENCE => self.print_game("skill.msg", 552, &[]),
            skill::REPAIR => self.print_game("skill.msg", 553, &[]),
            _ => self.world.diagnostic("invalid skill used"),
        }
        Ok(())
    }
    fn skill_of(&self, critter: ObjectRef, index: i32) -> i32 {
        self.world
            .get(critter)
            .and_then(|object| object.character.as_ref())
            .map_or(0, |sheet| {
                sheet.skill(index as usize, self.world.context(critter))
            })
    }
    /// §3.5 skill-use experience: the player only; message 505 shows the
    /// actual change after Swift Learner and the cap.
    fn skill_use_experience(&mut self, actor: ObjectRef, index: i32, count: i32, kit: i32) {
        let amount = skills::skill_experience(index, count, kit, actor == self.world.player);
        if amount == 0 {
            return;
        }
        let experience = |host: &Self| {
            host.world
                .player_sheet()
                .map_or(0, |sheet| sheet.experience)
        };
        let before = experience(self);
        self.give_experience(amount);
        let gained = i64::from(experience(self)) - i64::from(before);
        self.print_game("skill.msg", 505, &[Arg::Int(gained)]);
    }
    fn usage_available(&self, index: i32) -> bool {
        skills::usage_slot(
            &self.world.persistence.skill_uses[index as usize],
            self.world.clock.ticks,
        )
        .is_some()
    }
    fn record_usage(&mut self, index: i32) {
        let now = self.world.clock.ticks;
        skills::record_use(&mut self.world.persistence.skill_uses[index as usize], now);
    }
    /// §3.6 an unavailable use: `skill.msg` 590 plus D(0, 2), for any actor.
    fn usage_refused(&mut self) -> Result<(), WorldError> {
        let draw = self.world.rng.range(0, 2)?;
        self.print_game("skill.msg", 590 + draw, &[]);
        Ok(())
    }
    /// Current and maximum HP of a critter.
    fn hit_point_range(&self, critter: ObjectRef) -> (i32, i32) {
        let hp = self.world.hit_points(critter).unwrap_or(0);
        let max = self.world.effective(critter, 7);
        (hp, max)
    }
    /// Add HP, capped at the maximum; the player's HP counter rolls.
    pub(crate) fn heal_critter(&mut self, critter: ObjectRef, amount: i32) {
        let (hp, max) = self.hit_point_range(critter);
        let healed = hp.saturating_add(amount).min(max);
        if let Some(object) = self.world.get_mut(critter) {
            if let Some(sheet) = object.character.as_mut() {
                sheet.current_hp = healed;
            }
            if let ObjectState::Critter(state) = &mut object.raw.state {
                state.hp = healed;
            }
        }
        if critter == self.world.player {
            self.world.emit(WorldEffect::RollHitPoints);
        }
    }
    fn name_of(&self, object: ObjectRef) -> Vec<u8> {
        self.world
            .get(object)
            .map(|o| o.name.clone())
            .unwrap_or_default()
    }
    fn fade(&mut self, out: bool) {
        self.world.emit(WorldEffect::Fade {
            out,
            duration: 0,
            from_black: false,
        });
    }
    /// SPEC-013 §3.25 reading a skill book through the item-use route;
    /// `false` when `book` is not one. In combat message 902 shows and the
    /// book is kept. Otherwise the skill gains `trunc((100 - S) / 10)` points
    /// bought with added unspent points (message 801 and no change when that
    /// is not positive); the screen fades to black; the clock advances
    /// `36000 × (11 - IN)` ticks directly, so due events wait for the next
    /// queue pass; the map-update scripts run; the screen fades back;
    /// messages 800 and the result show; and the book is destroyed, from the
    /// inventory after being placed on the player's hex, from the item slot
    /// with one more map update. No draws, no experience.
    pub fn read_book(&mut self, book: ObjectRef, route: BookRoute) -> Result<bool, WorldError> {
        let Some((skill, message)) = self
            .world
            .get(book)
            .and_then(|object| skills::skill_book(object.raw.pid))
        else {
            return Ok(false);
        };
        if self.world.combat {
            self.print_game("proto.msg", 902, &[]);
            return Ok(true);
        }
        let player = self.world.player;
        let points = skills::book_points(self.skill_of(player, skill));
        let result = if points > 0 { message } else { 801 };
        if let Some(sheet) = self.world.player_sheet_mut() {
            for _ in 0..points.max(0) {
                sheet.unspent_skills = sheet.unspent_skills.saturating_add(1);
                sheet.buy_skill(skill as usize);
            }
        }
        self.fade(true);
        let intelligence = self.world.effective(player, 4);
        self.world.clock.ticks += skills::book_ticks(intelligence);
        self.map_update()?;
        self.fade(false);
        self.print_game("proto.msg", 800, &[]);
        self.print_game("proto.msg", result, &[]);
        match route {
            BookRoute::Inventory => {
                self.world.drop_item(player, book, 1)?;
                self.world.erase(book);
            }
            BookRoute::ItemSlot => {
                self.world.destroy_unit(book)?;
                self.map_update()?;
            }
        }
        Ok(true)
    }
    /// §3.11 First Aid with kit bonus `kit`. Returns `false` only when the
    /// daily usage limit refused it (the routine's failure result). Safe
    /// rules: a dead target's message is displayed (as Doctor does), and the
    /// palette fades back on every exit after the fade to black.
    pub fn first_aid(
        &mut self,
        actor: ObjectRef,
        target: ObjectRef,
        kit: i32,
    ) -> Result<bool, WorldError> {
        let player = self.world.player;
        if !self.usage_available(skill::FIRST_AID) {
            self.usage_refused()?;
            return Ok(false);
        }
        if self.world.is_dead(target) {
            let draw = self.world.rng.range(0, 2)?;
            self.print_game("skill.msg", 512 + draw, &[]);
            return Ok(true);
        }
        let (hp, max) = self.hit_point_range(target);
        let healer = if actor == player {
            self.world.perk(Perk::Healer) as i32
        } else {
            0
        };
        let critical = self.world.effective(actor, 15);
        let mut count = 0;
        if hp < max {
            self.fade(true);
            let success = self.world.body_type(target) != 2
                && rng::skill_check(
                    self.skill_of(actor, skill::FIRST_AID),
                    critical + kit,
                    critical,
                    self.world.clock.ticks,
                    &mut self.world.rng,
                )?
                .code
                    >= 2;
            if success {
                let heal = self.world.rng.range(1 + 2 * healer, 5 + 5 * healer)?;
                self.heal_critter(target, heal);
                if actor == player {
                    self.print_game("skill.msg", 500, &[Arg::Int(i64::from(heal.min(max - hp)))]);
                }
                self.record_usage(skill::FIRST_AID);
                self.map_update()?;
                count = 1;
            } else {
                self.print_game("skill.msg", 503, &[]);
            }
            self.fade(false);
        } else if actor == player {
            self.healthy_message(target);
        }
        if actor == player {
            self.world.clock.ticks += 18_000;
        }
        self.skill_use_experience(actor, skill::FIRST_AID, count, kit);
        self.map_update()?;
        Ok(true)
    }
    fn healthy_message(&mut self, target: ObjectRef) {
        if target == self.world.player {
            self.print_game("skill.msg", 501, &[]);
        } else {
            let name = self.name_of(target);
            self.print_game("skill.msg", 502, &[Arg::Text(&name)]);
        }
    }
    /// §3.12 Doctor with kit bonus `kit`: each crippled bit in the fixed
    /// order (blind, left arm, right arm, right leg, left leg) gets a skill
    /// check, then the HP roll (bare Doctor, critical modifier CC + K). Safe
    /// rule: a limb attempt awards experience only when it succeeded.
    pub fn doctor(
        &mut self,
        actor: ObjectRef,
        target: ObjectRef,
        kit: i32,
    ) -> Result<bool, WorldError> {
        let player = self.world.player;
        if !self.usage_available(skill::DOCTOR) {
            self.usage_refused()?;
            return Ok(false);
        }
        if self.world.is_dead(target) {
            let draw = self.world.rng.range(0, 2)?;
            self.print_game("skill.msg", 512 + draw, &[]);
            return Ok(true);
        }
        let (hp, max) = self.hit_point_range(target);
        let crippled = self
            .world
            .critter_state(target)
            .map_or(0, |state| state.results.0 & CRIPPLED);
        let robot = self.world.body_type(target) == 2;
        let healer = if actor == player {
            self.world.perk(Perk::Healer) as i32
        } else {
            0
        };
        let critical = self.world.effective(actor, 15);
        let doctor = self.skill_of(actor, skill::DOCTOR);
        let mut attempts = 1;
        if hp < max || crippled != 0 {
            self.fade(true);
            let mut limb_healed = false;
            if !robot && crippled != 0 {
                for (position, bit) in LIMBS.into_iter().enumerate() {
                    if crippled & bit == 0 {
                        continue;
                    }
                    attempts += 1;
                    let success = rng::skill_check(
                        doctor,
                        critical + kit,
                        critical,
                        self.world.clock.ticks,
                        &mut self.world.rng,
                    )?
                    .code
                        >= 2;
                    let part = self
                        .game_text("skill.msg", 530 + position as i32)
                        .unwrap_or_default();
                    let yours = target == player;
                    if success {
                        if let Some(ObjectState::Critter(state)) =
                            self.world.get_mut(target).map(|o| &mut o.raw.state)
                        {
                            state.results.0 &= !bit;
                        }
                        self.record_usage(skill::DOCTOR);
                        limb_healed = true;
                    }
                    let id = match (success, yours) {
                        (true, true) => 520,
                        (true, false) => 521,
                        (false, true) => 525,
                        (false, false) => 526,
                    };
                    self.print_game("skill.msg", id, &[Arg::Text(&part)]);
                    self.skill_use_experience(actor, skill::DOCTOR, i32::from(success), kit);
                }
            }
            let success = !robot
                && rng::skill_check(
                    doctor,
                    0,
                    critical + kit,
                    self.world.clock.ticks,
                    &mut self.world.rng,
                )?
                .code
                    >= 2;
            if success {
                let heal = self.world.rng.range(4 + 2 * healer, 10 + 5 * healer)?;
                let (hp, max) = self.hit_point_range(target);
                self.heal_critter(target, heal);
                if actor == player {
                    self.print_game("skill.msg", 500, &[Arg::Int(i64::from(heal.min(max - hp)))]);
                }
                if !limb_healed {
                    self.record_usage(skill::DOCTOR);
                }
                self.skill_use_experience(actor, skill::DOCTOR, 1, kit);
            } else {
                self.print_game("skill.msg", 503, &[]);
            }
            self.map_update()?;
            self.fade(false);
        } else if actor == player {
            self.healthy_message(target);
        }
        if actor == player {
            self.world.clock.ticks += 36_000 * attempts as u64;
        }
        self.map_update()?;
        Ok(true)
    }
    /// The First Aid Kit and Doctor's Bag PIDs.
    pub fn kit_skill(&self, pid: u32) -> Option<i32> {
        if pid == self.world.rules.doctor_bag_pid.unwrap_or(DOCTORS_BAG) {
            Some(skill::DOCTOR)
        } else if pid == self.world.rules.first_aid_kit_pid.unwrap_or(FIRST_AID_KIT) {
            Some(skill::FIRST_AID)
        } else {
            None
        }
    }
    /// §3.13 the kit route: no `use_obj_on_p_proc`; refused in combat; the
    /// routine with K = 20; then, unless the usage limit refused, D(1, 10)
    /// where 1 uses the kit up; then one more map-update pass. Safe rule: a
    /// non-critter target shows `proto.msg` 582 and consumes nothing.
    pub fn use_kit(
        &mut self,
        actor: ObjectRef,
        kit: ObjectRef,
        target: ObjectRef,
    ) -> Result<(), WorldError> {
        let player = self.world.player;
        let pid = self.world.get(kit).map_or(0, |o| o.raw.pid);
        let Some(index) = self.kit_skill(pid) else {
            return Ok(());
        };
        if self.world.combat {
            if actor == player {
                self.print_game("proto.msg", 902, &[]);
            }
            return Ok(());
        }
        if self
            .world
            .get(target)
            .is_none_or(|o| o.category() != Category::Critter)
        {
            self.print_game("proto.msg", 582, &[]);
            return Ok(());
        }
        let completed = if index == skill::DOCTOR {
            self.doctor(actor, target, KIT_BONUS)?
        } else {
            self.first_aid(actor, target, KIT_BONUS)?
        };
        if !completed {
            return Ok(());
        }
        if self.world.rng.range(1, 10)? == 1 {
            if actor == player {
                let id = if index == skill::DOCTOR { 900 } else { 901 };
                self.print_game("proto.msg", id, &[]);
            }
            if let Some(owner) = self.world.get(kit).and_then(|o| o.owner) {
                self.world.remove_item(owner, kit, 1)?;
            }
            self.world.erase(kit);
        }
        self.map_update()
    }

    /// The player's sneak flag.
    pub fn sneaking(&self) -> bool {
        self.world
            .player_sheet()
            .is_some_and(|sheet| sheet.flags & SNEAK_FLAG != 0)
    }
    pub fn toggle_sneak(&mut self) -> Result<(), WorldError> {
        self.set_sneak(!self.sneaking())
    }
    /// §3.7 sneak on: set the flag and recheck at once (which queues the
    /// next type-10 recheck). Off: clear it and remove the rechecks.
    pub fn set_sneak(&mut self, on: bool) -> Result<(), WorldError> {
        let player = self.world.player;
        if let Some(sheet) = self.world.player_sheet_mut() {
            if on {
                sheet.flags |= SNEAK_FLAG;
            } else {
                sheet.flags &= !SNEAK_FLAG;
            }
        }
        if on {
            self.sneak_recheck()
        } else {
            self.world.queue.retain(|event| {
                !(event.object == player && event.kind == EventKind::Other(skills::SNEAK_EVENT))
            });
            Ok(())
        }
    }
    /// The recheck: "working" exactly when effective Sneak is at least 2 (the
    /// reference compares with the success code and draws nothing), then the
    /// next recheck in 600 ticks.
    fn sneak_recheck(&mut self) -> Result<(), WorldError> {
        let player = self.world.player;
        self.world.sneak_working = self.skill_of(player, skill::SNEAK) >= 2;
        self.world.queue.insert(
            self.world.clock.ticks,
            600,
            player,
            EventKind::Other(skills::SNEAK_EVENT),
        )?;
        Ok(())
    }

    /// §3.19 poison adjustment (the `poison` opcode, stat setting and drug
    /// effects): the player only; a positive amount less its resistance
    /// share (`trunc(amount × resistance / 100)`, step 2). A positive level
    /// removes the pending ticks, restarts one at `10 (505 - 5 level)` and
    /// shows `misc.msg` 3000 (step 4); otherwise the level is 0 and pending
    /// ticks stay queued (step 5, `[Must-match]`). The level is held at 0
    /// from below by step 5 itself, and the saturating addition is the safe
    /// bound on a crafted amount.
    pub fn adjust_poison(&mut self, critter: ObjectRef, amount: i32) -> Result<bool, WorldError> {
        let player = self.world.player;
        if critter != player {
            return Ok(false);
        }
        let resistance = i64::from(self.world.player_stat(32));
        let amount = if amount > 0 {
            let amount = i64::from(amount);
            (amount - amount * resistance / 100) as i32
        } else {
            amount
        };
        let level = self
            .world
            .player_sheet()
            .map_or(0, |sheet| sheet.poison)
            .saturating_add(amount);
        let level = level.max(0);
        if let Some(sheet) = self.world.player_sheet_mut() {
            sheet.poison = level;
        }
        if level > 0 {
            self.world
                .queue
                .retain(|event| !(event.object == player && event.kind == EventKind::Poison));
            self.world.queue.insert(
                self.world.clock.ticks,
                skills::poison_delay(level).max(0),
                player,
                EventKind::Poison,
            )?;
            self.print_game("misc.msg", 3000, &[]);
        }
        Ok(true)
    }
    /// The poison tick: adjust by -2, lose 1 HP (dying normally at 0),
    /// show 3001, and interrupt at 5 HP or below.
    fn poison_tick(&mut self, owner: ObjectRef) -> Result<i32, WorldError> {
        let player = self.world.player;
        if owner != player {
            return Ok(0);
        }
        self.adjust_poison(player, -2)?;
        self.damage_critter(player, None, 1, 0)?;
        self.print_game("misc.msg", 3001, &[]);
        Ok(i32::from(self.world.hit_points(player).unwrap_or(0) <= 5))
    }
    /// §3.20 radiation exposure: the player only (others are a silent
    /// no-op); a positive amount less its resistance share sets the exposed
    /// flag; an active Geiger counter reports it (1009 above 5, else 1008);
    /// 10 or more shows 1007; the total never drops below 0.
    pub fn adjust_radiation(&mut self, critter: ObjectRef, amount: i32) -> bool {
        if critter != self.world.player {
            return false;
        }
        let resistance = i64::from(self.world.player_stat(31));
        let amount = if amount > 0 {
            let amount = i64::from(amount);
            (amount - resistance * amount / 100) as i32
        } else {
            amount
        };
        if amount > 0 {
            if let Some(sheet) = self.world.player_sheet_mut() {
                sheet.flags |= EXPOSED_FLAG;
            }
            if self.geiger_active() {
                self.print_game("misc.msg", if amount > 5 { 1009 } else { 1008 }, &[]);
            }
        }
        if amount >= 10 {
            self.print_game("misc.msg", 1007, &[]);
        }
        if let Some(sheet) = self.world.player_sheet_mut() {
            sheet.radiation = sheet.radiation.saturating_add(amount).max(0);
        }
        true
    }
    /// A Geiger counter in either hand with charges and an active drain.
    fn geiger_active(&self) -> bool {
        let player = self.world.player;
        let Some(object) = self.world.get(player) else {
            return false;
        };
        object.inventory.iter().any(|(item, _)| {
            self.world.get(*item).is_some_and(|counter| {
                GEIGER.contains(&counter.raw.pid)
                    && counter.raw.flags.0 & (BasicFlags::LEFT_HAND.0 | BasicFlags::RIGHT_HAND.0)
                        != 0
                    && matches!(
                        counter.raw.state,
                        ObjectState::Other {
                            detail: ObjectDetail::Charges(charges),
                            ..
                        } if charges > 0
                    )
                    && self.world.queue.events().any(|event| {
                        event.object == *item && event.kind == EventKind::Other(DRAIN_EVENT)
                    })
            })
        })
    }
    /// §3.20 the midnight check: for an exposed player, the last scheduled
    /// level against the base level (raised by 1 when D(1, 10) exceeds
    /// Endurance plus the level's modifier); a higher level queues an onset
    /// D(4, 18) game hours later. The exposed flag is cleared either way.
    pub(crate) fn radiation_check(&mut self) -> Result<(), WorldError> {
        let player = self.world.player;
        let Some(sheet) = self.world.player_sheet() else {
            return Ok(());
        };
        if sheet.flags & EXPOSED_FLAG == 0 {
            return Ok(());
        }
        let total = sheet.radiation;
        let scheduled = self
            .world
            .queue
            .events()
            .filter(|event| event.object == player)
            .filter_map(|event| match event.kind {
                EventKind::Radiation { level, .. } => Some(level),
                _ => None,
            })
            .last()
            .unwrap_or(0);
        let (mut level, modifier) = skills::radiation_level(total);
        if self.world.rng.range(1, 10)? > self.world.player_stat(2) + modifier {
            level += 1;
        }
        if level > scheduled {
            let hours = self.world.rng.range(4, 18)?;
            self.world.queue.insert(
                self.world.clock.ticks,
                36_000 * i64::from(hours),
                player,
                EventKind::Radiation { level, healing: 0 },
            )?;
        }
        if let Some(sheet) = self.world.player_sheet_mut() {
            sheet.flags &= !EXPOSED_FLAG;
        }
        Ok(())
    }
    /// §3.20 an onset removes every other radiation event (applying removed
    /// recoveries first), queues its recovery 7 days later, and applies the
    /// level; a recovery reverses its level. Safe rule: events of another
    /// owner are ignored.
    fn radiation_event(
        &mut self,
        owner: ObjectRef,
        level: i32,
        healing: i32,
    ) -> Result<(), WorldError> {
        if owner != self.world.player {
            return Ok(());
        }
        if healing != 0 {
            return self.apply_radiation(owner, level, -1);
        }
        let removed: Vec<_> = self
            .world
            .queue
            .events()
            .filter(|event| matches!(event.kind, EventKind::Radiation { .. }))
            .cloned()
            .collect();
        for event in removed {
            self.world.queue.remove(event.id);
            // Safe rule: another owner's recovery is not applied.
            if let EventKind::Radiation { level, healing: 1 } = event.kind
                && event.object == owner
            {
                self.apply_radiation(event.object, level, -1)?;
            }
        }
        self.world.queue.insert(
            self.world.clock.ticks,
            skills::RADIATION_RECOVERY_TICKS as i64,
            owner,
            EventKind::Radiation { level, healing: 1 },
        )?;
        self.apply_radiation(owner, level, 1)
    }
    /// Apply level `level` with `sign`: the stage message, each §2.6 column
    /// through the stat bonuses (current HP directly: a loss can kill, a gain
    /// is capped), death when a SPECIAL falls below 1, and 1006 for a dead
    /// player.
    fn apply_radiation(
        &mut self,
        critter: ObjectRef,
        level: i32,
        sign: i32,
    ) -> Result<(), WorldError> {
        // SPEC-013 §3.20 applies levels 1-6 (the §2.6 rows); the reference
        // indexes its penalty table with the raw event level, so an event
        // carrying another level applies nothing (safe bound).
        let Some(row) = usize::try_from(level - 1)
            .ok()
            .and_then(|row| skills::RADIATION_PENALTIES.get(row))
        else {
            return Ok(());
        };
        let player = self.world.player;
        if critter == player {
            self.print_game("misc.msg", 1000 + level - 1, &[]);
        }
        let context = self.world.context(critter);
        for (penalty, stat) in row.iter().zip(skills::RADIATION_STATS) {
            let delta = sign * penalty;
            if delta == 0 {
                continue;
            }
            match stat {
                Some(stat) => {
                    if let Some(sheet) = self
                        .world
                        .get_mut(critter)
                        .and_then(|o| o.character.as_mut())
                    {
                        sheet.adjust_bonus(stat, delta, context);
                    }
                }
                None if delta < 0 => {
                    self.damage_critter(critter, None, -delta, 0)?;
                    if critter == self.world.player {
                        self.world.emit(WorldEffect::RollHitPoints);
                    }
                }
                None => self.heal_critter(critter, delta),
            }
        }
        let below_one = self
            .world
            .get(critter)
            .and_then(|o| o.character.as_ref())
            .is_some_and(|sheet| {
                (0..7).any(|stat| sheet.raw(stat, context) + i64::from(sheet.bonus[stat]) < 1)
            });
        if !self.world.is_dead(critter) && below_one {
            self.kill_critter(critter, 20)?;
        }
        if critter == player && self.world.is_dead(player) {
            self.print_game("misc.msg", 1006, &[]);
        }
        Ok(())
    }

    /// §3.10 arming step 1: an explosive already carrying the in-use flag
    /// shows `proto.msg` 590 and opens no timer.
    pub fn explosive_armable(&mut self, item: ObjectRef) -> bool {
        let used = self
            .world
            .get(item)
            .is_some_and(|object| object.raw.flags.contains(BasicFlags::USED));
        if used {
            self.print_game("proto.msg", 590, &[]);
        }
        !used
    }
    /// §3.10 steps 3-5 after the Set Timer dialog accepted `seconds`:
    /// `proto.msg` 589, the permanent armed PID, the player's Traps check, and
    /// the explosion event owned by the explosive (which marks it in use).
    /// Only one unit is armed: from a stack it is split off first as its
    /// own entry (keeping the hand), so the rest keeps its PID, carries no
    /// in-use flag and stays armable.
    pub fn arm_explosive(&mut self, item: ObjectRef, seconds: i32) -> Result<(), WorldError> {
        let item = self.world.split_unit(item)?;
        self.print_game("proto.msg", 589, &[]);
        let pid = self.world.get(item).map_or(0, |o| o.raw.pid);
        if let Some(armed) = skills::armed_pid(pid) {
            let prototype = self.assets.prototype(armed).ok();
            if let Some(object) = self.world.get_mut(item) {
                object.raw.pid = armed;
                if prototype.is_some() {
                    object.prototype = prototype;
                }
            }
        }
        let player = self.world.player;
        let check = rng::skill_check(
            self.skill_of(player, skill::TRAPS),
            0,
            self.world.effective(player, 15),
            self.world.clock.ticks,
            &mut self.world.rng,
        )?;
        let (kind, delay) = skills::explosive_timer(check.code, seconds);
        self.world.queue.insert(
            self.world.clock.ticks,
            delay as i64,
            item,
            EventKind::Other(kind),
        )?;
        if let Some(object) = self.world.get_mut(item) {
            object.raw.flags.0 |= BasicFlags::USED.0;
        }
        Ok(())
    }
    /// §3.10 detonation of a timed explosive at the outermost holder's hex
    /// (or its own). While another animated explosion is in progress an
    /// animated detonation is re-queued as type 8 after 50 ticks, and the
    /// explosive is kept; otherwise the explosion starts, credited to the
    /// player, and the explosive is destroyed.
    pub(crate) fn detonate(
        &mut self,
        explosive: ObjectRef,
        animated: bool,
    ) -> Result<(), WorldError> {
        if animated && self.explosion_in_progress() {
            self.world.queue.insert(
                self.world.clock.ticks,
                EXPLOSION_RETRY_TICKS,
                explosive,
                EventKind::Other(skills::EXPLOSION_EVENT),
            )?;
            return Ok(());
        }
        let holder = self.world.outermost_owner(explosive);
        let center = self.world.get(holder).and_then(|object| {
            Some((
                object.raw.hex?,
                object.raw.elevation,
                self.world.get(explosive)?.raw.pid,
            ))
        });
        if let Some((hex, elevation, pid)) = center {
            let (min, max) = skills::explosion_range(pid);
            let player = self.world.player;
            self.start_explosion(hex, elevation, min, max, Some(player), animated)?;
        }
        if let Some(owner) = self.world.get(explosive).and_then(|o| o.owner) {
            self.world.remove_item(owner, explosive, 1)?;
        }
        self.world.erase(explosive);
        Ok(())
    }
    /// §3.10 only one animated explosion can be in progress: a timed or
    /// scripted one until its damage step, a weapon explosion while its
    /// projectile flies (SPEC-007 §3.9) and its art plays.
    fn explosion_in_progress(&self) -> bool {
        self.world.explosion.is_some() || self.world.shows.iter().any(|show| show.explosive)
    }
    /// §3.10 the seven arts of an explosion at `center` with art `fid`:
    /// its frame count per direction and rate (a stored rate of 0 is 10
    /// frames per second); art that cannot be loaded shows nothing.
    pub(crate) fn explosion_art(
        &self,
        center: i32,
        elevation: u8,
        fid: u32,
        first_delay: u32,
    ) -> crate::world::ExplosionArt {
        let timing = self.assets.object_art(fid).ok().map_or([0, 100], |art| {
            [
                art.frame_count() as u64,
                art.frame_interval_ms().unwrap_or(100),
            ]
        });
        crate::world::ExplosionArt::new(center, elevation, fid, timing, first_delay)
    }
    /// §3.10 an explosion at `hex`: damage covers only the living critter
    /// blocking the center hex, computed now with one D(min, max) draw (the
    /// splash enumeration is disabled in R1, so nothing else takes damage).
    /// An animated explosion plays `whn1xxx1` with its art, `roktxpd.frm` on
    /// the center and the six adjacent hexes, and stays in progress until
    /// its damage step (`step_explosions`); an unanimated one applies the
    /// damage, then notifies the scenery scripts, at once.
    fn start_explosion(
        &mut self,
        hex: i32,
        elevation: u8,
        min: i32,
        max: i32,
        source: Option<ObjectRef>,
        animated: bool,
    ) -> Result<(), WorldError> {
        let victim = self
            .world
            .movement_blocker(hex, elevation, 0)
            .filter(|object| {
                self.world
                    .get(*object)
                    .is_some_and(|o| Fid(o.raw.art_id).category() == 1)
                    && !self.world.is_dead(*object)
            });
        let victim = match victim {
            Some(victim) => {
                let draw = self.world.rng.range(min, max)?;
                let multihex = self
                    .world
                    .get(victim)
                    .is_some_and(|o| o.raw.flags.contains(BasicFlags::MULTIHEX));
                let (damage, knockback) = skills::explosion_damage(
                    draw,
                    self.world.effective(victim, EXPLOSION_DT),
                    self.world.effective(victim, EXPLOSION_DR),
                    multihex,
                );
                Some((victim, damage, knockback))
            }
            None => None,
        };
        let art = self.explosion_art(hex, elevation, skills::explosion_art::ROCKET, 0);
        let explosion = crate::world::ActiveExplosion {
            hex,
            elevation,
            victim,
            source,
            art,
            notified: false,
        };
        if animated {
            self.world.emit(WorldEffect::SoundEffect {
                name: EXPLOSION_SOUND.to_vec(),
                owner: None,
            });
            self.world.emit(WorldEffect::Explosion {
                hex,
                elevation: i32::from(elevation),
                min,
                max,
            });
            self.world.explosion = Some(explosion);
            return Ok(());
        }
        self.apply_explosion(&explosion)?;
        self.notify_explosion_scripts(hex, elevation, source)
    }
    /// §3.10 one poll of the explosions at `now_ms`. The animated explosion
    /// in progress advances its art; once the center art has played the
    /// scenery scripts are notified; once every art has played and been
    /// removed the damage and attribution apply, ending the in-progress
    /// state. Weapon explosions play in their attack's show instead.
    pub(crate) fn step_explosions(&mut self, now_ms: u64) -> Result<bool, WorldError> {
        let Some(explosion) = self.world.explosion.as_mut() else {
            return Ok(false);
        };
        let mut changed = explosion.art.poll(now_ms);
        if explosion.art.center_done() && !explosion.notified {
            explosion.notified = true;
            let (hex, elevation, source) = (explosion.hex, explosion.elevation, explosion.source);
            self.notify_explosion_scripts(hex, elevation, source)?;
        }
        if let Some(explosion) = self
            .world
            .explosion
            .take_if(|explosion| explosion.art.finished())
        {
            self.apply_explosion(&explosion)?;
            changed = true;
        }
        Ok(changed)
    }
    /// §3.10 the computed damage on the center critter (still a living
    /// critter), its knockback, and the credited source's attribution: kill
    /// experience for a death, otherwise the victim references the source
    /// and the first such victim requests combat. A scripted explosion has
    /// no source, so none of this attribution happens.
    fn apply_explosion(
        &mut self,
        explosion: &crate::world::ActiveExplosion,
    ) -> Result<(), WorldError> {
        let Some((victim, damage, knockback)) = explosion.victim else {
            return Ok(());
        };
        if self.world.get(victim).is_none() || self.world.is_dead(victim) {
            return Ok(());
        }
        // SPEC-007 §3.12: the explosion's message lines precede its damage.
        let lines = self.damage_lines(victim, damage);
        self.print_lines(lines);
        self.damage_critter(victim, explosion.source, damage, 0)?;
        let player = self.world.player;
        if knockback > 0 && !self.world.is_dead(victim) {
            self.knock_back(explosion.source.unwrap_or(player), victim, knockback)?;
        }
        let Some(source) = explosion.source else {
            return Ok(());
        };
        if self.world.is_dead(victim) {
            // Out of combat the kill's experience goes to the player now.
            if !self.world.combat {
                let experience = std::mem::take(&mut self.world.combat_experience);
                if experience > 0 {
                    self.give_experience(experience);
                }
            }
        } else if victim != source {
            self.set_last_attacker(victim, source);
            if !self.world.combat && self.world.pending_combat.is_none() {
                self.world.pending_combat = Some(CombatRequest {
                    attacker: victim,
                    defender: source,
                    accuracy: 0,
                    damage: [0, i32::MAX],
                    flags: None,
                });
            }
        }
        Ok(())
    }
    /// §3.10 scenery and other blockers: the engine never damages them.
    /// With spatial checks off, collect in script-list order every item or
    /// scenery object script within hex distance 3 of the center on its
    /// elevation, then every spatial script there, keeping those with a
    /// `damage_p_proc` (a program not yet loaded runs `start` first); then
    /// run each collected `damage_p_proc` once. Critter scripts are not
    /// notified.
    fn notify_explosion_scripts(
        &mut self,
        hex: i32,
        elevation: u8,
        source: Option<ObjectRef>,
    ) -> Result<(), WorldError> {
        let near = |at: i32| {
            vault_formats::HexGrid::angular_distance(at, hex).is_ok_and(|distance| distance <= 3)
        };
        let mut objects = Vec::new();
        let mut spatials = Vec::new();
        for script in self
            .world
            .script_order
            .iter()
            .filter(|slot| slot.inert.is_none())
            .filter_map(|slot| self.world.scripts.get(&slot.id))
        {
            if let Some((at, level, _)) = script.spatial {
                if level == elevation && near(at) {
                    spatials.push((script.id, 0));
                }
                continue;
            }
            let Some(owner) = self.world.get(script.owner) else {
                continue;
            };
            let category = owner.category();
            if matches!(category, Category::Item | Category::Scenery)
                && owner.raw.elevation == elevation
                && owner.raw.hex.is_some_and(near)
            {
                objects.push((script.id, script.owner));
            }
        }
        let previous = self.world.spatial_enabled;
        self.world.spatial_enabled = false;
        let mut collected = Vec::new();
        for (script, owner) in objects.into_iter().chain(spatials) {
            let loaded = self
                .world
                .scripts
                .get(&script)
                .is_some_and(|instance| instance.initialized);
            if !loaded {
                let started = self.dispatch(ScriptRequest {
                    script,
                    event: ScriptEvent::Start,
                    source: 0,
                    target: owner,
                    fixed: 0,
                    action: -1,
                    only_present: false,
                });
                if let Err(error) = started {
                    self.world
                        .diagnostic(format!("explosion script start: {error}"));
                }
            }
            if self.has_procedure(script, ScriptEvent::Damage) {
                collected.push((script, owner));
            }
        }
        self.world.spatial_enabled = previous;
        for (script, owner) in collected {
            let result = self.dispatch(ScriptRequest {
                script,
                event: ScriptEvent::Damage,
                source: source.unwrap_or(0),
                target: owner,
                fixed: 0,
                action: -1,
                only_present: true,
            });
            if let Err(error) = result {
                self.world
                    .diagnostic(format!("explosion damage_p_proc: {error}"));
            }
        }
        Ok(())
    }
    /// §3.10 the request pass for a scripted explosion: in combat the
    /// combat request pass discards it; out of combat it detonates with
    /// animation and no credited source. One that finds another animated
    /// explosion in progress waits for the next pass (safe rule; the
    /// reference drops it).
    pub(crate) fn service_explosion_request(&mut self) -> Result<bool, WorldError> {
        let Some(request) = self.world.pending_explosion else {
            return Ok(false);
        };
        if self.world.combat {
            self.world.pending_explosion = None;
            return Ok(false);
        }
        if self.explosion_in_progress() {
            return Ok(false);
        }
        self.world.pending_explosion = None;
        let Ok(elevation) = u8::try_from(request.elevation)
            .map_err(|_| ())
            .and_then(|e| if e <= 2 { Ok(e) } else { Err(()) })
        else {
            self.world.diagnostic(format!(
                "explosion elevation {} is invalid",
                request.elevation
            ));
            return Ok(false);
        };
        if !(0..40000).contains(&request.hex) {
            self.world
                .diagnostic(format!("explosion hex {} is invalid", request.hex));
            return Ok(false);
        }
        self.start_explosion(request.hex, elevation, request.min, request.max, None, true)?;
        Ok(true)
    }
    /// The queued handlers for SPEC-013's payload-free events: explosions
    /// (type 11 first shows `misc.msg` 4000) and sneak rechecks.
    pub(crate) fn skill_event(&mut self, object: ObjectRef, kind: u32) -> Result<i32, WorldError> {
        match kind {
            skills::EXPLOSION_EVENT | skills::PREMATURE_EVENT => {
                if kind == skills::PREMATURE_EVENT {
                    self.print_game("misc.msg", 4000, &[]);
                }
                self.detonate(object, true)?;
            }
            skills::SNEAK_EVENT if object == self.world.player && self.sneaking() => {
                self.sneak_recheck()?;
            }
            skills::SNEAK_EVENT => {}
            other => self
                .world
                .diagnostic(format!("timed maintenance event type {other}")),
        }
        Ok(0)
    }
    pub(crate) fn poison_or_radiation_event(
        &mut self,
        object: ObjectRef,
        kind: EventKind,
    ) -> Result<i32, WorldError> {
        match kind {
            EventKind::Poison => self.poison_tick(object),
            EventKind::Radiation { level, healing } => {
                self.radiation_event(object, level, healing)?;
                Ok(0)
            }
            _ => Ok(0),
        }
    }
    /// §2.5 map-exit handlers: armed explosives detonate at once, without
    /// animation or message 4000, on the map being left; sneak turns off.
    pub(crate) fn skill_map_exit(&mut self) -> Result<(), WorldError> {
        let events: Vec<_> = self
            .world
            .queue
            .events()
            .filter(|event| {
                matches!(
                    event.kind,
                    EventKind::Other(skills::EXPLOSION_EVENT | skills::PREMATURE_EVENT)
                )
            })
            .cloned()
            .collect();
        for event in events {
            self.world.queue.remove(event.id);
            self.detonate(event.object, false)?;
        }
        if self.sneaking() {
            self.set_sneak(false)?;
        }
        Ok(())
    }

    /// §3.18 barter entry (`gdialog_barter`, which always replaces the
    /// script modifier, SPEC-011 §3.17): a merchant prototype without flag
    /// `0x02` shows `proto.msg` 903 in the reply area instead of switching.
    pub fn request_barter(&mut self, modifier: i32) -> bool {
        self.world.dialogue.barter_modifier = modifier;
        self.barter_entry()
    }
    /// §3.18 the barter request from the dialogue control panel (the
    /// Barter button or `b`, SPEC-014 §3.45): the same merchant test,
    /// keeping the script barter modifier.
    pub fn barter_entry(&mut self) -> bool {
        let merchant = self.world.dialogue.target;
        // SPEC-019 §3.7: a Fallout 2 party member with a control record
        // trades without the prototype's barter flag.
        let trades = self.control_of(merchant).is_some();
        let barters = trades
            || self
                .world
                .get(merchant)
                .and_then(|object| object.prototype.as_ref())
                .is_some_and(|prototype| match &prototype.data {
                    vault_formats::pro::PrototypeData::Critter(critter) => {
                        critter.flags.0 & skills::BARTER_FLAG != 0
                    }
                    _ => false,
                });
        if barters {
            self.world.dialogue.pending_barter = true;
        } else if let Some(text) = self.game_text("proto.msg", 903) {
            self.world.dialogue.resolved_reply = text;
        }
        barters
    }

    /// §3.21 the elevator window's start level (safe rule: the entry for
    /// the current map and the player's elevation, else 0).
    pub fn elevator_start_level(&self, kind: i32) -> usize {
        let elevation = self
            .world
            .get(self.world.player)
            .map_or(0, |o| o.raw.elevation);
        skills::elevator(kind).map_or(0, |elevator| {
            elevator.start_level(self.world.map.header.map_number, elevation)
        })
    }
    /// §3.21 performing an elevator choice. Safe rule: Cancel (`None`)
    /// changes nothing. The same map places the player (closing a nearby
    /// elevator door when the elevation changes); another map closes the
    /// door and requests a transition facing southeast.
    pub fn perform_elevator(&mut self, kind: i32, choice: Option<usize>) -> Result<(), WorldError> {
        let Some(&(map, elevation, hex)) = skills::elevator(kind)
            .zip(choice)
            .and_then(|(elevator, choice)| elevator.destinations.get(choice))
        else {
            return Ok(());
        };
        self.world.capture_automap();
        let player = self.world.player;
        let current = self.world.get(player).map_or(0, |o| o.raw.elevation);
        if map == self.world.map.header.map_number {
            let door = (elevation != current)
                .then(|| self.elevator_door())
                .flatten();
            self.place_player(hex, elevation, 2)?;
            if elevation != current {
                match door {
                    Some(door) => self.close_door_instantly(door),
                    None => self.world.diagnostic("no elevator door near the player"),
                }
            }
        } else {
            // The combat pass does not close the door on a map change.
            if !self.world.combat
                && let Some(door) = self.elevator_door()
            {
                self.close_door_instantly(door);
            }
            self.request_transition(Transition {
                map,
                elevation: i32::from(elevation),
                hex,
                facing: 2,
            });
        }
        Ok(())
    }
    /// The first elevator door within 4 hexes of the player on its
    /// elevation, in object order.
    fn elevator_door(&self) -> Option<ObjectRef> {
        let player = self.world.player;
        let (hex, elevation) = self
            .world
            .get(player)
            .and_then(|o| Some((o.raw.hex?, o.raw.elevation)))?;
        self.world.iter().find_map(|(reference, object)| {
            (skills::ELEVATOR_DOORS.contains(&object.raw.pid)
                && object.raw.elevation == elevation
                && object.raw.hex.is_some_and(|door| {
                    vault_formats::HexGrid::lattice_distance(hex, door).is_ok_and(|d| d <= 4)
                }))
            .then_some(reference)
        })
    }
    /// Close a door with no sound or animation: frame 0, its pass-through
    /// flags and open bit cleared, lighting recomputed.
    fn close_door_instantly(&mut self, door: ObjectRef) {
        if let Some(object) = self.world.get_mut(door) {
            object.raw.frame = 0;
            object.raw.flags.0 &=
                !(0x10 | BasicFlags::LIGHT_THROUGH.0 | BasicFlags::PROJECTILE_THROUGH.0);
            if let Some(mut lock) = object.lock() {
                lock.0 &= !LockFlags::OPEN.0;
                object.set_lock(lock);
            }
        }
        self.world.rebuild_lighting();
    }
    /// §3.21 placement: animations cancelled and the facing set; an
    /// unblocked destination, else the first unblocked neighbor (NE, E, SE,
    /// SW, W, NW), else a hex 6 away. An elevation change ends combat and
    /// re-syncs the party.
    pub(crate) fn place_player(
        &mut self,
        hex: i32,
        elevation: u8,
        facing: u8,
    ) -> Result<(), WorldError> {
        let player = self.world.player;
        if hex < 0 {
            return Ok(());
        }
        self.clear_animation(player);
        if let Some(object) = self.world.get_mut(player) {
            object.raw.orientation = facing;
        }
        let free = |host: &Self, hex: i32| {
            host.world
                .movement_blocker(hex, elevation, player)
                .is_none()
        };
        let destination = if free(self, hex) {
            hex
        } else {
            (0..6)
                .filter_map(|direction| vault_formats::HexGrid::step(hex, direction, 1).ok())
                .find(|neighbor| free(self, *neighbor))
                .or_else(|| vault_formats::HexGrid::step(hex, 0, 6).ok())
                .unwrap_or(hex)
        };
        let previous = self.world.get(player).map_or(0, |o| o.raw.elevation);
        // The move re-syncs the party on an elevation change.
        self.world
            .move_object(player, destination, elevation, false)?;
        if previous != elevation && self.world.combat {
            self.end_combat()?;
        }
        Ok(())
    }
    /// SPEC-008 §3.10 / §3.22 step 3: after a map is loaded by number the
    /// party is moved on the world map to the map's fixed table town by the
    /// script-directed town travel of SPEC-008 §3.4, run to completion
    /// (`scripted_town_travel`): no discovery reveal, path marks, encounter
    /// tests, walk-mask checks or queue processing. Nothing moves for town
    /// -1 (an unnumbered map) or when the party's cell is already the
    /// town's cell. The current town does not change (SPEC-014 §3.39).
    pub fn move_to_map_town(&mut self) -> Result<(), WorldError> {
        // SPEC-018 §5.4: under Fallout 2 the map load has already moved the
        // party to its area, with no travel.
        if self.world.fallout2.is_some() {
            return Ok(());
        }
        let town = self
            .world
            .rules
            .tables()
            .map_town(self.world.map.header.map_number);
        self.scripted_town_travel(town)
    }
    /// §3.22 a transition request: one made in combat ends combat; the last
    /// request wins.
    pub fn request_transition(&mut self, transition: Transition) {
        if self.world.combat {
            self.world.combat_state.flags |= crate::critical::state::END_REQUESTED;
        }
        self.world.pending_transition = Some(transition);
    }
    /// §3.22 processing out of combat: map 0 (an exit grid's world map,
    /// also requested as -2) opens the world map and -1 the town map, once
    /// the outgoing map is saved and cleared (SPEC-014 §3.35); a failed
    /// save ends the session instead. Otherwise the map loads by number
    /// and, for a hex on the grid other than 0 and an elevation 0-2, the
    /// player is placed there with the facing; the party then follows the
    /// final placement. A transition that fails (an unknown map, a load or
    /// placement error) is a diagnostic, not a failed frame, and the
    /// requests made during the load are cleared either way. Returns
    /// whether a transition ran.
    pub fn process_transition(&mut self) -> Result<bool, WorldError> {
        if self.world.combat {
            return Ok(false);
        }
        let Some(transition) = self.world.pending_transition.take() else {
            return Ok(false);
        };
        match transition.map {
            -2..=0 => match self.unload_map() {
                Ok(false) => self.world.emit(WorldEffect::MapSaveFailed),
                Ok(true) if transition.map == -1 => {
                    self.transition_world_map = true;
                    self.world.emit(WorldEffect::TownMap);
                }
                Ok(true) => {
                    self.transition_world_map = true;
                    // SPEC-008 §3.10: the world map plays `03wrldmp`.
                    self.play_world_map_music();
                    self.world.emit(WorldEffect::WorldMap);
                }
                Err(error) => {
                    self.world
                        .diagnostic(format!("leaving the map failed: {error}"));
                    self.world.emit(WorldEffect::MapSaveFailed);
                }
            },
            map => {
                let result = self.transition_to(map, transition);
                // SPEC-013 §3.22 [May-improve]: steps 2 and 4 use the hex,
                // elevation and facing saved when processing started, and
                // a request made during the load (an exit grid at the
                // entrance or the step-2 hex, a script's `load_map`) is
                // cleared rather than replacing them, even when the
                // transition fails.
                self.world.pending_transition = None;
                if let Err(error) = result {
                    self.world
                        .diagnostic(format!("map transition to {map} failed: {error}"));
                }
            }
        }
        Ok(true)
    }
    /// §3.22 steps 1-4 for a positive map number.
    fn transition_to(&mut self, map: i32, transition: Transition) -> Result<(), WorldError> {
        // SPEC-018 §5.1: under Fallout 2 a number is a `maps.txt` record.
        let path = if self.world.fallout2.is_some() {
            self.fallout2_map_path(map)
        } else {
            self.assets.map_path(map)
        }
        .ok_or_else(|| WorldError::Invalid(format!("map {map} is not available")))?;
        self.load_map(&path)?;
        // Step 3: the party moves on the world map to the map's town.
        self.move_to_map_town()?;
        // Safe rule: a hex off the grid keeps the entrance, as -1 and 0 do.
        if (1..40000).contains(&transition.hex) && (0..=2).contains(&transition.elevation) {
            let facing = transition.facing.clamp(0, 5) as u8;
            self.place_player(transition.hex, transition.elevation as u8, facing)?;
            // The party follows the final placement on the same elevation
            // too (an elevation change has re-synced it already).
            self.world.place_party()?;
        }
        if transition.facing >= 0
            && let Some(object) = self.world.get_mut(self.world.player)
        {
            object.raw.orientation = transition.facing.clamp(0, 5) as u8;
        }
        Ok(())
    }
}
