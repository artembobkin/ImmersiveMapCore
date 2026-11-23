// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use serde::{Deserialize, Serialize};

use crate::style::{
    LabelPlacement, LabelTypeScale, LineGeometryStyle, PoiSpriteIcon, RoadDecorationKind, RouteShieldStyle, ZoomFade,
};
use crate::support::ScreenScale;
use crate::{Float3, Float4};

/// The weight of a label's face: one of the two atlases. The raw values are
/// written into the prepared-tile disk format and folded into the style
/// fingerprint, so they never change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[repr(u8)]
pub enum LabelFontWeight {
    Bold = 0,
    Thin = 1,
}

impl LabelFontWeight {
    pub fn raw_value(self) -> u8 {
        self as u8
    }
}

/// Where a line's geometry draws: with the ground fills, or in the bridge
/// overlay above the roads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LinePlacement {
    #[default]
    Ground,
    BridgeOverlay,
}

/// The passes a road is drawn in, bottom to top. A `RoadStyle` states one
/// stroke per role it wants (any number of paint strokes). The roads of a
/// tile are then stacked role by role, so every casing lies under every
/// fill of its tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum RoadPassRole {
    Shadow = 0,
    Casing = 1,
    Fill = 2,
    Detail = 3,
    Overlay = 4,
}

impl RoadPassRole {
    pub const ALL: [RoadPassRole; 5] =
        [RoadPassRole::Shadow, RoadPassRole::Casing, RoadPassRole::Fill, RoadPassRole::Detail, RoadPassRole::Overlay];

    pub fn raw_value(self) -> u8 {
        self as u8
    }
}

/// Which network a road belongs to on the ground. The automobile tier draws
/// above the pedestrian one, so a path ending against an avenue never lies
/// over its kerb and a path crossing a street never cuts the street's
/// casing. The style decides the tier per road class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RoadTier {
    #[default]
    Pedestrian,
    Automobile,
}

/// Where a road draws in the road stack: tunnels under everything on the
/// ground, bridges over everything. The style decides from the facts (a
/// road in a tunnel, a road diving under a bridge with only a negative
/// `layer`, a ramp with a positive one).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RoadLevel {
    Tunnel,
    #[default]
    Ground,
    Bridge,
}

/// How a label's text is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelTextStyle {
    /// The style identity the label's glyph runs are grouped by. Set by the
    /// `FeatureStyle` factories from the feature style's key. A style that
    /// builds its `FeatureStyle` by hand sets it itself.
    pub key: i32,
    pub fill_color: Float3,
    pub stroke_color: Float3,
    /// Halo width as a fraction of the em, so that it tracks the text size
    /// instead of drifting: an absolute width makes the smallest labels
    /// carry the widest halo relative to their strokes.
    pub halo_em: f32,
    /// Em size in layout points, not device pixels: the engine multiplies
    /// by the display's pixels-per-point at render time, so one value reads
    /// at the same physical size on a 2x desktop display and a 3x phone.
    /// Sizes below `LabelTypeScale::MINIMUM_SIZE_POINTS` are raised to it
    /// by the factories, the floor for type that is meant to be read.
    pub size_points: f32,
    pub weight: LabelFontWeight,
    /// Whether the label is set in capitals. The text is uppercased when
    /// the name is resolved, before the atlas is asked whether it can
    /// render it, so a name whose capitals it lacks falls back like any
    /// other.
    pub uppercased: bool,
    /// How far the letters are grown past the font's outline, in layout
    /// points: a little weight that keeps small type solid over a busy map.
    /// Zero draws the face as the font designs it, and a negative value
    /// thins it.
    pub embolden_points: f32,
    /// Extra room after every letter, in ems: the tracking a name spaced
    /// out over the land it names is set with, the way an atlas letters a
    /// country. Zero sets the letters as the font spaces them.
    pub letter_spacing_em: f32,
    /// The opacity of the label, fill and halo alike, 0 to 1: a name laid
    /// over the land as lettering rather than a sign, letting the map show
    /// through. 1 is solid.
    pub opacity: f32,
    /// How softly the halo ends, 0 to 1: the share of the halo's width,
    /// counted in from its outer edge, over which it fades to nothing, so
    /// the halo melts into the map instead of ending in a line. 0 is a halo
    /// solid to its edge. The halo keeps its width either way. Held to
    /// 0...1 by `with_halo_softness`.
    pub halo_softness: f32,
    /// Whether the label is a POI's name, which fades with the POIs and not
    /// with the other names (`LabelOpacity::pois`).
    pub is_point_of_interest: bool,
    /// The fill and the halo the label wears on the globe, `None` for its
    /// own: the day's dark lettering under a night palette, so the planet
    /// keeps its daylight while the plane is dark. They hold through the
    /// globe's view, the unroll into the plane included, and are handed
    /// over to the label's own colours with the globe's air at the end of
    /// the unroll, the way a fill's `FillStyle::sphere_color` is. The size,
    /// the weight, the halo's width and the opacity stay the label's own.
    /// Only the labels of the globe's zooms read them.
    pub sphere_fill_color: Option<Float3>,
    pub sphere_stroke_color: Option<Float3>,
}

