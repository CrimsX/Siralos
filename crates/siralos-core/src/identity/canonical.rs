//! Canonical JSON serialization matching the TypeScript reference's
//! canonicalizeJson (the Node-free reference digest module).
//!
//! Equal semantic values always serialize to equal bytes: object keys are
//! sorted, arrays preserve order, strings use the exact `JSON.stringify`
//! escaping (short escapes for \b \f \n \r \t, \uXXXX for other
//! control characters, literal UTF-8 otherwise). Key order never affects
//! the canonical form.

use std::collections::BTreeMap;

/// JSON string escaping exactly matching `JSON.stringify` for strings.
pub fn json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str(r"\b"),
            '\u{000c}' => out.push_str(r"\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if (character as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

/// Canonical JSON scalar/container value used by R3 identity payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalValue {
    /// JSON string.
    Str(String),
    /// Non-negative integer (counts, versions).
    U64(u64),
    /// JSON boolean.
    Bool(bool),
    /// JSON null.
    Null,
    /// JSON array (order authoritative).
    Array(Vec<CanonicalValue>),
    /// JSON object (keys sorted during serialization).
    Object(BTreeMap<String, CanonicalValue>),
}

impl CanonicalValue {
    /// Serialize this value to canonical JSON bytes.
    pub fn to_canonical(&self) -> String {
        let mut out = String::new();
        self.write_canonical(&mut out);
        out
    }

    fn write_canonical(&self, out: &mut String) {
        match self {
            CanonicalValue::Str(value) => out.push_str(&json_escape(value)),
            CanonicalValue::U64(value) => out.push_str(&value.to_string()),
            CanonicalValue::Bool(true) => out.push_str("true"),
            CanonicalValue::Bool(false) => out.push_str("false"),
            CanonicalValue::Null => out.push_str("null"),
            CanonicalValue::Array(entries) => {
                out.push('[');
                for (index, entry) in entries.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    entry.write_canonical(out);
                }
                out.push(']');
            }
            CanonicalValue::Object(entries) => {
                out.push('{');
                for (index, (key, value)) in entries.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    out.push_str(&json_escape(key));
                    out.push(':');
                    value.write_canonical(out);
                }
                out.push('}');
            }
        }
    }
}

/// Canonical JSON serialization of a value (sorted keys, deterministic
/// escapes). Equal semantic values always produce equal bytes.
pub fn canonicalize(value: &CanonicalValue) -> String {
    value.to_canonical()
}

#[cfg(test)]
mod tests {
    use super::{CanonicalValue, canonicalize, json_escape};
    use std::collections::BTreeMap;

    fn object(entries: &[(&str, CanonicalValue)]) -> CanonicalValue {
        CanonicalValue::Object(
            entries
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect(),
        )
    }

    #[test]
    fn canonical_object_keys_are_sorted_regardless_of_insertion_order() {
        let a = object(&[
            ("z", CanonicalValue::U64(1)),
            ("a", CanonicalValue::U64(2)),
            ("m", CanonicalValue::U64(3)),
        ]);
        let b = object(&[
            ("m", CanonicalValue::U64(3)),
            ("a", CanonicalValue::U64(2)),
            ("z", CanonicalValue::U64(1)),
        ]);
        assert_eq!(canonicalize(&a), "{\"a\":2,\"m\":3,\"z\":1}");
        assert_eq!(canonicalize(&a), canonicalize(&b));
    }

    #[test]
    fn canonical_nested_values_match_javascript_stringify_ordering() {
        let value = object(&[
            (
                "nested",
                CanonicalValue::Object(BTreeMap::from([
                    ("deep".to_owned(), CanonicalValue::U64(7)),
                    (
                        "alpha".to_owned(),
                        CanonicalValue::Str("beta".to_owned()),
                    ),
                ])),
            ),
            (
                "list",
                CanonicalValue::Array(vec![
                    CanonicalValue::U64(1),
                    CanonicalValue::Str("two".to_owned()),
                    CanonicalValue::Null,
                    CanonicalValue::Bool(true),
                ]),
            ),
        ]);
        assert_eq!(
            canonicalize(&value),
            "{\"list\":[1,\"two\",null,true],\"nested\":{\"alpha\":\"beta\",\"deep\":7}}"
        );
    }

    #[test]
    fn json_escape_round_trips_control_characters() {
        assert_eq!(json_escape("a\nb"), "\"a\\nb\"");
        assert_eq!(json_escape("\u{001f}"), "\"\\u001f\"");
        assert_eq!(json_escape("\u{007f}"), "\"\u{007f}\"");
    }
}
