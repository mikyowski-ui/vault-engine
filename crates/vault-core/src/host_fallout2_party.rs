//! SPEC-019 §3.6 and §3.14-§3.18 in the host, under the Fallout 2 profile:
//! the control records of party members and the panels' actions on them,
//! the world-dependent parts of the SPEC-009 deltas (hostile candidates,
//! the burst gate's friendlies, the snipe retreat), party-member level-ups,
//! dead and hidden members on map changes, and the Fallout 2 Push.
//!
//! Nothing here runs without a Fallout 2 game ([`crate::WorldState::fallout2`]),
//! and the AI hooks apply only to a party member other than the player
//! with a control record ([`WorldHost::control_of`]).
use crate::{
    ObjectRef, WorldEffect, WorldError,
    ai::{self, Taunt, intent},
    assets::WorldAssets,
    critical::{effect, hit_mode},
    fallout2_party::{self as party, Candidate, Exposure, distance},
    host::WorldHost,
    routing::rotation,
};
use vault_formats::{
    Fid, HexGrid,
    fallout2::{
        party::{ControlField, PartyMember},
        save::MemberRecord,
    },
    pro::{BasicFlags, Category, PrototypeData},
};

/// §2.3 E-push-blocked, with the critter's name for `%s`.
pub const PUSH_BLOCKED: &str = "There is no room to push %s.";
/// §2.3 E-level, with the member's name for `%s`.
pub const LEVEL_GAINED: &str = "%s has gained a level.";

/// What the Fallout 2 Push did (§3.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Push2 {
    /// The player approaches first; the push runs on arrival.
    Approaching,
    /// `push_p_proc` overrode the push.
    Overridden,
    /// The member walks to this hex.
    Moved(i32),
    /// No free neighbor: the monitor printed E-push-blocked.
    Blocked,
}

impl<A: WorldAssets> WorldHost<A> {
    /// §2.5 the control record of `critter`: a party member other than the
    /// player, in a Fallout 2 game, with a `party.txt` record.
    pub fn control_of(&self, critter: ObjectRef) -> Option<MemberRecord> {
        let state = self.world.fallout2.as_deref()?;
        if critter == self.world.player || !self.world.party.contains(&critter) {
            return None;
        }
        let id = self.world.get(critter)?.raw.id;
        state
            .control
            .get(&id)
            .copied()
            .filter(|record| record.record != -1)
    }
    /// The `party.txt` record behind `critter`'s control record.
    pub fn party_record_of(&self, critter: ObjectRef) -> Option<PartyMember> {
        let record = self.control_of(critter)?;
        self.world
            .fallout2
            .as_deref()?
            .tables
            .party
            .get(record.record)
            .cloned()
    }
    fn store_control(&mut self, critter: ObjectRef, record: MemberRecord) {
        let Some(id) = self.world.get(critter).map(|object| object.raw.id) else {
            return;
        };
        if let Some(state) = self.world.fallout2.as_deref_mut() {
            state.control.insert(id, record);
        }
    }
    /// §3.15.2 a disposition row; `false` when the row is disabled.
    pub fn choose_disposition(&mut self, critter: ObjectRef, disposition: i32) -> bool {
        let (Some(mut record), Some(member)) =
            (self.control_of(critter), self.party_record_of(critter))
        else {
            return false;
        };
        let chosen = party::choose_disposition(&mut record, &member, disposition);
        if chosen {
            self.store_control(critter, record);
        }
        chosen
    }
    /// §3.15.3 one press of a custom-panel row's button.
    pub fn cycle_control(&mut self, critter: ObjectRef, field: ControlField) -> bool {
        let (Some(mut record), Some(member)) =
            (self.control_of(critter), self.party_record_of(critter))
        else {
            return false;
        };
        let changed = party::cycle(&mut record, &member, field);
        if changed {
            self.store_control(critter, record);
        }
        changed
    }
    /// §3.15.2 Use Best Weapon: SPEC-009 §3.9's inventory search without
    /// the AP check, ranked by `best_weapon`. A weapon other than the
    /// right-hand item is wielded with its animation and no AP cost.
    /// Returns whether one was wielded (else the caller plays `iisxxxx1`).
    pub fn use_best_weapon(&mut self, critter: ObjectRef) -> Result<bool, WorldError> {
        if self.control_of(critter).is_none() {
            return Ok(false);
        }
        let Some(weapon) = self.inventory_search(critter, false) else {
            return Ok(false);
        };
        if self.world.wielded(critter, hit_mode::RIGHT_PRIMARY) == Some(weapon) {
            return Ok(false);
        }
        let ap = self.world.action_points(critter);
        self.wield_object(critter, weapon)?;
        if let Some(state) = self.world.critter_state_mut(critter) {
            state.ap = ap;
        }
        Ok(true)
    }

