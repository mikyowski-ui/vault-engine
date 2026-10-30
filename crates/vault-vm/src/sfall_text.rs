//! SPEC-024b §4.6: the string and conversion helpers. None of them depends
//! on a locale, and the float text comes from Rust's exact formatter.
use crate::sfall::SfallValue;

/// `string_format`'s limit on its format, in bytes.
pub const FORMAT_LIMIT: usize = 1024;

fn start_of_number(text: &[u8]) -> usize {
    text.iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(text.len())
}
fn digits(text: &[u8], from: usize) -> usize {
    text[from..]
        .iter()
        .position(|byte| !byte.is_ascii_digit())
        .map_or(text.len(), |at| from + at)
}
/// Skips ASCII whitespace, takes one sign, reads digits up to the first byte
/// that does not fit. No digits gives 0, and the value saturates.
pub fn atoi(text: &[u8]) -> i32 {
    let mut at = start_of_number(text);
    let negative = text.get(at) == Some(&b'-');
    if matches!(text.get(at), Some(b'+' | b'-')) {
        at += 1;
    }
    let mut value = 0i64;
    for byte in &text[at..digits(text, at)] {
        value = (value * 10 + i64::from(byte - b'0')).min(1 << 32);
    }
    let value = if negative { -value } else { value };
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}
/// As [`atoi`], with a fraction and an exponent.
pub fn atof(text: &[u8]) -> f32 {
    let start = start_of_number(text);
    let mut at = start;
    if matches!(text.get(at), Some(b'+' | b'-')) {
        at += 1;
    }
    let whole = digits(text, at);
    let mut end = whole;
    let mut any = whole > at;
    if text.get(end) == Some(&b'.') {
        let fraction = digits(text, end + 1);
        any |= fraction > end + 1;
        end = fraction;
    }
    if !any {
        return 0.0;
    }
    if matches!(text.get(end), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(text.get(end + 1), Some(b'+' | b'-')));
        let exponent = digits(text, end + 1 + sign);
        if exponent > end + 1 + sign {
            end = exponent;
        }
    }
    std::str::from_utf8(&text[start..end])
        .ok()
        .and_then(|number| number.parse().ok())
        .unwrap_or(0.0)
}
/// `substr(text, start, length)`: a negative start counts from the end, a
/// negative length omits that many bytes from the end, and length 0 runs to
/// the end. Positions are then clamped to the text.
pub fn substr(text: &[u8], start: i32, length: i32) -> &[u8] {
    let size = text.len() as i64;
    let start = i64::from(start);
    let from = if start < 0 { size + start } else { start }.clamp(0, size);
    let to = match i64::from(length) {
        0 => size,
        length if length < 0 => size + length,
        length => from + length,
    }
    .clamp(from, size);
    &text[from as usize..to as usize]
}
/// `string_find`: the first match at or after `position`, or -1. A negative
/// position counts from the end.
pub fn find(haystack: &[u8], needle: &[u8], position: i32) -> i32 {
    let size = haystack.len() as i64;
    let position = i64::from(position);
    let from = if position < 0 {
        (size + position).max(0)
    } else {
        position
    };
    if from > size {
        return -1;
    }
    let from = from as usize;
    if needle.is_empty() {
        return from as i32;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map_or(-1, |at| (from + at) as i32)
}
/// `string_to_case`: 0 lowers, and any other mode uppercases, A-Z and a-z
/// only.
pub fn to_case(text: &[u8], mode: i32) -> Vec<u8> {
    if mode == 0 {
        text.to_ascii_lowercase()
    } else {
        text.to_ascii_uppercase()
    }
}
/// `string_compare`: whether the strings match with ASCII case folded.
pub fn compare(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}
/// `string_split`: the pieces of `text`; an empty separator gives single
/// bytes.
pub fn split(text: &[u8], separator: &[u8]) -> Vec<Vec<u8>> {
    if separator.is_empty() {
        return text.iter().map(|byte| vec![*byte]).collect();
    }
    let mut pieces = Vec::new();
    let mut from = 0;
    let mut at = 0;
    while at + separator.len() <= text.len() {
        if &text[at..at + separator.len()] == separator {
            pieces.push(text[from..at].to_vec());
            at += separator.len();
            from = at;
        } else {
            at += 1;
        }
    }
    pieces.push(text[from..].to_vec());
    pieces
}

#[derive(Debug, Default, Clone, Copy)]
struct Spec {
    left: bool,
    plus: bool,
    space: bool,
    alternate: bool,
    zero: bool,
    width: usize,
    precision: Option<usize>,
}
fn number_from(text: &[u8], at: &mut usize) -> Option<usize> {
    let from = *at;
    let mut value = 0usize;
    while let Some(byte) = text.get(*at).filter(|byte| byte.is_ascii_digit()) {
        value = (value * 10 + usize::from(byte - b'0')).min(4096);
        *at += 1;
    }
    (*at > from).then_some(value)
}
/// Pads `sign + prefix + body` to the width, with `0` after the prefix only
/// when `zero` allows it.
fn pad(spec: &Spec, head: &str, body: &str, zero: bool) -> Vec<u8> {
    let length = head.len() + body.len();
    let fill = spec.width.saturating_sub(length);
    let mut out = Vec::with_capacity(length + fill);
    if spec.left {
        out.extend_from_slice(head.as_bytes());
        out.extend_from_slice(body.as_bytes());
        out.resize(out.len() + fill, b' ');
    } else if zero && spec.zero {
        out.extend_from_slice(head.as_bytes());
        out.resize(out.len() + fill, b'0');
        out.extend_from_slice(body.as_bytes());
    } else {
        out.resize(fill, b' ');
        out.extend_from_slice(head.as_bytes());
        out.extend_from_slice(body.as_bytes());
    }
    out
}
fn sign(spec: &Spec, negative: bool) -> &'static str {
    if negative {
        "-"
    } else if spec.plus {
        "+"
    } else if spec.space {
        " "
    } else {
        ""
    }
}
fn with_precision(spec: &Spec, digits: String) -> String {
    match spec.precision {
        Some(0) if digits == "0" => String::new(),
        Some(precision) if digits.len() < precision => {
            "0".repeat(precision - digits.len()) + &digits
        }
        _ => digits,
    }
}
fn signed(spec: &Spec, value: i32) -> Vec<u8> {
    let digits = with_precision(spec, i64::from(value).abs().to_string());
    pad(
        spec,
        sign(spec, value < 0),
        &digits,
        spec.precision.is_none(),
    )
}
fn unsigned(spec: &Spec, value: u32, conversion: u8) -> Vec<u8> {
    let (digits, head) = match conversion {
        b'o' => (format!("{value:o}"), if spec.alternate { "0" } else { "" }),
        b'x' => (
            format!("{value:x}"),
            if spec.alternate && value != 0 {
                "0x"
            } else {
                ""
            },
        ),
        b'X' => (
            format!("{value:X}"),
            if spec.alternate && value != 0 {
                "0X"
            } else {
                ""
            },
        ),
        _ => (value.to_string(), ""),
    };
    let digits = with_precision(spec, digits);
    pad(spec, head, &digits, spec.precision.is_none())
}
/// `%e`'s text, with C's sign and two-digit exponent.
fn exponent_text(value: f64, precision: usize, upper: bool) -> String {
    let text = format!("{value:.precision$e}");
    let (mantissa, power) = text.split_once('e').expect("Rust writes an exponent");
    let power: i32 = power.parse().expect("an exponent");
    let mark = if upper { 'E' } else { 'e' };
    let sign = if power < 0 { '-' } else { '+' };
    format!("{mantissa}{mark}{sign}{:02}", power.abs())
}
/// `%g`: the shorter of `%f` and `%e`, by C's rule on the exponent.
fn general_text(value: f64, precision: usize, upper: bool, alternate: bool) -> String {
    let precision = precision.max(1);
    if value == 0.0 {
        return if alternate {
            format!("{:.*}", precision - 1, 0.0)
        } else {
            "0".to_owned()
        };
    }
    let scientific = exponent_text(value, precision - 1, upper);
    let mark = if upper { 'E' } else { 'e' };
    let (mantissa, power) = scientific.split_once(mark).expect("an exponent");
    let power: i32 = power.parse().expect("an exponent");
    let trim = |text: &str| -> String {
        if alternate || !text.contains('.') {
            text.to_owned()
        } else {
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        }
    };
    if power >= -4 && power < precision as i32 {
        let places = (precision as i32 - 1 - power) as usize;
        trim(&format!("{value:.places$}"))
    } else {
        format!(
            "{}{mark}{}{:02}",
            trim(mantissa),
            if power < 0 { '-' } else { '+' },
            power.abs()
        )
    }
}
fn floating(spec: &Spec, value: f32, conversion: u8) -> Vec<u8> {
    let number = f64::from(value);
    let upper = conversion.is_ascii_uppercase();
    let head = sign(spec, value.is_sign_negative() && !value.is_nan());
    if !value.is_finite() {
        let text = match (value.is_nan(), upper) {
            (true, false) => "nan",
            (true, true) => "NAN",
            (false, false) => "inf",
            (false, true) => "INF",
        };
        return pad(spec, head, text, false);
    }
    let magnitude = number.abs();
    let precision = spec.precision.unwrap_or(6);
    let mut body = match conversion {
        b'f' | b'F' => format!("{magnitude:.precision$}"),
        b'e' | b'E' => exponent_text(magnitude, precision, upper),
        _ => general_text(magnitude, precision, upper, spec.alternate),
    };
    if spec.alternate && precision == 0 && !body.contains('.') {
        body.push('.');
    }
    pad(spec, head, &body, true)
}
fn plain_text(value: &SfallValue) -> Vec<u8> {
    match value {
        SfallValue::Int(value) => value.to_string().into_bytes(),
        SfallValue::Float(bits) => crate::float_text(f32::from_bits(*bits)),
        SfallValue::Text(text) => text.clone(),
    }
}
/// A number operand of a `%` conversion; a string is `None`.
fn int_of(value: &SfallValue) -> Option<i32> {
    match value {
        SfallValue::Text(_) => None,
        value => Some(value.int()),
    }
}
/// The format strings of `sprintf` and `string_format`. Each value serves
/// one conversion, in order; a missing value leaves its conversion's text
/// empty; a number conversion given a string fails the operand (`None`). An
/// unknown conversion is copied as written.
pub fn format(format: &[u8], values: &[SfallValue]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(format.len());
    let mut next = values.iter();
    let mut at = 0;
    while at < format.len() {
        if format[at] != b'%' {
            out.push(format[at]);
            at += 1;
            continue;
        }
        let begin = at;
        at += 1;
        let mut spec = Spec::default();
        while let Some(flag) = format.get(at) {
            match flag {
                b'-' => spec.left = true,
                b'+' => spec.plus = true,
                b' ' => spec.space = true,
                b'#' => spec.alternate = true,
                b'0' => spec.zero = true,
                _ => break,
            }
            at += 1;
        }
        spec.width = number_from(format, &mut at).unwrap_or(0);
        if format.get(at) == Some(&b'.') {
            at += 1;
            spec.precision = Some(number_from(format, &mut at).unwrap_or(0));
        }
        while matches!(format.get(at), Some(b'h' | b'l' | b'L')) {
            at += 1;
        }
        let Some(conversion) = format.get(at).copied() else {
            out.extend_from_slice(&format[begin..]);
            break;
        };
        at += 1;
        match conversion {
            b'%' => out.push(b'%'),
            b'd' | b'i' | b'u' | b'x' | b'X' | b'o' | b'c' | b'f' | b'F' | b'e' | b'E' | b'g'
            | b'G' | b's' => {
                let Some(value) = next.next() else {
                    continue;
                };
                match conversion {
                    b's' => {
                        let text = plain_text(value);
                        let cut = spec.precision.unwrap_or(text.len()).min(text.len());
                        let fill = spec.width.saturating_sub(cut);
                        if !spec.left {
                            out.resize(out.len() + fill, b' ');
                        }
                        out.extend_from_slice(&text[..cut]);
                        if spec.left {
                            out.resize(out.len() + fill, b' ');
                        }
                    }
                    b'c' => {
                        let byte = int_of(value)? as u8;
                        let fill = spec.width.saturating_sub(1);
                        if !spec.left {
                            out.resize(out.len() + fill, b' ');
                        }
                        out.push(byte);
                        if spec.left {
                            out.resize(out.len() + fill, b' ');
                        }
                    }
                    b'd' | b'i' => out.extend(signed(&spec, int_of(value)?)),
                    b'u' | b'x' | b'X' | b'o' => {
                        out.extend(unsigned(&spec, int_of(value)? as u32, conversion));
                    }
                    _ => {
                        let number = match value {
                            SfallValue::Text(_) => return None,
                            value => value.number(),
                        };
                        out.extend(floating(&spec, number, conversion));
                    }
                }
            }
            _ => out.extend_from_slice(&format[begin..at]),
        }
    }
    Some(out)
}
