//! Formal context classes and PhaseContract (Stage 3 — Interpretable
//! Context Architecture, ADR 0030; R10b ICM).
//!
//! Mirrors `packages/core/src/context/phase-contract.ts`. Context
//! classes formalize the categories Siralos already approximates; no
//! phase defaults to repository-wide context.
//!
//! A PhaseContract DECLARES one bounded phase's inputs, authority
//! ceiling, operations, outputs, and verification. It is not a state
//! machine — TaskState remains authoritative for workflow progress — and
//! it can only NARROW authority: the authority profile is a fixed
//! vocabulary, so a malformed contract is rejected structurally before
//! it can influence anything. ToolProjector and security enforcement
//! remain authoritative.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use super::{ContextError, context_error};
use crate::identity::{
    ArtifactDigest, CanonicalValue, compute_artifact_digest,
};

fn object(entries: Vec<(&str, CanonicalValue)>) -> CanonicalValue {
    CanonicalValue::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn string_value(value: &str) -> CanonicalValue {
    CanonicalValue::Str(value.to_owned())
}

// ---------------------------------------------------------------------------
// Context classes
// ---------------------------------------------------------------------------

/// The five context classes (`ContextClass` in the oracle).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContextClass {
    /// Execution identity/rules.
    Global,
    /// Planning/scope/routing.
    Routing,
    /// Current operation requirements.
    PhaseContract,
    /// Guidance/architecture/instructions.
    StableReference,
    /// Plan/source/evidence.
    Working,
}

impl ContextClass {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Routing => "routing",
            Self::PhaseContract => "phase_contract",
            Self::StableReference => "stable_reference",
            Self::Working => "working",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "global" => Some(Self::Global),
            "routing" => Some(Self::Routing),
            "phase_contract" => Some(Self::PhaseContract),
            "stable_reference" => Some(Self::StableReference),
            "working" => Some(Self::Working),
            _ => None,
        }
    }
}

/// The bounded context-class vocabulary, in oracle declaration order.
pub const CONTEXT_CLASSES: [&str; 5] =
    ["global", "routing", "phase_contract", "stable_reference", "working"];

/// Bounded artifact-kind vocabulary per context class (by reference).
pub const CONTEXT_CLASS_ARTIFACT_KINDS: [&[&str]; 5] = [
    &["ExecutionContract", "RuntimeIdentity"],
    &[
        "PlanningPolicy",
        "WorkspaceScope",
        "ActiveWorkingSet",
        "DocumentationSelection",
        "ToolProjection",
    ],
    &["PhaseContract"],
    &[
        "ScopedAgents",
        "ArchitectureDocs",
        "ApplicableAdrs",
        "ProjectInstructions",
        "ProjectKnowledge",
        "References",
    ],
    &[
        "TaskPlan",
        "ActiveWorkingSetFiles",
        "CurrentSourceRevisions",
        "PreparedMutations",
        "ValidationEvidence",
        "ReviewFindings",
    ],
];

/// Artifact kinds for one context class, in declaration order.
///
/// Order mirrors the `CONTEXT_CLASS_ARTIFACT_KINDS` table above:
/// global, routing, phase_contract, stable_reference, working.
#[must_use]
pub fn class_artifact_kinds(
    context_class: ContextClass,
) -> &'static [&'static str] {
    match context_class {
        ContextClass::Global => CONTEXT_CLASS_ARTIFACT_KINDS[0],
        ContextClass::Routing => CONTEXT_CLASS_ARTIFACT_KINDS[1],
        ContextClass::PhaseContract => CONTEXT_CLASS_ARTIFACT_KINDS[2],
        ContextClass::StableReference => CONTEXT_CLASS_ARTIFACT_KINDS[3],
        ContextClass::Working => CONTEXT_CLASS_ARTIFACT_KINDS[4],
    }
}

// ---------------------------------------------------------------------------
// PhaseContract input shapes (raw, pre-validation)
// ---------------------------------------------------------------------------

/// One declared phase input requirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseInputRequirement {
    /// Artifact type consumed by the phase.
    pub artifact_type: String,
    /// Whether the phase may run without it.
    pub optional: bool,
    /// Why the phase needs this input.
    pub reason: String,
}

