//! RuntimeBudget, cancellation, and restart reconciliation (Stage 3 —
//! Runtime Readiness & Operational Resilience, ADR 0031; R10c H3).
//!
//! Mirrors `packages/core/src/runtime/budget.ts`. Budgets list only
//! what the existing sandbox/backend can enforce or reliably observe;
//! unsupported limits (memory/CPU) are exposed as capability state,
//! never pretended enforced. Cancellation is deterministic and
//! idempotent; restart reconciliation classifies incomplete runs
//! conservatively.

use std::sync::OnceLock;

use crate::identity::{artifact_digest_hex, canonical_json_value};

/// Host-owned resource and observation budget for one run.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeBudget {
    /// Startup timeout in milliseconds.
    pub startup_timeout_ms: u64,
    /// Idle timeout in milliseconds.
    pub idle_timeout_ms: u64,
    /// Hard lifetime in milliseconds.
    pub hard_lifetime_ms: u64,
    /// Stdout capture limit in bytes.
    pub stdout_bytes: u64,
    /// Stderr capture limit in bytes.
    pub stderr_bytes: u64,
    /// Aggregate artifact byte budget.
    pub artifact_bytes: u64,
    /// Maximum artifacts per run.
    pub artifact_count: u64,
    /// Maximum child processes.
    pub child_process_count: u64,
    /// Memory limit ONLY when the backend can enforce/observe it.
    pub memory_mb: Option<u64>,
    /// CPU limit ONLY when the backend can enforce/observe it.
    pub cpu_percent: Option<f64>,
    /// Digest over the canonical `RuntimeBudget v1` payload.
    pub digest: String,
}

/// Optional overrides for [`create_runtime_budget`]; every field
/// defaults to the oracle value when absent.
#[derive(Debug, Clone, Default)]
pub struct RuntimeBudgetInput {
    /// Startup timeout override.
    pub startup_timeout_ms: Option<u64>,
    /// Idle timeout override.
    pub idle_timeout_ms: Option<u64>,
    /// Hard lifetime override.
    pub hard_lifetime_ms: Option<u64>,
    /// Stdout limit override.
    pub stdout_bytes: Option<u64>,
    /// Stderr limit override.
    pub stderr_bytes: Option<u64>,
    /// Artifact byte budget override.
    pub artifact_bytes: Option<u64>,
    /// Artifact count override.
    pub artifact_count: Option<u64>,
    /// Child process count override.
    pub child_process_count: Option<u64>,
    /// Memory limit override (`None` keeps it unenforced).
    pub memory_mb: Option<Option<u64>>,
    /// CPU limit override (`None` keeps it unenforced).
    pub cpu_percent: Option<Option<f64>>,
}

#[allow(clippy::too_many_arguments)]
fn create_budget_from_fields(
    startup_timeout_ms: u64,
    idle_timeout_ms: u64,
    hard_lifetime_ms: u64,
    stdout_bytes: u64,
    stderr_bytes: u64,
    artifact_bytes: u64,
    artifact_count: u64,
    child_process_count: u64,
    memory_mb: Option<u64>,
    cpu_percent: Option<f64>,
) -> RuntimeBudget {
    // The oracle digests the payload WITH the empty digest placeholder,
    // so the canonical payload carries "digest":"" exactly like the
    // TypeScript reference.
    let payload = serde_json::json!({
        "startupTimeoutMs": startup_timeout_ms,
        "idleTimeoutMs": idle_timeout_ms,
        "hardLifetimeMs": hard_lifetime_ms,
        "stdoutBytes": stdout_bytes,
        "stderrBytes": stderr_bytes,
        "artifactBytes": artifact_bytes,
        "artifactCount": artifact_count,
        "childProcessCount": child_process_count,
        "memoryMb": memory_mb,
        "cpuPercent": cpu_percent,
        "digest": "",
    });
    let digest = artifact_digest_hex(
        "RuntimeBudget",
        1,
        &canonical_json_value(&payload),
    );
    RuntimeBudget {
        startup_timeout_ms,
        idle_timeout_ms,
        hard_lifetime_ms,
        stdout_bytes,
        stderr_bytes,
        artifact_bytes,
        artifact_count,
        child_process_count,
        memory_mb,
        cpu_percent,
        digest,
    }
}

/// Create a runtime budget from optional overrides. Equivalent inputs
/// produce identical digests; unsupported limits stay `None`.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn create_runtime_budget(input: &RuntimeBudgetInput) -> RuntimeBudget {
    create_budget_from_fields(
        input.startup_timeout_ms.unwrap_or(15_000),
        input.idle_timeout_ms.unwrap_or(60_000),
        input.hard_lifetime_ms.unwrap_or(300_000),
        input.stdout_bytes.unwrap_or(4 * 1024 * 1024),
        input.stderr_bytes.unwrap_or(4 * 1024 * 1024),
        input.artifact_bytes.unwrap_or(64 * 1024 * 1024),
        input.artifact_count.unwrap_or(128),
        input.child_process_count.unwrap_or(4),
        input.memory_mb.unwrap_or_default(),
        input.cpu_percent.unwrap_or_default(),
    )
}

