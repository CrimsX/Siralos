import type { SandboxBackendStatus, SolarisApplication, SolarisSecurity } from "@solaris/core";
import { GitError } from "@solaris/core";
import {
  describeError,
  formatCommands,
  formatGitDiff,
  formatGitStatus,
  formatProviderFailure,
  formatSandbox,
  formatSandboxViolation,
  formatUndoOutcome,
  sanitizeForDisplay,
  type CommandsView,
} from "../output.js";
import type { SessionIO, SessionInfo } from "./session-types.js";

export async function runCommandsCommand(
  io: SessionIO,
  application: SolarisApplication,
  sessionInfo: SessionInfo,
): Promise<void> {
  const [availabilityEntries, backendStatus] = await Promise.all([
    Promise.all(
      sessionInfo.runners.definitions.map(async (runner) => {
        const instance = sessionInfo.runners.get(runner.id);
        const available = (await instance?.isAvailable().catch(() => false)) ?? false;
        return [runner.id, available] as const;
      }),
    ),
    sessionInfo.sandbox.inspect().catch(() => null as SandboxBackendStatus | null),
  ]);
  const availability = Object.fromEntries(availabilityEntries);
  const decision = sessionInfo.security.evaluateCapability("process.execute");
  const view: CommandsView = {
    runners: sessionInfo.runners.definitions,
    runnerAvailability: availability,
    backendStatus,
    processDecision: decision.decision === "deny" ? "denied" : "approval required",
    activeCommandId: application.getStatus().activeCommandId,
    history: application.getCommandHistory(),
  };
  io.write(formatCommands(view));
}

export async function runGitStatusCommand(io: SessionIO, sessionInfo: SessionInfo): Promise<void> {
  try {
    const inspection = await sessionInfo.git.inspectRepository();
    const result =
      inspection.repositoryState === "repository" ? await sessionInfo.git.getStatus({}) : undefined;
    io.write(formatGitStatus(inspection, result));
  } catch (error: unknown) {
    io.write(formatProviderFailure(describeGitFailure(error)));
  }
}

export async function runDiffCommand(
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

export async function runUndoCommand(
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

export async function runSandboxCheck(io: SessionIO, security: SolarisSecurity): Promise<void> {
  for await (const event of security.checkSandbox()) {
    switch (event.type) {
      case "sandbox_check_started":
        io.write("Checking sandbox…\n");
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

function describeGitFailure(error: unknown): string {
  return error instanceof GitError ? error.message : describeError(error);
}
