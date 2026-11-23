// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! What a feature is. The `TileSchema` trait, the facts ([`FeatureFacts`],
//! one case per kind of thing, carrying [`RoadFacts`],
//! [`BuildingExtrusion`] or [`LabelFacts`]), the typed property accessors,
//! and the built-in reading of the Protomaps basemap in [`default`]. No
//! drawing, no colours, no GPU, no parsing.

pub mod default;
mod facts;
mod properties;

pub use facts::{BuildingExtrusion, FeatureFacts, LabelFacts, NamesByLanguage, RoadFacts, RoadStructure, RouteFacts};
pub use properties::{Attributes, FeatureProperties};
pub(crate) use properties::integer_value as properties_integer;

use crate::support::Tile;

/// The geometry a feature carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FeatureGeometry {
    Point,
    Line,
    Polygon,
    #[default]
    Unknown,
}

impl From<mvt::GeometryType> for FeatureGeometry {
    fn from(geometry: mvt::GeometryType) -> Self {
        match geometry {
            mvt::GeometryType::Point => FeatureGeometry::Point,
            mvt::GeometryType::LineString => FeatureGeometry::Line,
            mvt::GeometryType::Polygon => FeatureGeometry::Polygon,
            mvt::GeometryType::Unknown => FeatureGeometry::Unknown,
        }
    }
}

/// One feature as the tile carries it: which layer, which tile, which
/// geometry kind, and its properties. What the schema reading is given.
#[derive(Debug, Clone, Copy)]
pub struct Feature<'p, 'a> {
    pub layer_name: &'a str,
    pub tile: Tile,
    pub geometry: FeatureGeometry,
    pub properties: FeatureProperties<'p, 'a>,
}

impl<'p, 'a> Feature<'p, 'a> {
    pub fn new(
        layer_name: &'a str,
        tile: Tile,
        geometry: impl Into<FeatureGeometry>,
        properties: &'p [(&'a str, mvt::Value<'a>)],
    ) -> Self {
        Feature { layer_name, tile, geometry: geometry.into(), properties: FeatureProperties::new(properties) }
    }
}

/// An OpenStreetMap element: how an app names a feature of the map
/// independently of the tiles, for example the building a model of a model
/// archive replaces (`Relation(3334755)` is the Bolshoi Theatre). Which
/// tile feature that is, the schema answers (`TileSchema::tile_feature_id`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OsmElement {
    Node(u64),
    Way(u64),
    Relation(u64),
}

/// The reading of a tile schema: what a feature is, in the engine's own
/// words. Every schema names things differently (a road in a tunnel is
/// `brunnel=tunnel` in one, `structure=tunnel` or `layer=-1` in another),
/// and this is the one place a schema's spelling is turned into facts: a
/// road with a structure, a layer and a name, a building with its heights,
/// a point with a name in several languages.
///
/// One question, asked per feature. What the parser decides per layer it
/// derives from the answers: every layer whose features are roads is a
/// road layer, and the road layers of a tile merge into one before they
/// are read, so the roads of a tile stitch and count junctions as one
/// network whichever layer each came from.
///
/// The engine's geometry work reads the facts and nothing else: which draw
/// phase a road takes, where a street stitches, what rises as a building,
/// which spelling of a name the map's language shows. The style reads the
/// same facts next to the properties and answers only with how the feature
/// looks. Neither the engine nor the style reads a tag by name to learn
/// what a feature is.
pub trait TileSchema: Send + Sync {
    /// Folded into the prepared-tile cache identity: any change to the
    /// reading must change it, or the map keeps drawing from tiles prepared
    /// under the old reading.
    fn cache_fingerprint(&self) -> u32;

    /// What the feature is. `FeatureFacts::None` for a feature that is none
    /// of the things the facts describe: a ground fill, a border, a river.
    fn read<'a>(&self, feature: &Feature<'_, 'a>) -> FeatureFacts<'a>;

    /// The id the tiles give the features made from an OSM element, `None`
    /// when the tiles carry no OSM identity. Every tile and every zoom the
    /// element appears in carries the same id, which is how the engine
    /// finds the building a model of a model archive replaces. The default
    /// is a schema whose tiles carry no OSM identity: no building can be
    /// replaced by a model.
    fn tile_feature_id(&self, element: OsmElement) -> Option<u64> {
        let _ = element;
        None
    }
}