/// Raw declared authority profile (fixed vocabulary, validated by
/// [`validate_authority_profile`]). `mutation` stays a raw protocol
/// string so vocabulary violations are rejected with the oracle's
/// message instead of failing at deserialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseAuthorityProfileInput {
    /// Whether the contract declares read-only scope.
    pub read_only: bool,
    /// `"none"` or `"prepared_only"` — no unrestricted form exists.
    pub mutation: String,
    /// Whether the contract may request approval binding.
    pub approval_grant: bool,
    /// Whether the contract may evaluate acceptance.
    pub acceptance_authority: bool,
    /// Declared capability narrowings (never broadenings).
    pub capability_narrowing: Vec<String>,
}

/// One declared phase operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseOperation {
    /// Stable operation id.
    pub id: String,
    /// What the operation does.
    pub description: String,
}

/// One declared phase output requirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseOutputRequirement {
    /// Artifact type produced by the phase.
    pub artifact_type: String,
    /// Verification kind: deterministic, host_verified, or review.
    pub verification_kind: String,
}

/// One declared phase verification requirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseVerificationRequirement {
    /// Stable requirement id.
    pub id: String,
    /// What must be verified.
    pub description: String,
    /// Evidence class the verification produces.
    pub evidence_class: String,
}

/// Input for [`create_phase_contract`] (raw, pre-validation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatePhaseContractInput {
    /// Contract id (registry ids are typed at [`PhaseContractId`]).
    pub id: String,
    /// Positive schema version of the contract declaration.
    pub version: u64,
    /// Free-form phase label recorded in the digest payload.
    pub phase: String,
    /// Required and optional input requirements (non-empty).
    pub inputs: Vec<PhaseInputRequirement>,
    /// Authority ceiling (narrowing-only).
    pub authority: PhaseAuthorityProfileInput,
    /// Declared process operations (may be empty).
    pub process: Vec<PhaseOperation>,
    /// Output requirements (non-empty).
    pub outputs: Vec<PhaseOutputRequirement>,
    /// Verification requirements (non-empty).
    pub verification: Vec<PhaseVerificationRequirement>,
    /// Context classes this contract draws on (raw strings, validated).
    pub context_classes: Vec<String>,
}

// ---------------------------------------------------------------------------
// Validated PhaseContract
// ---------------------------------------------------------------------------

/// Mutation authority vocabulary. A contract can prepare exact changes
/// under approval, or nothing at all; unrestricted mutation does not
/// exist in the vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseMutation {
    /// No mutation authority.
    None,
    /// Prepared-only mutation under the approval workflow.
    PreparedOnly,
}

impl PhaseMutation {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::PreparedOnly => "prepared_only",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "prepared_only" => Some(Self::PreparedOnly),
            _ => None,
        }
    }
}

/// Validated authority profile (narrowing-only by construction).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseAuthorityProfile {
    /// Whether the contract declares read-only scope.
    pub read_only: bool,
    /// Mutation ceiling.
    pub mutation: PhaseMutation,
    /// Whether the contract may request approval binding.
    pub approval_grant: bool,
    /// Whether the contract may evaluate acceptance.
    pub acceptance_authority: bool,
    /// Declared capability narrowings (never broadenings).
    pub capability_narrowing: Vec<String>,
}

/// Contract id vocabulary realized by the pre-built registry.
pub type PhaseContractId = &'static str;

/// All eleven registry contract ids, in oracle declaration order.
pub const PHASE_CONTRACT_IDS: [PhaseContractId; 11] = [
    "planning",
    "inspection",
    "preparation",
    "approval",
    "mutation",
    "verification",
    "validation",
    "impact",
    "review",
    "repair",
    "acceptance",
];

/// A validated, digest-bound phase contract declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseContract {
    /// Contract id.
    pub id: String,
    /// Declaration version (positive).
    pub version: u64,
    /// Phase label.
    pub phase: String,
    /// Input requirements.
    pub inputs: Vec<PhaseInputRequirement>,
    /// Validated authority profile.
    pub authority: PhaseAuthorityProfile,
    /// Process operations.
    pub process: Vec<PhaseOperation>,
    /// Output requirements.
    pub outputs: Vec<PhaseOutputRequirement>,
    /// Verification requirements.
    pub verification: Vec<PhaseVerificationRequirement>,
    /// Parsed context classes.
    pub context_classes: Vec<ContextClass>,
    /// Domain-separated digest over the canonical declaration payload
    /// (`siralos:PhaseContract:v1\0…`).
    pub digest: ArtifactDigest,
}

