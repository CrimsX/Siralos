import type {
  PreparedMutation,
  PreparedMutationTool,
  ToolExecutionContext,
  ToolExecutionResult,
  ToolPreparationResult,
} from "@solaris/core";
import type { CheckpointStore } from "@solaris/core";
import type { MutationLock } from "./mutation-lock.js";

export const WORKSPACE_EDIT_UNAVAILABLE_MESSAGE =
  "workspace.edit_file is unavailable: Node offers no directory-relative (openat/renameat) primitive, so a same-user process that swaps a parent or target at any instruction boundary can redirect pathname-based staging and replacement outside the workspace. The operation fails closed before any write; it will become available when a mechanically identity-bound commit primitive exists.";

/**
 * The workspace mutation tools fail closed before any write.
 *
 * The required invariant — no mutation may create, write, link, rename,
 * replace, unlink, or remove an outside entry even when a same-user process
 * swaps a parent or target at any instruction boundary — cannot be enforced
 * with Node's pathname-based filesystem APIs: every open, rename, link, and
 * unlink resolves a mutable pathname, and verification-then-use is not
 * atomic. The pinned runtime exposes no dirfd-relative primitive and no
 * native adapter is shipped. Rather than performing a partial outside
 * mutation and describing cleanup as prevention, every workspace mutation is
 * refused before any filesystem activity, approval, or checkpoint recording.
 */
export function createWorkspaceEditFileTool(
  _workspaceRoot: string,
  _lock: MutationLock,
  _store: CheckpointStore,
): PreparedMutationTool {
  return {
    kind: "prepared_mutation",
    definition: {
      name: "workspace.edit_file",
      description:
        "Unavailable: Node cannot bind pathname-based replacement to a verified parent against a same-user adversary, so the operation fails closed before any write.",
      inputSchema: {
        type: "object",
        properties: {
          path: { type: "string", description: "Workspace-relative file path." },
          expectedSha256: {
            type: "string",
            description: "Complete SHA-256 of the file's current bytes.",
          },
          replacements: {
            type: "array",
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
        },
        required: ["path", "expectedSha256", "replacements"],
        additionalProperties: false,
      },
    },
    capability: "workspace.write",
    prepare(_input: unknown, context: ToolExecutionContext): Promise<ToolPreparationResult> {
      if (context.signal?.aborted) {
        return Promise.resolve({ status: "cancelled", message: "Preparation was cancelled." });
      }
      return Promise.resolve({
        status: "unavailable",
        message: WORKSPACE_EDIT_UNAVAILABLE_MESSAGE,
      });
    },
    apply(
      _prepared: PreparedMutation,
      _context: ToolExecutionContext,
    ): Promise<ToolExecutionResult> {
      return Promise.resolve({
        status: "unavailable",
        message: WORKSPACE_EDIT_UNAVAILABLE_MESSAGE,
      });
    },
  };
}
