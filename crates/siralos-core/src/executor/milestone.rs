//! Structured MilestoneManifest and the standard acceptance library
//! (executor briefing foundation, ADR 0022).
//!
//! A manifest contains ONLY the requirements unique to one milestone and
//! is immutable and versioned like every other Siralos artifact. It
//! grants nothing: acceptance is satisfied only by host-observed evidence
//! through the acceptance evaluator.

use serde_json::json;

use crate::identity::{canonical_json_value, sha256_hex};
use crate::task::EvidenceKind;

/// Stable standard-acceptance id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardAcceptanceId {
    /// No workspace mutation.
    NoWorkspaceMutation,
    /// No process execution.
    NoProcessExecution,
    /// No network.
    NoNetwork,
    /// No secret output.
    NoSecretOutput,
    /// No tool leakage.
    NoToolLeakage,
    /// Full validation ran and passed.
    FullValidation,
}

impl StandardAcceptanceId {
    /// Stable machine-readable spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            StandardAcceptanceId::NoWorkspaceMutation => {
                "STANDARD.NO_WORKSPACE_MUTATION"
            }
            StandardAcceptanceId::NoProcessExecution => {
                "STANDARD.NO_PROCESS_EXECUTION"
            }
            StandardAcceptanceId::NoNetwork => "STANDARD.NO_NETWORK",
            StandardAcceptanceId::NoSecretOutput => {
                "STANDARD.NO_SECRET_OUTPUT"
            }
            StandardAcceptanceId::NoToolLeakage => "STANDARD.NO_TOOL_LEAKAGE",
            StandardAcceptanceId::FullValidation => "STANDARD.FULL_VALIDATION",
        }
    }

    /// Parse a raw id against the closed vocabulary.
    pub(crate) fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "STANDARD.NO_WORKSPACE_MUTATION" => Self::NoWorkspaceMutation,
            "STANDARD.NO_PROCESS_EXECUTION" => Self::NoProcessExecution,
            "STANDARD.NO_NETWORK" => Self::NoNetwork,
            "STANDARD.NO_SECRET_OUTPUT" => Self::NoSecretOutput,
            "STANDARD.NO_TOOL_LEAKAGE" => Self::NoToolLeakage,
            "STANDARD.FULL_VALIDATION" => Self::FullValidation,
            _ => return None,
        })
    }
}

/// One reusable standard acceptance definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StandardAcceptanceDefinition {
    /// Stable id.
    pub id: StandardAcceptanceId,
    /// Exact description text.
    pub description: &'static str,
    /// Evidence kinds whose host-attached records count toward the property.
    pub evidence_kinds: &'static [EvidenceKind],
}

/// The fixed standard acceptance vocabulary (not an arbitrary policy
/// language); definitions describe evidence kinds only and grant nothing.
pub const STANDARD_ACCEPTANCE_DEFINITIONS: &[StandardAcceptanceDefinition] = &[
    StandardAcceptanceDefinition {
        id: StandardAcceptanceId::NoWorkspaceMutation,
        description: "No workspace mutation: no create/edit/delete/undo of workspace files.",
        evidence_kinds: &[
            EvidenceKind::ValidationResult,
            EvidenceKind::ReviewResult,
        ],
    },
    StandardAcceptanceDefinition {
        id: StandardAcceptanceId::NoProcessExecution,
        description: "No process execution or engine launch for the inspected surface.",
        evidence_kinds: &[
            EvidenceKind::ValidationResult,
            EvidenceKind::ReviewResult,
        ],
    },
    StandardAcceptanceDefinition {
        id: StandardAcceptanceId::NoNetwork,
        description: "No network access during the task.",
        evidence_kinds: &[
            EvidenceKind::ValidationResult,
            EvidenceKind::ReviewResult,
        ],
    },
    StandardAcceptanceDefinition {
        id: StandardAcceptanceId::NoSecretOutput,
        description: "No secrets or absolute host paths appear in provider-visible output.",
        evidence_kinds: &[
            EvidenceKind::ReviewResult,
            EvidenceKind::ValidationResult,
        ],
    },
    StandardAcceptanceDefinition {
        id: StandardAcceptanceId::NoToolLeakage,
        description: "No native mutation tool surface leaks into provider-visible tools.",
        evidence_kinds: &[
            EvidenceKind::ReviewResult,
            EvidenceKind::ValidationResult,
        ],
    },
    StandardAcceptanceDefinition {
        id: StandardAcceptanceId::FullValidation,
        description: "The standard repository validation profile ran and passed.",
        evidence_kinds: &[EvidenceKind::ValidationResult],
    },
];

