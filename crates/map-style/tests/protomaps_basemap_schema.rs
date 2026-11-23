// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Ported from `ProtomapsBasemapSchemaTests` and
//! `ImmersiveMapBuildingExtrusionTests`: what a feature is, before any
//! style says how it draws.

mod support;

use map_style::schema::default::ProtomapsBasemapSchema;
use map_style::schema::{BuildingExtrusion, FeatureFacts, FeatureGeometry, FeatureProperties, NamesByLanguage, OsmElement, RoadStructure, RouteFacts, TileSchema};
use map_style::style::default::ProtomapsBasemapDefaultMapStyle;
use map_style::style::RoadLevel;
use mvt::Value;
use support::*;

fn read<'a>(layer: &'a str, props: &[(&'a str, Value<'a>)], geometry: FeatureGeometry) -> FeatureFacts<'a> {
    facts(layer, props, 15, geometry)
}

fn building(props: &[(&'static str, Value<'static>)]) -> BuildingExtrusion {
    *read("buildings", props, FeatureGeometry::Polygon).building().unwrap()
}

#[test]
fn a_road_line_carries_its_structure_layer_and_name() {
    let props = [
        ("kind", s("major_road")),
        ("kind_detail", s("primary")),
        ("is_tunnel", b(true)),
        ("layer", i(-1)),
        ("name", s("Mokhovaya")),
    ];
    let facts = read("roads", &props, FeatureGeometry::Line);
    let road = facts.road().unwrap();
    assert_eq!(road.structure, RoadStructure::Tunnel);
    assert_eq!(road.layer, -1);
    assert_eq!(road.name, "Mokhovaya");
    assert!(road.stitching_key.is_some(), "A line carries the key it stitches on");
    let unnamed = [("kind", s("major_road"))];
    assert!(read("roads", &unnamed, FeatureGeometry::Line).road().unwrap().stitching_key.is_none());
    let bridge = [("is_bridge", b(true))];
    assert_eq!(read("roads", &bridge, FeatureGeometry::Line).road().unwrap().structure, RoadStructure::Bridge);
    assert!(read("roads", &unnamed, FeatureGeometry::Polygon).road().is_none(), "A polygon under the road layer is no road");
}

#[test]
fn a_negative_layer_alone_is_not_a_tunnel() {
    let down = [("layer", i(-1))];
    let facts = read("roads", &down, FeatureGeometry::Line);
    let road = facts.road().unwrap();
    assert_eq!(road.structure, RoadStructure::Ground);
    assert_eq!(ProtomapsBasemapDefaultMapStyle::road_level(road), RoadLevel::Tunnel);
    let up = [("layer", i(1))];
    let facts = read("roads", &up, FeatureGeometry::Line);
    let ramp = facts.road().unwrap();
    assert_eq!(ramp.structure, RoadStructure::Ground);
    assert_eq!(ProtomapsBasemapDefaultMapStyle::road_level(ramp), RoadLevel::Bridge);
}

#[test]
fn the_stitching_key_reads_what_changes_the_drawing_and_not_the_ref() {
    let key = |extra: &[(&'static str, Value<'static>)]| {
        let mut props: Props = vec![("kind", s("major_road")), ("kind_detail", s("primary")), ("name", s("Tverskaya"))];
        props.extend_from_slice(extra);
        read("roads", &props, FeatureGeometry::Line).road().unwrap().stitching_key.clone()
    };
    assert_eq!(key(&[("ref", s("M10"))]), key(&[("ref", s("A104"))]), "a route reference is not a drawing change");
    assert_ne!(key(&[("is_link", b(true))]), key(&[]), "a ramp draws apart from its parent");
    assert_ne!(key(&[("is_bridge", b(true))]), key(&[]));
    assert_ne!(key(&[("oneway", b(true))]), key(&[]));
}

#[test]
fn a_road_with_only_a_route_number_is_named_by_it_but_carries_no_label() {
    let props = [("kind", s("highway")), ("ref", s("M10;E105"))];
    let facts = read("roads", &props, FeatureGeometry::Line);
    let road = facts.road().unwrap();
    assert_eq!(road.name, "M10 / E105", "the number is the road's identity for counting junctions");
    assert!(road.label.is_none(), "a number is drawn on its sign, not laid along the road");
    assert!(road.stitching_key.is_some());
    assert_eq!(road.routes, vec![RouteFacts::unnetworked("M10"), RouteFacts::unnetworked("E105")]);
}

