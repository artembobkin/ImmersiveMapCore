// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use serde::{Deserialize, Serialize};

use crate::style::{
    BaseColors, Hillshade, LabelFontWeight, LabelOpacity, LabelTextStyle, TerrainOcclusion, TreeFillStyle, TreePalette,
    TreePointStyle,
};
use crate::style::default::RouteShields;
use crate::{Float3, Float4};

/// The look of the built-in style (the Protomaps basemap) as plain values:
/// the colour of every layer and road class, the building fill, the label
/// appearances and which labels show, and the route signs. An app recolours
/// the map by changing a field of the default and never touches the layer
/// logic:
///
/// ```
/// use map_style::style::default::ProtomapsBasemapTheme;
/// let theme = ProtomapsBasemapTheme::default().apply(|theme| {
///     theme.layers.water = [0.2, 0.4, 0.8, 1.0];
///     theme.layers.roads.motorway = [0.95, 0.6, 0.2, 1.0];
/// });
/// assert_ne!(theme.cache_fingerprint(), ProtomapsBasemapTheme::default().cache_fingerprint());
/// ```
///
/// Colours are RGBA in 0...1. Every value feeds the cache fingerprint, so a
/// change rebakes the prepared tiles by itself, but the hillshade, the
/// terrain occlusion, the label opacity and the tree palette, which are
/// drawn at frame time and baked into nothing.
///
/// The theme is the serde entry point the browser configures the built-in
/// style with: JSON field names are the engine's property names in camel
/// case (`labelVisibility`, `roadMetrics`, `globeLayers`). A struct with a
/// default of its own fills the fields a JSON object leaves out from it. A
/// value whose default depends on where it stands (a label appearance, one
/// value per road class) is given whole: a partial patch is merged over the
/// serialized default theme by the caller.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProtomapsBasemapTheme {
    pub labels: LabelStyles,
    /// The labels the globe wears, `None` for `labels`: the fill and the
    /// halo of a name on the globe's view, the unroll into the plane
    /// included, come from here and the plane's from `labels`, handed over
    /// with the globe's air at the end of the unroll
    /// (`LabelTextStyle::sphere_fill_color`, `sphere_stroke_color`). The
    /// size, the weight, the halo's width and the opacity are always those
    /// of `labels`. The night palette sets the day's here, with
    /// `globe_layers`. Baked into the prepared tiles, so a change re-parses
    /// them.
    pub globe_labels: Option<LabelStyles>,
    pub label_visibility: LabelVisibility,
    pub layers: LayerStyles,
    /// The palette the globe wears, `None` for `layers`: the ground of the
    /// globe's view, the unroll into the plane included, takes its colours
    /// from here and the plane from `layers`, handed over with the globe's
    /// air at the end of the unroll (`FillStyle::sphere_color`,
    /// `LinePass::sphere_color`). The night palette sets the day's here, so
    /// the planet keeps its daylight and its sun while the map in it is
    /// dark. The caps past the poles follow it. Baked into the prepared
    /// tiles, so a change re-parses them.
    pub globe_layers: Option<LayerStyles>,
    pub features: FeatureStyles,
    pub road_metrics: RoadMetrics,
    pub route_shields: RouteShields,
    /// How the slopes are shaded where the ground relief stands.
    pub hillshade: Hillshade,
    /// How the ground is darkened where the terrain hides the sky, where
    /// the ground relief stands.
    pub terrain_occlusion: TerrainOcclusion,
    /// How opaque the names and the POIs are by the zoom, a factor over
    /// each label's own opacity. Drawn at frame time, so a change re-parses
    /// nothing.
    pub label_opacity: LabelOpacity,
}

/// How one class of label looks.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelAppearance {
    pub fill_color: Float3,
    pub stroke_color: Float3,
    /// Halo width as a fraction of the em, so it tracks the text size.
    pub halo_em: f32,
    /// Em size in layout points, not device pixels: the engine multiplies
    /// by the display's pixels-per-point at render time, so a value here
    /// reads at the same physical size on a 2x desktop display and a 3x
    /// phone.
    pub size_points: f32,
    pub weight: LabelFontWeight,
    /// Whether the label is set in capitals.
    #[serde(default)]
    pub uppercased: bool,
    /// How far the letters are grown past the font's outline, in layout
    /// points (`LabelTextStyle::embolden_points`).
    #[serde(default = "default_embolden_points")]
    pub embolden_points: f32,
    /// Extra room after every letter, in ems
    /// (`LabelTextStyle::letter_spacing_em`): the tracking of a name spaced
    /// out over the land it names.
    #[serde(default)]
    pub letter_spacing_em: f32,
    /// The opacity of the label, fill and halo alike
    /// (`LabelTextStyle::opacity`): 1 solid, less lets the map show through
    /// a name laid over it.
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    /// How softly the halo ends (`LabelTextStyle::halo_softness`): the
    /// share of its width over which it fades out into the map, 0 a halo
    /// solid to its edge.
    #[serde(default)]
    pub halo_softness: f32,
}

fn default_embolden_points() -> f32 {
    LabelTextStyle::DEFAULT_EMBOLDEN_POINTS
}

fn default_opacity() -> f32 {
    1.0
}

impl LabelAppearance {
    /// An appearance of these colours, halo, size and weight, with the
    /// default growth, no capitals, no tracking, solid, a hard halo.
    pub const fn new(fill_color: Float3, stroke_color: Float3, halo_em: f32, size_points: f32, weight: LabelFontWeight) -> Self {
        LabelAppearance {
            fill_color,
            stroke_color,
            halo_em,
            size_points,
            weight,
            uppercased: false,
            embolden_points: LabelTextStyle::DEFAULT_EMBOLDEN_POINTS,
            letter_spacing_em: 0.0,
            opacity: 1.0,
            halo_softness: 0.0,
        }
    }
}

/// The appearance of every class of label.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LabelStyles {
    pub city: LabelAppearance,
    pub town: LabelAppearance,
    /// The districts of a city: its neighbourhoods and the larger quarters
    /// they make up, and the places smaller than a town (a village, a
    /// hamlet, a named square).
    pub district: LabelAppearance,
    /// A country's name: the name of a land, not of a point, so by default
    /// it is set apart from the places in it, in spaced capitals above
    /// every city's size.
    pub country: LabelAppearance,
    /// A region's name (a state, a province, a land): under the country's,
    /// in the same manner, above a city's.
    pub region: LabelAppearance,
    pub poi: LabelAppearance,
    pub water: LabelAppearance,
    pub road: LabelAppearance,
    /// The plates of the POI icons, one colour per category of place
    /// (`PoiCategoryColors`). A POI of no category wears `poi`'s fill.
    pub poi_categories: PoiCategoryColors,
}

