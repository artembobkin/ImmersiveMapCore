// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The region the view centre is held to (`CameraBounds`), in normalized
//! world Mercator: the area allowed at a zoom, the centre brought into it,
//! a drag resisted at its edge, and the pull back to it.

use map_geo::projection::{clamp_normalized_world_y, world_mercator, wrap_normalized_world_x};

use crate::settings::{CameraBounds, PullCurve};

/// The area allowed at a zoom.
struct Area {
    /// Half the allowed width about the bounds' centre, `None` for the whole world.
    half_width: Option<f64>,
    lowest_y: f64,
    highest_y: f64,
}

/// A step of the pull back to the area: the centre after it and how far it still is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundsReturnStep {
    pub center: [f64; 2],
    pub remaining_distance: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraBoundsConstraint {
    center_x: f64,
    half_width: f64,
    minimum_y: f64,
    maximum_y: f64,
    pull_zoom_range: (f64, f64),
    pull_curve: PullCurve,
}

/// An offset along the world's x taken the short way around.
fn short_offset(offset: f64) -> f64 {
    let wrapped = offset % 1.0;
    if wrapped >= 0.5 {
        wrapped - 1.0
    } else if wrapped < -0.5 {
        wrapped + 1.0
    } else {
        wrapped
    }
}

/// A move from `current` to `proposed` held by the edges: free inside, and
/// past an edge slowed so that no drag reaches `maximum_stretch` beyond it.
fn stretch(current: f64, proposed: f64, lower_edge: f64, upper_edge: f64, maximum_stretch: f64) -> f64 {
    if !(maximum_stretch > 0.0) {
        return proposed.max(lower_edge).min(upper_edge);
    }
    if proposed > upper_edge && proposed > current {
        let past = (current - upper_edge).max(0.0);
        if !(past < maximum_stretch) {
            return current;
        }
        let travel = (maximum_stretch * past) / (maximum_stretch - past) + (proposed - current.max(upper_edge));
        return upper_edge + (maximum_stretch * travel) / (maximum_stretch + travel);
    }
    if proposed < lower_edge && proposed < current {
        let past = (lower_edge - current).max(0.0);
        if !(past < maximum_stretch) {
            return current;
        }
        let travel = (maximum_stretch * past) / (maximum_stretch - past) + (current.min(lower_edge) - proposed);
        return lower_edge - (maximum_stretch * travel) / (maximum_stretch + travel);
    }
    proposed
}

fn bezier_coordinate(t: f64, p1: f64, p2: f64) -> f64 {
    let inverse = 1.0 - t;
    3.0 * inverse * inverse * t * p1 + 3.0 * inverse * t * t * p2 + t * t * t
}

fn bezier_derivative(t: f64, p1: f64, p2: f64) -> f64 {
    let inverse = 1.0 - t;
    3.0 * inverse * inverse * p1 + 6.0 * inverse * t * (p2 - p1) + 3.0 * t * t * (1.0 - p2)
}

/// The curve's y at an x in 0...1: Newton's steps on the curve's parameter, then a bisection where they do not settle.
pub fn pull_curve_value(curve: &PullCurve, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let mut t = x;
    for _ in 0..8 {
        let error = bezier_coordinate(t, curve.x1, curve.x2) - x;
        if error.abs() < 1e-9 {
            return bezier_coordinate(t, curve.y1, curve.y2);
        }
        let slope = bezier_derivative(t, curve.x1, curve.x2);
        if !(slope.abs() > 1e-6) {
            break;
        }
        t = (t - error / slope).max(0.0).min(1.0);
    }
    let (mut low, mut high) = (0.0, 1.0);
    t = x;
    for _ in 0..60 {
        let value = bezier_coordinate(t, curve.x1, curve.x2);
        if (value - x).abs() < 1e-9 {
            break;
        }
        if value < x {
            low = t;
        } else {
            high = t;
        }
        t = (low + high) * 0.5;
    }
    bezier_coordinate(t, curve.y1, curve.y2)
}

