// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The tiles parsed for the ground relief, on small synthetic tiles encoded
//! here by a minimal MVT writer independent of the decoder. Ported from the
//! Swift `GroundReliefBuildingParseTests` and `GroundReliefRoadParseTests`.
//!
//! The buildings: every extruded vertex carries its volume's ordinal in the
//! tile, and the tile lists each volume's anchor, the centre of its
//! footprint's bounds, so the relief lifts a building whole. The roads: with
//! the relief on, a road's ribbon takes a row of vertices at every crossing
//! of the flattening's grid, so it follows the relief between its nodes as
//! the ground does. Off, a segment stays one quad.
//!
//! The prepared-cache identity of the relief switch is the TS side's
//! (`preparedTileCacheNamespace`, tile-parser-config.test.ts).

use std::collections::{BTreeMap, BTreeSet};

use map_style::adaptation::{GlyphCoverage, TileLabelDecisions};
use map_style::style::default::ProtomapsBasemapMapStyle;
use map_style::style::{AnyMapStyle, MapStyleRuntime};
use map_style::support::{LabelFallbackPolicy, LabelLanguage};
use tile_parse::coordinate_space::TILE_EXTENT_F64;
use tile_parse::geom::V2;
use tile_parse::options::TileParseOptions;
use tile_parse::parse_line::ParseLine;
use tile_parse::parser::{PreparedTile, TileMvtParser};
use tile_parse::tile::Tile;
use tile_parse::unification::unify;

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
    Line(&'a [(i32, i32)]),
    /// One exterior ring, its last point not repeated.
    Polygon(&'a [(i32, i32)]),
}

struct FeatureSpec<'a> {
    id: u64,
    geometry: Geometry<'a>,
    tags: &'a [(&'a str, &'a str)],
}

fn path_commands(points: &[(i32, i32)], out: &mut Vec<u32>) {
    let command = |id: u32, count: u32| (count << 3) | id;
    out.extend([command(1, 1), zigzag(points[0].0), zigzag(points[0].1), command(2, points.len() as u32 - 1)]);
    for pair in points.windows(2) {
        out.push(zigzag(pair[1].0 - pair[0].0));
        out.push(zigzag(pair[1].1 - pair[0].1));
    }
}

fn geometry_commands(geometry: Geometry<'_>) -> (u32, Vec<u32>) {
    let mut out = Vec::new();
    match geometry {
        Geometry::Line(points) => {
            path_commands(points, &mut out);
            (2, out)
        }
        Geometry::Polygon(ring) => {
            path_commands(ring, &mut out);
            // ClosePath.
            out.push((1 << 3) | 7);
            (3, out)
        }
    }
}

