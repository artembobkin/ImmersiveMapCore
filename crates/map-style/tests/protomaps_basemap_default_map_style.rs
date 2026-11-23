// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Ported from `ProtomapsBasemapDefaultMapStyleTests`: the built-in style's
//! ground, roads, POIs, borders and buildings.

mod support;

use std::collections::HashSet;

use map_style::schema::FeatureGeometry;
use map_style::style::default::{ProtomapsBasemapDefaultMapStyle, ProtomapsBasemapTheme};
use map_style::style::{FeatureStyle, OnewayArrowDecoration, PoiSpriteIcon, RoadDecorationKind, RoadPassRole, VectorTileStyle, WidthRamp, ZoomFade};
use mvt::Value;
use support::*;

/// A feature of a layer, its `kind`, `kind_detail` and `min_zoom` stated,
/// with a name for a POI or a place, on the tile at the origin of a zoom.
#[allow(clippy::too_many_arguments)]
fn make(
    style: &ProtomapsBasemapDefaultMapStyle,
    layer: &str,
    kind: Option<&'static str>,
    kind_detail: Option<&'static str>,
    min_zoom: Option<i64>,
    zoom: i32,
    geometry: FeatureGeometry,
    extra: &[(&'static str, Value<'static>)],
) -> FeatureStyle {
    let mut props: Props = extra.to_vec();
    if let Some(kind) = kind {
        props.push(("kind", s(kind)));
    }
    if let Some(detail) = kind_detail {
        props.push(("kind_detail", s(detail)));
    }
    if let Some(min_zoom) = min_zoom {
        props.push(("min_zoom", i(min_zoom)));
    }
    if (layer == "pois" || layer == "places") && !props.iter().any(|(key, _)| *key == "name") {
        props.push(("name", s("Test")));
    }
    style_of(style, layer, &props, zoom, geometry)
}

fn kind(style: &ProtomapsBasemapDefaultMapStyle, layer: &str, kind: &'static str, zoom: i32) -> FeatureStyle {
    make(style, layer, Some(kind), None, None, zoom, FeatureGeometry::Unknown, &[])
}

fn road(style: &ProtomapsBasemapDefaultMapStyle, kind: &'static str, detail: Option<&'static str>, zoom: i32) -> FeatureStyle {
    make(style, "roads", Some(kind), detail, None, zoom, FeatureGeometry::Unknown, &[])
}

fn road_with(style: &ProtomapsBasemapDefaultMapStyle, kind: &'static str, detail: Option<&'static str>, zoom: i32, extra: &[(&'static str, Value<'static>)]) -> FeatureStyle {
    make(style, "roads", Some(kind), detail, None, zoom, FeatureGeometry::Unknown, extra)
}

fn poi(style: &ProtomapsBasemapDefaultMapStyle, kind: &'static str, min_zoom: i64, zoom: i32) -> FeatureStyle {
    make(style, "pois", Some(kind), None, Some(min_zoom), zoom, FeatureGeometry::Point, &[])
}

fn polygon(style: &ProtomapsBasemapDefaultMapStyle, layer: &str, kind: Option<&'static str>, zoom: i32) -> FeatureStyle {
    make(style, layer, kind, None, None, zoom, FeatureGeometry::Polygon, &[])
}

fn icon(style: &FeatureStyle) -> Option<PoiSpriteIcon> {
    style.point_label_style().and_then(|label| label.icon)
}

/// The default theme with the overview's biomes never drawn: the land cover
/// rules as they stand with the overview off.
fn theme_without_the_overview() -> ProtomapsBasemapTheme {
    ProtomapsBasemapTheme::default().with_layers(|layers| layers.biomes.maximum_tile_zoom = -1)
}

fn blend(base: [f32; 4], target: [f32; 4], amount: f32) -> [f32; 4] {
    [0, 1, 2, 3].map(|index| base[index] + (target[index] - base[index]) * amount)
}

/// The land cover's own rules, with the overview's biomes never drawn.
#[test]
fn ground_styles_use_the_layer_colour_at_every_zoom() {
    let configuration = theme_without_the_overview();
    let style = ProtomapsBasemapDefaultMapStyle::new(configuration.clone());
    // One colour per class, at every tile zoom: the globe and the street map
    // wear the same water and the same land.
    for zoom in [2, 8, 9, 10, 12] {
        assert_eq!(color(&polygon(&style, "water", None, zoom)), configuration.layers.water, "z{zoom}");
        let earth = polygon(&style, "earth", Some("earth"), zoom);
        assert_eq!(earth.key(), 2, "z{zoom}");
        assert_eq!(color(&earth), configuration.layers.land, "z{zoom}");
        // The base quad is the land as well, under the earth polygon.
        let background = style.background_style(zoom);
        assert_eq!(background.key(), 1, "z{zoom}");
        assert_eq!(color(&background), configuration.layers.land, "z{zoom}");
    }
    // A land cover class wears the colour of the land use kind that replaces
    // it at z8, so the swap changes geometry, not the colour.
    let cover_forest = kind(&style, "landcover", "forest", 7);
    let use_forest = kind(&style, "landuse", "forest", 8);
    assert_eq!(color(&use_forest), configuration.layers.wood);
    let step = (0..3).map(|index| (color(&cover_forest)[index] - color(&use_forest)[index]).abs()).fold(0.0, f32::max);
    assert!(step < 1.0 / 255.0, "The z7 land cover forest must read as the z8 land use forest");
    // Cities render: the urban area class is the residential tone.
    let urban = kind(&style, "landcover", "urban_area", 6);
    assert_eq!(urban.key(), 10);
    assert_eq!(color(&urban), configuration.layers.residential);
}

