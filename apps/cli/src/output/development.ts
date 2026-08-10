import type {
  ChangeReviewResult,
  DevelopmentQualityReport,
  GDScriptDevelopmentPreview,
  GDScriptDevelopmentResult,
  GDScriptDevelopmentStatus,
  QualityStatus,
} from "@solaris/core";
import { operationMark } from "./format-utils.js";

/** Workflow-start approval preview (§21 shape). */
export function formatDevelopmentStartPreview(preview: GDScriptDevelopmentPreview): string {
  const lines = [
    "Development workflow approval",
    "",
    `Request: ${preview.request}`,
    "",
    "Files: (no source changes yet; each proposed change set is approved separately)",
    "",
    "Authorization (read-only validation context):",
    "  LSP recreation after approved edits  covered",
    "  --check-only parser validation      covered",
    "  Godot API lookup                     covered",
    "  workspace inspection                 covered",
    "  Git inspection                       covered",
    "  project validation commands          each command approved separately",
    "  independent review                   read-only; fresh provider context",
    "  source writes                        each change set approved separately",
    "  network                              denied",
    "  game execution                       disabled",
    "",
    `Project fingerprint: ${preview.projectFingerprint.slice(0, 12)}…`,
    `Engine: ${preview.engineVersion ?? "no selected engine"}`,
    `Iteration limit: ${preview.limits.maxIterations} (${preview.limits.maxRepairProposals} repairs, ${preview.limits.maxReviewRounds} review rounds)`,
    "",
    "Approve this development workflow once? [y/N]",
  ];
  return lines.join("\n");
}

/** Bounded workflow status for /development-status (§38, §47). */
export function formatDevelopmentStatus(status: GDScriptDevelopmentStatus): string {
  if (status.session === null) {
    return status.support.available
      ? "No development workflow is active. Start one with /develop <request>."
      : `The GDScript development workflow is unavailable: ${status.support.reason ?? "unknown reason"}`;
  }
  const session = status.session;
  const state = session.state.kind === "active" ? session.state.phase : session.state.status;
  const validation =
    session.validation === null
      ? "not yet run"
      : session.validation === "clean"
        ? "clean"
        : session.validation === "warnings"
          ? "warnings"
          : session.validation === "errors"
            ? "errors"
            : session.validation === "infrastructure_failure"
              ? "infrastructure failure"
              : "cancelled";
  const quality = session.quality;
  const qualityLines =
    quality.status === null && quality.report === null
      ? ["Quality: not run"]
      : [
          `Quality: ${quality.status === null ? "pending" : describeQualityStatus(quality.status)}`,
          `  Review rounds: ${quality.reviewRoundsUsed}/${quality.maxReviewRounds}`,
          `  Repair rounds: ${quality.repairRoundsUsed}/${quality.maxRepairRounds}`,
          `  Blocking findings: ${quality.blockingFindings}`,
          `  Advisories: ${quality.advisories}`,
          ...(quality.report === null
            ? []
            : [
                "  Gates:",
                ...quality.report.gates.map(
                  (gate) => `    ${qualityGateMark(gate.status)} ${gate.id}`,
                ),
              ]),
        ];
  return `State: ${state}
Request: ${session.request}
Iteration: ${session.iteration} / ${session.maxIterations}
Applied change sets: ${session.appliedChangeSets}
Validation: ${validation}
Diagnostics: ${session.errors} error(s), ${session.warnings} warning(s)
Repair proposals remaining: ${session.repairProposalsRemaining}
${qualityLines.join("\n")}`;
}

/** Final development result summary for the CLI (§35, §38). */
export function formatDevelopmentResult(result: GDScriptDevelopmentResult): string {
  const changed = result.changes
    .map((change) => `  ${operationMark(change.operation)} ${change.path}`)
    .join("\n");
  const lines = [
    `Development workflow ${describeDevelopmentStatus(result.status)}`,
    `Iterations: ${result.iterations}`,
    `Changed:`,
    changed.length > 0 ? changed : "  (no source changes)",
    `Diagnostics: ${result.diagnostics.errors} error(s), ${result.diagnostics.warnings} warning(s)`,
    `Validation: parser ${result.validation.parser ? "passed" : "failed"}, LSP ${result.validation.lsp ? "started" : "failed"}, workspace integrity ${result.validation.workspaceIntegrity ? "verified" : "not verified"}`,
    `Checkpoints: ${result.checkpointIds.length > 0 ? result.checkpointIds.map((id) => id.slice(0, 8)).join(", ") : "(none)"}`,
    ...(result.quality === null
      ? []
      : [`Quality: ${describeQualityStatus(result.quality.status)}`]),
  ];
  return lines.join("\n");
}

