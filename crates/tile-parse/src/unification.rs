// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The second stage of a parse: packs what the readers accumulated into the
//! tile's streams, one geometry layer per bucket with the style tables the
//! vertices index into, then lays everything out as the arena the GPU binds.
//! A port of the Swift `TileUnificationStage`, `ReadingStageResult`,
//! `TileArenaSchema`, `TileArenaImageMath`, `IndexStorageMath` and
//! `GroundStyleRunScanner`.
//!
//! The ground layer is class-split (fills, then ribbons), the roads are
//! bucketed by structure and pass role and ranked by class priority, the
//! buildings are one mesh, each building's distinct vertices held once (not
//! in Swift, docs/STATUS.md). The arena's slot order, strides, 256-byte span
//! alignment and index narrowing are the contract with the TypeScript
//! renderer (`tile/store/tile-gpu.ts`) and with the disk cache.

use std::collections::BTreeMap;

use serde::Serialize;
use text_layout::LabelVertex;

use crate::buildings::mesh_builder::{ParsedExtrudedMesh, ParsedExtrudedVertex};
use crate::geom::{V2, clamp_i16, round};
use crate::parse_line::ParseLine;
use crate::parsed_polygon::ParsedPolygon;
use crate::prepared_labels::{PreparedLabels, TileLabelsMeta};
use crate::tile::quarters;

/// The prepared format version: bumped whenever what the parser bakes changes.
/// 2: the label slots carry the laid-out labels, the metadata their records.
/// 3: a sharp level of detail of the buildings in a slot of its own (measured
/// and dropped, docs/benchmarks/B21-building-levels.md).
/// 4: the version 2 layout again, each building's distinct vertices held once.
pub const PREPARED_FORMAT_VERSION: u32 = 4;

pub const ARENA_SPAN_ALIGNMENT: usize = 256;

/// The GPU-side line parameters of one style, 68 bytes (the Swift
/// `TileLineStyle`, mirrored by `LineStyle` in tile-shading.wgsl).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[repr(C)]
pub struct TileLineStyle {
    pub width_points: f32,
    pub dash_length_points: f32,
    pub dash_gap_points: f32,
    pub edge_threshold: f32,
    pub minimum_width_points: f32,
    pub dash_in_tile_units: f32,
    pub half_width_units: f32,
    pub world_lock_zoom: f32,
    pub ramp_start_width_points: f32,
    pub ramp_start_zoom: f32,
    pub ramp_end_zoom: f32,
    pub ramp_start_alpha: f32,
    pub sphere_alpha: f32,
    pub sphere_color: [f32; 3],
    pub sphere_look_until_zoom: f32,
}

/// What a line pass says about a stroke, the inputs of `make_tile_line_style`
/// (the fields of the Swift `LinePass` the unification reads).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinePassParameters {
    pub color: [f32; 4],
    pub line_width: f64,
    pub line_width_points: f32,
    pub dash_length_points: f32,
    pub dash_gap_points: f32,
    pub minimum_width_points: f32,
    pub dash_in_tile_units: bool,
    pub point_width_world_lock_zoom: f32,
    /// (start width points, start zoom, end zoom, start alpha), zeros for none.
    pub point_width_ramp: [f32; 4],
    pub sphere_alpha: f32,
    pub sphere_color: Option<[f32; 3]>,
    pub sphere_look_until_zoom: Option<f32>,
}

/// The GPU-side line parameters of one style. The edge threshold derives from
/// the tessellated width and the tessellator's feather constant, so the two
/// stay one definition: a style with no line width keeps a zero threshold,
/// which tells the shader to skip line coverage.
pub fn make_tile_line_style(p: &LinePassParameters) -> TileLineStyle {
    let half_width = (p.line_width * 0.5) as f32;
    let edge_threshold = if half_width > 0.0 { half_width / ParseLine::extruded_half_width(half_width) } else { 0.0 };
    TileLineStyle {
        width_points: p.line_width_points,
        dash_length_points: p.dash_length_points,
        dash_gap_points: p.dash_gap_points,
        edge_threshold,
        minimum_width_points: p.minimum_width_points,
        dash_in_tile_units: if p.dash_in_tile_units { 1.0 } else { 0.0 },
        half_width_units: half_width,
        world_lock_zoom: p.point_width_world_lock_zoom,
        ramp_start_width_points: p.point_width_ramp[0],
        ramp_start_zoom: p.point_width_ramp[1],
        ramp_end_zoom: p.point_width_ramp[2],
        ramp_start_alpha: p.point_width_ramp[3],
        sphere_alpha: p.sphere_alpha,
        sphere_color: p.sphere_color.unwrap_or([p.color[0], p.color[1], p.color[2]]),
        sphere_look_until_zoom: p.sphere_look_until_zoom.unwrap_or(0.0),
    }
}

/// One style as the tile bakes it: its colour, its zoom fade (x the zoom of
/// zero alpha, y of full alpha) and its line parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BakedStyle {
    pub color: [f32; 4],
    pub zoom_fade: [f32; 2],
    pub line: TileLineStyle,
}

impl BakedStyle {
    /// The ribbon width every fill is baked with: the edge threshold it
    /// yields is what the fill shaders have always read.
    pub const FILL_RIBBON_WIDTH: f64 = 100.0;

