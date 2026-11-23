// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The roads: the `roads` layer's lines, from the overview strokes of a
//! country view to the symbols of a street, with the casing and the fill of
//! each, the name laid along a street, the signs of its routes and the
//! arrows of a one-way street. How wide a road is: a road is a symbol, the
//! theme states its width on screen in points per class
//! (`RoadMetrics::symbol_width_points`), the shader draws exactly that many
//! points until the world lock zoom and the ground width those points had
//! there from then on. Nothing here reads a lane count or a width off the
//! tiles.

use crate::schema::{FeatureProperties, RoadFacts, RoadStructure};
use crate::style::default::labels::LabelBand;
use crate::style::default::rules::parse_bool_value;
use crate::style::default::{ProtomapsBasemapDefaultMapStyle, RoadClass, RoadLayerStyles};
use crate::style::{
    FeatureStyle, LabelTextStyle, LabelTypeScale, LineGeometryStyle, LinePass, LineStyle, OnewayArrowDecoration,
    RoadDecorationKind, RoadStyle, RouteShieldStyle, WidthRamp, ZoomFade,
};
use crate::Float4;

/// One class's stroke over a country or region view, in on-screen points,
/// so the ladder is a design decision rather than a property of the tile
/// scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverviewRoadStroke {
    pub fill_key: u8,
    pub priority: i32,
}

impl ProtomapsBasemapDefaultMapStyle {
    /// The ferry route's stroke: a dashed line a little heavier than a
    /// regional border, in points.
    pub const FERRY_WIDTH_POINTS: f32 = 1.2;
    pub const FERRY_DASH_POINTS: f32 = 6.0;
    pub const FERRY_GAP_POINTS: f32 = 4.0;

    /// The opacity of everything a vehicular tunnel draws: its ribbon at
    /// every zoom. A tunnel is a plain fill with this alpha and nothing
    /// else, no kerb, no dash, no paint, so the ground shows through it in
    /// whichever palette the map wears and the road network stays joined
    /// across it without pretending to be at grade. Eighty percent
    /// transparent: the tunnel is a hint of where the road goes, not a
    /// road.
    pub const TUNNEL_FILL_OPACITY: f32 = 0.2;

    /// The last tile zoom of the overview era: through it a road is the
    /// bare overview stroke (see `overview_road_stroke`). From the next tile
    /// level the same symbol draws by the street era's rules, with its
    /// casing and its paint.
    pub const OVERVIEW_ROAD_MAXIMUM_TILE_ZOOM: i32 = 11;

    /// From this tile zoom a road takes the engine's road path (stitched
    /// across features, sorted by structure and class, drawn over the whole
    /// ground) even while it is still a symbol stroke. Below it the stroke
    /// is a plain ground line. z8 is where the engine used to switch on its
    /// own, kept so the picture does not change. The symbol era runs through
    /// z11, so this could move to z12 once that has been looked at.
    pub const ROAD_PATH_MINIMUM_TILE_ZOOM: i32 = 8;

    /// The camera zoom from which a road's casing shows: below it the fill
    /// is too narrow for an edge to read, and the kerb only muddies the
    /// antialiasing. The casing pass carries it as its fade band, so it
    /// eases in over the following zoom level, continuous with the camera.
    pub const CASING_MINIMUM_CAMERA_ZOOM: i32 = 16;

    /// The casing of a road's symbol, per side, in points: the same margin
    /// the theme's casing switch (`RoadMetrics::draws_casing`) adds around
    /// the overview stroke, so the tile level where the eras hand over
    /// changes nothing on screen.
    pub const ROAD_CASING_POINTS_PER_SIDE: f64 = 1.0;

    /// The paint on the asphalt shows wherever the tile states it.
    pub const ROAD_MARKING_ZOOM_FADE: ZoomFade = ZoomFade::NONE;

    /// The stroke the arrow figure is sized from, in tile units: the
    /// figure's length and width follow it, and on the z14 and z15 tiles
    /// that carry the arrows a unit is a few decimetres, so a narrow stroke
    /// keeps the figure a few metres long.
    pub const ROAD_MARKING_RIBBON_WIDTH: f64 = 8.0;