    /// §3.16 rule 1 whether an NPC turn starts by fleeing, under the
    /// member's `run_away_mode` (`None`: SPEC-009's test applies).
    pub(crate) fn party_forced_flee(
        &self,
        critter: ObjectRef,
        packet: &vault_formats::ai::Packet,
    ) -> Option<bool> {
        let record = self.control_of(critter)?;
        let state = self.world.critter_state(critter)?;
        let test = party::flee_test(
            party::value(&record, ControlField::RunAwayMode),
            self.world.effective(critter, 7),
            packet.min_hp,
        );
        Some(
            state.maneuvers & intent::FLEE != 0
                || (test.hurt_too_much && state.results.0 & packet.hurt_too_much != 0)
                || test.hp_below.is_some_and(|below| state.hp < below),
        )
    }
    /// §3.16 rule 3 the target `attack_who` picks with no requested target,
    /// or `None` for `danger(self)`.
    pub(crate) fn party_target(&mut self, critter: ObjectRef) -> Option<ObjectRef> {
        let record = self.control_of(critter)?;
        let mode = party::value(&record, ControlField::AttackWho);
        if mode == party::attack_who::WHOMEVER {
            return None;
        }
        let mut snapshot = std::mem::take(&mut self.world.ai.snapshot);
        snapshot.sort_by_key(|entry| self.world.object_distance(critter, *entry));
        let candidates: Vec<ObjectRef> = snapshot
            .iter()
            .copied()
            .filter(|entry| self.hostile_candidate(critter, *entry))
            .collect();
        self.world.ai.snapshot = snapshot;
        let player = self.world.player;
        let rows: Vec<Candidate> = candidates
            .iter()
            .map(|entry| Candidate {
                attacked_by_player: self.last_attacker(*entry) == Some(player),
                rating: self.world.retaliation_rating(*entry),
                hp: self.world.hit_points(*entry).unwrap_or(0),
            })
            .collect();
        party::choose_target(mode, &rows).map(|index| candidates[index])
    }
    fn last_attacker(&self, critter: ObjectRef) -> Option<ObjectRef> {
        self.world
            .get(critter)
            .map(|object| object.opponent)
            .filter(|reference| *reference != 0 && self.world.get(*reference).is_some())
    }
    /// §3.16 rule 3's hostile candidate test for snapshot entry `entry`.
    fn hostile_candidate(&self, critter: ObjectRef, entry: ObjectRef) -> bool {
        let world = &self.world;
        let player = world.player;
        if entry == critter
            || entry == player
            || world.party.contains(&entry)
            || world.is_dead(entry)
            || !world.can_perceive(critter, entry)
        {
            return false;
        }
        let ours = |other: Option<ObjectRef>| {
            other.is_some_and(|other| other == player || world.party.contains(&other))
        };
        let attacked_us = std::iter::once(player)
            .chain(world.party.iter().copied())
            .any(|member| self.last_attacker(member) == Some(entry));
        let team = |object| world.critter_state(object).map_or(0, |state| state.team);
        let alert = world
            .critter_state(entry)
            .is_some_and(|state| state.maneuvers & intent::ALERT != 0);
        ours(self.last_attacker(entry)) || attacked_us || (alert && team(entry) != team(player))
    }
    /// §3.16 rule 4: under `charge` the max-distance stop is skipped.
    pub(crate) fn party_charges(&self, critter: ObjectRef) -> bool {
        self.control_of(critter)
            .is_some_and(|record| party::value(&record, ControlField::Distance) == distance::CHARGE)
    }
    /// §3.16 rule 5 the follow threshold, or `None` without a record
    /// (SPEC-009's 5); `Some(None)` is `stay`, which never follows.
    pub(crate) fn party_follow(&self, critter: ObjectRef) -> Option<Option<i32>> {
        let record = self.control_of(critter)?;
        Some(party::follow_threshold(party::value(
            &record,
            ControlField::Distance,
        )))
    }
    /// The member's `distance` field, when it has a control record.
    pub(crate) fn party_distance(&self, critter: ObjectRef) -> Option<i32> {
        self.control_of(critter)
            .map(|record| party::value(&record, ControlField::Distance))
    }
    /// §3.16 rule 8 the weapon-preference rule for `critter`: SPEC-009's,
    /// or the ranking of its `best_weapon`.
    pub(crate) fn party_best_weapon(&self, critter: ObjectRef) -> Option<i32> {
        self.control_of(critter)
            .map(|record| party::value(&record, ControlField::BestWeapon))
    }
    /// §3.16 rule 2 the healing step: `None` without a record (SPEC-009),
    /// `Some(None)` for `clean`.
    pub(crate) fn party_healing(&self, critter: ObjectRef) -> Option<Option<party::Healing>> {
        let record = self.control_of(critter)?;
        Some(party::healing(
            party::value(&record, ControlField::ChemUse),
            self.world.effective(critter, 7),
        ))
    }
    /// §3.16 rule 9 the friendlies' exposure to a burst from `critter` at
    /// `target`: the player and the visible living party critters on the
    /// shooter's elevation, other than both.
    pub(crate) fn burst_exposure(&self, critter: ObjectRef, target: ObjectRef) -> Exposure {
        let world = &self.world;
        let (Some(from), Some(to), elevation) = (
            world.get(critter).and_then(|object| object.raw.hex),
            world.get(target).and_then(|object| object.raw.hex),
            world.get(critter).map_or(0, |object| object.raw.elevation),
        ) else {
            return Exposure::default();
        };
        let between = ray_between(from, to, elevation);
        let mut exposure = Exposure::default();
        for friendly in std::iter::once(world.player).chain(world.party.iter().copied()) {
            if friendly == critter || friendly == target || world.is_dead(friendly) {
                continue;
            }
            let Some(object) = world.get(friendly) else {
                continue;
            };
            if object.category() != Category::Critter
                || object.raw.flags.contains(BasicFlags::HIDDEN)
                || object.raw.elevation != elevation
            {
                continue;
            }
            let Some(hex) = object.raw.hex else {
                continue;
            };
            let from_target = HexGrid::lattice_distance(hex, to).unwrap_or(i32::MAX);
            exposure.nearest = Some(exposure.nearest.map_or(from_target, |n| n.min(from_target)));
            let near_ray = between.iter().any(|visited| {
                *visited == hex || HexGrid::lattice_distance(*visited, hex) == Ok(1)
            });
            if from_target <= 1 || near_ray {
                exposure.at_risk += 1;
            }
        }
        exposure
    }
    /// §3.16 rule 9 after §3.10 selected `mode` against `target`: the mode
    /// the burst gate leaves, and whether the next attack step fails
    /// instead. Without a record, or for a mode that is not burst or
    /// continuous, nothing changes.
    pub(crate) fn party_burst_gate(
        &self,
        critter: ObjectRef,
        target: ObjectRef,
        weapon: Option<ObjectRef>,
        mode: u8,
    ) -> (u8, bool) {
        let (Some(record), Some(weapon)) = (self.control_of(critter), weapon) else {
            return (mode, false);
        };
        if !party::area_nibble(self.world.mode_nibble(weapon, mode)) {
            return (mode, false);
        }
        let gate = party::value(&record, ControlField::AreaAttackMode);
        if party::burst_allowed(gate, self.burst_exposure(critter, target)) {
            return (mode, false);
        }
        let primary = party::area_nibble(self.world.mode_nibble(weapon, hit_mode::RIGHT_PRIMARY));
        if mode == hit_mode::RIGHT_SECONDARY && !primary {
            (hit_mode::RIGHT_PRIMARY, false)
        } else {
            (mode, true)
        }
    }
    /// §3.16 rule 7 the snipe retreat before the attack loop: with a ranged
    /// right-hand weapon and `target` within the snipe range, the member
    /// steps away from `target` by SPEC-009 §3.8 steps 2-4, with N from
    /// `min(A, 3)` down to 1, without the fleeing bit or a taunt.
    pub(crate) fn party_snipe_retreat(
        &mut self,
        critter: ObjectRef,
        target: ObjectRef,
    ) -> Result<(), WorldError> {
        if self.party_distance(critter) != Some(distance::SNIPE) {
            return Ok(());
        }
        let Some(weapon) = self.world.wielded(critter, hit_mode::RIGHT_PRIMARY) else {
            return Ok(());
        };
        let nibble = self.world.mode_nibble(weapon, hit_mode::RIGHT_PRIMARY);
        if ai::subtype_of(nibble).0 != ai::subtype::RANGED
            || self.world.object_distance(critter, target) > party::SNIPE_RANGE
        {
            return Ok(());
        }
        let cost = self
            .world
            .attack_profile(
                critter,
                target,
                hit_mode::RIGHT_PRIMARY,
                crate::critical::location::UNCALLED,
                false,
            )
            .map_or(i32::MAX, |attack| attack.ap_cost());
        let spare = self.world.action_points(critter).saturating_sub(cost);
        if spare <= 0 {
            return Ok(());
        }
        let Some(away_from) = self.world.get(target).and_then(|object| object.raw.hex) else {
            return Ok(());
        };
        self.step_away(critter, away_from, spare.min(3), false)
    }
    /// SPEC-009 §3.8 steps 2-4 from `origin`: the first candidate D, D + 1,
    /// D + 5 at N = `steps` down to 1 whose route exists, walked (or run,
    /// for a flight) within the current AP.
    pub(crate) fn step_away(
        &mut self,
        critter: ObjectRef,
        origin: i32,
        steps: i32,
        flight: bool,
    ) -> Result<(), WorldError> {
        let Some(from) = self.world.get(critter).and_then(|object| object.raw.hex) else {
            return Ok(());
        };
        let direction = rotation(origin, from).unwrap_or(0);
        let goal = ai::flee_candidates(direction, steps).find_map(|(steps, toward)| {
            let goal = HexGrid::step(from, toward, steps).ok()?;
            (!self.world.route(critter, from, goal, true).is_empty()).then_some(goal)
        });
        let Some(goal) = goal else {
            return Ok(());
        };
        self.world.ai.callbacks.clear();
        if flight {
            self.request_taunt(critter, Taunt::Run)?;
        }
        let budget = usize::try_from(self.world.action_points(critter)).unwrap_or(0);
        if let Some(movement) = self.plan_to_tile(critter, goal, budget) {
            self.run_movement(movement, budget, flight)?;
        }
        if flight {
            self.run_taunts();
        }
        Ok(())
    }

