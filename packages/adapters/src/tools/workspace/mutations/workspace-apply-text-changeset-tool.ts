import type {
  GDScriptDevelopmentService,
  PreparedMutation,
  PreparedMutationTool,
  ToolExecutionContext,
  ToolExecutionResult,
  ToolPreparationResult,
} from "@solaris/core";
import { createPreparedMutation } from "@solaris/core";

export const CHANGE_SET_REQUIRES_WORKFLOW_MESSAGE =
  "workspace.apply_text_changeset requires an active development workflow (start one with /develop): outside the workflow every workspace mutation fails closed and no approval for mutations is ever requested.";

/**
 * `workspace.apply_text_changeset` — the provider's exact text change-set
 * tool inside a development workflow (§17–§23).
 *
 * The provider proposes create/edit/delete changes for bounded UTF-8 text
 * files with exact current SHA-256 preconditions; preparation is read-only
 * and freezes the immutable change-set digest; the one-time approval binds
 * to exactly that digest; application runs through the workflow
 * orchestration (language-session suspension, checkpoints, sequential
 * hash-verified application, parser gate, fresh language session,
 * validation evidence, repair budgeting).
 *
 * Outside an active workflow — and whenever the change-set applier is
 * unavailable on this platform — preparation fails closed with a typed
 * `unavailable` result before any approval is requested, exactly like the
 * sibling mutation tools.
 */
export function createWorkspaceApplyTextChangesetTool(
  development: GDScriptDevelopmentService,
): PreparedMutationTool {
  const preparedByHandle = new Map<PreparedMutation, string>();
  return {
    kind: "prepared_mutation",
    definition: {
      name: "workspace.apply_text_changeset",
      description:
        "Propose one exact text change set (create/edit/delete of bounded UTF-8 text files) inside the active GDScript development workflow. Every existing file requires its exact current SHA-256; the complete diff is shown and approved once before application; checkpoints, parser validation, and fresh language diagnostics follow automatically. Fails closed as unavailable outside a workflow or when the platform cannot apply identity-bound mutations.",
      inputSchema: {
        type: "object",
        additionalProperties: false,
        properties: {
          changes: {
            type: "array",
            description: "Bounded exact text changes: create, edit (exact replacements), delete.",
            items: {
              type: "object",
              properties: {
                operation: {
                  type: "string",
                  enum: ["create", "edit", "delete"],
                },
                path: { type: "string", description: "Workspace-relative path." },
                expectedSha256: {
                  type: "string",
                  description: "Exact current SHA-256 (required for edit and delete).",
                },
                replacements: {
                  type: "array",
                  description: "Exact text replacements (edit only).",
                  items: {
                    type: "object",
                    properties: {
                      oldText: { type: "string" },
                      newText: { type: "string" },
                    },
                    required: ["oldText", "newText"],
                    additionalProperties: false,
                  },
                },
                content: {
                  type: "string",
                  description: "Complete UTF-8 text content (create only).",
                },
              },
              required: ["operation", "path"],
              additionalProperties: false,
            },
          },
        },
        required: ["changes"],
      },
    },
    capability: "workspace.write",
    async prepare(input: unknown, context: ToolExecutionContext): Promise<ToolPreparationResult> {
      const prepared = await development.prepareChangeSet(input, context);
      switch (prepared.status) {
        case "ready": {
          const handle = createPreparedMutation();
          preparedByHandle.set(handle, prepared.changeSetId);
          return {
            status: "ready",
            mutation: handle,
            preview: prepared.preview,
            digest: prepared.digest,
          };
        }
        case "cancelled":
          return { status: "cancelled", message: prepared.message };
        case "unavailable":
          return { status: "unavailable", message: prepared.message };
        case "stale_revision":
          return {
            status: "failed",
            message: prepared.message,
          };
        case "invalid_input":
        case "conflict":
        case "changeset_too_large":
        case "repair_budget_exhausted":
        case "iteration_budget_exhausted":
        case "failed":
          return { status: "failed", message: prepared.message };
      }
    },
    async apply(
      prepared: PreparedMutation,
      context: ToolExecutionContext,
    ): Promise<ToolExecutionResult> {
      const changeSetId = preparedByHandle.get(prepared);
      preparedByHandle.delete(prepared);
      if (changeSetId === undefined) {
        return {
          status: "failed",
          message:
            "The prepared change set is not valid for this session; prepare a new change set.",
        };
      }
      const result = await development.applyChangeSet(changeSetId, {
        approvedDigest: context.approvedDigest ?? "",
        ...(context.signal === undefined ? {} : { signal: context.signal }),
      });
      switch (result.status) {
        case "applied": {
          const record = result.result;
          return {
            status: "success",
            output: {
              status: record.status,
              iterations: record.iterations,
              changedFiles: record.changes.map((change) => ({
                path: change.path,
                operation: change.operation,
                beforeSha256: change.beforeSha256,
                afterSha256: change.afterSha256,
              })),
              diagnostics: record.diagnostics,
              validation: record.validation,
              checkpointIds: record.checkpointIds,
              quality: record.quality as import("@solaris/core").JsonValue | null,
            },
            summary: `change set applied: ${record.changes.length} file(s), ${record.diagnostics.errors} errors, ${record.diagnostics.warnings} warnings`,
          };
        }
        case "denied":
          return { status: "denied", message: result.message };
        case "conflict":
          return { status: "conflict", message: result.message };
        case "cancelled":
          return { status: "cancelled", message: result.message };
        case "unavailable":
          return { status: "unavailable", message: result.message };
        case "apply_failed":
        case "validation_failed":
        case "failed":
          return { status: "failed", message: result.message };
      }
    },
  };
}
