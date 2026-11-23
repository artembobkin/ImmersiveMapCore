// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! A parsed tile's labels laid out for the GPU and the frame (the Swift
//! `TileTextLabelsBuilder`, `TileRoadLabelsBuilder`,
//! `TileSurfaceLabelsBuilder` and the label parts of `PreparedTileCPU`):
//!
//! - the screen labels ([`text_labels`]): glyph runs by text style, POI
//!   icon runs, route sign plates, and one placement record per label the
//!   frame places and collides;
//! - the road names ([`road_labels`]): merged along their roads, with
//!   their glyphs once per anchor and the anchors along each path;
//! - the names painted on the map ([`surface_labels`]): glyph quads
//!   anchored in the tile and one record per name.
//!
//! The vertices become arena spans (`unification::write_arena`). The rest
//! is the label part of the tile's metadata ([`TileLabelsMeta`]), JSON in
//! camelCase for the TypeScript side (`tile/store/tile-gpu.ts`), a `u64`
//! key as a decimal string.

pub mod poi_sprite_atlas;
pub mod road_labels;
pub mod surface_labels;
pub mod text_labels;

use map_style::style::{LabelPlacement, LabelTextStyle, RouteShieldShape, SurfaceLabelPlacement};
use serde::Serialize;
use text_layout::{LabelVertex, TextLayout};

use crate::labels::contract::u64_as_string;
use crate::unification::ReadingStageResult;

/// What the frame places a screen label by (the Swift
/// `TextLabelPlacementInput`: its `TilePointInput` and `LabelPlacementMeta`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextLabelPlacementInput {
    /// The anchor as a share of the tile, (0, 0) at its north-west corner.
    pub uv: [f32; 2],
    /// The roof over the anchor in tile units (a 4096 extent), zero on open ground.
    pub roof_height: f32,
    /// Whether the label draws on that roof rather than on the ground.
    pub lifts_to_roof: bool,
    #[serde(serialize_with = "u64_as_string")]
    pub key: u64,
    pub sort_key: i32,
    pub collision_priority: i32,
    /// The collision box in layout points.
    pub label_size_points: [f32; 2],
    /// The camera zoom the label shows from, 0 for always.
    pub min_camera_zoom: f32,
    /// The camera zoom it goes away at. Infinity (never) serializes as null.
    pub max_camera_zoom: f32,
    /// Local detail, shown only near the camera.
    pub is_local: bool,
}

/// The look of one run of route sign plates (the Swift
/// `RouteShieldRunStyle`): the sign's appearance with its em fractions
/// resolved to layout points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteShieldRunStyle {
    pub shape: RouteShieldShape,
    pub fill_color: [f32; 3],
    pub border_color: [f32; 3],
    /// The band across the top, `None` (null) for a plate in one colour.
    pub header_color: Option<[f32; 3]>,
    /// The band's height as a share of the plate's, zero without a band.
    pub header_fraction: f32,
    pub border_width_points: f32,
    pub corner_radius_points: f32,
}

impl RouteShieldRunStyle {
    /// The identity two plates share a run by, bit for bit like the Swift `Hashable`.
    fn bits(&self) -> [u32; 14] {
        let header = self.header_color.unwrap_or([0.0; 3]);
        [
            self.shape.raw_value() as u32,
            self.fill_color[0].to_bits(),
            self.fill_color[1].to_bits(),
            self.fill_color[2].to_bits(),
            self.border_color[0].to_bits(),
            self.border_color[1].to_bits(),
            self.border_color[2].to_bits(),
            self.header_color.is_some() as u32,
            header[0].to_bits(),
            header[1].to_bits(),
            header[2].to_bits(),
            self.header_fraction.to_bits(),
            self.border_width_points.to_bits(),
            self.corner_radius_points.to_bits(),
        ]
    }

    pub fn same_run(&self, other: &RouteShieldRunStyle) -> bool {
        self.bits() == other.bits()
    }
}