    /// §3.17 after the player gained `levels` levels: each single gain, in
    /// order, advances each eligible member at most one stage.
    pub(crate) fn party_level_ups(&mut self, levels: u32) {
        if levels == 0 || self.world.fallout2.is_none() {
            return;
        }
        let level = self.world.player_sheet().map_or(1, |sheet| sheet.level);
        for gained in 0..levels {
            let reached = level - (levels - 1 - gained) as i32;
            for member in self.world.party_members() {
                self.level_up_member(member, reached);
            }
        }
    }
    fn level_up_member(&mut self, member: ObjectRef, level: i32) {
        let (Some(mut record), Some(table)) =
            (self.control_of(member), self.party_record_of(member))
        else {
            return;
        };
        let Some(object) = self.world.get(member) else {
            return;
        };
        if object.category() != Category::Critter
            || object.raw.flags.contains(BasicFlags::HIDDEN)
            || self.world.is_dead(member)
            || !party::advances(&table, record.stage, level)
        {
            return;
        }
        let pid = table.level_pids[record.stage as usize];
        let prototype = match self.assets.prototype(pid) {
            Ok(prototype) if matches!(prototype.data, PrototypeData::Critter(_)) => prototype,
            _ => {
                self.world.diagnostic(format!(
                    "party level PID {pid:#010x} has no critter prototype; the member stays at stage {}",
                    record.stage
                ));
                return;
            }
        };
        self.world.adopt_prototype(member, prototype);
        record.stage += 1;
        self.store_control(member, record);
        let name = self
            .world
            .get(member)
            .map(|object| object.name.clone())
            .unwrap_or_default();
        self.world
            .emit(WorldEffect::Message(substitute(LEVEL_GAINED, &name)));
    }

