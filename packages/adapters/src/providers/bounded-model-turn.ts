import type {
  ConversationItem,
  ModelProvider,
  ToolDefinition,
  ToolExecutionResult,
} from "@solaris/core";
import { errorMessage } from "../support/error-message.js";

/**
 * Immutable collection limits for one provider turn.
 *
 * Callers may choose a smaller assistant-text limit, but every other
 * dimension remains explicitly bounded so a stream cannot trade one
 * representation (many tiny deltas, giant ids, or giant tool arguments)
 * for unbounded host memory.
 */
export interface BoundedModelTurnLimits {
  readonly maxTextBytes: number;
  readonly maxTextEvents: number;
  readonly maxToolCalls: number;
  readonly maxToolNameBytes: number;
  readonly maxCallIdBytes: number;
  readonly maxToolArgumentBytes: number;
  readonly maxTurnBytes: number;
}

export interface BoundedModelToolCall {
  readonly callId: string;
  readonly toolName: string;
  readonly input: unknown;
}

export type BoundedModelTurnOutcome =
  | {
      readonly kind: "turn";
      readonly text: string;
      readonly toolCalls: readonly BoundedModelToolCall[];
    }
  | { readonly kind: "aborted" }
  | { readonly kind: "failed"; readonly message: string };

export interface CollectBoundedModelTurnOptions {
  readonly actor: string;
  readonly provider: ModelProvider;
  readonly messages: readonly ConversationItem[];
  readonly tools: readonly ToolDefinition[];
  readonly signal: AbortSignal;
  readonly limits: BoundedModelTurnLimits;
  /**
   * Attempt-wide correlation set. Supplying one rejects a call id reused
   * in a later turn, preserving a one-call/one-result transcript identity.
   */
  readonly seenCallIds?: Set<string>;
}

export type BoundedToolResultOutcome =
  | {
      readonly ok: true;
      readonly result: ToolExecutionResult;
      readonly byteLength: number;
    }
  | { readonly ok: false; readonly message: string };

const TOOL_FAILURE_STATUSES = new Set([
  "invalid_input",
  "denied",
  "conflict",
  "failed",
  "cancelled",
  "timed_out",
  "output_limit",
  "sandbox_denied",
  "sandbox_unavailable",
  "workspace_violation",
  "unavailable",
]);

export function utf8ByteLength(value: string): number {
  return Buffer.byteLength(value, "utf8");
}

/** Normalize an internal numeric budget to a finite integer hard bound. */
export function normalizeBoundedInteger(
  value: number | undefined,
  fallback: number,
  minimum: number,
  maximum: number,
): number {
  if (value === undefined || !Number.isFinite(value)) {
    return fallback;
  }
  return Math.min(maximum, Math.max(minimum, Math.floor(value)));
}

/**
 * Validate, byte-bound, and JSON-detach a tool result before it is retained
 * in a provider conversation. Tool adapters are host-owned, but a malformed
 * adapter result must still fail at this boundary instead of crashing the
 * planner/reviewer or smuggling unknown fields into later turns.
 */
export function detachBoundedToolResult(
  value: unknown,
  maxBytes: number,
  actor: string,
): BoundedToolResultOutcome {
  let serialized: string;
  try {
    const candidate = JSON.stringify(value);
    if (candidate === undefined) {
      return { ok: false, message: `${actor} returned a non-JSON tool result.` };
    }
    serialized = candidate;
  } catch {
    return { ok: false, message: `${actor} returned a non-JSON tool result.` };
  }
  const byteLength = utf8ByteLength(serialized);
  if (byteLength > maxBytes) {
    return {
      ok: false,
      message: `${actor} returned a tool result exceeding the ${maxBytes}-byte limit.`,
    };
  }
  const parsed = JSON.parse(serialized) as unknown;
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    return { ok: false, message: `${actor} returned an invalid tool-result shape.` };
  }
  const record = parsed as Record<string, unknown>;
  const status = record["status"];
  if (status === "success") {
    if (!("output" in record) || typeof record["summary"] !== "string") {
      return { ok: false, message: `${actor} returned an invalid success tool result.` };
    }
    return {
      ok: true,
      result: {
        status: "success",
        output: record["output"] as import("@solaris/core").JsonValue,
        summary: record["summary"],
      },
      byteLength,
    };
  }
  if (typeof status !== "string" || !TOOL_FAILURE_STATUSES.has(status)) {
    return { ok: false, message: `${actor} returned an unknown tool-result status.` };
  }
  if (typeof record["message"] !== "string") {
    return { ok: false, message: `${actor} returned an invalid failure tool result.` };
  }
  return {
    ok: true,
    result: {
      status: status as Exclude<ToolExecutionResult["status"], "success">,
      message: record["message"],
    },
    byteLength,
  };
}

/**
 * Collect one strict provider turn and race it against cancellation.
 * Iterator EOF is not completion, and any event after `completed` rejects
 * the whole turn. A provider that ignores its AbortSignal may leave its
 * own pending operation behind, but the caller never awaits or consumes
 * that abandoned turn again.
 */
