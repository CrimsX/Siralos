import { createEnvironmentManifest, type EnvironmentManifestInput } from "./environment.js";

/**
 * Determinism doctor surface (Stage 3 — Deterministic Execution &
 * Reproducibility, ADR 0029). Read-only, offline, no mutation.
 */

export interface DeterminismDiagnosticResult {
  readonly clockMode: "system" | "fixed" | "unknown";
  readonly randomnessMode: "none" | "seeded" | "system" | "unknown";
  readonly localePolicy: string | null;
  readonly timezonePolicy: string | null;
  /** Digest of the execution-relevant environment snapshot; null when absent. */
  readonly environmentDigest: string | null;
  /** Digest of the recorded reproducibility manifest; null when absent. */
  readonly reproducibilityDigest: string | null;
  /** Static host guarantees provided by core policy modules. */
  readonly staticGuarantees: {
    readonly fileOrderingNormalized: boolean;
    readonly documentationSelectionDeterministic: boolean;
    readonly workspaceScopeDeterministic: boolean;
    readonly validationSelectionDeterministic: boolean;
    readonly toolSurfaceFingerprinted: boolean;
    readonly acceptanceDeterministic: boolean;
    readonly nondeterminismAuditClean: boolean;
  };
}

/** Build the environment manifest from explicit runtime observations. */
export function buildRuntimeEnvironmentManifest(input: {
  readonly siralosVersion: string | null;
  readonly nodeVersion: string | null;
  readonly npmVersion: string | null;
  readonly platform: string | null;
  readonly arch: string | null;
  readonly osRelease: string | null;
  readonly godotExecutableFingerprint?: string | null;
  readonly sandboxBackendId?: string | null;
  readonly sandboxVersion?: string | null;
  readonly localePolicy?: string | null;
  readonly timezonePolicy?: string | null;
  readonly environmentAllowlist?: readonly string[];
}): EnvironmentManifestInput {
  return {
    siralosVersion: input.siralosVersion,
    nodeVersion: input.nodeVersion,
    npmVersion: input.npmVersion,
    platform: input.platform === null ? null : String(input.platform),
    arch: input.arch,
    osRelease: input.osRelease,
    godotExecutableFingerprint: input.godotExecutableFingerprint ?? null,
    sandboxBackendId: input.sandboxBackendId ?? null,
    sandboxVersion: input.sandboxVersion ?? null,
    localePolicy: input.localePolicy ?? "C",
    timezonePolicy: input.timezonePolicy ?? "UTC",
    environmentAllowlist: input.environmentAllowlist ?? [],
    toolIdentities: [],
  };
}

export { createEnvironmentManifest };