    /// A stroke, as the Swift `BakedStyle(pass:)`.
    pub fn from_pass(pass: &map_style::style::LinePass) -> BakedStyle {
        let ramp = pass.point_width_ramp.map_or([0.0, 0.0, 0.0, 1.0], |r| [r.start_width_points, r.start_zoom, r.end_zoom, r.start_alpha]);
        let parameters = LinePassParameters {
            color: pass.color,
            line_width: pass.line_geometry.line_width,
            line_width_points: pass.line_width_points,
            dash_length_points: pass.dash_length_points,
            dash_gap_points: pass.dash_gap_points,
            minimum_width_points: pass.minimum_width_points,
            dash_in_tile_units: pass.dash_in_tile_units,
            point_width_world_lock_zoom: pass.point_width_world_lock_zoom,
            point_width_ramp: ramp,
            sphere_alpha: pass.sphere_alpha,
            sphere_color: pass.sphere_color,
            sphere_look_until_zoom: pass.sphere_look_until_zoom,
        };
        BakedStyle { color: pass.color, zoom_fade: pass.zoom_fade.shader_pair(), line: make_tile_line_style(&parameters) }
    }

    /// A fill, baked as a stroke of its colour on the standard fill ribbon.
    /// Its colour on the globe rides in the line style's sphere colour.
    pub fn from_fill(fill: &map_style::style::FillStyle) -> BakedStyle {
        let pass = map_style::style::LinePass {
            zoom_fade: fill.zoom_fade,
            sphere_color: fill.sphere_color,
            ..map_style::style::LinePass::new(fill.key, fill.color, map_style::style::LineGeometryStyle::new(Self::FILL_RIBBON_WIDTH))
        };
        Self::from_pass(&pass)
    }

