// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use serde::{Deserialize, Serialize};

use crate::style::LabelFontWeight;
use crate::Float3;

/// The outline of a route sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[repr(u8)]
pub enum RouteShieldShape {
    /// A rectangle with corners rounded by `corner_radius_em`: the plate
    /// most countries number their roads on.
    #[default]
    Rectangle = 0,
    /// A rectangle with fully round ends.
    Capsule = 1,
    /// A heraldic shield: a flat top, straight sides, and a bottom that
    /// comes to a point. The United States' Interstate and US highway
    /// signs.
    Escutcheon = 2,
}

impl RouteShieldShape {
    pub fn raw_value(self) -> u8 {
        self as u8
    }
}

/// The sign a route number is drawn on: its outline, its colours and the
/// colour of the number. The shapes are drawn by the shader from their
/// distance fields, so a sign costs one quad and no image, at any size.
///
/// Built with `new` and the chained setters, which hold every value to its
/// range the way the engine's initializer does. Deserialization goes
/// through the same setters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", from = "RouteShieldAppearanceRepr")]
pub struct RouteShieldAppearance {
    pub shape: RouteShieldShape,
    pub fill_color: Float3,
    pub text_color: Float3,
    pub border_color: Float3,
    /// The width of the border inside the outline, as a fraction of the em.
    /// Zero draws none.
    pub border_em: f32,
    /// The corner radius of a `Rectangle` (and of an `Escutcheon`'s top
    /// corners), as a fraction of the em.
    pub corner_radius_em: f32,
    /// The room between the number and each side of the sign, as a
    /// fraction of the em. A sign is never narrower than it is tall.
    pub padding_em: f32,
    /// A band across the top of the sign in its own colour (the red crown
    /// of an Interstate sign), `None` for a sign in one colour. The number
    /// sits in the part below it.
    pub header_color: Option<Float3>,
    /// The band's height as a fraction of the sign's.
    pub header_fraction: f32,
}

impl RouteShieldAppearance {
    /// A rectangular plate of a colour with a number of a colour, its
    /// border the plate's own colour and of no width, corners of a fifth of
    /// the em, a padding of 0.35 em, no band.
    pub fn new(fill_color: Float3, text_color: Float3) -> Self {
        RouteShieldAppearance {
            shape: RouteShieldShape::Rectangle,
            fill_color,
            text_color,
            border_color: fill_color,
            border_em: 0.0,
            corner_radius_em: 0.2,
            padding_em: 0.35,
            header_color: None,
            header_fraction: 0.25,
        }
    }

    pub fn shape(mut self, shape: RouteShieldShape) -> Self {
        self.shape = shape;
        self
    }

    /// The border's colour and width in ems, the width not below zero.
    pub fn border(mut self, color: Float3, width_em: f32) -> Self {
        self.border_color = color;
        self.border_em = width_em.max(0.0);
        self
    }

    pub fn corner_radius_em(mut self, radius_em: f32) -> Self {
        self.corner_radius_em = radius_em.max(0.0);
        self
    }

    pub fn padding_em(mut self, padding_em: f32) -> Self {
        self.padding_em = padding_em.max(0.0);
        self
    }

    pub fn header(mut self, color: Option<Float3>) -> Self {
        self.header_color = color;
        self
    }

