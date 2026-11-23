// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The model tile's writer, the one a baker makes a tile with: what it
//! writes is what the decoder reads, and a vertex lies in the bytes as the
//! format's reference says.

use std::collections::HashSet;

use model_tile::contents::Texture;
use model_tile::writer::{self, Model};
use model_tile::{ModelTile, ModelTileContents, ModelTileElement, ModelTileVertex, ReplacedBuilding};

use crate::fixture::SOLID_BLOCK;

/// A tile of one triangle in a box from (10, 20, 0) to (110, 220, 50).
fn make_tile() -> ModelTile {
    let minimum = [10.0, 20.0, 0.0];
    let maximum = [110.0, 220.0, 50.0];
    let mut vertices = Vec::new();
    for position in [minimum, [110.0, 20.0, 0.0], maximum] {
        let mut vertex = ModelTileVertex::new([0.0, 0.0, 1.0], 0, [0.5, 1.0]);
        vertex.place(position, minimum, maximum);
        vertex.append_to(&mut vertices);
    }
    // A 16 pixel texture in 8x8 blocks: levels of 16, 8, 4, 2 and 1
    // pixels, four blocks and then one a level.
    let levels = [4, 1, 1, 1, 1].map(|blocks| vec![SOLID_BLOCK.repeat(blocks)]).to_vec();
    let model = Model {
        id: "bolshoi".to_owned(),
        latitude: 55.7602,
        longitude: 37.6186,
        altitude_meters: 0.0,
        replaced_buildings: vec![
            ReplacedBuilding { element: ModelTileElement::Relation(3_334_755), map_tile_zoom: 15 },
            ReplacedBuilding { element: ModelTileElement::Way(84_653_688), map_tile_zoom: 14 },
        ],
        first_index: 0,
        index_count: 3,
        first_vertex: 0,
        vertex_count: 3,
        bounds_minimum: minimum,
        bounds_maximum: maximum,
    };
    ModelTile {
        zoom: 14,
        x: 9904,
        y: 5121,
        models: vec![model],
        bounds_minimum: minimum,
        bounds_maximum: maximum,
        vertices,
        indices: vec![0, 1, 2],
        texture: writer::Texture { width: 16, height: 16, block_edge: 8, layer_count: 1, levels },
    }
}

#[test]
fn a_written_tile_reads_back() {
    let tile = make_tile();
    let bytes = tile.serialized();
    let contents = ModelTileContents::decode(&bytes).unwrap();

    assert_eq!([contents.zoom, contents.x, contents.y], [14, 9904, 5121]);
    assert_eq!(contents.vertex_count, 3);
    assert_eq!(contents.index_count, 3);
    assert_eq!(contents.bounds_minimum, tile.bounds_minimum);
    assert_eq!(contents.bounds_maximum, tile.bounds_maximum);
    assert_eq!(contents.texture, Texture { width: 16, height: 16, mip_level_count: 5, block_edge: 8, layer_count: 1 });
    assert_eq!(contents.models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["bolshoi"]);
    assert_eq!(contents.models[0].replaced_buildings, tile.models[0].replaced_buildings);
    assert_eq!(contents.models[0].first_index..contents.models[0].index_count, 0..3);
    assert_eq!([contents.models[0].latitude, contents.models[0].longitude], [55.7602, 37.6186]);
    contents.check_geometry(&bytes[contents.vertex_range.clone()], &bytes[contents.index_range.clone()]).unwrap();
}

/// The GPU's blocks start on the alignment the reference names, and the
/// tile ends on it: a tile mapped from the disk hands them over whole.
#[test]
fn the_blocks_are_aligned() {
    let bytes = make_tile().serialized();
    let contents = ModelTileContents::decode(&bytes).unwrap();

    for offset in [contents.vertex_range.start, contents.index_range.start, contents.texture_range.start, bytes.len()] {
        assert_eq!(offset % ModelTileContents::BLOCK_ALIGNMENT, 0);
    }
}

/// A vertex is its sixteen bytes in the reference's order: the place in the
/// tile's box, the texture coordinate, the normal, the layer and two
/// reserved bytes.
#[test]
fn a_vertex_lies_as_the_reference_says() {
    let mut vertex = ModelTileVertex::new([0.0, -1.0, 1.0], 3, [0.0, 1.0]);
    vertex.place([110.0, 120.0, 0.0], [10.0, 20.0, 0.0], [110.0, 220.0, 50.0]);
    let mut block = Vec::new();
    vertex.append_to(&mut block);

    assert_eq!(
        block,
        [0xFF, 0xFF, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0x00, 0x81, 0x7F, 0x03, 0x00, 0x00]
    );
    assert_eq!(block.len(), ModelTileContents::VERTEX_STRIDE);
}

/// The decoder's matrix takes a written place back to where it was, to
/// within a step of the box.
#[test]
fn a_placed_position_comes_back_through_the_tiles_matrix() {
    let tile = make_tile();
    let contents = ModelTileContents::decode(&tile.serialized()).unwrap();
    let mut vertex = ModelTileVertex::new([0.0, 0.0, 1.0], 0, [0.0, 0.0]);
    let position = [47.3, 151.9, 12.4];
    vertex.place(position, tile.bounds_minimum, tile.bounds_maximum);

    let stored = [vertex.x, vertex.y, vertex.z].map(|v| f32::from(v) / 65_535.0);
    let matrix = contents.position_matrix();
    for axis in 0..3 {
        let back = matrix[axis * 4 + axis] * stored[axis] + matrix[12 + axis];
        let step = (tile.bounds_maximum[axis] - tile.bounds_minimum[axis]) / 65_535.0;
        assert!((back - position[axis]).abs() <= step, "axis {axis}: {back} for {}", position[axis]);
    }
}

/// Two vertices of one surface at one place are one record, and a baker
/// writes it once.
#[test]
fn vertices_of_one_place_and_surface_are_equal() {
    let minimum = [0.0, 0.0, 0.0];
    let maximum = [100.0, 100.0, 100.0];
    let mut first = ModelTileVertex::new([1.0, 0.0, 0.0], 0, [0.25, 0.75]);
    let mut second = first;
    let mut other = ModelTileVertex::new([0.0, 1.0, 0.0], 0, [0.25, 0.75]);
    first.place([5.0, 6.0, 7.0], minimum, maximum);
    second.place([5.0, 6.0, 7.0], minimum, maximum);
    other.place([5.0, 6.0, 7.0], minimum, maximum);

    assert_eq!(first, second);
    assert_ne!(first, other);
    assert_eq!(HashSet::from([first, second, other]).len(), 2);
}
