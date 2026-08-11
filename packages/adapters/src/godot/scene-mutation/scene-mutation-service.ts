import { createHash } from "node:crypto";
import { stat } from "node:fs/promises";
import {
  GODOT_SCENE_LIMITS,
  applyResourceOperations,
  applySceneOperations,
  createPreparedGodotMutation,
  expectedSemanticEffect,
  parseGodotResource,
  parseGodotScene,
  serializeResource,
  serializeScene,
  validateMutationOperations,
  verifyResourceSemanticEffect,
  verifySceneSemanticEffect,
  type CheckpointStore,
  type ChangeSetApplyRequest,
  type ChangeSetFilePrimitives,
  type GodotTextDocument,
  type MutationOperation,
  type PreparedGodotMutation,
  type SemanticVerification,
  type WorkspaceRevisionHandle,
  type WorkspaceRevisionRegistry,
} from "@solaris/core";
import { readFileBounded } from "../../fs/file-read.js";
import { resolveWorkspacePath } from "../../tools/workspace/workspace-path.js";
import { decodeUtf8, looksBinary } from "../../tools/workspace/text.js";
import { buildUnifiedDiff, countLines } from "../../tools/workspace/mutations/diff.js";
import { applyChangeSetProtocol } from "../development/change-set-executor.js";

/**
 * Godot scene/resource mutation service (Stage 3 milestone 10, ADR 0026).
 *
 * The application-owned owner of approved native mutation: prepare
 * (inspect exact revision -> validate operations -> structural apply ->
 * deterministic serialization -> complete preview -> fingerprint) and
 * apply orchestration (approval binding -> revision revalidation ->
 * checkpoint -> structural apply -> reparse -> semantic verification ->
 * impact context). Native mutations never route through generic text
 * editing: the applied text is serializer-generated from a prepared
 * mutation, and success is never a successful write alone.
 *
 * The apply gate follows the repository's change-set applier: production
 * primitives fail closed (`canApplyIdentityBound: false`), tests inject
 * in-memory primitives.
 */

export interface SceneMutationServiceDependencies {
  readonly workspaceRoot: string;
  readonly revisions: WorkspaceRevisionRegistry;
  readonly store: CheckpointStore;
  readonly lock: { acquire(signal?: AbortSignal): Promise<() => void> };
  /** True only when the platform can mechanically bind every write. */
  readonly canApplyIdentityBound: boolean;
  readonly primitives: ChangeSetFilePrimitives;
  /** Bounded read limit; defaults to GODOT_SCENE_LIMITS.maxDocumentBytes. */
  readonly maxDocumentBytes?: number;
}

export type MutationPrepareResult =
  | { readonly status: "ready"; readonly message: null; readonly prepared: PreparedGodotMutation }
  | {
      readonly status: "not_found" | "unreadable" | "unsupported" | "denied" | "failed";
      readonly message: string;
      readonly prepared: null;
    };

export type MutationApplyResult =
  | {
      readonly status: "applied";
      readonly message: null;
      readonly revision: WorkspaceRevisionHandle;
      readonly verification: SemanticVerification;
      readonly checkpointIds: readonly string[];
      readonly impact?: { readonly taskId: string; readonly completeness: string } | null;
    }
  | {
      readonly status: "conflict" | "verification_failed" | "unavailable" | "cancelled" | "failed";
      readonly message: string;
      readonly verification: SemanticVerification | null;
      readonly checkpointIds: readonly string[];
    };

interface ReadOutcome {
  readonly status: "ok" | "not_found" | "unreadable" | "denied" | "failed";
  readonly message: string | null;
  readonly relativePath: string | null;
  readonly content: string | null;
  readonly sha256: string | null;
  readonly revision: WorkspaceRevisionHandle | null;
}