impl LabelTextStyle {
    /// The growth every label had before it was a style value: three
    /// quarters of a pixel on the 2x display the palette was authored on.
    pub const DEFAULT_EMBOLDEN_POINTS: f32 = 0.375;

    /// A label of these colours, halo and size, with every other knob at
    /// its default: key 0, not in capitals, the default growth, no
    /// tracking, solid, a hard halo, no POI, no globe colours.
    pub fn new(fill_color: Float3, stroke_color: Float3, halo_em: f32, size_points: f32, weight: LabelFontWeight) -> Self {
        LabelTextStyle {
            key: 0,
            fill_color,
            stroke_color,
            halo_em,
            size_points,
            weight,
            uppercased: false,
            embolden_points: Self::DEFAULT_EMBOLDEN_POINTS,
            letter_spacing_em: 0.0,
            opacity: 1.0,
            halo_softness: 0.0,
            is_point_of_interest: false,
            sphere_fill_color: None,
            sphere_stroke_color: None,
        }
    }

    /// The same style with a halo softness, held to 0...1.
    pub fn with_halo_softness(mut self, halo_softness: f32) -> Self {
        self.halo_softness = halo_softness.clamp(0.0, 1.0);
        self
    }

    /// The fill at a point of the globe's handover to the plane: the
    /// globe's at 0, the label's own at 1 and where it states no globe
    /// colour, mixed between.
    pub fn fill_color_at_handover(&self, handover: f32) -> Float3 {
        mix(self.sphere_fill_color, self.fill_color, handover)
    }

    /// The halo at a point of the globe's handover to the plane, as
    /// `fill_color_at_handover`.
    pub fn stroke_color_at_handover(&self, handover: f32) -> Float3 {
        mix(self.sphere_stroke_color, self.stroke_color, handover)
    }

    /// Halo width in device pixels for this style at a given screen scale.
    pub fn halo_width_pixels(&self, screen_scale: ScreenScale) -> f32 {
        screen_scale.pixels(self.halo_em * self.size_points)
    }

    /// The growth of the letters in device pixels at a given screen scale.
    pub fn embolden_pixels(&self, screen_scale: ScreenScale) -> f32 {
        screen_scale.pixels(self.embolden_points)
    }
}

fn mix(sphere: Option<Float3>, own: Float3, handover: f32) -> Float3 {
    let Some(sphere) = sphere else {
        return own;
    };
    let t = handover.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| sphere[i] + (own[i] - sphere[i]) * t)
}

/// How a point-locked width comes in with the camera: `start_width_points`
/// wide, at `start_alpha` of the colour's alpha, up to `start_zoom`, and
/// the stroke's own `line_width_points` and alpha from `end_zoom`. Between
/// the two the width grows by the same ratio per zoom level and the alpha
/// linearly, both continuous in camera zoom. The ramp is a function of the
/// camera alone, so two tile levels that state the same ramp draw the same
/// stroke, and the swap between them shows nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WidthRamp {
    pub start_width_points: f32,
    pub start_zoom: f32,
    pub end_zoom: f32,
    pub start_alpha: f32,
}

impl WidthRamp {
    /// The width at a camera zoom, for a stroke `end_width_points` wide.
    pub fn width_points(&self, zoom: f32, end_width_points: f32) -> f32 {
        if !(self.end_zoom > self.start_zoom && self.start_width_points > 0.0 && end_width_points > 0.0) {
            return end_width_points;
        }
        let progress = ((zoom - self.start_zoom) / (self.end_zoom - self.start_zoom)).clamp(0.0, 1.0);
        self.start_width_points
            * ((end_width_points / self.start_width_points) as f64).powf(progress as f64) as f32
    }
}

