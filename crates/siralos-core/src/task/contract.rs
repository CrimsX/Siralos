//! Revisioned, immutable task contracts (Stage 3R R3).
//!
//! A task contract distinguishes what the user requested, the constraints
//! the runtime must respect, the explicit acceptance criteria completion
//! is evaluated against, and the pause policy. Contracts are immutable and
//! revisioned: a material change produces a new revision; revision N is
//! never mutated. The 'revision' is the lifecycle identity; the 'digest'
//! is the exact material content identity over the canonical payload with
//! the revision excluded (ADR 0028), so content-identical revisions share
//! a digest while their revision numbers differ.
//!
//! The TypeScript reference re-validates contracts at the runtime boundary
//! (validateTaskContract); in Rust the contract type is validated by
//! construction and its fields are private, so an already-constructed
//! TaskContract cannot carry an invalid shape.

use std::collections::BTreeMap;

use crate::identity::{
    CanonicalValue, TASK_CONTRACT_IDENTITY_SCHEMA, artifact_digest_hex,
};

/// How a criterion's satisfaction is established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationKind {
    /// Deterministic.
    Deterministic,
    /// Review.
    Review,
    /// User.
    User,
}

impl VerificationKind {
    /// Stable machine-readable form matching the TypeScript reference.
    pub fn as_str(self) -> &'static str {
        match self {
            VerificationKind::Deterministic => "deterministic",
            VerificationKind::Review => "review",
            VerificationKind::User => "user",
        }
    }
}

/// Task constraint kinds from the reference contract model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintKind {
    /// Scope.
    Scope,
    /// Process.
    Process,
    /// Security.
    Security,
    /// Escalation.
    Escalation,
}

impl ConstraintKind {
    /// Stable machine-readable form matching the TypeScript reference.
    pub fn as_str(self) -> &'static str {
        match self {
            ConstraintKind::Scope => "scope",
            ConstraintKind::Process => "process",
            ConstraintKind::Security => "security",
            ConstraintKind::Escalation => "escalation",
        }
    }
}

/// Conditions that require the runtime to pause the task and surface it
/// for user attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PausePolicy {
    /// None.
    None,
    /// On approval.
    OnApproval,
    /// On escalation.
    OnEscalation,
}

impl PausePolicy {
    /// Stable machine-readable form matching the TypeScript reference.
    pub fn as_str(self) -> &'static str {
        match self {
            PausePolicy::None => "none",
            PausePolicy::OnApproval => "on_approval",
            PausePolicy::OnEscalation => "on_escalation",
        }
    }
}

/// One acceptance criterion authored by the host runtime/workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceCriterion {
    pub(crate) id: String,
    pub(crate) description: String,
    pub(crate) verification_kind: VerificationKind,
}

impl AcceptanceCriterion {
    /// Construct an acceptance criterion; the enclosing contract
    /// validation trims and bounds the description.
    pub fn new(
        id: String,
        description: String,
        verification_kind: VerificationKind,
    ) -> Self {
        Self { id, description, verification_kind }
    }

    /// Validated criterion entry identifier.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Criterion description (trimmed at construction).
    pub fn description(&self) -> &str {
        &self.description
    }

    /// How this criterion's satisfaction is established.
    pub fn verification_kind(&self) -> VerificationKind {
        self.verification_kind
    }
}

/// One task constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskConstraint {
    pub(crate) id: String,
    pub(crate) description: String,
    pub(crate) kind: ConstraintKind,
}

impl TaskConstraint {
    /// Construct a task constraint; the enclosing contract validation
    /// trims and bounds the description.
    pub fn new(id: String, description: String, kind: ConstraintKind) -> Self {
        Self { id, description, kind }
    }

    /// Validated constraint entry identifier.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Constraint description (trimmed at construction).
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Constraint kind.
    pub fn kind(&self) -> ConstraintKind {
        self.kind
    }
}

