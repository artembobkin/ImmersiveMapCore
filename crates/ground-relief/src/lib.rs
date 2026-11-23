// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The CPU half of the ground relief, a port of the Swift terrain store's
//! decoding (`GroundReliefDecoder`, `GroundReliefTile`): a terrain tile's
//! image to its heights in metres, the heights to the half floats the
//! relief's texture holds, and their range. No IO and no bindings:
//! `crates/worker-wasm` calls it in the tile workers. The occlusion of the
//! sky is computed on the GPU (decision B10,
//! `tile/relief/store/ground-relief-occlusion-gpu.ts`).

mod decoder;
mod half;

pub use decoder::{DecodeError, Encoding, Heights, decode, height};
pub use half::{f16_bits, f32_from_f16_bits, halves};

/// The lowest and the highest of the heights, in metres: (0, 0) for none.
/// NaN heights are passed over.
pub fn height_range(metres: &[f32]) -> (f32, f32) {
    let mut values = metres.iter().copied().filter(|m| !m.is_nan());
    let Some(first) = values.next() else { return (0.0, 0.0) };
    values.fold((first, first), |(lowest, highest), m| (lowest.min(m), highest.max(m)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_height_range_spans_the_lowest_and_the_highest() {
        assert_eq!(height_range(&[3.0, -2.5, 10.0, 0.0]), (-2.5, 10.0));
        assert_eq!(height_range(&[]), (0.0, 0.0));
        assert_eq!(height_range(&[f32::NAN, 7.0]), (7.0, 7.0));
    }
}
