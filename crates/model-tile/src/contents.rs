// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! A model tile, decoded: every model whose origin lies in one map tile,
//! merged into one mesh, with the places of the tile's vertex, index and
//! texture blocks in the bytes. One vertex buffer, one index buffer and one
//! texture array serve the whole tile and every vertex is in the tile's own
//! space, so the frame draws the whole tile in one call under the tile's
//! matrix.
//!
//! Tile space is the space the map's own buildings are in: X east and Y
//! north from the tile's south-west corner, `TILE_EXTENT` units to a tile
//! edge, Z up from the ground in the same units. A model reaching over the
//! tile's edge has coordinates outside that range. It is not cut.
//!
//! The format, version 6. This comment is its reference: the archives are
//! baked outside this repository, by the baker of the project the models
//! are made in, which writes a tile with the Swift module's `ModelTile` (of
//! which `crate::ModelTile` is a port).
//!
//! All integers and floats are little-endian. The blocks follow the header
//! in the order below. The vertex, index and texture blocks each start on a
//! multiple of `BLOCK_ALIGNMENT` bytes, and the tile ends on one, so a tile
//! mapped from the disk cache hands each block to the GPU without a copy.
//!
//! Header, `HEADER_BYTE_COUNT` bytes:
//!
//! ```text
//!     0   4 bytes  magic "IMMT"
//!     4   u32      version, 6
//!     8   u32 x 3  tile zoom, x, y
//!     20  u32      model count
//!     24  u32      vertex count
//!     28  u32      index count
//!     32  u32      vertex stride, 16
//!     36  u32      texture layer count
//!     40  u32 x 2  texture width, height of mip level 0
//!     48  u32      texture mip level count
//!     52  u32      ASTC block edge: 8 means 8x8
//!     56  u32 x 2  model table offset, byte count
//!     64  u32 x 2  string block offset, byte count
//!     72  u32 x 2  vertex block offset, byte count
//!     80  u32 x 2  index block offset, byte count
//!     88  u32 x 2  texture block offset, byte count
//!     96  u32 x 2  replaced building block offset, byte count
//!     104 u32      reserved, 0
//!     108 u32      reserved, 0
//!     112 f32 x 3  minimum of the tile's bounding box, tile space
//!     124 f32 x 3  maximum of the tile's bounding box, tile space
//! ```
//!
//! Model table, `MODEL_RECORD_BYTE_COUNT` bytes per model:
//!
//! ```text
//!     0   f64 x 2  latitude, longitude of the model's origin, degrees
//!     16  f32      altitude of the origin above the map surface, meters,
//!                  already in the vertices
//!     20  u32 x 2  the buildings the model replaces, in the replaced
//!                  building block: first entry, entry count
//!     28  u32      reserved, 0
//!     32  u32 x 2  the model's id in the string block: offset, byte count
//!     40  u32 x 2  first index, index count
//!     48  u32 x 2  first vertex, vertex count
//!     56  f32 x 3  minimum of the model's bounding box, tile space
//!     68  f32 x 3  maximum of the model's bounding box, tile space
//!     80  u32      reserved, 0
//! ```
//!
//! String block: the ids, UTF-8, one after another.
//!
//! Replaced building block, `REPLACED_BUILDING_ENTRY_BYTE_COUNT` bytes per
//! entry: a feature of the map's own tiles a model stands in for, in the
//! map tiles of one zoom.
//!
//! ```text
//!     0   u64      the element: its kind in the top four bits (1 node,
//!                  2 way, 3 relation) and its OSM id in the rest
//!     8   u32      the zoom of the map tiles the entry is for
//!     12  u32      reserved, 0
//! ```
//!
//! An id names a building only within a zoom. The deepest map tiles carry
//! one feature per OSM element. The tiles above them merge buildings into
//! groups, and a group goes by the id of one of its members: there the
//! same id is a group of buildings, most of them elsewhere. So a model
//! lists what it replaces zoom by zoom, and the map hides in a tile only
//! the entries of that tile's zoom. A model lists every feature the map
//! would draw in its place: the map hides exactly the ids it is given.
//!
//! Vertex, `VERTEX_STRIDE` bytes:
//!
//! ```text
//!     0   u16 x 3  position, unsigned normalized across the tile's
//!                  bounding box
//!     6   u16 x 2  texture coordinate, unsigned normalized, V down
//!     10  i8 x 3   normal, signed normalized, tile space
//!     13  u8       texture layer
//!     14  u16      reserved, 0
//! ```
//!
//! A position is a place in the tile's bounding box, the header's: 0 on an
//! axis is the box's minimum there and 65535 its maximum, so a position in
//! tile space is `minimum + stored / 65535 * (maximum - minimum)`
//! (`position_matrix`). Across a tile of zoom 14 with the models reaching
//! over its edges a step is two to three centimeters on the ground, and
//! finer in height. The stride is a multiple of four bytes, which some
//! GPUs ask of a vertex. Version 5 kept a position as three floats in a
//! vertex of 20 bytes.
//!
//! Index block: `u32` triangle list, counter-clockwise front faces seen
//! from outside. An index counts from the start of the tile's vertex block.
//!
//! Texture block: ASTC LDR blocks with no file header, raw channel values
//! (no sRGB decode). Mip level 0 first: all layers of a level, layer 0
//! first, then the next level. A level of `w` by `h` pixels takes
//! `ceil(w / edge) * ceil(h / edge) * 16` bytes per layer.

