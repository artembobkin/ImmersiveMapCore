// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The camera state's funnel: the ranges, the pan, the bounds and the terrain follow.

use map_camera::bounds::CameraBoundsConstraint;
use map_camera::controller::{CameraPositionError, CameraStateController, SettlePath};
use map_camera::settings::{BoundsEdgeBehavior, CameraBounds, CameraSettings};
use map_camera::terrain::{TerrainFollow, eye_distance_metres};
use map_camera::CameraState;
use map_geo::projection::{latitude_from_normalized_world_y, world_mercator};

fn at(latitude: f64, longitude: f64, zoom: f64, bearing: f64, pitch: f64, settings: CameraSettings) -> CameraStateController {
    let mut controller = CameraStateController::new(settings);
    controller.set_camera_position(latitude, longitude, zoom, bearing, pitch).expect("a position");
    controller
}

#[test]
fn the_ranges_hold_from_the_first_frame() {
    let settings = CameraSettings { minimum_zoom: 4.0, maximum_zoom: 12.0, minimum_pitch: 0.2, ..CameraSettings::default() };
    let controller = CameraStateController::new(settings);
    assert_eq!(controller.camera_state().zoom, 4.0);
    assert_eq!(controller.camera_state().pitch, 0.2);
    assert_eq!(controller.planning_zoom(), 4.0);
    let mut controller = controller;
    controller.set_zoom(99.0, None);
    assert_eq!(controller.camera_state().zoom, 12.0);
    controller.set_pitch(9.0);
    assert_eq!(controller.camera_state().pitch, settings.maximum_pitch);
    controller.zoom_by_delta(f64::NAN, None);
    assert_eq!(controller.camera_state().zoom, 12.0, "a zoom that is no number changes nothing");
    // A range the wrong way round is its upper end.
    assert_eq!(CameraSettings { minimum_zoom: 9.0, maximum_zoom: 3.0, ..settings }.clamp_zoom(5.0), 3.0);
}

#[test]
fn the_pitch_reaches_further_at_high_zoom() {
    let settings = CameraSettings { high_zoom_pitch_extension: 0.2, ..CameraSettings::default() };
    assert_eq!(settings.maximum_reachable_pitch(14.0), settings.maximum_pitch);
    assert!((settings.maximum_reachable_pitch(15.5) - (settings.maximum_pitch + 0.1)).abs() < 1e-12);
    assert!((settings.maximum_reachable_pitch(18.0) - (settings.maximum_pitch + 0.2)).abs() < 1e-12);
    let mut controller = at(40.0, 10.0, 17.0, 0.0, 0.0, settings);
    controller.set_pitch(9.0);
    assert!((controller.camera_state().pitch - (settings.maximum_pitch + 0.2)).abs() < 1e-12);
}

#[test]
fn a_position_past_the_mercator_limit_is_refused() {
    let mut controller = CameraStateController::new(CameraSettings::default());
    assert_eq!(controller.set_camera_position(89.0, 0.0, 3.0, 0.0, 0.0), Err(CameraPositionError::LatitudeOutOfRange(89.0)));
    assert_eq!(controller.set_camera_position(f64::NAN, 0.0, 3.0, 0.0, 0.0), Err(CameraPositionError::NotFinite));
    controller.set_lat_lon_degrees(55.75, 37.62).expect("Moscow");
    let (latitude, longitude) = controller.lat_lon_degrees();
    assert!((latitude - 55.75).abs() < 1e-9 && (longitude - 37.62).abs() < 1e-9);
}

