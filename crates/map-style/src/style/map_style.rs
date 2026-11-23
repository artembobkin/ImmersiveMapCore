// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use std::sync::Arc;

use crate::schema::default::ProtomapsBasemapSchema;
use crate::schema::{Feature, FeatureFacts, TileSchema};
use crate::style::{
    BaseColors, FeatureStyle, FeatureStyleContext, FillStyle, Hillshade, LabelOpacity, TerrainOcclusion, TreePalette,
    VectorTileStyle, WaterNameKind,
};
use crate::support::Tile;

/// A map style as an app configures it: the reading of the tile schema
/// (what every feature is), the vector tile style (how every feature
/// draws), and a fingerprint of the whole configuration that the disk
/// caches are keyed by.
pub trait MapStyle: Send + Sync {
    fn configuration_fingerprint(&self) -> u64;
    fn schema(&self) -> Arc<dyn TileSchema>;
    fn vector_tile_style(&self) -> Arc<dyn VectorTileStyle>;
}

/// A map style of any kind, compared by its configuration fingerprint.
#[derive(Clone)]
pub struct AnyMapStyle {
    configuration_fingerprint: u64,
    schema: Arc<dyn TileSchema>,
    vector_tile_style: Arc<dyn VectorTileStyle>,
}

impl AnyMapStyle {
    pub fn new<S: MapStyle + ?Sized>(map_style: &S) -> Self {
        AnyMapStyle {
            configuration_fingerprint: map_style.configuration_fingerprint(),
            schema: map_style.schema(),
            vector_tile_style: map_style.vector_tile_style(),
        }
    }

    pub fn configuration_fingerprint(&self) -> u64 {
        self.configuration_fingerprint
    }

    pub fn schema(&self) -> &Arc<dyn TileSchema> {
        &self.schema
    }

    pub fn vector_tile_style(&self) -> &Arc<dyn VectorTileStyle> {
        &self.vector_tile_style
    }

    /// The theme of a map style that is the built-in one, for an app that
    /// reads a theme value back out of its settings. `None` for any other
    /// style.
    pub fn basemap_theme(&self) -> Option<&crate::style::default::ProtomapsBasemapTheme> {
        self.vector_tile_style.basemap_theme()
    }
}

impl PartialEq for AnyMapStyle {
    fn eq(&self, other: &Self) -> bool {
        self.configuration_fingerprint == other.configuration_fingerprint
    }
}

impl std::fmt::Debug for AnyMapStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnyMapStyle").field("configuration_fingerprint", &self.configuration_fingerprint).finish()
    }
}

/// Draws any MVT source with a hand-written per-feature style, over the
/// reading of its schema. The usual reading is the Protomaps basemap's
/// (`VectorTileMapStyle::with_basemap_schema`), which covers a source that
/// spells its tags the same way. A source that names things differently
/// pairs the style with a reading of its own.
#[derive(Clone)]
pub struct VectorTileMapStyle {
    configuration_fingerprint: u64,
    schema: Arc<dyn TileSchema>,
    vector_tile_style: Arc<dyn VectorTileStyle>,
}

impl VectorTileMapStyle {
    /// The style over a reading. The fingerprint is the reading's in the
    /// upper 32 bits and the style's in the lower, unless one is given.
    pub fn new(
        style: Arc<dyn VectorTileStyle>,
        schema: Arc<dyn TileSchema>,
        configuration_fingerprint: Option<u64>,
    ) -> Self {
        let fingerprint = configuration_fingerprint
            .unwrap_or(((schema.cache_fingerprint() as u64) << 32) | style.cache_fingerprint() as u64);
        VectorTileMapStyle { configuration_fingerprint: fingerprint, schema, vector_tile_style: style }
    }

    /// The style over the Protomaps basemap's reading.
    pub fn with_basemap_schema(style: Arc<dyn VectorTileStyle>) -> Self {
        Self::new(style, Arc::new(ProtomapsBasemapSchema), None)
    }
}

impl MapStyle for VectorTileMapStyle {
    fn configuration_fingerprint(&self) -> u64 {
        self.configuration_fingerprint
    }

    fn schema(&self) -> Arc<dyn TileSchema> {
        self.schema.clone()
    }

