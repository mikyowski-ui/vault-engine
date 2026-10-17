//! Deterministic, bounded implementation of approved SPEC-004.
mod arithmetic;
mod host;
mod machine;
mod program;
mod stack;

pub mod procedure_flags {
    pub use crate::program::{CONDITIONAL, CRITICAL, EXPORT, IMPORT, TIMED};
}

pub use host::{BasicHost, ExportedProcedure, ExternalValue, LaunchMode, VmHost};
pub use machine::{RunOutcome, Vm, VmStatus};
pub use program::{IntError, IntProgram, Lifecycle, Procedure};
pub use stack::{Stack, Value, VmError};

#[cfg(test)]
mod tests;

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/fixtures/formats/support.rs"]
pub(crate) mod fixture_assets;
