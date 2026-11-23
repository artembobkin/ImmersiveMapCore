// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Label text into glyph quads (the layout half of the Swift `TextRenderer`):
//! measuring, breaking into lines at spaces and hyphens, aligning the lines,
//! tracking the letters. Six vertices per drawn glyph, in layout points
//! (y up), the label's box starting at the origin.

use map_style::style::LabelFontWeight;

use crate::atlas::{AtlasData, FontAtlas, LayoutGlyph};

/// One vertex of a label quad, 32 bytes as the GPU reads it (the Swift
/// `LabelVertex`): the position in layout points, the atlas UV, the label
/// the vertex belongs to, and a second UV a sprite or a plate reads.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[repr(C)]
pub struct LabelVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    pub label_index: i32,
    pub sprite_uv: [f32; 2],
}

impl LabelVertex {
    /// The bytes of one vertex: position at 0, uv at 8, the label index at
    /// 16, four bytes of padding, the sprite UV at 24.
    pub const STRIDE: usize = 32;

    pub fn new(position: [f32; 2], uv: [f32; 2], label_index: i32) -> LabelVertex {
        LabelVertex { position, uv, label_index, sprite_uv: [0.0, 0.0] }
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        for v in [self.position[0], self.position[1], self.uv[0], self.uv[1]] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&self.label_index.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&self.sprite_uv[0].to_le_bytes());
        out.extend_from_slice(&self.sprite_uv[1].to_le_bytes());
    }
}

/// A laid-out text: its box and its glyph quads.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
    pub vertices: Vec<LabelVertex>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LabelTextAlignment {
    #[default]
    Left,
    Center,
    Right,
}

/// How a label breaks into lines: the widest a line may be, in layout
/// points, the most lines, and how the lines align on each other.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LabelWrapOptions {
    pub max_width: f32,
    pub max_lines: usize,
    pub alignment: LabelTextAlignment,
}

impl LabelWrapOptions {
    pub fn new(max_width: f32, max_lines: usize, alignment: LabelTextAlignment) -> LabelWrapOptions {
        LabelWrapOptions { max_width, max_lines, alignment }
    }
}

/// What a label is laid out with besides its text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LabelLayoutRequest {
    pub label_index: i32,
    /// The em size in layout points.
    pub scale: f32,
    pub wrap: Option<LabelWrapOptions>,
    /// Whether the box's bottom moves to zero. A road name keeps its
    /// baseline at zero instead.
    pub normalize_y: bool,
    pub weight: LabelFontWeight,
    /// Extra room after every character, in ems: the tracking a spaced-out name is set with.
    pub letter_spacing: f32,
}

impl LabelLayoutRequest {
    /// One line, normalized, no tracking: the Swift defaults.
    pub fn new(label_index: i32, scale: f32, weight: LabelFontWeight) -> LabelLayoutRequest {
        LabelLayoutRequest { label_index, scale, wrap: None, normalize_y: true, weight, letter_spacing: 0.0 }
    }

    pub fn wrap(mut self, wrap: LabelWrapOptions) -> Self {
        self.wrap = Some(wrap);
        self
    }

    pub fn normalize_y(mut self, normalize_y: bool) -> Self {
        self.normalize_y = normalize_y;
        self
    }

    pub fn letter_spacing(mut self, letter_spacing: f32) -> Self {
        self.letter_spacing = letter_spacing;
        self
    }
}

struct LineLayout {
    vertices: Vec<LabelVertex>,
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

impl LineLayout {
    fn width(&self) -> f32 {
        (self.max_x - self.min_x).max(0.0)
    }
}

enum WrapSegment<'a> {
    Text(&'a str),
    ForcedBreak,
}

/// `CharacterSet.whitespaces`: the space separators and the tab.
fn is_whitespace(c: char) -> bool {
    matches!(c, '\t' | ' ' | '\u{00A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}')
}

/// `CharacterSet.whitespacesAndNewlines`: the whitespaces and the line and paragraph breaks.
fn is_whitespace_or_newline(c: char) -> bool {
    is_whitespace(c) || matches!(c, '\u{000A}'..='\u{000D}' | '\u{0085}' | '\u{2028}' | '\u{2029}')
}

fn trim_leading(text: &str) -> &str {
    text.trim_start_matches(is_whitespace_or_newline)
}

fn trim_trailing(text: &str) -> &str {
    text.trim_end_matches(is_whitespace_or_newline)
}

/// The bold and the thin atlas, and the layout over them.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLayout {
    bold: FontAtlas,
    thin: FontAtlas,
}

impl TextLayout {
    pub fn new(bold: AtlasData, thin: AtlasData) -> TextLayout {
        TextLayout { bold: FontAtlas::new(bold), thin: FontAtlas::new(thin) }
    }

    pub fn atlas(&self, weight: LabelFontWeight) -> &FontAtlas {
        match weight {
            LabelFontWeight::Bold => &self.bold,
            LabelFontWeight::Thin => &self.thin,
        }
    }

    /// Atlas texels per em of a weight's atlas.
    pub fn atlas_em_texels(&self, weight: LabelFontWeight) -> f64 {
        self.atlas(weight).info.size
    }

