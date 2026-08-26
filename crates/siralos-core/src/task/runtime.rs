//! Host-owned task runtime (Stage 3R R3).
//!
//! The runtime is the single authoritative owner of every mutable
//! TaskState: state is created, transitioned, and finalized only through
//! the narrow handle API. Providers, adapters, the CLI, and the UI
//! receive owned snapshots or read-only views; model input arrives only as
//! typed requests (dispositions), never as direct state mutation, and
//! 'complete' is a completion request that still passes the host
//! completion gate. Task state is descriptive/control-flow state: it can
//! never grant capabilities, and security policy remains authoritative
//! elsewhere.
//!
//! The runtime is provider-neutral, sandbox-neutral, and domain-neutral:
//! it observes typed host observations for progress and never imports
//! provider, sandbox, workspace, or domain ports. Time is supplied by an
//! explicit clock so deterministic tests and differential fixtures
//! control every timestamp.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::task::contract::{
    ContractError, ReviseTaskContractInput, TaskContract,
};
use crate::task::evidence::{
    EvidenceError, FindingError, FindingInput, validate_evidence_payload,
    validate_findings,
};
use crate::task::model::{
    AcceptanceState, AcceptanceStatus, ActivityEvent, DispositionSource,
    EvidenceKind, EvidenceRecord, EvidenceRef, EvidenceSource,
    EvidenceVerification, ProgressState, TaskPhase, TaskReviewStatus,
    TaskState, TaskStepSpec, TaskStepState, TaskStepStatus,
    TaskValidationStatus, VerificationOutcome, WorkflowDisposition,
    is_terminal_phase, limits,
};
use crate::task::progress::{
    HostObservation, InternalProgress, create_internal_progress,
    observe_progress, progress_snapshot,
};

/// Clock source for host timestamps; deterministic callers supply a
/// fixed clock.
pub type Clock = fn() -> i64;

/// Ambient wall-clock fallback used only when no explicit clock is
/// supplied (future CLI composition). Deterministic core paths and all
/// differential fixtures supply an explicit clock.
pub fn ambient_clock() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis() as i64,
        Err(_) => 0,
    }
}

/// Task id (validated at contract construction).
pub type TaskId = String;

/// Outcome of a step-level operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOpResult {
    /// The operation succeeded.
    /// Ok.
    Ok,
    /// The operation was rejected with a typed reason.
    /// Rejected.
    Rejected(OpError),
}

impl StepOpResult {
    /// Whether the operation succeeded.
    pub fn is_ok(&self) -> bool {
        matches!(self, StepOpResult::Ok)
    }
}

/// Outcome of an evidence attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachResult {
    /// Evidence was attached.
    /// Attached.
    Attached,
    /// The payload or task state rejected the attachment.
    /// Rejected.
    Rejected(AttachRejection),
}

/// Typed rejection of an evidence attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachRejection {
    /// The task is terminal.
    /// Terminal.
    Terminal,
    /// The payload failed validation.
    /// Invalid.
    Invalid(EvidenceError),
    /// The evidence id is already attached.
    /// Duplicate id.
    DuplicateId,
    /// The task has reached the evidence record bound.
    /// Record limit.
    RecordLimit,
}

impl AttachRejection {
    /// Stable machine-branchable code.
    pub fn code(&self) -> &'static str {
        match self {
            AttachRejection::Terminal => "terminal",
            AttachRejection::Invalid(error) => error.code(),
            AttachRejection::DuplicateId => "duplicate_evidence_id",
            AttachRejection::RecordLimit => "evidence_limit",
        }
    }
}

/// Outcome of a criterion verification operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CriterionResult {
    /// The criterion was satisfied through exact valid evidence.
    /// Verified.
    Verified,
    /// The criterion was marked failed.
    /// Failed.
    Failed,
    /// The operation was rejected.
    /// Rejected.
    Rejected(OpError),
}

/// Typed rejection of a task operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpError {
    /// The task is terminal; authoritative state can no longer change.
    /// Terminal.
    Terminal,
    /// The task is already in the requested phase.
    /// Already phase.
    AlreadyPhase,
    /// The requested phase transition is not allowed.
    /// Invalid transition.
    InvalidTransition,
    /// The step is unknown.
    /// Unknown step.
    UnknownStep,
    /// The step is already active.
    /// Step already active.
    StepAlreadyActive,
    /// The step is already completed.
    /// Step already completed.
    StepAlreadyCompleted,
    /// Only an active step can be completed.
    /// Step not active.
    StepNotActive,
    /// Step completion requires at least one evidence reference.
    /// Step requires evidence.
    StepRequiresEvidence,
    /// A duplicate evidence reference was supplied.
    /// Duplicate evidence ref.
    DuplicateEvidenceRef,
    /// The step does not accept the evidence kind.
    /// Step rejects evidence kind.
    StepRejectsEvidenceKind,
    /// The evidence reference is unknown or belongs to another task.
    /// Unknown evidence ref.
    UnknownEvidenceRef,
    /// The evidence kind does not match the reference kind.
    /// Evidence kind mismatch.
    EvidenceKindMismatch,
    /// The acceptance criterion is unknown.
    /// Unknown criterion.
    UnknownCriterion,
    /// Criterion verification requires exact successful evidence.
    /// Criterion requires verification evidence.
    CriterionRequiresVerificationEvidence,
    /// The evidence reference is unknown.
    /// Unknown evidence.
    UnknownEvidence,
    /// The evidence is not bound to the current contract revision.
    /// Evidence not bound to contract.
    EvidenceNotBoundToContract,
    /// The evidence is not bound to the criterion.
    /// Evidence not bound to criterion.
    EvidenceNotBoundToCriterion,
    /// The evidence does not contain a successful outcome.
    /// Evidence not successful.
    EvidenceNotSuccessful,
    /// The evidence kind cannot verify this criterion kind.
    /// Evidence kind cannot verify criterion.
    EvidenceKindCannotVerifyCriterion,
    /// The completion gate rejected the request.
    /// Completion gate.
    CompletionGate,
}

