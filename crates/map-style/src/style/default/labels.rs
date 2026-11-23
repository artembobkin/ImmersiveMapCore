// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The labels: every named point of `places`, `water` and `pois` and every
//! house number of `buildings` becomes a label on each tile that ships it.
//! The POIs and the house numbers wait for the camera zoom the theme's
//! `label_visibility` gives them, and how the rest rank against each other
//! decides which of them the collisions keep. The road names are laid
//! along the roads themselves (`road_style`).

use crate::schema::{FeatureFacts, FeatureProperties};
use crate::style::default::rules::{lowercased, parse_int_value};
use crate::style::default::{LabelAppearance, LabelStyles, PoiCategory, ProtomapsBasemapDefaultMapStyle};
use crate::style::{
    FeatureStyle, LabelFontWeight, LabelPlacement, PoiSpriteIcon, PointLabelOptions, SurfaceLabelPlacement,
};
use crate::Float3;

/// The collision bands: the countries, the regions and the cities beat the
/// peaks beat the towns and every smaller place beat the water names beat
/// the airports beat the route signs beat the other POIs beat the house
/// numbers, whatever their ranks. A peak stands ahead of the settlements
/// because it is what a mountain view is of: the Matterhorn's name over the
/// hamlets on its slopes, not under them, while a city's name still wins
/// the frame it is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LabelBand {
    /// A country, a region, a city.
    Place,
    /// A peak or a volcano.
    Peak,
    /// A town, a village, a hamlet, a neighbourhood: every place under a
    /// city.
    Settlement,
    Water,
    /// An airport.
    Landmark,
    RouteShield,
    Poi,
    Address,
}

impl LabelBand {
    pub fn base(self) -> i32 {
        match self {
            LabelBand::Place => 0,
            LabelBand::Peak => 10_000,
            LabelBand::Settlement => 15_000,
            LabelBand::Water => 20_000,
            LabelBand::Landmark => 40_000,
            LabelBand::RouteShield => 45_000,
            LabelBand::Poi => 50_000,
            LabelBand::Address => 70_000,
        }
    }
}

/// Which slot of the theme's labels a place is set in.
type LabelSlot = fn(&LabelStyles) -> &LabelAppearance;

impl ProtomapsBasemapDefaultMapStyle {
    /// A feature without a rank is the least important thing in its layer.
    pub const UNRANKED_LABEL_RANK: i32 = 1_000;

    /// The tracking of a water name, in ems.
    pub const WATER_LABEL_LETTER_SPACING_EM: f32 = 0.15;

    /// A place's importance, lower first: the basemap's `population_rank`
    /// (0 to 17, higher is bigger) turned around, and the kind of place
    /// breaking the ties, a country before its regions before its towns.
    pub fn place_rank(&self, kind: Option<&str>, props: &FeatureProperties<'_, '_>) -> i32 {
        let Some(population_rank) = parse_int_value(props.get("population_rank")) else {
            return Self::UNRANKED_LABEL_RANK;
        };
        let kind_offset = match kind {
            Some("country") => 0,
            Some("region") => 1,
            Some("locality") => 2,
            Some("macrohood") => 3,
            _ => 4,
        };
        ((18 - population_rank).max(0) * 10 + kind_offset) as i32
    }

    /// A water body's or a POI's importance: the tile zoom the basemap
    /// ships it from, then its `sort_rank`.
    pub fn min_zoom_rank(&self, props: &FeatureProperties<'_, '_>) -> i32 {
        let Some(min_zoom) = parse_int_value(props.get("min_zoom")) else {
            return Self::UNRANKED_LABEL_RANK;
        };
        (min_zoom * 100 + parse_int_value(props.get("sort_rank")).unwrap_or(0)) as i32
    }

    /// Whether a place is a city or bigger: the places that keep their band
    /// ahead of the peaks (`LabelBand::Place`).
    pub fn is_major_place(kind: Option<&str>, kind_detail: Option<&str>) -> bool {
        match kind {
            Some("country" | "region") => true,
            Some("locality") => kind_detail == Some("city"),
            _ => false,
        }
    }

    pub fn label_collision_rank(band: LabelBand, rank: i32) -> i32 {
        band.base() + rank
    }

    /// The `capital` attribute is the raw OSM value, present on a capital
    /// of any level.
    pub fn is_capital(&self, props: &FeatureProperties<'_, '_>) -> bool {
        match lowercased(props, "capital") {
            Some(capital) => !capital.is_empty() && capital != "no",
            None => parse_int_value(props.get("capital")).is_some_and(|value| value > 0),
        }
    }

