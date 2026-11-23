// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use crate::style::default::{BiomeStyles, LabelAppearance, LabelStyles, ProtomapsBasemapTheme, RoadLayerStyles};
use crate::style::{RouteShieldAppearance, TreePalette};
use crate::{Float3, Float4};

/// The day's tone sunk into the night: a fifth of its light with a blue
/// cast, so a forest stays a greener dark than a field.
fn sunk(color: Float4) -> Float4 {
    [color[0] * 0.22 + 0.03, color[1] * 0.22 + 0.04, color[2] * 0.22 + 0.07, color[3]]
}

/// A plate in the night's light: half its day's light with a blue cast, so
/// a white plate is a grey one and a red plate a dark red, its hue kept.
fn sunk_plate(color: Float3) -> Float3 {
    [color[0] * 0.5 + 0.02, color[1] * 0.5 + 0.03, color[2] * 0.5 + 0.05]
}

/// A route sign in the night's light: the plate, its edge and its band
/// sunk (`sunk_plate`), the edge a step further so it does not ring the
/// plate, and a light number dimmed to a grey, a dark one as it is.
fn sunk_sign(mut sign: RouteShieldAppearance) -> RouteShieldAppearance {
    fn number(color: Float3) -> Float3 {
        if color[0] + color[1] + color[2] > 1.5 { color.map(|channel| channel * 0.74) } else { color }
    }
    sign.fill_color = sunk_plate(sign.fill_color);
    sign.border_color = sunk_plate(sign.border_color).map(|channel| channel * 0.85);
    sign.header_color = sign.header_color.map(sunk_plate);
    sign.text_color = number(sign.text_color);
    sign
}

