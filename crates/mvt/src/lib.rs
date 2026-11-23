// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! A zero-copy Mapbox Vector Tile decoder. Layers and features borrow the
//! tile's bytes, and a feature's geometry is decoded only when asked for.
//! It knows nothing of the tile schema: no layer names, no style.

use std::fmt;

pub mod geometry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError(pub &'static str);

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MVT: {}", self.0)
    }
}

impl std::error::Error for DecodeError {}

type Result<T> = std::result::Result<T, DecodeError>;

/// A protobuf wire reader over a byte slice.
#[derive(Clone)]
struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, position: 0 }
    }

    fn is_at_end(&self) -> bool {
        self.position >= self.bytes.len()
    }

    #[inline(always)]
    fn varint(&mut self) -> Result<u64> {
        let mut value = 0u64;
        let mut shift = 0u32;
        loop {
            let byte = *self.bytes.get(self.position).ok_or(DecodeError("truncated varint"))?;
            self.position += 1;
            value |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
            if shift >= 64 {
                return Err(DecodeError("varint is too long"));
            }
        }
    }

    fn bytes(&mut self, length: usize) -> Result<&'a [u8]> {
        let end = self.position.checked_add(length).ok_or(DecodeError("length overflow"))?;
        let slice = self.bytes.get(self.position..end).ok_or(DecodeError("truncated field"))?;
        self.position = end;
        Ok(slice)
    }

    fn length_delimited(&mut self) -> Result<&'a [u8]> {
        let length = self.varint()? as usize;
        self.bytes(length)
    }

    fn fixed32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    fn fixed64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }

    /// Reads a field key: (field number, wire type).
    fn key(&mut self) -> Result<(u32, u8)> {
        let key = self.varint()?;
        Ok(((key >> 3) as u32, (key & 7) as u8))
    }

    fn skip(&mut self, wire_type: u8) -> Result<()> {
        match wire_type {
            0 => {
                self.varint()?;
            }
            1 => {
                self.bytes(8)?;
            }
            2 => {
                self.length_delimited()?;
            }
            5 => {
                self.bytes(4)?;
            }
            _ => return Err(DecodeError("unknown wire type")),
        }
        Ok(())
    }
}

