export interface GitWorkspaceStatus {
  readonly gitAvailable: boolean;
  readonly gitVersion: string | null;
  readonly repositoryState:
    "repository" | "not_repository" | "root_mismatch" | "unavailable" | "failed";
  readonly repositoryRoot: string | null;
  readonly message?: string;
}

export type GitDiffScope = "working" | "staged" | "head";

export type GitStatusKind = "ordinary" | "renamed" | "copied";

export type GitFileStatus =
  "unmodified" | "added" | "modified" | "deleted" | "renamed" | "copied" | "unmerged";

export interface GitBranchStatus {
  readonly head: string;
  readonly oid: string | null;
  readonly upstream: string | null;
  readonly ahead: number | null;
  readonly behind: number | null;
  readonly detached: boolean;
  readonly unborn: boolean;
}

export interface GitChangeEntry {
  readonly path: string;
  readonly originalPath: string | null;
  readonly indexStatus: GitFileStatus;
  readonly worktreeStatus: GitFileStatus;
  readonly kind: GitStatusKind;
}

export interface GitConflictEntry {
  readonly path: string;
  readonly stage1Oid: string | null;
  readonly stage2Oid: string | null;
  readonly stage3Oid: string | null;
}

export interface GitStatusResult {
  readonly repository: boolean;
  readonly branch: GitBranchStatus;
  readonly changes: readonly GitChangeEntry[];
  readonly conflicts: readonly GitConflictEntry[];
  readonly untracked: readonly string[];
  readonly truncated: boolean;
}

export interface GitDiffFileSummary {
  readonly path: string;
  readonly originalPath: string | null;
  readonly operation: "add" | "modify" | "delete" | "rename";
  readonly addedLines: number;
  readonly removedLines: number;
  readonly binary: boolean;
}

export interface GitDiffResult {
  readonly scope: GitDiffScope;
  readonly files: readonly GitDiffFileSummary[];
  readonly patch: string;
  readonly truncated: boolean;
  readonly untrackedExcluded: boolean;
}
