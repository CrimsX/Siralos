//! Bounded recordings store (Stage 8, decision 78 B1).
//!
//! Owns the bounded in-memory [`ReplayStore`] and its digest/bounds
//! contracts. The store is a bounded cache, not an archive: at most
//! 64 recordings and 2 MiB total body bytes; persistence is via the
//! adapters `replay_store` on the canonical JSON with digest-bound
//! integrity.

use serde_json::{Value, json};

use super::provider_replay::ReplayRecording;

/// Maximum number of recordings the bounded store may hold.
pub const REPLAY_STORE_MAX_RECORDINGS: usize = 64;

/// Maximum total body bytes across all recordings (2 MiB).
pub const REPLAY_STORE_MAX_TOTAL_BODY_BYTES: usize = 2 * 1024 * 1024;

/// Canonicalize a hexadecimal identity field for store digest/serialization.
///
/// SHA-256 digests are case-insensitive on the wire, but the store has one
/// canonical representation. Normalizing at the digest boundary keeps a
/// caller-supplied uppercase value from producing a document whose own
/// canonical digest cannot be reproduced by the loader.
fn canonical_digest(value: &str) -> String {
    value.to_ascii_lowercase()
}

/// Bounded in-memory recordings store.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReplayStore {
    /// Ordered recordings (oldest first).
    pub recordings: Vec<ReplayRecording>,
}

impl ReplayStore {
    /// Build a checked store without changing the legacy public `recordings`
    /// field or struct-literal construction path.
    pub fn try_new(
        recordings: Vec<ReplayRecording>,
    ) -> Result<Self, ReplayStoreValidationError> {
        let store = Self { recordings };
        store.validate()?;
        Ok(store)
    }

    /// Validate this store's capacity and every retained body identity.
    pub fn validate(&self) -> Result<(), ReplayStoreValidationError> {
        validate_replay_store(&self.recordings)
    }
}

/// Digest the bounded store's canonical contents (`ReplayStore` v1-v3).
///
/// The payload is the array in order of `{providerId, model, status,
/// bodySha256, bodyBytes, observedAtMs[, inputTokens, outputTokens,
/// cachedTokens][, requestSha256], body}` per recording through the
/// domain-separated artifact primitive `siralos:ReplayStore:v{1|2|3}\0` +
/// canonical JSON. Hash fields are lowercase-canonical before they enter the
/// payload. When no recording carries usage or request binding, v1 is used;
/// usage selects v2 and request binding selects v3.
#[must_use]
pub fn compute_replay_store_digest(recordings: &[ReplayRecording]) -> String {
    let has_usage = recordings.iter().any(|r| {
        r.identity.input_tokens.is_some()
            || r.identity.output_tokens.is_some()
            || r.identity.cached_tokens.is_some()
    });
    let has_request_binding =
        recordings.iter().any(|r| r.request_sha256.is_some());
    let entries: Vec<Value> = recordings
        .iter()
        .map(|recording| {
            let mut entry = if has_usage {
                json!({
                    "providerId": recording.identity.provider_id,
                    "model": recording.identity.model,
                    "status": match recording.identity.status {
                        Some(value) => json!(value),
                        None => Value::Null,
                    },
                    "bodySha256": canonical_digest(&recording.identity.body_sha256),
                    "bodyBytes": recording.identity.body_bytes,
                    "observedAtMs": match recording.identity.observed_at_ms {
                        Some(value) => json!(value),
                        None => Value::Null,
                    },
                    "inputTokens": match recording.identity.input_tokens {
                        Some(value) => json!(value),
                        None => Value::Null,
                    },
                    "outputTokens": match recording.identity.output_tokens {
                        Some(value) => json!(value),
                        None => Value::Null,
                    },
                    "cachedTokens": match recording.identity.cached_tokens {
                        Some(value) => json!(value),
                        None => Value::Null,
                    },
                    "body": recording.body,
                })
            } else {
                json!({
                    "providerId": recording.identity.provider_id,
                    "model": recording.identity.model,
                    "status": match recording.identity.status {
                        Some(value) => json!(value),
                        None => Value::Null,
                    },
                    "bodySha256": canonical_digest(&recording.identity.body_sha256),
                    "bodyBytes": recording.identity.body_bytes,
                    "observedAtMs": match recording.identity.observed_at_ms {
                        Some(value) => json!(value),
                        None => Value::Null,
                    },
                    "body": recording.body,
                })
            };
            if has_request_binding {
                entry
                    .as_object_mut()
                    .expect("replay entry is an object")
                    .insert(
                        "requestSha256".to_owned(),
                        match &recording.request_sha256 {
                            Some(value) => Value::String(canonical_digest(value)),
                            None => Value::Null,
                        },
                    );
            }
            entry
        })
        .collect();
    let payload = Value::Array(entries);
    let version = if has_request_binding {
        3
    } else if has_usage {
        2
    } else {
        1
    };
    crate::determinism::helpers::digest_artifact_payload(
        "ReplayStore",
        version,
        &payload,
    )
    .expect("ReplayStore digest is infallible")
}

