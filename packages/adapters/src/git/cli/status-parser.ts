import {
  GitError,
  type GitBranchStatus,
  type GitChangeEntry,
  type GitConflictEntry,
  type GitFileStatus,
  type GitStatusKind,
  type GitStatusResult,
} from "@siralos/core";

export const MAX_GIT_STATUS_ENTRIES = 10_000;

interface BranchAccumulator {
  head: string;
  oid: string | null;
  upstream: string | null;
  ahead: number | null;
  behind: number | null;
  detached: boolean;
  unborn: boolean;
}

export function parsePorcelainV2(output: string): GitStatusResult {
  const records = output.split("\0");
  const branch: BranchAccumulator = {
    head: "(unknown)",
    oid: null,
    upstream: null,
    ahead: null,
    behind: null,
    detached: false,
    unborn: false,
  };
  const changes: GitChangeEntry[] = [];
  const conflicts: GitConflictEntry[] = [];
  const untracked: string[] = [];
  let truncated = false;
  let index = 0;
  for (; index < records.length; index += 1) {
    const record = records[index];
    if (record === undefined || record.length === 0) {
      continue;
    }
    if (record.startsWith("# ")) {
      parseBranchRecord(record, branch);
      continue;
    }
    if (record.startsWith("? ")) {
      untracked.push(normalizePath(record.slice(2)));
      if (totalEntries(untracked, changes, conflicts) > MAX_GIT_STATUS_ENTRIES) {
        truncated = true;
        break;
      }
      continue;
    }
    if (record.startsWith("1 ") || record.startsWith("2 ")) {
      const entry = parseOrdinaryRecord(record, records, index);
      if (entry !== null) {
        changes.push(entry.change);
        if (entry.consumedExtra) {
          index += 1;
        }
      }
      if (totalEntries(untracked, changes, conflicts) > MAX_GIT_STATUS_ENTRIES) {
        truncated = true;
        break;
      }
      continue;
    }
    if (record.startsWith("u ")) {
      const conflict = parseUnmergedRecord(record);
      if (conflict !== null) {
        conflicts.push(conflict);
      }
      if (totalEntries(untracked, changes, conflicts) > MAX_GIT_STATUS_ENTRIES) {
        truncated = true;
        break;
      }
      continue;
    }
    throw new GitError("git_parse_failed", "Malformed porcelain v2 status record.");
  }
  return { repository: true, branch: { ...branch }, changes, conflicts, untracked, truncated };
}

/**
 * Recovers whatever branch records are intact in a truncated status output.
 * Used when `parsePorcelainV2` cannot complete because truncation cut a
 * record mid-stream; the result defaults to a detached-HEAD-shaped branch
 * when no branch records survive.
 */
export function parseBranchFromTruncatedOutput(output: string): GitBranchStatus {
  const branch: BranchAccumulator = {
    head: "HEAD",
    oid: null,
    upstream: null,
    ahead: null,
    behind: null,
    detached: false,
    unborn: false,
  };
  for (const record of output.split("\0")) {
    if (record.startsWith("# ")) {
      parseBranchRecord(record, branch);
    }
  }
  if (branch.detached) {
    branch.head = "HEAD";
  }
  return { ...branch };
}

function totalEntries(
  untracked: readonly unknown[],
  changes: readonly unknown[],
  conflicts: readonly unknown[],
): number {
  return untracked.length + changes.length + conflicts.length;
}

function parseBranchRecord(record: string, branch: BranchAccumulator): void {
  const parts = record.slice(2).split(" ");
  const kind = parts[0] ?? "";
  switch (kind) {
    case "branch.oid":
      branch.oid = parts[1] === "(initial)" ? null : (parts[1] ?? null);
      branch.unborn = parts[1] === "(initial)";
      break;
    case "branch.head":
      if (parts[1] === "(detached)") {
        branch.detached = true;
      }
      branch.head = parts[1] ?? "(unknown)";
      break;
    case "branch.upstream":
      branch.upstream = parts[1] === "(gone)" ? null : (parts[1] ?? null);
      break;
    case "branch.ab":
      branch.ahead = parseInteger(parts[1]);
      branch.behind = parseInteger(parts[2]);
      break;
    default:
      break;
  }
}

interface OrdinaryRecordResult {
  readonly change: GitChangeEntry;
  readonly consumedExtra: boolean;
}

function parseOrdinaryRecord(
  record: string,
  records: readonly string[],
  index: number,
): OrdinaryRecordResult | null {
  const fieldCount = record.startsWith("2 ") ? 10 : 9;
  const parsed = splitHeaderAndPath(record, fieldCount);
  const xy = parsed.header[1] ?? "";
  const kind: GitStatusKind = record.startsWith("2 ")
    ? xy[0] === "C"
      ? "copied"
      : "renamed"
    : "ordinary";
  let originalPath: string | null = null;
  let consumedExtra = false;
  if (kind !== "ordinary") {
    const next = records[index + 1];
    if (next !== undefined && next.length > 0) {
      originalPath = normalizePath(next);
      consumedExtra = true;
    }
  }
  return {
    change: {
      path: normalizePath(parsed.path),
      originalPath,
      indexStatus: mapIndexStatus(xy[0] ?? "."),
      worktreeStatus: mapWorktreeStatus(xy[1] ?? "."),
      kind,
    },
    consumedExtra,
  };
}

function parseUnmergedRecord(record: string): GitConflictEntry | null {
  const parsed = splitHeaderAndPath(record, 11);
  return {
    path: normalizePath(parsed.path),
    stage1Oid: parsed.header[7] ?? null,
    stage2Oid: parsed.header[8] ?? null,
    stage3Oid: parsed.header[9] ?? null,
  };
}

function splitHeaderAndPath(
  record: string,
  fieldCount: number,
): { header: readonly string[]; path: string } {
  const separators = fieldCount - 1;
  let spaceCount = 0;
  let index = 0;
  while (index < record.length && spaceCount < separators) {
    if (record[index] === " ") {
      spaceCount += 1;
    }
    index += 1;
  }
  return {
    header: record.slice(0, index).split(" "),
    path: record.slice(Math.min(index, record.length)),
  };
}

function mapIndexStatus(code: string): GitFileStatus {
  switch (code) {
    case "A":
      return "added";
    case "M":
      return "modified";
    case "D":
      return "deleted";
    case "R":
      return "renamed";
    case "C":
      return "copied";
    case "U":
      return "unmerged";
    case "T":
      return "modified";
    default:
      return "unmodified";
  }
}

function mapWorktreeStatus(code: string): GitFileStatus {
  switch (code) {
    case "M":
      return "modified";
    case "D":
      return "deleted";
    case "T":
      return "modified";
    default:
      return "unmodified";
  }
}

function parseInteger(value: string | undefined): number | null {
  if (value === undefined) {
    return null;
  }
  const parsed = Number.parseInt(value, 10);
  return Number.isNaN(parsed) ? null : parsed;
}

function normalizePath(value: string): string {
  return value.split("\\").join("/");
}
