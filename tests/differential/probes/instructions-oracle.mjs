import { readFileSync } from "node:fs";
import {
  buildInstruction,
  computeInstructionInventoryRevision,
  detectConflicts,
  resolveInstructionsForPath,
  resolveInstructionSet,
} from "../../../packages/core/src/instructions/instruction-resolver.ts";
import {
  normalizeInstructionContent,
  renderResolvedInstructions,
} from "../../../packages/core/src/instructions/instruction-model.ts";

const root = (content) =>
  buildInstruction({ source: { kind: "project_root", path: null }, content });
const directory = (path, content) =>
  buildInstruction({ source: { kind: "project_directory", path }, content });

function summary(instruction) {
  return {
    id: instruction.id,
    kind: instruction.source.kind,
    scope: instruction.scope.path,
    priority: instruction.priority,
  };
}

function runCase(inputCase) {
  switch (inputCase.name) {
    case "precedence-ordering": {
      const set = resolveInstructionSet({
        instructions: [
          directory("packages/core/src/deep", "deepest guidance"),
          root("root baseline"),
          directory("packages/core", "core guidance"),
        ],
        paths: ["packages/core/src/deep/x.ts"],
      });
      return {
        order: set.instructions.map(summary),
        revisionPrefix: set.revision.slice(0, 12),
      };
    }
    case "scope-applicability": {
      const scoped = directory("packages/core", "core guidance");
      const universal = buildInstruction({
        source: { kind: "task", path: null },
        content: "task-level framing",
        scopePath: null,
      });
      const inside = resolveInstructionsForPath([scoped], "packages/core/src/engine.ts");
      const outside = resolveInstructionsForPath([scoped], "apps/cli/main.ts");
      const normalizedTrailing = resolveInstructionsForPath(
        [directory("packages/core/", "trailing")],
        "./packages/core/deep/file.txt",
      );
      return {
        insideApplies: inside.instructions.length === 1,
        outsideEmpty: outside.instructions.length === 0,
        universalAppliesToBoth:
          resolveInstructionSet({ instructions: [universal], paths: ["a/b.ts", "c/d.ts"] })
            .instructions.length === 1,
        trailingNormalized: normalizedTrailing.instructions.length === 1,
      };
    }
    case "conflict-detection": {
      const conflicting = [
        directory("packages/core", "use tabs"),
        directory("packages/core", "use spaces"),
      ];
      // Distinct sources/ids, same layer+scope, and raw bytes differ only
      // in ways normalization erases: no conflict may be surfaced.
      const agreeingRawDistinct = [
        buildInstruction({
          source: { kind: "project_directory", path: "packages/core" },
          content: "same guidance",
        }),
        buildInstruction({
          source: { kind: "project_directory", path: "elsewhere" },
          scopePath: "packages/core",
          content: "same   guidance ",
        }),
      ];
      const rawBytesDiffer = agreeingRawDistinct[0].content !== agreeingRawDistinct[1].content;
      const conflicts = detectConflicts(
        resolveInstructionSet({ instructions: conflicting, paths: ["packages/core/x.ts"] })
          .instructions,
      );
      const agreeConflicts = detectConflicts(
        resolveInstructionSet({ instructions: agreeingRawDistinct, paths: ["packages/core/x.ts"] })
          .instructions,
      );
      return {
        conflictCount: conflicts.length,
        reason: conflicts[0]?.reason ?? null,
        agreeingConflictCount: agreeConflicts.length,
        rawBytesDiffer,
      };
    }
    case "normalization-identity": {
      const base = "Guidance text.\nSecond line.";
      const variants = [
        root(base),
        root(base.replace(/\n/g, "\r\n")),
        root(`  ${base}\n \t\n\n\n`),
      ];
      const different = root("Different guidance entirely.");
      const normalizedProbe = normalizeInstructionContent("A\r\nB\tC   \n\n\n\nD");
      return {
        sameId: variants.every((instruction) => instruction.id === variants[0].id),
        idFormat: /^instr_[0-9a-f]{24}$/.test(variants[0].id),
        differentId: different.id !== variants[0].id,
        normalizedProbe,
      };
    }
    case "revision-determinism": {
      const instructions = [root("stable"), directory("packages/core", "scoped")];
      const first = resolveInstructionSet({ instructions, paths: ["packages/core/a.ts"] });
      const second = resolveInstructionSet({ instructions, paths: ["packages/core/a.ts"] });
      const withRevision = resolveInstructionSet({
        instructions: [
          buildInstruction({
            source: { kind: "project_root", path: null },
            content: "stable",
            sourceRevision: "rev_abc",
          }),
          ...instructions.slice(1),
        ],
        paths: ["packages/core/a.ts"],
      });
      return {
        stable: first.revision === second.revision,
        revisionChangesOnSourceRevision: withRevision.revision !== first.revision,
      };
    }
    case "rendering-framing": {
      const set = resolveInstructionSet({
        instructions: [
          root("be careful"),
          directory("packages/core", "core conventions"),
          directory("packages/core", "contradicting conventions"),
        ],
        paths: ["packages/core/x.ts"],
      });
      const rendered = renderResolvedInstructions(set);
      return {
        leadsWithAuthorityFraming: rendered.startsWith("Behavior guidance for this task."),
        neverGrantsMentioned: rendered.includes("never grant capabilities"),
        conflictSurfaced: rendered.includes("Conflicting guidance (surfaced, not resolved):"),
        conflictReasonIncluded: rendered.includes("contain different content"),
      };
    }
    case "inventory-revision": {
      const list = [
        root("alpha"),
        directory("packages/core", "beta"),
        directory("apps/cli", "gamma"),
      ];
      const forward = computeInstructionInventoryRevision(list);
      const shuffled = computeInstructionInventoryRevision([list[2], list[0], list[1]]);
      return { orderInsensitive: forward === shuffled };
    }
    default:
      throw new Error(`unknown instructions-resolution fixture case ${inputCase.name}`);
  }
}

const input = JSON.parse(readFileSync(0, "utf8"));
process.stdout.write(JSON.stringify({ cases: input.cases.map(runCase) }));