/// Host-owned hard bounds for revisioned task contracts (exact reference
/// limits, UTF-8 byte lengths).
pub mod limits {
    /// Maximum TaskContract id bytes.
    pub const MAX_ID_BYTES: usize = 95;
    /// Maximum request UTF-8 bytes.
    pub const MAX_REQUEST_BYTES: usize = 16 * 1024;
    /// Maximum context UTF-8 bytes.
    pub const MAX_CONTEXT_BYTES: usize = 32 * 1024;
    /// Maximum constraint count.
    pub const MAX_CONSTRAINTS: usize = 32;
    /// Maximum acceptance criterion count.
    pub const MAX_ACCEPTANCE_CRITERIA: usize = 64;
    /// Maximum entry id bytes (criteria, constraints).
    pub const MAX_ENTRY_ID_BYTES: usize = 64;
    /// Maximum entry description UTF-8 bytes.
    pub const MAX_ENTRY_DESCRIPTION_BYTES: usize = 4096;
}

fn task_id_pattern_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > limits::MAX_ID_BYTES {
        return false;
    }
    if !bytes[0].is_ascii_alphanumeric() {
        return false;
    }
    bytes[1..].iter().all(|byte| {
        byte.is_ascii_alphanumeric()
            || *byte == b'.'
            || *byte == b'_'
            || *byte == b'-'
    })
}

fn entry_id_pattern_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > limits::MAX_ENTRY_ID_BYTES {
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

/// Typed failure of task-contract validation or revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractError {
    /// The id does not match the reference task-id pattern or byte bound.
    /// Invalid task contract id.
    InvalidTaskContractId,
    /// The trimmed request is empty.
    /// Empty request.
    EmptyRequest,
    /// The request exceeds the UTF-8 byte bound.
    /// Request too large.
    RequestTooLarge,
    /// The revision must be at least 1.
    /// Invalid revision.
    InvalidRevision,
    /// More than the maximum acceptance criteria were supplied.
    /// Too many acceptance criteria.
    TooManyAcceptanceCriteria,
    /// More than the maximum constraints were supplied.
    /// Too many constraints.
    TooManyConstraints,
    /// An acceptance criterion id is invalid.
    /// Invalid criterion id.
    InvalidCriterionId,
    /// An acceptance criterion id is duplicated.
    /// Duplicate criterion id.
    DuplicateCriterionId,
    /// An acceptance criterion description is empty after trimming.
    /// Empty criterion description.
    EmptyCriterionDescription,
    /// An acceptance criterion description exceeds the byte bound.
    /// Criterion description too large.
    CriterionDescriptionTooLarge,
    /// An acceptance criterion verification kind is unsupported.
    /// Invalid verification kind.
    InvalidVerificationKind,
    /// A contract requires at least one acceptance criterion.
    /// No acceptance criteria.
    NoAcceptanceCriteria,
    /// A task constraint id is invalid.
    /// Invalid constraint id.
    InvalidConstraintId,
    /// A task constraint id is duplicated.
    /// Duplicate constraint id.
    DuplicateConstraintId,
    /// A task constraint description is empty after trimming.
    /// Empty constraint description.
    EmptyConstraintDescription,
    /// A task constraint description exceeds the byte bound.
    /// Constraint description too large.
    ConstraintDescriptionTooLarge,
    /// A task constraint kind is unsupported.
    /// Invalid constraint kind.
    InvalidConstraintKind,
    /// The pause policy is unsupported.
    /// Invalid pause policy.
    InvalidPausePolicy,
    /// The context exceeds the UTF-8 byte bound.
    /// Context too large.
    ContextTooLarge,
    /// A revision must preserve the contract id.
    /// Revision id mismatch.
    RevisionIdMismatch,
}

