import { describe, expect, it } from "vitest";
import {
  TerminalSanitizer,
  sanitizeForDisplay,
  sanitizePathForDisplay,
  formatApprovalPrompt,
  formatUndoOutcome,
  formatGitDiff,
  formatSafeDoctorReport,
  formatSelfReference,
  formatSolarisDoctorReport,
} from "./output.js";
import { toSafeReport } from "@solaris/core";
import type { GitDiffResult, WorkspaceWriteApprovalRequest } from "@solaris/core";

describe("TerminalSanitizer", () => {
  it("preserves ordinary text and readable newlines", () => {
    const sanitizer = new TerminalSanitizer();
    expect(sanitizer.push("hello world\nsecond line\twith tab")).toBe(
      "hello world\nsecond line\twith tab",
    );
  });

  it("strips ANSI CSI sequences completely", () => {
    const sanitizer = new TerminalSanitizer();
    expect(sanitizer.push("before\u001b[31mred\u001b[0mafter")).toBe("beforeredafter");
  });

  it("strips OSC sequences including links, titles, and clipboard writes", () => {
    const sanitizer = new TerminalSanitizer();
    expect(sanitizer.push("a\u001b]8;;https://evil.example\u0007link\u001b]8;;\u0007b")).toBe(
      "alinkb",
    );
    expect(sanitizer.push("\u001b]0;title\u0007rest")).toBe("rest");
    expect(sanitizer.push("\u001b]52;c;c2VjcmV0\u0007rest2")).toBe("rest2");
  });

  it("renders carriage return, backspace, and DEL visibly", () => {
    const sanitizer = new TerminalSanitizer();
    expect(sanitizer.push("a\rb\u0008c\u007fd")).toBe("a^Mb^Hc^?d");
  });

  it("replaces other C0 and C1 controls", () => {
    const sanitizer = new TerminalSanitizer();
    expect(sanitizer.push("a\u0000b\u0003c\u0085d\u009fe")).toBe("a^@b^Cc\uFFFDd\uFFFDe");
  });

  it("handles sequences fragmented across chunks", () => {
    const sanitizer = new TerminalSanitizer();
    expect(sanitizer.push("x\u001b")).toBe("x");
    expect(sanitizer.push("[3")).toBe("");
    expect(sanitizer.push("1mboom\u001b]8;;https://e")).toBe("boom");
    expect(sanitizer.push("\u0007tail")).toBe("tail");
  });

  it("drops a dangling sequence at flush", () => {
    const sanitizer = new TerminalSanitizer();
    expect(sanitizer.push("safe\u001b[31m")).toBe("safe");
    expect(sanitizer.flush()).toBe("");
    expect(sanitizer.push("after")).toBe("after");
  });

  it("keeps ordinary unicode intact", () => {
    const sanitizer = new TerminalSanitizer();
    expect(sanitizer.push("caf\u00e9 \u4e2d\u6587 \u{1F600}")).toBe(
      "caf\u00e9 \u4e2d\u6587 \u{1F600}",
    );
  });

  it("does not let a dangling escape corrupt the next chunk", () => {
    const sanitizer = new TerminalSanitizer();
    expect(sanitizer.push("\u001b")).toBe("");
    expect(sanitizer.push("> prompt text")).toBe(" prompt text");
    expect(sanitizer.push("[99m")).toBe("[99m");
    expect(sanitizer.push("ok")).toBe("ok");
  });

  it("preserves emoji split across chunk boundaries through the encoding boundary", () => {
    const sanitizer = new TerminalSanitizer();
    const emoji = "\u{1F600}\u{1F680}\u{1F4A9}";
    const chunks = [...emoji].map((codepoint) => codepoint);
    const rendered = chunks.map((chunk) => sanitizer.push(chunk)).join("");
    expect(rendered).toBe(emoji);
  });

  it("pairs a surrogate split in the middle of a multi-byte codepoint across chunks", () => {
    const sanitizer = new TerminalSanitizer();
    const rocket = "\u{1F680}";
    // Split between the high and low surrogate.
    expect(sanitizer.push(rocket.slice(0, 1))).toBe("");
    expect(sanitizer.push(rocket.slice(1))).toBe(rocket);
  });

  it("renders a dangling high surrogate at flush instead of corrupting later output", () => {
    const sanitizer = new TerminalSanitizer();
    expect(sanitizer.push("before \uD83D")).toBe("before ");
    expect(sanitizer.flush()).toBe("\uFFFD");
    expect(sanitizer.push(" after")).toBe(" after");
  });

  it("renders a lone low surrogate visibly", () => {
    const sanitizer = new TerminalSanitizer();
    expect(sanitizer.push("a\uDE00b")).toBe("a\uFFFDb");
  });
});

describe("sanitizeForDisplay", () => {
  it("sanitizes complete and partial control sequences in one shot", () => {
    expect(sanitizeForDisplay("diff \u001b[1mheader\u001b[0m")).toBe("diff header");
    expect(sanitizeForDisplay("partial \u001b[31")).toBe("partial ");
    expect(sanitizeForDisplay("osc \u001b]52;c;bGFtZXI=\u0007 end")).toBe("osc  end");
    expect(sanitizeForDisplay("cr rewrite\rback")).toBe("cr rewrite^Mback");
  });
});

