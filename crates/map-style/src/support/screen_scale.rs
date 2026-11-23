// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

/// Conversion between layout points and device pixels.
///
/// Points are the engine's screen-space unit for everything with a
/// perceived physical size: label text, halos, POI icons. The conversion
/// happens once, at the render boundary: geometry baked into a prepared
/// tile stays in points and therefore stays resolution independent.
///
/// Stand-in for the engine's `ScreenScale` (`Utils/ScreenScale.swift`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenScale {
    /// Device pixels per layout point.
    pixels_per_point: f32,
}

impl ScreenScale {
    /// The scale the built-in styles are authored against: a 2x display.
    /// Style values in points multiplied by this reproduce the pixel sizes
    /// the engine used before screen-space quantities carried a unit.
    pub const REFERENCE: ScreenScale = ScreenScale { pixels_per_point: 2.0 };

    pub const IDENTITY: ScreenScale = ScreenScale { pixels_per_point: 1.0 };

    pub fn new(pixels_per_point: f32) -> Self {
        ScreenScale { pixels_per_point: pixels_per_point.max(0.01) }
    }

    pub fn pixels_per_point(&self) -> f32 {
        self.pixels_per_point
    }

    pub fn pixels(&self, points: f32) -> f32 {
        points * self.pixels_per_point
    }

    pub fn points(&self, pixels: f32) -> f32 {
        pixels / self.pixels_per_point
    }
}
