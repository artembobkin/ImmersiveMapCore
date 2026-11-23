// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use crate::monotone_triangulator::{MonotoneTriangulator, StripWalls};
use crate::sweep::{EdgePiece, Sweep, WallEvent};
use crate::{FastHashMap, FlatMesh, FlatVertex, FlattenInput, VertexKey, fmax, fmin};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlattenOptions {
    /// The area to produce, in the units of the input. Everything outside is clipped away.
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,

    /// Number of vertical strips. Each strip is swept independently (and in parallel), and no
    /// triangle crosses a strip border. More strips make the sweep faster up to a point, and add
    /// triangles along the borders.
    pub columns: usize,

    /// Number of horizontal bands. No triangle crosses a band border. 1 means no bands.
    ///
    /// For a mesh that will be displaced by a heightmap, set `columns` and `rows` to the grid size
    /// of the terrain: every triangle then lies inside one grid cell.
    pub rows: usize,

    /// Sweep the strips on all cores. Without threads (WebAssembly) the strips are always swept
    /// one after another, and the result is the same either way.
    pub parallel: bool,
}

impl Default for FlattenOptions {
    fn default() -> Self {
        FlattenOptions {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 4096.0,
            max_y: 4096.0,
            columns: 16,
            rows: 1,
            parallel: true,
        }
    }
}

/// Wall-clock time of each stage, in seconds.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FlattenTimings {
    pub binning: f64,
    pub sweep: f64,
    pub triangulation: f64,
    pub merge: f64,
}

impl FlattenTimings {
    pub fn total(&self) -> f64 {
        self.binning + self.sweep + self.triangulation + self.merge
    }
}

/// Flattens stacked, overlapping painted rings into one layer of triangles that do not overlap.
///
/// Inside the bounds the mesh has no T-junctions: two triangles that touch share whole edges,
/// so the mesh stays closed when its vertices are moved, for example by a heightmap. Its
/// vertices are the vertices of the visible outlines, the crossings of outlines, and the points
/// where an outline crosses the grid, each once per color that touches it: the regions of one
/// color share the vertices on their common edges, and no region is cut by a line that the
/// input does not have.
pub fn flatten(input: &FlattenInput, options: &FlattenOptions) -> FlatMesh {
    let mut timings = FlattenTimings::default();
    flatten_timed(input, options, &mut timings, || 0.0)
}

/// `flatten`, measuring each stage with `now`, a clock in seconds. `std::time::Instant` is not
/// available in WebAssembly, so the caller provides the clock (for example `performance.now()`
/// divided by 1000).
pub fn flatten_timed(
    input: &FlattenInput,
    options: &FlattenOptions,
    timings: &mut FlattenTimings,
    mut now: impl FnMut() -> f64,
) -> FlatMesh {
    let mut mark = now();
    let mut lap = || {
        let time = now();
        let duration = time - mark;
        mark = time;
        duration
    };

    let columns = options.columns.max(1);
    let rows = options.rows.max(1);
    if !(options.max_x > options.min_x && options.max_y > options.min_y) || input.paints.is_empty()
    {
        return FlatMesh::default();
    }

    let mut walls = vec![0.0f64; columns + 1];
    for (t, wall) in walls.iter_mut().enumerate() {
        *wall = options.min_x + (options.max_x - options.min_x) * t as f64 / columns as f64;
    }
    walls[columns] = options.max_x;
    let row_ys: Vec<f64> = (1..rows)
        .map(|r| options.min_y + (options.max_y - options.min_y) * r as f64 / rows as f64)
        .collect();

    let binned = bin(input, &walls, options.min_y, options.max_y);
    let mut strips: Vec<Strip<'_>> = binned
        .pieces
        .into_iter()
        .zip(binned.wall_events)
        .enumerate()
        .map(|(t, (pieces, wall_events))| Strip {
            sweep: Sweep::new(
                pieces,
                wall_events,
                walls[t],
                walls[t + 1],
                options.min_y,
                options.max_y,
                &row_ys,
                &input.paints,
            ),
            triangulator: MonotoneTriangulator::default(),
        })
        .collect();
    timings.binning = lap();

    for_each_strip(&mut strips, options.parallel, |_, strip| strip.sweep.run());
    timings.sweep = lap();

    // Both sides of a wall must agree on the vertices that lie on it.
    let wall_marks: Vec<Vec<f64>> = (0..=columns)
        .map(|t| {
            let from_right: &[f64] = if t < columns {
                &strips[t].sweep.left_marks
            } else {
                &[]
            };
            let from_left: &[f64] = if t > 0 {
                &strips[t - 1].sweep.right_marks
            } else {
                &[]
            };
            merge_ascending(from_left, from_right)
        })
        .collect();
    for_each_strip(&mut strips, options.parallel, |t, strip| {
        let walls = StripWalls {
            wall_l: walls[t],
            wall_r: walls[t + 1],
            left_marks: &wall_marks[t],
            right_marks: &wall_marks[t + 1],
        };
        let Strip {
            sweep,
            triangulator,
        } = strip;
        for poly in &sweep.polys {
            triangulator.triangulate(poly, &sweep.poly_data, &walls);
        }
    });
    timings.triangulation = lap();

    // The strips' vertices in one list. A vertex on a wall is in both strips that share the
    // wall, and comes out once.
    let vertex_count: usize = strips.iter().map(|s| s.triangulator.vertices.len()).sum();
    let index_count: usize = strips.iter().map(|s| s.triangulator.indices.len()).sum();
    let mut vertices: Vec<FlatVertex> = Vec::with_capacity(vertex_count);
    let mut indices: Vec<u32> = Vec::with_capacity(index_count);
    let mut wall_vertices: FastHashMap<VertexKey, u32> = FastHashMap::default();
    let mut remap: Vec<u32> = Vec::new();
    for (t, strip) in strips.iter().enumerate() {
        let wall_l = walls[t] as f32;
        let wall_r = walls[t + 1] as f32;
        let source = &strip.triangulator.vertices;
        remap.clear();
        remap.reserve(source.len());
        for vertex in source {
            if vertex.x == wall_l || vertex.x == wall_r {
                let next = vertices.len() as u32;
                let shared = *wall_vertices
                    .entry(VertexKey::new(vertex.x, vertex.y, vertex.color))
                    .or_insert(next);
                if shared != next {
                    remap.push(shared);
                    continue;
                }
            }
            remap.push(vertices.len() as u32);
            vertices.push(*vertex);
        }
        indices.extend(
            strip
                .triangulator
                .indices
                .iter()
                .map(|&k| remap[k as usize]),
        );
    }
    timings.merge = lap();
    FlatMesh::new(vertices, indices)
}

