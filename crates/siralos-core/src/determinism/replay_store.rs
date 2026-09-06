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

/// Bounded in-memory recordings store.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReplayStore {
    /// Ordered recordings (oldest first).
    pub recordings: Vec<ReplayRecording>,
}

/// Digest the bounded store's canonical contents (`ReplayStore` v1/v2).
///
/// The payload is the array in order of `{providerId, model, status,
/// bodySha256, bodyBytes, observedAtMs[, inputTokens, outputTokens,
/// cachedTokens], body}` per recording through the domain-separated artifact
/// primitive `siralos:ReplayStore:v{1|2}\0` + canonical JSON. When no recording
/// carries usage fields, v1 is used (byte-identical to the pre-102 digest);
/// otherwise v2 includes the usage bindings (null when absent within v2).
#[must_use]
pub fn compute_replay_store_digest(recordings: &[ReplayRecording]) -> String {
    let has_usage = recordings.iter().any(|r| {
        r.identity.input_tokens.is_some()
            || r.identity.output_tokens.is_some()
            || r.identity.cached_tokens.is_some()
    });
    let entries: Vec<Value> = recordings
        .iter()
        .map(|recording| {
            if has_usage {
                json!({
                    "providerId": recording.identity.provider_id,
                    "model": recording.identity.model,
                    "status": match recording.identity.status {
                        Some(value) => json!(value),
                        None => Value::Null,
                    },
                    "bodySha256": recording.identity.body_sha256,
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
                    "bodySha256": recording.identity.body_sha256,
                    "bodyBytes": recording.identity.body_bytes,
                    "observedAtMs": match recording.identity.observed_at_ms {
                        Some(value) => json!(value),
                        None => Value::Null,
                    },
                    "body": recording.body,
                })
            }
        })
        .collect();
    let payload = Value::Array(entries);
    let version = if has_usage { 2 } else { 1 };
    crate::determinism::helpers::digest_artifact_payload(
        "ReplayStore",
        version,
        &payload,
    )
    .expect("ReplayStore digest is infallible")
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
    let total: usize = recordings.iter().map(|r| r.body.len()).sum();
    if total > REPLAY_STORE_MAX_TOTAL_BODY_BYTES {
        return Err(ReplayStoreBoundsError::TotalBodyBytesExceeded { total });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        REPLAY_STORE_MAX_RECORDINGS, REPLAY_STORE_MAX_TOTAL_BODY_BYTES,
        compute_replay_store_digest, validate_replay_store_bounds,
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
                body_sha256: format!("sha{id}"),
                body_bytes: body.len() as u64,
                observed_at_ms: Some(id as u64),
                input_tokens: None,
                output_tokens: None,
                cached_tokens: None,
            },
            body: body.to_owned(),
        }
    }

    fn small_recording(id: usize) -> ReplayRecording {
        recording_with_body(id, "hello")
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
    fn digest_is_field_order_canonical() {
        let recordings = vec![ReplayRecording {
            identity: ProviderResponseIdentity {
                provider_id: "openai".to_owned(),
                model: "gpt-4o".to_owned(),
                status: Some(200),
                body_sha256: "abc".to_owned(),
                body_bytes: 5,
                observed_at_ms: Some(42),
                input_tokens: None,
                output_tokens: None,
                cached_tokens: None,
            },
            body: "hello".to_owned(),
        }];
        let digest = compute_replay_store_digest(&recordings);
        // Recompute via explicit payload with different key insertion order
        // must yield same digest because canonical JSON sorts keys.
        let payload = json!([{
            "body": "hello",
            "observedAtMs": 42,
            "bodyBytes": 5,
            "bodySha256": "abc",
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