impl Default for LabelStyles {
    /// Sizes are layout points: the pixel size this style used before
    /// labels carried a unit, divided by the 2x reference scale the palette
    /// was authored against.
    ///
    /// The curve below is the design, not the final size. Sizes are raised
    /// to `LabelTypeScale::MINIMUM_SIZE_POINTS` where they fall under the
    /// floor for readable type, and that happens once, where a style is
    /// resolved, so that a class derived from another (an ocean label is the
    /// water appearance a few points larger) is measured against the floor
    /// after its own adjustment rather than on top of a base that was
    /// already lifted.
    ///
    /// Every name is laid over the map a little transparent (0.8, the
    /// countries and the regions lighter still), and every halo fades out
    /// over the outer half of its width, so a name reads without standing
    /// out of the map as a sticker would.
    fn default() -> Self {
        let white = [1.0, 1.0, 1.0];
        LabelStyles {
            city: LabelAppearance {
                opacity: 0.8,
                halo_softness: 0.5,
                ..LabelAppearance::new([0.20, 0.20, 0.22], white, 0.153, 15.0, LabelFontWeight::Bold)
            },
            town: LabelAppearance {
                opacity: 0.8,
                halo_softness: 0.5,
                ..LabelAppearance::new([0.30, 0.30, 0.32], white, 0.173, 11.0, LabelFontWeight::Thin)
            },
            // The districts are lettered like the streets, the deep black
            // being the cities' alone: light grey capitals in the thin face
            // with a little added weight and a narrow halo, so a district
            // names the stretch of the city it lies over without standing
            // out of it.
            district: LabelAppearance {
                uppercased: true,
                embolden_points: 0.2,
                opacity: 0.8,
                halo_softness: 0.5,
                ..LabelAppearance::new([0.45, 0.45, 0.46], white, 0.1, 10.0, LabelFontWeight::Thin)
            },
            // A country's name is the name of a land: spaced capitals in the
            // thin face with a little added weight, a shade above a
            // capital's size (16.5), in a lighter grey of its own with a
            // narrow halo, so it reads as lettering over the country and not
            // as one more town, and a globe full of them stays quiet. Both
            // are lettering laid over the land, so they let the map show
            // through them. A region's name is the same manner, a city's
            // size and lighter still.
            country: LabelAppearance {
                uppercased: true,
                embolden_points: 0.3,
                letter_spacing_em: 0.12,
                opacity: 0.65,
                halo_softness: 0.5,
                ..LabelAppearance::new([0.38, 0.37, 0.44], white, 0.1, 17.0, LabelFontWeight::Thin)
            },
            region: LabelAppearance {
                uppercased: true,
                embolden_points: 0.3,
                letter_spacing_em: 0.1,
                opacity: 0.7,
                halo_softness: 0.5,
                ..LabelAppearance::new([0.46, 0.45, 0.51], white, 0.1, 15.0, LabelFontWeight::Thin)
            },
            poi: LabelAppearance {
                opacity: 0.8,
                halo_softness: 0.5,
                ..LabelAppearance::new([0.40, 0.42, 0.40], white, 0.225, 8.0, LabelFontWeight::Thin)
            },
            // The water names lie on the water: a deeper blue of its own hue
            // and no halo, so they read as part of the surface.
            water: LabelAppearance {
                opacity: 0.8,
                ..LabelAppearance::new([0.29, 0.49, 0.71], white, 0.0, 9.5, LabelFontWeight::Thin)
            },
            // The street names stay under the places and the landmarks:
            // small light grey capitals in the thin face with a little added
            // weight and a narrow halo, the way a commercial map letters its
            // streets, so a street reads as a street and never outweighs the
            // city it runs through.
            road: LabelAppearance {
                uppercased: true,
                embolden_points: 0.2,
                opacity: 0.8,
                halo_softness: 0.5,
                ..LabelAppearance::new([0.45, 0.45, 0.46], white, 0.1, 11.0, LabelFontWeight::Thin)
            },
            poi_categories: PoiCategoryColors::default(),
        }
    }
}

/// The colours of the POI icons' plates, one per category of place, RGB in
/// `0...1`: the icon stands white on the plate, and the POI's name is set
/// in the same colour. The categories are the built-in style's reading of
/// the basemap's POI kinds (`ProtomapsBasemapDefaultMapStyle::poi_category_color`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PoiCategoryColors {
    /// Restaurants and fast food.
    pub food: Float3,
    /// Cafés and bakeries.
    pub coffee: Float3,
    /// Bars, pubs and clubs.
    pub bar: Float3,
    /// Shops and markets.
    pub shop: Float3,
    /// Hotels and hostels.
    pub hotel: Float3,
    /// Hospitals, pharmacies and clinics.
    pub health: Float3,
    /// Schools, universities and libraries.
    pub education: Float3,
    /// Museums, theatres and attractions.
    pub culture: Float3,
    /// Parks, gardens and sports grounds.
    pub leisure: Float3,
    /// Stations, stops, fuel and parking.
    pub transport: Float3,
    /// Offices and public services.
    pub civic: Float3,
    /// Places of worship.
    pub worship: Float3,
}

impl Default for PoiCategoryColors {
    /// The day's plates.
    fn default() -> Self {
        PoiCategoryColors {
            food: [0.85, 0.40, 0.12],
            coffee: [0.58, 0.37, 0.18],
            bar: [0.62, 0.16, 0.34],
            shop: [0.16, 0.44, 0.78],
            hotel: [0.66, 0.26, 0.60],
            health: [0.82, 0.22, 0.26],
            education: [0.22, 0.46, 0.52],
            culture: [0.46, 0.30, 0.66],
            leisure: [0.22, 0.54, 0.30],
            transport: [0.32, 0.42, 0.55],
            civic: [0.40, 0.44, 0.52],
            worship: [0.55, 0.45, 0.22],
        }
    }
}

impl PoiCategoryColors {
    /// Every plate, in the order declared.
    pub fn colors(&self) -> [Float3; 12] {
        [
            self.food,
            self.coffee,
            self.bar,
            self.shop,
            self.hotel,
            self.health,
            self.education,
            self.culture,
            self.leisure,
            self.transport,
            self.civic,
            self.worship,
        ]
    }

    /// The plates with `change` applied to each, the way a theme sinks them
    /// into a night.
    pub fn map(&self, change: impl Fn(Float3) -> Float3) -> PoiCategoryColors {
        PoiCategoryColors {
            food: change(self.food),
            coffee: change(self.coffee),
            bar: change(self.bar),
            shop: change(self.shop),
            hotel: change(self.hotel),
            health: change(self.health),
            education: change(self.education),
            culture: change(self.culture),
            leisure: change(self.leisure),
            transport: change(self.transport),
            civic: change(self.civic),
            worship: change(self.worship),
        }
    }
}

