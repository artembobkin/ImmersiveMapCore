// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Splits the ground geometry of a coarse tile on a regular grid so it can
//! be drawn straight onto the sphere.
//!
//! A tile-spanning triangle drawn on the globe is a chord: the surface morph
//! projects vertices, not edges, so the only way to follow the curvature is
//! more vertices. Every triangle of every ground polygon is cut along the
//! grid lines `x = k * step` and `y = m * step`, each piece fan
//! triangulated. The step per tile zoom keeps the chord sag of one cell
//! around half a pixel at the zoom the tile is native to. Tiles from z8 are
//! never on the sphere.
//!
//! Attributes ride along linearly, so the analytic antialiasing and the
//! dashes survive the split. Intersections are computed from
//! lexicographically ordered edge endpoints, so the two triangles sharing an
//! edge produce bit-identical split vertices. Baked into the prepared tile.

use std::collections::HashMap;

use crate::geom::{V2, clamp_i8, clamp_i16, round};
use crate::parsed_polygon::ParsedPolygon;

/// Grid step in tile units for the tile zoom, `None` where no split is needed.
pub fn step_for_tile_zoom(zoom: i32) -> Option<i32> {
    match zoom {
        i32::MIN..=1 => Some(64),
        2..=3 => Some(128),
        4..=5 => Some(256),
        6..=7 => Some(512),
        _ => None,
    }
}

/// Line ribbons split on a grid four times as coarse as the fills.
pub fn ribbon_step(fill_step: i32) -> i32 {
    (fill_step * 4).min(4096)
}

pub fn subdivide_if_needed<'a>(polygons: impl IntoIterator<Item = &'a mut ParsedPolygon>, tile_zoom: i32) {
    let Some(step) = step_for_tile_zoom(tile_zoom) else { return };
    let ribbon = ribbon_step(step);
    for polygon in polygons {
        // A deferred ribbon has no geometry to split yet and never reaches a
        // sphere-era tile. An outline has no triangles: the flattening cuts it.
        if !polygon.line_normals.is_empty() || polygon.is_outline() {
            debug_assert!(polygon.line_normals.is_empty(), "a deferred ribbon in a sphere-era tile");
            continue;
        }
        let s = if polygon.is_line_ribbon() { ribbon } else { step };
        *polygon = subdivide(polygon, s);
    }
}

/// One vertex with its attributes in float, the working form of a piece.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub position: V2,
    pub line_distance: f32,
    pub line_parameter: f32,
}

#[inline(always)]
fn axis(p: V2, a: usize) -> f32 {
    if a == 0 { p.x } else { p.y }
}

#[inline(always)]
fn key_bits(v: f32) -> u32 {
    // Swift hashes -0 and 0 alike.
    if v == 0.0 { 0 } else { v.to_bits() }
}

pub fn subdivide(polygon: &ParsedPolygon, step: i32) -> ParsedPolygon {
    if step <= 0 || polygon.indices.len() < 3 {
        return polygon.clone();
    }
    let has_attributes = polygon.is_line_ribbon();
    let step_value = step as f32;
    let mut output = ParsedPolygon::default();
    output.vertices.reserve(polygon.vertices.len());
    output.indices.reserve(polygon.indices.len());
    let mut index_by_key: HashMap<(u32, u32, i8, i16), u32> = HashMap::with_capacity(polygon.vertices.len());

    let point = |i: u32| Point {
        position: polygon.vertices[i as usize],
        line_distance: if has_attributes { polygon.line_distances[i as usize] as f32 } else { 0.0 },
        line_parameter: if has_attributes { polygon.line_parameters[i as usize] as f32 } else { 0.0 },
    };
    let mut emit = |p: Point, output: &mut ParsedPolygon| -> u32 {
        let d = clamp_i8(round(p.line_distance));
        let t = clamp_i16(round(p.line_parameter));
        let key = (key_bits(p.position.x), key_bits(p.position.y), d, t);
        if let Some(&existing) = index_by_key.get(&key) {
            return existing;
        }
        let index = output.vertices.len() as u32;
        output.vertices.push(p.position);
        if has_attributes {
            output.line_distances.push(d);
            output.line_parameters.push(t);
        }
        index_by_key.insert(key, index);
        index
    };
    let mut emit_triangle = |a: Point, b: Point, c: Point, output: &mut ParsedPolygon| {
        let ia = emit(a, output);
        let ib = emit(b, output);
        let ic = emit(c, output);
        if ia != ib && ib != ic && ia != ic {
            output.indices.extend_from_slice(&[ia, ib, ic]);
        }
    };

    let mut strips: Vec<Vec<Point>> = Vec::new();
    let mut pieces: Vec<Vec<Point>> = Vec::new();
    for t in polygon.indices.as_chunks::<3>().0 {
        let (a, b, c) = (point(t[0]), point(t[1]), point(t[2]));
        let min_x = a.position.x.min(b.position.x).min(c.position.x);
        let max_x = a.position.x.max(b.position.x).max(c.position.x);
        let min_y = a.position.y.min(b.position.y).min(c.position.y);
        let max_y = a.position.y.max(b.position.y).max(c.position.y);
        let first_column = (min_x / step_value).floor() as i64 + 1;
        let last_column = ((max_x - 1e-3) / step_value).floor() as i64;
        let first_row = (min_y / step_value).floor() as i64 + 1;
        let last_row = ((max_y - 1e-3) / step_value).floor() as i64;
        if first_column > last_column && first_row > last_row {
            emit_triangle(a, b, c, &mut output);
            continue;
        }
        // The grid lines sweep through the triangle: each line hands the piece
        // below it to the output and keeps only the remainder above.
        strips.clear();
        sweep(vec![a, b, c], 0, first_column, last_column, step_value, &mut strips);
        for strip in strips.drain(..) {
            pieces.clear();
            sweep(strip, 1, first_row, last_row, step_value, &mut pieces);
            for piece in pieces.iter().filter(|p| p.len() >= 3) {
                for corner in 1..piece.len() - 1 {
                    emit_triangle(piece[0], piece[corner], piece[corner + 1], &mut output);
                }
            }
        }
    }
    output
}

