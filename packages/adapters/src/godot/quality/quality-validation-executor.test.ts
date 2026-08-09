import { describe, expect, it } from "vitest";
import {
  createPreparedCommand,
  type ApprovalDecision,
  type ApprovalRequest,
  type ApprovalReviewer,
  type CommandToolPreparationResult,
  type PreparedCommandTool,
  type ToolExecutionResult,
  type ValidationStep,
} from "@solaris/core";
import { createQualityValidationExecutor } from "./quality-validation-executor.js";

/**
 * Validation-executor tests (ADR 0013 §22–§23, §30–§34, §90–§98). A
 * project-defined validation command is untrusted executable project
 * content: it must be shown exactly, approved once per exact plan, and
 * its outcome must be represented truthfully (never `passed` when denied
 * or infrastructure-unavailable).
 */

interface RecordingTool {
  readonly tool: PreparedCommandTool;
  readonly preparedInputs: unknown[];
  readonly executed: { digest: string; onOutput: boolean }[];
}

function createRecordingProcessTool(options: {
  readonly prepareStatus?: "ready" | "unavailable";
  readonly executionResult?: ToolExecutionResult;
}): RecordingTool {
  const preparedInputs: unknown[] = [];
  const executed: { digest: string; onOutput: boolean }[] = [];
  const tool: PreparedCommandTool = {
    kind: "prepared_command",
    definition: {
      name: "process.run",
      description: "recorded",
      inputSchema: { type: "object" },
    },
    capability: "process.execute",
    prepare(input): Promise<CommandToolPreparationResult> {
      preparedInputs.push(input);
      if (options.prepareStatus === "unavailable") {
        return Promise.resolve({ status: "unavailable", message: "runner unavailable" });
      }
      const command = createPreparedCommand();
      return Promise.resolve({
        status: "ready",
        command,
        preview: {
          runnerId: "npm-script",
          displayName: "npm run check",
          workingDirectory: ".",
          executableIdentity: "node",
          arguments: ["run", "check"],
          timeoutMs: 120_000,
          stdoutLimitBytes: 1024 * 1024,
          stderrLimitBytes: 1024 * 1024,
          workspaceAccess: "read-only",
          networkAccess: "denied",
          environmentPolicy: "minimal",
          stdinPolicy: "closed",
        },
        digest: "digest-1",
        commandId: "cmd-1",
      });
    },
    executePrepared(command, context): Promise<ToolExecutionResult> {
      executed.push({ digest: context.approvedDigest, onOutput: context.onOutput !== undefined });
      void command;
      return Promise.resolve(
        options.executionResult ?? {
          status: "success",
          output: { exitCode: 0, durationMs: 10 },
          summary: "exit 0",
        },
      );
    },
  };
  return { tool, preparedInputs, executed };
}

function createReviewerRecording(decisions: readonly ApprovalDecision[]): {
  readonly reviewer: ApprovalReviewer;
  readonly requests: readonly ApprovalRequest[];
} {
  const requests: ApprovalRequest[] = [];
  let index = 0;
  return {
    reviewer: {
      review(request: ApprovalRequest): Promise<ApprovalDecision> {
        requests.push(request);
        const decision = decisions[Math.min(index, decisions.length - 1)];
        index += 1;
        return Promise.resolve(decision ?? { type: "deny", reason: "denied" });
      },
    },
    requests,
  };
}

function npmStep(): ValidationStep {
  return {
    id: "npm-check",
    kind: "npm-script",
    displayName: "npm run check",
    reason: "aggregates validation",
    command: {
      runner: "npm-script",
      scriptName: "check",
      arguments: [],
      workingDirectory: ".",
    },
  };
}

