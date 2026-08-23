//! Determinism helpers shared across submodules.
//!
//! Uses `serde_json::Value` + the identity canonicalization primitives
//! for canonical JSON serialization so that f64 numbers serialize as
//! unquoted JSON numbers (matching TypeScript's `JSON.stringify`), not
//! quoted strings.

use crate::identity::canonical_json_value;
use crate::identity::sha256_hex;
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
        canonical_json_value(payload)
    );
    Ok(sha256_hex(framed.as_bytes()))
}
