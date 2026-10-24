//! SPEC-008 §3.9 Pip-Boy rest: eligibility, interpolated time steps with
//! queue interruption, credited-minute healing, until-healed, and wake-up
//! presets.
use crate::{WorldError, assets::WorldAssets, critical::effect, host::WorldHost};
use vault_formats::{map::ObjectState, pro::Category};

/// Map numbers that force the team check, and those/towns that omit it.
pub const TEAM_CHECK_MAPS: [i32; 5] = [35, 39, 40, 45, 46];
pub const TEAM_CHECK_EXEMPT_TOWNS: [i32; 6] = [0, 2, 3, 6, 7, 10];
/// Credited minutes that trigger three hours of party resting healing.
pub const REST_CREDIT_HEAL: i32 = 180;

/// The visible alarm-menu choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestRequest {
    /// 10 or 30 minutes.
    Minutes(i32),
    /// 1-6 hours.
    Hours(i32),
    /// Until 06:00, 12:00, 18:00, or 00:00.
    UntilHour(i32),
    UntilHealed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestOutcome {
    Completed,
    /// Rest is not allowed here.
    Refused,
    /// A queue handler returned nonzero; the Pip-Boy is marked for exit.
    Interrupted(i32),
    /// The UI step callback stopped waiting (Escape/quit).
    Cancelled,
}

/// An in-progress rest, advanced one UI step at a time.
#[derive(Debug, Clone, PartialEq)]
pub struct RestSession {
    plan: Plan,
    wait: Option<Wait>,
    finished: Option<RestOutcome>,
}
impl RestSession {
    /// How the rest ended, once it has.
    pub fn outcome(&self) -> Option<RestOutcome> {
        self.finished
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
enum Plan {
    /// One explicit wait.
    Once,
    /// Until healed: estimated hours still to wait in chunks of 24.
    Chunks { estimate: i32 },
    /// Until healed: three-hour rests while the sampled deficit is nonzero.
    Repeat { sampled: i32 },
}
#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Minutes,
    Hours,
}
/// One explicit `H`-hour, `M`-minute wait.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Wait {
    hours: i32,
    minutes: i32,
    minute_steps: f64,
    hour_steps: f64,
    start: f64,
    phase: Phase,
    step: u64,
    pending_credit: Option<i32>,
}
impl Wait {
    /// `None` for a zero-length (immediately complete) wait.
    fn new<A: WorldAssets>(hours: i32, minutes: i32, host: &WorldHost<A>) -> Option<Self> {
        let (_, minute_steps, hour_steps) = rest_allocation(hours, minutes)?;
        Some(Self {
            hours,
            minutes,
            minute_steps,
            hour_steps,
            start: host.world.clock.ticks as f64,
            phase: Phase::Minutes,
            step: 0,
            pending_credit: None,
        })
    }
}

/// The recovered allocation for `T = 60 x H + M`: A seconds, then N
/// minute-phase and K hour-phase step counts before truncation.
pub fn rest_allocation(hours: i32, minutes: i32) -> Option<(f64, f64, f64)> {
    let total = 60 * i64::from(hours) + i64::from(minutes);
    if total <= 0 || hours < 0 || minutes < 0 {
        return None;
    }
    let total = total as f64;
    let seconds = 0.25 + 3.5 * total / 1440.0;
    let minute_steps = 20.0 * seconds * f64::from(minutes) / total;
    Some((seconds, minute_steps, 20.0 * seconds - minute_steps))
}

/// Minutes until the next requested whole hour; exactly at that hour and
/// minute zero waits a full day instead of zero.
pub fn minutes_until_hour(ticks: u64, hour: i32) -> i32 {
    let now = ((ticks / 36000 % 24) * 60 + ticks / 600 % 60) as i32;
    match (60 * hour.rem_euclid(24) - now).rem_euclid(1440) {
        0 => 1440,
        delta => delta,
    }
}

