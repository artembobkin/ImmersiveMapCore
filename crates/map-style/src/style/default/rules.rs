// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use mvt::Value;

use crate::schema::default::trim_whitespaces;
use crate::schema::{FeatureGeometry, FeatureProperties, RoadFacts, RoadStructure};
use crate::style::default::{LayerStyles, ProtomapsBasemapTheme};
use crate::style::{
    BaseColors, FeatureStyle, FeatureStyleContext, FillStyle, Hillshade, LabelOpacity, LineGeometryStyle, LinePass,
    LineStyle, RoadLevel, RoadTier, TerrainOcclusion, TreePalette, VectorTileStyle, WaterNameKind, ZoomFade,
};
use crate::{Float3, Float4};

/// The built-in style, for the Protomaps basemap schema (tiles version 4):
/// the layers `earth`, `landcover`, `landuse`, `water`, `buildings`,
/// `roads`, `boundaries`, `places` and `pois`, each feature told apart by
/// `kind` and `kind_detail`, over the reading `ProtomapsBasemapSchema`
/// makes of it, and the layers `biome`, `lake` and `road` the engine's
/// overview of the globe lays into the tiles of the overview zooms.
///
/// This file is the dispatch: the layer match in `make_style`, the road
/// policy every road feature carries, and the builders the rules share. The
/// rules themselves are split by layer family into the modules next to it:
/// `ground`, `buildings`, `roads` and `labels`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProtomapsBasemapDefaultMapStyle {
    pub theme: ProtomapsBasemapTheme,
}

impl ProtomapsBasemapDefaultMapStyle {
    pub const IMPLEMENTATION_REVISION: u32 = 27;

    /// The style draws what the tile carries and adds no zoom gate of its
    /// own: the basemap already thins every layer per tile level, so a
    /// feature in a tile is a feature on the map. A road shows from the
    /// first tile that ships it, without a fade.
    pub const ROAD_ZOOM_FADE: ZoomFade = ZoomFade::NONE;

    /// The camera zoom the roads switch from the globe's look, the highway
    /// yellow under its veil, to their own colour and alpha, at once
    /// (`LinePass::sphere_look_until_zoom`): where the globe has finished
    /// its unroll and the land cover is fading out (`landcover_zoom_fade`).
    pub const ROAD_LOOK_SWITCH_ZOOM: f32 = 7.0;

    /// From this class priority up a road is part of the automobile network
    /// and draws in the tier above the pedestrian one: service roads sit at
    /// 45 and paths at 35 with rail between.
    pub const AUTOMOBILE_TIER_PRIORITY: i32 = 45;

    pub fn new(theme: ProtomapsBasemapTheme) -> Self {
        ProtomapsBasemapDefaultMapStyle { theme }
    }

    /// The basemap ships the continuous `landcover` through tile z7 only,
    /// and the engine draws the z7 tiles for camera zoom 7.0 up to 8.0. The
    /// land cover and the biomes fade out over camera zoom 6.7 to 7.1,
    /// around the globe's last tenths of the unroll and the roads' switch to
    /// their own look (`ROAD_LOOK_SWITCH_ZOOM`), so the country view arrives
    /// as one change. Up to zoom 7 the z6 tiles serve the view, and they
    /// carry a handful of the land use's woods and fields, so the land the
    /// cover leaves there is plainer until the z7 tiles bring them. The fade
    /// hides nothing a tile would otherwise show past it: from camera zoom 8
    /// no tile has the cover.
    pub fn landcover_zoom_fade() -> ZoomFade {
        ZoomFade::fade_out(6.7, 7.1)
    }

    /// The road policy every road of this style carries, stated once for
    /// the branches that produce roads: where it draws and which tier it
    /// belongs to. A style that is no road (a hidden class, a ferry's plain
    /// line) passes through.
    pub fn road_policy(&self, facts: &RoadFacts<'_>, style: FeatureStyle) -> FeatureStyle {
        let FeatureStyle::Road(mut road) = style else {
            return style;
        };
        road.level = Self::road_level(facts);
        road.tier = if road.class_priority >= Self::AUTOMOBILE_TIER_PRIORITY {
            RoadTier::Automobile
        } else {
            RoadTier::Pedestrian
        };
        FeatureStyle::Road(road)
    }

