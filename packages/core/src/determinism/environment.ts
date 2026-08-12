import { computeArtifactDigest } from "../identity/artifact-digest.js";

/**
 * Environment manifest (Stage 3 — Deterministic Execution &
 * Reproducibility, ADR 0029).
 *
 * Bounded identity of the execution-relevant environment: build/version,
 * Node/npm identity, OS/platform/architecture, Godot fingerprint, sandbox
 * identity, explicit locale and timezone policy, and the relevant
 * environment allowlist. Secrets are never included. The H1 digest binds
 * into the ReproducibilityManifest.
 */

export interface EnvironmentManifestInput {
  readonly solarisVersion: string | null;
  readonly nodeVersion: string | null;
  readonly npmVersion: string | null;
  readonly platform: string | null;
  readonly arch: string | null;
  readonly osRelease: string | null;
  readonly godotExecutableFingerprint: string | null;
  readonly sandboxBackendId: string | null;
  readonly sandboxVersion: string | null;
  /** Explicit locale policy, e.g. "en-US" or "C" (never ambient). */
  readonly localePolicy: string | null;
  /** Explicit timezone policy, e.g. "UTC" (never ambient). */
  readonly timezonePolicy: string | null;
  /** Environment allowlist (names only, never values of secrets). */
  readonly environmentAllowlist: readonly string[];
  /** Tool executable identities relevant to execution (name + digest). */
  readonly toolIdentities: readonly { readonly name: string; readonly digest: string }[];
}

export interface EnvironmentManifest {
  readonly inputs: EnvironmentManifestInput;
  readonly digest: string;
}

export function createEnvironmentManifest(input: EnvironmentManifestInput): EnvironmentManifest {
  const sortedAllowlist = [...input.environmentAllowlist].sort();
  const sortedTools = [...input.toolIdentities].sort((a, b) =>
    a.name < b.name ? -1 : a.name > b.name ? 1 : 0,
  );
  const digest = computeArtifactDigest({
    artifactType: "EnvironmentManifest",
    schemaVersion: 1,
    payload: {
      solarisVersion: input.solarisVersion,
      nodeVersion: input.nodeVersion,
      npmVersion: input.npmVersion,
      platform: input.platform,
      arch: input.arch,
      osRelease: input.osRelease,
      godotExecutableFingerprint: input.godotExecutableFingerprint,
      sandboxBackendId: input.sandboxBackendId,
      sandboxVersion: input.sandboxVersion,
      localePolicy: input.localePolicy,
      timezonePolicy: input.timezonePolicy,
      environmentAllowlist: sortedAllowlist,
      toolIdentities: sortedTools,
    },
  });
  return {
    inputs: { ...input, environmentAllowlist: sortedAllowlist, toolIdentities: sortedTools },
    digest: digest.value,
  };
}

/** Bounded deterministic delta between two environment manifests. */
export interface EnvironmentDelta {
  readonly baseDigest: string;
  readonly resultDigest: string;
  readonly changed: readonly string[];
  readonly unchanged: readonly string[];
  readonly unchangedContent: boolean;
}

const ENVIRONMENT_SECTIONS = [
  "solarisVersion",
  "nodeVersion",
  "npmVersion",
  "platform",
  "arch",
  "osRelease",
  "godotExecutableFingerprint",
  "sandboxBackendId",
  "sandboxVersion",
  "localePolicy",
  "timezonePolicy",
  "environmentAllowlist",
  "toolIdentities",
] as const;

export function computeEnvironmentDelta(
  base: EnvironmentManifest,
  result: EnvironmentManifest,
): EnvironmentDelta {
  const changed: string[] = [];
  const unchanged: string[] = [];
  const canonical = (value: unknown): string =>
    JSON.stringify(value, Object.keys(value ?? {}).sort());
  for (const section of ENVIRONMENT_SECTIONS) {
    const baseValue = canonical(base.inputs[section]);
    const resultValue = canonical(result.inputs[section]);
    if (baseValue === resultValue) {
      unchanged.push(section);
    } else {
      changed.push(section);
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
