// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Writes a model tile the way the format describes it, so a test states a
//! tile as models and the bytes travel the crate's decoder. The decoder
//! never sees this code: the fixture has its own writer, as the PMTiles
//! fixtures do, so the round trip checks the decoder against the format and
//! not against itself. A port of the Swift `ModelTileFixture`.

pub struct FixtureModel {
    pub id: String,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_meters: f32,
    /// The map features the model replaces: the kind (1 node, 2 way,
    /// 3 relation) and the id of each, and the zoom of the map tiles the
    /// entry is for.
    pub replaced: Vec<(u64, u64, u32)>,
    /// The model's triangles: three vertices each, a position in tile
    /// space.
    pub triangles: Vec<[[f32; 3]; 3]>,
    /// The texture layer of every vertex.
    pub layer: u8,
}

impl FixtureModel {
    pub fn new(id: &str) -> FixtureModel {
        FixtureModel {
            id: id.to_owned(),
            latitude: 55.7558,
            longitude: 37.6173,
            altitude_meters: 0.0,
            replaced: Vec::new(),
            triangles: vec![[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 20.0, -5.0]]],
            layer: 0,
        }
    }
}

pub struct ModelTileFixture {
    pub zoom: u32,
    pub x: u32,
    pub y: u32,
    pub models: Vec<FixtureModel>,
    pub layer_count: usize,
    pub texture_size: usize,
    pub block_edge: usize,
    /// The alignment of the GPU blocks. The baker uses a page, and a test
    /// keeps its tiles small.
    pub block_alignment: usize,
}

impl Default for ModelTileFixture {
    fn default() -> Self {
        ModelTileFixture {
            zoom: 14,
            x: 9904,
            y: 5121,
            models: vec![FixtureModel::new("model")],
            layer_count: 1,
            texture_size: 16,
            block_edge: 8,
            block_alignment: 256,
        }
    }
}

/// The ASTC block of one opaque colour: a void extent block, the simplest
/// block the format has.
pub const SOLID_BLOCK: [u8; 16] = [0xFC, 0xFD, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x80, 0x00, 0x60, 0x00, 0x40, 0xFF, 0xFF];

impl ModelTileFixture {
    pub fn mip_level_count(&self) -> usize {
        self.texture_size.ilog2() as usize + 1
    }

    pub fn serialized(&self) -> Vec<u8> {
        let mut strings = Vec::new();
        let mut replaced = Vec::new();
        let mut table = Vec::new();
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut vertex_count = 0u32;
        let mut index_count = 0u32;
        // The tile's bounding box first: a vertex keeps its position as a
        // place in it.
        let mut tile_minimum = [f32::INFINITY; 3];
        let mut tile_maximum = [f32::NEG_INFINITY; 3];
        for position in self.models.iter().flat_map(|m| m.triangles.iter().flatten()) {
            for axis in 0..3 {
                tile_minimum[axis] = tile_minimum[axis].min(position[axis]);
                tile_maximum[axis] = tile_maximum[axis].max(position[axis]);
            }
        }
        let tile_size: Vec<f32> = (0..3).map(|axis| tile_maximum[axis] - tile_minimum[axis]).collect();
        for model in &self.models {
            let first_vertex = vertex_count;
            let first_index = index_count;
            let mut minimum = [f32::INFINITY; 3];
            let mut maximum = [f32::NEG_INFINITY; 3];
            for position in model.triangles.iter().flatten() {
                for axis in 0..3 {
                    minimum[axis] = minimum[axis].min(position[axis]);
                    maximum[axis] = maximum[axis].max(position[axis]);
                    let share = if tile_size[axis] > 0.0 { (position[axis] - tile_minimum[axis]) / tile_size[axis] } else { 0.0 };
                    vertices.extend_from_slice(&((share * 65_535.0).round() as u16).to_le_bytes());
                }
                vertices.extend_from_slice(&0u16.to_le_bytes());
                vertices.extend_from_slice(&65_535u16.to_le_bytes());
                // The normal straight up, then the layer and the reserved
                // bytes.
                vertices.extend_from_slice(&[0, 127, 0, model.layer, 0, 0]);
                indices.extend_from_slice(&vertex_count.to_le_bytes());
                vertex_count += 1;
                index_count += 1;
            }
            let id = model.id.as_bytes();
            table.extend_from_slice(&model.latitude.to_bits().to_le_bytes());
            table.extend_from_slice(&model.longitude.to_bits().to_le_bytes());
            table.extend_from_slice(&model.altitude_meters.to_bits().to_le_bytes());
            let mut words = vec![(replaced.len() / 16) as u32, model.replaced.len() as u32];
            for &(kind, element, zoom) in &model.replaced {
                replaced.extend_from_slice(&(kind << 60 | element).to_le_bytes());
                replaced.extend_from_slice(&zoom.to_le_bytes());
                replaced.extend_from_slice(&0u32.to_le_bytes());
            }
            words.extend([0, strings.len() as u32, id.len() as u32]);
            words.extend([first_index, index_count - first_index, first_vertex, vertex_count - first_vertex]);
            words.extend(minimum.iter().chain(&maximum).map(|v| v.to_bits()));
            words.push(0);
            for word in words {
                table.extend_from_slice(&word.to_le_bytes());
            }
            strings.extend_from_slice(id);
        }

        let mut texture = Vec::new();
        for level in 0..self.mip_level_count() {
            let size = (self.texture_size >> level).max(1);
            let blocks = size.div_ceil(self.block_edge);
            for _ in 0..self.layer_count * blocks * blocks {
                texture.extend_from_slice(&SOLID_BLOCK);
            }
        }

        let aligned = |offset: usize| offset.div_ceil(self.block_alignment) * self.block_alignment;
        let table_offset = 136;
        let strings_offset = table_offset + table.len();
        let replaced_offset = strings_offset + strings.len();
        let vertices_offset = aligned(replaced_offset + replaced.len());
        let indices_offset = aligned(vertices_offset + vertices.len());
        let texture_offset = aligned(indices_offset + indices.len());

        let mut tile = b"IMMT".to_vec();
        for value in [
            6,
            self.zoom as usize,
            self.x as usize,
            self.y as usize,
            self.models.len(),
            vertex_count as usize,
            index_count as usize,
            16,
            self.layer_count,
            self.texture_size,
            self.texture_size,
            self.mip_level_count(),
            self.block_edge,
            table_offset,
            table.len(),
            strings_offset,
            strings.len(),
            vertices_offset,
            vertices.len(),
            indices_offset,
            indices.len(),
            texture_offset,
            texture.len(),
            replaced_offset,
            replaced.len(),
            0,
            0,
        ] {
            tile.extend_from_slice(&(value as u32).to_le_bytes());
        }
        for value in tile_minimum.iter().chain(&tile_maximum) {
            tile.extend_from_slice(&value.to_bits().to_le_bytes());
        }
        tile.extend_from_slice(&table);
        tile.extend_from_slice(&strings);
        tile.extend_from_slice(&replaced);
        tile.resize(vertices_offset, 0);
        tile.extend_from_slice(&vertices);
        tile.resize(indices_offset, 0);
        tile.extend_from_slice(&indices);
        tile.resize(texture_offset, 0);
        tile.extend_from_slice(&texture);
        tile.resize(aligned(tile.len()), 0);
        tile
    }
}
