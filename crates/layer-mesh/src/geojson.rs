// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! GeoJSON (RFC 7946) read into the flat shapes of a source, a port of the
//! Swift `ImmersiveMapGeoJSON` and its object readers, by `serde_json` in one
//! streaming pass (decision B11): the positions go straight into the flat
//! coordinates, no tree of the document is built.
//!
//! A FeatureCollection, a Feature, a GeometryCollection or a bare geometry.
//! Positions are longitude, latitude, an altitude after them is ignored. A
//! feature's simplestyle properties are kept as a style record with the
//! shapes of its geometry that read them. A feature's shapes take its `id`
//! when it is a whole number, and the feature's place in the document
//! otherwise. A feature with a null geometry is skipped.
//!
//! The members of an object come in any order. A member read before the
//! object's `type` is known is kept as raw JSON and read when the object
//! ends. A feature's geometry is read as it comes, and its shapes are given
//! the feature's id and style once the feature ends, since its `id` and
//! `properties` may follow the geometry.

use std::fmt;

use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use serde_json::value::RawValue;

use crate::content::{NO_STYLE, ShapesBuilder, SimpleStyle, SourceContent, StyleRecord};

/// Why a GeoJSON document could not be read (the Swift `ImmersiveMapGeoJSONError`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GeoJsonError {
    /// The bytes are not JSON.
    InvalidJson,
    /// An object has no `type`, or one GeoJSON does not define.
    UnknownType(String),
    /// A geometry's `coordinates` are not shaped as its type requires, at
    /// the path given (the members from the document's root).
    InvalidCoordinates(String),
}

impl fmt::Display for GeoJsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GeoJsonError::InvalidJson => write!(f, "invalid JSON"),
            GeoJsonError::UnknownType(t) => write!(f, "unknown type {t}"),
            GeoJsonError::InvalidCoordinates(p) => write!(f, "invalid coordinates at {p}"),
        }
    }
}

/// Reads a GeoJSON document into the shapes of a source.
pub fn parse(bytes: &[u8]) -> Result<SourceContent, GeoJsonError> {
    let mut ctx = Context::default();
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let outcome = ObjectSeed { ctx: &mut ctx, path: "", null_skips: false }
        .deserialize(&mut deserializer)
        .and_then(|()| deserializer.end());
    match outcome {
        Ok(()) => Ok(ctx.shapes.finish()),
        Err(_) => match ctx.error.take() {
            // The Swift reader parses the JSON first: a document that is
            // not JSON is that, wherever its first GeoJSON fault lies.
            Some(error) if serde_json::from_slice::<IgnoredAny>(bytes).is_ok() => Err(error),
            _ => Err(GeoJsonError::InvalidJson),
        },
    }
}

// MARK: - The context

#[derive(Default)]
struct Context {
    shapes: ShapesBuilder,
    /// The feature each shape was read under, by its serial, `NO_STYLE` outside one.
    polygon_owners: Vec<u32>,
    polyline_owners: Vec<u32>,
    point_owners: Vec<u32>,
    /// The feature whose geometry is being read.
    owner: Option<u32>,
    next_serial: u32,
    /// How many features were read: a feature's place in the document.
    feature_count: u64,
    /// The first GeoJSON fault, the error the reader reports.
    error: Option<GeoJsonError>,
}

/// A feature being read: its place, its serial and where its shapes start.
#[derive(Clone, Copy)]
struct FeatureStart {
    serial: u32,
    index: u64,
    polygons: usize,
    polylines: usize,
    points: usize,
}

impl Context {
    fn fail<E: de::Error>(&mut self, error: GeoJsonError) -> E {
        if self.error.is_none() {
            self.error = Some(error);
        }
        E::custom("GeoJSON")
    }

    fn owner_word(&self) -> u32 {
        self.owner.unwrap_or(NO_STYLE)
    }