    /// The railway's stroke: a dashed symbol in points, like every road,
    /// held on screen at every zoom.
    pub const RAIL_WIDTH_POINTS: f32 = 1.5;
    pub const RAIL_DASH_POINTS: f32 = 6.0;
    pub const RAIL_GAP_POINTS: f32 = 6.0;

    /// The class a road of the basemap draws as, from its `kind` and
    /// `kind_detail`. Every line of the layer draws: a sidewalk and a
    /// crossing are paths, and a kind the style does not name (an aerialway,
    /// a pier) is `Other`. The ferries, the aeroways and the railways are
    /// handled before this is asked, since they are not roads of any class.
    pub fn road_class(kind: Option<&str>, kind_detail: Option<&str>) -> RoadClass {
        let detail = kind_detail.map(|detail| detail.strip_suffix("_link").unwrap_or(detail));
        match kind {
            Some("highway") => RoadClass::Motorway,
            Some("major_road") => match detail {
                Some("trunk") => RoadClass::Trunk,
                Some("primary") => RoadClass::Primary,
                Some("secondary") => RoadClass::Secondary,
                _ => RoadClass::Tertiary,
            },
            Some("minor_road") => {
                if detail == Some("service") {
                    RoadClass::Service
                } else {
                    RoadClass::Minor
                }
            }
            Some("path") => RoadClass::Path,
            _ => RoadClass::Other,
        }
    }

