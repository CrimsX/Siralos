import {
  COMMAND_LIMITS,
  type CommandAuditRecord,
  type CommandRunnerDefinition,
  type FileCheckpoint,
  type GitDiffResult,
  type GitStatusResult,
  type GitWorkspaceStatus,
  type SandboxBackendStatus,
  type UndoOutcome,
} from "@siralos/core";
import { formatBytes, formatDuration, formatTimeoutSeconds } from "./format-utils.js";
import { sanitizeForDisplay, sanitizePathForDisplay } from "./sanitize.js";

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
