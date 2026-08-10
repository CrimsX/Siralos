import { truncateText } from "../projection/evidence-projector.js";
import type { CapabilityPolicy } from "../security/capability.js";
import { evaluatePermission } from "../security/permission-evaluator.js";
import type { SandboxProfile } from "../security/profile.js";
import {
  RESEARCH_LIMITS,
  defaultResearchBounds,
  validateResearchRequest,
  type ResearchBounds,
  type ResearchOutcome,
  type ResearchRequest,
  type ResearchSourceKind,
} from "./research-model.js";
import type { ResearchSourcePort } from "./research-ports.js";
import { createResearchEvidenceStore } from "./research-evidence-store.js";
import type {
  ResearchEvidence,
  ResearchFetchResult,
  ResearchTaskBinding,
} from "./research-service-model.js";
import {
  composeResearchSignal,
  createResearchRequestId,
  describeResearchError,
  isValidResearchTaskBinding,
  normalizeBoundedInteger,
  normalizeResearchBounds,
  raceWithResearchSignal,
  sameResearchTaskBinding,
} from "./research-service-support.js";

export type {
  ResearchEvidence,
  ResearchFetchResult,
  ResearchTaskBinding,
} from "./research-service-model.js";

/**
 * ResearchService (Stage 3 milestone 5) — the application-owned research
 * coordinator.
 *
 * The service is the single gate for research fetches:
 *
 *   1. Policy gate FIRST: the `research.fetch` capability must evaluate to
 *      `allow` (evaluatePermission). There is no approval protocol for
 *      research in this milestone, so `ask` is refused too. The source port
 *      is NEVER invoked when the gate does not allow — this is the
 *      effect-test contract.
 *   2. The untrusted request is validated against the bounded model.
 *   3. The source must be one of the configured sources (kind+id or label);
 *      unknown sources are refused.
 *   4. The fetch races the caller's abort signal and a timeout (default
 *      `bounds.timeoutMs`); `hardLifetimeMs` is communicated to the source
 *      as its absolute ceiling. Timeout -> `timeout`, abort -> `cancelled`.
 *      No indefinite waits.
 *
 * Stale-result binding is enforced inside the service. Every fetch records
 * a unique request id and captures the active task id + TaskContract
 * revision before invoking a source. That identity is checked again before
 * returning or retaining a document; stale results are discarded before
 * they can enter evidence or context, so callers cannot omit the check.
 *
 * Research never becomes knowledge automatically: the KnowledgeCoordinator
 * only accepts `research_evidence` provenance through an explicit
 * `propose` call with host verification.
 */

export interface ResearchServiceOptions {
  readonly policy: CapabilityPolicy;
  readonly profile: SandboxProfile;
  readonly sources: readonly ResearchSourcePort[];
  /** Current active task identity; null means research must fail closed. */
  readonly currentTask: () => ResearchTaskBinding | null;
  readonly bounds?: ResearchBounds;
  /** Total retained evidence-excerpt budget for the ring. */
  readonly maxEvidenceBytes?: number;
}

export interface ResearchService {
  fetch(
    request: ResearchRequest,
    opts?: { readonly signal?: AbortSignal },
  ): Promise<ResearchFetchResult>;
  /** Retained evidence views, oldest first (bounded ring). */
  latestEvidence(): readonly ResearchEvidence[];
  /** In-flight fetch count (CLI /research-status). */
  activeRequestCount(): number;
  /** Configured source kinds, in configuration order (deduplicated). */
  sourceKinds(): readonly ResearchSourceKind[];
}

export const DEFAULT_RESEARCH_VIEW_MAX_BYTES = 2 * 1024;

