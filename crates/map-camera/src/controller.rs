// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The one funnel every camera change goes through (gestures, commands,
//! flights): it holds the camera state and applies the zoom and pitch
//! ranges, the region bounds and the terrain follow to every change.

use map_geo::projection::{EARTH_CIRCUMFERENCE_METERS, clamp_normalized_world_y, latitude_from_normalized_world_y, longitude_from_normalized_world_x, max_mercator_latitude, world_mercator, wrap_normalized_world_x};

use crate::bounds::CameraBoundsConstraint;
use crate::settings::{BoundsEdgeBehavior, CameraSettings, pinch_zoom_delta};
use crate::state::CameraState;
use crate::surface_scale::{proximity_distance_factor, surface_scale};
use crate::terrain::{TerrainFollow, eye_distance_metres, forward};

/// The horizontal latitude pan compensation ramps in as the camera zooms in:
/// below start there is none, above end it is full.
const HORIZONTAL_PAN_COMPENSATION_START_ZOOM: f64 = 2.5;
const HORIZONTAL_PAN_COMPENSATION_END_ZOOM: f64 = 5.0;
/// The fastest the elastic pull gets, as a multiple of its rate at the edge:
/// a centre far out, after a jump or a deep zoom, still glides in over a few
/// tenths of a second instead of teleporting.
const MAXIMUM_PULL_GROWTH: f64 = 16.0;
/// The most samples the march takes along the ray: a reach far past what the
/// texel's step covers in that many is marched coarser.
pub const MARCH_SAMPLE_LIMIT: f64 = 1024.0;
/// How far under the terrain the eye may be read and still be over it.
pub const EYE_OVER_GROUND_TOLERANCE_METRES: f64 = 1.0;

/// Which path a settle on the terrain took: the view ray met the terrain, or the move fell back to the plain pan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettlePath {
    Hit,
    Fallback,
}

/// How the camera's last move over the terrain was settled, for a debug panel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerrainFollowReport {
    pub path: SettlePath,
    /// The eye's distance from the point it looks at, in metres.
    pub arm_metres: f64,
    /// The zoom after the clearance less the zoom the settle answered, zero or negative.
    pub cleared_zoom_delta: f64,
}

/// A settled move over the terrain: the point looked at and the zoom that puts the eye where it belongs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerrainFollowResult {
    pub center: [f64; 2],
    pub zoom: f64,
}

/// Why a position was not taken.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CameraPositionError {
    NotFinite,
    /// The latitude, in degrees, lies past the Mercator limit.
    LatitudeOutOfRange(f64),
}

impl std::fmt::Display for CameraPositionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CameraPositionError::NotFinite => write!(f, "Camera position values must be finite."),
            CameraPositionError::LatitudeOutOfRange(latitude) => write!(f, "Latitude out of range for Mercator: {latitude}"),
        }
    }
}

impl std::error::Error for CameraPositionError {}

fn horizontal_compensation_ramp(zoom: f64) -> f64 {
    let progress = ((zoom - HORIZONTAL_PAN_COMPENSATION_START_ZOOM) / (HORIZONTAL_PAN_COMPENSATION_END_ZOOM - HORIZONTAL_PAN_COMPENSATION_START_ZOOM)).max(0.0).min(1.0);
    progress * progress * (3.0 - 2.0 * progress)
}

#[derive(Debug, Clone)]
pub struct CameraStateController {
    state: CameraState,
    settings: CameraSettings,
    bounds_constraint: Option<CameraBoundsConstraint>,
    terrain_follow_report: Option<TerrainFollowReport>,
    last_settle_path: SettlePath,
    planning_zoom: f64,
    /// The zoom a drag over the terrain moves the map by: the camera's zoom
    /// at the drag's start, held to its end (`end_pan`), `None` between
    /// drags. The camera's zoom follows the ground through the drag, and a
    /// drag moved by the zoom of the moment would slow on the way up a slope
    /// and hurry on the way down.
    drag_zoom: Option<f64>,
}

impl CameraStateController {
    pub fn new(settings: CameraSettings) -> CameraStateController {
        let mut controller = CameraStateController { state: CameraState::default(), settings, bounds_constraint: None, terrain_follow_report: None, last_settle_path: SettlePath::Hit, planning_zoom: 0.0, drag_zoom: None };
        controller.apply(settings);
        controller
    }