impl CameraBoundsConstraint {
    pub fn new(bounds: &CameraBounds) -> CameraBoundsConstraint {
        let west_x = (bounds.south_west[1] + 180.0) / 360.0;
        let east_x = (bounds.north_east[1] + 180.0) / 360.0;
        let mut width = east_x - west_x;
        if width < 0.0 {
            width += 1.0;
        }
        let width = width.min(1.0);
        let south_y = world_mercator(bounds.south_west[0].to_radians(), 0.0)[1];
        let north_y = world_mercator(bounds.north_east[0].to_radians(), 0.0)[1];
        CameraBoundsConstraint { center_x: west_x + width * 0.5, half_width: width * 0.5, minimum_y: south_y.min(north_y), maximum_y: south_y.max(north_y), pull_zoom_range: bounds.pull_zoom_range, pull_curve: bounds.pull_curve }
    }

    /// How far the area has closed in at a zoom: 0 the whole world, 1 the bounds themselves.
    pub fn pull(&self, zoom: f64) -> f64 {
        let lower = self.pull_zoom_range.0;
        let span = self.pull_zoom_range.1 - lower;
        if !(span > f64::from_bits(1)) {
            return if zoom >= lower { 1.0 } else { 0.0 };
        }
        pull_curve_value(&self.pull_curve, ((zoom - lower) / span).max(0.0).min(1.0)).max(0.0).min(1.0)
    }

    fn area(&self, zoom: f64) -> Area {
        let open_share = 1.0 - self.pull(zoom);
        let allowed_half_width = self.half_width + (0.5 - self.half_width) * open_share;
        Area { half_width: (allowed_half_width < 0.5).then_some(allowed_half_width), lowest_y: self.minimum_y * (1.0 - open_share), highest_y: self.maximum_y + (1.0 - self.maximum_y) * open_share }
    }

    /// The centre brought into the area of a zoom.
    pub fn apply(&self, center: [f64; 2], zoom: f64) -> [f64; 2] {
        let area = self.area(zoom);
        let mut x = center[0];
        if let Some(half_width) = area.half_width {
            x = self.center_x + short_offset(x - self.center_x).max(-half_width).min(half_width);
        }
        let y = center[1].max(area.lowest_y).min(area.highest_y);
        [wrap_normalized_world_x(x), clamp_normalized_world_y(y)]
    }

    /// A drag from `current` to `proposed`, resisted past the area's edges (`maximum_stretch` in world units).
    pub fn resist(&self, current: [f64; 2], proposed: [f64; 2], zoom: f64, maximum_stretch: f64) -> [f64; 2] {
        let area = self.area(zoom);
        let mut x = proposed[0];
        if let Some(half_width) = area.half_width {
            let current_offset = short_offset(current[0] - self.center_x);
            let proposed_offset = current_offset + short_offset(proposed[0] - current[0]);
            x = self.center_x + stretch(current_offset, proposed_offset, -half_width, half_width, maximum_stretch);
        }
        let y = stretch(current[1], proposed[1], area.lowest_y, area.highest_y, maximum_stretch);
        [wrap_normalized_world_x(x), clamp_normalized_world_y(y)]
    }

    /// How far outside the area a centre is, 0 inside it.
    pub fn distance_outside(&self, center: [f64; 2], zoom: f64) -> f64 {
        let target = self.apply(center, zoom);
        let (dx, dy) = (short_offset(target[0] - center[0]), target[1] - center[1]);
        (dx * dx + dy * dy).sqrt()
    }

    /// The centre moved a `fraction` of the way back to the area.
    pub fn return_step(&self, center: [f64; 2], zoom: f64, fraction: f64) -> BoundsReturnStep {
        let target = self.apply(center, zoom);
        let gap = [short_offset(target[0] - center[0]), target[1] - center[1]];
        let share = fraction.max(0.0).min(1.0);
        let next = [wrap_normalized_world_x(center[0] + gap[0] * share), clamp_normalized_world_y(center[1] + gap[1] * share)];
        let (dx, dy) = (short_offset(target[0] - next[0]), target[1] - next[1]);
        BoundsReturnStep { center: next, remaining_distance: (dx * dx + dy * dy).sqrt() }
    }
}
