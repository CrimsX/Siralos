import { isCancellationError } from "../domain/cancellation.js";
import { validateConversationItems, type ConversationItem } from "../domain/conversation.js";
import type { ModelEvent, ModelProvider, ModelRequest } from "../ports/provider.js";
import type { ProjectionMode, ProjectionService } from "../projection/projection-service.js";
import type { ToolRegistry } from "../tools/tool-registry.js";
import type { ToolDefinition } from "../tools/tool.js";
import type { ApplicationEvent } from "./application-events.js";

export type TurnToolCall =
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

export type TurnOutcome =
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

export interface ProviderTurnContext {
  readonly provider: ModelProvider;
  readonly tools: ToolRegistry;
  readonly toolDefinitions: readonly ToolDefinition[];
  readonly history: readonly ConversationItem[];
  readonly projection?: ProjectionService;
}

/**
 * Per-turn provider stream bounds. Every bound is enforced on UTF-8 byte
 * counts, not JavaScript character counts, and exceeding any bound fails the
 * turn without committing partial output as a successful response.
 */
export const PROVIDER_TURN_LIMITS = {
  /** Total assistant text bytes across all deltas of one turn. */
  maxAssistantTextBytes: 64 * 1024,
  /** Number of text_delta events in one turn. */
  maxTextEvents: 4096,
  /** Number of tool_call events in one turn. */
  maxToolCallsPerTurn: 32,
  /** UTF-8 bytes of one tool-call correlation id. */
  maxCallIdBytes: 256,
  /** UTF-8 bytes of one tool name. */
  maxToolNameBytes: 256,
  /** UTF-8 bytes of one tool-call argument payload. */
  maxToolArgumentBytes: 128 * 1024,
  /** Aggregate UTF-8 bytes (text + ids + tool names + arguments) of one turn. */
  maxTurnBytes: 256 * 1024,
} as const;

const textEncoder = new TextEncoder();

type ProviderIteratorRead =
  | { readonly kind: "next"; readonly result: IteratorResult<ModelEvent> }
  | { readonly kind: "cancelled" };

/**
 * Collect and validate exactly one provider turn.
 *
 * Provider events are externally supplied data: the TypeScript discriminated
 * union is not a runtime trust boundary. The event discriminator is
 * authoritative, and unknown or malformed runtime events fail the turn closed
 * instead of being reinterpreted by field shape.
 */