    pub fn place_label_style(
        &self,
        kind: Option<&str>,
        kind_detail: Option<&str>,
        props: &FeatureProperties<'_, '_>,
    ) -> FeatureStyle {
        let visibility = &self.theme.label_visibility;
        // The slot of the theme's labels the place is set in, read from the
        // plane's labels and, for the colours on the globe, from the
        // globe's. The name of a land goes away where the land no longer
        // fits the view (`LabelVisibility::country_maximum_zoom`,
        // `region_maximum_zoom`).
        let (slot, max_camera_zoom): (LabelSlot, f32) = match kind {
            Some("country") => (|labels| &labels.country, visibility.country_maximum_zoom as f32),
            Some("region") => (|labels| &labels.region, visibility.region_maximum_zoom as f32),
            Some("locality") => match kind_detail {
                Some("city") => (|labels| &labels.city, f32::INFINITY),
                Some("town") => (|labels| &labels.town, f32::INFINITY),
                // A village, a hamlet, and the rest: in a city, its named
                // corners (a square, a quarter), which read as its districts
                // do, never in a city's deep black.
                _ => (|labels| &labels.district, f32::INFINITY),
            },
            // A macrohood, a neighbourhood.
            _ => (|labels| &labels.district, f32::INFINITY),
        };
        let mut appearance = *slot(&self.theme.labels);
        // A national capital is a city carrying the `capital` attribute.
        if kind == Some("locality") && kind_detail == Some("city") && self.is_capital(props) {
            appearance.size_points += 1.5;
            appearance.weight = LabelFontWeight::Bold;
        }
        let rank = self.place_rank(kind, props);
        let band = if Self::is_major_place(kind, kind_detail) { LabelBand::Place } else { LabelBand::Settlement };
        self.point_label(
            70,
            band,
            &appearance,
            self.theme.globe_labels.as_ref().map(slot),
            PointLabelOptions { rank, max_camera_zoom, ..PointLabelOptions::default() },
        )
    }

    /// A water body's name lies on the water: it turns and tilts with the
    /// map and grows with the zoom, the way an atlas letters a sea. It is
    /// ink on the water rather than a label over it: no halo, a deeper blue
    /// of the water's own hue (the theme's water label), the letters spaced
    /// out.
    pub fn water_label_style(&self, kind: Option<&str>, props: &FeatureProperties<'_, '_>, tile_zoom: i32) -> FeatureStyle {
        let mut appearance = self.theme.labels.water;
        if kind == Some("ocean") {
            appearance.size_points += 3.0;
        }
        let minimum_zoom = parse_int_value(props.get("min_zoom")).unwrap_or(tile_zoom as i64);
        self.point_label(
            73,
            LabelBand::Water,
            &appearance,
            self.theme.globe_labels.as_ref().map(|labels| &labels.water),
            PointLabelOptions {
                rank: self.min_zoom_rank(props),
                placement: LabelPlacement::Surface(Self::water_label_surface_placement(minimum_zoom)),
                ..PointLabelOptions::default()
            },
        )
    }

    /// Where a water name painted on the map shows. The basemap ships the
    /// name from the tile zoom `minimum_zoom`, which the engine draws from
    /// that camera zoom on: the name shows from there at its point size,
    /// grows with the map from half a zoom in, and stays on the water at
    /// every deeper zoom, covering the same ground as the water it names.
    pub fn water_label_surface_placement(minimum_zoom: i64) -> SurfaceLabelPlacement {
        let start = minimum_zoom.max(0) as f64;
        SurfaceLabelPlacement {
            minimum_zoom: start,
            letter_spacing_em: Self::WATER_LABEL_LETTER_SPACING_EM,
            ..SurfaceLabelPlacement::new(start + 0.5)
        }
    }

    /// The landmarks among the POIs: the peaks and the airports, which take
    /// their own keys and collision bands (`LabelBand::Peak`, `Landmark`).
    pub fn is_landmark_poi(kind: Option<&str>) -> bool {
        matches!(kind, Some("peak" | "volcano" | "aerodrome"))
    }

    /// The band a POI collides in: the peaks ahead of the settlements, the
    /// airports behind the water names, the rest behind the route signs.
    pub fn poi_band(kind: Option<&str>) -> LabelBand {
        match kind {
            Some("peak" | "volcano") => LabelBand::Peak,
            Some("aerodrome") => LabelBand::Landmark,
            _ => LabelBand::Poi,
        }
    }

