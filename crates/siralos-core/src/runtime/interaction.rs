//! Bounded run interaction rounds (Stage 4.5, decision 43).
//!
//! The fifth arrow of the frozen Stage-4 sequence (decision 08, step 5):
//! typed request/response exchange rounds with a supervised run, routed
//! through the same generic host decision order as
//! [`execution`](super::execution) and [`visual`](super::visual) —
//! validation, capability, staleness, budget, cancellation, primitive —
//! over the closed 13-kind failure taxonomy and the 6-disposition table,
//! neither of which is extended.
//!
//! The interactive primitive is identity-bound and absent on this
//! platform, so every otherwise-valid interaction reports typed
//! `UNAVAILABLE` with the exact reason
//! `"identity-bound interactive-run primitive not available"`. A run
//! interaction against a non-interactive (one-shot) run is a typed
//! pairing refusal (mirroring the headless-capture refusal in
//! [`visual`](super::visual)). No process is launched, no live process
//! I/O occurs, no filesystem is touched, and no wall clock is read: all
//! round content is injected request data, and evidence binds digests and
//! counts only — never raw transcript streams.

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

/// Capability required for any run interaction.
pub const RUN_INTERACTION_CAPABILITY: &str = "run.interact";

/// The exact typed unavailability reason when the identity-bound
/// interactive-run primitive is absent.
pub const RUN_INTERACTION_UNAVAILABLE_REASON: &str =
    "identity-bound interactive-run primitive not available";

/// Maximum number of interaction rounds in one request.
pub const MAX_INTERACTION_ROUNDS: usize = 16;

/// Maximum bytes per interaction-round request line.
pub const MAX_ROUND_BYTES: usize = 64 * 1024;

/// Whether the identity-bound interactive-run primitive is available on
/// this platform.
///
/// Stage 4.5 is fail-closed (decision 43 C3): the primitive is absent, so
/// interaction always reports typed `UNAVAILABLE` without mutation,
/// spawn, or live process I/O. No filesystem probe, no ambient check, no
/// wall clock is consulted.
#[must_use]
pub const fn is_identity_bound_interactive_run_primitive_available() -> bool {
    false
}

/// One bounded interaction-round request declared by the caller.
/// `index` must equal the round's zero-based position; request content
/// is injected data that only ever leaves this module as a digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionRound {
    /// Zero-based round position.
    pub index: usize,
    /// Bounded request line (never surfaced in evidence).
    pub request: String,
}

/// Validated inputs for [`decide_run_interaction`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunInteractionRequest {
    /// Owning run id (non-empty, bounded).
    pub run_id: String,
    /// Optional operation id; when absent a deterministic id is derived.
    pub operation_id: Option<String>,
    /// Whether the target run is interactive; one-shot runs refuse.
    pub is_interactive: bool,
    /// Whether the observed revision is stale.
    pub is_stale: bool,
    /// Declared rounds (bounded count, bounded per-round bytes).
    pub rounds: Vec<InteractionRound>,
}

impl RunInteractionRequest {
    /// Total declared interaction bytes; the budget compares this against
    /// [`RuntimeBudget::artifact_bytes`].
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.rounds.iter().map(|round| round.request.len() as u64).sum()
    }
}

/// Validate a run interaction request. A one-shot target is a pairing
/// refusal (the adapter-side pairing discipline, generic here): an
/// interaction without an interactive run is a malformed request, never a
/// silent success.
fn validate_run_interaction_request(
    request: &RunInteractionRequest,
) -> Result<(), RuntimeError> {
    if request.run_id.is_empty() {
        return Err(runtime_error("A run interaction requires a run id."));
    }
    if request.run_id.len() > MAX_RUN_ID_BYTES {
        return Err(runtime_error(format!(
            "The run interaction run id exceeds the {MAX_RUN_ID_BYTES}-byte bound."
        )));
    }
    if let Some(operation_id) = &request.operation_id {
        if operation_id.is_empty() {
            return Err(runtime_error(
                "A run interaction operation id must not be empty.",
            ));
        }
        if operation_id.len() > MAX_OPERATION_ID_BYTES {
            return Err(runtime_error(format!(
                "The run interaction operation id exceeds the {MAX_OPERATION_ID_BYTES}-byte bound."
            )));
        }
    }
    if !request.is_interactive {
        return Err(runtime_error(
            "A run interaction requires an interactive run; one-shot runs never accept interaction rounds.",
        ));
    }
    if request.rounds.is_empty() {
        return Err(runtime_error(
            "A run interaction requires at least one round.",
        ));
    }
    if request.rounds.len() > MAX_INTERACTION_ROUNDS {
        return Err(runtime_error(format!(
            "The run interaction exceeds the {MAX_INTERACTION_ROUNDS}-round bound."
        )));
    }
    for (position, round) in request.rounds.iter().enumerate() {
        if round.index != position {
            return Err(runtime_error(
                "A run interaction round index must equal its zero-based position.",
            ));
        }
        if round.request.is_empty() {
            return Err(runtime_error(
                "A run interaction round must not be empty.",
            ));
        }
        if round.request.len() > MAX_ROUND_BYTES {
            return Err(runtime_error(format!(
                "A run interaction round exceeds the {MAX_ROUND_BYTES}-byte bound."
            )));
        }
        if round.request.contains('\0') {
            return Err(runtime_error(
                "A run interaction round must not contain NUL.",
            ));
        }
    }
    Ok(())
}

