import type {
  ApprovalRequest,
  Capability,
  CapabilityPolicy,
  RegisteredToolInfo,
  SandboxBackendStatus,
  SandboxProfile,
  SessionStatus,
  SolarisSecurity,
} from "@solaris/core";
import { renderExecutorBrief } from "@solaris/core";
import type { SandboxDoctorReport } from "./bootstrap/sandbox-doctor.js";
import { sanitizeForDisplay, sanitizePathForDisplay } from "./output/sanitize.js";
import {
  formatBytes,
  formatFileCount,
  operationMark,
  formatTimeoutSeconds,
  yesNo,
} from "./output/format-utils.js";
import { formatGodotDiagnosticApprovalPrompt } from "./output/godot.js";
import { formatDevelopmentStartPreview } from "./output/development.js";

export {
  TerminalSanitizer,
  describeError,
  sanitizeForDisplay,
  sanitizePathForDisplay,
} from "./output/sanitize.js";
export {
  formatCancelReport,
  formatCheckpoints,
  formatCommandCompleted,
  formatCommandStarted,
  formatCommandTerminal,
  formatCommands,
  formatGitDiff,
  formatGitStatus,
  formatInvalidCommand,
  formatNoActiveCommand,
  formatProviderFailure,
  formatToolCancelled,
  formatToolCompleted,
  formatToolFailed,
  formatToolStarted,
  formatUndoOutcome,
  type CommandsView,
} from "./output/system.js";
export {
  formatGodotDiagnosticPreview,
  formatGodotKnowledgeStatus,
  formatGodotApiSearchResult,
  formatGodotDiagnosticsResult,
  formatGodotLSPSessionStatus,
  formatGodotLSPSessionPreview,
  formatGodotHoverResult,
  formatGodotCompletionResult,
  formatGodotDefinitionResult,
  formatGodotSummary,
  formatGodotInstallations,
  formatGodotProject,
  formatGodotProbePreview,
  formatGodotProbeTerminal,
  formatGodotProbeResult,
  formatGodotProbeStatus,
  formatGodotDoctor,
} from "./output/godot.js";
export {
  formatDevelopmentStartPreview,
  formatDevelopmentStatus,
  formatDevelopmentResult,
  formatQualityReport,
  formatQualitySummary,
  formatChangeReviewResult,
} from "./output/development.js";
export {
  formatTaskStatus,
  formatContextStatus,
  formatToolProjection,
  formatInstructions,
  formatKnowledge,
  formatKnowledgeTrace,
  formatReferences,
  formatReferenceDetail,
  formatResearchStatus,
  formatStructuralRead,
} from "./output/context.js";
export {
  formatSolarisDoctorReport,
  formatSafeDoctorReport,
  formatSelfReference,
} from "./output/doctor.js";

const CAPABILITIES: readonly Capability[] = [
  "workspace.read",
  "workspace.write",
  "git.inspect",
  "godot.inspect",
  "godot.probe_project",
  "godot.development",
  "process.execute",
  "network.outbound",
];

export function formatHeader(providerId: string): string {
  return `Solaris
Interactive Godot development harness
Provider: ${providerId}
`;
}

