//! Runtime artifact references and budget admission (Stage 3 — Runtime
//! Readiness & Operational Resilience, ADR 0031; R10c H3).
//!
//! Budget-admission subset of `packages/core/src/runtime/artifacts.ts`.
//! Artifacts are REFERENCE-ONLY; budgets behave deterministically at
//! limits — truncate with explicit metadata, stop capture, or produce an
//! artifact_limit outcome. Evidence is never silently dropped while
//! claiming complete capture. The artifact store, retention/cleanup
//! planner, and context projection are not exercised by any wired
//! differential subject yet and stay unported.

use super::RuntimeError;
use super::runtime_error;

/// The closed runtime-artifact kind vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeArtifactKind {
    /// Captured stdout.
    Stdout,
    /// Captured stderr.
    Stderr,
    /// Structured log stream.
    StructuredLog,
    /// Screenshot frame.
    Screenshot,
    /// Generic capture payload.
    Capture,
    /// Profiling data.
    Profile,
    /// Crash diagnostics.
    CrashInfo,
    /// Any other producer-specific kind.
    Other,
}

impl RuntimeArtifactKind {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
            Self::StructuredLog => "structured_log",
            Self::Screenshot => "screenshot",
            Self::Capture => "capture",
            Self::Profile => "profile",
            Self::CrashInfo => "crash_info",
            Self::Other => "other",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "stdout" => Some(Self::Stdout),
            "stderr" => Some(Self::Stderr),
            "structured_log" => Some(Self::StructuredLog),
            "screenshot" => Some(Self::Screenshot),
            "capture" => Some(Self::Capture),
            "profile" => Some(Self::Profile),
            "crash_info" => Some(Self::CrashInfo),
            "other" => Some(Self::Other),
            _ => None,
        }
    }
}

/// How long an artifact is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionClass {
    /// Dropped at cleanup.
    Ephemeral,
    /// Kept for the task lifetime.
    Task,
    /// Kept as diagnostics evidence.
    Diagnostic,
    /// Explicitly retained.
    Retained,
}

impl RetentionClass {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ephemeral => "ephemeral",
            Self::Task => "task",
            Self::Diagnostic => "diagnostic",
            Self::Retained => "retained",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ephemeral" => Some(Self::Ephemeral),
            "task" => Some(Self::Task),
            "diagnostic" => Some(Self::Diagnostic),
            "retained" => Some(Self::Retained),
            _ => None,
        }
    }
}

/// Reference-only descriptor of one captured runtime artifact (H1
/// semantics: the digest is identity, never trust).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeArtifactRef {
    /// Stable artifact id.
    pub id: String,
    /// 64-hex content digest.
    pub digest: String,
    /// Owning run id.
    pub run_id: String,
    /// Artifact kind.
    pub kind: RuntimeArtifactKind,
    /// Media type of the captured content.
    pub media_type: String,
    /// Exact size in bytes.
    pub size: u64,
    /// Producer identity.
    pub producer: String,
    /// Controlled-clock creation time.
    pub created_at_ms: u64,
    /// Retention class.
    pub retention_class: RetentionClass,
    /// Host-resolved location (never model-supplied).
    pub location: String,
    /// True when truncated at a budget limit (explicit).
    pub truncated: bool,
}

/// Validated inputs for [`RuntimeArtifactRef`] construction. Fields
/// mirror [`RuntimeArtifactRef`]; `retention_class` defaults to
/// ephemeral and `truncated` to false when absent.
pub struct RuntimeArtifactRefInput {
    /// Stable artifact id.
    pub id: String,
    /// 64-hex content digest.
    pub digest: String,
    /// Owning run id.
    pub run_id: String,
    /// Artifact kind protocol string.
    pub kind: String,
    /// Media type.
    pub media_type: String,
    /// Exact size in bytes.
    pub size: u64,
    /// Producer identity.
    pub producer: String,
    /// Controlled-clock creation time.
    pub created_at_ms: u64,
    /// Retention class protocol string.
    pub retention_class: Option<String>,
    /// Host-resolved location.
    pub location: String,
    /// Truncation marker.
    pub truncated: Option<bool>,
}

