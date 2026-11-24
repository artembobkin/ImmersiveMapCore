// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The tile grid's coverage, a port of the Swift engine's `TileGridZoom` and
//! `TileGridCoverage`: which zoom's tiles the grid shows under a camera, and
//! the tiles of that zoom in view, the nearest to where the camera looks
//! first, and no more than a limit.
//!
//! The grid names squares of the ground, not the map's tiles, so it reads
//! nothing of the ring rules. The map's own coverage (`flat`, `globe`)
//! answers another question, which of its tiles to load, and without a
//! bound: under a camera tilted to the horizon the tiles of a deep zoom in
//! view are without number, so the grid walks the tree nearest first and
//! stops at its limit. What shows is decided by the same tests as the
//! map's: the frustum's footprint on the plane, the frustum and the horizon
//! on the sphere.

use crate::frustum::Frustum;
use crate::globe::{self, Evaluation, GlobeBounds, TRANSITION_LOW_ZOOM_FALLBACK_LIMIT, Visibility};
use crate::polygon::CoveragePolygon;
use crate::{FlatRenderState, VisibleTile};

/// The deepest zoom a tile of the grid is named at.
pub const MAXIMUM_ZOOM: i32 = 24;

/// The world copies the flat map draws.
const WORLD_WRAPS: [i32; 3] = [-1, 0, 1];

// MARK: The zoom

/// What decides the zoom the grid shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomSettings {
    /// The zooms whose tiles the grid shows, the first and the last.
    pub zoom_range: (i32, i32),
    /// The grid hides while a tile is narrower on screen than this, in points.
    pub minimum_tile_screen_points: f64,
    /// The texts hide while a tile is narrower on screen than this, in points.
    pub label_minimum_tile_screen_points: f64,
}

/// The grid a camera zoom shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Resolution {
    /// The zoom whose tiles the grid shows.
    pub zoom: i32,
    /// How wide one of those tiles is on screen under the camera, in points.
    pub tile_screen_points: f64,
    /// Whether the tiles are wide enough for their texts.
    pub draws_labels: bool,
}

/// A range within the zooms a tile is named at.
pub fn clamped_zoom_range(range: (i32, i32)) -> (i32, i32) {
    let lower = range.0.max(0).min(MAXIMUM_ZOOM);
    (lower, range.1.max(lower).min(MAXIMUM_ZOOM))
}

/// The grid at `camera_zoom`: the tiles of the camera's own zoom, clamped
/// into the settings' range, `None` while such a tile is narrower on screen
/// than the settings allow. `base_tile_screen_points` is what a tile spans
/// at the camera zoom equal to its own (`base_tile_screen_points`), and a
/// tile of another zoom spans twice that for every zoom the camera is
/// deeper.
pub fn resolve_zoom(camera_zoom: f64, base_tile_screen_points: f64, settings: &ZoomSettings) -> Option<Resolution> {
    if !camera_zoom.is_finite() || !(base_tile_screen_points > 0.0) {
        return None;
    }
    let (lower, upper) = settings.zoom_range;
    let zoom = (camera_zoom.floor().max(-1.0).min(64.0) as i32).max(lower).min(upper);
    let points = base_tile_screen_points * 2f64.powf(camera_zoom - zoom as f64);
    if !points.is_finite() || !(points >= settings.minimum_tile_screen_points) {
        return None;
    }
    Some(Resolution { zoom, tile_screen_points: points, draws_labels: points >= settings.label_minimum_tile_screen_points })
}

/// Screen points a tile spans at the camera zoom equal to its own: the
/// render camera (vertical field of view pi/4, at distance 1) shows the
/// tile's world size over the viewport's height, so the share is the same in
/// any window.
pub fn base_tile_screen_points(viewport_height_points: f64, tile_world_size: f64) -> f64 {
    viewport_height_points * tile_world_size / (2.0 * (std::f64::consts::PI / 8.0).tan())
}

