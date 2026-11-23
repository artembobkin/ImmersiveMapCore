// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The road and label feature readers on small synthetic tiles, encoded here
//! by a minimal MVT writer independent of the decoder. Ported from the Swift
//! `TileMvtParserRoadLabelTests`, `TileMvtParserFallbackLabelTests` and
//! `LabelsEnabledParseTests`, driving the readers directly instead of the
//! whole parser.
//!
//! Skipped: the bundled atlas coverage cases (the atlas files are the
//! renderer's), and the prepared-cache identity of the label switch.

use std::sync::Arc;

use map_style::adaptation::{GlyphCoverage, TileLabelDecisions};
use map_style::style::{
    AnyMapStyle, FeatureStyle, FeatureStyleContext, FillStyle, LabelFontWeight, LabelTextStyle, LineGeometryStyle, LinePass, MapStyleRuntime,
    OnewayArrowDecoration, PointLabelOptions, PointLabelStyle, RoadDecorationKind, RoadPassRole, RoadStyle, RoadTier, RouteShield,
    RouteShieldAppearance, RouteShieldStyle, VectorTileMapStyle, VectorTileStyle, WaterNameKind,
};
use map_style::support::{LabelFallbackPolicy, LabelLanguage};
use tile_parse::labels::{LabelFeatureReader, ParsedTextLabel};
use tile_parse::options::TileParseOptions;
use tile_parse::reading::{PreparedLayer, prepare_layers};
use tile_parse::roads::layer_precomputation::RoadLayerContext;
use tile_parse::roads::line_reader::LineFeatureReader;
use tile_parse::tile::Tile;
use tile_parse::unification::{ReadingStageResult, RoadStructureKind};

// MARK: A minimal MVT writer

fn varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn key(out: &mut Vec<u8>, field: u32, wire: u32) {
    varint(out, ((field << 3) | wire) as u64);
}

fn bytes_field(out: &mut Vec<u8>, field: u32, bytes: &[u8]) {
    key(out, field, 2);
    varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

fn packed(out: &mut Vec<u8>, field: u32, values: &[u32]) {
    let mut body = Vec::new();
    for &v in values {
        varint(&mut body, v as u64);
    }
    bytes_field(out, field, &body);
}

fn zigzag(v: i32) -> u32 {
    ((v << 1) ^ (v >> 31)) as u32
}

#[derive(Clone, Copy)]
enum Geometry<'a> {
    Point((i32, i32)),
    Line(&'a [(i32, i32)]),
}

struct FeatureSpec<'a> {
    id: Option<u64>,
    geometry: Geometry<'a>,
    tags: &'a [(&'a str, &'a str)],
}

fn geometry_commands(geometry: Geometry<'_>) -> (u32, Vec<u32>) {
    let command = |id: u32, count: u32| (count << 3) | id;
    match geometry {
        Geometry::Point((x, y)) => (1, vec![command(1, 1), zigzag(x), zigzag(y)]),
        Geometry::Line(points) => {
            let mut out = vec![command(1, 1), zigzag(points[0].0), zigzag(points[0].1), command(2, points.len() as u32 - 1)];
            for pair in points.windows(2) {
                out.push(zigzag(pair[1].0 - pair[0].0));
                out.push(zigzag(pair[1].1 - pair[0].1));
            }
            (2, out)
        }
    }
}

