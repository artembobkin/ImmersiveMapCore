// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The tile grid's frame, the logic of the Swift engine's
//! `TileGridRenderSubsystem` and `TileGridLabels`: from the camera, the
//! settings and the selected tiles to what the GPU draws, in one call a
//! frame. The grid's own tiles in view, the nearest first
//! (`map_visibility::grid`), the selected among them, the selected tiles of
//! every other zoom in view, and the glyphs of the tiles' texts.
//!
//! What a host reads back is flat and ready for its buffers:
//!
//! - the tiles (`TileGrid::instances`), `TILE_WORDS` 32-bit words each: the
//!   south-west corner and the size in the flat render world, the world x
//!   the unroll unwraps the tile around, the world uv of its north-west
//!   corner and its size there, six floats and one more, then the flags.
//!   They come in three runs: the grid's tiles that are not selected, the
//!   grid's selected tiles, and the selected tiles of other zooms, drawn as
//!   their fill alone (`Frame`);
//! - the tiles' numbers in the same order (`TileGrid::tiles`), four ints
//!   each: zoom, x, y and the copy of the world;
//! - the glyphs (`TileGrid::glyphs`), `GLYPH_WORDS` words each: the tile's
//!   index in the frame's tiles in the low half of the first word and the
//!   glyph's index in the host's glyph table in the high half, then the pen
//!   from the text's middle in ems, x right and y up, and the share of the
//!   tile's side one em takes.
//!
//! The grid knows nothing of fonts. A host lays a text out once
//! (`define_layout`) and says which text a tile carries (`set_tile_label`),
//! when the grid asks (`label_misses`): the tiles in view whose text it has
//! not been told.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

use map_geo::frustum::Frustum;
use map_geo::{FlatRenderState, GlobeUniform};
use map_visibility::VisibleTile;
use map_visibility::globe::{GlobeBounds, Visibility};
use map_visibility::grid::{self, GlobeView, MAXIMUM_ZOOM, Resolution, Walk, ZoomSettings};
use map_visibility::polygon::footprint;

/// 32-bit words a tile of the frame.
pub const TILE_WORDS: usize = 8;
/// 32-bit words a glyph of the frame.
pub const GLYPH_WORDS: usize = 4;
/// A tile's flags: it is selected.
pub const FLAG_SELECTED: u32 = 1;
/// A tile's flags: its fill alone is drawn, no border (a selected tile of another zoom).
pub const FLAG_FILL_ONLY: u32 = 2;
/// A tile's flags carry how finely it is divided from this bit up, as the power of two.
pub const SUBDIVISION_SHIFT: u32 = 8;
/// What `set_tile_label` takes for a tile with no text.
pub const NO_LABEL: u32 = u32::MAX;
/// The tiles a glyph can name: the index's room in a glyph's first word.
const GLYPH_TILE_LIMIT: usize = 1 << 16;

/// What the grid reads of the map's settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileGridSettings {
    pub zoom: ZoomSettings,
    /// The most tiles the grid draws in a frame, those nearest to where the camera looks first.
    pub maximum_drawn_tiles: usize,
    /// The widest a text may be, as a share of the tile's width.
    pub label_maximum_width_share: f32,
    /// The tallest a text may be, as a share of the tile's height.
    pub label_maximum_height_share: f32,
}

impl Default for TileGridSettings {
    fn default() -> TileGridSettings {
        TileGridSettings {
            zoom: ZoomSettings { zoom_range: (0, MAXIMUM_ZOOM), minimum_tile_screen_points: 48.0, label_minimum_tile_screen_points: 96.0 },
            maximum_drawn_tiles: 512,
            label_maximum_width_share: 0.7,
            label_maximum_height_share: 0.12,
        }
    }
}

/// What a frame's grid is resolved from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameInput {
    /// The frame's projection-view matrix, column-major.
    pub camera_matrix: [f64; 16],
    pub flat: FlatRenderState,
    /// The globe and the eye in world units on the sphere and through the unroll, `None` on the flat map.
    pub globe: Option<(GlobeUniform, [f64; 3])>,
    /// The point the camera looks at, in the normalized world Mercator.
    pub centre: [f64; 2],
    /// The camera's own zoom, not the one the map is planned by: the grid's threshold is about what is on screen.
    pub camera_zoom: f64,
    pub viewport_height_points: f64,
    /// The size in the flat render world of a tile of the camera's integer zoom.
    pub tile_world_size: f64,
}

