// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The Swift engine's `FlatRingRuleCoverageTests` and `FlatTileCoverageTests`:
//! the ground in view cut into bands by the distance in tiles from the
//! look-at tile, one band per rule, each drawn at the rule's zoom, nothing
//! beyond the last rule and nothing outside the footprint.

mod support;

use std::collections::{HashMap, HashSet};

use map_visibility::flat::{FlatCoverage, Square, band_polygons, clipped, look_at_tile, resolve, square, tiles};
use map_visibility::polygon::{CoveragePolygon, footprint};
use map_visibility::rules::{DISTANCE_RANGE, FlatRingRule, RASTER_SIZES, ZOOM_DROP_RANGE, default_rules, normalized};
use map_visibility::{FlatRenderState, Target, VisibleTile};
use support::{Fixture, covers, fixture, lies, matrix, meets, overlap, rect};

/// The zoom of the world cover, which no band goes down to.
const BACKDROP: Option<i32> = Some(0);

fn rule(zoom_drop: i32, distance: f64) -> FlatRingRule {
    FlatRingRule::new(zoom_drop, distance)
}

fn covered(fixture: &Fixture, target_zoom: i32, rules: &[FlatRingRule]) -> FlatCoverage {
    resolve(&fixture.flat, target_zoom, BACKDROP, rules, &fixture.polygon, None)
}

fn key_set(targets: &[Target]) -> HashSet<VisibleTile> {
    targets.iter().map(|target| target.tile).collect()
}

fn tiles_at(zoom: i32, polygon: &CoveragePolygon, flat: &FlatRenderState) -> (Vec<VisibleTile>, usize) {
    let (mut out, mut visited) = (Vec::new(), 0);
    tiles(zoom, polygon, flat, &mut out, &mut visited);
    (out, visited)
}

#[test]
fn the_footprint_over_the_relief_holds_the_planes_and_reaches_past_it() {
    let (camera, _) = matrix("relief", 12.5, 0.0, 0.0);
    let plane = footprint(&camera, None).expect("the plane's footprint");
    let slab = footprint(&camera, Some((-0.05, 0.2))).expect("the slab's footprint");
    for vertex in plane.vertices() {
        assert!(slab.intersects(vertex[0] - 1e-4, vertex[1] - 1e-4, vertex[0] + 1e-4, vertex[1] + 1e-4), "the plane's footprint lies within the slab's");
    }
    let (plane_bounds, slab_bounds) = (plane.bounds(), slab.bounds());
    assert!(slab_bounds.0 < plane_bounds.0, "wider at the sides");
    assert!(slab_bounds.1 > plane_bounds.1);
    assert!(slab.signed_area().abs() > plane.signed_area().abs());
    assert!(slab.vertices().len() > 2);
    let same = footprint(&camera, Some((0.0, 0.0))).expect("a footprint");
    assert_eq!(same.vertices().len(), plane.vertices().len(), "a slab of no height is the plane");
}

#[test]
fn each_band_carries_its_rules_terrain_zoom() {
    let fixture = fixture(13.4, 60.0, 0.0);
    let rules = [rule(0, 1.0), FlatRingRule { terrain_zoom_drop: 5, ..rule(2, 3.0) }];
    let resolved = covered(&fixture, 13, &rules);
    assert_eq!(resolved.bands.iter().map(|band| band.zoom).collect::<Vec<_>>(), [13, 11]);
    assert_eq!(resolved.bands.iter().map(|band| band.terrain_zoom).collect::<Vec<_>>(), [13, 6]);
    let defaults = covered(&fixture, 13, &default_rules());
    assert_eq!(defaults.bands.iter().map(|band| band.terrain_zoom).collect::<Vec<_>>(), [13, 10, 10, 6], "the defaults: the near ring's own, then the drops of the far bands");
}

