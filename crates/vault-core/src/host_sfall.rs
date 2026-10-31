//! SPEC-024b part 1 on the host: sfall mode's words. A word's stack contract
//! comes from the oracle table (`vault_vm::sfall`): the row pops its operands
//! first, and a failed operand then has no effect, writes a diagnostic and
//! pushes the row's default. Unlike SPEC-017, that is not a VM error, because
//! scripts written for sfall expect its functions not to stop them. Tier P2
//! and X rows keep the stack balanced with a stub, and tier R rows fail
//! closed, consuming nothing.
use crate::{
    RandomSource, WorldError,
    assets::WorldAssets,
    host::{HostResult, WorldHost},
    sfall::{GlobalKey, SfallWorld},
    sfall_ini::{IniCache, IniFile, IniFiles, Setting},
    stats::CharacterSheet,
    world::ObjectRef,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use vault_formats::pro::Category;
use vault_vm::{
    ExternalValue, Value, Vm, VmError,
    extensions::{Argument, FALLOUT2, HostCall},
    sfall::{self, Class, SfallRow, SfallValue, Tier},
    sfall_arrays::{ArrayStore, Storage},
};

/// A frame's length to `get_uptime`, in milliseconds (§4.4).
const STEP_MS: i64 = 16;

/// The `get_game_mode` bits the world itself knows (§4.4). The screens' bits
/// come from the shell ([`WorldHost::set_sfall_screens`]).
pub mod game_mode {
    pub const WORLD_MAP: u32 = 0x1;
    pub const LOCAL_MAP: u32 = 0x2;
    pub const DIALOGUE: u32 = 0x4;
    pub const ESCAPE_MENU: u32 = 0x8;
    pub const SAVE: u32 = 0x10;
    pub const LOAD: u32 = 0x20;
    pub const COMBAT: u32 = 0x40;
    pub const OPTIONS: u32 = 0x80;
    pub const HELP: u32 = 0x100;
    pub const CHARACTER: u32 = 0x200;
    pub const PIP_BOY: u32 = 0x400;
    pub const PLAYER_TURN: u32 = 0x800;
    pub const INVENTORY: u32 = 0x1000;
    pub const AUTOMAP: u32 = 0x2000;
    pub const SKILLDEX: u32 = 0x4000;
    pub const USE_ITEM: u32 = 0x8000;
    pub const LOOT: u32 = 0x10000;
    pub const BARTER: u32 = 0x20000;
}

/// `set_self`'s object: for the next word only, and a second consecutive
/// call also reaches the object's script (§4.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SelfOverride {
    program: u32,
    object: ObjectRef,
    strong: bool,
}

pub use crate::host_sfall_hooks::{Preview, SfallHooks};

