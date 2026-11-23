// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The flat map's coverage by ring rules: the ground in view is cut into
//! bands by the distance, in tiles, from the tile the camera looks at, one
//! band per rule, and each band is drawn at the rule's zoom.
//!
//! The look-at tile is the target zoom's tile under the point the camera
//! looks at (the render world's origin). A rule's square is that tile and
//! `distance` rings of target tiles around it, and a band's ground is the
//! frustum's footprint inside the rule's square and outside the previous
//! rule's. The look-at point stays where it is through a turn or a tilt of
//! the camera, and the squares run along the grid, so neither changes a
//! tile's rule. A pan moves the squares by whole tiles, when the look-at
//! point crosses a tile edge. A band's tiles are every tile of its zoom its
//! ground meets, so nothing outside the footprint is placed, and a coarse
//! tile is placed for the part of it that is in view. Where a coarse tile of
//! a farther band reaches under a nearer band, the nearer band's finer tiles
//! are drawn over it by the tile-priority stencil. Beyond the last rule's
//! square nothing is placed by the rules.

use std::collections::HashSet;

use crate::polygon::{CoveragePolygon, footprint};
use crate::rules::{FlatRingBand, FlatRingRule, normalized};
use crate::{FlatRenderState, Target, VisibleTile};

/// How far a band's ground is drawn in from its squares' sides, in target
/// tiles: well over the polygon test's tolerance, far under anything a tile
/// could lie in.
pub const EDGE_INSET_IN_TARGET_TILES: f64 = 1e-3;

/// A rectangle of the flat render world along the tile grid's axes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Square {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

/// What the flat coverage resolved.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FlatCoverage {
    /// Finest first, then by world copy, column and row. A tile two bands
    /// share takes the nearer band's switches.
    pub targets: Vec<Target>,
    /// Nearest first, one per rule.
    pub bands: Vec<FlatRingBand>,
    /// How many tiles the enumeration looked at, for the diagnostics.
    pub visited: usize,
}

/// What a frame's flat coverage is resolved from.
#[derive(Debug, Clone, Copy)]
pub struct FlatCoverageInput<'a> {
    /// The frame's projection-view matrix, column-major.
    pub camera_matrix: &'a [f64; 16],
    /// How far the ground relief falls and rises about the plane, in world units, `None` off the relief.
    pub relief_heights: Option<(f64, f64)>,
    /// The eye's x and y in render world units, which the texture bands' footprint is pushed out from.
    pub eye: [f64; 2],
    /// How far past the visible ground the texture bands are placed, as a share of the footprint, 0 for none.
    pub raster_band_margin_share: f64,
    pub flat: FlatRenderState,
    pub target_zoom: i32,
    /// The zoom no band goes down to (the bands stop one above it), `None` for none.
    pub backdrop_zoom: Option<i32>,
    pub rules: &'a [FlatRingRule],
}

/// The frame's coverage on the plane, `None` for a camera with no footprint.
pub fn coverage(input: &FlatCoverageInput<'_>) -> Option<FlatCoverage> {
    let polygon = footprint(input.camera_matrix, input.relief_heights)?;
    let raster_polygon = (input.raster_band_margin_share > 0.0).then(|| polygon.inflated(input.eye[0], input.eye[1], 1.0 + input.raster_band_margin_share));
    Some(resolve(&input.flat, input.target_zoom, input.backdrop_zoom, input.rules, &polygon, raster_polygon.as_ref()))
}

