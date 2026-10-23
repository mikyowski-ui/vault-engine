//! Read-only SPEC-004 instruction decoding for inspection. Nothing here
//! executes or mutates a VM.
use std::fmt;
use vault_vm::Vm;

/// Which SPEC-004 §3.7 registration range an instruction word falls in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OpcodeClass {
    /// Dispatch slot 1: a tagged four-byte immediate push.
    Push,
    /// `0x8000`-`0x804B`, registered by the core VM.
    Core,
    /// `0x804C`-`0x80A0`, the general interpreter library.
    Library,
    /// `0x80A1`-`0x8155`, game-specific extensions.
    Game,
    /// A word outside every registered range. `decode` never produces one
    /// (it reports `DecodeError::Invalid`); only an `Instruction` built by
    /// hand can carry such a word, and it is classified here instead of
    /// panicking.
    Invalid,
}

/// One decoded instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instruction {
    pub address: u32,
    pub word: u16,
    /// The big-endian payload of a slot-1 push.
    pub immediate: Option<u32>,
}

/// Why an address does not hold a decodable instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// The address is not an instruction boundary.
    Address(u32),
    /// The address is the end of the code (a fragment's successful end).
    End(u32),
    /// The word or immediate runs past the end of the code.
    Truncated(u32),
    /// The word is outside every registered range.
    Invalid { address: u32, word: u16 },
}
impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Address(at) => write!(f, "{at:#06x} is not an instruction boundary"),
            Self::End(at) => write!(f, "{at:#06x} is the end of the code"),
            Self::Truncated(at) => write!(f, "instruction at {at:#06x} is truncated"),
            Self::Invalid { address, word } => {
                write!(f, "invalid instruction {word:#06x} at {address:#06x}")
            }
        }
    }
}

/// SPEC-004 §2.5: a word whose low ten bits select slot 1 carries a
/// four-byte immediate; every other word is two bytes.
pub fn instruction_length(word: u16) -> u32 {
    if word & 0x3ff == 1 { 6 } else { 2 }
}

/// The class of a word the VM accepts, or `None` for one it rejects. As in
/// the VM, every slot-1 word with bit 15 set is a push (SPEC-004 §2.5),
/// including the non-standard tags such as `0x8401`.
pub fn classify(word: u16) -> Option<OpcodeClass> {
    if word & 0x3ff == 1 {
        return (word & 0x8000 != 0).then_some(OpcodeClass::Push);
    }
    match word {
        0x8000..=0x804b => Some(OpcodeClass::Core),
        0x804c..=0x80a0 => Some(OpcodeClass::Library),
        0x80a1..=0x8155 => Some(OpcodeClass::Game),
        _ => None,
    }
}

/// Decode the instruction starting at `address` in `code`, without checking
/// that the address is a boundary.
pub fn decode(code: &[u8], address: u32) -> Result<Instruction, DecodeError> {
    let at = address as usize;
    if at == code.len() {
        return Err(DecodeError::End(address));
    }
    let bytes = code
        .get(at..at + 2)
        .ok_or(DecodeError::Truncated(address))?;
    let word = u16::from_be_bytes([bytes[0], bytes[1]]);
    if classify(word).is_none() {
        return Err(DecodeError::Invalid { address, word });
    }
    let immediate = if instruction_length(word) == 6 {
        let payload = code
            .get(at + 2..at + 6)
            .ok_or(DecodeError::Truncated(address))?;
        Some(u32::from_be_bytes(payload.try_into().expect("immediate")))
    } else {
        None
    };
    Ok(Instruction {
        address,
        word,
        immediate,
    })
}

/// Decode the instruction a VM would execute next.
pub fn decode_next(vm: &Vm) -> Result<Instruction, DecodeError> {
    if vm.pc as usize == vm.code().len() {
        return Err(DecodeError::End(vm.pc));
    }
    if !vm.instruction_boundary(vm.pc) {
        return Err(DecodeError::Address(vm.pc));
    }
    decode(vm.code(), vm.pc)
}

/// Decode consecutive instructions from `address`, stopping at the first
/// failure or after `count` instructions.
pub fn disassemble(code: &[u8], address: u32, count: usize) -> Vec<Instruction> {
    let mut listing = Vec::new();
    let mut at = address;
    while listing.len() < count {
        let Ok(instruction) = decode(code, at) else {
            break;
        };
        listing.push(instruction);
        at = instruction.next_address();
    }
    listing
}

