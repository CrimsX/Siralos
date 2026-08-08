import { lstat, opendir } from "node:fs/promises";
import path from "node:path";
import type { Tool, ToolExecutionContext, ToolExecutionResult } from "@solaris/core";
import { WORKSPACE_LIMITS } from "./limits.js";
import { foldPathComponent } from "../../fs-case.js";
import { readFileBounded } from "../../fs/file-read.js";
import {
  DEFAULT_EXCLUDED_DIRECTORIES,
  findExcludedComponent,
  resolveWorkspacePath,
} from "./workspace-path.js";
import { decodeUtf8, looksBinary, splitIntoLines } from "./text.js";
import {
  readJsonObject,
  readOptionalPositiveInteger,
  readOptionalString,
  readRequiredString,
  type ParsedValue,
} from "./validation.js";

interface SearchInput {
  readonly query: string;
  readonly path: string;
  readonly maxResults: number;
}

type SearchMatch = {
  readonly path: string;
  readonly line: number;
  readonly column: number;
  readonly text: string;
};

function parseSearchInput(input: unknown): ParsedValue<SearchInput> {
  const object = readJsonObject(input);
  if (!object.ok) {
    return object;
  }
  const parsedQuery = readRequiredString(object.value, "query");
  if (!parsedQuery.ok) {
    return parsedQuery;
  }
  const parsedPath = readOptionalString(object.value, "path");
  if (!parsedPath.ok) {
    return parsedPath;
  }
  const parsedMaxResults = readOptionalPositiveInteger(object.value, "maxResults");
  if (!parsedMaxResults.ok) {
    return parsedMaxResults;
  }
  const requestedMaxResults = parsedMaxResults.value ?? WORKSPACE_LIMITS.maxSearchMatches;
  return {
    ok: true,
    value: {
      query: parsedQuery.value,
      path: parsedPath.value ?? ".",
      maxResults: Math.min(requestedMaxResults, WORKSPACE_LIMITS.maxSearchMatches),
    },
  };
}

export interface SearchBoundsOverrides {
  readonly maxSearchDirectories?: number;
  readonly maxSearchEntries?: number;
  readonly maxSearchFilesConsidered?: number;
  readonly maxSearchFiles?: number;
  readonly maxSearchInputBytes?: number;
  readonly maxSearchOutputBytes?: number;
  readonly maxSearchDurationMs?: number;
  readonly maxSearchFileSizeBytes?: number;
  readonly maxSearchLineLengthChars?: number;
  readonly maxSearchDepth?: number;
}

export type SearchBounds = {
  readonly [Key in keyof typeof WORKSPACE_LIMITS]: number;
} & Required<SearchBoundsOverrides>;

export function createWorkspaceSearchTool(
  workspaceRoot: string,
  overrides: SearchBoundsOverrides = {},
): Tool {
  const bounds: SearchBounds = { ...WORKSPACE_LIMITS, ...overrides };
  return {
    definition: {
      name: "workspace.search",
      description: "Search text files recursively within a bounded workspace directory.",
      inputSchema: {
        type: "object",
        properties: {
          query: {
            type: "string",
            minLength: 1,
            description: "Literal text to search for (case-sensitive).",
          },
          path: {
            type: "string",
            description:
              "Directory path relative to the workspace root. Defaults to the workspace root.",
          },
          maxResults: {
            type: "integer",
            minimum: 1,
            description: "Maximum number of matches to return.",
          },
        },
        required: ["query"],
        additionalProperties: false,
      },
    },
    async execute(input: unknown, context: ToolExecutionContext): Promise<ToolExecutionResult> {
      const parsed = parseSearchInput(input);
      if (!parsed.ok) {
        return { status: "invalid_input", message: parsed.message };
      }
      const resolved = await resolveWorkspacePath(workspaceRoot, parsed.value.path);
      if (resolved.status === "rejected") {
        return { status: "denied", message: resolved.message };
      }
      const excludedComponent = findExcludedComponent(
        resolved.workspaceRelativePath,
        DEFAULT_EXCLUDED_DIRECTORIES,
      );
      if (excludedComponent !== null) {
        return {
          status: "denied",
          message: `Path is inside the excluded directory ${excludedComponent}.`,
        };
      }
      if (context.signal?.aborted) {
        return { status: "cancelled", message: "Search was cancelled." };
      }
      const outcome = await search(
        resolved,
        parsed.value.query,
        parsed.value.maxResults,
        context,
        bounds,
      );
      if (outcome.status === "cancelled") {
        return { status: "cancelled", message: "Search was cancelled." };
      }
      const { matches, scannedFiles, skippedFiles, truncated, truncationReason } = outcome;
      return {
        status: "success",
        output: {
          query: parsed.value.query,
          path: resolved.workspaceRelativePath,
          matches,
          scannedFiles,
          skippedFiles,
          truncated,
          truncationReason,
        },
        summary: `${matches.length} matches${truncated ? " (truncated)" : ""}`,
      };
    },
  };
}

type SearchOutcome =
  | { readonly status: "cancelled" }
  | {
      readonly status: "done";
      readonly matches: readonly SearchMatch[];
      readonly scannedFiles: number;
      readonly skippedFiles: number;
      readonly truncated: boolean;
      readonly truncationReason: string | null;
    };

type TruncationReason =
  | "directory_budget"
  | "entry_budget"
  | "file_budget"
  | "scan_budget"
  | "input_budget"
  | "output_budget"
  | "time_budget"
  | "match_limit"
  | "depth_budget";

