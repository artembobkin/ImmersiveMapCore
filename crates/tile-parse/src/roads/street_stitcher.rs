// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Stitches the pieces of one street back into one polyline before
//! tessellation, a port of the Swift `RoadStreetStitcher`.
//!
//! A street often arrives as several features. Drawn as separate ribbons,
//! every boundary between two pieces is a seam. Pieces join end to end when,
//! and only when, they are the same street to draw (equal stitching keys,
//! the schema reading's answer), meeting at an endpoint no third road
//! shares. A node where a third road meets is a junction, never glued
//! across. The first feature of a chain receives the whole polyline, the
//! others nothing, so the per-feature bookkeeping stays intact.

use std::collections::{HashMap, HashSet};

use super::RoadConnectionPointKey;
use crate::geom::V2;

/// What the stitcher needs of a feature: its stitching key (pieces with
/// equal keys stitch, `None` passes through) and whether it is a road of the
/// automobile tier.
#[derive(Debug, Clone, Copy)]
pub struct StitchFacts<'a> {
    pub stitching_key: Option<&'a str>,
    pub is_automobile: bool,
}

pub fn stitch(lines_by_feature: Vec<Vec<Vec<V2>>>, facts: &[StitchFacts<'_>]) -> Vec<Vec<Vec<V2>>> {
    let count = lines_by_feature.len();
    if count <= 1 {
        return lines_by_feature;
    }
    let mut identity: Vec<Option<&str>> = vec![None; count];
    let mut participates = false;
    for i in 0..count {
        if lines_by_feature[i].is_empty() || !facts[i].is_automobile {
            continue;
        }
        if let Some(key) = facts[i].stitching_key {
            identity[i] = Some(key);
            participates = true;
        }
    }
    if !participates {
        return lines_by_feature;
    }

    struct Piece {
        feature: usize,
        points: Vec<V2>,
        consumed: bool,
    }
    let mut pieces: Vec<Piece> = Vec::new();
    for (i, lines) in lines_by_feature.iter().enumerate() {
        if identity[i].is_some() && lines.len() == 1 && lines[0].len() >= 2 {
            pieces.push(Piece { feature: i, points: lines[0].clone(), consumed: false });
        }
    }
    if pieces.len() <= 1 {
        return lines_by_feature;
    }
    // How many distinct drive-tier features touch each endpoint: a T with an
    // unnamed service road is still a T.
    let mut features_at_point: HashMap<RoadConnectionPointKey, HashSet<usize>> = HashMap::new();
    for (i, lines) in lines_by_feature.iter().enumerate() {
        if !facts[i].is_automobile {
            continue;
        }
        for line in lines {
            if let (Some(&first), Some(&last)) = (line.first(), line.last()) {
                features_at_point.entry(RoadConnectionPointKey::new(first)).or_default().insert(i);
                features_at_point.entry(RoadConnectionPointKey::new(last)).or_default().insert(i);
            }
        }
    }
    #[derive(Clone, Copy)]
    struct End {
        piece: usize,
        is_start: bool,
    }
    let mut ends_at_point: HashMap<RoadConnectionPointKey, Vec<End>> = HashMap::new();
    for (p, piece) in pieces.iter().enumerate() {
        ends_at_point.entry(RoadConnectionPointKey::new(piece.points[0])).or_default().push(End { piece: p, is_start: true });
        ends_at_point
            .entry(RoadConnectionPointKey::new(piece.points[piece.points.len() - 1]))
            .or_default()
            .push(End { piece: p, is_start: false });
    }
    let partner = |pieces: &[Piece], p: usize, at_start: bool| -> Option<End> {
        let piece = &pieces[p];
        let point = if at_start { piece.points[0] } else { piece.points[piece.points.len() - 1] };
        let key = RoadConnectionPointKey::new(point);
        if features_at_point.get(&key).map(HashSet::len) != Some(2) {
            return None;
        }
        for &candidate in ends_at_point.get(&key)? {
            if candidate.piece == p {
                continue;
            }
            let other = &pieces[candidate.piece];
            if other.consumed || identity[other.feature] != identity[piece.feature] {
                continue;
            }
            return Some(candidate);
        }
        None
    };

    let mut output = lines_by_feature;
    for p in 0..pieces.len() {
        if pieces[p].consumed {
            continue;
        }
        pieces[p].consumed = true;
        let mut chain = pieces[p].points.clone();
        // Grow forward from the end.
        let mut current = p;
        let mut current_end_is_start = false;
        while let Some(next) = partner(&pieces, current, current_end_is_start) {
            pieces[next.piece].consumed = true;
            let mut points = pieces[next.piece].points.clone();
            if !next.is_start {
                points.reverse();
            }
            chain.extend_from_slice(&points[1..]);
            output[pieces[next.piece].feature] = Vec::new();
            current = next.piece;
            current_end_is_start = !next.is_start;
        }
        // Grow backward from the start.
        current = p;
        current_end_is_start = true;
        while let Some(previous) = partner(&pieces, current, current_end_is_start) {
            pieces[previous.piece].consumed = true;
            let mut points = pieces[previous.piece].points.clone();
            if previous.is_start {
                points.reverse();
            }
            points.pop();
            points.extend_from_slice(&chain);
            chain = points;
            output[pieces[previous.piece].feature] = Vec::new();
            current = previous.piece;
            current_end_is_start = previous.is_start;
        }
        output[pieces[p].feature] = vec![chain];
    }
    output
}
