// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The Protomaps basemap's readings of one feature's tags into the facts.
//! What the tags are called is the basemap's contract. The facts they
//! become carry none of it.

use std::borrow::Cow;

use mvt::Value;

use super::ProtomapsBasemapSchema;
use crate::schema::{BuildingExtrusion, FeatureProperties, LabelFacts, NamesByLanguage, RoadFacts, RoadStructure, RouteFacts};

/// The basemap names at most this many routes on one road (`network_1` to
/// `network_6`).
const ROUTE_SLOT_COUNT: usize = 6;

/// The attributes that change how a piece of road draws, in the order they
/// are spelled into the stitching key.
const DRAWING_ATTRIBUTES: [&str; 7] = ["kind", "kind_detail", "is_link", "oneway", "layer", "is_bridge", "is_tunnel"];

impl ProtomapsBasemapSchema {
    /// A road's structure, layer, name, label, routes and stitching key.
    ///
    /// The structure is `is_tunnel` or `is_bridge`, and the layer among
    /// roads of one structure is `layer`. The label is the name in every
    /// language, and nothing else: the route numbers are the routes, which
    /// the style draws on their signs. The routes are the basemap's
    /// `network_N` and `shield_text_N` pairs (`e-road` and `E22`,
    /// `ru:national` and `М-9`), and for a road in no route relation the
    /// `ref` alone, every number of it a route of no network. The name, the
    /// road's identity for counting junctions, is `name`, or the route
    /// reference for a road that has only a number. The stitching key is the
    /// name with the attributes that change how a piece draws (`kind`,
    /// `kind_detail`, `is_link`, `oneway`, `layer`, `is_bridge`,
    /// `is_tunnel`). A reference is left out of the key for a named road on
    /// purpose: a `ref` that changes mid-street is not a drawing change. A
    /// road with neither has no key and is never stitched.
    pub fn road<'a>(&self, properties: &FeatureProperties<'_, 'a>) -> RoadFacts<'a> {
        let structure = if properties.bool("is_tunnel") == Some(true) {
            RoadStructure::Tunnel
        } else if properties.bool("is_bridge") == Some(true) {
            RoadStructure::Bridge
        } else {
            RoadStructure::Ground
        };
        let references = route_references(properties);
        let reference = if references.is_empty() { None } else { Some(references.join(" / ")) };
        let own_name = properties.string("name").unwrap_or("");
        let has_reference = reference.is_some();
        let name: Cow<'a, str> =
            if own_name.is_empty() { Cow::Owned(reference.unwrap_or_default()) } else { Cow::Borrowed(own_name) };
        let stitching_key = if !own_name.is_empty() {
            Some(key(properties, "name"))
        } else if has_reference {
            Some(key(properties, "ref"))
        } else {
            None
        };
        let routes = routes(properties, &references);
        RoadFacts {
            structure,
            layer: properties.integer("layer").unwrap_or(0),
            name,
            stitching_key,
            label: self.names(properties),
            routes,
        }
    }

    /// An address point's house number, `addr_housenumber`, as a label.
    /// `None` for a point without one.
    pub fn house_number<'a>(&self, properties: &FeatureProperties<'_, 'a>) -> Option<LabelFacts<'a>> {
        let number = trim_whitespaces(properties.string("addr_housenumber")?);
        if number.is_empty() {
            return None;
        }
        Some(LabelFacts::named(number))
    }

    /// A building's heights and whether it is a part. Heights come from
    /// `height` and `min_height`, in metres, which the basemap fills in from
    /// the storey count itself when the source states no height. A part is
    /// `kind=building_part`. The basemap names no building identity, so
    /// every feature stands for itself, and a feature with a negative
    /// `layer` lies underground and is not raised. Neither is a part whose
    /// `kind_detail` is `no`, OSM's `building:part=no`: an area of a
    /// building's outline that is no part of it (the ground around Lenin's
    /// Mausoleum and its tribunes), which Simple 3D Buildings never raises.
    /// It ships with no height, and raised at the style's fallback height it
    /// would stand as a slab over the whole area, the real parts sunk in
    /// it.
    pub fn building(&self, properties: &FeatureProperties<'_, '_>) -> BuildingExtrusion {
        let is_part = properties.string("kind") == Some("building_part");
        let is_no_part = is_part && properties.string("kind_detail") == Some("no");
        BuildingExtrusion {
            height_metres: properties.metres("height"),
            base_height_metres: properties.metres("min_height"),
            building_identity: None,
            is_part,
            is_hidden: is_no_part || properties.integer("layer").unwrap_or(0) < 0,
        }
    }

