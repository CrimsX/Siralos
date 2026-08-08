import {
  COMMAND_LIMITS,
  createPreparedCommand,
  evaluatePermission,
  type CapabilityPolicy,
  type CommandPreparationContext,
  type CommandPreview,
  type CommandRunnerRegistry,
  type CommandToolExecutionContext,
  type CommandToolPreparationResult,
  type GitInspector,
  type GitStatusResult,
  type PreparedCommand,
  type PreparedCommandTool,
  type SandboxBackend,
  type SandboxedProcessResult,
  type SandboxProfile,
  type ToolExecutionContext,
  type ToolExecutionResult,
} from "@solaris/core";
import type { MutationLock } from "../tools/workspace/mutations/mutation-lock.js";
import type { CommandRunPaths } from "@solaris/core";
import type { RunDirectoryProvider } from "./run-directories.js";

export interface ProcessRunToolDependencies {
  readonly workspaceRoot: string;
  readonly runners: CommandRunnerRegistry;
  readonly backend: SandboxBackend;
  readonly runDirectories: RunDirectoryProvider;
  readonly lock: MutationLock;
  /** Optional Git inspector used for workspace immutability verification. */
  readonly git?: GitInspector;
  /** Internal execution profile; never broader than approved file edits. */
  readonly executionProfile: SandboxProfile;
  readonly executionPolicy: CapabilityPolicy;
}

interface ToolPayload {
  readonly runnerId: string;
  readonly runnerCommand: PreparedCommand;
  readonly commandId: string;
  readonly digest: string;
  readonly preview: CommandPreview;
}

const PROVIDER_WINDOW_HEAD_RATIO = 0.45;

