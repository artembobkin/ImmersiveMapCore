// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Turns one resolved building candidate into its mesh: a flat lid at the top
//! height and a wall per footprint edge, in render space with every normal
//! facing out of the building material. With an edge radius the edges are
//! rounded: one bevel face per edge, whose vertices take the normals of the
//! two faces it joins. A port of the Swift `BuildingExtrusionMeshBuilder`
//! and `BuildingRoundedExtrusion`.

use crate::geom::{V2, V3};
use crate::parse_polygon::ClipSquare;
use crate::parsed_polygon::ParsedPolygon;
use crate::tile::quarters;

/// One building's extruded vertex before unification.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParsedExtrudedVertex {
    pub position: V3,
    pub normal: V3,
    pub surface_id: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedExtrudedMesh {
    pub vertices: Vec<ParsedExtrudedVertex>,
    pub indices: Vec<u32>,
    /// The tile feature id of the building, zero for a feature without one.
    pub building_id: u64,
    /// The quarter of its tile the volume stands in, `None` for a tile kept whole.
    pub quarter: Option<usize>,
}

/// The smallest radius worth drawing, in tile units: under it the bevel is
/// lost to the vertex quantization.
pub const MINIMUM_EDGE_RADIUS: f32 = 0.5;

#[allow(clippy::too_many_arguments)]
pub fn build(
    clipped_exterior: &[V2],
    clipped_interiors: &[Vec<V2>],
    roof: &ParsedPolygon,
    base_height: f32,
    top_height: f32,
    edge_radius: f32,
    tile_extent: f32,
    quarter: Option<usize>,
) -> Option<ParsedExtrudedMesh> {
    if top_height <= base_height {
        return None;
    }
    let cut = cut_square(tile_extent, quarter);
    if edge_radius > 0.0
        && let Some(rounded) = build_rounded(clipped_exterior, clipped_interiors, base_height, top_height, edge_radius, cut)
    {
        return Some(rounded);
    }
    let mut vertices: Vec<ParsedExtrudedVertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut next_surface: u32 = 1;
    let exterior = sanitize_ring(clipped_exterior);
    if roof.indices.len() >= 3 {
        let surface = next_surface;
        next_surface = next_surface.wrapping_add(1);
        let offset = vertices.len() as u32;
        vertices.extend(roof.vertices.iter().map(|v| ParsedExtrudedVertex { position: V3::new(v.x, v.y, top_height), normal: V3::UP, surface_id: surface }));
        for t in roof.indices.as_chunks::<3>().0 {
            indices.extend_from_slice(&[t[0] + offset, t[2] + offset, t[1] + offset]);
        }
    }
    let mut append_walls = |ring: &[V2], clockwise: bool, vertices: &mut Vec<ParsedExtrudedVertex>, indices: &mut Vec<u32>| {
        if ring.len() < 2 {
            return;
        }
        let points = ensure_winding(ring.to_vec(), clockwise);
        for i in 0..points.len() {
            let p0 = points[i];
            let p1 = points[(i + 1) % points.len()];
            if p0 == p1 || is_boundary_edge(p0, p1, cut) {
                continue;
            }
            let v0 = V3::new(p0.x, p0.y, base_height);
            let v1 = V3::new(p1.x, p1.y, base_height);
            let v2 = V3::new(p1.x, p1.y, top_height);
            let v3 = V3::new(p0.x, p0.y, top_height);
            // Every wall normal faces out of the material: away from an
            // exterior's interior, into a hole's cavity.
            let normal = (v2 - v0).cross(v1 - v0).normalized();
            if normal.x.is_nan() || normal.y.is_nan() || normal.z.is_nan() {
                continue;
            }
            let surface = next_surface;
            next_surface = next_surface.wrapping_add(1);
            let start = vertices.len() as u32;
            for p in [v0, v1, v2, v3] {
                vertices.push(ParsedExtrudedVertex { position: p, normal, surface_id: surface });
            }
            indices.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
        }
    };
    // Exterior clockwise so walls are front-facing with back culling.
    append_walls(&exterior, true, &mut vertices, &mut indices);
    for interior in clipped_interiors {
        append_walls(&sanitize_ring(interior), false, &mut vertices, &mut indices);
    }
    if indices.is_empty() { None } else { Some(ParsedExtrudedMesh { vertices, indices, ..Default::default() }) }
}