/// The land cover's own look on the globe, with the overview's biomes
/// never drawn: the way the overview zooms draw with the overview off.
#[test]
fn massive_overview_merges_vegetation_classes_through_zoom_two() {
    let configuration = theme_without_the_overview();
    let colors = configuration.layers;
    let style = ProtomapsBasemapDefaultMapStyle::new(configuration);
    for class in ["grassland", "scrub", "farmland"] {
        assert_eq!(color(&kind(&style, "landcover", class, 2)), colors.grass, "Expected massive overview color for {class}");
    }
    let overview_forest = blend(colors.grass, colors.wood, 0.25);
    let forest = color(&kind(&style, "landcover", "forest", 2));
    for index in 0..4 {
        assert_close(forest[index], overview_forest[index], 1e-6);
    }
    assert_eq!(color(&kind(&style, "landcover", "farmland", 3)), blend(colors.farmland, colors.grass, 0.5));
    assert_eq!(color(&kind(&style, "landcover", "forest", 3)), blend(colors.wood, colors.grass, 0.5 * 0.75));
    assert_eq!(color(&kind(&style, "landcover", "forest", 7)), blend(colors.wood, colors.grass, 0.04 * 0.75));
}

/// Open country has no land use counterpart: from z8 it is the bare earth
/// polygon. So grassland and scrub leave the globe's green on the same
/// curve as the fields and reach the land tone by z7.
#[test]
fn open_country_releases_to_the_land_tone_by_the_last_landcover_zoom() {
    let configuration = theme_without_the_overview();
    let colors = configuration.layers;
    let style = ProtomapsBasemapDefaultMapStyle::new(configuration);
    for class in ["grassland", "scrub"] {
        assert_eq!(color(&kind(&style, "landcover", class, 3)), blend(colors.land, colors.grass, 0.5), "{class} at z3");
        assert_eq!(color(&kind(&style, "landcover", class, 7)), blend(colors.land, colors.grass, 0.04), "{class} at z7");
    }
}

#[test]
fn landcover_and_landuse_keep_apart_on_the_tiles_they_share() {
    let configuration = theme_without_the_overview();
    let colors = configuration.layers;
    let style = ProtomapsBasemapDefaultMapStyle::new(configuration);
    let cover = [
        ("barren", 3u8, colors.sand),
        ("grassland", 4, blend(colors.land, colors.grass, 0.04)),
        ("scrub", 4, blend(colors.land, colors.grass, 0.04)),
        ("glacier", 8, colors.ice),
        ("urban_area", 10, colors.residential),
    ];
    for (class, key, expected) in cover {
        let at7 = kind(&style, "landcover", class, 7);
        assert_eq!(at7.key(), key, "Unexpected key for {class}");
        assert_eq!(color(&at7), expected, "Unexpected color for {class}");
    }
    let usage = [
        ("residential", 9u8, colors.residential),
        ("industrial", 9, colors.industrial),
        ("school", 9, colors.industrial),
        ("wood", 11, colors.wood),
        ("park", 12, colors.grass),
        ("cemetery", 12, colors.grass),
        ("orchard", 13, colors.farmland),
        ("wetland", 14, colors.wetland),
        ("glacier", 17, colors.ice),
        ("beach", 18, colors.sand),
        ("runway", 19, colors.aeroway),
    ];
    for (class, key, expected) in usage {
        let at8 = kind(&style, "landuse", class, 8);
        assert_eq!(at8.key(), key, "Unexpected key for {class}");
        assert_eq!(color(&at8), expected, "Unexpected color for {class}");
    }
    let cover_keys: HashSet<u8> = cover.iter().map(|entry| entry.1).collect();
    let use_keys: HashSet<u8> = usage.iter().map(|entry| entry.1).collect();
    assert!(cover_keys.is_disjoint(&use_keys), "Land cover and land use share the coarse tiles, so they must not share a key");
    // A protected area blankets whole city centres: a faint tint under every
    // other fill, never the park green.
    let protected = kind(&style, "landuse", "protected_area", 12);
    assert!(protected.key() < 9);
    assert_ne!(color(&protected), colors.grass);
    // A kind the style does not name draws in the built tone.
    assert_eq!(kind(&style, "landuse", "brownfield", 12).key(), 9);
    assert_eq!(color(&kind(&style, "landuse", "brownfield", 12)), colors.industrial);
}

/// A bridge's deck ships as a `pedestrian` area. It draws among the ground
/// lines, over the water, the waterway lines and the ferry routes, in the
/// land tone, on every tile that ships it.
#[test]
fn bridge_deck_draws_above_the_water() {
    let style = default_style();
    let water = make(&style, "water", Some("water"), None, None, 15, FeatureGeometry::Polygon, &[]);
    let waterway = make(&style, "water", Some("river"), None, None, 15, FeatureGeometry::Line, &[]);
    let ferry = make(&style, "roads", Some("ferry"), None, None, 15, FeatureGeometry::Line, &[]);
    for class in ["pedestrian", "pier"] {
        let deck = kind(&style, "landuse", class, 15);
        assert!(deck.key() > water.key(), "{class} lies over the river");
        assert!(deck.key() > waterway.key(), "{class} hides the river's centre line");
        assert!(deck.key() > ferry.key(), "{class} hides the ferry routes under the bridge");
        let fill = deck.fill_style().expect("a deck is a fill");
        assert!(fill.draws_among_ground_lines, "{class} draws with the lines it covers");
        assert_eq!(color(&deck), style.theme.layers.land);
        assert_eq!(kind(&style, "landuse", class, 10).key(), deck.key(), "{class} draws on the coarser tiles too");
    }
}

