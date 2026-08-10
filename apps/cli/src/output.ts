import type {
  ApprovalRequest,
  Capability,
  CapabilityPolicy,
  ChangeReviewResult,
  CommandAuditRecord,
  CommandRunnerDefinition,
  DevelopmentQualityReport,
  FileCheckpoint,
  GDScriptDevelopmentPreview,
  GDScriptDevelopmentStatus,
  GDScriptDevelopmentResult,
  GitDiffResult,
  GitStatusResult,
  GitWorkspaceStatus,
  GodotCompatibilityAssessment,
  GDScriptCompletionResult,
  GDScriptDefinitionResult,
  GDScriptHoverResult,
  GDScriptLSPSessionPreview,
  GDScriptQueryOutcome,
  GDScriptSessionStatus,
  GodotDiagnosticPreview,
  GodotDiscoveryResult,
  GodotDoctorReport,
  GodotKnowledgeQueryResult,
  GodotKnowledgeStatus,
  GodotProjectCheckResult,
  GodotProjectProfile,
  GodotProjectProbeStatus,
  GodotProbePreview,
  GodotRecoveryProbeResult,
  GodotSelectedInstallation,
  ProjectionService,
  QualityStatus,
  Reference,
  ReferenceMaterializerPort,
  ReferenceRegistry,
  ReferenceRevision,
  ReferenceSource,
  RegisteredToolInfo,
  ResearchService,
  ResearchSourcePort,
  SandboxBackendStatus,
  SandboxProfile,
  SessionStatus,
  SolarisSecurity,
  TaskState,
  UndoOutcome,
} from "@solaris/core";
import type { SandboxDoctorReport } from "./bootstrap/sandbox-doctor.js";
import { COMMAND_LIMITS, describeInstructionScope, formatReferenceAlias } from "@solaris/core";

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

