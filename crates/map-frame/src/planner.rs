// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The frame's tile placements, a port of the Swift `TilePlacementPlanner`.
//! A resident target
//! draws itself. A target not yet resident draws what the last frame showed
//! on its ground, else its resident descendants, else its finest resident
//! ancestor above the backdrop's zoom.
//!
//! `plan` takes native values. `plan_flat` takes the same as flat `i32`
//! arrays, for a host that calls across a binding:
//! - targets: z, x, y, wrap, lines, labels, raster size (-1 for none);
//! - resident: z, x, y;
//! - previous and the result: one `PLACEMENT_STRIDE` record per placement.
//!
//! On the relief the terrain a tile is placed with comes from the host's
//! `Relief`, asked only when the relief is on.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

/// Ints per placement record: source z x y, placed-in z x y wrap, slot z x y
/// wrap, lines, labels, raster, terrain z x y, displacement z x y (z -1 for none).
pub const PLACEMENT_STRIDE: usize = 20;
pub const TARGET_STRIDE: usize = 7;

#[derive(Default)]
struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0.rotate_left(5) ^ *b as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }
    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0 ^ v).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

type Map<K, V> = HashMap<K, V, BuildHasherDefault<KeyHasher>>;
type Set<K> = HashSet<K, BuildHasherDefault<KeyHasher>>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct T {
    pub z: i32,
    pub x: i32,
    pub y: i32,
}