/// The zoom a POI category waits for, whatever the basemap ranks the POI
/// at. The categories not named here follow the basemap alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PoiCategoryZooms {
    /// Universities, colleges and institutes: many, large and named at
    /// length, so they would crowd out the landmarks at z13.
    pub campus: i32,
    /// Shops, other than malls, department stores and markets.
    pub shop: i32,
    /// The street-level services: post and parcel offices, banks and cash
    /// machines, pharmacies, clinics and doctors, schools and
    /// kindergartens, offices of the state, embassies, police and fire
    /// stations, public toilets, fuel and charging.
    pub service: i32,
    /// The pieces of transit: stops, platforms, metro entrances, taxi
    /// ranks, parking and bike docks. The stations themselves follow the
    /// basemap.
    pub transit_detail: i32,
}

impl Default for PoiCategoryZooms {
    fn default() -> Self {
        PoiCategoryZooms { campus: 14, shop: 16, service: 17, transit_detail: 17 }
    }
}

/// A POI category that waits for a zoom of its own, see
/// `PoiCategoryZooms`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoiCategory {
    Campus,
    Shop,
    Service,
    TransitDetail,
}

impl PoiCategoryZooms {
    pub fn zoom(&self, category: PoiCategory) -> i32 {
        match category {
            PoiCategory::Campus => self.campus,
            PoiCategory::Shop => self.shop,
            PoiCategory::Service => self.service,
            PoiCategory::TransitDetail => self.transit_detail,
        }
    }
}

/// Zoom thresholds that decide whether a label class is drawn at all (as
/// opposed to `LabelStyles`, which only decides how a drawn label looks).
///
/// The default is a density curve, not a filter: a city centre reads at
/// every zoom instead of drowning in names. A POI shows from the zoom the
/// basemap ranks it at (its `min_zoom`), so the landmarks, the parks and
/// the metro lead at z13 and z14, and the restaurants, cafés and shops come
/// in at z16 and z17. On top of that, the categories that help only someone
/// standing in the street (post offices, cash machines, clinics, schools,
/// stops and entrances, parking, named plots without a symbol) wait for the
/// street zooms, and so do the house numbers and the longest names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LabelVisibility {
    /// Minimum camera zoom from which icon-less POIs (offices, companies,
    /// and other categories outside the set of recognized icons) are drawn.
    /// POIs with an icon draw from the regular threshold, so overview zooms
    /// keep only icon POIs while the dense scatter of text-only labels kicks
    /// in deeper. Read only when `poi_requires_icon` is false, since
    /// otherwise icon-less POIs are not drawn at any zoom.
    pub poi_iconless_minimum_zoom: i32,
    /// Minimum camera zoom from which any POI label (icon or icon-less) is
    /// drawn, applied on top of the rank-derived thresholds. The default 0
    /// keeps the rank behavior. A value above the camera's maximum zoom
    /// hides POIs entirely (useful for clean cinematic footage).
    pub poi_minimum_zoom: i32,
    /// Whether a POI needs an icon to be labelled at all.
    ///
    /// A category the icon set does not recognize (an office, a company, a
    /// monument, a named building) has nothing to draw but its name, and
    /// those names are the bulk of a city centre's POIs: bare text over the
    /// buildings, in the type size of a landmark, saying nothing about what
    /// the place is. The default draws their names alone, without an icon,
    /// from `poi_iconless_minimum_zoom` on. Set to true to leave them out
    /// altogether.
    pub poi_requires_icon: bool,
    /// Whether a POI waits for the zoom the basemap ranks it at, its
    /// `min_zoom`. Off, every POI a tile ships competes from the tile's own
    /// zoom, and only the collisions thin them out.
    pub poi_follows_source_minimum_zoom: bool,
    /// The zooms whole POI categories wait for.
    pub poi_category_minimum_zoom: PoiCategoryZooms,
    /// A POI whose name runs past `poi_long_name_character_count`
    /// characters waits for this zoom: a name that wraps to three lines is
    /// a block of text, not a label, until the map has room for it.
    pub poi_long_name_minimum_zoom: i32,
    pub poi_long_name_character_count: i32,
    /// Minimum camera zoom of the house numbers.
    pub address_minimum_zoom: i32,
    /// The camera zoom a country's name goes away at: shown below it,
    /// hidden from it on. The basemap ships the names of the countries into
    /// the street zooms, where a name that stands for the whole land reads
    /// as one more town among the towns. The default keeps it to the views
    /// that hold a country, below zoom 8.
    pub country_maximum_zoom: i32,
    /// The camera zoom a region's name goes away at, as
    /// `country_maximum_zoom`: a state or a province is read a couple of
    /// zooms deeper than its country.
    pub region_maximum_zoom: i32,
    /// Whether the local detail, the icon-less POIs (a memorial, an office,
    /// a ticket window) and the house numbers, shows only near the camera
    /// (`PointLabelStyle::is_local`): in the three by three tiles around
    /// the look-at tile, and there within the settings' local detail
    /// distance of the camera. They are what one reads standing next to
    /// them, and on a tilted map the street beyond would fill with small
    /// text saying nothing at that distance. Off, they show wherever their
    /// zoom lets them. Baked into the prepared tiles, so a change re-parses
    /// them.
    pub local_detail_in_look_at_tile_only: bool,
}

impl Default for LabelVisibility {
    fn default() -> Self {
        LabelVisibility {
            poi_iconless_minimum_zoom: 17,
            poi_minimum_zoom: 0,
            poi_requires_icon: false,
            poi_follows_source_minimum_zoom: true,
            poi_category_minimum_zoom: PoiCategoryZooms::default(),
            poi_long_name_minimum_zoom: 17,
            poi_long_name_character_count: 32,
            address_minimum_zoom: 17,
            country_maximum_zoom: 8,
            region_maximum_zoom: 10,
            local_detail_in_look_at_tile_only: true,
        }
    }
}

/// The road classes the style draws, the tiers it folds the Protomaps
/// `roads` layer's `kind` and `kind_detail` into: a `highway` is a
/// motorway, a `major_road` is a trunk, primary, secondary or tertiary by
/// its detail, a `minor_road` is minor or service, a `path` is a path.
/// `Other` stands for every road the style draws without naming its class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RoadClass {
    Motorway,
    Trunk,
    Primary,
    Secondary,
    Tertiary,
    Minor,
    Service,
    Path,
    Other,
}

impl RoadClass {
    pub const ALL: [RoadClass; 9] = [
        RoadClass::Motorway,
        RoadClass::Trunk,
        RoadClass::Primary,
        RoadClass::Secondary,
        RoadClass::Tertiary,
        RoadClass::Minor,
        RoadClass::Service,
        RoadClass::Path,
        RoadClass::Other,
    ];
}

