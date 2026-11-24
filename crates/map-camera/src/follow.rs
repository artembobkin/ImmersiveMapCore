// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The eased follow of a bearing or a pitch toward a target, the constraints
//! they are held to, the shift that keeps the point under the cursor under
//! it through a zoom, and the decay of a pan's inertia.

use std::f64::consts::PI;

use map_geo::projection::{clamp_normalized_world_y, latitude_from_normalized_world_y, wrap_normalized_world_x};

use crate::flight::normalized_bearing;
use crate::settings::CameraSettings;
use crate::surface_scale::{proximity_distance_factor, surface_scale};

/// The longest step a follow or an inertia takes at once, seconds: a frame after a stall does not jump.
pub const MAXIMUM_DELTA_TIME: f64 = 0.05;
/// How near its target a follow is snapped onto it, radians.
pub const SNAP_THRESHOLD: f64 = 0.0003;
/// The least a follow moves in a frame and still counts as moving, radians.
pub const PROGRESS_THRESHOLD: f64 = 0.00002;

pub fn clamped_delta_time(delta_time: f64) -> f64 {
    if !delta_time.is_finite() {
        return 0.0;
    }
    delta_time.max(0.0).min(MAXIMUM_DELTA_TIME)
}

/// The share of the way left after `delta_time` at a half life.
fn decay(delta_time: f64, half_life: f64) -> f64 {
    let half_life = if half_life.is_finite() { half_life } else { 0.001 }.max(0.001);
    (-std::f64::consts::LN_2 * delta_time / half_life).exp()
}

/// The turn from `current` to `target`, the short way around.
pub fn shortest_bearing_follow_delta(current: f64, target: f64) -> f64 {
    let two_pi = PI * 2.0;
    let delta = (target - current) % two_pi;
    if delta > PI {
        delta - two_pi
    } else if delta < -PI {
        delta + two_pi
    } else {
        delta
    }
}

/// A bearing brought into -pi (exclusive) to pi.
pub fn normalized_follow_bearing(bearing: f64) -> f64 {
    let two_pi = PI * 2.0;
    let value = bearing % two_pi;
    if value > PI {
        value - two_pi
    } else if value <= -PI {
        value + two_pi
    } else {
        value
    }
}

/// The turn a follow makes: the short way around on a map that turns
/// freely, and the way that does not cross the limit on one that does not.
pub fn bearing_follow_delta(current: f64, target: f64, maximum_absolute_bearing: f64) -> f64 {
    if !(maximum_absolute_bearing < PI) {
        return shortest_bearing_follow_delta(current, target);
    }
    normalized_follow_bearing(target) - normalized_follow_bearing(current)
}

pub fn stepped_bearing(current: f64, target: f64, delta_time: f64, half_life: f64, maximum_absolute_bearing: f64) -> f64 {
    current + bearing_follow_delta(current, target, maximum_absolute_bearing) * (1.0 - decay(delta_time, half_life))
}

pub fn bearing_should_snap(current: f64, target: f64) -> bool {
    shortest_bearing_follow_delta(current, target).abs() <= SNAP_THRESHOLD
}

/// Whether a bearing follow has stopped short of its target, held by a constraint.
pub fn bearing_is_stalled(current: f64, previous: f64, target: f64) -> bool {
    shortest_bearing_follow_delta(previous, current).abs() <= PROGRESS_THRESHOLD && shortest_bearing_follow_delta(current, target).abs() > SNAP_THRESHOLD
}

pub fn stepped_pitch(current: f64, target: f64, delta_time: f64, half_life: f64) -> f64 {
    target - (target - current) * decay(delta_time, half_life)
}

pub fn pitch_should_snap(current: f64, target: f64) -> bool {
    (target - current).abs() <= SNAP_THRESHOLD
}

pub fn pitch_is_stalled(current: f64, previous: f64, target: f64) -> bool {
    (current - previous).abs() <= PROGRESS_THRESHOLD && (target - current).abs() > SNAP_THRESHOLD
}

/// The bearing held within a limit either side of north, `None` for a map that turns freely.
pub fn constrained_bearing(bearing: f64, maximum_absolute_bearing: Option<f64>) -> f64 {
    let normalized = normalized_bearing(bearing);
    match maximum_absolute_bearing {
        None => normalized,
        Some(limit) => {
            let limit = limit.max(0.0).min(PI);
            normalized.max(-limit).min(limit)
        }
    }
}

