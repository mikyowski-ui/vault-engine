use serde::{Deserialize, Serialize};

/// A command's position in a deterministic stream, starting at one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CommandId(u64);

impl CommandId {
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// An event's position in a separate deterministic stream, starting at one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EventId(u64);

impl EventId {
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// World-owned counters, independent of wall clocks, threads, or process globals.
///
/// `WorldState` carries them, but the SPEC-008 save has no field for them,
/// so a loaded game restarts both streams. Serializing them with any other
/// state (as below) resumes a stream without reusing IDs.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SequenceIds {
    last_command: u64,
    last_event: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{stream} sequence exhausted")]
pub struct SequenceExhausted {
    pub stream: &'static str,
}

impl SequenceIds {
    pub const fn from_last(last_command: u64, last_event: u64) -> Self {
        Self {
            last_command,
            last_event,
        }
    }

    pub fn next_command(&mut self) -> Result<CommandId, SequenceExhausted> {
        let next = self
            .last_command
            .checked_add(1)
            .ok_or(SequenceExhausted { stream: "command" })?;
        self.last_command = next;
        Ok(CommandId(next))
    }

    pub fn next_event(&mut self) -> Result<EventId, SequenceExhausted> {
        let next = self
            .last_event
            .checked_add(1)
            .ok_or(SequenceExhausted { stream: "event" })?;
        self.last_event = next;
        Ok(EventId(next))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_and_restore_produce_identical_independent_streams() {
        let fixture = include_str!("../../../tests/fixtures/core/sequences.txt");
        let mut original = SequenceIds::default();
        let mut restored = SequenceIds::default();
        for line in fixture.lines() {
            let (kind, expected) = line.split_once(' ').unwrap();
            let expected: u64 = expected.parse().unwrap();
            for ids in [&mut original, &mut restored] {
                let actual = match kind {
                    "command" => ids.next_command().unwrap().get(),
                    "event" => ids.next_event().unwrap().get(),
                    _ => panic!("unknown synthetic command"),
                };
                assert_eq!(actual, expected);
            }
            restored = serde_json::from_slice(&serde_json::to_vec(&restored).unwrap()).unwrap();
        }
        assert_eq!(original, restored);
    }

    #[test]
    fn exhaustion_never_wraps_or_advances_the_other_stream() {
        let mut ids = SequenceIds::from_last(u64::MAX - 1, u64::MAX - 1);
        assert_eq!(ids.next_command().unwrap().get(), u64::MAX);
        for _ in 0..2 {
            assert_eq!(ids.next_command().unwrap_err().stream, "command");
        }
        assert_eq!(ids.next_event().unwrap().get(), u64::MAX);
        for _ in 0..2 {
            assert_eq!(ids.next_event().unwrap_err().stream, "event");
        }
    }
}
