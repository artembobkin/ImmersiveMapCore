// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The ground: the land cover of the overview zooms, the land use of the
//! street zooms, the rivers and the administrative boundaries drawn on it.
//!
//! Polygon draw order is by ascending key. The built-up tints (residential,
//! industrial) are lowered to key 9 while all greenery goes above them
//! (11-18), otherwise a park lying inside a residential polygon would be
//! covered and look like bare ground. Greenery stays below water (key 30)
//! and roads. Each colour gets its own key (the first-wins `styles[key]`
//! would otherwise collapse every fill of a tile into the first polygon's
//! colour, producing seams at tile borders).
//!
//! At the overview zooms (through `BiomeStyles::maximum_tile_zoom`) the
//! engine's overview lays its biomes into the tiles and the biomes paint
//! the land in place of the land cover and the land use, which take the
//! keys the hidden fills leave free, 3 to 7, 9, 11 to 16, 18 and 19, one
//! per biome as above: a tile carries either the biomes or the land cover,
//! never both, and the rules read the one zoom (`overview_paints_land`).
//! Over the biomes, from `BiomeStyles::cover_minimum_tile_zoom`, the tiles'
//! own land cover draws its forests, fields, bare ground, ice and cities in
//! the block 20 to 24 and the land use its woods, fields, bare ground,
//! wetlands and ice in the block 25 to 29, both under the water: what the
//! biomes say of the climate, the cover says of the ground, and the holes
//! between the raster-derived blobs show the biome instead of bare land.
//! The two blocks part at the end of the overview: the biomes and the land
//! cover fade out over camera zoom 7 to 8, the land use, the same OSM
//! polygons the z8 tiles carry, stays.

use crate::schema::FeatureProperties;
use crate::style::default::rules::{lowercased, parse_bool_value, parse_int_value};
use crate::style::default::{BiomeStyles, LayerStyles, ProtomapsBasemapDefaultMapStyle};
use crate::style::{FeatureStyle, FillStyle, LineGeometryStyle, LinePass, LineStyle, ZoomFade};
use crate::Float4;

impl ProtomapsBasemapDefaultMapStyle {
    /// The water fill's key: over every painted ground, under the waterway
    /// lines, the ferries and the bridge decks.
    pub const WATER_KEY: u8 = 30;

    /// The deck's place among the ground lines: above the waterway lines
    /// (31 and 32) and the ferry routes (33), below the aeroway lines and
    /// the borders.
    pub const BRIDGE_DECK_KEY: u8 = 34;

    /// Whether the tile's land cover and land use give way to the
    /// overview's biomes: the tile zooms through
    /// `BiomeStyles::maximum_tile_zoom`, where the overview lays its biomes
    /// into the tiles. The overview is laid in by a setting, and a map that
    /// has it off shows the land cover at these zooms the way it did
    /// before: the style cannot see the setting, so the land cover rules
    /// take the same turn whether the biomes arrived or not, and with the
    /// overview off the land under the hidden cover is the earth polygon's
    /// land tone.
    pub fn overview_paints_land(&self, tile_zoom: i32) -> bool {
        tile_zoom <= self.theme.layers.biomes.maximum_tile_zoom
    }

    /// Whether the overview's road network stands in for the tiles' own
    /// roads at a tile zoom (`OverviewRoadStyles::maximum_tile_zoom`):
    /// through it the Natural Earth network draws and the basemap's roads,
    /// the motorways alone at those zooms, are hidden.
    pub fn overview_road_network_draws(&self, tile_zoom: i32) -> bool {
        self.overview_paints_land(tile_zoom) && tile_zoom <= self.theme.layers.overview_roads.maximum_tile_zoom
    }

