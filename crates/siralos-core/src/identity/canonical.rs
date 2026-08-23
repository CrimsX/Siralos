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

/// Serialize a `serde_json::Value` to canonical JSON bytes directly,
/// without going through [`CanonicalValue`]. This handles all JSON
/// number types exactly like the TypeScript oracle's
/// `JSON.parse -> Number -> JSON.stringify` round trip: every number
/// passes through an IEEE-754 double and is formatted by the
/// ECMAScript `Number::toString` algorithm (shortest round-trip
/// digits, exponential form outside [1e-6, 1e21), integral doubles
/// without a fractional suffix).
pub fn canonical_json_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_owned(),
        serde_json::Value::Bool(b) => {
            if *b {
                "true".to_owned()
            } else {
                "false".to_owned()
            }
        }
        serde_json::Value::Number(n) => {
            js_number_to_string(n.as_f64().unwrap_or(f64::NAN))
        }
        serde_json::Value::String(s) => json_escape(s),
        serde_json::Value::Array(items) => {
            let parts: Vec<String> =
                items.iter().map(canonical_json_value).collect();
            format!("[{}]", parts.join(","))
        }
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        json_escape(key),
                        canonical_json_value(&map[key.as_str()])
                    )
                })
                .collect();
            format!("{{{}}}", parts.join(","))
        }
    }
}

/// ECMAScript `Number::toString(value)` (ECMA-262 §6.1.6.1.20) for a
/// finite double: shortest decimal digits that round-trip, rendered per
/// the spec's three ranges (`k <= n <= 21`, `0 < n <= 21`,
/// `-6 < n <= 0`, else exponential with explicit sign). NaN/-0/±0 are
/// handled per spec ("NaN" cannot appear in JSON input, "-0" prints as
/// "0").
fn js_number_to_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value == 0.0 {
        return "0".to_owned();
    }
    let negative = value < 0.0;
    let magnitude = if negative { -value } else { value };

    // Rust's LowerExp yields the shortest round-trip digits as
    // `d[.ddd]e<exp>` with no trailing zeros and no "+" sign.
    let scientific = format!("{magnitude:e}");
    let (mantissa, exponent_text) =
        scientific.split_once('e').unwrap_or((scientific.as_str(), "0"));
    let exponent: i32 = exponent_text.parse().unwrap_or(0);
    let mut digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    while digits.ends_with('0') {
        digits.pop();
    }
    if digits.is_empty() {
        digits.push('0');
    }

    let digit_count = digits.len() as i32;
    // Position of the decimal point relative to the first digit.
    let point = exponent + 1;

    let mut out = String::new();
    if digit_count <= point && point <= 21 {
        out.push_str(&digits);
        for _ in 0..(point - digit_count) {
            out.push('0');
        }
    } else if point > 0 && point <= 21 {
        out.push_str(&digits[..point as usize]);
        out.push('.');
        out.push_str(&digits[point as usize..]);
    } else if point > -6 && point <= 0 {
        out.push_str("0.");
        for _ in 0..(-point) {
            out.push('0');
        }
        out.push_str(&digits);
    } else {
        out.push_str(&digits[..1]);
        if digit_count > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        let order = point - 1;
        if order >= 0 {
            out.push('+');
        } else {
            out.push('-');
        }
        out.push_str(&(order.abs()).to_string());
    }

    if negative { format!("-{out}") } else { out }
}

#[cfg(test)]
mod tests {
    use super::{
        CanonicalValue, canonical_json_value, canonicalize,
        js_number_to_string, json_escape,
    };
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

    #[test]
    fn numbers_format_exactly_like_ecmascript_string_to_number_to_string() {
        // Every expectation below was generated by Node
        // `String(number)` (ECMA-262 Number::toString), including the
        // two regression cases from the R11 review: sub-normal-range
        // exponentials and u64 overflow that previously saturated via
        // `as i64`.
        let cases: &[(f64, &str)] = &[
            (0.000_001, "0.000001"),
            (0.000_000_1, "1e-7"),
            (1e21, "1e+21"),
            (18_446_744_073_709_551_615.0, "18446744073709552000"),
            (-0.0, "0"),
            (0.1, "0.1"),
            (123.456, "123.456"),
            (100.0, "100"),
            (1.5, "1.5"),
            (5e-324, "5e-324"),
            (1.797_693_134_862_315_7e308, "1.7976931348623157e+308"),
            (2.5, "2.5"),
            (-12.75, "-12.75"),
            (9_007_199_254_740_993.0, "9007199254740992"),
        ];
        for (value, expected) in cases {
            assert_eq!(
                &js_number_to_string(*value),
                expected,
                "input {value}"
            );
        }
    }

    #[test]
    fn canonical_json_numbers_match_the_oracle_round_trip() {
        let value = serde_json::from_str::<serde_json::Value>(
            "{\"tiny\":0.000001,\"huge\":18446744073709551615}",
        )
        .expect("valid json");
        assert_eq!(
            canonical_json_value(&value),
            "{\"huge\":18446744073709552000,\"tiny\":0.000001}"
        );
    }
}
