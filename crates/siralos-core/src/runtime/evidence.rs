//! Bounded structured runtime evidence (Stage 4.1, ADR 0031).
//!
//! Host-owned deterministic evidence projection for generic controlled
//! runtime execution. Large or unbounded capture (stdout/stderr) is never
//! granted authority: every buffer is bounded at 1 MiB, the `truncated`
//! flag is explicit, and the artifact digest is content identity (SHA-256
//! over the canonical bounded payload, `candidateRecordsSha256` style).
//! No wall clock, no ambient env, no filesystem access is consulted; the
//! projection is pure.

use std::collections::BTreeMap;

use crate::identity::{CanonicalValue, compute_artifact_digest, sha256_hex};

use super::{RuntimeError, runtime_error};

/// Maximum stdout capture in bytes (1 MiB).
pub const MAX_RUNTIME_EVIDENCE_STDOUT_BYTES: usize = 1024 * 1024;

/// Maximum stderr capture in bytes (1 MiB).
pub const MAX_RUNTIME_EVIDENCE_STDERR_BYTES: usize = 1024 * 1024;

/// Maximum combined evidence projection in bytes (bounded rendering only).
pub const MAX_RUNTIME_EVIDENCE_TOTAL_BYTES: usize = 2 * 1024 * 1024;

/// Maximum run-id length in bytes.
pub const MAX_EVIDENCE_RUN_ID_BYTES: usize = 128;

/// Maximum operation-id length in bytes.
pub const MAX_EVIDENCE_OPERATION_ID_BYTES: usize = 128;

/// Validated inputs for [`create_runtime_evidence`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeEvidenceInput {
    /// Owning run id (non-empty, bounded).
    pub run_id: String,
    /// Operation within the run (non-empty, bounded).
    pub operation_id: String,
    /// Exit code when one exists.
    pub exit_code: Option<i32>,
    /// Duration in milliseconds (controlled clock).
    pub duration_ms: u64,
    /// Captured stdout (UTF-8, bounded at 1 MiB, truncated with flag).
    pub stdout: String,
    /// Captured stderr (UTF-8, bounded at 1 MiB, truncated with flag).
    pub stderr: String,
}

/// Bounded structured runtime evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeEvidence {
    /// Owning run id.
    pub run_id: String,
    /// Operation within the run.
    pub operation_id: String,
    /// Exit code when one exists.
    pub exit_code: Option<i32>,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Bounded stdout (at most 1 MiB, scalar-safe truncation).
    pub stdout: String,
    /// Bounded stderr (at most 1 MiB, scalar-safe truncation).
    pub stderr: String,
    /// True when stdout or stderr was bounded.
    pub truncated: bool,
    /// Artifact digest over the bounded stdout+stderr content (64-hex).
    pub artifact_digest: String,
    /// Digest over the canonical `RuntimeEvidence v1` payload (64-hex).
    pub digest: String,
}

/// Truncate `text` to `max_bytes` UTF-8 bytes on scalar boundaries. Returns
/// the bounded text and whether truncation occurred. Deterministic and
/// never splits a scalar.
fn bound_text(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_owned(), false);
    }
    // Walk scalar boundaries to find the largest prefix that fits.
    let mut end = 0usize;
    for (index, ch) in text.char_indices() {
        let next = index + ch.len_utf8();
        if next > max_bytes {
            break;
        }
        end = next;
    }
    // `end` is on a scalar boundary; if no scalar fits, return empty truncated.
    let bounded = text[..end].to_owned();
    (bounded, true)
}

fn validate_evidence_input(
    input: &RuntimeEvidenceInput,
) -> Result<(), RuntimeError> {
    if input.run_id.is_empty() {
        return Err(runtime_error("A runtime evidence requires a run id."));
    }
    if input.run_id.len() > MAX_EVIDENCE_RUN_ID_BYTES {
        return Err(runtime_error(format!(
            "The runtime evidence run id exceeds the {MAX_EVIDENCE_RUN_ID_BYTES}-byte bound."
        )));
    }
    if input.operation_id.is_empty() {
        return Err(runtime_error(
            "A runtime evidence requires an operation id.",
        ));
    }
    if input.operation_id.len() > MAX_EVIDENCE_OPERATION_ID_BYTES {
        return Err(runtime_error(format!(
            "The runtime evidence operation id exceeds the {MAX_EVIDENCE_OPERATION_ID_BYTES}-byte bound."
        )));
    }
    Ok(())
}

