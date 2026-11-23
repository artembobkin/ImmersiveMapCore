// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Lines: the clipper that cuts a polyline to the tile, and the ribbon
//! tessellator, with joins, caps, analytic antialiasing attributes, the
//! deferred extrusion of the flat roads and the relief grid rows. A port of
//! the Swift `ParseLine.swift`.

use crate::geom::{V2, clamp_i8, clamp_i16, round};
use crate::parsed_polygon::ParsedPolygon;

const EPSILON: f32 = 0.0001;

#[derive(Debug, Clone, PartialEq)]
pub struct ClippedLineFragment {
    pub points: Vec<V2>,
    pub start_clipped: bool,
    pub end_clipped: bool,
}

#[derive(Clone, Copy)]
struct ClipBounds {
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
}

impl ClipBounds {
    #[inline(always)]
    fn clamp(&self, p: V2) -> V2 {
        V2::new(p.x.max(self.min_x).min(self.max_x), p.y.max(self.min_y).min(self.max_y))
    }
}

#[inline(always)]
fn points_equal(a: V2, b: V2) -> bool {
    (a.x - b.x).abs() <= EPSILON && (a.y - b.y).abs() <= EPSILON
}

/// Liang-Barsky clipping of a polyline to the tile square (with padding),
/// in tile space.
#[derive(Default)]
pub struct LineClipper;

struct ClippedSegment {
    start: V2,
    end: V2,
    start_clipped: bool,
    end_clipped: bool,
}

impl LineClipper {
    pub fn new() -> LineClipper {
        LineClipper
    }

    pub fn clip(&self, points: &[V2], tile_extent: f32, padding: f32) -> Vec<ClippedLineFragment> {
        if points.len() < 2 {
            return Vec::new();
        }
        let bounds = ClipBounds { min_x: -padding, max_x: tile_extent + padding, min_y: -padding, max_y: tile_extent + padding };
        let mut fragments = Vec::new();
        let mut current: Vec<V2> = Vec::new();
        let mut start_clipped = false;
        let mut end_clipped = false;

        let finalize = |current: &mut Vec<V2>, start_clipped: &mut bool, end_clipped: &mut bool, fragments: &mut Vec<ClippedLineFragment>| {
            let sanitized = sanitize(current, &bounds);
            if sanitized.len() >= 2 {
                fragments.push(ClippedLineFragment { points: sanitized, start_clipped: *start_clipped, end_clipped: *end_clipped });
            }
            current.clear();
            *start_clipped = false;
            *end_clipped = false;
        };

        for i in 0..points.len() - 1 {
            let Some(segment) = clip_segment(points[i], points[i + 1], &bounds) else {
                finalize(&mut current, &mut start_clipped, &mut end_clipped, &mut fragments);
                continue;
            };
            let clipped_start = bounds.clamp(segment.start);
            let clipped_end = bounds.clamp(segment.end);
            if current.is_empty() {
                current.push(clipped_start);
                start_clipped = segment.start_clipped;
            } else if !points_equal(current[current.len() - 1], clipped_start) {
                finalize(&mut current, &mut start_clipped, &mut end_clipped, &mut fragments);
                current.push(clipped_start);
                start_clipped = segment.start_clipped;
            }
            if !points_equal(current[current.len() - 1], clipped_end) {
                current.push(clipped_end);
            }
            end_clipped = segment.end_clipped;
        }
        finalize(&mut current, &mut start_clipped, &mut end_clipped, &mut fragments);
        fragments
    }

    pub fn is_on_tile_boundary(point: V2, tile_extent: f32) -> bool {
        point.x.abs() <= EPSILON
            || point.y.abs() <= EPSILON
            || (point.x - tile_extent).abs() <= EPSILON
            || (point.y - tile_extent).abs() <= EPSILON
    }
}

