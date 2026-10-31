//! SPEC-024b part 1: the sfall word table, compiled from
//! `oracle/SPEC-024/sfall-opcodes.tsv`, the value model of section 4.1 and the
//! pure word effects. What needs the world lives behind the host's
//! [`extension`](crate::VmHost::extension), in `vault_core::host_sfall`.
use crate::extensions::{OpcodeSpec, PushKind, Rule};
use crate::{Value, Vm, VmError};

pub use crate::sfall_math::{
    arctan, ceil, cosine, divide, exponent, floor, logarithm, power, round, sine, square_root,
    tangent,
};
pub use crate::sfall_text::{
    FORMAT_LIMIT, atof, atoi, compare, find, format, split, substr, to_case,
};

/// The first and last word of SPEC-024b's table.
pub const FIRST_WORD: u16 = 0x8156;
pub const LAST_WORD: u16 = 0x8281;
/// The reported sfall version (SPEC-024b §4.4).
pub const VERSION: [i32; 3] = [4, 5, 1];

/// An operand class (§4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    I,
    F,
    N,
    S,
    K,
    A,
    O,
    P,
}
/// A row's tier (§1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    P1,
    P2,
    X,
    R,
}
/// One row of the oracle table. `id` is the word or sub-function number,
/// `dispatch` the word that reaches it, and `pops` counts the name or
/// sub-function operand too. `args` is in public-call order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SfallRow {
    pub id: u16,
    pub name: &'static str,
    pub dispatch: u16,
    pub pops: Option<u8>,
    pub push: bool,
    pub args: &'static [Class],
    pub ret: Option<Class>,
    pub tier: Tier,
}
/// A hook argument's or return's declared type (§6.5): every object type
/// converts as class `I`, and `mixed` is taken as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookType {
    Int,
    Critter,
    Item,
    Object,
    Mixed,
    Text,
}
impl HookType {
    /// `value` as this type, or `None` when it cannot convert.
    pub fn convert(self, value: &SfallValue) -> Option<SfallValue> {
        match self {
            Self::Int | Self::Critter | Self::Item | Self::Object => value.coerce(Class::I),
            Self::Mixed => Some(value.clone()),
            Self::Text => value.coerce(Class::S),
        }
    }
}
/// One row of `sfall-hooks.tsv`. A hook that does not fire is tier P2: its
/// script loads and registers, and no chain runs (§6.14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookRow {
    pub id: i32,
    pub name: &'static str,
    pub file: &'static str,
    pub fires: bool,
    pub args: &'static [HookType],
    pub rets: &'static [HookType],
}
include!(concat!(env!("OUT_DIR"), "/sfall_catalog.rs"));

/// The row of hook `id`: none for the reserved range or an unknown ID.
pub fn hook_row(id: i32) -> Option<&'static HookRow> {
    SFALL_HOOKS.iter().find(|row| row.id == id)
}

/// The row of `word`, reserved words included.
pub fn word_row(word: u16) -> Option<&'static SfallRow> {
    let index = usize::from(word.checked_sub(FIRST_WORD)?);
    SFALL_WORDS.get(index).filter(|row| row.id == word)
}
/// The stack row of `word`: none for a reserved word (§3.3).
pub fn spec(word: u16) -> Option<&'static OpcodeSpec> {
    SFALL_SPECS
        .binary_search_by_key(&word, |spec| spec.word)
        .ok()
        .and_then(|index| SFALL_SPECS.get(index))
}
/// The tier of function `name`, matched as exact bytes.
pub fn function_tier(name: &[u8]) -> Option<Tier> {
    SFALL_FUNCTIONS
        .iter()
        .find(|row| row.name.as_bytes() == name)
        .map(|row| row.tier)
}
/// The P1 function names, ascending by bytes, one each (§4.2).
pub fn p1_functions() -> Vec<&'static str> {
    let mut names: Vec<&str> = SFALL_FUNCTIONS
        .iter()
        .filter(|row| row.tier == Tier::P1)
        .map(|row| row.name)
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}
/// Whether `word` is a P1 word, as `opcode_exists` reports it (§4.2).
pub fn p1_word(word: i32) -> bool {
    u16::try_from(word)
        .ok()
        .and_then(word_row)
        .is_some_and(|row| row.tier == Tier::P1)
}

