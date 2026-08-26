//! Authoritative task state model (Stage 3R R3).
//!
//! TaskState is the materialized, host-owned working state of one task.
//! It never stores private chain-of-thought, provider continuation
//! internals, or secrets; evidence references point at already-owned
//! artifacts instead of duplicating raw adapter output. The R3 subset
//! deliberately omits later-milestone surfaces (planning state, runtime
//! snapshots, reference/research evidence producers).

use crate::task::contract::VerificationKind;

/// Host-controlled task phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskPhase {
    /// Prepared.
    Prepared,
    /// Working.
    Working,
    /// Validating.
    Validating,
    /// Reviewing.
    Reviewing,
    /// Blocked.
    Blocked,
    /// Completed.
    Completed,
    /// Cancelled.
    Cancelled,
    /// Failed.
    Failed,
}

impl TaskPhase {
    /// Stable machine-readable form matching the TypeScript reference.
    pub fn as_str(self) -> &'static str {
        match self {
            TaskPhase::Prepared => "prepared",
            TaskPhase::Working => "working",
            TaskPhase::Validating => "validating",
            TaskPhase::Reviewing => "reviewing",
            TaskPhase::Blocked => "blocked",
            TaskPhase::Completed => "completed",
            TaskPhase::Cancelled => "cancelled",
            TaskPhase::Failed => "failed",
        }
    }
}

/// A terminal phase: authoritative state must not continue mutating
/// through ordinary task operations.
pub fn is_terminal_phase(phase: TaskPhase) -> bool {
    matches!(
        phase,
        TaskPhase::Completed | TaskPhase::Cancelled | TaskPhase::Failed
    )
}

/// Task step status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStepStatus {
    /// Pending.
    Pending,
    /// Active.
    Active,
    /// Completed.
    Completed,
    /// Failed.
    Failed,
    /// Blocked.
    Blocked,
}

impl TaskStepStatus {
    /// Stable machine-readable form matching the TypeScript reference.
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStepStatus::Pending => "pending",
            TaskStepStatus::Active => "active",
            TaskStepStatus::Completed => "completed",
            TaskStepStatus::Failed => "failed",
            TaskStepStatus::Blocked => "blocked",
        }
    }
}

/// Step kind drives the minimal extensible evidence rule boundary: each
/// step declares the evidence kinds it accepts, so completion never
/// hard-codes a mutation requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStepKind {
    /// Research.
    Research,
    /// Implementation.
    Implementation,
    /// Review.
    Review,
}

impl TaskStepKind {
    /// Stable machine-readable form matching the TypeScript reference.
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStepKind::Research => "research",
            TaskStepKind::Implementation => "implementation",
            TaskStepKind::Review => "review",
        }
    }
}

/// R3 evidence kinds: the minimum variants needed to prove the generic
/// evidence and acceptance behavior. Later milestones add their own
/// producers (workspace mutation, checkpoints, LSP, references, research,
/// native verification) without changing this host kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EvidenceKind {
    /// Workspace read.
    WorkspaceRead,
    /// Parser result.
    ParserResult,
    /// Validation result.
    ValidationResult,
    /// Review result.
    ReviewResult,
    /// User approval.
    UserApproval,
}

impl EvidenceKind {
    /// Stable machine-readable form matching the TypeScript reference.
    pub fn as_str(self) -> &'static str {
        match self {
            EvidenceKind::WorkspaceRead => "workspace_read",
            EvidenceKind::ParserResult => "parser_result",
            EvidenceKind::ValidationResult => "validation_result",
            EvidenceKind::ReviewResult => "review_result",
            EvidenceKind::UserApproval => "user_approval",
        }
    }
}

/// User-approval decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    /// Approved.
    Approved,
    /// Denied.
    Denied,
}

