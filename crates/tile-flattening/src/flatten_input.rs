// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use crate::FlatColor;

/// One ring in `FlattenInput::coords`: `count` points from point `start`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ring {
    pub(crate) start: u32,
    pub(crate) count: u32,
    pub(crate) paint: u16,
}

/// The geometry to flatten: closed rings, each filled with a paint.
///
/// Paints are stacked in the order they are added: a paint added later covers the earlier ones.
/// All rings of one paint are filled together with the nonzero winding rule, so overlapping rings
/// of the same paint merge, and a ring with the opposite orientation cuts a hole.
#[derive(Debug, Clone, Default)]
pub struct FlattenInput {
    pub(crate) paints: Vec<FlatColor>,
    /// Interleaved x, y.
    pub(crate) coords: Vec<f64>,
    pub(crate) rings: Vec<Ring>,
    open_ring_start: usize,
    open_ring_paint: u16,
    /// Scratch for the stroker.
    pub(crate) stroke_points: Vec<[f64; 2]>,
    pub(crate) stroke_right: Vec<[f64; 2]>,
}

impl FlattenInput {
    pub fn new() -> Self {
        Self::default()
    }

    /// The paints, bottom first.
    pub fn paints(&self) -> &[FlatColor] {
        &self.paints
    }

    pub fn ring_count(&self) -> usize {
        self.rings.len()
    }

    pub fn point_count(&self) -> usize {
        self.coords.len() / 2
    }

    pub fn reserve(&mut self, points: usize, rings: usize) {
        self.coords.reserve(points * 2);
        self.rings.reserve(rings);
    }

    /// Adds a paint on top of the existing ones and returns its index.
    ///
    /// # Panics
    /// When there are `u16::MAX` paints already.
    pub fn add_paint(&mut self, color: FlatColor) -> usize {
        assert!(self.paints.len() < usize::from(u16::MAX), "too many paints");
        self.paints.push(color);
        self.paints.len() - 1
    }

    // Rings

    /// Starts a ring. Add its points with `add_point` and finish it with `end_ring`.
    /// The ring is closed implicitly: do not repeat the first point.
    ///
    /// # Panics
    /// When `paint` does not fit in 16 bits.
    #[inline(always)]
    pub fn begin_ring(&mut self, paint: usize) {
        self.open_ring_start = self.coords.len() / 2;
        self.open_ring_paint = u16::try_from(paint).expect("paint index out of range");
    }

    #[inline(always)]
    pub fn add_point(&mut self, x: f64, y: f64) {
        self.coords.push(x);
        self.coords.push(y);
    }

    #[inline(always)]
    pub fn end_ring(&mut self) {
        let count = self.coords.len() / 2 - self.open_ring_start;
        if count >= 3 {
            self.rings.push(Ring {
                start: self.open_ring_start as u32,
                count: count as u32,
                paint: self.open_ring_paint,
            });
        } else {
            self.coords.truncate(self.open_ring_start * 2);
        }
        self.open_ring_start = self.coords.len() / 2;
    }

    pub fn add_ring(&mut self, points: &[[f64; 2]], paint: usize) {
        self.begin_ring(paint);
        self.coords.reserve(points.len() * 2);
        for p in points {
            self.add_point(p[0], p[1]);
        }
        self.end_ring();
    }

    /// Adds a polygon: an exterior ring and its holes. Holes must have the opposite orientation.
    pub fn add_polygon<R: AsRef<[[f64; 2]]>>(&mut self, polygon_rings: &[R], paint: usize) {
        for ring in polygon_rings {
            self.add_ring(ring.as_ref(), paint);
        }
    }

    pub fn add_rect(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64, paint: usize) {
        self.begin_ring(paint);
        self.add_point(min_x, min_y);
        self.add_point(max_x, min_y);
        self.add_point(max_x, max_y);
        self.add_point(min_x, max_y);
        self.end_ring();
    }
}
