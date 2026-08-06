import { lstat, readdir, readFile } from "node:fs/promises";
import path from "node:path";
import type { Tool, ToolExecutionContext, ToolExecutionResult } from "@solaris/core";
import { WORKSPACE_LIMITS } from "./limits.js";
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

export function createWorkspaceSearchTool(workspaceRoot: string): Tool {
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
      const outcome = await search(resolved, parsed.value.query, parsed.value.maxResults, context);
      if (outcome.status === "cancelled") {
        return { status: "cancelled", message: "Search was cancelled." };
      }
      const { matches, scannedFiles, skippedFiles, truncated } = outcome;
      return {
        status: "success",
        output: {
          query: parsed.value.query,
          path: resolved.workspaceRelativePath,
          matches,
          scannedFiles,
          skippedFiles,
          truncated,
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
    };

async function search(
  resolved: { readonly workspaceRelativePath: string; readonly absolutePath: string },
  query: string,
  maxResults: number,
  context: ToolExecutionContext,
): Promise<SearchOutcome> {
  const matches: SearchMatch[] = [];
  let scannedFiles = 0;
  let skippedFiles = 0;
  let truncated = false;
  const pendingDirectories: Array<{ absolute: string; relative: string }> = [
    { absolute: resolved.absolutePath, relative: resolved.workspaceRelativePath },
  ];
  while (pendingDirectories.length > 0) {
    if (context.signal?.aborted) {
      return { status: "cancelled" };
    }
    const directory = pendingDirectories.pop();
    if (directory === undefined) {
      break;
    }
    let names: string[];
    try {
      names = await readdir(directory.absolute);
    } catch {
      continue;
    }
    names.sort();
    for (const name of names) {
      if (DEFAULT_EXCLUDED_DIRECTORIES.includes(name)) {
        continue;
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
        });
        continue;
      }
      if (!stats.isFile()) {
        skippedFiles += 1;
        continue;
      }
      if (stats.size > WORKSPACE_LIMITS.maxSearchFileSizeBytes) {
        skippedFiles += 1;
        continue;
      }
      if (scannedFiles >= WORKSPACE_LIMITS.maxSearchFiles) {
        truncated = true;
        return { status: "done", matches, scannedFiles, skippedFiles, truncated };
      }
      scannedFiles += 1;
      if (context.signal?.aborted) {
        return { status: "cancelled" };
      }
      let buffer: Buffer;
      try {
        buffer = await readFile(absolute);
      } catch {
        skippedFiles += 1;
        continue;
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
        const column = line.indexOf(query);
        if (column >= 0) {
          matches.push({
            path: relativePath,
            line: lineIndex + 1,
            column: column + 1,
            text: line.slice(0, WORKSPACE_LIMITS.maxSearchLineLengthChars),
          });
          if (matches.length >= maxResults) {
            truncated = true;
            return { status: "done", matches, scannedFiles, skippedFiles, truncated };
          }
        }
      }
    }
  }
  const sortedMatches = [...matches].sort(compareMatches);
  return {
    status: "done",
    matches: sortedMatches,
    scannedFiles,
    skippedFiles,
    truncated,
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
