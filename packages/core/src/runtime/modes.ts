import { computeArtifactDigest } from "../identity/artifact-digest.js";
import { createRunId, type PhaseId, type RunId } from "./identity.js";

/**
 * Runtime modes and RunManifest (Stage 3 — Runtime Readiness &
 * Operational Resilience, ADR 0031).
 *
 * Runtime modes are explicit capability dimensions: Godot available
 * never implies visual runtime available. The RunManifest is an
 * immutable description of what WOULD be run — H3 never executes it.
 */

export type RuntimeMode = "headless" | "visual";

export const RUNTIME_MODES: readonly RuntimeMode[] = ["headless", "visual"] as const;

export type RuntimeCapabilityState =
  "supported" | "available" | "configured" | "degraded" | "blocked" | "unsupported";

export interface RuntimeModeCapabilityInput {
  readonly mode: RuntimeMode;
  readonly godotAvailable: boolean;
  readonly sandboxSupportsMode: boolean;
  /** Display availability (required for visual; null when unknown). */
  readonly displayAvailable: boolean | null;
  readonly platform: string | null;
}

/**
 * Deterministic mode capability evaluation. Visual mode is never assumed
 * from Godot availability: display and sandbox support are explicit
 * requirements.
 */
export function evaluateRuntimeModeCapability(input: RuntimeModeCapabilityInput): {
  readonly state: RuntimeCapabilityState;
  readonly reasons: readonly string[];
} {
  const reasons: string[] = [];
  if (!input.godotAvailable) {
    return { state: "blocked", reasons: ["Godot executable unavailable"] };
  }
  if (!input.sandboxSupportsMode) {
    return { state: "blocked", reasons: [`sandbox does not support ${input.mode} mode`] };
  }
  if (input.mode === "visual") {
    if (input.displayAvailable === false) {
      return { state: "blocked", reasons: ["no display available"] };
    }
    if (input.displayAvailable === null) {
      return { state: "degraded", reasons: ["display availability unknown; visual mode degraded"] };
    }
    reasons.push("display available");
  }
  reasons.push(`${input.mode} mode supported`);
  return { state: "available", reasons };
}

// ---------------------------------------------------------------------------
// RunManifest
// ---------------------------------------------------------------------------

export interface RunManifestInput {
  readonly taskId: string;
  readonly phaseId: PhaseId;
  readonly runId: RunId;
  readonly taskContractDigest: string | null;
  readonly phaseContractDigest: string | null;
  readonly executionInputDigest: string | null;
  readonly reproducibilityDigest: string | null;
  readonly godotExecutableFingerprint: string | null;
  /** Project/workspace identity (opaque host fingerprint). */
  readonly projectIdentity: string | null;
  readonly runtimeMode: RuntimeMode;
  readonly sandboxProfileId: string | null;
  readonly sideEffectPolicyDigest: string | null;
  readonly resourceBudgetDigest: string | null;
  readonly environmentDigest: string | null;
}

export interface RunManifest {
  readonly inputs: RunManifestInput;
  /** Canonical H1 digest over the exact would-be-run description. */
  readonly digest: string;
}

export function createRunManifest(input: RunManifestInput): RunManifest {
  const digest = computeArtifactDigest({
    artifactType: "RunManifest",
    schemaVersion: 1,
    payload: { ...input },
  });
  return { inputs: { ...input }, digest: digest.value };
}

/** Deterministic run id for a manifest (idempotent for equal inputs). */
export function runIdForManifest(manifest: RunManifest): RunId {
  return createRunId({
    taskId: manifest.inputs.taskId,
    phaseId: manifest.inputs.phaseId,
    sequence: 1,
    kind: "runtime",
  });
}

/** Human-readable manifest projection (bounded; never executes). */
export function renderRunManifest(manifest: RunManifest): string {
  const input = manifest.inputs;
  return [
    `RunManifest ${manifest.digest.slice(0, 12)}\u2026`,
    `task=${input.taskId} phase=${input.phaseId} run=${input.runId}`,
    `mode=${input.runtimeMode} sandbox=${input.sandboxProfileId ?? "none"}`,
    `godot=${input.godotExecutableFingerprint?.slice(0, 12) ?? "none"}\u2026`,
    `reproducibility=${input.reproducibilityDigest?.slice(0, 12) ?? "none"}\u2026`,
  ].join("\n");
}
