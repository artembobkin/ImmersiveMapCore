// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Ported from `RouteShieldTests`: which routes the built-in theme signs
//! and on what, and which roads carry them. The placement of the signs
//! along a tile's roads (`RouteShieldPlacement`) belongs to the parser and
//! is not ported here.

mod support;

use map_style::schema::{FeatureGeometry, RouteFacts};
use map_style::style::default::{ProtomapsBasemapTheme, RouteShieldRule, RouteShields};
use map_style::style::{RoadStyle, RouteShieldShape};
use support::*;

fn road(props: Props, zoom: i32) -> Option<RoadStyle> {
    style_of(&default_style(), "roads", &props, zoom, FeatureGeometry::Line).road_style().map(|road| road.into_owned())
}

#[test]
fn a_national_number_leads_the_european_one() {
    let shields = RouteShields::default().shields(&[RouteFacts::new("e-road", "E22"), RouteFacts::new("ru:national", "М-9")]);
    assert_eq!(shields.iter().map(|shield| shield.text.as_str()).collect::<Vec<_>>(), ["М-9", "E22"]);
    assert_eq!(shields.iter().map(|shield| shield.appearance.fill_color[0]).collect::<Vec<_>>(), [0.827, 0.0]);
}

#[test]
fn the_unsigned_networks_and_the_limit_leave_the_right_signs() {
    // The M-4 south of Moscow carries four routes.
    let shields = RouteShields::default().shields(&[
        RouteFacts::new("AsianHighway", "AH8"),
        RouteFacts::new("e-road", "E115"),
        RouteFacts::new("e-road", "E119"),
        RouteFacts::new("ru:national", "М-4"),
    ]);
    assert_eq!(
        shields.iter().map(|shield| shield.text.as_str()).collect::<Vec<_>>(),
        ["М-4", "E115"],
        "the Asian Highway is not signed, and a road carries at most two signs"
    );
}

#[test]
fn the_united_states_signs_its_interstates_highways_and_state_routes_only() {
    let shields = RouteShields::default().shields(&[
        RouteFacts::new("US:NJ:CR", "527"),
        RouteFacts::new("US:I:Truck", "5"),
        RouteFacts::new("US:NJ", "33"),
        RouteFacts::new("US:I", "95"),
    ]);
    assert_eq!(shields.iter().map(|shield| shield.text.as_str()).collect::<Vec<_>>(), ["95", "33"]);
    assert_eq!(
        shields.iter().map(|shield| shield.appearance.shape).collect::<Vec<_>>(),
        [RouteShieldShape::Escutcheon, RouteShieldShape::Capsule]
    );
    assert!(shields[0].appearance.header_color.is_some(), "the Interstate sign wears its red crown");
}

#[test]
fn a_network_pattern_matches_without_regard_to_case_and_by_segment() {
    let rule = RouteShieldRule::new("FR:*:D-road", None);
    assert!(rule.matches("FR:94:D-road"));
    assert!(rule.matches("fr:92:d-road"));
    assert!(!rule.matches("FR:D-road"));
    assert!(!rule.matches("FR:94:D-road:extra"));
    let open = RouteShieldRule::new("US:*", None);
    assert!(open.matches("US:CA"));
    assert!(open.matches("US:NJ:CR"), "a trailing star stands for one or more segments");
    assert!(!open.matches("US"));
}

#[test]
fn a_route_no_rule_names_takes_the_fallback_plate_and_the_same_number_stands_once() {
    let signs = RouteShields::default();
    let shields = signs.shields(&[RouteFacts::unnetworked("46К-1011"), RouteFacts::new("ru:regional", "46К-1011")]);
    assert_eq!(shields.iter().map(|shield| shield.text.as_str()).collect::<Vec<_>>(), ["46К-1011"]);
    assert_eq!(shields[0].appearance, RouteShields::default_fallback());
    let unsigned = RouteShields { fallback: None, ..signs };
    assert!(unsigned.shields(&[RouteFacts::unnetworked("46К-1011")]).is_empty());
}

#[test]
fn a_changed_sign_changes_the_cache_fingerprint() {
    let base = ProtomapsBasemapTheme::default();
    let resized = base.with_route_shields(|shields| shields.size_points += 1.0);
    let recoloured = base.with_route_shields(|shields| {
        if let Some(appearance) = shields.rules[0].appearance.as_mut() {
            appearance.fill_color = [0.0, 0.0, 1.0];
        }
    });
    assert_ne!(base.cache_fingerprint(), resized.cache_fingerprint());
    assert_ne!(base.cache_fingerprint(), recoloured.cache_fingerprint());
}

#[test]
fn a_motorway_carries_its_signs_and_a_ramp_carries_none() {
    let motorway = road(
        vec![("kind", s("highway")), ("kind_detail", s("motorway")), ("network_1", s("ru:national")), ("shield_text_1", s("М-9"))],
        8,
    )
    .unwrap();
    let shields = motorway.shields.unwrap();
    assert_eq!(shields.shields.iter().map(|shield| shield.text.as_str()).collect::<Vec<_>>(), ["М-9"]);
    assert!(motorway.label.is_none(), "a road known only by its number lays no text along itself");
    let ramp = road(
        vec![
            ("kind", s("highway")),
            ("kind_detail", s("motorway_link")),
            ("is_link", b(true)),
            ("network_1", s("ru:national")),
            ("shield_text_1", s("М-9")),
        ],
        12,
    )
    .unwrap();
    assert!(ramp.shields.is_none());
}

#[test]
fn a_lesser_road_waits_for_its_zoom() {
    let props = || vec![("kind", s("major_road")), ("kind_detail", s("secondary")), ("network_1", s("ru:regional")), ("shield_text_1", s("46К-1300"))];
    assert!(road(props(), 10).unwrap().shields.is_none());
    let shields = road(props(), 11).unwrap().shields.unwrap();
    assert_eq!(shields.shields.iter().map(|shield| shield.text.as_str()).collect::<Vec<_>>(), ["46К-1300"]);
}
