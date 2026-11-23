// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Turns the bytes of one vector tile into the prepared tile (a port of the
//! Swift `TileMvtParser`): the reading stage walks the layers, resolves every
//! feature's attributes, facts and style once and hands each feature to the
//! reader for its geometry kind, then the unification packs the result into
//! the GPU arena. One parser serves every parse of a map: it holds nothing
//! per tile.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use map_style::adaptation::TileLabelDecisions;
use map_style::style::{FeatureStyle, LinePass, LinePlacement, MapStyleRuntime};
use mvt::GeometryType;
use mvt::geometry::Polygon;
use text_layout::TextLayout;

use crate::buildings::candidate::{BuildingExtrusionCandidate, BuildingRoofLookup};
use crate::buildings::reader::{BuildingFeatureReader, PartInfo};
use crate::coordinate_space::TILE_EXTENT;
use crate::ground::GroundFeatureReader;
use crate::ground_flattening;
use crate::ground_subdivider;
use crate::labels::label_reader::LabelFeatureReader;
use crate::options::TileParseOptions;
use crate::parse_polygon::ParsePolygon;
use crate::prepared_labels;
use crate::reading::{PreparedLayer, prepare_layers};
use crate::roads::layer_precomputation::RoadLayerContext;
use crate::roads::line_reader::LineFeatureReader;
use crate::tile::{Tile, quarters};
use crate::trees::reader::TreeFeatureReader;
use crate::trees::scatter::Footprint;
use crate::unification::{BakedStyle, ReadingStageResult, TileArenaMeta, unify, write_arena};

/// A parsed tile: the arena the GPU takes as it is, and its metadata.
pub struct PreparedTile {
    pub arena: Vec<u8>,
    pub meta: TileArenaMeta,
    pub result: ReadingStageResult,
}

/// The parser owns the style, the label policy, the options a parse reads
/// and the text layout the labels are baked with. The feature readers hold
/// nothing across tiles and are made per parse.
pub struct TileMvtParser {
    style: MapStyleRuntime,
    decisions: TileLabelDecisions,
    options: TileParseOptions,
    text: Arc<TextLayout>,
}

struct Readers<'s> {
    labels: LabelFeatureReader<'s>,
    lines: LineFeatureReader<'s>,
    ground: GroundFeatureReader<'s>,
    buildings: BuildingFeatureReader,
    trees: TreeFeatureReader,
}

impl TileMvtParser {
    /// A parser that lays its labels out with the package's atlases (`text_layout::bundled`).
    pub fn new(style: MapStyleRuntime, decisions: TileLabelDecisions, options: TileParseOptions) -> Self {
        Self::with_text_layout(style, decisions, options, text_layout::bundled())
    }

    pub fn with_text_layout(style: MapStyleRuntime, decisions: TileLabelDecisions, options: TileParseOptions, text: Arc<TextLayout>) -> Self {
        TileMvtParser { style, decisions, options, text }
    }

    pub fn text_layout(&self) -> &TextLayout {
        &self.text
    }

    pub fn style(&self) -> &MapStyleRuntime {
        &self.style
    }

    pub fn options(&self) -> &TileParseOptions {
        &self.options
    }

