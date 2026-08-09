import {
  computeResolvedInstructionSetRevision,
  createProjectInstruction,
  instructionAppliesTo,
  normalizeInstructionContent,
  type InstructionConflict,
  type InstructionSource,
  type ProjectInstruction,
  type ResolvedInstructionSet,
} from "./instruction-model.js";

/**
 * Single instruction resolver (Stage 3 milestone 4).
 *
 * One subsystem owns instruction-resolution semantics: the CLI,
 * ContextProjector, provider adapters, and /develop consume resolved sets
 * and never re-resolve. Resolution is pure and deterministic — no
 * filesystem, no provider, no network.
 */

export interface ResolveInstructionsInput {
  readonly instructions: readonly ProjectInstruction[];
  /** Workspace-relative paths the current task affects. */
  readonly paths: readonly string[];
}

/**
 * Resolve the applicable instructions for one or more workspace-relative
 * paths. For multiple paths the union is computed and scope is preserved;
 * deterministic conflicts between same-layer instructions are surfaced.
 */
export function resolveInstructionSet(input: ResolveInstructionsInput): ResolvedInstructionSet {
  const applicable = new Map<string, ProjectInstruction>();
  for (const path of input.paths) {
    for (const instruction of input.instructions) {
      if (instructionAppliesTo(instruction, path)) {
        applicable.set(instruction.id, instruction);
      }
    }
  }
  const instructions = [...applicable.values()].sort(compareInstructions);
  const conflicts = detectConflicts(instructions);
  return {
    instructions,
    conflicts,
    revision: computeResolvedInstructionSetRevision(instructions),
  };
}

export function resolveInstructionsForPath(
  instructions: readonly ProjectInstruction[],
  workspaceRelativePath: string,
): ResolvedInstructionSet {
  return resolveInstructionSet({ instructions, paths: [workspaceRelativePath] });
}

/** Deterministic ordering: most authoritative first, then scope depth, then id. */
export function compareInstructions(a: ProjectInstruction, b: ProjectInstruction): number {
  if (a.priority !== b.priority) {
    return a.priority - b.priority;
  }
  const aScope = a.scope.path ?? "";
  const bScope = b.scope.path ?? "";
  const aDepth = aScope === "" ? 0 : aScope.split("/").length;
  const bDepth = bScope === "" ? 0 : bScope.split("/").length;
  if (aDepth !== bDepth) {
    return bDepth - aDepth;
  }
  if (aScope !== bScope) {
    return aScope < bScope ? -1 : 1;
  }
  return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
}

/**
 * Simple structural conflict detection. Instructions at the same
 * precedence layer with the same scope but different content are
 * contradictory; both are preserved and surfaced. Semantic contradiction
 * across layers is intentionally NOT AI-classified in this milestone.
 */
export function detectConflicts(
  instructions: readonly ProjectInstruction[],
): readonly InstructionConflict[] {
  const conflicts: InstructionConflict[] = [];
  const byLayerScope = new Map<string, ProjectInstruction[]>();
  for (const instruction of instructions) {
    const key = `${instruction.priority}|${instruction.scope.path ?? ""}`;
    const bucket = byLayerScope.get(key) ?? [];
    bucket.push(instruction);
    byLayerScope.set(key, bucket);
  }
  for (const [key, bucket] of byLayerScope) {
    if (bucket.length < 2) {
      continue;
    }
    const [first] = bucket;
    if (first === undefined) {
      continue;
    }
    const distinct = new Set(
      bucket.map((instruction) => normalizeInstructionContent(instruction.content)),
    );
    if (distinct.size > 1) {
      conflicts.push({
        instructionIds: bucket.map((instruction) => instruction.id).sort(),
        reason: `Instructions at the same precedence and scope (${key.split("|")[1] ?? "."}) contain different content`,
      });
    }
  }
  return conflicts;
}

/** Convenience constructor used by discovery and tests. */
export function buildInstruction(input: {
  readonly source: InstructionSource;
  readonly content: string;
  readonly scopePath?: string | null;
  readonly sourceRevision?: string | null;
}): ProjectInstruction {
  const derivedScope =
    input.scopePath === undefined
      ? input.source.kind === "project_root"
        ? "."
        : input.source.kind === "project_directory"
          ? (input.source.path ?? ".")
          : null
      : input.scopePath;
  return createProjectInstruction({
    source: input.source,
    scope: { path: derivedScope },
    content: input.content,
    sourceRevision: input.sourceRevision ?? null,
  });
}

/** Deterministic digest over every discovered instruction (task-snapshot identity). */
export function computeInstructionInventoryRevision(
  instructions: readonly ProjectInstruction[],
): string {
  return computeResolvedInstructionSetRevision([...instructions].sort(compareInstructions));
}
