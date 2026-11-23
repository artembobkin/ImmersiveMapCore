// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! A real tile parsed end to end with the built-in Protomaps style: central
//! Moscow at zoom 14 from the local fixtures (`scripts/fetch-fixtures`). The
//! test passes without a word when the fixtures are not there, since it
//! never goes to the network.

use std::io::Read;
use std::path::PathBuf;

use map_style::adaptation::{GlyphCoverage, TileLabelDecisions};
use map_style::style::default::ProtomapsBasemapMapStyle;
use map_style::style::{AnyMapStyle, MapStyleRuntime};
use map_style::support::{LabelFallbackPolicy, LabelLanguage};
use tile_parse::options::TileParseOptions;
use tile_parse::parser::TileMvtParser;
use tile_parse::tile::Tile;

/// The spans of every arena besides the text runs, whatever the tile holds.
const ARENA_FIXED_SPAN_COUNT: usize = 116;
const LABEL_VERTEX_STRIDE: usize = 32;

fn fixture(z: i32, x: i32, y: i32) -> Option<Vec<u8>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../fixtures/tiles/{z}-{x}-{y}.mvt.gz"));
    let compressed = std::fs::read(&path).ok()?;
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(compressed.as_slice()).read_to_end(&mut bytes).expect("a fixture is a gzip member");
    Some(bytes)
}

#[test]
fn central_moscow_parses_into_every_kind_of_geometry() {
    let tile = Tile::new(14, 9903, 5121);
    let Some(bytes) = fixture(tile.z, tile.x, tile.y) else {
        eprintln!("fixtures/tiles is missing: run scripts/fetch-fixtures to exercise the whole-tile parse");
        return;
    };
    let style = MapStyleRuntime::new(&AnyMapStyle::new(&ProtomapsBasemapMapStyle::default()));
    let decisions = TileLabelDecisions::new(style.style.as_ref(), GlyphCoverage::legacy_atlas_for_tests(), &LabelLanguage::english(), LabelFallbackPolicy::International);
    let parser = TileMvtParser::new(style, decisions, TileParseOptions::basic(true, 13));
    let prepared = parser.parse(tile, &bytes).expect("the fixture decodes");
    let result = &prepared.result;

    assert!(result.polygon_by_style.values().any(|p| !p.is_empty()), "ground");
    assert!(!result.ordered_road_polygons.is_empty(), "roads");
    assert!(result.extruded_by_style.values().any(|m| !m.is_empty()), "buildings");
    assert!(!result.text_labels.is_empty(), "point labels");
    assert!(!result.road_text_labels.is_empty(), "road names");
    assert!(result.route_shield_candidates.is_empty(), "the sign candidates are spent");
    assert!(!prepared.arena.is_empty());

    // The labels laid out: glyph runs by style, road glyphs per anchor.
    let meta = &prepared.meta;
    let counts = meta.text_run_counts;
    assert!(counts.glyph_run_count > 0, "glyph runs");
    assert_eq!(meta.spans.len(), ARENA_FIXED_SPAN_COUNT + counts.glyph_run_count + counts.poi_icon_run_count + counts.route_shield_run_count);
    let labels = &meta.labels;
    assert_eq!(labels.text.glyph_run_styles.len(), counts.glyph_run_count);
    assert_eq!(labels.text.poi_icon_run_styles.len(), counts.poi_icon_run_count);
    assert_eq!(labels.text.route_shield_run_styles.len(), counts.route_shield_run_count);
    assert!(!labels.text.placement_inputs.is_empty(), "screen labels to place");
    let text_spans = &meta.spans[ARENA_FIXED_SPAN_COUNT - 2..meta.spans.len() - 2];
    assert!(text_spans.iter().take(counts.glyph_run_count).all(|s| s.element_count > 0 && s.element_count % 6 == 0 && s.byte_count == s.element_count * LABEL_VERTEX_STRIDE));
    let placed = labels.text.placement_inputs.len() as i32;
    for span in text_spans {
        for v in 0..span.element_count {
            let at = span.byte_offset + v * LABEL_VERTEX_STRIDE + 16;
            let index = i32::from_le_bytes(prepared.arena[at..at + 4].try_into().unwrap());
            assert!((0..placed).contains(&index), "a vertex names one of the tile's labels");
        }
    }
    assert!(!labels.road.path_labels.is_empty(), "road names");
    assert_eq!(labels.road.glyph_bound_ranges.len(), labels.road.path_labels.len());
    let road_glyphs = &meta.spans[meta.spans.len() - 2];
    let anchored: usize = labels.road.anchor_ranges.iter().zip(&labels.road.glyph_bound_ranges).map(|(a, g)| a.count * g.count).sum();
    assert_eq!(road_glyphs.element_count, anchored * 6, "every anchor draws its name's glyphs");
    assert!(anchored > 0);
    let json = serde_json::to_string(meta).expect("the metadata serializes");
    assert!(json.contains("\"placementInputs\"") && json.contains("\"glyphRunStyles\""));
}