    /// The settings from now on. A configured minimum zoom, minimum pitch or
    /// region holds from the very first frame, not from the first gesture.
    pub fn apply(&mut self, settings: CameraSettings) {
        self.settings = settings;
        self.bounds_constraint = settings.bounds.as_ref().map(CameraBoundsConstraint::new);
        let zoom = settings.clamp_zoom(self.state.zoom);
        self.planning_zoom = settings.clamp_zoom(self.planning_zoom);
        self.state.zoom = zoom;
        self.state.pitch = settings.clamp_pitch(self.state.pitch, zoom);
        self.constrain_center_to_bounds();
    }

    /// The camera state, an immutable value: every change replaces it.
    pub fn camera_state(&self) -> CameraState {
        self.state
    }

    /// How the last move over the terrain was settled. `None` before any, or off the terrain.
    pub fn last_terrain_follow_report(&self) -> Option<TerrainFollowReport> {
        self.terrain_follow_report
    }

    /// The zoom the map is planned by: the zoom of the coverage's target
    /// tiles and the ring rules and of the relief's thresholds. It moves only
    /// when the zoom is set on purpose, and then takes the camera's zoom as
    /// it comes out: a zoom gesture, or a zoom set outright. Over the terrain
    /// the camera's zoom also follows the ground the camera looks at, deeper
    /// up a slope and shallower down one, and backs off over a ridge, and
    /// none of that moves this zoom: a map planned by the zoom of the moment
    /// would reshuffle its rings with every slope a drag crosses, and a
    /// relief faded by it would breathe.
    pub fn planning_zoom(&self) -> f64 {
        self.planning_zoom
    }

    /// Shifts the map centre by the gesture delta. `transition` is the
    /// globe-to-flat phase (0 the globe, 1 the plane). On the sphere a screen
    /// point covers `1 / cos(latitude)` more Mercator than on the plane at
    /// the same camera distance, and the camera's globe proximity moves in
    /// by the same scale: the Mercator delta per point is the proximity
    /// factor over the surface scale.
    ///
    /// Vertically the compensation is full at any zoom. Horizontally, near a
    /// pole a swipe is a rotation around the view axis: at low zooms full
    /// compensation turns it into uncontrollable spinning, so the horizontal
    /// factor is ramped in smoothly by zoom.
    pub fn pan(&mut self, delta_x: f64, delta_y: f64, transition: f64, terrain: Option<&TerrainFollow<'_>>) {
        let yaw = self.state.bearing;
        let zoom = self.state.zoom;
        let latitude = self.state.latitude();
        let scale = surface_scale(latitude, transition);
        let proximity = proximity_distance_factor(latitude, transition, zoom);
        let terrain = terrain.filter(|_| transition >= 1.0);
        let pan_zoom = match terrain {
            Some(_) => *self.drag_zoom.get_or_insert(zoom),
            None => zoom,
        };
        let sensitivity = self.settings.world_pan_sensitivity / 2f64.powf(pan_zoom);
        // forward = rotate((0, 1), -yaw), right = -perpendicular(forward).
        let (sin_yaw, cos_yaw) = ((-yaw).sin(), (-yaw).cos());
        let (forward_x, forward_y) = (-sin_yaw, cos_yaw);
        let (right_x, right_y) = (forward_y, -forward_x);
        let speed = self.settings.world_pan_speed;
        let pan_delta_x = sensitivity * (forward_x * delta_y * speed + right_x * delta_x * speed);
        let pan_delta_y = sensitivity * (forward_y * delta_y * speed + right_y * delta_x * speed);
        let vertical_compensation = proximity / scale;
        let horizontal_compensation = 1.0 + (vertical_compensation - 1.0) * horizontal_compensation_ramp(zoom);
        let center = self.state.center_world_mercator;
        let proposed = [center[0] - 0.5 * pan_delta_x * horizontal_compensation, center[1] - 0.5 * pan_delta_y * vertical_compensation];
        if let (Some(bounds), Some(maximum_stretch_points)) = (&self.bounds_constraint, self.elastic_maximum_stretch_points()) {
            self.state.center_world_mercator = bounds.resist(center, proposed, zoom, maximum_stretch_points * self.world_per_point());
            return;
        }
        if let Some(follow) = terrain {
            let followed = self.follow_terrain(center, proposed, follow);
            self.state.zoom = self.cleared_zoom(followed.zoom, followed.center, follow);
            self.record_terrain_follow(followed.zoom, followed.center, follow);
            self.set_center_world_mercator(followed.center);
            return;
        }
        self.set_center_world_mercator(proposed);
    }

