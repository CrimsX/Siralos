import { describe, expect, it } from "vitest";
import type { ApprovalRequest, ApprovalReviewer } from "@solaris/core";
import { createInteractiveApprovalReviewer } from "./approval-reviewer.js";
import type { SessionIO } from "../interactive-session.js";

class ScriptedIO implements SessionIO {
  private readonly lines: readonly string[];
  private index = 0;
  private readonly chunks: string[] = [];

  constructor(lines: readonly string[]) {
    this.lines = lines;
  }

  ask(_prompt: string): Promise<string | null> {
    if (this.index >= this.lines.length) {
      return Promise.resolve(null);
    }
    const line = this.lines[this.index];
    this.index += 1;
    return Promise.resolve(line === undefined ? null : line);
  }

  write(text: string): void {
    this.chunks.push(text);
  }

  clear(): void {}

  get text(): string {
    return this.chunks.join("");
  }
}

class HangingIO implements SessionIO {
  ask(): Promise<string | null> {
    return new Promise(() => {});
  }

  write(): void {}

  clear(): void {}
}

function createRequest(overrides: Partial<ApprovalRequest> = {}): ApprovalRequest {
  return {
    id: "approval-1",
    capability: "workspace.write",
    toolName: "workspace.edit_file",
    summary: "1 file, +1 -1",
    paths: ["README.md"],
    preview: {
      files: [
        {
          path: "README.md",
          operation: "update",
          beforeSha256: "before",
          afterSha256: "after",
          addedLines: 1,
          removedLines: 1,
          unifiedDiff: "--- README.md\n+++ README.md\n@@\n-old\n+new\n",
        },
      ],
      totalAddedLines: 1,
      totalRemovedLines: 1,
      truncated: false,
    },
    ...overrides,
  };
}

async function reviewWith(
  lines: readonly string[],
): Promise<{ decision: Awaited<ReturnType<ApprovalReviewer["review"]>>; text: string }> {
  const io = new ScriptedIO(lines);
  const interactive = createInteractiveApprovalReviewer(io, 60_000);
  const decision = await interactive.review(createRequest());
  return { decision, text: io.text };
}

describe("createInteractiveApprovalReviewer", () => {
  it("approves once on y", async () => {
    const { decision } = await reviewWith(["y"]);
    expect(decision).toEqual({ type: "approve_once" });
  });

  it("approves once on yes", async () => {
    const { decision } = await reviewWith(["yes"]);
    expect(decision).toEqual({ type: "approve_once" });
  });

  it("denies on n", async () => {
    const { decision } = await reviewWith(["n"]);
    expect(decision).toMatchObject({ type: "deny" });
  });

  it("denies on empty input", async () => {
    const { decision } = await reviewWith([""]);
    expect(decision).toMatchObject({ type: "deny" });
  });

  it("denies on end of input", async () => {
    const { decision } = await reviewWith([]);
    expect(decision).toMatchObject({ type: "deny" });
  });

  it("approves after one invalid answer followed by y", async () => {
    const { decision } = await reviewWith(["maybe", "y"]);
    expect(decision).toEqual({ type: "approve_once" });
  });

  it("denies after two invalid answers", async () => {
    const { decision } = await reviewWith(["maybe", "sure"]);
    expect(decision).toMatchObject({ type: "deny" });
  });

  it("renders the tool, capability, file, change, and diff", async () => {
    const { text } = await reviewWith(["n"]);
    expect(text).toContain("Approval required");
    expect(text).toContain("Tool: workspace.edit_file");
    expect(text).toContain("Capability: workspace.write");
    expect(text).toContain("File: README.md");
    expect(text).toContain("Change: +1 -1");
    expect(text).toContain("-old");
    expect(text).toContain("+new");
  });

  it("cancels when the signal is aborted while waiting", async () => {
    const io = new HangingIO();
    const reviewer = createInteractiveApprovalReviewer(io, 60_000);
    const controller = new AbortController();
    const promise = reviewer.review(createRequest(), controller.signal);
    controller.abort();
    const decision = await promise;
    expect(decision).toEqual({ type: "cancelled" });
  });

  it("cancels on timeout", async () => {
    const io = new HangingIO();
    const reviewer = createInteractiveApprovalReviewer(io, 20);
    const decision = await reviewer.review(createRequest());
    expect(decision).toEqual({ type: "cancelled" });
  });

  it("cancels immediately when the signal is pre-aborted", async () => {
    const io = new HangingIO();
    const reviewer = createInteractiveApprovalReviewer(io, 60_000);
    const controller = new AbortController();
    controller.abort();
    const decision = await reviewer.review(createRequest(), controller.signal);
    expect(decision).toEqual({ type: "cancelled" });
  });
});