/// Authority validation: the fixed vocabulary means a malformed contract
/// (e.g. a review contract demanding unrestricted mutation) is rejected
/// structurally before it can influence anything.
///
/// Boolean-typed fields cannot fail type checks in Rust, so only the two
/// reachable rejections remain: the mutation vocabulary and the
/// read-only narrowing invariant.
pub fn validate_authority_profile(
    contract_id: &str,
    authority: &PhaseAuthorityProfileInput,
) -> Result<PhaseAuthorityProfile, ContextError> {
    let mutation = PhaseMutation::parse(&authority.mutation).ok_or_else(|| {
        context_error(format!(
            "PhaseContract {contract_id}: mutation must be none or prepared_only."
        ))
    })?;
    if authority.read_only && mutation != PhaseMutation::None {
        return Err(context_error(format!(
            "PhaseContract {contract_id}: a read-only contract cannot declare mutation authority."
        )));
    }
    Ok(PhaseAuthorityProfile {
        read_only: authority.read_only,
        mutation,
        approval_grant: authority.approval_grant,
        acceptance_authority: authority.acceptance_authority,
        capability_narrowing: authority.capability_narrowing.clone(),
    })
}

/// Canonical digest payload over a raw declaration (all fields except
/// the digest itself), keyed exactly like the oracle payload.
fn contract_payload(input: &CreatePhaseContractInput) -> CanonicalValue {
    let inputs = CanonicalValue::Array(
        input
            .inputs
            .iter()
            .map(|entry| {
                object(vec![
                    ("artifactType", string_value(&entry.artifact_type)),
                    ("optional", CanonicalValue::Bool(entry.optional)),
                    ("reason", string_value(&entry.reason)),
                ])
            })
            .collect(),
    );
    let authority = object(vec![
        ("readOnly", CanonicalValue::Bool(input.authority.read_only)),
        ("mutation", string_value(&input.authority.mutation)),
        (
            "approvalGrant",
            CanonicalValue::Bool(input.authority.approval_grant),
        ),
        (
            "acceptanceAuthority",
            CanonicalValue::Bool(input.authority.acceptance_authority),
        ),
        (
            "capabilityNarrowing",
            CanonicalValue::Array(
                input
                    .authority
                    .capability_narrowing
                    .iter()
                    .map(|entry| string_value(entry))
                    .collect(),
            ),
        ),
    ]);
    let process = CanonicalValue::Array(
        input
            .process
            .iter()
            .map(|entry| {
                object(vec![
                    ("id", string_value(&entry.id)),
                    ("description", string_value(&entry.description)),
                ])
            })
            .collect(),
    );
    let outputs = CanonicalValue::Array(
        input
            .outputs
            .iter()
            .map(|entry| {
                object(vec![
                    ("artifactType", string_value(&entry.artifact_type)),
                    (
                        "verificationKind",
                        string_value(&entry.verification_kind),
                    ),
                ])
            })
            .collect(),
    );
    let verification = CanonicalValue::Array(
        input
            .verification
            .iter()
            .map(|entry| {
                object(vec![
                    ("id", string_value(&entry.id)),
                    ("description", string_value(&entry.description)),
                    ("evidenceClass", string_value(&entry.evidence_class)),
                ])
            })
            .collect(),
    );
    let context_classes = CanonicalValue::Array(
        input
            .context_classes
            .iter()
            .map(|entry| string_value(entry))
            .collect(),
    );
    object(vec![
        ("id", string_value(&input.id)),
        ("version", CanonicalValue::U64(input.version)),
        ("phase", string_value(&input.phase)),
        ("inputs", inputs),
        ("authority", authority),
        ("process", process),
        ("outputs", outputs),
        ("verification", verification),
        ("contextClasses", context_classes),
    ])
}