/// One colour per road class, plus the rail, the casing and the paint on
/// the asphalt.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RoadLayerStyles {
    pub motorway: Float4,
    pub trunk: Float4,
    pub primary: Float4,
    pub secondary: Float4,
    pub tertiary: Float4,
    pub minor: Float4,
    pub service: Float4,
    pub path: Float4,
    pub rail: Float4,
    pub casing: Float4,
    /// The paint on the asphalt: the arrows of a one-way street. Fully
    /// opaque, since two overlapping quads of a translucent colour composite
    /// twice and stamp a denser patch.
    pub marking: Float4,
}

impl RoadLayerStyles {
    /// An off-white that reads on the asphalt grey without glaring.
    pub const DEFAULT_MARKING: Float4 = [0.97, 0.97, 0.96, 1.0];
}

impl Default for RoadLayerStyles {
    /// Asphalt streets in the driving-map manner: every drive tier is the
    /// same cool neutral grey, one road surface across the network, and
    /// importance reads as width alone, motorways widest down to narrow
    /// service alleys. Paths keep their warm gravel tone (they are not
    /// asphalt) and the casing the style derives from these fills is a
    /// uniformly darker grey, so every road sits in a slightly deeper edge.
    fn default() -> Self {
        let asphalt = [0.757, 0.769, 0.784, 1.0];
        RoadLayerStyles {
            motorway: asphalt,
            trunk: asphalt,
            primary: asphalt,
            secondary: asphalt,
            tertiary: asphalt,
            minor: asphalt,
            service: asphalt,
            // A pedestrian path is a warm sand tone a clear step under the
            // ground, the way Apple draws park walks: visible against the
            // beige land AND against park green, yet quiet enough that the
            // footway network stays a texture rather than a second road
            // system. It used to be the exact ground off-white, which made
            // every path invisible until it crossed a park.
            path: [0.898, 0.871, 0.812, 1.0],
            rail: [0.741, 0.741, 0.765, 1.0],
            casing: [0.718, 0.725, 0.735, 1.0],
            marking: Self::DEFAULT_MARKING,
        }
    }
}

/// One value per road class (`RoadClass`), `other` standing for every road
/// the style draws without naming its class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoadClassValues<T> {
    pub motorway: T,
    pub trunk: T,
    pub primary: T,
    pub secondary: T,
    pub tertiary: T,
    pub minor: T,
    pub service: T,
    pub path: T,
    pub other: T,
}

impl<T: Copy> RoadClassValues<T> {
    /// The value of a road class.
    pub fn value(&self, road_class: RoadClass) -> T {
        match road_class {
            RoadClass::Motorway => self.motorway,
            RoadClass::Trunk => self.trunk,
            RoadClass::Primary => self.primary,
            RoadClass::Secondary => self.secondary,
            RoadClass::Tertiary => self.tertiary,
            RoadClass::Minor => self.minor,
            RoadClass::Service => self.service,
            RoadClass::Path => self.path,
            RoadClass::Other => self.other,
        }
    }

    /// Every value, in the classes' order.
    pub fn all(&self) -> [T; 9] {
        [
            self.motorway,
            self.trunk,
            self.primary,
            self.secondary,
            self.tertiary,
            self.minor,
            self.service,
            self.path,
            self.other,
        ]
    }
}

/// How wide the roads of the street era draw and from where, as opposed to
/// `RoadLayerStyles`, which is only their colours.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RoadMetrics {
    /// The width of a class as a symbol, in points: what it draws at on
    /// screen from `symbol_zoom` up to `world_lock_zoom`.
    pub symbol_width_points: RoadClassValues<f32>,
    /// The width of a class over a country view, in points: what it draws
    /// at up to `overview_zoom`. Between `overview_zoom` and `symbol_zoom`
    /// the width grows from this to `symbol_width_points` by the same ratio
    /// per zoom level, continuous in camera zoom, so a road never steps in
    /// width, neither with the camera nor when the engine swaps the tile
    /// level serving it.
    pub overview_width_points: RoadClassValues<f32>,
    /// The camera zoom up to which a road is its overview stroke.
    pub overview_zoom: f32,
    /// The camera zoom from which a road is its full symbol.
    pub symbol_zoom: f32,
    /// The opacity of the overview stroke. It grows to one along with the
    /// width, by `symbol_zoom`. One by default: a road is opaque at every
    /// zoom, which is what lets the engine shade each of its pixels once,
    /// with no blending. Below one the stroke is a veil that lets the
    /// ground colours stay the picture over a country view, and the roads
    /// blend until `symbol_zoom`.
    pub overview_opacity: f32,
    /// The opacity of the overview stroke on the globe, `0...1`: a veil the
    /// globe's view lays over the roads, so the painted ground stays the
    /// picture of a continent and the roads a net over it. 0.6 by default.
    /// One is the same stroke as on the plane. It holds through the globe's
    /// view and its unroll into the plane, is handed over with the globe's
    /// air at the end of the unroll, and the plane never has it, whatever
    /// `overview_opacity` says there. The engine blends each stroke once on
    /// the globe, so the veil never darkens where two strokes cross.
    pub globe_opacity: f32,
    /// The camera zoom from which a road's width is fixed on the ground
    /// instead of on screen. Up to it a road is a symbol of
    /// `symbol_width_points`. Past it the road keeps the ground width the
    /// symbol had at this zoom, so it doubles on screen with every zoom
    /// level, like the blocks and the buildings around it, and never thins
    /// into a hairline at street level. The handover is continuous in
    /// camera zoom. Zero keeps every road a symbol at every zoom.
    pub world_lock_zoom: f32,
    /// Whether the automobile roads wear a casing: an outline a point wide
    /// on each side of a road's symbol that eases in at street zoom, and
    /// the kerb of a measured junction surface. Off by default: the roads
    /// read as one sheet of asphalt against the ground, and an outline
    /// around every street turns the sheet back into separate ribbons.
    pub draws_casing: bool,
    /// The tile zoom a class first draws at. The style draws what the tiles
    /// carry from this zoom on. The default is 0 for every class, so a road
    /// shows from the first tile the Protomaps basemap ships it in
    /// (motorways from z4, trunks from z6, primaries from z7, secondaries
    /// from z10, the minor network from z12).
    pub minimum_tile_zoom: RoadClassValues<i32>,
}

impl RoadMetrics {
    /// With every drive tier sharing one asphalt grey, width is the whole
    /// hierarchy, so the ramp is spread wide.
    pub const DEFAULT_SYMBOL_WIDTH_POINTS: RoadClassValues<f32> = RoadClassValues {
        motorway: 13.65,
        trunk: 12.7,
        primary: 11.7,
        secondary: 5.0,
        tertiary: 4.5,
        minor: 4.0,
        service: 2.5,
        path: 2.0,
        other: 2.0,
    };

