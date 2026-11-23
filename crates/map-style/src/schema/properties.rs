// Copyright (c) 2025-2026 Artem Bobkin.
// SPDX-License-Identifier: MIT

use mvt::Value;

/// A feature's attributes as the decoder hands them over: `(key, value)`
/// pairs in tag order, each key once, borrowing the tile's bytes. One is
/// built per feature per tile (`mvt::Layer::attributes`), so it stays a
/// plain vector looked up by a linear scan: a feature carries a few dozen
/// keys at most.
pub type Attributes<'a> = Vec<(&'a str, Value<'a>)>;

/// A feature's properties as the tile carries them, read through typed
/// accessors: a schema is loose about types (an id may be a number or its
/// digits, a height a number or "12 ft", a flag a bool or the word "yes").
///
/// Borrows the attribute list (`'p`), whose keys and strings borrow the
/// tile (`'a`), so a reading can hand back text that outlives the list.
#[derive(Debug, Clone, Copy)]
pub struct FeatureProperties<'p, 'a> {
    values: &'p [(&'a str, Value<'a>)],
}

impl<'p, 'a> FeatureProperties<'p, 'a> {
    pub fn new(values: &'p [(&'a str, Value<'a>)]) -> Self {
        FeatureProperties { values }
    }

    /// No properties at all.
    pub fn empty() -> Self {
        FeatureProperties { values: &[] }
    }

    /// Every `(key, value)` pair, in tag order.
    pub fn entries(&self) -> &'p [(&'a str, Value<'a>)] {
        self.values
    }

    /// The names of every property the feature carries, for a reading that
    /// looks for a family of keys (`name:xx`) rather than one it knows.
    pub fn keys(&self) -> impl Iterator<Item = &'a str> + 'p {
        self.values.iter().map(|(key, _)| *key)
    }

    /// The value under `key` as the tile typed it.
    pub fn get(&self, key: &str) -> Option<Value<'a>> {
        self.values.iter().find(|(name, _)| *name == key).map(|(_, value)| *value)
    }

    /// The value under `key` when it is a string, `None` for every other
    /// kind and for an absent key (the decoder's `stringValue`).
    pub fn str(&self, key: &str) -> Option<&'a str> {
        match self.get(key)? {
            Value::String(text) => Some(text),
            _ => None,
        }
    }

    /// The property as text: `None` when the key is absent, and the empty
    /// string when the key is present with a non-string value (the reading
    /// this accessor has always given, kept so a style written against it
    /// keeps working).
    pub fn string(&self, key: &str) -> Option<&'a str> {
        let value = self.get(key)?;
        Some(match value {
            Value::String(text) => text,
            _ => "",
        })
    }

    pub fn double(&self, key: &str) -> Option<f64> {
        match self.get(key)? {
            Value::Double(number) => Some(number),
            Value::Float(number) => Some(number as f64),
            Value::Int(number) => Some(number as f64),
            Value::UInt(number) => Some(number as f64),
            Value::String(text) => text.parse().ok(),
            Value::Bool(_) | Value::Null => None,
        }
    }

    pub fn integer(&self, key: &str) -> Option<i64> {
        integer_value(self.get(key)?)
    }

    /// The property as a measure in metres: a number as it is, a string by
    /// its leading number (`"12"`, `"12.5 m"`, `"3;4"` reads 3), feet
    /// converted. `None` when the key is absent or carries no number.
    pub fn metres(&self, key: &str) -> Option<f32> {
        metres_value(self.get(key)?)
    }

    /// The property as an identifier: a non-negative integer, or a string
    /// that spells one.
    pub fn unsigned_integer(&self, key: &str) -> Option<u64> {
        uint64_value(self.get(key)?)
    }

    /// The property spelled as text, whatever its type: a string as it is,
    /// a number as its digits, a flag as `1` or `0`, and empty when the key
    /// is absent. For keys that make up an identity.
    pub fn text(&self, key: &str) -> String {
        match self.get(key) {
            Some(Value::String(text)) => text.to_owned(),
            Some(Value::Int(number)) => number.to_string(),
            Some(Value::UInt(number)) => number.to_string(),
            Some(Value::Double(number)) => swift_double_description(number),
            Some(Value::Float(number)) => swift_float_description(number),
            Some(Value::Bool(flag)) => if flag { "1" } else { "0" }.to_owned(),
            Some(Value::Null) | None => String::new(),
        }
    }

    pub fn bool(&self, key: &str) -> Option<bool> {
        let value = self.get(key)?;
        if let Value::Bool(flag) = value {
            return Some(flag);
        }
        if let Some(integer) = integer_value(value) {
            return Some(integer != 0);
        }
        if let Value::String(text) = value {
            let normalized = text.to_lowercase();
            if normalized == "true" || normalized == "yes" || normalized == "1" {
                return Some(true);
            }
            if normalized == "false" || normalized == "no" || normalized == "0" {
                return Some(false);
            }
        }
        None
    }
}

