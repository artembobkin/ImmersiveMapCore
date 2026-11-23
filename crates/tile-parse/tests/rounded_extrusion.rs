// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Ported from the Swift `BuildingRoundedExtrusionTests`.

use std::collections::{HashMap, HashSet};

use tile_parse::buildings::mesh_builder::{ParsedExtrudedMesh, build};
use tile_parse::geom::{V2, V3};
use tile_parse::parsed_polygon::ParsedPolygon;

fn v(x: f32, y: f32) -> V2 {
    V2::new(x, y)
}

fn square() -> Vec<V2> {
    vec![v(1000.0, 1000.0), v(1200.0, 1000.0), v(1200.0, 1200.0), v(1000.0, 1200.0)]
}

fn ell() -> Vec<V2> {
    vec![v(1000.0, 1000.0), v(1300.0, 1000.0), v(1300.0, 1100.0), v(1100.0, 1100.0), v(1100.0, 1300.0), v(1000.0, 1300.0)]
}

fn mesh(exterior: &[V2], top: f32, radius: f32, quarter: Option<usize>) -> ParsedExtrudedMesh {
    let roof = ParsedPolygon::new(exterior.to_vec(), vec![0, 1, 2, 0, 2, 3]);
    build(exterior, &[], &roof, 0.0, top, radius, 4096.0, quarter).expect("a mesh")
}

type PointKey = (u32, u32, u32);

fn key(p: V3) -> PointKey {
    (p.x.to_bits(), p.y.to_bits(), p.z.to_bits())
}

#[test]
fn no_radius_is_the_sharp_box() {
    assert_eq!(mesh(&square(), 80.0, 0.0, None).vertices.len(), 4 + 16);
    let tiny = vec![v(1000.0, 1000.0), v(1002.0, 1000.0), v(1002.0, 1002.0), v(1000.0, 1002.0)];
    assert_eq!(mesh(&tiny, 80.0, 10.0, None).vertices.len(), 4 + 16);
}

#[test]
fn every_face_of_a_rounded_block_faces_out() {
    let rounded = mesh(&square(), 80.0, 10.0, None);
    assert!(rounded.vertices.len() > 20);
    let middle = V3::new(1100.0, 1100.0, 40.0);
    for t in rounded.indices.chunks(3) {
        let (a, b, c) = (rounded.vertices[t[0] as usize].position, rounded.vertices[t[1] as usize].position, rounded.vertices[t[2] as usize].position);
        let centroid = (a + b + c) * (1.0 / 3.0);
        assert!((c - a).cross(b - a).dot(centroid - middle) > 0.0);
    }
}

#[test]
fn a_rounded_building_is_closed_but_for_its_floor() {
    for ring in [square(), ell()] {
        let rounded = mesh(&ring, 80.0, 10.0, None);
        let mut uses: HashMap<(PointKey, PointKey), (usize, bool)> = HashMap::new();
        for t in rounded.indices.chunks(3) {
            let corners: Vec<V3> = t.iter().map(|&i| rounded.vertices[i as usize].position).collect();
            for side in 0..3 {
                let (a, b) = (corners[side], corners[(side + 1) % 3]);
                let (ka, kb) = (key(a), key(b));
                let edge = if ka < kb { (ka, kb) } else { (kb, ka) };
                let entry = uses.entry(edge).or_insert((0, a.z == 0.0 && b.z == 0.0));
                entry.0 += 1;
            }
        }
        for (_, (count, ground)) in uses {
            assert_eq!(count, if ground { 1 } else { 2 });
        }
    }
}

#[test]
fn the_roof_is_drawn_in_and_the_walls_stop_a_radius_under_it() {
    let rounded = mesh(&square(), 80.0, 10.0, None);
    let roof: Vec<_> = rounded.vertices.iter().filter(|v| v.position.z == 80.0 && v.normal.z > 0.99).collect();
    assert!(!roof.is_empty());
    for vertex in roof {
        assert!((1009.0..=1191.0).contains(&vertex.position.x) && (1009.0..=1191.0).contains(&vertex.position.y));
    }
    let wall_top = rounded.vertices.iter().filter(|v| v.normal.z.abs() < 0.01).map(|v| v.position.z).fold(f32::MIN, f32::max);
    assert!((wall_top - 70.0).abs() < 1e-3);
}

