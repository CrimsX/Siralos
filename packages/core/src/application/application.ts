import { isCancellationError } from "../domain/cancellation.js";
import type { ConversationItem } from "../domain/conversation.js";
import type { JsonObject, JsonValue } from "../domain/json.js";
import type { ModelProvider } from "../ports/provider.js";
import { evaluatePermission } from "../security/permission-evaluator.js";
import type { ApprovalDecision, ApprovalRequest, ApprovalReviewer } from "../security/approval.js";
import type { CapabilityPolicy } from "../security/capability.js";
import { createDefaultPolicy } from "../security/default-policy.js";
import { INSPECT_PROFILE, type SandboxProfile } from "../security/profile.js";
import type { ProcessOutputEvent } from "../security/sandbox-backend.js";
import type { ToolRegistry } from "../tools/tool-registry.js";
import type { ToolExecutionContext, ToolExecutionResult } from "../tools/tool.js";
import type { PreparedGodotProbe, GodotProbePreview } from "../godot/probe.js";
import type { GodotDiagnosticPreview, PreparedGDScriptCheck } from "../godot/gdscript.js";
import type { GDScriptLSPSessionPreview, PreparedGDScriptSession } from "../godot/lsp.js";
import {
  isPreparedCommandTool,
  isPreparedMutationTool,
  toolCapability,
} from "../tools/prepared-mutation-tool.js";
import { isPreparedProbeTool } from "../tools/prepared-probe-tool.js";
import {
  isPreparedDiagnosticTool,
  type PreparedDiagnosticTool,
} from "../tools/prepared-diagnostic-tool.js";
import {
  isPreparedLSPSessionTool,
  type PreparedLSPSessionTool,
} from "../tools/prepared-lsp-session-tool.js";
import type { PreparedProjectProbeTool } from "../tools/prepared-probe-tool.js";
import type { PreparedCommandTool } from "../commands/command-tool.js";
import type { CommandAuditRecord } from "../commands/command-events.js";
import { MAX_RETAINED_COMMAND_AUDIT_RECORDS } from "../commands/command-events.js";
import type { CommandPreview, PreparedCommand } from "../commands/command-runners.js";
import type { PermissionEvaluation } from "../security/permission-evaluator.js";
import { PROCESS_RUN_TOOL_NAME } from "../commands/command-tool.js";
import type { ProjectionMode, ProjectionService } from "../projection/projection-service.js";
import type { ApplicationEvent } from "./application-events.js";
import { collectProviderTurn, type TurnToolCall } from "./provider-turn.js";
import { executeToolRound } from "./tool-round.js";

export type { ApplicationEvent } from "./application-events.js";
export { PROVIDER_TURN_LIMITS } from "./provider-turn.js";

/**
 * Shared shape of the one-time approval protocol for reviewable Godot
 * tools. The application prepares the plan, asks for one-time approval when
 * the policy says `ask`, and only then executes with the approved digest.
 */
interface ApprovedToolAdapter<THandle, TPreview> {
  readonly capability: "godot.probe_project" | "godot.diagnose" | "godot.lsp";
  prepare(
    input: unknown,
    context: ToolExecutionContext,
  ): Promise<
    | {
        readonly status: "ready";
        readonly handle: THandle;
        readonly preview: TPreview;
        readonly digest: string;
      }
    | {
        readonly status: "unavailable" | "unsupported" | "cancelled" | "invalid_input" | "failed";
        readonly message: string;
      }
  >;
  executePrepared(handle: THandle, context: ToolExecutionContext): Promise<ToolExecutionResult>;
  buildApprovalRequest(
    id: string,
    capability: "godot.probe_project" | "godot.diagnose" | "godot.lsp",
    preview: TPreview,
    digest: string,
  ): ApprovalRequest;
  readonly deniedMessage: string;
  readonly cancelledMessage: string;
}

export interface SessionStatus {
  readonly providerId: string;
  readonly state: "idle" | "responding";
  readonly messageCount: number;
  readonly pendingApproval: boolean;
  readonly activeCommandId: string | null;
}

