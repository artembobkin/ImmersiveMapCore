// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Where each POI sprite sits in the sprite atlas (the Swift
//! `PoiSpriteAtlasLayout` and `PoiSpriteAtlasDescriptor.default`): square
//! cells in the order the icons are declared, row by row from the top
//! left. Pure layout: the page that draws the sprites rasterizes them into
//! the same cells.

use map_style::style::PoiSpriteIcon;

/// The cell grid of the atlas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoiSpriteAtlasDescriptor {
    /// A cell's side in texels: a texture resolution, not a layout size.
    pub cell_size: u32,
    /// The empty border inside a cell, in texels.
    pub cell_padding: u32,
    pub minimum_columns: u32,
}

impl PoiSpriteAtlasDescriptor {
    /// 128 texel cells with 16 of padding: 96 drawable texels cover a 12
    /// point POI label's icon at 3x with a gentle magnification.
    pub const DEFAULT: PoiSpriteAtlasDescriptor = PoiSpriteAtlasDescriptor { cell_size: 128, cell_padding: 16, minimum_columns: 4 };
}

/// One icon's cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PoiSpriteAtlasRegion {
    pub index: usize,
    /// (x, y, width, height) in texels, y down from the top.
    pub pixel_rect: [u32; 4],
    /// (min u, min v, max u, max v).
    pub uv_rect: [f32; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub struct PoiSpriteAtlasLayout {
    pub icons: Vec<PoiSpriteIcon>,
    pub descriptor: PoiSpriteAtlasDescriptor,
    pub cells_per_side: u32,
    /// The atlas's side in texels.
    pub pixel_size: u32,
    regions: Vec<PoiSpriteAtlasRegion>,
}

impl Default for PoiSpriteAtlasLayout {
    /// Every icon, in its declared order, in the default cells.
    fn default() -> Self {
        PoiSpriteAtlasLayout::new(PoiSpriteIcon::ALL.to_vec(), PoiSpriteAtlasDescriptor::DEFAULT)
    }
}

impl PoiSpriteAtlasLayout {
    /// The layout of some icons. Panics on no icons or a padding that leaves
    /// no drawable texels, as the Swift preconditions do.
    pub fn new(icons: Vec<PoiSpriteIcon>, descriptor: PoiSpriteAtlasDescriptor) -> Self {
        assert!(!icons.is_empty(), "POI sprite atlas requires at least one icon.");
        assert!(descriptor.cell_size > descriptor.cell_padding * 2, "POI sprite atlas padding must leave drawable pixels inside each cell.");
        let required_side = (icons.len() as f64).sqrt().ceil() as u32;
        let cells_per_side = descriptor.minimum_columns.max(required_side);
        let pixel_size = cells_per_side * descriptor.cell_size;
        let inv_size = 1.0 / pixel_size as f32;
        let cell = descriptor.cell_size;
        let regions = (0..icons.len())
            .map(|index| {
                let (x, y) = ((index as u32 % cells_per_side) * cell, (index as u32 / cells_per_side) * cell);
                PoiSpriteAtlasRegion {
                    index,
                    pixel_rect: [x, y, cell, cell],
                    uv_rect: [x as f32 * inv_size, y as f32 * inv_size, (x + cell) as f32 * inv_size, (y + cell) as f32 * inv_size],
                }
            })
            .collect();
        PoiSpriteAtlasLayout { icons, descriptor, cells_per_side, pixel_size, regions }
    }

    pub fn region(&self, icon: PoiSpriteIcon) -> Option<&PoiSpriteAtlasRegion> {
        self.icons.iter().position(|i| *i == icon).map(|i| &self.regions[i])
    }
}

/// The disc's side relative to an icon's: the plain marker is a small dot,
/// lighter than the POIs whose symbol says what they are.
pub fn disc_scale(icon: PoiSpriteIcon) -> f32 {
    match icon {
        PoiSpriteIcon::Marker => 0.42,
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_takes_its_own_cell_of_a_five_by_five_atlas() {
        let layout = PoiSpriteAtlasLayout::default();
        assert_eq!(layout.cells_per_side, 5);
        assert_eq!(layout.pixel_size, 640);
        let first = layout.region(PoiSpriteIcon::Restaurant).unwrap();
        assert_eq!(first.uv_rect, [0.0, 0.0, 0.2, 0.2]);
        let marker = layout.region(PoiSpriteIcon::Marker).unwrap();
        assert_eq!(marker.index, 22);
        assert_eq!(marker.pixel_rect, [256, 512, 128, 128]);
        let mut cells: Vec<[u32; 4]> = PoiSpriteIcon::ALL.iter().map(|i| layout.region(*i).unwrap().pixel_rect).collect();
        cells.dedup();
        assert_eq!(cells.len(), PoiSpriteIcon::ALL.len());
    }

    #[test]
    fn a_small_icon_set_keeps_the_minimum_columns() {
        let layout = PoiSpriteAtlasLayout::new(vec![PoiSpriteIcon::Cafe, PoiSpriteIcon::Bar], PoiSpriteAtlasDescriptor::DEFAULT);
        assert_eq!(layout.cells_per_side, 4);
        assert!(layout.region(PoiSpriteIcon::Park).is_none());
        assert_eq!(layout.region(PoiSpriteIcon::Bar).unwrap().uv_rect, [0.25, 0.0, 0.5, 0.25]);
    }

    #[test]
    fn only_the_marker_draws_a_smaller_disc() {
        assert_eq!(disc_scale(PoiSpriteIcon::Marker), 0.42);
        assert_eq!(disc_scale(PoiSpriteIcon::Museum), 1.0);
    }
}
