// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! What the camera reads of the map's settings: its zoom and pitch ranges,
//! the bounds it is held to, and the factors that turn a gesture into a move.

/// What the camera does at the edge of its bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoundsEdgeBehavior {
    /// The centre stops at the edge.
    Hard,
    /// A drag past the edge stretches, less the farther it goes, up to
    /// `maximum_stretch` screen points, and the centre is pulled back once
    /// nothing moves it: half the way in `pull_half_life` seconds at the
    /// edge, faster the farther out by `pull_progression`.
    Elastic { maximum_stretch: f64, pull_half_life: f64, pull_progression: f64 },
}

impl BoundsEdgeBehavior {
    pub const DEFAULT_ELASTIC: BoundsEdgeBehavior = BoundsEdgeBehavior::Elastic { maximum_stretch: 200.0, pull_half_life: 0.35, pull_progression: 2.0 };
}

/// A cubic Bezier timing curve from (0, 0) to (1, 1) by its two control points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PullCurve {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

impl PullCurve {
    pub const LINEAR: PullCurve = PullCurve { x1: 0.0, y1: 0.0, x2: 1.0, y2: 1.0 };
    pub const EASE_IN: PullCurve = PullCurve { x1: 0.42, y1: 0.0, x2: 1.0, y2: 1.0 };
    pub const EASE_OUT: PullCurve = PullCurve { x1: 0.0, y1: 0.0, x2: 0.58, y2: 1.0 };
    pub const EASE_IN_OUT: PullCurve = PullCurve { x1: 0.42, y1: 0.0, x2: 0.58, y2: 1.0 };

    /// A curve with its control points' x within 0...1, so it is a function of x.
    pub fn new(x1: f64, y1: f64, x2: f64, y2: f64) -> PullCurve {
        PullCurve { x1: x1.max(0.0).min(1.0), y1, x2: x2.max(0.0).min(1.0), y2 }
    }
}

/// The region the view centre is held to: a rectangle of latitude and
/// longitude, in degrees, that closes in on the centre as the camera zooms
/// through `pull_zoom_range`, from the whole world to the rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraBounds {
    /// Latitude and longitude of the south west corner, degrees.
    pub south_west: [f64; 2],
    /// Latitude and longitude of the north east corner, degrees.
    pub north_east: [f64; 2],
    pub pull_zoom_range: (f64, f64),
    pub pull_curve: PullCurve,
    pub edge_behavior: BoundsEdgeBehavior,
}

impl CameraBounds {
    pub fn new(south_west: [f64; 2], north_east: [f64; 2]) -> CameraBounds {
        CameraBounds { south_west, north_east, pull_zoom_range: (1.0, 2.0), pull_curve: PullCurve::EASE_IN_OUT, edge_behavior: BoundsEdgeBehavior::DEFAULT_ELASTIC }
    }
}

/// The camera group of the map's settings, as the camera state reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraSettings {
    /// Radians from straight down.
    pub maximum_pitch: f64,
    pub minimum_pitch: f64,
    pub minimum_zoom: f64,
    pub maximum_zoom: f64,
    pub bounds: Option<CameraBounds>,
    /// How far past `maximum_pitch` the camera may tilt at high zoom, radians, reached between the two zooms.
    pub high_zoom_pitch_extension: f64,
    pub high_zoom_pitch_extension_start_zoom: f64,
    pub high_zoom_pitch_extension_end_zoom: f64,
    /// A second extension on top, closer still.
    pub extra_high_zoom_pitch_extension: f64,
    pub extra_high_zoom_pitch_extension_start_zoom: f64,
    pub extra_high_zoom_pitch_extension_end_zoom: f64,
    pub gesture_pan_translation_scale: f64,
    pub world_pan_sensitivity: f64,
    pub world_pan_speed: f64,
    pub pinch_zoom_factor: f64,
    pub pinch_zoom_velocity_factor: f64,
    pub pinch_zoom_velocity_limit: f64,
}