    /// The overview's `road` layer: the Natural Earth network, one stroke
    /// per road in points, the main roads by rank in the theme's highway
    /// yellow and the rest thinner and paler, round-joined like the
    /// basemap's overview strokes and under the same veil on the globe
    /// (`RoadMetrics::globe_opacity`). A ground line among the ground lines,
    /// the main roads over the minor ones, both under the borders.
    pub fn overview_road_network_style(&self, rank: Option<i64>, tile_zoom: i32) -> FeatureStyle {
        if !self.overview_road_network_draws(tile_zoom) {
            return FeatureStyle::Hidden;
        }
        let network = &self.theme.layers.overview_roads;
        let is_major = rank.unwrap_or(i64::MAX) <= network.major_maximum_rank as i64;
        let width_points = if is_major { network.major_width_points } else { network.minor_width_points };
        let pass = LinePass {
            zoom_fade: Self::ROAD_ZOOM_FADE,
            line_width_points: width_points,
            sphere_alpha: self.theme.road_metrics.globe_opacity,
            sphere_color: self.globe_color(|layers| {
                if is_major { layers.overview_roads.major } else { layers.overview_roads.minor }
            }),
            ..LinePass::new(
                if is_major { 45 } else { 44 },
                if is_major { network.major } else { network.minor },
                LineGeometryStyle::rounded(
                    width_points as f64 * FeatureStyle::POINT_LOCKED_RIBBON_UNITS_PER_POINT,
                    true,
                    true,
                ),
            )
        };
        FeatureStyle::Line(LineStyle::outlines(pass))
    }

    /// The overview's `biome`: one fill per biome, each in the atlas tone
    /// the theme gives it (`BiomeStyles`), on the tiles of the overview
    /// zooms and hidden past them, where the land is the earth polygon under
    /// the land use. The biomes tile the land without overlapping, so their
    /// order among themselves is nothing. A kind the style does not name
    /// draws in the temperate grassland tone rather than not at all. The
    /// biomes fade out with the land cover over camera zoom 6.7 to 7.1
    /// (`landcover_zoom_fade`), with the globe's unroll and the roads'
    /// switch to their own look.
    pub fn biome_style(&self, kind: Option<&str>, tile_zoom: i32) -> FeatureStyle {
        if !self.overview_paints_land(tile_zoom) {
            return FeatureStyle::Hidden;
        }
        let (key, tone): (u8, fn(&BiomeStyles) -> Float4) = match kind {
            Some("tundra") => (3, |biomes| biomes.tundra),
            Some("desert") => (4, |biomes| biomes.desert),
            Some("montane_grassland") => (6, |biomes| biomes.montane_grassland),
            Some("tropical_grassland") => (7, |biomes| biomes.tropical_grassland),
            Some("mediterranean_scrub") => (9, |biomes| biomes.mediterranean_scrub),
            Some("flooded_grassland") => (11, |biomes| biomes.flooded_grassland),
            Some("mangrove") => (12, |biomes| biomes.mangrove),
            Some("boreal_forest") => (13, |biomes| biomes.boreal_forest),
            Some("temperate_conifer_forest") => (14, |biomes| biomes.temperate_conifer_forest),
            Some("temperate_broadleaf_forest") => (15, |biomes| biomes.temperate_broadleaf_forest),
            Some("tropical_conifer_forest") => (16, |biomes| biomes.tropical_conifer_forest),
            Some("tropical_dry_forest") => (18, |biomes| biomes.tropical_dry_forest),
            Some("tropical_moist_forest") => (19, |biomes| biomes.tropical_moist_forest),
            _ => (5, |biomes| biomes.temperate_grassland),
        };
        self.painted_style(key, |layers| tone(&layers.biomes), Self::landcover_zoom_fade())
    }

    /// The overview's `lake`, the lakes of tile zoom 0, where the basemap
    /// ships none: the water fill, under the same key as the basemap's.
    pub fn overview_lake_style(&self, tile_zoom: i32) -> FeatureStyle {
        if !self.overview_paints_land(tile_zoom) {
            return FeatureStyle::Hidden;
        }
        self.painted_style(Self::WATER_KEY, |layers| layers.water, ZoomFade::NONE)
    }

