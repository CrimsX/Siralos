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
  /help         Show this help
  /status       Show provider, session, and workspace status
  /clear        Clear the terminal (conversation is kept)
  /tools        List the available tools
  /sandbox      Show the sandbox backend status
  /permissions  Show capability rules
  /commands     Show command runners and command status
  /cancel       Cancel the running command
  /git-status   Show Git availability and repository status
  /diff         Show a bounded Git diff (working, staged, or head)
  /checkpoints  List recorded recovery checkpoints
  /undo         Undo the latest Solaris mutation (or /undo <checkpoint-id>)
  /exit         Close Solaris
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
    lines.push("Live conformance: not run (use --sandbox-doctor --run-probes)");
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
    const kind = info.capability === "workspace.write" ? "write" : "read-only";
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
  const file = request.preview.files[0];
  const lines = [
    "Approval required",
    "",
    `Tool: ${request.toolName}`,
    `Capability: ${request.capability}`,
    `File: ${file?.path ?? "(none)"}`,
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
    const rename = file.originalPath === null ? "" : ` (from ${file.originalPath})`;
    lines.push(`  ${file.operation} ${file.path}${rename} [${label}]`);
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
        checkpoint.relativePath.padEnd(24),
        formatRelativeTime(created),
      ].join(" "),
    );
  }
  return `${lines.join("\n")}\n`;
}

export function formatUndoOutcome(outcome: UndoOutcome): string {
  switch (outcome.type) {
    case "undone":
      return `\u25CF Checkpoint ${shortenId(outcome.checkpointId)} undone (${outcome.path})\n`;
    case "denied":
      return `\u25CB Undo denied for checkpoint ${shortenId(outcome.checkpointId)} (${outcome.path}).\n`;
    case "cancelled":
      return `\u25CB Undo cancelled for checkpoint ${shortenId(outcome.checkpointId)} (${outcome.path}).\n`;
    case "conflict":
      return `\u2715 Undo conflict for ${outcome.path}: ${outcome.message}\n`;
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

const CONTROL_CHARACTER_PATTERN = new RegExp(`[\u0000-\u001f\u007f]`, "g");

export function sanitizeForDisplay(text: string): string {
  return text.replace(CONTROL_CHARACTER_PATTERN, "\uFFFD");
}

export function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "An unexpected error occurred.";
}
