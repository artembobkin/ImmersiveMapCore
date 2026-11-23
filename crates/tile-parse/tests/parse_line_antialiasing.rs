// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Ported from the Swift `ParseLineAnalyticAntialiasingTests`: line geometry
//! is extruded past the styled half-width by the feather, every vertex
//! carries a normalized signed centreline distance and a longitudinal
//! parameter. (The vertex layout and edge-threshold parts are tested with
//! the unification stage.)

use tile_parse::geom::V2;
use tile_parse::parse_line::{LineOptions, ParseLine};
use tile_parse::parsed_polygon::ParsedPolygon;

const EXTENT: f32 = 4096.0;

#[derive(Default, Clone, Copy)]
struct F {
    start_cap: bool,
    end_cap: bool,
    join: bool,
    feather_start: bool,
    feather_end: bool,
    arc: bool,
    no_clip: bool,
}

fn parse(points: &[(f32, f32)], width: f64, f: F) -> Option<ParsedPolygon> {
    let mut o = LineOptions::new(width, EXTENT);
    o.start_cap_round = f.start_cap;
    o.end_cap_round = f.end_cap;
    o.line_join_round = f.join;
    o.feather_start = f.feather_start;
    o.feather_end = f.feather_end;
    o.emits_arc_length = f.arc;
    o.clip_geometry_to_tile_bounds = !f.no_clip;
    let pts: Vec<V2> = points.iter().map(|&(x, y)| V2::new(x, y)).collect();
    ParseLine::new().parse(&pts, &o)
}

#[test]
fn straight_segment_is_extruded_by_the_feather_and_carries_the_rim_distances() {
    let width = 10.0;
    let p = parse(&[(100.0, 100.0), (200.0, 100.0)], width, F::default()).unwrap();
    assert_eq!(p.vertices.len(), 4);
    assert_eq!(p.line_distances.len(), 4);
    assert_eq!(p.line_parameters.len(), 4);
    let extruded = ParseLine::extruded_half_width(width as f32 * 0.5);
    let ys: Vec<f32> = p.vertices.iter().map(|v| v.y).collect();
    let max = ys.iter().cloned().fold(f32::MIN, f32::max);
    let min = ys.iter().cloned().fold(f32::MAX, f32::min);
    assert!((max - (EXTENT - 100.0 + extruded)).abs() <= 0.501);
    assert!((min - (EXTENT - 100.0 - extruded)).abs() <= 0.501);
    assert!(p.line_distances.iter().all(|&d| (d as i32).abs() == 127));
    assert!(p.line_parameters.iter().all(|&t| t == i16::MAX));
}

#[test]
fn a_hairline_is_extruded_to_the_minimum() {
    let p = parse(&[(100.0, 100.0), (200.0, 100.0)], 1.0, F::default()).unwrap();
    let max = p.vertices.iter().map(|v| v.y).fold(f32::MIN, f32::max);
    assert!((max - (EXTENT - 100.0 + ParseLine::MINIMUM_EXTRUDED_HALF_WIDTH)).abs() <= 0.501);
    assert_eq!(ParseLine::extruded_half_width(0.5), ParseLine::MINIMUM_EXTRUDED_HALF_WIDTH);
    assert_eq!(ParseLine::extruded_half_width(20.0), 20.0 + ParseLine::FEATHER_TILE_UNITS);
}

#[test]
fn a_deferred_ribbon_carries_the_centreline_and_the_directions() {
    let mut o = LineOptions::new(10.0, EXTENT);
    o.start_cap_round = true;
    o.line_join_round = true;
    o.clip_geometry_to_tile_bounds = false;
    o.deferred_extrusion = true;
    let p = ParseLine::new().parse(&[V2::new(100.0, 100.0), V2::new(200.0, 100.0), V2::new(200.0, 300.0)], &o).unwrap();
    assert_eq!(p.line_normals.len(), p.vertices.len());
    assert!(p.is_line_ribbon());
    assert_eq!(p.vertices[0], V2::new(100.0, EXTENT - 100.0));
    assert_eq!(p.vertices[1], V2::new(100.0, EXTENT - 100.0));
    assert_eq!(p.line_normals[0], [0, 127]);
    assert_eq!(p.line_normals[1], [0, -127]);
    assert_eq!(p.line_distances[0], 127);
    assert_eq!(p.line_distances[1], -127);
    for (n, &d) in p.line_normals.iter().zip(&p.line_distances) {
        let length = (n[0] as f32).hypot(n[1] as f32) / 127.0;
        if d == 0 {
            assert_eq!(*n, [0, 0], "a hub carries no direction");
        } else {
            assert!((length - 1.0).abs() <= 0.02);
        }
    }
    assert!(p.line_distances.contains(&0));
    assert!(p.vertices.iter().all(|v| v.x >= 100.0 && v.x <= 200.0));
    let baked = parse(&[(100.0, 100.0), (200.0, 100.0)], 10.0, F::default()).unwrap();
    assert!(baked.line_normals.is_empty());
}

#[test]
fn free_butt_ends_get_a_longitudinal_ramp() {
    let p = parse(&[(100.0, 100.0), (200.0, 100.0)], 6.0, F { feather_start: true, feather_end: true, ..F::default() }).unwrap();
    assert_eq!(p.vertices.len(), 8);
    let f = ParseLine::FEATHER_TILE_UNITS;
    let min = p.vertices.iter().map(|v| v.x).fold(f32::MAX, f32::min);
    let max = p.vertices.iter().map(|v| v.x).fold(f32::MIN, f32::max);
    assert!((min - (100.0 - f)).abs() <= 0.501);
    assert!((max - (200.0 + f)).abs() <= 0.501);
    assert_eq!(p.line_parameters.iter().filter(|&&t| t == -i16::MAX).count(), 4);
    assert_eq!(p.line_parameters.iter().filter(|&&t| t == i16::MAX).count(), 4);
    assert!(p.line_distances.iter().all(|&d| (d as i32).abs() == 127));
}

