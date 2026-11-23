// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Ported from `ProtomapsBasemapThemeTests`, `NightThemeTests`,
//! `BuildingExtrusionToggleTests`, `GlobeCapPaletteTests` and
//! `ImmersiveMapSettingsDefaultsTests` (the parts on the theme), with the
//! engine's own fingerprints as golden values and the theme's JSON form.

use map_style::style::default::{ProtomapsBasemapDefaultMapStyle, ProtomapsBasemapMapStyle, ProtomapsBasemapTheme};
use map_style::style::{AnyMapStyle, LabelFontWeight, MapStyle, VectorTileStyle};

/// The fingerprints the engine computes for the same themes (read off the
/// Swift build): a port that hashes one value differently, or in another
/// order, keys the disk cache apart from the engine's.
#[test]
fn the_fingerprints_are_the_engines() {
    let day = ProtomapsBasemapTheme::default();
    assert_eq!(day.cache_fingerprint(), 2055041381);
    assert_eq!(ProtomapsBasemapDefaultMapStyle::new(day.clone()).cache_fingerprint(), 2055041408);
    assert_eq!(ProtomapsBasemapTheme::night().cache_fingerprint(), 344023019);
    assert_eq!(day.with_road_metrics(|metrics| metrics.draws_casing = true).cache_fingerprint(), 2296371049);
    assert_eq!(day.with_layers(|layers| layers.biomes.maximum_tile_zoom = -1).cache_fingerprint(), 3097153701);
    let globe = day
        .with_globe_layers(|layers| layers.water = [1.0, 0.0, 1.0, 1.0])
        .with_globe_labels(|labels| labels.country.fill_color = [1.0, 0.0, 1.0]);
    assert_eq!(globe.cache_fingerprint(), 3681475292);
}

/// A recolor must change disk-cache identity, otherwise the map keeps
/// drawing from prepared tiles baked with the old palette.
#[test]
fn cache_fingerprint_changes_with_every_palette_group() {
    let base = ProtomapsBasemapTheme::default();
    let fingerprint = base.cache_fingerprint();
    assert_ne!(fingerprint, base.with_labels(|labels| labels.town.halo_em += 0.05).cache_fingerprint());
    assert_ne!(fingerprint, base.with_layers(|layers| layers.water = [0.12, 0.34, 0.56, 1.0]).cache_fingerprint());
    assert_ne!(
        fingerprint,
        base.with_features(|features| features.building_fill_color = [0.18, 0.19, 0.23, 1.0]).cache_fingerprint()
    );
    assert_ne!(fingerprint, base.with_labels(|labels| labels.district.fill_color = [0.6, 0.6, 0.6]).cache_fingerprint());
    // The biomes of the overview are baked into the overview zooms' tiles,
    // and the zoom they stop at decides which tiles carry them.
    assert_ne!(fingerprint, base.with_layers(|layers| layers.biomes.desert = [0.9, 0.8, 0.6, 1.0]).cache_fingerprint());
    assert_ne!(fingerprint, base.with_layers(|layers| layers.biomes.maximum_tile_zoom = 5).cache_fingerprint());
    assert_ne!(fingerprint, base.with_layers(|layers| layers.biomes.forest_cover = [0.5, 0.7, 0.4, 1.0]).cache_fingerprint());
    assert_ne!(fingerprint, base.with_layers(|layers| layers.biomes.cover_minimum_tile_zoom = 5).cache_fingerprint());
}

/// `apply` is a copy with the changes: the original is untouched, and the
/// style built from it carries them.
#[test]
fn apply_makes_a_changed_copy() {
    let base = ProtomapsBasemapTheme::default();
    let water = [0.2, 0.4, 0.8, 1.0];
    let recoloured = base.apply(|theme| theme.layers.water = water);
    assert_eq!(base, ProtomapsBasemapTheme::default());
    assert_eq!(recoloured.layers.water, water);
    assert_ne!(base.cache_fingerprint(), recoloured.cache_fingerprint());
    let style = ProtomapsBasemapMapStyle::default().apply(|theme| theme.layers.water = water);
    assert_eq!(style.theme, recoloured);
    assert_eq!(
        AnyMapStyle::new(&style).configuration_fingerprint(),
        recoloured.cache_fingerprint() as u64
    );
}

