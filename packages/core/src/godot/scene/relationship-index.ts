import { GODOT_SCENE_LIMITS } from "./limits.js";
import type { WorkspaceRevisionHandle } from "../../workspace/workspace-revision.js";

/**
 * Read-only Godot relationship index (Stage 3 milestone 8).
 *
 * A small application-owned derived index of known parsed relationships
 * (scene→script, scene→scene, resource→resource, project→main scene,
 * project→autoload). It is NOT a generic graph database and does NOT own
 * source-of-truth file contents: entries are derived from revision-bound
 * parse results, each entry records the source revision it was derived
 * from, and a stale entry (source file changed since) is never presented
 * as current. One application subsystem (the scene intelligence service)
 * owns the current parsed state; no other component builds its own maps.
 */

export type GodotRelationshipKind =
  | "scene_inherits"
  | "scene_instances"
  | "scene_uses_script"
  | "resource_references"
  | "project_main_scene"
  | "project_autoload";

export interface GodotRelationshipEntry {
  /** Workspace-relative path of the source document. */
  readonly sourcePath: string;
  /** Revision the source document was parsed from (stale when the source changed). */
  readonly sourceRevision: WorkspaceRevisionHandle | null;
  readonly kind: GodotRelationshipKind;
  /** Workspace-relative resolved target path. */
  readonly targetPath: string;
  /** Target `uid://` identity when both path and UID are known. */
  readonly targetUid?: string;
}

export interface GodotRelationshipIndex {
  /** Replace all entries previously recorded for `sourcePath` (reparse replaces stale derivations). */
  record(sourcePath: string, entries: readonly GodotRelationshipEntry[]): void;
  /** Immediate outgoing relationships of a source document. */
  dependenciesOf(sourcePath: string): readonly GodotRelationshipEntry[];
  /** Incoming relationships: which parsed documents reference `targetPath`. */
  referrersOf(targetPath: string): readonly GodotRelationshipEntry[];
  /** True when an entry's recorded revision is not the current one for its source. */
  isStale(entry: GodotRelationshipEntry, currentRevision: WorkspaceRevisionHandle | null): boolean;
  readonly size: number;
  clear(): void;
}

export function createGodotRelationshipIndex(
  options: { readonly maxEntries?: number } = {},
): GodotRelationshipIndex {
  const maxEntries = options.maxEntries ?? GODOT_SCENE_LIMITS.maxIndexEntries;
  // sourcePath -> entries (insertion order preserved for FIFO eviction).
  const bySource = new Map<string, GodotRelationshipEntry[]>();
  // targetPath -> entries.
  const byTarget = new Map<string, GodotRelationshipEntry[]>();
  // FIFO order of source paths for bounded eviction.
  const order: string[] = [];
  let total = 0;

  function removeSource(sourcePath: string): void {
    const entries = bySource.get(sourcePath);
    if (entries === undefined) {
      return;
    }
    bySource.delete(sourcePath);
    const orderIndex = order.indexOf(sourcePath);
    if (orderIndex >= 0) {
      order.splice(orderIndex, 1);
    }
    for (const entry of entries) {
      const targets = byTarget.get(entry.targetPath);
      if (targets !== undefined) {
        const remaining = targets.filter((candidate) => candidate.sourcePath !== sourcePath);
        if (remaining.length === 0) {
          byTarget.delete(entry.targetPath);
        } else {
          byTarget.set(entry.targetPath, remaining);
        }
      }
    }
    total -= entries.length;
  }

  function evictIfNeeded(): void {
    while (total > maxEntries) {
      const oldest = order.shift();
      if (oldest === undefined) {
        break;
      }
      removeSource(oldest);
    }
  }

  return {
    record(sourcePath: string, entries: readonly GodotRelationshipEntry[]): void {
      removeSource(sourcePath);
      if (entries.length === 0) {
        return;
      }
      bySource.set(sourcePath, [...entries]);
      order.push(sourcePath);
      for (const entry of entries) {
        const targets = byTarget.get(entry.targetPath) ?? [];
        targets.push(entry);
        byTarget.set(entry.targetPath, targets);
      }
      total += entries.length;
      evictIfNeeded();
    },

    dependenciesOf(sourcePath: string): readonly GodotRelationshipEntry[] {
      return [...(bySource.get(sourcePath) ?? [])];
    },

    referrersOf(targetPath: string): readonly GodotRelationshipEntry[] {
      return [...(byTarget.get(targetPath) ?? [])];
    },

    isStale(
      entry: GodotRelationshipEntry,
      currentRevision: WorkspaceRevisionHandle | null,
    ): boolean {
      return entry.sourceRevision !== null && entry.sourceRevision !== currentRevision;
    },

    get size(): number {
      return total;
    },

    clear(): void {
      bySource.clear();
      byTarget.clear();
      order.length = 0;
      total = 0;
    },
  };
}
