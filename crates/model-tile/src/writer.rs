// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! A model tile to be written: what a baker hands over, and
//! `ModelTile::serialized` lays out as the format describes (the comment on
//! `crate::contents` is the reference). Every model of the tile is merged
//! into one vertex block and one index block, in the tile's own space, with
//! one texture array.
//!
//! The baker owns everything before the bytes: reading the models, placing
//! them in tile space, encoding the textures, merging the vertices
//! (`ModelTileVertex`). This module owns the layout alone.

use crate::{ByteWriter, ModelTileContents, ReplacedBuilding};

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub id: String,
    /// The model's origin, in degrees, and its height above the map's
    /// surface in meters, already in the vertices.
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_meters: f32,
    /// The features of the map's own tiles the model stands in for, each
    /// in the map tiles of one zoom.
    pub replaced_buildings: Vec<ReplacedBuilding>,
    /// The model's part of the tile's index and vertex blocks.
    pub first_index: u32,
    pub index_count: u32,
    pub first_vertex: u32,
    pub vertex_count: u32,
    /// In tile space.
    pub bounds_minimum: [f32; 3],
    pub bounds_maximum: [f32; 3],
}

/// The texture array of a tile: `levels[level][layer]` is the ASTC blocks
/// of one layer at one mip level.
#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    pub width: usize,
    pub height: usize,
    /// The edge of an ASTC block in pixels: 8 for 8x8.
    pub block_edge: usize,
    pub layer_count: usize,
    pub levels: Vec<Vec<Vec<u8>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelTile {
    /// The map tile the models' origins lie in.
    pub zoom: u32,
    pub x: u32,
    pub y: u32,
    /// The models, every one's indices merged.
    pub models: Vec<Model>,
    /// The bounds of every vertex of the tile, in tile space: the box the
    /// vertices' positions are places in (`ModelTileVertex::place`).
    pub bounds_minimum: [f32; 3],
    pub bounds_maximum: [f32; 3],
    /// `vertex_count` vertices of `ModelTileContents::VERTEX_STRIDE` bytes,
    /// each written by `ModelTileVertex::append_to`.
    pub vertices: Vec<u8>,
    /// A triangle list, an index counting from the start of `vertices`.
    pub indices: Vec<u32>,
    pub texture: Texture,
}

impl ModelTile {
    pub fn vertex_count(&self) -> usize {
        self.vertices.len() / ModelTileContents::VERTEX_STRIDE
    }

    /// The tile's bytes, as `ModelTileContents::decode` reads them.
    pub fn serialized(&self) -> Vec<u8> {
        let mut strings = Vec::new();
        let mut table = ByteWriter::new();
        let mut replaced = ByteWriter::new();
        for model in &self.models {
            let id = model.id.as_bytes();
            table.f64(model.latitude);
            table.f64(model.longitude);
            table.f32(model.altitude_meters);
            table.u32((replaced.len() / ModelTileContents::REPLACED_BUILDING_ENTRY_BYTE_COUNT) as u32);
            table.u32(model.replaced_buildings.len() as u32);
            for entry in &model.replaced_buildings {
                replaced.u64(entry.element.entry());
                replaced.u32(entry.map_tile_zoom);
                replaced.u32(0);
            }
            table.u32(0);
            table.u32(strings.len() as u32);
            table.u32(id.len() as u32);
            table.u32(model.first_index);
            table.u32(model.index_count);
            table.u32(model.first_vertex);
            table.u32(model.vertex_count);
            for value in model.bounds_minimum.iter().chain(&model.bounds_maximum) {
                table.f32(*value);
            }
            table.u32(0);
            strings.extend_from_slice(id);
        }
        assert_eq!(table.len(), self.models.len() * ModelTileContents::MODEL_RECORD_BYTE_COUNT);

        let mut index_block = ByteWriter::new();
        for index in &self.indices {
            index_block.u32(*index);
        }
        let mut texture_block = Vec::new();
        for level in &self.texture.levels {
            assert_eq!(level.len(), self.texture.layer_count);
            for layer in level {
                texture_block.extend_from_slice(layer);
            }
        }

        let aligned = |offset: usize| offset.next_multiple_of(ModelTileContents::BLOCK_ALIGNMENT);
        let table_offset = ModelTileContents::HEADER_BYTE_COUNT;
        let strings_offset = table_offset + table.len();
        let replaced_offset = strings_offset + strings.len();
        let vertices_offset = aligned(replaced_offset + replaced.len());
        let indices_offset = aligned(vertices_offset + self.vertices.len());
        let texture_offset = aligned(indices_offset + index_block.len());

        let mut writer = ByteWriter::new();
        writer.bytes(&ModelTileContents::MAGIC);
        writer.u32(ModelTileContents::VERSION);
        writer.u32(self.zoom);
        writer.u32(self.x);
        writer.u32(self.y);
        writer.u32(self.models.len() as u32);
        writer.u32(self.vertex_count() as u32);
        writer.u32(self.indices.len() as u32);
        writer.u32(ModelTileContents::VERTEX_STRIDE as u32);
        writer.u32(self.texture.layer_count as u32);
        writer.u32(self.texture.width as u32);
        writer.u32(self.texture.height as u32);
        writer.u32(self.texture.levels.len() as u32);
        writer.u32(self.texture.block_edge as u32);
        for (offset, byte_count) in [
            (table_offset, table.len()),
            (strings_offset, strings.len()),
            (vertices_offset, self.vertices.len()),
            (indices_offset, index_block.len()),
            (texture_offset, texture_block.len()),
            (replaced_offset, replaced.len()),
        ] {
            writer.u32(offset as u32);
            writer.u32(byte_count as u32);
        }
        // Reserved.
        writer.u32(0);
        writer.u32(0);
        for value in self.bounds_minimum.iter().chain(&self.bounds_maximum) {
            writer.f32(*value);
        }
        assert_eq!(writer.len(), ModelTileContents::HEADER_BYTE_COUNT);

        writer.bytes(table.data());
        writer.bytes(&strings);
        writer.bytes(replaced.data());
        writer.pad_to_multiple_of(ModelTileContents::BLOCK_ALIGNMENT);
        assert_eq!(writer.len(), vertices_offset);
        writer.bytes(&self.vertices);
        writer.pad_to_multiple_of(ModelTileContents::BLOCK_ALIGNMENT);
        assert_eq!(writer.len(), indices_offset);
        writer.bytes(index_block.data());
        writer.pad_to_multiple_of(ModelTileContents::BLOCK_ALIGNMENT);
        assert_eq!(writer.len(), texture_offset);
        writer.bytes(&texture_block);
        writer.pad_to_multiple_of(ModelTileContents::BLOCK_ALIGNMENT);
        writer.into_data()
    }
}
