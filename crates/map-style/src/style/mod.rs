// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! How a feature draws: the public style API (`MapStyle`,
//! `VectorTileStyle`, `FeatureStyle` and its payloads, the GPU style tables
//! `TilePolygonStyle` and `TileLineStyle`), the runtime the parser asks
//! (`MapStyleRuntime`), and the built-in style for the Protomaps basemap
//! with its theme in [`default`].
//!
//! Rendering parameters live on the style, never in the engine. The engine
//! reads the feature facts from the schema and the drawing parameters from
//! the style and decides nothing of its own. A new drawing knob is a field
//! on the style, not a constant in a shader, a parser or a resolver. How a
//! layer appears or disappears with the zoom is one of those fields: a
//! `ZoomFade` on the fill or the line pass, evaluated against the camera
//! zoom, never a band built into the engine.

mod attribution;
pub mod default;
mod feature_style;
mod looks;
mod map_style;
mod route_shield;
mod small;
mod tile_style;
mod vector_tile_style;
mod zoom_fade;

pub use attribution::{Attribution, AttributionCredit};
pub use feature_style::{
    ExtrusionStyle, FeatureStyle, FillStyle, LabelFontWeight, LabelTextStyle, LinePass, LinePlacement, LineStyle,
    PointLabelOptions, PointLabelStyle, RoadLevel, RoadPass, RoadPassRole, RoadStyle, RoadTier, TreeFillStyle,
    TreePointStyle, WidthRamp,
};
pub use looks::{BaseColors, Hillshade, LabelOpacity, LabelOpacityFrame, TerrainOcclusion, TreePalette};
pub use map_style::{AnyMapStyle, MapStyle, MapStyleRuntime, VectorTileMapStyle};
pub use route_shield::{RouteShield, RouteShieldAppearance, RouteShieldShape, RouteShieldStyle};
pub use small::{
    LabelPlacement, LabelTypeScale, LineGeometryStyle, OnewayArrowDecoration, PoiSpriteIcon, RoadDecorationKind,
    SurfaceLabelPlacement,
};
pub use tile_style::{TileLineStyle, TilePolygonStyle};
pub use vector_tile_style::{BasicVectorTileStyle, FeatureStyleContext, GENERIC_STYLE_ID, VectorTileStyle, WaterNameKind};
pub use zoom_fade::ZoomFade;
