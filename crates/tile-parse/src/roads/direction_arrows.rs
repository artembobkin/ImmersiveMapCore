// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The oneway arrows stamped along a road, a port of the Swift
//! `RoadDirectionArrowGeometryBuilder`. Each arrow is a tail quad and a head
//! triangle, counter-clockwise in render space like every tile triangle.

use map_style::style::OnewayArrowDecoration;

use crate::coordinate_space::render_points;
use crate::geom::V2;
use crate::parsed_polygon::ParsedPolygon;

const EPSILON: f32 = 0.0001;

/// The input polyline is in TILE space (y south). Its conversion here is the
/// decoration path's one named entry into render space, and the output
/// polygons are render-space vertices.
pub fn build_polygons(points: &[V2], line_width: f32, arrow: &OnewayArrowDecoration) -> Vec<ParsedPolygon> {
    if points.len() < 2 {
        return Vec::new();
    }

    let render = render_points(points);
    let total_length = polyline_length(&render);
    let minimum_fragment_length = arrow.minimum_fragment_length.max(line_width * arrow.minimum_fragment_length_factor);
    if total_length < minimum_fragment_length {
        return Vec::new();
    }

    let metrics = ArrowMetrics::new(line_width);
    let end_inset = metrics.total_length.max(line_width * 1.5);
    let usable_length = total_length - end_inset * 2.0;
    if usable_length <= EPSILON {
        return Vec::new();
    }

    let placements: Vec<f32> = if usable_length <= arrow.repeat_step {
        vec![total_length * 0.5]
    } else {
        let arrow_count = (usable_length / arrow.repeat_step).floor() as i64 + 1;
        let actual_step = usable_length / (arrow_count - 1).max(1) as f32;
        (0..arrow_count).map(|i| end_inset + i as f32 * actual_step).collect()
    };

    let mut polygons = Vec::with_capacity(placements.len() * 2);
    let turn_check_tolerance = line_width.max(metrics.total_length * 0.5);
    for distance in placements {
        if !is_away_from_sharp_turn(distance, turn_check_tolerance, arrow.turn_threshold_radians, &render) {
            continue;
        }
        let Some((position, tangent)) = sample(distance, &render) else {
            continue;
        };
        make_arrow_polygons(position, tangent, &metrics, &mut polygons);
    }
    polygons
}

struct ArrowMetrics {
    total_length: f32,
    tail_length: f32,
    tail_width: f32,
    head_width: f32,
}

impl ArrowMetrics {
    fn new(line_width: f32) -> ArrowMetrics {
        let total_length = (line_width * 3.0).clamp(26.0, 64.0);
        let head_length = total_length * 0.42;
        let tail_length = (total_length - head_length).max(total_length * 0.58);
        let tail_width = (line_width * 0.22).clamp(3.5, 9.0);
        ArrowMetrics { total_length, tail_length, tail_width, head_width: tail_width * 2.0 }
    }
}

fn make_arrow_polygons(center: V2, tangent: V2, metrics: &ArrowMetrics, out: &mut Vec<ParsedPolygon>) {
    let normal = V2::new(-tangent.y, tangent.x);
    let tail_half_width = metrics.tail_width * 0.5;
    let head_half_width = metrics.head_width * 0.5;

    let tail_start = center - tangent * (metrics.total_length * 0.5);
    let tail_end = tail_start + tangent * metrics.tail_length;
    let tip = tail_start + tangent * metrics.total_length;

    // (left, right, next left) and (right, next right, next left):
    // counter-clockwise in render space, like every tile triangle.
    out.push(ParsedPolygon::new(
        vec![
            tail_start + normal * tail_half_width,
            tail_start - normal * tail_half_width,
            tail_end + normal * tail_half_width,
            tail_end - normal * tail_half_width,
        ],
        vec![0, 1, 2, 1, 3, 2],
    ));
    out.push(ParsedPolygon::new(vec![tail_end + normal * head_half_width, tail_end - normal * head_half_width, tip], vec![0, 1, 2]));
}

fn is_away_from_sharp_turn(distance: f32, tolerance: f32, turn_threshold_radians: f32, points: &[V2]) -> bool {
    if points.len() < 3 {
        return true;
    }
    let mut traversed = 0f32;
    for index in 1..points.len() {
        let start = points[index - 1];
        let end = points[index];
        let segment_length = (end - start).length();
        if segment_length <= EPSILON {
            continue;
        }
        if index > 1 && (distance - traversed).abs() <= tolerance && turn_angle(points[index - 2], start, end) > turn_threshold_radians {
            return false;
        }
        traversed += segment_length;
        if index < points.len() - 1 && (distance - traversed).abs() <= tolerance && turn_angle(start, end, points[index + 1]) > turn_threshold_radians {
            return false;
        }
    }
    true
}

