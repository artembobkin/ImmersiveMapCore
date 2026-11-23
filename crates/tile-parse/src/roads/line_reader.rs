// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Reads a line feature into the ribbons its style's passes draw, the
//! decoration stamped along it (the oneway arrows), its road name label, and
//! the places its route signs could stand. A port of the Swift
//! `LineFeatureReader`.
//!
//! On the road layer at street zooms the feature takes the separate-road
//! path: its lines come stitched from the layer's `RoadLayerPrecomputation`,
//! its ribbons join the road phases sorted by structure and class, and ends
//! that continue into a neighbouring tile or another road stay hard. Every
//! other line (a boundary, a waterway, a road at an overview zoom) draws as
//! ground or bridge geometry clipped to the tile with no junction knowledge.
//!
//! Stateless across tiles.

use map_style::adaptation::TileLabelDecisions;
use map_style::schema::RoadFacts;
use map_style::style::{LabelTextStyle, RoadDecorationKind, RoadPass, RoadPassRole, RoadStyle, RouteShield, RouteShieldStyle};

use super::layer_precomputation::{PreparedRoadLine, RoadLayerContext, RoadLayerPrecomputation, float_points};
use super::route_shield_placement::{self, ShieldCandidate};
use super::{RoadConnectionPointKey, dash_pattern, direction_arrows};
use crate::coordinate_space::TILE_EXTENT;
use crate::geom::{V2, clamp_i16};
use crate::ground_subdivider;
use crate::labels::{ParsedRoadTextLabel, ParsedTextLabel};
use crate::options::TileParseOptions;
use crate::parse_line::{ClippedLineFragment, LineClipper, LineOptions, ParseLine};
use crate::reading::{PreparedLayer, to_style_tile};
use crate::tile::Tile;
use crate::unification::{BakedStyle, ReadingStageResult, RoadStructureKind};

/// A road label on a fragment cut by the tile edge needs this much line
/// inside the tile, or the name is left to the neighbour.
const MIN_CLIPPED_ROAD_LABEL_FRAGMENT_LENGTH: f32 = 256.0;

/// One place along a signed road where its route signs could stand.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteShieldCandidate {
    pub shields: Vec<RouteShield>,
    pub style: RouteShieldStyle,
    /// The key of the road's style, the key the numbers' glyph runs are
    /// grouped under.
    pub style_key: i32,
    pub position: V2,
    /// The signs' identity within the tile: two roads carrying the same
    /// numbers (the two carriageways of a motorway, the pieces the tile cut
    /// a route into) share it and are spaced as one.
    pub group: String,
}

impl ShieldCandidate for RouteShieldCandidate {
    fn group(&self) -> &str {
        &self.group
    }

    fn position(&self) -> V2 {
        self.position
    }

    fn spacing_points(&self) -> f32 {
        self.style.spacing_points
    }
}

pub struct LineFeatureReader<'s> {
    label_decisions: &'s TileLabelDecisions,
    options: &'s TileParseOptions,
    clipper: LineClipper,
    parse_line: ParseLine,
}

