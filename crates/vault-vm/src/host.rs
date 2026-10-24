use crate::{RunOutcome, Value, Vm, VmError};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalValue {
    Value(Value),
    String(Vec<u8>),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedProcedure {
    pub owner: u32,
    pub address: u32,
    pub arguments: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    Attached,
    Replace,
    Spawn,
    Independent,
}

/// No wall clock, filesystem, or global process registry is implicit in execution.
/// Game/library extensions and program ownership are supplied explicitly by the host.
pub trait VmHost {
    fn now_ms(&self) -> u32 {
        0
    }
    fn extension(&mut self, opcode: u16, _vm: &mut Vm) -> Result<(), VmError> {
        Err(VmError::Opcode(opcode))
    }
    fn program_mut(&mut self, _id: u32) -> Result<&mut Vm, VmError> {
        Err(VmError::Host("program reference".into()))
    }
    fn launch(
        &mut self,
        _name: &[u8],
        _mode: LaunchMode,
        _parent: Option<u32>,
        _critical: bool,
    ) -> Result<u32, VmError> {
        Err(VmError::Host("program launch".into()))
    }
    fn purge(&mut self, _id: u32) -> Result<(), VmError> {
        Ok(())
    }
    /// SPEC-005 §2.2 `0x8009`: the replaced program's exported procedures
    /// are removed (variables are never removed per program).
    fn remove_procedures(&mut self, _owner: u32) -> Result<(), VmError> {
        Ok(())
    }
    /// A debug observer's view of an instruction executed outside an
    /// ordinary run (a conditional procedure's condition stream).
    fn observe_before(&mut self, _vm: &Vm) {}
    fn observe_after(&mut self, _vm: &Vm, _result: &Result<RunOutcome, VmError>) {}
    fn wait_ready(
        &mut self,
        _reference: u32,
        _start: u32,
        _deadline: u32,
    ) -> Result<bool, VmError> {
        Err(VmError::Host("wait predicate".into()))
    }
    fn export_variable(&mut self, _owner: u32, _name: &[u8]) -> Result<(), VmError> {
        Err(VmError::Host("variable registry".into()))
    }
    fn fetch_variable(&mut self, name: &[u8]) -> Result<ExternalValue, VmError> {
        Err(VmError::Symbol(String::from_utf8_lossy(name).into()))
    }
    fn store_variable(&mut self, name: &[u8], _value: ExternalValue) -> Result<(), VmError> {
        Err(VmError::Symbol(String::from_utf8_lossy(name).into()))
    }
    fn export_procedure(
        &mut self,
        _name: &[u8],
        _procedure: ExportedProcedure,
    ) -> Result<(), VmError> {
        Err(VmError::Host("procedure registry".into()))
    }
    fn import_procedure(&mut self, name: &[u8]) -> Result<ExportedProcedure, VmError> {
        Err(VmError::Symbol(String::from_utf8_lossy(name).into()))
    }
}

/// A deterministic shared export registry; applications may wrap it with their
/// program store, asset loader, and extension dispatcher.
#[derive(Debug, Default)]
pub struct BasicHost {
    pub milliseconds: u32,
    variables: BTreeMap<Vec<u8>, (Vec<u8>, ExternalValue)>,
    procedures: BTreeMap<Vec<u8>, ExportedProcedure>,
    filenames: BTreeMap<u32, Vec<u8>>,
}
impl BasicHost {
    pub fn register_program(&mut self, id: u32, filename: &[u8]) {
        self.filenames.insert(id, key(filename));
    }
    /// SPEC-005 §2.2 the full clear of every map load and leave, every full
    /// game reset and new game: both registries are emptied, whatever their
    /// owners.
    pub fn clear(&mut self) {
        self.variables.clear();
        self.procedures.clear();
        self.filenames.clear();
    }
    fn owner(&self, id: u32) -> Vec<u8> {
        self.filenames
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("program-{id}").into_bytes())
    }
}
fn key(name: &[u8]) -> Vec<u8> {
    name.iter().map(u8::to_ascii_lowercase).collect()
}
fn registration(name: &[u8]) -> Vec<u8> {
    key(&name[..name.len().min(31)])
}
fn symbol(name: &[u8]) -> VmError {
    VmError::Symbol(String::from_utf8_lossy(name).into())
}
impl VmHost for BasicHost {
    fn now_ms(&self) -> u32 {
        self.milliseconds
    }
    fn export_variable(&mut self, owner: u32, name: &[u8]) -> Result<(), VmError> {
        let key = registration(name);
        let owner = self.owner(owner);
        if let Some((other, _)) = self.variables.get(&key) {
            if *other != owner {
                return Err(symbol(name));
            }
        } else if self.variables.len() == 1013 {
            return Err(symbol(name));
        }
        self.variables
            .insert(key, (owner, ExternalValue::Value(Value::integer(0))));
        Ok(())
    }
    // Lookups use the same 31-byte retained key as registration, so a long
    // exported name remains reachable (with the documented collision hazard).
    fn fetch_variable(&mut self, name: &[u8]) -> Result<ExternalValue, VmError> {
        self.variables
            .get(&registration(name))
            .map(|(_, value)| value.clone())
            .ok_or_else(|| symbol(name))
    }
    fn store_variable(&mut self, name: &[u8], value: ExternalValue) -> Result<(), VmError> {
        self.variables
            .get_mut(&registration(name))
            .ok_or_else(|| symbol(name))?
            .1 = value;
        Ok(())
    }
    fn export_procedure(
        &mut self,
        name: &[u8],
        procedure: ExportedProcedure,
    ) -> Result<(), VmError> {
        let key = registration(name);
        if !self.procedures.contains_key(&key) && self.procedures.len() == 1013 {
            return Err(symbol(name));
        }
        if self
            .procedures
            .get(&key)
            .is_some_and(|old| old.owner != procedure.owner)
        {
            return Err(symbol(name));
        }
        self.procedures.insert(key, procedure);
        Ok(())
    }
    fn import_procedure(&mut self, name: &[u8]) -> Result<ExportedProcedure, VmError> {
        self.procedures
            .get(&registration(name))
            .cloned()
            .ok_or_else(|| symbol(name))
    }
    // SPEC-005 §2.2: ending a program removes only its procedure entries;
    // exported variables survive until the next full clear.
    fn purge(&mut self, id: u32) -> Result<(), VmError> {
        self.remove_procedures(id)?;
        self.filenames.remove(&id);
        Ok(())
    }
    fn remove_procedures(&mut self, owner: u32) -> Result<(), VmError> {
        self.procedures
            .retain(|_, procedure| procedure.owner != owner);
        Ok(())
    }
}
