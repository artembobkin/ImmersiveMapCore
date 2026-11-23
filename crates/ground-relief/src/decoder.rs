// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! A terrain tile's image to its heights, the Swift `GroundReliefDecoder`:
//! a square image whose pixel holds its height in metres in one of the RGB
//! encodings. The settings promise PNG or WebP (Swift reads whatever
//! ImageIO reads), and the format is told by the file's signature. The
//! bytes are numbers, not colours: no colour conversion touches them, and
//! the alpha is ignored.

use std::fmt;
use std::io::Cursor;

/// How a pixel's red, green and blue hold its height
/// (the Swift `TileSettings.GroundReliefEncoding`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// Mapzen Terrarium: `red * 256 + green + blue / 256 - 32768`.
    Terrarium,
    /// Mapbox Terrain-RGB: `(red * 65536 + green * 256 + blue) * 0.1 - 10000`.
    Mapbox,
}

/// A terrain tile decoded: its heights in metres, row by row from the
/// north edge, `size` a side.
#[derive(Clone, Debug, PartialEq)]
pub struct Heights {
    pub size: usize,
    pub metres: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// Not a PNG or a WebP, or one that does not decode.
    UnreadableImage(String),
    NotSquare { width: u32, height: u32 },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::UnreadableImage(reason) => write!(f, "unreadable image ({reason})"),
            DecodeError::NotSquare { width, height } => write!(f, "not square ({width} x {height})"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// The height of one pixel, in metres, as the Swift `GroundReliefDecoder.height`.
#[inline(always)]
pub fn height(red: u8, green: u8, blue: u8, encoding: Encoding) -> f32 {
    let (r, g, b) = (red as f32, green as f32, blue as f32);
    match encoding {
        Encoding::Terrarium => r * 256.0 + g + b / 256.0 - 32768.0,
        Encoding::Mapbox => (r * 65536.0 + g * 256.0 + b) * 0.1 - 10000.0,
    }
}

/// An image's pixels as 8-bit channels, `channels` to a pixel: 1 grey,
/// 2 grey and alpha, 3 RGB, 4 RGBA.
struct Pixels {
    width: u32,
    height: u32,
    channels: usize,
    bytes: Vec<u8>,
}

fn unreadable(error: impl fmt::Display) -> DecodeError {
    DecodeError::UnreadableImage(error.to_string())
}

fn decode_webp(bytes: &[u8]) -> Result<Pixels, DecodeError> {
    let mut decoder = image_webp::WebPDecoder::new(Cursor::new(bytes)).map_err(unreadable)?;
    let (width, height) = decoder.dimensions();
    let channels = if decoder.has_alpha() { 4 } else { 3 };
    let length = decoder.output_buffer_size().ok_or_else(|| unreadable("too large"))?;
    let mut pixels = vec![0u8; length];
    decoder.read_image(&mut pixels).map_err(unreadable)?;
    Ok(Pixels { width, height, channels, bytes: pixels })
}

fn decode_png(bytes: &[u8]) -> Result<Pixels, DecodeError> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    // Palettes and low bit depths expanded, 16 bits to their high byte:
    // every image reads as 8-bit grey or RGB, with or without alpha.
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(unreadable)?;
    let length = reader.output_buffer_size().ok_or_else(|| unreadable("too large"))?;
    let mut pixels = vec![0u8; length];
    let frame = reader.next_frame(&mut pixels).map_err(unreadable)?;
    let channels = match frame.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err(unreadable("palette not expanded")),
    };
    if frame.line_size != frame.width as usize * channels {
        return Err(unreadable("rows not packed"));
    }
    pixels.truncate(frame.buffer_size());
    Ok(Pixels { width: frame.width, height: frame.height, channels, bytes: pixels })
}

