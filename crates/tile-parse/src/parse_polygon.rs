// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Polygon fills: clipped to the tile square (or a quarter of it) in tile
//! space, then flipped into render space and tessellated, a convex ring by a
//! fan and any other by earcut. Or kept as an outline for the flattening.

use mvt::geometry::{Point, Polygon};

use crate::coordinate_space::render_points;
use crate::geom::V2;
use crate::parsed_polygon::ParsedPolygon;

const EPSILON: f32 = 0.0001;

/// Rings in tile space (y down, raw MVT), clipped to the tile square.
#[derive(Debug, Clone, PartialEq)]
pub struct ClippedPolygon {
    pub exterior: Vec<V2>,
    pub interiors: Vec<Vec<V2>>,
}

/// `clipped` is tile space, `parsed` the tessellated result in render
/// space. The flip between them happens once, in `triangulate_geometry`.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedGeometry {
    pub clipped: ClippedPolygon,
    pub parsed: ParsedPolygon,
}

/// A square of tile space a polygon is clipped to: the tile's own, or one
/// of its quarters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipSquare {
    pub minimum: V2,
    pub maximum: V2,
}

impl ClipSquare {
    pub fn tile(extent: f32) -> ClipSquare {
        ClipSquare { minimum: V2::ZERO, maximum: V2::new(extent, extent) }
    }
}

/// Sutherland-Hodgman in f64, the Swift `Clipper`.
pub mod clipper {
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct P {
        pub x: f64,
        pub y: f64,
    }

    #[inline(always)]
    fn is_inside(p1: P, p2: P, p3: P) -> bool {
        (p3.x - p2.x) * (p1.y - p2.y) > (p3.y - p2.y) * (p1.x - p2.x)
    }

    #[inline(always)]
    fn intersection(p1: P, p2: P, s: P, e: P) -> P {
        let dc = P { x: p1.x - p2.x, y: p1.y - p2.y };
        let dp = P { x: s.x - e.x, y: s.y - e.y };
        let n1 = p1.x * p2.y - p1.y * p2.x;
        let n2 = s.x * e.y - s.y * e.x;
        let n3 = 1.0 / (dc.x * dp.y - dc.y * dp.x);
        P { x: (n1 * dp.x - n2 * dc.x) * n3, y: (n1 * dp.y - n2 * dc.y) * n3 }
    }

    pub fn sutherland_hodgman(subject: Vec<P>, clip: &[P]) -> Option<Vec<P>> {
        let mut ring = subject;
        let mut p1 = *clip.last()?;
        for &p2 in clip {
            let input = std::mem::take(&mut ring);
            let mut s = *input.last()?;
            for &e in &input {
                if is_inside(e, p1, p2) {
                    if !is_inside(s, p1, p2) {
                        ring.push(intersection(p1, p2, s, e));
                    }
                    ring.push(e);
                } else if is_inside(s, p1, p2) {
                    ring.push(intersection(p1, p2, s, e));
                }
                s = e;
            }
            p1 = p2;
        }
        if ring.is_empty() { None } else { Some(ring) }
    }
}

/// Keeps scratch buffers between polygons: one per tile, never shared.
#[derive(Default)]
pub struct ParsePolygon {
    earcut: earcut::Earcut<f64>,
    earcut_coordinates: Vec<[f64; 2]>,
    earcut_holes: Vec<u32>,
    earcut_vertices: Vec<V2>,
    triangles: Vec<u32>,
}

impl ParsePolygon {
    pub fn new() -> ParsePolygon {
        ParsePolygon::default()
    }

    pub fn parse_geometry(&mut self, polygon: &Polygon, tile_extent: f32) -> Option<ParsedGeometry> {
        let clipped = clip(polygon, ClipSquare::tile(tile_extent))?;
        self.triangulate_geometry(clipped)
    }

