//! Deterministic decision components (Stage 3 — Deterministic Execution
//! & Reproducibility, ADR 0029; R10a H2).
//!
//! Mirrors `packages/core/src/determinism/decisions.ts`: host-derived
//! validation plans, acceptance evaluation, typed retry policy,
//! concurrency normalization, working-set derivation, and lease
//! evaluation. Same authoritative inputs → same host decision.

use super::ports::stable_sort_by_key;
use crate::identity::{
    ArtifactIdentityError, CanonicalValue, compute_artifact_digest,
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
// Deterministic validation plan
// ---------------------------------------------------------------------------

/// Requirement class of one validation item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationRequirementClass {
    /// Must run before any mutation is applied.
    Required,
    /// Should run after application.
    Recommended,
    /// Cannot run on this platform.
    Unavailable,
}

impl ValidationRequirementClass {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Recommended => "recommended",
            Self::Unavailable => "unavailable",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "required" => Some(Self::Required),
            "recommended" => Some(Self::Recommended),
            "unavailable" => Some(Self::Unavailable),
            _ => None,
        }
    }
}

/// One validation item in a derived plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationItem {
    /// Check id from the registry.
    pub id: String,
    /// Requirement class.
    pub class: ValidationRequirementClass,
    /// Rationale binding the check to changed surfaces/evidence.
    pub rationale: String,
}

/// Derived deterministic minimum validation plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationPlan {
    /// Items in canonical (id-sorted) order.
    pub items: Vec<ValidationItem>,
    /// Digest over the ordered plan (`ValidationPlan` v1).
    pub digest: String,
}

/// One registry entry describing a known validation check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationRegistryEntry {
    /// Check id.
    pub id: String,
    /// Surfaces this check applies to.
    pub applies_to: Vec<String>,
    /// Base requirement class.
    pub base_class: ValidationRequirementClass,
}

/// One verified impact relationship between two surfaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactRelationship {
    /// Changed surface.
    pub source: String,
    /// Impacted surface.
    pub target: String,
}

/// Inputs for [`derive_validation_plan`].
pub struct ValidationPlanInput<'a> {
    /// Workspace-relative changed surfaces.
    pub changed_surfaces: &'a [String],
    /// Verified impact relationships (path pairs).
    pub impact_relationships: &'a [ImpactRelationship],
    /// Acceptance criteria as `(id, verification_kind)` pairs.
    pub acceptance_criteria: &'a [(String, String)],
    /// Registry of known validation checks.
    pub validation_registry: &'a [ValidationRegistryEntry],
}

/// Deterministic minimum validation selection: required checks derive
/// from actual changed surfaces and verified impact relationships, never
/// from model preference. Registry is processed in id order; a check
/// applies when any declared surface matches a changed or impacted path
/// exactly or by suffix/prefix.
pub fn derive_validation_plan(
    input: &ValidationPlanInput<'_>,
) -> Result<ValidationPlan, ArtifactIdentityError> {
    let mut impacted: Vec<String> = input.changed_surfaces.to_vec();
    for relationship in input.impact_relationships {
        for surface in [&relationship.source, &relationship.target] {
            if !impacted.contains(surface) {
                impacted.push(surface.clone());
            }
        }
    }
    let mut registry: Vec<&ValidationRegistryEntry> =
        input.validation_registry.iter().collect();
    registry.sort_by(|left, right| left.id.cmp(&right.id));
    let mut items: Vec<ValidationItem> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for check in registry {
        if seen.contains(&check.id) {
            continue;
        }
        let applies = check.applies_to.iter().any(|surface| {
            input.changed_surfaces.iter().any(|changed| changed == surface)
                || impacted.iter().any(|path| {
                    path.ends_with(surface.as_str())
                        || surface.ends_with(path.as_str())
                })
        });
        if !applies {
            continue;
        }
        seen.push(check.id.clone());
        items.push(ValidationItem {
            id: check.id.clone(),
            class: check.base_class,
            rationale: format!(
                "applies to changed surface(s): {}",
                check.applies_to.join(", ")
            ),
        });
    }
    let ordered = stable_sort_by_key(&items, |item| item.id.clone());
    let item_values: Vec<CanonicalValue> = ordered
        .iter()
        .map(|item| {
            object(vec![
                ("id", string_value(&item.id)),
                ("class", string_value(item.class.as_str())),
                ("rationale", string_value(&item.rationale)),
            ])
        })
        .collect();
    let digest = compute_artifact_digest(
        "ValidationPlan",
        1,
        &object(vec![("items", CanonicalValue::Array(item_values))]),
    )?
    .value;
    Ok(ValidationPlan { items: ordered, digest })
}