/// Decodes a terrain tile, a PNG or a WebP, to its heights.
pub fn decode(bytes: &[u8], encoding: Encoding) -> Result<Heights, DecodeError> {
    let pixels = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        decode_png(bytes)?
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        decode_webp(bytes)?
    } else {
        return Err(unreadable("neither PNG nor WebP"));
    };
    if pixels.width != pixels.height || pixels.width == 0 {
        return Err(DecodeError::NotSquare { width: pixels.width, height: pixels.height });
    }
    let size = pixels.width as usize;
    if pixels.bytes.len() != size * size * pixels.channels {
        return Err(unreadable("short pixel data"));
    }
    let metres = match pixels.channels {
        // Grey is the same value in red, green and blue.
        1 | 2 => pixels.bytes.chunks_exact(pixels.channels).map(|p| height(p[0], p[0], p[0], encoding)).collect(),
        _ => pixels.bytes.chunks_exact(pixels.channels).map(|p| height(p[0], p[1], p[2], encoding)).collect(),
    };
    Ok(Heights { size, metres })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG of the given pixels, row 0 first, RGBA with an opaque alpha.
    fn png(width: u32, height: u32, pixels: &[(u8, u8, u8)]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            let data: Vec<u8> = pixels.iter().flat_map(|&(r, g, b)| [r, g, b, 255]).collect();
            writer.write_image_data(&data).unwrap();
        }
        out
    }

    #[test]
    fn the_terrarium_encoding_reads_metres() {
        assert_eq!(height(128, 0, 0, Encoding::Terrarium), 0.0);
        assert_eq!(height(128, 100, 128, Encoding::Terrarium), 100.5);
        assert_eq!(height(127, 255, 0, Encoding::Terrarium), -1.0);
        // Mapbox: (red * 65536 + green * 256 + blue) / 10 - 10000.
        assert_eq!(height(1, 134, 160, Encoding::Mapbox), 0.0);
        assert_eq!(height(1, 134, 170, Encoding::Mapbox), 1.0);
        assert_eq!(height(0, 0, 0, Encoding::Mapbox), -10000.0);
    }

    #[test]
    fn a_terrain_image_decodes_to_its_heights() {
        let pixels = [(128, 0, 0), (128, 100, 128), (127, 255, 0), (130, 16, 64)];
        let decoded = decode(&png(2, 2, &pixels), Encoding::Terrarium).unwrap();
        assert_eq!(decoded.size, 2);
        assert_eq!(decoded.metres, [0.0, 100.5, -1.0, 528.25]);
    }

    #[test]
    fn a_bordered_tile_decodes_its_plateau_and_its_edge() {
        let mut pixels = Vec::new();
        for row in 0..8 {
            for column in 0..8 {
                let at_edge = row == 0 || row == 7 || column == 0 || column == 7;
                pixels.push(if at_edge { (128, 0, 0) } else { (128, 150, 0) });
            }
        }
        let decoded = decode(&png(8, 8, &pixels), Encoding::Terrarium).unwrap();
        assert_eq!(decoded.size, 8);
        assert_eq!(decoded.metres[0], 0.0);
        assert_eq!(decoded.metres[9], 150.0);
        assert_eq!(decoded.metres[27], 150.0);
        assert_eq!(decoded.metres[63], 0.0);
        assert_eq!(decoded.metres.iter().filter(|&&m| m == 150.0).count(), 36);
    }

    #[test]
    fn a_non_square_image_is_refused() {
        let data = png(2, 1, &[(128, 0, 0), (128, 0, 0)]);
        assert_eq!(decode(&data, Encoding::Terrarium), Err(DecodeError::NotSquare { width: 2, height: 1 }));
    }

    #[test]
    fn bytes_that_are_no_image_are_unreadable() {
        assert!(matches!(decode(b"<html>Not Found</html>", Encoding::Terrarium), Err(DecodeError::UnreadableImage(_))));
        let mut truncated = png(2, 2, &[(1, 2, 3); 4]);
        truncated.truncate(truncated.len() / 2);
        assert!(matches!(decode(&truncated, Encoding::Terrarium), Err(DecodeError::UnreadableImage(_))));
        assert!(matches!(decode(b"RIFF\0\0\0\0WEBPVP8L", Encoding::Terrarium), Err(DecodeError::UnreadableImage(_))));
    }

    #[test]
    fn the_alpha_is_ignored_and_grey_reads_in_every_channel() {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, 1, 1);
            encoder.set_color(png::ColorType::GrayscaleAlpha);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[128, 0]).unwrap();
        }
        assert_eq!(decode(&out, Encoding::Terrarium).unwrap().metres, [128.0 * 256.0 + 128.0 + 0.5 - 32768.0]);
    }

    #[test]
    fn the_mapterhorn_fixtures_decode_to_alpine_heights() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/terrain");
        let Ok(entries) = std::fs::read_dir(&directory) else {
            // The fixtures are fetched once by scripts/fetch-fixtures.mjs and are not in git.
            eprintln!("no fixtures at {}: run `node scripts/fetch-fixtures.mjs`", directory.display());
            return;
        };
        let mut files: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "webp")).collect();
        files.sort();
        assert_eq!(files.len(), 27);
        let (mut lowest, mut highest) = (f32::MAX, f32::MIN);
        for file in &files {
            let decoded = decode(&std::fs::read(file).unwrap(), Encoding::Terrarium).unwrap();
            assert_eq!(decoded.size, 512, "{}", file.display());
            assert_eq!(decoded.metres.len(), 512 * 512);
            let (low, high) = crate::height_range(&decoded.metres);
            lowest = lowest.min(low);
            highest = highest.max(high);
        }
        // Zermatt: valleys above 500 m, the Monte Rosa and the Matterhorn near 4500 m.
        assert!(lowest > 200.0 && lowest < 1500.0, "lowest {lowest}");
        assert!(highest > 4000.0 && highest < 4900.0, "highest {highest}");
    }
}
