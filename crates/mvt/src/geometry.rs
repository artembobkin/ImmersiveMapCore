// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The tile-space geometry of a feature as the engine's parser reads it: a
//! port of the Swift `MvtGeometryDecoder`. Rings are grouped into polygons
//! by the sign of their area (an exterior ring has a positive shoelace sum
//! in tile space, y south), and a malformed command stream yields an empty
//! result, so the feature drops and the tile survives.

/// A point in tile space (x east, y south), the layer's own units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Polygon {
    pub exterior: Vec<Point>,
    pub interiors: Vec<Vec<Point>>,
}

pub type MultiPolygon = Vec<Polygon>;
pub type LineString = Vec<Point>;
pub type MultiLineString = Vec<LineString>;
pub type MultiPoint = Vec<Point>;

/// A reader of the packed command integers.
struct Commands<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Commands<'_> {
    #[inline(always)]
    fn next(&mut self) -> Option<u32> {
        let mut value = 0u64;
        let mut shift = 0u32;
        loop {
            let byte = *self.bytes.get(self.position)?;
            self.position += 1;
            if shift < 64 {
                value |= ((byte & 0x7f) as u64) << shift;
            }
            if byte & 0x80 == 0 {
                return Some(value as u32);
            }
            shift += 7;
        }
    }

    /// At most how many integers can remain: one byte each at the least.
    fn remaining_upper_bound(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }
}

#[inline(always)]
fn zigzag(value: u32) -> i32 {
    ((value >> 1) as i32) ^ -((value & 1) as i32)
}

/// Twice the signed area with wrapping arithmetic: a crafted tile can place
/// vertices anywhere in the i32 range, and wrapping keeps such rings from
/// trapping the parse. For sane geometry the result is exact.
pub fn shoelace(points: &[Point]) -> i64 {
    if points.len() < 3 {
        return 0;
    }
    let mut sum = 0i64;
    let mut previous = points[points.len() - 1];
    for &point in points {
        sum = sum.wrapping_add(
            (previous.x as i64).wrapping_mul(point.y as i64).wrapping_sub((point.x as i64).wrapping_mul(previous.y as i64)),
        );
        previous = point;
    }
    sum
}

/// Reads one MoveTo(1) + LineTo(n) run, the start of a ring or a line.
fn read_run(commands: &mut Commands<'_>, cursor: &mut (i32, i32)) -> Option<Vec<Point>> {
    let dx = commands.next()?;
    let dy = commands.next()?;
    cursor.0 = cursor.0.wrapping_add(zigzag(dx));
    cursor.1 = cursor.1.wrapping_add(zigzag(dy));
    let mut points = vec![Point { x: cursor.0, y: cursor.1 }];
    let line = commands.next()?;
    let count = (line >> 3) as usize;
    if line & 7 != 2 || count == 0 {
        return None;
    }
    // Capped by what could remain, so a corrupt count cannot force a huge allocation.
    points.reserve(count.min(commands.remaining_upper_bound() / 2));
    for _ in 0..count {
        let dx = commands.next()?;
        let dy = commands.next()?;
        cursor.0 = cursor.0.wrapping_add(zigzag(dx));
        cursor.1 = cursor.1.wrapping_add(zigzag(dy));
        points.push(Point { x: cursor.0, y: cursor.1 });
    }
    Some(points)
}

pub fn decode_polygons(geometry: &[u8]) -> MultiPolygon {
    decode_polygons_checked(geometry).unwrap_or_default()
}

fn decode_polygons_checked(geometry: &[u8]) -> Option<MultiPolygon> {
    let mut commands = Commands { bytes: geometry, position: 0 };
    let mut cursor = (0i32, 0i32);
    let mut result = Vec::new();
    let mut exterior: Option<Vec<Point>> = None;
    let mut interiors: Vec<Vec<Point>> = Vec::new();
    while let Some(command) = commands.next() {
        if command & 7 != 1 || command >> 3 != 1 {
            return None;
        }
        let ring = read_run(&mut commands, &mut cursor)?;
        let close = commands.next()?;
        if close & 7 != 7 || close >> 3 != 1 {
            return None;
        }
        if shoelace(&ring) > 0 {
            if let Some(previous) = exterior.take() {
                result.push(Polygon { exterior: previous, interiors: std::mem::take(&mut interiors) });
            }
            exterior = Some(ring);
        } else {
            exterior.as_ref()?;
            interiors.push(ring);
        }
    }
    if let Some(last) = exterior {
        result.push(Polygon { exterior: last, interiors });
    }
    Some(result)
}

