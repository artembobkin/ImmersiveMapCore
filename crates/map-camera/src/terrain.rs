// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The terrain under the camera: what the camera asks of a relief to keep
//! its eye over the ground it looks at. The eye stands at the end of an arm
//! from the point the camera looks at, as long as the zoom makes it, so over
//! a terrain the zoom is the arm, and the heights along the view decide it.

use map_geo::projection::{EARTH_CIRCUMFERENCE_METERS, latitude_from_normalized_world_y, world_units_per_meter};

/// The heights of a terrain, as a host holds them.
pub trait TerrainHeights {
    /// The terrain's metres at a point of normalized world Mercator, `None` where it is not known yet.
    fn metres(&self, point: [f64; 2]) -> Option<f64>;
}

impl<F: Fn([f64; 2]) -> Option<f64>> TerrainHeights for F {
    fn metres(&self, point: [f64; 2]) -> Option<f64> {
        self(point)
    }
}

/// A terrain the camera follows.
#[derive(Clone, Copy)]
pub struct TerrainFollow<'a> {
    pub heights: &'a dyn TerrainHeights,
    /// How near the terrain under it the eye may come, metres.
    pub clearance_metres: f64,
    /// The presentation's globe radius scale, which sets the arm's length in metres at a zoom.
    pub globe_radius_scale: f64,
    /// A terrain texel in normalized world Mercator: the step the view ray is marched by. 0 for none known.
    pub sample_step_world: f64,
}

/// The direction the camera looks in over the map, in normalized world Mercator (y south).
pub fn forward(bearing: f64) -> [f64; 2] {
    [-bearing.sin(), -bearing.cos()]
}

/// The eye's distance from the point it looks at, in metres.
pub fn eye_distance_metres(zoom: f64, latitude: f64, globe_radius_scale: f64) -> f64 {
    let distance_units = 1.0 - (zoom % 1.0) * 0.5;
    let render_map_size = 2.0 * std::f64::consts::PI * globe_radius_scale * 2f64.powf(zoom.floor());
    distance_units / world_units_per_meter(latitude, render_map_size)
}

/// How much over a rise the eye keeps: nothing over flat ground, the whole
/// clearance over a rise as high as it, smooth between.
pub fn clearance_margin(rise: f64, clearance: f64) -> f64 {
    if !(clearance > 0.0) || !(rise > 0.0) {
        return 0.0;
    }
    let t = (rise / clearance).min(1.0);
    clearance * t * t * (3.0 - 2.0 * t)
}

impl TerrainFollow<'_> {
    pub fn terrain_metres(&self, point: [f64; 2]) -> Option<f64> {
        self.heights.metres(point)
    }

    /// The eye's height over the point it looks at, metres.
    pub fn eye_height_metres(&self, zoom: f64, pitch: f64, latitude: f64) -> f64 {
        eye_distance_metres(zoom, latitude, self.globe_radius_scale) * pitch.cos()
    }

    /// How far back from the point it looks at the eye stands over the map, metres.
    pub fn eye_reach_metres(&self, zoom: f64, pitch: f64, latitude: f64) -> f64 {
        eye_distance_metres(zoom, latitude, self.globe_radius_scale) * pitch.sin()
    }

    /// The point of the map the eye stands over.
    pub fn eye_ground(&self, center: [f64; 2], zoom: f64, pitch: f64, bearing: f64, latitude: f64) -> [f64; 2] {
        let metres_per_world = EARTH_CIRCUMFERENCE_METERS * latitude.cos().max(1e-6);
        let direction = forward(bearing);
        let reach = self.eye_reach_metres(zoom, pitch, latitude) / metres_per_world;
        [center[0] - direction[0] * reach, center[1] - direction[1] * reach]
    }

    /// The zoom that puts the eye `metres` over the point it looks at, within a range.
    pub fn zoom_for_eye_height_metres(&self, metres: f64, pitch: f64, latitude: f64, range: (f64, f64)) -> f64 {
        let (mut low, mut high) = range;
        if !(self.eye_height_metres(low, pitch, latitude) > metres) {
            return low;
        }
        if !(self.eye_height_metres(high, pitch, latitude) < metres) {
            return high;
        }
        for _ in 0..40 {
            let middle = (low + high) * 0.5;
            if self.eye_height_metres(middle, pitch, latitude) > metres {
                low = middle;
            } else {
                high = middle;
            }
        }
        low
    }

    /// Whether the eye is its clearance over the terrain under it.
    pub fn is_clear(&self, center: [f64; 2], zoom: f64, pitch: f64, bearing: f64) -> bool {
        let latitude = latitude_from_normalized_world_y(center[1]);
        let ground = self.eye_ground(center, zoom, pitch, bearing, latitude);
        let rise = self.terrain_metres(ground).unwrap_or(0.0) - self.terrain_metres(center).unwrap_or(0.0);
        self.eye_height_metres(zoom, pitch, latitude) >= rise + clearance_margin(rise, self.clearance_metres)
    }

    /// The zoom, or the shallower one that keeps the eye its clearance over
    /// the terrain under it: over a ridge between the eye and the point the
    /// eye backs off along its view ray, by the arm.
    pub fn clear_zoom(&self, zoom: f64, center: [f64; 2], pitch: f64, bearing: f64, range: (f64, f64)) -> f64 {
        let mut cleared = zoom.max(range.0).min(range.1);
        let latitude = latitude_from_normalized_world_y(center[1]);
        let point_metres = self.terrain_metres(center).unwrap_or(0.0);
        for _ in 0..4 {
            let height = self.eye_height_metres(cleared, pitch, latitude);
            let ground = self.eye_ground(center, cleared, pitch, bearing, latitude);
            let rise = self.terrain_metres(ground).unwrap_or(0.0) - point_metres;
            let needed = rise + clearance_margin(rise, self.clearance_metres);
            if !(height < needed) {
                return cleared;
            }
            let lower = self.zoom_for_eye_height_metres(needed, pitch, latitude, range);
            if !(lower < cleared) {
                return cleared;
            }
            cleared = lower;
        }
        cleared
    }
}
