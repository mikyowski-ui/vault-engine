use crate::WorldError;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Script {
        script: u32,
        parameter: i32,
    },
    /// Knockout wake-up for the event's owner object.
    Knockout,
    MapUpdate,
    Midnight,
    Poison,
    Radiation,
    Other(u32),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedEvent {
    pub id: u64,
    pub due: u64,
    pub object: u32,
    pub kind: EventKind,
}
#[derive(Debug, Clone, Default)]
pub struct TimedQueue {
    next: u64,
    events: BTreeMap<(u64, u64), TimedEvent>,
}
impl TimedQueue {
    pub fn insert(
        &mut self,
        now: u64,
        delay: i64,
        object: u32,
        kind: EventKind,
    ) -> Result<u64, WorldError> {
        if self.events.len() >= 100_000 {
            return Err(WorldError::Invalid("timed queue limit exceeded".into()));
        }
        let due = now
            .checked_add_signed(delay)
            .ok_or(WorldError::Arithmetic)?;
        self.next = self.next.checked_add(1).ok_or(WorldError::Arithmetic)?;
        self.events.insert(
            (due, self.next),
            TimedEvent {
                id: self.next,
                due,
                object,
                kind,
            },
        );
        Ok(self.next)
    }
    pub fn pop_due(&mut self, snapshot: u64) -> Option<TimedEvent> {
        let key = *self.events.first_key_value()?.0;
        (key.0 <= snapshot).then(|| self.events.remove(&key).expect("existing event"))
    }
    pub fn remove_object(&mut self, object: u32) {
        self.events.retain(|_, event| event.object != object);
    }
    pub fn retain(&mut self, mut keep: impl FnMut(&TimedEvent) -> bool) {
        self.events.retain(|_, event| keep(event));
    }
    pub fn events(&self) -> impl Iterator<Item = &TimedEvent> {
        self.events.values()
    }
    pub fn process(
        &mut self,
        snapshot: u64,
        mut handler: impl FnMut(&mut Self, TimedEvent) -> bool,
    ) -> Result<usize, WorldError> {
        let mut count = 0;
        while let Some(event) = self.pop_due(snapshot) {
            count += 1;
            if count > 100_000 {
                return Err(WorldError::Invalid(
                    "timed event processing limit exceeded".into(),
                ));
            }
            if handler(self, event) {
                break;
            }
        }
        Ok(count)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldClock {
    pub ticks: u64,
    pub last_tick_ms: u64,
    pub last_update_ms: u64,
}
impl Default for WorldClock {
    fn default() -> Self {
        Self {
            ticks: 1,
            last_tick_ms: 0,
            last_update_ms: 0,
        }
    }
}
impl WorldClock {
    pub fn set(&mut self, ticks: u64) {
        self.ticks = ticks.max(1);
    }
    pub fn advance(&mut self, delta: i64) -> Result<(), WorldError> {
        self.ticks = self
            .ticks
            .checked_add_signed(delta)
            .ok_or(WorldError::Arithmetic)?
            .max(1);
        Ok(())
    }
    pub fn hhmm(&self) -> i32 {
        ((self.ticks / 36000 % 24) * 100 + self.ticks / 600 % 60) as i32
    }
    pub fn midnight_delay(&self) -> i64 {
        let hour = (self.ticks / 36000 % 24) as i64;
        let minute = (self.ticks / 600 % 60) as i64;
        10 * (60 * (60 - minute - 1) + 3600 * (24 - hour - 1) + 60)
    }
    /// Returns queue permission and the independent 30-second update request.
    pub fn background(
        &mut self,
        now_ms: u64,
        combat: bool,
        dialogue: bool,
        mode: i32,
    ) -> Result<(bool, bool), WorldError> {
        if dialogue {
            return Ok((false, false));
        }
        let mut queue = !combat && mode != 4;
        if now_ms.saturating_sub(self.last_tick_ms) >= 100 {
            self.last_tick_ms = now_ms;
            if !combat {
                self.advance(1)?;
            }
            queue = true;
        }
        let update = mode != 4 && now_ms.saturating_sub(self.last_update_ms) >= 30000;
        if update {
            self.last_update_ms = now_ms;
        }
        Ok((queue, update))
    }
}