    /// The polygon clipped to `square` of its tile and tessellated, `None`
    /// where none of it lies in the square: a building's piece in a quarter.
    pub fn parse_geometry_within(&mut self, polygon: &Polygon, square: ClipSquare) -> Option<ParsedGeometry> {
        let clipped = clip(polygon, square)?;
        self.triangulate_geometry(clipped)
    }

    pub fn parse(&mut self, polygon: &Polygon, tile_extent: f32) -> Option<ParsedPolygon> {
        self.parse_geometry(polygon, tile_extent).map(|g| g.parsed)
    }

    /// The fill as its outline, for a tile the flattening takes: clipped,
    /// flipped into render space, never tessellated. The flattener fills a
    /// paint's rings by the nonzero rule, so each ring's orientation is
    /// settled here by its own area: the exterior counter-clockwise, a hole
    /// clockwise.
    pub fn parse_outline(&mut self, polygon: &Polygon, tile_extent: f32) -> Option<ParsedPolygon> {
        let clipped = clip(polygon, ClipSquare::tile(tile_extent))?;
        let mut parsed = ParsedPolygon::default();
        parsed.vertices.reserve(clipped.exterior.len() + clipped.interiors.iter().map(Vec::len).sum::<usize>());
        append_outline_ring(&render_points(&clipped.exterior), true, &mut parsed);
        for interior in &clipped.interiors {
            append_outline_ring(&render_points(interior), false, &mut parsed);
        }
        Some(parsed)
    }

    fn triangulate_geometry(&mut self, clipped: ClippedPolygon) -> Option<ParsedGeometry> {
        // The one entry into render space for fills. The flip must precede
        // the winding decisions: reflecting afterwards would invert every
        // triangle's orientation.
        let render_exterior = render_points(&clipped.exterior);
        if clipped.interiors.is_empty()
            && let Some(mut polygon) = triangulate_convex(render_exterior.clone())
        {
            polygon.wind_counter_clockwise();
            return Some(ParsedGeometry { clipped, parsed: polygon });
        }
        let render_interiors: Vec<Vec<V2>> = clipped.interiors.iter().map(|r| render_points(r)).collect();
        let mut polygon = self.triangulate_earcut(&render_exterior, &render_interiors)?;
        // Earcut can hand back the odd inverted ear.
        polygon.wind_counter_clockwise();
        Some(ParsedGeometry { clipped, parsed: polygon })
    }

    fn triangulate_earcut(&mut self, exterior: &[V2], interiors: &[Vec<V2>]) -> Option<ParsedPolygon> {
        self.earcut_coordinates.clear();
        self.earcut_holes.clear();
        self.earcut_vertices.clear();
        let append = |ring: &[V2], coords: &mut Vec<[f64; 2]>, verts: &mut Vec<V2>| {
            for &p in ring {
                verts.push(p);
                coords.push([p.x as f64, p.y as f64]);
            }
        };
        append(exterior, &mut self.earcut_coordinates, &mut self.earcut_vertices);
        for ring in interiors {
            self.earcut_holes.push(self.earcut_vertices.len() as u32);
            append(ring, &mut self.earcut_coordinates, &mut self.earcut_vertices);
        }
        self.earcut.earcut_slice(&self.earcut_coordinates, &self.earcut_holes, &mut self.triangles);
        if self.triangles.is_empty() {
            return None;
        }
        Some(ParsedPolygon::new(self.earcut_vertices.clone(), self.triangles.clone()))
    }
}

fn append_outline_ring(ring: &[V2], counter_clockwise: bool, parsed: &mut ParsedPolygon) {
    parsed.outline_starts.push(parsed.vertices.len() as i32);
    if (signed_area(ring) >= 0.0) == counter_clockwise {
        parsed.vertices.extend_from_slice(ring);
    } else {
        parsed.vertices.extend(ring.iter().rev());
    }
}

