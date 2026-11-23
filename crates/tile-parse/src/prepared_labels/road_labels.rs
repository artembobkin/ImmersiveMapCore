// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The tile's road names laid out along their roads (the Swift
//! `TileRoadLabelsBuilder`): the pieces of one name in one style are
//! chained end to end into the longest paths they make, each path's name
//! laid out once on its baseline, and its glyphs repeated for every anchor
//! the path has room for.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};

use map_style::style::LabelTextStyle;
use text_layout::{LabelLayoutRequest, LabelVertex, TextLayout};

use super::{IndexRange, RoadLabelAnchor, RoadLabels, RoadPathLabel, RoadPathRange};
use crate::coordinate_space::{TILE_EXTENT, TILE_EXTENT_F64};
use crate::labels::ParsedRoadTextLabel;

/// The distance between two anchors of a name, in layout points at the tile's zoom.
const REPEAT_DISTANCE_POINTS: f32 = 50.0;
/// A tile's side on screen at the reference scale, in layout points: a
/// screen distance crosses into tile units through it.
const TILE_SCREEN_POINT_SIZE: f32 = 256.0;
const MAX_ANCHORS_PER_PATH: usize = 7;

pub fn build(layout: &TextLayout, labels: &[ParsedRoadTextLabel]) -> RoadLabels {
    let merged = merge(labels);
    let mut out = RoadLabels::default();
    out.path_inputs.reserve(merged.len() * 4);
    let mut tile_glyph_index = 0usize;
    let tile_units_per_point = TILE_EXTENT / TILE_SCREEN_POINT_SIZE;
    for label in &merged {
        let start = out.path_inputs.len();
        out.path_inputs.extend(label.path.iter().map(|p| [(p[0] as f64 / TILE_EXTENT_F64) as f32, (p[1] as f64 / TILE_EXTENT_F64) as f32]));
        let count = out.path_inputs.len() - start;
        if count == 0 {
            continue;
        }
        let label_index = out.path_labels.len();
        out.path_labels.push(RoadPathLabel { text: label.text.clone(), key: label.key });
        out.path_ranges.push(RoadPathRange { start, count, label_index });
        // Every road glyph of a tile draws in one call with one atlas, the
        // first label's weight: a glyph laid out against the other atlas
        // would sample the wrong region. The size stays each label's own.
        let style = *out.label_style.get_or_insert(label.style);
        debug_assert!(label.style.weight == style.weight, "Road labels in one tile must share a font weight: the tile binds a single atlas texture per draw.");
        let metrics = layout.label_vertices(&label.text, &LabelLayoutRequest::new(0, label.style.size_points, style.weight).normalize_y(false));
        out.sizes.push([metrics.width, metrics.height]);

        let glyph_start = out.glyph_bounds.len();
        let glyph_count = metrics.vertices.len() / 6;
        for quad in metrics.vertices.as_chunks::<6>().0 {
            let (mut min_x, mut max_x, mut min_y, mut max_y) = (f32::MAX, -f32::MAX, f32::MAX, -f32::MAX);
            for v in quad {
                min_x = min_x.min(v.position[0]);
                max_x = max_x.max(v.position[0]);
                min_y = min_y.min(v.position[1]);
                max_y = max_y.max(v.position[1]);
            }
            out.glyph_bounds.push([min_x, max_x, min_y, max_y]);
        }
        out.glyph_bound_ranges.push(IndexRange { start: glyph_start, count: glyph_count });

        let anchor_start = out.anchors.len();
        if glyph_count > 0 && count > 1 {
            let anchors = build_anchors(&label.path, metrics.width * tile_units_per_point, REPEAT_DISTANCE_POINTS * tile_units_per_point, label_index);
            for anchor in anchors {
                out.anchors.push(anchor);
                for (glyph, quad) in metrics.vertices.as_chunks::<6>().0.iter().enumerate() {
                    let index = (tile_glyph_index + glyph) as i32;
                    out.local_glyph_vertices.extend(quad.iter().map(|v| LabelVertex { label_index: index, ..*v }));
                }
                tile_glyph_index += glyph_count;
            }
        }
        out.anchor_ranges.push(IndexRange { start: anchor_start, count: out.anchors.len() - anchor_start });
    }
    out
}

