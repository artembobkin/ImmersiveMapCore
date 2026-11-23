// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The label text of ImmersiveMap, the data half of the Swift engine's
//! `Text` folder: the atlas metrics ([`atlas`]) and the layout of a label's
//! text into glyph quads ([`layout`]): measuring, line breaking, alignment
//! and tracking. No schema-specific name or language rules, no label
//! placement, no GPU.
//!
//! The atlases are the package's own (`packages/immersive-map/assets/atlas.json`
//! and `atlas_thin.json`), bundled into the crate the way the Swift engine
//! bundles them as resources of `TextRenderer`: the same file a page draws
//! the glyphs from is the one the tiles are laid out with.

pub mod atlas;
pub mod layout;

use std::sync::{Arc, OnceLock};

pub use atlas::{AtlasData, FontAtlas};
pub use layout::{LabelLayoutRequest, LabelTextAlignment, LabelVertex, LabelWrapOptions, TextLayout, TextMetrics};

/// The revision of how text is laid out into a prepared tile (the Swift
/// `TextRenderer.preparedTileTextRevisionValue`): part of the prepared
/// cache's identity, bumped whenever the layout or the bundled atlas
/// metrics change what a tile bakes.
/// 7: glyph quads are baked in layout points rather than device pixels.
pub const PREPARED_TILE_TEXT_REVISION: u32 = 7;

const BOLD_ATLAS_JSON: &str = include_str!("../assets/atlas.json");
const THIN_ATLAS_JSON: &str = include_str!("../assets/atlas_thin.json");

/// The layout over the package's atlases, read once per process.
pub fn bundled() -> Arc<TextLayout> {
    static LAYOUT: OnceLock<Arc<TextLayout>> = OnceLock::new();
    LAYOUT
        .get_or_init(|| {
            let bold = AtlasData::from_json(BOLD_ATLAS_JSON).expect("the bundled atlas.json is an atlas");
            let thin = AtlasData::from_json(THIN_ATLAS_JSON).expect("the bundled atlas_thin.json is an atlas");
            Arc::new(TextLayout::new(bold, thin))
        })
        .clone()
}

#[cfg(test)]
mod tests;
