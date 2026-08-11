import type {
  GDScriptDevelopmentPreview,
  GDScriptDevelopmentResult,
  GDScriptDevelopmentStatus,
  DevelopmentEvent,
} from "../godot/development/development-model.js";
import type { QualityStatus } from "../godot/quality/quality-model.js";
import type { AcceptanceCriterion, TaskContract } from "./task-contract.js";
import { createTaskContract } from "./task-contract.js";
import type {
  EvidenceKind,
  EvidenceRef,
  FindingRef,
  TaskId,
  TaskState,
  TaskStepSpec,
} from "./task-model.js";
import type { TaskHandle, TaskRuntime } from "./task-runtime.js";
import type { TaskRuntimeSnapshotSources } from "./task-snapshot.js";
import { createTaskRuntimeSnapshot } from "./task-snapshot.js";

/**
 * GDScript development workflow <-> task runtime bridge (Stage 3
 * milestone 1).
 *
 * Core owns the mapping: the development request becomes a revisioned
 * TaskContract with explicit acceptance criteria (user approval, applied
 * mutation, workspace scope, parser, fresh-LSP, independent review), the
 * bounded step plan, the immutable runtime snapshot, and the host-observed
 * event mapping into the TaskRuntime. The existing Stage 2 quality gates
 * remain authoritative — the task completion gate references the same
 * deterministic results instead of duplicating them, and a model-issued
 * "complete" never bypasses them.
 *
 * The flow is a host component: it only ever mutates the task through the
 * TaskRuntime handle API. The provider never touches TaskState.
 */

export const DEVELOPMENT_WORKFLOW_ID = "gdscript-development";
export const DEVELOPMENT_WORKFLOW_VERSION = "development-model-1";

/** Development steps with their evidence-acceptance rule boundary. */
export function createDevelopmentTaskSteps(): readonly TaskStepSpec[] {
  return [
    {
      id: "investigate",
      description: "Investigate the workspace, API knowledge, and request",
      kind: "research",
      accepts: [
        "workspace_read",
        "api_lookup",
        "lsp_query",
        "change_preview",
        "reference_read",
        "reference_search",
        "research",
      ],
    },
    {
      id: "propose",
      description: "Propose an exact, prepared change set",
      kind: "implementation",
      accepts: ["change_preview"],
    },
    {
      id: "apply",
      description: "Apply the approved change set with checkpoints",
      kind: "implementation",
      accepts: ["mutation_receipt", "checkpoint"],
    },
    {
      id: "validate",
      description: "Validate the change: parser and fresh LSP diagnostics",
      kind: "implementation",
      accepts: ["parser_result", "lsp_result", "validation_result"],
    },
    {
      id: "review",
      description: "Independent review of the change",
      kind: "review",
      accepts: ["review_result"],
    },
  ];
}

const DEVELOPMENT_CONSTRAINTS = [
  {
    id: "workspace-scope",
    kind: "scope" as const,
    description: "All changes are contained within the workspace root.",
  },
  {
    id: "no-network",
    kind: "security" as const,
    description: "Network egress is denied; the workflow runs offline.",
  },
  {
    id: "no-game-execution",
    kind: "security" as const,
    description: "Game execution is disabled; only headless check-only validation runs.",
  },
  {
    id: "per-change-set-approval",
    kind: "process" as const,
    description: "Every source change set requires its own exact one-time approval.",
  },
  {
    id: "pause-on-approval",
    kind: "escalation" as const,
    description: "The task pauses while a change-set approval is pending.",
  },
];

/** Explicit, individually trackable acceptance criteria (§25). */
export function createDevelopmentAcceptanceCriteria(): readonly AcceptanceCriterion[] {
  return [
    {
      id: "user-approval",
      description: "Proposed change sets are approved by the user.",
      verificationKind: "user",
    },
    {
      id: "mutation-applied",
      description: "The requested mutation is applied.",
      verificationKind: "deterministic",
    },
    {
      id: "scope-verified",
      description: "No unexpected workspace changes were introduced.",
      verificationKind: "deterministic",
    },
    {
      id: "parses",
      description: "Changed GDScript parses cleanly.",
      verificationKind: "deterministic",
    },
    {
      id: "lsp-clean",
      description: "A fresh LSP session reports no errors on the changed files.",
      verificationKind: "deterministic",
    },
    {
      id: "review-clean",
      description: "Independent review has no unresolved blocking finding.",
      verificationKind: "review",
    },
  ];
}