describe("sanitizePathForDisplay", () => {
  it("escapes embedded newlines so paths cannot spoof additional output lines", () => {
    expect(sanitizePathForDisplay("notes\nApproval: granted.md")).toBe(
      "notes\\nApproval: granted.md",
    );
    expect(sanitizePathForDisplay("a\nb")).toBe("a\\nb");
    expect(sanitizePathForDisplay("a\rb\tc")).toBe("a\\rb\\tc");
    expect(sanitizePathForDisplay("a\\b")).toBe("a\\\\b");
    expect(sanitizePathForDisplay("a\u0007b\u007fc")).toBe("a^Gb^?c");
    expect(sanitizePathForDisplay(null)).toBe("(none)");
  });

  it("keeps ordinary unicode and forward separators intact", () => {
    expect(sanitizePathForDisplay("src/ma\u00efn.gd \u{1F680}")).toBe("src/ma\u00efn.gd \u{1F680}");
  });
});

describe("path-bearing display output", () => {
  function approvalRequest(path: string): WorkspaceWriteApprovalRequest {
    return {
      id: "a1",
      capability: "workspace.write",
      toolName: "workspace.edit_file",
      summary: "1 file, +1 -1",
      paths: [path],
      preview: {
        files: [
          {
            path,
            operation: "update",
            beforeSha256: null,
            afterSha256: null,
            addedLines: 1,
            removedLines: 1,
            unifiedDiff: "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new",
          },
        ],
        totalAddedLines: 1,
        totalRemovedLines: 1,
        truncated: false,
      },
      digest: "d".repeat(64),
    };
  }

  it("renders a hostile filename without fabricating an approval line", () => {
    const rendered = formatApprovalPrompt(
      approvalRequest("evil\nApproved: yes\nstill-one-path.txt"),
    );
    expect(rendered).toContain("M evil\\nApproved: yes\\nstill-one-path.txt");
    expect(rendered).not.toContain("Approved: yes\n");
  });

  it("renders hostile checkpoint and undo paths without extra lines", () => {
    const undo = formatUndoOutcome({
      type: "undone",
      checkpointId: "cp-1",
      path: "a\n[undo complete]",
    });
    expect(undo).toBe("\u25CF Checkpoint cp-1 undone (a\\n[undo complete])\n");

    const diff = formatGitDiff({
      scope: "working",
      files: [
        {
          operation: "modify" as const,
          path: "x\nsecond-line.txt",
          originalPath: "y\rname.txt",
          binary: false,
          addedLines: 1,
          removedLines: 1,
        },
      ],
      patch: "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new",
      truncated: false,
      untrackedExcluded: true,
    } satisfies GitDiffResult);
    expect(diff).toContain("modify x\\nsecond-line.txt (from y\\rname.txt)");
    // The hostile path renders as one single line: no line begins with the
    // text that followed the embedded newline.
    expect(diff.split("\n").some((line) => line.startsWith("second-line.txt"))).toBe(false);
  });
});

describe("solaris doctor formatters", () => {
  const report = {
    schemaVersion: 1,
    generatedAtMs: 1_700_000_000_000,
    runtime: { version: "0.0.0", nodeMajor: 24, platform: "linux" },
    requestedAreas: ["runtime", "sandbox"],
    checks: [
      {
        id: "runtime.node_version",
        area: "runtime",
        status: "pass",
        summary: "Node.js 24 is supported",
      },
      {
        id: "sandbox.backend",
        area: "sandbox",
        status: "warn",
        summary: "Sandbox backend x is setup-required",
      },
      {
        id: "sandbox.required_enforcement",
        area: "sandbox",
        status: "skip",
        summary: "Profile inspect does not require sandboxed execution",
      },
    ],
    counts: { pass: 1, warn: 1, fail: 0, skip: 1, total: 3 },
    snapshot: null,
  } as import("@solaris/core").DoctorReport;

  it("formats the human report with per-area status lines and interesting checks", () => {
    const text = formatSolarisDoctorReport(report);
    expect(text).toContain("Solaris Doctor");
    expect(text).toContain("runtime         PASS");
    expect(text).toContain("sandbox         WARN");
    expect(text).toContain("1 passed, 1 warning, 0 failed, 1 skipped.");
    expect(text).toContain("sandbox: Sandbox backend x is setup-required");
    expect(text).not.toContain("Profile inspect does not require"); // skip checks are not "interesting"
  });

  it("formats the safe report with sanitized summaries and categories", () => {
    const safe = toSafeReport(report);
    const text = formatSafeDoctorReport(safe);
    expect(text).toContain("Solaris Doctor (safe report)");
    expect(text).toContain("Schema: 1");
    expect(text).toContain("[WARN] sandbox sandbox.backend: Sandbox backend x is setup-required");
    expect(text).toContain("Category: sandbox warn x1");
  });

  it("formats the self-reference identity", () => {
    const self = {
      name: "@solaris",
      runtime: { version: "0.0.0", nodeMajor: 24, platform: "linux" },
      revision: "a".repeat(64),
      sections: [{ id: "commands", title: "Interactive commands", lines: [] }],
    } as unknown as import("@solaris/core").SelfReference;
    const text = formatSelfReference(self);
    expect(text).toContain("@solaris — installed Solaris runtime");
    expect(text).toContain("Version: 0.0.0");
    expect(text).toContain(`Self-reference revision: ${"a".repeat(64)}`);
    expect(text).toContain("commands");
  });
});