use std::ops::Range;

use crate::error::malformed;
use crate::{ModelTileElement, ModelTileFormatError};

/// A feature of the map's tiles a model stands in for, in the map tiles of
/// one zoom: an id names a building only within a zoom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReplacedBuilding {
    pub element: ModelTileElement,
    pub map_tile_zoom: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub id: String,
    /// The model's origin, in degrees, and its height above the map's
    /// surface in meters, already in the vertices. The Swift decoder skips
    /// these: the web engine places a model's pin and its tap from them.
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_meters: f32,
    /// The features of the map's own tiles the model stands in for, each
    /// in the map tiles of one zoom: the building's outline and its parts
    /// in the deepest tiles, the groups they are merged into above. The map
    /// leaves out exactly these. Empty for a model that stands where the
    /// map has no building.
    pub replaced_buildings: Vec<ReplacedBuilding>,
    pub first_index: usize,
    pub index_count: usize,
    /// In tile space.
    pub bounds_minimum: [f32; 3],
    pub bounds_maximum: [f32; 3],
}

/// The texture array of a tile, as the header describes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Texture {
    pub width: usize,
    pub height: usize,
    pub mip_level_count: usize,
    /// The edge of an ASTC block in pixels: 8 for 8x8.
    pub block_edge: usize,
    pub layer_count: usize,
}

impl Texture {
    /// The pixels of a mip level: width, height.
    pub fn level_size(&self, level: usize) -> (usize, usize) {
        ((self.width >> level).max(1), (self.height >> level).max(1))
    }

    /// The blocks a mip level takes: across, down.
    pub fn level_blocks(&self, level: usize) -> (usize, usize) {
        let (width, height) = self.level_size(level);
        (self.blocks_across(width), self.blocks_across(height))
    }

    /// The bytes one layer takes at a mip level.
    pub fn layer_byte_count(&self, level: usize) -> usize {
        let (across, down) = self.level_blocks(level);
        across * down * 16
    }

    /// The bytes of one row of blocks at a mip level.
    pub fn bytes_per_row(&self, level: usize) -> usize {
        self.level_blocks(level).0 * 16
    }

    /// The bytes of the whole array: every layer of every level.
    pub fn byte_count(&self) -> usize {
        (0..self.mip_level_count).map(|level| self.layer_byte_count(level) * self.layer_count).sum()
    }

    /// `byte_count` in 64 bits, for a header that is not yet trusted: on
    /// a 32-bit target the largest description the decoder allows would
    /// overflow.
    fn byte_count_wide(&self) -> u64 {
        (0..self.mip_level_count)
            .map(|level| {
                let (across, down) = self.level_blocks(level);
                across as u64 * down as u64 * 16 * self.layer_count as u64
            })
            .sum()
    }