    /// §3.14 leaving a map: every dead party critter leaves the list first
    /// and stays as a corpse; its control record is discarded.
    pub(crate) fn drop_dead_members(&mut self) {
        if self.world.fallout2.is_none() {
            return;
        }
        let dead: Vec<ObjectRef> = self
            .world
            .party
            .iter()
            .copied()
            .filter(|member| *member != self.world.player && self.world.is_dead(*member))
            .collect();
        for member in dead {
            self.world.party.remove(&member);
            let id = self.world.get(member).map(|object| object.raw.id);
            if let (Some(id), Some(state)) = (id, self.world.fallout2.as_deref_mut()) {
                state.control.remove(&id);
                state.retained.remove(&id);
            }
        }
    }
    /// §3.14 the visible entries: the party list without hidden objects,
    /// the player included. Internal only (no script word returns it).
    pub fn visible_party_count(&self) -> usize {
        self.world
            .party_members()
            .into_iter()
            .filter(|member| {
                self.world
                    .get(*member)
                    .is_some_and(|object| !object.raw.flags.contains(BasicFlags::HIDDEN))
            })
            .count()
    }

    /// §3.6 whether Fallout 2's Push is offered on `critter`: a party
    /// member other than the player, a critter neither dead nor knocked
    /// out, out of combat.
    pub fn fallout2_can_push(&self, critter: ObjectRef) -> bool {
        let world = &self.world;
        world.fallout2.is_some()
            && !world.combat
            && critter != world.player
            && world.party.contains(&critter)
            && world.get(critter).is_some_and(|object| {
                object.category() == Category::Critter && object.raw.hex.is_some()
            })
            && world
                .critter_state(critter)
                .is_some_and(|state| state.results.0 & (effect::DEAD | effect::KNOCKED_OUT) == 0)
    }
    /// §3.6 choosing Push: the talk approach (SPEC-014 §3.27), then the
    /// push on arrival ([`ObjectAction::Push`](crate::ObjectAction::Push)).
    pub fn fallout2_push(&mut self, critter: ObjectRef) -> Result<Option<Push2>, WorldError> {
        if !self.fallout2_can_push(critter) {
            return Ok(None);
        }
        let player = self.world.player;
        self.last_push = None;
        self.action(player, critter, crate::ObjectAction::Push)?;
        Ok(self.last_push.take().or(Some(Push2::Approaching)))
    }
    /// §3.6 steps 1-4, once the talk approach succeeded.
    pub(crate) fn perform_push(&mut self, critter: ObjectRef) -> Result<Push2, WorldError> {
        if self.request_push(critter)? {
            return Ok(Push2::Overridden);
        }
        let outcome = match self.push_step(critter)? {
            Some(hex) => Push2::Moved(hex),
            None => {
                let name = self
                    .world
                    .get(critter)
                    .map(|object| object.name.clone())
                    .unwrap_or_default();
                self.world
                    .emit(WorldEffect::Message(substitute(PUSH_BLOCKED, &name)));
                Push2::Blocked
            }
        };
        Ok(outcome)
    }
    /// The step rule shared by both pushes: the neighbor in direction D
    /// from the player's hex to the critter's, then D + 1, then D + 5; the
    /// first on the grid and not blocked is walked to. `None` when none is.
    pub(crate) fn push_step(&mut self, critter: ObjectRef) -> Result<Option<i32>, WorldError> {
        let world = &self.world;
        let (Some(from), Some(player)) = (
            world.get(critter).and_then(|object| object.raw.hex),
            world.get(world.player).and_then(|object| object.raw.hex),
        ) else {
            return Ok(None);
        };
        let elevation = world.get(critter).map_or(0, |object| object.raw.elevation);
        let away = rotation(player, from).unwrap_or(0);
        for direction in [away, (away + 1) % 6, (away + 5) % 6] {
            let Ok(Some(hex)) = HexGrid::neighbor(from, direction) else {
                continue;
            };
            if self
                .world
                .movement_blocker(hex, elevation, critter)
                .is_some()
            {
                continue;
            }
            if self.walk_to(critter, hex)? {
                return Ok(Some(hex));
            }
        }
        Ok(None)
    }
}

