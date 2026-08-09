import type { ConversationItem } from "../domain/conversation.js";
import type { ToolDefinition } from "../tools/tool.js";

export interface ModelRequest {
  readonly messages: readonly ConversationItem[];
  readonly tools: readonly ToolDefinition[];
  /** Projected provider-neutral system context (stable + contextual). */
  readonly system?: string;
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
  /**
   * Whether this route supports tool calling. Absent means tool calling is
   * supported. Task modes that require tools fail clearly up front when
   * this is false instead of silently degrading into a text-only session.
   */
  readonly toolCalling?: boolean;

  stream(request: ModelRequest): AsyncIterable<ModelEvent>;
}