/// How finely a tile of `zoom` is divided, in quads a side. The plane needs
/// no division, a quad is flat. On the sphere and through the unroll a tile
/// curves, and the coarser it is the more of the sphere it spans: the whole
/// world at zoom 0 takes `LATTICE_SIDE` quads a side, and each zoom deeper
/// half as many, four at the least.
pub fn subdivision(zoom: i32, flat: bool) -> u32 {
    if flat {
        return 1;
    }
    (LATTICE_SIDE >> zoom.max(0).min(30)).max(4)
}

/// The finest a tile is divided, in quads a side.
pub const LATTICE_SIDE: u32 = 64;

// MARK: The walk

/// What the walk is told of a tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing of the tile shows.
    Reject,
    /// Some of the tile may show: its children are asked.
    Descend,
    /// All of the tile shows: nothing under it is asked again.
    AcceptSubtree,
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    distance: f64,
    node: VisibleTile,
    accepted: bool,
}

/// The walk's heap, kept between frames so a frame allocates nothing.
#[derive(Debug, Default)]
pub struct Walk {
    /// A binary heap of the walk's nodes, the nearest on top.
    entries: Vec<Entry>,
}

impl Walk {
    fn push(&mut self, entry: Entry) {
        self.entries.push(entry);
        let mut index = self.entries.len() - 1;
        while index > 0 {
            let parent = (index - 1) / 2;
            if !(self.entries[index].distance < self.entries[parent].distance) {
                break;
            }
            self.entries.swap(index, parent);
            index = parent;
        }
    }

    fn pop(&mut self) -> Option<Entry> {
        let top = *self.entries.first()?;
        let last = self.entries.pop()?;
        if self.entries.is_empty() {
            return Some(top);
        }
        self.entries[0] = last;
        let mut index = 0;
        loop {
            let left = index * 2 + 1;
            let right = left + 1;
            let mut smallest = index;
            if left < self.entries.len() && self.entries[left].distance < self.entries[smallest].distance {
                smallest = left;
            }
            if right < self.entries.len() && self.entries[right].distance < self.entries[smallest].distance {
                smallest = right;
            }
            if smallest == index {
                break;
            }
            self.entries.swap(index, smallest);
            index = smallest;
        }
        Some(top)
    }

    /// The tiles of `zoom` under `roots` that `test` lets through, appended
    /// to `out`, the nearest to `centre` (normalized world Mercator, in the
    /// world's own copy) first, `limit` at most. A tile is no nearer than
    /// the tile that holds it, so the tree is walked best first and the
    /// first `limit` tiles reached are the nearest. `wraps_round`: the world
    /// closes round the antimeridian (the sphere), so a tile across it is
    /// near.
    #[allow(clippy::too_many_arguments)]
    pub fn nearest(&mut self, zoom: i32, limit: usize, roots: &[VisibleTile], centre: [f64; 2], wraps_round: bool, mut test: impl FnMut(VisibleTile) -> Verdict, out: &mut Vec<VisibleTile>) {
        self.entries.clear();
        if zoom < 0 || limit == 0 {
            return;
        }
        let mut offer = |walk: &mut Walk, node: VisibleTile, accepted: bool| {
            let mut accepted = accepted;
            if !accepted {
                match test(node) {
                    Verdict::Reject => return,
                    Verdict::Descend => {}
                    Verdict::AcceptSubtree => accepted = true,
                }
            }
            walk.push(Entry { distance: distance_squared(centre, node, wraps_round), node, accepted });
        };
        for &root in roots {
            offer(self, root, false);
        }
        let first = out.len();
        // A bound on the walk itself, for a view that rejects nearly every tile it reaches.
        let mut budget = limit.saturating_mul(64).saturating_add(4096);
        while out.len() - first < limit && budget > 0 {
            let Some(entry) = self.pop() else { break };
            budget -= 1;
            let node = entry.node;
            if node.z == zoom {
                out.push(node);
                continue;
            }
            let (x, y) = (node.x * 2, node.y * 2);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                offer(self, VisibleTile { z: node.z + 1, x: x + dx, y: y + dy, wrap: node.wrap }, entry.accepted);
            }
        }
        self.entries.clear();
    }
}

