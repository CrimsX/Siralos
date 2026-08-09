/**
 * Deterministic token estimation (Stage 3 milestone 2).
 *
 * Exact provider tokenizers are unavailable without network/runtime
 * dependencies, so projection uses a conservative deterministic estimator:
 * UTF-8 characters / 4 rounded up, with multi-byte characters counted
 * byte-wise. This is documented as an approximation; the working budget is
 * configured with headroom so estimation error cannot silently overflow a
 * real provider limit.
 */

const textEncoder = new TextEncoder();

function utf8ByteLength(text: string): number {
  return textEncoder.encode(text).length;
}

export function estimateTokens(text: string): number {
  if (text.length === 0) {
    return 0;
  }
  return Math.ceil(utf8ByteLength(text) / 4);
}

/** Deterministic JSON token estimate for structured payloads. */
export function estimateJsonTokens(value: unknown): number {
  try {
    return estimateTokens(JSON.stringify(value));
  } catch {
    return 0;
  }
}

export interface TokenEstimate {
  readonly tokens: number;
  readonly bytes: number;
}

/** Estimate one conversation item (content + structured payloads). */
export function estimateConversationItemTokens(item: unknown): TokenEstimate {
  if (typeof item !== "object" || item === null) {
    return { tokens: 0, bytes: 0 };
  }
  const record = item as Record<string, unknown>;
  let bytes = 0;
  for (const key of ["content", "summary", "message", "toolName", "callId"]) {
    const value = record[key];
    if (typeof value === "string") {
      bytes += utf8ByteLength(value);
    }
  }
  const input = record["input"];
  if (input !== undefined) {
    try {
      bytes += utf8ByteLength(JSON.stringify(input));
    } catch {
      // non-serializable input contributes nothing to the estimate
    }
  }
  const output = record["output"];
  if (output !== undefined && typeof output === "object") {
    try {
      bytes += utf8ByteLength(JSON.stringify(output));
    } catch {
      // non-serializable output contributes nothing to the estimate
    }
  }
  const result = record["result"];
  if (result !== undefined && typeof result === "object" && result !== null) {
    const resultRecord = result as Record<string, unknown>;
    for (const key of ["summary", "message"]) {
      const value = resultRecord[key];
      if (typeof value === "string") {
        bytes += utf8ByteLength(value);
      }
    }
  }
  return { tokens: Math.ceil(bytes / 4), bytes };
}