/// Host-authorized run interaction decision table.
///
/// Pure and deterministic: equivalent inputs produce the same
/// [`RuntimeExecutionOutcome`]. The gate order is the generic table's own
/// order with the interaction capability and primitive; no I/O, no spawn,
/// no live process interaction, no wall-clock read.
///
/// # Errors
///
/// Returns [`RuntimeError`] only for malformed inputs; all capability,
/// staleness, budget, cancellation, and primitive-absent cases are typed
/// [`RuntimeExecutionOutcome`] variants, never errors.
pub fn decide_run_interaction(
    request: &RunInteractionRequest,
    policy: &PermissionPolicy,
    budget: &RuntimeBudget,
    cancellation: CancellationSignal<'_>,
) -> Result<RuntimeExecutionOutcome, RuntimeError> {
    validate_run_interaction_request(request)?;

    // 2. Capability gate: run.interact must be allow.
    let capability = CapabilityId::parse(RUN_INTERACTION_CAPABILITY)
        .expect("run.interact is a valid capability id");
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

    // 4. Budget gate: declared interaction bytes exceed the host budget.
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
            reason: "CANCELLED: run interaction was cancelled.".to_owned(),
        });
    }

    // 6. Identity-bound primitive gate.
    if !is_identity_bound_interactive_run_primitive_available() {
        return Ok(RuntimeExecutionOutcome::Unavailable {
            reason: format!(
                "UNAVAILABLE: {RUN_INTERACTION_UNAVAILABLE_REASON}"
            ),
        });
    }

    // 7. Success (only observable when the primitive is available).
    let operation_id = request.operation_id.clone().unwrap_or_else(|| {
        create_operation_id(&request.run_id, RUN_INTERACTION_CAPABILITY)
    });
    Ok(RuntimeExecutionOutcome::Success {
        run_id: request.run_id.clone(),
        operation_id,
    })
}

/// Convenience wrapper that accepts a simple cancellation flag. Useful for
/// unit tests and for callers that already materialized a boolean.
pub fn decide_run_interaction_with_flag(
    request: &RunInteractionRequest,
    policy: &PermissionPolicy,
    budget: &RuntimeBudget,
    is_cancelled: bool,
) -> Result<RuntimeExecutionOutcome, RuntimeError> {
    let token = CancellationToken::new();
    if is_cancelled {
        token.cancel();
    }
    decide_run_interaction(request, policy, budget, token.signal())
}

/// Interaction-structured detail bound to an interaction outcome: round
/// count, per-round request digests, and total declared bytes — never raw
/// transcript streams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunInteractionEvidenceDetail {
    /// Number of declared rounds.
    pub round_count: usize,
    /// Deterministic digest per round, in round order.
    pub round_digests: Vec<String>,
    /// Total declared interaction bytes.
    pub total_bytes: u64,
}

/// An interaction outcome bound to its interaction-structured detail
/// under a domain-separated digest over `RunInteractionEvidence v1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunInteractionEvidence {
    /// The typed decision-table outcome (dispositions unchanged).
    pub outcome: RuntimeExecutionOutcome,
    /// Interaction-shaped detail (digests and counts only).
    pub detail: RunInteractionEvidenceDetail,
    /// Domain-separated digest over the outcome disposition and detail.
    pub interaction_digest: String,
}

