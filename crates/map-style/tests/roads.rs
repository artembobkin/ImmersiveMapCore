// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Ported from `RoadTierTests`, `ProtomapsBasemap/RoadSymbolWidthTests`,
//! `RoadSymbolWidthFadeTests`, `RoadPathAndCasingStyleTests`,
//! `CountryViewHandoverTests` and `TileLineStyleDashUnitsContractTests`:
//! the road as a symbol of the theme, its tier, its path and its casing, and
//! what the parser bakes a stroke into.

mod support;

use map_style::schema::FeatureGeometry;
use map_style::style::default::{OverviewRoadStroke, ProtomapsBasemapDefaultMapStyle, ProtomapsBasemapTheme, RoadClass, RoadMetrics};
use map_style::style::{
    FeatureStyle, FillStyle, LineGeometryStyle, LinePass, LineStyle, RoadPassRole, RoadTier, TileLineStyle, ZoomFade,
};
use map_style::support::{line_baking, Tile};
use support::*;

fn road_of(style: &ProtomapsBasemapDefaultMapStyle, class: &'static str, zoom: i32) -> FeatureStyle {
    style_of(style, "roads", &road_class_props(class), zoom, FeatureGeometry::Line)
}

fn fill_of(style: &ProtomapsBasemapDefaultMapStyle, class: &'static str, zoom: i32) -> Option<LinePass> {
    pass_of(&road_of(style, class, zoom), RoadPassRole::Fill)
}

/// Central Moscow, the tile column and row of the screenshots this
/// behavior was reported from, at each zoom.
fn moscow_tile(z: i32) -> Tile {
    if z >= 16 {
        let scale = 1 << (z - 16);
        Tile::new(39616 * scale, 20486 * scale, z)
    } else {
        Tile::new(39616 >> (16 - z), 20486 >> (16 - z), z)
    }
}

fn moscow_road(class: &'static str, lanes: Option<i64>, z: i32, oneway: bool, width: Option<i64>) -> FeatureStyle {
    let mut props = road_class_props(class);
    // The lane count and the width are not the basemap's, and a source that
    // carries them must change nothing.
    if let Some(lanes) = lanes {
        props.push(("lanes", i(lanes)));
    }
    if oneway {
        props.push(("oneway", b(true)));
    }
    if let Some(width) = width {
        props.push(("width", i(width)));
    }
    style_on_tile(&default_style(), "roads", &props, moscow_tile(z), FeatureGeometry::Unknown)
}

#[test]
fn the_tier_line_sits_between_service_roads_and_paths() {
    let style = default_style();
    let tier = |class: &'static str| {
        style_on_tile(&style, "roads", &road_class_props(class), Tile::new(19808, 10243, 15), FeatureGeometry::Unknown)
            .road_style()
            .map(|road| road.tier)
            .unwrap_or_default()
    };
    for automobile in ["motorway", "trunk", "primary", "secondary", "tertiary", "minor", "service"] {
        assert_eq!(tier(automobile), RoadTier::Automobile, "{automobile} is automobile");
    }
    for pedestrian in ["path", "track"] {
        assert_eq!(tier(pedestrian), RoadTier::Pedestrian, "{pedestrian} is the finer network under it");
    }
    let rail = style_on_tile(&style, "roads", &[("kind", s("rail")), ("kind_detail", s("rail"))], Tile::new(19808, 10243, 15), FeatureGeometry::Unknown);
    assert_eq!(rail.road_style().unwrap().tier, RoadTier::Pedestrian, "a railway is the finer network under the automobile one");
}

#[test]
fn the_tiles_say_nothing_about_the_width() {
    let fill = |style: FeatureStyle| pass_of(&style, RoadPassRole::Fill).unwrap();
    let two_lanes = fill(moscow_road("primary", Some(2), 15, false, None));
    let eight_lanes = fill(moscow_road("primary", Some(8), 15, false, None));
    let stated = fill(moscow_road("primary", Some(8), 15, false, Some(75)));
    for pass in [two_lanes, eight_lanes, stated] {
        assert_eq!(pass.line_width_points, RoadMetrics::DEFAULT_SYMBOL_WIDTH_POINTS.primary);
        assert_eq!(pass.line_geometry.line_width, two_lanes.line_geometry.line_width, "and its ribbon hosts the same points");
    }
}

