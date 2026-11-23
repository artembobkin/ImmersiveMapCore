// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! A building footprint to extrude once the whole tile is read, its footprint
//! signature, and the roof lookup the labels rise by.

use mvt::geometry::{Point, Polygon};

use crate::coordinate_space::render_point;
use crate::geom::V2;
use crate::parsed_polygon::ParsedPolygon;

use super::resolver::point_in_ring;

/// A building footprint reduced to a value two features of the same shape
/// share whatever their winding or starting vertex.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FootprintSignature {
    pub exterior: Vec<u64>,
    pub interiors: Vec<Vec<u64>>,
}

impl FootprintSignature {
    /// `None` for an exterior with fewer than three distinct points.
    pub fn new(polygon: &Polygon) -> Option<FootprintSignature> {
        let exterior = canonical_ring(&polygon.exterior)?;
        let mut interiors: Vec<Vec<u64>> = polygon.interiors.iter().filter_map(|r| canonical_ring(r)).collect();
        interiors.sort();
        Some(FootprintSignature { exterior, interiors })
    }
}

fn canonical_ring(ring: &[Point]) -> Option<Vec<u64>> {
    let mut points: Vec<Point> = ring.to_vec();
    if points.len() > 1 && points[points.len() - 1] == points[0] {
        points.pop();
    }
    let mut filtered: Vec<Point> = Vec::with_capacity(points.len());
    for p in points {
        if filtered.last() == Some(&p) {
            continue;
        }
        if filtered.len() >= 2 && filtered[filtered.len() - 2] == p {
            filtered.pop();
            continue;
        }
        filtered.push(p);
    }
    if filtered.len() > 1 && filtered[filtered.len() - 1] == filtered[0] {
        filtered.pop();
    }
    if filtered.len() < 3 {
        return None;
    }
    let forward: Vec<u64> = filtered.iter().map(|p| ((p.x as u32 as u64) << 32) | p.y as u32 as u64).collect();
    let backward: Vec<u64> = forward.iter().rev().copied().collect();
    let f = canonical_rotation(&forward);
    let b = canonical_rotation(&backward);
    Some(if f < b { f } else { b })
}

fn canonical_rotation(values: &[u64]) -> Vec<u64> {
    let mut best = values.to_vec();
    for start in 1..values.len() {
        let mut candidate = values[start..].to_vec();
        candidate.extend_from_slice(&values[..start]);
        if candidate < best {
            best = candidate;
        }
    }
    best
}

/// A footprint that will be extruded once the tile has been read: the
/// resolver needs every candidate of a building (and of its neighbours)
/// before it can tell an outline from a part. Every ring is render space.
#[derive(Debug, Clone)]
pub struct BuildingExtrusionCandidate {
    pub style_key: u8,
    pub building_id: u64,
    /// A `building:part` rather than an outline.
    pub is_part: bool,
    pub footprint_signature: Option<FootprintSignature>,
    pub clipped_exterior: Vec<V2>,
    pub clipped_interiors: Vec<Vec<V2>>,
    /// The flat lid's triangulation of the clipped footprint.
    pub roof: ParsedPolygon,
    pub base_height: f32,
    pub top_height: f32,
    /// The radius its edges are rounded by, in tile units, zero for a sharp box.
    pub edge_radius: f32,
    /// The quarter of its tile the footprint was cut to, `None` for a tile kept whole.
    pub quarter: Option<usize>,
}

/// The roof a point of the tile stands under: the top of the tallest volume
/// whose footprint holds it, zero where none does. A point in a courtyard is
/// not under that building's roof.
/// A volume of the lookup: its exterior, its courtyards, its bounds and its top.
type RoofVolume = (Vec<V2>, Vec<Vec<V2>>, V2, V2, f32);

pub struct BuildingRoofLookup {
    volumes: Vec<RoofVolume>,
    cells: std::collections::HashMap<i64, Vec<usize>>,
}

impl BuildingRoofLookup {
    const CELL: f32 = 256.0;

    pub fn new(candidates: &[BuildingExtrusionCandidate]) -> BuildingRoofLookup {
        let mut lookup = BuildingRoofLookup { volumes: Vec::with_capacity(candidates.len()), cells: Default::default() };
        for c in candidates {
            if c.clipped_exterior.len() < 3 {
                continue;
            }
            let (mut min, mut max) = (c.clipped_exterior[0], c.clipped_exterior[0]);
            for p in &c.clipped_exterior {
                min = V2::new(min.x.min(p.x), min.y.min(p.y));
                max = V2::new(max.x.max(p.x), max.y.max(p.y));
            }
            let index = lookup.volumes.len();
            lookup.volumes.push((c.clipped_exterior.clone(), c.clipped_interiors.clone(), min, max, c.top_height));
            let (a, b) = (Self::cell(min), Self::cell(max));
            for row in a.1..=b.1 {
                for column in a.0..=b.0 {
                    lookup.cells.entry(Self::key(column, row)).or_default().push(index);
                }
            }
        }
        lookup
    }

    pub fn is_empty(&self) -> bool {
        self.volumes.is_empty()
    }

    /// The roof height over a point given in tile space (y down, as labels carry their anchors).
    pub fn roof_height(&self, tile_point: [i16; 2]) -> f32 {
        let point = render_point(V2::new(tile_point[0] as f32, tile_point[1] as f32));
        let (column, row) = Self::cell(point);
        let Some(indices) = self.cells.get(&Self::key(column, row)) else { return 0.0 };
        let mut roof = 0f32;
        for &i in indices {
            let (exterior, interiors, min, max, top) = &self.volumes[i];
            if *top > roof
                && point.x >= min.x
                && point.x <= max.x
                && point.y >= min.y
                && point.y <= max.y
                && point_in_ring(point, exterior)
                && !interiors.iter().any(|r| point_in_ring(point, r))
            {
                roof = *top;
            }
        }
        roof
    }

    fn cell(p: V2) -> (i64, i64) {
        ((p.x / Self::CELL).floor() as i64, (p.y / Self::CELL).floor() as i64)
    }

    fn key(column: i64, row: i64) -> i64 {
        row.wrapping_mul(1_000_003).wrapping_add(column)
    }
}
