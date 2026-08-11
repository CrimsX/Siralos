import { describe, expect, it } from "vitest";
import {
  createNewFileRationale,
  detectProliferationSignals,
  evaluateScopeDiff,
  pathMatchesPattern,
} from "./new-file-discipline.js";

describe("new file discipline", () => {
  it("records a bounded rationale naming the existing owners inspected", () => {
    const rationale = createNewFileRationale({
      path: "packages/core/src/godot/scene/scene-parser.ts",
      reason:
        "Godot text-resource grammar is a distinct responsibility from GDScript structure parsing.",
      existingOwnersInspected: ["workspace/workspace-revision.ts", "godot/gdscript-structure.ts"],
    });
    expect(rationale.path).toContain("scene-parser.ts");
    expect(rationale.existingOwnersInspected).toHaveLength(2);
  });

  it("rejects a new file without a recorded reason", () => {
    expect(() =>
      createNewFileRationale({
        path: "src/mystery.ts",
        reason: "",
        existingOwnersInspected: [],
      }),
    ).toThrow(/requires a reason/);
  });

  it("flags a narrow task that produces many new production files", () => {
    const signals = detectProliferationSignals({
      newProductionFiles: Array.from({ length: 7 }, (_, index) => ({
        path: `src/new${index}.ts`,
        sizeBytes: 1024,
      })),
      plannedPaths: ["src/**"],
      knownDirectories: ["src"],
    });
    expect(signals.map((signal) => signal.id)).toContain("PROLIF.MANY_NEW_FILES");
  });

  it("flags tiny one-use helper modules", () => {
    const signals = detectProliferationSignals({
      newProductionFiles: [
        { path: "src/tiny1.ts", sizeBytes: 40 },
        { path: "src/tiny2.ts", sizeBytes: 60 },
        { path: "src/tiny3.ts", sizeBytes: 80 },
      ],
      plannedPaths: ["src/**"],
      knownDirectories: ["src"],
    });
    expect(signals.map((signal) => signal.id)).toContain("PROLIF.TINY_HELPERS");
  });

  it("flags new directories created for a narrow change", () => {
    const signals = detectProliferationSignals({
      newProductionFiles: [{ path: "src/parser/scanner.ts", sizeBytes: 1024 }],
      plannedPaths: ["src/**"],
      knownDirectories: ["src"],
    });
    expect(signals.map((signal) => signal.id)).toContain("PROLIF.NEW_DIRECTORY");
  });

  it("flags files created outside the planned scope", () => {
    const signals = detectProliferationSignals({
      newProductionFiles: [
        { path: "src/one.ts", sizeBytes: 1024 },
        { path: "src/two.ts", sizeBytes: 1024 },
        { path: "src/three.ts", sizeBytes: 1024 },
        { path: "src/four.ts", sizeBytes: 1024 },
      ],
      plannedPaths: ["src/parser/**"],
      knownDirectories: ["src"],
    });
    expect(signals.map((signal) => signal.id)).toContain("PROLIF.OUTSIDE_SCOPE");
  });

  it("treats expansion with recorded rationale as justified, not silent", () => {
    const report = evaluateScopeDiff({
      plannedPaths: ["packages/core/src/executor/context-pack.ts"],
      changedPaths: [
        "packages/core/src/executor/context-pack.ts",
        "packages/core/src/executor/workspace-scope.ts",
        "apps/cli/src/commands/brief-command.ts",
      ],
      rationales: [
        createNewFileRationale({
          path: "packages/core/src/executor/workspace-scope.ts",
          reason: "Workspace scope is a distinct responsibility from briefing compilation.",
          existingOwnersInspected: ["executor/context-pack.ts"],
        }),
      ],
    });
    const byPath = new Map(report.entries.map((entry) => [entry.path, entry.classification]));
    expect(byPath.get("packages/core/src/executor/context-pack.ts")).toBe("expected");
    expect(byPath.get("packages/core/src/executor/workspace-scope.ts")).toBe("justified expansion");
    expect(byPath.get("apps/cli/src/commands/brief-command.ts")).toBe("unexplained expansion");
    expect(report.unexplained).toEqual(["apps/cli/src/commands/brief-command.ts"]);
  });

  it("matches planned glob paths deterministically", () => {
    expect(
      pathMatchesPattern("packages/core/src/godot/scene/parser.ts", "packages/core/src/godot/**"),
    ).toBe(true);
    expect(
      pathMatchesPattern("packages/core/src/executor/a.ts", "packages/core/src/executor/*.ts"),
    ).toBe(true);
    expect(
      pathMatchesPattern("packages/core/src/executor/sub/a.ts", "packages/core/src/executor/*.ts"),
    ).toBe(false);
    expect(pathMatchesPattern("src/a.ts", "src/a.ts")).toBe(true);
    expect(pathMatchesPattern("src/b.ts", "src/a.ts")).toBe(false);
  });

  it("signals are review signals, not hard rules: a justified expansion passes", () => {
    const signals = detectProliferationSignals({
      newProductionFiles: [
        { path: "src/parser/parser.ts", sizeBytes: 4096 },
        { path: "src/parser/parser.test.ts", sizeBytes: 2048 },
      ],
      plannedPaths: ["src/parser/**"],
      knownDirectories: ["src", "src/parser"],
    });
    expect(signals).toHaveLength(0);
  });
});
