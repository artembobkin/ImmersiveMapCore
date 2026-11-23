// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Ported from the Swift `ParseLineClipWindingTests` and
//! `ParsePolygonClippedSpaceTests`.

use std::collections::HashSet;

use mvt::geometry::{Point, Polygon};
use tile_parse::geom::V2;
use tile_parse::parse_line::{LineOptions, ParseLine};
use tile_parse::parse_polygon::ParsePolygon;
use tile_parse::parsed_polygon::ParsedPolygon;

fn pt(x: i32, y: i32) -> Point {
    Point { x, y }
}

fn polygon(exterior: Vec<Point>, interiors: Vec<Vec<Point>>) -> Polygon {
    Polygon { exterior, interiors }
}

#[test]
fn a_clipped_ribbon_with_gentle_joins_keeps_its_winding() {
    for step in 1..=40 {
        let bend = step as f32 * 0.0004;
        let mut points = Vec::new();
        let mut position = V2::new(1900.0, 263.0);
        let mut heading = 0.2f32;
        for _ in 0..40 {
            points.push(position);
            heading += bend;
            position += V2::new(heading.cos(), heading.sin()) * 61.7;
        }
        points.push(V2::new(4300.0, 263.0));
        let mut o = LineOptions::new(21.92, 4096.0);
        o.start_cap_round = true;
        o.end_cap_round = true;
        o.line_join_round = true;
        let ribbon = ParseLine::new().parse(&points, &o).expect("a ribbon");
        assert!(ribbon.line_normals.is_empty(), "the clipped, pre-extruded path");
        assert_eq!(ParsedPolygon::first_clockwise_triangle(&ribbon.vertices, &ribbon.indices), None, "bend {bend}");
    }
}

#[test]
fn clipped_rings_stay_in_tile_space_and_vertices_flip_once() {
    let p = polygon(vec![pt(600, -200), pt(1400, -200), pt(1400, 900), pt(600, 900)], vec![]);
    let parsed = ParsePolygon::new().parse_geometry(&p, 4096.0).unwrap();
    let ys: Vec<f32> = parsed.clipped.exterior.iter().map(|v| v.y).collect();
    assert!((ys.iter().cloned().fold(f32::MAX, f32::min) - 0.0).abs() < 0.001, "cut at the north edge");
    assert!((ys.iter().cloned().fold(f32::MIN, f32::max) - 900.0).abs() < 0.001);
    let render_ys: HashSet<i32> = parsed.parsed.vertices.iter().map(|v| v.y as i32).collect();
    assert_eq!(render_ys, HashSet::from([4096, 4096 - 900]));
}

#[test]
fn winding_is_settled_on_the_rounded_vertices() {
    let points: [(i16, i16); 25] = [
        (2007, 71), (2020, 51), (2026, 48), (2033, 49), (2040, 54), (2080, 55), (2091, 62), (2091, 63), (2086, 62),
        (2080, 57), (2020, 57), (2010, 71), (2066, 60), (2067, 61), (2051, 50), (2051, 53), (2048, 51), (2048, 53),
        (2048, 54), (2039, 56), (2032, 51), (2024, 54), (2031, 52), (2031, 54), (2028, 50),
    ];
    let indices: Vec<u32> = vec![
        0, 1, 2, 2, 3, 4, 5, 6, 7, 7, 8, 9, 10, 11, 0, 5, 7, 9, 12, 5, 9, 12, 9, 13, 14, 12, 13, 14, 13, 15, 4, 16, 17, 16,
        14, 15, 16, 15, 17, 4, 17, 18, 4, 18, 19, 17, 15, 18, 4, 19, 20, 2, 4, 20, 21, 22, 0, 0, 21, 2, 2, 20, 23, 10, 0, 2,
        2, 23, 21, 21, 10, 2,
    ];
    let mut p = ParsedPolygon::new(points.iter().map(|&(x, y)| V2::new(x as f32, y as f32)).collect(), indices.clone());
    assert_eq!(ParsedPolygon::first_clockwise_triangle(&p.vertices, &p.indices), Some(19));
    p.wind_counter_clockwise();
    assert_eq!(ParsedPolygon::first_clockwise_triangle(&p.vertices, &p.indices), None);
    for t in 0..indices.len() / 3 {
        let before: HashSet<u32> = indices[t * 3..t * 3 + 3].iter().copied().collect();
        let after: HashSet<u32> = p.indices[t * 3..t * 3 + 3].iter().copied().collect();
        assert_eq!(before, after, "triangle {t} keeps its corners");
    }
}

fn orientation(ring: Vec<Point>) -> f32 {
    let parsed = ParsePolygon::new().parse(&polygon(ring, vec![]), 4096.0).unwrap();
    parsed
        .indices
        .chunks(3)
        .map(|t| {
            let (a, b, c) = (parsed.vertices[t[0] as usize], parsed.vertices[t[1] as usize], parsed.vertices[t[2] as usize]);
            a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y)
        })
        .sum()
}

fn first_clockwise(exterior: Vec<Point>, interiors: Vec<Vec<Point>>) -> Option<usize> {
    let parsed = ParsePolygon::new().parse(&polygon(exterior, interiors), 4096.0).unwrap();
    ParsedPolygon::first_clockwise_triangle(&parsed.vertices, &parsed.indices)
}

fn rev(mut r: Vec<Point>) -> Vec<Point> {
    r.reverse();
    r
}

#[test]
fn triangle_orientation_survives_the_flip() {
    let ring = vec![pt(600, 300), pt(1400, 300), pt(1400, 900), pt(600, 900)];
    assert!(orientation(ring.clone()) > 0.0);
    assert!(orientation(rev(ring.clone())) > 0.0);
    let concave = vec![pt(600, 300), pt(1400, 300), pt(1400, 900), pt(1000, 500), pt(600, 900)];
    assert!(orientation(concave.clone()) > 0.0);
    assert!(orientation(rev(concave)) > 0.0);
    let hole = vec![pt(900, 500), pt(1100, 500), pt(1100, 700), pt(900, 700)];
    assert_eq!(first_clockwise(ring.clone(), vec![hole.clone()]), None);
    assert_eq!(first_clockwise(rev(ring), vec![rev(hole)]), None);
    let past_north = vec![pt(600, -200), pt(1400, -200), pt(1400, 900), pt(1000, 500), pt(600, 900)];
    assert_eq!(first_clockwise(past_north.clone(), vec![]), None);
    assert_eq!(first_clockwise(rev(past_north), vec![]), None);
}