    /// The country view's ladder: hairlines whose rank still reads as width
    /// alone.
    pub const DEFAULT_OVERVIEW_WIDTH_POINTS: RoadClassValues<f32> = RoadClassValues {
        motorway: 1.3,
        trunk: 1.1,
        primary: 0.9,
        secondary: 0.8,
        tertiary: 0.7,
        minor: 0.6,
        service: 0.5,
        path: 0.4,
        other: 0.4,
    };

    /// No class is held back: the basemap's own tile levels decide where
    /// each road first appears.
    pub const DEFAULT_MINIMUM_TILE_ZOOM: RoadClassValues<i32> = RoadClassValues {
        motorway: 0,
        trunk: 0,
        primary: 0,
        secondary: 0,
        tertiary: 0,
        minor: 0,
        service: 0,
        path: 0,
        other: 0,
    };
}

impl Default for RoadMetrics {
    fn default() -> Self {
        RoadMetrics {
            symbol_width_points: Self::DEFAULT_SYMBOL_WIDTH_POINTS,
            overview_width_points: Self::DEFAULT_OVERVIEW_WIDTH_POINTS,
            overview_zoom: 6.0,
            symbol_zoom: 14.0,
            overview_opacity: 1.0,
            globe_opacity: 0.6,
            world_lock_zoom: 15.0,
            draws_casing: false,
            minimum_tile_zoom: Self::DEFAULT_MINIMUM_TILE_ZOOM,
        }
    }
}

/// The biomes of the globe's overview: the colour of each of the fourteen
/// biomes of RESOLVE Ecoregions 2017 the engine lays into the tiles of the
/// overview zooms, the tones the tiles' own land cover draws over them in,
/// and the zooms of both. A biome is a region of one climate and
/// vegetation, so the palette is a physical atlas's: the forests in the
/// greens, deepest in the tropics and bluest in the taiga, the grasslands
/// and the savannas in the pale yellow-greens, the deserts alone in the
/// sand, the tundra in a grey-green a step off the ice. Everything that
/// grows is a green, so a cream on the map is dry ground and nothing else.
/// Every value is baked into the prepared tiles, so a change re-parses
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BiomeStyles {
    /// `tropical_moist_forest`: the Amazon, the Congo, Southeast Asia.
    pub tropical_moist_forest: Float4,
    /// `tropical_dry_forest`: the deciduous forests of India, Mexico, the
    /// Chaco.
    pub tropical_dry_forest: Float4,
    /// `tropical_conifer_forest`: the pine forests of Mexico and the
    /// Caribbean.
    pub tropical_conifer_forest: Float4,
    /// `temperate_broadleaf_forest`: Europe, the eastern United States,
    /// East Asia.
    pub temperate_broadleaf_forest: Float4,
    /// `temperate_conifer_forest`: the Rockies, the Cascades, the Alps,
    /// Siberia's south.
    pub temperate_conifer_forest: Float4,
    /// `boreal_forest`: the taiga of Canada, Scandinavia and Siberia.
    pub boreal_forest: Float4,
    /// `tropical_grassland`: the savannas of Africa, the Cerrado, northern
    /// Australia.
    pub tropical_grassland: Float4,
    /// `temperate_grassland`: the prairies, the pampas, the steppes.
    pub temperate_grassland: Float4,
    /// `flooded_grassland`: the Pantanal, the Sudd, the Okavango.
    pub flooded_grassland: Float4,
    /// `montane_grassland`: the Tibetan plateau, the Andean puna, the
    /// Ethiopian highlands.
    pub montane_grassland: Float4,
    /// `tundra`: the Arctic coasts, the Antarctic shore, the high north.
    pub tundra: Float4,
    /// `mediterranean_scrub`: the Mediterranean, California, the Cape,
    /// southwest Australia.
    pub mediterranean_scrub: Float4,
    /// `desert`: the Sahara, Arabia, Central Asia, the Australian outback.
    pub desert: Float4,
    /// `mangrove`: the tropical tidal forests.
    pub mangrove: Float4,
    /// The forests of the tiles' own land cover and land use drawn over the
    /// biomes, from `cover_minimum_tile_zoom`: a step darker than the
    /// forest biomes, so a wood reads on them.
    pub forest_cover: Float4,
    /// The fields drawn over the biomes: the hue of the temperate forest
    /// biomes they mostly lie on, a shade lighter (the same hue and chroma,
    /// the lightness up by a twentieth), so a plain reads as open land on a
    /// wooded biome and the greens layer in one tone, the biome, the fields
    /// on it, the woods on both. Not a wheat, which stood in one row with
    /// the deserts and made France read as the Sahara, and not a yellower
    /// green, which cut the fields out of the map instead of laying them on
    /// it.
    pub farmland_cover: Float4,
    /// The bare ground (the basemap's `barren`, the sand and the rock of
    /// the land use) drawn over the biomes: the sand.
    pub barren_cover: Float4,
    /// The deepest tile zoom the biomes draw at, 7 by default, the last
    /// zoom the basemap ships its land cover for. Through it the biomes
    /// paint the land in place of the tiles' own land cover and land use,
    /// which draw over them from `cover_minimum_tile_zoom`, and the biomes
    /// fade out with the land cover over camera zoom 7 to 8, where the
    /// street map's land tone and land use take over. The overview archive
    /// goes to tile zoom 7, so a deeper zoom draws no biomes. A lower zoom
    /// hands the land to the source's land cover earlier, and a negative
    /// zoom never draws the biomes.
    pub maximum_tile_zoom: i32,
    /// The tile zoom from which the tiles' own forests, fields, bare
    /// ground, wetlands and ice draw over the biomes, in the cover tones
    /// above, 2 by default. Under it the globe is the biomes alone, with the
    /// ice and the cities: a region of one climate in one tone. From it the
    /// cover gives the ground its structure, the fields against the woods,
    /// and the holes between the land cover's raster-derived blobs show the
    /// biome instead of bare land.
    pub cover_minimum_tile_zoom: i32,
    /// How many camera zooms the cover takes to come in over the biomes
    /// from `cover_minimum_tile_zoom`, 1 by default: the forests, the
    /// fields and the bare ground of the first cover tiles fade in with the
    /// camera over that span rather than appearing whole at the zoom, a
    /// fixed fade by the camera's zoom, not an animation. Zero shows them
    /// whole from the zoom. A span longer than one zoom fades the tiles of
    /// the next zooms too, while the camera is within it.
    pub cover_fade_in_zooms: f32,
}