#[test]
fn the_rules_are_normalized() {
    let messy = [rule(3, 4.0), rule(-1, 1.0), rule(99, -5.0), rule(1, 4.0), rule(2, 100_000.0)];
    let cleaned = normalized(&messy);
    assert_eq!(cleaned.iter().map(|rule| rule.distance).collect::<Vec<_>>(), [0.0, 1.0, 4.0, DISTANCE_RANGE.1]);
    assert_eq!(cleaned.iter().map(|rule| rule.zoom_drop).collect::<Vec<_>>(), [ZOOM_DROP_RANGE.1, 0, 3, 2], "drops clamped, the first of two rules at one distance kept");
    assert_eq!(normalized(&[]), default_rules(), "at least one rule");
    assert_eq!(normalized(&default_rules()), default_rules(), "the default is already normal");
    let defaults = default_rules();
    assert_eq!(defaults.iter().map(|rule| rule.zoom_drop).collect::<Vec<_>>(), [0, 1, 2, 4]);
    assert_eq!(defaults.iter().map(|rule| rule.distance).collect::<Vec<_>>(), [1.0, 2.0, 3.0, 9.0]);
    assert_eq!(defaults.iter().map(|rule| rule.draws_lines).collect::<Vec<_>>(), [true, false, false, false]);
    assert_eq!(defaults.iter().map(|rule| rule.draws_labels).collect::<Vec<_>>(), [true, false, false, false], "only the exact tiles are labelled");
    assert_eq!(defaults.iter().map(|rule| rule.raster_size).collect::<Vec<_>>(), [None, Some(128), Some(128), Some(64)], "past the exact tiles the ground is textures");
    let unlabelled = normalized(&[FlatRingRule { draws_labels: false, ..rule(0, 1.0) }]);
    assert_eq!(unlabelled.iter().map(|rule| rule.draws_labels).collect::<Vec<_>>(), [false], "the label switch survives normalizing");
}

#[test]
fn an_unlabelled_rule_names_its_tiles() {
    let rules = [rule(0, 1.0), FlatRingRule { draws_labels: false, ..rule(2, 20.0) }];
    let fixture = fixture(16.0, 75.0, 47.0);
    let resolution = covered(&fixture, 16, &rules);
    let unlabelled: Vec<&Target> = resolution.targets.iter().filter(|target| !target.labels).collect();
    assert!(!unlabelled.is_empty());
    assert!(unlabelled.iter().all(|target| target.tile.z == 14));
    assert!(resolution.targets.iter().filter(|target| target.tile.z == 16).all(|target| target.labels));
    assert!(resolution.targets.iter().all(|target| target.lines), "the two switches are independent");
    let defaults = covered(&fixture, 16, &default_rules());
    assert!(defaults.targets.iter().any(|target| !target.labels), "the default leaves the far rings unlabelled");
    assert!(defaults.targets.iter().filter(|target| target.tile.z == 16).all(|target| target.labels), "the default labels the exact tiles");
}

#[test]
fn a_raster_rule_names_its_tiles_with_their_size() {
    let rules = [rule(0, 1.0), FlatRingRule { raster_size: Some(256), ..rule(2, 20.0) }];
    let fixture = fixture(16.0, 75.0, 47.0);
    let resolution = covered(&fixture, 16, &rules);
    let rasters: Vec<&Target> = resolution.targets.iter().filter(|target| target.raster_size.is_some()).collect();
    assert!(!rasters.is_empty());
    assert!(rasters.iter().all(|target| target.tile.z == 14 && target.raster_size == Some(256)));
    assert!(resolution.targets.iter().filter(|target| target.tile.z == 16).all(|target| target.raster_size.is_none()));
    let defaults = covered(&fixture, 16, &default_rules());
    assert!(defaults.targets.iter().any(|target| target.raster_size.is_some()), "the default draws its far rings from textures");
    assert!(defaults.targets.iter().filter(|target| target.tile.z == 16).all(|target| target.raster_size.is_none()), "the default draws the exact tiles as geometry");
}

#[test]
fn the_raster_size_is_normalized() {
    let rules = [rule(0, 1.0), FlatRingRule { raster_size: Some(300), ..rule(1, 2.0) }, FlatRingRule { raster_size: Some(1), ..rule(2, 3.0) }, FlatRingRule { raster_size: Some(100_000), ..rule(3, 4.0) }];
    assert_eq!(normalized(&rules).iter().map(|rule| rule.raster_size).collect::<Vec<_>>(), [None, Some(256), Some(64), Some(1024)]);
    assert!(RASTER_SIZES.iter().all(|size| size.is_power_of_two()));
}

#[test]
fn the_square_is_the_look_at_tile_and_its_rings() {
    for zoom in [3, 9, 16] {
        let fixture = fixture(zoom as f64, 60.0, 33.0);
        let (x, y) = look_at_tile(zoom, &fixture.flat);
        let r = rect(VisibleTile { z: zoom, x, y, wrap: 0 }, &fixture.flat);
        assert!(r.min_x <= 1e-4 && r.max_x >= -1e-4 && r.min_y <= 1e-4 && r.max_y >= -1e-4, "z{zoom}: the look-at tile holds the origin");
        let size = r.max_x - r.min_x;
        let alone = square(0.0, zoom, &fixture.flat);
        assert!((alone.min_x - r.min_x).abs() <= 1e-4);
        assert!((alone.max_y - r.max_y).abs() <= 1e-4);
        let three = square(3.0, zoom, &fixture.flat);
        assert!((three.max_x - three.min_x - size * 7.0).abs() <= 1e-3);
        assert!((three.max_y - three.min_y - size * 7.0).abs() <= 1e-3);
        assert!((three.min_x - (r.min_x - size * 3.0)).abs() <= 1e-3);
    }
}

