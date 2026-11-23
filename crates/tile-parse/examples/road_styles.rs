// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Prints a fixture tile's road layers: per structure and role, the index
//! count and every style's colour and line parameters. A debugging aid.
//! `cargo run --release -p tile-parse --example road_styles 15-9648-12320`

use std::io::Read;
use std::path::PathBuf;

use map_style::adaptation::{GlyphCoverage, TileLabelDecisions};
use map_style::style::default::ProtomapsBasemapMapStyle;
use map_style::style::{AnyMapStyle, MapStyleRuntime};
use map_style::support::{LabelFallbackPolicy, LabelLanguage};
use tile_parse::options::TileParseOptions;
use tile_parse::parser::TileMvtParser;
use tile_parse::tile::Tile;
use tile_parse::unification::unify;

fn main() {
    let name = std::env::args().nth(1).expect("a tile name such as 15-9648-12320");
    let parts: Vec<i32> = name.split('-').map(|p| p.parse().unwrap()).collect();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../fixtures/tiles/{name}.mvt.gz"));
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(std::fs::read(path).unwrap().as_slice()).read_to_end(&mut bytes).unwrap();
    let style = MapStyleRuntime::new(&AnyMapStyle::new(&ProtomapsBasemapMapStyle::default()));
    let decisions = TileLabelDecisions::new(style.style.as_ref(), GlyphCoverage::legacy_atlas_for_tests(), &LabelLanguage::english(), LabelFallbackPolicy::International);
    let parser = TileMvtParser::new(style, decisions, TileParseOptions::basic(true, 13));
    let prepared = parser.parse(Tile::new(parts[0], parts[1], parts[2]), &bytes).unwrap();
    let unified = unify(&prepared.result);
    let roles = ["shadow", "casing", "fill", "detail", "overlay"];
    let structures = ["tunnel", "ground", "automobileGround", "bridge"];
    for (i, layer) in unified.roads.iter().enumerate() {
        if layer.indices.is_empty() {
            continue;
        }
        println!("{} {}: {} indices", structures[i / 5], roles[i % 5], layer.indices.len());
        for (s, (color, line)) in layer.styles.iter().zip(&layer.line_styles).enumerate() {
            println!("  style {s}: color {:.2?} widthPts {} halfUnits {} edge {:.3} minPts {} lock {}", color, line.width_points, line.half_width_units, line.edge_threshold, line.minimum_width_points, line.world_lock_zoom);
        }
    }
}
