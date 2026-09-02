//! Provider response replay recording (Stage 8, decision 68 §3).
//!
//! Records provider HTTP responses via the determinism ports for replay. A
//! non-recorded live call is a typed `unavailable` for replay. The input side
//! is [`super::reproducibility::ProviderInputIdentity`]; this module adds the
//! response side without changing the closed `ProviderEvent`/`ModelEvent` set.

use serde_json::{Value, json};

/// Identity of one provider HTTP response for replay.
///
/// The record never contains a credential or the raw body text, only the
/// `sha256` of the sanitized bounded text and its byte length.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderResponseIdentity {
    /// Provider identifier (e.g. `"openai"`, `"anthropic"`).
    pub provider_id: String,
    /// Model identifier used for the request.
    pub model: String,
    /// HTTP status when a response was observed.
    pub status: Option<u16>,
    /// `sha256` hex of the sanitized bounded body text.
    pub body_sha256: String,
    /// Length of the sanitized bounded body text in bytes.
    pub body_bytes: u64,
    /// Wall-clock time when the response was observed, when a clock is bound.
    pub observed_at_ms: Option<u64>,
}

/// Digest one provider response identity (`ProviderResponseIdentity` v1).
///
/// The payload binds `providerId`, `model`, `status`, `bodySha256`,
/// `bodyBytes`, and `observedAtMs` through the domain-separated artifact
/// primitive `siralos:ProviderResponseIdentity:v1\0` + canonical JSON, matching
/// [`super::reproducibility::compute_provider_input_identity_digest`].
pub fn compute_provider_response_identity_digest(
    record: &ProviderResponseIdentity,
) -> Result<String, String> {
    let payload = json!({
        "providerId": record.provider_id,
        "model": record.model,
        "status": match record.status {
            Some(value) => json!(value),
            None => Value::Null,
        },
        "bodySha256": record.body_sha256,
        "bodyBytes": record.body_bytes,
        "observedAtMs": match record.observed_at_ms {
            Some(value) => json!(value),
            None => Value::Null,
        },
    });
    crate::determinism::helpers::digest_artifact_payload(
        "ProviderResponseIdentity",
        1,
        &payload,
    )
}

/// Typed availability of a provider response for replay.
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderReplayAvailability {
    /// A response was recorded and is available for replay.
    Recorded {
        /// Deterministic digest of the response identity.
        digest: String,
    },
    /// No response was recorded for replay.
    Unavailable {
        /// Human-readable reason.
        reason: String,
    },
}

impl ProviderReplayAvailability {
    /// Deterministic diagnostic for host-visible reporting.
    #[must_use]
    pub fn as_diagnostic(&self) -> String {
        match self {
            Self::Recorded { digest } => {
                let prefix = if digest.len() >= 16 {
                    &digest[..16]
                } else {
                    digest.as_str()
                };
                format!("provider response recorded for replay: {prefix}")
            }
            Self::Unavailable { reason } => {
                format!("replay unavailable: {reason}")
            }
        }
    }
}

/// Port for recording provider responses for replay.
pub trait ReplayRecorder: std::fmt::Debug {
    /// Record one provider response identity.
    fn record_provider_response(&self, identity: &ProviderResponseIdentity);
    /// Whether this recorder is actively recording.
    fn is_recording(&self) -> bool;
}

/// No-op recorder that never records.
#[derive(Debug, Default)]
pub struct NoopReplayRecorder;

impl ReplayRecorder for NoopReplayRecorder {
    fn record_provider_response(&self, _identity: &ProviderResponseIdentity) {}

    fn is_recording(&self) -> bool {
        false
    }
}

/// Collecting recorder that retains every recorded identity and digest.
#[derive(Debug, Default)]
pub struct CollectingReplayRecorder {
    /// Insertion-order record of `(identity, digest)`.
    records: core::cell::RefCell<Vec<(ProviderResponseIdentity, String)>>,
}

impl CollectingReplayRecorder {
    /// Create a new empty collecting recorder.
    #[must_use]
    pub fn new() -> Self {
        Self { records: core::cell::RefCell::new(Vec::new()) }
    }

    /// Snapshot the recorded `(identity, digest)` pairs in insertion order.
    ///
    /// The returned vector is detached; mutating it does not affect the
    /// recorder's internal state.
    #[must_use]
    pub fn records_snapshot(&self) -> Vec<(ProviderResponseIdentity, String)> {
        self.records.borrow().clone()
    }
}

