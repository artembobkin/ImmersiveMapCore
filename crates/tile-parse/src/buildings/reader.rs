// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! What a polygon feature contributes to the buildings of a tile (the Swift
//! `BuildingFeatureReader`): whether it is extruded, how tall, and the
//! candidate that records it until the whole tile has been read and the
//! resolver can tell the outlines from the parts.

use std::collections::HashSet;

use map_style::schema::BuildingExtrusion;
use map_style::style::ExtrusionStyle;
use mvt::geometry::Polygon;

use super::candidate::{BuildingExtrusionCandidate, FootprintSignature};
use super::{mesh_builder, resolver};
use crate::coordinate_space::{TILE_EXTENT, render_points};
use crate::options::TileParseOptions;
use crate::parse_polygon::ParsedGeometry;
use crate::reading::PreparedLayer;
use crate::tile::{Tile, quarters};
use crate::unification::ReadingStageResult;

/// What a building layer says about its parts before any feature is read.
#[derive(Debug, Default)]
pub struct PartInfo {
    pub part_ids: HashSet<u64>,
    pub footprint_signatures: HashSet<FootprintSignature>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExtrusionHeights {
    pub base: f32,
    pub top: f32,
    pub edge_radius: f32,
}

/// A feature's extrusion, decided once for all of its polygons.
#[derive(Debug, Clone, Copy)]
pub struct Extrusion {
    pub building_id: u64,
    pub is_part: bool,
    pub heights: ExtrusionHeights,
}

pub struct BuildingFeatureReader {
    minimum_source_zoom: i32,
    deepest_source_zoom: Option<i32>,
}

impl BuildingFeatureReader {
    pub fn new(options: &TileParseOptions) -> Self {
        BuildingFeatureReader { minimum_source_zoom: options.building_minimum_source_zoom, deepest_source_zoom: options.building_deepest_source_zoom }
    }

    fn ships_merged_buildings(&self, zoom: i32) -> bool {
        self.deepest_source_zoom.is_some_and(|d| zoom < d)
    }

    /// The part ids and part footprint signatures of a layer's buildings.
    pub fn part_info(&self, layer: &PreparedLayer<'_>) -> PartInfo {
        let mut info = PartInfo::default();
        if !layer.features.iter().any(|f| f.facts.building().is_some()) {
            return info;
        }
        for f in &layer.features {
            let Some(building) = f.facts.building() else { continue };
            if !building.is_part {
                continue;
            }
            info.part_ids.insert(building.building_identity.unwrap_or(f.id()));
            for polygon in layer.geometry.polygons(&f.feature) {
                if let Some(signature) = FootprintSignature::new(&polygon) {
                    info.footprint_signatures.insert(signature);
                }
            }
        }
        info
    }

    /// Whether and how the feature is extruded, `None` when it stays a flat
    /// ground fill: not a building, hidden, a tile coarser than the building
    /// grid, an outline whose building has parts, or a footprint that repeats a part.
    pub fn extrusion(&self, feature_id: u64, building: Option<&BuildingExtrusion>, style: &ExtrusionStyle, polygons: &[Polygon], parts: &PartInfo, tile: Tile) -> Option<Extrusion> {
        if tile.z < self.minimum_source_zoom {
            return None;
        }
        let building = building?;
        if building.is_hidden {
            return None;
        }
        let building_id = building.building_identity.unwrap_or(feature_id);
        // A merged group's id is one member's: each polygon then stands, and
        // the resolver sorts an outline from its parts by their footprints.
        let merged = self.ships_merged_buildings(tile.z);
        let has_parts = !merged && parts.part_ids.contains(&building_id);
        // The signature is quadratic in ring vertices: only computed when there are part signatures to match.
        let matches_part_footprint = !merged
            && !building.is_part
            && !parts.footprint_signatures.is_empty()
            && polygons.iter().any(|p| FootprintSignature::new(p).is_some_and(|s| parts.footprint_signatures.contains(&s)));
        if matches_part_footprint || (has_parts && !building.is_part) {
            return None;
        }
        let heights = extrusion_heights(building, tile.z, style)?;
        Some(Extrusion { building_id, is_part: building.is_part, heights })
    }

    /// The candidate for one polygon of an extruded feature: the extrusion
    /// path's one entry into render space.
    pub fn candidate(&self, polygon: &Polygon, geometry: &ParsedGeometry, style_key: u8, extrusion: &Extrusion, quarter: Option<usize>) -> Option<BuildingExtrusionCandidate> {
        let h = extrusion.heights;
        if h.top <= h.base {
            return None;
        }
        let signature = FootprintSignature::new(polygon)?;
        Some(BuildingExtrusionCandidate {
            style_key,
            building_id: extrusion.building_id,
            is_part: extrusion.is_part,
            footprint_signature: Some(signature),
            clipped_exterior: render_points(&geometry.clipped.exterior),
            clipped_interiors: geometry.clipped.interiors.iter().map(|r| render_points(r)).collect(),
            roof: geometry.parsed.clone(),
            base_height: h.base,
            top_height: h.top,
            edge_radius: h.edge_radius,
            quarter,
        })
    }

    /// After the whole tile is read: resolves the candidates and builds a
    /// mesh for each survivor. Returns the survivors, the volumes the tile
    /// draws. A tile kept by quarters resolves quarter by quarter.
    pub fn append_extruded_meshes(&self, candidates: &[BuildingExtrusionCandidate], result: &mut ReadingStageResult) -> Vec<BuildingExtrusionCandidate> {
        let resolved: Vec<BuildingExtrusionCandidate> = if candidates.iter().any(|c| c.quarter.is_some()) {
            (0..quarters::COUNT).flat_map(|q| resolver::resolve_exterior(candidates.iter().filter(|c| c.quarter == Some(q)).cloned().collect())).collect()
        } else {
            resolver::resolve_exterior(candidates.to_vec())
        };
        for c in &resolved {
            if let Some(mut mesh) = mesh_builder::build(&c.clipped_exterior, &c.clipped_interiors, &c.roof, c.base_height, c.top_height, c.edge_radius, TILE_EXTENT, c.quarter) {
                mesh.building_id = c.building_id;
                mesh.quarter = c.quarter;
                result.extruded_by_style.entry(c.style_key).or_default().push(mesh);
            }
        }
        resolved
    }
}

/// The building's heights in tile units at the tile's zoom, from the schema
/// reading or the style's fallback; `None` when nothing says how tall it is.
pub fn extrusion_heights(building: &BuildingExtrusion, tile_zoom: i32, style: &ExtrusionStyle) -> Option<ExtrusionHeights> {
    let fallback = style.fallback_height;
    if building.height_metres.is_none() && building.base_height_metres.is_none() && fallback <= 0.0 {
        return None;
    }
    let height = building.height_metres.unwrap_or(fallback);
    if height <= 0.0 {
        return None;
    }
    let min_height = building.base_height_metres.unwrap_or(0.0);
    let zoom_scale = 2f32.powf((tile_zoom - style.anchor_zoom) as f32);
    let scaled = height * style.height_scale * zoom_scale;
    let scaled_min = min_height * style.height_scale * zoom_scale;
    let base = scaled_min.min(scaled).max(0.0);
    let top = scaled.max(base);
    let edge_radius = style.edge_radius.max(0.0) * style.height_scale * zoom_scale;
    Some(ExtrusionHeights { base, top, edge_radius })
}