/// Bounded evidence source. Evidence points at already-owned artifacts
/// and never embeds raw adapter output such as full diagnostics or diffs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceSource {
    /// Bounded workspace read observation.
    /// Workspace read.
    WorkspaceRead {
        /// Inspected relative paths.
        paths: Vec<String>,
        /// Revision handle of the inspected file state, when known.
        revision: Option<String>,
    },
    /// Deterministic parse result counts.
    Parser {
        /// Files checked by the parser.
        checked_files: u64,
        /// Files that parsed validly.
        valid_files: u64,
        /// Files with errors.
        errors: u64,
    },
    /// Deterministic validation-gate result.
    Validation {
        /// Free-form outcome label recorded by the producer.
        outcome: String,
        /// Whether workspace integrity was verified.
        workspace_integrity_verified: bool,
        /// Unexpected-change count.
        unexpected_changes: u64,
    },
    /// Independent review result.
    Review {
        /// Review status label ("clean" for success).
        status: String,
        /// Blocking finding count.
        blocking_findings: u64,
    },
    /// Host-mediated user approval.
    UserApproval {
        /// Approval record id.
        approval_id: String,
        /// Subject the approval covers.
        subject_id: String,
        /// Approval decision.
        decision: ApprovalDecision,
    },
}

/// Verification outcome recorded by the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationOutcome {
    /// Passed.
    Passed,
    /// Failed.
    Failed,
    /// Incomplete.
    Incomplete,
}

/// Exact immutable milestone target of host-observed verification
/// evidence (Stage 3R R13.4 acceptance parity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MilestoneEvidenceTarget {
    /// Target manifest id.
    pub manifest_id: String,
    /// Target manifest version.
    pub manifest_version: u64,
    /// Target requirement id within the manifest.
    pub requirement_id: String,
}

/// Host-owned verification binding. The enclosing EvidenceRecord supplies
/// the task id and exact contract revision/digest; this value binds the
/// observation to one check and, when applicable, one task criterion
/// and/or milestone requirement.
/// Unbound evidence remains useful for task steps but cannot satisfy
/// acceptance. Milestone-requirement bindings arrive with their owning
/// milestone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceVerification {
    /// Check identity (validated pattern).
    /// Check id.
    pub check_id: String,
    /// Criterion this evidence verifies, when applicable.
    /// Criterion id.
    pub criterion_id: Option<String>,
    /// Milestone requirement this evidence targets, when applicable.
    pub milestone: Option<MilestoneEvidenceTarget>,
    /// Host-recorded outcome.
    /// Outcome.
    pub outcome: VerificationOutcome,
}

/// One accepted evidence record attached to a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRecord {
    /// Validated evidence id.
    /// Id.
    pub id: String,
    /// Evidence kind.
    /// Kind.
    pub kind: EvidenceKind,
    /// Owning task id.
    /// Task id.
    pub task_id: String,
    /// Exact TaskContract revision current when the host attached this.
    /// Task contract revision.
    pub task_contract_revision: u64,
    /// Exact TaskContract digest current when the host attached this.
    /// Task contract digest.
    pub task_contract_digest: String,
    /// Bounded structured source.
    /// Source.
    pub source: EvidenceSource,
    /// Acceptance verification binding, when the host recorded one.
    /// Verification.
    pub verification: Option<EvidenceVerification>,
    /// Host timestamp of the attachment.
    /// Attached at ms.
    pub attached_at_ms: i64,
}

/// A step's reference to already-attached evidence of its task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRef {
    /// Attached evidence id.
    /// Evidence id.
    pub evidence_id: String,
    /// Declared evidence kind.
    /// Kind.
    pub kind: EvidenceKind,
}

/// Bounded step specification authored by the host at task creation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStepSpec {
    /// Validated step id.
    /// Id.
    pub id: String,
    /// Step description.
    /// Description.
    pub description: String,
    /// Step kind.
    /// Kind.
    pub kind: TaskStepKind,
    /// Evidence kinds this step accepts; completion requires at least one
    /// reference of an accepted kind.
    /// Accepts.
    pub accepts: Vec<EvidenceKind>,
}

/// Materialized step state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStepState {
    /// Step id.
    /// Id.
    pub id: String,
    /// Step description.
    /// Description.
    pub description: String,
    /// Step kind.
    /// Kind.
    pub kind: TaskStepKind,
    /// Current step status.
    /// Status.
    pub status: TaskStepStatus,
    /// Evidence references backing completion.
    /// Evidence refs.
    pub evidence_refs: Vec<EvidenceRef>,
    /// Failure reason, when failed.
    /// Failed reason.
    pub failed_reason: Option<String>,
    /// Blocked reason, when blocked.
    /// Blocked reason.
    pub blocked_reason: Option<String>,
}