export function formatHelp(): string {
  return `Available commands:
  /help              Show this help
  /status            Show provider, session, and workspace status
  /clear             Clear the terminal (conversation is kept)
  /tools             List the available tools
  /sandbox           Show the sandbox backend status
  /permissions       Show capability rules
  /commands          Show command runners and command status
  /cancel            Cancel the running command
  /context           Show the projected context (stable/contextual/volatile, pressure)
  /instructions      Show discovered project instruction files with revisions
  /knowledge         Show current project knowledge facts (/knowledge why: last retrieval trace)
  /references        Show configured external references and their status
  /reference <alias> Show one reference's identity and availability
  /research-status   Show research capability, sources, and recent evidence
  /git-status        Show Git availability and repository status
  /diff              Show a bounded Git diff (working, staged, or head)
  /checkpoints       List recorded recovery checkpoints
  /undo              Undo the latest Solaris mutation (or /undo <checkpoint-id>)
  /task <request>    Start a host-owned ad-hoc task (completion requires host verification)
  /task-status       Show the current task: phase, contract revision, criteria, steps, progress
  /godot             Show the selected Godot installation and project compatibility
  /godot-installations  Show all discovered Godot installations and selection rationale
  /godot-project     Show the static Godot project profile
  /godot-doctor      Run bounded Godot diagnostics
  /godot-probe       Prepare one recovery-mode Godot project probe (approval required; reports unavailable when the platform cannot bind execution)
  /godot-probe-status  Show the recovery probe capability and last outcome
  /godot-knowledge   Show the exact-engine API knowledge status
  /godot-knowledge-refresh  Regenerate the exact-engine API knowledge profile (reports unavailable when the platform cannot bind execution)
  /godot-api <query>  Search the exact engine's API documentation locally
  /gdscript-check <relative-path>  Check one .gd script with --check-only (approval required)
  /gdscript-diagnostics  Check the project's .gd scripts sequentially with --check-only (approval required)
  /gdscript-lsp      Start (approval required) or show the Godot GDScript language session
  /gdscript-lsp-stop  Gracefully stop the language session (no approval needed)
  /gdscript-hover <path> <line> <column>  Hover information from the language session
  /gdscript-complete <path> <line> <column>  Completion candidates from the language session
  /gdscript-definition <path> <line> <column>  Definition locations from the language session
  /develop <request>  Start one GDScript development workflow (one-time approval; each source change is approved separately)
  /development-status  Show the active development workflow's bounded status
  /quality           Show the current or final development quality report
  /review-change     Run a fresh read-only independent review of the current development change (no approval, no modifications)
  /exit              Close Solaris
`;
}

export interface StatusView {
  readonly status: SessionStatus;
  readonly workspaceRoot: string;
  readonly toolCount: number;
  readonly providerToolCount: number;
  readonly profileId: string;
  readonly gitRepositoryState: string;
  readonly gitBranch: string | null;
  readonly gitDirtyCount: number;
  readonly latestCheckpoint: string | null;
  readonly uncertainCheckpointCount: number;
  readonly processPermission: string;
  readonly runnerCount: number;
  readonly activeCommandId: string | null;
  readonly lastCommandExitCode: number | null;
  readonly commandProfile: string;
  readonly godotSelectedInstallation: string | null;
  readonly godotVersion: string | null;
  readonly godotProjectDetected: boolean;
  readonly godotCompatibility: string | null;
  readonly godotWarningCount: number;
  readonly projectProbe: string;
  readonly knowledge: string;
  readonly languageSession: string;
  /** Compact quality summary when a development workflow exists. */
  readonly developmentQuality: string | null;
  /** Research capability state and configured source count. */
  readonly research: string;
}

export function formatStatus(view: StatusView): string {
  const { status } = view;
  const sessionState = status.state === "responding" ? "responding" : "active";
  const gitLine =
    view.gitRepositoryState === "repository"
      ? `Git: ${view.gitRepositoryState}${view.gitBranch === null ? "" : ` (${view.gitBranch})`}, ${view.gitDirtyCount} dirty`
      : `Git: ${view.gitRepositoryState}`;
  return `Provider: ${status.providerId}
Session: ${sessionState}
Messages: ${status.messageCount}
Workspace: ${view.workspaceRoot}
Sandbox: ${view.profileId}
Pending approval: ${status.pendingApproval ? "yes" : "no"}
${gitLine}
Checkpoint: ${view.latestCheckpoint === null ? "none" : view.latestCheckpoint}
Uncertain checkpoints: ${view.uncertainCheckpointCount}
Provider tools: ${view.providerToolCount}
Tools: ${view.toolCount}
Process execution: ${view.processPermission}
Command runners: ${view.runnerCount}
Active command: ${view.activeCommandId ?? "none"}
Last command exit: ${view.lastCommandExitCode ?? "none"}
Command profile: ${view.commandProfile}
Godot: ${view.godotSelectedInstallation === null ? "no installation selected" : view.godotSelectedInstallation}${view.godotVersion === null ? "" : ` (${view.godotVersion})`}
Godot project: ${view.godotProjectDetected ? "detected" : "none"}${view.godotCompatibility === null ? "" : `, compatibility: ${view.godotCompatibility}`}${view.godotWarningCount > 0 ? `, warnings: ${view.godotWarningCount}` : ""}
Recovery probe: ${view.projectProbe}
Knowledge: ${view.knowledge}
Godot LSP: ${view.languageSession}
Research: ${view.research}
${view.developmentQuality === null ? "" : `${view.developmentQuality}\n`}`;
}

