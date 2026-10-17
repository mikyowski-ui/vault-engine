//! SPEC-005 stack ABI. Host-specific effects live behind the checked call boundary.
use crate::{Value, Vm, VmError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    Integer,
    String,
    Float,
    Unchecked,
    Optional,
    Filename,
    FillColor,
    TextColor,
    IntegerOrString,
    Previous,
    ScrollFourth,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushKind {
    None,
    Value,
    Text,
}
#[derive(Debug, Clone, Copy)]
pub struct OpcodeSpec {
    pub word: u16,
    pub name: &'static str,
    pub operands: &'static [Rule],
    pub push: PushKind,
    pub registered: bool,
    pub variable: bool,
}
include!(concat!(env!("OUT_DIR"), "/extension_catalog.rs"));
pub fn opcode(word: u16) -> Option<&'static OpcodeSpec> {
    EXTENSIONS.get(usize::from(word.checked_sub(0x804c)?))
}
pub fn base_tag(value: Value) -> u16 {
    value.tag & 0xf7ff
}
#[derive(Debug, Clone, PartialEq)]
pub struct Argument {
    pub value: Value,
    pub text: Option<Vec<u8>>,
    pub channel: Option<f32>,
}
impl Argument {
    pub fn int(&self) -> i32 {
        self.value.payload as i32
    }
    pub fn uint(&self) -> u32 {
        self.value.payload
    }
}
#[derive(Debug, Clone)]
pub struct HostCall {
    pub spec: &'static OpcodeSpec,
    pub args: Vec<Argument>,
}
impl HostCall {
    pub fn int(&self, index: usize) -> i32 {
        self.args[index].int()
    }
    pub fn uint(&self, index: usize) -> u32 {
        self.args[index].uint()
    }
    pub fn text(&self, index: usize) -> Option<&[u8]> {
        self.args[index].text.as_deref()
    }
}
fn checked(vm: &Vm, value: Value, rule: Rule) -> Result<Argument, VmError> {
    let tag = base_tag(value);
    let mut arg = Argument {
        value,
        text: None,
        channel: None,
    };
    let bad = || VmError::Type(value.tag);
    let resolve = || vm.host_string(value).map(|text| text.map(<[u8]>::to_vec));
    match rule {
        Rule::Integer => {
            if tag != Value::INTEGER {
                return Err(bad());
            }
        }
        Rule::Float => {
            if tag != Value::FLOAT {
                return Err(bad());
            }
        }
        Rule::String => {
            if tag != Value::STATIC_STRING {
                return Err(bad());
            }
            arg.text = resolve()?;
        }
        Rule::Unchecked => {}
        Rule::Optional => {
            if tag == Value::STATIC_STRING {
                arg.text = resolve()?;
            }
        }
        Rule::Filename | Rule::Previous | Rule::ScrollFourth => {
            if tag == Value::STATIC_STRING {
                arg.text = resolve()?;
            } else if tag != Value::INTEGER
                || !(value.payload == 0
                    || (rule == Rule::ScrollFourth && value.payload == u32::MAX))
            {
                return Err(bad());
            }
        }
        Rule::IntegerOrString => {
            if tag == Value::STATIC_STRING {
                arg.text = resolve()?;
            } else if tag != Value::INTEGER {
                return Err(bad());
            }
        }
        Rule::FillColor | Rule::TextColor => {
            arg.channel = Some(if tag == Value::INTEGER {
                match value.payload {
                    0 => 0.0,
                    1 if rule == Rule::FillColor => 1.0,
                    _ => return Err(bad()),
                }
            } else {
                f32::from_bits(value.payload)
            });
            let scaled = arg.channel.unwrap() * 31.0;
            if !scaled.is_finite() || !(-2147483648.0..2147483648.0).contains(&(scaled as f64)) {
                return Err(VmError::Arithmetic);
            }
        }
    }
    Ok(arg)
}
pub fn decode(vm: &mut Vm, word: u16) -> Result<HostCall, VmError> {
    let spec = opcode(word).ok_or(VmError::Opcode(word))?;
    if !spec.registered || word == 0x8091 {
        return Err(VmError::Opcode(word));
    }
    if spec.variable {
        let count = vm.value_stack.pop()?;
        let first = checked(vm, count, Rule::Integer)?;
        if first.int() < 2 {
            return Err(VmError::Host(
                "region count must include points and a name".into(),
            ));
        }
        let mut args = vec![first];
        for _ in 0..count.payload / 2 {
            for _ in 0..2 {
                let value = vm.value_stack.pop()?;
                args.push(checked(vm, value, Rule::Integer)?);
            }
        }
        if count.payload % 2 == 0 {
            return Err(VmError::Host("even region count leaves no name".into()));
        }
        let value = vm.value_stack.pop()?;
        if base_tag(value) == Value::INTEGER && value.payload != 0 {
            return Err(VmError::Type(value.tag));
        }
        let text = vm.host_string(value)?.map(<[u8]>::to_vec);
        args.push(Argument {
            value,
            text,
            channel: None,
        });
        return Ok(HostCall { spec, args });
    }
    // These wrappers gather their declared operands before checking their types;
    // the variable-arity region operation has the documented earlier boundaries.
    let mut values = Vec::new();
    for _ in spec.operands {
        values.push(vm.value_stack.pop()?);
    }
    let args = values
        .into_iter()
        .zip(spec.operands)
        .map(|(value, rule)| checked(vm, value, *rule))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(HostCall { spec, args })
}
