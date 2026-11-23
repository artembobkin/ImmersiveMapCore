// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use std::collections::HashSet;
use std::f64::consts::PI;

use super::mesh_checks::{MeshLocator, Reference, SeededGenerator, inspect};
use crate::{FlatColor, FlattenInput, FlattenOptions, LineCap, LineJoin, LineStyle, flatten};

fn opaque(index: usize) -> FlatColor {
    FlatColor::rgb(
        (40 + index * 23 % 200) as u8,
        (index * 67 % 256) as u8,
        (255 - index * 41 % 256) as u8,
    )
}

fn options(max_x: f64, max_y: f64) -> FlattenOptions {
    FlattenOptions {
        max_x,
        max_y,
        ..FlattenOptions::default()
    }
}

fn assert_close(value: f64, expected: f64, accuracy: f64, label: &str) {
    assert!(
        (value - expected).abs() <= accuracy,
        "{label}: {value} is not within {accuracy} of {expected}"
    );
}

/// The bits of an `f32` position, with `-0` turned into `+0` as Swift's hashing of floats does.
fn position(x: f32, y: f32) -> (u32, u32) {
    ((x + 0.0).to_bits(), (y + 0.0).to_bits())
}

/// Compares the mesh with the brute-force reference at random points and checks that it is closed.
fn verify(
    input: &FlattenInput,
    options: &FlattenOptions,
    samples: usize,
    seed: u64,
    label: &str,
    closed: bool,
) {
    let mesh = flatten(input, options);
    let report = inspect(&mesh, options);
    // Without a background the mesh has holes, and their outlines are open edges.
    if closed {
        assert_eq!(report.open_edges, 0, "open edges, {label}");
        let area = (options.max_x - options.min_x) * (options.max_y - options.min_y);
        assert_close(report.area, area, area * 1e-5, &format!("area, {label}"));
    }
    assert_eq!(report.mixed_color_triangles, 0, "{label}");
    assert_eq!(report.grid_crossing_triangles, 0, "{label}");
    assert_eq!(report.flipped_triangles, 0, "{label}");

    let reference = Reference { input };
    let locator = MeshLocator::new(
        &mesh,
        options.min_x,
        options.min_y,
        options.max_x,
        options.max_y,
    );
    let mut generator = SeededGenerator::new(seed);
    let mut mismatches = 0;
    let margin = 1e-3 * (options.max_x - options.min_x).max(options.max_y - options.min_y);
    for _ in 0..samples {
        let x = generator.double_in(options.min_x, options.max_x);
        let y = generator.double_in(options.min_y, options.max_y);
        if reference.distance_to_edges(x, y) < margin {
            continue;
        }
        let expected = reference.color(x, y);
        let found = locator.colors(x, y);
        if expected == 0 {
            if !found.is_empty() {
                mismatches += 1;
            }
        } else if found.is_empty() || found.iter().any(|&c| c != expected) {
            mismatches += 1;
        }
    }
    assert_eq!(mismatches, 0, "wrong color at sample points, {label}");
}

#[test]
fn square_on_background() {
    let mut input = FlattenInput::new();
    let background = input.add_paint(opaque(0));
    let square = input.add_paint(opaque(1));
    input.add_rect(0.0, 0.0, 100.0, 100.0, background);
    input.add_rect(30.0, 30.0, 60.0, 70.0, square);
    let mut options = options(100.0, 100.0);
    for columns in [1, 2, 3, 10] {
        for rows in [1, 4] {
            options.columns = columns;
            options.rows = rows;
            let mesh = flatten(&input, &options);
            let report = inspect(&mesh, &options);
            assert_close(report.area, 100.0 * 100.0, 1e-3, "area");
            assert_eq!(report.open_edges, 0);
            let square_area: f64 = (0..mesh.triangle_count())
                .map(|t| {
                    let v = [0, 1, 2].map(|k| mesh.vertices[mesh.indices[3 * t + k] as usize]);
                    if v[0].color != opaque(1).packed() {
                        return 0.0;
                    }
                    0.5 * f64::from(
                        (v[1].x - v[0].x) * (v[2].y - v[0].y)
                            - (v[1].y - v[0].y) * (v[2].x - v[0].x),
                    )
                })
                .sum();
            assert_close(square_area, 30.0 * 40.0, 1e-3, "square area");
        }
    }
}

