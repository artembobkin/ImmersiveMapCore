// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The cameras the tests look through. `camera_fixtures.txt` holds
//! projection-view matrices recorded from the web engine's camera
//! (`RenderCamera` posed by `RenderCameraPoseResolver` over the flat world
//! of `PresentationStateResolver`), one per line: a kind, the zoom, the
//! pitch and the bearing in degrees, the sixteen doubles of the matrix,
//! then the flat world's pan and size. Until the camera itself is in this
//! workspace, a new pose is recorded there and added to the file.
#![allow(dead_code)]

use map_visibility::flat::Square;
use map_visibility::polygon::{CoveragePolygon, footprint};
use map_visibility::{FlatRenderState, VisibleTile};

const FIXTURES: &str = include_str!("camera_fixtures.txt");

pub struct Fixture {
    pub matrix: [f64; 16],
    pub flat: FlatRenderState,
    pub polygon: CoveragePolygon,
}

/// The recorded matrix of a pose, with no footprint asked of it.
pub fn matrix(kind: &str, zoom: f64, pitch: f64, bearing: f64) -> ([f64; 16], FlatRenderState) {
    for line in FIXTURES.lines() {
        let mut fields = line.split(' ');
        if fields.next() != Some(kind) {
            continue;
        }
        let numbers: Vec<f64> = fields.map(|field| field.parse().expect("a number")).collect();
        if numbers[0] == zoom && numbers[1] == pitch && numbers[2] == bearing {
            let matrix: [f64; 16] = numbers[3..19].try_into().expect("sixteen doubles");
            return (matrix, FlatRenderState { pan: [numbers[19], numbers[20]], render_map_size: numbers[21] });
        }
    }
    panic!("no recorded camera for {kind} zoom {zoom} pitch {pitch} bearing {bearing}");
}

/// The engine's camera over lower Manhattan at a zoom, pitched and turned, with its footprint on the plane.
pub fn fixture(zoom: f64, pitch: f64, bearing: f64) -> Fixture {
    let (matrix, flat) = matrix("flat", zoom, pitch, bearing);
    let polygon = footprint(&matrix, None).expect("the footprint exists");
    Fixture { matrix, flat, polygon }
}

pub fn tile(x: i32, y: i32, z: i32) -> VisibleTile {
    VisibleTile { z, x, y, wrap: 0 }
}

/// The tile's square in the flat world.
pub fn rect(tile: VisibleTile, flat: &FlatRenderState) -> Square {
    let origin = flat.tile_origin_and_size(tile.x, tile.y, tile.z, tile.wrap);
    Square { min_x: origin[0], min_y: origin[1], max_x: origin[0] + origin[2], max_y: origin[1] + origin[2] }
}

/// Whether the two rectangles share ground, more than an edge.
pub fn overlap(a: Square, b: Square) -> bool {
    let slack = 1e-4;
    a.min_x < b.max_x - slack && a.max_x > b.min_x + slack && a.min_y < b.max_y - slack && a.max_y > b.min_y + slack
}

/// Whether `a` lies within `b`.
pub fn lies(a: Square, b: Square) -> bool {
    let slack = 1e-4;
    a.min_x >= b.min_x - slack && a.max_x <= b.max_x + slack && a.min_y >= b.min_y - slack && a.max_y <= b.max_y + slack
}

pub fn meets(polygon: &CoveragePolygon, r: Square) -> bool {
    polygon.intersects(r.min_x, r.min_y, r.max_x, r.max_y)
}

/// Whether `tile` is an ancestor of `other`.
pub fn covers(tile: VisibleTile, other: VisibleTile) -> bool {
    if tile.z >= other.z {
        return false;
    }
    let scale = 1 << (other.z - tile.z);
    other.x >= tile.x * scale && other.x < (tile.x + 1) * scale && other.y >= tile.y * scale && other.y < (tile.y + 1) * scale
}