impl ContractError {
    /// Stable machine-branchable code (differential observation
    /// vocabulary shared with the TypeScript oracle adapter).
    pub fn code(&self) -> &'static str {
        match self {
            ContractError::InvalidTaskContractId => "invalid_task_contract_id",
            ContractError::EmptyRequest => "empty_request",
            ContractError::RequestTooLarge => "request_too_large",
            ContractError::InvalidRevision => "invalid_revision",
            ContractError::TooManyAcceptanceCriteria => {
                "too_many_acceptance_criteria"
            }
            ContractError::TooManyConstraints => "too_many_constraints",
            ContractError::InvalidCriterionId => "invalid_criterion_id",
            ContractError::DuplicateCriterionId => "duplicate_criterion_id",
            ContractError::EmptyCriterionDescription => {
                "empty_criterion_description"
            }
            ContractError::CriterionDescriptionTooLarge => {
                "criterion_description_too_large"
            }
            ContractError::InvalidVerificationKind => {
                "invalid_verification_kind"
            }
            ContractError::NoAcceptanceCriteria => "no_acceptance_criteria",
            ContractError::InvalidConstraintId => "invalid_constraint_id",
            ContractError::DuplicateConstraintId => "duplicate_constraint_id",
            ContractError::EmptyConstraintDescription => {
                "empty_constraint_description"
            }
            ContractError::ConstraintDescriptionTooLarge => {
                "constraint_description_too_large"
            }
            ContractError::InvalidConstraintKind => "invalid_constraint_kind",
            ContractError::InvalidPausePolicy => "invalid_pause_policy",
            ContractError::ContextTooLarge => "context_too_large",
            ContractError::RevisionIdMismatch => "revision_id_mismatch",
        }
    }
}

impl std::fmt::Display for ContractError {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter.write_str(match self {
            ContractError::InvalidTaskContractId => "Invalid task contract id",
            ContractError::EmptyRequest => {
                "A task contract requires a non-empty request."
            }
            ContractError::RequestTooLarge => {
                "A task contract request exceeds the UTF-8 byte bound."
            }
            ContractError::InvalidRevision => {
                "A task contract revision must be at least 1."
            }
            ContractError::TooManyAcceptanceCriteria => {
                "A task contract accepts at most 64 acceptance criteria."
            }
            ContractError::TooManyConstraints => {
                "A task contract accepts at most 32 constraints."
            }
            ContractError::InvalidCriterionId => {
                "Invalid acceptance criterion id"
            }
            ContractError::DuplicateCriterionId => {
                "Duplicate acceptance criterion id"
            }
            ContractError::EmptyCriterionDescription => {
                "An acceptance criterion requires a non-empty description."
            }
            ContractError::CriterionDescriptionTooLarge => {
                "An acceptance criterion description exceeds the byte bound."
            }
            ContractError::InvalidVerificationKind => {
                "An acceptance criterion has an invalid verification kind."
            }
            ContractError::NoAcceptanceCriteria => {
                "A task contract requires at least one acceptance criterion."
            }
            ContractError::InvalidConstraintId => "Invalid task constraint id",
            ContractError::DuplicateConstraintId => {
                "Duplicate task constraint id"
            }
            ContractError::EmptyConstraintDescription => {
                "A task constraint requires a non-empty description."
            }
            ContractError::ConstraintDescriptionTooLarge => {
                "A task constraint description exceeds the byte bound."
            }
            ContractError::InvalidConstraintKind => {
                "A task constraint has an invalid kind."
            }
            ContractError::InvalidPausePolicy => "Invalid task pause policy",
            ContractError::ContextTooLarge => {
                "A task contract context exceeds the UTF-8 byte bound."
            }
            ContractError::RevisionIdMismatch => {
                "A task contract revision must preserve the contract id."
            }
        })
    }
}

impl std::error::Error for ContractError {}