function yesNo(value: boolean): string {
  return value ? "yes" : "no";
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

function operationMark(operation: "create" | "update" | "delete"): string {
  return operation === "create" ? "A" : operation === "update" ? "M" : "D";
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

export function formatGodotDiagnosticPreview(preview: GodotDiagnosticPreview): string {
  const scripts = preview.scripts;
  const scope =
    scripts.paths !== null && scripts.paths.length > 0
      ? scripts.paths.map((entry) => `  ${sanitizeForDisplay(entry)}`).join("\n")
      : `  ${scripts.count} project scripts`;
  const lines = [
    "GDScript diagnostic probe",
    "",
    "Engine:",
    `  ${preview.engineVersion}`,
    `  ${preview.engineEdition} edition`,
    `  Solaris support: ${preview.support}`,
    `  Static compatibility: ${preview.compatibility}`,
    "",
    "Project:",
    `  ${preview.projectName ?? "(unnamed)"}`,
    "",
    "Scripts:",
    scope,
    `  ${formatBytes(scripts.totalBytes)} total`,
    "",
    "Operation:",
    "  Parse GDScript only (--check-only)",
    "",
    "Project source:",
    "  Disposable mirror",
    "",
    "Game execution:",
    "  disabled",
    "",
    "Scene execution:",
    "  disabled",
    "",
    "Network:",
    "  denied",
    "",
    "Provider credentials:",
    "  absent",
    "",
    "Project modifications:",
    "  none",
    "",
  ];
  return `${lines.join("\n")}\n`;
}

function formatGodotDiagnosticApprovalPrompt(
  request: Extract<ApprovalRequest, { capability: "godot.diagnose" }>,
): string {
  const preview = request.preview;
  const scripts = preview.scripts;
  const scope =
    scripts.paths !== null && scripts.paths.length > 0
      ? scripts.paths.map((entry) => `  ${sanitizeForDisplay(entry)}`).join("\n")
      : `  ${scripts.count} project scripts`;
  const lines = [
    "GDScript diagnostic probe requires approval",
    "",
    "Engine:",
    `  ${preview.engineVersion}`,
    `  ${preview.engineEdition} edition`,
    `  Solaris support: ${preview.support}`,
    `  Static compatibility: ${preview.compatibility}`,
    "",
    "Project:",
    `  ${preview.projectName ?? "(unnamed)"}`,
    "",
    "Scripts:",
    scope,
    `  ${formatBytes(scripts.totalBytes)} total`,
    "",
    "Operation:",
    "  Parse GDScript only (--check-only)",
    "",
    "Isolation:",
    "  Source workspace     not used as project (never writable)",
    "  Disposable mirror    yes",
    "  Game execution       disabled",
    "  Scene execution      disabled",
    "  Network              denied",
    "  Provider credentials absent",
    "  stdin                closed",
    "  Mirror deleted       after check",
    "",
    `Approval is one-time and binds to plan ${request.digest.slice(0, 8)}.`,
  ];
  return `${lines.join("\n")}\n`;
}

export function formatGodotKnowledgeStatus(status: GodotKnowledgeStatus): string {
  if (status.state !== "ready" || status.profile === null) {
    return [
      "Godot API knowledge",
      "",
      `Engine: none selected (${status.platform})`,
      "Knowledge status: unavailable",
      "",
      `Reason: ${sanitizeForDisplay(status.reason ?? "unknown")}`,
      "",
      "Documentation channels:",
      "  Engine API:          exact executable-derived (not generated yet)",
      "  Manual docs:         not locally synchronized",
    ].join("\n");
  }
  const profile = status.profile;
  return [
    "Godot API knowledge",
    "",
    `Engine: ${profile.engine.godotVersion}`,
    `Executable profile: ${profile.engine.executableSha256.slice(0, 16)}`,
    "Knowledge status: ready",
    "API documentation: exact engine-generated",
    `Classes: ${profile.api.classCount} (+${profile.api.builtinClassCount} built-in)`,
    `Methods/utilities: ${profile.api.utilityFunctionCount} utility functions`,
    `Enums: ${profile.api.globalEnumCount} global`,
    `Constants: ${profile.api.globalConstantCount} global`,
    `Symbols indexed: ${profile.index.symbolCount}`,
    `Generated: ${profile.api.generatedAt}`,
    `Manual documentation channel: ${status.manualChannel ?? "unverified"} (not synchronized)`,
    "",
    "Documentation channels:",
    "  Engine API:          exact executable-derived",
    "  Manual docs:         not locally synchronized",
  ].join("\n");
}

export function formatGodotApiSearchResult(result: GodotKnowledgeQueryResult): string {
  if (result.status !== "ready") {
    return `API search unavailable: ${sanitizeForDisplay(result.message)}`;
  }
  const lines = [
    `Godot API search (${result.engineVersion})`,
    "",
    ...result.results.map((entry) => {
      const location = entry.owner === null ? entry.name : `${entry.owner}.${entry.name}`;
      const summary = entry.summary.length > 0 ? ` - ${sanitizeForDisplay(entry.summary)}` : "";
      return `  ${entry.rank.padEnd(8)} ${entry.kind.padEnd(9)} ${location}${summary}`;
    }),
  ];
  if (result.results.length === 0) {
    lines.push("  (no results)");
  } else if (result.truncated) {
    lines.push("  (results truncated)");
  }
  return `${lines.join("\n")}\n`;
}

export function formatGodotDiagnosticsResult(result: GodotProjectCheckResult): string {
  if (result.status !== "checked") {
    return `GDScript diagnostics unavailable: ${sanitizeForDisplay(result.message)}`;
  }
  const lines = [
    "GDScript diagnostics",
    "",
    `Scripts checked: ${result.scriptsChecked}`,
    `Valid: ${result.validCount}`,
    `Invalid: ${result.invalidCount}`,
    "",
  ];
  let shown = 0;
  let currentPath: string | null = null;
  for (const diagnostic of result.diagnostics) {
    if (shown >= 25) {
      lines.push(`  ... ${result.diagnostics.length - shown} more diagnostics`);
      break;
    }
    shown += 1;
    if (diagnostic.path !== currentPath) {
      currentPath = diagnostic.path;
      lines.push(sanitizeForDisplay(diagnostic.path ?? "(unknown file)"));
    }
    const location =
      diagnostic.line === null
        ? ""
        : diagnostic.column === null
          ? `${diagnostic.line}`
          : `${diagnostic.line}:${diagnostic.column}`;
    lines.push(
      `  ${diagnostic.severity.toUpperCase().padEnd(7)} ${location.padEnd(9)} ${sanitizeForDisplay(diagnostic.message)}`,
    );
  }
  if (shown === 0) {
    lines.push("  (no diagnostics)");
  } else if (result.truncated) {
    lines.push("  (diagnostics truncated)");
  }
  return `${lines.join("\n")}\n`;
}

export function formatGodotLSPSessionStatus(status: GDScriptSessionStatus): string {
  if (status.state !== "ready") {
    return (
      [
        "Godot GDScript language session",
        "",
        "Status: inactive",
        `Network isolation: ${status.networkIsolation}`,
      ].join("\n") + "\n"
    );
  }
  const ageMs = status.startedAtMs === null ? null : Date.now() - status.startedAtMs;
  return (
    [
      "Godot GDScript language session",
      "",
      "Status: active",
      `Engine: ${status.engineVersion ?? "unknown"}`,
      `Project: ${status.projectName ?? "(unnamed)"}`,
      `Session age: ${ageMs === null ? "unknown" : `${Math.floor(ageMs / 1000)}s`}`,
      `Idle time: ${status.idleMs === null ? "unknown" : `${Math.floor(status.idleMs / 1000)}s`}`,
      "Capabilities:",
      `  diagnostics  ${yesNo(status.capabilities.diagnostics)}`,
      `  hover        ${yesNo(status.capabilities.hover)}`,
      `  completion   ${yesNo(status.capabilities.completion)}`,
      `  definition   ${yesNo(status.capabilities.definition)}`,
      `Open documents: ${status.openDocumentCount}`,
      `Diagnostics: ${status.diagnosticCount}`,
      `Network isolation: ${status.networkIsolation}`,
    ].join("\n") + "\n"
  );
}

export function formatGodotLSPSessionPreview(preview: GDScriptLSPSessionPreview): string {
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
  ];
  return `${lines.join("\n")}\n`;
}

export function formatGodotHoverResult(result: GDScriptQueryOutcome<GDScriptHoverResult>): string {
  if (result.status !== "ready") {
    return `Hover unavailable: ${sanitizeForDisplay(result.message)}`;
  }
  const range = result.result.range;
  const lines = [
    `Hover: ${sanitizeForDisplay(result.result.path)}`,
    ...(range === null
      ? []
      : [`Range: ${range.start.line}:${range.start.column}-${range.end.line}:${range.end.column}`]),
    ...result.result.contents.map((section) => sanitizeForDisplay(section.text)),
  ];
  return `${lines.join("\n")}\n`;
}

export function formatGodotCompletionResult(
  result: GDScriptQueryOutcome<GDScriptCompletionResult>,
): string {
  if (result.status !== "ready") {
    return `Completion unavailable: ${sanitizeForDisplay(result.message)}`;
  }
  const lines = [
    `Completion: ${sanitizeForDisplay(result.result.path)}`,
    ...result.result.items.slice(0, 50).map((item) => {
      const detail = item.detail === null ? "" : ` - ${sanitizeForDisplay(item.detail)}`;
      return `  ${sanitizeForDisplay(item.label)}${detail}`;
    }),
  ];
  if (result.result.items.length === 0) {
    lines.push("  (no results)");
  } else if (result.result.truncated) {
    lines.push("  (results truncated)");
  }
  return `${lines.join("\n")}\n`;
}

export function formatGodotDefinitionResult(
  result: GDScriptQueryOutcome<GDScriptDefinitionResult>,
): string {
  if (result.status !== "ready") {
    return `Definition unavailable: ${sanitizeForDisplay(result.message)}`;
  }
  const lines = [
    `Definition: ${sanitizeForDisplay(result.result.path)}`,
    ...result.result.locations.slice(0, 25).map((location) => {
      const marker = location.external ? " (external)" : "";
      return `  ${sanitizeForDisplay(location.path)}:${location.range.start.line}:${location.range.start.column}${marker}`;
    }),
  ];
  if (result.result.locations.length === 0) {
    lines.push("  (no locations)");
  } else if (result.result.truncated) {
    lines.push("  (results truncated)");
  }
  return `${lines.join("\n")}\n`;
}

function formatFileCount(count: number): string {
  return count.toLocaleString("en-US");
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

function formatTimeoutSeconds(timeoutMs: number): string {
  if (timeoutMs % 1000 === 0) {
    return `${timeoutMs / 1000} seconds`;
  }
  return `${(timeoutMs / 1000).toFixed(1)} seconds`;
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024) {
    const value = bytes / (1024 * 1024);
    return `${value} MiB`;
  }
  return `${Math.round(bytes / 1024)} KiB`;
}

export function formatGitStatus(inspection: GitWorkspaceStatus, result?: GitStatusResult): string {
  const lines = [
    `Git: ${inspection.gitAvailable ? "available" : "unavailable"}`,
    `Version: ${inspection.gitVersion ?? "unknown"}`,
    `Repository: ${inspection.repositoryState}`,
  ];
  if (inspection.message !== undefined) {
    lines.push(`Note: ${inspection.message}`);
  }
  if (inspection.repositoryState === "repository" && result !== undefined) {
    const { branch } = result;
    const branchLine = branch.detached
      ? `Branch: (detached) ${branch.oid ?? "unknown"}`
      : `Branch: ${branch.head}${branch.unborn ? " (unborn)" : ""}`;
    lines.push(branchLine);
    if (branch.upstream !== null) {
      lines.push(
        `Upstream: ${branch.upstream}${branch.ahead !== null || branch.behind !== null ? ` (ahead ${branch.ahead ?? 0}, behind ${branch.behind ?? 0})` : ""}`,
      );
    }
    const stagedCount = result.changes.filter(
      (change) => change.indexStatus !== "unmodified",
    ).length;
    const unstagedCount = result.changes.filter(
      (change) => change.worktreeStatus !== "unmodified",
    ).length;
    lines.push(`Staged: ${stagedCount}`);
    lines.push(`Unstaged: ${unstagedCount}`);
    lines.push(`Conflicts: ${result.conflicts.length}`);
    lines.push(`Untracked: ${result.untracked.length}`);
    if (result.truncated) {
      lines.push("Note: status output was truncated.");
    }
  }
  return `${lines.join("\n")}\n`;
}

export function formatGitDiff(result: GitDiffResult): string {
  const lines = [`Scope: ${result.scope}`, `Files: ${result.files.length}`];
  for (const file of result.files) {
    const label = file.binary ? "binary" : `+${file.addedLines} -${file.removedLines}`;
    const rename =
      file.originalPath === null ? "" : ` (from ${sanitizePathForDisplay(file.originalPath)})`;
    lines.push(`  ${file.operation} ${sanitizePathForDisplay(file.path)}${rename} [${label}]`);
  }
  lines.push("");
  if (result.patch.length > 0) {
    lines.push(sanitizeForDisplay(result.patch));
  }
  if (result.truncated) {
    lines.push("Note: the diff was truncated by the output limit.");
  }
  if (result.untrackedExcluded) {
    lines.push("Note: untracked file contents are excluded; use git.status and workspace.read.");
  }
  return `${lines.join("\n")}\n`;
}

export function formatCheckpoints(checkpoints: readonly FileCheckpoint[]): string {
  if (checkpoints.length === 0) {
    return "No checkpoints recorded.\n";
  }
  const lines = ["ID          STATE       OPERATION  PATH                     CREATED"];
  for (const checkpoint of checkpoints) {
    const created = new Date(checkpoint.createdAt);
    lines.push(
      [
        shortenId(checkpoint.id),
        checkpoint.state.padEnd(11),
        checkpoint.operation.padEnd(10),
        sanitizePathForDisplay(checkpoint.relativePath).padEnd(24),
        formatRelativeTime(created),
      ].join(" "),
    );
  }
  return `${lines.join("\n")}\n`;
}

export function formatUndoOutcome(outcome: UndoOutcome): string {
  const path = sanitizePathForDisplay(outcome.path);
  switch (outcome.type) {
    case "undone":
      return `\u25CF Checkpoint ${shortenId(outcome.checkpointId)} undone (${path})\n`;
    case "denied":
      return `\u25CB Undo denied for checkpoint ${shortenId(outcome.checkpointId)} (${path}).\n`;
    case "cancelled":
      return `\u25CB Undo cancelled for checkpoint ${shortenId(outcome.checkpointId)} (${path}).\n`;
    case "conflict":
      return `\u2715 Undo conflict for ${path}: ${outcome.message}\n`;
    case "failed":
      return `\u2715 Undo failed: ${outcome.message}\n`;
  }
}

function shortenId(id: string): string {
  return id.length > 12 ? `${id.slice(0, 12)}` : id;
}

function formatRelativeTime(date: Date): string {
  const seconds = Math.max(0, Math.floor((Date.now() - date.getTime()) / 1000));
  if (seconds < 60) {
    return `${seconds}s ago`;
  }
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) {
    return `${minutes}m ago`;
  }
  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    return `${hours}h ago`;
  }
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