impl OpError {
    /// Stable machine-branchable code (differential observation
    /// vocabulary shared with the TypeScript oracle adapter).
    pub fn code(&self) -> &'static str {
        match self {
            OpError::Terminal => "terminal",
            OpError::AlreadyPhase => "already_phase",
            OpError::InvalidTransition => "invalid_transition",
            OpError::UnknownStep => "unknown_step",
            OpError::StepAlreadyActive => "step_already_active",
            OpError::StepAlreadyCompleted => "step_already_completed",
            OpError::StepNotActive => "step_not_active",
            OpError::StepRequiresEvidence => "step_requires_evidence",
            OpError::DuplicateEvidenceRef => "duplicate_evidence_ref",
            OpError::StepRejectsEvidenceKind => "step_rejects_evidence_kind",
            OpError::UnknownEvidenceRef => "unknown_evidence_ref",
            OpError::EvidenceKindMismatch => "evidence_kind_mismatch",
            OpError::UnknownCriterion => "unknown_criterion",
            OpError::CriterionRequiresVerificationEvidence => {
                "criterion_requires_verification_evidence"
            }
            OpError::UnknownEvidence => "unknown_evidence",
            OpError::EvidenceNotBoundToContract => {
                "evidence_not_bound_to_contract"
            }
            OpError::EvidenceNotBoundToCriterion => {
                "evidence_not_bound_to_criterion"
            }
            OpError::EvidenceNotSuccessful => "evidence_not_successful",
            OpError::EvidenceKindCannotVerifyCriterion => {
                "evidence_kind_cannot_verify_criterion"
            }
            OpError::CompletionGate => "completion_gate",
        }
    }
}

/// Completion-gate evaluation: what is missing before the task may
/// complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionEvaluation {
    /// Whether completion is currently allowed.
    /// Allowed.
    pub allowed: bool,
    /// Exact missing-condition reasons (contractually observable).
    /// Missing.
    pub missing: Vec<String>,
}

/// Outcome of a completion request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionResult {
    /// The task completed.
    Completed,
    /// The completion request was rejected with reasons.
    Rejected {
        /// Exact missing-condition reasons.
        reasons: Vec<String>,
    },
}

/// Outcome of a disposition submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispositionResult {
    /// Whether the disposition was accepted.
    /// Accepted.
    pub accepted: bool,
    /// Completion-gate evaluation when the disposition was a completion
    /// request.
    /// Evaluation.
    pub evaluation: Option<CompletionEvaluation>,
    /// Typed rejection code when the disposition was not accepted.
    /// Code.
    pub code: Option<OpError>,
}

/// Typed failure of task creation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskCreateError {
    /// A task with the same id already exists; history is never replaced.
    /// Duplicate task.
    DuplicateTask,
    /// The step specifications failed validation.
    /// Steps.
    Steps(StepSpecError),
}

impl TaskCreateError {
    /// Stable machine-branchable code.
    pub fn code(&self) -> &'static str {
        match self {
            TaskCreateError::DuplicateTask => "duplicate_task",
            TaskCreateError::Steps(error) => error.code(),
        }
    }
}

/// Typed rejection of a step-spec preparation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepSpecError {
    /// More than the maximum steps were supplied.
    /// Too many steps.
    TooManySteps,
    /// A step id is invalid.
    /// Invalid step id.
    InvalidStepId,
    /// A step id is duplicated.
    /// Duplicate step id.
    DuplicateStepId,
    /// A step description is empty after trimming.
    /// Empty step description.
    EmptyStepDescription,
    /// A step description exceeds the byte bound.
    /// Step description too large.
    StepDescriptionTooLarge,
    /// A step kind is unsupported.
    /// Invalid step kind.
    InvalidStepKind,
    /// A step accepts no evidence kinds.
    /// Empty step accepts.
    EmptyStepAccepts,
    /// A step accepts an invalid evidence kind.
    /// Invalid step evidence kind.
    InvalidStepEvidenceKind,
    /// A step accepts duplicate evidence kinds.
    /// Duplicate step evidence kind.
    DuplicateStepEvidenceKind,
}

impl StepSpecError {
    /// Stable machine-branchable code.
    pub fn code(&self) -> &'static str {
        match self {
            StepSpecError::TooManySteps => "too_many_steps",
            StepSpecError::InvalidStepId => "invalid_step_id",
            StepSpecError::DuplicateStepId => "duplicate_step_id",
            StepSpecError::EmptyStepDescription => "empty_step_description",
            StepSpecError::StepDescriptionTooLarge => {
                "step_description_too_large"
            }
            StepSpecError::InvalidStepKind => "invalid_step_kind",
            StepSpecError::EmptyStepAccepts => "empty_step_accepts",
            StepSpecError::InvalidStepEvidenceKind => {
                "invalid_step_evidence_kind"
            }
            StepSpecError::DuplicateStepEvidenceKind => {
                "duplicate_step_evidence_kind"
            }
        }
    }
}

/// Typed failure of a contract revision request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviseError {
    /// The task is terminal; the contract can no longer be revised.
    /// Terminal.
    Terminal,
    /// The revision input failed contract validation.
    /// Contract.
    Contract(ContractError),
}

impl ReviseError {
    /// Stable machine-branchable code.
    pub fn code(&self) -> &'static str {
        match self {
            ReviseError::Terminal => "terminal",
            ReviseError::Contract(error) => error.code(),
        }
    }
}

/// Input for creating a task.
#[derive(Debug, Clone)]
pub struct CreateTaskInput {
    /// The validated revision-1 contract.
    /// Contract.
    pub contract: TaskContract,
    /// Optional host-authored step specifications.
    /// Steps.
    pub steps: Vec<TaskStepSpec>,
    /// Optional starting iteration count.
    /// Iteration.
    pub iteration: Option<f64>,
}

fn step_id_pattern_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return false;
    }
    if !bytes[0].is_ascii_alphabetic() {
        return false;
    }
    bytes[1..].iter().all(|byte| {
        byte.is_ascii_alphanumeric()
            || *byte == b'.'
            || *byte == b'_'
            || *byte == b'-'
    })
}

