// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Ported from `FeatureStyleFactoryTests`, `ImmersiveMapZoomFadeTests`,
//! `ImmersiveMapZoomCurveTests` (in `GroundFogTests`),
//! `ImmersiveMapLabelOpacityTests`, `MapStyleRuntimeTests`,
//! `TileSourceStyleSeparationTests`, `NightThemeTests` (the style's globe
//! colours and the label handover), `LabelScreenUnitTests` (the halo),
//! `AttributionSettingsTests` (the credits at a zoom) and `TreePointTests`
//! (the built-in style's mapped tree).
//!
//! Skipped: the settings, the settings planner and the view modifiers
//! (`ImmersiveMapSettings`, `ImmersiveMapView`), the debug panel's export,
//! the POI plate's GPU layout, the shader mirror of the fade
//! (`TileStyleFadeMath`), the tree scatter and every parse of a tile.

mod support;

use std::sync::Arc;

use map_style::schema::default::ProtomapsBasemapSchema;
use map_style::schema::{Feature, FeatureFacts, FeatureGeometry, LabelFacts, TileSchema};
use map_style::style::default::{ProtomapsBasemapDefaultMapStyle, ProtomapsBasemapMapStyle, ProtomapsBasemapTheme};
use map_style::style::{
    AnyMapStyle, Attribution, AttributionCredit, BaseColors, BasicVectorTileStyle, ExtrusionStyle, FeatureStyle,
    FeatureStyleContext, LabelFontWeight, LabelOpacity, LabelOpacityFrame, LabelTextStyle, LabelTypeScale,
    MapStyleRuntime, PointLabelOptions, TreePointStyle, VectorTileMapStyle, VectorTileStyle, ZoomFade, GENERIC_STYLE_ID,
};
use map_style::support::{ScreenScale, Tile, ZoomCurve, ZoomCurveInterpolation};
use support::*;

fn text_style() -> LabelTextStyle {
    LabelTextStyle::new([0.2, 0.2, 0.18], [1.0, 0.98, 0.92], 0.15, 14.0, LabelFontWeight::Bold)
}

// MARK: The factories

#[test]
fn point_label_keys_the_text_style_and_draws_no_geometry() {
    let style = FeatureStyle::point_label(70, text_style(), PointLabelOptions { min_camera_zoom: 9.0, ..PointLabelOptions::default() });
    assert_eq!(style.key(), 70);
    assert!(primary_pass(&style).is_none(), "A label draws no geometry of its own");
    let text = style.point_label_style().unwrap().text;
    assert_eq!(text.key, 70, "The text style carries the feature style's key");
    assert_eq!(text.fill_color, [0.2, 0.2, 0.18]);
    assert_eq!(text.size_points, 14.0);
    assert_eq!(text.weight, LabelFontWeight::Bold);
    assert_eq!(label_min_camera_zoom(&style), 9.0);
    assert!(style.road_style().is_none());
}

#[test]
fn label_sizes_below_the_readable_floor_are_raised_to_it() {
    let small = LabelTextStyle { size_points: 6.0, ..text_style() };
    let style = FeatureStyle::point_label(1, small, PointLabelOptions::default());
    assert_eq!(style.point_label_style().unwrap().text.size_points, LabelTypeScale::MINIMUM_SIZE_POINTS);
}

#[test]
fn road_label_is_a_line_with_the_name_along_it() {
    let style = FeatureStyle::road_label(40, [0.96, 0.94, 0.90, 1.0], 1.6, text_style());
    assert_eq!(color(&style), [0.96, 0.94, 0.90, 1.0]);
    assert!((line_geometry(&style).line_width - 1.6).abs() < 0.0001);
    let road = style.road_style().unwrap();
    assert!(road.label.is_some());
    assert!(style.point_label_style().is_none());
    assert_eq!(road.label.unwrap().key, 40);
    assert_eq!(road.label.unwrap().weight, LabelFontWeight::Bold);
}