impl ReplayRecorder for CollectingReplayRecorder {
    fn record_provider_response(&self, identity: &ProviderResponseIdentity) {
        if let Ok(digest) = compute_provider_response_identity_digest(identity)
        {
            self.records.borrow_mut().push((identity.clone(), digest));
        }
    }

    fn is_recording(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CollectingReplayRecorder, NoopReplayRecorder,
        ProviderReplayAvailability, ProviderResponseIdentity, ReplayRecorder,
        compute_provider_response_identity_digest,
    };

    fn base_identity() -> ProviderResponseIdentity {
        ProviderResponseIdentity {
            provider_id: "openai".to_owned(),
            model: "gpt-4o".to_owned(),
            status: Some(200),
            body_sha256: "abc".to_owned(),
            body_bytes: 3,
            observed_at_ms: Some(1234),
        }
    }

    #[test]
    fn digest_is_64_hex_chars_and_stable_for_identical_inputs() {
        let identity = base_identity();
        let first = compute_provider_response_identity_digest(&identity)
            .expect("digest");
        let second = compute_provider_response_identity_digest(&identity)
            .expect("digest");
        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn digest_differs_when_status_body_sha256_or_observed_at_ms_differ() {
        let base = base_identity();
        let base_digest =
            compute_provider_response_identity_digest(&base).expect("digest");

        let mut with_status_none = base.clone();
        with_status_none.status = None;
        let digest_status =
            compute_provider_response_identity_digest(&with_status_none)
                .expect("digest");
        assert_ne!(base_digest, digest_status);

        let mut with_body = base.clone();
        with_body.body_sha256 = "different".to_owned();
        let digest_body =
            compute_provider_response_identity_digest(&with_body)
                .expect("digest");
        assert_ne!(base_digest, digest_body);

        let mut with_observed = base.clone();
        with_observed.observed_at_ms = Some(9999);
        let digest_observed =
            compute_provider_response_identity_digest(&with_observed)
                .expect("digest");
        assert_ne!(base_digest, digest_observed);

        let mut with_observed_none = base.clone();
        with_observed_none.observed_at_ms = None;
        let digest_none =
            compute_provider_response_identity_digest(&with_observed_none)
                .expect("digest");
        assert_ne!(base_digest, digest_none);
    }

    #[test]
    fn noop_is_recording_false() {
        let recorder = NoopReplayRecorder;
        assert!(!recorder.is_recording());
        // Recording is a no-op and does not panic.
        recorder.record_provider_response(&base_identity());
    }

    #[test]
    fn collecting_captures_two_records_in_order() {
        let recorder = CollectingReplayRecorder::new();
        assert!(recorder.is_recording());
        let first = ProviderResponseIdentity {
            provider_id: "openai".to_owned(),
            model: "gpt-4o".to_owned(),
            status: Some(200),
            body_sha256: "aaa".to_owned(),
            body_bytes: 3,
            observed_at_ms: Some(1),
        };
        let second = ProviderResponseIdentity {
            provider_id: "anthropic".to_owned(),
            model: "claude-3".to_owned(),
            status: None,
            body_sha256: "bbb".to_owned(),
            body_bytes: 0,
            observed_at_ms: Some(2),
        };
        recorder.record_provider_response(&first);
        recorder.record_provider_response(&second);
        let snapshot = recorder.records_snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].0, first);
        assert_eq!(snapshot[1].0, second);
        assert_eq!(snapshot[0].1.len(), 64);
        assert_eq!(snapshot[1].1.len(), 64);
    }

    #[test]
    fn snapshot_mutation_does_not_affect_internal_state() {
        let recorder = CollectingReplayRecorder::new();
        recorder.record_provider_response(&base_identity());
        let mut snapshot = recorder.records_snapshot();
        assert_eq!(snapshot.len(), 1);
        snapshot.clear();
        assert_eq!(snapshot.len(), 0);
        let again = recorder.records_snapshot();
        assert_eq!(again.len(), 1);
    }

    #[test]
    fn replay_availability_diagnostic_formats_as_specified() {
        let digest = "a".repeat(64);
        let recorded =
            ProviderReplayAvailability::Recorded { digest: digest.clone() };
        assert_eq!(
            recorded.as_diagnostic(),
            format!(
                "provider response recorded for replay: {}",
                &digest[..16]
            )
        );
        let unavailable = ProviderReplayAvailability::Unavailable {
            reason: "live call not recorded".to_owned(),
        };
        assert_eq!(
            unavailable.as_diagnostic(),
            "replay unavailable: live call not recorded"
        );
    }
}
