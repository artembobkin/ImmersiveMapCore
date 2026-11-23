// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The ground of a tile (the Swift `GroundFeatureReader`): the ocean split
//! that keeps a polygon with hundreds of holes tessellable, and what every
//! tile gets under and around its features: the background quad, the debug
//! frame, and the subdivision that lets a coarse tile's triangles follow the
//! sphere.

use map_style::style::{FeatureStyle, FillStyle, MapStyleRuntime};
use mvt::geometry::Polygon;

use crate::coordinate_space::TILE_EXTENT;
use crate::geom::V2;
use crate::ground_subdivider;
use crate::parse_polygon::ParsePolygon;
use crate::parsed_polygon::ParsedPolygon;
use crate::reading::to_style_tile;
use crate::tile::Tile;
use crate::unification::{BakedStyle, ReadingStageResult};

/// An ocean polygon with this many holes or more draws its exterior as ocean
/// and each hole as land, so earcut never sees the islands of a coastal tile at once.
pub const COMPLEX_OCEAN_HOLE_SPLIT_THRESHOLD: usize = 64;

pub struct GroundFeatureReader<'s> {
    style: &'s MapStyleRuntime,
}

impl<'s> GroundFeatureReader<'s> {
    pub fn new(style: &'s MapStyleRuntime) -> Self {
        GroundFeatureReader { style }
    }

    /// Whether a feature's polygons take the ocean split: only a fill whose
    /// style asks for it, and only when one of its polygons is complex enough.
    pub fn splits_complex_ocean_holes(&self, fill: &FillStyle, polygons: &[Polygon]) -> bool {
        fill.splits_complex_holes && polygons.iter().any(|p| p.interiors.len() >= COMPLEX_OCEAN_HOLE_SPLIT_THRESHOLD)
    }

    /// The ocean split for one polygon: its exterior as the feature's style,
    /// its holes as the background (land) style. False when the polygon is
    /// not complex or its exterior does not tessellate, and the caller draws
    /// it the ordinary way. With `as_outline` the pieces stay outlines for the flattening.
    pub fn append_complex_ocean_polygon(&self, polygon: &Polygon, fill: &FillStyle, as_outline: bool, result: &mut ReadingStageResult, parse: &mut ParsePolygon, tile: Tile) -> bool {
        if polygon.interiors.len() < COMPLEX_OCEAN_HOLE_SPLIT_THRESHOLD {
            return false;
        }
        let mut parse_one = |p: &Polygon| if as_outline { parse.parse_outline(p, TILE_EXTENT) } else { parse.parse(p, TILE_EXTENT) };
        let ocean = Polygon { exterior: polygon.exterior.clone(), interiors: vec![] };
        let Some(parsed_ocean) = parse_one(&ocean) else { return false };
        result.polygon_by_style.entry(fill.key).or_default().push(parsed_ocean);
        result.styles.insert(fill.key, BakedStyle::from_fill(fill));
        let FeatureStyle::Fill(land) = self.style.background_style(to_style_tile(tile)) else { return true };
        if land.key == 0 {
            return true;
        }
        result.styles.insert(land.key, BakedStyle::from_fill(&land));
        for ring in &polygon.interiors {
            if let Some(parsed) = parse_one(&Polygon { exterior: ring.clone(), interiors: vec![] }) {
                result.polygon_by_style.entry(land.key).or_default().push(parsed);
            }
        }
        true
    }

    /// After every layer is read: the background under everything, the debug
    /// frame when asked for, and the sphere subdivision of the ground bucket.
    pub fn finish(&self, tile: Tile, add_test_borders: bool, result: &mut ReadingStageResult) {
        self.append_background(tile, result);
        if add_test_borders {
            self.append_border(1.0, result);
        }
        ground_subdivider::subdivide_if_needed(result.polygon_by_style.values_mut().flatten(), tile.z);
    }

    fn append_background(&self, tile: Tile, result: &mut ReadingStageResult) {
        // The real tile: the background colour is zoom-banded. A background
        // that is not a fill paints nothing.
        let FeatureStyle::Fill(style) = self.style.background_style(to_style_tile(tile)) else { return };
        let e = TILE_EXTENT;
        let quad = ParsedPolygon::new(vec![V2::new(0.0, 0.0), V2::new(e, 0.0), V2::new(e, e), V2::new(0.0, e)], vec![0, 1, 2, 0, 2, 3]);
        result.polygon_by_style.entry(style.key).or_default().insert(0, quad);
        result.styles.insert(style.key, BakedStyle::from_fill(&style));
    }

    fn append_border(&self, width: f32, result: &mut ReadingStageResult) {
        let style = self.style.debug_border_style();
        let s = 4096f32;
        // Bottom-left, bottom-right, top-left, top-right, wound counter-clockwise in render space.
        let rects = [
            [V2::new(0.0, 0.0), V2::new(s, 0.0), V2::new(0.0, width), V2::new(s, width)],
            [V2::new(0.0, s - width), V2::new(s, s - width), V2::new(0.0, s), V2::new(s, s)],
            [V2::new(0.0, 0.0), V2::new(width, 0.0), V2::new(0.0, s), V2::new(width, s)],
            [V2::new(s - width, 0.0), V2::new(s, 0.0), V2::new(s - width, s), V2::new(s, s)],
        ];
        let polygons = rects.iter().map(|r| ParsedPolygon::new(r.to_vec(), vec![0, 1, 2, 1, 3, 2])).collect();
        result.polygon_by_style.insert(style.key, polygons);
        result.styles.insert(style.key, BakedStyle::from_fill(&style));
    }
}