fn definition_of(
    id: StandardAcceptanceId,
) -> &'static StandardAcceptanceDefinition {
    STANDARD_ACCEPTANCE_DEFINITIONS
        .iter()
        .find(|definition| definition.id == id)
        .expect("registered definition")
}

/// Deterministic resolved evidence kinds for one requirement (manifest +
/// standards, first occurrence order preserved).
pub fn resolve_acceptance_evidence_kinds(
    evidence_kinds: &[EvidenceKind],
    standard_ids: &[StandardAcceptanceId],
) -> Vec<EvidenceKind> {
    let mut kinds: Vec<EvidenceKind> = evidence_kinds.to_vec();
    for standard_id in standard_ids {
        for kind in definition_of(*standard_id).evidence_kinds {
            if !kinds.contains(kind) {
                kinds.push(*kind);
            }
        }
    }
    kinds
}

/// One milestone prerequisite/deliverable/invariant/test entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MilestoneRequirement {
    /// Stable entry id.
    pub id: String,
    /// Bounded description.
    pub description: String,
}

/// One milestone deliverable.
pub type MilestoneDeliverable = MilestoneRequirement;

/// One milestone invariant.
pub type MilestoneInvariant = MilestoneRequirement;

/// One milestone test requirement.
pub type TestRequirement = MilestoneRequirement;

/// One acceptance requirement. Satisfaction is host-evidence-backed only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceRequirement {
    /// Stable acceptance id.
    pub id: String,
    /// Exact host check identity required by evidence.
    pub check_id: String,
    /// Bounded description.
    pub description: String,
    /// Evidence kinds whose host-attached records satisfy this requirement.
    pub evidence_kinds: Vec<EvidenceKind>,
    /// Optional link to a TaskContract criterion.
    pub criterion_id: Option<String>,
    /// Optional reusable standard acceptance references.
    pub standard_ids: Vec<StandardAcceptanceId>,
    /// Optional requirements stay `not_applicable` when no linked
    /// criterion exists for the current task.
    pub optional: bool,
}

/// Reference to another milestone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MilestoneRef {
    /// Referenced milestone id.
    pub id: String,
    /// Referenced manifest version.
    pub version: u64,
}

/// Immutable revisioned milestone manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MilestoneManifest {
    /// Stable milestone id.
    pub id: String,
    /// Immutable version identity; starts at 1 and only increases.
    pub version: u64,
    /// Bounded title.
    pub title: String,
    /// Bounded goal.
    pub goal: String,
    /// Prerequisites.
    pub prerequisites: Vec<MilestoneRequirement>,
    /// Deliverables.
    pub deliverables: Vec<MilestoneDeliverable>,
    /// Non-goals.
    pub non_goals: Vec<String>,
    /// Invariants.
    pub invariants: Vec<MilestoneInvariant>,
    /// Acceptance requirements.
    pub acceptance: Vec<AcceptanceRequirement>,
    /// Required tests.
    pub required_tests: Vec<TestRequirement>,
    /// Deterministic architecture-concern tags for context selection.
    pub architecture_concerns: Vec<String>,
    /// Only when this milestone adds/specializes validation beyond the
    /// profile.
    pub validation_profile:
        Option<crate::executor::contracts::ValidationProfileRef>,
    /// Next milestone reference.
    pub next_milestone: Option<MilestoneRef>,
}

/// Stable reference to one immutable manifest version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MilestoneManifestRef {
    /// Manifest id.
    pub id: String,
    /// Manifest version.
    pub version: u64,
}

/// Hard bounds for milestone manifests.
pub struct MilestoneManifestLimits;

impl MilestoneManifestLimits {
    /// Maximum id bytes.
    pub const MAX_ID_BYTES: usize = 32;
    /// Maximum title bytes.
    pub const MAX_TITLE_BYTES: usize = 256;
    /// Maximum goal bytes.
    pub const MAX_GOAL_BYTES: usize = 2048;
    /// Maximum prerequisites.
    pub const MAX_PREREQUISITES: usize = 16;
    /// Maximum deliverables.
    pub const MAX_DELIVERABLES: usize = 16;
    /// Maximum non-goals.
    pub const MAX_NON_GOALS: usize = 16;
    /// Maximum invariants.
    pub const MAX_INVARIANTS: usize = 16;
    /// Maximum acceptance requirements.
    pub const MAX_ACCEPTANCE: usize = 32;
    /// Maximum required tests.
    pub const MAX_REQUIRED_TESTS: usize = 16;
    /// Maximum entry bytes.
    pub const MAX_ENTRY_BYTES: usize = 512;
    /// Maximum concerns.
    pub const MAX_CONCERNS: usize = 12;
    /// Maximum concern bytes.
    pub const MAX_CONCERN_BYTES: usize = 64;
}