fn triangulate_convex(exterior: Vec<V2>) -> Option<ParsedPolygon> {
    if exterior.len() < 3 || !is_convex(&exterior) {
        return None;
    }
    let clockwise = signed_area(&exterior) < 0.0;
    let mut indices = Vec::with_capacity((exterior.len() - 2) * 3);
    for i in 1..exterior.len() - 1 {
        indices.push(0);
        if clockwise {
            indices.push(i as u32 + 1);
            indices.push(i as u32);
        } else {
            indices.push(i as u32);
            indices.push(i as u32 + 1);
        }
    }
    if indices.is_empty() { None } else { Some(ParsedPolygon::new(exterior, indices)) }
}

/// Clips a polygon to a square of tile space. A ring fully inside is only
/// sanitized, any other goes through Sutherland-Hodgman in f64.
pub fn clip(polygon: &Polygon, square: ClipSquare) -> Option<ClippedPolygon> {
    if is_fully_inside(&polygon.exterior, square) {
        let exterior = sanitize_ring(convert_ring(&polygon.exterior));
        if exterior.len() < 3 {
            return None;
        }
        let interiors = polygon
            .interiors
            .iter()
            .map(|r| sanitize_ring(convert_ring(r)))
            .filter(|r| r.len() >= 3)
            .collect();
        return Some(ClippedPolygon { exterior, interiors });
    }
    use clipper::P;
    let clip_poly = [
        P { x: square.minimum.x as f64, y: square.minimum.y as f64 },
        P { x: square.maximum.x as f64, y: square.minimum.y as f64 },
        P { x: square.maximum.x as f64, y: square.maximum.y as f64 },
        P { x: square.minimum.x as f64, y: square.maximum.y as f64 },
    ];
    let to_p = |ring: &[Point]| ring.iter().map(|p| P { x: p.x as f64, y: p.y as f64 }).collect::<Vec<_>>();
    let exterior_clipped = clipper::sutherland_hodgman(to_p(&polygon.exterior), &clip_poly)?;
    let to_v = |ring: Vec<P>| ring.into_iter().map(|p| V2::new(p.x as f32, p.y as f32)).collect::<Vec<_>>();
    let exterior = sanitize_ring(to_v(exterior_clipped));
    if exterior.len() < 3 {
        return None;
    }
    let interiors = polygon
        .interiors
        .iter()
        .filter_map(|r| clipper::sutherland_hodgman(to_p(r), &clip_poly))
        .map(|r| sanitize_ring(to_v(r)))
        .filter(|r| r.len() >= 3)
        .collect();
    Some(ClippedPolygon { exterior, interiors })
}

fn is_fully_inside(ring: &[Point], square: ClipSquare) -> bool {
    let Some(first) = ring.first() else { return false };
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (first.x as f32, first.x as f32, first.y as f32, first.y as f32);
    for p in &ring[1..] {
        let (x, y) = (p.x as f32, p.y as f32);
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    min_x >= square.minimum.x && max_x <= square.maximum.x && min_y >= square.minimum.y && max_y <= square.maximum.y
}

fn convert_ring(ring: &[Point]) -> Vec<V2> {
    ring.iter().map(|p| V2::new(p.x as f32, p.y as f32)).collect()
}

#[inline(always)]
fn points_equal(a: V2, b: V2) -> bool {
    (a - b).length_squared() <= EPSILON * EPSILON
}

/// Drops the closing duplicate, repeated points and back-and-forth spikes.
pub fn sanitize_ring(ring: Vec<V2>) -> Vec<V2> {
    if ring.is_empty() {
        return ring;
    }
    let mut points = ring;
    if points.len() > 1 && points_equal(points[points.len() - 1], points[0]) {
        points.pop();
    }
    let mut filtered: Vec<V2> = Vec::with_capacity(points.len());
    for p in points {
        if let Some(&last) = filtered.last()
            && points_equal(last, p)
        {
            continue;
        }
        if filtered.len() >= 2 && points_equal(filtered[filtered.len() - 2], p) {
            filtered.pop();
            continue;
        }
        filtered.push(p);
    }
    if filtered.len() > 1 && points_equal(filtered[filtered.len() - 1], filtered[0]) {
        filtered.pop();
    }
    filtered
}

fn is_convex(ring: &[V2]) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let mut expected = 0f32;
    for i in 0..ring.len() {
        let a = ring[i];
        let b = ring[(i + 1) % ring.len()];
        let c = ring[(i + 2) % ring.len()];
        let cross = (b - a).cross(c - b);
        if cross.abs() <= EPSILON {
            continue;
        }
        if expected == 0.0 {
            expected = if cross > 0.0 { 1.0 } else { -1.0 };
            continue;
        }
        if cross * expected < 0.0 {
            return false;
        }
    }
    expected != 0.0
}