    /// A POI: both the icon circle and the label are tinted in the venue
    /// category colour. The colour flows through `LabelTextStyle::fill_color`,
    /// used by both the icon background and the text fill. The icon glyph
    /// is white. All POIs but the landmarks share one key (72): runs are
    /// grouped by full style identity (weight and colours), so different
    /// categories land in separate draw runs.
    pub fn poi_label_style(&self, kind: Option<&str>, props: &FeatureProperties<'_, '_>) -> FeatureStyle {
        let visibility = &self.theme.label_visibility;
        // The basemap ranks a POI by the zoom it belongs to: its deepest
        // tile (z15) ships the whole street, down to the cafés at 16, the
        // shops at 17 and the bus stops at 18, and the POI waits for that
        // camera zoom instead of crowding the ones ranked above it.
        let mut min_camera_zoom: f32 = 0.0;
        if visibility.poi_follows_source_minimum_zoom
            && let Some(source_minimum_zoom) = parse_int_value(props.get("min_zoom"))
        {
            min_camera_zoom = source_minimum_zoom as f32;
        }
        if let Some(category) = Self::poi_category(kind) {
            min_camera_zoom = min_camera_zoom.max(visibility.poi_category_minimum_zoom.zoom(category) as f32);
        }
        if let Some(name) = props.str("name")
            && name.chars().count() as i64 > visibility.poi_long_name_character_count as i64
        {
            min_camera_zoom = min_camera_zoom.max(visibility.poi_long_name_minimum_zoom as f32);
        }
        let icon = Self::poi_icon(kind);
        let is_iconless = icon.is_none();
        if is_iconless {
            // A category the icon set does not know draws its name alone, in
            // the category colour: a stand-in dot says nothing the name does
            // not. A theme that wants such POIs gone (`poi_requires_icon`) or
            // held back to a zoom (`poi_iconless_minimum_zoom`) says so.
            if visibility.poi_requires_icon {
                return FeatureStyle::Hidden;
            }
            min_camera_zoom = min_camera_zoom.max(visibility.poi_iconless_minimum_zoom as f32);
        }
        // The global POI floor is an absolute visibility gate that may
        // exceed everything above (up to hiding POIs entirely).
        min_camera_zoom = min_camera_zoom.max(visibility.poi_minimum_zoom as f32);

        let is_landmark = Self::is_landmark_poi(kind);
        let mut appearance = self.theme.labels.poi;
        appearance.fill_color = self.poi_category_color(kind);
        let key = match kind {
            Some("peak" | "volcano") => 74,
            Some("aerodrome") => 75,
            _ => 72,
        };
        // An icon-less POI is local detail: a plaque, an office, a ticket
        // window, read standing next to it and left to the look-at tile.
        self.point_label(
            key,
            Self::poi_band(kind),
            &appearance,
            None,
            PointLabelOptions {
                rank: self.min_zoom_rank(props),
                min_camera_zoom,
                icon,
                stands_on_roof: self.theme.features.labels_stand_on_roofs,
                is_local: is_iconless && !is_landmark && visibility.local_detail_in_look_at_tile_only,
                ..PointLabelOptions::default()
            },
        )
    }

    /// A house number: the address points of the `buildings` layer, which
    /// the schema reads as a label carrying the number. Small grey text with
    /// no icon, below every other label in the collisions, so a number shows
    /// only where the street and its places leave room.
    pub fn address_label_style(&self, facts: &FeatureFacts<'_>) -> FeatureStyle {
        if facts.label().is_none() {
            return FeatureStyle::Hidden;
        }
        let mut appearance = self.theme.labels.poi;
        appearance.size_points -= 2.0;
        appearance.weight = LabelFontWeight::Thin;
        appearance.fill_color = [0.45, 0.45, 0.47];
        let visibility = &self.theme.label_visibility;
        self.point_label(
            76,
            LabelBand::Address,
            &appearance,
            None,
            PointLabelOptions {
                rank: 0,
                min_camera_zoom: visibility.address_minimum_zoom as f32,
                stands_on_roof: self.theme.features.labels_stand_on_roofs,
                is_local: visibility.local_detail_in_look_at_tile_only,
                ..PointLabelOptions::default()
            },
        )
    }

