// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The Swift `ImmersiveMapGeoJSONTests` and the GeoJSON case of
//! `LayerSelectionSnapshotTests`, plus the order of members and the error paths.

use layer_mesh::content::{NO_STYLE, PolygonStyle, SourceContent, StyleRecord};
use layer_mesh::geojson::{GeoJsonError, parse, parse_color};

fn layer(json: &str) -> Result<SourceContent, GeoJsonError> {
    parse(json.as_bytes())
}

/// Every geometry type lands as its kind of shape, the positions read as
/// longitude then latitude, an altitude ignored, a null geometry skipped.
#[test]
fn a_feature_collection_reads_every_geometry_type() {
    let json = r#"
    {"type": "FeatureCollection", "features": [
      {"type": "Feature", "properties": {}, "geometry": {"type": "Point", "coordinates": [37.6, 55.7, 120]}},
      {"type": "Feature", "properties": null, "geometry": {"type": "MultiPoint", "coordinates": [[1, 2], [3, 4]]}},
      {"type": "Feature", "geometry": {"type": "LineString", "coordinates": [[0, 0], [1, 1]]}},
      {"type": "Feature", "geometry": {"type": "MultiLineString", "coordinates": [[[0, 0], [1, 1]], [[2, 2], [3, 3]]]}},
      {"type": "Feature", "geometry": {"type": "Polygon", "coordinates": [
          [[0, 0], [10, 0], [10, 10], [0, 10], [0, 0]],
          [[2, 2], [4, 2], [4, 4], [2, 2]]]}},
      {"type": "Feature", "geometry": {"type": "MultiPolygon", "coordinates": [
          [[[20, 20], [21, 20], [21, 21], [20, 20]]],
          [[[30, 30], [31, 30], [31, 31], [30, 30]]]]}},
      {"type": "Feature", "geometry": null},
      {"type": "Feature", "geometry": {"type": "GeometryCollection", "geometries": [
          {"type": "Point", "coordinates": [5, 6]}]}}
    ]}"#;
    let content = layer(json).unwrap();
    assert_eq!(content.point_count(), 4);
    assert_eq!(content.point_coordinate(0), [37.6, 55.7]);
    assert_eq!(content.polyline_count(), 3);
    assert_eq!(content.polygon_count(), 3);
    assert_eq!(content.polygons[1], 2, "The first zone has its outline and one hole");
    assert_eq!(content.ring_coordinates(content.polygons[0] as usize).len(), 5);
    assert_eq!(content.polygons[2], NO_STYLE, "A feature that states no style takes the layer's");
    assert!(content.styles.is_empty());
}

/// A bare geometry is a document of its own.
#[test]
fn a_bare_geometry_is_a_document() {
    let content = layer(r#"{"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 0]]]}"#).unwrap();
    assert_eq!(content.polygon_count(), 1);
    assert!(content.ids[0].is_nan(), "A shape outside a feature has no id of its own");
}

/// The simplestyle properties lay the feature's colours and widths over the
/// layer's style, keeping the layer's for what they leave out.
#[test]
fn simplestyle_properties_style_a_feature_over_the_layer() {
    let base = PolygonStyle { fill_color: [0.0, 0.0, 1.0, 0.5], stroke_color: [0.0, 1.0, 0.0, 1.0], stroke_width_points: 3.0 };
    let json = r##"
    {"type": "Feature",
     "properties": {"fill": "#f00", "stroke-width": 5},
     "geometry": {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 0]]]}}"##;
    let content = layer(json).unwrap();
    let Some(StyleRecord::Simple(stated)) = content.record(content.polygons[2]) else { panic!("the feature's style is kept with its zone") };
    let style = stated.polygon_style(&base).unwrap();
    assert_eq!(style.fill_color, [1.0, 0.0, 0.0, 0.5], "The fill's colour is the feature's, its opacity the layer's");
    assert_eq!(style.stroke_color, base.stroke_color);
    assert_eq!(style.stroke_width_points, 5.0);
}

