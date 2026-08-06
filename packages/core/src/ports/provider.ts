import type { ConversationItem } from "../domain/conversation.js";
import type { ToolDefinition } from "../tools/tool.js";

export interface ModelRequest {
  readonly messages: readonly ConversationItem[];
  readonly tools: readonly ToolDefinition[];
  readonly signal?: AbortSignal;
}

export type ModelEvent =
  | {
      readonly type: "text_delta";
      readonly text: string;
    }
  | {
      readonly type: "tool_call";
      readonly callId: string;
      readonly toolName: string;
      readonly input: unknown;
    }
  | {
      readonly type: "completed";
    };

export interface ModelProvider {
  readonly id: string;

  stream(request: ModelRequest): AsyncIterable<ModelEvent>;
}
