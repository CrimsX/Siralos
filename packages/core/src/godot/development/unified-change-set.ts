import { canonicalizeJson, sha256Hex } from "../digest.js";
import { deepFreeze } from "../../domain/deep-freeze.js";
import type { PreparedGodotMutation } from "../scene-mutation/prepared.js";
import type { ChangeSetOperation } from "./development-change-set.js";
import type { DevelopmentSurfaceKind } from "./development-surface.js";

/**
 * Unified multi-surface change set (Stage 3 milestone 11, ADR 0027).
 *
 * A development iteration is a bounded change set containing one or more
 * targets: exact-text change sets (GDScript and other UTF-8 text source)
 * and/or prepared native scene/resource mutations. Every target retains
 * its own source revision identity, its own prepared fingerprint (the
 * exact approval identity), its own approval state, its own checkpoint
 * relationship, and its own post-apply verification slot — per-file
 * authorization semantics never dissolve because targets share one task.
 *
 * The raw text boundary is unchanged: `.tscn`/`.tres` paths remain
 * refused at the provider-facing text change-set boundary (S3M10
 * invariant); native targets enter only as prepared structural
 * mutations.
 */

export type UnifiedTarget =
  | {
      readonly kind: "text";
      /** Validated exact-text operations (never scene/resource paths). */
      readonly fileOps: readonly ChangeSetOperation[];
    }
  | {
      readonly kind: "native";
      readonly prepared: PreparedGodotMutation;
    };

export type UnifiedTargetApprovalState = "pending" | "approved" | "denied";

/** One target of a unified change set with its own authorization state. */
export interface UnifiedChangeSetTargetEntry {
  /** Stable target identity inside the change set (t-1, t-2, ...). */
  readonly targetId: string;
  readonly kind: "text" | "native";
  /** Primary workspace-relative path of the target. */
  readonly path: string;
  /**
   * Exact prepared identity: for native targets the prepared mutation
   * fingerprint; for text targets a deterministic digest over the exact
   * file operations. Approval binds this value.
   */
  readonly fingerprint: string;
  /** Exact expected pre-state SHA-256 per file this target touches. */
  readonly preStates: readonly { readonly path: string; readonly sha256: string }[];
  readonly target: UnifiedTarget;
  /** Host-owned approval state; changing the target invalidates it. */
  readonly approval: {
    readonly state: UnifiedTargetApprovalState;
    readonly approvedDigest: string | null;
  };
  /** Post-apply verification evidence slot (host-filled after apply). */
  readonly verification: {
    readonly status: "pending" | "verified" | "failed";
    readonly detail: string | null;
  } | null;
}

export interface UnifiedChangeSet {
  readonly id: string;
  /** Targets in the derived deterministic apply order. */
  readonly targets: readonly UnifiedChangeSetTargetEntry[];
  /** Host-derived surface classification for this change set. */
  readonly surface: DevelopmentSurfaceKind;
  /** Deterministic rationale for the apply order (explicit, evidenced). */
  readonly orderRationale: string;
  /** Combined digest over the exact prepared targets in order. */
  readonly combinedDigest: string;
  /** Immutable creation time (host clock). */
  readonly createdAtMs: number;
  /** Prepared-life TTL; after expiry every approval is invalid. */
  readonly ttlMs: number;
}

export interface CreateUnifiedChangeSetInput {
  readonly id: string;
  readonly targets: readonly {
    readonly kind: "text" | "native";
    readonly path: string;
    readonly fingerprint: string;
    readonly preStates: readonly { readonly path: string; readonly sha256: string }[];
    readonly target: UnifiedTarget;
  }[];
  readonly surface: DevelopmentSurfaceKind;
  readonly orderRationale: string;
  readonly createdAtMs: number;
  readonly ttlMs: number;
}

/** Per-target digest for a text change set (exact operations only). */
export function computeTextTargetDigest(fileOps: readonly ChangeSetOperation[]): string {
  return sha256Hex(
    canonicalizeJson(
      fileOps.map((operation) => {
        if (operation.operation === "create") {
          return { operation: "create", path: operation.path, content: operation.content };
        }
        if (operation.operation === "delete") {
          return { operation: "delete", path: operation.path };
        }
        return {
          operation: "edit",
          path: operation.path,
          replacements: operation.replacements,
        };
      }),
    ),
  );
}

export function computeUnifiedChangeSetDigest(parts: {
  readonly targets: readonly { readonly kind: "text" | "native"; readonly fingerprint: string }[];
}): string {
  return sha256Hex(
    canonicalizeJson({
      targets: parts.targets.map((target) => ({
        kind: target.kind,
        fingerprint: target.fingerprint,
      })),
    }),
  );
}

const UNIFIED_LIMITS = {
  /** Maximum targets in one unified change set. */
  maxTargets: 16,
  /** Maximum pre-state files across all targets. */
  maxPreStateFiles: 32,
  /** Maximum combined preview bytes before approval. */
  maxPreviewBytes: 512 * 1024,
} as const;

