import type { GitDiffFileSummary } from "@solaris/core";

export interface ParsedDiff {
  readonly files: readonly GitDiffFileSummary[];
  readonly truncated: boolean;
}

export const MAX_GIT_DIFF_FILES = 1000;

interface SectionAccumulator {
  path: string;
  originalPath: string | null;
  operation: "add" | "modify" | "delete" | "rename";
  addedLines: number;
  removedLines: number;
  binary: boolean;
}

export function parseUnifiedDiff(patch: string): ParsedDiff {
  const lines = patch.split("\n");
  const files: GitDiffFileSummary[] = [];
  let current: SectionAccumulator | null = null;
  let truncated = false;
  const flush = (): void => {
    if (current === null) {
      return;
    }
    files.push({
      path: current.path,
      originalPath: current.originalPath,
      operation: current.operation,
      addedLines: current.addedLines,
      removedLines: current.removedLines,
      binary: current.binary,
    });
    current = null;
  };
  for (const line of lines) {
    if (line.startsWith("diff --git ")) {
      flush();
      if (files.length >= MAX_GIT_DIFF_FILES) {
        truncated = true;
        break;
      }
      current = {
        path: parseDiffGitPath(line),
        originalPath: null,
        operation: "modify",
        addedLines: 0,
        removedLines: 0,
        binary: false,
      };
      continue;
    }
    if (current === null) {
      continue;
    }
    if (line.startsWith("new file mode ")) {
      current.operation = "add";
    } else if (line.startsWith("deleted file mode ")) {
      current.operation = "delete";
    } else if (line.startsWith("rename from ")) {
      current.originalPath = normalizePath(line.slice("rename from ".length));
    } else if (line.startsWith("rename to ")) {
      current.path = normalizePath(line.slice("rename to ".length));
      current.operation = "rename";
    } else if (line.startsWith("similarity index ") || line.startsWith("copy from ")) {
      // rename/copy metadata
    } else if (line.startsWith("Binary files ") && line.includes(" differ")) {
      current.binary = true;
    } else if (line.startsWith("+") && !line.startsWith("+++")) {
      current.addedLines += 1;
    } else if (line.startsWith("-") && !line.startsWith("---")) {
      current.removedLines += 1;
    }
  }
  flush();
  return { files, truncated };
}

function parseDiffGitPath(header: string): string {
  const rest = header.slice("diff --git ".length);
  const separator = rest.lastIndexOf(" b/");
  if (separator < 0) {
    return rest;
  }
  return normalizePath(rest.slice(separator + 3));
}

function normalizePath(value: string): string {
  return value.split("\\").join("/");
}
