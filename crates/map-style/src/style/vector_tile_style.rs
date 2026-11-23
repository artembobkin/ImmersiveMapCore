// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use crate::schema::{Feature, FeatureFacts, FeatureGeometry, FeatureProperties};
use crate::style::default::{ProtomapsBasemapTheme, default_theme};
use crate::style::{BaseColors, FeatureStyle, FillStyle, Hillshade, LabelOpacity, TerrainOcclusion, TreePalette};
use crate::support::Tile;
use crate::Float4;

/// Everything a style is told about one feature before it says how the
/// feature draws: the feature as the tile carries it, the facts the schema
/// reading made of it, and the style's own identity.
#[derive(Debug, Clone, Copy)]
pub struct FeatureStyleContext<'c> {
    /// The style's identity, the namespace its label identities are minted
    /// in.
    pub style_id: &'c str,
    pub layer_name: &'c str,
    pub tile: Tile,
    pub geometry: FeatureGeometry,
    pub properties: FeatureProperties<'c, 'c>,
    /// What the feature is, as the schema reading says (`TileSchema`): a
    /// road in a tunnel, a building of some height. A style draws by these
    /// rather than by reading the tags again.
    pub facts: &'c FeatureFacts<'c>,
}

impl<'c> FeatureStyleContext<'c> {
    /// The context of a feature and the facts read from it.
    pub fn new(style_id: &'c str, feature: &Feature<'c, 'c>, facts: &'c FeatureFacts<'c>) -> Self {
        FeatureStyleContext {
            style_id,
            layer_name: feature.layer_name,
            tile: feature.tile,
            geometry: feature.geometry,
            properties: feature.properties,
            facts,
        }
    }

    pub fn tile_zoom(&self) -> i32 {
        self.tile.z
    }
}

/// The bodies of water the parser can name on its own at the coarse zooms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WaterNameKind {
    Ocean,
    Sea,
}

/// The label identity namespace of a style that declares none of its own
/// (`VectorTileStyle::style_id`).
pub const GENERIC_STYLE_ID: &str = "vector";

/// A map style: given a feature and what it is, how it draws. Every style
/// the engine draws with goes through this trait, the built-in one
/// included.
///
/// The parser asks the style for every feature of every layer. What a
/// feature is (a building of some height, a road in a tunnel) is the schema
/// reading's answer (`TileSchema`), handed to the style in the context's
/// `facts`. The style answers only with the look, a `FeatureStyle`:
/// colours, widths, dashes, the passes a road draws in, the text style and
/// rank of a label. The factories on `FeatureStyle` (`polygon`, `line`,
/// `extruded_polygon`, `point_label` and the rest) cover the common drawing
/// modes. The full value is there for a style that needs every knob the
/// built-in one has. The label text itself is not the style's: the engine
/// reads it in the map's language, from the names the schema reading
/// states (`LabelFacts`).
pub trait VectorTileStyle: Send + Sync {
    /// Folded into the prepared-tile cache identity: any change to the
    /// rules or the palette must change it, or the map keeps drawing from
    /// tiles prepared by the old style.
    fn cache_fingerprint(&self) -> u32;

    /// The colours of what no tile paints: the map where no tile has
    /// arrived and the two polar caps. The default is the built-in theme's.
    fn base_colors(&self) -> BaseColors {
        default_theme().base_colors()
    }

    /// How the slopes are shaded where the ground relief stands. Drawn at
    /// frame time, not part of the cache fingerprint. The default is the
    /// built-in theme's.
    fn hillshade(&self) -> Hillshade {
        default_theme().hillshade.clone()
    }

    /// How the ground is darkened where the terrain hides the sky, where
    /// the ground relief stands. Drawn at frame time, not part of the cache
    /// fingerprint. The default is the built-in theme's.
    fn terrain_occlusion(&self) -> TerrainOcclusion {
        default_theme().terrain_occlusion
    }

    /// The colours of the trees on the woods and the parks. Drawn at frame
    /// time, not part of the cache fingerprint. The default is the built-in
    /// theme's.
    fn tree_palette(&self) -> TreePalette {
        default_theme().tree_palette()
    }

    /// How opaque the names and the POIs are by the zoom, over each label's
    /// own opacity. Drawn at frame time, not part of the cache fingerprint.
    /// The default is the built-in theme's.
    fn label_opacity(&self) -> LabelOpacity {
        default_theme().label_opacity.clone()
    }

    /// The style's identity: the namespace its label identities are minted
    /// in, so two styles never share a label across tiles. The default is
    /// one shared namespace for styles that state none.
    fn style_id(&self) -> &str {
        GENERIC_STYLE_ID
    }

    /// Whether a label keeps one identity across tiles by its feature id, so
    /// a feature that straddles a tile edge is one label with one fade.
    /// False identifies labels by tile, layer, text and anchor.
    fn labels_use_feature_identity(&self) -> bool {
        true
    }

    fn make_style(&self, feature: &FeatureStyleContext<'_>) -> FeatureStyle;

    /// The full-tile quad the parser puts under every feature of a tile,
    /// which is what paints the land where the schema ships no land
    /// polygon. Asked per tile, since the colour may follow the zoom. The
    /// default is transparent, under a key below the range a style normally
    /// uses, so the tile background colour shows.
    fn background_style(&self, tile_zoom: i32) -> FeatureStyle {
        let _ = tile_zoom;
        FeatureStyle::Fill(FillStyle::new(1, [0.0, 0.0, 0.0, 0.0]))
    }

    /// The style of a water name the parser adds itself at zooms 0 to 2,
    /// for a schema whose tiles carry the ocean and sea names unreliably.
    /// `None`, the default, adds none.
    fn water_name_style(&self, kind: WaterNameKind, tile_zoom: i32) -> Option<FeatureStyle> {
        let _ = (kind, tile_zoom);
        None
    }

    /// The theme of a style that is the built-in one, for an app that reads
    /// a theme value back out of its settings. `None` for any other style.
    fn basemap_theme(&self) -> Option<&ProtomapsBasemapTheme> {
        None
    }
}

/// One colour for everything: the placeholder a source starts with before
/// it has a style.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BasicVectorTileStyle {
    pub cache_fingerprint: u32,
    pub fallback_color: Float4,
}

impl Default for BasicVectorTileStyle {
    fn default() -> Self {
        BasicVectorTileStyle { cache_fingerprint: 1, fallback_color: [1.0, 0.0, 0.0, 1.0] }
    }
}

impl BasicVectorTileStyle {
    pub fn new(cache_fingerprint: u32) -> Self {
        BasicVectorTileStyle { cache_fingerprint, ..BasicVectorTileStyle::default() }
    }
}

impl VectorTileStyle for BasicVectorTileStyle {
    fn cache_fingerprint(&self) -> u32 {
        self.cache_fingerprint
    }

    fn make_style(&self, _feature: &FeatureStyleContext<'_>) -> FeatureStyle {
        FeatureStyle::polygon(2, self.fallback_color, crate::style::ZoomFade::NONE)
    }
}