/// The default host budget (oracle `DEFAULT_RUNTIME_BUDGET`).
pub fn default_runtime_budget() -> &'static RuntimeBudget {
    static BUDGET: OnceLock<RuntimeBudget> = OnceLock::new();
    BUDGET
        .get_or_init(|| create_runtime_budget(&RuntimeBudgetInput::default()))
}

/// Bounded human-readable budget projection.
#[must_use]
pub fn render_runtime_budget(budget: &RuntimeBudget) -> String {
    let mut out = format!(
        "startup={}ms idle={}ms hard={}ms stdout={}B stderr={}B artifacts={}x{}B children={}",
        budget.startup_timeout_ms,
        budget.idle_timeout_ms,
        budget.hard_lifetime_ms,
        budget.stdout_bytes,
        budget.stderr_bytes,
        budget.artifact_count,
        budget.artifact_bytes,
        budget.child_process_count,
    );
    if let Some(memory) = budget.memory_mb {
        out.push_str(&format!(" mem={memory}MB"));
    }
    if let Some(cpu) = budget.cpu_percent {
        out.push_str(&format!(" cpu={cpu}%"));
    }
    out
}

// ---------------------------------------------------------------------------
// Cancellation
// ---------------------------------------------------------------------------

/// Cancellation lifecycle phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancellationPhase {
    /// No cancellation requested.
    None,
    /// Cancellation requested; cleanup flow allocated once.
    Requested,
    /// Cancellation finalized.
    Finalized,
}

impl CancellationPhase {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Requested => "requested",
            Self::Finalized => "finalized",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "requested" => Some(Self::Requested),
            "finalized" => Some(Self::Finalized),
            _ => None,
        }
    }
}

/// Deterministic cancellation state; repeated requests never duplicate
/// the single cleanup flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancellationState {
    /// Lifecycle phase.
    pub phase: CancellationPhase,
    /// Single cleanup flow identity.
    pub cleanup_flow_id: Option<String>,
    /// Controlled-clock time of the request.
    pub requested_at_ms: Option<u64>,
}

/// Outcome of one cancellation request.
pub struct CancellationRequest {
    /// Next state.
    pub state: CancellationState,
    /// True when the request repeated an existing cancellation and the
    /// SAME flow was returned unchanged.
    pub idempotent: bool,
}

/// Request cancellation: the first request allocates
/// `cleanup_<runId>`; later requests return the same flow untouched.
pub fn request_cancellation(
    state: &CancellationState,
    run_id: &str,
    now_ms: u64,
) -> CancellationRequest {
    if state.phase != CancellationPhase::None {
        return CancellationRequest { state: state.clone(), idempotent: true };
    }
    CancellationRequest {
        state: CancellationState {
            phase: CancellationPhase::Requested,
            cleanup_flow_id: Some(format!("cleanup_{run_id}")),
            requested_at_ms: Some(now_ms),
        },
        idempotent: false,
    }
}

/// Finalize an existing cancellation request.
#[must_use]
pub fn finalize_cancellation(state: &CancellationState) -> CancellationState {
    CancellationState {
        phase: CancellationPhase::Finalized,
        cleanup_flow_id: state.cleanup_flow_id.clone(),
        requested_at_ms: state.requested_at_ms,
    }
}

/// Deterministic cancellation semantics checklist (projection).
#[must_use]
pub fn render_cancellation_state(state: &CancellationState) -> String {
    match &state.cleanup_flow_id {
        Some(flow) => {
            format!("cancellation={} flow={flow}", state.phase.as_str())
        }
        None => format!("cancellation={}", state.phase.as_str()),
    }
}

// ---------------------------------------------------------------------------
// Restart reconciliation
// ---------------------------------------------------------------------------

/// Conservative classification of a run left non-terminal by a restart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncompleteRunClassification {
    /// The run never started.
    Interrupted,
    /// No reliable process observation exists.
    Unknown,
    /// Run-owned state may exist and requires cleanup first.
    CleanupRequired,
}

impl IncompleteRunClassification {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Interrupted => "interrupted",
            Self::Unknown => "unknown",
            Self::CleanupRequired => "cleanup_required",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "interrupted" => Some(Self::Interrupted),
            "unknown" => Some(Self::Unknown),
            "cleanup_required" => Some(Self::CleanupRequired),
            _ => None,
        }
    }
}

/// One incomplete-run record observed after a Siralos restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncompleteRunRecord {
    /// The interrupted run.
    pub run_id: String,
    /// Last known supervisor state label.
    pub last_known_state: String,
    /// Controlled-clock time of the last observation.
    pub last_observed_at_ms: u64,
}

/// Classification result with its conservative reason.
pub struct IncompleteRunResult {
    /// The classification.
    pub classification: IncompleteRunClassification,
    /// Why the classification applies.
    pub reason: String,
}

