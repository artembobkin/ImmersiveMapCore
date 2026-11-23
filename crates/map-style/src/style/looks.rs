// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The looks a style states that are drawn at frame time and baked into no
//! prepared tile: what no tile paints, the shading of the slopes, the
//! darkening of the valleys, the colours of the trees and the opacity of
//! the labels by the zoom.

use serde::{Deserialize, Serialize};

use crate::style::LabelTextStyle;
use crate::support::ZoomCurve;
use crate::{Float3, Float4};

/// The colours of what no tile paints, stated by the style like every other
/// colour of the map: the map's ground where no tile has arrived yet (the
/// flat map's clear colour, and the tint of its horizon fog), and the two
/// polar caps past the last Mercator tile row, the north continuing the
/// open ocean and the south the ice sheet. The built-in style takes all
/// three from its theme, so a recolour of the land, the water and the ice
/// carries to them by itself.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseColors {
    pub map: Float4,
    pub north_cap: Float4,
    pub south_cap: Float4,
}

/// How the ground's slopes are shaded where the relief stands, stated by
/// the style like every other colour of the map: a slope turned toward the
/// light takes the highlight, a slope turned away from it the shadow, and
/// level ground keeps its own colour. The light is the scene's (the sun
/// the buildings cast their shadows by), and the slopes are the terrain's
/// own, read per pixel from the terrain tiles, so the shading is finer than
/// the relief's grid. Drawn at frame time, never baked into the prepared
/// tiles, so a change needs no re-parse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Hillshade {
    /// How strongly the slopes are shaded: 0 draws none, 1 takes a slope
    /// that faces the light square on, or turns away from it entirely, all
    /// the way to its colour. 0.76 by default.
    pub intensity: f32,
    /// The colour a slope turned away from the light is drawn toward, RGBA
    /// in 0...1. The alpha caps how far, 0.34 by default.
    pub shadow_color: Float4,
    /// The colour a slope turned toward the light is drawn toward, RGBA in
    /// 0...1. The alpha caps how far, 0.18 by default: the light side of a
    /// mountain keeps the map's colour, the shade side carries the form.
    pub highlight_color: Float4,
    /// Where the shading starts to fade, in metres from the eye, by the
    /// camera zoom (read at the zoom the relief stands by): 60 km at zoom
    /// 10, 4 km at 14 and 250 m at 18 by default, halving with every zoom
    /// between the stops as the map's scale does (the curve runs
    /// geometrically), which keeps the fade at about the same place on
    /// screen. The shading is for the ground near the camera. Far off, a
    /// pixel spans many of the terrain's heights and the slopes it falls on
    /// change from frame to frame, so the shading there only shimmers, and
    /// the far ground is read by its outline anyway. In metres, like the
    /// ground fog's lengths, so the same curve means the same ground on
    /// every screen and at every pitch.
    pub fade_start_meters: ZoomCurve,
    /// Where the shading has faded out, in the same metres by the zoom:
    /// twice the start by default (120 km at zoom 10 to 500 m at 18). Past
    /// it no slope is shaded, and the ground there costs nothing for it.
    /// Raise both to shade further out, lower them if the far ground still
    /// shimmers.
    pub fade_end_meters: ZoomCurve,
}

impl Default for Hillshade {
    fn default() -> Self {
        Hillshade {
            intensity: 0.76,
            shadow_color: [0.05, 0.07, 0.15, 0.34],
            highlight_color: [1.0, 0.98, 0.92, 0.18],
            fade_start_meters: ZoomCurve::new([(10.0, 60_000.0), (14.0, 4000.0), (18.0, 250.0)]),
            fade_end_meters: ZoomCurve::new([(10.0, 120_000.0), (14.0, 8000.0), (18.0, 500.0)]),
        }
    }
}

impl Hillshade {
    /// No shading.
    pub fn none() -> Self {
        Hillshade { intensity: 0.0, ..Hillshade::default() }
    }
}

/// How much the ground is darkened where the terrain around it hides the
/// sky, where the relief stands, stated by the style like the hillshade:
/// the floor of a valley sees a strip of sky between the ridges and takes
/// less of its light than a summit or an open slope, which see it all. The
/// occlusion is computed from the terrain tiles themselves, once a tile
/// lands: for a grid of points over the tile, how high the terrain rises
/// against the sky in each of `directions` directions out to
/// `radius_meters`, averaged. In metres on the ground, so it reads the same
/// at every zoom, a valley kilometres wide as dark from afar as from near.
/// Drawn at frame time, never baked into the prepared tiles, so a change
/// needs no re-parse, only the terrain tiles computed again.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TerrainOcclusion {
    /// Whether the ground is darkened. True by default.
    pub is_enabled: bool,
    /// How dark a point the terrain hides the sky from entirely is drawn, 0
    /// to 1: 0 draws nothing, 1 draws it black. 0.6 by default: a deep
    /// valley reads clearly darker, the colours of the map still show.
    pub strength: f32,
    /// How far out the terrain is looked at for what hides the sky, in
    /// metres on the ground. 3000 by default: a valley between ridges a few
    /// kilometres apart is read whole, a hill reads against its own
    /// surroundings. Farther reads wider valleys at more computing per
    /// tile, nearer only the folds.
    pub radius_meters: f32,
    /// How many directions the sky is looked at in, 4 to 32. 8 by default.
    /// More directions smooth the result, at a cost in computing that grows
    /// with them.
    pub directions: i32,
    /// How many points a side the occlusion is computed for over a terrain
    /// tile, 16 to 1024. 128 by default: with the terrain tiles of 512
    /// texels, one point for every four texels, finer than the relief's
    /// grid. Raise it for sharper folds at four times the computing with
    /// each doubling.
    pub resolution: i32,
}

