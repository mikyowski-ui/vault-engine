//! Headless inspectors for the VM and world state, with deterministic
//! plain-text rendering for overlays and CLI dumps.
//!
//! - [`decode`] and [`stepper`]: read-only SPEC-004 instruction decoding,
//!   single-instruction stepping with address breakpoints, and a bounded
//!   [`trace::TraceRing`] of [`trace::TraceEntry`] records.
//! - [`watch`]: GVAR/MVAR/LVAR watches (flat or per-script locals) diffed
//!   between ticks.
//! - [`objects`]: map object and critter snapshots, filters, and diffs.
//!
//! Nothing here mutates world state; stepping drives the VM only through
//! `Vm::run`. A frontend owns one [`Inspector`] and calls
//! [`Inspector::tick`] once per world update:
//!
//! ```
//! use vault_debug::{Inspector, InspectorConfig};
//! # let world = vault_core::WorldState::empty(1).unwrap();
//! let mut inspector = Inspector::new(InspectorConfig::default());
//! // Each frame, after the world update:
//! let frame = inspector.tick(&world);
//! if !frame.is_empty() {
//!     println!("{frame}");
//! }
//! ```
pub mod decode;
pub mod objects;
pub mod stepper;
pub mod trace;
pub mod watch;

pub use decode::{DecodeError, Instruction, OpcodeClass};
pub use objects::{CritterSnapshot, ObjectChange, ObjectFilter, ObjectSnapshot, ObjectTracker};
pub use stepper::{RunStop, Stepper, StopReason, VmSnapshot};
pub use trace::{StepOutcome, TraceEntry, TraceRing};
pub use watch::{VarAddress, VarChange, VarScope, VarSnapshot, WatchList, Watcher};

use std::fmt;
use vault_core::WorldState;
use vault_vm::{Vm, VmHost};

/// What an [`Inspector`] watches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectorConfig {
    /// Retained instruction trace entries.
    pub trace_capacity: usize,
    pub watches: WatchList,
    pub objects: ObjectFilter,
    /// Diff objects on every tick (snapshots cost one pass over the world).
    pub track_objects: bool,
}
impl Default for InspectorConfig {
    /// A 256-entry trace, no variable watches, and every active object
    /// tracked.
    fn default() -> Self {
        Self {
            trace_capacity: 256,
            watches: WatchList::new(),
            objects: ObjectFilter::default(),
            track_objects: true,
        }
    }
}

/// Changes observed by one [`Inspector::tick`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectorFrame {
    /// Ticks since the inspector was created, starting at zero.
    pub tick: u64,
    pub vars: Vec<VarChange>,
    pub objects: Vec<ObjectChange>,
}
impl InspectorFrame {
    pub fn is_empty(&self) -> bool {
        self.vars.is_empty() && self.objects.is_empty()
    }
}
impl fmt::Display for InspectorFrame {
    /// `tick N: V var changes, O object changes`, then one line per change.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "tick {}: {} var changes, {} object changes",
            self.tick,
            self.vars.len(),
            self.objects.len()
        )?;
        for change in &self.vars {
            writeln!(f, "  {change}")?;
        }
        for change in &self.objects {
            writeln!(f, "  {change}")?;
        }
        Ok(())
    }
}

/// The single entry point a frontend wires in: variable and object tracking
/// per world tick, plus a VM stepper with its trace.
#[derive(Debug, Clone)]
pub struct Inspector {
    pub stepper: Stepper,
    pub watcher: Watcher,
    pub objects: ObjectTracker,
    track_objects: bool,
    ticks: u64,
}
impl Inspector {
    pub fn new(config: InspectorConfig) -> Self {
        Self {
            stepper: Stepper::new(config.trace_capacity),
            watcher: Watcher::new(config.watches),
            objects: ObjectTracker::new(config.objects),
            track_objects: config.track_objects,
            ticks: 0,
        }
    }
    /// Record this tick's watched variables and objects and return what
    /// changed since the previous tick. The first tick sets the baseline.
    pub fn tick(&mut self, world: &WorldState) -> InspectorFrame {
        let frame = InspectorFrame {
            tick: self.ticks,
            vars: self.watcher.tick(world),
            objects: if self.track_objects {
                self.objects.tick(world)
            } else {
                Vec::new()
            },
        };
        self.ticks += 1;
        frame
    }
    /// Step one instruction of `vm` (see [`Stepper::step`]).
    pub fn step(&mut self, vm: &mut Vm, host: &mut impl VmHost) -> TraceEntry {
        self.stepper.step(vm, host)
    }
    /// Step until a breakpoint or stop (see [`Stepper::run_until_break`]).
    pub fn run_until_break(
        &mut self,
        vm: &mut Vm,
        host: &mut impl VmHost,
        max_steps: u32,
    ) -> RunStop {
        self.stepper.run_until_break(vm, host, max_steps)
    }
    /// Forget the variable and object baselines; the next tick reports none.
    pub fn reset_baselines(&mut self) {
        self.watcher.reset();
        self.objects.reset();
    }
    /// A full text dump of the current world view: watched variables, then
    /// the filtered object and critter tables. Does not affect baselines.
    pub fn render_world(&self, world: &WorldState) -> String {
        let vars = self.watcher.list.snapshot(world);
        let objects = objects::snapshot_objects(world, &self.objects.filter);
        format!(
            "== vars ==\n{}== objects ==\n{}== critters ==\n{}",
            watch::render_vars(&vars, &self.watcher.list),
            objects::render_objects(&objects),
            objects::render_critters(&objects)
        )
    }
    /// A text dump of one VM's state followed by the newest `count` trace
    /// entries.
    pub fn render_vm(&self, vm: &Vm, count: usize) -> String {
        format!(
            "{}{}",
            VmSnapshot::of(vm),
            trace::render_trace(self.stepper.trace(), count)
        )
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/fixtures/formats/support.rs"]
pub(crate) mod fixture_assets;
