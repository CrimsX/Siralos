import type {
  ApprovalRequest,
  Capability,
  CapabilityPolicy,
  CommandAuditRecord,
  CommandRunnerDefinition,
  FileCheckpoint,
  GitDiffResult,
  GitStatusResult,
  GitWorkspaceStatus,
  GodotCompatibilityAssessment,
  GodotDiscoveryResult,
  GodotDoctorReport,
  GodotProjectProfile,
  GodotProjectProbeStatus,
  GodotProbePreview,
  GodotRecoveryProbeResult,
  GodotSelectedInstallation,
  RegisteredToolInfo,
  SandboxBackendStatus,
  SandboxProfile,
  SessionStatus,
  SolarisSecurity,
  UndoOutcome,
} from "@solaris/core";
import type { SandboxDoctorReport } from "./bootstrap/sandbox-doctor.js";
import { COMMAND_LIMITS } from "@solaris/core";

const CAPABILITIES: readonly Capability[] = [
  "workspace.read",
  "workspace.write",
  "git.inspect",
  "godot.inspect",
  "godot.probe_project",
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
  /git-status        Show Git availability and repository status
  /diff              Show a bounded Git diff (working, staged, or head)
  /checkpoints       List recorded recovery checkpoints
  /undo              Undo the latest Solaris mutation (or /undo <checkpoint-id>)
  /godot             Show the selected Godot installation and project compatibility
  /godot-installations  Show all discovered Godot installations and selection rationale
  /godot-project     Show the static Godot project profile
  /godot-doctor      Run bounded Godot diagnostics
  /godot-probe       Prepare one recovery-mode Godot project probe (approval required; reports unavailable when the platform cannot bind execution)
  /godot-probe-status  Show the recovery probe capability and last outcome
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
`;
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
  const file = request.preview.files[0];
  const lines = [
    "Approval required",
    "",
    `Tool: ${request.toolName}`,
    `Capability: ${request.capability}`,
    `File: ${file === undefined ? "(none)" : sanitizePathForDisplay(file.path)}`,
    `Change: +${request.preview.totalAddedLines} -${request.preview.totalRemovedLines}`,
    "",
  ];
  if (request.toolName === "solaris.undo") {
    lines.push(`Note: ${request.summary}`);
    lines.push("This restores only the state recorded before the Solaris operation.");
    lines.push("Any later file change will cause a conflict.");
    lines.push("");
  }
  if (file !== undefined) {
    lines.push(sanitizeForDisplay(file.unifiedDiff));
  }
  lines.push("");
  lines.push(`Approval applies once to plan ${request.digest.slice(0, 8)}.`);
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
  ];
  if (report.recoveryProbe.reason !== null) {
    lines.push(`  ${sanitizeForDisplay(report.recoveryProbe.reason)}`);
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
