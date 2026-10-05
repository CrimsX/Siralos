//! Provider response replay recording (Stage 8, decision 68 §3, decision 102 usage capture).
//!
//! Records provider HTTP responses via the determinism ports for replay. A
//! non-recorded live call is a typed `unavailable` for replay. The in-process
//! recorder retains bounded response evidence in memory; the separate replay
//! store may persist only validated, sanitized bodies under its own bounds.
//!
//! Decision 102 extends [`ProviderResponseIdentity`] with optional usage fields
//! parsed from response bodies where present (OpenAI and Anthropic shapes).
//! Absent usage is recorded as `None`, never fabricated. The fields are
//! bounded `u64`s; no credentials, no raw bodies on portable surfaces.

use serde_json::{Value, json};

/// In-process record-then-replay run evidence; nothing persisted.
#[derive(Clone, PartialEq)]
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

impl std::fmt::Debug for SessionReplayEvidence {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter
            .debug_struct("SessionReplayEvidence")
            .field("provider_id", &"[CONFIGURED]")
            .field("model", &"[CONFIGURED]")
            .field("recorded_count", &self.recorded_count)
            .field("recorder_snapshot_count", &self.recorder_snapshot_count)
            .finish()
    }
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
#[derive(Clone, PartialEq, Default)]
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

/// Maximum byte length of a provider or model replay identifier.
pub const MAX_REPLAY_IDENTIFIER_BYTES: usize = 256;

/// Number of ASCII hexadecimal characters in a SHA-256 digest.
pub const REPLAY_SHA256_HEX_LENGTH: usize = 64;

/// Invalid provider-response identity fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayIdentityError {
    /// The provider identifier is empty or whitespace-only.
    ProviderIdEmpty,
    /// The provider identifier exceeds the bounded identifier length.
    ProviderIdTooLong {
        /// Observed UTF-8 byte length.
        bytes: usize,
    },
    /// The provider identifier contains a control character.
    ProviderIdControl,
    /// The model identifier is empty or whitespace-only.
    ModelEmpty,
    /// The model identifier exceeds the bounded identifier length.
    ModelTooLong {
        /// Observed UTF-8 byte length.
        bytes: usize,
    },
    /// The model identifier contains a control character.
    ModelControl,
    /// The body digest is not 64 ASCII hexadecimal characters.
    BodySha256Invalid,
}

impl std::fmt::Display for ReplayIdentityError {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::ProviderIdEmpty => {
                formatter.write_str("provider id is empty")
            }
            Self::ProviderIdTooLong { bytes } => write!(
                formatter,
                "provider id exceeds {MAX_REPLAY_IDENTIFIER_BYTES} bytes: {bytes}"
            ),
            Self::ProviderIdControl => {
                formatter.write_str("provider id contains a control character")
            }
            Self::ModelEmpty => formatter.write_str("model is empty"),
            Self::ModelTooLong { bytes } => write!(
                formatter,
                "model exceeds {MAX_REPLAY_IDENTIFIER_BYTES} bytes: {bytes}"
            ),
            Self::ModelControl => {
                formatter.write_str("model contains a control character")
            }
            Self::BodySha256Invalid => formatter
                .write_str("body sha256 is not 64 hexadecimal characters"),
        }
    }
}

impl std::error::Error for ReplayIdentityError {}

/// Return whether a value is a canonical-length SHA-256 hexadecimal digest.
///
/// Upper- and lower-case hexadecimal spellings are accepted; callers that
/// persist the value should canonicalize it to lower case separately.
#[must_use]
pub fn is_valid_replay_sha256(value: &str) -> bool {
    value.len() == REPLAY_SHA256_HEX_LENGTH
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

impl ProviderResponseIdentity {
    /// Validate the identity fields used by replay persistence and playback.
    ///
    /// This method intentionally does not inspect a response body. Use
    /// [`ReplayRecording::validate`] when the retained body is available.
    pub fn validate(&self) -> Result<(), ReplayIdentityError> {
        if self.provider_id.is_empty() || self.provider_id.trim().is_empty() {
            return Err(ReplayIdentityError::ProviderIdEmpty);
        }
        if self.provider_id.len() > MAX_REPLAY_IDENTIFIER_BYTES {
            return Err(ReplayIdentityError::ProviderIdTooLong {
                bytes: self.provider_id.len(),
            });
        }
        if self.provider_id.chars().any(char::is_control) {
            return Err(ReplayIdentityError::ProviderIdControl);
        }
        if self.model.is_empty() || self.model.trim().is_empty() {
            return Err(ReplayIdentityError::ModelEmpty);
        }
        if self.model.len() > MAX_REPLAY_IDENTIFIER_BYTES {
            return Err(ReplayIdentityError::ModelTooLong {
                bytes: self.model.len(),
            });
        }
        if self.model.chars().any(char::is_control) {
            return Err(ReplayIdentityError::ModelControl);
        }
        if !is_valid_replay_sha256(&self.body_sha256) {
            return Err(ReplayIdentityError::BodySha256Invalid);
        }
        Ok(())
    }
}

impl std::fmt::Debug for ProviderResponseIdentity {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderResponseIdentity")
            .field("provider_id", &"[CONFIGURED]")
            .field("model", &"[CONFIGURED]")
            .field("status", &self.status)
            .field("body_sha256", &self.body_sha256)
            .field("body_bytes", &self.body_bytes)
            .field("observed_at_ms", &self.observed_at_ms)
            .field("input_tokens", &self.input_tokens)
            .field("output_tokens", &self.output_tokens)
            .field("cached_tokens", &self.cached_tokens)
            .finish()
    }
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
    record.validate().map_err(|error| error.to_string())?;
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
            "bodySha256": record.body_sha256.to_ascii_lowercase(),
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
            "bodySha256": record.body_sha256.to_ascii_lowercase(),
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
    /// Record the sanitized bounded body.
    ///
    /// This is the original additive, void-returning extension point. It
    /// remains unchanged for downstream recorder implementations that
    /// predate checked retention. Use [`Self::try_record_provider_response_with_body`]
    /// when the caller needs a truthful body-availability result.
    fn record_provider_response_with_body(
        &self,
        _identity: &ProviderResponseIdentity,
        _body: &str,
    ) {
    }
    /// Try to retain the sanitized bounded body and report acceptance.
    ///
    /// The compatibility default invokes the legacy void method for its side
    /// effect and returns `false`: an old implementation cannot prove that
    /// it retained a replayable body, so callers must surface that result as
    /// unavailable rather than claiming body availability.
    fn try_record_provider_response_with_body(
        &self,
        identity: &ProviderResponseIdentity,
        body: &str,
    ) -> bool {
        self.record_provider_response_with_body(identity, body);
        false
    }
    /// Try to retain a body with the canonical digest of the originating
    /// request.
    ///
    /// The default delegates to the legacy body method for compatibility but
    /// returns `false` because it cannot establish route binding. A
    /// route-bound retaining recorder overrides this method and returns `true`
    /// only after retaining the canonical request digest.
    fn try_record_provider_response_with_body_and_request(
        &self,
        identity: &ProviderResponseIdentity,
        body: &str,
        _request_sha256: &str,
    ) -> bool {
        let _ = self.try_record_provider_response_with_body(identity, body);
        false
    }
    /// Try to retain sanitized terminal evidence without consuming replay
    /// capacity. The default preserves the legacy body side effect but cannot
    /// prove retention, so it returns `false`; retaining implementations can
    /// override this to keep diagnostics while leaving successful replay
    /// capacity available.
    fn try_record_provider_response_with_body_as_evidence(
        &self,
        identity: &ProviderResponseIdentity,
        body: &str,
    ) -> bool {
        self.record_provider_response_with_body(identity, body);
        false
    }
    /// Whether this recorder is actively recording.
    fn is_recording(&self) -> bool;
}

