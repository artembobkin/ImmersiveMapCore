// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The frame's label collision solve, a port of the Swift
//! `LabelCollisionSolver`: base labels and road
//! instances in one strict rank order, each accepted when none of its boxes
//! overlaps a box already placed. The grid is a bucket per cell with
//! intrusive lists over vectors kept between frames.
//!
//! Everything is flat arrays, so a host calls across a binding without a
//! copy per label: the base ranks once per topology change, the boxes and
//! the road items every solve, the decisions back.

/// Ints per road item: priority, secondary priority, sort priority, tile
/// order, key high and low halves (u32 bits), box start, box end, target.
pub const ROAD_ITEM_STRIDE: usize = 9;

#[derive(Default)]
pub struct LabelCollisionSolver {
    base_priority: Vec<i32>,
    base_order: Vec<i32>,
    road_order: Vec<usize>,
    grid_width: usize,
    grid_height: usize,
    cell_size: f32,
    cell_heads: Vec<i32>,
    node_next: Vec<i32>,
    node_box: Vec<i32>,
    /// min x, min y, max x, max y per placed box.
    placed: Vec<[f32; 4]>,
    placed_group: Vec<u32>,
    pending: Vec<[f32; 4]>,
    pending_cells: Vec<[usize; 4]>,
}

/// The frame's inputs, borrowed from the caller.
pub struct CollisionFrame<'a> {
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub cell_size_px: f32,
    pub base_centers: &'a [f32],
    pub base_half_sizes: &'a [f32],
    pub base_enabled: &'a [u8],
    pub road_items: &'a [i32],
    pub road_centers: &'a [f32],
    pub road_half_sizes: &'a [f32],
}

fn road_precedes(items: &[i32], a: usize, b: usize) -> std::cmp::Ordering {
    let x = &items[a * ROAD_ITEM_STRIDE..a * ROAD_ITEM_STRIDE + 6];
    let y = &items[b * ROAD_ITEM_STRIDE..b * ROAD_ITEM_STRIDE + 6];
    x[0].cmp(&y[0])
        .then(x[1].cmp(&y[1]))
        .then(x[2].cmp(&y[2]))
        .then(x[3].cmp(&y[3]))
        .then((x[4] as u32).cmp(&(y[4] as u32)))
        .then((x[5] as u32).cmp(&(y[5] as u32)))
}

impl LabelCollisionSolver {
    /// Solves one frame into the two decision arrays.
    pub fn solve_frame(&mut self, frame: &CollisionFrame<'_>, base_visible: &mut [u8], road_visible: &mut [u8]) {
        self.reset_grid(frame.viewport_width, frame.viewport_height, frame.cell_size_px);
        let items = frame.road_items;
        let road_total = items.len() / ROAD_ITEM_STRIDE;
        self.road_order.clear();
        self.road_order.extend(0..road_total);
        let mut road_order = std::mem::take(&mut self.road_order);
        road_order.sort_by(|&a, &b| road_precedes(items, a, b));
        let base_order = std::mem::take(&mut self.base_order);
        let base_limit = self
            .base_priority
            .len()
            .min(frame.base_centers.len() / 2)
            .min(frame.base_half_sizes.len() / 2)
            .min(frame.base_enabled.len())
            .min(base_visible.len());
        let road_boxes = frame.road_centers.len().min(frame.road_half_sizes.len()) / 2;
        let (mut base_cursor, mut road_cursor) = (0, 0);
        while base_cursor < base_order.len() || road_cursor < road_total {
            let take_base = if base_cursor < base_order.len() && road_cursor < road_total {
                self.base_priority[base_order[base_cursor] as usize] <= items[road_order[road_cursor] * ROAD_ITEM_STRIDE]
            } else {
                base_cursor < base_order.len()
            };
            if take_base {
                let index = base_order[base_cursor] as usize;
                base_cursor += 1;
                if index >= base_limit {
                    continue;
                }
                if frame.base_enabled[index] == 0 {
                    base_visible[index] = 0;
                    continue;
                }
                self.pending.clear();
                self.pending_cells.clear();
                let c = &frame.base_centers[index * 2..index * 2 + 2];
                let h = &frame.base_half_sizes[index * 2..index * 2 + 2];
                let accepted = self.offer(c[0], c[1], h[0], h[1], 0) && !self.pending.is_empty();
                if accepted {
                    self.commit_pending(0);
                }
                base_visible[index] = accepted as u8;
            } else {
                let item = road_order[road_cursor];
                road_cursor += 1;
                let record = &items[item * ROAD_ITEM_STRIDE..(item + 1) * ROAD_ITEM_STRIDE];
                let group = item as u32 + 1;
                self.pending.clear();
                self.pending_cells.clear();
                let (start, end) = (record[6].max(0) as usize, record[7].max(0) as usize);
                let mut accepted = end > start;
                let mut b = start;
                while accepted && b < end {
                    if b >= road_boxes {
                        accepted = false;
                        break;
                    }
                    let c = &frame.road_centers[b * 2..b * 2 + 2];
                    let h = &frame.road_half_sizes[b * 2..b * 2 + 2];
                    if !self.offer(c[0], c[1], h[0], h[1], group) {
                        accepted = false;
                    }
                    b += 1;
                }
                accepted = accepted && !self.pending.is_empty();
                if accepted {
                    self.commit_pending(group);
                }
                let target = record[8];
                if target >= 0 && (target as usize) < road_visible.len() {
                    road_visible[target as usize] = accepted as u8;
                }
            }
        }
        self.base_order = base_order;
        self.road_order = road_order;
    }