export function formatInvalidCommand(input: string): string {
  return `Unknown command: ${input}
Type /help for the list of available commands.
`;
}

export function formatProviderFailure(message: string): string {
  return `[error] ${message}
`;
}

export interface CommandsView {
  readonly runners: readonly CommandRunnerDefinition[];
  readonly runnerAvailability: Readonly<Record<string, boolean>>;
  readonly backendStatus: SandboxBackendStatus | null;
  readonly processDecision: string;
  readonly activeCommandId: string | null;
  readonly history: readonly CommandAuditRecord[];
}

export function formatCommands(view: CommandsView): string {
  const lines = ["RUNNER       STATUS       SECURITY"];
  for (const runner of view.runners) {
    const availability = view.runnerAvailability[runner.id] === true ? "available" : "unavailable";
    lines.push(
      `${runner.id.padEnd(12)} ${availability.padEnd(12)} approval, read-only workspace, offline`,
    );
  }
  lines.push("");
  if (view.backendStatus === null) {
    lines.push("Sandbox: unavailable");
  } else {
    lines.push(`Sandbox: ${view.backendStatus.backendId} (${view.backendStatus.state})`);
  }
  lines.push(`Process execution: ${view.processDecision}`);
  lines.push(`Active command: ${view.activeCommandId ?? "none"}`);
  lines.push(`Default timeout: ${formatTimeoutSeconds(COMMAND_LIMITS.defaultTimeoutMs)}`);
  lines.push(`stdout limit: ${formatBytes(COMMAND_LIMITS.stdoutHardLimitBytes)}`);
  lines.push(`stderr limit: ${formatBytes(COMMAND_LIMITS.stderrHardLimitBytes)}`);
  lines.push("");
  lines.push("Recent commands:");
  if (view.history.length === 0) {
    lines.push("  none");
  } else {
    for (const record of view.history.slice(-5)) {
      const duration =
        record.durationMs === null ? "unknown duration" : formatDuration(record.durationMs);
      const exit = record.exitCode === null ? "no exit" : `exit ${record.exitCode}`;
      lines.push(
        `  [${shortenCommandId(record.commandId)}] ${record.summary} \u2014 ${record.outcome} (${exit}, ${duration})`,
      );
    }
  }
  return `${lines.join("\n")}\n`;
}

export function formatCommandStarted(displayName: string, digestPrefix: string): string {
  return `\u25CF ${sanitizeForDisplay(displayName)} (plan ${digestPrefix})
`;
}

export function formatCommandCompleted(exitCode: number, durationMs: number): string {
  return `  \u2713 exit ${exitCode} in ${formatDuration(durationMs)}
`;
}

export function formatCommandTerminal(
  type:
    | "command_denied"
    | "command_conflict"
    | "command_cancelled"
    | "command_timed_out"
    | "command_failed",
  message: string,
): string {
  const label =
    type === "command_denied"
      ? "denied"
      : type === "command_conflict"
        ? "conflict"
        : type === "command_cancelled"
          ? "cancelled"
          : type === "command_timed_out"
            ? "timed out"
            : "failed";
  return `  \u2715 ${label}: ${sanitizeForDisplay(message)}
`;
}

export function formatNoActiveCommand(): string {
  return "  No command is active.\n";
}

export function formatCancelReport(): string {
  return "  Command cancelled.\n";
}

function shortenCommandId(id: string): string {
  return id.length > 8 ? `${id.slice(0, 8)}\u2026` : id;
}

function formatDuration(durationMs: number): string {
  if (durationMs < 1000) {
    return `${durationMs}ms`;
  }
  return `${(durationMs / 1000).toFixed(1)}s`;
}

export function formatToolStarted(toolName: string, displayInput: string): string {
  return `\u25CF ${toolName} ${displayInput}
`;
}

export function formatToolCompleted(summary: string): string {
  return `  ${summary}
`;
}

export function formatToolFailed(message: string): string {
  return `  \u2715 ${message}
`;
}

export function formatToolCancelled(): string {
  return "  \u2715 cancelled\n";
}

