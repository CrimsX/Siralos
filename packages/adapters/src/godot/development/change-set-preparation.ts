import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import type { ChangePreview, FileChangePreview, PreparedChangeSetFile } from "@solaris/core";
import {
  countChangeSetResultBytes,
  DEVELOPMENT_LIMITS,
  computeChangeSetDigest,
  validateChangeSetRequest,
  type ChangeSetOperation,
} from "@solaris/core";
import {
  resolveCreateTarget,
  resolveMutationTarget,
} from "../../tools/workspace/mutations/mutation-paths.js";
import { buildUnifiedDiff } from "../../tools/workspace/mutations/diff.js";
import { decodeUtf8, looksBinary } from "../../tools/workspace/text.js";
import { describeFsError } from "../../tools/workspace/workspace-path.js";

/**
 * Read-only change-set preparation (§20).
 *
 * Preparation acquires no write lock, validates every path and protected
 * path, reads every current file, validates the exact expected SHA-256
 * values, produces every resulting file content in memory, validates the
 * final size limits, generates complete deterministic diffs, calculates
 * after-hashes, and freezes the immutable change-set digest. Nothing is
 * written, approved, or checkpointed here; the workflow and the
 * change-set tool consume this module, and every execution gate fails
 * closed as `unavailable` on this stage before any write.
 */

export type ChangeSetPreparationResult =
  | {
      readonly status: "ready";
      readonly files: readonly PreparedChangeSetFile[];
      readonly preview: ChangePreview;
      /** SHA-256 over the immutable prepared change set; binds approval. */
      readonly digest: string;
      /** Total resulting bytes (limit check). */
      readonly resultBytes: number;
      /** Total complete-diff bytes (limit check). */
      readonly diffBytes: number;
    }
  | {
      readonly status: "invalid_input" | "conflict" | "changeset_too_large" | "failed";
      readonly message: string;
    };

export interface ChangeSetPreparationDependencies {
  readonly workspaceRoot: string;
  readonly platform?: NodeJS.Platform;
}

export async function prepareChangeSet(
  input: unknown,
  dependencies: ChangeSetPreparationDependencies,
  signal?: AbortSignal,
): Promise<ChangeSetPreparationResult> {
  if (signal?.aborted) {
    return { status: "failed", message: "The change-set preparation was cancelled." };
  }
  const validated = validateChangeSetRequest(input);
  if (!validated.ok) {
    return { status: "invalid_input", message: validated.message };
  }
  const files: PreparedChangeSetFile[] = [];
  const previewFiles: FileChangePreview[] = [];
  let diffBytes = 0;
  for (const change of validated.request.changes) {
    if (signal?.aborted) {
      return { status: "failed", message: "The change-set preparation was cancelled." };
    }
    const prepared = await prepareOneFile(change, dependencies);
    if (prepared.status !== "ready") {
      return prepared;
    }
    const { file, previewFile, diffBytes: fileDiffBytes } = prepared;
    files.push(file);
    previewFiles.push(previewFile);
    diffBytes += fileDiffBytes;
    if (diffBytes > DEVELOPMENT_LIMITS.maxChangeSetDiffBytes) {
      return {
        status: "changeset_too_large",
        message:
          "The complete change-set diff exceeds the approval limit; the provider must split the change set.",
      };
    }
  }
  const resultBytes = countChangeSetResultBytes(files);
  if (resultBytes > DEVELOPMENT_LIMITS.maxChangeSetResultBytes) {
    return {
      status: "changeset_too_large",
      message:
        "The change set's resulting bytes exceed the limit; the provider must split the change set.",
    };
  }
  const digest = computeChangeSetDigest({
    changes: files.map((file) => ({
      operation: file.operation,
      path: file.path,
      beforeSha256: file.beforeSha256,
      afterSha256: file.afterSha256,
    })),
  });
  const preview: ChangePreview = {
    files: previewFiles,
    totalAddedLines: previewFiles.reduce((total, file) => total + file.addedLines, 0),
    totalRemovedLines: previewFiles.reduce((total, file) => total + file.removedLines, 0),
    truncated: false,
  };
  return { status: "ready", files, preview, digest, resultBytes, diffBytes };
}

type OneFilePreparationResult =
  | {
      readonly status: "ready";
      readonly file: PreparedChangeSetFile;
      readonly previewFile: FileChangePreview;
      readonly diffBytes: number;
    }
  | {
      readonly status: "invalid_input" | "conflict" | "changeset_too_large" | "failed";
      readonly message: string;
    };

