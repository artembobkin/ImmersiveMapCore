// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Lays a tile's road glyphs along their roads on screen, in the frame that
//! draws them (the Swift `RoadLabelPlacer`): the same numbers go to the
//! collision solve and to the road text vertex shader, so the boxes tested
//! are the glyphs drawn.
//!
//! A record's paths are projected once (one tile, one origin), then each
//! path's arc length is accumulated once and every label instance on it is
//! placed from that: the anchor's distance along the path from the anchor's
//! own projected point, the reading direction from the label's own span of
//! the path (a stitched road that hooks back has a chord pointing the wrong
//! way), then each glyph at its offset from the anchor, extrapolated straight
//! past a path end where the label overhangs it. A glyph is drawn when placed
//! and the path is long enough for the label. A path with any point behind
//! the camera draws nothing.
//!
//! Everything is flat arrays, computed in doubles and kept in floats, so a
//! host calls across a binding once per record and reads the result as the
//! buffers the GPU takes.

// The clamps keep the forms of the engine this is ported from, which pass NaN
// through where `clamp` would panic.
#![allow(clippy::manual_clamp)]

/// How far past half its label a road name's glyph box can reach from the anchor, in points: a glyph's own half height and more.
const OFFSCREEN_MARGIN_POINTS: f64 = 48.0;

/// Floats per glyph of `RoadLabelGeometry::glyphs`: its centre along the
/// label, the label's vertical centre, the label's width, the least path
/// length it needs, in points.
pub const GLYPH_FLOATS: usize = 4;
const GLYPH_CENTER: usize = 0;
const GLYPH_LABEL_WIDTH: usize = 2;
const GLYPH_MIN_LENGTH: usize = 3;

/// One tile record's roads and labels, fixed when the record is made. Ranges
/// are start and end pairs, end exclusive.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoadLabelGeometry {
    /// Every path's points as tile uv (two per point), path after path, then each anchor's own point.
    pub path_points: Vec<f32>,
    /// Each path's points in `path_points`.
    pub path_ranges: Vec<i32>,
    /// Each path's label instances, contiguous.
    pub path_instance_ranges: Vec<i32>,
    /// Per instance: its path, the segment it sits on, the index of its own point in `path_points`.
    pub anchors: Vec<i32>,
    /// Each instance's glyphs, contiguous.
    pub instance_glyph_ranges: Vec<i32>,
    /// `GLYPH_FLOATS` per glyph.
    pub glyphs: Vec<f32>,
    /// Per glyph, the collision box half size in points before rotation.
    pub glyph_half_sizes: Vec<f32>,
    pub glyph_count: usize,
}

/// Where a record's glyphs go this frame, index-aligned with its glyphs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoadLabelPlacement {
    /// Per glyph: x, y in device pixels (origin bottom left), the angle, 1 when drawn.
    pub placements: Vec<f32>,
    /// Glyphs placed past a path end: drawn but never offered to the solve.
    pub extrapolated: Vec<u8>,
    /// The rotated collision box half sizes, two per glyph.
    pub box_half_sizes: Vec<f32>,
}

impl RoadLabelPlacement {
    pub fn resize(&mut self, glyph_count: usize) {
        if self.extrapolated.len() != glyph_count {
            self.placements = vec![0.0; glyph_count * 4];
            self.extrapolated = vec![0; glyph_count];
            self.box_half_sizes = vec![0.0; glyph_count * 2];
        }
    }

    pub fn hide_all(&mut self) {
        self.placements.fill(0.0);
        self.extrapolated.fill(0);
    }
}

/// The projected paths, reused between records and frames.
#[derive(Debug, Clone, Default)]
pub struct RoadLabelScratch {
    /// Two per point, device pixels with the origin at the bottom left.
    pub screen_points: Vec<f64>,
    pub visible: Vec<u8>,
    cumulative: Vec<f64>,
}

