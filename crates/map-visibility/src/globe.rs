// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The sphere's coverage: the plane's ring rules read off the sphere's grid.
//! The walk descends from the root with the frustum and horizon rejects,
//! which stand where the plane has its frustum footprint, and at every tile
//! the rings of its nearest and farthest target tiles from the look-at tile
//! say which rules' bands it meets. A tile is placed when a band it meets is
//! drawn at its zoom, and looked into where a band it meets is drawn finer.
//! Overlapping placements are fine, as on the plane: the sphere draws its
//! sources through the same tile-priority stencil.
//!
//! One thing differs from the plane. Beyond the last rule the plane places
//! nothing and leaves the ground to the haze. The sphere has no haze, so
//! ground past the last rule is asked for at the pinned world cover's zoom
//! (`FLOOR_ZOOM`, always resident, nothing to load), and no band goes below
//! that cover either.

use std::collections::{HashMap, HashSet};
use std::f64::consts::PI;

use crate::frustum::Frustum;
use crate::rules::{FlatRingBand, FlatRingRule, normalized};
use crate::{Target, VisibleTile};

/// The zoom of the pinned world cover, which a host keeps resident: the
/// floor of every placement on the sphere and what the ground past the last
/// rule is asked for.
pub const FLOOR_ZOOM: i32 = 0;
const TRANSITION_LOW_ZOOM_FALLBACK_LIMIT: i32 = 3;

/// The latitude, in radians, where Web Mercator's square world ends.
pub fn max_mercator_latitude() -> f64 {
    2.0 * PI.exp().atan() - PI * 0.5
}

fn wrap_normalized_world_x(x: f64) -> f64 {
    let wrapped = x % 1.0;
    if wrapped < 0.0 { wrapped + 1.0 } else { wrapped }
}

fn latitude_from_normalized_world_y(y: f64) -> f64 {
    let mercator_y = (1.0 - 2.0 * y.max(0.0).min(1.0)) * PI;
    2.0 * mercator_y.exp().atan() - PI * 0.5
}

fn longitude_from_normalized_world_x(x: f64) -> f64 {
    wrap_normalized_world_x(x) * (2.0 * PI) - PI
}

/// The globe as a frame draws it: the pan as the globe reads it, the render
/// radius, and the geometry phase of the unroll into the plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlobeUniform {
    pub pan_x: f64,
    pub pan_y: f64,
    pub radius: f64,
    pub transition: f64,
}

/// A column and row of the target zoom's grid.
pub type GridCell = (i64, i64);

/// The point the camera looks at in fractional tiles of the target zoom (x
/// east, y south), from the normalized world Mercator centre.
pub fn center_in_tiles(center_world_mercator: [f64; 2], target_zoom: i32) -> [f64; 2] {
    let tiles_count = 2f64.powi(target_zoom);
    [wrap_normalized_world_x(center_world_mercator[0]) * tiles_count, center_world_mercator[1].max(0.0).min(1.0) * tiles_count]
}

/// The target zoom's tile under the point the camera looks at, which the
/// globe's pan brings to the sphere's front point.
pub fn look_at_tile(center_in_tiles: [f64; 2], target_zoom: i32) -> GridCell {
    let tiles_count = 2f64.powi(target_zoom.max(0));
    let clamp = |value: f64| value.floor().max(0.0).min(tiles_count - 1.0) as i64;
    (clamp(center_in_tiles[0]), clamp(center_in_tiles[1]))
}

/// The rings of the nearest and the farthest target tile under a tile at or
/// above the target zoom. The count is the plane's (ring 0 the look-at tile
/// alone, ring 1 the three by three around it), with the two things the
/// sphere's grid does that the plane's does not: the columns close on
/// themselves at the antimeridian, so a column's distance is the shorter way
/// around, and the rows end at the poles, so a ring that runs off the last
/// row is cut there.
pub fn ring_range(tile: (i32, i32, i32), target_zoom: i32, look_at: GridCell) -> (i64, i64) {
    let (x, y, z) = tile;
    let shift = (target_zoom - z).max(0);
    let tiles_count = 1i64 << target_zoom.max(0);
    let span = 1i64 << shift;
    let rows = axis_range(y as i64 * span, span, look_at.1);
    let columns = wrapped_axis_range(x as i64 * span, span, look_at.0, tiles_count);
    (rows.0.max(columns.0), rows.1.max(columns.1))
}