/// Where a name stands along its path: one anchor in the middle of a path
/// no longer than the spacing, otherwise every spacing from half a spacing
/// in, never closer to an end than half the name, at most seven.
fn build_anchors(path: &[[i16; 2]], label_width_tile: f32, spacing_tile: f32, label_index: usize) -> Vec<RoadLabelAnchor> {
    if path.len() < 2 {
        return Vec::new();
    }
    let points: Vec<[f32; 2]> = path.iter().map(|p| [p[0] as f32, p[1] as f32]).collect();
    let segment_lengths: Vec<f32> = points.windows(2).map(|w| {
        let (dx, dy) = (w[1][0] - w[0][0], w[1][1] - w[0][1]);
        (dx * dx + dy * dy).sqrt()
    }).collect();
    let total: f32 = segment_lengths.iter().sum();
    if total <= 0.0 || label_width_tile <= 0.0 {
        return Vec::new();
    }
    let footprint = label_width_tile.min(total);
    let half_footprint = footprint * 0.5;
    let mut distances = Vec::new();
    if total <= spacing_tile {
        distances.push(total * 0.5);
    } else {
        let max_distance = total - half_footprint;
        let mut distance = (spacing_tile * 0.5).max(half_footprint);
        while distance <= max_distance && distances.len() < MAX_ANCHORS_PER_PATH {
            distances.push(distance);
            distance += spacing_tile;
        }
    }
    let mut anchors = Vec::with_capacity(distances.len());
    for (ordinal, &distance) in distances.iter().enumerate() {
        if distance < half_footprint || distance > total - half_footprint {
            continue;
        }
        let mut segment = 0usize;
        let mut accumulated = 0.0f32;
        let mut length = segment_lengths[0];
        while segment < segment_lengths.len() - 1 && accumulated + length < distance {
            accumulated += length;
            segment += 1;
            length = segment_lengths[segment];
        }
        let t = if length > 0.0 { (distance - accumulated) / length } else { 0.0 };
        anchors.push(RoadLabelAnchor { path_index: label_index as u32, segment_index: segment as u32, t: t.clamp(0.0, 1.0), anchor_ordinal: ordinal as u32 });
    }
    anchors
}

/// A road name's identity for the merge: its text and its style, bit for bit.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MergeKey {
    text: String,
    style: (i32, u8, [u32; 3], [u32; 3], [u32; 4]),
}

impl MergeKey {
    fn new(text: &str, style: &LabelTextStyle) -> MergeKey {
        MergeKey {
            text: text.to_string(),
            style: (
                style.key,
                style.weight.raw_value(),
                style.fill_color.map(f32::to_bits),
                style.stroke_color.map(f32::to_bits),
                [style.halo_em.to_bits(), style.size_points.to_bits(), style.embolden_points.to_bits(), style.opacity.to_bits()],
            ),
        }
    }
}

#[derive(Debug, Clone)]
struct Segment {
    source_order: usize,
    text: String,
    style: LabelTextStyle,
    path: Vec<[i16; 2]>,
    length: f32,
}

impl Segment {
    fn new(source_order: usize, text: String, style: LabelTextStyle, path: Vec<[i16; 2]>) -> Segment {
        let length = path
            .windows(2)
            .map(|w| {
                let (dx, dy) = ((w[1][0] as i32 - w[0][0] as i32) as f32, (w[1][1] as i32 - w[0][1] as i32) as f32);
                (dx * dx + dy * dy).sqrt()
            })
            .sum();
        Segment { source_order, text, style, path, length }
    }

    fn reversed(&self) -> Segment {
        let mut path = self.path.clone();
        path.reverse();
        Segment { path, ..self.clone() }
    }

    fn appending(&self, other: &Segment) -> Segment {
        let mut path = self.path.clone();
        let skip = usize::from(path.last() == other.path.first());
        path.extend_from_slice(&other.path[skip.min(other.path.len())..]);
        Segment::new(self.source_order.min(other.source_order), self.text.clone(), self.style, path)
    }

    fn first(&self) -> [i16; 2] {
        self.path.first().copied().unwrap_or([0, 0])
    }

    fn last(&self) -> [i16; 2] {
        self.path.last().copied().unwrap_or([0, 0])
    }

    /// The seed of a chain is the least segment by this order: the
    /// shortest, then the lowest first point, then the latest in the source.
    fn seed_order(a: &Segment, b: &Segment) -> Ordering {
        a.length
            .partial_cmp(&b.length)
            .unwrap_or(Ordering::Equal)
            .then(a.first()[0].cmp(&b.first()[0]))
            .then(a.first()[1].cmp(&b.first()[1]))
            .then(b.source_order.cmp(&a.source_order))
    }

    /// Whether `a` continues a chain better than `b`: the longer, then the
    /// lower end point, then the earlier in the source.
    fn continues_better(a: &Segment, b: &Segment) -> bool {
        if a.length != b.length {
            return a.length > b.length;
        }
        let (ea, eb) = (a.last(), b.last());
        if ea[0] != eb[0] {
            return ea[0] < eb[0];
        }
        if ea[1] != eb[1] {
            return ea[1] < eb[1];
        }
        a.source_order < b.source_order
    }
}