fn sweep(polygon: Vec<Point>, a: usize, first: i64, last: i64, step: f32, pieces: &mut Vec<Vec<Point>>) {
    let mut remainder = polygon;
    if first <= last {
        for line in first..=last {
            let (below, above) = split_sides(&remainder, a, line as f32 * step);
            if below.len() >= 3 {
                pieces.push(below);
            }
            if above.len() < 3 {
                return;
            }
            remainder = above;
        }
    }
    if remainder.len() >= 3 {
        pieces.push(remainder);
    }
}

/// Splits a convex polygon along `position[axis] == value` into the piece
/// below and the piece above (either may be empty), keeping the winding.
pub fn split(polygon: &[Point], a: usize, value: f32) -> Vec<Vec<Point>> {
    let (below, above) = split_sides(polygon, a, value);
    [below, above].into_iter().filter(|p| p.len() >= 3).collect()
}

fn split_sides(polygon: &[Point], a: usize, value: f32) -> (Vec<Point>, Vec<Point>) {
    let mut below = Vec::with_capacity(polygon.len() + 1);
    let mut above = Vec::with_capacity(polygon.len() + 1);
    for i in 0..polygon.len() {
        let current = polygon[i];
        let next = polygon[(i + 1) % polygon.len()];
        let cs = axis(current.position, a) - value;
        let ns = axis(next.position, a) - value;
        if cs <= 0.0 {
            below.push(current);
        }
        if cs >= 0.0 {
            above.push(current);
        }
        if (cs < 0.0 && ns > 0.0) || (cs > 0.0 && ns < 0.0) {
            let crossing = intersection(current, next, a, value);
            below.push(crossing);
            above.push(crossing);
        }
    }
    (below, above)
}

/// The crossing of an edge with an axis-aligned line, from the endpoints in
/// a canonical order so both polygons sharing the edge get the same vertex.
pub fn intersection(a: Point, b: Point, ax: usize, value: f32) -> Point {
    let (first, second) = if ordered_before(&a, &b) { (a, b) } else { (b, a) };
    let span = axis(second.position, ax) - axis(first.position, ax);
    let t = if span == 0.0 { 0.0 } else { (value - axis(first.position, ax)) / span };
    let mut position = first.position + (second.position - first.position) * t;
    if ax == 0 {
        position.x = value;
    } else {
        position.y = value;
    }
    Point {
        position,
        line_distance: first.line_distance + (second.line_distance - first.line_distance) * t,
        line_parameter: first.line_parameter + (second.line_parameter - first.line_parameter) * t,
    }
}

fn ordered_before(a: &Point, b: &Point) -> bool {
    if a.position.x != b.position.x {
        return a.position.x < b.position.x;
    }
    if a.position.y != b.position.y {
        return a.position.y < b.position.y;
    }
    if a.line_distance != b.line_distance {
        return a.line_distance < b.line_distance;
    }
    a.line_parameter < b.line_parameter
}