#[test]
fn colours_read_in_both_hex_forms() {
    assert_eq!(parse_color("#ff8000"), Some([1.0, 128.0 / 255.0, 0.0]));
    assert_eq!(parse_color("0f0"), Some([0.0, 1.0, 0.0]));
    assert_eq!(parse_color(" #00f "), Some([0.0, 0.0, 1.0]));
    assert_eq!(parse_color("red"), None);
    // A number is not a colour: the marker colour of 42 is left out.
    let content = layer(r#"{"type": "Feature", "properties": {"marker-color": 42}, "geometry": {"type": "Point", "coordinates": [0, 0]}}"#).unwrap();
    assert_eq!(content.points[1], NO_STYLE);
}

#[test]
fn a_non_geojson_document_throws() {
    assert_eq!(layer("not json"), Err(GeoJsonError::InvalidJson));
    assert_eq!(layer(r#"{"type": "Circle"}"#), Err(GeoJsonError::UnknownType("Circle".into())));
    assert_eq!(layer(r#"{"type": "Point", "coordinates": [1]}"#), Err(GeoJsonError::InvalidCoordinates("coordinates".into())));
}

/// A feature's parts share its id, and a feature with no numeric id goes by
/// its place in the document.
#[test]
fn a_geojson_features_shapes_go_by_its_numeric_id_or_its_place() {
    let json = r#"
    {"type": "FeatureCollection", "features": [
      {"type": "Feature", "id": 12, "geometry": {"type": "MultiPoint", "coordinates": [[1, 2], [3, 4]]}},
      {"type": "Feature", "id": "station-a", "geometry": {"type": "Point", "coordinates": [5, 6]}},
      {"type": "Feature", "geometry": {"type": "Point", "coordinates": [7, 8]}}
    ]}"#;
    assert_eq!(layer(json).unwrap().ids, vec![12.0, 12.0, 1.0, 2.0]);
}

/// The members of an object come in any order.
#[test]
fn members_read_in_any_order() {
    let json = r##"
    {"features": [
      {"geometry": {"coordinates": [[1, 2], [3, 4]], "type": "LineString"}, "properties": {"stroke": "#00ff00"}, "id": 7, "type": "Feature"},
      {"type": "Feature", "geometry": {"type": "Point", "coordinates": [5, 6]}, "id": 3.0, "properties": {"marker-size": "large"}}
    ], "type": "FeatureCollection"}"##;
    let content = layer(json).unwrap();
    assert_eq!(content.polyline_count(), 1);
    assert_eq!(content.point_count(), 1);
    assert_eq!(content.ids, vec![7.0, 3.0]);
    let Some(StyleRecord::Simple(line)) = content.record(content.polylines[2]) else { panic!("the line keeps its stroke") };
    assert_eq!(line.stroke, Some([0.0, 1.0, 0.0]));
    let Some(StyleRecord::Simple(point)) = content.record(content.points[1]) else { panic!("the point keeps its size") };
    assert_eq!(point.marker_radius, Some(8.0));
}

/// The part of a feature's style a shape does not read is not kept with it.
#[test]
fn a_shape_keeps_only_the_part_of_the_style_it_reads() {
    let json = r##"{"type": "Feature", "properties": {"marker-color": "#fff", "fill-opacity": "0.4"},
      "geometry": {"type": "GeometryCollection", "geometries": [
        {"type": "Point", "coordinates": [0, 0]}, {"type": "LineString", "coordinates": [[0, 0], [1, 1]]},
        {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1]]]}]}}"##;
    let content = layer(json).unwrap();
    assert_ne!(content.points[1], NO_STYLE);
    assert_eq!(content.polylines[2], NO_STYLE, "A line reads no fill and no marker");
    let Some(StyleRecord::Simple(zone)) = content.record(content.polygons[2]) else { panic!("the zone reads the fill") };
    assert_eq!(zone.fill_opacity, Some(0.4), "A numeric string is a number");
}

#[test]
fn errors_name_the_path() {
    assert_eq!(
        layer(r#"{"type": "FeatureCollection", "features": [{"type": "Feature", "geometry": {"type": "LineString", "coordinates": [[0, 0], [1]]}}]}"#),
        Err(GeoJsonError::InvalidCoordinates("features[0].geometry.coordinates[1]".into()))
    );
    assert_eq!(
        layer(r#"{"type": "MultiPolygon", "coordinates": [[[[0, 0], [1, 0], [1, 1]]], [[[0, 0], "x"]]]}"#),
        Err(GeoJsonError::InvalidCoordinates("coordinates[1][0][1]".into()))
    );
    assert_eq!(layer(r#"{"type": "Polygon", "coordinates": []}"#), Err(GeoJsonError::InvalidCoordinates("coordinates".into())));
    assert_eq!(layer(r#"{"type": "FeatureCollection"}"#), Err(GeoJsonError::InvalidCoordinates("features".into())));
    assert_eq!(layer(r#"{"type": "FeatureCollection", "features": [3]}"#), Err(GeoJsonError::UnknownType("features[0].".into())));
    assert_eq!(layer(r#"{"coordinates": [0, 0]}"#), Err(GeoJsonError::UnknownType("root".into())));
    assert_eq!(layer("42"), Err(GeoJsonError::UnknownType("root".into())));
    assert_eq!(layer(r#"{"type": "Point"}"#), Err(GeoJsonError::InvalidCoordinates("coordinates".into())));
    // A GeoJSON fault in a document that is not JSON further on is the JSON's.
    assert_eq!(layer(r#"{"type": "Circle", "x": "#), Err(GeoJsonError::InvalidJson));
}

#[test]
fn a_null_geometry_counts_as_a_feature() {
    let json = r#"{"type": "FeatureCollection", "features": [
      {"type": "Feature", "geometry": null},
      {"type": "Feature", "geometry": {"type": "Point", "coordinates": [true, 2]}}]}"#;
    let content = layer(json).unwrap();
    assert_eq!(content.ids, vec![1.0]);
    assert_eq!(content.point_coordinate(0), [1.0, 2.0], "A boolean is a number, as NSNumber reads it");
}
