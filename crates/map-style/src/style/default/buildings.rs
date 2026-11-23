// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The buildings: extruded from the heights the schema reading states, or,
//! with the theme's extrusion off, the footprints as flat fills.

use crate::schema::FeatureFacts;
use crate::style::default::ProtomapsBasemapDefaultMapStyle;
use crate::style::{ExtrusionStyle, FeatureStyle, FillStyle};

impl ProtomapsBasemapDefaultMapStyle {
    /// The height, in metres, of a building the tile states no height for.
    /// The basemap leaves `height` out when the source tags neither a height
    /// nor a storey count, which is common for low buildings, and without a
    /// fallback such a footprint stayed flat among its raised neighbours.
    /// Two storeys, the way the basemap itself converts a storey count
    /// (three metres a storey plus two).
    pub const BUILDING_FALLBACK_HEIGHT_METRES: f32 = 8.0;

    /// A footprint on every tile that ships one: the basemap's merged blocks
    /// of z12 and z13 draw too, flat or raised as the engine's building
    /// coverage decides.
    pub fn building_style(&self, facts: &FeatureFacts<'_>) -> FeatureStyle {
        if facts.building().is_none() {
            return FeatureStyle::Hidden;
        }
        let features = &self.theme.features;
        if !features.building_extrusion {
            // The same fill an extrusion draws under itself, and nothing
            // rises: how buildings draw on the globe, now on the plane too.
            return FeatureStyle::Fill(FillStyle::new(40, features.building_fill_color));
        }
        FeatureStyle::Extrusion(ExtrusionStyle {
            height_scale: 8.0,
            anchor_zoom: 16,
            fallback_height: Self::BUILDING_FALLBACK_HEIGHT_METRES,
            edge_radius: features.building_edge_radius,
            ..ExtrusionStyle::new(40, features.building_fill_color)
        })
    }
}