/// Validate and detach step specifications (reference semantics).
pub fn prepare_step_specs(
    steps: Vec<TaskStepSpec>,
) -> Result<Vec<TaskStepSpec>, StepSpecError> {
    if steps.len() > limits::MAX_TASK_STEPS {
        return Err(StepSpecError::TooManySteps);
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut validated = Vec::with_capacity(steps.len());
    for spec in steps {
        if !step_id_pattern_ok(&spec.id) {
            return Err(StepSpecError::InvalidStepId);
        }
        if !ids.insert(spec.id.clone()) {
            return Err(StepSpecError::DuplicateStepId);
        }
        let description = spec.description.trim();
        if description.is_empty() {
            return Err(StepSpecError::EmptyStepDescription);
        }
        if description.len() > limits::MAX_TASK_STEP_DESCRIPTION_BYTES {
            return Err(StepSpecError::StepDescriptionTooLarge);
        }
        if spec.accepts.is_empty() {
            return Err(StepSpecError::EmptyStepAccepts);
        }
        let mut kinds = std::collections::BTreeSet::new();
        for kind in &spec.accepts {
            if !kinds.insert(*kind) {
                return Err(StepSpecError::DuplicateStepEvidenceKind);
            }
        }
        validated.push(TaskStepSpec {
            id: spec.id,
            description: description.to_owned(),
            kind: spec.kind,
            accepts: spec.accepts,
        });
    }
    Ok(validated)
}

/// Internal record owned exclusively by the runtime.
#[derive(Debug)]
struct TaskRecord {
    id: TaskId,
    specs: Vec<TaskStepSpec>,
    contract: TaskContract,
    contract_revisions: Vec<TaskContract>,
    state: TaskState,
    plans: Vec<crate::planning::TaskPlan>,
    plan_approval: Option<crate::planning::PlanApproval>,
    activity: Vec<ActivityEvent>,
    sequence: u64,
    progress: InternalProgress,
}

impl TaskRecord {
    fn terminal_reason(&self) -> Option<String> {
        is_terminal_phase(self.state.phase).then(|| {
            format!(
                "The task is terminal ({}); authoritative state can no longer be changed.",
                self.state.phase.as_str()
            )
        })
    }
}

/// Allowed phase transitions (exact reference table).
fn allowed_transitions(phase: TaskPhase) -> &'static [TaskPhase] {
    match phase {
        TaskPhase::Prepared => {
            &[TaskPhase::Working, TaskPhase::Cancelled, TaskPhase::Failed]
        }
        TaskPhase::Working => &[
            TaskPhase::Validating,
            TaskPhase::Reviewing,
            TaskPhase::Blocked,
            TaskPhase::Cancelled,
            TaskPhase::Failed,
        ],
        TaskPhase::Validating => &[
            TaskPhase::Working,
            TaskPhase::Reviewing,
            TaskPhase::Blocked,
            TaskPhase::Cancelled,
            TaskPhase::Failed,
        ],
        TaskPhase::Reviewing => &[
            TaskPhase::Working,
            TaskPhase::Validating,
            TaskPhase::Blocked,
            TaskPhase::Cancelled,
            TaskPhase::Failed,
        ],
        TaskPhase::Blocked => {
            &[TaskPhase::Working, TaskPhase::Cancelled, TaskPhase::Failed]
        }
        TaskPhase::Completed | TaskPhase::Cancelled | TaskPhase::Failed => &[],
    }
}

/// The task runtime: single authoritative owner of all task records.
#[derive(Debug)]
pub struct TaskRuntime {
    records: Vec<TaskRecord>,
    clock: Clock,
}

impl Default for TaskRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskRuntime {
    /// Create a runtime using the ambient clock (CLI composition use;
    /// deterministic callers use with_clock).
    pub fn new() -> Self {
        Self::with_clock(ambient_clock)
    }

    /// Create a runtime with an explicit clock so every host timestamp
    /// is controllable.
    pub fn with_clock(clock: Clock) -> Self {
        Self { records: Vec::new(), clock }
    }

    /// Create a task from a validated contract and optional steps.
    ///
    /// # Errors
    ///
    /// Returns DuplicateTask when the id already exists (authoritative
    /// history is never replaced) or Steps when the step specifications
    /// fail validation.
    pub fn create_task(
        &mut self,
        input: CreateTaskInput,
    ) -> Result<TaskId, TaskCreateError> {
        if self.records.iter().any(|record| record.id == input.contract.id()) {
            return Err(TaskCreateError::DuplicateTask);
        }
        let specs =
            prepare_step_specs(input.steps).map_err(TaskCreateError::Steps)?;
        let now = (self.clock)();
        let record =
            self.build_record(input.contract, specs, input.iteration, now);
        let id = record.id.clone();
        self.records.push(record);
        Ok(id)
    }

    fn build_record(
        &mut self,
        contract: TaskContract,
        specs: Vec<TaskStepSpec>,
        iteration: Option<f64>,
        now: i64,
    ) -> TaskRecord {
        let steps: Vec<TaskStepState> = specs
            .iter()
            .map(|spec| TaskStepState {
                id: spec.id.clone(),
                description: spec.description.clone(),
                kind: spec.kind,
                status: TaskStepStatus::Pending,
                evidence_refs: Vec::new(),
                failed_reason: None,
                blocked_reason: None,
            })
            .collect();
        let acceptance: Vec<AcceptanceState> = contract
            .acceptance_criteria()
            .iter()
            .map(|criterion| AcceptanceState {
                criterion_id: criterion.id().to_owned(),
                description: criterion.description().to_owned(),
                verification_kind: criterion.verification_kind(),
                status: AcceptanceStatus::Pending,
                verified_by: None,
                note: None,
            })
            .collect();
        let task_id = contract.id().to_owned();
        let contract_revision = contract.revision();
        let contract_digest = contract.digest().to_owned();
        let mut record = TaskRecord {
            id: task_id.clone(),
            specs,
            contract,
            contract_revisions: Vec::new(),
            plans: Vec::new(),
            plan_approval: None,
            state: TaskState {
                task_id,
                contract_revision,
                contract_digest,
                phase: TaskPhase::Prepared,
                plan: crate::planning::NO_TASK_PLAN,
                steps,
                acceptance,
                current_findings: Vec::new(),
                evidence: Vec::new(),
                validation_status: TaskValidationStatus::NotRun,
                review_status: TaskReviewStatus::NotRun,
                iteration: crate::task::evidence::normalize_iteration(
                    iteration,
                ),
                progress: progress_snapshot(&create_internal_progress()),
                started_at_ms: now,
                completed_at_ms: None,
                terminal_reason: None,
            },
            activity: Vec::new(),
            sequence: 0,
            progress: create_internal_progress(),
        };
        record.contract_revisions.push(record.contract.clone());
        self.append_activity(
            &mut record,
            ActivityEvent::TaskStarted { sequence: 0, contract_revision },
        );
        self.observe_progress(
            &mut record,
            HostObservation {
                action: "task.started".to_owned(),
                fingerprint: contract_revision.to_string(),
                progress: false,
            },
            now,
        );
        record
    }

    fn append_activity(
        &mut self,
        record: &mut TaskRecord,
        mut event: ActivityEvent,
    ) {
        record.sequence += 1;
        match &mut event {
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
            | ActivityEvent::PlanInvalidated { sequence, .. } => {
                *sequence = record.sequence;
            }
        }
        record.activity.push(event);
    }

    fn observe_progress(
        &mut self,
        record: &mut TaskRecord,
        observation: HostObservation,
        now: i64,
    ) {
        observe_progress(&mut record.progress, &observation, now);
    }