    fn blocks_across(&self, pixels: usize) -> usize {
        pixels.div_ceil(self.block_edge)
    }
}

/// A model tile, decoded (the module's comment is the format's reference).
#[derive(Debug, Clone, PartialEq)]
pub struct ModelTileContents {
    /// The map tile the models' origins lie in.
    pub zoom: u32,
    pub x: u32,
    pub y: u32,
    pub models: Vec<Model>,
    pub vertex_count: usize,
    pub index_count: usize,
    /// The bounds of every vertex of the tile, in tile space.
    pub bounds_minimum: [f32; 3],
    pub bounds_maximum: [f32; 3],
    pub texture: Texture,
    /// The places of the blocks in the tile's bytes.
    pub vertex_range: Range<usize>,
    pub index_range: Range<usize>,
    pub texture_range: Range<usize>,
}

impl ModelTileContents {
    /// The zoom of the map tiles a model archive holds: a map asks for
    /// model tiles of this zoom alone, whatever zoom it is at, and a baker
    /// writes these.
    pub const TILE_ZOOM: u32 = 14;
    pub const MAGIC: [u8; 4] = *b"IMMT";
    pub const VERSION: u32 = 6;
    pub const REPLACED_BUILDING_ENTRY_BYTE_COUNT: usize = 16;
    pub const HEADER_BYTE_COUNT: usize = 136;
    /// The units of tile space along a tile's edge, the map's own extent.
    pub const TILE_EXTENT: f32 = 4096.0;
    /// The bits of a replaced building entry the element's kind sits above.
    pub const REPLACED_BUILDING_KIND_SHIFT: u64 = 60;
    pub const MODEL_RECORD_BYTE_COUNT: usize = 84;
    pub const VERTEX_STRIDE: usize = 16;
    /// The byte offsets inside a vertex.
    pub const TEXTURE_COORDINATE_OFFSET: usize = 6;
    pub const NORMAL_OFFSET: usize = 10;
    pub const LAYER_OFFSET: usize = 13;
    /// What the vertex, index and texture blocks each start on a multiple
    /// of, and the tile ends on: a page of the devices' memory.
    pub const BLOCK_ALIGNMENT: usize = 16_384;