/// The distances from `look_at` to the nearest and the farthest of `count` cells starting at `first`, on an axis with ends.
pub fn axis_range(first: i64, count: i64, look_at: i64) -> (i64, i64) {
    let last = first + count - 1;
    let nearest = if look_at < first { first - look_at } else if look_at > last { look_at - last } else { 0 };
    (nearest, (look_at - first).abs().max((look_at - last).abs()))
}

/// The same on an axis of `tiles_count` cells that closes on itself: a cell's distance is the shorter way around.
pub fn wrapped_axis_range(first: i64, count: i64, look_at: i64, tiles_count: i64) -> (i64, i64) {
    if count >= tiles_count {
        return (0, tiles_count / 2);
    }
    // The look-at cell's offset past the block's first cell, going up the
    // axis. Inside the block it is under `count`.
    let offset = (look_at - first).rem_euclid(tiles_count);
    if offset < count {
        return (0, offset.max(count - 1 - offset));
    }
    // Outside: going up the axis the block's cells are `nearest_up` to
    // `offset` steps behind the look-at cell, and a cell that many steps
    // behind is the shorter of that and the rest of the way around.
    let nearest_up = offset - (count - 1);
    let nearest = nearest_up.min(tiles_count - offset);
    // The farthest is the cell nearest the antipode: half way around when
    // the antipode is inside the block, otherwise the block's end on the
    // antipode's side.
    let half = tiles_count / 2;
    let farthest = if nearest_up <= half && half <= offset {
        half
    } else if offset < half {
        offset
    } else {
        tiles_count - nearest_up
    };
    (nearest, farthest)
}

/// A tile's bounding sphere on the unit sphere before the pan: its centre's direction and its radius over the globe's.
#[derive(Debug, Clone, Copy, PartialEq)]
struct StaticBound {
    center_direction: [f64; 3],
    radius_scale: f64,
}

fn unit_sphere_direction(latitude: f64, longitude: f64) -> [f64; 3] {
    let phi = latitude - PI * 0.5;
    let theta = longitude + PI;
    let (x, y, z) = (phi.sin() * theta.sin(), phi.cos(), phi.sin() * theta.cos());
    let inverse_length = 1.0 / (x * x + y * y + z * z).sqrt();
    [x * inverse_length, y * inverse_length, z * inverse_length]
}

fn angular_distance(lhs: [f64; 3], rhs: [f64; 3]) -> f64 {
    (lhs[0] * rhs[0] + lhs[1] * rhs[1] + lhs[2] * rhs[2]).max(-1.0).min(1.0).acos()
}

fn unwrap_longitude_midpoint(west: f64, east: f64) -> f64 {
    let east_unwrapped = if east >= west { east } else { east + 2.0 * PI };
    let midpoint = 0.5 * (west + east_unwrapped);
    if midpoint > PI { midpoint - 2.0 * PI } else { midpoint }
}

fn static_bound(x: i32, y: i32, z: i32) -> StaticBound {
    let scale = 2f64.powi(z);
    let north = latitude_from_normalized_world_y(y as f64 / scale);
    let south = latitude_from_normalized_world_y((y as f64 + 1.0) / scale);
    let west = longitude_from_normalized_world_x(x as f64 / scale);
    let east = longitude_from_normalized_world_x((x as f64 + 1.0) / scale);
    let center_direction = unit_sphere_direction(0.5 * (north + south), unwrap_longitude_midpoint(west, east));
    if z == 0 {
        return StaticBound { center_direction, radius_scale: 2.0 };
    }
    let mut max_angle = 0f64;
    for corner in [unit_sphere_direction(north, west), unit_sphere_direction(north, east), unit_sphere_direction(south, west), unit_sphere_direction(south, east)] {
        max_angle = max_angle.max(angular_distance(center_direction, corner));
    }
    StaticBound { center_direction, radius_scale: 2.0 * (max_angle * 0.5).sin() }
}

/// A tile's number in the quadtree, counted zoom by zoom from the root.
fn tile_index(x: i32, y: i32, z: i32) -> u64 {
    let side = 1u64 << z;
    (side * side - 1) / 3 + y as u64 * side + x as u64
}