#[test]
fn a_drag_moves_the_map_under_the_finger_along_the_bearing() {
    let mut north_up = at(0.0, 0.0, 10.0, 0.0, 0.0, CameraSettings::default());
    let before = north_up.camera_state().center_world_mercator;
    north_up.pan(100.0, 0.0, 1.0, None);
    let after = north_up.camera_state().center_world_mercator;
    assert!(after[0] < before[0], "dragging right brings the west into view");
    assert!((after[1] - before[1]).abs() < 1e-15);
    // Turned a quarter, the same drag moves the centre along the other axis.
    let mut turned = at(0.0, 0.0, 10.0, std::f64::consts::FRAC_PI_2, 0.0, CameraSettings::default());
    turned.pan(100.0, 0.0, 1.0, None);
    let moved = turned.camera_state().center_world_mercator;
    assert!((moved[0] - before[0]).abs() < 1e-12 && (moved[1] - before[1]).abs() > 1e-9);
    // A level deeper the same drag covers half the world.
    let mut deeper = at(0.0, 0.0, 11.0, 0.0, 0.0, CameraSettings::default());
    deeper.pan(100.0, 0.0, 1.0, None);
    let half = deeper.camera_state().center_world_mercator;
    assert!(((before[0] - half[0]) * 2.0 - (before[0] - after[0])).abs() < 1e-15);
}

#[test]
fn on_the_globe_a_drag_is_compensated_for_the_latitude() {
    // Below the proximity's activation the sphere shows a point of the map 1 / cos wider than the plane.
    let latitude = 60.0f64;
    let mut globe = at(latitude, 0.0, 2.0, 0.0, 0.0, CameraSettings::default());
    let mut plane = at(latitude, 0.0, 2.0, 0.0, 0.0, CameraSettings::default());
    let before = globe.camera_state().center_world_mercator;
    globe.pan(0.0, 0.5, 0.0, None);
    plane.pan(0.0, 0.5, 1.0, None);
    let ratio = (globe.camera_state().center_world_mercator[1] - before[1]) / (plane.camera_state().center_world_mercator[1] - before[1]);
    assert!((ratio - 1.0 / latitude.to_radians().cos()).abs() < 1e-9, "the vertical compensation is full at any zoom");
    // Horizontally there is none this far out: near a pole a swipe is a turn about the view axis.
    let mut sideways_globe = at(latitude, 0.0, 2.0, 0.0, 0.0, CameraSettings::default());
    let mut sideways_plane = at(latitude, 0.0, 2.0, 0.0, 0.0, CameraSettings::default());
    sideways_globe.pan(0.5, 0.0, 0.0, None);
    sideways_plane.pan(0.5, 0.0, 1.0, None);
    assert!((sideways_globe.camera_state().center_world_mercator[0] - sideways_plane.camera_state().center_world_mercator[0]).abs() < 1e-15);
}

fn region(edge_behavior: BoundsEdgeBehavior) -> CameraSettings {
    CameraSettings { bounds: Some(CameraBounds { edge_behavior, ..CameraBounds::new([40.0, 10.0], [50.0, 20.0]) }), ..CameraSettings::default() }
}

#[test]
fn a_hard_edge_holds_the_centre_once_the_area_has_closed_in() {
    let settings = region(BoundsEdgeBehavior::Hard);
    let bounds = CameraBoundsConstraint::new(settings.bounds.as_ref().expect("bounds"));
    assert_eq!(bounds.pull(0.5), 0.0, "the whole world below the pull's zooms");
    assert_eq!(bounds.pull(2.5), 1.0);
    assert!(bounds.pull(1.5) > 0.0 && bounds.pull(1.5) < 1.0);
    let far = world_mercator(0.0, 2.0);
    assert_eq!(bounds.apply(far, 0.5), far, "free while the area is the world");
    let mut controller = at(0.0, 100.0, 5.0, 0.0, 0.0, settings);
    let (latitude, longitude) = controller.lat_lon_degrees();
    assert!((latitude - 40.0).abs() < 1e-9 && (longitude - 20.0).abs() < 1e-9, "brought to the nearest corner");
    controller.pan(-100_000.0, 0.0, 1.0, None);
    assert!((controller.lat_lon_degrees().1 - 20.0).abs() < 1e-9, "and a drag does not leave it");
}

