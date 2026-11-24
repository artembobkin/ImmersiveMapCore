// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The frame's view: from the camera state an app moves to what the frame
//! draws by, in one call. The presentation (the globe, the plane, the
//! unroll between them), the render camera with its matrices and frustum,
//! and the tile coverage, each fed by the one before, so a host that calls
//! across a binding crosses it once a frame for all three.

use map_camera::{CameraFrame, CameraState, Presentation, PresentationSettings, SurfaceMode};
use map_visibility::culling::{TileCulling, VisibleContent};
use map_visibility::flat::FlatCoverageInput;
use map_visibility::globe::{GlobeCoverageInput, center_in_tiles, look_at_tile};
use map_visibility::rule_sets::{RingRuleSets, target_zoom};
use map_visibility::rules::FlatRingRule;

/// The zoom of the world cover a host keeps resident, which no band of the plane goes down to.
pub const FLAT_BACKDROP_ZOOM: i32 = 0;

/// What the view reads of the map's settings.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewSettings {
    pub presentation: PresentationSettings,
    /// The deepest zoom of the tileset.
    pub maximum_zoom_level: i32,
    /// How far past the visible ground the plane's texture bands are placed, as a share of the footprint, 0 for none.
    pub raster_band_margin_share: f64,
    pub rule_sets: RingRuleSets,
}

/// What a frame's view is resolved from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewInput {
    pub camera_state: CameraState,
    /// The zoom the map is planned by, not the camera's: over a terrain the
    /// camera's zoom follows the ground under the point it looks at, and a
    /// coverage planned by it would reshuffle its rings with every slope a
    /// drag crosses.
    pub planning_zoom: f64,
    /// The drawable's size in device pixels.
    pub draw_width: f64,
    pub draw_height: f64,
    /// A debug switch that wins over the zoom's choice of surface.
    pub forced_surface: Option<SurfaceMode>,
    /// How far the ground relief falls and rises about the plane, in world units, `None` off the relief.
    pub relief_heights: Option<(f64, f64)>,
}

/// A frame's view. The coverage is the resolver's own, valid until its next frame.
#[derive(Debug, Clone, Copy)]
pub struct View<'a> {
    pub presentation: Presentation,
    pub camera: CameraFrame,
    /// The zoom the coverage targets.
    pub target_zoom: i32,
    /// The point the camera looks at in fractional tiles of the target zoom: x east, y south.
    pub center_in_tiles: [f64; 2],
    pub content: &'a VisibleContent,
}

/// Everything the coverage is a function of, as the last frame resolved it.
#[derive(Debug, Clone, PartialEq)]
struct Fingerprint {
    camera_state: CameraState,
    draw_width: f64,
    draw_height: f64,
    target_zoom: i32,
    flat: bool,
    globe_transition: f64,
    globe_radius: f64,
    relief_heights: Option<(f64, f64)>,
    raster_band_margin_share: f64,
    rules: Vec<FlatRingRule>,
}

/// Resolves the view of frame after frame. The coverage is a pure function
/// of the camera pose, the drawable's size, the presentation and the rules:
/// with all of them as the last frame had them, the last coverage is handed
/// back, with its version, which a host's working set gates on.
#[derive(Debug, Default)]
pub struct FrameView {
    culling: TileCulling,
    fingerprint: Option<Fingerprint>,
    content: VisibleContent,
}

impl FrameView {
    pub fn new() -> FrameView {
        FrameView::default()
    }