struct Merged {
    text: String,
    path: Vec<[i16; 2]>,
    key: u64,
    style: LabelTextStyle,
    source_order: usize,
}

/// The pieces of every name chained into paths, in a deterministic order:
/// by text, first point, the longer path first, then the source order.
fn merge(labels: &[ParsedRoadTextLabel]) -> Vec<Merged> {
    let mut groups: HashMap<MergeKey, Vec<Segment>> = HashMap::new();
    for (order, label) in labels.iter().enumerate() {
        groups.entry(MergeKey::new(&label.text, &label.text_style)).or_default().push(Segment::new(order, label.text.clone(), label.text_style, label.path.clone()));
    }
    let mut merged: Vec<Merged> = groups.into_values().flat_map(merge_group).collect();
    merged.sort_by(|a, b| {
        let (fa, fb) = (a.path.first().copied().unwrap_or([0, 0]), b.path.first().copied().unwrap_or([0, 0]));
        a.text.cmp(&b.text).then(fa[0].cmp(&fb[0])).then(fa[1].cmp(&fb[1])).then(b.path.len().cmp(&a.path.len())).then(a.source_order.cmp(&b.source_order))
    });
    merged
}

fn merge_group(segments: Vec<Segment>) -> Vec<Merged> {
    let mut unused: BTreeSet<usize> = (0..segments.len()).collect();
    let mut results = Vec::with_capacity(segments.len());
    while let Some(&seed) = unused.iter().min_by(|&&a, &&b| Segment::seed_order(&segments[a], &segments[b])) {
        unused.remove(&seed);
        let mut chain = segments[seed].clone();
        while let Some((index, reverse)) = continuation(chain.last(), &segments, &unused, true) {
            unused.remove(&index);
            let next = if reverse { segments[index].reversed() } else { segments[index].clone() };
            chain = chain.appending(&next);
        }
        while let Some((index, reverse)) = continuation(chain.first(), &segments, &unused, false) {
            unused.remove(&index);
            let next = if reverse { segments[index].reversed() } else { segments[index].clone() };
            chain = next.appending(&chain);
        }
        let key = merged_key(&chain.text, &chain.style, &chain.path);
        results.push(Merged { text: chain.text, path: chain.path, key, style: chain.style, source_order: chain.source_order });
    }
    results
}

/// The best unused piece meeting a chain's end (`tail`, the piece then
/// starts there) or its start (the piece then ends there), reversed when
/// it meets the chain the other way round.
fn continuation(endpoint: [i16; 2], segments: &[Segment], unused: &BTreeSet<usize>, tail: bool) -> Option<(usize, bool)> {
    let mut best: Option<(usize, bool, Segment)> = None;
    for &index in unused {
        let segment = &segments[index];
        let (forward_end, reverse_end) = if tail { (segment.path.first(), segment.path.last()) } else { (segment.path.last(), segment.path.first()) };
        let candidate = if forward_end == Some(&endpoint) {
            (false, segment.clone())
        } else if reverse_end == Some(&endpoint) {
            (true, segment.reversed())
        } else {
            continue;
        };
        if best.as_ref().is_none_or(|(_, _, b)| Segment::continues_better(&candidate.1, b)) {
            best = Some((index, candidate.0, candidate.1));
        }
    }
    best.map(|(index, reverse, _)| (index, reverse))
}