fn clip_segment(start: V2, end: V2, bounds: &ClipBounds) -> Option<ClippedSegment> {
    let delta = end - start;
    let (mut t0, mut t1) = (0f32, 1f32);
    let p = [-delta.x, delta.x, -delta.y, delta.y];
    let q = [start.x - bounds.min_x, bounds.max_x - start.x, start.y - bounds.min_y, bounds.max_y - start.y];
    for i in 0..4 {
        let (ep, eq) = (p[i], q[i]);
        if ep.abs() <= EPSILON {
            if eq < 0.0 {
                return None;
            }
            continue;
        }
        let ratio = eq / ep;
        if ep < 0.0 {
            if ratio > t1 {
                return None;
            }
            t0 = t0.max(ratio);
        } else {
            if ratio < t0 {
                return None;
            }
            t1 = t1.min(ratio);
        }
    }
    if t0 > t1 {
        return None;
    }
    let clipped_start = start + delta * t0;
    let clipped_end = start + delta * t1;
    if points_equal(clipped_start, clipped_end) {
        return None;
    }
    Some(ClippedSegment { start: clipped_start, end: clipped_end, start_clipped: t0 > EPSILON, end_clipped: t1 < 1.0 - EPSILON })
}

fn sanitize(points: &[V2], bounds: &ClipBounds) -> Vec<V2> {
    let mut out: Vec<V2> = Vec::with_capacity(points.len());
    for &p in points {
        let c = bounds.clamp(p);
        if let Some(&last) = out.last()
            && points_equal(last, c)
        {
            continue;
        }
        out.push(c);
    }
    out
}

/// The options of one ribbon, the Swift `parse(points:width:...)` labels.
#[derive(Debug, Clone, Copy)]
pub struct LineOptions {
    pub width: f64,
    pub tile_extent: f32,
    pub start_cap_round: bool,
    pub end_cap_round: bool,
    pub line_join_round: bool,
    /// The endpoint is a genuine free butt end whose cut may be antialiased
    /// longitudinally. False for continuations that meet adjacent geometry
    /// flush (tile seams, junctions).
    pub feather_start: bool,
    pub feather_end: bool,
    /// The longitudinal parameter is the cumulative arc length in tile
    /// units, for styles whose dash pattern the shader cuts on screen.
    pub emits_arc_length: bool,
    pub extend_clipped_start: bool,
    pub extend_clipped_end: bool,
    pub clip_padding: f32,
    pub clip_geometry_to_tile_bounds: bool,
    /// The centreline and directions go to the GPU, which extrudes by the
    /// width the style resolves on screen.
    pub deferred_extrusion: bool,
    /// A row of vertices wherever a segment crosses this grid, so a ribbon
    /// drawn on the relief follows it as the flattened ground does.
    pub subdivision_step: Option<f32>,
}

impl LineOptions {
    pub fn new(width: f64, tile_extent: f32) -> LineOptions {
        LineOptions {
            width,
            tile_extent,
            start_cap_round: false,
            end_cap_round: false,
            line_join_round: false,
            feather_start: false,
            feather_end: false,
            emits_arc_length: false,
            extend_clipped_start: false,
            extend_clipped_end: false,
            clip_padding: 0.0,
            clip_geometry_to_tile_bounds: true,
            deferred_extrusion: false,
            subdivision_step: None,
        }
    }
}

#[derive(Default)]
struct GeneratedPolygon {
    vertices: Vec<V2>,
    centres: Vec<V2>,
    normals: Vec<V2>,
    distances: Vec<f32>,
    parameters: Vec<f32>,
    indices: Vec<u32>,
    edge_line_indices: Vec<u32>,
}

impl GeneratedPolygon {
    #[inline(always)]
    fn push(&mut self, vertex: V2, centre: V2, normal: V2, distance: f32, parameter: f32) {
        self.vertices.push(vertex);
        self.centres.push(centre);
        self.normals.push(normal);
        self.distances.push(distance);
        self.parameters.push(parameter);
    }

    #[inline(always)]
    fn edge_line(&mut self, a: u32, b: u32) {
        self.edge_line_indices.push(a);
        self.edge_line_indices.push(b);
    }
}

#[derive(Clone, Copy)]
struct AttributedPoint {
    position: V2,
    distance: f32,
    parameter: f32,
}

struct PrecomputedLine {
    points: Vec<V2>,
    segment_lengths: Vec<f32>,
    point_arc_lengths: Vec<f32>,
    segment_directions: Vec<V2>,
    segment_normals: Vec<V2>,
    valid_segment_count: usize,
    first_valid: Option<usize>,
    last_valid: Option<usize>,
}

/// The ribbon tessellator. Stateless: the Swift class kept no state either.
#[derive(Default)]
pub struct ParseLine;