    fn reset_grid(&mut self, width: f32, height: f32, cell_size: f32) {
        self.cell_size = cell_size.max(1.0);
        self.grid_width = ((width.max(1.0) / self.cell_size).ceil() as usize).max(1);
        self.grid_height = ((height.max(1.0) / self.cell_size).ceil() as usize).max(1);
        self.cell_heads.clear();
        self.cell_heads.resize(self.grid_width * self.grid_height, -1);
        self.node_next.clear();
        self.node_box.clear();
        self.placed.clear();
        self.placed_group.clear();
    }

    /// Tests one box against the placed ones and queues it: false when it
    /// overlaps a placed box of another group, or of any group for group 0.
    /// A box entirely off screen is neither queued nor a collision.
    fn offer(&mut self, cx: f32, cy: f32, hx: f32, hy: f32, group: u32) -> bool {
        let (min_x, min_y, max_x, max_y) = (cx - hx, cy - hy, cx + hx, cy + hy);
        let cell = self.cell_size;
        let width = self.grid_width as f32 * cell;
        let height = self.grid_height as f32 * cell;
        if max_x < 0.0 || max_y < 0.0 || min_x > width || min_y > height {
            return true;
        }
        let clamp_cell = |v: f32, last: usize| ((v / cell) as usize).min(last);
        let min_cx = clamp_cell(min_x.max(0.0), self.grid_width - 1);
        let max_cx = clamp_cell(max_x.min(width), self.grid_width - 1);
        let min_cy = clamp_cell(min_y.max(0.0), self.grid_height - 1);
        let max_cy = clamp_cell(max_y.min(height), self.grid_height - 1);
        for cell_y in min_cy..=max_cy {
            let row = cell_y * self.grid_width;
            for cell_x in min_cx..=max_cx {
                let mut node = self.cell_heads[row + cell_x];
                while node >= 0 {
                    let placed_box = self.node_box[node as usize] as usize;
                    let other = self.placed[placed_box];
                    if min_x < other[2] && max_x > other[0] && min_y < other[3] && max_y > other[1] && (group == 0 || self.placed_group[placed_box] != group) {
                        return false;
                    }
                    node = self.node_next[node as usize];
                }
            }
        }
        self.pending.push([min_x, min_y, max_x, max_y]);
        self.pending_cells.push([min_cx, max_cx, min_cy, max_cy]);
        true
    }

