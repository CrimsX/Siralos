import { open, readFile, rename, unlink } from "node:fs/promises";
import { dirname } from "node:path";
import type {
  ChangePreview,
  PreparedMutation,
  PreparedMutationTool,
  ToolExecutionContext,
  ToolExecutionResult,
  ToolPreparationResult,
} from "@solaris/core";
import { createPreparedMutation } from "@solaris/core";
import { WORKSPACE_LIMITS } from "../limits.js";
import { buildUnifiedDiff } from "./diff.js";
import { hashBuffer } from "./mutation-hash.js";
import type { MutationLock } from "./mutation-lock.js";
import { resolveMutationTarget } from "./mutation-paths.js";
import { createMutationTempPath, removeMutationTemp } from "./mutation-temp.js";
import { decodeUtf8, looksBinary } from "../text.js";
import {
  readArrayField,
  readJsonObject,
  readOptionalString,
  readRequiredString,
  type ParsedValue,
} from "../validation.js";

interface Replacement {
  readonly oldText: string;
  readonly newText: string;
}

interface EditInput {
  readonly path: string;
  readonly expectedSha256: string;
  readonly replacements: readonly Replacement[];
}

interface EditPayload {
  readonly workspaceRelativePath: string;
  readonly absolutePath: string;
  readonly expectedSha256: string;
  readonly originalBytes: Buffer;
  readonly newContent: Buffer;
  readonly afterSha256: string;
  readonly addedLines: number;
  readonly removedLines: number;
}

const SHA256_PATTERN = /^[0-9a-f]{64}$/;

function parseEditInput(input: unknown): ParsedValue<EditInput> {
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
  const replacementsField = readArrayField(object.value, "replacements");
  if (!replacementsField.ok) {
    return replacementsField;
  }
  const replacementsValue = replacementsField.value;
  if (replacementsValue.length === 0) {
    return { ok: false, message: "At least one replacement is required." };
  }
  if (replacementsValue.length > WORKSPACE_LIMITS.maxReplacements) {
    return {
      ok: false,
      message: `At most ${WORKSPACE_LIMITS.maxReplacements} replacements are allowed.`,
    };
  }
  const replacements: Replacement[] = [];
  for (let index = 0; index < replacementsValue.length; index += 1) {
    const entry = replacementsValue[index];
    const entryObject = readJsonObject(entry);
    if (!entryObject.ok) {
      return { ok: false, message: `replacements[${index}] must be an object.` };
    }
    const oldText = readOptionalString(entryObject.value, "oldText");
    if (!oldText.ok) {
      return oldText;
    }
    const newText = readOptionalString(entryObject.value, "newText");
    if (!newText.ok) {
      return newText;
    }
    if (oldText.value === undefined || oldText.value.length === 0) {
      return { ok: false, message: `replacements[${index}].oldText must be non-empty.` };
    }
    if (newText.value === undefined) {
      return { ok: false, message: `replacements[${index}].newText is required.` };
    }
    if (Buffer.byteLength(oldText.value, "utf8") > WORKSPACE_LIMITS.maxReplacementTextBytes) {
      return { ok: false, message: `replacements[${index}].oldText exceeds the size limit.` };
    }
    if (Buffer.byteLength(newText.value, "utf8") > WORKSPACE_LIMITS.maxReplacementTextBytes) {
      return { ok: false, message: `replacements[${index}].newText exceeds the size limit.` };
    }
    replacements.push({ oldText: oldText.value, newText: newText.value });
  }
  return {
    ok: true,
    value: { path: parsedPath.value, expectedSha256: parsedHash.value, replacements },
  };
}