/// Compute a store digest only after checking capacity and body identity.
///
/// The legacy [`compute_replay_store_digest`] function remains permissive for
/// callers that need to digest an in-progress or historical record; new
/// persistence paths should use this checked variant.
pub fn try_compute_replay_store_digest(
    recordings: &[ReplayRecording],
) -> Result<String, ReplayStoreValidationError> {
    validate_replay_store(recordings)?;
    Ok(compute_replay_store_digest(recordings))
}

/// Typed bounds violation for the bounded store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayStoreBoundsError {
    /// Too many recordings for the bounded cache.
    TooManyRecordings {
        /// Observed count.
        count: usize,
    },
    /// Total body bytes exceed the bounded cache cap.
    TotalBodyBytesExceeded {
        /// Observed total bytes.
        total: usize,
    },
}

impl std::fmt::Display for ReplayStoreBoundsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyRecordings { count } => write!(
                f,
                "replay store exceeds the {REPLAY_STORE_MAX_RECORDINGS}-recording bound: {count}"
            ),
            Self::TotalBodyBytesExceeded { total } => write!(
                f,
                "replay store exceeds the {REPLAY_STORE_MAX_TOTAL_BODY_BYTES}-byte total-body bound: {total}"
            ),
        }
    }
}

impl std::error::Error for ReplayStoreBoundsError {}

/// A bounded store failed capacity or detached-recording identity validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayStoreValidationError {
    /// The legacy bounded-cache invariant failed.
    Bounds(ReplayStoreBoundsError),
    /// A recording is not internally consistent.
    InvalidRecording {
        /// Index of the offending recording.
        index: usize,
        /// Identity/body validation failure.
        error: super::provider_replay::ReplayRecordingValidationError,
    },
}

impl std::fmt::Display for ReplayStoreValidationError {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::Bounds(error) => {
                write!(formatter, "replay store bounds: {error}")
            }
            Self::InvalidRecording { index, error } => write!(
                formatter,
                "replay recording {index} is invalid: {error}"
            ),
        }
    }
}

impl std::error::Error for ReplayStoreValidationError {}

/// Validate the bounded store invariants on the given recordings.
///
/// Checks at most 64 recordings and at most 2 MiB total body bytes.
/// Body bytes are measured as the UTF-8 length of each `body`.
pub fn validate_replay_store_bounds(
    recordings: &[ReplayRecording],
) -> Result<(), ReplayStoreBoundsError> {
    if recordings.len() > REPLAY_STORE_MAX_RECORDINGS {
        return Err(ReplayStoreBoundsError::TooManyRecordings {
            count: recordings.len(),
        });
    }
    let mut total = 0usize;
    for recording in recordings {
        total = total.saturating_add(recording.body.len());
        if total > REPLAY_STORE_MAX_TOTAL_BODY_BYTES {
            return Err(ReplayStoreBoundsError::TotalBodyBytesExceeded {
                total,
            });
        }
    }
    Ok(())
}

