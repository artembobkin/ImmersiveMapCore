// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use std::borrow::Cow;

use super::{GlyphCoverage, LanguagePreferences, TextResolver};
use crate::schema::LabelFacts;
use crate::style::{LabelTextStyle, PoiSpriteIcon, PointLabelStyle, RouteShield, VectorTileStyle};
use crate::support::{LabelFallbackPolicy, LabelLanguage, Tile};

/// A label's ranks: the order it is revealed in, its precedence in the
/// collisions, in the deduplication and in the drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LabelPriority {
    pub visibility_rank: i32,
    pub collision_rank: i32,
    pub deduplication_rank: i32,
    pub draw_rank: i32,
}

/// The shape a label collides with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelCollisionShape {
    Rect,
}

/// How a label stands on its anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelAnchorMode {
    Centered,
}

/// How a label is placed on the screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LabelPlacementIntent {
    pub collision_padding_px: f32,
    pub collision_shape: LabelCollisionShape,
    pub anchor_mode: LabelAnchorMode,
    pub screen_offset_px: [f32; 2],
}

impl LabelPlacementIntent {
    pub const CENTERED: LabelPlacementIntent = LabelPlacementIntent {
        collision_padding_px: 0.0,
        collision_shape: LabelCollisionShape::Rect,
        anchor_mode: LabelAnchorMode::Centered,
        screen_offset_px: [0.0, 0.0],
    };
}

/// A label's identity: one across tiles by its feature id, or one of its
/// tile alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LabelIdentity<'a> {
    StyleFeature { style_id: Cow<'a, str>, layer_name: Cow<'a, str>, feature_id: u64 },
    TileLocal { tile: Tile, layer_name: Cow<'a, str>, text: Cow<'a, str>, anchor: [i16; 2] },
}

impl LabelIdentity<'_> {
    pub fn participates_in_cross_tile_deduplication(&self) -> bool {
        matches!(self, LabelIdentity::StyleFeature { .. })
    }

    /// The key the runtime knows the label by: a stable FNV-1a hash of the
    /// identity, the same on every platform and every run.
    pub fn runtime_key(&self) -> u64 {
        let mut hasher = StableHasher::new();
        match self {
            LabelIdentity::StyleFeature { style_id, layer_name, feature_id } => {
                hasher.combine_str("providerFeature");
                hasher.combine_str(style_id);
                hasher.combine_str(layer_name);
                hasher.combine(*feature_id);
            }
            LabelIdentity::TileLocal { tile, layer_name, text, anchor } => {
                hasher.combine_str("tileLocal");
                hasher.combine_int(tile.x as i64);
                hasher.combine_int(tile.y as i64);
                hasher.combine_int(tile.z as i64);
                hasher.combine_str(layer_name);
                hasher.combine_str(text);
                hasher.combine_i16(anchor[0]);
                hasher.combine_i16(anchor[1]);
            }
        }
        hasher.finalize()
    }
}

/// FNV-1a over 64-bit words, the hash label identities are keyed by.
#[derive(Debug, Clone, Copy)]
pub struct StableHasher {
    hash: u64,
}

impl Default for StableHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl StableHasher {
    const SEED: u64 = 1469598103934665603;
    const PRIME: u64 = 1099511628211;

    pub fn new() -> Self {
        StableHasher { hash: Self::SEED }
    }

    pub fn combine(&mut self, value: u64) {
        self.hash ^= value;
        self.hash = self.hash.wrapping_mul(Self::PRIME);
    }

    pub fn combine_int(&mut self, value: i64) {
        self.combine(value as u64);
    }

    pub fn combine_u32(&mut self, value: u32) {
        self.combine(value as u64);
    }

    pub fn combine_i16(&mut self, value: i16) {
        self.combine(value as u16 as u64);
    }

    /// The UTF-8 length, then every byte.
    pub fn combine_str(&mut self, value: &str) {
        self.combine(value.len() as u64);
        for byte in value.bytes() {
            self.combine(byte as u64);
        }
    }

    pub fn finalize(&self) -> u64 {
        self.hash
    }
}

/// A labelled point as the parser hands it over: its tile, layer, feature
/// id and anchor in tile units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LabelFeature<'a> {
    pub style_id: &'a str,
    pub tile: Tile,
    pub layer_name: &'a str,
    pub feature_id: Option<u64>,
    pub anchor: [i16; 2],
}

/// How a point feature is labelled.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelDecision<'a> {
    pub text: String,
    pub identity: LabelIdentity<'a>,
    pub priority: LabelPriority,
    pub placement: LabelPlacementIntent,
    pub style: LabelTextStyle,
    pub poi_icon: Option<PoiSpriteIcon>,
}

