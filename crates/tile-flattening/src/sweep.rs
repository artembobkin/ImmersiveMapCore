// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use std::cmp::Ordering;

use crate::{FlatColor, fmax, fmin};

/// A non-horizontal part of an input edge, clipped to one strip and to the vertical bounds.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EdgePiece {
    pub(crate) x_top: f64,
    pub(crate) y_top: f64,
    pub(crate) x_bot: f64,
    pub(crate) y_bot: f64,
    /// Slope of the original edge, more accurate than the slope of a short clipped part.
    pub(crate) dxdy: f64,
    pub(crate) paint: u16,
    /// +1 when the original edge goes down, -1 when it goes up.
    pub(crate) dir: i16,
}

/// A change of the winding number on the left wall of a strip, caused by geometry to the left of it.
/// It applies to everything below `y`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WallEvent {
    pub(crate) y: f64,
    pub(crate) paint: u16,
    pub(crate) delta: i16,
}

/// One finished color region: a polygon that is monotone in y, with a horizontal top and bottom.
///
/// Layout in `poly_data` from `offset`: the x of the top points (left to right), the x of the
/// bottom points (left to right), then the intermediate points of the left side and of the right
/// side as (x, y, wall) triples from top to bottom. A side runs along a strip wall from the top
/// when the header's flag says so, and from a point whose `wall` is 1 to the next point. Such a
/// stretch stores no intermediate points: they are generated later from the marks shared with the
/// neighbour strip.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PolyHeader {
    pub(crate) color: u32,
    pub(crate) flags: u8,
    pub(crate) y_top: f64,
    pub(crate) y_bot: f64,
    pub(crate) offset: u32,
    pub(crate) top_count: u32,
    pub(crate) bottom_count: u32,
    pub(crate) left_count: u32,
    pub(crate) right_count: u32,
}

impl PolyHeader {
    pub(crate) const LEFT_ON_WALL: u8 = 1;
    pub(crate) const RIGHT_ON_WALL: u8 = 2;
}

/// Points closer than this are treated as the same point.
const EPS_X: f64 = 1e-5;
/// Crossings closer than this to a beam border are not cut out.
const EPS_Y: f64 = 1e-9;

/// An edge piece that crosses the current beam.
#[derive(Clone, Copy)]
struct ActiveEdge {
    x_top: f64,
    y_top: f64,
    x_bot: f64,
    y_bot: f64,
    dxdy: f64,
    /// x at the top and at the bottom of the current beam.
    xa: f64,
    xb: f64,
    index: i32,
    paint: u16,
    dir: i16,
}

impl ActiveEdge {
    #[inline(always)]
    fn x(&self, y: f64) -> f64 {
        if y >= self.y_bot {
            return self.x_bot;
        }
        let x = self.x_top + (y - self.y_top) * self.dxdy;
        if self.x_top < self.x_bot {
            fmin(fmax(x, self.x_top), self.x_bot)
        } else {
            fmin(fmax(x, self.x_bot), self.x_top)
        }
    }
}

/// A visible edge, or a group of coinciding edges, between two colors in the current beam.
#[derive(Clone, Copy)]
struct Boundary {
    /// x at the top and at the bottom of the beam.
    x: f64,
    x_end: f64,
    /// Smallest and largest piece index of the group. -1 is the left wall, -2 the right wall.
    id_lo: i32,
    id_hi: i32,
    /// Color to the right of the boundary. 0 is nothing.
    color: u32,
    /// Open span to the right of the boundary, or a negative state.
    span: i32,
    node: i32,
}

const SPAN_NONE: i32 = -1;
const SPAN_CONTINUED: i32 = -2;
const SPAN_TO_OPEN: i32 = -3;

/// A distinct point on the horizontal line of the current event.
#[derive(Clone, Copy)]
struct Node {
    x: f64,
    last_p: i32,
    last_c: i32,
    count_p: i32,
    count_c: i32,
    /// A marked node becomes a vertex of every region that touches it.
    marked: bool,
}

#[derive(Clone, Copy)]
struct OpenSpan {
    color: u32,
    flags: u8,
    y_top: f64,
    top_start: u32,
    top_count: u32,
    left_head: i32,
    left_tail: i32,
    left_count: u32,
    right_head: i32,
    right_tail: i32,
    right_count: u32,
    /// Two regions of one color that met from above at a vertex (`meet_x`, `meet_y`) go on as one
    /// region below it. Both stay open: this span is the left part and `partner` the right part,
    /// and the first vertex of the outline below the meeting point gets a diagonal from it, which
    /// settles where one part ends. -1 for a region in one piece.
    partner: i32,
    meet_x: f64,
    meet_y: f64,
}

