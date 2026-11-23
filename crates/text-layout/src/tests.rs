// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use map_style::style::LabelFontWeight;

use crate::layout::{LabelLayoutRequest, LabelTextAlignment, LabelWrapOptions, TextLayout, TextMetrics};
use crate::{AtlasData, bundled};

/// A monospaced atlas: every letter advances half an em and spans it,
/// from the baseline to 0.7 em, the space advances a quarter em and draws
/// nothing, the hyphen is a letter like the others. Lines are 1.25 em apart.
fn mono() -> TextLayout {
    let letters: String = "abcdefghijklmnopqrstuvwxyz-"
        .chars()
        .enumerate()
        .map(|(i, c)| {
            format!(
                r#",{{"unicode":{},"advance":0.5,"planeBounds":{{"left":0,"bottom":0,"right":0.5,"top":0.7}},"atlasBounds":{{"left":{},"bottom":0,"right":{},"top":10}}}}"#,
                c as u32,
                i * 10,
                i * 10 + 10
            )
        })
        .collect();
    let json = format!(
        r#"{{"atlas":{{"type":"mtsdf","distanceRange":24,"distanceRangeMiddle":0,"size":64,"width":1000,"height":100,"yOrigin":"bottom"}},
        "metrics":{{"emSize":1,"lineHeight":1.25,"ascender":1,"descender":-0.25,"underlineY":-0.1,"underlineThickness":0.05}},
        "glyphs":[{{"unicode":32,"advance":0.25}}{letters}]}}"#
    );
    let atlas = AtlasData::from_json(&json).unwrap();
    TextLayout::new(atlas.clone(), atlas)
}

fn lay(layout: &TextLayout, text: &str, request: LabelLayoutRequest) -> TextMetrics {
    layout.label_vertices(text, &request)
}

fn request(scale: f32) -> LabelLayoutRequest {
    LabelLayoutRequest::new(3, scale, LabelFontWeight::Bold)
}

fn wrapped(scale: f32, max_width: f32, max_lines: usize, alignment: LabelTextAlignment) -> LabelLayoutRequest {
    request(scale).wrap(LabelWrapOptions::new(max_width, max_lines, alignment))
}

/// The distinct line baselines of a layout, top first (y of each glyph's bottom).
fn line_bottoms(metrics: &TextMetrics) -> Vec<f32> {
    let mut bottoms: Vec<f32> = metrics.vertices.chunks(6).map(|q| q[0].position[1]).collect();
    bottoms.sort_by(|a, b| b.total_cmp(a));
    bottoms.dedup();
    bottoms
}

#[test]
fn a_line_is_six_vertices_per_drawn_glyph_from_the_origin() {
    let m = lay(&mono(), "ab c", request(10.0));
    assert_eq!(m.vertices.len(), 18, "the space draws nothing");
    // a at 0, b at 5, the space a quarter em, c at 12.5: 17.5 wide.
    assert_eq!(m.width, 17.5);
    assert_eq!(m.height, 7.0);
    assert_eq!(m.vertices[0].position, [0.0, 0.0]);
    assert_eq!(m.vertices[12].position, [12.5, 0.0]);
    assert!(m.vertices.iter().all(|v| v.label_index == 3));
    // The quad's corners: left-bottom, right-bottom, left-top, right-bottom, right-top, left-top.
    let q: Vec<[f32; 2]> = m.vertices[..6].iter().map(|v| v.position).collect();
    assert_eq!(q, vec![[0.0, 0.0], [5.0, 0.0], [0.0, 7.0], [5.0, 0.0], [5.0, 7.0], [0.0, 7.0]]);
    assert_eq!(m.vertices[0].uv, [0.0, 1.0]);
    assert_eq!(m.vertices[4].uv, [0.01, 0.9]);
}

#[test]
fn a_character_the_atlas_lacks_advances_a_quarter_em() {
    let m = lay(&mono(), "a\u{4E2D}b", request(10.0));
    assert_eq!(m.vertices.len(), 12);
    assert_eq!(m.vertices[6].position[0], 7.5);
}

#[test]
fn tracking_adds_its_ems_after_every_character() {
    let plain = lay(&mono(), "abc", request(10.0));
    let tracked = lay(&mono(), "abc", request(10.0).letter_spacing(0.2));
    // Two gaps between three letters, each 0.2 em of 10 points.
    assert!((tracked.width - plain.width - 4.0).abs() < 1e-4);
}

#[test]
fn the_box_moves_to_zero_unless_the_baseline_is_kept() {
    let atlas = bundled();
    let normalized = lay(&atlas, "Gy", request(12.0));
    let kept = lay(&atlas, "Gy", request(12.0).normalize_y(false));
    let min_y = |m: &TextMetrics| m.vertices.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
    assert_eq!(min_y(&normalized), 0.0);
    assert!(min_y(&kept) < 0.0, "the descender of y hangs under the baseline");
    assert_eq!(normalized.height, kept.height);
}

