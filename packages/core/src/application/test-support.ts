import type { ApplicationEvent, ModelEvent, ModelProvider, ModelRequest } from "../index.js";

export function createScriptedProvider(
  turns: readonly (readonly ModelEvent[])[],
): ScriptedProvider {
  const requests: ModelRequest[] = [];
  let index = 0;
  const provider: ModelProvider = {
    id: "scripted-stub",
    async *stream(request: ModelRequest): AsyncIterable<ModelEvent> {
      requests.push(request);
      const events = turns[index] ?? [];
      index += 1;
      for (const event of events) {
        yield event;
        await Promise.resolve();
      }
    },
  };
  return { provider, requests };
}

export interface ScriptedProvider {
  readonly provider: ModelProvider;
  readonly requests: ModelRequest[];
}

export function toolCall(callId: string, toolName: string, input: unknown): ModelEvent {
  return { type: "tool_call", callId, toolName, input };
}

export async function collectEvents(
  events: AsyncIterable<ApplicationEvent>,
): Promise<ApplicationEvent[]> {
  const collected: ApplicationEvent[] = [];
  for await (const event of events) {
    collected.push(event);
  }
  return collected;
}

export function describeItems(requests: ModelRequest[]): string[][] {
  return requests.map((request) =>
    request.messages.map((item) => {
      switch (item.type) {
        case "user_message":
        case "assistant_message":
          return `${item.type}:${item.content}`;
        case "assistant_tool_call":
          return `${item.type}:${item.toolName}`;
        case "tool_result":
          return `${item.type}:${item.toolName}:${item.result.status}`;
      }
    }),
  );
}