    fn commit_pending(&mut self, group: u32) {
        for (index, bounds) in self.pending.iter().enumerate() {
            let placed_box = self.placed.len() as i32;
            self.placed.push(*bounds);
            self.placed_group.push(group);
            let [min_cx, max_cx, min_cy, max_cy] = self.pending_cells[index];
            for cell_y in min_cy..=max_cy {
                let row = cell_y * self.grid_width;
                for cell_x in min_cx..=max_cx {
                    let cell = row + cell_x;
                    self.node_next.push(self.cell_heads[cell]);
                    self.node_box.push(placed_box);
                    self.cell_heads[cell] = self.node_next.len() as i32 - 1;
                }
            }
        }
    }
}

impl LabelCollisionSolver {
    pub fn new() -> LabelCollisionSolver {
        LabelCollisionSolver::default()
    }

    /// The base labels' collision priorities in the working set's order, and their indices in rank order.
    pub fn rebind_base(&mut self, priority: &[i32], order: &[i32]) {
        self.base_priority = priority.to_vec();
        self.base_order = order.to_vec();
    }

    /// `solve_frame` with the frame's inputs as arguments: the decisions are written into the two arrays.
    #[allow(clippy::too_many_arguments)]
    pub fn solve(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        cell_size_px: f32,
        base_centers: &[f32],
        base_half_sizes: &[f32],
        base_enabled: &[u8],
        road_items: &[i32],
        road_centers: &[f32],
        road_half_sizes: &[f32],
        base_visible: &mut [u8],
        road_visible: &mut [u8],
    ) {
        let frame = CollisionFrame { viewport_width, viewport_height, cell_size_px, base_centers, base_half_sizes, base_enabled, road_items, road_centers, road_half_sizes };
        self.solve_frame(&frame, base_visible, road_visible);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame<'a>(centers: &'a [f32], halves: &'a [f32], enabled: &'a [u8], road_items: &'a [i32], road_centers: &'a [f32], road_halves: &'a [f32]) -> CollisionFrame<'a> {
        CollisionFrame { viewport_width: 1000.0, viewport_height: 1000.0, cell_size_px: 32.0, base_centers: centers, base_half_sizes: halves, base_enabled: enabled, road_items, road_centers, road_half_sizes: road_halves }
    }

    #[test]
    fn the_earlier_rank_keeps_the_space() {
        let mut solver = LabelCollisionSolver::new();
        solver.rebind_base(&[5, 1], &[1, 0]);
        let mut visible = [0u8; 2];
        solver.solve_frame(&frame(&[100.0, 100.0, 105.0, 100.0], &[10.0, 10.0, 10.0, 10.0], &[1, 1], &[], &[], &[]), &mut visible, &mut []);
        assert_eq!(visible, [0, 1]);
    }

    #[test]
    fn a_road_instance_is_kept_or_dropped_whole_and_its_glyphs_never_block_each_other() {
        let mut solver = LabelCollisionSolver::new();
        solver.rebind_base(&[0], &[0]);
        let mut base = [0u8; 1];
        let mut road = [9u8; 2];
        // Two overlapping glyphs of one instance, then an instance over the base label.
        let items = [1_000_000_000, 0, 0, 0, 0, 1, 0, 2, 0, 1_000_000_000, 0, 1, 0, 0, 2, 2, 3, 1];
        let centers = [500.0, 500.0, 505.0, 500.0, 100.0, 100.0];
        let halves = [8.0; 6];
        solver.solve_frame(&frame(&[100.0, 100.0], &[10.0, 10.0], &[1], &items, &centers, &halves), &mut base, &mut road);
        assert_eq!(base, [1]);
        assert_eq!(road, [1, 0]);
    }

    #[test]
    fn a_box_off_screen_is_not_kept() {
        let mut solver = LabelCollisionSolver::new();
        solver.rebind_base(&[0], &[0]);
        let mut visible = [1u8; 1];
        solver.solve_frame(&frame(&[-100.0, -100.0], &[10.0, 10.0], &[1], &[], &[], &[]), &mut visible, &mut []);
        assert_eq!(visible, [0]);
    }
}
