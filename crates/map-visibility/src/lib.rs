// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The frame's tile coverage, free of any host: which tiles a camera pose
//! draws, at which zooms, a port of the Swift engine's `TileCulling`. On
//! the plane (`flat`) the frustum's footprint (`polygon`) is cut into bands
//! by the ring rules (`rules`). On the sphere (`globe`) the same rules are
//! read off the sphere's grid, with the frustum (`frustum`) and the horizon
//! in the footprint's place.
//!
//! A host calls it once per frame with the camera's matrix and reads the
//! targets as the flat records the placement planner of `map-frame` takes,
//! so the two run back to back without a value crossing a binding between
//! them.

// The guards keep the negated comparisons and the clamps of the engine this
// is ported from, which also reject NaN: a matrix gone wrong places nothing.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::manual_clamp, clippy::manual_range_contains)]

pub mod culling;
pub mod flat;
pub mod frustum;
pub mod globe;
pub mod polygon;
pub mod rules;

/// A tile in a copy of the world: `wrap` is the copy, 0 the one under the camera's pan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VisibleTile {
    pub z: i32,
    pub x: i32,
    pub y: i32,
    pub wrap: i32,
}

/// A coverage target: the tile and the switches of the rule that placed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub tile: VisibleTile,
    /// False for a rule that draws no lines.
    pub lines: bool,
    /// False for a rule that draws no labels.
    pub labels: bool,
    /// The side of the texture a rule draws the ground from, `None` for geometry.
    pub raster_size: Option<u32>,
}

/// Ints per target in the flat form: z, x, y, wrap, lines, labels, raster
/// size (-1 for none). The record `map_frame::planner` reads.
pub const TARGET_STRIDE: usize = 7;

impl Target {
    pub fn append_to(&self, out: &mut Vec<i32>) {
        out.extend_from_slice(&[self.tile.z, self.tile.x, self.tile.y, self.tile.wrap, self.lines as i32, self.labels as i32, self.raster_size.map_or(-1, |size| size as i32)]);
    }
}

/// The pan and the size of the flat render world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlatRenderState {
    pub pan: [f64; 2],
    pub render_map_size: f64,
}

impl FlatRenderState {
    /// A tile's origin (its south west corner) and size in the flat render
    /// world. The row index grows south while the world is y up.
    pub fn tile_origin_and_size(&self, x: i32, y: i32, z: i32, wrap: i32) -> [f64; 3] {
        let tiles_count = 2f64.powi(z);
        let tile_size = self.render_map_size / tiles_count;
        let half = self.render_map_size * 0.5;
        let origin_x = x as f64 * tile_size - half + self.pan[0] * half + wrap as f64 * self.render_map_size;
        let origin_y = (tiles_count - y as f64 - 1.0) * tile_size - half - self.pan[1] * half;
        [origin_x, origin_y, tile_size]
    }
}