fn encode_tile(layers: &[(&str, Vec<FeatureSpec<'_>>)]) -> Vec<u8> {
    let mut tile = Vec::new();
    for (name, features) in layers {
        let mut keys: Vec<&str> = Vec::new();
        let mut values: Vec<&str> = Vec::new();
        let mut layer = Vec::new();
        key(&mut layer, 15, 0);
        varint(&mut layer, 2);
        bytes_field(&mut layer, 1, name.as_bytes());
        for spec in features {
            let mut feature = Vec::new();
            if let Some(id) = spec.id {
                key(&mut feature, 1, 0);
                varint(&mut feature, id);
            }
            let mut tags = Vec::new();
            for (k, v) in spec.tags {
                let ki = keys.iter().position(|x| x == k).unwrap_or_else(|| {
                    keys.push(k);
                    keys.len() - 1
                });
                let vi = values.iter().position(|x| x == v).unwrap_or_else(|| {
                    values.push(v);
                    values.len() - 1
                });
                tags.push(ki as u32);
                tags.push(vi as u32);
            }
            packed(&mut feature, 2, &tags);
            let (kind, commands) = geometry_commands(spec.geometry);
            key(&mut feature, 3, 0);
            varint(&mut feature, kind as u64);
            packed(&mut feature, 4, &commands);
            bytes_field(&mut layer, 2, &feature);
        }
        for k in &keys {
            bytes_field(&mut layer, 3, k.as_bytes());
        }
        for v in &values {
            let mut value = Vec::new();
            bytes_field(&mut value, 1, v.as_bytes());
            bytes_field(&mut layer, 4, &value);
        }
        key(&mut layer, 5, 0);
        varint(&mut layer, 4096);
        bytes_field(&mut tile, 3, &layer);
    }
    tile
}

// MARK: Styles

fn runtime(style: impl VectorTileStyle + 'static) -> MapStyleRuntime {
    MapStyleRuntime::new(&AnyMapStyle::new(&VectorTileMapStyle::with_basemap_schema(Arc::new(style))))
}

fn decisions(runtime: &MapStyleRuntime, language: LabelLanguage, coverage: GlyphCoverage) -> TileLabelDecisions {
    TileLabelDecisions::new(runtime.style.as_ref(), coverage, &language, LabelFallbackPolicy::International)
}

fn text_style(key: i32, size: f32, weight: LabelFontWeight) -> LabelTextStyle {
    LabelTextStyle { key, ..LabelTextStyle::new([1.0, 1.0, 1.0], [0.0, 0.0, 0.0], 0.1, size, weight) }
}

/// Every feature a road of one fill stroke with a name along it, as the
/// Swift `RoadLabelStyle`, optionally oneway and with route signs.
struct RoadLabelStyle {
    oneway: bool,
    shields: bool,
}

impl VectorTileStyle for RoadLabelStyle {
    fn cache_fingerprint(&self) -> u32 {
        1
    }

    fn make_style(&self, _feature: &FeatureStyleContext<'_>) -> FeatureStyle {
        let shields = self.shields.then(|| {
            RouteShieldStyle::new(vec![RouteShield { text: "M9".to_owned(), appearance: RouteShieldAppearance::new([0.8, 0.0, 0.0], [1.0, 1.0, 1.0]) }])
        });
        FeatureStyle::Road(RoadStyle {
            fill: Some(LinePass::new(1, [1.0, 1.0, 1.0, 1.0], LineGeometryStyle::new(8.0))),
            paint: if self.oneway { vec![LinePass::new(2, [0.5, 0.5, 0.5, 1.0], LineGeometryStyle::new(8.0))] } else { Vec::new() },
            decoration: if self.oneway { RoadDecorationKind::OnewayArrow(OnewayArrowDecoration::default()) } else { RoadDecorationKind::None },
            tier: RoadTier::Automobile,
            label: Some(text_style(1, 12.0, LabelFontWeight::Thin)),
            shields,
            ..RoadStyle::default()
        })
    }
}

/// The ocean and sea names as point labels, as the Swift `FallbackWaterLabelStyle`.
struct FallbackWaterLabelStyle {
    style_id: &'static str,
}

impl FallbackWaterLabelStyle {
    fn water_style(text: Option<LabelTextStyle>) -> FeatureStyle {
        match text {
            Some(text) => FeatureStyle::PointLabel(PointLabelStyle::new(text.key as u8, text, PointLabelOptions::default())),
            None => FeatureStyle::Fill(FillStyle::new(1, [1.0, 1.0, 1.0, 1.0])),
        }
    }

    fn ocean() -> LabelTextStyle {
        text_style(3, 12.0, LabelFontWeight::Bold)
    }

    fn sea() -> LabelTextStyle {
        text_style(4, 10.0, LabelFontWeight::Thin)
    }
}

impl VectorTileStyle for FallbackWaterLabelStyle {
    fn cache_fingerprint(&self) -> u32 {
        1
    }

    fn style_id(&self) -> &str {
        self.style_id
    }

    fn make_style(&self, feature: &FeatureStyleContext<'_>) -> FeatureStyle {
        Self::water_style(match feature.properties.string("kind") {
            Some("ocean") => Some(Self::ocean()),
            Some("sea") => Some(Self::sea()),
            _ => None,
        })
    }

    fn water_name_style(&self, kind: WaterNameKind, _tile_zoom: i32) -> Option<FeatureStyle> {
        Some(Self::water_style(Some(if kind == WaterNameKind::Ocean { Self::ocean() } else { Self::sea() })))
    }
}

// MARK: Driving the readers

fn options(labels_enabled: bool) -> TileParseOptions {
    TileParseOptions::basic(labels_enabled, 13)
}

fn read_lines(layers: &[PreparedLayer<'_>], reader: &LineFeatureReader<'_>, tile: Tile, result: &mut ReadingStageResult) {
    for layer in layers {
        let roads = RoadLayerContext::build(layer, tile);
        for index in 0..layer.features.len() {
            if layer.features[index].feature.geometry_type == mvt::GeometryType::LineString {
                reader.read(layer, index, tile, &roads, result);
            }
        }
    }
    reader.append_route_shield_labels(tile, result);
}

fn read_points(layers: &[PreparedLayer<'_>], reader: &LabelFeatureReader<'_>, tile: Tile, result: &mut ReadingStageResult) {
    for layer in layers {
        for (index, read) in layer.features.iter().enumerate() {
            if let FeatureStyle::PointLabel(style) = &read.style {
                reader.read(layer, index, style, tile, result);
            }
        }
    }
    reader.append_low_zoom_water_labels(tile, result);
}

const ROAD_TAGS: &[(&str, &str)] = &[("name", "Rue Native"), ("name:en", "Rivoli Street"), ("name:fr", "Rue de Rivoli")];

fn parse_road(style: RoadLabelStyle, language: LabelLanguage, labels_enabled: bool, line: &[(i32, i32)]) -> ReadingStageResult {
    let runtime = runtime(style);
    let decisions = decisions(&runtime, language, GlyphCoverage::legacy_atlas_for_tests());
    let options = options(labels_enabled);
    let bytes = encode_tile(&[("roads", vec![FeatureSpec { id: Some(1), geometry: Geometry::Line(line), tags: ROAD_TAGS }])]);
    let decoded = mvt::Tile::decode(&bytes).expect("the writer writes a valid tile");
    let tile = Tile::new(14, 0, 0);
    let layers = prepare_layers(&decoded, tile, &runtime);
    let mut result = ReadingStageResult::default();
    read_lines(&layers, &LineFeatureReader::new(&decisions, &options), tile, &mut result);
    result
}

#[test]
fn road_labels_use_the_shared_language_preferences() {
    let result = parse_road(RoadLabelStyle { oneway: false, shields: false }, LabelLanguage::french(), true, &[(100, 100), (900, 100)]);
    let texts: Vec<_> = result.road_text_labels.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(texts, ["Rue de Rivoli"]);
    assert_eq!(result.road_text_labels[0].path, vec![[100, 100], [900, 100]]);
    assert!(!result.ordered_road_polygons.is_empty(), "the road draws on the separate-road path");
    assert!(result.ordered_road_polygons.iter().all(|p| p.structure == RoadStructureKind::AutomobileGround));
}

#[test]
fn labels_off_bake_no_road_label_and_no_signs() {
    let result = parse_road(RoadLabelStyle { oneway: false, shields: true }, LabelLanguage::english(), false, &[(100, 2048), (3900, 2048)]);
    assert!(result.road_text_labels.is_empty());
    assert!(result.text_labels.is_empty());
    assert!(!result.ordered_road_polygons.is_empty(), "the road itself still draws");
}

#[test]
fn a_oneway_road_stamps_arrows_under_its_detail_pass() {
    let result = parse_road(RoadLabelStyle { oneway: true, shields: false }, LabelLanguage::english(), true, &[(100, 2048), (3900, 2048)]);
    let detail: Vec<_> = result.ordered_road_polygons.iter().filter(|p| p.role == RoadPassRole::Detail).collect();
    assert!(!detail.is_empty());
    assert!(detail.iter().all(|p| p.style_key == 2 && !p.polygon.is_line_ribbon()), "arrows, not a ribbon");
    // 3800 units at width 8: 26 in from each end, 3748 left, nine arrows of a
    // tail and a head each.
    assert_eq!(detail.len(), 18);
    assert!(result.ordered_road_polygons.iter().any(|p| p.role == RoadPassRole::Fill && p.polygon.is_line_ribbon()));
}

#[test]
fn a_signed_road_stands_its_signs_once_near_the_centre() {
    let result = parse_road(RoadLabelStyle { oneway: false, shields: true }, LabelLanguage::english(), true, &[(0, 2048), (4096, 2048)]);
    assert!(result.route_shield_candidates.is_empty(), "the candidates are spent");
    let signs: Vec<&ParsedTextLabel> = result.text_labels.iter().filter(|l| l.route_shields.is_some()).collect();
    assert_eq!(signs.len(), 1, "200 points apart on a 256-point tile");
    let sign = signs[0];
    assert_eq!(sign.text, "M9");
    assert_eq!(sign.position, [1920, 2048]);
    assert_eq!(sign.text_style.key, 1);
    assert_eq!(sign.text_style.fill_color, [1.0, 1.0, 1.0]);
    assert_eq!(sign.text_style.halo_em, 0.0);
}

fn parse_water(language: LabelLanguage, tile: Tile, layers: &[(&str, Vec<FeatureSpec<'_>>)], coverage: GlyphCoverage, style_id: &'static str) -> Vec<ParsedTextLabel> {
    let runtime = runtime(FallbackWaterLabelStyle { style_id });
    let decisions = decisions(&runtime, language, coverage);
    let bytes = encode_tile(layers);
    let decoded = mvt::Tile::decode(&bytes).expect("the writer writes a valid tile");
    let prepared = prepare_layers(&decoded, tile, &runtime);
    let mut result = ReadingStageResult::default();
    read_points(&prepared, &LabelFeatureReader::new(&decisions, &runtime), tile, &mut result);
    result.text_labels
}

fn fallback_texts(language: LabelLanguage, tile: Tile) -> Vec<String> {
    parse_water(language, tile, &[], GlyphCoverage::legacy_atlas_for_tests(), "generic").into_iter().map(|l| l.text).collect()
}

const WORLD: Tile = Tile::new(0, 0, 0);

const ATLANTIC_TAGS: &[(&str, &str)] = &[("kind", "ocean"), ("type", "ocean"), ("name", "Atlantic Ocean"), ("name:en", "Atlantic Ocean")];

fn atlantic_layer() -> Vec<(&'static str, Vec<FeatureSpec<'static>>)> {
    vec![("water", vec![FeatureSpec { id: Some(1), geometry: Geometry::Point((2048, 2048)), tags: ATLANTIC_TAGS }])]
}

#[test]
fn french_falls_back_to_english_where_the_accented_name_is_unsupported() {
    let labels = fallback_texts(LabelLanguage::french(), WORLD);
    assert!(labels.contains(&"Atlantic Ocean".to_owned()));
    assert!(!labels.contains(&"Océan Atlantique".to_owned()));
}

#[test]
fn german_resolves_a_renderable_name_and_falls_back_for_an_accented_one() {
    let labels = fallback_texts(LabelLanguage::german(), WORLD);
    assert!(labels.contains(&"Atlantischer Ozean".to_owned()));
    assert!(!labels.contains(&"Atlantic Ocean".to_owned()));
    assert!(labels.contains(&"Southern Ocean".to_owned()));
    assert!(!labels.contains(&"Südlicher Ozean".to_owned()));
}

#[test]
fn spanish_falls_back_to_english_for_the_ocean_and_the_sea() {
    let labels = fallback_texts(LabelLanguage::spanish(), WORLD);
    assert!(labels.contains(&"Atlantic Ocean".to_owned()));
    assert!(!labels.contains(&"Océano Atlántico".to_owned()));
    let seas = fallback_texts(LabelLanguage::spanish(), Tile::new(2, 2, 1));
    assert!(seas.contains(&"Mediterranean Sea".to_owned()));
    assert!(!seas.contains(&"Mar Mediterráneo".to_owned()));
}

#[test]
fn english_and_russian_fallback_names_remain_available() {
    assert!(fallback_texts(LabelLanguage::english(), WORLD).contains(&"Atlantic Ocean".to_owned()));
    assert!(fallback_texts(LabelLanguage::russian(), WORLD).contains(&"Атлантический океан".to_owned()));
}

#[test]
fn a_name_with_unsupported_glyphs_falls_back_to_english() {
    let ascii = GlyphCoverage::new("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789 ".chars().map(|c| c as u32));
    let labels: Vec<_> = parse_water(LabelLanguage::russian(), WORLD, &[], ascii, "generic").into_iter().map(|l| l.text).collect();
    assert!(labels.contains(&"Atlantic Ocean".to_owned()));
    assert!(!labels.contains(&"Атлантический океан".to_owned()));
}

#[test]
fn the_tiles_own_water_name_suppresses_the_fallback_in_any_spelling() {
    let labels: Vec<_> =
        parse_water(LabelLanguage::russian(), WORLD, &atlantic_layer(), GlyphCoverage::legacy_atlas_for_tests(), "generic").into_iter().map(|l| l.text).collect();
    assert_eq!(labels.iter().filter(|t| *t == "Atlantic Ocean").count(), 1);
    assert!(!labels.contains(&"Атлантический океан".to_owned()));
}

#[test]
fn no_fallback_name_above_zoom_two() {
    assert!(fallback_texts(LabelLanguage::english(), Tile::new(3, 0, 0)).is_empty());
}

#[test]
fn point_labels_are_identified_in_the_styles_own_namespace() {
    use map_style::adaptation::LabelIdentity;
    use std::borrow::Cow;
    let labels = parse_water(LabelLanguage::english(), WORLD, &atlantic_layer(), GlyphCoverage::legacy_atlas_for_tests(), "parser-provider");
    let key = |style_id: &str| {
        LabelIdentity::StyleFeature { style_id: Cow::Borrowed(style_id), layer_name: Cow::Borrowed("water"), feature_id: 1 }.runtime_key()
    };
    let keys: Vec<u64> = labels.iter().map(|l| l.key).collect();
    assert!(keys.contains(&key("parser-provider")));
    assert!(!keys.contains(&key("protomaps")));
    let own = labels.iter().find(|l| l.key == key("parser-provider")).expect("the tile's label");
    assert_eq!(own.feature_id, Some(1));
    assert_eq!(own.position, [2048, 2048]);
}

#[test]
fn a_label_serializes_camel_case_with_its_ids_as_strings() {
    let labels = parse_water(LabelLanguage::english(), WORLD, &atlantic_layer(), GlyphCoverage::legacy_atlas_for_tests(), "generic");
    let json = serde_json::to_value(&labels[0]).expect("serializes");
    assert!(json["key"].is_string());
    assert_eq!(json["featureId"], serde_json::json!("1"));
    assert_eq!(json["position"], serde_json::json!([2048, 2048]));
    assert_eq!(json["textStyle"]["sizePoints"], serde_json::json!(12.0));
    assert_eq!(json["placement"], serde_json::json!("screen"));
    assert_eq!(json["liftsToRoof"], serde_json::json!(false));
    let fallback = labels.iter().find(|l| l.feature_id.is_none()).expect("a synthesized name");
    assert!(serde_json::to_value(fallback).expect("serializes")["featureId"].is_null());
}
