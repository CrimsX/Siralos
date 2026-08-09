import { describe, expect, it } from "vitest";
import { GODOT_LIMITS } from "@solaris/core";
import { LSPFrameParser, frameMessage } from "./frame-parser.js";

function payload(text: string): Uint8Array {
  return new TextEncoder().encode(text);
}

function collect(parser: LSPFrameParser, chunks: readonly Uint8Array[]) {
  const outcomes = [];
  for (const chunk of chunks) {
    outcomes.push(...parser.feed(chunk));
  }
  return outcomes;
}

describe("LSPFrameParser", () => {
  it("parses a complete framed message", () => {
    const parser = new LSPFrameParser();
    const outcomes = parser.feed(frameMessage('{"jsonrpc":"2.0","id":1}'));
    expect(outcomes).toHaveLength(1);
    expect(outcomes[0]?.ok).toBe(true);
    if (outcomes[0]?.ok) {
      expect(Buffer.from(outcomes[0].frame.payload).toString("utf8")).toBe(
        '{"jsonrpc":"2.0","id":1}',
      );
    }
  });

  it("handles a fragmented Content-Length header", () => {
    const parser = new LSPFrameParser();
    const frame = frameMessage('{"jsonrpc":"2.0"}');
    const text = Buffer.from(frame).toString("utf8");
    const outcomes = collect(parser, [
      payload(text.slice(0, 4)),
      payload(text.slice(4, 18)),
      payload(text.slice(18)),
    ]);
    expect(outcomes.filter((entry) => entry.ok)).toHaveLength(1);
  });

  it("handles a fragmented body (including multibyte UTF-8 splits)", () => {
    const parser = new LSPFrameParser();
    const body = '{"text":"héllo 😀"}';
    const frame = frameMessage(body);
    const bytes = Buffer.from(frame);
    const outcomes = collect(parser, [
      bytes.subarray(0, bytes.length - 6),
      bytes.subarray(bytes.length - 6),
    ]);
    expect(outcomes.filter((entry) => entry.ok)).toHaveLength(1);
    if (outcomes[0]?.ok) {
      expect(Buffer.from(outcomes[0].frame.payload).toString("utf8")).toBe(body);
    }
  });

  it("handles multiple messages in one socket read", () => {
    const parser = new LSPFrameParser();
    const combined = Buffer.concat([
      Buffer.from(frameMessage('{"jsonrpc":"2.0","id":1}')),
      Buffer.from(frameMessage('{"jsonrpc":"2.0","id":2}')),
      Buffer.from(frameMessage('{"jsonrpc":"2.0","id":3}')),
    ]);
    const outcomes = parser.feed(combined);
    expect(outcomes.filter((entry) => entry.ok)).toHaveLength(3);
  });

  it("rejects invalid Content-Length values", () => {
    for (const header of [
      "Content-Length: abc\r\n\r\n",
      "Content-Length: -5\r\n\r\n",
      "Content-Length: 1.5\r\n\r\n",
      "Content-Length:\r\n\r\n",
      "Content-Length: 99999999999999999999\r\n\r\n",
    ]) {
      const parser = new LSPFrameParser();
      const outcomes = parser.feed(payload(`${header}{}`));
      expect(outcomes.some((entry) => !entry.ok)).toBe(true);
      expect(parser.failedMessage).not.toBeNull();
    }
  });

  it("rejects missing Content-Length", () => {
    const parser = new LSPFrameParser();
    const outcomes = parser.feed(payload("Content-Type: application/json\r\n\r\n{}"));
    expect(outcomes.some((entry) => !entry.ok)).toBe(true);
  });

  it("rejects duplicate Content-Length headers deterministically", () => {
    const parser = new LSPFrameParser();
    const outcomes = parser.feed(payload("Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}"));
    expect(outcomes.some((entry) => !entry.ok)).toBe(true);
  });

  it("bounds the header block", () => {
    const parser = new LSPFrameParser({ maxHeaderBytes: 64 });
    const huge = `${"x".repeat(100)}\r\n\r\n{}`;
    const outcomes = parser.feed(payload(huge));
    expect(outcomes.some((entry) => !entry.ok && entry.error.message.includes("header"))).toBe(
      true,
    );
  });

  it("bounds the message body", () => {
    const parser = new LSPFrameParser({ maxBodyBytes: 16 });
    const body = "x".repeat(32);
    const outcomes = parser.feed(payload(`Content-Length: ${body.length}\r\n\r\n${body}`));
    expect(outcomes.some((entry) => !entry.ok && entry.error.message.includes("body"))).toBe(true);
  });

  it("waits for the full body before emitting a frame", () => {
    const parser = new LSPFrameParser();
    const frame = frameMessage('{"jsonrpc":"2.0"}');
    const bytes = Buffer.from(frame);
    const first = parser.feed(bytes.subarray(0, bytes.length - 2));
    expect(first.filter((entry) => entry.ok)).toHaveLength(0);
    const second = parser.feed(bytes.subarray(bytes.length - 2));
    expect(second.filter((entry) => entry.ok)).toHaveLength(1);
  });

  it("fails deterministically and ignores input after a protocol error", () => {
    const parser = new LSPFrameParser();
    const bad = parser.feed(payload("Content-Length: nope\r\n\r\n{}"));
    expect(bad.some((entry) => !entry.ok)).toBe(true);
    const after = parser.feed(frameMessage('{"jsonrpc":"2.0"}'));
    expect(after.every((entry) => !entry.ok)).toBe(true);
    expect(after.length).toBeGreaterThan(0);
  });

  it("accepts the documented maximum body size", () => {
    const parser = new LSPFrameParser();
    const body = JSON.stringify({ payload: "x".repeat(GODOT_LIMITS.lspMessageBodyBytes - 64) });
    const outcomes = parser.feed(frameMessage(body));
    expect(outcomes.filter((entry) => entry.ok)).toHaveLength(1);
  });

  it("frameMessage emits the exact LSP framing", () => {
    const bytes = Buffer.from(frameMessage('{"a":1}'));
    expect(bytes.toString("utf8")).toBe('Content-Length: 7\r\n\r\n{"a":1}');
  });
});