    /// The drag ended: the next one moves the map by the zoom it starts at.
    pub fn end_pan(&mut self) {
        self.drag_zoom = None;
    }

    fn cleared_zoom(&self, zoom: f64, center: [f64; 2], follow: &TerrainFollow<'_>) -> f64 {
        follow.clear_zoom(zoom, center, self.state.pitch, self.state.bearing, (self.settings.minimum_zoom, self.settings.maximum_zoom))
    }

    fn record_terrain_follow(&mut self, followed_zoom: f64, center: [f64; 2], follow: &TerrainFollow<'_>) {
        let arm = eye_distance_metres(followed_zoom, latitude_from_normalized_world_y(center[1]), follow.globe_radius_scale);
        self.terrain_follow_report = Some(TerrainFollowReport { path: self.last_settle_path, arm_metres: arm, cleared_zoom_delta: self.state.zoom - followed_zoom });
    }

    /// A pan over the terrain: the eye moves over the map by the pan, from
    /// over `from` to over `to` as the plain pan would move the point it
    /// looks at, and keeps its altitude. The point it looks at is where its
    /// view ray, pitched as the pose is, meets the terrain from there: the
    /// zoom is the arm to that point, deeper up a slope and shallower down
    /// one, never under the clearance. Where the terrain along the ray is
    /// not known yet, or the ray meets none within reach, the point goes
    /// where the pan puts it and the zoom stays.
    pub fn follow_terrain(&mut self, from: [f64; 2], to: [f64; 2], follow: &TerrainFollow<'_>) -> TerrainFollowResult {
        let Some(height_from) = follow.terrain_metres(from) else {
            return TerrainFollowResult { center: to, zoom: self.state.zoom };
        };
        let pitch = self.state.pitch;
        let latitude = latitude_from_normalized_world_y(from[1]);
        let eye_altitude = height_from + follow.eye_height_metres(self.state.zoom, pitch, latitude);
        let reach = follow.eye_reach_metres(self.state.zoom, pitch, latitude);
        let metres_per_world = EARTH_CIRCUMFERENCE_METERS * latitude.cos().max(1e-6);
        let direction = forward(self.state.bearing);
        let back = reach / metres_per_world;
        self.settle_on_terrain(eye_altitude, [to[0] - direction[0] * back, to[1] - direction[1] * back], reach, to, follow)
    }

    /// A zoom over the terrain, by the same rule as a pan: the eye moves
    /// along its view ray by the zoom, and from where it lands the point is
    /// where the ray meets the terrain. The terrain the ray meets is the
    /// terrain the new zoom reads, which at a zoom crossing is another map:
    /// read where the point was, it stands higher or lower than the old one
    /// did, and were the point kept, the eye would jump with the difference.
    /// The eye stays where the zoom put it, the point moves along the ray by
    /// the difference and the zoom takes it up. Nothing moves when the
    /// terrain under the point reads as before.
    pub fn follow_terrain_zoom(&mut self, previous_zoom: f64, height_before: f64, follow: &TerrainFollow<'_>) -> TerrainFollowResult {
        let center = self.state.center_world_mercator;
        let zoom = self.state.zoom;
        if !follow.terrain_metres(center).is_some_and(|height_after| (height_after - height_before).abs() > 1e-3) {
            return TerrainFollowResult { center, zoom };
        }
        let pitch = self.state.pitch;
        let latitude = latitude_from_normalized_world_y(center[1]);
        let eye_altitude = height_before + follow.eye_height_metres(zoom, pitch, latitude);
        let reach = follow.eye_reach_metres(zoom, pitch, latitude);
        let metres_per_world = EARTH_CIRCUMFERENCE_METERS * latitude.cos().max(1e-6);
        let direction = forward(self.state.bearing);
        let back = reach / metres_per_world;
        // The march steps by a part of the old reach, as the pan's does.
        let old_reach = follow.eye_reach_metres(previous_zoom, pitch, latitude);
        self.settle_on_terrain(eye_altitude, [center[0] - direction[0] * back, center[1] - direction[1] * back], reach.max(old_reach), center, follow)
    }