/// `-0.0 == 0.0` is true, so two themes that compare equal would otherwise
/// hash differently and thrash the prepared-tile disk cache.
#[test]
fn cache_fingerprint_canonicalizes_signed_zero_float_values() {
    let positive = ProtomapsBasemapTheme::default().with_labels(|labels| labels.town.halo_em = 0.0);
    let negative = ProtomapsBasemapTheme::default().with_labels(|labels| labels.town.halo_em = -0.0);
    assert_eq!(positive, negative);
    assert_eq!(positive.cache_fingerprint(), negative.cache_fingerprint());
}

/// The raw values are written into the prepared-tile disk format and
/// folded into the style fingerprint, so swapping them would render every
/// cached label at the wrong weight.
#[test]
fn label_font_weight_raw_values_are_stable() {
    assert_eq!(LabelFontWeight::Bold.raw_value(), 0);
    assert_eq!(LabelFontWeight::Thin.raw_value(), 1);
}

/// The night is its own palette, baked into its own tiles: a dark land and
/// water, light grey labels, the globe's biomes sunk into the dark, with
/// the day's metrics and visibility.
#[test]
fn the_night_palette_is_dark_and_its_own_cache_identity() {
    let day = ProtomapsBasemapTheme::default();
    let night = ProtomapsBasemapTheme::night();
    assert_ne!(night.cache_fingerprint(), day.cache_fingerprint());
    assert!(night.layers.land[0] + night.layers.land[1] + night.layers.land[2] < 0.5);
    assert!(night.layers.water[1] < day.layers.water[1]);
    assert!(night.labels.city.fill_color[0] > 0.75, "Labels are light on the dark");
    assert!(night.labels.city.fill_color[0] < 0.9, "but never white, a lamp on a dark map");
    assert!(night.labels.city.fill_color[0] > night.labels.town.fill_color[0], "Graded as the day's greys");
    assert!(night.labels.town.fill_color[0] > night.labels.district.fill_color[0]);
    assert!(night.labels.city.stroke_color[0] < 0.1);
    assert!(
        night.layers.biomes.temperate_broadleaf_forest[1] < day.layers.biomes.temperate_broadleaf_forest[1] * 0.5,
        "The biomes sink into the night"
    );
    let forest = night.layers.biomes.temperate_broadleaf_forest;
    let desert = night.layers.biomes.desert;
    assert!(forest[1] - forest[0] > desert[1] - desert[0], "A forest stays a greener dark than a desert");
    assert_eq!(night.road_metrics, day.road_metrics, "The night changes colours, not the road metrics");
    let plate = night.route_shields.fallback.unwrap();
    assert!(plate.fill_color[0] < 0.6, "A white plate is a grey one in the night");
    assert!(plate.fill_color[0] > 0.4);
    assert!(plate.border_color[0] < plate.fill_color[0], "The edge does not ring the plate");
    let food = night.labels.poi_categories.food;
    assert!(food[0] < day.labels.poi_categories.food[0] * 0.6, "The POI plates are sunk too");
    assert!(food[0] > food[2] * 2.0, "and keep their hue");
    assert_eq!(plate.text_color, day.route_shields.fallback.unwrap().text_color, "A dark number stays");
    let russia = night.route_shields.rules.iter().find(|rule| rule.network == "ru:national").unwrap().appearance.unwrap();
    let day_russia = day.route_shields.rules.iter().find(|rule| rule.network == "ru:national").unwrap().appearance.unwrap();
    assert!(russia.fill_color[0] < day_russia.fill_color[0], "The red plate is a dark red");
    assert!(russia.fill_color[0] > russia.fill_color[1] * 3.0, "and still red");
    assert!(russia.text_color[0] < 1.0, "The white number a shade under white");
    let networks = |theme: &ProtomapsBasemapTheme| theme.route_shields.rules.iter().map(|rule| rule.network.clone()).collect::<Vec<_>>();
    assert_eq!(networks(&night), networks(&day));
    assert_eq!(night.route_shields.minimum_tile_zoom, day.route_shields.minimum_tile_zoom);
    assert_eq!(night.label_visibility, day.label_visibility);
    assert_eq!(night.base_colors().map, night.layers.land, "What no tile paints follows the night's land");
}

