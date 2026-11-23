// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! What the ported tests share: a feature stated as a test states it, the
//! built-in style asked through the public context, the way the parser asks
//! it, and the flat reading of a style the tests assert with.

#![allow(dead_code)]

use map_style::schema::default::ProtomapsBasemapSchema;
use map_style::schema::{Feature, FeatureFacts, FeatureGeometry, TileSchema};
use map_style::style::default::ProtomapsBasemapDefaultMapStyle;
use map_style::style::{FeatureStyle, FeatureStyleContext, LineGeometryStyle, LinePass, RoadPass, RoadPassRole, VectorTileStyle, ZoomFade};
use map_style::support::{line_baking, Tile};
use mvt::Value;

pub type Props = Vec<(&'static str, Value<'static>)>;

pub fn s(text: &'static str) -> Value<'static> {
    Value::String(text)
}

pub fn i(number: i64) -> Value<'static> {
    Value::Int(number)
}

pub fn b(flag: bool) -> Value<'static> {
    Value::Bool(flag)
}

/// The Protomaps basemap's spelling of the road classes the theme names,
/// for a test that states a road by its class.
pub fn road_class_props(class: &'static str) -> Props {
    match class {
        "motorway" => vec![("kind", s("highway")), ("kind_detail", s("motorway"))],
        "trunk" | "primary" | "secondary" | "tertiary" => vec![("kind", s("major_road")), ("kind_detail", s(class))],
        "minor" => vec![("kind", s("minor_road")), ("kind_detail", s("residential"))],
        "service" => vec![("kind", s("minor_road")), ("kind_detail", s("service"))],
        "path" => vec![("kind", s("path")), ("kind_detail", s("footway"))],
        "footway" | "track" => vec![("kind", s("path")), ("kind_detail", s(class))],
        other => vec![("kind", s(other))],
    }
}

/// The Protomaps basemap's reading of a feature.
pub fn facts<'a>(layer: &'a str, props: &[(&'a str, Value<'a>)], zoom: i32, geometry: FeatureGeometry) -> FeatureFacts<'a> {
    ProtomapsBasemapSchema.read(&Feature::new(layer, Tile::new(0, 0, zoom), geometry, props))
}

/// A style's answer for a feature whose facts the basemap's schema reads,
/// through the public context, on the tile at the origin of a zoom.
pub fn style_of(style: &dyn VectorTileStyle, layer: &str, props: &[(&str, Value<'_>)], zoom: i32, geometry: FeatureGeometry) -> FeatureStyle {
    style_on_tile(style, layer, props, Tile::new(0, 0, zoom), geometry)
}

pub fn style_on_tile(style: &dyn VectorTileStyle, layer: &str, props: &[(&str, Value<'_>)], tile: Tile, geometry: FeatureGeometry) -> FeatureStyle {
    let feature = Feature::new(layer, tile, geometry, props);
    let facts = ProtomapsBasemapSchema.read(&feature);
    style.make_style(&FeatureStyleContext::new(style.style_id(), &feature, &facts))
}

/// The built-in style with the default theme.
pub fn default_style() -> ProtomapsBasemapDefaultMapStyle {
    ProtomapsBasemapDefaultMapStyle::default()
}

/// Every stroke of a road, or of a line read as a road, with its role.
pub fn passes(style: &FeatureStyle) -> Vec<RoadPass> {
    style.road_style().map(|road| road.ordered_passes()).unwrap_or_default()
}

pub fn pass_of(style: &FeatureStyle, role: RoadPassRole) -> Option<LinePass> {
    passes(style).into_iter().find(|pass| pass.role == role).map(|pass| pass.pass)
}

/// The style's main stroke: a line's, a road's fill (or first stroke), and
/// a fill or an extrusion as the stroke the parser bakes it to.
pub fn primary_pass(style: &FeatureStyle) -> Option<LinePass> {
    match style {
        FeatureStyle::Hidden | FeatureStyle::PointLabel(_) | FeatureStyle::Tree(_) => None,
        FeatureStyle::Fill(fill) => Some(line_baking::baked_fill_pass(fill)),
        FeatureStyle::Line(line) => Some(line.pass),
        FeatureStyle::Road(road) => road.fill.or_else(|| road.ordered_passes().first().map(|pass| pass.pass)),
        FeatureStyle::Extrusion(extrusion) => Some(line_baking::baked_extrusion_pass(extrusion)),
    }
}

pub fn color(style: &FeatureStyle) -> [f32; 4] {
    primary_pass(style).map(|pass| pass.color).unwrap_or([0.0; 4])
}

pub fn zoom_fade(style: &FeatureStyle) -> ZoomFade {
    primary_pass(style).map(|pass| pass.zoom_fade).unwrap_or(ZoomFade::NONE)
}

pub fn line_width_points(style: &FeatureStyle) -> f32 {
    primary_pass(style).map(|pass| pass.line_width_points).unwrap_or(0.0)
}

pub fn dash_length_points(style: &FeatureStyle) -> f32 {
    primary_pass(style).map(|pass| pass.dash_length_points).unwrap_or(0.0)
}

pub fn dash_gap_points(style: &FeatureStyle) -> f32 {
    primary_pass(style).map(|pass| pass.dash_gap_points).unwrap_or(0.0)
}

pub fn minimum_width_points(style: &FeatureStyle) -> f32 {
    primary_pass(style).map(|pass| pass.minimum_width_points).unwrap_or(0.0)
}

pub fn line_geometry(style: &FeatureStyle) -> LineGeometryStyle {
    primary_pass(style).map(|pass| pass.line_geometry).unwrap_or(LineGeometryStyle::new(0.0))
}

pub fn suppresses_polygon_fill(style: &FeatureStyle) -> bool {
    style.line_style().is_some_and(|line| !line.fills_areas)
}

pub fn label_rank(style: &FeatureStyle) -> i32 {
    style.point_label_style().map(|label| label.rank).unwrap_or(0)
}

pub fn label_collision_rank(style: &FeatureStyle) -> i32 {
    style.point_label_style().map(|label| label.collision_rank).unwrap_or(0)
}

pub fn label_min_camera_zoom(style: &FeatureStyle) -> f32 {
    style.point_label_style().map(|label| label.min_camera_zoom).unwrap_or(0.0)
}

pub fn assert_close(actual: f32, expected: f32, accuracy: f32) {
    assert!((actual - expected).abs() <= accuracy, "{actual} is not within {accuracy} of {expected}");
}

pub fn rgb(color: [f32; 4]) -> [f32; 3] {
    [color[0], color[1], color[2]]
}