    pub fn from_extrusion(extrusion: &map_style::style::ExtrusionStyle) -> BakedStyle {
        Self::from_pass(&map_style::style::LinePass::new(extrusion.key, extrusion.color, map_style::style::LineGeometryStyle::new(Self::FILL_RIBBON_WIDTH)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RoadStructureKind {
    Tunnel = 0,
    Ground = 1,
    AutomobileGround = 2,
    Bridge = 3,
}

impl RoadStructureKind {
    pub const DRAW_ORDER: [RoadStructureKind; 4] = [RoadStructureKind::Tunnel, RoadStructureKind::Ground, RoadStructureKind::AutomobileGround, RoadStructureKind::Bridge];
}

/// The style's road pass roles, in their draw order (`RoadPassRole::ALL`).
pub use map_style::style::RoadPassRole;

/// A polygon of the separate-road path with the keys it sorts by.
#[derive(Debug, Clone)]
pub struct OrderedRoadPolygon {
    pub polygon: ParsedPolygon,
    pub style_key: u8,
    pub structure: RoadStructureKind,
    pub layer: i32,
    pub class_priority: i32,
    pub role: RoadPassRole,
    pub sequence: usize,
}

impl OrderedRoadPolygon {
    fn sort_key(&self) -> (RoadStructureKind, i32, RoadPassRole, i32, u8, usize) {
        (self.structure, self.layer, self.role, self.class_priority, self.style_key, self.sequence)
    }
}

/// Where a line style is drawn: on the ground, or over the roads with the bridges.
pub use map_style::style::LinePlacement;

/// What the reading stage accumulates, and the unification packs. The readers
/// append through the methods here, so the invariants between the buckets
/// hold in one place.
#[derive(Debug, Default)]
pub struct ReadingStageResult {
    pub polygon_by_style: BTreeMap<u8, Vec<ParsedPolygon>>,
    pub ground_is_flattened: bool,
    pub styles: BTreeMap<u8, BakedStyle>,
    pub road_polygon_by_style: BTreeMap<u8, Vec<ParsedPolygon>>,
    pub ordered_road_polygons: Vec<OrderedRoadPolygon>,
    pub road_styles: BTreeMap<u8, BakedStyle>,
    pub bridge_polygon_by_style: BTreeMap<u8, Vec<ParsedPolygon>>,
    pub bridge_styles: BTreeMap<u8, BakedStyle>,
    pub extruded_by_style: BTreeMap<u8, Vec<ParsedExtrudedMesh>>,
    pub keeps_quarters: bool,
    /// The fills that grow trees, gathered as the layers are read and placed
    /// once every layer is in (`trees::reader`).
    pub tree_areas: Vec<crate::trees::scatter::Area>,
    /// The roads of the ground the trees keep off, and the fills no tree stands on.
    pub tree_roads: Vec<crate::trees::scatter::Road>,
    pub tree_excluded_fills: Vec<crate::trees::scatter::Footprint>,
    pub mapped_trees: Vec<crate::trees::scatter::MappedTree>,
    /// The tile's trees, by priority, quarter by quarter on a tile that keeps quarters.
    pub trees: Vec<crate::trees::TreeInstance>,
    /// Where each quarter's trees start, the count last, on a tile that keeps quarters.
    pub tree_quarter_starts: Vec<u32>,
    /// The tile's point labels, the route signs among them.
    pub text_labels: Vec<crate::labels::ParsedTextLabel>,
    /// The texts of the tile's own water-name labels, so the names the
    /// parser adds itself at the coarse zooms skip any the tile already shows.
    pub water_name_texts: std::collections::HashSet<String>,
    pub road_text_labels: Vec<crate::labels::ParsedRoadTextLabel>,
    /// Every place along the tile's signed roads where their route signs
    /// could stand. The tile keeps some of them as labels once every road is
    /// read (`roads::route_shield_placement`).
    pub route_shield_candidates: Vec<crate::roads::line_reader::RouteShieldCandidate>,
    road_sequence: usize,
}

impl ReadingStageResult {
    /// Records the style a ground or bridge key draws with, the first time the key is seen.
    pub fn register_style(&mut self, style: BakedStyle, key: u8, placement: LinePlacement) {
        match placement {
            LinePlacement::Ground => {
                self.styles.entry(key).or_insert(style);
            }
            LinePlacement::BridgeOverlay => {
                self.bridge_styles.entry(key).or_insert(style);
            }
        }
    }

    pub fn register_road_style(&mut self, style: BakedStyle, key: u8) {
        self.road_styles.entry(key).or_insert(style);
    }

    pub fn append_ground(&mut self, polygon: ParsedPolygon, key: u8, placement: LinePlacement) {
        match placement {
            LinePlacement::Ground => self.polygon_by_style.entry(key).or_default().push(polygon),
            LinePlacement::BridgeOverlay => self.bridge_polygon_by_style.entry(key).or_default().push(polygon),
        }
    }

    pub fn append_road(&mut self, polygon: ParsedPolygon, key: u8, structure: RoadStructureKind, layer: i32, class_priority: i32, role: RoadPassRole) {
        self.road_polygon_by_style.entry(key).or_default().push(polygon.clone());
        self.ordered_road_polygons.push(OrderedRoadPolygon { polygon, style_key: key, structure, layer, class_priority, role, sequence: self.road_sequence });
        self.road_sequence += 1;
    }

    pub fn remove_empty_buckets(&mut self) {
        self.polygon_by_style.retain(|_, v| !v.is_empty());
        self.road_polygon_by_style.retain(|_, v| !v.is_empty());
        self.bridge_polygon_by_style.retain(|_, v| !v.is_empty());
        self.extruded_by_style.retain(|_, v| !v.is_empty());
    }
}

/// One drawable layer: the unified streams and the style tables.
#[derive(Debug, Default, Clone)]
pub struct GeometryLayer {
    /// 16 bytes per vertex (the Swift `TileVertexIn`).
    pub vertices: Vec<u8>,
    pub vertex_count: usize,
    pub indices: Vec<u32>,
    pub styles: Vec<[f32; 4]>,
    pub zoom_fades: Vec<[f32; 2]>,
    pub line_styles: Vec<TileLineStyle>,
    /// Ground only: the boundary between fills and ribbons, in index elements.
    pub fills_index_count: Option<usize>,
    /// Road buckets only: where the edge lines start.
    pub edge_line_index_start: Option<usize>,
    /// The style index of each vertex, for the run scan.
    vertex_styles: Vec<u8>,
}

impl GeometryLayer {
    fn push_vertex(&mut self, position: V2, style: u8, distance: i8, parameter: i16, normal: [i8; 2]) {
        self.vertices.extend_from_slice(&position.x.to_le_bytes());
        self.vertices.extend_from_slice(&position.y.to_le_bytes());
        self.vertices.push(style);
        self.vertices.push(distance as u8);
        self.vertices.extend_from_slice(&parameter.to_le_bytes());
        self.vertices.push(normal[0] as u8);
        self.vertices.push(normal[1] as u8);
        self.vertices.extend_from_slice(&[0, 0]);
        self.vertex_styles.push(style);
        self.vertex_count += 1;
    }

    /// Appends one polygon. Attribute-less polygons default to the saturated
    /// line interior, so decorations that share a line style render covered.
    fn append_polygon(&mut self, polygon: &ParsedPolygon, style: u8) {
        debug_assert!(!polygon.is_outline(), "a fill kept as its outline reached the unification unflattened");
        debug_assert!(
            ParsedPolygon::first_clockwise_triangle(&polygon.vertices, &polygon.indices).is_none(),
            "tile geometry must be counter-clockwise in render space"
        );
        let offset = self.vertex_count as u32;
        let attributes = polygon.is_line_ribbon();
        let normals = attributes && polygon.line_normals.len() == polygon.vertices.len();
        for (i, &p) in polygon.vertices.iter().enumerate() {
            self.push_vertex(
                p,
                style,
                if attributes { polygon.line_distances[i] } else { 0 },
                if attributes { polygon.line_parameters[i] } else { i16::MAX },
                if normals { polygon.line_normals[i] } else { [0, 0] },
            );
        }
        self.indices.extend(polygon.indices.iter().map(|i| i.wrapping_add(offset)));
    }

    fn push_style(&mut self, style: &BakedStyle) {
        self.styles.push(style.color);
        self.zoom_fades.push(style.zoom_fade);
        self.line_styles.push(style.line);
    }
}

/// Packs a polygon layer by ascending style key. `split_lines_class` orders
/// the indices fills first, then ribbons, each by ascending style.
fn unify_polygon_layer(polygon_by_style: &BTreeMap<u8, Vec<ParsedPolygon>>, styles: &BTreeMap<u8, BakedStyle>, split_lines_class: bool) -> GeometryLayer {
    let mut layer = GeometryLayer::default();
    let keys: Vec<u8> = polygon_by_style.iter().filter(|(_, v)| !v.is_empty()).map(|(k, _)| *k).collect();
    let index_of: BTreeMap<u8, u8> = keys.iter().enumerate().filter(|(i, _)| *i <= u8::MAX as usize).map(|(i, k)| (*k, i as u8)).collect();
    let sweeps: &[fn(&ParsedPolygon) -> bool] = if split_lines_class { &[|p| !p.is_line_ribbon(), |p| p.is_line_ribbon()] } else { &[|_| true] };
    for (sweep, include) in sweeps.iter().enumerate() {
        if sweep == 1 {
            layer.fills_index_count = Some(layer.indices.len());
        }
        for key in &keys {
            let style = index_of.get(key).copied().unwrap_or(0);
            for polygon in polygon_by_style[key].iter().filter(|p| include(p)) {
                layer.append_polygon(polygon, style);
            }
        }
    }
    for key in &keys {
        if let Some(style) = styles.get(key) {
            layer.push_style(style);
        }
    }
    layer
}

/// One road layer: the style indices handed out by class priority, the lowest
/// first, and the polygons nearest first. The indices are the bodies as
/// triangles, then the edge lines as line segments.
fn unify_ordered_road_layer(sorted: &[&OrderedRoadPolygon], styles: &BTreeMap<u8, BakedStyle>) -> GeometryLayer {
    let mut layer = GeometryLayer::default();
    let mut priority: BTreeMap<u8, i32> = BTreeMap::new();
    for p in sorted {
        let e = priority.entry(p.style_key).or_insert(i32::MIN);
        *e = (*e).max(p.class_priority);
    }
    let mut keys: Vec<u8> = priority.keys().copied().collect();
    keys.sort_by(|a, b| priority[a].cmp(&priority[b]).then(a.cmp(b)));
    let mut index_of: BTreeMap<u8, u8> = BTreeMap::new();
    for (i, key) in keys.iter().enumerate() {
        if i > u8::MAX as usize {
            continue;
        }
        index_of.insert(*key, i as u8);
        if let Some(style) = styles.get(key) {
            layer.push_style(style);
        }
    }
    // Stable: the roads of one style keep the order they were sorted in.
    let mut nearest_first: Vec<(usize, &OrderedRoadPolygon)> = sorted.iter().copied().enumerate().collect();
    nearest_first.sort_by(|(ia, a), (ib, b)| {
        let (sa, sb) = (index_of.get(&a.style_key).copied().unwrap_or(0), index_of.get(&b.style_key).copied().unwrap_or(0));
        sb.cmp(&sa).then(ia.cmp(ib))
    });
    let mut offsets = Vec::with_capacity(nearest_first.len());
    for (_, p) in &nearest_first {
        offsets.push(layer.vertex_count as u32);
        layer.append_polygon(&p.polygon, index_of.get(&p.style_key).copied().unwrap_or(0));
    }
    layer.edge_line_index_start = Some(layer.indices.len());
    for ((_, p), offset) in nearest_first.iter().zip(offsets) {
        layer.indices.extend(p.polygon.edge_line_indices.iter().map(|i| i.wrapping_add(offset)));
    }
    layer
}

/// The building mesh of a tile, unified, with each building's index range and anchor.
#[derive(Debug, Default)]
pub struct ExtrudedLayer {
    /// 12 bytes per vertex (the Swift `ExtrudedVertexIn`), each building's
    /// distinct vertices once.
    pub vertices: Vec<u8>,
    pub vertex_count: usize,
    pub indices: Vec<u32>,
    pub styles: Vec<[f32; 4]>,
    /// (feature id, index start, index count), sorted by id then start.
    pub building_ranges: Vec<(u64, u32, u32)>,
    pub building_anchors: Vec<[f32; 2]>,
    pub quarter_index_starts: Vec<u32>,
}

/// 14.2 fixed point: a quarter-unit step, two tile squares of headroom.
const EXTRUDED_POSITION_SCALE: f32 = 4.0;

fn quantize_position(v: f32) -> i16 {
    clamp_i16(round(v * EXTRUDED_POSITION_SCALE))
}

fn quantize_normal(v: f32) -> i8 {
    round(v * 127.0).clamp(-127, 127) as i8
}

/// Appends a mesh: its vertices as the arena holds them, each distinct one
/// once, and its triangles over them. The builder emits a vertex per face
/// corner, and the faces of a rounded building meet in vertices that agree
/// to the bit (a wall's top edge is the bevel's lower one), so the arena
/// holds each once and the GPU shades it once. Returns how many vertices
/// the mesh added.
#[allow(clippy::too_many_arguments)]
fn write_extruded_mesh(out: &mut Vec<u8>, out_indices: &mut Vec<u32>, first_vertex: u32, vertices: &[ParsedExtrudedVertex], indices: &[u32], style: u8, ordinal: u16, welded: &mut ExtrudedWeld) -> u32 {
    welded.slots.clear();
    welded.remap.clear();
    let mut count = 0u32;
    for v in vertices {
        let mut bytes = [0u8; 16];
        for (axis, c) in [v.position.x, v.position.y, v.position.z].into_iter().enumerate() {
            bytes[axis * 2..axis * 2 + 2].copy_from_slice(&quantize_position(c).to_le_bytes());
        }
        bytes[6..10].copy_from_slice(&[quantize_normal(v.normal.x) as u8, quantize_normal(v.normal.y) as u8, quantize_normal(v.normal.z) as u8, style]);
        bytes[10..12].copy_from_slice(&ordinal.to_le_bytes());
        let index = *welded.slots.entry(u128::from_le_bytes(bytes)).or_insert_with(|| {
            out.extend_from_slice(&bytes[..EXTRUDED_VERTEX_BYTES]);
            count += 1;
            first_vertex + count - 1
        });
        welded.remap.push(index);
    }
    out_indices.extend(indices.iter().map(|&i| welded.remap[i as usize]));
    count
}

const EXTRUDED_VERTEX_BYTES: usize = 12;

/// The scratch of `write_extruded_mesh`, kept across a tile's meshes: a
/// vertex's bytes to its index, and each of the mesh's vertices to its own.
#[derive(Default)]
struct ExtrudedWeld {
    slots: std::collections::HashMap<u128, u32, std::hash::BuildHasherDefault<VertexHasher>>,
    remap: Vec<u32>,
}

/// A multiplicative hash of a vertex's bytes: the keys are a building's own
/// vertices, a few dozen, so the default hasher's guard against chosen keys
/// buys nothing here and costs a seventh of the parse.
#[derive(Default)]
struct VertexHasher(u64);

impl std::hash::Hasher for VertexHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.write_u64(u64::from_le_bytes(word));
        }
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    fn write_u128(&mut self, value: u128) {
        self.write_u64(value as u64);
        self.write_u64((value >> 64) as u64);
    }
}

/// A quarter starts at an even index, so its byte offset in 16-bit indices is a multiple of four.
fn start_quarter(indices: &mut Vec<u32>, starts: &mut Vec<u32>) {
    if indices.len() % 2 == 1 {
        indices.extend_from_slice(&[0, 0, 0]);
    }
    starts.push(indices.len() as u32);
}

fn unify_extruded(extruded_by_style: &BTreeMap<u8, Vec<ParsedExtrudedMesh>>, styles: &BTreeMap<u8, BakedStyle>, keeps_quarters: bool) -> ExtrudedLayer {
    let mut out = ExtrudedLayer::default();
    let keys: Vec<u8> = extruded_by_style.iter().filter(|(_, v)| !v.is_empty()).map(|(k, _)| *k).collect();
    let mut index_of: BTreeMap<u8, u8> = BTreeMap::new();
    for (i, key) in keys.iter().enumerate() {
        if i > u8::MAX as usize {
            continue;
        }
        index_of.insert(*key, i as u8);
        if let Some(style) = styles.get(key) {
            out.styles.push(style.color);
        }
    }
    let passes: Vec<Option<usize>> = if keeps_quarters { (0..quarters::COUNT).map(Some).collect() } else { vec![None] };
    let mut welded = ExtrudedWeld::default();
    let mut vertex_count = 0u32;
    for pass in &passes {
        if pass.is_some() {
            start_quarter(&mut out.indices, &mut out.quarter_index_starts);
        }
        for key in &keys {
            let style = index_of.get(key).copied().unwrap_or(0);
            for mesh in extruded_by_style[key].iter().filter(|m| m.quarter == *pass) {
                let ordinal = out.building_anchors.len().min(u16::MAX as usize) as u16;
                if out.building_anchors.len() <= u16::MAX as usize && !mesh.vertices.is_empty() {
                    let (mut min, mut max) = ([f32::MAX; 2], [f32::MIN; 2]);
                    for v in &mesh.vertices {
                        min = [min[0].min(v.position.x), min[1].min(v.position.y)];
                        max = [max[0].max(v.position.x), max[1].max(v.position.y)];
                    }
                    out.building_anchors.push([(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5]);
                }
                let start = out.indices.len() as u32;
                vertex_count += write_extruded_mesh(&mut out.vertices, &mut out.indices, vertex_count, &mesh.vertices, &mesh.indices, style, ordinal, &mut welded);
                if mesh.building_id != 0 && !mesh.indices.is_empty() {
                    out.building_ranges.push((mesh.building_id, start, mesh.indices.len() as u32));
                }
            }
        }
    }
    if keeps_quarters {
        out.quarter_index_starts.push(out.indices.len() as u32);
    }
    out.building_ranges.sort();
    out.vertex_count = vertex_count as usize;
    out
}

/// Everything a tile's geometry unifies to.
#[derive(Debug, Default)]
pub struct UnifiedTile {
    pub ground: GeometryLayer,
    /// By structure (draw order) then role (draw order).
    pub roads: Vec<GeometryLayer>,
    pub bridge: GeometryLayer,
    pub extruded: ExtrudedLayer,
}

pub fn unify(result: &ReadingStageResult) -> UnifiedTile {
    let ground = unify_polygon_layer(&result.polygon_by_style, &result.styles, true);
    let mut roads: Vec<GeometryLayer> = Vec::with_capacity(20);
    if result.ordered_road_polygons.is_empty() {
        // Roads drawn as plain polygons go into the ground structure's fill.
        let unified = unify_polygon_layer(&result.road_polygon_by_style, &result.road_styles, false);
        for structure in RoadStructureKind::DRAW_ORDER {
            for role in RoadPassRole::ALL {
                roads.push(if structure == RoadStructureKind::Ground && role == RoadPassRole::Fill { unified.clone() } else { GeometryLayer::default() });
            }
        }
    } else {
        let mut buckets: BTreeMap<(RoadStructureKind, RoadPassRole), Vec<&OrderedRoadPolygon>> = BTreeMap::new();
        for p in &result.ordered_road_polygons {
            buckets.entry((p.structure, p.role)).or_default().push(p);
        }
        for structure in RoadStructureKind::DRAW_ORDER {
            for role in RoadPassRole::ALL {
                let mut bucket = buckets.remove(&(structure, role)).unwrap_or_default();
                bucket.sort_by_key(|p| p.sort_key());
                roads.push(unify_ordered_road_layer(&bucket, &result.road_styles));
            }
        }
    }
    let bridge = unify_polygon_layer(&result.bridge_polygon_by_style, &result.bridge_styles, false);
    let extruded = unify_extruded(&result.extruded_by_style, &result.styles, result.keeps_quarters);
    UnifiedTile { ground, roads, bridge, extruded }
}

// MARK: The arena

/// One span of the arena, as the renderer reads it.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArenaSpan {
    pub byte_offset: usize,
    pub byte_count: usize,
    pub element_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_width: Option<u8>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroundStyleRun {
    pub index_start: u32,
    pub index_count: u32,
    pub zoom_fade: [f32; 2],
    pub flags: u32,
}

pub const RUN_ALPHA_OPAQUE: u32 = 1;
pub const RUN_LINES_CLASS: u32 = 2;
pub const RUN_FLATTENED: u32 = 4;
pub const RUN_SPHERE_VEILED: u32 = 8;

#[derive(Debug, Clone, Copy, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TextRunCounts {
    pub glyph_run_count: usize,
    pub poi_icon_run_count: usize,
    pub route_shield_run_count: usize,
}

/// The tile's CPU metadata beside its arena (`TileArenaMeta` on the TS side).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TileArenaMeta {
    pub kind: &'static str,
    pub format_version: u32,
    pub spans: Vec<ArenaSpan>,
    pub text_run_counts: TextRunCounts,
    pub ground_style_runs: Vec<GroundStyleRun>,
    pub road_edge_line_index_starts: Vec<usize>,
    /// Feature ids as decimal strings: a u64 does not fit a JS number.
    pub building_ranges: Vec<(String, u32, u32)>,
    pub building_anchors: Vec<[f32; 2]>,
    pub building_quarter_index_starts: Vec<u32>,
    pub tree_quarter_starts: Vec<u32>,
    /// The labels' records and run styles, their vertices the label slots.
    pub labels: TileLabelsMeta,
}

struct ArenaWriter {
    bytes: Vec<u8>,
    spans: Vec<ArenaSpan>,
}

impl ArenaWriter {
    fn align(&mut self) {
        let aligned = self.bytes.len().div_ceil(ARENA_SPAN_ALIGNMENT) * ARENA_SPAN_ALIGNMENT;
        self.bytes.resize(aligned, 0);
    }

