import { isCancellationError } from "../domain/cancellation.js";
import type { ConversationItem } from "../domain/conversation.js";
import type { JsonObject, JsonValue } from "../domain/json.js";
import type { ModelProvider, ModelRequest } from "../ports/provider.js";
import { evaluatePermission } from "../security/permission-evaluator.js";
import type { ApprovalDecision, ApprovalRequest, ApprovalReviewer } from "../security/approval.js";
import type { CapabilityPolicy } from "../security/capability.js";
import { createDefaultPolicy } from "../security/default-policy.js";
import { INSPECT_PROFILE, type SandboxProfile } from "../security/profile.js";
import type { ToolRegistry } from "../tools/tool-registry.js";
import type { ToolExecutionResult } from "../tools/tool.js";
import { isPreparedMutationTool, toolCapability } from "../tools/prepared-mutation-tool.js";

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
      readonly capability: "workspace.write";
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
    };

export interface SessionStatus {
  readonly providerId: string;
  readonly state: "idle" | "responding";
  readonly messageCount: number;
  readonly pendingApproval: boolean;
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

export interface SolarisApplication {
  sendPrompt(text: string, signal?: AbortSignal): AsyncIterable<ApplicationEvent>;

  getStatus(): SessionStatus;
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
        if (toolRounds >= maxToolRounds) {
          yield {
            type: "response_failed",
            message: `Solaris reached the maximum of ${maxToolRounds} tool rounds.`,
          };
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
        if (turn.assistantText.length > 0) {
          history.push({ type: "assistant_message", content: turn.assistantText });
        }
        if (turn.toolCalls.length === 0) {
          yield { type: "response_completed" };
          return;
        }
        toolRounds += 1;
        for (const call of turn.toolCalls) {
          if (call.kind === "execute") {
            history.push({
              type: "assistant_tool_call",
              callId: call.callId,
              toolName: call.toolName,
              input: call.input,
            });
          }
        }
        for (const call of turn.toolCalls) {
          if (signal?.aborted) {
            yield { type: "response_cancelled" };
            return;
          }
          if (call.kind === "invalid") {
            yield {
              type: "tool_failed",
              callId: call.callId,
              toolName: call.toolName,
              message: call.message,
            };
            history.push({
              type: "tool_result",
              callId: call.callId,
              toolName: call.toolName,
              result: { status: "failed", message: call.message },
            });
            continue;
          }
          const result = yield* runToolCall(call, signal);
          history.push({
            type: "tool_result",
            callId: call.callId,
            toolName: call.toolName,
            result,
          });
          if (result.status === "cancelled") {
            yield { type: "response_cancelled" };
            return;
          }
        }
      }
    } finally {
      state = "idle";
    }
  }

  async function* collectProviderTurn(
    signal?: AbortSignal,
  ): AsyncGenerator<ApplicationEvent, TurnOutcome, void> {
    const request: ModelRequest = {
      messages: [...history],
      tools: toolDefinitions,
      ...(signal === undefined ? {} : { signal }),
    };
    let assistantText = "";
    const toolCalls: TurnToolCall[] = [];
    const seenCallIds = new Set<string>();
    let invalidCallIndex = 0;
    try {
      for await (const event of dependencies.provider.stream(request)) {
        if (event.type === "text_delta") {
          assistantText += event.text;
          yield { type: "text_delta", text: event.text };
        } else if (event.type === "tool_call") {
          if (event.callId.length === 0 || event.toolName.length === 0) {
            invalidCallIndex += 1;
            toolCalls.push({
              kind: "invalid",
              callId: `invalid-call-${invalidCallIndex}`,
              toolName: event.toolName.length === 0 ? "<empty>" : event.toolName,
              message: "Provider emitted a tool call with an empty call id or tool name.",
            });
          } else if (seenCallIds.has(event.callId)) {
            toolCalls.push({
              kind: "invalid",
              callId: event.callId,
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
      }
    } catch (error: unknown) {
      if (signal?.aborted || isCancellationError(error)) {
        return { kind: "cancelled" };
      }
      return { kind: "failed", message: describeError(error) };
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
    if (!isPreparedMutationTool(tool)) {
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
    const { mutation, preview } = prepared;
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
      result = await tool.apply(mutation, signal === undefined ? {} : { signal });
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
      };
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

function readCheckpointId(output: JsonValue): string | null {
  if (typeof output !== "object" || output === null || Array.isArray(output)) {
    return null;
  }
  const value = (output as JsonObject)["checkpointId"];
  return typeof value === "string" && value.length > 0 ? value : null;
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
