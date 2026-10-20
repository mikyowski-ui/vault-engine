//! SPEC-007 §3.3 combat initialization, participation, turns, and ending.
//! AI decisions come from [`CombatAi`]: the built-in SPEC-009 packet AI
//! ([`PacketAi`]), or a host-supplied policy.
use crate::{
    ObjectRef, ScriptEvent, WorldError, WorldState,
    assets::WorldAssets,
    critical::{effect, maneuver, state},
    host::WorldHost,
    stats::Perk,
    world::ScriptRequest,
};
use serde::{Deserialize, Serialize};
use vault_formats::{Fid, pro::BasicFlags};

/// Combat list and partitions: `list[..active]` take turns in order;
/// `list[active..]` are waiting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombatState {
    /// Active `0x01`, player-input gate `0x02`, accepted end request `0x08`.
    pub flags: u32,
    pub list: Vec<ObjectRef>,
    pub active: usize,
    /// The fixed combat elevation.
    pub elevation: u8,
    /// Completed rounds; the first pass uses the initiating order unsorted.
    pub round: u32,
    /// Index of the next turn within the active partition.
    pub cursor: usize,
    /// Whether the current turn is a resumed player turn (no AP refill).
    pub resumed: bool,
    /// Whether the player's turn is awaiting input.
    pub player_turn: bool,
    /// The player ended the current turn (end-turn / space).
    pub turn_ended: bool,
    /// Initial scripted combat-request AP bonus.
    pub ap_bonus: i32,
    /// The initiating scripted request, consumed by the attacker's first attack.
    pub request: Option<CombatRequest>,
}

/// A scripted combat request (SPEC-005 `attack`/`attack_setup`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombatRequest {
    pub attacker: ObjectRef,
    pub defender: ObjectRef,
    /// Initial accuracy adjustment for the requested attack.
    pub accuracy: i32,
    /// Defender damage bounds applied before sequencing.
    pub damage: [i32; 2],
    /// The equal override pair: attacker flags, then defender flags.
    pub flags: Option<[i32; 2]>,
}
impl Default for CombatState {
    /// Inactive, with the input-gate bit set and no participants.
    fn default() -> Self {
        Self {
            flags: state::INPUT_GATE,
            list: Vec::new(),
            active: 0,
            elevation: 0,
            round: 0,
            cursor: 0,
            resumed: false,
            player_turn: false,
            turn_ended: false,
            ap_bonus: 0,
            request: None,
        }
    }
}

/// NPC combat policy.
pub trait CombatAi {
    /// Whether this is the built-in SPEC-009 AI, which the host runs itself
    /// (the other methods are then not consulted).
    fn builtin(&self) -> bool {
        false
    }
    /// The critter's AI danger target, if any.
    fn danger_target(&mut self, _world: &WorldState, _critter: ObjectRef) -> Option<ObjectRef> {
        None
    }
    /// Run a non-player turn's actions (moves/attacks) through the host.
    fn take_turn(
        &mut self,
        _world: &mut WorldState,
        _critter: ObjectRef,
    ) -> Result<(), WorldError> {
        Ok(())
    }
}
/// No AI: no danger targets and no NPC actions.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoAi;
impl CombatAi for NoAi {}
/// The SPEC-009 packet-driven AI (the host default).
#[derive(Debug, Default, Clone, Copy)]
pub struct PacketAi;
impl CombatAi for PacketAi {
    fn builtin(&self) -> bool {
        true
    }
}

/// Result of advancing combat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatProgress {
    /// The player's turn awaits input.
    PlayerTurn,
    /// Combat ended (and was cleaned up).
    Ended,
}