impl ParseLine {
    /// Extra extrusion beyond the styled half-width, in tile units, giving
    /// the analytic antialiasing ramp room inside the geometry.
    pub const FEATHER_TILE_UNITS: f32 = 2.0;
    /// The narrowest ribbon, in tile units from the centreline: a pixel a
    /// side at the tile's own scale.
    pub const MINIMUM_EXTRUDED_HALF_WIDTH: f32 = 8.0;
    /// Largest angle one fan triangle of a round join may span.
    pub const JOIN_ARC_MAXIMUM_STEP_RADIANS: f32 = std::f32::consts::PI / 9.0;
    const CAP_SEGMENTS: usize = 8;

    pub fn new() -> ParseLine {
        ParseLine
    }

    /// How far the ribbon is extruded for a styled half-width.
    pub fn extruded_half_width(half_width: f32) -> f32 {
        (half_width + Self::FEATHER_TILE_UNITS).max(Self::MINIMUM_EXTRUDED_HALF_WIDTH)
    }

    fn cap_unit_semicircle() -> [V2; Self::CAP_SEGMENTS + 1] {
        let mut out = [V2::ZERO; Self::CAP_SEGMENTS + 1];
        for (i, slot) in out.iter_mut().enumerate() {
            let t = i as f32 / Self::CAP_SEGMENTS as f32;
            let angle = (-0.5 + t) * std::f32::consts::PI;
            *slot = V2::new(angle.cos(), angle.sin());
        }
        out
    }

    /// `points` are tile space (y down). The single flip into render space
    /// happens in `precompute`, and the emitted vertices are render space.
    /// Every emitted triangle is counter-clockwise in render space.
    pub fn parse(&self, points: &[V2], o: &LineOptions) -> Option<ParsedPolygon> {
        if points.len() < 2 || o.width <= 0.0 {
            return None;
        }
        let half_width = (o.width * 0.5) as f32;
        let extruded = Self::extruded_half_width(half_width);
        let effective = extended_endpoints(points, o.tile_extent, o.clip_padding, o.extend_clipped_start, o.extend_clipped_end);
        let pre = precompute(&effective, o.tile_extent);
        if pre.valid_segment_count == 0 {
            return None;
        }
        let mut polygon = GeneratedPolygon::default();
        let segment_vertices = pre.valid_segment_count * 4;
        polygon.vertices.reserve(segment_vertices);
        polygon.indices.reserve(pre.valid_segment_count * 6);
        // A deferred ribbon's edges are hard: a free end is cut where the
        // line ends, with no feather rows past it.
        let feathers_ends = !o.emits_arc_length && !o.deferred_extrusion;
        append_segments(
            &pre,
            extruded,
            o.feather_start && !o.start_cap_round && feathers_ends,
            o.feather_end && !o.end_cap_round && feathers_ends,
            o.emits_arc_length,
            o.deferred_extrusion,
            o.subdivision_step,
            &mut polygon,
        );
        if o.line_join_round {
            append_round_joins(&pre, extruded, o.emits_arc_length, o.deferred_extrusion, &mut polygon);
        }
        if o.start_cap_round
            && let Some(first) = pre.first_valid
        {
            let parameter = if o.emits_arc_length { pre.point_arc_lengths[0] } else { 1.0 };
            append_cap(pre.points[0], pre.segment_directions[first], extruded, true, parameter, o.deferred_extrusion, &mut polygon);
        }
        if o.end_cap_round
            && let Some(last) = pre.last_valid
        {
            let end = points.len() - 1;
            let parameter = if o.emits_arc_length { pre.point_arc_lengths[end] } else { 1.0 };
            append_cap(pre.points[end], pre.segment_directions[last], extruded, false, parameter, o.deferred_extrusion, &mut polygon);
        }
        if o.deferred_extrusion {
            return quantize_deferred(polygon, o.emits_arc_length);
        }
        if o.clip_geometry_to_tile_bounds {
            clip_to_tile(polygon, o.tile_extent, o.emits_arc_length)
        } else {
            Some(quantize(polygon, o.emits_arc_length))
        }
    }