/// One stroke of a line: what the tessellator bakes (the geometry) and what
/// the shader draws it with. A plain line is one stroke. A road is a stroke
/// per role.
///
/// `key` is the stroke's identity within a tile and its place in the draw
/// order: the ground geometry of a tile draws in ascending key, and every
/// feature with the same key shares one style table entry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinePass {
    pub key: u8,
    pub color: Float4,
    /// How the stroke appears or disappears with the camera zoom.
    pub zoom_fade: ZoomFade,
    /// Point-locked visible line width (full width, layout points). Zero
    /// keeps the world-locked behavior: the visible edge is the tessellated
    /// width. Non-zero resolves the edge in screen space at render time, so
    /// the width holds steady through fractional zoom instead of pumping
    /// with the tile's on-screen scale. The tessellated width then only
    /// bounds how wide the line can get.
    pub line_width_points: f32,
    /// Point-locked dash pattern (layout points), cut per fragment from the
    /// vertices' arc-length parameter, so dashes hold their on-screen size
    /// at every zoom. Zero dash length draws solid. A stroke with a point
    /// dash is tessellated as a continuous line: the unit-based dash fields
    /// of `line_geometry` must stay zero for it.
    pub dash_length_points: f32,
    pub dash_gap_points: f32,
    /// True when `dash_length_points` and `dash_gap_points` are tile units
    /// rather than points: a world-locked pattern, paint on the ground
    /// whose period is a length and must not re-flow with the camera or
    /// the serving tile level. See `TileLineStyle::dash_in_tile_units`.
    pub dash_in_tile_units: bool,
    /// Floor for a world-locked width. See
    /// `TileLineStyle::minimum_width_points`.
    pub minimum_width_points: f32,
    /// The camera zoom from which a point-locked width
    /// (`line_width_points`) is fixed on the ground instead of on screen.
    /// Up to this zoom the stroke is a symbol, as wide in points as the
    /// style says. Past it the stroke keeps the ground width those points
    /// had at this zoom, so it grows on screen with the map and never thins
    /// into a hairline against the buildings around it. Continuous in
    /// camera zoom by construction. Zero keeps the width in points at every
    /// zoom.
    pub point_width_world_lock_zoom: f32,
    /// The zoom ramp `line_width_points` comes in over. `None` holds the
    /// width at every zoom up to the world lock.
    pub point_width_ramp: Option<WidthRamp>,
    /// The share of the colour's alpha the stroke keeps on the globe, in
    /// `0...1`: one, the default, is the same stroke on both surfaces, and
    /// less is a veil the globe's view lays over it, so the ground stays
    /// the picture of a continent and the roads a net over it. The veil
    /// holds through the globe's view, the unroll into the plane included,
    /// is handed over to the plane's with the globe's air at the end of the
    /// unroll, and the plane never has it. Only the sphere's ribbons read
    /// it: a road of the street zooms is never on the sphere.
    pub sphere_alpha: f32,
    /// The colour the stroke wears on the globe, `None` for its own: an
    /// atlas's highway yellow over the painted globe where the plane draws
    /// asphalt. Handed over with `sphere_alpha`, unless
    /// `sphere_look_until_zoom` switches both at a zoom of its own.
    pub sphere_color: Option<Float3>,
    /// The camera zoom the stroke keeps its globe look up to, on the plane
    /// as well as on the globe: `sphere_color` and `sphere_alpha` below it,
    /// the stroke's own colour and alpha from it, switched at once with no
    /// blend, so the change lands where the style wants it rather than with
    /// the globe's air. `None`, the default, hands both over with the air
    /// and draws the plane in the stroke's own.
    pub sphere_look_until_zoom: Option<f32>,
    pub line_geometry: LineGeometryStyle,
}

impl LinePass {
    /// A stroke of a colour and a geometry, with every other knob at its
    /// default: no fade, world-locked, solid, the same on the globe.
    pub const fn new(key: u8, color: Float4, line_geometry: LineGeometryStyle) -> Self {
        LinePass {
            key,
            color,
            zoom_fade: ZoomFade::NONE,
            line_width_points: 0.0,
            dash_length_points: 0.0,
            dash_gap_points: 0.0,
            dash_in_tile_units: false,
            minimum_width_points: 0.0,
            point_width_world_lock_zoom: 0.0,
            point_width_ramp: None,
            sphere_alpha: 1.0,
            sphere_color: None,
            sphere_look_until_zoom: None,
            line_geometry,
        }
    }

    /// A stroke whose width is stated in on-screen points and held there at
    /// every zoom: by default coming in over the first zoom level off the
    /// whole-planet view (camera zoom 0 to 1) and opaque from there, butt
    /// ends, plain joins, the dash pattern in points, and a ribbon
    /// provisioned to host the width.
    pub fn point_locked(key: u8, color: Float4, width_points: f32) -> Self {
        LinePass {
            zoom_fade: ZoomFade::OVERVIEW,
            line_width_points: width_points,
            ..LinePass::new(
                key,
                color,
                LineGeometryStyle::new(width_points as f64 * FeatureStyle::POINT_LOCKED_RIBBON_UNITS_PER_POINT),
            )
        }
    }
}

