import { lstat, readFile, unlink } from "node:fs/promises";
import type {
  ChangePreview,
  CheckpointStore,
  FileCheckpoint,
  PreparedMutation,
  PreparedMutationTool,
  ToolExecutionContext,
  ToolExecutionResult,
  ToolPreparationResult,
} from "@solaris/core";
import { createPreparedMutation } from "@solaris/core";
import { WORKSPACE_LIMITS } from "../limits.js";
import { buildUnifiedDiff } from "./diff.js";
import { hashBuffer, hashMutationPlan } from "./mutation-hash.js";
import type { MutationLock } from "./mutation-lock.js";
import { resolveMutationTarget } from "./mutation-paths.js";
import { decodeUtf8, looksBinary } from "../text.js";
import { readJsonObject, readRequiredString, type ParsedValue } from "../validation.js";

interface DeleteInput {
  readonly path: string;
  readonly expectedSha256: string;
}

interface DeletePayload {
  readonly workspaceRelativePath: string;
  readonly absolutePath: string;
  readonly expectedSha256: string;
  readonly removedLines: number;
  readonly digest: string;
}

const SHA256_PATTERN = /^[0-9a-f]{64}$/;

function parseDeleteInput(input: unknown): ParsedValue<DeleteInput> {
  const object = readJsonObject(input);
  if (!object.ok) {
    return object;
  }
  const parsedPath = readRequiredString(object.value, "path");
  if (!parsedPath.ok) {
    return parsedPath;
  }
  const parsedHash = readRequiredString(object.value, "expectedSha256");
  if (!parsedHash.ok) {
    return parsedHash;
  }
  if (!SHA256_PATTERN.test(parsedHash.value)) {
    return {
      ok: false,
      message: '"expectedSha256" must be a lowercase 64-character SHA-256 hex digest.',
    };
  }
  return { ok: true, value: { path: parsedPath.value, expectedSha256: parsedHash.value } };
}

