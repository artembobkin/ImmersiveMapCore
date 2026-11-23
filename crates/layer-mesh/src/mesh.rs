// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! A layer's shapes turned into its GPU form, a port of the Swift
//! `LayerMeshBuilder`. Every coordinate goes to the Web-Mercator world (x
//! east and y south, 0 to 1 across the world), with the longitudes of a
//! ring or a line run on past 180 degrees rather than jumping back, so a
//! shape across the 180th meridian stays whole, and the latitude kept within
//! the Mercator range. A shape belongs to the chunk of its bounds' centre. A
//! zone is triangulated whole (earcut, holes included), its triangles and
//! every line's segments cut until no edge is longer than
//! `MAXIMUM_EDGE_LENGTH`, so on the globe a zone and a line follow the sphere
//! instead of cutting through it.

use std::collections::HashMap;

use crate::content::{LayerStyles, PointStyle, SourceView};

/// The tile zoom of the chunks.
pub const CHUNK_ZOOM: i32 = 6;
/// The longest edge a triangle or a segment keeps, in world units: about 150 km.
pub const MAXIMUM_EDGE_LENGTH: f64 = 1.0 / 256.0;
/// How many times a triangle may be split.
pub const MAXIMUM_SPLIT_DEPTH: u32 = 20;
/// A tile's side in render units.
pub const TILE_EXTENT: f64 = 4096.0;
/// The Mercator range's latitude limit in radians (`ImmersiveMapProjection.maxMercatorLatitude`).
pub fn max_mercator_latitude() -> f64 {
    2.0 * std::f64::consts::PI.exp().atan() - std::f64::consts::FRAC_PI_2
}

/// The rounding flag of a segment whose start is round.
pub const ROUND_START: u8 = 1;
/// The rounding flag of a segment whose end is round.
pub const ROUND_END: u8 = 2;

/// A zone's fill vertex: the position in its chunk's render units and the colour. 12 bytes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FillVertex {
    pub position: [f32; 2],
    pub color: [u8; 4],
}

/// One segment of a line, an instance: its ends, its width in points and
/// its colour, whose fourth byte is the rounding flags. 24 bytes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineSegment {
    pub start: [f32; 2],
    pub end: [f32; 2],
    pub width_points: f32,
    pub color: [u8; 4],
}

/// A disc on screen: a point, or a line of a single vertex. 24 bytes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Disc {
    pub position: [f32; 2],
    pub radius_points: f32,
    pub stroke_width_points: f32,
    pub color: [u8; 4],
    pub stroke_color: [u8; 4],
}

/// The shapes of a layer whose geometry lies in one tile of `CHUNK_ZOOM`,
/// in that tile's render units (x east, y north, 0 to 4096 across). The
/// ranges are into the layer's buffers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chunk {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub fill_index_start: u32,
    pub fill_index_count: u32,
    pub line_start: u32,
    pub line_count: u32,
    pub join_start: u32,
    pub join_count: u32,
    pub point_start: u32,
    pub point_count: u32,
    pub bounds_minimum: [f32; 2],
    pub bounds_maximum: [f32; 2],
    /// How far past its position a line or a point reaches on screen, in points.
    pub maximum_reach_points: f32,
}

/// A layer on the CPU, ready for its buffers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    pub fill_vertices: Vec<FillVertex>,
    pub fill_indices: Vec<u32>,
    pub line_segments: Vec<LineSegment>,
    /// The lines of a single vertex first, then the points.
    pub discs: Vec<Disc>,
    pub chunks: Vec<Chunk>,
}

impl MeshData {
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }
}

type World = [f64; 2];

/// A coordinate in the Mercator world, the longitude not wrapped.
pub fn world(longitude: f64, latitude: f64) -> World {
    let maximum = max_mercator_latitude().to_degrees();
    let latitude = latitude.clamp(-maximum, maximum).to_radians();
    let x = (longitude + 180.0) / 360.0;
    let y = (1.0 - (latitude.tan() + 1.0 / latitude.cos()).ln() / std::f64::consts::PI) / 2.0;
    [x, y]
}

fn distance_squared(a: World, b: World) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

/// A ring or a line in the world from interleaved (longitude, latitude),
/// each longitude run on from the one before it (from `reference` for the
/// first). Consecutive repeats and a ring's repeated last coordinate are dropped.
pub fn unwrapped_world(coordinates: &[f64], reference: Option<World>) -> Vec<World> {
    let mut points: Vec<World> = Vec::with_capacity(coordinates.len() / 2);
    let mut previous = reference;
    for pair in coordinates.as_chunks::<2>().0 {
        let (longitude, latitude) = (pair[0], pair[1]);
        if !latitude.is_finite() || !longitude.is_finite() {
            continue;
        }
        let mut point = world(longitude, latitude);
        if let Some(previous) = previous {
            point[0] += (previous[0] - point[0]).round();
        }
        if let Some(&last) = points.last()
            && distance_squared(last, point) < 1e-24
        {
            continue;
        }
        points.push(point);
        previous = Some(point);
    }
    if points.len() > 1 && distance_squared(points[0], points[points.len() - 1]) < 1e-24 {
        points.pop();
    }
    points
}