/// Acceptance criterion status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptanceStatus {
    /// Pending.
    Pending,
    /// Satisfied.
    Satisfied,
    /// Failed.
    Failed,
}

impl AcceptanceStatus {
    /// Stable machine-readable form matching the TypeScript reference.
    pub fn as_str(self) -> &'static str {
        match self {
            AcceptanceStatus::Pending => "pending",
            AcceptanceStatus::Satisfied => "satisfied",
            AcceptanceStatus::Failed => "failed",
        }
    }
}

/// Materialized acceptance state for one criterion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceState {
    /// Criterion id.
    /// Criterion id.
    pub criterion_id: String,
    /// Criterion description.
    /// Description.
    pub description: String,
    /// Verification kind.
    /// Verification kind.
    pub verification_kind: VerificationKind,
    /// Current status.
    /// Status.
    pub status: AcceptanceStatus,
    /// Evidence id that satisfied the criterion, when verified.
    /// Verified by.
    pub verified_by: Option<String>,
    /// Host note.
    /// Note.
    pub note: Option<String>,
}

/// Finding severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingSeverity {
    /// Critical.
    Critical,
    /// High.
    High,
    /// Medium.
    Medium,
    /// Low.
    Low,
}

impl FindingSeverity {
    /// Stable machine-readable form matching the TypeScript reference.
    pub fn as_str(self) -> &'static str {
        match self {
            FindingSeverity::Critical => "critical",
            FindingSeverity::High => "high",
            FindingSeverity::Medium => "medium",
            FindingSeverity::Low => "low",
        }
    }

    /// Whether this severity blocks completion (critical/high).
    pub fn is_blocking(self) -> bool {
        matches!(self, FindingSeverity::Critical | FindingSeverity::High)
    }
}

/// Evidence-backed finding reference; never hidden model reasoning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindingRef {
    /// Finding id.
    /// Finding id.
    pub finding_id: String,
    /// Severity.
    /// Severity.
    pub severity: FindingSeverity,
    /// Source label.
    /// Source.
    pub source: String,
}

/// Validation status of the task's deterministic gates. 'incomplete'
/// means a required gate could not run or was denied; it must never be
/// treated as success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskValidationStatus {
    /// Not run.
    NotRun,
    /// Clean.
    Clean,
    /// Warnings.
    Warnings,
    /// Failed.
    Failed,
    /// Incomplete.
    Incomplete,
}

impl TaskValidationStatus {
    /// Stable machine-readable form matching the TypeScript reference.
    pub fn as_str(self) -> &'static str {
        match self {
            TaskValidationStatus::NotRun => "not_run",
            TaskValidationStatus::Clean => "clean",
            TaskValidationStatus::Warnings => "warnings",
            TaskValidationStatus::Failed => "failed",
            TaskValidationStatus::Incomplete => "incomplete",
        }
    }

    /// Whether this status satisfies the completion gate.
    pub fn is_acceptable_for_completion(self) -> bool {
        matches!(
            self,
            TaskValidationStatus::Clean | TaskValidationStatus::Warnings
        )
    }
}

/// Review status of the task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskReviewStatus {
    /// Not run.
    NotRun,
    /// Clean.
    Clean,
    /// Findings.
    Findings,
    /// Incomplete.
    Incomplete,
}

impl TaskReviewStatus {
    /// Stable machine-readable form matching the TypeScript reference.
    pub fn as_str(self) -> &'static str {
        match self {
            TaskReviewStatus::NotRun => "not_run",
            TaskReviewStatus::Clean => "clean",
            TaskReviewStatus::Findings => "findings",
            TaskReviewStatus::Incomplete => "incomplete",
        }
    }
}

/// Host-observed progress state: distinguishes productive execution from
/// repeated no-progress loops. Progress is based on new host-observed
/// useful state, never merely another model turn or another identical
/// action with an identical result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressStateValue {
    /// Healthy.
    Healthy,
    /// Degraded.
    Degraded,
    /// Stalled.
    Stalled,
}

