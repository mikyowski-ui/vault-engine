//! SPEC-011 §3.1-3.9 drug consumption on the world: the re-dose rule, stage
//! application (prototype-shared bonuses, current HP with overdose death,
//! player-only poison/radiation, gained/lost messages), delayed stages,
//! the addiction check, withdrawal onset and end, map-exit handling, and
//! the §3.6 inventory-use and use-on paths. SPEC-009 NPC healing consumes
//! through [`WorldHost::consume_drug`] (the §3.7 contract).
use crate::{
    ObjectRef, Perk, RandomSource, Trait, WorldEffect, WorldError, WorldState,
    ai::intent,
    assets::WorldAssets,
    drugs::{self, Sensitivity, Stage},
    host::WorldHost,
    progression::ADDICTED_FLAG,
    queue::{EventKind, TimedEvent},
};
use vault_formats::{
    map::ObjectState,
    pro::{Category, Drug, ItemData},
};

/// A `%d` or `%s` message argument.
pub(crate) enum Arg<'a> {
    Int(i64),
    Text(&'a [u8]),
}
/// Substitute `%d` and `%s` conversions in order (`%%` is a percent sign);
/// conversions without an argument are dropped.
pub(crate) fn format_message(template: &[u8], args: &[Arg]) -> Vec<u8> {
    let mut out = Vec::with_capacity(template.len() + 16);
    let mut args = args.iter();
    let mut bytes = template.iter().copied();
    while let Some(byte) = bytes.next() {
        if byte != b'%' {
            out.push(byte);
            continue;
        }
        match bytes.next() {
            Some(b'%') => out.push(b'%'),
            Some(b'd' | b's') => match args.next() {
                Some(Arg::Int(value)) => out.extend_from_slice(value.to_string().as_bytes()),
                Some(Arg::Text(text)) => out.extend_from_slice(text),
                None => {}
            },
            Some(other) => out.extend_from_slice(&[b'%', other]),
            None => out.push(b'%'),
        }
    }
    out
}

impl WorldState {
    /// Set a critter's current HP in both the instance record and its sheet.
    pub fn set_hit_points(&mut self, critter: ObjectRef, hp: i32) {
        if let Some(object) = self.get_mut(critter) {
            if let Some(sheet) = &mut object.character {
                sheet.current_hp = hp;
            }
            if let ObjectState::Critter(state) = &mut object.raw.state {
                state.hp = hp;
            }
        }
    }
    /// §3.4 step 1 (safe rule): the drug's mapped addiction variable reads
    /// nonzero. Unlisted PIDs read "not addicted".
    pub fn addicted(&self, pid: i32) -> bool {
        drugs::addiction_variable(pid).is_some_and(|variable| self.campaign(variable) != 0)
    }
    fn campaign(&self, variable: usize) -> i32 {
        self.gvars
            .as_ref()
            .and_then(|gvars| gvars.get(variable))
            .copied()
            .unwrap_or(0)
    }
    fn set_campaign(&mut self, variable: usize, value: i32) {
        if let Some(slot) = self
            .gvars
            .as_mut()
            .and_then(|gvars| gvars.get_mut(variable))
        {
            *slot = value;
        }
    }
    /// The player's drug sensitivity; NPCs receive no trait or perk scaling.
    fn sensitivity(&self, critter: ObjectRef) -> Sensitivity {
        match self.player_sheet().filter(|_| critter == self.player) {
            Some(sheet) => Sensitivity {
                chem_reliant: sheet.has_trait(Trait::ChemReliant),
                chem_resistant: sheet.has_trait(Trait::ChemResistant),
                flower_child: sheet.rank(Perk::FlowerChild) > 0,
            },
            None => Sensitivity::default(),
        }
    }
    /// The effective value a stage slot changes: a bonus-array stat, current
    /// HP, or the poison/radiation level.
    fn drug_stat(&self, critter: ObjectRef, selector: i32) -> i32 {
        let Some(sheet) = self
            .get(critter)
            .and_then(|object| object.character.as_ref())
        else {
            return 0;
        };
        match selector {
            drugs::CURRENT_HP => self.hit_points(critter).unwrap_or(sheet.current_hp),
            drugs::POISON => sheet.poison,
            drugs::RADIATION => sheet.radiation,
            stat => sheet.stat(stat as usize, self.context(critter)),
        }
    }
    fn drug_payload(&self, drug: ObjectRef) -> Option<(i32, Drug)> {
        let object = self.get(drug)?;
        match &self.item_data(drug)?.data {
            ItemData::Drug(payload) => Some((object.raw.pid as i32, payload.clone())),
            _ => None,
        }
    }
}