export function formatPermissions(policy: CapabilityPolicy, profileId: string): string {
  const lines = CAPABILITIES.map((capability) => {
    const rule = policy.rules[capability] ?? "deny";
    return `  ${capability.padEnd(18)} ${rule}`;
  });
  return `Profile: ${profileId}

${lines.join("\n")}

Command execution requires one-time approval per exact command plan.
`;
}

export function formatSandbox(status: SandboxBackendStatus, profile: SandboxProfile): string {
  const lines = [
    `Profile: ${profile.id}`,
    `Backend: ${status.backendId}`,
    `Platform: ${status.platform}`,
    `State: ${status.state}`,
    `Version: ${status.version}`,
    `Filesystem read restriction: ${yesNo(status.capabilities.filesystemReadRestriction)}`,
    `Filesystem write restriction: ${yesNo(status.capabilities.filesystemWriteRestriction)}`,
    `Network restriction: ${yesNo(status.capabilities.networkRestriction)}`,
    `Process-tree restriction: ${yesNo(status.capabilities.processTreeRestriction)}`,
    `Violation reporting: ${yesNo(status.capabilities.violationReporting)}`,
    `Network: denied`,
    `Environment: minimal`,
  ];
  if (status.message !== undefined) {
    lines.push(`Setup: ${status.message}`);
  }
  if (status.state === "degraded") {
    lines.push("Warning: the sandbox backend is degraded.");
  }
  if (status.state === "failed") {
    lines.push("Warning: the sandbox backend failed its checks; nothing will run sandboxed.");
  }
  if (status.platform === "windows") {
    lines.push(
      "Warning: the native Windows backend is alpha; do not treat it as secure until Solaris conformance passes.",
    );
  }
  return `${lines.join("\n")}\n`;
}

export function formatSandboxViolation(category: string, summary: string): string {
  return `  \u26A0 sandbox violation (${category}): ${summary}\n`;
}

export function formatDoctor(report: SandboxDoctorReport): string {
  const lines = [
    "Solaris sandbox doctor",
    `Profile: ${report.profileId}`,
    `Backend: ${report.backendId}`,
    `Backend version: ${report.backendVersion}`,
    `Platform: ${report.platform}`,
    `State: ${report.state}`,
    `Filesystem read restriction: ${yesNo(report.capabilities.filesystemReadRestriction)}`,
    `Filesystem write restriction: ${yesNo(report.capabilities.filesystemWriteRestriction)}`,
    `Network restriction: ${yesNo(report.capabilities.networkRestriction)}`,
    `Process-tree restriction: ${yesNo(report.capabilities.processTreeRestriction)}`,
    `Violation reporting: ${yesNo(report.capabilities.violationReporting)}`,
  ];
  if (report.statusMessage !== null) {
    lines.push(`Setup requirements: ${report.statusMessage}`);
  }
  if (!report.probesRun) {
    if (report.state === "available") {
      lines.push("Live conformance: not run (use --sandbox-doctor --run-probes)");
    } else {
      lines.push(
        `Live conformance: not run — the backend state is ${report.state}; probes are never treated as passing when they cannot execute.`,
      );
    }
  } else {
    lines.push("Live conformance: ran");
    if (report.conformance !== null) {
      for (const result of report.conformance.results) {
        const mark =
          result.outcome === "passed" ? "PASS" : result.outcome === "skipped" ? "SKIP" : "FAIL";
        lines.push(`  [${mark}] ${result.probeId}: ${result.description}`);
      }
      lines.push(
        `Result: ${report.conformance.passed} passed, ${report.conformance.failed} failed, ${report.conformance.skipped} skipped.`,
      );
    }
  }
  lines.push("Exit code: 0 = passed, 1 = probe failure, 3 = probes unavailable");
  return `${lines.join("\n")}\n`;
}

