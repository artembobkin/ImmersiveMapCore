// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The Swift `LayerMeshBuilderTests`, `LayerChunkCullingTests.testAChunkKnowsItsBoundsAndItsReach`
//! and `LayerMeshFileTests`.

use layer_mesh::content::{LayerStyles, NO_STYLE, PointStyle, PolygonStyle, PolylineStyle, ShapesBuilder, SourceContent, StyleRecord};
use layer_mesh::mesh::{self, MAXIMUM_EDGE_LENGTH, MeshData, ROUND_END, ROUND_START, TILE_EXTENT};
use layer_mesh::mesh_file;

/// A ring as (longitude, latitude) from (latitude, longitude) pairs, as the Swift tests write them.
fn ring(coordinates: &[(f64, f64)]) -> Vec<[f64; 2]> {
    coordinates.iter().map(|&(latitude, longitude)| [longitude, latitude]).collect()
}

fn build(content: &SourceContent, styles: &LayerStyles) -> MeshData {
    mesh::build(&content.view(), styles)
}

fn zones(rings: &[Vec<[f64; 2]>]) -> SourceContent {
    let mut shapes = ShapesBuilder::new();
    for exterior in rings {
        shapes.push_polygon(&[exterior], None, NO_STYLE);
    }
    shapes.finish()
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// Every triangle of a zone is counter-clockwise in render space, whichever way its ring runs.
#[test]
fn a_zones_triangles_are_counter_clockwise_whichever_way_its_ring_runs() {
    let forward = ring(&[(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)]);
    let mut backward = forward.clone();
    backward.reverse();
    for exterior in [forward, backward] {
        let data = build(&zones(&[exterior]), &LayerStyles::default());
        assert!(!data.fill_indices.is_empty());
        for t in data.fill_indices.as_chunks::<3>().0 {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| data.fill_vertices[i as usize].position);
            let cross = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            assert!(cross > 0.0);
        }
    }
}

/// A zone wider than the longest edge is cut into triangles that follow the
/// sphere, and its outline into short segments.
#[test]
fn a_large_zone_is_cut_into_short_edges() {
    let data = build(&zones(&[ring(&[(-20.0, -20.0), (-20.0, 20.0), (20.0, 20.0), (20.0, -20.0)])]), &LayerStyles::default());
    let chunk = data.chunks[0];
    let units_per_world = (1 << chunk.z) as f32 * TILE_EXTENT as f32;
    let longest = MAXIMUM_EDGE_LENGTH as f32 * units_per_world * 1.0001;
    for t in data.fill_indices.as_chunks::<3>().0 {
        let corners = [t[0], t[1], t[2]].map(|i| data.fill_vertices[i as usize].position);
        for i in 0..3 {
            assert!(distance(corners[i], corners[(i + 1) % 3]) <= longest);
        }
    }
    for segment in &data.line_segments {
        assert!(distance(segment.start, segment.end) <= longest);
    }
    // Round at the four corners alone, flat where a side was only cut. A join has no disc.
    assert_eq!(chunk.join_count, 0);
    let rounding: Vec<u8> = data.line_segments.iter().map(|s| s.color[3]).collect();
    assert_eq!(chunk.line_count as usize, rounding.len());
    assert!(rounding.len() > 4, "Each side is cut into pieces");
    assert_eq!(rounding.iter().filter(|&&r| r == ROUND_START).count(), 4);
    assert_eq!(rounding.iter().filter(|&&r| r == ROUND_END).count(), 4);
    assert_eq!(rounding.iter().filter(|&&r| r == 0).count(), rounding.len() - 8, "A piece in the middle of a side is flat at both ends");
}

