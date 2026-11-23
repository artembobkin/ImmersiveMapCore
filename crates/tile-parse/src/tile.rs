// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Tile identity and the four quarters of a tile.

use crate::coordinate_space::{TILE_EXTENT, render_point};
use crate::geom::V2;
use crate::parse_polygon::ClipSquare;

/// Content identity of a vector tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Tile {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl Tile {
    pub const fn new(z: i32, x: i32, y: i32) -> Tile {
        Tile { x, y, z }
    }

    /// Whether this tile, of a lower zoom, contains `other`.
    pub fn covers(&self, other: &Tile) -> bool {
        if self.z >= other.z {
            return false;
        }
        let scale = 1 << (other.z - self.z);
        let (min_x, max_x) = (self.x * scale, (self.x + 1) * scale - 1);
        let (min_y, max_y) = (self.y * scale, (self.y + 1) * scale - 1);
        other.x >= min_x && other.x <= max_x && other.y >= min_y && other.y <= max_y
    }

    pub fn parent_at(&self, zoom: i32) -> Option<Tile> {
        if self.z < zoom || zoom < 0 {
            return None;
        }
        let d = self.z - zoom;
        Some(Tile { x: self.x >> d, y: self.y >> d, z: zoom })
    }
}

/// The four quarters of a tile: the squares of its four children. A quarter
/// is numbered by where it lies in tile space, x east and y south: 0 the
/// north-west, 1 the north-east, 2 the south-west, 3 the south-east.
pub mod quarters {
    use super::*;

    pub const COUNT: usize = 4;
    /// Every quarter, the mask a whole tile draws with.
    pub const ALL: u8 = 0b1111;

    /// The quarter a point in tile space lies in. A point on a quarter's
    /// edge goes east and south.
    pub fn of_tile_point(point: V2) -> usize {
        let half = TILE_EXTENT / 2.0;
        (if point.y >= half { 2 } else { 0 }) + (if point.x >= half { 1 } else { 0 })
    }

    pub fn of_render_point(point: V2) -> usize {
        of_tile_point(render_point(point))
    }

    pub fn square(quarter: usize) -> ClipSquare {
        let half = TILE_EXTENT / 2.0;
        let corner = V2::new((quarter & 1) as f32 * half, (quarter >> 1) as f32 * half);
        ClipSquare { minimum: corner, maximum: corner + V2::new(half, half) }
    }

    pub fn child(tile: &Tile, quarter: usize) -> Tile {
        Tile { x: tile.x * 2 + (quarter & 1) as i32, y: tile.y * 2 + (quarter >> 1) as i32, z: tile.z + 1 }
    }

    /// The runs of `starts` (five quarter starts, the last the count) that
    /// `mask` draws, consecutive quarters joined. Empty `starts` is a tile
    /// kept whole: `whole` is drawn when the mask holds every quarter.
    pub fn runs(starts: &[usize], whole: std::ops::Range<usize>, mask: u8) -> Vec<std::ops::Range<usize>> {
        if starts.len() != COUNT + 1 {
            return if mask == ALL { vec![whole] } else { vec![] };
        }
        let mut runs: Vec<std::ops::Range<usize>> = Vec::new();
        for q in 0..COUNT {
            if mask & (1 << q) == 0 {
                continue;
            }
            let run = starts[q]..starts[q + 1];
            if run.is_empty() {
                continue;
            }
            match runs.last_mut() {
                Some(last) if last.end == run.start => last.end = run.end,
                _ => runs.push(run),
            }
        }
        runs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarters_runs_join_neighbours() {
        let starts = [0, 10, 10, 25, 40];
        assert_eq!(quarters::runs(&starts, 0..40, quarters::ALL), vec![0..40]);
        assert_eq!(quarters::runs(&starts, 0..40, 0b1001), vec![0..10, 25..40]);
        assert_eq!(quarters::runs(&[], 0..40, 0b0111), Vec::<std::ops::Range<usize>>::new());
        assert_eq!(quarters::of_tile_point(V2::new(2048.0, 100.0)), 1);
        assert_eq!(quarters::child(&Tile::new(3, 1, 2), 3), Tile::new(4, 3, 5));
    }

    #[test]
    fn covers_and_parent() {
        let t = Tile::new(2, 1, 1);
        assert!(t.covers(&Tile::new(4, 7, 4)));
        assert!(!t.covers(&Tile::new(4, 8, 4)));
        assert_eq!(Tile::new(5, 13, 9).parent_at(3), Some(Tile::new(3, 3, 2)));
    }
}
