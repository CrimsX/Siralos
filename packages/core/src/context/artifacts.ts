import { computeArtifactDigest, type ArtifactDigest } from "../identity/artifact-digest.js";
import { createOrderingPolicy } from "../determinism/context.js";

/**
 * Workflow artifact identities, dependency manifests, and lineage
 * (Stage 3 — Interpretable Context Architecture, ADR 0030).
 *
 * Major phases communicate through typed, digest-bound artifacts rather
 * than conversation history. The envelope REFERENCES existing artifacts —
 * it is not a generic payload type. Dependency manifests record only
 * high-value explicit dependencies (H1 digests); lineage is bounded and
 * inspectable, never a recursive graph dump.
 */

export interface WorkflowArtifactIdentity {
  readonly artifactType: string;
  readonly schemaVersion: number;
  readonly revision?: number;
  readonly digest: ArtifactDigest;
  /** Digest of the execution input manifest this artifact was produced under. */
  readonly producedUnder: ArtifactDigest;
}

export function createWorkflowArtifactIdentity(input: {
  readonly artifactType: string;
  readonly schemaVersion: number;
  readonly revision?: number;
  readonly digest: ArtifactDigest;
  readonly producedUnder: ArtifactDigest;
}): WorkflowArtifactIdentity {
  if (input.artifactType.length === 0) {
    throw new Error("An artifact identity requires an artifact type.");
  }
  if (!Number.isSafeInteger(input.schemaVersion) || input.schemaVersion < 1) {
    throw new Error("An artifact schema version must be a positive safe integer.");
  }
  return {
    artifactType: input.artifactType,
    schemaVersion: input.schemaVersion,
    ...(input.revision === undefined ? {} : { revision: input.revision }),
    digest: { ...input.digest },
    producedUnder: { ...input.producedUnder },
  };
}

/** Concise human-readable projection (rendering never mutates state). */
export function renderArtifactIdentity(identity: WorkflowArtifactIdentity): string {
  return `${identity.artifactType} v${identity.schemaVersion}${
    identity.revision === undefined ? "" : ` rev ${identity.revision}`
  } / ${identity.digest.value.slice(0, 8)}\u2026 (produced under ${identity.producedUnder.value.slice(
    0,
    8,
  )}\u2026)`;
}

// ---------------------------------------------------------------------------
// Artifact dependency manifest
// ---------------------------------------------------------------------------

export interface ArtifactDependency {
  readonly artifactType: string;
  readonly digest: string;
}

export interface ArtifactDependencyManifest {
  readonly artifactType: string;
  readonly artifactId: string;
  readonly dependsOn: readonly ArtifactDependency[];
  readonly digest: string;
}

export function createArtifactDependencyManifest(input: {
  readonly artifactType: string;
  readonly artifactId: string;
  readonly dependsOn: readonly ArtifactDependency[];
}): ArtifactDependencyManifest {
  if (input.artifactType.length === 0 || input.artifactId.length === 0) {
    throw new Error("A dependency manifest requires an artifact type and id.");
  }
  if (input.dependsOn.length === 0) {
    throw new Error(
      `A dependency manifest for ${input.artifactType} requires at least one dependency.`,
    );
  }
  const ordered = createOrderingPolicy().stableSort(
    input.dependsOn.map((entry) => ({ ...entry })),
    (entry) => `${entry.artifactType}:${entry.digest}`,
  );
  const digest = computeArtifactDigest({
    artifactType: "ArtifactDependencyManifest",
    schemaVersion: 1,
    payload: { artifactType: input.artifactType, artifactId: input.artifactId, dependsOn: ordered },
  });
  return {
    artifactType: input.artifactType,
    artifactId: input.artifactId,
    dependsOn: ordered,
    digest: digest.value,
  };
}

/**
 * Deterministic dependency tables for the known high-value artifacts.
 * Each table lists ONLY the explicit inputs that materially affect the
 * derived artifact.
 */
export const HIGH_VALUE_DEPENDENCIES: Readonly<
  Record<string, readonly { readonly artifactType: string; readonly input: string }[]>
> = {
  TaskPlan: [
    { artifactType: "TaskContract", input: "taskContractDigest" },
    { artifactType: "GuidanceManifest", input: "guidanceDigest" },
    { artifactType: "SourceRevisions", input: "verifiedSourceRevisions" },
  ],
  ReviewVerdict: [
    { artifactType: "TaskContract", input: "taskContractDigest" },
    { artifactType: "Changeset", input: "changesetDigest" },
    { artifactType: "ReviewContextManifest", input: "reviewContextDigest" },
    { artifactType: "ValidationEvidence", input: "validationEvidenceDigest" },
  ],
  AcceptanceResult: [
    { artifactType: "AcceptanceCriteria", input: "acceptanceDigest" },
    { artifactType: "ValidationEvidence", input: "validationEvidenceDigest" },
    { artifactType: "ReviewVerdict", input: "reviewVerdictDigest" },
    { artifactType: "MutationVerificationEvidence", input: "mutationVerificationDigest" },
  ],
  PreparedChangeset: [
    { artifactType: "TaskPlan", input: "taskPlanDigest" },
    { artifactType: "SourceRevisions", input: "sourceRevisionDigests" },
  ],
};

/** Build the dependency manifest for a known artifact from its inputs. */
export function buildDependencyManifest(input: {
  readonly artifactType: string;
  readonly artifactId: string;
  readonly currentDigests: Readonly<Record<string, string | null>>;
}): ArtifactDependencyManifest | null {
  const table = HIGH_VALUE_DEPENDENCIES[input.artifactType];
  if (table === undefined) {
    return null;
  }
  const dependsOn: ArtifactDependency[] = [];
  for (const entry of table) {
    const digest = input.currentDigests[entry.input];
    if (digest !== null && digest !== undefined) {
      dependsOn.push({ artifactType: entry.artifactType, digest });
    }
  }
  if (dependsOn.length === 0) {
    return null;
  }
  return createArtifactDependencyManifest({
    artifactType: input.artifactType,
    artifactId: input.artifactId,
    dependsOn,
  });
}

// ---------------------------------------------------------------------------
// Lineage
// ---------------------------------------------------------------------------

export interface LineageLink {
  readonly artifactType: string;
  readonly artifactId: string;
  readonly digest: string;
}

/** Bounded lineage chain (newest first); never recursive graph dumps. */
export function computeArtifactLineage(
  manifests: readonly ArtifactDependencyManifest[],
  terminalArtifactId: string,
  maxDepth = 8,
): readonly LineageLink[] {
  const byId = new Map(manifests.map((manifest) => [manifest.artifactId, manifest]));
  const lineage: LineageLink[] = [];
  let current = byId.get(terminalArtifactId);
  while (current !== undefined && lineage.length < maxDepth) {
    lineage.push({
      artifactType: current.artifactType,
      artifactId: current.artifactId,
      digest: current.digest,
    });
    const nextDependency = current.dependsOn[0];
    if (nextDependency === undefined) {
      break;
    }
    current = manifests.find((manifest) => manifest.artifactType === nextDependency.artifactType);
  }
  return lineage;
}

/** Human-readable lineage projection: `AcceptanceResult ← ReviewVerdict ← ...`. */
export function renderLineage(lineage: readonly LineageLink[]): string {
  return lineage.map((link) => `${link.artifactType} ${link.digest.slice(0, 8)}`).join(" \u2190 ");
}