export function createWorkspaceEditFileTool(
  workspaceRoot: string,
  lock: MutationLock,
): PreparedMutationTool {
  const payloads = new WeakMap<PreparedMutation, EditPayload>();

  async function prepare(
    input: unknown,
    context: ToolExecutionContext,
  ): Promise<ToolPreparationResult> {
    if (context.signal?.aborted) {
      return { status: "cancelled", message: "Preparation was cancelled." };
    }
    const parsed = parseEditInput(input);
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
    const readResult = await readBoundedFile(resolved.absolutePath);
    if (readResult.status === "too_large") {
      return { status: "failed", message: readResult.message };
    }
    if (looksBinary(readResult.bytes)) {
      return {
        status: "failed",
        message: "File appears to be binary; only UTF-8 text files can be edited.",
      };
    }
    const currentHash = hashBuffer(readResult.bytes);
    if (currentHash !== parsed.value.expectedSha256) {
      return {
        status: "conflict",
        message: "The file hash does not match expectedSha256; reread the file.",
      };
    }
    const originalText = decodeUtf8(readResult.bytes);
    if (originalText === null) {
      return { status: "failed", message: "File is not valid UTF-8 text." };
    }
    const replacementOutcome = applyReplacements(originalText, parsed.value.replacements);
    if (replacementOutcome.status !== "applied") {
      return { status: "conflict", message: replacementOutcome.message };
    }
    if (replacementOutcome.text === originalText) {
      return {
        status: "failed",
        message: "The resulting content is identical; there is no change to apply.",
      };
    }
    const newBytes = Buffer.from(replacementOutcome.text, "utf8");
    if (newBytes.length > WORKSPACE_LIMITS.maxTextFileSizeBytes) {
      return {
        status: "failed",
        message: `The resulting file exceeds the ${WORKSPACE_LIMITS.maxTextFileSizeBytes}-byte text limit.`,
      };
    }
    const diff = buildUnifiedDiff(
      resolved.workspaceRelativePath,
      originalText,
      replacementOutcome.text,
    );
    if (diff.status === "too_large") {
      return { status: "failed", message: diff.message };
    }
    const afterSha256 = hashBuffer(newBytes);
    const filePreview = {
      path: resolved.workspaceRelativePath,
      operation: "update" as const,
      beforeSha256: currentHash,
      afterSha256,
      addedLines: diff.diff.addedLines,
      removedLines: diff.diff.removedLines,
      unifiedDiff: diff.diff.unifiedDiff,
    };
    const preview: ChangePreview = {
      files: [filePreview],
      totalAddedLines: diff.diff.addedLines,
      totalRemovedLines: diff.diff.removedLines,
      truncated: false,
    };
    const mutation = createPreparedMutation();
    payloads.set(mutation, {
      workspaceRelativePath: resolved.workspaceRelativePath,
      absolutePath: resolved.absolutePath,
      expectedSha256: parsed.value.expectedSha256,
      originalBytes: readResult.bytes,
      newContent: newBytes,
      afterSha256,
      addedLines: diff.diff.addedLines,
      removedLines: diff.diff.removedLines,
    });
    return { status: "ready", mutation, preview };
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
    let tempPath: string | undefined;
    try {
      const conflict = await revalidateTarget(payload);
      if (conflict !== null) {
        return { status: "conflict", message: conflict };
      }
      if (context.signal?.aborted) {
        return { status: "cancelled", message: "The mutation was cancelled before writing." };
      }
      tempPath = createMutationTempPath(dirname(payload.absolutePath));
      try {
        const handle = await open(tempPath, "wx");
        try {
          await handle.writeFile(payload.newContent);
          await handle.sync();
        } finally {
          await handle.close();
        }
      } catch (error: unknown) {
        await removeMutationTemp(tempPath);
        return {
          status: "failed",
          message: `Cannot stage the replacement: ${describeError(error)}`,
        };
      }
      if (context.signal?.aborted) {
        await removeMutationTemp(tempPath);
        return { status: "cancelled", message: "The mutation was cancelled during staging." };
      }
      const finalConflict = await revalidateTarget(payload);
      if (finalConflict !== null) {
        await removeMutationTemp(tempPath);
        return { status: "conflict", message: finalConflict };
      }
      const commitError = await commitReplacement(payload, tempPath);
      tempPath = undefined;
      if (commitError !== null) {
        return { status: "failed", message: commitError };
      }
      const verification = await verifyEditedFile(payload);
      if (verification !== null) {
        return { status: "failed", message: verification };
      }
      return {
        status: "success",
        output: {
          path: payload.workspaceRelativePath,
          operation: "update",
          beforeSha256: payload.expectedSha256,
          afterSha256: payload.afterSha256,
          addedLines: payload.addedLines,
          removedLines: payload.removedLines,
        },
        summary: `Applied +${payload.addedLines} -${payload.removedLines}`,
      };
    } finally {
      if (tempPath !== undefined) {
        await removeMutationTemp(tempPath).catch(() => {});
      }
      release();
    }
  }

  async function revalidateTarget(payload: EditPayload): Promise<string | null> {
    const revalidated = await resolveMutationTarget(workspaceRoot, payload.workspaceRelativePath);
    if (revalidated.status !== "resolved") {
      return `The target changed since the proposal: ${revalidated.message}`;
    }
    const readResult = await readBoundedFile(revalidated.absolutePath);
    if (readResult.status === "too_large") {
      return `The target changed since the proposal: ${readResult.message}`;
    }
    if (hashBuffer(readResult.bytes) !== payload.expectedSha256) {
      return "The file changed since the proposal was approved; reread the file.";
    }
    return null;
  }

  async function commitReplacement(payload: EditPayload, tempPath: string): Promise<string | null> {
    if (process.platform === "win32") {
      try {
        await rename(tempPath, payload.absolutePath);
        return null;
      } catch (firstError: unknown) {
        try {
          await unlink(payload.absolutePath);
        } catch (unlinkError: unknown) {
          return `The replacement could not be committed: ${describeError(firstError)}; recovery state: ${describeError(unlinkError)}`;
        }
        try {
          await rename(tempPath, payload.absolutePath);
          return null;
        } catch (secondError: unknown) {
          return `The replacement could not be committed on Windows: ${describeError(secondError)}; the original may need restoration.`;
        }
      }
    }
    try {
      await rename(tempPath, payload.absolutePath);
      return null;
    } catch (error: unknown) {
      return `The replacement could not be committed: ${describeError(error)}`;
    }
  }

  async function verifyEditedFile(payload: EditPayload): Promise<string | null> {
    let bytes: Buffer;
    try {
      bytes = await readFile(payload.absolutePath);
    } catch (error: unknown) {
      return `Post-write verification could not read the file: ${describeError(error)}`;
    }
    if (bytes.length !== payload.newContent.length || !bytes.equals(payload.newContent)) {
      return "Post-write verification failed: the written bytes do not match the proposal.";
    }
    if (hashBuffer(bytes) !== payload.afterSha256) {
      return "Post-write verification failed: the written hash does not match the proposal.";
    }
    return null;
  }

  return {
    kind: "prepared_mutation",
    definition: {
      name: "workspace.edit_file",
      description:
        "Apply a bounded sequence of exact text replacements to one existing UTF-8 text file.",
      inputSchema: {
        type: "object",
        properties: {
          path: { type: "string", description: "File path relative to the workspace root." },
          expectedSha256: {
            type: "string",
            description: "Complete-file SHA-256 of the current file, from workspace.read.",
          },
          replacements: {
            type: "array",
            description:
              "Exact text replacements applied sequentially; each oldText must match exactly once.",
            items: {
              type: "object",
              properties: {
                oldText: { type: "string" },
                newText: { type: "string" },
              },
              required: ["oldText", "newText"],
            },
          },
        },
        required: ["path", "expectedSha256", "replacements"],
        additionalProperties: false,
      },
    },
    capability: "workspace.write",
    prepare,
    apply,
  };
}