export function formatTools(
  tools: readonly RegisteredToolInfo[],
  security: SolarisSecurity,
): string {
  if (tools.length === 0) {
    return "Available tools:\n  (none)\n";
  }
  const lines = tools.map((info) => {
    const kind =
      info.capability === "workspace.write"
        ? "write"
        : info.capability === "godot.probe_project"
          ? "reviewable"
          : "read-only";
    const decision = security.evaluateCapability(info.capability);
    const status =
      decision.decision === "deny"
        ? "denied"
        : decision.decision === "ask"
          ? "approval required"
          : "allowed";
    return `  ${info.definition.name} - ${info.definition.description} (${kind}, ${status})`;
  });
  return `Available tools:\n${lines.join("\n")}\n`;
}

export function formatApprovalPrompt(request: ApprovalRequest): string {
  if (request.capability === "process.execute") {
    return formatCommandApprovalPrompt(request);
  }
  if (request.capability === "godot.probe_project") {
    return formatGodotProbeApprovalPrompt(request);
  }
  if (request.capability === "godot.diagnose") {
    return formatGodotDiagnosticApprovalPrompt(request);
  }
  if (request.capability === "godot.lsp") {
    return formatGodotLSPSessionApprovalPrompt(request);
  }
  if (request.capability === "godot.development") {
    return formatDevelopmentStartPreview(request.preview);
  }
  if (request.capability === "plan.approve") {
    return formatPlanApprovalPrompt(request);
  }
  const files = request.preview.files;
  const lines = [
    "Approval required",
    "",
    `Tool: ${request.toolName}`,
    `Capability: ${request.capability}`,
    `Files: ${files.length === 0 ? "(none)" : files.map((file) => `${operationMark(file.operation)} ${sanitizePathForDisplay(file.path)}`).join(", ")}`,
    `Change: +${request.preview.totalAddedLines} -${request.preview.totalRemovedLines}`,
    "",
  ];
  if (request.toolName === "solaris.undo") {
    lines.push(`Note: ${request.summary}`);
    lines.push("This restores only the state recorded before the Solaris operation.");
    lines.push("Any later file change will cause a conflict.");
    lines.push("");
  }
  for (const file of files) {
    lines.push(`--- ${operationMark(file.operation)} ${sanitizePathForDisplay(file.path)} ---`);
    if (file.unifiedDiff.length > 0) {
      lines.push(sanitizeForDisplay(file.unifiedDiff));
    } else if (file.operation === "delete") {
      lines.push(`(delete: ${file.removedLines} line(s) removed)`);
    }
  }
  lines.push("");
  lines.push(`Approval applies once to plan ${request.digest.slice(0, 8)}.`);
  return `${lines.join("\n")}\n`;
}

function formatPlanApprovalPrompt(
  request: Extract<ApprovalRequest, { capability: "plan.approve" }>,
): string {
  return (
    [
      "Plan approval required",
      "",
      `Plan: ${request.planId} rev ${request.planRevision}`,
      `TaskContract revision: ${request.taskContractRevision}`,
      `Digest: ${request.digest.slice(0, 8)}`,
      "",
      request.summary,
      "",
      "Approving this plan binds the host decision to EXACTLY this plan revision.",
      "Plan approval NEVER authorizes source edits or commands: every mutation",
      "and every command still requires its own exact one-time approval, and",
      "plan content never grants capability.",
    ].join("\n") + "\n"
  );
}