    pub fn roads_style(
        &self,
        kind: Option<&str>,
        kind_detail: Option<&str>,
        props: &FeatureProperties<'_, '_>,
        road: &RoadFacts<'_>,
        tile_zoom: i32,
    ) -> FeatureStyle {
        let is_tunnel = road.structure == RoadStructure::Tunnel;
        let roads = &self.theme.layers.roads;
        match kind {
            // A ferry route is a symbolic line like a border: a thin dashed
            // stroke in the water's colour, stated in points and held there
            // at every zoom. It crosses the river under the bridges' decks.
            Some("ferry") => {
                return FeatureStyle::point_locked_line(
                    33,
                    self.theme.layers.water,
                    Self::FERRY_WIDTH_POINTS,
                    Self::FERRY_DASH_POINTS,
                    Self::FERRY_GAP_POINTS,
                    ZoomFade::OVERVIEW,
                    None,
                );
            }
            // One key per width: the width is part of the baked style.
            Some("aeroway") => {
                return if kind_detail == Some("runway") {
                    self.round_line(38, self.theme.layers.aeroway, 8.0, 0.0, None)
                } else {
                    self.round_line(37, self.theme.layers.aeroway, 3.0, 0.0, None)
                };
            }
            Some("rail") => return self.rail_style(is_tunnel),
            _ => {}
        }
        let road_class = Self::road_class(kind, kind_detail);
        // The theme may hold a class back to a tile zoom. By default it does
        // not: every road the tile ships draws, except at the zooms where
        // the overview's road network stands in for them
        // (`overview_road_network_style`).
        if tile_zoom < self.theme.road_metrics.minimum_tile_zoom.value(road_class)
            || self.overview_road_network_draws(tile_zoom)
        {
            return FeatureStyle::Hidden;
        }
        // The name laid along the road is the road's own name: a road known
        // only by its number carries the number on its signs instead.
        let has_name = road.label.is_some() && !road.name.is_empty();
        let shields =
            self.route_shield_style(road, road_class, parse_bool_value(props.get("is_link")), tile_zoom);
        // Over a country or region view a road is a symbol, not a surface,
        // and it draws on the same principle as the country borders: one
        // point-locked stroke in the generic ground path, opaque from the
        // first frame it is visible. It is the same asphalt grey the street
        // era draws the class in, so the road never changes colour with the
        // zoom, and it has no kerb, because the street era has none either.
        // Joins and caps are round: the tiles chop a corridor into many
        // short features, and butt ends with plain joins tore the stroke
        // open at every bend and every feature boundary. On an opaque stroke
        // the overlaps of round geometry are invisible. The overview era
        // runs through tile z11. From z12 the same symbol draws by the
        // street era's rules below, with its casing, its name and its paint.
        // See `overview_road_stroke` for the classes.
        if tile_zoom <= Self::OVERVIEW_ROAD_MAXIMUM_TILE_ZOOM
            && let Some(stroke) = Self::overview_road_stroke(road_class)
        {
            return self.overview_road_style(
                stroke,
                road_class,
                has_name,
                shields,
                Self::street_road_color(road_class, roads),
                tile_zoom,
                is_tunnel,
            );
        }
        // From here the road is the street era's symbol: the class's width
        // in points from the theme (`road_style`) and the casing the theme
        // asks for. The width never reads the lane count: a symbol is the
        // same on every street of its class. Constant in points, so a street
        // keeps one readable weight across the region zooms instead of
        // doubling with every tile level.
        let symbol_width_points = self.theme.road_metrics.symbol_width_points.value(road_class);
        // Casing joins a class only from the zoom where the fill is wide
        // enough (about two points) for an edge to render. Below that a
        // sub-pixel casing just muddies the fill's antialiasing.
        let mut casing = tile_zoom >= 12;
        let (fill_key, color, mut priority) = match road_class {
            RoadClass::Motorway => (56, roads.motorway, 95),
            RoadClass::Trunk => (54, roads.trunk, 90),
            RoadClass::Primary => (52, roads.primary, 80),
            RoadClass::Secondary => (50, roads.secondary, 78),
            RoadClass::Tertiary => (48, roads.tertiary, 74),
            RoadClass::Minor => {
                casing = tile_zoom >= 13;
                (44, roads.minor, 50)
            }
            RoadClass::Service => {
                // A parking aisle sits one step below the rest of the tier.
                let is_parking_aisle = props.str("service") == Some("parking_aisle");
                casing = tile_zoom >= 14;
                (42, roads.service, if is_parking_aisle { 45 } else { 46 })
            }
            // Park alleys and walkways (footway, path, track): a plain strip
            // of the ground color, no kerb and no dashes. Over land it is the
            // ground itself (the footway network is not a second road
            // system), and over a park, a square or water it reads as a pale
            // route across the surface. A kerb on a ground-colored strip
            // turned every path into a grey band wider than its interior.
            RoadClass::Path => {
                casing = false;
                (40, roads.path, 35)
            }
            RoadClass::Other => {
                casing = tile_zoom >= 13;
                (43, roads.minor, 40)
            }
        };
        // A ramp sorts one step under its parent at a junction, in the same
        // colour and width, so the junction reads as the parent's.
        if parse_bool_value(props.get("is_link")) {
            priority -= 1;
        }
        self.road_style(RoadSymbol {
            fill_key,
            color,
            priority,
            casing,
            tunnel: is_tunnel,
            symbol_width_points,
            road_class,
            has_name,
            shields,
            oneway: parse_bool_value(props.get("oneway")),
        })
    }

    /// The style key of everything a tunnel draws, one per class fill key. A
    /// key is a colour slot per tile: the parser bakes the first style it
    /// meets under a key and every polygon with that key samples it. A
    /// translucent tunnel sharing its class key made the first tunnel in a
    /// tile the colour of every road of that class in it. From the
    /// separate-road zoom keys do not order drawing (the road phases sort by
    /// structure, layer, role and class). Below it the generic ground path
    /// draws by ascending key, where a motorway tunnel stroke sits above the
    /// other roads, invisible at a point and a half and twenty percent.
    pub fn road_tunnel_key(fill_key: u8) -> u8 {
        match fill_key {
            56 => 89,
            54 => 88,
            52 => 87,
            50 => 86,
            48 => 85,
            44 => 84,
            43 => 83,
            42 => 82,
            _ => 81,
        }
    }

    /// Casing keys sort below every road fill (and above buildings at 30):
    /// below the separate-road zoom the generic ground path draws by
    /// ascending key, so this is what puts every casing under every fill,
    /// the same layering the separate-road phases produce at street zoom.
    pub fn road_casing_key(fill_key: u8) -> u8 {
        match fill_key {
            56 => 39,
            54 => 38,
            52 => 37,
            50 => 36,
            48 => 35,
            44 => 34,
            43 => 33,
            _ => 32,
        }
    }

