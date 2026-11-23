// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

//! Ported from `TileLabelDecisionsTests`,
//! `VectorTileLabelLanguagePreferencesTests`, `VectorTileLabelRomanizerTests`,
//! `VectorTileLabelGlyphCoverageTests` and `CustomVectorTileStyleLabelTests`.
//!
//! Skipped: the parsed text label's key (`ParsedTextLabel` is the
//! parser's), and the coverage of the bundled atlases and their distance
//! range (the atlas files are the renderer's).

mod support;

use std::borrow::Cow;

use map_style::adaptation::{
    CandidateKind, GlyphCoverage, LabelDecision, LabelFeature, LabelIdentity, LabelRomanizer, LanguagePreferences,
    TextResolver, TileLabelDecisions,
};
use map_style::schema::default::ProtomapsBasemapSchema;
use map_style::schema::{Feature, FeatureFacts, FeatureGeometry, LabelFacts, TileSchema};
use map_style::style::{
    BasicVectorTileStyle, FeatureStyle, FeatureStyleContext, LabelFontWeight, LabelTextStyle, MapStyle, PointLabelOptions,
    VectorTileMapStyle, VectorTileStyle,
};
use map_style::support::{LabelFallbackPolicy, LabelLanguage, Tile};
use mvt::Value;
use support::*;

fn label<'a>(props: &[(&'a str, Value<'a>)]) -> LabelFacts<'a> {
    facts("places", props, 10, FeatureGeometry::Unknown).label().cloned().unwrap_or_default()
}