/// Recorded provider response with bounded sanitized body.
///
/// `body` is the sanitized bounded response text retained in memory only,
/// never persisted, never contains credentials.
#[derive(Clone, PartialEq)]
pub struct ReplayRecording {
    /// Response identity.
    pub identity: ProviderResponseIdentity,
    /// Sanitized bounded body text.
    pub body: String,
    /// Canonical SHA-256 of the request that produced this response. `None`
    /// marks legacy recordings made before request binding was available.
    pub request_sha256: Option<String>,
}

/// A detached replay recording failed identity or body integrity validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayRecordingValidationError {
    /// The response identity is malformed.
    Identity(ReplayIdentityError),
    /// A replay recording must contain a retained body.
    BodyEmpty,
    /// The retained body exceeds the per-response memory bound.
    BodyTooLarge,
    /// The declared body length differs from the retained body length.
    BodyBytesMismatch,
    /// The declared body digest differs from the retained body.
    BodyDigestMismatch,
    /// The optional request digest is not 64 hexadecimal characters.
    RequestDigestInvalid,
}

impl std::fmt::Display for ReplayRecordingValidationError {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::Identity(error) => {
                write!(formatter, "invalid replay identity: {error}")
            }
            Self::BodyEmpty => {
                formatter.write_str("replay recording body is empty")
            }
            Self::BodyTooLarge => {
                formatter.write_str("replay recording body exceeds its bound")
            }
            Self::BodyBytesMismatch => {
                formatter.write_str("replay body byte count does not match")
            }
            Self::BodyDigestMismatch => {
                formatter.write_str("replay body digest does not match")
            }
            Self::RequestDigestInvalid => {
                formatter.write_str("replay request digest is invalid")
            }
        }
    }
}

impl std::error::Error for ReplayRecordingValidationError {}

impl ReplayRecording {
    /// Validate the complete body-bearing replay record.
    ///
    /// Identity-only records used by the in-process recorder should validate
    /// [`ProviderResponseIdentity`] directly instead; this method intentionally
    /// rejects an empty body so persisted replay data cannot claim an identity
    /// for material that was never retained.
    pub fn validate(&self) -> Result<(), ReplayRecordingValidationError> {
        self.identity
            .validate()
            .map_err(ReplayRecordingValidationError::Identity)?;
        if self.body.is_empty() {
            return Err(ReplayRecordingValidationError::BodyEmpty);
        }
        if self.body.len() > MAX_RETAINED_REPLAY_BODY_BYTES {
            return Err(ReplayRecordingValidationError::BodyTooLarge);
        }
        let body_bytes = u64::try_from(self.body.len())
            .map_err(|_| ReplayRecordingValidationError::BodyBytesMismatch)?;
        if self.identity.body_bytes != body_bytes {
            return Err(ReplayRecordingValidationError::BodyBytesMismatch);
        }
        let body_digest = crate::identity::sha256_hex(self.body.as_bytes());
        if !self.identity.body_sha256.eq_ignore_ascii_case(&body_digest) {
            return Err(ReplayRecordingValidationError::BodyDigestMismatch);
        }
        if self
            .request_sha256
            .as_deref()
            .is_some_and(|digest| !is_valid_replay_sha256(digest))
        {
            return Err(ReplayRecordingValidationError::RequestDigestInvalid);
        }
        Ok(())
    }
}

impl std::fmt::Debug for ReplayRecording {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter
            .debug_struct("ReplayRecording")
            .field(
                "provider_configured",
                &!self.identity.provider_id.is_empty(),
            )
            .field("model_configured", &!self.identity.model.is_empty())
            .field("status", &self.identity.status)
            .field("body_bytes", &self.body.len())
            .field("body_sha256", &"[DIGEST]")
            .field("request_bound", &self.request_sha256.is_some())
            .finish()
    }
}

/// Maximum number of body-bearing recordings retained in one session.
pub const MAX_RETAINED_REPLAY_RECORDINGS: usize =
    super::replay_store::REPLAY_STORE_MAX_RECORDINGS;
