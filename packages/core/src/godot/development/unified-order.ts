/**
 * Deterministic unified apply ordering (Stage 3 milestone 11, ADR 0027).
 *
 * The apply order across targets of a mixed change set is explicit and
 * evidenced: explicit cross-target dependency edges are resolved from
 * the prepared targets (a target referencing another target's path must
 * apply after it), then a deterministic topological sort with a
 * deterministic tie-break. "Scripts first" or "scenes first" are never
 * hardcoded: when no dependency edge exists the order is deterministic
 * path order and the rationale records that no ordering semantics are
 * required.
 */

export interface UnifiedOrderTarget {
  readonly targetId: string;
  readonly path: string;
  /**
   * Workspace-relative paths this target references, resolved by the
   * host from the current documents (scene ext_resources, resource
   * references, script attachments). Absent when no resolution exists.
   */
  readonly references: readonly string[];
}

export interface UnifiedApplyOrder {
  /** Target ids in apply order. */
  readonly order: readonly string[];
  /** Deterministic rationale for the derived order. */
  readonly rationale: string;
}

/** Dependency edge: `before` must apply before `after`. */
export interface UnifiedOrderEdge {
  readonly before: string;
  readonly after: string;
}

/**
 * Resolve the explicit cross-target edges. A reference to another
 * target's path creates the edge referenced -> referencing (the
 * referenced path must already be in its final state).
 */
export function deriveUnifiedOrderEdges(targets: readonly UnifiedOrderTarget[]): {
  readonly edges: readonly UnifiedOrderEdge[];
  readonly unresolvedReferences: readonly { readonly targetId: string; readonly path: string }[];
} {
  const pathToTarget = new Map<string, string>();
  for (const target of targets) {
    pathToTarget.set(target.path, target.targetId);
  }
  const edges: UnifiedOrderEdge[] = [];
  const unresolvedReferences: { readonly targetId: string; readonly path: string }[] = [];
  for (const target of targets) {
    for (const reference of target.references) {
      const referencedTarget = pathToTarget.get(reference);
      if (referencedTarget === undefined || referencedTarget === target.targetId) {
        if (referencedTarget === undefined) {
          unresolvedReferences.push({ targetId: target.targetId, path: reference });
        }
        continue;
      }
      // The referenced target's final state must exist before this target applies.
      edges.push({ before: referencedTarget, after: target.targetId });
    }
  }
  return { edges, unresolvedReferences };
}

/**
 * Deterministic topological sort with path tie-break. Cycles (which
 * cannot occur from path references of one target set) are reported
 * rather than silently reordered.
 */
export function deriveUnifiedApplyOrder(
  targets: readonly UnifiedOrderTarget[],
  edges: readonly UnifiedOrderEdge[],
): UnifiedApplyOrder {
  if (targets.length === 0) {
    return { order: [], rationale: "No targets to order." };
  }
  const byId = new Map(targets.map((target) => [target.targetId, target]));
  const incoming = new Map<string, Set<string>>();
  const dependents = new Map<string, Set<string>>();
  for (const target of targets) {
    incoming.set(target.targetId, new Set());
    dependents.set(target.targetId, new Set());
  }
  for (const edge of edges) {
    if (!byId.has(edge.before) || !byId.has(edge.after)) {
      continue;
    }
    incoming.get(edge.after)?.add(edge.before);
    dependents.get(edge.before)?.add(edge.after);
  }
  const ordered: string[] = [];
  const pending = new Set(targets.map((target) => target.targetId));
  while (pending.size > 0) {
    const ready = [...pending]
      .filter((id) => (incoming.get(id)?.size ?? 0) === 0)
      .sort((a, b) => {
        const pathA = byId.get(a)?.path ?? "";
        const pathB = byId.get(b)?.path ?? "";
        return pathA.localeCompare(pathB);
      });
    if (ready.length === 0) {
      const cycle = [...pending].sort().join(", ");
      throw new Error(`The unified apply order contains a dependency cycle: ${cycle}.`);
    }
    for (const id of ready) {
      ordered.push(id);
      pending.delete(id);
      for (const dependent of dependents.get(id) ?? []) {
        incoming.get(dependent)?.delete(id);
      }
    }
  }
  const edgeCount = edges.length;
  const rationale =
    edgeCount === 0
      ? `No cross-target dependency was resolved; targets apply in deterministic path order (${ordered
          .map((id) => byId.get(id)?.path ?? id)
          .join(", ")}).`
      : `Apply order derived from ${edgeCount} resolved cross-target dependency edge(s); ties broken deterministically by path.`;
  return { order: ordered, rationale };
}