/// A jagged ring over a background: the mesh has the ring's points and the corners, and
/// nothing else. Every extremum of the ring used to cut its neighbours with a horizontal line,
/// which put a vertex on every edge level with it, and the regions of one color each had their
/// own copy of the vertices on their common edges.
#[test]
fn an_outline_adds_no_vertices() {
    let mut generator = SeededGenerator::new(11);
    let ring: Vec<[f64; 2]> = (0..200)
        .map(|k| {
            let angle = k as f64 / 200.0 * 2.0 * PI;
            let radius = generator.double_in(300.0, 500.0);
            [512.0 + radius * angle.cos(), 512.0 + radius * angle.sin()]
        })
        .collect();
    let mut input = FlattenInput::new();
    let background = input.add_paint(opaque(0));
    input.add_rect(0.0, 0.0, 1024.0, 1024.0, background);
    let shape = input.add_paint(opaque(1));
    input.add_ring(&ring, shape);
    let mut options = options(1024.0, 1024.0);
    options.columns = 1;
    let mesh = flatten(&input, &options);
    let report = inspect(&mesh, &options);
    assert_eq!(report.open_edges, 0);
    assert_close(report.area, 1024.0 * 1024.0, 1.0, "area");
    let positions: HashSet<(u32, u32)> = mesh.vertices.iter().map(|v| position(v.x, v.y)).collect();
    let mut expected: HashSet<(u32, u32)> = ring
        .iter()
        .map(|p| position(p[0] as f32, p[1] as f32))
        .collect();
    for (x, y) in [(0.0, 0.0), (1024.0, 0.0), (1024.0, 1024.0), (0.0, 1024.0)] {
        expected.insert(position(x, y));
    }
    assert_eq!(
        positions, expected,
        "The mesh has the outline's points and the corners only"
    );
    // Every point of the ring borders both colors, every corner one color.
    assert_eq!(
        mesh.vertices.len(),
        2 * ring.len() + 4,
        "A position is one vertex per color"
    );

    // With a grid, the only vertices added lie on the grid lines.
    options.columns = 8;
    options.rows = 8;
    let gridded = flatten(&input, &options);
    assert_eq!(inspect(&gridded, &options).open_edges, 0);
    let added: Vec<(u32, u32)> = gridded
        .vertices
        .iter()
        .map(|v| position(v.x, v.y))
        .collect::<HashSet<_>>()
        .difference(&expected)
        .copied()
        .collect();
    assert!(!added.is_empty());
    for &(x, y) in &added {
        let (x, y) = (f32::from_bits(x), f32::from_bits(y));
        assert!(
            x % 128.0 == 0.0 || y % 128.0 == 0.0,
            "({x}, {y}) is not on a grid line"
        );
    }
    let pairs: HashSet<(u32, u32, u32)> = gridded
        .vertices
        .iter()
        .map(|v| {
            let (x, y) = position(v.x, v.y);
            (x, y, (v.color as f32).to_bits())
        })
        .collect();
    assert_eq!(
        gridded.vertices.len(),
        pairs.len(),
        "A position is one vertex per color"
    );
}

#[test]
fn single_column_square_is_minimal() {
    let mut input = FlattenInput::new();
    let paint = input.add_paint(opaque(0));
    input.add_rect(0.0, 0.0, 10.0, 10.0, paint);
    let mut options = options(10.0, 10.0);
    options.columns = 1;
    assert_eq!(flatten(&input, &options).triangle_count(), 2);
}

#[test]
fn hole_and_same_color_merge() {
    let mut input = FlattenInput::new();
    let ground = input.add_paint(opaque(0));
    let a = input.add_paint(opaque(1));
    let b = input.add_paint(opaque(1));
    input.add_rect(0.0, 0.0, 64.0, 64.0, ground);
    // A ring with a hole: the hole has the opposite orientation.
    input.add_ring(&[[8.0, 8.0], [40.0, 8.0], [40.0, 40.0], [8.0, 40.0]], a);
    input.add_ring(&[[16.0, 16.0], [16.0, 32.0], [32.0, 32.0], [32.0, 16.0]], a);
    // A second paint with the same color, overlapping the first.
    input.add_ring(&[[24.0, 24.0], [56.0, 24.0], [56.0, 56.0], [24.0, 56.0]], b);
    let mut options = options(64.0, 64.0);
    options.columns = 4;
    verify(&input, &options, 4000, 1, "hole", true);
}

#[test]
fn translucent_paint_is_blended() {
    let mut input = FlattenInput::new();
    let ground = input.add_paint(FlatColor::rgb(0, 0, 200));
    let glass = input.add_paint(FlatColor::new(200, 0, 0, 128));
    input.add_rect(0.0, 0.0, 10.0, 10.0, ground);
    input.add_rect(0.0, 0.0, 5.0, 10.0, glass);
    let mut options = options(10.0, 10.0);
    options.columns = 1;
    let mesh = flatten(&input, &options);
    let colors: HashSet<u32> = mesh.vertices.iter().map(|v| v.color).collect();
    assert_eq!(colors.len(), 2);
    let blended = colors
        .iter()
        .find(|&&c| c != FlatColor::rgb(0, 0, 200).packed())
        .map(|&c| FlatColor::from_packed(c));
    assert_eq!(blended, Some(FlatColor::new(100, 0, 100, 255)));
}

