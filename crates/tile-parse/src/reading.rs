// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The reading stage's view of a tile: its layers with every feature's
//! attributes, facts and style resolved once (the Swift `TileMvtParser`'s
//! `PreparedLayer`), as the feature readers take them. The road layers of a
//! tile are merged into one prepared layer, so the roads are one feature list
//! for stitching and junction counting.

use map_style::schema::{Feature as SchemaFeature, FeatureFacts};
use map_style::style::{FeatureStyle, MapStyleRuntime};
use mvt::Value;

use crate::layer_geometry::LayerGeometry;
use crate::tile::Tile;

/// One feature with what the reading stage resolved for it.
pub struct ReadFeature<'t> {
    pub feature: mvt::Feature<'t>,
    /// The name of the layer the feature came from: a merged road layer holds several.
    pub layer_name: &'t str,
    pub attributes: Vec<(&'t str, Value<'t>)>,
    pub facts: FeatureFacts<'t>,
    pub style: FeatureStyle,
}

impl<'t> ReadFeature<'t> {
    /// The feature's id, 0 where the tile gave none (as the Swift decoder reads it).
    pub fn id(&self) -> u64 {
        self.feature.id.unwrap_or(0)
    }

    /// The feature as the schema and the style read it.
    pub fn schema_feature<'p>(&'p self, tile: Tile) -> SchemaFeature<'p, 't> {
        SchemaFeature::new(self.layer_name, to_style_tile(tile), self.feature.geometry_type, &self.attributes)
    }
}

/// A layer of the tile, or the tile's road layers merged.
pub struct PreparedLayer<'t> {
    pub name: &'t str,
    pub extent: u32,
    pub geometry: LayerGeometry,
    pub features: Vec<ReadFeature<'t>>,
    /// Any feature the reading found to be a road.
    pub has_roads: bool,
}

pub fn to_style_tile(tile: Tile) -> map_style::support::Tile {
    map_style::support::Tile { x: tile.x, y: tile.y, z: tile.z }
}

/// Attributes, facts and style for every feature of every layer, then the
/// road layers merged into the first of them. A road layer whose extent
/// differs from the first's stays on its own, since its coordinates would
/// not line up. Features the decoder cannot read are skipped.
pub fn prepare_layers<'t>(decoded: &mvt::Tile<'t>, tile: Tile, style: &MapStyleRuntime) -> Vec<PreparedLayer<'t>> {
    let mut merged: Vec<PreparedLayer<'t>> = Vec::with_capacity(decoded.layers.len());
    let mut road_position: Option<usize> = None;
    for layer in &decoded.layers {
        let mut features = Vec::with_capacity(layer.feature_count());
        let mut has_roads = false;
        for feature in layer.features().flatten() {
            let attributes = layer.attributes(&feature);
            let schema = SchemaFeature::new(layer.name, to_style_tile(tile), feature.geometry_type, &attributes);
            let facts = style.read_facts(&schema);
            has_roads = has_roads || facts.road().is_some();
            features.push(ReadFeature { feature, layer_name: layer.name, attributes, facts, style: FeatureStyle::Hidden });
        }
        let prepared = PreparedLayer { name: layer.name, extent: layer.extent, geometry: LayerGeometry::new(layer.extent), features, has_roads };
        if prepared.has_roads {
            if let Some(position) = road_position {
                if merged[position].extent == prepared.extent {
                    merged[position].features.extend(prepared.features);
                    continue;
                }
            } else {
                road_position = Some(merged.len());
            }
        }
        merged.push(prepared);
    }
    // Styled under the prepared layer's name, as the Swift parser does: a
    // merged road layer styles every road under the first road layer's name.
    for layer in &mut merged {
        let name = layer.name;
        for f in &mut layer.features {
            let schema = SchemaFeature::new(name, to_style_tile(tile), f.feature.geometry_type, &f.attributes);
            f.style = style.make_style(&schema, &f.facts);
        }
    }
    merged
}