    /// The band's height, held to 0...0.5 of the sign.
    pub fn header_fraction(mut self, fraction: f32) -> Self {
        self.header_fraction = fraction.clamp(0.0, 0.5);
        self
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RouteShieldAppearanceRepr {
    #[serde(default)]
    shape: RouteShieldShape,
    fill_color: Float3,
    text_color: Float3,
    #[serde(default)]
    border_color: Option<Float3>,
    #[serde(default)]
    border_em: f32,
    #[serde(default = "default_corner_radius")]
    corner_radius_em: f32,
    #[serde(default = "default_padding")]
    padding_em: f32,
    #[serde(default)]
    header_color: Option<Float3>,
    #[serde(default = "default_header_fraction")]
    header_fraction: f32,
}

fn default_corner_radius() -> f32 {
    0.2
}
fn default_padding() -> f32 {
    0.35
}
fn default_header_fraction() -> f32 {
    0.25
}

impl From<RouteShieldAppearanceRepr> for RouteShieldAppearance {
    fn from(repr: RouteShieldAppearanceRepr) -> Self {
        RouteShieldAppearance::new(repr.fill_color, repr.text_color)
            .shape(repr.shape)
            .border(repr.border_color.unwrap_or(repr.fill_color), repr.border_em)
            .corner_radius_em(repr.corner_radius_em)
            .padding_em(repr.padding_em)
            .header(repr.header_color)
            .header_fraction(repr.header_fraction)
    }
}

/// One route's sign: the number and what it is drawn on.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteShield {
    pub text: String,
    pub appearance: RouteShieldAppearance,
}

/// The route signs a road carries: the numbers of the routes it is part
/// of, each on its own sign, side by side, upright on the screen whatever
/// way the road runs, and repeated along the road. They take part in the
/// label collisions as one label.
///
/// The style decides the signs from the road's facts (`RoadFacts::routes`):
/// which routes are signed, in which order, and on which sign. The engine
/// draws what it is given.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteShieldStyle {
    /// The signs, left to right. Empty draws nothing.
    pub shields: Vec<RouteShield>,
    /// The em size of the numbers in layout points. The sign is sized from
    /// it.
    pub size_points: f32,
    pub weight: LabelFontWeight,
    /// The height of a sign as a fraction of the em. An `Escutcheon` is
    /// drawn a fifth taller, and at least as tall as it is wide, so its
    /// point has room below the number.
    pub plate_height_em: f32,
    /// The room between two signs side by side, as a fraction of the em.
    pub gap_em: f32,
    /// The least distance, in layout points at the tile's own zoom, between
    /// two copies of the same signs along the roads of a tile. A road
    /// shorter than that inside a tile carries one copy.
    pub spacing_points: f32,
    /// The signs' importance among the labels, lower first, as a point
    /// label's `rank`.
    pub rank: i32,
    /// The signs' precedence when they overlap another label, lower wins,
    /// as a point label's `collision_rank`.
    pub collision_rank: i32,
    /// Minimum camera zoom for the signs (0 = always visible).
    pub min_camera_zoom: f32,
}

impl RouteShieldStyle {
    /// The signs at 11 points in the bold face, plates 1.45 em tall with a
    /// quarter em between them, 200 points apart, rank 0, every zoom.
    pub fn new(shields: Vec<RouteShield>) -> Self {
        RouteShieldStyle {
            shields,
            size_points: 11.0,
            weight: LabelFontWeight::Bold,
            plate_height_em: 1.45,
            gap_em: 0.25,
            spacing_points: 200.0,
            rank: 0,
            collision_rank: 0,
            min_camera_zoom: 0.0,
        }
    }

    pub fn size_points(mut self, size_points: f32) -> Self {
        self.size_points = size_points;
        self
    }

    pub fn weight(mut self, weight: LabelFontWeight) -> Self {
        self.weight = weight;
        self
    }

    pub fn plate_height_em(mut self, plate_height_em: f32) -> Self {
        self.plate_height_em = plate_height_em.max(0.0);
        self
    }

    pub fn gap_em(mut self, gap_em: f32) -> Self {
        self.gap_em = gap_em.max(0.0);
        self
    }

    pub fn spacing_points(mut self, spacing_points: f32) -> Self {
        self.spacing_points = spacing_points.max(0.0);
        self
    }

    /// The rank, and the collision rank (`None` for the rank itself).
    pub fn rank(mut self, rank: i32, collision_rank: Option<i32>) -> Self {
        self.rank = rank;
        self.collision_rank = collision_rank.unwrap_or(rank);
        self
    }

    pub fn min_camera_zoom(mut self, zoom: f32) -> Self {
        self.min_camera_zoom = zoom;
        self
    }
}
