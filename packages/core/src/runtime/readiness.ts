import { computeArtifactDigest } from "../identity/artifact-digest.js";
import { createOrderingPolicy } from "../determinism/context.js";
import type { RuntimeMode, RuntimeCapabilityState } from "./modes.js";

/**
 * Runtime readiness manifest and fail-closed evaluation (Stage 3 —
 * Runtime Readiness & Operational Resilience, ADR 0031).
 *
 * Readiness is deterministic from explicit capability inputs and FAILS
 * CLOSED: if the requested runtime mode requires an isolation/security
 * property that cannot be provided, readiness is blocked and no
 * execution request can proceed. Readiness evaluation never executes
 * Godot and never duplicates CapabilityDoctor semantics — it reports
 * the runtime-specific projection.
 */

export type ReadinessItemId =
  | "godot_executable"
  | "godot_fingerprint"
  | "project_identity"
  | "sandbox_backend"
  | "process_supervision"
  | "filesystem_isolation"
  | "user_data_isolation"
  | "network_policy"
  | "artifact_storage"
  | "headless_mode"
  | "visual_mode"
  | "display"
  | "resource_limits";

export interface ReadinessItem {
  readonly id: ReadinessItemId;
  readonly state: RuntimeCapabilityState;
  readonly detail: string;
}

export interface RuntimeReadinessManifest {
  readonly runtimeMode: RuntimeMode;
  readonly items: readonly ReadinessItem[];
  /** True only when every required item is available/supported. */
  readonly ready: boolean;
  /** Exactly why readiness is blocked (fail-closed evidence). */
  readonly blockedReasons: readonly string[];
  readonly digest: string;
}

export interface RuntimeReadinessInput {
  readonly runtimeMode: RuntimeMode;
  readonly godotExecutable: { readonly available: boolean; readonly fingerprint: string | null };
  readonly projectIdentity: string | null;
  readonly sandboxBackend: {
    readonly available: boolean;
    readonly supportsProcessSupervision: boolean;
  };
  readonly filesystemIsolation: { readonly available: boolean; readonly userDataRedirect: boolean };
  readonly networkPolicyResolvable: boolean;
  readonly artifactStorageAvailable: boolean;
  readonly displayAvailable: boolean | null;
  /** Which resource limits the backend can enforce/observe. */
  readonly resourceLimitCapabilities: { readonly memory: boolean; readonly cpu: boolean };
}

const REQUIRED_ITEMS: Readonly<Record<RuntimeMode, readonly ReadinessItemId[]>> = {
  headless: [
    "godot_executable",
    "godot_fingerprint",
    "project_identity",
    "sandbox_backend",
    "process_supervision",
    "filesystem_isolation",
    "user_data_isolation",
    "network_policy",
    "artifact_storage",
    "headless_mode",
  ],
  visual: [
    "godot_executable",
    "godot_fingerprint",
    "project_identity",
    "sandbox_backend",
    "process_supervision",
    "filesystem_isolation",
    "user_data_isolation",
    "network_policy",
    "artifact_storage",
    "visual_mode",
    "display",
  ],
};

function item(id: ReadinessItemId, state: RuntimeCapabilityState, detail: string): ReadinessItem {
  return { id, state, detail };
}

/**
 * Deterministic fail-closed readiness evaluation: equivalent capability
 * inputs produce the same manifest; any missing required isolation
 * property blocks the whole run request.
 */