export function createWorkspaceDeleteFileTool(
  workspaceRoot: string,
  lock: MutationLock,
  store: CheckpointStore,
): PreparedMutationTool {
  const payloads = new WeakMap<PreparedMutation, DeletePayload>();

  async function prepare(
    input: unknown,
    context: ToolExecutionContext,
  ): Promise<ToolPreparationResult> {
    if (context.signal?.aborted) {
      return { status: "cancelled", message: "Preparation was cancelled." };
    }
    const parsed = parseDeleteInput(input);
    if (!parsed.ok) {
      return { status: "invalid_input", message: parsed.message };
    }
    const resolved = await resolveMutationTarget(workspaceRoot, parsed.value.path);
    if (resolved.status === "missing") {
      return { status: "conflict", message: resolved.message };
    }
    if (resolved.status !== "resolved") {
      return { status: "denied", message: resolved.message };
    }
    let bytes: Buffer;
    try {
      bytes = await readFile(resolved.absolutePath);
    } catch (error: unknown) {
      return { status: "failed", message: `Cannot read the file: ${describeError(error)}` };
    }
    if (bytes.length > WORKSPACE_LIMITS.maxTextFileSizeBytes) {
      return {
        status: "failed",
        message: `File is too large (limit ${WORKSPACE_LIMITS.maxTextFileSizeBytes} bytes).`,
      };
    }
    if (looksBinary(bytes)) {
      return {
        status: "failed",
        message: "File appears to be binary; only UTF-8 text files can be deleted.",
      };
    }
    const beforeSha256 = hashBuffer(bytes);
    if (beforeSha256 !== parsed.value.expectedSha256) {
      return {
        status: "conflict",
        message: "The file hash does not match expectedSha256; reread the file.",
      };
    }
    const text = decodeUtf8(bytes);
    if (text === null) {
      return { status: "failed", message: "File is not valid UTF-8 text." };
    }
    const diff = buildUnifiedDiff(resolved.workspaceRelativePath, text, "");
    if (diff.status === "too_large") {
      return { status: "failed", message: diff.message };
    }
    const filePreview = {
      path: resolved.workspaceRelativePath,
      operation: "delete" as const,
      beforeSha256,
      afterSha256: null,
      addedLines: 0,
      removedLines: diff.diff.removedLines,
      unifiedDiff: diff.diff.unifiedDiff,
    };
    const preview: ChangePreview = {
      files: [filePreview],
      totalAddedLines: 0,
      totalRemovedLines: diff.diff.removedLines,
      truncated: false,
    };
    const mutation = createPreparedMutation();
    const digest = hashMutationPlan({
      relativePath: resolved.workspaceRelativePath,
      operation: "delete",
      beforeSha256,
      afterSha256: null,
    });
    payloads.set(mutation, {
      workspaceRelativePath: resolved.workspaceRelativePath,
      absolutePath: resolved.absolutePath,
      expectedSha256: parsed.value.expectedSha256,
      removedLines: diff.diff.removedLines,
      digest,
    });
    return { status: "ready", mutation, preview, digest };
  }

  async function apply(
    prepared: PreparedMutation,
    context: ToolExecutionContext,
  ): Promise<ToolExecutionResult> {
    const payload = payloads.get(prepared);
    payloads.delete(prepared);
    if (payload === undefined) {
      return {
        status: "failed",
        message: "The prepared mutation is not valid for this tool or has already been used.",
      };
    }
    if (context.approvedDigest !== payload.digest) {
      return {
        status: "denied",
        message: "The prepared plan does not match the approved plan; the mutation was denied.",
      };
    }
    let release: () => void;
    try {
      release = await lock.acquire(context.signal);
    } catch (error: unknown) {
      if (context.signal?.aborted || isAbortError(error)) {
        return {
          status: "cancelled",
          message: "The mutation was cancelled while waiting for the lock.",
        };
      }
      throw error;
    }
    try {
      const revalidated = await resolveMutationTarget(workspaceRoot, payload.workspaceRelativePath);
      if (revalidated.status !== "resolved") {
        return {
          status: "conflict",
          message: `The target changed since the proposal: ${revalidated.message}`,
        };
      }
      let currentBytes: Buffer;
      try {
        currentBytes = await readFile(revalidated.absolutePath);
      } catch (error: unknown) {
        return {
          status: "conflict",
          message: `The target disappeared before deletion: ${describeError(error)}`,
        };
      }
      if (hashBuffer(currentBytes) !== payload.expectedSha256) {
        return {
          status: "conflict",
          message: "The file changed since the proposal was approved; reread the file.",
        };
      }
      let checkpoint: FileCheckpoint;
      try {
        checkpoint = await store.prepare(
          {
            relativePath: payload.workspaceRelativePath,
            operation: "delete",
            toolName: "workspace.delete_file",
            before: {
              exists: true,
              sha256: payload.expectedSha256,
              byteLength: currentBytes.length,
              bytes: new Uint8Array(currentBytes),
            },
            after: { exists: false, sha256: null, byteLength: null },
            preview: { addedLines: 0, removedLines: payload.removedLines },
          },
          context.signal,
        );
      } catch (error: unknown) {
        return {
          status: "failed",
          message: `Checkpoint could not be recorded; the mutation was not applied: ${describeError(error)}`,
        };
      }
      try {
        await unlink(payload.absolutePath);
      } catch (error: unknown) {
        return {
          status: "failed",
          message: `The file could not be deleted: ${describeError(error)}`,
        };
      }
      let stillExists = true;
      try {
        await lstat(payload.absolutePath);
      } catch {
        stillExists = false;
      }
      if (stillExists) {
        return {
          status: "failed",
          message: "Post-deletion verification failed: the file still exists.",
        };
      }
      try {
        await store.finalizeApplied(checkpoint.id, { afterSha256: null, absent: true });
      } catch (error: unknown) {
        return {
          status: "failed",
          message: `The mutation was applied but its checkpoint could not be finalized; recovery state is uncertain: ${describeError(error)}`,
        };
      }
      return {
        status: "success",
        output: {
          path: payload.workspaceRelativePath,
          operation: "delete",
          checkpointId: checkpoint.id,
          beforeSha256: payload.expectedSha256,
          removedLines: payload.removedLines,
        },
        summary: `Deleted ${payload.workspaceRelativePath} (-${payload.removedLines})`,
      };
    } finally {
      release();
    }
  }

  return {
    kind: "prepared_mutation",
    definition: {
      name: "workspace.delete_file",
      description: "Delete one existing UTF-8 text file after explicit review.",
      inputSchema: {
        type: "object",
        properties: {
          path: { type: "string", description: "File path relative to the workspace root." },
          expectedSha256: {
            type: "string",
            description: "Complete-file SHA-256 of the current file, from workspace.read.",
          },
        },
        required: ["path", "expectedSha256"],
        additionalProperties: false,
      },
    },
    capability: "workspace.write",
    prepare,
    apply,
  };
}

function isAbortError(error: unknown): boolean {
  return error instanceof Error && error.name === "AbortError";
}

function describeError(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return "An unknown mutation failure occurred.";
}
