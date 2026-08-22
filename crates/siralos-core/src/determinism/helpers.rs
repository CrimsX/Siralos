//! Determinism helpers shared across submodules.
//!
//! Uses `serde_json::Value` + the godot digest re-exports for canonical
//! JSON serialization so that f64 numbers serialize as unquoted JSON
//! numbers (matching TypeScript's `JSON.stringify`), not quoted strings.

use crate::godot::digest::{canonicalize_json, sha256_hex_str};
use serde_json::Value;

/// Digest any JSON payload through the domain-separated artifact
/// primitive (`siralos:<type>:v<version>\0` + canonical JSON).
pub fn digest_artifact_payload(
    artifact_type: &str,
    schema_version: u64,
    payload: &Value,
) -> Result<String, String> {
    if schema_version < 1 {
        return Err(
            "An artifact schema version must be a positive safe integer."
                .to_owned(),
        );
    }
    let framed = format!(
        "siralos:{artifact_type}:v{schema_version}\u{0}{}",
        canonicalize_json(payload)
    );
    Ok(sha256_hex_str(&framed))
}
