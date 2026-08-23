//! Digest-backed manifests (Stage 3 — Content Identity & Delta
//! Verification, ADR 0028; R10a `content-identity.manifests`).
//!
//! Mirrors `packages/core/src/identity/manifests.ts`. Manifests reference
//! exact identities (digests) of the artifacts they cover — they never
//! duplicate giant artifact contents. Deltas between manifests project
//! what materially changed; the authoritative state always remains the
//! full current artifact.
//!
//! All aggregate digests flow through the single [`super::ArtifactDigest`]
//! architecture; entry ordering is deterministic code-unit order.

use super::{CanonicalValue, compute_artifact_digest};
use std::collections::BTreeMap;

/// Internal: hash one frozen-shape artifact payload whose type/schema are
/// compile-time constants (infallible by construction).
fn digest_payload(artifact_type: &str, payload: &CanonicalValue) -> String {
    compute_artifact_digest(artifact_type, 1, payload)
        .expect("constant artifact type and schema version")
        .value
}

fn object(entries: Vec<(&str, CanonicalValue)>) -> CanonicalValue {
    CanonicalValue::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn string(value: &str) -> CanonicalValue {
    CanonicalValue::Str(value.to_owned())
}

fn optional_string(value: &Option<String>) -> CanonicalValue {
    match value {
        Some(text) => CanonicalValue::Str(text.clone()),
        None => CanonicalValue::Null,
    }
}

// ---------------------------------------------------------------------------
// Guidance manifest (3.7B active documentation/instructions).
// ---------------------------------------------------------------------------

/// Kind of one guidance manifest entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuidanceEntryKind {
    /// Root AGENTS.md.
    RootAgents,
    /// Nested AGENTS.md.
    NestedAgents,
    /// Architecture index/document.
    Architecture,
    /// Accepted ADR.
    Adr,
    /// Development guide.
    Development,
}

impl GuidanceEntryKind {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RootAgents => "root-agents",
            Self::NestedAgents => "nested-agents",
            Self::Architecture => "architecture",
            Self::Adr => "adr",
            Self::Development => "development",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "root-agents" => Some(Self::RootAgents),
            "nested-agents" => Some(Self::NestedAgents),
            "architecture" => Some(Self::Architecture),
            "adr" => Some(Self::Adr),
            "development" => Some(Self::Development),
            _ => None,
        }
    }
}

/// One guidance manifest entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuidanceManifestEntry {
    /// Stable entry id.
    pub id: String,
    /// Entry kind protocol string. The oracle passes unknown kinds
    /// through verbatim into the aggregate payload (type-level union
    /// only), so the candidate keeps the raw string too.
    pub kind: String,
    /// Workspace-relative document path.
    pub path: String,
    /// SHA-256 of the exact document content.
    pub digest: String,
}

/// Exact active documentation/instructions selected for a task (3.7B).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuidanceManifest {
    /// Entries sorted by path.
    pub entries: Vec<GuidanceManifestEntry>,
    /// Deterministic aggregate digest.
    pub aggregate_digest: String,
}

/// Create a guidance manifest: entries are sorted by path and bound into
/// one domain-separated aggregate digest.
pub fn create_guidance_manifest(
    mut entries: Vec<GuidanceManifestEntry>,
) -> GuidanceManifest {
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    let payload_entries: Vec<CanonicalValue> = entries
        .iter()
        .map(|entry| {
            object(vec![
                ("id", string(&entry.id)),
                ("kind", string(&entry.kind)),
                ("path", string(&entry.path)),
                ("digest", string(&entry.digest)),
            ])
        })
        .collect();
    let aggregate_digest = digest_payload(
        "GuidanceManifest",
        &object(vec![("entries", CanonicalValue::Array(payload_entries))]),
    );
    GuidanceManifest { entries, aggregate_digest }
}

/// Derived delta between two guidance manifests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuidanceDelta {
    /// Base aggregate digest.
    pub base_digest: String,
    /// Result aggregate digest.
    pub result_digest: String,
    /// Paths present only in the result.
    pub added: Vec<String>,
    /// Paths present only in the base.
    pub removed: Vec<String>,
    /// Paths whose kind or content digest differs.
    pub changed: Vec<String>,
    /// Paths identical in kind and content digest.
    pub unchanged: Vec<String>,
    /// True when nothing differs.
    pub unchanged_content: bool,
}

