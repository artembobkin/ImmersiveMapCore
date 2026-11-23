// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use std::f64::consts::PI;
use std::ops::{Add, Div, Mul, Sub};

use crate::FlattenInput;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LineJoin {
    Bevel,
    Miter,
    #[default]
    Round,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LineCap {
    #[default]
    Butt,
    Square,
    Round,
}

/// How `FlattenInput::add_line` strokes a polyline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineStyle {
    /// The full width of the stroke, in the units of the points.
    pub width: f64,
    pub join: LineJoin,
    pub cap: LineCap,
    /// The largest allowed distance between a round join or cap and the true circle, in the
    /// units of the points.
    pub tolerance: f64,
}

impl LineStyle {
    /// A stroke of the given width with round joins, butt caps and a tolerance of 0.25.
    pub const fn new(width: f64) -> Self {
        LineStyle {
            width,
            join: LineJoin::Round,
            cap: LineCap::Butt,
            tolerance: 0.25,
        }
    }

    pub const fn join(mut self, join: LineJoin) -> Self {
        self.join = join;
        self
    }

    pub const fn cap(mut self, cap: LineCap) -> Self {
        self.cap = cap;
        self
    }

    pub const fn tolerance(mut self, tolerance: f64) -> Self {
        self.tolerance = tolerance;
        self
    }
}

#[derive(Clone, Copy, PartialEq)]
struct V2 {
    x: f64,
    y: f64,
}

impl V2 {
    #[inline(always)]
    fn new(x: f64, y: f64) -> Self {
        V2 { x, y }
    }

    #[inline(always)]
    fn from(p: [f64; 2]) -> Self {
        V2 { x: p[0], y: p[1] }
    }
}

