// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The geometry of one layer's features in the parser's tile space (the
//! Swift `TileLayerGeometry`): the decoder's coordinates scaled from the
//! layer's own extent to the tile extent, decoded on demand per feature so a
//! feature a reader skips costs nothing.

use mvt::Feature;
use mvt::geometry::{MultiLineString, MultiPoint, MultiPolygon, Point, Polygon, decode_lines, decode_points, decode_polygons};

use crate::coordinate_space::TILE_EXTENT_F64;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerGeometry {
    /// 1 when the layer is at the tile extent, the common case, which leaves the points untouched.
    scale: f64,
}

impl LayerGeometry {
    pub fn new(extent: u32) -> LayerGeometry {
        LayerGeometry { scale: if extent > 0 { TILE_EXTENT_F64 / extent as f64 } else { 1.0 } }
    }

    pub fn polygons(&self, feature: &Feature<'_>) -> MultiPolygon {
        let polygons = decode_polygons(feature.geometry_bytes());
        if self.scale == 1.0 {
            return polygons;
        }
        polygons.into_iter().map(|p| Polygon { exterior: self.normalize(p.exterior), interiors: p.interiors.into_iter().map(|r| self.normalize(r)).collect() }).collect()
    }

    pub fn lines(&self, feature: &Feature<'_>) -> MultiLineString {
        let lines = decode_lines(feature.geometry_bytes());
        if self.scale == 1.0 {
            return lines;
        }
        lines.into_iter().map(|l| self.normalize(l)).collect()
    }

    pub fn points(&self, feature: &Feature<'_>) -> MultiPoint {
        let points = decode_points(feature.geometry_bytes());
        if self.scale == 1.0 {
            return points;
        }
        self.normalize(points)
    }

    fn normalize(&self, points: Vec<Point>) -> Vec<Point> {
        if self.scale == 1.0 {
            return points;
        }
        points.into_iter().map(|p| Point { x: (p.x as f64 * self.scale).round() as i32, y: (p.y as f64 * self.scale).round() as i32 }).collect()
    }
}
