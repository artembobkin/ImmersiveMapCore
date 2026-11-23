// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Ported from the Swift `RoadStreetStitcherTests`, with the stitching keys
//! stated directly (the schema's reading of them is tested with the schema).

use tile_parse::geom::V2;
use tile_parse::roads::street_stitcher::{StitchFacts, stitch};

fn v(x: f32, y: f32) -> V2 {
    V2::new(x, y)
}

fn car(key: Option<&str>) -> StitchFacts<'_> {
    StitchFacts { stitching_key: key, is_automobile: true }
}

#[test]
fn two_pieces_of_one_street_become_one_polyline() {
    let lines = vec![vec![vec![v(0.0, 100.0), v(500.0, 100.0), v(1000.0, 120.0)]], vec![vec![v(1000.0, 120.0), v(1500.0, 150.0), v(2000.0, 150.0)]]];
    let out = stitch(lines, &[car(Some("T")), car(Some("T"))]);
    assert_eq!(out[0], vec![vec![v(0.0, 100.0), v(500.0, 100.0), v(1000.0, 120.0), v(1500.0, 150.0), v(2000.0, 150.0)]]);
    assert!(out[1].is_empty());
}

#[test]
fn a_reversed_piece_is_flipped_into_the_chain() {
    let lines = vec![vec![vec![v(0.0, 100.0), v(1000.0, 100.0)]], vec![vec![v(2000.0, 100.0), v(1000.0, 100.0)]]];
    let out = stitch(lines, &[car(Some("M")), car(Some("M"))]);
    assert_eq!(out[0], vec![vec![v(0.0, 100.0), v(1000.0, 100.0), v(2000.0, 100.0)]]);
    assert!(out[1].is_empty());
}

#[test]
fn a_chain_grows_backward_too() {
    let lines = vec![
        vec![vec![v(1000.0, 0.0), v(2000.0, 0.0)]],
        vec![vec![v(0.0, 0.0), v(1000.0, 0.0)]],
        vec![vec![v(2000.0, 0.0), v(3000.0, 0.0)]],
    ];
    let out = stitch(lines, &[car(Some("O")), car(Some("O")), car(Some("O"))]);
    assert_eq!(out[0], vec![vec![v(0.0, 0.0), v(1000.0, 0.0), v(2000.0, 0.0), v(3000.0, 0.0)]]);
    assert!(out[1].is_empty() && out[2].is_empty());
}

#[test]
fn a_junction_is_never_stitched_across() {
    let lines = vec![
        vec![vec![v(0.0, 100.0), v(1000.0, 100.0)]],
        vec![vec![v(1000.0, 100.0), v(2000.0, 100.0)]],
        vec![vec![v(1000.0, 100.0), v(1000.0, 800.0)]],
    ];
    let out = stitch(lines.clone(), &[car(Some("A")), car(Some("A")), car(Some("B"))]);
    assert_eq!(out, lines);
}

#[test]
fn different_streets_stay_apart() {
    let lines = vec![vec![vec![v(0.0, 100.0), v(1000.0, 100.0)]], vec![vec![v(1000.0, 100.0), v(2000.0, 100.0)]]];
    assert_eq!(stitch(lines.clone(), &[car(Some("A")), car(Some("B"))]), lines);
}

#[test]
fn pieces_without_a_key_are_left_alone() {
    let lines = vec![vec![vec![v(0.0, 100.0), v(1000.0, 100.0)]], vec![vec![v(1000.0, 100.0), v(2000.0, 100.0)]]];
    assert_eq!(stitch(lines.clone(), &[car(None), car(None)]), lines);
}

#[test]
fn pedestrian_pieces_are_not_stitched() {
    let lines = vec![vec![vec![v(0.0, 100.0), v(1000.0, 100.0)]], vec![vec![v(1000.0, 100.0), v(2000.0, 100.0)]]];
    let foot = StitchFacts { stitching_key: Some("Alley"), is_automobile: false };
    assert_eq!(stitch(lines.clone(), &[foot, foot]), lines);
}