fn is_milestone_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 8
        && bytes[0].is_ascii_uppercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

fn is_entry_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_alphabetic()
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn is_acceptance_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    bytes.len() >= 2
        && bytes.len() <= 64
        && bytes[0].is_ascii_uppercase()
        && bytes[1].is_ascii_uppercase()
        && bytes[2..].iter().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn validate_entry(
    kind: &str,
    id: &str,
    description: &str,
) -> Result<MilestoneRequirement, String> {
    if !is_entry_id(id) {
        return Err(format!("Invalid {kind} id: {id}"));
    }
    let description = description.trim();
    if description.is_empty() {
        return Err(format!("{kind} {id} requires a description."));
    }
    if description.len() > MilestoneManifestLimits::MAX_ENTRY_BYTES {
        return Err(format!(
            "{kind} {id} exceeds {} UTF-8 bytes.",
            MilestoneManifestLimits::MAX_ENTRY_BYTES
        ));
    }
    Ok(MilestoneRequirement {
        id: id.to_owned(),
        description: description.to_owned(),
    })
}

fn copy_entries(
    kind: &str,
    values: &[MilestoneRequirement],
    max: usize,
) -> Result<Vec<MilestoneRequirement>, String> {
    if values.len() > max {
        return Err(format!("{kind} accepts at most {max} entries."));
    }
    let mut ids: Vec<&str> = Vec::new();
    for value in values {
        if ids.contains(&value.id.as_str()) {
            return Err(format!("Duplicate {kind} id: {}", value.id));
        }
        ids.push(value.id.as_str());
    }
    values
        .iter()
        .map(|value| validate_entry(kind, &value.id, &value.description))
        .collect()
}

fn copy_statements(
    kind: &str,
    values: &[String],
    max: usize,
) -> Result<Vec<String>, String> {
    if values.len() > max {
        return Err(format!("{kind} accepts at most {max} entries."));
    }
    values
        .iter()
        .map(|value| {
            let text = value.trim();
            if text.is_empty() {
                return Err(format!("{kind} entries must be non-empty."));
            }
            if text.len() > MilestoneManifestLimits::MAX_ENTRY_BYTES {
                return Err(format!(
                    "{kind} entry exceeds {} UTF-8 bytes.",
                    MilestoneManifestLimits::MAX_ENTRY_BYTES
                ));
            }
            Ok(text.to_owned())
        })
        .collect()
}

/// Creation input for an acceptance requirement; an omitted check id
/// deterministically defaults to the linked criterion or requirement id.
pub struct AcceptanceRequirementInput {
    /// Stable acceptance id.
    pub id: String,
    /// Optional explicit check id.
    pub check_id: Option<String>,
    /// Description.
    pub description: String,
    /// Direct evidence kinds.
    pub evidence_kinds: Vec<EvidenceKind>,
    /// Linked TaskContract criterion.
    pub criterion_id: Option<String>,
    /// Reusable standard ids (raw; validated against the closed vocabulary).
    pub standard_ids: Vec<String>,
    /// Optional flag.
    pub optional: bool,
}