    /// Where the view ray of an eye `eye_altitude` metres up over
    /// `eye_ground`, pitched and turned as the pose is, meets the terrain,
    /// and the zoom that puts the eye there: the ray is marched a terrain
    /// texel at a time and its first crossing bisected, which settles on any
    /// slope, steeper than the ray included.
    fn settle_on_terrain(&mut self, eye_altitude: f64, eye_ground: [f64; 2], reach: f64, to: [f64; 2], follow: &TerrainFollow<'_>) -> TerrainFollowResult {
        let pitch = self.state.pitch;
        let zoom_range = (self.settings.minimum_zoom.max(0.0), self.settings.maximum_zoom);
        let latitude = latitude_from_normalized_world_y(to[1]);
        let metres_per_world = EARTH_CIRCUMFERENCE_METERS * latitude.cos().max(1e-6);
        let direction = forward(self.state.bearing);
        let tan_pitch = pitch.tan();
        // How far past the ray the terrain is at `r` metres ahead of the eye:
        // the ray at that reach is `r / tan(pitch)` under the eye, and it has
        // met the terrain once r >= (altitude - terrain) * tan. `None` where
        // the terrain is not known.
        let crossing = |r: f64| -> Option<f64> {
            let ahead = r / metres_per_world;
            follow.terrain_metres([eye_ground[0] + direction[0] * ahead, eye_ground[1] + direction[1] * ahead]).map(|height| r - (eye_altitude - height) * tan_pitch)
        };
        // The fallback keeps the arm: the point goes where the pan puts it
        // and the zoom stays, so a frame the ray cannot be settled on moves
        // the camera the plain way and changes nothing else.
        let fallback = TerrainFollowResult { center: to, zoom: self.state.zoom };
        // The eye is over the terrain under it, to within a metre: an eye
        // skimming a crest is a hair over or under it from frame to frame,
        // and the hair must not pick the path.
        let over_ground = crossing(0.0).is_some_and(|start| start <= tan_pitch * EYE_OVER_GROUND_TOLERANCE_METRES);
        if !over_ground {
            self.last_settle_path = SettlePath::Fallback;
            return fallback;
        }
        self.last_settle_path = SettlePath::Hit;
        // The march: one terrain texel at a time, as far as eight times the
        // reach. By the texel, not by the zoom: a step that grew with the
        // reach found a ridge at one zoom and stepped over it at the next. A
        // far reach caps the samples and coarsens the step instead. The march
        // always takes its first sample: a camera looking straight down has
        // no reach.
        let texel_step = (follow.sample_step_world * metres_per_world).max(0.5);
        let farthest = (reach * 8.0).max(texel_step);
        let step = texel_step.max(farthest / MARCH_SAMPLE_LIMIT);
        let mut low_reach = 0.0;
        let mut high_reach: Option<f64> = None;
        let mut r = step;
        while r <= farthest {
            match crossing(r) {
                None => {
                    self.last_settle_path = SettlePath::Fallback;
                    return fallback;
                }
                Some(value) if value >= 0.0 => {
                    high_reach = Some(r);
                    break;
                }
                Some(_) => low_reach = r,
            }
            r += step;
        }
        let Some(mut high) = high_reach else {
            self.last_settle_path = SettlePath::Fallback;
            return fallback;
        };
        // The crossing, to within a tenth of a metre.
        let mut iteration = 0;
        while iteration < 24 && high - low_reach > 0.1 {
            let middle = (low_reach + high) * 0.5;
            match crossing(middle) {
                None => {
                    self.last_settle_path = SettlePath::Fallback;
                    return fallback;
                }
                Some(value) if value >= 0.0 => high = middle,
                Some(_) => low_reach = middle,
            }
            iteration += 1;
        }
        let ahead = high / metres_per_world;
        let center = [eye_ground[0] + direction[0] * ahead, eye_ground[1] + direction[1] * ahead];
        let Some(height_to) = follow.terrain_metres(center) else {
            self.last_settle_path = SettlePath::Fallback;
            return fallback;
        };
        let height = (eye_altitude - height_to).max(follow.clearance_metres);
        TerrainFollowResult { center, zoom: follow.zoom_for_eye_height_metres(height, pitch, latitude_from_normalized_world_y(center[1]), zoom_range) }
    }

