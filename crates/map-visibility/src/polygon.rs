// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The frustum's footprint on the flat plane: the convex polygon the
//! coverage tests every tile square against, built once per pose from the
//! camera matrix.
//!
//! On the ground relief the ground is not the plane: a tile whose square
//! lies outside the plane's footprint still rises into view where the relief
//! lifts it, and ground below the plane shows past the far edge. The
//! footprint is then the frustum's over the slab of the world between the
//! relief's lowest and highest heights: its cut with each of the slab's two
//! planes and the corners of the frustum within the slab, under one convex
//! hull.

/// The tolerance of the footprint's plane cuts and polygon tests, in world units.
pub const PLANE_INTERSECTION_TOLERANCE: f64 = 1e-5;
/// `Float.ulpOfOne`: the edges the Swift engine treats as level in its slab test.
const LEVEL_EDGE_TOLERANCE: f64 = 1.1920928955078125e-7;

use map_geo::matrix::inverted;

type Point = [f64; 2];
type Point3 = [f64; 3];

/// A convex polygon of the plane, in world units, with its bounds.
#[derive(Debug, Clone, PartialEq)]
pub struct CoveragePolygon {
    vertices: Vec<Point>,
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

impl CoveragePolygon {
    pub fn new(vertices: Vec<Point>) -> CoveragePolygon {
        let first = vertices.first().copied().unwrap_or([0.0, 0.0]);
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (first[0], first[0], first[1], first[1]);
        for vertex in vertices.iter().skip(1) {
            min_x = min_x.min(vertex[0]);
            max_x = max_x.max(vertex[0]);
            min_y = min_y.min(vertex[1]);
            max_y = max_y.max(vertex[1]);
        }
        CoveragePolygon { vertices, min_x, max_x, min_y, max_y }
    }

    pub fn vertices(&self) -> &[Point] {
        &self.vertices
    }

    /// The axis-aligned bounds: the least and the greatest x, then y.
    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        (self.min_x, self.max_x, self.min_y, self.max_y)
    }

    /// The polygon scaled about a point by `factor`: the footprint pushed out
    /// from the camera's ground point, which the far texture bands are asked
    /// for ahead of a turn. Convex stays convex, and the winding is kept.
    pub fn inflated(&self, center_x: f64, center_y: f64, factor: f64) -> CoveragePolygon {
        if factor == 1.0 {
            return self.clone();
        }
        CoveragePolygon::new(self.vertices.iter().map(|v| [center_x + (v[0] - center_x) * factor, center_y + (v[1] - center_y) * factor]).collect())
    }

    /// Twice the signed area: the sign is the winding.
    pub fn signed_area(&self) -> f64 {
        let vertices = &self.vertices;
        let mut area = 0.0;
        for index in 0..vertices.len() {
            let start = vertices[index];
            let end = vertices[(index + 1) % vertices.len()];
            area += start[0] * end[1] - end[0] * start[1];
        }
        area
    }

