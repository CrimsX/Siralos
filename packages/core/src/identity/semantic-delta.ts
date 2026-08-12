import { canonicalizeJson, sha256Hex } from "../godot/digest.js";

/**
 * Semantic delta helpers (Stage 3 — Content Identity & Delta
 * Verification, ADR 0028).
 *
 * Deltas are DERIVED communication/evidence — never authoritative
 * current state. The full authoritative artifact always remains the
 * source of truth; deltas only describe what materially changed between
 * two exact identities. Each domain decides what constitutes a
 * meaningful change; these helpers provide the shared mechanics.
 */

export interface SemanticDelta {
  /** Exact content identity of the base state. */
  readonly baseDigest: string;
  /** Exact content identity of the result state. */
  readonly resultDigest: string;
  /** True when no material change was detected. */
  readonly unchanged: boolean;
}

/**
 * Section-level delta over a record: each section is canonicalized
 * independently, so only sections whose content changed appear in
 * `changed`.
 */
export function computeSectionDelta(
  base: Record<string, unknown>,
  result: Record<string, unknown>,
  sectionKeys: readonly string[],
): {
  readonly changed: readonly string[];
  readonly unchanged: readonly string[];
} {
  const changed: string[] = [];
  const unchanged: string[] = [];
  for (const key of sectionKeys) {
    const baseValue = canonicalizeJson(base[key]);
    const resultValue = canonicalizeJson(result[key]);
    if (baseValue === resultValue) {
      unchanged.push(key);
    } else {
      changed.push(key);
    }
  }
  return { changed, unchanged };
}

/** True when two structured values are canonically identical. */
export function canonicalValuesEqual(a: unknown, b: unknown): boolean {
  return canonicalizeJson(a) === canonicalizeJson(b);
}

/**
 * Item-level delta over id-keyed lists: items are compared by their
 * canonical serialization, so a material change to an item appears in
 * `changed` while identity-preserving reorderings do not.
 */
export function computeItemListDelta(
  base: readonly { readonly id: string }[],
  result: readonly { readonly id: string }[],
): {
  readonly added: readonly string[];
  readonly removed: readonly string[];
  readonly changed: readonly string[];
  readonly unchanged: readonly string[];
} {
  const baseById = new Map(base.map((item) => [item.id, item]));
  const resultById = new Map(result.map((item) => [item.id, item]));
  const added: string[] = [];
  const removed: string[] = [];
  const changed: string[] = [];
  const unchanged: string[] = [];
  for (const id of resultById.keys()) {
    if (!baseById.has(id)) {
      added.push(id);
    }
  }
  for (const id of baseById.keys()) {
    if (!resultById.has(id)) {
      removed.push(id);
    }
  }
  for (const [id, baseItem] of baseById) {
    const resultItem = resultById.get(id);
    if (resultItem === undefined) {
      continue;
    }
    if (canonicalValuesEqual(baseItem, resultItem)) {
      unchanged.push(id);
    } else {
      changed.push(id);
    }
  }
  return { added, removed, changed, unchanged };
}

/** Deterministic code-unit comparison (locale-independent; stable across hosts). */
function compareCodeUnits(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

/** Digest of an id-keyed item list (canonical, order-insensitive). */
export function digestItemList(items: readonly { readonly id: string }[]): string {
  return sha256Hex(
    canonicalizeJson(
      [...items]
        .map((item) => ({ id: item.id, value: canonicalizeJson(item) }))
        .sort((a, b) => compareCodeUnits(a.id, b.id)),
    ),
  );
}