/// The session's sfall state on the host side: everything scripts rebuild,
/// so none of it is saved or hashed. `None` on the host with the mode off.
#[derive(Default)]
pub struct SfallHost {
    /// Each P2 or X row's one diagnostic per program (§4.10).
    diagnosed: BTreeSet<(u32, &'static str)>,
    self_override: Option<SelfOverride>,
    /// `game_loaded`'s scripts since the last new game or load.
    loaded: BTreeSet<u64>,
    /// Steps since the session started, for `get_uptime`.
    pub(crate) steps: u64,
    /// The screens' `get_game_mode` bits, as the shell last set them.
    screens: u32,
    /// §4.8 the open lists, by ID: each is a snapshot and a cursor.
    lists: BTreeMap<i32, VecDeque<ObjectRef>>,
    next_list: i32,
    ini: IniCache,
    ini_files: Option<Box<dyn IniFiles>>,
    /// Each attacker's last target (§4.8).
    last_targets: BTreeMap<ObjectRef, ObjectRef>,
    /// `get_ini_config`'s map of each file while it lasts (§4.5).
    ini_configs: BTreeMap<String, u32>,
    /// Keys the hook mapped, waiting for the game (metarule3 201 empties it).
    pub(crate) key_queue: VecDeque<i32>,
    /// Part 3: hooks and global scripts.
    pub(crate) hooks: SfallHooks,
}

fn integer(value: i32) -> Option<SfallValue> {
    Some(SfallValue::Int(value))
}

impl<A: WorldAssets> WorldHost<A> {
    /// Whether sfall mode is on. It is fixed when the session is built.
    pub fn sfall_mode(&self) -> bool {
        self.sfall.is_some()
    }
    /// Turn sfall mode on (§3.1). It exists only under the `fallout2`
    /// profile, and asking for it under `fallout1` is refused.
    pub fn enable_sfall(&mut self) -> Result<(), WorldError> {
        if self.assets.game_profile() != crate::GameProfile::Fallout2 {
            return Err(WorldError::Invalid(
                "sfall mode exists only under the fallout2 profile".into(),
            ));
        }
        self.sfall = Some(Box::default());
        self.world.sfall = Some(Box::<SfallWorld>::default());
        self.assets.set_sfall_scripts(true);
        self.sfall_setup_owed();
        Ok(())
    }
    /// Where `set_ini_setting` writes, and where a file no VFS layer holds
    /// is read from: the profile's writable folder (§4.5).
    pub fn set_sfall_ini_files(&mut self, files: Box<dyn IniFiles>) {
        if let Some(sfall) = self.sfall.as_deref_mut() {
            sfall.ini_files = Some(files);
        }
    }
    /// A new world state came in from a load: what scripts rebuild goes.
    pub(crate) fn sfall_restored(&mut self) {
        if let Some(sfall) = self.sfall.as_deref_mut() {
            sfall.loaded.clear();
            sfall.lists.clear();
            sfall.ini_configs.clear();
            sfall.last_targets.clear();
        }
        self.sfall_setup_owed();
    }
    /// The screens' `get_game_mode` bits, from the shell.
    pub fn set_sfall_screens(&mut self, bits: u32) {
        if let Some(sfall) = self.sfall.as_deref_mut() {
            sfall.screens = bits;
        }
    }
    /// The object `set_self` put in place for `vm`'s next word.
    pub(crate) fn sfall_self(&self, vm: &Vm) -> Option<ObjectRef> {
        self.sfall
            .as_deref()?
            .self_override
            .filter(|set| set.program == vm.id)
            .map(|set| set.object)
    }
    /// The script a second consecutive `set_self` reaches.
    pub(crate) fn sfall_script(&self, vm: &Vm) -> Option<u32> {
        let set = self
            .sfall
            .as_deref()?
            .self_override
            .filter(|set| set.program == vm.id && set.strong)?;
        u32::try_from(self.world.get(set.object)?.raw.script_id)
            .ok()
            .filter(|sid| self.world.scripts.contains_key(sid))
    }
    /// One host word has finished: `set_self`'s object returns to normal
    /// after the next word.
    pub(crate) fn sfall_word_done(&mut self, vm: &Vm, word: u16) {
        if word == 0x8206 {
            return;
        }
        if let Some(sfall) = self.sfall.as_deref_mut()
            && sfall.self_override.is_some_and(|set| set.program == vm.id)
        {
            sfall.self_override = None;
        }
    }
    fn set_self(&mut self, vm: &Vm, object: ObjectRef) {
        let Some(sfall) = self.sfall.as_deref_mut() else {
            return;
        };
        let held = sfall.self_override.filter(|set| set.program == vm.id);
        sfall.self_override = (object != 0).then_some(SelfOverride {
            program: vm.id,
            object,
            strong: held.is_some(),
        });
    }
    /// Once per program and row: a stub's diagnostic (§4.10).
    fn stub_diagnostic(&mut self, vm: &Vm, name: &'static str) {
        let first = self
            .sfall
            .as_deref_mut()
            .is_some_and(|sfall| sfall.diagnosed.insert((vm.id, name)));
        if first {
            self.world
                .diagnostic(format!("{name}: sfall stub, no effect in this revision"));
        }
    }
    /// A sfall word (§3.2): `0x8156..=0x8281`, with sfall mode on.
    pub(crate) fn sfall_word(&mut self, word: u16, vm: &mut Vm) -> Result<(), VmError> {
        let row = sfall::word_row(word).ok_or(VmError::Opcode(word))?;
        if row.tier == Tier::R {
            return Err(VmError::Opcode(word));
        }
        let operands = sfall::pop_operands(vm, row)?;
        if self.calls.len() < 100_000 {
            self.calls.push(crate::host::CallTrace {
                opcode: word,
                operands: operands.clone(),
            });
        }
        let result = if row.tier == Tier::P1 {
            self.sfall_p1(vm, row, &operands)?
        } else {
            self.stub_diagnostic(vm, row.name);
            sfall::stub_default(row)
        };
        match result {
            Some(value) => value.push(vm),
            None => Ok(()),
        }
    }
    /// `operands` as `classes` says, or `None` for a failed operand. An `O`
    /// operand that is not a live object means null (SPEC-017 §1).
    fn operands(
        &mut self,
        vm: &Vm,
        name: &str,
        classes: &[Class],
        operands: &[Value],
    ) -> Option<Vec<SfallValue>> {
        let mut values = Vec::with_capacity(operands.len());
        for (class, operand) in classes.iter().zip(operands) {
            let mut value = SfallValue::read(vm, *operand)?.coerce(*class)?;
            if *class == Class::O
                && value.int() != 0
                && self.world.get(value.int() as u32).is_none()
            {
                self.world.diagnostic(format!(
                    "{name}: reference {:#x} names no live object; treated as null",
                    value.int()
                ));
                value = SfallValue::Int(0);
            }
            values.push(value);
        }
        Some(values)
    }
    fn failed(&mut self, row: &SfallRow) -> Option<SfallValue> {
        self.world
            .diagnostic(format!("{}: failed operand", row.name));
        sfall::failed_default(row)
    }
    /// A P1 word, with its operands popped.
    fn sfall_p1(
        &mut self,
        vm: &mut Vm,
        row: &'static SfallRow,
        operands: &[Value],
    ) -> Result<Option<SfallValue>, VmError> {
        if (0x8276..=0x827c).contains(&row.id) || matches!(row.id, 0x8280 | 0x8281) {
            return self.sfall_function(vm, row, operands);
        }
        let Some(args) = self.operands(vm, row.name, row.args, operands) else {
            return Ok(self.failed(row));
        };
        let int = |index: usize| args[index].int();
        let number = |index: usize| args[index].number();
        let object = |index: usize| args[index].int() as u32;
        let player = self.world.player;
        let bad_stat = |host: &mut Self, stat: i32, size: usize| {
            let bad = usize::try_from(stat).is_ok_and(|stat| stat < size);
            if !bad {
                host.world
                    .diagnostic(format!("{}: {stat} is out of range", row.name));
            }
            !bad
        };
        Ok(match row.id {
            0x815a | 0x815b | 0x815e | 0x815f => {
                let (critter, stat, value) = match row.id {
                    0x815a | 0x815b => (player, int(0), int(1)),
                    _ => (object(0), int(1), int(2)),
                };
                if !bad_stat(self, stat, 35)
                    && let Some(sheet) = self.sheet_mut(critter)
                {
                    let stats = if row.id & 1 == 0 {
                        &mut sheet.base
                    } else {
                        &mut sheet.bonus
                    };
                    stats[stat as usize] = value;
                }
                None
            }
            0x815c | 0x815d | 0x8160 | 0x8161 => {
                let (critter, stat) = match row.id {
                    0x815c | 0x815d => (player, int(0)),
                    _ => (object(0), int(1)),
                };
                if bad_stat(self, stat, 35) {
                    return Ok(integer(0));
                }
                let bonus = matches!(row.id, 0x815d | 0x8161);
                integer(self.sheet(critter).map_or(0, |sheet| {
                    if bonus {
                        sheet.bonus[stat as usize]
                    } else {
                        sheet.base[stat as usize]
                    }
                }))
            }
            0x8163 => integer(i32::from(self.world.game_date()[2])),
            0x8164 => {
                let key = self.sid(vm).map_or(u64::from(vm.id), |sid| {
                    (u64::from(sid) << 32) | u64::from(vm.id)
                });
                let first = self
                    .sfall
                    .as_deref_mut()
                    .is_some_and(|sfall| sfall.loaded.insert(key));
                integer(i32::from(first))
            }
            0x8170 => integer(i32::from(
                self.sfall_game_mode() & game_mode::WORLD_MAP != 0,
            )),
            0x818c => integer(self.kill_count(int(0)).map_or(0, |count| *count)),
            0x818d => {
                let amount = int(1);
                match self.kill_count(int(0)) {
                    Some(count) => *count = count.saturating_add(amount),
                    None => self.world.diagnostic(format!(
                        "mod_kill_counter: category {} is out of range",
                        int(0)
                    )),
                }
                None
            }
            0x8191 => integer(self.world.action_points(object(0))),
            0x8192 => {
                self.world.set_ap(object(0), int(1));
                None
            }
            0x8193 => integer(self.world.active_hand),
            0x8194 => {
                self.swap_hands();
                None
            }
            0x819d => {
                let Some(key) = GlobalKey::of(&args[0]) else {
                    return Ok(self.failed(row));
                };
                if let Some(state) = self.world.sfall.as_deref_mut() {
                    state.globals.insert(key, args[1].clone());
                }
                None
            }
            0x819e | 0x819f => {
                let Some(key) = GlobalKey::of(&args[0]) else {
                    return Ok(self.failed(row));
                };
                let value = self
                    .world
                    .sfall
                    .as_deref()
                    .and_then(|state| state.globals.get(&key))
                    .cloned();
                Some(if row.id == 0x819e {
                    SfallValue::Int(value.map_or(0, |value| value.int()))
                } else {
                    SfallValue::float(value.map_or(0.0, |value| value.number()))
                })
            }
            0x81ac => {
                let value = self.ini_value(args[0].bytes());
                integer(match value {
                    Some(Some(value)) => sfall::atoi(value.as_bytes()),
                    _ => -1,
                })
            }
            0x81af => integer(self.sfall_game_mode() as i32),
            0x81b3 => {
                let steps = self.sfall.as_deref().map_or(0, |sfall| sfall.steps);
                integer(
                    (steps as i64)
                        .saturating_mul(STEP_MS)
                        .min(i64::from(i32::MAX)) as i32,
                )
            }
            0x81eb => match self.ini_value(args[0].bytes()) {
                None => integer(-1),
                Some(value) => Some(SfallValue::text(value.unwrap_or_default())),
            },
            0x81ec => Some(SfallValue::float(sfall::square_root(number(0)))),
            0x81ed => Some(match &args[0] {
                SfallValue::Int(value) => SfallValue::Int(value.wrapping_abs()),
                value => SfallValue::float(value.number().abs()),
            }),
            0x81ee => Some(SfallValue::float(sfall::sine(number(0)))),
            0x81ef => Some(SfallValue::float(sfall::cosine(number(0)))),
            0x81f0 => Some(SfallValue::float(sfall::tangent(number(0)))),
            0x81f1 => Some(SfallValue::float(sfall::arctan(number(0), number(1)))),
            0x8204 => integer(self.sfall_proto_get(int(0), int(1))),
            0x8205 => {
                self.sfall_proto_set(int(0), int(1), int(2));
                None
            }
            0x8206 => {
                self.set_self(vm, object(0));
                None
            }
            0x820d => integer(self.list_begin(int(0))),
            0x820e => integer(self.list_next(int(0))),
            0x820f => {
                if let Some(sfall) = self.sfall.as_deref_mut() {
                    sfall.lists.remove(&int(0));
                }
                None
            }
            0x8210..=0x8212 => integer(sfall::VERSION[usize::from(row.id - 0x8210)]),
            0x8217 | 0x8219 => {
                let loaded =
                    self.sfall_weapon(object(0)).map(
                        |(rounds, pid)| {
                            if row.id == 0x8217 { *pid } else { *rounds }
                        },
                    );
                integer(loaded.unwrap_or(0))
            }
            0x8218 | 0x821a => {
                let value = int(1);
                if let Some((rounds, pid)) = self.sfall_weapon(object(0)) {
                    *(if row.id == 0x8218 { pid } else { rounds }) = value;
                }
                None
            }
            0x822d | 0x8233 => {
                let storage = if row.id == 0x822d {
                    Storage::Permanent
                } else {
                    Storage::Temporary
                };
                integer(self.arrays().create(int(0), int(1), storage))
            }
            0x822e => {
                self.arrays().set(int(0), args[1].clone(), args[2].clone());
                None
            }
            0x822f => Some(self.arrays().get(int(0), &args[1])),
            0x8230 => {
                self.arrays().free(int(0));
                None
            }
            0x8231 => integer(self.arrays().len(int(0))),
            0x8232 => {
                self.resize_array(int(0), int(1))?;
                None
            }
            0x8234 => {
                self.arrays().fix(int(0));
                None
            }
            0x8235 => {
                let pieces = sfall::split(args[0].bytes(), args[1].bytes())
                    .into_iter()
                    .map(SfallValue::Text)
                    .collect();
                integer(self.arrays().temp_list(pieces))
            }
            0x8236 => {
                let objects = self
                    .sfall_list(int(0))
                    .into_iter()
                    .map(|object| SfallValue::Int(object as i32))
                    .collect();
                integer(self.arrays().temp_list(objects))
            }
            0x8237 => integer(sfall::atoi(args[0].bytes())),
            0x8238 => Some(SfallValue::float(sfall::atof(args[0].bytes()))),
            0x8239 => Some(self.arrays().scan(int(0), &args[1])),
            0x823b => integer(self.modified_ini()),
            0x8242 => {
                let (skill, value) = (int(1), int(2));
                if !bad_stat(self, skill, 18)
                    && let Some(sheet) = self.sheet_mut(object(0))
                {
                    sheet.invested[skill as usize] = value;
                }
                None
            }
            0x8243 => {
                if bad_stat(self, int(1), 18) {
                    return Ok(integer(0));
                }
                integer(
                    self.sheet(object(0))
                        .map_or(0, |sheet| sheet.invested[int(1) as usize]),
                )
            }
            0x8248 => integer(self.last_target(object(0)) as i32),
            0x8249 => integer(self.world.get(object(0)).map_or(0, |o| o.opponent) as i32),
            0x824e => Some(SfallValue::text(sfall::substr(
                args[0].bytes(),
                int(1),
                int(2),
            ))),
            0x824f => integer(args[0].bytes().len() as i32),
            0x8250 => match sfall::format(args[0].bytes(), &args[1..]) {
                Some(text) => Some(SfallValue::Text(text)),
                None => self.failed(row),
            },
            0x8251 => integer(args[0].bytes().first().map_or(0, |byte| i32::from(*byte))),
            0x8253 => integer(args[0].type_code()),
            0x8254 => {
                self.arrays().save(args[0].clone(), int(1));
                None
            }
            0x8255 => integer(self.arrays().load(&args[0])),
            0x8256 => Some(self.arrays().key_at(int(0), int(1))),
            0x8257 => {
                self.arrays().expression(args[0].clone(), args[1].clone());
                integer(0)
            }
            0x825e..=0x8260 => {
                self.sfall_registration(row.id, &args);
                None
            }
            0x8263 => Some(sfall::power(&args[0], &args[1])),
            0x8264 => Some(SfallValue::float(sfall::logarithm(number(0)))),
            0x8265 => Some(SfallValue::float(sfall::exponent(number(0)))),
            0x8266 => integer(sfall::ceil(number(0))),
            0x8267 => integer(sfall::round(number(0))),
            0x827f => Some(sfall::divide(&args[0], &args[1])?),
            _ => self.sfall_special(vm, row, &args)?,
        })
    }
    /// The words of parts 2 and 3 and the global-script words.
    fn sfall_special(
        &mut self,
        vm: &mut Vm,
        row: &'static SfallRow,
        args: &[SfallValue],
    ) -> Result<Option<SfallValue>, VmError> {
        let int = |index: usize| args[index].int();
        Ok(match row.id {
            0x816a => {
                self.set_global_repeat(vm, int(0));
                None
            }
            0x819b => {
                self.set_global_kind(vm, int(0));
                None
            }
            0x819c => integer(15),
            0x81e4 => Some(self.next_sfall_arg()),
            0x81e5 => {
                self.set_sfall_return(&args[0]);
                None
            }
            0x81ea => integer(self.init_hook()),
            0x8207 => {
                self.register_hook(vm, int(0), None, false);
                None
            }
            0x8262 | 0x827d => {
                self.register_hook(vm, int(0), Some(int(1)), row.id == 0x827d);
                None
            }
            0x823c => {
                let values = self.sfall_args();
                integer(self.arrays().temp_list(values))
            }
            0x823d => {
                self.set_sfall_arg(int(0), int(1));
                None
            }
            _ => {
                self.stub_diagnostic(vm, row.name);
                sfall::stub_default(row)
            }
        })
    }
    /// `sfall_func0..8` (§4.2): the name, then its arguments. Every call
    /// pushes exactly one value.
    fn sfall_function(
        &mut self,
        vm: &mut Vm,
        row: &'static SfallRow,
        operands: &[Value],
    ) -> Result<Option<SfallValue>, VmError> {
        let Some(SfallValue::Text(name)) = SfallValue::read(vm, operands[0]) else {
            self.world
                .diagnostic(format!("{}: the name is not a string", row.name));
            return Ok(integer(0));
        };
        let shown = String::from_utf8_lossy(&name).into_owned();
        match sfall::function_tier(&name) {
            None => {
                self.world
                    .diagnostic(format!("{}: unknown function {shown:?}", row.name));
                Ok(integer(0))
            }
            Some(Tier::P1) => {
                let mut args = Vec::with_capacity(operands.len() - 1);
                for operand in &operands[1..] {
                    match SfallValue::read(vm, *operand) {
                        Some(value) => args.push(value),
                        None => {
                            self.world.diagnostic(format!("{shown}: failed operand"));
                            return Ok(integer(0));
                        }
                    }
                }
                let result = self.sfall_named(vm, &shown, &args)?;
                Ok(Some(match result {
                    Some(value) => value,
                    None => SfallValue::Int(0),
                }))
            }
            Some(_) => {
                let row = sfall::SFALL_FUNCTIONS
                    .iter()
                    .find(|row| row.name.as_bytes() == name)
                    .expect("a known name has a row");
                self.stub_diagnostic(vm, row.name);
                Ok(integer(0))
            }
        }
    }
    /// A P1 function, by name. A missing argument takes its default, and a
    /// wrong type is a failed operand, which gives int 0.
    fn sfall_named(
        &mut self,
        vm: &mut Vm,
        name: &str,
        args: &[SfallValue],
    ) -> Result<Option<SfallValue>, VmError> {
        let int = |index: usize, class: Class| match args.get(index) {
            None => Some(0),
            Some(value) => value.coerce(class).map(|value| value.int()),
        };
        let text = |index: usize| match args.get(index) {
            Some(SfallValue::Text(text)) => Some(text.as_slice()),
            _ => None,
        };
        let failed = |host: &mut Self| {
            host.world.diagnostic(format!("{name}: failed operand"));
            Ok(integer(0))
        };
        let live = |host: &mut Self, index: usize| -> Option<ObjectRef> {
            let object = int(index, Class::O)? as u32;
            if object != 0 && host.world.get(object).is_none() {
                host.world.diagnostic(format!(
                    "{name}: reference {object:#x} names no live object; treated as null"
                ));
                return Some(0);
            }
            Some(object)
        };
        match name {
            "metarule_exist" => {
                let Some(asked) = text(0) else {
                    return failed(self);
                };
                Ok(integer(i32::from(
                    sfall::function_tier(asked) == Some(Tier::P1),
                )))
            }
            "opcode_exists" => {
                let Some(word) = int(0, Class::I) else {
                    return failed(self);
                };
                Ok(integer(i32::from(sfall::p1_word(word))))
            }
            "string_find" => {
                let (Some(haystack), Some(needle), Some(position)) =
                    (text(0), text(1), int(2, Class::I))
                else {
                    return failed(self);
                };
                Ok(integer(sfall::find(haystack, needle, position)))
            }
            "string_format" => {
                let Some(format) = text(0).filter(|text| text.len() <= sfall::FORMAT_LIMIT) else {
                    return failed(self);
                };
                match sfall::format(format, &args[1..]) {
                    Some(text) => Ok(Some(SfallValue::Text(text))),
                    None => failed(self),
                }
            }
            "string_to_case" => {
                let (Some(value), Some(mode)) = (text(0), int(1, Class::I)) else {
                    return failed(self);
                };
                Ok(Some(SfallValue::Text(sfall::to_case(value, mode))))
            }
            "string_compare" => {
                let (Some(a), Some(b)) = (text(0), text(1)) else {
                    return failed(self);
                };
                Ok(integer(i32::from(sfall::compare(a, b))))
            }
            "floor2" => match args.first().and_then(|value| value.coerce(Class::N)) {
                Some(SfallValue::Int(value)) => Ok(integer(value)),
                Some(value) => Ok(integer(sfall::floor(value.number()))),
                None => failed(self),
            },
            "get_flags" => {
                let Some(object) = live(self, 0) else {
                    return failed(self);
                };
                Ok(integer(self.sfall_object_get_or_zero(object, 0x24)))
            }
            "set_flags" => {
                let (Some(object), Some(flags)) = (live(self, 0), int(1, Class::I)) else {
                    return failed(self);
                };
                if object != 0 {
                    self.sfall_object_set(object, 0x24, flags);
                }
                Ok(None)
            }
            "get_object_data" => {
                let (Some(object), Some(offset)) = (live(self, 0), int(1, Class::I)) else {
                    return failed(self);
                };
                Ok(integer(self.sfall_object_get_or_zero(object, offset)))
            }
            "set_object_data" => {
                let (Some(object), Some(offset), Some(value)) =
                    (live(self, 0), int(1, Class::I), int(2, Class::I))
                else {
                    return failed(self);
                };
                if object != 0 {
                    self.sfall_object_set(object, offset, value);
                }
                Ok(None)
            }
            "critter_inven_obj2" => {
                let (Some(critter), Some(selector)) = (live(self, 0), int(1, Class::I)) else {
                    return failed(self);
                };
                Ok(integer(self.critter_inventory_object(critter, selector)))
            }
            "set_ini_setting" => {
                let (Some(setting), Some(value)) = (text(0), args.get(1)) else {
                    return failed(self);
                };
                let value = match value {
                    SfallValue::Int(value) => value.to_string(),
                    SfallValue::Text(text) => String::from_utf8_lossy(text).into_owned(),
                    SfallValue::Float(_) => return failed(self),
                };
                let Some(setting) = Setting::parse(setting) else {
                    return failed(self);
                };
                self.write_ini(&setting, &value);
                Ok(None)
            }
            _ => self.sfall_array_function(vm, name, args),
        }
    }
    /// The functions of part 2 that make arrays.
    fn sfall_array_function(
        &mut self,
        _vm: &mut Vm,
        name: &str,
        args: &[SfallValue],
    ) -> Result<Option<SfallValue>, VmError> {
        let file = args
            .first()
            .and_then(|file| crate::sfall_ini::valid_file(file.bytes()));
        match name {
            "get_metarule_table" => {
                let names = sfall::p1_functions()
                    .into_iter()
                    .map(SfallValue::text)
                    .collect();
                Ok(integer(self.arrays().temp_list(names)))
            }
            "get_ini_section" => {
                let section = args.get(1).map_or(&[][..], SfallValue::bytes);
                let section = String::from_utf8_lossy(section).into_owned();
                let pairs: Vec<(SfallValue, SfallValue)> = file
                    .and_then(|file| {
                        let section = self.ini_file(&file)?.section(&section)?;
                        Some(
                            section
                                .pairs
                                .iter()
                                .map(|(key, value)| {
                                    (
                                        SfallValue::text(key.as_bytes()),
                                        SfallValue::text(value.as_bytes()),
                                    )
                                })
                                .collect(),
                        )
                    })
                    .unwrap_or_default();
                Ok(integer(self.arrays().map_of(pairs, Storage::Temporary)))
            }
            "get_ini_sections" => {
                let names: Vec<SfallValue> = file
                    .and_then(|file| {
                        let file = self.ini_file(&file)?;
                        Some(
                            file.sections
                                .iter()
                                .map(|section| SfallValue::text(section.name.as_bytes()))
                                .collect(),
                        )
                    })
                    .unwrap_or_default();
                Ok(integer(self.arrays().temp_list(names)))
            }
            "get_sfall_arg_at" => {
                let index = args.first().and_then(|value| value.coerce(Class::I));
                Ok(match index {
                    Some(index) => Some(self.sfall_arg_at(index.int())),
                    None => integer(0),
                })
            }
            "get_ini_config" => match file {
                Some(file) => Ok(integer(self.ini_config(&file))),
                None => Ok(integer(0)),
            },
            _ => Ok(None),
        }
    }
    /// `get_ini_config`: a permanent map of section name to permanent
    /// sub-map, the same array on a repeated call unless it was freed.
    fn ini_config(&mut self, name: &str) -> i32 {
        let held = self
            .sfall
            .as_deref()
            .and_then(|sfall| sfall.ini_configs.get(name).copied());
        if let Some(id) = held.filter(|id| self.arrays().exists(*id)) {
            return id as i32;
        }
        let sections: Vec<(String, Vec<(String, String)>)> = self
            .ini_file(name)
            .map(|file| {
                file.sections
                    .iter()
                    .map(|section| (section.name.clone(), section.pairs.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let store = self.arrays();
        let pairs = sections
            .into_iter()
            .map(|(section, pairs)| {
                let pairs = pairs
                    .into_iter()
                    .map(|(key, value)| {
                        (
                            SfallValue::text(key.into_bytes()),
                            SfallValue::text(value.into_bytes()),
                        )
                    })
                    .collect();
                let map = store.map_of(pairs, Storage::Permanent);
                (SfallValue::text(section.into_bytes()), SfallValue::Int(map))
            })
            .collect();
        let id = store.map_of(pairs, Storage::Permanent);
        if let Some(sfall) = self.sfall.as_deref_mut() {
            sfall.ini_configs.insert(name.to_owned(), id as u32);
        }
        id
    }
    /// The store: it exists with the mode on.
    fn arrays(&mut self) -> &mut ArrayStore {
        &mut self.world.sfall.get_or_insert_default().arrays
    }
    /// `resize_array`, whose shuffle draws from the gameplay generator
    /// (§5.6).
    fn resize_array(&mut self, id: i32, size: i32) -> Result<(), VmError> {
        let world = &mut self.world;
        let store = &mut world.sfall.get_or_insert_default().arrays;
        let rng = &mut world.rng;
        let known = store
            .resize(id, size, |last| rng.range(0, last))
            .map_err(|error| VmError::Host(error.to_string()))?;
        if !known {
            world.diagnostic(format!("resize_array: size {size} names no operation"));
        }
        Ok(())
    }
    fn sfall_object_get_or_zero(&mut self, object: ObjectRef, offset: i32) -> i32 {
        if object == 0 {
            0
        } else {
            self.sfall_object_get(object, offset)
        }
    }
    /// The player's count for `category`, over SPEC-018 §7.3's 0-18.
    fn kill_count(&mut self, category: i32) -> Option<&mut i32> {
        let category = usize::try_from(category).ok()?;
        match category {
            0..=14 => self.world.kills.get_mut(category),
            _ => self
                .world
                .fallout2
                .as_deref_mut()?
                .kills
                .get_mut(category - 15),
        }
    }
    fn sheet(&self, critter: ObjectRef) -> Option<&CharacterSheet> {
        self.world.get(critter)?.character.as_ref()
    }
    fn sheet_mut(&mut self, critter: ObjectRef) -> Option<&mut CharacterSheet> {
        self.world.get_mut(critter)?.character.as_mut()
    }
    /// The world's bits (§4.4), and the shell's for the screens.
    pub(crate) fn sfall_mode_bits(&self) -> u32 {
        self.sfall_game_mode()
    }
    fn sfall_game_mode(&self) -> u32 {
        let mut bits = self.sfall.as_deref().map_or(0, |sfall| sfall.screens);
        if bits & game_mode::WORLD_MAP == 0 {
            bits |= game_mode::LOCAL_MAP;
        }
        if self.world.dialogue.open {
            bits |= game_mode::DIALOGUE;
        }
        if self.world.combat {
            bits |= game_mode::COMBAT;
            if self.world.combat_active(self.world.player) && !self.player_turn_over() {
                bits |= game_mode::PLAYER_TURN;
            }
        }
        bits
    }
    /// SPEC-005 `critter_inven_obj` without its hand gate: selector 0 worn,
    /// 1 right hand, 2 left hand, -2 the entry count.
    fn critter_inventory_object(&mut self, critter: ObjectRef, selector: i32) -> i32 {
        let Some(object) = self.world.get(critter) else {
            return 0;
        };
        if object.category() != Category::Critter {
            return 0;
        }
        if selector == -2 {
            return object.inventory.len() as i32;
        }
        let flag = match selector {
            0 => 0x0400_0000,
            1 => 0x0200_0000,
            2 => 0x0100_0000,
            _ => return 0,
        };
        object
            .inventory
            .iter()
            .find(|(item, _)| {
                self.world
                    .get(*item)
                    .is_some_and(|object| object.raw.flags.0 & flag != 0)
            })
            .map_or(0, |(item, _)| *item as i32)
    }
    fn last_target(&self, critter: ObjectRef) -> ObjectRef {
        self.sfall
            .as_deref()
            .and_then(|sfall| sfall.last_targets.get(&critter).copied())
            .filter(|target| self.world.get(*target).is_some())
            .unwrap_or(0)
    }
    /// A critter that attacked `target` just now, for `get_last_target`.
    pub(crate) fn note_target(&mut self, attacker: ObjectRef, target: ObjectRef) {
        if let Some(sfall) = self.sfall.as_deref_mut() {
            sfall.last_targets.insert(attacker, target);
        }
    }
    /// SPEC-017 §4.1's registration words at their sfall numbers.
    fn sfall_registration(&mut self, word: u16, args: &[SfallValue]) {
        let spec = FALLOUT2
            .iter()
            .find(|spec| spec.word == word - 0x825e + 0x8157)
            .expect("SPEC-017 §4.1 has the row");
        // A SPEC-017 row is in pop order.
        let args = args
            .iter()
            .rev()
            .map(|value| Argument {
                value: Value::integer(value.int()),
                text: None,
                channel: None,
            })
            .collect();
        self.fallout2_registration(&HostCall { spec, args });
    }
    /// The objects of list type `kind`, in SPEC-021's entity order (§4.8).
    pub(crate) fn sfall_list(&mut self, kind: i32) -> Vec<ObjectRef> {
        if !matches!(kind, 0..=3 | 5 | 6 | 9) {
            self.world
                .diagnostic(format!("list_begin: type {kind} is not defined"));
            return Vec::new();
        }
        let mut objects: Vec<ObjectRef> = self
            .world
            .iter()
            .filter(|(_, object)| {
                let placed = object.owner.is_none() && object.raw.hex.is_some();
                let category = object.category();
                placed
                    && match kind {
                        0 => category == Category::Critter,
                        1 => category == Category::Item,
                        2 => category == Category::Scenery,
                        3 => category == Category::Wall,
                        5 => category == Category::Misc,
                        // Spatial scripts are no objects here.
                        6 => false,
                        _ => true,
                    }
            })
            .map(|(reference, _)| reference)
            .collect();
        objects.sort_by_key(|object| self.world.state_handle(*object));
        objects
    }
    fn list_begin(&mut self, kind: i32) -> i32 {
        let objects = self.sfall_list(kind);
        let Some(sfall) = self.sfall.as_deref_mut() else {
            return 0;
        };
        sfall.next_list += 1;
        sfall.lists.insert(sfall.next_list, objects.into());
        sfall.next_list
    }
    fn list_next(&mut self, list: i32) -> i32 {
        self.sfall
            .as_deref_mut()
            .and_then(|sfall| sfall.lists.get_mut(&list))
            .and_then(VecDeque::pop_front)
            .map_or(0, |object| object as i32)
    }
    /// The file `name`, read once for the session: the VFS layers, then the
    /// writable folder.
    fn ini_file(&mut self, name: &str) -> Option<&IniFile> {
        let sfall = self.sfall.as_deref_mut()?;
        if !sfall.ini.contains_key(name) {
            let bytes = self.assets.resource(name).ok().or_else(|| {
                sfall
                    .ini_files
                    .as_deref()
                    .and_then(|files| files.read(name))
            });
            sfall
                .ini
                .insert(name.to_owned(), bytes.map(|bytes| IniFile::parse(&bytes)));
        }
        sfall.ini.get(name)?.as_ref()
    }
    /// A setting's value: `None` for a malformed setting, `Some(None)` for a
    /// file or key that is absent.
    fn ini_value(&mut self, setting: &[u8]) -> Option<Option<String>> {
        let setting = Setting::parse(setting)?;
        Some(
            self.ini_file(&setting.file)
                .and_then(|file| file.get(&setting.section, &setting.key))
                .map(str::to_owned),
        )
    }
    /// `ModifiedIni`, in whichever section of `ddraw.ini` holds it.
    fn modified_ini(&mut self) -> i32 {
        self.ini_file("ddraw.ini")
            .and_then(|file| {
                file.sections
                    .iter()
                    .find_map(|section| section.get("ModifiedIni"))
            })
            .map_or(0, |value| sfall::atoi(value.as_bytes()))
    }
    /// `set_ini_setting`: the cache, then the writable folder, never an
    /// archive. A missing file is created.
    fn write_ini(&mut self, setting: &Setting, value: &str) {
        self.ini_file(&setting.file);
        let Some(sfall) = self.sfall.as_deref_mut() else {
            return;
        };
        let file = sfall
            .ini
            .entry(setting.file.clone())
            .or_default()
            .get_or_insert_with(IniFile::default);
        file.set(&setting.section, &setting.key, value);
        let text = file.write();
        let written = sfall
            .ini_files
            .as_deref_mut()
            .map(|files| files.write(&setting.file, &text));
        if let Some(Err(error)) = written {
            self.world
                .diagnostic(format!("set_ini_setting: {}: {error}", setting.file));
        }
    }
    /// `metarule3`'s sfall sub-functions (§4.3), or `None` for another.
    pub(crate) fn sfall_metarule3(&mut self, vm: &Vm, sub: i32) -> Option<HostResult> {
        let row = sfall::SFALL_METARULE3
            .iter()
            .find(|row| i32::from(row.id) == sub)?;
        if row.tier == Tier::P1 {
            if let Some(sfall) = self.sfall.as_deref_mut() {
                sfall.key_queue.clear();
            }
        } else {
            self.stub_diagnostic(vm, row.name);
        }
        Some(Ok(Some(ExternalValue::Value(Value::integer(0)))))
    }
    /// The end of a headless step's Simulation phase (SPEC-021 §3.10): a
    /// shell calls it once a frame, whether the world ticked or not.
    pub fn sfall_end_of_step(&mut self) {
        self.sfall_setup_if_due();
        self.sfall_repeat_globals();
        if let Some(sfall) = self.sfall.as_deref_mut() {
            sfall.steps = sfall.steps.saturating_add(1);
            sfall.lists.clear();
        }
        if let Some(state) = self.world.sfall.as_deref_mut() {
            state.arrays.end_of_step();
        }
    }
}