/// A line is always solid: a translucent colour draws it opaque, and only a
/// zero alpha leaves it out. The fill keeps its alpha.
#[test]
fn a_line_is_solid_whatever_its_colours_alpha() {
    let square = ring(&[(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)]);
    let mut shapes = ShapesBuilder::new();
    shapes.push_polygon(&[&square], None, NO_STYLE);
    shapes.push_polyline(&square, None, NO_STYLE);
    let content = shapes.finish();
    let mesh = |line_alpha: f32| {
        let styles = LayerStyles {
            polygon: PolygonStyle { fill_color: [1.0, 0.0, 0.0, 0.5], stroke_color: [0.0, 1.0, 0.0, line_alpha], ..PolygonStyle::default() },
            polyline: PolylineStyle { color: [0.0, 0.0, 1.0, line_alpha], ..PolylineStyle::default() },
            point: PointStyle::default(),
        };
        build(&content, &styles)
    };
    let translucent = mesh(0.25);
    assert!(!translucent.line_segments.is_empty());
    assert_eq!(translucent.line_segments, mesh(1.0).line_segments, "The alpha changes nothing of a line");
    assert!(translucent.line_segments.iter().all(|s| s.color[3] == ROUND_START | ROUND_END));
    assert!(translucent.discs.is_empty(), "A join is the round ends of its segments, not a disc");
    assert!(translucent.fill_vertices.iter().all(|v| v.color[3] == 128), "A fill may be translucent");

    let hidden = mesh(0.0);
    assert!(hidden.line_segments.is_empty());
    assert!(hidden.discs.is_empty());
}

/// A zone across the 180th meridian stays whole: its fill spans 20 degrees, not 340.
#[test]
fn a_zone_across_the_antimeridian_stays_whole() {
    let data = build(&zones(&[ring(&[(0.0, 170.0), (0.0, -170.0), (10.0, -170.0), (10.0, 170.0)])]), &LayerStyles::default());
    let chunk = data.chunks[0];
    let units_per_world = (1 << chunk.z) as f32 * TILE_EXTENT as f32;
    let xs: Vec<f32> = data.fill_vertices.iter().map(|v| v.position[0]).collect();
    let span = (xs.iter().cloned().fold(f32::MIN, f32::max) - xs.iter().cloned().fold(f32::MAX, f32::min)) / units_per_world;
    assert!((span - 20.0 / 360.0).abs() < 1e-4);
}

/// Points are grouped by their chunk, and a point drawn in nothing is left out.
#[test]
fn points_land_in_their_chunks() {
    let mut shapes = ShapesBuilder::new();
    for (latitude, longitude) in [(55.75, 37.6), (55.76, 37.61), (-33.9, 151.2)] {
        shapes.push_point([longitude, latitude], None, NO_STYLE);
    }
    let invisible = shapes.push_style(&StyleRecord::Point(PointStyle { radius_points: 0.0, stroke_width_points: 0.0, ..PointStyle::default() }));
    shapes.push_point([0.0, 0.0], None, invisible);
    let data = build(&shapes.finish(), &LayerStyles::default());
    assert_eq!(data.chunks.len(), 2);
    assert_eq!(data.chunks.iter().map(|c| c.point_count).sum::<u32>(), 3);
    assert_eq!(data.discs.len(), 3);
    for chunk in &data.chunks {
        for disc in &data.discs[chunk.point_start as usize..(chunk.point_start + chunk.point_count) as usize] {
            assert!((0.0..=4096.0).contains(&disc.position[0]));
            assert!((0.0..=4096.0).contains(&disc.position[1]));
        }
    }
}

/// A line of one vertex is a disc with the lines, before the points.
#[test]
fn a_line_of_one_vertex_is_a_join_disc() {
    let mut shapes = ShapesBuilder::new();
    shapes.push_point([37.6, 55.75], None, NO_STYLE);
    shapes.push_polyline(&[[37.6, 55.75]], None, NO_STYLE);
    let data = build(&shapes.finish(), &LayerStyles::default());
    let chunk = data.chunks[0];
    assert_eq!((chunk.join_start, chunk.join_count, chunk.point_start, chunk.point_count), (0, 1, 1, 1));
    assert_eq!(data.discs[0].radius_points, 1.5);
    assert_eq!(data.discs[0].stroke_width_points, 0.0);
}

/// The chunk's box holds every position, and its reach is the widest of
/// what its shapes add on screen.
#[test]
fn a_chunk_knows_its_bounds_and_its_reach() {
    let mut shapes = ShapesBuilder::new();
    shapes.push_polygon(&[&ring(&[(55.7, 37.5), (55.7, 37.7), (55.8, 37.7), (55.8, 37.5)])], None, NO_STYLE);
    shapes.push_point([37.6, 55.75], None, NO_STYLE);
    let styles = LayerStyles {
        polygon: PolygonStyle { stroke_width_points: 6.0, ..PolygonStyle::default() },
        point: PointStyle { radius_points: 10.0, stroke_width_points: 2.0, ..PointStyle::default() },
        ..LayerStyles::default()
    };
    let data = build(&shapes.finish(), &styles);
    assert_eq!(data.chunks.len(), 1);
    let chunk = data.chunks[0];
    let positions = data
        .fill_vertices
        .iter()
        .map(|v| v.position)
        .chain(data.line_segments.iter().flat_map(|s| [s.start, s.end]))
        .chain(data.discs.iter().map(|d| d.position));
    for p in positions {
        assert!(p[0] >= chunk.bounds_minimum[0] && p[0] <= chunk.bounds_maximum[0]);
        assert!(p[1] >= chunk.bounds_minimum[1] && p[1] <= chunk.bounds_maximum[1]);
    }
    assert!(chunk.bounds_minimum[0] < chunk.bounds_maximum[0]);
    assert_eq!(chunk.maximum_reach_points, 12.0);
}

