//! Bounded dependency-aware staleness rules (Stage 3 — Content Identity
//! & Delta Verification, ADR 0028; R10a H1).
//!
//! Mirrors `packages/core/src/identity/staleness.ts`: explicit
//! high-value dependency rules over exact digests, never a generic
//! reactive dependency graph. Rules are pure functions; the
//! authoritative state (task phases, approvals, evidence) remains the
//! owner of any actual invalidation.

/// Inputs for one identity-staleness derivation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IdentityStalenessInput {
    /// Current TaskContract content digest.
    pub contract_digest: Option<String>,
    /// The digest the plan was created against.
    pub plan_contract_digest: Option<String>,
    /// Current guidance content digest.
    pub guidance_digest: Option<String>,
    /// The digest the execution context was built against.
    pub prior_guidance_digest: Option<String>,
    /// Current changeset content digest.
    pub changeset_digest: Option<String>,
    /// The changeset digest the review recorded.
    pub review_input_changeset_digest: Option<String>,
    /// Current validation evidence digest.
    pub validation_evidence_digest: Option<String>,
    /// The evidence digest acceptance accepted.
    pub accepted_evidence_digest: Option<String>,
}

/// One honest staleness derivation with its reasons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityStaleness {
    /// The TaskContract content changed since the plan was created.
    pub plan_potentially_stale: bool,
    /// Guidance changed since planning/execution context was built.
    pub execution_context_potentially_stale: bool,
    /// The reviewed changeset content changed since the last review.
    pub review_stale: bool,
    /// Validation evidence changed since the last acceptance evaluation.
    pub acceptance_requires_reevaluation: bool,
    /// Bounded truthful reasons, in rule order.
    pub reasons: Vec<String>,
}

fn drifted(current: &Option<String>, recorded: &Option<String>) -> bool {
    match (current, recorded) {
        (Some(current), Some(recorded)) => current != recorded,
        _ => false,
    }
}

/// Derive identity staleness: a rule fires only when both digests are
/// present and differ; absent evidence never fabricates staleness.
pub fn derive_identity_staleness(
    input: &IdentityStalenessInput,
) -> IdentityStaleness {
    let mut reasons = Vec::new();

    let plan_potentially_stale =
        drifted(&input.contract_digest, &input.plan_contract_digest);
    if plan_potentially_stale {
        reasons.push(
            "TaskContract content digest changed since the plan was created; the plan is potentially stale.".to_owned(),
        );
    }

    let execution_context_potentially_stale =
        drifted(&input.guidance_digest, &input.prior_guidance_digest);
    if execution_context_potentially_stale {
        reasons.push(
            "Guidance digest changed; planning/execution context may be stale.".to_owned(),
        );
    }

    let review_stale =
        drifted(&input.changeset_digest, &input.review_input_changeset_digest);
    if review_stale {
        reasons.push(
            "The reviewed changeset content changed; the previous review no longer applies."
                .to_owned(),
        );
    }

    let acceptance_requires_reevaluation = drifted(
        &input.validation_evidence_digest,
        &input.accepted_evidence_digest,
    );
    if acceptance_requires_reevaluation {
        reasons.push(
            "Validation evidence changed; acceptance must be reevaluated against the current evidence set."
                .to_owned(),
        );
    }

    IdentityStaleness {
        plan_potentially_stale,
        execution_context_potentially_stale,
        review_stale,
        acceptance_requires_reevaluation,
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use super::{IdentityStalenessInput, derive_identity_staleness};

    #[test]
    fn absent_evidence_never_fabricates_staleness() {
        let staleness =
            derive_identity_staleness(&IdentityStalenessInput::default());
        assert!(!staleness.plan_potentially_stale);
        assert!(!staleness.execution_context_potentially_stale);
        assert!(!staleness.review_stale);
        assert!(!staleness.acceptance_requires_reevaluation);
        assert!(staleness.reasons.is_empty());
    }

    #[test]
    fn identical_digests_count_as_current() {
        let staleness = derive_identity_staleness(&IdentityStalenessInput {
            contract_digest: Some("d1".to_owned()),
            plan_contract_digest: Some("d1".to_owned()),
            guidance_digest: Some("g".to_owned()),
            prior_guidance_digest: Some("g".to_owned()),
            changeset_digest: Some("c".to_owned()),
            review_input_changeset_digest: Some("c".to_owned()),
            validation_evidence_digest: Some("v".to_owned()),
            accepted_evidence_digest: Some("v".to_owned()),
        });
        assert_eq!(staleness.reasons.len(), 0);
        assert!(!staleness.plan_potentially_stale);
    }

    #[test]
    fn each_drifted_pair_reports_in_rule_order_with_oracle_reasons() {
        let staleness = derive_identity_staleness(&IdentityStalenessInput {
            contract_digest: Some("contract-2".to_owned()),
            plan_contract_digest: Some("contract-1".to_owned()),
            guidance_digest: Some("guidance-2".to_owned()),
            prior_guidance_digest: Some("guidance-1".to_owned()),
            changeset_digest: Some("changeset-2".to_owned()),
            review_input_changeset_digest: Some("changeset-1".to_owned()),
            validation_evidence_digest: Some("evidence-2".to_owned()),
            accepted_evidence_digest: Some("evidence-1".to_owned()),
        });
        assert!(staleness.plan_potentially_stale);
        assert!(staleness.execution_context_potentially_stale);
        assert!(staleness.review_stale);
        assert!(staleness.acceptance_requires_reevaluation);
        assert_eq!(
            staleness.reasons,
            vec![
                "TaskContract content digest changed since the plan was created; the plan is potentially stale.".to_owned(),
                "Guidance digest changed; planning/execution context may be stale.".to_owned(),
                "The reviewed changeset content changed; the previous review no longer applies.".to_owned(),
                "Validation evidence changed; acceptance must be reevaluated against the current evidence set.".to_owned(),
            ]
        );
    }
}