/// Outcome of one acceptance evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptanceOutcome {
    /// All required evidence classes present.
    Satisfied,
    /// Some required evidence classes missing.
    NotSatisfied,
    /// No evidence classes were required.
    Unverifiable,
}

impl AcceptanceOutcome {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Satisfied => "satisfied",
            Self::NotSatisfied => "not_satisfied",
            Self::Unverifiable => "unverifiable",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "satisfied" => Some(Self::Satisfied),
            "not_satisfied" => Some(Self::NotSatisfied),
            "unverifiable" => Some(Self::Unverifiable),
            _ => None,
        }
    }
}

/// One available evidence entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableEvidence {
    /// Evidence id.
    pub id: String,
    /// Evidence class.
    pub class: String,
    /// Exact content digest.
    pub digest: String,
}

impl super::ports::IdKeyed for AvailableEvidence {
    fn id_key(&self) -> String {
        self.id.clone()
    }
}

/// Result of one deterministic acceptance evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceResult {
    /// Evaluated criterion id.
    pub criterion_id: String,
    /// Outcome.
    pub outcome: AcceptanceOutcome,
    /// Evidence identities (digests), sorted by evidence id.
    pub evidence_identities: Vec<String>,
    /// Digest over the ordered decision inputs (`AcceptanceResult` v1).
    pub digest: String,
}

/// Inputs for one acceptance evaluation.
pub struct AcceptanceInput<'a> {
    /// Evaluated criterion id.
    pub criterion_id: &'a str,
    /// Required evidence classes for this criterion.
    pub required_evidence_classes: &'a [String],
    /// Available evidence, any insertion order.
    pub available_evidence: &'a [AvailableEvidence],
}

/// Deterministic acceptance: same evidence classes + identities → same
/// result. Zero required classes is `unverifiable`, not satisfied.
pub fn evaluate_acceptance(
    input: &AcceptanceInput<'_>,
) -> Result<AcceptanceResult, ArtifactIdentityError> {
    let normalized =
        stable_sort_by_key(input.available_evidence, |entry| entry.id.clone());
    let outcome = if input.required_evidence_classes.is_empty() {
        AcceptanceOutcome::Unverifiable
    } else if input.required_evidence_classes.iter().all(|required| {
        normalized.iter().any(|entry| &entry.class == required)
    }) {
        AcceptanceOutcome::Satisfied
    } else {
        AcceptanceOutcome::NotSatisfied
    };
    let evidence_identities: Vec<String> =
        normalized.iter().map(|entry| entry.digest.clone()).collect();
    let mut required_sorted: Vec<String> =
        input.required_evidence_classes.to_vec();
    required_sorted.sort();
    let digest = compute_artifact_digest(
        "AcceptanceResult",
        1,
        &object(vec![
            ("criterionId", string_value(input.criterion_id)),
            ("outcome", string_value(outcome.as_str())),
            (
                "evidenceIdentities",
                CanonicalValue::Array(
                    evidence_identities
                        .iter()
                        .map(|value| string_value(value))
                        .collect(),
                ),
            ),
            (
                "requiredEvidenceClasses",
                CanonicalValue::Array(
                    required_sorted
                        .iter()
                        .map(|value| string_value(value))
                        .collect(),
                ),
            ),
        ]),
    )?
    .value;
    Ok(AcceptanceResult {
        criterion_id: input.criterion_id.to_owned(),
        outcome,
        evidence_identities,
        digest,
    })
}

/// Typed retry decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    /// Bounded retry applies.
    Retry,
    /// A bounded repair attempt applies.
    Repair,
    /// No retry; the operation ends failed/blocked.
    NoRetry,
}

impl RetryDecision {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Retry => "retry",
            Self::Repair => "repair",
            Self::NoRetry => "no_retry",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "retry" => Some(Self::Retry),
            "repair" => Some(Self::Repair),
            "no_retry" => Some(Self::NoRetry),
            _ => None,
        }
    }
}

