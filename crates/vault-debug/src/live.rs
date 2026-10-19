//! A live trace of a world's script execution. Attached to a vault-core
//! `WorldHost` as its `VmObserver`, it records every instruction the
//! world's scripts execute, nested runs included, into a bounded
//! [`TraceRing`], without stepping any VM itself.
use crate::{
    stepper::PendingStep,
    trace::{TraceEntry, TraceRing},
};
use vault_core::VmObserver;
use vault_vm::{RunOutcome, Vm, VmError};

#[derive(Debug, Clone)]
pub struct LiveTrace {
    trace: TraceRing<TraceEntry>,
    /// The steps under way, innermost last (a host call can run another
    /// script inside a step).
    pending: Vec<PendingStep>,
}
impl LiveTrace {
    pub fn new(capacity: usize) -> Self {
        Self {
            trace: TraceRing::new(capacity),
            pending: Vec::new(),
        }
    }
    pub fn trace(&self) -> &TraceRing<TraceEntry> {
        &self.trace
    }
    pub fn clear(&mut self) {
        self.trace.clear();
    }
}
impl VmObserver for LiveTrace {
    fn before_step(&mut self, vm: &Vm) {
        self.pending.push(PendingStep::of(vm));
    }
    fn after_step(&mut self, vm: &Vm, result: &Result<RunOutcome, VmError>) {
        if let Some(pending) = self.pending.pop() {
            let entry = pending.finish(vm, result.clone(), self.trace.pushed());
            self.trace.push(entry);
        }
    }
}