impl WorldState {
    fn status(&self, critter: ObjectRef) -> u32 {
        self.critter_state(critter).map_or(0, |s| s.results.0)
    }
    /// "Active critter": not knocked out, dead, or losing its turn.
    pub fn combat_active(&self, critter: ObjectRef) -> bool {
        self.status(critter) & (effect::KNOCKED_OUT | effect::DEAD | effect::LOSE_TURN) == 0
    }
    /// The dead query also recognizes nonpositive HP.
    pub fn is_dead(&self, critter: ObjectRef) -> bool {
        self.status(critter) & effect::DEAD != 0
            || self.hit_points(critter).is_some_and(|hp| hp <= 0)
    }
    /// Prone: knockout/knockdown, or art animation 20-35 or 48-63.
    pub fn is_prone(&self, critter: ObjectRef) -> bool {
        let animation = self
            .get(critter)
            .map_or(0, |o| Fid(o.raw.art_id).animation());
        self.status(critter) & (effect::KNOCKED_OUT | effect::KNOCKED_DOWN) != 0
            || matches!(animation, 20..=35 | 48..=63)
    }
    fn maximum_ap(&self, critter: ObjectRef) -> i32 {
        let context = self.context(critter);
        self.get(critter)
            .and_then(|o| o.character.as_ref())
            .map_or(0, |sheet| sheet.stat(8, context))
    }
    fn set_ap(&mut self, critter: ObjectRef, ap: i32) {
        if let Some(state) = self.critter_state_mut(critter) {
            state.ap = ap;
        }
    }
    fn critter_team(&self, critter: ObjectRef) -> i32 {
        self.critter_state(critter).map_or(0, |s| s.team)
    }
    /// Relevant to an end request: a non-player on a different team, or one
    /// whose last opponent is on its own team.
    fn relevant(&self, critter: ObjectRef) -> bool {
        critter != self.player
            && (self.critter_team(critter) != self.critter_team(self.player)
                || self.get(critter).is_some_and(|o| {
                    o.opponent != 0 && self.critter_team(o.opponent) == self.critter_team(critter)
                }))
    }
}