/// Maximum bytes retained by one body-bearing recording.
pub const MAX_RETAINED_REPLAY_BODY_BYTES: usize = 1024 * 1024;
/// Maximum aggregate bytes retained by the replay portion of one recorder.
pub const MAX_RETAINED_REPLAY_TOTAL_BYTES: usize =
    super::replay_store::REPLAY_STORE_MAX_TOTAL_BODY_BYTES;
/// Maximum number of terminal evidence records retained for diagnostics.
pub const MAX_RETAINED_EVIDENCE_RECORDS: usize = 16;
/// Maximum aggregate terminal evidence bytes retained for diagnostics.
pub const MAX_RETAINED_EVIDENCE_TOTAL_BYTES: usize = 1024 * 1024;

/// Retaining recorder that keeps full response recordings in memory for replay.
///
/// Recordings live in memory only. Successful/replay body-bearing calls are
/// bounded by the replay count and aggregate-byte budgets. Terminal evidence
/// is retained in a separate diagnostic budget of at most 16 records and 1 MiB
/// so it cannot starve a later successful replay. Replay availability still
/// requires the provider shape/status gate. A base identity call also retains
/// an identity-only record so a terminal transport/failure outcome is not
/// silently lost. Once the replay budget is reached, further replay retention
/// fails closed and `is_recording()` becomes `false`.
#[derive(Default)]
pub struct RetainingReplayRecorder {
    /// Insertion-order replay recordings and terminal evidence.
    records: core::cell::RefCell<Vec<ReplayRecording>>,
    /// Parallel classification for `records`; evidence placeholders must not
    /// be upgraded into replay bodies.
    record_is_evidence: core::cell::RefCell<Vec<bool>>,
    /// Number of records that consume the replay capacity budget.
    replay_record_count: core::cell::RefCell<usize>,
    /// Aggregate replay-body bytes.
    total_body_bytes: core::cell::RefCell<usize>,
    /// Number of separately retained terminal-evidence records.
    evidence_record_count: core::cell::RefCell<usize>,
    /// Aggregate terminal-evidence body bytes.
    evidence_total_body_bytes: core::cell::RefCell<usize>,
    /// Once set, no further replay body is retained.
    saturated: core::cell::RefCell<bool>,
}

impl std::fmt::Debug for RetainingReplayRecorder {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter
            .debug_struct("RetainingReplayRecorder")
            .field("record_count", &self.records.borrow().len())
            .field("replay_record_count", &self.replay_record_count.borrow())
            .field("total_body_bytes", &self.total_body_bytes.borrow())
            .field(
                "evidence_record_count",
                &self.evidence_record_count.borrow(),
            )
            .field(
                "evidence_total_body_bytes",
                &self.evidence_total_body_bytes.borrow(),
            )
            .field("saturated", &self.saturated.borrow())
            .finish()
    }
}

