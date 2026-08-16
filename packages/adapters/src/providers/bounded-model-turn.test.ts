import { describe, expect, it } from "vitest";
import type { ModelEvent, ModelProvider } from "@siralos/core";
import {
  collectBoundedModelTurn,
  detachBoundedToolResult,
  type BoundedModelTurnLimits,
} from "./bounded-model-turn.js";

const LIMITS: BoundedModelTurnLimits = {
  maxTextBytes: 32,
  maxTextEvents: 2,
  maxToolCalls: 2,
  maxToolNameBytes: 16,
  maxCallIdBytes: 16,
  maxToolArgumentBytes: 32,
  maxTurnBytes: 64,
};

function provider(events: readonly ModelEvent[]): ModelProvider {
  return {
    id: "bounded-turn-test",
    async *stream() {
      for (const event of events) {
        yield event;
        await Promise.resolve();
      }
    },
  };
}

function collect(
  events: readonly ModelEvent[],
  overrides: Partial<BoundedModelTurnLimits> = {},
  seenCallIds?: Set<string>,
) {
  return collectBoundedModelTurn({
    actor: "The test actor",
    provider: provider(events),
    messages: [],
    tools: [],
    signal: new AbortController().signal,
    limits: { ...LIMITS, ...overrides },
    ...(seenCallIds === undefined ? {} : { seenCallIds }),
  });
}

async function expectFailure(pending: ReturnType<typeof collect>, message: string): Promise<void> {
  const outcome = await pending;
  expect(outcome.kind).toBe("failed");
  if (outcome.kind === "failed") {
    expect(outcome.message).toContain(message);
  }
}