/// A ground fill.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FillStyle {
    pub key: u8,
    pub color: Float4,
    /// How the fill appears or disappears with the camera zoom.
    pub zoom_fade: ZoomFade,
    /// A fill whose polygons with many holes (an ocean with its islands)
    /// are not tessellated as one polygon: the exterior draws as the fill
    /// and each hole as the background, so the tessellator never sees the
    /// hundreds of islands of a coastal tile at once.
    pub splits_complex_holes: bool,
    /// A fill that takes its place among the ground lines by its key. The
    /// ground draws every fill first and every line over them, so a plain
    /// fill lies under every river and ferry route whatever its key. A fill
    /// that must cover such lines (a bridge's deck over the river) draws
    /// with the lines instead, over those of a lower key and under those of
    /// a higher one, and over every plain fill.
    pub draws_among_ground_lines: bool,
    /// The colour the fill wears on the globe, `None` for its own: the
    /// day's ocean under a night palette, so the planet keeps its daylight
    /// while the plane is dark. It holds through the globe's view, the
    /// unroll into the plane included, and is handed over to the fill's own
    /// colour with the globe's air at the end of the unroll, the way a
    /// stroke's `LinePass::sphere_color` is. The alpha stays the fill's
    /// own. Only the sphere's fills read it: a fill of the street zooms is
    /// never on the sphere.
    pub sphere_color: Option<Float3>,
    /// The trees the fill grows on the flat map, `None` for none: a wood or
    /// a park stands the engine's tree over its polygons (`TreeFillStyle`).
    pub trees: Option<TreeFillStyle>,
    /// A fill no tree stands on, whatever fill it lies in: the water, a
    /// pond in a park, a pitch or a lawn. The trees of the fills around it
    /// keep off its polygons, an island in a lake excepted.
    pub excludes_trees: bool,
}

impl FillStyle {
    /// A plain fill of a colour.
    pub const fn new(key: u8, color: Float4) -> Self {
        FillStyle {
            key,
            color,
            zoom_fade: ZoomFade::NONE,
            splits_complex_holes: false,
            draws_among_ground_lines: false,
            sphere_color: None,
            trees: None,
            excludes_trees: false,
        }
    }
}

/// The trees a fill grows: the engine stands its tree model on a lattice
/// over the fill's polygons, one tree at most in every cell, each moved
/// within its cell, turned and sized by a hash of the cell, so the same
/// ground grows the same trees at every parse. The trees draw on the flat
/// map from the camera zoom the settings name, thinned at the first zooms,
/// and keep off the buildings' footprints and the roads. Baked into the
/// prepared tiles with the rest of the style, so a change re-parses them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", from = "TreeFillStyleRepr")]
pub struct TreeFillStyle {
    /// The distance between neighbouring trees in metres, the lattice's
    /// cell: 9 for a wood whose crowns touch, 20 and more for the scattered
    /// trees of a park.
    pub spacing_metres: f32,
    /// The height of the shortest and the tallest tree in metres: every
    /// tree takes its own height between the two.
    pub minimum_height_metres: f32,
    pub maximum_height_metres: f32,
    /// How far from a road's edge a tree keeps, in metres, beyond the
    /// road's own width as the style draws it: the verge no trunk stands
    /// on, whatever the fill says. Every road of the ground counts, a
    /// park's paths among them, not the tunnels and the bridges. 3 by
    /// default.
    pub road_clearance_metres: f32,
}

impl TreeFillStyle {
    /// The spacing at least half a metre, the heights not below zero and in
    /// order, the clearance not below zero.
    pub fn new(spacing_metres: f32, height_metres: std::ops::RangeInclusive<f32>, road_clearance_metres: f32) -> Self {
        let minimum = height_metres.start().max(0.0);
        TreeFillStyle {
            spacing_metres: spacing_metres.max(0.5),
            minimum_height_metres: minimum,
            maximum_height_metres: height_metres.end().max(minimum),
            road_clearance_metres: road_clearance_metres.max(0.0),
        }
    }

    /// The default verge from a road's edge, in metres.
    pub const DEFAULT_ROAD_CLEARANCE_METRES: f32 = 3.0;