    /// The code points either atlas carries, once each, ascending (the
    /// Swift `VectorTileLabelGlyphCoverage(atlasData:thinAtlasData:)`).
    pub fn code_points(&self) -> Vec<u32> {
        let mut points: Vec<u32> = self.bold.code_points().chain(self.thin.code_points()).collect();
        points.sort_unstable();
        points.dedup();
        points
    }

    /// Lays a label out (the Swift `collectLabelVertices`): wrapped when the
    /// request wraps to more than one line at a positive width and the
    /// wrapped layout draws anything, on one line otherwise.
    pub fn label_vertices(&self, text: &str, request: &LabelLayoutRequest) -> TextMetrics {
        if let Some(wrap) = request.wrap
            && wrap.max_lines > 1
            && wrap.max_width > 0.0
        {
            let wrapped = self.wrapped_label_vertices(text, request, &wrap);
            if !wrapped.vertices.is_empty() {
                return wrapped;
            }
        }
        match self.line_layout(text, request, 0.0) {
            Some(line) => normalized(line.vertices, line.min_x, line.min_y, line.max_x, line.max_y, request.normalize_y),
            None => TextMetrics::default(),
        }
    }

    /// The advance a character takes: a code point the atlas lacks takes a quarter em.
    fn advance(atlas: &FontAtlas, glyph: Option<&LayoutGlyph>, scale: f32, letter_spacing: f32) -> f32 {
        match glyph {
            Some(g) => g.advance * scale + letter_spacing * scale,
            None => atlas.metrics.em_size as f32 * scale * 0.25 + letter_spacing * scale,
        }
    }