export interface SiralosApplicationDependencies {
  readonly provider: ModelProvider;
  readonly tools: ToolRegistry;
  readonly policy?: CapabilityPolicy;
  readonly profile?: SandboxProfile;
  readonly reviewer?: ApprovalReviewer;
  readonly maxToolRounds?: number;
  /**
   * Optional application hook fired when a provider turn completes
   * without tool calls (a final assistant response). The development
   * workflow uses it to terminate deterministically when the provider has
   * finished reviewing validation evidence.
   */
  readonly onProviderTurnCompleted?: () => void;
  /**
   * Optional host-owned projection service. When configured, every provider
   * request is projected (context segments, tool visibility, evidence
   * views) and preflighted against the working context budget before the
   * provider is invoked; hard pressure blocks the call entirely. When
   * absent, requests are built as before (raw history + policy-filtered
   * registry) — the default keeps existing consumers unchanged.
   */
  readonly projection?: ProjectionService;
}

export const DEFAULT_MAX_TOOL_ROUNDS = 8;
const MAX_TOOL_ROUNDS = 32;

export interface SiralosApplication {
  sendPrompt(
    text: string,
    signal?: AbortSignal,
    options?: { readonly mode?: ProjectionMode },
  ): AsyncIterable<ApplicationEvent>;

  getStatus(): SessionStatus;

  /**
   * Detached authoritative conversation history.
   *
   * R7.2 differential observability only: the Tool-loop oracle records
   * the final transcript from the same owned history the loop appends
   * to. The returned array is a fresh copy; callers can never mutate
   * application state through it.
   */
  getHistory(): readonly ConversationItem[];

  /** Number of completed Tool Rounds for the most recent prompt. */
  getCompletedToolRounds(): number;

  /** Bounded in-memory metadata for commands that executed this session. */
  getCommandHistory(): readonly CommandAuditRecord[];

  /** Exit code of the most recently completed command, if any. */
  getLastCommandExitCode(): number | null;
}

const MAX_DISPLAY_INPUT_LENGTH = 200;

function normalizeMaxToolRounds(value: number | undefined): number {
  if (value === undefined || !Number.isFinite(value)) {
    return DEFAULT_MAX_TOOL_ROUNDS;
  }
  return Math.min(MAX_TOOL_ROUNDS, Math.max(0, Math.floor(value)));
}