/**
 * Human-readable plan rendering (Stage 3 milestone 7). Rendering stays
 * separate from the domain types; only the CURRENT plan revision is ever
 * rendered, and plans are compact by validation bounds.
 */
export function formatPlan(
  plan: import("@solaris/core").TaskPlan,
  state: import("@solaris/core").TaskPlanState | null,
): string {
  const lines = [
    `Plan rev ${plan.revision} \u2014 ${plan.depth === "full" ? "Full" : "Light"}`,
    "",
    "Objective",
    plan.objective,
  ];
  if (plan.depth === "full") {
    const scope = [
      ...plan.scope.inScope,
      ...plan.scope.outOfScope.map((entry) => `(out of scope) ${entry}`),
    ];
    if (scope.length > 0) {
      lines.push("", "Scope", ...scope.map((entry) => `- ${entry}`));
    }
    if (plan.nonGoals.length > 0) {
      lines.push("", "Non-goals", ...plan.nonGoals.map((entry) => `- ${entry}`));
    }
  }
  const verified = plan.touchpoints.filter((touchpoint) => touchpoint.confidence === "verified");
  const candidates = plan.touchpoints.filter((touchpoint) => touchpoint.confidence === "candidate");
  if (verified.length > 0) {
    lines.push(
      "",
      "Verified",
      ...verified.map(
        (touchpoint) =>
          `- ${touchpoint.path} @ ${touchpoint.revision}${touchpoint.evidence === undefined ? "" : ` (${touchpoint.evidence})`}`,
      ),
    );
  }
  if (candidates.length > 0) {
    lines.push("", "Candidate", ...candidates.map((touchpoint) => `- ${touchpoint.path}`));
  }
  lines.push("", "Steps");
  for (const step of plan.steps) {
    const verification =
      step.verification === undefined || step.verification.length === 0
        ? ""
        : ` [acceptance: ${step.verification.join(", ")}]`;
    lines.push(`${step.id}: ${step.title}${verification}`);
    if (step.description !== undefined) {
      lines.push(`   ${step.description}`);
    }
  }
  if (plan.risks.length > 0) {
    lines.push(
      "",
      "Risks",
      ...plan.risks.map((risk) => `- [${risk.severity}] ${risk.description}`),
    );
  }
  if (plan.constraints.length > 0) {
    lines.push("", "Constraints", ...plan.constraints.map((entry) => `- ${entry.description}`));
  }
  lines.push("", "Validation", ...plan.validation.checks.map((check) => `- ${check}`));
  if (plan.validation.requirements !== undefined && plan.validation.requirements.length > 0) {
    lines.push(
      "",
      "Requirements (descriptive only \u2014 they grant nothing)",
      ...plan.validation.requirements.map((requirement) => `- ${requirement}`),
    );
  }
  if (plan.rollback !== undefined) {
    lines.push("", "Rollback", plan.rollback.description);
  }
  if (plan.rationale !== undefined) {
    lines.push("", "Rationale", plan.rationale);
  }
  if (state !== null) {
    lines.push(
      "",
      `Plan state: ${state.state}${state.staleReason === null ? "" : ` \u2014 ${state.staleReason}`}`,
      `Plan approval: ${state.approval}`,
    );
  }
  return `${lines.join("\n")}\n`;
}

/** Compact planning status block appended to /development-status and /status. */
export function formatPlanningStatus(task: import("@solaris/core").TaskState | null): string {
  if (task === null || task.plan.state === "none") {
    return "Planning: none\n";
  }
  const plan = task.plan;
  return [
    `Planning: ${plan.depth} (plan ${plan.planId} rev ${plan.planRevision})`,
    `Plan state: ${plan.state}${plan.staleReason === null ? "" : ` \u2014 ${plan.staleReason}`}`,
    `Plan approval: ${plan.approval}`,
    "",
  ].join("\n");
}

