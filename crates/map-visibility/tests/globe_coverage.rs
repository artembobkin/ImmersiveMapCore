// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The Swift engine's `GlobeTileCoverageTests` and `GlobeRingMathTests`: the
//! ring rules read off the globe's grid from the look-at tile, with the
//! pinned world cover as the floor and as the ground past the last rule,
//! and the rings of target tiles themselves, the columns closing at the
//! antimeridian and the rows ending at the poles.

mod support;

use map_visibility::frustum::Frustum;
use map_visibility::globe::{FLOOR_ZOOM, GlobeBounds, GlobeCoverage, GlobeCoverageInput, GlobeUniform, GridCell, axis_range, coverage, look_at_tile, ring_range, wrapped_axis_range};
use map_visibility::rules::{FlatRingRule, default_rules, globe_default_rules, normalized};
use map_visibility::{Target, VisibleTile};
use support::{covers, matrix, tile};

const GLOBE: GlobeUniform = GlobeUniform { pan_x: 0.0, pan_y: 0.0, radius: 1.0, transition: 0.0 };

fn rule(zoom_drop: i32, distance: f64) -> FlatRingRule {
    FlatRingRule::new(zoom_drop, distance)
}

/// The coverage from an eye on the z axis looking at the sphere's front
/// point through a wide frustum. The globe at pan zero looks at longitude
/// and latitude zero, the corner four tiles share: the look-at tile is the
/// south-east one unless `look_at` names another.
fn resolved_on(eye_distance: f64, rules: &[FlatRingRule], target_zoom: i32, globe: GlobeUniform, look_at: Option<GridCell>) -> GlobeCoverage {
    let (camera, _) = matrix("globe", eye_distance, 0.0, 0.0);
    let frustum = Frustum::new(&camera);
    let half = (1i64 << target_zoom) / 2;
    let input = GlobeCoverageInput { eye: [0.0, 0.0, eye_distance], globe, rules, look_at: look_at.unwrap_or((half, half)), target_zoom, frustum: &frustum };
    coverage(&input, &mut GlobeBounds::default())
}

/// The eye is high enough for the frustum to hold the first three rings whole.
fn resolved(rules: &[FlatRingRule]) -> GlobeCoverage {
    resolved_on(0.3, rules, 6, GLOBE, None)
}

fn contains(output: &[Target], wanted: VisibleTile) -> bool {
    output.iter().any(|target| target.tile == wanted)
}

fn cover(wanted: VisibleTile, output: &[Target]) -> Option<&Target> {
    output.iter().find(|target| target.tile == wanted || covers(target.tile, wanted))
}

#[test]
fn the_rings_around_the_look_at_tile_follow_the_rules() {
    let resolution = resolved(&default_rules());
    let output = &resolution.targets;
    for x in 31..=33 {
        for y in 31..=33 {
            assert!(contains(output, tile(x, y, 6)), "ring 1 is exact: z6/{x}/{y}");
        }
    }
    assert!(!contains(output, tile(34, 32, 6)), "ring 2 is not drawn at the target zoom");
    assert!(output.iter().any(|t| t.tile.z == 5 && covers(t.tile, tile(34, 32, 6))), "ring 2 is one level coarser");
    assert!(output.iter().any(|t| t.tile.z == 4 && covers(t.tile, tile(35, 32, 6))), "ring 3 is two levels coarser");
    assert!(output.iter().all(|t| t.tile.z >= FLOOR_ZOOM));
    assert!(resolution.metrics.visited_nodes < 600, "the walk stays small");
    for x in 28..=36 {
        for y in 28..=36 {
            assert!(cover(tile(x, y, 6), output).is_some(), "z6/{x}/{y} in front of the eye is covered");
        }
    }
}

#[test]
fn every_target_sits_in_a_band_of_its_zoom() {
    let rules = normalized(&default_rules());
    let output = resolved(&default_rules()).targets;
    assert!(!output.is_empty());
    for target in &output {
        let (nearest, farthest) = ring_range((target.tile.x, target.tile.y, target.tile.z), 6, (32, 32));
        let (nearest, farthest) = (nearest as f64, farthest as f64);
        let mut first_ring = 0.0;
        let mut answers = target.tile.z == FLOOR_ZOOM && farthest > rules.last().map_or(0.0, |rule| rule.distance);
        for rule in &rules {
            let zoom = FLOOR_ZOOM.max(6 - rule.zoom_drop);
            if zoom == target.tile.z && first_ring <= farthest && rule.distance >= nearest {
                answers = true;
            }
            first_ring = rule.distance + 1.0;
        }
        assert!(answers, "{:?} at rings {nearest}...{farthest} answers to no rule", target.tile);
    }
}

