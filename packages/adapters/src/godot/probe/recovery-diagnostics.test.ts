import { describe, expect, it } from "vitest";
import {
  classifyDiagnosticLine,
  classifyRecoveryDiagnostics,
  emptyRecoveryDiagnosticSummary,
  sanitizeDiagnosticText,
} from "./recovery-diagnostics.js";

describe("classifyDiagnosticLine", () => {
  it("classifies Godot ERROR and WARNING markers", () => {
    expect(classifyDiagnosticLine("ERROR: something broke")).toMatchObject({
      severity: "error",
      category: "unknown",
      message: "something broke",
    });
    expect(classifyDiagnosticLine("WARNING: be careful")).toMatchObject({
      severity: "warning",
      category: "unknown",
      message: "be careful",
    });
    expect(classifyDiagnosticLine("SCRIPT ERROR: parse error at line 3")).toMatchObject({
      severity: "error",
      category: "parser",
      message: "parse error at line 3",
    });
  });

  it("recognizes well-known error patterns conservatively", () => {
    expect(classifyDiagnosticLine("Failed to import: res://x.png")).toMatchObject({
      severity: "error",
      category: "import",
    });
    expect(classifyDiagnosticLine("cannot open file foo")).toMatchObject({
      severity: "error",
      category: "resource",
    });
    expect(classifyDiagnosticLine("missing script res://a.gd")).toMatchObject({
      severity: "error",
      category: "script",
    });
  });

  it("never treats arbitrary stdout as structured truth", () => {
    expect(classifyDiagnosticLine("Godot Engine v4.7.1.stable - hello world")).toBeNull();
    expect(classifyDiagnosticLine("not an error at all")).toBeNull();
    expect(classifyDiagnosticLine("")).toBeNull();
  });

  it("sanitizes control characters from messages", () => {
    const line = "ERROR: bad \u001b[31mred\u001b[0m \x00 content";
    const classified = classifyDiagnosticLine(line);
    expect(classified?.message).toBe("bad [31mred[0m  content");
  });
});

describe("sanitizeDiagnosticText", () => {
  it("strips control characters and trims", () => {
    expect(sanitizeDiagnosticText("\x1b[31mhello\x00")).toBe("[31mhello");
    expect(sanitizeDiagnosticText("  plain text  ")).toBe("plain text");
  });
});

describe("classifyRecoveryDiagnostics", () => {
  it("classifies and bounds both streams", () => {
    const stdout = ["Godot Engine v4.7", "WARNING: first warning", "WARNING: second warning"].join(
      "\n",
    );
    const stderr = "ERROR: fatal\n";
    const summary = classifyRecoveryDiagnostics(stdout, stderr, { maxWarnings: 1 });
    expect(summary.errors).toHaveLength(1);
    expect(summary.warnings).toHaveLength(1);
    expect(summary.truncated).toBe(true);
  });

  it("caps the retained raw lines", () => {
    const noisy = Array.from({ length: 300 }, (_, index) => `line ${index}`).join("\n");
    const summary = classifyRecoveryDiagnostics(noisy, "", { maxRawLines: 100 });
    expect(summary.truncated).toBe(true);
  });

  it("caps retained diagnostics per severity", () => {
    const errors = Array.from({ length: 50 }, () => "ERROR: boom").join("\n");
    const summary = classifyRecoveryDiagnostics(errors, "", { maxErrors: 10 });
    expect(summary.errors).toHaveLength(10);
    expect(summary.truncated).toBe(true);
  });

  it("returns an empty summary for benign output", () => {
    const summary = classifyRecoveryDiagnostics("Godot Engine v4.7\nhello", "");
    expect(summary).toEqual({ errors: [], warnings: [], truncated: false });
  });
});

describe("emptyRecoveryDiagnosticSummary", () => {
  it("is always empty and untruncated", () => {
    expect(emptyRecoveryDiagnosticSummary()).toEqual({
      errors: [],
      warnings: [],
      truncated: false,
    });
  });
});