impl<'s> LineFeatureReader<'s> {
    pub fn new(label_decisions: &'s TileLabelDecisions, options: &'s TileParseOptions) -> LineFeatureReader<'s> {
        LineFeatureReader { label_decisions, options, clipper: LineClipper::new(), parse_line: ParseLine::new() }
    }

    pub fn read(&self, layer: &PreparedLayer<'_>, feature_index: usize, tile: Tile, roads: &RoadLayerContext, result: &mut ReadingStageResult) {
        let read = &layer.features[feature_index];
        // A line or a road. Every other style draws no line geometry. A plain
        // line is a road of one fill stroke with no class.
        let Some(style) = read.style.road_style() else { return };
        let passes: Vec<RoadPass> = style.ordered_passes().into_iter().filter(|p| p.pass.line_geometry.line_width > 0.0).collect();
        if passes.is_empty() {
            return;
        }
        let separate = roads.uses_separate_road_rendering;
        let precomputation = &roads.precomputation;
        let labels_enabled = self.options.labels_enabled;

        // Labels off: no road name is resolved or baked. The switch is
        // prepared-cache identity, so a tile prepared without labels never
        // answers a map that wants them.
        let road_label_style: Option<&LabelTextStyle> = style.label.as_ref();
        let label_text = if labels_enabled { self.label_decisions.road_label_text(read.facts.label(), road_label_style) } else { None };
        let route_shields: Vec<RouteShield> = if labels_enabled {
            style.shields.as_ref().map(|s| self.label_decisions.renderable_route_shields(&s.shields).into_iter().cloned().collect()).unwrap_or_default()
        } else {
            Vec::new()
        };
        let ground = RoadFacts::ground();
        let road = read.facts.road().unwrap_or(&ground);
        let structure = RoadStructureKind::new(style.level, style.tier);
        let road_layer = road.layer as i32;
        let shared_padding = passes.iter().fold(0.0f64, |partial, p| partial.max(p.pass.line_geometry.line_width * 0.5)) as f32;

        let converted: Vec<PreparedRoadLine>;
        let prepared_lines: &[PreparedRoadLine] = if separate {
            precomputation.lines_by_feature_index.get(feature_index).map_or(&[], Vec::as_slice)
        } else {
            converted = layer
                .geometry
                .lines(&read.feature)
                .iter()
                .map(|line| {
                    let points = float_points(line);
                    let exact_fragments = self.clipper.clip(&points, TILE_EXTENT, 0.0);
                    PreparedRoadLine { points, exact_fragments }
                })
                .collect();
            &converted
        };

        let subdivision_step = self.options.relief_subdivision_step(tile.z);
        let deferred_extrusion = separate && ground_subdivider::step_for_tile_zoom(tile.z).is_none();

        for prepared in prepared_lines {
            let line_points = &prepared.points;
            let exact_fragments = &prepared.exact_fragments;
            if exact_fragments.is_empty() {
                continue;
            }
            let shared_padded_fragments = if separate { self.clipper.clip(line_points, TILE_EXTENT, shared_padding) } else { Vec::new() };

            for road_pass in &passes {
                let pass = &road_pass.pass;
                if separate {
                    result.register_road_style(BakedStyle::from_pass(pass), pass.key);
                } else {
                    result.register_style(BakedStyle::from_pass(pass), pass.key, style.placement);
                }

                if separate && self.append_decoration(&style, road_pass, exact_fragments, structure, road_layer, tile, result) {
                    continue;
                }

                let own_padded_fragments;
                let padded_fragments: &[ClippedLineFragment] = if separate {
                    &shared_padded_fragments
                } else {
                    own_padded_fragments = self.clipper.clip(line_points, TILE_EXTENT, (pass.line_geometry.line_width * 0.5) as f32);
                    &own_padded_fragments
                };

                let geometry = &pass.line_geometry;
                for fragment in padded_fragments {
                    for render_fragment in dash_pattern::fragments(fragment, geometry).iter() {
                        let first = render_fragment.points.first().copied();
                        let last = render_fragment.points.last().copied();
                        let connected = |point: Option<V2>| {
                            separate && point.is_some_and(|p| precomputation.shared_point_counts.get(&RoadConnectionPointKey::new(p)).copied().unwrap_or(0) > 1)
                        };
                        let start_connected = connected(first);
                        let end_connected = connected(last);
                        let start_boundary_continuation = separate && is_road_boundary_continuation_endpoint(first);
                        let end_boundary_continuation = separate && is_road_boundary_continuation_endpoint(last);
                        let start_continuation = separate && (render_fragment.start_clipped || start_boundary_continuation);
                        let end_continuation = separate && (render_fragment.end_clipped || end_boundary_continuation);
                        let should_extend_start = separate
                            && ((render_fragment.start_clipped && should_extend_clipped_road_endpoint(first))
                                || (start_boundary_continuation && should_extend_road_boundary_endpoint(first)));
                        let should_extend_end = separate
                            && ((render_fragment.end_clipped && should_extend_clipped_road_endpoint(last))
                                || (end_boundary_continuation && should_extend_road_boundary_endpoint(last)));

                        // A free end is a genuine end of the line: not a cut
                        // that continues into a neighbouring tile, not a shared
                        // road junction, and not sitting on the tile boundary.
                        // Free ends are the ones that may be capped or
                        // feathered. Every other cut must stay hard so it meets
                        // adjacent geometry flush.
                        let start_free = !start_continuation && !start_connected && first.is_some_and(is_point_strictly_inside_tile);
                        let end_free = !end_continuation && !end_connected && last.is_some_and(is_point_strictly_inside_tile);
                        // A connected end where the road goes on as another
                        // piece of the same style is rounded off, whatever the
                        // style's cap: the disc fills the wedge two square cuts
                        // leave on the outside of a bend, and its overlap with
                        // the next piece is invisible
                        // (`RoadLayerPrecomputation::continuation_keys_by_point`).
                        let start_continues_same_style = start_connected && !start_continuation && is_road_continuation_endpoint(first, style.key(), precomputation);
                        let end_continues_same_style = end_connected && !end_continuation && is_road_continuation_endpoint(last, style.key(), precomputation);
                        let start_cap_round = (geometry.line_cap_round && start_free) || start_continues_same_style;
                        let end_cap_round = (geometry.line_cap_round && end_free) || end_continues_same_style;

                        let options = LineOptions {
                            start_cap_round,
                            end_cap_round,
                            line_join_round: geometry.line_join_round,
                            feather_start: start_free,
                            feather_end: end_free,
                            emits_arc_length: pass.dash_length_points > 0.0,
                            extend_clipped_start: should_extend_start,
                            extend_clipped_end: should_extend_end,
                            clip_padding: if separate { shared_padding } else { 0.0 },
                            clip_geometry_to_tile_bounds: !separate,
                            // A road bucket ribbon of a flat-era tile is
                            // extruded on the GPU to its width on screen. A
                            // sphere-era tile keeps the baked ribbon, which the
                            // grid split and the sphere projection need.
                            deferred_extrusion,
                            // On a relief the ribbon takes a row at every grid
                            // crossing, as the ground takes a vertex.
                            subdivision_step,
                            ..LineOptions::new(geometry.line_width, TILE_EXTENT)
                        };
                        if let Some(polygon) = self.parse_line.parse(&render_fragment.points, &options) {
                            if separate {
                                result.append_road(polygon, pass.key, structure, road_layer, style.class_priority, road_pass.role);
                            } else {
                                result.append_ground(polygon, pass.key, style.placement);
                            }
                        }
                    }
                }
            }

            if let Some(shield_style) = style.shields.as_ref().filter(|_| !route_shields.is_empty()) {
                let group = route_shields.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");
                for fragment in exact_fragments {
                    for point in route_shield_placement::candidate_points(&fragment.points, TILE_EXTENT) {
                        result.route_shield_candidates.push(RouteShieldCandidate {
                            shields: route_shields.clone(),
                            style: shield_style.clone(),
                            style_key: style.key() as i32,
                            position: point,
                            group: group.clone(),
                        });
                    }
                }
            }

            if let (Some(text), Some(label_style)) = (label_text.as_deref(), road_label_style) {
                for fragment in exact_fragments {
                    if !should_include_road_label_fragment(fragment) {
                        continue;
                    }
                    let path = line_path(&fragment.points);
                    if path.len() >= 2 {
                        result.road_text_labels.push(ParsedRoadTextLabel::new(text.to_owned(), path, read.id(), read.feature.id.is_some(), layer.name, *label_style));
                    }
                }
            }
        }
    }

    /// The route signs of the tile's roads as point labels, once every road
    /// is read: the candidates `route_shield_placement` keeps, each a label
    /// that draws its signs, keyed by the tile and the place.
    pub fn append_route_shield_labels(&self, tile: Tile, result: &mut ReadingStageResult) {
        let candidates = std::mem::take(&mut result.route_shield_candidates);
        for candidate in route_shield_placement::select(&candidates, TILE_EXTENT) {
            let anchor = [clamp_i16(candidate.position.x.round() as i64), clamp_i16(candidate.position.y.round() as i64)];
            let mut style = candidate.style;
            style.shields = candidate.shields;
            let text_color = style.shields[0].appearance.text_color;
            let text_style = LabelTextStyle { key: candidate.style_key, ..LabelTextStyle::new(text_color, text_color, 0.0, style.size_points, style.weight) };
            let key = self.label_decisions.route_shield_key(to_style_tile(tile), &candidate.group, anchor);
            let mut label = ParsedTextLabel::new(candidate.group, anchor, key, style.rank, style.collision_rank, text_style);
            label.min_camera_zoom = style.min_camera_zoom;
            label.route_shields = Some(style);
            result.text_labels.push(label);
        }
    }

    /// The decoration a pass stamps along the feature's exact fragments on
    /// the separate-road path, appended in place: under the detail pass the
    /// oneway arrows. Returns false when the pass draws the plain ribbon
    /// instead.
    #[allow(clippy::too_many_arguments)]
    fn append_decoration(
        &self,
        style: &RoadStyle,
        road_pass: &RoadPass,
        fragments: &[ClippedLineFragment],
        structure: RoadStructureKind,
        layer: i32,
        tile: Tile,
        result: &mut ReadingStageResult,
    ) -> bool {
        let RoadDecorationKind::OnewayArrow(arrow) = style.decoration else { return false };
        if road_pass.role != RoadPassRole::Detail {
            return false;
        }
        let pass = &road_pass.pass;
        // On a relief a decoration is cut at the grid like the ground, so it
        // lies on the relief as the ground does.
        let relief_step = self.options.relief_subdivision_step(tile.z).map(|s| s.round() as i32);
        for fragment in fragments {
            for polygon in direction_arrows::build_polygons(&fragment.points, pass.line_geometry.line_width as f32, &arrow) {
                let polygon = match relief_step {
                    Some(step) => ground_subdivider::subdivide(&polygon, step),
                    None => polygon,
                };
                result.append_road(polygon, pass.key, structure, layer, style.class_priority, road_pass.role);
            }
        }
        true
    }
}

