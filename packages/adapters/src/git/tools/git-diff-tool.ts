import type {
  GitDiffScope,
  GitInspector,
  Tool,
  ToolExecutionContext,
  ToolExecutionResult,
} from "@siralos/core";
import { GitError } from "@siralos/core";
import { errorMessage } from "../../support/error-message.js";
import {
  readJsonObject,
  readOptionalString,
  readArrayField,
  type ParsedValue,
} from "../../tools/workspace/validation.js";

interface DiffInput {
  readonly scope: GitDiffScope;
  readonly paths: readonly string[];
}

const SCOPES: readonly string[] = ["working", "staged", "head"];

function parseDiffInput(input: unknown): ParsedValue<DiffInput> {
  const object = readJsonObject(input);
  if (!object.ok) {
    return object;
  }
  const scopeValue = readOptionalString(object.value, "scope");
  if (!scopeValue.ok) {
    return scopeValue;
  }
  if (scopeValue.value !== undefined && !SCOPES.includes(scopeValue.value)) {
    return { ok: false, message: '"scope" must be one of: working, staged, head.' };
  }
  const pathsValue = readArrayField(object.value, "paths");
  if (!pathsValue.ok) {
    return pathsValue;
  }
  const paths: string[] = [];
  for (const entry of pathsValue.value) {
    if (typeof entry !== "string") {
      return { ok: false, message: '"paths" must contain only strings.' };
    }
    paths.push(entry);
  }
  return {
    ok: true,
    value: { scope: (scopeValue.value ?? "working") as GitDiffScope, paths },
  };
}

export function createGitDiffTool(git: GitInspector): Tool {
  return {
    definition: {
      name: "git.diff",
      description: "Show a bounded Git diff for the working tree, index, or HEAD.",
      inputSchema: {
        type: "object",
        properties: {
          scope: {
            type: "string",
            enum: ["working", "staged", "head"],
            description: "Diff scope. Defaults to working.",
          },
          paths: {
            type: "array",
            items: { type: "string" },
            description: "Optional workspace-relative paths to limit the diff to.",
          },
        },
        additionalProperties: false,
      },
    },
    capability: "git.inspect",
    async execute(input: unknown, context: ToolExecutionContext): Promise<ToolExecutionResult> {
      const parsed = parseDiffInput(input);
      if (!parsed.ok) {
        return { status: "invalid_input", message: parsed.message };
      }
      try {
        const result = await git.getDiff({
          scope: parsed.value.scope,
          ...(parsed.value.paths.length > 0 ? { paths: parsed.value.paths } : {}),
          ...(context.signal === undefined ? {} : { signal: context.signal }),
        });
        return {
          status: "success",
          output: {
            scope: result.scope,
            files: result.files.map((file) => ({
              path: file.path,
              originalPath: file.originalPath,
              operation: file.operation,
              addedLines: file.addedLines,
              removedLines: file.removedLines,
              binary: file.binary,
            })),
            patch: result.patch,
            truncated: result.truncated,
            untrackedExcluded: result.untrackedExcluded,
          },
          summary: `${result.files.length} files, +${totalAdded(result.files)} -${totalRemoved(result.files)}`,
        };
      } catch (error: unknown) {
        if (error instanceof GitError) {
          return { status: "failed", message: error.message };
        }
        return {
          status: "failed",
          message: errorMessage(error, "An unknown Git diff failure occurred."),
        };
      }
    },
  };
}

function totalAdded(files: readonly { addedLines: number }[]): number {
  return files.reduce((total, file) => total + file.addedLines, 0);
}

function totalRemoved(files: readonly { removedLines: number }[]): number {
  return files.reduce((total, file) => total + file.removedLines, 0);
}