/// How far `centre` is from the nearest point of a tile's square, in the
/// normalized world, squared.
pub fn distance_squared(centre: [f64; 2], node: VisibleTile, wraps_round: bool) -> f64 {
    let size = 1.0 / (1u64 << node.z.max(0).min(62)) as f64;
    let min_x = node.x as f64 * size + node.wrap as f64;
    let min_y = node.y as f64 * size;
    let along_x = |x: f64| (min_x - x).max(0.0).max(x - (min_x + size));
    let mut dx = along_x(centre[0]);
    if wraps_round {
        dx = dx.min(along_x(centre[0] - 1.0)).min(along_x(centre[0] + 1.0));
    }
    let dy = (min_y - centre[1]).max(0.0).max(centre[1] - (min_y + size));
    dx * dx + dy * dy
}

// MARK: The plane

/// Whether the frustum's footprint on the plane meets a tile's square.
fn square_meets(node: VisibleTile, polygon: &CoveragePolygon, flat: &FlatRenderState) -> bool {
    let [x, y, size] = flat.tile_origin_and_size(node.x, node.y, node.z, node.wrap);
    polygon.intersects(x, y, x + size, y + size)
}

/// The tiles of `zoom` under the camera on the flat map, in every copy of
/// the world in view, appended to `out`. A tile whose square lies wholly
/// within the footprint is taken with everything under it and asked no more:
/// under a camera tilted to the horizon that is most of the walk
/// (ImmersiveMapWeb's docs/benchmarks/B23-tile-grid.md). The tiles and their
/// order are those of the walk that asks every tile.
pub fn flat(walk: &mut Walk, zoom: i32, limit: usize, polygon: &CoveragePolygon, flat: &FlatRenderState, centre: [f64; 2], out: &mut Vec<VisibleTile>) {
    let roots = WORLD_WRAPS.map(|wrap| VisibleTile { z: 0, x: 0, y: 0, wrap });
    walk.nearest(
        zoom,
        limit,
        &roots,
        centre,
        false,
        |node| {
            let [x, y, size] = flat.tile_origin_and_size(node.x, node.y, node.z, node.wrap);
            if !polygon.intersects(x, y, x + size, y + size) {
                Verdict::Reject
            } else if node.z < zoom && polygon.contains_square(x, y, x + size, y + size) {
                Verdict::AcceptSubtree
            } else {
                Verdict::Descend
            }
        },
        out,
    );
}

/// The copies of the world a tile shows in on the flat map, appended to
/// `out`: none when it is out of view. Returns how many.
pub fn flat_copies(tile: (i32, i32, i32), polygon: &CoveragePolygon, flat: &FlatRenderState, out: &mut Vec<VisibleTile>) -> usize {
    let (x, y, z) = tile;
    let mut count = 0;
    for wrap in WORLD_WRAPS {
        let node = VisibleTile { z, x, y, wrap };
        if square_meets(node, polygon, flat) {
            out.push(node);
            count += 1;
        }
    }
    count
}

// MARK: The sphere

/// What the sphere's tests read of a frame.
#[derive(Debug, Clone, Copy)]
pub struct GlobeView<'a> {
    pub visibility: Visibility,
    pub frustum: &'a Frustum,
}

/// The tiles of `zoom` in view on the sphere and through the unroll,
/// appended to `out`.
pub fn globe(walk: &mut Walk, zoom: i32, limit: usize, view: &GlobeView<'_>, bounds: &mut GlobeBounds, centre: [f64; 2], out: &mut Vec<VisibleTile>) {
    // Through the unroll at the coarse zooms the visibility tests do not
    // hold, and the map takes every tile (`globe::coverage`).
    let takes_all = view.visibility.transition > 0.0 && zoom <= TRANSITION_LOW_ZOOM_FALLBACK_LIMIT;
    let root = [VisibleTile { z: 0, x: 0, y: 0, wrap: 0 }];
    walk.nearest(
        zoom,
        limit,
        &root,
        centre,
        true,
        |node| {
            if takes_all {
                return Verdict::AcceptSubtree;
            }
            match globe::evaluate((node.x, node.y, node.z), zoom, view.frustum, &view.visibility, bounds) {
                Evaluation::RejectFrustum | Evaluation::RejectHorizon => Verdict::Reject,
                Evaluation::Descend => Verdict::Descend,
                Evaluation::AcceptWholeSubtree => Verdict::AcceptSubtree,
            }
        },
        out,
    );
}