    /// The lattice's salt: fills of different spacing hash their cells
    /// apart, so a park inside a wood grows its own trees.
    pub fn lattice(&self) -> u32 {
        self.spacing_metres.to_bits()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TreeFillStyleRepr {
    spacing_metres: f32,
    minimum_height_metres: f32,
    maximum_height_metres: f32,
    #[serde(default = "default_road_clearance")]
    road_clearance_metres: f32,
}

fn default_road_clearance() -> f32 {
    TreeFillStyle::DEFAULT_ROAD_CLEARANCE_METRES
}

impl From<TreeFillStyleRepr> for TreeFillStyle {
    fn from(repr: TreeFillStyleRepr) -> Self {
        TreeFillStyle::new(
            repr.spacing_metres,
            repr.minimum_height_metres..=repr.maximum_height_metres,
            repr.road_clearance_metres,
        )
    }
}

/// A tree that stands at a point the data maps it at (OpenStreetMap's
/// `natural=tree`): one tree of the engine's model there, sized and turned
/// by a hash of its place, among the trees the fills grow
/// (`TreeFillStyle`). The scattered trees keep off its crown, so a mapped
/// tree in a park is never doubled.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", from = "TreePointStyleRepr")]
pub struct TreePointStyle {
    /// The height of the shortest and the tallest mapped tree in metres:
    /// each takes its own between the two, since the data states none.
    pub minimum_height_metres: f32,
    pub maximum_height_metres: f32,
    /// How far from a mapped tree no scattered tree stands, in metres:
    /// about its crown's radius.
    pub crown_clearance_metres: f32,
}

impl TreePointStyle {
    /// The default clearance around a mapped tree's trunk, in metres.
    pub const DEFAULT_CROWN_CLEARANCE_METRES: f32 = 4.0;

    pub fn new(height_metres: std::ops::RangeInclusive<f32>, crown_clearance_metres: f32) -> Self {
        let minimum = height_metres.start().max(0.0);
        TreePointStyle {
            minimum_height_metres: minimum,
            maximum_height_metres: height_metres.end().max(minimum),
            crown_clearance_metres: crown_clearance_metres.max(0.0),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TreePointStyleRepr {
    minimum_height_metres: f32,
    maximum_height_metres: f32,
    #[serde(default = "default_crown_clearance")]
    crown_clearance_metres: f32,
}

fn default_crown_clearance() -> f32 {
    TreePointStyle::DEFAULT_CROWN_CLEARANCE_METRES
}

impl From<TreePointStyleRepr> for TreePointStyle {
    fn from(repr: TreePointStyleRepr) -> Self {
        TreePointStyle::new(repr.minimum_height_metres..=repr.maximum_height_metres, repr.crown_clearance_metres)
    }
}

/// A line that is not a road: one stroke, with no place in the road order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineStyle {
    pub pass: LinePass,
    pub placement: LinePlacement,
    /// Whether areal geometry a source ships under this style is filled
    /// with the stroke's colour. False draws only lines: a border style
    /// must not fill the reservations that arrive as polygons in the same
    /// layer.
    pub fills_areas: bool,
}

impl LineStyle {
    /// A ground line that fills the areas a source ships under it.
    pub const fn new(pass: LinePass) -> Self {
        LineStyle { pass, placement: LinePlacement::Ground, fills_areas: true }
    }

    /// A ground line that draws only the outlines of areal geometry.
    pub const fn outlines(pass: LinePass) -> Self {
        LineStyle { pass, placement: LinePlacement::Ground, fills_areas: false }
    }
}

/// A road: a line drawn in the road order. One stroke per role, bottom to
/// top (`shadow`, `casing`, `fill`, the `paint` on the surface, `overlay`),
/// plus what the engine's road work needs to know about the road as the
/// style sees it: where it sorts, which tier it draws in, what figure is
/// stamped along it, the name laid along it and the signs of its routes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RoadStyle {
    pub shadow: Option<LinePass>,
    pub casing: Option<LinePass>,
    pub fill: Option<LinePass>,
    /// The strokes painted on the surface, drawn in the `Detail` role above
    /// every fill: the stroke a decoration is stamped in.
    pub paint: Vec<LinePass>,
    pub overlay: Option<LinePass>,
    /// The road's place among the roads of its level: higher draws over
    /// lower.
    pub class_priority: i32,
    /// Where the road draws in the stack (see `RoadLevel`).
    pub level: RoadLevel,
    /// The network the road draws in on the ground (see `RoadTier`).
    pub tier: RoadTier,
    /// The figure stamped along the geometry instead of a plain stroke.
    pub decoration: RoadDecorationKind,
    /// The name laid along the road, `None` for a road that carries none.
    pub label: Option<LabelTextStyle>,
    /// The signs of the routes the road carries, `None` for a road that
    /// carries none.
    pub shields: Option<RouteShieldStyle>,
    pub placement: LinePlacement,
}

/// One stroke of a road with its role.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadPass {
    pub role: RoadPassRole,
    pub pass: LinePass,
}

impl RoadStyle {
    /// Every stroke the road draws, bottom to top.
    pub fn ordered_passes(&self) -> Vec<RoadPass> {
        let mut passes = Vec::with_capacity(4 + self.paint.len());
        if let Some(pass) = self.shadow {
            passes.push(RoadPass { role: RoadPassRole::Shadow, pass });
        }
        if let Some(pass) = self.casing {
            passes.push(RoadPass { role: RoadPassRole::Casing, pass });
        }
        if let Some(pass) = self.fill {
            passes.push(RoadPass { role: RoadPassRole::Fill, pass });
        }
        for pass in &self.paint {
            passes.push(RoadPass { role: RoadPassRole::Detail, pass: *pass });
        }
        if let Some(pass) = self.overlay {
            passes.push(RoadPass { role: RoadPassRole::Overlay, pass });
        }
        passes
    }

    /// The road's identity within a tile: the fill's key, or the first
    /// stroke's for a road that is paint alone.
    pub fn key(&self) -> u8 {
        if let Some(fill) = &self.fill {
            return fill.key;
        }
        self.shadow
            .or(self.casing)
            .or_else(|| self.paint.first().copied())
            .or(self.overlay)
            .map(|pass| pass.key)
            .unwrap_or(0)
    }
}

/// A building: the footprint raised to the heights the schema reading
/// states for it (`FeatureFacts::Building`). The footprint also draws as a
/// ground fill in the same colour, whether or not it rises.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExtrusionStyle {
    pub key: u8,
    pub color: Float4,
    /// How metres become tile units at `anchor_zoom`. The parser doubles
    /// the scale per zoom level above it and halves it below.
    pub height_scale: f32,
    pub anchor_zoom: i32,
    /// The height in metres of a building the reading states no height
    /// for. Zero leaves it flat.
    pub fallback_height: f32,
    /// The radius, in metres, the building's edges are rounded by: every
    /// corner of its footprint and the rim of its roof take a narrow bevel
    /// whose shading runs from one face into the next, so a block reads as
    /// a soft solid rather than a sharp box. Zero keeps the sharp box. A
    /// building too small or too low for the radius takes a smaller one.
    /// Baked into the prepared tiles with the rest of the style.
    pub edge_radius: f32,
}

impl ExtrusionStyle {
    /// A building of a colour: a scale of one at zoom 16, flat without a
    /// height, sharp edges.
    pub const fn new(key: u8, color: Float4) -> Self {
        ExtrusionStyle { key, color, height_scale: 1.0, anchor_zoom: 16, fallback_height: 0.0, edge_radius: 0.0 }
    }
}

/// A point label. The text is the name the schema reading states, in the
/// map's language. This says how it is drawn and how important it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointLabelStyle {
    pub key: u8,
    pub text: LabelTextStyle,
    /// The label's importance among the labels of the map, lower first: the
    /// order the runtime reveals labels in as room appears, and
    /// deduplicates them in. The built-in style reads it from the tiles'
    /// rank.
    pub rank: i32,
    /// The label's precedence when two labels overlap, lower wins. Usually
    /// the rank offset by layer, so a place name beats a shop's.
    pub collision_rank: i32,
    /// Minimum camera zoom for the label (0 = always visible). Travels with
    /// the label to runtime, where it is compared against the camera zoom.
    pub min_camera_zoom: f32,
    /// The camera zoom the label goes away at: shown while the camera zoom
    /// is below it, hidden from it on. Infinity shows the label at every
    /// zoom past its minimum. A country's name is read over the country and
    /// says nothing among the streets of one of its towns.
    pub max_camera_zoom: f32,
    /// The sprite drawn beside the text, `None` for text alone.
    pub icon: Option<PoiSpriteIcon>,
    /// Upright on the screen, or painted on the map. A label painted on the
    /// map draws its text alone: the icon, the ranks and the minimum camera
    /// zoom belong to the screen labels.
    pub placement: LabelPlacement,
    /// Whether a screen label standing inside an extruded building belongs
    /// to it. The label takes the roof over its anchor, the top of the
    /// tallest volume whose footprint holds it (or of the model drawn in
    /// its place), wherever the frame draws that building. A label naming
    /// the building itself, the same OSM element as the building, rises to
    /// the roof, and one naming something inside it stays on the ground. On
    /// the ground where the frame draws no buildings (the globe, a zoom
    /// coarser than the building tiles).
    pub stands_on_roof: bool,
    /// Whether the label is local detail, shown only near the camera: the
    /// house numbers, a memorial's plaque, a ticket office, the things one
    /// reads standing next to them. It keeps to the three by three tiles
    /// around the one the camera looks at, and within them to the settings'
    /// local detail distance from the camera, so on a tilted map the detail
    /// up the street, where it would be a scatter of small text saying
    /// nothing, is left out.
    pub is_local: bool,
}

