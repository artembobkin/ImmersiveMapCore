// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The fallback for a GPU without ASTC: a tile's whole texture array, ASTC
//! LDR blocks as the format stores them, decoded to RGBA8 on the CPU with
//! the `astc-decode` crate (Apache-2.0, pure Rust).
//!
//! The channel values are the raw ones, as the format says: no sRGB decode.
//! The decoded array keeps the stored order, mip level 0 first, all layers
//! of a level one after another, then the next level, and a layer of a
//! level of `w` by `h` pixels is `w * h * 4` bytes, rows top first with no
//! padding. A level smaller than a block is decoded as its one block and
//! cropped, as is the right and bottom edge of any level whose size is not
//! a multiple of the block edge.
//!
//! A block the decoder finds illegal decodes to the ASTC error colour,
//! opaque magenta, as a GPU decodes it: the texture block has the right
//! size, so the tile stays drawable and the fault shows on the model.

use astc_decode::{Footprint, astc_decode_block};

use crate::ModelTileFormatError;
use crate::contents::Texture;

/// The bytes one layer of a mip level takes as RGBA8.
pub fn rgba8_layer_byte_count(texture: &Texture, level: usize) -> usize {
    let (width, height) = texture.level_size(level);
    width * height * 4
}

/// The bytes of the whole array as RGBA8: every layer of every level.
pub fn rgba8_byte_count(texture: &Texture) -> usize {
    (0..texture.mip_level_count).map(|level| rgba8_layer_byte_count(texture, level) * texture.layer_count).sum()
}

/// Decodes the texture block of a tile (`blocks`, the bytes of
/// `ModelTileContents::texture_range`) described by `texture` to RGBA8, in
/// the layout of the module's comment.
pub fn decode_rgba8(texture: &Texture, blocks: &[u8]) -> Result<Vec<u8>, ModelTileFormatError> {
    let mut pixels = vec![0; rgba8_byte_count(texture)];
    decode_rgba8_into(texture, blocks, &mut pixels)?;
    Ok(pixels)
}

/// `decode_rgba8` into a buffer of `rgba8_byte_count` bytes.
pub fn decode_rgba8_into(texture: &Texture, blocks: &[u8], pixels: &mut [u8]) -> Result<(), ModelTileFormatError> {
    if blocks.len() != texture.byte_count() || pixels.len() != rgba8_byte_count(texture) {
        return Err(ModelTileFormatError::Truncated);
    }
    let edge = texture.block_edge;
    let footprint = Footprint::new(edge as u32, edge as u32);
    let mut blocks = blocks.as_chunks::<16>().0.iter();
    let mut target = 0;
    for level in 0..texture.mip_level_count {
        let (width, height) = texture.level_size(level);
        let (across, down) = texture.level_blocks(level);
        for _ in 0..texture.layer_count {
            let image = &mut pixels[target..target + width * height * 4];
            for block_y in 0..down {
                for block_x in 0..across {
                    let block = blocks.next().ok_or(ModelTileFormatError::Truncated)?;
                    let (left, top) = (block_x * edge, block_y * edge);
                    astc_decode_block(block, footprint, |x, y, colour| {
                        let (x, y) = (left + x as usize, top + y as usize);
                        if x < width && y < height {
                            let at = (y * width + x) * 4;
                            image[at..at + 4].copy_from_slice(&colour);
                        }
                    });
                }
            }
            target += width * height * 4;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A void extent block: one colour over the whole block, RGBA
    /// 0x8000, 0x6000, 0x4000, 0xFFFF as UNORM16, so its decode is exact.
    const SOLID_BLOCK: [u8; 16] = [0xFC, 0xFD, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x80, 0x00, 0x60, 0x00, 0x40, 0xFF, 0xFF];

    fn texture(size: usize, layers: usize) -> Texture {
        Texture { width: size, height: size, mip_level_count: size.ilog2() as usize + 1, block_edge: 8, layer_count: layers }
    }

    #[test]
    fn a_void_extent_block_decodes_to_its_colour_on_every_level_and_layer() {
        let texture = texture(16, 2);
        let blocks: Vec<u8> = (0..texture.byte_count() / 16).flat_map(|_| SOLID_BLOCK).collect();
        let pixels = decode_rgba8(&texture, &blocks).unwrap();

        // 16, 8, 4, 2 and 1 pixels: the levels under a block are cropped.
        assert_eq!(pixels.len(), 2 * 4 * (256 + 64 + 16 + 4 + 1));
        assert!(pixels.as_chunks::<4>().0.iter().all(|pixel| *pixel == [0x80, 0x60, 0x40, 0xFF]));
    }

    #[test]
    fn a_level_keeps_each_block_in_its_place() {
        // 16 by 16 pixels in four 8x8 blocks: the top right one is another
        // colour, so a block lands at its row and column.
        let texture = Texture { width: 16, height: 16, mip_level_count: 1, block_edge: 8, layer_count: 1 };
        let mut other = SOLID_BLOCK;
        other[8..10].copy_from_slice(&0x2000u16.to_le_bytes());
        let blocks: Vec<u8> = [SOLID_BLOCK, other, SOLID_BLOCK, SOLID_BLOCK].concat();
        let pixels = decode_rgba8(&texture, &blocks).unwrap();

        let red = |x: usize, y: usize| pixels[(y * 16 + x) * 4];
        assert_eq!(red(7, 0), 0x80);
        assert_eq!(red(8, 0), 0x20);
        assert_eq!(red(15, 7), 0x20);
        assert_eq!(red(15, 8), 0x80);
    }

    #[test]
    fn a_texture_block_of_another_size_is_turned_away() {
        let texture = texture(16, 1);
        let blocks = vec![0; texture.byte_count() - 16];
        assert_eq!(decode_rgba8(&texture, &blocks), Err(ModelTileFormatError::Truncated));
    }

    /// The bytes of a tile are untrusted: whatever a block holds, the
    /// decode ends with a colour for every pixel and does not panic.
    #[test]
    fn any_block_decodes_without_a_panic() {
        let texture = Texture { width: 64, height: 64, mip_level_count: 1, block_edge: 8, layer_count: 1 };
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..200 {
            let blocks: Vec<u8> = (0..texture.byte_count())
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    (state >> 24) as u8
                })
                .collect();
            assert_eq!(decode_rgba8(&texture, &blocks).unwrap().len(), 64 * 64 * 4);
        }
    }
}