export function formatGodotSummary(
  selected: GodotSelectedInstallation | null,
  compatibility: GodotCompatibilityAssessment,
  projectDetected: boolean,
): string {
  const lines: string[] = ["Godot:"];
  if (selected === null) {
    lines.push("  Selected installation: none");
  } else {
    lines.push(`  Selected installation: ${selected.installationId}`);
    lines.push(`  Executable: ${selected.sourceLabel}`);
    lines.push(`  Version: ${selected.version.raw}`);
    lines.push(`  Edition: ${selected.edition}`);
    lines.push(`  Release channel: ${selected.releaseChannel}`);
    lines.push(`  Solaris support: ${selected.support}`);
  }
  lines.push(`  Project detected: ${projectDetected ? "yes" : "no"}`);
  if (projectDetected) {
    lines.push(`  Compatibility: ${compatibility.status} (${compatibility.severity})`);
  }
  if (selected !== null) {
    lines.push("  Capabilities:");
    lines.push(`    editor                  ${yesNo(selected.capabilities.editor)}`);
    lines.push(`    headless                ${yesNo(selected.capabilities.headless)}`);
    lines.push(`    recovery mode           ${yesNo(selected.capabilities.recoveryMode)}`);
    lines.push(`    import                  ${yesNo(selected.capabilities.import)}`);
    lines.push(`    GDScript LSP            ${yesNo(selected.capabilities.lsp)}`);
    lines.push(`    GDScript DAP            ${yesNo(selected.capabilities.dap)}`);
    lines.push(`    extension API dump      ${yesNo(selected.capabilities.extensionApiDump)}`);
  }
  lines.push("No project code was executed.");
  lines.push("No project import was performed.");
  return `${lines.join("\n")}\n`;
}

export function formatGodotInstallations(discovery: GodotDiscoveryResult): string {
  const lines: string[] = [];
  if (discovery.candidates.length === 0) {
    lines.push("No Godot installations were discovered.");
  } else {
    lines.push(
      `${"ID".padEnd(14)}${"VERSION".padEnd(24)}${"EDITION".padEnd(18)}${"SOURCE".padEnd(12)}SUPPORT`,
    );
    for (const candidate of discovery.candidates) {
      const marker = candidate.selected ? "*" : " ";
      const version = candidate.version?.raw ?? "-";
      const edition = candidate.edition ?? "-";
      const source = candidate.sourceLabel;
      const support = candidate.support ?? "invalid";
      lines.push(
        `${marker}${candidate.installationId.padEnd(13)}${version.padEnd(24)}${edition.padEnd(18)}${source.padEnd(12)}${support}${candidate.invalid === null ? "" : `  [${candidate.invalid}]`}`,
      );
    }
  }
  const duplicates = discovery.candidates.filter((candidate) => candidate.isDuplicate);
  if (duplicates.length > 0) {
    lines.push(
      `Canonical-path duplicates: ${duplicates.map((candidate) => candidate.installationId).join(", ")}`,
    );
  }
  lines.push("");
  if (discovery.rationale.length > 0) {
    lines.push("Selection rationale:");
    for (const reason of discovery.rationale) {
      lines.push(`  ${reason}`);
    }
  }
  if (discovery.configuration.overrides.length > 0) {
    lines.push(`Overrides: ${discovery.configuration.overrides.join(", ")}`);
  }
  for (const diagnostic of discovery.diagnostics) {
    lines.push(`Warning: ${diagnostic.message}`);
  }
  return `${lines.join("\n")}\n`;
}

export function formatGodotProject(
  project: GodotProjectProfile,
  compatibility: GodotCompatibilityAssessment,
): string {
  if (!project.detected) {
    return "No Godot project detected at the workspace root.\n";
  }
  const lines: string[] = [
    `Project: ${project.name ?? "(unnamed)"}`,
    `Config version: ${project.configVersion ?? "unknown"}`,
    `Declared version: ${project.declaredEngineVersion?.raw ?? "none"}`,
    `Main scene: ${project.mainScene ?? "none"}${project.mainScene === null ? "" : project.mainSceneExists === true ? " (exists)" : project.mainSceneExists === false ? " (missing)" : ""}`,
    `Language profile: ${project.languageProfile}`,
    `Rendering methods: ${project.renderingMethods.length > 0 ? project.renderingMethods.join(", ") : "none"}`,
    `Autoloads: ${project.autoloads.length}`,
    `Enabled plugins: ${project.enabledEditorPlugins.length}`,
    `Tool scripts: ${project.executableContent.toolScripts.length}`,
    `GDExtensions: ${project.executableContent.gdextensionDescriptors.length}`,
    `Compatibility: ${compatibility.status} (${compatibility.severity})`,
  ];
  if (project.executableContent.scanTruncated) {
    lines.push("Scan: truncated (results are partial)");
  }
  for (const warning of project.warnings) {
    lines.push(`${warning.severity === "warning" ? "Warning" : "Note"}: ${warning.message}`);
  }
  lines.push("No project code was executed.");
  lines.push("No project import was performed.");
  return `${lines.join("\n")}\n`;
}

export function formatGodotProbePreview(preview: GodotProbePreview): string {
  const risks = preview.risks;
  const mirror = preview.mirror;
  const lines = [
    "Godot project probe requires approval.",
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
    "Static risk inventory:",
    `  @tool scripts         ${risks.toolScripts}`,
    `  enabled plugins       ${risks.enabledEditorPlugins}`,
    `  GDExtensions          ${risks.gdextensions}`,
    `  autoloads             ${risks.autoloads}`,
    `  .NET projects         ${risks.dotnetProjects}`,
    "",
    "Probe isolation:",
    `  Source workspace     not used as project (never writable)`,
    `  Disposable mirror    yes (~${formatFileCount(mirror.estimatedFileCount)} files, ${formatBytes(mirror.estimatedBytes)})`,
    `  Recovery mode        required`,
    `  Headless editor      yes`,
    `  Network              denied`,
    `  Provider secrets     removed`,
    `  Runtime game         disabled`,
    `  Project scripts      recovery-mode restricted`,
    `  Mirror deleted       after probe`,
    "",
    "The probe may cause Godot to import resources inside the disposable mirror.",
    "",
  ];
  return `${lines.join("\n")}\n`;
}

export function formatGodotProbeTerminal(status: string, message: string): string {
  return `  \u2715 ${status}: ${sanitizeForDisplay(message)}
`;
}

export function formatGodotProbeResult(result: GodotRecoveryProbeResult): string {
  const lines = [
    "Recovery probe:",
    `  Status: ${result.status}`,
    `  Engine: ${result.engine.version} (${result.engine.installationId})`,
    `  Recovery mode: ${result.recoveryMode ? "active" : "not used"}`,
    `  Source workspace loaded: no`,
    `  Mirror: ${result.mirror.sourceFiles} files, ${formatBytes(result.mirror.sourceBytes)} copied`,
    `  Generated .godot in mirror: ${result.mirror.generatedGodotDirectory ? `yes (${result.mirror.generatedFiles ?? "?"} files, ${result.mirror.generatedBytes === null ? "unknown" : formatBytes(result.mirror.generatedBytes)})` : "no"}`,
    `  Import state: ${result.mirror.importState}`,
    `  Errors: ${result.diagnostics.errors.length}`,
    `  Warnings: ${result.diagnostics.warnings.length}${result.diagnostics.truncated ? " (truncated)" : ""}`,
    `  Exit: ${result.process.exitCode === null ? "none" : String(result.process.exitCode)} in ${formatDuration(result.process.durationMs)}${result.process.timedOut ? " (timed out)" : ""}`,
    `  Workspace integrity: ${result.workspaceIntegrity.unchanged ? "unchanged" : "changed during probe"}${result.workspaceIntegrity.bounded ? " (bounded baseline)" : ""}`,
    `  Mirror removed: ${result.cleanup.completed ? "yes" : "no"}${result.cleanup.message === undefined ? "" : ` (${result.cleanup.message})`}`,
  ];
  if (result.diagnostics.errors.length > 0) {
    lines.push("  Errors:");
    for (const error of result.diagnostics.errors.slice(0, 10)) {
      lines.push(`    [${error.category}] ${sanitizeForDisplay(error.message)}`);
    }
  }
  if (result.diagnostics.warnings.length > 0) {
    lines.push("  Warnings:");
    for (const warning of result.diagnostics.warnings.slice(0, 10)) {
      lines.push(`    [${warning.category}] ${sanitizeForDisplay(warning.message)}`);
    }
  }
  lines.push("");
  lines.push(
    "Recovery mode reduces editor-side execution risk but does not make arbitrary",
    "project data inherently safe. The probe also relies on a disposable mirror",
    "and the OS sandbox.",
  );
  return `${lines.join("\n")}\n`;
}

