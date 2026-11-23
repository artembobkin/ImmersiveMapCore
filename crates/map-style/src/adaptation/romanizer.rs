// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use super::romanizer_table::LETTERS;
use super::LabelScript;
use crate::support::LabelLanguage;

/// Writes a local name in Latin letters for a map whose language is written
/// in them, so a street that has no spelling in the map's language reads
/// "Georgiyevskiy Lane" instead of "Георгиевский переулок" and the map shows
/// one alphabet.
///
/// The engine's romanizer runs the system's ICU transliterators: Cyrillic
/// through the BGN romanization (the one maps use), Greek, Armenian and
/// Georgian through the generic one, then folded to plain ASCII, with the
/// soft and hard signs dropped rather than written as apostrophes. This is
/// that transform rewritten as a letter table read off it
/// (`romanizer_table`) and the context rules of the BGN romanization it
/// applies (the `ye` of a word start or after a vowel, the middle dot that
/// keeps `t·s` from reading as `ts`), so a browser without ICU writes the
/// same spellings. Only the words in those scripts are touched: a Latin part
/// of the name (a route number, a brand) keeps its spelling and
/// punctuation. A name with a letter of any other script is declined,
/// because the generic transliterator reads Japanese kanji as Mandarin and
/// drops the vowels of Arabic, and a wrong spelling is worse than the local
/// one.
///
/// For an English map a trailing or leading Russian street type is also
/// translated and moved to the end, the way English names streets:
/// "улица Щепкина" becomes "Shchepkina Street". Only a lowercase type word
/// counts, so a capitalized one that is part of a proper name, like the
/// metro station "Площадь Революции", stays transliterated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LabelRomanizer {
    /// False when the map's language is not written in Latin letters:
    /// nothing is romanized then.
    pub is_enabled: bool,
    translates_russian_street_types: bool,
}

/// A run of letters of a script this romanizes, as opposed to the text
/// between such runs (spaces, punctuation, digits, Latin).
struct Word {
    text: String,
    is_romanizable: bool,
}

#[derive(PartialEq)]
enum Script {
    Latin,
    /// Cyrillic, Greek, Armenian and Georgian.
    Romanizable,
    /// A letter of any other script.
    Unsupported,
    /// Not a letter: digits, punctuation, spaces, combining marks.
    Other,
}

fn script(c: char) -> Script {
    match LabelScript::of(c) {
        None => Script::Other,
        Some(LabelScript::Latin) => Script::Latin,
        Some(LabelScript::Cyrillic | LabelScript::Greek | LabelScript::Armenian | LabelScript::Georgian) => {
            Script::Romanizable
        }
        Some(LabelScript::Other) => Script::Unsupported,
    }
}

/// The English words for the Russian street types, keyed by the lowercase
/// Russian word.
const RUSSIAN_STREET_TYPES: [(&str, &str); 11] = [
    ("улица", "Street"),
    ("переулок", "Lane"),
    ("проспект", "Avenue"),
    ("площадь", "Square"),
    ("бульвар", "Boulevard"),
    ("шоссе", "Highway"),
    ("набережная", "Embankment"),
    ("проезд", "Passage"),
    ("аллея", "Alley"),
    ("мост", "Bridge"),
    ("тупик", "Dead End"),
];

impl LabelRomanizer {
    pub fn new(target_language: &LabelLanguage) -> Self {
        LabelRomanizer {
            is_enabled: target_language.script() == Some(LabelScript::Latin),
            translates_russian_street_types: target_language.is_english(),
        }
    }

    /// The name in ASCII Latin letters, or `None` when there is nothing to
    /// romanize (the name is already Latin) or the name carries a script
    /// this does not handle.
    pub fn romanize(&self, name: &str) -> Option<String> {
        if !self.is_enabled {
            return None;
        }
        let mut has_romanizable_letter = false;
        for c in name.chars() {
            match script(c) {
                Script::Romanizable => has_romanizable_letter = true,
                Script::Unsupported => return None,
                Script::Latin | Script::Other => {}
            }
        }
        if !has_romanizable_letter {
            return None;
        }

        let mut words = words(name);
        let trailing_type =
            if self.translates_russian_street_types { extract_russian_street_type(&mut words) } else { None };

        let mut result = String::with_capacity(name.len());
        for word in &words {
            if word.is_romanizable {
                result.push_str(&transliterate(&word.text));
            } else {
                result.push_str(&word.text);
            }
        }
        if let Some(trailing_type) = trailing_type {
            result = format!("{} {}", crate::schema::default::trim_whitespaces(&result), trailing_type);
        }
        Some(result)
    }
}