impl Add for V2 {
    type Output = V2;
    #[inline(always)]
    fn add(self, o: V2) -> V2 {
        V2::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for V2 {
    type Output = V2;
    #[inline(always)]
    fn sub(self, o: V2) -> V2 {
        V2::new(self.x - o.x, self.y - o.y)
    }
}

impl Mul<f64> for V2 {
    type Output = V2;
    #[inline(always)]
    fn mul(self, s: f64) -> V2 {
        V2::new(self.x * s, self.y * s)
    }
}

impl Div<f64> for V2 {
    type Output = V2;
    #[inline(always)]
    fn div(self, s: f64) -> V2 {
        V2::new(self.x / s, self.y / s)
    }
}

#[inline(always)]
fn rotate(v: V2, angle: f64) -> V2 {
    let (s, c) = (angle.sin(), angle.cos());
    V2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

impl FlattenInput {
    /// Adds a polyline as a filled stroke.
    ///
    /// The stroke is one ring: the left side of the line forwards, then the right side backwards.
    /// All strokes have the same orientation, so overlapping strokes of one paint merge under the
    /// nonzero winding rule. Where the ring overlaps itself (a sharp turn between short segments,
    /// a line that crosses itself) the winding number only grows in magnitude, so no holes appear.
    pub fn add_line(&mut self, points: &[[f64; 2]], paint: usize, style: &LineStyle) {
        let h = style.width * 0.5;
        let drawable = h > 0.0 && points.len() >= 2;
        if !drawable {
            return;
        }

        // Drop repeated points: every remaining segment has a direction.
        let mut stroke_points = std::mem::take(&mut self.stroke_points);
        let mut stroke_right = std::mem::take(&mut self.stroke_right);
        stroke_points.clear();
        stroke_right.clear();
        for &p in points {
            if stroke_points.last() != Some(&p) {
                stroke_points.push(p);
            }
        }
        if stroke_points.len() >= 2 {
            self.stroke(&stroke_points, &mut stroke_right, h, paint, style);
        }
        self.stroke_points = stroke_points;
        self.stroke_right = stroke_right;
    }

    fn stroke(
        &mut self,
        points: &[[f64; 2]],
        right: &mut Vec<[f64; 2]>,
        h: f64,
        paint: usize,
        style: &LineStyle,
    ) {
        let count = points.len();
        let (join, cap, tolerance) = (style.join, style.cap, style.tolerance);

        // The largest arc step that stays within the tolerance.
        let step_angle = if h <= tolerance {
            PI
        } else {
            2.0 * (1.0 - tolerance / h).acos()
        };
        let cap_steps = ((PI / step_angle).ceil() as i64).max(2);

        let mut a = V2::from(points[0]);
        let mut b = V2::from(points[1]);
        let delta = b - a;
        let mut length = (delta.x * delta.x + delta.y * delta.y).sqrt();
        let mut d = delta / length;
        if cap == LineCap::Square {
            a = a - d * h;
        }
        let first_direction = d;
        let first_point = a;

        self.begin_ring(paint);
        self.add_point(a.x - d.y * h, a.y + d.x * h);

        if count > 2 {
            for i in 1..count - 1 {
                let v = b;
                let next = V2::from(points[i + 1]);
                let next_delta = next - v;
                let next_length =
                    (next_delta.x * next_delta.x + next_delta.y * next_delta.y).sqrt();
                let next_d = next_delta / next_length;
                let cross = d.x * next_d.y - d.y * next_d.x;
                let dot = d.x * next_d.x + d.y * next_d.y;

                // Collinear segments need no vertex on either side.
                if !(dot > 0.0 && cross.abs() < 1e-9) {
                    let n0 = V2::new(-d.y, d.x) * h;
                    let n1 = V2::new(-next_d.y, next_d.x) * h;
                    // A turn with a positive cross product has its outer side on the right (-n).
                    let outer_is_left = cross < 0.0;
                    let outer_sign = if outer_is_left { 1.0 } else { -1.0 };
                    let o0 = n0 * outer_sign;
                    let o1 = n1 * outer_sign;
                    let k = 1.0 + dot;

                    // Inner side: the two offset lines meet in one point when both segments are
                    // long enough. Otherwise go through the vertex itself, which keeps the winding
                    // number equal to that of separate overlapping segments.
                    let reach = if k > 1e-6 {
                        h * cross.abs() / k
                    } else {
                        f64::INFINITY
                    };
                    let inner_fits = reach <= 0.5 * crate::fmin(length, next_length);

                    if outer_is_left {
                        self.add_outer_join(right, v, o0, o1, cross, dot, join, step_angle, false);
                        if inner_fits {
                            let inner = v - (o0 + o1) / k;
                            right.push([inner.x, inner.y]);
                        } else {
                            let p0 = v - o0;
                            let p1 = v - o1;
                            right.push([p0.x, p0.y]);
                            right.push([v.x, v.y]);
                            right.push([p1.x, p1.y]);
                        }
                    } else {
                        if inner_fits {
                            let inner = v - (o0 + o1) / k;
                            self.add_point(inner.x, inner.y);
                        } else {
                            self.add_point(v.x - o0.x, v.y - o0.y);
                            self.add_point(v.x, v.y);
                            self.add_point(v.x - o1.x, v.y - o1.y);
                        }
                        self.add_outer_join(right, v, o0, o1, cross, dot, join, step_angle, true);
                    }
                }

                b = next;
                length = next_length;
                d = next_d;
            }
        }

        // The end: left corner, cap, right corner.
        let mut end = b;
        if cap == LineCap::Square {
            end = end + d * h;
        }
        let end_normal = V2::new(-d.y, d.x) * h;
        self.add_point(end.x + end_normal.x, end.y + end_normal.y);
        if cap == LineCap::Round {
            // Rotating the normal by a negative angle passes through the line direction.
            for s in 1..cap_steps {
                let p = end + rotate(end_normal, -PI * s as f64 / cap_steps as f64);
                self.add_point(p.x, p.y);
            }
        }
        self.add_point(end.x - end_normal.x, end.y - end_normal.y);

        // The right side, backwards.
        self.coords.reserve(right.len() * 2);
        for p in right.iter().rev() {
            self.add_point(p[0], p[1]);
        }

        // The start: right corner, cap. The ring closes at the left corner.
        let start_normal = V2::new(-first_direction.y, first_direction.x) * h;
        self.add_point(
            first_point.x - start_normal.x,
            first_point.y - start_normal.y,
        );
        if cap == LineCap::Round {
            for s in 1..cap_steps {
                let p = first_point - rotate(start_normal, -PI * s as f64 / cap_steps as f64);
                self.add_point(p.x, p.y);
            }
        }
        self.end_ring();
    }

    /// Emits the outer corner of a turn, from offset `o0` to offset `o1` around `v`.
    #[allow(clippy::too_many_arguments)]
    fn add_outer_join(
        &mut self,
        right: &mut Vec<[f64; 2]>,
        v: V2,
        o0: V2,
        o1: V2,
        cross: f64,
        dot: f64,
        join: LineJoin,
        step_angle: f64,
        to_right: bool,
    ) {
        let mut emit = |p: V2| {
            if to_right {
                right.push([p.x, p.y]);
            } else {
                self.add_point(p.x, p.y);
            }
        };
        emit(v + o0);
        match join {
            LineJoin::Bevel => {}
            LineJoin::Miter => {
                let k = 1.0 + dot;
                // A miter longer than twice the half width falls back to a bevel.
                if k > 0.5 {
                    emit(v + (o0 + o1) / k);
                }
            }
            LineJoin::Round => {
                let angle = cross.abs().atan2(dot);
                let steps = (angle / step_angle).ceil() as i64;
                if steps > 1 {
                    let step = (if cross > 0.0 { angle } else { -angle }) / steps as f64;
                    for s in 1..steps {
                        emit(v + rotate(o0, step * s as f64));
                    }
                }
            }
        }
        emit(v + o1);
    }
}
