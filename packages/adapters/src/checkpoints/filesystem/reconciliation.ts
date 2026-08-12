import type { CheckpointStore, WorkspaceFileState } from "@siralos/core";
import { readWorkspaceFileState } from "./checkpoint-file-state.js";

export interface ReconciliationReport {
  checked: number;
  abandoned: number;
  applied: number;
  uncertain: number;
  undoneAfterRestore: number;
}

export interface ReconciliationOptions {
  readonly workspaceRoot: string;
  readonly store: CheckpointStore;
  readonly maxStateBytes?: number;
}

export async function reconcileWorkspaceCheckpoints(
  options: ReconciliationOptions,
): Promise<ReconciliationReport> {
  const report: ReconciliationReport = {
    checked: 0,
    abandoned: 0,
    applied: 0,
    uncertain: 0,
    undoneAfterRestore: 0,
  };
  const pending = await options.store.list({ states: ["prepared", "applied"] });
  for (const checkpoint of pending) {
    const current = await readWorkspaceFileState(
      options.workspaceRoot,
      checkpoint.relativePath,
      options.maxStateBytes,
    );
    const beforeState: WorkspaceFileState = {
      exists: checkpoint.before.exists,
      sha256: checkpoint.before.sha256,
    };
    const afterState: WorkspaceFileState = {
      exists: checkpoint.after.exists,
      sha256: checkpoint.after.sha256,
    };
    if (checkpoint.state === "prepared") {
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
      continue;
    }
    // Applied checkpoints: a crash between the destructive undo commit and
    // markUndone leaves the file at the before-state while the metadata
    // still says applied. Classify that recoverable state as undone; any
    // other divergence is left alone (undo will conflict safely).
    if (statesEqual(current, beforeState) && !statesEqual(current, afterState)) {
      report.checked += 1;
      report.undoneAfterRestore += 1;
      try {
        await options.store.markUndone(checkpoint.id);
      } catch {
        // leave applied if the transition fails; undo still conflicts safely
      }
    }
  }
  return report;
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
