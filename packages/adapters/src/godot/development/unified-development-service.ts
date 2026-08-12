import { createHash } from "node:crypto";
import { readFileBounded } from "../../fs/file-read.js";
import { resolveWorkspacePath } from "../../tools/workspace/workspace-path.js";
import { decodeUtf8, looksBinary } from "../../tools/workspace/text.js";
import type {
  BlockedDisposition,
  ChangePreview,
  ChangeSetApplyRequest,
  ChangeSetFilePrimitives,
  CheckpointStore,
  DevelopmentEvent,
  GDScriptLanguageService,
  GodotDiagnostics,
  MutationOperation,
  PreparedChangeSetFile,
  PreparedGodotMutation,
  ReviewContextManifest,
  SemanticVerification,
  UnifiedChangeSet,
  WorkspaceRevisionRegistry,
} from "@solaris/core";
import {
  approveUnifiedTarget,
  classifyDevelopmentSurface,
  createBlockedDisposition,
  createUnifiedChangeSet,
  deriveUnifiedApplyOrder,
  deriveUnifiedOrderEdges,
  parseGodotResource,
  parseGodotScene,
  unifiedChangeSetReadyToApply,
  validateChangeSetRequest,
  verifyCrossSurfaceConsistency,
  verifyResourceSemanticEffect,
  verifySceneSemanticEffect,
} from "@solaris/core";
import { prepareChangeSet } from "./change-set-preparation.js";
import {
  applyChangeSetProtocol,
  CHANGE_SET_EXECUTION_UNAVAILABLE_MESSAGE,
} from "./change-set-executor.js";
import type { MutationPrepareResult } from "../scene-mutation/scene-mutation-service.js";

/**
 * Unified development service (Stage 3 milestone 11, ADR 0027).
 *
 * The application-owned orchestrator for mixed script/native change
 * sets: preparation composes exact-text targets and prepared native
 * scene/resource mutations into one bounded unified change set with a
 * derived apply order and a combined digest; application revalidates
 * EVERY target's exact pre-state before any write, applies the whole
 * batch through one checkpoint-then-apply protocol (one lock, per-file
 * checkpoints, sequential hash-verified application), then verifies
 * per surface (GDScript parser/fresh-LSP evidence; native reparse and
 * semantic-effect verification), checks cross-surface consistency, and
 * derives post-change impact. A mutation step is not successful until
 * its required verification passes.
 *
 * The identity-bound apply gate follows the repository posture:
 * production primitives fail closed (`canApplyIdentityBound: false`)
 * before any lock, checkpoint, or write; tests inject in-memory
 * primitives.
 */

export interface UnifiedDevelopmentServiceDependencies {
  readonly workspaceRoot: string;
  readonly store: CheckpointStore;
  readonly lock: { acquire(signal?: AbortSignal): Promise<() => void> };
  readonly revisions: WorkspaceRevisionRegistry;
  /** True only when the platform can mechanically bind every write. */
  readonly canApplyIdentityBound: boolean;
  readonly primitives: ChangeSetFilePrimitives;
  /** Host-assignable event listener slot (reassignment allowed). */
  readonly onEvent?: ((event: DevelopmentEvent) => void) | undefined;
  /** Native prepare surface (Stage 3 milestone 10 service). */
  readonly native: {
    prepareSceneChange(input: {
      readonly path: string;
      readonly operations: readonly MutationOperation[];
    }): Promise<MutationPrepareResult>;
    prepareResourceChange(input: {
      readonly path: string;
      readonly operations: readonly MutationOperation[];
    }): Promise<MutationPrepareResult>;
  };
  /** GDScript check-only gate; absent/unavailable fails closed honestly. */
  readonly diagnostics?: GodotDiagnostics | null;
  /** Fresh LSP session source; absent/unavailable fails closed honestly. */
  readonly language?: GDScriptLanguageService | null;
  /** Impact derivation over the applied change set (read-only). */
  readonly impact?:
    | ((input: {
        readonly taskId: string;
        readonly primaryChanges: readonly { readonly path: string; readonly operation: string }[];
      }) => Promise<ReviewContextManifest | null>)
    | null;
  readonly now?: () => number;
}

export interface UnifiedPrepareTargetInput {
  readonly kind: "text" | "scene" | "resource";
  readonly path?: string;
  readonly changes?: unknown;
  readonly operations?: readonly MutationOperation[];
}

