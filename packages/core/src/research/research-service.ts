import { canonicalizeJson, sha256Hex } from "../godot/digest.js";
import { truncateText } from "../projection/evidence-projector.js";
import type { CapabilityPolicy } from "../security/capability.js";
import { evaluatePermission } from "../security/permission-evaluator.js";
import type { SandboxProfile } from "../security/profile.js";
import {
  RESEARCH_LIMITS,
  defaultResearchBounds,
  validateResearchRequest,
  type ResearchBounds,
  type ResearchDocument,
  type ResearchOutcome,
  type ResearchRequest,
  type ResearchSourceKind,
  type ResearchSourceRef,
} from "./research-model.js";
import type { ResearchSourcePort } from "./research-ports.js";

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

export interface ResearchEvidence {
  readonly evidenceId: string;
  readonly requestId: string;
  readonly taskId: string;
  readonly taskContractRevision: number;
  readonly source: ResearchSourceRef;
  readonly fetchedAtMs: number;
  readonly resolvedRevision: string | null;
  readonly version: string | null;
  readonly fallback: boolean;
  /** First-section excerpt, bounded by `maxResearchEvidenceExcerptBytes`. */
  readonly excerpt: string;
  readonly truncated: boolean;
  readonly byteLength: number;
}

export type ResearchFetchResult =
  | { readonly status: "refused"; readonly reason: string }
  | {
      readonly status: "document";
      readonly document: ResearchDocument;
      readonly evidence: ResearchEvidence;
    }
  | {
      readonly status:
        | "unsupported-content"
        | "oversized"
        | "timeout"
        | "cancelled"
        | "stale"
        | "unavailable"
        | "failed";
      readonly reason: string;
    };

/** Exact task identity captured around one asynchronous research request. */
export interface ResearchTaskBinding {
  readonly taskId: string;
  readonly taskContractRevision: number;
}

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