impl crate::WorldState {
    /// §3.17 advancing: the member takes `prototype` and its stats; current
    /// HP rises by the maximum's increase and is clamped to 1 through the
    /// new maximum; a different base art is taken with the facing and
    /// weapon code kept. Identity, script, inventory, team, AI packet,
    /// position and facing are unchanged.
    pub(crate) fn adopt_prototype(
        &mut self,
        member: ObjectRef,
        prototype: std::sync::Arc<vault_formats::pro::Prototype>,
    ) {
        let PrototypeData::Critter(critter) = &prototype.data else {
            return;
        };
        let old_max = self.effective(member, 7);
        let current = self.hit_points(member).unwrap_or(old_max);
        let mut sheet = self.critter_sheet(prototype.pid, critter);
        let new_max = sheet.stat(7, self.context(member));
        sheet.current_hp = party::advanced_hp(old_max, current, new_max);
        let Some(object) = self.get_mut(member) else {
            return;
        };
        let old = Fid(object.raw.art_id);
        let new = Fid(prototype.art_id);
        if new.base() != old.base() {
            object.raw.art_id = Fid::new(
                old.category(),
                new.base(),
                old.animation(),
                old.weapon(),
                old.selector(),
            )
            .0;
        }
        object.raw.pid = prototype.pid;
        if let vault_formats::map::ObjectState::Critter(state) = &mut object.raw.state {
            state.hp = sheet.current_hp;
        }
        object.character = Some(sheet);
        object.prototype = Some(prototype);
    }
}

/// The SPEC-014 `%s` rule for the engine strings: the first `%s` becomes
/// `name`.
pub fn substitute(template: &str, name: &[u8]) -> Vec<u8> {
    match template.split_once("%s") {
        Some((head, tail)) => [head.as_bytes(), name, tail.as_bytes()].concat(),
        None => template.as_bytes().to_vec(),
    }
}

/// The hexes the SPEC-007 §3.2 ray from `from` to `to` visits strictly
/// between them, in order, each once: the traversal's changed-hex queries,
/// with a blocker test that never stops it.
pub fn ray_between(from: i32, to: i32, elevation: u8) -> Vec<i32> {
    let mut hexes: Vec<i32> = Vec::new();
    crate::routing::cast_ray(
        from,
        to,
        elevation,
        crate::routing::RayOptions {
            stride: 32,
            check_blockers: true,
            ignore: None,
        },
        |hex| {
            if hex != from && hex != to && !hexes.contains(&hex) {
                hexes.push(hex);
            }
            None
        },
        |_| false,
    );
    hexes
}