export function evaluateRuntimeReadiness(input: RuntimeReadinessInput): RuntimeReadinessManifest {
  const ordering = createOrderingPolicy();
  const items: ReadinessItem[] = [];
  items.push(
    item(
      "godot_executable",
      input.godotExecutable.available ? "available" : "blocked",
      input.godotExecutable.available ? "Godot executable present" : "Godot executable unavailable",
    ),
    item(
      "godot_fingerprint",
      input.godotExecutable.fingerprint === null ? "unsupported" : "available",
      input.godotExecutable.fingerprint === null
        ? "no current Godot fingerprint"
        : `fingerprint ${input.godotExecutable.fingerprint.slice(0, 12)}\u2026`,
    ),
    item(
      "project_identity",
      input.projectIdentity === null ? "blocked" : "available",
      input.projectIdentity === null ? "no project identity" : "project identity resolved",
    ),
    item(
      "sandbox_backend",
      input.sandboxBackend.available ? "available" : "blocked",
      input.sandboxBackend.available ? "sandbox backend available" : "sandbox backend unavailable",
    ),
    item(
      "process_supervision",
      input.sandboxBackend.supportsProcessSupervision ? "available" : "unsupported",
      input.sandboxBackend.supportsProcessSupervision
        ? "process supervision supported"
        : "process supervision unsupported",
    ),
    item(
      "filesystem_isolation",
      input.filesystemIsolation.available ? "available" : "blocked",
      input.filesystemIsolation.available
        ? "filesystem isolation available"
        : "filesystem isolation unavailable",
    ),
    item(
      "user_data_isolation",
      input.filesystemIsolation.userDataRedirect ? "available" : "blocked",
      input.filesystemIsolation.userDataRedirect
        ? "user-data redirect available"
        : "user-data redirect unavailable",
    ),
    item(
      "network_policy",
      input.networkPolicyResolvable ? "configured" : "blocked",
      input.networkPolicyResolvable ? "network policy resolvable" : "network policy unresolvable",
    ),
    item(
      "artifact_storage",
      input.artifactStorageAvailable ? "available" : "blocked",
      input.artifactStorageAvailable
        ? "artifact storage available"
        : "artifact storage unavailable",
    ),
    item("headless_mode", "available", "headless runtime mode supported by the readiness contract"),
    item(
      "visual_mode",
      input.displayAvailable === false
        ? "blocked"
        : input.displayAvailable === null
          ? "degraded"
          : "available",
      input.displayAvailable === false
        ? "visual mode blocked: no display"
        : input.displayAvailable === null
          ? "visual mode degraded: display availability unknown"
          : "visual mode available: display present",
    ),
    item(
      "display",
      input.displayAvailable === false
        ? "blocked"
        : input.displayAvailable === null
          ? "degraded"
          : "available",
      input.displayAvailable === false
        ? "no display"
        : input.displayAvailable === null
          ? "display unknown"
          : "display available",
    ),
    item(
      "resource_limits",
      input.resourceLimitCapabilities.memory || input.resourceLimitCapabilities.cpu
        ? "available"
        : "degraded",
      `memory ${input.resourceLimitCapabilities.memory ? "enforced" : "not enforced"}; cpu ${
        input.resourceLimitCapabilities.cpu ? "enforced" : "not enforced"
      }`,
    ),
  );
  const required = REQUIRED_ITEMS[input.runtimeMode];
  const blockedReasons: string[] = [];
  for (const requiredId of required) {
    const entry = items.find((candidate) => candidate.id === requiredId);
    if (entry !== undefined && (entry.state === "blocked" || entry.state === "unsupported")) {
      blockedReasons.push(`${requiredId}: ${entry.detail}`);
    }
  }
  const ready = blockedReasons.length === 0;
  const ordered = ordering.stableSort(items, (entry) => entry.id);
  const digest = computeArtifactDigest({
    artifactType: "RuntimeReadinessManifest",
    schemaVersion: 1,
    payload: { runtimeMode: input.runtimeMode, items: ordered },
  }).value;
  return { runtimeMode: input.runtimeMode, items: ordered, ready, blockedReasons, digest };
}

/** Fail-closed gate: readiness must be ready before any execution request. */
export function executionAllowed(manifest: RuntimeReadinessManifest): boolean {
  return manifest.ready;
}

export function renderRuntimeReadiness(manifest: RuntimeReadinessManifest): string {
  const lines = [
    `Runtime readiness (${manifest.runtimeMode}): ${manifest.ready ? "ready" : "BLOCKED"}`,
  ];
  for (const entry of manifest.items) {
    lines.push(`  ${entry.id}: ${entry.state} — ${entry.detail}`);
  }
  if (!manifest.ready) {
    lines.push(`Blocked because:`);
    for (const reason of manifest.blockedReasons) {
      lines.push(`  - ${reason}`);
    }
  }
  return lines.join("\n");
}
