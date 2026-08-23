//! Context provenance and why-diagnostics (Stage 3 — Interpretable
//! Context Architecture, ADR 0030; R10b ICM).
//!
//! Mirrors `packages/core/src/context/provenance.ts`. Important context
//! items carry bounded provenance references; why-diagnostics derive
//! from structured provenance/evidence deterministically — never by
//! asking another model to reconstruct reasoning.

use super::{ContextError, context_error};
use crate::determinism::{
    ImpactRelationship, ValidationItem, stable_sort_by_key,
};
use crate::identity::{CanonicalValue, compute_artifact_digest};

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

/// Bounded source of one provenance reference. `kind` is an opaque
/// protocol string in the oracle vocabulary; the reference never
/// carries authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextProvenanceRefSource {
    /// Source kind, e.g. `execution_contract` or `phase_contract`.
    pub kind: String,
    /// Stable source id.
    pub id: String,
    /// Exact content digest of the source when observed; `None`
    /// serializes as JSON null.
    pub digest: Option<String>,
}

/// One bounded provenance reference for a context item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextProvenanceRef {
    /// The context item being explained.
    pub item: String,
    /// Where the item came from.
    pub source: ContextProvenanceRefSource,
}

/// Create a provenance reference. Both the item and the source id are
/// required; the digest is optional and stays `null` when unobserved.
pub fn create_context_provenance_ref(
    item: &str,
    kind: &str,
    id: &str,
    digest: Option<&str>,
) -> Result<ContextProvenanceRef, ContextError> {
    if item.is_empty() || id.is_empty() {
        return Err(context_error(
            "A provenance reference requires an item and a source id.",
        ));
    }
    Ok(ContextProvenanceRef {
        item: item.to_owned(),
        source: ContextProvenanceRefSource {
            kind: kind.to_owned(),
            id: id.to_owned(),
            digest: digest.map(str::to_owned),
        },
    })
}

fn ref_value(reference: &ContextProvenanceRef) -> CanonicalValue {
    let source = object(vec![
        ("kind", string_value(&reference.source.kind)),
        ("id", string_value(&reference.source.id)),
        (
            "digest",
            match &reference.source.digest {
                Some(digest) => string_value(digest),
                None => CanonicalValue::Null,
            },
        ),
    ]);
    object(vec![("item", string_value(&reference.item)), ("source", source)])
}

/// Bounded deterministic digest over a provenance set (evidence
/// identity). Ordering is canonical over `item:kind:id`, so insertion
/// order never affects the digest.
pub fn compute_provenance_digest(
    refs: &[ContextProvenanceRef],
) -> Result<String, ContextError> {
    let ordered = stable_sort_by_key(refs, |reference| {
        format!(
            "{}:{}:{}",
            reference.item, reference.source.kind, reference.source.id
        )
    });
    let payload = object(vec![(
        "refs",
        CanonicalValue::Array(ordered.iter().map(ref_value).collect()),
    )]);
    let digest = compute_artifact_digest("ContextProvenance", 1, &payload)
        .map_err(|error| context_error(error.message))?;
    Ok(digest.value)
}

// ---------------------------------------------------------------------------
// Why-diagnostics
// ---------------------------------------------------------------------------

/// Deterministic answer for "why is this validation required", derived
/// from the ValidationPlan rationale — never a model invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhyValidationRequired {
    /// The validation item being explained.
    pub item_id: String,
    /// Surfaces that changed.
    pub changed_surfaces: Vec<String>,
    /// Verified impact relationships.
    pub impact_relations: Vec<ImpactRelationship>,
    /// Acceptance criteria bound to the validation.
    pub acceptance_criteria: Vec<String>,
}

/// Derive [`WhyValidationRequired`] from the plan items; `None` when the
/// item is not part of the plan.
pub fn why_validation_required(
    item_id: &str,
    plan_items: &[ValidationItem],
    changed_surfaces: &[String],
    impact_relations: &[ImpactRelationship],
    acceptance_criteria: &[String],
) -> Option<WhyValidationRequired> {
    plan_items.iter().any(|item| item.id == item_id).then(|| {
        WhyValidationRequired {
            item_id: item_id.to_owned(),
            changed_surfaces: changed_surfaces.to_vec(),
            impact_relations: impact_relations.to_vec(),
            acceptance_criteria: acceptance_criteria.to_vec(),
        }
    })
}

/// Rendered why-diagnostic (bounded, human-readable).
#[must_use]
pub fn render_why_validation_required(
    diagnostic: &WhyValidationRequired,
) -> String {
    let mut lines = vec!["Required because (validation_plan):".to_owned()];
    if !diagnostic.changed_surfaces.is_empty() {
        lines.push(format!(
            "- changed surface(s): {}",
            diagnostic.changed_surfaces.join(", ")
        ));
    }
    if !diagnostic.impact_relations.is_empty() {
        let relations = diagnostic
            .impact_relations
            .iter()
            .map(|relation| {
                format!("{} -> {}", relation.source, relation.target)
            })
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("- verified impact relation(s): {relations}"));
    }
    if !diagnostic.acceptance_criteria.is_empty() {
        lines.push(format!(
            "- acceptance criterion/criteria: {}",
            diagnostic.acceptance_criteria.join(", ")
        ));
    }
    lines.join("\n")
}

/// One stale-artifact diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhyStale {
    /// The stale artifact id.
    pub artifact_id: String,
    /// Staleness reason.
    pub reason: String,
}

/// Rendered stale diagnostic.
#[must_use]
pub fn render_why_stale(diagnostic: &WhyStale) -> String {
    format!("Stale because: {}", diagnostic.reason)
}