impl ProgressStateValue {
    /// Stable machine-readable form matching the TypeScript reference.
    pub fn as_str(self) -> &'static str {
        match self {
            ProgressStateValue::Healthy => "healthy",
            ProgressStateValue::Degraded => "degraded",
            ProgressStateValue::Stalled => "stalled",
        }
    }
}

/// Observable progress snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressState {
    /// Current classification.
    /// State.
    pub state: ProgressStateValue,
    /// Distinct useful observations accepted so far.
    /// Useful observations.
    pub useful_observations: u64,
    /// Current run of identical (action, result) observations.
    /// Repeated actions.
    pub repeated_actions: u64,
    /// When the last useful observation was accepted.
    /// Last progress at ms.
    pub last_progress_at_ms: Option<i64>,
    /// When the stalled state was first entered; None while not stalled.
    /// Stalled at ms.
    pub stalled_at_ms: Option<i64>,
}

/// Structured workflow disposition: a request, never an authoritative
/// mutation. 'complete' is a completion request that still goes through
/// the host completion gate; 'blocked' preserves a clear reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowDisposition {
    /// Continue with an optional next action.
    Continue {
        /// Optional next action hint.
        next_action: Option<String>,
    },
    /// Request completion; still subject to the host completion gate.
    Complete,
    /// Request a blocked state with a reason.
    Blocked {
        /// Reason for the blocked state.
        reason: String,
    },
}

impl WorkflowDisposition {
    /// Stable machine-readable form matching the TypeScript reference.
    pub fn type_str(&self) -> &'static str {
        match self {
            WorkflowDisposition::Continue { .. } => "continue",
            WorkflowDisposition::Complete => "complete",
            WorkflowDisposition::Blocked { .. } => "blocked",
        }
    }
}

/// Disposition submission source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispositionSource {
    /// Host.
    Host,
    /// Model.
    Model,
}

impl DispositionSource {
    /// Stable machine-readable form matching the TypeScript reference.
    pub fn as_str(self) -> &'static str {
        match self {
            DispositionSource::Host => "host",
            DispositionSource::Model => "model",
        }
    }
}

