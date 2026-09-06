//! Provider response replay recording (Stage 8, decision 68 §3, decision 102 usage capture).
//!
//! Records provider HTTP responses via the determinism ports for replay. A
//! non-recorded live call is a typed `unavailable` for replay. The input side
//! is [`super::reproducibility::ProviderInputIdentity`]; this module adds the
//! response side without changing the closed `ProviderEvent`/`ModelEvent` set.
//!
//! Decision 102 extends [`ProviderResponseIdentity`] with optional usage fields
//! parsed from response bodies where present (OpenAI and Anthropic shapes).
//! Absent usage is recorded as `None`, never fabricated. The fields are
//! bounded `u64`s; no credentials, no raw bodies on portable surfaces.

use serde_json::{Value, json};

/// In-process record-then-replay run evidence; nothing persisted.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionReplayEvidence {
    /// Provider identifier recorded in the session.
    pub provider_id: String,
    /// Model identifier recorded in the session.
    pub model: String,
    /// Number of responses recorded during the record phase.
    pub recorded_count: usize,
    /// Snapshot count of the retaining recorder after recording.
    pub recorder_snapshot_count: usize,
}

/// Digest one session replay evidence value (`SessionReplayEvidence` v1).
///
/// The payload binds `providerId`, `model`, `recordedCount`, and
/// `recorderSnapshotCount` through the domain-separated artifact primitive
/// `siralos:SessionReplayEvidence:v1\0` + canonical JSON, mirroring
/// [`compute_provider_response_identity_digest`].
pub fn compute_session_replay_evidence_digest(
    evidence: &SessionReplayEvidence,
) -> String {
    let payload = json!({
        "providerId": evidence.provider_id,
        "model": evidence.model,
        "recordedCount": evidence.recorded_count,
        "recorderSnapshotCount": evidence.recorder_snapshot_count,
    });
    crate::determinism::helpers::digest_artifact_payload(
        "SessionReplayEvidence",
        1,
        &payload,
    )
    .expect("SessionReplayEvidence digest is infallible")
}

/// Identity of one provider HTTP response for replay.
///
/// The record never contains a credential or the raw body text, only the
/// `sha256` of the sanitized bounded text and its byte length. Decision 102
/// adds optional usage fields captured from the response body where present.
#[derive(Debug, Clone, PartialEq, Default)]
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
    /// Provider-reported input/prompt tokens where present.
    pub input_tokens: Option<u64>,
    /// Provider-reported output/completion tokens where present.
    pub output_tokens: Option<u64>,
    /// Provider-reported cached tokens where present (e.g. OpenAI `cached_tokens`).
    pub cached_tokens: Option<u64>,
}

/// Parsed provider usage from a response body (bounded, sanitized `u64`s).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderUsage {
    /// Input/prompt tokens.
    pub input_tokens: Option<u64>,
    /// Output/completion tokens.
    pub output_tokens: Option<u64>,
    /// Cached tokens (where provider reports it).
    pub cached_tokens: Option<u64>,
}

/// Parse provider-reported usage from a sanitized bounded body text.
///
/// Supports:
/// - OpenAI shape: `{usage:{input_tokens|prompt_tokens, output_tokens|completion_tokens, ...cached_tokens}}`
///   and `prompt_tokens_details.cached_tokens` / `cached_tokens_details`
/// - Anthropic shape: `{usage:{input_tokens, output_tokens}}`
///
/// Returns `None`-fields when the body has no usage object or fields are
/// absent/non-numeric. Never fabricates — absent stays absent. Numbers are
/// bounded `u64` (negative or non-integer JSON numbers are treated as absent).
/// `cached_tokens` inside `prompt_tokens_details` or nested `cached_tokens_details`
/// is also recognized for OpenAI cache reporting.
#[must_use]
pub fn parse_provider_usage(body_text: &str) -> ProviderUsage {
    let value: Value = match serde_json::from_str(body_text) {
        Ok(v) => v,
        Err(_) => {
            return ProviderUsage {
                input_tokens: None,
                output_tokens: None,
                cached_tokens: None,
            };
        }
    };
    let usage = match value.get("usage") {
        Some(v) if v.is_object() => v,
        _ => {
            return ProviderUsage {
                input_tokens: None,
                output_tokens: None,
                cached_tokens: None,
            };
        }
    };
    let input_tokens = extract_u64(usage, "input_tokens")
        .or_else(|| extract_u64(usage, "prompt_tokens"));
    let output_tokens = extract_u64(usage, "output_tokens")
        .or_else(|| extract_u64(usage, "completion_tokens"));
    // cached_tokens may appear at usage.cached_tokens, or nested in
    // usage.prompt_tokens_details.cached_tokens, or
    // usage.prompt_tokens_details.cached_tokens_details or similar.
    let cached_tokens = extract_u64(usage, "cached_tokens").or_else(|| {
        usage
            .get("prompt_tokens_details")
            .and_then(|v| v.as_object())
            .and_then(|obj| {
                extract_u64_obj(obj, "cached_tokens").or_else(|| {
                    obj.get("cached_tokens_details")
                        .and_then(|v| v.as_object())
                        .and_then(|inner| {
                            extract_u64_obj(inner, "cached_tokens")
                        })
                })
            })
    });
    ProviderUsage { input_tokens, output_tokens, cached_tokens }
}