    /// Narrow mutation surface for one task. The handle borrows the
    /// runtime exclusively, so no other code can observe or mutate the
    /// task while an operation is in flight.
    pub fn task(&mut self, task_id: &str) -> Option<TaskHandle<'_>> {
        self.records
            .iter_mut()
            .find(|record| record.id == task_id)
            .map(|record| TaskHandle { record, clock: self.clock })
    }

    /// Task ids in deterministic creation order.
    pub fn list_task_ids(&self) -> Vec<TaskId> {
        self.records.iter().map(|record| record.id.clone()).collect()
    }

    /// Most recently created task id, when any task exists.
    pub fn latest_task_id(&self) -> Option<TaskId> {
        self.records.last().map(|record| record.id.clone())
    }

    /// Number of tasks.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether no task exists.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// Narrow, exclusive mutation surface for one task record.
pub struct TaskHandle<'a> {
    record: &'a mut TaskRecord,
    clock: Clock,
}

impl TaskHandle<'_> {
    /// Owned immutable snapshot of the authoritative state.
    pub fn snapshot(&self) -> TaskState {
        self.record.state.clone()
    }

    /// Current contract revision.
    pub fn contract(&self) -> &TaskContract {
        &self.record.contract
    }

    /// All contract revisions, oldest first (immutable history).
    pub fn contract_revisions(&self) -> &[TaskContract] {
        &self.record.contract_revisions
    }

    /// Append-only activity records.
    pub fn activity_log(&self) -> &[ActivityEvent] {
        &self.record.activity
    }

    /// Current progress snapshot.
    pub fn progress(&self) -> ProgressState {
        progress_snapshot(&self.record.progress)
    }

    /// Produce the next immutable contract revision; the current
    /// revision is never mutated and acceptance is reconciled against the
    /// new exact contract.
    ///
    /// # Errors
    ///
    /// Returns ReviseError::Terminal on a terminal task or a contract
    /// validation error.
    pub fn revise_contract(
        &mut self,
        changes: ReviseTaskContractInput,
    ) -> Result<TaskContract, ReviseError> {
        if self.record.terminal_reason().is_some() {
            return Err(ReviseError::Terminal);
        }
        let revision = self
            .record
            .contract
            .revise(changes)
            .map_err(ReviseError::Contract)?;
        self.record.contract_revisions.push(revision.clone());
        self.record.contract = revision.clone();
        self.record.state.contract_revision = revision.revision();
        self.record.state.contract_digest = revision.digest().to_owned();
        self.reconcile_acceptance(&revision);
        self.invalidate_plan_for_contract_revision();
        let mut event = ActivityEvent::TaskContractRevised {
            sequence: 0,
            revision: revision.revision(),
        };
        self.append_activity(&mut event);
        Ok(revision)
    }

    /// Mark the current plan stale and invalidate any approval when a
    /// TaskContract revision advanced (exact reference activity trail).
    fn invalidate_plan_for_contract_revision(&mut self) {
        let Some(current) = self.record.plans.last().cloned() else {
            return;
        };
        self.record.state.plan.state =
            crate::planning::TaskPlanStateKind::Stale;
        self.record.state.plan.stale_reason = Some(
            "The TaskContract revision advanced after this plan was created."
                .to_owned(),
        );
        if let Some(approval) = self.record.plan_approval.take() {
            self.record.state.plan.approval =
                crate::planning::TaskPlanApprovalKind::Invalidated;
            let mut event = ActivityEvent::PlanInvalidated {
                sequence: 0,
                plan_id: approval.plan_id,
                revision: approval.plan_revision,
                reason: "The TaskContract revision advanced; the plan approval no longer applies."
                    .to_owned(),
            };
            self.append_activity(&mut event);
        }
        let mut event = ActivityEvent::PlanInvalidated {
            sequence: 0,
            plan_id: current.id,
            revision: current.revision,
            reason: "The TaskContract revision advanced; the plan is stale until revalidated or replanned."
                .to_owned(),
        };
        self.append_activity(&mut event);
    }

    fn reconcile_acceptance(&mut self, contract: &TaskContract) {
        self.record.state.acceptance = contract
            .acceptance_criteria()
            .iter()
            .map(|criterion| AcceptanceState {
                criterion_id: criterion.id().to_owned(),
                description: criterion.description().to_owned(),
                verification_kind: criterion.verification_kind(),
                status: AcceptanceStatus::Pending,
                verified_by: None,
                note: None,
            })
            .collect();
    }

    /// Host-controlled phase transition, validated against the reference
    /// transition table.
    pub fn transition_phase(&mut self, phase: TaskPhase) -> StepOpResult {
        if self.record.state.phase == phase {
            return StepOpResult::Rejected(OpError::AlreadyPhase);
        }
        if !allowed_transitions(self.record.state.phase).contains(&phase) {
            return StepOpResult::Rejected(OpError::InvalidTransition);
        }
        self.record.state.phase = phase;
        if is_terminal_phase(phase) {
            self.record.state.completed_at_ms = Some((self.clock)());
        }
        let mut event = ActivityEvent::TaskPhaseChanged { sequence: 0, phase };
        self.append_activity(&mut event);
        StepOpResult::Ok
    }

    /// Begin (activate) a step.
    pub fn begin_step(&mut self, step_id: &str) -> StepOpResult {
        if self.record.terminal_reason().is_some() {
            return StepOpResult::Rejected(OpError::Terminal);
        }
        let Some(step) = self.find_step_mut(step_id) else {
            return StepOpResult::Rejected(OpError::UnknownStep);
        };
        if step.status == TaskStepStatus::Active {
            return StepOpResult::Rejected(OpError::StepAlreadyActive);
        }
        if step.status == TaskStepStatus::Completed {
            return StepOpResult::Rejected(OpError::StepAlreadyCompleted);
        }
        step.status = TaskStepStatus::Active;
        step.failed_reason = None;
        step.blocked_reason = None;
        let mut event = ActivityEvent::StepStarted {
            sequence: 0,
            step_id: step_id.to_owned(),
        };
        self.append_activity(&mut event);
        StepOpResult::Ok
    }

    /// Fail a step with a reason.
    pub fn fail_step(&mut self, step_id: &str, reason: &str) -> StepOpResult {
        if self.record.terminal_reason().is_some() {
            return StepOpResult::Rejected(OpError::Terminal);
        }
        let Some(step) = self.find_step_mut(step_id) else {
            return StepOpResult::Rejected(OpError::UnknownStep);
        };
        if step.status == TaskStepStatus::Completed {
            return StepOpResult::Rejected(OpError::StepAlreadyCompleted);
        }
        step.status = TaskStepStatus::Failed;
        step.failed_reason = Some(reason.to_owned());
        let mut event = ActivityEvent::StepFailed {
            sequence: 0,
            step_id: step_id.to_owned(),
            reason: reason.to_owned(),
        };
        self.append_activity(&mut event);
        StepOpResult::Ok
    }

    /// Evidence-backed step completion: refs must exist, be task-scoped,
    /// and be accepted by the step.
    pub fn complete_step(
        &mut self,
        step_id: &str,
        refs: &[EvidenceRef],
    ) -> StepOpResult {
        if self.record.terminal_reason().is_some() {
            return StepOpResult::Rejected(OpError::Terminal);
        }
        let step_index =
            self.record.state.steps.iter().position(|step| step.id == step_id);
        let spec = self.record.specs.iter().find(|spec| spec.id == step_id);
        let (Some(step_index), Some(spec)) = (step_index, spec) else {
            return StepOpResult::Rejected(OpError::UnknownStep);
        };
        let step = &self.record.state.steps[step_index];
        if step.status != TaskStepStatus::Active {
            return StepOpResult::Rejected(OpError::StepNotActive);
        }
        if refs.is_empty() {
            return StepOpResult::Rejected(OpError::StepRequiresEvidence);
        }
        let mut seen = std::collections::BTreeSet::new();
        for reference in refs {
            if !seen.insert(reference.evidence_id.clone()) {
                return StepOpResult::Rejected(OpError::DuplicateEvidenceRef);
            }
            if !spec.accepts.contains(&reference.kind) {
                return StepOpResult::Rejected(
                    OpError::StepRejectsEvidenceKind,
                );
            }
            let evidence = self
                .record
                .state
                .evidence
                .iter()
                .find(|entry| entry.id == reference.evidence_id);
            let Some(evidence) = evidence else {
                return StepOpResult::Rejected(OpError::UnknownEvidenceRef);
            };
            if evidence.task_id != self.record.id {
                return StepOpResult::Rejected(OpError::UnknownEvidenceRef);
            }
            if evidence.kind != reference.kind {
                return StepOpResult::Rejected(OpError::EvidenceKindMismatch);
            }
        }
        let step = &mut self.record.state.steps[step_index];
        step.status = TaskStepStatus::Completed;
        step.failed_reason = None;
        step.blocked_reason = None;
        step.evidence_refs = refs.to_vec();
        let now = (self.clock)();
        let mut event = ActivityEvent::StepCompleted {
            sequence: 0,
            step_id: step_id.to_owned(),
            evidence_refs: refs.to_vec(),
        };
        self.append_activity(&mut event);
        self.observe_progress(
            HostObservation {
                action: "step.completed".to_owned(),
                fingerprint: step_id.to_owned(),
                progress: true,
            },
            now,
        );
        StepOpResult::Ok
    }

    fn find_step_mut(&mut self, step_id: &str) -> Option<&mut TaskStepState> {
        self.record.state.steps.iter_mut().find(|step| step.id == step_id)
    }

    /// Attach a bounded, validated evidence record.
    pub fn attach_evidence(
        &mut self,
        id: &str,
        kind: EvidenceKind,
        source: EvidenceSource,
        verification: Option<EvidenceVerification>,
    ) -> AttachResult {
        if self.record.terminal_reason().is_some() {
            return AttachResult::Rejected(AttachRejection::Terminal);
        }
        let validated =
            match validate_evidence_payload(id, kind, source, verification) {
                Ok(validated) => validated,
                Err(error) => {
                    return AttachResult::Rejected(AttachRejection::Invalid(
                        error,
                    ));
                }
            };
        if self.record.state.evidence.iter().any(|entry| entry.id == id) {
            return AttachResult::Rejected(AttachRejection::DuplicateId);
        }
        if self.record.state.evidence.len()
            >= limits::MAX_TASK_EVIDENCE_RECORDS
        {
            return AttachResult::Rejected(AttachRejection::RecordLimit);
        }
        let now = (self.clock)();
        self.record.state.evidence.push(EvidenceRecord {
            id: id.to_owned(),
            kind,
            task_id: self.record.id.clone(),
            task_contract_revision: self.record.contract.revision(),
            task_contract_digest: self.record.contract.digest().to_owned(),
            source: validated.source,
            verification: validated.verification,
            attached_at_ms: now,
        });
        let mut event = ActivityEvent::EvidenceAttached {
            sequence: 0,
            evidence_id: id.to_owned(),
            kind,
        };
        self.append_activity(&mut event);
        self.observe_progress(
            HostObservation {
                action: "evidence.attached".to_owned(),
                fingerprint: kind.as_str().to_owned(),
                progress: true,
            },
            now,
        );
        AttachResult::Attached
    }
}

