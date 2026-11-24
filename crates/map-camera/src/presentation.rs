// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The resolved presentation of a camera state: the globe, the plane and the
//! transition between them, which the camera, the tiles and the renderer
//! share. Both surfaces' render states are always produced, and the shaders
//! morph between sphere and plane by the transition.

use std::f64::consts::PI;

use map_geo::projection::{flat_pan, globe_pan};
use map_geo::{FlatRenderState, GlobeUniform};

use crate::state::CameraState;

/// Fraction of the transition phase by which the morph geometry fully unfolds into the plane.
pub const GEOMETRY_COMPLETION_PHASE: f64 = 0.9;

/// The presentation group of the map's settings: where the globe unrolls
/// into the plane and how large the render world is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresentationSettings {
    /// The zoom the automatic globe-to-plane transition starts at.
    pub automatic_transition_start_zoom: f64,
    /// The zoom levels the transition takes, from its start to the finished plane.
    pub automatic_transition_span: f64,
    /// The globe's render radius at zoom 0, in render units. The render world
    /// is renormalized per integer zoom: at zoom z the radius is this times
    /// `2^floor(z)` and the flat map is `2 pi` radii across.
    pub globe_radius_scale: f64,
    /// Whether the map is a globe at low zoom. Off, the world is the Mercator plane at every zoom.
    pub is_globe_enabled: bool,
    /// Where in the unroll, as a fraction of the transition window, the globe's air starts giving way to the plane's.
    pub air_handover_start: f64,
    /// Where the handover is complete, at or past `air_handover_start`.
    pub air_handover_end: f64,
}

impl Default for PresentationSettings {
    fn default() -> PresentationSettings {
        PresentationSettings { automatic_transition_start_zoom: 6.0, automatic_transition_span: 1.0, globe_radius_scale: 0.14, is_globe_enabled: true, air_handover_start: 0.7, air_handover_end: 0.9 }
    }
}

impl PresentationSettings {
    /// How far the globe's air has given way to the plane's at a point of
    /// the transition, 0...1: 0 up to the start, a smoothstep to 1 at the
    /// end. A window of no width is a step at its start.
    pub fn air_handover(&self, transition: f64) -> f64 {
        let start = self.air_handover_start.max(0.0).min(1.0);
        let end = self.air_handover_end.max(start).min(1.0);
        let t = ((transition - start) / (end - start).max(1e-6)).max(0.0).min(1.0);
        t * t * (3.0 - 2.0 * t)
    }
}

/// The render surface: the sphere (with the unroll toward the plane) or the finished plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceMode {
    Spherical,
    Flat,
}

/// The presentation of one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Presentation {
    pub camera_state: CameraState,
    /// The semantic globe-to-plane transition, 0 the globe and 1 the plane:
    /// fades, fog and the surface choice read it. The geometry runs slightly
    /// ahead of it (`geometry_transition`).
    pub transition: f64,
    /// `2^floor(zoom)`: the render world's scale at the frame's integer zoom.
    pub zoom_scale: f64,
    /// `globe_radius_scale * zoom_scale`, in render units.
    pub globe_render_radius: f64,
    pub surface_mode: SurfaceMode,
    pub globe: GlobeUniform,
    pub flat: FlatRenderState,
}

/// Transition for shader geometry (`GlobeUniform`): the unfurl completes by
/// `GEOMETRY_COMPLETION_PHASE`, and for the rest of the phase the globe path
/// renders an already finished plane, so the surface switch at 1 happens
/// between geometrically identical frames.
pub fn geometry_transition(transition: f64) -> f64 {
    (transition.max(0.0) / GEOMETRY_COMPLETION_PHASE).min(1.0)
}

impl Presentation {
    /// The presentation of a camera state. `forced` (a debug switch) wins
    /// over the zoom. Otherwise the zoom decides, unless the globe is
    /// switched off, which is the plane at every zoom.
    pub fn resolve(camera_state: &CameraState, settings: &PresentationSettings, forced: Option<SurfaceMode>) -> Presentation {
        let zoom_scale = 2f64.powf(camera_state.zoom.floor());
        let span = settings.automatic_transition_span.max(f64::from_bits(1));
        let automatic = ((camera_state.zoom - settings.automatic_transition_start_zoom) / span).min(1.0).max(0.0);
        let transition = match forced {
            None => {
                if settings.is_globe_enabled {
                    automatic
                } else {
                    1.0
                }
            }
            Some(SurfaceMode::Spherical) => 0.0,
            Some(SurfaceMode::Flat) => 1.0,
        };
        let globe_render_radius = settings.globe_radius_scale * zoom_scale;
        let pan = globe_pan(camera_state.center_world_mercator);
        Presentation {
            camera_state: *camera_state,
            transition,
            zoom_scale,
            globe_render_radius,
            surface_mode: if transition >= 1.0 { SurfaceMode::Flat } else { SurfaceMode::Spherical },
            globe: GlobeUniform { pan_x: pan[0], pan_y: pan[1], radius: globe_render_radius, transition: geometry_transition(transition) },
            flat: FlatRenderState { pan: flat_pan(camera_state.center_world_mercator), render_map_size: 2.0 * PI * globe_render_radius },
        }
    }
}