/// The coverage of a footprint. `raster_polygon` is the footprint the
/// texture bands are placed over, `None` to place them over `polygon` like
/// the rest.
pub fn resolve(flat: &FlatRenderState, target_zoom: i32, backdrop_zoom: Option<i32>, rules: &[FlatRingRule], polygon: &CoveragePolygon, raster_polygon: Option<&CoveragePolygon>) -> FlatCoverage {
    if target_zoom < 0 {
        return FlatCoverage::default();
    }
    let floor_zoom = backdrop_zoom.map_or(0, |zoom| zoom + 1).min(target_zoom);
    let mut placed: HashSet<VisibleTile> = HashSet::new();
    let mut targets: Vec<Target> = Vec::new();
    let mut bands: Vec<FlatRingBand> = Vec::new();
    let mut visited = 0usize;
    let mut inner_square: Option<Square> = None;
    let inset = (flat.render_map_size / 2f64.powi(target_zoom)) * EDGE_INSET_IN_TARGET_TILES;
    let mut band_tiles: Vec<VisibleTile> = Vec::new();
    for rule in normalized(rules) {
        let zoom = floor_zoom.max(target_zoom - rule.zoom_drop);
        let square = square(rule.distance, target_zoom, flat);
        let footprint = if rule.raster_size.is_some() { raster_polygon.unwrap_or(polygon) } else { polygon };
        band_tiles.clear();
        for ground in band_polygons(footprint, square, inner_square, inset) {
            tiles(zoom, &ground, flat, &mut band_tiles, &mut visited);
        }
        // The pieces of a band overlap along their edges: a tile is one of the band's once.
        let mut in_band: HashSet<VisibleTile> = HashSet::with_capacity(band_tiles.len());
        for &tile in &band_tiles {
            if !in_band.insert(tile) {
                continue;
            }
            if placed.insert(tile) {
                targets.push(Target { tile, lines: rule.draws_lines, labels: rule.draws_labels, raster_size: rule.raster_size });
            }
        }
        bands.push(FlatRingBand { zoom, distance: rule.distance, tile_count: in_band.len(), terrain_zoom: (zoom - rule.terrain_zoom_drop).max(0) });
        inner_square = Some(square);
    }
    // Renderer-stable order: finest first, then by world copy and position.
    targets.sort_by(|lhs, rhs| rhs.tile.z.cmp(&lhs.tile.z).then(lhs.tile.wrap.cmp(&rhs.tile.wrap)).then(lhs.tile.x.cmp(&rhs.tile.x)).then(lhs.tile.y.cmp(&rhs.tile.y)));
    FlatCoverage { targets, bands, visited }
}

/// The target zoom's tile under the look-at point, which is the render
/// world's origin: its column and row in the world copy the pan keeps under
/// the camera.
pub fn look_at_tile(target_zoom: i32, flat: &FlatRenderState) -> (i32, i32) {
    let tiles_count = 2f64.powi(target_zoom);
    let half = flat.render_map_size * 0.5;
    let tile_size = flat.render_map_size / tiles_count;
    // The inverse of `FlatRenderState::tile_origin_and_size` at the origin.
    // The row index grows south while the world is y up.
    let column = ((half - flat.pan[0] * half) / tile_size).floor();
    let row_from_south = ((half + flat.pan[1] * half) / tile_size).floor();
    let clamp = |value: f64| value.max(0.0).min(tiles_count - 1.0);
    (clamp(column) as i32, (tiles_count - 1.0 - clamp(row_from_south)) as i32)
}

/// The look-at tile and `rings` rings of target tiles around it, in world units.
pub fn square(rings: f64, target_zoom: i32, flat: &FlatRenderState) -> Square {
    let (x, y) = look_at_tile(target_zoom, flat);
    let origin = flat.tile_origin_and_size(x, y, target_zoom, 0);
    let size = flat.render_map_size / 2f64.powi(target_zoom);
    let reach = rings * size;
    Square { min_x: origin[0] - reach, min_y: origin[1] - reach, max_x: origin[0] + size + reach, max_y: origin[1] + size + reach }
}

/// The footprint inside `square` and outside `inner_square`: one convex
/// piece with no inner square, otherwise the frame's four strips (the two
/// full-height sides, then the top and the bottom between them), those of
/// them the footprint reaches. Every piece is drawn in by `inset`, so a tile
/// that only touches the band along an edge is not one of its tiles.
pub fn band_polygons(polygon: &CoveragePolygon, square: Square, inner_square: Option<Square>, inset: f64) -> Vec<CoveragePolygon> {
    let outer = Square { min_x: square.min_x + inset, min_y: square.min_y + inset, max_x: square.max_x - inset, max_y: square.max_y - inset };
    let Some(inner_square) = inner_square else {
        return clipped(polygon, outer).into_iter().collect();
    };
    let inner = Square { min_x: inner_square.min_x - inset, min_y: inner_square.min_y - inset, max_x: inner_square.max_x + inset, max_y: inner_square.max_y + inset };
    if !(inner.min_x > outer.min_x || inner.max_x < outer.max_x || inner.min_y > outer.min_y || inner.max_y < outer.max_y) {
        return Vec::new();
    }
    let strips = [
        Square { min_x: outer.min_x, min_y: outer.min_y, max_x: inner.min_x, max_y: outer.max_y },
        Square { min_x: inner.max_x, min_y: outer.min_y, max_x: outer.max_x, max_y: outer.max_y },
        Square { min_x: inner.min_x, min_y: inner.max_y, max_x: inner.max_x, max_y: outer.max_y },
        Square { min_x: inner.min_x, min_y: outer.min_y, max_x: inner.max_x, max_y: inner.min_y },
    ];
    strips.into_iter().filter_map(|strip| clipped(polygon, strip)).collect()
}

