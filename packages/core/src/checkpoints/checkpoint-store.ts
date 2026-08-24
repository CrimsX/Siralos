import type {
  AppliedCheckpointResult,
  CheckpointListQuery,
  CheckpointTerminalState,
  FileCheckpoint,
  PreparedCheckpoint,
} from "./checkpoint-model.js";

export interface CheckpointStore {
  /** The store's own workspace fingerprint — names the checkpoint namespace directory; single source of truth for direct-inspection callers. */
  readonly fingerprint: string;

  prepare(checkpoint: PreparedCheckpoint, signal?: AbortSignal): Promise<FileCheckpoint>;

  finalizeApplied(checkpointId: string, result: AppliedCheckpointResult): Promise<FileCheckpoint>;

  markUndone(checkpointId: string): Promise<FileCheckpoint>;

  markState(checkpointId: string, state: CheckpointTerminalState): Promise<FileCheckpoint>;

  get(checkpointId: string): Promise<FileCheckpoint | null>;

  list(query?: CheckpointListQuery): Promise<readonly FileCheckpoint[]>;

  loadPreimage(checkpointId: string): Promise<Uint8Array | null>;
}
