// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Camera flights: the interpolation of a camera state from where it is to
//! a target over a duration, along the Mercator line or the great circle,
//! at a level altitude or rising to an overview first.

use std::f64::consts::PI;

use map_geo::projection::{latitude_from_normalized_world_y, longitude_from_normalized_world_x, world_mercator, wrap_normalized_world_x};

use crate::state::CameraState;

const DELTA_EPSILON: f64 = 1e-9;
const GREAT_CIRCLE_ANGLE_EPSILON: f64 = 1e-6;
/// The shortest a flight lasts, seconds.
pub const MINIMUM_DURATION: f64 = 0.001;
/// How strongly an overview flight trades zooming out for covering ground (van Wijk and Nuij's rho).
const RHO: f64 = 1.42;

/// A bearing brought into -pi...pi.
pub fn normalized_bearing(bearing: f64) -> f64 {
    let two_pi = PI * 2.0;
    let mut normalized = (bearing + PI) % two_pi;
    if normalized < 0.0 {
        normalized += two_pi;
    }
    normalized - PI
}

pub fn ease_in_out_cubic(progress: f64) -> f64 {
    let clamped = progress.max(0.0).min(1.0);
    if clamped < 0.5 {
        return 4.0 * clamped * clamped * clamped;
    }
    let inverted = -2.0 * clamped + 2.0;
    1.0 - (inverted * inverted * inverted) / 2.0
}

/// The move from `start` to `end` along the world's x, the short way around.
pub fn shortest_wrapped_world_delta(start: f64, end: f64) -> f64 {
    let delta = end - start;
    if delta > 0.5 {
        delta - 1.0
    } else if delta < -0.5 {
        delta + 1.0
    } else {
        delta
    }
}

/// The turn from `start` to `end`, the short way around.
pub fn shortest_bearing_delta(start: f64, end: f64) -> f64 {
    let delta = normalized_bearing(end) - normalized_bearing(start);
    if delta > PI {
        delta - 2.0 * PI
    } else if delta < -PI {
        delta + 2.0 * PI
    } else {
        delta
    }
}

pub fn interpolate_bearing(start: f64, end: f64, progress: f64) -> f64 {
    normalized_bearing(normalized_bearing(start) + shortest_bearing_delta(start, end) * progress)
}

/// Whether two states differ by anything a flight would show.
pub fn has_meaningful_delta(start: &CameraState, end: &CameraState) -> bool {
    shortest_wrapped_world_delta(start.center_world_mercator[0], end.center_world_mercator[0]).abs() > DELTA_EPSILON
        || (end.center_world_mercator[1] - start.center_world_mercator[1]).abs() > DELTA_EPSILON
        || (end.zoom - start.zoom).abs() > DELTA_EPSILON
        || shortest_bearing_delta(start.bearing, end.bearing).abs() > DELTA_EPSILON
        || (end.pitch - start.pitch).abs() > DELTA_EPSILON
}

/// The centre a share of the way along the straight Mercator line.
pub fn mercator_center(start: [f64; 2], end: [f64; 2], progress: f64) -> [f64; 2] {
    [wrap_normalized_world_x(start[0] + shortest_wrapped_world_delta(start[0], end[0]) * progress), start[1] + (end[1] - start[1]) * progress]
}

pub fn mercator_route_distance(start: [f64; 2], end: [f64; 2]) -> f64 {
    let (dx, dy) = (shortest_wrapped_world_delta(start[0], end[0]), end[1] - start[1]);
    (dx * dx + dy * dy).sqrt()
}

fn normalized3(v: [f64; 3]) -> [f64; 3] {
    let inverse = 1.0 / (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] * inverse, v[1] * inverse, v[2] * inverse]
}