impl Default for BiomeStyles {
    /// The atlas palette.
    fn default() -> Self {
        BiomeStyles {
            tropical_moist_forest: [0.624, 0.812, 0.541, 1.0],
            tropical_dry_forest: [0.706, 0.847, 0.580, 1.0],
            tropical_conifer_forest: [0.651, 0.820, 0.561, 1.0],
            temperate_broadleaf_forest: [0.667, 0.835, 0.576, 1.0],
            temperate_conifer_forest: [0.639, 0.812, 0.576, 1.0],
            boreal_forest: [0.686, 0.827, 0.643, 1.0],
            tropical_grassland: [0.812, 0.886, 0.643, 1.0],
            temperate_grassland: [0.788, 0.865, 0.654, 1.0],
            flooded_grassland: [0.741, 0.855, 0.698, 1.0],
            montane_grassland: [0.804, 0.863, 0.690, 1.0],
            tundra: [0.867, 0.894, 0.839, 1.0],
            mediterranean_scrub: [0.788, 0.851, 0.604, 1.0],
            desert: [0.949, 0.922, 0.808, 1.0],
            mangrove: [0.741, 0.855, 0.698, 1.0],
            forest_cover: [0.580, 0.770, 0.490, 1.0],
            farmland_cover: [0.728, 0.865, 0.654, 1.0],
            barren_cover: [0.950, 0.910, 0.780, 1.0],
            maximum_tile_zoom: 7,
            cover_minimum_tile_zoom: 2,
            cover_fade_in_zooms: 1.0,
        }
    }
}

impl BiomeStyles {
    /// The colours in a fixed order, for the fingerprint.
    pub fn colors(&self) -> [Float4; 17] {
        [
            self.tropical_moist_forest,
            self.tropical_dry_forest,
            self.tropical_conifer_forest,
            self.temperate_broadleaf_forest,
            self.temperate_conifer_forest,
            self.boreal_forest,
            self.tropical_grassland,
            self.temperate_grassland,
            self.flooded_grassland,
            self.montane_grassland,
            self.tundra,
            self.mediterranean_scrub,
            self.desert,
            self.mangrove,
            self.forest_cover,
            self.farmland_cover,
            self.barren_cover,
        ]
    }
}

/// The road network of the globe's overview: the Natural Earth 1:10m roads
/// the engine lays into the tiles of the overview zooms (layer `road`) in
/// place of the basemap's own roads, which at those zooms are the motorways
/// alone, a few pieces in most of the world, since the trunks arrive at
/// tile zoom 6. The network is one continuous line per road, ranked 3
/// (continental) to 7 (regional), and draws through `maximum_tile_zoom`,
/// where the basemap's trunks take over. The roads of the map's own tiles
/// are hidden through that zoom. The same tones are what the basemap's own
/// overview strokes wear on the globe past the network's zoom, through the
/// unroll into the plane, where the asphalt arrives. Every value is baked
/// into the prepared tiles, so a change re-parses them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OverviewRoadStyles {
    /// The main roads, rank up to `major_maximum_rank`: the yellow of an
    /// atlas's highways over the painted land.
    pub major: Float4,
    /// The rest of the network, thinner and paler.
    pub minor: Float4,
    pub major_width_points: f32,
    pub minor_width_points: f32,
    /// The last rank that draws as a main road, 4 by default: the
    /// continental routes and the first regional ones.
    pub major_maximum_rank: i32,
    /// The last tile zoom the network draws at, 5 by default, the zoom
    /// before the basemap ships its trunks. The archive carries the roads
    /// through tile zoom 5, so a higher value shows nothing more.
    pub maximum_tile_zoom: i32,
}

impl Default for OverviewRoadStyles {
    fn default() -> Self {
        OverviewRoadStyles {
            major: [0.961, 0.780, 0.302, 1.0],
            minor: [0.945, 0.863, 0.600, 1.0],
            major_width_points: 1.6,
            minor_width_points: 1.0,
            major_maximum_rank: 4,
            maximum_tile_zoom: 5,
        }
    }
}

/// The trees the street zooms' greenery grows (`TreeFillStyle`): the
/// engine's tree model stood over the fills, each kind at its own spacing
/// and height, `None` for a kind that grows none. When they draw is the
/// settings'. Baked into the prepared tiles, so a change re-parses them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TreeStyles {
    /// The trees of a wood or a forest (`landuse` forest and wood): a close
    /// stand, the crowns touching.
    pub wood: Option<TreeFillStyle>,
    /// The trees of the parks, the gardens, the cemeteries and the
    /// reserves: scattered, with lawn between them.
    pub park: Option<TreeFillStyle>,
    /// The trees the data maps one by one, at their points (the basemap's
    /// `pois` of kind `tree`, OpenStreetMap's `natural=tree`): the street
    /// trees, the trees of a square. `None` plants none.
    pub mapped: Option<TreePointStyle>,
    /// The colours of every tree (`TreePalette`), drawn at frame time: a
    /// change needs no re-parse.
    pub palette: TreePalette,
}

impl Default for TreeStyles {
    fn default() -> Self {
        TreeStyles {
            wood: Some(TreeFillStyle::new(9.0, 7.0..=11.0, TreeFillStyle::DEFAULT_ROAD_CLEARANCE_METRES)),
            park: Some(TreeFillStyle::new(22.0, 6.0..=10.0, TreeFillStyle::DEFAULT_ROAD_CLEARANCE_METRES)),
            mapped: Some(TreePointStyle::new(7.0..=12.0, 4.0)),
            palette: TreePalette::DAY,
        }
    }
}

impl TreeStyles {
    /// Every baked value in a fixed order, for the fingerprint: a kind that
    /// grows none contributes zeros. The palette is not among them.
    pub fn fingerprint_components(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(11);
        for style in [self.wood, self.park] {
            match style {
                Some(style) => out.extend([
                    style.spacing_metres,
                    style.minimum_height_metres,
                    style.maximum_height_metres,
                    style.road_clearance_metres,
                ]),
                None => out.extend([0.0; 4]),
            }
        }
        match self.mapped {
            Some(mapped) => {
                out.extend([mapped.minimum_height_metres, mapped.maximum_height_metres, mapped.crown_clearance_metres])
            }
            None => out.extend([0.0; 3]),
        }
        out
    }
}

/// The colours of the ground, the boundaries and the roads, with the
/// overview's biomes and road network and the trees.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LayerStyles {
    pub land: Float4,
    pub water: Float4,
    pub wood: Float4,
    pub grass: Float4,
    pub farmland: Float4,
    pub ice: Float4,
    pub sand: Float4,
    pub wetland: Float4,
    pub park: Float4,
    pub residential: Float4,
    pub industrial: Float4,
    pub boundary: Float4,
    pub aeroway: Float4,
    pub roads: RoadLayerStyles,
    /// The biomes of the globe's overview, over the land of the overview
    /// zooms in place of the land cover (`BiomeStyles`).
    pub biomes: BiomeStyles,
    /// The road network of the globe's overview (`OverviewRoadStyles`).
    pub overview_roads: OverviewRoadStyles,
    /// The trees of the street zooms' woods and parks (`TreeStyles`).
    pub trees: TreeStyles,
}