#[test]
fn an_elastic_edge_stretches_and_pulls_back() {
    let settings = region(BoundsEdgeBehavior::DEFAULT_ELASTIC);
    let mut controller = at(45.0, 19.9, 8.0, 0.0, 0.0, settings);
    for _ in 0..200 {
        controller.pan(-40.0, 0.0, 1.0, None);
    }
    let stretched = controller.lat_lon_degrees().1;
    assert!(stretched > 20.0, "the drag goes past the edge");
    let bounds = CameraBoundsConstraint::new(settings.bounds.as_ref().expect("bounds"));
    let outside = bounds.distance_outside(controller.camera_state().center_world_mercator, 8.0);
    let reach = 200.0 * (0.5 * settings.world_pan_sensitivity * settings.world_pan_speed * settings.gesture_pan_translation_scale) / 2f64.powi(8);
    assert!(outside < reach, "but never as far as the stretch's limit");
    let mut frames = 0;
    while controller.advance_bounds_pull(1.0 / 60.0) {
        frames += 1;
        assert!(frames < 600, "the pull settles");
    }
    assert!(frames > 3, "over several frames, not at once");
    assert!((controller.lat_lon_degrees().1 - 20.0).abs() < 1e-9);
    assert!(!at(45.0, 15.0, 8.0, 0.0, 0.0, settings).advance_bounds_pull(1.0 / 60.0), "a centre inside is left alone");
}

/// A plane of ground rising to the north by `slope` metres a metre, `base` metres at the latitude of the origin.
fn slope_heights(origin: [f64; 2], slope: f64, base: f64) -> impl Fn([f64; 2]) -> Option<f64> {
    let metres_per_world = map_geo::projection::EARTH_CIRCUMFERENCE_METERS * latitude_from_normalized_world_y(origin[1]).cos();
    move |point: [f64; 2]| Some(base + (origin[1] - point[1]) * metres_per_world * slope)
}

#[test]
fn over_flat_ground_the_terrain_changes_nothing() {
    let settings = CameraSettings::default();
    let mut plain = at(46.0, 7.7, 13.0, 0.4, 1.0, settings);
    let mut over = at(46.0, 7.7, 13.0, 0.4, 1.0, settings);
    let level = |_: [f64; 2]| Some(1500.0);
    let follow = TerrainFollow { heights: &level, clearance_metres: 30.0, globe_radius_scale: 0.14, sample_step_world: 1e-6 };
    plain.pan(30.0, -20.0, 1.0, None);
    over.pan(30.0, -20.0, 1.0, Some(&follow));
    let (a, b) = (plain.camera_state(), over.camera_state());
    // To within a few metres (1e-7 of the world): the ray is met to a tenth of a metre, and the metres of a
    // degree are taken at the latitude the move starts at, then at the one it ends at.
    assert!((a.center_world_mercator[0] - b.center_world_mercator[0]).abs() < 1e-7 && (a.center_world_mercator[1] - b.center_world_mercator[1]).abs() < 1e-7);
    assert!((a.zoom - b.zoom).abs() < 1e-3);
    assert_eq!(over.last_terrain_follow_report().expect("a report").path, SettlePath::Hit);
}

