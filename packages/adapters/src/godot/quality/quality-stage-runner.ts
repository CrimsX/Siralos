import {
  QUALITY_LIMITS,
  analyzeConventions,
  chunkChangeReviewRequests,
  classifyReviewFindingBlocking,
  classifyValidationGate,
  computeQualityReportStatus,
  computeWarningDelta,
  countReviewFindingsBySeverity,
  createQualityGateResult,
  discoverValidationPlan,
  type ChangeDiffMetrics,
  type ChangeReviewFinding,
  type ChangeReviewRequest,
  type ChangeReviewResult,
  type ChangeReviewer,
  type DevelopmentEvidence,
  type DevelopmentQualityReport,
  type GodotGDScriptDiagnostic,
  type QualityEvent,
  type QualityEvidence,
  type QualityGateResult,
  type QualityValidationExecutor,
  type ValidationPlanDiscovery,
  type ValidationRunOutcome,
} from "@solaris/core";
import { aggregateReviewResults } from "@solaris/core";

/**
 * Deterministic quality-stage runner (ADR 0013 §7–§11, §17–§24).
 *
 * Runs after the workflow's parser/LSP gates: computes the deterministic
 * gates, executes the applicable validation plan through the approved
 * command machinery, runs the independent read-only review (chunked by
 * complete file when the context is large), and produces the bounded
 * quality report. Deterministic gates are authoritative; the review is an
 * additional reasoning signal that can never replace a gate.
 */

export interface QualityStageChangeFile {
  readonly path: string;
  readonly operation: "create" | "update" | "delete";
  /** Post-change content (create/update); null for delete. */
  readonly afterContent: string | null;
  /** Deterministic bounded unified diff (synthesized for delete). */
  readonly unifiedDiff: string;
}

export interface QualityWarningBaseline {
  readonly available: boolean;
  readonly diagnostics: readonly GodotGDScriptDiagnostic[];
}

export interface QualityStageInput {
  readonly developmentId: string;
  readonly request: string;
  readonly engineVersion: string | null;
  readonly changeSetId: string;
  readonly files: readonly QualityStageChangeFile[];
  readonly evidence: DevelopmentEvidence;
  readonly checkpointIds: readonly string[];
  /** Git status at workflow start (best-effort); null when unavailable. */
  readonly gitBaseline: readonly string[] | null;
  /** Git changed files at validation time; null when unavailable. */
  readonly gitCurrent: readonly string[] | null;
  readonly warningBaseline: QualityWarningBaseline;
  /** Post-edit LSP diagnostics for the changed files. */
  readonly lspDiagnostics: readonly GodotGDScriptDiagnostic[];
  readonly reviewer: ChangeReviewer;
  readonly validation: {
    readonly discovery: ValidationPlanDiscovery;
    readonly executor: QualityValidationExecutor;
  };
  readonly previousFindingIds: readonly string[];
  readonly reviewRound: number;
  readonly repairRoundsUsed: number;
  readonly maxRepairRounds: number;
  readonly emit?: (event: QualityEvent) => void;
  readonly now?: () => number;
}

export interface QualityStageOutput {
  readonly report: DevelopmentQualityReport;
  /** Blocking findings when the review found them; empty otherwise. */
  readonly blockingFindings: readonly ChangeReviewFinding[];
}