/// Typed append-only task activity records. This is NOT event sourcing:
/// the authoritative TaskState remains a materialized object, and the
/// activity log exists only for auditability, debugging, future
/// persistence, UI projection, and behavior tests. Records are immutable
/// after append, deterministically sequenced per task, host-timestamped,
/// and never carry secrets or hidden reasoning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivityEvent {
    /// Task creation.
    /// Task started.
    TaskStarted {
        /// Monotonic per-task sequence.
        sequence: u64,
        /// Contract revision at start.
        contract_revision: u64,
    },
    /// Host-controlled phase transition.
    TaskPhaseChanged {
        /// Monotonic per-task sequence.
        sequence: u64,
        /// New phase.
        phase: TaskPhase,
    },
    /// Step activation.
    StepStarted {
        /// Monotonic per-task sequence.
        sequence: u64,
        /// Step id.
        step_id: String,
    },
    /// Evidence-backed step completion.
    StepCompleted {
        /// Monotonic per-task sequence.
        sequence: u64,
        /// Step id.
        step_id: String,
        /// Evidence references that backed completion.
        evidence_refs: Vec<EvidenceRef>,
    },
    /// Step failure.
    StepFailed {
        /// Monotonic per-task sequence.
        sequence: u64,
        /// Step id.
        step_id: String,
        /// Failure reason.
        reason: String,
    },
    /// Evidence attachment.
    EvidenceAttached {
        /// Monotonic per-task sequence.
        sequence: u64,
        /// Evidence id.
        evidence_id: String,
        /// Evidence kind.
        kind: EvidenceKind,
    },
    /// Criterion satisfied through verified evidence.
    CriterionVerified {
        /// Monotonic per-task sequence.
        sequence: u64,
        /// Criterion id.
        criterion_id: String,
        /// Evidence id that satisfied it.
        verified_by: String,
    },
    /// Task entered the blocked phase with a reason.
    TaskBlocked {
        /// Monotonic per-task sequence.
        sequence: u64,
        /// Blocked reason.
        reason: String,
    },
    /// Task completed through the completion gate.
    TaskCompleted {
        /// Monotonic per-task sequence.
        sequence: u64,
    },
    /// Task cancelled.
    TaskCancelled {
        /// Monotonic per-task sequence.
        sequence: u64,
        /// Cancellation reason.
        reason: String,
    },
    /// Task failed.
    TaskFailed {
        /// Monotonic per-task sequence.
        sequence: u64,
        /// Failure reason.
        reason: String,
    },
    /// Contract revision applied.
    TaskContractRevised {
        /// Monotonic per-task sequence.
        sequence: u64,
        /// New revision number.
        revision: u64,
    },
    /// Workflow disposition submitted.
    DispositionSubmitted {
        /// Monotonic per-task sequence.
        sequence: u64,
        /// The disposition request.
        disposition: WorkflowDisposition,
        /// Submission source.
        source: DispositionSource,
        /// Whether the disposition was accepted.
        accepted: bool,
        /// Host note.
        note: Option<String>,
    },
    /// Host planning-depth routing recorded.
    PlanningRouted {
        /// Monotonic per-task sequence.
        sequence: u64,
        /// Routed depth.
        depth: crate::planning::PlanningDepth,
        /// Deterministic decision reason.
        reason: String,
    },
    /// Immutable plan revision stored.
    PlanCreated {
        /// Monotonic per-task sequence.
        sequence: u64,
        /// Plan id.
        plan_id: String,
        /// Plan revision.
        revision: u64,
        /// Plan depth.
        depth: crate::planning::PlanningDepth,
    },
    /// Plan candidate rejected (invalid, denied).
    PlanRejected {
        /// Monotonic per-task sequence.
        sequence: u64,
        /// Exact rejection reason.
        reason: String,
    },
    /// Approval bound to the exact current plan revision.
    PlanApproved {
        /// Monotonic per-task sequence.
        sequence: u64,
        /// Plan id.
        plan_id: String,
        /// Plan revision.
        revision: u64,
        /// Exact content digest of the approved revision.
        digest: String,
    },
    /// Plan marked stale; approval invalidated when one existed.
    PlanInvalidated {
        /// Monotonic per-task sequence.
        sequence: u64,
        /// Plan id.
        plan_id: String,
        /// Plan revision.
        revision: u64,
        /// Exact invalidation reason.
        reason: String,
    },
}

impl ActivityEvent {
    /// Stable machine-readable event type matching the reference.
    pub fn type_str(&self) -> &'static str {
        match self {
            ActivityEvent::TaskStarted { .. } => "task_started",
            ActivityEvent::TaskPhaseChanged { .. } => "task_phase_changed",
            ActivityEvent::StepStarted { .. } => "step_started",
            ActivityEvent::StepCompleted { .. } => "step_completed",
            ActivityEvent::StepFailed { .. } => "step_failed",
            ActivityEvent::EvidenceAttached { .. } => "evidence_attached",
            ActivityEvent::CriterionVerified { .. } => "criterion_verified",
            ActivityEvent::TaskBlocked { .. } => "task_blocked",
            ActivityEvent::TaskCompleted { .. } => "task_completed",
            ActivityEvent::TaskCancelled { .. } => "task_cancelled",
            ActivityEvent::TaskFailed { .. } => "task_failed",
            ActivityEvent::TaskContractRevised { .. } => {
                "task_contract_revised"
            }
            ActivityEvent::DispositionSubmitted { .. } => {
                "disposition_submitted"
            }
            ActivityEvent::PlanningRouted { .. } => "planning_routed",
            ActivityEvent::PlanCreated { .. } => "plan_created",
            ActivityEvent::PlanRejected { .. } => "plan_rejected",
            ActivityEvent::PlanApproved { .. } => "plan_approved",
            ActivityEvent::PlanInvalidated { .. } => "plan_invalidated",
        }
    }

    /// Monotonic per-task sequence number.
    pub fn sequence(&self) -> u64 {
        match self {
            ActivityEvent::TaskStarted { sequence, .. }
            | ActivityEvent::TaskPhaseChanged { sequence, .. }
            | ActivityEvent::StepStarted { sequence, .. }
            | ActivityEvent::StepCompleted { sequence, .. }
            | ActivityEvent::StepFailed { sequence, .. }
            | ActivityEvent::EvidenceAttached { sequence, .. }
            | ActivityEvent::CriterionVerified { sequence, .. }
            | ActivityEvent::TaskBlocked { sequence, .. }
            | ActivityEvent::TaskCompleted { sequence }
            | ActivityEvent::TaskCancelled { sequence, .. }
            | ActivityEvent::TaskFailed { sequence, .. }
            | ActivityEvent::TaskContractRevised { sequence, .. }
            | ActivityEvent::DispositionSubmitted { sequence, .. }
            | ActivityEvent::PlanningRouted { sequence, .. }
            | ActivityEvent::PlanCreated { sequence, .. }
            | ActivityEvent::PlanRejected { sequence, .. }
            | ActivityEvent::PlanApproved { sequence, .. }
            | ActivityEvent::PlanInvalidated { sequence, .. } => *sequence,
        }
    }
}

