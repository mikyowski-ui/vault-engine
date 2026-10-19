//! Single-instruction VM stepping with address breakpoints, recorded into a
//! bounded trace.
use crate::{
    decode::{Instruction, decode_next},
    trace::{StepOutcome, TraceEntry, TraceRing},
};
use std::{collections::BTreeSet, fmt};
use vault_vm::{RunOutcome, Value, Vm, VmHost, VmStatus};

/// Why `Stepper::run_until_break` returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// The next instruction is at a breakpoint; it has not executed.
    Breakpoint(u32),
    /// The VM stopped running on its own (ended, stopped, blocked, waiting,
    /// yielded, or reached a fragment end).
    Outcome(RunOutcome),
    Fault(vault_vm::VmError),
    /// `max_steps` steps ran without another reason to stop.
    StepLimit,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunStop {
    pub steps: u32,
    pub reason: StopReason,
}
impl fmt::Display for RunStop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "stopped after {} steps: ", self.steps)?;
        match &self.reason {
            StopReason::Breakpoint(at) => write!(f, "breakpoint at {at:#06x}"),
            StopReason::Outcome(outcome) => {
                write!(f, "{}", format!("{outcome:?}").to_lowercase())
            }
            StopReason::Fault(error) => write!(f, "fault: {error}"),
            StopReason::StepLimit => f.write_str("step limit"),
        }
    }
}

/// Steps a VM one instruction at a time.
///
/// Each step is `Vm::run(host, 1)`, so scheduling (wait release, yield,
/// blocked/ended checks) behaves exactly as in normal execution. Inside a
/// critical section the VM ignores its budget, so one step runs the whole
/// critical section (up to the VM's hard instruction limit) and is recorded
/// as a single entry for its first instruction.
#[derive(Debug, Clone)]
pub struct Stepper {
    breakpoints: BTreeSet<u32>,
    trace: TraceRing<TraceEntry>,
}
impl Stepper {
    pub fn new(trace_capacity: usize) -> Self {
        Self {
            breakpoints: BTreeSet::new(),
            trace: TraceRing::new(trace_capacity),
        }
    }
    /// Break before executing the instruction at `address`. Returns whether
    /// it was newly added.
    pub fn add_breakpoint(&mut self, address: u32) -> bool {
        self.breakpoints.insert(address)
    }
    pub fn remove_breakpoint(&mut self, address: u32) -> bool {
        self.breakpoints.remove(&address)
    }
    pub fn clear_breakpoints(&mut self) {
        self.breakpoints.clear();
    }
    pub fn breakpoints(&self) -> impl Iterator<Item = u32> + '_ {
        self.breakpoints.iter().copied()
    }
    pub fn trace(&self) -> &TraceRing<TraceEntry> {
        &self.trace
    }
    pub fn clear_trace(&mut self) {
        self.trace.clear();
    }
    /// Execute at most one instruction (see the type docs for critical
    /// sections) and record it. A VM fault is recorded as the outcome rather
    /// than returned, so the trace keeps the faulting instruction.
    pub fn step(&mut self, vm: &mut Vm, host: &mut impl VmHost) -> TraceEntry {
        let pc = vm.pc;
        let instruction = decode_next(vm).ok();
        let status_before = vm.status.0;
        let before = Fingerprint::of(vm);
        let outcome = match vm.run(host, 1) {
            Ok(outcome) => StepOutcome::Ran(outcome),
            Err(error) => StepOutcome::Fault(error),
        };
        let entry = TraceEntry {
            seq: self.trace.pushed(),
            vm: vm.id,
            pc,
            instruction,
            next_pc: vm.pc,
            executed: Fingerprint::of(vm) != before,
            value_depth: vm.value_stack.byte_len() / 6,
            return_depth: vm.return_stack.byte_len() / 6,
            status_before,
            status_after: vm.status.0,
            outcome,
        };
        self.trace.push(entry.clone());
        entry
    }
    /// Step until a breakpoint, a non-runnable outcome, a fault, or
    /// `max_steps` steps. A breakpoint at the starting address does not stop
    /// the first step, so a run can continue from a breakpoint.
    pub fn run_until_break(
        &mut self,
        vm: &mut Vm,
        host: &mut impl VmHost,
        max_steps: u32,
    ) -> RunStop {
        let mut steps = 0;
        while steps < max_steps {
            if steps > 0 && self.breakpoints.contains(&vm.pc) {
                return RunStop {
                    steps,
                    reason: StopReason::Breakpoint(vm.pc),
                };
            }
            let entry = self.step(vm, host);
            steps += 1;
            match entry.outcome {
                StepOutcome::Ran(RunOutcome::Budget) => {}
                StepOutcome::Ran(outcome) => {
                    return RunStop {
                        steps,
                        reason: StopReason::Outcome(outcome),
                    };
                }
                StepOutcome::Fault(error) => {
                    return RunStop {
                        steps,
                        reason: StopReason::Fault(error),
                    };
                }
            }
        }
        let reason = if self.breakpoints.contains(&vm.pc) && steps > 0 {
            StopReason::Breakpoint(vm.pc)
        } else {
            StopReason::StepLimit
        };
        RunStop { steps, reason }
    }
}

