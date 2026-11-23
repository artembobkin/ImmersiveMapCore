// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The decode of a model tile for the worker: the checks, the texture as
//! the GPU takes it, and the meta JSON.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write};
use std::ops::Range;

use map_style::schema::default::ProtomapsBasemapSchema;
use map_style::schema::{OsmElement, TileSchema};
use model_tile::astc::{decode_rgba8, rgba8_layer_byte_count};
use model_tile::{ModelTileContents, ModelTileElement, ModelTileFormatError};

/// What the worker asks of the texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureMode {
    /// The ASTC blocks as stored, for a device with `texture-compression-astc`.
    Astc,
    /// Decoded to RGBA8, the fallback.
    Rgba8,
}

impl TextureMode {
    pub fn from_byte(mode: u8) -> Result<TextureMode, String> {
        match mode {
            0 => Ok(TextureMode::Astc),
            1 => Ok(TextureMode::Rgba8),
            _ => Err(format!("texture mode {mode} is neither 0 (ASTC) nor 1 (RGBA8)")),
        }
    }
}

/// The texture's bytes: a range of the tile's own, or a decoded array.
pub enum TextureBytes {
    Stored(Range<usize>),
    Decoded(Vec<u8>),
}

pub struct PreparedTile {
    pub meta: String,
    pub vertices: Range<usize>,
    pub indices: Range<usize>,
    pub texture: TextureBytes,
}

/// Decodes and checks a tile fetched for z/x/y and prepares its texture.
pub fn prepare(bytes: &[u8], z: u32, x: u32, y: u32, mode: TextureMode) -> Result<PreparedTile, ModelTileFormatError> {
    let contents = ModelTileContents::decode(bytes)?;
    if (contents.zoom, contents.x, contents.y) != (z, x, y) {
        return Err(ModelTileFormatError::Malformed(format!("the tile says it is {}/{}/{}", contents.zoom, contents.x, contents.y)));
    }
    contents.check_geometry(&bytes[contents.vertex_range.clone()], &bytes[contents.index_range.clone()])?;
    let texture = match mode {
        TextureMode::Astc => TextureBytes::Stored(contents.texture_range.clone()),
        TextureMode::Rgba8 => TextureBytes::Decoded(decode_rgba8(&contents.texture, &bytes[contents.texture_range.clone()])?),
    };
    Ok(PreparedTile {
        meta: meta(&contents, mode),
        vertices: contents.vertex_range.clone(),
        indices: contents.index_range.clone(),
        texture,
    })
}

/// The map tile feature id of a replaced building under the default
/// Protomaps schema, `None` for an element the map's tiles cannot name.
fn tile_feature_id(element: ModelTileElement) -> Option<u64> {
    let element = match element {
        ModelTileElement::Node(id) => OsmElement::Node(id),
        ModelTileElement::Way(id) => OsmElement::Way(id),
        ModelTileElement::Relation(id) => OsmElement::Relation(id),
    };
    ProtomapsBasemapSchema.tile_feature_id(element)
}

/// A float as JSON: its shortest exact decimal, `null` for what JSON has
/// no number for.
struct Number(f64);

impl fmt::Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_finite() { write!(f, "{}", self.0) } else { f.write_str("null") }
    }
}

fn number(value: f32) -> Number {
    // Through the f32's own shortest decimal, so 0.1 stays 0.1 rather than
    // the f64 nearest to the f32.
    Number(value.to_string().parse().unwrap_or(f64::NAN))
}

fn vector(values: &[f32]) -> String {
    let parts: Vec<String> = values.iter().map(|v| number(*v).to_string()).collect();
    format!("[{}]", parts.join(","))
}