/// Failure category driving the retry classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryCategory {
    /// Transient provider transport failure.
    TransientProviderTransport,
    /// Source revision went stale under the approval.
    StaleSourceRevision,
    /// Blocking review finding.
    BlockingReviewFinding,
    /// Malformed tool representation.
    MalformedToolRepresentation,
    /// Approval denied.
    ApprovalDenied,
    /// Infrastructure unavailable.
    InfrastructureUnavailable,
    /// Validation failed.
    ValidationFailed,
    /// Unclassified failure.
    Unknown,
}

impl RetryCategory {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TransientProviderTransport => "transient_provider_transport",
            Self::StaleSourceRevision => "stale_source_revision",
            Self::BlockingReviewFinding => "blocking_review_finding",
            Self::MalformedToolRepresentation => {
                "malformed_tool_representation"
            }
            Self::ApprovalDenied => "approval_denied",
            Self::InfrastructureUnavailable => "infrastructure_unavailable",
            Self::ValidationFailed => "validation_failed",
            Self::Unknown => "unknown",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "transient_provider_transport" => {
                Some(Self::TransientProviderTransport)
            }
            "stale_source_revision" => Some(Self::StaleSourceRevision),
            "blocking_review_finding" => Some(Self::BlockingReviewFinding),
            "malformed_tool_representation" => {
                Some(Self::MalformedToolRepresentation)
            }
            "approval_denied" => Some(Self::ApprovalDenied),
            "infrastructure_unavailable" => {
                Some(Self::InfrastructureUnavailable)
            }
            "validation_failed" => Some(Self::ValidationFailed),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

/// Host-owned retry policy: the model never decides retry counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Maximum attempts before the budget is exhausted.
    pub attempt_limit: u32,
}

/// Default bounded retry policy: three attempts.
pub const DEFAULT_RETRY_POLICY: RetryPolicy = RetryPolicy { attempt_limit: 3 };

/// Deterministic exponential backoff in ms per 0-based attempt index:
/// `min(100 * 2^attemptIndex, 2000)`.
#[must_use]
pub fn default_backoff_ms(attempt_index: u32) -> u64 {
    100_u64.saturating_mul(2_u64.saturating_pow(attempt_index)).min(2_000)
}

/// One retry classification result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryClassification {
    /// The decision.
    pub decision: RetryDecision,
    /// Bounded truthful reason.
    pub reason: String,
    /// Backoff before the next attempt, when retrying.
    pub next_backoff_ms: Option<u64>,
}

/// Host-owned retry classification: same category + attempts + policy →
/// same classification. Only transient transport failures retry; stale
/// revisions and validation failures require fresh preparation;
/// approval denial never retries.
#[must_use]
pub fn classify_retry(
    category: RetryCategory,
    attempts_used: u32,
    policy: RetryPolicy,
) -> RetryClassification {
    match category {
        RetryCategory::TransientProviderTransport => {
            if attempts_used < policy.attempt_limit {
                RetryClassification {
                    decision: RetryDecision::Retry,
                    reason: format!(
                        "Transient provider transport failure; bounded retry {}/{}.",
                        attempts_used + 1,
                        policy.attempt_limit
                    ),
                    next_backoff_ms: Some(default_backoff_ms(attempts_used)),
                }
            } else {
                RetryClassification {
                    decision: RetryDecision::NoRetry,
                    reason: format!(
                        "Retry budget exhausted ({}); the operation ends failed.",
                        policy.attempt_limit
                    ),
                    next_backoff_ms: None,
                }
            }
        }
        RetryCategory::MalformedToolRepresentation => RetryClassification {
            decision: RetryDecision::Repair,
            reason:
                "Malformed tool representation; a bounded repair attempt is the host response."
                    .to_owned(),
            next_backoff_ms: None,
        },
        RetryCategory::BlockingReviewFinding => RetryClassification {
            decision: RetryDecision::Repair,
            reason:
                "Blocking review finding; the existing bounded repair loop applies."
                    .to_owned(),
            next_backoff_ms: None,
        },
        RetryCategory::StaleSourceRevision => RetryClassification {
            decision: RetryDecision::NoRetry,
            reason: "Stale source revision: no automatic mutation retry under the old approval; re-prepare from the current revision.".to_owned(),
            next_backoff_ms: None,
        },
        RetryCategory::ApprovalDenied => RetryClassification {
            decision: RetryDecision::NoRetry,
            reason: "Approval denied: the operation ends without retry.".to_owned(),
            next_backoff_ms: None,
        },
        RetryCategory::InfrastructureUnavailable => RetryClassification {
            decision: RetryDecision::NoRetry,
            reason:
                "Infrastructure unavailable: fail closed, never retry into an unsafe state."
                    .to_owned(),
            next_backoff_ms: None,
        },
        RetryCategory::ValidationFailed => RetryClassification {
            decision: RetryDecision::NoRetry,
            reason: "Validation failed: the result is reported failed; repairs require fresh preparation.".to_owned(),
            next_backoff_ms: None,
        },
        RetryCategory::Unknown => RetryClassification {
            decision: RetryDecision::NoRetry,
            reason: "Unclassified failure: no automatic retry.".to_owned(),
            next_backoff_ms: None,
        },
    }
}