    /// Reads a tile's header and tables and checks what they say: the
    /// blocks lie inside the bytes and the models' ranges inside the
    /// blocks. It reads the first bytes of the tile only, so a tile mapped
    /// from the disk is not paged in whole. The vertices and indices
    /// themselves are checked once they are in their buffers
    /// (`check_geometry`).
    pub fn decode(data: &[u8]) -> Result<ModelTileContents, ModelTileFormatError> {
        if data.len() < Self::HEADER_BYTE_COUNT {
            return Err(ModelTileFormatError::Truncated);
        }
        let mut reader = Reader { data, offset: 0 };
        if reader.bytes(4) != Self::MAGIC {
            return Err(ModelTileFormatError::BadMagic);
        }
        let version = reader.u32();
        if version != Self::VERSION {
            return Err(ModelTileFormatError::UnsupportedVersion(version));
        }
        let zoom = reader.u32();
        let x = reader.u32();
        let y = reader.u32();
        let model_count = reader.u32() as u64;
        let vertex_count = reader.u32() as u64;
        let index_count = reader.u32() as u64;
        let stride = reader.u32();
        if stride != Self::VERTEX_STRIDE as u32 {
            return Err(ModelTileFormatError::UnsupportedVertexStride(stride));
        }
        let layer_count = reader.u32() as usize;
        let width = reader.u32() as usize;
        let height = reader.u32() as usize;
        let mip_level_count = reader.u32() as usize;
        let block_edge = reader.u32() as usize;

        let mut block = |name: &str| -> Result<Range<usize>, ModelTileFormatError> {
            let offset = reader.u32() as u64;
            let byte_count = reader.u32() as u64;
            if offset < Self::HEADER_BYTE_COUNT as u64 || offset + byte_count > data.len() as u64 {
                return Err(malformed(format!("the {name} block lies outside the tile")));
            }
            Ok(offset as usize..(offset + byte_count) as usize)
        };
        let table_range = block("model table")?;
        let string_range = block("string")?;
        let vertex_range = block("vertex")?;
        let index_range = block("index")?;
        let texture_range = block("texture")?;
        let replaced_range = block("replaced building")?;
        // Reserved.
        reader.u32();
        reader.u32();
        let bounds_minimum = reader.vector();
        let bounds_maximum = reader.vector();

        if zoom > 24 || x >= 1 << zoom || y >= 1 << zoom {
            return Err(malformed("the tile coordinate is outside its zoom"));
        }
        if table_range.len() as u64 != model_count * Self::MODEL_RECORD_BYTE_COUNT as u64
            || vertex_range.len() as u64 != vertex_count * Self::VERTEX_STRIDE as u64
            || index_range.len() as u64 != index_count * 4
            || !replaced_range.len().is_multiple_of(Self::REPLACED_BUILDING_ENTRY_BYTE_COUNT)
            || !index_count.is_multiple_of(3)
        {
            return Err(malformed("a block's byte count disagrees with its element count"));
        }
        let texture = Texture { width, height, mip_level_count, block_edge, layer_count };
        if width == 0
            || height == 0
            || width > 4096
            || height > 4096
            || layer_count == 0
            || layer_count > 256
            || ![4, 5, 6, 8, 10, 12].contains(&block_edge)
            || mip_level_count == 0
            || mip_level_count > (usize::BITS - width.max(height).leading_zeros()) as usize
            || texture.byte_count_wide() != texture_range.len() as u64
        {
            return Err(malformed("the texture description disagrees with the texture block"));
        }

        // The replaced buildings of every model, one after another.
        let entry_count = replaced_range.len() / Self::REPLACED_BUILDING_ENTRY_BYTE_COUNT;
        let mut replaced = Vec::with_capacity(entry_count);
        reader.offset = replaced_range.start;
        for _ in 0..entry_count {
            let entry = reader.u64();
            let map_tile_zoom = reader.u32();
            reader.u32();
            let id = entry & ((1 << Self::REPLACED_BUILDING_KIND_SHIFT) - 1);
            let Some(element) = ModelTileElement::from_kind(entry >> Self::REPLACED_BUILDING_KIND_SHIFT, id) else {
                return Err(malformed("a replaced building is an element of an unknown kind"));
            };
            if map_tile_zoom > 24 {
                return Err(malformed("a replaced building is listed for a zoom no map tile has"));
            }
            replaced.push(ReplacedBuilding { element, map_tile_zoom });
        }

        if !bounds_minimum.iter().chain(&bounds_maximum).all(|v| v.is_finite()) {
            return Err(malformed("the tile's bounds are not numbers"));
        }

        let mut models = Vec::with_capacity(model_count as usize);
        reader.offset = table_range.start;
        for _ in 0..model_count {
            let latitude = reader.f64();
            let longitude = reader.f64();
            let altitude_meters = reader.f32();
            let first_replaced = reader.u32() as u64;
            let replaced_count = reader.u32() as u64;
            reader.u32();
            let id_offset = reader.u32() as u64;
            let id_byte_count = reader.u32() as u64;
            let first_index = reader.u32() as u64;
            let model_index_count = reader.u32() as u64;
            let first_vertex = reader.u32() as u64;
            let model_vertex_count = reader.u32() as u64;
            let model_minimum = reader.vector();
            let model_maximum = reader.vector();
            reader.u32();

            let id = (id_offset + id_byte_count <= string_range.len() as u64)
                .then(|| {
                    let start = string_range.start + id_offset as usize;
                    std::str::from_utf8(&data[start..start + id_byte_count as usize]).ok()
                })
                .flatten()
                .ok_or_else(|| malformed("a model's id lies outside the string block"))?
                .to_owned();
            if first_index + model_index_count > index_count
                || first_vertex + model_vertex_count > vertex_count
                || !model_index_count.is_multiple_of(3)
            {
                return Err(malformed(format!("model {id} reaches outside the tile's vertices or indices")));
            }
            if !latitude.is_finite()
                || !longitude.is_finite()
                || latitude.abs() > 90.0
                || longitude.abs() > 180.0
                || !altitude_meters.is_finite()
                || !model_minimum.iter().chain(&model_maximum).all(|v| v.is_finite())
            {
                return Err(malformed(format!("model {id} has a placement that is not a number")));
            }
            if first_replaced + replaced_count > replaced.len() as u64 {
                return Err(malformed(format!("model {id} reaches outside the replaced buildings")));
            }
            let replaced_buildings = replaced[first_replaced as usize..(first_replaced + replaced_count) as usize].to_vec();
            models.push(Model {
                id,
                latitude,
                longitude,
                altitude_meters,
                replaced_buildings,
                first_index: first_index as usize,
                index_count: model_index_count as usize,
                bounds_minimum: model_minimum,
                bounds_maximum: model_maximum,
            });
        }

        Ok(ModelTileContents {
            zoom,
            x,
            y,
            models,
            vertex_count: vertex_count as usize,
            index_count: index_count as usize,
            bounds_minimum,
            bounds_maximum,
            texture,
            vertex_range,
            index_range,
            texture_range,
        })
    }