#[test]
fn a_named_road_with_a_route_number_keeps_the_number_out_of_its_label() {
    let props = [("kind", s("major_road")), ("ref", s("A104")), ("name", s("Dmitrovskoye")), ("name:en", s("Dmitrov Highway"))];
    let facts = read("roads", &props, FeatureGeometry::Line);
    let road = facts.road().unwrap();
    assert_eq!(road.name, "Dmitrovskoye");
    let label = road.label.as_ref().unwrap();
    assert_eq!(label.name.as_deref(), Some("Dmitrovskoye"));
    assert_eq!(label.names_by_language.get("en"), Some("Dmitrov Highway"));
    assert_eq!(road.routes, vec![RouteFacts::unnetworked("A104")]);
}

#[test]
fn a_roads_routes_are_its_network_and_sign_text_pairs_in_order() {
    // The M-9 west of Moscow as the basemap ships it: a European route
    // first, then the national one. The railway network in the last slot has
    // no sign text and is no route of the road.
    let props = [
        ("kind", s("highway")),
        ("ref", s("М-9")),
        ("network", s("e-road")),
        ("shield_text", s("E22")),
        ("network_1", s("e-road")),
        ("shield_text_1", s("E22")),
        ("network_2", s("ru:national")),
        ("shield_text_2", s("М-9")),
        ("network_3", s("Московская железная дорога")),
    ];
    let facts = read("roads", &props, FeatureGeometry::Line);
    assert_eq!(facts.road().unwrap().routes, vec![RouteFacts::new("e-road", "E22"), RouteFacts::new("ru:national", "М-9")]);
}

#[test]
fn a_building_carries_its_heights_and_kind() {
    let tall = building(&[("kind", s("building")), ("height", Value::Double(24.0)), ("min_height", Value::Double(3.0))]);
    assert_eq!(tall.height_metres, Some(24.0));
    assert_eq!(tall.base_height_metres, Some(3.0));
    assert!(!tall.is_part);
    assert_eq!(tall.building_identity, None, "the basemap names no building identity");
    assert!(building(&[("kind", s("building_part"))]).is_part);
    assert!(building(&[("layer", i(-1))]).is_hidden, "an underground structure is not raised");
    assert!(building(&[("kind", s("building_part")), ("kind_detail", s("no"))]).is_hidden, "building:part=no is never raised");
    assert!(!building(&[("kind", s("building_part")), ("kind_detail", s("yes"))]).is_hidden);
    assert!(!building(&[("kind", s("building")), ("kind_detail", s("no"))]).is_hidden, "only a part says no");
    let address = [("kind", s("address")), ("addr_housenumber", s("12b"))];
    assert!(read("buildings", &address, FeatureGeometry::Point).building().is_none());
    assert!(read("buildings", &[], FeatureGeometry::Polygon).road().is_none());
}

#[test]
fn heights_come_from_the_basemap_in_metres() {
    assert_eq!(building(&[("height", Value::Double(12.0))]).height_metres, Some(12.0));
    assert_eq!(building(&[("height", s("30 ft"))]).height_metres, Some(30.0 * 0.3048));
    let podium = building(&[("height", Value::Double(40.0)), ("min_height", Value::Double(12.0))]);
    assert_eq!(podium.height_metres, Some(40.0));
    assert_eq!(podium.base_height_metres, Some(12.0));
    assert_eq!(building(&[]).height_metres, None, "the basemap states the height, or there is none");
    assert!(!building(&[]).is_hidden, "absent flags mean extruded");
}

