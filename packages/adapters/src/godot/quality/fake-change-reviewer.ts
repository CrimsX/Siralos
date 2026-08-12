import {
  deterministicFindingId,
  type ChangeReviewFinding,
  type ChangeReviewRequest,
  type ChangeReviewResult,
  type ChangeReviewer,
} from "@siralos/core";

/**
 * Deterministic fake change reviewer for tests (ADR 0013 §65). Scenarios:
 * `clean`, `medium` (advisory finding), `high` (blocking correctness
 * finding), `malformed` (failed output), `timeout` (failed after the
 * injected tick), `duplicate` (two identical findings collapse), and
 * `repair-resolved` / `new-after-repair` for the review-repair cycle.
 * No model API is ever called and no test depends on the network.
 */

export type FakeReviewerScenario =
  | "clean"
  | "medium"
  | "high"
  | "malformed"
  | "timeout"
  | "duplicate"
  | "cancelled"
  | "repair-resolved"
  | "new-after-repair";

export interface FakeChangeReviewerOptions {
  readonly scenario: FakeReviewerScenario;
  /** The first review round returns the scenario; later rounds resolve. */
  readonly resolveAfterRounds?: number;
  /** Deterministic tick for the timeout scenario (tests inject an immediate tick). */
  readonly tick?: () => Promise<void>;
}

export interface FakeReviewerControl {
  readonly reviews: readonly ChangeReviewRequest[];
}

export function createFakeChangeReviewer(options: FakeChangeReviewerOptions): {
  readonly reviewer: ChangeReviewer;
  readonly control: FakeReviewerControl;
} {
  const reviews: ChangeReviewRequest[] = [];
  let round = 0;
  const reviewer: ChangeReviewer = {
    async review(request: ChangeReviewRequest, signal?: AbortSignal): Promise<ChangeReviewResult> {
      if (signal?.aborted) {
        return { status: "cancelled", findings: [], message: "review cancelled" };
      }
      reviews.push(request);
      round += 1;
      const resolving =
        options.resolveAfterRounds !== undefined && round > options.resolveAfterRounds;
      const activeScenario = resolving ? "clean" : options.scenario;
      switch (activeScenario) {
        case "clean":
          return { status: "completed", findings: [], message: null };
        case "medium":
          return {
            status: "completed",
            findings: [
              makeFinding(request, {
                severity: "medium",
                category: "maintainability",
                title: "helper function used only once",
                confidence: "medium",
                evidence: "the helper is called from a single site",
                impact: "minor maintainability concern",
                recommendation:
                  "inline the helper or keep it if the project prefers small functions",
              }),
            ],
            message: null,
          };
        case "high":
          return {
            status: "completed",
            findings: [
              makeFinding(request, {
                severity: "high",
                category: "correctness",
                title: "health can exceed max_health",
                confidence: "high",
                evidence: "heal() adds the amount without clamping the result to max_health",
                impact: "the player can heal beyond the intended maximum",
                recommendation: "clamp the result to max_health",
              }),
            ],
            message: null,
          };
        case "malformed":
          return {
            status: "failed",
            findings: [],
            message: "the reviewer returned malformed output",
          };
        case "timeout": {
          await options.tick?.();
          return { status: "failed", findings: [], message: "the review timed out" };
        }
        case "duplicate":
          return {
            status: "completed",
            findings: [
              makeFinding(request, {
                severity: "low",
                category: "style",
                title: "duplicate style note",
                confidence: "high",
                evidence: "a style observation repeated by the reviewer",
                impact: "none; the observation is duplicated",
                recommendation: "no action",
              }),
              makeFinding(request, {
                severity: "low",
                category: "style",
                title: "duplicate style note",
                confidence: "high",
                evidence: "the same observation appears twice",
                impact: "none",
                recommendation: "no action",
              }),
            ],
            message: null,
          };
        case "cancelled":
          return { status: "cancelled", findings: [], message: "the review was cancelled" };
        case "repair-resolved":
          // The first review finds a blocker; a later review resolves it
          // when the request's previous finding ids prove the repair ran.
          return {
            status: "completed",
            findings:
              request.previousFindingIds.length === 0
                ? [
                    makeFinding(request, {
                      severity: "high",
                      category: "correctness",
                      title: "repair required",
                      confidence: "high",
                      evidence: "the fixture repair scenario requires one blocking round",
                      impact: "clean completion must not be claimed",
                      recommendation: "apply the approved repair and revalidate",
                    }),
                  ]
                : [],
            message: null,
          };
        case "new-after-repair":
          return {
            status: "completed",
            findings:
              request.previousFindingIds.length === 0
                ? [
                    makeFinding(request, {
                      severity: "high",
                      category: "correctness",
                      title: "first blocker",
                      confidence: "high",
                      evidence: "the fixture repair scenario requires one blocking round",
                      impact: "clean completion must not be claimed",
                      recommendation: "apply the approved repair and revalidate",
                    }),
                  ]
                : [
                    makeFinding(request, {
                      severity: "high",
                      category: "correctness",
                      title: "second blocker after repair",
                      confidence: "high",
                      evidence: "a fresh holistic review found a new issue",
                      impact: "the repaired change still has a blocking issue",
                      recommendation: "apply a focused repair",
                    }),
                  ],
            message: null,
          };
      }
    },
  };
  return { reviewer, control: { reviews } };
}

function makeFinding(
  request: ChangeReviewRequest,
  parts: {
    readonly severity: ChangeReviewFinding["severity"];
    readonly category: ChangeReviewFinding["category"];
    readonly title: string;
    readonly confidence: ChangeReviewFinding["confidence"];
    readonly evidence: string;
    readonly impact: string;
    readonly recommendation: string;
  },
): ChangeReviewFinding {
  const path = request.changedPaths[0] ?? null;
  return {
    id: deterministicFindingId({ category: parts.category, path, line: 1, title: parts.title }),
    severity: parts.severity,
    category: parts.category,
    title: parts.title,
    path,
    line: 1,
    evidence: parts.evidence,
    impact: parts.impact,
    recommendation: parts.recommendation,
    confidence: parts.confidence,
  };
}
