// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Decides which of a tile's building candidates are extruded, once every
//! footprint is known: duplicates go, a part nested inside a taller part of
//! the same building goes, an outline that ground-standing parts already
//! cover goes, a ground outline that engulfs the articulated parts of other
//! buildings is clamped to a pedestal, and a volume hidden inside the volumes
//! kept around it goes. A port of the Swift `BuildingExtrusionResolver`.
//!
//! One deliberate difference: the Swift code groups the candidates by
//! building in a dictionary, whose order changes from run to run, so the
//! order of its output did too. Here the groups keep the order of first
//! appearance, and a tile resolves the same way every time.

use std::collections::{HashMap, HashSet};

use super::candidate::BuildingExtrusionCandidate;
use crate::geom::V2;

#[derive(Debug, Clone, Copy)]
struct Bounds {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

impl Bounds {
    fn of(ring: &[V2]) -> Bounds {
        let Some(first) = ring.first() else { return Bounds { min_x: 0.0, min_y: 0.0, max_x: 0.0, max_y: 0.0 } };
        let mut b = Bounds { min_x: first.x, min_y: first.y, max_x: first.x, max_y: first.y };
        for p in &ring[1..] {
            b.min_x = b.min_x.min(p.x);
            b.min_y = b.min_y.min(p.y);
            b.max_x = b.max_x.max(p.x);
            b.max_y = b.max_y.max(p.y);
        }
        b
    }

    fn union(self, o: Bounds) -> Bounds {
        Bounds { min_x: self.min_x.min(o.min_x), min_y: self.min_y.min(o.min_y), max_x: self.max_x.max(o.max_x), max_y: self.max_y.max(o.max_y) }
    }

    fn intersects(self, o: Bounds) -> bool {
        self.min_x <= o.max_x && self.max_x >= o.min_x && self.min_y <= o.max_y && self.max_y >= o.min_y
    }

    fn is_inside_or_equal(self, o: Bounds) -> bool {
        let e = 0.001;
        self.min_x >= o.min_x - e && self.min_y >= o.min_y - e && self.max_x <= o.max_x + e && self.max_y <= o.max_y + e
    }
}

#[derive(Debug, Clone)]
struct Measured {
    candidate: BuildingExtrusionCandidate,
    bounds: Bounds,
    area: f32,
}

#[derive(Default)]
struct BoundsGrid {
    cells: HashMap<i64, Vec<usize>>,
}

impl BoundsGrid {
    const CELL: f32 = 256.0;

    fn cells_of(b: Bounds) -> impl Iterator<Item = i64> {
        let (c0, c1) = ((b.min_x / Self::CELL).floor() as i64, (b.max_x / Self::CELL).floor() as i64);
        let (r0, r1) = ((b.min_y / Self::CELL).floor() as i64, (b.max_y / Self::CELL).floor() as i64);
        (r0..=r1).flat_map(move |r| (c0..=c1).map(move |c| r.wrapping_mul(1_000_003).wrapping_add(c)))
    }

    fn insert(&mut self, index: usize, b: Bounds) {
        for key in Self::cells_of(b) {
            self.cells.entry(key).or_default().push(index);
        }
    }

    fn near(&self, b: Bounds) -> Vec<usize> {
        let mut found: Vec<usize> = Vec::new();
        for key in Self::cells_of(b) {
            if let Some(stored) = self.cells.get(&key) {
                found.extend_from_slice(stored);
            }
        }
        found.sort_unstable();
        found.dedup();
        found
    }

    fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

const HIDDEN_VOLUME_COVERAGE: f32 = 0.95;
const PART_COVERAGE_TO_DROP_OUTLINE: f32 = 0.9;
const COVERAGE_SAMPLES: usize = 256;
const DENSE_COVERAGE_SAMPLES: usize = 1024;
const MINIMUM_COVERAGE_SAMPLES: usize = 64;
// The R2 sequence steps (the plastic number), kept as the Swift source states them.
#[allow(clippy::excessive_precision)]
const R2_STEP_X: f32 = 0.754_877_67;
#[allow(clippy::excessive_precision)]
const R2_STEP_Y: f32 = 0.569_840_29;
const COVERAGE_BOUNDS_MARGIN: f32 = 0.03;
const ENVELOPE_CLAMP_SIGNIFICANCE_RATIO: f32 = 0.02;
const ENVELOPE_CLAMP_CHAIN_GAP_RATIO: f32 = 0.05;

fn area_magnitude(ring: &[V2]) -> f32 {
    if ring.len() < 3 {
        return 0.0;
    }
    let mut sum = 0f32;
    for i in 0..ring.len() {
        let n = (i + 1) % ring.len();
        sum += ring[i].x * ring[n].y - ring[n].x * ring[i].y;
    }
    sum.abs() * 0.5
}

pub fn resolve_exterior(candidates: Vec<BuildingExtrusionCandidate>) -> Vec<BuildingExtrusionCandidate> {
    if candidates.len() <= 1 {
        return candidates;
    }
    let measured: Vec<Measured> = candidates
        .into_iter()
        .map(|c| {
            let bounds = Bounds::of(&c.clipped_exterior);
            let area = area_magnitude(&c.clipped_exterior);
            Measured { candidate: c, bounds, area }
        })
        .collect();
    let mut order: Vec<u64> = Vec::new();
    let mut groups: HashMap<u64, Vec<Measured>> = HashMap::new();
    for m in measured {
        let id = m.candidate.building_id;
        groups
            .entry(id)
            .or_insert_with(|| {
                order.push(id);
                Vec::new()
            })
            .push(m);
    }
    let mut filtered: Vec<Measured> = Vec::new();
    for id in order {
        let group = groups.remove(&id).unwrap_or_default();
        filtered.extend(suppress_nested(deduplicate(group)));
    }
    let clamped = clamp_envelopes(drop_outlines_covered_by_parts(filtered));
    drop_volumes_hidden_inside_others(clamped).into_iter().map(|m| m.candidate).collect()
}

fn drop_volumes_hidden_inside_others(candidates: Vec<Measured>) -> Vec<Measured> {
    if candidates.len() <= 1 {
        return candidates;
    }
    let e = 0.01f32;
    let mut ordered: Vec<(usize, &Measured)> = candidates.iter().enumerate().collect();
    ordered.sort_by(|(ia, a), (ib, b)| {
        b.area
            .total_cmp(&a.area)
            .then(b.candidate.top_height.total_cmp(&a.candidate.top_height))
            .then(a.candidate.base_height.total_cmp(&b.candidate.base_height))
            .then(a.candidate.building_id.cmp(&b.candidate.building_id))
            .then(ia.cmp(ib))
    });
    let mut kept: Vec<&Measured> = Vec::new();
    let mut kept_offsets: Vec<usize> = Vec::new();
    let mut grid = BoundsGrid::default();
    for (offset, m) in ordered {
        let enclosing: Vec<&Measured> = grid
            .near(m.bounds)
            .into_iter()
            .map(|i| kept[i])
            .filter(|o| o.candidate.base_height <= m.candidate.base_height + e && o.candidate.top_height >= m.candidate.top_height - e && o.bounds.intersects(m.bounds))
            .collect();
        // Only a lid in the plane of another lid flickers.
        let shares_lid = enclosing.iter().any(|o| (o.candidate.top_height - m.candidate.top_height).abs() <= e);
        if !shares_lid || !is_covered(m, &enclosing, HIDDEN_VOLUME_COVERAGE) {
            grid.insert(kept.len(), m.bounds);
            kept.push(m);
            kept_offsets.push(offset);
        }
    }
    // Back to the tile's order.
    let mut back: Vec<(usize, Measured)> = kept_offsets.into_iter().zip(kept.into_iter().cloned()).collect();
    back.sort_by_key(|(o, _)| *o);
    back.into_iter().map(|(_, m)| m).collect()
}

fn drop_outlines_covered_by_parts(candidates: Vec<Measured>) -> Vec<Measured> {
    let base_epsilon = 0.5f32;
    let mut parts = BoundsGrid::default();
    for (i, m) in candidates.iter().enumerate() {
        if m.candidate.is_part && m.candidate.base_height <= base_epsilon {
            parts.insert(i, m.bounds);
        }
    }
    if parts.is_empty() {
        return candidates;
    }
    let keep: Vec<bool> = candidates
        .iter()
        .map(|m| {
            if m.candidate.is_part {
                return true;
            }
            let overlapping: Vec<&Measured> = parts.near(m.bounds).into_iter().map(|i| &candidates[i]).filter(|o| o.bounds.intersects(m.bounds)).collect();
            overlapping.is_empty() || !is_covered(m, &overlapping, PART_COVERAGE_TO_DROP_OUTLINE)
        })
        .collect();
    candidates.into_iter().zip(keep).filter(|(_, k)| *k).map(|(m, _)| m).collect()
}

fn in_footprint(point: V2, c: &BuildingExtrusionCandidate) -> bool {
    point_in_ring(point, &c.clipped_exterior) && !c.clipped_interiors.iter().any(|r| point_in_ring(point, r))
}

fn is_wholly_inside(m: &Measured, cover: &Measured) -> bool {
    if !m.bounds.is_inside_or_equal(cover.bounds) {
        return false;
    }
    let exterior = &m.candidate.clipped_exterior;
    if !exterior.iter().all(|&p| in_footprint(p, &cover.candidate)) {
        return false;
    }
    cover.candidate.clipped_interiors.iter().all(|courtyard| !courtyard.iter().any(|&p| point_in_ring(p, exterior)))
}

/// Whether at least `share` of the footprint lies inside the covers, measured
/// on R2 low-discrepancy samples over its bounds.
fn is_covered(m: &Measured, covers: &[&Measured], share: f32) -> bool {
    let b = m.bounds;
    let width = b.max_x - b.min_x;
    let height = b.max_y - b.min_y;
    if width <= 0.0 || height <= 0.0 || m.area <= 0.0 || covers.is_empty() {
        return false;
    }
    if covers.iter().any(|c| is_wholly_inside(m, c)) {
        return true;
    }
    let margin = COVERAGE_BOUNDS_MARGIN * width.max(height);
    let cover_bounds = covers[1..].iter().fold(covers[0].bounds, |acc, c| acc.union(c.bounds));
    if b.min_x < cover_bounds.min_x - margin || b.max_x > cover_bounds.max_x + margin || b.min_y < cover_bounds.min_y - margin || b.max_y > cover_bounds.max_y + margin {
        return false;
    }
    let fill = (m.area / (width * height)).min(1.0);
    let mut total = COVERAGE_SAMPLES;
    if fill * (total as f32) < MINIMUM_COVERAGE_SAMPLES as f32 {
        total = DENSE_COVERAGE_SAMPLES;
    }
    let allowed_misses = ((1.0 - share) * fill * total as f32).ceil() as usize;
    let (mut ux, mut uy) = (0.5f32, 0.5f32);
    let (mut inside, mut misses) = (0usize, 0usize);
    for _ in 0..total {
        ux += R2_STEP_X;
        uy += R2_STEP_Y;
        ux -= ux.floor();
        uy -= uy.floor();
        let point = V2::new(b.min_x + ux * width, b.min_y + uy * height);
        if !in_footprint(point, &m.candidate) {
            continue;
        }
        inside += 1;
        if !covers.iter().any(|c| in_footprint(point, &c.candidate)) {
            misses += 1;
            if misses > allowed_misses {
                return false;
            }
        }
    }
    inside > 0 && (inside - misses) as f32 >= share * inside as f32
}

fn clamp_envelopes(candidates: Vec<Measured>) -> Vec<Measured> {
    let min_enclosed = 4usize;
    if candidates.len() <= min_enclosed {
        return candidates;
    }
    let base_epsilon = 0.5f32;
    let stacked: Vec<usize> = (0..candidates.len()).filter(|&i| candidates[i].candidate.base_height > base_epsilon).collect();
    if stacked.len() < min_enclosed {
        return candidates;
    }
    candidates
        .iter()
        .map(|m| {
            let c = &m.candidate;
            if c.base_height > base_epsilon {
                return m.clone();
            }
            let tolerance = (ENVELOPE_CLAMP_CHAIN_GAP_RATIO * c.top_height).max(1.0);
            let mut spans: Vec<(f32, f32)> = Vec::new();
            let mut above_top = false;
            for &other in &stacked {
                let part = &candidates[other];
                if part.candidate.building_id == c.building_id
                    || part.area < ENVELOPE_CLAMP_SIGNIFICANCE_RATIO * m.area
                    || !part.bounds.is_inside_or_equal(m.bounds)
                    || !is_ring_contained(&part.candidate.clipped_exterior, &c.clipped_exterior)
                {
                    continue;
                }
                if part.candidate.base_height >= c.top_height - tolerance {
                    above_top = true;
                    break;
                }
                spans.push((part.candidate.base_height, part.candidate.top_height));
            }
            if above_top || spans.len() < min_enclosed {
                return m.clone();
            }
            let Some(mut level) = spans.iter().map(|s| s.1).reduce(f32::max) else { return m.clone() };
            let mut descended = true;
            while descended {
                descended = false;
                for &(base, top) in &spans {
                    if base < level && top + tolerance >= level {
                        level = base;
                        descended = true;
                    }
                }
            }
            let clamped_top = c.base_height.max(level);
            if clamped_top >= c.top_height {
                return m.clone();
            }
            let mut clamped = m.clone();
            clamped.candidate.top_height = clamped_top;
            clamped
        })
        .collect()
}

fn deduplicate(candidates: Vec<Measured>) -> Vec<Measured> {
    let mut seen: HashSet<(u64, Option<super::candidate::FootprintSignature>, u32, u32)> = HashSet::new();
    candidates
        .into_iter()
        .filter(|m| seen.insert((m.candidate.building_id, m.candidate.footprint_signature.clone(), m.candidate.base_height.to_bits(), m.candidate.top_height.to_bits())))
        .collect()
}

fn suppress_nested(candidates: Vec<Measured>) -> Vec<Measured> {
    if candidates.len() <= 1 {
        return candidates;
    }
    let mut sorted = candidates;
    sorted.sort_by(|a, b| {
        b.area
            .total_cmp(&a.area)
            .then(a.candidate.base_height.total_cmp(&b.candidate.base_height))
            .then(b.candidate.top_height.total_cmp(&a.candidate.top_height))
    });
    let mut kept: Vec<Measured> = Vec::with_capacity(sorted.len());
    for m in sorted {
        let nested = kept.iter().any(|container| {
            m.candidate.base_height >= container.candidate.base_height
                && m.candidate.top_height <= container.candidate.top_height
                && m.bounds.is_inside_or_equal(container.bounds)
                && is_ring_contained(&m.candidate.clipped_exterior, &container.candidate.clipped_exterior)
        });
        if !nested {
            kept.push(m);
        }
    }
    kept
}

fn is_ring_contained(ring: &[V2], container: &[V2]) -> bool {
    !ring.is_empty() && container.len() >= 3 && ring.iter().all(|&p| point_in_ring(p, container))
}

/// Even-odd point in ring, with points on an edge counted inside.
pub fn point_in_ring(point: V2, ring: &[V2]) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let e = 0.001;
    let mut inside = false;
    let mut previous = ring[ring.len() - 1];
    for &current in ring {
        if point_on_segment(point, previous, current, e) {
            return true;
        }
        let intersects = ((current.y > point.y) != (previous.y > point.y))
            && (point.x < (previous.x - current.x) * (point.y - current.y) / (previous.y - current.y) + current.x);
        if intersects {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

fn point_on_segment(p: V2, a: V2, b: V2, e: f32) -> bool {
    let ab = b - a;
    let ap = p - a;
    if (ab.x * ap.y - ab.y * ap.x).abs() > e {
        return false;
    }
    let dot = ap.dot(ab);
    dot >= -e && dot - ab.dot(ab) <= e
}