#[test]
fn the_building_stands_on_its_sharp_footprint() {
    let rounded = mesh(&square(), 80.0, 10.0, None);
    let on_ground: HashSet<(u32, u32)> = rounded.vertices.iter().filter(|v| v.position.z == 0.0).map(|v| (v.position.x.to_bits(), v.position.y.to_bits())).collect();
    let footprint: HashSet<(u32, u32)> = square().iter().map(|p| (p.x.to_bits(), p.y.to_bits())).collect();
    assert_eq!(on_ground, footprint);
    let lowest = rounded.vertices.iter().map(|v| v.position.z).filter(|&z| z > 0.0).fold(f32::MAX, f32::min);
    assert!((lowest - 10.0).abs() < 1e-3);
}

#[test]
fn the_radius_is_held_to_the_building() {
    let low = mesh(&square(), 8.0, 10.0, None);
    let low_top = low.vertices.iter().filter(|v| v.normal.z.abs() < 0.01).map(|v| v.position.z).fold(f32::MIN, f32::max);
    assert!((low_top - (8.0 - 8.0 * 0.45)).abs() < 1e-3);
    let narrow = vec![v(1000.0, 1000.0), v(1400.0, 1000.0), v(1400.0, 1010.0), v(1000.0, 1010.0)];
    let thin = mesh(&narrow, 80.0, 10.0, None);
    let thin_top = thin.vertices.iter().filter(|v| v.normal.z.abs() < 0.01).map(|v| v.position.z).fold(f32::MIN, f32::max);
    assert!(thin_top > 80.0 - 2.6);
}

#[test]
fn the_tile_edge_stays_sharp() {
    let clipped = vec![v(0.0, 1000.0), v(200.0, 1000.0), v(200.0, 1200.0), v(0.0, 1200.0)];
    let rounded = mesh(&clipped, 80.0, 10.0, None);
    let rim: HashSet<(u32, u32, u32)> = rounded.vertices.iter().filter(|v| v.position.z == 80.0 && v.normal.z > 0.99 && v.position.x == 0.0).map(|v| key(v.position)).collect();
    assert_eq!(rim, HashSet::from([key(V3::new(0.0, 1000.0, 80.0)), key(V3::new(0.0, 1200.0, 80.0))]));
    assert!(!rounded.vertices.iter().any(|v| (v.normal.x + 1.0).abs() < 0.01));
}

#[test]
fn the_quarter_edge_stays_sharp() {
    let clipped = vec![v(1800.0, 3000.0), v(2048.0, 3000.0), v(2048.0, 3200.0), v(1800.0, 3200.0)];
    let face_east = |m: &ParsedExtrudedMesh| m.vertices.iter().any(|v| (v.normal.x - 1.0).abs() < 0.01);
    assert!(face_east(&mesh(&clipped, 80.0, 10.0, None)));
    let cut = mesh(&clipped, 80.0, 10.0, Some(0));
    assert!(!face_east(&cut));
    let rim: HashSet<(u32, u32, u32)> = cut.vertices.iter().filter(|v| v.position.z == 80.0 && v.normal.z > 0.99 && v.position.x == 2048.0).map(|v| key(v.position)).collect();
    assert_eq!(rim, HashSet::from([key(V3::new(2048.0, 3000.0, 80.0)), key(V3::new(2048.0, 3200.0, 80.0))]));
}

#[test]
fn a_corner_that_turns_back_stays_sharp() {
    let spiked = vec![v(1000.0, 1000.0), v(1300.0, 1000.0), v(1300.0, 1200.0), v(1150.0, 1200.0), v(1150.0, 1260.0), v(1150.0, 1230.0), v(1000.0, 1200.0)];
    let rounded = mesh(&spiked, 80.0, 10.0, None);
    assert!(!rounded.vertices.iter().any(|v| v.normal.x.is_nan() || v.normal.y.is_nan() || v.normal.z.is_nan()));
}