/// The whole worlds a shape is moved by so its bounds' centre lies in the first copy.
pub fn world_shift(points: &[World]) -> f64 {
    if points.is_empty() {
        return 0.0;
    }
    let minimum = points.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let maximum = points.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
    ((minimum + maximum) / 2.0).floor()
}

/// The chunk tile of a shape: the tile of `CHUNK_ZOOM` its bounds' centre lies in.
pub fn chunk_tile(points: &[World]) -> (i32, i32) {
    let mut minimum = points[0];
    let mut maximum = points[0];
    for p in points {
        minimum = [minimum[0].min(p[0]), minimum[1].min(p[1])];
        maximum = [maximum[0].max(p[0]), maximum[1].max(p[1])];
    }
    let count = f64::from(1 << CHUNK_ZOOM);
    let last = (1 << CHUNK_ZOOM) - 1;
    let x = ((minimum[0] + maximum[0]) / 2.0 * count).floor().clamp(0.0, f64::from(last)) as i32;
    let y = ((minimum[1] + maximum[1]) / 2.0 * count).floor().clamp(0.0, f64::from(last)) as i32;
    (x, y)
}

/// A colour in bytes: clamped to 0...1, rounded half away from zero.
pub fn packed_color(color: [f32; 4]) -> [u8; 4] {
    color.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)
}

struct ChunkBuilder {
    x: i32,
    y: i32,
    fill_vertices: Vec<FillVertex>,
    fill_indices: Vec<u32>,
    line_segments: Vec<LineSegment>,
    joins: Vec<Disc>,
    points: Vec<Disc>,
    bounds_minimum: [f32; 2],
    bounds_maximum: [f32; 2],
    maximum_reach_points: f32,
    tile_origin: World,
    units_per_world: f64,
}

impl ChunkBuilder {
    fn new(x: i32, y: i32) -> ChunkBuilder {
        let count = f64::from(1 << CHUNK_ZOOM);
        ChunkBuilder {
            x,
            y,
            fill_vertices: Vec::new(),
            fill_indices: Vec::new(),
            line_segments: Vec::new(),
            joins: Vec::new(),
            points: Vec::new(),
            bounds_minimum: [f32::INFINITY; 2],
            bounds_maximum: [f32::NEG_INFINITY; 2],
            maximum_reach_points: 0.0,
            tile_origin: [f64::from(x) / count, f64::from(y) / count],
            units_per_world: count * TILE_EXTENT,
        }
    }

    /// A world point in the chunk's render units, y north, the box grown round it.
    fn local(&mut self, world: World) -> [f32; 2] {
        let tx = (world[0] - self.tile_origin[0]) * self.units_per_world;
        let ty = (world[1] - self.tile_origin[1]) * self.units_per_world;
        let position = [tx as f32, (TILE_EXTENT - ty) as f32];
        self.bounds_minimum = [self.bounds_minimum[0].min(position[0]), self.bounds_minimum[1].min(position[1])];
        self.bounds_maximum = [self.bounds_maximum[0].max(position[0]), self.bounds_maximum[1].max(position[1])];
        position
    }

