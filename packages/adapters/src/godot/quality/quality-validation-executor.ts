import type {
  ApprovalReviewer,
  PreparedCommandTool,
  QualityValidationExecutor,
  ToolExecutionResult,
  ValidationRunOutcome,
  ValidationStep,
} from "@solaris/core";
import { PROCESS_RUN_TOOL_NAME } from "@solaris/core";

/**
 * Validation executor over the existing sandboxed command machinery
 * (ADR 0013 §22). A project-defined validation command is untrusted
 * executable project content: the exact command is shown with the exact
 * repository script body and requires its own exact one-time process
 * approval; execution stays sandboxed (read-only workspace, denied
 * network, closed stdin, bounded output). A denied, cancelled, or
 * infrastructure-unavailable step is never reported as passed.
 *
 * The runner is fail-closed at this stage exactly like every other
 * process surface: when the underlying runner reports `unavailable`,
 * preparation refuses before any approval and the outcome is
 * `unavailable` — never `passed`.
 */
export function createQualityValidationExecutor(options: {
  readonly processTool: PreparedCommandTool;
  readonly reviewer: ApprovalReviewer;
  readonly idFactory?: () => string;
}): QualityValidationExecutor {
  const idFactory =
    options.idFactory ?? (() => `validation-${Math.random().toString(36).slice(2, 10)}`);
  return {
    async run(step: ValidationStep, signal?: AbortSignal): Promise<ValidationRunOutcome> {
      if (signal?.aborted) {
        return {
          step,
          status: "unavailable",
          exitCode: null,
          summary: "cancelled before execution",
        };
      }
      if (step.command === undefined) {
        return { step, status: "not_applicable", exitCode: null, summary: "intrinsic gate" };
      }
      const input: unknown =
        step.command.runner === "npm-script"
          ? {
              runner: "npm-script",
              script: step.command.scriptName ?? "",
              arguments: step.command.arguments,
              workingDirectory: step.command.workingDirectory,
            }
          : {
              runner: "node-script",
              path: step.command.path ?? "",
              arguments: step.command.arguments,
              workingDirectory: step.command.workingDirectory,
            };
      const prepared = await options.processTool.prepare(input, {
        ...(signal === undefined ? {} : { signal }),
      });
      if (prepared.status !== "ready") {
        if (prepared.status === "unavailable") {
          return {
            step,
            status: "unavailable",
            exitCode: null,
            summary: `the ${step.command.runner} runner is unavailable: ${prepared.message}`,
          };
        }
        if (prepared.status === "denied" || prepared.status === "cancelled") {
          return {
            step,
            status: "denied",
            exitCode: null,
            summary: prepared.message,
          };
        }
        return {
          step,
          status: "unavailable",
          exitCode: null,
          summary: `the validation command could not be prepared: ${prepared.message}`,
        };
      }
      const { command, preview, digest } = prepared;
      const decision = await options.reviewer.review(
        {
          id: idFactory(),
          capability: "process.execute",
          toolName: PROCESS_RUN_TOOL_NAME,
          summary: `project validation command: ${preview.displayName}`,
          preview,
          digest,
        },
        signal,
      );
      if (decision.type !== "approve_once") {
        return {
          step,
          status: "denied",
          exitCode: null,
          summary:
            decision.type === "cancelled"
              ? "the validation command approval was cancelled"
              : (decision.reason ?? "the validation command was denied"),
        };
      }
      let result: ToolExecutionResult;
      try {
        result = await options.processTool.executePrepared(command, {
          approvedDigest: digest,
          ...(signal === undefined ? {} : { signal }),
        });
      } catch (error: unknown) {
        return {
          step,
          status: "unavailable",
          exitCode: null,
          summary: `the validation command failed to execute: ${error instanceof Error ? error.message : "unknown error"}`,
        };
      }
      return mapExecutionOutcome(step, result);
    },
  };
}

function mapExecutionOutcome(
  step: ValidationStep,
  result: ToolExecutionResult,
): ValidationRunOutcome {
  switch (result.status) {
    case "success": {
      const exitCode = readExitCode(result.output);
      return {
        step,
        status: exitCode === 0 ? "passed" : "failed",
        exitCode,
        summary:
          exitCode === 0 ? `exited with code 0` : `exited with code ${exitCode ?? "unknown"}`,
      };
    }
    case "denied":
      return { step, status: "denied", exitCode: null, summary: result.message };
    case "cancelled":
      return {
        step,
        status: "denied",
        exitCode: null,
        summary: "the validation command was cancelled",
      };
    case "conflict":
    case "timed_out":
    case "output_limit":
    case "sandbox_denied":
    case "sandbox_unavailable":
    case "workspace_violation":
    case "unavailable":
    case "invalid_input":
    case "failed":
      return { step, status: "unavailable", exitCode: null, summary: result.message };
  }
}

function readExitCode(output: unknown): number | null {
  if (typeof output !== "object" || output === null || Array.isArray(output)) {
    return null;
  }
  const value = (output as Record<string, unknown>)["exitCode"];
  return typeof value === "number" ? value : null;
}