export function createProcessRunTool(
  dependencies: ProcessRunToolDependencies,
): PreparedCommandTool {
  const payloads = new WeakMap<PreparedCommand, ToolPayload>();
  let workspaceViolated = false;

  async function prepare(
    input: unknown,
    context: ToolExecutionContext,
  ): Promise<CommandToolPreparationResult> {
    if (context.signal?.aborted) {
      return { status: "cancelled", message: "Preparation was cancelled." };
    }
    const runnerId = readRunnerId(input);
    if (runnerId === null) {
      return {
        status: "invalid_input",
        message: '"runner" must be a supported runner id.',
      };
    }
    const runner = dependencies.runners.get(runnerId);
    if (runner === undefined) {
      return {
        status: "invalid_input",
        message: `Unknown runner "${runnerId}".`,
      };
    }
    const preparationContext: CommandPreparationContext = {
      workspaceRoot: dependencies.workspaceRoot,
      ...(context.signal === undefined ? {} : { signal: context.signal }),
    };
    const result = await runner.prepare(input, preparationContext);
    if (result.status !== "ready") {
      return result;
    }
    const command = createPreparedCommand();
    payloads.set(command, {
      runnerId: result.preview.runnerId,
      runnerCommand: result.command,
      commandId: result.commandId,
      digest: result.digest,
      preview: result.preview,
    });
    return {
      status: "ready",
      command,
      preview: result.preview,
      digest: result.digest,
      commandId: result.commandId,
    };
  }

  async function executePrepared(
    command: PreparedCommand,
    context: CommandToolExecutionContext,
  ): Promise<ToolExecutionResult> {
    const payload = payloads.get(command);
    payloads.delete(command);
    if (payload === undefined) {
      return {
        status: "failed",
        message: "The prepared command is not valid for this tool or has already been used.",
      };
    }
    if (workspaceViolated) {
      return {
        status: "workspace_violation",
        message:
          "Command execution is disabled for this session because a workspace modification was detected despite read-only enforcement.",
      };
    }
    if (context.signal?.aborted) {
      return { status: "cancelled", message: "The command was cancelled." };
    }
    const policyCheck = evaluatePermission(
      "process.execute",
      dependencies.executionPolicy,
      dependencies.executionProfile,
    );
    if (policyCheck.decision === "deny") {
      return { status: "denied", message: policyCheck.reason };
    }
    const backendStatus = await dependencies.backend.inspect().catch(() => null);
    if (backendStatus === null || backendStatus.state !== "available") {
      return {
        status: "sandbox_unavailable",
        message:
          backendStatus?.message ?? "The sandbox backend is unavailable; the command did not run.",
      };
    }
    if (
      !backendStatus.capabilities.filesystemReadRestriction ||
      !backendStatus.capabilities.filesystemWriteRestriction ||
      !backendStatus.capabilities.networkRestriction ||
      !backendStatus.capabilities.processTreeRestriction
    ) {
      return {
        status: "sandbox_unavailable",
        message:
          "The sandbox backend cannot enforce the host-read allowlist, read-only workspace, network denial, and process-tree confinement; the command did not run.",
      };
    }
    let runPaths;
    try {
      runPaths = await dependencies.runDirectories.create();
    } catch (error: unknown) {
      return {
        status: "failed",
        message: `The command run directory could not be prepared: ${describeError(error)}`,
      };
    }
    let result: ToolExecutionResult;
    let failure: unknown;
    try {
      result = await executeWithinLock(payload, context, runPaths);
    } catch (error: unknown) {
      failure = error;
      result = { status: "failed", message: describeError(error) };
    }
    const cleanup = await dependencies.runDirectories.remove(runPaths.runId);
    if (!cleanup.ok) {
      result = attachCleanupWarning(result, cleanup.message);
    }
    if (failure !== undefined) {
      throw failure instanceof Error ? failure : new Error(describeError(failure));
    }
    return result;
  }

  async function executeWithinLock(
    payload: ToolPayload,
    context: CommandToolExecutionContext,
    runPaths: CommandRunPaths,
  ): Promise<ToolExecutionResult> {
    let release: (() => void) | undefined;
    try {
      try {
        release = await dependencies.lock.acquire(context.signal);
      } catch (error: unknown) {
        if (context.signal?.aborted || isAbortError(error)) {
          return {
            status: "cancelled",
            message: "The command was cancelled while waiting for the execution lock.",
          };
        }
        throw error;
      }
      const runner = dependencies.runners.get(payload.runnerId);
      if (runner === undefined) {
        return {
          status: "failed",
          message: `Runner "${payload.runnerId}" is no longer available.`,
        };
      }
      const execution = await runner.toExecutionRequest(payload.runnerCommand, {
        approvedDigest: context.approvedDigest,
        ...(context.signal === undefined ? {} : { signal: context.signal }),
        runPaths,
      });
      if (execution.status !== "ready") {
        return mapRunnerExecutionFailure(execution.status, execution.message);
      }
      if (execution.request.digest !== context.approvedDigest) {
        return {
          status: "conflict",
          message: "The command plan changed after approval.",
        };
      }
      const before = await snapshotGitState(dependencies.git);
      let backendResult: SandboxedProcessResult;
      try {
        backendResult = await dependencies.backend.execute({
          executable: execution.request.executable,
          arguments: execution.request.arguments,
          workingDirectory: execution.request.workingDirectory,
          profile: dependencies.executionProfile,
          environment: execution.request.environment,
          runDirectory: runPaths.root,
          timeoutMs: payload.preview.timeoutMs,
          stdoutLimitBytes: COMMAND_LIMITS.stdoutHardLimitBytes,
          stderrLimitBytes: COMMAND_LIMITS.stderrHardLimitBytes,
          ...(context.signal === undefined ? {} : { signal: context.signal }),
          ...(context.onOutput === undefined ? {} : { onOutput: context.onOutput }),
        });
      } catch (error: unknown) {
        if (context.signal?.aborted || isAbortError(error)) {
          return { status: "cancelled", message: "The command was cancelled." };
        }
        return {
          status: "failed",
          message: `The sandboxed command failed: ${describeError(error)}`,
        };
      }
      const after = await snapshotGitState(dependencies.git);
      if (after.available && before.available && after.state !== before.state) {
        workspaceViolated = true;
        return {
          status: "workspace_violation",
          message:
            "The command modified the workspace despite read-only enforcement. Command execution is disabled for this session. Inspect the workspace before continuing.",
        };
      }
      return mapBackendResult(backendResult, payload, execution.request.digest);
    } finally {
      if (release !== undefined) {
        release();
      }
    }
  }

  return {
    kind: "prepared_command",
    definition: {
      name: "process.run",
      description:
        "Run a validated Solaris development command (an npm package script or a JavaScript file) inside the OS sandbox with a read-only workspace, denied network, and a minimal environment. Every command requires one-time approval.",
      inputSchema: {
        type: "object",
        properties: {
          runner: {
            type: "string",
            enum: ["npm-script", "node-script"],
            description: "The Solaris-owned command runner to use.",
          },
          script: {
            type: "string",
            description: "Existing npm package script name (npm-script only).",
          },
          path: {
            type: "string",
            description: "Workspace-relative JavaScript file path (node-script only).",
          },
          arguments: {
            type: "array",
            items: { type: "string" },
            description: "Bounded argument list passed as separate argv values.",
          },
          workingDirectory: {
            type: "string",
            description: "Workspace-relative directory; defaults to the workspace root.",
          },
          timeoutMs: {
            type: "integer",
            description: "Timeout in milliseconds between 1000 and 600000; defaults to 120000.",
          },
        },
        oneOf: [{ required: ["runner", "script"] }, { required: ["runner", "path"] }],
        additionalProperties: false,
      },
    },
    capability: "process.execute",
    prepare,
    executePrepared,
  };
}

function readRunnerId(input: unknown): string | null {
  if (typeof input !== "object" || input === null || Array.isArray(input)) {
    return null;
  }
  const runner = (input as Record<string, unknown>)["runner"];
  return typeof runner === "string" && runner.length > 0 ? runner : null;
}

function mapRunnerExecutionFailure(
  status: "conflict" | "unavailable" | "failed",
  message: string,
): ToolExecutionResult {
  switch (status) {
    case "conflict":
      return { status: "conflict", message };
    case "unavailable":
      return { status: "unavailable", message };
    case "failed":
      return { status: "failed", message };
  }
}

