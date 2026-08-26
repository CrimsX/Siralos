//! Host-owned structured planning model (Stage 3 milestone 7, ADR 0020;
//! Stage 3R R13.4 differential parity).
//!
//! Planning is a runtime-owned phase: the host decides whether planning is
//! needed and at what depth; the planner is a read-only advisor; the plan
//! is an immutable revisioned artifact bound to exactly one TaskContract
//! revision; and plan approval never authorizes source edits or commands.
//! Plans carry only structured content and public rationale, and they
//! grant no capability: there is no capability/policy surface in the
//! model, validation rejects policy-shaped claims, and security policy
//! remains authoritative outside planning.

use std::collections::BTreeMap;

use crate::identity::{
    ArtifactDigest, CanonicalValue, compute_artifact_digest,
};
use crate::task::identity::TASK_PLAN_IDENTITY_SCHEMA;
use crate::task::{TaskContract, VerificationKind};

/// Host-routed planning depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanningDepth {
    /// No plan.
    None,
    /// Compact plan.
    Light,
    /// Full plan.
    Full,
}

impl PlanningDepth {
    /// Stable machine-readable spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            PlanningDepth::None => "none",
            PlanningDepth::Light => "light",
            PlanningDepth::Full => "full",
        }
    }
}

/// Whether a touchpoint was inspected (`verified`) or is likely but
/// unconfirmed (`candidate`). Guesses are never promoted to verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchpointConfidence {
    /// Inspected against an exact workspace revision.
    Verified,
    /// Likely relevant; may use glob paths.
    Candidate,
}

impl TouchpointConfidence {
    /// Stable machine-readable spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            TouchpointConfidence::Verified => "verified",
            TouchpointConfidence::Candidate => "candidate",
        }
    }
}

/// Risk severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanRiskSeverity {
    /// Low.
    Low,
    /// Medium.
    Medium,
    /// High.
    High,
}

impl PlanRiskSeverity {
    /// Stable machine-readable spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            PlanRiskSeverity::Low => "low",
            PlanRiskSeverity::Medium => "medium",
            PlanRiskSeverity::High => "high",
        }
    }
}

/// What the plan intends to cover and deliberately does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanScope {
    /// Bounded covered statements.
    pub in_scope: Vec<String>,
    /// Bounded uncovered statements.
    pub out_of_scope: Vec<String>,
}

/// One inspected or likely touchpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanTouchpoint {
    /// Touchpoint identifier referenced by steps.
    pub id: String,
    /// Workspace-relative path; candidates may use globs.
    pub path: String,
    /// Verified or candidate confidence.
    pub confidence: TouchpointConfidence,
    /// Required `rev_` + 32 hex handle for verified touchpoints.
    pub revision: Option<String>,
    /// Bounded `kind:ref` evidence reference.
    pub evidence: Option<String>,
    /// Bounded note.
    pub note: Option<String>,
}

/// One plan constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanConstraint {
    /// Stable id.
    pub id: String,
    /// Bounded description.
    pub description: String,
}

/// One plan risk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRisk {
    /// Stable id.
    pub id: String,
    /// Severity.
    pub severity: PlanRiskSeverity,
    /// Bounded description.
    pub description: String,
}

/// One plan step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanStep {
    /// Stable id.
    pub id: String,
    /// Bounded title.
    pub title: String,
    /// Optional bounded description.
    pub description: Option<String>,
    /// Ids of plan touchpoints this step touches.
    pub expected_touchpoints: Vec<String>,
    /// Ids of TaskContract criteria this step helps satisfy.
    pub verification: Option<Vec<String>>,
}

/// Plan validation strategy; `requirements` are descriptive only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanValidationStrategy {
    /// Bounded primary checks.
    pub checks: Vec<String>,
    /// Descriptive requirements; they never grant anything.
    pub requirements: Option<Vec<String>>,
}

/// Plan rollback strategy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRollbackStrategy {
    /// Bounded description.
    pub description: String,
}