/// Create a validated, digest-bound PhaseContract from a raw
/// declaration. Validation order mirrors the oracle exactly so both
/// implementations reject malformed declarations with identical
/// messages.
pub fn create_phase_contract(
    input: &CreatePhaseContractInput,
) -> Result<PhaseContract, ContextError> {
    if input.id.is_empty() {
        return Err(context_error("A PhaseContract requires an id."));
    }
    if input.version < 1 {
        return Err(context_error(
            "A PhaseContract version must be a positive safe integer.",
        ));
    }
    if input.inputs.is_empty() {
        return Err(context_error(format!(
            "PhaseContract {} requires at least one input.",
            input.id
        )));
    }
    if input.outputs.is_empty() {
        return Err(context_error(format!(
            "PhaseContract {} requires at least one output.",
            input.id
        )));
    }
    if input.verification.is_empty() {
        return Err(context_error(format!(
            "PhaseContract {} requires at least one verification requirement.",
            input.id
        )));
    }
    if input.context_classes.is_empty() {
        return Err(context_error(format!(
            "PhaseContract {} requires at least one context class.",
            input.id
        )));
    }
    let mut parsed_classes = Vec::with_capacity(input.context_classes.len());
    for class in &input.context_classes {
        let parsed = ContextClass::parse(class).ok_or_else(|| {
            context_error(format!(
                "PhaseContract {} declares unknown context class {class}.",
                input.id
            ))
        })?;
        parsed_classes.push(parsed);
    }
    let authority = validate_authority_profile(&input.id, &input.authority)?;
    let digest =
        compute_artifact_digest("PhaseContract", 1, &contract_payload(input))
            .map_err(|error| ContextError { message: error.message })?;
    Ok(PhaseContract {
        id: input.id.clone(),
        version: input.version,
        phase: input.phase.clone(),
        inputs: input.inputs.clone(),
        authority,
        process: input.process.clone(),
        outputs: input.outputs.clone(),
        verification: input.verification.clone(),
        context_classes: parsed_classes,
        digest,
    })
}

/// Deterministic phase → context-class mapping (host-owned table).
/// Unknown phases map to an empty list.
#[must_use]
pub fn context_classes_for_phase(phase_id: &str) -> Vec<ContextClass> {
    phase_contracts()
        .get(phase_id)
        .map(|contract| contract.context_classes.clone())
        .unwrap_or_default()
}

fn read_only_authority() -> PhaseAuthorityProfileInput {
    PhaseAuthorityProfileInput {
        read_only: true,
        mutation: "none".to_owned(),
        approval_grant: false,
        acceptance_authority: false,
        capability_narrowing: Vec::new(),
    }
}

fn input(
    artifact_type: &str,
    optional: bool,
    reason: &str,
) -> PhaseInputRequirement {
    PhaseInputRequirement {
        artifact_type: artifact_type.to_owned(),
        optional,
        reason: reason.to_owned(),
    }
}

fn operation(id: &str, description: &str) -> PhaseOperation {
    PhaseOperation { id: id.to_owned(), description: description.to_owned() }
}

fn output(
    artifact_type: &str,
    verification_kind: &str,
) -> PhaseOutputRequirement {
    PhaseOutputRequirement {
        artifact_type: artifact_type.to_owned(),
        verification_kind: verification_kind.to_owned(),
    }
}

fn verification(
    id: &str,
    description: &str,
    evidence_class: &str,
) -> PhaseVerificationRequirement {
    PhaseVerificationRequirement {
        id: id.to_owned(),
        description: description.to_owned(),
        evidence_class: evidence_class.to_owned(),
    }
}

fn classes(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn authority(
    read_only: bool,
    mutation: &str,
    approval_grant: bool,
    acceptance_authority: bool,
) -> PhaseAuthorityProfileInput {
    PhaseAuthorityProfileInput {
        read_only,
        mutation: mutation.to_owned(),
        approval_grant,
        acceptance_authority,
        capability_narrowing: Vec::new(),
    }
}

/// The pre-built host-owned registry of eleven phase contracts. Every
/// entry is validated at first access and carries its stable digest;
/// iteration order is id-sorted.
pub fn phase_contracts() -> &'static BTreeMap<PhaseContractId, PhaseContract> {
    static REGISTRY: OnceLock<BTreeMap<PhaseContractId, PhaseContract>> =
        OnceLock::new();
    REGISTRY.get_or_init(|| {
        let declarations = [
            planning_declaration(),
            inspection_declaration(),
            preparation_declaration(),
            approval_declaration(),
            mutation_declaration(),
            verification_declaration(),
            validation_declaration(),
            impact_declaration(),
            review_declaration(),
            repair_declaration(),
            acceptance_declaration(),
        ];
        let mut registry = BTreeMap::new();
        for (declaration, id) in
            declarations.into_iter().zip(PHASE_CONTRACT_IDS)
        {
            let contract = create_phase_contract(&declaration)
                .expect("registry phase contracts are valid");
            assert_eq!(
                contract.id, id,
                "registry declaration id is canonical"
            );
            registry.insert(id, contract);
        }
        registry
    })
}

