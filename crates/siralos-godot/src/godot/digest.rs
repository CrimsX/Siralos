//! Pure digest primitives (R8).
//!
//! Mirrors `packages/core/src/godot/digest.ts` — small pure SHA-256 over
//! canonical JSON so `siralos-core` stays dependency-free.
//! Re-exports the identity primitives that already guarantee byte parity.

pub use siralos_core::identity::{canonicalize, json_escape, sha256_hex};

/// Canonicalize a JSON value to deterministic bytes.
///
/// Equal semantic values always produce equal bytes: object keys sorted,
/// arrays preserve order, strings use `JSON.stringify` escaping.
/// All JSON number types (including negative integers and floats)
/// serialize as unquoted numbers, matching TypeScript's `JSON.stringify`.
#[must_use]
pub fn canonicalize_json(value: &serde_json::Value) -> String {
    siralos_core::identity::canonical_json_value(value)
}

/// Hex SHA-256 of the exact input bytes (pure FIPS 180-4).
#[must_use]
pub fn sha256_hex_str(text: &str) -> String {
    sha256_hex(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::{canonicalize_json, sha256_hex_str};

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256_hex_str(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn floats_serialize_as_unquoted_numbers() {
        let value = serde_json::json!({"temperature": 0.5});
        let canonical = canonicalize_json(&value);
        assert!(
            canonical.contains("0.5"),
            "float must be unquoted: {canonical}"
        );
    }
}
