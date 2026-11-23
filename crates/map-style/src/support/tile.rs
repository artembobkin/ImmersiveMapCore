// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

/// The content identity of a vector tile: its column, row and zoom.
///
/// Stand-in for the engine's `Tile` (`Tile/Tile.swift`), ported only as far
/// as the schema, the style and the label policy read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Tile {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl Tile {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Tile { x, y, z }
    }
}
