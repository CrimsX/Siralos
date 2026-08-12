import { computeArtifactDigest } from "../identity/artifact-digest.js";
import { createOrderingPolicy } from "./context.js";

/**
 * Deterministic repository discovery and ownership resolution (Stage 3 —
 * Deterministic Execution & Reproducibility, ADR 0029).
 *
 * Baseline LLM repository discovery is repeatable: search/discovery →
 * classify relevance → normalize paths → stable rank/order → apply bounds.
 * Same task + same repository state → same initial deterministic
 * discovery result; filesystem enumeration order never decides the
 * baseline selection. The model may request additional exploration, but
 * the baseline is host-owned.
 */

export type DiscoveryRelevance = "verified" | "candidate";

export interface DiscoveryCandidate {
  readonly path: string;
  readonly relevance: DiscoveryRelevance;
  /** Bounded classification evidence (e.g. "exact task target", "direct import"). */
  readonly evidence: readonly string[];
  /** Rank signal: lower is more relevant (host-derived, never model claim). */
  readonly rank: number;
}

export interface DiscoveryResult {
  readonly candidates: readonly DiscoveryCandidate[];
  /** Canonical digest over the ordered candidates. */
  readonly digest: string;
}

export interface DiscoveryInput {
  readonly unorderedCandidates: readonly {
    readonly path: string;
    readonly relevance: DiscoveryRelevance;
    readonly evidence: readonly string[];
  }[];
  readonly maxCandidates: number;
  /** Normalized workspace-relative target paths of the task. */
  readonly taskTargets: readonly string[];
}

/**
 * Deterministic baseline discovery: exact task targets rank first
 * (strong evidence class), then verified, then candidate; ties break by
 * canonical path; bounds apply last. Shuffled input order cannot change
 * the result.
 */
export function discoverRepository(input: DiscoveryInput): DiscoveryResult {
  const ordering = createOrderingPolicy();
  const ranked: DiscoveryCandidate[] = input.unorderedCandidates.map((candidate) => {
    const exactTarget = input.taskTargets.includes(candidate.path);
    const strongEvidence =
      candidate.relevance === "verified" ||
      candidate.evidence.some((item) =>
        [
          "exact task target",
          "direct import",
          "structural relationship",
          "explicit architecture ownership",
          "test mapping",
        ].includes(item),
      );
    const rank = exactTarget ? 0 : strongEvidence ? 1 : 2;
    return {
      path: candidate.path,
      relevance: candidate.relevance,
      evidence: [...candidate.evidence],
      rank,
    };
  });
  const ordered = ordering.stableSort(ranked, (candidate) => {
    const rankKey = String(candidate.rank).padStart(4, "0");
    return `${rankKey}:${candidate.path}`;
  });
  const bounded = ordered.slice(0, input.maxCandidates);
  const digest = computeArtifactDigest({
    artifactType: "DiscoveryResult",
    schemaVersion: 1,
    payload: {
      candidates: bounded.map((candidate) => ({
        path: candidate.path,
        relevance: candidate.relevance,
        rank: candidate.rank,
        evidence: candidate.evidence,
      })),
    },
  }).value;
  return { candidates: bounded, digest };
}

// ---------------------------------------------------------------------------
// Ownership resolution
// ---------------------------------------------------------------------------

/**
 * Canonical subsystem ownership metadata (architecture/navigation only —
 * never a service registry, never capability). Before an executor creates
 * an overlapping abstraction, context identifies the existing owner.
 */
export interface OwnershipEntry {
  /** Canonical responsibility identifier, e.g. "tool projection". */
  readonly responsibility: string;
  /** Canonical owner module/class, e.g. "ToolProjector". */
  readonly owner: string;
  /** Source path of the owner. */
  readonly path: string;
  /** Related responsibilities that overlap and must reuse the owner. */
  readonly overlapsWith: readonly string[];
}

