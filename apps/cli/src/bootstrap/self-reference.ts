import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import {
  createSelfReference,
  type CapabilityPolicy,
  type SelfReference,
  type RegisteredToolInfo,
} from "@solaris/core";

/**
 * Composition-root self-reference builder (Stage 3 milestone 6).
 *
 * The installed-version identity comes from the installed package metadata
 * (the CLI package's own package.json), NOT from model memory or from
 * source control. In packaged builds the package.json ships with the
 * installed files, so the version is authoritative for the exact runtime.
 * No Git/build metadata is invented when unavailable.
 */

export function readInstalledSolarisVersion(): string {
  try {
    // The module lives at <package>/bootstrap/self-reference.js, so the
    // installed package.json is two levels up (src and dist both keep the
    // same relative layout).
    const packagePath = fileURLToPath(new URL("../../package.json", import.meta.url));
    const pkg = JSON.parse(readFileSync(packagePath, "utf8")) as { readonly version?: unknown };
    return typeof pkg.version === "string" && pkg.version.length > 0 ? pkg.version : "unknown";
  } catch {
    return "unknown";
  }
}

export function runningNodeMajor(): number {
  const major = Number(process.versions.node.split(".")[0]);
  return Number.isFinite(major) ? major : 0;
}

export function createRuntimeSelfReference(input: {
  readonly registeredTools: readonly RegisteredToolInfo[];
  readonly sandboxProfileId: string;
  readonly policy: CapabilityPolicy;
}): SelfReference {
  return createSelfReference({
    runtime: {
      version: readInstalledSolarisVersion(),
      nodeMajor: runningNodeMajor(),
      platform: process.platform,
    },
    registeredTools: input.registeredTools,
    sandboxProfileId: input.sandboxProfileId,
    policy: input.policy,
  });
}
