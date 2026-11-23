// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Where the trees of a tile stand, a port of the Swift `TreeScatter`: one at
//! most in every cell of a lattice laid over the fills that grow them, moved,
//! turned and sized by a hash of the cell, so the same ground grows the same
//! trees at every parse and in every tile zoom. The lattice is fixed in the
//! world, in the tile units of `LATTICE_ZOOM`. A mapped tree stands where the
//! data puts it. Every tree draws a priority from its hash, and a tile's trees
//! are sorted by it, so the frame thins a tile by drawing a prefix.

use mvt::geometry::Polygon;

use super::TreeInstance;
use crate::coordinate_space::{TILE_EXTENT, TILE_EXTENT_F64, render_point};
use crate::geom::V2;
use crate::tile::Tile;

/// The tile zoom whose tile units the lattice is laid in.
pub const LATTICE_ZOOM: i32 = 16;
/// The tile zoom whose latitude sizes the cell and the trees: the building
/// coverage's grid, so a tile and its relatives read one latitude.
pub const LATITUDE_ZOOM: i32 = 14;
const JITTER_SPAN: f32 = 0.8;
const MAPPED_SALT: u32 = 0x7EE5_0001;
const EARTH_CIRCUMFERENCE_METERS: f64 = 40_075_016.686;

/// What a fill that grows trees says about them (the Swift `TreeFillStyle`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TreeFillParams {
    pub spacing_metres: f32,
    /// The lattice's salt: fills of one lattice share their cells.
    pub lattice: u32,
    pub minimum_height_metres: f32,
    pub maximum_height_metres: f32,
    pub road_clearance_metres: f32,
}

/// A tree mapped at a point (the Swift `TreePointStyle`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TreePointParams {
    pub minimum_height_metres: f32,
    pub maximum_height_metres: f32,
    pub crown_clearance_metres: f32,
}

#[derive(Debug, Clone)]
pub struct Area {
    pub polygons: Vec<Polygon>,
    pub style: TreeFillParams,
}

/// A road the trees keep off: its centreline in render space and half its
/// drawn width, in the tile units of `LATTICE_ZOOM`.
#[derive(Debug, Clone)]
pub struct Road {
    pub points: Vec<V2>,
    pub half_width: f32,
}

/// A polygon no tree stands in, in render space. Its holes stand trees again.
#[derive(Debug, Clone)]
pub struct Footprint {
    pub exterior: Vec<V2>,
    pub interiors: Vec<Vec<V2>>,
}

#[derive(Debug, Clone)]
pub struct MappedTree {
    pub tile_point: V2,
    pub style: TreePointParams,
}

#[derive(Default)]
pub struct Obstacles {
    pub footprints: Vec<Footprint>,
    pub roads: Vec<Road>,
}

/// A 32-bit mix of a cell and a salt: the one source of every tree's chance.
pub fn hash(x: u32, y: u32, salt: u32) -> u32 {
    let mut h = x.wrapping_mul(0x8DA6_B343);
    h ^= y.wrapping_mul(0xD816_3841);
    h ^= salt.wrapping_mul(0xCB1A_B31F);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^= h >> 16;
    h
}

fn jitter(h: u32) -> V2 {
    V2::new((h & 0xFF) as f32 / 255.0 - 0.5, ((h >> 8) & 0xFF) as f32 / 255.0 - 0.5) * JITTER_SPAN
}

/// Even-odd point in ring, in whatever space the ring is.
pub fn ring_contains(p: V2, ring: &[V2]) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut previous = ring[ring.len() - 1];
    for &current in ring {
        if (current.y > p.y) != (previous.y > p.y) {
            let x = previous.x + (p.y - previous.y) / (current.y - previous.y) * (current.x - previous.x);
            if p.x < x {
                inside = !inside;
            }
        }
        previous = current;
    }
    inside
}

pub fn polygon_contains(p: V2, exterior: &[V2], interiors: &[Vec<V2>]) -> bool {
    ring_contains(p, exterior) && !interiors.iter().any(|r| ring_contains(p, r))
}

struct CrownIndex {
    buckets: Vec<Vec<(V2, f32)>>,
}

impl CrownIndex {
    const GRID: usize = 32;

    fn bucket(p: V2) -> (usize, usize) {
        let scale = Self::GRID as f32 / TILE_EXTENT;
        (((p.x * scale) as i64).clamp(0, Self::GRID as i64 - 1) as usize, ((p.y * scale) as i64).clamp(0, Self::GRID as i64 - 1) as usize)
    }