#[allow(clippy::too_many_arguments)]
fn declaration(
    id: &str,
    phase: &str,
    inputs: Vec<PhaseInputRequirement>,
    authority_input: PhaseAuthorityProfileInput,
    process: Vec<PhaseOperation>,
    outputs: Vec<PhaseOutputRequirement>,
    verification: Vec<PhaseVerificationRequirement>,
    context_classes: Vec<String>,
) -> CreatePhaseContractInput {
    CreatePhaseContractInput {
        id: id.to_owned(),
        version: 1,
        phase: phase.to_owned(),
        inputs,
        authority: authority_input,
        process,
        outputs,
        verification,
        context_classes,
    }
}

fn planning_declaration() -> CreatePhaseContractInput {
    declaration(
        "planning",
        "working",
        vec![
            input("TaskContract", false, "the plan binds the exact contract"),
            input("WorkspaceScope", true, "structural source evidence"),
            input(
                "DocumentationSelection",
                true,
                "applicable architecture/ADRs",
            ),
            input("References", true, "external reference material"),
        ],
        read_only_authority(),
        vec![operation("route", "deterministic planning-depth routing")],
        vec![output("TaskPlan", "deterministic")],
        vec![verification(
            "plan-validated",
            "plan candidate validated against contract and depth",
            "plan_validation",
        )],
        classes(&["global", "routing", "stable_reference", "phase_contract"]),
    )
}

fn inspection_declaration() -> CreatePhaseContractInput {
    declaration(
        "inspection",
        "working",
        vec![
            input("WorkspaceScope", false, "verified/candidate files"),
            input(
                "CurrentSourceRevisions",
                false,
                "exact revisions of inspected files",
            ),
        ],
        read_only_authority(),
        vec![operation("inspect", "bounded structural/semantic inspection")],
        vec![output("InspectionEvidence", "host_verified")],
        vec![verification(
            "evidence-attached",
            "observations attached as typed evidence",
            "workspace_read",
        )],
        classes(&["routing", "working"]),
    )
}

fn preparation_declaration() -> CreatePhaseContractInput {
    declaration(
        "preparation",
        "working",
        vec![
            input("TaskPlan", false, "prepared changes follow the plan"),
            input("CurrentSourceRevisions", false, "exact pre-state"),
        ],
        read_only_authority(),
        vec![operation("prepare", "read-only preparation of exact changes")],
        vec![output("PreparedChangeset", "deterministic")],
        vec![verification(
            "prepared-fingerprint",
            "prepared identity digest bound",
            "change_preview",
        )],
        classes(&["working", "phase_contract"]),
    )
}

fn approval_declaration() -> CreatePhaseContractInput {
    declaration(
        "approval",
        "working",
        vec![
            input("PreparedChangeset", false, "exact content being approved"),
            input("TaskContract", false, "criteria and constraints"),
        ],
        authority(true, "none", true, false),
        vec![operation(
            "request-approval",
            "host approval request binding exact digest",
        )],
        vec![output("ApprovalRecord", "host_verified")],
        vec![verification(
            "digest-bound",
            "approval binds the exact prepared digest",
            "change_preview",
        )],
        classes(&["phase_contract", "working"]),
    )
}

fn mutation_declaration() -> CreatePhaseContractInput {
    declaration(
        "mutation",
        "working",
        vec![
            input(
                "PreparedChangeset",
                false,
                "only prepared operations apply",
            ),
            input("ApprovalRecord", false, "exact approval required"),
        ],
        authority(false, "prepared_only", false, false),
        vec![
            operation("checkpoint", "checkpoint before mutation"),
            operation("apply", "hash-verified exact application"),
        ],
        vec![output("MutationResult", "deterministic")],
        vec![verification(
            "applied-verified",
            "per-surface verification after apply",
            "mutation_receipt",
        )],
        classes(&["working"]),
    )
}