function attachCleanupWarning(result: ToolExecutionResult, message: string): ToolExecutionResult {
  if (result.status === "success") {
    return {
      ...result,
      output:
        typeof result.output === "object" && result.output !== null && !Array.isArray(result.output)
          ? { ...result.output, cleanupWarning: message }
          : result.output,
    };
  }
  return { ...result, message: `${result.message} Run directory cleanup also failed: ${message}` };
}

function mapBackendResult(
  result: SandboxedProcessResult,
  payload: ToolPayload,
  commandDigest: string,
): ToolExecutionResult {
  switch (result.status) {
    case "completed": {
      const stdout = windowOutput(result.stdout, COMMAND_LIMITS.providerStdoutReturnBytes);
      const stderr = windowOutput(result.stderr, COMMAND_LIMITS.providerStderrReturnBytes);
      const exitCode = result.exitCode ?? 0;
      return {
        status: "success",
        output: {
          status: "completed",
          exitCode,
          stdout: stdout.text,
          stderr: stderr.text,
          stdoutTruncated: result.stdoutTruncated || stdout.truncated,
          stderrTruncated: result.stderrTruncated || stderr.truncated,
          durationMs: result.durationMs,
          runnerId: payload.preview.runnerId,
          commandDigest,
        },
        summary: `Completed ${payload.preview.displayName} (exit ${exitCode}).`,
      };
    }
    case "cancelled":
      return { status: "cancelled", message: "The command was cancelled." };
    case "timed-out":
      return {
        status: "timed_out",
        message: `The command timed out after ${formatSeconds(payload.preview.timeoutMs)}.`,
      };
    case "output-limit":
      return {
        status: "output_limit",
        message: "The command exceeded its output limit and was terminated.",
      };
    case "sandbox-denied":
      return {
        status: "sandbox_denied",
        message: describeSandboxDenial(result),
      };
    case "sandbox-unavailable":
      return {
        status: "sandbox_unavailable",
        message: "The sandbox denied the command because it is unavailable.",
      };
    case "failed":
      if (result.exitCode !== null) {
        const stdout = windowOutput(result.stdout, COMMAND_LIMITS.providerStdoutReturnBytes);
        const stderr = windowOutput(result.stderr, COMMAND_LIMITS.providerStderrReturnBytes);
        return {
          status: "success",
          output: {
            status: "completed",
            exitCode: result.exitCode,
            stdout: stdout.text,
            stderr: stderr.text,
            stdoutTruncated: result.stdoutTruncated || stdout.truncated,
            stderrTruncated: result.stderrTruncated || stderr.truncated,
            durationMs: result.durationMs,
            runnerId: payload.preview.runnerId,
            commandDigest,
          },
          summary: `Completed ${payload.preview.displayName} (exit ${result.exitCode}).`,
        };
      }
      return {
        status: "failed",
        message: "The command could not start inside the sandbox.",
      };
  }
}

function describeSandboxDenial(result: SandboxedProcessResult): string {
  const details = result.violations
    .map((violation) => violation.summary)
    .filter((summary) => summary.length > 0);
  const detailText = details.length > 0 ? `: ${details.join(" | ")}` : "";
  return `The sandbox denied part of the command${detailText}.`;
}

function windowOutput(
  text: string,
  limitBytes: number,
): { readonly text: string; readonly truncated: boolean } {
  if (Buffer.byteLength(text, "utf8") <= limitBytes) {
    return { text, truncated: false };
  }
  const headBytes = Math.floor(limitBytes * PROVIDER_WINDOW_HEAD_RATIO);
  const tailBytes = limitBytes - headBytes;
  const head = sliceUtf8(text, 0, headBytes);
  const tail = sliceUtf8(text, Buffer.byteLength(text, "utf8") - tailBytes);
  return {
    text: `${head}\n\u2026[omitted ${Math.max(0, Buffer.byteLength(text, "utf8") - limitBytes)} bytes]\u2026\n${tail}`,
    truncated: true,
  };
}

function sliceUtf8(text: string, startBytes: number, lengthBytes?: number): string {
  const buffer = Buffer.from(text, "utf8");
  return buffer
    .subarray(startBytes, lengthBytes === undefined ? undefined : startBytes + lengthBytes)
    .toString("utf8");
}

async function snapshotGitState(
  git: GitInspector | undefined,
): Promise<{ readonly available: boolean; readonly state: string | null }> {
  if (git === undefined) {
    return { available: false, state: null };
  }
  try {
    const inspection = await git.inspectRepository();
    if (inspection.repositoryState !== "repository") {
      return { available: false, state: null };
    }
    const status = await git.getStatus({});
    return { available: true, state: canonicalGitStatus(status) };
  } catch {
    return { available: false, state: null };
  }
}

function canonicalGitStatus(status: GitStatusResult): string {
  return JSON.stringify({
    changes: status.changes,
    conflicts: status.conflicts,
    untracked: status.untracked,
    branch: {
      head: status.branch.head,
      oid: status.branch.oid,
    },
  });
}

function formatSeconds(timeoutMs: number): string {
  return `${(timeoutMs / 1000).toFixed(1)} seconds`;
}

function isAbortError(error: unknown): boolean {
  return error instanceof Error && error.name === "AbortError";
}

function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "an unknown error occurred";
}
