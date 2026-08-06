import { readFile, stat } from "node:fs/promises";
import type { Tool, ToolExecutionContext, ToolExecutionResult } from "@solaris/core";
import { WORKSPACE_LIMITS } from "./limits.js";
import {
  DEFAULT_EXCLUDED_DIRECTORIES,
  describeFsError,
  findExcludedComponent,
  resolveWorkspacePath,
} from "./workspace-path.js";
import { decodeUtf8, looksBinary, splitIntoLines } from "./text.js";
import {
  readJsonObject,
  readOptionalPositiveInteger,
  readRequiredString,
  type ParsedValue,
} from "./validation.js";

interface ReadInput {
  readonly path: string;
  readonly startLine?: number;
  readonly endLine?: number;
}

function parseReadInput(input: unknown): ParsedValue<ReadInput> {
  const object = readJsonObject(input);
  if (!object.ok) {
    return object;
  }
  const parsedPath = readRequiredString(object.value, "path");
  if (!parsedPath.ok) {
    return parsedPath;
  }
  const parsedStartLine = readOptionalPositiveInteger(object.value, "startLine");
  if (!parsedStartLine.ok) {
    return parsedStartLine;
  }
  const parsedEndLine = readOptionalPositiveInteger(object.value, "endLine");
  if (!parsedEndLine.ok) {
    return parsedEndLine;
  }
  const startLine = parsedStartLine.value ?? 1;
  if (parsedEndLine.value !== undefined && parsedEndLine.value < startLine) {
    return { ok: false, message: `"endLine" must not precede "startLine".` };
  }
  const value: ReadInput = {
    path: parsedPath.value,
    startLine,
    ...(parsedEndLine.value === undefined ? {} : { endLine: parsedEndLine.value }),
  };
  return { ok: true, value };
}

export function createWorkspaceReadTool(workspaceRoot: string): Tool {
  return {
    definition: {
      name: "workspace.read",
      description: "Read a bounded range from one text file inside the workspace.",
      inputSchema: {
        type: "object",
        properties: {
          path: { type: "string", description: "File path relative to the workspace root." },
          startLine: {
            type: "integer",
            minimum: 1,
            description: "One-based start line. Defaults to 1.",
          },
          endLine: {
            type: "integer",
            minimum: 1,
            description: "Inclusive one-based end line. Defaults to the last line.",
          },
        },
        required: ["path"],
        additionalProperties: false,
      },
    },
    async execute(input: unknown, context: ToolExecutionContext): Promise<ToolExecutionResult> {
      const parsed = parseReadInput(input);
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
        return { status: "cancelled", message: "Reading was cancelled." };
      }
      let stats;
      try {
        stats = await stat(resolved.absolutePath);
      } catch (error: unknown) {
        return { status: "failed", message: `Cannot inspect file: ${describeFsError(error)}` };
      }
      if (!stats.isFile()) {
        return { status: "failed", message: "Target is not a regular file." };
      }
      if (stats.size > WORKSPACE_LIMITS.maxReadFileSizeBytes) {
        return {
          status: "failed",
          message: `File is too large (${stats.size} bytes; limit ${WORKSPACE_LIMITS.maxReadFileSizeBytes}).`,
        };
      }
      let buffer: Buffer;
      try {
        buffer = await readFile(resolved.absolutePath);
      } catch (error: unknown) {
        return { status: "failed", message: `Cannot read file: ${describeFsError(error)}` };
      }
      if (context.signal?.aborted) {
        return { status: "cancelled", message: "Reading was cancelled." };
      }
      if (looksBinary(buffer)) {
        return { status: "failed", message: "File appears to be binary." };
      }
      const text = decodeUtf8(buffer);
      if (text === null) {
        return { status: "failed", message: "File is not valid UTF-8 text." };
      }
      const lines = splitIntoLines(text);
      const totalLines = lines.length;
      const startLine = parsed.value.startLine ?? 1;
      if (startLine > totalLines) {
        return {
          status: "failed",
          message: `"startLine" (${startLine}) is beyond the end of the file (${totalLines} lines).`,
        };
      }
      const endLine = Math.min(parsed.value.endLine ?? totalLines, totalLines);
      let content = lines.slice(startLine - 1, endLine).join("\n");
      let truncated = false;
      if (content.length > WORKSPACE_LIMITS.maxReadContentChars) {
        content = content.slice(0, WORKSPACE_LIMITS.maxReadContentChars);
        truncated = true;
      }
      return {
        status: "success",
        output: {
          path: resolved.workspaceRelativePath,
          content,
          startLine,
          endLine,
          totalLines,
          truncated,
        },
        summary: `${endLine - startLine + 1} lines${truncated ? " (truncated)" : ""}`,
      };
    },
  };
}
