//! SPEC-024b §4.7: the math words. `sqrt` is IEEE-754's. The others must not
//! reach a platform math library, whose last bit differs between systems, so
//! they are computed here from `+ - * /` alone, which IEEE-754 pins exactly,
//! in binary64 and rounded to binary32 at the end. Every platform gives the
//! same bits. A NaN result is always the one quiet NaN.
use crate::VmError;
use crate::sfall::SfallValue;
use std::f64::consts::{FRAC_2_PI, FRAC_PI_2, FRAC_PI_4, LOG2_E, PI, SQRT_2};

/// π/2 in two parts, so that a reduction loses no bits below 2^30.
const PIO2_HIGH: f64 = 1.570_796_326_734_125_6;
const PIO2_LOW: f64 = 6.077_100_506_506_192e-11;
const LN_2_HIGH: f64 = 6.931_471_803_691_238e-1;
const LN_2_LOW: f64 = 1.908_214_929_270_587_7e-10;

/// The binary32 result: a NaN loses its platform's sign and payload.
fn finish(value: f64) -> f32 {
    if value.is_nan() {
        f32::NAN
    } else {
        value as f32
    }
}
fn round_to_integer(value: f64) -> f64 {
    let whole = value.trunc();
    let rest = value - whole;
    if rest >= 0.5 {
        whole + 1.0
    } else if rest <= -0.5 {
        whole - 1.0
    } else {
        whole
    }
}
/// `sin(x)` on |x| ≤ π/4.
fn sine_kernel(x: f64) -> f64 {
    let square = x * x;
    let mut term = x;
    let mut sum = x;
    for n in 1..=11 {
        let n = f64::from(n);
        term *= -square / ((2.0 * n) * (2.0 * n + 1.0));
        sum += term;
    }
    sum
}
/// `cos(x)` on |x| ≤ π/4.
fn cosine_kernel(x: f64) -> f64 {
    let square = x * x;
    let mut term = 1.0;
    let mut sum = 1.0;
    for n in 1..=11 {
        let n = f64::from(n);
        term *= -square / ((2.0 * n - 1.0) * (2.0 * n));
        sum += term;
    }
    sum
}
/// `x` as a quarter-turn count modulo 4 and a remainder within π/4. Past
/// 2^30 the remainder comes from an exact `%`; it is then less accurate,
/// and still the same on every platform.
fn reduce(x: f64) -> (u8, f64) {
    let bounded = if x.abs() < 1_073_741_824.0 {
        x
    } else {
        x % (2.0 * PI)
    };
    let quarters = round_to_integer(bounded * FRAC_2_PI);
    let rest = (bounded - quarters * PIO2_HIGH) - quarters * PIO2_LOW;
    ((quarters % 4.0 + 4.0) as u8 % 4, rest)
}
fn sine_f64(x: f64) -> f64 {
    let (quarter, rest) = reduce(x);
    match quarter {
        0 => sine_kernel(rest),
        1 => cosine_kernel(rest),
        2 => -sine_kernel(rest),
        _ => -cosine_kernel(rest),
    }
}
fn cosine_f64(x: f64) -> f64 {
    let (quarter, rest) = reduce(x);
    match quarter {
        0 => cosine_kernel(rest),
        1 => -sine_kernel(rest),
        2 => -cosine_kernel(rest),
        _ => sine_kernel(rest),
    }
}
pub fn sine(x: f32) -> f32 {
    if !x.is_finite() {
        return f32::NAN;
    }
    finish(sine_f64(f64::from(x)))
}
pub fn cosine(x: f32) -> f32 {
    if !x.is_finite() {
        return f32::NAN;
    }
    finish(cosine_f64(f64::from(x)))
}
pub fn tangent(x: f32) -> f32 {
    if !x.is_finite() {
        return f32::NAN;
    }
    finish(sine_f64(f64::from(x)) / cosine_f64(f64::from(x)))
}
/// The arctangent of |t| ≤ 1, by the series on a reduced argument: with
/// tan(π/8) = √2 - 1, a |t| above that uses `π/4 + atan((t - 1) / (t + 1))`.
fn arctan_unit(t: f64) -> f64 {
    const TAN_PI_8: f64 = 0.414_213_562_373_095_03;
    if t > TAN_PI_8 {
        return FRAC_PI_4 + arctan_series((t - 1.0) / (t + 1.0));
    }
    arctan_series(t)
}
fn arctan_series(t: f64) -> f64 {
    let square = t * t;
    let mut power = t;
    let mut sum = t;
    for n in 1..=26 {
        power *= -square;
        sum += power / f64::from(2 * n + 1);
    }
    sum
}
fn arctan_f64(value: f64) -> f64 {
    let magnitude = value.abs();
    let angle = if magnitude <= 1.0 {
        arctan_unit(magnitude)
    } else {
        FRAC_PI_2 - arctan_unit(1.0 / magnitude)
    };
    if value < 0.0 { -angle } else { angle }
}
/// `arctan(x, y)` (§4.7): the two-argument arctangent of `x / y`, in the
/// quadrant of its signs; pass y = 1 for the plain form.
pub fn arctan(x: f32, y: f32) -> f32 {
    if x.is_nan() || y.is_nan() {
        return f32::NAN;
    }
    let (top, bottom) = (f64::from(x), f64::from(y));
    let negative = x.is_sign_negative();
    let signed = |angle: f64| if negative { -angle } else { angle };
    let angle = if top == 0.0 {
        signed(if y.is_sign_negative() { PI } else { 0.0 })
    } else if bottom == 0.0 {
        signed(FRAC_PI_2)
    } else if top.is_infinite() || bottom.is_infinite() {
        let corner = match (top.is_infinite(), bottom.is_infinite()) {
            (true, true) if bottom > 0.0 => FRAC_PI_4,
            (true, true) => 3.0 * FRAC_PI_4,
            (true, false) => FRAC_PI_2,
            _ if bottom > 0.0 => 0.0,
            _ => PI,
        };
        signed(corner)
    } else if bottom > 0.0 {
        arctan_f64(top / bottom)
    } else if top > 0.0 {
        arctan_f64(top / bottom) + PI
    } else {
        arctan_f64(top / bottom) - PI
    };
    finish(angle)
}
/// The natural logarithm of a positive finite `x`: `x = m * 2^k` with m
/// within a factor of √2 of 1, then the `atanh` series of `(m-1)/(m+1)`.
fn logarithm_f64(x: f64) -> f64 {
    let bits = x.to_bits();
    let raw = ((bits >> 52) & 0x7ff) as i32;
    let (bits, offset) = if raw == 0 {
        // A subnormal: scale it into the normal range first.
        ((x * 18_014_398_509_481_984.0).to_bits(), -54)
    } else {
        (bits, 0)
    };
    let raw = ((bits >> 52) & 0x7ff) as i32;
    let mut scale = raw - 1023 + offset;
    let mut mantissa = f64::from_bits((bits & !(0x7ff << 52)) | (1023 << 52));
    if mantissa > SQRT_2 {
        mantissa /= 2.0;
        scale += 1;
    }
    let s = (mantissa - 1.0) / (mantissa + 1.0);
    let square = s * s;
    let mut power = s;
    let mut sum = 0.0;
    for n in 0..=16 {
        sum += power / f64::from(2 * n + 1);
        power *= square;
    }
    let k = f64::from(scale);
    k * LN_2_HIGH + (k * LN_2_LOW + 2.0 * sum)
}
pub fn logarithm(x: f32) -> f32 {
    if x.is_nan() || x < 0.0 {
        f32::NAN
    } else if x == 0.0 {
        f32::NEG_INFINITY
    } else if x.is_infinite() {
        f32::INFINITY
    } else {
        finish(logarithm_f64(f64::from(x)))
    }
}
/// `2^k` times `value`, by steps that cannot overflow on the way.
fn scale(mut value: f64, mut k: i32) -> f64 {
    while k > 1000 {
        value *= f64::from_bits((1023 + 1000) << 52);
        k -= 1000;
    }
    while k < -1000 {
        value *= f64::from_bits((1023 - 1000) << 52);
        k += 1000;
    }
    value * f64::from_bits(((1023 + i64::from(k)) as u64) << 52)
}
fn exponent_f64(x: f64) -> f64 {
    if x > 800.0 {
        return f64::INFINITY;
    }
    if x < -800.0 {
        return 0.0;
    }
    let k = round_to_integer(x * LOG2_E);
    let rest = (x - k * LN_2_HIGH) - k * LN_2_LOW;
    let mut term = 1.0;
    let mut sum = 1.0;
    for n in 1..=18 {
        term *= rest / f64::from(n);
        sum += term;
    }
    scale(sum, k as i32)
}
pub fn exponent(x: f32) -> f32 {
    if x.is_nan() {
        f32::NAN
    } else {
        finish(exponent_f64(f64::from(x)))
    }
}
pub fn square_root(x: f32) -> f32 {
    let root = x.sqrt();
    if root.is_nan() { f32::NAN } else { root }
}
/// `ceil`: the least integer not below x, saturating.
pub fn ceil(x: f32) -> i32 {
    x.ceil() as i32
}
/// `floor2`: toward negative infinity, saturating.
pub fn floor(x: f32) -> i32 {
    x.floor() as i32
}
/// `round`: the nearest integer, ties away from zero, saturating.
pub fn round(x: f32) -> i32 {
    x.round() as i32
}
fn integer_power(base: i32, exponent: i32) -> i32 {
    if exponent < 0 {
        return match base {
            1 => 1,
            -1 if exponent % 2 == 0 => 1,
            -1 => -1,
            _ => 0,
        };
    }
    let (mut result, mut square, mut left) = (1i32, base, exponent);
    while left > 0 {
        if left & 1 == 1 {
            result = result.wrapping_mul(square);
        }
        square = square.wrapping_mul(square);
        left >>= 1;
    }
    result
}
/// `base ^ exponent` by repeated squaring, in binary64.
fn float_integer_power(base: f64, exponent: i32) -> f64 {
    let (mut result, mut square, mut left) = (1.0f64, base, u64::from(exponent.unsigned_abs()));
    while left > 0 {
        if left & 1 == 1 {
            result *= square;
        }
        square *= square;
        left >>= 1;
    }
    if exponent < 0 { 1.0 / result } else { result }
}
/// `^` (§4.7): two ints give an int, wrapping at 32 bits; an int exponent
/// allows a negative base; a float exponent with a negative base gives NaN.
/// Both operands are of class `N`.
pub fn power(base: &SfallValue, exponent: &SfallValue) -> SfallValue {
    match (base, exponent) {
        (SfallValue::Int(base), SfallValue::Int(exponent)) => {
            SfallValue::Int(integer_power(*base, *exponent))
        }
        (base, SfallValue::Int(exponent)) => SfallValue::float(finish(float_integer_power(
            f64::from(base.number()),
            *exponent,
        ))),
        (base, exponent) => {
            let (base, exponent) = (f64::from(base.number()), f64::from(exponent.number()));
            SfallValue::float(if base.is_nan() || exponent.is_nan() || base < 0.0 {
                f32::NAN
            } else if exponent == 0.0 {
                1.0
            } else if base == 0.0 {
                if exponent > 0.0 { 0.0 } else { f32::INFINITY }
            } else if base.is_infinite() {
                if exponent > 0.0 { f32::INFINITY } else { 0.0 }
            } else {
                finish(exponent_f64(exponent * logarithm_f64(base)))
            })
        }
    }
}
/// `div` (§4.7): two ints divide as unsigned 32-bit values, and the result
/// bits come back as an int; with a float it is ordinary signed division.
/// Division by zero follows SPEC-004 §3.8.
pub fn divide(dividend: &SfallValue, divisor: &SfallValue) -> Result<SfallValue, VmError> {
    if let (SfallValue::Int(dividend), SfallValue::Int(divisor)) = (dividend, divisor) {
        let (dividend, divisor) = (*dividend as u32, *divisor as u32);
        return dividend
            .checked_div(divisor)
            .map(|quotient| SfallValue::Int(quotient as i32))
            .ok_or(VmError::DivisionByZero);
    }
    let bottom = divisor.number();
    if bottom == 0.0 {
        return Err(VmError::DivisionByZero);
    }
    Ok(SfallValue::float(dividend.number() / bottom))
}
