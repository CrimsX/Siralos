/**
 * Revision-bound asynchronous results (Stage 3 milestone 2).
 *
 * Every async projection/helper result that can arrive after state changes
 * is bound to enough identity (task id, contract revision, request id) to
 * determine whether it is still current. When it completes, the owner asks
 * `isCurrent`; stale results are discarded and never injected into a newer
 * turn. Deterministic fake scheduling is used in tests — no sleep-based
 * timing.
 */

export interface RevisionBound<T> {
  readonly key: string;
  readonly revision: number;
  readonly value: T;
}

export interface RevisionGuard {
  /** Current revision identity; increments on each state advance. */
  readonly revision: number;
  /** Bind a value to the CURRENT revision. */
  bind<T>(value: T): RevisionBound<T>;
  /** True when the bound result belongs to the current revision. */
  isCurrent<T>(bound: RevisionBound<T>): boolean;
  /** Advance the revision; previously bound results become stale. */
  advance(): void;
}

export function createRevisionGuard(initialRevision = 1): RevisionGuard {
  let revision = initialRevision;
  return {
    get revision(): number {
      return revision;
    },
    bind<T>(value: T): RevisionBound<T> {
      return { key: "projection", revision, value };
    },
    isCurrent<T>(bound: RevisionBound<T>): boolean {
      return bound.revision === revision;
    },
    advance(): void {
      revision += 1;
    },
  };
}

/** Await an async value and resolve to null when it went stale meanwhile. */
export async function awaitCurrent<T>(
  guard: RevisionGuard,
  promise: Promise<T>,
): Promise<T | null> {
  const revision = guard.revision;
  const value = await promise;
  return revision === guard.revision ? value : null;
}
