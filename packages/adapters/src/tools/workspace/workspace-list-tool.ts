import { lstat, stat } from "node:fs/promises";
import path from "node:path";
import type { Tool, ToolExecutionContext, ToolExecutionResult, JsonValue } from "@siralos/core";
import { WORKSPACE_LIMITS } from "./limits.js";
import { MUTATION_TEMP_PREFIX } from "./mutations/mutation-temp.js";
import {
  DEFAULT_EXCLUDED_DIRECTORIES,
  describeFsError,
  findExcludedComponent,
  resolveWorkspacePath,
} from "./workspace-path.js";
import { enumerateDirectoryBounded } from "../../fs/directory-enumeration.js";
import { foldPathComponent } from "../../fs-case.js";
import { readJsonObject, readOptionalString, type ParsedValue } from "./validation.js";

interface ListInput {
  readonly path: string;
}

function parseListInput(input: unknown): ParsedValue<ListInput> {
  const object = readJsonObject(input);
  if (!object.ok) {
    return object;
  }
  const parsedPath = readOptionalString(object.value, "path");
  if (!parsedPath.ok) {
    return parsedPath;
  }
  return { ok: true, value: { path: parsedPath.value ?? "." } };
}

export function createWorkspaceListTool(workspaceRoot: string): Tool {
  return {
    definition: {
      name: "workspace.list",
      description: "List one directory within the approved workspace.",
      inputSchema: {
        type: "object",
        properties: {
          path: {
            type: "string",
            description:
              "Directory path relative to the workspace root. Defaults to the workspace root.",
          },
        },
        additionalProperties: false,
      },
    },
    async execute(input: unknown, context: ToolExecutionContext): Promise<ToolExecutionResult> {
      const parsed = parseListInput(input);
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
        return { status: "cancelled", message: "Listing was cancelled." };
      }
      let stats;
      try {
        stats = await stat(resolved.absolutePath);
      } catch (error: unknown) {
        return { status: "failed", message: `Cannot inspect directory: ${describeFsError(error)}` };
      }
      if (!stats.isDirectory()) {
        return { status: "failed", message: "Target is not a directory." };
      }
      // Entries are enumerated incrementally with a hard cap so a hostile
      // directory with millions of entries can never be materialized; the
      // cap counts excluded and hidden entries too. Exclusions fold case on
      // case-insensitive platforms so a `NODE_MODULES` spelling cannot
      // bypass the filter.
      const names: string[] = [];
      let truncated: boolean;
      try {
        const outcome = await enumerateDirectoryBounded({
          directory: resolved.absolutePath,
          maxEntries: WORKSPACE_LIMITS.maxDirectoryEntries + 1,
          signal: context.signal,
          onEntry: (entry) => {
            if (
              !DEFAULT_EXCLUDED_DIRECTORIES.some(
                (excluded) => foldPathComponent(excluded) === foldPathComponent(entry.name),
              ) &&
              !entry.name.startsWith(MUTATION_TEMP_PREFIX)
            ) {
              names.push(entry.name);
            }
          },
        });
        truncated = outcome.truncated;
      } catch (error: unknown) {
        if (context.signal?.aborted) {
          return { status: "cancelled", message: "Listing was cancelled." };
        }
        return { status: "failed", message: `Cannot list directory: ${describeFsError(error)}` };
      }
      names.sort();
      truncated = truncated || names.length > WORKSPACE_LIMITS.maxDirectoryEntries;
      const selectedNames = names.slice(0, WORKSPACE_LIMITS.maxDirectoryEntries);
      const entries: JsonValue[] = [];
      for (const name of selectedNames) {
        if (context.signal?.aborted) {
          return { status: "cancelled", message: "Listing was cancelled." };
        }
        let entryStats;
        try {
          entryStats = await lstat(path.join(resolved.absolutePath, name));
        } catch (error: unknown) {
          return { status: "failed", message: `Cannot inspect entry: ${describeFsError(error)}` };
        }
        const entryPath = entryRelativePath(resolved.workspaceRelativePath, name);
        if (entryStats.isSymbolicLink()) {
          entries.push({ name, path: entryPath, type: "symlink" });
        } else if (entryStats.isDirectory()) {
          entries.push({ name, path: entryPath, type: "directory" });
        } else if (entryStats.isFile()) {
          entries.push({ name, path: entryPath, type: "file", size: entryStats.size });
        } else {
          entries.push({ name, path: entryPath, type: "other" });
        }
      }
      return {
        status: "success",
        output: {
          path: resolved.workspaceRelativePath,
          entries,
          truncated,
        },
        summary: `${entries.length} entries${truncated ? " (truncated)" : ""}`,
      };
    },
  };
}

function entryRelativePath(directoryPath: string, name: string): string {
  return directoryPath === "." ? name : `${directoryPath}/${name}`;
}
