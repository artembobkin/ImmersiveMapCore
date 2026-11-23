// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The shapes of a source as flat arrays (the TS `LayerSourceContent`), the
//! styles of a layer and how a shape's style is resolved against them (the
//! Swift `ImmersiveMapLayer.style(of:)` and `ImmersiveMapGeoJSON`'s
//! simplestyle overlay).
//!
//! The layout, which the TS side builds too for shapes made in code:
//! - `coordinates`: longitude and latitude in degrees, interleaved;
//! - `polygons`: three words a zone, its first ring, its ring count, its style;
//! - `rings`: two words a ring, its first coordinate and its coordinate count;
//! - `polylines`: three words a line, its first coordinate, its count, its style;
//! - `points`: two words a point, its coordinate and its style;
//! - `ids`: one a shape, the zones first, then the lines, then the points, NaN
//!   for a shape with no id of its own;
//! - `styles`: `STYLE_RECORD_FLOATS` floats a record, a shape's style word its
//!   index or `NO_STYLE`.

/// The style word of a shape that carries none.
pub const NO_STYLE: u32 = u32::MAX;
/// The floats of one style record.
pub const STYLE_RECORD_FLOATS: usize = 16;
/// The floats of a layer's three styles (`LayerStyles::from_floats`).
pub const LAYER_STYLE_FLOATS: usize = 24;

const KIND_SIMPLE: f32 = 1.0;
const KIND_POLYGON: f32 = 2.0;
const KIND_POLYLINE: f32 = 3.0;
const KIND_POINT: f32 = 4.0;

/// The shapes of a source, owned.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceContent {
    pub coordinates: Vec<f64>,
    pub polygons: Vec<u32>,
    pub rings: Vec<u32>,
    pub polylines: Vec<u32>,
    pub points: Vec<u32>,
    pub ids: Vec<f64>,
    pub styles: Vec<f32>,
}

/// The shapes of a source, borrowed: what the mesh is built from.
#[derive(Clone, Copy, Debug, Default)]
pub struct SourceView<'a> {
    pub coordinates: &'a [f64],
    pub polygons: &'a [u32],
    pub rings: &'a [u32],
    pub polylines: &'a [u32],
    pub points: &'a [u32],
    pub styles: &'a [f32],
}

impl SourceContent {
    pub fn view(&self) -> SourceView<'_> {
        SourceView {
            coordinates: &self.coordinates,
            polygons: &self.polygons,
            rings: &self.rings,
            polylines: &self.polylines,
            points: &self.points,
            styles: &self.styles,
        }
    }

    pub fn polygon_count(&self) -> usize {
        self.polygons.len() / 3
    }

    pub fn polyline_count(&self) -> usize {
        self.polylines.len() / 3
    }

    pub fn point_count(&self) -> usize {
        self.points.len() / 2
    }

    /// The coordinates of a ring as (longitude, latitude) pairs.
    pub fn ring_coordinates(&self, ring: usize) -> Vec<[f64; 2]> {
        let (start, count) = (self.rings[ring * 2] as usize, self.rings[ring * 2 + 1] as usize);
        pairs(&self.coordinates[start * 2..(start + count) * 2])
    }

    /// The point's (longitude, latitude).
    pub fn point_coordinate(&self, point: usize) -> [f64; 2] {
        let index = self.points[point * 2] as usize;
        [self.coordinates[index * 2], self.coordinates[index * 2 + 1]]
    }

    /// The style record a shape's style word names, `None` for none.
    pub fn record(&self, style: u32) -> Option<StyleRecord> {
        self.view().record(style)
    }
}

fn pairs(values: &[f64]) -> Vec<[f64; 2]> {
    values.as_chunks::<2>().0.to_vec()
}

impl<'a> SourceView<'a> {
    /// The record a style word names, `None` for `NO_STYLE`, an index past
    /// the records or a kind no record has.
    pub fn record(&self, style: u32) -> Option<StyleRecord> {
        if style == NO_STYLE {
            return None;
        }
        let start = style as usize * STYLE_RECORD_FLOATS;
        let record = self.styles.get(start..start + STYLE_RECORD_FLOATS)?;
        StyleRecord::decode(record)
    }
}

/// Builds a `SourceContent` shape by shape, the ids of each kind apart so
/// they land in their order (zones, lines, points) whatever order the shapes
/// come in.
#[derive(Debug, Default)]
pub struct ShapesBuilder {
    pub coordinates: Vec<f64>,
    pub polygons: Vec<u32>,
    pub rings: Vec<u32>,
    pub polylines: Vec<u32>,
    pub points: Vec<u32>,
    pub polygon_ids: Vec<f64>,
    pub polyline_ids: Vec<f64>,
    pub point_ids: Vec<f64>,
    pub styles: Vec<f32>,
}