/// The value as an integer: a number truncated toward zero, a string that
/// spells one.
pub(crate) fn integer_value(value: Value<'_>) -> Option<i64> {
    match value {
        Value::Int(number) => Some(number),
        Value::UInt(number) => i64::try_from(number).ok(),
        Value::Double(number) => float_to_int(number),
        Value::Float(number) => float_to_int(number as f64),
        Value::String(text) => text.parse().ok(),
        Value::Bool(_) | Value::Null => None,
    }
}

fn float_to_int(number: f64) -> Option<i64> {
    // Swift's `Int(Double)` traps outside the range. A tile value that
    // large is no number a reading can use.
    if number.is_finite() && number > i64::MIN as f64 && number < i64::MAX as f64 {
        Some(number as i64)
    } else {
        None
    }
}

/// The value as an identifier: a non-negative integer, or a string that
/// spells one.
pub(crate) fn uint64_value(value: Value<'_>) -> Option<u64> {
    match value {
        Value::UInt(number) => Some(number),
        Value::Int(number) => u64::try_from(number).ok(),
        Value::String(text) => text.parse().ok(),
        Value::Float(_) | Value::Double(_) | Value::Bool(_) | Value::Null => None,
    }
}

/// The value as a measure in metres: a number as it is, a string by its
/// leading number (`"12"`, `"12.5 m"`, `"3;4"` reads 3), feet converted.
pub(crate) fn metres_value(value: Value<'_>) -> Option<f32> {
    match value {
        Value::Float(number) => Some(number),
        Value::Double(number) => Some(number as f32),
        Value::UInt(number) => Some(number as f32),
        Value::Int(number) => Some(number as f32),
        Value::Bool(_) | Value::Null => None,
        Value::String(text) => {
            let raw = text.trim().to_lowercase();
            let token = raw.split([';', ',', ' ']).find(|token| !token.is_empty())?;
            let mut numeric = String::new();
            let mut has_digit = false;
            for ch in token.chars() {
                if ch.is_numeric() {
                    numeric.push(ch);
                    has_digit = true;
                    continue;
                }
                if (ch == '-' || ch == '+') && numeric.is_empty() {
                    numeric.push(ch);
                    continue;
                }
                if ch == '.' && !numeric.contains('.') {
                    numeric.push(ch);
                    continue;
                }
                break;
            }
            if !has_digit {
                return None;
            }
            let value: f32 = numeric.parse().ok()?;
            if raw.contains("ft") || raw.contains("feet") {
                return Some(value * 0.3048);
            }
            Some(value)
        }
    }
}

/// A double as Swift's `String(Double)` spells it: the shortest digits that
/// read back, with a `.0` on a whole number.
fn swift_double_description(number: f64) -> String {
    if number.is_finite() && number == number.trunc() && number.abs() < 1e16 {
        format!("{number:.1}")
    } else if number.is_nan() {
        "nan".to_owned()
    } else if number.is_infinite() {
        if number > 0.0 { "inf" } else { "-inf" }.to_owned()
    } else {
        format!("{number}")
    }
}

fn swift_float_description(number: f32) -> String {
    if number.is_finite() && number == number.trunc() && number.abs() < 1e16 {
        format!("{number:.1}")
    } else if number.is_nan() {
        "nan".to_owned()
    } else if number.is_infinite() {
        if number > 0.0 { "inf" } else { "-inf" }.to_owned()
    } else {
        format!("{number}")
    }
}