export async function runQualityStage(input: QualityStageInput): Promise<QualityStageOutput> {
  const now = input.now ?? (() => Date.now());
  const emit = input.emit ?? (() => undefined);
  const gates: QualityGateResult[] = [];
  const reviewInput = buildReviewRequest(input);
  const reviewChunks = chunkChangeReviewRequests(reviewInput);

  emit({ type: "quality_started", developmentId: input.developmentId });

  // 1. Deterministic hard gates.
  const changedPaths = input.files.map((file) => file.path);
  const changedScripts = changedPaths.filter((path) => path.endsWith(".gd"));
  const integrity = input.evidence.workspaceIntegrity;
  const approvedPaths = new Set(changedPaths);

  gates.push(
    createQualityGateResult(
      "approved-change-applied",
      input.evidence.changeSetId === input.changeSetId && input.files.length > 0
        ? "passed"
        : "blocked",
      input.files.length > 0
        ? `The approved change set (${input.changeSetId}) was applied exactly.`
        : "No approved change set was applied.",
      [{ kind: "change-set", summary: `change set ${input.changeSetId}, ${input.files.length} file(s)` }],
    ),
  );

  gates.push(
    createQualityGateResult(
      "checkpoint-recorded",
      input.checkpointIds.length > 0 ? "passed" : "blocked",
      input.checkpointIds.length > 0
        ? `${input.checkpointIds.length} checkpoint(s) recorded before application.`
        : "No checkpoint was recorded before application.",
      input.checkpointIds.map((id) => ({ kind: "checkpoint", summary: id })),
    ),
  );

  const scopeEvidence: QualityEvidence[] = [];
  let scopeStatus: QualityGateResult["status"] = "passed";
  const unexpected = integrity.unexpectedChanges;
  if (!integrity.verified || unexpected.length > 0) {
    scopeStatus = "blocked";
    scopeEvidence.push({ kind: "unexpected-changes", summary: unexpected.join(", ") });
  }
  for (const path of unexpected) {
    if (containsGodotDirectory(path)) {
      scopeEvidence.push({
        kind: "generated-leak",
        summary: `Generated .godot content leaked into the workspace: ${path}`,
      });
    }
  }
  for (const path of approvedPaths) {
    if (containsGodotDirectory(path)) {
      scopeStatus = "blocked";
      scopeEvidence.push({
        kind: "generated-leak",
        summary: `The change set itself addresses generated .godot content: ${path}`,
      });
    }
  }
  if (scopeStatus === "passed" && input.gitCurrent !== null && input.gitBaseline !== null) {
    const baseline = new Set(input.gitBaseline);
    const unrelated = input.gitCurrent.filter((path) => !baseline.has(path) && !approvedPaths.has(path));
    if (unrelated.length > 0) {
      scopeStatus = "blocked";
      scopeEvidence.push({
        kind: "git-unrelated-changes",
        summary: `Git reports changes outside the approved change set: ${unrelated.join(", ")}`,
      });
    }
  }
  gates.push(
    createQualityGateResult(
      "scope-verified",
      scopeStatus,
      scopeStatus === "passed"
        ? "Only approved files changed; no unexpected workspace change was detected."
        : "Unexpected workspace changes were detected; the change is not cleanly scoped.",
      scopeEvidence.length > 0
        ? scopeEvidence
        : [{ kind: "scope", summary: "workspace integrity verified" }],
    ),
  );

  const parserValid =
    changedScripts.length === 0
      ? null
      : input.evidence.parser.checkedFiles > 0 &&
        input.evidence.parser.validFiles === input.evidence.parser.checkedFiles;
  gates.push(
    createQualityGateResult(
      "parser",
      parserValid === null
        ? "not_applicable"
        : parserValid
          ? "passed"
          : "blocked",
      parserValid === null
        ? "No GDScript files changed; the parser gate is not applicable."
        : parserValid
          ? `${input.evidence.parser.checkedFiles} changed script(s) parsed with the exact engine.`
          : "At least one changed script failed --check-only parsing.",
      [
        {
          kind: "parser",
          summary: `${input.evidence.parser.validFiles}/${input.evidence.parser.checkedFiles} scripts valid`,
        },
      ],
    ),
  );

  const errorDiagnostics = [
    ...input.evidence.lsp.diagnostics,
    ...input.evidence.parser.diagnostics,
  ].filter(
    (diagnostic) =>
      diagnostic.severity === "error" &&
      (diagnostic.path === null || changedPaths.includes(diagnostic.path)),
  );
  gates.push(
    createQualityGateResult(
      "lsp-errors",
      errorDiagnostics.length === 0 ? "passed" : "blocked",
      errorDiagnostics.length === 0
        ? "No error-severity diagnostics in the changed files."
        : `${errorDiagnostics.length} error-severity diagnostic(s) in the changed files.`,
      errorDiagnostics.slice(0, QUALITY_LIMITS.maxEvidenceEntriesPerGate).map((diagnostic) => ({
        kind: "lsp-error",
        summary: `${diagnostic.path ?? "<unknown>"}:${diagnostic.line ?? "?"} ${diagnostic.message}`,
        ...(diagnostic.code === null ? {} : { detail: diagnostic.code }),
      })),
    ),
  );

  // 2. Required validation plan (commands require one-time process approval).
  const packageScripts = await input.validation.discovery.discover().catch(() => null);
  const plan = discoverValidationPlan(packageScripts?.packageScripts ?? null, changedPaths);
  const outcomes: ValidationRunOutcome[] = [];
  const commandSteps = plan.optional;
  for (const step of commandSteps) {
    const outcome = await input.validation.executor.run(step);
    outcomes.push(outcome);
  }
  const validationGate = classifyValidationGate(plan, outcomes, parserValid === true);
  const validationEvidence: QualityEvidence[] = [];
  for (const outcome of outcomes) {
    validationEvidence.push({
      kind:
        outcome.status === "passed"
          ? "validation-passed"
          : outcome.status === "denied"
            ? "validation-denied"
            : outcome.status === "unavailable"
              ? "validation-unavailable"
              : "validation-failed",
      summary: `${outcome.step.displayName}: ${outcome.summary}`,
      ...(outcome.exitCode === null ? {} : { detail: `exit ${outcome.exitCode}` }),
    });
  }
  if (validationGate.evidenceKinds.some((kind) => kind.startsWith("intrinsic-"))) {
    validationEvidence.push({
      kind: "intrinsic-validation",
      summary: "Changed GDScript --check-only and changed-file LSP diagnostics ran as required.",
    });
  }
  if (validationGate.status === "not_applicable") {
    validationEvidence.push({
      kind: "no-project-test-runner",
      summary: "No supported project test runner was discovered; this is not an infrastructure failure.",
    });
  }
  gates.push(
    createQualityGateResult(
      "required-validation",
      validationGate.status,
      validationGate.summary,
      validationEvidence,
    ),
  );

  // 3. Independent review (chunked by complete file when needed).
  emit({ type: "review_started", developmentId: input.developmentId });
  const chunkedResults: ChangeReviewResult[] = [];
  for (const chunk of reviewChunks) {
    const chunkResult = await input.reviewer.review(chunk);
    chunkedResults.push(chunkResult);
    // A cancelled or failed chunk invalidates the whole review: the
    // remaining chunks are not silently claimed as reviewed.
    if (chunkResult.status !== "completed") {
      break;
    }
  }
  const reviewResult = aggregateReviewResults(chunkedResults);
  const reviewCounts = countReviewFindingsBySeverity(reviewResult.findings);
  emit({
    type: "review_completed",
    developmentId: input.developmentId,
    critical: reviewCounts.critical,
    high: reviewCounts.high,
    medium: reviewCounts.medium,
    low: reviewCounts.low,
  });
  const blockingFindings = reviewResult.findings.filter(classifyReviewFindingBlocking);
  const reviewGate: QualityGateResult = (() => {
    switch (reviewResult.status) {
      case "completed":
        return createQualityGateResult(
          "independent-review",
          blockingFindings.length === 0 ? "passed" : "blocked",
          blockingFindings.length === 0
            ? "The independent review found no blocking findings."
            : `${blockingFindings.length} blocking finding(s) from the independent review.`,
          reviewResult.findings.map((finding) => ({
            kind: `review-${finding.severity}`,
            summary: `[${finding.category}] ${finding.title} (${finding.path ?? "project-wide"})`,
            detail: finding.id,
          })),
        );
      case "cancelled":
        return createQualityGateResult(
          "independent-review",
          "not_run",
          "The independent review was cancelled; validation is incomplete.",
          [{ kind: "review-cancelled", summary: reviewResult.message ?? "cancelled" }],
        );
      case "too_large":
        return createQualityGateResult(
          "independent-review",
          "not_run",
          "The change exceeds the review-context bound and could not be fully reviewed.",
          [{ kind: "review-too-large", summary: reviewResult.message ?? "too large" }],
        );
      case "failed":
        return createQualityGateResult(
          "independent-review",
          "not_run",
          "The independent review could not run; validation is incomplete.",
          [{ kind: "review-failed", summary: reviewResult.message ?? "failed" }],
        );
    }
  })();
  gates.push(reviewGate);

  // 4. Soft gates: warning delta and conventions.
  const warningDelta = computeWarningDelta(
    input.warningBaseline.diagnostics,
    input.lspDiagnostics,
    changedPaths,
    { baselineAvailable: input.warningBaseline.available },
  );
  const warningEvidence: QualityEvidence[] = [];
  if (!input.warningBaseline.available) {
    warningEvidence.push({
      kind: "warning-baseline-unavailable",
      summary:
        "The pre-change warning baseline could not be captured; warning attribution may be uncertain.",
    });
  }
  if (warningDelta.introducedWarnings > 0) {
    warningEvidence.push({
      kind: "warning-introduced",
      summary: `${warningDelta.introducedWarnings} newly introduced warning(s).`,
    });
  }
  if (warningDelta.resolvedWarnings > 0) {
    warningEvidence.push({
      kind: "warning-resolved",
      summary: `${warningDelta.resolvedWarnings} pre-existing warning(s) resolved by the change.`,
    });
  }
  if (warningDelta.uncertainWarnings > 0) {
    warningEvidence.push({
      kind: "warning-uncertain",
      summary: `${warningDelta.uncertainWarnings} warning(s) could not be attributed with certainty.`,
    });
  }
  for (const entry of warningDelta.entries) {
    if (entry.classification === "introduced" || entry.classification === "uncertain") {
      warningEvidence.push({
        kind: `warning-${entry.classification}`,
        summary: `${entry.path}:${entry.line ?? "?"} ${entry.message}`,
        ...(entry.code === null ? {} : { detail: entry.code }),
      });
    }
  }
  gates.push(
    createQualityGateResult(
      "warnings",
      warningDelta.introducedWarnings > 0 ||
        warningDelta.uncertainWarnings > 0 ||
        !input.warningBaseline.available
        ? "advisory"
        : "passed",
      warningDelta.introducedWarnings > 0
        ? "The change introduced new warnings (advisory unless project policy promotes them)."
        : warningDelta.uncertainWarnings > 0
          ? "Some warning attribution is uncertain."
          : !input.warningBaseline.available
            ? "No pre-change baseline was available; warning attribution is limited."
            : "No new warnings were introduced.",
      warningEvidence.slice(0, QUALITY_LIMITS.maxEvidenceEntriesPerGate),
    ),
  );

  const conventionFindings = analyzeConventions(
    input.files.map((file) => ({
      path: file.path,
      operation: file.operation,
      afterContent: file.afterContent,
      unifiedDiff: file.unifiedDiff,
    })),
  );
  const mandatoryConventionBlock = conventionFindings.some((finding) => finding.severity === "warning");
  gates.push(
    createQualityGateResult(
      "conventions",
      mandatoryConventionBlock
        ? "blocked"
        : conventionFindings.length > 0
          ? "advisory"
          : "passed",
      mandatoryConventionBlock
        ? "A repository-mandatory convention rule was violated."
        : conventionFindings.length > 0
          ? `${conventionFindings.length} convention advisory(ies).`
          : "No convention issues found in the changed lines.",
      conventionFindings.slice(0, QUALITY_LIMITS.maxEvidenceEntriesPerGate).map((finding) => ({
        kind: `convention-${finding.rule}`,
        summary: `${finding.path}:${finding.line ?? "?"} ${finding.message}`,
        detail: finding.basis,
      })),
    ),
  );

  const metrics = computeMetrics(input.files);
  gates.push(
    createQualityGateResult(
      "diff-metrics",
      "passed",
      `${metrics.filesChanged} file(s), +${metrics.linesAdded} -${metrics.linesRemoved}.`,
      [
        {
          kind: "diff-metrics",
          summary: `files ${metrics.filesChanged}, added ${metrics.linesAdded}, removed ${metrics.linesRemoved}, created ${metrics.filesCreated}, deleted ${metrics.filesDeleted}${metrics.functionsTouched === null ? "" : `, functions touched ${metrics.functionsTouched}`}`,
        },
      ],
    ),
  );

  const report: DevelopmentQualityReport = {
    developmentId: input.developmentId,
    status: computeQualityReportStatus(gates, {
      status: reviewResult.status,
      findings: reviewResult.findings,
      blockingCount: blockingFindings.length,
      message: reviewResult.message,
    }),
    gates,
    review:
      reviewResult.status === "completed" || reviewResult.status === "failed" || reviewResult.status === "cancelled" || reviewResult.status === "too_large"
        ? {
            status: reviewResult.status,
            findings: reviewResult.findings,
            blockingCount: blockingFindings.length,
            message: reviewResult.message,
          }
        : null,
    repairRoundsUsed: input.repairRoundsUsed,
    maxRepairRounds: input.maxRepairRounds,
    reviewRoundsUsed: input.reviewRound,
    maxReviewRounds: QUALITY_LIMITS.maxReviewRounds,
    previousFindingIds: input.previousFindingIds,
    completedAtMs: now(),
  };
  for (const gate of gates) {
    emit({
      type: "quality_gate_completed",
      developmentId: input.developmentId,
      gateId: gate.id,
      status: gate.status,
    });
  }
  emit({
    type: "quality_completed",
    developmentId: input.developmentId,
    status: report.status,
  });
  return { report, blockingFindings };
}

