// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The tile parser of ImmersiveMap, ported from the Swift engine's
//! `Tile/Parse` folder: the bytes of one vector tile in, GPU-ready geometry
//! and the labels it carries out. It knows no tile schema: what a feature is
//! comes from the schema's facts and how it draws from the style.
//!
//! Every geometry follows the y-axis contract of [`coordinate_space`]: the
//! parse works in tile space (y south, raw MVT) and enters render space
//! (y north) at exactly one named point per geometry kind.

pub mod buildings;
pub mod coordinate_space;
pub mod geom;
pub mod ground;
pub mod ground_flattening;
pub mod ground_subdivider;
pub mod labels;
pub mod layer_geometry;
pub mod options;
pub mod parse_line;
pub mod parse_polygon;
pub mod parsed_polygon;
pub mod parser;
pub mod prepared_labels;
pub mod reading;
pub mod roads;
pub mod tile;
pub mod trees;
pub mod unification;