/// A value of the model of §4.1: object references and array IDs are ints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SfallValue {
    Int(i32),
    /// The binary32 bits, unchanged: equality is by bits (§5.6).
    Float(u32),
    Text(Vec<u8>),
}
impl SfallValue {
    pub fn float(value: f32) -> Self {
        Self::Float(value.to_bits())
    }
    pub fn text(value: impl Into<Vec<u8>>) -> Self {
        Self::Text(value.into())
    }
    /// The int payload of a value already of class `I`, `O` or `P`.
    pub fn int(&self) -> i32 {
        match self {
            Self::Int(value) => *value,
            Self::Float(bits) => f32::from_bits(*bits) as i32,
            Self::Text(_) => 0,
        }
    }
    /// The float value of a value already of class `F`.
    pub fn number(&self) -> f32 {
        match self {
            Self::Int(value) => *value as f32,
            Self::Float(bits) => f32::from_bits(*bits),
            Self::Text(_) => 0.0,
        }
    }
    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Text(text) => text,
            _ => &[],
        }
    }
    /// `typeof`'s result, which is also the sidecar's type byte (§5.8).
    pub fn type_code(&self) -> i32 {
        match self {
            Self::Int(_) => 1,
            Self::Float(_) => 2,
            Self::Text(_) => 3,
        }
    }
    /// The value of a stack entry: `None` for any other tag, or for a string
    /// reference that resolves to nothing (§4.1). Bit `0x0800` is cleared
    /// first, as SPEC-004 §2.5 says.
    pub fn read(vm: &Vm, value: Value) -> Option<Self> {
        match crate::extensions::base_tag(value) {
            Value::INTEGER => Some(Self::Int(value.payload as i32)),
            Value::FLOAT => Some(Self::Float(value.payload)),
            Value::STATIC_STRING => vm
                .host_string(value)
                .ok()
                .flatten()
                .map(|text| Self::Text(text.to_vec())),
            _ => None,
        }
    }
    /// Push onto `vm`: a string goes to the program's runtime storage, tag
    /// `0x9801` (§4.1).
    pub fn push(&self, vm: &mut Vm) -> Result<(), VmError> {
        let value = match self {
            Self::Int(value) => Value::integer(*value),
            Self::Float(bits) => Value {
                payload: *bits,
                tag: Value::FLOAT,
            },
            Self::Text(text) => vm.intern(text)?,
        };
        vm.value_stack.push(value)
    }
    /// §4.1's table: this value as class `class`, or `None` for a failed
    /// operand. An `O` operand still has to be tested for life by the host.
    pub fn coerce(&self, class: Class) -> Option<Self> {
        match (class, self) {
            (Class::I, Self::Int(_))
            | (Class::F, Self::Float(_))
            | (Class::N | Class::K, Self::Int(_))
            | (Class::N, Self::Float(_))
            | (Class::K | Class::S, Self::Text(_))
            | (Class::O | Class::P, Self::Int(_))
            | (Class::A, _) => Some(self.clone()),
            (Class::I, Self::Float(bits)) => Some(Self::Int(f32::from_bits(*bits) as i32)),
            (Class::F, Self::Int(value)) => Some(Self::float(*value as f32)),
            _ => None,
        }
    }
}
/// The default a failed operand pushes for return class `ret` (§4.1): `None`
/// for a word that pushes nothing.
pub fn failed_default(row: &SfallRow) -> Option<SfallValue> {
    row.push.then(|| match row.ret {
        Some(Class::F) => SfallValue::float(0.0),
        Some(Class::S) => SfallValue::Text(Vec::new()),
        _ => SfallValue::Int(0),
    })
}
/// The result of a P2 or X row (§4.10): int 0, or the empty string for
/// return class `S`.
pub fn stub_default(row: &SfallRow) -> Option<SfallValue> {
    row.push.then(|| match row.ret {
        Some(Class::S) => SfallValue::Text(Vec::new()),
        _ => SfallValue::Int(0),
    })
}
/// Pop `row`'s operands. They come back in public-call order: the call's
/// last argument is popped first (SPEC-024b §2). Underflow stays a VM error.
pub fn pop_operands(vm: &mut Vm, row: &SfallRow) -> Result<Vec<Value>, VmError> {
    let mut values = Vec::new();
    for _ in 0..row.pops.unwrap_or(0) {
        values.push(vm.value_stack.pop()?);
    }
    values.reverse();
    Ok(values)
}