fn verification_declaration() -> CreatePhaseContractInput {
    declaration(
        "verification",
        "validating",
        vec![
            input("MutationResult", false, "what was applied"),
            input("CurrentSourceRevisions", false, "post-apply revisions"),
        ],
        read_only_authority(),
        vec![operation(
            "verify",
            "per-surface verification (parser/LSP/semantic)",
        )],
        vec![output("VerificationEvidence", "deterministic")],
        vec![verification(
            "verified",
            "required verification passed",
            "parser_result",
        )],
        classes(&["working"]),
    )
}

fn validation_declaration() -> CreatePhaseContractInput {
    declaration(
        "validation",
        "validating",
        vec![
            input("VerificationEvidence", false, "changed surfaces"),
            input("ImpactRelationships", true, "verified impact"),
            input("AcceptanceCriteria", false, "host-required minimum"),
        ],
        read_only_authority(),
        vec![operation(
            "derive-plan",
            "deterministic validation plan derivation",
        )],
        vec![output("ValidationPlan", "deterministic")],
        vec![verification(
            "required-completed",
            "required validation completed or honestly unavailable",
            "validation_result",
        )],
        classes(&["working", "phase_contract"]),
    )
}

fn impact_declaration() -> CreatePhaseContractInput {
    declaration(
        "impact",
        "working",
        vec![
            input("ChangedSurfaces", false, "what changed"),
            input("RelationshipIndex", true, "verified relationships"),
        ],
        read_only_authority(),
        vec![operation("derive-impact", "bounded impact analysis")],
        vec![output("ReviewContextManifest", "host_verified")],
        vec![verification(
            "impact-derived",
            "impact manifest derived from evidence",
            "validation_result",
        )],
        classes(&["working"]),
    )
}

fn review_declaration() -> CreatePhaseContractInput {
    declaration(
        "review",
        "reviewing",
        vec![
            input("TaskContract", false, "criteria"),
            input("Changeset", false, "exact change under review"),
            input("ReviewContextManifest", true, "impact context"),
            input("ValidationEvidence", false, "relevant evidence"),
            input("CurrentSourceRevisions", false, "relevant source"),
        ],
        read_only_authority(),
        vec![operation("review", "fresh read-only review")],
        vec![output("ReviewVerdict", "review")],
        vec![verification(
            "verdict-bound",
            "verdict bound to review input digest",
            "review_result",
        )],
        classes(&["working", "stable_reference", "phase_contract"]),
    )
}

fn repair_declaration() -> CreatePhaseContractInput {
    declaration(
        "repair",
        "working",
        vec![
            input("ReviewFindings", false, "blocking findings"),
            input("CurrentSourceRevisions", false, "current revisions"),
            input("AcceptanceCriteria", false, "affected criteria"),
        ],
        authority(false, "prepared_only", false, false),
        vec![operation(
            "re-prepare",
            "fresh preparation from current revisions",
        )],
        vec![output("PreparedChangeset", "deterministic")],
        vec![verification(
            "fresh-artifacts",
            "repair uses fresh revisions/approvals only",
            "change_preview",
        )],
        classes(&["working"]),
    )
}