/// Build the run interaction evidence record for a decided outcome: binds
/// the outcome to per-round digests and the interaction detail under a
/// domain-separated digest. The request is re-validated so malformed
/// inputs can never produce evidence.
///
/// # Errors
///
/// Returns [`RuntimeError`] for malformed requests and for
/// outcome/detail mismatches.
pub fn create_run_interaction_evidence(
    outcome: &RuntimeExecutionOutcome,
    request: &RunInteractionRequest,
) -> Result<RunInteractionEvidence, RuntimeError> {
    validate_run_interaction_request(request)?;
    let mut round_digests = Vec::with_capacity(request.rounds.len());
    for round in &request.rounds {
        let payload = CanonicalValue::Object(BTreeMap::from([
            ("index".to_owned(), CanonicalValue::U64(round.index as u64)),
            (
                "len".to_owned(),
                CanonicalValue::U64(round.request.len() as u64),
            ),
            (
                "sha256".to_owned(),
                CanonicalValue::Str(sha256_hex(round.request.as_bytes())),
            ),
        ]));
        let digest =
            compute_artifact_digest("RunInteractionRound", 1, &payload)
                .map_err(|error| RuntimeError { message: error.message })?
                .value;
        round_digests.push(digest);
    }
    let total_bytes = request.total_bytes();
    let detail = RunInteractionEvidenceDetail {
        round_count: request.rounds.len(),
        round_digests,
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
                    "roundCount".to_owned(),
                    CanonicalValue::U64(detail.round_count as u64),
                ),
                (
                    "roundDigests".to_owned(),
                    CanonicalValue::Array(
                        detail
                            .round_digests
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
    let interaction_digest = compute_artifact_digest(
        "RunInteractionEvidence",
        1,
        &evidence_payload,
    )
    .map_err(|error| RuntimeError { message: error.message })?
    .value;
    Ok(RunInteractionEvidence {
        outcome: outcome.clone(),
        detail,
        interaction_digest,
    })
}

/// Bounded deterministic rendering: disposition, reason, and the
/// interaction detail (lengths and digests only, never transcript
/// streams).
#[must_use]
pub fn render_run_interaction_evidence(
    evidence: &RunInteractionEvidence,
) -> String {
    let reason = evidence.outcome.reason().unwrap_or("-");
    format!(
        "{} rounds={} bytes={} digests={}",
        reason,
        evidence.detail.round_count,
        evidence.detail.total_bytes,
        evidence.detail.round_digests.len(),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_INTERACTION_ROUNDS, MAX_ROUND_BYTES, RUN_INTERACTION_CAPABILITY,
        RUN_INTERACTION_UNAVAILABLE_REASON, RunInteractionRequest,
        create_run_interaction_evidence, decide_run_interaction_with_flag,
        is_identity_bound_interactive_run_primitive_available,
        render_run_interaction_evidence,
    };
    use crate::runtime::budget::{RuntimeBudgetInput, create_runtime_budget};
    use crate::runtime::execution::RuntimeExecutionOutcome;
    use crate::tool::capability::CapabilityId;
    use crate::tool::permission::{
        PermissionPolicy, PermissionRule, PolicyRule,
    };

    fn interaction_request() -> RunInteractionRequest {
        RunInteractionRequest {
            run_id: "run_interact_abc".to_owned(),
            operation_id: Some("op_i1".to_owned()),
            is_interactive: true,
            is_stale: false,
            rounds: vec![super::InteractionRound {
                index: 0,
                request: "status".to_owned(),
            }],
        }
    }

    fn allow_policy() -> PermissionPolicy {
        PermissionPolicy::from_rules(vec![PolicyRule {
            capability: CapabilityId::parse(RUN_INTERACTION_CAPABILITY)
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
        assert!(!is_identity_bound_interactive_run_primitive_available());
        let outcome = decide_run_interaction_with_flag(
            &interaction_request(),
            &allow_policy(),
            &budget_with(1024),
            false,
        )
        .expect("decision");
        assert!(outcome.is_unavailable());
        assert!(outcome.reason().is_some_and(|reason| {
            reason.contains(RUN_INTERACTION_UNAVAILABLE_REASON)
        }));
        let evidence =
            create_run_interaction_evidence(&outcome, &interaction_request())
                .expect("evidence");
        assert_eq!(evidence.detail.round_count, 1);
        assert_eq!(evidence.detail.total_bytes, 6);
        assert_eq!(evidence.detail.round_digests.len(), 1);
        assert!(
            render_run_interaction_evidence(&evidence).contains("rounds=1")
        );
    }

    #[test]
    fn one_shot_targets_are_a_typed_pairing_refusal() {
        let mut request = interaction_request();
        request.is_interactive = false;
        let error = decide_run_interaction_with_flag(
            &request,
            &allow_policy(),
            &budget_with(1024),
            false,
        )
        .expect_err("one-shot interaction refused");
        assert!(error.message.contains("requires an interactive run"));
    }

    #[test]
    fn budget_exceeds_are_typed_resource_exceeded() {
        let mut request = interaction_request();
        request.rounds.push(super::InteractionRound {
            index: 1,
            request: "a".repeat(64),
        });
        let outcome = decide_run_interaction_with_flag(
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
        let outcome = decide_run_interaction_with_flag(
            &interaction_request(),
            &allow_policy(),
            &budget_with(1024),
            true,
        )
        .expect("decision");
        assert!(matches!(outcome, RuntimeExecutionOutcome::Cancelled { .. }));
    }

    #[test]
    fn round_bounds_are_enforced() {
        let mut request = interaction_request();
        request.rounds = (0..MAX_INTERACTION_ROUNDS + 1)
            .map(|index| super::InteractionRound {
                index,
                request: "x".to_owned(),
            })
            .collect();
        let error = decide_run_interaction_with_flag(
            &request,
            &allow_policy(),
            &budget_with(1 << 30),
            false,
        )
        .expect_err("round count refused");
        assert!(error.message.contains("round bound"));
        let mut request = interaction_request();
        request.rounds[0].request = "a".repeat(MAX_ROUND_BYTES + 1);
        let error = decide_run_interaction_with_flag(
            &request,
            &allow_policy(),
            &budget_with(1 << 30),
            false,
        )
        .expect_err("round size refused");
        assert!(error.message.contains("byte bound"));
    }
}
