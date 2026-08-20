//! Pure digest primitives (R8).
//!
//! Mirrors `packages/core/src/godot/digest.ts` — small pure SHA-256 over
//! canonical JSON so `siralos-core` stays dependency-free.
//! Re-exports the identity primitives that already guarantee byte parity.

pub use crate::identity::{
    CanonicalValue, canonicalize, json_escape, sha256_hex,
};

/// Canonicalize a JSON value to deterministic bytes.
///
/// Equal semantic values always produce equal bytes: object keys sorted,
/// arrays preserve order, strings use `JSON.stringify` escaping.
#[must_use]
pub fn canonicalize_json(value: &serde_json::Value) -> String {
    canonical_value_from_json(value).to_canonical()
}

fn canonical_value_from_json(value: &serde_json::Value) -> CanonicalValue {
    match value {
        serde_json::Value::Null => CanonicalValue::Null,
        serde_json::Value::Bool(b) => CanonicalValue::Bool(*b),
        serde_json::Value::Number(n) => {
            // Preserve integer vs float via string round-trip; fallback to string.
            if let Some(u) = n.as_u64() {
                CanonicalValue::U64(u)
            } else if let Some(i) = n.as_i64() {
                // Negative integers have no direct U64 encoding; store as string losslessly.
                CanonicalValue::Str(i.to_string())
            } else {
                CanonicalValue::Str(n.to_string())
            }
        }
        serde_json::Value::String(s) => CanonicalValue::Str(s.clone()),
        serde_json::Value::Array(arr) => CanonicalValue::Array(
            arr.iter().map(canonical_value_from_json).collect(),
        ),
        serde_json::Value::Object(map) => {
            let mut out = std::collections::BTreeMap::new();
            for (k, v) in map {
                out.insert(k.clone(), canonical_value_from_json(v));
            }
            CanonicalValue::Object(out)
        }
    }
}

/// Hex SHA-256 of the exact input bytes (pure FIPS 180-4).
#[must_use]
pub fn sha256_hex_str(text: &str) -> String {
    sha256_hex(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::sha256_hex_str;

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256_hex_str(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
