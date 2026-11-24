// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The map's camera, free of any host: the camera state an app moves
//! (`state`), the one funnel every change of it goes through, with the zoom
//! and pitch ranges, the bounds and the terrain follow (`controller`), the presentation it resolves to, the globe, the plane and the
//! unroll between them (`presentation`), and the render camera of a frame,
//! its eye, matrices and frustum (`render_camera`).
//!
//! No gesture recognizers, no GPU, no tile loading: a host turns its input
//! into a camera state, and reads a frame's camera from here.

// The clamps and the guards keep the forms of the engine this is ported from,
// which pass NaN through or reject it where `clamp` would panic.
#![allow(clippy::manual_clamp, clippy::neg_cmp_op_on_partial_ord)]

pub mod bounds;
pub mod controller;
pub mod presentation;
pub mod render_camera;
pub mod settings;
pub mod state;
pub mod surface_scale;
pub mod terrain;

pub use presentation::{Presentation, PresentationSettings, SurfaceMode};
pub use render_camera::{CameraFrame, RenderCamera};
pub use settings::CameraSettings;
pub use state::CameraState;