impl TaskHandle<'_> {
    fn append_activity(&mut self, event: &mut ActivityEvent) {
        self.record.sequence += 1;
        match event {
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
            | ActivityEvent::PlanInvalidated { sequence, .. } => {
                *sequence = self.record.sequence;
            }
        }
        self.record.activity.push(event.clone());
    }

    fn observe_progress(&mut self, observation: HostObservation, now: i64) {
        observe_progress(&mut self.record.progress, &observation, now);
    }

    /// Verify an acceptance criterion against exact valid evidence bound
    /// to the current contract revision/digest.
    pub fn verify_criterion(
        &mut self,
        criterion_id: &str,
        verified_by: Option<&str>,
        note: Option<&str>,
    ) -> CriterionResult {
        if self.record.terminal_reason().is_some() {
            return CriterionResult::Rejected(OpError::Terminal);
        }
        let Some(criterion_index) = self
            .record
            .state
            .acceptance
            .iter()
            .position(|criterion| criterion.criterion_id == criterion_id)
        else {
            return CriterionResult::Rejected(OpError::UnknownCriterion);
        };
        let Some(verified_by) = verified_by else {
            return CriterionResult::Rejected(
                OpError::CriterionRequiresVerificationEvidence,
            );
        };
        let Some(evidence) = self
            .record
            .state
            .evidence
            .iter()
            .find(|entry| entry.id == verified_by)
        else {
            return CriterionResult::Rejected(OpError::UnknownEvidence);
        };
        if evidence.task_id != self.record.id
            || evidence.task_contract_revision
                != self.record.contract.revision()
            || evidence.task_contract_digest != self.record.contract.digest()
        {
            return CriterionResult::Rejected(
                OpError::EvidenceNotBoundToContract,
            );
        }
        let Some(verification) = &evidence.verification else {
            return CriterionResult::Rejected(
                OpError::EvidenceNotBoundToCriterion,
            );
        };
        if verification.criterion_id.as_deref() != Some(criterion_id)
            || verification.check_id != criterion_id
        {
            return CriterionResult::Rejected(
                OpError::EvidenceNotBoundToCriterion,
            );
        }
        if verification.outcome != VerificationOutcome::Passed
            || !crate::task::evidence::source_supports_successful_outcome(
                evidence.kind,
                &evidence.source,
            )
        {
            return CriterionResult::Rejected(OpError::EvidenceNotSuccessful);
        }
        let kind_matches = match self.record.state.acceptance[criterion_index]
            .verification_kind
        {
            crate::task::contract::VerificationKind::User => {
                evidence.kind == EvidenceKind::UserApproval
            }
            crate::task::contract::VerificationKind::Review => {
                evidence.kind == EvidenceKind::ReviewResult
            }
            crate::task::contract::VerificationKind::Deterministic => {
                evidence.kind != EvidenceKind::ReviewResult
                    && evidence.kind != EvidenceKind::UserApproval
            }
        };
        if !kind_matches {
            return CriterionResult::Rejected(
                OpError::EvidenceKindCannotVerifyCriterion,
            );
        }
        let criterion = &mut self.record.state.acceptance[criterion_index];
        criterion.status = AcceptanceStatus::Satisfied;
        criterion.verified_by = Some(verified_by.to_owned());
        criterion.note = note.map(str::to_owned);
        let now = (self.clock)();
        let mut event = ActivityEvent::CriterionVerified {
            sequence: 0,
            criterion_id: criterion_id.to_owned(),
            verified_by: verified_by.to_owned(),
        };
        self.append_activity(&mut event);
        self.observe_progress(
            HostObservation {
                action: "criterion.verified".to_owned(),
                fingerprint: criterion_id.to_owned(),
                progress: true,
            },
            now,
        );
        CriterionResult::Verified
    }

    /// Mark a criterion failed.
    pub fn mark_criterion_failed(
        &mut self,
        criterion_id: &str,
        note: Option<&str>,
    ) -> CriterionResult {
        if self.record.terminal_reason().is_some() {
            return CriterionResult::Rejected(OpError::Terminal);
        }
        let Some(criterion) = self
            .record
            .state
            .acceptance
            .iter_mut()
            .find(|criterion| criterion.criterion_id == criterion_id)
        else {
            return CriterionResult::Rejected(OpError::UnknownCriterion);
        };
        criterion.status = AcceptanceStatus::Failed;
        criterion.verified_by = None;
        criterion.note = note.map(str::to_owned);
        CriterionResult::Failed
    }

    /// Replace the task's evidence-backed findings list (host-observed).
    ///
    /// # Errors
    ///
    /// Returns a FindingError when the findings fail validation; on a
    /// terminal task the request is a no-op, mirroring the reference.
    pub fn set_findings(
        &mut self,
        findings: Vec<FindingInput>,
    ) -> Result<(), FindingError> {
        if self.record.terminal_reason().is_some() {
            return Ok(());
        }
        let validated = validate_findings(findings)?;
        self.record.state.current_findings = validated;
        Ok(())
    }

    /// Set the validation gate status; no-op on a terminal task.
    pub fn set_validation_status(&mut self, status: TaskValidationStatus) {
        if self.record.terminal_reason().is_none() {
            self.record.state.validation_status = status;
        }
    }

    /// Set the review status; no-op on a terminal task.
    pub fn set_review_status(&mut self, status: TaskReviewStatus) {
        if self.record.terminal_reason().is_none() {
            self.record.state.review_status = status;
        }
    }

    /// Set the host-observed iteration count; no-op on a terminal task.
    pub fn set_iteration(&mut self, iteration: u64) {
        if self.record.terminal_reason().is_none() {
            self.record.state.iteration = iteration;
        }
    }

    /// Evaluate the completion gate without mutating state.
    pub fn evaluate_completion(&self) -> CompletionEvaluation {
        let mut missing = Vec::new();
        for step in &self.record.state.steps {
            if step.status != TaskStepStatus::Completed {
                missing.push(format!("step not completed: {}", step.id));
            }
        }
        for criterion in &self.record.state.acceptance {
            if criterion.status != AcceptanceStatus::Satisfied {
                missing.push(format!(
                    "acceptance criterion not satisfied: {}",
                    criterion.criterion_id
                ));
            }
        }
        if !self.record.state.validation_status.is_acceptable_for_completion()
        {
            missing.push(format!(
                "validation is {} (clean required)",
                self.record.state.validation_status.as_str()
            ));
        }
        if self.record.state.review_status != TaskReviewStatus::Clean {
            missing.push(format!(
                "review is {} (clean required)",
                self.record.state.review_status.as_str()
            ));
        }
        let blocking = self
            .record
            .state
            .current_findings
            .iter()
            .filter(|finding| finding.severity.is_blocking())
            .count();
        if blocking > 0 {
            missing.push(format!("{blocking} blocking finding(s) unresolved"));
        }
        CompletionEvaluation { allowed: missing.is_empty(), missing }
    }

    /// Request completion through the host completion gate.
    pub fn complete_task(&mut self) -> CompletionResult {
        if let Some(reason) = self.record.terminal_reason() {
            return CompletionResult::Rejected { reasons: vec![reason] };
        }
        let evaluation = self.evaluate_completion();
        if !evaluation.allowed {
            return CompletionResult::Rejected { reasons: evaluation.missing };
        }
        self.record.state.phase = TaskPhase::Completed;
        self.record.state.completed_at_ms = Some((self.clock)());
        self.record.state.terminal_reason = None;
        let mut event = ActivityEvent::TaskCompleted { sequence: 0 };
        self.append_activity(&mut event);
        let now = (self.clock)();
        self.observe_progress(
            HostObservation {
                action: "task.completed".to_owned(),
                fingerprint: self.record.id.clone(),
                progress: true,
            },
            now,
        );
        CompletionResult::Completed
    }

    /// Cancel the task with a reason; no-op when already terminal.
    pub fn cancel(&mut self, reason: &str) {
        if is_terminal_phase(self.record.state.phase) {
            return;
        }
        self.record.state.phase = TaskPhase::Cancelled;
        self.record.state.completed_at_ms = Some((self.clock)());
        self.record.state.terminal_reason = Some(reason.to_owned());
        let mut event = ActivityEvent::TaskCancelled {
            sequence: 0,
            reason: reason.to_owned(),
        };
        self.append_activity(&mut event);
    }

    /// Fail the task with a reason; no-op when already terminal.
    pub fn fail(&mut self, reason: &str) {
        if is_terminal_phase(self.record.state.phase) {
            return;
        }
        self.record.state.phase = TaskPhase::Failed;
        self.record.state.completed_at_ms = Some((self.clock)());
        self.record.state.terminal_reason = Some(reason.to_owned());
        let mut event = ActivityEvent::TaskFailed {
            sequence: 0,
            reason: reason.to_owned(),
        };
        self.append_activity(&mut event);
    }

    /// Move the task to the blocked phase through the transition table
    /// and record the reason; a rejected transition is a no-op.
    pub fn mark_blocked(&mut self, reason: &str) {
        if let StepOpResult::Ok = self.transition_phase(TaskPhase::Blocked) {
            self.record.state.terminal_reason = Some(reason.to_owned());
            let mut event = ActivityEvent::TaskBlocked {
                sequence: 0,
                reason: reason.to_owned(),
            };
            self.append_activity(&mut event);
        }
    }

    /// Submit a workflow disposition. A disposition is a request: it
    /// never directly mutates arbitrary Host state, and 'complete' still
    /// passes the completion gate.
    pub fn submit_disposition(
        &mut self,
        disposition: WorkflowDisposition,
        source: DispositionSource,
    ) -> DispositionResult {
        if self.record.terminal_reason().is_some() {
            let evaluation =
                matches!(disposition, WorkflowDisposition::Complete)
                    .then(|| self.evaluate_completion());
            return DispositionResult {
                accepted: false,
                evaluation,
                code: Some(OpError::Terminal),
            };
        }
        match &disposition {
            WorkflowDisposition::Complete => {
                let evaluation = self.evaluate_completion();
                if evaluation.allowed {
                    let completed = self.complete_task();
                    if matches!(completed, CompletionResult::Completed) {
                        let mut event = ActivityEvent::DispositionSubmitted {
                            sequence: 0,
                            disposition: disposition.clone(),
                            source,
                            accepted: true,
                            note: Some("completion gate passed".to_owned()),
                        };
                        self.append_activity(&mut event);
                        return DispositionResult {
                            accepted: true,
                            evaluation: Some(evaluation),
                            code: None,
                        };
                    }
                }
                let reason =
                    evaluation.missing.first().cloned().unwrap_or_else(|| {
                        "completion gate not satisfied".to_owned()
                    });
                let mut event = ActivityEvent::DispositionSubmitted {
                    sequence: 0,
                    disposition: disposition.clone(),
                    source,
                    accepted: false,
                    note: Some(reason.clone()),
                };
                self.append_activity(&mut event);
                DispositionResult {
                    accepted: false,
                    evaluation: Some(evaluation),
                    code: Some(OpError::CompletionGate),
                }
            }
            WorkflowDisposition::Blocked { reason } => {
                let transition = self.transition_phase(TaskPhase::Blocked);
                if let StepOpResult::Rejected(error) = &transition {
                    let mut event = ActivityEvent::DispositionSubmitted {
                        sequence: 0,
                        disposition: disposition.clone(),
                        source,
                        accepted: false,
                        note: Some(error.code().to_owned()),
                    };
                    self.append_activity(&mut event);
                    return DispositionResult {
                        accepted: false,
                        evaluation: None,
                        code: Some(error.clone()),
                    };
                }
                self.record.state.terminal_reason = Some(reason.clone());
                let mut blocked = ActivityEvent::TaskBlocked {
                    sequence: 0,
                    reason: reason.clone(),
                };
                self.append_activity(&mut blocked);
                let mut event = ActivityEvent::DispositionSubmitted {
                    sequence: 0,
                    disposition: disposition.clone(),
                    source,
                    accepted: true,
                    note: None,
                };
                self.append_activity(&mut event);
                DispositionResult {
                    accepted: true,
                    evaluation: None,
                    code: None,
                }
            }
            WorkflowDisposition::Continue { next_action } => {
                let mut event = ActivityEvent::DispositionSubmitted {
                    sequence: 0,
                    disposition: disposition.clone(),
                    source,
                    accepted: true,
                    note: next_action.clone(),
                };
                self.append_activity(&mut event);
                DispositionResult {
                    accepted: true,
                    evaluation: None,
                    code: None,
                }
            }
        }
    }

    /// Feed a host observation; on a terminal task this is a read-only
    /// no-op, mirroring the reference.
    pub fn observe(&mut self, observation: HostObservation) -> ProgressState {
        if self.record.terminal_reason().is_some() {
            return progress_snapshot(&self.record.progress);
        }
        let now = (self.clock)();
        observe_progress(&mut self.record.progress, &observation, now)
    }

    /// The owning task id.
    pub fn task_id(&self) -> &str {
        &self.record.id
    }

    /// Record the host's planning-depth routing (deterministic policy).
    pub fn route_planning(
        &mut self,
        depth: crate::planning::PlanningDepth,
        reason: &str,
    ) {
        if self.record.terminal_reason().is_some() {
            return;
        }
        self.record.state.plan.depth = depth;
        let mut event = ActivityEvent::PlanningRouted {
            sequence: 0,
            depth,
            reason: reason.to_owned(),
        };
        self.append_activity(&mut event);
    }

    /// Record a host-observed plan rejection (invalid candidate, denial).
    pub fn reject_plan(&mut self, reason: &str) {
        if self.record.terminal_reason().is_some() {
            return;
        }
        let mut event = ActivityEvent::PlanRejected {
            sequence: 0,
            reason: reason.to_owned(),
        };
        self.append_activity(&mut event);
    }

    /// Store an immutable plan revision bound to the current TaskContract.
    ///
    /// # Errors
    ///
    /// Exact reference rejection reasons (terminal state, task/revision
    /// binding, id pattern, sequencing rules, revision cap, invalid
    /// candidate content).
    #[allow(clippy::too_many_lines)]
    pub fn set_plan(
        &mut self,
        plan: crate::planning::TaskPlan,
    ) -> Result<(), String> {
        if let Some(reason) = self.record.terminal_reason() {
            return Err(reason);
        }
        if plan.task_id != self.record.id {
            return Err(format!(
                "Plan {} belongs to task {}, not {}.",
                plan.id, plan.task_id, self.record.id
            ));
        }
        if plan.task_contract_revision != self.record.contract.revision() {
            return Err(format!(
                "Plan {} binds to TaskContract revision {}, but the current revision is {}.",
                plan.id,
                plan.task_contract_revision,
                self.record.contract.revision()
            ));
        }
        if !crate::planning::is_valid_plan_id(&plan.id) {
            return Err(format!("Invalid plan id: {}", plan.id));
        }
        if plan.revision < 1 {
            return Err(
                "A plan revision must be a positive safe integer.".to_owned()
            );
        }
        if plan.created_at < 0 {
            return Err(
                "A plan requires a valid createdAt timestamp.".to_owned()
            );
        }
        let validated = crate::planning::validate_plan_candidate(
            &candidate_value_of_plan(&plan),
            &crate::planning::PlanCandidateContext {
                contract: &self.record.contract,
                depth: plan.depth,
            },
        );
        let content = match validated {
            crate::planning::PlanCandidateResult::Ok(content) => *content,
            crate::planning::PlanCandidateResult::Rejected(reasons) => {
                return Err(format!(
                    "The plan is invalid: {}",
                    reasons.join(" ")
                ));
            }
        };
        let previous = self.record.plans.last().cloned();
        if previous.is_none() && plan.revision != 1 {
            return Err("The first plan revision must be 1.".to_owned());
        }
        if let Some(previous) = &previous {
            if previous.id == plan.id && plan.revision != previous.revision + 1
            {
                return Err(format!(
                    "Plan {} revision {} does not follow revision {}; plans are immutable and revisions only ever advance by one.",
                    plan.id, plan.revision, previous.revision
                ));
            }
        }
        if let Some(previous) = &previous {
            if previous.id != plan.id {
                if plan.revision != 1 {
                    return Err(format!(
                        "Replacement plan {} must begin at revision 1.",
                        plan.id
                    ));
                }
                if self.record.plans.iter().any(|entry| entry.id == plan.id) {
                    return Err(format!(
                        "Plan id {} was already used by this task and cannot be restarted.",
                        plan.id
                    ));
                }
            }
        }
        if self.record.plans.len()
            >= crate::planning::PlanningLimits::MAX_PLAN_REVISIONS
        {
            return Err(format!(
                "The task already holds the maximum of {} plan revisions; replanning is not possible within this bound.",
                crate::planning::PlanningLimits::MAX_PLAN_REVISIONS
            ));
        }
        let prior_approved =
            self.record.plan_approval.as_ref().is_some_and(|approval| {
                approval.plan_id != plan.id
                    || approval.plan_revision != plan.revision
            });
        let stored_plan = crate::planning::TaskPlan { content, ..plan };
        self.record.state.plan.plan_id = Some(stored_plan.id.clone());
        self.record.state.plan.plan_revision = stored_plan.revision;
        self.record.state.plan.plan_digest =
            Some(stored_plan.digest.value.clone());
        self.record.state.plan.depth = stored_plan.depth;
        self.record.state.plan.state =
            crate::planning::TaskPlanStateKind::Current;
        self.record.state.plan.stale_reason = None;
        if prior_approved {
            self.record.state.plan.approval =
                crate::planning::TaskPlanApprovalKind::Invalidated;
            let approval =
                self.record.plan_approval.clone().expect("checked some");
            let mut event = ActivityEvent::PlanInvalidated {
                sequence: 0,
                plan_id: approval.plan_id,
                revision: approval.plan_revision,
                reason:
                    "The plan identity or revision advanced; the previous approval no longer applies."
                        .to_owned(),
            };
            self.append_activity(&mut event);
        } else {
            self.record.state.plan.approval =
                crate::planning::TaskPlanApprovalKind::None;
        }
        self.record.plan_approval = None;
        let mut created = ActivityEvent::PlanCreated {
            sequence: 0,
            plan_id: stored_plan.id.clone(),
            revision: stored_plan.revision,
            depth: stored_plan.depth,
        };
        self.append_activity(&mut created);
        self.observe(HostObservation {
            action: "plan.created".to_owned(),
            fingerprint: format!(
                "{}:{}",
                stored_plan.id, stored_plan.revision
            ),
            progress: true,
        });
        self.record.plans.push(stored_plan);
        Ok(())
    }

    /// Bind approval to the exact current plan and contract revisions.
    ///
    /// # Errors
    ///
    /// Exact reference rejection reasons.
    pub fn approve_plan(
        &mut self,
        plan_id: &str,
        plan_revision: u64,
    ) -> Result<(), String> {
        if let Some(reason) = self.record.terminal_reason() {
            return Err(reason);
        }
        let Some(current) = self.record.plans.last().cloned() else {
            return Err(format!(
                "No current plan matches {plan_id}; nothing was approved."
            ));
        };
        if current.id != plan_id {
            return Err(format!(
                "No current plan matches {plan_id}; nothing was approved."
            ));
        }
        if current.revision != plan_revision {
            return Err(format!(
                "Approval binds to the exact plan revision: plan {} is revision {}, not {}; the stale approval is refused.",
                plan_id, current.revision, plan_revision
            ));
        }
        if current.task_contract_revision != self.record.contract.revision() {
            return Err(format!(
                "Plan {} binds to TaskContract revision {}, which is no longer current; the approval is refused.",
                plan_id, current.task_contract_revision
            ));
        }
        if current.task_contract_digest != self.record.contract.digest() {
            return Err(format!(
                "Plan {} binds to a different TaskContract content digest; the approval is refused.",
                plan_id
            ));
        }
        let recomputed = crate::planning::stored_plan_digest(&current)?;
        if current.digest.value != recomputed {
            return Err(format!(
                "Plan {} content does not match its own identity digest; the approval is refused.",
                plan_id
            ));
        }
        if self.record.state.plan.state
            == crate::planning::TaskPlanStateKind::Stale
        {
            return Err(
                "The current plan is stale and cannot be approved.".to_owned()
            );
        }
        self.record.plan_approval = Some(crate::planning::PlanApproval {
            plan_id: current.id.clone(),
            plan_revision: current.revision,
            plan_digest: current.digest.value.clone(),
            task_contract_revision: current.task_contract_revision,
            task_contract_digest: current.task_contract_digest.clone(),
            approved_at: (self.clock)(),
        });
        self.record.state.plan.approval =
            crate::planning::TaskPlanApprovalKind::Approved;
        let mut event = ActivityEvent::PlanApproved {
            sequence: 0,
            plan_id: current.id.clone(),
            revision: current.revision,
            digest: current.digest.value,
        };
        self.append_activity(&mut event);
        Ok(())
    }

    /// Mark the current plan stale and its approval invalid.
    pub fn invalidate_plan(&mut self, reason: &str) {
        if self.record.terminal_reason().is_some() {
            return;
        }
        let Some(current) = self.record.plans.last().cloned() else {
            return;
        };
        self.record.state.plan.state =
            crate::planning::TaskPlanStateKind::Stale;
        self.record.state.plan.stale_reason = Some(reason.to_owned());
        if self.record.plan_approval.is_some() {
            self.record.state.plan.approval =
                crate::planning::TaskPlanApprovalKind::Invalidated;
        }
        self.record.plan_approval = None;
        let mut event = ActivityEvent::PlanInvalidated {
            sequence: 0,
            plan_id: current.id,
            revision: current.revision,
            reason: reason.to_owned(),
        };
        self.append_activity(&mut event);
    }

    /// Detached copy of the current plan revision, when any.
    pub fn current_plan(&self) -> Option<crate::planning::TaskPlan> {
        self.record.plans.last().cloned()
    }

    /// Detached copies of every stored plan revision (oldest first).
    pub fn plan_revisions(&self) -> Vec<crate::planning::TaskPlan> {
        self.record.plans.clone()
    }
}

/// Raw JSON view of a stored plan for candidate validation (identity keys
/// are ignored by validation, exactly like the reference). Optional plan
fn candidate_value_of_plan(
    plan: &crate::planning::TaskPlan,
) -> serde_json::Value {
    crate::planning::content_candidate_value(&plan.content)
}
