import { isCancellationError } from "../domain/cancellation.js";
import { validateConversationItems, type ConversationItem } from "../domain/conversation.js";
import type { JsonObject, JsonValue } from "../domain/json.js";
import type { ModelProvider, ModelRequest } from "../ports/provider.js";
import { evaluatePermission } from "../security/permission-evaluator.js";
import type { ApprovalDecision, ApprovalRequest, ApprovalReviewer } from "../security/approval.js";
import type { CapabilityPolicy } from "../security/capability.js";
import { createDefaultPolicy } from "../security/default-policy.js";
import { INSPECT_PROFILE, type SandboxProfile } from "../security/profile.js";
import type { ProcessOutputEvent } from "../security/sandbox-backend.js";
import type { ToolRegistry } from "../tools/tool-registry.js";
import type { ToolExecutionResult } from "../tools/tool.js";
import {
  isPreparedCommandTool,
  isPreparedMutationTool,
  toolCapability,
} from "../tools/prepared-mutation-tool.js";
import { isPreparedProbeTool } from "../tools/prepared-probe-tool.js";
import type { PreparedProjectProbeTool } from "../tools/prepared-probe-tool.js";
import type { PreparedCommandTool } from "../commands/command-tool.js";
import type { CommandAuditRecord, CommandApplicationEvent } from "../commands/command-events.js";
import { MAX_RETAINED_COMMAND_AUDIT_RECORDS } from "../commands/command-events.js";
import type { CommandPreview, PreparedCommand } from "../commands/command-runners.js";
import type { PermissionEvaluation } from "../security/permission-evaluator.js";
import { PROCESS_RUN_TOOL_NAME } from "../commands/command-tool.js";

export type ApplicationEvent =
  | {
      readonly type: "response_started";
    }
  | {
      readonly type: "text_delta";
      readonly text: string;
    }
  | {
      readonly type: "response_completed";
    }
  | {
      readonly type: "response_cancelled";
    }
  | {
      readonly type: "response_failed";
      readonly message: string;
    }
  | {
      readonly type: "tool_started";
      readonly callId: string;
      readonly toolName: string;
      readonly displayInput: string;
    }
  | {
      readonly type: "tool_awaiting_approval";
      readonly callId: string;
      readonly toolName: string;
      readonly requestId: string;
    }
  | {
      readonly type: "tool_completed";
      readonly callId: string;
      readonly toolName: string;
      readonly summary: string;
    }
  | {
      readonly type: "tool_failed";
      readonly callId: string;
      readonly toolName: string;
      readonly message: string;
    }
  | {
      readonly type: "tool_cancelled";
      readonly callId: string;
      readonly toolName: string;
    }
  | {
      readonly type: "approval_requested";
      readonly requestId: string;
      readonly toolName: string;
      readonly capability: "workspace.write" | "process.execute" | "godot.probe_project";
      readonly summary: string;
    }
  | {
      readonly type: "approval_resolved";
      readonly requestId: string;
      readonly decision: "approved" | "denied" | "cancelled";
    }
  | {
      readonly type: "checkpoint_applied";
      readonly checkpointId: string;
      readonly path: string;
    }
  | CommandApplicationEvent;

export interface SessionStatus {
  readonly providerId: string;
  readonly state: "idle" | "responding";
  readonly messageCount: number;
  readonly pendingApproval: boolean;
  readonly activeCommandId: string | null;
}

export interface SolarisApplicationDependencies {
  readonly provider: ModelProvider;
  readonly tools: ToolRegistry;
  readonly policy?: CapabilityPolicy;
  readonly profile?: SandboxProfile;
  readonly reviewer?: ApprovalReviewer;
  readonly maxToolRounds?: number;
}

export const DEFAULT_MAX_TOOL_ROUNDS = 8;

/**
 * Per-turn provider stream bounds. Every bound is enforced on UTF-8 byte
 * counts, not JavaScript character counts, and exceeding any bound fails the
 * turn without committing partial output as a successful response.
 */
export const PROVIDER_TURN_LIMITS = {
  /** Total assistant text bytes across all deltas of one turn. */
  maxAssistantTextBytes: 64 * 1024,
  /** Number of text_delta events in one turn. */
  maxTextEvents: 4096,
  /** Number of tool_call events in one turn. */
  maxToolCallsPerTurn: 32,
  /** UTF-8 bytes of one tool name. */
  maxToolNameBytes: 256,
  /** UTF-8 bytes of one tool-call argument payload. */
  maxToolArgumentBytes: 128 * 1024,
  /** Aggregate UTF-8 bytes (text + tool names + arguments) of one turn. */
  maxTurnBytes: 256 * 1024,
} as const;

export interface SolarisApplication {
  sendPrompt(text: string, signal?: AbortSignal): AsyncIterable<ApplicationEvent>;

  getStatus(): SessionStatus;

  /** Bounded in-memory metadata for commands that executed this session. */
  getCommandHistory(): readonly CommandAuditRecord[];

  /** Exit code of the most recently completed command, if any. */
  getLastCommandExitCode(): number | null;
}