/// A combining mark, which belongs to the character before it the way a
/// grapheme cluster holds it.
fn is_combining_mark(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F | 0x0483..=0x0489 | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

/// The name split into the runs of romanizable letters and the text between
/// them. A run is one word of the source, so the transliteration sees word
/// starts where the name has them (BGN writes a leading "е" as "ye").
fn words(name: &str) -> Vec<Word> {
    let mut words: Vec<Word> = Vec::new();
    for c in name.chars() {
        let is_romanizable = match words.last() {
            // A mark rides with the character it is on.
            Some(last) if is_combining_mark(c) => last.is_romanizable,
            _ => script(c) == Script::Romanizable,
        };
        match words.last_mut() {
            Some(last) if last.is_romanizable == is_romanizable => last.text.push(c),
            _ => words.push(Word { text: c.to_string(), is_romanizable }),
        }
    }
    words
}

/// Removes a lowercase Russian street type standing as the first or the
/// last word of a name with at least one other word, and returns its
/// English word. `None`, and the words untouched, when the name has none.
fn extract_russian_street_type(words: &mut Vec<Word>) -> Option<&'static str> {
    let letter_runs: Vec<usize> = (0..words.len()).filter(|&index| words[index].is_romanizable).collect();
    if letter_runs.len() < 2 {
        return None;
    }
    for index in [letter_runs[letter_runs.len() - 1], letter_runs[0]] {
        let Some(&(_, english)) = RUSSIAN_STREET_TYPES.iter().find(|(russian, _)| *russian == words[index].text) else {
            continue;
        };
        words.remove(index);
        // The space that separated the type from the rest goes with it.
        let is_space = |word: &Word| !word.is_romanizable && word.text.chars().all(char::is_whitespace);
        if index < words.len() && is_space(&words[index]) {
            words.remove(index);
        } else if index > 0 && is_space(&words[index - 1]) {
            words.remove(index - 1);
        }
        return Some(english);
    }
    None
}

/// One word in ASCII. The soft and hard signs come out of the ASCII fold as
/// `'` and `"` and are dropped ("Bolshoy", not "Bol'shoy"). A Georgian word
/// starts with a capital: the script writes names without one (Unicode
/// counts its everyday letters as lowercase), Latin does not.
fn transliterate(word: &str) -> String {
    let letters: Vec<char> = word.chars().collect();
    let mut latin = String::with_capacity(word.len() * 2);
    let contexts = bgn_contexts(&letters);
    let mut index = 0;
    while index < letters.len() {
        index += write_letter(&letters, &contexts, index, &mut latin);
    }
    latin.retain(|c| c != '\'' && c != '"');
    if letters.first().is_some_and(|&c| is_georgian(c)) {
        let mut chars = latin.chars();
        if let Some(first) = chars.next() {
            let rest: String = chars.collect();
            latin = first.to_uppercase().collect::<String>() + &rest;
        }
    }
    latin
}

fn is_georgian(c: char) -> bool {
    matches!(c as u32, 0x10A0..=0x10FF | 0x1C90..=0x1CBF | 0x2D00..=0x2D2F)
}

/// The letter's spelling alone (or before a capital) and before a lowercase
/// letter, `None` for a letter the transform leaves as it is.
fn table(c: char) -> Option<(&'static str, &'static str)> {
    LETTERS
        .binary_search_by_key(&(c as u32), |&(scalar, _, _)| scalar)
        .ok()
        .map(|index| (LETTERS[index].1, LETTERS[index].2))
}

/// A Cyrillic letter other than the Russian vowels and `й`: a consonant, a
/// sign, or a letter of another Cyrillic alphabet (`ґ`, `ў`, `ї`).
fn is_other_cyrillic(c: char) -> bool {
    LabelScript::of(c) == Some(LabelScript::Cyrillic) && !"аеёиоуыэюяйАЕЁИОУЫЭЮЯЙ".contains(c)
}

/// What the BGN context rules decide for one letter of a word, read in
/// order: whether an `е` or `ё` is written `ye`, and whether a middle dot
/// stands before or after the letter.
#[derive(Clone, Copy, Default)]
struct Context {
    /// An `е` or `ё` written `ye`.
    ye: bool,
    /// A middle dot before the letter: an `э` after a consonant, a sign or
    /// an `е` written `ye` (`m·er`), a `ы` after a vowel (`ba·y`).
    dot_before: bool,
    /// A middle dot after the letter: `т·с`, `ш·ч`, and a `й` or `ы` before
    /// `а`, `у`, `ы` or `э` (`y·a`).
    dot_after: bool,
}