impl ShapesBuilder {
    pub fn new() -> ShapesBuilder {
        ShapesBuilder::default()
    }

    /// The number of coordinates so far, the index the next one takes.
    pub fn coordinate_count(&self) -> u32 {
        (self.coordinates.len() / 2) as u32
    }

    pub fn push_coordinate(&mut self, longitude: f64, latitude: f64) {
        self.coordinates.push(longitude);
        self.coordinates.push(latitude);
    }

    /// Adds a style record, returning its index.
    pub fn push_style(&mut self, record: &StyleRecord) -> u32 {
        let index = (self.styles.len() / STYLE_RECORD_FLOATS) as u32;
        self.styles.extend_from_slice(&record.encode());
        index
    }

    /// A zone of rings given as (longitude, latitude), the exterior first.
    pub fn push_polygon(&mut self, rings: &[&[[f64; 2]]], id: Option<f64>, style: u32) {
        let first_ring = (self.rings.len() / 2) as u32;
        for ring in rings {
            let start = self.coordinate_count();
            for p in ring.iter() {
                self.push_coordinate(p[0], p[1]);
            }
            self.rings.push(start);
            self.rings.push(ring.len() as u32);
        }
        self.polygons.extend_from_slice(&[first_ring, rings.len() as u32, style]);
        self.polygon_ids.push(id.unwrap_or(f64::NAN));
    }

    pub fn push_polyline(&mut self, coordinates: &[[f64; 2]], id: Option<f64>, style: u32) {
        let start = self.coordinate_count();
        for p in coordinates {
            self.push_coordinate(p[0], p[1]);
        }
        self.polylines.extend_from_slice(&[start, coordinates.len() as u32, style]);
        self.polyline_ids.push(id.unwrap_or(f64::NAN));
    }

    pub fn push_point(&mut self, coordinate: [f64; 2], id: Option<f64>, style: u32) {
        let index = self.coordinate_count();
        self.push_coordinate(coordinate[0], coordinate[1]);
        self.points.extend_from_slice(&[index, style]);
        self.point_ids.push(id.unwrap_or(f64::NAN));
    }

    pub fn finish(self) -> SourceContent {
        let mut ids = self.polygon_ids;
        ids.extend(self.polyline_ids);
        ids.extend(self.point_ids);
        SourceContent {
            coordinates: self.coordinates,
            polygons: self.polygons,
            rings: self.rings,
            polylines: self.polylines,
            points: self.points,
            ids,
            styles: self.styles,
        }
    }
}

/// How a zone draws (the Swift `ImmersiveMapPolygonStyle`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PolygonStyle {
    pub fill_color: [f32; 4],
    pub stroke_color: [f32; 4],
    pub stroke_width_points: f32,
}

impl Default for PolygonStyle {
    fn default() -> PolygonStyle {
        PolygonStyle { fill_color: [0.18, 0.47, 0.96, 0.25], stroke_color: [0.18, 0.47, 0.96, 1.0], stroke_width_points: 2.0 }
    }
}

/// How a line draws (the Swift `ImmersiveMapPolylineStyle`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PolylineStyle {
    pub color: [f32; 4],
    pub width_points: f32,
}

impl Default for PolylineStyle {
    fn default() -> PolylineStyle {
        PolylineStyle { color: [0.18, 0.47, 0.96, 1.0], width_points: 3.0 }
    }
}

/// How a point draws (the Swift `ImmersiveMapPointStyle`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointStyle {
    pub color: [f32; 4],
    pub radius_points: f32,
    pub stroke_color: [f32; 4],
    pub stroke_width_points: f32,
}

impl Default for PointStyle {
    fn default() -> PointStyle {
        PointStyle { color: [0.18, 0.47, 0.96, 1.0], radius_points: 4.0, stroke_color: [1.0, 1.0, 1.0, 1.0], stroke_width_points: 1.0 }
    }
}

/// The styles of a layer, one per kind of shape.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayerStyles {
    pub polygon: PolygonStyle,
    pub polyline: PolylineStyle,
    pub point: PointStyle,
}

fn rgba(values: &[f32]) -> [f32; 4] {
    [values[0], values[1], values[2], values[3]]
}

fn rgb(values: &[f32]) -> [f32; 3] {
    [values[0], values[1], values[2]]
}

/// A width or a radius as the Swift styles keep it: never below zero.
fn non_negative(value: f32) -> f32 {
    value.max(0.0)
}