/// Create a validated artifact reference with oracle error messages.
pub fn create_runtime_artifact_ref(
    input: &RuntimeArtifactRefInput,
) -> Result<RuntimeArtifactRef, RuntimeError> {
    if input.id.is_empty() || input.location.is_empty() {
        return Err(runtime_error(
            "A runtime artifact requires an id and a location.",
        ));
    }
    let valid_digest = input.digest.len() == 64
        && input.digest.bytes().all(|byte| {
            byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
        });
    if !valid_digest {
        return Err(runtime_error(
            "A runtime artifact requires a 64-hex content digest.",
        ));
    }
    Ok(RuntimeArtifactRef {
        id: input.id.clone(),
        digest: input.digest.clone(),
        run_id: input.run_id.clone(),
        kind: RuntimeArtifactKind::parse(&input.kind)
            .unwrap_or(RuntimeArtifactKind::Other),
        media_type: input.media_type.clone(),
        size: input.size,
        producer: input.producer.clone(),
        created_at_ms: input.created_at_ms,
        retention_class: input
            .retention_class
            .as_deref()
            .and_then(RetentionClass::parse)
            .unwrap_or(RetentionClass::Ephemeral),
        location: input.location.clone(),
        truncated: input.truncated.unwrap_or(false),
    })
}

/// Per-run artifact budget limits (0 = unlimited for each bound).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtifactBudget {
    /// Maximum bytes of one artifact (0 = unlimited).
    pub max_artifact_bytes: u64,
    /// Maximum artifact count per run.
    pub max_artifacts_per_run: u64,
    /// Maximum aggregate artifact bytes per run.
    pub max_aggregate_bytes_per_run: u64,
    /// Maximum retained artifact bytes per task.
    pub max_retained_bytes_per_task: u64,
}

/// The default artifact budget (oracle `DEFAULT_RUNTIME_ARTIFACT_BUDGET`).
pub const DEFAULT_RUNTIME_ARTIFACT_BUDGET: ArtifactBudget = ArtifactBudget {
    max_artifact_bytes: 4 * 1024 * 1024,
    max_artifacts_per_run: 128,
    max_aggregate_bytes_per_run: 64 * 1024 * 1024,
    max_retained_bytes_per_task: 256 * 1024 * 1024,
};

/// Current store counters fed into admission decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ArtifactBudgetState {
    /// Artifacts admitted so far.
    pub artifact_count: u64,
    /// Aggregate bytes admitted so far.
    pub aggregate_bytes: u64,
}

/// Deterministic admission decision for one incoming artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactAdmission {
    /// Admit; truncated marks byte-limit bounding.
    Admit {
        /// True when the artifact was bounded at the byte limit.
        truncated: bool,
    },
    /// Deny with an explicit typed reason.
    Limit {
        /// The deterministic over-budget reason.
        reason: String,
    },
}

/// Deterministic budget enforcement: equivalent state + incoming
/// artifact produce the same decision. An exceeded limit yields an
/// explicit `artifact_limit`, never a silent drop.
#[must_use]
pub fn enforce_artifact_budget(
    budget: &ArtifactBudget,
    state: &ArtifactBudgetState,
    incoming_size: u64,
    incoming_count: u64,
) -> ArtifactAdmission {
    if budget.max_artifacts_per_run > 0
        && state.artifact_count + incoming_count > budget.max_artifacts_per_run
    {
        return ArtifactAdmission::Limit {
            reason: format!(
                "artifact count would exceed {} per run",
                budget.max_artifacts_per_run
            ),
        };
    }
    if budget.max_aggregate_bytes_per_run > 0
        && state.aggregate_bytes + incoming_size
            > budget.max_aggregate_bytes_per_run
    {
        return ArtifactAdmission::Limit {
            reason: format!(
                "aggregate artifact bytes would exceed {}",
                budget.max_aggregate_bytes_per_run
            ),
        };
    }
    if budget.max_artifact_bytes > 0
        && incoming_size > budget.max_artifact_bytes
    {
        // Truncate with explicit metadata: the artifact is admitted
        // bounded.
        return ArtifactAdmission::Admit { truncated: true };
    }
    ArtifactAdmission::Admit { truncated: false }
}

