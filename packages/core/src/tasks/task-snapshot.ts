import { canonicalizeJson, sha256Hex } from "../godot/digest.js";
import type { CapabilityPolicy } from "../security/capability.js";

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
}

export interface TaskRuntimeSnapshotSources {
  readonly runtimeVersion: string;
  readonly provider: TaskRuntimeSnapshotProviderIdentity | null;
  readonly sandboxProfileId: string | null;
  readonly capabilityPolicyRevision: string | null;
  readonly workspaceIdentity: string | null;
  readonly godotEngineFingerprint: string | null;
  readonly workflow: TaskRuntimeSnapshotWorkflowIdentity | null;
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
  };
  return Object.freeze(snapshot);
}