#[inline(always)]
fn zigzag(value: u64) -> i64 {
    ((value >> 1) as i64) ^ -((value & 1) as i64)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value<'a> {
    String(&'a str),
    Float(f32),
    Double(f64),
    Int(i64),
    UInt(u64),
    Bool(bool),
    Null,
}

impl<'a> Value<'a> {
    fn decode(bytes: &'a [u8]) -> Result<Value<'a>> {
        let mut reader = Reader::new(bytes);
        let mut value = Value::Null;
        while !reader.is_at_end() {
            let (field, wire) = reader.key()?;
            value = match (field, wire) {
                (1, 2) => Value::String(
                    std::str::from_utf8(reader.length_delimited()?).map_err(|_| DecodeError("value is not UTF-8"))?,
                ),
                (2, 5) => Value::Float(f32::from_bits(reader.fixed32()?)),
                (3, 1) => Value::Double(f64::from_bits(reader.fixed64()?)),
                (4, 0) => Value::Int(reader.varint()? as i64),
                (5, 0) => Value::UInt(reader.varint()?),
                (6, 0) => Value::Int(zigzag(reader.varint()?)),
                (7, 0) => Value::Bool(reader.varint()? != 0),
                (_, w) => {
                    reader.skip(w)?;
                    continue;
                }
            };
        }
        Ok(value)
    }

    pub fn as_str(&self) -> Option<&'a str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Value::Float(v) => Some(v as f64),
            Value::Double(v) => Some(v),
            Value::Int(v) => Some(v as f64),
            Value::UInt(v) => Some(v as f64),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometryType {
    Unknown,
    Point,
    LineString,
    Polygon,
}

pub struct Tile<'a> {
    pub layers: Vec<Layer<'a>>,
}

impl<'a> Tile<'a> {
    pub fn decode(bytes: &'a [u8]) -> Result<Tile<'a>> {
        let mut reader = Reader::new(bytes);
        let mut layers = Vec::new();
        while !reader.is_at_end() {
            let (field, wire) = reader.key()?;
            if field == 3 && wire == 2 {
                layers.push(Layer::decode(reader.length_delimited()?)?);
            } else {
                reader.skip(wire)?;
            }
        }
        Ok(Tile { layers })
    }

    pub fn layer(&self, name: &str) -> Option<&Layer<'a>> {
        self.layers.iter().find(|l| l.name == name)
    }
}

pub struct Layer<'a> {
    pub name: &'a str,
    pub extent: u32,
    pub keys: Vec<&'a str>,
    pub values: Vec<Value<'a>>,
    features: Vec<&'a [u8]>,
}

impl<'a> Layer<'a> {
    fn decode(bytes: &'a [u8]) -> Result<Layer<'a>> {
        let mut reader = Reader::new(bytes);
        let mut layer = Layer { name: "", extent: 4096, keys: Vec::new(), values: Vec::new(), features: Vec::new() };
        while !reader.is_at_end() {
            let (field, wire) = reader.key()?;
            match (field, wire) {
                (1, 2) => {
                    layer.name = std::str::from_utf8(reader.length_delimited()?)
                        .map_err(|_| DecodeError("layer name is not UTF-8"))?
                }
                (2, 2) => layer.features.push(reader.length_delimited()?),
                (3, 2) => layer.keys.push(
                    std::str::from_utf8(reader.length_delimited()?).map_err(|_| DecodeError("key is not UTF-8"))?,
                ),
                (4, 2) => layer.values.push(Value::decode(reader.length_delimited()?)?),
                (5, 0) => layer.extent = reader.varint()? as u32,
                (_, w) => reader.skip(w)?,
            }
        }
        Ok(layer)
    }

    /// The feature's attributes as (key, value) pairs, in tag order. A tag
    /// whose key or value index is out of the layer's tables is skipped, and
    /// a later tag with the same key wins, as in the Swift decoder's map.
    pub fn attributes(&self, feature: &Feature<'a>) -> Vec<(&'a str, Value<'a>)> {
        let mut out: Vec<(&'a str, Value<'a>)> = Vec::new();
        let mut tags = Reader::new(feature.tags);
        while !tags.is_at_end() {
            let Ok(k) = tags.varint() else { break };
            let Ok(v) = tags.varint() else { break };
            let (Some(key), Some(value)) = (self.keys.get(k as usize), self.values.get(v as usize)) else { continue };
            if let Some(existing) = out.iter_mut().find(|(name, _)| name == key) {
                existing.1 = *value;
            } else {
                out.push((key, *value));
            }
        }
        out
    }

    pub fn feature_count(&self) -> usize {
        self.features.len()
    }

    pub fn features(&self) -> impl Iterator<Item = Result<Feature<'a>>> + '_ {
        self.features.iter().map(|bytes| Feature::decode(bytes))
    }

    /// The feature's value for `key`, looked up through the layer's tables.
    pub fn property(&self, feature: &Feature<'a>, key: &str) -> Option<Value<'a>> {
        let mut tags = Reader::new(feature.tags);
        while !tags.is_at_end() {
            let k = tags.varint().ok()? as usize;
            let v = tags.varint().ok()? as usize;
            if self.keys.get(k).is_some_and(|name| *name == key) {
                return self.values.get(v).copied();
            }
        }
        None
    }
}

pub struct Feature<'a> {
    pub id: Option<u64>,
    pub geometry_type: GeometryType,
    tags: &'a [u8],
    geometry: &'a [u8],
}

impl<'a> Feature<'a> {
    fn decode(bytes: &'a [u8]) -> Result<Feature<'a>> {
        let mut reader = Reader::new(bytes);
        let mut feature = Feature { id: None, geometry_type: GeometryType::Unknown, tags: &[], geometry: &[] };
        while !reader.is_at_end() {
            let (field, wire) = reader.key()?;
            match (field, wire) {
                (1, 0) => feature.id = Some(reader.varint()?),
                (2, 2) => feature.tags = reader.length_delimited()?,
                (3, 0) => {
                    feature.geometry_type = match reader.varint()? {
                        1 => GeometryType::Point,
                        2 => GeometryType::LineString,
                        3 => GeometryType::Polygon,
                        _ => GeometryType::Unknown,
                    }
                }
                (4, 2) => feature.geometry = reader.length_delimited()?,
                (_, w) => reader.skip(w)?,
            }
        }
        Ok(feature)
    }

    /// The packed geometry command stream, for `geometry::decode_*`.
    pub fn geometry_bytes(&self) -> &'a [u8] {
        self.geometry
    }

    /// The packed tag pairs (key index, value index), for walking a
    /// feature's attributes through its layer's tables.
    pub fn tag_bytes(&self) -> &'a [u8] {
        self.tags
    }

    /// Decodes the geometry into `points` (tile coordinates), appending one
    /// end index per part (a ring, a line or a point run) to `part_ends`.
    /// Both buffers are cleared first, so a caller reuses them across features.
    pub fn decode_geometry(&self, points: &mut Vec<[i32; 2]>, part_ends: &mut Vec<u32>) -> Result<()> {
        points.clear();
        part_ends.clear();
        let mut reader = Reader::new(self.geometry);
        let (mut x, mut y) = (0i32, 0i32);
        let closes = self.geometry_type == GeometryType::Polygon;
        while !reader.is_at_end() {
            let command = reader.varint()? as u32;
            let id = command & 7;
            let count = command >> 3;
            match id {
                1 | 2 => {
                    if id == 1 && !points.is_empty() && self.geometry_type != GeometryType::Point {
                        part_ends.push(points.len() as u32);
                    }
                    for _ in 0..count {
                        x = x.wrapping_add(zigzag(reader.varint()?) as i32);
                        y = y.wrapping_add(zigzag(reader.varint()?) as i32);
                        points.push([x, y]);
                    }
                }
                7 => {
                    // ClosePath: rings are closed implicitly by the triangulator.
                    if !closes {
                        return Err(DecodeError("ClosePath outside a polygon"));
                    }
                }
                _ => return Err(DecodeError("unknown geometry command")),
            }
        }
        if !points.is_empty() {
            part_ends.push(points.len() as u32);
        }
        Ok(())
    }
}

/// Twice the signed area of a ring in tile coordinates (y down). Positive for
/// an exterior ring, which MVT winds clockwise on screen.
pub fn ring_area2(ring: &[[i32; 2]]) -> i64 {
    let mut sum = 0i64;
    let n = ring.len();
    for i in 0..n {
        let [x0, y0] = ring[i];
        let [x1, y1] = ring[(i + 1) % n];
        sum += x0 as i64 * y1 as i64 - x1 as i64 * y0 as i64;
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    fn varint(mut v: u64, out: &mut Vec<u8>) {
        while v >= 0x80 {
            out.push((v as u8) | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
    }

    fn field_bytes(field: u32, payload: &[u8], out: &mut Vec<u8>) {
        varint(((field as u64) << 3) | 2, out);
        varint(payload.len() as u64, out);
        out.extend_from_slice(payload);
    }

    fn zz(v: i32) -> u64 {
        ((v << 1) ^ (v >> 31)) as u32 as u64
    }

    #[test]
    fn decodes_a_square_polygon_with_a_tag() {
        // Geometry: MoveTo(0,0) LineTo(+10,0)(0,+10)(-10,0) ClosePath.
        let mut geometry = Vec::new();
        varint((1 << 3) | 1, &mut geometry);
        varint(zz(0), &mut geometry);
        varint(zz(0), &mut geometry);
        varint((3 << 3) | 2, &mut geometry);
        for (dx, dy) in [(10, 0), (0, 10), (-10, 0)] {
            varint(zz(dx), &mut geometry);
            varint(zz(dy), &mut geometry);
        }
        varint((1 << 3) | 7, &mut geometry);

        let mut feature = Vec::new();
        field_bytes(2, &[0, 0], &mut feature);
        varint(3 << 3, &mut feature); // field 3, varint
        varint(3, &mut feature);
        field_bytes(4, &geometry, &mut feature);

        let mut value = Vec::new();
        field_bytes(1, b"water", &mut value);

        let mut layer = Vec::new();
        field_bytes(1, b"water", &mut layer);
        field_bytes(2, &feature, &mut layer);
        field_bytes(3, b"kind", &mut layer);
        field_bytes(4, &value, &mut layer);

        let mut tile_bytes = Vec::new();
        field_bytes(3, &layer, &mut tile_bytes);

        let tile = Tile::decode(&tile_bytes).unwrap();
        let layer = tile.layer("water").unwrap();
        let feature = layer.features().next().unwrap().unwrap();
        assert_eq!(feature.geometry_type, GeometryType::Polygon);
        assert_eq!(layer.property(&feature, "kind").and_then(|v| v.as_str()), Some("water"));
        let (mut points, mut ends) = (Vec::new(), Vec::new());
        feature.decode_geometry(&mut points, &mut ends).unwrap();
        assert_eq!(points, vec![[0, 0], [10, 0], [10, 10], [0, 10]]);
        assert_eq!(ends, vec![4]);
        assert!(ring_area2(&points) > 0);
    }
}
