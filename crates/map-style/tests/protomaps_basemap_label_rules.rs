// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Ported from `ProtomapsBasemapLabelRulesTests` and the label parts of
//! `LabelScreenUnitTests` and `SurfaceLabelTests`.
//!
//! The built-in style's label rules, read through the same `make_style` the
//! parser calls: every named point the tile ships becomes a label, and how
//! the labels rank decides which of them the collisions keep. The rank of a
//! place is the basemap's `population_rank` turned around (lower is more
//! important), and a place without one is the least important thing in its
//! layer. A water name and a POI rank by the tile zoom the basemap ships
//! them from (`min_zoom`).

mod support;

use map_style::schema::FeatureGeometry;
use map_style::style::default::{ProtomapsBasemapDefaultMapStyle, ProtomapsBasemapTheme};
use map_style::style::{FeatureStyle, LabelFontWeight, LabelPlacement, LabelTypeScale, SurfaceLabelPlacement};
use mvt::Value;
use support::*;

fn place(kind: &'static str, kind_detail: &'static str, population_rank: Option<i64>, extra: Props, zoom: i32) -> FeatureStyle {
    let mut props: Props = vec![("name", s("Test")), ("kind", s(kind)), ("kind_detail", s(kind_detail))];
    if let Some(rank) = population_rank {
        props.push(("population_rank", i(rank)));
    }
    props.extend(extra);
    style_of(&default_style(), "places", &props, zoom, FeatureGeometry::Point)
}

fn city(population_rank: Option<i64>) -> FeatureStyle {
    place("locality", "city", population_rank, vec![], 10)
}

fn poi(kind: &'static str, min_zoom: Option<i64>, zoom: i32) -> FeatureStyle {
    let mut props: Props = vec![("name", s("Test")), ("kind", s(kind))];
    if let Some(min_zoom) = min_zoom {
        props.push(("min_zoom", i(min_zoom)));
    }
    style_of(&default_style(), "pois", &props, zoom, FeatureGeometry::Point)
}

