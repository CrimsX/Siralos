import {
  DEVELOPMENT_LIMITS,
  computeGDScriptDevelopmentDigest,
  type ChangePreview,
  type ChangeSetFilePrimitives,
  type CheckpointStore,
  type DevelopmentChangeSetApplicationResult,
  type DevelopmentChangeSetPreparationResult,
  type DevelopmentChangeSetExecutionContext,
  type DevelopmentEvent,
  type DevelopmentEvidence,
  type DevelopmentStartPreparationResult,
  type DevelopmentStartResult,
  type DevelopmentValidationStatus,
  type GDScriptDevelopmentPreview,
  type GDScriptDevelopmentResult,
  type GDScriptDevelopmentService,
  type GDScriptDevelopmentSession,
  type GDScriptDevelopmentStatus,
  type GDScriptLanguageService,
  type GitInspector,
  type GodotDiagnostics,
  type GodotGDScriptDiagnostic,
  type ToolExecutionContext,
} from "@solaris/core";
import { randomUUID } from "node:crypto";
import { createAbortError } from "../probe/risk-manifest.js";
import { scanAuthoredFiles } from "../probe/authored-files.js";
import { prepareChangeSet as prepareChangeSetExact } from "./change-set-preparation.js";
import {
  CHANGE_SET_EXECUTION_UNAVAILABLE_MESSAGE,
  createDevelopmentChangeSetApplier,
  type ChangeSetExecutorDependencies,
} from "./change-set-executor.js";

export interface GDScriptDevelopmentServiceDependencies {
  readonly workspaceRoot: string;
  readonly platform: NodeJS.Platform;
  readonly store: CheckpointStore;
  readonly lock: { acquire(signal?: AbortSignal): Promise<() => void> };
  readonly language: GDScriptLanguageService;
  readonly diagnostics: GodotDiagnostics;
  /** Read-only Git inspector; null when Git inspection is unavailable. */
  readonly git: GitInspector | null;
  /**
   * False on every platform at this stage: the exact change set cannot be
   * applied until a mechanically identity-bound commit primitive exists.
   */
  readonly canApplyIdentityBound: boolean;
  /**
   * File primitives the change-set applier uses. Production injects a
   * fail-closed implementation (zero filesystem operations); tests inject
   * an in-memory implementation to exercise the apply protocol.
   */
  readonly primitives: ChangeSetFilePrimitives;
  readonly onEvent?: (event: DevelopmentEvent) => void;
  readonly now?: () => number;
  readonly idFactory?: () => string;
  /** Diagnostic-settling parameters (test-visible, deterministic). */
  readonly settling?: {
    readonly hardTimeoutMs: number;
    readonly pollIntervalMs: number;
  };
}

const AUTHENTICATION_POLICY_VERSION = 1;
const MAX_REQUEST_CHARS = 4096;

interface PendingStart {
  readonly request: string;
  readonly digest: string;
  readonly projectFingerprint: string;
  readonly engineFingerprint: string | null;
  readonly engineVersion: string | null;
  readonly preview: GDScriptDevelopmentPreview;
}

interface PreparedChangeSetRecord {
  readonly id: string;
  readonly files: readonly import("@solaris/core").PreparedChangeSetFile[];
  readonly preview: ChangePreview;
  readonly digest: string;
  readonly repair: boolean;
  readonly expiresAtMs: number;
}

interface InternalSession {
  readonly id: string;
  readonly request: string;
  readonly projectFingerprint: string;
  /** Baseline authored-file entries (path -> sha256) at workflow start. */
  readonly baselineEntries: ReadonlyMap<string, string>;
  readonly engineFingerprint: string | null;
  readonly engineVersion: string | null;
  readonly startedAtMs: number;
  state: import("@solaris/core").DevelopmentState;
  iteration: number;
  repairProposalsUsed: number;
  validation: DevelopmentValidationStatus | null;
  evidence: DevelopmentEvidence[];
  errorCount: number;
  warningCount: number;
  checkpointIds: string[];
  changes: import("@solaris/core").DevelopmentChangeRecord[];
  prepared: Map<string, PreparedChangeSetRecord>;
  /** Files applied by previous change sets (path -> afterSha256). */
  appliedFiles: Map<string, string>;
}

/**
 * Bounded GDScript development workflow (§7–§14, §21–§35).
 *
 * The workflow orchestrates existing capabilities and never bypasses
 * them: every source mutation is an exact approved change set, every
 * change set is checkpointed before application, the language session is
 * suspended before an edit and recreated fresh afterwards, the
 * `--check-only` parser gate runs before language validation, validation
 * evidence is collected deterministically, and repair iterations are
 * bounded. Approval semantics: the workflow start is a one-time approval
 * covering the read-only validation context (LSP recreation after
 * approved edits, check-only parsing, API lookup, workspace and Git
 * inspection); each source change set still requires its own exact
 * one-time approval.
 *
 * At this stage the change-set applier fails closed as unavailable on
 * every platform (no directory-relative commit primitive), so the
 * workflow refuses before any approval for a mutation and no checkpoint
 * is ever created; the full orchestration below is tested internal code
 * exercised through injected in-memory primitives and fakes.
 */