/// What a frame's grid came to.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Frame {
    /// The zoom the grid shows, `None` for a frame with no grid.
    pub resolution: Option<Resolution>,
    /// The grid's tiles that are not selected: the first run of the tiles.
    pub plain_count: usize,
    /// The grid's selected tiles: the run after them.
    pub selected_count: usize,
    /// The selected tiles of other zooms, as their fill alone: the last run.
    pub other_count: usize,
    /// How finely the grid's own tiles are divided, in quads a side.
    pub grid_subdivision: u32,
    /// How finely the tiles of the last run are divided: by the coarsest zoom among them.
    pub other_subdivision: u32,
}

impl Frame {
    pub fn tile_count(&self) -> usize {
        self.plain_count + self.selected_count + self.other_count
    }
}

/// A hasher for a tile's number, which is spread already by one multiply.
#[derive(Default)]
pub struct TileHasher(u64);

impl Hasher for TileHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0 ^ byte as u64).wrapping_mul(0x0000_0100_0000_01B3);
        }
    }

    fn write_u64(&mut self, value: u64) {
        let mixed = value.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.0 = mixed ^ (mixed >> 29);
    }
}

type TileBuild = BuildHasherDefault<TileHasher>;

/// A tile in one number: five bits of zoom over twice twenty-nine of x and
/// y, room for every zoom a tile is named at.
pub fn tile_key(z: i32, x: i32, y: i32) -> u64 {
    (z as u64) << 58 | (x as u64) << 29 | y as u64
}

/// A text laid out round its own middle.
#[derive(Debug, Clone, Default)]
struct Layout {
    width_em: f32,
    height_em: f32,
    /// The glyphs as the frame takes them, with no tile and no share yet.
    records: Vec<u32>,
}

/// The share of a tile's side one em of a text takes, the largest at which
/// the text keeps within its shares of the tile's width and height. Zero
/// for a text with no extent.
pub fn tile_share_per_em(width_em: f32, height_em: f32, width_share: f32, height_share: f32) -> f32 {
    // A width or a height that is no number has no extent either.
    if width_em.is_nan() || height_em.is_nan() || width_em <= 0.0 || height_em <= 0.0 {
        return 0.0;
    }
    (width_share / width_em).min(height_share / height_em).max(0.0)
}

/// The grid of frame after frame: the selection, the texts, and what the
/// last frame drew.
#[derive(Debug, Default)]
pub struct TileGrid {
    settings: TileGridSettings,
    /// The selected tiles, zoom by zoom.
    selected: Vec<HashSet<u64, TileBuild>>,
    layouts: Vec<Layout>,
    tile_labels: HashMap<u64, u32, TileBuild>,
    /// Moves with everything a frame reads besides its input.
    revision: u64,
    last: Option<(FrameInput, u64)>,
    frame: Frame,
    walk: Walk,
    bounds: GlobeBounds,
    visible: Vec<VisibleTile>,
    others: Vec<VisibleTile>,
    tiles: Vec<i32>,
    instances: Vec<u32>,
    glyphs: Vec<u32>,
    misses: Vec<i32>,
}

impl TileGrid {
    pub fn new() -> TileGrid {
        TileGrid { selected: (0..=MAXIMUM_ZOOM).map(|_| HashSet::default()).collect(), ..TileGrid::default() }
    }

    pub fn settings(&self) -> &TileGridSettings {
        &self.settings
    }

    pub fn set_settings(&mut self, mut settings: TileGridSettings) {
        settings.zoom.zoom_range = grid::clamped_zoom_range(settings.zoom.zoom_range);
        if settings != self.settings {
            self.settings = settings;
            self.revision += 1;
        }
    }

    // MARK: The selection

    /// Replaces the selected tiles: three ints each, zoom, x and y. A tile
    /// outside the zooms a tile is named at is left out.
    pub fn set_selection(&mut self, tiles: &[i32]) {
        if self.selected.is_empty() {
            self.selected = (0..=MAXIMUM_ZOOM).map(|_| HashSet::default()).collect();
        }
        for set in &mut self.selected {
            set.clear();
        }
        for tile in tiles.as_chunks::<3>().0 {
            let [z, x, y] = *tile;
            if (0..=MAXIMUM_ZOOM).contains(&z) && x >= 0 && y >= 0 {
                self.selected[z as usize].insert(tile_key(z, x, y));
            }
        }
        self.revision += 1;
    }

    pub fn selected_count(&self) -> usize {
        self.selected.iter().map(HashSet::len).sum()
    }

