import type {
  CheckpointStore,
  GitInspector,
  RegisteredToolInfo,
  SolarisApplication,
  SolarisSecurity,
  UndoService,
} from "@solaris/core";
import { GitError } from "@solaris/core";
import { parseInput } from "./input/parse-input.js";
import type { StatusView } from "./output.js";
import {
  describeError,
  formatCheckpoints,
  formatGitDiff,
  formatGitStatus,
  formatHelp,
  formatInvalidCommand,
  formatPermissions,
  formatProviderFailure,
  formatSandbox,
  formatSandboxViolation,
  formatStatus,
  formatToolCancelled,
  formatToolCompleted,
  formatToolFailed,
  formatTools,
  formatToolStarted,
  formatUndoOutcome,
  sanitizeForDisplay,
} from "./output.js";

export interface SessionIO {
  ask(prompt: string): Promise<string | null>;
  write(text: string): void;
  clear(): void;
}

export interface SessionInfo {
  readonly workspaceRoot: string;
  readonly tools: readonly RegisteredToolInfo[];
  readonly security: SolarisSecurity;
  readonly git: GitInspector;
  readonly checkpoints: CheckpointStore;
  readonly undo: UndoService;
}

const PROMPT = "> ";

export async function runInteractiveSession(
  io: SessionIO,
  application: SolarisApplication,
  sessionInfo: SessionInfo,
): Promise<number> {
  for (;;) {
    const input = await io.ask(PROMPT);
    if (input === null) {
      return 0;
    }
    const parsed = parseInput(input);
    switch (parsed.type) {
      case "prompt":
        await runPrompt(io, application, parsed.text);
        break;
      case "command":
        switch (parsed.command) {
          case "help":
            io.write(formatHelp());
            break;
          case "status":
            io.write(formatStatus(await buildStatusView(application, sessionInfo)));
            break;
          case "clear":
            io.clear();
            break;
          case "tools":
            io.write(formatTools(sessionInfo.tools, sessionInfo.security));
            break;
          case "sandbox":
            await runSandboxCheck(io, sessionInfo.security);
            break;
          case "permissions":
            io.write(
              formatPermissions(sessionInfo.security.policy, sessionInfo.security.profile.id),
            );
            break;
          case "git-status":
            await runGitStatusCommand(io, sessionInfo);
            break;
          case "diff":
            await runDiffCommand(io, sessionInfo, parsed.args);
            break;
          case "checkpoints":
            io.write(formatCheckpoints(await sessionInfo.checkpoints.list({ limit: 10 })));
            break;
          case "undo":
            await runUndoCommand(io, sessionInfo, parsed.args);
            break;
          case "exit":
            return 0;
        }
        break;
      case "empty":
        break;
      case "invalid_command":
        io.write(formatInvalidCommand(parsed.input));
        break;
    }
  }
}

async function buildStatusView(
  application: SolarisApplication,
  sessionInfo: SessionInfo,
): Promise<StatusView> {
  const inspection = await sessionInfo.git.inspectRepository().catch(() => null);
  const statusResult =
    inspection?.repositoryState === "repository"
      ? await sessionInfo.git.getStatus({}).catch(() => null)
      : null;
  const checkpoints = await sessionInfo.checkpoints.list().catch(() => []);
  const latestApplied = checkpoints.find((checkpoint) => checkpoint.state === "applied");
  return {
    status: application.getStatus(),
    workspaceRoot: sessionInfo.workspaceRoot,
    toolCount: sessionInfo.tools.length,
    providerToolCount: sessionInfo.tools.filter(
      (info) => sessionInfo.security.evaluateCapability(info.capability).decision !== "deny",
    ).length,
    profileId: sessionInfo.security.profile.id,
    gitRepositoryState: inspection?.repositoryState ?? "unavailable",
    gitBranch:
      inspection?.repositoryState === "repository" && statusResult !== null
        ? statusResult.branch.detached
          ? `(detached) ${statusResult.branch.oid ?? "unknown"}`
          : statusResult.branch.head
        : null,
    gitDirtyCount:
      statusResult === null
        ? 0
        : statusResult.changes.length +
          statusResult.conflicts.length +
          statusResult.untracked.length,
    latestCheckpoint: latestApplied === undefined ? null : shortenId(latestApplied.id),
    uncertainCheckpointCount: checkpoints.filter((checkpoint) => checkpoint.state === "uncertain")
      .length,
  };
}