#[test]
fn a_band_is_the_footprint_inside_its_square() {
    let rules = [rule(0, 2.0)];
    for pitch in [0.0, 40.0, 64.0, 75.0] {
        for bearing in (0..360).step_by(17) {
            let fixture = fixture(15.07, pitch, bearing as f64);
            let resolution = covered(&fixture, 15, &rules);
            let reach = square(2.0, 15, &fixture.flat);
            let (in_view, _) = tiles_at(15, &fixture.polygon, &fixture.flat);
            let expected: HashSet<VisibleTile> = in_view.into_iter().filter(|tile| lies(rect(*tile, &fixture.flat), reach)).collect();
            assert_eq!(key_set(&resolution.targets), expected, "pitch {pitch} bearing {bearing}");
            assert!(!expected.is_empty());
            assert!(resolution.targets.len() <= 25);
        }
    }
}

/// Whether the tile has ground in the band of its own zoom's rule: inside the rule's square and outside the previous rule's.
fn band_holds(tile: VisibleTile, rules: &[FlatRingRule], fixture: &Fixture) -> bool {
    let r = rect(tile, &fixture.flat);
    let mut inner: Option<Square> = None;
    for rule in normalized(rules) {
        let reach = square(rule.distance, 15, &fixture.flat);
        if 15 - rule.zoom_drop != tile.z {
            inner = Some(reach);
            continue;
        }
        return band_polygons(&fixture.polygon, reach, inner, (r.max_x - r.min_x) * 0.01).iter().any(|piece| meets(piece, r));
    }
    false
}

#[test]
fn a_turn_only_changes_what_the_footprint_holds() {
    let rules = [rule(0, 1.0), rule(1, 5.0), rule(3, 23.0)];
    let bearings: Vec<i32> = (0..360).step_by(9).collect();
    let fixtures: Vec<Fixture> = bearings.iter().map(|bearing| fixture(15.07, 70.0, *bearing as f64)).collect();
    let placed: Vec<HashSet<VisibleTile>> = fixtures.iter().map(|fixture| key_set(&covered(fixture, 15, &rules).targets)).collect();
    let ever_placed: HashSet<VisibleTile> = placed.iter().flatten().copied().collect();
    assert!(ever_placed.len() > placed[0].len(), "the turn brings tiles in and out");
    for (index, fixture) in fixtures.iter().enumerate() {
        for tile in &ever_placed {
            if placed[index].contains(tile) {
                continue;
            }
            // Drawn in by the band's inset the tile may still touch the
            // footprint along an edge: what it must not do is reach into it.
            let r = rect(*tile, &fixture.flat);
            let inset = (r.max_x - r.min_x) * 0.01;
            let reaches_in = fixture.polygon.intersects(r.min_x + inset, r.min_y + inset, r.max_x - inset, r.max_y - inset) && band_holds(*tile, &rules, fixture);
            assert!(!reaches_in, "bearing {}: {tile:?} is in view in its band and not placed", bearings[index]);
        }
    }
}

#[test]
fn a_street_tilt_places_each_rules_band() {
    let rules = [rule(0, 0.0), rule(1, 2.0), rule(4, 40.0)];
    let fixture = fixture(16.0, 75.0, 47.0);
    let resolution = covered(&fixture, 16, &rules);
    assert_eq!(resolution.bands.iter().map(|band| band.zoom).collect::<Vec<_>>(), [16, 15, 12]);
    assert_eq!(resolution.bands.iter().map(|band| band.distance).collect::<Vec<_>>(), [0.0, 2.0, 40.0]);
    assert_eq!(resolution.bands[0].tile_count, 1, "the look-at tile alone");
    assert!(resolution.bands.iter().all(|band| band.tile_count > 0), "every rule has ground at a street tilt");
    let squares: Vec<Square> = rules.iter().map(|rule| square(rule.distance, 16, &fixture.flat)).collect();
    for target in &resolution.targets {
        let index = rules.iter().position(|rule| 16 - rule.zoom_drop == target.tile.z).unwrap_or_else(|| panic!("{:?} is at a rule's zoom", target.tile));
        let r = rect(target.tile, &fixture.flat);
        assert!(overlap(r, squares[index]), "{:?} reaches into its rule's square", target.tile);
        if index > 0 {
            assert!(!lies(r, squares[index - 1]), "{:?} is not wholly under the nearer band", target.tile);
        }
        assert!(meets(&fixture.polygon, r), "{:?} is in view", target.tile);
    }
    let order = |target: &Target| (-target.tile.z, target.tile.wrap, target.tile.x, target.tile.y);
    assert!(resolution.targets.windows(2).all(|pair| order(&pair[0]) < order(&pair[1])), "finest first, each tile once");
    assert!(resolution.targets.len() <= 40, "a few tiles per band");
    assert_eq!(covered(&fixture, 16, &rules), resolution, "deterministic");
}

