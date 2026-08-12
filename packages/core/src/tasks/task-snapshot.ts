import { canonicalizeJson, sha256Hex } from "../godot/digest.js";
import { deepFreeze } from "../domain/deep-freeze.js";
import type { CapabilityPolicy } from "../security/capability.js";
import type { ReferenceAlias, ReferenceRevision } from "../reference/reference-model.js";

/**
 * Immutable task-runtime configuration snapshot (Stage 3 milestone 1).
 *
 * Captured once when a task starts. Ordinary global configuration changes
 * affect future tasks, never an already-running task's snapshot. The
 * snapshot prefers revisions/fingerprints/references over eager copies of
 * large configuration blobs; where a component has no stable revision
 * identity yet, its existing fingerprint is used and the gap documented.
 */

export const TASK_RUNTIME_VERSION = "task-runtime-1";

export interface TaskRuntimeSnapshotProviderIdentity {
  readonly profileId: string;
  /** Model route (for example an aliased model name); null when unknown. */
  readonly route: string | null;
}

export interface TaskRuntimeSnapshotWorkflowIdentity {
  readonly id: string;
  readonly version: string;
  /** Immutable prepared-operation digest the task is bound to, when any. */
  readonly digest: string | null;
}

export interface TaskRuntimeSnapshot {
  readonly capturedAtMs: number;
  /** Task runtime schema/behavior version, for future persistence. */
  readonly runtimeVersion: string;
  readonly provider: TaskRuntimeSnapshotProviderIdentity | null;
  readonly sandboxProfileId: string | null;
  readonly capabilityPolicyRevision: string | null;
  /** Canonicalized workspace identity (root path), when known. */
  readonly workspaceIdentity: string | null;
  /** Selected Godot engine executable fingerprint, when known. */
  readonly godotEngineFingerprint: string | null;
  readonly workflow: TaskRuntimeSnapshotWorkflowIdentity | null;
  /**
   * Revision of the resolved project-instruction inventory that influenced
   * this task, when the host tracks it (ADR 0017). Historical task
   * provenance is never silently mutated.
   */
  readonly instructionSetRevision: string | null;
  /** Revision of the project-knowledge state at task start, when tracked. */
  readonly knowledgeStateRevision: string | null;
  /**
   * Reference revisions bound at task start (the registry's `bindTask`
   * snapshot), newest task-visible; bounded to 16. Historical — a later
   * registry refresh never mutates a captured task snapshot.
   */
  readonly referenceRevisions: readonly { alias: ReferenceAlias; revision: ReferenceRevision }[];
  /**
   * Execution-contract identity bound at task start (executor briefing
   * foundation). Changing the contract affects future tasks, never this
   * snapshot, unless immediate hard-security policy requires otherwise.
   */
  readonly executionContract: { readonly id: string; readonly revision: number } | null;
  /** Milestone-manifest identity bound at task start, when one applies. */
  readonly milestoneManifest: { readonly id: string; readonly version: number } | null;
  /** Fingerprint of the executor brief compiled at task start, when any. */
  readonly executorBriefFingerprint: string | null;
  /**
   * Digest of the exact execution-input environment at task start (ADR
   * 0028), when the host recorded it.
   */
  readonly executionInputDigest: string | null;
  /** Digest of the exact guidance manifest at task start (ADR 0028). */
  readonly guidanceManifestDigest: string | null;
  /** Digest of the projected tool-surface manifest at task start (ADR 0028). */
  readonly toolSurfaceManifestDigest: string | null;
  /** Digest of the effective capability snapshot at task start (ADR 0028). */
  readonly capabilitySnapshotDigest: string | null;
}

export interface TaskRuntimeSnapshotSources {
  readonly runtimeVersion: string;
  readonly provider: TaskRuntimeSnapshotProviderIdentity | null;
  readonly sandboxProfileId: string | null;
  readonly capabilityPolicyRevision: string | null;
  readonly workspaceIdentity: string | null;
  readonly godotEngineFingerprint: string | null;
  readonly workflow: TaskRuntimeSnapshotWorkflowIdentity | null;
  readonly instructionSetRevision?: string | null;
  readonly knowledgeStateRevision?: string | null;
  readonly referenceRevisions?: readonly { alias: ReferenceAlias; revision: ReferenceRevision }[];
  readonly executionContract?: { readonly id: string; readonly revision: number } | null;
  readonly milestoneManifest?: { readonly id: string; readonly version: number } | null;
  readonly executorBriefFingerprint?: string | null;
  /** Exact execution-input digest at task start (ADR 0028). */
  readonly executionInputDigest?: string | null;
  /** Guidance-manifest digest at task start (ADR 0028). */
  readonly guidanceManifestDigest?: string | null;
  /** Tool-surface-manifest digest at task start (ADR 0028). */
  readonly toolSurfaceManifestDigest?: string | null;
  /** Capability-snapshot digest at task start (ADR 0028). */
  readonly capabilitySnapshotDigest?: string | null;
}

/** Deterministic revision identity for a capability policy. */
export function capabilityPolicyFingerprint(policy: CapabilityPolicy): string {
  return sha256Hex(canonicalizeJson(policy.rules));
}

export function createTaskRuntimeSnapshot(
  sources: TaskRuntimeSnapshotSources,
  now: () => number = Date.now,
): TaskRuntimeSnapshot {
  const snapshot: TaskRuntimeSnapshot = {
    capturedAtMs: now(),
    runtimeVersion: sources.runtimeVersion,
    provider: sources.provider === null ? null : { ...sources.provider },
    sandboxProfileId: sources.sandboxProfileId,
    capabilityPolicyRevision: sources.capabilityPolicyRevision,
    workspaceIdentity: sources.workspaceIdentity,
    godotEngineFingerprint: sources.godotEngineFingerprint,
    workflow: sources.workflow === null ? null : { ...sources.workflow },
    instructionSetRevision: sources.instructionSetRevision ?? null,
    knowledgeStateRevision: sources.knowledgeStateRevision ?? null,
    referenceRevisions: (sources.referenceRevisions ?? [])
      .slice(0, 16)
      .map((entry) => ({ alias: entry.alias, revision: entry.revision })),
    executionContract:
      sources.executionContract === null || sources.executionContract === undefined
        ? null
        : { id: sources.executionContract.id, revision: sources.executionContract.revision },
    milestoneManifest:
      sources.milestoneManifest === null || sources.milestoneManifest === undefined
        ? null
        : { id: sources.milestoneManifest.id, version: sources.milestoneManifest.version },
    executorBriefFingerprint: sources.executorBriefFingerprint ?? null,
    executionInputDigest: sources.executionInputDigest ?? null,
    guidanceManifestDigest: sources.guidanceManifestDigest ?? null,
    toolSurfaceManifestDigest: sources.toolSurfaceManifestDigest ?? null,
    capabilitySnapshotDigest: sources.capabilitySnapshotDigest ?? null,
  };
  return deepFreeze(snapshot);
}
