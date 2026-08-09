import { describe, expect, it } from "vitest";
import type { GodotGDScriptDiagnostic } from "../gdscript.js";
import {
  computeWarningDelta,
  diagnosticIdentityKey,
  normalizeDiagnosticMessage,
} from "./quality-warnings.js";

function warning(
  path: string,
  line: number,
  message: string,
  code: string | null = null,
): GodotGDScriptDiagnostic {
  return {
    source: "godot-lsp",
    severity: "warning",
    path,
    line,
    column: null,
    code,
    message,
    rawCategory: null,
  };
}

function error(path: string, line: number, message: string): GodotGDScriptDiagnostic {
  return {
    source: "godot-lsp",
    severity: "error",
    path,
    line,
    column: null,
    code: null,
    message,
    rawCategory: null,
  };
}

const CHANGED_PATH = "scripts/player/player.gd";
const CHANGED = [CHANGED_PATH];

describe("warning identity", () => {
  it("normalizes embedded integers so identities do not depend on line numbers", () => {
    expect(normalizeDiagnosticMessage("value at line 42 is unused")).toBe(
      "value at line # is unused",
    );
    expect(diagnosticIdentityKey({ path: "a.gd", code: null, message: "x at line 5" })).toBe(
      diagnosticIdentityKey({ path: "a.gd", code: null, message: "x at line 9" }),
    );
  });

  it("does not depend on message ordering", () => {
    const left = warning("a.gd", 1, "first message", "W001");
    const right = warning("a.gd", 2, "first message", "W001");
    expect(diagnosticIdentityKey(left)).toBe(diagnosticIdentityKey(right));
  });
});

describe("warning delta", () => {
  it("classifies a newly introduced warning as introduced", () => {
    const delta = computeWarningDelta([], [warning(CHANGED_PATH, 4, "unused variable")], CHANGED);
    expect(delta.introducedWarnings).toBe(1);
    expect(delta.introducedErrors).toBe(0);
    expect(delta.entries[0]?.classification).toBe("introduced");
    expect(delta.baselineAvailable).toBe(false);
  });

  it("classifies a newly introduced error as introduced (blocking by the LSP gate, not the warning gate)", () => {
    const delta = computeWarningDelta([], [error(CHANGED_PATH, 4, "parse error")], CHANGED);
    expect(delta.introducedErrors).toBe(1);
  });

  it("keeps a warning unchanged when it moves within the tolerance", () => {
    const baseline = [warning(CHANGED_PATH, 4, "unused variable")];
    const after = [warning(CHANGED_PATH, 6, "unused variable")];
    const delta = computeWarningDelta(baseline, after, CHANGED);
    expect(delta.unchangedWarnings).toBe(1);
    expect(delta.introducedWarnings).toBe(0);
    expect(delta.resolvedWarnings).toBe(0);
  });

  it("labels a warning uncertain when it moved beyond the tolerance", () => {
    const baseline = [warning(CHANGED_PATH, 4, "unused variable")];
    const after = [warning(CHANGED_PATH, 80, "unused variable")];
    const delta = computeWarningDelta(baseline, after, CHANGED, { lineTolerance: 30 });
    expect(delta.uncertainWarnings).toBe(1);
    expect(delta.entries[0]?.classification).toBe("uncertain");
  });

  it("classifies a warning that disappeared as resolved", () => {
    const baseline = [warning(CHANGED_PATH, 4, "unused variable")];
    const delta = computeWarningDelta(baseline, [], CHANGED);
    expect(delta.resolvedWarnings).toBe(1);
  });

  it("does not attribute pre-existing warnings outside the changed files", () => {
    const delta = computeWarningDelta(
      [warning("scripts/other.gd", 4, "pre-existing")],
      [warning("scripts/other.gd", 4, "pre-existing")],
      CHANGED,
    );
    expect(delta.entries).toHaveLength(0);
  });

  it("does not attribute pre-existing warnings to the change when they are unchanged", () => {
    const delta = computeWarningDelta(
      [warning(CHANGED_PATH, 4, "pre-existing")],
      [warning(CHANGED_PATH, 4, "pre-existing")],
      CHANGED,
    );
    expect(delta.unchangedWarnings).toBe(1);
    expect(delta.introducedWarnings).toBe(0);
  });

  it("matches multiple same-identity diagnostics deterministically by nearest line", () => {
    const baseline = [
      warning(CHANGED_PATH, 4, "shadowed variable"),
      warning(CHANGED_PATH, 20, "shadowed variable"),
    ];
    const after = [
      warning(CHANGED_PATH, 5, "shadowed variable"),
      warning(CHANGED_PATH, 22, "shadowed variable"),
    ];
    const delta = computeWarningDelta(baseline, after, CHANGED);
    expect(delta.unchangedWarnings).toBe(2);
    expect(delta.introducedWarnings).toBe(0);
  });

  it("reports unmatched baseline instances of a surviving identity as resolved (never dropped)", () => {
    const baseline = [
      warning(CHANGED_PATH, 4, "shadowed variable"),
      warning(CHANGED_PATH, 100, "shadowed variable"),
    ];
    const after = [warning(CHANGED_PATH, 5, "shadowed variable")];
    const delta = computeWarningDelta(baseline, after, CHANGED);
    expect(delta.unchangedWarnings).toBe(1);
    expect(delta.resolvedWarnings).toBe(1);
    const resolved = delta.entries.find(
      (entry) => entry.classification === "resolved" && entry.line === 100,
    );
    expect(resolved).toBeDefined();
  });

  it("binds the delta to the immutable entry bound", () => {
    const baseline = Array.from({ length: 400 }, (_, index) =>
      warning(CHANGED_PATH, index + 1, `w${index}`),
    );
    const after: GodotGDScriptDiagnostic[] = [];
    const delta = computeWarningDelta(baseline, after, CHANGED);
    expect(delta.entries.length).toBeLessThanOrEqual(200);
  });

  it("reports baseline availability truthfully", () => {
    const withBaseline = computeWarningDelta(
      [warning(CHANGED_PATH, 1, "x")],
      [warning(CHANGED_PATH, 1, "x")],
      CHANGED,
      { baselineAvailable: true },
    );
    expect(withBaseline.baselineAvailable).toBe(true);
    const unavailable = computeWarningDelta([], [], CHANGED, { baselineAvailable: false });
    expect(unavailable.baselineAvailable).toBe(false);
  });
});
