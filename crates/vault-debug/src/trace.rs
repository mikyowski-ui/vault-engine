//! Bounded instruction traces: a ring buffer that keeps the newest entries.
use crate::decode::Instruction;
use std::{collections::VecDeque, fmt};
use vault_vm::{RunOutcome, VmError};

/// A fixed-capacity ring. Pushing past capacity evicts the oldest entry, so
/// memory stays bounded however long a trace runs. Capacity zero records
/// nothing but still counts pushes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceRing<T> {
    entries: VecDeque<T>,
    capacity: usize,
    pushed: u64,
    evicted: u64,
}
impl<T> TraceRing<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity.min(4096)),
            capacity,
            pushed: 0,
            evicted: 0,
        }
    }
    pub fn capacity(&self) -> usize {
        self.capacity
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Append an entry, returning the one evicted to make room (or the entry
    /// itself when the capacity is zero).
    pub fn push(&mut self, entry: T) -> Option<T> {
        self.pushed += 1;
        if self.capacity == 0 {
            self.evicted += 1;
            return Some(entry);
        }
        let evicted = if self.entries.len() == self.capacity {
            self.evicted += 1;
            self.entries.pop_front()
        } else {
            None
        };
        self.entries.push_back(entry);
        evicted
    }
    /// Entries oldest to newest.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> + ExactSizeIterator {
        self.entries.iter()
    }
    /// The newest `count` entries, oldest to newest.
    pub fn last(&self, count: usize) -> impl DoubleEndedIterator<Item = &T> + ExactSizeIterator {
        self.entries
            .iter()
            .skip(self.entries.len().saturating_sub(count))
    }
    pub fn newest(&self) -> Option<&T> {
        self.entries.back()
    }
    /// Every entry ever pushed, including evicted and cleared ones.
    pub fn pushed(&self) -> u64 {
        self.pushed
    }
    /// Entries dropped because the ring was full.
    pub fn evicted(&self) -> u64 {
        self.evicted
    }
    /// Empty the ring. The push and eviction counters are kept, so sequence
    /// numbers derived from them stay monotonic.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// What one step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    /// `Vm::run` returned normally; `Budget` means the step completed with the
    /// program still runnable.
    Ran(RunOutcome),
    /// The VM faulted; it is now ended with its error state set.
    Fault(VmError),
}
impl fmt::Display for StepOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ran(RunOutcome::Budget) => f.write_str("ok"),
            Self::Ran(outcome) => write!(f, "{}", format!("{outcome:?}").to_lowercase()),
            Self::Fault(error) => write!(f, "fault: {error}"),
        }
    }
}

/// One stepped instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceEntry {
    /// Monotonic step number within its stepper, starting at zero.
    pub seq: u64,
    pub vm: u32,
    /// The instruction address before the step.
    pub pc: u32,
    /// The instruction at `pc`, or `None` when it does not decode (the
    /// step then reports the VM's own fault, if any).
    pub instruction: Option<Instruction>,
    /// The instruction address after the step.
    pub next_pc: u32,
    /// Whether the step changed the VM's address, stacks, or status. A
    /// blocked, waiting, stopped, or ended program steps without executing.
    pub executed: bool,
    /// Complete six-byte values on each stack after the step.
    pub value_depth: usize,
    pub return_depth: usize,
    pub status_before: u16,
    pub status_after: u16,
    pub outcome: StepOutcome,
}
impl fmt::Display for TraceEntry {
    /// `#seq vm=ID 0xPC  INSTRUCTION  -> 0xNEXT vs=N rs=N st=0xBEFORE>0xAFTER OUTCOME`
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:<6} vm={} {:#06x}  ", self.seq, self.vm, self.pc)?;
        match &self.instruction {
            Some(instruction) => write!(f, "{:<24}", instruction.text())?,
            None => write!(f, "{:<24}", "<undecodable>")?,
        }
        write!(
            f,
            " -> {:#06x} vs={} rs={} st={:#06x}>{:#06x} {}",
            self.next_pc,
            self.value_depth,
            self.return_depth,
            self.status_before,
            self.status_after,
            self.outcome
        )?;
        if !self.executed {
            f.write_str(" (not executed)")?;
        }
        Ok(())
    }
}

/// Render the newest `count` entries of a trace, one per line, with a header
/// giving the retained/pushed/evicted counts.
pub fn render_trace(trace: &TraceRing<TraceEntry>, count: usize) -> String {
    let mut text = format!(
        "trace: {} of {} retained ({} evicted, capacity {})\n",
        trace.len(),
        trace.pushed(),
        trace.evicted(),
        trace.capacity()
    );
    for entry in trace.last(count) {
        text.push_str(&entry.to_string());
        text.push('\n');
    }
    text
}