/// Whether a tile shows on the sphere or through the unroll.
pub fn globe_shows(tile: (i32, i32, i32), view: &GlobeView<'_>, bounds: &mut GlobeBounds) -> bool {
    if view.visibility.transition > 0.0 && tile.2 <= TRANSITION_LOW_ZOOM_FALLBACK_LIMIT {
        return true;
    }
    !matches!(globe::evaluate(tile, tile.2, view.frustum, &view.visibility, bounds), Evaluation::RejectFrustum | Evaluation::RejectHorizon)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(range: (i32, i32), minimum: f64, label_minimum: f64) -> ZoomSettings {
        ZoomSettings { zoom_range: range, minimum_tile_screen_points: minimum, label_minimum_tile_screen_points: label_minimum }
    }

    #[test]
    fn the_grid_follows_the_camera_inside_the_range() {
        let settings = settings((12, 15), 48.0, 96.0);
        assert_eq!(resolve_zoom(13.7, 400.0, &settings).map(|r| r.zoom), Some(13));
        assert_eq!(resolve_zoom(18.2, 400.0, &settings).map(|r| r.zoom), Some(15));
        assert_eq!(resolve_zoom(11.5, 400.0, &settings).map(|r| r.zoom), Some(12));
    }

    #[test]
    fn a_range_of_one_zoom_shows_that_zoom_alone() {
        let settings = settings((15, 15), 48.0, 96.0);
        for camera_zoom in [13.0, 15.0, 15.9, 19.0] {
            assert_eq!(resolve_zoom(camera_zoom, 400.0, &settings).map(|r| r.zoom), Some(15));
        }
    }

    #[test]
    fn a_tile_spans_twice_as_much_for_every_zoom_the_camera_is_deeper() {
        let settings = settings((15, 15), 48.0, 96.0);
        let level = resolve_zoom(15.0, 400.0, &settings).unwrap();
        let deeper = resolve_zoom(17.0, 400.0, &settings).unwrap();
        assert!((level.tile_screen_points - 400.0).abs() < 1e-9);
        assert!((deeper.tile_screen_points - 1600.0).abs() < 1e-9);
    }

    #[test]
    fn the_grid_hides_while_its_tiles_are_too_small() {
        let settings = settings((15, 15), 50.0, 100.0);
        // 400 points at zoom 15: 100 at 13, 50 at 12, 25 at 11.
        assert_eq!(resolve_zoom(11.0, 400.0, &settings), None);
        assert_eq!(resolve_zoom(12.0, 400.0, &settings).map(|r| r.draws_labels), Some(false), "shown, its texts too small to read");
        assert_eq!(resolve_zoom(13.0, 400.0, &settings).map(|r| r.draws_labels), Some(true));
        assert_eq!(resolve_zoom(15.0, 0.0, &settings), None);
        assert_eq!(resolve_zoom(f64::NAN, 400.0, &settings), None);
    }

    #[test]
    fn a_zoom_range_is_held_to_the_zooms_a_tile_is_named_at() {
        assert_eq!(clamped_zoom_range((-3, 40)), (0, MAXIMUM_ZOOM));
        assert_eq!(clamped_zoom_range((9, 4)), (9, 9));
    }

    #[test]
    fn a_coarse_tile_of_the_sphere_is_divided_finer() {
        assert_eq!(subdivision(0, true), 1);
        assert_eq!(subdivision(0, false), 64);
        assert_eq!(subdivision(2, false), 16);
        assert_eq!(subdivision(9, false), 4);
    }
}