#[test]
fn point_locked_line_carries_the_whole_principle() {
    let color_value = [0.4, 0.3, 0.5, 0.9];
    let style = FeatureStyle::point_locked_line(100, color_value, 1.3, 6.0, 3.0, ZoomFade::OVERVIEW, None);
    assert_eq!(color(&style), color_value);
    assert_eq!(line_width_points(&style), 1.3, "The width is point-locked");
    assert_eq!(zoom_fade(&style), ZoomFade::fade_in(0.0, 1.0), "Opaque from the first zoom level off the globe");
    let geometry = line_geometry(&style);
    assert!(!geometry.line_cap_round, "Butt ends");
    assert!(!geometry.line_join_round, "Plain joins");
    assert_eq!(dash_length_points(&style), 6.0, "Dashes are stated in points");
    assert_eq!(dash_gap_points(&style), 3.0);
    assert!(!geometry.uses_dash_pattern(), "Point dashes are shader-cut");
    assert!((geometry.line_width - 1.3f32 as f64 * FeatureStyle::POINT_LOCKED_RIBBON_UNITS_PER_POINT).abs() < 0.001);
    assert!(suppresses_polygon_fill(&style), "Areal geometry under a line mode draws outlines only");
}

#[test]
fn plain_line_opts_into_none_of_it() {
    let style = FeatureStyle::line(44, [1.0; 4], 2.0, ZoomFade::NONE);
    assert_eq!(line_width_points(&style), 0.0);
    assert_eq!(line_geometry(&style).line_width, 2.0, "A plain line width lives in tile units");
    assert!(!suppresses_polygon_fill(&style));
    assert!(style.extrusion_style().is_none());
}

#[test]
fn extruded_polygon_rises_and_a_polygon_does_not() {
    let extruded = FeatureStyle::extruded_polygon(ExtrusionStyle { fallback_height: 8.0, ..ExtrusionStyle::new(30, [1.0; 4]) });
    assert_eq!(extruded.extrusion_style().unwrap().fallback_height, 8.0);
    assert!(FeatureStyle::polygon(30, [1.0; 4], ZoomFade::NONE).extrusion_style().is_none());
}

#[test]
fn hidden_is_key_zero() {
    assert_eq!(FeatureStyle::Hidden.key(), 0, "Key 0 is what the parser skips");
}

// MARK: The zoom fade

#[test]
fn no_fade_draws_in_full_at_every_zoom() {
    for zoom in [0.0, 0.3, 7.5, 22.0] {
        assert_eq!(ZoomFade::NONE.alpha(zoom), 1.0, "{zoom}");
    }
}

#[test]
fn a_fade_in_rises_from_nothing_to_full() {
    let fade = ZoomFade::fade_in(7.0, 8.0);
    assert_eq!(fade.alpha(3.0), 0.0);
    assert_eq!(fade.alpha(7.0), 0.0);
    assert_close(fade.alpha(7.5), 0.5, 1e-6);
    assert_eq!(fade.alpha(8.0), 1.0);
    assert_eq!(fade.alpha(12.0), 1.0);
    assert_eq!(fade.shader_pair(), [7.0, 8.0]);
}

#[test]
fn a_fade_out_falls_from_full_to_nothing() {
    let fade = ZoomFade::fade_out(7.0, 8.0);
    assert_eq!(fade.alpha(3.0), 1.0);
    assert_eq!(fade.alpha(7.0), 1.0);
    assert_close(fade.alpha(7.5), 0.5, 1e-6);
    assert_eq!(fade.alpha(8.0), 0.0);
    assert_eq!(fade.alpha(12.0), 0.0);
    assert_eq!(fade.shader_pair(), [8.0, 7.0]);
}

/// The curve is a smoothstep, not a ramp: flat at both ends, so a fade
/// starts and settles without a visible kink.
#[test]
fn the_curve_is_a_smoothstep() {
    let fade = ZoomFade::fade_in(0.0, 1.0);
    assert_close(fade.alpha(0.25), 0.15625, 1e-6);
    assert_close(fade.alpha(0.75), 0.84375, 1e-6);
    let out = ZoomFade::fade_out(0.0, 1.0);
    assert_close(fade.alpha(0.3) + out.alpha(0.3), 1.0, 1e-6);
}

