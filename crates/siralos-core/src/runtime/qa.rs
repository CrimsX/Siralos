//! Bounded QA workflow contracts (Stage 4.6, decision 44).
//!
//! The sixth arrow of the frozen Stage-4 sequence (decision 08, step 6):
//! deterministic, ordered sequences of typed QA steps composed over the
//! generic runtime boundary, routed through the same generic host decision
//! order as [`execution`](super::execution), [`visual`](super::visual),
//! and [`interaction`](super::interaction) — validation, capability,
//! staleness, budget, cancellation, primitive — over the closed 13-kind
//! failure taxonomy and the 6-disposition table, neither of which is
//! extended.
//!
//! The QA-workflow execution primitive is identity-bound and absent on
//! this platform, so every otherwise-valid workflow reports typed
//! `UNAVAILABLE` with the exact reason
//! `"identity-bound QA workflow execution primitive not available"`. A
//! workflow with zero steps is a typed pairing refusal (mirroring the
//! one-shot-interaction refusal in [`interaction`](super::interaction)).
//! No process is launched, no live process I/O occurs, no filesystem is
//! touched, and no wall clock is read: all step content is injected
//! request data, and evidence binds digests and counts only — never raw
//! step payloads.

use std::collections::BTreeMap;

use crate::identity::sha256_hex;
use crate::identity::{CanonicalValue, compute_artifact_digest};
use crate::provider::{CancellationSignal, CancellationToken};
use crate::runtime::budget::RuntimeBudget;
use crate::runtime::execution::{
    MAX_OPERATION_ID_BYTES, MAX_RUN_ID_BYTES, RuntimeExecutionOutcome,
};
use crate::runtime::identity::create_operation_id;
use crate::tool::capability::CapabilityId;
use crate::tool::permission::{
    PermissionDecision, PermissionPolicy, evaluate_permission,
};

use super::{RuntimeError, runtime_error};

/// Capability required for any QA workflow execution.
pub const QA_WORKFLOW_CAPABILITY: &str = "qa.workflow";

/// The exact typed unavailability reason when the identity-bound
/// QA-workflow execution primitive is absent.
pub const QA_WORKFLOW_UNAVAILABLE_REASON: &str =
    "identity-bound QA workflow execution primitive not available";

/// Maximum number of ordered steps in one QA workflow.
pub const MAX_QA_WORKFLOW_STEPS: usize = 32;

/// Maximum bytes per step spec descriptor.
pub const MAX_STEP_BYTES: usize = 4 * 1024;

/// Whether the identity-bound QA-workflow execution primitive is
/// available on this platform.
///
/// Stage 4.6 is fail-closed (decision 44 C3): the primitive is absent, so
/// workflow execution always reports typed `UNAVAILABLE` without
/// mutation, spawn, or live process I/O. No filesystem probe, no ambient
/// check, no wall clock is consulted.
#[must_use]
pub const fn is_identity_bound_qa_workflow_primitive_available() -> bool {
    false
}

/// One ordered typed QA step declared by the caller. `index` must equal
/// the step's zero-based position; step content is injected data that
/// only ever leaves this module as a digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QaWorkflowStep {
    /// Zero-based step position.
    pub index: usize,
    /// Bounded step spec descriptor (never surfaced in evidence).
    pub spec: String,
}

/// Validated inputs for [`decide_qa_workflow`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QaWorkflowRequest {
    /// Owning run id (non-empty, bounded).
    pub run_id: String,
    /// Optional operation id; when absent a deterministic id is derived.
    pub operation_id: Option<String>,
    /// Whether the observed revision is stale.
    pub is_stale: bool,
    /// Ordered steps (bounded count, bounded per-step bytes).
    pub steps: Vec<QaWorkflowStep>,
}

impl QaWorkflowRequest {
    /// Total declared workflow bytes; the budget compares this against
    /// [`RuntimeBudget::artifact_bytes`].
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.steps.iter().map(|step| step.spec.len() as u64).sum()
    }
}