    /// A feature's names: `name` for the local name and `name:xx` for the
    /// name in language `xx`. The basemap's other name fields are not names
    /// in a language: `name2`, `name3` and `script*` split a mixed local
    /// name by writing system, and `pgf:name:xx` is a pre-shaped glyph
    /// string for a script the engine does not draw that way. A regional
    /// code (`zh-Hans`) is also filed under its language (`zh`) where that
    /// is free, since the map's language asks by the code before the dash.
    /// `None` for a feature that carries no name at all.
    pub fn names<'a>(&self, properties: &FeatureProperties<'_, 'a>) -> Option<LabelFacts<'a>> {
        let mut name: Option<&'a str> = None;
        let mut names_by_language = NamesByLanguage::new();
        let mut regional_codes: Vec<(&'a str, &'a str, &'a str)> = Vec::new();
        for &(key, value) in properties.entries() {
            if !key.starts_with("name") {
                continue;
            }
            let Value::String(text) = value else { continue };
            if text.is_empty() {
                continue;
            }
            if key.len() == 4 {
                name = Some(text);
            } else if let Some(code) = key.strip_prefix("name:") {
                if code.is_empty() {
                    continue;
                }
                names_by_language.insert(code, text);
                if let Some(dash) = code.find('-') {
                    regional_codes.push((&code[..dash], code, text));
                }
            }
        }
        regional_codes.sort_by(|lhs, rhs| lhs.1.cmp(rhs.1));
        for (language, _, text) in regional_codes {
            if !names_by_language.contains(language) {
                names_by_language.insert(language, text);
            }
        }
        if name.is_none() && names_by_language.is_empty() {
            return None;
        }
        Some(LabelFacts { name: name.map(Cow::Borrowed), names_by_language, names_water_body: false })
    }
}

/// A road's routes: the pairs of `network_N` and `shield_text_N` in their
/// order, a slot with no sign text skipped (the basemap lists the railway
/// and ferry networks a road belongs to there too, with no text). A road
/// the basemap ships no pair for, but which states its numbers in `ref`,
/// has one route of no network per number.
fn routes<'a>(properties: &FeatureProperties<'_, 'a>, references: &[&'a str]) -> Vec<RouteFacts<'a>> {
    let mut routes = Vec::new();
    for slot in 1..=ROUTE_SLOT_COUNT {
        let Some(text) = trimmed(properties.string(&format!("shield_text_{slot}"))) else {
            continue;
        };
        let network = trimmed(properties.string(&format!("network_{slot}"))).unwrap_or("");
        routes.push(RouteFacts::new(network, text));
    }
    if routes.is_empty()
        && let Some(text) = trimmed(properties.string("shield_text"))
    {
        routes.push(RouteFacts::new(trimmed(properties.string("network")).unwrap_or(""), text));
    }
    if routes.is_empty() {
        routes = references.iter().map(|text| RouteFacts::unnetworked(*text)).collect();
    }
    routes
}

/// A road's route references as the basemap's `ref` states them, the
/// several routes of one road (`M10;E105`) apart. Empty for a road without
/// one.
fn route_references<'a>(properties: &FeatureProperties<'_, 'a>) -> Vec<&'a str> {
    let Some(reference) = properties.string("ref") else {
        return Vec::new();
    };
    reference.split(';').map(trim_whitespaces).filter(|part| !part.is_empty()).collect()
}

fn trimmed(text: Option<&str>) -> Option<&str> {
    let text = trim_whitespaces(text?);
    if text.is_empty() { None } else { Some(text) }
}

/// The key spelled from the first attribute and the drawing attributes,
/// each as `attribute=value;`.
fn key(properties: &FeatureProperties<'_, '_>, first: &str) -> String {
    let mut key = String::new();
    for attribute in std::iter::once(first).chain(DRAWING_ATTRIBUTES) {
        key.push_str(attribute);
        key.push('=');
        key.push_str(&properties.text(attribute));
        key.push(';');
    }
    key
}

/// Whitespace without the line breaks, as Foundation's
/// `CharacterSet.whitespaces` holds it: the tab and the space separators.
pub(crate) fn is_inline_whitespace(c: char) -> bool {
    c.is_whitespace() && !matches!(c, '\n' | '\u{0B}' | '\u{0C}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// The text without leading or trailing inline whitespace
/// (`trimmingCharacters(in: .whitespaces)`).
pub(crate) fn trim_whitespaces(text: &str) -> &str {
    text.trim_matches(is_inline_whitespace)
}