/// Compute the guidance-manifest delta keyed by path.
#[must_use]
pub fn compute_guidance_delta(
    base: &GuidanceManifest,
    result: &GuidanceManifest,
) -> GuidanceDelta {
    let base_by_path: BTreeMap<&str, &GuidanceManifestEntry> = base
        .entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect();
    let result_by_path: BTreeMap<&str, &GuidanceManifestEntry> = result
        .entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect();
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    let mut unchanged = Vec::new();
    for path in result_by_path.keys() {
        if !base_by_path.contains_key(path) {
            added.push((*path).to_owned());
        }
    }
    for path in base_by_path.keys() {
        if !result_by_path.contains_key(path) {
            removed.push((*path).to_owned());
        }
    }
    for (path, base_entry) in &base_by_path {
        let Some(result_entry) = result_by_path.get(path) else {
            continue;
        };
        if base_entry.digest == result_entry.digest
            && base_entry.kind == result_entry.kind
        {
            unchanged.push((*path).to_owned());
        } else {
            changed.push((*path).to_owned());
        }
    }
    let unchanged_content =
        added.is_empty() && removed.is_empty() && changed.is_empty();
    GuidanceDelta {
        base_digest: base.aggregate_digest.clone(),
        result_digest: result.aggregate_digest.clone(),
        added,
        removed,
        changed,
        unchanged,
        unchanged_content,
    }
}

// ---------------------------------------------------------------------------
// Tool-surface manifests (per role/phase projected provider schemas).
// ---------------------------------------------------------------------------

/// Role whose projected tool surface is bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSurfaceRole {
    /// Planner.
    Planner,
    /// Developer.
    Developer,
    /// Reviewer.
    Reviewer,
    /// Executor.
    Executor,
}

impl ToolSurfaceRole {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planner => "planner",
            Self::Developer => "developer",
            Self::Reviewer => "reviewer",
            Self::Executor => "executor",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "planner" => Some(Self::Planner),
            "developer" => Some(Self::Developer),
            "reviewer" => Some(Self::Reviewer),
            "executor" => Some(Self::Executor),
            _ => None,
        }
    }
}

/// Phase whose projected tool surface is bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSurfacePhase {
    /// Planning phase.
    Planning,
    /// Inspection phase.
    Inspection,
    /// Mutation phase.
    Mutation,
    /// Review phase.
    Review,
    /// Repair phase.
    Repair,
}

impl ToolSurfacePhase {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planning => "planning",
            Self::Inspection => "inspection",
            Self::Mutation => "mutation",
            Self::Review => "review",
            Self::Repair => "repair",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "planning" => Some(Self::Planning),
            "inspection" => Some(Self::Inspection),
            "mutation" => Some(Self::Mutation),
            "review" => Some(Self::Review),
            "repair" => Some(Self::Repair),
            _ => None,
        }
    }
}

/// One projected tool with its canonical schema digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSurfaceEntry {
    /// Tool name.
    pub name: String,
    /// Canonical hash of the exact provider-visible tool schema.
    pub schema_digest: String,
}

/// Per-role/per-phase projected tool surface (actual provider schemas).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSurfaceManifest {
    /// Bound role.
    pub role: ToolSurfaceRole,
    /// Bound phase.
    pub phase: ToolSurfacePhase,
    /// Tools sorted by name.
    pub tools: Vec<ToolSurfaceEntry>,
    /// Deterministic surface digest.
    pub digest: String,
}

/// One provider-visible tool definition input.
pub struct ToolSurfaceDefinition<'a> {
    /// Tool name.
    pub name: &'a str,
    /// Exact provider-visible JSON schema.
    pub input_schema: &'a CanonicalValue,
    /// Provider-visible description.
    pub description: &'a str,
}

/// Create a tool-surface manifest: each tool's exact schema is digested
/// (`ToolSchema` over name/description/inputSchema), then the sorted
/// entries plus role/phase are bound into the surface digest
/// (`ToolSurfaceManifest`).
pub fn create_tool_surface_manifest(
    role: ToolSurfaceRole,
    phase: ToolSurfacePhase,
    tools: &[ToolSurfaceDefinition<'_>],
) -> ToolSurfaceManifest {
    let mut sorted: Vec<&ToolSurfaceDefinition<'_>> = tools.iter().collect();
    sorted.sort_by(|left, right| left.name.cmp(right.name));
    let entries: Vec<ToolSurfaceEntry> = sorted
        .iter()
        .map(|tool| ToolSurfaceEntry {
            name: (*tool.name).to_owned(),
            schema_digest: digest_payload(
                "ToolSchema",
                &object(vec![
                    ("name", string(tool.name)),
                    ("description", string(tool.description)),
                    ("inputSchema", tool.input_schema.clone()),
                ]),
            ),
        })
        .collect();
    let entry_values: Vec<CanonicalValue> = entries
        .iter()
        .map(|entry| {
            object(vec![
                ("name", string(&entry.name)),
                ("schemaDigest", string(&entry.schema_digest)),
            ])
        })
        .collect();
    let digest = digest_payload(
        "ToolSurfaceManifest",
        &object(vec![
            ("role", string(role.as_str())),
            ("phase", string(phase.as_str())),
            ("tools", CanonicalValue::Array(entry_values)),
        ]),
    );
    ToolSurfaceManifest { role, phase, tools: entries, digest }
}

/// Derived semantic surface delta between two roles/phases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSurfaceDelta {
    /// Base bound role.
    pub base_role: ToolSurfaceRole,
    /// Result bound role.
    pub result_role: ToolSurfaceRole,
    /// Base surface digest.
    pub base_digest: String,
    /// Result surface digest.
    pub result_digest: String,
    /// Names present only in the result.
    pub added: Vec<String>,
    /// Names present only in the base.
    pub removed: Vec<String>,
    /// Names whose schema digest differs.
    pub changed: Vec<String>,
    /// Names whose schema digest matches.
    pub retained: Vec<String>,
}