    /// The key of the paint on a class's asphalt, one per class fill key,
    /// above every fill key of the tier so the ground path never buries an
    /// arrow under the road that crosses it.
    pub fn road_marking_key(fill_key: u8) -> u8 {
        match fill_key {
            56 => 57,
            54 => 55,
            52 => 53,
            50 => 51,
            48 => 49,
            44 => 45,
            43 => 47,
            42 => 58,
            _ => 59,
        }
    }

    /// `color` with the tunnel opacity in place of its own alpha.
    pub fn tunnel_tone(color: Float4) -> Float4 {
        [color[0], color[1], color[2], Self::TUNNEL_FILL_OPACITY]
    }

    /// A fade in over the one zoom level from `start_zoom`, what a road's
    /// casing comes in with.
    pub fn class_zoom_fade(start_zoom: i32) -> ZoomFade {
        ZoomFade::fade_in(start_zoom as f64, start_zoom as f64 + 1.0)
    }

    /// The class grey the road draws in at every zoom: the same asphalt as
    /// the street era, so no colour blends or steps on the way down.
    pub fn street_road_color(road_class: RoadClass, roads: &RoadLayerStyles) -> Float4 {
        match road_class {
            RoadClass::Motorway => roads.motorway,
            RoadClass::Trunk => roads.trunk,
            RoadClass::Primary => roads.primary,
            RoadClass::Secondary => roads.secondary,
            _ => roads.tertiary,
        }
    }

    /// The classes the overview era draws, with the key and the priority of
    /// each: one asphalt grey, rank read as width alone, the principle the
    /// street era follows. How wide and how opaque the stroke is at a camera
    /// zoom is the theme's ramp (`road_width_ramp`), the same one the street
    /// era states, so the handover between the eras at a tile level shows
    /// nothing on screen.
    pub fn overview_road_stroke(road_class: RoadClass) -> Option<OverviewRoadStroke> {
        let (fill_key, priority) = match road_class {
            RoadClass::Motorway => (56, 95),
            RoadClass::Trunk => (54, 90),
            RoadClass::Primary => (52, 80),
            RoadClass::Secondary => (50, 78),
            RoadClass::Tertiary => (48, 74),
            _ => return None,
        };
        Some(OverviewRoadStroke { fill_key, priority })
    }

    /// The ramp a road class's symbol comes in over, from the theme: the
    /// overview stroke and its veil up to the overview zoom, the full symbol
    /// from the symbol zoom.
    pub fn road_width_ramp(&self, road_class: RoadClass, extra_width_points: f32) -> WidthRamp {
        let metrics = &self.theme.road_metrics;
        WidthRamp {
            start_width_points: metrics.overview_width_points.value(road_class) + extra_width_points,
            start_zoom: metrics.overview_zoom,
            end_zoom: metrics.symbol_zoom,
            start_alpha: metrics.overview_opacity,
        }
    }

    /// The ribbon a pre-extruded tile bakes to host a ramped point width:
    /// the widest the stroke gets while the tile is the one on screen, which
    /// is at the end of its zoom level.
    pub fn ramped_ribbon_width(ramp: &WidthRamp, width_points: f32, tile_zoom: i32) -> f64 {
        ramp.width_points((tile_zoom + 1) as f32, width_points) as f64 * FeatureStyle::POINT_LOCKED_RIBBON_UNITS_PER_POINT
    }

    /// A road's geometry: butt ends and round joins.
    pub fn road_geometry(width: f64) -> LineGeometryStyle {
        LineGeometryStyle::rounded(width, false, true)
    }

