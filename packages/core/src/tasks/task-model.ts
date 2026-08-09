import type { AcceptanceCriterionId } from "./task-contract.js";
import type {
  ReferenceAlias,
  ReferenceId,
  ReferenceRevision,
} from "../reference/reference-model.js";
import type { ResearchSourceRef } from "../research/research-model.js";

/**
 * Authoritative application-owned task state (Stage 3 milestone 1).
 *
 * TaskState is a materialized, serializable working-state object. It is
 * produced exclusively by the host-owned TaskRuntime: every other
 * component receives immutable snapshots, projections, or events. It never
 * stores private chain-of-thought, provider continuation internals, or
 * secrets, and evidence references point at already-owned artifacts
 * instead of duplicating raw adapter output.
 */

export type TaskId = string;

/** Host-controlled task phases. */
export type TaskPhase =
  | "prepared"
  | "working"
  | "validating"
  | "reviewing"
  | "blocked"
  | "completed"
  | "cancelled"
  | "failed";

export type TaskStepId = string;

export type TaskStepStatus = "pending" | "active" | "completed" | "failed" | "blocked";

/**
 * Step kind drives the minimal extensible evidence rule boundary: each
 * step declares the evidence kinds it accepts, so completion never
 * hard-codes "every step must have a mutation" — a research/review step
 * completes on read/review evidence instead.
 */
export type TaskStepKind = "research" | "implementation" | "review";

export type EvidenceKind =
  | "workspace_read"
  | "api_lookup"
  | "lsp_query"
  | "change_preview"
  | "mutation_receipt"
  | "checkpoint"
  | "parser_result"
  | "lsp_result"
  | "validation_result"
  | "review_result"
  | "reference_read"
  | "reference_search"
  | "research";

/**
 * Bounded evidence source. Evidence points at already-owned artifacts
 * (change-set ids, checkpoint ids, counts, digests) and never embeds raw
 * adapter output such as full diagnostics, diffs, or provider responses.
 */
export type EvidenceSource =
  | {
      readonly type: "mutation";
      readonly changeSetId: string;
      readonly checkpointId: string | null;
      /** Post-edit revision handle of the primary changed file, when known. */
      readonly revision?: string;
    }
  | { readonly type: "checkpoint"; readonly checkpointId: string }
  | {
      readonly type: "parser";
      readonly checkedFiles: number;
      readonly validFiles: number;
      readonly errors: number;
    }
  | {
      readonly type: "lsp";
      readonly diagnosticCount: number;
      readonly errors: number;
      readonly warnings: number;
    }
  | {
      readonly type: "validation";
      readonly outcome: string;
      readonly workspaceIntegrityVerified: boolean;
      readonly unexpectedChanges: number;
    }
  | { readonly type: "review"; readonly status: string; readonly blockingFindings: number }
  | { readonly type: "change_preview"; readonly changeSetId: string }
  | {
      readonly type: "workspace_read";
      readonly paths: readonly string[];
      /** Revision handle of the inspected file state, when known. */
      readonly revision?: string;
    }
  | { readonly type: "api_lookup"; readonly symbol: string }
  | { readonly type: "lsp_query"; readonly query: string }
  | {
      readonly type: "reference_read";
      readonly referenceId: ReferenceId;
      readonly alias: ReferenceAlias;
      /** Registry-bound revision at read time; historical revisions stay immutable. */
      readonly revision: ReferenceRevision;
      readonly path: string;
      readonly mode: "exact" | "structural" | "summary";
      readonly sha256: string;
    }
  | {
      readonly type: "reference_search";
      readonly referenceId: ReferenceId;
      readonly alias: ReferenceAlias;
      readonly revision: ReferenceRevision;
      readonly query: string;
      readonly matchCount: number;
    }
  | {
      readonly type: "research";
      readonly source: ResearchSourceRef;
      readonly requestId: string;
      readonly fetchedAtMs: number;
      readonly resolvedRevision: string | null;
      readonly version: string | null;
      readonly fallback: boolean;
      /** Bounded first-section excerpt (service-bounded; attach-time byte checks still apply). */
      readonly excerpt: string;
      readonly truncated: boolean;
    };