    /// The rows with one more at every crossing of a grid line, and of a
    /// grid cell's diagonal (x - y = k * step), between two neighbouring
    /// rows: on the relief the ground is two triangles a cell, cut from the
    /// south-west corner to the north-east one, and a row at every edge of
    /// those triangles keeps each piece of the strip on one plane.
    pub fn subdivided_rows(rows: &[(V2, f32)], step: f32) -> Vec<(V2, f32)> {
        if step <= 0.0 || rows.len() < 2 {
            return rows.to_vec();
        }
        let mut result = Vec::with_capacity(rows.len());
        result.push(rows[0]);
        let mut crossings: Vec<f32> = Vec::new();
        for i in 1..rows.len() {
            let (from, from_p) = rows[i - 1];
            let (to, to_p) = rows[i];
            let delta = to - from;
            crossings.clear();
            let coordinates = [(from.x, to.x), (from.y, to.y), (from.x - from.y, to.x - to.y)];
            for (cf, ct) in coordinates {
                if (ct - cf).abs() <= EPSILON {
                    continue;
                }
                let lower = cf.min(ct);
                let upper = cf.max(ct);
                let mut line = (lower / step).floor() + 1.0;
                while line * step < upper {
                    let value = line * step;
                    if value > lower {
                        crossings.push((value - cf) / (ct - cf));
                    }
                    line += 1.0;
                }
            }
            crossings.sort_by(|a, b| a.total_cmp(b));
            let mut previous = 0f32;
            for &t in &crossings {
                if t > EPSILON && t < 1.0 - EPSILON && t - previous > EPSILON {
                    result.push((from + delta * t, from_p + (to_p - from_p) * t));
                    previous = t;
                }
            }
            result.push((to, to_p));
        }
        result
    }

    pub fn quantize_normal(direction: V2) -> [i8; 2] {
        [clamp_i8(round(direction.x * i8::MAX as f32)), clamp_i8(round(direction.y * i8::MAX as f32))]
    }
}

fn endpoint_direction(points: &[V2], from_start: bool) -> Option<V2> {
    if points.len() < 2 {
        return None;
    }
    if from_start {
        let anchor = points[0];
        for &p in &points[1..] {
            let delta = p - anchor;
            let length = delta.length();
            if length > EPSILON {
                return Some(delta / length);
            }
        }
        return None;
    }
    let anchor = points[points.len() - 1];
    for &p in points[..points.len() - 1].iter().rev() {
        let delta = anchor - p;
        let length = delta.length();
        if length > EPSILON {
            return Some(delta / length);
        }
    }
    None
}

fn clipped_endpoint_extension_length(point: V2, direction: V2, tile_extent: f32, clip_padding: f32) -> f32 {
    let min_bound = -clip_padding;
    let max_bound = tile_extent + clip_padding;
    let mut length = clip_padding;
    if (point.x - min_bound).abs() <= EPSILON || (point.x - max_bound).abs() <= EPSILON {
        length = length.max(clip_padding / direction.x.abs().max(EPSILON));
    }
    if (point.y - min_bound).abs() <= EPSILON || (point.y - max_bound).abs() <= EPSILON {
        length = length.max(clip_padding / direction.y.abs().max(EPSILON));
    }
    length
}

fn extended_endpoints(points: &[V2], tile_extent: f32, clip_padding: f32, extend_start: bool, extend_end: bool) -> Vec<V2> {
    if !(extend_start || extend_end) || points.len() < 2 || clip_padding <= EPSILON {
        return points.to_vec();
    }
    let mut adjusted = points.to_vec();
    if extend_start && let Some(d) = endpoint_direction(&adjusted, true) {
        let length = clipped_endpoint_extension_length(adjusted[0], d, tile_extent, clip_padding);
        adjusted[0] -= d * length;
    }
    if extend_end && let Some(d) = endpoint_direction(&adjusted, false) {
        let last = adjusted.len() - 1;
        let length = clipped_endpoint_extension_length(adjusted[last], d, tile_extent, clip_padding);
        adjusted[last] += d * length;
    }
    adjusted
}

fn precompute(source: &[V2], tile_extent: f32) -> PrecomputedLine {
    // The one entry into render space for lines.
    let points: Vec<V2> = source.iter().map(|p| V2::new(p.x, tile_extent - p.y)).collect();
    let segment_count = source.len().saturating_sub(1);
    let mut segment_lengths = vec![0f32; segment_count];
    let mut segment_directions = vec![V2::ZERO; segment_count];
    let mut segment_normals = vec![V2::ZERO; segment_count];
    let mut valid_segment_count = 0;
    let (mut first_valid, mut last_valid) = (None, None);
    for i in 0..segment_count {
        let delta = points[i + 1] - points[i];
        let length = delta.length();
        segment_lengths[i] = length;
        if length <= EPSILON {
            continue;
        }
        let direction = delta / length;
        segment_directions[i] = direction;
        segment_normals[i] = V2::new(-direction.y, direction.x);
        valid_segment_count += 1;
        if first_valid.is_none() {
            first_valid = Some(i);
        }
        last_valid = Some(i);
    }
    let mut point_arc_lengths = vec![0f32; points.len()];
    for i in 0..segment_count {
        point_arc_lengths[i + 1] = point_arc_lengths[i] + segment_lengths[i];
    }
    PrecomputedLine {
        points,
        segment_lengths,
        point_arc_lengths,
        segment_directions,
        segment_normals,
        valid_segment_count,
        first_valid,
        last_valid,
    }
}

