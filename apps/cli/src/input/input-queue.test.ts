import { describe, expect, it } from "vitest";
import { createInputQueue } from "./input-queue.js";

interface ControllableLineReader {
  readLine: (prompt: string) => Promise<string | null>;
  pushLine: (line: string) => void;
  pushEof: () => void;
  pendingReads: () => number;
}

function createControllableReader(): ControllableLineReader {
  const waiters: Array<(line: string | null) => void> = [];
  return {
    readLine: (_prompt: string) =>
      new Promise<string | null>((resolve) => {
        waiters.push(resolve);
      }),
    pushLine(line: string) {
      const waiter = waiters.shift();
      if (waiter === undefined) {
        throw new Error("No pending terminal read.");
      }
      waiter(line);
    },
    pushEof() {
      const waiter = waiters.shift();
      if (waiter === undefined) {
        throw new Error("No pending terminal read.");
      }
      waiter(null);
    },
    pendingReads: () => waiters.length,
  };
}

describe("createInputQueue", () => {
  it("delivers typed lines to pending asks in order", async () => {
    const reader = createControllableReader();
    const queue = createInputQueue(reader.readLine, () => {});
    const first = queue.ask("> ");
    const second = queue.ask("> ");
    expect(reader.pendingReads()).toBe(1);
    reader.pushLine("line one");
    await expect(first).resolves.toEqual({ kind: "answer", value: "line one" });
    expect(reader.pendingReads()).toBe(1);
    reader.pushLine("line two");
    await expect(second).resolves.toEqual({ kind: "answer", value: "line two" });
  });

  it("resolves a timed-out ask and reroutes the later line to the next ask", async () => {
    const reader = createControllableReader();
    const queue = createInputQueue(reader.readLine, () => {});
    const timedOut = queue.ask("approve? ", { timeoutMs: 10 });
    await expect(timedOut).resolves.toEqual({ kind: "timeout" });
    const nextCommand = queue.ask("> ");
    reader.pushLine("hello world");
    await expect(nextCommand).resolves.toEqual({ kind: "answer", value: "hello world" });
    expect(reader.pendingReads()).toBe(0);
  });

  it("reroutes the line to the next live ask after an abort", async () => {
    const reader = createControllableReader();
    const queue = createInputQueue(reader.readLine, () => {});
    const controller = new AbortController();
    const aborted = queue.ask("approve? ", { signal: controller.signal });
    controller.abort();
    await expect(aborted).resolves.toEqual({ kind: "aborted" });
    const next = queue.ask("> ");
    reader.pushLine("next input");
    await expect(next).resolves.toEqual({ kind: "answer", value: "next input" });
  });

  it("resolves every pending ask with null on EOF", async () => {
    const reader = createControllableReader();
    const queue = createInputQueue(reader.readLine, () => {});
    const first = queue.ask("> ");
    const second = queue.ask("> ");
    reader.pushEof();
    await expect(first).resolves.toEqual({ kind: "answer", value: null });
    await expect(second).resolves.toEqual({ kind: "answer", value: null });
    await expect(queue.ask("> ")).resolves.toEqual({ kind: "answer", value: null });
  });

  it("survives repeated timeout cycles without losing input", async () => {
    const reader = createControllableReader();
    const queue = createInputQueue(reader.readLine, () => {});
    for (let index = 0; index < 3; index += 1) {
      const timedOut = queue.ask(`cycle ${index}: `, { timeoutMs: 5 });
      await expect(timedOut).resolves.toEqual({ kind: "timeout" });
    }
    const next = queue.ask("> ");
    reader.pushLine("still here");
    await expect(next).resolves.toEqual({ kind: "answer", value: "still here" });
  });

  it("discards the oldest pending ask without resolving the queue", async () => {
    const reader = createControllableReader();
    const queue = createInputQueue(reader.readLine, () => {});
    const busyAsk = queue.ask("");
    queue.cancelPendingAsk();
    await expect(busyAsk).resolves.toEqual({ kind: "discarded" });
    const mainAsk = queue.ask("> ");
    reader.pushLine("typed later");
    await expect(mainAsk).resolves.toEqual({ kind: "answer", value: "typed later" });
  });

  it("buffers a line that arrives with no live entry and delivers it to the next ask", async () => {
    const reader = createControllableReader();
    const queue = createInputQueue(reader.readLine, () => {});
    const stale = queue.ask("approve? ", { timeoutMs: 5 });
    await expect(stale).resolves.toEqual({ kind: "timeout" });
    queue.cancelPendingAsk();
    reader.pushLine("buffered line");
    const next = queue.ask("> ");
    await expect(next).resolves.toEqual({ kind: "answer", value: "buffered line" });
  });

  it("removes timers and abort listeners when an ask settles", async () => {
    const reader = createControllableReader();
    const queue = createInputQueue(reader.readLine, () => {});
    const controller = new AbortController();
    const first = queue.ask("> ", { signal: controller.signal });
    const second = queue.ask("> ", { signal: controller.signal });
    reader.pushLine("first line");
    await expect(first).resolves.toEqual({ kind: "answer", value: "first line" });
    controller.abort();
    await expect(second).resolves.toEqual({ kind: "aborted" });
    const after = queue.ask("> ");
    reader.pushLine("after abort");
    await expect(after).resolves.toEqual({ kind: "answer", value: "after abort" });
  });
});