    fn span(&mut self, data: &[u8], element_count: usize, index_width: Option<u8>) {
        self.align();
        self.spans.push(ArenaSpan { byte_offset: self.bytes.len(), byte_count: data.len(), element_count, index_width });
        self.bytes.extend_from_slice(data);
    }

    fn floats<const N: usize>(&mut self, values: &[[f32; N]]) {
        let mut data = Vec::with_capacity(values.len() * N * 4);
        for v in values {
            for c in v {
                data.extend_from_slice(&c.to_le_bytes());
            }
        }
        self.span(&data, values.len(), None);
    }

    fn line_styles(&mut self, values: &[TileLineStyle]) {
        let mut data = Vec::with_capacity(values.len() * 68);
        for s in values {
            for c in [
                s.width_points,
                s.dash_length_points,
                s.dash_gap_points,
                s.edge_threshold,
                s.minimum_width_points,
                s.dash_in_tile_units,
                s.half_width_units,
                s.world_lock_zoom,
                s.ramp_start_width_points,
                s.ramp_start_zoom,
                s.ramp_end_zoom,
                s.ramp_start_alpha,
                s.sphere_alpha,
                s.sphere_color[0],
                s.sphere_color[1],
                s.sphere_color[2],
                s.sphere_look_until_zoom,
            ] {
                data.extend_from_slice(&c.to_le_bytes());
            }
        }
        self.span(&data, values.len(), None);
    }

