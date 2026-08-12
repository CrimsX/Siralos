/**
 * Bounded, expiring, single-use registry for prepared Siralos plans
 * (recovery probes and GDScript checks).
 *
 * Prepared plans are opaque handles into an in-memory registry with a
 * count limit, an aggregate serialized-byte limit, and a TTL. Every handle
 * can be consumed exactly once (the plan is removed on consume), denied or
 * abandoned plans are explicitly disposed, and `disposeAll()` clears the
 * registry on session shutdown so prepared plans can never leak
 * indefinitely. The serialized byte estimate counts UTF-8 bytes of the
 * caller-provided canonical plan JSON, never JavaScript string length.
 */

export interface PreparedPlanStoreConfig {
  /** Maximum simultaneously prepared plans. */
  readonly maxPlans?: number;
  /** Maximum aggregate serialized plan bytes. */
  readonly maxStateBytes?: number;
  /** Maximum lifetime of a prepared plan before it expires. */
  readonly ttlMs?: number;
  /** Test seam; defaults to `Date.now`. */
  readonly now?: () => number;
  /** Human-readable plan label used in limit messages. */
  readonly label: string;
}

export interface PreparedPlanStore<THandle, TPlan extends object> {
  put(plan: TPlan):
    | {
        readonly ok: true;
        readonly handle: THandle;
        readonly stateBytes: number;
      }
    | {
        readonly ok: false;
        readonly reason: "count-limit" | "byte-limit";
        readonly message: string;
      };
  consume(
    handle: THandle,
  ): (TPlan & { readonly createdAt: number; readonly stateBytes: number }) | null;
  dispose(handle: THandle): boolean;
  expireNow(): number;
  size(): number;
  stateBytes(): number;
  disposeAll(): void;
}

export function createPreparedPlanStore<THandle, TPlan extends object>(
  config: PreparedPlanStoreConfig,
  createHandle: () => THandle,
  serialize: (plan: TPlan) => string,
): PreparedPlanStore<THandle, TPlan> {
  const maxPlans = config.maxPlans ?? 8;
  const maxStateBytes = config.maxStateBytes ?? 8 * 1024 * 1024;
  const ttlMs = config.ttlMs ?? 10 * 60 * 1000;
  const now = config.now ?? ((): number => Date.now());
  const plans = new Map<
    THandle,
    TPlan & { readonly createdAt: number; readonly stateBytes: number }
  >();

  function put(plan: TPlan) {
    expireNow();
    const stateBytes = new TextEncoder().encode(serialize(plan)).length;
    if (plans.size >= maxPlans) {
      return {
        ok: false,
        reason: "count-limit",
        message: `The ${config.label} limit of ${maxPlans} was reached; prepare again later.`,
      } as const;
    }
    const currentBytes = [...plans.values()].reduce((total, entry) => total + entry.stateBytes, 0);
    if (currentBytes + stateBytes > maxStateBytes) {
      return {
        ok: false,
        reason: "byte-limit",
        message: `The ${config.label} state byte limit was reached; the plan could not be prepared.`,
      } as const;
    }
    const handle = createHandle();
    plans.set(handle, { ...plan, createdAt: now(), stateBytes });
    return { ok: true, handle, stateBytes } as const;
  }

  function consume(handle: THandle) {
    const plan = plans.get(handle);
    if (plan === undefined) {
      return null;
    }
    plans.delete(handle);
    if (now() - plan.createdAt > ttlMs) {
      return null;
    }
    return plan;
  }

  function dispose(handle: THandle): boolean {
    return plans.delete(handle);
  }

  function expireNow(): number {
    let removed = 0;
    for (const [handle, plan] of plans) {
      if (now() - plan.createdAt > ttlMs) {
        plans.delete(handle);
        removed += 1;
      }
    }
    return removed;
  }

  function size(): number {
    return plans.size;
  }

  function stateBytes(): number {
    return [...plans.values()].reduce((total, entry) => total + entry.stateBytes, 0);
  }

  function disposeAll(): void {
    plans.clear();
  }

  return { put, consume, dispose, expireNow, size, stateBytes, disposeAll };
}
