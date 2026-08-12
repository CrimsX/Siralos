import type {
  Reference,
  ReferenceAccessPort,
  ReferenceAlias,
  ReferenceId,
  ReferenceRegistry,
  ReferenceRevision,
  Tool,
  ToolExecutionContext,
  ToolExecutionResult,
} from "@siralos/core";
import { formatReferenceAlias } from "@siralos/core";
import {
  readJsonObject,
  readOptionalString,
  readRequiredString,
  type ParsedValue,
} from "../workspace/validation.js";
import { createReferenceReadTool } from "./reference-read-tool.js";
import { createReferenceSearchTool } from "./reference-search-tool.js";

/**
 * `reference.list` tool (Stage 3 milestone 5): list one directory inside a
 * declared external reference. References are read-only external material
 * outside the Siralos workspace — the reference namespace is separate from
 * the workspace namespace, and this tool never mutates anything.
 */

export type ReferenceTool = Tool & { readonly capability: "reference.inspect" };

export interface ReferenceToolDependencies {
  readonly registry: ReferenceRegistry;
  readonly access: ReferenceAccessPort;
}

/** Model-facing anchor of a revision: `{kind, commit}` or `{kind, fingerprint}`. Never absolute paths. */
export function referenceRevisionAnchor(
  revision: ReferenceRevision,
):
  | { readonly kind: "repository"; readonly commit: string }
  | { readonly kind: "local-directory"; readonly fingerprint: string } {
  if (revision.identity.kind === "repository") {
    return { kind: "repository", commit: revision.identity.commit };
  }
  return { kind: "local-directory", fingerprint: revision.identity.fingerprint };
}

type ResolvedReferenceSelector =
  | { readonly ok: true; readonly reference: Reference; readonly revision: ReferenceRevision }
  | { readonly ok: false; readonly message: string };

/** Resolve a model-supplied selector (alias or id) through the registry — never inferred from a path. */
export function resolveReferenceSelector(
  registry: ReferenceRegistry,
  selector: string,
): ResolvedReferenceSelector {
  const reference = registry.get(selector as ReferenceAlias | ReferenceId);
  if (reference === undefined) {
    return { ok: false, message: `Unknown reference: ${selector}.` };
  }
  if (reference.status !== "ready") {
    const message =
      reference.failureReason ?? `Reference ${selector} is not ready (${reference.status}).`;
    return { ok: false, message };
  }
  const revision = registry.revision(reference.id);
  if (revision === null) {
    return { ok: false, message: `Reference ${selector} has no resolved revision.` };
  }
  return { ok: true, reference, revision };
}

export function mapReferenceFailure(result: {
  readonly status: string;
  readonly reason: string;
}): ToolExecutionResult {
  if (result.status === "unavailable") {
    return { status: "unavailable", message: result.reason };
  }
  return { status: "failed", message: result.reason };
}

interface ListInput {
  readonly reference: string;
  readonly path: string;
}

function parseListInput(input: unknown): ParsedValue<ListInput> {
  const object = readJsonObject(input);
  if (!object.ok) {
    return object;
  }
  const parsedReference = readRequiredString(object.value, "reference");
  if (!parsedReference.ok) {
    return parsedReference;
  }
  const parsedPath = readOptionalString(object.value, "path");
  if (!parsedPath.ok) {
    return parsedPath;
  }
  return { ok: true, value: { reference: parsedReference.value, path: parsedPath.value ?? "." } };
}

export function createReferenceListTool(dependencies: ReferenceToolDependencies): ReferenceTool {
  const { registry, access } = dependencies;
  return {
    definition: {
      name: "reference.list",
      description:
        "List one directory inside a declared external reference. References are read-only external material outside the Siralos workspace; the reference namespace is separate from the workspace namespace.",
      inputSchema: {
        type: "object",
        properties: {
          reference: {
            type: "string",
            minLength: 1,
            maxLength: 128,
            description: "Reference alias (or ref_ id) to list from.",
          },
          path: {
            type: "string",
            maxLength: 4096,
            description: "Reference-relative directory path. Defaults to the reference root.",
          },
        },
        required: ["reference"],
        additionalProperties: false,
      },
    },
    capability: "reference.inspect",
    async execute(input: unknown, context: ToolExecutionContext): Promise<ToolExecutionResult> {
      const parsed = parseListInput(input);
      if (!parsed.ok) {
        return { status: "invalid_input", message: parsed.message };
      }
      const resolved = resolveReferenceSelector(registry, parsed.value.reference);
      if (!resolved.ok) {
        return { status: "unavailable", message: resolved.message };
      }
      if (context.signal?.aborted) {
        return { status: "cancelled", message: "Listing was cancelled." };
      }
      const result = await access.list({
        reference: resolved.reference.id,
        ...(parsed.value.path === "." ? {} : { path: parsed.value.path }),
      });
      if (result.status !== "ok") {
        return mapReferenceFailure(result);
      }
      return {
        status: "success",
        output: {
          reference: formatReferenceAlias(result.alias),
          revision: referenceRevisionAnchor(result.revision),
          path: result.path,
          entries: result.entries,
          truncated: result.truncated,
        },
        summary: `${result.entries.length} entries${result.truncated ? " (truncated)" : ""}`,
      };
    },
  };
}

export function createReferenceTools(
  dependencies: ReferenceToolDependencies,
): readonly ReferenceTool[] {
  return [
    createReferenceListTool(dependencies),
    createReferenceReadTool(dependencies),
    createReferenceSearchTool(dependencies),
  ];
}
