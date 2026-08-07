export type FileChangeOperation = "create" | "update" | "delete";

export interface FileChangePreview {
  readonly path: string;
  readonly operation: FileChangeOperation;
  readonly beforeSha256: string | null;
  readonly afterSha256: string | null;
  readonly addedLines: number;
  readonly removedLines: number;
  readonly unifiedDiff: string;
}

export interface ChangePreview {
  readonly files: readonly FileChangePreview[];
  readonly totalAddedLines: number;
  readonly totalRemovedLines: number;
  readonly truncated: boolean;
}
