// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Parses every fixture tile natively and prints the slowest and the totals:
//! a quick look at the whole parser's cost outside the browser.
//! `cargo run --release -p tile-parse --example parse_fixtures`

use std::io::Read;
use std::path::PathBuf;
use std::time::Instant;

use map_style::adaptation::{GlyphCoverage, TileLabelDecisions};
use map_style::style::default::ProtomapsBasemapMapStyle;
use map_style::style::{AnyMapStyle, MapStyleRuntime};
use map_style::support::{LabelFallbackPolicy, LabelLanguage};
use tile_parse::options::TileParseOptions;
use tile_parse::parser::TileMvtParser;
use tile_parse::tile::Tile;

fn main() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/tiles");
    let style = MapStyleRuntime::new(&AnyMapStyle::new(&ProtomapsBasemapMapStyle::default()));
    let decisions = TileLabelDecisions::new(style.style.as_ref(), GlyphCoverage::legacy_atlas_for_tests(), &LabelLanguage::english(), LabelFallbackPolicy::International);
    let parser = TileMvtParser::new(style, decisions, TileParseOptions::basic(true, 13));
    let mut times: Vec<(f64, String, usize)> = Vec::new();
    let mut meta_bytes = 0usize;
    for entry in std::fs::read_dir(&dir).expect("fixtures/tiles") {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let Some(stem) = name.strip_suffix(".mvt.gz") else { continue };
        let parts: Vec<i32> = stem.split('-').map(|p| p.parse().unwrap()).collect();
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(std::fs::read(&path).unwrap().as_slice()).read_to_end(&mut bytes).unwrap();
        let tile = Tile::new(parts[0], parts[1], parts[2]);
        let start = Instant::now();
        let prepared = parser.parse(tile, &bytes).expect("decodes");
        times.push((start.elapsed().as_secs_f64() * 1000.0, stem.to_string(), prepared.arena.len()));
        meta_bytes += serde_json::to_string(&prepared.meta).map_or(0, |json| json.len());
    }
    times.sort_by(|a, b| b.0.total_cmp(&a.0));
    let total: f64 = times.iter().map(|t| t.0).sum();
    let mut sorted: Vec<f64> = times.iter().map(|t| t.0).collect();
    sorted.sort_by(f64::total_cmp);
    println!("tiles {}  total {:.0} ms  median {:.2} ms  p95 {:.2} ms", times.len(), total, sorted[sorted.len() / 2], sorted[sorted.len() * 95 / 100]);
    let arena_total: usize = times.iter().map(|t| t.2).sum();
    let mut arenas: Vec<usize> = times.iter().map(|t| t.2).collect();
    arenas.sort();
    println!(
        "arena total {:.1} MB  median {:.2} MB  max {:.2} MB  metadata JSON total {:.1} MB",
        arena_total as f64 / 1e6,
        arenas[arenas.len() / 2] as f64 / 1e6,
        arenas[arenas.len() - 1] as f64 / 1e6,
        meta_bytes as f64 / 1e6
    );
    for (ms, name, bytes) in times.iter().take(8) {
        println!("{ms:8.2} ms  {name:>16}  arena {:.1} MB", *bytes as f64 / 1e6);
    }
    // Where the biggest arena's bytes go, by slot.
    if let Some(name) = std::env::args().nth(1) {
        let parts: Vec<i32> = name.split('-').map(|p| p.parse().unwrap()).collect();
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(std::fs::read(dir.join(format!("{name}.mvt.gz"))).unwrap().as_slice()).read_to_end(&mut bytes).unwrap();
        let prepared = parser.parse(Tile::new(parts[0], parts[1], parts[2]), &bytes).unwrap();
        let mut spans: Vec<(usize, usize)> = prepared.meta.spans.iter().enumerate().map(|(i, s)| (s.byte_count, i)).collect();
        spans.sort_by(|a, b| b.cmp(a));
        for (count, slot) in spans.iter().take(10) {
            println!("slot {slot:3}  {:.2} MB", *count as f64 / 1e6);
        }
    }
}
