import { computeArtifactDigest } from "../identity/artifact-digest.js";
import { createOrderingPolicy } from "../determinism/context.js";

/**
 * Source-integrity signals (Stage 3 — Interpretable Context
 * Architecture, ADR 0030).
 *
 * Groundwork for identifying repeated downstream corrections that likely
 * originate from an upstream instruction/configuration problem. This
 * milestone RECORDS and CLASSIFIES the pattern and exposes evidence — it
 * never modifies source guidance automatically (controlled self-
 * improvement belongs to Stage 6 `/evolve`).
 */

export type CorrectionPatternKind =
  | "repeated_architecture_finding"
  | "repeated_file_pattern_rejected"
  | "repeated_validation_omitted";

export interface CorrectionPattern {
  readonly patternId: string;
  readonly kind: CorrectionPatternKind;
  /** Occurrence count observed so far (bounded). */
  readonly occurrences: number;
  /** Evidence references for the occurrences (bounded). */
  readonly evidenceRefs: readonly string[];
}

export function recordCorrectionPattern(input: {
  readonly patternId: string;
  readonly kind: CorrectionPatternKind;
  readonly evidenceRef: string;
}): CorrectionPattern {
  return {
    patternId: input.patternId,
    kind: input.kind,
    occurrences: 1,
    evidenceRefs: [input.evidenceRef],
  };
}

/** Accumulate a repeated pattern (bounded evidence). */
export function accumulateCorrectionPattern(
  pattern: CorrectionPattern,
  evidenceRef: string,
  maxOccurrences = 32,
  maxEvidenceRefs = 8,
): CorrectionPattern {
  return {
    ...pattern,
    occurrences: Math.min(pattern.occurrences + 1, maxOccurrences),
    evidenceRefs: [...pattern.evidenceRefs, evidenceRef].slice(-maxEvidenceRefs),
  };
}

export type SourceProblemClass =
  | "instruction"
  | "architecture_documentation"
  | "model_profile"
  | "validation_rule"
  | "workflow_contract"
  | "skill";

export interface SourceProblemCandidate {
  readonly id: string;
  readonly likelySourceClass: SourceProblemClass;
  readonly supportingPatterns: readonly CorrectionPattern[];
  readonly createdAtMs: number;
  /** Recording-only: automatic remediation is never performed. */
  readonly remediated: false;
}

export function createSourceProblemCandidate(input: {
  readonly id: string;
  readonly likelySourceClass: SourceProblemClass;
  readonly supportingPatterns: readonly CorrectionPattern[];
  readonly createdAtMs: number;
}): SourceProblemCandidate {
  if (input.id.length === 0) {
    throw new Error("A source-problem candidate requires an id.");
  }
  if (input.supportingPatterns.length === 0) {
    throw new Error("A source-problem candidate requires supporting evidence patterns.");
  }
  return {
    id: input.id,
    likelySourceClass: input.likelySourceClass,
    supportingPatterns: input.supportingPatterns.map((pattern) => ({
      ...pattern,
      evidenceRefs: [...pattern.evidenceRefs],
    })),
    createdAtMs: input.createdAtMs,
    remediated: false,
  };
}

/** Deterministic digest over a candidate (evidence identity). */
export function computeSourceProblemCandidateDigest(candidate: SourceProblemCandidate): string {
  const ordered = createOrderingPolicy().stableSort(
    candidate.supportingPatterns.map((pattern) => ({
      ...pattern,
      evidenceRefs: [...pattern.evidenceRefs].sort(),
    })),
    (pattern) => pattern.patternId,
  );
  return computeArtifactDigest({
    artifactType: "SourceProblemCandidate",
    schemaVersion: 1,
    payload: {
      id: candidate.id,
      likelySourceClass: candidate.likelySourceClass,
      patterns: ordered,
    },
  }).value;
}

/** Human-readable projection (recording-only view). */
export function renderSourceProblemCandidate(candidate: SourceProblemCandidate): string {
  const patterns = candidate.supportingPatterns
    .map((pattern) => `${pattern.kind} x${pattern.occurrences}`)
    .join(", ");
  return `SourceProblemCandidate ${candidate.id} (likely ${candidate.likelySourceClass}): ${patterns} — recorded, not remediated.`;
}
