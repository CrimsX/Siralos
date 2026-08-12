import { computeArtifactDigest } from "../identity/artifact-digest.js";
import { createOrderingPolicy } from "../determinism/context.js";
import type { ArtifactDependencyManifest } from "./artifacts.js";

/**
 * Targeted incremental staleness (Stage 3 — Interpretable Context
 * Architecture, ADR 0030).
 *
 * Staleness propagates ONLY along explicit dependency manifests:
 * unrelated repository changes never stale unrelated artifacts. A change
 * to one input invalidates the affected derived artifact (and only its
 * explicit downstream path); recomputation stays with the workflow
 * owners — this is not an incremental build system.
 */

export interface ArtifactStalenessInput {
  /** Dependency manifests of the derived artifacts under consideration. */
  readonly manifests: readonly ArtifactDependencyManifest[];
  /**
   * Current observable input digests keyed by input artifact type
   * (e.g. "TaskContract" -> its current content digest).
   */
  readonly currentInputDigests: Readonly<Record<string, string>>;
}

export interface ArtifactStalenessResult {
  /** artifactId -> staleness reason (only artifacts whose dependencies changed). */
  readonly stale: Readonly<Record<string, string>>;
  /** artifactIds verified current against their dependencies. */
  readonly current: readonly string[];
  /** Input types that changed but are inputs of no considered artifact. */
  readonly unrelatedChanges: readonly string[];
}

/**
 * Deterministic targeted staleness: for each manifest, compare each
 * recorded dependency digest to the CURRENT digest of that input type.
 * A mismatch marks the artifact stale with an explicit reason. Input
 * types consumed by no manifest are reported as unrelated changes.
 */
export function deriveArtifactStaleness(input: ArtifactStalenessInput): ArtifactStalenessResult {
  const ordering = createOrderingPolicy();
  const stale: Record<string, string> = {};
  const current: string[] = [];
  const consumed = new Set<string>();
  for (const manifest of input.manifests) {
    for (const dependency of manifest.dependsOn) {
      consumed.add(dependency.artifactType);
    }
  }
  const unrelatedChanges = Object.keys(input.currentInputDigests).filter(
    (artifactType) => !consumed.has(artifactType),
  );
  for (const manifest of ordering.stableSort(input.manifests, (entry) => entry.artifactId)) {
    const changed: string[] = [];
    const missing: string[] = [];
    for (const dependency of manifest.dependsOn) {
      const currentDigest = input.currentInputDigests[dependency.artifactType];
      if (currentDigest === undefined) {
        missing.push(`${dependency.artifactType}@${dependency.digest.slice(0, 8)}`);
      } else if (currentDigest !== dependency.digest) {
        changed.push(
          `${dependency.artifactType} ${dependency.digest.slice(0, 8)} -> ${currentDigest.slice(0, 8)}`,
        );
      }
    }
    if (changed.length > 0 || missing.length > 0) {
      const reasons: string[] = [];
      for (const entry of changed) {
        reasons.push(entry);
      }
      for (const entry of missing) {
        reasons.push(`${entry} no longer observable`);
      }
      stale[manifest.artifactId] = `stale because ${reasons.join("; ")}`;
    } else {
      current.push(manifest.artifactId);
    }
  }
  return {
    stale,
    current: ordering.stableSort(current, (id) => id),
    unrelatedChanges: ordering.stableSort(unrelatedChanges, (key) => key),
  };
}

/**
 * Prepared-mutation staleness: a prepared mutation binds exact source
 * revisions; a source revision change makes the prepared mutation stale
 * (no automatic mutation retry under the old preparation).
 */
export function isPreparedMutationStale(input: {
  readonly preparedSourceRevisions: readonly { readonly path: string; readonly revision: string }[];
  readonly currentSourceRevisions: Readonly<Record<string, string>>;
}): { readonly stale: boolean; readonly stalePaths: readonly string[] } {
  const stalePaths: string[] = [];
  for (const prepared of input.preparedSourceRevisions) {
    const current = input.currentSourceRevisions[prepared.path];
    if (current === undefined || current !== prepared.revision) {
      stalePaths.push(prepared.path);
    }
  }
  return { stale: stalePaths.length > 0, stalePaths };
}

/** Deterministic digest of a staleness result (for evidence identity). */
export function computeStalenessDigest(result: ArtifactStalenessResult): string {
  return computeArtifactDigest({
    artifactType: "ArtifactStalenessResult",
    schemaVersion: 1,
    payload: {
      stale: Object.keys(result.stale)
        .sort()
        .map((id) => ({ id, reason: result.stale[id] })),
      current: result.current,
      unrelatedChanges: result.unrelatedChanges,
    },
  }).value;
}