#[test]
fn a_drag_up_a_slope_keeps_the_eyes_altitude_and_deepens_the_zoom() {
    let settings = CameraSettings::default();
    let mut controller = at(46.0, 7.7, 13.0, 0.0, 1.0, settings);
    let origin = controller.camera_state().center_world_mercator;
    let rising = slope_heights(origin, 0.2, 1000.0);
    let follow = TerrainFollow { heights: &rising, clearance_metres: 30.0, globe_radius_scale: 0.14, sample_step_world: 1e-6 };
    let latitude = latitude_from_normalized_world_y(origin[1]);
    let altitude_before = 1000.0 + eye_distance_metres(13.0, latitude, 0.14) * 1.0f64.cos();
    // Dragging down moves the view north, up the slope.
    for _ in 0..8 {
        controller.pan(0.0, 25.0, 1.0, Some(&follow));
    }
    assert_eq!(controller.last_terrain_follow_report().expect("a report").path, SettlePath::Hit);
    let state = controller.camera_state();
    assert!(state.center_world_mercator[1] < origin[1], "the view went north");
    assert!(state.zoom > 13.0, "the ground came nearer the eye, and the zoom is the arm");
    assert_eq!(controller.planning_zoom(), 13.0, "the map is still planned by the zoom set on purpose");
    let ground = rising(state.center_world_mercator).expect("a height");
    let altitude_after = ground + eye_distance_metres(state.zoom, latitude_from_normalized_world_y(state.center_world_mercator[1]), 0.14) * 1.0f64.cos();
    assert!((altitude_after - altitude_before).abs() < 0.01, "the eye kept its altitude: {altitude_before} then {altitude_after}");
    // Further up in the same drag, which still moves by the zoom it began at, the slope stands over the eye's
    // altitude: the ray cannot be settled, and the move is the plain one.
    for _ in 0..12 {
        controller.pan(0.0, 25.0, 1.0, Some(&follow));
    }
    assert_eq!(controller.last_terrain_follow_report().expect("a report").path, SettlePath::Fallback);
    controller.end_pan();
    // A zoom on purpose brings the planning zoom to what the camera sees.
    controller.zoom_by_delta(-0.5, Some(&follow));
    assert_eq!(controller.planning_zoom(), controller.camera_state().zoom);
}

#[test]
fn unknown_terrain_moves_the_camera_the_plain_way() {
    let settings = CameraSettings::default();
    let mut plain = at(46.0, 7.7, 13.0, 0.3, 0.9, settings);
    let mut over = at(46.0, 7.7, 13.0, 0.3, 0.9, settings);
    let unknown = |_: [f64; 2]| None;
    let follow = TerrainFollow { heights: &unknown, clearance_metres: 30.0, globe_radius_scale: 0.14, sample_step_world: 1e-6 };
    plain.pan(40.0, 10.0, 1.0, None);
    over.pan(40.0, 10.0, 1.0, Some(&follow));
    assert_eq!(plain.camera_state(), over.camera_state());
    // On the globe the terrain is not followed at all.
    let level = |_: [f64; 2]| Some(3000.0);
    let follow = TerrainFollow { heights: &level, clearance_metres: 30.0, globe_radius_scale: 0.14, sample_step_world: 1e-6 };
    let mut globe = at(46.0, 7.7, 4.0, 0.0, 0.0, settings);
    globe.pan(40.0, 10.0, 0.0, Some(&follow));
    assert!(globe.last_terrain_follow_report().is_none());
    let state = CameraState::new([1.25, -3.0], 5.0, 0.1, 0.2);
    globe.set_camera_state(state);
    assert_eq!(globe.camera_state().center_world_mercator, [0.25, 0.0], "a state set outright is wrapped and clamped into the world");
}

#[test]
fn the_zoom_stops_at_the_clearance_over_a_ridge() {
    let settings = CameraSettings::default();
    let mut controller = at(46.0, 7.7, 12.0, 0.0, 1.2, settings);
    let center = controller.camera_state().center_world_mercator;
    // A wall behind the camera: the ground under the eye stands 4 km over the point looked at.
    let wall = move |point: [f64; 2]| Some(if point[1] > center[1] + 1e-7 { 4000.0 } else { 0.0 });
    let follow = TerrainFollow { heights: &wall, clearance_metres: 50.0, globe_radius_scale: 0.14, sample_step_world: 1e-6 };
    controller.zoom_by_delta(6.0, Some(&follow));
    let state = controller.camera_state();
    assert!(state.zoom < 18.0, "the zoom stopped short of what was asked");
    let latitude = latitude_from_normalized_world_y(state.center_world_mercator[1]);
    assert!(follow.eye_height_metres(state.zoom, state.pitch, latitude) >= 4000.0, "with the eye over the wall");
    assert!(follow.is_clear(state.center_world_mercator, state.zoom, state.pitch, state.bearing));
    assert!(controller.last_terrain_follow_report().expect("a report").cleared_zoom_delta <= 0.0);
}