/// Everything a step can change, compared to tell whether it executed.
#[derive(PartialEq, Eq)]
struct Fingerprint {
    pc: u32,
    status: u16,
    values: Vec<u8>,
    returns: Vec<u8>,
}
impl Fingerprint {
    fn of(vm: &Vm) -> Self {
        Self {
            pc: vm.pc,
            status: vm.status.0,
            values: vm.value_stack.bytes().to_vec(),
            returns: vm.return_stack.bytes().to_vec(),
        }
    }
}

/// A point-in-time view of one VM's registers, stacks, and next instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmSnapshot {
    pub id: u32,
    pub pc: u32,
    pub status: u16,
    /// Local frame base `F` and global base `G` (byte heights, -1 unset).
    pub frame: i32,
    pub globals: i32,
    /// Value-stack values, bottom first.
    pub values: Vec<Value>,
    /// Return-stack values, bottom first.
    pub returns: Vec<Value>,
    pub next: Result<Instruction, crate::decode::DecodeError>,
    pub last_error: Option<vault_vm::VmError>,
}
impl VmSnapshot {
    pub fn of(vm: &Vm) -> Self {
        let values = |bytes: &[u8]| {
            bytes
                .as_chunks::<6>()
                .0
                .iter()
                .map(|slot| Value {
                    payload: u32::from_be_bytes([slot[0], slot[1], slot[2], slot[3]]),
                    tag: u16::from_be_bytes([slot[4], slot[5]]),
                })
                .collect()
        };
        Self {
            id: vm.id,
            pc: vm.pc,
            status: vm.status.0,
            frame: vm.f,
            globals: vm.g,
            values: values(vm.value_stack.bytes()),
            returns: values(vm.return_stack.bytes()),
            next: decode_next(vm),
            last_error: vm.last_error.clone(),
        }
    }
}

/// Status bit names in SPEC-004 order, e.g. `ended|error`, or `running`.
pub fn status_names(status: u16) -> String {
    let names: Vec<&str> = [
        (VmStatus::ENDED, "ended"),
        (VmStatus::ERROR, "error"),
        (VmStatus::STOPPED, "stopped"),
        (VmStatus::WAITING, "waiting"),
        (VmStatus::ATTACHED, "attached"),
        (VmStatus::YIELD, "yield"),
        (VmStatus::CRITICAL, "critical"),
        (VmStatus::SPAWN_BLOCKED, "spawn-blocked"),
    ]
    .into_iter()
    .filter(|(flag, _)| status & flag != 0)
    .map(|(_, name)| name)
    .collect();
    if names.is_empty() {
        "running".into()
    } else {
        names.join("|")
    }
}

/// A tagged value as `TAG:PAYLOAD`, with integers in decimal.
pub fn value_text(value: Value) -> String {
    match value.tag {
        Value::INTEGER => format!("int:{}", value.payload as i32),
        Value::FLOAT => format!(
            "float:{}",
            String::from_utf8_lossy(&vault_vm::float_text(f32::from_bits(value.payload)))
        ),
        Value::STATIC_STRING => format!("str:{:#x}", value.payload),
        Value::DYNAMIC_STRING => format!("dstr:{:#x}", value.payload),
        tag => format!("{tag:#06x}:{:#x}", value.payload),
    }
}

impl fmt::Display for VmSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "vm {} pc={:#06x} status={:#06x} ({}) F={} G={}",
            self.id,
            self.pc,
            self.status,
            status_names(self.status),
            self.frame,
            self.globals
        )?;
        match &self.next {
            Ok(instruction) => writeln!(f, "next: {instruction}")?,
            Err(error) => writeln!(f, "next: {error}")?,
        }
        for (name, stack) in [("values", &self.values), ("returns", &self.returns)] {
            let text: Vec<String> = stack.iter().map(|value| value_text(*value)).collect();
            writeln!(f, "{name} [{}]: {}", stack.len(), text.join(" "))?;
        }
        if let Some(error) = &self.last_error {
            writeln!(f, "last error: {error}")?;
        }
        Ok(())
    }
}