export function createGDScriptDevelopmentService(
  dependencies: GDScriptDevelopmentServiceDependencies,
): GDScriptDevelopmentService {
  const now = dependencies.now ?? (() => Date.now());
  const idFactory = dependencies.idFactory ?? (() => randomUUID());
  const emit = (event: DevelopmentEvent): void => {
    dependencies.onEvent?.(event);
  };
  const executorDependencies: ChangeSetExecutorDependencies = {
    store: dependencies.store,
    lock: dependencies.lock,
    toolName: "workspace.apply_text_changeset",
    canApplyIdentityBound: dependencies.canApplyIdentityBound,
  };
  const applier = createDevelopmentChangeSetApplier(executorDependencies);
  let pendingStart: PendingStart | null = null;
  let session: InternalSession | null = null;

  async function support(): Promise<{
    readonly state: "available" | "unavailable";
    readonly reason: string | null;
    readonly platform: string;
  }> {
    const available = await applier.isAvailable();
    return {
      state: available ? "available" : "unavailable",
      reason: available
        ? null
        : `The GDScript development workflow cannot apply source changes on this platform: ${CHANGE_SET_EXECUTION_UNAVAILABLE_MESSAGE}`,
      platform: dependencies.platform,
    };
  }

  async function prepareStart(
    request: string,
    signal?: AbortSignal,
  ): Promise<DevelopmentStartPreparationResult> {
    if (signal?.aborted) {
      throw createAbortError();
    }
    const trimmed = request.trim();
    if (trimmed.length === 0) {
      return { status: "invalid_input", message: "The development request must not be empty." };
    }
    if (trimmed.length > MAX_REQUEST_CHARS) {
      return {
        status: "invalid_input",
        message: `The development request exceeds ${MAX_REQUEST_CHARS} characters.`,
      };
    }
    if (session !== null) {
      return {
        status: "conflict",
        message: "A development workflow is already active; finish or cancel it first.",
      };
    }
    const available = await applier.isAvailable();
    if (!available) {
      return {
        status: "unavailable",
        message: CHANGE_SET_EXECUTION_UNAVAILABLE_MESSAGE,
      };
    }
    const manifest = await scanAuthoredFiles({
      workspaceRoot: dependencies.workspaceRoot,
      ...(signal === undefined ? {} : { signal }),
    });
    const engine = await dependencies.language.selectedEngine(signal);
    const preview: GDScriptDevelopmentPreview = {
      request: trimmed,
      projectName: null,
      projectFingerprint: manifest.digest,
      engineVersion: engine?.version ?? null,
      engineFingerprint: engine?.sha256 ?? null,
      limits: {
        maxIterations: DEVELOPMENT_LIMITS.maxTotalIterations,
        maxRepairProposals: DEVELOPMENT_LIMITS.maxRepairProposals,
        maxFilesPerChangeSet: DEVELOPMENT_LIMITS.maxFilesPerChangeSet,
      },
      authorization: {
        sourceWrites: "each change set approved separately",
        languageSession: "read-only; recreated after approved edits under this approval",
        checkOnlyParsing: "covered",
        apiLookup: "covered",
        workspaceInspection: "covered",
        gitInspection: "covered",
        network: "denied",
        gameExecution: "disabled",
      },
    };
    const digest = computeGDScriptDevelopmentDigest({
      request: trimmed,
      projectFingerprint: manifest.digest,
      engineFingerprint: engine?.sha256 ?? null,
      limits: preview.limits,
      authorizationPolicyVersion: AUTHENTICATION_POLICY_VERSION,
    });
    const workflowId = idFactory();
    pendingStart = {
      request: trimmed,
      digest,
      projectFingerprint: manifest.digest,
      engineFingerprint: engine?.sha256 ?? null,
      engineVersion: engine?.version ?? null,
      preview,
    };
    return { status: "ready", workflowId, preview, digest };
  }

  async function start(
    workflowId: string,
    context: { readonly approvedDigest: string; readonly signal?: AbortSignal },
  ): Promise<DevelopmentStartResult> {
    if (context.signal?.aborted) {
      throw createAbortError();
    }
    if (pendingStart === null) {
      return { status: "failed", message: "No development workflow is prepared for start." };
    }
    if (workflowId !== session?.id && session !== null) {
      return { status: "conflict", message: "A development workflow is already active." };
    }
    if (context.approvedDigest !== pendingStart.digest) {
      return {
        status: "conflict",
        message:
          "The approval does not match the prepared development workflow; a new approval is required.",
      };
    }
    const manifest = await scanAuthoredFiles({
      workspaceRoot: dependencies.workspaceRoot,
      ...(context.signal === undefined ? {} : { signal: context.signal }),
    });
    if (manifest.digest !== pendingStart.projectFingerprint) {
      pendingStart = null;
      return {
        status: "conflict",
        message:
          "The project changed while the workflow approval was pending; approve the workflow again.",
      };
    }
    const engine = await dependencies.language.selectedEngine(context.signal);
    if ((engine?.sha256 ?? null) !== pendingStart.engineFingerprint) {
      pendingStart = null;
      return {
        status: "conflict",
        message:
          "The selected Godot engine changed while the workflow approval was pending; approve the workflow again.",
      };
    }
    const created: InternalSession = {
      id: workflowId,
      request: pendingStart.request,
      projectFingerprint: pendingStart.projectFingerprint,
      baselineEntries: new Map(manifest.entries.map((entry) => [entry.relativePath, entry.sha256])),
      engineFingerprint: pendingStart.engineFingerprint,
      engineVersion: pendingStart.engineVersion,
      startedAtMs: now(),
      state: { kind: "active", phase: "investigating" },
      iteration: 0,
      repairProposalsUsed: 0,
      validation: null,
      evidence: [],
      errorCount: 0,
      warningCount: 0,
      checkpointIds: [],
      changes: [],
      prepared: new Map(),
      appliedFiles: new Map(),
    };
    pendingStart = null;
    session = created;
    emit({ type: "development_started", id: created.id });
    emit({ type: "development_investigating", id: created.id });
    const languageResult = await startLanguageSession(created, new Map(), context.signal);
    if (languageResult.status === "conflict") {
      session.state = { kind: "terminal", status: "conflict" };
      emit({ type: "development_completed", id: created.id, status: "conflict" });
      return {
        status: "conflict",
        message: languageResult.message,
      };
    }
    if (languageResult.status === "ready") {
      emit({ type: "development_language_restarted", id: created.id });
    }
    return { status: "ready", session: toPublicSession(created) };
  }

  async function prepareChangeSet(
    input: unknown,
    context: ToolExecutionContext,
  ): Promise<DevelopmentChangeSetPreparationResult> {
    if (context.signal?.aborted) {
      return { status: "cancelled", message: "The change-set preparation was cancelled." };
    }
    const current = session;
    if (current === null) {
      return {
        status: "failed",
        message:
          "No development workflow is active; start one with /develop before proposing source changes.",
      };
    }
    if (current.state.kind !== "active") {
      return {
        status: "failed",
        message: "The development workflow has finished; start a new one to propose changes.",
      };
    }
    if (
      current.state.phase !== "investigating" &&
      current.state.phase !== "proposal_ready" &&
      current.state.phase !== "reviewing"
    ) {
      return {
        status: "failed",
        message: `The development workflow is mid-validation (${current.state.phase}); wait for the current change set to finish.`,
      };
    }
    if (now() - current.startedAtMs > DEVELOPMENT_LIMITS.totalWorkflowBudgetMs) {
      return {
        status: "failed",
        message: "The total development workflow budget was exceeded; start a new workflow.",
      };
    }
    const isRepair = current.validation === "errors" && current.evidence.length > 0;
    if (isRepair) {
      if (current.repairProposalsUsed >= DEVELOPMENT_LIMITS.maxRepairProposals) {
        return {
          status: "repair_budget_exhausted",
          message: `The maximum of ${DEVELOPMENT_LIMITS.maxRepairProposals} repair proposals has been reached; start a new development workflow.`,
        };
      }
      current.repairProposalsUsed += 1;
    }
    if (current.iteration >= DEVELOPMENT_LIMITS.maxTotalIterations) {
      return {
        status: "iteration_budget_exhausted",
        message: `The maximum of ${DEVELOPMENT_LIMITS.maxTotalIterations} development iterations has been reached; start a new development workflow.`,
      };
    }
    const available = await applier.isAvailable();
    if (!available) {
      return {
        status: "unavailable",
        message: CHANGE_SET_EXECUTION_UNAVAILABLE_MESSAGE,
      };
    }
    const prepared = await prepareChangeSetExact(
      input,
      {
        workspaceRoot: dependencies.workspaceRoot,
        platform: dependencies.platform,
      },
      context.signal,
    );
    if (prepared.status !== "ready") {
      return prepared;
    }
    const changeSetId = idFactory();
    current.prepared.set(changeSetId, {
      id: changeSetId,
      files: prepared.files,
      preview: prepared.preview,
      digest: prepared.digest,
      repair: isRepair,
      expiresAtMs: now() + DEVELOPMENT_LIMITS.preparedChangeSetTtlMs,
    });
    current.state = { kind: "active", phase: "proposal_ready" };
    emit({ type: "development_change_prepared", id: current.id, files: prepared.files.length });
    return {
      status: "ready",
      changeSetId,
      preview: prepared.preview,
      digest: prepared.digest,
      repair: isRepair,
    };
  }

  async function applyChangeSet(
    changeSetId: string,
    context: DevelopmentChangeSetExecutionContext,
  ): Promise<DevelopmentChangeSetApplicationResult> {
    const current = session;
    if (current === null) {
      return {
        status: "failed",
        message: "No development workflow is active.",
        result: null,
      };
    }
    const prepared = current.prepared.get(changeSetId);
    if (prepared === undefined) {
      return {
        status: "failed",
        message: "The prepared change set is not valid for this workflow; prepare it again.",
        result: null,
      };
    }
    if (context.approvedDigest !== prepared.digest) {
      current.prepared.delete(changeSetId);
      return {
        status: "conflict",
        message: "The approval does not match the prepared change set; a new approval is required.",
        result: null,
      };
    }
    if (now() > prepared.expiresAtMs) {
      current.prepared.delete(changeSetId);
      return {
        status: "failed",
        message: "The prepared change set expired; prepare it again.",
        result: null,
      };
    }
    current.prepared.delete(changeSetId);
    current.state = { kind: "active", phase: "awaiting_approval" };
    emit({ type: "development_change_approved", id: current.id, changeSetId });
    const available = await applier.isAvailable();
    if (!available) {
      current.state = { kind: "terminal", status: "unavailable" };
      emit({ type: "development_completed", id: current.id, status: "unavailable" });
      return {
        status: "unavailable",
        message: CHANGE_SET_EXECUTION_UNAVAILABLE_MESSAGE,
        result: finalizeResult(current),
      };
    }
    current.state = { kind: "active", phase: "applying" };
    // 1. Suspend the language session (closing_for_edit). The edit never
    // proceeds when the old session cannot be stopped safely.
    const suspended = await suspendLanguageSession(current);
    if (!suspended.ok) {
      current.state = { kind: "terminal", status: "apply_failed" };
      emit({ type: "development_completed", id: current.id, status: "apply_failed" });
      return {
        status: "apply_failed",
        message: suspended.message,
        result: finalizeResult(current),
      };
    }
    // 2. Apply the approved change set: revalidate preconditions, record
    //    checkpoints, apply sequentially with hash verification, and
    //    recover on partial failure.
    const request = buildApplyRequest(changeSetId, prepared);
    const outcome = await applier.apply(request, dependencies.primitives);
    if (outcome.status === "unavailable") {
      current.state = { kind: "terminal", status: "unavailable" };
      emit({ type: "development_completed", id: current.id, status: "unavailable" });
      return {
        status: "unavailable",
        message: outcome.message,
        result: finalizeResult(current),
      };
    }
    if (outcome.status === "cancelled") {
      current.state = { kind: "terminal", status: "cancelled" };
      emit({ type: "development_completed", id: current.id, status: "cancelled" });
      return {
        status: "cancelled",
        message: outcome.message,
        result: finalizeResult(current),
      };
    }
    if (outcome.status === "conflict") {
      current.state = { kind: "terminal", status: "conflict" };
      emit({ type: "development_completed", id: current.id, status: "conflict" });
      return {
        status: "conflict",
        message: outcome.message,
        result: finalizeResult(current),
      };
    }
    if (outcome.status !== "applied") {
      current.checkpointIds.push(...outcome.checkpointIds);
      current.state = { kind: "terminal", status: "apply_failed" };
      emit({ type: "development_completed", id: current.id, status: "apply_failed" });
      return {
        status: "apply_failed",
        message: outcome.message,
        result: finalizeResult(current),
      };
    }
    current.checkpointIds.push(...outcome.checkpointIds);
    current.iteration += 1;
    for (const file of prepared.files) {
      current.changes.push({
        path: file.path,
        operation: file.operation,
        beforeSha256: file.beforeSha256,
        afterSha256: file.afterSha256,
      });
      if (file.afterSha256 !== null) {
        current.appliedFiles.set(file.path, file.afterSha256);
      } else {
        current.appliedFiles.delete(file.path);
      }
    }
    emit({ type: "development_change_applied", id: current.id, files: prepared.files.length });
    emit({ type: "development_validation_started", id: current.id });

    // 3. Post-edit parser gate: --check-only over the changed scripts.
    current.state = { kind: "active", phase: "parser_validation" };
    const parser = await runParserGate(changedScripts(prepared.files), context.signal);
    if (parser.status === "cancelled") {
      current.state = { kind: "terminal", status: "cancelled" };
      emit({ type: "development_completed", id: current.id, status: "cancelled" });
      return {
        status: "cancelled",
        message: "The validation was cancelled after the change set was applied.",
        result: finalizeResult(current),
      };
    }
    if (parser.status === "infrastructure_failure") {
      current.state = { kind: "terminal", status: "validation_failed" };
      emit({ type: "development_completed", id: current.id, status: "validation_failed" });
      return {
        status: "validation_failed",
        message: parser.message,
        result: finalizeResult(current),
      };
    }
    emit({
      type: "development_parser_completed",
      id: current.id,
      checkedFiles: parser.checkedFiles,
      validFiles: parser.validFiles,
    });

    // 4. Fresh language session from the current source state.
    current.state = { kind: "active", phase: "language_validation" };
    const expectedAfter = new Map(current.appliedFiles);
    const language = await startLanguageSession(current, expectedAfter, context.signal);
    if (language.status === "conflict") {
      current.state = { kind: "terminal", status: "conflict" };
      emit({ type: "development_completed", id: current.id, status: "conflict" });
      return {
        status: "conflict",
        message: language.message,
        result: finalizeResult(current),
      };
    }
    if (language.status !== "ready") {
      current.state = { kind: "terminal", status: "validation_failed" };
      emit({ type: "development_completed", id: current.id, status: "validation_failed" });
      return {
        status: "validation_failed",
        message: language.message,
        result: finalizeResult(current),
      };
    }
    emit({ type: "development_language_restarted", id: current.id });

    // 5. LSP diagnostics for the changed scripts with deterministic settling.
    const lspDiagnostics = await collectSettledDiagnostics(
      changedScripts(prepared.files),
      context.signal,
    );
    if (lspDiagnostics.status === "cancelled") {
      current.state = { kind: "terminal", status: "cancelled" };
      emit({ type: "development_completed", id: current.id, status: "cancelled" });
      return {
        status: "cancelled",
        message: "The validation was cancelled after the change set was applied.",
        result: finalizeResult(current),
      };
    }

    // 6. Workspace integrity: the delta against the workflow baseline must
    //    equal exactly the approved change sets.
    const integrity = await verifyWorkspaceIntegrity(current, expectedAfter);
    const evidence: DevelopmentEvidence = {
      changeSetId,
      files: prepared.files.map((file) => ({
        path: file.path,
        operation: file.operation,
        beforeSha256: file.beforeSha256,
        afterSha256: file.afterSha256,
      })),
      parser: {
        checkedFiles: parser.checkedFiles,
        validFiles: parser.validFiles,
        diagnostics: parser.diagnostics,
      },
      lsp: {
        started: language.status === "ready",
        diagnosticCount: lspDiagnostics.diagnostics.length,
        diagnostics: lspDiagnostics.diagnostics,
      },
      git: await collectGitEvidence(),
      workspaceIntegrity: integrity,
    };
    current.evidence.push(evidence);
    current.errorCount = countSeverity(evidence, "error");
    current.warningCount = countSeverity(evidence, "warning");

    // 7. Normalize the validation status (§30).
    let validation: DevelopmentValidationStatus;
    if (integrity.unexpectedChanges.length > 0) {
      validation = "errors";
    } else if (current.errorCount > 0) {
      validation = "errors";
    } else if (current.warningCount > 0) {
      validation = "warnings";
    } else {
      validation = "clean";
    }
    current.validation = validation;
    current.state = { kind: "active", phase: "reviewing" };
    emit({
      type: "development_validation_completed",
      id: current.id,
      errors: current.errorCount,
      warnings: current.warningCount,
    });
    if (validation === "errors") {
      emit({ type: "development_repair_requested", id: current.id, iteration: current.iteration });
    }
    return {
      status: "applied",
      result: finalizeResult(current),
    };
  }

  function languageQueryGate(): { readonly blocked: boolean; readonly message: string | null } {
    const current = session;
    if (current === null || current.state.kind !== "active") {
      return { blocked: false, message: null };
    }
    if (
      current.state.phase === "applying" ||
      current.state.phase === "parser_validation" ||
      current.state.phase === "language_validation"
    ) {
      return {
        blocked: true,
        message:
          "The language session is closing for an approved edit; new queries are rejected until the fresh session starts.",
      };
    }
    return { blocked: false, message: null };
  }

  function validationStatus(): DevelopmentValidationStatus | null {
    return session?.validation ?? null;
  }

  function completeFromProviderTurn(): void {
    const current = session;
    if (current === null || current.state.kind !== "active") {
      return;
    }
    let status: import("@solaris/core").DevelopmentStatus | null = null;
    if (current.state.phase === "reviewing") {
      status =
        current.validation === "clean"
          ? "completed"
          : current.validation === "warnings"
            ? "completed_with_warnings"
            : current.validation === "errors"
              ? "completed_with_errors"
              : "validation_failed";
    } else if (current.state.phase === "investigating") {
      status = "cancelled";
    }
    if (status === null) {
      return;
    }
    current.state = { kind: "terminal", status };
    emit({ type: "development_completed", id: current.id, status });
    if (status !== "completed" && status !== "completed_with_warnings") {
      void dependencies.language.closeAll();
    }
  }

  async function cancel(
    signal?: AbortSignal,
  ): Promise<import("@solaris/core").DevelopmentCancelResult> {
    const current = session;
    if (current === null) {
      return { status: "inactive", message: "No development workflow is active." };
    }
    if (signal?.aborted) {
      throw createAbortError();
    }
    if (current.state.kind === "terminal") {
      return { status: "cancelled", result: finalizeResult(current) };
    }
    current.state = { kind: "terminal", status: "cancelled" };
    current.validation = current.validation ?? "cancelled";
    emit({ type: "development_completed", id: current.id, status: "cancelled" });
    await dependencies.language.closeAll();
    return { status: "cancelled", result: finalizeResult(current) };
  }

  async function close(): Promise<void> {
    pendingStart = null;
    session?.prepared.clear();
    await dependencies.language.closeAll();
  }

  function status(): GDScriptDevelopmentStatus {
    const supportState = supportSync();
    if (session === null) {
      return { support: supportState, session: null };
    }
    const current = session;
    const remaining = Math.max(
      0,
      DEVELOPMENT_LIMITS.maxRepairProposals - current.repairProposalsUsed,
    );
    return {
      support: supportState,
      session: {
        id: current.id,
        request: current.request,
        state: current.state,
        iteration: current.iteration,
        maxIterations: DEVELOPMENT_LIMITS.maxTotalIterations,
        repairProposalsRemaining: current.state.kind === "active" ? remaining : 0,
        validation: current.validation,
        appliedChangeSets: current.iteration,
        errors: current.errorCount,
        warnings: current.warningCount,
      },
    };
  }

  function supportSync(): {
    readonly available: boolean;
    readonly reason: string | null;
    readonly platform: string;
  } {
    return {
      available: dependencies.canApplyIdentityBound,
      reason: dependencies.canApplyIdentityBound
        ? null
        : `The GDScript development workflow cannot apply source changes on this platform: ${CHANGE_SET_EXECUTION_UNAVAILABLE_MESSAGE}`,
      platform: dependencies.platform,
    };
  }

  /** Suspends the language session before an approved edit (closing_for_edit). */
  async function suspendLanguageSession(
    current: InternalSession,
  ): Promise<{ readonly ok: true } | { readonly ok: false; readonly message: string }> {
    if (dependencies.language.activeSession() === null) {
      return { ok: true };
    }
    emit({ type: "development_language_suspending", id: current.id });
    try {
      await dependencies.language.closeAll();
    } catch (error: unknown) {
      return {
        ok: false,
        message: `The language session could not be stopped safely (${error instanceof Error ? error.message : "unknown error"}); the edit was not applied.`,
      };
    }
    emit({ type: "development_language_suspended", id: current.id });
    return { ok: true };
  }

  /**
   * Starts (or recreates) the read-only language session under the
   * workflow authorization (§13–§14): the engine fingerprint must be
   * unchanged and the project delta must correspond exactly to the
   * approved change sets; capabilities, sandbox profile, and network
   * policy never broaden.
   */
  async function startLanguageSession(
    current: InternalSession,
    expectedAfter: Map<string, string>,
    signal: AbortSignal | undefined,
  ): Promise<
    | { readonly status: "ready" }
    | { readonly status: "conflict"; readonly message: string }
    | { readonly status: "failed" | "unavailable"; readonly message: string }
  > {
    const engine = await dependencies.language.selectedEngine(signal);
    if ((engine?.sha256 ?? null) !== current.engineFingerprint) {
      return {
        status: "conflict",
        message:
          "The selected Godot engine changed during the development workflow; the workflow is invalidated.",
      };
    }
    const delta = await workspaceDelta(current, signal);
    if (!delta.ok) {
      return { status: "conflict", message: delta.message };
    }
    const unexpected = unexpectedChanges(delta.changed, expectedAfter);
    if (unexpected.length > 0) {
      return {
        status: "conflict",
        message: `The project changed outside the approved change set (${unexpected.join(", ")}); the language session cannot be recreated.`,
      };
    }
    const prepared = await dependencies.language.prepare(signal);
    if (prepared.status !== "ready") {
      return { status: "failed", message: prepared.message };
    }
    const started = await dependencies.language.start(prepared.session, {
      approvedDigest: prepared.digest,
      ...(signal === undefined ? {} : { signal }),
    });
    if (started.status === "ready") {
      return { status: "ready" };
    }
    if (started.status === "conflict") {
      return { status: "conflict", message: started.message };
    }
    return { status: "failed", message: started.message };
  }

  /** The workspace delta against the workflow-start baseline. */
  async function workspaceDelta(
    current: InternalSession,
    signal?: AbortSignal,
  ): Promise<
    | { readonly ok: true; readonly changed: Map<string, string> }
    | { readonly ok: false; readonly message: string }
  > {
    const manifest = await scanAuthoredFiles({
      workspaceRoot: dependencies.workspaceRoot,
      ...(signal === undefined ? {} : { signal }),
    });
    const currentEntries = new Map(
      manifest.entries.map((entry) => [entry.relativePath, entry.sha256]),
    );
    const changed = new Map<string, string>();
    for (const [path, baselineSha] of current.baselineEntries) {
      const currentSha = currentEntries.get(path);
      if (currentSha === undefined) {
        changed.set(path, "absent");
        continue;
      }
      if (currentSha !== baselineSha) {
        changed.set(path, currentSha);
      }
    }
    for (const [path, currentSha] of currentEntries) {
      if (!current.baselineEntries.has(path)) {
        changed.set(path, currentSha);
      }
    }
    for (const [path, appliedSha] of current.appliedFiles) {
      const currentSha = changed.get(path) ?? currentEntries.get(path);
      if (currentSha === undefined || currentSha !== appliedSha) {
        return {
          ok: false,
          message: `"${path}" no longer matches the approved change-set result.`,
        };
      }
    }
    return { ok: true, changed };
  }

  async function verifyWorkspaceIntegrity(
    current: InternalSession,
    expectedAfter: Map<string, string>,
  ): Promise<{ readonly verified: boolean; readonly unexpectedChanges: readonly string[] }> {
    const delta = await workspaceDelta(current);
    if (!delta.ok) {
      return { verified: false, unexpectedChanges: [delta.message] };
    }
    const unexpected = unexpectedChanges(delta.changed, expectedAfter);
    return {
      verified: unexpected.length === 0,
      unexpectedChanges: unexpected,
    };
  }

  /** Post-edit --check-only parser gate over the changed scripts (§12). */
  async function runParserGate(
    scripts: readonly string[],
    signal: AbortSignal | undefined,
  ): Promise<
    | {
        readonly status: "ok";
        readonly checkedFiles: number;
        readonly validFiles: number;
        readonly diagnostics: readonly GodotGDScriptDiagnostic[];
      }
    | {
        readonly status: "infrastructure_failure" | "cancelled";
        readonly message: string;
        readonly checkedFiles: number;
        readonly validFiles: number;
        readonly diagnostics: readonly GodotGDScriptDiagnostic[];
      }
  > {
    const diagnostics: GodotGDScriptDiagnostic[] = [];
    let checkedFiles = 0;
    let validFiles = 0;
    const deadline = now() + DEVELOPMENT_LIMITS.validationBudgetMs;
    for (const script of scripts) {
      if (signal?.aborted) {
        return {
          status: "cancelled",
          message: "The parser validation was cancelled.",
          checkedFiles,
          validFiles,
          diagnostics,
        };
      }
      if (now() > deadline) {
        return {
          status: "infrastructure_failure",
          message: "The validation budget for this iteration was exceeded.",
          checkedFiles,
          validFiles,
          diagnostics,
        };
      }
      const prepared = await dependencies.diagnostics.prepare({ paths: [script] }, signal);
      if (prepared.status !== "ready") {
        return {
          status: "infrastructure_failure",
          message: `The --check-only gate for "${script}" could not be prepared: ${prepared.message}`,
          checkedFiles,
          validFiles,
          diagnostics,
        };
      }
      const result = await dependencies.diagnostics.execute(prepared.check, {
        approvedDigest: prepared.digest,
        ...(signal === undefined ? {} : { signal }),
      });
      if (result.status === "cancelled") {
        return {
          status: "cancelled",
          message: "The parser validation was cancelled.",
          checkedFiles,
          validFiles,
          diagnostics,
        };
      }
      if (result.status !== "checked") {
        return {
          status: "infrastructure_failure",
          message: `The --check-only gate for "${script}" could not run: ${result.message}`,
          checkedFiles,
          validFiles,
          diagnostics,
        };
      }
      checkedFiles += 1;
      if (result.invalidCount === 0) {
        validFiles += 1;
      }
      diagnostics.push(...result.diagnostics);
    }
    return { status: "ok", checkedFiles, validFiles, diagnostics };
  }

  /** LSP diagnostics with initial receipt, quiet period, and hard timeout (§28). */
  async function collectSettledDiagnostics(
    scripts: readonly string[],
    signal: AbortSignal | undefined,
  ): Promise<
    | { readonly status: "ready"; readonly diagnostics: readonly GodotGDScriptDiagnostic[] }
    | { readonly status: "cancelled"; readonly message: string }
  > {
    const sessionHandle = dependencies.language.activeSession();
    if (sessionHandle === null) {
      return { status: "ready", diagnostics: [] };
    }
    const hardTimeoutMs = dependencies.settling?.hardTimeoutMs ?? 5_000;
    const pollIntervalMs = dependencies.settling?.pollIntervalMs ?? 50;
    for (const script of scripts) {
      const opened = await sessionHandle.openDocument({ path: script }, signal);
      if (opened.status === "cancelled") {
        return { status: "cancelled", message: "The language validation was cancelled." };
      }
    }
    const startedAt = now();
    let previous: string | null = null;
    let polls = 0;
    let collected: GodotGDScriptDiagnostic[] = [];
    for (;;) {
      if (signal?.aborted) {
        return { status: "cancelled", message: "The language validation was cancelled." };
      }
      const snapshot: GodotGDScriptDiagnostic[] = [];
      let failed = false;
      for (const script of scripts) {
        const result = await sessionHandle.diagnostics({ path: script }, signal);
        if (result.status === "ready") {
          snapshot.push(...result.result.diagnostics);
        } else {
          failed = true;
          break;
        }
      }
      if (!failed) {
        polls += 1;
        collected = snapshot;
        const signature = JSON.stringify(collected);
        if (polls > 1 && signature === previous) {
          return { status: "ready", diagnostics: collected };
        }
        previous = signature;
      }
      if (now() - startedAt >= hardTimeoutMs) {
        return { status: "ready", diagnostics: collected };
      }
      await sleep(pollIntervalMs, signal);
    }
  }

  async function collectGitEvidence(): Promise<DevelopmentEvidence["git"]> {
    if (dependencies.git === null) {
      return { available: false, changedFiles: [] };
    }
    try {
      const status = await dependencies.git.getStatus({});
      const changedFiles = [
        ...status.changes.map((change) => change.path),
        ...status.untracked,
      ].sort();
      return { available: true, changedFiles: [...new Set(changedFiles)] };
    } catch {
      return { available: false, changedFiles: [] };
    }
  }

  function finalizeResult(current: InternalSession): GDScriptDevelopmentResult {
    let status: import("@solaris/core").DevelopmentStatus;
    if (current.state.kind === "terminal") {
      status = current.state.status;
    } else if (current.validation === "clean") {
      status = "completed";
    } else if (current.validation === "warnings") {
      status = "completed_with_warnings";
    } else if (current.validation === "errors") {
      status = "completed_with_errors";
    } else if (current.validation === "infrastructure_failure") {
      status = "validation_failed";
    } else {
      status = "completed";
    }
    const diagnostics = current.evidence.flatMap((evidence) => [
      ...evidence.parser.diagnostics,
      ...evidence.lsp.diagnostics,
    ]);
    return {
      status,
      iterations: current.iteration,
      changes: current.changes,
      diagnostics: {
        errors: diagnostics.filter((entry) => entry.severity === "error").length,
        warnings: diagnostics.filter((entry) => entry.severity === "warning").length,
      },
      validation: {
        parser: current.evidence.every(
          (evidence) => evidence.parser.validFiles === evidence.parser.checkedFiles,
        ),
        lsp: current.evidence.every((evidence) => evidence.lsp.started),
        workspaceIntegrity: current.evidence.every(
          (evidence) => evidence.workspaceIntegrity.verified,
        ),
      },
      checkpointIds: [...current.checkpointIds],
    };
  }

  function toPublicSession(current: InternalSession): GDScriptDevelopmentSession {
    return {
      id: current.id,
      projectFingerprint: current.projectFingerprint,
      engineFingerprint: current.engineFingerprint,
      request: current.request,
      state: current.state,
      iteration: current.iteration,
      repairProposalsUsed: current.repairProposalsUsed,
      evidence: current.evidence,
    };
  }

  return {
    support,
    prepareStart,
    start,
    status,
    prepareChangeSet,
    applyChangeSet,
    languageQueryGate,
    validationStatus,
    completeFromProviderTurn,
    cancel,
    close,
  };
}