/// Validate a QA workflow request. A zero-step workflow is a pairing
/// refusal (the adapter-side pairing discipline, generic here): a
/// workflow without steps is a malformed request, never a silent
/// success.
fn validate_qa_workflow_request(
    request: &QaWorkflowRequest,
) -> Result<(), RuntimeError> {
    if request.run_id.is_empty() {
        return Err(runtime_error("A QA workflow requires a run id."));
    }
    if request.run_id.len() > MAX_RUN_ID_BYTES {
        return Err(runtime_error(format!(
            "The QA workflow run id exceeds the {MAX_RUN_ID_BYTES}-byte bound."
        )));
    }
    if let Some(operation_id) = &request.operation_id {
        if operation_id.is_empty() {
            return Err(runtime_error(
                "A QA workflow operation id must not be empty.",
            ));
        }
        if operation_id.len() > MAX_OPERATION_ID_BYTES {
            return Err(runtime_error(format!(
                "The QA workflow operation id exceeds the {MAX_OPERATION_ID_BYTES}-byte bound."
            )));
        }
    }
    if request.steps.is_empty() {
        return Err(runtime_error(
            "A QA workflow requires at least one step; empty workflows are a pairing refusal.",
        ));
    }
    if request.steps.len() > MAX_QA_WORKFLOW_STEPS {
        return Err(runtime_error(format!(
            "The QA workflow exceeds the {MAX_QA_WORKFLOW_STEPS}-step bound."
        )));
    }
    for (position, step) in request.steps.iter().enumerate() {
        if step.index != position {
            return Err(runtime_error(
                "A QA workflow step index must equal its zero-based position.",
            ));
        }
        if step.spec.is_empty() {
            return Err(runtime_error(
                "A QA workflow step must not be empty.",
            ));
        }
        if step.spec.len() > MAX_STEP_BYTES {
            return Err(runtime_error(format!(
                "A QA workflow step exceeds the {MAX_STEP_BYTES}-byte bound."
            )));
        }
        if step.spec.contains('\0') {
            return Err(runtime_error(
                "A QA workflow step must not contain NUL.",
            ));
        }
    }
    Ok(())
}

/// Host-authorized QA workflow decision table.
///
/// Pure and deterministic: equivalent inputs produce the same
/// [`RuntimeExecutionOutcome`]. The gate order is the generic table's own
/// order with the QA workflow capability and primitive; no I/O, no spawn,
/// no live process execution, no wall-clock read.
///
/// # Errors
///
/// Returns [`RuntimeError`] only for malformed inputs; all capability,
/// staleness, budget, cancellation, and primitive-absent cases are typed
/// [`RuntimeExecutionOutcome`] variants, never errors.
pub fn decide_qa_workflow(
    request: &QaWorkflowRequest,
    policy: &PermissionPolicy,
    budget: &RuntimeBudget,
    cancellation: CancellationSignal<'_>,
) -> Result<RuntimeExecutionOutcome, RuntimeError> {
    validate_qa_workflow_request(request)?;

    // 2. Capability gate: qa.workflow must be allow.
    let capability = CapabilityId::parse(QA_WORKFLOW_CAPABILITY)
        .expect("qa.workflow is a valid capability id");
    match evaluate_permission(&capability, policy) {
        PermissionDecision::Allow => {}
        PermissionDecision::Ask { reason }
        | PermissionDecision::Deny { reason } => {
            return Ok(RuntimeExecutionOutcome::Denied {
                reason: format!("COMMAND_DENIED: {reason}"),
            });
        }
    }

    // 3. Staleness gate.
    if request.is_stale {
        return Ok(RuntimeExecutionOutcome::Stale {
            reason: "STALE: revision is stale.".to_owned(),
        });
    }

    // 4. Budget gate: declared workflow bytes exceed the host budget.
    let total_bytes = request.total_bytes();
    if total_bytes > budget.artifact_bytes {
        return Ok(RuntimeExecutionOutcome::ResourceExceeded {
            reason: format!(
                "RESOURCE_EXCEEDED: requested {total_bytes} exceeds budget {}.",
                budget.artifact_bytes
            ),
        });
    }

    // 5. Cancellation gate.
    if cancellation.is_cancelled() {
        return Ok(RuntimeExecutionOutcome::Cancelled {
            reason: "CANCELLED: QA workflow was cancelled.".to_owned(),
        });
    }

    // 6. Identity-bound primitive gate.
    if !is_identity_bound_qa_workflow_primitive_available() {
        return Ok(RuntimeExecutionOutcome::Unavailable {
            reason: format!("UNAVAILABLE: {QA_WORKFLOW_UNAVAILABLE_REASON}"),
        });
    }

    // 7. Success (only observable when the primitive is available).
    let operation_id = request.operation_id.clone().unwrap_or_else(|| {
        create_operation_id(&request.run_id, QA_WORKFLOW_CAPABILITY)
    });
    Ok(RuntimeExecutionOutcome::Success {
        run_id: request.run_id.clone(),
        operation_id,
    })
}

