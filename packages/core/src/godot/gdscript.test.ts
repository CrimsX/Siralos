import { describe, expect, it } from "vitest";
import {
  aggregateGDScriptDiagnostics,
  computeGodotPreparedCheckDigest,
  type GodotGDScriptDiagnostic,
  type GodotPreparedCheckDigestParts,
} from "../index.js";

function diagnostic(partial: Partial<GodotGDScriptDiagnostic> = {}): GodotGDScriptDiagnostic {
  return {
    source: "godot-check-only",
    severity: "error",
    path: "src/player/player.gd",
    line: 34,
    column: 17,
    code: null,
    message: 'Identifier "velocityy" not declared in the current scope.',
    rawCategory: null,
    ...partial,
  };
}

describe("aggregateGDScriptDiagnostics", () => {
  it("collapses exact duplicates without double-counting", () => {
    const first = diagnostic();
    const second = diagnostic();
    const result = aggregateGDScriptDiagnostics([first, second, diagnostic({ message: "other" })]);
    expect(result.diagnostics).toHaveLength(2);
    expect(result.truncated).toBe(false);
  });

  it("keeps distinct diagnostics with different locations or messages", () => {
    const result = aggregateGDScriptDiagnostics([
      diagnostic(),
      diagnostic({ line: 35 }),
      diagnostic({ message: "Different message." }),
      diagnostic({ path: "src/ui/menu.gd" }),
    ]);
    expect(result.diagnostics).toHaveLength(4);
  });

  it("sorts deterministically by path, line, column, then message", () => {
    const result = aggregateGDScriptDiagnostics([
      diagnostic({ path: "src/ui/menu.gd", line: 9 }),
      diagnostic({ path: "src/player/player.gd", line: 40, column: 2 }),
      diagnostic({ path: "src/player/player.gd", line: 34, column: 2 }),
      diagnostic({ path: "src/player/player.gd", line: 34, column: 1 }),
      diagnostic({ path: null, message: "orphan" }),
    ]);
    expect(result.diagnostics.map((entry) => entry.path ?? "∅")).toEqual([
      "∅",
      "src/player/player.gd",
      "src/player/player.gd",
      "src/player/player.gd",
      "src/ui/menu.gd",
    ]);
    expect(result.diagnostics[1]?.column).toBe(1);
    expect(result.diagnostics[2]?.column).toBe(2);
  });

  it("applies the run-wide bound with explicit truncation", () => {
    const many = Array.from({ length: 12 }, (_, index) => diagnostic({ line: index + 1 }));
    const result = aggregateGDScriptDiagnostics(many, 10);
    expect(result.diagnostics).toHaveLength(10);
    expect(result.truncated).toBe(true);
  });

  it("never fabricates line or column values for unknown positions", () => {
    const result = aggregateGDScriptDiagnostics([diagnostic({ line: null, column: null })]);
    expect(result.diagnostics[0]?.line).toBeNull();
    expect(result.diagnostics[0]?.column).toBeNull();
  });
});

describe("computeGodotPreparedCheckDigest", () => {
  function parts(): GodotPreparedCheckDigestParts {
    return {
      scriptTargets: [
        { path: "src/player/player.gd", sha256: "a".repeat(64), bytes: 120 },
        { path: "src/ui/menu.gd", sha256: "b".repeat(64), bytes: 240 },
      ],
      manifestDigest: "c".repeat(64),
      commandDigest: "d".repeat(64),
      sandboxProfileId: "godot-diagnostics-offline",
      checkLimits: {
        timeoutMs: 30_000,
        maxScripts: 10_000,
        maxTotalBytes: 256 * 1024 * 1024,
        maxDiagnosticsPerScript: 500,
        maxDiagnosticsPerRun: 10_000,
      },
    };
  }

  it("produces a deterministic 64-character hex digest", () => {
    const digest = computeGodotPreparedCheckDigest(parts());
    expect(digest).toMatch(/^[0-9a-f]{64}$/);
    expect(computeGodotPreparedCheckDigest(parts())).toBe(digest);
  });

  it("binds every security-relevant field", () => {
    const base = parts();
    const digest = computeGodotPreparedCheckDigest(base);
    const variants: GodotPreparedCheckDigestParts[] = [
      {
        ...base,
        scriptTargets: [...base.scriptTargets, { path: "x.gd", sha256: "e".repeat(64), bytes: 1 }],
      },
      {
        ...base,
        scriptTargets: [
          {
            ...(base.scriptTargets[0] as { path: string; sha256: string; bytes: number }),
            sha256: "f".repeat(64),
          },
        ],
      },
      { ...base, manifestDigest: "g".repeat(64) },
      { ...base, commandDigest: "h".repeat(64) },
      { ...base, sandboxProfileId: "validation-offline" },
      { ...base, checkLimits: { ...base.checkLimits, maxScripts: 1 } },
    ];
    for (const variant of variants) {
      expect(computeGodotPreparedCheckDigest(variant)).not.toBe(digest);
    }
  });

  it("binds the exact ordered target list (the service sorts before digesting)", () => {
    const base = parts();
    const reversed = {
      ...base,
      scriptTargets: [base.scriptTargets[1] as never, base.scriptTargets[0] as never],
    };
    expect(computeGodotPreparedCheckDigest(reversed)).not.toBe(
      computeGodotPreparedCheckDigest(base),
    );
  });
});