impl RoadStructureKind {
    /// Where a road draws: on the ground the automobile network draws as its
    /// own tier above the pedestrian one, so a path ending against an avenue
    /// never lies over its kerb. Both the level and the tier are the style's.
    pub fn new(level: map_style::style::RoadLevel, tier: map_style::style::RoadTier) -> RoadStructureKind {
        use map_style::style::{RoadLevel, RoadTier};
        match level {
            RoadLevel::Tunnel => RoadStructureKind::Tunnel,
            RoadLevel::Bridge => RoadStructureKind::Bridge,
            RoadLevel::Ground if tier == RoadTier::Automobile => RoadStructureKind::AutomobileGround,
            RoadLevel::Ground => RoadStructureKind::Ground,
        }
    }
}

fn line_path(points: &[V2]) -> Vec<[i16; 2]> {
    points
        .iter()
        .map(|p| {
            let x = p.x.clamp(0.0, TILE_EXTENT);
            let y = p.y.clamp(0.0, TILE_EXTENT);
            [clamp_i16(x.round() as i64), clamp_i16(y.round() as i64)]
        })
        .collect()
}

fn is_point_strictly_inside_tile(point: V2) -> bool {
    point.x > 0.0 && point.x < TILE_EXTENT && point.y > 0.0 && point.y < TILE_EXTENT
}

