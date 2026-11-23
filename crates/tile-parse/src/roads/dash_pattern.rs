// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Cuts a line fragment into the pieces a dashed pass draws: the dashes
//! themselves, each a fragment of its own. A solid pass gets the fragment
//! back whole. A port of the Swift `RoadDashPattern`.

use std::borrow::Cow;

use map_style::style::LineGeometryStyle;

use crate::geom::V2;
use crate::parse_line::ClippedLineFragment;

const EPSILON: f32 = 0.0001;

/// The fragments a pass tessellates: one for a solid style, the dashes for a
/// dashed one. With `dash_resets_per_segment` the pattern restarts on every
/// segment of the line and centres its whole dashes on it, pulled in from
/// the corners. Otherwise it runs continuously along the path.
pub fn fragments<'a>(fragment: &'a ClippedLineFragment, style: &LineGeometryStyle) -> Cow<'a, [ClippedLineFragment]> {
    if !style.uses_dash_pattern() {
        return Cow::Borrowed(std::slice::from_ref(fragment));
    }
    if !style.dash_resets_per_segment {
        return Cow::Owned(dashed_fragments(fragment, style.dash_length as f32, style.dash_gap as f32));
    }
    let points = &fragment.points;
    if points.len() < 2 {
        return Cow::Borrowed(std::slice::from_ref(fragment));
    }

    let mut segmented = Vec::with_capacity(points.len() - 1);
    let corner_inset = (style.line_width as f32).max(style.dash_length as f32);
    for index in 0..points.len() - 1 {
        let segment_start = points[index];
        let segment_end = points[index + 1];
        let Some(segment_direction) = direction(segment_start, segment_end) else {
            continue;
        };

        let mut trimmed_start = segment_start;
        let mut trimmed_end = segment_end;

        if index > 0
            && let Some(previous_direction) = direction(points[index - 1], segment_start)
            && is_turn(previous_direction, segment_direction)
        {
            trimmed_start += segment_direction * corner_inset;
        }

        if index < points.len() - 2
            && let Some(next_direction) = direction(segment_end, points[index + 2])
            && is_turn(segment_direction, next_direction)
        {
            trimmed_end -= segment_direction * corner_inset;
        }

        if (trimmed_end - trimmed_start).length() <= EPSILON {
            continue;
        }

        segmented.extend(centered_full_dash_fragments_for_segment(trimmed_start, trimmed_end, style.dash_length as f32, style.dash_gap as f32));
    }
    Cow::Owned(segmented)
}

fn direction(start: V2, end: V2) -> Option<V2> {
    let delta = end - start;
    let length = delta.length();
    if length <= EPSILON {
        return None;
    }
    Some(delta / length)
}

fn is_turn(previous: V2, current: V2) -> bool {
    let cross = previous.x * current.y - previous.y * current.x;
    let dot = previous.x * current.x + previous.y * current.y;
    cross.abs() > 0.001 || dot < 0.999
}

fn centered_full_dash_fragments_for_segment(start: V2, end: V2, dash_length: f32, dash_gap: f32) -> Vec<ClippedLineFragment> {
    let delta = end - start;
    let segment_length = delta.length();
    if segment_length <= EPSILON || dash_length <= EPSILON || dash_gap <= EPSILON {
        return Vec::new();
    }

    let direction = delta / segment_length;
    let pattern_length = dash_length + dash_gap;
    let dash_count = ((segment_length + dash_gap) / pattern_length).floor() as i64;
    if dash_count <= 0 {
        return Vec::new();
    }

    let occupied_length = dash_count as f32 * dash_length + (dash_count - 1).max(0) as f32 * dash_gap;
    let leading_inset = ((segment_length - occupied_length) * 0.5).max(0.0);
    let first_dash_start = start + direction * leading_inset;

    (0..dash_count)
        .map(|index| {
            let offset = index as f32 * pattern_length;
            let dash_start = first_dash_start + direction * offset;
            let dash_end = dash_start + direction * dash_length;
            ClippedLineFragment { points: vec![dash_start, dash_end], start_clipped: false, end_clipped: false }
        })
        .collect()
}

fn points_equal(a: V2, b: V2) -> bool {
    (a.x - b.x).abs() <= EPSILON && (a.y - b.y).abs() <= EPSILON
}

