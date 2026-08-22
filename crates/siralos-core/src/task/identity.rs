//! TaskContract / TaskPlan content identity (Stage 3 — Content Identity
//! & Delta Verification, ADR 0028; R10a `content-identity.contract-digest`).
//!
//! Mirrors `packages/core/src/identity/contract-plan-identity.ts`:
//! revision = chronological identity; digest = exact content identity.
//! The digest is computed over CONTENT ONLY (revision excluded), so
//! content-identical revisions share a digest and any material change
//! produces a new digest. Deltas are derived evidence between two exact
//! identities; the authoritative state remains the full current artifact.
//!
//! Plan identity operates at the canonical-payload level: the Rust task
//! kernel deliberately omits a typed plan model until host-routed
//! planning parity lands, so the ten declared plan sections are compared
//! as canonical values over the same section vocabulary.

use crate::identity::{
    ArtifactDigest, ArtifactIdentityError, CanonicalValue, SectionDelta,
    compute_artifact_digest, compute_section_delta,
};
use crate::task::TaskContract;
use std::collections::BTreeMap;

/// Schema version of the contract canonical payload.
pub const TASK_CONTRACT_IDENTITY_SCHEMA: u64 = 1;

/// Schema version of the plan canonical payload.
pub const TASK_PLAN_IDENTITY_SCHEMA: u64 = 1;

/// Declared contract sections in oracle order.
pub const CONTRACT_SECTION_KEYS: [&str; 5] =
    ["request", "context", "constraints", "acceptanceCriteria", "pausePolicy"];

/// Declared plan sections in oracle order.
pub const PLAN_SECTION_KEYS: [&str; 10] = [
    "objective",
    "scope",
    "nonGoals",
    "touchpoints",
    "constraints",
    "risks",
    "steps",
    "validation",
    "rollback",
    "rationale",
];

fn payload_object(
    payload: &CanonicalValue,
) -> BTreeMap<String, CanonicalValue> {
    match payload {
        CanonicalValue::Object(map) => map.clone(),
        _ => BTreeMap::new(),
    }
}

/// Typed content digest of a contract revision (algorithm, domain
/// separator, schema version, value). Revision is excluded from the
/// payload.
pub fn compute_task_contract_artifact_digest(
    contract: &TaskContract,
) -> Result<ArtifactDigest, ArtifactIdentityError> {
    compute_artifact_digest(
        "TaskContract",
        TASK_CONTRACT_IDENTITY_SCHEMA,
        &contract.content_payload(),
    )
}

/// Hex content digest of a contract revision.
pub fn compute_task_contract_content_digest(
    contract: &TaskContract,
) -> Result<String, ArtifactIdentityError> {
    Ok(compute_task_contract_artifact_digest(contract)?.value)
}

/// Derived semantic delta between two contract revisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskContractDelta {
    /// Lifecycle revision of the base contract.
    pub base_revision: u64,
    /// Lifecycle revision of the result contract.
    pub result_revision: u64,
    /// Exact content digest of the base revision.
    pub base_digest: String,
    /// Exact content digest of the result revision.
    pub result_digest: String,
    /// Sections whose canonical form differs (declared order).
    pub changed: Vec<String>,
    /// Sections whose canonical form matches (declared order).
    pub unchanged: Vec<String>,
    /// True when no material content change was detected.
    pub unchanged_content: bool,
}

/// Derive the semantic delta between two contract revisions over the
/// five declared sections.
pub fn compute_task_contract_delta(
    base: &TaskContract,
    result: &TaskContract,
) -> Result<TaskContractDelta, ArtifactIdentityError> {
    let base_sections = payload_object(&base.content_payload());
    let result_sections = payload_object(&result.content_payload());
    let SectionDelta { changed, unchanged } = compute_section_delta(
        &base_sections,
        &result_sections,
        &CONTRACT_SECTION_KEYS,
    );
    let unchanged_content = changed.is_empty();
    Ok(TaskContractDelta {
        base_revision: base.revision(),
        result_revision: result.revision(),
        base_digest: compute_task_contract_content_digest(base)?,
        result_digest: compute_task_contract_content_digest(result)?,
        unchanged_content,
        changed,
        unchanged,
    })
}

