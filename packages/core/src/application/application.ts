import { isCancellationError } from "../domain/cancellation.js";
import type { ConversationItem } from "../domain/conversation.js";
import type { ModelProvider, ModelRequest } from "../ports/provider.js";
import type { ToolRegistry } from "../tools/tool-registry.js";
import type { ToolExecutionResult } from "../tools/tool.js";

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
    };

export interface SessionStatus {
  readonly providerId: string;
  readonly state: "idle" | "responding";
  readonly messageCount: number;
}

export interface SolarisApplicationDependencies {
  readonly provider: ModelProvider;
  readonly tools: ToolRegistry;
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
  const toolDefinitions = dependencies.tools.definitions();
  const maxToolRounds = dependencies.maxToolRounds ?? DEFAULT_MAX_TOOL_ROUNDS;
  let state: "idle" | "responding" = "idle";

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
    switch (result.status) {
      case "success":
        yield {
          type: "tool_completed",
          callId: call.callId,
          toolName: call.toolName,
          summary: result.summary,
        };
        break;
      case "cancelled":
        yield { type: "tool_cancelled", callId: call.callId, toolName: call.toolName };
        break;
      case "failed":
      case "invalid_input":
      case "denied":
        yield {
          type: "tool_failed",
          callId: call.callId,
          toolName: call.toolName,
          message: result.message,
        };
        break;
    }
    return result;
  }

  return {
    sendPrompt,
    getStatus(): SessionStatus {
      return {
        providerId: dependencies.provider.id,
        state,
        messageCount: history.length,
      };
    },
  };
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
