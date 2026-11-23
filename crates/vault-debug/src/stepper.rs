//! Single-instruction VM stepping with address breakpoints, recorded into a
//! bounded trace.
use crate::{
    decode::{Instruction, decode_next},
    trace::{HoldBreach, StepOutcome, TraceEntry, TraceRing},
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
    /// A [`CriticalHold`] was breached; the last step did not execute.
    Refused(HoldBreach),
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
            StopReason::Refused(breach) => write!(f, "refused: {breach}"),
            StopReason::StepLimit => f.write_str("step limit"),
        }
    }
}

/// A critical section a stepper stopped inside, with its VM still runnable.
///
/// SPEC-004 §3.3 has a critical section run to its end before anything else
/// runs, and scripts rely on that for what they share. Stopping inside one is
/// invisible to them only while nothing else runs, so **while
/// [`Stepper::critical_hold`] is `Some`, a caller must not tick the world or
/// run another program.** A step checks what it can see of that, and refuses
/// with a [`HoldBreach`], changing nothing, when another VM asks to step or
/// the host's clock has moved. It cannot see a program run on the host with
/// the clock still, or a tick that leaves the clock where it was, so the
/// check does not replace the caller's freeze. After a breach the section's
/// promise is already broken: [`Stepper::release_critical`] accepts that and
/// lets stepping go on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CriticalHold {
    pub vm: u32,
    /// The host's clock when the VM stopped.
    pub now_ms: u32,
}

/// Steps a VM one instruction at a time.
///
/// By default each step is `Vm::run(host, 1)`, so scheduling (wait release,
/// yield, blocked/ended checks) behaves exactly as in normal execution. Inside
/// a critical section the VM ignores its budget, so one step runs the whole
/// critical section (up to the VM's hard instruction limit) and is recorded
/// as a single entry for its first instruction.
///
/// With [`step_into_critical`](Self::step_into_critical) on, each step is
/// `Vm::step`, with the same checks, one instruction also inside a critical
/// section, and breakpoints there stop. See [`CriticalHold`] for what that
/// asks of the caller. `Vm::step` counts each step alone, so the hard
/// instruction limit does not bound a stepped section; `max_steps` does.
#[derive(Debug, Clone)]
pub struct Stepper {
    /// Stop inside critical sections, one instruction at a time. Turned off
    /// while a section is held, the next step runs the rest of it at once,
    /// plus the one counted instruction after it.
    pub step_into_critical: bool,
    hold: Option<CriticalHold>,
    breakpoints: BTreeSet<u32>,
    trace: TraceRing<TraceEntry>,
}
impl Stepper {
    pub fn new(trace_capacity: usize) -> Self {
        Self {
            step_into_critical: false,
            hold: None,
            breakpoints: BTreeSet::new(),
            trace: TraceRing::new(trace_capacity),
        }
    }
    /// The critical section this stepper is stopped inside, during which the
    /// caller must keep the world still.
    pub fn critical_hold(&self) -> Option<CriticalHold> {
        self.hold
    }
    /// Forget the hold: after a breach, or when its VM has been discarded.
    /// Returns the hold there was.
    pub fn release_critical(&mut self) -> Option<CriticalHold> {
        self.hold.take()
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
    /// than returned, so the trace keeps the faulting instruction, and a
    /// refused step likewise, with nothing executed.
    pub fn step(&mut self, vm: &mut Vm, host: &mut impl VmHost) -> TraceEntry {
        let pending = PendingStep::of(vm);
        let outcome = match self.breach(vm, host) {
            Some(breach) => StepOutcome::Refused(breach),
            None => {
                let result = if self.step_into_critical {
                    vm.step(host)
                } else {
                    vm.run(host, 1)
                };
                // Any other outcome ends the run here in normal execution
                // too, critical or not, so others may run (SPEC-004 §3.3).
                let held =
                    result == Ok(RunOutcome::Budget) && vm.status.contains(VmStatus::CRITICAL);
                self.hold = held.then(|| CriticalHold {
                    vm: vm.id,
                    now_ms: host.now_ms(),
                });
                result.into()
            }
        };
        let entry = pending.finish(vm, outcome, self.trace.pushed());
        self.trace.push(entry.clone());
        entry
    }
    /// How a step of `vm` now would break the hold, if one is in place.
    fn breach(&self, vm: &Vm, host: &impl VmHost) -> Option<HoldBreach> {
        let hold = self.hold?;
        let now_ms = host.now_ms();
        if vm.id != hold.vm {
            Some(HoldBreach::OtherVm {
                held: hold.vm,
                stepped: vm.id,
            })
        } else if now_ms != hold.now_ms {
            Some(HoldBreach::ClockMoved {
                vm: hold.vm,
                from_ms: hold.now_ms,
                to_ms: now_ms,
            })
        } else {
            None
        }
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
                StepOutcome::Refused(breach) => {
                    return RunStop {
                        steps,
                        reason: StopReason::Refused(breach),
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

/// What a trace entry needs from before its step ran.
#[derive(Debug, Clone)]
pub(crate) struct PendingStep {
    pc: u32,
    instruction: Option<Instruction>,
    status_before: u16,
    before: Fingerprint,
}
impl PendingStep {
    pub(crate) fn of(vm: &Vm) -> Self {
        Self {
            pc: vm.pc,
            instruction: decode_next(vm).ok(),
            status_before: vm.status.0,
            before: Fingerprint::of(vm),
        }
    }
    /// The entry for the step that has now run.
    pub(crate) fn finish(self, vm: &Vm, outcome: StepOutcome, seq: u64) -> TraceEntry {
        TraceEntry {
            seq,
            vm: vm.id,
            pc: self.pc,
            instruction: self.instruction,
            next_pc: vm.pc,
            executed: !self.before.matches(vm),
            value_depth: vm.value_stack.byte_len() / 6,
            return_depth: vm.return_stack.byte_len() / 6,
            status_before: self.status_before,
            status_after: vm.status.0,
            outcome,
        }
    }
}

/// Everything a step can change, captured once before the step and
/// compared against the live VM afterwards (no second copy of the stacks).
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Whether `vm` still matches this fingerprint.
    fn matches(&self, vm: &Vm) -> bool {
        self.pc == vm.pc
            && self.status == vm.status.0
            && self.values == vm.value_stack.bytes()
            && self.returns == vm.return_stack.bytes()
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
