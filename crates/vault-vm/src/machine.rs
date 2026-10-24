use crate::program::{self, instruction_length, valid_word};
use crate::{
    ExportedProcedure, ExternalValue, IntProgram, LaunchMode, Lifecycle, Stack, Value, VmError,
    VmHost,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VmStatus(pub u16);
impl VmStatus {
    pub const ENDED: u16 = 0x1;
    pub const ERROR: u16 = 0x4;
    pub const STOPPED: u16 = 0x8;
    pub const WAITING: u16 = 0x10;
    pub const ATTACHED: u16 = 0x20;
    pub const YIELD: u16 = 0x40;
    pub const CRITICAL: u16 = 0x80;
    pub const SPAWN_BLOCKED: u16 = 0x100;
    pub fn contains(self, flag: u16) -> bool {
        self.0 & flag != 0
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    Ended,
    Stopped,
    Blocked,
    Waiting,
    Yielded,
    Budget,
    FragmentEnd,
}

#[derive(Debug, Clone)]
struct Fragment {
    bytes: Vec<u8>,
    boundaries: Vec<bool>,
}
#[derive(Debug, Clone)]
pub struct Vm {
    pub id: u32,
    pub program: IntProgram,
    pub value_stack: Stack,
    pub return_stack: Stack,
    pub pc: u32,
    pub f: i32,
    pub g: i32,
    pub status: VmStatus,
    pub window: i32,
    pub wait_predicate: u32,
    pub wait_start: u32,
    pub wait_deadline: u32,
    pub parent: Option<u32>,
    pub child: Option<u32>,
    pub hard_instruction_limit: u32,
    pub last_error: Option<VmError>,
    fragment: Option<Fragment>,
    strings: BTreeMap<u32, Vec<u8>>,
    interned: BTreeMap<Vec<u8>, u32>,
    string_bytes: u32,
}
impl Vm {
    pub fn new(program: IntProgram) -> Self {
        Self {
            id: 1,
            program,
            value_stack: Stack::default(),
            return_stack: Stack::default(),
            pc: 0,
            f: -1,
            g: -1,
            status: VmStatus::default(),
            window: -1,
            wait_predicate: 0,
            wait_start: 0,
            wait_deadline: 0,
            parent: None,
            child: None,
            hard_instruction_limit: 100_000,
            last_error: None,
            fragment: None,
            strings: BTreeMap::new(),
            interned: BTreeMap::new(),
            string_bytes: 0,
        }
    }
    /// Isolated instruction harness, retaining the supplied program's metadata and
    /// literal tables. Reaching the fragment end is an explicit successful boundary.
    pub fn fragment(program: IntProgram, bytes: &[u8]) -> Result<Self, VmError> {
        if bytes.len() > program::MAX_PROGRAM_BYTES {
            return Err(VmError::InstructionLimit);
        }
        let mut boundaries = vec![false; bytes.len()];
        let mut at = 0;
        while at < bytes.len() {
            boundaries[at] = true;
            if bytes.len() - at < 2 {
                break;
            }
            at += instruction_length(u16::from_be_bytes([bytes[at], bytes[at + 1]]));
        }
        let mut vm = Self::new(program);
        vm.fragment = Some(Fragment {
            bytes: bytes.to_vec(),
            boundaries,
        });
        Ok(vm)
    }
    pub fn string(&self, value: Value) -> Result<&[u8], VmError> {
        match value.tag {
            Value::STATIC_STRING => self
                .program
                .literal(value.payload)
                .ok_or(VmError::StringReference(value.payload)),
            Value::DYNAMIC_STRING => self
                .strings
                .get(&value.payload)
                .map(Vec::as_slice)
                .ok_or(VmError::StringReference(value.payload)),
            _ => Err(VmError::Type(value.tag)),
        }
    }
    /// Host string resolution inspects raw qualifier bits independently of ABI
    /// type checks. A qualified integer can therefore address the dynamic heap.
    pub fn host_string(&self, value: Value) -> Result<Option<&[u8]>, VmError> {
        if value.tag & 0x0800 != 0 {
            Ok(Some(
                self.strings
                    .get(&value.payload)
                    .ok_or(VmError::StringReference(value.payload))?,
            ))
        } else if value.tag & 0x1000 != 0 {
            Ok(Some(
                self.program
                    .literal(value.payload)
                    .ok_or(VmError::StringReference(value.payload))?,
            ))
        } else {
            Ok(None)
        }
    }
    pub fn intern(&mut self, text: &[u8]) -> Result<Value, VmError> {
        let text = &text[..text
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(text.len())];
        if let Some(offset) = self.interned.get(text) {
            return Ok(Value {
                payload: *offset,
                tag: Value::DYNAMIC_STRING,
            });
        }
        let length = u32::try_from(text.len()).map_err(|_| VmError::StringLimit)?;
        let end = self
            .string_bytes
            .checked_add(length)
            .and_then(|v| v.checked_add(1))
            .filter(|end| *end <= 8 * 1024 * 1024)
            .ok_or(VmError::StringLimit)?;
        let offset = self.string_bytes;
        self.string_bytes = end;
        self.strings.insert(offset, text.to_vec());
        self.interned.insert(text.to_vec(), offset);
        Ok(Value {
            payload: offset,
            tag: Value::DYNAMIC_STRING,
        })
    }
    pub(crate) fn external(&self, value: Value) -> Result<ExternalValue, VmError> {
        if value.is_string() {
            Ok(ExternalValue::String(self.string(value)?.to_vec()))
        } else {
            Ok(ExternalValue::Value(value))
        }
    }
    fn receive(&mut self, value: ExternalValue) -> Result<(), VmError> {
        let value = match value {
            ExternalValue::Value(value) if value.is_string() => {
                return Err(VmError::Type(value.tag));
            }
            ExternalValue::Value(value) => value,
            ExternalValue::String(text) => self.intern(&text)?,
        };
        self.value_stack.push(value)
    }
    /// The bytes `pc` addresses: the program image, or an isolated fragment.
    pub fn code(&self) -> &[u8] {
        self.fragment
            .as_ref()
            .map_or(&self.program.image, |fragment| &fragment.bytes)
    }
    /// Whether `address` is a valid jump target: an instruction boundary, or
    /// an isolated fragment's end. Read-only; used by debug inspectors.
    pub fn instruction_boundary(&self, address: u32) -> bool {
        self.address_valid(address)
    }
    fn address_valid(&self, address: u32) -> bool {
        if let Some(fragment) = &self.fragment {
            address as usize == fragment.bytes.len()
                || fragment
                    .boundaries
                    .get(address as usize)
                    .copied()
                    .unwrap_or(false)
        } else {
            self.program
                .boundaries
                .get(address as usize)
                .copied()
                .unwrap_or(false)
        }
    }
    fn jump(&mut self, address: u32) -> Result<(), VmError> {
        if !self.address_valid(address) {
            return Err(VmError::Address(address));
        }
        self.pc = address;
        Ok(())
    }
    fn fetch(&mut self) -> Result<(u16, Option<u32>), VmError> {
        let at = self.pc as usize;
        if !self.address_valid(self.pc) {
            return Err(VmError::Address(self.pc));
        }
        let code = self.code();
        let bytes = code.get(at..at + 2).ok_or(VmError::Truncated(self.pc))?;
        let word = u16::from_be_bytes(bytes.try_into().expect("instruction word"));
        if !valid_word(word) {
            return Err(VmError::Opcode(word));
        }
        let size = instruction_length(word);
        let payload = if size == 6 {
            Some(u32::from_be_bytes(
                code.get(at + 2..at + 6)
                    .ok_or(VmError::Truncated(self.pc))?
                    .try_into()
                    .expect("immediate"),
            ))
        } else {
            None
        };
        self.pc += size as u32;
        Ok((word, payload))
    }
    fn index(&mut self, exact: bool) -> Result<usize, VmError> {
        let value = self.value_stack.pop()?;
        if exact {
            value.integer_value()?;
        }
        let index = value.payload as usize;
        if index >= self.program.procedures.len() {
            return Err(VmError::Procedure(value.payload));
        }
        Ok(index)
    }
    /// A variable slot; every index must be integer-tagged (the SPEC-004
    /// §3.1 safe rule extends the local-fetch check to the other accesses).
    fn slot(&self, base: i32, index: Value) -> Result<usize, VmError> {
        let index = index.integer_value()?;
        let base = usize::try_from(base).map_err(|_| VmError::Slot)?;
        let index = usize::try_from(index).map_err(|_| VmError::Slot)?;
        base.checked_add(index.checked_mul(6).ok_or(VmError::Slot)?)
            .ok_or(VmError::Slot)
    }
    fn name(&mut self) -> Result<Vec<u8>, VmError> {
        let offset = self.value_stack.pop()?.payload;
        self.program
            .identifier(offset)
            .map(<[u8]>::to_vec)
            .ok_or(VmError::StringReference(offset))
    }
    fn restore_context(&mut self) -> Result<(), VmError> {
        self.window = self.value_stack.pop()?.integer_value()?;
        self.wait_predicate = self.value_stack.pop()?.integer_value()? as u32;
        self.status = VmStatus((self.value_stack.pop()?.integer_value()? as u32 & 0xffff) as u16);
        Ok(())
    }
    fn return_address(&mut self) -> Result<(), VmError> {
        let address = self.return_stack.pop()?.payload;
        self.jump(address)
    }
    fn external_return(
        &mut self,
        host: &mut impl VmHost,
        result: Option<ExternalValue>,
    ) -> Result<(), VmError> {
        self.restore_context()?;
        let caller_id = self.return_stack.pop()?.integer_value()? as u32;
        let wait = self.return_stack.pop()?.integer_value()? as u32;
        let status = self.return_stack.pop()?.integer_value()? as u16;
        if caller_id == self.id {
            return Err(VmError::Host("external caller refers to current VM".into()));
        }
        let caller = host.program_mut(caller_id)?;
        caller.wait_predicate = wait;
        caller.status = VmStatus(status);
        if let Some(result) = result {
            caller.receive(result)?;
            caller.return_address()?;
            if caller.status.contains(VmStatus::CRITICAL) {
                self.status.0 &= !VmStatus::CRITICAL;
            }
        }
        self.return_address()
    }
    /// SPEC-004 §3.3 `0x800E`/`0x800F`: unblock the parent and drop the
    /// link. A parent the host no longer has cannot be blocked on this
    /// program, so the link is simply dropped; that is not a fault.
    fn detach(&mut self, host: &mut impl VmHost, spawn_only: bool) -> Result<(), VmError> {
        if let Some(parent) = self.parent {
            let Ok(caller) = host.program_mut(parent) else {
                self.parent = None;
                return Ok(());
            };
            if !spawn_only || caller.status.contains(VmStatus::SPAWN_BLOCKED) {
                caller.status.0 &= !(VmStatus::ATTACHED | VmStatus::SPAWN_BLOCKED);
                caller.child = None;
                self.parent = None;
            }
        }
        Ok(())
    }
    fn execute(&mut self, host: &mut impl VmHost) -> Result<(), VmError> {
        let (word, immediate) = self.fetch()?;
        if let Some(payload) = immediate {
            return self.value_stack.push(Value { payload, tag: word });
        }
        match word {
            0x8000 => {}
            0x8002 | 0x804a => self.status.0 |= VmStatus::CRITICAL,
            0x8003 | 0x804b => self.status.0 &= !VmStatus::CRITICAL,
            0x8004 => {
                let target = self.value_stack.pop()?;
                if target.tag & !0x0800 != Value::INTEGER {
                    return Err(VmError::Type(target.tag));
                }
                self.jump(target.payload)?;
            }
            0x8005 => {
                let index = self.index(false)?;
                // Only the scalar metadata is needed; the record's name is
                // borrowed for an import rather than cloned on every call.
                let procedure = &self.program.procedures[index];
                let (flags, body) = (procedure.flags, procedure.body);
                if flags & program::IMPORT != 0 {
                    host.import_procedure(&procedure.name)?;
                    return Err(VmError::ImportedAbi);
                }
                if flags & program::CRITICAL != 0 {
                    self.status.0 |= VmStatus::CRITICAL;
                }
                self.jump(body)?;
            }
            0x8006 | 0x8007 => {
                let index = self.index(false)?;
                let value = self.value_stack.pop()?.payload;
                let old = &self.program.procedures[index];
                let (flags, deadline, condition) = (old.flags, old.deadline, old.condition);
                if word == 0x8006 {
                    self.program.schedule(
                        index,
                        flags | program::TIMED,
                        host.now_ms().wrapping_add(value.wrapping_mul(1000)),
                        condition,
                    );
                } else {
                    // SPEC-004 §3.8 safe rule: a condition stream must start
                    // at a complete instruction, so an address that is not an
                    // instruction boundary (the code end included) is
                    // rejected here rather than when the condition first runs.
                    if !self.address_valid(value) {
                        return Err(VmError::Address(value));
                    }
                    self.program
                        .schedule(index, flags | program::CONDITIONAL, deadline, value);
                }
            }
            0x8008..=0x800b => {
                let value = self.value_stack.pop()?;
                let name = self.string(value)?.to_vec();
                let mode = match word {
                    0x8008 => LaunchMode::Attached,
                    0x8009 => LaunchMode::Replace,
                    0x800a => LaunchMode::Spawn,
                    _ => LaunchMode::Independent,
                };
                let parent = match mode {
                    LaunchMode::Attached | LaunchMode::Spawn => Some(self.id),
                    LaunchMode::Replace => self.parent,
                    LaunchMode::Independent => None,
                };
                // SPEC-005 §2.2 `0x8009` [May-improve] safe rule: the old
                // program's procedure entries go before the replacement's
                // first instruction (its variables stay registered).
                if mode == LaunchMode::Replace {
                    host.remove_procedures(self.id)?;
                }
                let child = host.launch(
                    &name,
                    mode,
                    parent,
                    mode == LaunchMode::Spawn && self.status.contains(VmStatus::CRITICAL),
                )?;
                match mode {
                    LaunchMode::Attached => {
                        self.child = Some(child);
                        self.status.0 |= VmStatus::ATTACHED;
                    }
                    LaunchMode::Spawn => {
                        self.child = Some(child);
                        self.status.0 |= VmStatus::SPAWN_BLOCKED;
                    }
                    LaunchMode::Replace => self.status.0 |= VmStatus::ENDED,
                    LaunchMode::Independent => {}
                }
            }
            0x800c => {
                let value = self.return_stack.pop()?;
                self.value_stack.push(value)?;
            }
            0x800d => {
                let value = self.value_stack.pop()?;
                self.return_stack.push(value)?;
            }
            0x800e => {
                self.status.0 |= VmStatus::ENDED;
                self.detach(host, true)?;
                host.purge(self.id)?;
            }
            0x800f => self.detach(host, false)?,
            0x8010 => self.status.0 |= VmStatus::ENDED,
            0x8011 => self.status.0 |= VmStatus::STOPPED,
            0x8012 | 0x8032 => {
                let index = self.value_stack.pop()?;
                let slot = self.slot(if word == 0x8032 { self.f } else { self.g }, index)?;
                self.value_stack.push(self.value_stack.get(slot)?)?;
            }
            0x8013 | 0x8031 => {
                let index = self.value_stack.pop()?;
                let value = self.value_stack.pop()?;
                let slot = self.slot(if word == 0x8031 { self.f } else { self.g }, index)?;
                self.value_stack.set(slot, value)?;
            }
            0x8014 => {
                let name = self.name()?;
                self.receive(host.fetch_variable(&name)?)?;
            }
            0x8015 => {
                let name = self.name()?;
                let value = self.value_stack.pop()?;
                host.store_variable(&name, self.external(value)?)?;
            }
            0x8016 => {
                let name = self.name()?;
                host.export_variable(self.id, &name)?;
            }
            0x8017 => {
                let index = self.index(false)?;
                let arguments = self.value_stack.pop()?.payload;
                let procedure = &self.program.procedures[index];
                host.export_procedure(
                    &procedure.name,
                    ExportedProcedure {
                        owner: self.id,
                        address: procedure.body,
                        arguments,
                    },
                )?;
            }
            0x8018 => self.value_stack.swap()?,
            0x8019 => self.return_stack.swap()?,
            0x801a => {
                self.value_stack.pop()?;
            }
            0x801b => self.value_stack.push(self.value_stack.top()?)?,
            0x801c | 0x801d => {
                self.return_address()?;
                if word == 0x801d {
                    self.status.0 |= VmStatus::YIELD;
                }
            }
            0x801e => {
                self.return_stack.pop()?;
            }
            0x801f => self.restore_context()?,
            0x8020 | 0x8021 => {
                self.restore_context()?;
                self.return_address()?;
                if word == 0x8021 {
                    self.status.0 |= VmStatus::YIELD;
                }
            }
            0x8022 | 0x8023 => {
                self.external_return(host, None)?;
                if word == 0x8023 {
                    self.status.0 |= VmStatus::YIELD;
                }
            }
            0x8024 => {
                let value = self.value_stack.pop()?;
                self.external_return(host, Some(self.external(value)?))?;
            }
            0x8025 | 0x8026 => {
                let value = self.value_stack.pop()?;
                if word == 0x8026 {
                    self.external_return(host, None)?;
                } else {
                    self.restore_context()?;
                    self.return_address()?;
                }
                self.value_stack.push(value)?;
                self.status.0 |= VmStatus::YIELD;
            }
            0x8027 => {
                let expected = self.value_stack.pop()?.payload;
                let index = self.index(false)?;
                if self.program.procedures[index].arguments != expected {
                    return Err(VmError::Arguments);
                }
            }
            0x8028 => {
                let value = self.value_stack.pop()?;
                let name = self.string(value)?;
                let index = self
                    .program
                    .find_procedure(name, false)
                    .ok_or_else(|| VmError::Symbol(String::from_utf8_lossy(name).into()))?;
                self.value_stack.push(Value::integer(index as i32))?;
            }
            0x8029 => {
                let base = self.return_stack.pop()?.integer_value()?;
                if base != -1
                    && (base < 0 || base as usize > self.value_stack.byte_len() || base % 6 != 0)
                {
                    return Err(VmError::Slot);
                }
                self.f = base;
            }
            0x802a => self
                .value_stack
                .truncate(usize::try_from(self.f).map_err(|_| VmError::Slot)?)?,
            0x802b => {
                let count = self.value_stack.pop()?.integer_value()?;
                let base = (self.value_stack.byte_len() as i64) - i64::from(count) * 6;
                if count < 0 || base < 0 || base > self.value_stack.byte_len() as i64 {
                    return Err(VmError::Slot);
                }
                self.return_stack.push(Value::integer(self.f))?;
                self.f = base as i32;
            }
            0x802c => self.g = self.value_stack.byte_len() as i32,
            0x802d => {
                let index = self.index(true)?;
                self.value_stack
                    .push(Value::integer(self.program.procedures[index].body as i32))?;
            }
            // SPEC-004 §3.2 safe rule: discard exactly the counted values
            // (the reference reuses each popped payload as the next bound).
            0x802e => {
                let count = self.value_stack.pop()?.integer_value()?;
                for _ in 0..count.max(0) {
                    self.value_stack.pop()?;
                }
            }
            0x802f | 0x8030 => {
                let condition = self.value_stack.pop()?;
                if word == 0x802f || condition.payload == 0 {
                    let target = self.value_stack.pop()?;
                    if condition.payload == 0 {
                        self.jump(target.payload)?;
                    }
                }
            }
            0x8033..=0x8046 => crate::arithmetic::execute(self, word)?,
            0x8047 => {
                let delay = self.value_stack.pop()?.payload;
                self.wait_start = host.now_ms();
                self.wait_deadline = self.wait_start.wrapping_add(delay);
                self.wait_predicate = 1;
                self.status.0 |= VmStatus::WAITING;
            }
            0x8048 => {
                let index = self.index(false)?;
                self.program.schedule(index, 0, 0, 0);
            }
            0x8049 => {
                for index in 0..self.program.procedures.len() {
                    self.program.schedule(index, 0, 0, 0);
                }
            }
            0x804c..=0x8155 => host.extension(word, self)?,
            _ => return Err(VmError::Opcode(word)),
        }
        Ok(())
    }
    fn fault<T>(&mut self, error: VmError) -> Result<T, VmError> {
        self.status.0 |= VmStatus::ENDED | VmStatus::ERROR;
        self.last_error = Some(error.clone());
        Err(error)
    }
    pub fn update(&mut self, host: &mut impl VmHost) -> Result<RunOutcome, VmError> {
        self.run(host, 10)
    }
    /// Run up to `budget` instructions. SPEC-006 §3.5 / SPEC-004 §3.3:
    /// instructions executed while critical is set do not count against the
    /// budget; the hard instruction limit counts every instruction.
    pub fn run(&mut self, host: &mut impl VmHost, budget: u32) -> Result<RunOutcome, VmError> {
        self.execute_loop(host, budget, false)
    }
    /// Execute at most one instruction, after the same status checks as
    /// [`Self::run`]: `Budget` when it ran and nothing else is to report.
    /// A debugger's observer steps a program this way.
    pub fn step(&mut self, host: &mut impl VmHost) -> Result<RunOutcome, VmError> {
        self.execute_loop(host, 0, true)
    }
    fn execute_loop(
        &mut self,
        host: &mut impl VmHost,
        budget: u32,
        single: bool,
    ) -> Result<RunOutcome, VmError> {
        let (mut executed, mut counted) = (0, 0);
        loop {
            if self.status.contains(VmStatus::YIELD) {
                self.status.0 &= !VmStatus::YIELD;
                return Ok(RunOutcome::Yielded);
            }
            if self.status.contains(VmStatus::ENDED) {
                return Ok(RunOutcome::Ended);
            }
            if self.status.contains(VmStatus::STOPPED) {
                return Ok(RunOutcome::Stopped);
            }
            if self
                .status
                .contains(VmStatus::ATTACHED | VmStatus::SPAWN_BLOCKED)
            {
                return Ok(RunOutcome::Blocked);
            }
            if self.status.contains(VmStatus::WAITING) {
                // SPEC-004 §3.3 safe rule: a timed wait releases on its first
                // readiness check, so a late check cannot hang the program.
                let ready = if self.wait_predicate == 1 {
                    Ok(true)
                } else {
                    host.wait_ready(self.wait_predicate, self.wait_start, self.wait_deadline)
                };
                match ready {
                    Ok(true) => self.status.0 &= !VmStatus::WAITING,
                    Ok(false) => return Ok(RunOutcome::Waiting),
                    Err(error) => return self.fault(error),
                }
            }
            if self
                .fragment
                .as_ref()
                .is_some_and(|fragment| self.pc as usize == fragment.bytes.len())
            {
                return Ok(RunOutcome::FragmentEnd);
            }
            let critical = self.status.contains(VmStatus::CRITICAL);
            let spent = if single {
                executed >= 1
            } else {
                counted >= budget && !critical
            };
            if spent {
                return Ok(RunOutcome::Budget);
            }
            if executed >= self.hard_instruction_limit {
                return self.fault(VmError::InstructionLimit);
            }
            if let Err(error) = self.execute(host) {
                return self.fault(error);
            }
            executed += 1;
            if !critical {
                counted += 1;
            }
        }
    }
    fn save_context(&mut self) -> Result<(), VmError> {
        for value in [
            self.status.0 as i32,
            self.wait_predicate as i32,
            self.window,
        ] {
            self.value_stack.push(Value::integer(value))?;
        }
        self.value_stack.push(Value::integer(0))
    }
    pub fn prepare_host_call(&mut self, index: usize, synchronous: bool) -> Result<(), VmError> {
        if self
            .program
            .procedures
            .get(index)
            .is_some_and(|procedure| procedure.flags & program::IMPORT != 0)
        {
            return Err(VmError::Host(
                "imported host call requires explicit external preparation".into(),
            ));
        }
        let body = self
            .program
            .procedures
            .get(index)
            .ok_or(VmError::Procedure(index as u32))?
            .body;
        if self.fragment.is_some() {
            return Err(VmError::Host("host calls require a complete image".into()));
        }
        self.return_stack.push(Value::integer(self.pc as i32))?;
        self.return_stack
            .push(Value::integer(if synchronous { 24 } else { 20 }))?;
        self.save_context()?;
        self.status.0 &= VmStatus::CRITICAL;
        self.jump(body)
    }
    pub fn prepare_lifecycle(&mut self, event: Lifecycle) -> Result<bool, VmError> {
        let index = self
            .program
            .lifecycle_entry(event)
            .map_err(|error| VmError::Host(error.to_string()))?;
        if let Some(index) = index {
            self.prepare_host_call(index, true)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    pub fn prepare_external_call(
        &mut self,
        caller: &mut Vm,
        index: usize,
        synchronous: bool,
    ) -> Result<(), VmError> {
        let body = self
            .program
            .procedures
            .get(index)
            .ok_or(VmError::Procedure(index as u32))?
            .body;
        if self.id == caller.id || self.fragment.is_some() {
            return Err(VmError::Host("invalid external call context".into()));
        }
        for value in [
            self.pc as i32,
            caller.status.0 as i32,
            caller.wait_predicate as i32,
            caller.id as i32,
            if synchronous { 32 } else { 28 },
        ] {
            self.return_stack.push(Value::integer(value))?;
        }
        self.save_context()?;
        caller.status.0 |= VmStatus::ATTACHED;
        self.status.0 &= VmStatus::CRITICAL;
        self.jump(body)
    }
    /// Fires at most one procedure per poll: firing prepares the call on
    /// this VM (its program counter now points at the procedure body), so a
    /// second candidate would clobber it. Later candidates fire on later
    /// polls, in procedure order. Conditions execute with the real host and
    /// can mutate globals/exports; they have no purity privilege.
    ///
    /// Any failure, in a condition or in preparing the fired call, faults
    /// the VM (ended/error with `last_error`), so a scheduled procedure
    /// never silently disappears: SPEC-004 §3.8 safe rule.
    pub fn poll_scheduled(
        &mut self,
        host: &mut impl VmHost,
        condition_budget: u32,
    ) -> Result<Option<usize>, VmError> {
        for index in 0..self.program.procedures.len() {
            let procedure = &self.program.procedures[index];
            let (flags, deadline, condition) =
                (procedure.flags, procedure.deadline, procedure.condition);
            let fire = if flags & program::CONDITIONAL != 0 {
                match self.condition(host, condition, condition_budget) {
                    Ok(result) => result,
                    Err(error) => return self.fault(error),
                }
            } else {
                flags & program::TIMED != 0 && deadline < host.now_ms()
            };
            if fire {
                self.program.schedule(index, 0, deadline, condition);
                if let Err(error) = self.prepare_host_call(index, false) {
                    return self.fault(error);
                }
                if flags & program::CRITICAL != 0 {
                    self.status.0 |= VmStatus::CRITICAL;
                }
                return Ok(Some(index));
            }
        }
        Ok(None)
    }
    fn condition(
        &mut self,
        host: &mut impl VmHost,
        address: u32,
        budget: u32,
    ) -> Result<bool, VmError> {
        let (pc, status, f, g, wait, window) = (
            self.pc,
            self.status,
            self.f,
            self.g,
            self.wait_predicate,
            self.window,
        );
        let returns = self.return_stack.byte_len();
        let values = self.value_stack.byte_len();
        self.return_stack.push(Value::integer(pc as i32))?;
        self.status = VmStatus::default();
        self.jump(address)?;
        let mut completed = false;
        for _ in 0..budget.min(self.hard_instruction_limit) {
            host.observe_before(self);
            let result = self.execute(host);
            host.observe_after(self, &result.clone().map(|()| RunOutcome::Budget));
            result?;
            if self.pc == pc && self.return_stack.byte_len() == returns {
                completed = true;
                break;
            }
            if self.status.contains(
                VmStatus::ENDED
                    | VmStatus::STOPPED
                    | VmStatus::ATTACHED
                    | VmStatus::SPAWN_BLOCKED
                    | VmStatus::WAITING,
            ) {
                break;
            }
        }
        if !completed || self.value_stack.byte_len() != values + 6 || self.f != f || self.g != g {
            return Err(VmError::Condition);
        }
        let predicate = self.value_stack.pop()?.payload != 0;
        self.pc = pc;
        self.status = status;
        self.wait_predicate = wait;
        self.window = window;
        Ok(predicate)
    }
}