/// The land cover, which the basemap ships through z7 only, fades out with
/// the camera, and past the overview the land use has no fade.
#[test]
fn landcover_fades_out_over_the_last_zoom_of_its_tiles_and_landuse_draws_everywhere() {
    let style = ProtomapsBasemapDefaultMapStyle::new(theme_without_the_overview());
    for class in ["barren", "grassland", "farmland", "forest", "glacier", "urban_area"] {
        for zoom in [2, 5, 7, 9] {
            let fill = kind(&style, "landcover", class, zoom);
            assert_ne!(fill.key(), 0, "{class} z{zoom}");
            assert_eq!(zoom_fade(&fill), ProtomapsBasemapDefaultMapStyle::landcover_zoom_fade(), "{class} z{zoom}");
        }
    }
    for class in ["residential", "wood", "park", "farmland", "runway", "protected_area", "other", "platform"] {
        for zoom in [8, 14] {
            let fill = kind(&style, "landuse", class, zoom);
            assert_ne!(fill.key(), 0, "{class} z{zoom}");
            assert_eq!(zoom_fade(&fill), ZoomFade::NONE, "{class} z{zoom}");
        }
    }
}

/// The cover's fade in is the theme's span from the cover zoom, on every
/// tile whose zoom the camera is within the span at: none with a span of
/// nothing, and the span is baked into the tiles.
#[test]
fn the_cover_comes_in_over_the_themes_span() {
    let standard = default_style();
    assert_eq!(standard.cover_fade_in(2), Some(ZoomFade::fade_in(2.0, 3.0)));
    assert_eq!(standard.cover_fade_in(3), None);
    assert_eq!(standard.cover_fade_in(1), None, "Under the cover zoom there is no cover");
    let wide = ProtomapsBasemapDefaultMapStyle::new(ProtomapsBasemapTheme::default().with_layers(|layers| layers.biomes.cover_fade_in_zooms = 2.0));
    assert_eq!(wide.cover_fade_in(2), Some(ZoomFade::fade_in(2.0, 4.0)));
    assert_eq!(wide.cover_fade_in(3), Some(ZoomFade::fade_in(2.0, 4.0)));
    assert_eq!(wide.cover_fade_in(4), None);
    let whole = ProtomapsBasemapDefaultMapStyle::new(ProtomapsBasemapTheme::default().with_layers(|layers| layers.biomes.cover_fade_in_zooms = 0.0));
    assert_eq!(whole.cover_fade_in(2), None);
    assert_eq!(zoom_fade(&kind(&whole, "landcover", "forest", 2)), ProtomapsBasemapDefaultMapStyle::landcover_zoom_fade());
    assert_ne!(whole.cache_fingerprint(), standard.cache_fingerprint(), "The span is baked into the tiles");
}