function buildReviewRequest(input: QualityStageInput): ChangeReviewRequest {
  const metrics = computeMetrics(input.files);
  const evidenceSummary: QualityEvidence[] = [
    {
      kind: "parser",
      summary: `${input.evidence.parser.validFiles}/${input.evidence.parser.checkedFiles} changed scripts parsed`,
    },
    {
      kind: "lsp",
      summary: `${input.evidence.lsp.diagnosticCount} changed-file LSP diagnostics collected`,
    },
    {
      kind: "scope",
      summary: input.evidence.workspaceIntegrity.verified
        ? "workspace integrity verified"
        : `unexpected changes: ${input.evidence.workspaceIntegrity.unexpectedChanges.join(", ")}`,
    },
  ];
  return {
    developmentId: input.developmentId,
    request: input.request,
    engineVersion: input.engineVersion,
    changedPaths: input.files.map((file) => file.path),
    files: input.files.map((file) => ({
      path: file.path,
      unifiedDiff:
        file.operation === "delete"
          ? `--- a/${file.path}\n+++ /dev/null\n@@ -1,0 +0,0 @@\nFile deleted by the approved change set.`
          : file.unifiedDiff,
    })),
    metrics,
    evidenceSummary,
    repositoryGuidance: null,
    previousFindingIds: input.previousFindingIds,
    reviewRound: input.reviewRound,
  };
}

function computeMetrics(files: readonly QualityStageChangeFile[]): ChangeDiffMetrics {
  let linesAdded = 0;
  let linesRemoved = 0;
  let filesCreated = 0;
  let filesDeleted = 0;
  let functionsTouched = 0;
  for (const file of files) {
    if (file.operation === "create") {
      filesCreated += 1;
    } else if (file.operation === "delete") {
      filesDeleted += 1;
    }
    for (const line of file.unifiedDiff.split("\n")) {
      if (line.startsWith("+") && !line.startsWith("+++")) {
        linesAdded += 1;
        if (/^\s*func\s+[A-Za-z_]/.test(line.slice(1))) {
          functionsTouched += 1;
        }
      } else if (line.startsWith("-") && !line.startsWith("---")) {
        linesRemoved += 1;
      }
    }
  }
  return {
    filesChanged: files.length,
    linesAdded,
    linesRemoved,
    filesCreated,
    filesDeleted,
    functionsTouched,
  };
}

function containsGodotDirectory(path: string): boolean {
  return path.split("/").includes(".godot") || path.startsWith(".godot");
}
