import type { ChangeReviewResult } from "./quality-review.js";

/**
 * Provider-neutral GDScript development quality model (ADR 0013).
 *
 * The quality stage sits between the existing validation gates (parser,
 * fresh-LSP diagnostics, workspace integrity) and development completion.
 * It separates DETERMINISTIC quality gates — measurable conditions the
 * application itself computes (scope, parser, LSP errors, required
 * validation, warning deltas, conventions, diff metrics) — from the
 * MODEL-BASED INDEPENDENT REVIEW, an additional reasoning signal that can
 * never replace a deterministic gate and can never weaken one. Core owns
 * the vocabulary, the immutable limits, the deterministic status mapping,
 * and the UI-neutral events; the adapters own the orchestration and the
 * reviewer implementations.
 */

/** Fixed deterministic quality-gate ids (§9). */
export type QualityGateId =
  | "approved-change-applied"
  | "checkpoint-recorded"
  | "scope-verified"
  | "parser"
  | "lsp-errors"
  | "required-validation"
  | "independent-review"
  | "warnings"
  | "conventions"
  | "diff-metrics";

/** Hard gates block clean completion; soft gates advise; informational gates only record evidence. */
export type QualityGateClassification = "hard" | "soft" | "informational";

export type QualityGateStatus =
  "passed" | "advisory" | "blocked" | "not_applicable" | "not_run" | "failed";

/** Bounded evidence entry of one gate; never contains hidden model reasoning. */
export interface QualityEvidence {
  readonly kind: string;
  readonly summary: string;
  /** Bounded optional detail (for example a diagnostic code or finding id). */
  readonly detail?: string;
}

export interface QualityGateResult {
  readonly id: QualityGateId;
  readonly classification: QualityGateClassification;
  readonly status: QualityGateStatus;
  readonly summary: string;
  readonly evidence: readonly QualityEvidence[];
}

/**
 * Final quality-report statuses (§38). These are not collapsed into a
 * single success/failure: `validation_incomplete` means a required gate
 * could not run (denied or infrastructure-unavailable), `blocking_findings`
 * means the independent review found evidence-backed Critical/High issues,
 * and `failed` means a deterministic hard gate failed.
 */
export type QualityStatus =
  | "passed"
  | "passed_with_advisories"
  | "blocking_findings"
  | "validation_incomplete"
  | "failed"
  | "cancelled";

/**
 * Bounded change metrics surfaced to the independent reviewer (§18). No
 * arbitrary hard maximum rejects legitimate tasks; the metrics are review
 * evidence so a large unexpected expansion relative to the request is
 * visible.
 */
export interface ChangeDiffMetrics {
  readonly filesChanged: number;
  readonly linesAdded: number;
  readonly linesRemoved: number;
  readonly filesCreated: number;
  readonly filesDeleted: number;
  /** Functions touched where determinable from added GDScript lines; null otherwise. */
  readonly functionsTouched: number | null;
}

/** Immutable quality-stage limits (§52). Provider input can never raise them. */
export const QUALITY_LIMITS = {
  /** Maximum review findings in one result (§52). */
  maxReviewFindings: 50,
  /** Maximum characters of one finding's evidence field (§52). */
  maxFindingEvidenceChars: 4096,
  /** Maximum characters of one finding's recommendation field (§52). */
  maxFindingRecommendationChars: 4096,
  /** Maximum characters of one finding's impact field. */
  maxFindingImpactChars: 4096,
  /** Maximum characters of one finding's title field. */
  maxFindingTitleChars: 256,
  /** Maximum characters of one finding's path field. */
  maxFindingPathChars: 4096,
  /** Maximum UTF-8 bytes of the review context diff (complete changed files) (§52). */
  maxReviewContextDiffBytes: 1024 * 1024,
  /** Maximum review rounds per development workflow (initial + repairs) (§52). */
  maxReviewRounds: 3,
  /** Maximum repair change sets triggered by review findings (§34). */
  maxReviewRepairRounds: 2,
  /** Review timeout (§52). */
  reviewTimeoutMs: 120_000,
  /** Conservative line-movement tolerance for warning-delta attribution (§11). */
  warningLineTolerance: 30,
  /** Newly introduced lines longer than this are a convention advisory. */
  longLineChars: 160,
  /** Maximum convention findings in one analysis. */
  maxConventionFindings: 100,
  /** Maximum warning-delta entries in one gate result. */
  maxWarningDeltaEntries: 200,
  /** Maximum evidence entries per gate result. */
  maxEvidenceEntriesPerGate: 20,
} as const;

