// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use std::borrow::Cow;

use super::{GlyphCoverage, LabelRomanizer, LabelScript};
use crate::schema::LabelFacts;
use crate::support::{LabelFallbackPolicy, LabelLanguage};

/// What a candidate spelling of a name is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CandidateKind {
    Preferred,
    Native,
    /// The local name in Latin letters.
    RomanizedNative,
    /// The local name, only when it is written in the map language's
    /// alphabet (`LanguagePreferences::map_script`).
    NativeInMapScript,
    English,
}

/// One spelling the chain tries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageCandidate {
    /// The language code the spelling is looked up under, `None` for the
    /// local name and the romanized local name.
    pub language_code: Option<String>,
    pub kind: CandidateKind,
}

/// The order the spellings of a name are tried in for the map's language:
/// the preferred language, then English and the local name in the order the
/// fallback policy states. Under the international policy a map in a
/// language written in Latin letters tries the local name romanized before
/// the local name as it is (`LabelRomanizer`), so a name in another alphabet
/// shows in the map's. A map in a language written in another alphabet
/// tries the local name before English when the local name is written in
/// that alphabet: a Moscow shop with no `name:ru` is "Охотный Ряд" on a
/// Russian map, not "Okhotny Ryad". Which fields of a tile carry a
/// language's spelling is the schema reading's business (`LabelFacts`). The
/// chain speaks in language codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguagePreferences {
    pub fallback_chain: Vec<LanguageCandidate>,
    pub selected_language: LabelLanguage,
    pub fallback_policy: LabelFallbackPolicy,
    pub romanizer: LabelRomanizer,
    /// The alphabet the map's language is written in, `None` when the text
    /// logic does not tell it apart.
    pub map_script: Option<LabelScript>,
}

impl LanguagePreferences {
    pub fn new(language: &LabelLanguage, fallback_policy: LabelFallbackPolicy) -> Self {
        let english = LanguageCandidate { language_code: Some("en".to_owned()), kind: CandidateKind::English };
        let native = LanguageCandidate { language_code: None, kind: CandidateKind::Native };
        let romanizer = LabelRomanizer::new(language);
        let map_script = language.script();
        // In an alphabet other than Latin, a local name in the map's own
        // alphabet reads better than the English one.
        let native_in_map_script: Vec<LanguageCandidate> =
            if map_script.is_some_and(|script| script != LabelScript::Latin && script != LabelScript::Other) {
                vec![LanguageCandidate { language_code: None, kind: CandidateKind::NativeInMapScript }]
            } else {
                Vec::new()
            };
        let international_native: Vec<LanguageCandidate> = if romanizer.is_enabled {
            vec![LanguageCandidate { language_code: None, kind: CandidateKind::RomanizedNative }, native.clone()]
        } else {
            vec![native.clone()]
        };

        let fallback_chain = if *language == LabelLanguage::english() {
            std::iter::once(english).chain(international_native).collect()
        } else {
            let preferred = LanguageCandidate {
                language_code: Some(language.name_field_suffix().to_owned()),
                kind: CandidateKind::Preferred,
            };
            match fallback_policy {
                LabelFallbackPolicy::International => std::iter::once(preferred)
                    .chain(native_in_map_script)
                    .chain(std::iter::once(english))
                    .chain(international_native)
                    .collect(),
                LabelFallbackPolicy::LocalFirst => vec![preferred, native, english],
            }
        };

        LanguagePreferences {
            fallback_chain,
            selected_language: language.clone(),
            fallback_policy,
            romanizer,
            map_script,
        }
    }

    /// The candidate's text from a spelling the source states: the
    /// romanized form for a romanized candidate, nothing for a local name
    /// outside the map's alphabet.
    fn adapt<'t>(&self, kind: CandidateKind, text: Option<&'t str>) -> Option<Cow<'t, str>> {
        let text = text?;
        match kind {
            CandidateKind::RomanizedNative => self.romanizer.romanize(text).map(Cow::Owned),
            CandidateKind::NativeInMapScript => {
                self.map_script.is_some_and(|script| script.writes(text)).then_some(Cow::Borrowed(text))
            }
            CandidateKind::Preferred | CandidateKind::Native | CandidateKind::English => Some(Cow::Borrowed(text)),
        }
    }
}

/// Picks the spelling of a name the map shows: the first of the language
/// chain's candidates the feature carries and the text atlas can render.
#[derive(Debug, Clone)]
pub struct TextResolver {
    glyph_coverage: GlyphCoverage,
}

impl TextResolver {
    pub fn new(glyph_coverage: GlyphCoverage) -> Self {
        TextResolver { glyph_coverage }
    }

    pub fn resolve_text<'l>(
        &self,
        label: &'l LabelFacts<'_>,
        preferences: &LanguagePreferences,
        uppercased: bool,
    ) -> Option<Cow<'l, str>> {
        for candidate in &preferences.fallback_chain {
            let source = match &candidate.language_code {
                Some(code) => label.names_by_language.get(code),
                None => label.name.as_deref(),
            };
            let Some(mut text) = preferences.adapt(candidate.kind, source) else { continue };
            if uppercased {
                text = Cow::Owned(text.to_uppercase());
            }
            if text.is_empty() || !self.glyph_coverage.can_render(&text) {
                continue;
            }
            return Some(text);
        }
        None
    }

    /// The spelling of a name from names keyed by language code plus
    /// `"native"` for the local one, walking the same chain, and English as
    /// the last resort.
    pub(crate) fn localized_name(
        &self,
        names: &dyn Fn(&str) -> Option<String>,
        preferences: &LanguagePreferences,
    ) -> Option<String> {
        for candidate in &preferences.fallback_chain {
            let code = candidate.language_code.as_deref().unwrap_or("native");
            let source = names(code);
            let Some(text) = preferences.adapt(candidate.kind, source.as_deref()) else { continue };
            if text.is_empty() || !self.glyph_coverage.can_render(&text) {
                continue;
            }
            return Some(text.into_owned());
        }
        names("en").filter(|text| self.glyph_coverage.can_render(text))
    }

    pub fn glyph_coverage(&self) -> &GlyphCoverage {
        &self.glyph_coverage
    }
}