export function formatGodotProbeStatus(status: GodotProjectProbeStatus): string {
  const lines = [
    "Project probe:",
    `  Trust state: ${status.state}`,
    `  Manifest digest: ${status.lastManifestDigest === null ? "none" : `${status.lastManifestDigest.slice(0, 12)}\u2026`}`,
    `  Last engine: ${status.lastEngineVersion ?? "none"}`,
  ];
  if (status.lastResult === null) {
    lines.push("  Last result: never run");
  } else {
    const result = status.lastResult;
    lines.push(`  Last result: ${result.status}`);
    lines.push(
      `  Diagnostics: ${result.diagnostics.errors.length} errors, ${result.diagnostics.warnings.length} warnings${result.diagnostics.truncated ? " (truncated)" : ""}`,
    );
    lines.push(
      `  Workspace integrity: ${result.workspaceIntegrity.unchanged ? "unchanged" : "changed"}`,
    );
    lines.push(`  Mirror removed: ${result.cleanup.completed ? "yes" : "no"}`);
  }
  return `${lines.join("\n")}\n`;
}

export function formatGodotDoctor(report: GodotDoctorReport): string {
  const discovery = report.discovery;
  const lines: string[] = [
    "Solaris Godot doctor",
    "",
    "Configuration:",
    `  Active installation: ${discovery.configuration.activeInstallation ?? "none"}`,
    `  Configured installations: ${discovery.configuration.configuredCount}`,
    `  PATH discovery: ${discovery.configuration.discoverOnPath ? "enabled" : "disabled"}`,
    `  Overrides: ${discovery.configuration.overrides.length > 0 ? discovery.configuration.overrides.join(", ") : "none"}`,
    "",
    "Sandbox:",
    `  State: ${report.sandbox.state}`,
    `  Backend: ${report.sandbox.backendId}`,
    `  Network restriction: ${yesNo(report.sandbox.networkRestriction)}`,
    `  Filesystem write restriction: ${yesNo(report.sandbox.filesystemWriteRestriction)}`,
    `  Process-tree restriction: ${yesNo(report.sandbox.processTreeRestriction)}`,
    "",
    "Cache:",
    `  Schema version: ${report.cache.schemaVersion}`,
    `  Cached profiles: ${report.cache.cachedProfileCount}`,
    "",
    `Recovery-mode project probe: ${report.recoveryProbe.state} (${report.recoveryProbe.platform})`,
    `API knowledge: ${report.knowledge.state} (${report.knowledge.platform})`,
    `GDScript diagnostics: ${report.diagnostics.state} (${report.diagnostics.platform})`,
  ];
  if (report.recoveryProbe.reason !== null) {
    lines.push(`  ${sanitizeForDisplay(report.recoveryProbe.reason)}`);
  }
  if (report.knowledge.reason !== null) {
    lines.push(`  ${sanitizeForDisplay(report.knowledge.reason)}`);
  }
  if (report.diagnostics.reason !== null) {
    lines.push(`  ${sanitizeForDisplay(report.diagnostics.reason)}`);
  }
  lines.push("");
  lines.push(formatGodotInstallations(discovery).trimEnd());
  if (report.degradedCapabilities.length > 0) {
    lines.push("");
    lines.push(
      `Degraded capabilities: ${report.degradedCapabilities.join(", ")} (probes ran but did not fully verify)`,
    );
  }
  if (report.project.detected) {
    lines.push("");
    lines.push(formatGodotProject(report.project, report.compatibility).trimEnd());
  } else {
    lines.push("");
    lines.push("Project: not detected (no project.godot at the workspace root)");
  }
  lines.push("");
  lines.push("No project code was executed.");
  lines.push("No project import was performed.");
  return `${lines.join("\n")}\n`;
}

/**
 * One final terminal-rendering boundary. Provider responses, repository
 * filenames, Git output, checkpoint metadata, tool activity, errors, and
 * approval information are all untrusted; every byte that reaches the
 * terminal passes through this sanitizer, which neutralizes C0/C1 controls,
 * ANSI CSI sequences, OSC sequences (including OSC 8 links, title changes,
 * and clipboard writes), carriage-return and backspace rewriting, and DEL.
 * Ordinary Unicode and readable newlines survive. Sequences split across
 * stream chunks are tracked across `push` calls; `flush` drops any dangling
 * sequence so truncation can never leave the terminal inside an active
 * escape sequence.
 */
export class TerminalSanitizer {
  private mode: "normal" | "escape" | "csi" | "osc" | "osc_escape" = "normal";
  /**
   * A high surrogate held back because its low surrogate may arrive in the
   * next chunk. Node encodes each `write` call separately, so a pair split
   * across chunks would otherwise be corrupted into replacement characters;
   * pairing across pushes keeps emoji and other non-BMP text intact.
   */
  private pendingHighSurrogate: string | null = null;

  push(text: string): string {
    let out = "";
    for (const character of text) {
      const code = character.codePointAt(0) ?? 0;
      if (this.pendingHighSurrogate !== null) {
        if (code >= 0xdc00 && code <= 0xdfff) {
          out += this.pendingHighSurrogate + character;
          this.pendingHighSurrogate = null;
          continue;
        }
        out += "\uFFFD";
        this.pendingHighSurrogate = null;
      }
      if (this.mode === "normal" && code >= 0xd800 && code <= 0xdbff) {
        this.pendingHighSurrogate = character;
        continue;
      }
      if (code >= 0xdc00 && code <= 0xdfff) {
        // A lone low surrogate is never valid UTF-16; render it visibly.
        out += "\uFFFD";
        continue;
      }
      switch (this.mode) {
        case "normal": {
          if (character === "\u001b") {
            this.mode = "escape";
          } else if (character === "\n" || character === "\t") {
            out += character;
          } else if (code <= 0x1f) {
            out += caretNotation(code);
          } else if (code === 0x7f) {
            out += "^?";
          } else if (code >= 0x80 && code <= 0x9f) {
            out += "\uFFFD";
          } else {
            out += character;
          }
          break;
        }
        case "escape":
          if (character === "[") {
            this.mode = "csi";
          } else if (character === "]") {
            this.mode = "osc";
          } else {
            this.mode = "normal";
          }
          break;
        case "csi":
          if (character >= "\x40" && character <= "\x7e") {
            this.mode = "normal";
          }
          break;
        case "osc":
          if (character === "\u0007") {
            this.mode = "normal";
          } else if (character === "\u001b") {
            this.mode = "osc_escape";
          }
          break;
        case "osc_escape":
          this.mode = character === "\\" ? "normal" : "osc";
          break;
      }
    }
    return out;
  }

