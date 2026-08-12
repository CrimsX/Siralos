import type { GitStatusResult } from "@siralos/core";
import { scanAuthoredFiles, type AuthoredFileManifest } from "./authored-files.js";

export interface WorkspaceIntegritySnapshot {
  /** Canonical summary of the Git status when a Git inspector is available. */
  readonly gitStatus: string | null;
  readonly authored: AuthoredFileManifest;
}

export interface WorkspaceIntegrityComparison {
  readonly unchanged: boolean;
  /** True when the baseline itself was truncated by its bounds. */
  readonly bounded: boolean;
}

/**
 * Canonical, deterministic summary of the Git status: branch identity plus
 * the sorted change/conflict/untracked entry set. Equal canonical strings
 * mean the repository view did not change.
 */
export function canonicalizeGitStatus(status: GitStatusResult): string {
  const changes = status.changes
    .map(
      (change) =>
        `${change.path}|${change.originalPath ?? ""}|${change.indexStatus}|${change.worktreeStatus}`,
    )
    .sort();
  const conflicts = status.conflicts
    .map(
      (conflict) =>
        `${conflict.path}|${conflict.stage1Oid ?? ""}|${conflict.stage2Oid ?? ""}|${conflict.stage3Oid ?? ""}`,
    )
    .sort();
  const untracked = [...status.untracked].sort();
  return JSON.stringify({
    branch: status.branch.detached
      ? `detached:${status.branch.oid ?? "unknown"}`
      : status.branch.head,
    changes,
    conflicts,
    untracked,
  });
}

export function compareWorkspaceIntegrity(
  before: WorkspaceIntegritySnapshot,
  after: WorkspaceIntegritySnapshot,
): WorkspaceIntegrityComparison {
  const bounded = before.authored.truncated || after.authored.truncated;
  const authoredUnchanged =
    before.authored.fileCount === after.authored.fileCount &&
    before.authored.totalBytes === after.authored.totalBytes &&
    before.authored.digest === after.authored.digest;
  const gitUnchanged =
    (before.gitStatus ?? null) === (after.gitStatus ?? null) || before.gitStatus === null;
  return { unchanged: authoredUnchanged && gitUnchanged, bounded };
}

export async function snapshotWorkspaceIntegrity(options: {
  readonly workspaceRoot: string;
  readonly git?: {
    readonly getStatus: (request: { readonly signal?: AbortSignal }) => Promise<GitStatusResult>;
  };
  readonly signal?: AbortSignal;
}): Promise<WorkspaceIntegritySnapshot> {
  const authored = await scanAuthoredFiles({
    workspaceRoot: options.workspaceRoot,
    ...(options.signal === undefined ? {} : { signal: options.signal }),
  });
  let gitStatus: string | null = null;
  if (options.git !== undefined) {
    try {
      const status = await options.git.getStatus({
        ...(options.signal === undefined ? {} : { signal: options.signal }),
      });
      gitStatus = canonicalizeGitStatus(status);
    } catch {
      gitStatus = null;
    }
  }
  return { gitStatus, authored };
}
