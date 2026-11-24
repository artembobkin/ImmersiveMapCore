// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The Swift engine's `TileGridCoverageTests`: the tiles of one zoom in
//! view, the nearest to where the camera looks first, a limit at most.

mod support;

use std::collections::HashSet;

use map_visibility::globe::{GlobeBounds, GlobeUniform, Visibility};
use map_visibility::frustum::Frustum;
use map_visibility::grid::{GlobeView, Verdict, Walk, distance_squared, flat, flat_copies, globe, globe_shows};
use map_visibility::polygon::CoveragePolygon;
use map_visibility::{FlatRenderState, VisibleTile};
use support::{fixture, meets, rect, tile};

const ROOT: [VisibleTile; 1] = [VisibleTile { z: 0, x: 0, y: 0, wrap: 0 }];

fn nearest(zoom: i32, limit: usize, centre: [f64; 2], test: impl FnMut(VisibleTile) -> Verdict) -> Vec<VisibleTile> {
    let mut out = Vec::new();
    Walk::default().nearest(zoom, limit, &ROOT, centre, false, test, &mut out);
    out
}

fn containing(world: [f64; 2], zoom: i32) -> (i32, i32) {
    let count = (1i64 << zoom) as f64;
    ((world[0] * count).floor() as i32, (world[1] * count).floor() as i32)
}

#[test]
fn every_tile_of_the_zoom_is_reached_when_all_show() {
    let tiles = nearest(3, 1000, [0.5, 0.5], |_| Verdict::Descend);
    assert_eq!(tiles.len(), 64);
    assert_eq!(tiles.iter().collect::<HashSet<_>>().len(), 64);
    assert!(tiles.iter().all(|t| t.z == 3));
}

#[test]
fn the_limit_keeps_the_nearest() {
    let centre = [0.30, 0.70];
    let tiles = nearest(6, 9, centre, |_| Verdict::Descend);
    assert_eq!(tiles.len(), 9);
    let home = containing(centre, 6);
    assert_eq!((tiles[0].x, tiles[0].y), home, "the tile under the camera first");
    let distances: Vec<f64> = tiles.iter().map(|&t| distance_squared(centre, t, false)).collect();
    assert!(distances.windows(2).all(|pair| pair[0] <= pair[1]));
    // The block of three by three round the centre tile.
    assert!(tiles.iter().all(|t| (t.x - home.0).abs() <= 1 && (t.y - home.1).abs() <= 1));
}

#[test]
fn a_rejected_tile_is_not_looked_into() {
    let mut asked = Vec::new();
    let tiles = nearest(4, 1000, [0.1, 0.1], |node| {
        asked.push(node);
        // The world's western half alone shows.
        if node.z > 0 && node.x >= 1 << (node.z - 1) { Verdict::Reject } else { Verdict::Descend }
    });
    assert_eq!(tiles.len(), 8 * 16);
    assert!(tiles.iter().all(|t| t.x < 8));
    assert!(!asked.iter().any(|t| t.z == 2 && t.x >= 2), "nothing under a rejected tile is asked");
}

#[test]
fn an_accepted_subtree_is_not_asked_again() {
    let mut asked_below_root = 0;
    let tiles = nearest(3, 1000, [0.5, 0.5], |node| {
        if node.z > 0 {
            asked_below_root += 1;
        }
        Verdict::AcceptSubtree
    });
    assert_eq!(tiles.len(), 64);
    assert_eq!(asked_below_root, 0);
}

#[test]
fn nothing_is_drawn_without_a_limit() {
    assert!(nearest(3, 0, [0.5, 0.5], |_| Verdict::Descend).is_empty());
}

#[test]
fn the_sphere_closes_round_the_antimeridian() {
    let east = tile(7, 3, 3);
    let centre = [0.01, 0.4];
    assert!((distance_squared(centre, east, true) - 0.01 * 0.01).abs() < 1e-12, "the tile across the antimeridian is a step away");
    assert!(distance_squared(centre, east, false) > 0.7);
}

#[test]
fn the_flat_map_shows_the_tiles_under_its_footprint_in_every_copy() {
    // A world one unit wide, seen as a small square round its middle.
    let state = FlatRenderState { pan: [0.0, 0.0], render_map_size: 1.0 };
    let polygon = CoveragePolygon::new(vec![[-0.1, -0.1], [-0.1, 0.1], [0.1, 0.1], [0.1, -0.1]]);
    let mut tiles = Vec::new();
    flat(&mut Walk::default(), 2, 100, &polygon, &state, [0.5, 0.5], &mut tiles);
    assert_eq!(tiles.iter().copied().collect::<HashSet<_>>(), HashSet::from([tile(1, 1, 2), tile(2, 1, 2), tile(1, 2, 2), tile(2, 2, 2)]));

    // The same square over the antimeridian: both copies of the world.
    let seam = CoveragePolygon::new(vec![[0.4, -0.1], [0.4, 0.1], [0.6, 0.1], [0.6, -0.1]]);
    let mut seam_tiles = Vec::new();
    flat(&mut Walk::default(), 2, 100, &seam, &state, [1.0, 0.5], &mut seam_tiles);
    assert!(seam_tiles.contains(&VisibleTile { z: 2, x: 3, y: 1, wrap: 0 }));
    assert!(seam_tiles.contains(&VisibleTile { z: 2, x: 0, y: 1, wrap: 1 }));
    let mut copies = Vec::new();
    assert_eq!(flat_copies((0, 1, 2), &seam, &state, &mut copies), 1);
    assert_eq!(copies, vec![VisibleTile { z: 2, x: 0, y: 1, wrap: 1 }]);
    assert_eq!(flat_copies((1, 0, 2), &seam, &state, &mut copies), 0);
}

