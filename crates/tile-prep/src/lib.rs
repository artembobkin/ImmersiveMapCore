// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Prototype tile preparation: an MVT tile of the Protomaps basemap in, one
//! GPU-ready vertex and index buffer out, in draw order. Fills are
//! triangulated with earcut, lines become one quad per segment extruded in
//! the vertex stage by a width in pixels. The style is a fixed table here:
//! it exists to measure the pipeline, not to match the Swift engine's look.

use mvt::{GeometryType, Layer, Tile, ring_area2};

pub use mvt::DecodeError;

/// Words per vertex: x, y (tile space 0..1, y down), extrude x, y (pixels,
/// already scaled by half the line width), and the color as RGBA8.
pub const VERTEX_WORDS: usize = 5;

#[derive(Default)]
pub struct PreparedTile {
    /// `VERTEX_WORDS` u32 per vertex, the floats as their bits.
    pub vertices: Vec<u32>,
    pub indices: Vec<u32>,
    pub feature_count: u32,
}

#[derive(Clone, Copy)]
struct Paint {
    color: u32,
    /// The full line width in pixels, 0 for a fill.
    width: f32,
}

const fn rgb(hex: u32) -> u32 {
    let r = (hex >> 16) & 0xff;
    let g = (hex >> 8) & 0xff;
    let b = hex & 0xff;
    r | (g << 8) | (b << 16) | (0xff << 24)
}

/// The rules in draw order. A feature lands in at most one.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
enum Rule {
    Background,
    Earth,
    Landcover,
    Landuse,
    Water,
    WaterLine,
    Building,
    Boundary,
    RoadPath,
    RoadMinor,
    RoadMedium,
    RoadMajor,
    RoadHighway,
}

const RULE_COUNT: usize = Rule::RoadHighway as usize + 1;

fn line_scale(z: u8) -> f32 {
    ((z as f32 - 10.0) / 4.0).clamp(0.6, 1.8)
}

fn fill_rule(layer: &str, kind: &str) -> Option<(Rule, u32)> {
    Some(match layer {
        "earth" => (Rule::Earth, rgb(0xe9e5df)),
        "landcover" => (
            Rule::Landcover,
            match kind {
                "forest" => rgb(0xcfdfc0),
                "grassland" | "scrub" => rgb(0xdce6c6),
                "farmland" => rgb(0xe8e6d2),
                "glacier" => rgb(0xf6f9fb),
                "barren" => rgb(0xe8e1d3),
                "urban_area" => rgb(0xe2ddd6),
                _ => return None,
            },
        ),
        "landuse" => (
            Rule::Landuse,
            match kind {
                "park" | "garden" | "golf_course" | "playground" | "nature_reserve" | "grass" | "meadow"
                | "recreation_ground" | "village_green" => rgb(0xc9e0bb),
                "forest" | "wood" => rgb(0xbdd6ac),
                "cemetery" => rgb(0xd3dccb),
                "hospital" => rgb(0xf0dede),
                "school" | "university" | "college" | "kindergarten" => rgb(0xeee4d2),
                "industrial" | "railway" => rgb(0xdedadb),
                "pedestrian" | "dam" => rgb(0xebe7e1),
                "aerodrome" | "runway" | "taxiway" => rgb(0xdcdbe2),
                "beach" | "sand" => rgb(0xf2ebcf),
                "military" => rgb(0xe5dcd5),
                "zoo" | "theme_park" => rgb(0xd8e4c8),
                _ => return None,
            },
        ),
        "water" => (Rule::Water, rgb(0x9ec4e4)),
        "buildings" => (Rule::Building, rgb(0xd7d0c6)),
        _ => return None,
    })
}

fn line_rule(layer: &str, kind: &str, z: u8) -> Option<(Rule, Paint)> {
    let s = line_scale(z);
    let paint = |color, width: f32| Paint { color, width: width * s };
    Some(match layer {
        "water" | "physical_line" => (Rule::WaterLine, paint(rgb(0x9ec4e4), 1.2)),
        "boundaries" => (Rule::Boundary, paint(rgb(0xa8a2b8), 1.0)),
        "roads" => match kind {
            "highway" => (Rule::RoadHighway, paint(rgb(0xf3c58c), 4.0)),
            "major_road" => (Rule::RoadMajor, paint(rgb(0xfbe3ad), 3.2)),
            "medium_road" => (Rule::RoadMedium, paint(rgb(0xffffff), 2.4)),
            "minor_road" => (Rule::RoadMinor, paint(rgb(0xffffff), 1.6)),
            "path" | "other" => (Rule::RoadPath, paint(rgb(0xf7f5f2), 0.9)),
            _ => return None,
        },
        _ => return None,
    })
}

#[derive(Default)]
struct Bucket {
    vertices: Vec<u32>,
    indices: Vec<u32>,
}

impl Bucket {
    #[inline(always)]
    fn vertex(&mut self, x: f32, y: f32, ex: f32, ey: f32, color: u32) {
        self.vertices.extend_from_slice(&[x.to_bits(), y.to_bits(), ex.to_bits(), ey.to_bits(), color]);
    }

    fn vertex_count(&self) -> u32 {
        (self.vertices.len() / VERTEX_WORDS) as u32
    }
}

/// Scratch buffers reused across features and tiles.
#[derive(Default)]
pub struct Preparer {
    points: Vec<[i32; 2]>,
    part_ends: Vec<u32>,
    ring_points: Vec<[f64; 2]>,
    holes: Vec<u32>,
    triangles: Vec<u32>,
    earcut: earcut::Earcut<f64>,
    buckets: Vec<Bucket>,
}