/// The bearing limit on the globe: from the least at zoom 0 to the map's own at `unlock_zoom`.
pub fn globe_maximum_absolute_bearing(zoom: f64, minimum_absolute_bearing: f64, unlock_zoom: f64, maximum_absolute_bearing: Option<f64>) -> f64 {
    let ceiling = maximum_absolute_bearing.unwrap_or(PI).max(0.0).min(PI);
    let minimum = minimum_absolute_bearing.max(0.0).min(ceiling);
    let unlock_zoom = unlock_zoom.max(0.0);
    if !(unlock_zoom > f64::from_bits(1)) {
        return ceiling;
    }
    minimum + (ceiling - minimum) * (zoom / unlock_zoom).max(0.0).min(1.0)
}

/// The pitch held within the settings' range at a zoom.
pub fn constrained_pitch(pitch: f64, settings: &CameraSettings, zoom: f64) -> f64 {
    let maximum = settings.maximum_reachable_pitch(zoom).max(0.0);
    pitch.max(settings.minimum_pitch.max(0.0).min(maximum)).min(maximum)
}

/// What a zoom about a point of the screen is compensated from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomAnchorInput {
    /// The point the zoom is about, in points from the viewport's top left.
    pub anchor_point: [f64; 2],
    pub viewport_size: [f64; 2],
    pub center_world_mercator: [f64; 2],
    pub zoom_before: f64,
    pub zoom_after: f64,
    pub bearing: f64,
    pub transition_before: f64,
    pub transition_after: f64,
    pub globe_radius_scale: f64,
    /// How much of the compensation is applied, 1 for all of it.
    pub anchor_factor: f64,
}

fn normalized_world_units_per_point(zoom: f64, transition: f64, latitude: f64, viewport_height: f64, globe_radius_scale: f64) -> f64 {
    let camera_distance = 1.0 - 0.5 * (zoom % 1.0);
    let render_map_size = 2.0 * PI * globe_radius_scale * 2f64.powf(zoom.floor());
    (2.0 * camera_distance * proximity_distance_factor(latitude, transition, zoom) * (PI / 8.0).tan()) / (viewport_height * render_map_size * surface_scale(latitude, transition))
}

/// The centre that keeps the world point under the anchor under it after a zoom.
pub fn zoom_anchor_center(input: &ZoomAnchorInput) -> [f64; 2] {
    let [width, height] = input.viewport_size;
    let center = input.center_world_mercator;
    if !(width > 0.0) || !(height > 0.0) || !(input.anchor_factor > 0.0) || input.zoom_before == input.zoom_after {
        return center;
    }
    let anchor_x = input.anchor_point[0] - width * 0.5;
    let anchor_y = height * 0.5 - input.anchor_point[1];
    if anchor_x == 0.0 && anchor_y == 0.0 {
        return center;
    }
    let (sin_bearing, cos_bearing) = (input.bearing.sin(), input.bearing.cos());
    let world_x = anchor_x * cos_bearing - anchor_y * sin_bearing;
    let world_y = anchor_x * sin_bearing + anchor_y * cos_bearing;
    let latitude = latitude_from_normalized_world_y(center[1]);
    let delta = normalized_world_units_per_point(input.zoom_before, input.transition_before, latitude, height, input.globe_radius_scale) - normalized_world_units_per_point(input.zoom_after, input.transition_after, latitude, height, input.globe_radius_scale);
    if !delta.is_finite() || delta == 0.0 {
        return center;
    }
    let shift = delta * input.anchor_factor;
    [wrap_normalized_world_x(center[0] + world_x * shift), clamp_normalized_world_y(center[1] - world_y * shift)]
}

fn speed(velocity: [f64; 2]) -> f64 {
    (velocity[0] * velocity[0] + velocity[1] * velocity[1]).sqrt()
}

/// Whether a released pan is fast enough to carry on.
pub fn pan_inertia_should_start(velocity: [f64; 2], activation_velocity: f64) -> bool {
    speed(velocity) >= activation_velocity.max(0.0)
}

pub fn pan_inertia_should_stop(velocity: [f64; 2], stop_velocity: f64) -> bool {
    speed(velocity) <= stop_velocity.max(0.0)
}