    /// One frame of the elastic edge's pull: moves the centre toward the area
    /// by the share of the way the pull covers in `delta_time` at the
    /// centre's distance, and snaps it in once it is within a twentieth of a
    /// screen point. Returns whether the centre is still outside, false at
    /// once for a centre inside the area, a hard edge or no bounds.
    pub fn advance_bounds_pull(&mut self, delta_time: f64) -> bool {
        let (Some(bounds), Some(BoundsEdgeBehavior::Elastic { maximum_stretch, pull_half_life, pull_progression })) = (self.bounds_constraint, self.settings.bounds.map(|bounds| bounds.edge_behavior)) else {
            return false;
        };
        let zoom = self.state.zoom;
        let center = self.state.center_world_mercator;
        let world_per_point = self.world_per_point();
        let distance_points = bounds.distance_outside(center, zoom) / world_per_point;
        let growth = if maximum_stretch > 0.0 && pull_progression > 0.0 { (1.0 + distance_points / maximum_stretch).powf(pull_progression).min(MAXIMUM_PULL_GROWTH) } else { 1.0 };
        let rate = if pull_half_life > 0.0 { (std::f64::consts::LN_2 / pull_half_life) * growth } else { f64::INFINITY };
        let fraction = 1.0 - (-rate * delta_time.max(0.0)).exp();
        let step = bounds.return_step(center, zoom, if fraction.is_finite() { fraction } else { 1.0 });
        if !(step.remaining_distance > world_per_point * 0.05) {
            self.state.center_world_mercator = bounds.apply(center, zoom);
            return false;
        }
        self.state.center_world_mercator = step.center;
        true
    }

    /// How far a screen point of drag moves the centre at the current zoom:
    /// the pan's sensitivity chain before the surface compensation, which is
    /// close enough for the elastic edge's reach and its settling distance.
    fn world_per_point(&self) -> f64 {
        let settings = &self.settings;
        (0.5 * settings.world_pan_sensitivity * settings.world_pan_speed * settings.gesture_pan_translation_scale).abs() / 2f64.powf(self.state.zoom)
    }

    /// The elastic edge's reach in screen points, `None` for a hard edge or no bounds.
    fn elastic_maximum_stretch_points(&self) -> Option<f64> {
        match self.settings.bounds?.edge_behavior {
            BoundsEdgeBehavior::Elastic { maximum_stretch, .. } => Some(if maximum_stretch.is_finite() { maximum_stretch.max(0.0) } else { 0.0 }),
            BoundsEdgeBehavior::Hard => None,
        }
    }

    pub fn set_zoom(&mut self, zoom: f64, terrain: Option<&TerrainFollow<'_>>) {
        self.apply_zoom_delta(zoom - self.state.zoom, terrain);
    }

    pub fn set_center_world_mercator(&mut self, center: [f64; 2]) {
        self.state.center_world_mercator = [wrap_normalized_world_x(center[0]), clamp_normalized_world_y(center[1])];
        self.constrain_center_to_bounds();
    }

    /// The centre moved by a zoom's anchor compensation, the shift that keeps
    /// the world point under the cursor under it: over the terrain the move
    /// is a pan's, so the eye keeps the altitude the zoom gave it, the point
    /// is where its view ray meets the terrain, and the difference goes into
    /// the zoom. Off the terrain it is the plain set.
    pub fn set_center_world_mercator_over_terrain(&mut self, center: [f64; 2], terrain: Option<&TerrainFollow<'_>>) {
        let Some(follow) = terrain else {
            self.set_center_world_mercator(center);
            return;
        };
        let followed = self.follow_terrain(self.state.center_world_mercator, center, follow);
        self.state.zoom = self.cleared_zoom(followed.zoom, followed.center, follow);
        self.set_center_world_mercator(followed.center);
    }

    /// A hard edge holds the centre after every change of the centre or the
    /// zoom, since the area closes in as the zoom grows. An elastic edge
    /// leaves it: the pan resists on its own, and the pull brings the camera
    /// back once nothing moves it.
    fn constrain_center_to_bounds(&mut self) {
        if let (Some(bounds), Some(BoundsEdgeBehavior::Hard)) = (&self.bounds_constraint, self.settings.bounds.map(|bounds| bounds.edge_behavior)) {
            self.state.center_world_mercator = bounds.apply(self.state.center_world_mercator, self.state.zoom);
        }
    }

    fn checked_latitude(latitude_degrees: f64) -> Result<f64, CameraPositionError> {
        if !(latitude_degrees.abs() <= max_mercator_latitude().to_degrees()) {
            return Err(CameraPositionError::LatitudeOutOfRange(latitude_degrees));
        }
        Ok(latitude_degrees / 180.0 * std::f64::consts::PI)
    }

