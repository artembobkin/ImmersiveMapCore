// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The local scale of the render surface at the view centre, and how much
//! closer the render camera sits to the globe for it.

use crate::presentation::geometry_transition;

/// Local scale of the render surface at the view centre relative to
/// normalized Mercator: the sphere compresses the world by `cos(latitude)`,
/// the plane does not, and through the transition the unroll gives
/// `cos + g^2 (1 - cos)` for the geometry phase `g`. The camera proximity,
/// the pan compensation and the zoom anchor all read this one curve, so
/// they agree with the geometry on screen. `transition`: 0 the globe, 1 the
/// plane.
pub fn surface_scale(latitude: f64, transition: f64) -> f64 {
    let geometry_phase = geometry_transition(transition);
    let compression = latitude.cos().max(1e-6);
    compression + geometry_phase * geometry_phase * (1.0 - compression)
}

/// The camera zoom the globe's move-in starts at.
pub const PROXIMITY_ACTIVATION_ZOOM: f64 = 3.0;
/// The zoom span over which it ramps to full.
pub const PROXIMITY_ACTIVATION_SPAN: f64 = 1.0;

/// 0 below the activation zoom, 1 a span above it, smooth between.
pub fn proximity_activation(zoom: f64) -> f64 {
    let progress = ((zoom - PROXIMITY_ACTIVATION_ZOOM) / PROXIMITY_ACTIVATION_SPAN).max(0.0).min(1.0);
    progress * progress * (3.0 - 2.0 * progress)
}

/// The factor on the camera's distance to the view centre, so the globe
/// shows the Mercator scale of the view centre: a tile near a pole is
/// `cos(latitude)` smaller on the sphere, so the camera moves in by the
/// local surface scale, and the frustum covers the same number of tiles at
/// every latitude. It ramps in from the activation zoom: at planet zooms the
/// whole globe is in view and a camera pulled onto a pole would show a
/// fraction of it for nothing. On the plane it is 1.
pub fn proximity_distance_factor(latitude: f64, transition: f64, zoom: f64) -> f64 {
    1.0 + (surface_scale(latitude, transition) - 1.0) * proximity_activation(zoom)
}