#[cfg(test)]
mod tests {
    use super::{
        ArtifactAdmission, ArtifactBudget, ArtifactBudgetState,
        DEFAULT_RUNTIME_ARTIFACT_BUDGET, RuntimeArtifactRefInput,
        create_runtime_artifact_ref, enforce_artifact_budget,
    };
    use crate::runtime::{RuntimeError, runtime_error};

    fn digest(character: char) -> String {
        std::iter::repeat_n(character, 64).collect()
    }

    #[test]
    fn admission_matrix_is_deterministic() {
        let budget = DEFAULT_RUNTIME_ARTIFACT_BUDGET;
        let empty = ArtifactBudgetState::default();
        assert_eq!(
            enforce_artifact_budget(&budget, &empty, 1024, 1),
            ArtifactAdmission::Admit { truncated: false }
        );
        assert_eq!(
            enforce_artifact_budget(
                &budget,
                &ArtifactBudgetState {
                    artifact_count: 128,
                    aggregate_bytes: 0,
                },
                10,
                1
            ),
            ArtifactAdmission::Limit {
                reason: "artifact count would exceed 128 per run".to_owned()
            }
        );
        assert_eq!(
            enforce_artifact_budget(
                &budget,
                &ArtifactBudgetState {
                    artifact_count: 0,
                    aggregate_bytes: 64 * 1024 * 1024 - 8,
                },
                16,
                1
            ),
            ArtifactAdmission::Limit {
                reason: format!(
                    "aggregate artifact bytes would exceed {}",
                    64 * 1024 * 1024
                )
            }
        );
        assert_eq!(
            enforce_artifact_budget(
                &budget,
                &empty,
                budget.max_artifact_bytes + 1,
                1
            ),
            ArtifactAdmission::Admit { truncated: true }
        );
        let zero = ArtifactBudget {
            max_artifact_bytes: 0,
            max_artifacts_per_run: 0,
            max_aggregate_bytes_per_run: 0,
            max_retained_bytes_per_task: 0,
        };
        assert_eq!(
            enforce_artifact_budget(
                &zero,
                &ArtifactBudgetState {
                    artifact_count: u64::MAX,
                    aggregate_bytes: u64::MAX
                },
                u64::MAX,
                u64::MAX
            ),
            ArtifactAdmission::Admit { truncated: false },
            "zero bounds are unlimited"
        );
    }

    #[test]
    fn artifact_refs_validate_with_oracle_messages() {
        let base = || RuntimeArtifactRefInput {
            id: "art-1".to_owned(),
            digest: digest('a'),
            run_id: "run_runtime_x".to_owned(),
            kind: "stdout".to_owned(),
            media_type: "text/plain".to_owned(),
            size: 128,
            producer: "capture".to_owned(),
            created_at_ms: 42,
            retention_class: None,
            location: "runs/r/art-1".to_owned(),
            truncated: None,
        };
        let reference =
            create_runtime_artifact_ref(&base()).expect("valid artifact ref");
        assert_eq!(reference.retention_class.as_str(), "ephemeral");
        assert!(!reference.truncated);
        let mut bad = base();
        bad.id = String::new();
        assert_eq!(
            create_runtime_artifact_ref(&bad),
            Err(runtime_error(
                "A runtime artifact requires an id and a location."
            ))
        );
        let mut bad = base();
        bad.digest = "XYZ".to_owned();
        assert_eq!(
            create_runtime_artifact_ref(&bad),
            Err(RuntimeError {
                message:
                    "A runtime artifact requires a 64-hex content digest."
                        .to_owned()
            })
        );
    }
}