#[test]
fn the_symbol_is_the_same_on_every_tile_zoom() {
    let passes: Vec<LinePass> = [13, 14, 15]
        .into_iter()
        .filter_map(|z| pass_of(&moscow_road("primary", Some(6), z, false, None), RoadPassRole::Fill))
        .collect();
    assert_eq!(passes.len(), 3);
    for pass in &passes {
        assert_eq!(pass.line_width_points, passes[0].line_width_points);
        assert_eq!(pass.point_width_world_lock_zoom, RoadMetrics::default().world_lock_zoom);
    }
}

#[test]
fn a_street_zoom_road_draws_kerbless() {
    let road = moscow_road("primary", Some(6), 15, false, None);
    assert!(pass_of(&road, RoadPassRole::Fill).is_some(), "The fill draws");
    assert!(pass_of(&road, RoadPassRole::Casing).is_none(), "and nothing outlines it");
}

#[test]
fn nothing_is_painted_on_a_two_way_symbol() {
    for class in ["motorway", "trunk", "primary", "secondary", "tertiary", "minor", "service"] {
        for lanes in [2, 4, 6] {
            assert!(pass_of(&moscow_road(class, Some(lanes), 15, false, None), RoadPassRole::Detail).is_none(), "{class}, {lanes} lanes");
            assert!(pass_of(&moscow_road(class, Some(lanes), 15, true, None), RoadPassRole::Detail).is_some(), "{class}, one way");
        }
    }
}

#[test]
fn borders_keep_their_screen_dashes() {
    let style = default_style();
    let region = style_on_tile(&style, "boundaries", &[("kind", s("region"))], moscow_tile(15), FeatureGeometry::Unknown);
    assert!(!primary_pass(&region).unwrap().dash_in_tile_units);
    assert_eq!(dash_length_points(&region), 7.0);
    let country = style_on_tile(&style, "boundaries", &[("kind", s("country"))], moscow_tile(15), FeatureGeometry::Unknown);
    assert_eq!(dash_length_points(&country), 0.0);
}

#[test]
fn a_ferry_is_a_point_locked_dashed_stroke() {
    let ferry = moscow_road("ferry", None, 10, false, None);
    assert_eq!(line_width_points(&ferry), ProtomapsBasemapDefaultMapStyle::FERRY_WIDTH_POINTS);
    assert_eq!(dash_length_points(&ferry), ProtomapsBasemapDefaultMapStyle::FERRY_DASH_POINTS);
    assert!(!primary_pass(&ferry).unwrap().dash_in_tile_units, "a route across water is a screen pattern");
    assert!(suppresses_polygon_fill(&ferry));
}

#[test]
fn paint_draws_over_every_carriageway() {
    assert!(RoadPassRole::Detail.raw_value() > RoadPassRole::Fill.raw_value());
    assert!(RoadPassRole::Fill.raw_value() > RoadPassRole::Casing.raw_value());
}

#[test]
fn roads_are_symbols_of_a_fixed_width_on_screen() {
    let primary = style_on_tile(&default_style(), "roads", &road_class_props("primary"), Tile::new(19807, 10243, 15), FeatureGeometry::Unknown);
    let fill = pass_of(&primary, RoadPassRole::Fill).unwrap();
    assert_eq!(fill.line_width_points, RoadMetrics::DEFAULT_SYMBOL_WIDTH_POINTS.primary);
    assert!(pass_of(&primary, RoadPassRole::Casing).is_none());
    assert!(fill.line_geometry.line_width > 0.0, "the ribbon hosts the point width");
}

