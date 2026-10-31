//! Deterministic, bounded implementation of approved SPEC-004.
mod arithmetic;
pub mod extensions;
mod host;
mod machine;
mod program;
pub mod sfall;
pub mod sfall_arrays;
mod sfall_math;
mod sfall_text;
mod stack;
pub mod stack_check;

pub mod procedure_flags {
    pub use crate::program::{CONDITIONAL, CRITICAL, EXPORT, IMPORT, TIMED};
}

pub use arithmetic::float_text;
pub use host::{BasicHost, ExportedProcedure, ExternalValue, LaunchMode, VmHost};
pub use machine::{RunOutcome, Vm, VmStatus};
pub use program::{IntError, IntProgram, Lifecycle, Procedure, WordSet};
pub use stack::{Stack, Value, VmError};

#[cfg(test)]
mod sfall_tests;
#[cfg(test)]
mod tests;
