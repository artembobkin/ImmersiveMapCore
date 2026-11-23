// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The few constants of the line tessellator and of the style baking that
//! the GPU line style is derived with (`TileLineStyle::from_pass`).
//!
//! Stand-ins for `ParseLine.featherTileUnits`,
//! `ParseLine.minimumExtrudedHalfWidth`, `ParseLine.extrudedHalfWidth` and
//! `BakedStyle.fillRibbonWidth` (`Tile/Parse/ParseLine.swift`,
//! `Tile/Parse/FeatureStyle+Parsing.swift`).

use crate::style::{ExtrusionStyle, FillStyle, LineGeometryStyle, LinePass};

/// The antialiasing feather the tessellator extrudes past a styled edge,
/// in tile units.
pub const FEATHER_TILE_UNITS: f32 = 2.0;

/// The least half-width a ribbon is extruded to, in tile units.
pub const MINIMUM_EXTRUDED_HALF_WIDTH: f32 = 8.0;

/// How far the ribbon is extruded from the centreline for a styled
/// half-width: the feather beyond the styled edge, never less than
/// [`MINIMUM_EXTRUDED_HALF_WIDTH`].
pub fn extruded_half_width(half_width: f32) -> f32 {
    (half_width + FEATHER_TILE_UNITS).max(MINIMUM_EXTRUDED_HALF_WIDTH)
}

/// The ribbon width every fill is baked with: the edge threshold it yields
/// is what the fill shaders have always read.
pub const FILL_RIBBON_WIDTH: f64 = 100.0;

/// The stroke a fill is baked to. A fill's colour on the globe rides in the
/// line style's slot (`TileLineStyle::sphere_color_*`): every style carries
/// one, and the sphere's fill vertex reads it the way the ribbons'
/// fragment does.
pub fn baked_fill_pass(fill: &FillStyle) -> LinePass {
    LinePass {
        zoom_fade: fill.zoom_fade,
        sphere_color: fill.sphere_color,
        ..LinePass::new(fill.key, fill.color, LineGeometryStyle::new(FILL_RIBBON_WIDTH))
    }
}

/// The stroke a building's footprint is baked to.
pub fn baked_extrusion_pass(extrusion: &ExtrusionStyle) -> LinePass {
    LinePass::new(extrusion.key, extrusion.color, LineGeometryStyle::new(FILL_RIBBON_WIDTH))
}