    /// Whether the polygon meets an axis-aligned square of the plane: the
    /// polygon is convex, so its cut with the square's horizontal band is
    /// one x interval, and the square meets it exactly when that interval
    /// overlaps the square's own.
    pub fn intersects(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> bool {
        let tolerance = PLANE_INTERSECTION_TOLERANCE;
        if self.max_x < min_x - tolerance || self.min_x > max_x + tolerance || self.max_y < min_y - tolerance || self.min_y > max_y + tolerance {
            return false;
        }
        match self.horizontal_slab_x_range(min_y - tolerance, max_y + tolerance) {
            Some((lowest, highest)) => lowest <= max_x + tolerance && highest >= min_x - tolerance,
            None => false,
        }
    }

    /// The polygon cut by the line through `a` and `b`, keeping the side
    /// `inside` lies on (Sutherland-Hodgman against one half-plane). `None`
    /// when nothing of it is left, or too little to have an area.
    pub fn clipped(&self, inside: Point, a: Point, b: Point) -> Option<CoveragePolygon> {
        let edge_x = b[0] - a[0];
        let edge_y = b[1] - a[1];
        let side = |x: f64, y: f64| edge_x * (y - a[1]) - edge_y * (x - a[0]);
        let inside_side = side(inside[0], inside[1]);
        if !(inside_side.abs() > 1e-18) {
            return Some(self.clone());
        }
        let orientation = if inside_side > 0.0 { 1.0 } else { -1.0 };
        let points = &self.vertices;
        let mut clipped: Vec<Point> = Vec::with_capacity(points.len() + 2);
        for index in 0..points.len() {
            let current = points[index];
            let previous = points[(index + points.len() - 1) % points.len()];
            let current_inside = orientation * side(current[0], current[1]) >= 0.0;
            let previous_inside = orientation * side(previous[0], previous[1]) >= 0.0;
            if current_inside {
                if !previous_inside {
                    clipped.push(intersection(previous, current, a, b));
                }
                clipped.push(current);
            } else if previous_inside {
                clipped.push(intersection(previous, current, a, b));
            }
        }
        if clipped.len() < 3 {
            return None;
        }
        let polygon = CoveragePolygon::new(clipped);
        if !(polygon.signed_area().abs() > PLANE_INTERSECTION_TOLERANCE) {
            return None;
        }
        Some(polygon)
    }

    /// The x interval of the polygon's cut with a horizontal band, `None`
    /// where the band misses it. The cut is a convex region whose x extrema
    /// lie at the polygon's vertices inside the band or where its edges cross
    /// the band's boundaries, so no interior point is enumerated.
    pub fn horizontal_slab_x_range(&self, slab_min_y: f64, slab_max_y: f64) -> Option<(f64, f64)> {
        let vertices = &self.vertices;
        let mut lowest = f64::INFINITY;
        let mut highest = f64::NEG_INFINITY;
        let mut has_intersection = false;
        for index in 0..vertices.len() {
            let start = vertices[index];
            let end = vertices[(index + 1) % vertices.len()];
            let (start_x, start_y) = (start[0], start[1]);
            if start_y >= slab_min_y && start_y <= slab_max_y {
                lowest = lowest.min(start_x);
                highest = highest.max(start_x);
                has_intersection = true;
            }
            let delta_y = end[1] - start_y;
            if !(delta_y.abs() > LEVEL_EDGE_TOLERANCE) {
                continue;
            }
            let delta_x = end[0] - start_x;
            let t_at_min = (slab_min_y - start_y) / delta_y;
            if (0.0..=1.0).contains(&t_at_min) {
                let x = start_x + delta_x * t_at_min;
                lowest = lowest.min(x);
                highest = highest.max(x);
                has_intersection = true;
            }
            let t_at_max = (slab_max_y - start_y) / delta_y;
            if (0.0..=1.0).contains(&t_at_max) {
                let x = start_x + delta_x * t_at_max;
                lowest = lowest.min(x);
                highest = highest.max(x);
                has_intersection = true;
            }
        }
        has_intersection.then_some((lowest, highest))
    }
}

/// Where the segment from `a` to `b` crosses the line through `c` and `d`.
fn intersection(a: Point, b: Point, c: Point, d: Point) -> Point {
    let (ab_x, ab_y) = (b[0] - a[0], b[1] - a[1]);
    let (cd_x, cd_y) = (d[0] - c[0], d[1] - c[1]);
    let denominator = ab_x * cd_y - ab_y * cd_x;
    if !(denominator.abs() > 1e-18) {
        return a;
    }
    let t = ((c[0] - a[0]) * cd_y - (c[1] - a[1]) * cd_x) / denominator;
    [a[0] + ab_x * t, a[1] + ab_y * t]
}

const CLIP_SPACE_CORNERS: [Point3; 8] = [[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0], [-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]];
const FRUSTUM_EDGES: [(usize, usize); 12] = [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4), (0, 4), (1, 5), (2, 6), (3, 7)];

fn unprojected(point: Point3, inverse: &[f64; 16]) -> Option<Point3> {
    let [x, y, z] = point;
    let hx = inverse[0] * x + inverse[4] * y + inverse[8] * z + inverse[12];
    let hy = inverse[1] * x + inverse[5] * y + inverse[9] * z + inverse[13];
    let hz = inverse[2] * x + inverse[6] * y + inverse[10] * z + inverse[14];
    let hw = inverse[3] * x + inverse[7] * y + inverse[11] * z + inverse[15];
    if !hw.is_finite() || !(hw.abs() > PLANE_INTERSECTION_TOLERANCE) {
        return None;
    }
    let world = [hx / hw, hy / hw, hz / hw];
    world.iter().all(|value| value.is_finite()).then_some(world)
}

fn append_unique(x: f64, y: f64, intersections: &mut Vec<Point>) {
    let limit = PLANE_INTERSECTION_TOLERANCE * PLANE_INTERSECTION_TOLERANCE;
    for existing in intersections.iter() {
        let (dx, dy) = (existing[0] - x, existing[1] - y);
        if dx * dx + dy * dy <= limit {
            return;
        }
    }
    intersections.push([x, y]);
}

fn append_if_on_plane(point: Point3, plane_z: f64, intersections: &mut Vec<Point>) {
    if (point[2] - plane_z).abs() <= PLANE_INTERSECTION_TOLERANCE {
        append_unique(point[0], point[1], intersections);
    }
}

fn append_plane_intersections(start: Point3, end: Point3, plane_z: f64, intersections: &mut Vec<Point>) {
    append_if_on_plane(start, plane_z, intersections);
    append_if_on_plane(end, plane_z, intersections);
    let denominator = start[2] - end[2];
    if !(denominator.abs() > PLANE_INTERSECTION_TOLERANCE) {
        return;
    }
    let t = (start[2] - plane_z) / denominator;
    if !(t >= -PLANE_INTERSECTION_TOLERANCE && t <= 1.0 + PLANE_INTERSECTION_TOLERANCE) {
        return;
    }
    let clamped = t.max(0.0).min(1.0);
    let x = start[0] + (end[0] - start[0]) * clamped;
    let y = start[1] + (end[1] - start[1]) * clamped;
    let z = start[2] + (end[2] - start[2]) * clamped;
    if (z - plane_z).abs() <= PLANE_INTERSECTION_TOLERANCE {
        append_unique(x, y, intersections);
    }
}