    fn new(mapped: &[MappedTree], units_per_metre: f32) -> CrownIndex {
        let mut buckets = vec![Vec::new(); Self::GRID * Self::GRID];
        for tree in mapped {
            let radius = tree.style.crown_clearance_metres * units_per_metre;
            if radius <= 0.0 {
                continue;
            }
            let reach = V2::new(radius, radius);
            let (s, e) = (Self::bucket(tree.tile_point - reach), Self::bucket(tree.tile_point + reach));
            for y in s.1..=e.1 {
                for x in s.0..=e.0 {
                    buckets[y * Self::GRID + x].push((tree.tile_point, radius));
                }
            }
        }
        CrownIndex { buckets }
    }

    fn covers(&self, p: V2) -> bool {
        let (x, y) = Self::bucket(p);
        self.buckets[y * Self::GRID + x].iter().any(|(c, r)| (p - *c).length_squared() < r * r)
    }
}

struct Segment {
    start: V2,
    end: V2,
    half_width: f32,
}

struct ObstacleIndex<'a> {
    footprints: &'a [Footprint],
    bounds: Vec<(V2, V2)>,
    segments: Vec<Segment>,
    footprint_buckets: Vec<Vec<u32>>,
    segment_buckets: Vec<Vec<u32>>,
}

impl<'a> ObstacleIndex<'a> {
    const GRID: usize = 16;

    fn bucket(p: V2) -> (usize, usize) {
        let scale = Self::GRID as f32 / TILE_EXTENT;
        (((p.x * scale) as i64).clamp(0, Self::GRID as i64 - 1) as usize, ((p.y * scale) as i64).clamp(0, Self::GRID as i64 - 1) as usize)
    }

    fn each_bucket(min: V2, max: V2, mut body: impl FnMut(usize)) {
        let (s, e) = (Self::bucket(min), Self::bucket(max));
        for y in s.1..=e.1 {
            for x in s.0..=e.0 {
                body(y * Self::GRID + x);
            }
        }
    }