/// Convenience wrapper that accepts a simple cancellation flag. Useful for
/// unit tests and for callers that already materialized a boolean.
pub fn decide_qa_workflow_with_flag(
    request: &QaWorkflowRequest,
    policy: &PermissionPolicy,
    budget: &RuntimeBudget,
    is_cancelled: bool,
) -> Result<RuntimeExecutionOutcome, RuntimeError> {
    let token = CancellationToken::new();
    if is_cancelled {
        token.cancel();
    }
    decide_qa_workflow(request, policy, budget, token.signal())
}

/// QA-workflow-structured detail bound to a workflow outcome: step count,
/// per-step digests, and total declared bytes — never raw step payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QaWorkflowEvidenceDetail {
    /// Number of declared steps.
    pub step_count: usize,
    /// Deterministic digest per step, in step order.
    pub step_digests: Vec<String>,
    /// Total declared workflow bytes.
    pub total_bytes: u64,
}

/// A workflow outcome bound to its QA-workflow-structured detail under a
/// domain-separated digest over `QaWorkflowEvidence v1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QaWorkflowEvidence {
    /// The typed decision-table outcome (dispositions unchanged).
    pub outcome: RuntimeExecutionOutcome,
    /// QA-workflow-shaped detail (digests and counts only).
    pub detail: QaWorkflowEvidenceDetail,
    /// Domain-separated digest over the outcome disposition and detail.
    pub workflow_digest: String,
}

/// Build the QA workflow evidence record for a decided outcome: binds the
/// outcome to per-step digests and the workflow detail under a
/// domain-separated digest. The request is re-validated so malformed
/// inputs can never produce evidence.
///
/// # Errors
///
/// Returns [`RuntimeError`] for malformed requests and for
/// outcome/detail mismatches.
pub fn create_qa_workflow_evidence(
    outcome: &RuntimeExecutionOutcome,
    request: &QaWorkflowRequest,
) -> Result<QaWorkflowEvidence, RuntimeError> {
    validate_qa_workflow_request(request)?;
    let mut step_digests = Vec::with_capacity(request.steps.len());
    for step in &request.steps {
        let payload = CanonicalValue::Object(BTreeMap::from([
            ("index".to_owned(), CanonicalValue::U64(step.index as u64)),
            ("len".to_owned(), CanonicalValue::U64(step.spec.len() as u64)),
            (
                "sha256".to_owned(),
                CanonicalValue::Str(sha256_hex(step.spec.as_bytes())),
            ),
        ]));
        let digest = compute_artifact_digest("QaWorkflowStep", 1, &payload)
            .map_err(|error| RuntimeError { message: error.message })?
            .value;
        step_digests.push(digest);
    }
    let total_bytes = request.total_bytes();
    let detail = QaWorkflowEvidenceDetail {
        step_count: request.steps.len(),
        step_digests,
        total_bytes,
    };
    let evidence_payload = CanonicalValue::Object(BTreeMap::from([
        (
            "disposition".to_owned(),
            CanonicalValue::Str(outcome.disposition().as_str().to_owned()),
        ),
        (
            "reason".to_owned(),
            match outcome.reason() {
                Some(reason) => CanonicalValue::Str(reason.to_owned()),
                None => CanonicalValue::Null,
            },
        ),
        (
            "detail".to_owned(),
            CanonicalValue::Object(BTreeMap::from([
                (
                    "stepCount".to_owned(),
                    CanonicalValue::U64(detail.step_count as u64),
                ),
                (
                    "stepDigests".to_owned(),
                    CanonicalValue::Array(
                        detail
                            .step_digests
                            .iter()
                            .map(|digest| CanonicalValue::Str(digest.clone()))
                            .collect(),
                    ),
                ),
                (
                    "totalBytes".to_owned(),
                    CanonicalValue::U64(detail.total_bytes),
                ),
            ])),
        ),
    ]));
    let workflow_digest =
        compute_artifact_digest("QaWorkflowEvidence", 1, &evidence_payload)
            .map_err(|error| RuntimeError { message: error.message })?
            .value;
    Ok(QaWorkflowEvidence {
        outcome: outcome.clone(),
        detail,
        workflow_digest,
    })
}

