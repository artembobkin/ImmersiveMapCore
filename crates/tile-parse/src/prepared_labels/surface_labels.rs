// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The names painted on the map (`LabelPlacement::Surface`) baked into
//! glyph quads anchored in the tile (the Swift `TileSurfaceLabelsBuilder`).
//! Each vertex carries the label's anchor in the tile's render units and
//! its own offset from it in layout points, the text laid out at its
//! style's point size and centred on the anchor. How many tile units a
//! point spans is the frame's to say (`SurfaceLabelScale`), since the map's
//! scale on screen follows the viewport.

use std::collections::HashSet;

use map_style::style::LabelPlacement;
use text_layout::{LabelLayoutRequest, LabelTextAlignment, LabelWrapOptions, TextLayout};

use super::{SurfaceLabelRecord, SurfaceLabelSet};
use crate::coordinate_space::TILE_EXTENT;
use crate::labels::ParsedTextLabel;

/// A name wraps at this many ems to at most three lines, centred on each other.
const WRAP_WIDTH_EMS: f32 = 10.0;
const WRAP_LINE_COUNT: usize = 3;
/// About an average letter's advance, in ems: the box widens by the
/// tracking over it, so a spaced-out name breaks at the same letters.
const AVERAGE_ADVANCE_EMS: f32 = 0.55;

/// The surface labels among `labels`, the screen labels skipped. A label no
/// zoom of this tile shows is not baked, nor a second label of a key the
/// tile already carries.
pub fn build(layout: &TextLayout, labels: &[ParsedTextLabel], tile_zoom: i32) -> SurfaceLabelSet {
    let mut set = SurfaceLabelSet::default();
    let mut baked: HashSet<u64> = HashSet::new();
    for label in labels {
        let LabelPlacement::Surface(placement) = label.placement else { continue };
        if !placement.is_visible_on_tile_zoom(tile_zoom) || baked.contains(&label.key) {
            continue;
        }
        let style = label.text_style;
        let widening = 1.0 + placement.letter_spacing_em.max(0.0) / AVERAGE_ADVANCE_EMS;
        let wrap = LabelWrapOptions::new(style.size_points * WRAP_WIDTH_EMS * widening, WRAP_LINE_COUNT, LabelTextAlignment::Center);
        let request = LabelLayoutRequest::new(set.labels.len() as i32, style.size_points, style.weight).wrap(wrap).letter_spacing(placement.letter_spacing_em);
        let metrics = layout.label_vertices(&label.text, &request);
        if metrics.vertices.is_empty() {
            continue;
        }
        // The parser's anchor is y down, the ground's vertices y up. The
        // layout runs y up too, so a glyph keeps its counter-clockwise winding.
        let anchor = [label.position[0] as f32, TILE_EXTENT - label.position[1] as f32];
        let half = [metrics.width * 0.5, metrics.height * 0.5];
        let vertex_start = set.vertices.len();
        for mut v in metrics.vertices {
            // The sprite slot is free on a text glyph: it carries the offset in points.
            v.sprite_uv = [v.position[0] - half[0], v.position[1] - half[1]];
            v.position = anchor;
            set.vertices.push(v);
        }
        set.labels.push(SurfaceLabelRecord { key: label.key, style, placement, vertex_start, vertex_count: set.vertices.len() - vertex_start });
        baked.insert(label.key);
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use map_style::style::{LabelFontWeight, LabelTextStyle, SurfaceLabelPlacement};

    fn label(text: &str, key: u64, placement: LabelPlacement) -> ParsedTextLabel {
        let style = LabelTextStyle::new([0.2, 0.4, 0.7], [1.0, 1.0, 1.0], 0.15, 12.0, LabelFontWeight::Thin);
        ParsedTextLabel { placement, ..ParsedTextLabel::new(text.to_string(), [1024, 1024], key, 0, 0, style) }
    }

    fn sea() -> LabelPlacement {
        LabelPlacement::Surface(SurfaceLabelPlacement { reference_zoom: 3.0, minimum_zoom: 2.5, maximum_zoom: 5.5, letter_spacing_em: 0.15 })
    }

    #[test]
    fn a_tile_no_zoom_of_which_shows_the_name_does_not_carry_it() {
        let LabelPlacement::Surface(placement) = sea() else { unreachable!() };
        assert!(!placement.is_visible_on_tile_zoom(1));
        assert!(placement.is_visible_on_tile_zoom(2));
        assert!(placement.is_visible_on_tile_zoom(5));
        assert!(!placement.is_visible_on_tile_zoom(6));
        let layout = text_layout::bundled();
        assert!(build(&layout, &[label("Black Sea", 7, sea())], 6).labels.is_empty());
    }

    #[test]
    fn each_name_is_baked_once_centred_on_its_anchor_in_render_space() {
        let layout = text_layout::bundled();
        let labels = [label("Black Sea", 7, sea()), label("Black Sea", 7, sea()), label("Moscow", 8, LabelPlacement::Screen), label("Sea of Azov", 9, sea())];
        let set = build(&layout, &labels, 4);
        assert_eq!(set.labels.iter().map(|l| l.key).collect::<Vec<_>>(), vec![7, 9]);
        assert_eq!(set.labels[0].vertex_start, 0);
        assert_eq!(set.labels[1].vertex_start, set.labels[0].vertex_count);
        assert_eq!(set.vertices.len(), set.labels[1].vertex_start + set.labels[1].vertex_count);
        assert!(set.vertices.iter().all(|v| v.position == [1024.0, 3072.0]), "the anchor, y up");
        let first = &set.vertices[..set.labels[0].vertex_count];
        assert!(first.iter().all(|v| v.label_index == 0));
        let (min_x, max_x) = first.iter().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v.sprite_uv[0]), b.max(v.sprite_uv[0])));
        assert!((min_x + max_x).abs() < 1e-3, "centred: {min_x} {max_x}");
        assert!(set.vertices[set.labels[1].vertex_start..].iter().all(|v| v.label_index == 1));
    }
}