    fn new(obstacles: &'a Obstacles, road_width_scale: f32, maximum_clearance: f32) -> ObstacleIndex<'a> {
        let mut bounds = Vec::with_capacity(obstacles.footprints.len());
        let mut footprint_buckets = vec![Vec::new(); Self::GRID * Self::GRID];
        for (i, f) in obstacles.footprints.iter().enumerate() {
            let Some(&first) = f.exterior.first() else {
                bounds.push((V2::ZERO, V2::ZERO));
                continue;
            };
            let (mut min, mut max) = (first, first);
            for p in &f.exterior {
                min = V2::new(min.x.min(p.x), min.y.min(p.y));
                max = V2::new(max.x.max(p.x), max.y.max(p.y));
            }
            bounds.push((min, max));
            Self::each_bucket(min, max, |b| footprint_buckets[b].push(i as u32));
        }
        let mut segments = Vec::new();
        let mut segment_buckets = vec![Vec::new(); Self::GRID * Self::GRID];
        for road in &obstacles.roads {
            let half_width = road.half_width * road_width_scale;
            let pad = half_width + maximum_clearance.max(0.0);
            for pair in road.points.windows(2) {
                let (s, e) = (pair[0], pair[1]);
                let index = segments.len() as u32;
                segments.push(Segment { start: s, end: e, half_width });
                let min = V2::new(s.x.min(e.x) - pad, s.y.min(e.y) - pad);
                let max = V2::new(s.x.max(e.x) + pad, s.y.max(e.y) + pad);
                Self::each_bucket(min, max, |b| segment_buckets[b].push(index));
            }
        }
        ObstacleIndex { footprints: &obstacles.footprints, bounds, segments, footprint_buckets, segment_buckets }
    }

    fn covers(&self, p: V2, road_clearance: f32) -> bool {
        let (x, y) = Self::bucket(p);
        let b = y * Self::GRID + x;
        for &i in &self.footprint_buckets[b] {
            let (min, max) = self.bounds[i as usize];
            if p.x < min.x || p.x > max.x || p.y < min.y || p.y > max.y {
                continue;
            }
            let f = &self.footprints[i as usize];
            if polygon_contains(p, &f.exterior, &f.interiors) {
                return true;
            }
        }
        for &i in &self.segment_buckets[b] {
            let s = &self.segments[i as usize];
            let reach = s.half_width + road_clearance;
            let d = s.end - s.start;
            let len = d.length_squared();
            let dist = if len <= 1e-6 {
                (p - s.start).length_squared()
            } else {
                let t = ((p - s.start).dot(d) / len).clamp(0.0, 1.0);
                (p - (s.start + d * t)).length_squared()
            };
            if dist <= reach * reach {
                return true;
            }
        }
        false
    }
}

/// The trees of `areas` and the `mapped` ones in `tile`, sorted by priority
/// and cut at `maximum_count`, the scattered ones off `obstacles` and off the
/// mapped trees' crowns.
pub fn scatter(areas: &[Area], mapped: &[MappedTree], tile: Tile, obstacles: &Obstacles, maximum_count: usize) -> Vec<TreeInstance> {
    if (areas.is_empty() && mapped.is_empty()) || maximum_count == 0 {
        return Vec::new();
    }
    let latitude_tile = tile.parent_at(LATITUDE_ZOOM).unwrap_or(tile);
    let y = (latitude_tile.y as f64 + 0.5) / (1i64 << latitude_tile.z) as f64;
    let latitude = (std::f64::consts::PI * (1.0 - 2.0 * y)).sinh().atan();
    let units_per_metre_at_lattice = (TILE_EXTENT_F64 * 2f64.powi(LATTICE_ZOOM) / (EARTH_CIRCUMFERENCE_METERS * latitude.cos())) as f32;
    let units_per_metre = units_per_metre_at_lattice * 2f32.powi(tile.z - LATTICE_ZOOM);
    let lattice_per_tile_unit = 2f64.powi(LATTICE_ZOOM - tile.z);
    let origin = (tile.x as f64 * TILE_EXTENT_F64 * lattice_per_tile_unit, tile.y as f64 * TILE_EXTENT_F64 * lattice_per_tile_unit);
    let clearances: Vec<f32> = areas.iter().map(|a| a.style.road_clearance_metres * units_per_metre).collect();
    let index = ObstacleIndex::new(obstacles, (1.0 / lattice_per_tile_unit) as f32, clearances.iter().copied().fold(0.0, f32::max));
    let crowns = CrownIndex::new(mapped, units_per_metre);
    let mut candidates: Vec<(f32, TreeInstance)> = Vec::new();
    for tree in mapped {
        let lx = origin.0 + tree.tile_point.x as f64 * lattice_per_tile_unit;
        let ly = origin.1 + tree.tile_point.y as f64 * lattice_per_tile_unit;
        let h = hash(lx.floor() as i64 as u32, ly.floor() as i64 as u32, MAPPED_SALT);
        let second = hash(h, 0x9E37_79B9, MAPPED_SALT);
        let size = (second & 0xFFFF) as f32 / 65535.0;
        let height = tree.style.minimum_height_metres + (tree.style.maximum_height_metres - tree.style.minimum_height_metres) * size;
        candidates.push((
            ((second >> 16) & 0xFFFF) as f32 / 65535.0,
            TreeInstance::new(render_point(tree.tile_point), height * units_per_metre, (h >> 24) as f32 / 255.0, ((h >> 16) & 0xFF) as f32 / 255.0),
        ));
    }
    for (ai, area) in areas.iter().enumerate() {
        let cell = area.style.spacing_metres * units_per_metre_at_lattice;
        if !(cell > 0.0 && cell.is_finite()) {
            continue;
        }
        let salt = area.style.lattice;
        let clearance = clearances[ai];
        for polygon in &area.polygons {
            let exterior: Vec<V2> = polygon.exterior.iter().map(|p| V2::new(p.x as f32, p.y as f32)).collect();
            if exterior.len() < 3 {
                continue;
            }
            let interiors: Vec<Vec<V2>> = polygon.interiors.iter().map(|r| r.iter().map(|p| V2::new(p.x as f32, p.y as f32)).collect()).collect();
            let (mut min, mut max) = (exterior[0], exterior[0]);
            for p in &exterior {
                min = V2::new(min.x.min(p.x), min.y.min(p.y));
                max = V2::new(max.x.max(p.x), max.y.max(p.y));
            }
            min = V2::new(min.x.max(0.0), min.y.max(0.0));
            max = V2::new(max.x.min(TILE_EXTENT), max.y.min(TILE_EXTENT));
            if min.x >= max.x || min.y >= max.y {
                continue;
            }
            let c = cell as f64;
            let start = (((origin.0 + min.x as f64 * lattice_per_tile_unit) / c) as i64, ((origin.1 + min.y as f64 * lattice_per_tile_unit) / c) as i64);
            let end = (((origin.0 + max.x as f64 * lattice_per_tile_unit) / c) as i64, ((origin.1 + max.y as f64 * lattice_per_tile_unit) / c) as i64);
            if end.0 < start.0 || end.1 < start.1 || (end.0 - start.0 + 1) * (end.1 - start.1 + 1) >= 1 << 24 {
                continue;
            }
            for cy in start.1..=end.1 {
                for cx in start.0..=end.0 {
                    let h = hash(cx as u32, cy as u32, salt);
                    let j = jitter(h);
                    let lx = (cx as f64 + 0.5 + j.x as f64) * c;
                    let ly = (cy as f64 + 0.5 + j.y as f64) * c;
                    let tile_point = V2::new(((lx - origin.0) / lattice_per_tile_unit) as f32, ((ly - origin.1) / lattice_per_tile_unit) as f32);
                    if tile_point.x < 0.0 || tile_point.x >= TILE_EXTENT || tile_point.y < 0.0 || tile_point.y >= TILE_EXTENT || !polygon_contains(tile_point, &exterior, &interiors) {
                        continue;
                    }
                    let rp = render_point(tile_point);
                    if index.covers(rp, clearance) || crowns.covers(tile_point) {
                        continue;
                    }
                    let second = hash(h, 0x9E37_79B9, salt);
                    let size = (second & 0xFFFF) as f32 / 65535.0;
                    let height = area.style.minimum_height_metres + (area.style.maximum_height_metres - area.style.minimum_height_metres) * size;
                    candidates.push((
                        ((second >> 16) & 0xFFFF) as f32 / 65535.0,
                        TreeInstance::new(rp, height * units_per_metre, (h >> 24) as f32 / 255.0, ((h >> 16) & 0xFF) as f32 / 255.0),
                    ));
                }
            }
        }
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.packed.cmp(&b.1.packed)));
    candidates.truncate(maximum_count);
    candidates.into_iter().map(|(_, t)| t).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mvt::geometry::Point;

