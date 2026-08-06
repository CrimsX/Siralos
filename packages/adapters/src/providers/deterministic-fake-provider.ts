import type {
  ConversationItem,
  JsonObject,
  JsonValue,
  ModelEvent,
  ModelProvider,
  ModelRequest,
  ToolDefinition,
  ToolExecutionResult,
} from "@solaris/core";

export const DETERMINISTIC_FAKE_PROVIDER_ID = "deterministic-fake";

const CHUNK_SIZE = 16;

type Scenario =
  | {
      readonly kind: "list";
      readonly toolName: "workspace.list";
      readonly input: { readonly path: string };
    }
  | {
      readonly kind: "read";
      readonly toolName: "workspace.read";
      readonly input: { readonly path: string };
    }
  | {
      readonly kind: "search";
      readonly toolName: "workspace.search";
      readonly input: { readonly query: string; readonly path: string };
    };

export function createDeterministicFakeProvider(): ModelProvider {
  return {
    id: DETERMINISTIC_FAKE_PROVIDER_ID,
    stream,
  };
}

async function* stream(request: ModelRequest): AsyncIterable<ModelEvent> {
  const signal = request.signal;
  if (signal?.aborted) {
    throw createAbortError();
  }
  const scenario = findScenario(request.messages);
  if (scenario !== null && isToolAvailable(request.tools, scenario.toolName)) {
    yield {
      type: "tool_call",
      callId: "call-1",
      toolName: scenario.toolName,
      input: scenario.input,
    };
    await Promise.resolve();
    const result = findResultForCall(request.messages, "call-1");
    const responseText = formatScenarioResponse(scenario, result);
    yield* streamTextChunks(responseText, signal);
    return;
  }
  const responseText = formatResponse(request.messages);
  yield* streamTextChunks(responseText, signal);
}

async function* streamTextChunks(
  text: string,
  signal: AbortSignal | undefined,
): AsyncIterable<ModelEvent> {
  for (const chunk of chunkText(text, CHUNK_SIZE)) {
    if (signal?.aborted) {
      throw createAbortError();
    }
    yield { type: "text_delta", text: chunk };
    await Promise.resolve();
  }
  if (signal?.aborted) {
    throw createAbortError();
  }
  yield { type: "completed" };
}

function findScenario(messages: readonly ConversationItem[]): Scenario | null {
  const latestUserPrompt = findLatestUserPrompt(messages);
  if (latestUserPrompt === "list files") {
    return { kind: "list", toolName: "workspace.list", input: { path: "." } };
  }
  if (latestUserPrompt === "read README.md") {
    return { kind: "read", toolName: "workspace.read", input: { path: "README.md" } };
  }
  if (latestUserPrompt.startsWith("search ")) {
    const query = latestUserPrompt.slice("search ".length).trim();
    if (query.length > 0) {
      return {
        kind: "search",
        toolName: "workspace.search",
        input: { query, path: "." },
      };
    }
  }
  return null;
}

function isToolAvailable(tools: readonly ToolDefinition[], toolName: string): boolean {
  return tools.some((tool) => tool.name === toolName);
}

function findResultForCall(
  messages: readonly ConversationItem[],
  callId: string,
): ToolExecutionResult | undefined {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const item = messages[index];
    if (item && item.type === "tool_result" && item.callId === callId) {
      return item.result;
    }
  }
  return undefined;
}

function formatScenarioResponse(
  scenario: Scenario,
  result: ToolExecutionResult | undefined,
): string {
  if (result === undefined) {
    return "Solaris has no tool result available.";
  }
  if (result.status !== "success") {
    return `Solaris could not complete the workspace operation: ${result.message}`;
  }
  switch (scenario.kind) {
    case "list": {
      const count = countArrayField(result.output, "entries");
      return count === null
        ? "Solaris inspected the workspace entries."
        : `Solaris inspected ${count} workspace entries.`;
    }
    case "read":
      return `Solaris read ${scenario.input.path}.`;
    case "search": {
      const count = countArrayField(result.output, "matches");
      return count === null
        ? "Solaris searched the workspace."
        : `Solaris found ${count} matching lines.`;
    }
  }
}

function countArrayField(output: JsonValue, key: string): number | null {
  if (typeof output !== "object" || output === null || Array.isArray(output)) {
    return null;
  }
  const record = output as JsonObject;
  const value = record[key];
  return Array.isArray(value) ? value.length : null;
}

function formatResponse(messages: readonly ConversationItem[]): string {
  return `Solaris received: ${findLatestUserPrompt(messages)}`;
}

function findLatestUserPrompt(messages: readonly ConversationItem[]): string {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const item = messages[index];
    if (item && item.type === "user_message") {
      return item.content;
    }
  }
  return "";
}

function chunkText(text: string, size: number): readonly string[] {
  const chunks: string[] = [];
  for (let index = 0; index < text.length; index += size) {
    chunks.push(text.slice(index, index + size));
  }
  return chunks;
}

function createAbortError(): Error {
  return new DOMException("The fake provider was aborted.", "AbortError");
}
