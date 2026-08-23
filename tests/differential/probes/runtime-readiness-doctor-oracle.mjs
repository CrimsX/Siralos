/**
 * runtime-readiness.doctor oracle probe (differential harness, ADR
 * 0033, Stage 3R R10c).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Exercises fail-closed readiness evaluation and the doctor surface
 * from the REAL TypeScript reference
 * (packages/core/src/runtime/{readiness,doctor}.ts).
 */
import { readFileSync } from "node:fs";
import { buildRuntimeReadinessDiagnostic } from "../../../packages/core/src/runtime/doctor.js";
import {
  evaluateRuntimeReadiness,
  executionAllowed,
  renderRuntimeReadiness,
} from "../../../packages/core/src/runtime/readiness.js";

const MAX_INPUT_BYTES = 64 * 1024;

const input = (() => {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
})();

const capabilities = (base) => ({
  godotExecutable: {
    available: base.godotAvailable,
    fingerprint: base.godotFingerprint,
  },
  projectIdentity: base.projectIdentity,
  sandboxBackend: {
    available: base.sandboxAvailable,
    supportsProcessSupervision: base.processSupervisionSupported,
  },
  filesystemIsolation: {
    available: base.filesystemIsolationAvailable,
    userDataRedirect: base.userDataRedirectAvailable,
  },
  networkPolicyResolvable: base.networkPolicyResolvable,
  artifactStorageAvailable: base.artifactStorageAvailable,
  displayAvailable: base.displayAvailable === undefined ? null : base.displayAvailable,
  resourceLimitCapabilities: { memory: false, cpu: false },
});

const op = input.op;

if (op === "readiness") {
  const manifest = evaluateRuntimeReadiness({
    ...capabilities(input.capabilities),
    runtimeMode: input.mode,
  });
  process.stdout.write(
    JSON.stringify({
      ready: manifest.ready,
      executionAllowed: executionAllowed(manifest),
      blockedReasons: manifest.blockedReasons,
      items: manifest.items.map((entry) => ({
        id: entry.id,
        state: entry.state,
        detail: entry.detail,
      })),
      digest: manifest.digest,
      rendered: renderRuntimeReadiness(manifest),
    }),
  );
} else if (op === "diagnostic") {
  // doctor.ts consumes the flat declared-capability set directly.
  const diagnostic = buildRuntimeReadinessDiagnostic({
    godotAvailable: input.capabilities.godotAvailable,
    godotFingerprint: input.capabilities.godotFingerprint ?? null,
    projectIdentity: input.capabilities.projectIdentity ?? null,
    sandboxAvailable: input.capabilities.sandboxAvailable,
    processSupervisionSupported: input.capabilities.processSupervisionSupported,
    filesystemIsolationAvailable: input.capabilities.filesystemIsolationAvailable,
    userDataRedirectAvailable: input.capabilities.userDataRedirectAvailable,
    networkPolicyResolvable: input.capabilities.networkPolicyResolvable,
    artifactStorageAvailable: input.capabilities.artifactStorageAvailable,
    displayAvailable:
      input.capabilities.displayAvailable === undefined
        ? null
        : input.capabilities.displayAvailable,
  });
  process.stdout.write(
    JSON.stringify({
      headless: {
        ready: diagnostic.headless.ready,
        digest: diagnostic.headless.digest,
      },
      visual: {
        ready: diagnostic.visual.ready,
        digest: diagnostic.visual.digest,
      },
    }),
  );
} else {
  throw new Error(`unknown runtime-readiness.doctor op ${JSON.stringify(op)}`);
}
