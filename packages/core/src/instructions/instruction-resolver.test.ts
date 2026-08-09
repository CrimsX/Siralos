import { describe, expect, it } from "vitest";
import {
  computeResolvedInstructionSetRevision,
  instructionAppliesTo,
} from "./instruction-model.js";
import {
  buildInstruction,
  detectConflicts,
  resolveInstructionSet,
  resolveInstructionsForPath,
} from "./instruction-resolver.js";

const root = () =>
  buildInstruction({
    source: { kind: "project_root", path: null },
    content: "Root guidance: GDScript style.",
  });
const src = () =>
  buildInstruction({
    source: { kind: "project_directory", path: "src" },
    content: "src guidance.",
  });
const srcPlayer = () =>
  buildInstruction({
    source: { kind: "project_directory", path: "src/player" },
    content: "player guidance.",
  });
const srcEnemy = () =>
  buildInstruction({
    source: { kind: "project_directory", path: "src/enemy" },
    content: "enemy guidance.",
  });

describe("instruction scope matching", () => {
  it("applies the root instruction to every workspace path", () => {
    expect(instructionAppliesTo(root(), "src/player/controller.gd")).toBe(true);
    expect(instructionAppliesTo(root(), "tests/player/test_controller.gd")).toBe(true);
  });

  it("applies a directory instruction only under its scope", () => {
    expect(instructionAppliesTo(src(), "src/player/controller.gd")).toBe(true);
    expect(instructionAppliesTo(src(), "src/player")).toBe(true);
    expect(instructionAppliesTo(src(), "src")).toBe(true);
    expect(instructionAppliesTo(src(), "tests/player/test_controller.gd")).toBe(false);
  });

  it("never applies a sibling instruction", () => {
    expect(instructionAppliesTo(srcEnemy(), "src/player/controller.gd")).toBe(false);
    expect(instructionAppliesTo(srcEnemy(), "src/enemy/enemy.gd")).toBe(true);
  });
});

describe("resolveInstructionsForPath", () => {
  it("applies root guidance to a project file", () => {
    const set = resolveInstructionsForPath([root()], "src/player/controller.gd");
    expect(set.instructions.map((instruction) => instruction.id)).toEqual([root().id]);
    expect(set.conflicts).toEqual([]);
  });

  it("applies the hierarchy root -> src -> src/player in deterministic precedence", () => {
    const set = resolveInstructionsForPath(
      [root(), src(), srcPlayer(), srcEnemy()],
      "src/player/controller.gd",
    );
    const ids = set.instructions.map((instruction) => instruction.id);
    expect(ids).toEqual([root().id, src().id, srcPlayer().id]);
    const priorities = set.instructions.map((instruction) => instruction.priority);
    expect(priorities).toEqual([...priorities].sort((a, b) => a - b));
  });

  it("does not include unrelated nested guidance for a sibling file", () => {
    const set = resolveInstructionsForPath(
      [root(), src(), srcPlayer(), srcEnemy()],
      "src/enemy/enemy.gd",
    );
    const ids = set.instructions.map((instruction) => instruction.id);
    expect(ids).toEqual([root().id, src().id, srcEnemy().id]);
  });

  it("computes a deterministic revision that changes when content changes", () => {
    const before = resolveInstructionsForPath([root()], "src/player/controller.gd");
    const changed = resolveInstructionsForPath(
      [buildInstruction({ source: { kind: "project_root", path: null }, content: "Other root." })],
      "src/player/controller.gd",
    );
    expect(before.revision).toBeTruthy();
    expect(changed.revision).not.toBe(before.revision);
  });

  it("changes the resolved revision when the source file revision changes", () => {
    const revA = resolveInstructionsForPath(
      [
        buildInstruction({
          source: { kind: "project_root", path: null },
          content: "Root guidance.",
          sourceRevision: "rev_11111111111111111111111111111111",
        }),
      ],
      ".",
    );
    const revB = resolveInstructionsForPath(
      [
        buildInstruction({
          source: { kind: "project_root", path: null },
          content: "Root guidance.",
          sourceRevision: "rev_22222222222222222222222222222222",
        }),
      ],
      ".",
    );
    expect(revB.revision).not.toBe(revA.revision);
    expect(revA.instructions[0]?.sourceRevision).toBe("rev_11111111111111111111111111111111");
  });
});

describe("multi-path union resolution", () => {
  it("computes the union and preserves scope provenance", () => {
    const set = resolveInstructionSet({
      instructions: [root(), src(), srcPlayer(), srcEnemy()],
      paths: ["src/player/controller.gd", "src/enemy/enemy.gd"],
    });
    const ids = set.instructions.map((instruction) => instruction.id);
    // Same-layer directory scopes order lexically: src/enemy before src/player.
    expect(ids).toEqual([root().id, src().id, srcEnemy().id, srcPlayer().id]);
    for (const instruction of set.instructions) {
      expect(instruction.scope.path).not.toBeNull();
    }
  });

  it("resolves an empty path set to no instructions", () => {
    const set = resolveInstructionSet({ instructions: [root()], paths: [] });
    expect(set.instructions).toEqual([]);
  });
});

describe("conflict handling", () => {
  it("surfaces same-layer same-scope conflicts instead of dropping them", () => {
    const first = buildInstruction({
      source: { kind: "project_directory", path: "src" },
      content: "Use tabs.",
      scopePath: "src",
    });
    const second = buildInstruction({
      source: { kind: "project_directory", path: "src" },
      content: "Use spaces.",
      scopePath: "src",
    });
    const conflicts = detectConflicts([first, second]);
    expect(conflicts).toHaveLength(1);
    expect([...conflicts[0]!.instructionIds].sort()).toEqual([first.id, second.id].sort());
  });

  it("treats identical content at the same layer as one guidance, not a conflict", () => {
    const first = buildInstruction({
      source: { kind: "project_directory", path: "src" },
      content: "Use tabs.",
      scopePath: "src",
    });
    const second = buildInstruction({
      source: { kind: "project_directory", path: "src" },
      content: "Use tabs.",
      scopePath: "src",
    });
    expect(detectConflicts([first, second])).toEqual([]);
  });

  it("does not flag different scopes as conflicts", () => {
    expect(detectConflicts([src(), srcPlayer()])).toEqual([]);
  });
});

describe("revision identity", () => {
  it("is deterministic for the same set", () => {
    const a = resolveInstructionsForPath([root(), src()], "src/player/controller.gd");
    const b = resolveInstructionsForPath([root(), src()], "src/player/controller.gd");
    expect(a.revision).toBe(b.revision);
    expect(computeResolvedInstructionSetRevision(a.instructions)).toBe(a.revision);
  });
});