/// One run of glyphs or icons of one style.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelRun<S> {
    pub style: S,
    pub vertices: Vec<LabelVertex>,
}

/// Every screen label of the tile, one set (the Swift
/// `PreparedTileCPU.TextLabelSet`): the frame draws them all and the
/// collisions decide what shows. A vertex's `label_index` is its label's
/// index in `placement_inputs`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextLabelSet {
    pub placement_inputs: Vec<TextLabelPlacementInput>,
    pub glyph_runs: Vec<LabelRun<LabelTextStyle>>,
    pub poi_icon_runs: Vec<LabelRun<LabelTextStyle>>,
    pub route_shield_runs: Vec<LabelRun<RouteShieldRunStyle>>,
}

/// A road path's points in `RoadLabels::path_inputs` and its label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoadPathRange {
    pub start: usize,
    pub count: usize,
    pub label_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoadPathLabel {
    pub text: String,
    #[serde(serialize_with = "u64_as_string")]
    pub key: u64,
}

/// A span of an array: of the glyph bounds, or of the anchors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexRange {
    pub start: usize,
    pub count: usize,
}

/// An anchor of a road label: the segment of its path, the parameter along
/// it, and its ordinal among the label's anchors.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoadLabelAnchor {
    pub path_index: u32,
    pub segment_index: u32,
    pub t: f32,
    pub anchor_ordinal: u32,
}

/// The tile's road names (the Swift `PreparedTileCPU.RoadLabels`). The
/// glyph vertices carry, in `label_index`, the glyph's index among all the
/// tile's placed glyphs: each anchor draws its label's glyphs again.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoadLabels {
    /// Every path's points as shares of the tile, (0, 0) north-west.
    pub path_inputs: Vec<[f32; 2]>,
    pub path_ranges: Vec<RoadPathRange>,
    pub path_labels: Vec<RoadPathLabel>,
    /// The style every road glyph of the tile draws with (the first label's).
    pub label_style: Option<LabelTextStyle>,
    #[serde(skip)]
    pub local_glyph_vertices: Vec<LabelVertex>,
    /// Each glyph's box in layout points about the baseline: (min x, max x, min y, max y).
    pub glyph_bounds: Vec<[f32; 4]>,
    /// Each label's glyphs in `glyph_bounds`, by path label.
    pub glyph_bound_ranges: Vec<IndexRange>,
    /// Each label's laid-out size in layout points.
    pub sizes: Vec<[f32; 2]>,
    /// Each label's anchors in `anchors`.
    pub anchor_ranges: Vec<IndexRange>,
    pub anchors: Vec<RoadLabelAnchor>,
}

/// One name painted on the map (the Swift `SurfaceLabelRecord`): what draws
/// it and its glyph quads in the tile's surface label vertices.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceLabelRecord {
    #[serde(serialize_with = "u64_as_string")]
    pub key: u64,
    pub style: LabelTextStyle,
    /// The zooms out of range (minus or plus infinity) serialize as null.
    pub placement: SurfaceLabelPlacement,
    pub vertex_start: usize,
    pub vertex_count: usize,
}

/// The names painted on the map: one array of glyph quads, a vertex's
/// `position` its label's anchor in tile render units (y up), its
/// `sprite_uv` its offset from the anchor in layout points.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SurfaceLabelSet {
    pub labels: Vec<SurfaceLabelRecord>,
    pub vertices: Vec<LabelVertex>,
}

/// All the labels a tile prepares.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PreparedLabels {
    pub text: TextLabelSet,
    pub road: RoadLabels,
    pub surface: SurfaceLabelSet,
}

