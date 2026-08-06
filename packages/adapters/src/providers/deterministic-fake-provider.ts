import type { ConversationMessage, ModelEvent, ModelProvider, ModelRequest } from "@solaris/core";

export const DETERMINISTIC_FAKE_PROVIDER_ID = "deterministic-fake";

const CHUNK_SIZE = 16;

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
  const responseText = formatResponse(request.messages);
  for (const chunk of chunkText(responseText, CHUNK_SIZE)) {
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

function formatResponse(messages: readonly ConversationMessage[]): string {
  return `Solaris received: ${findLatestUserPrompt(messages)}`;
}

function findLatestUserPrompt(messages: readonly ConversationMessage[]): string {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    if (message && message.role === "user") {
      return message.content;
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
