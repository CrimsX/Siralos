import { deepFreeze } from "../domain/deep-freeze.js";

/**
 * Architecture context index (executor briefing foundation).
 *
 * The ExecutorContextPack must include only RELEVANT architecture
 * material, but selection is deterministic — a small explicit tag index
 * instead of semantic/AI classification. Each entry maps a stable
 * concern tag to repository architecture documents; a milestone manifest
 * declares the concerns it exercises, and the context pack builder
 * selects the matching entries in canonical order, bounded.
 */

export interface ArchitectureContextEntry {
  readonly id: string;
  /** Repository-relative doc path (for example docs/adr/0021-...md). */
  readonly path: string;
  /** Deterministic concern tags this entry covers. */
  readonly concerns: readonly string[];
}

/** Bounded reference to one selected architecture document. */
export interface ArchitectureContextRef {
  readonly id: string;
  readonly path: string;
}

export const ARCHITECTURE_INDEX: readonly ArchitectureContextEntry[] = deepFreeze([
  {
    id: "adr:0002",
    path: "docs/adr/0002-provider-neutral-tool-loop.md",
    concerns: ["provider", "tool-loop", "projection"],
  },
  {
    id: "adr:0004",
    path: "docs/adr/0004-sandbox-and-permission-boundary.md",
    concerns: ["security", "sandbox", "capability"],
  },
  {
    id: "adr:0014",
    path: "docs/adr/0014-task-runtime-foundation.md",
    concerns: ["task-runtime", "evidence"],
  },
  {
    id: "adr:0015",
    path: "docs/adr/0015-context-tool-evidence-projection.md",
    concerns: ["projection", "context", "evidence"],
  },
  {
    id: "adr:0016",
    path: "docs/adr/0016-workspace-revision-and-structural-reads.md",
    concerns: ["workspace-revision", "workspace", "evidence"],
  },
  {
    id: "adr:0017",
    path: "docs/adr/0017-project-instructions-and-knowledge.md",
    concerns: ["instructions", "knowledge"],
  },
  {
    id: "adr:0018",
    path: "docs/adr/0018-external-references-and-research-sources.md",
    concerns: ["references", "research"],
  },
  {
    id: "adr:0019",
    path: "docs/adr/0019-self-reference-and-capability-diagnostics.md",
    concerns: ["capability", "self-reference", "doctor"],
  },
  {
    id: "adr:0020",
    path: "docs/adr/0020-host-controlled-planning-foundation.md",
    concerns: ["planning"],
  },
  {
    id: "adr:0021",
    path: "docs/adr/0021-read-only-godot-scene-resource-intelligence.md",
    concerns: ["godot-static-inspection", "godot", "read-only"],
  },
  {
    id: "arch:readme",
    path: "README.md",
    concerns: ["status"],
  },
]);

export interface SelectArchitectureContextInput {
  readonly concerns: readonly string[];
  /** Deterministic canonical order override (defaults to the index order). */
  readonly index?: readonly ArchitectureContextEntry[];
  /** Maximum number of selected entries. */
  readonly maxEntries?: number;
}

/**
 * Deterministic selection: an entry is selected when at least one of its
 * concern tags appears in the requested concerns; results preserve the
 * index order; unrelated material is omitted. Bounded by `maxEntries`.
 */
export function selectArchitectureContext(
  input: SelectArchitectureContextInput,
): readonly ArchitectureContextRef[] {
  const index = input.index ?? ARCHITECTURE_INDEX;
  const wanted = new Set(input.concerns);
  const maxEntries = input.maxEntries ?? 4;
  const selected: ArchitectureContextRef[] = [];
  for (const entry of index) {
    if (selected.length >= maxEntries) {
      break;
    }
    if (entry.concerns.some((concern) => wanted.has(concern))) {
      selected.push({ id: entry.id, path: entry.path });
    }
  }
  return selected;
}