impl LayerStyles {
    /// The 24 floats the TS side hands over, `None` when there are fewer.
    pub fn from_floats(values: &[f32]) -> Option<LayerStyles> {
        if values.len() < LAYER_STYLE_FLOATS {
            return None;
        }
        Some(LayerStyles {
            polygon: PolygonStyle {
                fill_color: rgba(&values[0..4]),
                stroke_color: rgba(&values[4..8]),
                stroke_width_points: non_negative(values[8]),
            },
            polyline: PolylineStyle { color: rgba(&values[9..13]), width_points: non_negative(values[13]) },
            point: PointStyle {
                color: rgba(&values[14..18]),
                radius_points: non_negative(values[18]),
                stroke_color: rgba(&values[19..23]),
                stroke_width_points: non_negative(values[23]),
            },
        })
    }

    pub fn to_floats(&self) -> [f32; LAYER_STYLE_FLOATS] {
        let mut out = [0.0; LAYER_STYLE_FLOATS];
        out[0..4].copy_from_slice(&self.polygon.fill_color);
        out[4..8].copy_from_slice(&self.polygon.stroke_color);
        out[8] = self.polygon.stroke_width_points;
        out[9..13].copy_from_slice(&self.polyline.color);
        out[13] = self.polyline.width_points;
        out[14..18].copy_from_slice(&self.point.color);
        out[18] = self.point.radius_points;
        out[19..23].copy_from_slice(&self.point.stroke_color);
        out[23] = self.point.stroke_width_points;
        out
    }

    /// The style a zone draws in: its own, or its feature's simplestyle
    /// laid over the layer's, or the layer's.
    pub fn polygon_style(&self, record: Option<StyleRecord>) -> PolygonStyle {
        match record {
            Some(StyleRecord::Polygon(style)) => style,
            Some(StyleRecord::Simple(simple)) => simple.polygon_style(&self.polygon).unwrap_or(self.polygon),
            _ => self.polygon,
        }
    }

    pub fn polyline_style(&self, record: Option<StyleRecord>) -> PolylineStyle {
        match record {
            Some(StyleRecord::Polyline(style)) => style,
            Some(StyleRecord::Simple(simple)) => simple.polyline_style(&self.polyline).unwrap_or(self.polyline),
            _ => self.polyline,
        }
    }

    pub fn point_style(&self, record: Option<StyleRecord>) -> PointStyle {
        match record {
            Some(StyleRecord::Point(style)) => style,
            Some(StyleRecord::Simple(simple)) => simple.point_style(&self.point).unwrap_or(self.point),
            _ => self.point,
        }
    }
}

/// What a GeoJSON feature's simplestyle properties state, each `None`
/// where the feature states nothing (the Swift `ImmersiveMapGeoJSON.SimpleStyle`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SimpleStyle {
    pub fill: Option<[f32; 3]>,
    pub fill_opacity: Option<f32>,
    pub stroke: Option<[f32; 3]>,
    pub stroke_opacity: Option<f32>,
    pub stroke_width: Option<f32>,
    pub marker_color: Option<[f32; 3]>,
    pub marker_radius: Option<f32>,
}

fn laid_over(rgb: Option<[f32; 3]>, opacity: Option<f32>, base: [f32; 4]) -> [f32; 4] {
    let [r, g, b] = rgb.unwrap_or([base[0], base[1], base[2]]);
    [r, g, b, opacity.unwrap_or(base[3])]
}

impl SimpleStyle {
    pub fn is_empty(&self) -> bool {
        *self == SimpleStyle::default()
    }

    /// Whether a zone reads anything of it (the Swift `polygonPart`).
    pub fn has_polygon_part(&self) -> bool {
        self.fill.is_some() || self.fill_opacity.is_some() || self.has_polyline_part()
    }

    /// Whether a line reads anything of it (`polylinePart`).
    pub fn has_polyline_part(&self) -> bool {
        self.stroke.is_some() || self.stroke_opacity.is_some() || self.stroke_width.is_some()
    }

    /// Whether a point reads anything of it (`pointPart`).
    pub fn has_point_part(&self) -> bool {
        self.marker_color.is_some() || self.marker_radius.is_some()
    }

    pub fn polygon_style(&self, base: &PolygonStyle) -> Option<PolygonStyle> {
        if !self.has_polygon_part() {
            return None;
        }
        Some(PolygonStyle {
            fill_color: laid_over(self.fill, self.fill_opacity, base.fill_color),
            stroke_color: laid_over(self.stroke, self.stroke_opacity, base.stroke_color),
            stroke_width_points: non_negative(self.stroke_width.unwrap_or(base.stroke_width_points)),
        })
    }