/// The BGN contexts of a word's letters. Each rule reads the letters before
/// it as the transform has already written them, so an `е` after an `е`
/// already written `ye` inside the word is a plain `e` (`yuyee`), and a
/// letter after a dotted `э` or `ы` is read as after a consonant.
fn bgn_contexts(letters: &[char]) -> Vec<Context> {
    let mut contexts = vec![Context::default(); letters.len()];
    for index in 0..letters.len() {
        let c = letters[index];
        let prev = index.checked_sub(1).map(|i| (letters[i], contexts[i]));
        let next = letters.get(index + 1).copied();
        // A previous letter that counts as a vowel for what follows it.
        let vowel_before = |upper_only: bool| match prev {
            None => false,
            Some((p, context)) => {
                let is_trigger = if upper_only {
                    "АЕЁИОУЫЭЮЯЙЪЬ".contains(p)
                } else {
                    "аеёиоуыэюяйъьАЕЁИОУЫЭЮЯЙЪЬ".contains(p)
                };
                let spent = match p {
                    'е' | 'ё' | 'Е' | 'Ё' => context.ye && index >= 2,
                    'э' | 'Э' | 'ы' | 'Ы' => context.dot_before,
                    _ => false,
                };
                is_trigger && !spent
            }
        };
        let context = &mut contexts[index];
        match c {
            'е' | 'ё' => context.ye = index == 0 || vowel_before(false),
            'Е' | 'Ё' => context.ye = index == 0 || vowel_before(true),
            'э' | 'Э' => {
                context.dot_before = prev.is_some_and(|(p, p_context)| {
                    is_other_cyrillic(p)
                        || (matches!(p, 'е' | 'ё' | 'Е' | 'Ё') && p_context.ye && index >= 2)
                        || (matches!(p, 'э' | 'Э' | 'ы' | 'Ы') && p_context.dot_before)
                })
            }
            'ы' | 'Ы' => {
                context.dot_before = prev.is_some_and(|(p, p_context)| {
                    "аеёиоуэюяАЕЁИОУЭЮЯ".contains(p)
                        && !(matches!(p, 'э' | 'Э') && p_context.dot_before)
                        && !(matches!(p, 'е' | 'ё' | 'Е' | 'Ё') && p_context.ye && index >= 2)
                });
            }
            _ => {}
        }
        context.dot_after = match c {
            'т' => next == Some('с'),
            'Т' => matches!(next, Some('с' | 'С')),
            'ш' => next == Some('ч'),
            'Ш' => matches!(next, Some('ч' | 'Ч')),
            'й' | 'Й' | 'Ы' => next.is_some_and(|next| "ауыэАУЫЭ".contains(next)),
            'ы' => !context.dot_before && next.is_some_and(|next| "ауыэ".contains(next)),
            _ => false,
        };
        if c == 'Ы' && context.dot_before {
            context.dot_after = false;
        }
    }
    contexts
}

/// Writes the letter at `index` and returns how many letters it took.
fn write_letter(letters: &[char], contexts: &[Context], index: usize, out: &mut String) -> usize {
    let c = letters[index];
    let prev = index.checked_sub(1).map(|i| letters[i]);
    let next = letters.get(index + 1).copied();
    let context = contexts[index];
    // A capital written in capitals or as a title: a title before a
    // lowercase letter the transform writes straight after it.
    let title = next.is_some_and(char::is_lowercase) && !contexts.get(index + 1).is_some_and(|next| next.dot_before);
    let spelling = |c: char| table(c).map(|(alone, before_lower)| if title { before_lower } else { alone });
    // One dot between two letters, whichever rule sets it.
    if context.dot_before && !(index > 0 && contexts[index - 1].dot_after) {
        out.push('·');
    }
    match c {
        // Cyrillic: the BGN context rules.
        'е' | 'ё' => out.push_str(if context.ye { "ye" } else { "e" }),
        'Е' | 'Ё' => {
            if context.ye {
                if next.is_some_and(char::is_uppercase) {
                    out.push_str("YE");
                } else if c == 'Ё' && index == 0 && !next.is_some_and(char::is_alphabetic) {
                    out.push('E');
                } else {
                    out.push_str("Ye");
                }
            } else {
                out.push('E');
            }
        }
        'э' => out.push('e'),
        'Э' => out.push('E'),
        'й' | 'ы' => out.push('y'),
        'Й' | 'Ы' => out.push('Y'),
        // Greek: the nasal gamma and the upsilon of a diphthong.
        'γ' | 'Γ' if next.is_some_and(|next| "γκξχΓΚΞΧ".contains(next)) => {
            out.push(if c == 'γ' { 'n' } else { 'N' });
        }
        _ if is_greek_upsilon(c) && prev.is_some_and(|prev| "αεοιυΑΕΟΙΥ".contains(prev)) => {
            out.push(if c.is_uppercase() { 'U' } else { 'u' });
        }
        // Armenian: the digraph `ու` of one case is one vowel.
        'ո' if next == Some('ւ') => {
            out.push('u');
            return 2;
        }
        'Ե' if next == Some('Ւ') => {
            out.push_str("EV");
            return 2;
        }
        'Ո' if next == Some('Ւ') => {
            out.push('U');
            return 2;
        }
        // The ASCII fold drops the marks a letter carries.
        _ if is_combining_mark(c) => {}
        _ => match spelling(c) {
            Some(spelling) => out.push_str(spelling),
            None => out.push(c),
        },
    }
    if context.dot_after {
        out.push('·');
    }
    1
}

