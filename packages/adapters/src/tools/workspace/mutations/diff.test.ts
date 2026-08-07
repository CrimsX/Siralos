import { describe, expect, it } from "vitest";
import { buildUnifiedDiff, countLines } from "./diff.js";
import { WORKSPACE_LIMITS } from "../limits.js";

describe("buildUnifiedDiff", () => {
  it("produces a complete creation diff", () => {
    const result = buildUnifiedDiff("docs/example.md", "", "# Example\n\nBody\n");
    expect(result.status).toBe("ready");
    if (result.status !== "ready") {
      return;
    }
    expect(result.diff.addedLines).toBe(3);
    expect(result.diff.removedLines).toBe(0);
    expect(result.diff.unifiedDiff).toContain("+# Example");
    expect(result.diff.unifiedDiff).toContain("+Body");
  });

  it("produces a complete deletion diff", () => {
    const result = buildUnifiedDiff("docs/obsolete.md", "line one\nline two\n", "");
    expect(result.status).toBe("ready");
    if (result.status !== "ready") {
      return;
    }
    expect(result.diff.addedLines).toBe(0);
    expect(result.diff.removedLines).toBe(2);
    expect(result.diff.unifiedDiff).toContain("-line one");
    expect(result.diff.unifiedDiff).toContain("-line two");
  });

  it("produces correct statistics for an update", () => {
    const result = buildUnifiedDiff(
      "file.txt",
      "one\ntwo\nthree\n",
      "one\ntwo-changed\nthree\nfour\n",
    );
    expect(result.status).toBe("ready");
    if (result.status !== "ready") {
      return;
    }
    expect(result.diff.addedLines).toBe(2);
    expect(result.diff.removedLines).toBe(1);
    expect(result.diff.unifiedDiff).toContain("-two");
    expect(result.diff.unifiedDiff).toContain("+two-changed");
    expect(result.diff.unifiedDiff).toContain("+four");
  });

  it("is deterministic for identical inputs", () => {
    const before = "a\nb\nc\n";
    const after = "a\nb-changed\nc\n";
    const first = buildUnifiedDiff("f.txt", before, after);
    const second = buildUnifiedDiff("f.txt", before, after);
    expect(first).toEqual(second);
  });

  it("omits svn-style Index headers from previews", () => {
    const result = buildUnifiedDiff("f.txt", "a\nb\n", "a\nc\n");
    expect(result.status).toBe("ready");
    if (result.status === "ready") {
      expect(result.diff.unifiedDiff).toMatch(/^--- f\.txt\n\+\+\+ f\.txt\n/);
      expect(result.diff.unifiedDiff).not.toContain("Index:");
    }
  });

  it("fails when the change involves too many lines", () => {
    const huge = `${"x\n".repeat(WORKSPACE_LIMITS.maxDiffLines + 1)}`;
    const result = buildUnifiedDiff("f.txt", huge, huge);
    expect(result.status).toBe("too_large");
  });

  it("counts lines without phantom lines for empty text", () => {
    expect(countLines("")).toBe(0);
    expect(countLines("a")).toBe(1);
    expect(countLines("a\nb\n")).toBe(2);
  });
});
