// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

/// The alphabets the label text logic tells apart: Latin, and the ones the
/// romanizer handles (`LabelRomanizer`). A letter of any other script is
/// `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LabelScript {
    Latin,
    Cyrillic,
    Greek,
    Armenian,
    Georgian,
    Other,
}

impl LabelScript {
    /// The script of a letter, `None` for anything that is not one: digits,
    /// punctuation, spaces, combining marks.
    pub fn of(c: char) -> Option<LabelScript> {
        if !c.is_alphabetic() {
            return None;
        }
        Some(match c as u32 {
            0x0400..=0x052F | 0x1C80..=0x1C8F | 0x2DE0..=0x2DFF | 0xA640..=0xA69F => LabelScript::Cyrillic,
            0x0370..=0x03FF | 0x1F00..=0x1FFF => LabelScript::Greek,
            0x0530..=0x058F | 0xFB13..=0xFB17 => LabelScript::Armenian,
            0x10A0..=0x10FF | 0x1C90..=0x1CBF | 0x2D00..=0x2D2F => LabelScript::Georgian,
            0x0000..=0x024F | 0x1E00..=0x1EFF | 0x2C60..=0x2C7F | 0xA720..=0xA7FF => LabelScript::Latin,
            _ => LabelScript::Other,
        })
    }

    /// Whether `text` is written in this script: it has a letter, and every
    /// letter is of this script. A Latin part (a route number, a brand)
    /// makes it mixed, which is not.
    pub fn writes(self, text: &str) -> bool {
        let mut has_letter = false;
        for c in text.chars() {
            let Some(script) = Self::of(c) else { continue };
            if script != self {
                return false;
            }
            has_letter = true;
        }
        has_letter
    }
}