  flush(): string {
    this.mode = "normal";
    const dangling = this.pendingHighSurrogate;
    this.pendingHighSurrogate = null;
    return dangling === null ? "" : "\uFFFD";
  }
}

function caretNotation(code: number): string {
  return `^${String.fromCharCode(code + 0x40)}`;
}

export function sanitizeForDisplay(text: string): string {
  const sanitizer = new TerminalSanitizer();
  return sanitizer.push(text) + sanitizer.flush();
}

/**
 * Renders a path-like single-line field safely. Paths are untrusted: a file
 * or checkpoint path may contain embedded newlines, tabs, carriage returns,
 * or other control characters that would otherwise spoof approval prompts,
 * status lines, or undo output by fabricating additional lines. The
 * sanitizer boundary still applies afterwards; this makes the spoofing
 * vector itself visible instead of structural.
 */
export function sanitizePathForDisplay(path: string | null): string {
  if (path === null) {
    return "(none)";
  }
  let out = "";
  for (const character of path) {
    const code = character.codePointAt(0) ?? 0;
    if (character === "\\") {
      out += "\\\\";
    } else if (character === "\n") {
      out += "\\n";
    } else if (character === "\r") {
      out += "\\r";
    } else if (character === "\t") {
      out += "\\t";
    } else if (code < 0x20) {
      out += caretNotation(code);
    } else if (code === 0x7f) {
      out += "^?";
    } else {
      out += character;
    }
  }
  return out;
}

export function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "An unexpected error occurred.";
}

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
function taskPhaseMark(phase: TaskState["phase"]): string {
  switch (phase) {
    case "prepared":
    case "working":
    case "validating":
    case "reviewing":
      return "\u25CF";
    case "blocked":
      return "\u23F3";
    case "completed":
      return "\u2713";
    case "cancelled":
      return "\u2715";
    case "failed":
      return "\u2717";
  }
}

function describeTaskProgress(state: TaskState["progress"]): string {
  return state.state === "healthy"
    ? "healthy"
    : state.state === "degraded"
      ? `degraded (${state.repeatedActions} repeated actions)`
      : `stalled (${state.repeatedActions} repeated actions)`;
}

/**
 * Host task status projection (Stage 3 milestone 1). The CLI is a
 * read-only client of the authoritative TaskState: it renders a snapshot
 * and the completion-gate evaluation, and never mutates task state.
 */
export function formatTaskStatus(
  task: TaskState,
  completion: { readonly allowed: boolean; readonly missing: readonly string[] },
): string {
  const activeStep = task.steps.find((step) => step.status === "active");
  const pendingSteps = task.steps.filter((step) => step.status !== "completed");
  const criteriaSatisfied = task.acceptance.filter(
    (criterion) => criterion.status === "satisfied",
  ).length;
  const phaseNote =
    task.phase === "blocked" && task.terminalReason !== null
      ? ` \u2014 ${task.terminalReason}`
      : "";
  const stepLines =
    task.steps.length === 0
      ? ["  (no structured steps)"]
      : task.steps.map((step) => {
          const mark =
            step.status === "completed"
              ? "\u2713"
              : step.status === "active"
                ? "\u25B8"
                : step.status === "failed"
                  ? "\u2717"
                  : step.status === "blocked"
                    ? "\u23F3"
                    : "\u00B7";
          return `  ${mark} ${step.id} ${step.status}${step.failedReason !== null ? ` (${step.failedReason})` : ""}`;
        });
  const criterionLines = task.acceptance.map((criterion) => {
    const mark =
      criterion.status === "satisfied"
        ? "\u2713"
        : criterion.status === "failed"
          ? "\u2717"
          : "\u00B7";
    const by = criterion.verifiedBy === null ? "" : ` [${criterion.verifiedBy}]`;
    return `  ${mark} ${criterion.criterionId} ${criterion.status}${by}`;
  });
  const completionLine = completion.allowed
    ? "Completion: allowed"
    : `Completion: NOT allowed (${completion.missing.length} reason${completion.missing.length === 1 ? "" : "s"})`;
  return `Task ${task.taskId} (contract revision ${task.contractRevision})
${taskPhaseMark(task.phase)} Phase: ${task.phase}${phaseNote}
Steps: ${task.steps.length - pendingSteps.length}/${task.steps.length} completed${
    activeStep === undefined ? "" : ` \u2014 active: ${activeStep.id}`
  }
${stepLines.join("\n")}
Acceptance: ${criteriaSatisfied}/${task.acceptance.length} satisfied
${criterionLines.join("\n")}
Validation: ${task.validationStatus}
Review: ${task.reviewStatus}
Progress: ${describeTaskProgress(task.progress)} (${task.progress.usefulObservations} useful observations)
${completionLine}
`;
}

/** Read-only projection observability: sizes, pressure, tool ABI. */
export function formatContextStatus(projection: ProjectionService): string {
  const last = projection.lastProjection();
  if (last === null) {
    return "Context projection: not yet computed (send a prompt first)\n";
  }
  const context = last.contextProjection;
  const stableBytes = context.stableSegments.reduce((sum, segment) => sum + segment.bytes, 0);
  const contextualBytes = context.contextualSegments.reduce(
    (sum, segment) => sum + segment.bytes,
    0,
  );
  const volatileBytes = context.volatileSegments.reduce((sum, segment) => sum + segment.bytes, 0);
  const pressure = last.pressure;
  const tool = last.toolProjection;
  return [
    `Context projection (mode ${last.mode})`,
    `  Stable: ${stableBytes} B (fingerprint ${context.stableFingerprint.slice(0, 8)})`,
    `  Contextual: ${contextualBytes} B`,
    `  Volatile: ${volatileBytes} B`,
    `  Estimated: ${last.estimatedTokens} tokens / ${pressure.workingMaximum} working`,
    `  Pressure: ${pressure.state} (${Math.round(pressure.ratio * 100)}%)`,
    `  Tool ABI: ${tool.fingerprint.slice(0, 8)} (${tool.counts.available} available, ${tool.counts.gated} gated, ${tool.counts.hidden} hidden)`,
    "",
  ].join("\n");
}

/** Compact tool projection summary for /tools. */
export function formatToolProjection(projection: ProjectionService): string {
  const last = projection.lastProjection();
  if (last === null) {
    return "Tool projection: not yet computed\n";
  }
  const tool = last.toolProjection;
  return `Tool projection: ${tool.counts.available} available, ${tool.counts.gated} gated, ${tool.counts.hidden} hidden (ABI ${tool.fingerprint.slice(0, 8)})\n`;
}