/// Planner-supplied plan content; identity is host-owned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskPlanContent {
    /// Bounded objective.
    pub objective: String,
    /// Scope statements.
    pub scope: PlanScope,
    /// Bounded non-goals.
    pub non_goals: Vec<String>,
    /// Touchpoints.
    pub touchpoints: Vec<PlanTouchpoint>,
    /// Constraints.
    pub constraints: Vec<PlanConstraint>,
    /// Risks.
    pub risks: Vec<PlanRisk>,
    /// Steps.
    pub steps: Vec<PlanStep>,
    /// Validation strategy.
    pub validation: PlanValidationStrategy,
    /// Optional rollback strategy.
    pub rollback: Option<PlanRollbackStrategy>,
    /// Concise public rationale; never private model reasoning.
    pub rationale: Option<String>,
}

/// Immutable host-owned planning artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskPlan {
    /// Stable plan id.
    pub id: String,
    /// Immutable revision identity; starts at 1 and only increases.
    pub revision: u64,
    /// Exact content identity of this revision (identity fields excluded).
    pub digest: ArtifactDigest,
    /// Owning task id.
    pub task_id: String,
    /// Exact TaskContract revision this plan was created against.
    pub task_contract_revision: u64,
    /// Exact content digest of the bound TaskContract.
    pub task_contract_digest: String,
    /// Routed depth (light or full).
    pub depth: PlanningDepth,
    /// Plan content.
    pub content: TaskPlanContent,
    /// Creation timestamp (host clock).
    pub created_at: i64,
}

/// Plan approval binding: exact plan digest AND exact contract digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanApproval {
    /// Approved plan id.
    pub plan_id: String,
    /// Approved plan revision.
    pub plan_revision: u64,
    /// Exact content digest of the approved revision.
    pub plan_digest: String,
    /// Bound contract revision.
    pub task_contract_revision: u64,
    /// Bound contract content digest.
    pub task_contract_digest: String,
    /// Approval timestamp (host clock).
    pub approved_at: i64,
}

/// Bounded current-plan reference carried in materialized task state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskPlanState {
    /// Current plan id, when any.
    pub plan_id: Option<String>,
    /// 0 when no plan exists.
    pub plan_revision: u64,
    /// Exact digest of the current plan, when any.
    pub plan_digest: Option<String>,
    /// Routed planning depth.
    pub depth: PlanningDepth,
    /// Current/staleness of the plan reference.
    pub state: TaskPlanStateKind,
    /// Approval state.
    pub approval: TaskPlanApprovalKind,
    /// Exact stale reason when stale.
    pub stale_reason: Option<String>,
}

/// Plan-reference lifecycle kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskPlanStateKind {
    /// No plan.
    None,
    /// Current plan.
    Current,
    /// Stale plan.
    Stale,
}

impl TaskPlanStateKind {
    /// Stable machine-readable spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            TaskPlanStateKind::None => "none",
            TaskPlanStateKind::Current => "current",
            TaskPlanStateKind::Stale => "stale",
        }
    }
}

/// Plan-approval state kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskPlanApprovalKind {
    /// No approval.
    None,
    /// Approved.
    Approved,
    /// Invalidated.
    Invalidated,
}

impl TaskPlanApprovalKind {
    /// Stable machine-readable spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            TaskPlanApprovalKind::None => "none",
            TaskPlanApprovalKind::Approved => "approved",
            TaskPlanApprovalKind::Invalidated => "invalidated",
        }
    }
}

/// The initial no-plan state.
pub const NO_TASK_PLAN: TaskPlanState = TaskPlanState {
    plan_id: None,
    plan_revision: 0,
    plan_digest: None,
    depth: PlanningDepth::None,
    state: TaskPlanStateKind::None,
    approval: TaskPlanApprovalKind::None,
    stale_reason: None,
};

/// Deterministic plan bounds; provider output and user configuration can
/// never raise them, and an oversized candidate is rejected, not trimmed.
pub struct PlanningLimits;