/// An upsilon with or without its accents and diaeresis, without a
/// breathing.
fn is_greek_upsilon(c: char) -> bool {
    matches!(
        c,
        'υ' | 'ύ' | 'ϋ' | 'ΰ' | 'Υ' | 'Ύ' | 'Ϋ' | '\u{1F7A}' | '\u{1F7B}' | '\u{1FE0}' | '\u{1FE1}' | '\u{1FE2}'
            | '\u{1FE3}' | '\u{1FE6}' | '\u{1FE7}' | '\u{1F50}' | '\u{1F52}' | '\u{1F54}' | '\u{1F56}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_letter_table_is_sorted() {
        assert!(LETTERS.windows(2).all(|pair| pair[0].0 < pair[1].0));
    }

    fn english() -> LabelRomanizer {
        LabelRomanizer::new(&LabelLanguage::english())
    }

    /// The spellings Foundation's transform writes, with the quotes
    /// dropped, for names and letter pairs of each script.
    #[test]
    fn the_spellings_match_the_system_transform() {
        let cases = [
            ("Москва", "Moskva"),
            ("Тверская", "Tverskaya"),
            ("Тёплый Стан", "Teplyy Stan"),
            ("Щёлково", "Shchelkovo"),
            ("Чистые пруды", "Chistyye prudy"),
            ("Охотный Ряд", "Okhotnyy Ryad"),
            ("Детская", "Det·skaya"),
            ("Якутск", "Yakut·sk"),
            ("шчот", "sh·chot"),
            ("мэр", "m·er"),
            ("Майя", "Mayya"),
            ("Мыа", "My·a"),
            ("бэе", "b·ee"),
            ("Новые", "Novyye"),
            ("объезд", "obyezd"),
            ("ЦУМ", "TSUM"),
            ("ВДНХ", "VDNKH"),
            ("ЩуКА", "ShchuKA"),
            ("ЁЖ", "YEZH"),
            ("Ёж", "Yezh"),
            ("Ё", "E"),
            ("Е", "Ye"),
            ("АЕ", "AYe"),
            ("аЕ", "aE"),
            ("Київ", "Kiiv"),
            ("Ђурђевак", "Durdevak"),
            ("Қарағанды", "Kˌaragandy"),
            ("Θεσσαλονίκη", "Thessalonike"),
            ("Ευαγγελισμός", "Euangelismos"),
            ("γκρι", "nkri"),
            ("ηυ", "ey"),
            ("άυ", "ay"),
            ("ΑΘΗΝΑ", "ATHENA"),
            ("Ὅμηρος", "Homeros"),
            ("Ουρανός", "Ouranos"),
            ("Գյումրի", "Gyumri"),
            ("Ուղի", "Owgi"),
            ("ուխ", "ux"),
            ("Էջմիածին", "Ejmiacin"),
            ("ոչ", "oc"),
            ("მცხეთა", "Mtskheta"),
            ("წყალტუბო", "Tsqaltubo"),
        ];
        let romanizer = LabelRomanizer::new(&LabelLanguage::german());
        for (name, expected) in cases {
            assert_eq!(romanizer.romanize(name).as_deref(), Some(expected), "{name}");
        }
        assert_eq!(english().romanize("Красная площадь").as_deref(), Some("Krasnaya Square"));
    }
}