impl Instruction {
    pub fn length(&self) -> u32 {
        instruction_length(self.word)
    }
    pub fn next_address(&self) -> u32 {
        self.address + self.length()
    }
    pub fn class(&self) -> OpcodeClass {
        classify(self.word).unwrap_or(OpcodeClass::Invalid)
    }
    /// A short mnemonic for core and push words; extensions, whose catalogs
    /// SPEC-004 defers, are `lib`/`ext` with their word.
    pub fn mnemonic(&self) -> &'static str {
        mnemonic(self.word)
    }
    /// The mnemonic with its operand, e.g. `push.int 5` or `ext 0x80a5`.
    pub fn text(&self) -> String {
        let mnemonic = self.mnemonic();
        match (self.word, self.immediate) {
            (0xc001 | 0xc801, Some(payload)) => format!("{mnemonic} {}", payload as i32),
            (0xa001 | 0xa801, Some(payload)) => format!(
                "{mnemonic} {}",
                String::from_utf8_lossy(&vault_vm::float_text(f32::from_bits(payload)))
            ),
            (word, Some(payload)) if mnemonic == "push.tag" => {
                format!("{mnemonic} {word:#06x} {payload:#x}")
            }
            (_, Some(payload)) => format!("{mnemonic} {payload:#x}"),
            _ => match self.class() {
                OpcodeClass::Library | OpcodeClass::Game | OpcodeClass::Invalid => {
                    format!("{mnemonic} {:#06x}", self.word)
                }
                _ => mnemonic.to_string(),
            },
        }
    }
}
impl fmt::Display for Instruction {
    /// `0xADDR  WORD [PAYLOAD]  TEXT`, with raw bytes in fixed-width columns.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let payload = self
            .immediate
            .map_or_else(String::new, |payload| format!("{payload:08x}"));
        write!(
            f,
            "{:#06x}  {:04x} {:<8}  {}",
            self.address,
            self.word,
            payload,
            self.text()
        )
    }
}

/// Short names for the SPEC-004 §2.6 core instruction table and §2.5 pushes.
pub fn mnemonic(word: u16) -> &'static str {
    match word {
        0xc001 => "push.int",
        0xc801 => "push.int.q",
        0xa001 => "push.float",
        0xa801 => "push.float.q",
        0x9001 => "push.str",
        0x9801 => "push.dstr",
        0x8001 => "push.raw",
        0x8000 => "nop",
        0x8002 | 0x804a => "critical.enter",
        0x8003 | 0x804b => "critical.leave",
        0x8004 => "jump",
        0x8005 => "call",
        0x8006 => "schedule.timed",
        0x8007 => "schedule.cond",
        0x8008 => "start.attached",
        0x8009 => "replace",
        0x800a => "spawn",
        0x800b => "start.independent",
        0x800c => "r>v",
        0x800d => "v>r",
        0x800e => "purge",
        0x800f => "detach",
        0x8010 => "end",
        0x8011 => "stop",
        0x8012 => "global.fetch",
        0x8013 => "global.store",
        0x8014 => "extern.fetch",
        0x8015 => "extern.store",
        0x8016 => "export.var",
        0x8017 => "export.proc",
        0x8018 => "swap",
        0x8019 => "rswap",
        0x801a => "drop",
        0x801b => "dup",
        0x801c => "ret",
        0x801d => "ret.yield",
        0x801e => "rdrop",
        0x801f => "ctx.restore",
        0x8020 => "ctx.ret",
        0x8021 => "ctx.ret.yield",
        0x8022 => "xret",
        0x8023 => "xret.yield",
        0x8024 => "xret.value",
        0x8025 => "keep.yield",
        0x8026 => "xkeep.yield",
        0x8027 => "args.check",
        0x8028 => "proc.find",
        0x8029 => "frame.restore",
        0x802a => "frame.drop",
        0x802b => "frame.enter",
        0x802c => "globals.base",
        0x802d => "proc.address",
        0x802e => "drop.counted",
        0x802f => "branch.zero",
        0x8030 => "loop",
        0x8031 => "local.store",
        0x8032 => "local.fetch",
        0x8033 => "eq",
        0x8034 => "ne",
        0x8035 => "le",
        0x8036 => "ge",
        0x8037 => "lt",
        0x8038 => "gt",
        0x8039 => "add",
        0x803a => "sub",
        0x803b => "mul",
        0x803c => "div",
        0x803d => "mod",
        0x803e => "and",
        0x803f => "or",
        0x8040 => "bitand",
        0x8041 => "bitor",
        0x8042 => "bitxor",
        0x8043 => "bitnot",
        0x8044 => "ftoi",
        0x8045 => "not",
        0x8046 => "neg",
        0x8047 => "wait",
        0x8048 => "cancel",
        0x8049 => "cancel.all",
        0x804c..=0x80a0 => "lib",
        0x80a1..=0x8155 => "ext",
        // §2.5: a push whose tag has no defined value semantics.
        word if word & 0x83ff == 0x8001 => "push.tag",
        _ => "invalid",
    }
}
