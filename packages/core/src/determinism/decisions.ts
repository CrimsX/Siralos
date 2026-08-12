import { computeArtifactDigest } from "../identity/artifact-digest.js";
import { createOrderingPolicy } from "./context.js";

/**
 * Deterministic decision components (Stage 3 — Deterministic Execution &
 * Reproducibility, ADR 0029): host-derived validation plans, acceptance
 * evaluation, typed retry policy, and concurrency normalization. Same
 * authoritative inputs → same host decision.
 */

// ---------------------------------------------------------------------------
// Deterministic validation plan
// ---------------------------------------------------------------------------

export type ValidationRequirementClass = "required" | "recommended" | "unavailable";

export interface ValidationItem {
  readonly id: string;
  readonly class: ValidationRequirementClass;
  /** Rationale binding the item to actual changed surfaces/evidence. */
  readonly rationale: string;
}

export interface ValidationPlan {
  readonly items: readonly ValidationItem[];
  /** Canonical digest over the ordered plan. */
  readonly digest: string;
}

export interface ValidationPlanInput {
  readonly changedSurfaces: readonly string[];
  /** Verified impact relationships (path pairs). */
  readonly impactRelationships: readonly { readonly source: string; readonly target: string }[];
  readonly acceptanceCriteria: readonly {
    readonly id: string;
    readonly verificationKind: string;
  }[];
  /** Registry of known validation checks (id + applicable surface glob/kind). */
  readonly validationRegistry: readonly {
    readonly id: string;
    readonly appliesTo: readonly string[];
    readonly baseClass: ValidationRequirementClass;
  }[];
}

/**
 * Deterministic minimum validation selection: required checks are derived
 * from actual changed surfaces and verified impact relationships, never
 * from model preference. The model may recommend additional validation,
 * but may not remove host-required validation.
 */
export function deriveValidationPlan(input: ValidationPlanInput): ValidationPlan {
  const ordering = createOrderingPolicy();
  const items: ValidationItem[] = [];
  const seen = new Set<string>();
  const impacted = new Set<string>(input.changedSurfaces);
  for (const relationship of input.impactRelationships) {
    impacted.add(relationship.source);
    impacted.add(relationship.target);
  }
  for (const check of [...input.validationRegistry].sort((a, b) =>
    a.id < b.id ? -1 : a.id > b.id ? 1 : 0,
  )) {
    if (seen.has(check.id)) {
      continue;
    }
    const applies = check.appliesTo.some(
      (surface) =>
        input.changedSurfaces.includes(surface) ||
        [...impacted].some((path) => path.endsWith(surface) || surface.endsWith(path)),
    );
    if (!applies) {
      continue;
    }
    seen.add(check.id);
    const classFor = (): ValidationRequirementClass => {
      if (check.baseClass === "unavailable") {
        return "unavailable";
      }
      if (
        input.acceptanceCriteria.some((criterion) => criterion.verificationKind === "deterministic")
      ) {
        return check.baseClass;
      }
      return check.baseClass;
    };
    items.push({
      id: check.id,
      class: classFor(),
      rationale: `applies to changed surface(s): ${check.appliesTo.join(", ")}`,
    });
  }
  const ordered = ordering.stableSort(items, (item) => item.id);
  const digest = computeArtifactDigest({
    artifactType: "ValidationPlan",
    schemaVersion: 1,
    payload: { items: ordered },
  }).value;
  return { items: ordered, digest };
}

// ---------------------------------------------------------------------------
// Deterministic acceptance
// ---------------------------------------------------------------------------

export type AcceptanceOutcome = "satisfied" | "not_satisfied" | "unverifiable";

export interface AcceptanceResult {
  readonly criterionId: string;
  readonly outcome: AcceptanceOutcome;
  /** Evidence identities (digests) the decision was based on. */
  readonly evidenceIdentities: readonly string[];
  /** Canonical digest over the ordered decision inputs. */
  readonly digest: string;
}

export interface AcceptanceInput {
  readonly criterionId: string;
  /** Required evidence classes for this criterion (e.g. ["parser_result"]). */
  readonly requiredEvidenceClasses: readonly string[];
  /** Available evidence (class + content digest), any insertion order. */
  readonly availableEvidence: readonly {
    readonly id: string;
    readonly class: string;
    readonly digest: string;
  }[];
}

/** Deterministic acceptance: same evidence classes + identities → same result. */
export function evaluateAcceptance(input: AcceptanceInput): AcceptanceResult {
  const ordering = createOrderingPolicy();
  const normalized = ordering.stableSort(
    [...input.availableEvidence].map((entry) => ({ ...entry })),
    (entry) => entry.id,
  );
  const classPresent = new Set(normalized.map((entry) => entry.class));
  const outcome: AcceptanceOutcome = input.requiredEvidenceClasses.every((required) =>
    classPresent.has(required),
  )
    ? "satisfied"
    : input.requiredEvidenceClasses.length === 0
      ? "unverifiable"
      : "not_satisfied";
  const evidenceIdentities = normalized.map((entry) => entry.digest);
  const digest = computeArtifactDigest({
    artifactType: "AcceptanceResult",
    schemaVersion: 1,
    payload: {
      criterionId: input.criterionId,
      outcome,
      evidenceIdentities,
      requiredEvidenceClasses: [...input.requiredEvidenceClasses].sort(),
    },
  }).value;
  return { criterionId: input.criterionId, outcome, evidenceIdentities, digest };
}