    /// A road over a country or region view: a symbolic stroke, drawn
    /// through the point-locked line factory the borders use (see
    /// `FeatureStyle::point_locked_line`) but with round joins and caps, so
    /// a corridor the tiles ship in pieces reads as one continuous line.
    /// Tunnels are the same stroke at the tunnel opacity. On the globe the
    /// stroke wears the theme's veil (`RoadMetrics::globe_opacity`) and the
    /// highway tones of the overview's road network (`OverviewRoadStyles`,
    /// the main tone for the motorways and the trunks, the minor one below),
    /// so the globe's view is one yellow net through the unroll. The yellow
    /// under the veil holds until the land cover starts to fade, and
    /// switches to asphalt there with no blend (`ROAD_LOOK_SWITCH_ZOOM`).
    /// `has_name` lays the road's name along it.
    #[allow(clippy::too_many_arguments)]
    pub fn overview_road_style(
        &self,
        stroke: OverviewRoadStroke,
        road_class: RoadClass,
        has_name: bool,
        shields: Option<RouteShieldStyle>,
        color: Float4,
        tile_zoom: i32,
        tunnel: bool,
    ) -> FeatureStyle {
        let fill_key = if tunnel { Self::road_tunnel_key(stroke.fill_key) } else { stroke.fill_key };
        // The stroke is the class's symbol under the theme's ramp: the veil
        // and the hairline width of a country view are the ramp's start.
        let width_points = self.theme.road_metrics.symbol_width_points.value(road_class);
        let ramp = self.road_width_ramp(road_class, 0.0);
        let fill_color = if tunnel { Self::tunnel_tone(color) } else { color };
        // The highway tones of the globe's palette, where the theme has one,
        // as the overview's own network wears them.
        let network = &self.theme.globe_layers.as_ref().unwrap_or(&self.theme.layers).overview_roads;
        let globe_color =
            if matches!(road_class, RoadClass::Motorway | RoadClass::Trunk) { network.major } else { network.minor };
        let geometry =
            LineGeometryStyle::rounded(Self::ramped_ribbon_width(&ramp, width_points, tile_zoom), true, true);
        let fill = LinePass {
            // The class shows from the first tile that ships it.
            zoom_fade: Self::ROAD_ZOOM_FADE,
            line_width_points: width_points,
            point_width_world_lock_zoom: self.theme.road_metrics.world_lock_zoom,
            point_width_ramp: Some(ramp),
            sphere_alpha: self.theme.road_metrics.globe_opacity,
            sphere_color: Some([globe_color[0], globe_color[1], globe_color[2]]),
            // The yellow and the veil hold past the globe's air, and switch
            // to the road's own at once as the land cover starts to fade.
            sphere_look_until_zoom: Some(Self::ROAD_LOOK_SWITCH_ZOOM),
            ..LinePass::new(fill_key, fill_color, geometry)
        };
        // A ground line at the coarse zooms, a road from
        // `ROAD_PATH_MINIMUM_TILE_ZOOM`: the same stroke either way, and the
        // case is what tells the engine which path draws it.
        if tile_zoom < Self::ROAD_PATH_MINIMUM_TILE_ZOOM {
            return FeatureStyle::Line(LineStyle::outlines(fill));
        }
        // The name rides the stroke on the road path, as it does from the
        // street era.
        let label = has_name.then(|| self.label_text_style(stroke.fill_key as i32, &self.theme.labels.road, None, false));
        FeatureStyle::Road(RoadStyle {
            fill: Some(fill),
            class_priority: stroke.priority,
            label,
            shields,
            ..RoadStyle::default()
        })
    }