/// Compute the tool-surface delta keyed by tool name.
#[must_use]
pub fn compute_tool_surface_delta(
    base: &ToolSurfaceManifest,
    result: &ToolSurfaceManifest,
) -> ToolSurfaceDelta {
    let base_by_name: BTreeMap<&str, &ToolSurfaceEntry> =
        base.tools.iter().map(|tool| (tool.name.as_str(), tool)).collect();
    let result_by_name: BTreeMap<&str, &ToolSurfaceEntry> =
        result.tools.iter().map(|tool| (tool.name.as_str(), tool)).collect();
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    let mut retained = Vec::new();
    for name in result_by_name.keys() {
        if !base_by_name.contains_key(name) {
            added.push((*name).to_owned());
        }
    }
    for name in base_by_name.keys() {
        if !result_by_name.contains_key(name) {
            removed.push((*name).to_owned());
        }
    }
    for (name, base_tool) in &base_by_name {
        let Some(result_tool) = result_by_name.get(name) else {
            continue;
        };
        if base_tool.schema_digest == result_tool.schema_digest {
            retained.push((*name).to_owned());
        } else {
            changed.push((*name).to_owned());
        }
    }
    ToolSurfaceDelta {
        base_role: base.role,
        result_role: result.role,
        base_digest: base.digest.clone(),
        result_digest: result.digest.clone(),
        added,
        removed,
        changed,
        retained,
    }
}

// ---------------------------------------------------------------------------
// Capability snapshot + execution-input manifests.
// ---------------------------------------------------------------------------

/// Effective task-visible capability state identity (host-owned,
/// secret-free). The snapshot payload is the caller's canonical value.
#[must_use]
pub fn compute_capability_snapshot_digest(
    snapshot: &CanonicalValue,
) -> String {
    digest_payload("CapabilitySnapshot", snapshot)
}

/// One exact input reference inside an execution input manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionInputReference {
    /// Stable input id (taskContract, taskPlan, executionContract,
    /// milestone, guidance, toolSurface, capability, workspaceScope,
    /// sourceRevisions).
    pub id: String,
    /// Lifecycle revision when known.
    pub revision: Option<u64>,
    /// Exact content digest of the referenced artifact; null when
    /// unknown.
    pub digest: Option<String>,
}

/// Immutable exact effective input environment of one execution
/// iteration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionInputManifest {
    /// Owning task id.
    pub task_id: String,
    /// Iteration number.
    pub iteration: u64,
    /// Inputs sorted by id.
    pub inputs: Vec<ExecutionInputReference>,
    /// Deterministic environment digest.
    pub digest: String,
}

/// Create an execution input manifest: inputs are sorted by id and bound
/// with the task/iteration into one digest.
pub fn create_execution_input_manifest(
    task_id: &str,
    iteration: u64,
    mut inputs: Vec<ExecutionInputReference>,
) -> ExecutionInputManifest {
    inputs.sort_by(|left, right| left.id.cmp(&right.id));
    let input_values: Vec<CanonicalValue> = inputs
        .iter()
        .map(|input| {
            object(vec![
                ("id", string(&input.id)),
                (
                    "revision",
                    match input.revision {
                        Some(revision) => CanonicalValue::U64(revision),
                        None => CanonicalValue::Null,
                    },
                ),
                ("digest", optional_string(&input.digest)),
            ])
        })
        .collect();
    let digest = digest_payload(
        "ExecutionInputManifest",
        &object(vec![
            ("taskId", string(task_id)),
            ("iteration", CanonicalValue::U64(iteration)),
            ("inputs", CanonicalValue::Array(input_values)),
        ]),
    );
    ExecutionInputManifest {
        task_id: task_id.to_owned(),
        iteration,
        inputs,
        digest,
    }
}

/// One changed input reference between two execution input manifests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionInputChange {
    /// Changed input id.
    pub id: String,
    /// Base digest (null when newly added).
    pub before: Option<String>,
    /// Result digest (null when removed).
    pub after: Option<String>,
}

/// Derived semantic delta between two execution input manifests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionInputDelta {
    /// Base environment digest.
    pub base_digest: String,
    /// Result environment digest.
    pub result_digest: String,
    /// Added/removed/digest-or-revision-changed inputs.
    pub changed: Vec<ExecutionInputChange>,
    /// Inputs identical in digest and revision.
    pub unchanged: Vec<String>,
    /// True when nothing differs.
    pub unchanged_content: bool,
}

