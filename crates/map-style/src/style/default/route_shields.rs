// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use serde::{Deserialize, Serialize};

use crate::schema::RouteFacts;
use crate::style::default::RoadClassValues;
use crate::style::{LabelFontWeight, RouteShield, RouteShieldAppearance, RouteShieldShape};

/// The route signs: which networks are signed and on what sign, how large
/// the numbers are, how often a road repeats them, and from which tile zoom
/// each road class carries them.
///
/// A route is matched against `rules` in order, and the first rule whose
/// network pattern matches decides its sign. The same order is the order
/// the signs stand in on a road that carries several routes, so the rules
/// list the national networks before the European one: a road signed `М-9`
/// and `E22` reads `М-9` first. A route no rule matches (and a route of no
/// network, a bare `ref`) takes `fallback`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RouteShields {
    pub rules: Vec<RouteShieldRule>,
    /// The sign of a route no rule matches, `None` to leave such routes
    /// unsigned.
    pub fallback: Option<RouteShieldAppearance>,
    /// The em size of the numbers in layout points.
    pub size_points: f32,
    pub weight: LabelFontWeight,
    /// The least distance between two copies of a road's signs, in layout
    /// points at the tile's zoom.
    pub spacing_points: f32,
    /// The most signs one road carries side by side.
    pub maximum_count: i32,
    /// The tile zoom a road class first carries its signs at.
    pub minimum_tile_zoom: RoadClassValues<i32>,
}

/// One network pattern and the sign its routes are drawn on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteShieldRule {
    /// The network pattern, compared without regard to case, one segment
    /// per colon. `*` stands for any one segment, and as the last segment
    /// for one or more: `US:*` matches `US:CA` and `US:NJ:CR`, `US:*:*` only
    /// the second.
    pub network: String,
    /// The sign, `None` for a network that is not signed at all.
    #[serde(default)]
    pub appearance: Option<RouteShieldAppearance>,
}

impl RouteShieldRule {
    pub fn new(network: impl Into<String>, appearance: Option<RouteShieldAppearance>) -> Self {
        RouteShieldRule { network: network.into(), appearance }
    }

    /// Whether a route's network matches the pattern.
    pub fn matches(&self, network: &str) -> bool {
        let pattern_text = self.network.to_lowercase();
        let network_text = network.to_lowercase();
        let pattern: Vec<&str> = pattern_text.split(':').collect();
        let segments: Vec<&str> = network_text.split(':').collect();
        for (index, part) in pattern.iter().enumerate() {
            if index >= segments.len() {
                return false;
            }
            if *part == "*" {
                if index == pattern.len() - 1 {
                    return true;
                }
                continue;
            }
            if *part != segments[index] {
                return false;
            }
        }
        segments.len() == pattern.len()
    }
}

impl Default for RouteShields {
    fn default() -> Self {
        RouteShields {
            rules: Self::default_rules(),
            fallback: Some(Self::default_fallback()),
            size_points: 11.0,
            weight: LabelFontWeight::Bold,
            spacing_points: 220.0,
            maximum_count: 2,
            minimum_tile_zoom: Self::DEFAULT_MINIMUM_TILE_ZOOM,
        }
    }
}

impl RouteShields {
    /// The motorways and trunks carry their signs from the first road-path
    /// tile, z8. The lesser classes follow as the map gets room for them,
    /// and the service roads and paths carry none.
    pub const DEFAULT_MINIMUM_TILE_ZOOM: RoadClassValues<i32> = RoadClassValues {
        motorway: 8,
        trunk: 8,
        primary: 9,
        secondary: 11,
        tertiary: 12,
        minor: 13,
        service: 99,
        path: 99,
        other: 99,
    };

    /// A plain plate for a route of a network no rule names: white, with a
    /// grey edge and a dark number, a sign that says "route" without
    /// claiming any country's colours.
    pub fn default_fallback() -> RouteShieldAppearance {
        RouteShieldAppearance::new([1.0, 1.0, 1.0], [0.25, 0.25, 0.27]).border([0.58, 0.59, 0.61], 0.08)
    }

