use crate::{Value, Vm, VmError};
use std::cmp::Ordering;

fn text(vm: &Vm, value: Value) -> Result<Vec<u8>, VmError> {
    if value.is_string() {
        return Ok(vm.string(value)?.to_vec());
    }
    match value.tag {
        Value::INTEGER => Ok((value.payload as i32).to_string().into_bytes()),
        Value::FLOAT => Ok(float_text(f32::from_bits(value.payload))),
        _ => Err(VmError::Type(value.tag)),
    }
}
/// Float text with exactly five fractional digits; under the SPEC-004 §3.5
/// safe rule, NaN is `nan` and the infinities are `inf` and `-inf`.
pub fn float_text(value: f32) -> Vec<u8> {
    if value.is_nan() {
        b"nan".to_vec()
    } else if value == f32::INFINITY {
        b"inf".to_vec()
    } else if value == f32::NEG_INFINITY {
        b"-inf".to_vec()
    } else {
        format!("{value:.5}").into_bytes()
    }
}
fn numeric(value: Value) -> Result<f32, VmError> {
    match value.tag {
        Value::INTEGER => Ok(value.payload as i32 as f32),
        Value::FLOAT => Ok(f32::from_bits(value.payload)),
        _ => Err(VmError::Type(value.tag)),
    }
}
/// Truncating conversion; an out-of-range or NaN value gives the x86
/// integer-indefinite `0x80000000` (SPEC-004 §3.5 safe rule).
fn float_int(value: f32) -> i32 {
    if value.is_nan() || !(-2147483648.0..2147483648.0).contains(&f64::from(value)) {
        return i32::MIN;
    }
    value.trunc() as i32
}
fn truth(value: Value, float_bits: bool) -> Result<bool, VmError> {
    if value.is_string() {
        return Ok(true);
    }
    match value.tag {
        Value::INTEGER => Ok(value.payload != 0),
        Value::FLOAT => Ok(value.payload & if float_bits { u32::MAX } else { 0x7fffffff } != 0),
        _ => Err(VmError::Type(value.tag)),
    }
}
fn bits(value: Value) -> i32 {
    if value.tag == Value::FLOAT {
        float_int(value.payload as i32 as f32)
    } else {
        value.payload as i32
    }
}
pub(crate) fn execute(vm: &mut Vm, word: u16) -> Result<(), VmError> {
    if word >= 0x8043 {
        let value = vm.value_stack.pop()?;
        let result = match word {
            0x8043 => Value::integer(!(value.payload as i32)),
            // Both string tags are rejected (safe rule for the dynamic tag).
            0x8044 => match value.tag {
                _ if value.is_string() => return Err(VmError::Type(value.tag)),
                Value::FLOAT => Value::integer(float_int(f32::from_bits(value.payload))),
                _ => value,
            },
            0x8045 => Value::integer(i32::from(value.payload == 0)),
            0x8046 => Value::integer((value.payload as i32).wrapping_neg()),
            _ => return Err(VmError::Opcode(word)),
        };
        return vm.value_stack.push(result);
    }
    let b = vm.value_stack.pop()?;
    let a = vm.value_stack.pop()?;
    if (0x8033..=0x8038).contains(&word) {
        let order = if a.is_string() || b.is_string() {
            Some(text(vm, a)?.cmp(&text(vm, b)?))
        } else if a.tag == Value::INTEGER && b.tag == Value::INTEGER {
            Some((a.payload as i32).cmp(&(b.payload as i32)))
        } else {
            numeric(a)?.partial_cmp(&numeric(b)?)
        };
        let result = match word {
            0x8033 => order == Some(Ordering::Equal),
            0x8034 => order != Some(Ordering::Equal),
            0x8035 => matches!(order, Some(Ordering::Less | Ordering::Equal)),
            0x8036 => matches!(order, Some(Ordering::Greater | Ordering::Equal)),
            0x8037 => order == Some(Ordering::Less),
            _ => order == Some(Ordering::Greater),
        };
        return vm.value_stack.push(Value::integer(i32::from(result)));
    }
    let result = match word {
        0x8039 if a.is_string() || b.is_string() => {
            let mut value = text(vm, a)?;
            let right = text(vm, b)?;
            if value.len() + right.len() > 8 * 1024 * 1024 {
                return Err(VmError::StringLimit);
            }
            value.extend(right);
            vm.intern(&value)?
        }
        0x8039 => {
            if a.tag == Value::INTEGER && b.tag == Value::INTEGER {
                let (a, b) = (i64::from(a.payload as i32), i64::from(b.payload as i32));
                if (b <= 0 || a < i64::from(i32::MAX) - b)
                    && (b >= 0 || a >= i64::from(i32::MIN) - b)
                {
                    Value::integer((a + b) as i32)
                } else {
                    Value::float(a as f32 + b as f32)
                }
            } else {
                Value::float(numeric(a)? + numeric(b)?)
            }
        }
        // A string on the left would consume both operands and push nothing;
        // the safe rule raises a type error instead.
        0x803a..=0x803c => {
            if a.tag != Value::INTEGER && a.tag != Value::FLOAT {
                return Err(VmError::Type(a.tag));
            }
            if a.tag == Value::FLOAT || b.tag == Value::FLOAT {
                let left = numeric(a)?;
                let right = if b.tag == Value::FLOAT {
                    numeric(b)?
                } else {
                    b.payload as i32 as f32
                };
                if word == 0x803c && right == 0.0 {
                    return Err(VmError::DivisionByZero);
                }
                Value::float(match word {
                    0x803a => left - right,
                    0x803b => left * right,
                    _ => left / right,
                })
            } else {
                let (a, b) = (a.payload as i32, b.payload as i32);
                if word == 0x803c && b == 0 {
                    return Err(VmError::DivisionByZero);
                }
                // Results wrap modulo 2^32 (safe rule), so MIN / -1 is MIN.
                Value::integer(match word {
                    0x803a => a.wrapping_sub(b),
                    0x803b => a.wrapping_mul(b),
                    _ => a.wrapping_div(b),
                })
            }
        }
        0x803d => {
            if a.tag != Value::INTEGER && a.tag != Value::FLOAT {
                return Err(VmError::Type(a.tag));
            }
            if a.tag == Value::FLOAT || b.tag == Value::FLOAT {
                return Err(VmError::Type(Value::FLOAT));
            }
            let divisor = b.payload as i32;
            if divisor == 0 {
                return Err(VmError::DivisionByZero);
            }
            Value::integer(
                (a.payload as i32)
                    .checked_rem(divisor)
                    .ok_or(VmError::Arithmetic)?,
            )
        }
        0x803e | 0x803f => {
            let left = truth(a, word == 0x803e && a.tag == Value::FLOAT && b.is_string())?;
            let right = truth(b, false)?;
            Value::integer(i32::from(if word == 0x803e {
                left && right
            } else {
                left || right
            }))
        }
        0x8040..=0x8042 => {
            if a.tag != Value::INTEGER && a.tag != Value::FLOAT {
                return Err(VmError::Type(a.tag));
            }
            let (a, b) = (bits(a), bits(b));
            Value::integer(match word {
                0x8040 => a & b,
                0x8041 => a | b,
                _ => a ^ b,
            })
        }
        _ => return Err(VmError::Opcode(word)),
    };
    vm.value_stack.push(result)
}
