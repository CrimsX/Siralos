import { computeArtifactDigest } from "../identity/artifact-digest.js";
import type { RunId } from "./identity.js";

/**
 * Runtime side-effect policy and run-owned filesystem boundaries
 * (Stage 3 — Runtime Readiness & Operational Resilience, ADR 0031).
 *
 * The policy is host-owned and may narrow but never broaden sandbox/
 * security authority. Run-owned state is host-resolved and containment-
 * checked: cleanup never operates on arbitrary workspace paths based on
 * model-provided strings.
 */

export type RuntimeNetworkPolicy = "denied" | "loopback" | "explicit_allowlist";

export interface RuntimeSideEffectPolicy {
  /** Source workspace is always protected. */
  readonly sourceWorkspace: "protected";
  /** Disposable runtime workspace/copy is runtime-mutable. */
  readonly disposableRuntimeWorkspace: "runtime_mutable";
  /** Runtime user-data location is redirected and run-owned. */
  readonly userData: "redirected_run_owned";
  /** Temporary files are run-owned. */
  readonly tempFiles: "run_owned";
  readonly network: RuntimeNetworkPolicy;
  /** Child processes are supervised. */
  readonly childProcesses: "supervised";
  /** Environment is allowlisted. */
  readonly environment: "allowlisted";
  /** Digest over the exact policy. */
  readonly digest: string;
}

export function createRuntimeSideEffectPolicy(input: {
  readonly network?: RuntimeNetworkPolicy;
}): RuntimeSideEffectPolicy {
  const network = input.network ?? "denied";
  if (network !== "denied" && network !== "loopback" && network !== "explicit_allowlist") {
    throw new Error(
      "A runtime side-effect network policy must be denied, loopback, or explicit_allowlist.",
    );
  }
  const policy: RuntimeSideEffectPolicy = {
    sourceWorkspace: "protected",
    disposableRuntimeWorkspace: "runtime_mutable",
    userData: "redirected_run_owned",
    tempFiles: "run_owned",
    network,
    childProcesses: "supervised",
    environment: "allowlisted",
    digest: "",
  };
  return {
    ...policy,
    digest: computeArtifactDigest({
      artifactType: "RuntimeSideEffectPolicy",
      schemaVersion: 1,
      payload: { ...policy },
    }).value,
  };
}

/**
 * The policy can NEVER authorize source mutation: a run plan whose
 * targets are source-workspace paths is unsafe by construction.
 */
export function authorizesSourceMutation(_policy: RuntimeSideEffectPolicy): false {
  return false;
}

// ---------------------------------------------------------------------------
// Run-owned filesystem boundaries
// ---------------------------------------------------------------------------

export type RunOwnedPathKind = "project_copy" | "user_data" | "temp" | "output" | "artifacts";

export interface RunFilesystemBoundary {
  readonly runId: RunId;
  readonly roots: Readonly<Record<RunOwnedPathKind, string>>;
}

/** Host-resolved run-owned roots (never model-supplied). */
export function createRunFilesystemBoundary(input: {
  readonly runId: RunId;
  readonly hostRoots: Readonly<Record<RunOwnedPathKind, string>>;
}): RunFilesystemBoundary {
  for (const kind of Object.keys(input.hostRoots) as RunOwnedPathKind[]) {
    if (input.hostRoots[kind].length === 0) {
      throw new Error(`A run filesystem boundary requires a ${kind} root.`);
    }
  }
  return { runId: input.runId, roots: { ...input.hostRoots } };
}

const ABSOLUTE_PATTERN = /^([A-Za-z]:)?[\\/]/;

/**
 * Resolve a relative path inside a run-owned root with containment
 * checks: `..`, absolute, drive-qualified, and colon-bearing paths are
 * rejected. Model-controlled strings can never select arbitrary
 * external directories.
 */
export function resolveRunOwnedPath(
  boundary: RunFilesystemBoundary,
  kind: RunOwnedPathKind,
  relativePath: string,
):
  | { readonly status: "ok"; readonly absolutePath: string }
  | { readonly status: "rejected"; readonly message: string } {
  const root = boundary.roots[kind];
  if (root === undefined) {
    return { status: "rejected", message: `Unknown run-owned kind ${kind}.` };
  }
  const normalized = relativePath.replace(/\\/g, "/");
  if (normalized.length === 0) {
    return { status: "rejected", message: "A run-owned relative path must not be empty." };
  }
  if (normalized.includes("..")) {
    return { status: "rejected", message: "Run-owned paths must not contain '..'." };
  }
  if (ABSOLUTE_PATTERN.test(normalized) || normalized.includes(":")) {
    return {
      status: "rejected",
      message: "Run-owned paths must be workspace-relative to the run root.",
    };
  }
  const absolutePath = `${root.replace(/[\\/]+$/, "")}/${normalized}`;
  return { status: "ok", absolutePath };
}

/** Containment check for an already-resolved absolute path. */
export function isPathWithinRunRoot(
  boundary: RunFilesystemBoundary,
  kind: RunOwnedPathKind,
  absolutePath: string,
): boolean {
  const root = boundary.roots[kind];
  if (root === undefined) {
    return false;
  }
  const normalizedRoot = root.replace(/[\\/]+$/, "");
  const normalizedPath = absolutePath.replace(/[\\/]+$/, "");
  return normalizedPath === normalizedRoot || normalizedPath.startsWith(`${normalizedRoot}/`);
}

/**
 * Run-scoped cleanup descriptor: cleanup operates ONLY on host-owned run
 * roots, never on arbitrary paths derived from model strings.
 */
export function cleanupScopeForRun(boundary: RunFilesystemBoundary): readonly string[] {
  return Object.values(boundary.roots);
}