#[test]
fn the_bands_report_their_zooms_and_counts() {
    let resolution = resolved(&default_rules());
    assert_eq!(resolution.bands.iter().map(|band| band.zoom).collect::<Vec<_>>(), [6, 5, 4, 2]);
    assert_eq!(resolution.bands.iter().map(|band| band.distance).collect::<Vec<_>>(), [1.0, 2.0, 3.0, 9.0]);
    assert_eq!(resolution.bands[0].tile_count, 9, "the look-at tile and ring 1, all in view");
    assert!(resolution.bands.iter().map(|band| band.tile_count).sum::<usize>() <= resolution.targets.len());
}

#[test]
fn parents_overlap_the_exact_tiles() {
    let output = resolved(&default_rules()).targets;
    let exact: Vec<&Target> = output.iter().filter(|t| t.tile.z == 6).collect();
    assert!(output.iter().filter(|t| t.tile.z < 6).any(|parent| exact.iter().any(|e| covers(parent.tile, e.tile))), "some parent contains an exact tile");
}

#[test]
fn past_the_last_rule_the_ground_is_the_cover() {
    let output = resolved(&[rule(0, 1.0)]).targets;
    assert!(contains(&output, tile(32, 32, 6)));
    assert!(contains(&output, tile(0, 0, FLOOR_ZOOM)));
    assert!(output.iter().all(|t| t.tile.z == 6 || t.tile.z == FLOOR_ZOOM));
    assert_eq!(output.iter().filter(|t| t.tile.z == 6).count(), 9);
}

#[test]
fn the_rings_cross_the_antimeridian() {
    let seam = GlobeUniform { pan_x: 1.0, ..GLOBE };
    let output = resolved_on(0.3, &default_rules(), 6, seam, Some((0, 32))).targets;
    assert!(contains(&output, tile(0, 32, 6)));
    assert!(contains(&output, tile(63, 32, 6)), "the ring's column across the seam is exact");
    assert!(!contains(&output, tile(62, 32, 6)), "ring 2 across the seam is coarser");
}

#[test]
fn shallow_targets_stay_sane() {
    for zoom in 0..=2 {
        let output = resolved_on(4.0, &default_rules(), zoom, GLOBE, None).targets;
        assert!(!output.is_empty(), "z{zoom}");
        assert!(output.iter().all(|t| t.tile.z >= FLOOR_ZOOM && t.tile.z <= zoom), "z{zoom}");
        assert!(output.iter().any(|t| t.tile.z == zoom), "the look-at tile is exact at z{zoom}");
    }
}

#[test]
fn any_rule_list_covers_the_view() {
    let output = resolved(&[rule(2, 1.0), rule(0, 3.0), rule(3, 30.0)]).targets;
    assert!(contains(&output, tile(35, 32, 6)), "ring 3 is exact under the second rule");
    for x in 28..=36 {
        for y in 28..=36 {
            assert!(cover(tile(x, y, 6), &output).is_some(), "z6/{x}/{y} is covered");
        }
    }
}

#[test]
fn a_lineless_rule_names_its_tiles() {
    let resolution = resolved(&[rule(0, 1.0), FlatRingRule { draws_lines: false, ..rule(2, 6.0) }]);
    let lineless: Vec<&Target> = resolution.targets.iter().filter(|t| !t.lines).collect();
    assert!(!lineless.is_empty());
    assert!(lineless.iter().all(|t| t.tile.z == 4 || t.tile.z == FLOOR_ZOOM));
    assert!(lineless.iter().any(|t| t.tile == tile(0, 0, FLOOR_ZOOM)));
    assert!(!lineless.iter().any(|t| t.tile == tile(32, 32, 6)));
    assert!(resolved(&default_rules()).targets.iter().filter(|t| !t.lines).all(|t| t.tile.z < 6), "the default's exact tiles stay lined");
}

#[test]
fn an_unlabelled_rule_names_its_tiles() {
    let resolution = resolved(&[rule(0, 1.0), FlatRingRule { draws_labels: false, ..rule(2, 6.0) }]);
    let unlabelled: Vec<&Target> = resolution.targets.iter().filter(|t| !t.labels).collect();
    assert!(!unlabelled.is_empty());
    assert!(unlabelled.iter().all(|t| t.tile.z == 4 || t.tile.z == FLOOR_ZOOM));
    assert!(!unlabelled.iter().any(|t| t.tile == tile(32, 32, 6)));
    assert!(resolution.targets.iter().all(|t| t.lines), "the two switches are independent");
    assert!(resolved(&default_rules()).targets.iter().filter(|t| !t.labels).all(|t| t.tile.z < 6), "the default labels the exact tiles and nothing past them");
}

