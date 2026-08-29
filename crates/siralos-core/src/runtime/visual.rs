//! Visual capture evidence (Stage 4.4, decision 42).
//!
//! The second arrow of the frozen Stage-4 sequence (decision 08, step 4):
//! a bounded, digest-bound capture-evidence lifecycle for visual runs,
//! routed through the same generic host decision order as
//! [`execution`](super::execution) — validation, capability, staleness,
//! budget, cancellation, primitive — over the closed 13-kind failure
//! taxonomy and the 6-disposition table, neither of which is extended.
//!
//! Visual-mode **readiness** (injected display availability,
//! blocked/degraded/available) already lives in [`readiness`](super::readiness)
//! and is unchanged here. This module owns the **evidence** half: a capture
//! request declares bounded frames, the decision table admits or refuses
//! them, and the evidence record binds the outcome to per-frame digests,
//! the frame count, and the total captured bytes — digests and counts
//! only, never raw frame bytes.
//!
//! The capture primitive is identity-bound and absent on this platform, so
//! every otherwise-valid capture reports typed `UNAVAILABLE` with the
//! exact reason
//! `"identity-bound visual capture primitive not available"`. No process
//! is launched, no display is probed, no filesystem is touched, and no
//! wall clock is read.

use std::collections::BTreeMap;

use crate::identity::sha256_hex;
use crate::identity::{CanonicalValue, compute_artifact_digest};
use crate::provider::{CancellationSignal, CancellationToken};
use crate::runtime::budget::RuntimeBudget;
use crate::runtime::execution::{
    MAX_OPERATION_ID_BYTES, MAX_RUN_ID_BYTES, RuntimeExecutionOutcome,
};
use crate::runtime::identity::create_operation_id;
use crate::runtime::readiness::RuntimeMode;
use crate::tool::capability::CapabilityId;
use crate::tool::permission::{
    PermissionDecision, PermissionPolicy, evaluate_permission,
};

use super::{RuntimeError, runtime_error};

/// Capability required for any visual capture.
pub const VISUAL_CAPTURE_CAPABILITY: &str = "visual.capture";

/// The exact typed unavailability reason when the identity-bound visual
/// capture primitive is absent.
pub const VISUAL_CAPTURE_UNAVAILABLE_REASON: &str =
    "identity-bound visual capture primitive not available";

/// Maximum number of frames in one capture request.
pub const MAX_CAPTURE_FRAMES: usize = 64;

/// Maximum bytes per frame. Budget admission compares the request total
/// against [`RuntimeBudget::artifact_bytes`]; this bound only guards
/// single-frame allocation.
pub const MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;

/// Whether the identity-bound visual capture primitive is available on
/// this platform.
///
/// Stage 4.4 is fail-closed (decision 42 C3): the primitive is absent, so
/// capture always reports typed `UNAVAILABLE` without mutation, spawn,
/// or display probing. No filesystem probe, no ambient check, no wall
/// clock is consulted.
#[must_use]
pub const fn is_identity_bound_visual_capture_primitive_available() -> bool {
    false
}

/// One bounded frame declared by a capture request. `index` must equal
/// the frame's zero-based position; bytes are content that the caller
/// already holds and only ever leaves this module as a digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualFrame {
    /// Zero-based frame position.
    pub index: usize,
    /// Bounded frame bytes (never surfaced in evidence).
    pub bytes: Vec<u8>,
}

/// Validated inputs for [`decide_visual_capture`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualCaptureRequest {
    /// Owning run id (non-empty, bounded).
    pub run_id: String,
    /// Optional operation id; when absent a deterministic id is derived.
    pub operation_id: Option<String>,
    /// Must be [`RuntimeMode::Visual`]; headless runs never capture.
    pub mode: RuntimeMode,
    /// Whether the observed revision is stale.
    pub is_stale: bool,
    /// Declared frames (bounded count, bounded per-frame bytes).
    pub frames: Vec<VisualFrame>,
}

impl VisualCaptureRequest {
    /// Total declared capture bytes; the budget compares this against
    /// [`RuntimeBudget::artifact_bytes`].
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.frames.iter().map(|frame| frame.bytes.len() as u64).sum()
    }
}

