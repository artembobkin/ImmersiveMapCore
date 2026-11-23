// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Flattens the ground fills of a tile into one layer of triangles that do
//! not overlap, for the tiles of the zooms the camera blows up.
//!
//! A layered tile orders its fills by a rank in the vertex depth, which the
//! near plane's cut moves on a triangle blown up to many screens: the fills
//! then swap from frame to frame at a street tilt. A flattened tile has
//! nothing to order: every pixel of its ground is one fill, and no triangle
//! crosses a cell of a `grid` by `grid` grid, which keeps every edge short
//! and makes the mesh ready for a heightmap.
//!
//! The fills go in as rings of the style's paint in ascending style order,
//! so a later style covers an earlier one. A paint is opaque and names its
//! style in its red byte. The mesh comes back as one polygon per style. The
//! fills a style draws among the ground lines are not flattened.

use std::collections::BTreeMap;

use tile_flattening::{FlatColor, FlattenInput, FlattenOptions, flatten as flatten_mesh};

use crate::coordinate_space::TILE_EXTENT_F64;
use crate::geom::V2;
use crate::parsed_polygon::ParsedPolygon;

pub type PolygonsByStyle = BTreeMap<u8, Vec<ParsedPolygon>>;

/// The fewest strips the flattener sweeps a tile in, whatever the grid: the
/// sweep's cost grows faster than the strip's share of the fills.
pub const MINIMUM_SWEEP_COLUMNS: usize = 64;

/// A fill's rings into the flattener's input: an outline's rings as they
/// are, a tessellated fill's triangles one ring each.
pub fn add_rings(polygon: &ParsedPolygon, paint: usize, input: &mut FlattenInput) {
    let vertices = &polygon.vertices;
    if polygon.is_outline() {
        let starts = &polygon.outline_starts;
        for ring in 0..starts.len() {
            let end = if ring + 1 < starts.len() { starts[ring + 1] as usize } else { vertices.len() };
            input.begin_ring(paint);
            for v in &vertices[starts[ring] as usize..end] {
                input.add_point(v.x as f64, v.y as f64);
            }
            input.end_ring();
        }
        return;
    }
    for t in polygon.indices.as_chunks::<3>().0 {
        input.begin_ring(paint);
        for &i in t {
            let v = vertices[i as usize];
            input.add_point(v.x as f64, v.y as f64);
        }
        input.end_ring();
    }
}

pub fn flatten(polygon_by_style: PolygonsByStyle, grid: i32) -> PolygonsByStyle {
    let mut input = FlattenInput::new();
    let mut paint_styles: Vec<u8> = Vec::new();
    let mut kept = PolygonsByStyle::new();
    for (&style, polygons) in &polygon_by_style {
        let mut fills: Vec<&ParsedPolygon> = Vec::new();
        for polygon in polygons {
            if polygon.is_line_ribbon() {
                kept.entry(style).or_default().push(polygon.clone());
            } else {
                fills.push(polygon);
            }
        }
        if fills.is_empty() {
            continue;
        }
        let paint = input.add_paint(FlatColor::new(style, 0, 0, 255));
        paint_styles.push(style);
        for polygon in fills {
            add_rings(polygon, paint, &mut input);
        }
    }
    if paint_styles.is_empty() {
        return kept;
    }
    let options = FlattenOptions {
        min_x: 0.0,
        min_y: 0.0,
        max_x: TILE_EXTENT_F64,
        max_y: TILE_EXTENT_F64,
        columns: (grid.max(1) as usize).max(MINIMUM_SWEEP_COLUMNS),
        rows: grid.max(1) as usize,
        ..FlattenOptions::default()
    };
    let mesh = flatten_mesh(&input, &options);
    // One polygon per style, the mesh's vertices renumbered per style, the
    // coordinates unrounded so the triangles stay watertight. The mesh shares
    // a vertex within one paint only.
    let mut local_index = vec![u32::MAX; mesh.vertices.len()];
    let mut vertices_by_style: Vec<Vec<V2>> = vec![Vec::new(); paint_styles.len()];
    let mut indices_by_style: Vec<Vec<u32>> = vec![Vec::new(); paint_styles.len()];
    let mut slot_of_style = [-1i32; 256];
    for (slot, &style) in paint_styles.iter().enumerate() {
        slot_of_style[style as usize] = slot as i32;
    }
    for t in mesh.indices.as_chunks::<3>().0 {
        let slot = slot_of_style[FlatColor::from_packed(mesh.vertices[t[0] as usize].color).r as usize];
        if slot < 0 {
            continue;
        }
        let slot = slot as usize;
        for &source in t {
            let source = source as usize;
            let mut local = local_index[source];
            if local == u32::MAX {
                let v = mesh.vertices[source];
                local = vertices_by_style[slot].len() as u32;
                vertices_by_style[slot].push(V2::new(v.x, v.y));
                local_index[source] = local;
            }
            indices_by_style[slot].push(local);
        }
    }
    // The flattener winds a triangle by its strip's structure and keeps the
    // zero-area triangles closing a T-junction, which can round into a small
    // negative area as floats: the winding is settled last.
    let mut result = kept;
    for (slot, &style) in paint_styles.iter().enumerate() {
        if indices_by_style[slot].is_empty() {
            continue;
        }
        let mut polygon =
            ParsedPolygon::new(std::mem::take(&mut vertices_by_style[slot]), std::mem::take(&mut indices_by_style[slot]));
        polygon.wind_counter_clockwise();
        result.entry(style).or_default().push(polygon);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_later_style_covers_an_earlier_one_and_ribbons_are_kept() {
        let square = |min: f32, max: f32| ParsedPolygon {
            vertices: vec![V2::new(min, min), V2::new(max, min), V2::new(max, max), V2::new(min, max)],
            outline_starts: vec![0],
            ..Default::default()
        };
        let mut ribbon = ParsedPolygon::new(vec![V2::new(0.0, 0.0), V2::new(10.0, 0.0), V2::new(0.0, 10.0)], vec![0, 1, 2]);
        ribbon.make_line_class_fill();
        let mut input = PolygonsByStyle::new();
        input.insert(1, vec![square(0.0, 4096.0)]);
        input.insert(2, vec![square(1024.0, 2048.0), ribbon.clone()]);
        let out = flatten(input, 16);
        let area = |p: &ParsedPolygon| -> f64 {
            p.indices
                .chunks(3)
                .map(|t| crate::parsed_polygon::doubled_area(p.vertices[t[0] as usize], p.vertices[t[1] as usize], p.vertices[t[2] as usize]) / 2.0)
                .sum()
        };
        let a1: f64 = out[&1].iter().map(area).sum();
        let a2: f64 = out[&2].iter().filter(|p| !p.is_line_ribbon()).map(area).sum();
        assert!((a2 - 1024.0 * 1024.0).abs() < 1.0, "{a2}");
        assert!((a1 - (4096.0 * 4096.0 - 1024.0 * 1024.0)).abs() < 1.0, "{a1}");
        assert!(out[&2].iter().any(|p| p.is_line_ribbon()));
        for p in out.values().flatten() {
            assert_eq!(ParsedPolygon::first_clockwise_triangle(&p.vertices, &p.indices), None);
        }
    }
}