    /// A road of the street era: its symbol, the casing the theme asks for,
    /// the name along it and the arrows of a one-way street.
    pub fn road_style(&self, symbol: RoadSymbol) -> FeatureStyle {
        let RoadSymbol {
            fill_key,
            color,
            priority,
            casing,
            tunnel,
            symbol_width_points,
            road_class,
            has_name,
            shields,
            oneway,
        } = symbol;
        // The road is a symbol: the class states a width in points, the
        // shader extrudes the centreline to exactly that many points at
        // every vertex up to the theme's world lock zoom, and to the ground
        // width those points had there past it. The ribbon the parser sees
        // is the point-locked host, as for the overview strokes, and the
        // same ramp the overview era states brings the symbol in, so the
        // tile level where the eras hand over changes nothing on screen.
        let world_lock_zoom = self.theme.road_metrics.world_lock_zoom;
        let casing_margin_points = 2.0 * Self::ROAD_CASING_POINTS_PER_SIDE as f32;
        let fill_ramp = self.road_width_ramp(road_class, 0.0);
        let casing_ramp = self.road_width_ramp(road_class, casing_margin_points);
        let fill_ribbon_width = symbol_width_points as f64 * FeatureStyle::POINT_LOCKED_RIBBON_UNITS_PER_POINT;
        // A tunnel is the plain ribbon at the tunnel opacity: no kerb
        // (skipped below), no paint, and butt ends.
        let fill_geometry = if tunnel {
            LineGeometryStyle::rounded(fill_ribbon_width, false, true)
        } else {
            Self::road_geometry(fill_ribbon_width)
        };
        let fill_color = if tunnel { Self::tunnel_tone(color) } else { color };
        let fill_pass_key = if tunnel { Self::road_tunnel_key(fill_key) } else { fill_key };

        // The casing is the theme's switch (`RoadMetrics::draws_casing`):
        // the symbol's points plus a fixed margin of points on each side,
        // under the same lock and the same ramp as the fill, so the kerb
        // keeps its proportion at every zoom.
        let casing_pass = (casing && !tunnel && self.theme.road_metrics.draws_casing).then(|| {
            let casing_width_points = symbol_width_points + casing_margin_points;
            let casing_width = casing_width_points as f64 * FeatureStyle::POINT_LOCKED_RIBBON_UNITS_PER_POINT;
            LinePass {
                zoom_fade: Self::class_zoom_fade(Self::CASING_MINIMUM_CAMERA_ZOOM),
                line_width_points: casing_width_points,
                point_width_world_lock_zoom: world_lock_zoom,
                point_width_ramp: Some(casing_ramp),
                ..LinePass::new(
                    Self::road_casing_key(fill_key),
                    Self::road_casing_color(fill_color),
                    Self::road_geometry(casing_width),
                )
            }
        });
        let fill_pass = LinePass {
            zoom_fade: Self::ROAD_ZOOM_FADE,
            line_width_points: symbol_width_points,
            point_width_world_lock_zoom: world_lock_zoom,
            point_width_ramp: Some(fill_ramp),
            ..LinePass::new(fill_pass_key, fill_color, fill_geometry)
        };
        // The name is laid along the street's own symbol: the basemap ships
        // it on the road line, with no label layer of its own.
        let label = has_name.then(|| self.label_text_style(fill_key as i32, &self.theme.labels.road, None, false));
        // The arrows of a one-way street: a paint stroke in the detail role,
        // above every fill of the tier, stamped with the arrow figure along
        // the surface segments. A tunnel carries no paint.
        let (paint, decoration) = if oneway && !tunnel {
            (
                vec![LinePass {
                    zoom_fade: Self::ROAD_MARKING_ZOOM_FADE,
                    ..LinePass::new(
                        Self::road_marking_key(fill_key),
                        self.theme.layers.roads.marking,
                        LineGeometryStyle::new(Self::ROAD_MARKING_RIBBON_WIDTH),
                    )
                }],
                RoadDecorationKind::OnewayArrow(OnewayArrowDecoration::default()),
            )
        } else {
            (Vec::new(), RoadDecorationKind::None)
        };
        FeatureStyle::Road(RoadStyle {
            casing: casing_pass,
            fill: Some(fill_pass),
            paint,
            class_priority: priority,
            decoration,
            label,
            shields,
            ..RoadStyle::default()
        })
    }

    /// The signs of a road's routes, from the theme (`RouteShields`): the
    /// routes it signs, on their country's signs, from the tile zoom the
    /// road's class carries them at. A ramp carries none: it shares its
    /// parent's numbers, and a sign on every ramp of an interchange buries
    /// the one on the road itself. The motorways' signs win a collision over
    /// the lesser classes', and every sign wins over the POIs.
    pub fn route_shield_style(
        &self,
        road: &RoadFacts<'_>,
        road_class: RoadClass,
        is_link: bool,
        tile_zoom: i32,
    ) -> Option<RouteShieldStyle> {
        let signs = &self.theme.route_shields;
        if road.routes.is_empty() || is_link || tile_zoom < signs.minimum_tile_zoom.value(road_class) {
            return None;
        }
        let shields = signs.shields(&road.routes);
        if shields.is_empty() {
            return None;
        }
        let rank = Self::route_shield_class_rank(road_class);
        Some(
            RouteShieldStyle::new(shields)
                .size_points(LabelTypeScale::clamped(signs.size_points))
                .weight(signs.weight)
                .spacing_points(signs.spacing_points)
                .rank(rank, Some(Self::label_collision_rank(LabelBand::RouteShield, rank))),
        )
    }