/// Bounded deterministic rendering: disposition, reason, and the workflow
/// detail (lengths and digests only, never step payloads).
#[must_use]
pub fn render_qa_workflow_evidence(evidence: &QaWorkflowEvidence) -> String {
    let reason = evidence.outcome.reason().unwrap_or("-");
    format!(
        "{} steps={} bytes={} digests={}",
        reason,
        evidence.detail.step_count,
        evidence.detail.total_bytes,
        evidence.detail.step_digests.len(),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_QA_WORKFLOW_STEPS, MAX_STEP_BYTES, QA_WORKFLOW_CAPABILITY,
        QA_WORKFLOW_UNAVAILABLE_REASON, QaWorkflowRequest,
        create_qa_workflow_evidence, decide_qa_workflow_with_flag,
        is_identity_bound_qa_workflow_primitive_available,
        render_qa_workflow_evidence,
    };
    use crate::runtime::budget::{RuntimeBudgetInput, create_runtime_budget};
    use crate::runtime::execution::RuntimeExecutionOutcome;
    use crate::tool::capability::CapabilityId;
    use crate::tool::permission::{
        PermissionPolicy, PermissionRule, PolicyRule,
    };

    fn workflow_request() -> QaWorkflowRequest {
        QaWorkflowRequest {
            run_id: "run_qa_abc".to_owned(),
            operation_id: Some("op_q1".to_owned()),
            is_stale: false,
            steps: vec![super::QaWorkflowStep {
                index: 0,
                spec: "lint".to_owned(),
            }],
        }
    }

    fn allow_policy() -> PermissionPolicy {
        PermissionPolicy::from_rules(vec![PolicyRule {
            capability: CapabilityId::parse(QA_WORKFLOW_CAPABILITY)
                .expect("valid capability"),
            rule: PermissionRule::Allow,
        }])
    }

    fn budget_with(
        artifact_bytes: u64,
    ) -> crate::runtime::budget::RuntimeBudget {
        create_runtime_budget(&RuntimeBudgetInput {
            artifact_bytes: Some(artifact_bytes),
            ..RuntimeBudgetInput::default()
        })
    }

    #[test]
    fn primitive_is_absent_and_unavailable_is_typed() {
        assert!(!is_identity_bound_qa_workflow_primitive_available());
        let outcome = decide_qa_workflow_with_flag(
            &workflow_request(),
            &allow_policy(),
            &budget_with(1024),
            false,
        )
        .expect("decision");
        assert!(outcome.is_unavailable());
        assert!(outcome.reason().is_some_and(|reason| {
            reason.contains(QA_WORKFLOW_UNAVAILABLE_REASON)
        }));
        let evidence =
            create_qa_workflow_evidence(&outcome, &workflow_request())
                .expect("evidence");
        assert_eq!(evidence.detail.step_count, 1);
        assert_eq!(evidence.detail.total_bytes, 4);
        assert_eq!(evidence.detail.step_digests.len(), 1);
        assert!(render_qa_workflow_evidence(&evidence).contains("steps=1"));
    }

    #[test]
    fn zero_step_workflows_are_a_typed_pairing_refusal() {
        let mut request = workflow_request();
        request.steps.clear();
        let error = decide_qa_workflow_with_flag(
            &request,
            &allow_policy(),
            &budget_with(1024),
            false,
        )
        .expect_err("zero-step workflow refused");
        assert!(error.message.contains("at least one step"));
    }

    #[test]
    fn budget_exceeds_are_typed_resource_exceeded() {
        let mut request = workflow_request();
        request
            .steps
            .push(super::QaWorkflowStep { index: 1, spec: "a".repeat(64) });
        let outcome = decide_qa_workflow_with_flag(
            &request,
            &allow_policy(),
            &budget_with(16),
            false,
        )
        .expect("decision");
        assert!(matches!(
            outcome,
            RuntimeExecutionOutcome::ResourceExceeded { .. }
        ));
    }

    #[test]
    fn cancellation_is_typed_cancelled() {
        let outcome = decide_qa_workflow_with_flag(
            &workflow_request(),
            &allow_policy(),
            &budget_with(1024),
            true,
        )
        .expect("decision");
        assert!(matches!(outcome, RuntimeExecutionOutcome::Cancelled { .. }));
    }

    #[test]
    fn step_bounds_are_enforced() {
        let mut request = workflow_request();
        request.steps = (0..MAX_QA_WORKFLOW_STEPS + 1)
            .map(|index| super::QaWorkflowStep { index, spec: "x".to_owned() })
            .collect();
        let error = decide_qa_workflow_with_flag(
            &request,
            &allow_policy(),
            &budget_with(1 << 30),
            false,
        )
        .expect_err("step count refused");
        assert!(error.message.contains("step bound"));
        let mut request = workflow_request();
        request.steps[0].spec = "a".repeat(MAX_STEP_BYTES + 1);
        let error = decide_qa_workflow_with_flag(
            &request,
            &allow_policy(),
            &budget_with(1 << 30),
            false,
        )
        .expect_err("step size refused");
        assert!(error.message.contains("byte bound"));
    }
}