impl OpenSpan {
    fn new(color: u32, flags: u8, y_top: f64, top_start: u32, top_count: u32) -> Self {
        OpenSpan {
            color,
            flags,
            y_top,
            top_start,
            top_count,
            left_head: -1,
            left_tail: -1,
            left_count: 0,
            right_head: -1,
            right_tail: -1,
            right_count: 0,
            partner: -1,
            meet_x: 0.0,
            meet_y: 0.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ComponentKind {
    Fallback,
    Continuation,
    Split,
    Merge,
}

/// The spans of one color that meet at the current event line: the previous spans that end or
/// carry on there and the current spans that start or carry on, as far as they overlap.
#[derive(Clone, Copy)]
struct Component {
    color: u32,
    first_prev: usize,
    last_prev: usize,
    first_cur: usize,
    last_cur: usize,
    prev_count: i32,
    cur_count: i32,
    kind: ComponentKind,
}

#[derive(Clone, Copy)]
struct ChainPoint {
    x: f64,
    y: f64,
    next: i32,
    /// The side runs along the strip wall from this point to the next one (or to the bottom).
    /// The wall's vertices are only known at the end, from both strips, and the triangulator
    /// puts them in then.
    wall_after: bool,
}

/// The winding number of every paint at the current position of a left-to-right walk, and the
/// visible color it gives.
#[derive(Default)]
struct Winding {
    paint_color: Vec<u32>,
    paint_opaque: Vec<bool>,
    counts: Vec<i32>,
    /// One bit per paint with a nonzero winding number.
    mask: Vec<u64>,
}

impl Winding {
    #[inline(always)]
    fn add(&mut self, paint: u16, delta: i32) {
        let p = usize::from(paint);
        let old = self.counts[p];
        let new = old + delta;
        self.counts[p] = new;
        if old == 0 {
            self.mask[p >> 6] |= 1 << (p & 63);
        } else if new == 0 {
            self.mask[p >> 6] &= !(1 << (p & 63));
        }
    }

    #[inline(always)]
    fn covers(&self, p: usize) -> bool {
        self.mask[p >> 6] & (1 << (p & 63)) != 0
    }

    /// The color seen from above for the current set of covering paints.
    #[inline(always)]
    fn visible_color(&self) -> u32 {
        for w in (0..self.mask.len()).rev() {
            let bits = self.mask[w];
            if bits != 0 {
                let top = w * 64 + 63 - bits.leading_zeros() as usize;
                if self.paint_opaque[top] {
                    return self.paint_color[top];
                }
                return self.blended_color(top);
            }
        }
        0
    }

    /// Composites translucent paints over whatever lies below them.
    #[inline(never)]
    fn blended_color(&self, top: usize) -> u32 {
        // Find the topmost opaque paint below. Nothing under it is visible.
        let mut bottom = top;
        for p in (0..top).rev() {
            if self.covers(p) {
                bottom = p;
                if self.paint_opaque[p] {
                    break;
                }
            }
        }
        let (mut r, mut g, mut b, mut a) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for p in bottom..=top {
            if !self.covers(p) {
                continue;
            }
            let c = FlatColor::from_packed(self.paint_color[p]);
            let sa = f64::from(c.a) / 255.0;
            let out_a = sa + a * (1.0 - sa);
            if out_a > 0.0 {
                r = (f64::from(c.r) * sa + r * a * (1.0 - sa)) / out_a;
                g = (f64::from(c.g) * sa + g * a * (1.0 - sa)) / out_a;
                b = (f64::from(c.b) * sa + b * a * (1.0 - sa)) / out_a;
            }
            a = out_a;
        }
        let alpha = (a * 255.0).round() as u8;
        if alpha == 0 {
            return 0;
        }
        FlatColor::new(r.round() as u8, g.round() as u8, b.round() as u8, alpha).packed()
    }
}

/// Sweeps one vertical strip from top to bottom and cuts it into colored monotone polygons.
///
/// Between two consecutive events the active edges do not cross, so they split the beam into
/// trapezoids. Walking the edges from left to right and keeping a winding number per paint gives
/// the visible color of each trapezoid. A trapezoid with the same color as the one above it
/// extends that one, so a region is only cut where its own outline changes, and where it changes
/// the cut is a diagonal between two vertices of the outline (`connect`). The mesh then has no
/// vertex that the outlines, the crossings of the outlines and the grid do not have.
pub(crate) struct Sweep<'a> {
    // Input.
    pieces: Vec<EdgePiece>,
    wall_events: Vec<WallEvent>,
    wall_l: f64,
    wall_r: f64,
    min_y: f64,
    max_y: f64,
    row_ys: &'a [f64],
    paints: &'a [FlatColor],

    // Output.
    pub(crate) polys: Vec<PolyHeader>,
    pub(crate) poly_data: Vec<f64>,
    /// Heights at which this strip has a vertex on its left and right wall, ascending.
    pub(crate) left_marks: Vec<f64>,
    pub(crate) right_marks: Vec<f64>,

    // Scratch, allocated only while `run` executes.
    winding: Winding,
    active: Vec<ActiveEdge>,
    previous: Vec<Boundary>,
    current: Vec<Boundary>,
    nodes: Vec<Node>,
    spans: Vec<OpenSpan>,
    free_spans: Vec<i32>,
    chain: Vec<ChainPoint>,
    top_pool: Vec<f64>,
    components: Vec<Component>,
    previous_in_component: Vec<bool>,
}

impl<'a> Sweep<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        pieces: Vec<EdgePiece>,
        wall_events: Vec<WallEvent>,
        wall_l: f64,
        wall_r: f64,
        min_y: f64,
        max_y: f64,
        row_ys: &'a [f64],
        paints: &'a [FlatColor],
    ) -> Self {
        Sweep {
            pieces,
            wall_events,
            wall_l,
            wall_r,
            min_y,
            max_y,
            row_ys,
            paints,
            polys: Vec::new(),
            poly_data: Vec::new(),
            left_marks: Vec::new(),
            right_marks: Vec::new(),
            winding: Winding::default(),
            active: Vec::new(),
            previous: Vec::new(),
            current: Vec::new(),
            nodes: Vec::new(),
            spans: Vec::new(),
            free_spans: Vec::new(),
            chain: Vec::new(),
            top_pool: Vec::new(),
            components: Vec::new(),
            previous_in_component: Vec::new(),
        }
    }

    fn release_scratch(&mut self) {
        self.winding = Winding::default();
        self.active = Vec::new();
        self.previous = Vec::new();
        self.current = Vec::new();
        self.nodes = Vec::new();
        self.spans = Vec::new();
        self.free_spans = Vec::new();
        self.chain = Vec::new();
        self.top_pool = Vec::new();
        self.components = Vec::new();
        self.previous_in_component = Vec::new();
        self.pieces = Vec::new();
        self.wall_events = Vec::new();
    }

    // Sweep

