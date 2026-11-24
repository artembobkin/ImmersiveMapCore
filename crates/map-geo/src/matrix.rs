// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! 4x4 matrices of doubles, column-major, as the engine's shaders read them.

pub type Mat4 = [f64; 16];

pub const IDENTITY: Mat4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// `a * b`.
pub fn multiply(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [0.0; 16];
    for column in 0..4 {
        let base = column * 4;
        let (b0, b1, b2, b3) = (b[base], b[base + 1], b[base + 2], b[base + 3]);
        for row in 0..4 {
            out[base + row] = a[row] * b0 + a[4 + row] * b1 + a[8 + row] * b2 + a[12 + row] * b3;
        }
    }
    out
}

/// The perspective of the render camera. Its z row is the GL form
/// (`-(far + near) / (far - near)`, `-2 far near / (far - near)`), exactly as
/// the Swift engine builds it.
pub fn perspective(fov_radians: f64, aspect: f64, near: f64, far: f64) -> Mat4 {
    let y_scale = 1.0 / (fov_radians * 0.5).tan();
    let z_range = far - near;
    let mut out = [0.0; 16];
    out[0] = y_scale / aspect;
    out[5] = y_scale;
    out[10] = -(far + near) / z_range;
    out[11] = -1.0;
    out[14] = (-2.0 * far * near) / z_range;
    out
}

/// The orthographic matrix of the GL convention (NDC depth -1...1).
pub fn orthographic(left: f64, right: f64, bottom: f64, top: f64, near: f64, far: f64) -> Mat4 {
    let (rml, tmb, fmn) = (right - left, top - bottom, far - near);
    let mut out = [0.0; 16];
    out[0] = 2.0 / rml;
    out[5] = 2.0 / tmb;
    out[10] = -2.0 / fmn;
    out[12] = -(right + left) / rml;
    out[13] = -(top + bottom) / tmb;
    out[14] = -(far + near) / fmn;
    out[15] = 1.0;
    out
}

/// The view matrix of an eye looking at `center` with `up` upward.
pub fn look_at(eye: [f64; 3], center: [f64; 3], up: [f64; 3]) -> Mat4 {
    let (mut fx, mut fy, mut fz) = (center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]);
    let f_length = 1.0 / (fx * fx + fy * fy + fz * fz).sqrt();
    fx *= f_length;
    fy *= f_length;
    fz *= f_length;
    // s = normalize(cross(f, up))
    let (mut sx, mut sy, mut sz) = (fy * up[2] - fz * up[1], fz * up[0] - fx * up[2], fx * up[1] - fy * up[0]);
    let s_length = 1.0 / (sx * sx + sy * sy + sz * sz).sqrt();
    sx *= s_length;
    sy *= s_length;
    sz *= s_length;
    // u = cross(s, f)
    let (ux, uy, uz) = (sy * fz - sz * fy, sz * fx - sx * fz, sx * fy - sy * fx);
    [
        sx,
        ux,
        -fx,
        0.0,
        sy,
        uy,
        -fy,
        0.0,
        sz,
        uz,
        -fz,
        0.0,
        -(sx * eye[0] + sy * eye[1] + sz * eye[2]),
        -(ux * eye[0] + uy * eye[1] + uz * eye[2]),
        fx * eye[0] + fy * eye[1] + fz * eye[2],
        1.0,
    ]
}

/// The inverse, `None` for a singular matrix.
pub fn inverted(m: &Mat4) -> Option<Mat4> {
    let [a00, a01, a02, a03, a10, a11, a12, a13, a20, a21, a22, a23, a30, a31, a32, a33] = *m;
    let b00 = a00 * a11 - a01 * a10;
    let b01 = a00 * a12 - a02 * a10;
    let b02 = a00 * a13 - a03 * a10;
    let b03 = a01 * a12 - a02 * a11;
    let b04 = a01 * a13 - a03 * a11;
    let b05 = a02 * a13 - a03 * a12;
    let b06 = a20 * a31 - a21 * a30;
    let b07 = a20 * a32 - a22 * a30;
    let b08 = a20 * a33 - a23 * a30;
    let b09 = a21 * a32 - a22 * a31;
    let b10 = a21 * a33 - a23 * a31;
    let b11 = a22 * a33 - a23 * a32;
    let determinant = b00 * b11 - b01 * b10 + b02 * b09 + b03 * b08 - b04 * b07 + b05 * b06;
    if determinant == 0.0 || !determinant.is_finite() {
        return None;
    }
    let inverse = 1.0 / determinant;
    Some([
        (a11 * b11 - a12 * b10 + a13 * b09) * inverse,
        (a02 * b10 - a01 * b11 - a03 * b09) * inverse,
        (a31 * b05 - a32 * b04 + a33 * b03) * inverse,
        (a22 * b04 - a21 * b05 - a23 * b03) * inverse,
        (a12 * b08 - a10 * b11 - a13 * b07) * inverse,
        (a00 * b11 - a02 * b08 + a03 * b07) * inverse,
        (a32 * b02 - a30 * b05 - a33 * b01) * inverse,
        (a20 * b05 - a22 * b02 + a23 * b01) * inverse,
        (a10 * b10 - a11 * b08 + a13 * b06) * inverse,
        (a01 * b08 - a00 * b10 - a03 * b06) * inverse,
        (a30 * b04 - a31 * b02 + a33 * b00) * inverse,
        (a21 * b02 - a20 * b04 - a23 * b00) * inverse,
        (a11 * b07 - a10 * b09 - a12 * b06) * inverse,
        (a00 * b09 - a01 * b07 + a02 * b06) * inverse,
        (a31 * b01 - a30 * b03 - a32 * b00) * inverse,
        (a20 * b03 - a21 * b01 + a22 * b00) * inverse,
    ])
}