export function createDevelopmentTaskContract(id: TaskId, request: string): TaskContract {
  return createTaskContract({
    id,
    request,
    constraints: DEVELOPMENT_CONSTRAINTS,
    acceptanceCriteria: createDevelopmentAcceptanceCriteria(),
    pausePolicy: "on_approval",
  });
}

export interface DevelopmentTaskFlowOptions {
  readonly runtime: TaskRuntime;
  /** Base snapshot sources (provider, sandbox, policy, workspace). */
  readonly sources: TaskRuntimeSnapshotSources;
  readonly now?: () => number;
  readonly idFactory?: () => string;
  /**
   * Host-owned per-task snapshot extras (executor briefing foundation):
   * called once at task start so the immutable runtime snapshot records
   * execution-contract/manifest identity and the initial brief
   * fingerprint for reproducibility.
   */
  readonly snapshotExtras?: (input: {
    readonly taskId: TaskId;
    readonly contract: TaskContract;
  }) => Partial<TaskRuntimeSnapshotSources> | null;
}

export interface DevelopmentTaskFlow {
  readonly taskId: TaskId | null;
  /** Create the task for a started development workflow. */
  start(request: string, preview: GDScriptDevelopmentPreview, digest: string | null): TaskState;
  /** Feed one host-observed development event into the task. */
  handleEvent(event: DevelopmentEvent): void;
  /** Host evaluation when the development workflow reaches a terminal state. */
  finish(
    status: GDScriptDevelopmentStatus,
    result: GDScriptDevelopmentResult | null,
  ): TaskState | null;
  current(): TaskState | null;
}

const QUALITY_OK: readonly QualityStatus[] = ["passed", "passed_with_advisories"];