pub fn decode_lines(geometry: &[u8]) -> MultiLineString {
    let mut commands = Commands { bytes: geometry, position: 0 };
    let mut cursor = (0i32, 0i32);
    let mut result = Vec::new();
    while let Some(command) = commands.next() {
        if command & 7 != 1 || command >> 3 != 1 {
            return Vec::new();
        }
        match read_run(&mut commands, &mut cursor) {
            Some(line) => result.push(line),
            None => return Vec::new(),
        }
    }
    result
}

pub fn decode_points(geometry: &[u8]) -> MultiPoint {
    let mut commands = Commands { bytes: geometry, position: 0 };
    let mut cursor = (0i32, 0i32);
    let mut points = Vec::new();
    while let Some(command) = commands.next() {
        let id = command & 7;
        let count = command >> 3;
        if id == 1 || id == 2 {
            for _ in 0..count {
                // A short stream keeps the points decoded so far.
                let (Some(dx), Some(dy)) = (commands.next(), commands.next()) else { return points };
                cursor.0 = cursor.0.wrapping_add(zigzag(dx));
                cursor.1 = cursor.1.wrapping_add(zigzag(dy));
                points.push(Point { x: cursor.0, y: cursor.1 });
            }
        } else if id == 7 {
            continue;
        } else {
            break;
        }
    }
    points
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

    fn zz(v: i32) -> u64 {
        ((v << 1) ^ (v >> 31)) as u32 as u64
    }

    fn ring(out: &mut Vec<u8>, start: (i32, i32), deltas: &[(i32, i32)]) {
        varint(9, out);
        varint(zz(start.0), out);
        varint(zz(start.1), out);
        varint(((deltas.len() as u64) << 3) | 2, out);
        for &(dx, dy) in deltas {
            varint(zz(dx), out);
            varint(zz(dy), out);
        }
        varint(15, out);
    }

    #[test]
    fn rings_group_by_their_area_sign() {
        let mut g = Vec::new();
        ring(&mut g, (0, 0), &[(10, 0), (0, 10), (-10, 0)]); // exterior (clockwise on screen)
        ring(&mut g, (2, -2), &[(0, 6), (6, 0), (0, -6)]); // hole: starts at (2, 8)
        ring(&mut g, (18, -8), &[(10, 0), (0, 10), (-10, 0)]); // a second exterior
        let polygons = decode_polygons(&g);
        assert_eq!(polygons.len(), 2);
        assert_eq!(polygons[0].interiors.len(), 1);
        assert_eq!(polygons[0].interiors[0][0], Point { x: 2, y: 8 });
        assert!(polygons[1].interiors.is_empty());
    }

    #[test]
    fn a_hole_before_any_exterior_drops_the_feature() {
        let mut g = Vec::new();
        ring(&mut g, (0, 0), &[(0, 10), (10, 0), (0, -10)]);
        assert!(decode_polygons(&g).is_empty());
    }

    #[test]
    fn a_truncated_line_drops_the_feature_and_short_points_keep_what_was_read() {
        let mut g = Vec::new();
        varint(9, &mut g);
        varint(zz(1), &mut g);
        varint(zz(1), &mut g);
        varint((2 << 3) | 2, &mut g);
        varint(zz(1), &mut g);
        assert!(decode_lines(&g).is_empty());
        let mut p = Vec::new();
        varint((2 << 3) | 1, &mut p);
        varint(zz(3), &mut p);
        varint(zz(4), &mut p);
        varint(zz(1), &mut p);
        assert_eq!(decode_points(&p), vec![Point { x: 3, y: 4 }]);
    }
}