describe("quality validation executor", () => {
  it("prepares the exact command through the existing process runner", async () => {
    const recording = createRecordingProcessTool({});
    const approvals = createReviewerRecording([{ type: "approve_once" }]);
    const executor = createQualityValidationExecutor({
      processTool: recording.tool,
      reviewer: approvals.reviewer,
      idFactory: () => "approval-1",
    });
    const outcome = await executor.run(npmStep());
    expect(outcome.status).toBe("passed");
    expect(outcome.exitCode).toBe(0);
    expect(recording.preparedInputs).toEqual([
      {
        runner: "npm-script",
        script: "check",
        arguments: [],
        workingDirectory: ".",
      },
    ]);
    // The approval bound to the exact prepared digest.
    expect(recording.executed[0]?.digest).toBe("digest-1");
  });

  it("shows the exact command preview with the repository script body in the approval request", async () => {
    const recording = createRecordingProcessTool({});
    const approvals = createReviewerRecording([{ type: "approve_once" }]);
    const executor = createQualityValidationExecutor({
      processTool: recording.tool,
      reviewer: approvals.reviewer,
      idFactory: () => "approval-1",
    });
    await executor.run(npmStep());
    const request = approvals.requests[0];
    expect(request?.capability).toBe("process.execute");
    if (request?.capability !== "process.execute") {
      throw new Error("expected a process-execution approval request");
    }
    expect(request.toolName).toBe("process.run");
    expect(request.preview.runnerId).toBe("npm-script");
    expect(request.preview.arguments).toEqual(["run", "check"]);
    expect(request.preview.workspaceAccess).toBe("read-only");
    expect(request.preview.networkAccess).toBe("denied");
    expect(request.preview.stdinPolicy).toBe("closed");
    expect(request.digest).toBe("digest-1");
  });

  it("represents a nonzero exit code truthfully as failed", async () => {
    const recording = createRecordingProcessTool({
      executionResult: {
        status: "success",
        output: { exitCode: 1, durationMs: 10 },
        summary: "exit 1",
      },
    });
    const approvals = createReviewerRecording([{ type: "approve_once" }]);
    const executor = createQualityValidationExecutor({
      processTool: recording.tool,
      reviewer: approvals.reviewer,
      idFactory: () => "approval-1",
    });
    const outcome = await executor.run(npmStep());
    expect(outcome.status).toBe("failed");
    expect(outcome.exitCode).toBe(1);
  });

  it("never reports passed when the approval was denied", async () => {
    const recording = createRecordingProcessTool({});
    const approvals = createReviewerRecording([{ type: "deny", reason: "user denied" }]);
    const executor = createQualityValidationExecutor({
      processTool: recording.tool,
      reviewer: approvals.reviewer,
      idFactory: () => "approval-1",
    });
    const outcome = await executor.run(npmStep());
    expect(outcome.status).toBe("denied");
    expect(recording.executed).toHaveLength(0);
  });

  it("never reports passed when the approval was cancelled", async () => {
    const recording = createRecordingProcessTool({});
    const approvals = createReviewerRecording([{ type: "cancelled" }]);
    const executor = createQualityValidationExecutor({
      processTool: recording.tool,
      reviewer: approvals.reviewer,
      idFactory: () => "approval-1",
    });
    const outcome = await executor.run(npmStep());
    expect(outcome.status).toBe("denied");
    expect(recording.executed).toHaveLength(0);
  });

  it("never reports passed when the runner is infrastructure-unavailable (no approval is requested)", async () => {
    const recording = createRecordingProcessTool({ prepareStatus: "unavailable" });
    const approvals = createReviewerRecording([{ type: "approve_once" }]);
    const executor = createQualityValidationExecutor({
      processTool: recording.tool,
      reviewer: approvals.reviewer,
      idFactory: () => "approval-1",
    });
    const outcome = await executor.run(npmStep());
    expect(outcome.status).toBe("unavailable");
    expect(approvals.requests).toHaveLength(0);
  });

  it("never reports passed on execution-level infrastructure failures", async () => {
    for (const result of [
      { status: "timed_out", message: "timeout" },
      { status: "output_limit", message: "limit" },
      { status: "sandbox_denied", message: "denied" },
      { status: "sandbox_unavailable", message: "unavailable" },
      { status: "workspace_violation", message: "violation" },
      { status: "conflict", message: "conflict" },
    ] as const) {
      const recording = createRecordingProcessTool({
        executionResult: {
          ...result,
          output: undefined,
          summary: "x",
        } as unknown as ToolExecutionResult,
      });
      const approvals = createReviewerRecording([{ type: "approve_once" }]);
      const executor = createQualityValidationExecutor({
        processTool: recording.tool,
        reviewer: approvals.reviewer,
        idFactory: () => "approval-1",
      });
      const outcome = await executor.run(npmStep());
      expect(outcome.status).toBe("unavailable");
    }
  });

  it("requests one approval per step; a second step asks again", async () => {
    const recording = createRecordingProcessTool({});
    const approvals = createReviewerRecording([{ type: "approve_once" }, { type: "approve_once" }]);
    const executor = createQualityValidationExecutor({
      processTool: recording.tool,
      reviewer: approvals.reviewer,
      idFactory: () => "approval-1",
    });
    await executor.run(npmStep());
    await executor.run(npmStep());
    expect(approvals.requests).toHaveLength(2);
  });

  it("respects cancellation before execution", async () => {
    const recording = createRecordingProcessTool({});
    const approvals = createReviewerRecording([{ type: "approve_once" }]);
    const executor = createQualityValidationExecutor({
      processTool: recording.tool,
      reviewer: approvals.reviewer,
      idFactory: () => "approval-1",
    });
    const controller = new AbortController();
    controller.abort();
    const outcome = await executor.run(npmStep(), controller.signal);
    expect(outcome.status).toBe("unavailable");
    expect(recording.executed).toHaveLength(0);
  });
});