fn cartesian_of_world(world: [f64; 2]) -> [f64; 3] {
    let latitude = latitude_from_normalized_world_y(world[1]);
    let longitude = longitude_from_normalized_world_x(world[0]);
    let cos_latitude = latitude.cos();
    normalized3([cos_latitude * longitude.cos(), cos_latitude * longitude.sin(), latitude.sin()])
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The great circle's length between two centres, as a share of the world's circumference.
pub fn great_circle_route_distance(start: [f64; 2], end: [f64; 2]) -> f64 {
    dot(cartesian_of_world(start), cartesian_of_world(end)).max(-1.0).min(1.0).acos() / (2.0 * PI)
}

/// The centre a share of the way along the great circle, `None` between antipodes, where there is no one circle.
pub fn great_circle_center(start: [f64; 2], end: [f64; 2], progress: f64) -> Option<[f64; 2]> {
    let (from, to) = (cartesian_of_world(start), cartesian_of_world(end));
    let angle = dot(from, to).max(-1.0).min(1.0).acos();
    let blended = |from_weight: f64, to_weight: f64| normalized3([from[0] * from_weight + to[0] * to_weight, from[1] * from_weight + to[1] * to_weight, from[2] * from_weight + to[2] * to_weight]);
    let interpolated = if angle < GREAT_CIRCLE_ANGLE_EPSILON {
        blended(1.0 - progress, progress)
    } else if (PI - angle).abs() < GREAT_CIRCLE_ANGLE_EPSILON {
        return None;
    } else {
        let sin_angle = angle.sin();
        if !(sin_angle.abs() >= GREAT_CIRCLE_ANGLE_EPSILON) {
            return None;
        }
        blended(((1.0 - progress) * angle).sin() / sin_angle, (progress * angle).sin() / sin_angle)
    };
    Some(world_mercator(interpolated[2].max(-1.0).min(1.0).asin(), interpolated[1].atan2(interpolated[0])))
}

/// A flight that rises to an overview: the pan and the zoom coupled, so the
/// camera covers most of the ground at the apex and barely moves near the
/// ground at either end (van Wijk and Nuij's smooth zoom and pan).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverviewFlightProfile {
    start_visible_span: f64,
    route_distance: f64,
    start_r: f64,
    total_s: f64,
    cosh_start_r: f64,
    sinh_start_r: f64,
}

impl OverviewFlightProfile {
    /// `None` for a route of no length, which is a plain zoom.
    pub fn new(start_zoom: f64, target_zoom: f64, normalized_world_distance: f64) -> Option<OverviewFlightProfile> {
        let route_distance = normalized_world_distance;
        if !route_distance.is_finite() || !(route_distance > 1e-9) {
            return None;
        }
        let start_span = 2f64.powf(-start_zoom);
        let target_span = 2f64.powf(-target_zoom);
        let rho_squared = RHO * RHO;
        let span_delta = target_span * target_span - start_span * start_span;
        let arc_term = rho_squared * rho_squared * route_distance * route_distance;
        let b0 = (span_delta + arc_term) / (2.0 * start_span * rho_squared * route_distance);
        let b1 = (span_delta - arc_term) / (2.0 * target_span * rho_squared * route_distance);
        let start_r = -b0.asinh();
        let total_s = (-b1.asinh() - start_r) / RHO;
        if !total_s.is_finite() || !(total_s > 0.0) {
            return None;
        }
        Some(OverviewFlightProfile { start_visible_span: start_span, route_distance, start_r, total_s, cosh_start_r: start_r.cosh(), sinh_start_r: start_r.sinh() })
    }

    /// The share of the route covered and the zoom at a share of the flight.
    pub fn sample(&self, progress: f64) -> (f64, f64) {
        let x = RHO * progress.max(0.0).min(1.0) * self.total_s + self.start_r;
        let traveled = (self.start_visible_span / (RHO * RHO)) * (self.cosh_start_r * x.tanh() - self.sinh_start_r);
        let visible_span = (self.start_visible_span * self.cosh_start_r) / x.cosh();
        ((traveled / self.route_distance).max(0.0).min(1.0), -visible_span.log2())
    }
}