/// At the overview zooms the engine lays its biomes into the tiles and the
/// biomes paint the land. On the globe they paint it alone, with the ice
/// and the cities. From the cover zoom the tiles' own forests, fields, bare
/// ground, wetlands and ice draw over them in the cover tones, under the
/// water. Past the overview the biomes and the lakes are hidden and the
/// land use draws as on the street map.
#[test]
fn overview_biomes_paint_the_globe_and_the_land_cover_draws_over_them() {
    let configuration = ProtomapsBasemapTheme::default();
    let colors = configuration.layers;
    let biomes = colors.biomes;
    let style = ProtomapsBasemapDefaultMapStyle::new(configuration.clone());
    assert_eq!(biomes.maximum_tile_zoom, 7);
    assert_eq!(biomes.cover_minimum_tile_zoom, 2);
    let painted = [
        ("tropical_moist_forest", biomes.tropical_moist_forest),
        ("tropical_dry_forest", biomes.tropical_dry_forest),
        ("tropical_conifer_forest", biomes.tropical_conifer_forest),
        ("temperate_broadleaf_forest", biomes.temperate_broadleaf_forest),
        ("temperate_conifer_forest", biomes.temperate_conifer_forest),
        ("boreal_forest", biomes.boreal_forest),
        ("tropical_grassland", biomes.tropical_grassland),
        ("temperate_grassland", biomes.temperate_grassland),
        ("flooded_grassland", biomes.flooded_grassland),
        ("montane_grassland", biomes.montane_grassland),
        ("tundra", biomes.tundra),
        ("mediterranean_scrub", biomes.mediterranean_scrub),
        ("desert", biomes.desert),
        ("mangrove", biomes.mangrove),
    ];
    for zoom in [0, 1, 2, 3, 5, 7] {
        let earth = polygon(&style, "earth", Some("earth"), zoom);
        let water = polygon(&style, "water", None, zoom);
        let ice = kind(&style, "landcover", "glacier", zoom);
        let city = kind(&style, "landcover", "urban_area", zoom);
        let mut keys = HashSet::new();
        for (class, expected) in painted {
            let biome = kind(&style, "biome", class, zoom);
            assert_eq!(color(&biome), expected, "{class} z{zoom}");
            assert_eq!(zoom_fade(&biome), ProtomapsBasemapDefaultMapStyle::landcover_zoom_fade());
            assert!(biome.key() > earth.key());
            assert!(biome.key() < ice.key());
            assert!(biome.key() < city.key());
            assert!(biome.key() < water.key());
            keys.insert(biome.key());
        }
        assert_eq!(keys.len(), painted.len(), "One key per biome at z{zoom}");
        assert_eq!(color(&kind(&style, "biome", "moon", zoom)), biomes.temperate_grassland);
        assert_eq!(color(&ice), colors.ice);
        assert_eq!(zoom_fade(&ice), ProtomapsBasemapDefaultMapStyle::landcover_zoom_fade());
        let use_ice = kind(&style, "landuse", "glacier", zoom);
        assert_eq!(color(&use_ice), colors.ice);
        assert_eq!(zoom_fade(&use_ice), ZoomFade::NONE, "The land use's ice stays at z{zoom}");
        assert!(use_ice.key() > city.key());
        assert!(use_ice.key() < water.key());
        assert_eq!(color(&city), colors.residential);
        for class in ["grassland", "scrub"] {
            assert_eq!(kind(&style, "landcover", class, zoom).key(), 0, "{class} z{zoom}");
        }
        for class in ["grassland", "meadow", "national_park", "nature_reserve", "park", "military", "residential", "industrial", "protected_area"] {
            assert_eq!(kind(&style, "landuse", class, zoom).key(), 0, "{class} z{zoom}");
        }
        let lake = polygon(&style, "lake", Some("lake"), zoom);
        assert_eq!(lake.key(), water.key());
        assert_eq!(color(&lake), colors.water);

        let cover = [
            ("forest", biomes.forest_cover, vec!["forest", "wood"]),
            ("farmland", biomes.farmland_cover, vec!["farmland", "orchard", "vineyard"]),
            ("barren", biomes.barren_cover, vec!["sand", "beach", "bare_rock", "scree"]),
        ];
        if zoom < biomes.cover_minimum_tile_zoom {
            for (class, _, use_kinds) in &cover {
                assert_eq!(kind(&style, "landcover", class, zoom).key(), 0, "{class} z{zoom}");
                for use_kind in use_kinds {
                    assert_eq!(kind(&style, "landuse", use_kind, zoom).key(), 0, "{use_kind} z{zoom}");
                }
            }
            assert_eq!(kind(&style, "landuse", "wetland", zoom).key(), 0);
        } else {
            let cover_fade = if zoom == biomes.cover_minimum_tile_zoom {
                ZoomFade::fade_in(zoom as f64, zoom as f64 + 1.0)
            } else {
                ProtomapsBasemapDefaultMapStyle::landcover_zoom_fade()
            };
            let use_fade = if zoom == biomes.cover_minimum_tile_zoom { cover_fade } else { ZoomFade::NONE };
            for (class, expected, use_kinds) in &cover {
                let fill = kind(&style, "landcover", class, zoom);
                assert_eq!(color(&fill), *expected, "{class} z{zoom}");
                assert_eq!(zoom_fade(&fill), cover_fade, "{class} z{zoom}");
                assert!(fill.key() > *keys.iter().max().unwrap());
                assert!(fill.key() < city.key());
                for use_kind in use_kinds {
                    let usage = kind(&style, "landuse", use_kind, zoom);
                    assert!(usage.key() > city.key());
                    assert!(usage.key() < water.key());
                    assert_eq!(color(&usage), *expected, "{use_kind} wears the {class} cover's tone at z{zoom}");
                    assert_eq!(zoom_fade(&usage), use_fade);
                }
            }
            let wetland = kind(&style, "landuse", "wetland", zoom);
            assert_eq!(color(&wetland), colors.wetland);
            assert_eq!(zoom_fade(&wetland), use_fade);
            assert!(wetland.key() > city.key());
            assert!(wetland.key() < use_ice.key());
        }
    }
    for (class, _) in painted {
        assert_eq!(kind(&style, "biome", class, 8).key(), 0, "{class} at z8");
    }
    assert_eq!(polygon(&style, "lake", Some("lake"), 8).key(), 0);
    assert_eq!(kind(&style, "landuse", "wood", 8).key(), 11);
    assert_eq!(color(&kind(&style, "landuse", "wood", 8)), colors.wood);
    assert_eq!(kind(&style, "landuse", "park", 8).key(), 12);
    // The zooms are the theme's.
    let shorter = ProtomapsBasemapDefaultMapStyle::new(configuration.with_layers(|layers| layers.biomes.maximum_tile_zoom = 4));
    assert_eq!(kind(&shorter, "biome", "desert", 5).key(), 0);
    assert_ne!(kind(&shorter, "landcover", "grassland", 5).key(), 0, "Past a shorter overview the land cover draws whole again");
    let later = ProtomapsBasemapDefaultMapStyle::new(configuration.with_layers(|layers| layers.biomes.cover_minimum_tile_zoom = 5));
    assert_eq!(kind(&later, "landcover", "forest", 4).key(), 0);
    assert_ne!(kind(&later, "landcover", "forest", 5).key(), 0);
}

#[test]
fn every_road_draws_from_the_first_tile_that_ships_it() {
    let style = default_style();
    let plain = ProtomapsBasemapDefaultMapStyle::new(theme_without_the_overview());
    let classes: [(&'static str, Option<&'static str>); 16] = [
        ("highway", Some("motorway")),
        ("major_road", Some("trunk")),
        ("major_road", Some("primary")),
        ("ferry", None),
        ("major_road", Some("secondary")),
        ("major_road", Some("tertiary")),
        ("aeroway", Some("runway")),
        ("aeroway", Some("apron")),
        ("minor_road", Some("residential")),
        ("minor_road", Some("service")),
        ("path", Some("footway")),
        ("path", Some("sidewalk")),
        ("path", Some("crossing")),
        ("rail", Some("subway")),
        ("rail", Some("rail")),
        ("aerialway", Some("cable_car")),
    ];
    for (class, detail) in classes {
        for zoom in [6, 10, 12, 14, 15] {
            assert_ne!(road(&style, class, detail, zoom).key(), 0, "{class}/{detail:?} z{zoom}");
        }
        assert_ne!(road(&plain, class, detail, 2).key(), 0, "{class}/{detail:?} z2 without the overview");
    }
    // A subway in its tunnel draws at the tunnel opacity.
    let subway = road_with(&style, "rail", Some("subway"), 14, &[("is_tunnel", b(true))]);
    assert!(color(&subway)[3] < ProtomapsBasemapTheme::default().layers.roads.rail[3]);
    // A theme can still hold a class back.
    let held_back = ProtomapsBasemapDefaultMapStyle::new(ProtomapsBasemapTheme::default().with_road_metrics(|metrics| metrics.minimum_tile_zoom.minor = 14));
    assert_eq!(road(&held_back, "minor_road", Some("residential"), 13).key(), 0);
}