    fn vector_tile_style(&self) -> Arc<dyn VectorTileStyle> {
        self.vector_tile_style.clone()
    }
}

/// Everything the runtime derives from the configured map style: the
/// schema reading, the style itself and the base colors. The tile source
/// contributes nothing here: it is only a URL the loader fetches bytes
/// from.
///
/// This is the parser's view of the reading and the style: it asks the
/// schema what a feature is, builds the public `FeatureStyleContext` from
/// the feature and its facts, asks the style how it draws, and adds what
/// the engine decides on its own (the debug frame, the cache identity the
/// settings contribute). Every style goes through here, the built-in one
/// included.
///
/// The `style` module: the public style API (`MapStyle`,
/// `VectorTileStyle`, `FeatureStyle`, the building and road readings), this
/// runtime, and the built-in style in `default`. Everything about
/// interpreting a tile's bytes lives here. Nothing about fetching them
/// does. No GPU, no parsing, no networking.
#[derive(Clone)]
pub struct MapStyleRuntime {
    pub schema: Arc<dyn TileSchema>,
    pub style: Arc<dyn VectorTileStyle>,
    /// What no tile paints, as the style states it.
    pub base_colors: BaseColors,
    /// How the slopes are shaded where the relief stands, as the style
    /// states it.
    pub hillshade: Hillshade,
    /// How the ground is darkened where the terrain hides the sky, as the
    /// style states it.
    pub terrain_occlusion: TerrainOcclusion,
    /// The colours of the trees, as the style states them.
    pub tree_palette: TreePalette,
    /// How opaque the names and the POIs are by the zoom, as the style
    /// states it.
    pub label_opacity: LabelOpacity,
    /// The style's identity, the namespace its label identities are minted
    /// in.
    pub style_id: String,
}

impl MapStyleRuntime {
    pub fn new(map_style: &AnyMapStyle) -> Self {
        Self::with_overrides(map_style, None, None)
    }

    /// `style` and `schema` replace the map style's own, which is how a test
    /// runs the parser against a style or a reading of its own.
    pub fn with_overrides(
        map_style: &AnyMapStyle,
        style: Option<Arc<dyn VectorTileStyle>>,
        schema: Option<Arc<dyn TileSchema>>,
    ) -> Self {
        let style = style.unwrap_or_else(|| map_style.vector_tile_style().clone());
        let schema = schema.unwrap_or_else(|| map_style.schema().clone());
        MapStyleRuntime {
            style_id: style.style_id().to_owned(),
            base_colors: style.base_colors(),
            hillshade: style.hillshade(),
            terrain_occlusion: style.terrain_occlusion(),
            tree_palette: style.tree_palette(),
            label_opacity: style.label_opacity(),
            schema,
            style,
        }
    }

    /// The style's part of the prepared-tile cache identity: the reading's
    /// and the style's own fingerprints.
    pub fn prepared_tile_style_revision(&self) -> u32 {
        self.schema.cache_fingerprint().wrapping_mul(31).wrapping_add(self.style.cache_fingerprint())
    }

    /// What a feature is, as the schema reading says.
    pub fn read_facts<'a>(&self, feature: &Feature<'_, 'a>) -> FeatureFacts<'a> {
        self.schema.read(feature)
    }

    /// How a feature draws, given what the reading said it is.
    pub fn make_style(&self, feature: &Feature<'_, '_>, facts: &FeatureFacts<'_>) -> FeatureStyle {
        self.style.make_style(&FeatureStyleContext::new(&self.style_id, feature, facts))
    }

    pub fn background_style(&self, tile: Tile) -> FeatureStyle {
        self.style.background_style(tile.z)
    }

    pub fn water_name_style(&self, kind: WaterNameKind, tile: Tile) -> Option<FeatureStyle> {
        self.style.water_name_style(kind, tile.z)
    }

    /// The one-unit frame around a tile the parser draws when the settings
    /// ask for the debug borders: the engine's, in a debug red, whatever the
    /// style.
    pub fn debug_border_style(&self) -> FillStyle {
        FillStyle::new(0, [1.0, 0.0, 0.0, 1.0])
    }
}