    /// The 16-bit narrowing, decided once here: vertex counts up to 65535 keep
    /// every narrowed index below 0xFFFF, the strip-restart sentinel.
    fn indices(&mut self, indices: &[u32], vertex_count: usize) {
        if !indices.is_empty() && vertex_count <= u16::MAX as usize && indices.iter().all(|&i| i < u16::MAX as u32) {
            let mut data = Vec::with_capacity(indices.len() * 2);
            for &i in indices {
                data.extend_from_slice(&(i as u16).to_le_bytes());
            }
            self.span(&data, indices.len(), Some(2));
        } else {
            let mut data = Vec::with_capacity(indices.len() * 4);
            for &i in indices {
                data.extend_from_slice(&i.to_le_bytes());
            }
            self.span(&data, indices.len(), Some(4));
        }
    }

    fn label_vertices(&mut self, vertices: &[LabelVertex]) {
        let mut data = Vec::with_capacity(vertices.len() * LabelVertex::STRIDE);
        for v in vertices {
            v.write(&mut data);
        }
        self.span(&data, vertices.len(), None);
    }

    fn geometry_layer(&mut self, layer: &GeometryLayer) {
        self.span(&layer.vertices, layer.vertex_count, None);
        self.indices(&layer.indices, layer.vertex_count);
        self.floats(&layer.styles);
        self.floats(&layer.zoom_fades);
        self.line_styles(&layer.line_styles);
    }
}

/// The ground's per-style, per-class runs. The contract is two class
/// segments, fills up to `fills_index_count` and ribbons to the end, each one
/// contiguous run per style in ascending order. A violation falls back to one
/// ribbons-class run over everything, which draws as the unsplit path.
pub fn scan_ground_runs(ground: &GeometryLayer, flattened: bool) -> Vec<GroundStyleRun> {
    if ground.indices.is_empty() {
        return Vec::new();
    }
    let boundary = ground.fills_index_count.unwrap_or(0).min(ground.indices.len());
    let fallback = || vec![GroundStyleRun { index_start: 0, index_count: ground.indices.len() as u32, zoom_fade: [0.0, 0.0], flags: RUN_LINES_CLASS }];
    if !boundary.is_multiple_of(3) || !(ground.indices.len() - boundary).is_multiple_of(3) {
        return fallback();
    }
    let scan = |start: usize, end: usize, class: u32| -> Option<Vec<GroundStyleRun>> {
        let mut runs = Vec::new();
        let (mut run_start, mut run_style) = (start, -1i32);
        let make = |style: i32, start: usize, count: usize| {
            let s = style as usize;
            let mut flags = class;
            if ground.styles.get(s).is_some_and(|c| c[3] >= 1.0) {
                flags |= RUN_ALPHA_OPAQUE;
            }
            if class & RUN_LINES_CLASS != 0 && ground.line_styles.get(s).is_some_and(|l| l.sphere_alpha < 1.0) {
                flags |= RUN_SPHERE_VEILED;
            }
            GroundStyleRun { index_start: start as u32, index_count: count as u32, zoom_fade: ground.zoom_fades.get(s).copied().unwrap_or([0.0, 0.0]), flags }
        };
        let mut t = start;
        while t + 2 < end {
            let style = *ground.vertex_styles.get(ground.indices[t] as usize)? as i32;
            if style != run_style {
                if style < run_style {
                    return None;
                }
                if run_style >= 0 {
                    runs.push(make(run_style, run_start, t - run_start));
                }
                run_style = style;
                run_start = t;
            }
            t += 3;
        }
        if run_style >= 0 {
            runs.push(make(run_style, run_start, end - run_start));
        }
        Some(runs)
    };
    match (scan(0, boundary, if flattened { RUN_FLATTENED } else { 0 }), scan(boundary, ground.indices.len(), RUN_LINES_CLASS)) {
        (Some(mut fills), Some(ribbons)) => {
            fills.extend(ribbons);
            fills
        }
        _ => fallback(),
    }
}

/// The arena of a unified tile and its labels, in the canonical slot order,
/// and its metadata.
pub fn write_arena(unified: &UnifiedTile, ground_is_flattened: bool, trees: &[u8], tree_quarter_starts: &[u32], labels: PreparedLabels) -> (Vec<u8>, TileArenaMeta) {
    let mut w = ArenaWriter { bytes: Vec::new(), spans: Vec::new() };
    w.geometry_layer(&unified.ground);
    for layer in &unified.roads {
        w.geometry_layer(layer);
    }
    w.geometry_layer(&unified.bridge);
    let ex = &unified.extruded;
    w.span(&ex.vertices, ex.vertex_count, None);
    w.indices(&ex.indices, ex.vertex_count);
    w.floats(&ex.styles);
    w.span(trees, trees.len() / 16, None);
    let (label_vertices, labels) = labels.split();
    let text_run_counts = TextRunCounts {
        glyph_run_count: label_vertices.glyph_runs.len(),
        poi_icon_run_count: label_vertices.poi_icon_runs.len(),
        route_shield_run_count: label_vertices.route_shield_runs.len(),
    };
    for run in label_vertices.glyph_runs.iter().chain(&label_vertices.poi_icon_runs).chain(&label_vertices.route_shield_runs) {
        w.label_vertices(run);
    }
    w.label_vertices(&label_vertices.road_glyphs);
    w.label_vertices(&label_vertices.surface);
    w.align();
    let meta = TileArenaMeta {
        kind: "arena",
        format_version: PREPARED_FORMAT_VERSION,
        spans: w.spans,
        text_run_counts,
        ground_style_runs: scan_ground_runs(&unified.ground, ground_is_flattened),
        road_edge_line_index_starts: unified.roads.iter().map(|l| l.edge_line_index_start.unwrap_or(l.indices.len())).collect(),
        building_ranges: ex.building_ranges.iter().map(|(id, s, c)| (id.to_string(), *s, *c)).collect(),
        building_anchors: ex.building_anchors.clone(),
        building_quarter_index_starts: ex.quarter_index_starts.clone(),
        tree_quarter_starts: tree_quarter_starts.to_vec(),
        labels,
    };
    (w.bytes, meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(alpha: f32) -> BakedStyle {
        BakedStyle { color: [1.0, 0.0, 0.0, alpha], zoom_fade: [0.0, 0.0], line: TileLineStyle { sphere_alpha: 1.0, ..Default::default() } }
    }

    fn quad(min: f32, max: f32) -> ParsedPolygon {
        ParsedPolygon::new(vec![V2::new(min, min), V2::new(max, min), V2::new(max, max), V2::new(min, max)], vec![0, 1, 2, 0, 2, 3])
    }

    #[test]
    fn the_ground_splits_fills_and_ribbons_and_scans_into_runs() {
        let mut r = ReadingStageResult::default();
        r.register_style(style(1.0), 3, LinePlacement::Ground);
        r.register_style(style(0.5), 7, LinePlacement::Ground);
        r.append_ground(quad(0.0, 10.0), 3, LinePlacement::Ground);
        let mut ribbon = quad(20.0, 30.0);
        ribbon.make_line_class_fill();
        r.append_ground(ribbon, 3, LinePlacement::Ground);
        r.append_ground(quad(40.0, 50.0), 7, LinePlacement::Ground);
        let u = unify(&r);
        assert_eq!(u.ground.vertex_count, 12);
        assert_eq!(u.ground.fills_index_count, Some(12));
        let runs = scan_ground_runs(&u.ground, false);
        assert_eq!(runs.len(), 3);
        assert_eq!((runs[0].index_start, runs[0].index_count, runs[0].flags), (0, 6, RUN_ALPHA_OPAQUE));
        assert_eq!((runs[1].index_start, runs[1].flags), (6, 0));
        assert_eq!((runs[2].index_start, runs[2].flags), (12, RUN_LINES_CLASS | RUN_ALPHA_OPAQUE));
    }

    #[test]
    fn roads_rank_by_class_and_go_nearest_first_with_edge_lines_last() {
        let mut r = ReadingStageResult::default();
        r.register_road_style(style(1.0), 1);
        r.register_road_style(style(1.0), 2);
        let mut road = quad(0.0, 10.0);
        road.edge_line_indices = vec![0, 1];
        r.append_road(road.clone(), 2, RoadStructureKind::Ground, 0, 10, RoadPassRole::Fill);
        r.append_road(road, 1, RoadStructureKind::Ground, 0, 20, RoadPassRole::Fill);
        let u = unify(&r);
        let fill = &u.roads[5 + 2];
        assert_eq!(fill.vertex_count, 8);
        // Key 2 has the lower priority: style index 0. Key 1: index 1, drawn first (nearest).
        assert_eq!(fill.vertex_styles[0], 1);
        assert_eq!(fill.edge_line_index_start, Some(12));
        assert_eq!(&fill.indices[12..], &[0, 1, 4, 5]);
    }

    #[test]
    fn the_arena_is_aligned_in_slot_order_and_narrows_indices() {
        let mut r = ReadingStageResult::default();
        r.register_style(style(1.0), 1, LinePlacement::Ground);
        r.append_ground(quad(0.0, 10.0), 1, LinePlacement::Ground);
        let (bytes, meta) = write_arena(&unify(&r), false, &[], &[], PreparedLabels::default());
        // 22 geometry layers of 5 spans, 3 extruded, the trees and the two label spans.
        assert_eq!(meta.spans.len(), 22 * 5 + 3 + 1 + 2);
        assert!(meta.spans.iter().all(|s| s.byte_offset % ARENA_SPAN_ALIGNMENT == 0));
        assert_eq!(meta.spans[0].byte_count, 4 * 16);
        assert_eq!(meta.spans[1].index_width, Some(2));
        assert_eq!(meta.spans[4].byte_count, 68);
        assert_eq!(bytes.len() % ARENA_SPAN_ALIGNMENT, 0);
        let x = f32::from_le_bytes(bytes[16..20].try_into().unwrap());
        assert_eq!(x, 10.0, "the second vertex's x");
        assert_eq!(bytes[24], 0, "its style index");
        assert_eq!(i16::from_le_bytes([bytes[26], bytes[27]]), i16::MAX, "a fill saturates the parameter");
    }
    fn building(min: f32, max: f32, radius: f32, id: u64, quarter: Option<usize>) -> ParsedExtrudedMesh {
        let ring = vec![V2::new(min, min), V2::new(max, min), V2::new(max, max), V2::new(min, max)];
        let roof = ParsedPolygon::counter_clockwise_convex_fan(ring.clone());
        let mut mesh = crate::buildings::mesh_builder::build(&ring, &[], &roof, 0.0, 120.0, radius, 4096.0, quarter).unwrap();
        mesh.building_id = id;
        mesh.quarter = quarter;
        mesh
    }

    /// A vertex of the arena: its position in quarter units, its normal and its style and ordinal bytes.
    fn arena_vertex(ex: &ExtrudedLayer, index: u32) -> [u8; 12] {
        ex.vertices[index as usize * 12..index as usize * 12 + 12].try_into().unwrap()
    }

    #[test]
    fn a_building_holds_each_distinct_vertex_once_and_keeps_its_triangles() {
        let mut r = ReadingStageResult::default();
        r.register_style(style(1.0), 40, LinePlacement::Ground);
        let rounded = building(100.0, 300.0, 10.0, 7, None);
        r.extruded_by_style.insert(40, vec![rounded.clone(), building(100.0, 300.0, 10.0, 8, None)]);
        let ex = unify(&r).extruded;
        // Every triangle corner is the vertex the builder gave it, quantized.
        assert_eq!(ex.indices.len(), rounded.indices.len() * 2);
        for (corner, &index) in rounded.indices.iter().enumerate() {
            let v = rounded.vertices[index as usize];
            let bytes = arena_vertex(&ex, ex.indices[corner]);
            let position = [0, 2, 4].map(|at| i16::from_le_bytes([bytes[at], bytes[at + 1]]));
            assert_eq!(position, [v.position.x, v.position.y, v.position.z].map(quantize_position));
            assert_eq!([bytes[6] as i8, bytes[7] as i8, bytes[8] as i8], [v.normal.x, v.normal.y, v.normal.z].map(quantize_normal));
        }
        // No two vertices of a building are alike, and none is left unread.
        let per_building = ex.vertex_count / 2;
        let distinct: std::collections::BTreeSet<[u8; 12]> = (0..per_building as u32).map(|i| arena_vertex(&ex, i)).collect();
        assert_eq!(distinct.len(), per_building);
        assert!(per_building < rounded.vertices.len());
        let read: std::collections::BTreeSet<u32> = ex.indices.iter().copied().collect();
        assert_eq!(read.len(), ex.vertex_count);
        assert_eq!(ex.vertices.len(), ex.vertex_count * 12);
        // The second building, the same footprint, shares nothing with the first: its vertices carry its own ordinal.
        assert!(ex.indices[rounded.indices.len()..].iter().all(|&i| i as usize >= per_building));
    }
}