    fn is_selected(&self, z: i32, x: i32, y: i32) -> bool {
        self.selected.get(z as usize).is_some_and(|set| !set.is_empty() && set.contains(&tile_key(z, x, y)))
    }

    // MARK: The texts

    /// A text laid out: its width and height in ems and its glyphs,
    /// `GLYPH_WORDS` words each as a frame's glyph is, the index of the
    /// tile and the share left zero. Returns the text's handle.
    pub fn define_layout(&mut self, width_em: f32, height_em: f32, records: &[u32]) -> u32 {
        let mut records = records[..records.len() - records.len() % GLYPH_WORDS].to_vec();
        for record in records.as_chunks_mut::<GLYPH_WORDS>().0 {
            record[0] &= 0xFFFF_0000;
        }
        self.layouts.push(Layout { width_em, height_em, records });
        self.layouts.len() as u32 - 1
    }

    /// How many texts are laid out.
    pub fn layout_count(&self) -> usize {
        self.layouts.len()
    }

    /// The text a tile carries: a handle of `define_layout`, or `NO_LABEL`.
    pub fn set_tile_label(&mut self, z: i32, x: i32, y: i32, layout: u32) {
        self.tile_labels.insert(tile_key(z, x, y), layout);
    }

    /// How many tiles the grid has been told the text of.
    pub fn tile_label_count(&self) -> usize {
        self.tile_labels.len()
    }

    /// Forgets which text each tile carries: the tiles in view are asked
    /// again. The texts laid out are kept.
    pub fn clear_tile_labels(&mut self) {
        self.tile_labels.clear();
        self.revision += 1;
    }

    /// Forgets the texts laid out, and with them which a tile carries.
    pub fn clear_layouts(&mut self) {
        self.layouts.clear();
        self.clear_tile_labels();
    }

    // MARK: The frame