export const OWNERSHIP_INDEX: readonly OwnershipEntry[] = [
  {
    responsibility: "tool projection",
    owner: "ToolProjector",
    path: "packages/core/src/projection/tool-projector.ts",
    overlapsWith: ["provider tool schema"],
  },
  {
    responsibility: "context projection",
    owner: "ContextProjector",
    path: "packages/core/src/projection/context-projector.ts",
    overlapsWith: ["provider context assembly"],
  },
  {
    responsibility: "planning depth",
    owner: "PlanningPolicy",
    path: "packages/core/src/planning/planning-policy.ts",
    overlapsWith: ["plan routing"],
  },
  {
    responsibility: "task state",
    owner: "TaskRuntime",
    path: "packages/core/src/tasks/task-runtime.ts",
    overlapsWith: ["workflow state"],
  },
  {
    responsibility: "acceptance evaluation",
    owner: "AcceptanceEvaluator",
    path: "packages/core/src/executor/",
    overlapsWith: ["completion gate"],
  },
  {
    responsibility: "evidence",
    owner: "EvidenceStore",
    path: "packages/core/src/tasks/task-runtime-evidence.ts",
    overlapsWith: ["evidence records"],
  },
  {
    responsibility: "approval",
    owner: "ApprovalSystem",
    path: "packages/core/src/security/approval.ts",
    overlapsWith: ["mutation authorization"],
  },
  {
    responsibility: "checkpoints",
    owner: "CheckpointStore",
    path: "packages/adapters/src/checkpoints/",
    overlapsWith: ["undo", "recovery"],
  },
  {
    responsibility: "workspace revisions",
    owner: "WorkspaceRevisionRegistry",
    path: "packages/core/src/workspace/workspace-revision.ts",
    overlapsWith: ["source identity"],
  },
  {
    responsibility: "canonical digests",
    owner: "ArtifactDigest",
    path: "packages/core/src/identity/artifact-digest.ts",
    overlapsWith: ["hashing"],
  },
  {
    responsibility: "documentation selection",
    owner: "DocumentationSelection",
    path: "packages/core/src/executor/documentation-context.ts",
    overlapsWith: ["guidance selection"],
  },
  {
    responsibility: "executor briefing",
    owner: "ExecutorBriefCompiler",
    path: "packages/core/src/executor/brief-compiler.ts",
    overlapsWith: ["task briefing"],
  },
  {
    responsibility: "validation plan",
    owner: "ValidationPlan",
    path: "packages/core/src/determinism/decisions.ts",
    overlapsWith: ["validation selection"],
  },
  {
    responsibility: "retry policy",
    owner: "RetryPolicy",
    path: "packages/core/src/determinism/decisions.ts",
    overlapsWith: ["repair loop"],
  },
  {
    responsibility: "impact analysis",
    owner: "ImpactAnalyzer",
    path: "packages/core/src/godot/impact/impact-analyzer.ts",
    overlapsWith: ["review context"],
  },
  {
    responsibility: "independent review",
    owner: "ChangeReviewer",
    path: "packages/core/src/godot/quality/quality-review.ts",
    overlapsWith: ["quality gate"],
  },
  {
    responsibility: "surface routing",
    owner: "DevelopmentSurfaceClassifier",
    path: "packages/core/src/godot/development/development-surface.ts",
    overlapsWith: ["workflow routing"],
  },
  {
    responsibility: "nondeterminism audit",
    owner: "NondeterminismAudit",
    path: "scripts/check-nondeterminism.mjs",
    overlapsWith: ["architecture checks"],
  },
];

/** Deterministic ownership resolution: same responsibility → same owner. */
export function resolveOwner(responsibility: string): OwnershipEntry | null {
  const normalized = responsibility.trim().toLowerCase();
  const exact = OWNERSHIP_INDEX.find((entry) => entry.responsibility.toLowerCase() === normalized);
  if (exact !== undefined) {
    return exact;
  }
  return (
    OWNERSHIP_INDEX.find((entry) =>
      entry.overlapsWith.some((overlap) => overlap.toLowerCase() === normalized),
    ) ?? null
  );
}

/** Bounded deterministic listing of all canonical owners (stable order). */
export function listOwnership(): readonly OwnershipEntry[] {
  return createOrderingPolicy().stableSort(OWNERSHIP_INDEX, (entry) => entry.responsibility);
}
