// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use crate::ModelTileContents;

/// A vertex as a model tile holds it, `ModelTileContents::VERTEX_STRIDE`
/// bytes: what a baker writes into a tile's vertex block. It is made of a
/// vertex's surface, the normal, the texture layer and the texture
/// coordinate, and its place is set once the tile's bounding box is known
/// (`place`), since a position is kept as a share of that box.
///
/// Two vertices that are the same record are one vertex of the tile: a
/// baker writes each once and names it again by its index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModelTileVertex {
    /// The position across the tile's bounding box, 0...65535 on each axis.
    pub x: u16,
    pub y: u16,
    pub z: u16,
    /// The texture coordinate, unsigned normalized, V down.
    pub u: u16,
    pub v: u16,
    /// The normal, signed normalized, in tile space.
    pub normal: [i8; 3],
    /// The layer of the tile's texture array.
    pub layer: u8,
}

impl ModelTileVertex {
    /// A vertex of a surface, its place not yet set. `normal` is in tile
    /// space, `coordinate` inside the unit square.
    pub fn new(normal: [f32; 3], layer: u8, coordinate: [f32; 2]) -> ModelTileVertex {
        ModelTileVertex {
            x: 0,
            y: 0,
            z: 0,
            u: unsigned(coordinate[0]),
            v: unsigned(coordinate[1]),
            normal: normal.map(signed),
            layer,
        }
    }

    /// Sets the position, given in tile space, as a place in the tile's
    /// bounding box: on each axis 0 at the box's minimum and 65535 at its
    /// maximum, the nearest step. A box with no size on an axis has every
    /// place there at 0.
    pub fn place(&mut self, position: [f32; 3], minimum: [f32; 3], maximum: [f32; 3]) {
        let share = |axis: usize| -> u16 {
            let size = f64::from(maximum[axis]) - f64::from(minimum[axis]);
            if size <= 0.0 {
                return 0;
            }
            let share = (f64::from(position[axis]) - f64::from(minimum[axis])) / size;
            (share.clamp(0.0, 1.0) * 65_535.0).round() as u16
        };
        self.x = share(0);
        self.y = share(1);
        self.z = share(2);
    }

    /// The vertex's bytes, appended to a vertex block: the fields in the
    /// tile's order, each little-endian, with the reserved two bytes that
    /// make the stride a multiple of four.
    pub fn append_to(&self, block: &mut Vec<u8>) {
        let start = block.len();
        for value in [self.x, self.y, self.z, self.u, self.v] {
            block.extend_from_slice(&value.to_le_bytes());
        }
        block.extend(self.normal.map(|component| component as u8));
        block.push(self.layer);
        block.extend_from_slice(&[0, 0]);
        debug_assert_eq!(block.len() - start, ModelTileContents::VERTEX_STRIDE);
    }
}

fn signed(component: f32) -> i8 {
    (component * 127.0).round().clamp(-127.0, 127.0) as i8
}

fn unsigned(component: f32) -> u16 {
    (component.clamp(0.0, 1.0) * 65_535.0).round() as u16
}