/// Each segment as a strip of full-width rows: a plain segment is two rows,
/// a feathered free end adds a tip row one feather past the styled cut and a
/// ring row one feather inside it, so the longitudinal field ramps -1 to 1
/// across the cut with the styled end on its zero isoline.
#[allow(clippy::too_many_arguments)]
fn append_segments(
    pre: &PrecomputedLine,
    extruded: f32,
    feather_start: bool,
    feather_end: bool,
    emits_arc_length: bool,
    emits_edge_lines: bool,
    subdivision_step: Option<f32>,
    polygon: &mut GeneratedPolygon,
) {
    let feather = ParseLine::FEATHER_TILE_UNITS;
    let mut rows: Vec<(V2, f32)> = Vec::with_capacity(4);
    for i in 0..pre.segment_lengths.len() {
        let length = pre.segment_lengths[i];
        if length <= EPSILON {
            continue;
        }
        let start = pre.points[i];
        let end = pre.points[i + 1];
        let direction = pre.segment_directions[i];
        let normal = pre.segment_normals[i];
        let offset = normal * extruded;
        let feather_this_start = feather_start && Some(i) == pre.first_valid;
        let feather_this_end = feather_end && Some(i) == pre.last_valid;
        rows.clear();
        if emits_arc_length {
            rows.push((start, pre.point_arc_lengths[i]));
            rows.push((end, pre.point_arc_lengths[i + 1]));
        } else {
            if feather_this_start {
                let inset = feather.min(length * 0.5);
                rows.push((start - direction * feather, -1.0));
                rows.push((start + direction * inset, inset / feather));
            } else {
                rows.push((start, 1.0));
            }
            if feather_this_end {
                let inset = feather.min(length * 0.5);
                rows.push((end - direction * inset, inset / feather));
                rows.push((end + direction * feather, -1.0));
            } else {
                rows.push((end, 1.0));
            }
        }
        let rows_used: Vec<(V2, f32)>;
        let rows_ref: &[(V2, f32)] = if let Some(step) = subdivision_step {
            rows_used = ParseLine::subdivided_rows(&rows, step);
            &rows_used
        } else {
            &rows
        };
        let base = polygon.vertices.len() as u32;
        for &(position, parameter) in rows_ref {
            polygon.push(position + offset, position, normal, 1.0, parameter);
            polygon.push(position - offset, position, -normal, -1.0, parameter);
        }
        // Each row is (left, right): (L_k, R_k, L_k+1) and (R_k, R_k+1,
        // L_k+1) are counter-clockwise in render space.
        for r in 0..rows_ref.len() - 1 {
            let rb = base + (r * 2) as u32;
            polygon.indices.extend_from_slice(&[rb, rb + 1, rb + 2, rb + 1, rb + 3, rb + 2]);
        }
        if emits_edge_lines {
            for r in 0..rows_ref.len() - 1 {
                let rb = base + (r * 2) as u32;
                polygon.edge_line(rb, rb + 2);
                polygon.edge_line(rb + 1, rb + 3);
            }
        }
    }
}