export function createDevelopmentTaskFlow(
  options: DevelopmentTaskFlowOptions,
): DevelopmentTaskFlow {
  const runtime = options.runtime;
  const now = options.now ?? Date.now;
  let taskCounter = 0;
  let evidenceCounter = 0;
  let handle: TaskHandle | null = null;
  let lastApprovedChangeSetId: string | null = null;
  const previewEvidenceIds: {
    preview: string;
    mutation: string | null;
    parser: string | null;
    lsp: string | null;
    review: string | null;
  } = {
    preview: "",
    mutation: null,
    parser: null,
    lsp: null,
    review: null,
  };

  function nextEvidenceId(prefix: string): string {
    evidenceCounter += 1;
    return `ev-${prefix}-${evidenceCounter}`;
  }

  function nextTaskId(): TaskId {
    if (options.idFactory !== undefined) {
      return options.idFactory();
    }
    // A new bridge instance is created for each workflow run, so its local
    // counter alone is not a runtime-wide identity. Advance past existing
    // records rather than replacing or colliding with prior task history.
    let candidate: TaskId;
    do {
      taskCounter += 1;
      candidate = `task-dev-${taskCounter}`;
    } while (runtime.getTask(candidate) !== null);
    return candidate;
  }

  function attach(
    kind: EvidenceKind,
    source: TaskState["evidence"][number]["source"],
  ): string | null {
    if (handle === null) {
      return null;
    }
    const id = nextEvidenceId(kind.replace(/_/g, "-"));
    const result = handle.attachEvidence({ id, kind, source });
    return result.status === "attached" ? id : null;
  }

  function completeStep(stepId: string, refs: readonly EvidenceRef[]): void {
    handle?.completeStep(stepId, refs);
  }

  function beginStep(stepId: string): void {
    handle?.beginStep(stepId);
  }

  function transitionTo(phase: TaskState["phase"]): void {
    if (handle === null) {
      return;
    }
    const current = handle.snapshot().phase;
    if (current === phase) {
      return;
    }
    handle.transitionPhase(phase);
  }

  function verify(criterionId: string, verifiedBy: string | null): void {
    handle?.verifyCriterion(criterionId, verifiedBy);
  }

  return {
    get taskId(): TaskId | null {
      return handle?.taskId ?? null;
    },

    start(request, preview, digest): TaskState {
      if (handle !== null) {
        return handle.snapshot();
      }
      evidenceCounter = 0;
      const taskId = nextTaskId();
      const contract = createDevelopmentTaskContract(taskId, request);
      const snapshot = createTaskRuntimeSnapshot(
        {
          ...options.sources,
          godotEngineFingerprint: preview.engineFingerprint,
          workflow: {
            id: DEVELOPMENT_WORKFLOW_ID,
            version: DEVELOPMENT_WORKFLOW_VERSION,
            digest,
          },
          ...(options.snapshotExtras?.({
            taskId,
            contract,
          }) ?? {}),
        },
        now,
      );
      handle = runtime.createTask({ contract, snapshot, steps: createDevelopmentTaskSteps() });
      handle.transitionPhase("working");
      beginStep("investigate");
      return handle.snapshot();
    },

    handleEvent(event): void {
      if (handle === null) {
        return;
      }
      switch (event.type) {
        case "development_started":
        case "development_investigating":
          transitionTo("working");
          break;
        case "development_change_prepared":
          transitionTo("working");
          beginStep("propose");
          break;
        case "development_change_approved": {
          transitionTo("working");
          lastApprovedChangeSetId = event.changeSetId;
          const previewId = attach("change_preview", {
            type: "change_preview",
            changeSetId: event.changeSetId,
          });
          if (previewId !== null) {
            previewEvidenceIds.preview = previewId;
            verify("user-approval", previewId);
            completeStep("investigate", [{ evidenceId: previewId, kind: "change_preview" }]);
            completeStep("propose", [{ evidenceId: previewId, kind: "change_preview" }]);
          }
          beginStep("apply");
          break;
        }
        case "development_change_applied": {
          transitionTo("working");
          const firstRevision = event.revisions?.[0];
          const mutationId = attach("mutation_receipt", {
            type: "mutation",
            changeSetId: lastApprovedChangeSetId ?? "<applied>",
            checkpointId: null,
            ...(firstRevision === undefined ? {} : { revision: firstRevision.revision }),
          });
          if (mutationId !== null) {
            previewEvidenceIds.mutation = mutationId;
            verify("mutation-applied", mutationId);
            completeStep("apply", [{ evidenceId: mutationId, kind: "mutation_receipt" }]);
          }
          beginStep("validate");
          break;
        }
        case "development_validation_started":
          transitionTo("validating");
          break;
        case "development_parser_completed": {
          transitionTo("validating");
          const parserId = attach("parser_result", {
            type: "parser",
            checkedFiles: event.checkedFiles,
            validFiles: event.validFiles,
            errors: event.checkedFiles - event.validFiles,
          });
          if (parserId !== null) {
            previewEvidenceIds.parser = parserId;
            if (event.checkedFiles - event.validFiles === 0) {
              verify("parses", parserId);
            }
          }
          break;
        }
        case "development_validation_completed": {
          transitionTo("reviewing");
          const lspId = attach("lsp_result", {
            type: "lsp",
            diagnosticCount: event.errors + event.warnings,
            errors: event.errors,
            warnings: event.warnings,
          });
          if (lspId !== null) {
            previewEvidenceIds.lsp = lspId;
            if (event.errors === 0) {
              verify("lsp-clean", lspId);
            }
          }
          const refs: EvidenceRef[] = [];
          if (previewEvidenceIds.parser !== null) {
            refs.push({ evidenceId: previewEvidenceIds.parser, kind: "parser_result" });
          }
          if (previewEvidenceIds.lsp !== null) {
            refs.push({ evidenceId: previewEvidenceIds.lsp, kind: "lsp_result" });
          }
          if (refs.length > 0) {
            completeStep("validate", refs);
          }
          beginStep("review");
          break;
        }
        case "development_repair_requested":
          transitionTo("working");
          break;
        case "quality_started":
        case "review_started":
          transitionTo("reviewing");
          break;
        case "review_completed": {
          const blocking = event.critical + event.high;
          const reviewId = attach("review_result", {
            type: "review",
            status: blocking > 0 ? "findings" : "clean",
            blockingFindings: blocking,
          });
          if (reviewId !== null) {
            previewEvidenceIds.review = reviewId;
          }
          const findings: FindingRef[] = [];
          if (event.critical > 0) {
            findings.push({
              findingId: "review:critical",
              severity: "critical",
              source: "independent-review",
            });
          }
          if (event.high > 0) {
            findings.push({
              findingId: "review:high",
              severity: "high",
              source: "independent-review",
            });
          }
          if (event.medium > 0) {
            findings.push({
              findingId: "review:medium",
              severity: "medium",
              source: "independent-review",
            });
          }
          if (event.low > 0) {
            findings.push({
              findingId: "review:low",
              severity: "low",
              source: "independent-review",
            });
          }
          handle.setFindings(findings);
          handle.setReviewStatus(blocking > 0 ? "findings" : "clean");
          break;
        }
        case "quality_gate_completed":
        case "development_language_suspending":
        case "development_language_suspended":
        case "development_language_restarted":
        case "development_completed":
          break;
        case "quality_completed": {
          if (QUALITY_OK.includes(event.status)) {
            if (previewEvidenceIds.review !== null) {
              verify("review-clean", previewEvidenceIds.review);
              completeStep("review", [
                { evidenceId: previewEvidenceIds.review, kind: "review_result" },
              ]);
            }
            handle.setReviewStatus("clean");
          } else if (event.status === "blocking_findings") {
            handle.setReviewStatus("findings");
          } else if (event.status === "validation_incomplete") {
            handle.setReviewStatus("incomplete");
          } else if (event.status === "cancelled" || event.status === "failed") {
            handle.setReviewStatus(event.status === "cancelled" ? "incomplete" : "findings");
          }
          break;
        }
      }
    },

    finish(status, result): TaskState | null {
      if (handle === null) {
        return null;
      }
      const session = status.session;
      if (session !== null && session.state.kind === "terminal") {
        handle.setIteration(session.iteration);
        const terminal = session.state.status;
        if (terminal === "completed" || terminal === "completed_with_warnings") {
          handle.setValidationStatus(session.validation === "warnings" ? "warnings" : "clean");
          if (result !== null) {
            if (result.changes.length > 0 && previewEvidenceIds.mutation !== null) {
              verify("mutation-applied", previewEvidenceIds.mutation);
            } else if (result.changes.length === 0) {
              handle.verifyCriterion("mutation-applied", null, "No source changes were required.");
            }
            if (result.validation.parser && previewEvidenceIds.parser !== null) {
              verify("parses", previewEvidenceIds.parser);
            }
            if (result.validation.lsp && previewEvidenceIds.lsp !== null) {
              verify("lsp-clean", previewEvidenceIds.lsp);
            }
            if (result.validation.workspaceIntegrity) {
              const scopeId = attach("validation_result", {
                type: "validation",
                outcome: "verified",
                workspaceIntegrityVerified: true,
                unexpectedChanges: 0,
              });
              if (scopeId !== null) {
                verify("scope-verified", scopeId);
              }
            }
            const quality = result.quality;
            if (quality !== null && QUALITY_OK.includes(quality.status)) {
              if (previewEvidenceIds.review !== null) {
                verify("review-clean", previewEvidenceIds.review);
              }
              handle.setReviewStatus("clean");
            }
          }
          const completion = handle.completeTask();
          if (completion.status === "rejected") {
            handle.markBlocked(
              `Workflow completed but the task gate rejected completion: ${completion.reasons.join("; ")}`,
            );
          }
        } else if (terminal === "completed_with_blocking_findings") {
          handle.setReviewStatus("findings");
          handle.fail(
            "The development workflow completed with unresolved blocking review findings.",
          );
        } else if (terminal === "completed_with_errors" || terminal === "validation_failed") {
          handle.setValidationStatus("failed");
          handle.fail(`The development workflow did not pass validation (${terminal}).`);
        } else if (terminal === "quality_gate_failed") {
          handle.setReviewStatus("findings");
          handle.fail("The development workflow failed the quality gate.");
        } else if (terminal === "cancelled") {
          handle.cancel("The development workflow was cancelled.");
        } else if (
          terminal === "denied" ||
          terminal === "conflict" ||
          terminal === "apply_failed"
        ) {
          handle.fail(`The development workflow ended with ${terminal}.`);
        } else if (terminal === "unavailable") {
          handle.setValidationStatus("incomplete");
          handle.fail(
            "The development workflow infrastructure is unavailable; validation is incomplete.",
          );
        }
      }
      return handle.snapshot();
    },

    current(): TaskState | null {
      return handle === null ? null : handle.snapshot();
    },
  };
}