/// The night is the map's, not the planet's: the globe wears the day's
/// palette, its caps included, and the palette is part of the tiles'
/// identity.
#[test]
fn the_globe_of_the_night_is_the_days() {
    let day = ProtomapsBasemapTheme::default();
    let night = ProtomapsBasemapTheme::night();
    assert_eq!(night.globe_layers, Some(day.layers));
    assert_eq!(night.globe_labels, Some(day.labels), "The globe's names are the day's lettering");
    assert_eq!(day.globe_layers, None, "The day's globe is the day's own palette");
    assert_eq!(day.globe_labels, None);
    assert_ne!(night.apply(|theme| theme.globe_labels = None).cache_fingerprint(), night.cache_fingerprint());
    let own_lettering = day.with_globe_labels(|labels| labels.country.fill_color = [1.0, 0.0, 1.0]);
    assert_eq!(own_lettering.globe_labels.unwrap().country.fill_color, [1.0, 0.0, 1.0]);
    assert_eq!(own_lettering.globe_labels.unwrap().city, day.labels.city, "The builder starts from the plane's labels");
    assert_eq!(night.base_colors().north_cap, day.layers.water, "The caps are the globe's");
    assert_eq!(night.base_colors().south_cap, day.layers.ice);
    assert_ne!(night.apply(|theme| theme.globe_layers = None).cache_fingerprint(), night.cache_fingerprint());
    let own_globe = day.with_globe_layers(|layers| layers.water = [1.0, 0.0, 1.0, 1.0]);
    assert_eq!(own_globe.globe_layers.unwrap().water, [1.0, 0.0, 1.0, 1.0]);
    assert_eq!(own_globe.globe_layers.unwrap().land, day.layers.land, "The builder starts from the plane's palette");
}

/// The theme's extrusion switch is part of the theme, so it is
/// prepared-cache identity, and it is on by default.
#[test]
fn the_extrusion_switch_is_part_of_the_theme_fingerprint() {
    let with = ProtomapsBasemapTheme::default();
    let without = with.with_features(|features| features.building_extrusion = false);
    assert_ne!(with.cache_fingerprint(), without.cache_fingerprint());
    assert!(with.features.building_extrusion, "Buildings rise unless asked not to");
}

/// The built-in style's base colours and polar caps are its theme's land,
/// water and ice: the same constants the tiles paint the ocean and
/// Antarctica with, so the caps continue the tiles seamlessly.
#[test]
fn the_base_colours_are_the_themes_land_water_and_ice() {
    let theme = ProtomapsBasemapTheme::default();
    let base = ProtomapsBasemapDefaultMapStyle::new(theme.clone()).base_colors();
    assert_eq!(base.map, theme.layers.land);
    assert_eq!(base.north_cap, theme.layers.water);
    assert_eq!(base.south_cap, theme.layers.ice);
}

/// The built-in map style knows its theme, and the night spells as its own
/// map style.
#[test]
fn the_map_style_hands_back_its_theme() {
    let night = AnyMapStyle::new(&ProtomapsBasemapMapStyle::night());
    assert_eq!(night.basemap_theme(), Some(&ProtomapsBasemapTheme::night()));
    assert_ne!(night, AnyMapStyle::new(&ProtomapsBasemapMapStyle::default()));
    assert_eq!(ProtomapsBasemapMapStyle::night().schema().cache_fingerprint(), 4);
}

/// The theme round-trips through JSON under the engine's property names,
/// and a partial object fills the rest from the defaults.
#[test]
fn the_theme_reads_and_writes_json() {
    let night = ProtomapsBasemapTheme::night();
    let json = serde_json::to_value(&night).unwrap();
    assert!(json.get("labelVisibility").is_some());
    assert!(json["roadMetrics"].get("symbolWidthPoints").is_some());
    assert!(json["layers"]["overviewRoads"].get("majorWidthPoints").is_some());
    assert_eq!(json["labels"]["city"]["weight"], "bold");
    assert_eq!(json["routeShields"]["rules"][18]["appearance"]["shape"], "escutcheon");
    let back: ProtomapsBasemapTheme = serde_json::from_value(json).unwrap();
    assert_eq!(back, night);
    assert_eq!(back.cache_fingerprint(), night.cache_fingerprint());

    let partial: ProtomapsBasemapTheme = serde_json::from_str(
        r#"{"layers": {"water": [0.2, 0.4, 0.8, 1]}, "labelOpacity": {"pois": {"14": 0.6, "16": 1}}}"#,
    )
    .unwrap();
    let expected = ProtomapsBasemapTheme::default().apply(|theme| theme.layers.water = [0.2, 0.4, 0.8, 1.0]);
    assert_eq!(partial.layers, expected.layers);
    assert_eq!(partial.labels, expected.labels);
    assert_eq!(partial.label_opacity.pois.value(15.0), 0.8);
    assert_eq!(partial.label_opacity.names.value(15.0), 1.0);
}