/// The route a flight takes over the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlightRoute {
    MercatorShortestPath,
    GreatCircle,
}

/// How a flight treats the altitude between its ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlightAltitude {
    /// The zoom goes straight from its start to its target.
    Direct,
    /// The camera rises to see the route, then comes down.
    OverviewFirst,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Flight {
    start: CameraState,
    target: CameraState,
    duration: f64,
    route: FlightRoute,
    /// `None` is the direct zoom interpolation (`Direct` or a route of no length).
    overview: Option<OverviewFlightProfile>,
    start_time: f64,
}

/// A step of a flight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlightStep {
    pub camera_state: CameraState,
    pub did_finish: bool,
}

/// Holds one active flight and produces the camera state of a frame time, in seconds.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CameraFlightAnimator {
    flight: Option<Flight>,
}

impl CameraFlightAnimator {
    pub fn is_active(&self) -> bool {
        self.flight.is_some()
    }

    /// Starts a flight. False, with no flight left, when the two states do not differ.
    pub fn start(&mut self, start: CameraState, target: CameraState, duration: f64, route: FlightRoute, altitude: FlightAltitude, current_time: f64) -> bool {
        if !has_meaningful_delta(&start, &target) {
            self.flight = None;
            return false;
        }
        let overview = (altitude == FlightAltitude::OverviewFirst)
            .then(|| match route {
                FlightRoute::MercatorShortestPath => mercator_route_distance(start.center_world_mercator, target.center_world_mercator),
                FlightRoute::GreatCircle => great_circle_route_distance(start.center_world_mercator, target.center_world_mercator),
            })
            .and_then(|distance| OverviewFlightProfile::new(start.zoom, target.zoom, distance));
        self.flight = Some(Flight { start, target, duration: MINIMUM_DURATION.max(if duration.is_finite() { duration } else { 0.0 }), route, overview, start_time: current_time });
        true
    }

    /// The flight's state at a time, `None` with no flight. The step that reaches the target ends the flight.
    pub fn advance(&mut self, current_time: f64) -> Option<FlightStep> {
        let flight = self.flight?;
        let raw = if flight.duration > 0.0 { (current_time - flight.start_time) / flight.duration } else { 1.0 };
        let clamped = raw.max(0.0).min(1.0);
        let eased = ease_in_out_cubic(clamped);
        // The profile couples pan and zoom: near the ground the centre barely
        // moves, the camera covers most of the distance at the apex.
        let sample = flight.overview.map(|profile| profile.sample(eased));
        let pan_progress = sample.map_or(eased, |(pan, _)| pan);
        let (from, to) = (flight.start.center_world_mercator, flight.target.center_world_mercator);
        let center = match flight.route {
            FlightRoute::MercatorShortestPath => mercator_center(from, to, pan_progress),
            FlightRoute::GreatCircle => great_circle_center(from, to, pan_progress).unwrap_or_else(|| mercator_center(from, to, pan_progress)),
        };
        let zoom = sample.map_or(flight.start.zoom + (flight.target.zoom - flight.start.zoom) * eased, |(_, zoom)| zoom);
        let bearing = interpolate_bearing(flight.start.bearing, flight.target.bearing, eased);
        let pitch = flight.start.pitch + (flight.target.pitch - flight.start.pitch) * eased;
        let did_finish = clamped >= 1.0;
        if did_finish {
            self.flight = None;
        }
        Some(FlightStep { camera_state: CameraState::new(center, zoom, bearing, pitch), did_finish })
    }