#[test]
fn the_range_may_be_any_length() {
    assert_close(ZoomFade::fade_in(15.0, 15.4).alpha(15.2), 0.5, 1e-6);
    assert_close(ZoomFade::fade_out(4.0, 10.0).alpha(7.0), 0.5, 1e-6);
}

#[test]
#[should_panic(expected = "A zoom fade runs from a lower to a higher zoom")]
fn a_fade_runs_from_a_lower_to_a_higher_zoom() {
    ZoomFade::fade_in(8.0, 7.0);
}

// MARK: The zoom curve

#[test]
fn a_single_value_is_the_same_at_every_zoom() {
    let curve = ZoomCurve::constant(0.3);
    assert_eq!(curve.value(0.0), 0.3);
    assert_eq!(curve.value(22.0), 0.3);
    assert_eq!(curve.constant_value(), Some(0.3));
}

/// The ends hold past the first and the last stop, and between two stops
/// the value eases in and out on a smoothstep.
#[test]
fn stops_blend_smoothly() {
    let curve = ZoomCurve::new([(16.0, 1.0), (12.0, 0.0)]);
    assert_eq!(curve.stops().iter().map(|stop| stop.zoom).collect::<Vec<_>>(), [12.0, 16.0]);
    assert_eq!(curve.constant_value(), None);
    assert_eq!(curve.value(10.0), 0.0);
    assert_eq!(curve.value(12.0), 0.0);
    assert_close(curve.value(14.0), 0.5, 1e-6);
    assert_close(curve.value(13.0), 0.15625, 1e-6);
    assert_eq!(curve.value(16.0), 1.0);
    assert_eq!(curve.value(20.0), 1.0);
}

#[test]
fn each_segment_blends_between_its_own_stops() {
    let curve = ZoomCurve::new([(13.0, 4.0), (14.0, 1.0), (15.0, 0.15)]);
    assert_close(curve.value(13.5), 2.5, 1e-6);
    assert_eq!(curve.value(14.0), 1.0);
    assert_close(curve.value(14.5), 0.575, 1e-6);
}

/// Geometrically the value changes by the same ratio for every step of
/// zoom, the way the map's scale does, and a stop not above zero falls
/// back to the smoothstep.
#[test]
fn geometric_stops_keep_a_ratio_per_zoom() {
    let curve = ZoomCurve::new([(12.0, 3200.0), (16.0, 200.0)]);
    for (zoom, expected) in [(13.0, 1600.0), (14.0, 800.0), (15.0, 400.0)] {
        assert_close(curve.value_with(zoom, ZoomCurveInterpolation::Geometric), expected, 0.01);
    }
    let with_zero = ZoomCurve::new([(12.0, 0.0), (16.0, 1.0)]);
    assert_close(with_zero.value_with(14.0, ZoomCurveInterpolation::Geometric), 0.5, 1e-6);
}

