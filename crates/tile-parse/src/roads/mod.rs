// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The road path of the parse: the junction keys, the per-layer pre-pass, the
//! street stitcher, the line reader with its dashes and oneway arrows, and
//! the route-sign candidates.

pub mod dash_pattern;
pub mod direction_arrows;
pub mod layer_precomputation;
pub mod line_reader;
pub mod route_shield_placement;
pub mod street_stitcher;

use crate::geom::V2;

/// A road vertex rounded to the tile grid: the key the junction counts and
/// the street stitcher match endpoints by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RoadConnectionPointKey {
    pub x: i32,
    pub y: i32,
}

impl RoadConnectionPointKey {
    pub fn new(point: V2) -> RoadConnectionPointKey {
        RoadConnectionPointKey { x: point.x.round() as i32, y: point.y.round() as i32 }
    }
}
