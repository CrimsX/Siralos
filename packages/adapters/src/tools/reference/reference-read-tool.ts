import type { ToolExecutionContext, ToolExecutionResult } from "@solaris/core";
import { formatReferenceAlias, isWorkspaceReadMode } from "@solaris/core";
import {
  readJsonObject,
  readOptionalPositiveInteger,
  readRequiredString,
  type ParsedValue,
} from "../workspace/validation.js";
import {
  mapReferenceFailure,
  referenceRevisionAnchor,
  resolveReferenceSelector,
  type ReferenceTool,
  type ReferenceToolDependencies,
} from "./reference-list-tool.js";

/**
 * `reference.read` tool (Stage 3 milestone 5): read one text file inside a
 * declared external reference. Modes mirror workspace.read: exact
 * (authoritative source for editing, with a SHA-256), structural
 * (deterministic GDScript declarations), summary (bounded advisory
 * overview). References are read-only external material — never the
 * workspace — and nothing here mutates anything.
 */

type ReadMode = "exact" | "structural" | "summary";

interface ReadInput {
  readonly reference: string;
  readonly path: string;
  readonly mode: ReadMode;
  readonly startLine?: number;
  readonly endLine?: number;
}

function parseReadInput(input: unknown): ParsedValue<ReadInput> {
  const object = readJsonObject(input);
  if (!object.ok) {
    return object;
  }
  const parsedReference = readRequiredString(object.value, "reference");
  if (!parsedReference.ok) {
    return parsedReference;
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
  const mode: ReadMode = rawMode === undefined ? "exact" : rawMode;
  const value: ReadInput = {
    reference: parsedReference.value,
    path: parsedPath.value,
    mode,
    ...(parsedStartLine.value === undefined ? {} : { startLine: parsedStartLine.value }),
    ...(parsedEndLine.value === undefined ? {} : { endLine: parsedEndLine.value }),
  };
  return { ok: true, value };
}

export function createReferenceReadTool(dependencies: ReferenceToolDependencies): ReferenceTool {
  const { registry, access } = dependencies;
  return {
    definition: {
      name: "reference.read",
      description:
        "Read one text file inside a declared external reference. Modes: exact (authoritative source, returns the SHA-256), structural (deterministic GDScript declarations), summary (bounded advisory overview). Summaries/structural views are never authoritative source. References are read-only external material outside the Solaris workspace.",
      inputSchema: {
        type: "object",
        properties: {
          reference: {
            type: "string",
            minLength: 1,
            maxLength: 128,
            description: "Reference alias (or ref_ id) to read from.",
          },
          path: {
            type: "string",
            minLength: 1,
            maxLength: 4096,
            description: "File path relative to the reference root.",
          },
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
        required: ["reference", "path"],
        additionalProperties: false,
      },
    },
    capability: "reference.inspect",
    async execute(input: unknown, context: ToolExecutionContext): Promise<ToolExecutionResult> {
      const parsed = parseReadInput(input);
      if (!parsed.ok) {
        return { status: "invalid_input", message: parsed.message };
      }
      const resolved = resolveReferenceSelector(registry, parsed.value.reference);
      if (!resolved.ok) {
        return { status: "unavailable", message: resolved.message };
      }
      if (context.signal?.aborted) {
        return { status: "cancelled", message: "Reading was cancelled." };
      }
      const result = await access.read({
        reference: resolved.reference.id,
        path: parsed.value.path,
        mode: parsed.value.mode,
        ...(parsed.value.startLine === undefined ? {} : { startLine: parsed.value.startLine }),
        ...(parsed.value.endLine === undefined ? {} : { endLine: parsed.value.endLine }),
      });
      if (context.signal?.aborted) {
        return { status: "cancelled", message: "Reading was cancelled." };
      }
      if (result.status !== "ok") {
        return mapReferenceFailure(result);
      }
      const mode = parsed.value.mode;
      let readSummary: string;
      if (mode === "structural") {
        readSummary = "structural view";
      } else if (mode === "summary") {
        readSummary = `summary${result.truncated ? " (truncated)" : ""}`;
      } else {
        readSummary = "exact read";
      }
      return {
        status: "success",
        output: {
          reference: formatReferenceAlias(result.alias),
          revision: referenceRevisionAnchor(result.revision),
          path: result.path,
          mode,
          sha256: result.sha256,
          content: result.content,
          structure:
            result.structure === null
              ? null
              : (result.structure as unknown as import("@solaris/core").JsonObject),
          summary: result.summary as string | null,
          truncated: result.truncated,
          ...(mode === "summary" ? { advisory: true } : {}),
        },
        summary: readSummary,
      };
    },
  };
}