/// The labels of a parse (the label half of the Swift
/// `TilePreparedDataBuilder.build`): a label stands on the screen or lies
/// on the map, never both.
pub fn build(layout: &TextLayout, result: &ReadingStageResult, tile_zoom: i32) -> PreparedLabels {
    let screen: Vec<_> = result.text_labels.iter().filter(|l| l.placement == LabelPlacement::Screen).collect();
    PreparedLabels {
        text: text_labels::build(layout, &screen),
        road: road_labels::build(layout, &result.road_text_labels),
        surface: surface_labels::build(layout, &result.text_labels, tile_zoom),
    }
}

/// The screen label set's CPU side (the Swift `TextLabelSetMeta`): the run
/// styles in span order, glyph runs, then POI icon runs, then plates.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextLabelSetMeta {
    pub placement_inputs: Vec<TextLabelPlacementInput>,
    pub glyph_run_styles: Vec<LabelTextStyle>,
    pub poi_icon_run_styles: Vec<LabelTextStyle>,
    pub route_shield_run_styles: Vec<RouteShieldRunStyle>,
}

/// The label part of a tile's metadata.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TileLabelsMeta {
    pub text: TextLabelSetMeta,
    pub road: RoadLabels,
    pub surface: Vec<SurfaceLabelRecord>,
}

/// The vertex arrays a tile's labels put in its arena, in slot order.
pub struct LabelArenaVertices {
    pub glyph_runs: Vec<Vec<LabelVertex>>,
    pub poi_icon_runs: Vec<Vec<LabelVertex>>,
    pub route_shield_runs: Vec<Vec<LabelVertex>>,
    pub road_glyphs: Vec<LabelVertex>,
    pub surface: Vec<LabelVertex>,
}