export async function* collectProviderTurn(
  context: ProviderTurnContext,
  signal?: AbortSignal,
  mode?: ProjectionMode,
): AsyncGenerator<ApplicationEvent, TurnOutcome, void> {
  const transcriptError = validateConversationItems(context.history);
  if (transcriptError !== null) {
    return {
      kind: "failed",
      message: `The conversation transcript is structurally invalid; the provider request was blocked: ${transcriptError}`,
    };
  }
  let requestMessages: readonly ConversationItem[] = context.history;
  let requestTools: readonly ToolDefinition[] = context.toolDefinitions;
  let requestSystem: string | undefined;
  if (context.projection !== undefined) {
    const projected = context.projection.projectRequest({
      mode: mode ?? "generic",
      messages: [...context.history],
      tools: context.tools.definitions(),
      providerToolCalling: context.provider.toolCalling !== false,
    });
    if (projected.pressure.state !== "normal") {
      yield {
        type: "context_pressure",
        state: projected.pressure.state,
        estimatedTokens: projected.estimatedTokens,
        workingMaximum: projected.pressure.workingMaximum,
      };
    }
    if (projected.blocked !== null) {
      yield {
        type: "response_failed",
        message: projected.blocked.reason,
      };
      return { kind: "failed", message: projected.blocked.reason };
    }
    requestMessages = projected.messages;
    requestTools = projected.tools.map((info) => info.definition);
    requestSystem = projected.system ?? undefined;
  }
  const request: ModelRequest = {
    messages: [...requestMessages],
    tools: requestTools,
    ...(requestSystem === undefined ? {} : { system: requestSystem }),
    ...(signal === undefined ? {} : { signal }),
  };
  let assistantText = "";
  let assistantTextBytes = 0;
  let textEvents = 0;
  let turnBytes = 0;
  const toolCalls: TurnToolCall[] = [];
  const seenCallIds = new Set<string>();
  let invalidCallIndex = 0;
  let completionSeen = false;
  let exceeded: string | null = null;
  let protocolError: string | null = null;
  let iterator: AsyncIterator<ModelEvent> | undefined;
  let iteratorDone = false;
  try {
    iterator = context.provider.stream(request)[Symbol.asyncIterator]();
    for (;;) {
      const read = await nextProviderEvent(iterator, signal);
      if (read.kind === "cancelled") {
        break;
      }
      if (read.result.done === true) {
        iteratorDone = true;
        break;
      }
      const rawEvent = read.result.value as unknown;
      if (completionSeen) {
        exceeded = "an event after completion";
        break;
      }
      // Provider events are externally supplied data: the TypeScript
      // discriminated union is not a runtime trust boundary. The
      // discriminator is authoritative and everything else fails closed.
      if (rawEvent === null || typeof rawEvent !== "object" || Array.isArray(rawEvent)) {
        protocolError = "a malformed event";
        break;
      }
      const event = rawEvent as {
        readonly type?: unknown;
        readonly text?: unknown;
        readonly callId?: unknown;
        readonly toolName?: unknown;
        readonly input?: unknown;
      };
      if (event.type === "completed") {
        // Extra fields on a completed event are ignored; only the
        // discriminator is authoritative.
        completionSeen = true;
        continue;
      }
      if (event.type === "text_delta") {
        if (typeof event.text !== "string") {
          protocolError = "a text event without a string payload";
          break;
        }
        const bytes = utf8ByteLength(event.text);
        textEvents += 1;
        if (textEvents > PROVIDER_TURN_LIMITS.maxTextEvents) {
          exceeded = "the text-event count";
          break;
        }
        assistantTextBytes += bytes;
        if (assistantTextBytes > PROVIDER_TURN_LIMITS.maxAssistantTextBytes) {
          exceeded = "the assistant-text byte limit";
          break;
        }
        turnBytes += bytes;
        if (turnBytes > PROVIDER_TURN_LIMITS.maxTurnBytes) {
          exceeded = "the aggregate turn byte limit";
          break;
        }
        assistantText += event.text;
        yield { type: "text_delta", text: event.text };
        continue;
      }
      if (event.type === "tool_call") {
        if (typeof event.callId !== "string" || typeof event.toolName !== "string") {
          protocolError = "a tool call with a non-string id or name";
          break;
        }
        const callId = event.callId;
        const toolName = event.toolName;
        const callIdBytes = utf8ByteLength(callId);
        const nameBytes = utf8ByteLength(toolName);
        let serializedInput: string;
        try {
          const serialized = JSON.stringify(event.input);
          if (serialized === undefined) {
            exceeded = "the tool-argument JSON validity";
            break;
          }
          serializedInput = serialized;
        } catch {
          exceeded = "the tool-argument JSON validity";
          break;
        }
        const argumentBytes = utf8ByteLength(serializedInput);
        if (callIdBytes > PROVIDER_TURN_LIMITS.maxCallIdBytes) {
          exceeded = "the tool-call id byte limit";
          break;
        }
        if (nameBytes > PROVIDER_TURN_LIMITS.maxToolNameBytes) {
          exceeded = "the tool-name byte limit";
          break;
        }
        if (argumentBytes > PROVIDER_TURN_LIMITS.maxToolArgumentBytes) {
          exceeded = "the tool-argument byte limit";
          break;
        }
        turnBytes += callIdBytes + nameBytes + argumentBytes;
        if (turnBytes > PROVIDER_TURN_LIMITS.maxTurnBytes) {
          exceeded = "the aggregate turn byte limit";
          break;
        }
        if (toolCalls.length >= PROVIDER_TURN_LIMITS.maxToolCallsPerTurn) {
          exceeded = "the tool-call count";
          break;
        }
        if (callId.length === 0 || toolName.length === 0) {
          invalidCallIndex += 1;
          toolCalls.push({
            kind: "invalid",
            callId: `invalid-call-${invalidCallIndex}`,
            toolName: toolName.length === 0 ? "<empty>" : toolName,
            message: "Provider emitted a tool call with an empty call id or tool name.",
          });
        } else if (seenCallIds.has(callId)) {
          invalidCallIndex += 1;
          toolCalls.push({
            kind: "invalid",
            callId: `invalid-call-${invalidCallIndex}`,
            toolName,
            message: `Duplicate tool call id: ${callId}.`,
          });
        } else {
          seenCallIds.add(callId);
          toolCalls.push({
            kind: "execute",
            callId,
            toolName,
            input: JSON.parse(serializedInput) as unknown,
          });
        }
        continue;
      }
      protocolError = "an unknown event type";
      break;
    }
  } catch (error: unknown) {
    if (signal?.aborted === true || isCancellationError(error)) {
      return { kind: "cancelled" };
    }
    return { kind: "failed", message: describeError(error) };
  } finally {
    if (!iteratorDone) {
      closeProviderIterator(iterator);
    }
  }
  if (signal?.aborted === true) {
    return { kind: "cancelled" };
  }
  if (exceeded !== null) {
    return {
      kind: "failed",
      message: `The provider exceeded ${exceeded} limit; the response was rejected.`,
    };
  }
  if (protocolError !== null) {
    return {
      kind: "failed",
      message: `The provider emitted ${protocolError}; the response was rejected.`,
    };
  }
  if (!completionSeen) {
    return {
      kind: "failed",
      message: "The provider stream ended without a completion event; the response was rejected.",
    };
  }
  return { kind: "turn", assistantText, toolCalls };
}

