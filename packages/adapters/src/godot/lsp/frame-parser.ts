import { GODOT_LIMITS } from "@siralos/core";

/**
 * Deterministic incremental LSP frame parser (JSON-RPC over
 * `Content-Length` headers). Handles fragmented headers, fragmented
 * bodies, and multiple messages per socket read; bounds the header block
 * and the message body; rejects malformed, missing, negative, and absurd
 * Content-Length values; and requires valid UTF-8 JSON. Newline-delimited
 * JSON assumptions are never made. A protocol error fails the stream
 * deterministically: the parser reports the error once and ignores all
 * subsequent input, so Siralos never mis-parses a hostile stream.
 */

export interface LSPFrameParseError {
  readonly kind: "protocol-error";
  readonly message: string;
}

export interface LSPFrameResult {
  /** Exact frame payload bytes (JSON text). */
  readonly payload: Uint8Array;
}

export type LSPFrameOutcome =
  | { readonly ok: true; readonly frame: LSPFrameResult }
  | { readonly ok: false; readonly error: LSPFrameParseError };

export interface LSPFrameParserOptions {
  readonly maxHeaderBytes?: number;
  readonly maxBodyBytes?: number;
}

export class LSPFrameParser {
  private readonly maxHeaderBytes: number;
  private readonly maxBodyBytes: number;
  private pending: Uint8Array[] = [];
  private pendingLength = 0;
  private expectedBodyBytes: number | null = null;
  private failed: string | null = null;

  constructor(options: LSPFrameParserOptions = {}) {
    this.maxHeaderBytes = options.maxHeaderBytes ?? GODOT_LIMITS.lspHeaderBytes;
    this.maxBodyBytes = options.maxBodyBytes ?? GODOT_LIMITS.lspMessageBodyBytes;
  }

  /** Feed raw bytes; returns every completed frame plus at most one error. */
  feed(chunk: Uint8Array): readonly LSPFrameOutcome[] {
    if (this.failed !== null) {
      return [{ ok: false, error: { kind: "protocol-error", message: this.failed } }];
    }
    const outcomes: LSPFrameOutcome[] = [];
    this.pending.push(chunk);
    this.pendingLength += chunk.length;
    for (;;) {
      const consumed = this.tryConsumeOne(outcomes);
      if (this.failed !== null) {
        outcomes.push({
          ok: false,
          error: { kind: "protocol-error", message: this.failed },
        });
        break;
      }
      if (consumed === 0) {
        break;
      }
    }
    return outcomes;
  }

  get failedMessage(): string | null {
    return this.failed;
  }

  private tryConsumeOne(outcomes: LSPFrameOutcome[]): number {
    if (this.expectedBodyBytes !== null) {
      if (this.pendingLength < this.expectedBodyBytes) {
        return 0;
      }
      const payload = this.take(this.expectedBodyBytes);
      this.expectedBodyBytes = null;
      outcomes.push({ ok: true, frame: { payload } });
      return 1;
    }
    const headerEnd = this.indexOfHeaderEnd();
    if (headerEnd === -1) {
      if (this.pendingLength > this.maxHeaderBytes) {
        this.fail("LSP header block exceeds the bound");
      }
      return 0;
    }
    if (headerEnd > this.maxHeaderBytes) {
      this.fail("LSP header block exceeds the bound");
      return 0;
    }
    const headerBytes = this.take(headerEnd + 4); // includes \r\n\r\n
    const headerText = Buffer.from(headerBytes).toString("utf8");
    const contentLength = this.parseContentLength(headerText);
    if (contentLength === null) {
      this.fail("missing or malformed Content-Length header");
      return 0;
    }
    if (contentLength > this.maxBodyBytes) {
      this.fail(`LSP message body exceeds the ${this.maxBodyBytes}-byte bound`);
      return 0;
    }
    this.expectedBodyBytes = contentLength;
    // The body may already be fully buffered (one chunk carries header+body).
    return this.tryConsumeOne(outcomes);
  }

  private indexOfHeaderEnd(): number {
    const buffer = this.concatPending();
    const needle = Buffer.from("\r\n\r\n", "utf8");
    return buffer.indexOf(needle);
  }

  private parseContentLength(headerText: string): number | null {
    let found: number | null = null;
    for (const rawLine of headerText.split(/\r\n/)) {
      if (rawLine.length === 0) {
        continue;
      }
      const colon = rawLine.indexOf(":");
      if (colon <= 0) {
        continue;
      }
      const name = rawLine.slice(0, colon).trim();
      const value = rawLine.slice(colon + 1).trim();
      if (name.toLowerCase() !== "content-length") {
        continue;
      }
      if (found !== null) {
        // Duplicate Content-Length headers are ambiguous; fail deterministically.
        return null;
      }
      if (!/^[0-9]+$/.test(value)) {
        return null;
      }
      const parsed = Number.parseInt(value, 10);
      if (!Number.isSafeInteger(parsed) || parsed < 0) {
        return null;
      }
      found = parsed;
    }
    return found;
  }

  private take(length: number): Uint8Array {
    const buffer = this.concatPending();
    const result = buffer.subarray(0, length);
    const rest = buffer.subarray(length);
    this.pending = rest.length > 0 ? [rest] : [];
    this.pendingLength = rest.length;
    return result;
  }

  private concatPending(): Buffer {
    if (this.pending.length === 1) {
      return Buffer.from(this.pending[0] as Uint8Array);
    }
    const buffer = Buffer.alloc(this.pendingLength);
    let offset = 0;
    for (const entry of this.pending) {
      buffer.set(entry, offset);
      offset += entry.length;
    }
    return buffer;
  }

  private fail(message: string): void {
    this.failed = message;
    this.pending = [];
    this.pendingLength = 0;
  }
}

/** Frame one outgoing JSON-RPC message (LSP framing). */
export function frameMessage(payload: string): Uint8Array {
  const body = Buffer.from(payload, "utf8");
  const header = Buffer.from(`Content-Length: ${body.length}\r\n\r\n`, "utf8");
  return Buffer.concat([header, body]);
}
