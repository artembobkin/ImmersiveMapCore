// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The render camera of a frame: the eye, the up vector, the projection,
//! the view and their product, and the frustum of that product.
//!
//! The render world is centred on the point the camera looks at (the
//! origin), the map lies in the z = 0 plane (the globe's sphere touches it at
//! the origin), and the eye sits on the arm `1 - 0.5 frac(zoom)` units long,
//! times the globe proximity, tilted by the pitch about x and turned by the
//! bearing about z.

use std::f64::consts::PI;

use map_geo::frustum::Frustum;
use map_geo::matrix::{self, Mat4};

use crate::state::CameraState;
use crate::surface_scale::proximity_distance_factor;

/// One style rank step of the ground's depth bands.
const LAYER_DEPTH_STEP: f64 = 3.2e-6;
/// The rank of the buildings, the models and the trees: nearer than every ground layer and every label painted on the ground.
const REAL_GEOMETRY_RANK: f64 = 1_536.0;

/// The constants of the render camera.
pub struct RenderCamera;

impl RenderCamera {
    /// The fixed vertical field of view.
    pub const VERTICAL_FOV_RADIANS: f64 = PI / 4.0;
    /// The near plane, in view units.
    pub const NEAR_PLANE: f64 = 0.01;
    /// The far plane of the matrices the CPU works with: the coverage, the
    /// horizon and the picking unproject through them, and they want a far
    /// plane close to the visible ground. The GPU draws with a far plane far
    /// past it (`GPU_FAR_PLANE`).
    pub const FAR_PLANE: f64 = 200.0;
    /// The far plane the GPU draws with: the flat ground's depth is the
    /// projection's, so the ground ends at the far plane, and at 200 units
    /// it ended a visible band under the horizon.
    pub const GPU_FAR_PLANE: f64 = 100_000.0;
    /// The scale the camera uniform the GPU draws with applies to the
    /// projection's z, for everything real (the buildings, the models, the
    /// markers), so a wall's base is never covered by the ground behind it.
    pub const REAL_DEPTH_SCALE: f64 = 1.0 - REAL_GEOMETRY_RANK * LAYER_DEPTH_STEP;

    /// The map from the CPU projection's clip z to the GPU's: the far plane
    /// moved to `GPU_FAR_PLANE` and the z scaled by `REAL_DEPTH_SCALE`, both
    /// linear in clip space, so one matrix applied after the projection-view
    /// does it and x, y and w are untouched.
    pub fn gpu_clip_depth_adjustment() -> Mat4 {
        let z_row = |near: f64, far: f64| (-(far + near) / (far - near), (-2.0 * far * near) / (far - near));
        let cpu = z_row(Self::NEAR_PLANE, Self::FAR_PLANE);
        let gpu = z_row(Self::NEAR_PLANE, Self::GPU_FAR_PLANE);
        // The perspective's z_clip = zScale z_view + wzScale, and w_clip =
        // -z_view, so z_clip = -zScale w_clip + wzScale on both sides.
        let a = gpu.0 / cpu.0;
        let b = gpu.1 - a * cpu.1;
        let mut adjustment = matrix::IDENTITY;
        adjustment[10] = a * Self::REAL_DEPTH_SCALE;
        adjustment[14] = b * Self::REAL_DEPTH_SCALE;
        adjustment
    }

    /// The eye and the up vector of a camera state. `transition` is the
    /// frame's globe-to-plane phase: on the globe the camera moves in by the
    /// view centre's surface scale, and the phase carries that factor
    /// through the morph.
    pub fn pose(state: &CameraState, transition: f64) -> ([f64; 3], [f64; 3]) {
        let z_remains = state.zoom % 1.0;
        let distance = (1.0 - z_remains * 0.5) * proximity_distance_factor(state.latitude(), transition, state.zoom);
        // The pitch about x first, then the yaw about z, applied to the
        // camera position (0, 0, distance) and the up (0, 1, 0).
        let (sin_pitch, cos_pitch) = (state.pitch.sin(), state.pitch.cos());
        let (sin_yaw, cos_yaw) = (state.bearing.sin(), state.bearing.cos());
        let position_y = -distance * sin_pitch;
        let position_z = distance * cos_pitch;
        ([-position_y * sin_yaw, position_y * cos_yaw, position_z], [-cos_pitch * sin_yaw, cos_pitch * cos_yaw, sin_pitch])
    }
}

/// The camera of one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraFrame {
    /// The drawable's size in device pixels.
    pub draw_width: f64,
    pub draw_height: f64,
    pub projection: Mat4,
    pub view: Mat4,
    /// `projection * view`, the CPU form (far plane `RenderCamera::FAR_PLANE`).
    pub projection_view: Mat4,
    /// The drawable's pixel orthographic matrix: origin bottom left, y up, z in -1...1.
    pub screen: Mat4,
    /// The eye in render world units (the view centre at the origin).
    pub eye: [f64; 3],
    pub up: [f64; 3],
    pub frustum: Frustum,
}

impl CameraFrame {
    /// The frame's camera for a drawable of `draw_width` by `draw_height`
    /// device pixels, `None` for an empty drawable.
    pub fn resolve(state: &CameraState, transition: f64, draw_width: f64, draw_height: f64) -> Option<CameraFrame> {
        if !(draw_width > 0.0) || !(draw_height > 0.0) {
            return None;
        }
        let projection = matrix::perspective(RenderCamera::VERTICAL_FOV_RADIANS, draw_width / draw_height, RenderCamera::NEAR_PLANE, RenderCamera::FAR_PLANE);
        let (eye, up) = RenderCamera::pose(state, transition);
        let view = matrix::look_at(eye, [0.0, 0.0, 0.0], up);
        let projection_view = matrix::multiply(&projection, &view);
        Some(CameraFrame {
            draw_width,
            draw_height,
            projection,
            view,
            projection_view,
            screen: matrix::orthographic(0.0, draw_width, 0.0, draw_height, -1.0, 1.0),
            eye,
            up,
            frustum: Frustum::new(&projection_view),
        })
    }
}
