// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::adaptation::LabelScript;

/// The language the map's names are shown in, by its code (`"en"`, `"pl"`,
/// `"pt-BR"`). The tiles' names are looked up by the code's language part
/// (`pt` of `pt-BR`).
///
/// The built-in font has Latin and Cyrillic letters only: a name in another
/// alphabet (Japanese, Chinese, Arabic, Greek and the rest) cannot be
/// drawn, so the label falls back as [`LabelFallbackPolicy`] says, to the
/// English name on an international map.
///
/// Stand-in for `ImmersiveMapSettings.LabelLanguage`
/// (`Configuration/ImmersiveMapSettings.swift`), with the part of
/// Foundation's `Locale.Language` the label logic reads (the script a
/// language is written in, and whether it is English).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LabelLanguage {
    code: String,
}

impl LabelLanguage {
    /// The code trimmed, with underscores as dashes, lowercased. An empty
    /// code is English.
    pub fn new(code: &str) -> Self {
        let normalized = code.trim().replace('_', "-").to_lowercase();
        if normalized.is_empty() {
            return Self::english();
        }
        LabelLanguage { code: normalized }
    }

    pub fn english() -> Self {
        LabelLanguage { code: "en".to_owned() }
    }
    pub fn russian() -> Self {
        Self::new("ru")
    }
    pub fn french() -> Self {
        Self::new("fr")
    }
    pub fn german() -> Self {
        Self::new("de")
    }
    pub fn spanish() -> Self {
        Self::new("es")
    }
    pub fn italian() -> Self {
        Self::new("it")
    }
    pub fn portuguese() -> Self {
        Self::new("pt")
    }
    pub fn turkish() -> Self {
        Self::new("tr")
    }

    /// The normalized code.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The language part of the code, what the tiles' `name:xx` fields are
    /// looked up by.
    pub fn name_field_suffix(&self) -> &str {
        self.code.split('-').find(|part| !part.is_empty()).unwrap_or("en")
    }

    /// The code with everything but `-`, digits and lowercase ASCII letters
    /// replaced by `_`: safe as a path component of the prepared-tile cache.
    pub fn prepared_tile_cache_namespace_key(&self) -> String {
        self.code
            .chars()
            .map(|c| if c == '-' || c.is_ascii_digit() || c.is_ascii_lowercase() { c } else { '_' })
            .collect()
    }

    /// Whether the language is English, whatever its region or script.
    pub fn is_english(&self) -> bool {
        self.subtags().0 == "en"
    }

    /// The script the language is written in as the label logic tells the
    /// scripts apart, nil when it is none of them (Japanese, Arabic and the
    /// rest). An explicit script subtag (`sr-latn`) wins, then the likely
    /// script of the language in its region (`sr-me` is Latin), then the
    /// language's own (`sr` is Cyrillic). The tables are the likely subtags
    /// Foundation's `Locale.Language.script` reads, for the five scripts
    /// told apart here.
    pub fn script(&self) -> Option<LabelScript> {
        let (language, script, region) = self.subtags();
        if let Some(script) = script {
            return match script {
                "latn" => Some(LabelScript::Latin),
                "cyrl" => Some(LabelScript::Cyrillic),
                "grek" => Some(LabelScript::Greek),
                "armn" => Some(LabelScript::Armenian),
                "geor" => Some(LabelScript::Georgian),
                _ => None,
            };
        }
        if let Some(region) = region
            && let Some(&(_, _, script)) =
                REGIONAL_SCRIPTS.iter().find(|(lang, reg, _)| *lang == language && *reg == region)
        {
            return script;
        }
        language_script(language)
    }

    /// The language, the script and the region subtags of the code.
    fn subtags(&self) -> (&str, Option<&str>, Option<&str>) {
        let mut parts = self.code.split('-').filter(|part| !part.is_empty());
        let language = parts.next().unwrap_or("");
        let mut script = None;
        let mut region = None;
        for part in parts {
            if script.is_none() && region.is_none() && part.len() == 4 && part.chars().all(|c| c.is_ascii_alphabetic()) {
                script = Some(part);
            } else if region.is_none()
                && ((part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()))
                    || (part.len() == 3 && part.chars().all(|c| c.is_ascii_digit())))
            {
                region = Some(part);
            }
        }
        (language, script, region)
    }
}

impl Default for LabelLanguage {
    fn default() -> Self {
        Self::english()
    }
}

impl Serialize for LabelLanguage {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.code)
    }
}

impl<'de> Deserialize<'de> for LabelLanguage {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let code = String::deserialize(deserializer)?;
        Ok(LabelLanguage::new(&code))
    }
}