fn validate_acceptance(
    requirements: &[AcceptanceRequirementInput],
) -> Result<Vec<AcceptanceRequirement>, String> {
    if requirements.len() > MilestoneManifestLimits::MAX_ACCEPTANCE {
        return Err(format!(
            "A milestone manifest accepts at most {} acceptance requirements.",
            MilestoneManifestLimits::MAX_ACCEPTANCE
        ));
    }
    let mut ids: Vec<&str> = Vec::new();
    let mut result = Vec::new();
    for requirement in requirements {
        if !is_acceptance_id(&requirement.id) {
            return Err(format!(
                "Invalid acceptance requirement id: {}",
                requirement.id
            ));
        }
        if ids.contains(&requirement.id.as_str()) {
            return Err(format!(
                "Duplicate acceptance requirement id: {}",
                requirement.id
            ));
        }
        ids.push(requirement.id.as_str());
        let default_check = requirement
            .criterion_id
            .clone()
            .unwrap_or_else(|| requirement.id.clone());
        let check_id = requirement
            .check_id
            .as_ref()
            .map(|value| value.trim().to_owned())
            .unwrap_or(default_check);
        if !is_entry_id(&check_id) {
            return Err(format!("Invalid acceptance check id: {check_id}"));
        }
        if let Some(criterion_id) = &requirement.criterion_id {
            if check_id != *criterion_id {
                return Err(format!(
                    "Acceptance requirement {} must use its linked criterion id as checkId.",
                    requirement.id
                ));
            }
        }
        let description = requirement.description.trim();
        if description.is_empty() {
            return Err(format!(
                "Acceptance requirement {} requires a description.",
                requirement.id
            ));
        }
        if description.len() > MilestoneManifestLimits::MAX_ENTRY_BYTES {
            return Err(format!(
                "Acceptance requirement {} exceeds {} UTF-8 bytes.",
                requirement.id,
                MilestoneManifestLimits::MAX_ENTRY_BYTES
            ));
        }
        let mut parsed_standard_ids = Vec::new();
        for raw_id in &requirement.standard_ids {
            match StandardAcceptanceId::parse(raw_id) {
                Some(standard_id) => parsed_standard_ids.push(standard_id),
                None => {
                    return Err(format!(
                        "Acceptance requirement {} references unknown standard acceptance {}.",
                        requirement.id, raw_id
                    ));
                }
            }
        }
        if requirement.evidence_kinds.is_empty()
            && requirement.criterion_id.is_none()
            && parsed_standard_ids.is_empty()
        {
            return Err(format!(
                "Acceptance requirement {} must declare evidenceKinds, a criterionId, or standardIds.",
                requirement.id
            ));
        }
        result.push(AcceptanceRequirement {
            id: requirement.id.clone(),
            check_id,
            description: description.to_owned(),
            evidence_kinds: requirement.evidence_kinds.clone(),
            criterion_id: requirement.criterion_id.clone(),
            standard_ids: parsed_standard_ids,
            optional: requirement.optional,
        });
    }
    Ok(result)
}

fn validate_concerns(concerns: &[String]) -> Result<Vec<String>, String> {
    if concerns.len() > MilestoneManifestLimits::MAX_CONCERNS {
        return Err(format!(
            "A milestone manifest accepts at most {} architecture concerns.",
            MilestoneManifestLimits::MAX_CONCERNS
        ));
    }
    let mut seen: Vec<&str> = Vec::new();
    for concern in concerns {
        let text = concern.trim();
        if text.is_empty() {
            return Err("Architecture concerns must be non-empty.".to_owned());
        }
        if text.len() > MilestoneManifestLimits::MAX_CONCERN_BYTES {
            return Err(format!(
                "An architecture concern exceeds {} UTF-8 bytes.",
                MilestoneManifestLimits::MAX_CONCERN_BYTES
            ));
        }
        if seen.contains(&text) {
            return Err(format!("Duplicate architecture concern: {text}"));
        }
        seen.push(text);
    }
    Ok(concerns.iter().map(|concern| concern.trim().to_owned()).collect())
}

/// Full creation input for a milestone manifest.
#[allow(clippy::struct_excessive_bools)]
pub struct CreateMilestoneManifestInput {
    /// Stable milestone id.
    pub id: String,
    /// Version (creation uses 1).
    pub version: u64,
    /// Title.
    pub title: String,
    /// Goal.
    pub goal: String,
    /// Prerequisites.
    pub prerequisites: Vec<MilestoneRequirement>,
    /// Deliverables.
    pub deliverables: Vec<MilestoneDeliverable>,
    /// Non-goals.
    pub non_goals: Vec<String>,
    /// Invariants.
    pub invariants: Vec<MilestoneInvariant>,
    /// Acceptance inputs.
    pub acceptance: Vec<AcceptanceRequirementInput>,
    /// Required tests.
    pub required_tests: Vec<TestRequirement>,
    /// Architecture concerns.
    pub architecture_concerns: Vec<String>,
    /// Optional validation profile.
    pub validation_profile:
        Option<crate::executor::contracts::ValidationProfileRef>,
    /// Optional next milestone.
    pub next_milestone: Option<MilestoneRef>,
}