    fn push_point(&mut self, coordinate: u32) {
        self.shapes.points.extend_from_slice(&[coordinate, NO_STYLE]);
        self.shapes.point_ids.push(f64::NAN);
        self.point_owners.push(self.owner_word());
    }

    fn push_polyline(&mut self, start: u32) {
        let count = self.shapes.coordinate_count() - start;
        self.shapes.polylines.extend_from_slice(&[start, count, NO_STYLE]);
        self.shapes.polyline_ids.push(f64::NAN);
        self.polyline_owners.push(self.owner_word());
    }

    fn push_polygon(&mut self, first_ring: u32) {
        let count = (self.shapes.rings.len() / 2) as u32 - first_ring;
        self.shapes.polygons.extend_from_slice(&[first_ring, count, NO_STYLE]);
        self.shapes.polygon_ids.push(f64::NAN);
        self.polygon_owners.push(self.owner_word());
    }

    fn begin_feature(&mut self) -> FeatureStart {
        let start = FeatureStart {
            serial: self.next_serial,
            index: self.feature_count,
            polygons: self.polygon_owners.len(),
            polylines: self.polyline_owners.len(),
            points: self.point_owners.len(),
        };
        self.next_serial += 1;
        self.feature_count += 1;
        start
    }

    /// Gives the shapes the feature's geometry made its id and the parts
    /// of its style each kind reads. A nested feature's shapes keep theirs.
    fn end_feature(&mut self, start: FeatureStart, id: Option<u64>, style: &SimpleStyle) {
        let id = id.unwrap_or(start.index) as f64;
        let record = if style.is_empty() { NO_STYLE } else { self.shapes.push_style(&StyleRecord::Simple(*style)) };
        let part = |has: bool| if has { record } else { NO_STYLE };
        let (polygon_style, polyline_style, point_style) =
            (part(style.has_polygon_part()), part(style.has_polyline_part()), part(style.has_point_part()));
        for i in start.polygons..self.polygon_owners.len() {
            if self.polygon_owners[i] == start.serial {
                self.shapes.polygons[i * 3 + 2] = polygon_style;
                self.shapes.polygon_ids[i] = id;
            }
        }
        for i in start.polylines..self.polyline_owners.len() {
            if self.polyline_owners[i] == start.serial {
                self.shapes.polylines[i * 3 + 2] = polyline_style;
                self.shapes.polyline_ids[i] = id;
            }
        }
        for i in start.points..self.point_owners.len() {
            if self.point_owners[i] == start.serial {
                self.shapes.points[i * 2 + 1] = point_style;
                self.shapes.point_ids[i] = id;
            }
        }
    }
}

// MARK: - Paths

/// Where a value lies, built into a string only for an error.
#[derive(Clone, Copy)]
enum Path<'a> {
    Base(&'a str),
    Index(&'a Path<'a>, usize),
}

impl fmt::Display for Path<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Path::Base(base) => f.write_str(base),
            Path::Index(parent, index) => write!(f, "{parent}[{index}]"),
        }
    }
}

/// Every scalar a visitor does not take is `reject`ed.
macro_rules! reject_scalars {
    () => {
        fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
            Err(self.reject())
        }
        fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
            Err(self.reject())
        }
        fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
            Err(self.reject())
        }
        fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
            Err(self.reject())
        }
        fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
            Err(self.reject())
        }
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Err(self.reject())
        }
    };
}

// MARK: - Values without a context

/// A number as `NSNumber` reads one: a JSON number or a boolean.
struct MaybeNumber(Option<f64>);

