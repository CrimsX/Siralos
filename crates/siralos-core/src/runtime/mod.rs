//! Runtime readiness & operational resilience contracts (Stage 3 — H3,
//! ADR 0031; Stage 3R R10c).
//!
//! Mirrors `packages/core/src/runtime/**`: causal run identity,
//! manifest-bound budgets with typed resource limits, the deterministic
//! supervisor lifecycle and failure taxonomy, the harness-owned fault
//! injection driver, and fail-closed readiness evaluation. Everything
//! modeled here is host-owned description and decision semantics; no
//! process is ever launched (that remains R11/fail-closed).
//!
//! Submodules:
//! - [`artifacts`] — artifact references, budgets, and admission
//! - [`budget`] — RuntimeBudget, cancellation, restart reconciliation
//! - [`doctor`] — runtime readiness doctor surface
//! - [`faults`] — deterministic fake-process fault scripts
//! - [`identity`] — causal run/operation ids and trace refs
//! - [`readiness`] — fail-closed readiness manifests
//! - [`supervision`] — pure supervisor transitions and run outcomes
//!
//! Scope boundaries (decision 16): side-effect policies and run-owned
//! filesystem boundaries (`side-effects.ts`), RunManifest construction
//! and mode-capability evaluation (`modes.ts`), the artifact store /
//! cleanup planner / context projection (`artifacts.ts` store half), and
//! the activity log are not exercised by any wired differential subject
//! yet and stay unported.

pub mod artifacts;
pub mod budget;
pub mod doctor;
pub mod faults;
pub mod identity;
pub mod readiness;
pub mod supervision;

pub use artifacts::{
    ArtifactAdmission, ArtifactBudget, ArtifactBudgetState,
    DEFAULT_RUNTIME_ARTIFACT_BUDGET, RetentionClass, RuntimeArtifactKind,
    RuntimeArtifactRef, enforce_artifact_budget,
};
pub use budget::{
    CancellationPhase, CancellationState, IncompleteRunClassification,
    IncompleteRunRecord, RuntimeBudget, RuntimeBudgetInput,
    classify_incomplete_run, create_runtime_budget, default_runtime_budget,
    finalize_cancellation, render_cancellation_state,
    render_incomplete_run_classification, render_runtime_budget,
    request_cancellation,
};
pub use doctor::{DoctorCapabilities, build_runtime_readiness_diagnostic};
pub use faults::{
    FAULT_SCRIPTS, FaultScript, expected_failure_kind, list_fault_scripts,
    observe_fault_script,
};
pub use identity::{
    RunIdentityInput, RunTraceRef, create_operation_id, create_run_id,
    create_run_trace_ref, format_run_trace_ref,
};
pub use readiness::{
    RUNTIME_MODES, ReadinessItem, ReadinessItemId, RuntimeCapabilityState,
    RuntimeMode, RuntimeReadinessInput, RuntimeReadinessManifest,
    evaluate_runtime_readiness, execution_allowed, render_runtime_readiness,
};
pub use supervision::{
    CleanupStatus, LivenessKind, RUNTIME_FAILURE_KINDS, ResourceLimitKind,
    RunOutcome, RunOutcomeInput, RunTerminalStatus, RunTiming,
    RuntimeFailureKind, SupervisorObservation, SupervisorState,
    SupervisorStateView, create_run_outcome, initial_supervisor_state,
    is_supervisor_terminal, render_run_outcome, transition_supervisor,
};

/// Validation failure at a runtime-readiness boundary. The message
/// mirrors the TypeScript oracle exactly so both implementations reject
/// malformed inputs identically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError {
    /// Bounded truthful message.
    pub message: String,
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RuntimeError {}

pub(crate) fn runtime_error(message: impl Into<String>) -> RuntimeError {
    RuntimeError { message: message.into() }
}