/// The wedge on the outside of each turn, a fan from the centre along the
/// rim's arc in steps sized to the turn.
fn append_round_joins(pre: &PrecomputedLine, extruded: f32, emits_arc_length: bool, emits_edge_lines: bool, polygon: &mut GeneratedPolygon) {
    if pre.points.len() <= 2 {
        return;
    }
    for i in 1..pre.points.len() - 1 {
        if pre.segment_lengths[i - 1] <= EPSILON || pre.segment_lengths[i] <= EPSILON {
            continue;
        }
        let dir0 = pre.segment_directions[i - 1];
        let dir1 = pre.segment_directions[i];
        let cross = dir0.cross(dir1);
        if cross.abs() <= EPSILON {
            continue;
        }
        let center = pre.points[i];
        let left0 = pre.segment_normals[i - 1];
        let left1 = pre.segment_normals[i];
        let inner_is_left = cross < 0.0;
        let outer0 = if inner_is_left { left0 } else { -left0 };
        let outer1 = if inner_is_left { left1 } else { -left1 };
        let parameter = if emits_arc_length { pre.point_arc_lengths[i] } else { 1.0 };
        let turn = outer0.dot(outer1).clamp(-1.0, 1.0).acos();
        let steps = ((turn / ParseLine::JOIN_ARC_MAXIMUM_STEP_RADIANS).ceil() as usize).max(1);
        let base = polygon.vertices.len() as u32;
        polygon.push(center, center, V2::ZERO, 0.0, parameter);
        for s in 0..=steps {
            let t = s as f32 / steps as f32;
            let mut direction = outer0 * (1.0 - t) + outer1 * t;
            let length = direction.length();
            direction = if length > EPSILON { direction / length } else { outer0 };
            polygon.push(center + direction * extruded, center, direction, 1.0, parameter);
        }
        for s in 0..steps {
            let rim0 = base + 1 + s as u32;
            let rim1 = base + 2 + s as u32;
            // The hub last: the first vertex is the provoking one, whose flat
            // outputs (a deferred ribbon's width) the triangle reads.
            polygon.indices.push(if inner_is_left { rim1 } else { rim0 });
            polygon.indices.push(if inner_is_left { rim0 } else { rim1 });
            polygon.indices.push(base);
        }
        if emits_edge_lines {
            for s in 0..steps {
                polygon.edge_line(base + 1 + s as u32, base + 2 + s as u32);
            }
        }
    }
}

fn append_cap(center: V2, direction: V2, radius: f32, flip: bool, parameter: f32, emits_edge_lines: bool, polygon: &mut GeneratedPolygon) {
    let forward = if flip { -direction } else { direction };
    let right = V2::new(-forward.y, forward.x);
    let base = polygon.vertices.len() as u32;
    polygon.push(center, center, V2::ZERO, 0.0, parameter);
    let semicircle = ParseLine::cap_unit_semicircle();
    for p in semicircle {
        let d = forward * p.x + right * p.y;
        polygon.push(center + d * radius, center, d.normalized(), 1.0, parameter);
    }
    for i in 1..semicircle.len() {
        polygon.indices.extend_from_slice(&[base + i as u32, base + i as u32 + 1, base]);
    }
    if emits_edge_lines {
        for i in 1..semicircle.len() {
            polygon.edge_line(base + i as u32, base + i as u32 + 1);
        }
    }
}

#[inline(always)]
fn quantize_distance(v: f32) -> i8 {
    clamp_i8(round(v * i8::MAX as f32))
}

/// End-feather values use the full i16 range. Arc lengths are stored in half
/// tile units.
#[inline(always)]
fn quantize_parameter(v: f32, emits_arc_length: bool) -> i16 {
    if emits_arc_length { clamp_i16(round(v * 2.0)) } else { clamp_i16(round(v * i16::MAX as f32)) }
}

fn quantize_deferred(polygon: GeneratedPolygon, emits_arc_length: bool) -> Option<ParsedPolygon> {
    if polygon.indices.is_empty() || polygon.centres.len() != polygon.vertices.len() || polygon.normals.len() != polygon.vertices.len() {
        return None;
    }
    Some(ParsedPolygon {
        line_distances: polygon.distances.iter().map(|&d| quantize_distance(d)).collect(),
        line_parameters: polygon.parameters.iter().map(|&p| quantize_parameter(p, emits_arc_length)).collect(),
        line_normals: polygon.normals.iter().map(|&n| ParseLine::quantize_normal(n)).collect(),
        vertices: polygon.centres,
        indices: polygon.indices,
        edge_line_indices: polygon.edge_line_indices,
        outline_starts: Vec::new(),
    })
}

fn quantize(polygon: GeneratedPolygon, emits_arc_length: bool) -> ParsedPolygon {
    ParsedPolygon {
        line_distances: polygon.distances.iter().map(|&d| quantize_distance(d)).collect(),
        line_parameters: polygon.parameters.iter().map(|&p| quantize_parameter(p, emits_arc_length)).collect(),
        vertices: polygon.vertices,
        indices: polygon.indices,
        ..Default::default()
    }
}