impl Preparer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn prepare(&mut self, bytes: &[u8], z: u8) -> Result<PreparedTile, DecodeError> {
        let tile = Tile::decode(bytes)?;
        self.buckets.resize_with(RULE_COUNT, Bucket::default);
        for bucket in &mut self.buckets {
            bucket.vertices.clear();
            bucket.indices.clear();
        }
        // The tile's own water under everything, so a tile is opaque and
        // covers whatever coarser tile stands in under it.
        let background = &mut self.buckets[Rule::Background as usize];
        let water = rgb(0x9ec4e4);
        for (x, y) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
            background.vertex(x, y, 0.0, 0.0, water);
        }
        background.indices.extend_from_slice(&[0, 1, 2, 1, 3, 2]);

        let mut feature_count = 0u32;
        for layer in &tile.layers {
            feature_count += self.prepare_layer(layer, z)?;
        }

        let total_vertices: usize = self.buckets.iter().map(|b| b.vertices.len()).sum();
        let total_indices: usize = self.buckets.iter().map(|b| b.indices.len()).sum();
        let mut out = PreparedTile {
            vertices: Vec::with_capacity(total_vertices),
            indices: Vec::with_capacity(total_indices),
            feature_count,
        };
        for bucket in &self.buckets {
            let base = (out.vertices.len() / VERTEX_WORDS) as u32;
            out.vertices.extend_from_slice(&bucket.vertices);
            out.indices.extend(bucket.indices.iter().map(|i| i + base));
        }
        Ok(out)
    }

    fn prepare_layer(&mut self, layer: &Layer<'_>, z: u8) -> Result<u32, DecodeError> {
        let scale = 1.0 / layer.extent as f64;
        let mut count = 0;
        for feature in layer.features() {
            let feature = feature?;
            let kind = layer.property(&feature, "kind").and_then(|v| v.as_str()).unwrap_or("");
            match feature.geometry_type {
                GeometryType::Polygon => {
                    let Some((rule, color)) = fill_rule(layer.name, kind) else { continue };
                    feature.decode_geometry(&mut self.points, &mut self.part_ends)?;
                    self.fill(rule, color, scale);
                    count += 1;
                }
                GeometryType::LineString => {
                    let Some((rule, paint)) = line_rule(layer.name, kind, z) else { continue };
                    feature.decode_geometry(&mut self.points, &mut self.part_ends)?;
                    self.stroke(rule, paint, scale);
                    count += 1;
                }
                _ => {}
            }
        }
        Ok(count)
    }

    /// Groups the rings into polygons (an exterior ring, then its holes) and
    /// triangulates each.
    fn fill(&mut self, rule: Rule, color: u32, scale: f64) {
        self.ring_points.clear();
        self.holes.clear();
        let mut start = 0usize;
        for part in 0..self.part_ends.len() {
            let end = self.part_ends[part] as usize;
            let ring_start = start;
            start = end;
            if end - ring_start < 3 {
                continue;
            }
            let area = ring_area2(&self.points[ring_start..end]);
            if area == 0 {
                continue;
            }
            if area > 0 {
                self.flush_polygon(rule, color, scale);
            } else if self.ring_points.is_empty() {
                // A hole with no exterior ring before it: malformed, skip it.
                continue;
            } else {
                self.holes.push(self.ring_points.len() as u32);
            }
            self.ring_points.extend(self.points[ring_start..end].iter().map(|&[x, y]| [x as f64, y as f64]));
        }
        self.flush_polygon(rule, color, scale);
    }

    fn flush_polygon(&mut self, rule: Rule, color: u32, scale: f64) {
        if self.ring_points.len() < 3 {
            self.ring_points.clear();
            self.holes.clear();
            return;
        }
        self.earcut.earcut_slice(&self.ring_points, &self.holes, &mut self.triangles);
        let bucket = &mut self.buckets[rule as usize];
        let base = bucket.vertex_count();
        for &[x, y] in &self.ring_points {
            bucket.vertex((x * scale) as f32, (y * scale) as f32, 0.0, 0.0, color);
        }
        bucket.indices.extend(self.triangles.iter().map(|i| i + base));
        self.ring_points.clear();
        self.holes.clear();
    }

    /// One quad per segment, its corners pushed apart by the normal times half
    /// the width. The direction is taken in tile space, which is isotropic.
    fn stroke(&mut self, rule: Rule, paint: Paint, scale: f64) {
        let half = paint.width * 0.5;
        let bucket = &mut self.buckets[rule as usize];
        let mut start = 0usize;
        for &end in &self.part_ends {
            let end = end as usize;
            let line = &self.points[start..end];
            start = end;
            for pair in line.windows(2) {
                let [x0, y0] = pair[0];
                let [x1, y1] = pair[1];
                let (dx, dy) = ((x1 - x0) as f32, (y1 - y0) as f32);
                let length = (dx * dx + dy * dy).sqrt();
                if length == 0.0 {
                    continue;
                }
                let (nx, ny) = (-dy / length * half, dx / length * half);
                let (ax, ay) = ((x0 as f64 * scale) as f32, (y0 as f64 * scale) as f32);
                let (bx, by) = ((x1 as f64 * scale) as f32, (y1 as f64 * scale) as f32);
                let base = bucket.vertex_count();
                bucket.vertex(ax, ay, nx, ny, paint.color);
                bucket.vertex(ax, ay, -nx, -ny, paint.color);
                bucket.vertex(bx, by, nx, ny, paint.color);
                bucket.vertex(bx, by, -nx, -ny, paint.color);
                bucket.indices.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
            }
        }
    }
}