#[test]
fn the_theme_states_the_symbol_width_the_world_lock_and_the_first_zoom_of_a_class() {
    let standard = ProtomapsBasemapTheme::default();
    let style = ProtomapsBasemapDefaultMapStyle::new(standard.clone());
    assert_eq!(fill_of(&style, "minor", 14).unwrap().point_width_world_lock_zoom, 15.0);
    assert!(fill_of(&style, "minor", 12).is_some(), "the small network draws from the tile that ships it");
    let changed = standard.with_road_metrics(|metrics| {
        metrics.world_lock_zoom = 15.5;
        metrics.symbol_width_points.minor = 3.0;
        metrics.minimum_tile_zoom.minor = 14;
    });
    let changed_style = ProtomapsBasemapDefaultMapStyle::new(changed.clone());
    assert_eq!(fill_of(&changed_style, "minor", 14).unwrap().point_width_world_lock_zoom, 15.5);
    assert_eq!(fill_of(&changed_style, "minor", 14).unwrap().line_width_points, 3.0);
    assert!(fill_of(&changed_style, "minor", 13).is_none(), "a theme can still hold a class back");
    assert_ne!(changed.cache_fingerprint(), standard.cache_fingerprint());
}

#[test]
fn the_world_lock_zoom_reaches_the_shader_style() {
    let pass = LinePass {
        line_width_points: 4.0,
        point_width_world_lock_zoom: 14.0,
        ..LinePass::new(1, [1.0; 4], LineGeometryStyle::new(48.0))
    };
    assert_eq!(TileLineStyle::from_pass(&pass).world_lock_zoom, 14.0);
}

/// The sphere's veil on a stroke is a style field the parser bakes: one
/// unless the style says otherwise, so every other line is the same on
/// both surfaces.
#[test]
fn the_sphere_alpha_reaches_the_shader_style() {
    let plain = LinePass::new(1, [1.0; 4], LineGeometryStyle::new(8.0));
    assert_eq!(plain.sphere_alpha, 1.0);
    assert_eq!(TileLineStyle::from_pass(&plain).sphere_alpha, 1.0);
    let veiled = LinePass { sphere_alpha: 0.4, ..plain };
    assert_eq!(TileLineStyle::from_pass(&veiled).sphere_alpha, 0.4);
    assert_eq!(TileLineStyle::POLYGON.sphere_alpha, 1.0);
    let sphere_rgb = |style: TileLineStyle| [style.sphere_color_red, style.sphere_color_green, style.sphere_color_blue];
    let grey = LinePass::new(1, [0.5, 0.6, 0.7, 1.0], LineGeometryStyle::new(8.0));
    assert_eq!(sphere_rgb(TileLineStyle::from_pass(&grey)), [0.5, 0.6, 0.7]);
    let yellow = LinePass { sphere_color: Some([1.0, 0.8, 0.3]), ..grey };
    assert_eq!(sphere_rgb(TileLineStyle::from_pass(&yellow)), [1.0, 0.8, 0.3]);
    // A fill's globe colour rides in the same slot of its line style.
    let own = line_baking::baked_fill_pass(&FillStyle::new(2, [0.1, 0.2, 0.3, 1.0]));
    assert_eq!(sphere_rgb(TileLineStyle::from_pass(&own)), [0.1, 0.2, 0.3]);
    let daylit = line_baking::baked_fill_pass(&FillStyle { sphere_color: Some([0.6, 0.7, 0.9]), ..FillStyle::new(2, [0.1, 0.2, 0.3, 1.0]) });
    assert_eq!(sphere_rgb(TileLineStyle::from_pass(&daylit)), [0.6, 0.7, 0.9]);
}

/// The overview strokes wear the theme's globe veil, the street era's
/// symbols do not: a road of the street zooms is never on the sphere.
#[test]
fn the_overview_strokes_wear_the_globe_veil() {
    let standard = ProtomapsBasemapTheme::default();
    let style = ProtomapsBasemapDefaultMapStyle::new(standard.clone());
    assert_eq!(standard.road_metrics.globe_opacity, 0.6);
    assert_eq!(fill_of(&style, "motorway", 6).unwrap().sphere_alpha, 0.6);
    assert_eq!(fill_of(&style, "motorway", 7).unwrap().sphere_alpha, 0.6);
    assert_eq!(fill_of(&style, "minor", 14).unwrap().sphere_alpha, 1.0);
    let network = standard.layers.overview_roads;
    assert_eq!(fill_of(&style, "motorway", 6).unwrap().sphere_color, Some(rgb(network.major)));
    assert_eq!(fill_of(&style, "trunk", 6).unwrap().sphere_color, Some(rgb(network.major)));
    assert_eq!(fill_of(&style, "primary", 7).unwrap().sphere_color, Some(rgb(network.minor)));
    assert_eq!(fill_of(&style, "minor", 14).unwrap().sphere_color, None);
    let lifted = standard.with_road_metrics(|metrics| metrics.globe_opacity = 1.0);
    assert_eq!(fill_of(&ProtomapsBasemapDefaultMapStyle::new(lifted.clone()), "motorway", 6).unwrap().sphere_alpha, 1.0);
    assert_ne!(lifted.cache_fingerprint(), standard.cache_fingerprint());
}