#[test]
fn a_water_point_names_a_water_body_and_the_fills_are_nothing() {
    let ocean = [("kind", s("ocean")), ("name", s("Atlantic Ocean"))];
    assert!(read("water", &ocean, FeatureGeometry::Point).label().unwrap().names_water_body);
    let lake = [("kind", s("lake")), ("name", s("Baikal"))];
    assert!(read("water", &lake, FeatureGeometry::Polygon).label().is_none(), "the fill of a named lake is called nothing");
    let river = [("kind", s("river")), ("name", s("Volga"))];
    assert!(read("water", &river, FeatureGeometry::Line).label().is_none());
    for layer in ["landuse", "earth", "boundaries", "landcover", "transit"] {
        let named = [("kind", s("residential")), ("name", s("Named"))];
        let fill = read(layer, &named, FeatureGeometry::Polygon);
        assert!(fill.road().is_none(), "{layer}");
        assert!(fill.building().is_none(), "{layer}");
        assert!(fill.label().is_none(), "{layer} is called nothing, even with a name");
    }
}

#[test]
fn names_are_read_by_language() {
    let props = [
        ("name", s("Москва")),
        ("name:en", s("Moscow")),
        ("name:de", s("Moskau")),
        ("pgf:name:hi", s("shaped")),
        ("name2", s("Moskva")),
        ("script", s("Cyrillic")),
    ];
    let facts = read("places", &props, FeatureGeometry::Point);
    let place = facts.label().unwrap();
    assert_eq!(place.name.as_deref(), Some("Москва"));
    assert_eq!(place.names_by_language, [("en", "Moscow"), ("de", "Moskau")].into_iter().collect::<NamesByLanguage>());
    let cafe = [("kind", s("cafe")), ("name", s("Кафе"))];
    assert_eq!(read("pois", &cafe, FeatureGeometry::Point).label().unwrap().name.as_deref(), Some("Кафе"));
    assert!(read("pois", &[("kind", s("cafe"))], FeatureGeometry::Point).label().is_none(), "an unnamed POI is nothing");
    let street = [("kind", s("major_road")), ("name", s("Tverskaya"))];
    let road = read("roads", &street, FeatureGeometry::Line);
    assert_eq!(road.label().unwrap().name.as_deref(), Some("Tverskaya"), "A road carries its name for the label along it");
    assert!(road.road().is_some());
}

#[test]
fn a_regional_code_is_also_filed_under_its_language() {
    let props = [("name", s("北京市")), ("name:zh-Hans", s("北京")), ("name:zh-Hant", s("北京"))];
    let facts = read("places", &props, FeatureGeometry::Point);
    let place = facts.label().unwrap();
    assert_eq!(place.names_by_language.get("zh"), Some("北京"), "the map's language asks by the code before the dash");
    assert_eq!(place.names_by_language.get("zh-Hans"), Some("北京"));
    let stated = [("name:zh", s("plain")), ("name:zh-Hans", s("simplified"))];
    let facts = read("places", &stated, FeatureGeometry::Point);
    assert_eq!(facts.label().unwrap().names_by_language.get("zh"), Some("plain"), "a stated language entry is never overwritten");
}

#[test]
fn the_feature_id_is_the_osm_element() {
    let schema = ProtomapsBasemapSchema;
    assert_eq!(schema.tile_feature_id(OsmElement::Relation(3334755)), Some(3 << 44 | 3334755));
    assert_eq!(schema.tile_feature_id(OsmElement::Node(1)), Some(1 << 44 | 1));
    assert_eq!(schema.tile_feature_id(OsmElement::Way(1 << 44)), None);
}

#[test]
fn the_properties_read_loosely_typed_values() {
    let values = [
        ("id", s("42")),
        ("negative", i(-3)),
        ("flag", s("Yes")),
        ("zero", i(0)),
        ("height", s("12.5 m")),
        ("storeys", s("3;4")),
        ("text", Value::Bool(true)),
        ("number", Value::Double(2.0)),
    ];
    let properties = FeatureProperties::new(&values);
    assert_eq!(properties.unsigned_integer("id"), Some(42));
    assert_eq!(properties.unsigned_integer("negative"), None);
    assert_eq!(properties.bool("flag"), Some(true));
    assert_eq!(properties.bool("zero"), Some(false));
    assert_eq!(properties.metres("height"), Some(12.5));
    assert_eq!(properties.metres("storeys"), Some(3.0));
    assert_eq!(properties.text("text"), "1");
    assert_eq!(properties.text("number"), "2.0");
    assert_eq!(properties.text("absent"), "");
    assert_eq!(properties.string("negative"), Some(""), "a non-string value reads as the empty string");
    assert_eq!(properties.string("absent"), None);
}