    /// The basemap's `landcover`: the continuous Daylight land cover it
    /// ships through z7, raster-derived blobs whose classes blend toward one
    /// shared tone over the globe and release to the unmixed palette by the
    /// country zooms. Each class wears the colour of what replaces it from
    /// z8 (a landcover forest the wood, farmland the farmland, barren the
    /// sand, a glacier the ice), so the swap changes the geometry source and
    /// never the colour. Grassland and scrub have no land use counterpart:
    /// from z8 open country is the bare `earth` polygon, so they release to
    /// the land tone. Painting them in the full grass green turned every z7
    /// plain green and the step to z8 into a jump from green to cream.
    ///
    /// The land cover draws on every tile that ships it, under the land use
    /// of the same tile, and a class the style does not name draws in the
    /// grassland tone rather than not at all. It fades out over camera zoom
    /// 6.7 to 7.1 (`landcover_zoom_fade`), with the camera instead of
    /// popping off with the tile level.
    pub fn landcover_style(&self, kind: Option<&str>, tile_zoom: i32) -> FeatureStyle {
        let fade = Self::landcover_zoom_fade();
        if self.overview_paints_land(tile_zoom) {
            // The biomes paint the land. On the globe (under
            // `cover_minimum_tile_zoom`) the raster-derived classes give way
            // to them whole, but the ice, which the biomes do not paint,
            // stays over every biome, and the cities stay, the one thing a
            // region view exists to show. From the cover zoom the forests,
            // the fields and the bare ground draw over the biomes in the
            // cover tones, a step off the biomes', and their holes show the
            // biome. Grassland and scrub never draw: the biome is their
            // tone. (Painted in the fields' tone the grassland filled the
            // holes between the fields and took the biome's tone out of the
            // temperate zone, two tones where three read better, and the
            // woods got no finer for it.) One key per cover tone, the land
            // use of the same ground in its own block over this one
            // (`landuse_style`): the land cover leaves with the biomes, the
            // land use does not, so they cannot share a fill.
            let cover_drawn = tile_zoom >= self.theme.layers.biomes.cover_minimum_tile_zoom;
            // The cover comes in with the camera over the first cover zooms
            // (`cover_fade_in`), and leaves with the biomes.
            let cover_fade = self.cover_fade_in(tile_zoom).unwrap_or(fade);
            return match kind {
                Some("forest") if cover_drawn => self.painted_style(20, |layers| layers.biomes.forest_cover, cover_fade),
                Some("farmland") if cover_drawn => {
                    self.painted_style(21, |layers| layers.biomes.farmland_cover, cover_fade)
                }
                Some("barren") if cover_drawn => self.painted_style(22, |layers| layers.biomes.barren_cover, cover_fade),
                Some("glacier") => self.painted_style(23, |layers| layers.ice, fade),
                Some("urban_area") => self.painted_style(24, |layers| layers.residential, fade),
                _ => FeatureStyle::Hidden,
            };
        }
        let amount = Self::vegetation_blend_amount(tile_zoom);
        match kind {
            Some("barren") => self.painted_style(3, |layers| layers.sand, fade),
            Some("farmland") => self.painted_style(5, |layers| blend(layers.farmland, layers.grass, amount), fade),
            Some("forest") => self.painted_style(6, |layers| blend(layers.wood, layers.grass, amount * 0.75), fade),
            Some("glacier") => self.painted_style(8, |layers| layers.ice, fade),
            // Cities are the one thing a region view exists to show: the
            // residential tone, clearly apart from the greens, the same the
            // street map's residential land use wears from z8.
            Some("urban_area") => self.painted_style(10, |layers| layers.residential, fade),
            // Grassland, scrub, a feature with no kind, and every class the
            // style does not name.
            _ => self.painted_style(4, |layers| blend(layers.land, layers.grass, amount), fade),
        }
    }

    /// The fade the cover over the biomes comes in with on a tile of the
    /// first cover zooms (`BiomeStyles::cover_fade_in_zooms`): from the
    /// cover zoom, in full `cover_fade_in_zooms` camera zooms later, on
    /// every tile whose zoom the camera is within that span at. `None` on a
    /// deeper tile, or with no span: the cover shows whole there.
    pub fn cover_fade_in(&self, tile_zoom: i32) -> Option<ZoomFade> {
        let biomes = &self.theme.layers.biomes;
        let span = biomes.cover_fade_in_zooms as f64;
        let from = biomes.cover_minimum_tile_zoom as f64;
        if span > 0.0 && tile_zoom >= biomes.cover_minimum_tile_zoom && (tile_zoom as f64) < from + span {
            Some(ZoomFade::fade_in(from, from + span))
        } else {
            None
        }
    }

