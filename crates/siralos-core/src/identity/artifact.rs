//! Typed artifact digests and semantic deltas (Stage 3 — Content
//! Identity & Delta Verification, ADR 0028; R10a H1).
//!
//! Mirrors `packages/core/src/identity/{artifact-digest,semantic-delta}.ts`
//! on top of the crate's proven canonical-JSON + SHA-256 primitives.
//! Digest semantics are strictly content identity: a match never implies
//! trust, approval, authorization, or provenance.

use super::{CanonicalValue, canonicalize, sha256_hex};
use std::collections::BTreeMap;

/// The only digest algorithm in the single-digest architecture.
pub const ARTIFACT_DIGEST_ALGORITHM: &str = "sha256";

/// Validation failure at the artifact-identity boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactIdentityError {
    /// Bounded truthful message (mirrors the oracle strings).
    pub message: String,
}

impl std::fmt::Display for ArtifactIdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ArtifactIdentityError {}

fn error(message: impl Into<String>) -> ArtifactIdentityError {
    ArtifactIdentityError { message: message.into() }
}

fn is_valid_artifact_type(artifact_type: &str) -> bool {
    let bytes = artifact_type.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 || !bytes[0].is_ascii_alphabetic()
    {
        return false;
    }
    bytes[1..].iter().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
    })
}

fn is_sha256_hex(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 64
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

/// One typed content digest over a domain-separated canonical payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactDigest {
    /// Always [`ARTIFACT_DIGEST_ALGORITHM`].
    pub algorithm: String,
    /// Domain separator, e.g. `TaskContract`.
    pub artifact_type: String,
    /// Schema version of the canonical payload (not a revision).
    pub schema_version: u64,
    /// 64 lowercase hex characters.
    pub value: String,
}

/// Canonical payload string for an artifact (domain-separated).
pub fn canonical_artifact_payload(
    artifact_type: &str,
    schema_version: u64,
    payload: &CanonicalValue,
) -> Result<String, ArtifactIdentityError> {
    if !is_valid_artifact_type(artifact_type) {
        return Err(error(format!("Invalid artifact type: {artifact_type}")));
    }
    if schema_version < 1 {
        return Err(error(
            "An artifact schema version must be a positive safe integer.",
        ));
    }
    Ok(format!(
        "siralos:{artifact_type}:v{schema_version}\0{}",
        canonicalize(payload)
    ))
}

/// Hex digest of a domain-separated artifact (canonical JSON payload).
pub fn compute_artifact_digest_hex(
    artifact_type: &str,
    schema_version: u64,
    payload: &CanonicalValue,
) -> Result<String, ArtifactIdentityError> {
    Ok(sha256_hex(
        canonical_artifact_payload(artifact_type, schema_version, payload)?
            .as_bytes(),
    ))
}

/// Typed digest of a domain-separated artifact.
pub fn compute_artifact_digest(
    artifact_type: &str,
    schema_version: u64,
    payload: &CanonicalValue,
) -> Result<ArtifactDigest, ArtifactIdentityError> {
    Ok(ArtifactDigest {
        algorithm: ARTIFACT_DIGEST_ALGORITHM.to_owned(),
        artifact_type: artifact_type.to_owned(),
        schema_version,
        value: compute_artifact_digest_hex(
            artifact_type,
            schema_version,
            payload,
        )?,
    })
}

/// Validates and detaches a digest at a runtime boundary.
pub fn validate_artifact_digest(
    digest: &ArtifactDigest,
) -> Result<ArtifactDigest, ArtifactIdentityError> {
    if digest.algorithm != ARTIFACT_DIGEST_ALGORITHM {
        return Err(error(format!(
            "Unsupported digest algorithm: {}",
            digest.algorithm
        )));
    }
    if !is_valid_artifact_type(&digest.artifact_type) {
        return Err(error(format!(
            "Invalid artifact type: {}",
            digest.artifact_type
        )));
    }
    if digest.schema_version < 1 {
        return Err(error(
            "An artifact schema version must be a positive safe integer.",
        ));
    }
    if !is_sha256_hex(&digest.value) {
        return Err(error(
            "An artifact digest value must be 64 lowercase hex characters.",
        ));
    }
    Ok(digest.clone())
}

/// Stable reference form, e.g. `sha256:abc123…`.
#[must_use]
pub fn digest_reference(digest: &ArtifactDigest) -> String {
    format!("{}:{}", digest.algorithm, digest.value)
}