/// Validate a visual capture request. Headless mode is a pairing refusal
/// (the adapter-side pairing discipline, generic here): a capture without
/// a visual run is a malformed request, never a silent success.
fn validate_visual_capture_request(
    request: &VisualCaptureRequest,
) -> Result<(), RuntimeError> {
    if request.run_id.is_empty() {
        return Err(runtime_error("A visual capture requires a run id."));
    }
    if request.run_id.len() > MAX_RUN_ID_BYTES {
        return Err(runtime_error(format!(
            "The visual capture run id exceeds the {MAX_RUN_ID_BYTES}-byte bound."
        )));
    }
    if let Some(operation_id) = &request.operation_id {
        if operation_id.is_empty() {
            return Err(runtime_error(
                "A visual capture operation id must not be empty.",
            ));
        }
        if operation_id.len() > MAX_OPERATION_ID_BYTES {
            return Err(runtime_error(format!(
                "The visual capture operation id exceeds the {MAX_OPERATION_ID_BYTES}-byte bound."
            )));
        }
    }
    if request.mode != RuntimeMode::Visual {
        return Err(runtime_error(
            "A visual capture requires visual mode; headless runs never capture frames.",
        ));
    }
    if request.frames.is_empty() {
        return Err(runtime_error(
            "A visual capture requires at least one frame.",
        ));
    }
    if request.frames.len() > MAX_CAPTURE_FRAMES {
        return Err(runtime_error(format!(
            "The visual capture exceeds the {MAX_CAPTURE_FRAMES}-frame bound."
        )));
    }
    for (position, frame) in request.frames.iter().enumerate() {
        if frame.index != position {
            return Err(runtime_error(
                "A visual capture frame index must equal its zero-based position.",
            ));
        }
        if frame.bytes.is_empty() {
            return Err(runtime_error(
                "A visual capture frame must not be empty.",
            ));
        }
        if frame.bytes.len() > MAX_FRAME_BYTES {
            return Err(runtime_error(format!(
                "A visual capture frame exceeds the {MAX_FRAME_BYTES}-byte bound."
            )));
        }
    }
    Ok(())
}

/// Host-authorized visual capture decision table.
///
/// Pure and deterministic: equivalent inputs produce the same
/// [`RuntimeExecutionOutcome`]. The gate order is the generic table's own
/// order with the capture capability and primitive; no I/O, no spawn, no
/// display probe, no wall-clock read.
///
/// # Errors
///
/// Returns [`RuntimeError`] only for malformed inputs; all capability,
/// staleness, budget, cancellation, and primitive-absent cases are typed
/// [`RuntimeExecutionOutcome`] variants, never errors.
pub fn decide_visual_capture(
    request: &VisualCaptureRequest,
    policy: &PermissionPolicy,
    budget: &RuntimeBudget,
    cancellation: CancellationSignal<'_>,
) -> Result<RuntimeExecutionOutcome, RuntimeError> {
    validate_visual_capture_request(request)?;

    // 2. Capability gate: visual.capture must be allow.
    let capability = CapabilityId::parse(VISUAL_CAPTURE_CAPABILITY)
        .expect("visual.capture is a valid capability id");
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

    // 4. Budget gate: declared capture bytes exceed the host budget.
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
            reason: "CANCELLED: visual capture was cancelled.".to_owned(),
        });
    }

    // 6. Identity-bound primitive gate.
    if !is_identity_bound_visual_capture_primitive_available() {
        return Ok(RuntimeExecutionOutcome::Unavailable {
            reason: format!(
                "UNAVAILABLE: {VISUAL_CAPTURE_UNAVAILABLE_REASON}"
            ),
        });
    }

    // 7. Success (only observable when the primitive is available).
    let operation_id = request.operation_id.clone().unwrap_or_else(|| {
        create_operation_id(&request.run_id, VISUAL_CAPTURE_CAPABILITY)
    });
    Ok(RuntimeExecutionOutcome::Success {
        run_id: request.run_id.clone(),
        operation_id,
    })
}