/// What a label shows when the feature has no name in the map's language.
///
/// Stand-in for `ImmersiveMapSettings.LabelFallbackPolicy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LabelFallbackPolicy {
    /// The English name, then the local name. When the map's language is
    /// written in Latin letters, a local name in Cyrillic, Greek, Armenian
    /// or Georgian is romanized to plain ASCII first, so the map shows one
    /// alphabet ("Georgiyevskiy Lane" for "Георгиевский переулок" on an
    /// English map). A local name in another script is shown as it is.
    /// When the map's language is written in Cyrillic, Greek, Armenian or
    /// Georgian, a local name in that alphabet comes before the English
    /// one ("Охотный Ряд", not "Okhotny Ryad", on a Russian map).
    #[default]
    International,
    /// The local name as the place spells it, then the English name.
    LocalFirst,
}

/// The script of a language with no region, for the scripts the label
/// logic tells apart.
fn language_script(language: &str) -> Option<LabelScript> {
    if CYRILLIC_LANGUAGES.binary_search(&language).is_ok() {
        Some(LabelScript::Cyrillic)
    } else if LATIN_LANGUAGES.binary_search(&language).is_ok() {
        Some(LabelScript::Latin)
    } else if GREEK_LANGUAGES.contains(&language) {
        Some(LabelScript::Greek)
    } else if ARMENIAN_LANGUAGES.contains(&language) {
        Some(LabelScript::Armenian)
    } else if GEORGIAN_LANGUAGES.contains(&language) {
        Some(LabelScript::Georgian)
    } else {
        None
    }
}

/// The languages whose likely script changes with the region, where the
/// change matters to the scripts told apart here. `None` is a script of no
/// interest (Arabic, Mongolian and the rest).
const REGIONAL_SCRIPTS: &[(&str, &str, Option<LabelScript>)] = &[
    ("az", "iq", None),
    ("az", "ir", None),
    ("az", "ru", Some(LabelScript::Cyrillic)),
    ("ha", "cm", None),
    ("ha", "sd", None),
    ("kk", "af", None),
    ("kk", "cn", None),
    ("kk", "ir", None),
    ("kk", "mn", None),
    ("ku", "am", Some(LabelScript::Cyrillic)),
    ("ku", "az", Some(LabelScript::Cyrillic)),
    ("ku", "ge", Some(LabelScript::Cyrillic)),
    ("ku", "iq", None),
    ("ku", "ir", None),
    ("ku", "lb", None),
    ("ku", "tm", Some(LabelScript::Cyrillic)),
    ("ky", "cn", None),
    ("ky", "tr", Some(LabelScript::Latin)),
    ("lzz", "ge", Some(LabelScript::Georgian)),
    ("mn", "cn", None),
    ("ms", "cc", None),
    ("sr", "me", Some(LabelScript::Latin)),
    ("sr", "ro", Some(LabelScript::Latin)),
    ("sr", "tr", Some(LabelScript::Latin)),
    ("tg", "pk", None),
    ("ug", "kz", Some(LabelScript::Cyrillic)),
    ("ug", "mn", Some(LabelScript::Cyrillic)),
    ("uz", "af", None),
    ("uz", "cn", Some(LabelScript::Cyrillic)),
];

const GREEK_LANGUAGES: &[&str] = &["el", "grc", "pnt", "tsd"];
const ARMENIAN_LANGUAGES: &[&str] = &["hy"];
const GEORGIAN_LANGUAGES: &[&str] = &["ka", "xmf"];

/// Sorted, for a binary search.
const CYRILLIC_LANGUAGES: &[&str] = &[
    "ab", "ady", "alt", "av", "ba", "be", "bg", "bua", "ce", "chm", "crh", "cu", "cv", "dar", "inh", "kaa", "kbd",
    "kk", "koi", "krc", "kum", "kv", "ky", "lez", "mdf", "mk", "mn", "mrj", "myv", "nog", "os", "ru", "rue", "sah",
    "sel", "sjd", "sr", "tg", "tt", "tyv", "udm", "uk", "xal",
];

