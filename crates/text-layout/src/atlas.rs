// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The metrics of a label atlas (the Swift `AtlasData`, `AtlasInfo`,
//! `Metrics`, `Glyph` and `Bounds`), as msdf-atlas-gen writes them beside
//! the atlas image (`packages/immersive-map/assets/atlas*.json`).

use serde::Deserialize;

/// The atlas image the metrics describe.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtlasInfo {
    #[serde(rename = "type")]
    pub kind: String,
    pub distance_range: f64,
    pub distance_range_middle: f64,
    /// Atlas texels per em: what a width stated in ems spans in the distance field.
    pub size: f64,
    pub width: u32,
    pub height: u32,
    pub y_origin: String,
}

/// The font's vertical metrics, in ems.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FontMetrics {
    pub em_size: f64,
    pub line_height: f64,
    pub ascender: f64,
    pub descender: f64,
    #[serde(rename = "underlineY")]
    pub underline_y: f64,
    pub underline_thickness: f64,
}

/// A rectangle: in ems for a glyph's plane bounds, in texels (y up from
/// the atlas's bottom) for its atlas bounds.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Bounds {
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub top: f64,
}

/// One glyph of the atlas. A glyph without atlas bounds (a space) only advances.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtlasGlyph {
    pub unicode: u32,
    pub advance: f64,
    #[serde(default)]
    pub plane_bounds: Option<Bounds>,
    #[serde(default)]
    pub atlas_bounds: Option<Bounds>,
}

/// A whole atlas JSON.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AtlasData {
    pub atlas: AtlasInfo,
    pub metrics: FontMetrics,
    pub glyphs: Vec<AtlasGlyph>,
}

impl AtlasData {
    /// Reads an atlas JSON.
    pub fn from_json(json: &str) -> Result<AtlasData, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// A glyph as the layout reads it, in single precision the way the Swift
/// layout converts every `CGFloat` it reads: the plane bounds in ems and
/// the atlas UVs (v down from the top, the texture's orientation) of its quad.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutGlyph {
    pub unicode: u32,
    pub advance: f32,
    pub plane_left: f32,
    pub plane_bottom: f32,
    pub plane_right: f32,
    pub plane_top: f32,
    /// The atlas UVs (left, bottom, right, top), `None` for a glyph that only advances.
    pub uv: Option<[f32; 4]>,
}

/// One weight's atlas, ready for the layout: the em metrics and the glyphs
/// sorted by code point for the lookup.
#[derive(Debug, Clone, PartialEq)]
pub struct FontAtlas {
    pub info: AtlasInfo,
    pub metrics: FontMetrics,
    glyphs: Vec<LayoutGlyph>,
}

impl FontAtlas {
    pub fn new(data: AtlasData) -> FontAtlas {
        let width = data.atlas.width as f32;
        let height = data.atlas.height as f32;
        let mut glyphs: Vec<LayoutGlyph> = data
            .glyphs
            .iter()
            .map(|g| {
                let plane_left = g.plane_bounds.map_or(0.0, |b| b.left as f32);
                let plane_bottom = g.plane_bounds.map_or(0.0, |b| b.bottom as f32);
                let plane_right = g.plane_bounds.map_or(plane_left, |b| b.right as f32);
                let plane_top = g.plane_bounds.map_or(plane_bottom, |b| b.top as f32);
                let uv = g.atlas_bounds.map(|b| {
                    [b.left as f32 / width, 1.0 - b.bottom as f32 / height, b.right as f32 / width, 1.0 - b.top as f32 / height]
                });
                LayoutGlyph { unicode: g.unicode, advance: g.advance as f32, plane_left, plane_bottom, plane_right, plane_top, uv }
            })
            .collect();
        // A code point listed twice keeps its last entry, as the Swift lookup table does.
        glyphs.reverse();
        glyphs.sort_by_key(|g| g.unicode);
        glyphs.dedup_by_key(|g| g.unicode);
        FontAtlas { info: data.atlas, metrics: data.metrics, glyphs }
    }

    pub fn glyph(&self, unicode: u32) -> Option<&LayoutGlyph> {
        self.glyphs.binary_search_by_key(&unicode, |g| g.unicode).ok().map(|i| &self.glyphs[i])
    }

    /// The code points the atlas carries, ascending.
    pub fn code_points(&self) -> impl Iterator<Item = u32> + '_ {
        self.glyphs.iter().map(|g| g.unicode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_glyph_reads_its_uvs_flipped_to_the_texture_and_a_space_only_advances() {
        let json = r#"{"atlas":{"type":"mtsdf","distanceRange":24,"distanceRangeMiddle":0,"size":64,"width":100,"height":200,"yOrigin":"bottom"},
            "metrics":{"emSize":1,"lineHeight":1.25,"ascender":1,"descender":-0.25,"underlineY":-0.1,"underlineThickness":0.05},
            "glyphs":[{"unicode":32,"advance":0.25},{"unicode":65,"advance":0.5,"planeBounds":{"left":0,"bottom":-0.1,"right":0.5,"top":0.7},"atlasBounds":{"left":10,"bottom":20,"right":60,"top":100}}]}"#;
        let atlas = FontAtlas::new(AtlasData::from_json(json).unwrap());
        assert_eq!(atlas.glyph(32).unwrap().uv, None);
        let a = atlas.glyph(65).unwrap();
        assert_eq!(a.uv, Some([0.1, 0.9, 0.6, 0.5]));
        assert_eq!((a.plane_bottom, a.plane_top), (-0.1, 0.7));
        assert!(atlas.glyph(66).is_none());
        assert_eq!(atlas.code_points().collect::<Vec<_>>(), vec![32, 65]);
    }
}