#[inline(always)]
fn inside_tile(p: V2, extent: f32) -> bool {
    p.x >= 0.0 && p.x <= extent && p.y >= 0.0 && p.y <= extent
}

fn clip_to_tile(polygon: GeneratedPolygon, extent: f32, emits_arc_length: bool) -> Option<ParsedPolygon> {
    if polygon.indices.is_empty() {
        return None;
    }
    if polygon.vertices.iter().all(|&v| inside_tile(v, extent)) {
        return Some(quantize(polygon, emits_arc_length));
    }
    let mut vertices = Vec::new();
    let mut distances = Vec::new();
    let mut parameters = Vec::new();
    let mut indices = Vec::new();
    let n = polygon.vertices.len();
    for t in polygon.indices.as_chunks::<3>().0 {
        let (i0, i1, i2) = (t[0] as usize, t[1] as usize, t[2] as usize);
        if i0 >= n || i1 >= n || i2 >= n {
            continue;
        }
        let at = |i: usize| AttributedPoint { position: polygon.vertices[i], distance: polygon.distances[i], parameter: polygon.parameters[i] };
        let ring = sanitize_clipped_ring(clip_ring_to_tile(vec![at(i0), at(i1), at(i2)], extent), extent);
        if ring.len() < 3 {
            continue;
        }
        let base = vertices.len() as u32;
        for p in &ring {
            vertices.push(p.position);
            distances.push(quantize_distance(p.distance));
            parameters.push(quantize_parameter(p.parameter, emits_arc_length));
        }
        let clockwise = signed_area_f64(&ring) < 0.0;
        for k in 1..ring.len() - 1 {
            indices.push(base);
            if clockwise {
                indices.push(base + k as u32 + 1);
                indices.push(base + k as u32);
            } else {
                indices.push(base + k as u32);
                indices.push(base + k as u32 + 1);
            }
        }
    }
    if indices.is_empty() {
        return None;
    }
    let mut clipped = ParsedPolygon { vertices, indices, line_distances: distances, line_parameters: parameters, ..Default::default() };
    clipped.wind_counter_clockwise();
    Some(clipped)
}

/// Sutherland-Hodgman against the tile square, interpolating the attributes
/// at every intersection so the antialiasing field survives the clip.
fn clip_ring_to_tile(ring: Vec<AttributedPoint>, extent: f32) -> Vec<AttributedPoint> {
    fn clip(points: Vec<AttributedPoint>, plane: impl Fn(V2) -> f32) -> Vec<AttributedPoint> {
        if points.len() < 3 {
            return Vec::new();
        }
        let mut result = Vec::with_capacity(points.len() + 1);
        for i in 0..points.len() {
            let current = points[i];
            let previous = points[(i + points.len() - 1) % points.len()];
            let cd = plane(current.position);
            let pd = plane(previous.position);
            let inside = cd >= 0.0;
            if inside != (pd >= 0.0) {
                let t = pd / (pd - cd);
                result.push(AttributedPoint {
                    position: previous.position + (current.position - previous.position) * t,
                    distance: previous.distance + (current.distance - previous.distance) * t,
                    parameter: previous.parameter + (current.parameter - previous.parameter) * t,
                });
            }
            if inside {
                result.push(current);
            }
        }
        result
    }
    let c = clip(ring, |p| p.x);
    let c = clip(c, |p| extent - p.x);
    let c = clip(c, |p| p.y);
    clip(c, |p| extent - p.y)
}

fn sanitize_clipped_ring(ring: Vec<AttributedPoint>, extent: f32) -> Vec<AttributedPoint> {
    let mut out: Vec<AttributedPoint> = Vec::with_capacity(ring.len());
    for p in ring {
        let clamped = V2::new(p.position.x.clamp(0.0, extent), p.position.y.clamp(0.0, extent));
        if let Some(last) = out.last()
            && points_equal(last.position, clamped)
        {
            continue;
        }
        out.push(AttributedPoint { position: clamped, ..p });
    }
    if out.len() >= 2 && points_equal(out[out.len() - 1].position, out[0].position) {
        out.pop();
    }
    out
}

