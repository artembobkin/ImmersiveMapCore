// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use std::collections::HashMap;

use crate::{FlatMesh, FlatVertex, FlattenInput, FlattenOptions};

/// Brute-force reference: what color should be visible at a point, straight from the input rings.
pub(super) struct Reference<'a> {
    pub(super) input: &'a FlattenInput,
}

impl Reference<'_> {
    /// Calls `edge` with the two ends of every edge of every ring, and the ring's paint.
    fn for_each_edge(&self, mut edge: impl FnMut(f64, f64, f64, f64, usize)) {
        let coords = &self.input.coords;
        for ring in &self.input.rings {
            let start = ring.start as usize * 2;
            let count = ring.count as usize;
            let mut ax = coords[start + 2 * (count - 1)];
            let mut ay = coords[start + 2 * (count - 1) + 1];
            for k in 0..count {
                let bx = coords[start + 2 * k];
                let by = coords[start + 2 * k + 1];
                edge(ax, ay, bx, by, usize::from(ring.paint));
                ax = bx;
                ay = by;
            }
        }
    }

    /// Winding number of every paint at the point.
    pub(super) fn windings(&self, x: f64, y: f64) -> Vec<i32> {
        let mut result = vec![0; self.input.paints.len()];
        self.for_each_edge(|ax, ay, bx, by, paint| {
            if (ay <= y) != (by <= y) {
                let x_cross = ax + (y - ay) * (bx - ax) / (by - ay);
                if x_cross < x {
                    result[paint] += if by > ay { 1 } else { -1 };
                }
            }
        });
        result
    }

    /// The topmost covering paint. Only valid when all paints are opaque.
    pub(super) fn color(&self, x: f64, y: f64) -> u32 {
        let winding = self.windings(x, y);
        (0..self.input.paints.len())
            .rev()
            .find(|&paint| winding[paint] != 0)
            .map_or(0, |paint| self.input.paints[paint].packed())
    }

    /// Distance from the point to the nearest input edge.
    pub(super) fn distance_to_edges(&self, x: f64, y: f64) -> f64 {
        let mut best = f64::INFINITY;
        self.for_each_edge(|ax, ay, bx, by, _| {
            let (dx, dy) = (bx - ax, by - ay);
            let length_squared = dx * dx + dy * dy;
            let t = if length_squared > 0.0 {
                ((x - ax) * dx + (y - ay) * dy) / length_squared
            } else {
                0.0
            };
            let t = t.clamp(0.0, 1.0);
            let (px, py) = (ax + t * dx - x, ay + t * dy - y);
            best = best.min((px * px + py * py).sqrt());
        });
        best
    }
}

fn triangle(mesh: &FlatMesh, t: usize) -> [FlatVertex; 3] {
    [0, 1, 2].map(|k| mesh.vertices[mesh.indices[3 * t + k] as usize])
}

/// Finds the triangle under a point with a uniform grid over the mesh.
pub(super) struct MeshLocator<'a> {
    mesh: &'a FlatMesh,
    min_x: f64,
    min_y: f64,
    cell: f64,
    size: usize,
    buckets: Vec<Vec<u32>>,
}

impl<'a> MeshLocator<'a> {
    pub(super) fn new(mesh: &'a FlatMesh, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        let size = 64;
        let mut locator = MeshLocator {
            mesh,
            min_x,
            min_y,
            cell: (max_x - min_x).max(max_y - min_y) / size as f64,
            size,
            buckets: vec![Vec::new(); size * size],
        };
        for t in 0..mesh.triangle_count() {
            let [a, b, c] = triangle(mesh, t);
            let x0 = locator.index(f64::from(a.x.min(b.x).min(c.x)) - min_x);
            let x1 = locator.index(f64::from(a.x.max(b.x).max(c.x)) - min_x);
            let y0 = locator.index(f64::from(a.y.min(b.y).min(c.y)) - min_y);
            let y1 = locator.index(f64::from(a.y.max(b.y).max(c.y)) - min_y);
            for gy in y0..=y1 {
                for gx in x0..=x1 {
                    locator.buckets[gy * size + gx].push(t as u32);
                }
            }
        }
        locator
    }

    fn index(&self, v: f64) -> usize {
        ((v / self.cell) as i64).clamp(0, self.size as i64 - 1) as usize
    }

    /// Colors of all triangles that contain the point.
    pub(super) fn colors(&self, x: f64, y: f64) -> Vec<u32> {
        let side = |p: FlatVertex, q: FlatVertex| {
            (f64::from(q.x) - f64::from(p.x)) * (y - f64::from(p.y))
                - (f64::from(q.y) - f64::from(p.y)) * (x - f64::from(p.x))
        };
        let bucket =
            &self.buckets[self.index(y - self.min_y) * self.size + self.index(x - self.min_x)];
        bucket
            .iter()
            .filter_map(|&t| {
                let [a, b, c] = triangle(self.mesh, t as usize);
                (side(a, b) >= 0.0 && side(b, c) >= 0.0 && side(c, a) >= 0.0).then_some(a.color)
            })
            .collect()
    }
}