fn validate_manifest_shape(
    input: &CreateMilestoneManifestInput,
) -> Result<MilestoneManifest, String> {
    if !is_milestone_id(&input.id) {
        return Err(format!("Invalid milestone id: {}", input.id));
    }
    if input.version < 1 {
        return Err(
            "A milestone manifest version must be at least 1.".to_owned()
        );
    }
    let title = input.title.trim();
    if title.is_empty() {
        return Err("A milestone manifest requires a title.".to_owned());
    }
    if title.len() > MilestoneManifestLimits::MAX_TITLE_BYTES {
        return Err(format!(
            "A milestone title exceeds {} UTF-8 bytes.",
            MilestoneManifestLimits::MAX_TITLE_BYTES
        ));
    }
    let goal = input.goal.trim();
    if goal.is_empty() {
        return Err("A milestone manifest requires a goal.".to_owned());
    }
    if goal.len() > MilestoneManifestLimits::MAX_GOAL_BYTES {
        return Err(format!(
            "A milestone goal exceeds {} UTF-8 bytes.",
            MilestoneManifestLimits::MAX_GOAL_BYTES
        ));
    }
    if input.acceptance.is_empty() {
        return Err(
            "A milestone manifest requires at least one acceptance requirement."
                .to_owned(),
        );
    }
    if let Some(profile) = &input.validation_profile {
        if profile.revision < 1 {
            return Err(
                "A validation profile revision must be at least 1.".to_owned()
            );
        }
        if profile.profile_id.trim().is_empty() {
            return Err(
                "A validation profile requires a profile id.".to_owned()
            );
        }
    }
    Ok(MilestoneManifest {
        id: input.id.clone(),
        version: input.version,
        title: title.to_owned(),
        goal: goal.to_owned(),
        prerequisites: copy_entries(
            "prerequisite",
            &input.prerequisites,
            MilestoneManifestLimits::MAX_PREREQUISITES,
        )?,
        deliverables: copy_entries(
            "deliverable",
            &input.deliverables,
            MilestoneManifestLimits::MAX_DELIVERABLES,
        )?,
        non_goals: copy_statements(
            "non-goal",
            &input.non_goals,
            MilestoneManifestLimits::MAX_NON_GOALS,
        )?,
        invariants: copy_entries(
            "invariant",
            &input.invariants,
            MilestoneManifestLimits::MAX_INVARIANTS,
        )?,
        acceptance: validate_acceptance(&input.acceptance)?,
        required_tests: copy_entries(
            "test requirement",
            &input.required_tests,
            MilestoneManifestLimits::MAX_REQUIRED_TESTS,
        )?,
        architecture_concerns: validate_concerns(
            &input.architecture_concerns,
        )?,
        validation_profile: input.validation_profile.as_ref().map(|profile| {
            crate::executor::contracts::ValidationProfileRef {
                profile_id: profile.profile_id.trim().to_owned(),
                revision: profile.revision,
            }
        }),
        next_milestone: input.next_milestone.clone(),
    })
}

/// Create the first immutable manifest version.
///
/// # Errors
///
/// Exact reference messages.
pub fn create_milestone_manifest(
    mut input: CreateMilestoneManifestInput,
) -> Result<MilestoneManifest, String> {
    input.version = 1;
    validate_manifest_shape(&input)
}

/// Produce the next immutable manifest version; the previous object is
/// untouched.
///
/// # Errors
///
/// Exact reference messages.
pub fn revise_milestone_manifest(
    previous: &MilestoneManifest,
    revise_title: Option<&str>,
    revise_goal: Option<&str>,
    revise_acceptance: Option<Vec<AcceptanceRequirementInput>>,
) -> Result<MilestoneManifest, String> {
    if previous.version < 1 || previous.version >= u64::MAX - 1 {
        return Err(
            "A previous manifest version must be an incrementable safe integer.".to_owned(),
        );
    }
    let input = CreateMilestoneManifestInput {
        id: previous.id.clone(),
        version: previous.version + 1,
        title: revise_title.unwrap_or(&previous.title).to_owned(),
        goal: revise_goal.unwrap_or(&previous.goal).to_owned(),
        prerequisites: previous.prerequisites.clone(),
        deliverables: previous.deliverables.clone(),
        non_goals: previous.non_goals.clone(),
        invariants: previous.invariants.clone(),
        acceptance: revise_acceptance.unwrap_or_else(|| {
            previous
                .acceptance
                .iter()
                .map(|requirement| AcceptanceRequirementInput {
                    id: requirement.id.clone(),
                    check_id: Some(requirement.check_id.clone()),
                    description: requirement.description.clone(),
                    evidence_kinds: requirement.evidence_kinds.clone(),
                    criterion_id: requirement.criterion_id.clone(),
                    standard_ids: requirement
                        .standard_ids
                        .iter()
                        .map(|id| id.as_str().to_owned())
                        .collect(),
                    optional: requirement.optional,
                })
                .collect()
        }),
        required_tests: previous.required_tests.clone(),
        architecture_concerns: previous.architecture_concerns.clone(),
        validation_profile: previous.validation_profile.clone(),
        next_milestone: previous.next_milestone.clone(),
    };
    validate_manifest_shape(&input)
}

