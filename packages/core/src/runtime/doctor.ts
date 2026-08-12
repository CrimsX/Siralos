import { evaluateRuntimeReadiness, type RuntimeReadinessManifest } from "./readiness.js";

/**
 * Runtime readiness doctor surface (Stage 3 — Runtime Readiness &
 * Operational Resilience, ADR 0031). Read-only, offline, never launches
 * the project.
 */

export interface RuntimeReadinessDiagnosticResult {
  readonly headless: RuntimeReadinessManifest;
  readonly visual: RuntimeReadinessManifest;
}

export function buildRuntimeReadinessDiagnostic(input: {
  readonly godotAvailable: boolean;
  readonly godotFingerprint: string | null;
  readonly projectIdentity: string | null;
  readonly sandboxAvailable: boolean;
  readonly processSupervisionSupported: boolean;
  readonly filesystemIsolationAvailable: boolean;
  readonly userDataRedirectAvailable: boolean;
  readonly networkPolicyResolvable: boolean;
  readonly artifactStorageAvailable: boolean;
  readonly displayAvailable: boolean | null;
}): RuntimeReadinessDiagnosticResult {
  const base = {
    godotExecutable: { available: input.godotAvailable, fingerprint: input.godotFingerprint },
    projectIdentity: input.projectIdentity,
    sandboxBackend: {
      available: input.sandboxAvailable,
      supportsProcessSupervision: input.processSupervisionSupported,
    },
    filesystemIsolation: {
      available: input.filesystemIsolationAvailable,
      userDataRedirect: input.userDataRedirectAvailable,
    },
    networkPolicyResolvable: input.networkPolicyResolvable,
    artifactStorageAvailable: input.artifactStorageAvailable,
    displayAvailable: input.displayAvailable,
    resourceLimitCapabilities: { memory: false, cpu: false },
  };
  return {
    headless: evaluateRuntimeReadiness({ ...base, runtimeMode: "headless" }),
    visual: evaluateRuntimeReadiness({ ...base, runtimeMode: "visual" }),
  };
}