impl Default for TerrainOcclusion {
    fn default() -> Self {
        TerrainOcclusion { is_enabled: true, strength: 0.6, radius_meters: 3000.0, directions: 8, resolution: 128 }
    }
}

impl TerrainOcclusion {
    /// No darkening.
    pub fn none() -> Self {
        TerrainOcclusion { is_enabled: false, ..TerrainOcclusion::default() }
    }
}

/// The colours of the trees that stand on the woods and the parks
/// (`TreeFillStyle`). The tree model carries no colour of its own, only
/// which part a point belongs to and how deep in the crown it lies, and the
/// frame colours it from here: the crown runs from `crown_shade_color` at
/// its underside to `crown_color` at its top, the trunk is `trunk_color`,
/// each tree a shade lighter or darker than the next. Drawn at frame time,
/// never baked into the prepared tiles, so a change needs no re-parse. RGB
/// in 0...1.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TreePalette {
    /// The crown's top, in the light.
    pub crown_color: Float3,
    /// The crown's underside, in its own shade.
    pub crown_shade_color: Float3,
    pub trunk_color: Float3,
}

impl TreePalette {
    /// The day's trees: a crown a touch deeper than the map's wood, so a
    /// tree reads on it, lightening to the wood's own tone at the top.
    pub const DAY: TreePalette =
        TreePalette { crown_color: [0.64, 0.82, 0.46], crown_shade_color: [0.40, 0.63, 0.36], trunk_color: [0.48, 0.36, 0.25] };

    /// The night's trees, for the night palette: the crown a dim green over
    /// the night's wood, the trunk nearly lost in the dark.
    pub const NIGHT: TreePalette =
        TreePalette { crown_color: [0.17, 0.30, 0.22], crown_shade_color: [0.08, 0.16, 0.13], trunk_color: [0.14, 0.12, 0.11] };
}

impl Default for TreePalette {
    fn default() -> Self {
        TreePalette::DAY
    }
}

/// How opaque the map's names and its POIs are, by the zoom: a factor over
/// each label's own opacity (`LabelTextStyle::opacity`), 0 to 1, so a name
/// laid over the land can fade as the map grows busy, or the POIs give way
/// to the places at the zooms where both stand.
///
/// Read at the zoom the labels are planned by on every frame, so a zoom
/// gesture plays the change. Drawn at frame time, never baked into the
/// prepared tiles: a change needs no re-parse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LabelOpacity {
    /// Every name but the POIs': the places, the countries and the regions,
    /// the waters and the streets. 1 at every zoom by default.
    pub names: ZoomCurve,
    /// The POIs, their icon plates and their names alike. 1 at every zoom
    /// by default.
    pub pois: ZoomCurve,
}

impl Default for LabelOpacity {
    fn default() -> Self {
        LabelOpacity { names: ZoomCurve::constant(1.0), pois: ZoomCurve::constant(1.0) }
    }
}

impl LabelOpacity {
    /// The two factors at a zoom, each held to 0...1.
    pub fn frame(&self, zoom: f64) -> LabelOpacityFrame {
        LabelOpacityFrame {
            names: self.names.value(zoom).clamp(0.0, 1.0),
            pois: self.pois.value(zoom).clamp(0.0, 1.0),
        }
    }
}

/// The label opacity of one frame (`LabelOpacity` read at the frame's
/// zoom): what every label drawer multiplies a label's own opacity by, the
/// POIs' factor for a POI's plate and name, the names' for the rest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LabelOpacityFrame {
    pub names: f32,
    pub pois: f32,
}

impl LabelOpacityFrame {
    /// Every label at its own opacity.
    pub const SOLID: LabelOpacityFrame = LabelOpacityFrame { names: 1.0, pois: 1.0 };

    /// The opacity a label of the style draws at.
    pub fn opacity(&self, style: &LabelTextStyle) -> f32 {
        style.opacity * if style.is_point_of_interest { self.pois } else { self.names }
    }
}
