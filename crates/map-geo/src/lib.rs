// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The map's coordinate spaces and the matrix math the other crates share,
//! with no dependency of its own.
//!
//! The spaces:
//! - latitude and longitude in radians;
//! - normalized world Mercator: x east, y south, both in 0...1;
//! - normalized Mercator Y (`projection::y_mercator_normalized`): north up, in -1...1;
//! - the flat render world: y up, `render_map_size` units across, centred
//!   on the point the camera looks at (`FlatRenderState`);
//! - the globe's render world: the sphere touches the origin at its front
//!   point, and the pan turns it under the camera (`GlobeUniform`).

// The clamps and the guards keep the forms of the engine this is ported from,
// which pass NaN through or reject it where `clamp` would panic.
#![allow(clippy::manual_clamp, clippy::neg_cmp_op_on_partial_ord)]

pub mod frustum;
pub mod matrix;
pub mod projection;

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

/// The globe as a frame draws it: the pan as the globe reads it
/// (`projection::globe_pan`), the render radius, and the geometry phase of
/// the unroll into the plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlobeUniform {
    pub pan_x: f64,
    pub pan_y: f64,
    pub radius: f64,
    pub transition: f64,
}
