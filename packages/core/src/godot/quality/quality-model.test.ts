import { describe, expect, it } from "vitest";
import {
  QUALITY_LIMITS,
  computeQualityReportStatus,
  createQualityGateResult,
  gateClassification,
  type DevelopmentQualityReport,
  type QualityGateResult,
  type QualityStatus,
} from "./quality-model.js";

function gate(id: "scope-verified" | "parser" | "lsp-errors" | "required-validation" | "independent-review" | "warnings" | "conventions", status: QualityGateResult["status"]): QualityGateResult {
  return createQualityGateResult(id, status, `gate ${id}`);
}

function cleanReview(blockingCount: number): DevelopmentQualityReport["review"] {
  return { status: "completed", findings: [], blockingCount, message: null };
}

describe("quality-gate classification", () => {
  it("classifies scope, parser, LSP, validation, and review gates as hard", () => {
    for (const id of [
      "approved-change-applied",
      "checkpoint-recorded",
      "scope-verified",
      "parser",
      "lsp-errors",
      "required-validation",
      "independent-review",
    ] as const) {
      expect(gateClassification(id)).toBe("hard");
    }
  });

  it("classifies warnings and conventions as soft", () => {
    expect(gateClassification("warnings")).toBe("soft");
    expect(gateClassification("conventions")).toBe("soft");
  });

  it("classifies diff metrics as informational", () => {
    expect(gateClassification("diff-metrics")).toBe("informational");
  });

  it("bounds gate evidence entries", () => {
    const evidence = Array.from({ length: 100 }, (_, index) => ({
      kind: "e",
      summary: `entry ${index}`,
    }));
    const result = createQualityGateResult("scope-verified", "passed", "ok", evidence);
    expect(result.evidence.length).toBe(QUALITY_LIMITS.maxEvidenceEntriesPerGate);
  });
});

describe("deterministic report status", () => {
  it("passes when every hard gate passes and the review is clean", () => {
    const status = computeQualityReportStatus(
      [
        gate("scope-verified", "passed"),
        gate("parser", "passed"),
        gate("lsp-errors", "passed"),
        gate("required-validation", "passed"),
        gate("independent-review", "passed"),
        gate("warnings", "passed"),
        gate("conventions", "passed"),
      ],
      cleanReview(0),
    );
    expect(status).toBe("passed");
  });

  it("reports validation_incomplete when a required validation step was denied", () => {
    const deniedGate = createQualityGateResult(
      "required-validation",
      "not_run",
      "the required validation command was denied",
      [{ kind: "validation-denied", summary: "denied" }],
    );
    const status = computeQualityReportStatus([deniedGate], cleanReview(0));
    expect(status).toBe("validation_incomplete");
  });

  it("reports validation_incomplete when required validation infrastructure was unavailable", () => {
    const unavailableGate = createQualityGateResult(
      "required-validation",
      "not_run",
      "the runner is unavailable",
      [{ kind: "validation-unavailable", summary: "unavailable" }],
    );
    const status = computeQualityReportStatus([unavailableGate], cleanReview(0));
    expect(status).toBe("validation_incomplete");
  });

  it("never reports passed when a required gate could not run", () => {
    const notRunGate = createQualityGateResult(
      "required-validation",
      "not_run",
      "not run",
      [{ kind: "validation-not-run", summary: "not run" }],
    );
    expect(computeQualityReportStatus([notRunGate], cleanReview(0))).toBe("validation_incomplete");
  });

  it("reports failed when a deterministic hard gate is blocked", () => {
    const status = computeQualityReportStatus(
      [gate("scope-verified", "blocked"), gate("parser", "passed")],
      cleanReview(0),
    );
    expect(status).toBe("failed");
  });

  it("reports failed when a required test exits nonzero", () => {
    const failedGate = createQualityGateResult(
      "required-validation",
      "blocked",
      "exit 1",
      [{ kind: "validation-failed", summary: "exit 1" }],
    );
    expect(computeQualityReportStatus([failedGate], cleanReview(0))).toBe("failed");
  });

  it("reports blocking_findings when the review has evidence-backed blockers", () => {
    const status = computeQualityReportStatus(
      [
        gate("scope-verified", "passed"),
        gate("parser", "passed"),
        gate("lsp-errors", "passed"),
        gate("required-validation", "passed"),
        gate("independent-review", "passed"),
      ],
      cleanReview(1),
    );
    expect(status).toBe("blocking_findings");
  });

  it("reports passed_with_advisories when soft gates carry advisories", () => {
    const status = computeQualityReportStatus(
      [
        gate("scope-verified", "passed"),
        gate("parser", "passed"),
        gate("lsp-errors", "passed"),
        gate("required-validation", "passed"),
        gate("independent-review", "passed"),
        gate("warnings", "advisory"),
        gate("conventions", "advisory"),
      ],
      cleanReview(0),
    );
    expect(status).toBe("passed_with_advisories");
  });

  it("reports cancelled when the review was cancelled", () => {
    const status = computeQualityReportStatus(
      [gate("independent-review", "not_run")],
      { status: "cancelled", findings: [], blockingCount: 0, message: "cancelled" },
    );
    expect(status).toBe("cancelled");
  });

  it("reports validation_incomplete when the review infrastructure failed or was too large", () => {
    for (const reviewStatus of ["failed", "too_large"] as const) {
      const status = computeQualityReportStatus([gate("independent-review", "not_run")], {
        status: reviewStatus,
        findings: [],
        blockingCount: 0,
        message: reviewStatus,
      });
      expect(status).toBe("validation_incomplete");
    }
  });

  it("is deterministic: the same inputs always produce the same status", () => {
    const inputs = [
      [gate("scope-verified", "passed"), gate("warnings", "advisory")] as const,
      [gate("scope-verified", "blocked")] as const,
      [gate("required-validation", "not_run")] as const,
    ];
    for (const gates of inputs) {
      const first = computeQualityReportStatus([...gates], cleanReview(0));
      const second = computeQualityReportStatus([...gates], cleanReview(0));
      expect(first).toBe(second);
    }
  });
});

describe("quality status vocabulary", () => {
  it("keeps the explicit quality states distinct", () => {
    const states: readonly QualityStatus[] = [
      "passed",
      "passed_with_advisories",
      "blocking_findings",
      "validation_incomplete",
      "failed",
      "cancelled",
    ];
    expect(new Set(states).size).toBe(states.length);
  });

  it("binds report limits immutably", () => {
    expect(QUALITY_LIMITS.maxReviewFindings).toBe(50);
    expect(QUALITY_LIMITS.maxReviewContextDiffBytes).toBe(1024 * 1024);
    expect(QUALITY_LIMITS.maxReviewRounds).toBe(3);
    expect(QUALITY_LIMITS.maxReviewRepairRounds).toBe(2);
    expect(QUALITY_LIMITS.reviewTimeoutMs).toBe(120_000);
  });
});
