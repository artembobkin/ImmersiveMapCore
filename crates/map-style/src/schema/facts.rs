// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use std::borrow::Cow;

/// What one feature is, as the schema reading says: the facts the engine's
/// geometry work needs and the style draws by. One case per kind of thing,
/// each carrying only that kind's facts. A reader matches on the case and
/// cannot read a label off a building.
///
/// Text borrows the tile (`'a`) wherever the reading takes it as the tile
/// spells it, and owns what it builds (a stitching key, a joined route
/// reference).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum FeatureFacts<'a> {
    /// None of the things the facts describe: a ground fill, a border, a
    /// river.
    #[default]
    None,
    /// A road: a centreline, a carriageway surface, a parking lot, or a
    /// line of measured paint. The name along it, where it has one, is
    /// inside (`RoadFacts::label`).
    Road(RoadFacts<'a>),
    /// A building.
    Building(BuildingExtrusion),
    /// A point that can be labelled: a place, a POI, a water name, a house
    /// number.
    Labelled(LabelFacts<'a>),
}

impl<'a> FeatureFacts<'a> {
    pub fn road(&self) -> Option<&RoadFacts<'a>> {
        match self {
            FeatureFacts::Road(road) => Some(road),
            _ => None,
        }
    }

    pub fn building(&self) -> Option<&BuildingExtrusion> {
        match self {
            FeatureFacts::Building(building) => Some(building),
            _ => None,
        }
    }

    /// The names of the feature: a labelled point's, or the name along a
    /// road.
    pub fn label(&self) -> Option<&LabelFacts<'a>> {
        match self {
            FeatureFacts::Labelled(label) => Some(label),
            FeatureFacts::Road(road) => road.label.as_ref(),
            FeatureFacts::None | FeatureFacts::Building(_) => None,
        }
    }
}

/// The physical structure a road runs on, as the source tags it. A road
/// that ships only a negative or positive `layer` is on the ground by this
/// reading, below or above its neighbours: a street diving under a bridge
/// is not in a tunnel, and is in full view from above.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RoadStructure {
    Tunnel,
    #[default]
    Ground,
    Bridge,
}

/// What a line feature is as a road, beyond how it draws: where it sits (in
/// a tunnel, on the ground, on a bridge, and on which `layer`) and which
/// street it is a piece of. The schema reading (`TileSchema`) states
/// these. The engine orders the roads and stitches the pieces of a street
/// from them, and the style draws by them. Neither reads a road tag
/// itself.
///
/// A feature that is not a road (a river, a border) has no road facts at
/// all.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RoadFacts<'a> {
    pub structure: RoadStructure,
    /// The vertical layer among roads of one structure, the source's
    /// `layer`: a bridge over a bridge, a road under a road.
    pub layer: i64,
    /// The road's name, empty when it has none. The identity of a street
    /// for counting junctions.
    pub name: Cow<'a, str>,
    /// The key two pieces must share to be drawn as one ribbon with no seam
    /// where they met: the street they belong to plus everything that
    /// changes how a piece draws. `None` for a piece that is never
    /// stitched.
    pub stitching_key: Option<String>,
    /// The name laid along the road, `None` for a road without one. A route
    /// number is not part of it: the numbers are `routes`, which the style
    /// draws on their signs.
    pub label: Option<LabelFacts<'a>>,
    /// The numbered routes the road carries, in the order the source lists
    /// them. Empty for a road that is part of no route.
    pub routes: Vec<RouteFacts<'a>>,
}

impl RoadFacts<'_> {
    /// A road on the ground with no identity.
    pub fn ground() -> RoadFacts<'static> {
        RoadFacts::default()
    }
}

/// One numbered route a road carries, as the schema reading states it: the
/// network the route belongs to and what its sign reads. A road that is
/// part of several routes (a national road that is also a European one)
/// carries one of these per route, in the order the source lists them.
///
/// The network is the source's own vocabulary (OpenStreetMap's `network`
/// tag: `ru:national`, `e-road`, `US:I`, `BAB`), passed through as it is.
/// The engine never reads it: the style decides from it what sign a route
/// is drawn on, or whether it is drawn at all.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RouteFacts<'a> {
    /// The route's network, empty when the source names none (a road that
    /// states its number but belongs to no route the source knows).
    pub network: Cow<'a, str>,
    /// What the route's sign reads: `М-9`, `E22`, `95`, `A3`.
    pub text: Cow<'a, str>,
}