    pub fn polyline_style(&self, base: &PolylineStyle) -> Option<PolylineStyle> {
        if !self.has_polyline_part() {
            return None;
        }
        Some(PolylineStyle {
            color: laid_over(self.stroke, self.stroke_opacity, base.color),
            width_points: non_negative(self.stroke_width.unwrap_or(base.width_points)),
        })
    }

    pub fn point_style(&self, base: &PointStyle) -> Option<PointStyle> {
        if !self.has_point_part() {
            return None;
        }
        Some(PointStyle {
            color: laid_over(self.marker_color, None, base.color),
            radius_points: self.marker_radius.unwrap_or(base.radius_points),
            ..*base
        })
    }
}

/// One style record: a feature's simplestyle, or a shape's own style.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StyleRecord {
    Simple(SimpleStyle),
    Polygon(PolygonStyle),
    Polyline(PolylineStyle),
    Point(PointStyle),
}

impl StyleRecord {
    pub fn encode(&self) -> [f32; STYLE_RECORD_FLOATS] {
        let mut r = [0.0; STYLE_RECORD_FLOATS];
        match self {
            StyleRecord::Simple(s) => {
                r[0] = KIND_SIMPLE;
                let mut mask = 0u32;
                if let Some(c) = s.fill {
                    mask |= 1;
                    r[2..5].copy_from_slice(&c);
                }
                if let Some(v) = s.fill_opacity {
                    mask |= 2;
                    r[5] = v;
                }
                if let Some(c) = s.stroke {
                    mask |= 4;
                    r[6..9].copy_from_slice(&c);
                }
                if let Some(v) = s.stroke_opacity {
                    mask |= 8;
                    r[9] = v;
                }
                if let Some(v) = s.stroke_width {
                    mask |= 16;
                    r[10] = v;
                }
                if let Some(c) = s.marker_color {
                    mask |= 32;
                    r[11..14].copy_from_slice(&c);
                }
                if let Some(v) = s.marker_radius {
                    mask |= 64;
                    r[14] = v;
                }
                r[1] = mask as f32;
            }
            StyleRecord::Polygon(s) => {
                r[0] = KIND_POLYGON;
                r[1..5].copy_from_slice(&s.fill_color);
                r[5..9].copy_from_slice(&s.stroke_color);
                r[9] = s.stroke_width_points;
            }
            StyleRecord::Polyline(s) => {
                r[0] = KIND_POLYLINE;
                r[1..5].copy_from_slice(&s.color);
                r[5] = s.width_points;
            }
            StyleRecord::Point(s) => {
                r[0] = KIND_POINT;
                r[1..5].copy_from_slice(&s.color);
                r[5] = s.radius_points;
                r[6..10].copy_from_slice(&s.stroke_color);
                r[10] = s.stroke_width_points;
            }
        }
        r
    }

    /// A record from its floats, `None` for a kind no record has.
    pub fn decode(r: &[f32]) -> Option<StyleRecord> {
        if r.len() < STYLE_RECORD_FLOATS {
            return None;
        }
        let kind = r[0];
        if kind == KIND_SIMPLE {
            let mask = r[1] as u32;
            let has = |bit: u32| mask & bit != 0;
            Some(StyleRecord::Simple(SimpleStyle {
                fill: has(1).then(|| rgb(&r[2..5])),
                fill_opacity: has(2).then_some(r[5]),
                stroke: has(4).then(|| rgb(&r[6..9])),
                stroke_opacity: has(8).then_some(r[9]),
                stroke_width: has(16).then_some(r[10]),
                marker_color: has(32).then(|| rgb(&r[11..14])),
                marker_radius: has(64).then_some(r[14]),
            }))
        } else if kind == KIND_POLYGON {
            Some(StyleRecord::Polygon(PolygonStyle {
                fill_color: rgba(&r[1..5]),
                stroke_color: rgba(&r[5..9]),
                stroke_width_points: non_negative(r[9]),
            }))
        } else if kind == KIND_POLYLINE {
            Some(StyleRecord::Polyline(PolylineStyle { color: rgba(&r[1..5]), width_points: non_negative(r[5]) }))
        } else if kind == KIND_POINT {
            Some(StyleRecord::Point(PointStyle {
                color: rgba(&r[1..5]),
                radius_points: non_negative(r[5]),
                stroke_color: rgba(&r[6..10]),
                stroke_width_points: non_negative(r[10]),
            }))
        } else {
            None
        }
    }
}
