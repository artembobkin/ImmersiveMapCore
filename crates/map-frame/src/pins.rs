// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The pin collision layout (the Swift `PinCollisionLayoutSolver`,
//! `PinCollisionMath` and `PinScreenHashGrid`). Pins push each other
//! apart as circles in screen space (Gauss-Seidel with a spring to the
//! anchor). Only a pin standing exactly on its geo point keeps the pin shape,
//! a displaced pin is a shrunken circle with a beam to the geo point. Dense
//! piles are laid out as a flower of petals, and members that do not fit are
//! hidden. Inter-frame state gives smooth, fps independent transitions.
//!
//! The layout is computed expression for expression in doubles, so a
//! TypeScript port of it gives the same layout. Swift computes in `Float`.
//!
//! Everything crosses the boundary as flat `f64` arrays: the pins in, the
//! items and the flowers out.
//!
//! The guards keep the negated comparisons of the Swift code (`guard x > 0`), which
//! also reject NaN, and the loops index several parallel arrays at once.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::needless_range_loop, clippy::manual_clamp, clippy::too_many_arguments)]

/// Doubles per input pin: id, x, y, visibility alpha, world x, world y,
/// screen size scale, draw priority, draw order, flags (1 selected, 2 the
/// event cluster policy).
pub const PIN_INPUT_STRIDE: usize = 10;
/// Doubles per output item: the input index, x, y, visibility alpha, display
/// scale, morph, 1 for a flower petal.
pub const PIN_ITEM_STRIDE: usize = 7;
/// Doubles of the output header: item count, flower count, 1 while animating.
pub const PIN_OUTPUT_HEADER: usize = 3;

pub const FLAG_SELECTED: u32 = 1;
pub const FLAG_EVENT: u32 = 2;

// MARK: Math (the Swift `PinCollisionMath`)

pub mod math {
    use std::sync::OnceLock;

    /// The transcendental functions. In Wasm with the `js-math` feature
    /// they are the JS engine's own (`Math`), so the layout matches a
    /// TypeScript solver bit for bit in every browser (engines differ in
    /// the last bit of `sin` and `cos`, and a long contact chain amplifies
    /// it). Otherwise Rust's.
    #[cfg(all(target_arch = "wasm32", feature = "js-math"))]
    mod transcendental {
        pub use js_sys::Math::{cos, log2, pow, sin};
    }

    #[cfg(not(all(target_arch = "wasm32", feature = "js-math")))]
    mod transcendental {
        pub fn cos(x: f64) -> f64 {
            x.cos()
        }
        pub fn sin(x: f64) -> f64 {
            x.sin()
        }
        pub fn pow(x: f64, y: f64) -> f64 {
            x.powf(y)
        }
        pub fn log2(x: f64) -> f64 {
            x.log2()
        }
    }

    pub use transcendental::{log2, pow};

    /// The hash of `stable_unit_angle` takes this many values.
    const HASH_ANGLES: usize = 6283;

    /// Every angle the solver takes a sine or cosine of comes from a small
    /// fixed set: the hash angles, the petal slots and the ring spacing.
    /// They are computed once, so a frame calls no trigonometry.
    struct Trigonometry {
        hash_cos: Vec<f64>,
        hash_sin: Vec<f64>,
        /// By petal count (1 to 7) and slot index.
        petal_cos: [[f64; MAX_FLOWER_PETALS]; MAX_FLOWER_PETALS + 1],
        petal_sin: [[f64; MAX_FLOWER_PETALS]; MAX_FLOWER_PETALS + 1],
        /// sin(pi / count) by petal count.
        ring_sin: [f64; MAX_FLOWER_PETALS + 1],
    }

    fn trigonometry() -> &'static Trigonometry {
        static TABLES: OnceLock<Trigonometry> = OnceLock::new();
        TABLES.get_or_init(|| {
            let angles = (0..HASH_ANGLES).map(|k| k as f64 * 0.001);
            let mut tables = Trigonometry {
                hash_cos: angles.clone().map(transcendental::cos).collect(),
                hash_sin: angles.map(transcendental::sin).collect(),
                petal_cos: [[0.0; MAX_FLOWER_PETALS]; MAX_FLOWER_PETALS + 1],
                petal_sin: [[0.0; MAX_FLOWER_PETALS]; MAX_FLOWER_PETALS + 1],
                ring_sin: [0.0; MAX_FLOWER_PETALS + 1],
            };
            for count in 1..=MAX_FLOWER_PETALS {
                tables.ring_sin[count] = transcendental::sin(std::f64::consts::PI / count as f64);
                for index in 0..count {
                    let angle = flower_petal_angle(index, count);
                    tables.petal_cos[count][index] = transcendental::cos(angle);
                    tables.petal_sin[count][index] = transcendental::sin(angle);
                }
            }
            tables
        })
    }

    pub const OFFSET_SNAP_EPSILON_PX: f64 = 0.5;
    pub const SCALE_SNAP_EPSILON: f64 = 0.01;
    pub const GROUPING_RADIUS_SCALE: f64 = 0.35;
    pub const GROUPING_HYSTERESIS_RATIO: f64 = 1.3;
    pub const CLUSTER_CROSSFADE_SECONDS: f64 = 0.25;
    pub const MAX_FRAME_DELTA_SECONDS: f64 = 0.25;
    pub const BADGE_FADE_START_SCALE: f64 = 0.8;
    pub const BADGE_FADE_END_SCALE: f64 = 0.95;
    pub const START_JITTER_SCALE: f64 = 0.05;
    pub const REST_SNAP_RADIUS_PX: f64 = 1.5;
    pub const START_JITTER_SALT: u64 = 0x517cc1b727220a95;
    pub const DISPLACED_CIRCLE_SCALE: f64 = 0.7;
    pub const STATIC_TOUCH_DEPTH_PX: f64 = 8.0;
    pub const MAX_FLOWER_PETALS: usize = 7;
    pub const FLOWER_COMPACTNESS_LIMIT_SCALE: f64 = 2.5;
    pub const DISPLACED_MORPH_START_PX: f64 = 2.0;
    pub const DISPLACED_MORPH_END_PX: f64 = 10.0;

    pub fn clamp(value: f64, low: f64, high: f64) -> f64 {
        value.max(low).min(high)
    }

    pub fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
        let t = clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// Fraction of the path toward the target per frame, normalized to the frame duration.
    pub fn smoothing_factor(smoothing: f64, delta_seconds: f64) -> f64 {
        let clamped = clamp(smoothing, 0.0, 1.0);
        if !(delta_seconds > 0.0) {
            return 0.0;
        }
        1.0 - pow(1.0 - clamped, delta_seconds * 60.0)
    }

    /// The angle of the deterministic separation direction for two keys, independent of their order.
    pub fn stable_unit_angle(id_a: u64, id_b: u64) -> f64 {
        stable_unit_index(id_a, id_b) as f64 * 0.001
    }

    /// The angle of `stable_unit_angle` in thousandths of a radian.
    fn stable_unit_index(id_a: u64, id_b: u64) -> usize {
        let mut hash: u64 = 0xcbf29ce484222325;
        for id in [id_a.min(id_b), id_a.max(id_b)] {
            let mut value = id;
            for _ in 0..8 {
                hash ^= value & 0xff;
                hash = hash.wrapping_mul(0x100000001b3);
                value >>= 8;
            }
        }
        (hash % HASH_ANGLES as u64) as usize
    }

    /// Deterministic separation direction for a pair of nodes whose centers coincide.
    pub fn stable_unit_direction(id_a: u64, id_b: u64) -> (f64, f64) {
        let k = stable_unit_index(id_a, id_b);
        let tables = trigonometry();
        (tables.hash_cos[k], tables.hash_sin[k])
    }

    /// The cosine and sine of a petal slot's angle (`flower_petal_angle`).
    pub fn petal_direction(index: usize, petal_count: usize) -> (f64, f64) {
        let tables = trigonometry();
        (tables.petal_cos[petal_count][index], tables.petal_sin[petal_count][index])
    }

    pub fn static_touch_morph(overlap_depth: f64) -> f64 {
        smoothstep(0.0, STATIC_TOUCH_DEPTH_PX, overlap_depth)
    }

    pub fn scale_cap(morph: f64) -> f64 {
        1.0 + (DISPLACED_CIRCLE_SCALE - 1.0) * clamp(morph, 0.0, 1.0)
    }

    pub fn required_scale(distance: f64, body_radius: f64, other_body_radius: f64, padding: f64, other_is_rigid: bool) -> f64 {
        let available = distance - 2.0 * padding;
        if !(body_radius > 0.0) {
            return 1.0;
        }
        let scale = if other_is_rigid { (available - other_body_radius) / body_radius } else { available / (body_radius + other_body_radius) };
        clamp(scale, 0.0, 1.0)
    }

    pub fn flower_ring_radius(petal_body_radius: f64, petal_count: usize) -> f64 {
        if petal_count <= 1 {
            return 0.0;
        }
        petal_body_radius / trigonometry().ring_sin[petal_count.min(MAX_FLOWER_PETALS)]
    }

    pub fn flower_petal_angle(index: usize, petal_count: usize) -> f64 {
        std::f64::consts::PI / 2.0 + (2.0 * std::f64::consts::PI * index as f64) / petal_count as f64
    }

    pub fn displaced_morph(offset_length: f64) -> f64 {
        smoothstep(DISPLACED_MORPH_START_PX, DISPLACED_MORPH_END_PX, offset_length)
    }

    pub fn badge_content_alpha(display_scale: f64) -> f64 {
        smoothstep(BADGE_FADE_START_SCALE, BADGE_FADE_END_SCALE, display_scale)
    }

    pub fn flower_outer_radius(member_count: usize, petal_body_radius: f64) -> f64 {
        flower_ring_radius(petal_body_radius, member_count.min(MAX_FLOWER_PETALS)) + petal_body_radius
    }
}

use math::*;

/// The length of a vector, as `simd_length`: the square root of the dot product.
fn vector_length(x: f64, y: f64) -> f64 {
    (x * x + y * y).sqrt()
}

/// A pin id from its double (a whole number up to 2^53).
fn key(id: f64) -> u64 {
    id as u64
}

/// A stable merge sort with an insertion sort for short runs: one small
/// routine instead of the standard library's sorts, which would weigh
/// several times more in the module for each element type.
pub fn merge_sort<T: Copy>(values: &mut [T], less: &impl Fn(&T, &T) -> bool) {
    let mut scratch = Vec::with_capacity(values.len() / 2 + 1);
    merge_sort_with(values, less, &mut scratch);
}

fn merge_sort_with<T: Copy>(values: &mut [T], less: &impl Fn(&T, &T) -> bool, scratch: &mut Vec<T>) {
    if values.len() <= 20 {
        for i in 1..values.len() {
            let value = values[i];
            let mut j = i;
            while j > 0 && less(&value, &values[j - 1]) {
                values[j] = values[j - 1];
                j -= 1;
            }
            values[j] = value;
        }
        return;
    }
    let middle = values.len() / 2;
    merge_sort_with(&mut values[..middle], less, scratch);
    merge_sort_with(&mut values[middle..], less, scratch);
    if !less(&values[middle], &values[middle - 1]) {
        return;
    }
    scratch.clear();
    scratch.extend_from_slice(&values[..middle]);
    let left = &scratch[..];
    let (mut l, mut r, mut o) = (0, middle, 0);
    while l < left.len() && r < values.len() {
        if less(&values[r], &left[l]) {
            values[o] = values[r];
            r += 1;
        } else {
            values[o] = left[l];
            l += 1;
        }
        o += 1;
    }
    while l < left.len() {
        values[o] = left[l];
        l += 1;
        o += 1;
    }
}

/// Seeds in state key order (the keys are unique: the members are disjoint).
fn sort_seeds(seeds: Vec<ClusterSeed>) -> Vec<ClusterSeed> {
    let mut order: Vec<u32> = (0..seeds.len() as u32).collect();
    merge_sort(&mut order, &|&a: &u32, &b: &u32| seeds[a as usize].state_key < seeds[b as usize].state_key);
    if order.iter().enumerate().all(|(i, &o)| i == o as usize) {
        return seeds;
    }
    let mut slots: Vec<Option<ClusterSeed>> = seeds.into_iter().map(Some).collect();
    order.iter().map(|&o| slots[o as usize].take().unwrap_or_else(|| unreachable!())).collect()
}

// MARK: Grid (the Swift `PinScreenHashGrid` and `PinDisjointSet`)

const CELL_LIMIT: i32 = 1 << 20;
const EMPTY: i32 = -1;

fn cell_coordinate(value: f64, inverse_cell_size: f64) -> i32 {
    let cell = (value * inverse_cell_size).floor();
    if cell.is_nan() {
        return 0;
    }
    cell.clamp(-CELL_LIMIT as f64, CELL_LIMIT as f64) as i32
}

/// A uniform hash grid over screen positions: indices ascend within a cell,
/// slots are numbered in the order their first point appears, the neighbor
/// slots of every slot are precomputed. Cell coordinates are clamped to plus
/// or minus 2^20. The arrays are kept between builds.
#[derive(Default)]
pub struct PinScreenHashGrid {
    inverse_cell_size: f64,
    table_x: Vec<i32>,
    table_y: Vec<i32>,
    table_slot: Vec<i32>,
    table_mask: usize,
    slot_x: Vec<i32>,
    slot_y: Vec<i32>,
    pub starts: Vec<u32>,
    pub entries: Vec<u32>,
    pub slot_for_point: Vec<u32>,
    pub neighbor_slots: Vec<i32>,
    points: usize,
}

impl PinScreenHashGrid {
    pub fn new(positions: &[f64], cell_size: f64) -> Self {
        let mut grid = Self::default();
        grid.build(positions, cell_size);
        grid
    }

    pub fn cell_count(&self) -> usize {
        self.slot_x.len()
    }

    fn hash(&self, x: i32, y: i32) -> usize {
        ((x.wrapping_mul(0x9e3779b1u32 as i32) ^ y.wrapping_mul(0x85ebca77u32 as i32)) as u32 as usize) & self.table_mask
    }

    fn find(&self, x: i32, y: i32) -> i32 {
        let mut at = self.hash(x, y);
        loop {
            let slot = self.table_slot[at];
            if slot == EMPTY {
                return EMPTY;
            }
            if self.table_x[at] == x && self.table_y[at] == y {
                return slot;
            }
            at = (at + 1) & self.table_mask;
        }
    }