pub fn signed_area(ring: &[V2]) -> f32 {
    if ring.len() < 3 {
        return 0.0;
    }
    let mut area = 0f32;
    for i in 0..ring.len() {
        let n = (i + 1) % ring.len();
        area += ring[i].x * ring[n].y - ring[n].x * ring[i].y;
    }
    area * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poly(exterior: &[(i32, i32)], interiors: &[&[(i32, i32)]]) -> Polygon {
        let ring = |r: &[(i32, i32)]| r.iter().map(|&(x, y)| Point { x, y }).collect();
        Polygon { exterior: ring(exterior), interiors: interiors.iter().map(|r| ring(r)).collect() }
    }

    #[test]
    fn a_square_inside_fans_counter_clockwise_in_render_space() {
        let mut p = ParsePolygon::new();
        let g = p.parse_geometry(&poly(&[(100, 100), (200, 100), (200, 200), (100, 200)], &[]), 4096.0).unwrap();
        assert_eq!(g.parsed.indices.len(), 6);
        assert_eq!(g.parsed.vertices[0], V2::new(100.0, 3996.0));
        assert_eq!(ParsedPolygon::first_clockwise_triangle(&g.parsed.vertices, &g.parsed.indices), None);
    }

    #[test]
    fn a_polygon_across_the_edge_is_clipped_and_a_hole_goes_through_earcut() {
        let mut p = ParsePolygon::new();
        let g = p
            .parse_geometry(
                &poly(&[(-100, 0), (1000, 0), (1000, 1000), (-100, 1000)], &[&[(200, 200), (200, 400), (400, 400), (400, 200)]]),
                4096.0,
            )
            .unwrap();
        assert!(g.clipped.exterior.iter().all(|v| v.x >= 0.0));
        assert_eq!(g.clipped.interiors.len(), 1);
        assert_eq!(ParsedPolygon::first_clockwise_triangle(&g.parsed.vertices, &g.parsed.indices), None);
        // The hole takes area away: the triangles cover 1000x1000 minus 200x200.
        let area: f64 = g
            .parsed
            .indices
            .chunks(3)
            .map(|t| crate::parsed_polygon::doubled_area(g.parsed.vertices[t[0] as usize], g.parsed.vertices[t[1] as usize], g.parsed.vertices[t[2] as usize]) / 2.0)
            .sum();
        assert!((area - (1_000_000.0 - 40_000.0)).abs() < 1.0, "{area}");
    }

    #[test]
    fn an_outline_winds_exterior_ccw_and_holes_cw() {
        let mut p = ParsePolygon::new();
        let o = p
            .parse_outline(&poly(&[(0, 0), (100, 0), (100, 100), (0, 100)], &[&[(10, 10), (10, 20), (20, 20), (20, 10)]]), 4096.0)
            .unwrap();
        assert_eq!(o.outline_starts, vec![0, 4]);
        assert!(signed_area(&o.vertices[0..4]) > 0.0);
        assert!(signed_area(&o.vertices[4..8]) < 0.0);
        assert!(o.indices.is_empty());
    }

    #[test]
    fn a_ring_outside_the_square_is_dropped() {
        assert!(clip(&poly(&[(5000, 5000), (6000, 5000), (6000, 6000)], &[]), ClipSquare::tile(4096.0)).is_none());
    }
}