impl Default for LayerStyles {
    /// A light, warm, low-contrast palette in the manner of the system maps
    /// people already know: a warm off-white ground, soft pastel greens, a
    /// clear light blue for water, and asphalt-grey streets whose majors run
    /// wider, not darker. Contrast is spent on what carries meaning (water,
    /// parks, the road hierarchy) and taken out of everything that used to
    /// compete with the labels and the buildings for attention.
    fn default() -> Self {
        LayerStyles {
            land: [0.973, 0.965, 0.941, 1.0],
            water: [0.647, 0.812, 0.945, 1.0],
            // Landcover greens are opaque: they cover whole tiles (a tile can
            // be entirely forest or grass), and a translucent green over the
            // near-white `land` base reads as a washed, pale fill, and does
            // so per whole tile, so adjacent tiles jump in tone. Opaque keeps
            // the green saturated and consistent.
            wood: [0.667, 0.835, 0.576, 1.0],
            grass: [0.757, 0.886, 0.643, 1.0],
            // Farmland is a pale wheat, not a green: cultivated land covers
            // most of a continental plain (65% of a Central-Russia overview
            // tile), and as a green it merged with woods and grass into one
            // camouflage field. As a near-ground cream it recedes, and
            // forests read as green shapes on a light land the way region
            // maps draw them.
            farmland: [0.914, 0.922, 0.792, 1.0],
            ice: [0.937, 0.957, 0.973, 1.0],
            sand: [0.949, 0.922, 0.808, 1.0],
            // Wetland and bog cover huge areas in Russia's lowlands. A
            // near-grey tint made whole regions read as desaturated. A muted
            // green reads as the vegetation it is.
            wetland: [0.741, 0.855, 0.698, 1.0],
            park: [0.757, 0.886, 0.643, 1.0],
            // The built-up tints sit a hair under the ground: enough that a
            // city reads as a city over a region view, not enough to grey
            // the streets.
            residential: [0.961, 0.945, 0.914, 1.0],
            industrial: [0.949, 0.941, 0.925, 1.0],
            boundary: [0.52, 0.15, 0.72, 0.9],
            aeroway: [0.886, 0.882, 0.902, 1.0],
            roads: RoadLayerStyles::default(),
            biomes: BiomeStyles::default(),
            overview_roads: OverviewRoadStyles::default(),
            trees: TreeStyles::default(),
        }
    }
}

/// The buildings and what stands on them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FeatureStyles {
    pub building_fill_color: Float4,
    /// Whether buildings rise out of their footprints on the flat map. Off,
    /// every footprint stays a flat fill in the building colour, the way
    /// buildings draw on the globe. Baked into the prepared tiles, so a
    /// change re-parses them, like any other theme change.
    pub building_extrusion: bool,
    /// Whether the POIs and the house numbers inside an extruded building
    /// belong to it: the POI naming the building itself (a hotel, a mall,
    /// the same OSM element as the building) stands on its roof, the way a
    /// skyscraper's name sits on its top, and the places inside it stay on
    /// the ground but are never hidden by the building they are in, only by
    /// the buildings in front of it. Off, every label stays on the ground
    /// and any building can hide it. Baked into the prepared tiles, so a
    /// change re-parses them.
    pub labels_stand_on_roofs: bool,
    /// The radius, in metres, an extruded building's edges are rounded by
    /// (`ExtrusionStyle::edge_radius`): the corners of its footprint and
    /// the rim of its roof, so the blocks read soft rather than as sharp
    /// boxes. Zero keeps the boxes. Baked into the prepared tiles, so a
    /// change re-parses them.
    pub building_edge_radius: f32,
}

impl Default for FeatureStyles {
    /// A warm light grey a step under the ground: the roof of a building
    /// separates from the street around it, and its walls, which the
    /// renderer shades down from this, separate from the roof.
    fn default() -> Self {
        FeatureStyles {
            building_fill_color: [0.906, 0.890, 0.863, 1.0],
            building_extrusion: true,
            labels_stand_on_roofs: true,
            building_edge_radius: 1.5,
        }
    }
}

impl ProtomapsBasemapTheme {
    /// The colours of the trees (`TreeStyles::palette`).
    pub fn tree_palette(&self) -> TreePalette {
        self.layers.trees.palette
    }

    /// What no tile paints, from the same palette: the land where no tile
    /// has arrived, the water past the northern rim, the ice past the
    /// southern one. The caps are the globe's, so they wear the globe's
    /// palette where the theme has one.
    pub fn base_colors(&self) -> BaseColors {
        let globe = self.globe_layers.as_ref().unwrap_or(&self.layers);
        BaseColors { map: self.layers.land, north_cap: globe.water, south_cap: globe.ice }
    }

    /// A copy with the changes the closure makes: the way to state a theme
    /// as the default plus a few differences.
    pub fn apply(&self, change: impl FnOnce(&mut ProtomapsBasemapTheme)) -> ProtomapsBasemapTheme {
        let mut copy = self.clone();
        change(&mut copy);
        copy
    }

    pub fn with_labels(&self, update: impl FnOnce(&mut LabelStyles)) -> ProtomapsBasemapTheme {
        self.apply(|theme| update(&mut theme.labels))
    }

    pub fn with_label_visibility(&self, update: impl FnOnce(&mut LabelVisibility)) -> ProtomapsBasemapTheme {
        self.apply(|theme| update(&mut theme.label_visibility))
    }

    pub fn with_layers(&self, update: impl FnOnce(&mut LayerStyles)) -> ProtomapsBasemapTheme {
        self.apply(|theme| update(&mut theme.layers))
    }

    /// A copy whose globe wears its own palette, the changes made to the
    /// globe's palette so far or, where the theme has none, to `layers`.
    pub fn with_globe_layers(&self, update: impl FnOnce(&mut LayerStyles)) -> ProtomapsBasemapTheme {
        self.apply(|theme| {
            let mut globe = theme.globe_layers.unwrap_or(theme.layers);
            update(&mut globe);
            theme.globe_layers = Some(globe);
        })
    }

    /// A copy whose globe wears its own labels, the changes made to the
    /// globe's labels so far or, where the theme has none, to `labels`.
    pub fn with_globe_labels(&self, update: impl FnOnce(&mut LabelStyles)) -> ProtomapsBasemapTheme {
        self.apply(|theme| {
            let mut globe = theme.globe_labels.unwrap_or(theme.labels);
            update(&mut globe);
            theme.globe_labels = Some(globe);
        })
    }