impl<A: WorldAssets> WorldHost<A> {
    /// Service a pending scripted request: start a fresh combat, keeping the
    /// request for the attacker's first attack. During existing combat the
    /// opcode only marks the attacker's opponent, so nothing is pending.
    pub fn service_combat_request(&mut self) -> Result<bool, WorldError> {
        let Some(request) = self.world.pending_combat.take() else {
            return Ok(false);
        };
        if self.world.combat {
            return Ok(false);
        }
        // SPEC-007 §3.3 safe rule: begin only when every supplied (non-null)
        // participant is on the current elevation; the reference admits the
        // request when either one is.
        let elevation = self
            .world
            .get(self.world.player)
            .map_or(self.world.elevation, |o| o.raw.elevation);
        let here = |participant: ObjectRef| {
            participant == 0
                || self
                    .world
                    .get(participant)
                    .is_some_and(|object| object.raw.elevation == elevation)
        };
        if !(here(request.attacker) && here(request.defender)) {
            self.world
                .diagnostic("combat request participant is not on the current elevation");
            return Ok(false);
        }
        self.start_combat(request.attacker, request.defender)?;
        self.world.combat_state.request = Some(request);
        // SPEC-009 §3.6: the first turn's requested target and accuracy.
        self.world.ai.requested = Some((request.defender, request.accuracy));
        Ok(true)
    }
    /// The requested attack's adjustments: accuracy before resolution, then
    /// damage clamped to the request bounds and the override pair written to
    /// the attacker and defender flags. Knockback and death flags already
    /// computed are not rerun.
    pub fn apply_request(
        &mut self,
        attacker: ObjectRef,
        defender: ObjectRef,
        attack: &mut crate::combat::Attack,
    ) -> Option<CombatRequest> {
        let request = self
            .world
            .combat_state
            .request
            .filter(|request| request.attacker == attacker && request.defender == defender)?;
        self.world.combat_state.request = None;
        attack.accuracy_bonus = request.accuracy;
        Some(request)
    }
    /// Fresh combat: fix the elevation, collect visible critters there (in
    /// ascending hex, then per-hex object order) into the waiting partition
    /// with combat identifiers, clear accumulated damage/opponents/AP
    /// (keeping only the alert maneuver), copy the list into the AI
    /// snapshot, then move the initiating attacker, defender, and player (if
    /// neither) into the active order used by the first, unsorted pass.
    pub fn start_combat(
        &mut self,
        attacker: ObjectRef,
        defender: ObjectRef,
    ) -> Result<(), WorldError> {
        // Starting a fresh combat stops registered animations.
        self.stop_motions();
        self.building_animation = None;
        let elevation = self
            .world
            .get(self.world.player)
            .map_or(self.world.elevation, |o| o.raw.elevation);
        let mut list = Vec::new();
        let candidates: Vec<_> = self.world.iter().map(|(reference, _)| reference).collect();
        for critter in candidates {
            let Some(object) = self.world.get(critter) else {
                continue;
            };
            if object.owner.is_none()
                && object.raw.hex.is_some()
                && object.raw.elevation == elevation
                && Fid(object.raw.art_id).category() == 1
                && object.character.is_some()
                && !object.raw.flags.contains(BasicFlags::HIDDEN)
            {
                list.push(critter);
            }
        }
        // A stable sort keeps world order within a hex.
        list.sort_by_key(|critter| self.world.get(*critter).and_then(|o| o.raw.hex));
        for (identifier, critter) in list.iter().enumerate() {
            if let Some(object) = self.world.get_mut(*critter) {
                object.raw.combat_id = identifier as i32;
                object.opponent = 0;
            }
            if let Some(state) = self.world.critter_state_mut(*critter) {
                state.damage = 0;
                state.ap = 0;
                state.maneuvers &= maneuver::ALERTED;
            }
        }
        self.world.ai.snapshot = list.clone();
        let mut combat = CombatState {
            flags: state::ACTIVE | state::INPUT_GATE,
            list,
            elevation,
            ..CombatState::default()
        };
        let mut order = vec![attacker, defender];
        if attacker != self.world.player && defender != self.world.player {
            order.push(self.world.player);
        }
        // A same-object initiating pair is listed once (the SPEC-007 §3.3
        // safe rule): the second search finds it already active.
        for participant in order {
            if let Some(position) = combat.list[combat.active..]
                .iter()
                .position(|critter| *critter == participant)
            {
                let at = combat.active + position;
                combat.list.swap(combat.active, at);
                combat.active += 1;
            }
        }
        // The initiating pair reference each other through the setter.
        self.set_last_attacker(attacker, defender);
        self.set_last_attacker(defender, attacker);
        self.world.combat_state = combat;
        self.world.combat = true;
        self.world.combat_quit = false;
        self.world.combat_experience = 0;
        Ok(())
    }
    fn combat_script(&mut self, critter: ObjectRef, parameter: i32) -> bool {
        let Some(script) = self.world.script_for(critter) else {
            return false;
        };
        let overridden = match self.dispatch(ScriptRequest {
            script,
            event: ScriptEvent::Combat,
            source: 0,
            target: 0,
            fixed: parameter,
            action: -1,
            only_present: true,
        }) {
            Ok(result) => result.overridden,
            Err(error) => {
                self.world.diagnostic(format!("combat script: {error}"));
                false
            }
        };
        // Clear the invocation's source/target.
        if let Some(instance) = self.world.scripts.get_mut(&script) {
            instance.source = 0;
            instance.target = 0;
        }
        overridden
    }
    /// Join test for a waiter: not hidden, on the player's elevation, not
    /// dead/knocked out; recent damage admits; otherwise its combat script
    /// runs with parameter 5: alert admits, stop/flee rejects, else an AI
    /// danger target must exist.
    fn wants_to_join(&mut self, critter: ObjectRef, ai: &mut dyn CombatAi, invoke: bool) -> bool {
        let player_elevation = self.world.get(self.world.player).map(|o| o.raw.elevation);
        let Some(object) = self.world.get(critter) else {
            return false;
        };
        if object.raw.flags.contains(BasicFlags::HIDDEN)
            || Some(object.raw.elevation) != player_elevation
            || self.world.status(critter) & (effect::DEAD | effect::KNOCKED_OUT) != 0
        {
            return false;
        }
        if self
            .world
            .critter_state(critter)
            .is_some_and(|s| s.damage > 0)
        {
            return true;
        }
        if invoke {
            self.combat_script(critter, 5);
        }
        let maneuvers = self.world.critter_state(critter).map_or(0, |s| s.maneuvers);
        if maneuvers & maneuver::ALERTED != 0 {
            true
        } else if maneuvers & (maneuver::STOP_ATTACKING | maneuver::FLEEING) != 0 {
            false
        } else {
            self.danger_target(critter, ai).is_some()
        }
    }
    fn danger_target(&mut self, critter: ObjectRef, ai: &mut dyn CombatAi) -> Option<ObjectRef> {
        if ai.builtin() {
            self.danger(critter)
        } else {
            ai.danger_target(&self.world, critter)
        }
    }
    /// The round sweep before every pass after the first.
    fn sweep(&mut self, ai: &mut dyn CombatAi) -> Result<(), WorldError> {
        let mut scan = self.world.combat_state.active;
        while scan < self.world.combat_state.list.len() {
            let critter = self.world.combat_state.list[scan];
            if self.wants_to_join(critter, ai, true) {
                let combat = &mut self.world.combat_state;
                combat.list.swap(scan, combat.active);
                combat.active += 1;
                // Admitted NPCs (not the player) take an immediate turn.
                if critter != self.world.player {
                    self.take_turn(critter, ai)?;
                }
            }
            // Swapping then incrementing can defer the displaced waiter.
            scan += 1;
        }
        // Dead participants leave both partitions.
        let dead: Vec<_> = self
            .world
            .combat_state
            .list
            .iter()
            .copied()
            .filter(|critter| self.world.is_dead(*critter))
            .collect();
        for critter in dead {
            let combat = &mut self.world.combat_state;
            if let Some(position) = combat.list.iter().position(|c| *c == critter) {
                if position < combat.active {
                    combat.active -= 1;
                }
                combat.list.remove(position);
            }
        }
        // Non-player knockout, or maneuver exactly 2, moves to waiting.
        let mut index = 0;
        while index < self.world.combat_state.active {
            let critter = self.world.combat_state.list[index];
            let maneuvers = self.world.critter_state(critter).map_or(0, |s| s.maneuvers);
            let knocked_out = self.world.status(critter) & effect::KNOCKED_OUT != 0;
            if (critter != self.world.player && knocked_out)
                || maneuvers == maneuver::STOP_ATTACKING
            {
                if let Some(state) = self.world.critter_state_mut(critter) {
                    state.maneuvers &= !maneuver::ALERTED;
                }
                let combat = &mut self.world.combat_state;
                let critter = combat.list.remove(index);
                combat.active -= 1;
                combat.list.insert(combat.active, critter);
            } else {
                index += 1;
            }
        }
        // Descending effective Sequence, then Luck (a stable sort here).
        let mut active: Vec<_> =
            self.world.combat_state.list[..self.world.combat_state.active].to_vec();
        active.sort_by_key(|critter| {
            let context = self.world.context(*critter);
            let sheet = self.world.get(*critter).and_then(|o| o.character.as_ref());
            std::cmp::Reverse((
                sheet.map_or(0, |s| s.stat(13, context)),
                sheet.map_or(0, |s| s.stat(6, context)),
            ))
        });
        let count = active.len();
        self.world.combat_state.list[..count].copy_from_slice(&active);
        self.world.clock.advance(50)?;
        Ok(())
    }
    /// Run one turn up to its action branch. A player turn stops for input.
    fn take_turn(&mut self, critter: ObjectRef, ai: &mut dyn CombatAi) -> Result<bool, WorldError> {
        self.world.combat_actor = critter;
        // Only the very first turn after a scripted request sees its target.
        let requested = self.world.ai.requested.take();
        let resumed = std::mem::take(&mut self.world.combat_state.resumed);
        if !self.world.combat_active(critter) {
            if let Some(state) = self.world.critter_state_mut(critter) {
                state.results.0 &= !effect::LOSE_TURN;
            }
            self.end_turn(critter);
            return Ok(false);
        }
        if !resumed {
            let ap = self.world.maximum_ap(critter) + self.world.combat_state.ap_bonus;
            self.world.set_ap(critter, ap);
        }
        if critter == self.world.player {
            self.world.free_movement = 2 * self.world.perk(Perk::BonusMove) as i32;
        }
        if self.combat_script(critter, 4) {
            // An override suppresses stand-up and input/AI action processing.
            self.end_turn(critter);
            return Ok(false);
        }
        if self.world.is_prone(critter) {
            let ap = self.world.critter_state(critter).map_or(0, |s| s.ap);
            self.world.set_ap(critter, if ap < 3 { 0 } else { ap - 3 });
            if let Some(state) = self.world.critter_state_mut(critter) {
                state.results.0 &= !effect::KNOCKED_DOWN;
            }
            // SPEC-007 §3.4: it gets up, 37 after a fall back, 36 otherwise.
            self.stand_up(critter);
        }
        if critter == self.world.player {
            self.world.combat_state.player_turn = true;
            // SPEC-014 §3.21: the AC counter rolls at the turn's start.
            self.world.emit(crate::WorldEffect::RollArmorClass);
            return Ok(true);
        }
        if ai.builtin() {
            let (target, accuracy) = requested.map_or((None, 0), |(t, a)| (Some(t), a));
            self.world.ai.accuracy = accuracy;
            let turn = self.ai_turn(critter, target);
            self.world.ai.accuracy = 0;
            turn?;
        } else {
            ai.take_turn(&mut self.world, critter)?;
        }
        self.end_turn(critter);
        Ok(false)
    }
    /// Turn end: damage received this turn resets and free movement clears.
    fn end_turn(&mut self, critter: ObjectRef) {
        if let Some(state) = self.world.critter_state_mut(critter) {
            state.damage = 0;
        }
        if critter == self.world.player {
            self.world.free_movement = 0;
            self.world.combat_state.player_turn = false;
            self.world.combat_state.turn_ended = false;
            self.world.emit(crate::WorldEffect::RollArmorClass);
        }
    }
    /// Whether the player's input loop must stop: input gate clear, accepted
    /// end request, knockout/dead/lose-turn, ordinary AP at most zero (free
    /// movement does not keep it open), or quit.
    pub fn player_turn_over(&self) -> bool {
        let player = self.world.player;
        let combat = &self.world.combat_state;
        combat.turn_ended
            || combat.flags & state::INPUT_GATE == 0
            || combat.flags & state::END_REQUESTED != 0
            || !self.world.combat_active(player)
            || self.world.critter_state(player).is_none_or(|s| s.ap <= 0)
            || self.world.combat_quit
    }
    /// Advance turns and rounds until the player must act or combat ends.
    pub fn advance_combat(&mut self, ai: &mut dyn CombatAi) -> Result<CombatProgress, WorldError> {
        for _ in 0..100_000 {
            if !self.world.combat {
                return Ok(CombatProgress::Ended);
            }
            if self.world.combat_state.player_turn {
                if !self.player_turn_over() {
                    return Ok(CombatProgress::PlayerTurn);
                }
                let player = self.world.player;
                self.end_turn(player);
                if self.world.combat_state.flags & state::END_REQUESTED != 0 {
                    // Player input consumes the accepted request and exits.
                    self.world.combat_state.flags &= !state::END_REQUESTED;
                    self.end_combat()?;
                    return Ok(CombatProgress::Ended);
                }
                self.world.combat_state.cursor += 1;
            }
            let player = self.world.player;
            if self.world.is_dead(player)
                || self.world.get(player).map(|o| o.raw.elevation)
                    != Some(self.world.combat_state.elevation)
            {
                self.end_combat()?;
                return Ok(CombatProgress::Ended);
            }
            let combat = &self.world.combat_state;
            if combat.cursor >= combat.active {
                // A complete round: automatic ending, else another round.
                if self.combat_should_end() {
                    self.end_combat()?;
                    return Ok(CombatProgress::Ended);
                }
                self.world.combat_state.cursor = 0;
                self.world.combat_state.round += 1;
                self.sweep(ai)?;
                continue;
            }
            let critter = combat.list[combat.cursor];
            if self.take_turn(critter, ai)? {
                continue;
            }
            self.world.combat_state.cursor += 1;
        }
        Err(WorldError::Invalid("combat turn limit exceeded".into()))
    }
    /// The player ends the current turn (end-turn / space).
    pub fn end_player_turn(&mut self) {
        if self.world.combat_state.player_turn {
            self.world.combat_state.turn_ended = true;
        }
    }
    /// Automatic end after a complete round: at most one active participant,
    /// the player absent, or every active actor on the player's team with no
    /// active actor's last opponent on that team.
    pub fn combat_should_end(&self) -> bool {
        let combat = &self.world.combat_state;
        let active = &combat.list[..combat.active];
        let team = self.world.critter_team(self.world.player);
        active.len() <= 1
            || !active.contains(&self.world.player)
            || active.iter().all(|critter| {
                self.world.critter_team(*critter) == team
                    && self.world.get(*critter).is_none_or(|o| {
                        o.opponent == 0 || self.world.critter_team(o.opponent) != team
                    })
            })
    }
    /// Stop preference: stop/flee maneuver, knockout/death, no AI danger
    /// target, danger outside perception, or AP still at maximum.
    fn wants_to_stop(&mut self, critter: ObjectRef, ai: &mut dyn CombatAi) -> bool {
        let maneuvers = self.world.critter_state(critter).map_or(0, |s| s.maneuvers);
        if maneuvers & (maneuver::STOP_ATTACKING | maneuver::FLEEING) != 0
            || self.world.status(critter) & (effect::KNOCKED_OUT | effect::DEAD) != 0
        {
            return true;
        }
        let unperceived = match self.danger_target(critter, ai) {
            None => true,
            Some(danger) => !self.world.can_perceive(critter, danger),
        };
        unperceived
            || self.world.critter_state(critter).map(|s| s.ap)
                == Some(self.world.maximum_ap(critter))
    }
    /// Explicit end-combat request: refused if, on the player's elevation, an
    /// active relevant opponent does not want to stop or a relevant waiter
    /// wants to join. Acceptance sets the request bit.
    pub fn request_end_combat(&mut self, ai: &mut dyn CombatAi) -> bool {
        let elevation = self.world.get(self.world.player).map(|o| o.raw.elevation);
        let combat = self.world.combat_state.clone();
        for (index, critter) in combat.list.iter().copied().enumerate() {
            if self.world.get(critter).map(|o| o.raw.elevation) != elevation
                || !self.world.relevant(critter)
            {
                continue;
            }
            let refuse = if index < combat.active {
                !self.wants_to_stop(critter, ai)
            } else {
                self.wants_to_join(critter, ai, false)
            };
            if refuse {
                return false;
            }
        }
        self.world.combat_state.flags |= state::END_REQUESTED;
        true
    }
    /// Cleanup: clear participants' damage/opponents/maneuvers/AP/outlines,
    /// restore the player's maximum AP, clear the active state while restoring
    /// the input gate, award kill experience to a living player unless quit,
    /// wake a knocked-out surviving player, and dispatch map updates.
    pub fn end_combat(&mut self) -> Result<(), WorldError> {
        let list = std::mem::take(&mut self.world.combat_state.list);
        for critter in &list {
            if let Some(object) = self.world.get_mut(*critter) {
                object.opponent = 0;
                object.raw.outline = 0;
            }
            if let Some(state) = self.world.critter_state_mut(*critter) {
                state.damage = 0;
                state.maneuvers = 0;
                state.ap = 0;
            }
        }
        // SPEC-009: the snapshot and pending AI state are discarded.
        self.world.ai.snapshot.clear();
        self.world.ai.callbacks.clear();
        self.world.ai.requested = None;
        let player = self.world.player;
        self.world.combat = false;
        self.world.combat_actor = 0;
        self.world.free_movement = 0;
        self.world.combat_state = CombatState::default();
        let maximum = self.world.maximum_ap(player);
        self.world.set_ap(player, maximum);
        let living = !self.world.is_dead(player);
        if living && !self.world.combat_quit {
            let experience = std::mem::take(&mut self.world.combat_experience);
            if experience > 0 {
                self.give_experience(experience);
            }
        }
        self.world.combat_experience = 0;
        // SPEC-014 §3.5 and SPEC-007 §3.11.5: normal cleanup has a prone
        // living player's wake-up event removed and gets it up at once, so
        // the game goes on; a script's ending of combat leaves a knockout in
        // place.
        let knocked_out = self
            .world
            .critter_state(player)
            .is_some_and(|state| state.results.0 & effect::KNOCKED_OUT != 0);
        if living && self.world.is_prone(player) && !(self.world.combat_quit && knocked_out) {
            if let Some(state) = self.world.critter_state_mut(player) {
                state.results.0 &= !(effect::KNOCKED_OUT | effect::KNOCKED_DOWN);
            }
            self.world.queue.retain(|event| {
                !(event.object == player && event.kind == crate::queue::EventKind::Knockout)
            });
            self.stand_up(player);
        }
        self.world.combat_quit = false;
        self.world.emit(crate::WorldEffect::RollArmorClass);
        self.map_update()
    }
}