/// The knobs of a point label beside its key and text, for
/// `FeatureStyle::point_label`. The default is rank 0 colliding at its
/// rank, shown at every zoom, no sprite, upright on the screen, on the
/// ground, not local.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointLabelOptions {
    pub rank: i32,
    /// `None` collides at the rank.
    pub collision_rank: Option<i32>,
    pub min_camera_zoom: f32,
    pub max_camera_zoom: f32,
    pub icon: Option<PoiSpriteIcon>,
    pub placement: LabelPlacement,
    pub stands_on_roof: bool,
    pub is_local: bool,
}

impl Default for PointLabelOptions {
    fn default() -> Self {
        PointLabelOptions {
            rank: 0,
            collision_rank: None,
            min_camera_zoom: 0.0,
            max_camera_zoom: f32::INFINITY,
            icon: None,
            placement: LabelPlacement::Screen,
            stands_on_roof: false,
            is_local: false,
        }
    }
}

impl PointLabelStyle {
    /// A label under a key with the text style as given (no floor, no key
    /// applied to the text: see `FeatureStyle::point_label` for those).
    pub fn new(key: u8, text: LabelTextStyle, options: PointLabelOptions) -> Self {
        PointLabelStyle {
            key,
            text,
            rank: options.rank,
            collision_rank: options.collision_rank.unwrap_or(options.rank),
            min_camera_zoom: options.min_camera_zoom,
            max_camera_zoom: options.max_camera_zoom,
            icon: options.icon,
            placement: options.placement,
            stands_on_roof: options.stands_on_roof,
            is_local: options.is_local,
        }
    }
}

