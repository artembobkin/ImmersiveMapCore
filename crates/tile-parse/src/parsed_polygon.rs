// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! One tessellated polygon in render space (x east, y up, float tile
//! units): a fill, a line ribbon or a decoration, before the unification
//! stage packs it into a layer's streams.

use crate::geom::V2;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedPolygon {
    pub vertices: Vec<V2>,
    pub indices: Vec<u32>,
    /// Per-vertex signed distance from a line's centreline, normalized so
    /// the extruded rim is `±i8::MAX`. Empty for plain polygon geometry.
    /// When non-empty it runs in lockstep with `vertices`.
    pub line_distances: Vec<i8>,
    /// Per-vertex longitudinal parameter, lockstep with `line_distances`:
    /// end-feather distance for solid styles, arc length for point-dashed
    /// ones (see the tile vertex's `line_parameter`).
    pub line_parameters: Vec<i16>,
    /// A deferred ribbon: every vertex is a point of the centreline and this
    /// is the unit direction, snorm i8, the vertex shader extrudes along by
    /// the width the style resolves on screen. Zero for a centreline vertex
    /// itself (a join's or a cap's hub). Empty for pre-extruded ribbons and
    /// fills, whose vertices are final.
    pub line_normals: Vec<[i8; 2]>,
    /// A deferred ribbon's edge lines: the rim as line segments, two indices
    /// each, over the rim vertices `indices` reads too. The road buckets
    /// draw them apart from the body as one-pixel lines at half the road's
    /// alpha that soften its hard edge. Empty for everything else.
    pub edge_line_indices: Vec<u32>,
    /// A fill kept as its outline for the flattening: where each ring starts
    /// in `vertices`, the exterior first and its holes after it, every ring
    /// closed implicitly, the exterior counter-clockwise and the holes
    /// clockwise in render space. Such a polygon has no `indices`: the
    /// flattener triangulates the ground as one surface. Empty for a
    /// tessellated polygon.
    pub outline_starts: Vec<i32>,
}

impl ParsedPolygon {
    /// How far below zero the doubled signed area may go before a triangle
    /// counts as clockwise, per unit of its longest edge.
    pub const CLOCKWISE_TOLERANCE_PER_EDGE_UNIT: f64 = 1e-3;

    pub fn new(vertices: Vec<V2>, indices: Vec<u32>) -> ParsedPolygon {
        ParsedPolygon { vertices, indices, ..Default::default() }
    }

    /// Whether this is an outline awaiting the flattening.
    pub fn is_outline(&self) -> bool {
        !self.outline_starts.is_empty()
    }

    /// A line ribbon carries per-vertex line attributes. A fill does not,
    /// including the decorations that share a line style.
    pub fn is_line_ribbon(&self) -> bool {
        self.line_distances.len() == self.vertices.len() && self.line_parameters.len() == self.vertices.len()
    }

    /// Gives a fill the line attributes of the saturated line interior, so
    /// it sweeps with the ribbons and draws among the ground lines by its
    /// key, fully covered.
    pub fn make_line_class_fill(&mut self) {
        self.line_distances = vec![0; self.vertices.len()];
        self.line_parameters = vec![i16::MAX; self.vertices.len()];
        self.line_normals.clear();
    }

    /// Turns every clockwise triangle counter-clockwise by swapping two of
    /// its indices, judged on the vertices as they are. Earcut hands a ring
    /// back with the odd inverted ear. A degenerate triangle is left alone.
    pub fn wind_counter_clockwise(&mut self) {
        let mut start = 0;
        while start + 2 < self.indices.len() {
            let a = self.vertices[self.indices[start] as usize];
            let b = self.vertices[self.indices[start + 1] as usize];
            let c = self.vertices[self.indices[start + 2] as usize];
            if doubled_area(a, b, c) < 0.0 {
                self.indices.swap(start + 1, start + 2);
            }
            start += 3;
        }
    }

    /// The first triangle (in triangles) wound clockwise in render space by
    /// more than the vertex rounding can explain.
    pub fn first_clockwise_triangle(vertices: &[V2], indices: &[u32]) -> Option<usize> {
        let mut start = 0;
        while start + 2 < indices.len() {
            let a = vertices[indices[start] as usize];
            let b = vertices[indices[start + 1] as usize];
            let c = vertices[indices[start + 2] as usize];
            let doubled = doubled_area(a, b, c);
            if doubled < 0.0 {
                let sq = |p: V2, q: V2| {
                    let dx = q.x as f64 - p.x as f64;
                    let dy = q.y as f64 - p.y as f64;
                    dx * dx + dy * dy
                };
                let longest = sq(a, b).max(sq(b, c)).max(sq(c, a)).sqrt();
                if doubled < -(Self::CLOCKWISE_TOLERANCE_PER_EDGE_UNIT * longest + 1e-3) {
                    return Some(start / 3);
                }
            }
            start += 3;
        }
        None
    }

    /// A convex ring in render space, fanned from its first vertex and wound
    /// counter-clockwise whichever way the ring runs.
    pub fn counter_clockwise_convex_fan(ring: Vec<V2>) -> ParsedPolygon {
        let mut doubled = 0f32;
        for i in 0..ring.len() {
            let c = ring[i];
            let n = ring[(i + 1) % ring.len()];
            doubled += c.x * n.y - n.x * c.y;
        }
        let mut indices = Vec::with_capacity(ring.len().saturating_sub(2) * 3);
        for corner in 1..ring.len().saturating_sub(1).max(1) {
            indices.push(0);
            if doubled < 0.0 {
                indices.push(corner as u32 + 1);
                indices.push(corner as u32);
            } else {
                indices.push(corner as u32);
                indices.push(corner as u32 + 1);
            }
        }
        ParsedPolygon::new(ring, indices)
    }
}

/// The doubled signed area of a triangle, positive counter-clockwise in
/// render space, in f64 so a large triangle's corners keep a sliver's sign.
#[inline(always)]
pub fn doubled_area(a: V2, b: V2, c: V2) -> f64 {
    (b.x as f64 - a.x as f64) * (c.y as f64 - a.y as f64) - (b.y as f64 - a.y as f64) * (c.x as f64 - a.x as f64)
}