type ReplacementOutcome =
  | { readonly status: "applied"; readonly text: string }
  | { readonly status: "conflict"; readonly message: string };

function applyReplacements(text: string, replacements: readonly Replacement[]): ReplacementOutcome {
  let current = text;
  for (const replacement of replacements) {
    const occurrences = countOccurrences(current, replacement.oldText);
    if (occurrences === 0) {
      return {
        status: "conflict",
        message: `oldText "${truncate(replacement.oldText)}" was not found in the file; reread the file.`,
      };
    }
    if (occurrences > 1) {
      return {
        status: "conflict",
        message: `oldText "${truncate(replacement.oldText)}" matched ${occurrences} times; the replacement is ambiguous.`,
      };
    }
    current = current.replace(replacement.oldText, replacement.newText);
  }
  return { status: "applied", text: current };
}

function countOccurrences(text: string, needle: string): number {
  let count = 0;
  let index = text.indexOf(needle);
  while (index >= 0) {
    count += 1;
    index = text.indexOf(needle, index + needle.length);
  }
  return count;
}

async function readBoundedFile(
  absolutePath: string,
): Promise<{ status: "ok"; bytes: Buffer } | { status: "too_large"; message: string }> {
  let bytes: Buffer;
  try {
    bytes = await readFile(absolutePath);
  } catch (error: unknown) {
    return { status: "too_large", message: `Cannot read the file: ${describeError(error)}` };
  }
  if (bytes.length > WORKSPACE_LIMITS.maxTextFileSizeBytes) {
    return {
      status: "too_large",
      message: `File is too large (limit ${WORKSPACE_LIMITS.maxTextFileSizeBytes} bytes).`,
    };
  }
  return { status: "ok", bytes };
}

function truncate(text: string): string {
  const limit = 60;
  return text.length > limit ? `${text.slice(0, limit)}...` : text;
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
