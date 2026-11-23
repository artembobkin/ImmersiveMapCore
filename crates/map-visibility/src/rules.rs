// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The ring rules: the bands the ground in view is cut into by the
//! distance, in tiles, from the tile the camera looks at.

/// One band: the tiles within `distance` rings of the look-at tile, drawn
/// `zoom_drop` zooms above the target zoom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlatRingRule {
    pub zoom_drop: i32,
    pub distance: f64,
    pub draws_lines: bool,
    pub draws_labels: bool,
    /// The side of the texture the band's ground is drawn from, `None` for geometry.
    pub raster_size: Option<u32>,
    pub terrain_zoom_drop: i32,
}

impl FlatRingRule {
    pub const fn new(zoom_drop: i32, distance: f64) -> FlatRingRule {
        FlatRingRule { zoom_drop, distance, draws_lines: true, draws_labels: true, raster_size: None, terrain_zoom_drop: 0 }
    }
}

pub const ZOOM_DROP_RANGE: (i32, i32) = (0, 8);
pub const DISTANCE_RANGE: (f64, f64) = (0.0, 256.0);
pub const TERRAIN_ZOOM_DROP_RANGE: (i32, i32) = (0, 8);
pub const RASTER_SIZES: [u32; 5] = [64, 128, 256, 512, 1024];

/// The rules of a frame with no rule of its own.
pub fn default_rules() -> Vec<FlatRingRule> {
    let band = |zoom_drop, distance, raster_size, terrain_zoom_drop| FlatRingRule { zoom_drop, distance, draws_lines: false, draws_labels: false, raster_size: Some(raster_size), terrain_zoom_drop };
    vec![FlatRingRule::new(0, 1.0), band(1, 2.0, 128, 2), band(2, 3.0, 128, 1), band(4, 9.0, 64, 3)]
}

/// The nearest size a texture band is baked at, the first of two equally near.
pub fn normalized_raster_size(size: Option<u32>) -> Option<u32> {
    let size = size? as i64;
    let mut best: Option<u32> = None;
    for candidate in RASTER_SIZES {
        if best.is_none_or(|best| (candidate as i64 - size).abs() < (best as i64 - size).abs()) {
            best = Some(candidate);
        }
    }
    best
}

/// The rules as the coverage reads them: every value within its range, the
/// rules nearest first, one rule per distance (the first given), and the
/// default rules for none.
pub fn normalized(rules: &[FlatRingRule]) -> Vec<FlatRingRule> {
    let mut cleaned: Vec<FlatRingRule> = rules
        .iter()
        .map(|rule| FlatRingRule {
            zoom_drop: rule.zoom_drop.clamp(ZOOM_DROP_RANGE.0, ZOOM_DROP_RANGE.1),
            distance: rule.distance.max(DISTANCE_RANGE.0).min(DISTANCE_RANGE.1),
            draws_lines: rule.draws_lines,
            draws_labels: rule.draws_labels,
            raster_size: normalized_raster_size(rule.raster_size),
            terrain_zoom_drop: rule.terrain_zoom_drop.clamp(TERRAIN_ZOOM_DROP_RANGE.0, TERRAIN_ZOOM_DROP_RANGE.1),
        })
        .collect();
    // A stable sort, as the first of two rules at one distance is the one kept.
    cleaned.sort_by(|lhs, rhs| lhs.distance.total_cmp(&rhs.distance));
    let mut unique: Vec<FlatRingRule> = Vec::with_capacity(cleaned.len());
    for rule in cleaned {
        if unique.last().is_none_or(|last| last.distance < rule.distance) {
            unique.push(rule);
        }
    }
    if unique.is_empty() { default_rules() } else { unique }
}

/// What a rule drew: its zoom, its last ring, its tile count and the zoom of its terrain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlatRingBand {
    pub zoom: i32,
    pub distance: f64,
    pub tile_count: usize,
    pub terrain_zoom: i32,
}