/// Compute the execution-input delta keyed by input id. A change in
/// either the content digest or the lifecycle revision counts; removals
/// report `after: null` and additions `before: null`.
#[must_use]
pub fn compute_execution_input_delta(
    base: &ExecutionInputManifest,
    result: &ExecutionInputManifest,
) -> ExecutionInputDelta {
    let base_by_id: BTreeMap<&str, &ExecutionInputReference> =
        base.inputs.iter().map(|input| (input.id.as_str(), input)).collect();
    let result_by_id: BTreeMap<&str, &ExecutionInputReference> =
        result.inputs.iter().map(|input| (input.id.as_str(), input)).collect();
    let mut changed: Vec<ExecutionInputChange> = Vec::new();
    let mut unchanged: Vec<String> = Vec::new();
    for (id, base_input) in &base_by_id {
        let Some(result_input) = result_by_id.get(id) else {
            changed.push(ExecutionInputChange {
                id: (*id).to_owned(),
                before: base_input.digest.clone(),
                after: None,
            });
            continue;
        };
        if base_input.digest == result_input.digest
            && base_input.revision == result_input.revision
        {
            unchanged.push((*id).to_owned());
        } else {
            changed.push(ExecutionInputChange {
                id: (*id).to_owned(),
                before: base_input.digest.clone(),
                after: result_input.digest.clone(),
            });
        }
    }
    for (id, result_input) in &result_by_id {
        if !base_by_id.contains_key(id) {
            changed.push(ExecutionInputChange {
                id: (*id).to_owned(),
                before: None,
                after: result_input.digest.clone(),
            });
        }
    }
    let unchanged_content = changed.is_empty();
    ExecutionInputDelta {
        base_digest: base.digest.clone(),
        result_digest: result.digest.clone(),
        changed,
        unchanged,
        unchanged_content,
    }
}

// ---------------------------------------------------------------------------
// Validation result identity + delta.
// ---------------------------------------------------------------------------

/// Stable identity of one structured validation run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationResultIdentity {
    /// Caller-supplied validation id.
    pub validation_id: String,
    /// Identity of the validation plan/command that produced the result.
    pub plan_identity: Option<String>,
    /// Exact digest of the structured result payload.
    pub result_digest: String,
    /// Bounded evidence references backing the run.
    pub evidence_refs: Vec<String>,
}

/// Create one validation result identity over the exact result payload
/// (`ValidationResult`).
pub fn create_validation_result_identity(
    validation_id: &str,
    plan_identity: Option<&str>,
    result: &CanonicalValue,
    evidence_refs: &[String],
) -> ValidationResultIdentity {
    let result_digest = digest_payload("ValidationResult", result);
    ValidationResultIdentity {
        validation_id: validation_id.to_owned(),
        plan_identity: plan_identity.map(str::to_owned),
        result_digest,
        evidence_refs: evidence_refs.to_vec(),
    }
}

/// One pass/fail observation inside a validation delta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidationObservation {
    /// Stable observation id (test/criterion id).
    pub id: &'static str,
    /// Pass state.
    pub passed: bool,
}

/// Derived semantic delta between two validation result sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationDelta {
    /// Base identity as provided (null when absent).
    pub base_identity: Option<String>,
    /// Result identity as provided (empty when absent).
    pub result_identity: String,
    /// Ids now passing that were not passing before (or are new).
    pub newly_passing: Vec<String>,
    /// Ids failing before and still failing.
    pub still_failing: Vec<String>,
    /// Ids now failing that were not failing before (or are new).
    pub new_failures: Vec<String>,
    /// Ids whose pass state did not change.
    pub unchanged_ids: Vec<String>,
}

fn observations_map(
    observations: &[ValidationObservation],
) -> BTreeMap<&str, bool> {
    observations
        .iter()
        .map(|observation| (observation.id, observation.passed))
        .collect()
}

/// Compute the validation delta between two observation sets. New ids
/// split into `newly_passing` / `new_failures`; identical-state ids land
/// in `unchanged_ids` (still-failing ones additionally in
/// `still_failing`); flipped ids land in the matching transition bucket.
#[must_use]
pub fn compute_validation_delta(
    base: &[ValidationObservation],
    result: &[ValidationObservation],
    base_identity: Option<&str>,
    result_identity: Option<&str>,
) -> ValidationDelta {
    let base_by_id = observations_map(base);
    let result_by_id = observations_map(result);
    let mut newly_passing = Vec::new();
    let mut still_failing = Vec::new();
    let mut new_failures = Vec::new();
    let mut unchanged_ids = Vec::new();
    for (id, result_passed) in &result_by_id {
        let Some(base_passed) = base_by_id.get(id) else {
            if *result_passed {
                newly_passing.push((*id).to_owned());
            } else {
                new_failures.push((*id).to_owned());
            }
            continue;
        };
        if base_passed == result_passed {
            unchanged_ids.push((*id).to_owned());
            if !result_passed {
                still_failing.push((*id).to_owned());
            }
        } else if *result_passed {
            newly_passing.push((*id).to_owned());
        } else {
            new_failures.push((*id).to_owned());
        }
    }
    ValidationDelta {
        base_identity: base_identity.map(str::to_owned),
        result_identity: result_identity.unwrap_or_default().to_owned(),
        newly_passing,
        still_failing,
        new_failures,
        unchanged_ids,
    }
}

