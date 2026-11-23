// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Reads point features into labels, and adds the labels the parser
//! synthesizes itself (the ocean and sea names of the coarse zooms), a port
//! of the Swift `LabelFeatureReader`. Which features are labelled, with
//! which text and at which priority is the label policy's answer
//! (`TileLabelDecisions`). This only walks the points.
//!
//! Called only with labels on: with them off the point features are skipped
//! whole by the dispatcher, decision engine included, and the switch is
//! prepared-cache identity so a tile prepared without labels never answers
//! a map that wants them.

use map_style::adaptation::{LabelFeature, TileLabelDecisions};
use map_style::style::{FeatureStyle, LabelPlacement, MapStyleRuntime, PointLabelStyle};
use mvt::geometry::Point;

use super::contract::ParsedTextLabel;
use super::low_zoom_water;
use crate::coordinate_space::TILE_EXTENT_I32;
use crate::reading::{PreparedLayer, to_style_tile};
use crate::tile::Tile;
use crate::unification::ReadingStageResult;

pub struct LabelFeatureReader<'s> {
    label_decisions: &'s TileLabelDecisions,
    map_style: &'s MapStyleRuntime,
}

impl<'s> LabelFeatureReader<'s> {
    pub fn new(label_decisions: &'s TileLabelDecisions, map_style: &'s MapStyleRuntime) -> LabelFeatureReader<'s> {
        LabelFeatureReader { label_decisions, map_style }
    }

    /// One label per point of the feature that lies inside the tile and
    /// that the policy decides to label.
    pub fn read(&self, layer: &PreparedLayer<'_>, feature_index: usize, style: &PointLabelStyle, tile: Tile, result: &mut ReadingStageResult) {
        let read = &layer.features[feature_index];
        let Some(label) = read.facts.label() else { return };
        let points = layer.geometry.points(&read.feature);
        let feature_id = read.feature.id;
        let on_screen = style.placement == LabelPlacement::Screen;
        for point in points.into_iter().filter(|p| is_point_inside_tile(*p)) {
            let anchor = [point.x as i16, point.y as i16];
            let label_feature = LabelFeature {
                style_id: &self.label_decisions.style_id,
                tile: to_style_tile(tile),
                layer_name: layer.name,
                feature_id,
                anchor,
            };
            let Some(decision) = self.label_decisions.point_label_decision(&label_feature, label, style) else {
                continue;
            };
            let key = decision.identity.runtime_key();
            if label.names_water_body {
                result.water_name_texts.insert(decision.text.clone());
            }
            result.text_labels.push(ParsedTextLabel {
                poi_icon: decision.poi_icon,
                min_camera_zoom: style.min_camera_zoom,
                max_camera_zoom: style.max_camera_zoom,
                placement: style.placement,
                stands_on_roof: style.stands_on_roof && on_screen,
                feature_id,
                is_local: style.is_local && on_screen,
                ..ParsedTextLabel::new(decision.text, anchor, key, decision.priority.visibility_rank, decision.priority.collision_rank, decision.style)
            });
        }
    }

    /// The ocean and sea names of zooms 0 to 2, for a schema whose tiles
    /// carry them unreliably: added for every water body whose anchor falls
    /// in the tile and whose name no label of the tile already shows, in the
    /// style the map style gives such a name, or not at all.
    pub fn append_low_zoom_water_labels(&self, tile: Tile, result: &mut ReadingStageResult) {
        if tile.z > 2 {
            return;
        }
        for fallback in low_zoom_water::labels(tile) {
            let Some(name) = self.label_decisions.localized_name(fallback.names) else { continue };
            if fallback.is_duplicate(&result.water_name_texts) {
                continue;
            }
            let Some(point) = low_zoom_water::tile_point(fallback.latitude, fallback.longitude, tile) else { continue };
            let Some(FeatureStyle::PointLabel(style)) = self.map_style.water_name_style(fallback.kind, to_style_tile(tile)) else {
                continue;
            };
            result.text_labels.push(ParsedTextLabel::keyed(
                name,
                point,
                0,
                false,
                low_zoom_water::IDENTITY_NAMESPACE,
                fallback.sort_key,
                fallback.sort_key,
                style.text,
                None,
                style.min_camera_zoom,
                style.max_camera_zoom,
                style.placement,
            ));
        }
    }
}

fn is_point_inside_tile(point: Point) -> bool {
    point.x >= 0 && point.x <= TILE_EXTENT_I32 && point.y >= 0 && point.y <= TILE_EXTENT_I32
}