    /// Where a road draws: its tagged structure or, for a road on the
    /// ground, its `layer`, so a street diving under a bridge draws with
    /// the tunnels and a ramp climbing over one with the bridges.
    pub fn road_level(facts: &RoadFacts<'_>) -> RoadLevel {
        match facts.structure {
            RoadStructure::Tunnel => RoadLevel::Tunnel,
            RoadStructure::Bridge => RoadLevel::Bridge,
            RoadStructure::Ground if facts.layer < 0 => RoadLevel::Tunnel,
            RoadStructure::Ground if facts.layer > 0 => RoadLevel::Bridge,
            RoadStructure::Ground => RoadLevel::Ground,
        }
    }

    /// The rules applied to one feature.
    pub fn style_feature(&self, data: &FeatureStyleContext<'_>) -> FeatureStyle {
        let layer = data.layer_name.to_lowercase();
        let props = &data.properties;
        let z = data.tile.z;
        let kind_owned = props.str("kind").map(str::to_lowercase);
        let kind = kind_owned.as_deref();
        let kind_detail_owned = props.str("kind_detail").map(str::to_lowercase);
        let kind_detail = kind_detail_owned.as_deref();

        match layer.as_str() {
            "earth" => self.painted_style(2, |layers| layers.land, ZoomFade::NONE),
            "landcover" => self.landcover_style(kind, z),
            "landuse" => self.landuse_style(kind, z),
            "biome" => self.biome_style(kind, z),
            "lake" => self.overview_lake_style(z),
            "road" => self.overview_road_network_style(parse_int_value(props.get("rank")), z),
            "water" => match data.geometry {
                FeatureGeometry::Point => self.water_label_style(kind, props, z),
                FeatureGeometry::Line => self.waterway_style(kind, kind_detail, props),
                // No tree stands in a pond or a river.
                FeatureGeometry::Polygon | FeatureGeometry::Unknown => FeatureStyle::Fill(FillStyle {
                    excludes_trees: true,
                    ..self.painted(Self::WATER_KEY, |layers| layers.water)
                }),
            },
            "buildings" => {
                if data.geometry == FeatureGeometry::Point {
                    return self.address_label_style(data.facts);
                }
                self.building_style(data.facts)
            }
            "roads" => {
                let ground = RoadFacts::ground();
                let road = data.facts.road().unwrap_or(&ground);
                self.road_policy(road, self.roads_style(kind, kind_detail, props, road, z))
            }
            "boundaries" => self.boundary_style(kind, props, z),
            "places" => self.place_label_style(kind, kind_detail, props),
            "pois" => {
                // A mapped tree is no POI to name: it stands as a tree.
                if kind == Some("tree") {
                    return self.theme.layers.trees.mapped.map(FeatureStyle::Tree).unwrap_or(FeatureStyle::Hidden);
                }
                self.poi_label_style(kind, props)
            }
            _ => FeatureStyle::Hidden,
        }
    }

    /// A fill whose colour is one pick from the palette: the plane's from
    /// `layers`, the globe's from `globe_layers` where the theme has one, so
    /// one rule colours both surfaces.
    pub fn painted(&self, key: u8, pick: impl Fn(&LayerStyles) -> Float4) -> FillStyle {
        FillStyle { sphere_color: self.globe_color(&pick), ..FillStyle::new(key, pick(&self.theme.layers)) }
    }

    /// A painted fill (`painted`) that fades with the camera zoom.
    pub fn painted_style(&self, key: u8, pick: impl Fn(&LayerStyles) -> Float4, zoom_fade: ZoomFade) -> FeatureStyle {
        FeatureStyle::Fill(FillStyle { zoom_fade, ..self.painted(key, pick) })
    }