#[test]
fn a_link_sorts_one_step_under_its_parent() {
    let style = default_style();
    let primary = road(&style, "major_road", Some("primary"), 14);
    let ramp = road_with(&style, "major_road", Some("primary_link"), 14, &[("is_link", b(true))]);
    assert_eq!(ramp.road_style().unwrap().class_priority, primary.road_style().unwrap().class_priority - 1);
    assert_eq!(passes(&ramp)[0].pass.color, passes(&primary)[0].pass.color, "in the parent's colour");
    assert_eq!(passes(&ramp)[0].pass.line_width_points, passes(&primary)[0].pass.line_width_points, "and the parent's width");
    let aisle = road_with(&style, "minor_road", Some("service"), 14, &[("service", s("parking_aisle"))]);
    let service = road(&style, "minor_road", Some("service"), 14);
    assert_eq!(aisle.road_style().unwrap().class_priority, service.road_style().unwrap().class_priority - 1);
}

#[test]
fn a_named_road_carries_its_label_on_the_road_path() {
    let style = default_style();
    let named = road_with(&style, "major_road", Some("primary"), 12, &[("name", s("Tverskaya"))]);
    let label = named.road_style().unwrap().label.expect("the name is laid along the road's own symbol");
    assert_eq!(label.key, named.key() as i32);
    let overview = road_with(&style, "major_road", Some("primary"), 10, &[("name", s("Tverskaya"))]);
    assert!(overview.road_style().unwrap().label.is_some(), "the overview stroke carries its name too");
    assert!(road(&style, "major_road", Some("primary"), 12).road_style().unwrap().label.is_none());
}

#[test]
fn a_one_way_street_carries_arrows_in_the_paint_role() {
    let style = default_style();
    let oneway = road_with(&style, "minor_road", Some("residential"), 14, &[("oneway", b(true))]);
    assert_eq!(oneway.road_style().unwrap().decoration, RoadDecorationKind::OnewayArrow(OnewayArrowDecoration::default()));
    let paint = pass_of(&oneway, RoadPassRole::Detail).expect("the arrows are stamped in a paint stroke");
    assert_eq!(paint.color, ProtomapsBasemapTheme::default().layers.roads.marking);
    assert_eq!(paint.zoom_fade, ZoomFade::NONE);
    let two_way = road(&style, "minor_road", Some("residential"), 14);
    assert_eq!(two_way.road_style().unwrap().decoration, RoadDecorationKind::None);
    assert!(pass_of(&two_way, RoadPassRole::Detail).is_none());
    let early = road_with(&style, "major_road", Some("primary"), 13, &[("oneway", b(true))]);
    assert_ne!(early.road_style().unwrap().decoration, RoadDecorationKind::None, "the arrows ride every tile that states oneway");
    let tunnel = road_with(&style, "minor_road", Some("residential"), 14, &[("oneway", b(true)), ("is_tunnel", b(true))]);
    assert_eq!(tunnel.road_style().unwrap().decoration, RoadDecorationKind::None, "a tunnel carries no paint");
}

#[test]
fn roads_show_with_their_tile_without_a_fade() {
    let style = default_style();
    for (class, detail, zoom) in [
        ("highway", "motorway", 6),
        ("major_road", "trunk", 7),
        ("major_road", "primary", 8),
        ("major_road", "secondary", 9),
        ("major_road", "tertiary", 11),
        ("highway", "motorway", 12),
        ("minor_road", "residential", 12),
    ] {
        let road = road(&style, class, Some(detail), zoom);
        assert_eq!(zoom_fade(&road), ZoomFade::NONE, "{detail}");
        for pass in passes(&road).into_iter().filter(|pass| pass.role != RoadPassRole::Casing) {
            assert_eq!(pass.pass.zoom_fade, ZoomFade::NONE, "{detail}");
        }
    }
}

/// The automobile tier is kerbless: the roadway is held by its fill and its
/// paint, not an outline. No drive-tier class carries a casing pass at any
/// zoom.
#[test]
fn automobile_roads_carry_no_casing_pass() {
    let style = default_style();
    for (class, detail) in [
        ("highway", "motorway"),
        ("major_road", "trunk"),
        ("major_road", "primary"),
        ("major_road", "secondary"),
        ("major_road", "tertiary"),
        ("minor_road", "residential"),
        ("minor_road", "service"),
    ] {
        for zoom in [7, 10, 12, 14, 15] {
            assert!(pass_of(&road(&style, class, Some(detail), zoom), RoadPassRole::Casing).is_none(), "{detail} at z{zoom}");
        }
    }
}