/// A merged name's identity: its text, its style and its path, FNV-1a 64.
fn merged_key(text: &str, style: &LabelTextStyle, path: &[[i16; 2]]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    feed(text.as_bytes());
    feed(&[0xff]);
    feed(&style.key.to_le_bytes());
    for c in style.fill_color.iter().chain(&style.stroke_color).chain(&[style.halo_em, style.size_points]) {
        feed(&c.to_bits().to_le_bytes());
    }
    feed(&[style.weight.raw_value()]);
    feed(&style.embolden_points.to_bits().to_le_bytes());
    feed(&style.opacity.to_bits().to_le_bytes());
    feed(&(path.len() as u64).to_le_bytes());
    for p in path {
        feed(&p[0].to_le_bytes());
        feed(&p[1].to_le_bytes());
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use map_style::style::LabelFontWeight;

    fn style() -> LabelTextStyle {
        LabelTextStyle::new([0.2, 0.2, 0.2], [1.0, 1.0, 1.0], 0.15, 12.0, LabelFontWeight::Bold)
    }

    fn road(text: &str, path: &[[i16; 2]]) -> ParsedRoadTextLabel {
        ParsedRoadTextLabel { text: text.to_string(), path: path.to_vec(), key: 0, text_style: style() }
    }

    #[test]
    fn the_pieces_of_a_name_chain_into_one_path_whichever_way_they_run() {
        // The middle piece runs backwards: it is reversed to continue the chain.
        let labels = [road("Tverskaya", &[[0, 0], [1000, 0]]), road("Tverskaya", &[[2000, 0], [1000, 0]]), road("Tverskaya", &[[2000, 0], [3000, 0]]), road("Arbat", &[[0, 500], [900, 500]])];
        let merged = merge(&labels);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].text, "Arbat");
        let tverskaya = &merged[1];
        assert_eq!(tverskaya.path, vec![[0, 0], [1000, 0], [2000, 0], [3000, 0]]);
        assert_eq!(tverskaya.source_order, 0);
    }

    #[test]
    fn a_name_in_another_style_stays_its_own_path() {
        let mut other = road("Tverskaya", &[[1000, 0], [2000, 0]]);
        other.text_style.size_points = 14.0;
        let merged = merge(&[road("Tverskaya", &[[0, 0], [1000, 0]]), other]);
        assert_eq!(merged.len(), 2);
        assert_ne!(merged[0].key, merged[1].key);
    }

    #[test]
    fn a_branching_name_continues_along_its_longest_piece() {
        let labels = [road("Ring", &[[0, 0], [100, 0]]), road("Ring", &[[100, 0], [200, 0]]), road("Ring", &[[100, 0], [100, 900]])];
        let merged = merge(&labels);
        assert_eq!(merged.len(), 2);
        // The shortest piece seeds the first chain, then the longest branch continues it.
        let long = merged.iter().find(|m| m.path.len() == 3).unwrap();
        assert_eq!(long.path, vec![[0, 0], [100, 0], [100, 900]]);
    }

    #[test]
    fn anchors_repeat_every_spacing_and_keep_half_the_name_from_the_ends() {
        let path = [[0, 0], [4000, 0]];
        let anchors = build_anchors(&path, 400.0, 800.0, 3);
        assert_eq!(anchors.len(), 5, "at 400, 1200, 2000, 2800, 3600");
        assert!(anchors.iter().all(|a| a.path_index == 3 && a.segment_index == 0));
        assert_eq!(anchors[0].t, 0.1);
        assert_eq!(anchors.iter().map(|a| a.anchor_ordinal).collect::<Vec<_>>(), vec![0, 1, 2, 3, 4]);
        // A path no longer than the spacing takes one anchor in its middle.
        let short = build_anchors(&[[0, 0], [300, 0], [300, 300]], 100.0, 800.0, 0);
        assert_eq!(short.len(), 1);
        assert_eq!((short[0].segment_index, short[0].t), (0, 1.0));
        // A name longer than its path is held to the path and still takes the middle.
        assert_eq!(build_anchors(&[[0, 0], [100, 0]], 1000.0, 800.0, 0).len(), 1);
        assert!(build_anchors(&[[0, 0], [0, 0]], 10.0, 800.0, 0).is_empty());
    }

    #[test]
    fn anchors_stop_at_seven() {
        assert_eq!(build_anchors(&[[0, 0], [30000, 0]], 100.0, 800.0, 0).len(), MAX_ANCHORS_PER_PATH);
    }

    #[test]
    fn every_anchor_draws_the_names_glyphs_with_tile_wide_glyph_indices() {
        let layout = text_layout::bundled();
        let labels = [road("Arbat", &[[0, 2000], [4000, 2000]]), road("Noviy", &[[0, 100], [4000, 100]])];
        let built = build(&layout, &labels);
        assert_eq!(built.path_labels.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), vec!["Arbat", "Noviy"]);
        assert_eq!(built.path_ranges[1], RoadPathRange { start: 2, count: 2, label_index: 1 });
        assert_eq!(built.path_inputs[0], [0.0, 2000.0 / 4096.0]);
        assert_eq!(built.glyph_bound_ranges, vec![IndexRange { start: 0, count: 5 }, IndexRange { start: 5, count: 5 }]);
        let placed: usize = built.anchor_ranges.iter().map(|r| r.count * 5).sum();
        assert!(placed > 0);
        assert_eq!(built.local_glyph_vertices.len(), placed * 6);
        let indices: Vec<i32> = built.local_glyph_vertices.chunks(6).map(|q| q[0].label_index).collect();
        assert_eq!(indices, (0..placed as i32).collect::<Vec<_>>());
        assert_eq!(built.label_style.unwrap().size_points, 12.0);
        // Laid out on the baseline: a glyph's box is about it, not moved to zero.
        let width = built.sizes[0][0];
        assert!(width > 20.0 && width < 60.0, "{width}");
    }
}