    /// The signs as each country posts them on its roads: the colours of the
    /// plate and of the number, and the white or black edge the plate has.
    /// The national networks come first, so a road that is also a European
    /// route leads with its national number, and the European routes follow
    /// on their green plate. The Asian Highway numbers, the US truck,
    /// historic and auto-trail routes and the US county routes are not
    /// signed.
    pub fn default_rules() -> Vec<RouteShieldRule> {
        let white = [1.0, 1.0, 1.0];
        let black = [0.08, 0.08, 0.08];
        let red = [0.827, 0.133, 0.145];
        let green = [0.0, 0.502, 0.271];
        let blue = [0.0, 0.345, 0.667];
        let yellow = [1.0, 0.8, 0.0];
        let border: f32 = 0.09;

        let white_on_red = RouteShieldAppearance::new(red, white).border(white, border);
        let white_on_green = RouteShieldAppearance::new(green, white).border(white, border);
        let white_on_blue = RouteShieldAppearance::new(blue, white).border(white, border);
        let black_on_yellow = RouteShieldAppearance::new(yellow, black).border(black, 0.07);
        let rule = |network: &str, appearance: Option<RouteShieldAppearance>| RouteShieldRule::new(network, appearance);

        vec![
            // Russia: sign 6.14.1, a white number on red.
            rule("ru:national", Some(white_on_red)),
            // Germany: the Autobahn on blue, the Bundesstraße on yellow.
            rule("BAB", Some(white_on_blue)),
            rule("DE:national", Some(black_on_yellow)),
            // The United Kingdom's motorways, on their lighter blue.
            rule("UK:motorway", Some(RouteShieldAppearance::new([0.0, 0.475, 0.757], white).border(white, border))),
            // France: the autoroutes and routes nationales on red, the
            // départementales on yellow.
            rule("FR:A-road", Some(white_on_red)),
            rule("FR:N-road", Some(white_on_red)),
            rule("FR:*:D-road", Some(black_on_yellow)),
            // Italy: the autostrade on green, the strade statali on blue.
            rule("IT:A-road", Some(white_on_green)),
            rule("IT", Some(white_on_blue)),
            // Spain: the autovías and autopistas on blue.
            rule("ES:A-road", Some(white_on_blue)),
            rule("ES:AP-highway", Some(white_on_blue)),
            rule("ES:R-highway", Some(white_on_blue)),
            // Poland: the motorways, expressways and national roads on red,
            // the voivodeship roads on yellow.
            rule("PL:motorway", Some(white_on_red)),
            rule("PL:expressway", Some(white_on_red)),
            rule("PL:national", Some(white_on_red)),
            rule("PL:regional", Some(black_on_yellow)),
            // Japan: the expressway numbers on green, the national routes on
            // blue.
            rule("JP:E", Some(white_on_green)),
            rule("JP:national", Some(white_on_blue)),
            // The United States: the Interstate's blue shield under its red
            // crown, the US highway's white shield, and the state routes on a
            // white oval.
            rule(
                "US:I",
                Some(
                    RouteShieldAppearance::new([0.0, 0.247, 0.529], white)
                        .shape(RouteShieldShape::Escutcheon)
                        .border(white, border)
                        .header(Some([0.686, 0.118, 0.176])),
                ),
            ),
            rule(
                "US:US",
                Some(RouteShieldAppearance::new(white, black).shape(RouteShieldShape::Escutcheon).border(black, border)),
            ),
            rule("US:I:*", None),
            rule("US:US:*", None),
            rule("US:*:*", None),
            rule(
                "US:*",
                Some(RouteShieldAppearance::new(white, black).shape(RouteShieldShape::Capsule).border(black, border)),
            ),
            // The European routes, after every national network.
            rule("e-road", Some(white_on_green)),
            rule("AsianHighway", None),
            rule("AH", None),
        ]
    }

    /// The signs of a road's routes: each route on the sign its network is
    /// given, in the rules' order (a route the fallback signs after every
    /// ruled one), the same number never twice, at most `maximum_count`.
    pub fn shields(&self, routes: &[RouteFacts<'_>]) -> Vec<RouteShield> {
        let mut ranked: Vec<(usize, RouteShield)> = Vec::new();
        for route in routes.iter().filter(|route| !route.text.is_empty()) {
            let (order, appearance) = match self.rules.iter().position(|rule| rule.matches(&route.network)) {
                Some(index) => (index, self.rules[index].appearance),
                None => (self.rules.len(), self.fallback),
            };
            let Some(appearance) = appearance else { continue };
            if ranked.iter().any(|(_, shield)| shield.text == route.text) {
                continue;
            }
            ranked.push((order, RouteShield { text: route.text.to_string(), appearance }));
        }
        // A stable sort: two routes of one rule keep the source's order.
        ranked.sort_by_key(|(order, _)| *order);
        ranked.into_iter().take(self.maximum_count.max(0) as usize).map(|(_, shield)| shield).collect()
    }

    pub fn fingerprint_components(&self) -> Vec<f32> {
        let mut out: Vec<f32> =
            vec![self.size_points, self.weight.raw_value() as f32, self.spacing_points, self.maximum_count as f32];
        out.extend(self.minimum_tile_zoom.all().map(|value| value as f32));
        fn add(out: &mut Vec<f32>, appearance: &Option<RouteShieldAppearance>) {
            let Some(appearance) = appearance else {
                out.push(-1.0);
                return;
            };
            out.push(appearance.shape.raw_value() as f32);
            for color in [
                appearance.fill_color,
                appearance.text_color,
                appearance.border_color,
                appearance.header_color.unwrap_or([-1.0; 3]),
            ] {
                out.extend(color);
            }
            out.extend([appearance.border_em, appearance.corner_radius_em, appearance.padding_em, appearance.header_fraction]);
        }
        for rule in &self.rules {
            out.extend(rule.network.bytes().map(|byte| byte as f32));
            add(&mut out, &rule.appearance);
        }
        add(&mut out, &self.fallback);
        out
    }
}