fn is_road_continuation_endpoint(point: Option<V2>, key: u8, precomputation: &RoadLayerPrecomputation) -> bool {
    point.is_some_and(|p| precomputation.continuation_keys_by_point.get(&RoadConnectionPointKey::new(p)).is_some_and(|keys| keys.contains(&key)))
}

fn is_road_boundary_continuation_endpoint(point: Option<V2>) -> bool {
    point.is_some_and(|p| LineClipper::is_on_tile_boundary(p, TILE_EXTENT))
}

fn should_extend_road_boundary_endpoint(point: Option<V2>) -> bool {
    const EPSILON: f32 = 0.0001;
    point.is_some_and(|p| (p.x - TILE_EXTENT).abs() <= EPSILON || (p.y - TILE_EXTENT).abs() <= EPSILON)
}

fn should_extend_clipped_road_endpoint(point: Option<V2>) -> bool {
    point.is_some_and(|p| p.x > TILE_EXTENT || p.y > TILE_EXTENT)
}

fn should_include_road_label_fragment(fragment: &ClippedLineFragment) -> bool {
    if fragment.points.len() < 2 {
        return false;
    }
    if !(fragment.start_clipped || fragment.end_clipped) {
        return true;
    }
    polyline_length(&fragment.points) >= MIN_CLIPPED_ROAD_LABEL_FRAGMENT_LENGTH
}