function describeDevelopmentStatus(status: string): string {
  switch (status) {
    case "completed":
      return "complete";
    case "completed_with_warnings":
      return "complete (with warnings)";
    case "completed_with_errors":
      return "complete (with validation errors)";
    case "completed_with_blocking_findings":
      return "complete (with unresolved blocking review findings)";
    case "quality_gate_failed":
      return "stopped on a quality-gate failure; approved source changes remain";
    case "denied":
      return "denied; no source change was applied";
    case "conflict":
      return "stopped on a conflict; nothing stale was applied";
    case "cancelled":
      return "cancelled; approved changes (if any) remain";
    case "apply_failed":
      return "failed to apply a change set";
    case "validation_failed":
      return "validation infrastructure failed; approved source changes remain";
    case "unavailable":
      return "unavailable on this platform; nothing was changed";
    default:
      return status;
  }
}

function describeQualityStatus(status: QualityStatus): string {
  switch (status) {
    case "passed":
      return "READY";
    case "passed_with_advisories":
      return "READY WITH ADVISORIES";
    case "blocking_findings":
      return "BLOCKING FINDINGS";
    case "validation_incomplete":
      return "VALIDATION INCOMPLETE";
    case "failed":
      return "QUALITY GATE FAILED";
    case "cancelled":
      return "CANCELLED";
  }
}

function qualityGateMark(status: string): string {
  switch (status) {
    case "passed":
      return "\u2713";
    case "advisory":
      return "!";
    case "blocked":
      return "\u2715";
    case "not_applicable":
      return "-";
    case "not_run":
      return "?";
    case "failed":
      return "\u2715";
    default:
      return "?";
  }
}

/** Full quality report for /quality (§45). */
export function formatQualityReport(report: DevelopmentQualityReport | null): string {
  if (report === null) {
    return "No quality report exists yet; apply an approved change set in a /develop workflow first.";
  }
  const lines: string[] = ["Development quality"];
  const gateLines: string[] = [];
  const advisories: string[] = [];
  for (const gate of report.gates) {
    const mark = qualityGateMark(gate.status);
    gateLines.push(`  ${mark} ${gate.id} (${gate.classification})`);
    if (gate.status === "advisory") {
      advisories.push(`  ${gate.summary}`);
    }
  }
  lines.push("", "Gates:", ...gateLines);
  if (advisories.length > 0) {
    lines.push("", "Advisories:", ...advisories);
  }
  const review = report.review;
  if (review !== null && review.findings.length > 0) {
    lines.push("", "Independent review findings:");
    for (const finding of review.findings.slice(0, 20)) {
      const location =
        finding.path === null
          ? "project-wide"
          : `${finding.path}${finding.line === null ? "" : `:${finding.line}`}`;
      lines.push(
        `  [${finding.severity}/${finding.confidence}] ${finding.title} (${location})`,
        `    ${finding.evidence}`,
      );
    }
    if (review.findings.length > 20) {
      lines.push(`  ... and ${review.findings.length - 20} more (bounded)`);
    }
  }
  lines.push(
    "",
    `Result: ${describeQualityStatus(report.status)}`,
    `Review rounds: ${report.reviewRoundsUsed}/${report.maxReviewRounds} | Repair rounds: ${report.repairRoundsUsed}/${report.maxRepairRounds}`,
  );
  return lines.join("\n");
}

/** Compact quality summary for /status and /development-status (§47). */
export function formatQualitySummary(
  report: DevelopmentQualityReport | null,
  blockingFindings: number,
  advisories: number,
): string {
  if (report === null) {
    return "Quality: not run";
  }
  const counts =
    report.review === null
      ? ""
      : ` (${report.review.findings.length} finding(s), ${blockingFindings} blocking)`;
  return `Quality: ${describeQualityStatus(report.status)}${counts}${advisories > 0 ? `, ${advisories} advisory(ies)` : ""}`;
}

/** Read-only review result for /review-change (§45). */
export function formatChangeReviewResult(result: ChangeReviewResult): string {
  switch (result.status) {
    case "completed":
      if (result.findings.length === 0) {
        return "Independent review: no findings. The reviewer is one reasoning signal; deterministic gates still govern completion.";
      }
      return [
        `Independent review: ${result.findings.length} finding(s)`,
        ...result.findings.map((finding) => {
          const location =
            finding.path === null
              ? "project-wide"
              : `${finding.path}${finding.line === null ? "" : `:${finding.line}`}`;
          return `  [${finding.severity}/${finding.confidence}] ${finding.title} (${location})
    evidence: ${finding.evidence}
    impact: ${finding.impact}
    recommendation: ${finding.recommendation}`;
        }),
      ].join("\n");
    case "cancelled":
      return "Independent review cancelled; validation is incomplete.";
    case "too_large":
      return `Independent review could not cover the change: ${result.message ?? "the change exceeds the review-context bound"}.`;
    case "failed":
      return `Independent review failed: ${result.message ?? "unknown failure"}`;
  }
}

/** Task phase display mark. */