impl RetainingReplayRecorder {
    /// Create a new empty retaining recorder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            records: core::cell::RefCell::new(Vec::new()),
            record_is_evidence: core::cell::RefCell::new(Vec::new()),
            replay_record_count: core::cell::RefCell::new(0),
            total_body_bytes: core::cell::RefCell::new(0),
            evidence_record_count: core::cell::RefCell::new(0),
            evidence_total_body_bytes: core::cell::RefCell::new(0),
            saturated: core::cell::RefCell::new(false),
        }
    }

    /// Snapshot the recorded [`ReplayRecording`]s in insertion order.
    ///
    /// The returned vector is detached; mutating it does not affect the
    /// recorder's internal state.
    #[must_use]
    pub fn records_snapshot(&self) -> Vec<ReplayRecording> {
        self.records.borrow().clone()
    }

    /// Snapshot only records eligible for replay projection.
    ///
    /// Terminal evidence remains available through [`Self::records_snapshot`]
    /// for diagnostics, but its private classification is preserved here so a
    /// syntactically valid failed body can never be persisted or replayed.
    #[must_use]
    pub fn replayable_records_snapshot(&self) -> Vec<ReplayRecording> {
        let records = self.records.borrow();
        let evidence = self.record_is_evidence.borrow();
        records
            .iter()
            .zip(evidence.iter())
            .filter(|(_, is_evidence)| !**is_evidence)
            .map(|(recording, _)| recording.clone())
            .filter(|recording| !recording.body.is_empty())
            .collect()
    }

    fn has_identity_placeholder(
        &self,
        identity: &ProviderResponseIdentity,
    ) -> bool {
        let records = self.records.borrow();
        let evidence = self.record_is_evidence.borrow();
        records.iter().enumerate().any(|(index, recording)| {
            !evidence.get(index).copied().unwrap_or(false)
                && recording.identity == *identity
                && recording.body.is_empty()
                && recording.request_sha256.is_none()
        })
    }

    fn identity_placeholder_indices(
        &self,
        identity: &ProviderResponseIdentity,
    ) -> Vec<usize> {
        let records = self.records.borrow();
        let evidence = self.record_is_evidence.borrow();
        records
            .iter()
            .enumerate()
            .filter_map(|(index, recording)| {
                (!evidence.get(index).copied().unwrap_or(false)
                    && recording.identity == *identity
                    && recording.body.is_empty()
                    && recording.request_sha256.is_none())
                .then_some(index)
            })
            .collect()
    }

    fn identity_evidence_indices(
        &self,
        identity: &ProviderResponseIdentity,
    ) -> Vec<usize> {
        let records = self.records.borrow();
        let evidence = self.record_is_evidence.borrow();
        records
            .iter()
            .enumerate()
            .filter_map(|(index, recording)| {
                (evidence.get(index).copied().unwrap_or(false)
                    && recording.identity == *identity
                    && recording.body.is_empty()
                    && recording.request_sha256.is_none())
                .then_some(index)
            })
            .collect()
    }

    fn refresh_saturation(&self) {
        if *self.replay_record_count.borrow() < MAX_RETAINED_REPLAY_RECORDINGS
            && *self.total_body_bytes.borrow()
                < MAX_RETAINED_REPLAY_TOTAL_BYTES
        {
            *self.saturated.borrow_mut() = false;
        }
    }

    fn discard_identity_placeholders(
        &self,
        identity: &ProviderResponseIdentity,
    ) -> bool {
        let indices = self.identity_placeholder_indices(identity);
        if indices.is_empty() {
            return false;
        }
        for index in indices.iter().rev() {
            self.records.borrow_mut().remove(*index);
            self.record_is_evidence.borrow_mut().remove(*index);
        }
        let mut replay_count = self.replay_record_count.borrow_mut();
        *replay_count = replay_count.saturating_sub(indices.len());
        drop(replay_count);
        self.refresh_saturation();
        true
    }

    fn record_identity_only(
        &self,
        identity: &ProviderResponseIdentity,
    ) -> bool {
        if identity.validate().is_err() {
            return false;
        }
        if *self.saturated.borrow() {
            return self.record_evidence_identity_only(identity);
        }
        if self.has_identity_placeholder(identity) {
            return true;
        }
        let mut records = self.records.borrow_mut();
        if *self.replay_record_count.borrow() >= MAX_RETAINED_REPLAY_RECORDINGS
        {
            drop(records);
            *self.saturated.borrow_mut() = true;
            return self.record_evidence_identity_only(identity);
        }
        records.push(ReplayRecording {
            identity: identity.clone(),
            body: String::new(),
            request_sha256: None,
        });
        self.record_is_evidence.borrow_mut().push(false);
        *self.replay_record_count.borrow_mut() += 1;
        true
    }

    fn retain_body(
        &self,
        identity: &ProviderResponseIdentity,
        body: &str,
        request_sha256: Option<&str>,
    ) -> bool {
        if identity.validate().is_err()
            || request_sha256
                .is_some_and(|digest| !is_valid_replay_sha256(digest))
        {
            return false;
        }

        // Keep the identity even when the body is rejected. The caller can
        // then report a truthful unavailable replay without losing the fact
        // that a terminal response was observed. A rejected body is retained
        // only in the separate evidence budget.
        let body_valid = !body.is_empty()
            && body.len() <= MAX_RETAINED_REPLAY_BODY_BYTES
            && u64::try_from(body.len()).ok() == Some(identity.body_bytes)
            && identity.body_sha256.eq_ignore_ascii_case(
                &crate::identity::sha256_hex(body.as_bytes()),
            );
        if !body_valid {
            // A body that fails length/digest validation is evidence, not a
            // replay record. Keeping it out of the replay budget prevents a
            // malformed provider response from starving a later valid turn.
            let _ = self.record_evidence_identity_only(identity);
            return false;
        }
        if *self.saturated.borrow() {
            let _ = self.record_evidence_identity_only(identity);
            return false;
        }
        let current_total = *self.total_body_bytes.borrow();
        if current_total.saturating_add(body.len())
            > MAX_RETAINED_REPLAY_TOTAL_BYTES
        {
            *self.saturated.borrow_mut() = true;
            let _ = self.record_evidence_identity_only(identity);
            return false;
        }

        let mut records = self.records.borrow_mut();
        let request_sha256 = request_sha256.map(str::to_ascii_lowercase);
        let last_is_evidence =
            self.record_is_evidence.borrow().last().copied().unwrap_or(false);
        if !last_is_evidence {
            if let Some(last) = records.last_mut() {
                if last.identity == *identity
                    && last.body.is_empty()
                    && last.request_sha256.is_none()
                {
                    last.body = body.to_owned();
                    last.request_sha256 = request_sha256;
                    *self.total_body_bytes.borrow_mut() += body.len();
                    return true;
                }
            }
        }
        if *self.replay_record_count.borrow() >= MAX_RETAINED_REPLAY_RECORDINGS
        {
            drop(records);
            *self.saturated.borrow_mut() = true;
            let _ = self.record_evidence_identity_only(identity);
            return false;
        }
        records.push(ReplayRecording {
            identity: identity.clone(),
            body: body.to_owned(),
            request_sha256,
        });
        self.record_is_evidence.borrow_mut().push(false);
        *self.replay_record_count.borrow_mut() += 1;
        *self.total_body_bytes.borrow_mut() += body.len();
        true
    }

    fn record_evidence_identity_only(
        &self,
        identity: &ProviderResponseIdentity,
    ) -> bool {
        if identity.validate().is_err() {
            return false;
        }
        if !self.identity_evidence_indices(identity).is_empty() {
            return true;
        }
        let placeholders = self.identity_placeholder_indices(identity);
        if !placeholders.is_empty() {
            let current_evidence = *self.evidence_record_count.borrow();
            if current_evidence.saturating_add(placeholders.len())
                <= MAX_RETAINED_EVIDENCE_RECORDS
            {
                let mut evidence = self.record_is_evidence.borrow_mut();
                for index in &placeholders {
                    if let Some(slot) = evidence.get_mut(*index) {
                        *slot = true;
                    }
                }
                drop(evidence);
                let mut replay_count = self.replay_record_count.borrow_mut();
                *replay_count =
                    replay_count.saturating_sub(placeholders.len());
                drop(replay_count);
                *self.evidence_record_count.borrow_mut() += placeholders.len();
                self.refresh_saturation();
                return true;
            }
            // The evidence budget is full. Remove the phantom replay
            // reservation rather than allowing it to starve a later valid
            // response; the bounded recorder truthfully reports no retained
            // identity when no evidence capacity remains.
            return self.discard_identity_placeholders(identity);
        }
        if *self.evidence_record_count.borrow()
            >= MAX_RETAINED_EVIDENCE_RECORDS
        {
            return false;
        }
        self.records.borrow_mut().push(ReplayRecording {
            identity: identity.clone(),
            body: String::new(),
            request_sha256: None,
        });
        self.record_is_evidence.borrow_mut().push(true);
        *self.evidence_record_count.borrow_mut() += 1;
        true
    }

    fn retain_evidence_body(
        &self,
        identity: &ProviderResponseIdentity,
        body: &str,
        request_sha256: Option<&str>,
    ) -> bool {
        if identity.validate().is_err()
            || request_sha256
                .is_some_and(|digest| !is_valid_replay_sha256(digest))
        {
            return false;
        }
        let body_valid = !body.is_empty()
            && body.len() <= MAX_RETAINED_REPLAY_BODY_BYTES
            && u64::try_from(body.len()).ok() == Some(identity.body_bytes)
            && identity.body_sha256.eq_ignore_ascii_case(
                &crate::identity::sha256_hex(body.as_bytes()),
            );
        if !body_valid {
            let _ = self.record_evidence_identity_only(identity);
            return false;
        }
        if *self.evidence_record_count.borrow()
            >= MAX_RETAINED_EVIDENCE_RECORDS
        {
            return false;
        }
        if self.evidence_total_body_bytes.borrow().saturating_add(body.len())
            > MAX_RETAINED_EVIDENCE_TOTAL_BYTES
        {
            let _ = self.record_evidence_identity_only(identity);
            return false;
        }
        self.records.borrow_mut().push(ReplayRecording {
            identity: identity.clone(),
            body: body.to_owned(),
            request_sha256: request_sha256.map(str::to_ascii_lowercase),
        });
        self.record_is_evidence.borrow_mut().push(true);
        *self.evidence_record_count.borrow_mut() += 1;
        *self.evidence_total_body_bytes.borrow_mut() += body.len();
        true
    }
}

