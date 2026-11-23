// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use std::collections::HashSet;

/// Which characters the text atlases can draw: a name with any other
/// character (whitespace aside) is not drawn in that spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphCoverage {
    supported_scalars: HashSet<u32>,
}

impl GlyphCoverage {
    pub fn new(supported_scalars: impl IntoIterator<Item = u32>) -> Self {
        GlyphCoverage { supported_scalars: supported_scalars.into_iter().collect() }
    }

    /// The glyphs of the bold and the thin atlas together (the engine's
    /// `init(atlasData:thinAtlasData:)`, given the atlases' glyph
    /// unicodes).
    pub fn from_atlases(bold: impl IntoIterator<Item = u32>, thin: impl IntoIterator<Item = u32>) -> Self {
        GlyphCoverage::new(bold.into_iter().chain(thin))
    }

    /// The ASCII letters, the digits, the ASCII punctuation and the Cyrillic
    /// block: the atlas the tests were written against.
    pub fn legacy_atlas_for_tests() -> Self {
        let ascii = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*()_+-=[]{}|;:'\\\",./<>?";
        GlyphCoverage::new(ascii.chars().map(|c| c as u32).chain(0x0400..=0x04FF))
    }

    /// Whether every character of the text is drawn by the atlases or is
    /// whitespace.
    pub fn can_render(&self, text: &str) -> bool {
        text.chars().all(|c| self.supported_scalars.contains(&(c as u32)) || c.is_whitespace())
    }
}