#[test]
fn arc_length_mode_accumulates_along_the_polyline_in_half_units() {
    let p = parse(
        &[(100.0, 100.0), (200.0, 100.0), (200.0, 250.0)],
        6.0,
        F { feather_start: true, feather_end: true, arc: true, ..F::default() },
    )
    .unwrap();
    assert_eq!(p.vertices.len(), 8);
    assert_eq!(p.line_parameters, vec![0, 0, 200, 200, 200, 200, 500, 500]);
}

#[test]
fn round_join_fan_follows_the_rim_arc_at_the_join_arc_length() {
    let width = 6.0;
    let p = parse(&[(100.0, 100.0), (200.0, 100.0), (200.0, 250.0)], width, F { join: true, arc: true, ..F::default() }).unwrap();
    assert!(p.vertices.len() > 8);
    let fan = &p.vertices[8..];
    assert!(p.line_parameters[8..].iter().all(|&t| t == 200));
    assert_eq!(p.line_distances[8], 0);
    assert!(p.line_distances[9..].iter().all(|&d| (d as i32).abs() == 127));
    let center = V2::new(200.0, 3996.0);
    let radius = ParseLine::extruded_half_width(width as f32 * 0.5);
    assert_eq!(fan[0], center);
    let rim = &fan[1..];
    assert_eq!(rim[0], V2::new(200.0, 3996.0 + radius));
    assert_eq!(rim[rim.len() - 1], V2::new(200.0 + radius, 3996.0));
    for &point in rim {
        assert!(((point - center).length() - radius).abs() <= 0.75);
    }
    assert!(rim.len() >= 6);
}

#[test]
fn arc_survives_clipping_by_interpolation() {
    let p = parse(&[(-50.0, 100.0), (150.0, 100.0)], 10.0, F { arc: true, ..F::default() }).unwrap();
    assert!(p.vertices.iter().all(|v| v.x >= 0.0 && v.x <= EXTENT));
    let min = *p.line_parameters.iter().min().unwrap() as i32;
    let max = *p.line_parameters.iter().max().unwrap() as i32;
    assert!((min - 100).abs() <= 3, "{min}");
    assert!((max - 400).abs() <= 3, "{max}");
}

#[test]
fn round_cap_wins_over_the_end_feather() {
    let p = parse(
        &[(500.0, 500.0), (600.0, 500.0)],
        8.0,
        F { start_cap: true, end_cap: true, feather_start: true, feather_end: true, ..F::default() },
    )
    .unwrap();
    assert!(p.line_parameters.iter().all(|&t| t == i16::MAX));
    assert_eq!(p.line_distances.iter().filter(|&&d| d == 0).count(), 2);
    assert!(p.line_distances.iter().all(|&d| d == 0 || (d as i32).abs() == 127));
}

#[test]
fn diagonal_clip_interpolates_intermediate_distances() {
    let p = parse(&[(-30.0, 60.0), (120.0, -40.0)], 12.0, F::default()).unwrap();
    assert!(p.line_distances.iter().any(|&d| (d as i32).abs() > 0 && (d as i32).abs() < 127));
}

#[test]
fn every_triangle_is_counter_clockwise_in_render_space() {
    let check = |p: Option<ParsedPolygon>, name: &str| {
        let p = p.unwrap_or_else(|| panic!("{name}"));
        assert_eq!(ParsedPolygon::first_clockwise_triangle(&p.vertices, &p.indices), None, "{name}");
        let doubled: i64 = p
            .indices
            .chunks(3)
            .map(|t| {
                let (a, b, c) = (p.vertices[t[0] as usize], p.vertices[t[1] as usize], p.vertices[t[2] as usize]);
                (b.x as i64 - a.x as i64) * (c.y as i64 - a.y as i64) - (b.y as i64 - a.y as i64) * (c.x as i64 - a.x as i64)
            })
            .sum();
        assert!(doubled > 0, "{name}: the ribbon has area");
    };
    let w = 10.0;
    check(parse(&[(100.0, 100.0), (200.0, 100.0)], w, F::default()), "straight");
    check(parse(&[(100.0, 100.0), (200.0, 100.0)], w, F { feather_start: true, feather_end: true, ..F::default() }), "feathered");
    check(parse(&[(100.0, 100.0), (200.0, 100.0), (200.0, 250.0)], w, F { arc: true, ..F::default() }), "arc");
    check(parse(&[(100.0, 100.0), (200.0, 100.0), (200.0, 250.0)], w, F { join: true, ..F::default() }), "right turn");
    check(parse(&[(100.0, 300.0), (200.0, 300.0), (200.0, 200.0)], w, F { join: true, ..F::default() }), "left turn");
    check(parse(&[(100.0, 100.0), (200.0, 100.0)], w, F { start_cap: true, end_cap: true, ..F::default() }), "caps");
    check(parse(&[(-50.0, 100.0), (150.0, 100.0)], w, F::default()), "crossing the edge");
    check(parse(&[(100.0, 100.0), (200.0, 100.0), (200.0, 250.0)], w, F { join: true, no_clip: true, ..F::default() }), "unclipped bend");
}