#[test]
fn the_bands_cover_the_footprint_inside_the_last_square() {
    let rules = [rule(0, 1.0), rule(2, 6.0), rule(4, 30.0)];
    for bearing in [0.0, 36.0, 89.0, 200.0] {
        let fixture = fixture(15.07, 70.0, bearing);
        let resolution = covered(&fixture, 15, &rules);
        let rects: Vec<Square> = resolution.targets.iter().map(|target| rect(target.tile, &fixture.flat)).collect();
        let ground = clipped(&fixture.polygon, square(30.0, 15, &fixture.flat)).expect("the footprint reaches into the last square");
        let vertices = ground.vertices();
        let count = vertices.len() as f64;
        let centroid = [vertices.iter().map(|v| v[0]).sum::<f64>() / count, vertices.iter().map(|v| v[1]).sum::<f64>() / count];
        let mut samples = 0;
        for index in 0..vertices.len() {
            let (a, b) = (vertices[index], vertices[(index + 1) % vertices.len()]);
            let mut u = 0.05;
            while u <= 0.95 + 1e-9 {
                let mut v = 0.05;
                while v <= 0.95 + 1e-9 {
                    let px = centroid[0] + (a[0] + (b[0] - a[0]) * u - centroid[0]) * v;
                    let py = centroid[1] + (a[1] + (b[1] - a[1]) * u - centroid[1]) * v;
                    samples += 1;
                    let under_a_tile = rects.iter().any(|r| px >= r.min_x - 1e-3 && px <= r.max_x + 1e-3 && py >= r.min_y - 1e-3 && py <= r.max_y + 1e-3);
                    assert!(under_a_tile, "bearing {bearing}: ({px}, {py}) is in view inside the last square and under no tile");
                    v += 0.15;
                }
                u += 0.15;
            }
        }
        assert!(samples > 100);
    }
}

#[test]
fn the_last_rule_is_the_reach() {
    let fixture = fixture(16.0, 75.0, 0.0);
    let short = [rule(0, 1.0), rule(2, 3.0)];
    let long = [rule(0, 1.0), rule(2, 3.0), rule(5, 60.0)];
    let short_resolution = covered(&fixture, 16, &short);
    let long_resolution = covered(&fixture, 16, &long);
    assert!(short_resolution.targets.len() < long_resolution.targets.len());
    let shared: HashSet<VisibleTile> = long_resolution.targets.iter().filter(|target| target.tile.z >= 14).map(|target| target.tile).collect();
    assert_eq!(key_set(&short_resolution.targets), shared, "the shared rules place the same tiles");
    let reach = square(3.0, 16, &fixture.flat);
    for target in &short_resolution.targets {
        assert!(overlap(rect(target.tile, &fixture.flat), reach), "{:?} is not beyond the last rule", target.tile);
    }
}

#[test]
fn the_drop_is_floored_by_the_backdrop() {
    let fixture = fixture(3.0, 75.0, 0.0);
    let deep = [rule(0, 0.0), rule(8, 7.0)];
    let with_backdrop = covered(&fixture, 3, &deep);
    assert_eq!(with_backdrop.bands.iter().map(|band| band.zoom).collect::<Vec<_>>(), [3, 1]);
    assert!(with_backdrop.targets.iter().all(|target| target.tile.z > 0));
    let without = resolve(&fixture.flat, 3, None, &deep, &fixture.polygon, None);
    assert_eq!(without.bands.iter().map(|band| band.zoom).collect::<Vec<_>>(), [3, 0]);
    assert!(without.targets.iter().any(|target| target.tile.z == 0));
}