/** UI-neutral quality-stage events (§48); no general event bus. */
export type QualityEvent =
  | {
      readonly type: "quality_started";
      readonly developmentId: string;
    }
  | {
      readonly type: "quality_gate_completed";
      readonly developmentId: string;
      readonly gateId: QualityGateId;
      readonly status: QualityGateStatus;
    }
  | {
      readonly type: "review_started";
      readonly developmentId: string;
    }
  | {
      readonly type: "review_completed";
      readonly developmentId: string;
      readonly critical: number;
      readonly high: number;
      readonly medium: number;
      readonly low: number;
    }
  | {
      readonly type: "quality_completed";
      readonly developmentId: string;
      readonly status: QualityStatus;
    };

/** Bounded reviewer summary stored in the report; never hidden reasoning. */
export interface IndependentReviewResult {
  readonly status: ChangeReviewResult["status"];
  readonly findings: ChangeReviewResult["findings"];
  readonly blockingCount: number;
  readonly message: string | null;
}

/**
 * Final quality report (§7, §37). Every gate carries evidence; the review
 * carries only structured findings. The report never stores credentials,
 * mirror paths, primary-provider reasoning, or approval internals.
 */
export interface DevelopmentQualityReport {
  readonly developmentId: string;
  readonly status: QualityStatus;
  readonly gates: readonly QualityGateResult[];
  readonly review: IndependentReviewResult | null;
  readonly repairRoundsUsed: number;
  readonly maxRepairRounds: number;
  readonly reviewRoundsUsed: number;
  readonly maxReviewRounds: number;
  /** Finding ids of previous review rounds, for re-review traceability (§35). */
  readonly previousFindingIds: readonly string[];
  readonly completedAtMs: number;
}

/** Deterministic gate classification (§8). */
export function gateClassification(id: QualityGateId): QualityGateClassification {
  switch (id) {
    case "approved-change-applied":
    case "checkpoint-recorded":
    case "scope-verified":
    case "parser":
    case "lsp-errors":
    case "required-validation":
    case "independent-review":
      return "hard";
    case "warnings":
    case "conventions":
      return "soft";
    case "diff-metrics":
      return "informational";
  }
}

export function createQualityGateResult(
  id: QualityGateId,
  status: QualityGateStatus,
  summary: string,
  evidence: readonly QualityEvidence[] = [],
): QualityGateResult {
  return {
    id,
    classification: gateClassification(id),
    status,
    summary,
    evidence: evidence.slice(0, QUALITY_LIMITS.maxEvidenceEntriesPerGate),
  };
}

/**
 * Deterministic report-status mapping (§38). Order matters:
 *
 * 1. A cancelled review invalidates the stage: `cancelled`.
 * 2. A review that failed, timed out, or was too large, or a required
 *    validation step that was denied or could not run, makes validation
 *    incomplete — never passed.
 * 3. A blocked deterministic hard gate (other than the review gate itself)
 *    is a quality failure.
 * 4. Blocking review findings make the report `blocking_findings`.
 * 5. Remaining advisories (soft gates, non-blocking findings) yield
 *    `passed_with_advisories`; otherwise `passed`.
 */
export function computeQualityReportStatus(
  gates: readonly QualityGateResult[],
  review: IndependentReviewResult | null,
): QualityStatus {
  if (review !== null) {
    if (review.status === "cancelled") {
      return "cancelled";
    }
    if (review.status === "failed" || review.status === "too_large") {
      return "validation_incomplete";
    }
  }
  for (const gate of gates) {
    if (gate.id === "independent-review") {
      continue;
    }
    if (
      gate.id === "required-validation" &&
      gate.status === "not_run" &&
      gate.evidence.some(
        (entry) =>
          entry.kind === "validation-denied" ||
          entry.kind === "validation-unavailable" ||
          entry.kind === "validation-not-run",
      )
    ) {
      return "validation_incomplete";
    }
    if (gate.status === "blocked" || gate.status === "failed") {
      return "failed";
    }
  }
  if (review !== null && review.status === "completed" && review.blockingCount > 0) {
    return "blocking_findings";
  }
  if (
    review !== null &&
    review.status === "completed" &&
    review.findings.length > review.blockingCount
  ) {
    return "passed_with_advisories";
  }
  for (const gate of gates) {
    if (gate.status === "advisory") {
      return "passed_with_advisories";
    }
  }
  return "passed";
}