fn turn_angle(previous: V2, current: V2, next: V2) -> f32 {
    // As simd_normalize: a zero vector comes out not finite, and the turn is none.
    let a = current - previous;
    let b = next - current;
    let incoming = a / a.length();
    let outgoing = b / b.length();
    if !incoming.x.is_finite() || !incoming.y.is_finite() || !outgoing.x.is_finite() || !outgoing.y.is_finite() {
        return 0.0;
    }
    incoming.dot(outgoing).clamp(-1.0, 1.0).acos()
}

fn sample(distance: f32, points: &[V2]) -> Option<(V2, V2)> {
    let first = *points.first()?;
    if points.len() < 2 {
        return Some((first, V2::new(1.0, 0.0)));
    }
    let mut traversed = 0f32;
    for index in 1..points.len() {
        let start = points[index - 1];
        let delta = points[index] - start;
        let segment_length = delta.length();
        if segment_length <= EPSILON {
            continue;
        }
        let next_traversed = traversed + segment_length;
        if distance <= next_traversed + EPSILON {
            let t = ((distance - traversed) / segment_length).clamp(0.0, 1.0);
            return Some((start + delta * t, delta / segment_length));
        }
        traversed = next_traversed;
    }
    let last = points[points.len() - 1];
    let fallback_delta = last - points[points.len() - 2];
    let fallback_length = fallback_delta.length();
    if fallback_length <= EPSILON {
        return None;
    }
    Some((last, fallback_delta / fallback_length))
}

fn polyline_length(points: &[V2]) -> f32 {
    points.windows(2).map(|pair| (pair[1] - pair[0]).length()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arrows(points: &[(f32, f32)], width: f32) -> Vec<ParsedPolygon> {
        let points: Vec<V2> = points.iter().map(|&(x, y)| V2::new(x, y)).collect();
        build_polygons(&points, width, &OnewayArrowDecoration::default())
    }

    #[test]
    fn a_short_fragment_carries_no_arrow() {
        assert!(arrows(&[(0.0, 0.0), (90.0, 0.0)], 10.0).is_empty(), "under the 96-unit minimum");
        assert!(arrows(&[(0.0, 0.0)], 10.0).is_empty());
    }

    #[test]
    fn a_fragment_within_one_step_carries_one_arrow_at_its_middle() {
        let polygons = arrows(&[(0.0, 2000.0), (400.0, 2000.0)], 10.0);
        assert_eq!(polygons.len(), 2, "a tail and a head");
        // Total 30 long at width 10, centred at x 200: the tip at 215.
        let tip = polygons[1].vertices[2];
        assert!((tip.x - 215.0).abs() < 1e-3);
        assert!((tip.y - (4096.0 - 2000.0)).abs() < 1e-3, "in render space");
    }

    #[test]
    fn a_long_fragment_repeats_its_arrows_evenly() {
        // 4000 long, width 10: inset 30, usable 3940, ten arrows from 30 to 3970.
        let polygons = arrows(&[(0.0, 100.0), (4000.0, 100.0)], 10.0);
        assert_eq!(polygons.len(), 20);
        let first_tip = polygons[1].vertices[2].x;
        let last_tip = polygons[19].vertices[2].x;
        assert!((first_tip - 45.0).abs() < 1e-3 && (last_tip - 3985.0).abs() < 1e-2);
    }

    #[test]
    fn every_arrow_winds_counter_clockwise_in_render_space() {
        for points in [[(0.0, 100.0), (4000.0, 100.0)], [(4000.0, 100.0), (0.0, 3000.0)], [(100.0, 0.0), (100.0, 4000.0)]] {
            for polygon in arrows(&points, 12.0) {
                assert_eq!(ParsedPolygon::first_clockwise_triangle(&polygon.vertices, &polygon.indices), None);
            }
        }
    }

    #[test]
    fn no_arrow_stands_on_a_sharp_turn() {
        // A right angle at the middle of a 400 + 400 line: the one arrow
        // centred at 400 would sit on the corner.
        let straight = arrows(&[(0.0, 0.0), (200.0, 0.0), (400.0, 0.0)], 10.0);
        assert_eq!(straight.len(), 2);
        let bent = arrows(&[(0.0, 0.0), (200.0, 0.0), (200.0, 200.0)], 10.0);
        assert!(bent.is_empty());
    }
}