    fn line_layout(&self, text: &str, request: &LabelLayoutRequest, baseline_y: f32) -> Option<LineLayout> {
        let atlas = self.atlas(request.weight);
        let scale = request.scale;
        let index = request.label_index;
        let mut vertices = Vec::with_capacity(text.len() * 6);
        let mut x = 0.0f32;
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::MAX, f32::MAX, -f32::MAX, -f32::MAX);
        for c in text.chars() {
            if c == '\n' {
                continue;
            }
            let glyph = atlas.glyph(c as u32);
            let (Some(g), Some(uv)) = (glyph, glyph.and_then(|g| g.uv)) else {
                x += Self::advance(atlas, glyph, scale, request.letter_spacing);
                continue;
            };
            let left = x + g.plane_left * scale;
            let bottom = baseline_y + g.plane_bottom * scale;
            let right = left + (g.plane_right - g.plane_left) * scale;
            let top = bottom + (g.plane_top - g.plane_bottom) * scale;
            let [u0, v0, u1, v1] = uv;
            vertices.extend_from_slice(&[
                LabelVertex::new([left, bottom], [u0, v0], index),
                LabelVertex::new([right, bottom], [u1, v0], index),
                LabelVertex::new([left, top], [u0, v1], index),
                LabelVertex::new([right, bottom], [u1, v0], index),
                LabelVertex::new([right, top], [u1, v1], index),
                LabelVertex::new([left, top], [u0, v1], index),
            ]);
            x += Self::advance(atlas, Some(g), scale, request.letter_spacing);
            min_x = min_x.min(left);
            min_y = min_y.min(bottom);
            max_x = max_x.max(right);
            max_y = max_y.max(top);
        }
        if vertices.is_empty() || ![min_x, min_y, max_x, max_y].iter().all(|v| v.is_finite()) {
            return None;
        }
        Some(LineLayout { vertices, min_x, min_y, max_x, max_y })
    }

    fn wrapped_label_vertices(&self, text: &str, request: &LabelLayoutRequest, wrap: &LabelWrapOptions) -> TextMetrics {
        let lines = self.wrapped_lines(text, request, wrap);
        if lines.is_empty() {
            return TextMetrics::default();
        }
        let scale = request.scale;
        let line_advance = (self.atlas(request.weight).metrics.line_height as f32 * scale).max(scale);
        let layouts: Vec<LineLayout> = lines.iter().enumerate().filter_map(|(i, line)| self.line_layout(line, request, -(i as f32) * line_advance)).collect();
        if layouts.is_empty() {
            return TextMetrics::default();
        }
        let total_width = layouts.iter().map(LineLayout::width).fold(0.0f32, f32::max);
        let mut vertices = Vec::with_capacity(layouts.iter().map(|l| l.vertices.len()).sum());
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::MAX, f32::MAX, -f32::MAX, -f32::MAX);
        for layout in &layouts {
            let origin = match wrap.alignment {
                LabelTextAlignment::Left => 0.0,
                LabelTextAlignment::Center => (total_width - layout.width()) * 0.5,
                LabelTextAlignment::Right => total_width - layout.width(),
            };
            let offset = origin - layout.min_x;
            vertices.extend(layout.vertices.iter().map(|v| LabelVertex::new([v.position[0] + offset, v.position[1]], v.uv, v.label_index)));
            min_x = min_x.min(layout.min_x + offset);
            min_y = min_y.min(layout.min_y);
            max_x = max_x.max(layout.max_x + offset);
            max_y = max_y.max(layout.max_y);
        }
        normalized(vertices, min_x, min_y, max_x, max_y, request.normalize_y)
    }

    /// The width a text's glyphs span on one line, zero for one that draws nothing.
    pub fn measure_width(&self, text: &str, scale: f32, weight: LabelFontWeight, letter_spacing: f32) -> f32 {
        let atlas = self.atlas(weight);
        let mut x = 0.0f32;
        let (mut min_x, mut max_x) = (f32::MAX, -f32::MAX);
        for c in text.chars() {
            if c == '\n' {
                continue;
            }
            let glyph = atlas.glyph(c as u32);
            if let Some(g) = glyph
                && g.uv.is_some()
            {
                min_x = min_x.min(x + g.plane_left * scale);
                max_x = max_x.max(x + g.plane_right * scale);
            }
            x += Self::advance(atlas, glyph, scale, letter_spacing);
        }
        if min_x.is_finite() && max_x.is_finite() { (max_x - min_x).max(0.0) } else { 0.0 }
    }

    /// The text broken into at most `max_lines` lines no wider than the
    /// box where a break allows (the Swift `wrappedLines`). The last line
    /// allowed takes the whole remainder, a forced break collapsing into a
    /// space there.
    fn wrapped_lines(&self, text: &str, request: &LabelLayoutRequest, wrap: &LabelWrapOptions) -> Vec<String> {
        let max_lines = wrap.max_lines.max(1);
        let segments = wrap_segments(text);
        if segments.is_empty() {
            return if text.is_empty() { Vec::new() } else { vec![text.to_string()] };
        }
        let mut lines: Vec<String> = Vec::with_capacity(max_lines);
        let mut current = String::new();
        let mut needs_collapsed_break_separator = false;
        for segment in segments {
            match segment {
                WrapSegment::ForcedBreak => {
                    if lines.len() + 1 >= max_lines {
                        needs_collapsed_break_separator = !current.is_empty();
                        continue;
                    }
                    let normalized = trim_trailing(&current);
                    if !normalized.is_empty() {
                        lines.push(normalized.to_string());
                    }
                    current.clear();
                    needs_collapsed_break_separator = false;
                }
                WrapSegment::Text(raw) => {
                    let mut segment = if current.is_empty() { trim_leading(raw) } else { raw };
                    if segment.is_empty() {
                        continue;
                    }
                    if lines.len() + 1 >= max_lines {
                        if needs_collapsed_break_separator && !current.is_empty() && !current.ends_with('-') && !segment.starts_with(is_whitespace_or_newline) {
                            current.push(' ');
                        }
                        if current.is_empty() {
                            segment = trim_leading(segment);
                        }
                        current.push_str(segment);
                        needs_collapsed_break_separator = false;
                        continue;
                    }
                    if current.is_empty() {
                        current.push_str(segment);
                        continue;
                    }
                    let length = current.len();
                    current.push_str(segment);
                    if self.measure_width(&current, request.scale, request.weight, request.letter_spacing) > wrap.max_width {
                        current.truncate(length);
                        let normalized = trim_trailing(&current);
                        if !normalized.is_empty() {
                            lines.push(normalized.to_string());
                        }
                        current.clear();
                        current.push_str(trim_leading(segment));
                    }
                }
            }
        }
        let normalized = trim_trailing(&current);
        if !normalized.is_empty() {
            lines.push(normalized.to_string());
        }
        if lines.is_empty() && !text.is_empty() {
            return vec![trim_trailing(trim_leading(text)).to_string()];
        }
        lines.truncate(max_lines);
        lines
    }
}

/// The text cut after every whitespace and hyphen, a line break its own segment.
fn wrap_segments(text: &str) -> Vec<WrapSegment<'_>> {
    let mut segments = Vec::new();
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if c == '\n' {
            if start < i {
                segments.push(WrapSegment::Text(&text[start..i]));
            }
            segments.push(WrapSegment::ForcedBreak);
            start = i + 1;
            continue;
        }
        if is_whitespace(c) || c == '-' {
            let end = i + c.len_utf8();
            segments.push(WrapSegment::Text(&text[start..end]));
            start = end;
        }
    }
    if start < text.len() {
        segments.push(WrapSegment::Text(&text[start..]));
    }
    segments
}

/// The box of the vertices moved to start at x zero, and at y zero when asked.
fn normalized(mut vertices: Vec<LabelVertex>, min_x: f32, min_y: f32, max_x: f32, max_y: f32, normalize_y: bool) -> TextMetrics {
    if vertices.is_empty() || ![min_x, min_y, max_x, max_y].iter().all(|v| v.is_finite()) {
        return TextMetrics::default();
    }
    let shift_y = if normalize_y { min_y } else { 0.0 };
    if min_x != 0.0 || shift_y != 0.0 {
        for v in &mut vertices {
            v.position[0] -= min_x;
            v.position[1] -= shift_y;
        }
    }
    TextMetrics { width: (max_x - min_x).max(0.0), height: (max_y - min_y).max(0.0), vertices }
}