/// How one feature draws: the style's whole answer, which the parser bakes
/// and the renderer draws. One case per drawing mode, each carrying only
/// the knobs that mode has: a reader matches on the case and cannot read a
/// label's fields off a fill. What the feature is (a road in a tunnel, a
/// building of some height) is not in here: that is the schema reading's
/// `FeatureFacts`, which the parser carries next to the style.
///
/// The factories below cover the common cases. The payload constructors
/// expose every knob.
// A road carries its strokes inline: the style is made and consumed one
// feature at a time, never stored in bulk, so the size of the largest case
// costs nothing a box would save, and a box would cost an allocation.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Default)]
pub enum FeatureStyle {
    /// Nothing is drawn.
    #[default]
    Hidden,
    Fill(FillStyle),
    Line(LineStyle),
    Road(RoadStyle),
    Extrusion(ExtrusionStyle),
    PointLabel(PointLabelStyle),
    /// A tree at the feature's point (`TreePointStyle`).
    Tree(TreePointStyle),
}

impl FeatureStyle {
    /// The tessellated ribbon a point-locked stroke is hosted on, in tile
    /// units per layout point: wide enough that the screen-space edge the
    /// shader resolves never runs past the geometry at the zooms the stroke
    /// is meant for.
    pub const POINT_LOCKED_RIBBON_UNITS_PER_POINT: f64 = 12.0;

    /// The style's identity within a tile, 0 for `Hidden` and a `Tree`,
    /// which bake no style of their own.
    pub fn key(&self) -> u8 {
        match self {
            FeatureStyle::Hidden | FeatureStyle::Tree(_) => 0,
            FeatureStyle::Fill(fill) => fill.key,
            FeatureStyle::Line(line) => line.pass.key,
            FeatureStyle::Road(road) => road.key(),
            FeatureStyle::Extrusion(extrusion) => extrusion.key,
            FeatureStyle::PointLabel(label) => label.key,
        }
    }

    pub fn fill_style(&self) -> Option<&FillStyle> {
        match self {
            FeatureStyle::Fill(fill) => Some(fill),
            _ => None,
        }
    }

    pub fn line_style(&self) -> Option<&LineStyle> {
        match self {
            FeatureStyle::Line(line) => Some(line),
            _ => None,
        }
    }