export type UnifiedPrepareResult =
  | {
      readonly status: "ready";
      readonly changeSet: UnifiedChangeSet;
      readonly preview: ChangePreview;
    }
  | {
      readonly status:
        | "invalid_input"
        | "conflict"
        | "changeset_too_large"
        | "stale_revision"
        | "unavailable"
        | "cancelled"
        | "failed";
      readonly message: string;
      readonly blocked?: BlockedDisposition;
    };

export type UnifiedApplyResult =
  | {
      readonly status: "applied";
      readonly changeSetId: string;
      readonly revisions: readonly { readonly path: string; readonly revision: string }[];
      readonly checkpointIds: readonly string[];
      readonly nativeVerification: readonly {
        readonly path: string;
        readonly status: "verified" | "failed";
        readonly detail: string | null;
      }[];
      readonly consistency: {
        readonly consistent: boolean;
        readonly concernCount: number;
      };
      readonly parser: { readonly checkedFiles: number; readonly validFiles: number } | null;
      readonly lsp: { readonly errors: number; readonly warnings: number } | null;
      readonly impact: { readonly taskId: string; readonly completeness: string } | null;
      readonly blocked?: BlockedDisposition;
    }
  | {
      readonly status:
        | "denied"
        | "conflict"
        | "cancelled"
        | "unavailable"
        | "apply_failed"
        | "verification_failed"
        | "validation_failed"
        | "failed";
      readonly message: string;
      readonly checkpointIds: readonly string[];
      readonly nativeVerification?: readonly {
        readonly path: string;
        readonly status: "verified" | "failed";
        readonly detail: string | null;
      }[];
      readonly consistency?: {
        readonly consistent: boolean;
        readonly concernCount: number;
      };
      readonly parser?: { readonly checkedFiles: number; readonly validFiles: number } | null;
      readonly lsp?: { readonly errors: number; readonly warnings: number } | null;
      readonly impact?: { readonly taskId: string; readonly completeness: string } | null;
      readonly blocked?: BlockedDisposition;
    };

export interface UnifiedDevelopmentService {
  support(): { readonly state: "available" | "unavailable"; readonly reason: string | null };
  /** Read-only: prepare the unified change set; nothing is written. */
  prepareUnified(input: {
    readonly request: string;
    readonly targets: readonly UnifiedPrepareTargetInput[];
    readonly signal?: AbortSignal;
  }): Promise<UnifiedPrepareResult>;
  /** Apply under the combined approved digest; revalidates everything. */
  applyUnified(input: {
    readonly changeSetId: string;
    readonly approvedDigest: string;
    readonly signal?: AbortSignal;
  }): Promise<UnifiedApplyResult>;
  cancel(): Promise<void>;
  close(): Promise<void>;
  onEvent?: ((event: DevelopmentEvent) => void) | undefined;
}

const UNIFIED_MAX_TARGETS = 16;
const UNIFIED_MAX_RESULT_BYTES = 4 * 1024 * 1024;
const UNIFIED_MAX_DOCUMENT_BYTES = 4 * 1024 * 1024;
const UNIFIED_TTL_MS = 10 * 60 * 1000;

function sha256Text(text: string): string {
  return createHash("sha256").update(text, "utf8").digest("hex");
}