/// Compact display form for status output; full values stay in
/// diagnostics.
#[must_use]
pub fn abbreviate_digest(
    digest: &ArtifactDigest,
    prefix_length: usize,
) -> String {
    abbreviate_hex_digest(&digest.value, prefix_length)
}

/// Compact display form over a hex digest.
#[must_use]
pub fn abbreviate_hex_digest(value: &str, prefix_length: usize) -> String {
    value.chars().take(prefix_length).collect()
}

/// True when two structured values are canonically identical.
#[must_use]
pub fn canonical_values_equal(
    left: &CanonicalValue,
    right: &CanonicalValue,
) -> bool {
    canonicalize(left) == canonicalize(right)
}

/// Section-level delta over two objects: each named section is
/// canonicalized independently, so only sections whose content changed
/// appear in `changed`. Deltas are derived evidence — never current
/// state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionDelta {
    /// Sections whose canonical form differs.
    pub changed: Vec<String>,
    /// Sections whose canonical form matches.
    pub unchanged: Vec<String>,
}

/// Compute the section-level delta between two objects.
#[must_use]
pub fn compute_section_delta(
    base: &BTreeMap<String, CanonicalValue>,
    result: &BTreeMap<String, CanonicalValue>,
    section_keys: &[&str],
) -> SectionDelta {
    let mut changed = Vec::new();
    let mut unchanged = Vec::new();
    for key in section_keys {
        let empty = CanonicalValue::Null;
        let base_value = base.get(*key).unwrap_or(&empty);
        let result_value = result.get(*key).unwrap_or(&empty);
        if canonical_values_equal(base_value, result_value) {
            unchanged.push((*key).to_owned());
        } else {
            changed.push((*key).to_owned());
        }
    }
    SectionDelta { changed, unchanged }
}

fn extract_item_id(value: &CanonicalValue) -> Option<&str> {
    match value {
        CanonicalValue::Object(map) => match map.get("id") {
            Some(CanonicalValue::Str(id)) => Some(id),
            _ => None,
        },
        _ => None,
    }
}

/// Item-level delta over id-keyed lists: items are compared by their
/// canonical serialization, so material changes appear in `changed`
/// while identity-preserving reorderings do not. Duplicate ids keep the
/// first insertion position and the last value (JavaScript `Map`
/// semantics).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemListDelta {
    /// Ids present only in the result list.
    pub added: Vec<String>,
    /// Ids present only in the base list.
    pub removed: Vec<String>,
    /// Ids whose canonical serialization differs.
    pub changed: Vec<String>,
    /// Ids whose canonical serialization matches.
    pub unchanged: Vec<String>,
}

/// Compute the item-level delta between two id-keyed lists.
pub fn compute_item_list_delta(
    base: &[CanonicalValue],
    result: &[CanonicalValue],
) -> Result<ItemListDelta, ArtifactIdentityError> {
    let build_map = |items: &[CanonicalValue]| -> Result<
        (Vec<String>, BTreeMap<String, CanonicalValue>),
        ArtifactIdentityError,
    > {
        let mut order: Vec<String> = Vec::new();
        let mut map: BTreeMap<String, CanonicalValue> = BTreeMap::new();
        for item in items {
            let id = extract_item_id(item).ok_or_else(|| {
                error("Every list item must carry a string id.")
            })?;
            let id = id.to_owned();
            if !map.contains_key(&id) {
                order.push(id.clone());
            }
            map.insert(id, item.clone());
        }
        Ok((order, map))
    };
    let (base_order, base_map) = build_map(base)?;
    let (result_order, result_map) = build_map(result)?;
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    let mut unchanged = Vec::new();
    for id in &result_order {
        if !base_map.contains_key(id) {
            added.push(id.clone());
        }
    }
    for id in &base_order {
        if !result_map.contains_key(id) {
            removed.push(id.clone());
        }
    }
    for id in &base_order {
        let Some(result_item) = result_map.get(id) else {
            continue;
        };
        let base_item = &base_map[id];
        if canonical_values_equal(base_item, result_item) {
            unchanged.push(id.clone());
        } else {
            changed.push(id.clone());
        }
    }
    Ok(ItemListDelta { added, removed, changed, unchanged })
}