    /// How far the vegetation classes blend toward the shared tone at a
    /// tile zoom: 1 is the full massive-overview merge, 0 the unmixed
    /// palette.
    pub fn vegetation_blend_amount(tile_zoom: i32) -> f32 {
        // The full merge exists for the globe (z0-2), where raster blobs
        // must disappear into one green mass. It releases fast below that:
        // a half blend held into the country zooms kept every class the same
        // green family while leaving the blob edges visible, which read as
        // camouflage blotches over a plain that is two-thirds cropland. By
        // z5 the ground and the fields are back to their own near-cream
        // tones and forests are the one green left on them.
        match tile_zoom {
            i32::MIN..=2 => 1.0,
            3 => 0.5,
            4 => 0.3,
            5 => 0.15,
            6 => 0.08,
            7 => 0.04,
            _ => 0.0,
        }
    }

    /// The basemap's `landuse`: the OSM land use and the natural cover, one
    /// kind per feature, on every tile that ships it. It lies over the land
    /// cover of the coarse tiles that carry both. A kind the style does not
    /// name draws in the muted built tone rather than not at all.
    pub fn landuse_style(&self, kind: Option<&str>, tile_zoom: i32) -> FeatureStyle {
        let layers = &self.theme.layers;
        let fade = ZoomFade::NONE;
        if self.overview_paints_land(tile_zoom) {
            // The biomes paint the land, and the land use draws over them
            // what the land cover does, in the cover tones, in its own block
            // of keys over the land cover's: the woods, the fields, the bare
            // ground, the wetlands and the ice, from the cover zoom, the ice
            // alone before it. The parks, the reserves and the built ground
            // of the coarse tiles are hidden: they would lie as blotches on
            // the painting. The land use does not fade: it is the same OSM
            // polygons the z8 tiles carry, so while the biomes and the
            // raster-derived land cover fade out under it over camera zoom 7
            // to 8, the woods and the fields stand through to the street map
            // and the z8 tiles take them over in place, with no cover to
            // appear from nowhere. It comes in with the land cover over the
            // first cover zooms (`cover_fade_in`).
            if tile_zoom < layers.biomes.cover_minimum_tile_zoom {
                return if kind == Some("glacier") {
                    self.painted_style(29, |layers| layers.ice, ZoomFade::NONE)
                } else {
                    FeatureStyle::Hidden
                };
            }
            let cover_fade = self.cover_fade_in(tile_zoom).unwrap_or(ZoomFade::NONE);
            return match kind {
                Some("forest" | "wood") => self.painted_style(25, |layers| layers.biomes.forest_cover, cover_fade),
                Some("farmland" | "orchard" | "allotments" | "vineyard" | "plant_nursery") => {
                    self.painted_style(26, |layers| layers.biomes.farmland_cover, cover_fade)
                }
                Some("sand" | "beach" | "bare_rock" | "quarry" | "scree" | "shingle") => {
                    self.painted_style(27, |layers| layers.biomes.barren_cover, cover_fade)
                }
                Some("wetland") => self.painted_style(28, |layers| layers.wetland, cover_fade),
                Some("glacier") => self.painted_style(29, |layers| layers.ice, ZoomFade::NONE),
                _ => FeatureStyle::Hidden,
            };
        }
        match kind {
            Some("residential" | "neighbourhood" | "farmyard") => {
                self.painted_style(9, |layers| layers.residential, fade)
            }
            // A protected area blankets whole regions and city centres: a
            // faint green over the land, under the built-up tints and every
            // other fill, so it shows only where nothing else is.
            Some("protected_area") => self.painted_style(7, |layers| blend(layers.land, layers.grass, 0.25), fade),
            // Schools and hospitals are the muted built tone, not a green:
            // their grounds are courtyards and car parks, not parks.
            Some(
                "commercial" | "industrial" | "railway" | "military" | "hospital" | "school" | "university"
                | "college",
            ) => self.painted_style(9, |layers| layers.industrial, fade),
            Some("forest" | "wood") => FeatureStyle::Fill(FillStyle {
                zoom_fade: fade,
                trees: layers.trees.wood,
                ..self.painted(11, |layers| layers.wood)
            }),
            // One green for all urban greenery, so no two-tone seam where a
            // park meets the pitch inside it. The parks grow the scattered
            // trees of a park, the open grass of the next case none, and
            // keeps the park's trees off itself: a pitch in a park is a
            // pitch.
            Some(
                "park" | "garden" | "recreation_ground" | "cemetery" | "zoo" | "national_park" | "nature_reserve"
                | "scrub" | "heath" | "village_green" | "camp_site" | "picnic_site",
            ) => FeatureStyle::Fill(FillStyle {
                zoom_fade: fade,
                trees: layers.trees.park,
                ..self.painted(12, |layers| layers.grass)
            }),
            Some("grass" | "meadow" | "golf_course" | "pitch" | "playground" | "dog_park" | "grassland") => {
                FeatureStyle::Fill(FillStyle {
                    zoom_fade: fade,
                    excludes_trees: true,
                    ..self.painted(12, |layers| layers.grass)
                })
            }
            Some("farmland" | "orchard" | "allotments" | "vineyard" | "plant_nursery") => {
                self.painted_style(13, |layers| layers.farmland, fade)
            }
            Some("wetland") => self.painted_style(14, |layers| layers.wetland, fade),
            Some("glacier") => self.painted_style(17, |layers| layers.ice, fade),
            Some("sand" | "beach" | "bare_rock" | "quarry" | "scree" | "shingle") => {
                self.painted_style(18, |layers| layers.sand, fade)
            }
            Some("aerodrome" | "airfield" | "runway" | "taxiway" | "apron" | "helipad") => {
                self.painted_style(19, |layers| layers.aeroway, fade)
            }
            // The basemap ships a bridge's deck (`man_made=bridge`) as a
            // `pedestrian` area, the same kind as a paved square. The deck
            // lies over the river, so it draws above the water, its waterway
            // lines and the ferry routes, in the land tone, under the roads
            // it carries. A square on land in the land tone reads as the
            // ground it is.
            Some("pedestrian" | "pier") => FeatureStyle::Fill(FillStyle {
                draws_among_ground_lines: true,
                ..self.painted(Self::BRIDGE_DECK_KEY, |layers| layers.land)
            }),
            // Everything else the basemap ships (retail, construction, a
            // platform, a kindergarten, a stadium, a marina, `other`) is
            // built or managed ground: the muted built tone marks where it
            // is without a colour of its own.
            _ => self.painted_style(9, |layers| layers.industrial, fade),
        }
    }