function buildApplyRequest(
  changeSetId: string,
  prepared: PreparedChangeSetRecord,
): import("@solaris/core").ChangeSetApplyRequest {
  return {
    changeSetId,
    toolName: "workspace.apply_text_changeset",
    files: prepared.files.map((file) => ({
      path: file.path,
      operation: file.operation,
      expectedSha256: file.expectedSha256,
      content: file.content,
      beforeSha256: file.beforeSha256,
      afterSha256: file.afterSha256,
      addedLines: file.addedLines,
      removedLines: file.removedLines,
    })),
  };
}

function changedScripts(
  files: readonly import("@solaris/core").PreparedChangeSetFile[],
): readonly string[] {
  return files.filter((file) => file.path.endsWith(".gd")).map((file) => file.path);
}

function countSeverity(evidence: DevelopmentEvidence, severity: "error" | "warning"): number {
  let count = 0;
  for (const diagnostic of evidence.parser.diagnostics) {
    if (diagnostic.severity === severity) {
      count += 1;
    }
  }
  for (const diagnostic of evidence.lsp.diagnostics) {
    if (diagnostic.severity === severity) {
      count += 1;
    }
  }
  return count;
}

/**
 * Paths whose current state differs from the approved change-set results:
 * changed paths not covered by `expectedAfter`, covered paths whose hash
 * differs, and covered paths that no longer exist. Deleted files carry the
 * `"absent"` marker and are expected to be absent from `expectedAfter`.
 */
function unexpectedChanges(
  changed: Map<string, string>,
  expectedAfter: Map<string, string>,
): readonly string[] {
  const unexpected = new Set<string>();
  for (const [path, sha256] of changed) {
    const expected = expectedAfter.get(path);
    if (expected === undefined || expected !== sha256) {
      unexpected.add(path);
    }
  }
  for (const [path] of expectedAfter) {
    if (!changed.has(path)) {
      unexpected.add(path);
    }
  }
  return [...unexpected].sort();
}

function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  if (ms <= 0) {
    return Promise.resolve();
  }
  return new Promise<void>((resolve) => {
    if (signal?.aborted) {
      resolve();
      return;
    }
    const timer = setTimeout(() => {
      signal?.removeEventListener("abort", onAbort);
      resolve();
    }, ms);
    const onAbort = (): void => {
      clearTimeout(timer);
      resolve();
    };
    signal?.addEventListener("abort", onAbort, { once: true });
  });
}
