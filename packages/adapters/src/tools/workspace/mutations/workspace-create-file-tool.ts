import { open, readFile } from "node:fs/promises";
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
import { resolveCreateTarget } from "./mutation-paths.js";
import { readJsonObject, readRequiredString, type ParsedValue } from "../validation.js";

interface CreatePayload {
  readonly workspaceRelativePath: string;
  readonly absolutePath: string;
  readonly content: Buffer;
  readonly afterSha256: string;
  readonly addedLines: number;
  readonly digest: string;
}

interface CreateInput {
  readonly path: string;
  readonly content: string;
}

function parseCreateInput(input: unknown): ParsedValue<CreateInput> {
  const object = readJsonObject(input);
  if (!object.ok) {
    return object;
  }
  const parsedPath = readRequiredString(object.value, "path");
  if (!parsedPath.ok) {
    return parsedPath;
  }
  const parsedContent = readRequiredString(object.value, "content");
  if (!parsedContent.ok) {
    return parsedContent;
  }
  return { ok: true, value: { path: parsedPath.value, content: parsedContent.value } };
}

export function createWorkspaceCreateFileTool(
  workspaceRoot: string,
  lock: MutationLock,
  store: CheckpointStore,
): PreparedMutationTool {
  const payloads = new WeakMap<PreparedMutation, CreatePayload>();

  async function prepare(
    input: unknown,
    context: ToolExecutionContext,
  ): Promise<ToolPreparationResult> {
    if (context.signal?.aborted) {
      return { status: "cancelled", message: "Preparation was cancelled." };
    }
    const parsed = parseCreateInput(input);
    if (!parsed.ok) {
      return { status: "invalid_input", message: parsed.message };
    }
    if (parsed.value.content.includes("\0")) {
      return { status: "invalid_input", message: "Content must be valid text without null bytes." };
    }
    if (Buffer.byteLength(parsed.value.content, "utf8") > WORKSPACE_LIMITS.maxCreatedContentBytes) {
      return {
        status: "invalid_input",
        message: `Content exceeds the ${WORKSPACE_LIMITS.maxCreatedContentBytes}-byte creation limit.`,
      };
    }
    const resolved = await resolveCreateTarget(workspaceRoot, parsed.value.path);
    if (resolved.status === "exists") {
      return { status: "conflict", message: resolved.message };
    }
    if (resolved.status !== "resolved") {
      return { status: "denied", message: resolved.message };
    }
    const content = Buffer.from(parsed.value.content, "utf8");
    const diff = buildUnifiedDiff(resolved.workspaceRelativePath, "", parsed.value.content);
    if (diff.status === "too_large") {
      return { status: "failed", message: diff.message };
    }
    const afterSha256 = hashBuffer(content);
    const filePreview = {
      path: resolved.workspaceRelativePath,
      operation: "create" as const,
      beforeSha256: null,
      afterSha256,
      addedLines: diff.diff.addedLines,
      removedLines: 0,
      unifiedDiff: diff.diff.unifiedDiff,
    };
    const preview: ChangePreview = {
      files: [filePreview],
      totalAddedLines: diff.diff.addedLines,
      totalRemovedLines: 0,
      truncated: false,
    };
    const mutation = createPreparedMutation();
    const digest = hashMutationPlan({
      relativePath: resolved.workspaceRelativePath,
      operation: "create",
      beforeSha256: null,
      afterSha256,
    });
    payloads.set(mutation, {
      workspaceRelativePath: resolved.workspaceRelativePath,
      absolutePath: resolved.absolutePath,
      content,
      afterSha256,
      addedLines: diff.diff.addedLines,
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
      const revalidated = await resolveCreateTarget(workspaceRoot, payload.workspaceRelativePath);
      if (revalidated.status !== "resolved") {
        return {
          status: "conflict",
          message: `The target changed since the proposal: ${revalidated.message}`,
        };
      }
      let checkpoint: FileCheckpoint;
      try {
        checkpoint = await store.prepare(
          {
            relativePath: payload.workspaceRelativePath,
            operation: "create",
            toolName: "workspace.create_file",
            before: { exists: false, sha256: null, byteLength: null, bytes: null },
            after: {
              exists: true,
              sha256: payload.afterSha256,
              byteLength: payload.content.length,
            },
            preview: { addedLines: payload.addedLines, removedLines: 0 },
          },
          context.signal,
        );
      } catch (error: unknown) {
        return {
          status: "failed",
          message: `Checkpoint could not be recorded; the mutation was not applied: ${describeError(error)}`,
        };
      }
      if (context.signal?.aborted) {
        return { status: "cancelled", message: "The mutation was cancelled before commit." };
      }
      const finalRevalidation = await resolveCreateTarget(
        workspaceRoot,
        payload.workspaceRelativePath,
      );
      if (finalRevalidation.status !== "resolved") {
        return {
          status: "conflict",
          message: `The target changed since the proposal: ${finalRevalidation.message}`,
        };
      }
      if (context.signal?.aborted) {
        return { status: "cancelled", message: "The mutation was cancelled before commit." };
      }
      let handle;
      try {
        handle = await open(finalRevalidation.absolutePath, "wx");
      } catch (error: unknown) {
        return {
          status: "conflict",
          message: `The target appeared before the write: ${describeError(error)}`,
        };
      }
      try {
        await handle.writeFile(payload.content);
        await handle.sync();
      } finally {
        await handle.close();
      }
      const verification = await verifyCreatedFile(payload);
      if (verification !== null) {
        return { status: "failed", message: verification };
      }
      try {
        await store.finalizeApplied(checkpoint.id, {
          afterSha256: payload.afterSha256,
          absent: false,
        });
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
          operation: "create",
          checkpointId: checkpoint.id,
          sha256: payload.afterSha256,
          bytesWritten: payload.content.length,
          addedLines: payload.addedLines,
        },
        summary: `Created ${payload.workspaceRelativePath} (+${payload.addedLines} -0)`,
      };
    } finally {
      release();
    }
  }

  async function verifyCreatedFile(payload: CreatePayload): Promise<string | null> {
    let bytes: Buffer;
    try {
      bytes = await readFile(payload.absolutePath);
    } catch (error: unknown) {
      return `Post-write verification could not read the file: ${describeError(error)}`;
    }
    if (bytes.length !== payload.content.length) {
      return "Post-write verification failed: the written bytes do not match.";
    }
    if (!bytes.equals(payload.content)) {
      return "Post-write verification failed: the written bytes do not match.";
    }
    if (hashBuffer(bytes) !== payload.afterSha256) {
      return "Post-write verification failed: the written hash does not match.";
    }
    return null;
  }

  return {
    kind: "prepared_mutation",
    definition: {
      name: "workspace.create_file",
      description: "Create one new UTF-8 text file inside an existing workspace directory.",
      inputSchema: {
        type: "object",
        properties: {
          path: { type: "string", description: "File path relative to the workspace root." },
          content: { type: "string", description: "UTF-8 text content for the new file." },
        },
        required: ["path", "content"],
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