/** Rendered executor brief with its identity and fingerprint (dry-run surface). */
export function formatExecutorBrief(
  brief: import("@solaris/core").ExecutorBrief,
  fingerprint: string | null,
): string {
  const identity = [
    `Executor brief (${brief.executionContract.id} rev ${brief.executionContract.revision})`,
    `Task: ${brief.taskId} (contract rev ${brief.contractRevision})`,
    brief.milestone === null
      ? null
      : `Milestone: ${brief.milestone.id} rev ${brief.milestone.version}`,
    fingerprint === null ? null : `Fingerprint: ${fingerprint.slice(0, 16)}\u2026`,
  ]
    .filter((line): line is string => line !== null)
    .join("\n");
  return `${identity}\n\n${renderExecutorBrief(brief)}\n`;
}

/** Milestone manifest summary plus its evidence-backed acceptance status. */
export function formatMilestoneManifest(
  manifest: import("@solaris/core").MilestoneManifest,
  report: import("@solaris/core").MilestoneAcceptanceReport | null,
): string {
  const lines = [
    `Milestone ${manifest.id} rev ${manifest.version} \u2014 ${manifest.title}`,
    `Goal: ${manifest.goal}`,
    "",
    "Invariants:",
    ...manifest.invariants.map((invariant) => `- ${invariant.description}`),
    "",
    "Non-goals:",
    ...manifest.nonGoals.map((nonGoal) => `- ${nonGoal}`),
    "",
    "Acceptance:",
  ];
  if (report === null) {
    lines.push("- no task evidence yet (start /task or /develop to attach host evidence)");
  } else {
    for (const requirement of report.requirements) {
      lines.push(
        `- ${requirement.id} [${requirement.status}]${requirement.note === null ? "" : ` \u2014 ${requirement.note}`}`,
      );
    }
    lines.push(
      "",
      `Result: ${report.counts.pass} pass, ${report.counts.fail} fail, ${report.counts.incomplete} incomplete, ${report.counts.not_applicable} n/a${report.passed ? " \u2014 PASSED" : " \u2014 not complete"}`,
    );
  }
  return `${lines.join("\n")}\n`;
}

function formatGodotProbeApprovalPrompt(
  request: Extract<ApprovalRequest, { capability: "godot.probe_project" }>,
): string {
  const preview = request.preview;
  const risks = preview.risks;
  const mirror = preview.mirror;
  const lines = [
    "Godot project probe requires approval",
    "",
    `Project: ${preview.projectName ?? "(unnamed)"}`,
    "",
    "Engine:",
    `  ${preview.engineVersion}`,
    `  ${preview.engineEdition} edition`,
    `  Solaris support: ${preview.support}`,
    `  Static compatibility: ${preview.compatibility}`,
    "",
    "Static risk inventory:",
    `  @tool scripts        ${risks.toolScripts}`,
    `  enabled plugins      ${risks.enabledEditorPlugins}`,
    `  GDExtensions         ${risks.gdextensions}`,
    `  autoloads            ${risks.autoloads}`,
    `  .NET projects        ${risks.dotnetProjects}`,
    "",
    "Probe isolation:",
    `  Source workspace     not used as project (never writable)`,
    `  Disposable mirror    yes (~${formatFileCount(mirror.estimatedFileCount)}, ${formatBytes(mirror.estimatedBytes)})`,
    `  Recovery mode        required`,
    `  Headless editor      yes`,
    `  Network              denied`,
    `  Provider secrets     removed`,
    `  Runtime game         disabled`,
    `  Project scripts      recovery-mode restricted`,
    `  Mirror deleted       after probe`,
    "",
    "The probe may cause Godot to import resources inside the disposable mirror.",
    "Recovery mode reduces editor-side execution risk but does not make arbitrary",
    "project data inherently safe; the probe also relies on a disposable mirror",
    "and the OS sandbox.",
    "",
    `Approval is one-time and binds to project risk manifest ${request.digest.slice(0, 8)}.`,
  ];
  return `${lines.join("\n")}\n`;
}