/// Classify a run left non-terminal by a restart. We never assume the
/// external process is gone solely because Siralos restarted; without a
/// reliable process observation the record is `unknown`, with cleanup
/// required when run-owned state may exist.
pub fn classify_incomplete_run(
    record: &IncompleteRunRecord,
    run_state_may_exist: bool,
) -> IncompleteRunResult {
    if record.last_known_state == "prepared" {
        return IncompleteRunResult {
            classification: IncompleteRunClassification::Interrupted,
            reason:
                "The run never started; it was interrupted during preparation."
                    .to_owned(),
        };
    }
    if matches!(
        record.last_known_state.as_str(),
        "starting" | "running" | "terminating"
    ) {
        return IncompleteRunResult {
            classification: if run_state_may_exist {
                IncompleteRunClassification::CleanupRequired
            } else {
                IncompleteRunClassification::Unknown
            },
            reason: if run_state_may_exist {
                "Run-owned state may exist; conservative cleanup is required before any new run."
                    .to_owned()
            } else {
                "The external process state is unknown after restart; the run is classified unknown, never success."
                    .to_owned()
            },
        };
    }
    IncompleteRunResult {
        classification: IncompleteRunClassification::Unknown,
        reason:
            "The run record is not in a recognized active state; classified conservatively as unknown."
                .to_owned(),
    }
}

/// Rendered incomplete-run classification.
#[must_use]
pub fn render_incomplete_run_classification(
    classification: IncompleteRunClassification,
) -> String {
    format!("incomplete run classified: {}", classification.as_str())
}

#[cfg(test)]
mod tests {
    use super::{
        CancellationPhase, CancellationState, IncompleteRunClassification,
        IncompleteRunRecord, RuntimeBudgetInput, classify_incomplete_run,
        create_runtime_budget, default_runtime_budget, finalize_cancellation,
        render_cancellation_state, render_incomplete_run_classification,
        render_runtime_budget, request_cancellation,
    };

    #[test]
    fn default_budget_matches_the_oracle_projection() {
        let budget = default_runtime_budget();
        assert_eq!(
            render_runtime_budget(budget),
            "startup=15000ms idle=60000ms hard=300000ms stdout=4194304B \
             stderr=4194304B artifacts=128x67108864B children=4"
        );
        assert_eq!(budget.memory_mb, None);
        assert_eq!(budget.cpu_percent, None);
        assert_eq!(budget.digest.len(), 64);
    }

    #[test]
    fn custom_budgets_bind_memory_and_cpu_when_supported() {
        let budget = create_runtime_budget(&RuntimeBudgetInput {
            idle_timeout_ms: Some(5_000),
            memory_mb: Some(Some(2048)),
            cpu_percent: Some(Some(50.0)),
            ..RuntimeBudgetInput::default()
        });
        assert!(
            render_runtime_budget(&budget).ends_with(" mem=2048MB cpu=50%")
        );
        assert_ne!(budget.digest, default_runtime_budget().digest);
        let same = create_runtime_budget(&RuntimeBudgetInput {
            idle_timeout_ms: Some(5_000),
            memory_mb: Some(Some(2048)),
            cpu_percent: Some(Some(50.0)),
            ..RuntimeBudgetInput::default()
        });
        assert_eq!(budget.digest, same.digest);
    }

    #[test]
    fn cancellation_is_idempotent_and_finalizable() {
        let initial = CancellationState {
            phase: CancellationPhase::None,
            cleanup_flow_id: None,
            requested_at_ms: None,
        };
        let first = request_cancellation(&initial, "run_x", 100);
        assert!(!first.idempotent);
        assert_eq!(
            first.state.cleanup_flow_id,
            Some("cleanup_run_x".to_owned())
        );
        let second = request_cancellation(&first.state, "run_x", 200);
        assert!(second.idempotent);
        assert_eq!(second.state, first.state);
        let finalized = finalize_cancellation(&first.state);
        assert_eq!(finalized.phase, CancellationPhase::Finalized);
        assert_eq!(
            render_cancellation_state(&finalized),
            "cancellation=finalized flow=cleanup_run_x"
        );
        assert_eq!(render_cancellation_state(&initial), "cancellation=none");
    }

    #[test]
    fn incomplete_runs_classify_conservatively() {
        let prepared = IncompleteRunRecord {
            run_id: "r".to_owned(),
            last_known_state: "prepared".to_owned(),
            last_observed_at_ms: 10,
        };
        assert_eq!(
            classify_incomplete_run(&prepared, true).classification,
            IncompleteRunClassification::Interrupted
        );
        let running = IncompleteRunRecord {
            run_id: "r".to_owned(),
            last_known_state: "running".to_owned(),
            last_observed_at_ms: 10,
        };
        assert_eq!(
            classify_incomplete_run(&running, false).classification,
            IncompleteRunClassification::Unknown
        );
        assert_eq!(
            classify_incomplete_run(&running, true).classification,
            IncompleteRunClassification::CleanupRequired
        );
        let mystery = IncompleteRunRecord {
            run_id: "r".to_owned(),
            last_known_state: "mystery".to_owned(),
            last_observed_at_ms: 10,
        };
        let result = classify_incomplete_run(&mystery, true);
        assert_eq!(
            result.classification,
            IncompleteRunClassification::Unknown
        );
        assert_eq!(
            render_incomplete_run_classification(result.classification),
            "incomplete run classified: unknown"
        );
    }
}