/// Every visited tile's bounding sphere on the unit sphere, made once: the
/// bounds depend on the tile alone, so a host keeps one of these for good.
#[derive(Debug, Default)]
pub struct GlobeBounds {
    cache: HashMap<u64, StaticBound>,
}

/// What the visibility tests read, taken off the frame once.
struct Visibility {
    radius: f64,
    globe_center: [f64; 3],
    transition: f64,
    /// The pan's rotation as the three columns a direction is dotted with.
    rotation: [f64; 9],
    globe_to_camera: [f64; 3],
    globe_to_camera_length: f64,
    horizon_threshold: f64,
}

impl Visibility {
    fn new(globe: &GlobeUniform, eye: [f64; 3]) -> Visibility {
        let pan_latitude = globe.pan_y * max_mercator_latitude();
        let pan_longitude = globe.pan_x * PI;
        let (cx, sx) = ((-pan_latitude).cos(), (-pan_latitude).sin());
        let (cy, sy) = ((-pan_longitude).cos(), (-pan_longitude).sin());
        let globe_center = [0.0, 0.0, -globe.radius];
        let globe_to_camera = [eye[0] - globe_center[0], eye[1] - globe_center[1], eye[2] - globe_center[2]];
        Visibility {
            radius: globe.radius,
            globe_center,
            transition: globe.transition,
            rotation: [cy, 0.0, -sy, sy * sx, cx, cy * sx, sy * cx, -sx, cy * cx],
            globe_to_camera,
            globe_to_camera_length: (globe_to_camera[0] * globe_to_camera[0] + globe_to_camera[1] * globe_to_camera[1] + globe_to_camera[2] * globe_to_camera[2]).sqrt(),
            horizon_threshold: globe.radius * globe.radius,
        }
    }

    /// The tile's bounding sphere in the camera's world: its centre and radius.
    fn tile_bound(&self, bound: &StaticBound) -> ([f64; 3], f64) {
        let [dx, dy, dz] = bound.center_direction;
        let r = &self.rotation;
        let rotated = [dx * r[0] + dy * r[1] + dz * r[2], dx * r[3] + dy * r[4] + dz * r[5], dx * r[6] + dy * r[7] + dz * r[8]];
        let c = self.globe_center;
        ([c[0] + rotated[0] * self.radius, c[1] + rotated[1] * self.radius, c[2] + rotated[2] * self.radius], self.radius * bound.radius_scale)
    }

    fn horizon_reject_enabled(&self) -> bool {
        self.globe_to_camera_length > 0.0 && self.transition <= 0.0
    }

    fn horizon_dot(&self, center: [f64; 3]) -> f64 {
        let (c, g) = (self.globe_center, self.globe_to_camera);
        (center[0] - c[0]) * g[0] + (center[1] - c[1]) * g[1] + (center[2] - c[2]) * g[2]
    }

    fn may_pass_horizon(&self, center: [f64; 3], radius: f64) -> bool {
        !self.horizon_reject_enabled() || self.horizon_dot(center) + radius * self.globe_to_camera_length >= self.horizon_threshold
    }

    fn passes_horizon_entirely(&self, center: [f64; 3], radius: f64) -> bool {
        !self.horizon_reject_enabled() || self.horizon_dot(center) - radius * self.globe_to_camera_length >= self.horizon_threshold
    }
}

/// What the walk did, for the diagnostics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GlobeMetrics {
    pub visited_nodes: usize,
    pub frustum_rejects: usize,
    pub horizon_rejects: usize,
    pub placed_tiles: usize,
    pub accepted_whole_subtrees: usize,
}

/// What the sphere's coverage resolved.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GlobeCoverage {
    /// Finest first, then by column and row, all in the world copy 0. A tile
    /// two bands ask for takes the nearer band's switches, and the cover past
    /// the last rule the last rule's.
    pub targets: Vec<Target>,
    /// One band per rule, nearest first, as on the plane.
    pub bands: Vec<FlatRingBand>,
    pub metrics: GlobeMetrics,
}

/// What a frame's coverage of the sphere is resolved from: the eye in world
/// units (the camera looks at the world origin, the sphere's front point,
/// and the pan turns the sphere under it), the globe, the ring rules, and
/// the target zoom's tile the camera looks at, which the rings are counted
/// from.
#[derive(Debug, Clone, Copy)]
pub struct GlobeCoverageInput<'a> {
    pub eye: [f64; 3],
    pub globe: GlobeUniform,
    pub rules: &'a [FlatRingRule],
    pub look_at: GridCell,
    pub target_zoom: i32,
    pub frustum: &'a Frustum,
}

