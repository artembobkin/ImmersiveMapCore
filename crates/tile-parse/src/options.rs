// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! What the parser is told about the map it parses for: the settings it
//! reads, and nothing else. Every field is part of the prepared-tile
//! identity: a tile parsed under one value must never be served to a map
//! that wants another. The label policy is not here: it comes in as the
//! label decisions.

use std::collections::BTreeMap;

use crate::coordinate_space::TILE_EXTENT_F64;

#[derive(Debug, Clone, PartialEq)]
pub struct TileParseOptions {
    /// Whether point and road labels are resolved and baked.
    pub labels_enabled: bool,
    /// Debug: a one-unit frame around every tile.
    pub add_test_borders: bool,
    /// The coarsest tile zoom that draws buildings: a coarser tile never
    /// extrudes, so its buildings are neither tessellated nor uploaded.
    pub building_minimum_source_zoom: i32,
    /// The tileset's deepest zoom, where every building is a feature of its
    /// own. A coarser tile ships its buildings merged, so the parts rules
    /// that go by a feature's id would lay a whole group flat. `None` when
    /// every tile's buildings are taken as features of their own.
    pub building_deepest_source_zoom: Option<i32>,
    /// The tile zoom the ground fills are flattened from, `None` for never,
    /// and the grid they keep to.
    pub ground_flattening_from_tile_zoom: Option<i32>,
    pub ground_flattening_grid: i32,
    /// The grid of a tile zoom, over `ground_flattening_grid` for the zooms
    /// it names.
    pub ground_flattening_grid_by_tile_zoom: BTreeMap<i32, i32>,
    /// Whether the flattened ground rises by a heightmap: the roads and the
    /// lines of a flattened tile then take a vertex at every crossing of the
    /// flattening's grid.
    pub ground_relief_enabled: bool,
    /// The coarsest tile zoom that stands trees, `None` with trees off.
    pub tree_minimum_source_zoom: Option<i32>,
    /// The most trees a tile keeps.
    pub tree_maximum_per_tile: i32,
}

impl TileParseOptions {
    pub const GROUND_FLATTENING_GRID_RANGE: std::ops::RangeInclusive<i32> = 1..=256;

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        labels_enabled: bool,
        add_test_borders: bool,
        building_minimum_source_zoom: i32,
        building_deepest_source_zoom: Option<i32>,
        ground_flattening_from_tile_zoom: Option<i32>,
        ground_flattening_grid: i32,
        ground_flattening_grid_by_tile_zoom: BTreeMap<i32, i32>,
        ground_relief_enabled: bool,
        tree_minimum_source_zoom: Option<i32>,
        tree_maximum_per_tile: i32,
    ) -> TileParseOptions {
        let clamp = |g: i32| g.clamp(*Self::GROUND_FLATTENING_GRID_RANGE.start(), *Self::GROUND_FLATTENING_GRID_RANGE.end());
        TileParseOptions {
            labels_enabled,
            add_test_borders,
            building_minimum_source_zoom,
            building_deepest_source_zoom,
            ground_flattening_from_tile_zoom,
            ground_flattening_grid: clamp(ground_flattening_grid),
            ground_flattening_grid_by_tile_zoom: ground_flattening_grid_by_tile_zoom.into_iter().map(|(k, v)| (k, clamp(v))).collect(),
            ground_relief_enabled,
            tree_minimum_source_zoom,
            tree_maximum_per_tile: tree_maximum_per_tile.max(0),
        }
    }

    /// The Swift initializer's defaults for everything after the building zoom.
    pub fn basic(labels_enabled: bool, building_minimum_source_zoom: i32) -> TileParseOptions {
        TileParseOptions::new(labels_enabled, false, building_minimum_source_zoom, None, None, 16, BTreeMap::new(), false, None, 0)
    }

    /// The grid a flattened tile of `tile_zoom` keeps to.
    pub fn ground_flattening_grid_for(&self, tile_zoom: i32) -> i32 {
        *self.ground_flattening_grid_by_tile_zoom.get(&tile_zoom).unwrap_or(&self.ground_flattening_grid)
    }

    /// The grids by zoom as one string for the prepared-tile identity, empty
    /// without overrides.
    pub fn ground_flattening_grid_by_tile_zoom_key(&self) -> String {
        self.ground_flattening_grid_by_tile_zoom.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(",")
    }

    /// The grid step, in tile units, the roads and the lines of a tile take
    /// a vertex at, `None` where they keep their own vertices.
    pub fn relief_subdivision_step(&self, tile_zoom: i32) -> Option<f32> {
        if !self.ground_relief_enabled || !self.flattens_ground(tile_zoom) {
            return None;
        }
        Some(TILE_EXTENT_F64 as f32 / self.ground_flattening_grid_for(tile_zoom) as f32)
    }

    /// Whether a tile keeps its buildings and trees quarter by quarter: a
    /// tile that draws buildings and is coarser than the tileset's deepest
    /// zoom, so its children carry buildings of their own.
    pub fn keeps_quarters(&self, tile_zoom: i32) -> bool {
        match self.building_deepest_source_zoom {
            Some(deepest) => tile_zoom >= self.building_minimum_source_zoom && tile_zoom < deepest,
            None => false,
        }
    }

    pub fn flattens_ground(&self, tile_zoom: i32) -> bool {
        self.ground_flattening_from_tile_zoom.is_some_and(|from| tile_zoom >= from)
    }
}