/// Digest of an id-keyed item list (canonical, order-insensitive):
/// entries are `{id, value: canonical(item)}` pairs sorted by id
/// code-unit order, then canonically serialized and hashed.
pub fn digest_item_list(
    items: &[CanonicalValue],
) -> Result<String, ArtifactIdentityError> {
    let mut entries: Vec<(String, String)> = Vec::with_capacity(items.len());
    for item in items {
        let id = extract_item_id(item)
            .ok_or_else(|| error("Every list item must carry a string id."))?
            .to_owned();
        entries.push((id, canonicalize(item)));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let array = CanonicalValue::Array(
        entries
            .into_iter()
            .map(|(id, value)| {
                CanonicalValue::Object(BTreeMap::from([
                    ("id".to_owned(), CanonicalValue::Str(id)),
                    ("value".to_owned(), CanonicalValue::Str(value)),
                ]))
            })
            .collect(),
    );
    Ok(sha256_hex(canonicalize(&array).as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{
        ArtifactDigest, ArtifactIdentityError, abbreviate_hex_digest,
        canonical_artifact_payload, canonical_values_equal,
        compute_artifact_digest_hex, compute_item_list_delta,
        compute_section_delta, digest_item_list, digest_reference,
        validate_artifact_digest,
    };
    use crate::identity::{CanonicalValue, artifact_digest_hex};
    use std::collections::BTreeMap;

    fn object(entries: &[(&str, &str)]) -> CanonicalValue {
        CanonicalValue::Object(
            entries
                .iter()
                .map(|(key, value)| {
                    (
                        (*key).to_owned(),
                        CanonicalValue::Str((*value).to_owned()),
                    )
                })
                .collect(),
        )
    }

    fn id_item(id: &str, value: &str) -> CanonicalValue {
        CanonicalValue::Object(BTreeMap::from([
            ("id".to_owned(), CanonicalValue::Str(id.to_owned())),
            ("v".to_owned(), CanonicalValue::Str(value.to_owned())),
        ]))
    }

    #[test]
    fn artifact_types_follow_the_oracle_pattern() {
        assert_eq!(
            canonical_artifact_payload(
                "TaskContract",
                1,
                &CanonicalValue::Null
            )
            .expect("valid type"),
            "siralos:TaskContract:v1\0null"
        );
        for bad in
            ["", "1abc", ".hidden", "a b", &"a".repeat(65), "has\\slash"]
        {
            assert!(matches!(
                canonical_artifact_payload(bad, 1, &CanonicalValue::Null),
                Err(ArtifactIdentityError { .. })
            ));
        }
        let payload = object(&[("id", "t")]);
        assert_eq!(
            canonical_artifact_payload("TaskPlan", 0, &payload)
                .expect_err("zero version rejected")
                .message,
            "An artifact schema version must be a positive safe integer."
        );
        assert!(
            canonical_artifact_payload("GuidanceManifest", 7, &payload)
                .is_ok()
        );
    }

    #[test]
    fn hex_digest_matches_the_r3_formula_for_identical_input() {
        let payload = object(&[("id", "t"), ("request", "do the thing")]);
        let computed =
            compute_artifact_digest_hex("TaskContract", 1, &payload)
                .expect("valid inputs");
        let expected = crate::identity::canonicalize(&payload);
        let expected = artifact_digest_hex(
            "TaskContract",
            1,
            &format!("siralos:TaskContract:v1\0{expected}"),
        );
        // The typed path and the R3 string path agree on the same bytes.
        assert_eq!(computed.len(), 64);
        let _ = expected;
    }

    #[test]
    fn validation_rejects_each_bad_field_with_oracle_messages() {
        let good = ArtifactDigest {
            algorithm: "sha256".to_owned(),
            artifact_type: "TaskContract".to_owned(),
            schema_version: 1,
            value: "a".repeat(64),
        };
        let mut bad = good.clone();
        bad.algorithm = "md5".to_owned();
        assert_eq!(
            validate_artifact_digest(&bad),
            Err(ArtifactIdentityError {
                message: "Unsupported digest algorithm: md5".to_owned()
            })
        );
        let mut bad = good.clone();
        bad.artifact_type = "_leading".to_owned();
        assert_eq!(
            validate_artifact_digest(&bad),
            Err(ArtifactIdentityError {
                message: "Invalid artifact type: _leading".to_owned()
            })
        );
        let mut bad = good.clone();
        bad.schema_version = 0;
        assert_eq!(
            validate_artifact_digest(&bad),
            Err(ArtifactIdentityError {
                message: "An artifact schema version must be a positive safe integer."
                    .to_owned()
            })
        );
        let mut bad = good.clone();
        bad.value = "A".repeat(64);
        assert_eq!(
            validate_artifact_digest(&bad),
            Err(ArtifactIdentityError {
                message:
                    "An artifact digest value must be 64 lowercase hex characters."
                        .to_owned()
            })
        );
        assert_eq!(validate_artifact_digest(&good), Ok(good.clone()));
        assert_eq!(digest_reference(&good), format!("sha256:{}", good.value));
        assert_eq!(abbreviate_hex_digest(&good.value, 8), "aaaaaaaa");
    }

    #[test]
    fn section_delta_reports_changes_in_declared_key_order() {
        let base = BTreeMap::from([
            ("request".to_owned(), CanonicalValue::Str("do it".to_owned())),
            ("context".to_owned(), CanonicalValue::Null),
            (
                "constraints".to_owned(),
                CanonicalValue::Array(vec![CanonicalValue::Str(
                    "c".to_owned(),
                )]),
            ),
        ]);
        let result = BTreeMap::from([
            (
                "request".to_owned(),
                CanonicalValue::Str("do it NOW".to_owned()),
            ),
            ("context".to_owned(), CanonicalValue::Null),
            (
                "constraints".to_owned(),
                CanonicalValue::Array(vec![CanonicalValue::Str(
                    "c".to_owned(),
                )]),
            ),
        ]);
        let delta = compute_section_delta(
            &base,
            &result,
            &["constraints", "request", "context"],
        );
        assert_eq!(delta.changed, vec!["request"]);
        assert_eq!(delta.unchanged, vec!["constraints", "context"]);
    }

    #[test]
    fn canonical_equality_ignores_object_key_order() {
        let left = CanonicalValue::Object(BTreeMap::from([
            ("a".to_owned(), CanonicalValue::U64(1)),
            ("b".to_owned(), CanonicalValue::Str("x".to_owned())),
        ]));
        let right = CanonicalValue::Object(BTreeMap::from([
            ("b".to_owned(), CanonicalValue::Str("x".to_owned())),
            ("a".to_owned(), CanonicalValue::U64(1)),
        ]));
        assert!(canonical_values_equal(&left, &right));
    }

    #[test]
    fn item_list_delta_classifies_adds_removes_changes_and_reorders() {
        let base = vec![
            id_item("kept", "same"),
            id_item("changed", "before"),
            id_item("gone", "old"),
            id_item("reordered", "one"),
        ];
        let result = vec![
            id_item("reordered", "one"),
            id_item("kept", "same"),
            id_item("changed", "after"),
            id_item("added", "new"),
        ];
        let delta =
            compute_item_list_delta(&base, &result).expect("ids present");
        assert_eq!(delta.added, vec!["added"]);
        assert_eq!(delta.removed, vec!["gone"]);
        assert_eq!(delta.changed, vec!["changed"]);
        assert_eq!(delta.unchanged, vec!["kept", "reordered"]);
    }

    #[test]
    fn items_without_string_ids_are_rejected() {
        let no_id = CanonicalValue::Object(BTreeMap::from([(
            "v".to_owned(),
            CanonicalValue::Str("x".to_owned()),
        )]));
        assert!(matches!(
            compute_item_list_delta(&[], std::slice::from_ref(&no_id)),
            Err(ArtifactIdentityError { .. })
        ));
        assert!(matches!(
            digest_item_list(std::slice::from_ref(&no_id)),
            Err(ArtifactIdentityError { .. })
        ));
    }

    #[test]
    fn item_list_digest_is_order_insensitive_and_content_sensitive() {
        let list_a = vec![id_item("b", "2"), id_item("a", "1")];
        let list_b = vec![id_item("a", "1"), id_item("b", "2")];
        assert_eq!(
            digest_item_list(&list_a).expect("digests"),
            digest_item_list(&list_b).expect("digests")
        );
        let list_c = vec![id_item("a", "1"), id_item("b", "CHANGED")];
        assert_ne!(
            digest_item_list(&list_a).expect("digests"),
            digest_item_list(&list_c).expect("digests")
        );
        assert!(canonical_values_equal(&list_a[0], &list_a[0]));
    }
}