impl T {
    fn key(self) -> u64 {
        ((self.z as u64) << 58) | ((self.x as u64 & 0x1FFF_FFFF) << 29) | (self.y as u64 & 0x1FFF_FFFF)
    }
    fn parent_at(self, z: i32) -> Option<T> {
        if z > self.z || z < 0 {
            return None;
        }
        let d = self.z - z;
        Some(T { z, x: self.x >> d, y: self.y >> d })
    }
    fn children(self) -> [T; 4] {
        let (x, y, z) = (self.x * 2, self.y * 2, self.z + 1);
        [T { z, x, y }, T { z, x: x + 1, y }, T { z, x, y: y + 1 }, T { z, x: x + 1, y: y + 1 }]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot {
    pub tile: T,
    pub wrap: i32,
}

impl Slot {
    fn key(self) -> u64 {
        self.tile.key() ^ (self.wrap as u64).wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rules {
    pub lines: bool,
    pub labels: bool,
    /// The texture size the ground draws from, -1 for the geometry.
    pub raster: i32,
}

impl Rules {
    const ALL: Rules = Rules { lines: true, labels: true, raster: -1 };

    /// The finer of two rules, switch by switch: geometry over a texture, the larger texture over the smaller.
    fn finer(self, other: Rules) -> Rules {
        let raster = if self.raster < 0 || other.raster < 0 { -1 } else { self.raster.max(other.raster) };
        Rules { lines: self.lines || other.lines, labels: self.labels || other.labels, raster }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Placement {
    pub source: T,
    pub place_in: Slot,
    pub slot: Slot,
    pub rules: Rules,
    pub terrain: Option<T>,
    pub displacement: Option<T>,
}

/// What a resident tile in a slot of a zoom needs to be ready.
pub trait Relief {
    /// The terrain and displacement tiles a tile is placed with in a slot of the zoom, `None` off the relief.
    fn terrain(&mut self, tile: T, slot_zoom: i32) -> Option<(T, T)>;
    fn terrain_resident(&mut self, tile: T) -> bool;
}

pub struct NoRelief;

impl Relief for NoRelief {
    fn terrain(&mut self, _: T, _: i32) -> Option<(T, T)> {
        None
    }
    fn terrain_resident(&mut self, _: T) -> bool {
        true
    }
}

pub struct PlannerInput<'a> {
    pub targets: &'a [(Slot, Rules)],
    pub resident: &'a [T],
    /// The backdrop's zoom, `None` without one.
    pub backdrop_zoom: Option<i32>,
    pub descendant_search_depth: i32,
    pub previous: &'a [Placement],
}

struct Ready {
    terrain: Option<T>,
    displacement: Option<T>,
}

/// The planner over native values.
pub fn plan(input: &PlannerInput<'_>, relief: &mut dyn Relief) -> Vec<Placement> {
    let resident: Set<u64> = input.resident.iter().map(|t| t.key()).collect();
    let mut target_rules: Map<u64, Rules> = Map::default();
    for (slot, rules) in input.targets {
        target_rules.entry(slot.key()).or_insert(*rules);
    }
    let slot_rules = |slot: Slot| target_rules.get(&slot.key()).copied().unwrap_or(Rules::ALL);
    // A stand-in that is a target itself draws by its own slot's rules.
    let rules = |source: T, slot: Slot| {
        let own = Slot { tile: source, wrap: slot.wrap };
        if target_rules.contains_key(&own.key()) { slot_rules(own) } else { slot_rules(slot) }
    };
    let ready = |tile: T, slot_zoom: i32, relief: &mut dyn Relief| -> Option<Ready> {
        if !resident.contains(&tile.key()) {
            return None;
        }
        match relief.terrain(tile, slot_zoom) {
            None => Some(Ready { terrain: None, displacement: None }),
            Some((terrain, displacement)) => {
                // The displacement is asked only when the terrain is in, as
                // in the Swift planner: the asking gathers the missing terrain.
                if !relief.terrain_resident(terrain) || !relief.terrain_resident(displacement) {
                    return None;
                }
                Some(Ready { terrain: Some(terrain), displacement: Some(displacement) })
            }
        }
    };
    // The ancestors of every resident tile: the descendant search descends only into branches that hold something.
    let mut branches: Set<u64> = Set::default();
    for &tile in input.resident {
        let mut t = tile;
        while branches.insert(t.key()) && t.z > 0 {
            t = t.parent_at(t.z - 1).expect("a parent");
        }
    }
    let useful = |t: T| input.backdrop_zoom.is_none_or(|b| t.z > b);

    let mut out: Vec<Placement> = Vec::new();
    let mut seen: Set<(u64, u64)> = Set::default();
    let mut append = |p: Placement, out: &mut Vec<Placement>| {
        if seen.insert((p.source.key(), p.place_in.key())) {
            out.push(p);
        }
    };

    // What the last frame showed and is still resident, by the ground it
    // stood on and under every ancestor of that ground, in its order.
    let mut by_ground: Map<u64, Placement> = Map::default();
    let mut inside: Map<u64, Vec<Placement>> = Map::default();
    for shown in input.previous {
        if !resident.contains(&shown.source.key()) || !useful(shown.source) {
            continue;
        }
        if shown.terrain.is_some_and(|t| !relief.terrain_resident(t)) || shown.displacement.is_some_and(|t| !relief.terrain_resident(t)) {
            continue;
        }
        by_ground.entry(shown.place_in.key()).or_insert(*shown);
        for z in (0..shown.place_in.tile.z).rev() {
            let a = Slot { tile: shown.place_in.tile.parent_at(z).expect("an ancestor"), wrap: shown.place_in.wrap };
            inside.entry(a.key()).or_default().push(*shown);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn descendants(
        tile: T,
        slot: Slot,
        depth: i32,
        ready: &dyn Fn(T, i32, &mut dyn Relief) -> Option<Ready>,
        rules: &dyn Fn(T, Slot) -> Rules,
        branches: &Set<u64>,
        relief: &mut dyn Relief,
        found: &mut Vec<Placement>,
    ) -> bool {
        if let Some(r) = ready(tile, slot.tile.z, relief) {
            found.push(Placement { source: tile, place_in: Slot { tile, wrap: slot.wrap }, slot, rules: rules(tile, slot), terrain: r.terrain, displacement: r.displacement });
            return true;
        }
        if depth <= 0 || !branches.contains(&tile.key()) {
            return false;
        }
        let mut complete = true;
        for child in tile.children() {
            complete &= descendants(child, slot, depth - 1, ready, rules, branches, relief, found);
        }
        complete
    }

    let empty: Vec<Placement> = Vec::new();
    for &(target, target_own_rules) in input.targets {
        let source = target.tile;
        if let Some(own) = ready(source, source.z, relief) {
            append(Placement { source, place_in: target, slot: target, rules: target_own_rules, terrain: own.terrain, displacement: own.displacement }, &mut out);
            continue;
        }
        // What stood on this ground last frame, by the ground it stood on.
        let shown_inside = inside.get(&target.key()).unwrap_or(&empty);
        let mut shown_over: Option<Placement> = None;
        for z in (0..=source.z).rev() {
            let ground = Slot { tile: source.parent_at(z).expect("an ancestor"), wrap: target.wrap };
            if let Some(p) = by_ground.get(&ground.key()) {
                shown_over = Some(*p);
                break;
            }
        }
        for shown in shown_inside {
            append(
                Placement { source: shown.source, place_in: shown.place_in, slot: target, rules: shown.rules.finer(rules(shown.source, target)), terrain: shown.terrain, displacement: shown.displacement },
                &mut out,
            );
        }
        if let Some(over) = shown_over {
            append(Placement { source: over.source, place_in: target, slot: target, rules: over.rules.finer(rules(over.source, target)), terrain: over.terrain, displacement: over.displacement }, &mut out);
            continue;
        }
        // Ground nothing was shown on: the descendants first where nothing at all was shown.
        if shown_inside.is_empty() {
            let mut found = Vec::new();
            let complete = input.descendant_search_depth > 0
                && branches.contains(&source.key())
                && descendants(source, target, input.descendant_search_depth, &ready, &rules, &branches, relief, &mut found);
            let any = !found.is_empty();
            for p in found {
                append(p, &mut out);
            }
            if complete && any {
                continue;
            }
        }
        // The finest resident ancestor, drawn under them.
        for z in (0..source.z).rev() {
            let ancestor = source.parent_at(z).expect("an ancestor");
            if !useful(ancestor) {
                break;
            }
            if let Some(r) = ready(ancestor, source.z, relief) {
                append(Placement { source: ancestor, place_in: target, slot: target, rules: rules(ancestor, target), terrain: r.terrain, displacement: r.displacement }, &mut out);
                break;
            }
        }
    }
    out
}

fn read_tile(c: &[i32]) -> Option<T> {
    (c[0] >= 0).then_some(T { z: c[0], x: c[1], y: c[2] })
}

fn read_placement(c: &[i32]) -> Placement {
    Placement {
        source: T { z: c[0], x: c[1], y: c[2] },
        place_in: Slot { tile: T { z: c[3], x: c[4], y: c[5] }, wrap: c[6] },
        slot: Slot { tile: T { z: c[7], x: c[8], y: c[9] }, wrap: c[10] },
        rules: Rules { lines: c[11] != 0, labels: c[12] != 0, raster: c[13] },
        terrain: read_tile(&c[14..17]),
        displacement: read_tile(&c[17..20]),
    }
}

fn write_placement(p: &Placement, out: &mut Vec<i32>) {
    let t = |t: Option<T>| t.map_or([-1, 0, 0], |t| [t.z, t.x, t.y]);
    out.extend_from_slice(&[p.source.z, p.source.x, p.source.y, p.place_in.tile.z, p.place_in.tile.x, p.place_in.tile.y, p.place_in.wrap]);
    out.extend_from_slice(&[p.slot.tile.z, p.slot.tile.x, p.slot.tile.y, p.slot.wrap, p.rules.lines as i32, p.rules.labels as i32, p.rules.raster]);
    out.extend_from_slice(&t(p.terrain));
    out.extend_from_slice(&t(p.displacement));
}

/// The planner over flat arrays (the module's comment states the records).
/// `backdrop_zoom` and `descendant_search_depth` are -1 for none and for
/// unbounded. `relief` is `None` off the relief.
pub fn plan_flat(targets: &[i32], resident: &[i32], previous: &[i32], backdrop_zoom: i32, descendant_search_depth: i32, relief: Option<&mut dyn Relief>) -> Vec<i32> {
    let targets: Vec<(Slot, Rules)> = targets
        .as_chunks::<TARGET_STRIDE>()
        .0
        .iter()
        .map(|c| (Slot { tile: T { z: c[0], x: c[1], y: c[2] }, wrap: c[3] }, Rules { lines: c[4] != 0, labels: c[5] != 0, raster: c[6] }))
        .collect();
    let resident: Vec<T> = resident.as_chunks::<3>().0.iter().map(|c| T { z: c[0], x: c[1], y: c[2] }).collect();
    let previous: Vec<Placement> = previous.as_chunks::<PLACEMENT_STRIDE>().0.iter().map(|c| read_placement(c)).collect();
    let input = PlannerInput {
        targets: &targets,
        resident: &resident,
        backdrop_zoom: (backdrop_zoom >= 0).then_some(backdrop_zoom),
        descendant_search_depth: if descendant_search_depth < 0 { i32::MAX } else { descendant_search_depth },
        previous: &previous,
    };
    let placed = match relief {
        Some(relief) => plan(&input, relief),
        None => plan(&input, &mut NoRelief),
    };
    let mut out = Vec::with_capacity(placed.len() * PLACEMENT_STRIDE);
    for p in &placed {
        write_placement(p, &mut out);
    }
    out
}