/// At the overview zooms the engine lays the Natural Earth road network into
/// the tiles, and the style draws it in place of the basemap's roads, which
/// there are the motorways alone. From the next tile zoom the basemap's
/// roads draw as before, and a theme without the overview never hides them.
#[test]
fn overview_road_network_stands_in_for_the_motorways() {
    let configuration = ProtomapsBasemapTheme::default();
    let network = configuration.layers.overview_roads;
    let style = ProtomapsBasemapDefaultMapStyle::new(configuration.clone());
    assert_eq!(network.maximum_tile_zoom, 5, "The basemap's trunks arrive at tile zoom 6");
    assert_eq!(network.major_maximum_rank, 4);
    let overview_road = |style: &ProtomapsBasemapDefaultMapStyle, zoom: i32, extra: &[(&'static str, Value<'static>)]| {
        make(style, "road", None, None, None, zoom, FeatureGeometry::Line, extra)
    };
    for zoom in [2, 3, 5] {
        let major = overview_road(&style, zoom, &[("kind", s("Road")), ("rank", i(3))]);
        let minor = overview_road(&style, zoom, &[("kind", s("Secondary Highway")), ("rank", i(6))]);
        assert_eq!(color(&major), network.major, "z{zoom}");
        assert_eq!(color(&minor), network.minor, "z{zoom}");
        assert_eq!(line_width_points(&major), network.major_width_points);
        assert_eq!(line_width_points(&minor), network.minor_width_points);
        assert!(major.key() > minor.key(), "The main roads lie over the minor ones at z{zoom}");
        assert!(major.key() < kind(&style, "boundaries", "country", zoom).key(), "The network lies under the borders");
        assert!(suppresses_polygon_fill(&major), "A road is never a blob");
        assert!(line_geometry(&major).line_join_round);
        for line in [&major, &minor] {
            assert_eq!(primary_pass(line).unwrap().sphere_alpha, configuration.road_metrics.globe_opacity);
        }
        assert_eq!(road(&style, "highway", Some("motorway"), zoom).key(), 0, "The motorway is hidden under the network at z{zoom}");
    }
    assert_eq!(color(&overview_road(&style, 4, &[("rank", i(5))])), network.minor);
    assert_eq!(color(&overview_road(&style, 4, &[])), network.minor);
    assert_eq!(overview_road(&style, 6, &[("rank", i(3))]).key(), 0);
    assert_ne!(road(&style, "highway", Some("motorway"), 6).key(), 0);
    assert_ne!(road(&style, "major_road", Some("trunk"), 6).key(), 0);
    let plain = ProtomapsBasemapDefaultMapStyle::new(theme_without_the_overview());
    assert_ne!(road(&plain, "highway", Some("motorway"), 4).key(), 0);
    assert_eq!(overview_road(&plain, 4, &[("rank", i(3))]).key(), 0);
    let yellower = configuration.with_layers(|layers| layers.overview_roads.major = [1.0, 0.9, 0.2, 1.0]);
    assert_ne!(yellower.cache_fingerprint(), configuration.cache_fingerprint());
    let shorter = configuration.with_layers(|layers| layers.overview_roads.maximum_tile_zoom = 4);
    assert_ne!(shorter.cache_fingerprint(), configuration.cache_fingerprint());
    assert_eq!(overview_road(&ProtomapsBasemapDefaultMapStyle::new(shorter), 5, &[("rank", i(3))]).key(), 0);
}

#[test]
fn overview_roads_are_one_asphalt_grey_with_seamless_joins() {
    let configuration = ProtomapsBasemapTheme::default();
    let style = ProtomapsBasemapDefaultMapStyle::new(configuration.clone());
    let motorway = road(&style, "highway", Some("motorway"), 7);
    assert_eq!(passes(&motorway).iter().map(|pass| pass.role).collect::<Vec<_>>(), vec![RoadPassRole::Fill]);
    let fill = passes(&motorway)[0].pass;
    let metrics = configuration.road_metrics;
    assert_eq!(fill.line_width_points, metrics.symbol_width_points.motorway);
    let ramp = fill.point_width_ramp.unwrap();
    assert_eq!(
        ramp,
        WidthRamp {
            start_width_points: metrics.overview_width_points.motorway,
            start_zoom: metrics.overview_zoom,
            end_zoom: metrics.symbol_zoom,
            start_alpha: metrics.overview_opacity,
        }
    );
    assert_eq!(fill.zoom_fade, ZoomFade::NONE, "Overview roads show with their tile");
    assert!(fill.line_geometry.line_cap_round);
    assert!(fill.line_geometry.line_join_round);
    let expected = ramp.width_points(8.0, fill.line_width_points) as f64 * FeatureStyle::POINT_LOCKED_RIBBON_UNITS_PER_POINT;
    assert!((fill.line_geometry.line_width - expected).abs() < 0.01, "The ribbon must host the widest the stroke gets");
    assert_eq!(fill.color, configuration.layers.roads.motorway, "the veil is the ramp's, not the colour's");
    let primary = road(&style, "major_road", Some("primary"), 8);
    assert_eq!(passes(&primary)[0].pass.color, configuration.layers.roads.primary);
    let city_fill = passes(&road(&style, "highway", Some("motorway"), 10))[0].pass;
    assert_eq!(city_fill.point_width_ramp, Some(ramp));
    assert_eq!(city_fill.line_width_points, fill.line_width_points);
    assert_eq!(ramp.width_points(5.0, 7.0), metrics.overview_width_points.motorway);
    assert!(ramp.width_points(8.0, 7.0) < ramp.width_points(11.0, 7.0));
    assert_eq!(ramp.width_points(15.0, 7.0), 7.0);
    for (detail, zoom) in [("trunk", 6), ("secondary", 9), ("tertiary", 11)] {
        let stroke = passes(&road(&style, "major_road", Some(detail), zoom))[0].pass;
        assert!(stroke.line_width_points > 0.0, "{detail} at z{zoom}");
        assert!(stroke.line_geometry.line_join_round, "{detail} at z{zoom}");
    }
    let street_fill = pass_of(&road(&style, "highway", Some("motorway"), 12), RoadPassRole::Fill).unwrap();
    assert_eq!(street_fill.line_width_points, metrics.symbol_width_points.motorway);
    assert_eq!(street_fill.minimum_width_points, 0.0);
    assert_eq!(street_fill.color, configuration.layers.roads.motorway);
    assert_eq!(street_fill.point_width_ramp, Some(ramp), "the street era states the overview era's ramp");
    let tunnel = road_with(&style, "highway", Some("motorway"), 8, &[("is_tunnel", b(true))]);
    assert!(passes(&tunnel)[0].pass.color[3] < fill.color[3]);
    // Rivers carry a width floor so they read from the z9 tiles the basemap
    // first ships them in.
    let river = make(&style, "water", Some("river"), None, None, 9, FeatureGeometry::Line, &[]);
    assert_ne!(river.key(), 0);
    assert_eq!(minimum_width_points(&river), 0.7);
    // A culverted stream stays on the map at the tunnel opacity, on its own
    // key, so it never shares a colour slot with the open water.
    for culvert in [("tunnel", s("culvert")), ("layer", i(-1))] {
        let stream = make(&style, "water", Some("stream"), None, None, 14, FeatureGeometry::Line, &[culvert]);
        assert_ne!(stream.key(), 0);
        assert_ne!(stream.key(), river.key());
        assert!(color(&stream)[3] < color(&river)[3]);
    }
}

