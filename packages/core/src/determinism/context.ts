/**
 * Explicit nondeterminism boundaries (Stage 3 — Deterministic Execution
 * & Reproducibility, ADR 0029).
 *
 * Authoritative host decisions must not silently depend on ambient
 * wall-clock time, randomness, process environment, filesystem
 * enumeration order, locale/timezone, or concurrency completion order.
 * Adapters own external nondeterminism; core consumes it through these
 * explicit ports. "Controlled time" never means frozen production time —
 * production uses the explicit system clock; tests use a fixed clock.
 */

// ---------------------------------------------------------------------------
// Clock
// ---------------------------------------------------------------------------

export type Clock = () => number;

/** Explicit system clock (adapter boundary: real time enters only here). */
export function createSystemClock(): Clock {
  return Date.now;
}

/** Fixed clock for tests and deterministic policy evaluation. */
export function createFixedClock(initialMs: number): {
  readonly now: Clock;
  readonly advance: (deltaMs: number) => void;
  readonly set: (ms: number) => void;
} {
  let current = initialMs;
  return {
    now: () => current,
    advance: (deltaMs) => {
      current += deltaMs;
    },
    set: (ms) => {
      current = ms;
    },
  };
}

// ---------------------------------------------------------------------------
// RandomSource
// ---------------------------------------------------------------------------

export interface RandomSource {
  /** Next uniform value in [0, 1). */
  next(): number;
  /** Next integer in [0, bound). */
  nextInt(bound: number): number;
  /** 128-bit hex token (identity generation; NOT a decision input). */
  nextToken(): string;
}

/**
 * Deterministic seeded PRNG (mulberry32). Used only where randomness is
 * genuinely part of the design; most host policy decisions need none.
 */
export function createSeededRandomSource(seed: number): RandomSource {
  let state = seed >>> 0;
  if (state === 0) {
    state = 0x9e3779b9;
  }
  const next = (): number => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  const nextInt = (bound: number): number => {
    if (!Number.isSafeInteger(bound) || bound <= 0) {
      throw new Error("A random bound must be a positive safe integer.");
    }
    return Math.floor(next() * bound);
  };
  const nextToken = (): string => {
    const hex = (value: number): string => (value >>> 0).toString(16).padStart(8, "0");
    return `${hex(Math.floor(next() * 0xffffffff))}${hex(Math.floor(next() * 0xffffffff))}${hex(
      Math.floor(next() * 0xffffffff),
    )}${hex(Math.floor(next() * 0xffffffff))}`;
  };
  return { next, nextInt, nextToken };
}

/** Explicit ambient random source (adapter boundary only). */
export function createSystemRandomSource(): RandomSource {
  const bytes = new Uint32Array(2);
  const next = (): number => {
    globalThis.crypto?.getRandomValues(bytes);
    return bytes[0]! / 4294967296;
  };
  return {
    next,
    nextInt: (bound) => Math.floor(next() * bound),
    nextToken: () => {
      const value = new Uint32Array(4);
      globalThis.crypto?.getRandomValues(value);
      return Array.from(value, (word) => (word >>> 0).toString(16).padStart(8, "0")).join("");
    },
  };
}

// ---------------------------------------------------------------------------
// OrderingPolicy
// ---------------------------------------------------------------------------

/**
 * Canonical ordering policy: sets whose order affects hashes, provider
 * context, decisions, validation, findings, or reports are sorted with a
 * domain-appropriate stable key using locale-independent code-unit
 * comparison. Ordering policy is mechanism, never domain/business logic.
 */
export interface OrderingPolicy {
  /** Stable ascending code-unit comparator (locale-independent). */
  compare(a: string, b: string): number;
  /** Stable sort (does not mutate the input). */
  stableSort<T>(items: readonly T[], key: (item: T) => string): T[];
}

export function createOrderingPolicy(): OrderingPolicy {
  const compare = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0);
  return {
    compare,
    stableSort: (items, key) =>
      [...items]
        .map((item, index) => ({ item, index, key: key(item) }))
        .sort((a, b) => compare(a.key, b.key) || a.index - b.index)
        .map((entry) => entry.item),
  };
}

/** Canonical ordering of an unordered keyed result set (order-insensitive). */
export function normalizeKeyedResults<T extends { readonly id: string }>(
  results: readonly T[],
): T[] {
  const ordering = createOrderingPolicy();
  return ordering.stableSort(results, (entry) => entry.id);
}