    /// The frame's view, `None` for an empty drawable.
    pub fn resolve(&mut self, settings: &ViewSettings, input: &ViewInput) -> Option<View<'_>> {
        let presentation = Presentation::resolve(&input.camera_state, &settings.presentation, input.forced_surface);
        let camera = CameraFrame::resolve(&input.camera_state, presentation.transition, input.draw_width, input.draw_height)?;
        let target_zoom = target_zoom(input.planning_zoom, settings.maximum_zoom_level);
        // The frame is drawn by the rules of the set its planning zoom falls
        // in. Not the target zoom: that stops at the tileset's deepest zoom,
        // and a set starting past it would never be reached.
        let rules = settings.rule_sets.rules_for_zoom(RingRuleSets::rule_set_zoom(input.planning_zoom));
        let flat = presentation.surface_mode == SurfaceMode::Flat;
        let center = center_in_tiles(input.camera_state.center_world_mercator, target_zoom);
        let unchanged = self.fingerprint.as_ref().is_some_and(|print| {
            print.camera_state == input.camera_state
                && print.draw_width == input.draw_width
                && print.draw_height == input.draw_height
                && print.target_zoom == target_zoom
                && print.flat == flat
                && print.globe_transition == presentation.globe.transition
                && print.globe_radius == presentation.globe.radius
                && print.relief_heights == input.relief_heights
                && print.raster_band_margin_share == settings.raster_band_margin_share
                && print.rules == rules
        });
        if !unchanged {
            self.content = if flat {
                self.culling.resolve_flat(&FlatCoverageInput {
                    camera_matrix: &camera.projection_view,
                    relief_heights: input.relief_heights,
                    eye: [camera.eye[0], camera.eye[1]],
                    raster_band_margin_share: settings.raster_band_margin_share,
                    flat: presentation.flat,
                    target_zoom,
                    backdrop_zoom: Some(FLAT_BACKDROP_ZOOM),
                    rules,
                })
            } else {
                self.culling.resolve_globe(&GlobeCoverageInput { eye: camera.eye, globe: presentation.globe, rules, look_at: look_at_tile(center, target_zoom), target_zoom, frustum: &camera.frustum })
            };
            self.fingerprint = Some(Fingerprint {
                camera_state: input.camera_state,
                draw_width: input.draw_width,
                draw_height: input.draw_height,
                target_zoom,
                flat,
                globe_transition: presentation.globe.transition,
                globe_radius: presentation.globe.radius,
                relief_heights: input.relief_heights,
                raster_band_margin_share: settings.raster_band_margin_share,
                rules: rules.to_vec(),
            });
        }
        Some(View { presentation, camera, target_zoom, center_in_tiles: center, content: &self.content })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use map_geo::projection::world_mercator;

    fn settings() -> ViewSettings {
        ViewSettings { presentation: PresentationSettings::default(), maximum_zoom_level: 15, raster_band_margin_share: 0.0, rule_sets: RingRuleSets::default() }
    }

    fn input(zoom: f64, pitch_degrees: f64) -> ViewInput {
        let center = world_mercator(40.7f64.to_radians(), (-74.0f64).to_radians());
        ViewInput { camera_state: CameraState::new(center, zoom, 0.3, pitch_degrees.to_radians()), planning_zoom: zoom, draw_width: 1024.0, draw_height: 768.0, forced_surface: None, relief_heights: None }
    }

    #[test]
    fn a_street_frame_is_the_plane_with_its_bands() {
        let mut frame_view = FrameView::new();
        let view = frame_view.resolve(&settings(), &input(15.4, 60.0)).expect("a view");
        assert_eq!(view.presentation.surface_mode, SurfaceMode::Flat);
        assert_eq!(view.target_zoom, 15);
        assert!(view.content.targets.iter().any(|target| target.tile.z == 15), "the exact tiles under the camera");
        assert!(view.content.targets.iter().any(|target| target.tile.z < 15 && target.raster_size.is_some()), "the far bands as textures");
        assert_eq!(view.content.bands.len(), 3, "the near street set's three rules");
        assert!(view.content.targets.iter().all(|target| target.tile.z > FLAT_BACKDROP_ZOOM));
    }

    #[test]
    fn a_planet_frame_is_the_globe() {
        let mut frame_view = FrameView::new();
        let view = frame_view.resolve(&settings(), &input(3.2, 0.0)).expect("a view");
        assert_eq!(view.presentation.surface_mode, SurfaceMode::Spherical);
        assert_eq!(view.target_zoom, 3);
        assert!(view.content.targets.iter().all(|target| target.tile.wrap == 0));
        assert!(view.content.targets.iter().any(|target| target.tile.z == 3));
        assert!(view.content.targets.iter().any(|target| target.tile.z == 0), "the world cover past the last rule");
    }

    #[test]
    fn the_coverage_is_kept_while_the_frame_is_the_same() {
        let mut frame_view = FrameView::new();
        let settings = settings();
        let first = frame_view.resolve(&settings, &input(15.4, 60.0)).expect("a view").content.clone();
        let again = frame_view.resolve(&settings, &input(15.4, 60.0)).expect("a view").content.clone();
        assert_eq!(again, first);
        let turned = ViewInput { camera_state: CameraState { bearing: 2.0, ..input(15.4, 60.0).camera_state }, ..input(15.4, 60.0) };
        let other = frame_view.resolve(&settings, &turned).expect("a view").content.clone();
        assert_ne!(other.targets, first.targets);
        assert_ne!(other.coverage_version, first.coverage_version);
        // The deepest zoom of the tileset caps the target zoom, and the rule set is still the camera zoom's.
        let deep = frame_view.resolve(&settings, &input(18.3, 30.0)).expect("a view");
        assert_eq!(deep.target_zoom, 15);
        assert_eq!(deep.content.bands.len(), 2, "the closest street set's two rules");
        assert!(frame_view.resolve(&settings, &ViewInput { draw_width: 0.0, ..input(15.4, 60.0) }).is_none());
    }
}
