/**
 * determinism-replay oracle probe (differential harness, ADR 0033,
 * Stage 3R R10a).
 *
 * Creates a reproducibility manifest using the REAL TypeScript reference
 * (packages/core/src/determinism/reproducibility.ts).
 */
import { readFileSync } from "node:fs";
import { createReproducibilityManifest } from "../../../packages/core/src/determinism/reproducibility.js";

const MAX_INPUT_BYTES = 64 * 1024;

const input = (() => {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
})();

const manifest = createReproducibilityManifest({
  taskId: input.taskId ?? "",
  executionInputDigest: input.executionInputDigest ?? null,
  environmentDigest: input.environmentDigest ?? null,
  taskContractDigest: input.taskContractDigest ?? null,
  taskPlanDigest: input.taskPlanDigest ?? null,
  guidanceDigest: input.guidanceDigest ?? null,
  toolSurfaceDigest: input.toolSurfaceDigest ?? null,
  capabilityDigest: input.capabilityDigest ?? null,
  sourceRevisionSet: (input.sourceRevisionSet ?? []).map((r) => ({
    path: r.path,
    revision: r.revision,
  })),
  validationProfile: input.validationProfile ?? null,
  providerInput: input.providerInput === null || input.providerInput === undefined ? null : {
    providerRoute: input.providerInput.providerRoute ?? null,
    modelIdentity: input.providerInput.modelIdentity ?? null,
    reasoningMode: input.providerInput.reasoningMode ?? null,
    temperature: input.providerInput.temperature ?? null,
    topP: input.providerInput.topP ?? null,
    seed: input.providerInput.seed ?? null,
    parameters: (input.providerInput.parameters ?? []).map((p) => ({
      name: p.name,
      value: p.value,
    })),
  },
  clockPolicy: input.clockPolicy ?? { mode: "system", fixedMs: null },
  rngPolicy: input.rngPolicy ?? { mode: "none", seed: null },
});

process.stdout.write(JSON.stringify({ digest: manifest.digest }));
