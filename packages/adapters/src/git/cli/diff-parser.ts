import { GitError, type GitDiffFileSummary } from "@solaris/core";

export interface ParsedDiff {
  readonly files: readonly GitDiffFileSummary[];
  readonly truncated: boolean;
}

export const MAX_GIT_DIFF_FILES = 1000;

/**
 * Parses `git diff --numstat -z` output into structured file summaries.
 * The NUL-delimited machine-readable form preserves exact paths (spaces,
 * tabs, unicode, newlines, backslashes, quotes) and classifies binary
 * entries (`-\t-`). A record is `<added>\t<removed>\t<path>`; the path is
 * everything after the second tab because `-z` mode does not c-quote paths,
 * so a path may itself contain tabs. Rename/copy pairs appear as
 * `added\tdeleted\t\0<original>\0<path>`; the two paths are separate
 * NUL-delimited records taken whole.
 * Human-oriented quoted diff headers are never parsed as authoritative
 * paths; the unified patch is used only for display.
 */
export function parseNumstatDiff(output: string): ParsedDiff {
  const records = output.split("\0");
  const files: GitDiffFileSummary[] = [];
  let truncated = false;
  let index = 0;
  for (; index < records.length; index += 1) {
    const record = records[index];
    if (record === undefined || record.length === 0) {
      continue;
    }
    const firstTab = record.indexOf("\t");
    const secondTab = firstTab === -1 ? -1 : record.indexOf("\t", firstTab + 1);
    const addedField = firstTab === -1 ? record : record.slice(0, firstTab);
    const removedField = secondTab === -1 ? "" : record.slice(firstTab + 1, secondTab);
    const inlinePath = secondTab === -1 ? "" : record.slice(secondTab + 1);
    const binary = addedField === "-" || removedField === "-";
    let path: string;
    let originalPath: string | null = null;
    let operation: GitDiffFileSummary["operation"];
    let addedLines: number;
    let removedLines: number;
    if (inlinePath.length === 0) {
      // rename/copy pair: added\tdeleted\t\0<original>\0<path>
      originalPath = records[index + 1] ?? "";
      path = records[index + 2] ?? "";
      index += 2;
      if (originalPath.length === 0 || path.length === 0) {
        throw new GitError("git_parse_failed", "Malformed numstat rename record.");
      }
      operation = "rename";
      addedLines = binary ? 0 : parseIntOrZero(addedField);
      removedLines = binary ? 0 : parseIntOrZero(removedField);
    } else {
      path = inlinePath;
      addedLines = binary ? 0 : parseIntOrZero(addedField);
      removedLines = binary ? 0 : parseIntOrZero(removedField);
      if (addedLines > 0 && removedLines === 0) {
        operation = "add";
      } else if (addedLines === 0 && removedLines > 0) {
        operation = "delete";
      } else {
        operation = "modify";
      }
    }
    files.push({
      path,
      originalPath,
      operation,
      addedLines,
      removedLines,
      binary,
    });
    if (files.length >= MAX_GIT_DIFF_FILES) {
      truncated = true;
      break;
    }
  }
  return { files, truncated };
}

function parseIntOrZero(value: string): number {
  const parsed = Number.parseInt(value, 10);
  return Number.isNaN(parsed) ? 0 : parsed;
}