/// The square the footprint was cut to, in render space: the tile's own, or
/// its quarter's.
pub fn cut_square(tile_extent: f32, quarter: Option<usize>) -> ClipSquare {
    match quarter {
        None => ClipSquare::tile(tile_extent),
        Some(q) => {
            let s = quarters::square(q);
            ClipSquare { minimum: V2::new(s.minimum.x, tile_extent - s.maximum.y), maximum: V2::new(s.maximum.x, tile_extent - s.minimum.y) }
        }
    }
}

/// Whether an edge runs along an edge of the square the footprint was cut to:
/// no wall stands there, the building goes on in the next tile or quarter.
pub fn is_boundary_edge(a: V2, b: V2, cut: ClipSquare) -> bool {
    let e = 0.001;
    let both_x = |v: f32| (a.x - v).abs() <= e && (b.x - v).abs() <= e;
    let both_y = |v: f32| (a.y - v).abs() <= e && (b.y - v).abs() <= e;
    both_x(cut.minimum.x) || both_x(cut.maximum.x) || both_y(cut.minimum.y) || both_y(cut.maximum.y)
}

pub fn ring_area(ring: &[V2]) -> f32 {
    if ring.len() < 3 {
        return 0.0;
    }
    let mut sum = 0f32;
    for i in 0..ring.len() {
        let j = (i + 1) % ring.len();
        sum += ring[i].x * ring[j].y - ring[j].x * ring[i].y;
    }
    sum * 0.5
}

/// The ring without its closing point, repeated points and spikes that fold
/// straight back (exact comparisons, as in Swift).
pub fn sanitize_ring(ring: &[V2]) -> Vec<V2> {
    let mut points: Vec<V2> = ring.to_vec();
    if points.len() > 1 && points[points.len() - 1] == points[0] {
        points.pop();
    }
    let mut out: Vec<V2> = Vec::with_capacity(points.len());
    for p in points {
        if out.last() == Some(&p) {
            continue;
        }
        if out.len() >= 2 && out[out.len() - 2] == p {
            out.pop();
            continue;
        }
        out.push(p);
    }
    if out.len() >= 2 && out[out.len() - 1] == out[0] {
        out.pop();
    }
    out
}

pub fn ensure_winding(mut ring: Vec<V2>, clockwise: bool) -> Vec<V2> {
    if (ring_area(&ring) < 0.0) != clockwise {
        ring.reverse();
    }
    ring
}

struct RoundedRing {
    points: Vec<V2>,
    directions: Vec<V2>,
    is_wall: Vec<bool>,
    draw_back: Vec<f32>,
    roof_point: Vec<V2>,
}

#[inline(always)]
fn left_normal(d: V2) -> V3 {
    V3::new(-d.y, d.x, 0.0)
}

fn rounded_ring(points: Vec<V2>, radius: f32, cut: ClipSquare) -> RoundedRing {
    let n = points.len();
    let is_wall: Vec<bool> = (0..n).map(|i| !is_boundary_edge(points[i], points[(i + 1) % n], cut)).collect();
    let lengths: Vec<f32> = (0..n).map(|i| (points[(i + 1) % n] - points[i]).length()).collect();
    let directions: Vec<V2> = (0..n).map(|i| if lengths[i] > 0.0 { (points[(i + 1) % n] - points[i]) / lengths[i] } else { V2::ZERO }).collect();
    let mut draw_back = vec![0f32; n];
    let mut roof_point = points.clone();
    for i in 0..n {
        let previous = (i + n - 1) % n;
        // A corner at the tile's edge stays sharp and its roof stays on the edge.
        if !is_wall[previous] || !is_wall[i] {
            continue;
        }
        let (incoming_length, outgoing_length) = (lengths[previous], lengths[i]);
        if incoming_length <= 0.0 || outgoing_length <= 0.0 {
            continue;
        }
        let incoming = directions[previous];
        let outgoing = directions[i];
        let shortest = incoming_length.min(outgoing_length);
        let incoming_normal = left_normal(incoming);
        let outgoing_normal = left_normal(outgoing);
        let bisector = incoming_normal + outgoing_normal;
        let bisector_length = bisector.length();
        // A corner that turns back on itself has no outward side.
        if bisector_length <= 1e-4 {
            continue;
        }
        let cosine = incoming.dot(outgoing).clamp(-1.0, 1.0);
        let tan_half = ((1.0 - cosine) / (1.0 + cosine).max(1e-4)).sqrt();
        let back = (radius * tan_half).min(shortest / 3.0);
        draw_back[i] = if back >= MINIMUM_EDGE_RADIUS * 0.5 { back } else { 0.0 };
        let miter = bisector * (1.0 / bisector_length);
        let cos_half = miter.dot(incoming_normal).max(0.25);
        let inset = (radius / cos_half).min(radius * 4.0).min(shortest / 2.0);
        roof_point[i] = points[i] - V2::new(miter.x, miter.y) * inset;
    }
    RoundedRing { points, directions, is_wall, draw_back, roof_point }
}