impl<A: WorldAssets> WorldHost<A> {
    /// Opening the Pip-Boy resets the credited-minute counter.
    pub fn open_pipboy(&mut self) {
        self.world.rest_credit = 0;
    }
    /// Living non-player critters on the current elevation block rest when
    /// the player was their last attacker or, in team-checking locations,
    /// when they belong to any other combat team.
    pub fn rest_allowed(&self) -> bool {
        let map = self.world.map.header.map_number;
        let elevation = self.world.elevation;
        // SPEC-008 §3.10: the town is the current map's fixed table town;
        // -1 (an unnumbered map) falls into the team-checking default.
        let town = self.world.rules.tables().map_town(map);
        let teams = TEAM_CHECK_MAPS.contains(&map)
            || !(map == 17
                || (map == 3 && elevation > 0)
                || TEAM_CHECK_EXEMPT_TOWNS.contains(&town));
        let player = self.world.player;
        let player_team = self.world.critter_state(player).map_or(0, |s| s.team);
        let player_combat_id = self.world.get(player).map_or(-1, |o| o.raw.combat_id);
        !self.world.iter().any(|(reference, object)| {
            let ObjectState::Critter(state) = &object.raw.state else {
                return false;
            };
            reference != player
                && object.category() == Category::Critter
                && object.owner.is_none()
                && object.raw.elevation == elevation
                && object.alive()
                && state.results.0 & effect::DEAD == 0
                && (object.opponent == player
                    || (player_combat_id != -1 && state.attacker == player_combat_id)
                    || (teams && state.team != player_team))
        })
    }
    fn credit_rest(&mut self, minutes: i32) {
        self.world.rest_credit = self.world.rest_credit.saturating_add(minutes);
        if self.world.rest_credit >= REST_CREDIT_HEAL {
            // Any overshoot is discarded.
            self.world.rest_credit = 0;
            self.heal_party(3);
        }
    }
    /// One UI step's candidate time: reaching/passing the next due event
    /// moves to that tick plus one and processes the queue; afterwards the
    /// time is set (back) to the candidate.
    fn rest_step(&mut self, candidate: u64) -> Result<Option<RestOutcome>, WorldError> {
        if let Some(due) = self.world.queue.next_due()
            && candidate >= due
        {
            self.world.clock.set(due.saturating_add(1));
            let result = self.process_queue()?;
            if result != 0 {
                return Ok(Some(RestOutcome::Interrupted(result)));
            }
        }
        self.world.clock.set(candidate);
        Ok(None)
    }
    /// Advance an explicit wait to its next UI step: the minute phase, then
    /// the hour phase from its own start, each ending on its exact endpoint,
    /// then the final strict catch-up pass. `Ok(None)` means a UI step is
    /// due; `Some(Completed)` ends the wait.
    fn advance_wait(&mut self, wait: &mut Wait) -> Result<Option<RestOutcome>, WorldError> {
        // An hour-phase step's credit applies once its UI step was not
        // cancelled.
        if let Some(credit) = wait.pending_credit.take() {
            self.credit_rest(credit);
        }
        loop {
            match wait.phase {
                Phase::Minutes => {
                    if wait.step < wait.minute_steps.trunc() as u64 {
                        let span = 600.0 * f64::from(wait.minutes);
                        let candidate =
                            (wait.start + wait.step as f64 * span / wait.minute_steps).trunc();
                        wait.step += 1;
                        return self.rest_step(candidate as u64);
                    }
                    self.world
                        .clock
                        .set((wait.start + 600.0 * f64::from(wait.minutes)) as u64);
                    self.credit_rest(wait.minutes);
                    wait.start = self.world.clock.ticks as f64;
                    wait.step = 0;
                    wait.phase = Phase::Hours;
                }
                Phase::Hours => {
                    let span = 36000.0 * f64::from(wait.hours);
                    if wait.step < wait.hour_steps.trunc() as u64 {
                        let candidate =
                            (wait.start + wait.step as f64 * span / wait.hour_steps).trunc();
                        wait.step += 1;
                        if let Some(outcome) = self.rest_step(candidate as u64)? {
                            return Ok(Some(outcome));
                        }
                        wait.pending_credit =
                            Some((60.0 * f64::from(wait.hours) / wait.hour_steps).trunc() as i32);
                        return Ok(None);
                    }
                    self.world.clock.set((wait.start + span) as u64);
                    if self
                        .world
                        .queue
                        .next_due()
                        .is_some_and(|due| self.world.clock.ticks > due)
                    {
                        let result = self.process_queue()?;
                        if result != 0 {
                            return Ok(Some(RestOutcome::Interrupted(result)));
                        }
                    }
                    return Ok(Some(RestOutcome::Completed));
                }
            }
        }
    }
    /// Begin resting for an alarm-menu choice. A refusal, an already full
    /// player for until-healed, or a zero-length wait finishes at once.
    pub fn begin_rest(&mut self, request: RestRequest) -> Result<RestSession, WorldError> {
        let mut session = RestSession {
            plan: Plan::Once,
            wait: None,
            finished: None,
        };
        if !self.rest_allowed() {
            session.finished = Some(RestOutcome::Refused);
            return Ok(session);
        }
        match request {
            RestRequest::Minutes(minutes) => session.wait = Wait::new(0, minutes, self),
            RestRequest::Hours(hours) => session.wait = Wait::new(hours, 0, self),
            RestRequest::UntilHour(hour) => {
                let delta = minutes_until_hour(self.world.clock.ticks, hour);
                session.wait = Wait::new(delta / 60, delta % 60, self);
            }
            RestRequest::UntilHealed => {
                let (deficit, rate) = self.player_deficit();
                if deficit == 0 {
                    session.finished = Some(RestOutcome::Completed);
                    return Ok(session);
                }
                // SPEC-008 §3.9 safe rule: a nonpositive healing rate
                // performs no until-healed rest.
                if rate <= 0 {
                    self.world
                        .diagnostic("until-healed rest skipped: the healing rate is not positive");
                    session.finished = Some(RestOutcome::Completed);
                    return Ok(session);
                }
                session.plan = Plan::Chunks {
                    estimate: 3 * deficit / rate,
                };
            }
        }
        Ok(session)
    }
    /// Advance a rest to its next UI step (the platform waits at least 50
    /// ms between calls, and stops calling to cancel). `Ok(None)` means a UI
    /// step is due; `Some` reports how the rest ended.
    pub fn advance_rest(
        &mut self,
        session: &mut RestSession,
    ) -> Result<Option<RestOutcome>, WorldError> {
        if let Some(outcome) = session.finished {
            return Ok(Some(outcome));
        }
        loop {
            if let Some(wait) = &mut session.wait {
                match self.advance_wait(wait)? {
                    None => return Ok(None),
                    Some(RestOutcome::Completed) => session.wait = None,
                    Some(outcome) => {
                        session.finished = Some(outcome);
                        return Ok(Some(outcome));
                    }
                }
            }
            // The next wait of the plan, if any.
            match session.plan {
                Plan::Once => {
                    session.finished = Some(RestOutcome::Completed);
                    return Ok(Some(RestOutcome::Completed));
                }
                // Until-healed: estimated hours in chunks of at most 24.
                Plan::Chunks { estimate } if estimate > 0 => {
                    let chunk = estimate.min(24);
                    session.plan = Plan::Chunks {
                        estimate: estimate - chunk,
                    };
                    session.wait = Wait::new(chunk, 0, self);
                }
                // Then three-hour rests while the deficit sampled before the
                // previous one was nonzero, so one more can follow full HP.
                Plan::Chunks { .. } => {
                    session.plan = Plan::Repeat {
                        sampled: self.player_deficit().0,
                    };
                }
                Plan::Repeat { sampled } if sampled != 0 => {
                    session.plan = Plan::Repeat {
                        sampled: self.player_deficit().0,
                    };
                    session.wait = Wait::new(3, 0, self);
                }
                Plan::Repeat { .. } => {
                    session.finished = Some(RestOutcome::Completed);
                    return Ok(Some(RestOutcome::Completed));
                }
            }
        }
    }
    fn player_deficit(&self) -> (i32, i32) {
        let context = self.world.context(self.world.player);
        self.world.player_sheet().map_or((0, 0), |sheet| {
            (
                sheet
                    .stat(7, context)
                    .saturating_sub(sheet.current_hp)
                    .max(0),
                sheet.stat(14, context),
            )
        })
    }
    /// Rest for an alarm-menu choice. `ui_step` runs after every step (the
    /// platform waits at least 50 ms there) and returns true to stop.
    pub fn rest(
        &mut self,
        request: RestRequest,
        ui_step: &mut dyn FnMut() -> bool,
    ) -> Result<RestOutcome, WorldError> {
        let mut session = self.begin_rest(request)?;
        loop {
            if let Some(outcome) = self.advance_rest(&mut session)? {
                return Ok(outcome);
            }
            if ui_step() {
                return Ok(RestOutcome::Cancelled);
            }
        }
    }
}