/// A string as JSON, quoted and escaped.
fn string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn meta(contents: &ModelTileContents, mode: TextureMode) -> String {
    let texture = &contents.texture;
    let mut levels = Vec::with_capacity(texture.mip_level_count);
    let mut offset = 0;
    for level in 0..texture.mip_level_count {
        let (width, height) = texture.level_size(level);
        let (bytes_per_row, rows_per_image, layer_byte_count) = match mode {
            TextureMode::Astc => (texture.bytes_per_row(level), texture.level_blocks(level).1, texture.layer_byte_count(level)),
            TextureMode::Rgba8 => (width * 4, height, rgba8_layer_byte_count(texture, level)),
        };
        levels.push(format!(
            r#"{{"width":{width},"height":{height},"bytesPerRow":{bytes_per_row},"rowsPerImage":{rows_per_image},"layerByteCount":{layer_byte_count},"byteOffset":{offset}}}"#
        ));
        offset += layer_byte_count * texture.layer_count;
    }
    let format = match mode {
        TextureMode::Astc => "astc",
        TextureMode::Rgba8 => "rgba8",
    };

    let models: Vec<String> = contents
        .models
        .iter()
        .map(|model| {
            format!(
                r#"{{"id":{},"firstIndex":{},"indexCount":{},"boundsMinimum":{},"boundsMaximum":{},"latitude":{},"longitude":{},"altitudeMeters":{}}}"#,
                string(&model.id),
                model.first_index,
                model.index_count,
                vector(&model.bounds_minimum),
                vector(&model.bounds_maximum),
                Number(model.latitude),
                Number(model.longitude),
                number(model.altitude_meters),
            )
        })
        .collect();

    // The replaced buildings of every model by the zoom of the map tiles
    // they are for, each id once.
    let mut replaced: BTreeMap<u32, BTreeSet<u64>> = BTreeMap::new();
    for building in contents.models.iter().flat_map(|m| &m.replaced_buildings) {
        if let Some(id) = tile_feature_id(building.element) {
            replaced.entry(building.map_tile_zoom).or_default().insert(id);
        }
    }
    let replaced: Vec<String> = replaced
        .iter()
        .map(|(zoom, ids)| {
            let ids: Vec<String> = ids.iter().map(u64::to_string).collect();
            format!(r#""{zoom}":[{}]"#, ids.join(","))
        })
        .collect();

    format!(
        r#"{{"zoom":{},"x":{},"y":{},"vertexCount":{},"indexCount":{},"boundsMinimum":{},"boundsMaximum":{},"positionMatrix":{},"texture":{{"format":"{format}","width":{},"height":{},"mipLevelCount":{},"layerCount":{},"blockEdge":{},"levels":[{}]}},"models":[{}],"replacedBuildingIds":{{{}}}}}"#,
        contents.zoom,
        contents.x,
        contents.y,
        contents.vertex_count,
        contents.index_count,
        vector(&contents.bounds_minimum),
        vector(&contents.bounds_maximum),
        vector(&contents.position_matrix()),
        texture.width,
        texture.height,
        texture.mip_level_count,
        texture.layer_count,
        texture.block_edge,
        levels.join(","),
        models.join(","),
        replaced.join(","),
    )
}

#[cfg(test)]
mod tests {
    use model_tile::writer::{Model, Texture};
    use model_tile::{ModelTile, ModelTileVertex, ReplacedBuilding};

    use super::*;

    const SOLID_BLOCK: [u8; 16] = [0xFC, 0xFD, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x80, 0x00, 0x60, 0x00, 0x40, 0xFF, 0xFF];

    /// A tile of one triangle with a 16 pixel texture of two layers, its
    /// model replacing a building at two zooms (one of them twice, and one
    /// id too large for the map's tiles).
    fn tile_bytes() -> Vec<u8> {
        let minimum = [10.0, 20.0, 0.0];
        let maximum = [110.0, 220.0, 50.0];
        let mut vertices = Vec::new();
        for position in [minimum, [110.0, 20.0, 0.0], maximum] {
            let mut vertex = ModelTileVertex::new([0.0, 0.0, 1.0], 1, [0.5, 1.0]);
            vertex.place(position, minimum, maximum);
            vertex.append_to(&mut vertices);
        }
        let levels = [4, 1, 1, 1, 1].map(|blocks| vec![SOLID_BLOCK.repeat(blocks); 2]).to_vec();
        let building = |element, map_tile_zoom| ReplacedBuilding { element, map_tile_zoom };
        let model = Model {
            id: "bolshoi \"theatre\"".to_owned(),
            latitude: 55.7602,
            longitude: 37.6186,
            altitude_meters: 1.5,
            replaced_buildings: vec![
                building(ModelTileElement::Relation(3_334_755), 15),
                building(ModelTileElement::Way(84_653_688), 14),
                building(ModelTileElement::Relation(3_334_755), 15),
                building(ModelTileElement::Way(1 << 50), 15),
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
            texture: Texture { width: 16, height: 16, block_edge: 8, layer_count: 2, levels },
        }
        .serialized()
    }

    #[test]
    fn the_stored_texture_is_described_in_whole_blocks() {
        let bytes = tile_bytes();
        let prepared = prepare(&bytes, 14, 9904, 5121, TextureMode::Astc).unwrap();
        let TextureBytes::Stored(range) = &prepared.texture else { panic!("the ASTC blocks pass through") };
        assert_eq!(range.len(), 2 * 16 * (4 + 1 + 1 + 1 + 1));
        assert!(prepared.meta.contains(r#""format":"astc","width":16,"height":16,"mipLevelCount":5,"layerCount":2,"blockEdge":8"#));
        assert!(prepared.meta.contains(r#"{"width":16,"height":16,"bytesPerRow":32,"rowsPerImage":2,"layerByteCount":64,"byteOffset":0}"#));
        assert!(prepared.meta.contains(r#"{"width":8,"height":8,"bytesPerRow":16,"rowsPerImage":1,"layerByteCount":16,"byteOffset":128}"#));
        assert!(prepared.meta.contains(r#"{"width":1,"height":1,"bytesPerRow":16,"rowsPerImage":1,"layerByteCount":16,"byteOffset":224}"#));
    }

    #[test]
    fn the_decoded_texture_is_described_in_pixels() {
        let bytes = tile_bytes();
        let prepared = prepare(&bytes, 14, 9904, 5121, TextureMode::Rgba8).unwrap();
        let TextureBytes::Decoded(pixels) = &prepared.texture else { panic!("the texture is decoded") };
        assert_eq!(pixels.len(), 2 * 4 * (256 + 64 + 16 + 4 + 1));
        assert!(prepared.meta.contains(r#""format":"rgba8""#));
        assert!(prepared.meta.contains(r#"{"width":16,"height":16,"bytesPerRow":64,"rowsPerImage":16,"layerByteCount":1024,"byteOffset":0}"#));
        assert!(prepared.meta.contains(r#"{"width":8,"height":8,"bytesPerRow":32,"rowsPerImage":8,"layerByteCount":256,"byteOffset":2048}"#));
    }

    #[test]
    fn the_meta_describes_the_tile_and_its_models() {
        let prepared = prepare(&tile_bytes(), 14, 9904, 5121, TextureMode::Astc).unwrap();
        let meta = &prepared.meta;
        assert!(meta.starts_with(r#"{"zoom":14,"x":9904,"y":5121,"vertexCount":3,"indexCount":3,"boundsMinimum":[10,20,0],"boundsMaximum":[110,220,50],"positionMatrix":[100,0,0,0,0,200,0,0,0,0,50,0,10,20,0,1],"#), "{meta}");
        assert!(meta.contains(r#""models":[{"id":"bolshoi \"theatre\"","firstIndex":0,"indexCount":3,"boundsMinimum":[10,20,0],"boundsMaximum":[110,220,50],"latitude":55.7602,"longitude":37.6186,"altitudeMeters":1.5}]"#), "{meta}");
        // Way 84653688 at zoom 14, relation 3334755 once at zoom 15, and
        // the way past the map's ids left out.
        let way = (2u64 << 44) | 84_653_688;
        let relation = (3u64 << 44) | 3_334_755;
        assert!(meta.ends_with(&format!(r#""replacedBuildingIds":{{"14":[{way}],"15":[{relation}]}}}}"#)), "{meta}");
    }

    #[test]
    fn a_tile_of_another_place_is_turned_away() {
        let error = prepare(&tile_bytes(), 14, 9904, 5122, TextureMode::Astc).err().unwrap();
        assert_eq!(error.to_string(), "the tile says it is 14/9904/5121");
    }

    #[test]
    fn a_format_error_keeps_its_message() {
        let mut bytes = tile_bytes();
        bytes[4] = 5;
        let error = prepare(&bytes, 14, 9904, 5121, TextureMode::Astc).err().unwrap();
        assert_eq!(error, ModelTileFormatError::UnsupportedVersion(5));
        assert_eq!(TextureMode::from_byte(2), Err("texture mode 2 is neither 0 (ASTC) nor 1 (RGBA8)".to_owned()));
    }
}
