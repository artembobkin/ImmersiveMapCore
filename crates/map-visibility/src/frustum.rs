// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The six planes of a projection-view matrix, for culling.

/// The planes as `(a, b, c, d)` quadruples in the order left, right, bottom,
/// top, near, far, each normalized so `a x + b y + c z + d` is a signed
/// distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frustum {
    pub planes: [f64; 24],
}

impl Frustum {
    /// Extracts the planes using Gribb-Hartmann adjusted for a 0 to 1 depth
    /// buffer (Metal, WebGPU, Vulkan): the near plane is the matrix's z row
    /// alone. `pv` is column-major.
    pub fn new(pv: &[f64; 16]) -> Frustum {
        let [m0x, m0y, m0z, m0w, m1x, m1y, m1z, m1w, m2x, m2y, m2z, m2w, m3x, m3y, m3z, m3w] = *pv;
        let mut frustum = Frustum { planes: [0.0; 24] };
        frustum.set_plane(0, m0w + m0x, m1w + m1x, m2w + m2x, m3w + m3x);
        frustum.set_plane(1, m0w - m0x, m1w - m1x, m2w - m2x, m3w - m3x);
        frustum.set_plane(2, m0w + m0y, m1w + m1y, m2w + m2y, m3w + m3y);
        frustum.set_plane(3, m0w - m0y, m1w - m1y, m2w - m2y, m3w - m3y);
        frustum.set_plane(4, m0z, m1z, m2z, m3z);
        frustum.set_plane(5, m0w - m0z, m1w - m1z, m2w - m2z, m3w - m3z);
        frustum
    }

    fn set_plane(&mut self, index: usize, a: f64, b: f64, c: f64, d: f64) {
        let inverse_length = 1.0 / (a * a + b * b + c * c).sqrt();
        self.planes[index * 4..index * 4 + 4].copy_from_slice(&[a * inverse_length, b * inverse_length, c * inverse_length, d * inverse_length]);
    }

    fn distances(&self, center: [f64; 3]) -> impl Iterator<Item = f64> + '_ {
        self.planes.as_chunks::<4>().0.iter().map(move |p| p[0] * center[0] + p[1] * center[1] + p[2] * center[2] + p[3])
    }

    /// Whether the sphere is at least partly inside.
    pub fn is_sphere_visible(&self, center: [f64; 3], radius: f64) -> bool {
        self.distances(center).all(|distance| distance >= -radius)
    }

    /// Whether the sphere is wholly inside.
    pub fn contains_sphere(&self, center: [f64; 3], radius: f64) -> bool {
        self.distances(center).all(|distance| distance >= radius)
    }
}