fn acceptance_declaration() -> CreatePhaseContractInput {
    declaration(
        "acceptance",
        "reviewing",
        vec![
            input("AcceptanceCriteria", false, "requirements"),
            input("ValidationEvidence", false, "current evidence identities"),
            input("ReviewVerdict", false, "required review verdict"),
            input(
                "MutationVerificationEvidence",
                false,
                "mutation verification",
            ),
        ],
        authority(true, "none", false, true),
        vec![operation("evaluate", "deterministic acceptance evaluation")],
        vec![output("AcceptanceResult", "deterministic")],
        vec![verification(
            "evidence-bound",
            "acceptance bound to exact evidence set",
            "validation_result",
        )],
        classes(&["working", "phase_contract"]),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ContextClass, CreatePhaseContractInput, PHASE_CONTRACT_IDS,
        PhaseAuthorityProfileInput, PhaseMutation, class_artifact_kinds,
        context_classes_for_phase, create_phase_contract, phase_contracts,
        validate_authority_profile,
    };
    use crate::context::{ContextError, context_error};

    fn read_only() -> PhaseAuthorityProfileInput {
        PhaseAuthorityProfileInput {
            read_only: true,
            mutation: "none".to_owned(),
            approval_grant: false,
            acceptance_authority: false,
            capability_narrowing: Vec::new(),
        }
    }

    fn valid_input() -> CreatePhaseContractInput {
        CreatePhaseContractInput {
            id: "inspection".to_owned(),
            version: 1,
            phase: "working".to_owned(),
            inputs: vec![super::input("WorkspaceScope", false, "files")],
            authority: read_only(),
            process: vec![super::operation("inspect", "bounded")],
            outputs: vec![super::output(
                "InspectionEvidence",
                "host_verified",
            )],
            verification: vec![super::verification(
                "evidence-attached",
                "typed evidence",
                "workspace_read",
            )],
            context_classes: vec!["routing".to_owned(), "working".to_owned()],
        }
    }

    #[test]
    fn creates_a_digest_bound_contract() {
        let contract =
            create_phase_contract(&valid_input()).expect("valid declaration");
        assert_eq!(contract.id, "inspection");
        assert_eq!(contract.version, 1);
        assert_eq!(
            contract.context_classes,
            vec![ContextClass::Routing, ContextClass::Working]
        );
        assert_eq!(contract.digest.value.len(), 64);
        // Identical declarations produce identical digests; any field
        // change moves the digest.
        let again = create_phase_contract(&valid_input()).expect("valid");
        assert_eq!(again.digest, contract.digest);
        let mut changed = valid_input();
        changed.version = 2;
        let bumped = create_phase_contract(&changed).expect("valid");
        assert_ne!(bumped.digest, contract.digest);
        assert_eq!(contract.digest.artifact_type, "PhaseContract");
        assert_eq!(contract.digest.schema_version, 1);
    }

    #[test]
    fn rejects_malformed_declarations_with_oracle_messages() {
        let mut bad = valid_input();
        bad.id = String::new();
        assert_eq!(
            create_phase_contract(&bad),
            Err(context_error("A PhaseContract requires an id."))
        );
        let mut bad = valid_input();
        bad.version = 0;
        assert_eq!(
            create_phase_contract(&bad),
            Err(context_error(
                "A PhaseContract version must be a positive safe integer."
            ))
        );
        let mut bad = valid_input();
        bad.inputs.clear();
        assert_eq!(
            create_phase_contract(&bad),
            Err(context_error(
                "PhaseContract inspection requires at least one input."
            ))
        );
        let mut bad = valid_input();
        bad.outputs.clear();
        assert_eq!(
            create_phase_contract(&bad),
            Err(context_error(
                "PhaseContract inspection requires at least one output."
            ))
        );
        let mut bad = valid_input();
        bad.verification.clear();
        assert_eq!(
            create_phase_contract(&bad),
            Err(context_error(
                "PhaseContract inspection requires at least one verification requirement."
            ))
        );
        let mut bad = valid_input();
        bad.context_classes.clear();
        assert_eq!(
            create_phase_contract(&bad),
            Err(context_error(
                "PhaseContract inspection requires at least one context class."
            ))
        );
    }

    #[test]
    fn unknown_context_classes_are_rejected_in_declaration_order() {
        let mut bad = valid_input();
        bad.id = "impact".to_owned();
        bad.context_classes = vec!["working".to_owned(), "bogus".to_owned()];
        assert_eq!(
            create_phase_contract(&bad),
            Err(ContextError {
                message:
                    "PhaseContract impact declares unknown context class bogus."
                        .to_owned()
            })
        );
    }

    #[test]
    fn authority_vocabulary_and_narrowing_are_enforced() {
        assert_eq!(
            validate_authority_profile(
                "review",
                &PhaseAuthorityProfileInput {
                    read_only: false,
                    mutation: "unrestricted".to_owned(),
                    approval_grant: false,
                    acceptance_authority: false,
                    capability_narrowing: Vec::new(),
                }
            ),
            Err(ContextError {
                message:
                    "PhaseContract review: mutation must be none or prepared_only."
                        .to_owned()
            })
        );
        assert_eq!(
            validate_authority_profile(
                "review",
                &PhaseAuthorityProfileInput {
                    read_only: true,
                    mutation: "prepared_only".to_owned(),
                    approval_grant: false,
                    acceptance_authority: false,
                    capability_narrowing: Vec::new(),
                }
            ),
            Err(ContextError {
                message:
                    "PhaseContract review: a read-only contract cannot declare mutation authority."
                        .to_owned()
            })
        );
        let prepared_only = validate_authority_profile(
            "repair",
            &PhaseAuthorityProfileInput {
                read_only: false,
                mutation: "prepared_only".to_owned(),
                approval_grant: false,
                acceptance_authority: false,
                capability_narrowing: Vec::new(),
            },
        )
        .expect("prepared-only is allowed for non-read-only");
        assert_eq!(prepared_only.mutation, PhaseMutation::PreparedOnly);
    }

    #[test]
    fn registry_holds_eleven_stable_contracts() {
        let registry = phase_contracts();
        assert_eq!(registry.len(), 11);
        for id in PHASE_CONTRACT_IDS {
            let contract = registry.get(id).expect("registry entry");
            assert_eq!(contract.id, id);
            assert_eq!(contract.version, 1);
            assert_eq!(contract.digest.value.len(), 64);
            assert!(
                contract
                    .digest
                    .value
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
            );
        }
        // A second access returns the same contracts (OnceLock cache).
        let again = phase_contracts();
        assert_eq!(
            again.get("planning").expect("planning").digest,
            registry.get("planning").expect("planning").digest
        );
    }

    #[test]
    fn registry_digests_match_the_type_script_oracle() {
        // Pinned from the differential oracle probe
        // (tests/differential/probes/icm-phase-contract-oracle.mjs,
        // op "registry") over packages/core/src/context/
        // phase-contract.ts. Any intentional payload change must move
        // these pins together with the corpus.
        let expected = [
            (
                "acceptance",
                "cb3e5862a4bce62793762aa4babd576aa2c5c85128a80d429e75fc7daeb30ad1",
            ),
            (
                "approval",
                "39e2d3255512b4e60ebddec2305735c367221f0b20bd71db63db3cdb7d038e8d",
            ),
            (
                "impact",
                "ada66731c065116a637c507d308d1cb41e4ebca7c4a85885cd95382250c2fbe1",
            ),
            (
                "inspection",
                "7cbe15ccba22d020c839ea703dd906f6528b5102bd9d0c8bf978c041d23508a3",
            ),
            (
                "mutation",
                "580ecae8c92068b3fbc181d200288201a0c7a1805324334803a103579e2bdd92",
            ),
            (
                "planning",
                "bcb96a2797d88e099b264e286314014dac91a4e8d3e84f01f73991f47e404583",
            ),
            (
                "preparation",
                "307f1fca14048d33377a23eaa1a4817644bd41ec5d2647354e63cdb61ea721ed",
            ),
            (
                "repair",
                "2c8192c3fa712bc4f4b2957cd3965cf47fddde7d033fe6649562e96115b65f6c",
            ),
            (
                "review",
                "0a460fc9ad54fe93eb30fac0288fd9f9339e16af41545a9ee7a11ecab43a43d0",
            ),
            (
                "validation",
                "cdd77658774cb6b234319842fc4f2c2f97f4721e7e9f2f977bd13ebe55f15363",
            ),
            (
                "verification",
                "fe54ad3c0ba0133497ff16e454ad16c8682ca224df3a747b70b50b65ec06cb67",
            ),
        ];
        let registry = phase_contracts();
        assert_eq!(expected.len(), registry.len());
        for (id, digest) in expected {
            assert_eq!(
                registry.get(id).expect("registry entry").digest.value,
                digest,
                "registry digest drifted for {id}"
            );
        }
    }

    #[test]
    fn phase_class_mapping_is_deterministic() {
        assert_eq!(
            context_classes_for_phase("planning"),
            vec![
                ContextClass::Global,
                ContextClass::Routing,
                ContextClass::StableReference,
                ContextClass::PhaseContract,
            ]
        );
        assert!(context_classes_for_phase("no-such-phase").is_empty());
        assert_eq!(
            class_artifact_kinds(ContextClass::PhaseContract),
            ["PhaseContract"]
        );
        assert_eq!(super::CONTEXT_CLASSES.len(), 5);
    }
}
