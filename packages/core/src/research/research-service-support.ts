import { canonicalizeJson, sha256Hex } from "../godot/digest.js";
import { RESEARCH_LIMITS, type ResearchBounds, type ResearchRequest } from "./research-model.js";
import type { ResearchTaskBinding } from "./research-service-model.js";

const textEncoder = new TextEncoder();

export function describeResearchError(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

export function createResearchRequestId(
  request: ResearchRequest,
  task: ResearchTaskBinding,
  sequence: number,
): string {
  const requestDigest = sha256Hex(
    canonicalizeJson({
      source: request.source,
      query: request.query,
      topic: request.topic,
      path: request.path,
      ref: request.ref,
      version: request.version,
    }),
  );
  return `req_${sha256Hex(
    canonicalizeJson({
      requestDigest,
      taskId: task.taskId,
      taskContractRevision: task.taskContractRevision,
      sequence,
    }),
  ).slice(0, 24)}`;
}

export interface ComposedResearchSignal {
  readonly signal: AbortSignal;
  readonly timedOut: () => boolean;
  readonly cleanup: () => void;
}

/** Compose caller cancellation and the service timeout into one source signal. */
export function composeResearchSignal(
  signal: AbortSignal | undefined,
  timeoutMs: number,
): ComposedResearchSignal {
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
  const timer = setTimeout(() => {
    timeoutTriggered = true;
    controller.abort(new DOMException("Research request timed out", "TimeoutError"));
  }, timeoutMs);
  cleanups.push(() => clearTimeout(timer));
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

/** Race a source promise against an already-composed cancellation signal. */
export function raceWithResearchSignal<T>(
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

export function normalizeBoundedInteger(
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

export function normalizeResearchBounds(input: ResearchBounds): ResearchBounds {
  const timeoutMs = normalizeBoundedInteger(
    input.timeoutMs,
    RESEARCH_LIMITS.timeoutMs,
    1,
    RESEARCH_LIMITS.timeoutMs,
  );
  return Object.freeze({
    maxDownloadBytes: normalizeBoundedInteger(
      input.maxDownloadBytes,
      RESEARCH_LIMITS.maxDownloadBytes,
      1,
      RESEARCH_LIMITS.maxDownloadBytes,
    ),
    maxDocumentBytes: normalizeBoundedInteger(
      input.maxDocumentBytes,
      RESEARCH_LIMITS.maxDocumentBytes,
      1,
      RESEARCH_LIMITS.maxDocumentBytes,
    ),
    maxSections: normalizeBoundedInteger(
      input.maxSections,
      RESEARCH_LIMITS.maxSections,
      1,
      RESEARCH_LIMITS.maxSections,
    ),
    maxLinks: normalizeBoundedInteger(
      input.maxLinks,
      RESEARCH_LIMITS.maxLinks,
      0,
      RESEARCH_LIMITS.maxLinks,
    ),
    maxHeadingBytes: normalizeBoundedInteger(
      input.maxHeadingBytes,
      RESEARCH_LIMITS.maxHeadingBytes,
      1,
      RESEARCH_LIMITS.maxHeadingBytes,
    ),
    maxSectionTextBytes: normalizeBoundedInteger(
      input.maxSectionTextBytes,
      RESEARCH_LIMITS.maxSectionTextBytes,
      1,
      RESEARCH_LIMITS.maxSectionTextBytes,
    ),
    maxRedirects: normalizeBoundedInteger(
      input.maxRedirects,
      RESEARCH_LIMITS.maxRedirects,
      0,
      RESEARCH_LIMITS.maxRedirects,
    ),
    timeoutMs,
    hardLifetimeMs: Math.max(
      timeoutMs,
      normalizeBoundedInteger(
        input.hardLifetimeMs,
        RESEARCH_LIMITS.hardLifetimeMs,
        1,
        RESEARCH_LIMITS.hardLifetimeMs,
      ),
    ),
  });
}

export function isValidResearchTaskBinding(
  value: ResearchTaskBinding | null,
): value is ResearchTaskBinding {
  return (
    value !== null &&
    value.taskId.trim().length > 0 &&
    textEncoder.encode(value.taskId).length <= 128 &&
    Number.isSafeInteger(value.taskContractRevision) &&
    value.taskContractRevision >= 1
  );
}

export function sameResearchTaskBinding(
  expected: ResearchTaskBinding,
  current: ResearchTaskBinding | null,
): boolean {
  return (
    current !== null &&
    current.taskId === expected.taskId &&
    current.taskContractRevision === expected.taskContractRevision
  );
}
