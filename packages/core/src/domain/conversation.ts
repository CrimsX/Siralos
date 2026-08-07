import type { ToolExecutionResult } from "../tools/tool.js";

export type ConversationItem =
  | {
      readonly type: "user_message";
      readonly content: string;
    }
  | {
      readonly type: "assistant_message";
      readonly content: string;
    }
  | {
      readonly type: "assistant_tool_call";
      readonly callId: string;
      readonly toolName: string;
      readonly input: unknown;
    }
  | {
      readonly type: "tool_result";
      readonly callId: string;
      readonly toolName: string;
      readonly result: ToolExecutionResult;
    };

/**
 * Validates transcript pairing invariants: every `assistant_tool_call` must
 * be followed by exactly one `tool_result` for the same call id before the
 * next user message, and no `tool_result` may appear without a recorded
 * call. Returns an error description or null when the transcript is valid.
 * Called before every provider request so a structurally invalid history
 * fails closed instead of being sent to the provider.
 */
export function validateConversationItems(items: readonly ConversationItem[]): string | null {
  const pendingCalls = new Map<string, { toolName: string; resolved: boolean }>();
  for (const item of items) {
    if (item.type === "user_message") {
      if (pendingCalls.size > 0) {
        const [callId, call] = [...pendingCalls.entries()][0] as [
          string,
          { toolName: string; resolved: boolean },
        ];
        return `Tool call ${callId} (${call.toolName}) has no result before the next user message.`;
      }
      continue;
    }
    if (item.type === "assistant_tool_call") {
      if (item.callId.length === 0) {
        return "A tool call has an empty call id.";
      }
      if (pendingCalls.has(item.callId)) {
        return `Tool call id ${item.callId} appears more than once.`;
      }
      pendingCalls.set(item.callId, { toolName: item.toolName, resolved: false });
      continue;
    }
    if (item.type === "tool_result") {
      const call = pendingCalls.get(item.callId);
      if (call === undefined) {
        return `Tool result for ${item.callId} has no recorded call.`;
      }
      if (call.resolved) {
        return `Tool call ${item.callId} has more than one result.`;
      }
      call.resolved = true;
      pendingCalls.delete(item.callId);
      continue;
    }
  }
  if (pendingCalls.size > 0) {
    const [callId, call] = [...pendingCalls.entries()][0] as [
      string,
      { toolName: string; resolved: boolean },
    ];
    return `Tool call ${callId} (${call.toolName}) has no result.`;
  }
  return null;
}
