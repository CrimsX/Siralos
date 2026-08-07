export type CheckpointState =
  "prepared" | "applied" | "undone" | "abandoned" | "conflicted" | "uncertain";

export type CheckpointOperation = "create" | "update" | "delete";

export interface CheckpointFileState {
  readonly exists: boolean;
  readonly sha256: string | null;
  readonly byteLength: number | null;
}

export interface FileCheckpoint {
  readonly version: 1;
  readonly id: string;
  readonly workspaceFingerprint: string;
  readonly relativePath: string;
  readonly operation: CheckpointOperation;
  readonly toolName: string;
  readonly createdAt: string;
  readonly state: CheckpointState;
  readonly before: CheckpointFileState;
  readonly after: CheckpointFileState;
  readonly preview: {
    readonly addedLines: number;
    readonly removedLines: number;
  };
}

export interface CheckpointPreimage {
  readonly exists: boolean;
  readonly sha256: string | null;
  readonly byteLength: number | null;
  readonly bytes: Uint8Array | null;
}

export interface PreparedCheckpoint {
  readonly relativePath: string;
  readonly operation: CheckpointOperation;
  readonly toolName: string;
  readonly before: CheckpointPreimage;
  readonly after: CheckpointFileState;
  readonly preview: {
    readonly addedLines: number;
    readonly removedLines: number;
  };
}

export interface AppliedCheckpointResult {
  readonly afterSha256: string | null;
  readonly absent: boolean;
}

export type CheckpointTerminalState = "abandoned" | "conflicted" | "uncertain" | "applied";

export interface CheckpointListQuery {
  readonly limit?: number;
  readonly states?: readonly CheckpointState[];
}