    pub(crate) fn run(&mut self) {
        self.pieces
            .sort_by(|a, b| a.y_top.partial_cmp(&b.y_top).unwrap_or(Ordering::Equal));
        self.wall_events
            .sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(Ordering::Equal));
        self.polys.reserve(self.pieces.len());
        self.poly_data.reserve(self.pieces.len() * 6);
        let paint_count = self.paints.len();
        self.winding.paint_color.reserve(paint_count);
        self.winding.paint_opaque.reserve(paint_count);
        for paint in self.paints {
            self.winding.paint_color.push(paint.packed());
            self.winding.paint_opaque.push(paint.a == 255);
        }
        self.winding.counts.resize(paint_count, 0);
        self.winding.mask.resize(paint_count.div_ceil(64).max(1), 0);

        let piece_count = self.pieces.len();
        let wall_count = self.wall_events.len();
        let row_count = self.row_ys.len();
        let max_y = self.max_y;
        let mut next_piece = 0;
        let mut next_wall = 0;
        let mut next_row = 0;
        let mut y = self.min_y;
        let mut forced = false;

        while next_row < row_count && self.row_ys[next_row] <= y {
            next_row += 1;
        }

        while y < max_y {
            // Events at y: winding changes on the left wall, edges that end and edges that start.
            while next_wall < wall_count && self.wall_events[next_wall].y <= y {
                let event = self.wall_events[next_wall];
                self.winding.add(event.paint, i32::from(event.delta));
                next_wall += 1;
            }
            let mut y_next = max_y;
            let mut kept = 0;
            for k in 0..self.active.len() {
                let y_bot = self.active[k].y_bot;
                if y_bot > y {
                    if kept != k {
                        self.active[kept] = self.active[k];
                    }
                    kept += 1;
                    if y_bot < y_next {
                        y_next = y_bot;
                    }
                }
            }
            self.active.truncate(kept);
            while next_piece < piece_count && self.pieces[next_piece].y_top <= y {
                let piece = self.pieces[next_piece];
                if piece.y_bot > y {
                    self.active.push(ActiveEdge {
                        x_top: piece.x_top,
                        y_top: piece.y_top,
                        x_bot: piece.x_bot,
                        y_bot: piece.y_bot,
                        dxdy: piece.dxdy,
                        xa: piece.x_top,
                        xb: piece.x_top,
                        index: next_piece as i32,
                        paint: piece.paint,
                        dir: piece.dir,
                    });
                    if piece.y_bot < y_next {
                        y_next = piece.y_bot;
                    }
                }
                next_piece += 1;
            }
            if next_row < row_count && self.row_ys[next_row] < y_next {
                y_next = self.row_ys[next_row];
            }
            if next_piece < piece_count && self.pieces[next_piece].y_top < y_next {
                y_next = self.pieces[next_piece].y_top;
            }
            if next_wall < wall_count && self.wall_events[next_wall].y < y_next {
                y_next = self.wall_events[next_wall].y;
            }

            y_next = Self::order_active_edges(&mut self.active, y, y_next);
            self.build_boundaries();
            self.connect(y, forced);
            std::mem::swap(&mut self.previous, &mut self.current);

            y = y_next;
            forced = false;
            while next_row < row_count && self.row_ys[next_row] <= y {
                forced = true;
                next_row += 1;
            }
        }

