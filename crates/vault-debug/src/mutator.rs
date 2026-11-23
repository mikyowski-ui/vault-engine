//! Debug commands for one object. [`ObjectMutator`] only builds them: nothing
//! here touches a world, so a frontend sends each through the logged
//! `InputCommand::Debug` path and a replay reproduces it.
use vault_core::debug::DebugCommand;

/// Builds [`DebugCommand`]s for the object with this map object id (`raw.id`,
/// never a handle, as logged records require).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectMutator(pub u32);
impl ObjectMutator {
    pub fn hp(self, value: i32) -> DebugCommand {
        DebugCommand::SetHitPoints {
            object: self.0,
            value,
        }
    }
    pub fn ap(self, value: i32) -> DebugCommand {
        DebugCommand::SetActionPoints {
            object: self.0,
            value,
        }
    }
    /// A door's state; `None` leaves that part as it is.
    pub fn door(
        self,
        open: Option<bool>,
        locked: Option<bool>,
        jammed: Option<bool>,
    ) -> DebugCommand {
        DebugCommand::SetDoor {
            object: self.0,
            open,
            locked,
            jammed,
        }
    }
    /// How many of `pid` this object carries.
    pub fn item_count(self, pid: u32, count: u32) -> DebugCommand {
        DebugCommand::SetItemCount {
            owner: self.0,
            pid,
            count,
        }
    }
    /// Run this object's script procedure for `event`, with `fixed` as its
    /// fixed parameter.
    pub fn fire(self, event: i32, fixed: i32) -> DebugCommand {
        DebugCommand::FireProc {
            object: self.0,
            event,
            fixed,
        }
    }
    /// `set_local_var` in the script attached to this object.
    pub fn lvar(self, index: usize, value: i32) -> DebugCommand {
        DebugCommand::SetObjectLocalVar {
            object: self.0,
            index,
            value,
        }
    }
}