#[test]
fn road_markings_show_wherever_the_tile_states_them() {
    for zoom in [12.0, 13.0, 14.9, 15.0, 16.0] {
        assert_eq!(ProtomapsBasemapDefaultMapStyle::ROAD_MARKING_ZOOM_FADE.alpha(zoom), 1.0, "z{zoom}");
    }
}

#[test]
fn class_fade_comes_in_over_the_zoom_level_after_its_start() {
    let fade = ProtomapsBasemapDefaultMapStyle::class_zoom_fade(5);
    assert_eq!(fade, ZoomFade::fade_in(5.0, 6.0));
    assert_eq!(fade.alpha(4.9), 0.0);
    assert_eq!(fade.alpha(5.0), 0.0);
    assert_close(fade.alpha(5.5), 0.5, 1e-6);
    assert_eq!(fade.alpha(6.0), 1.0);
    assert_eq!(fade.alpha(9.0), 1.0);
    assert_eq!(ProtomapsBasemapDefaultMapStyle::class_zoom_fade(7).alpha(7.25), fade.alpha(5.25));
}

/// Below the style's road path zoom an overview stroke is a plain line.
/// From it the same stroke is a road, so the parser stitches and sorts it.
#[test]
fn the_overview_stroke_is_a_line_below_the_road_path_zoom_and_a_road_from_it() {
    let style = default_style();
    assert_eq!(ProtomapsBasemapDefaultMapStyle::ROAD_PATH_MINIMUM_TILE_ZOOM, 8);
    let below = road_of(&style, "motorway", 7);
    let from = road_of(&style, "motorway", 8);
    assert!(matches!(below, FeatureStyle::Line(_)), "A z7 motorway is a ground line");
    assert!(matches!(from, FeatureStyle::Road(_)), "A z8 motorway is a road");
    assert_eq!(passes(&below)[0].pass.line_width_points, passes(&from)[0].pass.line_width_points);
    assert_eq!(passes(&below)[0].pass.color, passes(&from)[0].pass.color);
}

/// The casing eases in from the style's casing zoom, as the pass's own fade
/// band, while the fill keeps the road band.
#[test]
fn the_casing_carries_its_zoom_as_the_fade_band() {
    assert_eq!(ProtomapsBasemapDefaultMapStyle::CASING_MINIMUM_CAMERA_ZOOM, 16);
    assert!(pass_of(&road_of(&default_style(), "motorway", 14), RoadPassRole::Casing).is_none());
    let cased = ProtomapsBasemapDefaultMapStyle::new(ProtomapsBasemapTheme::default().with_road_metrics(|metrics| metrics.draws_casing = true));
    let street = road_of(&cased, "motorway", 14);
    let casing = pass_of(&street, RoadPassRole::Casing).unwrap();
    let fill = pass_of(&street, RoadPassRole::Fill).unwrap();
    assert_eq!(casing.zoom_fade, ZoomFade::fade_in(16.0, 17.0));
    assert_eq!(casing.zoom_fade.alpha(15.99), 0.0);
    assert_eq!(casing.zoom_fade.alpha(17.0), 1.0);
    assert_eq!(fill.zoom_fade, ProtomapsBasemapDefaultMapStyle::ROAD_ZOOM_FADE);
}