// ---------------------------------------------------------------------------
// Review input, acceptance evidence, and convenience digests.
// ---------------------------------------------------------------------------

/// One source revision bound into a review input manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewSourceRevision {
    /// Workspace-relative path.
    pub path: String,
    /// Exact revision handle.
    pub revision: String,
}

/// Review input identity: binds one review to its exact inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewInputManifest {
    /// Caller-supplied review id.
    pub review_id: String,
    /// Owning task id.
    pub task_id: String,
    /// Exact TaskContract content digest under review.
    pub task_contract_digest: String,
    /// Exact change-set/diff identity under review.
    pub changeset_digest: String,
    /// Review-context digest when derived.
    pub review_context_digest: Option<String>,
    /// Acceptance criteria set digest.
    pub acceptance_digest: String,
    /// Validation evidence set digest when present.
    pub validation_evidence_digest: Option<String>,
    /// Source revisions reviewed (sorted by path).
    pub source_revisions: Vec<ReviewSourceRevision>,
    /// Deterministic binding digest.
    pub digest: String,
}

/// Inputs for [`create_review_input_manifest`].
pub struct CreateReviewInputManifest {
    /// Caller-supplied review id.
    pub review_id: String,
    /// Owning task id.
    pub task_id: String,
    /// Exact TaskContract content digest.
    pub task_contract_digest: String,
    /// Change-set/diff identity.
    pub changeset_digest: String,
    /// Review-context digest when derived.
    pub review_context_digest: Option<String>,
    /// Acceptance criteria set digest.
    pub acceptance_digest: String,
    /// Validation evidence digest when present.
    pub validation_evidence_digest: Option<String>,
    /// Source revisions (sorted here).
    pub source_revisions: Vec<ReviewSourceRevision>,
}

/// Create the review input manifest: source revisions are sorted by
/// path and every binding field enters one domain-separated digest
/// (`ReviewInputManifest`).
pub fn create_review_input_manifest(
    input: CreateReviewInputManifest,
) -> ReviewInputManifest {
    let mut source_revisions = input.source_revisions;
    source_revisions.sort_by(|left, right| left.path.cmp(&right.path));
    let revision_values: Vec<CanonicalValue> = source_revisions
        .iter()
        .map(|revision| {
            object(vec![
                ("path", string(&revision.path)),
                ("revision", string(&revision.revision)),
            ])
        })
        .collect();
    let digest = digest_payload(
        "ReviewInputManifest",
        &object(vec![
            ("reviewId", string(&input.review_id)),
            ("taskId", string(&input.task_id)),
            ("taskContractDigest", string(&input.task_contract_digest)),
            ("changesetDigest", string(&input.changeset_digest)),
            (
                "reviewContextDigest",
                optional_string(&input.review_context_digest),
            ),
            ("acceptanceDigest", string(&input.acceptance_digest)),
            (
                "validationEvidenceDigest",
                optional_string(&input.validation_evidence_digest),
            ),
            ("sourceRevisions", CanonicalValue::Array(revision_values)),
        ]),
    );
    ReviewInputManifest {
        review_id: input.review_id,
        task_id: input.task_id,
        task_contract_digest: input.task_contract_digest,
        changeset_digest: input.changeset_digest,
        review_context_digest: input.review_context_digest,
        acceptance_digest: input.acceptance_digest,
        validation_evidence_digest: input.validation_evidence_digest,
        source_revisions,
        digest,
    }
}

/// One evidence entry inside an acceptance evidence manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceEvidenceEntry {
    /// Evidence id.
    pub evidence_id: String,
    /// Evidence kind string.
    pub kind: String,
    /// Exact content digest of the evidence payload.
    pub digest: String,
}

/// Aggregate identity of the exact evidence set backing one acceptance
/// result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceEvidenceManifest {
    /// Owning task id.
    pub task_id: String,
    /// Bound criterion id, or whole-task when `None`.
    pub criterion_id: Option<String>,
    /// Evidence sorted by evidence id.
    pub evidence: Vec<AcceptanceEvidenceEntry>,
    /// Deterministic aggregate digest.
    pub digest: String,
}

/// Inputs for [`create_acceptance_evidence_manifest`].
pub struct CreateAcceptanceEvidenceManifest {
    /// Owning task id.
    pub task_id: String,
    /// Bound criterion id, or whole-task when `None`.
    pub criterion_id: Option<String>,
    /// Evidence entries (sorted here by evidence id).
    pub evidence: Vec<AcceptanceEvidenceEntry>,
}