    /// The frame's grid. The second value is false when nothing the frame
    /// reads has changed since the last one, and what was read back then
    /// still stands.
    pub fn resolve(&mut self, input: &FrameInput) -> (Frame, bool) {
        if self.last == Some((*input, self.revision)) {
            return (self.frame, false);
        }
        self.last = Some((*input, self.revision));
        self.visible.clear();
        self.others.clear();
        self.tiles.clear();
        self.instances.clear();
        self.glyphs.clear();
        self.misses.clear();
        self.frame = Frame::default();

        let base = grid::base_tile_screen_points(input.viewport_height_points, input.tile_world_size);
        let Some(resolution) = grid::resolve_zoom(input.camera_zoom, base, &self.settings.zoom) else {
            return (self.frame, true);
        };
        self.frame.resolution = Some(resolution);
        let zoom = resolution.zoom;
        let limit = self.settings.maximum_drawn_tiles;
        let is_flat = input.globe.is_none();
        let mut other_coarsest = MAXIMUM_ZOOM;

        match input.globe {
            None => {
                // No footprint, no ground in view.
                let Some(polygon) = footprint(&input.camera_matrix, None) else {
                    return (self.frame, true);
                };
                grid::flat(&mut self.walk, zoom, limit, &polygon, &input.flat, input.centre, &mut self.visible);
                'others: for (z, set) in self.selected.iter().enumerate() {
                    if z as i32 == zoom {
                        continue;
                    }
                    for &key in set {
                        if self.others.len() >= limit {
                            break 'others;
                        }
                        if grid::flat_copies(tile_of(key), &polygon, &input.flat, &mut self.others) > 0 {
                            other_coarsest = other_coarsest.min(z as i32);
                        }
                    }
                }
            }
            Some((globe, eye)) => {
                let frustum = Frustum::new(&input.camera_matrix);
                let view = GlobeView { visibility: Visibility::new(&globe, eye), frustum: &frustum };
                grid::globe(&mut self.walk, zoom, limit, &view, &mut self.bounds, input.centre, &mut self.visible);
                'others: for (z, set) in self.selected.iter().enumerate() {
                    if z as i32 == zoom {
                        continue;
                    }
                    for &key in set {
                        if self.others.len() >= limit {
                            break 'others;
                        }
                        let (x, y, z) = tile_of(key);
                        if grid::globe_shows((x, y, z), &view, &mut self.bounds) {
                            self.others.push(VisibleTile { z, x, y, wrap: 0 });
                            other_coarsest = other_coarsest.min(z);
                        }
                    }
                }
            }
        }

        // The grid's tiles, those not selected first: each run is one draw.
        let grid_subdivision = grid::subdivision(zoom, is_flat);
        let grid_flags = grid_subdivision.trailing_zeros() << SUBDIVISION_SHIFT;
        let any_selected = self.selected.get(zoom as usize).is_some_and(|set| !set.is_empty());
        self.tiles.reserve((self.visible.len() + self.others.len()) * 4);
        self.instances.reserve((self.visible.len() + self.others.len()) * TILE_WORDS);
        let mut selected_count = 0;
        for pass in [false, true] {
            if pass && !any_selected {
                break;
            }
            for index in 0..self.visible.len() {
                let tile = self.visible[index];
                let is_selected = any_selected && self.is_selected(tile.z, tile.x, tile.y);
                if is_selected != pass {
                    continue;
                }
                selected_count += is_selected as usize;
                push_tile(&mut self.tiles, &mut self.instances, tile, &input.flat, grid_flags | if is_selected { FLAG_SELECTED } else { 0 });
            }
        }
        let other_subdivision = grid::subdivision(other_coarsest, is_flat);
        let other_flags = FLAG_SELECTED | FLAG_FILL_ONLY | other_subdivision.trailing_zeros() << SUBDIVISION_SHIFT;
        for index in 0..self.others.len() {
            push_tile(&mut self.tiles, &mut self.instances, self.others[index], &input.flat, other_flags);
        }
        self.frame.plain_count = self.visible.len() - selected_count;
        self.frame.selected_count = selected_count;
        self.frame.other_count = self.others.len();
        self.frame.grid_subdivision = grid_subdivision;
        self.frame.other_subdivision = other_subdivision;
        self.lay_out_labels();
        (self.frame, true)
    }

    /// Lays the texts of the last frame's tiles out again: after the host
    /// has answered `label_misses`. Returns the number of glyphs.
    pub fn lay_out_labels(&mut self) -> usize {
        self.glyphs.clear();
        self.misses.clear();
        let Some(resolution) = self.frame.resolution else { return 0 };
        if !resolution.draws_labels {
            return 0;
        }
        let count = (self.frame.plain_count + self.frame.selected_count).min(GLYPH_TILE_LIMIT);
        let (width_share, height_share) = (self.settings.label_maximum_width_share, self.settings.label_maximum_height_share);
        for index in 0..count {
            let [z, x, y, _] = *self.tiles[index * 4..].first_chunk::<4>().expect("four ints a tile");
            let Some(&handle) = self.tile_labels.get(&tile_key(z, x, y)) else {
                self.misses.extend_from_slice(&[z, x, y]);
                continue;
            };
            let Some(layout) = self.layouts.get(handle as usize) else { continue };
            let share = tile_share_per_em(layout.width_em, layout.height_em, width_share, height_share);
            if share <= 0.0 {
                continue;
            }
            let first = self.glyphs.len();
            self.glyphs.extend_from_slice(&layout.records);
            let share_bits = share.to_bits();
            for record in self.glyphs[first..].as_chunks_mut::<GLYPH_WORDS>().0 {
                record[0] |= index as u32;
                record[3] = share_bits;
            }
        }
        self.glyphs.len() / GLYPH_WORDS
    }

    /// The last frame's result.
    pub fn frame(&self) -> &Frame {
        &self.frame
    }

    /// The last frame's tiles as the GPU takes them, `TILE_WORDS` words each.
    pub fn instances(&self) -> &[u32] {
        &self.instances
    }

    /// The last frame's tiles by their numbers, in the order of `instances`: zoom, x, y and the copy of the world.
    pub fn tiles(&self) -> &[i32] {
        &self.tiles
    }

    /// The last frame's glyphs, `GLYPH_WORDS` words each.
    pub fn glyphs(&self) -> &[u32] {
        &self.glyphs
    }

    /// The tiles in view whose text the grid has not been told, three ints each: zoom, x and y.
    pub fn label_misses(&self) -> &[i32] {
        &self.misses
    }
}

fn tile_of(key: u64) -> (i32, i32, i32) {
    const MASK: u64 = (1 << 29) - 1;
    (((key >> 29) & MASK) as i32, (key & MASK) as i32, (key >> 58) as i32)
}

