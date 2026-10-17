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
        let town = self.world.world_map.saved.town;
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
    fn rest_step(
        &mut self,
        candidate: u64,
        ui_step: &mut dyn FnMut() -> bool,
    ) -> Result<Option<RestOutcome>, WorldError> {
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
        Ok((ui_step()).then_some(RestOutcome::Cancelled))
    }
    /// An explicit H-hour, M-minute wait: the minute phase, then the hour
    /// phase from its own start, each ending on its exact endpoint, then the
    /// final strict catch-up pass.
    fn rest_for(
        &mut self,
        hours: i32,
        minutes: i32,
        ui_step: &mut dyn FnMut() -> bool,
    ) -> Result<RestOutcome, WorldError> {
        let Some((_, minute_steps, hour_steps)) = rest_allocation(hours, minutes) else {
            return Ok(RestOutcome::Completed);
        };
        let start = self.world.clock.ticks as f64;
        let span = 600.0 * f64::from(minutes);
        for step in 0..minute_steps.trunc() as u64 {
            let candidate = (start + step as f64 * span / minute_steps).trunc() as u64;
            if let Some(outcome) = self.rest_step(candidate, ui_step)? {
                return Ok(outcome);
            }
        }
        self.world.clock.set((start + span) as u64);
        self.credit_rest(minutes);
        let start = self.world.clock.ticks as f64;
        let span = 36000.0 * f64::from(hours);
        let credit = (60.0 * f64::from(hours) / hour_steps).trunc() as i32;
        for step in 0..hour_steps.trunc() as u64 {
            let candidate = (start + step as f64 * span / hour_steps).trunc() as u64;
            if let Some(outcome) = self.rest_step(candidate, ui_step)? {
                return Ok(outcome);
            }
            self.credit_rest(credit);
        }
        self.world.clock.set((start + span) as u64);
        if self
            .world
            .queue
            .next_due()
            .is_some_and(|due| self.world.clock.ticks > due)
        {
            let result = self.process_queue()?;
            if result != 0 {
                return Ok(RestOutcome::Interrupted(result));
            }
        }
        Ok(RestOutcome::Completed)
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
        if !self.rest_allowed() {
            return Ok(RestOutcome::Refused);
        }
        match request {
            RestRequest::Minutes(minutes) => self.rest_for(0, minutes, ui_step),
            RestRequest::Hours(hours) => self.rest_for(hours, 0, ui_step),
            RestRequest::UntilHour(hour) => {
                let delta = minutes_until_hour(self.world.clock.ticks, hour);
                self.rest_for(delta / 60, delta % 60, ui_step)
            }
            RestRequest::UntilHealed => {
                let (deficit, rate) = self.player_deficit();
                if deficit == 0 {
                    return Ok(RestOutcome::Completed);
                }
                if rate <= 0 {
                    return Err(WorldError::Invalid(
                        "until-healed rest needs a positive healing rate".into(),
                    ));
                }
                let mut estimate = 3 * deficit / rate;
                while estimate > 0 {
                    let chunk = estimate.min(24);
                    estimate -= chunk;
                    match self.rest_for(chunk, 0, ui_step)? {
                        RestOutcome::Completed => {}
                        outcome => return Ok(outcome),
                    }
                }
                // Repeats while the deficit sampled before the previous
                // chunk was nonzero, so one more chunk can follow full HP.
                let mut sampled = self.player_deficit().0;
                while sampled != 0 {
                    sampled = self.player_deficit().0;
                    match self.rest_for(3, 0, ui_step)? {
                        RestOutcome::Completed => {}
                        outcome => return Ok(outcome),
                    }
                }
                Ok(RestOutcome::Completed)
            }
        }
    }
}