    pub fn with_features(&self, update: impl FnOnce(&mut FeatureStyles)) -> ProtomapsBasemapTheme {
        self.apply(|theme| update(&mut theme.features))
    }

    pub fn with_road_metrics(&self, update: impl FnOnce(&mut RoadMetrics)) -> ProtomapsBasemapTheme {
        self.apply(|theme| update(&mut theme.road_metrics))
    }

    pub fn with_route_shields(&self, update: impl FnOnce(&mut RouteShields)) -> ProtomapsBasemapTheme {
        self.apply(|theme| update(&mut theme.route_shields))
    }

    /// FNV-1a over every palette component so a recolor changes disk-cache
    /// identity. A negative zero hashes as zero, so two themes that compare
    /// equal hash alike.
    pub fn cache_fingerprint(&self) -> u32 {
        let mut hash: u64 = 1469598103934665603;
        for value in self.palette_components() {
            let bits = if value == 0.0 { 0.0f32.to_bits() } else { value.to_bits() };
            for byte in bits.to_le_bytes() {
                hash ^= byte as u64;
                hash = hash.wrapping_mul(1099511628211);
            }
        }
        let folded = (hash as u32) ^ ((hash >> 32) as u32);
        if folded == 0 { 1 } else { folded }
    }

    fn palette_components(&self) -> Vec<f32> {
        let mut out: Vec<f32> = Vec::with_capacity(1024);
        fn add_appearance(out: &mut Vec<f32>, appearance: &LabelAppearance) {
            out.extend(appearance.fill_color);
            out.extend(appearance.stroke_color);
            out.extend([
                appearance.halo_em,
                appearance.size_points,
                appearance.weight.raw_value() as f32,
                if appearance.uppercased { 1.0 } else { 0.0 },
                appearance.embolden_points,
                appearance.letter_spacing_em,
                appearance.opacity,
                appearance.halo_softness,
            ]);
        }
        fn add_layers(out: &mut Vec<f32>, layers: &LayerStyles) {
            for color in [
                layers.land,
                layers.water,
                layers.wood,
                layers.grass,
                layers.farmland,
                layers.ice,
                layers.sand,
                layers.wetland,
                layers.park,
                layers.residential,
                layers.industrial,
                layers.boundary,
                layers.aeroway,
                layers.roads.motorway,
                layers.roads.trunk,
                layers.roads.primary,
                layers.roads.secondary,
                layers.roads.tertiary,
                layers.roads.minor,
                layers.roads.service,
                layers.roads.path,
                layers.roads.rail,
                layers.roads.casing,
                layers.roads.marking,
            ] {
                out.extend(color);
            }
            // The biomes are baked into the overview zooms' tiles, and the
            // zoom they stop at decides which tiles carry them at all.
            for color in layers.biomes.colors() {
                out.extend(color);
            }
            out.push(layers.biomes.maximum_tile_zoom as f32);
            out.push(layers.biomes.cover_minimum_tile_zoom as f32);
            out.push(layers.biomes.cover_fade_in_zooms);
            // The overview's road network, baked into the same tiles.
            out.extend(layers.overview_roads.major);
            out.extend(layers.overview_roads.minor);
            out.extend([
                layers.overview_roads.major_width_points,
                layers.overview_roads.minor_width_points,
                layers.overview_roads.major_maximum_rank as f32,
                layers.overview_roads.maximum_tile_zoom as f32,
            ]);
            // The trees are placed at parse time over the fills that grow
            // them.
            out.extend(layers.trees.fingerprint_components());
        }
        add_layers(&mut out, &self.layers);
        // The globe's palette is baked into the same tiles as the colours
        // the ground wears on the sphere, none where the theme has none.
        match &self.globe_layers {
            Some(globe) => {
                out.push(1.0);
                add_layers(&mut out, globe);
            }
            None => out.push(0.0),
        }
        out.extend(self.features.building_fill_color);
        out.push(if self.features.building_extrusion { 1.0 } else { 0.0 });
        out.push(if self.features.labels_stand_on_roofs { 1.0 } else { 0.0 });
        out.push(self.features.building_edge_radius);
        let labels = &self.labels;
        for appearance in
            [&labels.city, &labels.town, &labels.district, &labels.country, &labels.region, &labels.poi, &labels.water, &labels.road]
        {
            add_appearance(&mut out, appearance);
        }
        for color in labels.poi_categories.colors() {
            out.extend(color);
        }
        // The globe's labels are baked into the tiles as the colours a name
        // wears on the sphere, none where the theme has none.
        match &self.globe_labels {
            Some(globe) => {
                out.push(1.0);
                for appearance in
                    [&globe.city, &globe.town, &globe.district, &globe.country, &globe.region, &globe.poi, &globe.water, &globe.road]
                {
                    add_appearance(&mut out, appearance);
                }
            }
            None => out.push(0.0),
        }
        // Not palette values, but they change which labels are drawn, so
        // they must participate in the disk-cache identity.
        let visibility = &self.label_visibility;
        out.push(visibility.poi_iconless_minimum_zoom as f32);
        out.push(visibility.poi_minimum_zoom as f32);
        out.push(if visibility.poi_requires_icon { 1.0 } else { 0.0 });
        out.push(if visibility.poi_follows_source_minimum_zoom { 1.0 } else { 0.0 });
        out.extend(
            [
                visibility.poi_category_minimum_zoom.campus,
                visibility.poi_category_minimum_zoom.shop,
                visibility.poi_category_minimum_zoom.service,
                visibility.poi_category_minimum_zoom.transit_detail,
                visibility.poi_long_name_minimum_zoom,
                visibility.poi_long_name_character_count,
                visibility.address_minimum_zoom,
                visibility.country_maximum_zoom,
                visibility.region_maximum_zoom,
            ]
            .map(|value| value as f32),
        );
        out.push(if visibility.local_detail_in_look_at_tile_only { 1.0 } else { 0.0 });
        // The road metrics are baked into the tiles' line styles and decide
        // which classes a tile carries at all.
        let metrics = &self.road_metrics;
        out.extend(metrics.symbol_width_points.all());
        out.extend(metrics.overview_width_points.all());
        out.extend([metrics.overview_zoom, metrics.symbol_zoom, metrics.overview_opacity]);
        out.push(metrics.globe_opacity);
        out.push(metrics.world_lock_zoom);
        out.push(if metrics.draws_casing { 1.0 } else { 0.0 });
        out.extend(metrics.minimum_tile_zoom.all().map(|value| value as f32));
        // The route signs are baked into the tiles as labels.
        out.extend(self.route_shields.fingerprint_components());
        out
    }
}