fn point(layer: &str, props: &[(&str, Value<'_>)], zoom: i32) -> FeatureStyle {
    style_of(&default_style(), layer, props, zoom, FeatureGeometry::Point)
}

fn is_label(style: &FeatureStyle) -> bool {
    style.point_label_style().is_some()
}

#[test]
fn a_bigger_population_rank_is_a_more_important_place() {
    assert!(label_rank(&city(Some(17))) < label_rank(&city(Some(5))));
    assert!(
        label_rank(&place("locality", "city", Some(17), vec![], 10))
            < label_rank(&place("locality", "town", Some(5), vec![], 10)),
        "a rank 17 city beats a rank 5 town"
    );
}

#[test]
fn absent_population_rank_is_least_important_even_with_legacy_signals_present() {
    let no_rank = label_rank(&city(None));
    assert_eq!(label_rank(&place("locality", "city", None, vec![("capital", s("yes"))], 10)), no_rank);
    assert_eq!(label_rank(&place("locality", "city", None, vec![("population", i(13_000_000))], 10)), no_rank);
    assert!(no_rank > label_rank(&city(Some(0))), "absent rank sits below every ranked place");
}

#[test]
fn the_kind_breaks_ties_country_first() {
    let country = label_rank(&place("country", "country", Some(10), vec![], 10));
    let region = label_rank(&place("region", "state", Some(10), vec![], 10));
    let city = label_rank(&place("locality", "city", Some(10), vec![], 10));
    assert!(country < region);
    assert!(region < city);
}

/// The bands: a city before a peak before a town before a lake before an
/// airport before a restaurant, whatever their own ranks.
#[test]
fn collision_rank_orders_the_layers_places_first() {
    let city_style = city(Some(5));
    let town = place("locality", "town", Some(17), vec![], 10);
    let water = point("water", &[("kind", s("lake")), ("name", s("Lake")), ("min_zoom", i(4))], 10);
    let peak = poi("peak", Some(9), 10);
    let airport = poi("aerodrome", Some(9), 10);
    let restaurant = poi("restaurant", Some(13), 14);
    assert_eq!(label_collision_rank(&city_style), label_rank(&city_style), "a city collides at its own rank");
    assert!(label_collision_rank(&city_style) < label_collision_rank(&peak));
    assert!(label_collision_rank(&peak) < label_collision_rank(&town), "The Matterhorn's name over the villages on its slopes");
    assert!(label_collision_rank(&town) < label_collision_rank(&water));
    assert!(label_collision_rank(&water) < label_collision_rank(&airport));
    assert!(label_collision_rank(&airport) < label_collision_rank(&restaurant));
}

/// Which places keep their band ahead of the peaks: a country, a region, a
/// city. A town, a village, a hamlet and a neighbourhood yield to a peak
/// whatever their population rank.
#[test]
fn a_peak_beats_every_place_under_a_city() {
    let peak = label_collision_rank(&poi("peak", Some(9), 10));
    for (kind, detail) in [
        ("locality", "town"),
        ("locality", "village"),
        ("locality", "hamlet"),
        ("locality", "locality"),
        ("neighbourhood", "neighbourhood"),
        ("macrohood", "macrohood"),
    ] {
        assert!(label_collision_rank(&place(kind, detail, Some(17), vec![], 10)) > peak, "{kind} {detail}");
    }
    for (kind, detail) in [("country", "country"), ("region", "state"), ("locality", "city")] {
        assert!(label_collision_rank(&place(kind, detail, Some(0), vec![], 10)) < peak, "{kind} {detail}");
    }
}

/// A water name lies on the water, painted on the map from the zoom the
/// basemap ships it at and at every deeper zoom. Every other label stands
/// on the screen.
#[test]
fn a_water_name_is_painted_on_the_map() {
    let lake = point("water", &[("kind", s("lake")), ("name", s("Lake")), ("min_zoom", i(6))], 8);
    let label = lake.point_label_style().expect("a lake's name is a label");
    let LabelPlacement::Surface(placement) = label.placement else {
        panic!("a lake's name is a label painted on the map");
    };
    assert_eq!(placement.reference_zoom, 6.5);
    assert!(!placement.is_visible_at_zoom(5.9), "not before the zoom the basemap ships it at");
    assert!(placement.is_visible_at_zoom(6.0));
    assert!(placement.is_visible_at_zoom(8.0));
    assert!(placement.is_visible_at_zoom(16.0), "still on the water at street zoom");
    assert!(placement.letter_spacing_em > 0.0, "a water name is spaced out");
    assert_eq!(label.text.halo_em, 0.0, "ink on the water, with no halo");
    assert_eq!(city(Some(5)).point_label_style().unwrap().placement, LabelPlacement::Screen);
}

/// An ocean the coarsest tiles carry is there from the widest view.
#[test]
fn an_ocean_name_of_the_coarsest_tiles_shows_from_the_widest_view() {
    let ocean = point("water", &[("kind", s("ocean")), ("name", s("Ocean")), ("min_zoom", i(0))], 0);
    let LabelPlacement::Surface(placement) = ocean.point_label_style().unwrap().placement else {
        panic!("an ocean's name is a label painted on the map");
    };
    assert!(placement.is_visible_at_zoom(0.0));
}

/// A country's name is the name of a land, above every city's size, a
/// capital's included, and set apart from them: spaced capitals in the thin
/// face. A region's name is the same manner, under the country's and still
/// above a city's.
#[test]
fn a_countrys_name_stands_above_every_citys() {
    let text = |style: FeatureStyle| style.point_label_style().unwrap().text;
    let country = text(place("country", "country", Some(10), vec![], 10));
    let region = text(place("region", "state", Some(10), vec![], 10));
    let capital = text(place("locality", "city", Some(10), vec![("capital", s("yes"))], 10));
    let city = text(city(Some(10)));
    assert!(country.size_points > capital.size_points);
    assert!(capital.size_points > city.size_points);
    assert!(region.size_points >= city.size_points);
    assert!(region.size_points < country.size_points, "and under its country's");
    for land in [country, region] {
        assert!(land.uppercased);
        assert!(land.letter_spacing_em > 0.0, "Spaced out over the land it names");
        assert_eq!(land.weight, LabelFontWeight::Thin);
        assert!(land.opacity < 1.0, "Lettering the land shows through");
        assert!(land.opacity > 0.5);
    }
    assert!(!city.uppercased);
    assert_eq!(city.letter_spacing_em, 0.0);
    assert!(city.opacity > country.opacity, "A city's name reads more solid than a land's");
    assert_eq!(capital.weight, LabelFontWeight::Bold);
}

/// The name of a land goes away where the land no longer fits the view, at
/// the theme's zoom, and a city's name never does.
#[test]
fn the_name_of_a_land_leaves_the_street_zooms() {
    let max = |style: FeatureStyle| style.point_label_style().unwrap().max_camera_zoom;
    assert_eq!(max(place("country", "country", None, vec![], 10)), 8.0);
    assert_eq!(max(place("region", "state", None, vec![], 10)), 10.0);
    assert_eq!(max(city(None)), f32::INFINITY);
    let deeper = ProtomapsBasemapDefaultMapStyle::new(ProtomapsBasemapTheme::default().with_label_visibility(|visibility| {
        visibility.country_maximum_zoom = 6;
        visibility.region_maximum_zoom = 12;
    }));
    let props: Props = vec![("name", s("Test")), ("kind", s("country")), ("kind_detail", s("country"))];
    assert_eq!(max(style_of(&deeper, "places", &props, 10, FeatureGeometry::Point)), 6.0);
    let props: Props = vec![("name", s("Test")), ("kind", s("region")), ("kind_detail", s("state"))];
    assert_eq!(max(style_of(&deeper, "places", &props, 10, FeatureGeometry::Point)), 12.0);
}

#[test]
fn every_place_the_tile_ships_is_labelled_at_every_zoom() {
    for zoom in [0, 2, 3, 4, 8, 12] {
        assert!(is_label(&place("country", "country", None, vec![], zoom)), "z{zoom}");
        assert!(is_label(&place("locality", "city", Some(17), vec![], zoom)));
        assert!(is_label(&place("region", "state", None, vec![], zoom)), "z{zoom}");
        assert!(is_label(&place("locality", "town", None, vec![("min_zoom", i(7))], zoom)), "z{zoom}");
        assert!(is_label(&place("neighbourhood", "neighbourhood", None, vec![], zoom)), "z{zoom}");
    }
}

#[test]
fn every_water_name_the_tile_ships_is_labelled() {
    for zoom in [0, 3, 4, 5, 12] {
        assert!(is_label(&point("water", &[("kind", s("ocean")), ("name", s("Sea"))], zoom)), "z{zoom}");
        assert!(is_label(&point("water", &[("kind", s("lake")), ("name", s("Lake"))], zoom)), "z{zoom}");
    }
}

#[test]
fn every_poi_kind_becomes_a_label() {
    for kind in ["bicycle_parking", "waste_basket", "gate", "entrance", "bench", "bus_stop", "parking", "office"] {
        assert!(is_label(&poi(kind, Some(13), 14)), "Kind {kind} is labelled");
    }
}

#[test]
fn a_poi_waits_for_its_stated_zoom() {
    let restaurant = poi("restaurant", Some(15), 14);
    assert!(is_label(&restaurant));
    assert_eq!(label_min_camera_zoom(&restaurant), 15.0, "the basemap's rank is the zoom it shows from");
    for zoom in [8, 12, 13] {
        assert!(is_label(&poi("restaurant", Some(12), zoom)), "z{zoom}");
        assert!(is_label(&poi("peak", Some(7), zoom)), "z{zoom}");
    }
}

#[test]
fn a_poi_without_a_stated_zoom_shows_with_its_tile() {
    assert_eq!(label_min_camera_zoom(&poi("restaurant", None, 14)), 0.0);
}

#[test]
fn an_address_point_is_labelled_with_its_house_number() {
    let props: Props = vec![("kind", s("address")), ("addr_housenumber", s("7/2"))];
    assert_eq!(facts("buildings", &props, 15, FeatureGeometry::Point).label().unwrap().name.as_deref(), Some("7/2"));
    assert!(is_label(&point("buildings", &props, 15)));
    assert!(!is_label(&point("buildings", &[("kind", s("address"))], 15)));
}

/// The palette states the design curve and may dip under the floor. What
/// must never reach the screen under it is a resolved style. This walks the
/// classes whose curve is lowest, which are the ones the floor exists for.
#[test]
fn no_resolved_label_style_is_below_the_readable_floor() {
    let cases: [(&str, &str, Props); 6] = [
        ("poi", "pois", vec![("kind", s("restaurant")), ("name", s("Cafe"))]),
        ("water", "water", vec![("kind", s("lake")), ("name", s("Lake"))]),
        ("ocean", "water", vec![("kind", s("ocean")), ("name", s("Ocean"))]),
        ("village", "places", vec![("kind", s("locality")), ("kind_detail", s("village")), ("name", s("Village"))]),
        ("city", "places", vec![("kind", s("locality")), ("kind_detail", s("city")), ("name", s("City"))]),
        ("region", "places", vec![("kind", s("region")), ("kind_detail", s("state")), ("name", s("Region"))]),
    ];
    for (name, layer, props) in cases {
        let style = point(layer, &props, 15);
        let text = style.point_label_style().unwrap_or_else(|| panic!("{name} no longer resolves to a label style")).text;
        assert!(text.size_points >= LabelTypeScale::MINIMUM_SIZE_POINTS, "{name} resolves below the readable floor");
        // The water names are painted on the water with no halo, so they
        // read as part of its surface.
        if layer != "water" {
            assert!(text.halo_em > 0.0, "{name} lost its halo");
        }
    }
}

/// The floor belongs at the end of the pipeline. Applied to the base of a
/// class instead, it would push everything derived from that base up too:
/// an ocean label is the water appearance a few points larger, and lifting
/// water first would make the ocean larger than the design asks for.
#[test]
fn the_floor_does_not_push_derived_classes_up() {
    let size = |kind: &'static str| point("water", &[("kind", s(kind)), ("name", s("Water"))], 6).point_label_style().unwrap().text.size_points;
    assert_close(size("lake"), LabelTypeScale::MINIMUM_SIZE_POINTS, 0.0001);
    assert_close(size("ocean"), 12.5, 0.0001);
}

#[test]
fn type_scale_floor_raises_undersized_labels_only() {
    assert_eq!(LabelTypeScale::clamped(6.0), LabelTypeScale::MINIMUM_SIZE_POINTS);
    assert_eq!(LabelTypeScale::clamped(20.0), 20.0);
    assert_eq!(LabelTypeScale::MINIMUM_SIZE_POINTS, 11.0);
}

/// A tile is drawn for the camera zooms from its own up to the next: a tile
/// whose whole range lies outside the name's zooms never bakes it.
#[test]
fn a_tile_no_zoom_of_which_shows_the_name_does_not_carry_it() {
    let placement = SurfaceLabelPlacement { minimum_zoom: 2.5, maximum_zoom: 5.5, letter_spacing_em: 0.15, ..SurfaceLabelPlacement::new(3.0) };
    assert!(!placement.is_visible_on_tile_zoom(1));
    assert!(placement.is_visible_on_tile_zoom(2));
    assert!(placement.is_visible_on_tile_zoom(5));
    assert!(!placement.is_visible_on_tile_zoom(6));
    assert!(!placement.is_visible_at_zoom(2.4));
    assert!(placement.is_visible_at_zoom(2.5));
    assert!(placement.is_visible_at_zoom(5.4));
    assert!(!placement.is_visible_at_zoom(5.5));
}
