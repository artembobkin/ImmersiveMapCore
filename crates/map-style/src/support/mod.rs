// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Stand-ins for the engine types from outside `Schema`, `Style` and
//! `VectorTileAdaptation` that those folders use, ported only as far as
//! they need them. Each names the Swift type it stands for, so the
//! integrator can move it to the crate that will own it.
//!
//! - `Tile` stands for `Tile/Tile.swift`, the identity of a tile.
//! - `ZoomCurve` stands for `Configuration/ImmersiveMapZoomCurve.swift`.
//! - `LabelLanguage` and `LabelFallbackPolicy` stand for the label
//!   settings of `Configuration/ImmersiveMapSettings.swift`.
//! - `ScreenScale` stands for `Utils/ScreenScale.swift`.
//! - `line_baking` holds the constants of `Tile/Parse/ParseLine.swift` and
//!   `Tile/Parse/FeatureStyle+Parsing.swift` that turn a stroke into its
//!   GPU line style (`TileLineStyle::from_pass`).

pub mod line_baking;
mod screen_scale;
mod settings;
mod tile;
mod zoom_curve;

pub use screen_scale::ScreenScale;
pub use settings::{LabelFallbackPolicy, LabelLanguage};
pub use tile::Tile;
pub use zoom_curve::{ZoomCurve, ZoomCurveInterpolation, ZoomCurveStop};