/// One blocked-operation diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhyBlocked {
    /// Blocking reason.
    pub reason: String,
}

/// Rendered blocked diagnostic.
#[must_use]
pub fn render_why_blocked(diagnostic: &WhyBlocked) -> String {
    format!("Blocked because: {}", diagnostic.reason)
}

/// One failed-acceptance diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhyAcceptanceFailed {
    /// The unsatisfied criterion.
    pub criterion_id: String,
    /// Required evidence classes that were missing.
    pub missing_evidence_classes: Vec<String>,
    /// Evidence identities considered for the decision.
    pub evidence_identities: Vec<String>,
}

/// Rendered failed-acceptance diagnostic.
#[must_use]
pub fn render_why_acceptance_failed(
    diagnostic: &WhyAcceptanceFailed,
) -> String {
    let missing = if diagnostic.missing_evidence_classes.is_empty() {
        "none".to_owned()
    } else {
        diagnostic.missing_evidence_classes.join(", ")
    };
    let considered = if diagnostic.evidence_identities.is_empty() {
        "none".to_owned()
    } else {
        diagnostic.evidence_identities.join(", ")
    };
    [
        format!(
            "Acceptance for {} not satisfied because:",
            diagnostic.criterion_id
        ),
        format!("- missing evidence class(es): {missing}"),
        format!("- considered evidence identities: {considered}"),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{
        ContextProvenanceRef, WhyAcceptanceFailed, WhyBlocked, WhyStale,
        compute_provenance_digest, create_context_provenance_ref,
        render_why_acceptance_failed, render_why_blocked, render_why_stale,
        render_why_validation_required, why_validation_required,
    };
    use crate::context::context_error;
    use crate::determinism::{ImpactRelationship, ValidationItem};

    fn reference(
        item: &str,
        kind: &str,
        id: &str,
        digest: Option<&str>,
    ) -> ContextProvenanceRef {
        create_context_provenance_ref(item, kind, id, digest)
            .expect("valid reference")
    }

    #[test]
    fn provenance_digest_is_order_insensitive_and_null_preserving() {
        let first = vec![
            reference(
                "ctx.plan",
                "adr",
                "ADR-0010",
                Some("d".repeat(64).as_str()),
            ),
            reference("ctx.plan", "evidence", "ev-1", None),
        ];
        let second = vec![
            reference("ctx.plan", "evidence", "ev-1", None),
            reference(
                "ctx.plan",
                "adr",
                "ADR-0010",
                Some("d".repeat(64).as_str()),
            ),
        ];
        let digest = compute_provenance_digest(&first).expect("digest");
        assert_eq!(digest.len(), 64);
        assert_eq!(
            digest,
            compute_provenance_digest(&second).expect("digest")
        );
        let different = vec![reference(
            "ctx.other",
            "adr",
            "ADR-0010",
            Some("d".repeat(64).as_str()),
        )];
        assert_ne!(
            digest,
            compute_provenance_digest(&different).expect("digest")
        );
    }

    #[test]
    fn references_require_item_and_source_id() {
        assert_eq!(
            create_context_provenance_ref("", "evidence", "ev-1", None),
            Err(context_error(
                "A provenance reference requires an item and a source id."
            ))
        );
        assert_eq!(
            create_context_provenance_ref("ctx.item", "evidence", "", None),
            Err(context_error(
                "A provenance reference requires an item and a source id."
            ))
        );
    }

    #[test]
    fn why_validation_required_finds_plan_items_only() {
        let items = vec![ValidationItem {
            id: "check-syntax".to_owned(),
            class:
                crate::determinism::decisions::ValidationRequirementClass::Required,
            rationale: "applies to changed surface(s)".to_owned(),
        }];
        let changed = vec!["src/a.rs".to_owned()];
        let relations = vec![ImpactRelationship {
            source: "src/a.rs".to_owned(),
            target: "src/b.rs".to_owned(),
        }];
        let criteria = vec!["AC-1".to_owned()];
        let found = why_validation_required(
            "check-syntax",
            &items,
            &changed,
            &relations,
            &criteria,
        )
        .expect("item is in the plan");
        assert_eq!(
            render_why_validation_required(&found),
            "Required because (validation_plan):\n\
             - changed surface(s): src/a.rs\n\
             - verified impact relation(s): src/a.rs -> src/b.rs\n\
             - acceptance criterion/criteria: AC-1"
        );
        assert!(
            why_validation_required(
                "missing", &items, &changed, &relations, &criteria,
            )
            .is_none()
        );
        let empty =
            why_validation_required("check-syntax", &items, &[], &[], &[])
                .expect("item is in the plan");
        assert_eq!(
            render_why_validation_required(&empty),
            "Required because (validation_plan):"
        );
    }

    #[test]
    fn rendered_diagnostics_match_the_oracle_templates() {
        assert_eq!(
            render_why_stale(&WhyStale {
                artifact_id: "derived".to_owned(),
                reason: "input changed".to_owned(),
            }),
            "Stale because: input changed"
        );
        assert_eq!(
            render_why_blocked(&WhyBlocked {
                reason: "no approval".to_owned(),
            }),
            "Blocked because: no approval"
        );
        assert_eq!(
            render_why_acceptance_failed(&WhyAcceptanceFailed {
                criterion_id: "AC-2".to_owned(),
                missing_evidence_classes: Vec::new(),
                evidence_identities: vec!["sha256:aa".to_owned()],
            }),
            "Acceptance for AC-2 not satisfied because:\n\
             - missing evidence class(es): none\n\
             - considered evidence identities: sha256:aa"
        );
    }
}