/// A shape's own style wins over the layer's.
#[test]
fn a_shapes_own_style_wins() {
    let mut shapes = ShapesBuilder::new();
    let red = shapes.push_style(&StyleRecord::Polyline(PolylineStyle { color: [1.0, 0.0, 0.0, 1.0], width_points: 9.0 }));
    shapes.push_polyline(&[[0.0, 0.0], [0.1, 0.1]], None, red);
    let data = build(&shapes.finish(), &LayerStyles::default());
    assert_eq!(data.line_segments[0].width_points, 9.0);
    assert_eq!(&data.line_segments[0].color[0..3], &[255, 0, 0]);
}

// MARK: - The file

fn sample_mesh() -> MeshData {
    let mut shapes = ShapesBuilder::new();
    shapes.push_polygon(&[&ring(&[(55.7, 37.5), (55.7, 37.7), (55.8, 37.7), (55.8, 37.5)])], None, NO_STYLE);
    shapes.push_point([151.2, -33.9], None, NO_STYLE);
    build(&shapes.finish(), &LayerStyles::default())
}

#[test]
fn a_mesh_comes_back_from_its_file_as_it_was() {
    let mesh = sample_mesh();
    let file = mesh_file::encode(&mesh);
    let prefix = mesh_file::prefix_size(&file[..mesh_file::HEADER_SIZE]).unwrap();
    let contents = mesh_file::decode(&file[..prefix], file.len()).unwrap();
    assert_eq!(contents.chunks, mesh.chunks);
    assert_eq!(contents.chunks.len(), 2);
    assert_eq!(&file[contents.blocks[0].clone()], &mesh_file::fill_vertex_bytes(&mesh.fill_vertices)[..]);
    assert_eq!(&file[contents.blocks[1].clone()], &mesh_file::fill_index_bytes(&mesh.fill_indices)[..]);
    assert_eq!(&file[contents.blocks[2].clone()], &mesh_file::line_segment_bytes(&mesh.line_segments)[..]);
    assert!(!mesh.line_segments.is_empty());
    assert_eq!(&file[contents.blocks[3].clone()], &mesh_file::disc_bytes(&mesh.discs)[..]);
    for block in &contents.blocks {
        assert_eq!(block.start % 16, 0);
    }
}

#[test]
fn an_empty_mesh_is_a_file_of_its_header_alone() {
    let file = mesh_file::encode(&MeshData::default());
    let contents = mesh_file::decode(&file, file.len()).unwrap();
    assert!(contents.chunks.is_empty());
    assert!(contents.blocks.iter().all(|b| b.is_empty()));
}

#[test]
fn a_damaged_file_is_not_a_mesh() {
    let file = mesh_file::encode(&sample_mesh());
    let mut other_version = file.clone();
    other_version[4] = 0xFF;
    assert!(mesh_file::decode(&other_version, other_version.len()).is_none());
    assert!(mesh_file::prefix_size(&other_version).is_none());
    assert!(mesh_file::decode(&file, file.len() - 8).is_none(), "A file cut short");
    assert!(mesh_file::decode(&file[..20], file.len()).is_none());
    assert!(mesh_file::decode(b"not a mesh at all", 17).is_none());
}

/// The GPU structs' sizes, as the pipelines read them.
#[test]
fn the_vertex_layouts_match_the_shaders() {
    assert_eq!(mesh_file::fill_vertex_bytes(&sample_mesh().fill_vertices[..1]).len(), 12);
    assert_eq!(mesh_file::LINE_SEGMENT_STRIDE, 24);
    assert_eq!(mesh_file::DISC_STRIDE, 24);
    assert_eq!(mesh_file::CHUNK_STRIDE, 64);
    assert_eq!(mesh_file::HEADER_SIZE, 76);
}
