// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use crate::sweep::PolyHeader;
use crate::{FastHashMap, FlatVertex, VertexKey};

#[derive(Clone, Copy)]
struct ChainVertex {
    x: f64,
    y: f64,
    index: u32,
    on_left: bool,
}

/// Twice the area below which three points count as collinear.
const COLLINEAR_TOLERANCE: f64 = 1e-6;
const COINCIDENT_DISTANCE_SQUARED: f64 = 1e-12;

/// The positions of the bounds of one strip, and the heights of all vertices on its walls.
pub(crate) struct StripWalls<'a> {
    pub(crate) wall_l: f64,
    pub(crate) wall_r: f64,
    /// Heights of all vertices on the left wall of the strip, from both sides, ascending.
    pub(crate) left_marks: &'a [f64],
    /// The same for the right wall.
    pub(crate) right_marks: &'a [f64],
}

/// Triangulates the monotone polygons produced by `Sweep`.
///
/// The polygons of one color share their vertices: a position is emitted once per color, and a
/// polygon that touches it refers to that vertex.
#[derive(Default)]
pub(crate) struct MonotoneTriangulator {
    pub(crate) vertices: Vec<FlatVertex>,
    pub(crate) indices: Vec<u32>,
    /// A position with its color, the identity of a vertex: the regions of one color share the
    /// vertices on their common edges.
    vertex_index: FastHashMap<VertexKey, u32>,

    left: Vec<ChainVertex>,
    right: Vec<ChainVertex>,
    order: Vec<ChainVertex>,
    stack: Vec<ChainVertex>,
    /// Vertices created by the polygon being triangulated.
    created: usize,
}

impl MonotoneTriangulator {
    #[inline]
    fn emit(&mut self, x: f64, y: f64, color: u32) -> u32 {
        // Adding 0 turns -0 into +0, so equal positions have equal bits.
        let fx = x as f32 + 0.0;
        let fy = y as f32 + 0.0;
        let next = self.vertices.len() as u32;
        let index = *self
            .vertex_index
            .entry(VertexKey::new(fx, fy, color))
            .or_insert(next);
        if index == next {
            self.vertices.push(FlatVertex {
                x: fx,
                y: fy,
                color,
            });
            self.created += 1;
        }
        index
    }

    /// A stretch of a side along a strip wall gets the wall's vertices, strictly between its ends.
    fn append_wall_marks(
        &mut self,
        marks: &[f64],
        wall_x: f64,
        above: f64,
        below: f64,
        on_left: bool,
        color: u32,
    ) {
        let mut k = upper_bound(marks, above);
        while k < marks.len() && marks[k] < below {
            let vertex = ChainVertex {
                x: wall_x,
                y: marks[k],
                index: self.emit(wall_x, marks[k], color),
                on_left,
            };
            if on_left {
                self.left.push(vertex);
            } else {
                self.right.push(vertex);
            }
            k += 1;
        }
    }