fn push_tile(tiles: &mut Vec<i32>, instances: &mut Vec<u32>, tile: VisibleTile, flat: &FlatRenderState, flags: u32) {
    let [origin_x, origin_y, size] = flat.tile_origin_and_size(tile.x, tile.y, tile.z, tile.wrap);
    let scale = 1.0 / (1u64 << tile.z) as f64;
    tiles.extend_from_slice(&[tile.z, tile.x, tile.y, tile.wrap]);
    instances.extend_from_slice(&[
        (origin_x as f32).to_bits(),
        (origin_y as f32).to_bits(),
        (size as f32).to_bits(),
        // The world x the unroll unwraps the tile around: its centre.
        (((tile.x as f64 + 0.5) * scale) as f32).to_bits(),
        ((tile.x as f64 * scale) as f32).to_bits(),
        ((tile.y as f64 * scale) as f32).to_bits(),
        (scale as f32).to_bits(),
        flags,
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A camera straight above the middle of a flat world `size` units wide, seeing a square `half` units each way.
    fn above(half: f64, size: f64) -> FrameInput {
        // An orthographic matrix over the square, the depth 0 to 1 across z -1 to 1.
        let matrix = [1.0 / half, 0.0, 0.0, 0.0, 0.0, 1.0 / half, 0.0, 0.0, 0.0, 0.0, -0.5, 0.0, 0.0, 0.0, 0.5, 1.0];
        FrameInput { camera_matrix: matrix, flat: FlatRenderState { pan: [0.0, 0.0], render_map_size: size }, globe: None, centre: [0.5, 0.5], camera_zoom: 2.0, viewport_height_points: 800.0, tile_world_size: size / 4.0 }
    }

    fn grid(range: (i32, i32)) -> TileGrid {
        let mut grid = TileGrid::new();
        grid.set_settings(TileGridSettings { zoom: ZoomSettings { zoom_range: range, minimum_tile_screen_points: 1.0, label_minimum_tile_screen_points: 1.0 }, ..TileGridSettings::default() });
        grid
    }

    fn tiles_of(grid: &TileGrid, range: std::ops::Range<usize>) -> Vec<(i32, i32, i32)> {
        grid.tiles().as_chunks::<4>().0[range].iter().map(|t| (t[0], t[1], t[2])).collect()
    }

    #[test]
    fn the_selected_tiles_of_the_grid_come_after_the_others() {
        let mut grid = grid((2, 2));
        grid.set_selection(&[2, 1, 1, 2, 3, 3]);
        let (frame, changed) = grid.resolve(&above(0.2, 1.0));
        assert!(changed);
        assert_eq!(frame.resolution.map(|r| r.zoom), Some(2));
        // The four tiles round the middle, of which one is selected: the other selected tile is out of view.
        assert_eq!((frame.plain_count, frame.selected_count, frame.other_count), (3, 1, 0));
        assert_eq!(tiles_of(&grid, 3..4), vec![(2, 1, 1)]);
        let flags: Vec<u32> = grid.instances().as_chunks::<TILE_WORDS>().0.iter().map(|tile| tile[7] & 0xFF).collect();
        assert_eq!(flags, vec![0, 0, 0, FLAG_SELECTED]);
        assert_eq!(grid.instances().len(), 4 * TILE_WORDS);
    }

    #[test]
    fn a_tile_is_written_as_the_gpu_reads_it() {
        let mut grid = grid((2, 2));
        grid.resolve(&above(0.01, 8.0));
        // The one tile south-east of the middle is the nearest of the four that meet at it.
        let index = tiles_of(&grid, 0..4).iter().position(|&t| t == (2, 2, 2)).expect("the tile shows");
        let tile = grid.instances().as_chunks::<TILE_WORDS>().0[index];
        let floats: Vec<f32> = tile[..7].iter().map(|&bits| f32::from_bits(bits)).collect();
        // Its south-west corner in a world eight units wide: a tile is two, the row below the middle.
        assert_eq!(floats, vec![0.0, -2.0, 2.0, 0.625, 0.5, 0.5, 0.25]);
        assert_eq!(tile[7] >> SUBDIVISION_SHIFT, 0, "a tile of the plane is one quad");
    }

    #[test]
    fn the_selected_tiles_of_other_zooms_are_drawn_as_their_fill() {
        let mut grid = grid((2, 2));
        // A zoom 3 tile at the middle, one far from it, and a zoom 1 tile that holds the view.
        grid.set_selection(&[3, 4, 4, 3, 0, 0, 1, 1, 1]);
        let (frame, _) = grid.resolve(&above(0.2, 1.0));
        assert_eq!((frame.plain_count, frame.selected_count, frame.other_count), (4, 0, 2));
        let mut others = tiles_of(&grid, 4..6);
        others.sort();
        assert_eq!(others, vec![(1, 1, 1), (3, 4, 4)]);
        assert!(grid.instances().as_chunks::<TILE_WORDS>().0[4..].iter().all(|tile| tile[7] & 0xFF == FLAG_SELECTED | FLAG_FILL_ONLY));
    }

    #[test]
    fn a_frame_with_tiles_too_small_has_no_grid() {
        let mut grid = TileGrid::new();
        let input = FrameInput { viewport_height_points: 10.0, ..above(0.3, 1.0) };
        let (frame, changed) = grid.resolve(&input);
        assert!(changed);
        assert_eq!(frame, Frame::default());
        assert!(grid.instances().is_empty());
    }

    #[test]
    fn the_same_frame_is_not_resolved_twice() {
        let mut grid = grid((2, 2));
        let input = above(0.2, 1.0);
        assert!(grid.resolve(&input).1);
        let (frame, changed) = grid.resolve(&input);
        assert!(!changed);
        assert_eq!(frame.plain_count, 4);
        grid.set_selection(&[2, 1, 1]);
        assert!(grid.resolve(&input).1, "the selection is read by a frame");
        assert!(grid.resolve(&FrameInput { centre: [0.51, 0.5], ..input }).1);
    }

    #[test]
    fn a_text_is_asked_for_once_and_laid_into_its_tile() {
        let mut grid = grid((2, 2));
        let input = above(0.01, 1.0);
        grid.resolve(&input);
        assert_eq!(grid.label_misses().len(), 4 * 3, "the four tiles round the middle are asked");
        assert!(grid.glyphs().is_empty());
        // Two glyphs, two ems wide and one tall.
        let text = grid.define_layout(2.0, 1.0, &[7 << 16, (-1.0f32).to_bits(), (-0.5f32).to_bits(), 0, 9 << 16, 0f32.to_bits(), (-0.5f32).to_bits(), 0]);
        for miss in grid.label_misses().to_vec().as_chunks::<3>().0 {
            grid.set_tile_label(miss[0], miss[1], miss[2], if miss[1] == 1 { text } else { NO_LABEL });
        }
        assert_eq!(grid.lay_out_labels(), 4, "two tiles carry the text");
        assert!(grid.label_misses().is_empty());
        let glyphs = grid.glyphs().as_chunks::<GLYPH_WORDS>().0;
        let tiles = tiles_of(&grid, 0..4);
        for glyph in glyphs {
            assert_eq!(tiles[(glyph[0] & 0xFFFF) as usize].1, 1, "a glyph names its tile");
            // 0.7 of the tile over two ems is narrower than 0.12 over one.
            assert_eq!(f32::from_bits(glyph[3]), 0.12);
        }
        assert_eq!(glyphs.iter().map(|glyph| glyph[0] >> 16).collect::<Vec<_>>(), vec![7, 9, 7, 9]);
        // The next frame asks nothing.
        grid.resolve(&FrameInput { centre: [0.5, 0.501], ..input });
        assert!(grid.label_misses().is_empty());
        assert_eq!(grid.glyphs().len(), 4 * GLYPH_WORDS);
        // Until the texts are let go.
        grid.clear_tile_labels();
        grid.resolve(&input);
        assert_eq!(grid.label_misses().len(), 12);
    }

    #[test]
    fn a_text_is_sized_to_its_tile() {
        assert_eq!(tile_share_per_em(7.0, 1.0, 0.7, 0.12), 0.1);
        assert_eq!(tile_share_per_em(2.0, 1.0, 0.7, 0.12), 0.12);
        assert_eq!(tile_share_per_em(0.0, 1.0, 0.7, 0.12), 0.0);
    }

    #[test]
    fn the_sphere_draws_its_coarse_tiles_divided() {
        let mut grid = grid((2, 2));
        let (near, far, distance) = (0.01, 100.0, 3.0);
        let depth = far / (near - far);
        let matrix = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, depth, -1.0, 0.0, 0.0, depth * near - depth * distance, distance];
        let globe = GlobeUniform { pan_x: 0.0, pan_y: 0.0, radius: 1.0, transition: 0.0 };
        let input = FrameInput { camera_matrix: matrix, globe: Some((globe, [0.0, 0.0, distance])), ..above(0.3, 1.0) };
        let (frame, _) = grid.resolve(&input);
        assert!(frame.plain_count > 0 && frame.plain_count < 16);
        assert_eq!(frame.grid_subdivision, 16);
        assert!(grid.instances().as_chunks::<TILE_WORDS>().0.iter().all(|tile| tile[7] >> SUBDIVISION_SHIFT == 4));
    }
}