/** Read-only /instructions listing (never exposes absolute host paths). */
export function formatInstructions(
  instructions: readonly import("@solaris/core").ProjectInstruction[],
  revision: string | null,
): string {
  if (instructions.length === 0) {
    return "Project instructions: none discovered (no AGENTS.md files inside the workspace)\n";
  }
  const lines = [
    `Project instructions (inventory revision ${revision === null ? "none" : revision.slice(0, 12)}):`,
  ];
  for (const instruction of instructions) {
    const scope = describeInstructionScope(instruction);
    const fileRevision =
      instruction.sourceRevision === null ? "" : ` @ ${instruction.sourceRevision}`;
    const firstLine = instruction.content.split("\n")[0] ?? "";
    lines.push(`- ${scope}${fileRevision}: ${firstLine.slice(0, 60)}`);
  }
  lines.push(
    "",
    "Instructions shape how work is performed; they never grant capabilities or override security policy.",
  );
  return `${lines.join("\n")}\n`;
}

/** Read-only /knowledge listing of current facts (ADR 0017 §36). */
export function formatKnowledge(coordinator: import("@solaris/core").KnowledgeCoordinator): string {
  const facts = coordinator.activeFacts();
  const retired = coordinator.retiredSubjects();
  if (facts.length === 0 && retired.length === 0) {
    return "Project knowledge: none recorded\n";
  }
  const lines = ["Project knowledge:"];
  for (const fact of facts) {
    const subject = fact.subjectKey ?? fact.id;
    const pinned = fact.activation === "pinned" ? ", pinned" : "";
    const expiry =
      fact.expiresAtMs === null ? "" : `, expires ${new Date(fact.expiresAtMs).toISOString()}`;
    lines.push(
      `- ${subject}`,
      `    ${fact.content}`,
      `    revision ${fact.revision}, ${fact.confidence} confidence, ${fact.volatility} volatility${pinned}${expiry}`,
    );
  }
  for (const subject of retired) {
    lines.push(`- ${subject} (retired; revisions retained)`);
  }
  lines.push(
    "",
    "Knowledge is factual context only: it never grants permissions, changes policy, or overrides the task contract.",
  );
  return `${lines.join("\n")}\n`;
}

/** Read-only /knowledge why rendering of the latest retrieval trace. */
export function formatKnowledgeTrace(
  trace: import("@solaris/core").KnowledgeRetrievalTrace | null,
): string {
  if (trace === null) {
    return "Knowledge retrieval: no retrieval has run yet (send a prompt first)\n";
  }
  const query = [
    trace.query.subjectKey === null ? null : `subject=${trace.query.subjectKey}`,
    trace.query.text === null ? null : `text="${trace.query.text.slice(0, 60)}"`,
    trace.query.paths.length === 0 ? null : `paths=${trace.query.paths.join(",")}`,
  ]
    .filter((part): part is string => part !== null)
    .join(" ");
  const lines = [
    `Knowledge retrieval trace (${new Date(trace.atMs).toISOString()})`,
    `  Query: ${query.length === 0 ? "(none)" : query}`,
    `  Considered ${trace.consideredCount} active fact(s); selected ${trace.selected.length}; omitted ${trace.omittedCount}`,
    `  Budget: ${trace.budget.limit} facts / ${trace.budget.maxBytes} bytes (used ${trace.budget.usedBytes})`,
  ];
  for (const selection of trace.selected) {
    lines.push(
      `  - ${selection.subjectKey ?? selection.factId} rev ${selection.revision} (score ${selection.score}, ${selection.confidence}): ${selection.matchReasons.join(", ")}`,
    );
  }
  return `${lines.join("\n")}\n`;
}

/** Render the declared references table (read-only projection). */
export function formatReferences(
  registry: ReferenceRegistry,
  materializer: ReferenceMaterializerPort,
  configError: string | null,
): string {
  const references = registry.list();
  const lines: string[] = [];
  if (configError !== null) {
    lines.push(`References configuration error: ${sanitizeForDisplay(configError)}`);
    lines.push("");
  }
  if (references.length === 0) {
    lines.push(
      configError === null ? "No references are configured." : "No references are available.",
    );
    return `${lines.join("\n")}\n`;
  }
  lines.push(`References (${references.length})`);
  for (const reference of references) {
    lines.push(
      `  ${formatReferenceAlias(reference.alias).padEnd(26)}${reference.kind.padEnd(17)}${materializer.status(reference.id).padEnd(16)}${reference.trust.padEnd(15)}${describeReferenceStatus(reference)}`,
    );
    lines.push(`    source: ${describeReferenceSource(reference.source)}`);
    lines.push(`    revision: ${describeReferenceRevision(registry, reference.id)}`);
    if (reference.description !== null) {
      lines.push(`    description: ${sanitizeForDisplay(reference.description)}`);
    }
  }
  return `${lines.join("\n")}\n`;
}

export function formatReferenceDetail(
  registry: ReferenceRegistry,
  materializer: ReferenceMaterializerPort,
  selector: string,
): string {
  const alias = selector.startsWith("@reference/")
    ? selector.slice("@reference/".length)
    : selector;
  const reference = registry.get(alias as Parameters<ReferenceRegistry["get"]>[0]);
  if (reference === undefined) {
    return `Unknown reference: ${sanitizeForDisplay(selector)}. List configured references with /references.\n`;
  }
  const revision = registry.revision(reference.id);
  const lines = [
    `Reference: ${formatReferenceAlias(reference.alias)}`,
    `Kind: ${reference.kind}`,
    `Description: ${reference.description === null ? "none" : sanitizeForDisplay(reference.description)}`,
    `Source: ${describeReferenceSource(reference.source)}`,
    `Identity: ${describeReferenceIdentity(revision)}`,
    `Materialization: ${materializer.status(reference.id)}`,
    `Trust: ${reference.trust}`,
    `Availability: ${describeReferenceStatus(reference)}`,
    `Resolved revision: ${describeReferenceRevision(registry, reference.id)}`,
  ];
  return `${lines.join("\n")}\n`;
}

export function formatResearchStatus(
  service: ResearchService,
  security: SolarisSecurity,
  sources: readonly ResearchSourcePort[],
): string {
  const decision = security.evaluateCapability("research.fetch");
  const state = decision.decision === "allow" ? "enabled" : "disabled";
  const lines = [`Research: ${state}`];
  if (decision.decision !== "allow") {
    lines.push(`Policy: ${sanitizeForDisplay(decision.reason)}`);
  }
  if (sources.length === 0) {
    lines.push("Sources: none configured");
  } else {
    lines.push(
      `Sources (${sources.length}): ${sources
        .map((source) => `${source.kind} (${sanitizeForDisplay(source.label)})`)
        .join(", ")}`,
    );
  }
  lines.push(`Active requests: ${service.activeRequestCount()}`);
  lines.push(`Recent evidence: ${service.latestEvidence().length}`);
  return `${lines.join("\n")}\n`;
}

function describeReferenceSource(source: ReferenceSource): string {
  if (source.kind === "local-directory") {
    // The path as the user configured it — managed/cache paths are never
    // shown.
    return sanitizeForDisplay(source.path);
  }
  const pin =
    source.ref.kind === "commit"
      ? `commit ${source.ref.commit}`
      : source.ref.kind === "tag"
        ? `tag ${source.ref.tag}`
        : `branch ${source.ref.branch}`;
  return `${sanitizeForDisplay(source.repository)} (${pin})`;
}