/// The lift of a tile's path points on the ground relief: the terrain's
/// metres under each point, the frame's datum, and the world units a metre
/// rises by times the relief's strength.
#[derive(Debug, Clone, Copy)]
pub struct PointLifts<'a> {
    pub metres: &'a [f32],
    pub datum_metres: f64,
    pub units_per_metre: f64,
}

/// A frame's projection: the projection-view matrix (column-major) and the viewport in device pixels.
#[derive(Debug, Clone, Copy)]
pub struct RoadLabelFrame<'a> {
    pub projection_view: &'a [f64; 16],
    pub viewport_width: f64,
    pub viewport_height: f64,
    pub pixels_per_point: f64,
}

/// Projects the record's path points onto the flat map and places its
/// glyphs. `origin` is the record's tile on the map (x, y, size in render
/// units). A point deeper than `maximum_depth` (clip w) is out of reach, and
/// a glyph on it is not placed.
pub fn place(geometry: &RoadLabelGeometry, origin: [f64; 3], frame: &RoadLabelFrame<'_>, maximum_depth: f64, lifts: Option<PointLifts<'_>>, scratch: &mut RoadLabelScratch, output: &mut RoadLabelPlacement) {
    project(&geometry.path_points, origin, frame, maximum_depth, lifts, scratch);
    let RoadLabelScratch { screen_points, visible, cumulative } = scratch;
    place_from(geometry, screen_points, visible, frame.pixels_per_point, frame.viewport_width, frame.viewport_height, cumulative, output);
}

/// The flat projection of a tile's points, device pixels with the origin at
/// the bottom left. `lifts` raises each point by the ground relief under it:
/// the road's names ride the relief with it.
pub fn project(path_points: &[f32], origin: [f64; 3], frame: &RoadLabelFrame<'_>, maximum_depth: f64, lifts: Option<PointLifts<'_>>, scratch: &mut RoadLabelScratch) {
    let count = path_points.len() / 2;
    if scratch.visible.len() < count {
        scratch.screen_points.resize(count * 2, 0.0);
        scratch.visible.resize(count, 0);
    }
    let [ox, oy, size] = origin;
    let m = frame.projection_view;
    let half_width = frame.viewport_width * 0.5;
    let half_height = frame.viewport_height * 0.5;
    let screen = &mut scratch.screen_points[..count * 2];
    let visible = &mut scratch.visible[..count];
    // The lift as two scalars over the kept metres: nothing read per point but the metres themselves.
    let lifts = lifts.filter(|lifts| lifts.metres.len() == count);
    for index in 0..count {
        // v grows from the north edge, the flat render world is y up.
        let x = ox + path_points[index * 2] as f64 * size;
        let y = oy + (1.0 - path_points[index * 2 + 1] as f64) * size;
        let z = lifts.map_or(0.0, |lifts| (lifts.metres[index] as f64 - lifts.datum_metres) * lifts.units_per_metre);
        let cw = m[3] * x + m[7] * y + m[11] * z + m[15];
        if cw > 0.0 {
            screen[index * 2] = ((m[0] * x + m[4] * y + m[8] * z + m[12]) / cw + 1.0) * half_width;
            screen[index * 2 + 1] = ((m[1] * x + m[5] * y + m[9] * z + m[13]) / cw + 1.0) * half_height;
            visible[index] = (cw <= maximum_depth) as u8;
        } else {
            screen[index * 2] = 0.0;
            screen[index * 2 + 1] = 0.0;
            visible[index] = 0;
        }
    }
}

/// Places the record's glyphs from its projected path points. An instance
/// whose anchor lies farther outside the viewport than half its label and a
/// margin is left hidden unplaced: none of its glyphs can reach the screen,
/// so the solve would keep none of it either, and the frame skips its
/// glyphs' trigonometry. An infinite viewport places every instance.
#[allow(clippy::too_many_arguments)]
pub fn place_projected(geometry: &RoadLabelGeometry, points: &[f64], visible: &[u8], pixels_per_point: f64, viewport_width: f64, viewport_height: f64, scratch: &mut RoadLabelScratch, output: &mut RoadLabelPlacement) {
    place_from(geometry, points, visible, pixels_per_point, viewport_width, viewport_height, &mut scratch.cumulative, output);
}

