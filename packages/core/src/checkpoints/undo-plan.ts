import type { FileCheckpoint } from "./checkpoint-model.js";

export interface WorkspaceFileState {
  readonly exists: boolean;
  readonly sha256: string | null;
}

export type UndoPlanDecision =
  | {
      readonly decision: "ready";
      readonly action: "create" | "restore" | "delete";
    }
  | {
      readonly decision: "conflict";
      readonly reason: string;
    };

export function planUndo(
  checkpoint: FileCheckpoint,
  current: WorkspaceFileState,
): UndoPlanDecision {
  const after = checkpoint.after;
  if (after.exists) {
    if (current.exists && current.sha256 === after.sha256) {
      return {
        decision: "ready",
        action: checkpoint.before.exists ? "restore" : "delete",
      };
    }
    return {
      decision: "conflict",
      reason:
        "The file changed after Solaris's operation; undo would overwrite newer work. Reread the file.",
    };
  }
  if (!current.exists) {
    return { decision: "ready", action: "create" };
  }
  return {
    decision: "conflict",
    reason:
      "A file appeared where Solaris deleted one; undo would overwrite newer work. Reread the file.",
  };
}
