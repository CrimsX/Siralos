import { computeArtifactDigest } from "./artifact-digest.js";

/** Deterministic code-unit comparison (locale-independent; stable across hosts). */
function compareCodeUnits(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

/**
 * Digest-backed manifests (Stage 3 — Content Identity & Delta
 * Verification, ADR 0028).
 *
 * Manifests reference exact identities (digests) of the artifacts they
 * cover — they never duplicate giant artifact contents. Deltas between
 * manifests project what materially changed; the authoritative state
 * always remains the full current artifact.
 */

export interface GuidanceManifestEntry {
  readonly id: string;
  readonly kind: "root-agents" | "nested-agents" | "architecture" | "adr" | "development";
  readonly path: string;
  /** SHA-256 of the exact document content. */
  readonly digest: string;
}

/** Exact active documentation/instructions selected for a task (3.7B). */
export interface GuidanceManifest {
  readonly entries: readonly GuidanceManifestEntry[];
  readonly aggregateDigest: string;
}

export function createGuidanceManifest(
  entries: readonly GuidanceManifestEntry[],
): GuidanceManifest {
  const sorted = [...entries].sort((a, b) => compareCodeUnits(a.path, b.path));
  const digest = computeArtifactDigest({
    artifactType: "GuidanceManifest",
    schemaVersion: 1,
    payload: {
      entries: sorted.map((entry) => ({
        id: entry.id,
        kind: entry.kind,
        path: entry.path,
        digest: entry.digest,
      })),
    },
  });
  return { entries: sorted, aggregateDigest: digest.value };
}

/** Derived delta between two guidance manifests. */
export interface GuidanceDelta {
  readonly baseDigest: string;
  readonly resultDigest: string;
  readonly added: readonly string[];
  readonly removed: readonly string[];
  readonly changed: readonly string[];
  readonly unchanged: readonly string[];
  readonly unchangedContent: boolean;
}

export function computeGuidanceDelta(
  base: GuidanceManifest,
  result: GuidanceManifest,
): GuidanceDelta {
  const baseByPath = new Map(base.entries.map((entry) => [entry.path, entry]));
  const resultByPath = new Map(result.entries.map((entry) => [entry.path, entry]));
  const added: string[] = [];
  const removed: string[] = [];
  const changed: string[] = [];
  const unchanged: string[] = [];
  for (const path of resultByPath.keys()) {
    if (!baseByPath.has(path)) {
      added.push(path);
    }
  }
  for (const path of baseByPath.keys()) {
    if (!resultByPath.has(path)) {
      removed.push(path);
    }
  }
  for (const [path, baseEntry] of baseByPath) {
    const resultEntry = resultByPath.get(path);
    if (resultEntry === undefined) {
      continue;
    }
    if (baseEntry.digest === resultEntry.digest && baseEntry.kind === resultEntry.kind) {
      unchanged.push(path);
    } else {
      changed.push(path);
    }
  }
  return {
    baseDigest: base.aggregateDigest,
    resultDigest: result.aggregateDigest,
    added,
    removed,
    changed,
    unchanged,
    unchangedContent: added.length === 0 && removed.length === 0 && changed.length === 0,
  };
}

export type ToolSurfaceRole = "planner" | "developer" | "reviewer" | "executor";
export type ToolSurfacePhase = "planning" | "inspection" | "mutation" | "review" | "repair";

export interface ToolSurfaceEntry {
  readonly name: string;
  /** Canonical hash of the exact provider-visible tool schema. */
  readonly schemaDigest: string;
}

/** Per-role/per-phase projected tool surface (actual provider schemas). */
export interface ToolSurfaceManifest {
  readonly role: ToolSurfaceRole;
  readonly phase: ToolSurfacePhase;
  readonly tools: readonly ToolSurfaceEntry[];
  readonly digest: string;
}

export function createToolSurfaceManifest(input: {
  readonly role: ToolSurfaceRole;
  readonly phase: ToolSurfacePhase;
  /** Provider-visible tool definitions (name + exact schema). */
  readonly tools: readonly {
    readonly name: string;
    readonly inputSchema: unknown;
    readonly description: string;
  }[];
}): ToolSurfaceManifest {
  const sorted = [...input.tools].sort((a, b) => compareCodeUnits(a.name, b.name));
  const entries: ToolSurfaceEntry[] = sorted.map((tool) => ({
    name: tool.name,
    schemaDigest: computeArtifactDigest({
      artifactType: "ToolSchema",
      schemaVersion: 1,
      payload: { name: tool.name, description: tool.description, inputSchema: tool.inputSchema },
    }).value,
  }));
  const digest = computeArtifactDigest({
    artifactType: "ToolSurfaceManifest",
    schemaVersion: 1,
    payload: { role: input.role, phase: input.phase, tools: entries },
  });
  return { role: input.role, phase: input.phase, tools: entries, digest: digest.value };
}

/** Derived semantic surface delta between two roles/phases. */
export interface ToolSurfaceDelta {
  readonly baseRole: ToolSurfaceRole;
  readonly resultRole: ToolSurfaceRole;
  readonly baseDigest: string;
  readonly resultDigest: string;
  readonly added: readonly string[];
  readonly removed: readonly string[];
  readonly changed: readonly string[];
  readonly retained: readonly string[];
}

export function computeToolSurfaceDelta(
  base: ToolSurfaceManifest,
  result: ToolSurfaceManifest,
): ToolSurfaceDelta {
  const baseByName = new Map(base.tools.map((tool) => [tool.name, tool]));
  const resultByName = new Map(result.tools.map((tool) => [tool.name, tool]));
  const added: string[] = [];
  const removed: string[] = [];
  const changed: string[] = [];
  const retained: string[] = [];
  for (const name of resultByName.keys()) {
    if (!baseByName.has(name)) {
      added.push(name);
    }
  }
  for (const name of baseByName.keys()) {
    if (!resultByName.has(name)) {
      removed.push(name);
    }
  }
  for (const [name, baseTool] of baseByName) {
    const resultTool = resultByName.get(name);
    if (resultTool === undefined) {
      continue;
    }
    if (baseTool.schemaDigest === resultTool.schemaDigest) {
      retained.push(name);
    } else {
      changed.push(name);
    }
  }
  return {
    baseRole: base.role,
    resultRole: result.role,
    baseDigest: base.digest,
    resultDigest: result.digest,
    added,
    removed,
    changed,
    retained,
  };
}

/** Effective task-visible capability state identity (host-owned, secret-free). */
export function computeCapabilitySnapshotDigest(snapshot: unknown): string {
  return computeArtifactDigest({
    artifactType: "CapabilitySnapshot",
    schemaVersion: 1,
    payload: snapshot,
  }).value;
}

export interface ExecutionInputReference {
  readonly id: string;
  readonly revision: number | null;
  /** Exact content digest of the referenced artifact; null when unknown. */
  readonly digest: string | null;
}

/** Immutable exact effective input environment of one execution iteration. */
export interface ExecutionInputManifest {
  readonly taskId: string;
  readonly iteration: number;
  readonly inputs: readonly ExecutionInputReference[];
  readonly digest: string;
}

export function createExecutionInputManifest(input: {
  readonly taskId: string;
  readonly iteration: number;
  /** Id-keyed input references: taskContract, taskPlan, executionContract, milestone, guidance, toolSurface, capability, workspaceScope, sourceRevisions. */
  readonly inputs: readonly ExecutionInputReference[];
}): ExecutionInputManifest {
  const sorted = [...input.inputs].sort((a, b) => compareCodeUnits(a.id, b.id));
  const digest = computeArtifactDigest({
    artifactType: "ExecutionInputManifest",
    schemaVersion: 1,
    payload: { taskId: input.taskId, iteration: input.iteration, inputs: sorted },
  });
  return { taskId: input.taskId, iteration: input.iteration, inputs: sorted, digest: digest.value };
}

/** Derived semantic delta between two execution input manifests. */
export interface ExecutionInputDelta {
  readonly baseDigest: string;
  readonly resultDigest: string;
  readonly changed: readonly {
    readonly id: string;
    readonly before: string | null;
    readonly after: string | null;
  }[];
  readonly unchanged: readonly string[];
  readonly unchangedContent: boolean;
}

export function computeExecutionInputDelta(
  base: ExecutionInputManifest,
  result: ExecutionInputManifest,
): ExecutionInputDelta {
  const baseById = new Map(base.inputs.map((input) => [input.id, input]));
  const resultById = new Map(result.inputs.map((input) => [input.id, input]));
  const changed: {
    readonly id: string;
    readonly before: string | null;
    readonly after: string | null;
  }[] = [];
  const unchanged: string[] = [];
  for (const [id, baseInput] of baseById) {
    const resultInput = resultById.get(id);
    if (resultInput === undefined) {
      changed.push({ id, before: baseInput.digest, after: null });
      continue;
    }
    if (baseInput.digest === resultInput.digest && baseInput.revision === resultInput.revision) {
      unchanged.push(id);
    } else {
      changed.push({ id, before: baseInput.digest, after: resultInput.digest });
    }
  }
  for (const [id, resultInput] of resultById) {
    if (!baseById.has(id)) {
      changed.push({ id, before: null, after: resultInput.digest });
    }
  }
  return {
    baseDigest: base.digest,
    resultDigest: result.digest,
    changed,
    unchanged,
    unchangedContent: changed.length === 0,
  };
}

/** Stable identity of one structured validation run. */
export interface ValidationResultIdentity {
  readonly validationId: string;
  /** Identity of the validation plan/command that produced the result. */
  readonly planIdentity: string | null;
  readonly resultDigest: string;
  readonly evidenceRefs: readonly string[];
}

export function createValidationResultIdentity(input: {
  readonly validationId: string;
  readonly planIdentity?: string | null;
  readonly result: unknown;
  readonly evidenceRefs?: readonly string[];
}): ValidationResultIdentity {
  const resultDigest = computeArtifactDigest({
    artifactType: "ValidationResult",
    schemaVersion: 1,
    payload: input.result,
  }).value;
  return {
    validationId: input.validationId,
    planIdentity: input.planIdentity ?? null,
    resultDigest,
    evidenceRefs: [...(input.evidenceRefs ?? [])],
  };
}

/** Derived semantic delta between two validation results. */
export interface ValidationDelta {
  readonly baseIdentity: string | null;
  readonly resultIdentity: string;
  readonly newlyPassing: readonly string[];
  readonly stillFailing: readonly string[];
  readonly newFailures: readonly string[];
  readonly unchangedIds: readonly string[];
}

export function computeValidationDelta(
  base: readonly { readonly id: string; readonly passed: boolean }[],
  result: readonly { readonly id: string; readonly passed: boolean }[],
  identities?: { readonly baseIdentity?: string | null; readonly resultIdentity?: string | null },
): ValidationDelta {
  const baseById = new Map(base.map((item) => [item.id, item]));
  const resultById = new Map(result.map((item) => [item.id, item]));
  const newlyPassing: string[] = [];
  const stillFailing: string[] = [];
  const newFailures: string[] = [];
  const unchangedIds: string[] = [];
  for (const [id, resultItem] of resultById) {
    const baseItem = baseById.get(id);
    if (baseItem === undefined) {
      // A result item that did not exist before: passing items are
      // newly passing; failing items are new failures.
      if (resultItem.passed) {
        newlyPassing.push(id);
      } else {
        newFailures.push(id);
      }
      continue;
    }
    if (baseItem.passed === resultItem.passed) {
      unchangedIds.push(id);
      if (!resultItem.passed) {
        stillFailing.push(id);
      }
    } else if (resultItem.passed) {
      newlyPassing.push(id);
    } else {
      newFailures.push(id);
    }
  }
  return {
    baseIdentity: identities?.baseIdentity ?? null,
    resultIdentity: identities?.resultIdentity ?? "",
    newlyPassing,
    stillFailing,
    newFailures,
    unchangedIds,
  };
}

/** Review input identity: binds one review to its exact inputs. */
export interface ReviewInputManifest {
  readonly reviewId: string;
  readonly taskId: string;
  readonly taskContractDigest: string;
  /** Exact change-set/diff identity under review (unified or per-surface). */
  readonly changesetDigest: string;
  readonly reviewContextDigest: string | null;
  readonly acceptanceDigest: string;
  readonly validationEvidenceDigest: string | null;
  readonly sourceRevisions: readonly { readonly path: string; readonly revision: string }[];
  readonly digest: string;
}

export function createReviewInputManifest(
  input: Omit<ReviewInputManifest, "digest">,
): ReviewInputManifest {
  const digest = computeArtifactDigest({
    artifactType: "ReviewInputManifest",
    schemaVersion: 1,
    payload: {
      reviewId: input.reviewId,
      taskId: input.taskId,
      taskContractDigest: input.taskContractDigest,
      changesetDigest: input.changesetDigest,
      reviewContextDigest: input.reviewContextDigest,
      acceptanceDigest: input.acceptanceDigest,
      validationEvidenceDigest: input.validationEvidenceDigest,
      sourceRevisions: input.sourceRevisions,
    },
  });
  return { ...input, digest: digest.value };
}

/** Aggregate identity of the exact evidence set backing one acceptance result. */
export interface AcceptanceEvidenceManifest {
  readonly taskId: string;
  readonly criterionId: string | null;
  readonly evidence: readonly {
    readonly evidenceId: string;
    readonly kind: string;
    readonly digest: string;
  }[];
  readonly digest: string;
}

export function createAcceptanceEvidenceManifest(input: {
  readonly taskId: string;
  readonly criterionId?: string | null;
  readonly evidence: readonly {
    readonly evidenceId: string;
    readonly kind: string;
    readonly digest: string;
  }[];
}): AcceptanceEvidenceManifest {
  const sorted = [...input.evidence].sort((a, b) => compareCodeUnits(a.evidenceId, b.evidenceId));
  const digest = computeArtifactDigest({
    artifactType: "AcceptanceEvidenceManifest",
    schemaVersion: 1,
    payload: { taskId: input.taskId, criterionId: input.criterionId ?? null, evidence: sorted },
  });
  return {
    taskId: input.taskId,
    criterionId: input.criterionId ?? null,
    evidence: sorted,
    digest: digest.value,
  };
}

/** Convenience: digest of a canonical acceptance-criteria set. */
export function computeAcceptanceCriteriaDigest(
  criteria: readonly {
    readonly id: string;
    readonly description: string;
    readonly verificationKind: string;
  }[],
): string {
  return computeArtifactDigest({
    artifactType: "AcceptanceCriteria",
    schemaVersion: 1,
    payload: { criteria },
  }).value;
}

/** Convenience: digest of a canonical evidence set (id + kind + content). */
export function computeValidationEvidenceDigest(
  evidence: readonly { readonly id: string; readonly kind: string; readonly content: unknown }[],
): string {
  return computeArtifactDigest({
    artifactType: "ValidationEvidence",
    schemaVersion: 1,
    payload: { evidence },
  }).value;
}

export function canonicalChangesetIdentity(payload: unknown): string {
  return computeArtifactDigest({
    artifactType: "ChangeSet",
    schemaVersion: 1,
    payload,
  }).value;
}