/// A stable insertion sort. The orders it is used with compare with a
/// tolerance, which is no total order, so the standard sort (free to give
/// up on such an order) is not asked.
fn insertion_sort<T: Copy>(values: &mut [T], precedes: impl Fn(&T, &T) -> bool) {
    for index in 1..values.len() {
        let value = values[index];
        let mut slot = index;
        while slot > 0 && precedes(&value, &values[slot - 1]) {
            values[slot] = values[slot - 1];
            slot -= 1;
        }
        values[slot] = value;
    }
}

/// The vertices in the order of their angle about the centroid, none for fewer than three.
fn sorted_by_angle(vertices: &[Point]) -> Vec<Point> {
    if vertices.len() < 3 {
        return Vec::new();
    }
    let (mut sum_x, mut sum_y) = (0.0, 0.0);
    for vertex in vertices {
        sum_x += vertex[0];
        sum_y += vertex[1];
    }
    let centroid_x = sum_x / vertices.len() as f64;
    let centroid_y = sum_y / vertices.len() as f64;
    let mut keyed: Vec<(f64, Point)> = vertices.iter().map(|v| ((v[1] - centroid_y).atan2(v[0] - centroid_x), *v)).collect();
    insertion_sort(&mut keyed, |lhs, rhs| {
        if (lhs.0 - rhs.0).abs() > PLANE_INTERSECTION_TOLERANCE {
            return lhs.0 < rhs.0;
        }
        if (lhs.1[0] - rhs.1[0]).abs() > PLANE_INTERSECTION_TOLERANCE {
            return lhs.1[0] < rhs.1[0];
        }
        lhs.1[1] < rhs.1[1]
    });
    keyed.into_iter().map(|(_, vertex)| vertex).collect()
}

fn polygon_signed_area(vertices: &[Point]) -> f64 {
    if vertices.len() < 3 {
        return 0.0;
    }
    let mut area = 0.0;
    for index in 0..vertices.len() {
        let current = vertices[index];
        let next = vertices[(index + 1) % vertices.len()];
        area += current[0] * next[1] - next[0] * current[1];
    }
    area * 0.5
}

/// The convex hull of the points (Andrew's monotone chain), without the points on its edges.
pub fn convex_hull(points: &[Point]) -> Vec<Point> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut sorted = points.to_vec();
    sorted.sort_by(|lhs, rhs| lhs[0].total_cmp(&rhs[0]).then(lhs[1].total_cmp(&rhs[1])));
    let cross = |o: Point, a: Point, b: Point| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut lower: Vec<Point> = Vec::new();
    for &point in &sorted {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], point) <= 0.0 {
            lower.pop();
        }
        lower.push(point);
    }
    let mut upper: Vec<Point> = Vec::new();
    for &point in sorted.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], point) <= 0.0 {
            upper.pop();
        }
        upper.push(point);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// The footprint on the plane, or, with `heights` (the lowest and the
/// highest the relief reaches about the plane, in world units), over the
/// slab between them, the plane itself always within the slab.
/// `camera_matrix` is the frame's projection-view matrix, column-major.
/// `None` for a matrix with no inverse or a footprint with no area.
pub fn footprint(camera_matrix: &[f64; 16], heights: Option<(f64, f64)>) -> Option<CoveragePolygon> {
    let inverse = inverted(camera_matrix)?;
    let mut corners = [[0.0; 3]; 8];
    for (corner, clip) in corners.iter_mut().zip(CLIP_SPACE_CORNERS) {
        *corner = unprojected(clip, &inverse)?;
    }
    let (low, high) = match heights {
        Some((lower, upper)) => (lower.min(0.0), upper.max(0.0)),
        None => (0.0, 0.0),
    };
    let slab = low < high;
    let mut intersections: Vec<Point> = Vec::with_capacity(16);
    for &(start, end) in &FRUSTUM_EDGES {
        append_plane_intersections(corners[start], corners[end], low, &mut intersections);
    }
    if slab {
        for &(start, end) in &FRUSTUM_EDGES {
            append_plane_intersections(corners[start], corners[end], high, &mut intersections);
        }
        for corner in corners {
            if corner[2] > low && corner[2] < high {
                append_unique(corner[0], corner[1], &mut intersections);
            }
        }
    }
    let sorted = sorted_by_angle(&if slab { convex_hull(&intersections) } else { intersections });
    if sorted.len() < 3 || !(polygon_signed_area(&sorted).abs() > PLANE_INTERSECTION_TOLERANCE) {
        return None;
    }
    Some(CoveragePolygon::new(sorted))
}