async function search(
  resolved: { readonly workspaceRelativePath: string; readonly absolutePath: string },
  query: string,
  maxResults: number,
  context: ToolExecutionContext,
  bounds: SearchBounds,
): Promise<SearchOutcome> {
  const matches: SearchMatch[] = [];
  let scannedFiles = 0;
  let skippedFiles = 0;
  let directoriesVisited = 0;
  let entriesExamined = 0;
  let filesConsidered = 0;
  let inputBytes = 0;
  let outputBytes = 0;
  let truncated = false;
  let truncationReason: TruncationReason | null = null;
  const deadline = Date.now() + bounds.maxSearchDurationMs;
  const stop = (reason: TruncationReason): SearchOutcome => {
    truncated = true;
    truncationReason = reason;
    return {
      status: "done",
      matches: [...matches].sort(compareMatches),
      scannedFiles,
      skippedFiles,
      truncated,
      truncationReason,
    };
  };
  const pendingDirectories: Array<{ absolute: string; relative: string; depth: number }> = [
    { absolute: resolved.absolutePath, relative: resolved.workspaceRelativePath, depth: 0 },
  ];
  while (pendingDirectories.length > 0) {
    if (context.signal?.aborted) {
      return { status: "cancelled" };
    }
    if (Date.now() >= deadline) {
      return stop("time_budget");
    }
    const directory = pendingDirectories.pop();
    if (directory === undefined) {
      break;
    }
    directoriesVisited += 1;
    if (directoriesVisited > bounds.maxSearchDirectories) {
      return stop("directory_budget");
    }
    if (directory.depth > bounds.maxSearchDepth) {
      return stop("depth_budget");
    }
    const names: string[] = [];
    let directoryHandle;
    try {
      directoryHandle = await opendir(directory.absolute);
    } catch {
      continue;
    }
    try {
      for await (const entry of directoryHandle) {
        if (context.signal?.aborted) {
          return { status: "cancelled" };
        }
        entriesExamined += 1;
        if (entriesExamined > bounds.maxSearchEntries) {
          return stop("entry_budget");
        }
        if (
          DEFAULT_EXCLUDED_DIRECTORIES.some(
            (excluded) => foldPathComponent(excluded) === foldPathComponent(entry.name),
          )
        ) {
          continue;
        }
        names.push(entry.name);
      }
    } finally {
      await directoryHandle.close().catch(() => {});
    }
    names.sort();
    for (const name of names) {
      if (context.signal?.aborted) {
        return { status: "cancelled" };
      }
      if (Date.now() >= deadline) {
        return stop("time_budget");
      }
      const absolute = path.join(directory.absolute, name);
      let stats;
      try {
        stats = await lstat(absolute);
      } catch {
        skippedFiles += 1;
        continue;
      }
      if (stats.isSymbolicLink()) {
        skippedFiles += 1;
        continue;
      }
      if (stats.isDirectory()) {
        pendingDirectories.push({
          absolute,
          relative: childRelativePath(directory.relative, name),
          depth: directory.depth + 1,
        });
        continue;
      }
      if (!stats.isFile()) {
        skippedFiles += 1;
        continue;
      }
      filesConsidered += 1;
      if (filesConsidered > bounds.maxSearchFilesConsidered) {
        return stop("file_budget");
      }
      if (stats.size > bounds.maxSearchFileSizeBytes) {
        skippedFiles += 1;
        continue;
      }
      if (scannedFiles >= bounds.maxSearchFiles) {
        return stop("scan_budget");
      }
      scannedFiles += 1;
      if (context.signal?.aborted) {
        return { status: "cancelled" };
      }
      // The read itself is capped: a file grown or swapped after the lstat
      // is read only up to the size bound plus one byte, so a hostile
      // replacement can never drive an unbounded read or block on a FIFO.
      const buffer = await readFileBounded(absolute, bounds.maxSearchFileSizeBytes);
      if (buffer === null) {
        skippedFiles += 1;
        continue;
      }
      inputBytes += buffer.length;
      if (inputBytes > bounds.maxSearchInputBytes) {
        return stop("input_budget");
      }
      if (looksBinary(buffer)) {
        skippedFiles += 1;
        continue;
      }
      const text = decodeUtf8(buffer);
      if (text === null) {
        skippedFiles += 1;
        continue;
      }
      const relativePath = childRelativePath(directory.relative, name);
      const lines = splitIntoLines(text);
      for (let lineIndex = 0; lineIndex < lines.length; lineIndex += 1) {
        const line = lines[lineIndex];
        if (line === undefined) {
          continue;
        }
        if ((lineIndex & 63) === 0) {
          if (context.signal?.aborted) {
            return { status: "cancelled" };
          }
          if (Date.now() >= deadline) {
            return stop("time_budget");
          }
        }
        const column = line.indexOf(query);
        if (column >= 0) {
          const matchText = line.slice(0, bounds.maxSearchLineLengthChars);
          matches.push({
            path: relativePath,
            line: lineIndex + 1,
            column: column + 1,
            text: matchText,
          });
          outputBytes += Buffer.byteLength(matchText, "utf8");
          if (outputBytes > bounds.maxSearchOutputBytes) {
            return stop("output_budget");
          }
          if (matches.length >= maxResults) {
            return stop("match_limit");
          }
        }
      }
    }
  }
  return {
    status: "done",
    matches: [...matches].sort(compareMatches),
    scannedFiles,
    skippedFiles,
    truncated,
    truncationReason,
  };
}
function childRelativePath(directoryPath: string, name: string): string {
  return directoryPath === "." ? name : `${directoryPath}/${name}`;
}

function compareMatches(a: SearchMatch, b: SearchMatch): number {
  if (a.path !== b.path) {
    return a.path < b.path ? -1 : 1;
  }
  if (a.line !== b.line) {
    return a.line - b.line;
  }
  return a.column - b.column;
}