/// One rule as the walk reads it: the rings it owns and the zoom it draws them at.
struct Band {
    first_ring: f64,
    last_ring: f64,
    zoom: i32,
    draws_lines: bool,
    draws_labels: bool,
    raster_size: Option<u32>,
    tile_count: usize,
}

struct Walk<'a> {
    target_zoom: i32,
    frustum: &'a Frustum,
    visibility: Visibility,
    look_at: GridCell,
    bands: Vec<Band>,
    targets: Vec<Target>,
    placed: HashSet<u64>,
    metrics: GlobeMetrics,
    bounds: &'a mut GlobeBounds,
}

enum Evaluation {
    RejectFrustum,
    RejectHorizon,
    Descend,
    AcceptWholeSubtree,
}

impl Walk<'_> {
    fn evaluate(&mut self, x: i32, y: i32, z: i32) -> Evaluation {
        let minimum_reject_zoom = self.target_zoom.min(if self.visibility.transition > 0.0 { 4 } else { 3 });
        let rejects = z >= minimum_reject_zoom;
        let whole_subtree = z < self.target_zoom;
        if !rejects && !whole_subtree {
            return Evaluation::Descend;
        }
        let bound = *self.bounds.cache.entry(tile_index(x, y, z)).or_insert_with(|| static_bound(x, y, z));
        let (center, radius) = self.visibility.tile_bound(&bound);
        if rejects && !self.frustum.is_sphere_visible(center, radius) {
            return Evaluation::RejectFrustum;
        }
        if rejects && !self.visibility.may_pass_horizon(center, radius) {
            return Evaluation::RejectHorizon;
        }
        if whole_subtree && self.frustum.contains_sphere(center, radius) && self.visibility.passes_horizon_entirely(center, radius) {
            return Evaluation::AcceptWholeSubtree;
        }
        Evaluation::Descend
    }

    fn place(&mut self, x: i32, y: i32, z: i32, lines: bool, labels: bool, raster_size: Option<u32>) {
        if self.placed.insert(tile_index(x, y, z)) {
            self.targets.push(Target { tile: VisibleTile { z, x, y, wrap: 0 }, lines, labels, raster_size });
            self.metrics.placed_tiles += 1;
        }
    }

    /// Through the morph at the coarse zooms every leaf is taken: the visibility tests do not hold there.
    fn accept_leaf_descendants(&mut self, x: i32, y: i32, z: i32) {
        if z == self.target_zoom {
            self.place(x, y, z, true, true, None);
            return;
        }
        for (cx, cy) in children(x, y) {
            self.accept_leaf_descendants(cx, cy, z + 1);
        }
    }

    /// `accepted_above`: the tile lies in a subtree the visibility tests accepted whole, so they are not asked again below it.
    fn visit(&mut self, x: i32, y: i32, z: i32, accepted_above: bool) {
        self.metrics.visited_nodes += 1;
        if self.visibility.transition > 0.0 && self.target_zoom <= TRANSITION_LOW_ZOOM_FALLBACK_LIMIT {
            self.accept_leaf_descendants(x, y, z);
            return;
        }
        let mut accepted = accepted_above;
        if !accepted {
            match self.evaluate(x, y, z) {
                Evaluation::RejectFrustum => {
                    self.metrics.frustum_rejects += 1;
                    return;
                }
                Evaluation::RejectHorizon => {
                    self.metrics.horizon_rejects += 1;
                    return;
                }
                Evaluation::AcceptWholeSubtree => {
                    self.metrics.accepted_whole_subtrees += 1;
                    accepted = true;
                }
                Evaluation::Descend => {}
            }
        }
        // The bands the tile meets: a rule's rings against the rings of the
        // tile's nearest and farthest target tiles. No order of the zooms
        // along the rings is assumed, so any rule list holds.
        let (nearest, farthest) = ring_range((x, y, z), self.target_zoom, self.look_at);
        let (nearest, farthest) = (nearest as f64, farthest as f64);
        let already_placed = self.placed.contains(&tile_index(x, y, z));
        let mut places: Option<(bool, bool, Option<u32>)> = None;
        let mut finest_wanted = FLOOR_ZOOM;
        for band in &mut self.bands {
            if !(band.first_ring <= farthest && band.last_ring >= nearest) {
                continue;
            }
            finest_wanted = finest_wanted.max(band.zoom);
            if band.zoom == z && places.is_none() {
                places = Some((band.draws_lines, band.draws_labels, band.raster_size));
                if !already_placed {
                    band.tile_count += 1;
                }
            }
        }
        // Past the last rule the pinned cover paints the ground.
        if places.is_none() && z == FLOOR_ZOOM {
            let last = self.bands.last();
            if farthest > last.map_or(-1.0, |band| band.last_ring) {
                places = Some(last.map_or((true, true, None), |band| (band.draws_lines, band.draws_labels, band.raster_size)));
            }
        }
        if let Some((lines, labels, raster_size)) = places {
            self.place(x, y, z, lines, labels, raster_size);
        }
        if z < finest_wanted.min(self.target_zoom) {
            for (cx, cy) in children(x, y) {
                self.visit(cx, cy, z + 1, accepted);
            }
        }
    }
}