// ---------------------------------------------------------------------------
// Typed retry policy
// ---------------------------------------------------------------------------

export type RetryDecision = "retry" | "repair" | "no_retry";
export type RetryCategory =
  | "transient_provider_transport"
  | "stale_source_revision"
  | "blocking_review_finding"
  | "malformed_tool_representation"
  | "approval_denied"
  | "infrastructure_unavailable"
  | "validation_failed"
  | "unknown";

export interface RetryPolicy {
  readonly attemptLimit: number;
  /** Deterministic backoff schedule in ms (per attempt index, 0-based). */
  readonly backoffMs: (attemptIndex: number) => number;
}

export const DEFAULT_RETRY_POLICY: RetryPolicy = {
  attemptLimit: 3,
  backoffMs: (attemptIndex) => Math.min(100 * 2 ** attemptIndex, 2_000),
};

/** Host-owned retry classification: the model never decides retry counts. */
export function classifyRetry(
  category: RetryCategory,
  attemptsUsed: number,
  policy: RetryPolicy = DEFAULT_RETRY_POLICY,
): {
  readonly decision: RetryDecision;
  readonly reason: string;
  readonly nextBackoffMs: number | null;
} {
  switch (category) {
    case "transient_provider_transport":
      if (attemptsUsed < policy.attemptLimit) {
        return {
          decision: "retry",
          reason: `Transient provider transport failure; bounded retry ${attemptsUsed + 1}/${policy.attemptLimit}.`,
          nextBackoffMs: policy.backoffMs(attemptsUsed),
        };
      }
      return {
        decision: "no_retry",
        reason: `Retry budget exhausted (${policy.attemptLimit}); the operation ends failed.`,
        nextBackoffMs: null,
      };
    case "malformed_tool_representation":
      return {
        decision: "repair",
        reason: "Malformed tool representation; a bounded repair attempt is the host response.",
        nextBackoffMs: null,
      };
    case "blocking_review_finding":
      return {
        decision: "repair",
        reason: "Blocking review finding; the existing bounded repair loop applies.",
        nextBackoffMs: null,
      };
    case "stale_source_revision":
      return {
        decision: "no_retry",
        reason:
          "Stale source revision: no automatic mutation retry under the old approval; re-prepare from the current revision.",
        nextBackoffMs: null,
      };
    case "approval_denied":
      return {
        decision: "no_retry",
        reason: "Approval denied: the operation ends without retry.",
        nextBackoffMs: null,
      };
    case "infrastructure_unavailable":
      return {
        decision: "no_retry",
        reason: "Infrastructure unavailable: fail closed, never retry into an unsafe state.",
        nextBackoffMs: null,
      };
    case "validation_failed":
      return {
        decision: "no_retry",
        reason:
          "Validation failed: the result is reported failed; repairs require fresh preparation.",
        nextBackoffMs: null,
      };
    default:
      return {
        decision: "no_retry",
        reason: "Unclassified failure: no automatic retry.",
        nextBackoffMs: null,
      };
  }
}

// ---------------------------------------------------------------------------
// Concurrency normalization
// ---------------------------------------------------------------------------

/**
 * Parallel reads/evidence collection may finish in arbitrary order.
 * Before results affect authoritative decisions or provider context:
 * collect → validate → normalize → stable order → consume. Completion
 * order never equals semantic priority.
 */
export function normalizeConcurrentResults<T extends { readonly id: string }>(
  results: readonly T[],
): T[] {
  return createOrderingPolicy().stableSort(results, (entry) => entry.id);
}

// ---------------------------------------------------------------------------
// Deterministic working set and lease evaluation
// ---------------------------------------------------------------------------

export interface ActiveWorkingSetEntry {
  readonly path: string;
  /** Inclusion reason from the deterministic discovery classes. */
  readonly reason: string;
}

/**
 * Deterministic initial active working set: derived from the ordered
 * discovery result (never from the model). Equivalent inputs produce the
 * same set.
 */
export function deriveActiveWorkingSet(
  orderedCandidates: readonly { readonly path: string; readonly relevance: string }[],
  maxEntries: number,
): ActiveWorkingSetEntry[] {
  return orderedCandidates.slice(0, maxEntries).map((candidate) => ({
    path: candidate.path,
    reason:
      candidate.relevance === "verified"
        ? "verified discovery candidate"
        : "candidate discovery candidate",
  }));
}

/** Deterministic lease/expiry evaluation: same clock input → same decision. */
export function evaluateLease(
  lease: { readonly issuedAtMs: number; readonly ttlMs: number },
  nowMs: number,
): { readonly valid: boolean; readonly remainingMs: number } {
  const remainingMs = lease.issuedAtMs + lease.ttlMs - nowMs;
  return { valid: remainingMs > 0, remainingMs };
}