    pub fn set_lat_lon_degrees(&mut self, latitude_degrees: f64, longitude_degrees: f64) -> Result<(), CameraPositionError> {
        if !latitude_degrees.is_finite() || !longitude_degrees.is_finite() {
            return Err(CameraPositionError::NotFinite);
        }
        let latitude = Self::checked_latitude(latitude_degrees)?;
        self.set_center_world_mercator(world_mercator(latitude, longitude_degrees / 180.0 * std::f64::consts::PI));
        Ok(())
    }

    /// The centre's latitude and longitude in degrees.
    pub fn lat_lon_degrees(&self) -> (f64, f64) {
        let center = self.state.center_world_mercator;
        (latitude_from_normalized_world_y(center[1]).to_degrees(), longitude_from_normalized_world_x(center[0]).to_degrees())
    }

    pub fn rotate_yaw(&mut self, delta: f64) {
        self.state.bearing += delta;
    }

    pub fn set_bearing(&mut self, bearing: f64) {
        self.state.bearing = bearing;
    }

    /// The bearing as a constraint leaves it.
    pub fn clamp_bearing(&mut self, constraint: impl Fn(f64) -> f64) {
        self.state.bearing = constraint(self.state.bearing);
    }

    /// The pitch as a constraint leaves it.
    pub fn clamp_pitch(&mut self, constraint: impl Fn(f64) -> f64) {
        self.state.pitch = constraint(self.state.pitch);
    }

    pub fn set_pitch(&mut self, pitch: f64) {
        self.state.pitch = self.settings.clamp_pitch(pitch, self.state.zoom);
    }

    /// A pinch: the zoom the pinch's scale and velocity ask for.
    pub fn zoom_by_scale(&mut self, scale: f64, velocity: f64, terrain: Option<&TerrainFollow<'_>>) {
        self.apply_zoom_delta(pinch_zoom_delta(scale, velocity, &self.settings), terrain);
    }

    pub fn zoom_by_delta(&mut self, delta: f64, terrain: Option<&TerrainFollow<'_>>) {
        self.apply_zoom_delta(delta, terrain);
    }

    /// The camera at a latitude and longitude in degrees, the zoom and pitch within their ranges.
    pub fn set_camera_position(&mut self, latitude_degrees: f64, longitude_degrees: f64, zoom: f64, bearing: f64, pitch: f64) -> Result<(), CameraPositionError> {
        if ![latitude_degrees, longitude_degrees, zoom, bearing, pitch].iter().all(|value| value.is_finite()) {
            return Err(CameraPositionError::NotFinite);
        }
        let latitude = Self::checked_latitude(latitude_degrees)?;
        let zoom = self.settings.clamp_zoom(zoom);
        self.state = CameraState { center_world_mercator: world_mercator(latitude, longitude_degrees / 180.0 * std::f64::consts::PI), zoom, bearing, pitch: self.settings.clamp_pitch(pitch, zoom) };
        self.planning_zoom = zoom;
        self.constrain_center_to_bounds();
        Ok(())
    }

    pub fn set_camera_state(&mut self, state: CameraState) {
        let zoom = self.settings.clamp_zoom(state.zoom);
        self.state = CameraState::new(state.center_world_mercator, zoom, state.bearing, self.settings.clamp_pitch(state.pitch, zoom));
        self.planning_zoom = zoom;
        self.constrain_center_to_bounds();
    }

    fn apply_zoom_delta(&mut self, delta: f64, terrain: Option<&TerrainFollow<'_>>) {
        if !delta.is_finite() {
            return;
        }
        let previous_zoom = self.state.zoom;
        // The terrain under the point as the old zoom reads it, before the
        // new one reads another map.
        let height_before = terrain.and_then(|follow| follow.terrain_metres(self.state.center_world_mercator));
        self.state.zoom = self.settings.clamp_zoom(self.state.zoom + delta);
        if let Some(follow) = terrain {
            if let Some(height_before) = height_before {
                let followed = self.follow_terrain_zoom(previous_zoom, height_before, follow);
                self.state.zoom = followed.zoom;
                self.state.center_world_mercator = followed.center;
            }
            // Over a terrain the zoom stops where the eye would come nearer
            // than the clearance to the ground under it.
            let followed_zoom = self.state.zoom;
            self.state.zoom = self.cleared_zoom(self.state.zoom, self.state.center_world_mercator, follow);
            self.record_terrain_follow(followed_zoom, self.state.center_world_mercator, follow);
        }
        // A zoom on purpose: the map is planned by what the camera sees.
        self.planning_zoom = self.state.zoom;
        self.constrain_center_to_bounds();
    }
}