    fn wood() -> Area {
        let ring = vec![Point { x: 0, y: 0 }, Point { x: 4096, y: 0 }, Point { x: 4096, y: 4096 }, Point { x: 0, y: 4096 }];
        Area {
            polygons: vec![Polygon { exterior: ring, interiors: vec![] }],
            style: TreeFillParams { spacing_metres: 8.0, lattice: 1, minimum_height_metres: 8.0, maximum_height_metres: 16.0, road_clearance_metres: 2.0 },
        }
    }

    #[test]
    fn the_same_ground_grows_the_same_trees_in_a_tile_and_its_children() {
        let parent = Tile::new(15, 19807, 10243);
        let trees = scatter(&[wood()], &[], parent, &Obstacles::default(), usize::MAX);
        assert!(!trees.is_empty());
        // The north-west child holds the parent's north-west quarter: the same
        // trees stand there, at the child's coordinates (doubled, render y flipped).
        let child = Tile::new(16, 19807 * 2, 10243 * 2);
        let child_trees = scatter(&[wood()], &[], child, &Obstacles::default(), usize::MAX);
        let in_quarter: Vec<_> = trees.iter().filter(|t| t.position.x < 2048.0 && t.position.y >= 2048.0).collect();
        assert_eq!(in_quarter.len(), child_trees.len());
        let first = in_quarter.iter().min_by(|a, b| a.position.x.total_cmp(&b.position.x).then(a.position.y.total_cmp(&b.position.y))).unwrap();
        let matching = child_trees.iter().any(|c| (c.position.x - first.position.x * 2.0).abs() < 0.05 && (c.position.y - (first.position.y - 2048.0) * 2.0).abs() < 0.05);
        assert!(matching, "the parent's tree stands in its child too");
    }

    #[test]
    fn the_cut_keeps_the_highest_priorities_and_footprints_keep_trees_off() {
        let tile = Tile::new(16, 39614, 20486);
        let all = scatter(&[wood()], &[], tile, &Obstacles::default(), usize::MAX);
        let ten = scatter(&[wood()], &[], tile, &Obstacles::default(), 10);
        assert_eq!(ten, all[..10]);
        let building = Footprint { exterior: vec![V2::new(0.0, 0.0), V2::new(4096.0, 0.0), V2::new(4096.0, 2048.0), V2::new(0.0, 2048.0)], interiors: vec![] };
        let off = scatter(&[wood()], &[], tile, &Obstacles { footprints: vec![building], roads: vec![] }, usize::MAX);
        assert!(off.iter().all(|t| t.position.y >= 2048.0));
        assert!(off.len() < all.len());
    }
}