/// The working state of one strip. Each strip is only touched by one thread at a time.
struct Strip<'a> {
    sweep: Sweep<'a>,
    triangulator: MonotoneTriangulator,
}

/// Runs `body` for every strip, on all cores when `parallel` is set and threads are available.
fn for_each_strip<'a>(
    strips: &mut [Strip<'a>],
    parallel: bool,
    body: impl Fn(usize, &mut Strip<'a>) + Sync,
) {
    #[cfg(not(target_arch = "wasm32"))]
    if parallel && strips.len() > 1 {
        use std::sync::Mutex;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let threads = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(strips.len());
        if threads > 1 {
            // Strips take very different times, so the threads take them one at a time.
            let cells: Vec<Mutex<&mut Strip<'a>>> = strips.iter_mut().map(Mutex::new).collect();
            let next = AtomicUsize::new(0);
            std::thread::scope(|scope| {
                for _ in 0..threads {
                    scope.spawn(|| {
                        loop {
                            let t = next.fetch_add(1, Ordering::Relaxed);
                            let Some(cell) = cells.get(t) else { break };
                            let mut strip = cell.lock().unwrap_or_else(|e| e.into_inner());
                            body(t, &mut strip);
                        }
                    });
                }
            });
            return;
        }
    }
    let _ = parallel;
    for (t, strip) in strips.iter_mut().enumerate() {
        body(t, strip);
    }
}

fn merge_ascending(a: &[f64], b: &[f64]) -> Vec<f64> {
    if a.is_empty() {
        return b.to_vec();
    }
    if b.is_empty() {
        return a.to_vec();
    }
    let mut result: Vec<f64> = Vec::with_capacity(a.len() + b.len());
    let mut i = 0;
    let mut j = 0;
    while i < a.len() || j < b.len() {
        let value;
        if j >= b.len() || (i < a.len() && a[i] <= b[j]) {
            value = a[i];
            i += 1;
        } else {
            value = b[j];
            j += 1;
        }
        if result.last() != Some(&value) {
            result.push(value);
        }
    }
    result
}

// Binning

struct Binned {
    pieces: Vec<Vec<EdgePiece>>,
    wall_events: Vec<Vec<WallEvent>>,
}

/// Distributes the edges of all rings over the strips.
///
/// A strip only stores the edge parts inside it. Geometry to the left of a strip still decides
/// what is inside and outside in the strip. That is carried by wall events: where a ring
/// crosses the left wall of a strip, the winding number on the wall changes for everything
/// below the crossing. Geometry to the right of a strip has no effect on it.
fn bin(input: &FlattenInput, walls: &[f64], min_y: f64, max_y: f64) -> Binned {
    let columns = walls.len() - 1;
    let icolumns = columns as isize;
    let min_x = walls[0];
    let inverse_width = columns as f64 / (walls[columns] - min_x);
    let estimate = input.point_count() / columns + 16;
    let mut pieces: Vec<Vec<EdgePiece>> =
        (0..columns).map(|_| Vec::with_capacity(estimate)).collect();
    let mut wall_events: Vec<Vec<WallEvent>> = (0..columns).map(|_| Vec::new()).collect();

    // -1 is left of the bounds, `columns` is right of them (including the right border itself).
    let strip_index = |x: f64| -> isize {
        let guess = ((x - min_x) * inverse_width).floor();
        let mut index = if guess < -1.0 {
            -1
        } else if guess > columns as f64 {
            icolumns
        } else {
            guess as isize
        };
        while index >= 0 && x < walls[index as usize] {
            index -= 1;
        }
        while index < icolumns && x >= walls[(index + 1) as usize] {
            index += 1;
        }
        index
    };

    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    fn add_piece(
        pieces: &mut [Vec<EdgePiece>],
        walls: &[f64],
        min_y: f64,
        max_y: f64,
        strip: isize,
        (x0, y0, x1, y1): (f64, f64, f64, f64),
        dxdy: f64,
        paint: u16,
    ) {
        if y0 == y1 {
            return;
        }
        let mut piece = if y0 < y1 {
            EdgePiece {
                x_top: x0,
                y_top: y0,
                x_bot: x1,
                y_bot: y1,
                dxdy,
                paint,
                dir: 1,
            }
        } else {
            EdgePiece {
                x_top: x1,
                y_top: y1,
                x_bot: x0,
                y_bot: y0,
                dxdy,
                paint,
                dir: -1,
            }
        };
        if piece.y_bot <= min_y || piece.y_top >= max_y {
            return;
        }
        let strip = strip as usize;
        let lo = walls[strip];
        let hi = walls[strip + 1];
        if piece.y_top < min_y {
            piece.x_top = fmin(fmax(piece.x_top + (min_y - piece.y_top) * dxdy, lo), hi);
            piece.y_top = min_y;
        }
        if piece.y_bot > max_y {
            piece.x_bot = fmin(fmax(piece.x_top + (max_y - piece.y_top) * dxdy, lo), hi);
            piece.y_bot = max_y;
        }
        pieces[strip].push(piece);
    }

    let coords = &input.coords[..];
    for ring in &input.rings {
        let start = ring.start as usize * 2;
        let count = ring.count as usize;
        let paint = ring.paint;
        let mut ax = coords[start + 2 * (count - 1)];
        let mut ay = coords[start + 2 * (count - 1) + 1];
        let mut sa = strip_index(ax);
        for k in 0..count {
            let bx = coords[start + 2 * k];
            let by = coords[start + 2 * k + 1];
            let sb = strip_index(bx);
            if sa == sb {
                if sa >= 0 && sa < icolumns && ay != by {
                    add_piece(
                        &mut pieces,
                        walls,
                        min_y,
                        max_y,
                        sa,
                        (ax, ay, bx, by),
                        (bx - ax) / (by - ay),
                        paint,
                    );
                }
            } else {
                let dydx = (by - ay) / (bx - ax);
                let dxdy = (bx - ax) / (by - ay);
                let y_lo = fmin(ay, by);
                let y_hi = fmax(ay, by);
                let mut px = ax;
                let mut py = ay;
                if sa < sb {
                    // Moving right through the walls sa+1...sb.
                    let mut strip = sa;
                    for t in sa + 1..=sb {
                        let wx = walls[t as usize];
                        let wy = if wx == bx {
                            by
                        } else {
                            fmin(fmax(ay + (wx - ax) * dydx, y_lo), y_hi)
                        };
                        if strip >= 0 {
                            add_piece(
                                &mut pieces,
                                walls,
                                min_y,
                                max_y,
                                strip,
                                (px, py, wx, wy),
                                dxdy,
                                paint,
                            );
                        }
                        if t < icolumns {
                            wall_events[t as usize].push(WallEvent {
                                y: wy,
                                paint,
                                delta: -1,
                            });
                        }
                        px = wx;
                        py = wy;
                        strip = t;
                    }
                    if sb < icolumns {
                        add_piece(
                            &mut pieces,
                            walls,
                            min_y,
                            max_y,
                            sb,
                            (px, py, bx, by),
                            dxdy,
                            paint,
                        );
                    }
                } else {
                    // Moving left through the walls sa...sb+1.
                    let mut strip = sa;
                    for t in (sb + 1..=sa).rev() {
                        let wx = walls[t as usize];
                        let wy = if wx == ax {
                            ay
                        } else {
                            fmin(fmax(ay + (wx - ax) * dydx, y_lo), y_hi)
                        };
                        if strip < icolumns {
                            add_piece(
                                &mut pieces,
                                walls,
                                min_y,
                                max_y,
                                strip,
                                (px, py, wx, wy),
                                dxdy,
                                paint,
                            );
                        }
                        if t < icolumns {
                            wall_events[t as usize].push(WallEvent {
                                y: wy,
                                paint,
                                delta: 1,
                            });
                        }
                        px = wx;
                        py = wy;
                        strip = t - 1;
                    }
                    if sb >= 0 {
                        add_piece(
                            &mut pieces,
                            walls,
                            min_y,
                            max_y,
                            sb,
                            (px, py, bx, by),
                            dxdy,
                            paint,
                        );
                    }
                }
            }
            ax = bx;
            ay = by;
            sa = sb;
        }
    }
    Binned {
        pieces,
        wall_events,
    }
}
