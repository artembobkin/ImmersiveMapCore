// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Where a tile's route signs stand, a port of the Swift
//! `RouteShieldPlacement`. The road reader offers a candidate every
//! `CANDIDATE_STEP_TILE_UNITS` along every signed road, and the tile keeps,
//! per group of identical signs, the candidates nearest the tile's centre
//! that stand the style's spacing apart.

use crate::geom::V2;

/// The distance between two candidates along a road: a sixteenth of the tile.
pub const CANDIDATE_STEP_TILE_UNITS: f32 = 256.0;
/// A tile's side in layout points at the reference scale.
pub const TILE_SCREEN_POINT_SIZE: f32 = 256.0;

/// The candidates along a line in tile units, every step from half a step in,
/// the points outside the tile skipped.
pub fn candidate_points(points: &[V2], tile_extent: f32) -> Vec<V2> {
    if points.len() < 2 {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    let mut next_distance = CANDIDATE_STEP_TILE_UNITS * 0.5;
    let mut travelled = 0f32;
    for pair in points.windows(2) {
        let start = pair[0];
        let segment = pair[1] - start;
        let length = segment.length();
        if length <= 0.0 {
            continue;
        }
        while next_distance <= travelled + length {
            let point = start + segment * ((next_distance - travelled) / length);
            if point.x >= 0.0 && point.y >= 0.0 && point.x <= tile_extent && point.y <= tile_extent {
                candidates.push(point);
            }
            next_distance += CANDIDATE_STEP_TILE_UNITS;
        }
        travelled += length;
    }
    candidates
}

/// What selection needs of a candidate.
pub trait ShieldCandidate {
    fn group(&self) -> &str;
    fn position(&self) -> V2;
    fn spacing_points(&self) -> f32;
}

/// The candidates kept, per group in the order the groups were first offered:
/// nearest the tile's centre first, each at least the style's spacing from
/// every one kept before it.
pub fn select<C: ShieldCandidate + Clone>(candidates: &[C], tile_extent: f32) -> Vec<C> {
    let mut group_order: Vec<&str> = Vec::new();
    let mut by_group: std::collections::HashMap<&str, Vec<&C>> = std::collections::HashMap::new();
    for c in candidates {
        let entry = by_group.entry(c.group()).or_insert_with(|| {
            group_order.push(c.group());
            Vec::new()
        });
        entry.push(c);
    }
    let centre = V2::new(tile_extent * 0.5, tile_extent * 0.5);
    let units_per_point = tile_extent / TILE_SCREEN_POINT_SIZE;
    let mut selected = Vec::new();
    for group in group_order {
        let mut ordered = by_group[group].clone();
        ordered.sort_by(|a, b| {
            let da = (a.position() - centre).length_squared();
            let db = (b.position() - centre).length_squared();
            da.total_cmp(&db)
                .then(a.position().x.total_cmp(&b.position().x))
                .then(a.position().y.total_cmp(&b.position().y))
        });
        let mut kept: Vec<V2> = Vec::new();
        for c in ordered {
            let spacing = c.spacing_points() * units_per_point;
            let sq = spacing * spacing;
            if kept.iter().all(|&k| (k - c.position()).length_squared() >= sq) {
                kept.push(c.position());
                selected.push(c.clone());
            }
        }
    }
    selected
}