#[test]
fn a_raster_rule_names_its_tiles() {
    let resolution = resolved(&[rule(0, 1.0), FlatRingRule { raster_size: Some(128), ..rule(2, 6.0) }]);
    let rasters: Vec<&Target> = resolution.targets.iter().filter(|t| t.raster_size.is_some()).collect();
    assert!(!rasters.is_empty());
    assert!(rasters.iter().all(|t| (t.tile.z == 4 || t.tile.z == FLOOR_ZOOM) && t.raster_size == Some(128)));
    assert!(rasters.iter().any(|t| t.tile == tile(0, 0, FLOOR_ZOOM)));
    assert!(!rasters.iter().any(|t| t.tile == tile(32, 32, 6)));
    assert!(resolved(&default_rules()).targets.iter().filter(|t| t.raster_size.is_some()).all(|t| t.tile.z < 6), "the default's exact tiles are geometry");
    assert!(resolved(&globe_default_rules()).targets.iter().all(|t| t.raster_size.is_none()), "the globe's default draws every ring as geometry");
}

#[test]
fn the_walk_carries_nothing_over() {
    assert_eq!(resolved(&default_rules()), resolved(&default_rules()));
    // Nor do the bounds a host keeps between frames change what a frame resolves.
    let (camera, _) = matrix("globe", 0.3, 0.0, 0.0);
    let frustum = Frustum::new(&camera);
    let rules = default_rules();
    let input = GlobeCoverageInput { eye: [0.0, 0.0, 0.3], globe: GLOBE, rules: &rules, look_at: (32, 32), target_zoom: 6, frustum: &frustum };
    let mut bounds = GlobeBounds::default();
    assert_eq!(coverage(&input, &mut bounds), coverage(&input, &mut bounds));
}

// MARK: The rings

#[test]
fn the_look_at_tile_is_the_centre_floored() {
    assert_eq!(look_at_tile([32.7, 12.2], 6), (32, 12));
    assert_eq!(look_at_tile([64.0, -0.5], 6), (63, 0), "clamped into the grid");
}

#[test]
fn a_target_tiles_ring_is_its_grid_distance() {
    assert_eq!(ring_range((32, 32, 6), 6, (32, 32)), (0, 0));
    assert_eq!(ring_range((33, 31, 6), 6, (32, 32)), (1, 1));
    assert_eq!(ring_range((30, 37, 6), 6, (32, 32)), (5, 5));
}

#[test]
fn a_coarser_tile_spans_its_rings() {
    // z4/8/8 holds columns and rows 32 to 35.
    assert_eq!(ring_range((8, 8, 4), 6, (32, 32)), (0, 3));
    // z4/9/8 holds columns 36 to 39.
    assert_eq!(ring_range((9, 8, 4), 6, (32, 32)), (4, 7));
    assert_eq!(ring_range((0, 0, 0), 6, (32, 32)), (0, 32));
}

#[test]
fn the_columns_close_on_themselves() {
    let look_at = (0, 32);
    assert_eq!(ring_range((63, 32, 6), 6, look_at), (1, 1));
    assert_eq!(ring_range((60, 32, 6), 6, look_at), (4, 4));
    assert_eq!(ring_range((32, 32, 6), 6, look_at), (32, 32), "the antipode's column is half the world away");
    // z4/15/8 holds columns 60 to 63, just across the seam.
    assert_eq!(ring_range((15, 8, 4), 6, look_at), (1, 4));
    // z3/4/4 holds columns 32 to 39: the antipode's column is in it.
    assert_eq!(wrapped_axis_range(32, 8, 0, 64), (25, 32));
    // Columns 24 to 31 lie short of the antipode going up.
    assert_eq!(wrapped_axis_range(24, 8, 0, 64), (24, 31));
}

#[test]
fn the_rows_end_at_the_poles() {
    assert_eq!(ring_range((32, 63, 6), 6, (32, 0)), (63, 63));
    assert_eq!(axis_range(0, 4, 0), (0, 3));
}

#[test]
fn the_shallow_world_is_a_few_rings_wide() {
    assert_eq!(ring_range((0, 0, 0), 0, (0, 0)), (0, 0));
    assert_eq!(ring_range((3, 2, 2), 2, (0, 2)), (1, 1));
    assert_eq!(ring_range((2, 2, 2), 2, (0, 2)), (2, 2));
}