/// Input for creating a new revision-1 task contract.
#[derive(Debug, Clone)]
pub struct CreateTaskContractInput {
    /// Validated task id.
    /// Id.
    pub id: String,
    /// Request text; trimmed and bounded at construction.
    /// Request.
    pub request: String,
    /// Optional context; empty values normalize to no context.
    /// Context.
    pub context: Option<String>,
    /// Constraints; defaults to empty when None.
    /// Constraints.
    pub constraints: Option<Vec<TaskConstraint>>,
    /// Acceptance criteria; at least one is required.
    /// Acceptance criteria.
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    /// Pause policy; defaults to none when None.
    /// Pause policy.
    pub pause_policy: Option<PausePolicy>,
}

/// How the context field is updated during a revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviseContext {
    /// Carry the previous revision's context over.
    /// Keep.
    Keep,
    /// Replace the context; an empty string normalizes to no context.
    /// Set.
    Set(String),
}

/// Input for producing the next immutable contract revision.
#[derive(Debug, Clone)]
pub struct ReviseTaskContractInput {
    /// The id must equal the current contract's id.
    /// Id.
    pub id: String,
    /// New request; None carries the previous value over.
    /// Request.
    pub request: Option<String>,
    /// New context or keep the previous value.
    /// Context.
    pub context: Option<ReviseContext>,
    /// New constraints or keep the previous value.
    /// Constraints.
    pub constraints: Option<Vec<TaskConstraint>>,
    /// New acceptance criteria or keep the previous value.
    /// Acceptance criteria.
    pub acceptance_criteria: Option<Vec<AcceptanceCriterion>>,
    /// New pause policy or keep the previous value.
    /// Pause policy.
    pub pause_policy: Option<PausePolicy>,
}

/// Immutable, revisioned task contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskContract {
    id: String,
    revision: u64,
    digest: String,
    request: String,
    context: Option<String>,
    constraints: Vec<TaskConstraint>,
    acceptance_criteria: Vec<AcceptanceCriterion>,
    pause_policy: PausePolicy,
}

impl TaskContract {
    /// Create a revision-1 contract from validated input.
    ///
    /// # Errors
    ///
    /// Returns a ContractError when any reference validation fails.
    pub fn create(
        input: CreateTaskContractInput,
    ) -> Result<TaskContract, ContractError> {
        validate_shape(
            input.id,
            input.request,
            input.context,
            input.constraints.unwrap_or_default(),
            input.acceptance_criteria,
            input.pause_policy.unwrap_or(PausePolicy::None),
            1,
        )
    }

    /// Produce the next immutable revision; omitted fields carry the
    /// previous revision's values over. The previous contract is never
    /// mutated.
    ///
    /// # Errors
    ///
    /// Returns RevisionIdMismatch when the id changes, or any validation
    /// error of the resulting shape.
    pub fn revise(
        &self,
        changes: ReviseTaskContractInput,
    ) -> Result<TaskContract, ContractError> {
        if changes.id != self.id {
            return Err(ContractError::RevisionIdMismatch);
        }
        let context = match changes.context {
            Some(ReviseContext::Set(value)) => Some(value),
            Some(ReviseContext::Keep) | None => self.context.clone(),
        };
        validate_shape(
            changes.id,
            changes.request.unwrap_or_else(|| self.request.clone()),
            context,
            changes.constraints.unwrap_or_else(|| self.constraints.clone()),
            changes
                .acceptance_criteria
                .unwrap_or_else(|| self.acceptance_criteria.clone()),
            changes.pause_policy.unwrap_or(self.pause_policy),
            self.revision + 1,
        )
    }

    /// Contract id (stable across revisions).
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Immutable revision identity; starts at 1 and only increases.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Exact content digest of this revision (revision excluded).
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// Normalized request text.
    pub fn request(&self) -> &str {
        &self.request
    }

    /// Normalized optional context.
    pub fn context(&self) -> Option<&str> {
        self.context.as_deref()
    }

    /// Constraints of this revision.
    pub fn constraints(&self) -> &[TaskConstraint] {
        &self.constraints
    }