async function prepareOneFile(
  change: ChangeSetOperation,
  dependencies: ChangeSetPreparationDependencies,
): Promise<OneFilePreparationResult> {
  if (change.operation === "create") {
    const resolved = await resolveCreateTarget(dependencies.workspaceRoot, change.path);
    if (resolved.status === "exists") {
      return {
        status: "conflict",
        message: `The create target "${change.path}" already exists; reread the workspace.`,
      };
    }
    if (resolved.status !== "resolved") {
      return { status: "invalid_input", message: resolved.message };
    }
    if (looksBinary(new TextEncoder().encode(change.content))) {
      return {
        status: "invalid_input",
        message: `The create for "${change.path}" contains binary bytes; only UTF-8 text files are supported.`,
      };
    }
    const afterSha256 = hashText(change.content);
    const unifiedDiff = buildUnifiedDiff(resolved.workspaceRelativePath, "", change.content);
    if (unifiedDiff.status !== "ready") {
      return {
        status: "changeset_too_large",
        message: `The create for "${change.path}" cannot be previewed: ${unifiedDiff.message}`,
      };
    }
    const file: PreparedChangeSetFile = {
      path: resolved.workspaceRelativePath,
      operation: "create",
      expectedSha256: null,
      content: change.content,
      beforeSha256: null,
      afterSha256,
      unifiedDiff: unifiedDiff.diff.unifiedDiff,
      addedLines: unifiedDiff.diff.addedLines,
      removedLines: 0,
    };
    return {
      status: "ready",
      file,
      previewFile: {
        path: file.path,
        operation: "create",
        beforeSha256: null,
        afterSha256,
        addedLines: unifiedDiff.diff.addedLines,
        removedLines: 0,
        unifiedDiff: unifiedDiff.diff.unifiedDiff,
      },
      diffBytes: utf8ByteLength(unifiedDiff.diff.unifiedDiff),
    };
  }
  const resolved = await resolveMutationTarget(dependencies.workspaceRoot, change.path);
  if (resolved.status !== "resolved") {
    return { status: "invalid_input", message: resolved.message };
  }
  let bytes: Buffer;
  try {
    bytes = await readFile(resolved.absolutePath);
  } catch (error: unknown) {
    return {
      status: "failed",
      message: `"${change.path}" could not be read: ${describeFsError(error)}`,
    };
  }
  if (bytes.byteLength > DEVELOPMENT_LIMITS.maxTextFileBytes) {
    return {
      status: "invalid_input",
      message: `"${change.path}" exceeds the text-file size limit and cannot be changed.`,
    };
  }
  if (looksBinary(bytes)) {
    return {
      status: "invalid_input",
      message: `"${change.path}" is not a UTF-8 text file and cannot be changed.`,
    };
  }
  const beforeText = decodeUtf8(bytes);
  if (beforeText === null) {
    return {
      status: "invalid_input",
      message: `"${change.path}" is not valid UTF-8 and cannot be changed.`,
    };
  }
  const beforeSha256 = hashBuffer(bytes);
  if (change.operation === "delete") {
    if (beforeSha256 !== change.expectedSha256) {
      return {
        status: "conflict",
        message: `"${change.path}" changed since it was read; reread the workspace before deleting.`,
      };
    }
    const file: PreparedChangeSetFile = {
      path: resolved.workspaceRelativePath,
      operation: "delete",
      expectedSha256: change.expectedSha256,
      content: null,
      beforeSha256,
      afterSha256: null,
      unifiedDiff: "",
      addedLines: 0,
      removedLines: countLines(beforeText),
    };
    return {
      status: "ready",
      file,
      previewFile: {
        path: file.path,
        operation: "delete",
        beforeSha256,
        afterSha256: null,
        addedLines: 0,
        removedLines: countLines(beforeText),
        unifiedDiff: "",
      },
      diffBytes: 0,
    };
  }
  if (beforeSha256 !== change.expectedSha256) {
    return {
      status: "conflict",
      message: `"${change.path}" changed since it was read; reread the workspace before editing.`,
    };
  }
  const applied = applyReplacements(beforeText, change.replacements);
  if (!applied.ok) {
    return { status: "invalid_input", message: applied.message };
  }
  const afterText = applied.text;
  if (utf8ByteLength(afterText) > DEVELOPMENT_LIMITS.maxChangeSetResultBytes) {
    return {
      status: "changeset_too_large",
      message: `"${change.path}" would exceed the change-set size limit.`,
    };
  }
  const afterSha256 = hashText(afterText);
  const unifiedDiff = buildUnifiedDiff(resolved.workspaceRelativePath, beforeText, afterText);
  if (unifiedDiff.status !== "ready") {
    return {
      status: "changeset_too_large",
      message: `The edit for "${change.path}" cannot be previewed: ${unifiedDiff.message}`,
    };
  }
  const file: PreparedChangeSetFile = {
    path: resolved.workspaceRelativePath,
    operation: "update",
    expectedSha256: change.expectedSha256,
    content: afterText,
    beforeSha256,
    afterSha256,
    unifiedDiff: unifiedDiff.diff.unifiedDiff,
    addedLines: unifiedDiff.diff.addedLines,
    removedLines: unifiedDiff.diff.removedLines,
  };
  return {
    status: "ready",
    file,
    previewFile: {
      path: file.path,
      operation: "update",
      beforeSha256,
      afterSha256,
      addedLines: unifiedDiff.diff.addedLines,
      removedLines: unifiedDiff.diff.removedLines,
      unifiedDiff: unifiedDiff.diff.unifiedDiff,
    },
    diffBytes: utf8ByteLength(unifiedDiff.diff.unifiedDiff),
  };
}

/**
 * Applies exact text replacements in order. Every oldText must appear at
 * least once in the current text; all non-overlapping occurrences are
 * replaced. Overlapping replacement windows are rejected deterministically
 * rather than guessed.
 */
export function applyReplacements(
  text: string,
  replacements: readonly { readonly oldText: string; readonly newText: string }[],
): { readonly ok: true; readonly text: string } | { readonly ok: false; readonly message: string } {
  let current = text;
  for (const replacement of replacements) {
    if (current.includes(replacement.oldText)) {
      current = current.split(replacement.oldText).join(replacement.newText);
      continue;
    }
    return {
      ok: false,
      message: `The replacement text "${replacement.oldText}" does not occur in the current file; the change was not prepared.`,
    };
  }
  return { ok: true, text: current };
}

export function hashText(text: string): string {
  return hashBuffer(new TextEncoder().encode(text));
}

export function hashBuffer(buffer: Uint8Array): string {
  return createHash("sha256").update(buffer).digest("hex");
}

function countLines(text: string): number {
  if (text.length === 0) {
    return 0;
  }
  const normalized = text.endsWith("\n") ? text.slice(0, -1) : text;
  return normalized.split("\n").length;
}

function utf8ByteLength(text: string): number {
  return new TextEncoder().encode(text).length;
}