impl ReplayRecorder for RetainingReplayRecorder {
    fn record_provider_response(&self, identity: &ProviderResponseIdentity) {
        let _ = self.record_identity_only(identity);
    }

    fn record_provider_response_with_body(
        &self,
        identity: &ProviderResponseIdentity,
        body: &str,
    ) {
        let _ = self.try_record_provider_response_with_body(identity, body);
    }

    fn try_record_provider_response_with_body(
        &self,
        identity: &ProviderResponseIdentity,
        body: &str,
    ) -> bool {
        self.retain_body(identity, body, None)
    }

    fn try_record_provider_response_with_body_and_request(
        &self,
        identity: &ProviderResponseIdentity,
        body: &str,
        request_sha256: &str,
    ) -> bool {
        self.retain_body(identity, body, Some(request_sha256))
    }

    fn try_record_provider_response_with_body_as_evidence(
        &self,
        identity: &ProviderResponseIdentity,
        body: &str,
    ) -> bool {
        self.retain_evidence_body(identity, body, None)
    }

    fn is_recording(&self) -> bool {
        !*self.saturated.borrow()
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
///
/// This recorder is intentionally metadata-only: it has no body retention
/// contract. Body-aware calls therefore record the identity evidence and
/// return `false`, requiring callers to report replay as unavailable rather
/// than implying that a body was retained.
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

    fn record_provider_response_with_body(
        &self,
        identity: &ProviderResponseIdentity,
        _body: &str,
    ) {
        self.record_provider_response(identity);
    }

    fn try_record_provider_response_with_body(
        &self,
        identity: &ProviderResponseIdentity,
        body: &str,
    ) -> bool {
        self.record_provider_response_with_body(identity, body);
        false
    }

    fn try_record_provider_response_with_body_and_request(
        &self,
        identity: &ProviderResponseIdentity,
        body: &str,
        _request_sha256: &str,
    ) -> bool {
        self.record_provider_response_with_body(identity, body);
        false
    }

    fn try_record_provider_response_with_body_as_evidence(
        &self,
        identity: &ProviderResponseIdentity,
        body: &str,
    ) -> bool {
        self.record_provider_response_with_body(identity, body);
        false
    }

    fn is_recording(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CollectingReplayRecorder, MAX_RETAINED_EVIDENCE_TOTAL_BYTES,
        MAX_RETAINED_REPLAY_BODY_BYTES, NoopReplayRecorder,
        ProviderReplayAvailability, ProviderResponseIdentity,
        ReplayIdentityError, ReplayRecorder, ReplayRecording,
        ReplayRecordingValidationError,
        compute_provider_response_identity_digest, is_valid_replay_sha256,
        parse_provider_usage,
    };

    fn base_identity() -> ProviderResponseIdentity {
        ProviderResponseIdentity {
            provider_id: "openai".to_owned(),
            model: "gpt-4o".to_owned(),
            status: Some(200),
            body_sha256: crate::identity::sha256_hex(b"body"),
            body_bytes: 4,
            observed_at_ms: Some(1234),
            input_tokens: None,
            output_tokens: None,
            cached_tokens: None,
        }
    }

    #[test]
    fn identity_validation_bounds_provider_model_and_digest() {
        let identity = base_identity();
        assert_eq!(identity.validate(), Ok(()));

        let mut missing_provider = identity.clone();
        missing_provider.provider_id.clear();
        assert_eq!(
            missing_provider.validate(),
            Err(ReplayIdentityError::ProviderIdEmpty)
        );

        let mut oversized_provider = identity.clone();
        oversized_provider.provider_id =
            "p".repeat(super::MAX_REPLAY_IDENTIFIER_BYTES + 1);
        assert_eq!(
            oversized_provider.validate(),
            Err(ReplayIdentityError::ProviderIdTooLong {
                bytes: super::MAX_REPLAY_IDENTIFIER_BYTES + 1,
            })
        );

        let mut control_model = identity.clone();
        control_model.model = "model\n".to_owned();
        assert_eq!(
            control_model.validate(),
            Err(ReplayIdentityError::ModelControl)
        );

        let mut bad_digest = identity;
        bad_digest.body_sha256 = "not-a-digest".to_owned();
        assert_eq!(
            bad_digest.validate(),
            Err(ReplayIdentityError::BodySha256Invalid)
        );
    }

    #[test]
    fn recording_validation_binds_body_length_digest_and_request_identity() {
        let body = "body";
        let mut identity = base_identity();
        identity.body_sha256 = crate::identity::sha256_hex(body.as_bytes());
        identity.body_bytes = body.len() as u64;
        let recording = ReplayRecording {
            identity: identity.clone(),
            body: body.to_owned(),
            request_sha256: Some("A".repeat(64)),
        };
        assert_eq!(recording.validate(), Ok(()));

        let mut wrong_length = recording.clone();
        wrong_length.identity.body_bytes += 1;
        assert_eq!(
            wrong_length.validate(),
            Err(ReplayRecordingValidationError::BodyBytesMismatch)
        );

        let mut wrong_digest = recording.clone();
        wrong_digest.identity.body_sha256 = "0".repeat(64);
        assert_eq!(
            wrong_digest.validate(),
            Err(ReplayRecordingValidationError::BodyDigestMismatch)
        );

        let mut wrong_request = recording;
        wrong_request.request_sha256 = Some("not-a-digest".to_owned());
        assert_eq!(
            wrong_request.validate(),
            Err(ReplayRecordingValidationError::RequestDigestInvalid)
        );

        let oversized_body = "x".repeat(MAX_RETAINED_REPLAY_BODY_BYTES + 1);
        let mut oversized_identity = base_identity();
        oversized_identity.body_bytes = oversized_body.len() as u64;
        oversized_identity.body_sha256 =
            crate::identity::sha256_hex(oversized_body.as_bytes());
        let oversized = ReplayRecording {
            identity: oversized_identity,
            body: oversized_body,
            request_sha256: None,
        };
        assert_eq!(
            oversized.validate(),
            Err(ReplayRecordingValidationError::BodyTooLarge)
        );
    }

    #[test]
    fn digest_rejects_malformed_identity_at_the_public_boundary() {
        let mut identity = base_identity();
        identity.body_sha256 = "short".to_owned();
        assert!(compute_provider_response_identity_digest(&identity).is_err());
        assert!(!is_valid_replay_sha256("short"));
        assert!(is_valid_replay_sha256(&"a".repeat(64)));
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
    fn digest_is_case_insensitive_for_body_hash_spelling() {
        let identity = base_identity();
        let mut uppercase = identity.clone();
        uppercase.body_sha256 = identity.body_sha256.to_ascii_uppercase();
        assert_eq!(
            compute_provider_response_identity_digest(&identity)
                .expect("digest"),
            compute_provider_response_identity_digest(&uppercase)
                .expect("digest")
        );
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
        with_body.body_sha256 = "d".repeat(64);
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
            body_sha256: "a".repeat(64),
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
            body_sha256: "b".repeat(64),
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
    fn collecting_body_call_keeps_identity_but_truthfully_refuses_body_replay()
    {
        let recorder = CollectingReplayRecorder::new();
        let mut identity = base_identity();
        let body = "body";
        identity.body_sha256 = crate::identity::sha256_hex(body.as_bytes());
        identity.body_bytes = body.len() as u64;
        assert!(
            !recorder.try_record_provider_response_with_body(&identity, body)
        );
        let snapshot = recorder.records_snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].0, identity);
        assert_eq!(snapshot[0].1.len(), 64);
    }

    #[test]
    fn route_bound_body_digest_is_canonicalized_and_legacy_method_stays_unbound()
     {
        use super::RetainingReplayRecorder;
        let recorder = RetainingReplayRecorder::new();
        let body = "body";
        let mut identity = base_identity();
        identity.body_sha256 = crate::identity::sha256_hex(body.as_bytes());
        identity.body_bytes = body.len() as u64;
        let request = "ABCDEF0123456789".repeat(4);
        assert!(recorder.try_record_provider_response_with_body_and_request(
            &identity, body, &request,
        ));
        let snapshot = recorder.records_snapshot();
        assert_eq!(
            snapshot[0].request_sha256,
            Some(request.to_ascii_lowercase())
        );

        let legacy = RetainingReplayRecorder::new();
        assert!(
            legacy.try_record_provider_response_with_body(&identity, body)
        );
        assert_eq!(legacy.records_snapshot()[0].request_sha256, None);
    }

    #[test]
    fn request_aware_default_does_not_overclaim_legacy_body_retention() {
        #[derive(Debug, Default)]
        struct LegacyBodyRecorder {
            calls: std::cell::RefCell<usize>,
        }

        impl ReplayRecorder for LegacyBodyRecorder {
            fn record_provider_response(
                &self,
                _identity: &ProviderResponseIdentity,
            ) {
            }

            fn record_provider_response_with_body(
                &self,
                _identity: &ProviderResponseIdentity,
                _body: &str,
            ) {
                *self.calls.borrow_mut() += 1;
            }

            fn is_recording(&self) -> bool {
                true
            }
        }

        let recorder = LegacyBodyRecorder::default();
        let body = "body";
        let mut identity = base_identity();
        identity.body_sha256 = crate::identity::sha256_hex(body.as_bytes());
        identity.body_bytes = body.len() as u64;
        recorder.record_provider_response_with_body(&identity, body);
        assert!(!recorder.try_record_provider_response_with_body_and_request(
            &identity,
            body,
            &"A".repeat(64),
        ));
        assert_eq!(*recorder.calls.borrow(), 2);
    }

    #[test]
    fn invalid_identity_or_request_does_not_consume_replay_capacity() {
        use super::RetainingReplayRecorder;
        let recorder = RetainingReplayRecorder::new();
        let body = "body";
        let mut identity = base_identity();
        identity.body_sha256 = crate::identity::sha256_hex(body.as_bytes());
        identity.body_bytes = body.len() as u64;

        let mut invalid_identity = identity.clone();
        invalid_identity.provider_id = "bad\nprovider".to_owned();
        assert!(
            !recorder.try_record_provider_response_with_body(
                &invalid_identity,
                body,
            )
        );
        assert!(recorder.records_snapshot().is_empty());
        assert!(recorder.is_recording());

        assert!(!recorder.try_record_provider_response_with_body_and_request(
            &identity,
            body,
            "not-a-request-digest",
        ));
        assert!(recorder.records_snapshot().is_empty());
        assert!(recorder.is_recording());

        assert!(
            recorder.try_record_provider_response_with_body(&identity, body,)
        );
        assert_eq!(recorder.records_snapshot().len(), 1);
    }

    #[test]
    fn retaining_recorder_upgrades_the_last_identity_only_record_at_capacity()
    {
        use super::{MAX_RETAINED_REPLAY_RECORDINGS, RetainingReplayRecorder};
        let recorder = RetainingReplayRecorder::new();
        let body = "body";
        let mut identities =
            Vec::with_capacity(MAX_RETAINED_REPLAY_RECORDINGS);
        for index in 0..MAX_RETAINED_REPLAY_RECORDINGS {
            let mut identity = base_identity();
            identity.provider_id = format!("provider-{index}");
            identity.body_sha256 =
                crate::identity::sha256_hex(body.as_bytes());
            identity.body_bytes = body.len() as u64;
            recorder.record_provider_response(&identity);
            identities.push(identity);
        }
        assert_eq!(
            recorder.records_snapshot().len(),
            MAX_RETAINED_REPLAY_RECORDINGS
        );
        assert!(recorder.try_record_provider_response_with_body(
            &identities[MAX_RETAINED_REPLAY_RECORDINGS - 1],
            body,
        ));
        let snapshot = recorder.records_snapshot();
        assert_eq!(snapshot.len(), MAX_RETAINED_REPLAY_RECORDINGS);
        assert_eq!(snapshot.last().expect("last").body, body);
    }

    #[test]
    fn terminal_evidence_does_not_consume_replay_capacity() {
        use super::{MAX_RETAINED_REPLAY_RECORDINGS, RetainingReplayRecorder};
        let recorder = RetainingReplayRecorder::new();
        for index in 0..(MAX_RETAINED_REPLAY_RECORDINGS - 1) {
            let mut identity = base_identity();
            identity.provider_id = format!("provider-{index}");
            recorder.record_provider_response(&identity);
        }
        let body = "terminal evidence";
        let mut terminal = base_identity();
        terminal.provider_id = "terminal-provider".to_owned();
        terminal.status = Some(500);
        terminal.body_sha256 = crate::identity::sha256_hex(body.as_bytes());
        terminal.body_bytes = body.len() as u64;
        assert!(recorder.try_record_provider_response_with_body_as_evidence(
            &terminal, body,
        ));
        let valid_body = "successful replay";
        let mut valid = base_identity();
        valid.provider_id = "valid-provider".to_owned();
        valid.body_sha256 = crate::identity::sha256_hex(valid_body.as_bytes());
        valid.body_bytes = valid_body.len() as u64;
        assert!(
            recorder
                .try_record_provider_response_with_body(&valid, valid_body)
        );
        assert!(recorder.is_recording());
        assert_eq!(recorder.records_snapshot().len(), 65);
    }

    #[test]
    fn oversized_terminal_evidence_retains_identity_without_replay_bytes() {
        use super::RetainingReplayRecorder;
        let recorder = RetainingReplayRecorder::new();
        let body = "x".repeat(MAX_RETAINED_REPLAY_BODY_BYTES + 1);
        let mut identity = base_identity();
        identity.status = Some(503);
        identity.body_bytes = body.len() as u64;
        identity.body_sha256 = crate::identity::sha256_hex(body.as_bytes());
        assert!(!recorder.try_record_provider_response_with_body_as_evidence(
            &identity, &body,
        ));
        let snapshot = recorder.records_snapshot();
        assert_eq!(snapshot.len(), 1);
        assert!(snapshot[0].body.is_empty());
        assert_eq!(snapshot[0].identity, identity);
        assert_eq!(*recorder.total_body_bytes.borrow(), 0);
    }

    #[test]
    fn evidence_byte_cap_retains_identity_without_a_body() {
        use super::RetainingReplayRecorder;
        let recorder = RetainingReplayRecorder::new();
        let first_body = "x".repeat(MAX_RETAINED_EVIDENCE_TOTAL_BYTES);
        let mut first = base_identity();
        first.status = Some(500);
        first.body_sha256 = crate::identity::sha256_hex(first_body.as_bytes());
        first.body_bytes = first_body.len() as u64;
        assert!(recorder.try_record_provider_response_with_body_as_evidence(
            &first,
            &first_body,
        ));
        let body = "tail";
        let mut second = base_identity();
        second.status = Some(503);
        second.body_sha256 = crate::identity::sha256_hex(body.as_bytes());
        second.body_bytes = body.len() as u64;
        assert!(!recorder.try_record_provider_response_with_body_as_evidence(
            &second, body,
        ));
        let snapshot = recorder.records_snapshot();
        assert_eq!(snapshot.len(), 2);
        assert!(snapshot[1].body.is_empty());
    }

    #[test]
    fn evidence_placeholder_is_not_upgraded_into_a_replay_body() {
        use super::RetainingReplayRecorder;
        let recorder = RetainingReplayRecorder::new();
        let body = "body";
        let mut identity = base_identity();
        identity.body_bytes = body.len() as u64;
        identity.body_sha256 = "0".repeat(64);
        assert!(!recorder.try_record_provider_response_with_body_as_evidence(
            &identity, body,
        ));
        identity.body_sha256 = crate::identity::sha256_hex(body.as_bytes());
        assert!(
            recorder.try_record_provider_response_with_body(&identity, body)
        );
        let snapshot = recorder.records_snapshot();
        assert_eq!(snapshot.len(), 2);
        assert!(snapshot[0].body.is_empty());
        assert_eq!(snapshot[1].body, body);
    }

    #[test]
    fn evidence_body_is_excluded_from_replayable_snapshot() {
        use super::RetainingReplayRecorder;
        let recorder = RetainingReplayRecorder::new();
        let body = "shape-valid terminal evidence";
        let mut identity = base_identity();
        identity.status = Some(200);
        identity.body_sha256 = crate::identity::sha256_hex(body.as_bytes());
        identity.body_bytes = body.len() as u64;
        assert!(recorder.try_record_provider_response_with_body_as_evidence(
            &identity, body,
        ));
        assert_eq!(recorder.records_snapshot().len(), 1);
        assert!(recorder.replayable_records_snapshot().is_empty());
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
        let mut identity = base_identity();
        let body = "hello body";
        identity.body_sha256 = crate::identity::sha256_hex(body.as_bytes());
        identity.body_bytes = body.len() as u64;
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
    fn retaining_recorder_base_record_retains_identity_only() {
        use super::RetainingReplayRecorder;
        let recorder = RetainingReplayRecorder::new();
        let identity = base_identity();
        recorder.record_provider_response(&identity);
        let identity_only = recorder.records_snapshot();
        assert_eq!(identity_only.len(), 1);
        assert!(identity_only[0].body.is_empty());
        // A later body-bearing call upgrades that same response in place.
        recorder.record_provider_response_with_body(&identity, "body");
        let snapshot = recorder.records_snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].body, "body");
    }