fn canonical_string(value: &str) -> CanonicalValue {
    CanonicalValue::Str(value.to_owned())
}

/// Create validated bounded runtime evidence.
///
/// Truncation is explicit: when stdout or stderr exceeds 1 MiB, the stored
/// buffer is the scalar-safe prefix and `truncated` is true. The
/// `artifact_digest` is SHA-256 over the bounded stdout+stderr bytes;
/// `digest` is the domain-separated artifact digest over the canonical
/// `RuntimeEvidence v1` payload.
///
/// # Errors
///
/// Returns [`RuntimeError`] for malformed ids.
pub fn create_runtime_evidence(
    input: &RuntimeEvidenceInput,
) -> Result<RuntimeEvidence, RuntimeError> {
    validate_evidence_input(input)?;

    let (stdout, stdout_truncated) =
        bound_text(&input.stdout, MAX_RUNTIME_EVIDENCE_STDOUT_BYTES);
    let (stderr, stderr_truncated) =
        bound_text(&input.stderr, MAX_RUNTIME_EVIDENCE_STDERR_BYTES);
    let truncated = stdout_truncated || stderr_truncated;

    // Artifact digest: SHA-256 over the bounded capture bytes.
    let mut artifact_bytes =
        Vec::with_capacity(stdout.len() + 1 + stderr.len());
    artifact_bytes.extend_from_slice(stdout.as_bytes());
    artifact_bytes.push(b'\n');
    artifact_bytes.extend_from_slice(stderr.as_bytes());
    let artifact_digest = sha256_hex(&artifact_bytes);

    // Domain-separated digest over the canonical payload. `exitCode` is
    // stored as a string to keep `CanonicalValue` domain-neutral (no I64
    // variant); `Null` represents absence.
    let exit_code_value = match input.exit_code {
        Some(code) => canonical_string(&code.to_string()),
        None => CanonicalValue::Null,
    };
    let payload = CanonicalValue::Object(BTreeMap::from([
        ("runId".to_owned(), canonical_string(&input.run_id)),
        ("operationId".to_owned(), canonical_string(&input.operation_id)),
        ("exitCode".to_owned(), exit_code_value),
        ("durationMs".to_owned(), CanonicalValue::U64(input.duration_ms)),
        ("stdout".to_owned(), canonical_string(&stdout)),
        ("stderr".to_owned(), canonical_string(&stderr)),
        ("truncated".to_owned(), CanonicalValue::Bool(truncated)),
        ("artifactDigest".to_owned(), canonical_string(&artifact_digest)),
    ]));
    let digest = compute_artifact_digest("RuntimeEvidence", 1, &payload)
        .map_err(|error| runtime_error(error.message))?
        .value;

    Ok(RuntimeEvidence {
        run_id: input.run_id.clone(),
        operation_id: input.operation_id.clone(),
        exit_code: input.exit_code,
        duration_ms: input.duration_ms,
        stdout,
        stderr,
        truncated,
        artifact_digest,
        digest,
    })
}