fn children(x: i32, y: i32) -> [(i32, i32); 4] {
    [(x * 2, y * 2), (x * 2 + 1, y * 2), (x * 2, y * 2 + 1), (x * 2 + 1, y * 2 + 1)]
}

/// The frame's coverage on the sphere.
pub fn coverage(input: &GlobeCoverageInput<'_>, bounds: &mut GlobeBounds) -> GlobeCoverage {
    if input.target_zoom < 0 {
        return GlobeCoverage::default();
    }
    let mut bands: Vec<Band> = Vec::new();
    for rule in normalized(input.rules) {
        let first_ring = bands.last().map_or(0.0, |previous| previous.last_ring + 1.0);
        bands.push(Band {
            first_ring,
            last_ring: rule.distance,
            zoom: input.target_zoom.min(FLOOR_ZOOM.max(input.target_zoom - rule.zoom_drop)),
            draws_lines: rule.draws_lines,
            draws_labels: rule.draws_labels,
            raster_size: rule.raster_size,
            tile_count: 0,
        });
    }
    let mut walk = Walk {
        target_zoom: input.target_zoom,
        frustum: input.frustum,
        visibility: Visibility::new(&input.globe, input.eye),
        look_at: input.look_at,
        bands,
        targets: Vec::new(),
        placed: HashSet::new(),
        metrics: GlobeMetrics::default(),
        bounds,
    };
    walk.visit(0, 0, 0, false);
    let Walk { mut targets, bands, metrics, .. } = walk;
    targets.sort_by(|lhs, rhs| rhs.tile.z.cmp(&lhs.tile.z).then(lhs.tile.x.cmp(&rhs.tile.x)).then(lhs.tile.y.cmp(&rhs.tile.y)));
    GlobeCoverage { targets, bands: bands.iter().map(|band| FlatRingBand { zoom: band.zoom, distance: band.last_ring, tile_count: band.tile_count, terrain_zoom: band.zoom }).collect(), metrics }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::default_rules;

    #[test]
    fn an_axis_with_ends_counts_from_the_nearest_and_the_farthest_cell() {
        assert_eq!(axis_range(4, 2, 4), (0, 1));
        assert_eq!(axis_range(4, 2, 1), (3, 4));
        assert_eq!(axis_range(4, 2, 9), (4, 5));
    }

    #[test]
    fn a_closed_axis_takes_the_shorter_way_around() {
        // Sixteen columns: the block of columns 14 and 15 is one step behind column 0.
        assert_eq!(wrapped_axis_range(14, 2, 0, 16), (1, 2));
        // The antipode of column 0 is column 8, inside the block 8 to 11.
        assert_eq!(wrapped_axis_range(8, 4, 0, 16), (5, 8));
        // The look-at cell inside the block.
        assert_eq!(wrapped_axis_range(4, 4, 5, 16), (0, 2));
        // A block that is the whole axis reaches half way around.
        assert_eq!(wrapped_axis_range(0, 16, 3, 16), (0, 8));
    }

    #[test]
    fn a_tile_above_the_target_zoom_spans_the_rings_of_its_target_tiles() {
        // The root holds the look-at tile and the cell farthest from it.
        assert_eq!(ring_range((0, 0, 0), 3, (1, 1)), (0, 6));
        // A target tile is one ring.
        assert_eq!(ring_range((3, 1, 3), 3, (1, 1)), (2, 2));
        // The columns wrap, the rows do not.
        assert_eq!(ring_range((7, 7, 3), 3, (0, 0)), (7, 7));
        assert_eq!(ring_range((7, 0, 3), 3, (0, 0)), (1, 1));
    }

    #[test]
    fn the_look_at_tile_is_clamped_to_the_grid() {
        assert_eq!(look_at_tile(center_in_tiles([0.5, 0.5], 3), 3), (4, 4));
        assert_eq!(look_at_tile(center_in_tiles([1.25, 2.0], 2), 2), (1, 3));
    }

    /// A camera on the z axis, `distance` from the globe's front point, looking at it down the axis.
    fn looking_at_the_front(distance: f64) -> ([f64; 16], [f64; 3]) {
        // Perspective with a 0 to 1 depth, 90 degrees of view, near 0.01, far 100, the eye at (0, 0, distance).
        let (near, far) = (0.01, 100.0);
        let depth = far / (near - far);
        ([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, depth, -1.0, 0.0, 0.0, depth * near - depth * distance, distance], [0.0, 0.0, distance])
    }

    #[test]
    fn the_ground_past_the_last_rule_is_the_world_cover() {
        let (matrix, eye) = looking_at_the_front(1.5);
        let frustum = Frustum::new(&matrix);
        let rules = default_rules();
        let globe = GlobeUniform { pan_x: 0.0, pan_y: 0.0, radius: 1.0, transition: 0.0 };
        let target_zoom = 4;
        let look_at = look_at_tile(center_in_tiles([0.5, 0.5], target_zoom), target_zoom);
        let input = GlobeCoverageInput { eye, globe, rules: &rules, look_at, target_zoom, frustum: &frustum };
        let mut bounds = GlobeBounds::default();
        let resolved = coverage(&input, &mut bounds);
        // The look-at tile and its ring are placed at the target zoom.
        let finest: Vec<(i32, i32)> = resolved.targets.iter().filter(|t| t.tile.z == 4).map(|t| (t.tile.x, t.tile.y)).collect();
        assert_eq!(finest.len(), 9);
        assert!(finest.contains(&(8, 8)));
        // Each farther band is drawn coarser, about the look-at tile, and the
        // far side of the globe is left to the world cover alone.
        assert_eq!(resolved.targets.iter().filter(|t| t.tile.z == 3).count(), 8);
        assert!(resolved.targets.iter().filter(|t| t.tile.z == 2).all(|t| (1..=2).contains(&t.tile.x) && (1..=2).contains(&t.tile.y)));
        assert_eq!(resolved.targets.iter().filter(|t| t.tile.z == 0).count(), 1);
        assert!(resolved.targets.iter().all(|t| t.tile.z != 1));
        // Finest first, nothing twice.
        assert!(resolved.targets.windows(2).all(|pair| pair[0].tile.z >= pair[1].tile.z));
        let unique: HashSet<VisibleTile> = resolved.targets.iter().map(|t| t.tile).collect();
        assert_eq!(unique.len(), resolved.targets.len());
        assert_eq!(resolved.metrics.placed_tiles, resolved.targets.len());
        // The same frame resolves the same way from the kept bounds.
        assert_eq!(coverage(&input, &mut bounds), resolved);
    }

    #[test]
    fn the_unroll_at_a_coarse_zoom_takes_every_leaf() {
        let (matrix, eye) = looking_at_the_front(3.0);
        let frustum = Frustum::new(&matrix);
        let rules = default_rules();
        let globe = GlobeUniform { pan_x: 0.0, pan_y: 0.0, radius: 1.0, transition: 0.5 };
        let input = GlobeCoverageInput { eye, globe, rules: &rules, look_at: (2, 2), target_zoom: 2, frustum: &frustum };
        let resolved = coverage(&input, &mut GlobeBounds::default());
        assert_eq!(resolved.targets.len(), 16);
        assert!(resolved.targets.iter().all(|t| t.tile.z == 2 && t.lines && t.labels && t.raster_size.is_none()));
    }
}
