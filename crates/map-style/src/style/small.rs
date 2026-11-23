// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The small values of the style: the line geometry the tessellator bakes,
//! where a label is drawn, the road decorations, the POI sprites and the
//! readable floor of the type scale.

use serde::Serialize;

/// What the tessellator bakes for a line: its width and the shape of its
/// ends, joins and dashes, all in tile units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineGeometryStyle {
    pub line_width: f64,
    pub line_cap_round: bool,
    pub line_join_round: bool,
    pub dash_length: f64,
    pub dash_gap: f64,
    pub dash_resets_per_segment: bool,
}

impl LineGeometryStyle {
    /// A line of a width, with butt ends, plain joins and no dash.
    pub const fn new(line_width: f64) -> Self {
        LineGeometryStyle {
            line_width,
            line_cap_round: false,
            line_join_round: false,
            dash_length: 0.0,
            dash_gap: 0.0,
            dash_resets_per_segment: false,
        }
    }

    /// A line of a width with the ends and the joins given.
    pub const fn rounded(line_width: f64, line_cap_round: bool, line_join_round: bool) -> Self {
        LineGeometryStyle { line_cap_round, line_join_round, ..LineGeometryStyle::new(line_width) }
    }

    pub fn uses_dash_pattern(&self) -> bool {
        self.dash_length > 0.0 && self.dash_gap > 0.0
    }
}

/// Where a point label is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LabelPlacement {
    /// Upright on the screen at its style's point size, whatever the camera
    /// does, and placed among the other labels by the collisions.
    #[default]
    Screen,
    /// Painted on the map itself: the text lies on the ground, turns and
    /// tilts with it, curves with the globe, and grows and shrinks with the
    /// zoom like the geometry around it. It takes no part in the
    /// collisions, and the screen labels draw over it. Suited to the name
    /// of a large area, an ocean, a sea or a lake.
    Surface(SurfaceLabelPlacement),
}

/// How a label painted on the map scales and when it shows.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceLabelPlacement {
    /// The camera zoom from which the text grows with the map: there it is
    /// as large on screen as its style's point size, one zoom deeper twice
    /// as large. It never draws smaller than that size, so under this zoom,
    /// and wherever the ground itself is small on screen (the globe near a
    /// pole), it keeps its point size and stays readable.
    pub reference_zoom: f64,
    /// The camera zooms the text shows at, whole: it appears and goes away
    /// at the ends without a fade.
    pub minimum_zoom: f64,
    pub maximum_zoom: f64,
    /// Extra room after every letter, in ems: a name spaced out over the
    /// area it names, the way an atlas letters a sea.
    pub letter_spacing_em: f32,
}

impl SurfaceLabelPlacement {
    /// Shown at every zoom, with no tracking.
    pub fn new(reference_zoom: f64) -> Self {
        SurfaceLabelPlacement {
            reference_zoom,
            minimum_zoom: f64::NEG_INFINITY,
            maximum_zoom: f64::INFINITY,
            letter_spacing_em: 0.0,
        }
    }

    /// Whether the text shows at a camera zoom.
    pub fn is_visible_at_zoom(&self, zoom: f64) -> bool {
        zoom >= self.minimum_zoom && zoom < self.maximum_zoom
    }

    /// Whether a tile of this zoom ever shows the text. The engine draws a
    /// tile for the camera zooms from its own zoom up to the next.
    pub fn is_visible_on_tile_zoom(&self, tile_zoom: i32) -> bool {
        let lowest = tile_zoom as f64;
        lowest + 1.0 > self.minimum_zoom && lowest < self.maximum_zoom
    }
}

/// A decoration the separate-road path stamps along a line on top of its
/// ribbon. Every figure's dimensions are the style's: each case carries
/// them, with the defaults the built-in style draws with.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum RoadDecorationKind {
    #[default]
    None,
    /// Arrows along a one-way road, in the paint stroke.
    OnewayArrow(OnewayArrowDecoration),
}

/// The arrows along a one-way road, in tile units: how far apart they
/// repeat, the shortest fragment that carries one, and the turn an arrow
/// keeps clear of.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OnewayArrowDecoration {
    /// The distance between arrows along the road, in tile units.
    pub repeat_step: f32,
    /// A fragment shorter than this, or than the stroke width times
    /// `minimum_fragment_length_factor`, carries no arrow.
    pub minimum_fragment_length: f32,
    pub minimum_fragment_length_factor: f32,
    /// An arrow within its own length of a turn sharper than this is not
    /// placed.
    pub turn_threshold_radians: f32,
}