export function createGodotSceneMutationService(dependencies: SceneMutationServiceDependencies): {
  prepareSceneChange(input: {
    readonly path: string;
    readonly operations: readonly MutationOperation[];
  }): Promise<MutationPrepareResult>;
  prepareResourceChange(input: {
    readonly path: string;
    readonly operations: readonly MutationOperation[];
  }): Promise<MutationPrepareResult>;
  applyPrepared(input: {
    readonly prepared: PreparedGodotMutation;
    readonly approvedDigest: string;
    readonly signal?: AbortSignal;
    readonly impactTaskId?: string;
    readonly impactTaskContractRevision?: number;
  }): Promise<MutationApplyResult>;
} {
  const maxDocumentBytes = dependencies.maxDocumentBytes ?? GODOT_SCENE_LIMITS.maxDocumentBytes;

  async function readDocument(path: string): Promise<ReadOutcome> {
    const resolved = await resolveWorkspacePath(dependencies.workspaceRoot, path);
    if (resolved.status === "rejected") {
      return {
        status: "denied",
        message: resolved.message,
        relativePath: null,
        content: null,
        sha256: null,
        revision: null,
      };
    }
    let stats;
    try {
      stats = await stat(resolved.absolutePath);
    } catch {
      return {
        status: "not_found",
        message: `Cannot read ${path}: file does not exist.`,
        relativePath: null,
        content: null,
        sha256: null,
        revision: null,
      };
    }
    if (!stats.isFile() || stats.size > maxDocumentBytes) {
      return {
        status: "unreadable",
        message: `Cannot read ${path}: not a regular file or exceeds ${maxDocumentBytes} bytes.`,
        relativePath: null,
        content: null,
        sha256: null,
        revision: null,
      };
    }
    const buffer = await readFileBounded(resolved.absolutePath, maxDocumentBytes);
    if (buffer === null || looksBinary(buffer)) {
      return {
        status: "unreadable",
        message: `Cannot read ${path}: missing, not a regular file, binary, or too large.`,
        relativePath: null,
        content: null,
        sha256: null,
        revision: null,
      };
    }
    const text = decodeUtf8(buffer);
    if (text === null) {
      return {
        status: "unreadable",
        message: `Cannot read ${path}: not valid UTF-8.`,
        relativePath: null,
        content: null,
        sha256: null,
        revision: null,
      };
    }
    const sha256 = createHash("sha256").update(buffer).digest("hex");
    const revision = dependencies.revisions.issue(resolved.workspaceRelativePath, sha256);
    dependencies.revisions.observeRead(resolved.workspaceRelativePath, revision, "exact");
    return {
      status: "ok",
      message: null,
      relativePath: resolved.workspaceRelativePath,
      content: text,
      sha256,
      revision,
    };
  }

  async function prepare(
    path: string,
    operations: readonly MutationOperation[],
    kind: "scene" | "resource",
  ): Promise<MutationPrepareResult> {
    const read = await readDocument(path);
    if (
      read.status !== "ok" ||
      read.content === null ||
      read.sha256 === null ||
      read.revision === null
    ) {
      return {
        status: read.status === "ok" ? "failed" : read.status,
        message: read.message ?? `Cannot prepare ${path}.`,
        prepared: null,
      };
    }
    if (read.relativePath === null) {
      return { status: "failed", message: "Cannot prepare: unresolved path.", prepared: null };
    }
    try {
      return prepareValidated(read, operations, kind);
    } catch (error: unknown) {
      return {
        status: "failed",
        message: error instanceof Error ? error.message : `Cannot prepare ${path}.`,
        prepared: null,
      };
    }
  }

  function prepareValidated(
    read: ReadOutcome,
    operations: readonly MutationOperation[],
    kind: "scene" | "resource",
  ): MutationPrepareResult {
    if (
      read.content === null ||
      read.sha256 === null ||
      read.revision === null ||
      read.relativePath === null
    ) {
      return { status: "failed", message: "Cannot prepare: incomplete read.", prepared: null };
    }
    const validated = validateMutationOperations(operations);
    let parsed: GodotTextDocument<unknown>;
    let serializedAfter: string;
    let reparsed: GodotTextDocument<unknown>;
    if (kind === "scene") {
      parsed = parseGodotScene(read.content, read.relativePath, { revision: read.revision });
      if (parsed.document === null) {
        return {
          status: "failed",
          message: "Cannot prepare: the current document does not parse as a scene.",
          prepared: null,
        };
      }
      const after = applySceneOperations(parsed.document as never, validated);
      serializedAfter = serializeScene(after);
      reparsed = parseGodotScene(serializedAfter, read.relativePath, { revision: read.revision });
    } else {
      parsed = parseGodotResource(read.content, read.relativePath, { revision: read.revision });
      if (parsed.document === null) {
        return {
          status: "failed",
          message: "Cannot prepare: the current document does not parse as a resource.",
          prepared: null,
        };
      }
      const after = applyResourceOperations(parsed.document as never, validated);
      serializedAfter = serializeResource(after);
      reparsed = parseGodotResource(serializedAfter, read.relativePath, {
        revision: read.revision,
      });
    }
    // The serialized output must itself parse: a preview that cannot
    // reparse is never approvable.
    if (reparsed.document === null) {
      return {
        status: "failed",
        message:
          "Cannot prepare: the serialized mutation output does not reparse as valid Godot syntax.",
        prepared: null,
      };
    }
    const beforeLines = countLines(read.content);
    const afterLines = countLines(serializedAfter);
    const diffResult = buildUnifiedDiff(read.relativePath, read.content, serializedAfter);
    if (diffResult.status === "too_large") {
      // Approval must never rely on a truncated diff: refuse.
      return { status: "failed", message: diffResult.message, prepared: null };
    }
    const diff = diffResult.diff.unifiedDiff;
    const addedLines = diffResult.diff.addedLines;
    const removedLines = diffResult.diff.removedLines;
    const structuralSummary = validated
      .map((operation) => summarizeOperation(operation))
      .join("\n");
    const prepared = createPreparedGodotMutation({
      targetPath: read.relativePath,
      sourceRevision: read.revision,
      sourceSha256: read.sha256,
      kind,
      operations: validated,
      expectedSemanticEffect: expectedSemanticEffect(validated),
      preview: { structuralSummary, diff },
      serializedAfter,
      addedLines,
      removedLines,
    });
    void beforeLines;
    void afterLines;
    return { status: "ready", message: null, prepared };
  }

  async function applyPrepared(input: {
    readonly prepared: PreparedGodotMutation;
    readonly approvedDigest: string;
    readonly signal?: AbortSignal;
    readonly impactTaskId?: string;
    readonly impactTaskContractRevision?: number;
  }): Promise<MutationApplyResult> {
    const prepared = input.prepared;
    // Approval binding: the digest must match the exact prepared mutation.
    if (input.approvedDigest !== prepared.fingerprint) {
      return {
        status: "conflict",
        message: "The approval does not match the prepared mutation; a new approval is required.",
        verification: null,
        checkpointIds: [],
      };
    }
    if (input.signal?.aborted) {
      return {
        status: "cancelled",
        message: "The mutation was cancelled.",
        verification: null,
        checkpointIds: [],
      };
    }
    if (!dependencies.canApplyIdentityBound) {
      return {
        status: "unavailable",
        message:
          "Scene/resource mutation cannot be applied on this platform: the identity-bound apply gate is unavailable.",
        verification: null,
        checkpointIds: [],
      };
    }
    const request: ChangeSetApplyRequest = {
      changeSetId: `native-${prepared.fingerprint.slice(0, 12)}`,
      toolName:
        prepared.kind === "scene" ? "godot.prepare_scene_change" : "godot.prepare_resource_change",
      files: [
        {
          path: prepared.targetPath,
          operation: "update",
          expectedSha256: prepared.sourceSha256,
          content: prepared.serializedAfter,
          beforeSha256: prepared.sourceSha256,
          afterSha256: createHash("sha256").update(prepared.serializedAfter, "utf8").digest("hex"),
          addedLines: prepared.addedLines,
          removedLines: prepared.removedLines,
        },
      ],
      ...(input.signal === undefined ? {} : { signal: input.signal }),
    };
    const outcome = await applyChangeSetProtocol(request, dependencies.primitives, {
      store: dependencies.store,
      lock: dependencies.lock,
      toolName:
        prepared.kind === "scene" ? "godot.prepare_scene_change" : "godot.prepare_resource_change",
      canApplyIdentityBound: true,
      revisions: dependencies.revisions,
    });
    if (outcome.status !== "applied") {
      return {
        status:
          outcome.status === "conflict"
            ? "conflict"
            : outcome.status === "cancelled"
              ? "cancelled"
              : "failed",
        message: outcome.message,
        verification: null,
        checkpointIds: "checkpointIds" in outcome ? outcome.checkpointIds : [],
      };
    }
    // Reparse the new exact revision and verify the intended semantics.
    const read = await readDocument(prepared.targetPath);
    if (read.status !== "ok" || read.content === null) {
      return {
        status: "verification_failed",
        message: `Applied, but the new revision could not be read for verification: ${read.message ?? "unknown"}.`,
        verification: null,
        checkpointIds: outcome.checkpointIds,
      };
    }
    const parsed =
      prepared.kind === "scene"
        ? parseGodotScene(read.content, prepared.targetPath, { revision: read.revision })
        : parseGodotResource(read.content, prepared.targetPath, { revision: read.revision });
    if (parsed.document === null) {
      return {
        status: "verification_failed",
        message: "Applied, but the new revision does not parse; the mutation is not verified.",
        verification: null,
        checkpointIds: outcome.checkpointIds,
      };
    }
    const verification =
      prepared.kind === "scene"
        ? verifySceneSemanticEffect(parsed.document as never, prepared.expectedSemanticEffect)
        : verifyResourceSemanticEffect(parsed.document as never, prepared.expectedSemanticEffect);
    if (verification.status !== "verified") {
      return {
        status: "verification_failed",
        message: `Applied, but semantic verification ${verification.status}: ${verification.checks
          .filter((check) => check.status !== "verified")
          .map((check) => check.detail)
          .join("; ")}`,
        verification,
        checkpointIds: outcome.checkpointIds,
      };
    }
    const revision = read.revision ?? prepared.sourceRevision;
    return {
      status: "applied",
      message: null,
      revision,
      verification,
      checkpointIds: outcome.checkpointIds,
    };
  }

  return {
    prepareSceneChange: (input) => prepare(input.path, input.operations, "scene"),
    prepareResourceChange: (input) => prepare(input.path, input.operations, "resource"),
    applyPrepared,
  };
}

function summarizeOperation(operation: MutationOperation): string {
  switch (operation.op) {
    case "set_property":
      return `set ${"nodePath" in operation ? operation.nodePath : "(resource)"}.${operation.property}`;
    case "remove_property":
      return `remove ${"nodePath" in operation ? operation.nodePath : "(resource)"}.${operation.property}`;
    case "add_node":
      return `add node ${operation.name} (${operation.type})`;
    case "remove_node":
      return `remove node ${operation.nodePath}`;
    case "set_script_attachment":
      return `set script attachment on ${operation.nodePath}${operation.extResourceId === null ? " (remove)" : ` -> ${operation.extResourceId}`}`;
    case "change_resource_reference":
      return `change resource reference ${operation.resourceId}`;
    case "add_signal_connection":
      return `add connection ${operation.signal} ${operation.from} -> ${operation.to}.${operation.method}`;
    case "remove_signal_connection":
      return `remove connection ${operation.signal} ${operation.from} -> ${operation.to}.${operation.method}`;
    case "create_subresource":
      return `create subresource ${operation.id ?? "(generated)"} (${operation.type})`;
    case "update_subresource":
      return `update subresource ${operation.id}`;
    case "remove_subresource":
      return `remove subresource ${operation.id}`;
  }
}
