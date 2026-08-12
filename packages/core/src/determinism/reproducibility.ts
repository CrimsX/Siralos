import { computeArtifactDigest } from "../identity/artifact-digest.js";
import { computeSectionDelta } from "../identity/semantic-delta.js";

/**
 * Reproducibility manifest (Stage 3 — Deterministic Execution &
 * Reproducibility, ADR 0029).
 *
 * Immutable reference set identifying the exact authoritative inputs of
 * an execution: H1 artifact digests (never duplicated contents), source
 * revision set, validation profile, provider/model runtime profile,
 * clock policy, and RNG policy. A result identifies
 * `producedUnder: <ReproducibilityManifest digest>`.
 */

export interface ProviderInputIdentity {
  readonly providerRoute: string | null;
  readonly modelIdentity: string | null;
  readonly reasoningMode: string | null;
  readonly temperature: number | null;
  readonly topP: number | null;
  readonly seed: number | null;
  /** Behavior-affecting provider parameters (bounded, no secrets). */
  readonly parameters: readonly { readonly name: string; readonly value: string }[];
}

export function computeProviderInputIdentityDigest(provider: ProviderInputIdentity): string {
  return computeArtifactDigest({
    artifactType: "ProviderInputIdentity",
    schemaVersion: 1,
    payload: {
      providerRoute: provider.providerRoute,
      modelIdentity: provider.modelIdentity,
      reasoningMode: provider.reasoningMode,
      temperature: provider.temperature,
      topP: provider.topP,
      seed: provider.seed,
      parameters: [...provider.parameters].sort((a, b) =>
        a.name < b.name ? -1 : a.name > b.name ? 1 : 0,
      ),
    },
  }).value;
}

export interface ReproducibilityManifestInput {
  readonly taskId: string;
  readonly executionInputDigest: string | null;
  readonly environmentDigest: string | null;
  readonly taskContractDigest: string | null;
  readonly taskPlanDigest: string | null;
  readonly guidanceDigest: string | null;
  readonly toolSurfaceDigest: string | null;
  readonly capabilityDigest: string | null;
  readonly sourceRevisionSet: readonly { readonly path: string; readonly revision: string }[];
  readonly validationProfile: string | null;
  readonly providerInput: ProviderInputIdentity | null;
  /** Clock policy: "system" | "fixed" (+ the fixed value when applicable). */
  readonly clockPolicy: { readonly mode: "system" | "fixed"; readonly fixedMs: number | null };
  /** RNG policy: "none" | "seeded" (seed) | "system". */
  readonly rngPolicy: { readonly mode: "none" | "seeded" | "system"; readonly seed: number | null };
}

export interface ReproducibilityManifest {
  readonly inputs: ReproducibilityManifestInput;
  readonly digest: string;
}

export function createReproducibilityManifest(
  input: ReproducibilityManifestInput,
): ReproducibilityManifest {
  const sourceRevisions = [...input.sourceRevisionSet].sort((a, b) =>
    a.path < b.path ? -1 : a.path > b.path ? 1 : 0,
  );
  const providerDigest =
    input.providerInput === null ? null : computeProviderInputIdentityDigest(input.providerInput);
  const digest = computeArtifactDigest({
    artifactType: "ReproducibilityManifest",
    schemaVersion: 1,
    payload: {
      taskId: input.taskId,
      executionInputDigest: input.executionInputDigest,
      environmentDigest: input.environmentDigest,
      taskContractDigest: input.taskContractDigest,
      taskPlanDigest: input.taskPlanDigest,
      guidanceDigest: input.guidanceDigest,
      toolSurfaceDigest: input.toolSurfaceDigest,
      capabilityDigest: input.capabilityDigest,
      sourceRevisionSet: sourceRevisions,
      validationProfile: input.validationProfile,
      providerInputDigest: providerDigest,
      clockPolicy: input.clockPolicy,
      rngPolicy: input.rngPolicy,
    },
  });
  return { inputs: { ...input, sourceRevisionSet: sourceRevisions }, digest: digest.value };
}

export type ReproducibilitySection =
  | "executionInput"
  | "environment"
  | "taskContract"
  | "taskPlan"
  | "guidance"
  | "toolSurface"
  | "capability"
  | "sourceRevisions"
  | "validationProfile"
  | "providerInput"
  | "clockPolicy"
  | "rngPolicy";

export interface ReproducibilityDelta {
  readonly baseDigest: string;
  readonly resultDigest: string;
  readonly changed: readonly ReproducibilitySection[];
  readonly unchanged: readonly ReproducibilitySection[];
  readonly unchangedContent: boolean;
}

export function computeReproducibilityDelta(
  base: ReproducibilityManifest,
  result: ReproducibilityManifest,
): ReproducibilityDelta {
  const sectionKeys: readonly ReproducibilitySection[] = [
    "executionInput",
    "environment",
    "taskContract",
    "taskPlan",
    "guidance",
    "toolSurface",
    "capability",
    "sourceRevisions",
    "validationProfile",
    "providerInput",
    "clockPolicy",
    "rngPolicy",
  ];
  const sections: Record<ReproducibilitySection, unknown> = {
    executionInput: base.inputs.executionInputDigest,
    environment: base.inputs.environmentDigest,
    taskContract: base.inputs.taskContractDigest,
    taskPlan: base.inputs.taskPlanDigest,
    guidance: base.inputs.guidanceDigest,
    toolSurface: base.inputs.toolSurfaceDigest,
    capability: base.inputs.capabilityDigest,
    sourceRevisions: base.inputs.sourceRevisionSet,
    validationProfile: base.inputs.validationProfile,
    providerInput: base.inputs.providerInput,
    clockPolicy: base.inputs.clockPolicy,
    rngPolicy: base.inputs.rngPolicy,
  };
  const resultSections: Record<ReproducibilitySection, unknown> = {
    executionInput: result.inputs.executionInputDigest,
    environment: result.inputs.environmentDigest,
    taskContract: result.inputs.taskContractDigest,
    taskPlan: result.inputs.taskPlanDigest,
    guidance: result.inputs.guidanceDigest,
    toolSurface: result.inputs.toolSurfaceDigest,
    capability: result.inputs.capabilityDigest,
    sourceRevisions: result.inputs.sourceRevisionSet,
    validationProfile: result.inputs.validationProfile,
    providerInput: result.inputs.providerInput,
    clockPolicy: result.inputs.clockPolicy,
    rngPolicy: result.inputs.rngPolicy,
  };
  const { changed, unchanged } = computeSectionDelta(sections, resultSections, sectionKeys);
  return {
    baseDigest: base.digest,
    resultDigest: result.digest,
    changed: changed as ReproducibilitySection[],
    unchanged: unchanged as ReproducibilitySection[],
    unchangedContent: changed.length === 0,
  };
}