    /// A river, a canal or a stream: the lines of the `water` layer. The
    /// basemap spells the waterway kind on `kind` for a line and on
    /// `kind_detail` for a body, so both are read.
    ///
    /// A culverted waterway (a negative `layer`, or a `tunnel` tag) still
    /// draws, at the tunnel opacity of a road, so the network stays joined
    /// and reads as underground.
    pub fn waterway_style(
        &self,
        kind: Option<&str>,
        kind_detail: Option<&str>,
        props: &FeatureProperties<'_, '_>,
    ) -> FeatureStyle {
        let tunnel_tag = lowercased(props, "tunnel");
        let is_culvert = parse_int_value(props.get("layer")).unwrap_or(0) < 0
            || tunnel_tag.is_some_and(|tag| !tag.is_empty() && tag != "no");
        // The basemap ships major rivers from z9. Without a floor their
        // 2.5-unit width is sub-pixel over a region view and the
        // antialiasing correctly dims them to near-invisibility, so a point
        // floor keeps them readable threads from the first tile they appear
        // in. World growth takes over at street zoom as with roads.
        let (width, minimum_width_points) = match kind_detail.or(kind) {
            Some("river" | "canal") => (2.5, 0.7),
            Some("stream") => (1.4, 0.5),
            _ => (1.0, 0.5),
        };
        let tone = |layers: &LayerStyles| if is_culvert { Self::tunnel_tone(layers.water) } else { layers.water };
        self.round_line(
            if is_culvert { 31 } else { 32 },
            tone(&self.theme.layers),
            width,
            minimum_width_points,
            self.globe_color(tone),
        )
    }

