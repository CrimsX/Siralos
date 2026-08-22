//! R3 host-owned task kernel.
//!
//! Authoritative task contracts, materialized task state, lifecycle
//! transitions, bounded evidence, acceptance criteria, the completion
//! gate, terminal-state invariants, deterministic activity records, and
//! host-observed progress. This is a small Host-owned kernel, not a
//! workflow engine: steps are bounded host-authored units, evidence is
//! Host-observed structured data, and the model never owns task state.
//!
//! Planning, runtime snapshots, provider/workspace/domain surfaces, and
//! later evidence producers belong to their owning milestones and are
//! deliberately absent here (ADR 0036).

pub mod contract;
pub mod evidence;
pub mod identity;
pub mod model;
pub mod progress;
pub mod runtime;

#[cfg(test)]
mod property_tests;
#[cfg(test)]
mod tests;

pub use contract::{
    AcceptanceCriterion, ConstraintKind, ContractError,
    CreateTaskContractInput, PausePolicy, ReviseContext,
    ReviseTaskContractInput, TaskConstraint, TaskContract, VerificationKind,
    create_adhoc_task_contract,
};
pub use evidence::{
    EvidenceError, FindingError, FindingInput, normalize_iteration,
    source_supports_successful_outcome, validate_evidence_payload,
};
pub use model::{
    AcceptanceState, AcceptanceStatus, ActivityEvent, ApprovalDecision,
    DispositionSource, EvidenceKind, EvidenceRecord, EvidenceRef,
    EvidenceSource, EvidenceVerification, FindingRef, FindingSeverity,
    ProgressState, ProgressStateValue, TaskPhase, TaskReviewStatus, TaskState,
    TaskStepKind, TaskStepSpec, TaskStepState, TaskStepStatus,
    TaskValidationStatus, VerificationOutcome, WorkflowDisposition,
    is_terminal_phase,
};
pub use progress::{HostObservation, InternalProgress, observe_progress};
pub use runtime::{
    AttachRejection, AttachResult, Clock, CompletionEvaluation,
    CompletionResult, CreateTaskInput, CriterionResult, DispositionResult,
    OpError, ReviseError, StepOpResult, StepSpecError, TaskCreateError,
    TaskHandle, TaskId, TaskRuntime, ambient_clock,
};