function shortenId(id: string): string {
  return id.length > 12 ? id.slice(0, 12) : id;
}

async function runGitStatusCommand(io: SessionIO, sessionInfo: SessionInfo): Promise<void> {
  try {
    const inspection = await sessionInfo.git.inspectRepository();
    const result =
      inspection.repositoryState === "repository" ? await sessionInfo.git.getStatus({}) : undefined;
    io.write(formatGitStatus(inspection, result));
  } catch (error: unknown) {
    io.write(formatProviderFailure(describeGitFailure(error)));
  }
}

async function runDiffCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  args: readonly string[],
): Promise<void> {
  const scope = args[0] ?? "working";
  if (args.length > 1 || !["working", "staged", "head"].includes(scope)) {
    io.write("Usage: /diff [working|staged|head]\n");
    return;
  }
  try {
    const result = await sessionInfo.git.getDiff({ scope: scope as "working" | "staged" | "head" });
    io.write(formatGitDiff(result));
  } catch (error: unknown) {
    io.write(formatProviderFailure(describeGitFailure(error)));
  }
}

async function runUndoCommand(
  io: SessionIO,
  sessionInfo: SessionInfo,
  args: readonly string[],
): Promise<void> {
  if (args.length > 1) {
    io.write("Usage: /undo [checkpoint-id]\n");
    return;
  }
  io.write(`Undo checkpoint ${args[0] === undefined ? "(latest)" : args[0]}...\n`);
  const outcome = await sessionInfo.undo.undo(args[0]);
  io.write(formatUndoOutcome(outcome));
}

function describeGitFailure(error: unknown): string {
  if (error instanceof GitError) {
    return error.message;
  }
  return describeError(error);
}

async function runSandboxCheck(io: SessionIO, security: SolarisSecurity): Promise<void> {
  for await (const event of security.checkSandbox()) {
    switch (event.type) {
      case "sandbox_check_started":
        io.write("Checking sandbox\u2026\n");
        break;
      case "sandbox_check_completed":
        io.write(formatSandbox(event.status, security.profile));
        break;
      case "sandbox_violation":
        io.write(formatSandboxViolation(event.category, sanitizeForDisplay(event.summary)));
        break;
    }
  }
}

async function runPrompt(
  io: SessionIO,
  application: SolarisApplication,
  text: string,
): Promise<void> {
  try {
    for await (const event of application.sendPrompt(text)) {
      switch (event.type) {
        case "response_started":
          io.write("\n");
          break;
        case "text_delta":
          io.write(event.text);
          break;
        case "response_completed":
          io.write("\n");
          break;
        case "response_cancelled":
          io.write("\n[response cancelled]\n");
          break;
        case "response_failed":
          io.write(`\n${formatProviderFailure(event.message)}`);
          break;
        case "tool_started":
          io.write(formatToolStarted(event.toolName, sanitizeForDisplay(event.displayInput)));
          break;
        case "tool_completed":
          io.write(formatToolCompleted(sanitizeForDisplay(event.summary)));
          break;
        case "tool_failed":
          io.write(formatToolFailed(sanitizeForDisplay(event.message)));
          break;
        case "tool_cancelled":
          io.write(formatToolCancelled());
          break;
        case "tool_awaiting_approval":
          io.write(`  \u23F3 ${event.toolName} awaiting approval\n`);
          break;
        case "approval_requested":
          io.write(`\nApproval required for ${event.toolName} (${event.summary})\n`);
          break;
        case "approval_resolved":
          io.write(
            `  approval ${event.decision === "approved" ? "approved" : event.decision === "denied" ? "denied" : "cancelled"}\n`,
          );
          break;
        case "checkpoint_applied":
          io.write(`\u25CF Checkpoint ${event.checkpointId} recorded (${event.path})\n`);
          break;
      }
    }
  } catch (error: unknown) {
    io.write(`\n${formatProviderFailure(describeError(error))}`);
  }
}