export async function collectBoundedModelTurn(
  options: CollectBoundedModelTurnOptions,
): Promise<BoundedModelTurnOutcome> {
  if (options.signal.aborted) {
    return { kind: "aborted" };
  }
  const pending = collect(options);
  return new Promise<BoundedModelTurnOutcome>((resolve) => {
    const onAbort = (): void => {
      resolve({ kind: "aborted" });
    };
    options.signal.addEventListener("abort", onAbort, { once: true });
    pending.then(
      (outcome) => {
        options.signal.removeEventListener("abort", onAbort);
        resolve(outcome);
      },
      (error: unknown) => {
        options.signal.removeEventListener("abort", onAbort);
        resolve({
          kind: "failed",
          message: `${options.actor} provider failed: ${errorMessage(error, "unknown error")}`,
        });
      },
    );
  });
}

async function collect(options: CollectBoundedModelTurnOptions): Promise<BoundedModelTurnOutcome> {
  const { actor, limits, provider, signal } = options;
  const seenCallIds = options.seenCallIds ?? new Set<string>();
  let text = "";
  let textBytes = 0;
  let textEvents = 0;
  let turnBytes = 0;
  let completionSeen = false;
  const toolCalls: BoundedModelToolCall[] = [];

  try {
    for await (const event of provider.stream({
      messages: [...options.messages],
      tools: options.tools,
      signal,
    })) {
      if (signal.aborted) {
        return { kind: "aborted" };
      }
      if (completionSeen) {
        return {
          kind: "failed",
          message: `${actor} stream emitted an event after completion.`,
        };
      }
      if (event.type === "completed") {
        completionSeen = true;
        continue;
      }
      if (event.type === "text_delta") {
        const bytes = utf8ByteLength(event.text);
        textEvents += 1;
        if (textEvents > limits.maxTextEvents) {
          return {
            kind: "failed",
            message: `${actor} exceeded the text-event count limit.`,
          };
        }
        textBytes += bytes;
        if (textBytes > limits.maxTextBytes) {
          return {
            kind: "failed",
            message: `${actor} output exceeded its byte limit.`,
          };
        }
        turnBytes += bytes;
        if (turnBytes > limits.maxTurnBytes) {
          return {
            kind: "failed",
            message: `${actor} exceeded the aggregate turn byte limit.`,
          };
        }
        text += event.text;
        continue;
      }

      if (event.callId.length === 0 || event.toolName.length === 0) {
        return {
          kind: "failed",
          message: `${actor} emitted a tool call with an empty id or name.`,
        };
      }
      if (seenCallIds.has(event.callId)) {
        return {
          kind: "failed",
          message: `${actor} emitted duplicate tool call id ${event.callId}.`,
        };
      }
      if (toolCalls.length >= limits.maxToolCalls) {
        return {
          kind: "failed",
          message: `${actor} exceeded the per-turn tool-call limit.`,
        };
      }

      const callIdBytes = utf8ByteLength(event.callId);
      if (callIdBytes > limits.maxCallIdBytes) {
        return {
          kind: "failed",
          message: `${actor} exceeded the tool-call id byte limit.`,
        };
      }
      const toolNameBytes = utf8ByteLength(event.toolName);
      if (toolNameBytes > limits.maxToolNameBytes) {
        return {
          kind: "failed",
          message: `${actor} exceeded the tool-name byte limit.`,
        };
      }
      let serializedInput: string;
      try {
        const serialized = JSON.stringify(event.input);
        if (serialized === undefined) {
          return {
            kind: "failed",
            message: `${actor} emitted a tool argument that is not JSON-serializable.`,
          };
        }
        serializedInput = serialized;
      } catch {
        return {
          kind: "failed",
          message: `${actor} emitted a tool argument that is not JSON-serializable.`,
        };
      }
      const argumentBytes = utf8ByteLength(serializedInput);
      if (argumentBytes > limits.maxToolArgumentBytes) {
        return {
          kind: "failed",
          message: `${actor} exceeded the tool-argument byte limit.`,
        };
      }
      turnBytes += callIdBytes + toolNameBytes + argumentBytes;
      if (turnBytes > limits.maxTurnBytes) {
        return {
          kind: "failed",
          message: `${actor} exceeded the aggregate turn byte limit.`,
        };
      }

      let input: unknown;
      try {
        input = JSON.parse(serializedInput) as unknown;
      } catch {
        return {
          kind: "failed",
          message: `${actor} emitted a tool argument that could not be detached as JSON data.`,
        };
      }
      seenCallIds.add(event.callId);
      toolCalls.push({ callId: event.callId, toolName: event.toolName, input });
    }
  } catch (error: unknown) {
    if (signal.aborted) {
      return { kind: "aborted" };
    }
    return {
      kind: "failed",
      message: `${actor} provider failed: ${errorMessage(error, "unknown error")}`,
    };
  }

  if (signal.aborted) {
    return { kind: "aborted" };
  }
  if (!completionSeen) {
    return {
      kind: "failed",
      message: `${actor} stream ended without a completion event.`,
    };
  }
  return { kind: "turn", text, toolCalls };
}