fn extract_u64(obj: &Value, key: &str) -> Option<u64> {
    obj.get(key).and_then(as_bounded_u64)
}

fn extract_u64_obj(
    obj: &serde_json::Map<String, Value>,
    key: &str,
) -> Option<u64> {
    obj.get(key).and_then(as_bounded_u64)
}

fn as_bounded_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(n) => n.as_u64(),
        _ => None,
    }
}

/// Digest one provider response identity (`ProviderResponseIdentity` v1/v2).
///
/// When no usage fields are present (`input_tokens`, `output_tokens`,
/// `cached_tokens` all `None`), the payload is the v1 shape binding
/// `providerId`, `model`, `status`, `bodySha256`, `bodyBytes`, and
/// `observedAtMs` — byte-identical to the pre-102 digest for existing
/// recordings (no re-pin required). When any usage field is `Some`, the
/// payload is v2 additionally binding `inputTokens`, `outputTokens`, and
/// `cachedTokens` (null when absent within v2).
pub fn compute_provider_response_identity_digest(
    record: &ProviderResponseIdentity,
) -> Result<String, String> {
    let has_usage = record.input_tokens.is_some()
        || record.output_tokens.is_some()
        || record.cached_tokens.is_some();
    if has_usage {
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
            "inputTokens": match record.input_tokens {
                Some(value) => json!(value),
                None => Value::Null,
            },
            "outputTokens": match record.output_tokens {
                Some(value) => json!(value),
                None => Value::Null,
            },
            "cachedTokens": match record.cached_tokens {
                Some(value) => json!(value),
                None => Value::Null,
            },
        });
        crate::determinism::helpers::digest_artifact_payload(
            "ProviderResponseIdentity",
            2,
            &payload,
        )
    } else {
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
    /// Called after `record_provider_response` when the sanitized bounded body
    /// text is available; default ignores it; trait stays object-safe.
    fn record_provider_response_with_body(
        &self,
        _identity: &ProviderResponseIdentity,
        _body: &str,
    ) {
    }
    /// Whether this recorder is actively recording.
    fn is_recording(&self) -> bool;
}

/// Recorded provider response with bounded sanitized body.
///
/// `body` is the sanitized bounded response text retained in memory only,
/// never persisted, never contains credentials.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayRecording {
    /// Response identity.
    pub identity: ProviderResponseIdentity,
    /// Sanitized bounded body text.
    pub body: String,
}

/// Retaining recorder that keeps full response recordings in memory for replay.
///
/// Recordings live in memory only. The retaining recorder captures both the
/// identity and the sanitized bounded body text via
/// `record_provider_response_with_body`; the base `record_provider_response`
/// call alone does not create a body-bearing record.
#[derive(Debug, Default)]
pub struct RetainingReplayRecorder {
    /// Insertion-order recordings.
    records: core::cell::RefCell<Vec<ReplayRecording>>,
}

impl RetainingReplayRecorder {
    /// Create a new empty retaining recorder.
    #[must_use]
    pub fn new() -> Self {
        Self { records: core::cell::RefCell::new(Vec::new()) }
    }

    /// Snapshot the recorded [`ReplayRecording`]s in insertion order.
    ///
    /// The returned vector is detached; mutating it does not affect the
    /// recorder's internal state.
    #[must_use]
    pub fn records_snapshot(&self) -> Vec<ReplayRecording> {
        self.records.borrow().clone()
    }
}

impl ReplayRecorder for RetainingReplayRecorder {
    fn record_provider_response(&self, _identity: &ProviderResponseIdentity) {}