/// Bounded human-readable evidence projection (deterministic, redacted only
/// by the terminal sanitizer).
#[must_use]
pub fn render_runtime_evidence(evidence: &RuntimeEvidence) -> String {
    let exit = match evidence.exit_code {
        Some(code) => code.to_string(),
        None => "n/a".to_owned(),
    };
    format!(
        "run {} op {} exit={} duration={}ms truncated={} digest={}",
        evidence.run_id,
        evidence.operation_id,
        exit,
        evidence.duration_ms,
        evidence.truncated,
        &evidence.artifact_digest[..12]
    )
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_RUNTIME_EVIDENCE_STDERR_BYTES, MAX_RUNTIME_EVIDENCE_STDOUT_BYTES,
        RuntimeEvidenceInput, create_runtime_evidence,
        render_runtime_evidence,
    };
    use crate::runtime::RuntimeError;

    fn valid_input() -> RuntimeEvidenceInput {
        RuntimeEvidenceInput {
            run_id: "run_runtime_abc".to_owned(),
            operation_id: "op_123".to_owned(),
            exit_code: Some(0),
            duration_ms: 1500,
            stdout: "hello\n".to_owned(),
            stderr: String::new(),
        }
    }

    #[test]
    fn creates_bounded_evidence_with_deterministic_digests() {
        let first = create_runtime_evidence(&valid_input()).expect("valid");
        assert!(!first.truncated);
        assert_eq!(first.artifact_digest.len(), 64);
        assert_eq!(first.digest.len(), 64);
        let rendered = render_runtime_evidence(&first);
        assert!(rendered.starts_with(
            "run run_runtime_abc op op_123 exit=0 duration=1500ms truncated=false digest="
        ));
        assert!(rendered.ends_with(&first.artifact_digest[..12]));
        let again = create_runtime_evidence(&valid_input()).expect("valid");
        assert_eq!(first.digest, again.digest);
        assert_eq!(first.artifact_digest, again.artifact_digest);

        // Different exit code changes the digest.
        let mut other = valid_input();
        other.exit_code = Some(1);
        let second = create_runtime_evidence(&other).expect("valid");
        assert_ne!(first.digest, second.digest);
    }

    #[test]
    fn truncates_stdout_and_stderr_at_one_mib_with_explicit_flag() {
        let mut input = valid_input();
        input.stdout = "a".repeat(MAX_RUNTIME_EVIDENCE_STDOUT_BYTES + 10);
        input.stderr = "b".repeat(MAX_RUNTIME_EVIDENCE_STDERR_BYTES + 5);
        let evidence = create_runtime_evidence(&input).expect("valid");
        assert!(evidence.truncated);
        assert_eq!(evidence.stdout.len(), MAX_RUNTIME_EVIDENCE_STDOUT_BYTES);
        assert_eq!(evidence.stderr.len(), MAX_RUNTIME_EVIDENCE_STDERR_BYTES);
        assert!(evidence.stdout.chars().all(|ch| ch == 'a'));
        assert!(render_runtime_evidence(&evidence).contains("truncated=true"));

        // Scalar-safe truncation: emoji (4 bytes) is never split.
        let mut input = valid_input();
        input.stdout = format!(
            "{}{}",
            "a".repeat(MAX_RUNTIME_EVIDENCE_STDOUT_BYTES - 2),
            "\u{1F600}"
        );
        // The emoji pushes over the bound, so it is dropped entirely.
        let evidence = create_runtime_evidence(&input).expect("valid");
        if evidence.stdout.len() == MAX_RUNTIME_EVIDENCE_STDOUT_BYTES {
            assert!(evidence.stdout.ends_with('a'));
        } else {
            assert!(evidence.stdout.len() < MAX_RUNTIME_EVIDENCE_STDOUT_BYTES);
        }
        assert!(evidence.truncated);
    }

    #[test]
    fn rejects_empty_ids_with_oracle_messages() {
        let mut input = valid_input();
        input.run_id = String::new();
        assert_eq!(
            create_runtime_evidence(&input).unwrap_err(),
            RuntimeError {
                message: "A runtime evidence requires a run id.".to_owned()
            }
        );
        let mut input = valid_input();
        input.operation_id = String::new();
        assert_eq!(
            create_runtime_evidence(&input).unwrap_err(),
            RuntimeError {
                message: "A runtime evidence requires an operation id."
                    .to_owned()
            }
        );
        let mut input = valid_input();
        input.exit_code = None;
        let evidence = create_runtime_evidence(&input).expect("valid");
        assert_eq!(evidence.exit_code, None);
        assert!(render_runtime_evidence(&evidence).contains("exit=n/a"));
    }
}