/// Materialized authoritative task state (R3 subset).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskState {
    /// Task id.
    /// Task id.
    pub task_id: String,
    /// Current contract revision.
    /// Contract revision.
    pub contract_revision: u64,
    /// Exact content digest of the current contract revision.
    /// Contract digest.
    pub contract_digest: String,
    /// Current phase.
    /// Phase.
    pub phase: TaskPhase,
    /// Bounded current-plan reference (identity, depth, staleness,
    /// approval state only; the full immutable plan lives in the runtime
    /// plan history).
    pub plan: crate::planning::TaskPlanState,
    /// Bounded step states.
    /// Steps.
    pub steps: Vec<TaskStepState>,
    /// Acceptance states (one per current contract criterion).
    /// Acceptance.
    pub acceptance: Vec<AcceptanceState>,
    /// Current evidence-backed findings.
    /// Current findings.
    pub current_findings: Vec<FindingRef>,
    /// Bounded evidence records attached to this task.
    /// Evidence.
    pub evidence: Vec<EvidenceRecord>,
    /// Validation gate status.
    /// Validation status.
    pub validation_status: TaskValidationStatus,
    /// Review status.
    /// Review status.
    pub review_status: TaskReviewStatus,
    /// Host-observed development/workflow iteration count.
    /// Iteration.
    pub iteration: u64,
    /// Progress classification.
    /// Progress.
    pub progress: ProgressState,
    /// Host timestamp of task start.
    /// Started at ms.
    pub started_at_ms: i64,
    /// Host timestamp of terminalization, when terminal.
    /// Completed at ms.
    pub completed_at_ms: Option<i64>,
    /// Terminal reason, when the task ended with a reason.
    /// Terminal reason.
    pub terminal_reason: Option<String>,
}

/// Host-owned hard bounds for the R3 task kernel (exact reference
/// limits, UTF-8 byte lengths).
pub mod limits {
    /// Maximum evidence source canonical JSON bytes.
    pub const MAX_EVIDENCE_SOURCE_BYTES: usize = 4096;
    /// Maximum evidence records per task.
    pub const MAX_TASK_EVIDENCE_RECORDS: usize = 256;
    /// Maximum step count per task.
    pub const MAX_TASK_STEPS: usize = 128;
    /// Maximum findings per task.
    pub const MAX_TASK_FINDINGS: usize = 128;
    /// Maximum step description UTF-8 bytes.
    pub const MAX_TASK_STEP_DESCRIPTION_BYTES: usize = 4096;
    /// Maximum finding field UTF-8 bytes.
    pub const MAX_TASK_FINDING_FIELD_BYTES: usize = 4096;
    /// Maximum evidence id UTF-8 bytes.
    pub const MAX_TASK_EVIDENCE_ID_BYTES: usize = 256;
    /// Progress observation window size.
    pub const PROGRESS_WINDOW_SIZE: usize = 8;
    /// Repetitions before the progress state degrades.
    pub const PROGRESS_DEGRADED_REPETITIONS: u64 = 3;
    /// Repetitions before the progress state stalls.
    pub const PROGRESS_STALLED_REPETITIONS: u64 = 5;
}