    fn record_provider_response_with_body(
        &self,
        identity: &ProviderResponseIdentity,
        body: &str,
    ) {
        if compute_provider_response_identity_digest(identity).is_ok() {
            self.records.borrow_mut().push(ReplayRecording {
                identity: identity.clone(),
                body: body.to_owned(),
            });
        }
    }

    fn is_recording(&self) -> bool {
        true
    }
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
        compute_provider_response_identity_digest, parse_provider_usage,
    };

    fn base_identity() -> ProviderResponseIdentity {
        ProviderResponseIdentity {
            provider_id: "openai".to_owned(),
            model: "gpt-4o".to_owned(),
            status: Some(200),
            body_sha256: "abc".to_owned(),
            body_bytes: 3,
            observed_at_ms: Some(1234),
            input_tokens: None,
            output_tokens: None,
            cached_tokens: None,
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
            input_tokens: None,
            output_tokens: None,
            cached_tokens: None,
        };
        let second = ProviderResponseIdentity {
            provider_id: "anthropic".to_owned(),
            model: "claude-3".to_owned(),
            status: None,
            body_sha256: "bbb".to_owned(),
            body_bytes: 0,
            observed_at_ms: Some(2),
            input_tokens: None,
            output_tokens: None,
            cached_tokens: None,
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

    #[test]
    fn retaining_recorder_captures_identity_and_body() {
        use super::RetainingReplayRecorder;
        let recorder = RetainingReplayRecorder::new();
        assert!(recorder.is_recording());
        let identity = base_identity();
        let body = "hello body";
        recorder.record_provider_response_with_body(&identity, body);
        let snapshot = recorder.records_snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].identity, identity);
        assert_eq!(snapshot[0].body, body);
        // Digest of stored identity matches computed digest.
        let expected = compute_provider_response_identity_digest(&identity)
            .expect("digest");
        let actual =
            compute_provider_response_identity_digest(&snapshot[0].identity)
                .expect("digest");
        assert_eq!(expected, actual);
    }

    #[test]
    fn retaining_recorder_snapshot_is_detached() {
        use super::RetainingReplayRecorder;
        let recorder = RetainingReplayRecorder::new();
        let identity = base_identity();
        recorder.record_provider_response_with_body(&identity, "body");
        let mut snapshot = recorder.records_snapshot();
        assert_eq!(snapshot.len(), 1);
        snapshot.clear();
        assert_eq!(snapshot.len(), 0);
        let again = recorder.records_snapshot();
        assert_eq!(again.len(), 1);
    }

    #[test]
    fn retaining_recorder_base_record_does_not_add_body_bearing_record() {
        use super::RetainingReplayRecorder;
        let recorder = RetainingReplayRecorder::new();
        let identity = base_identity();
        recorder.record_provider_response(&identity);
        assert_eq!(recorder.records_snapshot().len(), 0);
        // Now the with_body call adds exactly one.
        recorder.record_provider_response_with_body(&identity, "body");
        assert_eq!(recorder.records_snapshot().len(), 1);
    }

    #[test]
    fn noop_unaffected_by_new_method() {
        let recorder = NoopReplayRecorder;
        let identity = base_identity();
        recorder.record_provider_response_with_body(&identity, "ignored");
        assert!(!recorder.is_recording());
    }

    #[test]
    fn session_replay_evidence_digest_is_stable_and_canonical() {
        use super::{
            SessionReplayEvidence, compute_session_replay_evidence_digest,
        };
        use serde_json::json;

        let evidence = SessionReplayEvidence {
            provider_id: "session-subject".to_owned(),
            model: "session-model".to_owned(),
            recorded_count: 2,
            recorder_snapshot_count: 2,
        };
        let clone = evidence.clone();
        assert_eq!(evidence, clone);
        let first = compute_session_replay_evidence_digest(&evidence);
        let second = compute_session_replay_evidence_digest(&clone);
        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        // Must match the canonical payload digest.
        let payload = json!({
            "providerId": evidence.provider_id,
            "model": evidence.model,
            "recordedCount": evidence.recorded_count,
            "recorderSnapshotCount": evidence.recorder_snapshot_count,
        });
        let canonical = crate::determinism::helpers::digest_artifact_payload(
            "SessionReplayEvidence",
            1,
            &payload,
        )
        .expect("digest");
        assert_eq!(first, canonical);
    }

    // --- Decision 102: usage parsing ---

    #[test]
    fn parse_usage_openai_shape() {
        let body = r#"{"choices":[{"message":{"content":"hi"}}],"usage":{"prompt_tokens":100,"completion_tokens":50,"cached_tokens":10}}"#;
        let usage = parse_provider_usage(body);
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.output_tokens, Some(50));
        assert_eq!(usage.cached_tokens, Some(10));
    }

    #[test]
    fn parse_usage_openai_input_tokens_alias() {
        let body = r#"{"usage":{"input_tokens":77,"output_tokens":33}}"#;
        let usage = parse_provider_usage(body);
        assert_eq!(usage.input_tokens, Some(77));
        assert_eq!(usage.output_tokens, Some(33));
        assert_eq!(usage.cached_tokens, None);
    }

    #[test]
    fn parse_usage_anthropic_shape() {
        let body = r#"{"content":[{"type":"text","text":"hi"}],"usage":{"input_tokens":200,"output_tokens":40}}"#;
        let usage = parse_provider_usage(body);
        assert_eq!(usage.input_tokens, Some(200));
        assert_eq!(usage.output_tokens, Some(40));
        assert_eq!(usage.cached_tokens, None);
    }

    #[test]
    fn parse_usage_absent_is_none() {
        let body = r#"{"choices":[{"message":{"content":"hi"}}]}"#;
        let usage = parse_provider_usage(body);
        assert_eq!(usage.input_tokens, None);
        assert_eq!(usage.output_tokens, None);
        assert_eq!(usage.cached_tokens, None);
    }

    #[test]
    fn parse_usage_malformed_body_is_none() {
        let usage = parse_provider_usage("not json at all");
        assert_eq!(usage.input_tokens, None);
        assert_eq!(usage.output_tokens, None);
        assert_eq!(usage.cached_tokens, None);
    }

    #[test]
    fn parse_usage_cached_in_prompt_tokens_details() {
        let body = r#"{"usage":{"prompt_tokens":100,"completion_tokens":20,"prompt_tokens_details":{"cached_tokens":42}}}"#;
        let usage = parse_provider_usage(body);
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.output_tokens, Some(20));
        assert_eq!(usage.cached_tokens, Some(42));
    }

    #[test]
    fn parse_usage_non_numeric_ignored() {
        let body = r#"{"usage":{"input_tokens":"100","output_tokens":null}}"#;
        let usage = parse_provider_usage(body);
        assert_eq!(usage.input_tokens, None);
        assert_eq!(usage.output_tokens, None);
    }

    #[test]
    fn usage_fields_are_bounded_and_sanitized() {
        // Very large u64 is bounded by JSON number handling; negative ignored.
        let body =
            r#"{"usage":{"input_tokens":-1,"output_tokens":999999999999999}}"#;
        let usage = parse_provider_usage(body);
        assert_eq!(usage.input_tokens, None);
        assert_eq!(usage.output_tokens, Some(999999999999999));
    }

    #[test]
    fn digest_v1_stable_when_no_usage() {
        let identity = base_identity();
        // v1 digest must be stable — same as before 102.
        let d1 = compute_provider_response_identity_digest(&identity)
            .expect("digest");
        let d2 = compute_provider_response_identity_digest(&identity)
            .expect("digest");
        assert_eq!(d1, d2);
        assert_eq!(d1.len(), 64);
    }

    #[test]
    fn digest_v2_differs_when_usage_present() {
        let base = base_identity();
        let base_digest =
            compute_provider_response_identity_digest(&base).expect("digest");
        let mut with_usage = base.clone();
        with_usage.input_tokens = Some(100);
        with_usage.output_tokens = Some(50);
        let usage_digest =
            compute_provider_response_identity_digest(&with_usage)
                .expect("digest");
        assert_ne!(base_digest, usage_digest);
        assert_eq!(usage_digest.len(), 64);
        // With usage present, digest is v2 (different domain separation).
        // Verify determinism.
        let again = compute_provider_response_identity_digest(&with_usage)
            .expect("digest");
        assert_eq!(usage_digest, again);
    }

    #[test]
    fn digest_v2_cached_tokens_affects_digest() {
        let base = base_identity();
        let mut a = base.clone();
        a.input_tokens = Some(10);
        let mut b = base.clone();
        b.input_tokens = Some(10);
        b.cached_tokens = Some(2);
        let da =
            compute_provider_response_identity_digest(&a).expect("digest");
        let db =
            compute_provider_response_identity_digest(&b).expect("digest");
        assert_ne!(da, db);
    }
}
