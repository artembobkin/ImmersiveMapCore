// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The camera against the web engine's: every pose recorded from it
//! (`map-visibility/tests/support/camera_fixtures.txt`, whose comment in
//! `support/mod.rs` states the format) is resolved here and compared, matrix
//! entry by entry. The sines and cosines of the two engines differ in the
//! last bit, so the comparison allows a few units of it.

use map_camera::{CameraFrame, CameraState, Presentation, PresentationSettings, SurfaceMode};
use map_geo::projection::world_mercator;

const FIXTURES: &str = include_str!("../../map-visibility/tests/support/camera_fixtures.txt");

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-12 * a.abs().max(b.abs()).max(1.0)
}

/// The recorded lines of a kind: the zoom, the pitch, the bearing, then the matrix, the pan and the map size.
fn recorded(kind: &str) -> Vec<Vec<f64>> {
    FIXTURES.lines().filter(|line| line.starts_with(kind)).map(|line| line.split(' ').skip(1).map(|field| field.parse().expect("a number")).collect()).collect()
}

fn assert_matches(frame: &CameraFrame, presentation: &Presentation, numbers: &[f64], what: &str) {
    for (index, (computed, expected)) in frame.projection_view.iter().zip(&numbers[3..19]).enumerate() {
        assert!(close(*computed, *expected), "{what}: matrix entry {index} is {computed}, the web engine's {expected}");
    }
    assert!(close(presentation.flat.pan[0], numbers[19]) && close(presentation.flat.pan[1], numbers[20]), "{what}: the pan");
    assert!(close(presentation.flat.render_map_size, numbers[21]), "{what}: the map size");
}

#[test]
fn the_flat_camera_is_the_web_engines() {
    let center = world_mercator(40.7f64.to_radians(), (-74.0f64).to_radians());
    let poses = recorded("flat");
    assert!(poses.len() > 100);
    for numbers in &poses {
        let state = CameraState::new(center, numbers[0], numbers[2].to_radians(), numbers[1].to_radians());
        let presentation = Presentation::resolve(&state, &PresentationSettings::default(), Some(SurfaceMode::Flat));
        assert_eq!(presentation.surface_mode, SurfaceMode::Flat);
        let frame = CameraFrame::resolve(&state, presentation.transition, 4.0, 3.0).expect("a frame");
        assert_matches(&frame, &presentation, numbers, &format!("zoom {} pitch {} bearing {}", numbers[0], numbers[1], numbers[2]));
    }
}

#[test]
fn the_camera_over_the_alps_is_the_web_engines() {
    let numbers = &recorded("relief")[0];
    let state = CameraState::new(world_mercator(46.0f64.to_radians(), 7.7f64.to_radians()), 12.5, 0.0, 1.1);
    let presentation = Presentation::resolve(&state, &PresentationSettings::default(), Some(SurfaceMode::Flat));
    let frame = CameraFrame::resolve(&state, presentation.transition, 4.0, 3.0).expect("a frame");
    assert_matches(&frame, &presentation, numbers, "the Alps");
}

#[test]
fn the_frame_camera_resolves_its_own_transition() {
    // The web engine's frame camera at zoom 9 straight down, 1024 by 768, the surface left to the zoom.
    let numbers = &recorded("frame")[0];
    let state = CameraState::new(world_mercator(40.7f64.to_radians(), (-74.0f64).to_radians()), 9.0, 0.0, 0.0);
    let presentation = Presentation::resolve(&state, &PresentationSettings::default(), None);
    assert_eq!(presentation.transition, 1.0);
    let frame = CameraFrame::resolve(&state, presentation.transition, 1024.0, 768.0).expect("a frame");
    assert_matches(&frame, &presentation, numbers, "the frame camera");
    assert!(CameraFrame::resolve(&state, 1.0, 0.0, 768.0).is_none(), "an empty drawable has no camera");
}

#[test]
fn the_globe_unrolls_into_the_plane_over_the_transition_window() {
    let settings = PresentationSettings::default();
    let at = |zoom: f64| Presentation::resolve(&CameraState::new([0.5, 0.4], zoom, 0.0, 0.0), &settings, None);
    assert_eq!((at(3.0).transition, at(3.0).surface_mode), (0.0, SurfaceMode::Spherical));
    assert_eq!((at(6.5).transition, at(6.5).surface_mode), (0.5, SurfaceMode::Spherical));
    assert_eq!((at(7.0).transition, at(7.0).surface_mode), (1.0, SurfaceMode::Flat));
    // The geometry is a finished plane before the surface switches.
    assert_eq!(at(6.95).globe.transition, 1.0);
    assert!(at(6.5).globe.transition > 0.5);
    // The render world doubles with every whole zoom.
    assert_eq!(at(5.9).globe_render_radius * 2.0, at(6.1).globe_render_radius);
    // Without the globe the world is the plane at every zoom, and a forced surface wins over both.
    let plane_only = PresentationSettings { is_globe_enabled: false, ..settings };
    let state = CameraState::new([0.5, 0.4], 2.0, 0.0, 0.0);
    assert_eq!(Presentation::resolve(&state, &plane_only, None).surface_mode, SurfaceMode::Flat);
    assert_eq!(Presentation::resolve(&state, &plane_only, Some(SurfaceMode::Spherical)).transition, 0.0);
}

#[test]
fn the_camera_moves_in_on_the_globe_toward_a_pole() {
    let high = CameraState::new(world_mercator(70.0f64.to_radians(), 0.0), 5.0, 0.0, 0.0);
    let on_globe = CameraFrame::resolve(&high, 0.0, 4.0, 3.0).expect("a frame");
    let on_plane = CameraFrame::resolve(&high, 1.0, 4.0, 3.0).expect("a frame");
    assert!(close(on_plane.eye[2], 1.0));
    assert!(close(on_globe.eye[2], 70.0f64.to_radians().cos()), "by the cosine of the latitude");
    // Below the activation zoom the whole globe is in view and the camera stays out.
    let planet = CameraState::new(high.center_world_mercator, 2.0, 0.0, 0.0);
    assert!(close(CameraFrame::resolve(&planet, 0.0, 4.0, 3.0).expect("a frame").eye[2], 1.0));
}