export function createResearchService(options: ResearchServiceOptions): ResearchService {
  const bounds = normalizeResearchBounds(options.bounds ?? defaultResearchBounds());
  const sources = [...options.sources];
  const maxEvidenceBytes = normalizeBoundedInteger(
    options.maxEvidenceBytes,
    RESEARCH_LIMITS.maxRetainedEvidenceViews * RESEARCH_LIMITS.maxResearchEvidenceExcerptBytes,
    0,
    RESEARCH_LIMITS.maxRetainedEvidenceViews * RESEARCH_LIMITS.maxResearchEvidenceExcerptBytes,
  );
  const evidenceStore = createResearchEvidenceStore(maxEvidenceBytes);
  let requestCounter = 0;
  let activeRequests = 0;

  function findSource(request: ResearchRequest): ResearchSourcePort | null {
    for (const source of sources) {
      if (source.kind === request.source.kind && source.id === request.source.id) {
        return source;
      }
    }
    for (const source of sources) {
      if (source.kind === request.source.kind && source.label === request.source.label) {
        return source;
      }
    }
    return null;
  }

  function toFetchResult(
    outcome: ResearchOutcome,
    requestId: string,
    task: ResearchTaskBinding,
  ): ResearchFetchResult {
    switch (outcome.status) {
      case "document": {
        const evidenceEntry = evidenceStore.record(outcome.document, requestId, task);
        return { status: "document", document: outcome.document, evidence: evidenceEntry };
      }
      case "refused":
      case "unsupported-content":
      case "oversized":
      case "unavailable":
      case "failed":
        return { status: outcome.status, reason: outcome.reason };
      case "timeout":
        return {
          status: "timeout",
          reason: `Research request timed out after ${bounds.timeoutMs}ms.`,
        };
      case "cancelled":
        return { status: "cancelled", reason: "Research request cancelled." };
    }
  }

  return {
    async fetch(
      request: ResearchRequest,
      opts: { readonly signal?: AbortSignal } = {},
    ): Promise<ResearchFetchResult> {
      // 1. Policy gate FIRST — the source port is never invoked when the
      //    gate does not allow (effect-test contract).
      const permission = evaluatePermission("research.fetch", options.policy, options.profile);
      if (permission.decision !== "allow") {
        return {
          status: "refused",
          reason:
            permission.decision === "ask"
              ? "research requires explicit network permission"
              : "network policy denies research",
        };
      }
      // 2. Validate the untrusted request.
      const validated = validateResearchRequest(request);
      if (!validated.ok) {
        return { status: "failed", reason: `invalid research request: ${validated.reason}` };
      }
      // 3. The source must be configured.
      const source = findSource(validated.request);
      if (source === null) {
        return {
          status: "refused",
          reason: `Unknown research source ${request.source.kind}:${request.source.id}; it is not configured.`,
        };
      }
      const currentTask = options.currentTask();
      if (!isValidResearchTaskBinding(currentTask)) {
        return {
          status: "refused",
          reason:
            "Research requires an active task with a valid TaskContract revision; no task-bound request was started.",
        };
      }
      // Snapshot primitive identity fields. Holding a caller-owned object
      // reference would let an in-place revision change mutate both the
      // expected and current values and defeat the stale-result check.
      const task: ResearchTaskBinding = Object.freeze({
        taskId: currentTask.taskId,
        taskContractRevision: currentTask.taskContractRevision,
      });
      // Already-aborted calls fail fast without invoking the source.
      if (opts.signal !== undefined && opts.signal.aborted) {
        return { status: "cancelled", reason: "Research request cancelled." };
      }
      requestCounter += 1;
      const sourceRequest: ResearchRequest = {
        ...validated.request,
        source: { kind: source.kind, id: source.id, label: source.label },
      };
      const requestId = createResearchRequestId(sourceRequest, task, requestCounter);
      activeRequests += 1;
      try {
        const composed = composeResearchSignal(opts.signal, bounds.timeoutMs);
        try {
          const outcomePromise = (async (): Promise<ResearchOutcome> => {
            try {
              return await source.fetch(sourceRequest, bounds, composed.signal);
            } catch (error) {
              return {
                status: "failed",
                reason: `The research source failed unexpectedly: ${describeResearchError(error)}`,
              };
            }
          })();
          const outcome = await raceWithResearchSignal(
            outcomePromise,
            composed.signal,
            () =>
              composed.timedOut()
                ? { status: "timeout" as const }
                : { status: "cancelled" as const },
            (error: unknown) => ({
              status: "failed" as const,
              reason: `The research source failed unexpectedly: ${describeResearchError(error)}`,
            }),
          );
          if (
            outcome.status === "document" &&
            !sameResearchTaskBinding(task, options.currentTask())
          ) {
            return {
              status: "stale",
              reason:
                "The active task or TaskContract revision changed while research was in flight; the result was discarded before evidence retention.",
            };
          }
          return toFetchResult(outcome, requestId, task);
        } finally {
          composed.cleanup();
        }
      } finally {
        activeRequests -= 1;
      }
    },

    latestEvidence(): readonly ResearchEvidence[] {
      return evidenceStore.snapshots();
    },

    activeRequestCount(): number {
      return activeRequests;
    },

    sourceKinds(): readonly ResearchSourceKind[] {
      const kinds: ResearchSourceKind[] = [];
      for (const source of sources) {
        if (!kinds.includes(source.kind)) {
          kinds.push(source.kind);
        }
      }
      return kinds;
    },
  };
}

/**
 * Pure, bounded model-facing rendering of one research evidence record
 * (EvidenceProjector-integration shape). The EvidenceProjector pipeline
 * still sanitizes/redacts the final string before it reaches the model.
 *
 * ```
 * Source: <label>
 * Request: <requestId>
 * Fetched: <iso timestamp>
 * Revision: <resolvedRevision | "unknown">
 * Version: <version | "unknown"> (+ " (fallback)" when fallback)
 * Excerpt: <excerpt>
 * Evidence: <evidenceId>
 * ```
 */
export function formatResearchEvidenceView(
  evidence: ResearchEvidence,
  opts: { readonly maxBytes?: number } = {},
): string {
  const lines = [
    `Source: ${evidence.source.label}`,
    `Request: ${evidence.requestId}`,
    `Fetched: ${new Date(evidence.fetchedAtMs).toISOString()}`,
    `Revision: ${evidence.resolvedRevision ?? "unknown"}`,
    `Version: ${evidence.version ?? "unknown"}${evidence.fallback ? " (fallback)" : ""}`,
    `Excerpt: ${evidence.excerpt}`,
    `Evidence: ${evidence.evidenceId}`,
  ];
  const text = lines.join("\n");
  const maxBytes = opts.maxBytes ?? DEFAULT_RESEARCH_VIEW_MAX_BYTES;
  return truncateText(text, maxBytes).text;
}