/// Create the acceptance evidence manifest (`AcceptanceEvidenceManifest`
/// over taskId/criterionId/sorted evidence).
pub fn create_acceptance_evidence_manifest(
    input: CreateAcceptanceEvidenceManifest,
) -> AcceptanceEvidenceManifest {
    let mut evidence = input.evidence;
    evidence.sort_by(|left, right| left.evidence_id.cmp(&right.evidence_id));
    let evidence_values: Vec<CanonicalValue> = evidence
        .iter()
        .map(|entry| {
            object(vec![
                ("evidenceId", string(&entry.evidence_id)),
                ("kind", string(&entry.kind)),
                ("digest", string(&entry.digest)),
            ])
        })
        .collect();
    let digest = digest_payload(
        "AcceptanceEvidenceManifest",
        &object(vec![
            ("taskId", string(&input.task_id)),
            ("criterionId", optional_string(&input.criterion_id)),
            ("evidence", CanonicalValue::Array(evidence_values)),
        ]),
    );
    AcceptanceEvidenceManifest {
        task_id: input.task_id,
        criterion_id: input.criterion_id,
        evidence,
        digest,
    }
}

/// One acceptance criterion for [`compute_acceptance_criteria_digest`].
pub struct AcceptanceCriteriaEntry<'a> {
    /// Criterion id.
    pub id: &'a str,
    /// Description.
    pub description: &'a str,
    /// Verification kind string.
    pub verification_kind: &'a str,
}

/// Convenience: digest of a canonical acceptance-criteria set
/// (`AcceptanceCriteria` over {criteria}).
#[must_use]
pub fn compute_acceptance_criteria_digest(
    criteria: &[AcceptanceCriteriaEntry<'_>],
) -> String {
    let values: Vec<CanonicalValue> = criteria
        .iter()
        .map(|criterion| {
            object(vec![
                ("id", string(criterion.id)),
                ("description", string(criterion.description)),
                ("verificationKind", string(criterion.verification_kind)),
            ])
        })
        .collect();
    digest_payload(
        "AcceptanceCriteria",
        &object(vec![("criteria", CanonicalValue::Array(values))]),
    )
}

/// One evidence entry for [`compute_validation_evidence_digest`].
pub struct ValidationEvidenceEntry<'a> {
    /// Evidence id.
    pub id: &'a str,
    /// Evidence kind string.
    pub kind: &'a str,
    /// Exact content payload.
    pub content: &'a CanonicalValue,
}