fn resolve(props: &[(&str, Value<'_>)], language: LabelLanguage, policy: LabelFallbackPolicy) -> Option<String> {
    let resolver = TextResolver::new(GlyphCoverage::legacy_atlas_for_tests());
    let facts = label(props);
    resolver.resolve_text(&facts, &LanguagePreferences::new(&language, policy), false).map(Cow::into_owned)
}

fn codes(preferences: &LanguagePreferences) -> Vec<Option<&str>> {
    preferences.fallback_chain.iter().map(|candidate| candidate.language_code.as_deref()).collect()
}

fn kinds(preferences: &LanguagePreferences) -> Vec<CandidateKind> {
    preferences.fallback_chain.iter().map(|candidate| candidate.kind).collect()
}

const INTERNATIONAL: LabelFallbackPolicy = LabelFallbackPolicy::International;
const LOCAL_FIRST: LabelFallbackPolicy = LabelFallbackPolicy::LocalFirst;

// MARK: The language chain

#[test]
fn english_chain_tries_english_then_the_romanized_native_name() {
    let preferences = LanguagePreferences::new(&LabelLanguage::english(), INTERNATIONAL);
    assert_eq!(codes(&preferences), [Some("en"), None, None]);
    assert_eq!(kinds(&preferences), [CandidateKind::English, CandidateKind::RomanizedNative, CandidateKind::Native]);
}

/// A map in a language not written in Latin letters has nothing to romanize
/// into: a local name in its alphabet comes before English, and any other
/// local name follows English as it is.
#[test]
fn non_latin_international_chain_tries_the_native_name_in_its_alphabet_before_english() {
    let preferences = LanguagePreferences::new(&LabelLanguage::russian(), INTERNATIONAL);
    assert_eq!(codes(&preferences), [Some("ru"), None, Some("en"), None]);
    assert_eq!(
        kinds(&preferences),
        [CandidateKind::Preferred, CandidateKind::NativeInMapScript, CandidateKind::English, CandidateKind::Native]
    );
    assert_eq!(preferences.selected_language, LabelLanguage::russian());
    assert_eq!(preferences.fallback_policy, INTERNATIONAL);
}

/// A language whose alphabet the text logic does not tell apart keeps
/// English before the local name.
#[test]
fn unknown_script_international_chain_keeps_english_before_native() {
    let preferences = LanguagePreferences::new(&LabelLanguage::new("ja"), INTERNATIONAL);
    assert_eq!(kinds(&preferences), [CandidateKind::Preferred, CandidateKind::English, CandidateKind::Native]);
}

#[test]
fn latin_international_chain_romanizes_before_the_native_name() {
    let preferences = LanguagePreferences::new(&LabelLanguage::german(), INTERNATIONAL);
    assert_eq!(
        kinds(&preferences),
        [CandidateKind::Preferred, CandidateKind::English, CandidateKind::RomanizedNative, CandidateKind::Native]
    );
    assert_eq!(codes(&LanguagePreferences::new(&LabelLanguage::french(), INTERNATIONAL)), [Some("fr"), Some("en"), None, None]);
}

/// Local first asks for the local name: it is shown as the place spells it.
#[test]
fn local_first_chain_does_not_romanize() {
    let preferences = LanguagePreferences::new(&LabelLanguage::german(), LOCAL_FIRST);
    assert_eq!(kinds(&preferences), [CandidateKind::Preferred, CandidateKind::Native, CandidateKind::English]);
    assert_eq!(codes(&LanguagePreferences::new(&LabelLanguage::russian(), LOCAL_FIRST)), [Some("ru"), None, Some("en")]);
    let french = LanguagePreferences::new(&LabelLanguage::french(), LOCAL_FIRST);
    assert_eq!(french.fallback_policy, LOCAL_FIRST);
    assert_eq!(codes(&french), [Some("fr"), None, Some("en")]);
}

// MARK: The resolver

#[test]
fn the_resolver_reads_the_colon_form_and_not_the_underscore_form() {
    assert_eq!(
        resolve(&[("name", s("América")), ("name:en", s("Americas"))], LabelLanguage::english(), INTERNATIONAL).as_deref(),
        Some("Americas")
    );
    assert_eq!(
        resolve(&[("name", s("Deutschland")), ("name_en", s("Germany"))], LabelLanguage::english(), INTERNATIONAL).as_deref(),
        Some("Deutschland"),
        "the basemap spells a language with a colon, and nothing else is one"
    );
    assert_eq!(resolve(&[("name", s("Norge"))], LabelLanguage::english(), INTERNATIONAL).as_deref(), Some("Norge"));
}

#[test]
fn russian_preferences_prefer_russian_then_cyrillic_native_then_english_then_native() {
    let moscow = [("name", s("Москва")), ("name:en", s("Moscow")), ("name:ru", s("Москва"))];
    assert_eq!(resolve(&moscow, LabelLanguage::russian(), INTERNATIONAL).as_deref(), Some("Москва"));
    // The Moscow POIs the basemap ships with no `name:ru`: the local name is
    // Russian already, and it beats the English one.
    let row = [("name", s("Охотный Ряд")), ("name:en", s("Okhotny Ryad"))];
    assert_eq!(resolve(&row, LabelLanguage::russian(), INTERNATIONAL).as_deref(), Some("Охотный Ряд"));
    // A local name in another alphabet, or one mixing in Latin, still
    // yields to English on a Russian map.
    let rivoli = [("name", s("Rue de Rivoli")), ("name:en", s("Rivoli Street"))];
    assert_eq!(resolve(&rivoli, LabelLanguage::russian(), INTERNATIONAL).as_deref(), Some("Rivoli Street"));
    let mall = [("name", s("ТЦ Mega")), ("name:en", s("Mega Mall"))];
    assert_eq!(resolve(&mall, LabelLanguage::russian(), INTERNATIONAL).as_deref(), Some("Mega Mall"));
    assert_eq!(resolve(&[("name", s("Москва"))], LabelLanguage::russian(), INTERNATIONAL).as_deref(), Some("Москва"));
}

#[test]
fn latin_preferences_fall_back_through_english_and_the_romanized_name() {
    let moscow = [("name", s("Москва")), ("name:en", s("Moscow"))];
    assert_eq!(resolve(&moscow, LabelLanguage::french(), INTERNATIONAL).as_deref(), Some("Moscow"));
    assert_eq!(resolve(&moscow, LabelLanguage::french(), LOCAL_FIRST).as_deref(), Some("Москва"));
    let english = [("name", s("Moscow Native")), ("name:en", s("Moscow EN")), ("name:ru", s("Москва"))];
    assert_eq!(resolve(&english, LabelLanguage::english(), INTERNATIONAL).as_deref(), Some("Moscow EN"));
    let paris = [("name", s("Paris Native")), ("name:en", s("Paris EN")), ("name:fr", s("Paris FR"))];
    assert_eq!(resolve(&paris, LabelLanguage::french(), INTERNATIONAL).as_deref(), Some("Paris FR"));
    let rivoli = [("name", s("Rue Native")), ("name:en", s("Rivoli Street")), ("name:fr", s("Rue de Rivoli"))];
    assert_eq!(resolve(&rivoli, LabelLanguage::french(), INTERNATIONAL).as_deref(), Some("Rue de Rivoli"));
    assert_eq!(resolve(&[("name:en", s("Munich EN"))], LabelLanguage::german(), INTERNATIONAL).as_deref(), Some("Munich EN"));
    let latin = [("name", s("Moscow")), ("name:ru", s("Москва"))];
    assert_eq!(resolve(&latin, LabelLanguage::english(), INTERNATIONAL).as_deref(), Some("Moscow"));
    let cyrillic = [("name", s("Москва")), ("name:ru", s("Москва"))];
    assert_eq!(resolve(&cyrillic, LabelLanguage::english(), INTERNATIONAL).as_deref(), Some("Moskva"));
    assert_eq!(resolve(&[("name", s("Москва"))], LabelLanguage::german(), LOCAL_FIRST).as_deref(), Some("Москва"));
}

#[test]
fn unsupported_glyph_coverage_rejects_text() {
    assert_eq!(resolve(&[("name", s("東京"))], LabelLanguage::english(), INTERNATIONAL), None);
}

// MARK: Identities

#[test]
fn style_feature_identity_participates_in_cross_tile_deduplication() {
    let identity = LabelIdentity::StyleFeature { style_id: "example".into(), layer_name: "place_label".into(), feature_id: 42 };
    assert!(identity.participates_in_cross_tile_deduplication());
    assert_eq!(identity.runtime_key(), 8141700374101987561);
}

#[test]
fn tile_local_identity_includes_tile_coordinates() {
    let first = LabelIdentity::TileLocal { tile: Tile::new(10, 20, 5), layer_name: "poi_label".into(), text: "Museum".into(), anchor: [100, 200] };
    let second = LabelIdentity::TileLocal { tile: Tile::new(11, 20, 5), layer_name: "poi_label".into(), text: "Museum".into(), anchor: [100, 200] };
    assert!(!first.participates_in_cross_tile_deduplication());
    assert_eq!(first.runtime_key(), 6949302229354522716);
    assert_eq!(second.runtime_key(), 6830255165424541913);
}

#[test]
fn the_decisions_build_a_text_label_compatible_decision() {
    let style = default_style();
    let decisions = TileLabelDecisions::new(&style, GlyphCoverage::legacy_atlas_for_tests(), &LabelLanguage::english(), INTERNATIONAL);
    let tile = Tile::new(123, 456, 10);
    let props = [("name:en", s("Moscow")), ("kind", s("locality")), ("kind_detail", s("city")), ("population_rank", i(15))];
    let feature_style = style_on_tile(&style, "places", &props, tile, FeatureGeometry::Point);
    let label_style = feature_style.point_label_style().unwrap();
    let feature = LabelFeature { style_id: "protomaps", tile, layer_name: "places", feature_id: Some(7), anchor: [2048, 2048] };
    let facts = ProtomapsBasemapSchema.read(&Feature::new("places", tile, FeatureGeometry::Unknown, &props));
    let decision = decisions.point_label_decision(&feature, facts.label().unwrap(), label_style).unwrap();
    assert_eq!(decision.text, "Moscow");
    // A population rank of 15 out of 18, a city: (18 - 15) * 10 + 2.
    assert_eq!(decision.priority.visibility_rank, 32, "The rank is the style's reading of the tile");
    assert_eq!(decision.priority.collision_rank, 32, "A place collides at its own rank");
    assert_eq!(decision.identity, LabelIdentity::StyleFeature { style_id: "protomaps".into(), layer_name: "places".into(), feature_id: 7 });
    assert_eq!(decision.style.key, label_style.text.key);
    assert_eq!(decision.style.size_points, label_style.text.size_points);
}

#[test]
fn the_decisions_answer_for_roads_signs_and_synthesized_names() {
    let decisions = TileLabelDecisions::new(&default_style(), GlyphCoverage::legacy_atlas_for_tests(), &LabelLanguage::english(), INTERNATIONAL);
    let road = label(&[("name", s("улица Щепкина"))]);
    let capitals = LabelTextStyle { uppercased: true, ..LabelTextStyle::new([0.0; 3], [1.0; 3], 0.1, 11.0, LabelFontWeight::Thin) };
    assert_eq!(decisions.road_label_text(Some(&road), Some(&capitals)).as_deref(), Some("SHCHEPKINA STREET"));
    assert_eq!(decisions.road_label_text(None, Some(&capitals)), None);
    assert_eq!(decisions.road_label_text(Some(&road), None), None);
    let shields = map_style::style::default::RouteShields::default()
        .shields(&[map_style::schema::RouteFacts::unnetworked("M10"), map_style::schema::RouteFacts::unnetworked("東1")]);
    assert_eq!(decisions.renderable_route_shields(&shields).iter().map(|shield| shield.text.as_str()).collect::<Vec<_>>(), ["M10"]);
    let key = decisions.route_shield_key(Tile::new(1, 2, 3), "M10", [10, 20]);
    assert_eq!(
        key,
        LabelIdentity::TileLocal { tile: Tile::new(1, 2, 3), layer_name: "route-shield".into(), text: "M10".into(), anchor: [10, 20] }
            .runtime_key()
    );
    assert_eq!(decisions.localized_name(&[("native", "Москва"), ("ja", "モスクワ")]).as_deref(), Some("Moskva"));
    assert_eq!(decisions.localized_name(&[("en", "Atlantic Ocean")]).as_deref(), Some("Atlantic Ocean"));
    assert_eq!(decisions.localized_name(&[("native", "東京")]), None);
}

// MARK: The label language setting

#[test]
fn label_language_normalizes_bcp47_codes() {
    let language = LabelLanguage::new("PT-BR");
    assert_eq!(language.code(), "pt-br");
    assert_eq!(language.name_field_suffix(), "pt");
    assert_eq!(language.prepared_tile_cache_namespace_key(), "pt-br");
    assert_eq!(LabelLanguage::new("pt_BR").code(), "pt-br");
    let hostile = LabelLanguage::new("EN/../../secret:token").prepared_tile_cache_namespace_key();
    assert!(!hostile.contains('/'));
    assert!(!hostile.contains(':'));
    assert!(!hostile.contains(".."));
    assert_eq!(LabelLanguage::new("  ").code(), "en");
}

#[test]
fn known_label_languages_remain_available() {
    let codes: Vec<String> = [
        LabelLanguage::english(),
        LabelLanguage::russian(),
        LabelLanguage::french(),
        LabelLanguage::german(),
        LabelLanguage::spanish(),
        LabelLanguage::italian(),
        LabelLanguage::portuguese(),
        LabelLanguage::turkish(),
    ]
    .iter()
    .map(|language| language.code().to_owned())
    .collect();
    assert_eq!(codes, ["en", "ru", "fr", "de", "es", "it", "pt", "tr"]);
}

// MARK: The romanizer

fn english() -> LabelRomanizer {
    LabelRomanizer::new(&LabelLanguage::english())
}

/// BGN, folded to ASCII, with the soft and hard signs dropped.
#[test]
fn cyrillic_is_romanized_through_bgn_without_diacritics() {
    assert_eq!(english().romanize("Большой театр (Новая сцена)").as_deref(), Some("Bolshoy teatr (Novaya stsena)"));
    assert_eq!(english().romanize("Воробьёвы горы").as_deref(), Some("Vorobyevy gory"));
    assert_eq!(english().romanize("Ельцин-центр").as_deref(), Some("Yeltsin-tsentr"));
}

#[test]
fn english_moves_a_translated_street_type_to_the_end() {
    assert_eq!(english().romanize("Георгиевский переулок").as_deref(), Some("Georgiyevskiy Lane"));
    assert_eq!(english().romanize("улица Щепкина").as_deref(), Some("Shchepkina Street"));
    assert_eq!(english().romanize("Малая Бронная улица").as_deref(), Some("Malaya Bronnaya Street"));
    assert_eq!(english().romanize("M10 · Ленинградское шоссе").as_deref(), Some("M10 · Leningradskoye Highway"));
}

/// A capitalized type word is part of a proper name, and a lone one is the
/// whole name: both stay transliterated.
#[test]
fn a_street_type_that_is_part_of_the_name_stays() {
    assert_eq!(english().romanize("Площадь Революции").as_deref(), Some("Ploshchad Revolyutsii"));
    assert_eq!(english().romanize("улица").as_deref(), Some("ulitsa"));
}

/// Only English translates the street type: other languages keep the
/// transliterated word.
#[test]
fn other_latin_languages_keep_the_transliterated_street_type() {
    let german = LabelRomanizer::new(&LabelLanguage::german());
    assert_eq!(german.romanize("Георгиевский переулок").as_deref(), Some("Georgiyevskiy pereulok"));
}

#[test]
fn greek_armenian_and_georgian_are_romanized() {
    assert_eq!(english().romanize("Αθήνα").as_deref(), Some("Athena"));
    assert_eq!(english().romanize("Երևան").as_deref(), Some("Erevan"));
    assert_eq!(english().romanize("თბილისი").as_deref(), Some("Tbilisi"), "Georgian writes names without a capital, Latin with one");
}

/// The generic transliterator reads kanji as Mandarin and drops the vowels
/// of Arabic: those names are declined, whole.
#[test]
fn other_scripts_are_declined() {
    assert_eq!(english().romanize("東京駅"), None);
    assert_eq!(english().romanize("القاهرة"), None);
    assert_eq!(english().romanize("Москва 東京"), None);
}

#[test]
fn a_latin_name_is_left_to_the_native_candidate() {
    assert_eq!(english().romanize("Zürich"), None);
    assert_eq!(english().romanize("42"), None);
}

#[test]
fn a_map_in_a_non_latin_language_romanizes_nothing() {
    let russian = LabelRomanizer::new(&LabelLanguage::russian());
    assert!(!russian.is_enabled);
    assert_eq!(russian.romanize("Αθήνα"), None);
}

// MARK: The glyph coverage

#[test]
fn coverage_allows_only_glyphs_present_in_the_atlas() {
    let coverage = GlyphCoverage::new([65, 66, 32]);
    assert!(coverage.can_render("A B"));
    assert!(!coverage.can_render("AC"));
}

#[test]
fn coverage_allows_layout_controls_outside_the_glyph_set() {
    let coverage = GlyphCoverage::new([65]);
    for text in ["A\nA", "A\tA", "A\rA", "A\u{00A0}A"] {
        assert!(coverage.can_render(text), "{text:?}");
    }
}

#[test]
fn coverage_combines_bold_and_thin_atlas_glyphs() {
    let coverage = GlyphCoverage::from_atlases([65], [66]);
    assert!(coverage.can_render("AB"));
    assert!(!coverage.can_render("ABC"));
    let bold_only = GlyphCoverage::from_atlases([65], []);
    assert!(bold_only.can_render("A A\nA\rA\tA"));
    assert!(!bold_only.can_render("AЖA"));
    assert!(!GlyphCoverage::legacy_atlas_for_tests().can_render("東京"));
}

// MARK: A custom style and schema

/// A schema whose labels carry their text in `title` (after the usual
/// `name` fields).
struct CustomLabelTestSchema;

impl TileSchema for CustomLabelTestSchema {
    fn cache_fingerprint(&self) -> u32 {
        1
    }

    fn read<'a>(&self, feature: &Feature<'_, 'a>) -> FeatureFacts<'a> {
        // The basemap's names reading over a layer of this schema's own.
        let mut label = ProtomapsBasemapSchema.names(&feature.properties).unwrap_or_default();
        if label.name.is_none()
            && let Some(title) = feature.properties.string("title")
        {
            label.name = Some(Cow::Borrowed(title));
        }
        FeatureFacts::Labelled(label)
    }
}

struct CustomLabelTestStyle {
    labels_use_feature_identity: bool,
}

impl VectorTileStyle for CustomLabelTestStyle {
    fn cache_fingerprint(&self) -> u32 {
        1
    }

    fn style_id(&self) -> &str {
        "custom"
    }

    fn labels_use_feature_identity(&self) -> bool {
        self.labels_use_feature_identity
    }

    fn make_style(&self, feature: &FeatureStyleContext<'_>) -> FeatureStyle {
        let text = LabelTextStyle::new([0.1, 0.1, 0.1], [1.0, 1.0, 1.0], 0.15, 12.0, LabelFontWeight::Thin);
        match feature.layer_name {
            "custom_label" => FeatureStyle::point_label(70, text, PointLabelOptions { rank: 7, collision_rank: Some(70), ..PointLabelOptions::default() }),
            "address_label" => FeatureStyle::point_label(71, text, PointLabelOptions { rank: 100, ..PointLabelOptions::default() }),
            _ => FeatureStyle::Hidden,
        }
    }
}

fn decide(props: &[(&'static str, Value<'static>)], feature_id: Option<u64>, uses_feature_identity: bool) -> Option<LabelDecision<'static>> {
    let style = CustomLabelTestStyle { labels_use_feature_identity: uses_feature_identity };
    let decisions = TileLabelDecisions::new(&style, GlyphCoverage::legacy_atlas_for_tests(), &LabelLanguage::english(), INTERNATIONAL);
    let tile = Tile::new(1, 2, 10);
    let feature = Feature::new("custom_label", tile, FeatureGeometry::Point, props);
    let facts = CustomLabelTestSchema.read(&feature);
    let feature_style = style.make_style(&FeatureStyleContext::new(style.style_id(), &feature, &facts));
    let label_feature = LabelFeature { style_id: "custom", tile, layer_name: "custom_label", feature_id, anchor: [100, 200] };
    decisions.point_label_decision(&label_feature, facts.label()?, feature_style.point_label_style()?)
}

#[test]
fn a_custom_schema_states_the_text_and_a_custom_style_its_ranks() {
    assert_eq!(decide(&[("title", s("Custom Cafe"))], None, true).unwrap().text, "Custom Cafe");
    assert_eq!(
        decide(&[("name:en", s("Cafe")), ("title", s("Custom Cafe"))], None, true).unwrap().text,
        "Cafe",
        "the language chain is read before the style's text keys"
    );
    assert!(decide(&[("category", s("food"))], None, true).is_none(), "a feature without text is no label");
    let decision = decide(&[("title", s("Custom Cafe"))], None, true).unwrap();
    assert_eq!(decision.priority.visibility_rank, 7);
    assert_eq!(decision.priority.deduplication_rank, 7);
    assert_eq!(decision.priority.draw_rank, 7);
    assert_eq!(decision.priority.collision_rank, 70);
}

#[test]
fn labels_are_identified_in_the_style_namespace_by_feature_id() {
    let tile_local = LabelIdentity::TileLocal { tile: Tile::new(1, 2, 10), layer_name: "custom_label".into(), text: "Custom Cafe".into(), anchor: [100, 200] };
    assert_eq!(
        decide(&[("title", s("Custom Cafe"))], Some(42), true).unwrap().identity,
        LabelIdentity::StyleFeature { style_id: "custom".into(), layer_name: "custom_label".into(), feature_id: 42 }
    );
    assert_eq!(decide(&[("title", s("Custom Cafe"))], None, true).unwrap().identity, tile_local, "a feature without an id is identified by its tile");
    assert_eq!(decide(&[("title", s("Custom Cafe"))], Some(42), false).unwrap().identity, tile_local, "a style can opt out of feature identity");
}

#[test]
fn the_style_fingerprint_is_the_map_style_fingerprint() {
    let one = VectorTileMapStyle::with_basemap_schema(std::sync::Arc::new(BasicVectorTileStyle::new(1)));
    let two = VectorTileMapStyle::with_basemap_schema(std::sync::Arc::new(BasicVectorTileStyle::new(2)));
    assert_ne!(one.configuration_fingerprint(), two.configuration_fingerprint());
    assert_eq!(one.configuration_fingerprint(), (4u64 << 32) | 1);
}