#[test]
fn a_tilted_camera_is_given_the_nearest_tiles_of_its_footprint() {
    let fixture = fixture(13.4, 60.0, 0.0);
    let centre = [(1.0 - fixture.flat.pan[0]) * 0.5, (1.0 - fixture.flat.pan[1]) * 0.5];
    let mut tiles = Vec::new();
    flat(&mut Walk::default(), 15, 64, &fixture.polygon, &fixture.flat, centre, &mut tiles);
    assert_eq!(tiles.len(), 64, "the limit, where the view holds more");
    assert!(tiles.iter().all(|&t| t.z == 15 && meets(&fixture.polygon, rect(t, &fixture.flat))));
    assert_eq!((tiles[0].x, tiles[0].y), containing(centre, 15));
    let distances: Vec<f64> = tiles.iter().map(|&t| distance_squared(centre, t, false)).collect();
    assert!(distances.windows(2).all(|pair| pair[0] <= pair[1]));
}

#[test]
fn a_tile_within_the_footprint_is_taken_whole_and_changes_nothing() {
    for (zoom, pitch, grid_zoom, limit) in [(13.4, 60.0, 15, 512), (13.4, 60.0, 17, 512), (13.4, 60.0, 16, 40)] {
        let fixture = fixture(zoom, pitch, 0.0);
        let centre = [(1.0 - fixture.flat.pan[0]) * 0.5, (1.0 - fixture.flat.pan[1]) * 0.5];
        let mut tiles = Vec::new();
        flat(&mut Walk::default(), grid_zoom, limit, &fixture.polygon, &fixture.flat, centre, &mut tiles);
        // The walk that asks every tile whether it meets the footprint.
        let roots = [-1, 0, 1].map(|wrap| VisibleTile { z: 0, x: 0, y: 0, wrap });
        let mut asked = Vec::new();
        Walk::default().nearest(grid_zoom, limit, &roots, centre, false, |node| if meets(&fixture.polygon, rect(node, &fixture.flat)) { Verdict::Descend } else { Verdict::Reject }, &mut asked);
        assert_eq!(tiles, asked, "the same tiles in the same order at grid zoom {grid_zoom}");
    }
    let square = CoveragePolygon::new(vec![[0.0, 0.0], [0.0, 2.0], [2.0, 2.0], [2.0, 0.0]]);
    assert!(square.contains_square(0.5, 0.5, 1.5, 1.5));
    assert!(square.contains_square(0.0, 0.0, 2.0, 2.0), "its own square, the edges included");
    assert!(!square.contains_square(1.5, 1.5, 2.5, 2.5));
    let diamond = CoveragePolygon::new(vec![[1.0, 0.0], [2.0, 1.0], [1.0, 2.0], [0.0, 1.0]]);
    assert!(diamond.contains_square(0.75, 0.75, 1.25, 1.25));
    assert!(!diamond.contains_square(0.1, 0.1, 0.9, 0.9), "within the bounds, across an edge");
}

/// A camera on the z axis, `distance` from the globe's front point, looking at it down the axis.
fn looking_at_the_front(distance: f64) -> ([f64; 16], [f64; 3]) {
    let (near, far) = (0.01, 100.0);
    let depth = far / (near - far);
    ([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, depth, -1.0, 0.0, 0.0, depth * near - depth * distance, distance], [0.0, 0.0, distance])
}

#[test]
fn the_sphere_shows_its_near_side_alone() {
    let (matrix, eye) = looking_at_the_front(1.5);
    let frustum = Frustum::new(&matrix);
    let view = GlobeView { visibility: Visibility::new(&GlobeUniform { pan_x: 0.0, pan_y: 0.0, radius: 1.0, transition: 0.0 }, eye), frustum: &frustum };
    let mut bounds = GlobeBounds::default();
    let mut tiles = Vec::new();
    globe(&mut Walk::default(), 4, 1000, &view, &mut bounds, [0.5, 0.5], &mut tiles);
    assert!(!tiles.is_empty() && tiles.len() < 256, "some of the sixteen by sixteen, not all");
    assert!(tiles.contains(&tile(8, 8, 4)), "the tile the camera looks at");
    assert!(!tiles.contains(&tile(0, 8, 4)), "nothing of the far side");
    assert!(globe_shows((8, 8, 4), &view, &mut bounds));
    assert!(!globe_shows((0, 8, 4), &view, &mut bounds));
    // The limit keeps those round the centre.
    let mut few = Vec::new();
    globe(&mut Walk::default(), 4, 4, &view, &mut bounds, [0.5, 0.5], &mut few);
    assert_eq!(few.len(), 4);
    assert!(few.iter().all(|t| (7..=8).contains(&t.x) && (7..=8).contains(&t.y)));
}

#[test]
fn the_unroll_at_a_coarse_zoom_shows_every_tile() {
    let (matrix, eye) = looking_at_the_front(3.0);
    let frustum = Frustum::new(&matrix);
    let view = GlobeView { visibility: Visibility::new(&GlobeUniform { pan_x: 0.0, pan_y: 0.0, radius: 1.0, transition: 0.5 }, eye), frustum: &frustum };
    let mut bounds = GlobeBounds::default();
    let mut tiles = Vec::new();
    globe(&mut Walk::default(), 2, 1000, &view, &mut bounds, [0.5, 0.5], &mut tiles);
    assert_eq!(tiles.len(), 16);
    assert!(globe_shows((0, 0, 2), &view, &mut bounds));
}
