//! Bounded run-profiling sessions (Stage 4.7, decision 45).
//!
//! The seventh and final arrow of the frozen Stage-4 sequence (decision 08,
//! step 7): typed sampler sessions attached to a supervised run, routed
//! through the same generic host decision order as
//! [`execution`](super::execution), [`visual`](super::visual),
//! [`interaction`](super::interaction), and [`qa`](super::qa) —
//! validation, capability, staleness, budget, cancellation, primitive —
//! over the closed 13-kind failure taxonomy and the 6-disposition table,
//! neither of which is extended.
//!
//! The profiling primitive is identity-bound and absent on this platform,
//! so every otherwise-valid session reports typed `UNAVAILABLE` with the
//! exact reason
//! `"identity-bound profiling primitive not available"`. A session with
//! zero declared samples is a typed pairing refusal (mirroring the
//! zero-step-workflow refusal in [`qa`](super::qa)). No process is
//! launched, no live process I/O occurs, no filesystem is touched, and no
//! wall clock is read: all sample content is injected request data, and
//! evidence binds digests and counts only — never raw sample payloads.

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

/// Capability required for any run-profiling session.
pub const RUN_PROFILE_CAPABILITY: &str = "run.profile";

/// The exact typed unavailability reason when the identity-bound
/// profiling primitive is absent.
pub const RUN_PROFILE_UNAVAILABLE_REASON: &str =
    "identity-bound profiling primitive not available";

/// Maximum number of declared samples in one profiling session.
pub const MAX_PROFILE_SAMPLES: usize = 64;

/// Maximum bytes per sample label.
pub const MAX_SAMPLE_BYTES: usize = 1024;

/// Whether the identity-bound profiling primitive is available on this
/// platform.
///
/// Stage 4.7 is fail-closed (decision 45 C3): the primitive is absent, so
/// profiling always reports typed `UNAVAILABLE` without mutation, spawn,
/// or live process I/O. No filesystem probe, no ambient check, no wall
/// clock is consulted.
#[must_use]
pub const fn is_identity_bound_profiling_primitive_available() -> bool {
    false
}

/// One declared profiling sample. `index` must equal the sample's
/// zero-based position; sample content is injected data that only ever
/// leaves this module as a digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileSample {
    /// Zero-based sample position.
    pub index: usize,
    /// Bounded sample label (never surfaced in evidence).
    pub label: String,
}

/// Validated inputs for [`decide_run_profile`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunProfileRequest {
    /// Owning run id (non-empty, bounded).
    pub run_id: String,
    /// Optional operation id; when absent a deterministic id is derived.
    pub operation_id: Option<String>,
    /// Whether the observed revision is stale.
    pub is_stale: bool,
    /// Declared samples (bounded count, bounded per-sample bytes).
    pub samples: Vec<ProfileSample>,
}

impl RunProfileRequest {
    /// Total declared session bytes; the budget compares this against
    /// [`RuntimeBudget::artifact_bytes`].
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.samples.iter().map(|sample| sample.label.len() as u64).sum()
    }
}

/// Validate a run-profiling request. A zero-sample session is a pairing
/// refusal (the adapter-side pairing discipline, generic here): a session
/// without samples is a malformed request, never a silent success.
fn validate_run_profile_request(
    request: &RunProfileRequest,
) -> Result<(), RuntimeError> {
    if request.run_id.is_empty() {
        return Err(runtime_error("A run profile requires a run id."));
    }
    if request.run_id.len() > MAX_RUN_ID_BYTES {
        return Err(runtime_error(format!(
            "The run profile run id exceeds the {MAX_RUN_ID_BYTES}-byte bound."
        )));
    }
    if let Some(operation_id) = &request.operation_id {
        if operation_id.is_empty() {
            return Err(runtime_error(
                "A run profile operation id must not be empty.",
            ));
        }
        if operation_id.len() > MAX_OPERATION_ID_BYTES {
            return Err(runtime_error(format!(
                "The run profile operation id exceeds the {MAX_OPERATION_ID_BYTES}-byte bound."
            )));
        }
    }
    if request.samples.is_empty() {
        return Err(runtime_error(
            "A run profile requires at least one sample; zero-sample sessions are a pairing refusal.",
        ));
    }
    if request.samples.len() > MAX_PROFILE_SAMPLES {
        return Err(runtime_error(format!(
            "The run profile exceeds the {MAX_PROFILE_SAMPLES}-sample bound."
        )));
    }
    for (position, sample) in request.samples.iter().enumerate() {
        if sample.index != position {
            return Err(runtime_error(
                "A run profile sample index must equal its zero-based position.",
            ));
        }
        if sample.label.is_empty() {
            return Err(runtime_error(
                "A run profile sample must not be empty.",
            ));
        }
        if sample.label.len() > MAX_SAMPLE_BYTES {
            return Err(runtime_error(format!(
                "A run profile sample exceeds the {MAX_SAMPLE_BYTES}-byte bound."
            )));
        }
        if sample.label.contains('\0') {
            return Err(runtime_error(
                "A run profile sample must not contain NUL.",
            ));
        }
    }
    Ok(())
}

