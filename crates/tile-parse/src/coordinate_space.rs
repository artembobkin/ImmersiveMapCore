// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The single source of truth for the tile geometry Y axis.
//!
//! - **Tile space** (raw MVT): x grows east, y grows south from the north
//!   edge, 0 to 4096. The working space of the whole parse: the decoder
//!   output, the road pre-pass, label anchors and road-label paths.
//! - **Render space**: y grows north from the south edge, `4096 - y`. It
//!   exists only as the vertex storage and GPU contract (`ParsedPolygon`,
//!   the tile vertex, extrusion meshes). It is entered at exactly one named
//!   point per geometry kind: inside `ParseLine`'s precompute for lines, at
//!   `ParsePolygon`'s tessellation for fills, at the building candidate for
//!   extrusions, and at the top of each decoration builder, always through
//!   the helpers below and never as an inline subtraction.
//! - **Tile UV**: v = y / 4096, the same axis as tile space.
//! - **World spaces**: normalized world mercator y grows south.
//!
//! Every mirror bug the Swift engine shipped was an unmarked handoff between
//! the first two, so the ceremony stays.

use crate::geom::V2;

pub const TILE_EXTENT: f32 = 4096.0;
pub const TILE_EXTENT_F64: f64 = 4096.0;
pub const TILE_EXTENT_I32: i32 = 4096;

/// Tile space to render space. The reflection is its own inverse: the two
/// names exist so a call site says which way it is going.
#[inline(always)]
pub fn render_y(tile_y: f32) -> f32 {
    TILE_EXTENT - tile_y
}

#[inline(always)]
pub fn render_point(point: V2) -> V2 {
    V2::new(point.x, TILE_EXTENT - point.y)
}

pub fn render_points(points: &[V2]) -> Vec<V2> {
    points.iter().map(|&p| render_point(p)).collect()
}

/// Render space to tile space: the same reflection, named for the reader.
#[inline(always)]
pub fn tile_y(render_y: f32) -> f32 {
    TILE_EXTENT - render_y
}