    /// Acceptance criteria of this revision.
    pub fn acceptance_criteria(&self) -> &[AcceptanceCriterion] {
        &self.acceptance_criteria
    }

    /// Pause policy of this revision.
    pub fn pause_policy(&self) -> PausePolicy {
        self.pause_policy
    }

    /// Canonical content payload of this revision (revision excluded).
    /// Used by the differential observation and identity tests.
    pub fn content_payload(&self) -> CanonicalValue {
        content_payload(
            &self.id,
            &self.request,
            self.context.as_deref(),
            &self.constraints,
            &self.acceptance_criteria,
            self.pause_policy,
        )
    }
}

fn validate_entry_description(
    description: &str,
    empty: ContractError,
    too_large: ContractError,
) -> Result<String, ContractError> {
    let trimmed = description.trim();
    if trimmed.is_empty() {
        return Err(empty);
    }
    if trimmed.len() > limits::MAX_ENTRY_DESCRIPTION_BYTES {
        return Err(too_large);
    }
    Ok(trimmed.to_owned())
}

fn validate_shape(
    id: String,
    request: String,
    context: Option<String>,
    constraints: Vec<TaskConstraint>,
    acceptance_criteria: Vec<AcceptanceCriterion>,
    pause_policy: PausePolicy,
    revision: u64,
) -> Result<TaskContract, ContractError> {
    if !task_id_pattern_ok(&id) {
        return Err(ContractError::InvalidTaskContractId);
    }
    let request = request.trim();
    if request.is_empty() {
        return Err(ContractError::EmptyRequest);
    }
    if request.len() > limits::MAX_REQUEST_BYTES {
        return Err(ContractError::RequestTooLarge);
    }
    if revision < 1 {
        return Err(ContractError::InvalidRevision);
    }
    if acceptance_criteria.len() > limits::MAX_ACCEPTANCE_CRITERIA {
        return Err(ContractError::TooManyAcceptanceCriteria);
    }
    if constraints.len() > limits::MAX_CONSTRAINTS {
        return Err(ContractError::TooManyConstraints);
    }

    let mut criteria = Vec::with_capacity(acceptance_criteria.len());
    let mut criterion_ids = BTreeMap::new();
    for criterion in acceptance_criteria {
        if !entry_id_pattern_ok(&criterion.id) {
            return Err(ContractError::InvalidCriterionId);
        }
        if criterion_ids.insert(criterion.id.clone(), ()).is_some() {
            return Err(ContractError::DuplicateCriterionId);
        }
        let description = validate_entry_description(
            &criterion.description,
            ContractError::EmptyCriterionDescription,
            ContractError::CriterionDescriptionTooLarge,
        )?;
        criteria.push(AcceptanceCriterion {
            id: criterion.id,
            description,
            verification_kind: criterion.verification_kind,
        });
    }
    if criteria.is_empty() {
        return Err(ContractError::NoAcceptanceCriteria);
    }

    let mut validated_constraints = Vec::with_capacity(constraints.len());
    let mut constraint_ids = BTreeMap::new();
    for constraint in constraints {
        if !entry_id_pattern_ok(&constraint.id) {
            return Err(ContractError::InvalidConstraintId);
        }
        if constraint_ids.insert(constraint.id.clone(), ()).is_some() {
            return Err(ContractError::DuplicateConstraintId);
        }
        let description = validate_entry_description(
            &constraint.description,
            ContractError::EmptyConstraintDescription,
            ContractError::ConstraintDescriptionTooLarge,
        )?;
        validated_constraints.push(TaskConstraint {
            id: constraint.id,
            description,
            kind: constraint.kind,
        });
    }

    let context = match context {
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                if trimmed.len() > limits::MAX_CONTEXT_BYTES {
                    return Err(ContractError::ContextTooLarge);
                }
                Some(trimmed.to_owned())
            }
        }
        None => None,
    };

    let payload = content_payload(
        &id,
        request,
        context.as_deref(),
        &validated_constraints,
        &criteria,
        pause_policy,
    );
    let digest = artifact_digest_hex(
        "TaskContract",
        TASK_CONTRACT_IDENTITY_SCHEMA,
        &payload.to_canonical(),
    );

    Ok(TaskContract {
        id,
        revision,
        digest,
        request: request.to_owned(),
        context,
        constraints: validated_constraints,
        acceptance_criteria: criteria,
        pause_policy,
    })
}

