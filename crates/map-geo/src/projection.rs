// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Web Mercator between its spaces (the crate's comment names them).

use std::f64::consts::PI;

/// Equatorial Earth circumference in meters (WGS84).
pub const EARTH_CIRCUMFERENCE_METERS: f64 = 40_075_016.686;

/// The latitude, in radians, where Web Mercator's square world ends: about 85.05 degrees.
pub fn max_mercator_latitude() -> f64 {
    2.0 * PI.exp().atan() - PI * 0.5
}

/// Render units per ground meter on the flat map at a latitude: the full
/// `render_map_size` spans the equatorial circumference, and Mercator
/// stretches ground distances by 1/cos(latitude).
pub fn world_units_per_meter(latitude: f64, render_map_size: f64) -> f64 {
    let limit = max_mercator_latitude();
    render_map_size / (EARTH_CIRCUMFERENCE_METERS * latitude.max(-limit).min(limit).cos())
}

pub fn wrap_normalized_world_x(x: f64) -> f64 {
    let wrapped = x % 1.0;
    if wrapped < 0.0 { wrapped + 1.0 } else { wrapped }
}

pub fn clamp_normalized_world_y(y: f64) -> f64 {
    y.max(0.0).min(1.0)
}

/// Latitude in radians to normalized Mercator Y (-1...1), clamped at the poles to stay finite.
pub fn y_mercator_normalized(latitude: f64) -> f64 {
    let max_sin = PI.tanh();
    let clamped = latitude.sin().min(max_sin).max(-max_sin);
    0.5 * ((1.0 + clamped) / (1.0 - clamped)).ln() / PI
}

/// Normalized world Mercator of a latitude and longitude in radians.
pub fn world_mercator(latitude: f64, longitude: f64) -> [f64; 2] {
    let limit = max_mercator_latitude();
    let x = wrap_normalized_world_x((longitude + PI) / (2.0 * PI));
    let y = clamp_normalized_world_y((1.0 - y_mercator_normalized(latitude.max(-limit).min(limit))) * 0.5);
    [x, y]
}

/// Latitude in radians of a normalized world Y.
pub fn latitude_from_normalized_world_y(y: f64) -> f64 {
    let mercator_y = (1.0 - 2.0 * clamp_normalized_world_y(y)) * PI;
    2.0 * mercator_y.exp().atan() - PI * 0.5
}

/// Longitude in radians of a normalized world X.
pub fn longitude_from_normalized_world_x(x: f64) -> f64 {
    wrap_normalized_world_x(x) * (2.0 * PI) - PI
}

/// The flat render world's pan for a view centre.
pub fn flat_pan(center_world_mercator: [f64; 2]) -> [f64; 2] {
    [1.0 - 2.0 * wrap_normalized_world_x(center_world_mercator[0]), 1.0 - 2.0 * clamp_normalized_world_y(center_world_mercator[1])]
}

/// The globe's pan for a view centre: the longitude as on the plane, the latitude over the Mercator limit.
pub fn globe_pan(center_world_mercator: [f64; 2]) -> [f64; 2] {
    let latitude = latitude_from_normalized_world_y(center_world_mercator[1]);
    [1.0 - 2.0 * wrap_normalized_world_x(center_world_mercator[0]), (latitude / max_mercator_latitude()).max(-1.0).min(1.0)]
}
