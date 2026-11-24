// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The camera state: an immutable value, a change is a new state.

use map_geo::projection::{clamp_normalized_world_y, latitude_from_normalized_world_y, longitude_from_normalized_world_x, wrap_normalized_world_x};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraState {
    /// The point the camera looks at, normalized world Mercator (x east, y south, both 0...1).
    pub center_world_mercator: [f64; 2],
    pub zoom: f64,
    /// Radians.
    pub bearing: f64,
    /// Radians from straight down.
    pub pitch: f64,
}

impl Default for CameraState {
    fn default() -> CameraState {
        CameraState { center_world_mercator: [0.5, 0.5], zoom: 0.0, bearing: 0.0, pitch: 0.0 }
    }
}

impl CameraState {
    /// A state with the centre's x wrapped into the world and its y clamped to it.
    pub fn new(center_world_mercator: [f64; 2], zoom: f64, bearing: f64, pitch: f64) -> CameraState {
        CameraState { center_world_mercator: [wrap_normalized_world_x(center_world_mercator[0]), clamp_normalized_world_y(center_world_mercator[1])], zoom, bearing, pitch }
    }

    /// The latitude of the view centre, radians.
    pub fn latitude(&self) -> f64 {
        latitude_from_normalized_world_y(self.center_world_mercator[1])
    }

    /// The longitude of the view centre, radians.
    pub fn longitude(&self) -> f64 {
        longitude_from_normalized_world_x(self.center_world_mercator[0])
    }
}
