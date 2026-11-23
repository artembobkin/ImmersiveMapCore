// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Half floats by their bits: the relief's height texture is `r16float`,
//! and the occlusion reads the same half floats the texture holds.

/// The half float nearest to `value`, ties to the even one, as
/// `vImageConvert_PlanarFtoPlanar16F` rounds in the Swift store. Past the
/// half floats' range is infinity, a NaN stays a quiet NaN.
pub fn f16_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xFF) as i32;
    let mantissa = bits & 0x7F_FFFF;
    if exponent == 0xFF {
        let nan = if mantissa != 0 { 0x200 | (mantissa >> 13) as u16 } else { 0 };
        return sign | 0x7C00 | nan;
    }
    // The exponent rebiased for a half float.
    let biased = exponent - 127 + 15;
    if biased >= 0x1F {
        return sign | 0x7C00;
    }
    if biased <= 0 {
        // A half subnormal, or zero: the significand shifted down to units of 2^-24.
        if biased < -10 {
            return sign;
        }
        let significand = mantissa | 0x80_0000;
        let shift = (14 - biased) as u32;
        let kept = significand >> shift;
        let rest = significand & ((1 << shift) - 1);
        let halfway = 1 << (shift - 1);
        let up = rest > halfway || (rest == halfway && kept & 1 == 1);
        return sign | (kept + up as u32) as u16;
    }
    let kept = ((biased as u32) << 10) | (mantissa >> 13);
    let rest = mantissa & 0x1FFF;
    let up = rest > 0x1000 || (rest == 0x1000 && kept & 1 == 1);
    // A carry out of the mantissa steps the exponent up, to infinity at the top.
    sign | (kept + up as u32) as u16
}

/// Every height as a half float's bits.
pub fn halves(metres: &[f32]) -> Vec<u16> {
    metres.iter().map(|&m| f16_bits(m)).collect()
}

/// 2^-24, the unit of a half float's subnormals.
const SUBNORMAL_UNIT: f32 = 1.0 / 16_777_216.0;

/// A half float's value by its bits, as the Swift `GroundReliefTile.float(halfBits:)`.
#[inline(always)]
pub fn f32_from_f16_bits(bits: u16) -> f32 {
    let sign = ((bits & 0x8000) as u32) << 16;
    let exponent = ((bits >> 10) & 0x1F) as u32;
    let mantissa = (bits & 0x3FF) as u32;
    if exponent == 0 {
        // A subnormal, or zero: the mantissa over 2^24.
        let signum: f32 = if sign == 0 { 1.0 } else { -1.0 };
        return signum * mantissa as f32 * SUBNORMAL_UNIT;
    }
    if exponent == 0x1F {
        return f32::from_bits(sign | 0x7F80_0000 | (mantissa << 13));
    }
    f32::from_bits(sign | ((exponent + 112) << 23) | (mantissa << 13))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_floats_read_back() {
        // 1.0, -2.0 and 1000.0 as half floats (the Swift testHalfFloatsReadBack).
        let read: Vec<f32> = [0x3C00, 0xC000, 0x63D0].into_iter().map(f32_from_f16_bits).collect();
        assert_eq!(read, [1.0, -2.0, 1000.0]);
    }

    #[test]
    fn every_half_float_reads_as_the_reference_does() {
        for bits in 0..=u16::MAX {
            let reference = ::half::f16::from_bits(bits).to_f32();
            let read = f32_from_f16_bits(bits);
            if reference.is_nan() {
                assert!(read.is_nan(), "{bits:#06x}");
            } else {
                assert_eq!(read.to_bits(), reference.to_bits(), "{bits:#06x}");
            }
        }
    }

    #[test]
    fn floats_round_to_the_nearest_half_ties_to_even() {
        // Every half float itself, the halfway points between neighbours and
        // a sweep of f32 bit patterns, against the reference conversion.
        let mut checked = 0u64;
        let mut check = |value: f32| {
            let reference = ::half::f16::from_f32(value);
            let ours = f16_bits(value);
            if reference.is_nan() {
                assert!(::half::f16::from_bits(ours).is_nan(), "{value:e}");
            } else {
                assert_eq!(ours, reference.to_bits(), "{value:e} ({:#010x})", value.to_bits());
            }
            checked += 1;
        };
        for bits in 0..=u16::MAX {
            let value = f32_from_f16_bits(bits);
            check(value);
            let next = f32_from_f16_bits(bits.wrapping_add(1));
            if value.is_finite() && next.is_finite() {
                check((value + next) / 2.0);
            }
        }
        for bits in (0..=u32::MAX).step_by(4099) {
            check(f32::from_bits(bits));
        }
        for value in [0.0, -0.0, 65504.0, 65519.99, 65520.0, 1e9, -1e9, SUBNORMAL_UNIT, SUBNORMAL_UNIT / 2.0, SUBNORMAL_UNIT * 0.51, f32::INFINITY, f32::NAN] {
            check(value);
        }
        assert!(checked > 1_000_000);
    }

    #[test]
    fn the_terrains_heights_round_trip_within_half_a_step() {
        for metres in [-432.5f32, 0.0, 1.0, 100.5, 2047.0, 4478.0, 8848.0] {
            let back = f32_from_f16_bits(f16_bits(metres));
            assert!((back - metres).abs() <= (metres.abs() / 2048.0).max(1e-3), "{metres}: {back}");
        }
    }
}
