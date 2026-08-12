import { stat } from "node:fs/promises";
import { createHash } from "node:crypto";
import type {
  Tool,
  ToolExecutionContext,
  ToolExecutionResult,
  WorkspaceReadMode,
  WorkspaceRevisionRegistry,
} from "@siralos/core";
import {
  buildWorkspaceSummary,
  extractGDScriptStructure,
  isWorkspaceReadMode,
} from "@siralos/core";
import { WORKSPACE_LIMITS } from "./limits.js";
import {
  DEFAULT_EXCLUDED_DIRECTORIES,
  describeFsError,
  findExcludedComponent,
  resolveWorkspacePath,
} from "./workspace-path.js";
import { readFileBounded } from "../../fs/file-read.js";
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
  readonly mode?: WorkspaceReadMode;
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
  const rawMode = object.value["mode"];
  if (rawMode !== undefined && !isWorkspaceReadMode(rawMode)) {
    return { ok: false, message: `"mode" must be "exact", "structural", or "summary".` };
  }
  const mode: WorkspaceReadMode = rawMode === undefined ? "exact" : rawMode;
  const value: ReadInput = {
    path: parsedPath.value,
    startLine,
    ...(parsedEndLine.value === undefined ? {} : { endLine: parsedEndLine.value }),
    ...(mode === "exact" ? {} : { mode }),
  };
  return { ok: true, value };
}

export interface WorkspaceReadToolOptions {
  /** Session revision registry for revision-bound reads (opaque handles). */
  readonly revisions?: WorkspaceRevisionRegistry;
}

export function createWorkspaceReadTool(
  workspaceRoot: string,
  options: WorkspaceReadToolOptions = {},
): Tool {
  return {
    definition: {
      name: "workspace.read",
      description:
        "Read one text file inside the workspace. Modes: exact (authoritative source for editing, returns a revision handle), structural (deterministic GDScript declarations), summary (bounded advisory overview). Summaries/structural views are never authoritative source.",
      inputSchema: {
        type: "object",
        properties: {
          path: { type: "string", description: "File path relative to the workspace root." },
          startLine: {
            type: "integer",
            minimum: 1,
            description: "One-based start line (exact mode). Defaults to 1.",
          },
          endLine: {
            type: "integer",
            minimum: 1,
            description: "Inclusive one-based end line (exact mode). Defaults to the last line.",
          },
          mode: {
            type: "string",
            enum: ["exact", "structural", "summary"],
            description: "exact (default), structural, or summary.",
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
      // The read itself is capped: a file grown or swapped after the stat is
      // read only up to the size bound plus one byte, so a hostile
      // replacement can never drive an unbounded read or block on a FIFO.
      const buffer = await readFileBounded(
        resolved.absolutePath,
        WORKSPACE_LIMITS.maxReadFileSizeBytes,
      );
      if (buffer === null) {
        return {
          status: "failed",
          message: `Cannot read file: it is missing, not a regular file, or exceeds the ${WORKSPACE_LIMITS.maxReadFileSizeBytes}-byte limit.`,
        };
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
      const mode = parsed.value.mode ?? "exact";
      const sha256 = createHash("sha256").update(buffer).digest("hex");
      const revision =
        options.revisions === undefined
          ? null
          : options.revisions.issue(resolved.workspaceRelativePath, sha256);
      if (revision !== null) {
        options.revisions?.observeRead(resolved.workspaceRelativePath, revision, mode);
      }
      if (mode === "structural" || mode === "summary") {
        if (!resolved.workspaceRelativePath.toLowerCase().endsWith(".gd")) {
          return {
            status: "success",
            output: {
              path: resolved.workspaceRelativePath,
              mode,
              revision,
              supported: false,
              reason: "Structural and summary modes support GDScript (.gd) files only.",
            },
            summary: `${mode} read unsupported for this file type`,
          };
        }
        const structure = extractGDScriptStructure(text, resolved.workspaceRelativePath);
        if (mode === "structural") {
          return {
            status: "success",
            output: {
              path: resolved.workspaceRelativePath,
              mode: "structural",
              revision,
              structure: structure as unknown as import("@siralos/core").JsonObject,
            },
            summary: `structural: ${structure.functions.length} functions, ${structure.properties.length} properties, ${structure.signals.length} signals`,
          };
        }
        const summary = buildWorkspaceSummary(structure, revision, {
          maxBytes: Math.min(4096, text.length),
        });
        return {
          status: "success",
          output: {
            path: resolved.workspaceRelativePath,
            mode: "summary",
            revision,
            summary: summary.text,
            advisory: true,
            truncated: summary.truncated,
          },
          summary: `summary: ${summary.bytes} bytes${summary.truncated ? " (truncated)" : ""}`,
        };
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
          sha256,
          revision,
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