#[derive(Default)]
struct MeshBuilder {
    vertices: Vec<ParsedExtrudedVertex>,
    indices: Vec<u32>,
}

impl MeshBuilder {
    /// Wound as the sharp build's walls are: `cross(c - a, b - a)` faces out.
    fn triangle(&mut self, a: u32, b: u32, c: u32, outward: V3) {
        let (pa, pb, pc) = (self.vertices[a as usize].position, self.vertices[b as usize].position, self.vertices[c as usize].position);
        let facing = (pc - pa).cross(pb - pa).dot(outward);
        if facing == 0.0 {
            return;
        }
        if facing > 0.0 {
            self.indices.extend_from_slice(&[a, b, c]);
        } else {
            self.indices.extend_from_slice(&[a, c, b]);
        }
    }

    fn triangle_at(&mut self, positions: [V3; 3], normals: [V3; 3], outward: V3) {
        let start = self.vertices.len() as u32;
        for k in 0..3 {
            self.vertices.push(ParsedExtrudedVertex { position: positions[k], normal: normals[k], surface_id: 0 });
        }
        self.triangle(start, start + 1, start + 2, outward);
    }

    fn quad(&mut self, corners: [V3; 4], normals: [V3; 4], outward: V3) {
        let start = self.vertices.len() as u32;
        for k in 0..4 {
            self.vertices.push(ParsedExtrudedVertex { position: corners[k], normal: normals[k], surface_id: 0 });
        }
        self.triangle(start, start + 1, start + 2, outward);
        self.triangle(start, start + 2, start + 3, outward);
    }
}