function describeReferenceRevision(registry: ReferenceRegistry, id: string): string {
  const revision = registry.revision(id as Parameters<ReferenceRegistry["revision"]>[0]);
  if (revision === null) {
    return "unresolved";
  }
  return revision.identity.kind === "repository"
    ? `commit ${revision.identity.commit}`
    : `fingerprint ${revision.identity.fingerprint}`;
}

function describeReferenceIdentity(revision: ReferenceRevision | null): string {
  if (revision === null) {
    return "unresolved";
  }
  if (revision.identity.kind === "repository") {
    return `${sanitizeForDisplay(revision.identity.origin)} @ commit ${revision.identity.commit}`;
  }
  return `${sanitizeForDisplay(revision.identity.canonicalPath)} (fingerprint ${revision.identity.fingerprint})`;
}

function describeReferenceStatus(reference: Reference): string {
  if (reference.status === "ready") {
    return "ready";
  }
  return `${reference.status}${reference.failureReason === null ? "" : `: ${sanitizeForDisplay(reference.failureReason)}`}`;
}

/** Render a structural read tool result (read-only projection). */
export function formatStructuralRead(result: import("@solaris/core").ToolExecutionResult): string {
  if (result.status !== "success") {
    return "";
  }
  const output = result.output;
  if (typeof output !== "object" || output === null || Array.isArray(output)) {
    return "(no structural data)\n";
  }
  const record = output as Record<string, unknown>;
  const revision = typeof record["revision"] === "string" ? record["revision"] : "none";
  if (record["supported"] === false) {
    return `${record["path"] as string} @ ${revision}: unsupported (${String(record["reason"])})\n`;
  }
  const structure = record["structure"];
  if (typeof structure !== "object" || structure === null) {
    return "(no structure)\n";
  }
  const s = structure as Record<string, unknown>;
  const functions = Array.isArray(s["functions"])
    ? (s["functions"] as Array<Record<string, unknown>>)
    : [];
  const properties = Array.isArray(s["properties"])
    ? (s["properties"] as Array<Record<string, unknown>>)
    : [];
  const signals = Array.isArray(s["signals"])
    ? (s["signals"] as Array<Record<string, unknown>>)
    : [];
  const status = s["status"] === "partial" ? " (partial)" : "";
  const errors = Array.isArray(s["parserErrors"])
    ? (s["parserErrors"] as Array<Record<string, unknown>>)
    : [];
  const lines = [
    `${String(record["path"])} @ ${revision}${status}`,
    `  extends: ${typeof s["extendsType"] === "string" ? s["extendsType"] : "-"}`,
    `  class_name: ${typeof s["className"] === "string" ? s["className"] : "-"}`,
    `  ${signals.length} signals, ${properties.length} properties, ${functions.length} functions`,
    ...(functions.length === 0
      ? []
      : [`  functions: ${functions.map((fn) => String(fn["name"])).join(", ")}`]),
    ...(errors.length === 0
      ? []
      : [
          `  parser errors: ${errors.map((error) => `${String(error["line"])}:${String(error["message"])}`).join(" | ")}`,
        ]),
    ...(s["truncated"] === true ? ["  (declaration cap reached; output truncated)"] : []),
    "",
  ];
  return lines.join("\n");
}

// --- Stage 3 milestone 6: capability doctor + self-reference rendering ---

function doctorStatusMark(status: string): string {
  switch (status) {
    case "pass":
      return "PASS";
    case "warn":
      return "WARN";
    case "fail":
      return "FAIL";
    default:
      return "SKIP";
  }
}

export function formatSolarisDoctorReport(report: import("@solaris/core").DoctorReport): string {
  const lines: string[] = [
    "Solaris Doctor",
    `Solaris ${report.runtime.version} on Node ${report.runtime.nodeMajor} (${report.runtime.platform})`,
    "",
  ];
  const byArea = new Map<string, import("@solaris/core").DoctorCheckResult[]>();
  for (const check of report.checks) {
    const area = byArea.get(check.area) ?? [];
    area.push(check);
    byArea.set(check.area, area);
  }
  for (const area of report.requestedAreas) {
    const checks = byArea.get(area) ?? [];
    const status = checks.some((check) => check.status === "fail")
      ? "FAIL"
      : checks.some((check) => check.status === "warn")
        ? "WARN"
        : checks.some((check) => check.status === "pass")
          ? "PASS"
          : "SKIP";
    lines.push(`${area.padEnd(15)} ${status}`);
  }
  const warnings = report.checks.filter((check) => check.status === "warn");
  const failures = report.checks.filter((check) => check.status === "fail");
  if (report.counts.total > 0) {
    lines.push(
      "",
      `${report.counts.pass} passed, ${report.counts.warn} warning${report.counts.warn === 1 ? "" : "s"}, ${report.counts.fail} failed, ${report.counts.skip} skipped.`,
    );
  }
  const interesting = [...failures, ...warnings];
  if (interesting.length > 0) {
    lines.push("");
    for (const check of interesting) {
      lines.push(`${check.area}: ${check.summary}`);
      for (const detail of check.details ?? []) {
        lines.push(`- ${detail.label}: ${detail.value}`);
      }
    }
  }
  if (report.snapshot !== null) {
    lines.push(
      "",
      `Capability snapshot: ${report.snapshot.providers.length} provider(s), ${report.snapshot.tools.projectedAvailable} tools available, sandbox ${report.snapshot.sandbox.state}, godot ${report.snapshot.godot.state}.`,
    );
  }
  lines.push("", "Exit codes: 0 = no failures, 1 = one or more failures, 2 = invocation error.");
  return `${lines.join("\n")}\n`;
}

export function formatSafeDoctorReport(report: import("@solaris/core").SafeDoctorReport): string {
  const lines = [
    "Solaris Doctor (safe report)",
    `Solaris ${report.runtime.version} on Node ${report.runtime.nodeMajor} (${report.runtime.platform})`,
    `Schema: ${report.schemaVersion}`,
    `Checks: ${report.counts.pass} passed, ${report.counts.warn} warning${report.counts.warn === 1 ? "" : "s"}, ${report.counts.fail} failed, ${report.counts.skip} skipped.`,
  ];
  for (const check of report.checks) {
    if (check.status === "pass" || check.status === "skip") {
      continue;
    }
    lines.push(`[${doctorStatusMark(check.status)}] ${check.area} ${check.id}: ${check.summary}`);
  }
  for (const category of report.errorCategories) {
    lines.push(`Category: ${category.area} ${category.status} x${category.count}`);
  }
  return `${lines.join("\n")}\n`;
}

export function formatSelfReference(self: import("@solaris/core").SelfReference): string {
  const lines = [
    `${self.name} — installed Solaris runtime`,
    `Version: ${self.runtime.version}`,
    `Node major: ${self.runtime.nodeMajor}`,
    `Platform: ${self.runtime.platform}`,
    `Self-reference revision: ${self.revision}`,
    "",
    "Sections (self.read <section>):",
  ];
  for (const section of self.sections) {
    lines.push(`  ${section.id.padEnd(16)} ${section.title}`);
  }
  lines.push(
    "",
    "This is host-owned documentation of the exact installed build; it is read-only, contains no secrets, and is not model training memory.",
  );
  return `${lines.join("\n")}\n`;
}
