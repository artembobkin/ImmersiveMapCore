// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The trees of a tile: where they stand and how they are packed for the GPU.

pub mod reader;
pub mod scatter;

use crate::geom::V2;

/// One tree as the GPU reads it, 16 bytes (the Swift `TreeInstance`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TreeInstance {
    /// Where the trunk meets the ground, in render space, tile units.
    pub position: V2,
    /// The tree's height in tile units at the tile's zoom.
    pub height: f32,
    /// The turn in the low 16 bits (0 to 65535 a full turn), the tint in the next 8.
    pub packed: u32,
}

impl TreeInstance {
    pub fn new(position: V2, height: f32, rotation: f32, tint: f32) -> TreeInstance {
        let turn = rotation % 1.0;
        let turn = if turn < 0.0 { turn + 1.0 } else { turn };
        let rotation_bits = (turn.clamp(0.0, 1.0) * 65535.0) as u32;
        let tint_bits = (tint.clamp(0.0, 1.0) * 255.0) as u32;
        TreeInstance { position, height, packed: rotation_bits | (tint_bits << 16) }
    }

    pub fn rotation(&self) -> f32 {
        (self.packed & 0xFFFF) as f32 / 65535.0
    }

    pub fn tint(&self) -> f32 {
        ((self.packed >> 16) & 0xFF) as f32 / 255.0
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.position.x.to_le_bytes());
        out.extend_from_slice(&self.position.y.to_le_bytes());
        out.extend_from_slice(&self.height.to_le_bytes());
        out.extend_from_slice(&self.packed.to_le_bytes());
    }
}