export interface EvidenceRecord {
  readonly id: string;
  readonly kind: EvidenceKind;
  readonly taskId: TaskId;
  readonly source: EvidenceSource;
  readonly attachedAtMs: number;
}

/** A step's reference to already-attached evidence of its task. */
export interface EvidenceRef {
  readonly evidenceId: string;
  readonly kind: EvidenceKind;
}

/** Bounded step specification authored by the host at task creation. */
export interface TaskStepSpec {
  readonly id: TaskStepId;
  readonly description: string;
  readonly kind: TaskStepKind;
  /** Evidence kinds this step accepts; completion requires at least one. */
  readonly accepts: readonly EvidenceKind[];
}

export interface TaskStepState {
  readonly id: TaskStepId;
  readonly description: string;
  readonly kind: TaskStepKind;
  readonly status: TaskStepStatus;
  readonly evidenceRefs: readonly EvidenceRef[];
  readonly failedReason: string | null;
  readonly blockedReason: string | null;
}

export type AcceptanceStatus = "pending" | "satisfied" | "failed";

export interface AcceptanceState {
  readonly criterionId: AcceptanceCriterionId;
  readonly description: string;
  readonly verificationKind: "deterministic" | "review" | "user";
  readonly status: AcceptanceStatus;
  /** Evidence id that satisfied the criterion, when verified with one. */
  readonly verifiedBy: string | null;
  readonly note: string | null;
}

export type FindingSeverity = "critical" | "high" | "medium" | "low";

/** Evidence-backed finding reference; never hidden model reasoning. */
export interface FindingRef {
  readonly findingId: string;
  readonly severity: FindingSeverity;
  readonly source: string;
}

/**
 * Validation status of the task's deterministic gates. `incomplete` means
 * a required gate could not run or was denied — it must never be treated
 * as success (validation unavailable/denied stays validation incomplete).
 */
export type TaskValidationStatus = "not_run" | "clean" | "warnings" | "failed" | "incomplete";

export type TaskReviewStatus = "not_run" | "clean" | "findings" | "incomplete";

/**
 * Host-observed progress state: distinguishes productive execution from
 * repeated no-progress loops. Progress is based on new host-observed
 * useful state, never merely another model turn or another identical
 * action with an identical result.
 */
export type ProgressStateValue = "healthy" | "degraded" | "stalled";

export interface ProgressState {
  readonly state: ProgressStateValue;
  /** Distinct useful observations accepted so far. */
  readonly usefulObservations: number;
  /** Current run of identical (action, result) observations. */
  readonly repeatedActions: number;
  readonly lastProgressAtMs: number | null;
  /** When the stalled state was first entered; null while not stalled. */
  readonly stalledAtMs: number | null;
}

export interface TaskState {
  readonly taskId: TaskId;
  readonly contractRevision: number;
  readonly phase: TaskPhase;
  readonly steps: readonly TaskStepState[];
  readonly acceptance: readonly AcceptanceState[];
  readonly currentFindings: readonly FindingRef[];
  /** Bounded evidence records attached to this task (references only). */
  readonly evidence: readonly EvidenceRecord[];
  readonly validationStatus: TaskValidationStatus;
  readonly reviewStatus: TaskReviewStatus;
  /** Host-observed development/workflow iteration count. */
  readonly iteration: number;
  readonly progress: ProgressState;
  readonly startedAtMs: number;
  readonly completedAtMs: number | null;
  readonly terminalReason: string | null;
}

/**
 * Structured workflow disposition: a request, never an authoritative
 * mutation. `"complete"` is a completion *request* that still goes through
 * the host completion gate; `"blocked"` preserves a clear reason.
 */
export type WorkflowDisposition =
  | { readonly type: "continue"; readonly nextAction?: string }
  | { readonly type: "complete" }
  | { readonly type: "blocked"; readonly reason: string };

export function isTerminalPhase(phase: TaskPhase): boolean {
  return phase === "completed" || phase === "cancelled" || phase === "failed";
}

/** Deep copy of the materialized state for immutable snapshot delivery. */
export function cloneTaskState(state: TaskState): TaskState {
  return structuredClone(state);
}