/// Normalize concurrently collected results into canonical id order
/// before they affect authoritative decisions or provider context.
/// Completion order never equals semantic priority.
pub fn normalize_concurrent_results<T: super::ports::IdKeyed + Clone>(
    results: &[T],
) -> Vec<T> {
    stable_sort_by_key(results, |entry| entry.id_key())
}

/// One entry of the derived active working set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveWorkingSetEntry {
    /// Workspace-relative path.
    pub path: String,
    /// Inclusion reason from the discovery relevance class.
    pub reason: String,
}

/// One ranked discovery candidate feeding the working set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkingSetCandidate {
    /// Candidate path.
    pub path: String,
    /// Relevance class string ("verified" or candidate).
    pub relevance: String,
}

/// Deterministic initial active working set derived from the ordered
/// discovery result (never from the model). Equivalent inputs produce
/// the same set.
#[must_use]
pub fn derive_active_working_set(
    ordered_candidates: &[WorkingSetCandidate],
    max_entries: usize,
) -> Vec<ActiveWorkingSetEntry> {
    ordered_candidates
        .iter()
        .take(max_entries)
        .map(|candidate| ActiveWorkingSetEntry {
            path: candidate.path.clone(),
            reason: if candidate.relevance == "verified" {
                "verified discovery candidate".to_owned()
            } else {
                "candidate discovery candidate".to_owned()
            },
        })
        .collect()
}

/// Deterministic lease/expiry result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaseEvaluation {
    /// True while issued-at plus TTL exceeds now.
    pub valid: bool,
    /// Remaining milliseconds (zero past expiry).
    pub remaining_ms: u64,
}