/// Host-authorized run-profiling decision table.
///
/// Pure and deterministic: equivalent inputs produce the same
/// [`RuntimeExecutionOutcome`]. The gate order is the generic table's own
/// order with the profiling capability and primitive; no I/O, no spawn,
/// no live process sampling, no wall-clock read.
///
/// # Errors
///
/// Returns [`RuntimeError`] only for malformed inputs; all capability,
/// staleness, budget, cancellation, and primitive-absent cases are typed
/// [`RuntimeExecutionOutcome`] variants, never errors.
pub fn decide_run_profile(
    request: &RunProfileRequest,
    policy: &PermissionPolicy,
    budget: &RuntimeBudget,
    cancellation: CancellationSignal<'_>,
) -> Result<RuntimeExecutionOutcome, RuntimeError> {
    validate_run_profile_request(request)?;

    // 2. Capability gate: run.profile must be allow.
    let capability = CapabilityId::parse(RUN_PROFILE_CAPABILITY)
        .expect("run.profile is a valid capability id");
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

    // 4. Budget gate: declared session bytes exceed the host budget.
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
            reason: "CANCELLED: run profile was cancelled.".to_owned(),
        });
    }

    // 6. Identity-bound primitive gate.
    if !is_identity_bound_profiling_primitive_available() {
        return Ok(RuntimeExecutionOutcome::Unavailable {
            reason: format!("UNAVAILABLE: {RUN_PROFILE_UNAVAILABLE_REASON}"),
        });
    }

    // 7. Success (only observable when the primitive is available).
    let operation_id = request.operation_id.clone().unwrap_or_else(|| {
        create_operation_id(&request.run_id, RUN_PROFILE_CAPABILITY)
    });
    Ok(RuntimeExecutionOutcome::Success {
        run_id: request.run_id.clone(),
        operation_id,
    })
}

/// Convenience wrapper that accepts a simple cancellation flag. Useful for
/// unit tests and for callers that already materialized a boolean.
pub fn decide_run_profile_with_flag(
    request: &RunProfileRequest,
    policy: &PermissionPolicy,
    budget: &RuntimeBudget,
    is_cancelled: bool,
) -> Result<RuntimeExecutionOutcome, RuntimeError> {
    let token = CancellationToken::new();
    if is_cancelled {
        token.cancel();
    }
    decide_run_profile(request, policy, budget, token.signal())
}

/// Profiling-structured detail bound to a profiling outcome: sample
/// count, per-sample digests, and total declared bytes — never raw
/// sample payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunProfileEvidenceDetail {
    /// Number of declared samples.
    pub sample_count: usize,
    /// Deterministic digest per sample, in sample order.
    pub sample_digests: Vec<String>,
    /// Total declared session bytes.
    pub total_bytes: u64,
}

/// A profiling outcome bound to its profiling-structured detail under a
/// domain-separated digest over `RunProfileEvidence v1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunProfileEvidence {
    /// The typed decision-table outcome (dispositions unchanged).
    pub outcome: RuntimeExecutionOutcome,
    /// Profiling-shaped detail (digests and counts only).
    pub detail: RunProfileEvidenceDetail,
    /// Domain-separated digest over the outcome disposition and detail.
    pub profile_digest: String,
}