fn dashed_fragments(fragment: &ClippedLineFragment, dash_length: f32, dash_gap: f32) -> Vec<ClippedLineFragment> {
    let points = &fragment.points;
    if points.len() < 2 || dash_length <= EPSILON || dash_gap <= EPSILON {
        return vec![fragment.clone()];
    }

    let mut dashed = Vec::new();
    let mut current: Vec<V2> = Vec::with_capacity(points.len());

    let mut is_dash = true;
    let mut remaining_pattern_length = dash_length;
    let mut dash_started_at_fragment_start = true;

    let append_point_if_needed = |current: &mut Vec<V2>, point: V2| {
        if let Some(&last) = current.last()
            && points_equal(last, point)
        {
            return;
        }
        current.push(point);
    };

    let finalize_dash = |current: &mut Vec<V2>, dashed: &mut Vec<ClippedLineFragment>, started_at_start: bool, ended_at_end: bool| {
        if current.len() < 2 {
            current.clear();
            return;
        }
        dashed.push(ClippedLineFragment {
            points: std::mem::take(current),
            start_clipped: if started_at_start { fragment.start_clipped } else { false },
            end_clipped: if ended_at_end { fragment.end_clipped } else { false },
        });
    };

    for index in 0..points.len() - 1 {
        let segment_start = points[index];
        let segment_end = points[index + 1];
        let segment_delta = segment_end - segment_start;
        let segment_length = segment_delta.length();
        if segment_length <= EPSILON {
            continue;
        }

        let direction = segment_delta / segment_length;
        let mut current_point = segment_start;
        let mut remaining_segment_length = segment_length;

        while remaining_segment_length > EPSILON {
            let traveled_length = remaining_pattern_length.min(remaining_segment_length);
            let next_point = current_point + direction * traveled_length;
            let reaches_fragment_end = index == points.len() - 2 && remaining_segment_length - traveled_length <= EPSILON;

            if is_dash {
                if current.is_empty() {
                    append_point_if_needed(&mut current, current_point);
                }
                append_point_if_needed(&mut current, next_point);
            }

            current_point = next_point;
            remaining_segment_length -= traveled_length;
            remaining_pattern_length -= traveled_length;

            if remaining_pattern_length <= EPSILON {
                if is_dash {
                    finalize_dash(&mut current, &mut dashed, dash_started_at_fragment_start, reaches_fragment_end);
                }
                is_dash = !is_dash;
                remaining_pattern_length = if is_dash { dash_length } else { dash_gap };
                dash_started_at_fragment_start = false;
            }
        }
    }

    if is_dash && !current.is_empty() {
        finalize_dash(&mut current, &mut dashed, dash_started_at_fragment_start, true);
    }

    dashed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fragment(points: &[(f32, f32)], start_clipped: bool, end_clipped: bool) -> ClippedLineFragment {
        ClippedLineFragment { points: points.iter().map(|&(x, y)| V2::new(x, y)).collect(), start_clipped, end_clipped }
    }

    fn dashed(length: f64, gap: f64, resets: bool, width: f64) -> LineGeometryStyle {
        LineGeometryStyle { dash_length: length, dash_gap: gap, dash_resets_per_segment: resets, ..LineGeometryStyle::new(width) }
    }

    #[test]
    fn a_solid_pass_takes_the_fragment_whole() {
        let f = fragment(&[(0.0, 0.0), (100.0, 0.0)], true, false);
        let out = fragments(&f, &LineGeometryStyle::new(4.0));
        assert!(matches!(out, Cow::Borrowed(_)));
        assert_eq!(out.as_ref(), std::slice::from_ref(&f));
    }

    #[test]
    fn a_continuous_pattern_runs_across_the_corners() {
        // 10 on, 5 off along an L of 20 + 20.
        let f = fragment(&[(0.0, 0.0), (20.0, 0.0), (20.0, 20.0)], true, true);
        let out = fragments(&f, &dashed(10.0, 5.0, false, 2.0));
        let starts: Vec<V2> = out.iter().map(|d| d.points[0]).collect();
        assert_eq!(starts, vec![V2::new(0.0, 0.0), V2::new(15.0, 0.0), V2::new(20.0, 10.0)]);
        // The second dash turns the corner: 5 along x, 5 along y.
        assert_eq!(out[1].points, vec![V2::new(15.0, 0.0), V2::new(20.0, 0.0), V2::new(20.0, 5.0)]);
        // Only the first dash keeps the start cut, only one ending at the end keeps the end cut.
        assert!(out[0].start_clipped && !out[0].end_clipped);
        assert!(!out[1].start_clipped && !out[1].end_clipped);
        assert_eq!(out[2].points, vec![V2::new(20.0, 10.0), V2::new(20.0, 20.0)]);
        assert!(!out[2].start_clipped && out[2].end_clipped);
    }

    #[test]
    fn a_per_segment_pattern_centres_whole_dashes() {
        // 100 long, 10 on 10 off: five dashes occupy 90, inset 5 each side.
        let f = fragment(&[(0.0, 0.0), (100.0, 0.0)], false, false);
        let out = fragments(&f, &dashed(10.0, 10.0, true, 2.0));
        assert_eq!(out.len(), 5);
        assert_eq!(out[0].points, vec![V2::new(5.0, 0.0), V2::new(15.0, 0.0)]);
        assert_eq!(out[4].points, vec![V2::new(85.0, 0.0), V2::new(95.0, 0.0)]);
    }

    #[test]
    fn a_per_segment_pattern_pulls_in_from_a_corner() {
        // The inset is max(width, dash) = 20 at the corner only.
        let f = fragment(&[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)], false, false);
        let out = fragments(&f, &dashed(10.0, 10.0, true, 20.0));
        // First segment trimmed to 0..80: four dashes occupy 70, inset 5.
        assert_eq!(out[0].points[0], V2::new(5.0, 0.0));
        assert_eq!(out.iter().filter(|d| d.points[0].y == 0.0 && d.points[1].y == 0.0).count(), 4);
        // Second segment trimmed to 20..100.
        let second: Vec<_> = out.iter().filter(|d| d.points[0].x == 100.0).collect();
        assert_eq!(second.len(), 4);
        assert_eq!(second[0].points[0], V2::new(100.0, 25.0));
    }

    #[test]
    fn a_segment_shorter_than_one_dash_draws_none_per_segment() {
        let f = fragment(&[(0.0, 0.0), (5.0, 0.0)], false, false);
        assert!(fragments(&f, &dashed(10.0, 10.0, true, 1.0)).is_empty());
    }
}