/// Convenience: digest of a canonical evidence set
/// (`ValidationEvidence` over id + kind + content).
#[must_use]
pub fn compute_validation_evidence_digest(
    evidence: &[ValidationEvidenceEntry<'_>],
) -> String {
    let values: Vec<CanonicalValue> = evidence
        .iter()
        .map(|entry| {
            object(vec![
                ("id", string(entry.id)),
                ("kind", string(entry.kind)),
                ("content", entry.content.clone()),
            ])
        })
        .collect();
    digest_payload(
        "ValidationEvidence",
        &object(vec![("evidence", CanonicalValue::Array(values))]),
    )
}

/// Convenience: canonical change-set identity (`ChangeSet` over the
/// exact payload).
#[must_use]
pub fn canonical_changeset_identity(payload: &CanonicalValue) -> String {
    digest_payload("ChangeSet", payload)
}

#[cfg(test)]
mod tests {
    use super::{
        GuidanceManifestEntry, ToolSurfaceDefinition,
        ToolSurfacePhase, ToolSurfaceRole, ValidationObservation,
        compute_acceptance_criteria_digest,
        compute_capability_snapshot_digest, compute_execution_input_delta,
        compute_guidance_delta, compute_tool_surface_delta,
        compute_validation_delta, create_execution_input_manifest,
        create_guidance_manifest, create_review_input_manifest,
        create_tool_surface_manifest, create_validation_result_identity,
    };
    use crate::identity::CanonicalValue;

    fn guidance_entry(path: &str, digest: &str) -> GuidanceManifestEntry {
        GuidanceManifestEntry {
            id: format!("entry-{path}"),
            kind: "architecture".to_owned(),
            path: path.to_owned(),
            digest: digest.to_owned(),
        }
    }

    fn object(entries: &[(&str, &str)]) -> CanonicalValue {
        CanonicalValue::Object(
            entries
                .iter()
                .map(|(key, value)| {
                    (
                        (*key).to_owned(),
                        CanonicalValue::Str((*value).to_owned()),
                    )
                })
                .collect(),
        )
    }

    #[test]
    fn guidance_entries_sort_by_path_and_bind_an_aggregate_digest() {
        let manifest = create_guidance_manifest(vec![
            guidance_entry("docs/b.md", "d2"),
            guidance_entry("docs/a.md", "d1"),
        ]);
        assert_eq!(
            manifest
                .entries
                .iter()
                .map(|entry| entry.path.as_str())
                .collect::<Vec<_>>(),
            vec!["docs/a.md", "docs/b.md"]
        );
        assert_eq!(manifest.aggregate_digest.len(), 64);

        let reordered = create_guidance_manifest(vec![
            guidance_entry("docs/b.md", "d2"),
            guidance_entry("docs/a.md", "d1"),
        ]);
        assert_eq!(manifest, reordered);
        let changed = create_guidance_manifest(vec![
            guidance_entry("docs/a.md", "DIFFERENT"),
            guidance_entry("docs/b.md", "d2"),
        ]);
        assert_ne!(manifest.aggregate_digest, changed.aggregate_digest);
    }

    #[test]
    fn guidance_delta_classifies_adds_removes_changes_and_unchanged() {
        let base = create_guidance_manifest(vec![
            guidance_entry("kept.md", "same"),
            guidance_entry("changed.md", "old"),
            guidance_entry("removed.md", "gone"),
        ]);
        let result = create_guidance_manifest(vec![
            guidance_entry("added.md", "new"),
            guidance_entry("changed.md", "new-content"),
            guidance_entry("kept.md", "same"),
        ]);
        let delta = compute_guidance_delta(&base, &result);
        assert_eq!(delta.added, vec!["added.md"]);
        assert_eq!(delta.removed, vec!["removed.md"]);
        assert_eq!(delta.changed, vec!["changed.md"]);
        assert_eq!(delta.unchanged, vec!["kept.md"]);
        assert!(!delta.unchanged_content);
        assert_ne!(delta.base_digest, delta.result_digest);
    }

    #[test]
    fn tool_surface_schemas_are_digested_per_tool_and_bound_to_role_phase() {
        let schema_a = object(&[("type", "object")]);
        let tools = [
            ToolSurfaceDefinition {
                name: "zeta.tool",
                input_schema: &schema_a,
                description: "second",
            },
            ToolSurfaceDefinition {
                name: "alpha.tool",
                input_schema: &schema_a,
                description: "first",
            },
        ];
        let surface = create_tool_surface_manifest(
            ToolSurfaceRole::Developer,
            ToolSurfacePhase::Inspection,
            &tools,
        );
        assert_eq!(
            surface
                .tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha.tool", "zeta.tool"]
        );
        assert_eq!(surface.tools[0].schema_digest.len(), 64);
        let same = create_tool_surface_manifest(
            ToolSurfaceRole::Developer,
            ToolSurfacePhase::Inspection,
            &tools,
        );
        assert_eq!(surface.digest, same.digest);
        let different_description = create_tool_surface_manifest(
            ToolSurfaceRole::Developer,
            ToolSurfacePhase::Inspection,
            &[ToolSurfaceDefinition {
                name: "alpha.tool",
                input_schema: &schema_a,
                description: "CHANGED",
            }],
        );
        assert_ne!(surface.digest, different_description.digest);
    }

    #[test]
    fn tool_surface_delta_classifies_by_schema_digest() {
        let schema = object(&[("type", "object")]);
        let old_schema = object(&[("type", "string")]);
        let base_tools = [
            ToolSurfaceDefinition {
                name: "kept.tool",
                input_schema: &schema,
                description: "same",
            },
            ToolSurfaceDefinition {
                name: "changed.tool",
                input_schema: &old_schema,
                description: "before",
            },
        ];
        let base = create_tool_surface_manifest(
            ToolSurfaceRole::Reviewer,
            ToolSurfacePhase::Review,
            &base_tools,
        );
        let other_schema = object(&[("type", "string")]);
        let new_schema = object(&[("type", "number")]);
        let result_tools = [
            ToolSurfaceDefinition {
                name: "kept.tool",
                input_schema: &schema,
                description: "same",
            },
            ToolSurfaceDefinition {
                name: "added.tool",
                input_schema: &other_schema,
                description: "x",
            },
            ToolSurfaceDefinition {
                name: "changed.tool",
                input_schema: &new_schema,
                description: "y",
            },
        ];
        let result = create_tool_surface_manifest(
            ToolSurfaceRole::Reviewer,
            ToolSurfacePhase::Review,
            &result_tools,
        );
        let delta = compute_tool_surface_delta(&base, &result);
        assert_eq!(delta.retained, vec!["kept.tool"]);
        assert_eq!(delta.added, vec!["added.tool"]);
        assert_eq!(delta.changed, vec!["changed.tool"]);
    }

    #[test]
    fn acceptance_criteria_digest_is_content_sensitive() {
        let criteria = [super::AcceptanceCriteriaEntry {
            id: "ac1",
            description: "it works",
            verification_kind: "deterministic",
        }];
        assert_eq!(compute_acceptance_criteria_digest(&criteria).len(), 64);
    }

    #[test]
    fn capability_snapshot_digest_is_content_sensitive() {
        let snapshot = object(&[("tool", "read"), ("allowed", "yes")]);
        let digest = compute_capability_snapshot_digest(&snapshot);
        assert_eq!(digest.len(), 64);
        let widened = object(&[("tool", "read"), ("allowed", "no")]);
        assert_ne!(digest, compute_capability_snapshot_digest(&widened));
    }

    #[test]
    fn execution_inputs_sort_and_delta_reports_removals_as_null_after() {
        let base = create_execution_input_manifest(
            "task-1",
            3,
            vec![
                super::ExecutionInputReference {
                    id: "taskContract".to_owned(),
                    revision: Some(4),
                    digest: Some("digest-c".to_owned()),
                },
                super::ExecutionInputReference {
                    id: "guidance".to_owned(),
                    revision: None,
                    digest: Some("digest-g".to_owned()),
                },
            ],
        );
        assert_eq!(
            base.inputs
                .iter()
                .map(|input| input.id.as_str())
                .collect::<Vec<_>>(),
            vec!["guidance", "taskContract"]
        );
        let result = create_execution_input_manifest(
            "task-1",
            4,
            vec![
                super::ExecutionInputReference {
                    id: "taskContract".to_owned(),
                    revision: Some(5),
                    digest: Some("digest-c".to_owned()),
                },
                super::ExecutionInputReference {
                    id: "capability".to_owned(),
                    revision: None,
                    digest: Some("digest-cap".to_owned()),
                },
            ],
        );
        let delta = compute_execution_input_delta(&base, &result);
        assert!(!delta.unchanged_content);
        assert!(delta.base_digest != delta.result_digest);
        let removal = delta
            .changed
            .iter()
            .find(|change| change.id == "guidance")
            .expect("removal recorded");
        assert_eq!(removal.before.as_deref(), Some("digest-g"));
        assert_eq!(removal.after, None);
        // Digest equal but revision bumped counts as a change.
        let bump = delta
            .changed
            .iter()
            .find(|change| change.id == "taskContract")
            .expect("revision bump recorded");
        assert_eq!(bump.before.as_deref(), Some("digest-c"));
        assert_eq!(bump.after.as_deref(), Some("digest-c"));
        assert!(delta.unchanged.is_empty());
    }

    #[test]
    fn validation_identity_binds_the_result_payload() {
        let identity = create_validation_result_identity(
            "validation-1",
            Some("plan-7"),
            &object(&[("passed", "true")]),
            &["evidence-1".to_owned()],
        );
        assert_eq!(identity.plan_identity.as_deref(), Some("plan-7"));
        assert_eq!(identity.result_digest.len(), 64);
        assert_eq!(identity.evidence_refs, vec!["evidence-1"]);
    }

    #[test]
    fn validation_delta_classifies_transitions() {
        let base = [
            ValidationObservation { id: "was-passing", passed: true },
            ValidationObservation { id: "still-failing", passed: false },
            ValidationObservation { id: "now-failing", passed: true },
        ];
        let result = [
            ValidationObservation { id: "was-passing", passed: false },
            ValidationObservation { id: "still-failing", passed: false },
            ValidationObservation { id: "now-failing", passed: false },
            ValidationObservation { id: "fixed", passed: true },
            ValidationObservation { id: "regressed", passed: false },
        ];
        let delta = compute_validation_delta(
            &base,
            &result,
            Some("identity-base"),
            Some("identity-result"),
        );
        assert_eq!(delta.base_identity.as_deref(), Some("identity-base"));
        assert_eq!(delta.result_identity, "identity-result");
        assert_eq!(delta.newly_passing, vec!["fixed"]);
        assert_eq!(delta.still_failing, vec!["still-failing"]);
        // BTreeMap iteration is id-sorted: was-passing flipped to failing.
        assert_eq!(
            delta.new_failures,
            vec!["now-failing", "regressed", "was-passing"]
        );
        // was-passing flipped to failing, so it lands in new_failures;
        // only still-failing kept its state.
        assert_eq!(delta.unchanged_ids, vec!["still-failing"]);
    }

    #[test]
    fn review_input_manifest_binds_every_field_into_the_digest() {
        let input = super::CreateReviewInputManifest {
            review_id: "review-1".to_owned(),
            task_id: "task-1".to_owned(),
            task_contract_digest: "c-digest".to_owned(),
            changeset_digest: "cs-digest".to_owned(),
            review_context_digest: None,
            acceptance_digest: "a-digest".to_owned(),
            validation_evidence_digest: Some("v-digest".to_owned()),
            source_revisions: vec![
                super::ReviewSourceRevision {
                    path: "res://b.gd".to_owned(),
                    revision: "rev_b".to_owned(),
                },
                super::ReviewSourceRevision {
                    path: "res://a.gd".to_owned(),
                    revision: "rev_a".to_owned(),
                },
            ],
        };
        let manifest = create_review_input_manifest(input);
        assert_eq!(
            manifest
                .source_revisions
                .iter()
                .map(|r| r.path.as_str())
                .collect::<Vec<_>>(),
            vec!["res://a.gd", "res://b.gd"]
        );
        assert_eq!(manifest.digest.len(), 64);
        let flipped = super::CreateReviewInputManifest {
            changeset_digest: "OTHER".to_owned(),
            ..super::CreateReviewInputManifest {
                review_id: "review-1".to_owned(),
                task_id: "task-1".to_owned(),
                task_contract_digest: "c-digest".to_owned(),
                changeset_digest: "cs-digest".to_owned(),
                review_context_digest: None,
                acceptance_digest: "a-digest".to_owned(),
                validation_evidence_digest: Some("v-digest".to_owned()),
                source_revisions: Vec::new(),
            }
        };
        let flipped = create_review_input_manifest(flipped);
        assert_ne!(manifest.digest, flipped.digest);
    }
}