/// Build the run-profile evidence record for a decided outcome: binds the
/// outcome to per-sample digests and the profiling detail under a
/// domain-separated digest. The request is re-validated so malformed
/// inputs can never produce evidence.
///
/// # Errors
///
/// Returns [`RuntimeError`] for malformed requests and for
/// outcome/detail mismatches.
pub fn create_run_profile_evidence(
    outcome: &RuntimeExecutionOutcome,
    request: &RunProfileRequest,
) -> Result<RunProfileEvidence, RuntimeError> {
    validate_run_profile_request(request)?;
    let mut sample_digests = Vec::with_capacity(request.samples.len());
    for sample in &request.samples {
        let payload = CanonicalValue::Object(BTreeMap::from([
            ("index".to_owned(), CanonicalValue::U64(sample.index as u64)),
            ("len".to_owned(), CanonicalValue::U64(sample.label.len() as u64)),
            (
                "sha256".to_owned(),
                CanonicalValue::Str(sha256_hex(sample.label.as_bytes())),
            ),
        ]));
        let digest = compute_artifact_digest("ProfileSample", 1, &payload)
            .map_err(|error| RuntimeError { message: error.message })?
            .value;
        sample_digests.push(digest);
    }
    let total_bytes = request.total_bytes();
    let detail = RunProfileEvidenceDetail {
        sample_count: request.samples.len(),
        sample_digests,
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
                    "sampleCount".to_owned(),
                    CanonicalValue::U64(detail.sample_count as u64),
                ),
                (
                    "sampleDigests".to_owned(),
                    CanonicalValue::Array(
                        detail
                            .sample_digests
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
    let profile_digest =
        compute_artifact_digest("RunProfileEvidence", 1, &evidence_payload)
            .map_err(|error| RuntimeError { message: error.message })?
            .value;
    Ok(RunProfileEvidence { outcome: outcome.clone(), detail, profile_digest })
}

/// Bounded deterministic rendering: disposition, reason, and the
/// profiling detail (lengths and digests only, never sample payloads).
#[must_use]
pub fn render_run_profile_evidence(evidence: &RunProfileEvidence) -> String {
    let reason = evidence.outcome.reason().unwrap_or("-");
    format!(
        "{} samples={} bytes={} digests={}",
        reason,
        evidence.detail.sample_count,
        evidence.detail.total_bytes,
        evidence.detail.sample_digests.len(),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_PROFILE_SAMPLES, MAX_SAMPLE_BYTES, RUN_PROFILE_CAPABILITY,
        RUN_PROFILE_UNAVAILABLE_REASON, RunProfileRequest,
        create_run_profile_evidence, decide_run_profile_with_flag,
        is_identity_bound_profiling_primitive_available,
        render_run_profile_evidence,
    };
    use crate::runtime::budget::{RuntimeBudgetInput, create_runtime_budget};
    use crate::runtime::execution::RuntimeExecutionOutcome;
    use crate::tool::capability::CapabilityId;
    use crate::tool::permission::{
        PermissionPolicy, PermissionRule, PolicyRule,
    };

    fn profile_request() -> RunProfileRequest {
        RunProfileRequest {
            run_id: "run_profile_abc".to_owned(),
            operation_id: Some("op_p1".to_owned()),
            is_stale: false,
            samples: vec![super::ProfileSample {
                index: 0,
                label: "frame-time".to_owned(),
            }],
        }
    }

    fn allow_policy() -> PermissionPolicy {
        PermissionPolicy::from_rules(vec![PolicyRule {
            capability: CapabilityId::parse(RUN_PROFILE_CAPABILITY)
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
        assert!(!is_identity_bound_profiling_primitive_available());
        let outcome = decide_run_profile_with_flag(
            &profile_request(),
            &allow_policy(),
            &budget_with(1024),
            false,
        )
        .expect("decision");
        assert!(outcome.is_unavailable());
        assert!(outcome.reason().is_some_and(|reason| {
            reason.contains(RUN_PROFILE_UNAVAILABLE_REASON)
        }));
        let evidence =
            create_run_profile_evidence(&outcome, &profile_request())
                .expect("evidence");
        assert_eq!(evidence.detail.sample_count, 1);
        assert_eq!(evidence.detail.total_bytes, 10);
        assert_eq!(evidence.detail.sample_digests.len(), 1);
        assert!(render_run_profile_evidence(&evidence).contains("samples=1"));
    }

    #[test]
    fn zero_sample_sessions_are_a_typed_pairing_refusal() {
        let mut request = profile_request();
        request.samples.clear();
        let error = decide_run_profile_with_flag(
            &request,
            &allow_policy(),
            &budget_with(1024),
            false,
        )
        .expect_err("zero-sample session refused");
        assert!(error.message.contains("at least one sample"));
    }

    #[test]
    fn budget_exceeds_are_typed_resource_exceeded() {
        let mut request = profile_request();
        request
            .samples
            .push(super::ProfileSample { index: 1, label: "a".repeat(64) });
        let outcome = decide_run_profile_with_flag(
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
        let outcome = decide_run_profile_with_flag(
            &profile_request(),
            &allow_policy(),
            &budget_with(1024),
            true,
        )
        .expect("decision");
        assert!(matches!(outcome, RuntimeExecutionOutcome::Cancelled { .. }));
    }

    #[test]
    fn sample_bounds_are_enforced() {
        let mut request = profile_request();
        request.samples = (0..MAX_PROFILE_SAMPLES + 1)
            .map(|index| super::ProfileSample { index, label: "x".to_owned() })
            .collect();
        let error = decide_run_profile_with_flag(
            &request,
            &allow_policy(),
            &budget_with(1 << 30),
            false,
        )
        .expect_err("sample count refused");
        assert!(error.message.contains("sample bound"));
        let mut request = profile_request();
        request.samples[0].label = "a".repeat(MAX_SAMPLE_BYTES + 1);
        let error = decide_run_profile_with_flag(
            &request,
            &allow_policy(),
            &budget_with(1 << 30),
            false,
        )
        .expect_err("sample size refused");
        assert!(error.message.contains("byte bound"));
    }
}
