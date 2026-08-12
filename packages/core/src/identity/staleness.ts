/**
 * Bounded dependency-aware staleness (Stage 3 — Content Identity & Delta
 * Verification, ADR 0028).
 *
 * Explicit high-value dependency rules, never a generic reactive
 * dependency graph:
 *
 *   TaskContract digest changes            → TaskPlan potentially stale
 *   Guidance digest changes                → plan/executor context potentially stale
 *   changeset digest changes               → review stale
 *   validation evidence digest changes     → acceptance may require reevaluation
 *
 * Rules are pure functions over exact digests; the authoritative state
 * (TaskState phases, approval records, evidence) remains the owner of
 * any actual invalidation.
 */

export interface IdentityStalenessInput {
  readonly contractDigest?: string | null;
  readonly planContractDigest?: string | null;
  readonly guidanceDigest?: string | null;
  readonly priorGuidanceDigest?: string | null;
  readonly changesetDigest?: string | null;
  readonly reviewInputChangesetDigest?: string | null;
  readonly validationEvidenceDigest?: string | null;
  readonly acceptedEvidenceDigest?: string | null;
}

export interface IdentityStaleness {
  /** The TaskContract content changed since the plan was created. */
  readonly planPotentiallyStale: boolean;
  /** The guidance content changed since planning/execution context was built. */
  readonly executionContextPotentiallyStale: boolean;
  /** The reviewed changeset content changed since the last review. */
  readonly reviewStale: boolean;
  /** The validation evidence changed since the last acceptance evaluation. */
  readonly acceptanceRequiresReevaluation: boolean;
  readonly reasons: readonly string[];
}

export function deriveIdentityStaleness(input: IdentityStalenessInput): IdentityStaleness {
  const reasons: string[] = [];
  const planPotentiallyStale =
    input.contractDigest !== null &&
    input.contractDigest !== undefined &&
    input.planContractDigest !== null &&
    input.planContractDigest !== undefined &&
    input.contractDigest !== input.planContractDigest;
  if (planPotentiallyStale) {
    reasons.push(
      "TaskContract content digest changed since the plan was created; the plan is potentially stale.",
    );
  }
  const executionContextPotentiallyStale =
    input.guidanceDigest !== null &&
    input.guidanceDigest !== undefined &&
    input.priorGuidanceDigest !== null &&
    input.priorGuidanceDigest !== undefined &&
    input.guidanceDigest !== input.priorGuidanceDigest;
  if (executionContextPotentiallyStale) {
    reasons.push("Guidance digest changed; planning/execution context may be stale.");
  }
  const reviewStale =
    input.changesetDigest !== null &&
    input.changesetDigest !== undefined &&
    input.reviewInputChangesetDigest !== null &&
    input.reviewInputChangesetDigest !== undefined &&
    input.changesetDigest !== input.reviewInputChangesetDigest;
  if (reviewStale) {
    reasons.push("The reviewed changeset content changed; the previous review no longer applies.");
  }
  const acceptanceRequiresReevaluation =
    input.validationEvidenceDigest !== null &&
    input.validationEvidenceDigest !== undefined &&
    input.acceptedEvidenceDigest !== null &&
    input.acceptedEvidenceDigest !== undefined &&
    input.validationEvidenceDigest !== input.acceptedEvidenceDigest;
  if (acceptanceRequiresReevaluation) {
    reasons.push(
      "Validation evidence changed; acceptance must be reevaluated against the current evidence set.",
    );
  }
  return {
    planPotentiallyStale,
    executionContextPotentiallyStale,
    reviewStale,
    acceptanceRequiresReevaluation,
    reasons,
  };
}