    /// A road class's place among the route signs, the most important
    /// first.
    pub fn route_shield_class_rank(road_class: RoadClass) -> i32 {
        match road_class {
            RoadClass::Motorway => 0,
            RoadClass::Trunk => 1,
            RoadClass::Primary => 2,
            RoadClass::Secondary => 3,
            RoadClass::Tertiary => 4,
            RoadClass::Minor => 5,
            RoadClass::Service | RoadClass::Path | RoadClass::Other => 6,
        }
    }

    /// Every railway draws dashed: surface rail, trams and the subway. A
    /// line in a tunnel (most of a subway) takes the tunnel opacity of a
    /// road, so it shows where it runs without reading as surface track.
    pub fn rail_style(&self, tunnel: bool) -> FeatureStyle {
        let ribbon_width = Self::RAIL_WIDTH_POINTS as f64 * FeatureStyle::POINT_LOCKED_RIBBON_UNITS_PER_POINT;
        let rail = self.theme.layers.roads.rail;
        FeatureStyle::Road(RoadStyle {
            fill: Some(LinePass {
                zoom_fade: Self::ROAD_ZOOM_FADE,
                line_width_points: Self::RAIL_WIDTH_POINTS,
                dash_length_points: Self::RAIL_DASH_POINTS,
                dash_gap_points: Self::RAIL_GAP_POINTS,
                ..LinePass::new(
                    if tunnel { 80 } else { 46 },
                    if tunnel { Self::tunnel_tone(rail) } else { rail },
                    LineGeometryStyle::new(ribbon_width),
                )
            }),
            class_priority: 30,
            ..RoadStyle::default()
        })
    }

    /// Road border = the fill colour darkened and made fully opaque, a
    /// border of the same hue but darker, never see-through, drawn under the
    /// lighter fill. The step is small and uniform across the channels, so
    /// an asphalt-grey fill keeps its neutral hue in the edge (a
    /// channel-biased step would tint the casing against the fill) and the
    /// edge defines the street without drawing a dark net over the city.
    pub fn road_casing_color(fill: Float4) -> Float4 {
        [(fill[0] - 0.13).max(0.0), (fill[1] - 0.13).max(0.0), (fill[2] - 0.13).max(0.0), 1.0]
    }
}

/// What a street-era road is drawn from (`road_style`).
#[derive(Debug, Clone, PartialEq)]
pub struct RoadSymbol {
    pub fill_key: u8,
    pub color: Float4,
    pub priority: i32,
    pub casing: bool,
    pub tunnel: bool,
    pub symbol_width_points: f32,
    pub road_class: RoadClass,
    /// Whether the road's own name is laid along it.
    pub has_name: bool,
    pub shields: Option<RouteShieldStyle>,
    pub oneway: bool,
}

impl ProtomapsBasemapDefaultMapStyle {
    /// The text style of an appearance under a key, with the colours of
    /// `globe_appearance` as what the label wears on the globe where one is
    /// given.
    pub fn label_text_style(
        &self,
        key: i32,
        appearance: &crate::style::default::LabelAppearance,
        globe_appearance: Option<&crate::style::default::LabelAppearance>,
        is_point_of_interest: bool,
    ) -> LabelTextStyle {
        LabelTextStyle {
            key,
            uppercased: appearance.uppercased,
            embolden_points: appearance.embolden_points,
            letter_spacing_em: appearance.letter_spacing_em,
            opacity: appearance.opacity,
            is_point_of_interest,
            sphere_fill_color: globe_appearance.map(|globe| globe.fill_color),
            sphere_stroke_color: globe_appearance.map(|globe| globe.stroke_color),
            ..LabelTextStyle::new(
                appearance.fill_color,
                appearance.stroke_color,
                appearance.halo_em,
                LabelTypeScale::clamped(appearance.size_points),
                appearance.weight,
            )
        }
        .with_halo_softness(appearance.halo_softness)
    }
}