export function createSiralosApplication(
  dependencies: SiralosApplicationDependencies,
): SiralosApplication {
  const history: ConversationItem[] = [];
  const policy = dependencies.policy ?? createDefaultPolicy("inspect");
  const profile = dependencies.profile ?? INSPECT_PROFILE;
  const reviewer = dependencies.reviewer;
  const maxToolRounds = normalizeMaxToolRounds(dependencies.maxToolRounds);
  const toolDefinitions = dependencies.tools
    .definitions()
    .filter((info) => evaluatePermission(info.capability, policy, profile).decision !== "deny")
    .map((info) => info.definition);
  const providerTurnContext = {
    provider: dependencies.provider,
    tools: dependencies.tools,
    toolDefinitions,
    history,
    ...(dependencies.projection === undefined ? {} : { projection: dependencies.projection }),
  };
  let state: "idle" | "responding" = "idle";
  let completedToolRounds = 0;
  let pendingApproval = false;
  let approvalCounter = 0;
  let activeCommandId: string | null = null;
  let lastCommandExitCode: number | null = null;
  const commandHistory: CommandAuditRecord[] = [];

  async function* sendPrompt(
    text: string,
    signal?: AbortSignal,
    options?: { readonly mode?: ProjectionMode },
  ): AsyncIterable<ApplicationEvent> {
    if (state === "responding") {
      throw new Error("Siralos is already responding to a prompt.");
    }
    state = "responding";
    history.push({ type: "user_message", content: text });
    try {
      yield { type: "response_started" };
      let toolRounds = 0;
      for (;;) {
        if (signal?.aborted) {
          yield { type: "response_cancelled" };
          return;
        }
        const turn = yield* collectProviderTurn(providerTurnContext, signal, options?.mode);
        if (turn.kind === "cancelled") {
          yield { type: "response_cancelled" };
          return;
        }
        if (turn.kind === "failed") {
          yield { type: "response_failed", message: turn.message };
          return;
        }
        if (turn.toolCalls.length === 0) {
          if (turn.assistantText.length > 0) {
            history.push({ type: "assistant_message", content: turn.assistantText });
          }
          dependencies.onProviderTurnCompleted?.();
          yield { type: "response_completed" };
          return;
        }
        if (toolRounds >= maxToolRounds) {
          yield {
            type: "response_failed",
            message: `Siralos reached the maximum of ${maxToolRounds} tool rounds; the requested tool round was not executed.`,
          };
          return;
        }
        toolRounds += 1;
        const round = yield* executeToolRound({
          toolCalls: turn.toolCalls,
          execute: runToolCall,
          ...(signal === undefined ? {} : { signal }),
        });
        if (round.kind === "cancelled") {
          history.push(...round.transcript);
          yield { type: "response_cancelled" };
          return;
        }
        completedToolRounds += 1;
        if (turn.assistantText.length > 0) {
          history.push({ type: "assistant_message", content: turn.assistantText });
        }
        history.push(...round.transcript);
      }
    } finally {
      state = "idle";
    }
  }

  async function* runToolCall(
    call: Extract<TurnToolCall, { kind: "execute" }>,
    signal?: AbortSignal,
  ): AsyncGenerator<ApplicationEvent, ToolExecutionResult, void> {
    yield {
      type: "tool_started",
      callId: call.callId,
      toolName: call.toolName,
      displayInput: toDisplayInput(call.input),
    };
    const tool = dependencies.tools.get(call.toolName);
    if (tool === undefined) {
      const message = `Unknown tool: ${call.toolName}.`;
      yield { type: "tool_failed", callId: call.callId, toolName: call.toolName, message };
      return { status: "failed", message };
    }
    // Defense in depth: the projected request schema is the model's only
    // legitimate surface. A provider calling a tool that projection hid is
    // denied here too — visibility is enforced at the schema boundary, and
    // enforcement never relies on projection alone.
    const lastProjection = dependencies.projection?.lastProjection();
    if (lastProjection !== undefined && lastProjection !== null) {
      const projectedNames = new Set(lastProjection.tools.map((info) => info.definition.name));
      if (!projectedNames.has(call.toolName)) {
        const message = `Tool ${call.toolName} is not in the projected tool schema for this session and was denied before execution.`;
        yield { type: "tool_failed", callId: call.callId, toolName: call.toolName, message };
        return { status: "denied", message };
      }
    }
    const capability = toolCapability(tool);
    const permission = evaluatePermission(capability, policy, profile);
    if (permission.decision === "deny") {
      const message = `Capability ${capability} is denied by policy: ${permission.reason}`;
      yield { type: "tool_failed", callId: call.callId, toolName: call.toolName, message };
      return { status: "denied", message };
    }
    if (isPreparedCommandTool(tool)) {
      const result = yield* runPreparedCommandTool(
        tool,
        call.callId,
        call.toolName,
        call.input,
        permission,
        signal,
      );
      yield* emitToolOutcome(call.callId, call.toolName, result);
      return result;
    }
    if (isPreparedProbeTool(tool)) {
      const result = yield* runPreparedProbeTool(
        tool,
        call.callId,
        call.toolName,
        call.input,
        permission,
        signal,
      );
      yield* emitToolOutcome(call.callId, call.toolName, result);
      return result;
    }
    if (isPreparedDiagnosticTool(tool)) {
      const result = yield* runPreparedDiagnosticTool(
        tool,
        call.callId,
        call.toolName,
        call.input,
        permission,
        signal,
      );
      yield* emitToolOutcome(call.callId, call.toolName, result);
      return result;
    }
    if (isPreparedLSPSessionTool(tool)) {
      const result = yield* runPreparedLSPSessionTool(
        tool,
        call.callId,
        call.toolName,
        call.input,
        permission,
        signal,
      );
      yield* emitToolOutcome(call.callId, call.toolName, result);
      return result;
    }
    if (!isPreparedMutationTool(tool)) {
      if (permission.decision === "ask") {
        const message =
          `Capability ${capability} requires approval, but this tool does not support ` +
          "a reviewable preparation protocol; the call was denied without execution.";
        yield { type: "tool_failed", callId: call.callId, toolName: call.toolName, message };
        return { status: "denied", message };
      }
      let result: ToolExecutionResult;
      try {
        result = await tool.execute(call.input, signal === undefined ? {} : { signal });
      } catch (error: unknown) {
        if (signal?.aborted || isCancellationError(error)) {
          result = { status: "cancelled", message: "Tool execution was cancelled." };
        } else {
          result = { status: "failed", message: describeError(error) };
        }
      }
      yield* emitToolOutcome(call.callId, call.toolName, result);
      return result;
    }
    const prepared = await tool.prepare(call.input, signal === undefined ? {} : { signal });
    if (prepared.status !== "ready") {
      if (prepared.status === "cancelled") {
        yield { type: "tool_cancelled", callId: call.callId, toolName: call.toolName };
        return { status: "cancelled", message: prepared.message };
      }
      yield {
        type: "tool_failed",
        callId: call.callId,
        toolName: call.toolName,
        message: prepared.message,
      };
      return { status: prepared.status, message: prepared.message };
    }
    const { mutation, preview, digest } = prepared;
    if (preview.truncated) {
      const message = "The change preview is truncated and cannot be approved.";
      yield { type: "tool_failed", callId: call.callId, toolName: call.toolName, message };
      return { status: "failed", message };
    }
    if (permission.decision === "ask") {
      const requestId = `approval-${(approvalCounter += 1)}`;
      const approvalRequest: ApprovalRequest = {
        id: requestId,
        capability: "workspace.write",
        toolName: call.toolName,
        summary: summarizePreview(preview),
        paths: preview.files.map((file) => file.path),
        preview,
        digest,
      };
      yield {
        type: "approval_requested",
        requestId,
        toolName: call.toolName,
        capability: "workspace.write",
        summary: approvalRequest.summary,
      };
      yield {
        type: "tool_awaiting_approval",
        callId: call.callId,
        toolName: call.toolName,
        requestId,
      };
      pendingApproval = true;
      let decision: ApprovalDecision;
      try {
        decision =
          reviewer === undefined
            ? { type: "deny", reason: "No approval reviewer is available." }
            : await reviewer.review(approvalRequest, signal);
      } catch {
        decision = { type: "deny", reason: "The approval reviewer failed; the change was denied." };
      } finally {
        pendingApproval = false;
      }
      yield {
        type: "approval_resolved",
        requestId,
        decision:
          decision.type === "approve_once"
            ? "approved"
            : decision.type === "deny"
              ? "denied"
              : "cancelled",
      };
      if (decision.type !== "approve_once") {
        if (decision.type === "cancelled") {
          yield { type: "tool_cancelled", callId: call.callId, toolName: call.toolName };
          return { status: "cancelled", message: "The approval was cancelled." };
        }
        const message = decision.reason ?? "The change was denied by the user.";
        yield { type: "tool_failed", callId: call.callId, toolName: call.toolName, message };
        return { status: "denied", message };
      }
    }
    let result: ToolExecutionResult;
    try {
      result = await tool.apply(mutation, {
        ...(signal === undefined ? {} : { signal }),
        approvedDigest: digest,
      });
    } catch (error: unknown) {
      if (signal?.aborted || isCancellationError(error)) {
        result = { status: "cancelled", message: "The mutation was cancelled." };
      } else {
        result = { status: "failed", message: describeError(error) };
      }
    }
    if (result.status === "success") {
      const checkpointId = readCheckpointId(result.output);
      if (checkpointId !== null) {
        const path = preview.files[0]?.path ?? "<unknown>";
        yield { type: "checkpoint_applied", checkpointId, path };
      }
    }
    yield* emitToolOutcome(call.callId, call.toolName, result);
    return result;
  }

  async function* runPreparedProbeTool(
    tool: PreparedProjectProbeTool,
    callId: string,
    toolName: string,
    input: unknown,
    permission: PermissionEvaluation,
    signal?: AbortSignal,
  ): AsyncGenerator<ApplicationEvent, ToolExecutionResult, void> {
    const adapter: ApprovedToolAdapter<PreparedGodotProbe, GodotProbePreview> = {
      capability: "godot.probe_project",
      prepare: async (toolInput, context) => {
        const prepared = await tool.prepare(toolInput, context);
        return prepared.status === "ready"
          ? {
              status: "ready",
              handle: prepared.probe,
              preview: prepared.preview,
              digest: prepared.digest,
            }
          : prepared;
      },
      executePrepared: (handle, context) => tool.executePrepared(handle, context),
      buildApprovalRequest: (id, _capability, preview, digest) => ({
        id,
        capability: "godot.probe_project" as const,
        toolName,
        summary: summarizeProbePreview(preview),
        preview,
        digest,
      }),
      deniedMessage: "The project probe was denied by the user.",
      cancelledMessage: "The project probe approval was cancelled.",
    };
    return yield* runApprovedTool(adapter, callId, toolName, input, permission, signal);
  }

  async function* runPreparedDiagnosticTool(
    tool: PreparedDiagnosticTool,
    callId: string,
    toolName: string,
    input: unknown,
    permission: PermissionEvaluation,
    signal?: AbortSignal,
  ): AsyncGenerator<ApplicationEvent, ToolExecutionResult, void> {
    const adapter: ApprovedToolAdapter<PreparedGDScriptCheck, GodotDiagnosticPreview> = {
      capability: "godot.diagnose",
      prepare: async (toolInput, context) => {
        const prepared = await tool.prepare(toolInput, context);
        return prepared.status === "ready"
          ? {
              status: "ready",
              handle: prepared.check,
              preview: prepared.preview,
              digest: prepared.digest,
            }
          : prepared;
      },
      executePrepared: (handle, context) => tool.executePrepared(handle, context),
      buildApprovalRequest: (id, _capability, preview, digest) => ({
        id,
        capability: "godot.diagnose" as const,
        toolName,
        summary: summarizeDiagnosticPreview(preview),
        preview,
        digest,
      }),
      deniedMessage: "The GDScript check was denied by the user.",
      cancelledMessage: "The GDScript check approval was cancelled.",
    };
    return yield* runApprovedTool(adapter, callId, toolName, input, permission, signal);
  }

  async function* runPreparedLSPSessionTool(
    tool: PreparedLSPSessionTool,
    callId: string,
    toolName: string,
    input: unknown,
    permission: PermissionEvaluation,
    signal?: AbortSignal,
  ): AsyncGenerator<ApplicationEvent, ToolExecutionResult, void> {
    const adapter: ApprovedToolAdapter<PreparedGDScriptSession, GDScriptLSPSessionPreview> = {
      capability: "godot.lsp",
      prepare: async (toolInput, context) => {
        const prepared = await tool.prepare(toolInput, context);
        return prepared.status === "ready"
          ? {
              status: "ready",
              handle: prepared.session,
              preview: prepared.preview,
              digest: prepared.digest,
            }
          : prepared;
      },
      executePrepared: (handle, context) => tool.executePrepared(handle, context),
      buildApprovalRequest: (id, _capability, preview, digest) => ({
        id,
        capability: "godot.lsp" as const,
        toolName,
        summary: summarizeLSPSessionPreview(preview),
        preview,
        digest,
      }),
      deniedMessage: "The Godot language session was denied by the user.",
      cancelledMessage: "The Godot language session approval was cancelled.",
    };
    return yield* runApprovedTool(adapter, callId, toolName, input, permission, signal);
  }

  /**
   * Shared one-time approval protocol for reviewable Godot tools (project
   * probes and GDScript checks). Preparation freezes the plan; under an
   * `ask` policy the application requests one-time approval bound to the
   * exact digest; execution runs only under the approved digest and the
   * tool's own revalidation. Denial, EOF, reviewer failure, timeout, and
   * cancellation all prevent execution; approval is never reusable.
   */
  async function* runApprovedTool<THandle, TPreview>(
    adapter: ApprovedToolAdapter<THandle, TPreview>,
    callId: string,
    toolName: string,
    input: unknown,
    permission: PermissionEvaluation,
    signal?: AbortSignal,
  ): AsyncGenerator<ApplicationEvent, ToolExecutionResult, void> {
    const prepared = await adapter.prepare(input, signal === undefined ? {} : { signal });
    if (prepared.status !== "ready") {
      if (prepared.status === "cancelled") {
        yield { type: "tool_cancelled", callId, toolName };
        return { status: "cancelled", message: prepared.message };
      }
      yield {
        type: "tool_failed",
        callId,
        toolName,
        message: prepared.message,
      };
      return {
        status: prepared.status === "unsupported" ? "failed" : prepared.status,
        message: prepared.message,
      };
    }
    const { handle, preview, digest } = prepared;
    if (permission.decision === "ask") {
      const requestId = `approval-${(approvalCounter += 1)}`;
      const approvalRequest: ApprovalRequest = adapter.buildApprovalRequest(
        requestId,
        adapter.capability,
        preview,
        digest,
      );
      yield {
        type: "approval_requested",
        requestId,
        toolName,
        capability: adapter.capability,
        summary: approvalRequest.summary,
      };
      yield {
        type: "tool_awaiting_approval",
        callId,
        toolName,
        requestId,
      };
      pendingApproval = true;
      let decision: ApprovalDecision;
      try {
        decision =
          reviewer === undefined
            ? { type: "deny", reason: "No approval reviewer is available." }
            : await reviewer.review(approvalRequest, signal);
      } catch {
        decision = {
          type: "deny",
          reason: "The approval reviewer failed; the request was denied.",
        };
      } finally {
        pendingApproval = false;
      }
      yield {
        type: "approval_resolved",
        requestId,
        decision:
          decision.type === "approve_once"
            ? "approved"
            : decision.type === "deny"
              ? "denied"
              : "cancelled",
      };
      if (decision.type !== "approve_once") {
        if (decision.type === "cancelled") {
          yield { type: "tool_cancelled", callId, toolName };
          return { status: "cancelled", message: adapter.cancelledMessage };
        }
        const message = decision.reason ?? adapter.deniedMessage;
        yield { type: "tool_failed", callId, toolName, message };
        return { status: "denied", message };
      }
    }
    let result: ToolExecutionResult;
    try {
      result = await adapter.executePrepared(handle, {
        ...(signal === undefined ? {} : { signal }),
        approvedDigest: digest,
      });
    } catch (error: unknown) {
      if (signal?.aborted || isCancellationError(error)) {
        result = { status: "cancelled", message: adapter.cancelledMessage };
      } else {
        result = { status: "failed", message: describeError(error) };
      }
    }
    return result;
  }

  async function* runPreparedCommandTool(
    tool: PreparedCommandTool,
    callId: string,
    toolName: string,
    input: unknown,
    permission: PermissionEvaluation,
    signal?: AbortSignal,
  ): AsyncGenerator<ApplicationEvent, ToolExecutionResult, void> {
    const prepared = await tool.prepare(input, signal === undefined ? {} : { signal });
    if (prepared.status !== "ready") {
      if (prepared.status === "cancelled") {
        yield { type: "tool_cancelled", callId, toolName };
        return { status: "cancelled", message: prepared.message };
      }
      yield { type: "tool_failed", callId, toolName, message: prepared.message };
      return { status: prepared.status, message: prepared.message };
    }
    const { command, preview, digest, commandId } = prepared;
    yield {
      type: "command_prepared",
      commandId,
      runnerId: preview.runnerId,
      summary: preview.displayName,
    };
    if (permission.decision === "ask") {
      const requestId = `approval-${(approvalCounter += 1)}`;
      const approvalRequest: ApprovalRequest = {
        id: requestId,
        capability: "process.execute",
        toolName: PROCESS_RUN_TOOL_NAME,
        summary: preview.displayName,
        preview,
        digest,
      };
      yield {
        type: "approval_requested",
        requestId,
        toolName: PROCESS_RUN_TOOL_NAME,
        capability: "process.execute",
        summary: approvalRequest.summary,
      };
      yield {
        type: "tool_awaiting_approval",
        callId,
        toolName: PROCESS_RUN_TOOL_NAME,
        requestId,
      };
      pendingApproval = true;
      let decision: ApprovalDecision;
      try {
        decision =
          reviewer === undefined
            ? { type: "deny", reason: "No approval reviewer is available." }
            : await reviewer.review(approvalRequest, signal);
      } catch {
        decision = {
          type: "deny",
          reason: "The approval reviewer failed; the command was denied.",
        };
      } finally {
        pendingApproval = false;
      }
      yield {
        type: "approval_resolved",
        requestId,
        decision:
          decision.type === "approve_once"
            ? "approved"
            : decision.type === "deny"
              ? "denied"
              : "cancelled",
      };
      if (decision.type !== "approve_once") {
        if (decision.type === "cancelled") {
          yield { type: "tool_cancelled", callId, toolName };
          yield {
            type: "command_cancelled",
            commandId,
            message: "The command approval was cancelled.",
          };
          return { status: "cancelled", message: "The command approval was cancelled." };
        }
        const message = decision.reason ?? "The command was denied by the user.";
        yield { type: "command_denied", commandId, message };
        yield { type: "tool_failed", callId, toolName, message };
        return { status: "denied", message };
      }
    }
    const startedAt = Date.now();
    activeCommandId = commandId;
    try {
      yield {
        type: "command_started",
        commandId,
        runnerId: preview.runnerId,
        displayName: preview.displayName,
        digestPrefix: digest.slice(0, 8),
      };
      let result: ToolExecutionResult;
      try {
        result = yield* streamPreparedCommand(tool, command, digest, commandId, signal);
      } catch (error: unknown) {
        if (signal?.aborted || isCancellationError(error)) {
          result = { status: "cancelled", message: "The command was cancelled." };
        } else {
          result = { status: "failed", message: describeError(error) };
        }
      }
      yield* emitCommandOutcome(commandId, preview, digest, startedAt, result);
      return result;
    } finally {
      if (activeCommandId === commandId) {
        activeCommandId = null;
      }
    }
  }

  async function* streamPreparedCommand(
    tool: PreparedCommandTool,
    command: PreparedCommand,
    approvedDigest: string,
    commandId: string,
    signal?: AbortSignal,
  ): AsyncGenerator<ApplicationEvent, ToolExecutionResult, void> {
    const pending: ProcessOutputEvent[] = [];
    const waiters: Array<() => void> = [];
    let result: ToolExecutionResult | undefined;
    let failure: unknown;
    const wake = (): void => {
      const current = waiters.splice(0, waiters.length);
      for (const waiter of current) {
        waiter();
      }
    };
    tool
      .executePrepared(command, {
        approvedDigest,
        ...(signal === undefined ? {} : { signal }),
        onOutput: (event: ProcessOutputEvent) => {
          pending.push(event);
          wake();
        },
      })
      .then(
        (executed: ToolExecutionResult) => {
          result = executed;
          wake();
        },
        (error: unknown) => {
          failure = error;
          wake();
        },
      );
    for (;;) {
      if (pending.length > 0) {
        const event = pending.shift() as ProcessOutputEvent;
        if (event.type === "stdout") {
          yield { type: "command_stdout", commandId, text: event.text };
        } else {
          yield { type: "command_stderr", commandId, text: event.text };
        }
        continue;
      }
      if (result !== undefined) {
        return result;
      }
      if (failure !== undefined) {
        throw failure instanceof Error ? failure : new Error(describeError(failure));
      }
      await new Promise<void>((resolve) => {
        waiters.push(resolve);
      });
    }
  }

  function* emitCommandOutcome(
    commandId: string,
    preview: CommandPreview,
    digest: string,
    startedAt: number,
    result: ToolExecutionResult,
  ): Generator<ApplicationEvent, void, void> {
    switch (result.status) {
      case "success": {
        const output = asJsonObject(result.output);
        const exitCode = typeof output?.["exitCode"] === "number" ? output["exitCode"] : null;
        const durationMs =
          typeof output?.["durationMs"] === "number"
            ? output["durationMs"]
            : Date.now() - startedAt;
        yield { type: "command_completed", commandId, exitCode: exitCode ?? 0, durationMs };
        appendCommandAudit(preview, digest, startedAt, {
          outcome: "completed",
          exitCode,
          durationMs,
          stdoutTruncated: output?.["stdoutTruncated"] === true,
          stderrTruncated: output?.["stderrTruncated"] === true,
        });
        return;
      }
      case "timed_out":
        yield { type: "command_timed_out", commandId, message: result.message };
        appendCommandAudit(preview, digest, startedAt, { outcome: "timed_out" });
        return;
      case "cancelled":
        yield { type: "command_cancelled", commandId, message: result.message };
        appendCommandAudit(preview, digest, startedAt, { outcome: "cancelled" });
        return;
      case "conflict":
        yield { type: "command_conflict", commandId, message: result.message };
        appendCommandAudit(preview, digest, startedAt, { outcome: "conflict" });
        return;
      case "denied":
        yield { type: "command_denied", commandId, message: result.message };
        appendCommandAudit(preview, digest, startedAt, { outcome: "denied" });
        return;
      case "output_limit":
        yield { type: "command_failed", commandId, message: result.message };
        appendCommandAudit(preview, digest, startedAt, { outcome: "output_limit" });
        return;
      case "sandbox_denied":
        yield { type: "command_failed", commandId, message: result.message };
        appendCommandAudit(preview, digest, startedAt, { outcome: "sandbox_denied" });
        return;
      case "sandbox_unavailable":
        yield { type: "command_failed", commandId, message: result.message };
        appendCommandAudit(preview, digest, startedAt, { outcome: "sandbox_unavailable" });
        return;
      case "workspace_violation":
        yield { type: "command_failed", commandId, message: result.message };
        appendCommandAudit(preview, digest, startedAt, { outcome: "workspace_violation" });
        return;
      case "invalid_input":
      case "failed":
        yield { type: "command_failed", commandId, message: result.message };
        appendCommandAudit(preview, digest, startedAt, { outcome: "failed" });
        return;
      case "unavailable":
        yield { type: "command_failed", commandId, message: result.message };
        appendCommandAudit(preview, digest, startedAt, { outcome: "unavailable" });
        return;
    }
  }

  function appendCommandAudit(
    preview: CommandPreview,
    digest: string,
    startedAt: number,
    detail: {
      readonly outcome: string;
      readonly exitCode?: number | null;
      readonly durationMs?: number;
      readonly stdoutTruncated?: boolean;
      readonly stderrTruncated?: boolean;
    },
  ): void {
    const record: CommandAuditRecord = {
      commandId: activeCommandId ?? "<unknown>",
      runnerId: preview.runnerId,
      summary: preview.displayName,
      digest,
      startedAt,
      durationMs: detail.durationMs ?? null,
      exitCode: detail.exitCode ?? null,
      outcome: detail.outcome,
      stdoutTruncated: detail.stdoutTruncated ?? false,
      stderrTruncated: detail.stderrTruncated ?? false,
    };
    if (record.exitCode !== null && detail.outcome === "completed") {
      lastCommandExitCode = record.exitCode;
    }
    commandHistory.push(record);
    if (commandHistory.length > MAX_RETAINED_COMMAND_AUDIT_RECORDS) {
      commandHistory.splice(0, commandHistory.length - MAX_RETAINED_COMMAND_AUDIT_RECORDS);
    }
  }

  function* emitToolOutcome(
    callId: string,
    toolName: string,
    result: ToolExecutionResult,
  ): Generator<ApplicationEvent, void, void> {
    switch (result.status) {
      case "success":
        yield { type: "tool_completed", callId, toolName, summary: result.summary };
        break;
      case "cancelled":
        yield { type: "tool_cancelled", callId, toolName };
        break;
      case "timed_out":
      case "output_limit":
      case "sandbox_denied":
      case "sandbox_unavailable":
      case "workspace_violation":
      case "unavailable":
      case "invalid_input":
      case "denied":
      case "conflict":
      case "failed":
        yield { type: "tool_failed", callId, toolName, message: result.message };
        break;
    }
  }

  return {
    sendPrompt,
    getStatus(): SessionStatus {
      return {
        providerId: dependencies.provider.id,
        state,
        messageCount: history.length,
        pendingApproval,
        activeCommandId,
      };
    },
    getHistory(): readonly ConversationItem[] {
      return [...history];
    },
    getCompletedToolRounds(): number {
      return completedToolRounds;
    },
    getCommandHistory(): readonly CommandAuditRecord[] {
      return [...commandHistory];
    },
    getLastCommandExitCode(): number | null {
      return lastCommandExitCode;
    },
  };
}