impl Default for OnewayArrowDecoration {
    fn default() -> Self {
        OnewayArrowDecoration {
            repeat_step: 420.0,
            minimum_fragment_length: 96.0,
            minimum_fragment_length_factor: 4.5,
            turn_threshold_radians: std::f32::consts::PI / 4.0,
        }
    }
}

/// The sprites the engine can draw beside a point label. Which one a
/// feature takes is the style's decision (`PointLabelStyle::icon`). How
/// each is drawn is the engine's sprite atlas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PoiSpriteIcon {
    Restaurant,
    Cafe,
    Bar,
    Park,
    Museum,
    Hospital,
    School,
    Airport,
    Stadium,
    Hotel,
    Shopping,
    GasStation,
    Pharmacy,
    Viewpoint,
    Train,
    Transit,
    Parking,
    Bank,
    Theatre,
    Toilets,
    Civic,
    Worship,
    /// The plain marker of a POI whose category has no symbol of its own.
    Marker,
}

impl PoiSpriteIcon {
    /// Every sprite, in the order declared (the atlas's order).
    pub const ALL: [PoiSpriteIcon; 23] = [
        PoiSpriteIcon::Restaurant,
        PoiSpriteIcon::Cafe,
        PoiSpriteIcon::Bar,
        PoiSpriteIcon::Park,
        PoiSpriteIcon::Museum,
        PoiSpriteIcon::Hospital,
        PoiSpriteIcon::School,
        PoiSpriteIcon::Airport,
        PoiSpriteIcon::Stadium,
        PoiSpriteIcon::Hotel,
        PoiSpriteIcon::Shopping,
        PoiSpriteIcon::GasStation,
        PoiSpriteIcon::Pharmacy,
        PoiSpriteIcon::Viewpoint,
        PoiSpriteIcon::Train,
        PoiSpriteIcon::Transit,
        PoiSpriteIcon::Parking,
        PoiSpriteIcon::Bank,
        PoiSpriteIcon::Theatre,
        PoiSpriteIcon::Toilets,
        PoiSpriteIcon::Civic,
        PoiSpriteIcon::Worship,
        PoiSpriteIcon::Marker,
    ];

    /// The sprite's name, the raw value the engine's atlas is keyed by.
    pub fn name(&self) -> &'static str {
        match self {
            PoiSpriteIcon::Restaurant => "restaurant",
            PoiSpriteIcon::Cafe => "cafe",
            PoiSpriteIcon::Bar => "bar",
            PoiSpriteIcon::Park => "park",
            PoiSpriteIcon::Museum => "museum",
            PoiSpriteIcon::Hospital => "hospital",
            PoiSpriteIcon::School => "school",
            PoiSpriteIcon::Airport => "airport",
            PoiSpriteIcon::Stadium => "stadium",
            PoiSpriteIcon::Hotel => "hotel",
            PoiSpriteIcon::Shopping => "shopping",
            PoiSpriteIcon::GasStation => "gasStation",
            PoiSpriteIcon::Pharmacy => "pharmacy",
            PoiSpriteIcon::Viewpoint => "viewpoint",
            PoiSpriteIcon::Train => "train",
            PoiSpriteIcon::Transit => "transit",
            PoiSpriteIcon::Parking => "parking",
            PoiSpriteIcon::Bank => "bank",
            PoiSpriteIcon::Theatre => "theatre",
            PoiSpriteIcon::Toilets => "toilets",
            PoiSpriteIcon::Civic => "civic",
            PoiSpriteIcon::Worship => "worship",
            PoiSpriteIcon::Marker => "marker",
        }
    }
}

/// Rules a map style's label sizes follow.
///
/// Label sizes are layout points, so the same value has to stay readable on
/// a dense phone display and on a desktop one. That puts a floor under the
/// type scale: below it a label is present on screen without being
/// readable, which is worse than not drawing it, because it still takes
/// collision space from the labels around it.
pub struct LabelTypeScale;

impl LabelTypeScale {
    /// Smallest em size a label may be drawn at. Matches the platform floor
    /// for readable interface type: at this size the font's x-height is
    /// about six points.
    pub const MINIMUM_SIZE_POINTS: f32 = 11.0;

    /// Applies the floor to a size that a style computed itself (typically
    /// from the tile zoom).
    pub fn clamped(size_points: f32) -> f32 {
        size_points.max(Self::MINIMUM_SIZE_POINTS)
    }
}
