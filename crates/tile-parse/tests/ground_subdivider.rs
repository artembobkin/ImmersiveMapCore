// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Ported from the Swift `GroundGeometrySubdividerTests`: pieces stay within
//! one cell, area and attributes survive, and shared edges split identically.

use tile_parse::geom::V2;
use tile_parse::ground_subdivider::{ribbon_step, step_for_tile_zoom, subdivide, subdivide_if_needed};
use tile_parse::parsed_polygon::{ParsedPolygon, doubled_area};

fn v(x: f32, y: f32) -> V2 {
    V2::new(x, y)
}

fn signed_area(p: &ParsedPolygon) -> f64 {
    p.indices
        .chunks(3)
        .map(|t| doubled_area(p.vertices[t[0] as usize], p.vertices[t[1] as usize], p.vertices[t[2] as usize]) / 2.0)
        .sum()
}

fn assert_edges_within(p: &ParsedPolygon, bound: f32) {
    for t in p.indices.chunks(3) {
        for e in 0..3 {
            let a = p.vertices[t[e] as usize];
            let b = p.vertices[t[(e + 1) % 3] as usize];
            assert!((a - b).length() <= bound, "every edge must fit inside one grid cell");
        }
    }
}

#[test]
fn step_follows_the_tile_zoom() {
    assert_eq!(step_for_tile_zoom(0), Some(64));
    assert_eq!(step_for_tile_zoom(1), Some(64));
    assert_eq!(step_for_tile_zoom(2), Some(128));
    assert_eq!(step_for_tile_zoom(3), Some(128));
    assert_eq!(step_for_tile_zoom(4), Some(256));
    assert_eq!(step_for_tile_zoom(6), Some(512));
    assert_eq!(step_for_tile_zoom(7), Some(512));
    assert_eq!(step_for_tile_zoom(8), None, "from z8 the surface has unfurled");
    assert_eq!(step_for_tile_zoom(10), None);
}

#[test]
fn ribbons_split_on_a_quadruple_step_grid() {
    assert_eq!(ribbon_step(64), 256);
    assert_eq!(ribbon_step(1024), 4096);
    let fill = ParsedPolygon::new(vec![v(0.0, 0.0), v(4096.0, 0.0), v(0.0, 4096.0)], vec![0, 1, 2]);
    let mut ribbon = fill.clone();
    ribbon.line_distances = vec![127, 127, -127];
    ribbon.line_parameters = vec![0, i16::MAX, i16::MAX];
    let mut polygons = [fill, ribbon];
    subdivide_if_needed(polygons.iter_mut(), 0);
    assert!(polygons[1].vertices.len() < polygons[0].vertices.len());
    assert_edges_within(&polygons[0], 64.0 * 2f32.sqrt() + 1.0);
    assert_edges_within(&polygons[1], 256.0 * 2f32.sqrt() + 1.0);
}

#[test]
fn tile_spanning_triangle_is_cut_into_cells() {
    let polygon = ParsedPolygon::new(vec![v(0.0, 0.0), v(4096.0, 0.0), v(0.0, 4096.0)], vec![0, 1, 2]);
    let split = subdivide(&polygon, 1024);
    assert!(split.indices.len() > polygon.indices.len());
    assert_eq!(split.indices.len() % 3, 0);
    assert_edges_within(&split, 1024.0 * 2f32.sqrt() + 1.0);
    assert!((signed_area(&split) - signed_area(&polygon)).abs() <= 1.0);
}

#[test]
fn tile_quad_subdivides_into_exactly_the_grid_cells() {
    let quad = ParsedPolygon::new(vec![v(0.0, 0.0), v(4096.0, 0.0), v(4096.0, 4096.0), v(0.0, 4096.0)], vec![0, 1, 2, 0, 2, 3]);
    for step in [64, 128, 256, 512, 1024, 2048] {
        let cells = (4096 / step) as usize;
        let split = subdivide(&quad, step);
        assert_eq!(split.indices.len(), cells * cells * 6, "step {step}: two triangles per cell");
        assert_eq!(split.vertices.len(), (cells + 1) * (cells + 1), "step {step}: one vertex per corner");
        assert!((signed_area(&split) - signed_area(&quad)).abs() <= 1.0);
        assert_eq!(ParsedPolygon::first_clockwise_triangle(&split.vertices, &split.indices), None);
    }
}

#[test]
fn triangle_inside_one_cell_passes_through_untouched() {
    let polygon = ParsedPolygon::new(vec![v(10.0, 10.0), v(60.0, 20.0), v(30.0, 50.0)], vec![0, 1, 2]);
    let split = subdivide(&polygon, 64);
    assert_eq!(split.vertices, polygon.vertices);
    assert_eq!(split.indices, polygon.indices);
}

#[test]
fn attributes_interpolate_linearly_across_a_split() {
    let polygon = ParsedPolygon {
        vertices: vec![v(0.0, 0.0), v(128.0, 0.0), v(128.0, 10.0), v(0.0, 10.0)],
        indices: vec![0, 1, 2, 0, 2, 3],
        line_distances: vec![-127, -127, 127, 127],
        line_parameters: vec![0, 1000, 1000, 0],
        ..Default::default()
    };
    let split = subdivide(&polygon, 64);
    assert_eq!(split.line_distances.len(), split.vertices.len());
    let on_line: Vec<usize> = (0..split.vertices.len()).filter(|&i| split.vertices[i].x == 64.0).collect();
    assert!(!on_line.is_empty());
    for i in on_line {
        assert_eq!(split.line_parameters[i], 500);
        match split.vertices[i].y {
            0.0 => assert_eq!(split.line_distances[i], -127),
            10.0 => assert_eq!(split.line_distances[i], 127),
            5.0 => assert_eq!(split.line_distances[i], 0),
            y => panic!("unexpected crossing at y = {y}"),
        }
    }
}

#[test]
fn shared_edges_split_identically() {
    let polygon = ParsedPolygon::new(vec![v(0.0, 0.0), v(128.0, 0.0), v(128.0, 128.0), v(0.0, 128.0)], vec![0, 1, 2, 2, 3, 0]);
    let split = subdivide(&polygon, 64);
    assert_eq!(split.vertices.iter().filter(|p| p.x == 64.0 && p.y == 64.0).count(), 1);
    assert!((signed_area(&split) - signed_area(&polygon)).abs() <= 1.0);
}

#[test]
fn winding_is_preserved() {
    let ccw = ParsedPolygon::new(vec![v(0.0, 0.0), v(200.0, 0.0), v(0.0, 200.0)], vec![0, 1, 2]);
    let split = subdivide(&ccw, 64);
    for t in split.indices.chunks(3) {
        assert!(doubled_area(split.vertices[t[0] as usize], split.vertices[t[1] as usize], split.vertices[t[2] as usize]) > 0.0);
    }
}
