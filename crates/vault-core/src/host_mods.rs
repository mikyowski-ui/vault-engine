//! SPEC-024a: manifest mods on the host. A layer's `vault-mod.toml` attaches
//! `.INT` scripts to the hook points of `crate::host_sfall_hooks`, whose
//! chains gain a mod block after the sfall block. This module holds the
//! session's mods, their settings and log, the setup step of §4.4, and the
//! words of §7. Nothing here is saved or hashed (§6).
use crate::{assets::WorldAssets, host::WorldHost, host_sfall::SfallValue};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};
pub use vault_vm::extensions::MOD_API_VERSION;
use vault_vm::{
    Vm, VmError,
    sfall::{self, Class, SfallRow},
};

/// §7.5: the log's newest lines, and how long one is.
const LOG_LINES: usize = 256;
const LOG_TEXT: usize = 200;

/// A `[[global_scripts]]` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModGlobal {
    /// The lowercased name under `scripts/`.
    pub file: String,
    pub repeat: i32,
    pub kind: i32,
}
/// A `[[hooks]]` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModHook {
    pub hook: i32,
    /// The lowercased name under `scripts/`.
    pub file: String,
    pub procedure: String,
}
/// One scripted enabled layer.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptedMod {
    pub id: String,
    pub version: String,
    /// `fnv1a64` of the manifest's bytes (§2.3).
    pub manifest: u64,
    pub globals: Vec<ModGlobal>,
    pub hooks: Vec<ModHook>,
    /// The `[settings]` values, which fix each key's type.
    pub defaults: BTreeMap<String, SfallValue>,
}
#[derive(Debug, Default)]
struct SessionState {
    values: Vec<BTreeMap<String, SfallValue>>,
    /// The mods with a write this step (§7.4's commit).
    changed: BTreeSet<usize>,
    log: VecDeque<String>,
    /// Lines the shell has not yet sent to the debug log.
    unsent: Vec<String>,
    /// Diagnostics from before a host existed.
    notes: Vec<String>,
}
/// The session's scripted mods, lowest layer first. It outlives a world, so
/// a new game or a load changes no setting (§7.4).
#[derive(Debug)]
pub struct ModSession {
    mods: Vec<ScriptedMod>,
    /// Whether changed settings reach storage: a replay never commits.
    commits: bool,
    state: Mutex<SessionState>,
}
impl ModSession {
    pub fn new(mods: Vec<ScriptedMod>, commits: bool) -> Self {
        let state = SessionState {
            values: mods.iter().map(|one| one.defaults.clone()).collect(),
            ..SessionState::default()
        };
        Self {
            mods,
            commits,
            state: Mutex::new(state),
        }
    }
    fn state(&self) -> MutexGuard<'_, SessionState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
    pub fn mods(&self) -> &[ScriptedMod] {
        &self.mods
    }
    /// Every declared key's value in mod `index`, ascending by key bytes.
    pub fn settings(&self, index: usize) -> BTreeMap<String, SfallValue> {
        self.state().values.get(index).cloned().unwrap_or_default()
    }
    /// The value of `key`, if mod `index` declares it.
    fn setting(&self, index: usize, key: &[u8]) -> Option<SfallValue> {
        let state = self.state();
        let values = state.values.get(index)?;
        let key = std::str::from_utf8(key).ok()?;
        values.get(key).cloned()
    }
    /// `value` as `key`'s type: int by class `I`, float by class `F`, and a
    /// string only from a string (§7.4).
    fn converted(old: &SfallValue, value: &SfallValue) -> Option<SfallValue> {
        match (old, value) {
            (SfallValue::Int(_), value) => value.coerce(Class::I),
            (SfallValue::Float(_), value) => value.coerce(Class::F),
            (SfallValue::Text(_), SfallValue::Text(_)) => Some(value.clone()),
            (SfallValue::Text(_), _) => None,
        }
    }
    /// A script's write. It takes effect at once, and is false when `key`
    /// is undeclared or the value cannot convert.
    fn write(&self, index: usize, key: &[u8], value: &SfallValue) -> bool {
        let mut state = self.state();
        let Ok(key) = std::str::from_utf8(key) else {
            return false;
        };
        let Some(slot) = state.values.get_mut(index).and_then(|map| map.get_mut(key)) else {
            return false;
        };
        let Some(value) = Self::converted(slot, value) else {
            return false;
        };
        if *slot != value {
            *slot = value;
            state.changed.insert(index);
        }
        true
    }
    /// A value from storage or a replay's record, which no commit follows.
    /// False for an undeclared key or a value of another type.
    pub fn restore(&self, index: usize, key: &str, value: SfallValue) -> bool {
        let mut state = self.state();
        let Some(slot) = state.values.get_mut(index).and_then(|map| map.get_mut(key)) else {
            return false;
        };
        if std::mem::discriminant(&*slot) != std::mem::discriminant(&value) {
            return false;
        }
        *slot = value;
        true
    }
    /// Each mod changed since the last call, with the keys it holds that
    /// differ from its manifest (§7.4's commit).
    pub fn take_changed(&self) -> Vec<(String, BTreeMap<String, SfallValue>)> {
        let mut state = self.state();
        let changed = std::mem::take(&mut state.changed);
        if !self.commits {
            return Vec::new();
        }
        changed
            .into_iter()
            .map(|index| {
                let one = &self.mods[index];
                let mut kept = state.values[index].clone();
                kept.retain(|key, value| one.defaults.get(key) != Some(&*value));
                (one.id.clone(), kept)
            })
            .collect()
    }
    /// One line for the log (§7.5).
    fn log(&self, owner: Option<usize>, text: &[u8]) {
        let id = owner.map_or("-", |index| self.mods[index].id.as_str());
        let text: String = text
            .iter()
            .take(LOG_TEXT)
            .map(|byte| match byte {
                32..=126 => char::from(*byte),
                _ => '?',
            })
            .collect();
        let line = format!("{id}: {text}");
        let mut state = self.state();
        if state.log.len() == LOG_LINES {
            state.log.pop_front();
        }
        state.log.push_back(line.clone());
        state.unsent.push(line);
    }
    /// The mod log, oldest first.
    pub fn log_lines(&self) -> Vec<String> {
        self.state().log.iter().cloned().collect()
    }
    /// The lines since the last call, for the debug log.
    pub fn take_unsent(&self) -> Vec<String> {
        std::mem::take(&mut self.state().unsent)
    }
    /// A diagnostic from before a host existed; the next host writes it.
    pub fn note(&self, text: String) {
        self.state().notes.push(text);
    }
    pub fn take_notes(&self) -> Vec<String> {
        std::mem::take(&mut self.state().notes)
    }
}

