// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! What the separate-road path knows about a whole road layer before any of
//! its features is read: every line stitched into streets and the
//! connections counted. A port of the Swift `RoadLayerPrecomputation`,
//! `PreparedRoadLine` and `RoadLayerContext`.

use std::collections::{HashMap, HashSet};

use map_style::style::{FeatureStyle, RoadTier};
use mvt::GeometryType;
use mvt::geometry::LineString;

use super::RoadConnectionPointKey;
use super::street_stitcher::{StitchFacts, stitch};
use crate::coordinate_space::TILE_EXTENT;
use crate::geom::V2;
use crate::parse_line::{ClippedLineFragment, LineClipper};
use crate::reading::PreparedLayer;
use crate::tile::Tile;

/// A road line decoded, converted and exact-clipped once: the pre-pass
/// counts shared endpoints from it and the line reader tessellates from it,
/// so the geometry work is not repeated per pass.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedRoadLine {
    pub points: Vec<V2>,
    pub exact_fragments: Vec<ClippedLineFragment>,
}

/// Built once per road layer at street zooms, and empty for every other
/// layer.
#[derive(Debug, Clone, Default)]
pub struct RoadLayerPrecomputation {
    pub shared_point_counts: HashMap<RoadConnectionPointKey, usize>,
    /// The nodes where a road continues as another piece of the same style:
    /// for each point, the road identity keys (`RoadStyle::key`) of which two
    /// or more features END there. The pieces the tiles ship cut at such a
    /// node draw as separate ribbons whenever the stitcher leaves them apart
    /// (no name on the geometry, a third road meeting at the node, a footway
    /// of the pedestrian tier), and each ends in a hard butt cut square to
    /// its own last segment. Where the two directions differ, the cuts leave
    /// a wedge of ground open on the outside of the bend. The line reader
    /// rounds a connected end at one of these nodes off with a cap: the
    /// overlap of two ribbons of one style is invisible, since the two are
    /// one rank and a pixel takes one fragment of a rank, and the wedge is
    /// filled at any angle. A node shared with a road of another style is a
    /// junction, and its ends stay square.
    pub continuation_keys_by_point: HashMap<RoadConnectionPointKey, HashSet<u8>>,
    pub lines_by_feature_index: Vec<Vec<PreparedRoadLine>>,
}

impl RoadLayerPrecomputation {
    pub fn empty() -> RoadLayerPrecomputation {
        RoadLayerPrecomputation::default()
    }

    pub fn build(layer: &PreparedLayer<'_>) -> RoadLayerPrecomputation {
        let clipper = LineClipper::new();
        let count = layer.features.len();
        // Every feature as a road (its identity key and tier), `None` for one
        // that draws as none (hidden, a fill, a label): those take no part
        // in the road work.
        let roads: Vec<Option<(u8, RoadTier)>> = layer.features.iter().map(|f| road_identity(&f.style)).collect();

        let mut raw_lines: Vec<Vec<Vec<V2>>> = vec![Vec::new(); count];
        for (index, read) in layer.features.iter().enumerate() {
            if roads[index].is_none() || read.feature.geometry_type != GeometryType::LineString {
                continue;
            }
            raw_lines[index] = layer.geometry.lines(&read.feature).iter().map(float_points).collect();
        }

        // Pieces of one street that the tiles ship cut are stitched end to
        // end before tessellation, so the street is one ribbon with no seam
        // where the pieces met. Stitching needs a street identity on the
        // geometry (the schema's stitching key) on a road of the automobile
        // tier. Without it the pieces draw as they arrive.
        let facts: Vec<StitchFacts<'_>> = layer
            .features
            .iter()
            .zip(&roads)
            .map(|(read, road)| StitchFacts {
                stitching_key: read.facts.road().and_then(|r| r.stitching_key.as_deref()),
                is_automobile: road.is_some_and(|(_, tier)| tier == RoadTier::Automobile),
            })
            .collect();
        let stitched = stitch(raw_lines, &facts);

        let mut point_counts: HashMap<RoadConnectionPointKey, usize> = HashMap::new();
        // How many features of each road identity end at each point, from the
        // raw polyline ends: an end the tile clip moves onto the boundary is a
        // boundary continuation, which the reader keeps square for the
        // neighbouring tile to meet.
        let mut end_counts_by_point: HashMap<RoadConnectionPointKey, HashMap<u8, usize>> = HashMap::new();
        let mut lines_by_feature_index: Vec<Vec<PreparedRoadLine>> = vec![Vec::new(); count];
        for (index, lines) in stitched.into_iter().enumerate() {
            if lines.is_empty() {
                continue;
            }
            let identity_key = roads[index].map(|(key, _)| key);
            let mut prepared = Vec::with_capacity(lines.len());
            for points in lines {
                let fragments = clipper.clip(&points, TILE_EXTENT, 0.0);
                for fragment in &fragments {
                    for &point in &fragment.points {
                        *point_counts.entry(RoadConnectionPointKey::new(point)).or_insert(0) += 1;
                    }
                }
                if let (Some(key), Some(&first), Some(&last)) = (identity_key, points.first(), points.last()) {
                    *end_counts_by_point.entry(RoadConnectionPointKey::new(first)).or_default().entry(key).or_insert(0) += 1;
                    *end_counts_by_point.entry(RoadConnectionPointKey::new(last)).or_default().entry(key).or_insert(0) += 1;
                }
                prepared.push(PreparedRoadLine { points, exact_fragments: fragments });
            }
            lines_by_feature_index[index] = prepared;
        }

        let continuation_keys_by_point = end_counts_by_point
            .into_iter()
            .filter_map(|(point, counts)| {
                let keys: HashSet<u8> = counts.into_iter().filter(|&(_, n)| n >= 2).map(|(key, _)| key).collect();
                (!keys.is_empty()).then_some((point, keys))
            })
            .collect();

        RoadLayerPrecomputation { shared_point_counts: point_counts, continuation_keys_by_point, lines_by_feature_index }
    }
}

/// A feature's road identity key and tier, as the Swift `roadStyle` gives
/// them: a road, or a plain line read as a road of one fill stroke.
fn road_identity(style: &FeatureStyle) -> Option<(u8, RoadTier)> {
    match style {
        FeatureStyle::Road(road) => Some((road.key(), road.tier)),
        FeatureStyle::Line(line) => Some((line.pass.key, RoadTier::default())),
        _ => None,
    }
}

pub fn float_points(line: &LineString) -> Vec<V2> {
    line.iter().map(|p| V2::new(p.x as f32, p.y as f32)).collect()
}

/// What the readers know about the layer a feature comes from, decided once
/// per layer: whether it takes the separate-road path, and the pre-pass.
#[derive(Debug, Clone, Default)]
pub struct RoadLayerContext {
    /// The separate-road path: seamless ribbons with the casing under the
    /// fill, sorted by structure and class. Taken where the style answered a
    /// road for any feature of the layer. A layer of plain lines stays on
    /// the ground path.
    pub uses_separate_road_rendering: bool,
    pub precomputation: RoadLayerPrecomputation,
}

impl RoadLayerContext {
    /// The layer's context, as the Swift `TileMvtParser` decides it.
    pub fn build(layer: &PreparedLayer<'_>, _tile: Tile) -> RoadLayerContext {
        let uses_separate_road_rendering = layer.features.iter().any(|f| matches!(f.style, FeatureStyle::Road(_)));
        RoadLayerContext {
            uses_separate_road_rendering,
            precomputation: if uses_separate_road_rendering { RoadLayerPrecomputation::build(layer) } else { RoadLayerPrecomputation::empty() },
        }
    }
}