function summarizePreview(preview: {
  readonly files: readonly unknown[];
  readonly totalAddedLines: number;
  readonly totalRemovedLines: number;
}): string {
  const fileLabel = preview.files.length === 1 ? "1 file" : `${preview.files.length} files`;
  return `${fileLabel}, +${preview.totalAddedLines} -${preview.totalRemovedLines}`;
}

function summarizeProbePreview(preview: {
  readonly risks: {
    readonly toolScripts: number;
    readonly enabledEditorPlugins: number;
    readonly gdextensions: number;
    readonly autoloads: number;
    readonly dotnetProjects: number;
  };
}): string {
  const { risks } = preview;
  const parts = [
    `tool scripts ${risks.toolScripts}`,
    `plugins ${risks.enabledEditorPlugins}`,
    `GDExtensions ${risks.gdextensions}`,
    `autoloads ${risks.autoloads}`,
    `.NET ${risks.dotnetProjects}`,
  ];
  return `recovery-mode project probe (${parts.join(", ")})`;
}

function summarizeLSPSessionPreview(preview: GDScriptLSPSessionPreview): string {
  const project = preview.projectIntelligence;
  return `Godot GDScript language session (${project.gdscriptFiles} scripts, ${project.toolScripts} @tool, ${project.editorPlugins} plugins)`;
}

function summarizeDiagnosticPreview(preview: GodotDiagnosticPreview): string {
  const scripts = preview.scripts;
  const scope =
    scripts.paths !== null ? scripts.paths.join(", ") : `${scripts.count} project scripts`;
  return `GDScript check-only diagnostics (${scope})`;
}

function readCheckpointId(output: JsonValue): string | null {
  if (typeof output !== "object" || output === null || Array.isArray(output)) {
    return null;
  }
  const value = (output as JsonObject)["checkpointId"];
  return typeof value === "string" && value.length > 0 ? value : null;
}

function asJsonObject(value: JsonValue): JsonObject | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return null;
  }
  return value as JsonObject;
}

function toDisplayInput(input: unknown): string {
  const text = JSON.stringify(input);
  if (text === undefined) {
    return "<unprintable>";
  }
  return text.length > MAX_DISPLAY_INPUT_LENGTH
    ? `${text.slice(0, MAX_DISPLAY_INPUT_LENGTH)}...`
    : text;
}

function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "The provider failed with an unknown error.";
}