impl<A: WorldAssets> WorldHost<A> {
    /// Turn manifest scripting on (§2.1), before the campaign and the map
    /// load, under either profile and with sfall mode on or off.
    pub fn enable_mod_scripts(&mut self, session: Arc<ModSession>) {
        for note in session.take_notes() {
            self.world.diagnostic(note);
        }
        self.mod_session = Some(session);
        self.hook_host.get_or_insert_with(Box::default);
        self.assets.set_mod_scripts(true);
        self.sfall_setup_owed();
    }
    /// The lowercased names of every script a manifest declares.
    pub(crate) fn mod_files(&self) -> BTreeSet<Vec<u8>> {
        let Some(session) = &self.mod_session else {
            return BTreeSet::new();
        };
        session
            .mods()
            .iter()
            .flat_map(|one| {
                let globals = one.globals.iter().map(|row| row.file.as_bytes());
                let hooks = one.hooks.iter().map(|row| row.file.as_bytes());
                globals.chain(hooks).map(<[u8]>::to_vec)
            })
            .collect()
    }
    /// §4.4 step 4. Each layer's rows attach first, so a row's place comes
    /// from the manifest, and a script's `start` finds its own rows.
    pub(crate) fn mod_scripts_setup(&mut self) {
        let Some(session) = self.mod_session.clone() else {
            return;
        };
        for (layer, scripted) in session.mods().iter().enumerate() {
            for (order, row) in scripted.hooks.iter().enumerate() {
                self.attach_mod_row(layer, row.hook, order);
                if sfall::hook_row(row.hook).is_some_and(|hook| !hook.fires) {
                    self.world.diagnostic(format!(
                        "{}: hook {} has no boundary; its row runs nothing",
                        scripted.id, row.hook
                    ));
                }
            }
            for row in &scripted.globals {
                self.load_mod_script(layer, scripted, &row.file, Some((row.repeat, row.kind)));
            }
            let mut loaded: BTreeSet<&str> = scripted
                .globals
                .iter()
                .map(|row| row.file.as_str())
                .collect();
            for row in &scripted.hooks {
                if loaded.insert(&row.file) {
                    self.load_mod_script(layer, scripted, &row.file, None);
                }
            }
        }
    }
    /// One program instance of `file`, owned by `scripted`, bound to its
    /// rows before its `start` runs. A refused script leaves them unbound
    /// until the next setup (§4.4).
    fn load_mod_script(
        &mut self,
        layer: usize,
        scripted: &ScriptedMod,
        file: &str,
        global: Option<(i32, i32)>,
    ) {
        let Some(vm) = self.prepare_script(file.as_bytes(), global) else {
            return;
        };
        self.set_mod_owner(vm.id, layer);
        for (order, row) in scripted.hooks.iter().enumerate() {
            if row.file != file {
                continue;
            }
            // §4.2: a name lookup, with no fallback to a first procedure.
            let procedure = vm.program.find_procedure(row.procedure.as_bytes(), true);
            if procedure.is_none() {
                self.world.diagnostic(format!(
                    "{}: {file} has no procedure {}; its row is inert",
                    scripted.id, row.procedure
                ));
            }
            self.bind_mod_row(layer, row.hook, order, vm.id, procedure);
        }
        self.start_script(vm);
    }
    /// A word of §7.3. Operands follow SPEC-024b §4.1, with sfall mode on or
    /// off: a failed one pops them all, and the word pushes its default.
    pub(crate) fn mod_word(&mut self, word: u16, vm: &mut Vm) -> Result<(), VmError> {
        let row = sfall::mod_row(word).ok_or(VmError::Opcode(word))?;
        let operands = sfall::pop_operands(vm, row)?;
        if self.calls.len() < 100_000 {
            self.calls.push(crate::host::CallTrace {
                opcode: word,
                operands: operands.clone(),
            });
        }
        let result = match self.operands(vm, row.name, row.args, &operands) {
            Some(args) => self.mod_effect(vm, row, &args),
            None => self.sfall_failed(row),
        };
        match result {
            Some(value) => value.push(vm),
            None => Ok(()),
        }
    }
    fn mod_effect(
        &mut self,
        vm: &Vm,
        row: &'static SfallRow,
        args: &[SfallValue],
    ) -> Option<SfallValue> {
        let int = |index: usize| args[index].int();
        match row.id {
            0x8300 => Some(SfallValue::Int(MOD_API_VERSION)),
            0x8301 => Some(self.mod_setting(vm, &args[0], &args[1])),
            0x8302 => {
                self.mod_set_setting(vm, &args[0], &args[1]);
                None
            }
            0x8303 => {
                let owner = self.mod_owner(vm.id);
                if let (Some(session), SfallValue::Text(text)) = (&self.mod_session, &args[0]) {
                    session.log(owner, text);
                }
                None
            }
            0x8304 => {
                self.mod_register(vm, int(0), int(1));
                None
            }
            0x8305 => Some(SfallValue::Int(self.current_hook())),
            0x8306 => Some(self.sfall_arg_at(int(0))),
            0x8307 => {
                self.set_sfall_arg(int(0), &args[1]);
                None
            }
            0x8308 => {
                self.set_hook_return_at(int(0), &args[1]);
                None
            }
            0x8309 => Some(SfallValue::Int(self.sfall_mode_bits() as i32)),
            _ => None,
        }
    }
    /// The caller's value of `key`, or `default`, with one diagnostic per
    /// program (§7.4).
    fn mod_setting(&mut self, vm: &Vm, key: &SfallValue, default: &SfallValue) -> SfallValue {
        let owner = self.mod_owner(vm.id);
        let found = match (owner, &self.mod_session, key) {
            (Some(index), Some(session), SfallValue::Text(key)) => session.setting(index, key),
            _ => None,
        };
        found.unwrap_or_else(|| {
            if self.first_warning(vm.id) {
                self.world
                    .diagnostic("mod_get_setting: no such setting; the default stands".to_owned());
            }
            default.clone()
        })
    }
    fn mod_set_setting(&mut self, vm: &Vm, key: &SfallValue, value: &SfallValue) {
        let owner = self.mod_owner(vm.id);
        let written = match (owner, &self.mod_session, key) {
            (Some(index), Some(session), SfallValue::Text(key)) => session.write(index, key, value),
            _ => false,
        };
        if !written {
            self.world
                .diagnostic("mod_set_setting: that setting did not change".to_owned());
        }
    }
}