/// The release velocity held to a limit, its direction kept.
pub fn pan_inertia_initial_velocity(velocity: [f64; 2], maximum_velocity: f64) -> [f64; 2] {
    let limit = maximum_velocity.max(0.0);
    let speed = speed(velocity);
    if !speed.is_finite() || !(speed > 0.0) || !(speed > limit) || !(limit > 0.0) {
        return velocity;
    }
    [velocity[0] * limit / speed, velocity[1] * limit / speed]
}

pub fn pan_inertia_decayed_velocity(velocity: [f64; 2], delta_time: f64, half_life: f64) -> [f64; 2] {
    let factor = decay(delta_time, half_life);
    [velocity[0] * factor, velocity[1] * factor]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_follow_covers_half_the_way_in_a_half_life() {
        assert!((stepped_pitch(0.0, 1.0, 0.06, 0.06) - 0.5).abs() < 1e-12);
        assert!((stepped_bearing(0.0, 1.0, 0.06, 0.06, PI) - 0.5).abs() < 1e-12);
        assert!(pitch_should_snap(0.9998, 1.0) && !pitch_should_snap(0.99, 1.0));
        assert!(pitch_is_stalled(0.5, 0.5, 1.0) && !pitch_is_stalled(0.5, 0.4, 1.0));
        assert_eq!(clamped_delta_time(1.0), MAXIMUM_DELTA_TIME);
        assert_eq!(clamped_delta_time(f64::NAN), 0.0);
    }

    #[test]
    fn a_bearing_follows_the_short_way_unless_a_limit_stands_in_it() {
        assert!((shortest_bearing_follow_delta(3.0, -3.0) - (2.0 * PI - 6.0)).abs() < 1e-12);
        assert!(bearing_follow_delta(3.0, -3.0, PI) > 0.0, "freely, across south");
        assert!((bearing_follow_delta(3.0, -3.0, 3.1) + 6.0).abs() < 1e-12, "with a limit, back through north");
        assert_eq!(constrained_bearing(2.0, Some(1.0)), 1.0);
        assert!((constrained_bearing(2.0 * PI + 0.3, None) - 0.3).abs() < 1e-12);
        assert!((globe_maximum_absolute_bearing(2.0, 0.2, 4.0, None) - (0.2 + (PI - 0.2) * 0.5)).abs() < 1e-12);
        assert_eq!(globe_maximum_absolute_bearing(2.0, 0.2, 0.0, Some(1.0)), 1.0);
    }

    #[test]
    fn a_zoom_about_a_point_keeps_the_point_and_one_about_the_centre_keeps_the_centre() {
        let input = ZoomAnchorInput { anchor_point: [900.0, 300.0], viewport_size: [1200.0, 800.0], center_world_mercator: [0.5, 0.4], zoom_before: 12.0, zoom_after: 12.5, bearing: 0.0, transition_before: 1.0, transition_after: 1.0, globe_radius_scale: 0.14, anchor_factor: 1.0 };
        let moved = zoom_anchor_center(&input);
        assert!(moved[0] > 0.5 && moved[1] < 0.4, "toward the anchor, east and north of the centre");
        assert_eq!(zoom_anchor_center(&ZoomAnchorInput { anchor_point: [600.0, 400.0], ..input }), input.center_world_mercator);
        assert_eq!(zoom_anchor_center(&ZoomAnchorInput { zoom_after: 12.0, ..input }), input.center_world_mercator);
        // The world point under the anchor is the same before and after.
        let latitude = latitude_from_normalized_world_y(0.4);
        let under = |center: [f64; 2], zoom: f64| center[0] + 300.0 * normalized_world_units_per_point(zoom, 1.0, latitude, 800.0, 0.14);
        assert!((under(input.center_world_mercator, 12.0) - under(moved, 12.5)).abs() < 1e-12);
    }

    #[test]
    fn a_pans_inertia_starts_fast_and_dies_by_its_half_life() {
        assert!(pan_inertia_should_start([400.0, 300.0], 450.0) && !pan_inertia_should_start([300.0, 300.0], 450.0));
        assert_eq!(pan_inertia_initial_velocity([6000.0, 8000.0], 7000.0), [4200.0, 5600.0]);
        assert_eq!(pan_inertia_initial_velocity([60.0, 80.0], 7000.0), [60.0, 80.0]);
        let decayed = pan_inertia_decayed_velocity([100.0, 0.0], 0.28, 0.28);
        assert!((decayed[0] - 50.0).abs() < 1e-9);
        assert!(pan_inertia_should_stop(decayed, 60.0));
    }
}
