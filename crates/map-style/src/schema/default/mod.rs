// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The reading of the Protomaps basemap schema.

mod readings;

pub(crate) use readings::trim_whitespaces;

use crate::schema::{FeatureFacts, FeatureGeometry, OsmElement, TileSchema};

use super::Feature;

/// The reading of the Protomaps basemap schema (tiles version 4): the
/// `roads` layer's structure flags, names and routes, the `buildings`
/// layer's heights and house numbers, the names of `places`, `pois` and the
/// water label points of `water`. A source that spells its tags the same
/// way can use it as it is.
///
/// Every other layer of the basemap (`earth`, `landcover`, `landuse`,
/// `boundaries`, `transit`, the water fills and the river lines) is none of
/// the things the facts describe: the style draws them from their
/// properties alone. A layer the basemap does not ship reads as nothing
/// too, so a future layer is opted in here rather than labelled by
/// accident.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProtomapsBasemapSchema;

impl ProtomapsBasemapSchema {
    /// Bumped when the reading changes: every prepared tile is prepared
    /// again under the new reading. 4: a `building_part` whose
    /// `kind_detail` is `no` is not raised.
    pub const CACHE_FINGERPRINT: u32 = 4;

    pub fn new() -> Self {
        ProtomapsBasemapSchema
    }
}

impl TileSchema for ProtomapsBasemapSchema {
    fn cache_fingerprint(&self) -> u32 {
        Self::CACHE_FINGERPRINT
    }

    /// The basemap's feature id is the OSM element: the element type in the
    /// bits from 44 up (1 node, 2 way, 3 relation) and the OSM id below. A
    /// building and a point made from the same element share it.
    fn tile_feature_id(&self, element: OsmElement) -> Option<u64> {
        const OSM_ID_LIMIT: u64 = 1 << 44;
        let (kind, id) = match element {
            OsmElement::Node(id) => (1u64, id),
            OsmElement::Way(id) => (2, id),
            OsmElement::Relation(id) => (3, id),
        };
        if id < OSM_ID_LIMIT { Some(kind << 44 | id) } else { None }
    }

    fn read<'a>(&self, feature: &Feature<'_, 'a>) -> FeatureFacts<'a> {
        let properties = &feature.properties;
        match feature.layer_name.to_lowercase().as_str() {
            "roads" => {
                // The basemap ships roads as lines only. A polygon under the
                // layer name is not a road the engine can stitch or sort.
                if feature.geometry == FeatureGeometry::Polygon {
                    return FeatureFacts::None;
                }
                FeatureFacts::Road(self.road(properties))
            }
            "buildings" => {
                // The address points (`kind=address`) of z15 carry a house
                // number and no footprint: a label with the number as its
                // text.
                if feature.geometry == FeatureGeometry::Point {
                    return self.house_number(properties).map(FeatureFacts::Labelled).unwrap_or_default();
                }
                if properties.string("kind") == Some("address") {
                    return FeatureFacts::None;
                }
                FeatureFacts::Building(self.building(properties))
            }
            "water" => {
                // A named body of water arrives twice: as its fill and, from
                // the zoom its area earns, as one point carrying the name.
                // The point is the label. The fill and the river lines are
                // called nothing, so the name is never laid twice.
                if matches!(feature.geometry, FeatureGeometry::Polygon | FeatureGeometry::Line) {
                    return FeatureFacts::None;
                }
                match self.names(properties) {
                    Some(mut label) => {
                        label.names_water_body = true;
                        FeatureFacts::Labelled(label)
                    }
                    None => FeatureFacts::None,
                }
            }
            "places" | "pois" => self.names(properties).map(FeatureFacts::Labelled).unwrap_or_default(),
            _ => FeatureFacts::None,
        }
    }
}
