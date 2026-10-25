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
/// An integer-operand row of [`FALLOUT2`].
const fn fallout2(
    word: u16,
    name: &'static str,
    operands: &'static [Rule],
    push: PushKind,
) -> OpcodeSpec {
    OpcodeSpec {
        word,
        name,
        operands,
        push,
        registered: true,
        variable: false,
    }
}
const I: Rule = Rule::Integer;
/// SPEC-017's rows under the Fallout 2 profile, in ascending word order:
/// the six SPEC-005 words §3.2 reassigns and the four words of §4.1.
/// Operands are listed in pop order, first popped first. Every other word
/// of `0x804C..=0x8155` keeps its SPEC-005 row (§3.1), and the 42 reserved
/// words `0x815A..=0x8183` have no row: executing one is the
/// undefined-instruction VM error.
pub static FALLOUT2: &[OpcodeSpec] = &[
    // mark_area_known(mark type, index, state)
    fallout2(0x80b2, "mark_area_known", &[I, I, I], PushKind::None),
    // tile_in_tile_rect(upper-left, upper-right, lower-left, lower-right, test)
    fallout2(
        0x80cf,
        "tile_in_tile_rect",
        &[I, I, I, I, I],
        PushKind::Value,
    ),
    // metarule3(sub-function, p1, p2, p3): the parameters are unchecked.
    fallout2(
        0x80e1,
        "metarule3",
        &[Rule::Unchecked, Rule::Unchecked, Rule::Unchecked, I],
        PushKind::Value,
    ),
    // set_map_music(map number, track name)
    fallout2(0x80e2, "set_map_music", &[Rule::String, I], PushKind::None),
    // wm_area_set_pos(area, x, y)
    fallout2(0x80e5, "wm_area_set_pos", &[I, I, I], PushKind::None),
    // set_exit_grids(marked elevation, map, elevation, hex, orientation)
    fallout2(0x80e6, "set_exit_grids", &[I, I, I, I, I], PushKind::None),
    // inven_cmds(owner, command, index)
    fallout2(0x8156, "inven_cmds", &[I, I, I], PushKind::Value),
    // reg_anim_change_fid(object, art, delay)
    fallout2(0x8157, "reg_anim_change_fid", &[I, I, I], PushKind::None),
    // reg_anim_take_out(object, weapon code, delay)
    fallout2(0x8158, "reg_anim_take_out", &[I, I, I], PushKind::None),
    // reg_anim_turn_towards(object, hex, delay)
    fallout2(0x8159, "reg_anim_turn_towards", &[I, I, I], PushKind::None),
];
/// SPEC-017 §4.1: the reserved words of the Fallout 2 range.
pub const FALLOUT2_RESERVED: std::ops::RangeInclusive<u16> = 0x815a..=0x8183;
/// The host-word row of `word` under the profile whose word set is
/// `words` (SPEC-017 §2.3): under Fallout 2, [`FALLOUT2`]'s row where it
/// has one, otherwise SPEC-005's. A reserved word, or one outside the set,
/// has no row.
pub fn opcode_in(words: &crate::WordSet, word: u16) -> Option<&'static OpcodeSpec> {
    if !words.words.contains(&word) {
        return None;
    }
    if words.fallout2()
        && let Ok(index) = FALLOUT2.binary_search_by_key(&word, |spec| spec.word)
    {
        return FALLOUT2.get(index);
    }
    opcode(word)
}
/// The versioned "enhanced" opcode range. These words are not part of
/// SPEC-004/005: a vanilla host rejects every one of them as an unknown
/// word, and only an enhanced host ([`crate::VmHost::enhanced_opcodes`])
/// dispatches them through [`ENHANCED`].
pub const ENHANCED_RANGE: std::ops::RangeInclusive<u16> = 0x8200..=0x82ff;
/// The version `engine_api_version` (0x8200) reports. It grows whenever an
/// enhanced opcode is added or changes meaning, so a script can test for
/// the features it needs:
/// - 1: `engine_api_version` itself;
/// - 2: the camera requests `0x8210`-`0x8213`.
pub const ENGINE_API_VERSION: i32 = 2;
/// A camera request word with integer `operands`, pushing nothing.
const fn camera(word: u16, name: &'static str, operands: &'static [Rule]) -> OpcodeSpec {
    OpcodeSpec {
        word,
        name,
        operands,
        push: PushKind::None,
        registered: true,
        variable: false,
    }
}
/// The enhanced table, in ascending word order. A word of the range it does
/// not list is unassigned and stays an unknown word.
///
/// The camera requests (`0x8210`-`0x8213`) take integer operands, which a
/// script pushes in the order written: `op_cam_pan_to(tile, duration_ms)`
/// pushes `tile` first. They are popped last first, so in the decoded
/// [`HostCall`] `args[0]` is the last operand (`duration_ms`) and `args[1]`
/// the first. They push nothing. What they do is the host's; a host without
/// a view drops them (see [`execute_enhanced`]).
pub static ENHANCED: &[OpcodeSpec] = &[
    OpcodeSpec {
        word: 0x8200,
        name: "engine_api_version",
        operands: &[],
        push: PushKind::Value,
        registered: true,
        variable: false,
    },
    // op_cam_pan_to(tile_num, duration_ms)
    camera(0x8210, "op_cam_pan_to", &[Rule::Integer, Rule::Integer]),
    // op_cam_set_zoom(zoom_pct, duration_ms)
    camera(0x8211, "op_cam_set_zoom", &[Rule::Integer, Rule::Integer]),
    // op_cam_shake(intensity, duration_ms)
    camera(0x8212, "op_cam_shake", &[Rule::Integer, Rule::Integer]),
    // op_cam_reset(duration_ms)
    camera(0x8213, "op_cam_reset", &[Rule::Integer]),
];
pub fn enhanced_opcode(word: u16) -> Option<&'static OpcodeSpec> {
    if !ENHANCED_RANGE.contains(&word) {
        return None;
    }
    let index = ENHANCED
        .binary_search_by_key(&word, |spec| spec.word)
        .ok()?;
    ENHANCED.get(index)
}
/// Pop and check an enhanced word's operands, as [`decode`] does for the
/// SPEC-005 words. An unassigned word in the range is an unknown word.
pub fn decode_enhanced(vm: &mut Vm, word: u16) -> Result<HostCall, VmError> {
    let spec = enhanced_opcode(word).ok_or(VmError::Opcode(word))?;
    gather(vm, spec)
}
/// Run one enhanced word on a host with no enhanced services of its own
/// (the [`crate::VmHost::enhanced_extension`] default): `engine_api_version`
/// pushes [`ENGINE_API_VERSION`], and a camera request pops and checks its
/// operands and is dropped, as such a host has no view. An unassigned word
/// in the range is still an unknown word.
pub fn execute_enhanced(vm: &mut Vm, word: u16) -> Result<(), VmError> {
    let call = decode_enhanced(vm, word)?;
    match call.spec.word {
        0x8200 => vm.value_stack.push(Value::integer(ENGINE_API_VERSION)),
        0x8210..=0x8213 => Ok(()),
        _ => Err(VmError::Opcode(word)),
    }
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
                // SPEC-005 §3.1 safe rule: a nonfinite channel reads as 0;
                // consumers clamp the truncated channel to 0-31.
                Some(f32::from_bits(value.payload))
                    .filter(|channel| channel.is_finite())
                    .unwrap_or(0.0)
            });
        }
    }
    Ok(arg)
}
/// Pop and check the operands of SPEC-005 host word `word`, the Fallout 1
/// rows.
pub fn decode(vm: &mut Vm, word: u16) -> Result<HostCall, VmError> {
    decode_in(vm, &crate::WordSet::FALLOUT1, word)
}
/// [`decode`] under the profile whose word set is `words`
/// ([`opcode_in`]). A word without a row, such as a reserved Fallout 2
/// word, consumes nothing and is the undefined-instruction error.
pub fn decode_in(vm: &mut Vm, words: &crate::WordSet, word: u16) -> Result<HostCall, VmError> {
    let spec = opcode_in(words, word).ok_or(VmError::Opcode(word))?;
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
    gather(vm, spec)
}
/// Pop `spec`'s declared operands, then check their types. These wrappers
/// gather every operand before checking any; the variable-arity region
/// operation has the documented earlier boundaries.
fn gather(vm: &mut Vm, spec: &'static OpcodeSpec) -> Result<HostCall, VmError> {
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