impl<A: WorldAssets> WorldHost<A> {
    pub(crate) fn game_text(&self, file: &str, id: i32) -> Option<Vec<u8>> {
        let catalog = self.assets.game_messages(file).ok()?;
        catalog.get(id).map(|message| message.text.clone())
    }
    /// Print a game message with its conversions substituted.
    pub(crate) fn print_game(&mut self, file: &str, id: i32, args: &[Arg]) {
        if let Some(text) = self.game_text(file, id) {
            self.world
                .emit(WorldEffect::Message(format_message(&text, args)));
        }
    }
    /// SPEC-011 §3.1 consumption of drug item `drug` (already out of the
    /// consumer's inventory) by `critter`: -1 for a dead or robotic
    /// consumer with nothing changed and no draw; otherwise the re-dose
    /// rule, the immediate stage, the delayed stages, the addiction check,
    /// and 1 (even when the immediate stage killed the consumer). The
    /// caller decides whether to destroy the item.
    pub fn consume_drug(&mut self, critter: ObjectRef, drug: ObjectRef) -> Result<i32, WorldError> {
        if self.world.is_dead(critter) || self.world.body_type(critter) == 2 {
            return Ok(-1);
        }
        let (pid, payload) = self
            .world
            .drug_payload(drug)
            .ok_or_else(|| WorldError::Invalid("consumed item is not a drug".into()))?;
        self.redose(critter, pid, payload.onset)?;
        self.apply_stage(critter, &Stage::immediate(&payload), true)?;
        let sensitivity = self.world.sensitivity(critter);
        for (delay, stage) in Stage::delayed(&payload) {
            if stage.scheduled() {
                self.world.queue.insert(
                    self.world.clock.ticks,
                    sensitivity.stage_delay(delay),
                    critter,
                    EventKind::Drug {
                        stats: stage.selectors,
                        modifiers: stage.amounts,
                    },
                )?;
            }
        }
        self.addiction_check(critter, pid, &payload)?;
        Ok(1)
    }
    /// §3.5: the first queued withdrawal event of `critter` whose drug maps
    /// to the same addiction variable is replaced by a new onset delayed by
    /// this drug's onset; an end event first removes its perk effect.
    fn redose(&mut self, critter: ObjectRef, pid: i32, onset: i32) -> Result<(), WorldError> {
        let variable = drugs::addiction_variable(pid);
        let matched = self
            .world
            .queue
            .events()
            .find_map(|event| match event.kind {
                EventKind::Withdrawal(payload)
                    if event.object == critter
                        && drugs::addiction_variable(payload[1]) == variable =>
                {
                    Some((event.id, payload))
                }
                _ => None,
            });
        let Some((id, [kind, drug, perk])) = matched else {
            return Ok(());
        };
        if kind == drugs::END {
            self.world.perk_effect(critter, perk, false);
            if critter == self.world.player {
                self.print_game("item.msg", 3, &[]);
            }
        }
        self.world.queue.remove(id);
        self.world.queue.insert(
            self.world.clock.ticks,
            drugs::TICKS_PER_MINUTE * i64::from(onset),
            critter,
            EventKind::Withdrawal([drugs::ONSET, drug, perk]),
        )?;
        Ok(())
    }
    /// §3.2 stage application. Bonus stats change on the prototype for an
    /// NPC; current HP first clears the fleeing bit, is capped at maximum
    /// HP, and kills at or below zero; poison and radiation act only on
    /// the player. The player sees each gained/lost change, or "nothing
    /// happens" for an immediate stage that changed nothing. No overdose
    /// text is shown: death presentation comes from the kill path.
    pub(crate) fn apply_stage(
        &mut self,
        critter: ObjectRef,
        stage: &Stage,
        immediate: bool,
    ) -> Result<(), WorldError> {
        let player = critter == self.world.player;
        let slots = stage.slots(&mut self.world.rng)?;
        let mut shown = false;
        for (selector, amount) in slots {
            let before = self.world.drug_stat(critter, selector);
            match selector {
                drugs::CURRENT_HP => {
                    if let Some(state) = self.world.critter_state_mut(critter) {
                        state.maneuvers &= !intent::FLEE;
                    }
                    let dead = self.world.is_dead(critter);
                    let maximum = self.world.effective(critter, 7);
                    let hp = (i64::from(self.world.hit_points(critter).unwrap_or(0))
                        + i64::from(amount))
                    .min(i64::from(maximum))
                    .max(i64::from(i32::MIN)) as i32;
                    self.world.set_hit_points(critter, hp);
                    if player {
                        self.world.emit(WorldEffect::RollHitPoints);
                    }
                    if hp <= 0 && !dead {
                        self.kill_critter(critter, 20)?;
                    }
                }
                // SPEC-013 §3.19-3.20: the player-only adjustments.
                drugs::POISON => {
                    self.adjust_poison(critter, amount)?;
                }
                drugs::RADIATION => {
                    self.adjust_radiation(critter, amount);
                }
                stat => self.world.adjust_bonus(critter, stat as usize, amount),
            }
            let after = self.world.drug_stat(critter, selector);
            if player && after != before {
                shown = true;
                let name = self
                    .game_text("stat.msg", 100 + selector)
                    .unwrap_or_default();
                let difference = (i64::from(after) - i64::from(before)).abs();
                self.print_game(
                    "item.msg",
                    if after > before { 1 } else { 2 },
                    &[Arg::Int(difference), Arg::Text(&name)],
                );
            }
        }
        if immediate && player && !shown && self.world.hit_points(critter).unwrap_or(0) > 0 {
            self.print_game("item.msg", 10, &[]);
        }
        Ok(())
    }
    /// §3.4: unless the drug's addiction variable already reads "addicted",
    /// one `roll(1, 100)` (even at chance 0) against the chance (scaled for
    /// the player) queues a withdrawal onset; a player addiction also sets
    /// the mapped variable and the addicted flag.
    fn addiction_check(
        &mut self,
        critter: ObjectRef,
        pid: i32,
        drug: &Drug,
    ) -> Result<(), WorldError> {
        if self.world.addicted(pid) {
            return Ok(());
        }
        let chance = self
            .world
            .sensitivity(critter)
            .addiction_chance(drug.addiction);
        if self.world.rng.range(1, 100)? > chance {
            return Ok(());
        }
        self.world.queue.insert(
            self.world.clock.ticks,
            drugs::TICKS_PER_MINUTE * i64::from(drug.onset),
            critter,
            EventKind::Withdrawal([drugs::ONSET, pid, drug.withdrawal]),
        )?;
        if critter == self.world.player {
            if let Some(variable) = drugs::addiction_variable(pid) {
                self.world.set_campaign(variable, 1);
            }
            if let Some(sheet) = self.world.player_sheet_mut() {
                sheet.flags |= ADDICTED_FLAG;
            }
        }
        Ok(())
    }
    /// §3.3 queue handler for a delayed stage. It stops the queue pass only
    /// when the player owns it and is dead afterwards.
    pub(crate) fn drug_event(
        &mut self,
        owner: ObjectRef,
        stats: [i32; 3],
        modifiers: [i32; 3],
    ) -> Result<i32, WorldError> {
        if self.world.get(owner).is_none() {
            return Ok(0);
        }
        let stage = Stage {
            selectors: stats,
            amounts: modifiers,
        };
        self.apply_stage(owner, &stage, false)?;
        Ok(i32::from(
            owner == self.world.player && self.world.is_dead(owner),
        ))
    }
    /// §3.8 queue handler for a withdrawal onset or end. Every event the
    /// player owns stops the queue pass (interrupting travel or rest).
    pub(crate) fn withdrawal_event(
        &mut self,
        owner: ObjectRef,
        [kind, pid, perk]: [i32; 3],
    ) -> Result<i32, WorldError> {
        if self.world.get(owner).is_none() {
            return Ok(0);
        }
        let player = owner == self.world.player;
        match kind {
            drugs::ONSET => {
                self.world.perk_effect(owner, perk, true);
                if player {
                    self.print_game("perk.msg", 201 + perk, &[]);
                }
                let duration = self.world.sensitivity(owner).withdrawal_ticks();
                self.world.queue.insert(
                    self.world.clock.ticks,
                    duration,
                    owner,
                    EventKind::Withdrawal([drugs::END, pid, perk]),
                )?;
            }
            drugs::END => {
                self.world.perk_effect(owner, perk, false);
                if player {
                    self.print_game("item.msg", 3, &[]);
                    if let Some(variable) = drugs::addiction_variable(pid) {
                        self.world.set_campaign(variable, 0);
                    }
                    // Safe rule: the "any addiction" test reads 189-194.
                    let any = drugs::addiction_variables().any(|v| self.world.campaign(v) != 0);
                    if !any && let Some(sheet) = self.world.player_sheet_mut() {
                        sheet.flags &= !ADDICTED_FLAG;
                    }
                }
            }
            other => {
                self.world
                    .diagnostic(format!("withdrawal event kind {other} is undefined"));
                return Ok(0);
            }
        }
        Ok(i32::from(player))
    }
    /// §3.9 exit handlers for the drug and withdrawal events of non-party
    /// owners: a stage applies at once (non-immediate presentation), a
    /// withdrawal end removes its perk effect, and an onset is dropped.
    pub(crate) fn leave_map_events(&mut self, events: Vec<TimedEvent>) -> Result<(), WorldError> {
        for event in events {
            match event.kind {
                EventKind::Drug { stats, modifiers } if self.world.get(event.object).is_some() => {
                    let stage = Stage {
                        selectors: stats,
                        amounts: modifiers,
                    };
                    self.apply_stage(event.object, &stage, false)?;
                }
                EventKind::Withdrawal([drugs::END, _, perk]) => {
                    self.world.perk_effect(event.object, perk, false);
                }
                _ => {}
            }
        }
        Ok(())
    }
    /// Consume one unit of a drug, destroying it on result 1 and putting it
    /// back otherwise.
    fn consume_unit(&mut self, consumer: ObjectRef, drug: ObjectRef) -> Result<i32, WorldError> {
        let owner = self.world.get(drug).and_then(|object| object.owner);
        if let Some(owner) = owner {
            self.world.remove_item(owner, drug, 1)?;
        }
        let result = self.consume_drug(consumer, drug);
        match (&result, owner) {
            (Ok(1), _) => self.world.erase(drug),
            (_, Some(owner)) => self.world.add_item(owner, drug, 1, true)?,
            _ => {}
        }
        result
    }
    /// §3.6 inventory "use" of a drug on oneself. Safe rule: the unit is
    /// destroyed only on result 1 (a -1 consumer keeps it).
    pub fn use_drug(&mut self, critter: ObjectRef, drug: ObjectRef) -> Result<i32, WorldError> {
        self.consume_unit(critter, drug)
    }
    /// §3.6 default use of a drug on a target (after the target's script
    /// did not override it): a non-critter "does nothing", a dead critter
    /// draws one of four messages, and a living critter consumes one unit,
    /// destroyed on exactly 1. The player using it on another critter sees
    /// the item and target names first.
    pub(crate) fn use_drug_on(
        &mut self,
        actor: ObjectRef,
        drug: ObjectRef,
        target: ObjectRef,
    ) -> Result<i32, WorldError> {
        let player = actor == self.world.player;
        let critter = self
            .world
            .get(target)
            .is_some_and(|object| object.category() == Category::Critter);
        if !critter {
            if player {
                self.print_game("proto.msg", 582, &[]);
            }
            return Ok(-1);
        }
        if self.world.is_dead(target) {
            let message = 583 + self.world.rng.range(0, 3)?;
            if player {
                self.print_game("proto.msg", message, &[]);
            }
            return Ok(-1);
        }
        if player && target != actor {
            let name = |reference| {
                self.world
                    .get(reference)
                    .map_or(Vec::new(), |object| object.name.clone())
            };
            let (item, whom) = (name(drug), name(target));
            self.print_game("proto.msg", 581, &[Arg::Text(&item), Arg::Text(&whom)]);
        }
        self.consume_unit(target, drug)
    }
}