    /// Builds over the points whose coordinates are interleaved x, y.
    pub fn build(&mut self, positions: &[f64], cell_size: f64) {
        let count = positions.len() / 2;
        let inverse = 1.0 / cell_size.max(1.0);
        self.inverse_cell_size = inverse;
        self.points = count;
        let mut table_size = 256;
        while table_size < count * 2 {
            table_size *= 2;
        }
        self.table_mask = table_size - 1;
        self.table_slot.clear();
        self.table_slot.resize(table_size, EMPTY);
        self.table_x.resize(table_size, 0);
        self.table_y.resize(table_size, 0);
        self.slot_x.clear();
        self.slot_y.clear();
        self.slot_for_point.clear();
        let mut counts: Vec<u32> = std::mem::take(&mut self.starts);
        counts.clear();
        for index in 0..count {
            let x = cell_coordinate(positions[index * 2], inverse);
            let y = cell_coordinate(positions[index * 2 + 1], inverse);
            let mut slot = self.find(x, y);
            if slot == EMPTY {
                slot = self.slot_x.len() as i32;
                let mut at = self.hash(x, y);
                while self.table_slot[at] != EMPTY {
                    at = (at + 1) & self.table_mask;
                }
                self.table_x[at] = x;
                self.table_y[at] = y;
                self.table_slot[at] = slot;
                self.slot_x.push(x);
                self.slot_y.push(y);
                counts.push(0);
            }
            self.slot_for_point.push(slot as u32);
            counts[slot as usize] += 1;
        }
        let slots = self.slot_x.len();
        // counts becomes starts, and a cursor copy places the entries.
        let mut cursor = 0u32;
        for value in counts.iter_mut() {
            let size = *value;
            *value = cursor;
            cursor += size;
        }
        counts.push(cursor);
        self.entries.clear();
        self.entries.resize(count, 0);
        let mut cursors: Vec<u32> = counts[..slots].to_vec();
        for index in 0..count {
            let slot = self.slot_for_point[index] as usize;
            self.entries[cursors[slot] as usize] = index as u32;
            cursors[slot] += 1;
        }
        self.starts = counts;
        self.neighbor_slots.clear();
        self.neighbor_slots.reserve(slots * 9);
        for slot in 0..slots {
            let (cx, cy) = (self.slot_x[slot], self.slot_y[slot]);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let neighbor = self.find(cx + dx, cy + dy);
                    self.neighbor_slots.push(neighbor);
                }
            }
        }
    }

    pub fn cell_population_at(&self, x: f64, y: f64) -> usize {
        let slot = self.find(cell_coordinate(x, self.inverse_cell_size), cell_coordinate(y, self.inverse_cell_size));
        if slot == EMPTY { 0 } else { (self.starts[slot as usize + 1] - self.starts[slot as usize]) as usize }
    }

    pub fn cell_population_of_point(&self, index: usize) -> usize {
        let slot = self.slot_for_point[index] as usize;
        (self.starts[slot + 1] - self.starts[slot]) as usize
    }

    /// Neighbors of a grid point in its 3 by 3 neighborhood with an index
    /// above `min_index`, ascending. Each cell's run ascends and starts past
    /// `min_index` by a binary search, and the runs are merged pairwise
    /// through `scratch`.
    pub fn collect_neighbors(&self, index: usize, min_index: i64, out: &mut Vec<u32>, scratch: &mut Vec<u32>) {
        out.clear();
        let base = self.slot_for_point[index] as usize * 9;
        let mut bounds = [0usize; 10];
        let mut runs = 0;
        for offset in 0..9 {
            let slot = self.neighbor_slots[base + offset];
            if slot < 0 {
                continue;
            }
            let (begin, end) = (self.starts[slot as usize] as usize, self.starts[slot as usize + 1] as usize);
            let first = begin + self.entries[begin..end].partition_point(|&point| point as i64 <= min_index);
            if first < end {
                out.extend_from_slice(&self.entries[first..end]);
                runs += 1;
                bounds[runs] = out.len();
            }
        }
        while runs > 1 {
            scratch.clear();
            let mut merged = [0usize; 10];
            let mut merged_runs = 0;
            let mut run = 0;
            while run < runs {
                if run + 1 < runs {
                    let (mut l, l_end, mut r, r_end) = (bounds[run], bounds[run + 1], bounds[run + 1], bounds[run + 2]);
                    while l < l_end && r < r_end {
                        if out[r] < out[l] {
                            scratch.push(out[r]);
                            r += 1;
                        } else {
                            scratch.push(out[l]);
                            l += 1;
                        }
                    }
                    scratch.extend_from_slice(&out[l..l_end]);
                    scratch.extend_from_slice(&out[r..r_end]);
                    run += 2;
                } else {
                    scratch.extend_from_slice(&out[bounds[run]..bounds[run + 1]]);
                    run += 1;
                }
                merged_runs += 1;
                merged[merged_runs] = scratch.len();
            }
            std::mem::swap(out, scratch);
            bounds = merged;
            runs = merged_runs;
        }
    }

    /// Every pair (i, j > i) of the grid's points in adjacent cells that
    /// passes `keep`, as lists of j per i (`starts`, `neighbors`), in (i, j)
    /// order: the order of the (i, j > i) traversal by `collect_neighbors`
    /// without a sort per point. The pairs are found by ascending j and
    /// placed by a stable counting sort on i.
    pub fn ascending_pairs(&self, keep: impl Fn(usize, usize) -> bool, starts: &mut Vec<u32>, neighbors: &mut Vec<u32>, scratch: &mut Vec<u32>) {
        let count = self.points;
        starts.clear();
        starts.resize(count + 1, 0);
        scratch.clear();
        for j in 0..count {
            let base = self.slot_for_point[j] as usize * 9;
            for offset in 0..9 {
                let slot = self.neighbor_slots[base + offset];
                if slot < 0 {
                    continue;
                }
                let (begin, end) = (self.starts[slot as usize] as usize, self.starts[slot as usize + 1] as usize);
                let run = &self.entries[begin..end];
                for &i in &run[..run.partition_point(|&point| (point as usize) < j)] {
                    if keep(i as usize, j) {
                        scratch.push(i);
                        scratch.push(j as u32);
                        starts[i as usize + 1] += 1;
                    }
                }
            }
        }
        for i in 0..count {
            starts[i + 1] += starts[i];
        }
        neighbors.clear();
        neighbors.resize(scratch.len() / 2, 0);
        let mut cursors: Vec<u32> = starts[..count].to_vec();
        for &[i, j] in scratch.as_chunks::<2>().0 {
            let i = i as usize;
            neighbors[cursors[i] as usize] = j;
            cursors[i] += 1;
        }
    }

    /// All points in the 3 by 3 neighborhood of a grid point, itself
    /// included, in cell order: for folds that do not depend on the order.
    pub fn collect_cell_neighbors(&self, index: usize, out: &mut Vec<u32>) {
        out.clear();
        let base = self.slot_for_point[index] as usize * 9;
        for offset in 0..9 {
            let slot = self.neighbor_slots[base + offset];
            if slot < 0 {
                continue;
            }
            out.extend_from_slice(&self.entries[self.starts[slot as usize] as usize..self.starts[slot as usize + 1] as usize]);
        }
    }

    /// Candidates around an arbitrary position, in a fixed cell order.
    pub fn collect_candidates(&self, x: f64, y: f64, out: &mut Vec<u32>) {
        out.clear();
        if self.points == 0 {
            return;
        }
        let cx = cell_coordinate(x, self.inverse_cell_size);
        let cy = cell_coordinate(y, self.inverse_cell_size);
        for dy in -1..=1 {
            for dx in -1..=1 {
                let slot = self.find(cx + dx, cy + dy);
                if slot == EMPTY {
                    continue;
                }
                let slot = slot as usize;
                out.extend_from_slice(&self.entries[self.starts[slot] as usize..self.starts[slot + 1] as usize]);
            }
        }
    }
}

/// A disjoint set whose component root is its minimum index, with an iterative find.
#[derive(Default)]
pub struct PinDisjointSet {
    parent: Vec<u32>,
}

impl PinDisjointSet {
    pub fn reset(&mut self, count: usize) {
        self.parent.clear();
        self.parent.extend(0..count as u32);
    }

    pub fn find(&mut self, index: usize) -> usize {
        let mut root = index;
        while self.parent[root] as usize != root {
            root = self.parent[root] as usize;
        }
        let mut current = index;
        while self.parent[current] as usize != root {
            let next = self.parent[current] as usize;
            self.parent[current] = root as u32;
            current = next;
        }
        root
    }

    pub fn union(&mut self, lhs: usize, rhs: usize) {
        let lhs_root = self.find(lhs);
        let rhs_root = self.find(rhs);
        if lhs_root == rhs_root {
            return;
        }
        self.parent[lhs_root.max(rhs_root)] = lhs_root.min(rhs_root) as u32;
    }
}

// MARK: Solver

/// The geometry and settings of a solve.
#[derive(Clone, Copy, Debug)]
pub struct PinSolveParameters {
    pub pin_size_px: f64,
    pub body_radius_px: f64,
    pub circle_body_radius_px: f64,
    pub body_center_offset_x: f64,
    pub body_center_offset_y: f64,
    pub compressed_scale: f64,
    pub collision_padding_px: f64,
    pub grouping_threshold: f64,
    pub max_offset_px: f64,
    pub collision_iterations: f64,
    pub spring_k: f64,
    pub smoothing: f64,
}

impl PinSolveParameters {
    fn body_radius(&self, morph: f64) -> f64 {
        self.body_radius_px + (self.circle_body_radius_px - self.body_radius_px) * clamp(morph, 0.0, 1.0)
    }
}

#[derive(Clone, Copy)]
struct PinMotionState {
    offset_x: f64,
    offset_y: f64,
    scale: f64,
    morph: f64,
    cluster_blend: f64,
    last_cluster_key: Option<u64>,
}

struct ClusterMotionState {
    presence: f64,
    member_ids: Vec<u64>,
}

#[derive(Clone)]
struct ClusterSeed {
    state_key: u64,
    member_indexes: Vec<u32>,
    bounds_min_x: f64,
    bounds_min_y: f64,
    bounds_max_x: f64,
    bounds_max_y: f64,
    center_x: f64,
    center_y: f64,
}

impl ClusterSeed {
    fn member_count(&self) -> usize {
        self.member_indexes.len()
    }
}

const DENSE_GROUPING_CELL_POPULATION: u32 = 16;
const DENSE_GROUPING_PROBE_LIMIT: i32 = 64;

#[derive(Clone, Copy, PartialEq)]
enum Join {
    Event,
    Overflow,
    Bodies,
}

fn merge_sorted_indexes(lhs: &[u32], rhs: &[u32]) -> Vec<u32> {
    let mut result = Vec::with_capacity(lhs.len() + rhs.len());
    let (mut l, mut r) = (0, 0);
    while l < lhs.len() && r < rhs.len() {
        if lhs[l] <= rhs[r] {
            result.push(lhs[l]);
            l += 1;
        } else {
            result.push(rhs[r]);
            r += 1;
        }
    }
    result.extend_from_slice(&lhs[l..]);
    result.extend_from_slice(&rhs[r..]);
    result
}

/// Solver nodes: standalone pins, then the petal slots of live flowers.
#[derive(Default)]
struct SolverNodes {
    anchor_x: Vec<f64>,
    anchor_y: Vec<f64>,
    position_x: Vec<f64>,
    position_y: Vec<f64>,
    radius: Vec<f64>,
    inverse_mass: Vec<f64>,
    direction_key: Vec<u64>,
    pair_starts: Vec<u32>,
    pair_neighbors: Vec<u32>,
    positions: Vec<f64>,
}

impl SolverNodes {
    fn clear(&mut self) {
        self.anchor_x.clear();
        self.anchor_y.clear();
        self.position_x.clear();
        self.position_y.clear();
        self.radius.clear();
        self.inverse_mass.clear();
        self.direction_key.clear();
    }

    #[allow(clippy::too_many_arguments)]
    fn push(&mut self, anchor_x: f64, anchor_y: f64, position_x: f64, position_y: f64, radius: f64, inverse_mass: f64, direction_key: u64) {
        self.anchor_x.push(anchor_x);
        self.anchor_y.push(anchor_y);
        self.position_x.push(position_x);
        self.position_y.push(position_y);
        self.radius.push(radius);
        self.inverse_mass.push(inverse_mass);
        self.direction_key.push(direction_key);
    }

    fn len(&self) -> usize {
        self.anchor_x.len()
    }
}

/// The frame's input in id order.
#[derive(Default)]
struct FrameInput {
    source: Vec<u32>,
    ids: Vec<f64>,
    keys: Vec<u64>,
    px: Vec<f64>,
    py: Vec<f64>,
    wx: Vec<f64>,
    wy: Vec<f64>,
    alpha: Vec<f64>,
    size_scale: Vec<f64>,
    priority: Vec<f64>,
    draw_order: Vec<f64>,
    selected: Vec<bool>,
    event: Vec<bool>,
    was_grouped: Vec<bool>,
    seen: Vec<bool>,
    seed_of: Vec<i32>,
    state: Vec<Option<PinMotionState>>,
    /// The dissolved flower a stationarily hidden member flies out of.
    dissolved_key: Vec<Option<u64>>,
}

/// One emitted item, by input index.
#[derive(Clone, Copy)]
struct Item {
    index: u32,
    x: f64,
    y: f64,
    alpha: f64,
    scale: f64,
    morph: f64,
    petal: bool,
}

/// The pin collision layout solver with its inter-frame state.
#[derive(Default)]
pub struct PinCollisionSolver {
    /// Pin states by id, ascending: merged into the frame's input by index.
    pin_state_ids: Vec<u64>,
    pin_state_values: Vec<PinMotionState>,
    /// Flower states by key, ascending.
    cluster_states: Vec<(u64, ClusterMotionState)>,
    previously_grouped: Vec<u64>,
    last_time: Option<f64>,
    input: FrameInput,
    items: Vec<Item>,
    grid: PinScreenHashGrid,
    disjoint_set: PinDisjointSet,
    candidates: Vec<u32>,
    scratch: Vec<u32>,
    nodes: SolverNodes,
    positions: Vec<f64>,
    body_x: Vec<f64>,
    body_y: Vec<f64>,
    body_reach: Vec<f64>,
    jitter: Vec<(f64, f64)>,
    join: Option<Join>,
    join_radius: f64,
    join_exit_radius: f64,
    output: Vec<f64>,
}

/// A frame's result: the items in draw order and the flowers.
pub struct PinLayoutOutput<'a> {
    pub data: &'a [f64],
}

impl PinLayoutOutput<'_> {
    pub fn item_count(&self) -> usize {
        self.data[0] as usize
    }
    pub fn flower_count(&self) -> usize {
        self.data[1] as usize
    }
    pub fn has_active_animations(&self) -> bool {
        self.data[2] != 0.0
    }
    pub fn item(&self, k: usize) -> &[f64] {
        let o = PIN_OUTPUT_HEADER + k * PIN_ITEM_STRIDE;
        &self.data[o..o + PIN_ITEM_STRIDE]
    }
    /// The flowers: center x, center y, member input indices, visible member input indices.
    pub fn flowers(&self) -> Vec<(f64, f64, Vec<usize>, Vec<usize>)> {
        let mut o = PIN_OUTPUT_HEADER + self.item_count() * PIN_ITEM_STRIDE;
        let mut result = Vec::new();
        for _ in 0..self.flower_count() {
            let (cx, cy, members, visible) = (self.data[o], self.data[o + 1], self.data[o + 2] as usize, self.data[o + 3] as usize);
            o += 4;
            let member_list = self.data[o..o + members].iter().map(|&v| v as usize).collect();
            o += members;
            let visible_list = self.data[o..o + visible].iter().map(|&v| v as usize).collect();
            o += visible;
            result.push((cx, cy, member_list, visible_list));
        }
        result
    }
}

impl PinCollisionSolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets the inter-frame state.
    pub fn reset(&mut self) {
        self.pin_state_ids.clear();
        self.pin_state_values.clear();
        self.cluster_states.clear();
        self.previously_grouped.clear();
        self.last_time = None;
    }

    /// `solve_frame` over flat arrays: `pins` holds `PIN_INPUT_STRIDE` doubles
    /// per pin. Returns the header, the items in draw order
    /// (`PIN_ITEM_STRIDE` doubles each) and the flowers (center x, center y,
    /// member count, visible count, member input indices, visible input
    /// indices).
    #[allow(clippy::too_many_arguments)]
    pub fn solve(
        &mut self,
        pins: &[f64],
        pin_size_px: f64,
        body_radius_px: f64,
        circle_body_radius_px: f64,
        body_center_offset_x: f64,
        body_center_offset_y: f64,
        compressed_scale: f64,
        collision_padding_px: f64,
        grouping_threshold: f64,
        max_offset_px: f64,
        collision_iterations: f64,
        spring_k: f64,
        smoothing: f64,
        time: f64,
    ) -> Vec<f64> {
        let parameters = PinSolveParameters {
            pin_size_px,
            body_radius_px,
            circle_body_radius_px,
            body_center_offset_x,
            body_center_offset_y,
            compressed_scale,
            collision_padding_px,
            grouping_threshold,
            max_offset_px,
            collision_iterations,
            spring_k,
            smoothing,
        };
        self.solve_frame(pins, &parameters, time);
        std::mem::take(&mut self.output)
    }
}