impl PreparedLabels {
    /// Splits the labels into what the arena holds and what the metadata does.
    pub fn split(self) -> (LabelArenaVertices, TileLabelsMeta) {
        let PreparedLabels { text, mut road, surface } = self;
        let (glyph_run_styles, glyph_runs) = text.glyph_runs.into_iter().map(|r| (r.style, r.vertices)).unzip();
        let (poi_icon_run_styles, poi_icon_runs) = text.poi_icon_runs.into_iter().map(|r| (r.style, r.vertices)).unzip();
        let (route_shield_run_styles, route_shield_runs) = text.route_shield_runs.into_iter().map(|r| (r.style, r.vertices)).unzip();
        let road_glyphs = std::mem::take(&mut road.local_glyph_vertices);
        let vertices = LabelArenaVertices { glyph_runs, poi_icon_runs, route_shield_runs, road_glyphs, surface: surface.vertices };
        let meta = TileLabelsMeta {
            text: TextLabelSetMeta { placement_inputs: text.placement_inputs, glyph_run_styles, poi_icon_run_styles, route_shield_run_styles },
            road,
            surface: surface.labels,
        };
        (vertices, meta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use map_style::style::{LabelFontWeight, PoiSpriteIcon, RouteShield, RouteShieldAppearance, RouteShieldStyle};
    use serde_json::Value;

    use crate::labels::{ParsedRoadTextLabel, ParsedTextLabel};

    fn keys(value: &Value) -> Vec<&str> {
        let mut keys: Vec<&str> = value.as_object().expect("an object").keys().map(String::as_str).collect();
        keys.sort();
        keys
    }

    /// The JSON the TypeScript types in `tile/store/prepared-labels.ts` mirror, field by field.
    #[test]
    fn the_metadata_serializes_to_the_typescript_contract() {
        let style = LabelTextStyle::new([0.1, 0.1, 0.1], [1.0, 1.0, 1.0], 0.15, 12.0, LabelFontWeight::Bold);
        let mut result = ReadingStageResult::default();
        let mut poi = ParsedTextLabel::new("Museum".into(), [100, 200], u64::MAX, 0, 0, style);
        poi.poi_icon = Some(PoiSpriteIcon::Museum);
        let mut shield = ParsedTextLabel::new("M1".into(), [300, 300], 2, 0, 0, style);
        shield.route_shields = Some(RouteShieldStyle::new(vec![RouteShield { text: "M1".into(), appearance: RouteShieldAppearance::new([0.0; 3], [1.0; 3]) }]));
        let sea = ParsedTextLabel {
            placement: LabelPlacement::Surface(SurfaceLabelPlacement { reference_zoom: 3.0, minimum_zoom: f64::NEG_INFINITY, maximum_zoom: 5.0, letter_spacing_em: 0.1 }),
            ..ParsedTextLabel::new("Sea".into(), [1, 2], 3, 0, 0, style)
        };
        result.text_labels = vec![poi, shield, sea];
        result.road_text_labels = vec![ParsedRoadTextLabel { text: "Arbat".into(), path: vec![[0, 0], [4000, 0]], key: 0, text_style: style }];
        let (vertices, meta) = build(&text_layout::bundled(), &result, 4).split();
        assert_eq!((vertices.glyph_runs.len(), vertices.poi_icon_runs.len(), vertices.route_shield_runs.len()), (2, 1, 1), "the sign's number draws in its own colour");
        assert!(!vertices.road_glyphs.is_empty() && !vertices.surface.is_empty());

        let json: Value = serde_json::to_value(&meta).unwrap();
        assert_eq!(keys(&json), vec!["road", "surface", "text"]);
        let text = &json["text"];
        assert_eq!(keys(text), vec!["glyphRunStyles", "placementInputs", "poiIconRunStyles", "routeShieldRunStyles"]);
        let input = &text["placementInputs"][0];
        assert_eq!(
            keys(input),
            vec!["collisionPriority", "isLocal", "key", "labelSizePoints", "liftsToRoof", "maxCameraZoom", "minCameraZoom", "roofHeight", "sortKey", "uv"]
        );
        assert_eq!(input["maxCameraZoom"], Value::Null, "never goes away");
        let keys_of_labels: Vec<&str> = text["placementInputs"].as_array().unwrap().iter().map(|p| p["key"].as_str().unwrap()).collect();
        assert!(keys_of_labels.contains(&"18446744073709551615"), "a u64 key is a decimal string");
        assert_eq!(
            keys(&text["glyphRunStyles"][0]),
            vec![
                "emboldenPoints",
                "fillColor",
                "haloEm",
                "haloSoftness",
                "isPointOfInterest",
                "key",
                "letterSpacingEm",
                "opacity",
                "sizePoints",
                "sphereFillColor",
                "sphereStrokeColor",
                "strokeColor",
                "uppercased",
                "weight"
            ]
        );
        assert_eq!(text["glyphRunStyles"][0]["weight"], "bold");
        let plate = &text["routeShieldRunStyles"][0];
        assert_eq!(keys(plate), vec!["borderColor", "borderWidthPoints", "cornerRadiusPoints", "fillColor", "headerColor", "headerFraction", "shape"]);
        assert_eq!((plate["shape"].as_str(), &plate["headerColor"]), (Some("rectangle"), &Value::Null));
        let road = &json["road"];
        assert_eq!(keys(road), vec!["anchorRanges", "anchors", "glyphBoundRanges", "glyphBounds", "labelStyle", "pathInputs", "pathLabels", "pathRanges", "sizes"]);
        assert_eq!(keys(&road["anchors"][0]), vec!["anchorOrdinal", "pathIndex", "segmentIndex", "t"]);
        assert_eq!(keys(&road["pathRanges"][0]), vec!["count", "labelIndex", "start"]);
        assert_eq!(keys(&road["pathLabels"][0]), vec!["key", "text"]);
        assert!(road["pathLabels"][0]["key"].is_string());
        let surface = &json["surface"][0];
        assert_eq!(keys(surface), vec!["key", "placement", "style", "vertexCount", "vertexStart"]);
        assert_eq!(keys(&surface["placement"]), vec!["letterSpacingEm", "maximumZoom", "minimumZoom", "referenceZoom"]);
        assert_eq!(surface["placement"]["minimumZoom"], Value::Null, "no lower bound");
    }
}