function describeError(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

function requestDigest(request: ResearchRequest): string {
  return sha256Hex(
    canonicalizeJson({
      source: request.source,
      query: request.query,
      topic: request.topic,
      path: request.path,
      ref: request.ref,
      version: request.version,
    }),
  );
}

/**
 * Compose the caller's abort signal with a service timeout into one signal
 * handed to the source port, so the source can cancel its own work on
 * either condition.
 */
function composeSignal(
  signal: AbortSignal | undefined,
  timeoutMs: number,
): {
  readonly signal: AbortSignal;
  readonly timedOut: () => boolean;
  readonly cleanup: () => void;
} {
  const controller = new AbortController();
  const cleanups: Array<() => void> = [];
  let timeoutTriggered = false;
  if (signal !== undefined) {
    if (signal.aborted) {
      controller.abort(signal.reason);
    } else {
      const onAbort = (): void => controller.abort(signal.reason);
      signal.addEventListener("abort", onAbort, { once: true });
      cleanups.push(() => signal.removeEventListener("abort", onAbort));
    }
  }
  let timer: ReturnType<typeof setTimeout> | undefined;
  if (timeoutMs > 0) {
    timer = setTimeout(() => {
      timeoutTriggered = true;
      controller.abort(new DOMException("Research request timed out", "TimeoutError"));
    }, timeoutMs);
    cleanups.push(() => {
      if (timer !== undefined) {
        clearTimeout(timer);
      }
    });
  }
  return {
    signal: controller.signal,
    timedOut: () => timeoutTriggered,
    cleanup: () => {
      for (const cleanup of cleanups) {
        cleanup();
      }
    },
  };
}

/**
 * Race one promise against a composed abort signal. The signal already
 * carries both caller cancellation and the service timeout, so there is
 * only one timer and the source is always aborted when the service stops
 * waiting for it.
 */
function raceWithSignal<T>(
  promise: Promise<T>,
  signal: AbortSignal,
  onAbort: () => T,
  onFailure: (error: unknown) => T,
): Promise<T> {
  if (signal.aborted) {
    return Promise.resolve(onAbort());
  }
  return new Promise<T>((resolve) => {
    let settled = false;
    const finish = (value: T): void => {
      if (settled) {
        return;
      }
      settled = true;
      signal.removeEventListener("abort", onAbortEvent);
      resolve(value);
    };
    const onAbortEvent = (): void => finish(onAbort());
    signal.addEventListener("abort", onAbortEvent, { once: true });
    promise.then(finish, (error: unknown) => finish(onFailure(error)));
  });
}

function normalizeInteger(
  value: number | undefined,
  fallback: number,
  minimum: number,
  maximum: number,
): number {
  if (value === undefined || !Number.isFinite(value)) {
    return fallback;
  }
  return Math.min(maximum, Math.max(minimum, Math.floor(value)));
}

function normalizeResearchBounds(input: ResearchBounds): ResearchBounds {
  const timeoutMs = normalizeInteger(
    input.timeoutMs,
    RESEARCH_LIMITS.timeoutMs,
    1,
    RESEARCH_LIMITS.timeoutMs,
  );
  return Object.freeze({
    maxDownloadBytes: normalizeInteger(
      input.maxDownloadBytes,
      RESEARCH_LIMITS.maxDownloadBytes,
      1,
      RESEARCH_LIMITS.maxDownloadBytes,
    ),
    maxDocumentBytes: normalizeInteger(
      input.maxDocumentBytes,
      RESEARCH_LIMITS.maxDocumentBytes,
      1,
      RESEARCH_LIMITS.maxDocumentBytes,
    ),
    maxSections: normalizeInteger(
      input.maxSections,
      RESEARCH_LIMITS.maxSections,
      1,
      RESEARCH_LIMITS.maxSections,
    ),
    maxLinks: normalizeInteger(
      input.maxLinks,
      RESEARCH_LIMITS.maxLinks,
      0,
      RESEARCH_LIMITS.maxLinks,
    ),
    maxHeadingBytes: normalizeInteger(
      input.maxHeadingBytes,
      RESEARCH_LIMITS.maxHeadingBytes,
      1,
      RESEARCH_LIMITS.maxHeadingBytes,
    ),
    maxSectionTextBytes: normalizeInteger(
      input.maxSectionTextBytes,
      RESEARCH_LIMITS.maxSectionTextBytes,
      1,
      RESEARCH_LIMITS.maxSectionTextBytes,
    ),
    maxRedirects: normalizeInteger(
      input.maxRedirects,
      RESEARCH_LIMITS.maxRedirects,
      0,
      RESEARCH_LIMITS.maxRedirects,
    ),
    timeoutMs,
    hardLifetimeMs: Math.max(
      timeoutMs,
      normalizeInteger(
        input.hardLifetimeMs,
        RESEARCH_LIMITS.hardLifetimeMs,
        1,
        RESEARCH_LIMITS.hardLifetimeMs,
      ),
    ),
  });
}

function isValidTaskBinding(value: ResearchTaskBinding | null): value is ResearchTaskBinding {
  return (
    value !== null &&
    value.taskId.trim().length > 0 &&
    new TextEncoder().encode(value.taskId).length <= 128 &&
    Number.isSafeInteger(value.taskContractRevision) &&
    value.taskContractRevision >= 1
  );
}

function sameTaskBinding(
  expected: ResearchTaskBinding,
  current: ResearchTaskBinding | null,
): boolean {
  return (
    current !== null &&
    current.taskId === expected.taskId &&
    current.taskContractRevision === expected.taskContractRevision
  );
}

export function createResearchService(options: ResearchServiceOptions): ResearchService {
  const bounds = normalizeResearchBounds(options.bounds ?? defaultResearchBounds());
  const sources = [...options.sources];
  const evidence: ResearchEvidence[] = [];
  const maxEvidenceBytes = normalizeInteger(
    options.maxEvidenceBytes,
    RESEARCH_LIMITS.maxRetainedEvidenceViews * RESEARCH_LIMITS.maxResearchEvidenceExcerptBytes,
    0,
    RESEARCH_LIMITS.maxRetainedEvidenceViews * RESEARCH_LIMITS.maxResearchEvidenceExcerptBytes,
  );
  let evidenceCounter = 0;
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

  function buildEvidence(
    document: ResearchDocument,
    requestId: string,
    task: ResearchTaskBinding,
  ): ResearchEvidence {
    const firstSection = document.sections[0];
    const raw = firstSection?.text ?? "";
    const excerpt = truncateText(raw, RESEARCH_LIMITS.maxResearchEvidenceExcerptBytes);
    evidenceCounter += 1;
    return {
      evidenceId: `ev-research-${evidenceCounter}`,
      requestId,
      taskId: task.taskId,
      taskContractRevision: task.taskContractRevision,
      source: document.source,
      fetchedAtMs: document.fetchedAtMs,
      resolvedRevision: document.provenance.resolvedRevision,
      version: document.provenance.usedVersion,
      fallback: document.provenance.fallback,
      excerpt: excerpt.text,
      truncated: excerpt.truncated,
      byteLength: new TextEncoder().encode(excerpt.text).length,
    };
  }

  function retainEvidence(entry: ResearchEvidence): void {
    evidence.push(entry);
    let totalBytes = evidence.reduce((sum, item) => sum + item.byteLength, 0);
    while (
      evidence.length > RESEARCH_LIMITS.maxRetainedEvidenceViews ||
      totalBytes > maxEvidenceBytes
    ) {
      const dropped = evidence.shift();
      if (dropped === undefined) {
        break;
      }
      totalBytes -= dropped.byteLength;
    }
  }

  function toFetchResult(
    outcome: ResearchOutcome,
    requestId: string,
    task: ResearchTaskBinding,
  ): ResearchFetchResult {
    switch (outcome.status) {
      case "document": {
        const evidenceEntry = buildEvidence(outcome.document, requestId, task);
        retainEvidence(evidenceEntry);
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
      if (!isValidTaskBinding(currentTask)) {
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
      const requestId = `req_${sha256Hex(
        canonicalizeJson({
          requestDigest: requestDigest(sourceRequest),
          taskId: task.taskId,
          taskContractRevision: task.taskContractRevision,
          sequence: requestCounter,
        }),
      ).slice(0, 24)}`;
      activeRequests += 1;
      try {
        const composed = composeSignal(opts.signal, bounds.timeoutMs);
        try {
          const outcomePromise = (async (): Promise<ResearchOutcome> => {
            try {
              return await source.fetch(sourceRequest, bounds, composed.signal);
            } catch (error) {
              return {
                status: "failed",
                reason: `The research source failed unexpectedly: ${describeError(error)}`,
              };
            }
          })();
          const outcome = await raceWithSignal(
            outcomePromise,
            composed.signal,
            () =>
              composed.timedOut()
                ? { status: "timeout" as const }
                : { status: "cancelled" as const },
            (error: unknown) => ({
              status: "failed" as const,
              reason: `The research source failed unexpectedly: ${describeError(error)}`,
            }),
          );
          if (outcome.status === "document" && !sameTaskBinding(task, options.currentTask())) {
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
      return evidence.map((entry) => ({ ...entry, source: { ...entry.source } }));
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