#[test]
fn global_palette_update_changes_prepared_tile_revision() {
    let original = ProtomapsBasemapTheme::default();
    let updated = original.with_layers(|layers| layers.water = [0.1, 0.2, 0.3, 1.0]);
    assert_ne!(original.cache_fingerprint(), updated.cache_fingerprint());
    assert_ne!(
        ProtomapsBasemapDefaultMapStyle::new(original).cache_fingerprint(),
        ProtomapsBasemapDefaultMapStyle::new(updated).cache_fingerprint()
    );
}

#[test]
fn every_poi_waits_for_its_zoom_and_takes_an_icon() {
    let style = default_style();
    let hospital = poi(&style, "hospital", 12, 14);
    assert_eq!(label_min_camera_zoom(&hospital), 12.0);
    let fuel = poi(&style, "fuel", 15, 14);
    assert_eq!(label_min_camera_zoom(&fuel), 17.0);
    assert!(label_rank(&hospital) < label_rank(&fuel), "the earlier POI ranks first");
    let office = poi(&style, "office", 12, 14);
    assert_ne!(office.key(), 0);
    assert_eq!(icon(&office), None);
    for (class, expected) in [
        ("bus_stop", Some(PoiSpriteIcon::Transit)),
        ("parking", Some(PoiSpriteIcon::Parking)),
        ("atm", Some(PoiSpriteIcon::Bank)),
        ("toilets", Some(PoiSpriteIcon::Toilets)),
        ("station", Some(PoiSpriteIcon::Train)),
        ("bench", None),
        ("place_of_worship", Some(PoiSpriteIcon::Worship)),
        ("religious_administration", Some(PoiSpriteIcon::Worship)),
    ] {
        let label = poi(&style, class, 16, 15);
        assert_ne!(label.key(), 0, "{class}");
        assert_eq!(icon(&label), expected, "{class}");
    }
    assert_eq!(icon(&hospital), Some(PoiSpriteIcon::Hospital));
    assert_eq!(icon(&poi(&style, "cafe", 14, 14)), Some(PoiSpriteIcon::Cafe));
    assert_eq!(ProtomapsBasemapDefaultMapStyle::poi_icon(Some("supermarket")), Some(PoiSpriteIcon::Shopping));
    assert_eq!(ProtomapsBasemapDefaultMapStyle::poi_icon(Some("fuel")), Some(PoiSpriteIcon::GasStation));
    assert_eq!(ProtomapsBasemapDefaultMapStyle::poi_icon(Some("peak")), Some(PoiSpriteIcon::Viewpoint));
    assert_eq!(ProtomapsBasemapDefaultMapStyle::poi_icon(Some("office")), None);
    assert_eq!(poi(&style, "peak", 10, 12).key(), 74);
    assert_eq!(poi(&style, "aerodrome", 8, 12).key(), 75);
    assert_eq!(icon(&poi(&style, "aerodrome", 8, 12)), Some(PoiSpriteIcon::Airport));
}

#[test]
fn poi_minimum_zoom_floors_every_poi_above_the_stated_zoom() {
    let style = ProtomapsBasemapDefaultMapStyle::new(ProtomapsBasemapTheme::default().with_label_visibility(|visibility| visibility.poi_minimum_zoom = 30));
    assert_eq!(label_min_camera_zoom(&poi(&style, "hospital", 12, 14)), 30.0);
    assert_eq!(label_min_camera_zoom(&poi(&style, "supermarket", 15, 14)), 30.0);
}

#[test]
fn label_visibility_changes_the_prepared_tile_revision() {
    let original = ProtomapsBasemapTheme::default();
    let changes: [fn(&mut map_style::style::default::LabelVisibility); 5] = [
        |visibility| visibility.poi_minimum_zoom = 30,
        |visibility| visibility.poi_requires_icon = true,
        |visibility| visibility.poi_iconless_minimum_zoom = 14,
        |visibility| visibility.country_maximum_zoom = 6,
        |visibility| visibility.region_maximum_zoom = 12,
    ];
    for change in changes {
        let updated = original.with_label_visibility(change);
        assert_ne!(original.cache_fingerprint(), updated.cache_fingerprint());
        assert_ne!(
            ProtomapsBasemapDefaultMapStyle::new(original.clone()).cache_fingerprint(),
            ProtomapsBasemapDefaultMapStyle::new(updated).cache_fingerprint()
        );
    }
    // The tracking of a label's letters, a size and an opacity are baked in.
    assert_ne!(original.cache_fingerprint(), original.with_labels(|labels| labels.country.letter_spacing_em += 0.1).cache_fingerprint());
    assert_ne!(original.cache_fingerprint(), original.with_labels(|labels| labels.region.size_points += 1.0).cache_fingerprint());
    assert_ne!(original.cache_fingerprint(), original.with_labels(|labels| labels.country.opacity = 1.0).cache_fingerprint());
}