/// The label policy a tile parse asks questions of: which spelling a road
/// or a point feature is labelled with, how a labelled point is identified
/// and ranked, and which spelling of a name the map's language and the text
/// atlas allow.
///
/// Assembled once by the caller from the style (how labels are identified),
/// the glyph coverage of the text atlas and the language settings, then
/// handed to the tile parser, which holds nothing of the policy itself: the
/// parser decodes geometry and asks, this answers, from the names the
/// schema reading stated (`LabelFacts`). Every answer is pure, so one value
/// serves every parse of a map.
#[derive(Debug, Clone)]
pub struct TileLabelDecisions {
    /// The style the label identities are minted for.
    pub style_id: String,
    uses_feature_identity: bool,
    language_preferences: LanguagePreferences,
    text_resolver: TextResolver,
}

impl TileLabelDecisions {
    pub fn new(
        style: &dyn VectorTileStyle,
        glyph_coverage: GlyphCoverage,
        language: &LabelLanguage,
        fallback_policy: LabelFallbackPolicy,
    ) -> Self {
        TileLabelDecisions {
            style_id: style.style_id().to_owned(),
            uses_feature_identity: style.labels_use_feature_identity(),
            language_preferences: LanguagePreferences::new(language, fallback_policy),
            text_resolver: TextResolver::new(glyph_coverage),
        }
    }

    pub fn language_preferences(&self) -> &LanguagePreferences {
        &self.language_preferences
    }

    /// The name a road line is labelled with, in the map's language with
    /// its fallback chain, or `None` when the feature carries none the atlas
    /// can render. In capitals when the style sets it so.
    pub fn road_label_text<'l>(&self, label: Option<&'l LabelFacts<'_>>, style: Option<&LabelTextStyle>) -> Option<Cow<'l, str>> {
        let (label, style) = (label?, style?);
        self.text_resolver.resolve_text(label, &self.language_preferences, style.uppercased)
    }

    /// The route signs whose numbers the atlas can render, in their order.
    pub fn renderable_route_shields<'s>(&self, shields: &'s [RouteShield]) -> Vec<&'s RouteShield> {
        shields
            .iter()
            .filter(|shield| !shield.text.is_empty() && self.text_resolver.glyph_coverage().can_render(&shield.text))
            .collect()
    }

    /// The identity of a road's route signs standing at `anchor` in a tile:
    /// local to the tile, since a road's signs stand in different places in
    /// every tile that holds the road.
    pub fn route_shield_key(&self, tile: Tile, text: &str, anchor: [i16; 2]) -> u64 {
        LabelIdentity::TileLocal {
            tile,
            layer_name: Cow::Borrowed("route-shield"),
            text: Cow::Borrowed(text),
            anchor,
        }
        .runtime_key()
    }

    /// How a point feature the style labels is labelled: its text (the name
    /// in the map's language), its identity across tiles, and the
    /// priorities and the sprite the style gave it. `None` when the feature
    /// carries no text the atlas can render.
    pub fn point_label_decision<'a>(
        &self,
        feature: &LabelFeature<'a>,
        label: &LabelFacts<'_>,
        style: &PointLabelStyle,
    ) -> Option<LabelDecision<'a>> {
        let text = self
            .text_resolver
            .resolve_text(label, &self.language_preferences, style.text.uppercased)?
            .into_owned();
        let identity = match feature.feature_id {
            Some(feature_id) if self.uses_feature_identity => LabelIdentity::StyleFeature {
                style_id: Cow::Owned(self.style_id.clone()),
                layer_name: Cow::Borrowed(feature.layer_name),
                feature_id,
            },
            _ => LabelIdentity::TileLocal {
                tile: feature.tile,
                layer_name: Cow::Borrowed(feature.layer_name),
                text: Cow::Owned(text.clone()),
                anchor: feature.anchor,
            },
        };
        Some(LabelDecision {
            text,
            identity,
            priority: LabelPriority {
                visibility_rank: style.rank,
                collision_rank: style.collision_rank,
                deduplication_rank: style.rank,
                draw_rank: style.rank,
            },
            placement: LabelPlacementIntent::CENTERED,
            style: style.text,
            poi_icon: style.icon,
        })
    }

    /// The spelling of a name the map shows, chosen from names keyed by
    /// language code (`"en"`, `"ru"`, ...) plus `"native"` for the local
    /// one, walking the same fallback chain a feature's names would, and
    /// skipping any the atlas cannot render. English is the last resort.
    /// For the labels the parser synthesizes itself, which carry no facts to
    /// resolve from.
    pub fn localized_name(&self, names: &[(&str, &str)]) -> Option<String> {
        let lookup = |code: &str| names.iter().find(|(key, _)| *key == code).map(|(_, name)| (*name).to_owned());
        self.text_resolver.localized_name(&lookup, &self.language_preferences)
    }
}
