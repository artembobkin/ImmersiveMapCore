// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! The label policy a tile parse asks questions of
//! ([`TileLabelDecisions`]): the name language selection
//! ([`LanguagePreferences`], [`TextResolver`]), the romanization of a local
//! name for a map in Latin letters ([`LabelRomanizer`]), the glyph coverage
//! of the text atlas, and how a label is identified and ranked.
//!
//! Schema-specific tile logic is confined to this module, `schema::default`
//! and `style::default`. The renderer, the labels runtime and the tiles
//! consume only schema-neutral, already normalized data.

mod decisions;
mod glyph_coverage;
mod romanizer;
mod romanizer_table;
mod script;
mod text;

pub use decisions::{
    LabelAnchorMode, LabelCollisionShape, LabelDecision, LabelFeature, LabelIdentity, LabelPlacementIntent,
    LabelPriority, StableHasher, TileLabelDecisions,
};
pub use glyph_coverage::GlyphCoverage;
pub use romanizer::LabelRomanizer;
pub use script::LabelScript;
pub use text::{CandidateKind, LanguageCandidate, LanguagePreferences, TextResolver};