type TurnToolCall =
  | {
      readonly kind: "execute";
      readonly callId: string;
      readonly toolName: string;
      readonly input: unknown;
    }
  | {
      readonly kind: "invalid";
      readonly callId: string;
      readonly toolName: string;
      readonly message: string;
    };

type TurnOutcome =
  | {
      readonly kind: "turn";
      readonly assistantText: string;
      readonly toolCalls: readonly TurnToolCall[];
    }
  | {
      readonly kind: "cancelled";
    }
  | {
      readonly kind: "failed";
      readonly message: string;
    };

const MAX_DISPLAY_INPUT_LENGTH = 200;

const textEncoder = new TextEncoder();

function utf8ByteLength(text: string): number {
  return textEncoder.encode(text).length;
}

export function createSolarisApplication(
  dependencies: SolarisApplicationDependencies,
): SolarisApplication {
  const history: ConversationItem[] = [];
  const policy = dependencies.policy ?? createDefaultPolicy("inspect");
  const profile = dependencies.profile ?? INSPECT_PROFILE;
  const reviewer = dependencies.reviewer;
  const maxToolRounds = dependencies.maxToolRounds ?? DEFAULT_MAX_TOOL_ROUNDS;
  const toolDefinitions = dependencies.tools
    .definitions()
    .filter((info) => evaluatePermission(info.capability, policy, profile).decision !== "deny")
    .map((info) => info.definition);
  let state: "idle" | "responding" = "idle";
  let pendingApproval = false;
  let approvalCounter = 0;
  let activeCommandId: string | null = null;
  let lastCommandExitCode: number | null = null;
  const commandHistory: CommandAuditRecord[] = [];

  async function* sendPrompt(text: string, signal?: AbortSignal): AsyncIterable<ApplicationEvent> {
    if (state === "responding") {
      throw new Error("Solaris is already responding to a prompt.");
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
        const turn = yield* collectProviderTurn(signal);
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
          yield { type: "response_completed" };
          return;
        }
        if (toolRounds >= maxToolRounds) {
          yield {
            type: "response_failed",
            message: `Solaris reached the maximum of ${maxToolRounds} tool rounds; the requested tool round was not executed.`,
          };
          return;
        }
        toolRounds += 1;
        const executed: ConversationItem[] = [];
        for (const call of turn.toolCalls) {
          executed.push({
            type: "assistant_tool_call",
            callId: call.callId,
            toolName: call.toolName,
            input: call.kind === "invalid" ? undefined : call.input,
          });
        }
        let cancelledIndex = -1;
        let abortedBeforeExecution = false;
        for (let index = 0; index < turn.toolCalls.length; index += 1) {
          const call = turn.toolCalls[index] as TurnToolCall;
          if (signal?.aborted) {
            cancelledIndex = index;
            abortedBeforeExecution = true;
            break;
          }
          if (call.kind === "invalid") {
            yield {
              type: "tool_failed",
              callId: call.callId,
              toolName: call.toolName,
              message: call.message,
            };
            executed.push({
              type: "tool_result",
              callId: call.callId,
              toolName: call.toolName,
              result: { status: "failed", message: call.message },
            });
            continue;
          }
          const result = yield* runToolCall(call, signal);
          executed.push({
            type: "tool_result",
            callId: call.callId,
            toolName: call.toolName,
            result,
          });
          if (result.status === "cancelled") {
            cancelledIndex = index;
            break;
          }
        }
        if (cancelledIndex >= 0) {
          const start = cancelledIndex + (abortedBeforeExecution ? 0 : 1);
          for (let index = start; index < turn.toolCalls.length; index += 1) {
            const call = turn.toolCalls[index] as TurnToolCall;
            executed.push({
              type: "tool_result",
              callId: call.callId,
              toolName: call.toolName,
              result: {
                status: "cancelled",
                message: "The tool call was cancelled before it executed.",
              },
            });
          }
          for (const item of executed) {
            history.push(item);
          }
          yield { type: "response_cancelled" };
          return;
        }
        if (turn.assistantText.length > 0) {
          history.push({ type: "assistant_message", content: turn.assistantText });
        }
        for (const item of executed) {
          history.push(item);
        }
      }
    } finally {
      state = "idle";
    }
  }

  async function* collectProviderTurn(
    signal?: AbortSignal,
  ): AsyncGenerator<ApplicationEvent, TurnOutcome, void> {
    const transcriptError = validateConversationItems(history);
    if (transcriptError !== null) {
      return {
        kind: "failed",
        message: `The conversation transcript is structurally invalid; the provider request was blocked: ${transcriptError}`,
      };
    }
    const request: ModelRequest = {
      messages: [...history],
      tools: toolDefinitions,
      ...(signal === undefined ? {} : { signal }),
    };
    let assistantText = "";
    let assistantTextBytes = 0;
    let textEvents = 0;
    let turnBytes = 0;
    const toolCalls: TurnToolCall[] = [];
    const seenCallIds = new Set<string>();
    let invalidCallIndex = 0;
    let completionSeen = false;
    let exceeded: string | null = null;
    try {
      for await (const event of dependencies.provider.stream(request)) {
        if (signal?.aborted) {
          break;
        }
        if (completionSeen) {
          exceeded = "an event after completion";
          break;
        }
        if (event.type === "completed") {
          completionSeen = true;
          continue;
        }
        if (event.type === "text_delta") {
          const bytes = utf8ByteLength(event.text);
          textEvents += 1;
          if (textEvents > PROVIDER_TURN_LIMITS.maxTextEvents) {
            exceeded = "the text-event count";
            break;
          }
          // The assistant-text limit is cumulative across all deltas of the
          // turn, not a per-delta cap: individually legal deltas cannot
          // accumulate beyond the documented total.
          assistantTextBytes += bytes;
          if (assistantTextBytes > PROVIDER_TURN_LIMITS.maxAssistantTextBytes) {
            exceeded = "the assistant-text byte limit";
            break;
          }
          turnBytes += bytes;
          if (turnBytes > PROVIDER_TURN_LIMITS.maxTurnBytes) {
            exceeded = "the aggregate turn byte limit";
            break;
          }
          assistantText += event.text;
          yield { type: "text_delta", text: event.text };
          continue;
        }
        const nameBytes = utf8ByteLength(event.toolName);
        const argumentBytes = utf8ByteLength(JSON.stringify(event.input) ?? "");
        if (nameBytes > PROVIDER_TURN_LIMITS.maxToolNameBytes) {
          exceeded = "the tool-name byte limit";
          break;
        }
        if (argumentBytes > PROVIDER_TURN_LIMITS.maxToolArgumentBytes) {
          exceeded = "the tool-argument byte limit";
          break;
        }
        turnBytes += nameBytes + argumentBytes;
        if (turnBytes > PROVIDER_TURN_LIMITS.maxTurnBytes) {
          exceeded = "the aggregate turn byte limit";
          break;
        }
        if (toolCalls.length >= PROVIDER_TURN_LIMITS.maxToolCallsPerTurn) {
          exceeded = "the tool-call count";
          break;
        }
        if (event.callId.length === 0 || event.toolName.length === 0) {
          invalidCallIndex += 1;
          toolCalls.push({
            kind: "invalid",
            callId: `invalid-call-${invalidCallIndex}`,
            toolName: event.toolName.length === 0 ? "<empty>" : event.toolName,
            message: "Provider emitted a tool call with an empty call id or tool name.",
          });
        } else if (seenCallIds.has(event.callId)) {
          invalidCallIndex += 1;
          toolCalls.push({
            kind: "invalid",
            callId: `invalid-call-${invalidCallIndex}`,
            toolName: event.toolName,
            message: `Duplicate tool call id: ${event.callId}.`,
          });
        } else {
          seenCallIds.add(event.callId);
          toolCalls.push({
            kind: "execute",
            callId: event.callId,
            toolName: event.toolName,
            input: event.input,
          });
        }
      }
    } catch (error: unknown) {
      if (signal?.aborted || isCancellationError(error)) {
        return { kind: "cancelled" };
      }
      return { kind: "failed", message: describeError(error) };
    }
    if (signal?.aborted) {
      return { kind: "cancelled" };
    }
    if (exceeded !== null) {
      return {
        kind: "failed",
        message: `The provider exceeded ${exceeded} limit; the response was rejected.`,
      };
    }
    if (!completionSeen) {
      return {
        kind: "failed",
        message: "The provider stream ended without a completion event; the response was rejected.",
      };
    }
    return { kind: "turn", assistantText, toolCalls };
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
    const prepared = await tool.prepare(input, signal === undefined ? {} : { signal });
    if (prepared.status !== "ready") {
      yield {
        type: "tool_failed",
        callId,
        toolName,
        message: prepared.message,
      };
      return { status: "failed", message: prepared.message };
    }
    const { probe, preview, digest } = prepared;
    if (permission.decision === "ask") {
      const requestId = `approval-${(approvalCounter += 1)}`;
      const approvalRequest: ApprovalRequest = {
        id: requestId,
        capability: "godot.probe_project",
        toolName,
        summary: summarizeProbePreview(preview),
        preview,
        digest,
      };
      yield {
        type: "approval_requested",
        requestId,
        toolName,
        capability: "godot.probe_project",
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
          reason: "The approval reviewer failed; the probe was denied.",
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
          return { status: "cancelled", message: "The project probe approval was cancelled." };
        }
        const message = decision.reason ?? "The project probe was denied by the user.";
        yield { type: "tool_failed", callId, toolName, message };
        return { status: "denied", message };
      }
    }
    let result: ToolExecutionResult;
    try {
      result = await tool.executePrepared(probe, {
        ...(signal === undefined ? {} : { signal }),
        approvedDigest: digest,
      });
    } catch (error: unknown) {
      if (signal?.aborted || isCancellationError(error)) {
        result = { status: "cancelled", message: "The project probe was cancelled." };
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