/// The rounded mesh, `None` when the radius comes to nothing or the footprint
/// cannot be triangulated: the caller then builds the sharp box.
pub fn build_rounded(
    clipped_exterior: &[V2],
    clipped_interiors: &[Vec<V2>],
    base_height: f32,
    top_height: f32,
    edge_radius: f32,
    cut: ClipSquare,
) -> Option<ParsedExtrudedMesh> {
    let exterior = ensure_winding(sanitize_ring(clipped_exterior), true);
    if exterior.len() < 3 {
        return None;
    }
    let interiors: Vec<Vec<V2>> = clipped_interiors.iter().map(|r| ensure_winding(sanitize_ring(r), false)).filter(|r| r.len() >= 3).collect();
    let perimeter: f32 = (0..exterior.len()).map(|i| (exterior[(i + 1) % exterior.len()] - exterior[i]).length()).sum();
    let area = ring_area(&exterior).abs();
    if perimeter <= 0.0 {
        return None;
    }
    // Area over perimeter is half the narrow side of a long block.
    let radius = edge_radius.min((top_height - base_height) * 0.45).min(area / perimeter * 0.5);
    if radius < MINIMUM_EDGE_RADIUS {
        return None;
    }
    let mut coordinates: Vec<[f64; 2]> = exterior.iter().map(|p| [p.x as f64, p.y as f64]).collect();
    let mut holes: Vec<u32> = Vec::new();
    for interior in &interiors {
        holes.push(coordinates.len() as u32);
        coordinates.extend(interior.iter().map(|p| [p.x as f64, p.y as f64]));
    }
    let mut roof_indices: Vec<u32> = Vec::new();
    earcut::Earcut::<f64>::new().earcut_slice(&coordinates, &holes, &mut roof_indices);
    if roof_indices.len() < 3 {
        return None;
    }
    let mut all_rings = vec![exterior];
    all_rings.extend(interiors);
    let rings: Vec<RoundedRing> = all_rings.into_iter().map(|r| rounded_ring(r, radius, cut)).collect();
    let mut b = MeshBuilder::default();
    let up = V3::UP;
    let wall_top = top_height - radius;
    let bevel_bottom = base_height + radius;
    let roof_start = b.vertices.len() as u32;
    for ring in &rings {
        for p in &ring.roof_point {
            b.vertices.push(ParsedExtrudedVertex { position: V3::new(p.x, p.y, top_height), normal: up, surface_id: 0 });
        }
    }
    for t in roof_indices.as_chunks::<3>().0 {
        b.triangle(roof_start + t[0], roof_start + t[1], roof_start + t[2], up);
    }
    let at = |p: V2, z: f32| V3::new(p.x, p.y, z);
    for ring in &rings {
        let n = ring.points.len();
        for i in 0..n {
            let next = (i + 1) % n;
            let previous = (i + n - 1) % n;
            let point = ring.points[i];
            let back = ring.draw_back[i];
            if back > 0.0 {
                let incoming = ring.directions[previous];
                let outgoing = ring.directions[i];
                let incoming_normal = left_normal(incoming);
                let outgoing_normal = left_normal(outgoing);
                let from = point - incoming * back;
                let to = point + outgoing * back;
                let bevel_outward = (incoming_normal + outgoing_normal).normalized();
                b.quad(
                    [at(from, bevel_bottom), at(to, bevel_bottom), at(to, wall_top), at(from, wall_top)],
                    [incoming_normal, outgoing_normal, outgoing_normal, incoming_normal],
                    bevel_outward,
                );
                b.triangle_at(
                    [at(point, base_height), at(to, bevel_bottom), at(from, bevel_bottom)],
                    [bevel_outward, outgoing_normal, incoming_normal],
                    (bevel_outward + up).normalized(),
                );
                let roof_point = ring.roof_point[i];
                b.triangle_at([at(from, wall_top), at(to, wall_top), at(roof_point, top_height)], [incoming_normal, outgoing_normal, up], (bevel_outward + up).normalized());
            }
            if !ring.is_wall[i] {
                continue;
            }
            let direction = ring.directions[i];
            if direction == V2::ZERO {
                continue;
            }
            let normal = left_normal(direction);
            let start = point + direction * ring.draw_back[i];
            let end = ring.points[next] - direction * ring.draw_back[next];
            let footprint_end = ring.points[next];
            if ring.draw_back[i] > 0.0 || ring.draw_back[next] > 0.0 {
                b.quad([at(point, base_height), at(footprint_end, base_height), at(end, bevel_bottom), at(start, bevel_bottom)], [normal; 4], normal);
                b.quad([at(start, bevel_bottom), at(end, bevel_bottom), at(end, wall_top), at(start, wall_top)], [normal; 4], normal);
            } else {
                b.quad([at(start, base_height), at(end, base_height), at(end, wall_top), at(start, wall_top)], [normal; 4], normal);
            }
            let (rs, re) = (ring.roof_point[i], ring.roof_point[next]);
            b.quad([at(start, wall_top), at(end, wall_top), at(re, top_height), at(rs, top_height)], [normal, normal, up, up], (normal + up).normalized());
        }
    }
    if b.indices.is_empty() { None } else { Some(ParsedExtrudedMesh { vertices: b.vertices, indices: b.indices, ..Default::default() }) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(min: f32, max: f32) -> (Vec<V2>, ParsedPolygon) {
        let ring = vec![V2::new(min, min), V2::new(max, min), V2::new(max, max), V2::new(min, max)];
        let roof = ParsedPolygon::counter_clockwise_convex_fan(ring.clone());
        (ring, roof)
    }

    #[test]
    fn a_box_has_a_lid_and_four_outward_walls() {
        let (ring, roof) = square(100.0, 200.0);
        let mesh = build(&ring, &[], &roof, 0.0, 50.0, 0.0, 4096.0, None).unwrap();
        assert_eq!(mesh.indices.len(), 6 + 4 * 6);
        let centre = V3::new(150.0, 150.0, 25.0);
        for v in mesh.vertices.iter().filter(|v| v.normal.z == 0.0) {
            assert!((v.position - centre).dot(v.normal) > 0.0, "walls face out");
        }
    }

    #[test]
    fn edges_on_the_tile_border_stand_no_wall() {
        let (ring, roof) = square(0.0, 200.0);
        let mesh = build(&ring, &[], &roof, 0.0, 50.0, 0.0, 4096.0, None).unwrap();
        assert_eq!(mesh.indices.len(), 6 + 2 * 6);
    }

    #[test]
    fn a_rounded_box_keeps_its_triangles_outward() {
        let (ring, roof) = square(100.0, 300.0);
        let mesh = build(&ring, &[], &roof, 0.0, 120.0, 10.0, 4096.0, None).unwrap();
        assert!(mesh.indices.len() > 6 + 4 * 6);
        let centre = V3::new(200.0, 200.0, 60.0);
        for t in mesh.indices.chunks(3) {
            let (a, b, c) = (mesh.vertices[t[0] as usize].position, mesh.vertices[t[1] as usize].position, mesh.vertices[t[2] as usize].position);
            let face = (c - a).cross(b - a);
            let mid = (a + b + c) * (1.0 / 3.0);
            assert!(face.dot(mid - centre) >= -1e-3, "every triangle faces out of the material");
        }
    }
}