        self.current.clear();
        self.connect(max_y, true);
        self.release_scratch();
    }

    /// Sorts the active edges from left to right for the beam that starts at `y`, and shortens the
    /// beam to the first crossing inside it. Returns the bottom of the beam.
    fn order_active_edges(edges: &mut [ActiveEdge], y: f64, limit: f64) -> f64 {
        let n = edges.len();
        if n == 0 {
            return limit;
        }
        // Edges are ordered by their position at the top of the beam. Edges that start from the
        // same point (a vertex, or a crossing that ended the beam above) are ordered by slope,
        // which is their order just below that point. The list is nearly sorted already.
        #[inline(always)]
        fn before(key: &ActiveEdge, other: &ActiveEdge) -> bool {
            let d = key.xa - other.xa;
            d < -EPS_X || (d <= EPS_X && key.dxdy < other.dxdy)
        }
        for k in 1..n {
            // Most edges are in place already: look before moving anything.
            if !before(&edges[k], &edges[k - 1]) {
                continue;
            }
            let key = edges[k];
            edges[k] = edges[k - 1];
            let mut j = k - 1;
            while j > 0 && before(&key, &edges[j - 1]) {
                edges[j] = edges[j - 1];
                j -= 1;
            }
            edges[j] = key;
        }
        let mut y_next = limit;
        for edge in edges.iter_mut() {
            edge.xb = edge.x(y_next);
        }
        // Neighbours that are swapped at the bottom cross inside the beam. The first crossing in
        // the beam is always between neighbours, so the smallest one found here is the first.
        let mut cut = y_next;
        for k in 0..n - 1 {
            let db = edges[k].xb - edges[k + 1].xb;
            if db > EPS_X {
                let da = edges[k].xa - edges[k + 1].xa;
                if da < 0.0 {
                    let y_cross = y + da / (da - db) * (y_next - y);
                    if y_cross > y + EPS_Y && y_cross < y_next - EPS_Y && y_cross < cut {
                        cut = y_cross;
                    }
                }
            }
        }
        if cut < y_next {
            y_next = cut;
            for edge in edges.iter_mut() {
                edge.xb = edge.x(y_next);
            }
        }
        y_next
    }

    /// Walks the ordered edges and keeps the ones where the visible color changes.
    /// Also moves every edge on to the bottom of the beam.
    fn build_boundaries(&mut self) {
        let Sweep {
            active,
            current,
            winding,
            wall_l,
            wall_r,
            ..
        } = self;
        let (wall_l, wall_r) = (*wall_l, *wall_r);
        current.clear();
        // The left wall starts the first group. Its winding comes from the wall events.
        let mut group_a = wall_l;
        let mut group_b = wall_l;
        let mut id_lo: i32 = -1;
        let mut id_hi: i32 = -1;
        let mut is_wall = true;
        let mut last_color: u32 = 0;
        // Rounding noise must not make positions decrease from left to right.
        let mut last_a = wall_l;
        let mut last_b = wall_l;

        for edge in active.iter_mut() {
            last_a = fmin(fmax(edge.xa, last_a), wall_r);
            last_b = fmin(fmax(edge.xb, last_b), wall_r);
            // The edge has reached the bottom of this beam, which is the top of the next one.
            edge.xa = edge.xb;
            let index = edge.index;
            if (last_a - group_a).abs() <= EPS_X && (last_b - group_b).abs() <= EPS_X {
                if !is_wall {
                    id_lo = id_lo.min(index);
                    id_hi = id_hi.max(index);
                }
            } else {
                let color = winding.visible_color();
                if is_wall || color != last_color {
                    current.push(Boundary {
                        x: group_a,
                        x_end: group_b,
                        id_lo,
                        id_hi,
                        color,
                        span: SPAN_NONE,
                        node: -1,
                    });
                    last_color = color;
                }
                is_wall = false;
                group_a = last_a;
                group_b = last_b;
                id_lo = index;
                id_hi = index;
            }
            winding.add(edge.paint, i32::from(edge.dir));
        }
        let color = winding.visible_color();
        if is_wall || color != last_color {
            current.push(Boundary {
                x: group_a,
                x_end: group_b,
                id_lo,
                id_hi,
                color,
                span: SPAN_NONE,
                node: -1,
            });
        }
        // The right wall ends the strip. Edges lying on it are part of it.
        let right_wall = Boundary {
            x: wall_r,
            x_end: wall_r,
            id_lo: -2,
            id_hi: -2,
            color: 0,
            span: SPAN_NONE,
            node: -1,
        };
        let last = current.len() - 1;
        if last > 0
            && (current[last].x - wall_r).abs() <= EPS_X
            && (current[last].x_end - wall_r).abs() <= EPS_X
        {
            current[last] = right_wall;
        } else {
            current.push(right_wall);
        }

        for edge in active.iter() {
            winding.add(edge.paint, -i32::from(edge.dir));
        }
    }

    /// Connects the beam that ends at `y` (`previous`) with the beam that starts there (`current`):
    /// regions whose color and sides carry on are extended, the others are closed and new ones
    /// opened. With `forced` every region is closed, which produces a horizontal grid line.
    ///
    /// A region is only cut where its own outline makes it. Where a region of another color starts
    /// inside it, diagonals from its lowest vertex reach around the start (`split_component`), and
    /// where two regions of one color meet below a region that ends between them, both stay open
    /// as the two parts of one region until the next vertex of its outline, which a diagonal from
    /// the meeting point reaches (`merge_component`). Closing the neighbours of such a vertex with
    /// a horizontal cut would put a vertex on every edge level with it, and those vertices were
    /// most of the mesh.
    fn connect(&mut self, y: f64, forced: bool) {
        let n_p = self.previous.len();
        let n_c = self.current.len();

        // Merge the positions of both beams into one list of nodes, so that the regions above and
        // below the line share exactly the same points.
        self.nodes.clear();
        {
            let Sweep {
                previous,
                current,
                nodes,
                ..
            } = self;
            let mut i = 0;
            let mut j = 0;
            let mut last_x = f64::NEG_INFINITY;
            while i < n_p || j < n_c {
                let from_previous = if j >= n_c {
                    true
                } else if i >= n_p {
                    false
                } else {
                    previous[i].x_end <= current[j].x
                };
                let x = if from_previous {
                    previous[i].x_end
                } else {
                    current[j].x
                };
                if x > last_x + EPS_X {
                    nodes.push(Node {
                        x,
                        last_p: -1,
                        last_c: -1,
                        count_p: 0,
                        count_c: 0,
                        marked: forced,
                    });
                    last_x = x;
                }
                let node = nodes.len() - 1;
                if from_previous {
                    previous[i].node = node as i32;
                    nodes[node].last_p = i as i32;
                    nodes[node].count_p += 1;
                    i += 1;
                } else {
                    current[j].node = node as i32;
                    nodes[node].last_c = j as i32;
                    nodes[node].count_c += 1;
                    j += 1;
                }
            }
        }
        let node_count = self.nodes.len();
        if node_count == 0 {
            return;
        }

        // A region that starts at a point opens from it. One that is a point at the bottom of its
        // first beam too is nothing. With `forced` every other region opens too.
        if n_c > 1 {
            for j in 0..n_c - 1 {
                if self.current[j].color == 0 {
                    continue;
                }
                if self.current[j].node == self.current[j + 1].node {
                    if self.current[j + 1].x_end - self.current[j].x_end <= EPS_X {
                        continue;
                    }
                    self.current[j].span = SPAN_TO_OPEN;
                    let node = self.current[j].node as usize;
                    self.nodes[node].marked = true;
                } else if forced {
                    self.current[j].span = SPAN_TO_OPEN;
                }
            }
        }
        if !forced {
            // A node where the outline does not simply pass through along one edge is a vertex.
            for n in 0..node_count {
                let node = self.nodes[n];
                if node.count_p != 1 || node.count_c != 1 {
                    self.nodes[n].marked = true;
                } else {
                    let p = self.previous[node.last_p as usize];
                    let c = self.current[node.last_c as usize];
                    if p.id_lo != c.id_lo || p.id_hi != c.id_hi {
                        self.nodes[n].marked = true;
                    }
                }
            }
            self.connect_components(y);
        }

        // The regions that end here: with `forced` all of them, otherwise the ones no region below
        // carries on. Their ends are vertices already.
        if n_p > 1 {
            for i in 0..n_p - 1 {
                let p = self.previous[i];
                if p.span >= 0 {
                    let to = self.previous[i + 1].node as usize;
                    self.close_span(p.span, p.node as usize, to, y);
                }
            }
        }
        // The regions that start here.
        if n_c > 1 {
            for j in 0..n_c - 1 {
                let c = self.current[j];
                if c.span == SPAN_TO_OPEN {
                    let mut flags = 0u8;
                    if j == 0 {
                        flags |= PolyHeader::LEFT_ON_WALL;
                    }
                    if j + 1 == n_c - 1 {
                        flags |= PolyHeader::RIGHT_ON_WALL;
                    }
                    let to = self.current[j + 1].node as usize;
                    self.current[j].span = self.open_span(c.color, flags, c.node as usize, to, y);
                }
            }
        }

        if self.nodes[0].marked {
            self.left_marks.push(y);
        }
        if self.nodes[node_count - 1].marked {
            self.right_marks.push(y);
        }
    }

    // Components

    /// Groups the spans of one color that overlap across the event line into components, decides
    /// for each how its regions carry on, and carries them on.
    ///
    /// A span that carries on keeps its index in `current[j].span`. A current span that opens is
    /// left at `SPAN_TO_OPEN` and a previous span that closes keeps its index, for `connect` to
    /// finish.
    fn connect_components(&mut self, y: f64) {
        let n_p = self.previous.len();
        let n_c = self.current.len();
        self.components.clear();
        self.previous_in_component.clear();
        self.previous_in_component.resize(n_p, false);

        // The previous spans of a current span's color under it are its component, and a previous
        // span under two current spans joins their components.
        {
            let Sweep {
                previous,
                current,
                components,
                previous_in_component,
                ..
            } = self;
            let mut i = 0;
            for j in 0..n_c.saturating_sub(1) {
                let color = current[j].color;
                if color == 0 {
                    continue;
                }
                let cl = current[j].node;
                let cr = current[j + 1].node;
                if cl == cr {
                    continue;
                }
                while i + 1 < n_p && previous[i + 1].node <= cl {
                    i += 1;
                }
                let mut first: Option<usize> = None;
                let mut last = 0;
                let mut count: i32 = 0;
                let mut k = i;
                while k + 1 < n_p && previous[k].node < cr {
                    // A span that ends in a point carries nothing on.
                    if previous[k].span >= 0
                        && previous[k].color == color
                        && previous[k].node != previous[k + 1].node
                    {
                        if first.is_none() {
                            first = Some(k);
                        }
                        last = k;
                        count += 1;
                    }
                    k += 1;
                }
                let Some(first) = first else {
                    current[j].span = SPAN_TO_OPEN;
                    continue;
                };
                for i in first..=last {
                    if previous[i].color == color {
                        previous_in_component[i] = true;
                    }
                }
                match components.last_mut() {
                    Some(c) if c.color == color && c.last_prev == first => {
                        c.last_cur = j;
                        c.cur_count += 1;
                        c.last_prev = last;
                        c.prev_count += count - 1;
                    }
                    _ => components.push(Component {
                        color,
                        first_prev: first,
                        last_prev: last,
                        first_cur: j,
                        last_cur: j,
                        prev_count: count,
                        cur_count: 1,
                        kind: ComponentKind::Fallback,
                    }),
                }
            }

            // The ends of a span that opens or closes on its own are vertices.
            let nodes = &mut self.nodes;
            for i in 0..n_p.saturating_sub(1) {
                if previous[i].span >= 0 && !previous_in_component[i] {
                    nodes[previous[i].node as usize].marked = true;
                    nodes[previous[i + 1].node as usize].marked = true;
                }
            }
            for j in 0..n_c.saturating_sub(1) {
                if current[j].span == SPAN_TO_OPEN {
                    nodes[current[j].node as usize].marked = true;
                    nodes[current[j + 1].node as usize].marked = true;
                }
            }
        }

        // The kind of every component, and the vertices it makes: the ends of the spans a
        // component closes and opens the plain way, and the corners where a side steps along
        // the line. Every other vertex is one already, as the end of a boundary.
        for c in 0..self.components.len() {
            let component = self.components[c];
            let kind = self.classify(&component);
            self.components[c].kind = kind;
            let pl = self.previous[self
                .next_previous(&component, None)
                .expect("component without a previous span")]
            .node as usize;
            let pr = self.previous[component.last_prev + 1].node as usize;
            let cl = self.current[self
                .next_current(&component, None)
                .expect("component without a current span")]
            .node as usize;
            let cr = self.current[component.last_cur + 1].node as usize;
            let Sweep {
                previous,
                current,
                nodes,
                ..
            } = self;
            match kind {
                ComponentKind::Fallback => {
                    for i in component.first_prev..=component.last_prev {
                        if previous[i].span >= 0 && previous[i].color == component.color {
                            nodes[previous[i].node as usize].marked = true;
                            nodes[previous[i + 1].node as usize].marked = true;
                        }
                    }
                    for j in component.first_cur..=component.last_cur {
                        if current[j].color == component.color
                            && current[j].node != current[j + 1].node
                        {
                            nodes[current[j].node as usize].marked = true;
                            nodes[current[j + 1].node as usize].marked = true;
                        }
                    }
                }
                _ => {
                    if pl != cl {
                        nodes[pl].marked = true;
                        nodes[cl].marked = true;
                    }
                    if pr != cr {
                        nodes[pr].marked = true;
                        nodes[cr].marked = true;
                    }
                }
            }
        }

        for c in 0..self.components.len() {
            let component = self.components[c];
            match component.kind {
                ComponentKind::Continuation => self.continue_component(&component, y),
                ComponentKind::Split => self.split_component(&component, y),
                ComponentKind::Merge => self.merge_component(&component, y),
                ComponentKind::Fallback => {
                    // Closed and opened the plain way by `connect`.
                    for j in component.first_cur..=component.last_cur {
                        if self.current[j].color == component.color
                            && self.current[j].node != self.current[j + 1].node
                        {
                            self.current[j].span = SPAN_TO_OPEN;
                        }
                    }
                }
            }
        }
    }

    /// The previous span of the component after `after` (`None` for the first), or `None`.
    #[inline]
    fn next_previous(&self, component: &Component, after: Option<usize>) -> Option<usize> {
        let start = after.map_or(component.first_prev, |a| a + 1);
        (start..=component.last_prev)
            .find(|&i| self.previous[i].span >= 0 && self.previous[i].color == component.color)
    }

    /// The current span of the component after `after` (`None` for the first), or `None`.
    #[inline]
    fn next_current(&self, component: &Component, after: Option<usize>) -> Option<usize> {
        let start = after.map_or(component.first_cur, |a| a + 1);
        (start..=component.last_cur).find(|&j| {
            self.current[j].color == component.color
                && self.current[j].node != self.current[j + 1].node
        })
    }

    /// The first previous and the first current span of a component, which always exist.
    #[inline]
    fn first_spans(&self, component: &Component) -> (usize, usize) {
        let i = self
            .next_previous(component, None)
            .expect("component without a previous span");
        let j = self
            .next_current(component, None)
            .expect("component without a current span");
        (i, j)
    }

    fn classify(&self, component: &Component) -> ComponentKind {
        let (first_prev, first_cur) = self.first_spans(component);
        let pl = self.previous[first_prev].node;
        let pr = self.previous[component.last_prev + 1].node;
        let cl = self.current[first_cur].node;
        let cr = self.current[component.last_cur + 1].node;
        if component.prev_count == 1 && component.cur_count == 1 {
            // The same sides, or both sides stepped right, which both chains can do: a region's
            // left side narrows that way and its right side widens.
            if (pl == cl && pr == cr) || (pl < cl && pr < cr) {
                return ComponentKind::Continuation;
            }
            if cl >= pl && cr <= pr {
                return ComponentKind::Split;
            }
            if cl <= pl && cr >= pr {
                return ComponentKind::Merge;
            }
            return ComponentKind::Fallback;
        }
        if component.prev_count == 1 {
            return if cl >= pl && cr <= pr {
                ComponentKind::Split
            } else {
                ComponentKind::Fallback
            };
        }
        if component.cur_count == 1 {
            return if cl <= pl && cr >= pr {
                ComponentKind::Merge
            } else {
                ComponentKind::Fallback
            };
        }
        ComponentKind::Fallback
    }

    /// One previous span carries on as one current span with the same sides, or with both sides
    /// stepped to the right along the line.
    fn continue_component(&mut self, component: &Component, y: f64) {
        let (i, j) = self.first_spans(component);
        let pl = self.previous[i].node as usize;
        let pr = self.previous[i + 1].node as usize;
        let cl = self.current[j].node as usize;
        let cr = self.current[j + 1].node as usize;
        let mut span = self.previous[i].span;
        let left_wall = j == 0;
        let right_wall = j + 1 == self.current.len() - 1;
        for n in pl..=cl {
            if !self.nodes[n].marked {
                continue;
            }
            if self.spans[span as usize].partner >= 0 {
                span = self.resolve_left(span, n, y, left_wall);
            } else {
                self.append_left(span, self.nodes[n].x, y, left_wall);
            }
        }
        for n in pr..=cr {
            if !self.nodes[n].marked {
                continue;
            }
            if self.spans[span as usize].partner >= 0 {
                span = self.resolve_right(span, n, y, right_wall);
            } else {
                self.append_right(span, self.nodes[n].x, y, right_wall);
            }
        }
        self.current[j].span = span;
        self.previous[i].span = SPAN_CONTINUED;
    }

    /// One previous span carries on as several current spans with regions of other colors starting
    /// between them, or as one narrower current span.
    ///
    /// The beam of the previous span below its lowest vertex is a trapezoid, so a diagonal from that
    /// vertex reaches every point of the line inside the span. The span is cut by such diagonals
    /// into the parts that carry on, one for each current span, and the fans over the starts
    /// between them. The part on the side of the lowest vertex is the previous span itself, the
    /// part on the other side is a new span that starts at the vertex. When the span is two parts
    /// waiting at a meeting point, that point is the lowest vertex and the parts are the two.
    fn split_component(&mut self, component: &Component, y: f64) {
        let i = self
            .next_previous(component, None)
            .expect("component without a previous span");
        let a = self.previous[i].node as usize;
        let b = self.previous[i + 1].node as usize;
        let color = component.color;
        let span = self.previous[i].span;
        let left: i32;
        let right: i32;
        let vx: f64;
        let vy: f64;
        if self.spans[span as usize].partner >= 0 {
            let s = &mut self.spans[span as usize];
            left = span;
            right = s.partner;
            vx = s.meet_x;
            vy = s.meet_y;
            s.partner = -1;
        } else {
            // The lowest vertex of the span. The side below it goes to the new span, with the wall
            // if the side runs along one there, and the diagonal takes its place on the old span.
            let open = self.spans[span as usize];
            let none = ChainPoint {
                x: 0.0,
                y: f64::NEG_INFINITY,
                next: -1,
                wall_after: false,
            };
            let left_tail = if open.left_tail >= 0 {
                self.chain[open.left_tail as usize]
            } else {
                none
            };
            let right_tail = if open.right_tail >= 0 {
                self.chain[open.right_tail as usize]
            } else {
                none
            };
            if open.right_tail >= 0 && right_tail.y >= left_tail.y {
                left = span;
                vx = right_tail.x;
                vy = right_tail.y;
                let flags = if right_tail.wall_after {
                    PolyHeader::RIGHT_ON_WALL
                } else {
                    0
                };
                right = self.open_point_span(color, flags, vx, vy);
                self.chain[open.right_tail as usize].wall_after = false;
            } else if open.left_tail >= 0 {
                right = span;
                vx = left_tail.x;
                vy = left_tail.y;
                let flags = if left_tail.wall_after {
                    PolyHeader::LEFT_ON_WALL
                } else {
                    0
                };
                left = self.open_point_span(color, flags, vx, vy);
                self.chain[open.left_tail as usize].wall_after = false;
            } else {
                // No vertex on either side: the top-right corner.
                left = span;
                vx = self.top_pool[(open.top_start + open.top_count - 1) as usize];
                vy = open.y_top;
                right = self.open_point_span(color, open.flags & PolyHeader::RIGHT_ON_WALL, vx, vy);
                self.spans[span as usize].flags &= !PolyHeader::RIGHT_ON_WALL;
            }
        }
        self.previous[i].span = SPAN_CONTINUED;

        let first_cur = self
            .next_current(component, None)
            .expect("component without a current span");
        let last_cur = component.last_cur;
        let first_left = self.current[first_cur].node as usize;
        let first_right = self.current[first_cur + 1].node as usize;
        let last_left = self.current[last_cur].node as usize;
        let last_right = self.current[last_cur + 1].node as usize;
        let n_c = self.current.len();
        let mut left_owns: Option<usize> = None;
        let mut right_owns: Option<usize> = None;
        if first_left > a {
            self.close_span(left, a, first_left, y);
        } else {
            left_owns = Some(first_cur);
            self.append_right(left, self.nodes[first_right].x, y, first_cur + 1 == n_c - 1);
            if self.nodes[a].marked {
                self.append_left(left, self.nodes[a].x, y, first_cur == 0);
            }
            self.current[first_cur].span = left;
        }
        if last_right < b {
            self.close_span(right, last_right, b, y);
        } else if Some(last_cur) != left_owns {
            right_owns = Some(last_cur);
            self.append_left(right, self.nodes[last_left].x, y, last_cur == 0);
            if self.nodes[b].marked {
                self.append_right(right, self.nodes[b].x, y, last_cur + 1 == n_c - 1);
            }
            self.current[last_cur].span = right;
        }
        // The current spans between start at the vertex, as do the fans over the starts between
        // the current spans.
        let mut next = Some(first_cur);
        let mut previous_right: Option<usize> = None;
        while let Some(j) = next {
            let cl = self.current[j].node as usize;
            let cr = self.current[j + 1].node as usize;
            if let Some(pr) = previous_right
                && cl > pr
            {
                let fan = self.open_point_span(color, 0, vx, vy);
                self.close_span(fan, pr, cl, y);
            }
            if Some(j) != left_owns && Some(j) != right_owns {
                let part = self.open_point_span(color, 0, vx, vy);
                self.append_left(part, self.nodes[cl].x, y, j == 0);
                self.append_right(part, self.nodes[cr].x, y, j + 1 == n_c - 1);
                self.current[j].span = part;
            }
            previous_right = Some(cr);
            next = self.next_current(component, Some(j));
        }
    }

    /// Several previous spans carry on as one current span: the regions of other colors between
    /// them end here, and so may regions on either side of them. The first span goes on as the left
    /// part, or a new span whose top covers the bottoms of the regions that ended left of it, and
    /// the last span goes on as the right part, stepping right along the line to the right end.
    /// The parts meet at the left end of the last span, and the next vertex below settles them.
    /// With one previous span that starts at the left end there is one part, which steps right.
    fn merge_component(&mut self, component: &Component, y: f64) {
        let (first, j) = self.first_spans(component);
        let a = self.current[j].node as usize;
        let b = self.current[j + 1].node as usize;
        let mut last = first;
        while let Some(next) = self.next_previous(component, Some(last)) {
            last = next;
        }
        let first_left = self.previous[first].node as usize;
        let first_right = self.previous[first + 1].node as usize;
        let last_left = self.previous[last].node as usize;
        let last_right = self.previous[last + 1].node as usize;

        let left_wall = j == 0;
        let right_wall = j + 1 == self.current.len() - 1;
        if first == last && first_left == a {
            // One part. Its right side steps to the right end.
            let mut span = self.previous[first].span;
            let mut from = first_right;
            if self.spans[span as usize].partner >= 0 {
                span = self.resolve_right(span, first_right, y, first_right == b && right_wall);
                from = first_right + 1;
            }
            if self.nodes[a].marked {
                self.append_left(span, self.nodes[a].x, y, left_wall);
            }
            for n in from..=b {
                if n > first_right || self.nodes[n].marked {
                    self.append_right(span, self.nodes[n].x, y, n == b && right_wall);
                }
            }
            self.previous[first].span = SPAN_CONTINUED;
            self.current[j].span = span;
            return;
        }

        // The left part.
        let mut left: i32;
        let from: usize;
        if first_left == a {
            left = self.previous[first].span;
            let mut start = first_right;
            if self.spans[left as usize].partner >= 0 {
                left = self.resolve_right(left, first_right, y, false);
                start = first_right + 1;
            }
            from = start;
            if self.nodes[a].marked {
                self.append_left(left, self.nodes[a].x, y, left_wall);
            }
            self.previous[first].span = SPAN_CONTINUED;
        } else {
            let flags = if j == 0 { PolyHeader::LEFT_ON_WALL } else { 0 };
            left = self.open_span(component.color, flags, a, last_left, y);
            from = last_left + 1;
        }
        // The spans between close, and the first when a new span took its place.
        let mut i = if first_left == a {
            self.next_previous(component, Some(first))
        } else {
            Some(first)
        };
        while let Some(k) = i {
            if k == last {
                break;
            }
            let p = self.previous[k];
            let to = self.previous[k + 1].node as usize;
            self.close_span(p.span, p.node as usize, to, y);
            self.previous[k].span = SPAN_CONTINUED;
            i = self.next_previous(component, Some(k));
        }
        for n in from..=last_left {
            self.append_right(left, self.nodes[n].x, y, false);
        }
        // The right part, meeting the left one at its left end.
        let mut right = self.previous[last].span;
        if self.spans[right as usize].partner >= 0 {
            right = self.resolve_left(right, last_left, y, false);
        } else {
            self.append_left(right, self.nodes[last_left].x, y, false);
        }
        for n in last_right..=b {
            if n > last_right || self.nodes[n].marked {
                self.append_right(right, self.nodes[n].x, y, n == b && right_wall);
            }
        }
        self.previous[last].span = SPAN_CONTINUED;
        let meet_x = self.nodes[last_left].x;
        let s = &mut self.spans[left as usize];
        s.partner = right;
        s.meet_x = meet_x;
        s.meet_y = y;
        self.current[j].span = left;
    }

    // Spans

    /// The diagonal from the meeting point of the two parts of `span` to the vertex at `node` on
    /// its left side: the left part closes there and the right part goes on with the vertex on its
    /// left side. Returns the right part.
    fn resolve_left(&mut self, span: i32, node: usize, y: f64, wall_after: bool) -> i32 {
        let partner = self.spans[span as usize].partner;
        self.spans[span as usize].partner = -1;
        self.close_span(span, node, node, y);
        self.append_left(partner, self.nodes[node].x, y, wall_after);
        partner
    }

    /// The diagonal from the meeting point to the vertex at `node` on the right side: the right
    /// part closes there and the left part goes on with the vertex on its right side. Returns the
    /// left part.
    fn resolve_right(&mut self, span: i32, node: usize, y: f64, wall_after: bool) -> i32 {
        let partner = self.spans[span as usize].partner;
        self.spans[span as usize].partner = -1;
        self.close_span(partner, node, node, y);
        self.append_right(span, self.nodes[node].x, y, wall_after);
        span
    }

    fn append_left(&mut self, span: i32, x: f64, y: f64, wall_after: bool) {
        self.chain.push(ChainPoint {
            x,
            y,
            next: -1,
            wall_after,
        });
        let point = (self.chain.len() - 1) as i32;
        let s = &mut self.spans[span as usize];
        if s.left_tail >= 0 {
            self.chain[s.left_tail as usize].next = point;
        } else {
            s.left_head = point;
        }
        s.left_tail = point;
        s.left_count += 1;
    }

    fn append_right(&mut self, span: i32, x: f64, y: f64, wall_after: bool) {
        self.chain.push(ChainPoint {
            x,
            y,
            next: -1,
            wall_after,
        });
        let point = (self.chain.len() - 1) as i32;
        let s = &mut self.spans[span as usize];
        if s.right_tail >= 0 {
            self.chain[s.right_tail as usize].next = point;
        } else {
            s.right_head = point;
        }
        s.right_tail = point;
        s.right_count += 1;
    }

    fn open_span(&mut self, color: u32, flags: u8, from: usize, to: usize, y: f64) -> i32 {
        let top_start = self.top_pool.len() as u32;
        for n in from..=to {
            self.top_pool.push(self.nodes[n].x);
        }
        let top_count = (to + 1).saturating_sub(from) as u32;
        self.store(OpenSpan::new(color, flags, y, top_start, top_count))
    }

    /// A span that starts at one point above the current line.
    fn open_point_span(&mut self, color: u32, flags: u8, x: f64, y: f64) -> i32 {
        let top_start = self.top_pool.len() as u32;
        self.top_pool.push(x);
        self.store(OpenSpan::new(color, flags, y, top_start, 1))
    }

    fn store(&mut self, span: OpenSpan) -> i32 {
        if let Some(free) = self.free_spans.pop() {
            self.spans[free as usize] = span;
            return free;
        }
        self.spans.push(span);
        (self.spans.len() - 1) as i32
    }

    /// Closes a span with the nodes `from` through `to` as its bottom. Two parts waiting at a
    /// meeting point close as two polygons, cut by the diagonal from that point to the bottom node
    /// nearest to it.
    fn close_span(&mut self, index: i32, from: usize, to: usize, y: f64) {
        let partner = self.spans[index as usize].partner;
        if partner >= 0 {
            self.spans[index as usize].partner = -1;
            let meet_x = self.spans[index as usize].meet_x;
            let mut nearest = from;
            for n in from..=to {
                if (self.nodes[n].x - meet_x).abs() < (self.nodes[nearest].x - meet_x).abs() {
                    nearest = n;
                }
            }
            self.close_single_part(index, from, nearest, y);
            self.close_single_part(partner, nearest, to, y);
            return;
        }
        self.close_single_part(index, from, to, y);
    }

    fn close_single_part(&mut self, index: i32, from: usize, to: usize, y: f64) {
        let span = self.spans[index as usize];
        self.free_spans.push(index);
        let bottom_count = (to + 1).saturating_sub(from);
        // A region that starts and ends in a single point with straight sides has no area.
        if span.top_count == 1 && bottom_count == 1 && span.left_count == 0 && span.right_count == 0
        {
            return;
        }
        let Sweep {
            poly_data,
            top_pool,
            nodes,
            chain,
            polys,
            ..
        } = self;
        let offset = poly_data.len();
        let top_start = span.top_start as usize;
        poly_data.extend_from_slice(&top_pool[top_start..top_start + span.top_count as usize]);
        poly_data.extend(nodes.iter().take(to + 1).skip(from).map(|node| node.x));
        for head in [span.left_head, span.right_head] {
            let mut point = head;
            while point >= 0 {
                let p = chain[point as usize];
                poly_data.push(p.x);
                poly_data.push(p.y);
                poly_data.push(if p.wall_after { 1.0 } else { 0.0 });
                point = p.next;
            }
        }
        polys.push(PolyHeader {
            color: span.color,
            flags: span.flags,
            y_top: span.y_top,
            y_bot: y,
            offset: offset as u32,
            top_count: span.top_count,
            bottom_count: bottom_count as u32,
            left_count: span.left_count,
            right_count: span.right_count,
        });
    }
}