/// Deterministic digest over a manifest version (canonical JSON). Absent
/// optional fields are omitted from the canonical form entirely.
pub fn compute_milestone_manifest_digest(
    manifest: &MilestoneManifest,
) -> String {
    sha256_hex(
        canonical_json_value(&manifest_canonical_value(manifest)).as_bytes(),
    )
}

fn manifest_canonical_value(
    manifest: &MilestoneManifest,
) -> serde_json::Value {
    use std::collections::BTreeMap;
    // Build with a BTreeMap so absent optional keys are truly absent.
    let mut map: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    map.insert("id".to_owned(), json!(manifest.id));
    map.insert("version".to_owned(), json!(manifest.version));
    map.insert("title".to_owned(), json!(manifest.title));
    map.insert("goal".to_owned(), json!(manifest.goal));
    map.insert(
        "prerequisites".to_owned(),
        json!(
            manifest
                .prerequisites
                .iter()
                .map(|entry| json!({
                    "id": entry.id, "description": entry.description,
                }))
                .collect::<Vec<_>>()
        ),
    );
    map.insert(
        "deliverables".to_owned(),
        json!(
            manifest
                .deliverables
                .iter()
                .map(|entry| json!({
                    "id": entry.id, "description": entry.description,
                }))
                .collect::<Vec<_>>()
        ),
    );
    map.insert("nonGoals".to_owned(), json!(manifest.non_goals));
    map.insert(
        "invariants".to_owned(),
        json!(
            manifest
                .invariants
                .iter()
                .map(|entry| json!({
                    "id": entry.id, "description": entry.description,
                }))
                .collect::<Vec<_>>()
        ),
    );
    map.insert("acceptance".to_owned(), manifest_acceptance_value(manifest));
    map.insert(
        "requiredTests".to_owned(),
        json!(
            manifest
                .required_tests
                .iter()
                .map(|entry| json!({
                    "id": entry.id, "description": entry.description,
                }))
                .collect::<Vec<_>>()
        ),
    );
    map.insert(
        "architectureConcerns".to_owned(),
        json!(manifest.architecture_concerns),
    );
    if let Some(profile) = &manifest.validation_profile {
        map.insert(
            "validationProfile".to_owned(),
            json!({ "profileId": profile.profile_id, "revision": profile.revision }),
        );
    }
    if let Some(next) = &manifest.next_milestone {
        map.insert(
            "nextMilestone".to_owned(),
            json!({ "id": next.id, "version": next.version }),
        );
    }
    serde_json::Value::Object(map.into_iter().collect())
}

fn manifest_acceptance_value(
    manifest: &MilestoneManifest,
) -> serde_json::Value {
    use std::collections::BTreeMap;
    let entries: Vec<serde_json::Value> = manifest
        .acceptance
        .iter()
        .map(|requirement| {
            let mut map: BTreeMap<String, serde_json::Value> = BTreeMap::new();
            map.insert("id".to_owned(), json!(requirement.id));
            map.insert("checkId".to_owned(), json!(requirement.check_id));
            map.insert(
                "description".to_owned(),
                json!(requirement.description),
            );
            if !requirement.evidence_kinds.is_empty() {
                map.insert(
                    "evidenceKinds".to_owned(),
                    json!(
                        requirement
                            .evidence_kinds
                            .iter()
                            .map(|kind| kind.as_str())
                            .collect::<Vec<_>>()
                    ),
                );
            }
            if let Some(criterion_id) = &requirement.criterion_id {
                map.insert("criterionId".to_owned(), json!(criterion_id));
            }
            if !requirement.standard_ids.is_empty() {
                map.insert(
                    "standardIds".to_owned(),
                    json!(
                        requirement
                            .standard_ids
                            .iter()
                            .map(|standard_id| standard_id.as_str())
                            .collect::<Vec<_>>()
                    ),
                );
            }
            if requirement.optional {
                map.insert("optional".to_owned(), json!(true));
            }
            serde_json::Value::Object(map.into_iter().collect())
        })
        .collect();
    serde_json::Value::Array(entries)
}
