import { computeArtifactDigest } from "../identity/artifact-digest.js";
import { createOrderingPolicy } from "../determinism/context.js";
import type { RunId } from "./identity.js";

/**
 * Runtime artifact model, budgets, and retention (Stage 3 — Runtime
 * Readiness & Operational Resilience, ADR 0031).
 *
 * Artifacts are REFERENCE-ONLY: large/binary contents never enter
 * TaskState, ActivityLog, ExecutorBrief, or ordinary model context.
 * Budgets behave deterministically at limits — truncate with explicit
 * metadata, stop capture, or produce an artifact_limit outcome; evidence
 * is never silently dropped while claiming complete capture.
 */

export type RuntimeArtifactKind =
  | "stdout"
  | "stderr"
  | "structured_log"
  | "screenshot"
  | "capture"
  | "profile"
  | "crash_info"
  | "other";

export type RetentionClass = "ephemeral" | "task" | "diagnostic" | "retained";

export interface RuntimeArtifactRef {
  readonly id: string;
  /** Content digest (H1 semantics: identity, not trust). */
  readonly digest: string;
  readonly runId: RunId;
  readonly kind: RuntimeArtifactKind;
  readonly mediaType: string;
  readonly size: number;
  readonly producer: string;
  readonly createdAtMs: number;
  readonly retentionClass: RetentionClass;
  /** Host-resolved location (never model-supplied). */
  readonly location: string;
  /** True when the artifact is truncated at a budget limit (explicit). */
  readonly truncated: boolean;
}

export interface RegisterArtifactInput {
  readonly id: string;
  readonly runId: RunId;
  readonly kind: RuntimeArtifactKind;
  readonly mediaType: string;
  readonly size: number;
  readonly producer: string;
  readonly createdAtMs: number;
  readonly retentionClass?: RetentionClass;
  readonly location: string;
  readonly digest: string;
  readonly truncated?: boolean;
}

export function createRuntimeArtifactRef(input: RegisterArtifactInput): RuntimeArtifactRef {
  if (input.id.length === 0 || input.location.length === 0) {
    throw new Error("A runtime artifact requires an id and a location.");
  }
  if (!Number.isSafeInteger(input.size) || input.size < 0) {
    throw new Error("A runtime artifact size must be a non-negative safe integer.");
  }
  if (!/^[0-9a-f]{64}$/.test(input.digest)) {
    throw new Error("A runtime artifact requires a 64-hex content digest.");
  }
  return {
    id: input.id,
    digest: input.digest,
    runId: input.runId,
    kind: input.kind,
    mediaType: input.mediaType,
    size: input.size,
    producer: input.producer,
    createdAtMs: input.createdAtMs,
    retentionClass: input.retentionClass ?? "ephemeral",
    location: input.location,
    truncated: input.truncated ?? false,
  };
}

export interface ArtifactBudget {
  /** Maximum bytes of one artifact (0 = unlimited). */
  readonly maxArtifactBytes: number;
  /** Maximum artifact count per run. */
  readonly maxArtifactsPerRun: number;
  /** Maximum aggregate artifact bytes per run. */
  readonly maxAggregateBytesPerRun: number;
  /** Maximum retained artifact bytes per task (retention >= task). */
  readonly maxRetainedBytesPerTask: number;
}

export const DEFAULT_RUNTIME_ARTIFACT_BUDGET: ArtifactBudget = Object.freeze({
  maxArtifactBytes: 4 * 1024 * 1024,
  maxArtifactsPerRun: 128,
  maxAggregateBytesPerRun: 64 * 1024 * 1024,
  maxRetainedBytesPerTask: 256 * 1024 * 1024,
});

export interface ArtifactBudgetState {
  readonly artifactCount: number;
  readonly aggregateBytes: number;
}

export type ArtifactAdmission =
  | { readonly status: "admit"; readonly truncated: boolean }
  | { readonly status: "artifact_limit"; readonly reason: string };

/**
 * Deterministic budget enforcement: equivalent state + incoming artifact
 * produce the same admission decision. Never silently drops evidence —
 * an exceeded limit yields an explicit artifact_limit outcome.
 */
export function enforceArtifactBudget(input: {
  readonly budget: ArtifactBudget;
  readonly state: ArtifactBudgetState;
  readonly incomingSize: number;
  readonly incomingCount: number;
}): ArtifactAdmission {
  if (
    input.budget.maxArtifactsPerRun > 0 &&
    input.state.artifactCount + input.incomingCount > input.budget.maxArtifactsPerRun
  ) {
    return {
      status: "artifact_limit",
      reason: `artifact count would exceed ${input.budget.maxArtifactsPerRun} per run`,
    };
  }
  if (
    input.budget.maxAggregateBytesPerRun > 0 &&
    input.state.aggregateBytes + input.incomingSize > input.budget.maxAggregateBytesPerRun
  ) {
    return {
      status: "artifact_limit",
      reason: `aggregate artifact bytes would exceed ${input.budget.maxAggregateBytesPerRun}`,
    };
  }
  if (input.budget.maxArtifactBytes > 0 && input.incomingSize > input.budget.maxArtifactBytes) {
    // Truncate with explicit metadata: the artifact is admitted bounded.
    return { status: "admit", truncated: true };
  }
  return { status: "admit", truncated: false };
}