    #[test]
    fn rejected_body_uses_evidence_budget_and_does_not_starve_replay() {
        use super::RetainingReplayRecorder;
        let recorder = RetainingReplayRecorder::new();
        let body = "valid";
        let mut invalid = base_identity();
        invalid.body_sha256 = "0".repeat(64);
        invalid.body_bytes = body.len() as u64;
        recorder.record_provider_response(&invalid);
        assert!(
            !recorder.try_record_provider_response_with_body(&invalid, body,)
        );
        assert!(recorder.is_recording());
        assert!(recorder.replayable_records_snapshot().is_empty());
        let mut valid = base_identity();
        valid.provider_id = "valid-provider".to_owned();
        valid.body_sha256 = crate::identity::sha256_hex(body.as_bytes());
        valid.body_bytes = body.len() as u64;
        assert!(
            recorder.try_record_provider_response_with_body(&valid, body,)
        );
        let replayable = recorder.replayable_records_snapshot();
        assert_eq!(replayable.len(), 1);
        assert_eq!(replayable[0].identity.provider_id, "valid-provider");
    }

    #[test]
    fn empty_body_is_evidence_only_and_does_not_enter_replay() {
        use super::RetainingReplayRecorder;
        let recorder = RetainingReplayRecorder::new();
        let body = "";
        let mut identity = base_identity();
        identity.body_sha256 = crate::identity::sha256_hex(body.as_bytes());
        identity.body_bytes = 0;
        assert!(
            !recorder.try_record_provider_response_with_body(&identity, body,)
        );
        let snapshot = recorder.records_snapshot();
        assert_eq!(snapshot.len(), 1);
        assert!(snapshot[0].body.is_empty());
        assert!(recorder.replayable_records_snapshot().is_empty());
    }