/// The first segment of a path (`count` points, arc lengths in `cumulative`)
/// whose end lies at `distance` or past it, `None` past the path's end: the
/// segment the Swift placer's linear walk stops at, found by bisection. A
/// zero-length segment is never the first such one for a distance above 0.
fn segment_at(cumulative: &[f64], count: usize, distance: f64) -> Option<usize> {
    if count < 2 || cumulative[count - 1] < distance {
        return None;
    }
    let (mut low, mut high) = (1usize, count - 1);
    while low < high {
        let middle = (low + high) >> 1;
        if cumulative[middle] >= distance {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    Some(low - 1)
}

#[allow(clippy::too_many_arguments)]
fn place_from(geometry: &RoadLabelGeometry, points: &[f64], visible: &[u8], pixels_per_point: f64, viewport_width: f64, viewport_height: f64, cumulative: &mut Vec<f64>, output: &mut RoadLabelPlacement) {
    output.resize(geometry.glyph_count);
    let path_count = geometry.path_ranges.len() / 2;
    let mut longest_path = 0usize;
    for path in 0..path_count {
        longest_path = longest_path.max((geometry.path_ranges[path * 2 + 1] - geometry.path_ranges[path * 2]).max(0) as usize);
    }
    if cumulative.len() < longest_path {
        cumulative.resize(longest_path, 0.0);
    }
    if geometry.glyph_count == 0 || points.len() < geometry.path_points.len() {
        output.hide_all();
        return;
    }
    let placements = &mut output.placements;
    let extrapolated = &mut output.extrapolated;
    let boxes = &mut output.box_half_sizes;
    let glyphs = &geometry.glyphs;
    let point_along = |cumulative: &[f64], start: usize, count: usize, distance: f64| -> (f64, f64) {
        let Some(segment) = segment_at(cumulative, count, distance) else {
            let last = (start + count - 1) * 2;
            return (points[last], points[last + 1]);
        };
        let segment_length = cumulative[segment + 1] - cumulative[segment];
        let t = if segment_length > 0.0 { ((distance - cumulative[segment]) / segment_length).max(0.0).min(1.0) } else { 0.0 };
        let a = (start + segment) * 2;
        (points[a] + (points[a + 2] - points[a]) * t, points[a + 1] + (points[a + 3] - points[a + 1]) * t)
    };
    for path in 0..path_count {
        let start = geometry.path_ranges[path * 2] as usize;
        let count = (geometry.path_ranges[path * 2 + 1] - geometry.path_ranges[path * 2]).max(0) as usize;
        // The path's arc length, and whether all of it is in front of the camera.
        let mut path_visible = count >= 2;
        if path_visible {
            cumulative[0] = 0.0;
            path_visible = visible[start] != 0;
            for offset in 1..count {
                if visible[start + offset] == 0 {
                    path_visible = false;
                }
                let a = (start + offset) * 2;
                // A square root and not `hypot`, which guards against an
                // overflow screen pixels cannot reach and costs most of the
                // placement where it is a library call.
                let (step_x, step_y) = (points[a] - points[a - 2], points[a + 1] - points[a - 1]);
                cumulative[offset] = cumulative[offset - 1] + (step_x * step_x + step_y * step_y).sqrt();
            }
        }
        let total_length = if count >= 2 { cumulative[count - 1] } else { 0.0 };
        let instance_start = geometry.path_instance_ranges[path * 2] as usize;
        let instance_end = geometry.path_instance_ranges[path * 2 + 1] as usize;
        for instance in instance_start..instance_end {
            let point_index = geometry.anchors[instance * 3 + 2];
            let glyph_start = geometry.instance_glyph_ranges[instance * 2] as usize;
            let glyph_end = geometry.instance_glyph_ranges[instance * 2 + 1] as usize;
            let anchor = (path_visible && point_index >= 0 && (point_index as usize) * 2 < points.len() && glyph_end > glyph_start)
                .then_some(point_index as usize)
                .filter(|&point| {
                    let reach = (glyphs[glyph_start * GLYPH_FLOATS + GLYPH_LABEL_WIDTH] as f64 * 0.5 + OFFSCREEN_MARGIN_POINTS) * pixels_per_point;
                    let (anchor_x, anchor_y) = (points[point * 2], points[point * 2 + 1]);
                    let offscreen = anchor_x < -reach || anchor_y < -reach || anchor_x > viewport_width + reach || anchor_y > viewport_height + reach;
                    visible[point] != 0 && !offscreen
                });
            let Some(point) = anchor else {
                for glyph in glyph_start..glyph_end.max(glyph_start) {
                    placements[glyph * 4..glyph * 4 + 4].fill(0.0);
                    extrapolated[glyph] = 0;
                }
                continue;
            };
            // The anchor's distance along the path: its own projected point
            // dropped onto its segment. Lerping the world t along the
            // projected segment is not the projection of the anchor under a
            // tilted camera.
            let segment = (geometry.anchors[instance * 3 + 1].max(0) as usize).min(count - 2);
            let s0 = (start + segment) * 2;
            let segment_x = points[s0 + 2] - points[s0];
            let segment_y = points[s0 + 3] - points[s0 + 1];
            let segment_length = cumulative[segment + 1] - cumulative[segment];
            let mut along = 0.0;
            if segment_length > 0.0 {
                let ax = points[point * 2] - points[s0];
                let ay = points[point * 2 + 1] - points[s0 + 1];
                along = ((ax * segment_x + ay * segment_y) / segment_length).max(0.0).min(segment_length);
            }
            let anchor_distance = cumulative[segment] + along;
            // Which way the text reads: the label's own span of the path,
            // half a label either side of the anchor, runs left on screen or
            // right.
            let label_width = glyphs[glyph_start * GLYPH_FLOATS + GLYPH_LABEL_WIDTH] as f64;
            let half_span = label_width * 0.5 * pixels_per_point;
            let ahead = point_along(cumulative, start, count, (anchor_distance + half_span).max(0.0).min(total_length));
            let behind = point_along(cumulative, start, count, (anchor_distance - half_span).max(0.0).min(total_length));
            let mut span_x = ahead.0 - behind.0;
            let span_y = ahead.1 - behind.1;
            if span_x * span_x + span_y * span_y <= 0.0 {
                span_x = segment_x;
            }
            let reverse = span_x < 0.0;
            let can_draw = total_length >= glyphs[glyph_start * GLYPH_FLOATS + GLYPH_MIN_LENGTH] as f64 * pixels_per_point;

            let mut cached_segment: Option<usize> = None;
            let mut cached_angle = 0.0f64;
            let mut box_angle = f64::NAN;
            let (mut box_sin, mut box_cos) = (0.0f64, 1.0f64);
            for glyph in glyph_start..glyph_end {
                // Glyph metrics are in layout points, the arc length they are placed along in device pixels.
                let mut glyph_offset = (glyphs[glyph * GLYPH_FLOATS + GLYPH_CENTER] as f64 - label_width * 0.5) * pixels_per_point;
                if reverse {
                    glyph_offset = -glyph_offset;
                }
                let target_distance = anchor_distance + glyph_offset;
                let (mut placed, mut overhangs) = (false, false);
                let (mut px, mut py, mut angle) = (0.0f64, 0.0f64, 0.0f64);
                if target_distance <= 0.0 {
                    let a = start * 2;
                    let (dx, dy) = (points[a + 2] - points[a], points[a + 3] - points[a + 1]);
                    let length = (dx * dx + dy * dy).sqrt();
                    if length > 0.0 {
                        px = points[a] + (dx / length) * target_distance;
                        py = points[a + 1] + (dy / length) * target_distance;
                        angle = dy.atan2(dx);
                        placed = true;
                        overhangs = true;
                    }
                } else if target_distance >= total_length {
                    let a = (start + count - 1) * 2;
                    let (dx, dy) = (points[a] - points[a - 2], points[a + 1] - points[a - 1]);
                    let length = (dx * dx + dy * dy).sqrt();
                    if length > 0.0 {
                        px = points[a] + (dx / length) * (target_distance - total_length);
                        py = points[a + 1] + (dy / length) * (target_distance - total_length);
                        angle = dy.atan2(dx);
                        placed = true;
                        overhangs = true;
                    }
                } else if let Some(found) = segment_at(cumulative, count, target_distance) {
                    // 0 < target_distance < total_length: the segment found has a length.
                    let length = cumulative[found + 1] - cumulative[found];
                    let t = (target_distance - cumulative[found]) / length;
                    let a = (start + found) * 2;
                    let (dx, dy) = (points[a + 2] - points[a], points[a + 3] - points[a + 1]);
                    px = points[a] + dx * t;
                    py = points[a + 1] + dy * t;
                    // The glyphs of one segment share its direction.
                    if cached_segment != Some(found) {
                        cached_segment = Some(found);
                        cached_angle = dy.atan2(dx);
                    }
                    angle = cached_angle;
                    placed = true;
                }
                if placed && reverse {
                    angle += std::f64::consts::PI;
                }
                let is_visible = placed && can_draw;
                placements[glyph * 4] = px as f32;
                placements[glyph * 4 + 1] = py as f32;
                placements[glyph * 4 + 2] = angle as f32;
                placements[glyph * 4 + 3] = is_visible as u8 as f32;
                extrapolated[glyph] = overhangs as u8;
                if is_visible {
                    let hx = geometry.glyph_half_sizes[glyph * 2] as f64 * pixels_per_point;
                    let hy = geometry.glyph_half_sizes[glyph * 2 + 1] as f64 * pixels_per_point;
                    if angle != box_angle {
                        box_angle = angle;
                        box_sin = angle.sin().abs();
                        box_cos = angle.cos().abs();
                    }
                    boxes[glyph * 2] = (box_cos * hx + box_sin * hy) as f32;
                    boxes[glyph * 2 + 1] = (box_sin * hx + box_cos * hy) as f32;
                } else {
                    boxes[glyph * 2] = 0.0;
                    boxes[glyph * 2 + 1] = 0.0;
                }
            }
        }
    }
}

/// The road glyph boxes offered to one solve: centres and half sizes, two floats per box.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoadBoxes {
    pub centers: Vec<f32>,
    pub half_sizes: Vec<f32>,
}

impl RoadBoxes {
    pub fn count(&self) -> usize {
        self.centers.len() / 2
    }

    pub fn clear(&mut self) {
        self.centers.clear();
        self.half_sizes.clear();
    }
}

/// Appends the glyph boxes of one instance from the record's placement, for
/// the collision solve. Returns false when the instance gets no decision: a
/// hidden glyph (path behind the camera or shorter than the label), a glyph
/// placed past a path end, or a turn between adjacent glyphs beyond
/// `max_glyph_turn_radians`. Then nothing is appended.
pub fn append_instance_boxes(glyph_start: usize, glyph_end: usize, output: &RoadLabelPlacement, max_glyph_turn_radians: f64, boxes: &mut RoadBoxes) -> bool {
    if glyph_end <= glyph_start || glyph_end > output.extrapolated.len() {
        return false;
    }
    let box_start = boxes.centers.len();
    let mut previous_angle: Option<f64> = None;
    for glyph in glyph_start..glyph_end {
        let angle = output.placements[glyph * 4 + 2] as f64;
        let turns_too_far = previous_angle.is_some_and(|previous| normalized_angle_delta(previous, angle).abs() > max_glyph_turn_radians);
        if output.placements[glyph * 4 + 3] == 0.0 || output.extrapolated[glyph] != 0 || turns_too_far {
            boxes.centers.truncate(box_start);
            boxes.half_sizes.truncate(box_start);
            return false;
        }
        previous_angle = Some(angle);
        boxes.centers.extend_from_slice(&output.placements[glyph * 4..glyph * 4 + 2]);
        boxes.half_sizes.extend_from_slice(&output.box_half_sizes[glyph * 2..glyph * 2 + 2]);
    }
    true
}

/// The turn from `lhs` to `rhs`, within half a turn either way.
pub fn normalized_angle_delta(lhs: f64, rhs: f64) -> f64 {
    let mut delta = rhs - lhs;
    while delta > std::f64::consts::PI {
        delta -= 2.0 * std::f64::consts::PI;
    }
    while delta < -std::f64::consts::PI {
        delta += 2.0 * std::f64::consts::PI;
    }
    delta
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDENTITY: [f64; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

    fn frame(projection_view: &[f64; 16], viewport_width: f64, viewport_height: f64) -> RoadLabelFrame<'_> {
        RoadLabelFrame { projection_view, viewport_width, viewport_height, pixels_per_point: 1.0 }
    }

    /// One straight path of two points along x with the anchor's own point after them, and one instance of a glyph per centre.
    fn straight(from_x: f64, to_x: f64, anchor_x: f64, glyph_centers: &[f32]) -> (RoadLabelGeometry, Vec<f64>) {
        let geometry = RoadLabelGeometry {
            path_points: vec![0.0; 6],
            path_ranges: vec![0, 2],
            path_instance_ranges: vec![0, 1],
            anchors: vec![0, 0, 2],
            instance_glyph_ranges: vec![0, glyph_centers.len() as i32],
            glyphs: glyph_centers.iter().flat_map(|center| [*center, 0.0, 100.0, 100.0]).collect(),
            glyph_half_sizes: glyph_centers.iter().flat_map(|_| [50.0, 10.0]).collect(),
            glyph_count: glyph_centers.len(),
        };
        (geometry, vec![from_x, 50.0, to_x, 50.0, anchor_x, 50.0])
    }

    fn placed(geometry: &RoadLabelGeometry, points: &[f64], visible: &[u8]) -> RoadLabelPlacement {
        let mut output = RoadLabelPlacement::default();
        place_projected(geometry, points, visible, 1.0, f64::INFINITY, f64::INFINITY, &mut RoadLabelScratch::default(), &mut output);
        output
    }

    #[test]
    fn a_point_past_the_reach_is_not_visible() {
        // The clip w grows with the world y: 1 at the tile's south edge, 11 at its north edge.
        let matrix = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        let points = [0.5f32, 1.0, 0.5, 0.0];
        let mut scratch = RoadLabelScratch::default();
        project(&points, [0.0, 0.0, 10.0], &frame(&matrix, 100.0, 100.0), 5.0, None, &mut scratch);
        assert_eq!(scratch.visible[..2], [1, 0]);
        project(&points, [0.0, 0.0, 10.0], &frame(&matrix, 100.0, 100.0), f64::INFINITY, None, &mut scratch);
        assert_eq!(scratch.visible[..2], [1, 1], "no reach keeps every point in front of the camera");
    }

    #[test]
    fn a_point_lands_in_device_pixels_origin_bottom_left() {
        let mut scratch = RoadLabelScratch::default();
        // uv (0.75, 0.25) of a tile at (-1, -1) two units wide is (0.5, 0.5).
        project(&[0.75, 0.25], [-1.0, -1.0, 2.0], &frame(&IDENTITY, 200.0, 100.0), f64::INFINITY, None, &mut scratch);
        assert_eq!(scratch.screen_points[..2], [150.0, 75.0]);
    }

    #[test]
    fn the_relief_lifts_a_point_along_the_matrixs_z() {
        // The screen x takes the world z: a point a metre over the datum moves by the units a metre rises.
        let matrix = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        let mut scratch = RoadLabelScratch::default();
        let lifts = PointLifts { metres: &[12.0], datum_metres: 10.0, units_per_metre: 0.25 };
        project(&[0.5, 0.5], [-1.0, -1.0, 2.0], &frame(&matrix, 200.0, 100.0), f64::INFINITY, Some(lifts), &mut scratch);
        assert_eq!(scratch.screen_points[..2], [150.0, 50.0]);
    }

    #[test]
    fn the_glyphs_centre_on_the_anchors_own_point() {
        let (geometry, points) = straight(0.0, 400.0, 230.0, &[25.0, 50.0, 75.0]);
        let output = placed(&geometry, &points, &[1, 1, 1]);
        let xs: Vec<f32> = (0..3).map(|glyph| output.placements[glyph * 4]).collect();
        assert_eq!(xs, [205.0, 230.0, 255.0]);
        assert!((0..3).all(|glyph| output.placements[glyph * 4 + 3] == 1.0 && output.extrapolated[glyph] == 0));
        assert_eq!(output.box_half_sizes[..2], [50.0, 10.0], "a level glyph keeps its box");
    }

    #[test]
    fn a_leftward_path_turns_the_text_and_a_short_one_draws_nothing() {
        let (geometry, points) = straight(400.0, 0.0, 230.0, &[25.0, 75.0]);
        let output = placed(&geometry, &points, &[1, 1, 1]);
        assert!(output.placements[0] < output.placements[4], "the text still reads left to right");
        assert!(output.placements[2].abs() < 1e-6 || (output.placements[2].abs() - 2.0 * std::f32::consts::PI).abs() < 1e-5, "turned by half a turn from the path's direction");
        let (short, points) = straight(0.0, 60.0, 30.0, &[50.0]);
        assert_eq!(placed(&short, &points, &[1, 1, 1]).placements[3], 0.0, "a path shorter than the label");
        let (hidden, points) = straight(0.0, 400.0, 230.0, &[50.0]);
        assert_eq!(placed(&hidden, &points, &[1, 0, 1]).placements[3], 0.0, "a path point behind the camera");
    }

    #[test]
    fn a_glyph_past_the_path_end_is_extrapolated_and_gets_no_box() {
        let (geometry, points) = straight(0.0, 400.0, 390.0, &[50.0, 90.0]);
        let output = placed(&geometry, &points, &[1, 1, 1]);
        assert_eq!(output.extrapolated, [0, 1]);
        assert_eq!(output.placements[4], 430.0);
        let mut boxes = RoadBoxes::default();
        assert!(!append_instance_boxes(0, 2, &output, 1.0, &mut boxes));
        assert_eq!(boxes.count(), 0);
        assert!(append_instance_boxes(0, 1, &output, 1.0, &mut boxes));
        assert_eq!((boxes.centers.as_slice(), boxes.half_sizes.as_slice()), ([390.0f32, 50.0].as_slice(), [50.0f32, 10.0].as_slice()));
    }

    #[test]
    fn a_sharp_turn_between_glyphs_gets_no_decision() {
        let output = RoadLabelPlacement { placements: vec![10.0, 20.0, 0.1, 1.0, 30.0, 40.0, 1.5, 1.0], extrapolated: vec![0, 0], box_half_sizes: vec![4.0, 6.0, 5.0, 7.0] };
        let mut boxes = RoadBoxes::default();
        assert!(!append_instance_boxes(0, 2, &output, 1.0, &mut boxes));
        assert!(append_instance_boxes(0, 2, &output, 2.0, &mut boxes));
        assert_eq!(boxes.count(), 2);
        assert!((normalized_angle_delta(3.0, -3.0) - (2.0 * std::f64::consts::PI - 6.0)).abs() < 1e-12, "the short way around");
    }
}
