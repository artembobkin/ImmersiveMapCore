// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

/// How a styled layer appears or disappears with the camera: fully drawn on
/// one side of a zoom range, not drawn on the other, and a smooth blend in
/// between. Any fill, line or road pass takes one (`FillStyle::zoom_fade`,
/// `LinePass::zoom_fade`).
///
/// The fade is a function of the camera zoom alone, evaluated per frame in
/// the shader, so it is continuous while the camera moves and two tile
/// levels that state the same fade draw the same frame: the swap from one
/// tile level to the next shows nothing. Tiles are chosen by flooring the
/// camera zoom, so a layer a source ships through tile z7 is still on
/// screen for camera zoom 7.0 up to 8.0, and that is the range to fade it
/// out over (`ZoomFade::fade_out(7.0, 8.0)`) so it is gone before the z8
/// tiles that lack it take over. A layer that first ships at tile z8 but is
/// also in the z7 tiles fades in over the same range on the z7 tiles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomFade {
    zero_alpha_zoom: f64,
    full_alpha_zoom: f64,
}

impl ZoomFade {
    /// No fade: the layer is drawn at its full alpha at every zoom.
    pub const NONE: ZoomFade = ZoomFade { zero_alpha_zoom: -2.0, full_alpha_zoom: -1.0 };

    /// The band over which the globe's overview content comes in as the
    /// camera leaves the whole-planet view: point-locked strokes and the
    /// labels of the first zoom level.
    pub const OVERVIEW: ZoomFade = ZoomFade { zero_alpha_zoom: 0.0, full_alpha_zoom: 1.0 };

    /// Not drawn up to camera zoom `from`, drawn in full from `to`.
    ///
    /// # Panics
    ///
    /// When `from` is not below `to`.
    pub fn fade_in(from: f64, to: f64) -> ZoomFade {
        assert!(from < to, "A zoom fade runs from a lower to a higher zoom");
        ZoomFade { zero_alpha_zoom: from, full_alpha_zoom: to }
    }

    /// Drawn in full up to camera zoom `from`, gone from `to`.
    ///
    /// # Panics
    ///
    /// When `from` is not below `to`.
    pub fn fade_out(from: f64, to: f64) -> ZoomFade {
        assert!(from < to, "A zoom fade runs from a lower to a higher zoom");
        ZoomFade { zero_alpha_zoom: to, full_alpha_zoom: from }
    }

    /// The camera zoom at which the layer is not drawn at all.
    pub fn zero_alpha_zoom(&self) -> f64 {
        self.zero_alpha_zoom
    }

    /// The camera zoom from which the layer is drawn at its full alpha.
    pub fn full_alpha_zoom(&self) -> f64 {
        self.full_alpha_zoom
    }

    /// The share of the layer's alpha drawn at a camera zoom: a smoothstep
    /// from 0 at `zero_alpha_zoom` to 1 at `full_alpha_zoom`. The shader's
    /// `tileStyleFade` evaluates the same curve.
    pub fn alpha(&self, zoom: f64) -> f32 {
        let progress = ((zoom - self.zero_alpha_zoom) / (self.full_alpha_zoom - self.zero_alpha_zoom)) as f32;
        let clamped = progress.clamp(0.0, 1.0);
        clamped * clamped * (3.0 - 2.0 * clamped)
    }

    /// The pair the tile shaders read per style: the zero alpha zoom, then
    /// the full alpha zoom.
    pub fn shader_pair(&self) -> [f32; 2] {
        [self.zero_alpha_zoom as f32, self.full_alpha_zoom as f32]
    }
}

impl Default for ZoomFade {
    fn default() -> Self {
        ZoomFade::NONE
    }
}