#[test]
fn stroke_covers_its_center_line() {
    let mut input = FlattenInput::new();
    let ground = input.add_paint(opaque(0));
    let road = input.add_paint(opaque(1));
    input.add_rect(0.0, 0.0, 100.0, 100.0, ground);
    let line: [[f64; 2]; 6] = [
        [10.0, 10.0],
        [50.0, 12.0],
        [52.0, 60.0],
        [20.0, 40.0],
        [90.0, 90.0],
        [88.0, 86.0],
    ];
    let mut options = options(100.0, 100.0);
    options.columns = 5;
    for join in [LineJoin::Bevel, LineJoin::Miter, LineJoin::Round] {
        for cap in [LineCap::Butt, LineCap::Square, LineCap::Round] {
            let mut stroked = input.clone();
            stroked.add_line(&line, road, &LineStyle::new(6.0).join(join).cap(cap));
            let mesh = flatten(&stroked, &options);
            assert_eq!(inspect(&mesh, &options).open_edges, 0);
            let locator = MeshLocator::new(&mesh, 0.0, 0.0, 100.0, 100.0);
            for segment in 0..line.len() - 1 {
                // Stay clear of the vertices: a bevel cuts the corner of a sharp turn.
                for step in 1..20 {
                    let (a, b) = (line[segment], line[segment + 1]);
                    let f = step as f64 / 20.0;
                    let p = [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f];
                    for offset in [[0.05, 0.03], [-0.04, 0.05]] {
                        let found = locator.colors(p[0] + offset[0], p[1] + offset[1]);
                        assert_eq!(
                            found,
                            [opaque(1).packed()],
                            "join {join:?}, cap {cap:?}, segment {segment}"
                        );
                    }
                }
            }
        }
    }
}

/// Random polygons with real coordinates.
#[test]
fn random_polygons() {
    for seed in 0..120u64 {
        let mut generator = SeededGenerator::new(seed);
        let mut input = FlattenInput::new();
        // Paint 0 is kept for the background: a shape of the same paint could cut a hole in it.
        let paint_count = generator.int_in(2, 7) as usize;
        for p in 0..paint_count {
            input.add_paint(opaque(p));
        }
        let has_background = generator.bool();
        if has_background {
            input.add_rect(-10.0, -10.0, 110.0, 110.0, 0);
        }
        for _ in 0..generator.int_in(1, 25) {
            let paint = generator.int_below(1, paint_count as i64) as usize;
            let cx = generator.double_in(-20.0, 120.0);
            let cy = generator.double_in(-20.0, 120.0);
            let radius = generator.double_in(2.0, 60.0);
            // Arbitrary point order: the rings cross themselves.
            let points: Vec<[f64; 2]> = (0..generator.int_in(3, 9))
                .map(|_| {
                    let x = cx + generator.double_in(-radius, radius);
                    let y = cy + generator.double_in(-radius, radius);
                    [x, y]
                })
                .collect();
            if generator.int_below(0, 4) == 0 {
                let width = generator.double_in(0.5, 12.0);
                let join = generator.element(&[LineJoin::Bevel, LineJoin::Miter, LineJoin::Round]);
                let cap = generator.element(&[LineCap::Butt, LineCap::Square, LineCap::Round]);
                input.add_line(&points, paint, &LineStyle::new(width).join(join).cap(cap));
            } else {
                input.add_ring(&points, paint);
            }
        }
        let mut options = options(100.0, 100.0);
        options.columns = generator.int_in(1, 9) as usize;
        options.rows = generator.int_in(1, 5) as usize;
        options.parallel = generator.bool();
        verify(
            &input,
            &options,
            600,
            seed,
            &format!("seed {seed}"),
            has_background,
        );
    }
}

/// Polygons on a coarse integer grid: shared vertices, overlapping collinear edges, horizontal
/// edges, and vertices exactly on strip walls and grid rows.
#[test]
fn degenerate_grid_polygons() {
    for seed in 0..250u64 {
        let mut generator = SeededGenerator::new(seed.wrapping_add(10_000));
        let mut input = FlattenInput::new();
        let paint_count = generator.int_in(2, 6) as usize;
        for p in 0..paint_count {
            input.add_paint(opaque(p));
        }
        let has_background = generator.bool();
        if has_background {
            input.add_rect(0.0, 0.0, 16.0, 16.0, 0);
        }
        for _ in 0..generator.int_in(1, 20) {
            let paint = generator.int_below(1, paint_count as i64) as usize;
            let points: Vec<[f64; 2]> = (0..generator.int_in(3, 7))
                .map(|_| {
                    let x = generator.int_in(-2, 18) as f64;
                    let y = generator.int_in(-2, 18) as f64;
                    [x, y]
                })
                .collect();
            input.add_ring(&points, paint);
        }
        let mut options = options(16.0, 16.0);
        options.columns = generator.element(&[1, 2, 4, 8, 16]);
        options.rows = generator.element(&[1, 2, 4, 8]);
        options.parallel = false;
        verify(
            &input,
            &options,
            600,
            seed,
            &format!("grid seed {seed}"),
            has_background,
        );
    }
}
