import type { Tool, ToolExecutionContext, ToolExecutionResult } from "@solaris/core";
import {
  readArrayField,
  readJsonObject,
  readRequiredString,
} from "../../tools/workspace/validation.js";
import type { MutationPrepareResult } from "../scene-mutation/scene-mutation-service.js";

/**
 * Read-only `godot.prepare_scene_change` / `godot.prepare_resource_change`
 * provider tools (Stage 3 milestone 10, ADR 0026).
 *
 * These tools PREPARE only: they produce an immutable prepared mutation
 * (exact source revision, operation set, complete preview, expected
 * semantic effect, fingerprint) and never apply anything. Approval,
 * checkpointing, revalidation, structural apply, reparse, and semantic
 * verification are application-owned. The provider can never bypass the
 * revision check, preview, approval, checkpoint, or verification through
 * these tools — and no raw native text-write tool exists.
 */
export function createGodotPrepareSceneChangeTool(service: {
  prepareSceneChange(input: {
    readonly path: string;
    readonly operations: readonly unknown[];
  }): Promise<MutationPrepareResult>;
}): Tool {
  return createPrepareTool(
    service.prepareSceneChange.bind(service),
    "godot.prepare_scene_change",
    "scene",
  );
}

export function createGodotPrepareResourceChangeTool(service: {
  prepareResourceChange(input: {
    readonly path: string;
    readonly operations: readonly unknown[];
  }): Promise<MutationPrepareResult>;
}): Tool {
  return createPrepareTool(
    service.prepareResourceChange.bind(service),
    "godot.prepare_resource_change",
    "resource",
  );
}

function createPrepareTool(
  prepare: (input: {
    readonly path: string;
    readonly operations: readonly unknown[];
  }) => Promise<MutationPrepareResult>,
  name: string,
  kind: "scene" | "resource",
): Tool {
  return {
    definition: {
      name,
      description: `Prepare an approved ${kind} mutation for one .${kind === "scene" ? "tscn" : "tres"} file: typed operations (set/remove property, add/remove node, script attachment, resource references, signal connections, subresources), validated against the exact current revision, with a complete preview and a deterministic fingerprint. PREPARE ONLY: nothing is applied; application requires host approval binding the fingerprint, a checkpoint, revision revalidation, structural apply, reparse, and semantic verification.`,
      inputSchema: {
        type: "object",
        properties: {
          path: {
            type: "string",
            description: `Workspace-relative .${kind === "scene" ? "tscn" : "tres"} path, e.g. scenes/player.${kind === "scene" ? "tscn" : "tres"}.`,
          },
          operations: {
            type: "array",
            description: "Typed mutation operations (bounded).",
            items: { type: "object" },
          },
        },
        required: ["path", "operations"],
        additionalProperties: false,
      },
    },
    capability: "godot.inspect",
    async execute(input: unknown, context: ToolExecutionContext): Promise<ToolExecutionResult> {
      if (context.signal?.aborted) {
        return { status: "cancelled", message: "Mutation preparation was cancelled." };
      }
      const parsed = readJsonObject(input);
      if (!parsed.ok) {
        return { status: "invalid_input", message: parsed.message };
      }
      const path = readRequiredString(parsed.value, "path");
      if (!path.ok) {
        return { status: "invalid_input", message: path.message };
      }
      const rawOperations = readArrayField(parsed.value, "operations");
      if (!rawOperations.ok || rawOperations.value.length === 0) {
        return { status: "invalid_input", message: "operations must be a non-empty array." };
      }
      if (rawOperations.value.length > 32) {
        return {
          status: "invalid_input",
          message: "operations accepts at most 32 entries; reduce the operation set.",
        };
      }
      const result = await prepare({
        path: path.value,
        operations: rawOperations.value,
      });
      if (result.status !== "ready" || result.prepared === null) {
        return {
          status: result.status === "denied" ? "denied" : "failed",
          message: result.message ?? `${name} failed.`,
        };
      }
      const prepared = result.prepared;
      return {
        status: "success",
        summary: `Prepared ${kind} mutation for ${prepared.targetPath} at ${prepared.sourceRevision}: ${prepared.operations.length} operation(s); approval binds fingerprint ${prepared.fingerprint.slice(0, 16)}….`,
        output: {
          targetPath: prepared.targetPath,
          sourceRevision: prepared.sourceRevision,
          sourceSha256: prepared.sourceSha256,
          kind: prepared.kind,
          operations: prepared.operations.map((operation) => summarize(operation)),
          expectedSemanticEffect: prepared.expectedSemanticEffect.map(
            (expectation) => expectation.kind,
          ),
          fingerprint: prepared.fingerprint,
          preview: {
            structuralSummary: prepared.preview.structuralSummary,
            diff: prepared.preview.diff,
          },
        },
      };
    },
  };
}

function summarize(operation: { readonly op: string }): string {
  return operation.op;
}