impl<'de> Deserialize<'de> for MaybeNumber {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<MaybeNumber, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = MaybeNumber;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("any value")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<MaybeNumber, E> {
                Ok(MaybeNumber(Some(if v { 1.0 } else { 0.0 })))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<MaybeNumber, E> {
                Ok(MaybeNumber(Some(v as f64)))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<MaybeNumber, E> {
                Ok(MaybeNumber(Some(v as f64)))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<MaybeNumber, E> {
                Ok(MaybeNumber(Some(v)))
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<MaybeNumber, E> {
                Ok(MaybeNumber(None))
            }
            fn visit_unit<E: de::Error>(self) -> Result<MaybeNumber, E> {
                Ok(MaybeNumber(None))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<MaybeNumber, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(MaybeNumber(None))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<MaybeNumber, A::Error> {
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(MaybeNumber(None))
            }
        }
        deserializer.deserialize_any(V)
    }
}

/// An object's `type`: its string, or `None` for any other value.
struct TypeValue(Option<String>);

impl<'de> Deserialize<'de> for TypeValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<TypeValue, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = TypeValue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("any value")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<TypeValue, E> {
                Ok(TypeValue(Some(v.to_owned())))
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<TypeValue, E> {
                Ok(TypeValue(None))
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> Result<TypeValue, E> {
                Ok(TypeValue(None))
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> Result<TypeValue, E> {
                Ok(TypeValue(None))
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<TypeValue, E> {
                Ok(TypeValue(None))
            }
            fn visit_unit<E: de::Error>(self) -> Result<TypeValue, E> {
                Ok(TypeValue(None))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<TypeValue, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(TypeValue(None))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<TypeValue, A::Error> {
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(TypeValue(None))
            }
        }
        deserializer.deserialize_any(V)
    }
}

/// A feature's `id` as a shape's id: a whole number that is not negative
/// and below 2^64. A string, a fraction, a boolean or none is `None`.
struct FeatureId(Option<u64>);

impl<'de> Deserialize<'de> for FeatureId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<FeatureId, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = FeatureId;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("any value")
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<FeatureId, E> {
                Ok(FeatureId(Some(v)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<FeatureId, E> {
                Ok(FeatureId(u64::try_from(v).ok()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<FeatureId, E> {
                let whole = (0.0..18_446_744_073_709_551_615.0).contains(&v) && v.round() == v;
                Ok(FeatureId(whole.then_some(v as u64)))
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<FeatureId, E> {
                Ok(FeatureId(None))
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<FeatureId, E> {
                Ok(FeatureId(None))
            }
            fn visit_unit<E: de::Error>(self) -> Result<FeatureId, E> {
                Ok(FeatureId(None))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<FeatureId, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(FeatureId(None))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<FeatureId, A::Error> {
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(FeatureId(None))
            }
        }
        deserializer.deserialize_any(V)
    }
}

/// A simplestyle property's value: a number (a boolean counts, as an
/// `NSNumber`), a string, or anything else.
enum PropertyValue {
    Number(f64),
    Text(String),
    Other,
}

impl<'de> Deserialize<'de> for PropertyValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<PropertyValue, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = PropertyValue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("any value")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<PropertyValue, E> {
                Ok(PropertyValue::Number(if v { 1.0 } else { 0.0 }))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<PropertyValue, E> {
                Ok(PropertyValue::Number(v as f64))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<PropertyValue, E> {
                Ok(PropertyValue::Number(v as f64))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<PropertyValue, E> {
                Ok(PropertyValue::Number(v))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<PropertyValue, E> {
                Ok(PropertyValue::Text(v.to_owned()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<PropertyValue, E> {
                Ok(PropertyValue::Other)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<PropertyValue, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(PropertyValue::Other)
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<PropertyValue, A::Error> {
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(PropertyValue::Other)
            }
        }
        deserializer.deserialize_any(V)
    }
}

/// The simplestyle members of a feature's properties.
#[derive(Default)]
struct Properties {
    fill: Option<PropertyValue>,
    fill_opacity: Option<PropertyValue>,
    stroke: Option<PropertyValue>,
    stroke_opacity: Option<PropertyValue>,
    stroke_width: Option<PropertyValue>,
    marker_color: Option<PropertyValue>,
    marker_size: Option<PropertyValue>,
    marker_radius: Option<PropertyValue>,
}

/// A key of the properties, by the simplestyle member it names.
enum PropertyKey {
    Fill,
    FillOpacity,
    Stroke,
    StrokeOpacity,
    StrokeWidth,
    MarkerColor,
    MarkerSize,
    MarkerRadius,
    Other,
}

impl<'de> Deserialize<'de> for PropertyKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<PropertyKey, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = PropertyKey;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a key")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<PropertyKey, E> {
                Ok(match v {
                    "fill" => PropertyKey::Fill,
                    "fill-opacity" => PropertyKey::FillOpacity,
                    "stroke" => PropertyKey::Stroke,
                    "stroke-opacity" => PropertyKey::StrokeOpacity,
                    "stroke-width" => PropertyKey::StrokeWidth,
                    "marker-color" => PropertyKey::MarkerColor,
                    "marker-size" => PropertyKey::MarkerSize,
                    "marker-radius" => PropertyKey::MarkerRadius,
                    _ => PropertyKey::Other,
                })
            }
        }
        deserializer.deserialize_str(V)
    }
}

impl<'de> Deserialize<'de> for Properties {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Properties, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Properties;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("any value")
            }
            // Properties that are not an object state nothing.
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Properties, E> {
                Ok(Properties::default())
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> Result<Properties, E> {
                Ok(Properties::default())
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> Result<Properties, E> {
                Ok(Properties::default())
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Properties, E> {
                Ok(Properties::default())
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<Properties, E> {
                Ok(Properties::default())
            }
            fn visit_unit<E: de::Error>(self) -> Result<Properties, E> {
                Ok(Properties::default())
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Properties, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(Properties::default())
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Properties, A::Error> {
                let mut p = Properties::default();
                while let Some(key) = map.next_key::<PropertyKey>()? {
                    let slot = match key {
                        PropertyKey::Fill => &mut p.fill,
                        PropertyKey::FillOpacity => &mut p.fill_opacity,
                        PropertyKey::Stroke => &mut p.stroke,
                        PropertyKey::StrokeOpacity => &mut p.stroke_opacity,
                        PropertyKey::StrokeWidth => &mut p.stroke_width,
                        PropertyKey::MarkerColor => &mut p.marker_color,
                        PropertyKey::MarkerSize => &mut p.marker_size,
                        PropertyKey::MarkerRadius => &mut p.marker_radius,
                        PropertyKey::Other => {
                            map.next_value::<IgnoredAny>()?;
                            continue;
                        }
                    };
                    *slot = Some(map.next_value::<PropertyValue>()?);
                }
                Ok(p)
            }
        }
        deserializer.deserialize_any(V)
    }
}

impl Properties {
    /// The simplestyle of the properties (the Swift `ImmersiveMapGeoJSON.simpleStyle(of:)`).
    fn style(&self) -> SimpleStyle {
        let unit = |v: &Option<PropertyValue>| number(v).map(|x| x.clamp(0.0, 1.0));
        SimpleStyle {
            fill: color(&self.fill),
            fill_opacity: unit(&self.fill_opacity),
            stroke: color(&self.stroke),
            stroke_opacity: unit(&self.stroke_opacity),
            stroke_width: number(&self.stroke_width).map(|x| x.max(0.0)),
            marker_color: color(&self.marker_color),
            marker_radius: match number(&self.marker_radius) {
                Some(radius) => Some(radius.max(0.0)),
                None => match &self.marker_size {
                    Some(PropertyValue::Text(size)) => match size.as_str() {
                        "small" => Some(3.0),
                        "medium" => Some(5.0),
                        "large" => Some(8.0),
                        _ => None,
                    },
                    _ => None,
                },
            },
        }
    }
}

/// A number property: a JSON number or a numeric string, finite as a float.
fn number(value: &Option<PropertyValue>) -> Option<f32> {
    let parsed = match value {
        Some(PropertyValue::Number(n)) => *n as f32,
        Some(PropertyValue::Text(text)) => text.parse::<f32>().ok()?,
        _ => return None,
    };
    parsed.is_finite().then_some(parsed)
}

/// A `#rgb` or `#rrggbb` colour, the hash optional, spaces and tabs round it ignored.
pub fn parse_color(text: &str) -> Option<[f32; 3]> {
    let trimmed = text.trim_matches(|c: char| c.is_whitespace() && c != '\n' && c != '\r' && c != '\u{0B}' && c != '\u{0C}');
    let digits = trimmed.strip_prefix('#').unwrap_or(trimmed);
    let expanded: String = if digits.chars().count() == 3 { digits.chars().flat_map(|c| [c, c]).collect() } else { digits.to_owned() };
    if expanded.chars().count() != 6 {
        return None;
    }
    let value = u32::from_str_radix(&expanded, 16).ok()?;
    Some([((value >> 16) & 0xFF) as f32 / 255.0, ((value >> 8) & 0xFF) as f32 / 255.0, (value & 0xFF) as f32 / 255.0])
}

fn color(value: &Option<PropertyValue>) -> Option<[f32; 3]> {
    match value {
        Some(PropertyValue::Text(text)) => parse_color(text),
        _ => None,
    }
}

// MARK: - Objects

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    FeatureCollection,
    Feature,
    GeometryCollection,
    Point,
    MultiPoint,
    LineString,
    MultiLineString,
    Polygon,
    MultiPolygon,
}

impl Kind {
    fn named(name: &str) -> Option<Kind> {
        Some(match name {
            "FeatureCollection" => Kind::FeatureCollection,
            "Feature" => Kind::Feature,
            "GeometryCollection" => Kind::GeometryCollection,
            "Point" => Kind::Point,
            "MultiPoint" => Kind::MultiPoint,
            "LineString" => Kind::LineString,
            "MultiLineString" => Kind::MultiLineString,
            "Polygon" => Kind::Polygon,
            "MultiPolygon" => Kind::MultiPolygon,
            _ => return None,
        })
    }

    /// The member that holds what an object of the kind is made of.
    fn member(self) -> Member {
        match self {
            Kind::FeatureCollection => Member::Features,
            Kind::Feature => Member::Geometry,
            Kind::GeometryCollection => Member::Geometries,
            _ => Member::Coordinates,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Member {
    Coordinates,
    Features,
    Geometries,
    Geometry,
}

enum ObjectKey {
    Type,
    Properties,
    Id,
    Member(Member),
    Other,
}

impl<'de> Deserialize<'de> for ObjectKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<ObjectKey, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = ObjectKey;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a key")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<ObjectKey, E> {
                Ok(match v {
                    "type" => ObjectKey::Type,
                    "properties" => ObjectKey::Properties,
                    "id" => ObjectKey::Id,
                    "coordinates" => ObjectKey::Member(Member::Coordinates),
                    "features" => ObjectKey::Member(Member::Features),
                    "geometries" => ObjectKey::Member(Member::Geometries),
                    "geometry" => ObjectKey::Member(Member::Geometry),
                    _ => ObjectKey::Other,
                })
            }
        }
        deserializer.deserialize_str(V)
    }
}

/// One GeoJSON object at `path` (the members from the root, ending in a dot
/// unless empty). A null is passed over where `null_skips` (a feature's geometry).
struct ObjectSeed<'c, 'p> {
    ctx: &'c mut Context,
    path: &'p str,
    null_skips: bool,
}

impl ObjectSeed<'_, '_> {
    fn reject<E: de::Error>(self) -> E {
        let path = if self.path.is_empty() { "root".to_owned() } else { self.path.to_owned() };
        self.ctx.fail(GeoJsonError::UnknownType(path))
    }
}

impl<'de> DeserializeSeed<'de> for ObjectSeed<'_, '_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for ObjectSeed<'_, '_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a GeoJSON object")
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<(), E> {
        Err(self.reject())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<(), E> {
        Err(self.reject())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> Result<(), E> {
        Err(self.reject())
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<(), E> {
        Err(self.reject())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> Result<(), E> {
        Err(self.reject())
    }
    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        if self.null_skips { Ok(()) } else { Err(self.reject()) }
    }
    fn visit_seq<A: SeqAccess<'de>>(self, _: A) -> Result<(), A::Error> {
        Err(self.reject())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let ctx = self.ctx;
        let path = self.path;
        let mut kind: Option<Kind> = None;
        let mut feature: Option<FeatureStart> = None;
        let mut buffered: Vec<(Member, &'de RawValue)> = Vec::new();
        let mut properties: Option<Properties> = None;
        let mut id: Option<u64> = None;
        let mut has_member = false;
        while let Some(key) = map.next_key::<ObjectKey>()? {
            match key {
                ObjectKey::Type if kind.is_none() => match map.next_value::<TypeValue>()?.0 {
                    Some(name) => match Kind::named(&name) {
                        Some(k) => {
                            kind = Some(k);
                            if k == Kind::Feature {
                                feature = Some(ctx.begin_feature());
                            }
                        }
                        None => return Err(ctx.fail(GeoJsonError::UnknownType(name))),
                    },
                    None => {
                        let at = if path.is_empty() { "root".to_owned() } else { path.to_owned() };
                        return Err(ctx.fail(GeoJsonError::UnknownType(at)));
                    }
                },
                ObjectKey::Properties => properties = Some(map.next_value::<Properties>()?),
                ObjectKey::Id => id = map.next_value::<FeatureId>()?.0,
                ObjectKey::Member(member) => match kind {
                    None => buffered.push((member, map.next_value::<&'de RawValue>()?)),
                    Some(k) if k.member() == member => {
                        has_member = true;
                        map.next_value_seed(MemberSeed { ctx: &mut *ctx, kind: k, path, feature })?;
                    }
                    Some(_) => {
                        map.next_value::<IgnoredAny>()?;
                    }
                },
                ObjectKey::Type | ObjectKey::Other => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        let Some(kind) = kind else {
            let at = if path.is_empty() { "root".to_owned() } else { path.to_owned() };
            return Err(ctx.fail(GeoJsonError::UnknownType(at)));
        };
        for (member, raw) in buffered {
            if member != kind.member() {
                continue;
            }
            has_member = true;
            let mut deserializer = serde_json::Deserializer::from_str(raw.get());
            MemberSeed { ctx: &mut *ctx, kind, path, feature }.deserialize(&mut deserializer).map_err(de::Error::custom)?;
        }
        match kind {
            Kind::Feature => {
                if let Some(start) = feature {
                    let style = properties.map(|p| p.style()).unwrap_or_default();
                    ctx.end_feature(start, id, &style);
                }
            }
            _ if !has_member => {
                let name = match kind.member() {
                    Member::Features => "features",
                    Member::Geometries => "geometries",
                    _ => "coordinates",
                };
                return Err(ctx.fail(GeoJsonError::InvalidCoordinates(format!("{path}{name}"))));
            }
            _ => {}
        }
        Ok(())
    }
}

/// The member an object of `kind` is made of, read.
struct MemberSeed<'c, 'p> {
    ctx: &'c mut Context,
    kind: Kind,
    path: &'p str,
    feature: Option<FeatureStart>,
}

impl<'de> DeserializeSeed<'de> for MemberSeed<'_, '_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        let MemberSeed { ctx, kind, path, feature } = self;
        match kind {
            Kind::Feature => {
                let geometry_path = format!("{path}geometry.");
                let previous = ctx.owner;
                ctx.owner = feature.map(|f| f.serial);
                let outcome = ObjectSeed { ctx: &mut *ctx, path: &geometry_path, null_skips: true }.deserialize(deserializer);
                ctx.owner = previous;
                outcome
            }
            Kind::FeatureCollection => deserializer.deserialize_any(ObjectArraySeed { ctx, path, name: "features" }),
            Kind::GeometryCollection => deserializer.deserialize_any(ObjectArraySeed { ctx, path, name: "geometries" }),
            _ => {
                let base = format!("{path}coordinates");
                CoordinatesSeed { ctx, kind, path: Path::Base(&base) }.deserialize(deserializer)
            }
        }
    }
}

/// The objects of a FeatureCollection's `features` or a GeometryCollection's `geometries`.
struct ObjectArraySeed<'c, 'p> {
    ctx: &'c mut Context,
    path: &'p str,
    name: &'static str,
}

impl ObjectArraySeed<'_, '_> {
    fn reject<E: de::Error>(self) -> E {
        self.ctx.fail(GeoJsonError::InvalidCoordinates(format!("{}{}", self.path, self.name)))
    }
}

impl<'de> Visitor<'de> for ObjectArraySeed<'_, '_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an array of GeoJSON objects")
    }
    reject_scalars!();
    fn visit_map<A: MapAccess<'de>>(self, _: A) -> Result<(), A::Error> {
        Err(self.reject())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let mut index = 0;
        loop {
            let element_path = format!("{}{}[{index}].", self.path, self.name);
            let seed = ObjectSeed { ctx: &mut *self.ctx, path: &element_path, null_skips: false };
            if seq.next_element_seed(seed)?.is_none() {
                return Ok(());
            }
            index += 1;
        }
    }
}

// MARK: - Coordinates

/// A geometry's `coordinates`, by its kind.
struct CoordinatesSeed<'c, 'p> {
    ctx: &'c mut Context,
    kind: Kind,
    path: Path<'p>,
}

impl<'de> DeserializeSeed<'de> for CoordinatesSeed<'_, '_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        let CoordinatesSeed { ctx, kind, path } = self;
        match kind {
            Kind::Point => {
                let index = ctx.shapes.coordinate_count();
                PositionSeed { ctx: &mut *ctx, path }.deserialize(deserializer)?;
                ctx.push_point(index);
                Ok(())
            }
            Kind::MultiPoint => deserializer.deserialize_any(PositionsSeed { ctx, path, points: true }),
            Kind::LineString => {
                let start = ctx.shapes.coordinate_count();
                deserializer.deserialize_any(PositionsSeed { ctx: &mut *ctx, path, points: false })?;
                ctx.push_polyline(start);
                Ok(())
            }
            Kind::MultiLineString => deserializer.deserialize_any(ListSeed { ctx, path, of: List::Lines }),
            Kind::Polygon => deserializer.deserialize_any(RingsSeed { ctx, path }),
            Kind::MultiPolygon => deserializer.deserialize_any(ListSeed { ctx, path, of: List::Polygons }),
            _ => Ok(()),
        }
    }
}

/// One position: an array of at least two numbers, longitude then latitude.
struct PositionSeed<'c, 'p> {
    ctx: &'c mut Context,
    path: Path<'p>,
}

impl PositionSeed<'_, '_> {
    fn reject<E: de::Error>(self) -> E {
        let path = self.path.to_string();
        self.ctx.fail(GeoJsonError::InvalidCoordinates(path))
    }
}

impl<'de> DeserializeSeed<'de> for PositionSeed<'_, '_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for PositionSeed<'_, '_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a position")
    }
    reject_scalars!();
    fn visit_map<A: MapAccess<'de>>(self, _: A) -> Result<(), A::Error> {
        Err(self.reject())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let longitude = seq.next_element::<MaybeNumber>()?.and_then(|n| n.0);
        let latitude = match longitude {
            Some(_) => seq.next_element::<MaybeNumber>()?.and_then(|n| n.0),
            None => None,
        };
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        match (longitude, latitude) {
            (Some(longitude), Some(latitude)) if longitude.is_finite() && latitude.is_finite() => {
                self.ctx.shapes.push_coordinate(longitude, latitude);
                Ok(())
            }
            _ => Err(self.reject()),
        }
    }
}

/// An array of positions: a line's, a ring's, or a MultiPoint's, each of
/// whose positions is a point of its own (`points`).
struct PositionsSeed<'c, 'p> {
    ctx: &'c mut Context,
    path: Path<'p>,
    points: bool,
}

impl PositionsSeed<'_, '_> {
    fn reject<E: de::Error>(self) -> E {
        let path = self.path.to_string();
        self.ctx.fail(GeoJsonError::InvalidCoordinates(path))
    }
}

impl<'de> Visitor<'de> for PositionsSeed<'_, '_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an array of positions")
    }
    reject_scalars!();
    fn visit_map<A: MapAccess<'de>>(self, _: A) -> Result<(), A::Error> {
        Err(self.reject())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let mut index = 0;
        loop {
            let coordinate = self.ctx.shapes.coordinate_count();
            let path = Path::Index(&self.path, index);
            if seq.next_element_seed(PositionSeed { ctx: &mut *self.ctx, path })?.is_none() {
                return Ok(());
            }
            if self.points {
                self.ctx.push_point(coordinate);
            }
            index += 1;
        }
    }
}

/// A polygon's rings: the exterior, then the holes.
struct RingsSeed<'c, 'p> {
    ctx: &'c mut Context,
    path: Path<'p>,
}

impl RingsSeed<'_, '_> {
    fn reject<E: de::Error>(self) -> E {
        let path = self.path.to_string();
        self.ctx.fail(GeoJsonError::InvalidCoordinates(path))
    }
}

impl<'de> DeserializeSeed<'de> for RingsSeed<'_, '_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for RingsSeed<'_, '_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an array of rings")
    }
    reject_scalars!();
    fn visit_map<A: MapAccess<'de>>(self, _: A) -> Result<(), A::Error> {
        Err(self.reject())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let first_ring = (self.ctx.shapes.rings.len() / 2) as u32;
        let mut index = 0;
        loop {
            let start = self.ctx.shapes.coordinate_count();
            let path = Path::Index(&self.path, index);
            if seq.next_element_seed(PositionsAsSeed(PositionsSeed { ctx: &mut *self.ctx, path, points: false }))?.is_none() {
                break;
            }
            let count = self.ctx.shapes.coordinate_count() - start;
            self.ctx.shapes.rings.extend_from_slice(&[start, count]);
            index += 1;
        }
        if index == 0 {
            return Err(self.reject());
        }
        self.ctx.push_polygon(first_ring);
        Ok(())
    }
}

/// `PositionsSeed` as a seed of a sequence's element.
struct PositionsAsSeed<'c, 'p>(PositionsSeed<'c, 'p>);

impl<'de> DeserializeSeed<'de> for PositionsAsSeed<'_, '_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self.0)
    }
}