#[derive(Debug, Default)]
pub(super) struct MeshReport {
    pub(super) area: f64,
    /// Edges inside the bounds that are not matched by the same edge of a neighbour triangle.
    pub(super) open_edges: i64,
    pub(super) mixed_color_triangles: usize,
    /// Triangles that reach over a grid line of `columns` x `rows`.
    pub(super) grid_crossing_triangles: usize,
    /// Triangles that are clearly clockwise. Slivers along a straight edge do not count.
    pub(super) flipped_triangles: usize,
}

pub(super) fn inspect(mesh: &FlatMesh, options: &FlattenOptions) -> MeshReport {
    let mut report = MeshReport::default();
    let extent = (options.max_x - options.min_x).max(options.max_y - options.min_y);
    let mut edges: HashMap<[u32; 4], i64> = HashMap::with_capacity(mesh.indices.len());
    let cell_width = (options.max_x - options.min_x) / options.columns.max(1) as f64;
    let cell_height = (options.max_y - options.min_y) / options.rows.max(1) as f64;
    for t in 0..mesh.triangle_count() {
        let v = triangle(mesh, t);
        if v[0].color != v[1].color || v[0].color != v[2].color {
            report.mixed_color_triangles += 1;
        }
        let d = |a: f32, b: f32| f64::from(a) - f64::from(b);
        let area =
            0.5 * (d(v[1].x, v[0].x) * d(v[2].y, v[0].y) - d(v[1].y, v[0].y) * d(v[2].x, v[0].x));
        if area < -1e-7 * extent * extent {
            report.flipped_triangles += 1;
        }
        report.area += area;
        let xs = v.map(|p| (f64::from(p.x) - options.min_x) / cell_width);
        let ys = v.map(|p| (f64::from(p.y) - options.min_y) / cell_height);
        let crosses = |c: [f64; 3]| {
            let max = c.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let min = c.iter().copied().fold(f64::INFINITY, f64::min);
            (max - 1e-4).floor() > (min + 1e-4).floor()
        };
        if crosses(xs) || crosses(ys) {
            report.grid_crossing_triangles += 1;
        }
        for k in 0..3 {
            let (p, q) = (v[k], v[(k + 1) % 3]);
            // Adding 0 turns -0 into +0, so equal positions have equal bits.
            let forward = [
                (p.x + 0.0).to_bits(),
                (p.y + 0.0).to_bits(),
                (q.x + 0.0).to_bits(),
                (q.y + 0.0).to_bits(),
            ];
            let backward = [forward[2], forward[3], forward[0], forward[1]];
            // Two distinct points can round to the same f32 position. Such an edge has no length.
            if forward == backward {
                continue;
            }
            match edges.get_mut(&backward) {
                Some(count) if *count > 0 => *count -= 1,
                _ => *edges.entry(forward).or_insert(0) += 1,
            }
        }
    }
    let (min_x, max_x) = (options.min_x as f32, options.max_x as f32);
    let (min_y, max_y) = (options.min_y as f32, options.max_y as f32);
    for (edge, &count) in &edges {
        if count <= 0 {
            continue;
        }
        let [ax, ay, bx, by] = edge.map(f32::from_bits);
        let on_border = (ax == bx && (ax == min_x || ax == max_x))
            || (ay == by && (ay == min_y || ay == max_y));
        if !on_border {
            report.open_edges += count;
        }
    }
    report
}

/// A small deterministic generator, so that failures can be reproduced from the seed.
///
/// The sampling methods follow the algorithms of the Swift standard library, so a seed draws the
/// same values as in the Swift tests.
pub(super) struct SeededGenerator {
    state: u64,
}

impl SeededGenerator {
    pub(super) fn new(seed: u64) -> Self {
        SeededGenerator {
            state: seed
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(0x123_4567),
        }
    }

    pub(super) fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `0..upper_bound`, without bias (Lemire's method).
    fn next_below(&mut self, upper_bound: u64) -> u64 {
        let mut m = u128::from(self.next_u64()) * u128::from(upper_bound);
        if (m as u64) < upper_bound {
            let t = upper_bound.wrapping_neg() % upper_bound;
            while (m as u64) < t {
                m = u128::from(self.next_u64()) * u128::from(upper_bound);
            }
        }
        (m >> 64) as u64
    }

    /// `Int.random(in: lower...upper)`.
    pub(super) fn int_in(&mut self, lower: i64, upper: i64) -> i64 {
        let delta = upper.wrapping_sub(lower) as u64;
        if delta == u64::MAX {
            return self.next_u64() as i64;
        }
        lower.wrapping_add(self.next_below(delta + 1) as i64)
    }

    /// `Int.random(in: lower..<upper)`.
    pub(super) fn int_below(&mut self, lower: i64, upper: i64) -> i64 {
        lower.wrapping_add(self.next_below(upper.wrapping_sub(lower) as u64) as i64)
    }

    /// `Double.random(in: lower...upper)`.
    pub(super) fn double_in(&mut self, lower: f64, upper: f64) -> f64 {
        let max_significand: u64 = 1 << 53;
        let rand = self.next_below(max_significand + 1);
        if rand == max_significand {
            return upper;
        }
        (upper - lower) * (rand as f64 * (f64::EPSILON / 2.0)) + lower
    }

    /// `Bool.random()`.
    pub(super) fn bool(&mut self) -> bool {
        (self.next_u64() >> 17) & 1 == 0
    }

    /// `randomElement()` of a non-empty slice.
    pub(super) fn element<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.int_below(0, items.len() as i64) as usize]
    }
}