fn encode_tile(name: &str, features: &[FeatureSpec<'_>]) -> Vec<u8> {
    let mut keys: Vec<&str> = Vec::new();
    let mut values: Vec<&str> = Vec::new();
    let mut layer = Vec::new();
    key(&mut layer, 15, 0);
    varint(&mut layer, 2);
    bytes_field(&mut layer, 1, name.as_bytes());
    for spec in features {
        let mut feature = Vec::new();
        key(&mut feature, 1, 0);
        varint(&mut feature, spec.id);
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
    let mut tile = Vec::new();
    bytes_field(&mut tile, 3, &layer);
    tile
}

// MARK: The parser

const TILE: Tile = Tile { z: 15, x: 9651, y: 12319 };

/// The default flattening grids by tile zoom (`GroundFlatteningSettings.gridByTileZoom`).
fn default_grids() -> BTreeMap<i32, i32> {
    BTreeMap::from([(0, 64), (1, 64), (2, 32), (3, 32), (9, 128), (10, 128), (11, 128), (12, 128), (13, 64), (14, 32)])
}

fn options(flattening_from: Option<i32>, grid: i32, relief: bool) -> TileParseOptions {
    TileParseOptions::new(true, false, 14, Some(15), flattening_from, grid, default_grids(), relief, None, 0)
}

fn parse(options: TileParseOptions, bytes: &[u8]) -> PreparedTile {
    let style = MapStyleRuntime::new(&AnyMapStyle::new(&ProtomapsBasemapMapStyle::default()));
    let decisions = TileLabelDecisions::new(style.style.as_ref(), GlyphCoverage::legacy_atlas_for_tests(), &LabelLanguage::english(), LabelFallbackPolicy::International);
    TileMvtParser::new(style, decisions, options).parse(TILE, bytes).expect("the writer writes a valid tile")
}

// MARK: The buildings

/// 14.2 fixed point, the extruded vertex's position (`ExtrudedVertexIn.positionScale`).
const EXTRUDED_POSITION_SCALE: f32 = 4.0;
const EXTRUDED_VERTEX_STRIDE: usize = 12;

#[test]
fn every_volume_has_an_ordinal_and_an_anchor_at_its_footprints_centre() {
    // Two square buildings, tile space (y down): the first at 512...1024 in
    // x and 1024...1536 in y, the second lower right.
    let bytes = encode_tile(
        "buildings",
        &[
            FeatureSpec { id: 1, geometry: Geometry::Polygon(&[(512, 1024), (1024, 1024), (1024, 1536), (512, 1536)]), tags: &[("kind", "building"), ("height", "30")] },
            FeatureSpec { id: 2, geometry: Geometry::Polygon(&[(2048, 2560), (3072, 2560), (3072, 3584), (2048, 3584)]), tags: &[("kind", "building"), ("height", "12")] },
        ],
    );
    let prepared = parse(options(Some(15), 16, false), &bytes);
    let extruded = unify(&prepared.result).extruded;
    assert_eq!(extruded.building_anchors.len(), 2);
    assert_eq!(prepared.meta.building_anchors, extruded.building_anchors, "The anchors reach the metadata");
    // Render space is y up: the anchors' y is the extent less the tile y.
    let extent = TILE_EXTENT_F64 as i32;
    let anchors: BTreeSet<(i32, i32)> = extruded.building_anchors.iter().map(|a| (a[0].round() as i32, a[1].round() as i32)).collect();
    assert_eq!(anchors, BTreeSet::from([(768, extent - 1280), (2560, extent - 3072)]));
    assert_eq!(extruded.vertices.len(), extruded.vertex_count * EXTRUDED_VERTEX_STRIDE, "Every vertex keeps its twelve bytes");
    let mut ordinals = BTreeSet::new();
    for vertex in extruded.vertices.as_chunks::<EXTRUDED_VERTEX_STRIDE>().0 {
        let x = i16::from_le_bytes([vertex[0], vertex[1]]) as f32 / EXTRUDED_POSITION_SCALE;
        let y = i16::from_le_bytes([vertex[2], vertex[3]]) as f32 / EXTRUDED_POSITION_SCALE;
        let ordinal = u16::from_le_bytes([vertex[10], vertex[11]]);
        ordinals.insert(ordinal);
        // The ordinal names the volume the vertex belongs to: its position
        // lies within that anchor's footprint.
        let anchor = extruded.building_anchors[ordinal as usize];
        assert!((x - anchor[0]).abs() <= 520.0, "x of a vertex of volume {ordinal}");
        assert!((y - anchor[1]).abs() <= 520.0, "y of a vertex of volume {ordinal}");
    }
    assert_eq!(ordinals, BTreeSet::from([0, 1]), "Every vertex names one of the two volumes");
}

// MARK: The roads

/// One primary road straight across the tile, a third of the way down, with
/// a bend near its end so the join and the last segment are there too.
fn parse_road(relief: bool, grid: i32) -> PreparedTile {
    let bytes = encode_tile(
        "roads",
        &[FeatureSpec { id: 1, geometry: Geometry::Line(&[(0, 1400), (3500, 1400), (4096, 1900)]), tags: &[("kind", "major_road"), ("kind_detail", "primary")] }],
    );
    parse(options(Some(15), grid, relief), &bytes)
}

/// The distinct x positions of the ribbon's rows on the straight stretch:
/// the centreline points of the rim vertices (the ones with a direction) at
/// the road's y, in render space.
fn row_positions(prepared: &PreparedTile) -> BTreeSet<i32> {
    let road_y = TILE_EXTENT_F64 as f32 - 1400.0;
    let mut positions = BTreeSet::new();
    assert!(!prepared.result.ordered_road_polygons.is_empty(), "The road is parsed");
    for road in &prepared.result.ordered_road_polygons {
        let polygon = &road.polygon;
        for (vertex, normal) in polygon.vertices.iter().zip(&polygon.line_normals) {
            if *normal != [0, 0] && (vertex.y - road_y).abs() < 0.01 {
                positions.insert(vertex.x.round() as i32);
            }
        }
    }
    positions
}

#[test]
fn a_road_on_a_relief_takes_a_row_at_every_grid_crossing() {
    let rows = row_positions(&parse_road(true, 16));
    for crossing in (256..3500).step_by(256) {
        assert!(rows.contains(&crossing), "A row at the grid line x = {crossing}: {rows:?}");
    }
    assert!(!rows.contains(&3), "No row off the grid");
}

#[test]
fn the_rows_follow_the_grid() {
    let rows = row_positions(&parse_road(true, 64));
    assert!(rows.contains(&64));
    assert!(rows.contains(&3456));
    assert!(!rows.contains(&(256 + 32)));
}

#[test]
fn a_road_without_a_relief_keeps_its_segments_whole() {
    let rows = row_positions(&parse_road(false, 16));
    for crossing in (256..3500).step_by(256) {
        assert!(!rows.contains(&crossing), "No row at x = {crossing}");
    }
}

#[test]
fn a_row_is_added_at_every_crossing_between_two_rows() {
    let rows = ParseLine::subdivided_rows(&[(V2::new(10.0, 20.0), 0.0), (V2::new(550.0, 300.0), 1.0)], 256.0);
    assert_eq!(rows.len(), 6, "Crossings of the diagonal x - y = 0, then x = 256, y = 256 and x = 512");
    assert!((rows[1].0.x - rows[1].0.y).abs() < 0.001, "On the cell's diagonal");
    assert!((rows[2].0.x - 256.0).abs() < 0.001);
    assert!((rows[3].0.y - 256.0).abs() < 0.001);
    assert!((rows[4].0.x - 512.0).abs() < 0.001);
    for index in 1..rows.len() {
        assert!(rows[index].1 > rows[index - 1].1, "The parameter grows along the strip");
    }
    assert!((rows[2].1 - (256.0 - 10.0) / 540.0).abs() < 0.001);
}

/// Every piece of a strip between two rows lies inside one triangle of the
/// ground: no piece crosses a grid line or a cell's diagonal.
#[test]
fn no_piece_crosses_a_triangle_of_the_ground() {
    let step = 256.0_f32;
    let rows = ParseLine::subdivided_rows(&[(V2::new(37.0, 900.0), 0.0), (V2::new(1800.0, 110.0), 1.0)], step);
    for index in 1..rows.len() {
        let a = rows[index - 1].0;
        let b = rows[index].0;
        let cell = (((a.x + b.x) * 0.5 / step).floor(), ((a.y + b.y) * 0.5 / step).floor());
        for point in [a, b] {
            assert!(point.x >= cell.0 * step - 0.01 && point.x <= (cell.0 + 1.0) * step + 0.01);
            assert!(point.y >= cell.1 * step - 0.01 && point.y <= (cell.1 + 1.0) * step + 0.01);
        }
        // Both ends on the same side of the cell's diagonal, or on it.
        let diagonal = (cell.0 - cell.1) * step;
        let side_a = a.x - a.y - diagonal;
        let side_b = b.x - b.y - diagonal;
        assert!(side_a.abs() < 0.05 || side_b.abs() < 0.05 || side_a * side_b > 0.0, "A piece across the diagonal at rows {}, {index}", index - 1);
    }
}

#[test]
fn a_row_on_a_grid_line_is_not_doubled() {
    let rows = ParseLine::subdivided_rows(&[(V2::new(256.0, 0.0), 0.0), (V2::new(512.0, 0.0), 1.0)], 256.0);
    assert_eq!(rows.len(), 2, "The endpoints lie on grid lines, nothing lies between");
}

/// The options as the map makes them with the relief on (`tileParseOptionsConfig`):
/// the ground flattened from the relief's appear zoom, 10 by default.
#[test]
fn the_roads_follow_the_grid_of_every_flattened_tile_on_the_relief() {
    assert!(options(Some(10), 16, true).relief_subdivision_step(15).is_some(), "On");
    assert_eq!(options(Some(15), 16, false).relief_subdivision_step(15), None, "Off by default, laid flat");
    let relief = options(Some(10), 32, true);
    assert_eq!(relief.relief_subdivision_step(15), Some(128.0));
    assert_eq!(relief.relief_subdivision_step(16), Some(128.0), "With the relief on every tile from its zoom is flattened, and its roads follow the grid");
    assert_eq!(relief.relief_subdivision_step(11), Some(32.0), "A zoom with a grid of its own follows that grid");
    assert_eq!(relief.relief_subdivision_step(5), None, "A tile coarser than the relief's zoom is not flattened and keeps its roads");
    assert_eq!(options(Some(15), 32, false).relief_subdivision_step(15), None, "A tile off the relief keeps its roads");
}
