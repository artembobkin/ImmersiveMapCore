// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The model tile's decoder: what a written tile reads back as, and what it
//! turns away. A tile comes from the network and from a disk cache, so its
//! bytes are untrusted, and a draw relies on every range the decoder passes.

use model_tile::contents::Texture;
use model_tile::{ModelTileContents, ModelTileElement, ModelTileFormatError, ReplacedBuilding};

use crate::fixture::{FixtureModel, ModelTileFixture};

fn replaced(element: ModelTileElement, map_tile_zoom: u32) -> ReplacedBuilding {
    ReplacedBuilding { element, map_tile_zoom }
}

#[test]
fn a_written_tile_reads_back() {
    let fixture = ModelTileFixture {
        layer_count: 2,
        models: vec![
            FixtureModel { replaced: vec![(3, 3_334_755, 15)], ..FixtureModel::new("bolshoi") },
            FixtureModel {
                latitude: 55.7553,
                longitude: 37.6143,
                altitude_meters: -6.5,
                replaced: vec![(2, 31_560_418, 14), (3, 6_233_742, 15), (2, 228_130_090, 15), (1, 7, 15)],
                triangles: vec![
                    [[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 8.0, -2.0]],
                    [[1.0, 1.0, 1.0], [5.0, 1.0, 1.0], [1.0, 9.0, -1.0]],
                ],
                layer: 1,
                ..FixtureModel::new("okhotny-ryad")
            },
        ],
        ..ModelTileFixture::default()
    };
    let contents = ModelTileContents::decode(&fixture.serialized()).unwrap();

    assert_eq!([contents.zoom, contents.x, contents.y], [14, 9904, 5121]);
    assert_eq!(contents.vertex_count, 9);
    assert_eq!(contents.index_count, 9);
    assert_eq!(contents.bounds_minimum, [0.0, 0.0, -5.0]);
    assert_eq!(contents.bounds_maximum, [10.0, 20.0, 1.0]);
    assert_eq!(contents.texture.layer_count, 2);
    assert_eq!(contents.texture.block_edge, 8);
    assert_eq!(contents.texture.mip_level_count, 5);
    assert_eq!(contents.vertex_range.len(), 9 * ModelTileContents::VERTEX_STRIDE);
    assert_eq!(contents.texture_range.len(), contents.texture.byte_count());

    assert_eq!(contents.models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["bolshoi", "okhotny-ryad"]);
    let bolshoi = &contents.models[0];
    assert_eq!(bolshoi.replaced_buildings, [replaced(ModelTileElement::Relation(3_334_755), 15)]);
    assert_eq!(bolshoi.first_index, 0);
    assert_eq!(bolshoi.index_count, 3);
    assert_eq!(bolshoi.bounds_minimum, [0.0, 0.0, -5.0]);
    assert_eq!(bolshoi.bounds_maximum, [10.0, 20.0, 0.0]);

    let okhotny = &contents.models[1];
    assert_eq!(
        okhotny.replaced_buildings,
        [
            replaced(ModelTileElement::Way(31_560_418), 14),
            replaced(ModelTileElement::Relation(6_233_742), 15),
            replaced(ModelTileElement::Way(228_130_090), 15),
            replaced(ModelTileElement::Node(7), 15),
        ],
        "each feature with the zoom of the map tiles it is replaced in, in the tile's order"
    );
    assert_eq!(okhotny.first_index, 3);
    assert_eq!(okhotny.index_count, 6);
    assert_eq!([okhotny.latitude, okhotny.longitude], [55.7553, 37.6143]);
    assert_eq!(okhotny.altitude_meters, -6.5);
}

/// The bytes of a layer at each mip level, as the baker lays them out:
/// whole blocks, one at the least.
#[test]
fn the_texture_layout_counts_whole_blocks() {
    let texture = Texture { width: 512, height: 512, mip_level_count: 10, block_edge: 8, layer_count: 3 };

    assert_eq!(texture.layer_byte_count(0), 64 * 64 * 16);
    assert_eq!(texture.bytes_per_row(0), 64 * 16);
    assert_eq!(texture.layer_byte_count(6), 16, "an 8 pixel level is one block");
    assert_eq!(texture.layer_byte_count(9), 16, "a 1 pixel level is one block still");
    assert_eq!(texture.byte_count(), 3 * 16 * (4096 + 1024 + 256 + 64 + 16 + 4 + 1 + 1 + 1 + 1));
}