    pub(crate) fn triangulate(&mut self, poly: &PolyHeader, data: &[f64], walls: &StripWalls<'_>) {
        let offset = poly.offset as usize;
        let top_count = poly.top_count as usize;
        let bottom_count = poly.bottom_count as usize;
        let bottom_start = offset + top_count;
        let left_start = bottom_start + bottom_count;
        let right_start = left_start + poly.left_count as usize * 3;
        let color = poly.color;

        // Both chains run from the top-left point to the bottom-right point. The top side belongs
        // to the right chain and the bottom side to the left chain, which keeps both chains
        // ordered by (y, x).
        self.left.clear();
        self.right.clear();
        self.created = 0;

        let first = ChainVertex {
            x: data[offset],
            y: poly.y_top,
            index: self.emit(data[offset], poly.y_top, color),
            on_left: true,
        };
        self.left.push(first);
        self.right.push(first);
        for k in 1..top_count {
            let x = data[offset + k];
            let index = self.emit(x, poly.y_top, color);
            self.right.push(ChainVertex {
                x,
                y: poly.y_top,
                index,
                on_left: false,
            });
        }

        let right_count = poly.right_count as usize;
        let mut right_on_wall = poly.flags & PolyHeader::RIGHT_ON_WALL != 0;
        let mut above = poly.y_top;
        for k in 0..right_count {
            let x = data[right_start + 3 * k];
            let y = data[right_start + 3 * k + 1];
            if right_on_wall {
                self.append_wall_marks(walls.right_marks, walls.wall_r, above, y, false, color);
            }
            let index = self.emit(x, y, color);
            self.right.push(ChainVertex {
                x,
                y,
                index,
                on_left: false,
            });
            right_on_wall = data[right_start + 3 * k + 2] != 0.0;
            above = y;
        }
        if right_on_wall {
            self.append_wall_marks(
                walls.right_marks,
                walls.wall_r,
                above,
                poly.y_bot,
                false,
                color,
            );
        }

        let left_count = poly.left_count as usize;
        let mut left_on_wall = poly.flags & PolyHeader::LEFT_ON_WALL != 0;
        above = poly.y_top;
        for k in 0..left_count {
            let x = data[left_start + 3 * k];
            let y = data[left_start + 3 * k + 1];
            if left_on_wall {
                self.append_wall_marks(walls.left_marks, walls.wall_l, above, y, true, color);
            }
            let index = self.emit(x, y, color);
            self.left.push(ChainVertex {
                x,
                y,
                index,
                on_left: true,
            });
            left_on_wall = data[left_start + 3 * k + 2] != 0.0;
            above = y;
        }
        if left_on_wall {
            self.append_wall_marks(
                walls.left_marks,
                walls.wall_l,
                above,
                poly.y_bot,
                true,
                color,
            );
        }

        for k in 0..bottom_count {
            let x = data[bottom_start + k];
            let index = self.emit(x, poly.y_bot, color);
            self.left.push(ChainVertex {
                x,
                y: poly.y_bot,
                index,
                on_left: true,
            });
        }
        let last = self.left[self.left.len() - 1];
        self.right.push(last);

        let count = self.left.len() + self.right.len() - 2;
        if count < 3 {
            for _ in 0..self.created {
                let vertex = self.vertices.pop().expect("created vertex");
                self.vertex_index
                    .remove(&VertexKey::new(vertex.x, vertex.y, vertex.color));
            }
            return;
        }

        // A trapezoid without extra points is by far the most common shape.
        if count == 4 && top_count == 2 && bottom_count == 2 {
            // Left: top-left, bottom-left, bottom-right. Right: top-left, top-right, bottom-right.
            let (l0, l1, l2, r1) = (self.left[0], self.left[1], self.left[2], self.right[1]);
            self.add_triangle(l0, r1, l2, false);
            self.add_triangle(l0, l2, l1, false);
            return;
        }

        // Merge both chains into one sequence ordered by (y, x).
        let MonotoneTriangulator {
            left,
            right,
            order,
            stack,
            indices,
            ..
        } = self;
        order.clear();
        order.push(first);
        let mut l = 1;
        let mut r = 1;
        let left_end = left.len() - 1;
        let right_end = right.len() - 1;
        while l < left_end || r < right_end {
            let take_left = if r >= right_end {
                true
            } else if l >= left_end {
                false
            } else {
                let (a, b) = (&left[l], &right[r]);
                a.y < b.y || (a.y == b.y && a.x <= b.x)
            };
            if take_left {
                order.push(left[l]);
                l += 1;
            } else {
                order.push(right[r]);
                r += 1;
            }
        }
        order.push(last);

        stack.clear();
        stack.push(order[0]);
        stack.push(order[1]);
        for &u in &order[2..count - 1] {
            let top = stack[stack.len() - 1];
            if u.on_left != top.on_left {
                let stack_on_left = top.on_left;
                for s in 0..stack.len() - 1 {
                    push_triangle(indices, u, stack[s], stack[s + 1], stack_on_left);
                }
                stack.clear();
                stack.push(top);
                stack.push(u);
            } else {
                let mut last_popped = stack.pop().expect("stack has two points");
                while let Some(&s) = stack.last() {
                    // The diagonal from u to s is inside when the corner at last_popped is convex.
                    // A straight corner is left on the stack: the opposite chain connects to it
                    // later, which avoids a triangle without area.
                    let ex = last_popped.x - s.x;
                    let ey = last_popped.y - s.y;
                    let cross = ex * (u.y - last_popped.y) - ey * (u.x - last_popped.x);
                    // Two chain points that nearly coincide give a corner without a direction.
                    // Such a pair is removed with an empty triangle, so that it cannot hide a
                    // convex corner further up the chain.
                    let coincident = ex * ex + ey * ey < COINCIDENT_DISTANCE_SQUARED;
                    let convex = if u.on_left {
                        cross < -COLLINEAR_TOLERANCE
                    } else {
                        cross > COLLINEAR_TOLERANCE
                    };
                    if coincident || convex {
                        push_triangle(indices, u, last_popped, s, !u.on_left);
                        last_popped = s;
                        stack.pop();
                    } else {
                        break;
                    }
                }
                stack.push(last_popped);
                stack.push(u);
            }
        }
        let u = order[count - 1];
        let stack_on_left = stack[stack.len() - 1].on_left;
        for s in 0..stack.len() - 1 {
            push_triangle(indices, u, stack[s], stack[s + 1], stack_on_left);
        }
    }

    #[inline(always)]
    fn add_triangle(&mut self, a: ChainVertex, b: ChainVertex, c: ChainVertex, reversed: bool) {
        push_triangle(&mut self.indices, a, b, c, reversed);
    }
}

/// The order comes from the structure of the polygon, not from the sign of the area: a
/// triangle without area (three points on one straight edge) still has to face the same way
/// as its neighbours, and it must not be dropped, or a T-junction would take its place.
#[inline(always)]
fn push_triangle(
    indices: &mut Vec<u32>,
    a: ChainVertex,
    b: ChainVertex,
    c: ChainVertex,
    reversed: bool,
) {
    if reversed {
        indices.extend_from_slice(&[a.index, c.index, b.index]);
    } else {
        indices.extend_from_slice(&[a.index, b.index, c.index]);
    }
}

/// Index of the first element greater than `value` in an ascending slice.
#[inline]
fn upper_bound(array: &[f64], value: f64) -> usize {
    array.partition_point(|&v| v <= value)
}