/// The polygon's part inside the rectangle, `None` when there is none.
pub fn clipped(polygon: &CoveragePolygon, square: Square) -> Option<CoveragePolygon> {
    if !(square.max_x > square.min_x && square.max_y > square.min_y) {
        return None;
    }
    let centre = [(square.min_x + square.max_x) / 2.0, (square.min_y + square.max_y) / 2.0];
    let sides = [
        ([square.min_x, centre[1]], [square.min_x, centre[1] + 1.0]),
        ([square.max_x, centre[1]], [square.max_x, centre[1] + 1.0]),
        ([centre[0], square.min_y], [centre[0] + 1.0, square.min_y]),
        ([centre[0], square.max_y], [centre[0] + 1.0, square.max_y]),
    ];
    let mut result = polygon.clipped(centre, sides[0].0, sides[0].1)?;
    for (a, b) in &sides[1..] {
        result = result.clipped(centre, *a, *b)?;
    }
    Some(result)
}

/// Appends the tiles at `zoom` the polygon meets, every world copy, by world
/// copy, column and row, and adds the tiles looked at to `visited`.
pub fn tiles(zoom: i32, polygon: &CoveragePolygon, flat: &FlatRenderState, out: &mut Vec<VisibleTile>, visited: &mut usize) {
    if zoom < 0 {
        return;
    }
    let start = out.len();
    for wrap in [-1, 0, 1] {
        visit(0, 0, 0, wrap, zoom, polygon, flat, out, visited);
    }
    out[start..].sort_by(|lhs, rhs| lhs.wrap.cmp(&rhs.wrap).then(lhs.x.cmp(&rhs.x)).then(lhs.y.cmp(&rhs.y)));
}