/// The country view arrives as one change: the land cover and the biomes
/// fade out over zoom 6.7 to 7.1, and inside that fade the roads switch
/// from the globe's highway yellow under its veil to their own look, at
/// once.
#[test]
fn the_roads_switch_while_the_land_cover_fades() {
    let fade = ProtomapsBasemapDefaultMapStyle::landcover_zoom_fade();
    let switch_zoom = ProtomapsBasemapDefaultMapStyle::ROAD_LOOK_SWITCH_ZOOM;
    assert_close(fade.alpha(6.7), 1.0, 1e-6);
    assert_close(fade.alpha(7.1), 0.0, 1e-6);
    let at_switch = fade.alpha(switch_zoom as f64);
    assert!(at_switch > 0.0 && at_switch < 1.0, "the roads switch inside the cover's fade");
    let road = default_style().overview_road_style(
        OverviewRoadStroke { fill_key: 60, priority: 1 },
        RoadClass::Motorway,
        false,
        None,
        [0.5, 0.5, 0.5, 1.0],
        7,
        false,
    );
    let pass = primary_pass(&road).unwrap();
    assert_eq!(pass.sphere_look_until_zoom, Some(switch_zoom));
    assert!(pass.sphere_color.is_some(), "the road wears the globe's highway yellow up to the switch");
    assert!(pass.sphere_alpha < 1.0, "and its veil, which stays translucent until it switches");
}

/// The switch zoom rides to the GPU, and a stroke without one has none.
#[test]
fn the_line_style_carries_the_switch_zoom() {
    let geometry = LineGeometryStyle::rounded(4.0, true, true);
    let switching = LinePass {
        sphere_alpha: 0.6,
        sphere_color: Some([1.0, 0.8, 0.3]),
        sphere_look_until_zoom: Some(7.0),
        ..LinePass::new(1, [0.5, 0.5, 0.5, 1.0], geometry)
    };
    assert_eq!(TileLineStyle::from_pass(&switching).sphere_look_until_zoom, 7.0);
    let plain = LinePass::new(2, [0.5, 0.5, 0.5, 1.0], geometry);
    assert_eq!(TileLineStyle::from_pass(&plain).sphere_look_until_zoom, 0.0);
}

/// The world-locked dash contract between `TileLineStyle` and the tile
/// shaders: the flag lives in the sixth float, and the struct is the 17
/// floats the shaders read, in their order. The shader-source checks of
/// the engine's test belong to the WGSL port.
#[test]
fn the_line_style_layout_is_the_shaders() {
    assert_eq!(std::mem::size_of::<TileLineStyle>(), 68);
    assert_eq!(std::mem::offset_of!(TileLineStyle, dash_in_tile_units), 20);
    assert_eq!(std::mem::offset_of!(TileLineStyle, half_width_units), 24);
    assert_eq!(std::mem::offset_of!(TileLineStyle, world_lock_zoom), 28);
    assert_eq!(std::mem::offset_of!(TileLineStyle, ramp_start_width_points), 32);
    assert_eq!(std::mem::offset_of!(TileLineStyle, ramp_start_alpha), 44);
    assert_eq!(std::mem::offset_of!(TileLineStyle, sphere_alpha), 48);
    assert_eq!(std::mem::offset_of!(TileLineStyle, sphere_color_red), 52);
    assert_eq!(std::mem::offset_of!(TileLineStyle, sphere_color_blue), 60);
    assert_eq!(std::mem::offset_of!(TileLineStyle, sphere_look_until_zoom), 64);
    assert_eq!(std::mem::size_of::<map_style::style::TilePolygonStyle>(), 16);
}

#[test]
fn the_parser_bakes_the_flag_into_the_gpu_style() {
    let flagged = LinePass {
        line_width_points: 1.0,
        dash_length_points: 40.0,
        dash_gap_points: 80.0,
        dash_in_tile_units: true,
        ..LinePass::new(1, [1.0; 4], LineGeometryStyle::new(8.0))
    };
    let plain = LinePass {
        line_width_points: 1.0,
        dash_length_points: 7.0,
        dash_gap_points: 3.5,
        ..LinePass::new(2, [1.0; 4], LineGeometryStyle::new(8.0))
    };
    assert_eq!(TileLineStyle::from_pass(&flagged).dash_in_tile_units, 1.0);
    assert_eq!(TileLineStyle::from_pass(&plain).dash_in_tile_units, 0.0);
    assert!(passes(&FeatureStyle::Line(LineStyle::new(flagged)))[0].pass.dash_in_tile_units);
    assert!(!passes(&FeatureStyle::Line(LineStyle::new(plain)))[0].pass.dash_in_tile_units);
}