    /// A stored position, 0...1 on each axis across the tile's bounding
    /// box, into tile space: the box's minimum and its size, column-major.
    /// A draw takes it after the tile's own matrix, so the vertices stay as
    /// the tile wrote them.
    #[rustfmt::skip]
    pub fn position_matrix(&self) -> [f32; 16] {
        let [min_x, min_y, min_z] = self.bounds_minimum;
        let [max_x, max_y, max_z] = self.bounds_maximum;
        [
            max_x - min_x, 0.0, 0.0, 0.0,
            0.0, max_y - min_y, 0.0, 0.0,
            0.0, 0.0, max_z - min_z, 0.0,
            min_x, min_y, min_z, 1.0,
        ]
    }

    /// Checks the tile's vertex and index blocks, given as they lie in the
    /// tile: an index past the vertices or a layer past the texture array
    /// would have the GPU read what is not there. Run on the bytes the
    /// blocks were loaded into, before the tile draws.
    pub fn check_geometry(&self, vertices: &[u8], indices: &[u8]) -> Result<(), ModelTileFormatError> {
        if vertices.len() != self.vertex_range.len() || indices.len() != self.index_range.len() {
            return Err(ModelTileFormatError::Truncated);
        }
        let vertex_count = self.vertex_count as u64;
        // A fold rather than an early return: the loop vectorizes.
        let largest = indices.as_chunks::<4>().0.iter().fold(0u32, |largest, index| largest.max(u32::from_le_bytes(*index)));
        if !indices.is_empty() && u64::from(largest) >= vertex_count {
            return Err(malformed("an index names a vertex past the vertex block"));
        }
        let deepest = vertices
            .as_chunks::<{ Self::VERTEX_STRIDE }>()
            .0
            .iter()
            .fold(0u8, |deepest, vertex| deepest.max(vertex[Self::LAYER_OFFSET]));
        if !vertices.is_empty() && usize::from(deepest) >= self.texture.layer_count {
            return Err(malformed("a vertex names a texture layer past the texture array"));
        }
        Ok(())
    }
}

/// Reads little-endian values in order. The caller has checked that the
/// bytes it reads exist.
struct Reader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl Reader<'_> {
    fn bytes(&mut self, count: usize) -> &[u8] {
        let bytes = &self.data[self.offset..self.offset + count];
        self.offset += count;
        bytes
    }

    fn array<const N: usize>(&mut self) -> [u8; N] {
        let mut array = [0; N];
        array.copy_from_slice(self.bytes(N));
        array
    }

    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.array())
    }

    fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.array())
    }

    fn f32(&mut self) -> f32 {
        f32::from_bits(self.u32())
    }

    fn f64(&mut self) -> f64 {
        f64::from_bits(self.u64())
    }

    fn vector(&mut self) -> [f32; 3] {
        [self.f32(), self.f32(), self.f32()]
    }
}