// MARK: The enumeration of one zoom under a polygon

/// A z9 world whose tiles are one world unit wide, so distances read in tiles.
const ZOOM: i32 = 9;
const WORLD: FlatRenderState = FlatRenderState { pan: [0.0, 0.0], render_map_size: 512.0 };

/// The world position of a point in tile units (tile y grows south, world y north).
fn world(point: [f64; 2]) -> [f64; 2] {
    let (x, y) = (point[0].floor(), point[1].floor());
    let origin = WORLD.tile_origin_and_size(x as i32, y as i32, ZOOM, 0);
    [origin[0] + (point[0] - x) * origin[2], origin[1] + (1.0 - (point[1] - y)) * origin[2]]
}

/// The ground a square 45 degree frustum sees from a camera 1.2 tiles from
/// the look-at point at `tilt` degrees, as the convex wedge the enumeration
/// tests tiles against.
fn wedge(tilt: f64) -> CoveragePolygon {
    let (look_at, distance, reach, near) = ([256.5, 250.5], 1.2, 40.0, 0.25);
    let eye_ground = [look_at[0], look_at[1] + distance * tilt.to_radians().sin()];
    let half_width = |ahead: f64| ahead * (std::f64::consts::PI / 8.0).tan() + 0.5;
    let (near_y, far_y) = (eye_ground[1] - near, eye_ground[1] - reach);
    let corners = [[eye_ground[0] - half_width(near), near_y], [eye_ground[0] + half_width(near), near_y], [eye_ground[0] + half_width(reach), far_y], [eye_ground[0] - half_width(reach), far_y]];
    CoveragePolygon::new(corners.map(world).to_vec())
}

/// The leaves the polygon meets, column by column.
fn leaves(shape: &CoveragePolygon) -> Vec<VisibleTile> {
    let (min_x, max_x, min_y, max_y) = shape.bounds();
    let half = WORLD.render_map_size / 2.0;
    let mut found = Vec::new();
    for y in (half - max_y).floor() as i32 - 1..=(half - min_y).floor() as i32 + 1 {
        for x in (min_x + half).floor() as i32 - 1..=(max_x + half).floor() as i32 + 1 {
            if meets(shape, rect(VisibleTile { z: ZOOM, x, y, wrap: 0 }, &WORLD)) {
                found.push(VisibleTile { z: ZOOM, x, y, wrap: 0 });
            }
        }
    }
    found
}

#[test]
fn the_enumeration_at_a_zoom_is_the_tiles_the_polygon_meets() {
    let shape = wedge(75.0);
    let (found, _) = tiles_at(ZOOM, &shape, &WORLD);
    let unique: HashSet<VisibleTile> = found.iter().copied().collect();
    assert_eq!(unique, leaves(&shape).into_iter().collect::<HashSet<_>>());
    assert_eq!(unique.len(), found.len());
    assert_eq!(tiles_at(ZOOM, &shape, &WORLD).0, found, "deterministic");
}

#[test]
fn a_coarse_zoom_enumerates_the_footprint_at_that_zoom() {
    let shape = wedge(75.0);
    let (backdrop, visited) = tiles_at(3, &shape, &WORLD);
    assert!(!backdrop.is_empty());
    assert!(visited > backdrop.len());
    assert!(backdrop.iter().all(|tile| tile.z == 3));
    for leaf in leaves(&shape) {
        assert!(backdrop.iter().any(|tile| tile.wrap == leaf.wrap && covers(*tile, leaf)), "{leaf:?} is under a backdrop tile");
    }
    assert_eq!(tiles_at(3, &shape, &WORLD).0, backdrop, "deterministic");
}

#[test]
fn the_targets_come_finest_first() {
    // Two bands of a tilted view: every tile of the finer zoom before any of the coarser, each zoom by column and row.
    let fixture = fixture(16.0, 75.0, 47.0);
    let resolution = covered(&fixture, 16, &[rule(0, 1.0), rule(2, 20.0)]);
    let zooms: HashMap<i32, usize> = resolution.targets.iter().fold(HashMap::new(), |mut counts, target| {
        *counts.entry(target.tile.z).or_default() += 1;
        counts
    });
    assert_eq!(zooms.len(), 2);
    assert!(resolution.targets[..zooms[&16]].iter().all(|target| target.tile.z == 16));
    assert!(resolution.targets[zooms[&16]..].windows(2).all(|pair| (pair[0].tile.x, pair[0].tile.y) < (pair[1].tile.x, pair[1].tile.y)));
}
