import type { ConversationItem } from "../domain/conversation.js";
import type { ToolExecutionResult } from "../tools/tool.js";
import type { ApplicationEvent } from "./application-events.js";
import type { TurnToolCall } from "./provider-turn.js";

export interface ToolRoundContext {
  readonly toolCalls: readonly TurnToolCall[];
  readonly execute: (
    call: Extract<TurnToolCall, { readonly kind: "execute" }>,
    signal?: AbortSignal,
  ) => AsyncGenerator<ApplicationEvent, ToolExecutionResult, void>;
  readonly signal?: AbortSignal;
}

export type ToolRoundOutcome =
  | {
      readonly kind: "completed";
      readonly transcript: readonly ConversationItem[];
    }
  | {
      readonly kind: "cancelled";
      readonly transcript: readonly ConversationItem[];
    };

/**
 * Execute one complete provider tool round while preserving transcript
 * pairing. Every recorded assistant tool call receives exactly one result,
 * including invalid calls and calls skipped after cancellation.
 */
export async function* executeToolRound(
  context: ToolRoundContext,
): AsyncGenerator<ApplicationEvent, ToolRoundOutcome, void> {
  const transcript: ConversationItem[] = context.toolCalls.map((call) => ({
    type: "assistant_tool_call",
    callId: call.callId,
    toolName: call.toolName,
    input: call.kind === "invalid" ? undefined : call.input,
  }));
  let cancelledIndex = -1;
  let abortedBeforeExecution = false;
  for (let index = 0; index < context.toolCalls.length; index += 1) {
    const call = context.toolCalls[index] as TurnToolCall;
    if (context.signal?.aborted === true) {
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
      transcript.push({
        type: "tool_result",
        callId: call.callId,
        toolName: call.toolName,
        result: { status: "failed", message: call.message },
      });
      continue;
    }
    const result = yield* context.execute(call, context.signal);
    transcript.push({
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
  if (cancelledIndex < 0) {
    return { kind: "completed", transcript };
  }
  const firstSkipped = cancelledIndex + (abortedBeforeExecution ? 0 : 1);
  for (let index = firstSkipped; index < context.toolCalls.length; index += 1) {
    const call = context.toolCalls[index] as TurnToolCall;
    transcript.push({
      type: "tool_result",
      callId: call.callId,
      toolName: call.toolName,
      result: {
        status: "cancelled",
        message: "The tool call was cancelled before it executed.",
      },
    });
  }
  return { kind: "cancelled", transcript };
}
