// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! A two-component float vector, the counterpart of Swift's `SIMD2<Float>`,
//! with the arithmetic the tessellators use.

use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[repr(C)]
pub struct V2 {
    pub x: f32,
    pub y: f32,
}

impl V2 {
    pub const ZERO: V2 = V2 { x: 0.0, y: 0.0 };

    #[inline(always)]
    pub const fn new(x: f32, y: f32) -> V2 {
        V2 { x, y }
    }

    #[inline(always)]
    pub fn dot(self, other: V2) -> f32 {
        self.x * other.x + self.y * other.y
    }

    /// The z of the 3D cross product.
    #[inline(always)]
    pub fn cross(self, other: V2) -> f32 {
        self.x * other.y - self.y * other.x
    }

    #[inline(always)]
    pub fn length_squared(self) -> f32 {
        self.dot(self)
    }

    #[inline(always)]
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    #[inline(always)]
    pub fn normalized(self) -> V2 {
        self / self.length()
    }
}

impl Add for V2 {
    type Output = V2;
    #[inline(always)]
    fn add(self, o: V2) -> V2 {
        V2::new(self.x + o.x, self.y + o.y)
    }
}

impl AddAssign for V2 {
    #[inline(always)]
    fn add_assign(&mut self, o: V2) {
        self.x += o.x;
        self.y += o.y;
    }
}

impl Sub for V2 {
    type Output = V2;
    #[inline(always)]
    fn sub(self, o: V2) -> V2 {
        V2::new(self.x - o.x, self.y - o.y)
    }
}

impl SubAssign for V2 {
    #[inline(always)]
    fn sub_assign(&mut self, o: V2) {
        self.x -= o.x;
        self.y -= o.y;
    }
}

impl Mul<f32> for V2 {
    type Output = V2;
    #[inline(always)]
    fn mul(self, s: f32) -> V2 {
        V2::new(self.x * s, self.y * s)
    }
}

impl Div<f32> for V2 {
    type Output = V2;
    #[inline(always)]
    fn div(self, s: f32) -> V2 {
        V2::new(self.x / s, self.y / s)
    }
}

impl Neg for V2 {
    type Output = V2;
    #[inline(always)]
    fn neg(self) -> V2 {
        V2::new(-self.x, -self.y)
    }
}

/// Clamps an integer into `i8`, Swift's `Int8(clamping:)`.
#[inline(always)]
pub fn clamp_i8(v: i64) -> i8 {
    v.clamp(i8::MIN as i64, i8::MAX as i64) as i8
}

/// Clamps an integer into `i16`, Swift's `Int16(clamping:)`.
#[inline(always)]
pub fn clamp_i16(v: i64) -> i16 {
    v.clamp(i16::MIN as i64, i16::MAX as i64) as i16
}

/// Swift's `Float.rounded()`: to nearest, ties away from zero.
#[inline(always)]
pub fn round(v: f32) -> i64 {
    v.round() as i64
}

/// A three-component float vector, Swift's `SIMD3<Float>`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct V3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl V3 {
    pub const ZERO: V3 = V3 { x: 0.0, y: 0.0, z: 0.0 };
    pub const UP: V3 = V3 { x: 0.0, y: 0.0, z: 1.0 };

    #[inline(always)]
    pub const fn new(x: f32, y: f32, z: f32) -> V3 {
        V3 { x, y, z }
    }

    #[inline(always)]
    pub fn dot(self, o: V3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    #[inline(always)]
    pub fn cross(self, o: V3) -> V3 {
        V3::new(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }

    #[inline(always)]
    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    #[inline(always)]
    pub fn normalized(self) -> V3 {
        let l = self.length();
        V3::new(self.x / l, self.y / l, self.z / l)
    }
}

impl Add for V3 {
    type Output = V3;
    #[inline(always)]
    fn add(self, o: V3) -> V3 {
        V3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl Sub for V3 {
    type Output = V3;
    #[inline(always)]
    fn sub(self, o: V3) -> V3 {
        V3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Mul<f32> for V3 {
    type Output = V3;
    #[inline(always)]
    fn mul(self, s: f32) -> V3 {
        V3::new(self.x * s, self.y * s, self.z * s)
    }
}