/// The length of a polyline in tile units.
fn polyline_length(points: &[V2]) -> f32 {
    points.windows(2).map(|pair| (pair[1] - pair[0]).length()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use map_style::style::RouteShieldStyle;

    fn candidate(position: V2, group: &str, spacing: f32) -> RouteShieldCandidate {
        RouteShieldCandidate {
            shields: Vec::new(),
            style: RouteShieldStyle::new(Vec::new()).spacing_points(spacing),
            style_key: 0,
            position,
            group: group.to_owned(),
        }
    }

    // Ported from the Swift `RouteShieldTests` placement cases.

    #[test]
    fn the_two_carriageways_of_a_motorway_stand_one_set_of_signs() {
        // Two parallel lines 40 units apart across the whole tile, as the two
        // directions of a motorway ship.
        let north = route_shield_placement::candidate_points(&[V2::new(0.0, 2028.0), V2::new(4096.0, 2028.0)], 4096.0);
        let south = route_shield_placement::candidate_points(&[V2::new(0.0, 2068.0), V2::new(4096.0, 2068.0)], 4096.0);
        assert_eq!(north.len(), 16, "a candidate every sixteenth of the tile");
        let candidates: Vec<_> = north.into_iter().chain(south).map(|p| candidate(p, "М-9", 220.0)).collect();
        let kept = route_shield_placement::select(&candidates, 4096.0);
        assert_eq!(kept.len(), 1, "220 points is most of a 256-point tile");
        assert!((kept[0].position.x - 1920.0).abs() <= 1.0, "the candidate nearest the centre");
    }

    #[test]
    fn different_signs_are_spaced_apart() {
        let candidates = [candidate(V2::new(2000.0, 2000.0), "М-9", 220.0), candidate(V2::new(2100.0, 2000.0), "А-107", 220.0)];
        let groups: Vec<_> = route_shield_placement::select(&candidates, 4096.0).into_iter().map(|c| c.group).collect();
        assert_eq!(groups, ["М-9", "А-107"], "the spacing holds within one group of signs, the collisions settle the rest");
    }

    #[test]
    fn a_road_label_on_a_cut_fragment_needs_a_quarter_tile_of_line() {
        let short = ClippedLineFragment { points: vec![V2::new(0.0, 10.0), V2::new(255.0, 10.0)], start_clipped: true, end_clipped: false };
        let long = ClippedLineFragment { points: vec![V2::new(0.0, 10.0), V2::new(256.0, 10.0)], start_clipped: true, end_clipped: false };
        let inside = ClippedLineFragment { points: vec![V2::new(10.0, 10.0), V2::new(20.0, 10.0)], start_clipped: false, end_clipped: false };
        assert!(!should_include_road_label_fragment(&short));
        assert!(should_include_road_label_fragment(&long));
        assert!(should_include_road_label_fragment(&inside));
    }

    #[test]
    fn a_label_path_is_clamped_to_the_tile_and_rounded() {
        assert_eq!(line_path(&[V2::new(-3.0, 10.4), V2::new(4100.0, 10.5)]), vec![[0, 10], [4096, 11]]);
    }

    #[test]
    fn the_automobile_tier_splits_the_ground_only() {
        use map_style::style::{RoadLevel, RoadTier};
        assert_eq!(RoadStructureKind::new(RoadLevel::Ground, RoadTier::Automobile), RoadStructureKind::AutomobileGround);
        assert_eq!(RoadStructureKind::new(RoadLevel::Ground, RoadTier::Pedestrian), RoadStructureKind::Ground);
        assert_eq!(RoadStructureKind::new(RoadLevel::Bridge, RoadTier::Automobile), RoadStructureKind::Bridge);
        assert_eq!(RoadStructureKind::new(RoadLevel::Tunnel, RoadTier::Automobile), RoadStructureKind::Tunnel);
    }
}
