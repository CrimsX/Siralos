import { computeArtifactDigest } from "../identity/artifact-digest.js";
import { createOrderingPolicy } from "../determinism/context.js";
import type { ValidationPlan } from "../determinism/decisions.js";

/**
 * Context provenance and why-diagnostics (Stage 3 — Interpretable
 * Context Architecture, ADR 0030).
 *
 * Important context items carry bounded provenance references; why-
 * diagnostics derive from structured provenance/evidence deterministically
 * — never by asking another LLM to reconstruct reasoning.
 */

export type ProvenanceSourceKind =
  | "execution_contract"
  | "adr"
  | "acceptance_criterion"
  | "impact_relation"
  | "evidence"
  | "touchpoint"
  | "phase_contract"
  | "validation_plan";

export interface ContextProvenanceRef {
  readonly item: string;
  readonly source: {
    readonly kind: ProvenanceSourceKind;
    readonly id: string;
    readonly digest: string | null;
  };
}

export function createContextProvenanceRef(input: {
  readonly item: string;
  readonly kind: ProvenanceSourceKind;
  readonly id: string;
  readonly digest?: string | null;
}): ContextProvenanceRef {
  if (input.item.length === 0 || input.id.length === 0) {
    throw new Error("A provenance reference requires an item and a source id.");
  }
  return {
    item: input.item,
    source: { kind: input.kind, id: input.id, digest: input.digest ?? null },
  };
}

/** Bounded deterministic digest over a provenance set (evidence identity). */
export function computeProvenanceDigest(refs: readonly ContextProvenanceRef[]): string {
  const ordered = createOrderingPolicy().stableSort(
    refs.map((ref) => ({ ...ref, source: { ...ref.source } })),
    (ref) => `${ref.item}:${ref.source.kind}:${ref.source.id}`,
  );
  return computeArtifactDigest({
    artifactType: "ContextProvenance",
    schemaVersion: 1,
    payload: { refs: ordered },
  }).value;
}

// ---------------------------------------------------------------------------
// Why-diagnostics
// ---------------------------------------------------------------------------

export interface WhyValidationRequired {
  readonly itemId: string;
  readonly changedSurfaces: readonly string[];
  readonly impactRelations: readonly { readonly source: string; readonly target: string }[];
  readonly acceptanceCriteria: readonly string[];
  readonly source: "validation_plan";
}

/**
 * Deterministic answer for "why is this validation required" derived
 * from the ValidationPlan rationale (never a model invocation).
 */
export function whyValidationRequired(input: {
  readonly itemId: string;
  readonly plan: ValidationPlan;
  readonly changedSurfaces: readonly string[];
  readonly impactRelations: readonly { readonly source: string; readonly target: string }[];
  readonly acceptanceCriteria: readonly string[];
}): WhyValidationRequired | null {
  const item = input.plan.items.find((entry) => entry.id === input.itemId);
  if (item === undefined) {
    return null;
  }
  return {
    itemId: input.itemId,
    changedSurfaces: [...input.changedSurfaces],
    impactRelations: input.impactRelations.map((relation) => ({ ...relation })),
    acceptanceCriteria: [...input.acceptanceCriteria],
    source: "validation_plan",
  };
}

/** Rendered why-diagnostic (bounded, human-readable). */
export function renderWhyValidationRequired(diagnostic: WhyValidationRequired): string {
  const lines: string[] = [`Required because (${diagnostic.source}):`];
  if (diagnostic.changedSurfaces.length > 0) {
    lines.push(`- changed surface(s): ${diagnostic.changedSurfaces.join(", ")}`);
  }
  if (diagnostic.impactRelations.length > 0) {
    lines.push(
      `- verified impact relation(s): ${diagnostic.impactRelations
        .map((relation) => `${relation.source} -> ${relation.target}`)
        .join(", ")}`,
    );
  }
  if (diagnostic.acceptanceCriteria.length > 0) {
    lines.push(`- acceptance criterion/criteria: ${diagnostic.acceptanceCriteria.join(", ")}`);
  }
  return lines.join("\n");
}

export interface WhyStale {
  readonly artifactId: string;
  readonly reason: string;
}

export function renderWhyStale(diagnostic: WhyStale): string {
  return `Stale because: ${diagnostic.reason}`;
}

export interface WhyBlocked {
  readonly reason: string;
}

export function renderWhyBlocked(diagnostic: WhyBlocked): string {
  return `Blocked because: ${diagnostic.reason}`;
}

export interface WhyAcceptanceFailed {
  readonly criterionId: string;
  readonly missingEvidenceClasses: readonly string[];
  readonly evidenceIdentities: readonly string[];
}

export function renderWhyAcceptanceFailed(diagnostic: WhyAcceptanceFailed): string {
  const lines = [
    `Acceptance for ${diagnostic.criterionId} not satisfied because:`,
    `- missing evidence class(es): ${diagnostic.missingEvidenceClasses.join(", ") || "none"}`,
    `- considered evidence identities: ${diagnostic.evidenceIdentities.join(", ") || "none"}`,
  ];
  return lines.join("\n");
}
