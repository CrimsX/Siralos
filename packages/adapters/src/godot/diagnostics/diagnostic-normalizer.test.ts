import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { GODOT_LIMITS } from "@solaris/core";
import { normalizeGodotCheckOutput } from "./diagnostic-normalizer.js";

function fixture(name: string): string {
  const fixturePath = path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures", name);
  return readFileSync(fixturePath, "utf8");
}

const MIRROR = path.join(process.platform === "win32" ? "C:\\" : "/", "tmp", "solaris-mirror-1");

describe("normalizeGodotCheckOutput (engine-version fixtures)", () => {
  it("normalizes parser errors with their locations", () => {
    const result = normalizeGodotCheckOutput({
      stdout: fixture("syntax-error.txt"),
      stderr: "",
      mirrorProjectPath: MIRROR,
    });
    expect(result.diagnostics).toHaveLength(1);
    const diagnostic = result.diagnostics[0];
    expect(diagnostic).toMatchObject({
      source: "godot-check-only",
      severity: "error",
      code: "parse-error",
      rawCategory: "error",
      line: null,
      column: null,
    });
    expect(diagnostic?.message).toContain('Expected "end of file"');
    // The Godot source path inside the at-line must never surface.
    expect(JSON.stringify(result.diagnostics)).not.toContain("gdscript.cpp");
    expect(JSON.stringify(result.diagnostics)).not.toContain(MIRROR);
  });

  it("normalizes indentation errors", () => {
    const result = normalizeGodotCheckOutput({
      stdout: fixture("indentation-error.txt"),
      stderr: "",
    });
    expect(result.diagnostics[0]?.message).toContain("Indentation error");
    expect(result.diagnostics[0]?.severity).toBe("error");
  });

  it("normalizes undeclared identifiers with a stable code", () => {
    const result = normalizeGodotCheckOutput({
      stdout: fixture("undeclared-identifier.txt"),
      stderr: "",
    });
    expect(result.diagnostics[0]).toMatchObject({
      severity: "error",
      code: "undeclared-identifier",
    });
    expect(result.diagnostics[0]?.message).toContain('"velocityy"');
  });

  it("normalizes invalid types and missing base classes", () => {
    const invalidType = normalizeGodotCheckOutput({
      stdout: fixture("invalid-type.txt"),
      stderr: "",
    });
    expect(invalidType.diagnostics[0]?.message).toContain('Invalid type "Nope"');
    const missingBase = normalizeGodotCheckOutput({
      stdout: fixture("missing-base-class.txt"),
      stderr: "",
    });
    expect(missingBase.diagnostics[0]?.message).toContain('"MissingBase"');
  });

  it("keeps warnings as warnings and never upgrades them", () => {
    const result = normalizeGodotCheckOutput({
      stdout: fixture("warning.txt"),
      stderr: "",
    });
    expect(result.diagnostics[0]).toMatchObject({
      severity: "warning",
      rawCategory: "script-warning",
    });
  });

  it("normalizes multiple diagnostics in order without discarding any", () => {
    const result = normalizeGodotCheckOutput({
      stdout: fixture("multiple.txt"),
      stderr: "",
    });
    expect(result.diagnostics).toHaveLength(3);
    expect(result.diagnostics.map((entry) => entry.severity)).toEqual([
      "error",
      "error",
      "warning",
    ]);
  });

  it("extracts inline res:// locations with line and column", () => {
    const result = normalizeGodotCheckOutput({
      stdout: fixture("inline-location.txt"),
      stderr: "",
    });
    expect(result.diagnostics).toHaveLength(2);
    expect(result.diagnostics[0]).toMatchObject({
      severity: "unknown",
      path: "src/player/player.gd",
      line: 34,
      column: 17,
    });
    expect(result.diagnostics[1]).toMatchObject({
      severity: "error",
      path: "src/player/player.gd",
      line: 81,
      column: 9,
    });
    expect(result.diagnostics[0]?.message).toContain('"velocityy"');
  });

  it("preserves unmatched error-like lines as generic diagnostics", () => {
    const result = normalizeGodotCheckOutput({
      stdout: fixture("unexpected-output.txt"),
      stderr: "",
    });
    const generic = result.diagnostics.find((entry) => entry.message.includes("weird"));
    expect(generic).toBeDefined();
    expect(generic?.severity).toBe("unknown");
    expect(generic?.rawCategory).toBeNull();
    expect(generic?.line).toBeNull();
    // The at-line attaches no fabricated location when it has no script path.
    expect(generic?.path).toBeNull();
    expect(result.unmatchedLineCount).toBeGreaterThan(0);
  });

  it("sanitizes terminal escape and control characters", () => {
    const result = normalizeGodotCheckOutput({
      stdout: fixture("control-characters.txt"),
      stderr: "",
    });
    const message = result.diagnostics[0]?.message ?? "";
    expect(message).not.toContain("\u001b");
    expect(message).not.toContain("\u0000");
    expect(message).toContain("Control character");
  });

  it("never leaks mirror-absolute paths", () => {
    const mirrorOutput = `Godot Engine v4.7.1.stable.official\nERROR: Parse Error: boom.\n   at: ${MIRROR}/src/player/player.gd:34:17\n`;
    const result = normalizeGodotCheckOutput({
      stdout: mirrorOutput,
      stderr: "",
      mirrorProjectPath: MIRROR,
    });
    expect(JSON.stringify(result.diagnostics)).not.toContain(MIRROR);
    expect(result.diagnostics[0]).toMatchObject({
      path: "src/player/player.gd",
      line: 34,
      column: 17,
    });
  });

  it("bounds diagnostics per script with explicit truncation", () => {
    const many = Array.from(
      { length: GODOT_LIMITS.maxDiagnosticsPerScript + 10 },
      (_, index) => `ERROR: Parse Error: issue ${index}.`,
    ).join("\n");
    const result = normalizeGodotCheckOutput({
      stdout: many,
      stderr: "",
    });
    expect(result.diagnostics).toHaveLength(GODOT_LIMITS.maxDiagnosticsPerScript);
    expect(result.truncated).toBe(true);
  });

  it("bounds and sanitizes individual messages", () => {
    const huge = `ERROR: ${"y".repeat(GODOT_LIMITS.maxDiagnosticMessageBytes + 1024)}`;
    const result = normalizeGodotCheckOutput({ stdout: huge, stderr: "" });
    expect(Buffer.byteLength(result.diagnostics[0]?.message ?? "", "utf8")).toBeLessThanOrEqual(
      GODOT_LIMITS.maxDiagnosticMessageBytes,
    );
  });
});