    pub fn cancel(&mut self) {
        self.flight = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(longitude: f64, latitude: f64, zoom: f64) -> CameraState {
        CameraState::new(world_mercator(latitude.to_radians(), longitude.to_radians()), zoom, 0.0, 0.0)
    }

    #[test]
    fn a_flight_starts_at_its_start_and_ends_at_its_target() {
        let (moscow, paris) = (at(37.62, 55.75, 12.0), CameraState { bearing: 0.5, pitch: 0.8, ..at(2.35, 48.85, 14.0) });
        for (route, altitude) in [(FlightRoute::MercatorShortestPath, FlightAltitude::Direct), (FlightRoute::GreatCircle, FlightAltitude::OverviewFirst), (FlightRoute::MercatorShortestPath, FlightAltitude::OverviewFirst)] {
            let mut animator = CameraFlightAnimator::default();
            assert!(animator.start(moscow, paris, 2.0, route, altitude, 10.0));
            let first = animator.advance(10.0).expect("a step");
            assert!(!first.did_finish);
            assert!((first.camera_state.center_world_mercator[0] - moscow.center_world_mercator[0]).abs() < 1e-12);
            assert!((first.camera_state.zoom - 12.0).abs() < 1e-9);
            let last = animator.advance(12.5).expect("a step");
            assert!(last.did_finish && !animator.is_active());
            assert!((last.camera_state.center_world_mercator[0] - paris.center_world_mercator[0]).abs() < 1e-9);
            assert!((last.camera_state.center_world_mercator[1] - paris.center_world_mercator[1]).abs() < 1e-9);
            assert!((last.camera_state.zoom - 14.0).abs() < 1e-9 && (last.camera_state.bearing - 0.5).abs() < 1e-12 && (last.camera_state.pitch - 0.8).abs() < 1e-12);
            assert!(animator.advance(13.0).is_none());
        }
    }

    #[test]
    fn an_overview_flight_rises_over_the_route_and_a_direct_one_does_not() {
        let (moscow, paris) = (at(37.62, 55.75, 12.0), at(2.35, 48.85, 12.0));
        let mut overview = CameraFlightAnimator::default();
        overview.start(moscow, paris, 2.0, FlightRoute::GreatCircle, FlightAltitude::OverviewFirst, 0.0);
        let apex = overview.advance(1.0).expect("a step").camera_state;
        assert!(apex.zoom < 7.0, "the camera sees the route from above: zoom {}", apex.zoom);
        let mut direct = CameraFlightAnimator::default();
        direct.start(moscow, paris, 2.0, FlightRoute::GreatCircle, FlightAltitude::Direct, 0.0);
        assert!((direct.advance(1.0).expect("a step").camera_state.zoom - 12.0).abs() < 1e-12);
        // North of the straight Mercator line, as a great circle between two northern cities runs.
        let straight = mercator_center(moscow.center_world_mercator, paris.center_world_mercator, 0.5);
        assert!(apex.center_world_mercator[1] < straight[1]);
    }

    #[test]
    fn the_short_way_around_is_taken() {
        assert!((shortest_wrapped_world_delta(0.95, 0.05) - 0.1).abs() < 1e-12);
        assert!((shortest_bearing_delta(3.0, -3.0) - (2.0 * PI - 6.0)).abs() < 1e-12);
        assert!((mercator_center([0.95, 0.5], [0.05, 0.5], 0.75)[0] - 0.025).abs() < 1e-12, "across the antimeridian");
        assert!(great_circle_center([0.0, 0.5], [0.5, 0.5], 0.5).is_none(), "antipodes have no one great circle");
        assert!((great_circle_route_distance([0.25, 0.5], [0.5, 0.5]) - 0.25).abs() < 1e-12, "a quarter of the equator");
        let mut animator = CameraFlightAnimator::default();
        let here = at(10.0, 10.0, 5.0);
        assert!(!animator.start(here, here, 1.0, FlightRoute::GreatCircle, FlightAltitude::Direct, 0.0), "no flight to where the camera is");
        assert_eq!(ease_in_out_cubic(0.5), 0.5);
        assert!(OverviewFlightProfile::new(5.0, 6.0, 0.0).is_none());
    }
}