impl PinCollisionSolver {
    /// Solves one frame. The result stays valid until the next solve.
    pub fn solve_frame(&mut self, pins: &[f64], parameters: &PinSolveParameters, time: f64) -> PinLayoutOutput<'_> {
        self.output.clear();
        self.solve_into_output(pins, parameters, time);
        self.last_time = Some(time);
        PinLayoutOutput { data: &self.output }
    }

    fn load_input(&mut self, pins: &[f64]) {
        let n = pins.len() / PIN_INPUT_STRIDE;
        let input = &mut self.input;
        input.source.clear();
        input.source.extend(0..n as u32);
        let id_at = |i: u32| pins[i as usize * PIN_INPUT_STRIDE];
        let sorted = (1..n).all(|i| id_at(i as u32 - 1) < id_at(i as u32));
        if !sorted {
            merge_sort(&mut input.source, &|&a: &u32, &b: &u32| id_at(a) < id_at(b));
        }
        macro_rules! fill {
            ($field:ident, $offset:expr) => {
                input.$field.clear();
                input.$field.extend(input.source.iter().map(|&s| pins[s as usize * PIN_INPUT_STRIDE + $offset]));
            };
        }
        fill!(ids, 0);
        fill!(px, 1);
        fill!(py, 2);
        fill!(alpha, 3);
        fill!(wx, 4);
        fill!(wy, 5);
        fill!(size_scale, 6);
        fill!(priority, 7);
        fill!(draw_order, 8);
        input.keys.clear();
        input.keys.extend(input.ids.iter().map(|&id| key(id)));
        input.selected.clear();
        input.event.clear();
        for &s in &input.source {
            let flags = pins[s as usize * PIN_INPUT_STRIDE + 9] as u32;
            input.selected.push(flags & FLAG_SELECTED != 0);
            input.event.push(flags & FLAG_EVENT != 0);
        }
        input.seen.clear();
        input.seen.resize(n, false);
        input.seed_of.clear();
        input.seed_of.resize(n, -1);
        input.was_grouped.clear();
        input.dissolved_key.clear();
        input.dissolved_key.resize(n, None);
        let previous = &self.previously_grouped;
        let mut p = 0;
        for i in 0..n {
            let id = input.keys[i];
            while p < previous.len() && previous[p] < id {
                p += 1;
            }
            input.was_grouped.push(p < previous.len() && previous[p] == id);
        }
        // Last frame's pin states, by input index.
        input.state.clear();
        let mut p = 0;
        for i in 0..n {
            let id = input.keys[i];
            while p < self.pin_state_ids.len() && self.pin_state_ids[p] < id {
                p += 1;
            }
            input.state.push(if p < self.pin_state_ids.len() && self.pin_state_ids[p] == id { Some(self.pin_state_values[p]) } else { None });
        }
    }

    fn emit(&mut self, index: usize, x: f64, y: f64, alpha: f64, scale: f64, morph: f64, petal: bool) {
        self.items.push(Item { index: index as u32, x, y, alpha, scale, morph, petal });
    }

    fn solve_into_output(&mut self, pins: &[f64], parameters: &PinSolveParameters, time: f64) {
        let n = pins.len() / PIN_INPUT_STRIDE;
        if n == 0 {
            self.pin_state_ids.clear();
            self.pin_state_values.clear();
            self.cluster_states.clear();
            self.previously_grouped.clear();
            self.output.extend_from_slice(&[0.0, 0.0, 0.0]);
            return;
        }
        let p = *parameters;
        let delta_seconds = (time - self.last_time.unwrap_or(time)).max(0.0).min(MAX_FRAME_DELTA_SECONDS);
        let smoothing_k = smoothing_factor(p.smoothing, delta_seconds);
        let blend_step = delta_seconds / CLUSTER_CROSSFADE_SECONDS.max(0.001);
        let pin_size_px = p.pin_size_px;
        let compressed_scale = p.compressed_scale.min(1.0);
        let petal_body_radius = p.circle_body_radius_px * compressed_scale;
        let padding = p.collision_padding_px;
        let (offset_x, offset_y) = (p.body_center_offset_x, p.body_center_offset_y);

        self.load_input(pins);
        let mut unsettled = false;
        let compactness_limit = pin_size_px * FLOWER_COMPACTNESS_LIMIT_SCALE;

        let event_seeds = self.event_cluster_seeds(pin_size_px, padding);
        let mut excluded = vec![false; n];
        for seed in &event_seeds {
            for &member in &seed.member_indexes {
                excluded[member as usize] = true;
            }
        }
        let overflow_seeds = self.overflow_cluster_seeds(&excluded, !event_seeds.is_empty(), pin_size_px, compactness_limit, p.grouping_threshold);

        let mut all_seeds = event_seeds;
        all_seeds.extend(overflow_seeds);
        let mut cluster_seeds = self.merge_intersecting_flowers(all_seeds, petal_body_radius, compactness_limit);
        let mut clustered = vec![false; n];
        for seed in &cluster_seeds {
            for &member in &seed.member_indexes {
                clustered[member as usize] = true;
            }
        }
        let standalone: Vec<u32> = (0..n as u32).filter(|&i| !clustered[i as usize]).collect();
        let standalone = self.absorb_pins_into_flowers(&mut cluster_seeds, standalone, &p, petal_body_radius, padding);
        let mut cluster_seeds = sort_seeds(self.merge_intersecting_flowers(cluster_seeds, petal_body_radius, compactness_limit));
        separate_flower_rings(&mut cluster_seeds, petal_body_radius);

        for (s, seed) in cluster_seeds.iter().enumerate() {
            for &member in &seed.member_indexes {
                self.input.seed_of[member as usize] = s as i32;
            }
        }
        self.previously_grouped.clear();
        for i in 0..n {
            if self.input.seed_of[i] >= 0 {
                self.previously_grouped.push(self.input.keys[i]);
            }
        }

        let standalone_count = standalone.len();
        let mut rigid = self.dominant_flags(&standalone, &cluster_seeds, &p, compressed_scale, padding);
        for s in 0..standalone_count {
            if self.input.selected[standalone[s] as usize] {
                rigid[s] = true;
            }
        }

        // The deterministic starting jitter of each standalone pin, the same in every pass.
        let jitter_amplitude = p.body_radius_px * START_JITTER_SCALE;
        self.jitter.clear();
        for &index in &standalone {
            let (cos, sin) = stable_unit_direction(self.input.keys[index as usize], START_JITTER_SALT);
            self.jitter.push((cos * jitter_amplitude, sin * jitter_amplitude));
        }
        self.make_nodes(&standalone, &rigid, &cluster_seeds, &p, None);
        self.relax(&p);
        let full_body_morphs: Vec<f64> = (0..standalone_count)
            .map(|s| displaced_morph(vector_length(self.nodes.position_x[s] - self.nodes.anchor_x[s], self.nodes.position_y[s] - self.nodes.anchor_y[s])))
            .collect();

        self.make_nodes(&standalone, &rigid, &cluster_seeds, &p, Some(&full_body_morphs));
        self.relax(&p);

        let mut max_standalone_size_scale: f64 = 0.0;
        for &index in &standalone {
            max_standalone_size_scale = max_standalone_size_scale.max(self.input.size_scale[index as usize]);
        }
        let mut morph_targets = vec![0.0; standalone_count];
        let mut candidates = std::mem::take(&mut self.candidates);
        if standalone_count > 0 {
            let mut max_probe_radius: f64 = 1.0;
            let node_count = self.nodes.len();
            self.positions.clear();
            for i in 0..node_count {
                max_probe_radius = max_probe_radius.max(self.nodes.radius[i]);
                self.positions.push(self.nodes.position_x[i]);
                self.positions.push(self.nodes.position_y[i]);
            }
            self.grid.build(&self.positions, p.body_radius_px * max_standalone_size_scale.max(1.0) + max_probe_radius);
            let nodes = &self.nodes;
            for s in 0..standalone_count {
                if !(full_body_morphs[s] > 0.0) {
                    morph_targets[s] = 0.0;
                    continue;
                }
                let index = standalone[s] as usize;
                let scale = self.input.size_scale[index];
                let center_x = self.input.px[index] + offset_x * scale;
                let center_y = self.input.py[index] + offset_y * scale;
                let full_body_radius = p.body_radius_px * scale;
                let mut max_overlap_depth: f64 = 0.0;
                self.grid.collect_candidates(center_x, center_y, &mut candidates);
                for &other in &candidates {
                    let other = other as usize;
                    if other == s {
                        continue;
                    }
                    let other_body_radius = nodes.radius[other] - padding;
                    let distance = vector_length(center_x - nodes.position_x[other], center_y - nodes.position_y[other]);
                    max_overlap_depth = max_overlap_depth.max(full_body_radius + other_body_radius - distance);
                }
                morph_targets[s] = full_body_morphs[s].min(static_touch_morph(max_overlap_depth));
            }
        }

        self.make_nodes(&standalone, &rigid, &cluster_seeds, &p, Some(&morph_targets));
        self.relax(&p);

        let mut target_scales = vec![0.0; standalone_count];
        if standalone_count > 0 {
            let mut max_node_radius: f64 = 1.0;
            let node_count = self.nodes.len();
            self.positions.clear();
            for i in 0..node_count {
                max_node_radius = max_node_radius.max(self.nodes.radius[i]);
                self.positions.push(self.nodes.position_x[i]);
                self.positions.push(self.nodes.position_y[i]);
            }
            self.grid.build(&self.positions, (p.body_radius_px * max_standalone_size_scale.max(1.0)).max(max_node_radius) * 2.0 + 2.0 * padding);
            let nodes = &self.nodes;
            let input = &self.input;
            for s in 0..standalone_count {
                let index = standalone[s] as usize;
                let morph_target = morph_targets[s];
                let cap_scale = scale_cap(morph_target);
                let mut scale = cap_scale;
                if !rigid[s] && morph_target > 0.0 {
                    let body_radius = p.body_radius(morph_target) * input.size_scale[index];
                    let mut required: f64 = 1.0;
                    // The minimum does not depend on the order of the neighbors.
                    self.grid.collect_cell_neighbors(s, &mut candidates);
                    for &other in &candidates {
                        let other = other as usize;
                        if other == s {
                            continue;
                        }
                        let distance = vector_length(nodes.position_x[s] - nodes.position_x[other], nodes.position_y[s] - nodes.position_y[other]);
                        let other_is_pin = other < standalone_count;
                        let other_body_radius = if other_is_pin { p.body_radius(morph_targets[other]) * input.size_scale[standalone[other] as usize] } else { nodes.radius[other] - padding };
                        let other_is_rigid = if other_is_pin { rigid[other] } else { true };
                        if !(distance < body_radius + other_body_radius + 2.0 * padding) {
                            continue;
                        }
                        required = required.min(required_scale(distance, body_radius, other_body_radius, padding, other_is_rigid));
                    }
                    let lower_limit = compressed_scale.min(cap_scale);
                    scale = required.min(cap_scale).max(lower_limit);
                } else if input.selected[index] {
                    scale = 1.0;
                }
                target_scales[s] = scale;
            }
        }
        self.candidates = candidates;

        self.items.clear();
        let mut flower_output: Vec<f64> = Vec::new();
        // Dissolved flowers in key order, with their centers.
        let mut dissolved_centers: Vec<(u64, f64, f64)> = Vec::new();
        let previous_clusters = std::mem::take(&mut self.cluster_states);
        let mut live_clusters: Vec<(u64, ClusterMotionState)> = Vec::with_capacity(cluster_seeds.len());
        let mut previous_live: Vec<Option<ClusterMotionState>> = (0..cluster_seeds.len()).map(|_| None).collect();
        let mut dissolved_clusters: Vec<(u64, ClusterMotionState)> = Vec::new();
        for (key, state) in previous_clusters {
            match cluster_seeds.binary_search_by_key(&key, |seed| seed.state_key) {
                Ok(seed_index) => previous_live[seed_index] = Some(state),
                Err(_) => dissolved_clusters.push((key, state)),
            }
        }

        let mut selected_by_seed: Vec<Vec<u32>> = vec![Vec::new(); cluster_seeds.len()];
        for i in 0..n {
            if self.input.selected[i] && self.input.seed_of[i] >= 0 {
                selected_by_seed[self.input.seed_of[i] as usize].push(i as u32);
            }
        }

        for (seed_index, seed) in cluster_seeds.iter().enumerate() {
            let member_ids: Vec<u64> = seed.member_indexes.iter().map(|&m| self.input.keys[m as usize]).collect();
            let mut state = previous_live[seed_index].take().unwrap_or(ClusterMotionState { presence: 1.0, member_ids: Vec::new() });
            let presence = advance_blend(&mut state.presence, 1.0, blend_step);
            state.member_ids = member_ids;
            unsettled = unsettled || presence;
            live_clusters.push((seed.state_key, state));

            let petal_count = seed.member_count().min(MAX_FLOWER_PETALS);
            let ring_radius = flower_ring_radius(petal_body_radius, petal_count);
            let mut visible: Vec<u32> = Vec::with_capacity(petal_count);
            for &index in &selected_by_seed[seed_index] {
                visible.push(index);
                if visible.len() == petal_count {
                    break;
                }
            }
            if visible.len() < petal_count {
                for &index in &seed.member_indexes {
                    if self.input.selected[index as usize] {
                        continue;
                    }
                    visible.push(index);
                    if visible.len() == petal_count {
                        break;
                    }
                }
            }
            flower_output.push(seed.center_x);
            flower_output.push(seed.center_y);
            flower_output.push(seed.member_indexes.len() as f64);
            flower_output.push(visible.len() as f64);
            flower_output.extend(seed.member_indexes.iter().map(|&m| self.input.source[m as usize] as f64));
            flower_output.extend(visible.iter().map(|&m| self.input.source[m as usize] as f64));

            for (slot, &index) in visible.iter().enumerate() {
                let (cos, sin) = petal_direction(slot, petal_count);
                let slot_x = seed.center_x + cos * ring_radius;
                let slot_y = seed.center_y + sin * ring_radius;
                let was = self.input.was_grouped[index as usize];
                unsettled = self.emit_petal(index as usize, slot_x, slot_y, seed.state_key, was, offset_x, offset_y, compressed_scale, smoothing_k, blend_step) || unsettled;
            }
        }

        // Hidden flower members: only live inter-frame states are advanced, in id order.
        if !cluster_seeds.is_empty() {
            for i in 0..n {
                let seed_index = self.input.seed_of[i];
                if seed_index < 0 || self.input.seen[i] || self.input.state[i].is_none() {
                    continue;
                }
                let seed = &cluster_seeds[seed_index as usize];
                let (cx, cy, key) = (seed.center_x, seed.center_y, seed.state_key);
                unsettled = self.emit_hidden_member(i, cx, cy, key, compressed_scale, smoothing_k, blend_step) || unsettled;
            }
        }

        // Dissolved flowers: the center is still needed by members flying out while the presence fades.
        let mut surviving: Vec<(u64, ClusterMotionState)> = Vec::with_capacity(dissolved_clusters.len());
        for (key, mut state) in dissolved_clusters {
            let mut sum_x = 0.0;
            let mut sum_y = 0.0;
            let mut member_count = 0usize;
            for &member in &state.member_ids {
                let Ok(index) = self.input.keys.binary_search(&member) else { continue };
                sum_x += self.input.px[index];
                sum_y += self.input.py[index];
                member_count += 1;
            }
            if member_count == 0 {
                continue;
            }
            let presence = advance_blend(&mut state.presence, 0.0, blend_step);
            if !(state.presence > 0.0) {
                continue;
            }
            unsettled = unsettled || presence;
            dissolved_centers.push((key, sum_x / member_count as f64, sum_y / member_count as f64));
            for &member in &state.member_ids {
                if let Ok(index) = self.input.keys.binary_search(&member) {
                    self.input.dissolved_key[index].get_or_insert(key);
                }
            }
            surviving.push((key, state));
        }
        // The flower states for the next frame, in key order.
        let mut clusters: Vec<(u64, ClusterMotionState)> = Vec::with_capacity(live_clusters.len() + surviving.len());
        let mut live = live_clusters.into_iter().peekable();
        let mut gone = surviving.into_iter().peekable();
        loop {
            let take_live = match (live.peek(), gone.peek()) {
                (Some(a), Some(b)) => a.0 < b.0,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => break,
            };
            let next = if take_live { live.next() } else { gone.next() };
            clusters.extend(next);
        }
        self.cluster_states = clusters;
        let center_of = |key: u64| -> Option<(f64, f64)> {
            if let Ok(s) = cluster_seeds.binary_search_by_key(&key, |seed| seed.state_key) {
                return Some((cluster_seeds[s].center_x, cluster_seeds[s].center_y));
            }
            dissolved_centers.binary_search_by_key(&key, |entry| entry.0).ok().map(|d| (dissolved_centers[d].1, dissolved_centers[d].2))
        };

        for s in 0..standalone_count {
            let index = standalone[s] as usize;
            self.input.seen[index] = true;
            let target_offset_x = self.nodes.position_x[s] - self.nodes.anchor_x[s];
            let target_offset_y = self.nodes.position_y[s] - self.nodes.anchor_y[s];
            let target_morph = morph_targets[s];
            let target_scale = target_scales[s];
            let was_grouped = self.input.was_grouped[index];
            let dissolved_key = self.input.dissolved_key[index];
            let state = self.input.state[index].get_or_insert_with(|| {
                let dissolved = if was_grouped { dissolved_key } else { None };
                match dissolved {
                    Some(key) => PinMotionState { offset_x: target_offset_x, offset_y: target_offset_y, scale: target_scale, morph: target_morph, cluster_blend: 1.0, last_cluster_key: Some(key) },
                    None => PinMotionState { offset_x: target_offset_x, offset_y: target_offset_y, scale: target_scale, morph: target_morph, cluster_blend: 0.0, last_cluster_key: None },
                }
            });
            let offset_active = smooth_offset_toward(state, target_offset_x, target_offset_y, smoothing_k, OFFSET_SNAP_EPSILON_PX);
            let scale_active = smooth_toward(&mut state.scale, target_scale, smoothing_k, SCALE_SNAP_EPSILON);
            let morph_active = smooth_toward(&mut state.morph, target_morph, smoothing_k, SCALE_SNAP_EPSILON);
            let blend_active = advance_blend(&mut state.cluster_blend, 0.0, blend_step);
            unsettled = unsettled || offset_active || scale_active || morph_active || blend_active;

            let lift = self.input.size_scale[index] * (scale_cap(morph_targets[s]) - state.scale);
            let mut x = self.input.px[index] + state.offset_x + offset_x * lift;
            let mut y = self.input.py[index] + state.offset_y + offset_y * lift;
            let mut alpha_factor = 1.0;
            if state.cluster_blend > 0.0 {
                match state.last_cluster_key.and_then(center_of) {
                    Some((cx, cy)) => {
                        let ease = smoothstep(0.0, 1.0, state.cluster_blend);
                        x += (cx - x) * ease;
                        y += (cy - y) * ease;
                        alpha_factor = 1.0 - state.cluster_blend;
                    }
                    None => state.cluster_blend = 0.0,
                }
            }
            if state.cluster_blend <= 0.0 {
                state.last_cluster_key = None;
            }
            let (scale, morph) = (state.scale, state.morph);
            let alpha = self.input.alpha[index] * alpha_factor;
            self.emit(index, x, y, alpha, scale, morph, false);
        }

        // The states of the pins seen this frame, in id order, for the next one.
        self.pin_state_ids.clear();
        self.pin_state_values.clear();
        for i in 0..n {
            if let (true, Some(state)) = (self.input.seen[i], self.input.state[i]) {
                self.pin_state_ids.push(self.input.keys[i]);
                self.pin_state_values.push(state);
            }
        }

        self.resolve_displayed_overlaps(&p, compressed_scale);

        // The items in draw order, then id order.
        let input = &self.input;
        merge_sort(&mut self.items, &|a: &Item, b: &Item| {
            let (da, db) = (input.draw_order[a.index as usize], input.draw_order[b.index as usize]);
            da < db || (da == db && input.ids[a.index as usize] < input.ids[b.index as usize])
        });
        self.output.reserve(PIN_OUTPUT_HEADER + self.items.len() * PIN_ITEM_STRIDE + flower_output.len());
        self.output.push(self.items.len() as f64);
        self.output.push(cluster_seeds.len() as f64);
        self.output.push(if unsettled { 1.0 } else { 0.0 });
        for item in &self.items {
            self.output.extend_from_slice(&[input.source[item.index as usize] as f64, item.x, item.y, item.alpha, item.scale, item.morph, if item.petal { 1.0 } else { 0.0 }]);
        }
        self.output.extend_from_slice(&flower_output);
    }

    fn resolve_displayed_overlaps(&mut self, p: &PinSolveParameters, compressed_scale: f64) {
        let count = self.items.len();
        if count <= 1 {
            return;
        }
        let input = &self.input;
        let items = &mut self.items;
        let mut cx = vec![0.0; count];
        let mut cy = vec![0.0; count];
        let mut radii = vec![0.0; count];
        let mut inverse_masses = vec![0.0; count];
        for (k, item) in items.iter().enumerate() {
            let index = item.index as usize;
            let body_scale = item.scale * input.size_scale[index];
            cx[k] = item.x + p.body_center_offset_x * body_scale;
            cy[k] = item.y + p.body_center_offset_y * body_scale;
            let fading_through = item.alpha < input.alpha[index] * 0.99;
            radii[k] = if fading_through { 0.0 } else { p.body_radius(item.morph) * body_scale };
            inverse_masses[k] = if input.selected[index] || item.petal { 0.0 } else { item.morph };
        }
        let mut solid: Vec<usize> = Vec::new();
        let mut fading: Vec<usize> = Vec::new();
        let mut max_solid_radius: f64 = 1.0;
        for k in 0..count {
            if radii[k] > 0.0 {
                solid.push(k);
                max_solid_radius = max_solid_radius.max(radii[k]);
            } else {
                fading.push(k);
            }
        }
        self.positions.clear();
        for &k in &solid {
            self.positions.push(cx[k]);
            self.positions.push(cy[k]);
        }
        self.grid.build(&self.positions, 2.0 * max_solid_radius);
        let grid = &self.grid;
        let candidates = &mut self.candidates;
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        for &f in &fading {
            grid.collect_candidates(cx[f], cy[f], candidates);
            for &c in candidates.iter() {
                let s = solid[c as usize];
                let reach = radii[f] + radii[s] + max_solid_radius;
                let dx = cx[f] - cx[s];
                let dy = cy[f] - cy[s];
                if dx * dx + dy * dy < reach * reach {
                    pairs.push((f.min(s), f.max(s)));
                }
            }
        }
        let mut solid_starts = std::mem::take(&mut self.nodes.pair_starts);
        let mut solid_neighbors = std::mem::take(&mut self.nodes.pair_neighbors);
        grid.ascending_pairs(
            |l, m| {
                let (k, o) = (solid[l], solid[m]);
                let reach = radii[k] + radii[o] + max_solid_radius;
                let dx = cx[k] - cx[o];
                let dy = cy[k] - cy[o];
                dx * dx + dy * dy < reach * reach
            },
            &mut solid_starts,
            &mut solid_neighbors,
            &mut self.scratch,
        );
        for (l, &k) in solid.iter().enumerate() {
            for &m in &solid_neighbors[solid_starts[l] as usize..solid_starts[l + 1] as usize] {
                pairs.push((k, solid[m as usize]));
            }
        }
        self.nodes.pair_starts = solid_starts;
        self.nodes.pair_neighbors = solid_neighbors;

        let resolve_pair = |lhs: usize, rhs: usize, cx: &mut [f64], cy: &mut [f64], radii: &[f64], items: &[Item]| {
            let mut lhs_inverse_mass = inverse_masses[lhs];
            let mut rhs_inverse_mass = inverse_masses[rhs];
            let both_petals = items[lhs].petal && items[rhs].petal;
            if both_petals {
                lhs_inverse_mass = if input.selected[items[lhs].index as usize] { 0.0 } else { 1.0 };
                rhs_inverse_mass = if input.selected[items[rhs].index as usize] { 0.0 } else { 1.0 };
            }
            let inverse_mass_sum = lhs_inverse_mass + rhs_inverse_mass;
            if !(inverse_mass_sum > 0.0) {
                return;
            }
            let min_distance = radii[lhs] + radii[rhs];
            let dx = cx[rhs] - cx[lhs];
            let dy = cy[rhs] - cy[lhs];
            let distance_squared = dx * dx + dy * dy;
            if !(distance_squared < min_distance * min_distance) {
                return;
            }
            let distance = distance_squared.sqrt();
            let overlap = min_distance - distance;
            if both_petals && overlap <= 1.0 {
                return;
            }
            let (dir_x, dir_y) = if distance > 1e-4 {
                (dx / distance, dy / distance)
            } else {
                stable_unit_direction(input.keys[items[lhs].index as usize], input.keys[items[rhs].index as usize])
            };
            let lhs_share = overlap * (lhs_inverse_mass / inverse_mass_sum);
            let rhs_share = overlap * (rhs_inverse_mass / inverse_mass_sum);
            cx[lhs] -= dir_x * lhs_share;
            cy[lhs] -= dir_y * lhs_share;
            cx[rhs] += dir_x * rhs_share;
            cy[rhs] += dir_y * rhs_share;
        };

        for _ in 0..20 {
            for &(a, b) in &pairs {
                resolve_pair(a, b, &mut cx, &mut cy, &radii, items);
            }
        }

        let mut scale_adjusted = false;
        for &(a, b) in &pairs {
            let residual_min_distance = radii[a] + radii[b];
            let dx = cx[b] - cx[a];
            let dy = cy[b] - cy[a];
            let distance_squared = dx * dx + dy * dy;
            if !(residual_min_distance > 0.5) || !(distance_squared < (residual_min_distance - 0.5) * (residual_min_distance - 0.5)) {
                continue;
            }
            let distance = distance_squared.sqrt();
            for (candidate, other) in [(a, b), (b, a)] {
                let item = items[candidate];
                let index = item.index as usize;
                if item.petal || input.selected[index] || !(item.morph > 0.5) || !(radii[candidate] > 0.0) {
                    continue;
                }
                let unit_body_radius = p.body_radius(item.morph) * input.size_scale[index];
                if !(unit_body_radius > 0.0) {
                    continue;
                }
                let required = (distance - radii[other]) / unit_body_radius;
                let display_scale = item.scale;
                let lower_limit = compressed_scale.min(display_scale);
                let new_scale = required.min(display_scale).max(lower_limit);
                if !(new_scale < display_scale - 0.005) {
                    continue;
                }
                items[candidate].scale = new_scale;
                radii[candidate] = unit_body_radius * new_scale;
                scale_adjusted = true;
            }
        }
        if scale_adjusted {
            for _ in 0..8 {
                for &(a, b) in &pairs {
                    resolve_pair(a, b, &mut cx, &mut cy, &radii, items);
                }
            }
        }

        for (k, item) in items.iter_mut().enumerate() {
            let body_scale = item.scale * input.size_scale[item.index as usize];
            let x = cx[k] - p.body_center_offset_x * body_scale;
            let y = cy[k] - p.body_center_offset_y * body_scale;
            if !(vector_length(x - item.x, y - item.y) > 0.01) {
                continue;
            }
            item.x = x;
            item.y = y;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_petal(&mut self, index: usize, slot_x: f64, slot_y: f64, state_key: u64, was_grouped_last_frame: bool, offset_x: f64, offset_y: f64, compressed_scale: f64, smoothing_k: f64, blend_step: f64) -> bool {
        self.input.seen[index] = true;
        let lift = self.input.size_scale[index] * compressed_scale;
        let target_x = slot_x - self.input.px[index] - offset_x * lift;
        let target_y = slot_y - self.input.py[index] - offset_y * lift;
        let state = self.input.state[index].get_or_insert(PinMotionState {
            offset_x: target_x,
            offset_y: target_y,
            scale: compressed_scale,
            morph: 1.0,
            cluster_blend: if was_grouped_last_frame { 1.0 } else { 0.0 },
            last_cluster_key: Some(state_key),
        });
        state.last_cluster_key = Some(state_key);
        let offset_active = smooth_offset_toward(state, target_x, target_y, smoothing_k, OFFSET_SNAP_EPSILON_PX);
        let scale_active = smooth_toward(&mut state.scale, compressed_scale, smoothing_k, SCALE_SNAP_EPSILON);
        let morph_active = smooth_toward(&mut state.morph, 1.0, smoothing_k, SCALE_SNAP_EPSILON);
        let blend_active = advance_blend(&mut state.cluster_blend, 0.0, blend_step);
        let state = *state;
        let x = self.input.px[index] + state.offset_x;
        let y = self.input.py[index] + state.offset_y;
        let alpha = self.input.alpha[index] * (1.0 - state.cluster_blend);
        self.emit(index, x, y, alpha, state.scale, state.morph, true);
        offset_active || scale_active || morph_active || blend_active
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_hidden_member(&mut self, index: usize, center_x: f64, center_y: f64, state_key: u64, compressed_scale: f64, smoothing_k: f64, blend_step: f64) -> bool {
        self.input.seen[index] = true;
        let Some(state) = self.input.state[index].as_mut() else { return false };
        state.last_cluster_key = Some(state_key);
        let blend_active = advance_blend(&mut state.cluster_blend, 1.0, blend_step);
        let offset_active = smooth_offset_toward(state, 0.0, 0.0, smoothing_k, OFFSET_SNAP_EPSILON_PX);
        let scale_active = smooth_toward(&mut state.scale, compressed_scale, smoothing_k, SCALE_SNAP_EPSILON);
        let morph_active = smooth_toward(&mut state.morph, 1.0, smoothing_k, SCALE_SNAP_EPSILON);
        let active = blend_active || offset_active || scale_active || morph_active;
        let state = *state;
        if state.cluster_blend >= 0.999 && !active {
            self.input.state[index] = None;
            return active;
        }
        if !(state.cluster_blend < 0.999) {
            return active;
        }
        let ease = smoothstep(0.0, 1.0, state.cluster_blend);
        let base_x = self.input.px[index] + state.offset_x;
        let base_y = self.input.py[index] + state.offset_y;
        let alpha = self.input.alpha[index] * (1.0 - state.cluster_blend);
        self.emit(index, base_x + (center_x - base_x) * ease, base_y + (center_y - base_y) * ease, alpha, state.scale, state.morph, false);
        active
    }

    // MARK: Grouping

    fn make_seed(&self, member_indexes: Vec<u32>) -> ClusterSeed {
        let input = &self.input;
        let first = member_indexes[0] as usize;
        let (mut min_x, mut min_y) = (input.px[first], input.py[first]);
        let (mut max_x, mut max_y) = (min_x, min_y);
        let (mut sum_x, mut sum_y) = (0.0, 0.0);
        for &m in &member_indexes {
            let (x, y) = (input.px[m as usize], input.py[m as usize]);
            sum_x += x;
            sum_y += y;
            if x < min_x {
                min_x = x;
            }
            if y < min_y {
                min_y = y;
            }
            if x > max_x {
                max_x = x;
            }
            if y > max_y {
                max_y = y;
            }
        }
        let count = member_indexes.len() as f64;
        ClusterSeed {
            state_key: input.keys[first],
            center_x: sum_x / count,
            center_y: sum_y / count,
            member_indexes,
            bounds_min_x: min_x,
            bounds_min_y: min_y,
            bounds_max_x: max_x,
            bounds_max_y: max_y,
        }
    }

    fn event_cluster_seeds(&mut self, pin_size_px: f64, padding: f64) -> Vec<ClusterSeed> {
        let threshold = (pin_size_px + padding * 2.0).max(1.0);
        let candidates: Vec<u32> = (0..self.input.keys.len() as u32).filter(|&i| self.input.event[i as usize]).collect();
        self.join = Some(Join::Event);
        self.join_radius = threshold;
        let groups = self.connected_groups(&candidates, false, threshold);
        groups.into_iter().map(|group| self.make_seed(group)).collect()
    }

    fn overflow_cluster_seeds(&mut self, excluded: &[bool], any_excluded: bool, pin_size_px: f64, compactness_limit: f64, grouping_threshold: f64) -> Vec<ClusterSeed> {
        let enter_radius = pin_size_px * GROUPING_RADIUS_SCALE;
        let exit_radius = enter_radius * GROUPING_HYSTERESIS_RATIO;
        let candidates: Vec<u32> = (0..self.input.keys.len() as u32).filter(|&i| !any_excluded || !excluded[i as usize]).collect();
        self.join = Some(Join::Overflow);
        self.join_radius = enter_radius;
        self.join_exit_radius = exit_radius;
        let components = self.connected_groups(&candidates, false, exit_radius);
        self.split_sprawling_components(components, compactness_limit, grouping_threshold)
            .into_iter()
            .filter(|group| group.len() > 1 && group.len() as f64 >= grouping_threshold)
            .map(|group| self.make_seed(group))
            .collect()
    }

    fn split_sprawling_components(&self, components: Vec<Vec<u32>>, compactness_limit: f64, grouping_threshold: f64) -> Vec<Vec<u32>> {
        let input = &self.input;
        let mut result: Vec<Vec<u32>> = Vec::with_capacity(components.len());
        for component in components {
            if component.len() <= 1 {
                result.push(component);
                continue;
            }
            let first = component[0] as usize;
            let (mut min_x, mut min_y, mut max_x, mut max_y) = (input.px[first], input.py[first], input.px[first], input.py[first]);
            for &index in &component[1..] {
                let index = index as usize;
                min_x = min_x.min(input.px[index]);
                min_y = min_y.min(input.py[index]);
                max_x = max_x.max(input.px[index]);
                max_y = max_y.max(input.py[index]);
            }
            let screen_diagonal = vector_length(max_x - min_x, max_y - min_y);
            if !(screen_diagonal > compactness_limit) {
                result.push(component);
                continue;
            }
            let (mut wmin_x, mut wmin_y, mut wmax_x, mut wmax_y) = (input.wx[first], input.wy[first], input.wx[first], input.wy[first]);
            for &index in &component[1..] {
                let index = index as usize;
                wmin_x = wmin_x.min(input.wx[index]);
                wmin_y = wmin_y.min(input.wy[index]);
                wmax_x = wmax_x.max(input.wx[index]);
                wmax_y = wmax_y.max(input.wy[index]);
            }
            let world_diagonal = vector_length(wmax_x - wmin_x, wmax_y - wmin_y);
            if !(world_diagonal > 0.0) {
                result.push(component);
                continue;
            }
            let pixels_per_world_unit = screen_diagonal / world_diagonal;
            let raw_step = compactness_limit / pixels_per_world_unit;
            let step = pow(2.0, if raw_step.is_finite() { log2(raw_step).floor() } else { 0.0 });
            let base_x = (wmin_x / step).floor();
            let base_y = (wmin_y / step).floor();
            // Buckets by world cell, in the order of their first member, which is their smallest.
            let cell_keys: Vec<f64> = component.iter().map(|&index| ((input.wx[index as usize] / step).floor() - base_x) * 2097152.0 + ((input.wy[index as usize] / step).floor() - base_y)).collect();
            let mut order: Vec<u32> = (0..component.len() as u32).collect();
            merge_sort(&mut order, &|&a: &u32, &b: &u32| cell_keys[a as usize] < cell_keys[b as usize]);
            let mut subgroups: Vec<Vec<u32>> = Vec::new();
            for (k, &o) in order.iter().enumerate() {
                if k == 0 || cell_keys[o as usize] != cell_keys[order[k - 1] as usize] {
                    subgroups.push(Vec::new());
                }
                subgroups.last_mut().unwrap_or_else(|| unreachable!()).push(component[o as usize]);
            }
            let mut by_first: Vec<u32> = (0..subgroups.len() as u32).collect();
            merge_sort(&mut by_first, &|&a: &u32, &b: &u32| subgroups[a as usize][0] < subgroups[b as usize][0]);
            let mut slots: Vec<Option<Vec<u32>>> = subgroups.into_iter().map(Some).collect();
            let mut subgroups: Vec<Vec<u32>> = by_first.iter().map(|&g| slots[g as usize].take().unwrap_or_default()).collect();

            let seed_threshold = grouping_threshold.max(2.0);
            let mut full: Vec<(usize, f64, f64)> = Vec::new();
            for (g, subgroup) in subgroups.iter().enumerate() {
                if !(subgroup.len() as f64 >= seed_threshold) {
                    continue;
                }
                let (mut sum_x, mut sum_y) = (0.0, 0.0);
                for &index in subgroup {
                    sum_x += input.px[index as usize];
                    sum_y += input.py[index as usize];
                }
                full.push((g, sum_x / subgroup.len() as f64, sum_y / subgroup.len() as f64));
            }
            if !full.is_empty() {
                for g in 0..subgroups.len() {
                    if subgroups[g].len() as f64 >= seed_threshold {
                        continue;
                    }
                    let (mut sum_x, mut sum_y) = (0.0, 0.0);
                    for &index in &subgroups[g] {
                        sum_x += input.px[index as usize];
                        sum_y += input.py[index as usize];
                    }
                    let length = subgroups[g].len() as f64;
                    let (centroid_x, centroid_y) = (sum_x / length, sum_y / length);
                    let mut nearest = full[0].0;
                    let mut nearest_distance = f64::MAX;
                    for &(candidate, x, y) in &full {
                        let dx = x - centroid_x;
                        let dy = y - centroid_y;
                        let distance = dx * dx + dy * dy;
                        if distance < nearest_distance {
                            nearest_distance = distance;
                            nearest = candidate;
                        }
                    }
                    let moved = std::mem::take(&mut subgroups[g]);
                    subgroups[nearest].extend_from_slice(&moved);
                }
                subgroups.retain(|subgroup| !subgroup.is_empty());
                for subgroup in &mut subgroups {
                    merge_sort(subgroup, &|a: &u32, b: &u32| a < b);
                }
            }
            result.extend(subgroups);
        }
        result
    }

    fn joinable(&self, lhs: usize, rhs: usize) -> bool {
        let input = &self.input;
        match self.join {
            Some(Join::Event) => {
                let dx = input.px[lhs] - input.px[rhs];
                let dy = input.py[lhs] - input.py[rhs];
                dx * dx + dy * dy <= self.join_radius * self.join_radius
            }
            Some(Join::Overflow) => {
                let widened = input.was_grouped[lhs] && input.was_grouped[rhs];
                let radius = if widened { self.join_exit_radius } else { self.join_radius };
                let dx = input.px[lhs] - input.px[rhs];
                let dy = input.py[lhs] - input.py[rhs];
                dx * dx + dy * dy <= radius * radius
            }
            _ => {
                let min_distance = self.body_reach[lhs] + self.body_reach[rhs];
                let dx = self.body_x[lhs] - self.body_x[rhs];
                let dy = self.body_y[lhs] - self.body_y[rhs];
                dx * dx + dy * dy < min_distance * min_distance
            }
        }
    }

    /// Components of two or more members by the join predicate, in the order
    /// of their smallest member, each ascending. Positions are the screen
    /// anchors, or the body centers when `bodies`.
    fn connected_groups(&mut self, candidates: &[u32], bodies: bool, max_join_distance: f64) -> Vec<Vec<u32>> {
        let count = candidates.len();
        if count <= 1 {
            return Vec::new();
        }
        self.positions.clear();
        for &c in candidates {
            let c = c as usize;
            if bodies {
                self.positions.push(self.body_x[c]);
                self.positions.push(self.body_y[c]);
            } else {
                self.positions.push(self.input.px[c]);
                self.positions.push(self.input.py[c]);
            }
        }
        let mut grid = std::mem::take(&mut self.grid);
        grid.build(&self.positions, max_join_distance);
        let mut set = std::mem::take(&mut self.disjoint_set);
        set.reset(count);
        let starts = &grid.starts;
        let entries = &grid.entries;
        let neighbor_slots = &grid.neighbor_slots;
        let population = |slot: usize| starts[slot + 1] - starts[slot];

        for slot in 0..grid.cell_count() {
            if population(slot) < DENSE_GROUPING_CELL_POPULATION {
                continue;
            }
            let begin = starts[slot] as usize;
            let end = starts[slot + 1] as usize;
            let first = entries[begin] as usize;
            for &e in &entries[begin + 1..end] {
                set.union(first, e as usize);
            }
            for offset in 0..9 {
                let neighbor = neighbor_slots[slot * 9 + offset];
                if neighbor <= slot as i32 || population(neighbor as usize) < DENSE_GROUPING_CELL_POPULATION {
                    continue;
                }
                set.union(first, entries[starts[neighbor as usize] as usize] as usize);
            }
        }

        for lhs in 0..count {
            let own = grid.slot_for_point[lhs] as usize;
            if population(own) >= DENSE_GROUPING_CELL_POPULATION {
                continue;
            }
            let lhs_input = candidates[lhs] as usize;
            for offset in 0..9 {
                let slot = neighbor_slots[own * 9 + offset];
                if slot < 0 {
                    continue;
                }
                let begin = starts[slot as usize] as usize;
                let end = starts[slot as usize + 1] as usize;
                if (end - begin) as u32 >= DENSE_GROUPING_CELL_POPULATION {
                    let mut probes_left = DENSE_GROUPING_PROBE_LIMIT;
                    for &rhs in &entries[begin..end] {
                        if self.joinable(lhs_input, candidates[rhs as usize] as usize) {
                            set.union(lhs, rhs as usize);
                            break;
                        }
                        probes_left -= 1;
                        if probes_left <= 0 {
                            break;
                        }
                    }
                    continue;
                }
                for &rhs in &entries[begin..end] {
                    if rhs as usize > lhs && self.joinable(lhs_input, candidates[rhs as usize] as usize) {
                        set.union(lhs, rhs as usize);
                    }
                }
            }
        }

        let mut roots = vec![0u32; count];
        let mut sizes = vec![0u32; count];
        for (k, root) in roots.iter_mut().enumerate() {
            *root = set.find(k) as u32;
            sizes[*root as usize] += 1;
        }
        let mut group_of_root = vec![-1i32; count];
        let mut groups: Vec<Vec<u32>> = Vec::new();
        for k in 0..count {
            let root = roots[k] as usize;
            if sizes[root] < 2 {
                continue;
            }
            if group_of_root[root] < 0 {
                group_of_root[root] = groups.len() as i32;
                groups.push(Vec::new());
            }
            groups[group_of_root[root] as usize].push(candidates[k]);
        }
        self.grid = grid;
        self.disjoint_set = set;
        groups
    }

    fn merge_intersecting_flowers(&self, seeds: Vec<ClusterSeed>, petal_body_radius: f64, compactness_limit: f64) -> Vec<ClusterSeed> {
        if seeds.len() <= 1 {
            return seeds;
        }
        let limit_squared = compactness_limit * compactness_limit;
        let mut merged = sort_seeds(seeds);
        let mut set = PinDisjointSet::default();
        while merged.len() > 1 {
            set.reset(merged.len());
            let mut did_merge = false;
            for l in 0..merged.len() {
                let lhs = &merged[l];
                let lhs_radius = flower_outer_radius(lhs.member_count(), petal_body_radius);
                for r in l + 1..merged.len() {
                    let rhs = &merged[r];
                    let min_distance = lhs_radius + flower_outer_radius(rhs.member_count(), petal_body_radius);
                    let dx = lhs.center_x - rhs.center_x;
                    let dy = lhs.center_y - rhs.center_y;
                    if !(dx * dx + dy * dy < min_distance * min_distance) {
                        continue;
                    }
                    let span_x = lhs.bounds_max_x.max(rhs.bounds_max_x) - lhs.bounds_min_x.min(rhs.bounds_min_x);
                    let span_y = lhs.bounds_max_y.max(rhs.bounds_max_y) - lhs.bounds_min_y.min(rhs.bounds_min_y);
                    if !(span_x * span_x + span_y * span_y <= limit_squared) {
                        continue;
                    }
                    set.union(l, r);
                    did_merge = true;
                }
            }
            if !did_merge {
                break;
            }
            let mut component_of_root = vec![-1i32; merged.len()];
            let mut components: Vec<Vec<usize>> = Vec::new();
            for k in 0..merged.len() {
                let root = set.find(k);
                if component_of_root[root] < 0 {
                    component_of_root[root] = components.len() as i32;
                    components.push(Vec::new());
                }
                components[component_of_root[root] as usize].push(k);
            }
            let mut next: Vec<ClusterSeed> = Vec::with_capacity(components.len());
            for indexes in components {
                if indexes.len() == 1 {
                    next.push(merged[indexes[0]].clone());
                    continue;
                }
                let mut members = merged[indexes[0]].member_indexes.clone();
                for &k in &indexes[1..] {
                    members = merge_sorted_indexes(&members, &merged[k].member_indexes);
                }
                next.push(self.make_seed(members));
            }
            merged = sort_seeds(next);
        }
        merged
    }

    fn absorb_pins_into_flowers(&self, seeds: &mut [ClusterSeed], standalone: Vec<u32>, p: &PinSolveParameters, petal_body_radius: f64, padding: f64) -> Vec<u32> {
        if seeds.is_empty() || standalone.is_empty() {
            return standalone;
        }
        let input = &self.input;
        let mut absorbed = vec![false; input.keys.len()];
        let mut any = false;
        for &index in &standalone {
            let i = index as usize;
            let scale = input.size_scale[i];
            let is_selected = input.selected[i];
            let center_x = input.px[i] + p.body_center_offset_x * scale;
            let center_y = input.py[i] + p.body_center_offset_y * scale;
            let minimal_body_radius = petal_body_radius * scale;
            let mut touched_seed_count = 0;
            let mut nearest_touched_seed: i32 = -1;
            let mut nearest_touched_distance = f64::MAX;
            for s in 0..seeds.len() {
                let seed = &seeds[s];
                let ring_radius = flower_outer_radius(seed.member_count(), petal_body_radius) - petal_body_radius;
                let threshold = if is_selected { ring_radius + petal_body_radius + p.body_radius_px * scale + padding } else { ring_radius };
                let dx = center_x - seed.center_x;
                let dy = center_y - seed.center_y;
                let distance_squared = dx * dx + dy * dy;
                if distance_squared < threshold * threshold {
                    seeds[s] = self.make_seed(merge_sorted_indexes(&seed.member_indexes, &[index]));
                    absorbed[i] = true;
                    any = true;
                    break;
                }
                if is_selected {
                    continue;
                }
                let petal_count = seed.member_count().min(MAX_FLOWER_PETALS);
                let slot_ring = flower_ring_radius(petal_body_radius, petal_count);
                let touch_distance = minimal_body_radius + petal_body_radius;
                // A pin farther from the center than the ring plus the touch
                // distance touches no petal (a pixel of margin covers the rounding).
                let reach = slot_ring + touch_distance + 1.0;
                if distance_squared >= reach * reach {
                    continue;
                }
                for petal in 0..petal_count {
                    let (cos, sin) = petal_direction(petal, petal_count);
                    let distance = vector_length(center_x - (seed.center_x + cos * slot_ring), center_y - (seed.center_y + sin * slot_ring));
                    if distance < touch_distance {
                        touched_seed_count += 1;
                        if distance < nearest_touched_distance {
                            nearest_touched_distance = distance;
                            nearest_touched_seed = s as i32;
                        }
                        break;
                    }
                }
            }
            if !absorbed[i] && touched_seed_count >= 2 && nearest_touched_seed >= 0 {
                let s = nearest_touched_seed as usize;
                seeds[s] = self.make_seed(merge_sorted_indexes(&seeds[s].member_indexes, &[index]));
                absorbed[i] = true;
                any = true;
            }
        }
        if !any {
            return standalone;
        }
        standalone.into_iter().filter(|&index| !absorbed[index as usize]).collect()
    }

    // MARK: Relaxation

    fn make_nodes(&mut self, standalone: &[u32], rigid: &[bool], seeds: &[ClusterSeed], p: &PinSolveParameters, morphs: Option<&[f64]>) {
        let compressed_scale = p.compressed_scale.min(1.0);
        let petal_body_radius = p.circle_body_radius_px * compressed_scale;
        let padding = p.collision_padding_px;
        let input = &self.input;
        let jitter = &self.jitter;
        let nodes = &mut self.nodes;
        nodes.clear();
        for (s, &index) in standalone.iter().enumerate() {
            let i = index as usize;
            let (shape_scale, shape_radius) = match morphs {
                None => (1.0, p.body_radius_px),
                Some(morphs) => (scale_cap(morphs[s]), p.body_radius(morphs[s])),
            };
            let size_scale = input.size_scale[i];
            let center_x = input.px[i] + p.body_center_offset_x * (shape_scale * size_scale);
            let center_y = input.py[i] + p.body_center_offset_y * (shape_scale * size_scale);
            let radius = shape_radius * shape_scale * size_scale + padding;
            let (jitter_x, jitter_y) = jitter[s];
            nodes.push(center_x, center_y, center_x + jitter_x, center_y + jitter_y, radius.max(1.0), if rigid[s] { 0.0 } else { 1.0 }, input.keys[i]);
        }
        for seed in seeds {
            let petal_count = seed.member_count().min(MAX_FLOWER_PETALS);
            let ring_radius = flower_ring_radius(petal_body_radius, petal_count);
            for petal in 0..petal_count {
                let (cos, sin) = petal_direction(petal, petal_count);
                let slot_x = seed.center_x + cos * ring_radius;
                let slot_y = seed.center_y + sin * ring_radius;
                nodes.push(slot_x, slot_y, slot_x, slot_y, petal_body_radius + padding, 0.0, seed.state_key.wrapping_add(petal as u64));
            }
        }
    }

    fn dominant_flags(&mut self, standalone: &[u32], seeds: &[ClusterSeed], p: &PinSolveParameters, compressed_scale: f64, padding: f64) -> Vec<bool> {
        let count = standalone.len();
        let mut dominant = vec![true; count];
        if count == 0 {
            return dominant;
        }
        let total = self.input.keys.len();
        self.body_x.resize(total, 0.0);
        self.body_y.resize(total, 0.0);
        self.body_reach.resize(total, 0.0);
        let mut max_body_reach: f64 = 1.0;
        for &index in standalone {
            let i = index as usize;
            let scale = self.input.size_scale[i];
            self.body_x[i] = self.input.px[i] + p.body_center_offset_x * scale;
            self.body_y[i] = self.input.py[i] + p.body_center_offset_y * scale;
            self.body_reach[i] = p.body_radius_px * scale + padding;
            max_body_reach = max_body_reach.max(self.body_reach[i]);
        }
        let mut standalone_position = vec![0u32; total];
        for (s, &index) in standalone.iter().enumerate() {
            standalone_position[index as usize] = s as u32;
        }
        self.join = Some(Join::Bodies);
        let groups = self.connected_groups(standalone, true, 2.0 * max_body_reach);
        for group in groups {
            if group.len() != 2 {
                for &index in &group {
                    dominant[standalone_position[index as usize] as usize] = false;
                }
                continue;
            }
            let mut ranked = group[0];
            for &candidate in &group[1..] {
                if self.ranks_before(candidate as usize, ranked as usize) {
                    ranked = candidate;
                }
            }
            for &index in &group {
                if index != ranked {
                    dominant[standalone_position[index as usize] as usize] = false;
                }
            }
        }

        if seeds.is_empty() {
            return dominant;
        }
        let petal_body_radius = p.circle_body_radius_px * compressed_scale;
        let mut slots: Vec<f64> = Vec::new();
        for seed in seeds {
            let petal_count = seed.member_count().min(MAX_FLOWER_PETALS);
            let ring_radius = flower_ring_radius(petal_body_radius, petal_count);
            for petal in 0..petal_count {
                let (cos, sin) = petal_direction(petal, petal_count);
                slots.push(seed.center_x + cos * ring_radius);
                slots.push(seed.center_y + sin * ring_radius);
            }
        }
        self.grid.build(&slots, max_body_reach + petal_body_radius + padding);
        let mut candidates = std::mem::take(&mut self.candidates);
        for (s, &index) in standalone.iter().enumerate() {
            let i = index as usize;
            if self.input.selected[i] {
                continue;
            }
            let min_distance = self.body_reach[i] + petal_body_radius + padding;
            let (x, y) = (self.body_x[i], self.body_y[i]);
            self.grid.collect_candidates(x, y, &mut candidates);
            for &slot in &candidates {
                let dx = x - slots[slot as usize * 2];
                let dy = y - slots[slot as usize * 2 + 1];
                if dx * dx + dy * dy < min_distance * min_distance {
                    dominant[s] = false;
                    break;
                }
            }
        }
        self.candidates = candidates;
        dominant
    }

    fn ranks_before(&self, a: usize, b: usize) -> bool {
        let input = &self.input;
        if input.selected[a] != input.selected[b] {
            return input.selected[a];
        }
        if input.priority[a] != input.priority[b] {
            return input.priority[a] > input.priority[b];
        }
        input.ids[a] < input.ids[b]
    }

    fn relax(&mut self, p: &PinSolveParameters) {
        let nodes = &mut self.nodes;
        let count = nodes.len();
        if count > 1 {
            let spring_k = p.spring_k.max(0.0).min(1.0);
            let mut max_radius: f64 = 1.0;
            nodes.positions.clear();
            for i in 0..count {
                max_radius = max_radius.max(nodes.radius[i]);
                nodes.positions.push(nodes.position_x[i]);
                nodes.positions.push(nodes.position_y[i]);
            }
            self.grid.build(&nodes.positions, 3.0 * max_radius);
            // A pair stays a candidate if it can touch after closing in by the maxRadius budget.
            let (radius, positions) = (&nodes.radius, &nodes.positions);
            self.grid.ascending_pairs(
                |i, j| {
                    let reach = radius[i] + radius[j] + max_radius;
                    let dx = positions[i * 2] - positions[j * 2];
                    let dy = positions[i * 2 + 1] - positions[j * 2 + 1];
                    dx * dx + dy * dy < reach * reach
                },
                &mut nodes.pair_starts,
                &mut nodes.pair_neighbors,
                &mut self.scratch,
            );
            let iterations = p.collision_iterations.floor().max(1.0) as usize;
            let SolverNodes { anchor_x: ax, anchor_y: ay, position_x: x, position_y: y, radius, inverse_mass, direction_key: keys, pair_starts, pair_neighbors, .. } = nodes;
            for _ in 0..iterations {
                for i in 0..count {
                    if inverse_mass[i] > 0.0 {
                        x[i] += (ax[i] - x[i]) * spring_k;
                        y[i] += (ay[i] - y[i]) * spring_k;
                    } else {
                        x[i] = ax[i];
                        y[i] = ay[i];
                    }
                }
                for l in 0..count {
                    let lhs_mass = inverse_mass[l];
                    let lhs_radius = radius[l];
                    for &r in &pair_neighbors[pair_starts[l] as usize..pair_starts[l + 1] as usize] {
                        let r = r as usize;
                        let rhs_mass = inverse_mass[r];
                        let inverse_mass_sum = lhs_mass + rhs_mass;
                        if !(inverse_mass_sum > 0.0) {
                            continue;
                        }
                        let min_distance = lhs_radius + radius[r];
                        let dx = x[r] - x[l];
                        let dy = y[r] - y[l];
                        let distance_squared = dx * dx + dy * dy;
                        if !(distance_squared < min_distance * min_distance) {
                            continue;
                        }
                        let distance = distance_squared.sqrt();
                        let (dir_x, dir_y) = if distance > 1e-4 { (dx / distance, dy / distance) } else { stable_unit_direction(keys[l], keys[r]) };
                        let overlap = min_distance - distance;
                        let lhs_share = overlap * (lhs_mass / inverse_mass_sum);
                        let rhs_share = overlap * (rhs_mass / inverse_mass_sum);
                        x[l] -= dir_x * lhs_share;
                        y[l] -= dir_y * lhs_share;
                        x[r] += dir_x * rhs_share;
                        y[r] += dir_y * rhs_share;
                    }
                }
            }
        } else {
            for i in 0..count {
                nodes.position_x[i] = nodes.anchor_x[i];
                nodes.position_y[i] = nodes.anchor_y[i];
            }
        }
        let max_offset = p.max_offset_px.max(0.0);
        for i in 0..count {
            let dx = nodes.position_x[i] - nodes.anchor_x[i];
            let dy = nodes.position_y[i] - nodes.anchor_y[i];
            let length = vector_length(dx, dy);
            if length > max_offset {
                let factor = max_offset / length.max(1e-6);
                nodes.position_x[i] = nodes.anchor_x[i] + dx * factor;
                nodes.position_y[i] = nodes.anchor_y[i] + dy * factor;
            } else if length < REST_SNAP_RADIUS_PX {
                nodes.position_x[i] = nodes.anchor_x[i];
                nodes.position_y[i] = nodes.anchor_y[i];
            }
        }
    }
}

fn separate_flower_rings(seeds: &mut [ClusterSeed], petal_body_radius: f64) {
    if seeds.len() <= 1 {
        return;
    }
    for _ in 0..8 {
        let mut moved = false;
        for l in 0..seeds.len() {
            let lhs_radius = flower_outer_radius(seeds[l].member_count(), petal_body_radius);
            for r in l + 1..seeds.len() {
                let min_distance = lhs_radius + flower_outer_radius(seeds[r].member_count(), petal_body_radius);
                let dx = seeds[r].center_x - seeds[l].center_x;
                let dy = seeds[r].center_y - seeds[l].center_y;
                let distance_squared = dx * dx + dy * dy;
                if !(distance_squared < min_distance * min_distance) {
                    continue;
                }
                let distance = distance_squared.sqrt();
                let (dir_x, dir_y) = if distance > 1e-4 { (dx / distance, dy / distance) } else { stable_unit_direction(seeds[l].state_key, seeds[r].state_key) };
                let overlap = min_distance - distance;
                let (lc, rc) = (seeds[l].member_count(), seeds[r].member_count());
                let lhs_yields = lc < rc || (lc == rc && seeds[l].state_key > seeds[r].state_key);
                if lhs_yields {
                    seeds[l].center_x -= dir_x * overlap;
                    seeds[l].center_y -= dir_y * overlap;
                } else {
                    seeds[r].center_x += dir_x * overlap;
                    seeds[r].center_y += dir_y * overlap;
                }
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
}

// MARK: Smoothing

fn smooth_toward(value: &mut f64, target: f64, factor: f64, epsilon: f64) -> bool {
    let delta = target - *value;
    if delta.abs() <= epsilon {
        *value = target;
        return false;
    }
    *value += delta * factor;
    true
}

fn smooth_offset_toward(state: &mut PinMotionState, target_x: f64, target_y: f64, factor: f64, epsilon: f64) -> bool {
    let dx = target_x - state.offset_x;
    let dy = target_y - state.offset_y;
    if vector_length(dx, dy) <= epsilon {
        state.offset_x = target_x;
        state.offset_y = target_y;
        return false;
    }
    state.offset_x += dx * factor;
    state.offset_y += dy * factor;
    true
}

fn advance_blend(value: &mut f64, target: f64, step: f64) -> bool {
    let delta = target - *value;
    if delta.abs() <= 0.001 {
        *value = target;
        return false;
    }
    *value += if delta > 0.0 { step.min(delta) } else { (-step).max(delta) };
    true
}

#[cfg(test)]
mod tests {
    //! Ported from the Swift `PinCollisionMathTests`, `PinScreenHashGridTests`
    //! and `PinCollisionLayoutSolverTests`.

    use super::*;

    // MARK: Math

    #[test]
    fn the_smoothing_factor_is_zero_without_elapsed_time() {
        assert_eq!(smoothing_factor(0.5, 0.0), 0.0);
    }

    #[test]
    fn the_smoothing_factor_is_frame_rate_independent() {
        let single = smoothing_factor(0.35, 1.0 / 30.0);
        let half = smoothing_factor(0.35, 1.0 / 60.0);
        assert!((single - (1.0 - (1.0 - half) * (1.0 - half))).abs() < 1e-4);
    }

    #[test]
    fn the_stable_unit_direction_is_a_deterministic_unit_vector_independent_of_the_order() {
        let direction = stable_unit_direction(7, 42);
        assert!((vector_length(direction.0, direction.1) - 1.0).abs() < 1e-4);
        assert_eq!(direction, stable_unit_direction(42, 7));
        assert_eq!(direction, stable_unit_direction(7, 42));
    }

    #[test]
    fn the_required_scale_is_forced_only_by_lack_of_space() {
        assert_eq!(required_scale(120.0, 50.0, 50.0, 8.0, false), 1.0);
        assert!((required_scale(66.0, 50.0, 50.0, 8.0, false) - 0.5).abs() < 1e-4);
        assert!((required_scale(91.0, 50.0, 50.0, 8.0, true) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn the_scale_cap_keeps_displaced_circles_smaller_than_pins() {
        assert_eq!(scale_cap(0.0), 1.0);
        assert!((scale_cap(1.0) - DISPLACED_CIRCLE_SCALE).abs() < 1e-4);
        let partial = scale_cap(0.5);
        assert!(partial > DISPLACED_CIRCLE_SCALE && partial < 1.0);
    }

    #[test]
    fn the_flower_layout_packs_touching_petals_around_the_center() {
        let petal_radius = 30.0;
        let count = 5;
        let ring = flower_ring_radius(petal_radius, count);
        let offset = |index: usize| {
            let angle = flower_petal_angle(index, count);
            (angle.cos() * ring, angle.sin() * ring)
        };
        for index in 0..count {
            let (cx, cy) = offset(index);
            let (nx, ny) = offset((index + 1) % count);
            assert!((vector_length(cx, cy) - ring).abs() < 1e-3);
            assert!((vector_length(cx - nx, cy - ny) - petal_radius * 2.0).abs() < 1e-2);
        }
        assert!((flower_ring_radius(petal_radius, 2) - petal_radius).abs() < 1e-3);
        assert_eq!(flower_ring_radius(petal_radius, 1), 0.0);
    }

    #[test]
    fn the_displaced_morph_turns_a_shifted_pin_into_a_circle() {
        assert_eq!(displaced_morph(0.0), 0.0);
        assert!((displaced_morph(DISPLACED_MORPH_END_PX) - 1.0).abs() < 1e-4);
        assert_eq!(displaced_morph(100.0), 1.0);
        let partial = displaced_morph(6.0);
        assert!(partial > 0.0 && partial < 1.0);
    }

    #[test]
    fn the_badge_alpha_hides_badges_when_compressed() {
        assert_eq!(badge_content_alpha(1.0), 1.0);
        assert_eq!(badge_content_alpha(0.55), 0.0);
        let partial = badge_content_alpha(0.9);
        assert!(partial > 0.0 && partial < 1.0);
    }

    // MARK: Grid

    /// The Swift tests' SplitMix generator.
    struct SplitMix(u64);

    impl SplitMix {
        fn next(&mut self) -> f64 {
            self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
            ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    #[test]
    fn neighbor_pairs_match_a_brute_force_scan_including_the_order() {
        let mut generator = SplitMix(99);
        let positions: Vec<f64> = (0..600).flat_map(|_| [-500.0 + generator.next() * 2000.0, -400.0 + generator.next() * 1300.0]).collect();
        let radius = 130.0;
        let distance = |a: usize, b: usize| vector_length(positions[a * 2] - positions[b * 2], positions[a * 2 + 1] - positions[b * 2 + 1]);
        let grid = PinScreenHashGrid::new(&positions, radius);
        let mut brute = Vec::new();
        for a in 0..600 {
            for b in a + 1..600 {
                if distance(a, b) <= radius {
                    brute.push((a, b));
                }
            }
        }
        let (mut neighbors, mut scratch) = (Vec::new(), Vec::new());
        let mut via_grid = Vec::new();
        for a in 0..600 {
            grid.collect_neighbors(a, a as i64, &mut neighbors, &mut scratch);
            via_grid.extend(neighbors.iter().map(|&b| (a, b as usize)).filter(|&(a, b)| distance(a, b) <= radius));
        }
        assert_eq!(via_grid, brute);
        // The pair lists give the same pairs in the same order.
        let radii = vec![radius / 2.0; 600];
        let (mut starts, mut list) = (Vec::new(), Vec::new());
        grid.ascending_pairs(|i, j| distance(i, j) < radii[i] + radii[j], &mut starts, &mut list, &mut scratch);
        let mut via_pairs = Vec::new();
        for a in 0..600 {
            via_pairs.extend(list[starts[a] as usize..starts[a + 1] as usize].iter().map(|&b| (a, b as usize)));
        }
        assert_eq!(via_pairs, brute.into_iter().filter(|&(a, b)| distance(a, b) < radius).collect::<Vec<_>>());
    }

    #[test]
    fn negative_coordinates_and_cell_boundaries() {
        let grid = PinScreenHashGrid::new(&[-0.5, -0.5, 0.5, 0.5, -100.0, 0.0, 100.0, 0.0, 0.0, 0.0], 100.0);
        let (mut neighbors, mut scratch) = (Vec::new(), Vec::new());
        grid.collect_neighbors(0, -1, &mut neighbors, &mut scratch);
        assert_eq!(neighbors, [0, 1, 2, 4]);
        assert_eq!(grid.cell_population_at(0.5, 0.5), 2);
        assert_eq!(grid.cell_population_at(-0.5, -0.5), 1);
        assert_eq!(grid.cell_population_at(9999.0, 9999.0), 0);
    }

    #[test]
    fn empty_and_single_point() {
        let mut neighbors = vec![1, 2, 3];
        let mut scratch = Vec::new();
        PinScreenHashGrid::new(&[], 50.0).collect_candidates(0.0, 0.0, &mut neighbors);
        assert!(neighbors.is_empty());
        let single = PinScreenHashGrid::new(&[10.0, 10.0], 50.0);
        single.collect_neighbors(0, 0, &mut neighbors, &mut scratch);
        assert!(neighbors.is_empty());
        single.collect_neighbors(0, -1, &mut neighbors, &mut scratch);
        assert_eq!(neighbors, [0]);
        single.collect_candidates(12.0, 8.0, &mut neighbors);
        assert_eq!(neighbors, [0]);
    }

    #[test]
    fn a_dense_pile_keeps_all_entries_in_one_neighborhood() {
        let positions: Vec<f64> = (0..10_000).flat_map(|i| [(i % 100) as f64 * 0.1, (i / 100) as f64 * 0.1]).collect();
        let grid = PinScreenHashGrid::new(&positions, 64.0);
        assert_eq!(grid.cell_population_at(5.0, 5.0), 10_000);
        let (mut neighbors, mut scratch) = (Vec::new(), Vec::new());
        grid.collect_neighbors(0, 9_000, &mut neighbors, &mut scratch);
        assert_eq!(neighbors, (9_001..10_000).collect::<Vec<u32>>());
        assert_eq!(grid.cell_population_of_point(42), 10_000);
    }

    #[test]
    fn the_disjoint_set_survives_a_deep_chain() {
        let count = 50_000;
        let mut set = PinDisjointSet::default();
        set.reset(count);
        for index in (0..count - 1).rev() {
            set.union(index, index + 1);
        }
        assert_eq!(set.find(count - 1), 0);
        assert_eq!(set.find(count / 2), 0);
        assert_eq!(set.find(0), 0);
    }

    #[test]
    fn the_disjoint_set_root_is_the_minimum_index() {
        let mut set = PinDisjointSet::default();
        set.reset(6);
        set.union(4, 5);
        set.union(2, 4);
        assert_eq!(set.find(5), 2);
        set.union(5, 1);
        assert_eq!(set.find(4), 1);
        assert_eq!(set.find(0), 0);
        assert_eq!(set.find(3), 3);
    }

    #[test]
    fn the_merge_sort_is_stable_and_sorts() {
        let mut values: Vec<(u32, u32)> = (0..500u32).map(|i| ((i * 7919) % 37, i)).collect();
        merge_sort(&mut values, &|a: &(u32, u32), b: &(u32, u32)| a.0 < b.0);
        assert!(values.windows(2).all(|w| w[0].0 < w[1].0 || (w[0].0 == w[1].0 && w[0].1 < w[1].1)));
    }

    // MARK: Solver

    /// Body radius 50, visible circle 46, body center 55 px above the anchor.
    fn geometry_and(config: impl FnOnce(&mut PinSolveParameters)) -> PinSolveParameters {
        let mut p = PinSolveParameters {
            pin_size_px: 100.0,
            body_radius_px: 50.0,
            circle_body_radius_px: 46.0,
            body_center_offset_x: 0.0,
            body_center_offset_y: 55.0,
            compressed_scale: 0.55,
            collision_padding_px: 0.0,
            grouping_threshold: 10.0,
            max_offset_px: 220.0,
            collision_iterations: 10.0,
            spring_k: 0.25,
            smoothing: 0.35,
        };
        config(&mut p);
        p
    }

    fn config() -> PinSolveParameters {
        geometry_and(|_| {})
    }

    #[derive(Clone, Copy)]
    struct TestPin {
        id: u64,
        x: f64,
        y: f64,
        selected: bool,
        event: bool,
        world: Option<(f64, f64)>,
    }

    fn pin(id: u64, x: f64, y: f64) -> TestPin {
        TestPin { id, x, y, selected: false, event: false, world: None }
    }

    fn selected(id: u64, x: f64, y: f64) -> TestPin {
        TestPin { selected: true, ..pin(id, x, y) }
    }

    fn event(id: u64, x: f64, y: f64) -> TestPin {
        TestPin { event: true, ..pin(id, x, y) }
    }

    fn encode(pins: &[TestPin]) -> Vec<f64> {
        let mut out = Vec::new();
        for p in pins {
            let (wx, wy) = p.world.unwrap_or((p.x * 0.001, p.y * 0.001));
            let flags = if p.selected { FLAG_SELECTED } else { 0 } | if p.event { FLAG_EVENT } else { 0 };
            out.extend_from_slice(&[p.id as f64, p.x, p.y, 1.0, wx, wy, 1.0, 0.0, p.id as f64, flags as f64]);
        }
        out
    }

    #[derive(Clone, Debug)]
    struct Placed {
        id: u64,
        x: f64,
        y: f64,
        anchor_x: f64,
        anchor_y: f64,
        alpha: f64,
        scale: f64,
        morph: f64,
        petal: bool,
    }

    impl Placed {
        fn offset(&self) -> f64 {
            vector_length(self.x - self.anchor_x, self.y - self.anchor_y)
        }
        fn body(&self, p: &PinSolveParameters) -> (f64, f64) {
            (self.x + p.body_center_offset_x * self.scale, self.y + p.body_center_offset_y * self.scale)
        }
        fn body_radius(&self, p: &PinSolveParameters) -> f64 {
            p.body_radius(self.morph) * self.scale
        }
    }

    struct Flower {
        center: (f64, f64),
        members: Vec<u64>,
        visible: Vec<u64>,
    }

    struct Layout {
        items: Vec<Placed>,
        flowers: Vec<Flower>,
        animating: bool,
    }

    impl Layout {
        fn item(&self, id: u64) -> &Placed {
            self.items.iter().find(|item| item.id == id).unwrap_or_else(|| panic!("no item for pin {id}"))
        }
    }

    fn solve(solver: &mut PinCollisionSolver, pins: &[TestPin], p: &PinSolveParameters, time: f64) -> Layout {
        let input = encode(pins);
        let output = solver.solve_frame(&input, p, time);
        let items = (0..output.item_count())
            .map(|k| {
                let item = output.item(k);
                let source = &pins[item[0] as usize];
                Placed { id: source.id, x: item[1], y: item[2], anchor_x: source.x, anchor_y: source.y, alpha: item[3], scale: item[4], morph: item[5], petal: item[6] == 1.0 }
            })
            .collect();
        let flowers = output
            .flowers()
            .into_iter()
            .map(|(cx, cy, members, visible)| Flower { center: (cx, cy), members: members.iter().map(|&m| pins[m].id).collect(), visible: visible.iter().map(|&m| pins[m].id).collect() })
            .collect();
        Layout { items, flowers, animating: output.has_active_animations() }
    }

    fn settle_at(solver: &mut PinCollisionSolver, pins: &[TestPin], p: &PinSolveParameters, time: &mut f64) -> Layout {
        for iteration in 0..1200 {
            let layout = solve(solver, pins, p, *time);
            if !layout.animating && iteration > 0 {
                return layout;
            }
            *time += 1.0 / 60.0;
        }
        panic!("the solver did not settle within the iteration budget");
    }

    fn settle(pins: &[TestPin], p: &PinSolveParameters) -> Layout {
        settle_at(&mut PinCollisionSolver::new(), pins, p, &mut 0.0)
    }

    fn assert_no_body_overlaps(layout: &Layout, p: &PinSolveParameters, tolerance: f64) {
        for (l, lhs) in layout.items.iter().enumerate() {
            for rhs in &layout.items[l + 1..] {
                let ((lx, ly), (rx, ry)) = (lhs.body(p), rhs.body(p));
                let distance = vector_length(lx - rx, ly - ry);
                assert!(distance >= lhs.body_radius(p) + rhs.body_radius(p) - tolerance, "the bodies of pins {} and {} overlap", lhs.id, rhs.id);
            }
        }
    }

    #[test]
    fn a_lone_pin_stays_at_its_anchor_as_a_full_sized_pin() {
        let layout = solve(&mut PinCollisionSolver::new(), &[pin(1, 400.0, 300.0)], &config(), 0.0);
        assert_eq!(layout.items.len(), 1);
        let item = &layout.items[0];
        assert_eq!((item.x, item.y, item.scale, item.morph), (400.0, 300.0, 1.0, 0.0));
        assert!(!layout.animating);
    }

    #[test]
    fn displaced_pins_become_smaller_circles() {
        let p = config();
        let layout = settle(&[pin(1, 400.0, 300.0), pin(2, 415.0, 300.0), pin(3, 400.0, 315.0)], &p);
        for item in &layout.items {
            assert!((item.morph - 1.0).abs() < 0.01);
            assert!(item.scale <= DISPLACED_CIRCLE_SCALE + 0.001 && item.scale >= p.compressed_scale - 0.001);
        }
        assert_no_body_overlaps(&layout, &p, 1.0);
    }

    #[test]
    fn a_pair_keeps_the_dominant_pin_and_the_other_yields_as_a_circle() {
        let p = config();
        let layout = settle(&[pin(1, 400.0, 300.0), pin(2, 460.0, 300.0)], &p);
        let (dominant, yielding) = (layout.item(1), layout.item(2));
        assert_eq!((dominant.x, dominant.y, dominant.morph, dominant.scale), (400.0, 300.0, 0.0, 1.0));
        assert!((yielding.morph - 1.0).abs() < 0.01);
        assert!(yielding.scale <= DISPLACED_CIRCLE_SCALE + 0.001);
        assert!(yielding.offset() > 10.0);
        assert_no_body_overlaps(&layout, &p, 2.0);
    }

    #[test]
    fn the_yielding_circle_touches_the_dominant_without_a_gap() {
        let p = config();
        let layout = settle(&[pin(1, 400.0, 300.0), pin(2, 440.0, 300.0)], &p);
        let (dominant, yielding) = (layout.item(1), layout.item(2));
        let ((dx, dy), (yx, yy)) = (dominant.body(&p), yielding.body(&p));
        assert!((vector_length(dx - yx, dy - yy) - (dominant.body_radius(&p) + yielding.body_radius(&p))).abs() <= 1.5);
    }

    #[test]
    fn a_pin_reacts_only_to_the_actual_circle_body() {
        let p = config();
        let layout = settle(&[pin(1, 400.0, 300.0), pin(2, 400.0, 380.0), pin(3, 400.0, 505.0)], &p);
        assert_eq!(layout.item(1).morph, 0.0);
        assert!((layout.item(2).morph - 1.0).abs() < 0.01);
        let bystander = layout.item(3);
        assert!(bystander.morph.abs() < 0.05 && (bystander.scale - 1.0).abs() < 0.01);
        assert_eq!((bystander.x, bystander.y), (400.0, 505.0));
        assert_no_body_overlaps(&layout, &p, 2.0);
    }

    #[test]
    fn three_close_pins_all_become_circles() {
        let p = config();
        let layout = settle(&[pin(1, 400.0, 300.0), pin(2, 400.0, 300.0), pin(3, 400.0, 300.0)], &p);
        assert_eq!(layout.items.len(), 3);
        for item in &layout.items {
            assert!((item.morph - 1.0).abs() < 0.01 && item.scale <= DISPLACED_CIRCLE_SCALE + 0.001 && item.offset() > 5.0);
        }
        assert_no_body_overlaps(&layout, &p, 1.5);
    }

    #[test]
    fn pins_do_not_react_before_their_bodies_touch() {
        let layout = settle(&[pin(1, 400.0, 300.0), pin(2, 501.0, 300.0)], &config());
        for item in &layout.items {
            assert_eq!((item.x, item.y, item.scale, item.morph), (item.anchor_x, item.anchor_y, 1.0, 0.0));
        }
    }

    #[test]
    fn a_crowd_compresses_circles_down_to_the_limit_only() {
        let p = geometry_and(|p| p.max_offset_px = 60.0);
        let mut pins: Vec<TestPin> = (1..=6).map(|id| pin(id, 400.0, 300.0)).collect();
        pins.push(pin(99, 900.0, 300.0));
        let layout = settle(&pins, &p);
        assert_eq!((layout.item(99).scale, layout.item(99).morph), (1.0, 0.0));
        for item in layout.items.iter().filter(|item| item.id != 99) {
            assert!(item.scale < DISPLACED_CIRCLE_SCALE + 0.001 && item.scale >= p.compressed_scale - 0.001);
        }
    }

    #[test]
    fn mixed_pin_and_circle_bodies_do_not_overlap() {
        let p = config();
        let pins: Vec<TestPin> = (1..=6).map(|id| if id == 1 { selected(id, 401.0, 300.0) } else { pin(id, 400.0 + id as f64, 300.0) }).collect();
        let layout = settle(&pins, &p);
        assert_eq!((layout.item(1).scale, layout.item(1).morph), (1.0, 0.0));
        for item in layout.items.iter().filter(|item| item.id != 1) {
            assert!(item.scale <= DISPLACED_CIRCLE_SCALE + 0.001);
        }
        assert_no_body_overlaps(&layout, &p, 3.0);
    }

    #[test]
    fn a_selected_pin_is_immovable_and_full_sized() {
        let p = config();
        let layout = settle(&[selected(1, 400.0, 300.0), pin(2, 408.0, 300.0)], &p);
        let item = layout.item(1);
        assert_eq!((item.x, item.y, item.scale, item.morph), (400.0, 300.0, 1.0, 0.0));
        assert!(layout.item(2).offset() > 1.0);
        assert_no_body_overlaps(&layout, &p, 3.0);
    }

    #[test]
    fn the_spring_returns_a_pin_when_the_neighbor_disappears() {
        let p = config();
        let mut solver = PinCollisionSolver::new();
        let mut time = 0.0;
        settle_at(&mut solver, &[pin(1, 400.0, 300.0), pin(2, 410.0, 300.0)], &p, &mut time);
        let layout = settle_at(&mut solver, &[pin(2, 410.0, 300.0)], &p, &mut time);
        let item = &layout.items[0];
        assert_eq!((item.x, item.y, item.scale, item.morph), (410.0, 300.0, 1.0, 0.0));
    }

    #[test]
    fn the_displacement_is_clamped_by_max_offset() {
        let p = geometry_and(|p| p.max_offset_px = 30.0);
        let layout = settle(&(1..=5).map(|id| pin(id, 400.0, 300.0)).collect::<Vec<_>>(), &p);
        for item in &layout.items {
            let (x, y) = item.body(&p);
            let displacement = vector_length(x - (item.anchor_x + p.body_center_offset_x), y - (item.anchor_y + p.body_center_offset_y));
            assert!(displacement <= p.max_offset_px + p.body_center_offset_y + 1.0);
        }
    }

    #[test]
    fn coincident_pins_separate_deterministically() {
        let p = config();
        let first = settle(&[pin(1, 400.0, 300.0), pin(2, 400.0, 300.0)], &p);
        let second = settle(&[pin(1, 400.0, 300.0), pin(2, 400.0, 300.0)], &p);
        assert_no_body_overlaps(&first, &p, 1.0);
        assert_eq!((first.item(1).x, first.item(1).y), (400.0, 300.0));
        for (a, b) in first.items.iter().zip(&second.items) {
            assert_eq!((a.id, a.x, a.y), (b.id, b.x, b.y));
            assert!(!a.x.is_nan() && !a.y.is_nan());
        }
    }

    #[test]
    fn the_converged_layout_matches_across_frame_rates() {
        let p = config();
        let converge = |step: f64| {
            let mut solver = PinCollisionSolver::new();
            let mut time = 0.0;
            for _ in 0..1200 {
                let layout = solve(&mut solver, &[pin(1, 400.0, 300.0), pin(2, 412.0, 300.0)], &p, time);
                if !layout.animating {
                    return layout;
                }
                time += step;
            }
            panic!("the solver did not settle");
        };
        let (at60, at30) = (converge(1.0 / 60.0), converge(1.0 / 30.0));
        for (a, b) in at60.items.iter().zip(&at30.items) {
            assert!(vector_length(a.x - b.x, a.y - b.y) < 1.5 && (a.scale - b.scale).abs() <= 0.02);
        }
    }

    #[test]
    fn a_borderline_contact_settles_without_size_oscillation() {
        let p = config();
        let pins = [pin(1, 400.0, 300.0), pin(2, 505.0, 300.0)];
        let mut solver = PinCollisionSolver::new();
        let mut time = 0.0;
        let settled = settle_at(&mut solver, &pins, &p, &mut time);
        for _ in 0..120 {
            time += 1.0 / 60.0;
            let layout = solve(&mut solver, &pins, &p, time);
            assert!(!layout.animating);
            for (item, still) in layout.items.iter().zip(&settled.items) {
                assert!((item.scale - still.scale).abs() <= 0.001 && (item.morph - still.morph).abs() <= 0.001);
                assert!(vector_length(item.x - still.x, item.y - still.y) < 0.1);
            }
        }
        assert_no_body_overlaps(&settled, &p, 3.0);
    }

    #[test]
    fn transient_frames_never_show_body_overlaps() {
        let p = config();
        let mut solver = PinCollisionSolver::new();
        let mut time = 0.0;
        settle_at(&mut solver, &[pin(1, 200.0, 300.0), pin(2, 700.0, 300.0), pin(3, 450.0, 700.0)], &p, &mut time);
        let piled = [pin(1, 400.0, 300.0), pin(2, 408.0, 300.0), pin(3, 400.0, 308.0)];
        for _ in 0..240 {
            time += 1.0 / 60.0;
            assert_no_body_overlaps(&solve(&mut solver, &piled, &p, time), &p, 1.0);
        }
    }

    #[test]
    fn an_anchor_jump_carries_the_offsets_without_a_spike() {
        let p = config();
        let mut solver = PinCollisionSolver::new();
        let mut time = 0.0;
        let settled = settle_at(&mut solver, &[pin(1, 400.0, 300.0), pin(2, 412.0, 300.0)], &p, &mut time);
        time += 1.0 / 60.0;
        let jumped = solve(&mut solver, &[pin(1, 5400.0, 300.0), pin(2, 5412.0, 300.0)], &p, time);
        for item in &jumped.items {
            let before = settled.item(item.id);
            assert!(vector_length(item.x - item.anchor_x - (before.x - before.anchor_x), item.y - item.anchor_y - (before.y - before.anchor_y)) < 1.0);
        }
    }

    #[test]
    fn the_input_order_does_not_change_the_layout() {
        let p = config();
        let pins = [pin(3, 400.0, 308.0), pin(1, 400.0, 300.0), pin(2, 408.0, 300.0), selected(9, 900.0, 300.0)];
        let mut sorted = pins;
        sorted.sort_by_key(|p| p.id);
        let (a, b) = (settle(&pins, &p), settle(&sorted, &p));
        let key = |l: &Layout| l.items.iter().map(|i| (i.id, i.x.to_bits(), i.y.to_bits(), i.scale.to_bits())).collect::<Vec<_>>();
        assert_eq!(key(&a), key(&b));
    }

    #[test]
    fn an_event_policy_pair_forms_a_two_petal_flower() {
        let layout = settle(&[event(1, 400.0, 300.0), event(2, 480.0, 300.0)], &config());
        assert_eq!(layout.flowers.len(), 1);
        assert_eq!(layout.flowers[0].members, [1, 2]);
        assert_eq!(layout.flowers[0].visible, [1, 2]);
        assert_eq!(layout.items.len(), 2);
        for item in &layout.items {
            assert!((item.morph - 1.0).abs() < 0.01 && (item.alpha - 1.0).abs() < 0.01 && item.petal);
        }
    }

    #[test]
    fn a_flower_shows_its_petals_and_hides_the_overflow_members() {
        let p = config();
        let layout = settle(&(1..=12).map(|id| pin(id, 400.0 + id as f64 * 0.5, 300.0)).collect::<Vec<_>>(), &p);
        assert_eq!(layout.flowers.len(), 1);
        let flower = &layout.flowers[0];
        assert_eq!((flower.members.len(), flower.visible.len(), layout.items.len()), (12, MAX_FLOWER_PETALS, MAX_FLOWER_PETALS));
        let ring = flower_ring_radius(p.circle_body_radius_px * p.compressed_scale, MAX_FLOWER_PETALS);
        for item in &layout.items {
            let (x, y) = item.body(&p);
            assert!((item.scale - p.compressed_scale).abs() <= 0.01 && (item.morph - 1.0).abs() < 0.01);
            assert!((vector_length(x - flower.center.0, y - flower.center.1) - ring).abs() <= 2.0);
        }
    }

    #[test]
    #[allow(clippy::approx_constant)] // The Swift test's angles, as written there.
    fn petals_hold_their_slots_under_crowd_pressure() {
        let p = geometry_and(|p| p.grouping_threshold = 2.0);
        let mut pins: Vec<TestPin> = (1..=4).map(|id| pin(id, 400.0 + id as f64 * 0.5, 300.0)).collect();
        for (index, angle) in [0.0f64, 1.57, 3.14, 4.71].iter().enumerate() {
            pins.push(pin(50 + index as u64, 400.0 + angle.cos() * 70.0, 300.0 + angle.sin() * 70.0));
        }
        let layout = settle(&pins, &p);
        let flower = &layout.flowers[0];
        let ring = flower_ring_radius(p.circle_body_radius_px * p.compressed_scale, flower.visible.len());
        for item in layout.items.iter().filter(|item| item.petal) {
            let (x, y) = item.body(&p);
            assert!((vector_length(x - flower.center.0, y - flower.center.1) - ring).abs() <= 1.0, "petal {} was knocked out of its slot", item.id);
        }
    }

    #[test]
    fn ten_piled_pins_form_a_flower_with_the_default_threshold() {
        let p = geometry_and(|p| p.grouping_threshold = 5.0);
        let layout = settle(&(1..=10).map(|id| pin(id, 400.0 + id as f64 * 0.5, 300.0)).collect::<Vec<_>>(), &p);
        assert_eq!(layout.flowers.len(), 1);
        assert_eq!((layout.flowers[0].members.len(), layout.flowers[0].visible.len(), layout.items.len()), (10, MAX_FLOWER_PETALS, MAX_FLOWER_PETALS));
    }

    #[test]
    fn the_default_grouping_boundary_is_five_pins() {
        let p = geometry_and(|p| p.grouping_threshold = 5.0);
        let pile = |count: u64| settle(&(1..=count).map(|id| pin(id, 400.0 + id as f64 * 0.5, 300.0)).collect::<Vec<_>>(), &p);
        let five = pile(5);
        assert_eq!(five.flowers.len(), 1);
        assert_eq!((five.flowers[0].members.len(), five.flowers[0].visible.len(), five.items.len()), (5, 5, 5));
        let four = pile(4);
        assert!(four.flowers.is_empty());
        assert_eq!(four.items.len(), 4);
        assert_no_body_overlaps(&four, &p, 1.5);
    }

    #[test]
    fn grouping_hysteresis_keeps_a_flower_between_the_enter_and_exit_radii() {
        let p = geometry_and(|p| p.grouping_threshold = 1.0);
        let mut solver = PinCollisionSolver::new();
        let mut time = 0.0;
        let mut once = |distance: f64| {
            time += 1.0 / 60.0;
            solve(&mut solver, &[pin(1, 400.0, 300.0), pin(2, 400.0 + distance, 300.0)], &p, time).flowers.len()
        };
        assert_eq!(once(41.0), 0, "41 px is past the enter radius");
        assert_eq!(once(30.0), 1, "30 px is within the enter radius");
        assert_eq!(once(41.0), 1, "41 px is within the exit radius");
        assert_eq!(once(60.0), 0, "60 px is past the exit radius");
    }

    #[test]
    fn a_selected_pin_joins_a_flower_as_a_visible_petal() {
        let layout = settle(&[selected(5, 400.0, 300.0), pin(2, 402.0, 300.0), pin(3, 404.0, 300.0)], &geometry_and(|p| p.grouping_threshold = 1.0));
        assert_eq!(layout.flowers.len(), 1);
        assert_eq!(layout.flowers[0].members, [2, 3, 5]);
        assert_eq!(layout.flowers[0].visible[0], 5);
        assert_eq!(layout.items.len(), 3);
    }

    #[test]
    fn a_flower_stays_put_and_neighbors_react_only_to_a_petal_touch() {
        let pins = [pin(1, 398.0, 300.0), pin(2, 402.0, 300.0), pin(3, 400.0, 302.0), pin(8, 440.0, 300.0), pin(9, 475.0, 380.0)];
        let layout = settle(&pins, &geometry_and(|p| p.grouping_threshold = 2.0));
        assert_eq!(layout.flowers.len(), 1);
        assert_eq!(layout.flowers[0].members, [1, 2, 3]);
        assert!(vector_length(layout.flowers[0].center.0 - 400.0, layout.flowers[0].center.1 - 300.6666) < 0.01);
        let touching = layout.item(8);
        assert!(touching.offset() > 5.0 && touching.morph > 0.5);
        let bystander = layout.item(9);
        assert_eq!((bystander.x, bystander.y), (475.0, 380.0));
        assert!(bystander.morph.abs() < 0.05 && (bystander.scale - 1.0).abs() < 0.01);
    }

    #[test]
    fn overlapping_flowers_merge_into_one() {
        let p = geometry_and(|p| p.grouping_threshold = 2.0);
        let pins: Vec<TestPin> = (1..=3).map(|id| pin(id, 400.0 + id as f64, 300.0)).chain((11..=13).map(|id| pin(id, 460.0 + id as f64, 300.0))).collect();
        let layout = settle(&pins, &p);
        assert_eq!(layout.flowers.len(), 1);
        assert_eq!(layout.flowers[0].members.len(), 6);
        assert_no_body_overlaps(&layout, &p, 1.5);
    }

    #[test]
    fn a_selected_pin_touching_a_flower_joins_it_as_a_petal() {
        let p = geometry_and(|p| p.grouping_threshold = 2.0);
        let layout = settle(&[pin(1, 398.0, 300.0), pin(2, 402.0, 300.0), pin(3, 400.0, 302.0), selected(9, 470.0, 300.0)], &p);
        assert_eq!(layout.flowers.len(), 1);
        assert_eq!(layout.flowers[0].members, [1, 2, 3, 9]);
        assert_eq!(layout.flowers[0].visible[0], 9);
        assert_no_body_overlaps(&layout, &p, 1.5);
    }

    #[test]
    fn a_pin_anchored_inside_the_ring_is_absorbed() {
        let layout = settle(&[pin(1, 398.0, 300.0), pin(2, 402.0, 300.0), pin(3, 400.0, 302.0), pin(9, 400.0, 320.0)], &geometry_and(|p| p.grouping_threshold = 2.0));
        assert_eq!(layout.flowers.len(), 1);
        assert_eq!(layout.flowers[0].members, [1, 2, 3, 9]);
    }

    #[test]
    fn hidden_flower_members_reappear_after_a_breakup() {
        let p = geometry_and(|p| p.grouping_threshold = 3.0);
        let mut solver = PinCollisionSolver::new();
        let mut time = 0.0;
        let piled = settle_at(&mut solver, &(1..=9).map(|id| pin(id, 400.0 + id as f64 * 0.5, 300.0)).collect::<Vec<_>>(), &p, &mut time);
        assert_eq!(piled.items.len(), MAX_FLOWER_PETALS);
        let spread = settle_at(&mut solver, &(1..=9).map(|id| pin(id, id as f64 * 400.0, 300.0)).collect::<Vec<_>>(), &p, &mut time);
        assert!(spread.flowers.is_empty());
        assert_eq!(spread.items.len(), 9);
    }

    #[test]
    fn a_sprawling_chain_splits_into_local_flowers() {
        let p = geometry_and(|p| p.grouping_threshold = 3.0);
        let layout = settle(&(0..17).map(|step| pin(step + 1, 400.0 + step as f64 * 30.0, 300.0)).collect::<Vec<_>>(), &p);
        let limit = p.pin_size_px * FLOWER_COMPACTNESS_LIMIT_SCALE;
        assert!(layout.flowers.len() > 1, "the oversized group was not split");
        let mut covered = 0;
        for flower in &layout.flowers {
            covered += flower.members.len();
            for &id in &flower.members {
                assert!(vector_length(400.0 + (id - 1) as f64 * 30.0 - flower.center.0, 300.0 - flower.center.1) < limit, "member {id} is far from its ring");
            }
        }
        assert_eq!(covered, 17);
        assert_no_body_overlaps(&layout, &p, 1.5);
    }

    #[test]
    fn the_world_grid_split_is_stable_under_a_pan() {
        let p = geometry_and(|p| p.grouping_threshold = 3.0);
        let compositions = |pan_x: f64, pan_y: f64| {
            let pins: Vec<TestPin> = (0..17)
                .map(|step| {
                    let x = 400.0 + step as f64 * 30.0;
                    TestPin { world: Some((x * 0.001, 0.3)), ..pin(step + 1, x + pan_x, 300.0 + pan_y) }
                })
                .collect();
            settle(&pins, &p).flowers.into_iter().map(|flower| flower.members).collect::<Vec<_>>()
        };
        assert_eq!(compositions(37.0, 53.0), compositions(0.0, 0.0), "panning rebuilt the cluster memberships");
    }

    #[test]
    fn unmergeable_intersecting_rings_separate_without_overlap() {
        let p = geometry_and(|p| p.grouping_threshold = 2.0);
        let pins: Vec<TestPin> = (1..=5).map(|id| pin(id, 400.0 + id as f64 * 0.5, 300.0)).chain((0..9).map(|step| pin(20 + step, 450.0, 315.0 + step as f64 * 30.0))).collect();
        let layout = settle(&pins, &p);
        assert_eq!(layout.flowers.len(), 2, "the clusters merged although the compactness limit forbids it");
        let small = layout.flowers.iter().find(|f| f.members.len() == 5).expect("the small flower");
        let large = layout.flowers.iter().find(|f| f.members.len() == 9).expect("the large flower");
        assert!(vector_length(large.center.0 - 450.0, large.center.1 - 435.0) < 1.0);
        assert!(vector_length(small.center.0 - 401.5, small.center.1 - 300.0) > 1.0);
        assert_no_body_overlaps(&layout, &p, 1.5);
    }

    #[test]
    fn a_dense_mosaic_of_flowers_has_no_petal_overlaps() {
        let p = geometry_and(|p| p.grouping_threshold = 5.0);
        let mut state: u64 = 7;
        let mut random = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (state >> 40) as f64 / (u64::MAX >> 40) as f64
        };
        let pins: Vec<TestPin> = (1..=800)
            .map(|id| {
                let angle = random() * 2.0 * std::f64::consts::PI;
                let radius = 600.0 * random().sqrt();
                pin(id, 900.0 + angle.cos() * radius, 450.0 + angle.sin() * radius * 0.5)
            })
            .collect();
        let layout = settle(&pins, &p);
        assert!(layout.flowers.len() > 3, "a dense field must produce a mosaic of clusters");
        assert_no_body_overlaps(&layout, &p, 1.5);
    }

    #[test]
    fn an_empty_frame_resets_the_state() {
        let p = config();
        let mut solver = PinCollisionSolver::new();
        solve(&mut solver, &[pin(1, 400.0, 300.0), pin(2, 410.0, 300.0)], &p, 0.0);
        let empty = solve(&mut solver, &[], &p, 1.0 / 60.0);
        assert!(empty.items.is_empty() && empty.flowers.is_empty() && !empty.animating);
        assert!(solver.pin_state_ids.is_empty() && solver.cluster_states.is_empty());
    }
}