#[allow(clippy::too_many_arguments)]
fn visit(x: i32, y: i32, z: i32, wrap: i32, zoom: i32, polygon: &CoveragePolygon, flat: &FlatRenderState, out: &mut Vec<VisibleTile>, visited: &mut usize) {
    *visited += 1;
    let origin = flat.tile_origin_and_size(x, y, z, wrap);
    if !polygon.intersects(origin[0], origin[1], origin[0] + origin[2], origin[1] + origin[2]) {
        return;
    }
    if z == zoom {
        out.push(VisibleTile { z, x, y, wrap });
        return;
    }
    let (child_x, child_y, child_z) = (x * 2, y * 2, z + 1);
    visit(child_x, child_y, child_z, wrap, zoom, polygon, flat, out, visited);
    visit(child_x + 1, child_y, child_z, wrap, zoom, polygon, flat, out, visited);
    visit(child_x, child_y + 1, child_z, wrap, zoom, polygon, flat, out, visited);
    visit(child_x + 1, child_y + 1, child_z, wrap, zoom, polygon, flat, out, visited);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::default_rules;

    /// A camera looking straight down whose frustum cuts the plane in the square of ±100 about the origin.
    const DOWN: [f64; 16] = [0.01, 0.0, 0.0, 0.0, 0.0, 0.01, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.5, 1.0];
    const FLAT: FlatRenderState = FlatRenderState { pan: [0.0, 0.0], render_map_size: 1024.0 };

    fn input<'a>(rules: &'a [FlatRingRule]) -> FlatCoverageInput<'a> {
        FlatCoverageInput { camera_matrix: &DOWN, relief_heights: None, eye: [0.0, 0.0], raster_band_margin_share: 0.0, flat: FLAT, target_zoom: 3, backdrop_zoom: Some(0), rules }
    }

    #[test]
    fn the_look_at_tile_is_the_one_north_east_of_the_origin() {
        assert_eq!(look_at_tile(3, &FLAT), (4, 3));
        let square = square(1.0, 3, &FLAT);
        assert_eq!((square.min_x, square.min_y, square.max_x, square.max_y), (-128.0, -128.0, 256.0, 256.0));
    }

    #[test]
    fn the_tiles_under_the_footprint_are_placed_at_the_target_zoom() {
        let rules = default_rules();
        let resolved = coverage(&input(&rules)).expect("a footprint");
        let tiles: Vec<(i32, i32, i32, i32)> = resolved.targets.iter().map(|t| (t.tile.z, t.tile.x, t.tile.y, t.tile.wrap)).collect();
        assert_eq!(tiles, [(3, 3, 3, 0), (3, 3, 4, 0), (3, 4, 3, 0), (3, 4, 4, 0)]);
        assert!(resolved.targets.iter().all(|t| t.lines && t.labels && t.raster_size.is_none()));
        // One band per rule, and the footprint does not reach past the first rule's square.
        assert_eq!(resolved.bands.iter().map(|b| b.tile_count).collect::<Vec<_>>(), [4, 0, 0, 0]);
        assert_eq!(resolved.bands.iter().map(|b| b.zoom).collect::<Vec<_>>(), [3, 2, 1, 1]);
    }

    #[test]
    fn a_farther_band_is_drawn_coarser_with_its_rules_switches() {
        // No ring about the look-at tile, so the footprint's other three quarters
        // fall to the second rule. The coarse tile over the look-at tile is not
        // placed: all the footprint holds of it lies under the nearer band.
        let rules = [FlatRingRule::new(0, 0.0), FlatRingRule { zoom_drop: 1, distance: 2.0, draws_lines: false, draws_labels: true, raster_size: Some(100), terrain_zoom_drop: 0 }];
        let resolved = coverage(&input(&rules)).expect("a footprint");
        let near: Vec<&Target> = resolved.targets.iter().filter(|t| t.tile.z == 3).collect();
        assert_eq!(near.len(), 1);
        assert_eq!((near[0].tile.x, near[0].tile.y), (4, 3));
        let far: Vec<&Target> = resolved.targets.iter().filter(|t| t.tile.z == 2).collect();
        assert_eq!(far.iter().map(|t| (t.tile.x, t.tile.y)).collect::<Vec<_>>(), [(1, 1), (1, 2), (2, 2)]);
        // The size is the nearest the textures are baked at.
        assert!(far.iter().all(|t| !t.lines && t.labels && t.raster_size == Some(128)));
        // Finest first.
        assert!(resolved.targets.windows(2).all(|pair| pair[0].tile.z >= pair[1].tile.z));
    }

    #[test]
    fn the_relief_widens_the_footprint() {
        let plane = footprint(&DOWN, None).expect("a footprint");
        assert_eq!(plane.vertices().len(), 4);
        assert!(plane.intersects(99.0, 99.0, 150.0, 150.0));
        assert!(!plane.intersects(101.0, 101.0, 150.0, 150.0));
        // Straight down the slab's cut is the plane's, so the hull keeps the square.
        let slab = footprint(&DOWN, Some((-0.2, 0.3))).expect("a footprint");
        assert!(slab.intersects(99.0, 99.0, 150.0, 150.0));
        assert!(!slab.intersects(101.0, 101.0, 150.0, 150.0));
    }

    #[test]
    fn a_camera_without_an_inverse_covers_nothing() {
        let singular = [0.0; 16];
        let rules = default_rules();
        assert!(coverage(&FlatCoverageInput { camera_matrix: &singular, ..input(&rules) }).is_none());
    }

    #[test]
    fn the_rules_are_read_nearest_first_one_per_distance() {
        let rules = [FlatRingRule::new(9, 300.0), FlatRingRule::new(1, 2.0), FlatRingRule::new(3, 2.0)];
        let cleaned = normalized(&rules);
        assert_eq!(cleaned.iter().map(|r| (r.zoom_drop, r.distance)).collect::<Vec<_>>(), [(1, 2.0), (8, 256.0)]);
        assert_eq!(normalized(&[]), default_rules());
    }
}