    /// The style as a road: a `Road` as it is, and a plain `Line` as a road
    /// of one fill stroke with no class, so a line on a road layer takes
    /// the road path with the lowest priority.
    pub fn road_style(&self) -> Option<std::borrow::Cow<'_, RoadStyle>> {
        match self {
            FeatureStyle::Road(road) => Some(std::borrow::Cow::Borrowed(road)),
            FeatureStyle::Line(line) => Some(std::borrow::Cow::Owned(RoadStyle {
                fill: Some(line.pass),
                placement: line.placement,
                ..RoadStyle::default()
            })),
            _ => None,
        }
    }

    pub fn extrusion_style(&self) -> Option<&ExtrusionStyle> {
        match self {
            FeatureStyle::Extrusion(extrusion) => Some(extrusion),
            _ => None,
        }
    }

    pub fn point_label_style(&self) -> Option<&PointLabelStyle> {
        match self {
            FeatureStyle::PointLabel(label) => Some(label),
            _ => None,
        }
    }

    pub fn tree_point_style(&self) -> Option<&TreePointStyle> {
        match self {
            FeatureStyle::Tree(tree) => Some(tree),
            _ => None,
        }
    }

    // The common drawing modes.

    /// A fill, drawn at its full alpha unless `zoom_fade` says otherwise.
    pub fn polygon(key: u8, color: Float4, zoom_fade: ZoomFade) -> FeatureStyle {
        FeatureStyle::Fill(FillStyle { zoom_fade, ..FillStyle::new(key, color) })
    }

    /// A line of a width in tile units.
    pub fn line(key: u8, color: Float4, width: f32, zoom_fade: ZoomFade) -> FeatureStyle {
        FeatureStyle::Line(LineStyle::new(LinePass {
            zoom_fade,
            ..LinePass::new(key, color, LineGeometryStyle::new(width.max(0.0) as f64))
        }))
    }

    /// A line whose width is stated in on-screen points and held there at
    /// every zoom: the drawing mode the built-in style uses for country
    /// borders and for the overview road skeleton. The stroke is opaque from
    /// the first frame it is visible, ends in butt caps with plain joins,
    /// and an optional dash pattern is stated in points too, so it reads as
    /// dashes at every zoom. Suited to symbolic lines (borders, networks
    /// over a country view) whose weight is a design decision rather than a
    /// width on the ground. `line` stays the world-locked line whose width
    /// lives in tile units. Areal geometry a source ships under this mode
    /// is not filled: only the outlines draw. The usual fade is
    /// `ZoomFade::OVERVIEW`.
    pub fn point_locked_line(
        key: u8,
        color: Float4,
        width_points: f32,
        dash_length_points: f32,
        dash_gap_points: f32,
        zoom_fade: ZoomFade,
        sphere_color: Option<Float3>,
    ) -> FeatureStyle {
        FeatureStyle::Line(LineStyle::outlines(LinePass {
            zoom_fade,
            dash_length_points: dash_length_points.max(0.0),
            dash_gap_points: dash_gap_points.max(0.0),
            sphere_color,
            ..LinePass::point_locked(key, color, width_points.max(0.0))
        }))
    }

    /// A building: the footprint raised to the heights the schema reading
    /// states for it (`FeatureFacts::Building`). `height_scale`,
    /// `anchor_zoom` and `fallback_height` say how metres become tile units
    /// and what a building without a height gets, `edge_radius` how round
    /// its edges are, in metres. A feature the reading found to be no
    /// building stays a flat fill.
    pub fn extruded_polygon(extrusion: ExtrusionStyle) -> FeatureStyle {
        FeatureStyle::Extrusion(extrusion)
    }

    /// A point label. The text is the name the schema reading states, in
    /// the map's language. This says how it is drawn, how important it is
    /// (`rank`, lower first, and `collision_rank`, which defaults to the
    /// rank), from which camera zoom, which sprite stands beside it,
    /// whether it stands on the screen or lies on the map, and whether it
    /// is local detail shown only near the camera. The text style takes the
    /// feature style's key and the readable floor of its size.
    pub fn point_label(key: u8, text_style: LabelTextStyle, options: PointLabelOptions) -> FeatureStyle {
        FeatureStyle::PointLabel(PointLabelStyle::new(key, Self::keyed(text_style, key), options))
    }

    /// A road drawn as a line of a width in tile units, with its name laid
    /// along it.
    pub fn road_label(key: u8, color: Float4, width: f32, text_style: LabelTextStyle) -> FeatureStyle {
        FeatureStyle::Road(RoadStyle {
            fill: Some(LinePass::new(key, color, LineGeometryStyle::new(width.max(0.0) as f64))),
            label: Some(Self::keyed(text_style, key)),
            ..RoadStyle::default()
        })
    }

    /// The text style under the feature style's key, with the size floor
    /// applied.
    fn keyed(text_style: LabelTextStyle, key: u8) -> LabelTextStyle {
        LabelTextStyle { key: key as i32, size_points: LabelTypeScale::clamped(text_style.size_points), ..text_style }
    }
}
