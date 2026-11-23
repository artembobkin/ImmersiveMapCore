// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The built-in style for the Protomaps basemap: its theme
//! (`ProtomapsBasemapTheme`, with the night palette and the route signs),
//! the rules that read the theme (`ProtomapsBasemapDefaultMapStyle`, split
//! by layer family into the ground, the buildings, the roads and the
//! labels) and the map style that pairs them with the basemap's reading
//! (`ProtomapsBasemapMapStyle`).

mod buildings;
mod ground;
mod labels;
mod night;
mod roads;
mod route_shields;
mod rules;
mod theme;

use std::sync::{Arc, OnceLock};

pub use labels::LabelBand;
pub use roads::OverviewRoadStroke;
pub use route_shields::{RouteShieldRule, RouteShields};
pub use rules::ProtomapsBasemapDefaultMapStyle;
pub use theme::{
    BiomeStyles, FeatureStyles, LabelAppearance, LabelStyles, LabelVisibility, LayerStyles, OverviewRoadStyles,
    PoiCategory, PoiCategoryColors, PoiCategoryZooms, ProtomapsBasemapTheme, RoadClass, RoadClassValues,
    RoadLayerStyles, RoadMetrics, TreeStyles,
};

use crate::schema::default::ProtomapsBasemapSchema;
use crate::schema::TileSchema;
use crate::style::{MapStyle, VectorTileStyle};

/// The default theme, built once.
pub(crate) fn default_theme() -> &'static ProtomapsBasemapTheme {
    static THEME: OnceLock<ProtomapsBasemapTheme> = OnceLock::new();
    THEME.get_or_init(ProtomapsBasemapTheme::default)
}

/// The built-in style: draws the Protomaps basemap tiles of the hosted
/// archive. The default for a bare map, and `ProtomapsBasemapMapStyle::default().apply(...)`
/// is the same look with a few colours changed (see
/// `ProtomapsBasemapTheme`). A source in another schema pairs a
/// `VectorTileMapStyle` with its own schema and per-feature style instead.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProtomapsBasemapMapStyle {
    pub theme: ProtomapsBasemapTheme,
}

impl ProtomapsBasemapMapStyle {
    pub fn new(theme: ProtomapsBasemapTheme) -> Self {
        ProtomapsBasemapMapStyle { theme }
    }

    /// The built-in style in the night palette (`ProtomapsBasemapTheme::night`).
    pub fn night() -> Self {
        ProtomapsBasemapMapStyle { theme: ProtomapsBasemapTheme::night() }
    }

    /// The same style with the changes the closure makes to its theme.
    pub fn apply(&self, change: impl FnOnce(&mut ProtomapsBasemapTheme)) -> ProtomapsBasemapMapStyle {
        ProtomapsBasemapMapStyle { theme: self.theme.apply(change) }
    }
}

impl MapStyle for ProtomapsBasemapMapStyle {
    fn configuration_fingerprint(&self) -> u64 {
        self.theme.cache_fingerprint() as u64
    }

    /// The reading of the Protomaps basemap schema.
    fn schema(&self) -> Arc<dyn TileSchema> {
        Arc::new(ProtomapsBasemapSchema)
    }

    /// The rules, reading the palette and the label appearances from the
    /// theme. A public value like any other style's: an app can ask it
    /// directly what a feature draws as.
    fn vector_tile_style(&self) -> Arc<dyn VectorTileStyle> {
        Arc::new(ProtomapsBasemapDefaultMapStyle::new(self.theme.clone()))
    }
}
