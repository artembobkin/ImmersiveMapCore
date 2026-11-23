// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The two halves of a map style, ported from ImmersiveMap's `Schema`,
//! `Style` and `VectorTileAdaptation` folders.
//!
//! - [`schema`] says what a feature is and answers with facts
//!   ([`schema::FeatureFacts`]): a road with a structure, a layer and a
//!   name, a building with its heights, a point with a name in several
//!   languages. The built-in reading of the Protomaps basemap is in
//!   [`schema::default`].
//! - [`style`] says only how a feature draws ([`style::FeatureStyle`]),
//!   reading the facts from its context. Every drawing parameter lives
//!   here: a road's width, level, class priority, every decoration's
//!   dimensions, a colour, a cap, a join. The engine decides nothing of
//!   its own. The built-in style and its theme are in [`style::default`].
//! - [`adaptation`] is the label policy a tile parse asks questions of:
//!   which spelling of a name the map's language and the text atlas
//!   allow, how a label is identified and ranked.
//!
//! The schema and the style are kept apart: schema-specific logic never
//! reaches the renderer, the tiles or the labels, which consume only
//! normalized data. A style's and a schema's fingerprints are part of the
//! prepared-tile disk cache identity: a change to what a configuration
//! means must change its fingerprint.
//!
//! [`support`] holds the minimal stand-ins of engine types from outside the
//! three folders that they use (the tile identity, the zoom curve, the
//! label language settings, the screen scale and the line baking
//! constants), to be moved by the integrator.

pub mod adaptation;
pub mod schema;
pub mod style;
pub mod support;

/// A colour or a vector of three floats (`SIMD3<Float>` in the engine).
pub type Float3 = [f32; 3];
/// A colour or a vector of four floats (`SIMD4<Float>` in the engine).
pub type Float4 = [f32; 4];