describe("collectBoundedModelTurn", () => {
  it("accepts a completed bounded text-and-tool turn", async () => {
    const input = { path: "a" };
    const outcome = await collect([
      { type: "text_delta", text: "inspect" },
      { type: "tool_call", callId: "call-1", toolName: "read", input },
      { type: "completed" },
    ]);

    expect(outcome).toMatchObject({ kind: "turn", text: "inspect" });
    if (outcome.kind === "turn") {
      expect(outcome.toolCalls).toHaveLength(1);
      input.path = "mutated-after-validation";
      expect(outcome.toolCalls[0]?.input).toEqual({ path: "a" });
    }
  });

  it("requires exactly one completion terminator", async () => {
    await expectFailure(
      collect([{ type: "text_delta", text: "unterminated" }]),
      "without a completion event",
    );
    await expectFailure(
      collect([{ type: "completed" }, { type: "text_delta", text: "late" }]),
      "after completion",
    );
  });

  it("bounds text bytes and text-event count cumulatively", async () => {
    await expectFailure(
      collect(
        [
          { type: "text_delta", text: "界" },
          { type: "text_delta", text: "界" },
          { type: "completed" },
        ],
        { maxTextBytes: 5 },
      ),
      "byte limit",
    );
    await expectFailure(
      collect(
        [
          { type: "text_delta", text: "a" },
          { type: "text_delta", text: "b" },
          { type: "completed" },
        ],
        { maxTextEvents: 1 },
      ),
      "text-event",
    );
  });

  it("bounds call ids, names, arguments, call count, and aggregate turn bytes", async () => {
    const cases: Array<{
      events: ModelEvent[];
      limits: Partial<BoundedModelTurnLimits>;
      message: string;
    }> = [
      {
        events: [
          { type: "tool_call", callId: "long-id", toolName: "read", input: {} },
          { type: "completed" },
        ],
        limits: { maxCallIdBytes: 3 },
        message: "id byte limit",
      },
      {
        events: [
          { type: "tool_call", callId: "c", toolName: "long-name", input: {} },
          { type: "completed" },
        ],
        limits: { maxToolNameBytes: 3 },
        message: "tool-name byte limit",
      },
      {
        events: [
          { type: "tool_call", callId: "c", toolName: "read", input: { value: "large" } },
          { type: "completed" },
        ],
        limits: { maxToolArgumentBytes: 5 },
        message: "tool-argument byte limit",
      },
      {
        events: [
          { type: "tool_call", callId: "c1", toolName: "read", input: {} },
          { type: "tool_call", callId: "c2", toolName: "read", input: {} },
          { type: "completed" },
        ],
        limits: { maxToolCalls: 1 },
        message: "tool-call limit",
      },
      {
        events: [
          { type: "text_delta", text: "1234" },
          { type: "tool_call", callId: "c", toolName: "read", input: {} },
          { type: "completed" },
        ],
        limits: { maxTurnBytes: 5 },
        message: "aggregate turn byte limit",
      },
    ];

    for (const entry of cases) {
      const outcome = await collect(entry.events, entry.limits);
      expect(outcome.kind).toBe("failed");
      if (outcome.kind === "failed") {
        expect(outcome.message).toContain(entry.message);
      }
    }
  });

  it("rejects an unknown event discriminator, even tool-call-shaped", async () => {
    await expectFailure(
      collect([
        {
          type: "unexpected",
          callId: "call-1",
          toolName: "read",
          input: { path: "a" },
        } as unknown as ModelEvent,
      ]),
      "unknown event type",
    );
  });

  it("still accepts a valid tool call after protocol hardening", async () => {
    const outcome = await collect([
      { type: "tool_call", callId: "call-ok", toolName: "read", input: {} },
      { type: "completed" },
    ]);
    expect(outcome).toMatchObject({ kind: "turn" });
    if (outcome.kind === "turn") {
      expect(outcome.toolCalls).toHaveLength(1);
    }
  });

  it("still rejects a duplicate call id within one turn", async () => {
    await expectFailure(
      collect([
        { type: "tool_call", callId: "call-dupe", toolName: "read", input: {} },
        { type: "tool_call", callId: "call-dupe", toolName: "read", input: {} },
        { type: "completed" },
      ]),
      "duplicate",
    );
  });

  it("still rejects empty call ids and tool names", async () => {
    await expectFailure(
      collect([
        { type: "tool_call", callId: "", toolName: "read", input: {} },
        { type: "completed" },
      ]),
      "empty id or name",
    );
    await expectFailure(
      collect([
        { type: "tool_call", callId: "call-1", toolName: "", input: {} },
        { type: "completed" },
      ]),
      "empty id or name",
    );
  });

  it("rejects a text delta with a non-string payload", async () => {
    await expectFailure(
      collect([{ type: "text_delta", text: 42 } as unknown as ModelEvent]),
      "text event without a string payload",
    );
  });

  it("rejects a tool call with a non-string id or name", async () => {
    await expectFailure(
      collect([
        { type: "tool_call", callId: 7, toolName: "read", input: {} } as unknown as ModelEvent,
      ]),
      "non-string id or name",
    );
    await expectFailure(
      collect([
        { type: "tool_call", callId: "call-1", toolName: 9, input: {} } as unknown as ModelEvent,
      ]),
      "non-string id or name",
    );
  });

  it("rejects a malformed non-object event", async () => {
    await expectFailure(collect([null as unknown as ModelEvent]), "malformed event");
    await expectFailure(collect([5 as unknown as ModelEvent]), "malformed event");
  });

  it("still reports after-completion for an unknown event after completed", async () => {
    await expectFailure(
      collect([
        { type: "completed" },
        { type: "unexpected", callId: "c1", toolName: "read", input: {} } as unknown as ModelEvent,
      ]),
      "after completion",
    );
  });

  it("rejects non-serializable tool arguments and call-id reuse across turns", async () => {
    const cyclic: { self?: unknown } = {};
    cyclic.self = cyclic;
    await expectFailure(
      collect([
        { type: "tool_call", callId: "call-cyclic", toolName: "read", input: cyclic },
        { type: "completed" },
      ]),
      "JSON-serializable",
    );

    const seen = new Set<string>();
    const first = await collect(
      [
        { type: "tool_call", callId: "call-reused", toolName: "read", input: {} },
        { type: "completed" },
      ],
      {},
      seen,
    );
    expect(first.kind).toBe("turn");
    await expectFailure(
      collect(
        [
          { type: "tool_call", callId: "call-reused", toolName: "read", input: {} },
          { type: "completed" },
        ],
        {},
        seen,
      ),
      "duplicate",
    );
  });
});

describe("detachBoundedToolResult", () => {
  it("returns an exact JSON-detached success shape", () => {
    const output = { ok: true };
    const detached = detachBoundedToolResult(
      { status: "success", output, summary: "ok", unknown: "discarded" },
      1024,
      "test.tool",
    );

    expect(detached.ok).toBe(true);
    if (detached.ok) {
      output.ok = false;
      expect(detached.result).toEqual({ status: "success", output: { ok: true }, summary: "ok" });
      expect(detached.result).not.toHaveProperty("unknown");
    }
  });

  it("rejects cyclic, oversized, and malformed tool results", () => {
    const cyclic: { self?: unknown } = {};
    cyclic.self = cyclic;
    expect(detachBoundedToolResult(cyclic, 1024, "test.tool").ok).toBe(false);
    expect(
      detachBoundedToolResult(
        { status: "success", output: { text: "x".repeat(100) }, summary: "large" },
        32,
        "test.tool",
      ).ok,
    ).toBe(false);
    expect(
      detachBoundedToolResult({ status: "invented", message: "bad" }, 1024, "test.tool").ok,
    ).toBe(false);
  });
});