    fn append_fill(&mut self, earcut: &mut earcut::Earcut<f64>, exterior: &[World], holes: &[Vec<World>], color: [f32; 4]) {
        let mut rings: Vec<World> = exterior.to_vec();
        let mut hole_indices: Vec<u32> = Vec::with_capacity(holes.len());
        for hole in holes {
            hole_indices.push(rings.len() as u32);
            rings.extend_from_slice(hole);
        }
        let mut triangles: Vec<u32> = Vec::new();
        earcut.earcut_slice(&rings, &hole_indices, &mut triangles);
        if triangles.is_empty() {
            return;
        }
        let packed = packed_color(color);
        let mut cache: HashMap<[u64; 2], u32> = HashMap::new();
        for triangle in triangles.as_chunks::<3>().0 {
            let (a, b, c) = (rings[triangle[0] as usize], rings[triangle[1] as usize], rings[triangle[2] as usize]);
            split_triangle(a, b, c, 0, &mut |p, q, r| {
                // Counter-clockwise in render space (y north), the front
                // face the sphere's back-face culling keeps: in the world y
                // runs south, so a triangle clockwise there is
                // counter-clockwise on the map.
                let cross = (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
                if cross == 0.0 || cross.is_nan() {
                    return;
                }
                let (second, third) = if cross < 0.0 { (q, r) } else { (r, q) };
                for point in [p, second, third] {
                    let key = [point[0].to_bits(), point[1].to_bits()];
                    let index = match cache.get(&key) {
                        Some(&index) => index,
                        None => {
                            let index = self.fill_vertices.len() as u32;
                            let position = self.local(point);
                            self.fill_vertices.push(FillVertex { position, color: packed });
                            cache.insert(key, index);
                            index
                        }
                    };
                    self.fill_indices.push(index);
                }
            });
        }
    }

    /// A line, always solid: its colour's alpha only says whether it draws.
    fn append_line(&mut self, points: &[World], closed: bool, width_points: f32, color: [f32; 4]) {
        let packed = packed_color([color[0], color[1], color[2], 1.0]);
        self.maximum_reach_points = self.maximum_reach_points.max(width_points / 2.0);
        if points.len() < 2 {
            // A line of one vertex is a dot: there is no segment to round.
            for &point in points {
                let position = self.local(point);
                self.joins.push(Disc { position, radius_points: width_points / 2.0, stroke_width_points: 0.0, color: packed, stroke_color: packed });
            }
            return;
        }
        let mut segments: Vec<(World, World)> = points.windows(2).map(|w| (w[0], w[1])).collect();
        if closed {
            segments.push((points[points.len() - 1], points[0]));
        }
        for (start, end) in segments {
            let pieces = ((distance_squared(start, end).sqrt() / MAXIMUM_EDGE_LENGTH).ceil() as usize).max(1);
            let mut previous = start;
            for piece in 1..=pieces {
                let next = if piece == pieces {
                    end
                } else {
                    let t = piece as f64 / pieces as f64;
                    [start[0] + (end[0] - start[0]) * t, start[1] + (end[1] - start[1]) * t]
                };
                // Round at every vertex the shape states, flat where a long
                // segment was only cut to follow the sphere.
                let flags = (if piece == 1 { ROUND_START } else { 0 }) | (if piece == pieces { ROUND_END } else { 0 });
                let a = self.local(previous);
                let b = self.local(next);
                self.line_segments.push(LineSegment { start: a, end: b, width_points, color: [packed[0], packed[1], packed[2], flags] });
                previous = next;
            }
        }
    }

    fn append_point(&mut self, world: World, style: &PointStyle) {
        self.maximum_reach_points = self.maximum_reach_points.max(style.radius_points + style.stroke_width_points);
        let position = self.local(world);
        self.points.push(Disc {
            position,
            radius_points: style.radius_points,
            stroke_width_points: style.stroke_width_points,
            color: packed_color(style.color),
            stroke_color: packed_color(style.stroke_color),
        });
    }
}

/// A triangle cut at the midpoint of its longest edge until no edge is
/// longer than `MAXIMUM_EDGE_LENGTH`.
pub fn split_triangle(a: World, b: World, c: World, depth: u32, body: &mut dyn FnMut(World, World, World)) {
    let ab = distance_squared(a, b);
    let bc = distance_squared(b, c);
    let ca = distance_squared(c, a);
    let longest = ab.max(bc).max(ca);
    if longest <= MAXIMUM_EDGE_LENGTH * MAXIMUM_EDGE_LENGTH || depth >= MAXIMUM_SPLIT_DEPTH {
        body(a, b, c);
        return;
    }
    let mid = |p: World, q: World| [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0];
    if longest == ab {
        let m = mid(a, b);
        split_triangle(a, m, c, depth + 1, body);
        split_triangle(m, b, c, depth + 1, body);
    } else if longest == bc {
        let m = mid(b, c);
        split_triangle(a, b, m, depth + 1, body);
        split_triangle(a, m, c, depth + 1, body);
    } else {
        let m = mid(c, a);
        split_triangle(a, b, m, depth + 1, body);
        split_triangle(m, b, c, depth + 1, body);
    }
}

fn shifted(points: Vec<World>, shift: f64) -> Vec<World> {
    if shift == 0.0 { points } else { points.into_iter().map(|p| [p[0] - shift, p[1]]).collect() }
}

/// The mesh of a layer of `styles` drawing the shapes of `source`.
pub fn build(source: &SourceView<'_>, styles: &LayerStyles) -> MeshData {
    let mut chunks: Vec<ChunkBuilder> = Vec::new();
    let mut by_tile: HashMap<(i32, i32), usize> = HashMap::new();
    let mut chunk = |tile: (i32, i32), chunks: &mut Vec<ChunkBuilder>| -> usize {
        *by_tile.entry(tile).or_insert_with(|| {
            chunks.push(ChunkBuilder::new(tile.0, tile.1));
            chunks.len() - 1
        })
    };
    let ring_coordinates = |ring: usize| -> &[f64] {
        let start = source.rings[ring * 2] as usize;
        let count = source.rings[ring * 2 + 1] as usize;
        source.coordinates.get(start * 2..(start + count) * 2).unwrap_or(&[])
    };
    let mut earcut = earcut::Earcut::new();

    for polygon in source.polygons.as_chunks::<3>().0 {
        let (first_ring, ring_count) = (polygon[0] as usize, polygon[1] as usize);
        if ring_count == 0 || (first_ring + ring_count) * 2 > source.rings.len() {
            continue;
        }
        let style = styles.polygon_style(source.record(polygon[2]));
        let exterior = unwrapped_world(ring_coordinates(first_ring), None);
        if exterior.len() < 3 {
            continue;
        }
        let holes: Vec<Vec<World>> = (first_ring + 1..first_ring + ring_count)
            .map(|ring| unwrapped_world(ring_coordinates(ring), Some(exterior[0])))
            .filter(|ring| ring.len() >= 3)
            .collect();
        let shift = world_shift(&exterior);
        let exterior = shifted(exterior, shift);
        let holes: Vec<Vec<World>> = holes.into_iter().map(|ring| shifted(ring, shift)).collect();
        let index = chunk(chunk_tile(&exterior), &mut chunks);
        let builder = &mut chunks[index];
        if style.fill_color[3] > 0.0 {
            builder.append_fill(&mut earcut, &exterior, &holes, style.fill_color);
        }
        if style.stroke_color[3] > 0.0 && style.stroke_width_points > 0.0 {
            builder.append_line(&exterior, true, style.stroke_width_points, style.stroke_color);
            for hole in &holes {
                builder.append_line(hole, true, style.stroke_width_points, style.stroke_color);
            }
        }
    }

    for polyline in source.polylines.as_chunks::<3>().0 {
        let style = styles.polyline_style(source.record(polyline[2]));
        if style.color[3] <= 0.0 || style.width_points <= 0.0 {
            continue;
        }
        let (start, count) = (polyline[0] as usize, polyline[1] as usize);
        let line = unwrapped_world(source.coordinates.get(start * 2..(start + count) * 2).unwrap_or(&[]), None);
        if line.is_empty() {
            continue;
        }
        let shift = world_shift(&line);
        let line = shifted(line, shift);
        let index = chunk(chunk_tile(&line), &mut chunks);
        chunks[index].append_line(&line, false, style.width_points, style.color);
    }

    for point in source.points.as_chunks::<2>().0 {
        let style = styles.point_style(source.record(point[1]));
        if style.radius_points + style.stroke_width_points <= 0.0 || (style.color[3] <= 0.0 && style.stroke_color[3] <= 0.0) {
            continue;
        }
        let index = point[0] as usize;
        let Some(pair) = source.coordinates.get(index * 2..index * 2 + 2) else { continue };
        if !pair[0].is_finite() || !pair[1].is_finite() {
            continue;
        }
        let mut w = world(pair[0], pair[1]);
        w[0] -= w[0].floor();
        let index = chunk(chunk_tile(&[w]), &mut chunks);
        chunks[index].append_point(w, &style);
    }

    let mut data = MeshData::default();
    // The joins of every chunk first, then the points: the lines' joins
    // draw with the lines, before the points.
    let mut pending: Vec<(u32, u32, u32, u32, u32, u32)> = Vec::with_capacity(chunks.len());
    for builder in &chunks {
        let fill_base = data.fill_vertices.len() as u32;
        let fill_start = data.fill_indices.len() as u32;
        data.fill_vertices.extend_from_slice(&builder.fill_vertices);
        data.fill_indices.extend(builder.fill_indices.iter().map(|i| i + fill_base));
        let line_start = data.line_segments.len() as u32;
        data.line_segments.extend_from_slice(&builder.line_segments);
        let join_start = data.discs.len() as u32;
        data.discs.extend_from_slice(&builder.joins);
        pending.push((
            fill_start,
            builder.fill_indices.len() as u32,
            line_start,
            builder.line_segments.len() as u32,
            join_start,
            builder.joins.len() as u32,
        ));
    }
    for (builder, (fill_start, fill_count, line_start, line_count, join_start, join_count)) in chunks.iter().zip(pending) {
        let point_start = data.discs.len() as u32;
        data.discs.extend_from_slice(&builder.points);
        data.chunks.push(Chunk {
            x: builder.x,
            y: builder.y,
            z: CHUNK_ZOOM,
            fill_index_start: fill_start,
            fill_index_count: fill_count,
            line_start,
            line_count,
            join_start,
            join_count,
            point_start,
            point_count: builder.points.len() as u32,
            bounds_minimum: builder.bounds_minimum,
            bounds_maximum: builder.bounds_maximum,
            maximum_reach_points: builder.maximum_reach_points,
        });
    }
    data
}