    fn readers(&self) -> Readers<'_> {
        Readers {
            labels: LabelFeatureReader::new(&self.decisions, &self.style),
            lines: LineFeatureReader::new(&self.decisions, &self.options),
            ground: GroundFeatureReader::new(&self.style),
            buildings: BuildingFeatureReader::new(&self.options),
            trees: TreeFeatureReader::new(&self.options),
        }
    }

    /// The reading, the label layout and the unification of one tile (the
    /// Swift `TilePreparedDataBuilder.build`). A tile the decoder cannot read is an error.
    pub fn parse(&self, tile: Tile, mvt_bytes: &[u8]) -> Result<PreparedTile, mvt::DecodeError> {
        let decoded = mvt::Tile::decode(mvt_bytes)?;
        let mut result = self.reading_stage(&decoded, tile);
        if self.options.flattens_ground(tile.z) {
            let polygons = std::mem::take(&mut result.polygon_by_style);
            result.polygon_by_style = ground_flattening::flatten(polygons, self.options.ground_flattening_grid_for(tile.z));
            result.ground_is_flattened = true;
        }
        if let Some(step) = self.options.relief_subdivision_step(tile.z) {
            // The bridge decks lie on the relief like the ground: cut at the grid.
            let step = step.round() as i32;
            for polygons in result.bridge_polygon_by_style.values_mut() {
                for p in polygons.iter_mut() {
                    *p = ground_subdivider::subdivide(p, step);
                }
            }
        }
        let unified = unify(&result);
        let mut trees = Vec::with_capacity(result.trees.len() * 16);
        for t in &result.trees {
            t.write(&mut trees);
        }
        let labels = prepared_labels::build(&self.text, &result, tile.z);
        let (arena, meta) = write_arena(&unified, result.ground_is_flattened, &trees, &result.tree_quarter_starts, labels);
        Ok(PreparedTile { arena, meta, result })
    }

    /// One pass over the layers, then the synthesized labels, the ground's
    /// background and subdivision, the building meshes and the trees.
    pub fn reading_stage(&self, decoded: &mvt::Tile<'_>, tile: Tile) -> ReadingStageResult {
        let r = self.readers();
        let mut result = ReadingStageResult::default();
        let mut parse_polygon = ParsePolygon::new();
        let mut candidates: Vec<BuildingExtrusionCandidate> = Vec::new();
        // The ids of the building outlines the tile carries, raised or not.
        let mut outline_ids: HashSet<u64> = HashSet::new();
        for layer in prepare_layers(decoded, tile, &self.style) {
            let parts = r.buildings.part_info(&layer);
            let roads = RoadLayerContext::build(&layer, tile);
            for (index, f) in layer.features.iter().enumerate() {
                match (&f.style, f.feature.geometry_type) {
                    (FeatureStyle::Hidden, _) => {}
                    (_, GeometryType::Polygon) => {
                        self.read_polygons(&r, &layer, index, &parts, tile, &mut parse_polygon, &mut candidates, &mut outline_ids, &mut result);
                    }
                    (style, GeometryType::LineString) => {
                        if let FeatureStyle::Road(road) = style {
                            r.trees.collect_road(&layer.geometry.lines(&f.feature), road, tile, &mut result);
                        }
                        r.lines.read(&layer, index, tile, &roads, &mut result);
                    }
                    (FeatureStyle::Tree(tree), GeometryType::Point) => {
                        // A mapped tree stands where it is, labels or not.
                        r.trees.collect_points(&layer.geometry.points(&f.feature), tree, tile, &mut result);
                    }
                    (FeatureStyle::PointLabel(label), GeometryType::Point) if self.options.labels_enabled => {
                        r.labels.read(&layer, index, label, tile, &mut result);
                    }
                    _ => {}
                }
            }
        }
        if self.options.labels_enabled {
            r.lines.append_route_shield_labels(tile, &mut result);
            r.labels.append_low_zoom_water_labels(tile, &mut result);
        }
        r.ground.finish(tile, self.options.add_test_borders, &mut result);
        result.keeps_quarters = self.options.keeps_quarters(tile.z);
        // Only once every layer is read: an outline can come after its parts.
        let volumes = r.buildings.append_extruded_meshes(&candidates, &mut result);
        // Every building's own top by id, before the resolver clamps or drops
        // volumes: what a label naming that building rises to.
        let mut tops: HashMap<u64, f32> = HashMap::new();
        for c in &candidates {
            let top = tops.entry(c.building_id).or_insert(0.0);
            *top = top.max(c.top_height);
        }
        lift_labels_onto_roofs(&volumes, &tops, &outline_ids, &mut result);
        // The trees keep off the footprints the tile raises, the roads and the fills that stand none.
        let footprints = volumes.iter().map(|v| Footprint { exterior: v.clipped_exterior.clone(), interiors: v.clipped_interiors.clone() }).collect();
        r.trees.finish(tile, footprints, &mut result);
        result.remove_empty_buckets();
        result
    }

    /// A polygon feature by the case of its style: a fill (with the ocean
    /// split), a building (the footprint as a fill, plus the extrusion), a
    /// road's fill stroke, a line whose style fills areas.
    #[allow(clippy::too_many_arguments)]
    fn read_polygons(
        &self,
        r: &Readers<'_>,
        layer: &PreparedLayer<'_>,
        index: usize,
        parts: &PartInfo,
        tile: Tile,
        parse: &mut ParsePolygon,
        candidates: &mut Vec<BuildingExtrusionCandidate>,
        outline_ids: &mut HashSet<u64>,
        result: &mut ReadingStageResult,
    ) {
        let f = &layer.features[index];
        let polygons = layer.geometry.polygons(&f.feature);
        match &f.style {
            FeatureStyle::Fill(fill) => {
                result.register_style(BakedStyle::from_fill(fill), fill.key, LinePlacement::Ground);
                r.trees.collect_fill(&polygons, fill, tile, result);
                // A fill the flattening takes stays an outline. A fill drawn
                // among the ground lines is tessellated like a ribbon.
                let as_outline = self.options.flattens_ground(tile.z) && !fill.draws_among_ground_lines;
                let splits = r.ground.splits_complex_ocean_holes(fill, &polygons);
                for polygon in &polygons {
                    if splits && r.ground.append_complex_ocean_polygon(polygon, fill, as_outline, result, parse, tile) {
                        continue;
                    }
                    if as_outline {
                        if let Some(outline) = parse.parse_outline(polygon, TILE_EXTENT) {
                            result.append_ground(outline, fill.key, LinePlacement::Ground);
                        }
                        continue;
                    }
                    let Some(geometry) = parse.parse_geometry(polygon, TILE_EXTENT) else { continue };
                    let mut parsed = geometry.parsed;
                    if fill.draws_among_ground_lines {
                        parsed.make_line_class_fill();
                    }
                    result.append_ground(parsed, fill.key, LinePlacement::Ground);
                }
            }
            FeatureStyle::Extrusion(extrusion) => {
                result.register_style(BakedStyle::from_extrusion(extrusion), extrusion.key, LinePlacement::Ground);
                let building = f.facts.building();
                let info = r.buildings.extrusion(f.id(), building, extrusion, &polygons, parts, tile);
                // Every outline, raised or not: a label can name an outline whose volume is its parts.
                if tile.z >= self.options.building_minimum_source_zoom && building.is_some_and(|b| !b.is_part) && f.id() != 0 {
                    outline_ids.insert(f.id());
                }
                let keeps_quarters = self.options.keeps_quarters(tile.z);
                for polygon in &polygons {
                    let Some(geometry) = parse.parse_geometry(polygon, TILE_EXTENT) else { continue };
                    let Some(info) = info else {
                        result.append_ground(geometry.parsed, extrusion.key, LinePlacement::Ground);
                        continue;
                    };
                    if !keeps_quarters {
                        if let Some(c) = r.buildings.candidate(polygon, &geometry, extrusion.key, &info, None) {
                            candidates.push(c);
                        }
                        result.append_ground(geometry.parsed, extrusion.key, LinePlacement::Ground);
                        continue;
                    }
                    result.append_ground(geometry.parsed, extrusion.key, LinePlacement::Ground);
                    // A tile kept by quarters cuts each volume at the quarters' edges.
                    for q in 0..quarters::COUNT {
                        if let Some(piece) = parse.parse_geometry_within(polygon, quarters::square(q))
                            && let Some(c) = r.buildings.candidate(polygon, &piece, extrusion.key, &info, Some(q))
                        {
                            candidates.push(c);
                        }
                    }
                }
            }
            FeatureStyle::Road(road) => {
                if let Some(fill) = &road.fill {
                    self.append_ground_polygons(&polygons, fill, road.placement, tile, parse, result);
                }
            }
            // A line style on areal geometry is filled only when the style says so.
            FeatureStyle::Line(line) if line.fills_areas => self.append_ground_polygons(&polygons, &line.pass, line.placement, tile, parse, result),
            _ => {}
        }
    }

    /// Polygons filled with a stroke's colour. On a ground the flattening
    /// takes they stay outlines. The bridge overlay is never flattened.
    fn append_ground_polygons(&self, polygons: &[Polygon], pass: &LinePass, placement: LinePlacement, tile: Tile, parse: &mut ParsePolygon, result: &mut ReadingStageResult) {
        result.register_style(BakedStyle::from_pass(pass), pass.key, placement);
        let as_outline = placement == LinePlacement::Ground && self.options.flattens_ground(tile.z);
        for polygon in polygons {
            if as_outline {
                if let Some(outline) = parse.parse_outline(polygon, TILE_EXTENT) {
                    result.append_ground(outline, pass.key, placement);
                }
                continue;
            }
            if let Some(geometry) = parse.parse_geometry(polygon, TILE_EXTENT) {
                result.append_ground(geometry.parsed, pass.key, placement);
            }
        }
    }
}