    /// The colour a pick from the palette wears on the globe, `None` where
    /// the theme has no globe palette (the pick's own colour then).
    pub fn globe_color(&self, pick: impl Fn(&LayerStyles) -> Float4) -> Option<Float3> {
        self.theme.globe_layers.as_ref().map(|globe| {
            let color = pick(globe);
            [color[0], color[1], color[2]]
        })
    }

    /// A world-locked line of round caps and joins, its width in tile
    /// units.
    pub fn round_line(&self, key: u8, color: Float4, width: f64, minimum_width_points: f32, sphere_color: Option<Float3>) -> FeatureStyle {
        FeatureStyle::Line(LineStyle::new(LinePass {
            minimum_width_points,
            sphere_color,
            ..LinePass::new(key, color, LineGeometryStyle::rounded(width, true, true))
        }))
    }
}

impl VectorTileStyle for ProtomapsBasemapDefaultMapStyle {
    /// The palette's fingerprint and the rules' revision: a change to either
    /// re-prepares every tile.
    fn cache_fingerprint(&self) -> u32 {
        self.theme.cache_fingerprint().wrapping_add(Self::IMPLEMENTATION_REVISION)
    }

    /// The land, the water and the ice of the theme, where no tile paints.
    fn base_colors(&self) -> BaseColors {
        self.theme.base_colors()
    }

    /// The theme's shading of the slopes.
    fn hillshade(&self) -> Hillshade {
        self.theme.hillshade.clone()
    }

    /// The theme's darkening where the terrain hides the sky.
    fn terrain_occlusion(&self) -> TerrainOcclusion {
        self.theme.terrain_occlusion
    }

    /// The theme's colours of the trees.
    fn tree_palette(&self) -> TreePalette {
        self.theme.tree_palette()
    }

    fn label_opacity(&self) -> LabelOpacity {
        self.theme.label_opacity.clone()
    }

    fn style_id(&self) -> &str {
        "protomaps"
    }

    fn make_style(&self, feature: &FeatureStyleContext<'_>) -> FeatureStyle {
        self.style_feature(feature)
    }

    /// The full-tile base quad the engine emits per tile, in the land
    /// colour. The basemap ships the land as the `earth` polygon, so the
    /// quad only guards a tile with no earth feature (open ocean, where it
    /// lies under the water fill) and the holes an ocean's islands cut.
    fn background_style(&self, _tile_zoom: i32) -> FeatureStyle {
        self.painted_style(1, |layers| layers.land, ZoomFade::NONE)
    }

    /// The basemap ships the ocean names as points of `water` from z0, so
    /// the parser adds none of its own.
    fn water_name_style(&self, _kind: WaterNameKind, _tile_zoom: i32) -> Option<FeatureStyle> {
        None
    }

    fn basemap_theme(&self) -> Option<&ProtomapsBasemapTheme> {
        Some(&self.theme)
    }
}

/// An integer as the basemap spells it: a number truncated toward zero, or
/// a string of digits with spaces around it.
pub(crate) fn parse_int_value(value: Option<Value<'_>>) -> Option<i64> {
    match value? {
        Value::String(text) => trim_whitespaces(text).parse().ok(),
        other => crate::schema::properties_integer(other),
    }
}

/// A flag as the basemap spells it: a bool, a number or a word.
pub(crate) fn parse_bool_value(value: Option<Value<'_>>) -> bool {
    match value {
        Some(Value::Bool(flag)) => flag,
        Some(Value::Int(number)) => number != 0,
        Some(Value::UInt(number)) => number != 0,
        Some(Value::String(text)) => {
            let normalized = text.to_lowercase();
            normalized == "true" || normalized == "yes" || normalized == "1"
        }
        Some(Value::Double(_) | Value::Float(_) | Value::Null) | None => false,
    }
}

/// The value under a key, lowercased, when it is a string.
pub(crate) fn lowercased(props: &FeatureProperties<'_, '_>, key: &str) -> Option<String> {
    props.str(key).map(str::to_lowercase)
}
