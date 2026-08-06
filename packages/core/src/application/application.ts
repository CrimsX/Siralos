import type { ConversationMessage } from "../domain/conversation.js";
import { isCancellationError } from "../domain/cancellation.js";
import type { ModelProvider, ModelRequest } from "../ports/provider.js";

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
    };

export interface SessionStatus {
  readonly providerId: string;
  readonly state: "idle" | "responding";
  readonly messageCount: number;
}

export interface SolarisApplicationDependencies {
  readonly provider: ModelProvider;
}

export interface SolarisApplication {
  sendPrompt(text: string, signal?: AbortSignal): AsyncIterable<ApplicationEvent>;

  getStatus(): SessionStatus;
}

export function createSolarisApplication(
  dependencies: SolarisApplicationDependencies,
): SolarisApplication {
  const messages: ConversationMessage[] = [];
  let state: "idle" | "responding" = "idle";

  async function* sendPrompt(text: string, signal?: AbortSignal): AsyncIterable<ApplicationEvent> {
    if (state === "responding") {
      throw new Error("Solaris is already responding to a prompt.");
    }
    state = "responding";
    messages.push({ role: "user", content: text });
    let accumulated = "";
    try {
      yield { type: "response_started" };
      const snapshot: readonly ConversationMessage[] = [...messages];
      const request: ModelRequest =
        signal === undefined ? { messages: snapshot } : { messages: snapshot, signal };
      for await (const event of dependencies.provider.stream(request)) {
        if (event.type === "text_delta") {
          accumulated += event.text;
          yield { type: "text_delta", text: event.text };
        }
      }
      if (signal?.aborted) {
        yield { type: "response_cancelled" };
        return;
      }
      messages.push({ role: "assistant", content: accumulated });
      yield { type: "response_completed" };
    } catch (error: unknown) {
      if (signal?.aborted || isCancellationError(error)) {
        yield { type: "response_cancelled" };
        return;
      }
      yield { type: "response_failed", message: describeError(error) };
    } finally {
      state = "idle";
    }
  }

  return {
    sendPrompt,
    getStatus(): SessionStatus {
      return {
        providerId: dependencies.provider.id,
        state,
        messageCount: messages.length,
      };
    },
  };
}

function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "The provider failed with an unknown error.";
}