/// The labels whose style stands them in buildings take the roof over their
/// anchor. Only the label naming the building itself rises to it, and to
/// that building's own top. A label naming an outline the tile raises only
/// by its parts rises to the parts' roof over its anchor.
fn lift_labels_onto_roofs(volumes: &[BuildingExtrusionCandidate], tops: &HashMap<u64, f32>, outline_ids: &HashSet<u64>, result: &mut ReadingStageResult) {
    if volumes.is_empty() || !result.text_labels.iter().any(|l| l.stands_on_roof) {
        return;
    }
    let mut roofs: Option<BuildingRoofLookup> = None;
    for label in result.text_labels.iter_mut().filter(|l| l.stands_on_roof) {
        let position = label.position;
        let (height, lifts) = label_roof(label.feature_id, tops, outline_ids, || roofs.get_or_insert_with(|| BuildingRoofLookup::new(volumes)).roof_height(position));
        label.roof_height = height;
        label.lifts_to_roof = lifts;
    }
}

/// The roof a label in a building takes and whether it draws on it.
pub fn label_roof(feature_id: Option<u64>, tops: &HashMap<u64, f32>, outline_ids: &HashSet<u64>, roof_over_anchor: impl FnOnce() -> f32) -> (f32, bool) {
    if let Some(id) = feature_id {
        if let Some(&top) = tops.get(&id)
            && top > 0.0
        {
            return (top, true);
        }
        if outline_ids.contains(&id) {
            let roof = roof_over_anchor();
            return (roof, roof > 0.0);
        }
    }
    (0.0, false)
}