function stripResPrefix(path: string): string {
  return path.replace(/^res:\/\//, "");
}

export function createUnifiedDevelopmentService(
  dependencies: UnifiedDevelopmentServiceDependencies,
): UnifiedDevelopmentService {
  const now = dependencies.now ?? Date.now;
  let changeSet: UnifiedChangeSet | null = null;
  /** Host-internal prepared file sets per text target (apply content). */
  let preparedFilesByTarget = new Map<string, readonly PreparedChangeSetFile[]>();
  let eventSlot: ((event: DevelopmentEvent) => void) | undefined = dependencies.onEvent;

  function emit(event: DevelopmentEvent): void {
    eventSlot?.(event);
  }

  function blocked(kind: BlockedDisposition["kind"], detail: string): BlockedDisposition {
    return createBlockedDisposition({ kind, detail });
  }

  async function readDocumentText(path: string): Promise<string | null> {
    const resolved = await resolveWorkspacePath(dependencies.workspaceRoot, path);
    if (resolved.status === "rejected") {
      return null;
    }
    const buffer = await readFileBounded(resolved.absolutePath, UNIFIED_MAX_DOCUMENT_BYTES);
    if (buffer === null || looksBinary(buffer)) {
      return null;
    }
    return decodeUtf8(buffer);
  }

  /** Resolve operation references to workspace paths (order edges). */
  async function resolveTargetReferences(input: {
    readonly kind: "scene" | "resource";
    readonly path: string;
    readonly operations: readonly MutationOperation[];
  }): Promise<readonly string[]> {
    const text = await readDocumentText(input.path);
    if (text === null) {
      return [];
    }
    const parsed =
      input.kind === "scene"
        ? parseGodotScene(text, input.path, { revision: null })
        : parseGodotResource(text, input.path, { revision: null });
    const document = parsed.document;
    if (document === null) {
      return [];
    }
    const external = new Map(
      document.externalResources
        .filter((resource) => resource.path !== undefined)
        .map((resource) => [resource.id, stripResPrefix(resource.path as string)]),
    );
    const references: string[] = [];
    for (const operation of input.operations) {
      switch (operation.op) {
        case "set_script_attachment": {
          const path =
            operation.extResourceId === null ? null : external.get(operation.extResourceId);
          if (path !== undefined && path !== null) {
            references.push(path);
          }
          break;
        }
        case "change_resource_reference": {
          if (operation.newPath !== undefined && operation.newPath.length > 0) {
            references.push(stripResPrefix(operation.newPath));
          }
          break;
        }
        case "set_property":
        case "remove_property":
        case "add_node":
        case "remove_node":
        case "add_signal_connection":
        case "remove_signal_connection":
        case "create_subresource":
        case "update_subresource":
        case "remove_subresource":
          break;
      }
    }
    return references;
  }

  async function prepareUnified(input: {
    readonly request: string;
    readonly targets: readonly UnifiedPrepareTargetInput[];
    readonly signal?: AbortSignal;
  }): Promise<UnifiedPrepareResult> {
    if (input.signal?.aborted) {
      return { status: "cancelled", message: "The unified preparation was cancelled." };
    }
    if (input.targets.length === 0) {
      return {
        status: "invalid_input",
        message: "A unified change set requires at least one target.",
      };
    }
    if (input.targets.length > UNIFIED_MAX_TARGETS) {
      return {
        status: "invalid_input",
        message: `A unified change set is limited to ${UNIFIED_MAX_TARGETS} targets.`,
      };
    }
    // Refuse before any approval when the identity-bound apply gate is
    // unavailable on this platform (mirrors the GDScript flow).
    if (!dependencies.canApplyIdentityBound) {
      return {
        status: "unavailable",
        message: CHANGE_SET_EXECUTION_UNAVAILABLE_MESSAGE,
        blocked: blocked(
          "infrastructure_unavailable",
          "The identity-bound apply gate is unavailable on this platform.",
        ),
      };
    }
    const seen = new Set<string>();
    const targetInputs: {
      readonly kind: "text" | "native";
      readonly path: string;
      readonly fingerprint: string;
      readonly preStates: readonly { readonly path: string; readonly sha256: string }[];
      readonly target: import("@solaris/core").UnifiedTarget;
      readonly references: readonly string[];
    }[] = [];
    const preparedFiles: Map<string, readonly PreparedChangeSetFile[]> = new Map();
    let totalResultBytes = 0;
    let targetIndex = 0;
    for (const target of input.targets) {
      targetIndex += 1;
      if (target.kind === "text") {
        const validated = validateChangeSetRequest(target.changes);
        if (!validated.ok) {
          return { status: "invalid_input", message: validated.message };
        }
        const prepared = await prepareChangeSet(validated.request, {
          workspaceRoot: dependencies.workspaceRoot,
          revisions: dependencies.revisions,
        });
        if (prepared.status === "ready") {
          totalResultBytes += prepared.resultBytes;
          const paths = prepared.files.map((file) => file.path);
          for (const path of paths) {
            if (seen.has(path)) {
              return {
                status: "conflict",
                message: `The path "${path}" appears in more than one target.`,
              };
            }
            seen.add(path);
          }
          preparedFiles.set(paths[0] ?? `text-target-${targetIndex}`, prepared.files);
          targetInputs.push({
            kind: "text",
            path: paths[0] ?? `text-target-${targetIndex}`,
            fingerprint: prepared.digest,
            preStates: prepared.files
              .filter((file) => file.expectedSha256 !== null)
              .map((file) => ({ path: file.path, sha256: file.expectedSha256 as string })),
            target: { kind: "text", fileOps: validated.request.changes },
            references: [],
          });
          continue;
        }
        if (prepared.status === "stale_revision") {
          return { status: "stale_revision", message: prepared.message };
        }
        return { status: "failed", message: prepared.message };
      }
      if (target.kind !== "scene" && target.kind !== "resource") {
        return {
          status: "invalid_input",
          message: "Every target must be kind text, scene, or resource.",
        };
      }
      if (typeof target.path !== "string" || target.path.length === 0) {
        return { status: "invalid_input", message: "Every native target requires a path." };
      }
      const prepared =
        target.kind === "scene"
          ? await dependencies.native.prepareSceneChange({
              path: target.path,
              operations: target.operations ?? [],
            })
          : await dependencies.native.prepareResourceChange({
              path: target.path,
              operations: target.operations ?? [],
            });
      if (prepared.status !== "ready" || prepared.prepared === null) {
        return { status: "failed", message: prepared.message };
      }
      if (seen.has(prepared.prepared.targetPath)) {
        return {
          status: "conflict",
          message: `The path "${prepared.prepared.targetPath}" appears in more than one target.`,
        };
      }
      seen.add(prepared.prepared.targetPath);
      const references = await resolveTargetReferences({
        kind: target.kind,
        path: prepared.prepared.targetPath,
        operations: prepared.prepared.operations,
      });
      targetInputs.push({
        kind: "native",
        path: prepared.prepared.targetPath,
        fingerprint: prepared.prepared.fingerprint,
        preStates: [{ path: prepared.prepared.targetPath, sha256: prepared.prepared.sourceSha256 }],
        target: { kind: "native", prepared: prepared.prepared },
        references,
      });
    }
    if (totalResultBytes > UNIFIED_MAX_RESULT_BYTES) {
      return {
        status: "changeset_too_large",
        message: "The unified change set exceeds the result byte limit.",
      };
    }
    const surfaceDecision = classifyDevelopmentSurface({
      request: input.request,
      touchpoints: targetInputs.map((target) => ({
        path: target.path,
        status: "verified" as const,
      })),
    });
    const orderTargets = targetInputs.map((target) => ({
      targetId: target.path,
      path: target.path,
      references: target.references,
    }));
    const { edges, unresolvedReferences } = deriveUnifiedOrderEdges(orderTargets);
    let order;
    try {
      order = deriveUnifiedApplyOrder(orderTargets, edges);
    } catch (error: unknown) {
      return {
        status: "failed",
        message:
          error instanceof Error ? error.message : "The unified apply order could not be derived.",
        blocked: blocked(
          "mutation_not_representable",
          "The proposed targets form an unsatisfiable apply order.",
        ),
      };
    }
    const orderedTargets = order.order.map((id) => {
      const target = targetInputs.find((entry) => entry.path === id);
      if (target === undefined) {
        throw new Error(`Ordered target not found: ${id}`);
      }
      return target;
    });
    const id = `unified-${now().toString(36)}`;
    const created = createUnifiedChangeSet({
      id,
      targets: orderedTargets.map((target) => ({
        kind: target.kind,
        path: target.path,
        fingerprint: target.fingerprint,
        preStates: target.preStates,
        target: target.target,
      })),
      surface: surfaceDecision.kind,
      orderRationale: `${order.rationale}${
        unresolvedReferences.length === 0
          ? ""
          : ` Unresolved references: ${unresolvedReferences
              .map((reference) => `${reference.targetId}->${reference.path}`)
              .join(", ")}.`
      }`,
      createdAtMs: now(),
      ttlMs: UNIFIED_TTL_MS,
    });
    changeSet = created;
    preparedFilesByTarget = preparedFiles;
    const previewFiles: {
      path: string;
      operation: "create" | "update" | "delete";
      beforeSha256: string | null;
      afterSha256: string | null;
      addedLines: number;
      removedLines: number;
      unifiedDiff: string;
    }[] = [];
    let totalAddedLines = 0;
    let totalRemovedLines = 0;
    for (const target of created.targets) {
      if (target.kind === "text") {
        for (const file of preparedFiles.get(target.path) ?? []) {
          previewFiles.push({
            path: file.path,
            operation: file.operation,
            beforeSha256: file.beforeSha256,
            afterSha256: file.afterSha256,
            addedLines: file.addedLines,
            removedLines: file.removedLines,
            unifiedDiff: file.unifiedDiff,
          });
          totalAddedLines += file.addedLines;
          totalRemovedLines += file.removedLines;
        }
        continue;
      }
      const nativeTarget = target.target as Extract<
        typeof target.target,
        { readonly kind: "native" }
      >;
      const prepared = nativeTarget.prepared;
      previewFiles.push({
        path: prepared.targetPath,
        operation: "update",
        beforeSha256: prepared.sourceSha256,
        afterSha256: sha256Text(prepared.serializedAfter),
        addedLines: prepared.addedLines,
        removedLines: prepared.removedLines,
        unifiedDiff: prepared.preview.diff,
      });
      totalAddedLines += prepared.addedLines;
      totalRemovedLines += prepared.removedLines;
    }
    const preview: ChangePreview = {
      files: previewFiles,
      totalAddedLines,
      totalRemovedLines,
      truncated: false,
    };
    emit({
      type: "development_change_prepared",
      id: created.id,
      files: created.targets.length,
    });
    return { status: "ready", changeSet: created, preview };
  }

  async function suspendLanguageSession(): Promise<void> {
    if (dependencies.language === null || dependencies.language === undefined) {
      return;
    }
    if (dependencies.language.activeSession() === null) {
      return;
    }
    emit({ type: "development_language_suspending", id: "unified" });
    try {
      await dependencies.language.closeAll();
      emit({ type: "development_language_suspended", id: "unified" });
    } catch {
      // A session that cannot stop cleanly blocks the edit (mirrors the
      // GDScript loop); the apply callers see the failure through the
      // apply result below.
      emit({ type: "development_language_suspended", id: "unified" });
    }
  }

  async function applyUnified(input: {
    readonly changeSetId: string;
    readonly approvedDigest: string;
    readonly signal?: AbortSignal;
  }): Promise<UnifiedApplyResult> {
    const current = changeSet;
    if (current === null || current.id !== input.changeSetId) {
      return {
        status: "conflict",
        message: "The unified change set is not prepared for this session; prepare a new one.",
        checkpointIds: [],
      };
    }
    if (input.signal?.aborted) {
      return {
        status: "cancelled",
        message: "The unified apply was cancelled.",
        checkpointIds: [],
      };
    }
    // Combined approval binding: the approved digest must match the exact
    // prepared change set; any target change invalidates the whole batch.
    if (input.approvedDigest !== current.combinedDigest) {
      return {
        status: "conflict",
        message:
          "The approval does not match the prepared unified change set; a new approval is required.",
        checkpointIds: [],
      };
    }
    // The combined approval authorizes each exact prepared target; record
    // the per-target approval state so readiness is per-target exact.
    let approved = current;
    try {
      for (const target of approved.targets) {
        approved = approveUnifiedTarget(approved, target.targetId, target.fingerprint);
      }
    } catch (error: unknown) {
      return {
        status: "conflict",
        message:
          error instanceof Error
            ? error.message
            : "The prepared change set could not be authorized.",
        checkpointIds: [],
      };
    }
    const readiness = unifiedChangeSetReadyToApply(approved, now());
    if (!readiness.ready) {
      return {
        status: "conflict",
        message: readiness.reason ?? "The change set is not ready.",
        checkpointIds: [],
      };
    }
    // Authorization point: the combined approval now authorizes the exact
    // prepared batch; the workflow observes the approval event.
    emit({ type: "development_change_approved", id: current.id, changeSetId: current.id });
    if (!dependencies.canApplyIdentityBound) {
      return {
        status: "unavailable",
        message: CHANGE_SET_EXECUTION_UNAVAILABLE_MESSAGE,
        checkpointIds: [],
        blocked: blocked(
          "infrastructure_unavailable",
          "The identity-bound apply gate is unavailable on this platform; no mutation was applied.",
        ),
      };
    }
    // Suspend the language session before the edit (fresh session after).
    await suspendLanguageSession();
    // Build one combined apply request: text files plus serializer-generated
    // native content, in the derived order. Every pre-state is included so
    // the protocol revalidates ALL targets before any checkpoint or write.
    const files: import("@solaris/core").ChangeSetApplyFileRequest[] = [];
    for (const target of current.targets) {
      if (target.kind === "text") {
        for (const file of preparedFilesByTarget.get(target.path) ?? []) {
          files.push({
            path: file.path,
            operation: file.operation,
            expectedSha256: file.expectedSha256,
            content: file.content,
            beforeSha256: file.beforeSha256,
            afterSha256: file.afterSha256,
            addedLines: file.addedLines,
            removedLines: file.removedLines,
          });
        }
        continue;
      }
      const nativeTarget = target.target as Extract<
        typeof target.target,
        { readonly kind: "native" }
      >;
      const prepared = nativeTarget.prepared;
      files.push({
        path: prepared.targetPath,
        operation: "update",
        expectedSha256: prepared.sourceSha256,
        content: prepared.serializedAfter,
        beforeSha256: prepared.sourceSha256,
        afterSha256: sha256Text(prepared.serializedAfter),
        addedLines: prepared.addedLines,
        removedLines: prepared.removedLines,
      });
    }
    const request: ChangeSetApplyRequest = {
      changeSetId: current.id,
      toolName: "workspace.apply_unified_changeset",
      files,
      ...(input.signal === undefined ? {} : { signal: input.signal }),
    };
    const outcome = await applyChangeSetProtocol(request, dependencies.primitives, {
      store: dependencies.store,
      lock: dependencies.lock,
      toolName: "workspace.apply_unified_changeset",
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
              : "apply_failed",
        message: outcome.message,
        checkpointIds: "checkpointIds" in outcome ? outcome.checkpointIds : [],
      };
    }
    emit({
      type: "development_change_applied",
      id: current.id,
      files: files.length,
      ...(outcome.revisions === undefined ? {} : { revisions: outcome.revisions }),
    });
    const revisions = outcome.revisions ?? [];
    // Per-surface verification: native targets reparse + semantic check.
    const nativeVerification: {
      readonly path: string;
      readonly status: "verified" | "failed";
      readonly detail: string | null;
    }[] = [];
    const changedGDScript: string[] = [];
    for (const target of current.targets) {
      if (target.kind === "native") {
        const prepared = (
          target.target as Extract<typeof target.target, { readonly kind: "native" }>
        ).prepared;
        const verified = await verifyNativeTarget(prepared);
        nativeVerification.push({
          path: prepared.targetPath,
          status: verified.verified ? "verified" : "failed",
          detail: verified.detail,
        });
        emit({
          type: "development_native_verified",
          id: current.id,
          targetPath: prepared.targetPath,
          status: verified.verified ? "verified" : "failed",
        });
        continue;
      }
      const textTarget = target.target as Extract<typeof target.target, { readonly kind: "text" }>;
      for (const operation of textTarget.fileOps) {
        if (operation.path.endsWith(".gd")) {
          changedGDScript.push(operation.path);
        }
      }
    }
    const nativeFailed = nativeVerification.some((entry) => entry.status === "failed");
    // Text verification: parser gate + fresh LSP evidence (fail closed
    // honestly when a gate cannot run; never success).
    const parserOutcome =
      changedGDScript.length === 0 ? null : await runParserGate(changedGDScript);
    const lspOutcome =
      changedGDScript.length === 0
        ? null
        : await runFreshLspGate(changedGDScript, input.approvedDigest);
    if (parserOutcome !== null) {
      emit({
        type: "development_parser_completed",
        id: current.id,
        checkedFiles: parserOutcome.checkedFiles,
        validFiles: parserOutcome.validFiles,
      });
    }
    if (lspOutcome !== null) {
      emit({
        type: "development_validation_completed",
        id: current.id,
        errors: lspOutcome.errors,
        warnings: lspOutcome.warnings,
      });
    }
    const textGateFailure =
      parserOutcome !== null && parserOutcome.status !== "ok"
        ? parserOutcome.message
        : lspOutcome !== null && lspOutcome.status !== "ok"
          ? lspOutcome.message
          : null;
    if (textGateFailure !== null) {
      return {
        status: "validation_failed",
        message: `Applied, but text validation could not complete: ${textGateFailure}`,
        checkpointIds: outcome.checkpointIds,
        blocked: blocked("validation_gate_unavailable", textGateFailure),
      };
    }
    if (
      (parserOutcome !== null && parserOutcome.validFiles !== parserOutcome.checkedFiles) ||
      (lspOutcome !== null && lspOutcome.errors > 0)
    ) {
      return {
        status: "validation_failed",
        message: "Applied, but GDScript validation reported errors.",
        checkpointIds: outcome.checkpointIds,
      };
    }
    // Cross-surface consistency over the applied native documents.
    const documents = new Map<
      string,
      import("@solaris/core").GodotSceneModel | import("@solaris/core").GodotResourceModel
    >();
    for (const target of current.targets) {
      if (target.kind === "native") {
        const parsed = await readParsedTarget(
          (target.target as Extract<typeof target.target, { readonly kind: "native" }>).prepared,
        );
        if (parsed !== null) {
          documents.set(target.path, parsed);
        }
      }
    }
    const scriptTargetPaths = current.targets
      .filter((target) => target.kind === "text")
      .flatMap((target) =>
        (target.target as Extract<typeof target.target, { readonly kind: "text" }>).fileOps
          .filter((operation) => operation.path.endsWith(".gd"))
          .map((operation) => operation.path),
      );
    // Host path inventory: every externally referenced path is checked on
    // disk (bounded — only referenced paths), plus every changeset target.
    const diskPaths = new Set<string>();
    for (const document of documents.values()) {
      for (const external of document.externalResources) {
        if (external.path !== undefined && external.path.length > 0) {
          const path = stripResPrefix(external.path);
          if ((await readDocumentText(path)) !== null) {
            diskPaths.add(path);
          }
        }
      }
    }
    const consistency = verifyCrossSurfaceConsistency({
      changeSet: current,
      documents,
      pathExists: (path) =>
        diskPaths.has(path) || current.targets.some((target) => target.path === path),
      scriptTargetPaths,
    });
    emit({
      type: "development_consistency_completed",
      id: current.id,
      consistent: consistency.consistent,
      concernCount: consistency.checks.length,
    });
    // The batch applied exactly as prepared (hash-verified before and
    // after every file), so batch-scoped workspace integrity is verified.
    emit({ type: "development_scope_verified", id: current.id });
    // Impact derivation (read-only; absent provider leaves no impact evidence).
    let impact: { readonly taskId: string; readonly completeness: string } | null = null;
    if (dependencies.impact !== null && dependencies.impact !== undefined) {
      try {
        const manifest = await dependencies.impact({
          taskId: current.id,
          primaryChanges: current.targets.map((target) => ({
            path: target.path,
            operation: target.kind === "native" ? "update" : "update",
          })),
        });
        if (manifest !== null) {
          impact = { taskId: manifest.taskId, completeness: manifest.completeness };
          emit({
            type: "development_impact_derived",
            id: current.id,
            completeness: manifest.completeness,
          });
        }
      } catch {
        // Impact is derived context, never authority; absence is reported
        // through the result, not fabricated.
      }
    }
    changeSet = null;
    preparedFilesByTarget = new Map();
    if (nativeFailed) {
      return {
        status: "verification_failed",
        message:
          "One or more native targets failed semantic verification; earlier verified changes are preserved.",
        checkpointIds: outcome.checkpointIds,
        nativeVerification,
        consistency: {
          consistent: consistency.consistent,
          concernCount: consistency.checks.length,
        },
        parser:
          parserOutcome === null
            ? null
            : { checkedFiles: parserOutcome.checkedFiles, validFiles: parserOutcome.validFiles },
        lsp:
          lspOutcome === null ? null : { errors: lspOutcome.errors, warnings: lspOutcome.warnings },
        impact,
        blocked: blocked(
          "mutation_not_representable",
          "A native target's applied state does not match its expected semantic effect.",
        ),
      };
    }
    return {
      status: "applied",
      changeSetId: current.id,
      revisions,
      checkpointIds: outcome.checkpointIds,
      nativeVerification,
      consistency: {
        consistent: consistency.consistent,
        concernCount: consistency.checks.length,
      },
      parser:
        parserOutcome === null
          ? null
          : { checkedFiles: parserOutcome.checkedFiles, validFiles: parserOutcome.validFiles },
      lsp:
        lspOutcome === null ? null : { errors: lspOutcome.errors, warnings: lspOutcome.warnings },
      impact,
    };
  }

  async function verifyNativeTarget(prepared: PreparedGodotMutation): Promise<{
    readonly verified: boolean;
    readonly detail: string | null;
  }> {
    const parsed = await readParsedTarget(prepared);
    if (parsed === null) {
      return {
        verified: false,
        detail:
          "The applied native revision could not be read or parsed; the mutation is not verified.",
      };
    }
    const verification: SemanticVerification =
      prepared.kind === "scene"
        ? verifySceneSemanticEffect(parsed as never, prepared.expectedSemanticEffect)
        : verifyResourceSemanticEffect(parsed as never, prepared.expectedSemanticEffect);
    if (verification.status !== "verified") {
      return {
        verified: false,
        detail: verification.checks
          .filter((check) => check.status !== "verified")
          .map((check) => check.detail)
          .join("; "),
      };
    }
    return { verified: true, detail: null };
  }

  async function readParsedTarget(
    prepared: PreparedGodotMutation,
  ): Promise<
    import("@solaris/core").GodotSceneModel | import("@solaris/core").GodotResourceModel | null
  > {
    const resolved = await resolveWorkspacePath(dependencies.workspaceRoot, prepared.targetPath);
    if (resolved.status === "rejected") {
      return null;
    }
    const buffer = await readFileBounded(resolved.absolutePath, UNIFIED_MAX_DOCUMENT_BYTES);
    if (buffer === null || looksBinary(buffer)) {
      return null;
    }
    const text = decodeUtf8(buffer);
    if (text === null) {
      return null;
    }
    const revision = dependencies.revisions.currentRevision(resolved.workspaceRelativePath);
    const parsed =
      prepared.kind === "scene"
        ? parseGodotScene(text, resolved.workspaceRelativePath, { revision })
        : parseGodotResource(text, resolved.workspaceRelativePath, { revision });
    return parsed.document;
  }

  async function runParserGate(scripts: readonly string[]): Promise<
    | { readonly status: "ok"; readonly checkedFiles: number; readonly validFiles: number }
    | {
        readonly status: "infrastructure_failure" | "cancelled";
        readonly message: string;
        readonly checkedFiles: number;
        readonly validFiles: number;
      }
  > {
    if (dependencies.diagnostics === null || dependencies.diagnostics === undefined) {
      return {
        status: "infrastructure_failure",
        message: "The GDScript check-only gate is unavailable; text verification is incomplete.",
        checkedFiles: scripts.length,
        validFiles: 0,
      };
    }
    let checkedFiles = 0;
    let validFiles = 0;
    for (const script of scripts) {
      const prepared = await dependencies.diagnostics.prepare({ paths: [script] });
      if (prepared.status !== "ready") {
        return {
          status: "infrastructure_failure",
          message: `The --check-only gate for "${script}" could not run: ${prepared.message}`,
          checkedFiles,
          validFiles,
        };
      }
      const result = await dependencies.diagnostics.execute(prepared.check, {
        approvedDigest: prepared.digest,
      });
      if (result.status === "cancelled") {
        return {
          status: "cancelled",
          message: "The parser validation was cancelled.",
          checkedFiles,
          validFiles,
        };
      }
      if (result.status !== "checked") {
        return {
          status: "infrastructure_failure",
          message: `The --check-only gate for "${script}" could not run: ${result.message}`,
          checkedFiles,
          validFiles,
        };
      }
      checkedFiles += 1;
      if (result.invalidCount === 0) {
        validFiles += 1;
      }
    }
    return { status: "ok", checkedFiles, validFiles };
  }

  async function runFreshLspGate(
    scripts: readonly string[],
    approvedDigest: string,
  ): Promise<
    | { readonly status: "ok"; readonly errors: number; readonly warnings: number }
    | {
        readonly status: "infrastructure_failure" | "cancelled";
        readonly message: string;
        readonly errors: number;
        readonly warnings: number;
      }
  > {
    if (dependencies.language === null || dependencies.language === undefined) {
      return {
        status: "infrastructure_failure",
        message: "The language session is unavailable; fresh LSP verification is incomplete.",
        errors: scripts.length,
        warnings: 0,
      };
    }
    const support = await dependencies.language.support();
    if (support.state !== "available") {
      return {
        status: "infrastructure_failure",
        message: `The language session is unavailable: ${support.reason ?? "unknown"}`,
        errors: scripts.length,
        warnings: 0,
      };
    }
    const prepared = await dependencies.language.prepare();
    if (prepared.status !== "ready") {
      return {
        status: "infrastructure_failure",
        message: `A fresh language session could not be prepared: ${prepared.message}`,
        errors: scripts.length,
        warnings: 0,
      };
    }
    const started = await dependencies.language.start(prepared.session, { approvedDigest });
    if (started.status !== "ready") {
      return {
        status: "infrastructure_failure",
        message: `A fresh language session could not start: ${started.message}`,
        errors: scripts.length,
        warnings: 0,
      };
    }
    emit({ type: "development_language_restarted", id: "unified" });
    let errors = 0;
    let warnings = 0;
    for (const script of scripts) {
      const result = await started.session.diagnostics({ path: script });
      if (result.status !== "ready") {
        return {
          status: "infrastructure_failure",
          message: `Fresh LSP diagnostics for "${script}" could not be collected: ${result.message}`,
          errors,
          warnings,
        };
      }
      for (const diagnostic of result.result.diagnostics) {
        if (diagnostic.severity === "error") {
          errors += 1;
        } else if (diagnostic.severity === "warning") {
          warnings += 1;
        }
      }
    }
    return { status: "ok", errors, warnings };
  }

  return {
    support() {
      return dependencies.canApplyIdentityBound
        ? { state: "available", reason: null }
        : {
            state: "unavailable",
            reason: CHANGE_SET_EXECUTION_UNAVAILABLE_MESSAGE,
          };
    },
    prepareUnified,
    applyUnified,
    cancel(): Promise<void> {
      changeSet = null;
      preparedFilesByTarget = new Map();
      return Promise.resolve();
    },
    close(): Promise<void> {
      changeSet = null;
      preparedFilesByTarget = new Map();
      return Promise.resolve();
    },
    get onEvent(): ((event: DevelopmentEvent) => void) | undefined {
      return eventSlot;
    },
    set onEvent(listener: ((event: DevelopmentEvent) => void) | undefined) {
      eventSlot = listener;
    },
  };
}