#[derive(Clone, Copy)]
enum List {
    Lines,
    Polygons,
}

/// A MultiLineString's lines or a MultiPolygon's polygons.
struct ListSeed<'c, 'p> {
    ctx: &'c mut Context,
    path: Path<'p>,
    of: List,
}

impl ListSeed<'_, '_> {
    fn reject<E: de::Error>(self) -> E {
        let path = self.path.to_string();
        self.ctx.fail(GeoJsonError::InvalidCoordinates(path))
    }
}

impl<'de> Visitor<'de> for ListSeed<'_, '_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an array")
    }
    reject_scalars!();
    fn visit_map<A: MapAccess<'de>>(self, _: A) -> Result<(), A::Error> {
        Err(self.reject())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let mut index = 0;
        loop {
            let path = Path::Index(&self.path, index);
            let found = match self.of {
                List::Lines => {
                    let start = self.ctx.shapes.coordinate_count();
                    let found = seq.next_element_seed(PositionsAsSeed(PositionsSeed { ctx: &mut *self.ctx, path, points: false }))?.is_some();
                    if found {
                        self.ctx.push_polyline(start);
                    }
                    found
                }
                List::Polygons => seq.next_element_seed(RingsSeed { ctx: &mut *self.ctx, path })?.is_some(),
            };
            if !found {
                return Ok(());
            }
            index += 1;
        }
    }
}