/// Positive counter-clockwise in render space, in f64 from the ring's first
/// point: the products of absolute tile coordinates run to millions, where a
/// float steps by a whole unit.
fn signed_area_f64(ring: &[AttributedPoint]) -> f64 {
    if ring.len() < 3 {
        return 0.0;
    }
    let (ox, oy) = (ring[0].position.x as f64, ring[0].position.y as f64);
    let mut area = 0f64;
    for i in 0..ring.len() {
        let n = (i + 1) % ring.len();
        let (cx, cy) = (ring[i].position.x as f64 - ox, ring[i].position.y as f64 - oy);
        let (nx, ny) = (ring[n].position.x as f64 - ox, ring[n].position.y as f64 - oy);
        area += cx * ny - nx * cy;
    }
    area * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(width: f64) -> LineOptions {
        LineOptions::new(width, 4096.0)
    }

    #[test]
    fn a_straight_segment_is_one_quad_counter_clockwise() {
        let p = ParseLine::new().parse(&[V2::new(100.0, 100.0), V2::new(300.0, 100.0)], &opts(20.0)).unwrap();
        assert_eq!(p.vertices.len(), 4);
        assert_eq!(p.indices.len(), 6);
        assert!(p.is_line_ribbon());
        assert_eq!(ParsedPolygon::first_clockwise_triangle(&p.vertices, &p.indices), None);
        // Render space: y flipped, extruded by half width plus the feather.
        assert_eq!(p.vertices[0], V2::new(100.0, 3996.0 + 12.0));
        assert_eq!(p.line_distances[..2], [127, -127]);
    }

    #[test]
    fn round_joins_and_caps_stay_counter_clockwise() {
        let mut o = opts(30.0);
        o.line_join_round = true;
        o.start_cap_round = true;
        o.end_cap_round = true;
        for points in [
            vec![V2::new(100.0, 100.0), V2::new(300.0, 100.0), V2::new(300.0, 300.0)],
            vec![V2::new(100.0, 100.0), V2::new(300.0, 100.0), V2::new(300.0, -50.0)],
        ] {
            let p = ParseLine::new().parse(&points, &o).unwrap();
            assert_eq!(ParsedPolygon::first_clockwise_triangle(&p.vertices, &p.indices), None);
        }
    }

    #[test]
    fn a_line_past_the_edge_is_clipped_to_the_tile() {
        let p = ParseLine::new().parse(&[V2::new(-100.0, 100.0), V2::new(300.0, 100.0)], &opts(20.0)).unwrap();
        assert!(p.vertices.iter().all(|v| v.x >= 0.0));
        assert_eq!(ParsedPolygon::first_clockwise_triangle(&p.vertices, &p.indices), None);
    }

    #[test]
    fn deferred_ribbons_keep_the_centreline_and_emit_edge_lines() {
        let mut o = opts(20.0);
        o.deferred_extrusion = true;
        let p = ParseLine::new().parse(&[V2::new(100.0, 100.0), V2::new(300.0, 100.0)], &o).unwrap();
        assert_eq!(p.vertices[0], V2::new(100.0, 3996.0));
        assert_eq!(p.line_normals[0], [0, 127]);
        assert_eq!(p.edge_line_indices, vec![0, 2, 1, 3]);
    }

    #[test]
    fn subdivided_rows_cross_the_grid_and_the_diagonals() {
        let rows = ParseLine::subdivided_rows(&[(V2::new(10.0, 10.0), 0.0), (V2::new(90.0, 10.0), 1.0)], 32.0);
        // x crosses 32 and 64, and the diagonal x - y = k * 32 crosses at x = 42 and 74.
        let xs: Vec<f32> = rows.iter().map(|r| r.0.x).collect();
        let expected = [10.0, 32.0, 42.0, 64.0, 74.0, 90.0];
        assert_eq!(xs.len(), expected.len(), "{xs:?}");
        for (x, e) in xs.iter().zip(expected) {
            assert!((x - e).abs() < 1e-3, "{xs:?}");
        }
        assert!((rows[2].1 - 0.4).abs() < 1e-6);
    }

    #[test]
    fn the_clipper_splits_a_line_leaving_and_reentering() {
        let fragments = LineClipper::new().clip(
            &[V2::new(100.0, 100.0), V2::new(5000.0, 100.0), V2::new(5000.0, 200.0), V2::new(100.0, 200.0)],
            4096.0,
            0.0,
        );
        assert_eq!(fragments.len(), 2);
        assert!(fragments[0].end_clipped && !fragments[0].start_clipped);
        assert!(fragments[1].start_clipped && !fragments[1].end_clipped);
    }
}
