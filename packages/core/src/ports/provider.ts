import type { ConversationMessage } from "../domain/conversation.js";

export interface ModelRequest {
  readonly messages: readonly ConversationMessage[];
  readonly signal?: AbortSignal;
}

export type ModelEvent =
  | {
      readonly type: "text_delta";
      readonly text: string;
    }
  | {
      readonly type: "completed";
    };

export interface ModelProvider {
  readonly id: string;

  stream(request: ModelRequest): AsyncIterable<ModelEvent>;
}