impl PlanningLimits {
    /// Maximum plan steps (full plans).
    pub const MAX_STEPS: usize = 12;
    /// Maximum plan steps (light plans).
    pub const MAX_STEPS_LIGHT: usize = 6;
    /// Maximum touchpoints.
    pub const MAX_TOUCHPOINTS: usize = 24;
    /// Maximum constraints.
    pub const MAX_CONSTRAINTS: usize = 12;
    /// Maximum risks.
    pub const MAX_RISKS: usize = 12;
    /// Maximum non-goals.
    pub const MAX_NON_GOALS: usize = 16;
    /// Maximum entries per scope direction.
    pub const MAX_SCOPE_ENTRIES: usize = 16;
    /// Maximum validation checks.
    pub const MAX_VALIDATION_CHECKS: usize = 12;
    /// Maximum validation requirements.
    pub const MAX_VALIDATION_REQUIREMENTS: usize = 8;
    /// Maximum expected-touchpoint refs per step.
    pub const MAX_EXPECTED_TOUCHPOINTS_PER_STEP: usize = 12;
    /// Maximum verification refs per step.
    pub const MAX_VERIFICATION_REFS_PER_STEP: usize = 12;
    /// Maximum serialized plan content bytes.
    pub const MAX_PLAN_CONTENT_BYTES: usize = 32 * 1024;
    /// Maximum objective bytes.
    pub const MAX_OBJECTIVE_BYTES: usize = 2048;
    /// Maximum statement bytes.
    pub const MAX_STATEMENT_BYTES: usize = 512;
    /// Maximum step-title bytes.
    pub const MAX_STEP_TITLE_BYTES: usize = 256;
    /// Maximum step-description bytes.
    pub const MAX_STEP_DESCRIPTION_BYTES: usize = 1024;
    /// Maximum rollback bytes.
    pub const MAX_ROLLBACK_BYTES: usize = 1024;
    /// Maximum rationale bytes.
    pub const MAX_RATIONALE_BYTES: usize = 1024;
    /// Maximum touchpoint-path bytes.
    pub const MAX_PATH_BYTES: usize = 1024;
    /// Maximum evidence bytes.
    pub const MAX_EVIDENCE_BYTES: usize = 256;
    /// Maximum revision-handle bytes.
    pub const MAX_REVISION_BYTES: usize = 128;
    /// Maximum note bytes.
    pub const MAX_NOTE_BYTES: usize = 512;
    /// Maximum plan revisions one task may accumulate.
    pub const MAX_PLAN_REVISIONS: usize = 16;
}

/// `^plan-[A-Za-z0-9._-]{1,95}$`
pub fn is_valid_plan_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("plan-") else {
        return false;
    };
    !rest.is_empty()
        && rest.len() <= 95
        && rest.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
        })
}

/// `^[A-Za-z][A-Za-z0-9._-]{0,63}$` (step/touchpoint/constraint/risk ids).
pub fn is_valid_plan_element_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_alphabetic()
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
        })
}