#[test]
fn a_model_that_replaces_nothing_lists_nothing() {
    let contents = ModelTileContents::decode(&ModelTileFixture::default().serialized()).unwrap();
    assert!(contents.models[0].replaced_buildings.is_empty());
}

/// A tile of an earlier version of the format is not read: its archive is
/// baked again. Version 5 kept a vertex's position as three floats in tile
/// space, in a vertex of 20 bytes.
#[test]
fn an_earlier_version_is_turned_away() {
    let mut tile = ModelTileFixture::default().serialized();
    tile[4] = 5;
    assert_eq!(ModelTileContents::decode(&tile), Err(ModelTileFormatError::UnsupportedVersion(5)));
}

/// A vertex keeps its position as a place in the tile's bounding box, and
/// the tile's matrix takes it into tile space: the box's corners are the
/// stored 0 and 1.
#[test]
fn the_stored_positions_span_the_tiles_bounds() {
    let tile = ModelTileFixture::default().serialized();
    let contents = ModelTileContents::decode(&tile).unwrap();
    let matrix = contents.position_matrix();
    let transform = |p: [f32; 3]| -> [f32; 4] {
        let mut out = [0.0; 4];
        for (row, value) in out.iter_mut().enumerate() {
            *value = (0..4).map(|column| matrix[column * 4 + row] * [p[0], p[1], p[2], 1.0][column]).sum();
        }
        out
    };

    let [min_x, min_y, min_z] = contents.bounds_minimum;
    let [max_x, max_y, max_z] = contents.bounds_maximum;
    assert_eq!(transform([0.0, 0.0, 0.0]), [min_x, min_y, min_z, 1.0]);
    assert_eq!(transform([1.0, 1.0, 1.0]), [max_x, max_y, max_z, 1.0]);
    // The fixture's first vertex is the model's first corner.
    let at = contents.vertex_range.start;
    let stored = |offset: usize| f32::from(u16::from_le_bytes([tile[at + offset], tile[at + offset + 1]])) / 65_535.0;
    let position = transform([stored(0), stored(2), stored(4)]);
    for (axis, value) in position[..3].iter().enumerate() {
        assert!(value.abs() < 1e-3, "axis {axis}: {value}");
    }
}

/// Every model of a tile is in the one draw: the models partition the
/// index block, and the tile's index count is what the draw takes.
#[test]
fn a_tile_of_two_models_is_one_draw_of_every_index() {
    let fixture = ModelTileFixture {
        models: vec![
            FixtureModel::new("first"),
            FixtureModel {
                triangles: vec![
                    [[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 8.0, 0.0]],
                    [[1.0, 1.0, 1.0], [5.0, 1.0, 1.0], [1.0, 9.0, 1.0]],
                ],
                ..FixtureModel::new("second")
            },
        ],
        ..ModelTileFixture::default()
    };
    let contents = ModelTileContents::decode(&fixture.serialized()).unwrap();

    assert_eq!(contents.index_count, 9);
    assert_eq!(contents.index_range.len(), 9 * 4);
    let ranges: Vec<_> = contents.models.iter().map(|m| m.first_index..m.first_index + m.index_count).collect();
    assert_eq!(ranges, [0..3, 3..9]);
}

/// The first bakes of this version wrote a zoom the models showed from,
/// where the header now has a reserved field. There is no such rule, and an
/// archive baked then reads as one baked now.
#[test]
fn the_reserved_header_field_is_ignored() {
    let tile = ModelTileFixture::default().serialized();
    let mut earlier = tile.clone();
    earlier[108] = 15;

    assert_eq!(ModelTileContents::decode(&earlier), ModelTileContents::decode(&tile));
}

#[test]
fn other_bytes_are_not_a_tile() {
    assert_eq!(ModelTileContents::decode(&[0; 4096]), Err(ModelTileFormatError::BadMagic));
    assert_eq!(ModelTileContents::decode(b"IMMT"), Err(ModelTileFormatError::Truncated));
}