/// Convenience wrapper that accepts a simple cancellation flag. Useful for
/// unit tests and for callers that already materialized a boolean.
pub fn decide_visual_capture_with_flag(
    request: &VisualCaptureRequest,
    policy: &PermissionPolicy,
    budget: &RuntimeBudget,
    is_cancelled: bool,
) -> Result<RuntimeExecutionOutcome, RuntimeError> {
    let token = CancellationToken::new();
    if is_cancelled {
        token.cancel();
    }
    decide_visual_capture(request, policy, budget, token.signal())
}

/// Visual-structured detail bound to a capture outcome: mode, frame
/// count, per-frame digests, and total declared bytes — never raw frame
/// bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualCaptureEvidenceDetail {
    /// Always [`RuntimeMode::Visual`] for a validated request.
    pub mode: RuntimeMode,
    /// Number of declared frames.
    pub frame_count: usize,
    /// Deterministic digest per frame, in frame order.
    pub frame_digests: Vec<String>,
    /// Total declared capture bytes.
    pub total_bytes: u64,
}

/// A capture outcome bound to its visual-structured detail under a
/// domain-separated digest over `VisualCaptureEvidence v1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualCaptureEvidence {
    /// The typed decision-table outcome (dispositions unchanged).
    pub outcome: RuntimeExecutionOutcome,
    /// Visual-shaped detail (digests and counts only).
    pub detail: VisualCaptureEvidenceDetail,
    /// Domain-separated digest over the outcome disposition and detail.
    pub capture_digest: String,
}