fn content_payload(
    id: &str,
    request: &str,
    context: Option<&str>,
    constraints: &[TaskConstraint],
    acceptance_criteria: &[AcceptanceCriterion],
    pause_policy: PausePolicy,
) -> CanonicalValue {
    let mut object = BTreeMap::new();
    object.insert(
        "acceptanceCriteria".to_owned(),
        CanonicalValue::Array(
            acceptance_criteria
                .iter()
                .map(|criterion| {
                    let mut entry = BTreeMap::new();
                    entry.insert(
                        "description".to_owned(),
                        CanonicalValue::Str(criterion.description.clone()),
                    );
                    entry.insert(
                        "id".to_owned(),
                        CanonicalValue::Str(criterion.id.clone()),
                    );
                    entry.insert(
                        "verificationKind".to_owned(),
                        CanonicalValue::Str(
                            criterion.verification_kind.as_str().to_owned(),
                        ),
                    );
                    CanonicalValue::Object(entry)
                })
                .collect(),
        ),
    );
    object.insert(
        "constraints".to_owned(),
        CanonicalValue::Array(
            constraints
                .iter()
                .map(|constraint| {
                    let mut entry = BTreeMap::new();
                    entry.insert(
                        "description".to_owned(),
                        CanonicalValue::Str(constraint.description.clone()),
                    );
                    entry.insert(
                        "id".to_owned(),
                        CanonicalValue::Str(constraint.id.clone()),
                    );
                    entry.insert(
                        "kind".to_owned(),
                        CanonicalValue::Str(
                            constraint.kind.as_str().to_owned(),
                        ),
                    );
                    CanonicalValue::Object(entry)
                })
                .collect(),
        ),
    );
    if let Some(context) = context {
        object.insert(
            "context".to_owned(),
            CanonicalValue::Str(context.to_owned()),
        );
    }
    object.insert("id".to_owned(), CanonicalValue::Str(id.to_owned()));
    object.insert(
        "pausePolicy".to_owned(),
        CanonicalValue::Str(pause_policy.as_str().to_owned()),
    );
    object
        .insert("request".to_owned(), CanonicalValue::Str(request.to_owned()));
    CanonicalValue::Object(object)
}

/// Generic ad-hoc task contract for the CLI task surface: completion
/// requires host verification of the single explicit criterion.
pub fn create_adhoc_task_contract(
    id: &str,
    request: &str,
) -> Result<TaskContract, ContractError> {
    TaskContract::create(CreateTaskContractInput {
        id: id.to_owned(),
        request: request.to_owned(),
        context: None,
        constraints: Some(vec![TaskConstraint {
            id: "workspace-scope".to_owned(),
            description: "Work is contained within the workspace root."
                .to_owned(),
            kind: ConstraintKind::Scope,
        }]),
        acceptance_criteria: vec![AcceptanceCriterion {
            id: "host-verified".to_owned(),
            description:
                "The requested work is complete and verified by the host."
                    .to_owned(),
            verification_kind: VerificationKind::User,
        }],
        pause_policy: Some(PausePolicy::None),
    })
}
#[cfg(test)]
mod tests {
    use super::{
        AcceptanceCriterion, ConstraintKind, CreateTaskContractInput,
        PausePolicy, TaskConstraint, TaskContract, VerificationKind,
        create_adhoc_task_contract, limits,
    };
    use crate::task::contract::ContractError;