    /// The POI categories that wait for a zoom of their own
    /// (`PoiCategoryZooms`), from the POI's `kind`, the raw OSM value.
    /// `None` for the rest, which follow the basemap's rank alone: the
    /// landmarks, museums and theatres, the parks, the stations, the
    /// hospitals, the places of worship, the food and drink, the hotels, the
    /// malls and department stores.
    pub fn poi_category(kind: Option<&str>) -> Option<PoiCategory> {
        match kind {
            Some("university" | "college" | "research_institute") => Some(PoiCategory::Campus),
            Some("mall" | "department_store" | "marketplace") => None,
            Some(
                "post_office" | "post_box" | "parcel_locker" | "bank" | "atm" | "bureau_de_change" | "money_transfer"
                | "pharmacy" | "chemist" | "clinic" | "doctors" | "dentist" | "school" | "kindergarten" | "childcare"
                | "music_school" | "language_school" | "driving_school" | "townhall" | "civic_admin"
                | "administrative" | "government" | "courthouse" | "embassy" | "police" | "fire_station"
                | "community_centre" | "social_facility" | "toilets" | "fuel" | "charging_station" | "car_wash"
                | "car_repair" | "veterinary" | "hairdresser" | "laundry" | "dry_cleaning" | "copyshop",
            ) => Some(PoiCategory::Service),
            Some(
                "bus_stop" | "stop" | "tram_stop" | "platform" | "subway_entrance" | "halt" | "taxi"
                | "ticket_validator" | "parking" | "parking_entrance" | "parking_space" | "bicycle_parking"
                | "motorcycle_parking" | "kick-scooter_parking" | "car_sharing" | "bicycle_rental" | "mobility_hub",
            ) => Some(PoiCategory::TransitDetail),
            _ => (Self::poi_icon(kind) == Some(PoiSpriteIcon::Shopping)).then_some(PoiCategory::Shop),
        }
    }

    /// The sprite a POI draws beside its name, from its `kind`, the raw OSM
    /// value. `None` for a category with no symbol of its own, which draws
    /// its name alone or not at all (`poi_requires_icon`).
    pub fn poi_icon(kind: Option<&str>) -> Option<PoiSpriteIcon> {
        let icon = match kind? {
            "restaurant" | "fast_food" | "food_court" | "ice_cream" => PoiSpriteIcon::Restaurant,
            "cafe" | "bakery" | "pastry" | "coffee" | "tea" | "juice_bar" | "internet_cafe" => PoiSpriteIcon::Cafe,
            "bar" | "pub" | "biergarten" | "nightclub" => PoiSpriteIcon::Bar,
            "park" | "garden" | "national_park" | "nature_reserve" | "dog_park" => PoiSpriteIcon::Park,
            "museum" | "gallery" | "artwork" => PoiSpriteIcon::Museum,
            "hospital" | "clinic" | "doctors" | "dentist" => PoiSpriteIcon::Hospital,
            "school" | "college" | "university" | "kindergarten" | "library" => PoiSpriteIcon::School,
            "aerodrome" | "airfield" | "heliport" => PoiSpriteIcon::Airport,
            "stadium" | "sports_centre" | "pitch" | "fitness_centre" | "sports" | "dojo" | "ice_rink"
            | "swimming_pool" | "playground" | "golf_course" => PoiSpriteIcon::Stadium,
            "hotel" | "hostel" | "guest_house" | "motel" => PoiSpriteIcon::Hotel,
            "supermarket" | "mall" | "convenience" | "department_store" | "marketplace" | "grocery" | "kiosk"
            | "deli" | "alcohol" | "wine" | "cheese" | "seafood" | "confectionery" | "health_food" | "greengrocer"
            | "butcher" | "retail" | "clothes" | "shoes" | "bag" | "fashion_accessories" | "jewelry" | "watches"
            | "gift" | "books" | "stationery" | "newsagent" | "toys" | "electronics" | "mobile_phone" | "hifi"
            | "hardware" | "doityourself" | "furniture" | "houseware" | "florist" | "cosmetics" | "perfumery"
            | "beauty" | "optician" | "pet" | "bicycle" | "car" | "second_hand" | "antiques" | "art" | "music"
            | "musical_instrument" | "photo" | "fabric" | "tobacco" | "e-cigarette" | "cannabis"
            | "variety_store" | "baby_goods" => PoiSpriteIcon::Shopping,
            "fuel" | "charging_station" => PoiSpriteIcon::GasStation,
            "pharmacy" | "chemist" => PoiSpriteIcon::Pharmacy,
            "viewpoint" | "attraction" | "peak" | "volcano" => PoiSpriteIcon::Viewpoint,
            "station" | "halt" | "tram_stop" | "subway_entrance" | "platform" => PoiSpriteIcon::Train,
            "bus_stop" | "bus_station" | "stop" | "taxi" | "ferry_terminal" | "mobility_hub" => PoiSpriteIcon::Transit,
            "parking" | "parking_entrance" | "parking_space" | "bicycle_parking" | "motorcycle_parking"
            | "kick-scooter_parking" | "car_sharing" | "bicycle_rental" => PoiSpriteIcon::Parking,
            "bank" | "atm" | "bureau_de_change" | "money_transfer" => PoiSpriteIcon::Bank,
            "theatre" | "cinema" | "arts_centre" | "music_venue" | "events_venue" | "conference_centre" => {
                PoiSpriteIcon::Theatre
            }
            "toilets" => PoiSpriteIcon::Toilets,
            "townhall" | "police" | "fire_station" | "post_office" | "courthouse" | "civic_admin" | "administrative"
            | "community_centre" | "embassy" | "social_facility" => PoiSpriteIcon::Civic,
            "place_of_worship" | "religious" | "religious_administration" | "monastery" => PoiSpriteIcon::Worship,
            _ => return None,
        };
        Some(icon)
    }