#[test]
fn a_curve_reads_and_writes_json() {
    let curve = ZoomCurve::new([(13.0, 4.0), (14.0, 1.0)]);
    let json = serde_json::to_string(&curve).unwrap();
    assert_eq!(serde_json::from_str::<ZoomCurve>(&json).unwrap(), curve);
    assert_eq!(serde_json::from_str::<ZoomCurve>("0.3").unwrap(), ZoomCurve::constant(0.3));
    assert_eq!(serde_json::from_str::<ZoomCurve>("[[13, 4], [14, 1]]").unwrap(), curve);
    assert_eq!(serde_json::from_str::<ZoomCurve>(r#"{"14": 1, "13": 4}"#).unwrap(), curve);
    assert!(serde_json::from_str::<ZoomCurve>("[]").is_err(), "A zoom curve needs at least one stop");
}

// MARK: The label opacity

#[test]
fn the_default_leaves_every_label_at_its_own_opacity() {
    assert_eq!(LabelOpacity::default().frame(12.0), LabelOpacityFrame::SOLID);
    assert_eq!(ProtomapsBasemapTheme::default().label_opacity, LabelOpacity::default());
}

#[test]
fn the_curves_are_read_at_the_zoom_and_held_to_the_unit_range() {
    let opacity = LabelOpacity { names: ZoomCurve::new([(2.0, 1.0), (6.0, 0.4)]), pois: ZoomCurve::new([(14.0, 1.5), (16.0, -0.2)]) };
    assert_eq!(opacity.frame(1.0).names, 1.0);
    assert_close(opacity.frame(8.0).names, 0.4, 1e-6);
    assert_eq!(opacity.frame(10.0).pois, 1.0, "above one is held to one");
    assert_eq!(opacity.frame(18.0).pois, 0.0, "below zero is held to zero");
}

#[test]
fn a_poi_takes_the_pois_factor_and_every_other_label_the_names() {
    let frame = LabelOpacityFrame { names: 0.5, pois: 0.25 };
    let name = LabelTextStyle { opacity: 0.8, ..LabelTextStyle::new([0.0; 3], [1.0; 3], 0.1, 12.0, LabelFontWeight::Bold) };
    let poi = LabelTextStyle { is_point_of_interest: true, weight: LabelFontWeight::Thin, ..name };
    assert_close(frame.opacity(&name), 0.4, 1e-6);
    assert_close(frame.opacity(&poi), 0.2, 1e-6);
}

// MARK: The runtime and the map styles

struct RuntimeContextTestStyle;

impl VectorTileStyle for RuntimeContextTestStyle {
    fn cache_fingerprint(&self) -> u32 {
        42
    }

    fn style_id(&self) -> &str {
        "runtime-context-style"
    }

    fn base_colors(&self) -> BaseColors {
        BaseColors { map: [0.1, 0.2, 0.3, 1.0], north_cap: [0.0, 0.0, 1.0, 1.0], south_cap: [1.0; 4] }
    }

    fn make_style(&self, _feature: &FeatureStyleContext<'_>) -> FeatureStyle {
        FeatureStyle::polygon(1, [1.0; 4], ZoomFade::NONE)
    }
}

/// A reading of a schema whose labels carry their text in `title`.
struct TitledTileSchema;

impl TileSchema for TitledTileSchema {
    fn cache_fingerprint(&self) -> u32 {
        1
    }

    fn read<'a>(&self, feature: &Feature<'_, 'a>) -> FeatureFacts<'a> {
        FeatureFacts::Labelled(LabelFacts { name: feature.properties.string("title").map(Into::into), ..LabelFacts::default() })
    }
}

#[test]
fn the_runtime_context_materializes_the_style_outside_the_renderer() {
    let map_style = VectorTileMapStyle::new(Arc::new(RuntimeContextTestStyle), Arc::new(ProtomapsBasemapSchema), Some(42));
    let runtime = MapStyleRuntime::new(&AnyMapStyle::new(&map_style));
    assert_eq!(runtime.style.cache_fingerprint(), 42);
    assert_eq!(runtime.style_id, "runtime-context-style");
    assert_eq!(runtime.base_colors.map, [0.1, 0.2, 0.3, 1.0]);
    // The default looks of a style that states none are the built-in theme's.
    assert_eq!(runtime.hillshade, ProtomapsBasemapTheme::default().hillshade);
    assert_eq!(runtime.debug_border_style().color, [1.0, 0.0, 0.0, 1.0]);
}

#[test]
fn a_style_without_an_identity_gets_the_generic_one() {
    let map_style = VectorTileMapStyle::with_basemap_schema(Arc::new(BasicVectorTileStyle::new(7)));
    assert_eq!(MapStyleRuntime::new(&AnyMapStyle::new(&map_style)).style_id, GENERIC_STYLE_ID);
}

/// The tile source and the map style are independent: everything about
/// interpreting the bytes (the style, the reading) is configured on the
/// style side.
#[test]
fn the_style_configures_parsing() {
    let map_style = VectorTileMapStyle::new(Arc::new(BasicVectorTileStyle::new(77)), Arc::new(TitledTileSchema), None);
    let runtime = MapStyleRuntime::new(&AnyMapStyle::new(&map_style));
    assert_eq!(runtime.style.cache_fingerprint(), 77);
    assert_eq!(runtime.style_id, GENERIC_STYLE_ID);
    let props = [("title", s("Cafe"))];
    let facts = runtime.read_facts(&Feature::new("poi", Tile::new(0, 0, 14), FeatureGeometry::Point, &props));
    assert_eq!(facts.label().unwrap().name.as_deref(), Some("Cafe"));
    assert_eq!(runtime.prepared_tile_style_revision(), 31 + 77);
    let one = AnyMapStyle::new(&VectorTileMapStyle::with_basemap_schema(Arc::new(BasicVectorTileStyle::new(1))));
    let two = AnyMapStyle::new(&VectorTileMapStyle::with_basemap_schema(Arc::new(BasicVectorTileStyle::new(2))));
    assert_ne!(one, two, "Changing only the map style is a style change");
}

#[test]
fn the_runtime_asks_the_built_in_style() {
    let runtime = MapStyleRuntime::new(&AnyMapStyle::new(&ProtomapsBasemapMapStyle::default()));
    assert_eq!(runtime.style_id, "protomaps");
    let props = [("kind", s("lake")), ("name", s("Lake"))];
    let feature = Feature::new("water", Tile::new(0, 0, 10), FeatureGeometry::Point, &props);
    let facts = runtime.read_facts(&feature);
    assert!(runtime.make_style(&feature, &facts).point_label_style().is_some());
    assert_eq!(runtime.background_style(Tile::new(0, 0, 3)).key(), 1);
    assert_eq!(
        runtime.prepared_tile_style_revision(),
        4u32.wrapping_mul(31).wrapping_add(ProtomapsBasemapDefaultMapStyle::default().cache_fingerprint())
    );
}

// MARK: The night's globe

/// Every fill and stroke of the overview zooms states the day's colour as
/// what it wears on the globe, under the night's own for the plane, and a
/// theme without a globe palette states none.
#[test]
fn the_nights_ground_wears_the_days_colours_on_the_globe() {
    let day = ProtomapsBasemapTheme::default();
    let night = ProtomapsBasemapDefaultMapStyle::new(ProtomapsBasemapTheme::night());
    let fill = |style: FeatureStyle| *style.fill_style().expect("a fill");
    let line = |style: FeatureStyle| style.line_style().expect("a line").pass;
    let forest = fill(night.biome_style(Some("temperate_broadleaf_forest"), 3));
    assert_eq!(forest.color, ProtomapsBasemapTheme::night().layers.biomes.temperate_broadleaf_forest);
    assert_eq!(forest.sphere_color, Some(rgb(day.layers.biomes.temperate_broadleaf_forest)));
    assert_eq!(fill(night.overview_lake_style(0)).sphere_color, Some(rgb(day.layers.water)));
    let ocean = fill(style_of(&night, "water", &[], 2, FeatureGeometry::Unknown));
    assert_eq!(ocean.color, ProtomapsBasemapTheme::night().layers.water);
    assert_eq!(ocean.sphere_color, Some(rgb(day.layers.water)));
    assert_eq!(fill(style_of(&night, "earth", &[], 2, FeatureGeometry::Unknown)).sphere_color, Some(rgb(day.layers.land)));
    assert_eq!(fill(night.landcover_style(Some("forest"), 4)).sphere_color, Some(rgb(day.layers.biomes.forest_cover)));
    assert_eq!(line(night.overview_road_network_style(Some(1), 3)).sphere_color, Some(rgb(day.layers.overview_roads.major)));
    let border = line(night.boundary_style(Some("country"), &map_style::schema::FeatureProperties::empty(), 3));
    assert_eq!(
        border.sphere_color,
        Some(rgb(ProtomapsBasemapDefaultMapStyle::softened_boundary_color(day.layers.boundary, 0.15, 0.55)))
    );
    let day_style = ProtomapsBasemapDefaultMapStyle::new(day);
    assert_eq!(fill(day_style.biome_style(Some("desert"), 3)).sphere_color, None);
    assert_eq!(fill(day_style.overview_lake_style(0)).sphere_color, None);
    assert_eq!(line(day_style.boundary_style(Some("country"), &map_style::schema::FeatureProperties::empty(), 3)).sphere_color, None);
}

/// The names of the globe's zooms state the day's fill and halo as what
/// they wear on the globe, in the night's own size and weight, and the
/// names never on the globe, and a theme without globe labels, state none.
#[test]
fn the_nights_names_wear_the_days_lettering_on_the_globe() {
    let day = ProtomapsBasemapTheme::default();
    let night = ProtomapsBasemapDefaultMapStyle::new(ProtomapsBasemapTheme::night());
    let none = map_style::schema::FeatureProperties::empty();
    let text = |style: FeatureStyle| style.point_label_style().expect("a label").text;
    let country = text(night.place_label_style(Some("country"), None, &none));
    assert_eq!(country.fill_color, ProtomapsBasemapTheme::night().labels.country.fill_color);
    assert_eq!(country.sphere_fill_color, Some(day.labels.country.fill_color));
    assert_eq!(country.sphere_stroke_color, Some(day.labels.country.stroke_color));
    let capital_props = [("capital", s("yes"))];
    let capital = text(night.place_label_style(Some("locality"), Some("city"), &map_style::schema::FeatureProperties::new(&capital_props)));
    assert_eq!(capital.weight, LabelFontWeight::Bold, "The capital's weight is the night's own rule");
    assert_eq!(capital.sphere_fill_color, Some(day.labels.city.fill_color));
    assert_eq!(text(night.water_label_style(Some("ocean"), &none, 0)).sphere_fill_color, Some(day.labels.water.fill_color));
    assert_eq!(text(night.poi_label_style(Some("park"), &none)).sphere_fill_color, None, "A POI is never on the globe");
    let day_country = text(ProtomapsBasemapDefaultMapStyle::new(day).place_label_style(Some("country"), None, &none));
    assert_eq!(day_country.sphere_fill_color, None);
    assert_eq!(day_country.sphere_stroke_color, None);
}

/// The colours the drawers read: the globe's at a handover of 0, the
/// label's own at 1 and where it states none, mixed between.
#[test]
fn a_labels_colours_follow_the_handover() {
    let lettered = LabelTextStyle {
        sphere_fill_color: Some([0.0; 3]),
        sphere_stroke_color: Some([1.0; 3]),
        ..LabelTextStyle::new([1.0; 3], [0.0; 3], 0.1, 12.0, LabelFontWeight::Bold)
    };
    assert_eq!(lettered.fill_color_at_handover(0.0), [0.0; 3]);
    assert_eq!(lettered.stroke_color_at_handover(0.0), [1.0; 3]);
    assert_eq!(lettered.fill_color_at_handover(1.0), [1.0; 3]);
    assert_eq!(lettered.fill_color_at_handover(0.25), [0.25; 3]);
    assert_eq!(lettered.fill_color_at_handover(2.0), [1.0; 3], "Clamped");
    let own = LabelTextStyle::new([1.0; 3], [0.0; 3], 0.1, 12.0, LabelFontWeight::Bold);
    assert_eq!(own.fill_color_at_handover(0.0), [1.0; 3]);
    assert_eq!(own.stroke_color_at_handover(0.0), [0.0; 3]);
}

#[test]
fn halo_width_tracks_both_the_em_size_and_the_display() {
    let style = LabelTextStyle::new([0.0; 3], [1.0; 3], 0.15, 11.0, LabelFontWeight::Bold);
    assert_close(style.halo_width_pixels(ScreenScale::new(2.0)), 11.0 * 0.15 * 2.0, 0.0001);
    assert_close(style.halo_width_pixels(ScreenScale::new(3.0)), 11.0 * 0.15 * 3.0, 0.0001);
    let larger = LabelTextStyle { size_points: 22.0, ..style };
    assert!(style.halo_width_pixels(ScreenScale::REFERENCE) < larger.halo_width_pixels(ScreenScale::REFERENCE));
    assert_eq!(LabelTextStyle::new([0.0; 3], [1.0; 3], 0.1, 11.0, LabelFontWeight::Bold).with_halo_softness(3.0).halo_softness, 1.0);
}

// MARK: The attribution

/// A credit with no words is no credit, and a credit's zoom range is a
/// half-open range of camera zoom.
#[test]
fn credits_at_a_zoom_drop_the_empty_and_the_out_of_range() {
    let attribution = Attribution::new(
        vec![
            AttributionCredit::new("Always"),
            AttributionCredit::new(""),
            AttributionCredit::in_zooms("Low", 0.0..5.0),
            AttributionCredit::in_zooms("High", 5.0..20.0),
        ],
        None,
    );
    let texts = |zoom: Option<f64>| attribution.credits_at_zoom(zoom).iter().map(|credit| credit.text.clone()).collect::<Vec<_>>();
    assert!(Attribution::new(vec![AttributionCredit::new(""), AttributionCredit::new("")], None).is_empty());
    assert!(!attribution.is_empty());
    assert_eq!(texts(Some(4.99)), ["Always", "Low"]);
    assert_eq!(texts(Some(5.0)), ["Always", "High"]);
    assert_eq!(texts(Some(20.0)), ["Always"]);
    assert_eq!(texts(None), ["Always", "Low", "High"]);
    assert!(Attribution::none().is_empty());
    assert_eq!(Attribution::open_street_map().link_url.as_deref(), Some("https://www.openstreetmap.org/copyright"));
    assert_eq!(Attribution::none().appending(Attribution::engine_credit()).credits, [Attribution::engine_credit()]);
}

// MARK: The trees

/// The built-in style plants a tree at every point of the basemap's `pois`
/// of kind `tree`, and it is no label.
#[test]
fn the_built_in_style_plants_a_tree_at_a_tree_point() {
    let props = [("kind", s("tree")), ("name", s("")), ("min_zoom", i(16))];
    let style = style_on_tile(&default_style(), "pois", &props, Tile::new(19809, 10244, 15), FeatureGeometry::Point);
    assert_eq!(style.tree_point_style(), Some(&TreePointStyle::new(7.0..=12.0, 4.0)));
    assert_eq!(style.key(), 0, "a tree bakes no style of its own");
    assert_eq!(ProtomapsBasemapTheme::default().layers.trees.mapped, Some(TreePointStyle::new(7.0..=12.0, 4.0)));
    let treeless = ProtomapsBasemapDefaultMapStyle::new(ProtomapsBasemapTheme::default().with_layers(|layers| layers.trees.mapped = None));
    assert_eq!(style_on_tile(&treeless, "pois", &props, Tile::new(19809, 10244, 15), FeatureGeometry::Point), FeatureStyle::Hidden);
}

#[test]
fn the_tree_styles_hold_their_values_to_their_ranges() {
    let tree = map_style::style::TreeFillStyle::new(0.1, 5.0..=2.0, -1.0);
    assert_eq!(tree.spacing_metres, 0.5);
    assert_eq!(tree.minimum_height_metres, 5.0);
    assert_eq!(tree.maximum_height_metres, 5.0);
    assert_eq!(tree.road_clearance_metres, 0.0);
    let parsed: map_style::style::TreeFillStyle =
        serde_json::from_str(r#"{"spacingMetres": 0.1, "minimumHeightMetres": 5, "maximumHeightMetres": 2}"#).unwrap();
    assert_eq!(parsed, map_style::style::TreeFillStyle::new(0.1, 5.0..=2.0, 3.0));
}