export function createUnifiedChangeSet(input: CreateUnifiedChangeSetInput): UnifiedChangeSet {
  if (input.targets.length === 0) {
    throw new Error("A unified change set requires at least one target.");
  }
  if (input.targets.length > UNIFIED_LIMITS.maxTargets) {
    throw new Error(`A unified change set is limited to ${UNIFIED_LIMITS.maxTargets} targets.`);
  }
  const seenPaths = new Set<string>();
  const seenFiles = new Set<string>();
  const entries: UnifiedChangeSetTargetEntry[] = [];
  let targetIndex = 0;
  for (const target of input.targets) {
    targetIndex += 1;
    if (target.kind !== "text" && target.kind !== "native") {
      throw new Error("Every unified target must be kind text or native.");
    }
    if (target.path.length === 0 || target.path.includes(":")) {
      throw new Error(`Target path "${target.path}" is invalid.`);
    }
    if (seenPaths.has(target.path)) {
      throw new Error(`The unified change set addresses the path "${target.path}" more than once.`);
    }
    seenPaths.add(target.path);
    if (target.fingerprint.length !== 64) {
      throw new Error(`Target "${target.path}" requires a 64-hex prepared fingerprint.`);
    }
    if (target.preStates.length === 0) {
      throw new Error(`Target "${target.path}" requires at least one exact pre-state.`);
    }
    if (target.preStates.length > UNIFIED_LIMITS.maxPreStateFiles) {
      throw new Error(`Target "${target.path}" exceeds the unified pre-state file limit.`);
    }
    for (const preState of target.preStates) {
      if (!/^[0-9a-f]{64}$/.test(preState.sha256)) {
        throw new Error(`Target "${target.path}" requires 64-hex pre-state SHA-256 values.`);
      }
      if (seenFiles.has(preState.path)) {
        throw new Error(
          `The unified change set addresses the file "${preState.path}" in more than one target.`,
        );
      }
      seenFiles.add(preState.path);
    }
    entries.push({
      targetId: `t-${targetIndex}`,
      kind: target.kind,
      path: target.path,
      fingerprint: target.fingerprint,
      preStates: target.preStates.map((preState) => ({ ...preState })),
      target: target.target,
      approval: { state: "pending", approvedDigest: null },
      verification: null,
    });
  }
  const combinedDigest = computeUnifiedChangeSetDigest({
    targets: entries.map((entry) => ({ kind: entry.kind, fingerprint: entry.fingerprint })),
  });
  return deepFreeze({
    id: input.id,
    targets: entries,
    surface: input.surface,
    orderRationale: input.orderRationale,
    combinedDigest,
    createdAtMs: input.createdAtMs,
    ttlMs: input.ttlMs,
  });
}

/**
 * Host-owned approval record: binds one target's exact fingerprint.
 * A material change to the target (revision, operations, prepared
 * output) produces a new fingerprint, so a stale approval never matches.
 */
export function approveUnifiedTarget(
  changeSet: UnifiedChangeSet,
  targetId: string,
  approvedDigest: string,
): UnifiedChangeSet {
  if (approvedDigest.length !== 64) {
    throw new Error("An approval requires the 64-hex digest of the exact prepared target.");
  }
  const targets = changeSet.targets.map((entry) => {
    if (entry.targetId !== targetId) {
      return entry;
    }
    if (entry.approval.state === "approved") {
      throw new Error(
        `Target ${targetId} is already approved; a changed target needs re-approval.`,
      );
    }
    if (approvedDigest !== entry.fingerprint) {
      throw new Error(
        `The approval digest does not match target ${targetId}; a new prepared target requires a new approval.`,
      );
    }
    return {
      ...entry,
      approval: { state: "approved" as const, approvedDigest },
    };
  });
  return deepFreeze({ ...changeSet, targets });
}

/** True only when every target is approved and the prepared set is fresh. */
export function unifiedChangeSetReadyToApply(
  changeSet: UnifiedChangeSet,
  nowMs: number,
): {
  readonly ready: boolean;
  readonly reason: string | null;
} {
  if (nowMs - changeSet.createdAtMs > changeSet.ttlMs) {
    return {
      ready: false,
      reason: "The prepared change set expired; a fresh preparation is required.",
    };
  }
  for (const entry of changeSet.targets) {
    if (entry.approval.state !== "approved") {
      return {
        ready: false,
        reason: `Target ${entry.targetId} (${entry.path}) is not approved.`,
      };
    }
  }
  return { ready: true, reason: null };
}

/** Combined expected pre-state map over every target (apply precondition). */
export function unifiedPreStateMap(changeSet: UnifiedChangeSet): ReadonlyMap<string, string> {
  const map = new Map<string, string>();
  for (const entry of changeSet.targets) {
    for (const preState of entry.preStates) {
      map.set(preState.path, preState.sha256);
    }
  }
  return map;
}
