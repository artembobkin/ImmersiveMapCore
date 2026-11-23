// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

/// An 8-bit sRGB color with straight (non-premultiplied) alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FlatColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl FlatColor {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        FlatColor { r, g, b, a }
    }

    /// An opaque color.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        FlatColor { r, g, b, a: 255 }
    }

    /// Components in 0...1.
    pub fn from_unit(red: f64, green: f64, blue: f64, alpha: f64) -> Self {
        fn byte(v: f64) -> u8 {
            (crate::fmin(crate::fmax(v, 0.0), 1.0) * 255.0).round() as u8
        }
        FlatColor::new(byte(red), byte(green), byte(blue), byte(alpha))
    }

    /// `r` in the lowest byte, `a` in the highest: the byte order of `rgba8unorm` on little-endian.
    #[inline(always)]
    pub const fn packed(self) -> u32 {
        self.r as u32 | (self.g as u32) << 8 | (self.b as u32) << 16 | (self.a as u32) << 24
    }

    #[inline(always)]
    pub const fn from_packed(packed: u32) -> Self {
        FlatColor::new(
            packed as u8,
            (packed >> 8) as u8,
            (packed >> 16) as u8,
            (packed >> 24) as u8,
        )
    }
}

/// A mesh vertex: position in tile units (x right, y down) and a baked color (`FlatColor::packed`).
/// The layout is 12 bytes and can be handed to the GPU as is.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct FlatVertex {
    pub x: f32,
    pub y: f32,
    pub color: u32,
}

const _: () = assert!(std::mem::size_of::<FlatVertex>() == 12);

/// A flattened tile: triangles that do not overlap, each with one flat color.
///
/// Vertices are shared only inside one color region, so the three vertices of a triangle always
/// carry the same color. Every triangle has a positive signed area in tile coordinates.
#[derive(Debug, Clone, Default)]
pub struct FlatMesh {
    pub vertices: Vec<FlatVertex>,
    pub indices: Vec<u32>,
}

impl FlatMesh {
    pub fn new(vertices: Vec<FlatVertex>, indices: Vec<u32>) -> Self {
        FlatMesh { vertices, indices }
    }

    #[inline]
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}