#[test]
fn a_wrapped_label_breaks_at_spaces_and_hyphens_into_left_aligned_lines() {
    // Every letter 5 points: "abcd " fits 20, two words do not.
    let m = lay(&mono(), "abcd efgh-ijkl", wrapped(10.0, 22.0, 3, LabelTextAlignment::Left));
    let bottoms = line_bottoms(&m);
    assert_eq!(bottoms.len(), 3, "abcd / efgh- / ijkl");
    assert_eq!(bottoms[0] - bottoms[1], 12.5, "lines a line height apart");
    assert_eq!(m.width, 25.0, "the hyphen stays on its line");
    // Left aligned: every line starts at x zero.
    let line_starts: Vec<f32> = bottoms.iter().map(|b| m.vertices.iter().filter(|v| v.position[1] == *b).map(|v| v.position[0]).fold(f32::MAX, f32::min)).collect();
    assert_eq!(line_starts, vec![0.0, 0.0, 0.0]);
}

#[test]
fn centred_and_right_aligned_lines_share_the_widest_lines_box() {
    for (alignment, start) in [(LabelTextAlignment::Center, 10.0), (LabelTextAlignment::Right, 20.0)] {
        let m = lay(&mono(), "abcdef ab", wrapped(10.0, 32.0, 3, alignment));
        assert_eq!(m.width, 30.0);
        let bottoms = line_bottoms(&m);
        let short_line_start = m.vertices.iter().filter(|v| v.position[1] == bottoms[1]).map(|v| v.position[0]).fold(f32::MAX, f32::min);
        assert_eq!(short_line_start, start, "{alignment:?}");
    }
}

#[test]
fn the_last_line_takes_the_remainder_and_forced_breaks_collapse_into_spaces() {
    let m = lay(&mono(), "ab\ncd\nef\ngh", wrapped(10.0, 100.0, 2, LabelTextAlignment::Left));
    let bottoms = line_bottoms(&m);
    assert_eq!(bottoms.len(), 2);
    // The second line is "cd ef gh": six letters and two spaces.
    let second: Vec<_> = m.vertices.chunks(6).filter(|q| q[0].position[1] == bottoms[1]).collect();
    assert_eq!(second.len(), 6);
    assert_eq!(second.iter().map(|q| q[4].position[0]).fold(0.0f32, f32::max), 35.0);
}

#[test]
fn a_forced_break_splits_lines_even_when_they_fit() {
    let m = lay(&mono(), "ab\ncd", wrapped(10.0, 100.0, 3, LabelTextAlignment::Left));
    assert_eq!(line_bottoms(&m).len(), 2);
    assert_eq!(m.width, 10.0);
}

#[test]
fn a_one_line_request_ignores_the_wrap_and_skips_line_breaks() {
    let m = lay(&mono(), "ab\ncd", wrapped(10.0, 5.0, 1, LabelTextAlignment::Left));
    assert_eq!(line_bottoms(&m).len(), 1);
    assert_eq!(m.width, 20.0);
}

#[test]
fn text_that_draws_nothing_lays_out_empty() {
    for text in ["", "   ", "\n"] {
        let m = lay(&mono(), text, wrapped(10.0, 50.0, 3, LabelTextAlignment::Center));
        assert!(m.vertices.is_empty());
        assert_eq!((m.width, m.height), (0.0, 0.0));
    }
}

#[test]
fn the_layout_scales_with_the_em() {
    let atlas = bundled();
    let small = lay(&atlas, "Point", request(11.0));
    let large = lay(&atlas, "Point", request(22.0));
    assert!((large.width / small.width - 2.0).abs() < 1e-4);
    assert!((large.height / small.height - 2.0).abs() < 1e-4);
}

#[test]
fn the_bundled_atlases_carry_latin_and_cyrillic_in_both_weights() {
    let atlas = bundled();
    for weight in [LabelFontWeight::Bold, LabelFontWeight::Thin] {
        let font = atlas.atlas(weight);
        assert_eq!((font.info.width, font.info.height), (2048, 2048));
        assert_eq!(atlas.atlas_em_texels(weight), 64.0);
        for c in ['A', 'z', 'Ж', 'я', '0'] {
            assert!(font.glyph(c as u32).is_some_and(|g| g.uv.is_some()), "{c} in {weight:?}");
        }
    }
    let points = atlas.code_points();
    assert!(points.windows(2).all(|w| w[0] < w[1]));
    assert!(points.contains(&32));
}

#[test]
fn a_label_vertex_is_thirty_two_bytes_with_the_index_at_sixteen() {
    let mut bytes = Vec::new();
    let mut v = crate::LabelVertex::new([1.0, 2.0], [0.25, 0.5], -7);
    v.sprite_uv = [3.0, 4.0];
    v.write(&mut bytes);
    assert_eq!(bytes.len(), crate::LabelVertex::STRIDE);
    assert_eq!(f32::from_le_bytes(bytes[4..8].try_into().unwrap()), 2.0);
    assert_eq!(f32::from_le_bytes(bytes[12..16].try_into().unwrap()), 0.5);
    assert_eq!(i32::from_le_bytes(bytes[16..20].try_into().unwrap()), -7);
    assert_eq!(&bytes[20..24], &[0, 0, 0, 0]);
    assert_eq!(f32::from_le_bytes(bytes[28..32].try_into().unwrap()), 4.0);
}