    fn criterion(id: &str, kind: VerificationKind) -> AcceptanceCriterion {
        AcceptanceCriterion {
            id: id.to_owned(),
            description: format!("criterion {id}"),
            verification_kind: kind,
        }
    }

    fn base_input(id: &str) -> CreateTaskContractInput {
        CreateTaskContractInput {
            id: id.to_owned(),
            request: format!("Request for {id}"),
            context: None,
            constraints: None,
            acceptance_criteria: vec![criterion(
                "a",
                VerificationKind::Deterministic,
            )],
            pause_policy: None,
        }
    }

    #[test]
    fn creates_a_revision_1_contract_with_explicit_criteria() {
        let contract = TaskContract::create(CreateTaskContractInput {
            id: "task-1".to_owned(),
            request: "Add a health component".to_owned(),
            context: None,
            constraints: Some(vec![TaskConstraint {
                id: "scope".to_owned(),
                description: "Workspace only.".to_owned(),
                kind: ConstraintKind::Scope,
            }]),
            acceptance_criteria: vec![
                criterion("parses", VerificationKind::Deterministic),
                criterion("review", VerificationKind::Review),
            ],
            pause_policy: Some(PausePolicy::OnApproval),
        })
        .expect("valid contract");
        assert_eq!(contract.revision(), 1);
        assert_eq!(contract.request(), "Add a health component");
        assert_eq!(contract.acceptance_criteria().len(), 2);
        assert_eq!(contract.pause_policy(), PausePolicy::OnApproval);
        assert_eq!(contract.digest().len(), 64);
    }

    #[test]
    fn rejects_empty_requests_empty_criteria_and_duplicate_criterion_ids() {
        let empty = base_input("t");
        assert_eq!(
            TaskContract::create(CreateTaskContractInput {
                request: "   ".to_owned(),
                ..empty
            })
            .expect_err("empty request"),
            ContractError::EmptyRequest
        );
        let duplicate = base_input("t");
        assert_eq!(
            TaskContract::create(CreateTaskContractInput {
                acceptance_criteria: vec![
                    criterion("a", VerificationKind::Deterministic),
                    criterion("a", VerificationKind::Review),
                ],
                ..duplicate
            })
            .expect_err("duplicate criterion"),
            ContractError::DuplicateCriterionId
        );
        let none = base_input("t");
        assert_eq!(
            TaskContract::create(CreateTaskContractInput {
                acceptance_criteria: vec![],
                ..none
            })
            .expect_err("no criteria"),
            ContractError::NoAcceptanceCriteria
        );
    }

    #[test]
    fn revisions_are_immutable_and_omit_fields_carry_over() {
        let original =
            TaskContract::create(base_input("task-1")).expect("valid");
        let revision = original
            .revise(super::ReviseTaskContractInput {
                id: "task-1".to_owned(),
                request: Some("Changed request".to_owned()),
                context: None,
                constraints: None,
                acceptance_criteria: None,
                pause_policy: None,
            })
            .expect("valid revision");
        assert_eq!(revision.revision(), 2);
        assert_eq!(revision.request(), "Changed request");
        assert_eq!(original.revision(), 1);
        assert_eq!(original.request(), "Request for task-1");
        assert_eq!(
            revision.acceptance_criteria(),
            original.acceptance_criteria()
        );
        assert_ne!(revision.digest(), original.digest());
    }

    #[test]
    fn revision_and_digest_remain_distinct_identities() {
        let contract =
            TaskContract::create(base_input("task-id")).expect("valid");
        // A no-op revision keeps the same material content and therefore
        // the same digest while the revision number increases.
        let noop = contract
            .revise(super::ReviseTaskContractInput {
                id: "task-id".to_owned(),
                request: None,
                context: None,
                constraints: None,
                acceptance_criteria: None,
                pause_policy: None,
            })
            .expect("valid no-op revision");
        assert_eq!(noop.revision(), 2);
        assert_eq!(noop.digest(), contract.digest());
    }