impl ProtomapsBasemapTheme {
    /// The night palette: the same map in the dark, the land a deep
    /// blue-grey, the water a deep navy, the greens and the fields of the
    /// day's palette and the biomes of the globe sunk into the night at a
    /// fifth of their light with a blue cast, the roads a warm grey over the
    /// dark land and the overview's highways an amber net, the buildings a
    /// shade above the land, and every label a light grey on a black halo,
    /// graded as the day's greys are, never white: a white name on a dark
    /// map reads as a lamp. The slopes shade by the moon: a deeper shadow
    /// and a cool, faint highlight. Every plate keeps its colour, sunk to
    /// the night's light: the route signs their countries' colours and the
    /// POI icons their categories', since a white or a bright plate is a
    /// lamp on a dark map. The road metrics and the label visibility are the
    /// day's.
    ///
    /// The globe is the day's: the night is the map's, not the planet's, so
    /// the ground of the globe's view wears the day's palette
    /// (`globe_layers`) and its names the day's lettering (`globe_labels`)
    /// under the globe's own sun, and the night comes in with the plane's
    /// air at the end of the unroll, with the sky, the fog and the asphalt
    /// of the roads.
    ///
    /// The map's air, the sky, the horizon and the ground fog, is a setting
    /// rather than a theme, so the map of a night sets both.
    pub fn night() -> ProtomapsBasemapTheme {
        let day = ProtomapsBasemapTheme::default();
        day.with_layers(|layers| {
            layers.land = [0.09, 0.10, 0.13, 1.0];
            layers.water = [0.04, 0.09, 0.20, 1.0];
            layers.wood = [0.06, 0.14, 0.11, 1.0];
            layers.grass = [0.08, 0.16, 0.12, 1.0];
            layers.farmland = [0.10, 0.14, 0.10, 1.0];
            layers.wetland = [0.07, 0.14, 0.13, 1.0];
            layers.sand = [0.16, 0.15, 0.13, 1.0];
            layers.ice = [0.30, 0.32, 0.36, 1.0];
            layers.park = [0.08, 0.17, 0.13, 1.0];
            layers.residential = [0.12, 0.12, 0.15, 1.0];
            layers.industrial = [0.14, 0.13, 0.15, 1.0];
            layers.aeroway = [0.16, 0.16, 0.19, 1.0];
            layers.boundary = [0.58, 0.36, 0.78, 0.9];
            let base = [0.42, 0.40, 0.36, 1.0];
            let minor = [0.24, 0.24, 0.28, 1.0];
            layers.roads = RoadLayerStyles {
                motorway: base,
                trunk: base,
                primary: base,
                secondary: minor,
                tertiary: minor,
                minor,
                service: minor,
                path: minor,
                rail: minor,
                casing: [0.06, 0.06, 0.08, 0.95],
                marking: [0.55, 0.53, 0.48, 1.0],
            };
            // The globe's overview in the dark: every biome and cover tone
            // sunk from the day's, so the continents keep their climates in
            // the night, and the highways an amber net.
            let day = BiomeStyles::default();
            layers.biomes = BiomeStyles {
                tropical_moist_forest: sunk(day.tropical_moist_forest),
                tropical_dry_forest: sunk(day.tropical_dry_forest),
                tropical_conifer_forest: sunk(day.tropical_conifer_forest),
                temperate_broadleaf_forest: sunk(day.temperate_broadleaf_forest),
                temperate_conifer_forest: sunk(day.temperate_conifer_forest),
                boreal_forest: sunk(day.boreal_forest),
                tropical_grassland: sunk(day.tropical_grassland),
                temperate_grassland: sunk(day.temperate_grassland),
                flooded_grassland: sunk(day.flooded_grassland),
                montane_grassland: sunk(day.montane_grassland),
                tundra: sunk(day.tundra),
                mediterranean_scrub: sunk(day.mediterranean_scrub),
                desert: sunk(day.desert),
                mangrove: sunk(day.mangrove),
                forest_cover: sunk(day.forest_cover),
                farmland_cover: sunk(day.farmland_cover),
                barren_cover: sunk(day.barren_cover),
                maximum_tile_zoom: day.maximum_tile_zoom,
                cover_minimum_tile_zoom: day.cover_minimum_tile_zoom,
                cover_fade_in_zooms: BiomeStyles::default().cover_fade_in_zooms,
            };
            layers.overview_roads.major = [0.72, 0.58, 0.26, 1.0];
            layers.overview_roads.minor = [0.42, 0.39, 0.30, 1.0];
            layers.trees.palette = TreePalette::NIGHT;
        })
        .apply(|theme| {
            let day = ProtomapsBasemapTheme::default();
            theme.globe_layers = Some(day.layers);
            theme.globe_labels = Some(day.labels);
        })
        .with_route_shields(|shields| {
            for rule in &mut shields.rules {
                rule.appearance = rule.appearance.map(sunk_sign);
            }
            shields.fallback = shields.fallback.map(sunk_sign);
        })
        .with_features(|features| {
            features.building_fill_color = [0.18, 0.19, 0.23, 1.0];
        })
        .with_labels(|labels| {
            // The day's greys mirrored into the dark, graded as the day's
            // are: the cities the lightest, the villages and the lettering
            // of the lands a step dimmer, none of them white. A white name
            // on a dark map reads as a lamp, not as ink.
            let stroke = [0.02, 0.03, 0.06];
            type Slot = fn(&mut LabelStyles) -> &mut LabelAppearance;
            let fills: [(Slot, Float3); 8] = [
                (|labels| &mut labels.city, [0.84, 0.86, 0.92]),
                (|labels| &mut labels.town, [0.74, 0.77, 0.84]),
                (|labels| &mut labels.district, [0.60, 0.63, 0.71]),
                (|labels| &mut labels.country, [0.66, 0.68, 0.78]),
                (|labels| &mut labels.region, [0.60, 0.62, 0.72]),
                (|labels| &mut labels.poi, [0.62, 0.65, 0.73]),
                (|labels| &mut labels.road, [0.60, 0.62, 0.68]),
                (|labels| &mut labels.water, [0.42, 0.58, 0.84]),
            ];
            for (slot, fill) in fills {
                let appearance = slot(labels);
                appearance.fill_color = fill;
                appearance.stroke_color = stroke;
            }
            // The POI plates in their categories' colours, sunk.
            labels.poi_categories = labels.poi_categories.map(sunk_plate);
        })
        .apply(|theme| {
            theme.hillshade.shadow_color = [0.0, 0.01, 0.04, 0.45];
            theme.hillshade.highlight_color = [0.55, 0.65, 0.95, 0.12];
        })
    }
}