    #[test]
    fn non_last_placeholder_is_reclassified_without_starving_replay() {
        use super::RetainingReplayRecorder;
        let recorder = RetainingReplayRecorder::new();
        let body = "valid";
        let mut target = base_identity();
        target.provider_id = "target".to_owned();
        target.body_sha256 = crate::identity::sha256_hex(body.as_bytes());
        target.body_bytes = body.len() as u64;
        recorder.record_provider_response(&target);
        let mut other = base_identity();
        other.provider_id = "other".to_owned();
        other.body_sha256 = crate::identity::sha256_hex(body.as_bytes());
        other.body_bytes = body.len() as u64;
        assert!(
            recorder.try_record_provider_response_with_body(&other, body,)
        );
        let mut invalid = target.clone();
        invalid.body_sha256 = "0".repeat(64);
        assert!(
            !recorder.try_record_provider_response_with_body(&invalid, body,)
        );
        let replayable = recorder.replayable_records_snapshot();
        assert_eq!(replayable.len(), 1);
        assert_eq!(replayable[0].identity.provider_id, "other");
        // The placeholder, the one valid body, and the terminal evidence entry
        // for the identity whose declared digest did not match its body.
        assert_eq!(recorder.records_snapshot().len(), 3);
    }

    #[test]
    fn capacity_rejection_reclassifies_a_pending_identity_as_evidence() {
        use super::{
            MAX_RETAINED_REPLAY_RECORDINGS, MAX_RETAINED_REPLAY_TOTAL_BYTES,
            RetainingReplayRecorder,
        };
        let recorder = RetainingReplayRecorder::new();
        // 63 retained bodies must still fit the aggregate replay budget, so one
        // more body is the first that cannot.
        let body = "x".repeat(33_000);
        for index in 0..(MAX_RETAINED_REPLAY_RECORDINGS - 1) {
            let mut identity = base_identity();
            identity.provider_id = format!("provider-{index}");
            identity.body_sha256 =
                crate::identity::sha256_hex(body.as_bytes());
            identity.body_bytes = body.len() as u64;
            assert!(
                recorder
                    .try_record_provider_response_with_body(&identity, &body,)
            );
        }
        let mut target = base_identity();
        target.provider_id = "target".to_owned();
        target.body_sha256 = crate::identity::sha256_hex(body.as_bytes());
        target.body_bytes = body.len() as u64;
        recorder.record_provider_response(&target);
        assert!(
            *recorder.total_body_bytes.borrow() + body.len()
                > MAX_RETAINED_REPLAY_TOTAL_BYTES
        );
        assert!(
            !recorder.try_record_provider_response_with_body(&target, &body,)
        );
        assert_eq!(recorder.replayable_records_snapshot().len(), 63);
        assert_eq!(recorder.records_snapshot().len(), 64);
        assert_eq!(*recorder.evidence_record_count.borrow(), 1);
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