    /// The `boundaries` layer: every level the tile ships. A national
    /// border is the one solid line, the convention of the atlas: the
    /// regions and the finer levels (a county, a municipality, a map unit)
    /// draw dashed, thinner and lighter, and a line the basemap marks as no
    /// recognised border (a disputed claim, an unrecognised country, the
    /// limit of a sea overlay) takes the short dash of a disputed border.
    pub fn boundary_style(&self, kind: Option<&str>, props: &FeatureProperties<'_, '_>, tile_zoom: i32) -> FeatureStyle {
        #[derive(PartialEq)]
        enum Level {
            Country,
            Region,
            Local,
        }
        let mut disputed = parse_bool_value(props.get("disputed"));
        let level = match kind {
            Some("country") => Level::Country,
            Some("unrecognized_country" | "disputed") => {
                disputed = true;
                Level::Country
            }
            Some("region" | "map_unit") => Level::Region,
            Some("overlay_limit") => {
                disputed = true;
                Level::Local
            }
            _ => Level::Local,
        };
        // Borders draw through the point-locked line factory (see
        // `FeatureStyle::point_locked_line`, which documents the principle
        // they originated): width and dash pattern in layout points, opaque,
        // butt ends, ribbon provisioned to host the width. One key per
        // colour. The softening of the palette's boundary tone is the same
        // over the plane's palette and the globe's.
        let (key, width_points, (toward_white, alpha)): (u8, f32, (f32, f32)) = match level {
            // A national border is a reference line, not the subject of the
            // map: it draws through the fills, half transparent, and over a
            // country or the globe it lightens a little. Still the strongest
            // line of the family at every zoom, so it stands apart from the
            // regional borders by colour as well as by being the one solid
            // line among them.
            Level::Country => (103, 1.0, if tile_zoom <= 6 { (0.15, 0.55) } else { (0.0, 0.65) }),
            // Regional borders are a step lighter and more transparent than
            // the national ones at every zoom, and over a country or the
            // globe, where the lines are the one cold hue in the frame and a
            // full purple reads as scribble, they lighten further.
            Level::Region => (100, 1.1, if tile_zoom <= 6 { (0.45, 0.45) } else { (0.2, 0.6) }),
            Level::Local => (98, 0.7, (0.45, 0.55)),
        };
        let tone = |layers: &LayerStyles| Self::softened_boundary_color(layers.boundary, toward_white, alpha);
        // Borders are drawn as lines only: the point-locked mode fills no
        // areas, so a boundary that arrives as a polygon is never a blob.
        // The dash is part of the baked style, so a disputed line takes the
        // key under its level's. A recognised national border is solid, the
        // regions and below dash.
        let (dash_length_points, dash_gap_points) = if disputed {
            (3.0, 3.0)
        } else if level == Level::Country {
            (0.0, 0.0)
        } else {
            (7.0, 3.5)
        };
        FeatureStyle::point_locked_line(
            if disputed { key - 1 } else { key },
            tone(&self.theme.layers),
            width_points,
            dash_length_points,
            dash_gap_points,
            ZoomFade::OVERVIEW,
            self.globe_color(tone),
        )
    }

    /// The boundary tone moved toward white by `amount`, its alpha scaled
    /// by `alpha`.
    pub fn softened_boundary_color(color: Float4, amount: f32, alpha: f32) -> Float4 {
        let white = [1.0, 1.0, 1.0, color[3]];
        let softened: Float4 = [0, 1, 2, 3].map(|i| color[i] + (white[i] - color[i]) * amount);
        [softened[0], softened[1], softened[2], color[3] * alpha]
    }
}

/// `base` moved toward `target` by `amount`.
pub(crate) fn blend(base: Float4, target: Float4, amount: f32) -> Float4 {
    [0, 1, 2, 3].map(|i| base[i] + (target[i] - base[i]) * amount)
}