/// A POI the icon set cannot depict draws its name alone by default, and a
/// configuration can leave such POIs out or hold them back to a zoom.
#[test]
fn an_iconless_poi_draws_unless_the_configuration_leaves_it_out() {
    let by_default = default_style();
    assert_ne!(poi(&by_default, "office", 12, 14).key(), 0);
    assert_ne!(poi(&by_default, "monument", 12, 15).key(), 0);
    let icon_only = ProtomapsBasemapDefaultMapStyle::new(ProtomapsBasemapTheme::default().with_label_visibility(|visibility| visibility.poi_requires_icon = true));
    assert_eq!(poi(&icon_only, "office", 12, 14).key(), 0);
    assert_ne!(poi(&icon_only, "museum", 12, 14).key(), 0, "A category with an icon is untouched");
    let held_back = ProtomapsBasemapDefaultMapStyle::new(ProtomapsBasemapTheme::default().with_label_visibility(|visibility| visibility.poi_iconless_minimum_zoom = 16));
    assert_eq!(label_min_camera_zoom(&poi(&held_back, "office", 12, 14)), 16.0, "it arrives at the iconless floor");
}

#[test]
fn boundary_style_suppresses_polygon_fill() {
    let style = default_style();
    assert!(suppresses_polygon_fill(&kind(&style, "boundaries", "region", 6)));
    assert!(!suppresses_polygon_fill(&polygon(&style, "water", None, 6)));
}

#[test]
fn boundary_width_is_point_locked_with_feathered_butt_dashes() {
    let style = default_style();
    for zoom in [4, 8, 12] {
        assert_eq!(line_width_points(&kind(&style, "boundaries", "region", zoom)), 1.1, "a region border at z{zoom}");
    }
    let boundary = kind(&style, "boundaries", "region", 5);
    assert!(!line_geometry(&boundary).uses_dash_pattern());
    assert!(!line_geometry(&boundary).line_cap_round);
    assert!(dash_length_points(&boundary) > 0.0);
    assert!(dash_gap_points(&boundary) > 0.0);
    let disputed = make(&style, "boundaries", Some("country"), None, None, 5, FeatureGeometry::Unknown, &[("disputed", b(true))]);
    assert_eq!(line_width_points(&disputed), 1.0);
    assert!(dash_length_points(&disputed) < dash_length_points(&boundary));
    let motorway = road(&style, "highway", Some("motorway"), 12);
    assert_eq!(line_width_points(&motorway), ProtomapsBasemapTheme::default().road_metrics.symbol_width_points.motorway);
    assert!(!passes(&motorway).is_empty());
    for pass in passes(&motorway) {
        assert!(pass.pass.line_width_points > 0.0, "Every pass of the symbol has a width in points");
    }
}

#[test]
fn every_boundary_level_draws_at_every_zoom() {
    let style = default_style();
    let region = kind(&style, "boundaries", "region", 8);
    for zoom in [0, 2, 3, 4, 10, 15] {
        assert_ne!(kind(&style, "boundaries", "region", zoom).key(), 0, "a region at z{zoom}");
        for class in ["county", "locality", "map_unit", "overlay_limit", "unrecognized_country"] {
            assert_ne!(kind(&style, "boundaries", class, zoom).key(), 0, "{class} at z{zoom}");
        }
    }
    let county = kind(&style, "boundaries", "county", 10);
    assert!(line_width_points(&county) < line_width_points(&region));
    assert!(county.key() < region.key(), "a county draws under a region");
    let country = kind(&style, "boundaries", "country", 5);
    let unrecognized = kind(&style, "boundaries", "unrecognized_country", 5);
    assert!(dash_length_points(&unrecognized) > 0.0);
    assert_eq!(dash_length_points(&country), 0.0);
    assert_ne!(unrecognized.key(), country.key());
    let disputed = make(&style, "boundaries", Some("country"), None, None, 5, FeatureGeometry::Unknown, &[("disputed", b(true))]);
    assert_eq!(dash_length_points(&disputed), dash_length_points(&unrecognized));
    assert_eq!(disputed.key(), unrecognized.key());
    let purple = ProtomapsBasemapTheme::default().layers.boundary;
    let distance_from_white = |color: [f32; 4]| ((1.0 - color[0]).powi(2) + (1.0 - color[1]).powi(2) + (1.0 - color[2]).powi(2)).sqrt();
    for zoom in [1, 2, 5, 10] {
        let country = kind(&style, "boundaries", "country", zoom);
        assert_ne!(country.key(), 0);
        assert_eq!(line_width_points(&country), 1.0);
        assert_eq!(dash_length_points(&country), 0.0, "A solid national border at z{zoom}");
        assert_eq!(dash_gap_points(&country), 0.0);
        let region = kind(&style, "boundaries", "region", zoom);
        assert_eq!(dash_length_points(&region), 7.0, "A dashed regional border at z{zoom}");
        assert!(color(&country)[3] > color(&region)[3], "The national border is more opaque at z{zoom}");
        assert!(distance_from_white(color(&country)) > distance_from_white(color(&region)));
        assert!(distance_from_white(color(&country)) <= distance_from_white(purple) + 0.001);
    }
}

#[test]
fn buildings_draw_on_every_tile_and_an_address_point_is_a_house_number() {
    let style = default_style();
    for zoom in [12, 13, 15] {
        let building = make(&style, "buildings", Some("building"), None, None, zoom, FeatureGeometry::Polygon, &[]);
        let extrusion = building.extrusion_style().unwrap_or_else(|| panic!("z{zoom}"));
        assert!(extrusion.fallback_height > 0.0, "a building the tile states no height for still rises");
    }
    let number = make(&style, "buildings", Some("address"), None, None, 15, FeatureGeometry::Point, &[("addr_housenumber", s("12A"))]);
    assert!(number.point_label_style().is_some());
    assert_eq!(icon(&number), None);
    let cafe = poi(&style, "cafe", 18, 15);
    assert!(label_collision_rank(&number) > label_collision_rank(&cafe), "a house number yields to every POI");
    assert_eq!(make(&style, "buildings", Some("address"), None, None, 15, FeatureGeometry::Point, &[]).key(), 0);
}
