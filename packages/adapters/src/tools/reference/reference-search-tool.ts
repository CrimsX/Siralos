import type { ToolExecutionContext, ToolExecutionResult } from "@siralos/core";
import { formatReferenceAlias } from "@siralos/core";
import {
  readJsonObject,
  readOptionalPositiveInteger,
  readOptionalString,
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
 * `reference.search` tool (Stage 3 milestone 5): search text files
 * recursively within a bounded directory of a declared external reference.
 * The traversal is bounded by the same class of independent budgets as
 * workspace.search, with explicit truncation disclosure. References are
 * read-only external material — never the workspace.
 */

interface SearchInput {
  readonly reference: string;
  readonly query: string;
  readonly path: string;
  readonly maxResults?: number;
}

function parseSearchInput(input: unknown): ParsedValue<SearchInput> {
  const object = readJsonObject(input);
  if (!object.ok) {
    return object;
  }
  const parsedReference = readRequiredString(object.value, "reference");
  if (!parsedReference.ok) {
    return parsedReference;
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
  return {
    ok: true,
    value: {
      reference: parsedReference.value,
      query: parsedQuery.value,
      path: parsedPath.value ?? ".",
      ...(parsedMaxResults.value === undefined ? {} : { maxResults: parsedMaxResults.value }),
    },
  };
}

export function createReferenceSearchTool(dependencies: ReferenceToolDependencies): ReferenceTool {
  const { registry, access } = dependencies;
  return {
    definition: {
      name: "reference.search",
      description:
        "Search text files recursively within a bounded directory of a declared external reference. References are read-only external material outside the Siralos workspace.",
      inputSchema: {
        type: "object",
        properties: {
          reference: {
            type: "string",
            minLength: 1,
            maxLength: 128,
            description: "Reference alias (or ref_ id) to search in.",
          },
          query: {
            type: "string",
            minLength: 1,
            maxLength: 400,
            description: "Literal text to search for (case-sensitive).",
          },
          path: {
            type: "string",
            maxLength: 4096,
            description: "Reference-relative directory path. Defaults to the reference root.",
          },
          maxResults: {
            type: "integer",
            minimum: 1,
            description: "Maximum number of matches to return.",
          },
        },
        required: ["reference", "query"],
        additionalProperties: false,
      },
    },
    capability: "reference.inspect",
    async execute(input: unknown, context: ToolExecutionContext): Promise<ToolExecutionResult> {
      const parsed = parseSearchInput(input);
      if (!parsed.ok) {
        return { status: "invalid_input", message: parsed.message };
      }
      const resolved = resolveReferenceSelector(registry, parsed.value.reference);
      if (!resolved.ok) {
        return { status: "unavailable", message: resolved.message };
      }
      if (context.signal?.aborted) {
        return { status: "cancelled", message: "Search was cancelled." };
      }
      const result = await access.search({
        reference: resolved.reference.id,
        query: parsed.value.query,
        ...(parsed.value.path === "." ? {} : { path: parsed.value.path }),
        ...(parsed.value.maxResults === undefined ? {} : { maxResults: parsed.value.maxResults }),
      });
      if (context.signal?.aborted) {
        return { status: "cancelled", message: "Search was cancelled." };
      }
      if (result.status !== "ok") {
        return mapReferenceFailure(result);
      }
      return {
        status: "success",
        output: {
          reference: formatReferenceAlias(result.alias),
          revision: referenceRevisionAnchor(result.revision),
          query: result.query,
          matches: result.matches,
          scannedFiles: result.scannedFiles,
          skippedFiles: result.skippedFiles,
          truncated: result.truncated,
          truncationReason: result.truncationReason,
        },
        summary: `${result.matches.length} matches${result.truncated ? " (truncated)" : ""}`,
      };
    },
  };
}
