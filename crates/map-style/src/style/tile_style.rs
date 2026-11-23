// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use crate::style::LinePass;
use crate::support::line_baking;
use crate::{Float3, Float4};

/// One colour, 16 bytes. The memory layout is a binding contract with the
/// `Style` structs of the tile shaders and an arena span stride. Changing
/// it is a prepared-cache format change.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct TilePolygonStyle {
    pub color: Float4,
}

/// Per-style line rendering parameters, uploaded alongside
/// `TilePolygonStyle` and indexed by the same style index. The memory
/// layout (17 floats, 68 bytes, no padding) is a binding contract with the
/// tile shaders' `LineStyle` struct and an arena span stride. Changing it is
/// a prepared-cache format change.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct TileLineStyle {
    /// Point-locked visible full width. Zero keeps the world-locked width
    /// the tessellator baked. See `LinePass::line_width_points`.
    pub width_points: f32,
    /// Point-locked dash pattern, resolved per fragment from the vertices'
    /// arc-length parameter. Zero dash length draws solid.
    pub dash_length_points: f32,
    pub dash_gap_points: f32,
    /// Where the styled edge sits inside the extruded distance field (styled
    /// half-width over extruded half-width, 0...1). Zero marks a non-line
    /// style: the shader skips line coverage entirely for it.
    pub edge_threshold: f32,
    /// Floor for a world-locked width, in layout points: the visible width
    /// is the world width or this, whichever is wider, so a road class never
    /// thins into an unreadable hairline at region zooms yet still grows
    /// with the world at street level. Zero disables the floor. Ignored
    /// when `width_points` locks the width outright.
    pub minimum_width_points: f32,
    /// Non-zero when the dash pattern is world-locked: `dash_length_points`
    /// and `dash_gap_points` are then tile units, not points, and the shader
    /// cuts the pattern from arc length with no point-to-unit conversion.
    /// Paint on the ground (a lane divider) is part of the surface: its
    /// period is a length in metres, so it must not re-flow when the camera
    /// zooms or the engine swaps the tile level that serves the road.
    pub dash_in_tile_units: f32,
    /// The styled half-width in tile units, what a deferred ribbon's vertex
    /// shader extrudes a world-locked style by.
    pub half_width_units: f32,
    /// The camera zoom a point-locked width is frozen on the ground from: up
    /// to it the width holds in points, past it the width is what those
    /// points covered on the ground at that zoom, so it doubles on screen
    /// with every zoom level like the map around it. Zero: the points hold
    /// at every zoom. See `LinePass::point_width_world_lock_zoom`.
    pub world_lock_zoom: f32,
    /// The zoom ramp of a point-locked width. See `WidthRamp`. An end zoom
    /// of zero is no ramp, and the start alpha is then one.
    pub ramp_start_width_points: f32,
    pub ramp_start_zoom: f32,
    pub ramp_end_zoom: f32,
    pub ramp_start_alpha: f32,
    /// The share of the colour's alpha the stroke keeps on the globe, one on
    /// the plane, handed over with the globe's air at the end of the
    /// unroll: a veil the globe's view lays over a stroke and the plane does
    /// not. One is the same stroke on both. See `LinePass::sphere_alpha`.
    pub sphere_alpha: f32,
    /// The colour the stroke wears on the globe, the style's own on the
    /// plane, handed over with the alpha. Three floats, no padding: the
    /// shader's packed three-float vector. See `LinePass::sphere_color`.
    pub sphere_color_red: f32,
    pub sphere_color_green: f32,
    pub sphere_color_blue: f32,
    /// The camera zoom the stroke keeps its sphere colour and alpha up to,
    /// on both surfaces, switching to its own at it
    /// (`LinePass::sphere_look_until_zoom`). Zero is none: the plane draws
    /// the stroke's own look, and the globe hands it over with the air.
    pub sphere_look_until_zoom: f32,
}

impl TileLineStyle {
    /// The line style of a fill or an extrusion: no line coverage at all.
    pub const POLYGON: TileLineStyle = TileLineStyle {
        width_points: 0.0,
        dash_length_points: 0.0,
        dash_gap_points: 0.0,
        edge_threshold: 0.0,
        minimum_width_points: 0.0,
        dash_in_tile_units: 0.0,
        half_width_units: 0.0,
        world_lock_zoom: 0.0,
        ramp_start_width_points: 0.0,
        ramp_start_zoom: 0.0,
        ramp_end_zoom: 0.0,
        ramp_start_alpha: 1.0,
        sphere_alpha: 1.0,
        sphere_color_red: 0.0,
        sphere_color_green: 0.0,
        sphere_color_blue: 0.0,
        sphere_look_until_zoom: 0.0,
    };

    /// The GPU line style the parser bakes a stroke to (the engine's
    /// `TileUnificationStage.makeTileLineStyle`): the edge threshold from
    /// the ribbon's styled and extruded half-widths, the globe colour the
    /// stroke's own unless it states one.
    pub fn from_pass(pass: &LinePass) -> TileLineStyle {
        let half_width = pass.line_geometry.line_width as f32 * 0.5;
        let edge_threshold =
            if half_width > 0.0 { half_width / line_baking::extruded_half_width(half_width) } else { 0.0 };
        let sphere_color: Float3 = pass.sphere_color.unwrap_or([pass.color[0], pass.color[1], pass.color[2]]);
        let ramp = pass.point_width_ramp;
        TileLineStyle {
            width_points: pass.line_width_points,
            dash_length_points: pass.dash_length_points,
            dash_gap_points: pass.dash_gap_points,
            edge_threshold,
            minimum_width_points: pass.minimum_width_points,
            dash_in_tile_units: if pass.dash_in_tile_units { 1.0 } else { 0.0 },
            half_width_units: half_width,
            world_lock_zoom: pass.point_width_world_lock_zoom,
            ramp_start_width_points: ramp.map_or(0.0, |ramp| ramp.start_width_points),
            ramp_start_zoom: ramp.map_or(0.0, |ramp| ramp.start_zoom),
            ramp_end_zoom: ramp.map_or(0.0, |ramp| ramp.end_zoom),
            ramp_start_alpha: ramp.map_or(1.0, |ramp| ramp.start_alpha),
            sphere_alpha: pass.sphere_alpha,
            sphere_color_red: sphere_color[0],
            sphere_color_green: sphere_color[1],
            sphere_color_blue: sphere_color[2],
            sphere_look_until_zoom: pass.sphere_look_until_zoom.unwrap_or(0.0).max(0.0),
        }
    }

    /// The 17 floats in the shader's order.
    pub fn to_array(&self) -> [f32; 17] {
        [
            self.width_points,
            self.dash_length_points,
            self.dash_gap_points,
            self.edge_threshold,
            self.minimum_width_points,
            self.dash_in_tile_units,
            self.half_width_units,
            self.world_lock_zoom,
            self.ramp_start_width_points,
            self.ramp_start_zoom,
            self.ramp_end_zoom,
            self.ramp_start_alpha,
            self.sphere_alpha,
            self.sphere_color_red,
            self.sphere_color_green,
            self.sphere_color_blue,
            self.sphere_look_until_zoom,
        ]
    }
}
