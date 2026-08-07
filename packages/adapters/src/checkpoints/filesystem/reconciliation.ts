import { createHash } from "node:crypto";
import { lstat, readFile } from "node:fs/promises";
import { join } from "node:path";
import type { CheckpointStore, FileCheckpoint, WorkspaceFileState } from "@solaris/core";
import { validateRelativeWorkspacePath } from "../../tools/workspace/mutations/mutation-paths.js";

export interface ReconciliationReport {
  checked: number;
  abandoned: number;
  applied: number;
  uncertain: number;
}

export interface ReconciliationOptions {
  readonly workspaceRoot: string;
  readonly store: CheckpointStore;
  readonly maxStateBytes?: number;
}

const DEFAULT_MAX_STATE_BYTES = 1024 * 1024;

export async function reconcileWorkspaceCheckpoints(
  options: ReconciliationOptions,
): Promise<ReconciliationReport> {
  const report: ReconciliationReport = { checked: 0, abandoned: 0, applied: 0, uncertain: 0 };
  const pending = await options.store.list({ states: ["prepared"] });
  for (const checkpoint of pending) {
    const current = await readWorkspaceFileState(
      options.workspaceRoot,
      checkpoint,
      options.maxStateBytes ?? DEFAULT_MAX_STATE_BYTES,
    );
    const beforeState: WorkspaceFileState = {
      exists: checkpoint.before.exists,
      sha256: checkpoint.before.sha256,
    };
    const afterState: WorkspaceFileState = {
      exists: checkpoint.after.exists,
      sha256: checkpoint.after.sha256,
    };
    let nextState: "abandoned" | "applied" | "uncertain";
    if (statesEqual(current, beforeState)) {
      nextState = "abandoned";
    } else if (statesEqual(current, afterState)) {
      nextState = "applied";
    } else {
      nextState = "uncertain";
    }
    report.checked += 1;
    if (nextState === "abandoned") {
      report.abandoned += 1;
    } else if (nextState === "applied") {
      report.applied += 1;
    } else {
      report.uncertain += 1;
    }
    try {
      await options.store.markState(checkpoint.id, nextState);
    } catch {
      // leave the checkpoint prepared if the transition fails; it stays visible
    }
  }
  return report;
}

async function readWorkspaceFileState(
  workspaceRoot: string,
  checkpoint: FileCheckpoint,
  maxStateBytes: number,
): Promise<WorkspaceFileState> {
  const validation = validateRelativeWorkspacePath(checkpoint.relativePath);
  if (validation !== null) {
    return { exists: true, sha256: null };
  }
  const absolute = join(workspaceRoot, ...checkpoint.relativePath.split("/"));
  let stats;
  try {
    stats = await lstat(absolute);
  } catch {
    return { exists: false, sha256: null };
  }
  if (stats.isSymbolicLink() || !stats.isFile()) {
    return { exists: true, sha256: null };
  }
  if (stats.size > maxStateBytes) {
    return { exists: true, sha256: null };
  }
  let bytes: Buffer;
  try {
    bytes = await readFile(absolute);
  } catch {
    return { exists: true, sha256: null };
  }
  return { exists: true, sha256: createHash("sha256").update(bytes).digest("hex") };
}

function statesEqual(a: WorkspaceFileState, b: WorkspaceFileState): boolean {
  if (a.exists !== b.exists) {
    return false;
  }
  if (!a.exists) {
    return true;
  }
  return a.sha256 !== null && a.sha256 === b.sha256;
}