/// Sorted, for a binary search.
const LATIN_LANGUAGES: &[&str] = &[
    "aa", "ace", "ach", "ada", "af", "afh", "agq", "ak", "akz", "ale", "aln", "an", "ang", "apw", "arn", "aro", "arp",
    "arw", "asa", "ast", "avk", "ay", "az", "ban", "bar", "bas", "bbc", "bbj", "bem", "bew", "bez", "bfd", "bi",
    "bik", "bin", "bjn", "bkm", "bla", "blo", "bm", "br", "bs", "bss", "bug", "bum", "byv", "ca", "cad", "car",
    "cay", "cch", "ceb", "cgg", "ch", "chb", "chk", "chn", "cho", "chp", "chy", "cic", "co", "cps", "cs", "csb",
    "cst", "cy", "da", "dak", "dav", "de", "del", "den", "dgr", "din", "dje", "dsb", "dtp", "dua", "dum", "dyo",
    "dyu", "dzg", "ebu", "ee", "efi", "egl", "eka", "en", "enm", "eo", "es", "esu", "et", "eu", "ewo", "ext", "fan",
    "ff", "fi", "fil", "fit", "fj", "fo", "fon", "fr", "frc", "frm", "fro", "frp", "frr", "frs", "fur", "fy", "ga",
    "gaa", "gag", "gay", "gba", "gd", "gil", "gl", "gmh", "gn", "goh", "gor", "grb", "gsw", "guc", "gur", "guz",
    "gv", "gwi", "ha", "hai", "haw", "hch", "hil", "ho", "hr", "hsb", "ht", "hu", "hup", "hz", "ia", "iba", "ibb",
    "id", "ie", "ig", "ik", "ilo", "io", "is", "isc", "it", "izh", "jam", "jbo", "jgo", "jmc", "jut", "jv", "kab",
    "kac", "kaj", "kam", "kbl", "kcg", "kde", "kea", "ken", "kfo", "kg", "kgp", "kha", "khq", "ki", "kiu", "kj",
    "kkj", "kl", "kln", "kmb", "kos", "kpe", "kr", "kri", "krj", "krl", "ksb", "ksf", "ksh", "ku", "kut", "kw",
    "kxv", "la", "lag", "lam", "lb", "lfn", "lg", "li", "lij", "liv", "lkt", "lmo", "ln", "lol", "loz", "lt", "ltg",
    "lu", "lua", "lui", "lun", "luo", "lus", "lut", "luy", "lv", "lzz", "mad", "maf", "mak", "man", "mas", "mdh",
    "mdr", "men", "mer", "mfe", "mg", "mgh", "mgo", "mh", "mi", "mic", "min", "moh", "mos", "ms", "mt", "mua", "mus",
    "mwl", "mwv", "mye", "na", "nap", "naq", "nb", "nd", "nds", "nez", "ng", "nia", "niu", "njo", "nl", "nmg", "nn",
    "nnh", "no", "nov", "nr", "nso", "nus", "nv", "ny", "nym", "nyn", "nyo", "nzi", "oc", "om", "pag", "pam", "pap",
    "pau", "pcd", "pcm", "pdc", "pdt", "pfl", "pi", "pl", "pms", "pon", "pqm", "prg", "pro", "pt", "qu", "quc",
    "qug", "rap", "rar", "rej", "rgn", "rif", "rm", "rn", "ro", "rof", "rom", "rtm", "rug", "rup", "rw", "rwk",
    "sad", "saq", "sas", "sba", "sbp", "sc", "scn", "sco", "sdc", "se", "see", "seh", "sei", "ses", "sg", "sga",
    "sgs", "shp", "sid", "sje", "sju", "sk", "sl", "sli", "sly", "sm", "sma", "smj", "smn", "sms", "sn", "snk", "so",
    "sq", "srn", "srr", "ss", "ssy", "st", "stq", "su", "suk", "sus", "sv", "sw", "szl", "tem", "teo", "ter", "tet",
    "tiv", "tk", "tkl", "tkr", "tli", "tly", "tmh", "tn", "to", "tog", "tok", "tpi", "tr", "tru", "trv", "ts", "tsi",
    "ttt", "tum", "tvl", "twq", "ty", "tzm", "umb", "und", "uz", "ve", "vec", "vep", "vi", "vls", "vmf", "vmw", "vo",
    "vot", "vro", "vun", "wa", "wae", "war", "was", "wbp", "wo", "xh", "xog", "yao", "yap", "yav", "ybb", "yo", "yrl",
    "za", "zap", "zea", "zu", "zun", "zza",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_are_sorted() {
        assert!(LATIN_LANGUAGES.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(CYRILLIC_LANGUAGES.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn the_script_follows_the_likely_subtags() {
        assert_eq!(LabelLanguage::new("en").script(), Some(LabelScript::Latin));
        assert_eq!(LabelLanguage::new("ru").script(), Some(LabelScript::Cyrillic));
        assert_eq!(LabelLanguage::new("el").script(), Some(LabelScript::Greek));
        assert_eq!(LabelLanguage::new("hy").script(), Some(LabelScript::Armenian));
        assert_eq!(LabelLanguage::new("ka").script(), Some(LabelScript::Georgian));
        assert_eq!(LabelLanguage::new("ja").script(), None);
        assert_eq!(LabelLanguage::new("sr").script(), Some(LabelScript::Cyrillic));
        assert_eq!(LabelLanguage::new("sr-Latn").script(), Some(LabelScript::Latin));
        assert_eq!(LabelLanguage::new("sr-ME").script(), Some(LabelScript::Latin));
        assert_eq!(LabelLanguage::new("uz-cyrl").script(), Some(LabelScript::Cyrillic));
        assert_eq!(LabelLanguage::new("pt_BR").script(), Some(LabelScript::Latin));
        assert_eq!(LabelLanguage::new("zh-hans").script(), None);
        assert!(LabelLanguage::new("en-GB").is_english());
        assert!(!LabelLanguage::new("de").is_english());
    }
}