/// Typed content digest over a declared plan canonical payload. The
/// payload must be an object whose keys are the ten declared plan
/// sections; callers building it from a typed model keep this function
/// as the single identity seam.
pub fn compute_plan_content_digest(
    payload: &CanonicalValue,
) -> Result<ArtifactDigest, ArtifactIdentityError> {
    compute_artifact_digest("TaskPlan", TASK_PLAN_IDENTITY_SCHEMA, payload)
}

/// Hex content digest over a declared plan canonical payload.
pub fn compute_plan_content_digest_hex(
    payload: &CanonicalValue,
) -> Result<String, ArtifactIdentityError> {
    Ok(compute_plan_content_digest(payload)?.value)
}

/// Derived semantic delta between two plan payloads over the ten
/// declared sections. The authoritative state remains each full plan
/// payload; deltas only describe what materially changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskPlanSectionsDelta {
    /// Base payload's exact content digest.
    pub base_digest: String,
    /// Result payload's exact content digest.
    pub result_digest: String,
    /// Sections whose canonical form differs (declared order).
    pub changed: Vec<String>,
    /// Sections whose canonical form matches (declared order).
    pub unchanged: Vec<String>,
    /// True when no material content change was detected.
    pub unchanged_content: bool,
}

/// Derive the section-level delta between two plan payloads.
pub fn compute_task_plan_sections_delta(
    base_payload: &CanonicalValue,
    result_payload: &CanonicalValue,
) -> Result<TaskPlanSectionsDelta, ArtifactIdentityError> {
    let base_sections = payload_object(base_payload);
    let result_sections = payload_object(result_payload);
    let SectionDelta { changed, unchanged } = compute_section_delta(
        &base_sections,
        &result_sections,
        &PLAN_SECTION_KEYS,
    );
    let unchanged_content = changed.is_empty();
    Ok(TaskPlanSectionsDelta {
        base_digest: compute_plan_content_digest_hex(base_payload)?,
        result_digest: compute_plan_content_digest_hex(result_payload)?,
        changed,
        unchanged,
        unchanged_content,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        PLAN_SECTION_KEYS, compute_plan_content_digest,
        compute_task_contract_artifact_digest, compute_task_contract_delta,
        compute_task_plan_sections_delta,
    };
    use crate::identity::CanonicalValue;
    use crate::task::{
        AcceptanceCriterion, ConstraintKind, CreateTaskContractInput,
        PausePolicy, ReviseContext, ReviseTaskContractInput, TaskConstraint,
        TaskContract, VerificationKind,
    };
    use std::collections::BTreeMap;

    fn contract(request: &str) -> TaskContract {
        TaskContract::create(CreateTaskContractInput {
            id: "task-1".to_owned(),
            request: request.to_owned(),
            context: None,
            constraints: Some(vec![TaskConstraint::new(
                "c1".to_owned(),
                "stay bounded".to_owned(),
                ConstraintKind::Scope,
            )]),
            acceptance_criteria: vec![AcceptanceCriterion::new(
                "ac1".to_owned(),
                "it works".to_owned(),
                VerificationKind::Deterministic,
            )],
            pause_policy: Some(PausePolicy::OnApproval),
        })
        .expect("valid contract")
    }

    fn revised(
        base: &TaskContract,
        request: &str,
        context: Option<&str>,
    ) -> TaskContract {
        base.revise(ReviseTaskContractInput {
            id: base.id().to_owned(),
            request: Some(request.to_owned()),
            context: Some(match context {
                Some(value) => ReviseContext::Set(value.to_owned()),
                None => ReviseContext::Keep,
            }),
            constraints: None,
            acceptance_criteria: None,
            pause_policy: None,
        })
        .expect("revision valid")
    }

    #[test]
    fn digest_excludes_the_revision_lifecycle_field() {
        let base = contract("do the thing");
        let bumped = revised(&base, "do the thing", None);
        assert_ne!(base.revision(), bumped.revision());
        assert_eq!(base.digest(), bumped.digest());
        let delta =
            compute_task_contract_delta(&base, &bumped).expect("delta");
        assert!(delta.unchanged_content);
        assert!(delta.changed.is_empty());
        assert_eq!(delta.base_revision, 1);
        assert_eq!(delta.result_revision, 2);
        assert_eq!(delta.base_digest, delta.result_digest);
    }

    #[test]
    fn request_changes_report_in_declared_order() {
        let base = contract("do the thing");
        let result = revised(&base, "do the other thing", None);
        let delta =
            compute_task_contract_delta(&base, &result).expect("delta");
        assert_eq!(delta.changed, vec!["request"]);
        assert_eq!(
            delta.unchanged,
            vec![
                "context",
                "constraints",
                "acceptanceCriteria",
                "pausePolicy"
            ]
        );
        assert!(!delta.unchanged_content);
        assert_ne!(delta.base_digest, delta.result_digest);
    }

    #[test]
    fn context_presence_flips_only_the_context_section() {
        let base = contract("do the thing");
        let with_context = revised(&base, "do the thing", Some("extra"));
        let delta =
            compute_task_contract_delta(&base, &with_context).expect("delta");
        assert_eq!(delta.changed, vec!["context"]);
        assert!(!delta.unchanged_content);
    }

    #[test]
    fn typed_artifact_digest_view_binds_the_kernel_digest() {
        let contract = contract("do the thing");
        let digest =
            compute_task_contract_artifact_digest(&contract).expect("digest");
        assert_eq!(digest.algorithm, "sha256");
        assert_eq!(digest.artifact_type, "TaskContract");
        assert_eq!(digest.schema_version, 1);
        assert_eq!(digest.value, contract.digest());
    }

    fn plan_payload(objective: &str, risks: &[&str]) -> CanonicalValue {
        let mut sections: BTreeMap<String, CanonicalValue> = BTreeMap::new();
        sections.insert(
            "objective".to_owned(),
            CanonicalValue::Str(objective.to_owned()),
        );
        sections.insert("scope".to_owned(), CanonicalValue::Null);
        sections
            .insert("nonGoals".to_owned(), CanonicalValue::Array(Vec::new()));
        sections.insert(
            "touchpoints".to_owned(),
            CanonicalValue::Array(Vec::new()),
        );
        sections.insert(
            "constraints".to_owned(),
            CanonicalValue::Array(vec![CanonicalValue::Str(
                "bounded".to_owned(),
            )]),
        );
        sections.insert(
            "risks".to_owned(),
            CanonicalValue::Array(
                risks
                    .iter()
                    .map(|risk| CanonicalValue::Str((*risk).to_owned()))
                    .collect(),
            ),
        );
        sections.insert("steps".to_owned(), CanonicalValue::Array(Vec::new()));
        sections.insert("validation".to_owned(), CanonicalValue::Null);
        sections.insert("rollback".to_owned(), CanonicalValue::Null);
        sections.insert("rationale".to_owned(), CanonicalValue::Null);
        CanonicalValue::Object(sections)
    }

    #[test]
    fn plan_payload_digest_is_content_sensitive() {
        let base = plan_payload("ship it", &["regressions"]);
        let changed = plan_payload("ship it faster", &["regressions"]);
        assert_ne!(
            compute_plan_content_digest(&base).expect("digest").value,
            compute_plan_content_digest(&changed).expect("digest").value
        );
    }

    #[test]
    fn plan_section_delta_reports_declared_order_and_unchanged_flag() {
        let base = plan_payload("ship it", &["regressions"]);
        let result = plan_payload("ship it", &["regressions", "flakiness"]);
        let delta =
            compute_task_plan_sections_delta(&base, &result).expect("delta");
        assert_ne!(delta.base_digest, delta.result_digest);
        assert_eq!(delta.changed, vec!["risks"]);
        let expected_unchanged: Vec<String> = PLAN_SECTION_KEYS
            .iter()
            .filter(|key| **key != "risks")
            .map(|key| (*key).to_owned())
            .collect();
        assert_eq!(delta.unchanged, expected_unchanged);
        assert!(!delta.unchanged_content);

        let identical =
            compute_task_plan_sections_delta(&base, &base).expect("delta");
        assert!(identical.unchanged_content);
        assert!(identical.changed.is_empty());
        assert_eq!(identical.unchanged.len(), PLAN_SECTION_KEYS.len());
    }
}