function utf8ByteLength(text: string): number {
  return textEncoder.encode(text).length;
}

/**
 * Await one provider event without allowing an iterator that ignores its
 * AbortSignal to hold the application open forever after caller
 * cancellation. The abandoned `next()` remains handled to avoid an
 * unhandled rejection, but its eventual value is never consumed.
 */
function nextProviderEvent(
  iterator: AsyncIterator<ModelEvent>,
  signal: AbortSignal | undefined,
): Promise<ProviderIteratorRead> {
  if (signal?.aborted === true) {
    return Promise.resolve({ kind: "cancelled" });
  }
  if (signal === undefined) {
    return iterator.next().then((result) => ({ kind: "next", result }));
  }
  return new Promise<ProviderIteratorRead>((resolve, reject) => {
    let settled = false;
    const finish = (value: ProviderIteratorRead): void => {
      if (settled) {
        return;
      }
      settled = true;
      signal.removeEventListener("abort", onAbort);
      resolve(value);
    };
    const fail = (error: unknown): void => {
      if (settled) {
        return;
      }
      settled = true;
      signal.removeEventListener("abort", onAbort);
      reject(error instanceof Error ? error : new Error(describeError(error)));
    };
    const onAbort = (): void => finish({ kind: "cancelled" });
    signal.addEventListener("abort", onAbort, { once: true });
    iterator.next().then(
      (result) => finish({ kind: "next", result }),
      (error: unknown) => fail(error),
    );
  });
}

function closeProviderIterator(iterator: AsyncIterator<ModelEvent> | undefined): void {
  try {
    const closing = iterator?.return?.();
    if (closing !== undefined) {
      void closing.catch(() => undefined);
    }
  } catch {
    // Best-effort close only: the already-selected application outcome is
    // authoritative, and a provider cleanup failure cannot replace it.
  }
}

function describeError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
