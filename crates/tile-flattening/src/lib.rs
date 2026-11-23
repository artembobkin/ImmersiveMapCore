// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The ground flattener, pure geometry with no dependencies.
//!
//! Stacked painted rings go in (`FlattenInput`), and one layer of triangles that do not overlap
//! comes out (`FlatMesh`), cut to a grid for the relief's heights. The entry point is `flatten`.

mod flat_mesh;
mod flatten_input;
mod flattener;
mod monotone_triangulator;
mod stroker;
mod sweep;

#[cfg(test)]
mod tests;

pub use flat_mesh::{FlatColor, FlatMesh, FlatVertex};
pub use flatten_input::FlattenInput;
pub use flattener::{FlattenOptions, FlattenTimings, flatten, flatten_timed};
pub use stroker::{LineCap, LineJoin, LineStyle};

/// `max` with the semantics of Swift's `max(_:_:)`: `y` when `y >= x`, otherwise `x`.
/// It keeps the handling of signed zeros and NaN of the original code.
#[inline(always)]
pub(crate) fn fmax(x: f64, y: f64) -> f64 {
    if y >= x { y } else { x }
}

/// `min` with the semantics of Swift's `min(_:_:)`: `y` when `y < x`, otherwise `x`.
#[inline(always)]
pub(crate) fn fmin(x: f64, y: f64) -> f64 {
    if y < x { y } else { x }
}

/// A fast hasher for the small integer keys of the vertex maps. The default SipHash is built to
/// resist adversarial keys, which costs far more than these lookups can afford.
#[derive(Default, Clone, Copy)]
pub(crate) struct FastHasher {
    hash: u64,
}

impl FastHasher {
    #[inline(always)]
    fn add(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

impl std::hash::Hasher for FastHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.add(u64::from(byte));
        }
    }

    #[inline(always)]
    fn write_u32(&mut self, value: u32) {
        self.add(u64::from(value));
    }

    #[inline(always)]
    fn write_u64(&mut self, value: u64) {
        self.add(value);
    }

    #[inline(always)]
    fn finish(&self) -> u64 {
        // The table picks buckets with the low bits and tags with the high bits: mix both.
        let mut z = self.hash;
        z ^= z >> 29;
        z = z.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z ^ (z >> 32)
    }
}

pub(crate) type FastHashMap<K, V> =
    std::collections::HashMap<K, V, std::hash::BuildHasherDefault<FastHasher>>;

/// A position with its color, the identity of a vertex. The positions are the bits of `f32`s
/// with `-0` turned into `+0`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct VertexKey {
    position: u64,
    color: u32,
}

impl VertexKey {
    #[inline(always)]
    pub(crate) fn new(x: f32, y: f32, color: u32) -> Self {
        VertexKey {
            position: u64::from(x.to_bits()) | (u64::from(y.to_bits()) << 32),
            color,
        }
    }
}