impl Default for CameraSettings {
    fn default() -> CameraSettings {
        CameraSettings {
            maximum_pitch: std::f64::consts::PI * 17.0 / 36.0,
            minimum_pitch: 0.0,
            minimum_zoom: 0.0,
            maximum_zoom: 20.0,
            bounds: None,
            high_zoom_pitch_extension: 0.0,
            high_zoom_pitch_extension_start_zoom: 15.0,
            high_zoom_pitch_extension_end_zoom: 16.0,
            extra_high_zoom_pitch_extension: 0.0,
            extra_high_zoom_pitch_extension_start_zoom: 18.4,
            extra_high_zoom_pitch_extension_end_zoom: 20.0,
            gesture_pan_translation_scale: 0.1,
            world_pan_sensitivity: 0.05,
            world_pan_speed: 0.5,
            pinch_zoom_factor: 0.4,
            pinch_zoom_velocity_factor: 0.2,
            pinch_zoom_velocity_limit: 8.0,
        }
    }
}

fn interpolated_pitch_extension(zoom: f64, extension_angle: f64, start_zoom: f64, end_zoom: f64) -> f64 {
    let extension = extension_angle.max(0.0);
    if !(extension > 0.0) {
        return 0.0;
    }
    let end = end_zoom.max(start_zoom);
    if !(end - start_zoom > f64::from_bits(1)) {
        return if zoom >= start_zoom { extension } else { 0.0 };
    }
    extension * ((zoom - start_zoom) / (end - start_zoom)).max(0.0).min(1.0)
}

impl CameraSettings {
    /// The zoom within the settings' range, never under 0. A range the wrong way round is its upper end.
    pub fn clamp_zoom(&self, zoom: f64) -> f64 {
        let lower = self.minimum_zoom.max(0.0);
        if !(lower <= self.maximum_zoom) {
            return self.maximum_zoom;
        }
        zoom.max(lower).min(self.maximum_zoom)
    }

    /// How far past `maximum_pitch` the camera may tilt at a zoom.
    pub fn pitch_extension(&self, zoom: f64) -> f64 {
        interpolated_pitch_extension(zoom, self.high_zoom_pitch_extension, self.high_zoom_pitch_extension_start_zoom, self.high_zoom_pitch_extension_end_zoom)
            + interpolated_pitch_extension(zoom, self.extra_high_zoom_pitch_extension, self.extra_high_zoom_pitch_extension_start_zoom, self.extra_high_zoom_pitch_extension_end_zoom)
    }

    pub fn maximum_reachable_pitch(&self, zoom: f64) -> f64 {
        self.maximum_pitch.max(0.0) + self.pitch_extension(zoom)
    }

    pub fn minimum_reachable_pitch(&self, zoom: f64) -> f64 {
        self.minimum_pitch.max(0.0).min(self.maximum_reachable_pitch(zoom))
    }

    pub fn clamp_pitch(&self, pitch: f64, zoom: f64) -> f64 {
        pitch.max(self.minimum_reachable_pitch(zoom)).min(self.maximum_reachable_pitch(zoom))
    }
}

/// A pinch: the zoom the pinch's scale and velocity ask for. A velocity
/// along the scale's direction boosts it, up to the limit.
pub fn pinch_zoom_delta(scale: f64, velocity: f64, settings: &CameraSettings) -> f64 {
    let scale_delta = scale - 1.0;
    let (factor, velocity_factor, velocity_limit) = (settings.pinch_zoom_factor, settings.pinch_zoom_velocity_factor, settings.pinch_zoom_velocity_limit);
    if ![scale_delta, velocity, factor, velocity_factor, velocity_limit].iter().all(|value| value.is_finite()) {
        return 0.0;
    }
    let aligned = if scale_delta == 0.0 || scale_delta * velocity <= 0.0 { 0.0 } else { velocity.abs().min(velocity_limit.max(0.0)) };
    scale_delta * factor * (1.0 + aligned * velocity_factor.max(0.0))
}