impl<'a> RouteFacts<'a> {
    pub fn new(network: impl Into<Cow<'a, str>>, text: impl Into<Cow<'a, str>>) -> Self {
        RouteFacts { network: network.into(), text: text.into() }
    }

    /// A route of no network.
    pub fn unnetworked(text: impl Into<Cow<'a, str>>) -> Self {
        RouteFacts { network: Cow::Borrowed(""), text: text.into() }
    }
}

/// What a polygon feature is as a building, as the schema reading states
/// it (`TileSchema`): how tall, on which base, which building it belongs
/// to, and whether it is a part of one. The engine turns this into the
/// extrusion when the style asks for one. It never reads a building tag
/// itself.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BuildingExtrusion {
    /// The height above ground in metres, `None` when the tile states none:
    /// the style's fallback height applies, and the building stays flat
    /// when that is zero.
    pub height_metres: Option<f32>,
    /// The height of the base above ground in metres, for a part that
    /// starts above the ground (a tower on a podium). `None` means ground
    /// level.
    pub base_height_metres: Option<f32>,
    /// The building the feature belongs to, when the source names one, so
    /// the outline and the parts of one building are resolved against each
    /// other. `None` means the feature stands for itself.
    pub building_identity: Option<u64>,
    /// A part of a building rather than its outline.
    pub is_part: bool,
    /// The feature is not raised: the source says so (`extrude=false`,
    /// `hide_3d`) or the building lies underground.
    pub is_hidden: bool,
}

/// What a feature is called, as the schema reading states it: its name in
/// the local language and by language code. The engine picks the spelling
/// the map's language asks for from these and never reads a name field
/// itself.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LabelFacts<'a> {
    /// The name in the local language, `None` when the source states none.
    pub name: Option<Cow<'a, str>>,
    /// The name by language code (`"en"`, `"ru"`, ...), where the source
    /// states one.
    pub names_by_language: NamesByLanguage<'a>,
    /// The feature names a body of water. The parser adds ocean and sea
    /// names of its own at the coarse zooms and skips any the tile already
    /// labels. This is how it recognises those.
    pub names_water_body: bool,
}

impl<'a> LabelFacts<'a> {
    /// A label with a local name alone.
    pub fn named(name: impl Into<Cow<'a, str>>) -> Self {
        LabelFacts { name: Some(name.into()), ..LabelFacts::default() }
    }
}

/// The names of a feature by language code: a small map kept sorted by
/// code, so two equal sets of names compare equal whatever order the tile
/// listed them in. A feature carries a handful, so lookups scan.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NamesByLanguage<'a> {
    entries: Vec<(Cow<'a, str>, Cow<'a, str>)>,
}

impl<'a> NamesByLanguage<'a> {
    pub fn new() -> Self {
        NamesByLanguage { entries: Vec::new() }
    }

    /// The name under a language code.
    pub fn get(&self, code: &str) -> Option<&str> {
        self.entries.iter().find(|(key, _)| key == code).map(|(_, name)| name.as_ref())
    }

    pub fn contains(&self, code: &str) -> bool {
        self.get(code).is_some()
    }

    /// Files a name under a code, replacing the one there.
    pub fn insert(&mut self, code: impl Into<Cow<'a, str>>, name: impl Into<Cow<'a, str>>) {
        let code = code.into();
        let name = name.into();
        match self.entries.binary_search_by(|(key, _)| key.as_ref().cmp(code.as_ref())) {
            Ok(index) => self.entries[index].1 = name,
            Err(index) => self.entries.insert(index, (code, name)),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Every `(code, name)` pair, in code order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(code, name)| (code.as_ref(), name.as_ref()))
    }
}

impl<'a, C: Into<Cow<'a, str>>, N: Into<Cow<'a, str>>> FromIterator<(C, N)> for NamesByLanguage<'a> {
    fn from_iter<I: IntoIterator<Item = (C, N)>>(iter: I) -> Self {
        let mut names = NamesByLanguage::new();
        for (code, name) in iter {
            names.insert(code, name);
        }
        names
    }
}