/// Validate capacity and the complete identity/body contract of every
/// recording. This performs no collection allocation and hashes each body
/// directly from its existing buffer.
pub fn validate_replay_store(
    recordings: &[ReplayRecording],
) -> Result<(), ReplayStoreValidationError> {
    validate_replay_store_bounds(recordings)
        .map_err(ReplayStoreValidationError::Bounds)?;
    for (index, recording) in recordings.iter().enumerate() {
        recording.validate().map_err(|error| {
            ReplayStoreValidationError::InvalidRecording { index, error }
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        REPLAY_STORE_MAX_RECORDINGS, REPLAY_STORE_MAX_TOTAL_BODY_BYTES,
        ReplayStore, compute_replay_store_digest,
        try_compute_replay_store_digest, validate_replay_store,
        validate_replay_store_bounds,
    };
    use crate::determinism::provider_replay::{
        ProviderResponseIdentity, ReplayRecording,
    };
    use serde_json::json;

    fn recording_with_body(id: usize, body: &str) -> ReplayRecording {
        ReplayRecording {
            identity: ProviderResponseIdentity {
                provider_id: format!("provider-{id}"),
                model: format!("model-{id}"),
                status: Some(200),
                body_sha256: crate::identity::sha256_hex(body.as_bytes()),
                body_bytes: body.len() as u64,
                observed_at_ms: Some(id as u64),
                input_tokens: None,
                output_tokens: None,
                cached_tokens: None,
            },
            body: body.to_owned(),
            request_sha256: None,
        }
    }

    fn small_recording(id: usize) -> ReplayRecording {
        recording_with_body(id, "hello")
    }

    #[test]
    fn strict_store_validation_rejects_body_identity_mismatch() {
        let mut recording = small_recording(1);
        recording.identity.body_bytes += 1;
        let error = validate_replay_store(&[recording])
            .expect_err("mismatched recording must fail closed");
        assert!(matches!(
            error,
            super::ReplayStoreValidationError::InvalidRecording {
                index: 0,
                ..
            }
        ));
    }

    #[test]
    fn checked_store_constructor_validates_without_changing_legacy_fields() {
        let recording = small_recording(1);
        let store = ReplayStore::try_new(vec![recording.clone()])
            .expect("valid recording");
        assert_eq!(store.recordings, vec![recording]);
        assert!(store.validate().is_ok());
    }

    #[test]
    fn legacy_bounds_validator_remains_capacity_only() {
        let mut recording = small_recording(1);
        recording.identity.body_bytes += 1;
        assert!(validate_replay_store_bounds(&[recording.clone()]).is_ok());
        assert!(validate_replay_store(&[recording]).is_err());
    }

    #[test]
    fn checked_digest_refuses_inconsistent_recording() {
        let mut recording = small_recording(1);
        recording.identity.body_sha256 = "0".repeat(64);
        assert!(try_compute_replay_store_digest(&[recording]).is_err());
    }

    #[test]
    fn digest_is_stable_and_64_hex() {
        let recordings = vec![small_recording(1), small_recording(2)];
        let first = compute_replay_store_digest(&recordings);
        let second = compute_replay_store_digest(&recordings);
        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn digest_canonicalizes_uppercase_hash_fields() {
        let mut recording = small_recording(1);
        recording.identity.body_sha256 = "ABCDEF0123456789".repeat(4);
        recording.request_sha256 = Some("0123456789ABCDEF".repeat(4));
        let mut lower = recording.clone();
        lower.identity.body_sha256 =
            lower.identity.body_sha256.to_ascii_lowercase();
        lower.request_sha256 =
            lower.request_sha256.as_deref().map(str::to_ascii_lowercase);
        assert_eq!(
            compute_replay_store_digest(&[recording]),
            compute_replay_store_digest(&[lower])
        );
    }

    #[test]
    fn digest_binds_request_identity_when_present() {
        let mut first = small_recording(1);
        first.request_sha256 = Some("a".repeat(64));
        let mut second = first.clone();
        second.request_sha256 = Some("b".repeat(64));
        assert_ne!(
            compute_replay_store_digest(&[first]),
            compute_replay_store_digest(&[second])
        );
    }

    #[test]
    fn digest_is_field_order_canonical() {
        let recordings = vec![ReplayRecording {
            identity: ProviderResponseIdentity {
                provider_id: "openai".to_owned(),
                model: "gpt-4o".to_owned(),
                status: Some(200),
                body_sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                    .to_owned(),
                body_bytes: 5,
                observed_at_ms: Some(42),
                input_tokens: None,
                output_tokens: None,
                cached_tokens: None,
            },
            body: "hello".to_owned(),
            request_sha256: None,
        }];
        let digest = compute_replay_store_digest(&recordings);
        // Recompute via explicit payload with different key insertion order
        // must yield same digest because canonical JSON sorts keys.
        let payload = json!([{
            "body": "hello",
            "observedAtMs": 42,
            "bodyBytes": 5,
            "bodySha256": "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
            "status": 200,
            "model": "gpt-4o",
            "providerId": "openai",
        }]);
        let canonical = crate::determinism::helpers::digest_artifact_payload(
            "ReplayStore",
            1,
            &payload,
        )
        .expect("digest");
        assert_eq!(digest, canonical);
    }

    #[test]
    fn digest_differs_when_order_changes() {
        let a = vec![small_recording(1), small_recording(2)];
        let b = vec![small_recording(2), small_recording(1)];
        assert_ne!(
            compute_replay_store_digest(&a),
            compute_replay_store_digest(&b)
        );
    }

    #[test]
    fn bounds_accepts_64_recordings() {
        let recordings: Vec<_> =
            (0..REPLAY_STORE_MAX_RECORDINGS).map(small_recording).collect();
        assert!(validate_replay_store_bounds(&recordings).is_ok());
    }

    #[test]
    fn bounds_rejects_65_recordings() {
        let recordings: Vec<_> = (0..REPLAY_STORE_MAX_RECORDINGS + 1)
            .map(small_recording)
            .collect();
        let err =
            validate_replay_store_bounds(&recordings).expect_err("too many");
        assert_eq!(
            err,
            super::ReplayStoreBoundsError::TooManyRecordings { count: 65 }
        );
        assert!(format!("{err}").contains("65"));
    }

    #[test]
    fn bounds_accepts_total_bytes_at_cap() {
        // One recording whose body is exactly the cap.
        let body = "a".repeat(REPLAY_STORE_MAX_TOTAL_BODY_BYTES);
        let recording = recording_with_body(0, &body);
        assert!(validate_replay_store_bounds(&[recording]).is_ok());
    }

    #[test]
    fn bounds_rejects_total_bytes_over_cap() {
        let body = "a".repeat(REPLAY_STORE_MAX_TOTAL_BODY_BYTES + 1);
        let recording = recording_with_body(0, &body);
        let err = validate_replay_store_bounds(&[recording])
            .expect_err("over bytes");
        match err {
            super::ReplayStoreBoundsError::TotalBodyBytesExceeded {
                total,
            } => {
                assert_eq!(total, REPLAY_STORE_MAX_TOTAL_BODY_BYTES + 1);
            }
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn bounds_total_is_sum_of_bodies() {
        let a = recording_with_body(0, &"a".repeat(1024));
        let b = recording_with_body(1, &"b".repeat(1024));
        // 2048 < cap, ok
        assert!(validate_replay_store_bounds(&[a.clone(), b.clone()]).is_ok());
        // Over cap via many small bodies that individually are small but
        // together exceed cap.
        let many = vec!["x".repeat(1024 * 1024); 3]
            .into_iter()
            .enumerate()
            .map(|(i, body)| recording_with_body(i, &body))
            .collect::<Vec<_>>();
        // 3 MiB > 2 MiB
        assert!(validate_replay_store_bounds(&many).is_err());
    }
}