/// Deterministic lease evaluation: same clock input → same decision.
#[must_use]
pub fn evaluate_lease(
    issued_at_ms: u64,
    ttl_ms: u64,
    now_ms: u64,
) -> LeaseEvaluation {
    let expiry = issued_at_ms.saturating_add(ttl_ms);
    if expiry > now_ms {
        LeaseEvaluation { valid: true, remaining_ms: expiry - now_ms }
    } else {
        LeaseEvaluation { valid: false, remaining_ms: 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AcceptanceInput, AcceptanceOutcome, AvailableEvidence,
        DEFAULT_RETRY_POLICY, ImpactRelationship, RetryCategory,
        RetryDecision, ValidationRegistryEntry, WorkingSetCandidate,
        classify_retry, derive_active_working_set, derive_validation_plan,
        evaluate_acceptance, evaluate_lease, normalize_concurrent_results,
    };

    fn string_list(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn validation_plan_derives_from_changed_and_impacted_surfaces() {
        let changed = string_list(&["src/player.gd"]);
        let relationships = vec![ImpactRelationship {
            source: "src/player.gd".to_owned(),
            target: "res://player.tscn".to_owned(),
        }];
        let criteria = vec![("ac1".to_owned(), "deterministic".to_owned())];
        let registry = vec![ValidationRegistryEntry {
            id: "check.gd.parse".to_owned(),
            applies_to: string_list(&["player.gd"]),
            base_class: super::ValidationRequirementClass::Required,
        }];
        let plan = derive_validation_plan(&super::ValidationPlanInput {
            changed_surfaces: &changed,
            impact_relationships: &relationships,
            acceptance_criteria: &criteria,
            validation_registry: &registry,
        })
        .expect("plan derives");
        assert_eq!(plan.items.len(), 1);
        assert_eq!(plan.items[0].id, "check.gd.parse");
        assert_eq!(plan.items[0].class.as_str(), "required");
        assert!(plan.digest.len() == 64);
    }

    #[test]
    fn acceptance_is_satisfied_when_all_required_classes_present() {
        let evidence = [
            AvailableEvidence {
                id: "e1".to_owned(),
                class: "parser_result".to_owned(),
                digest: "d1".to_owned(),
            },
            AvailableEvidence {
                id: "e2".to_owned(),
                class: "review".to_owned(),
                digest: "d2".to_owned(),
            },
        ];
        let result = evaluate_acceptance(&AcceptanceInput {
            criterion_id: "ac1",
            required_evidence_classes: &string_list(&["parser_result"]),
            available_evidence: &evidence,
        })
        .expect("evaluates");
        assert_eq!(result.outcome, AcceptanceOutcome::Satisfied);
        assert_eq!(result.evidence_identities, vec!["d1", "d2"]);

        let zero_required = evaluate_acceptance(&AcceptanceInput {
            criterion_id: "ac-empty",
            required_evidence_classes: &[],
            available_evidence: &evidence,
        })
        .expect("evaluates");
        assert_eq!(zero_required.outcome, AcceptanceOutcome::Unverifiable);
    }

    #[test]
    fn retry_classification_covers_the_eight_categories() {
        let policy = DEFAULT_RETRY_POLICY;
        let retry = classify_retry(
            RetryCategory::TransientProviderTransport,
            0,
            policy,
        );
        assert_eq!(retry.decision, RetryDecision::Retry);
        assert!(retry.next_backoff_ms.is_some());
        let exhausted = classify_retry(
            RetryCategory::TransientProviderTransport,
            3,
            policy,
        );
        assert_eq!(exhausted.decision, RetryDecision::NoRetry);
        for (category, expected) in [
            (RetryCategory::StaleSourceRevision, RetryDecision::NoRetry),
            (RetryCategory::ApprovalDenied, RetryDecision::NoRetry),
            (RetryCategory::InfrastructureUnavailable, RetryDecision::NoRetry),
            (RetryCategory::ValidationFailed, RetryDecision::NoRetry),
            (RetryCategory::Unknown, RetryDecision::NoRetry),
            (
                RetryCategory::MalformedToolRepresentation,
                RetryDecision::Repair,
            ),
            (RetryCategory::BlockingReviewFinding, RetryDecision::Repair),
        ] {
            assert_eq!(
                classify_retry(category, 0, policy).decision,
                expected,
                "{category:?}"
            );
        }
    }

    #[test]
    fn concurrent_results_normalize_to_canonical_id_order() {
        #[derive(Clone)]
        struct Entry {
            id: String,
        }
        impl super::super::ports::IdKeyed for Entry {
            fn id_key(&self) -> String {
                self.id.clone()
            }
        }
        let results = vec![
            Entry { id: "c".to_owned() },
            Entry { id: "a".to_owned() },
            Entry { id: "b".to_owned() },
        ];
        let normalized = normalize_concurrent_results(&results);
        assert_eq!(
            normalized
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
    }

    #[test]
    fn working_set_takes_the_first_n_candidates_with_reasons() {
        let candidates = [
            WorkingSetCandidate {
                path: "a.gd".to_owned(),
                relevance: "verified".to_owned(),
            },
            WorkingSetCandidate {
                path: "b.gd".to_owned(),
                relevance: "candidate".to_owned(),
            },
            WorkingSetCandidate {
                path: "c.gd".to_owned(),
                relevance: "verified".to_owned(),
            },
        ];
        let set = derive_active_working_set(&candidates, 2);
        assert_eq!(set.len(), 2);
        assert_eq!(set[0].reason, "verified discovery candidate");
        assert_eq!(set[1].reason, "candidate discovery candidate");
    }

    #[test]
    fn lease_evaluation_is_deterministic() {
        let valid = evaluate_lease(1000, 500, 1200);
        assert!(valid.valid);
        assert_eq!(valid.remaining_ms, 300);
        let expired = evaluate_lease(1000, 500, 2000);
        assert!(!expired.valid);
        assert_eq!(expired.remaining_ms, 0);
    }
}
