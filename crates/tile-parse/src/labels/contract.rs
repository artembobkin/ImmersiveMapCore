// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The labels a tile carries out of the parse (the Swift `ParsedTextLabel`
//! and `ParsedRoadTextLabel`).

use map_style::style::{LabelPlacement, LabelTextStyle, PoiSpriteIcon, RouteShieldStyle};
use serde::{Serialize, Serializer};

use super::parsed_label_key;

/// A point label as the parser read it from a feature: its text, its
/// anchor in tile units, the identity that keeps it stable across parses,
/// the priorities and style the label policy decided, and whether it stands
/// on the screen or lies on the map. A road's route signs are one of these
/// too: `route_shields` holds the signs, each number drawn on its plate.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedTextLabel {
    pub text: String,
    /// The anchor in tile units (tile space, y south).
    pub position: [i16; 2],
    #[serde(serialize_with = "u64_as_string")]
    pub key: u64,
    pub sort_key: i32,
    pub collision_priority: i32,
    pub text_style: LabelTextStyle,
    pub poi_icon: Option<PoiSpriteIcon>,
    /// Minimum camera zoom at which the label is visible (0 = always).
    pub min_camera_zoom: f32,
    /// The camera zoom the label goes away at, infinity for never
    /// (`PointLabelStyle::max_camera_zoom`). Infinity serializes as null.
    pub max_camera_zoom: f32,
    pub placement: LabelPlacement,
    /// The route signs the label draws instead of its text, `None` for a
    /// plain label.
    pub route_shields: Option<RouteShieldStyle>,
    /// Whether the label belongs to the building it stands in
    /// (`PointLabelStyle::stands_on_roof`): it takes the roof over its
    /// anchor, and rises to it when it names the building itself.
    pub stands_on_roof: bool,
    /// The tile feature's id, `None` for a feature without one. A building
    /// and a point made from the same OSM element share it, which is how a
    /// label is known to name the building it stands in.
    #[serde(serialize_with = "optional_u64_as_string")]
    pub feature_id: Option<u64>,
    /// Local detail, shown only near the camera (`PointLabelStyle::is_local`).
    pub is_local: bool,
    /// The roof the label draws on in tile units, the extrusion mesh's own
    /// height scale, set once the tile's buildings are resolved when it
    /// names the building (`lifts_to_roof`). Zero for every other label,
    /// which draws on the ground.
    pub roof_height: f32,
    /// Whether the label draws on the roof over it: it names the building
    /// itself (the building's own OSM element), not something inside it.
    pub lifts_to_roof: bool,
}

impl ParsedTextLabel {
    /// A label the parser keys itself from its tile, layer, text and anchor
    /// or feature id (the Swift initializer taking `tile` and `layerName`).
    /// It stands on no roof, is not local and draws no route signs.
    #[allow(clippy::too_many_arguments)]
    pub fn keyed(
        text: String,
        position: [i16; 2],
        feature_id: u64,
        has_feature_id: bool,
        layer_name: &str,
        sort_key: i32,
        collision_priority: i32,
        text_style: LabelTextStyle,
        poi_icon: Option<PoiSpriteIcon>,
        min_camera_zoom: f32,
        max_camera_zoom: f32,
        placement: LabelPlacement,
    ) -> ParsedTextLabel {
        let key = parsed_label_key::make_point_label_key(&text, position, feature_id, has_feature_id, layer_name);
        ParsedTextLabel {
            text,
            position,
            key,
            sort_key,
            collision_priority,
            text_style,
            poi_icon,
            min_camera_zoom,
            max_camera_zoom,
            placement,
            route_shields: None,
            stands_on_roof: false,
            feature_id: has_feature_id.then_some(feature_id),
            is_local: false,
            roof_height: 0.0,
            lifts_to_roof: false,
        }
    }

    /// A label under a key the label policy minted, with every optional
    /// field at the Swift initializer's default: no icon, every zoom, on the
    /// screen, no signs, not on a roof, no feature id, not local.
    pub fn new(text: String, position: [i16; 2], key: u64, sort_key: i32, collision_priority: i32, text_style: LabelTextStyle) -> ParsedTextLabel {
        ParsedTextLabel {
            text,
            position,
            key,
            sort_key,
            collision_priority,
            text_style,
            poi_icon: None,
            min_camera_zoom: 0.0,
            max_camera_zoom: f32::INFINITY,
            placement: LabelPlacement::Screen,
            route_shields: None,
            stands_on_roof: false,
            feature_id: None,
            is_local: false,
            roof_height: 0.0,
            lifts_to_roof: false,
        }
    }
}

/// A road name as the parser read it: the text and the path in tile units
/// it is laid along, with the identity that keeps it stable across parses.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedRoadTextLabel {
    pub text: String,
    pub path: Vec<[i16; 2]>,
    #[serde(serialize_with = "u64_as_string")]
    pub key: u64,
    pub text_style: LabelTextStyle,
}

impl ParsedRoadTextLabel {
    pub fn new(text: String, path: Vec<[i16; 2]>, feature_id: u64, has_feature_id: bool, layer_name: &str, text_style: LabelTextStyle) -> ParsedRoadTextLabel {
        let key = parsed_label_key::make_road_label_key(&text, &path, feature_id, has_feature_id, layer_name);
        ParsedRoadTextLabel { text, path, key, text_style }
    }
}

pub(crate) fn u64_as_string<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(value)
}

fn optional_u64_as_string<S: Serializer>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error> {
    match value {
        Some(value) => serializer.collect_str(value),
        None => serializer.serialize_none(),
    }
}