#[test]
fn another_version_is_turned_away() {
    let mut tile = ModelTileFixture::default().serialized();
    tile[4] = 7;
    assert_eq!(ModelTileContents::decode(&tile), Err(ModelTileFormatError::UnsupportedVersion(7)));
}

#[test]
fn another_vertex_stride_is_turned_away() {
    let mut tile = ModelTileFixture::default().serialized();
    tile[32] = 20;
    assert_eq!(ModelTileContents::decode(&tile), Err(ModelTileFormatError::UnsupportedVertexStride(20)));
}

/// A tile cut short by a failed download keeps its header, and the header
/// then names blocks the bytes do not hold.
#[test]
fn a_tile_cut_short_is_turned_away() {
    let tile = ModelTileFixture::default().serialized();
    let result = ModelTileContents::decode(&tile[..tile.len() - 300]);
    assert!(matches!(result, Err(ModelTileFormatError::Malformed(_))), "expected a malformed tile, got {result:?}");
}

/// A header that does not hold together is turned away with the place it
/// was found: the coordinate, the counts, the texture.
#[test]
fn a_header_that_disagrees_with_itself_is_turned_away() {
    let tile = ModelTileFixture::default().serialized();
    let with = |offset: usize, value: u32| {
        let mut tile = tile.clone();
        tile[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        ModelTileContents::decode(&tile)
    };
    let malformed = |reason: &str| Err(ModelTileFormatError::Malformed(reason.to_owned()));
    assert_eq!(with(12, 1 << 14), malformed("the tile coordinate is outside its zoom"));
    assert_eq!(with(24, 4), malformed("a block's byte count disagrees with its element count"));
    assert_eq!(with(52, 7), malformed("the texture description disagrees with the texture block"));
    assert_eq!(with(48, 6), malformed("the texture description disagrees with the texture block"));
    assert_eq!(with(72, 100), malformed("the vertex block lies outside the tile"));
}

/// The check of the vertex and index blocks, as the mesh runs it on the
/// buffers the blocks were loaded into.
fn check_geometry(tile: &[u8]) -> Result<(), ModelTileFormatError> {
    let contents = ModelTileContents::decode(tile)?;
    contents.check_geometry(&tile[contents.vertex_range.clone()], &tile[contents.index_range.clone()])
}

#[test]
fn a_written_tiles_geometry_passes_the_check() {
    assert_eq!(check_geometry(&ModelTileFixture::default().serialized()), Ok(()));
}

/// An index past the vertices would have the GPU read past the buffer. The
/// tables say nothing of it: the check of the blocks does.
#[test]
fn an_index_past_the_vertices_is_turned_away() {
    let mut tile = ModelTileFixture::default().serialized();
    let contents = ModelTileContents::decode(&tile).unwrap();
    tile[contents.index_range.start] = 200;
    assert!(ModelTileContents::decode(&tile).is_ok());
    let result = check_geometry(&tile);
    assert!(matches!(result, Err(ModelTileFormatError::Malformed(_))), "expected a malformed tile, got {result:?}");
}

#[test]
fn a_layer_past_the_texture_array_is_turned_away() {
    let mut fixture = ModelTileFixture::default();
    fixture.models[0].layer = 1;
    let result = check_geometry(&fixture.serialized());
    assert!(matches!(result, Err(ModelTileFormatError::Malformed(_))), "expected a malformed tile, got {result:?}");
}

/// The blocks of another tile, shorter or longer, are not this tile's.
#[test]
fn blocks_of_another_size_are_turned_away() {
    let tile = ModelTileFixture::default().serialized();
    let contents = ModelTileContents::decode(&tile).unwrap();
    let vertices = &tile[contents.vertex_range.clone()];
    let indices = &tile[contents.index_range.start..contents.index_range.end - 4];
    assert_eq!(contents.check_geometry(vertices, indices), Err(ModelTileFormatError::Truncated));
}

/// The decoder takes a slice of a larger buffer as it takes a whole one:
/// the offsets in a tile count from the tile's own start.
#[test]
fn a_slice_of_a_larger_buffer_decodes() {
    let tile = ModelTileFixture::default().serialized();
    let mut buffer = vec![0xAA; 100];
    buffer.extend_from_slice(&tile);
    assert_eq!(ModelTileContents::decode(&buffer[100..]), ModelTileContents::decode(&tile));
}