/// Build the visual capture evidence record for a decided outcome: binds
/// the outcome to per-frame digests and the visual detail under a
/// domain-separated digest. The request is re-validated so malformed
/// inputs can never produce evidence.
///
/// # Errors
///
/// Returns [`RuntimeError`] for malformed requests and for
/// outcome/detail mismatches.
pub fn create_visual_capture_evidence(
    outcome: &RuntimeExecutionOutcome,
    request: &VisualCaptureRequest,
) -> Result<VisualCaptureEvidence, RuntimeError> {
    validate_visual_capture_request(request)?;
    let mut frame_digests = Vec::with_capacity(request.frames.len());
    for frame in &request.frames {
        let payload = CanonicalValue::Object(BTreeMap::from([
            ("index".to_owned(), CanonicalValue::U64(frame.index as u64)),
            ("len".to_owned(), CanonicalValue::U64(frame.bytes.len() as u64)),
            (
                "sha256".to_owned(),
                CanonicalValue::Str(sha256_hex(&frame.bytes)),
            ),
        ]));
        let digest = compute_artifact_digest("VisualFrame", 1, &payload)
            .map_err(|error| RuntimeError { message: error.message })?
            .value;
        frame_digests.push(digest);
    }
    let total_bytes = request.total_bytes();
    let detail = VisualCaptureEvidenceDetail {
        mode: request.mode,
        frame_count: request.frames.len(),
        frame_digests,
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
                    "mode".to_owned(),
                    CanonicalValue::Str(detail.mode.as_str().to_owned()),
                ),
                (
                    "frameCount".to_owned(),
                    CanonicalValue::U64(detail.frame_count as u64),
                ),
                (
                    "frameDigests".to_owned(),
                    CanonicalValue::Array(
                        detail
                            .frame_digests
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
    let capture_digest =
        compute_artifact_digest("VisualCaptureEvidence", 1, &evidence_payload)
            .map_err(|error| RuntimeError { message: error.message })?
            .value;
    Ok(VisualCaptureEvidence {
        outcome: outcome.clone(),
        detail,
        capture_digest,
    })
}

/// Bounded deterministic rendering: disposition, reason, and the visual
/// detail (lengths and digests only, never frame bytes).
#[must_use]
pub fn render_visual_capture_evidence(
    evidence: &VisualCaptureEvidence,
) -> String {
    let reason = evidence.outcome.reason().unwrap_or("-");
    format!(
        "{} mode={} frames={} bytes={} digests={}",
        reason,
        evidence.detail.mode.as_str(),
        evidence.detail.frame_count,
        evidence.detail.total_bytes,
        evidence.detail.frame_digests.len(),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_CAPTURE_FRAMES, MAX_FRAME_BYTES, VISUAL_CAPTURE_CAPABILITY,
        VISUAL_CAPTURE_UNAVAILABLE_REASON, VisualCaptureRequest, VisualFrame,
        create_visual_capture_evidence, decide_visual_capture_with_flag,
        is_identity_bound_visual_capture_primitive_available,
        render_visual_capture_evidence,
    };
    use crate::runtime::budget::{RuntimeBudgetInput, create_runtime_budget};
    use crate::runtime::execution::RuntimeExecutionOutcome;
    use crate::runtime::readiness::RuntimeMode;
    use crate::tool::capability::CapabilityId;
    use crate::tool::permission::{
        PermissionPolicy, PermissionRule, PolicyRule,
    };

    fn visual_request() -> VisualCaptureRequest {
        VisualCaptureRequest {
            run_id: "run_visual_abc".to_owned(),
            operation_id: Some("op_v1".to_owned()),
            mode: RuntimeMode::Visual,
            is_stale: false,
            frames: vec![VisualFrame {
                index: 0,
                bytes: b"frame-0-bytes".to_vec(),
            }],
        }
    }

    fn allow_policy() -> PermissionPolicy {
        PermissionPolicy::from_rules(vec![PolicyRule {
            capability: CapabilityId::parse(VISUAL_CAPTURE_CAPABILITY)
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
        assert!(!is_identity_bound_visual_capture_primitive_available());
        let outcome = decide_visual_capture_with_flag(
            &visual_request(),
            &allow_policy(),
            &budget_with(1024),
            false,
        )
        .expect("decision");
        assert!(outcome.is_unavailable());
        assert!(outcome.reason().is_some_and(|reason| {
            reason.contains(VISUAL_CAPTURE_UNAVAILABLE_REASON)
        }));
        let evidence =
            create_visual_capture_evidence(&outcome, &visual_request())
                .expect("evidence");
        assert_eq!(evidence.detail.frame_count, 1);
        assert_eq!(evidence.detail.total_bytes, 13);
        assert_eq!(evidence.detail.frame_digests.len(), 1);
        assert!(
            render_visual_capture_evidence(&evidence).contains("frames=1")
        );
    }

    #[test]
    fn headless_mode_is_a_typed_pairing_refusal() {
        let mut request = visual_request();
        request.mode = RuntimeMode::Headless;
        let error = decide_visual_capture_with_flag(
            &request,
            &allow_policy(),
            &budget_with(1024),
            false,
        )
        .expect_err("headless capture refused");
        assert!(error.message.contains("requires visual mode"));
    }

    #[test]
    fn budget_exceeds_are_typed_resource_exceeded() {
        let mut request = visual_request();
        request.frames.push(VisualFrame { index: 1, bytes: vec![0u8; 1024] });
        let outcome = decide_visual_capture_with_flag(
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
        let outcome = decide_visual_capture_with_flag(
            &visual_request(),
            &allow_policy(),
            &budget_with(1024),
            true,
        )
        .expect("decision");
        assert!(matches!(outcome, RuntimeExecutionOutcome::Cancelled { .. }));
    }

    #[test]
    fn frame_bounds_are_enforced() {
        let mut request = visual_request();
        request.frames = (0..MAX_CAPTURE_FRAMES + 1)
            .map(|index| VisualFrame { index, bytes: b"x".to_vec() })
            .collect();
        let error = decide_visual_capture_with_flag(
            &request,
            &allow_policy(),
            &budget_with(1 << 30),
            false,
        )
        .expect_err("frame count refused");
        assert!(error.message.contains("frame bound"));
        let mut request = visual_request();
        request.frames[0].bytes = vec![0u8; MAX_FRAME_BYTES + 1];
        let error = decide_visual_capture_with_flag(
            &request,
            &allow_policy(),
            &budget_with(1 << 30),
            false,
        )
        .expect_err("frame size refused");
        assert!(error.message.contains("byte bound"));
    }
}