    /// The plate of a POI's icon and the colour of its name, by the category
    /// its kind falls in (`PoiCategoryColors`), the POI appearance's own fill
    /// for a kind of no category.
    pub fn poi_category_color(&self, kind: Option<&str>) -> Float3 {
        let plates = &self.theme.labels.poi_categories;
        match kind {
            Some("restaurant" | "fast_food" | "food_court" | "ice_cream") => plates.food,
            Some("cafe" | "bakery") => plates.coffee,
            Some("bar" | "pub" | "biergarten" | "nightclub" | "wine") => plates.bar,
            Some(
                "supermarket" | "mall" | "convenience" | "department_store" | "marketplace" | "gift" | "hairdresser"
                | "hardware" | "laundry" | "florist" | "jewelry" | "shoes" | "clothes",
            ) => plates.shop,
            Some("hotel" | "hostel" | "guest_house" | "motel") => plates.hotel,
            Some("hospital" | "pharmacy" | "chemist" | "doctors" | "dentist" | "clinic") => plates.health,
            Some("school" | "college" | "university" | "kindergarten" | "library") => plates.education,
            Some(
                "museum" | "gallery" | "attraction" | "artwork" | "theatre" | "cinema" | "arts_centre"
                | "music_venue" | "events_venue" | "conference_centre",
            ) => plates.culture,
            Some(
                "park" | "garden" | "national_park" | "nature_reserve" | "dog_park" | "stadium" | "pitch"
                | "sports_centre" | "swimming_pool" | "golf_course" | "playground" | "picnic_site",
            ) => plates.leisure,
            Some(
                "station" | "aerodrome" | "airfield" | "heliport" | "fuel" | "charging_station" | "car_rental"
                | "ferry_terminal" | "harbour" | "halt" | "tram_stop" | "subway_entrance" | "platform" | "bus_stop"
                | "bus_station" | "stop" | "taxi" | "mobility_hub" | "parking" | "parking_entrance"
                | "parking_space" | "bicycle_parking" | "motorcycle_parking" | "kick-scooter_parking"
                | "car_sharing" | "bicycle_rental",
            ) => plates.transport,
            Some(
                "bank" | "post_office" | "townhall" | "police" | "fire_station" | "government" | "atm"
                | "bureau_de_change" | "money_transfer" | "courthouse" | "civic_admin" | "administrative"
                | "community_centre" | "embassy" | "social_facility" | "toilets",
            ) => plates.civic,
            Some("place_of_worship" | "religious" | "religious_administration" | "monastery") => plates.worship,
            _ => self.theme.labels.poi.fill_color,
        }
    }

    /// A point label of an appearance in a collision band. `globe_appearance`
    /// is the slot of the globe's labels the label's colours on the globe
    /// come from, `None` for a label never on the globe (a POI, an address)
    /// or a theme whose globe wears the plane's labels. A label with an
    /// icon is a POI's: its plate and its name fade with the POIs
    /// (`LabelOpacity::pois`). The options' collision rank is the band's.
    pub fn point_label(
        &self,
        key: u8,
        band: LabelBand,
        appearance: &LabelAppearance,
        globe_appearance: Option<&LabelAppearance>,
        options: PointLabelOptions,
    ) -> FeatureStyle {
        let text = self.label_text_style(key as i32, appearance, globe_appearance, options.icon.is_some());
        FeatureStyle::point_label(
            key,
            text,
            PointLabelOptions { collision_rank: Some(Self::label_collision_rank(band, options.rank)), ..options },
        )
    }
}