/// `^rev_[0-9a-f]{32}$` — opaque workspace revision handles.
pub fn is_valid_revision_handle(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("rev_") else {
        return false;
    };
    rest.len() == 32
        && rest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn is_sha256_hex(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 64
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// Input to [`create_task_plan`].
pub struct CreateTaskPlanInput {
    /// Stable plan id.
    pub id: String,
    /// Owning task id.
    pub task_id: String,
    /// Contract revision bound at creation.
    pub task_contract_revision: u64,
    /// Exact contract content digest.
    pub task_contract_digest: String,
    /// Routed depth.
    pub depth: PlanningDepth,
    /// Validated plan content.
    pub content: TaskPlanContent,
    /// Creation timestamp.
    pub created_at: i64,
}

/// Canonical content payload of a plan (identity fields excluded),
/// matching `taskPlanContentPayload` key-for-key.
pub fn plan_content_payload(content: &TaskPlanContent) -> CanonicalValue {
    let mut sections: BTreeMap<String, CanonicalValue> = BTreeMap::new();
    sections.insert(
        "objective".to_owned(),
        CanonicalValue::Str(content.objective.clone()),
    );
    sections.insert(
        "scope".to_owned(),
        CanonicalValue::Object(BTreeMap::from([
            (
                "inScope".to_owned(),
                CanonicalValue::Array(
                    content
                        .scope
                        .in_scope
                        .iter()
                        .map(|value| CanonicalValue::Str(value.clone()))
                        .collect(),
                ),
            ),
            (
                "outOfScope".to_owned(),
                CanonicalValue::Array(
                    content
                        .scope
                        .out_of_scope
                        .iter()
                        .map(|value| CanonicalValue::Str(value.clone()))
                        .collect(),
                ),
            ),
        ])),
    );
    sections.insert(
        "nonGoals".to_owned(),
        CanonicalValue::Array(
            content
                .non_goals
                .iter()
                .map(|value| CanonicalValue::Str(value.clone()))
                .collect(),
        ),
    );
    sections.insert(
        "touchpoints".to_owned(),
        CanonicalValue::Array(
            content
                .touchpoints
                .iter()
                .map(|touchpoint| {
                    let mut entry: BTreeMap<String, CanonicalValue> =
                        BTreeMap::new();
                    entry.insert(
                        "id".to_owned(),
                        CanonicalValue::Str(touchpoint.id.clone()),
                    );
                    entry.insert(
                        "path".to_owned(),
                        CanonicalValue::Str(touchpoint.path.clone()),
                    );
                    entry.insert(
                        "confidence".to_owned(),
                        CanonicalValue::Str(
                            touchpoint.confidence.as_str().to_owned(),
                        ),
                    );
                    if let Some(revision) = &touchpoint.revision {
                        entry.insert(
                            "revision".to_owned(),
                            CanonicalValue::Str(revision.clone()),
                        );
                    }
                    if let Some(evidence) = &touchpoint.evidence {
                        entry.insert(
                            "evidence".to_owned(),
                            CanonicalValue::Str(evidence.clone()),
                        );
                    }
                    if let Some(note) = &touchpoint.note {
                        entry.insert(
                            "note".to_owned(),
                            CanonicalValue::Str(note.clone()),
                        );
                    }
                    CanonicalValue::Object(entry)
                })
                .collect(),
        ),
    );
    sections.insert(
        "constraints".to_owned(),
        CanonicalValue::Array(
            content
                .constraints
                .iter()
                .map(|constraint| {
                    CanonicalValue::Object(BTreeMap::from([
                        (
                            "id".to_owned(),
                            CanonicalValue::Str(constraint.id.clone()),
                        ),
                        (
                            "description".to_owned(),
                            CanonicalValue::Str(
                                constraint.description.clone(),
                            ),
                        ),
                    ]))
                })
                .collect(),
        ),
    );
    sections.insert(
        "risks".to_owned(),
        CanonicalValue::Array(
            content
                .risks
                .iter()
                .map(|risk| {
                    CanonicalValue::Object(BTreeMap::from([
                        (
                            "id".to_owned(),
                            CanonicalValue::Str(risk.id.clone()),
                        ),
                        (
                            "severity".to_owned(),
                            CanonicalValue::Str(
                                risk.severity.as_str().to_owned(),
                            ),
                        ),
                        (
                            "description".to_owned(),
                            CanonicalValue::Str(risk.description.clone()),
                        ),
                    ]))
                })
                .collect(),
        ),
    );
    sections.insert(
        "steps".to_owned(),
        CanonicalValue::Array(
            content
                .steps
                .iter()
                .map(|step| {
                    let mut entry: BTreeMap<String, CanonicalValue> =
                        BTreeMap::new();
                    entry.insert(
                        "id".to_owned(),
                        CanonicalValue::Str(step.id.clone()),
                    );
                    entry.insert(
                        "title".to_owned(),
                        CanonicalValue::Str(step.title.clone()),
                    );
                    if let Some(description) = &step.description {
                        entry.insert(
                            "description".to_owned(),
                            CanonicalValue::Str(description.clone()),
                        );
                    }
                    entry.insert(
                        "expectedTouchpoints".to_owned(),
                        CanonicalValue::Array(
                            step.expected_touchpoints
                                .iter()
                                .map(|value| {
                                    CanonicalValue::Str(value.clone())
                                })
                                .collect(),
                        ),
                    );
                    if let Some(verification) = &step.verification {
                        entry.insert(
                            "verification".to_owned(),
                            CanonicalValue::Array(
                                verification
                                    .iter()
                                    .map(|value| {
                                        CanonicalValue::Str(value.clone())
                                    })
                                    .collect(),
                            ),
                        );
                    }
                    CanonicalValue::Object(entry)
                })
                .collect(),
        ),
    );
    let mut validation: BTreeMap<String, CanonicalValue> = BTreeMap::new();
    validation.insert(
        "checks".to_owned(),
        CanonicalValue::Array(
            content
                .validation
                .checks
                .iter()
                .map(|value| CanonicalValue::Str(value.clone()))
                .collect(),
        ),
    );
    if let Some(requirements) = &content.validation.requirements {
        validation.insert(
            "requirements".to_owned(),
            CanonicalValue::Array(
                requirements
                    .iter()
                    .map(|value| CanonicalValue::Str(value.clone()))
                    .collect(),
            ),
        );
    }
    sections
        .insert("validation".to_owned(), CanonicalValue::Object(validation));
    if let Some(rollback) = &content.rollback {
        sections.insert(
            "rollback".to_owned(),
            CanonicalValue::Object(BTreeMap::from([(
                "description".to_owned(),
                CanonicalValue::Str(rollback.description.clone()),
            )])),
        );
    }
    if let Some(rationale) = &content.rationale {
        sections.insert(
            "rationale".to_owned(),
            CanonicalValue::Str(rationale.clone()),
        );
    }
    CanonicalValue::Object(sections)
}

/// Create the first immutable plan revision. Identity is host-owned and
/// assigned only after the structural checks pass.
///
/// # Errors
///
/// Exact reference messages for invalid ids, revisions, digests, depths,
/// and timestamps.
pub fn create_task_plan(
    input: CreateTaskPlanInput,
) -> Result<TaskPlan, String> {
    if !is_valid_plan_id(&input.id) {
        return Err(format!("Invalid plan id: {}", input.id));
    }
    if input.task_contract_revision < 1 {
        return Err(
            "A plan requires a positive safe-integer task contract revision."
                .to_owned(),
        );
    }
    if !is_sha256_hex(&input.task_contract_digest) {
        return Err(
            "A plan requires the exact 64-hex TaskContract content digest it binds to."
                .to_owned(),
        );
    }
    let depth = match input.depth {
        PlanningDepth::Light => PlanningDepth::Light,
        PlanningDepth::Full => PlanningDepth::Full,
        PlanningDepth::None => {
            return Err("A plan requires depth light or full.".to_owned());
        }
    };
    if input.created_at < 0 {
        return Err(
            "A plan requires a non-negative safe-integer createdAt timestamp."
                .to_owned(),
        );
    }
    let plan_content = input.content.clone();
    let provisional = TaskPlan {
        id: input.id,
        revision: 1,
        digest: ArtifactDigest {
            algorithm: String::new(),
            artifact_type: String::new(),
            schema_version: 0,
            value: String::new(),
        },
        task_id: input.task_id,
        task_contract_revision: input.task_contract_revision,
        task_contract_digest: input.task_contract_digest,
        depth,
        content: plan_content.clone(),
        created_at: input.created_at,
    };
    let digest = compute_artifact_digest(
        "TaskPlan",
        TASK_PLAN_IDENTITY_SCHEMA,
        &plan_content_payload(&plan_content),
    )
    .map_err(|error| error.message)?;
    Ok(TaskPlan { digest, ..provisional })
}

/// Input to [`revise_task_plan`].
pub struct ReviseTaskPlanInput {
    /// New validated content.
    pub content: TaskPlanContent,
    /// Optional updated contract digest; defaults to the previous binding.
    pub task_contract_digest: Option<String>,
}

/// Produce the next immutable plan revision; the previous revision is
/// untouched and any existing approval is invalid by construction.
///
/// # Errors
///
/// Exact reference messages mirrored from the TypeScript reference.
pub fn revise_task_plan(
    previous: &TaskPlan,
    changes: &ReviseTaskPlanInput,
) -> Result<TaskPlan, String> {
    if !is_valid_plan_id(&previous.id) {
        return Err(format!("Invalid plan id: {}", previous.id));
    }
    if previous.revision < 1 || previous.revision >= u64::MAX - 1 {
        return Err(
            "A previous plan revision must be a positive incrementable safe integer."
                .to_owned(),
        );
    }
    if previous.task_contract_revision < 1 {
        return Err(
            "A previous plan requires a positive safe-integer task contract revision."
                .to_owned(),
        );
    }
    if previous.created_at < 0 {
        return Err(
            "A previous plan requires a non-negative safe-integer createdAt timestamp."
                .to_owned(),
        );
    }
    if !matches!(previous.depth, PlanningDepth::Light | PlanningDepth::Full) {
        return Err("A previous plan requires depth light or full.".to_owned());
    }
    let task_contract_digest = changes
        .task_contract_digest
        .clone()
        .unwrap_or_else(|| previous.task_contract_digest.clone());
    if !is_sha256_hex(&task_contract_digest) {
        return Err(
            "A plan requires the exact 64-hex TaskContract content digest it binds to."
                .to_owned(),
        );
    }
    let content = changes.content.clone();
    let digest = compute_artifact_digest(
        "TaskPlan",
        TASK_PLAN_IDENTITY_SCHEMA,
        &plan_content_payload(&content),
    )
    .map_err(|error| error.message)?;
    Ok(TaskPlan {
        id: previous.id.clone(),
        revision: previous.revision + 1,
        digest,
        task_id: previous.task_id.clone(),
        task_contract_revision: previous.task_contract_revision,
        task_contract_digest,
        depth: previous.depth,
        content,
        created_at: previous.created_at,
    })
}

/// Hex content digest of a plan revision.
///
/// # Errors
///
/// Propagates identity failures.
pub fn compute_plan_revision_digest(
    plan: &TaskPlan,
) -> Result<String, String> {
    Ok(compute_artifact_digest(
        "TaskPlan",
        TASK_PLAN_IDENTITY_SCHEMA,
        &plan_content_payload(&plan.content),
    )
    .map_err(|error| error.message)?
    .value)
}

/// Compact deterministic description of a plan's public shape.
pub fn summarize_plan(plan: &TaskPlan) -> String {
    let verified = plan
        .content
        .touchpoints
        .iter()
        .filter(|touchpoint| {
            touchpoint.confidence == TouchpointConfidence::Verified
        })
        .count();
    let candidates = plan.content.touchpoints.len() - verified;
    format!(
        "{} rev {}, {} steps, {} verified / {} candidate touchpoints",
        plan.depth.as_str(),
        plan.revision,
        plan.content.steps.len(),
        verified,
        candidates
    )
}

/// Whether a contract carries meaningful acceptance criteria for
/// full-plan execution: at least two criteria, at least one
/// host-verifiable (non-user) criterion.
pub fn has_meaningful_acceptance_criteria(contract: &TaskContract) -> bool {
    let criteria = contract.acceptance_criteria();
    if criteria.len() < 2 {
        return false;
    }
    criteria.iter().any(|criterion| {
        criterion.verification_kind() != VerificationKind::User
    })
}

/// Recompute the exact content digest of a stored plan revision.
///
/// # Errors
///
/// Propagates identity failures.
pub fn stored_plan_digest(plan: &TaskPlan) -> Result<String, String> {
    Ok(compute_artifact_digest(
        "TaskPlan",
        TASK_PLAN_IDENTITY_SCHEMA,
        &plan_content_payload(&plan.content),
    )
    .map_err(|error| error.message)?
    .value)
}

/// Raw JSON view of validated plan content for untrusted-candidate
/// revalidation. Optional fields stay absent rather than null, matching
/// the reference candidate shape exactly.
pub fn content_candidate_value(
    content: &TaskPlanContent,
) -> serde_json::Value {
    let touchpoints: Vec<serde_json::Value> = content
        .touchpoints
        .iter()
        .map(|touchpoint| {
            let mut entry = serde_json::Map::new();
            entry.insert("id".to_owned(), serde_json::json!(touchpoint.id));
            entry
                .insert("path".to_owned(), serde_json::json!(touchpoint.path));
            entry.insert(
                "confidence".to_owned(),
                serde_json::json!(touchpoint.confidence.as_str()),
            );
            if let Some(revision) = &touchpoint.revision {
                entry.insert(
                    "revision".to_owned(),
                    serde_json::json!(revision),
                );
            }
            if let Some(evidence) = &touchpoint.evidence {
                entry.insert(
                    "evidence".to_owned(),
                    serde_json::json!(evidence),
                );
            }
            if let Some(note) = &touchpoint.note {
                entry.insert("note".to_owned(), serde_json::json!(note));
            }
            serde_json::Value::Object(entry)
        })
        .collect();
    let constraints: Vec<serde_json::Value> = content
        .constraints
        .iter()
        .map(|constraint| {
            serde_json::json!({
                "id": constraint.id,
                "description": constraint.description,
            })
        })
        .collect();
    let risks: Vec<serde_json::Value> = content
        .risks
        .iter()
        .map(|risk| {
            serde_json::json!({
                "id": risk.id,
                "severity": risk.severity.as_str(),
                "description": risk.description,
            })
        })
        .collect();
    let steps: Vec<serde_json::Value> = content
        .steps
        .iter()
        .map(|step| {
            let mut entry = serde_json::Map::new();
            entry.insert("id".to_owned(), serde_json::json!(step.id));
            entry.insert("title".to_owned(), serde_json::json!(step.title));
            if let Some(description) = &step.description {
                entry.insert(
                    "description".to_owned(),
                    serde_json::json!(description),
                );
            }
            entry.insert(
                "expectedTouchpoints".to_owned(),
                serde_json::json!(step.expected_touchpoints),
            );
            if let Some(verification) = &step.verification {
                entry.insert(
                    "verification".to_owned(),
                    serde_json::json!(verification),
                );
            }
            serde_json::Value::Object(entry)
        })
        .collect();
    let mut validation = serde_json::Map::new();
    validation.insert(
        "checks".to_owned(),
        serde_json::json!(content.validation.checks),
    );
    if let Some(requirements) = &content.validation.requirements {
        validation.insert(
            "requirements".to_owned(),
            serde_json::json!(requirements),
        );
    }
    let mut aggregate = serde_json::Map::new();
    aggregate
        .insert("objective".to_owned(), serde_json::json!(content.objective));
    aggregate.insert(
        "scope".to_owned(),
        serde_json::json!({
            "inScope": content.scope.in_scope,
            "outOfScope": content.scope.out_of_scope,
        }),
    );
    aggregate
        .insert("nonGoals".to_owned(), serde_json::json!(content.non_goals));
    aggregate.insert(
        "touchpoints".to_owned(),
        serde_json::Value::Array(touchpoints),
    );
    aggregate.insert(
        "constraints".to_owned(),
        serde_json::Value::Array(constraints),
    );
    aggregate.insert("risks".to_owned(), serde_json::Value::Array(risks));
    aggregate.insert("steps".to_owned(), serde_json::Value::Array(steps));
    aggregate.insert(
        "validation".to_owned(),
        serde_json::Value::Object(validation),
    );
    if let Some(rollback) = &content.rollback {
        aggregate.insert(
            "rollback".to_owned(),
            serde_json::json!({ "description": rollback.description }),
        );
    }
    if let Some(rationale) = &content.rationale {
        aggregate.insert("rationale".to_owned(), serde_json::json!(rationale));
    }
    serde_json::Value::Object(aggregate)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content() -> TaskPlanContent {
        TaskPlanContent {
            objective: "Implement the feature".to_owned(),
            scope: PlanScope {
                in_scope: vec!["src/a.ts".to_owned()],
                out_of_scope: vec!["docs".to_owned()],
            },
            non_goals: vec!["no public API change".to_owned()],
            touchpoints: vec![
                PlanTouchpoint {
                    id: "tp1".to_owned(),
                    path: "src/a.ts".to_owned(),
                    confidence: TouchpointConfidence::Verified,
                    revision: Some(
                        "rev_a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6".to_owned(),
                    ),
                    evidence: Some("read:src/a.ts".to_owned()),
                    note: None,
                },
                PlanTouchpoint {
                    id: "tp2".to_owned(),
                    path: "src/b*.ts".to_owned(),
                    confidence: TouchpointConfidence::Candidate,
                    revision: None,
                    evidence: None,
                    note: None,
                },
            ],
            constraints: vec![PlanConstraint {
                id: "con1".to_owned(),
                description: "stay within scope".to_owned(),
            }],
            risks: vec![PlanRisk {
                id: "risk1".to_owned(),
                severity: PlanRiskSeverity::Low,
                description: "minor regression risk".to_owned(),
            }],
            steps: vec![
                PlanStep {
                    id: "s1".to_owned(),
                    title: "Edit a".to_owned(),
                    description: None,
                    expected_touchpoints: vec!["tp1".to_owned()],
                    verification: Some(vec!["ac1".to_owned()]),
                },
                PlanStep {
                    id: "s2".to_owned(),
                    title: "Verify b".to_owned(),
                    description: None,
                    expected_touchpoints: vec!["tp2".to_owned()],
                    verification: Some(vec!["ac2".to_owned()]),
                },
            ],
            validation: PlanValidationStrategy {
                checks: vec!["check-only parse".to_owned()],
                requirements: Some(vec!["workspace mutation".to_owned()]),
            },
            rollback: Some(PlanRollbackStrategy {
                description: "revert commits".to_owned(),
            }),
            rationale: Some("straightforward".to_owned()),
        }
    }

    #[test]
    fn plan_digest_is_deterministic_and_content_sensitive() {
        let plan = create_task_plan(CreateTaskPlanInput {
            id: "plan-task-1".to_owned(),
            task_id: "task-1".to_owned(),
            task_contract_revision: 1,
            task_contract_digest: "a".repeat(64),
            depth: PlanningDepth::Full,
            content: content(),
            created_at: 1_700_000_000_000,
        })
        .expect("valid plan");
        assert_eq!(plan.digest.value.len(), 64);
        let revised = revise_task_plan(
            &plan,
            &ReviseTaskPlanInput {
                content: content(),
                task_contract_digest: None,
            },
        )
        .expect("valid revision");
        assert_eq!(revised.revision, 2);
        assert_eq!(
            revised.digest.value, plan.digest.value,
            "identical content shares the digest"
        );
    }

    #[test]
    fn plan_digest_matches_the_reference_canonical_form() {
        // Digest captured by running the TypeScript reference
        // createTaskPlan over this exact fixture (R13.4 differential
        // fixture parity).
        let plan = create_task_plan(CreateTaskPlanInput {
            id: "plan-task-1".to_owned(),
            task_id: "task-1".to_owned(),
            task_contract_revision: 1,
            task_contract_digest:
                "1111111111111111111111111111111111111111111111111111111111111111"
                    .to_owned(),
            depth: PlanningDepth::Full,
            content: content(),
            created_at: 1_700_000_000_000,
        })
        .expect("valid plan");
        assert_eq!(
            plan.digest.value,
            "e84d00888350bd05cafdcd1eba34bbc77f04a8cfc773e377c4970aebcdc94fa7"
        );
    }

    #[test]
    fn summary_reports_depth_revision_and_touchpoints() {
        let plan = create_task_plan(CreateTaskPlanInput {
            id: "plan-task-1".to_owned(),
            task_id: "task-1".to_owned(),
            task_contract_revision: 1,
            task_contract_digest: "a".repeat(64),
            depth: PlanningDepth::Full,
            content: content(),
            created_at: 1,
        })
        .expect("valid plan");
        assert_eq!(
            summarize_plan(&plan),
            "full rev 1, 2 steps, 1 verified / 1 candidate touchpoints"
        );
    }
}
