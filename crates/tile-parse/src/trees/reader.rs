// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The trees of a tile (the Swift `TreeFeatureReader`): the fills that grow
//! them and the roads they keep off are gathered as the layers are read, and
//! the trees are placed once every layer is in, since they keep off the
//! buildings' footprints too.

use map_style::style::{FillStyle, RoadLevel, RoadStyle, TreePointStyle};
use mvt::geometry::{Point, Polygon};

use super::TreeInstance;
use super::scatter::{self, Area, Footprint, MappedTree, Obstacles, Road, TreeFillParams, TreePointParams};
use crate::coordinate_space::{TILE_EXTENT, render_point};
use crate::geom::V2;
use crate::options::TileParseOptions;
use crate::tile::{Tile, quarters};
use crate::unification::ReadingStageResult;

pub struct TreeFeatureReader {
    minimum_zoom: Option<i32>,
    maximum_per_tile: usize,
}

fn render_ring(ring: &[Point]) -> Vec<V2> {
    ring.iter().map(|p| render_point(V2::new(p.x as f32, p.y as f32))).collect()
}

impl TreeFeatureReader {
    pub fn new(options: &TileParseOptions) -> Self {
        TreeFeatureReader { minimum_zoom: options.tree_minimum_source_zoom, maximum_per_tile: options.tree_maximum_per_tile.max(0) as usize }
    }

    pub fn stands_trees(&self, tile: Tile) -> bool {
        self.minimum_zoom.is_some_and(|m| tile.z >= m)
    }

    /// A fill's polygons, kept for the scatter when the fill grows trees, and
    /// for the trees to keep off when it stands none.
    pub fn collect_fill(&self, polygons: &[Polygon], fill: &FillStyle, tile: Tile, result: &mut ReadingStageResult) {
        if !self.stands_trees(tile) || polygons.is_empty() {
            return;
        }
        if let Some(trees) = &fill.trees {
            let style = TreeFillParams {
                spacing_metres: trees.spacing_metres,
                lattice: trees.lattice(),
                minimum_height_metres: trees.minimum_height_metres,
                maximum_height_metres: trees.maximum_height_metres,
                road_clearance_metres: trees.road_clearance_metres,
            };
            result.tree_areas.push(Area { polygons: polygons.to_vec(), style });
        }
        if fill.excludes_trees {
            for p in polygons.iter().filter(|p| p.exterior.len() >= 3) {
                result.tree_excluded_fills.push(Footprint { exterior: render_ring(&p.exterior), interiors: p.interiors.iter().map(|r| render_ring(r)).collect() });
            }
        }
    }

    /// A mapped tree's points. A point outside the tile is its neighbour's.
    pub fn collect_points(&self, points: &[Point], tree: &TreePointStyle, tile: Tile, result: &mut ReadingStageResult) {
        if !self.stands_trees(tile) {
            return;
        }
        let style = TreePointParams { minimum_height_metres: tree.minimum_height_metres, maximum_height_metres: tree.maximum_height_metres, crown_clearance_metres: tree.crown_clearance_metres };
        for p in points {
            let t = V2::new(p.x as f32, p.y as f32);
            if t.x >= 0.0 && t.x < TILE_EXTENT && t.y >= 0.0 && t.y < TILE_EXTENT {
                result.mapped_trees.push(MappedTree { tile_point: t, style });
            }
        }
    }

    /// A ground road's lines, as wide as the widest stroke its style draws. A
    /// tunnel runs under the trees and a bridge over them.
    pub fn collect_road(&self, lines: &[Vec<Point>], road: &RoadStyle, tile: Tile, result: &mut ReadingStageResult) {
        if !self.stands_trees(tile) || road.level != RoadLevel::Ground || lines.is_empty() {
            return;
        }
        let width = road.ordered_passes().iter().map(|p| p.pass.line_geometry.line_width as f32).fold(0f32, f32::max);
        for line in lines.iter().filter(|l| l.len() >= 2) {
            result.tree_roads.push(Road { points: render_ring(line), half_width: width.max(0.0) / 2.0 });
        }
    }

    /// Places the tile's trees over the gathered fills, off the footprints, roads and excluded fills.
    pub fn finish(&self, tile: Tile, footprints: Vec<Footprint>, result: &mut ReadingStageResult) {
        let areas = std::mem::take(&mut result.tree_areas);
        let roads = std::mem::take(&mut result.tree_roads);
        let excluded = std::mem::take(&mut result.tree_excluded_fills);
        let mapped = std::mem::take(&mut result.mapped_trees);
        if !areas.is_empty() || !mapped.is_empty() {
            let mut all = footprints;
            all.extend(excluded);
            result.trees = scatter::scatter(&areas, &mapped, tile, &Obstacles { footprints: all, roads }, self.maximum_per_tile);
        }
        if result.keeps_quarters {
            let (trees, starts) = by_quarter(std::mem::take(&mut result.trees));
            result.trees = trees;
            result.tree_quarter_starts = starts;
        }
    }
}

/// `trees` laid out quarter by quarter, each in priority order, and where each quarter starts, the count last.
pub fn by_quarter(trees: Vec<TreeInstance>) -> (Vec<TreeInstance>, Vec<u32>) {
    let mut q: [Vec<TreeInstance>; quarters::COUNT] = Default::default();
    for t in trees {
        q[quarters::of_render_point(t.position)].push(t);
    }
    let mut starts = vec![0u32];
    for quarter in &q {
        starts.push(starts[starts.len() - 1] + quarter.len() as u32);
    }
    (q.into_iter().flatten().collect(), starts)
}
