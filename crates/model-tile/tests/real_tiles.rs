// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Real tiles of the published model archive, from the local fixtures
//! (`scripts/fetch-fixtures.mjs`, decompressed tile bytes): each decodes,
//! its geometry passes the check and its whole texture array decodes to
//! RGBA8, exactly where the colour is in the block. The tests pass without a word when the fixtures are not there,
//! since they never go to the network.

use std::path::PathBuf;

use model_tile::ModelTileContents;
use model_tile::astc::{decode_rgba8, rgba8_byte_count};

fn fixture_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/models")
}

/// The model tiles in the fixtures, smallest first, with their z/x/y from
/// the file name.
fn fixtures() -> Vec<((u32, u32, u32), Vec<u8>)> {
    let Ok(entries) = std::fs::read_dir(fixture_directory()) else { return Vec::new() };
    let mut tiles: Vec<_> = entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_str()?.strip_suffix(".immt")?.to_owned();
            let mut parts = name.split('-').map(|part| part.parse::<u32>().ok());
            let tile = (parts.next()??, parts.next()??, parts.next()??);
            Some((tile, std::fs::read(&path).ok()?))
        })
        .collect();
    tiles.sort_by_key(|(_, bytes)| bytes.len());
    tiles
}

#[test]
fn every_fixture_tile_decodes_and_its_geometry_passes_the_check() {
    let tiles = fixtures();
    if tiles.is_empty() {
        eprintln!("fixtures/models is missing: run scripts/fetch-fixtures.mjs to exercise the real model tiles");
        return;
    }
    for ((z, x, y), bytes) in &tiles {
        let contents = ModelTileContents::decode(bytes).unwrap_or_else(|e| panic!("{z}/{x}/{y}: {e}"));
        assert_eq!((contents.zoom, contents.x, contents.y), (*z, *x, *y), "the tile says where it is");
        assert_eq!(contents.zoom, ModelTileContents::TILE_ZOOM);
        contents
            .check_geometry(&bytes[contents.vertex_range.clone()], &bytes[contents.index_range.clone()])
            .unwrap_or_else(|e| panic!("{z}/{x}/{y}: {e}"));
        assert!(!contents.models.is_empty(), "{z}/{x}/{y} holds models");
        assert_eq!(contents.models.iter().map(|m| m.index_count).sum::<usize>(), contents.index_count, "the models partition the indices");
        for block in [&contents.vertex_range, &contents.index_range, &contents.texture_range] {
            assert_eq!(block.start % ModelTileContents::BLOCK_ALIGNMENT, 0);
        }
    }
}

#[test]
fn every_fixture_tiles_texture_decodes_to_rgba8() {
    let tiles = fixtures();
    if tiles.is_empty() {
        eprintln!("fixtures/models is missing: run scripts/fetch-fixtures.mjs to exercise the real model textures");
    }
    for ((z, x, y), bytes) in tiles {
        check_texture(z, x, y, &bytes);
    }
}

fn check_texture(z: u32, x: u32, y: u32, bytes: &[u8]) {
    let contents = ModelTileContents::decode(bytes).unwrap();
    let texture = contents.texture;
    let pixels = decode_rgba8(&texture, &bytes[contents.texture_range.clone()]).unwrap_or_else(|e| panic!("{z}/{x}/{y}: {e}"));

    assert_eq!(pixels.len(), rgba8_byte_count(&texture));
    // The archive's baker writes its materials as swatches of one colour,
    // ASTC void extent blocks, whose colour is in the block itself: a
    // UNORM16 per channel, of which the decode keeps the top byte. Every
    // pixel of such a block is checked against it, and any other block
    // must not decode to the error colour (a whole block of opaque
    // magenta).
    let edge = texture.block_edge;
    let (across, down) = texture.level_blocks(0);
    let layer_pixels = texture.width * texture.height * 4;
    let mut void_extent_blocks = 0;
    for layer in 0..texture.layer_count {
        let blocks = &bytes[contents.texture_range.start + layer * texture.layer_byte_count(0)..][..texture.layer_byte_count(0)];
        let image = &pixels[layer * layer_pixels..][..layer_pixels];
        for (index, block) in blocks.as_chunks::<16>().0.iter().enumerate() {
            let (block_x, block_y) = (index % across, index / across);
            let block_pixels: Vec<&[u8]> = (0..edge * edge)
                .map(|i| {
                    let at = ((block_y * edge + i / edge) * texture.width + block_x * edge + i % edge) * 4;
                    &image[at..at + 4]
                })
                .collect();
            let is_void_extent_ldr = block[0] == 0xFC && block[1] & 0x03 == 0x01;
            if is_void_extent_ldr {
                void_extent_blocks += 1;
                let colour = [block[9], block[11], block[13], block[15]];
                assert!(block_pixels.iter().all(|p| *p == colour), "{z}/{x}/{y} layer {layer} block {block_x}, {block_y}");
            } else {
                assert!(
                    !block_pixels.iter().all(|p| *p == [0xFF, 0, 0xFF, 0xFF]),
                    "{z}/{x}/{y} layer {layer} block {block_x}, {block_y} decodes to the error colour"
                );
            }
        }
    }
    eprintln!("{z}/{x}/{y}: {void_extent_blocks} of {} blocks of level 0 are void extent", across * down * texture.layer_count);
}