/** In-memory artifact store (host-owned; Stage 4 may back it durably). */
export interface RuntimeArtifactStore {
  register(
    ref: RuntimeArtifactRef,
  ): { readonly status: "registered" } | { readonly status: "limit"; readonly reason: string };
  list(runId: RunId): readonly RuntimeArtifactRef[];
  count(): number;
  aggregateBytes(): number;
  /** Deterministic digest over the store's refs (evidence identity). */
  digest(): string;
}

export function createRuntimeArtifactStore(
  options: {
    readonly budget?: ArtifactBudget;
    readonly now?: () => number;
  } = {},
): RuntimeArtifactStore {
  const budget = options.budget ?? DEFAULT_RUNTIME_ARTIFACT_BUDGET;
  const refs: RuntimeArtifactRef[] = [];
  return {
    register(ref) {
      const admission = enforceArtifactBudget({
        budget,
        state: {
          artifactCount: refs.length,
          aggregateBytes: refs.reduce((sum, entry) => sum + entry.size, 0),
        },
        incomingSize: ref.size,
        incomingCount: 1,
      });
      if (admission.status === "artifact_limit") {
        return { status: "limit", reason: admission.reason };
      }
      refs.push(admission.truncated ? { ...ref, truncated: true } : ref);
      return { status: "registered" };
    },
    list(runId) {
      return createOrderingPolicy().stableSort(
        refs.filter((ref) => ref.runId === runId),
        (ref) => ref.id,
      );
    },
    count: () => refs.length,
    aggregateBytes: () => refs.reduce((sum, ref) => sum + ref.size, 0),
    digest: () =>
      computeArtifactDigest({
        artifactType: "RuntimeArtifactStore",
        schemaVersion: 1,
        payload: { refs: createOrderingPolicy().stableSort(refs, (ref) => ref.id) },
      }).value,
  };
}

// ---------------------------------------------------------------------------
// Retention / cleanup
// ---------------------------------------------------------------------------

export type CleanupStatus = "cleaned" | "partial" | "failed" | "skipped";

export interface CleanupOutcome {
  readonly status: CleanupStatus;
  readonly cleanedPaths: readonly string[];
  readonly failedPaths: readonly string[];
  /** Observable cleanup failure — never hidden by the primary result. */
  readonly message: string | null;
}

/**
 * Deterministic run-scoped cleanup decision over host-owned roots.
 * Idempotent by construction: cleaning an already-cleaned root reports
 * cleaned with no competing flows.
 */
export function planRunCleanup(input: {
  readonly hostRoots: readonly string[];
  readonly pathsAlreadyCleaned: Readonly<Set<string>>;
}): { readonly toClean: readonly string[]; readonly alreadyClean: readonly string[] } {
  const toClean = input.hostRoots.filter((root) => !input.pathsAlreadyCleaned.has(root));
  const alreadyClean = input.hostRoots.filter((root) => input.pathsAlreadyCleaned.has(root));
  return { toClean, alreadyClean };
}

/** Projection of a cleanup outcome (bounded). */
export function renderCleanupOutcome(outcome: CleanupOutcome): string {
  return `cleanup ${outcome.status}${outcome.message === null ? "" : `: ${outcome.message}`} (cleaned ${outcome.cleanedPaths.length}, failed ${outcome.failedPaths.length})`;
}

// ---------------------------------------------------------------------------
// Artifact context discipline
// ---------------------------------------------------------------------------

/**
 * Bounded semantic projection of runtime artifacts for model context:
 * references + summaries + explicit truncation markers. Raw megabytes of
 * logs never enter context.
 */
export function projectRuntimeArtifactsForContext(
  refs: readonly RuntimeArtifactRef[],
  maxEntries = 16,
): string {
  const ordered = createOrderingPolicy()
    .stableSort(refs, (ref) => ref.id)
    .slice(0, maxEntries);
  const lines = ordered.map((ref) => {
    const truncated = ref.truncated ? " (truncated at budget limit)" : "";
    return `${ref.kind} ${ref.id} ${ref.size}B ${ref.digest.slice(0, 12)}\u2026${truncated}`;
  });
  const dropped = refs.length - ordered.length;
  return (
    lines.join("\n") +
    (dropped > 0 ? `\n[${dropped} further artifact reference(s) not projected]` : "")
  );
}