function formatGodotLSPSessionApprovalPrompt(
  request: Extract<ApprovalRequest, { capability: "godot.lsp" }>,
): string {
  const preview = request.preview;
  const project = preview.projectIntelligence;
  const lines = [
    "Godot GDScript language-server session requires approval",
    "",
    "Project:",
    `  ${preview.projectName ?? "(unnamed)"}`,
    "",
    "Engine:",
    `  ${preview.engineVersion}`,
    `  ${preview.engineEdition} edition`,
    `  Solaris support: ${preview.support}`,
    `  Static compatibility: ${preview.compatibility}`,
    "",
    "Project intelligence:",
    `  GDScript files       ${project.gdscriptFiles}`,
    `  @tool scripts        ${project.toolScripts}`,
    `  editor plugins       ${project.editorPlugins}`,
    `  GDExtensions         ${project.gdextensions}`,
    "",
    "Session:",
    "  Source project       disposable mirror",
    "  Godot mode           headless recovery editor",
    "  LSP network          loopback only",
    "  External network     denied",
    "  Source writes        denied",
    "  Provider secrets     removed",
    "  LSP mutations        disabled",
    "",
    "Capabilities requested:",
    "  diagnostics",
    "  hover",
    "  completion",
    "  definition",
    "",
    "Approval applies only to this session and binds to plan",
    `  ${request.digest.slice(0, 8)}.`,
  ];
  return `${lines.join("\n")}\n`;
}

function formatCommandApprovalPrompt(
  request: Extract<ApprovalRequest, { capability: "process.execute" }>,
): string {
  const preview = request.preview;
  const lines = [
    "Command approval required",
    "",
    `Tool: ${request.toolName}`,
    `Runner: ${preview.runnerId}`,
  ];
  if (preview.packageName !== undefined) {
    lines.push(`Package: ${preview.packageName}`);
  }
  if (preview.scriptName !== undefined) {
    lines.push(`Script: ${preview.scriptName}`);
  }
  lines.push(`Working directory: ${preview.workingDirectory}`);
  lines.push("");
  lines.push("Arguments:");
  if (preview.arguments.length === 0) {
    lines.push("  none");
  } else {
    preview.arguments.forEach((argument, index) => {
      lines.push(`  [${index}] "${sanitizeForDisplay(argument)}"`);
    });
  }
  if (preview.repositoryScript !== undefined) {
    lines.push("");
    lines.push("Repository script:");
    for (const scriptLine of preview.repositoryScript.split("\n")) {
      lines.push(`  ${sanitizeForDisplay(scriptLine)}`);
    }
  }
  lines.push("");
  lines.push("Execution:");
  lines.push(`  Workspace access: ${preview.workspaceAccess}`);
  lines.push(`  Network: ${preview.networkAccess}`);
  lines.push(`  Environment: ${preview.environmentPolicy}`);
  lines.push(`  stdin: ${preview.stdinPolicy}`);
  lines.push(`  Timeout: ${formatTimeoutSeconds(preview.timeoutMs)}`);
  lines.push(`  stdout limit: ${formatBytes(preview.stdoutLimitBytes)}`);
  lines.push(`  stderr limit: ${formatBytes(preview.stderrLimitBytes)}`);
  lines.push("");
  if (preview.scriptShellNotice !== undefined) {
    lines.push(preview.scriptShellNotice);
  }
  if (preview.hooksNotice !== undefined) {
    lines.push(preview.hooksNotice);
  }
  lines.push("");
  lines.push(`Approval applies once to command plan ${request.digest.slice(0, 8)}.`);
  return `${lines.join("\n")}\n`;
}