    #[test]
    fn rejects_an_id_change_across_revisions() {
        let contract =
            TaskContract::create(base_input("task-stable")).expect("valid");
        assert_eq!(
            contract
                .revise(super::ReviseTaskContractInput {
                    id: "task-different".to_owned(),
                    request: None,
                    context: None,
                    constraints: None,
                    acceptance_criteria: None,
                    pause_policy: None,
                })
                .expect_err("id change"),
            ContractError::RevisionIdMismatch
        );
    }

    #[test]
    fn rejects_invalid_ids_and_oversized_fields() {
        let invalid_id = base_input("task with spaces");
        assert_eq!(
            TaskContract::create(invalid_id).expect_err("invalid id"),
            ContractError::InvalidTaskContractId
        );
        let oversized = base_input("task-large-request");
        assert_eq!(
            TaskContract::create(CreateTaskContractInput {
                request: "界".repeat(limits::MAX_REQUEST_BYTES),
                ..oversized
            })
            .expect_err("oversized request"),
            ContractError::RequestTooLarge
        );
        // Byte-boundary precision: max bytes pass, max+1 fails.
        let at_limit = base_input("task-limit");
        let ok = TaskContract::create(CreateTaskContractInput {
            request: "x".repeat(limits::MAX_REQUEST_BYTES),
            ..at_limit
        });
        assert!(ok.is_ok(), "exactly the limit must pass");
        let over = base_input("task-limit");
        assert_eq!(
            TaskContract::create(CreateTaskContractInput {
                request: "x".repeat(limits::MAX_REQUEST_BYTES + 1),
                ..over
            })
            .expect_err("over the limit"),
            ContractError::RequestTooLarge
        );
    }

    #[test]
    fn trims_and_normalizes_inputs() {
        let contract = TaskContract::create(CreateTaskContractInput {
            id: "task-trim".to_owned(),
            request: "  Request with padding  ".to_owned(),
            context: Some("   ".to_owned()),
            constraints: None,
            acceptance_criteria: vec![AcceptanceCriterion {
                id: "a".to_owned(),
                description: "  padded description  ".to_owned(),
                verification_kind: VerificationKind::Deterministic,
            }],
            pause_policy: None,
        })
        .expect("valid");
        assert_eq!(contract.request(), "Request with padding");
        assert_eq!(contract.context(), None, "blank context normalizes away");
        assert_eq!(
            contract.acceptance_criteria()[0].description(),
            "padded description"
        );
    }

    #[test]
    fn digest_matches_the_typescript_reference_for_the_same_contract() {
        // Reference digest computed by the TypeScript oracle for this
        // exact contract (packages/core/src/tasks/task-contract.ts).
        let contract = TaskContract::create(CreateTaskContractInput {
            id: "task-1".to_owned(),
            request: "Add a health component".to_owned(),
            context: None,
            constraints: Some(vec![TaskConstraint {
                id: "scope".to_owned(),
                description: "Workspace only.".to_owned(),
                kind: ConstraintKind::Scope,
            }]),
            acceptance_criteria: vec![AcceptanceCriterion {
                id: "parses".to_owned(),
                description: "Parses cleanly.".to_owned(),
                verification_kind: VerificationKind::Deterministic,
            }],
            pause_policy: Some(PausePolicy::OnApproval),
        })
        .expect("valid contract");
        assert_eq!(
            contract.digest(),
            "cfc155baae2d2a56c1675518185cc4ffab948d909f60ca34f431553c6f3f0a8a"
        );
    }

    #[test]
    fn adhoc_contract_requires_host_verification() {
        let contract = create_adhoc_task_contract("task-adhoc", "Do the work")
            .expect("valid adhoc contract");
        assert_eq!(contract.revision(), 1);
        assert_eq!(contract.acceptance_criteria().len(), 1);
        assert_eq!(
            contract.acceptance_criteria()[0].verification_kind(),
            VerificationKind::User
        );
    }
}
