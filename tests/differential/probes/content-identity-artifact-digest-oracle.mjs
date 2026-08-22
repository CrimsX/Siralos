/**
 * content-identity-artifact-digest oracle probe (differential harness,
 * ADR 0033, Stage 3R R10a).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Computes a domain-separated artifact digest against the REAL
 * TypeScript reference (packages/core/src/identity/artifact-digest.ts).
 */
import { readFileSync } from "node:fs";
import { computeArtifactDigestHex } from "../../../packages/core/src/identity/artifact-digest.js";

const MAX_INPUT_BYTES = 64 * 1024;

const input = (() => {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
})();

const digest = computeArtifactDigestHex({
  artifactType: input.artifactType,
  schemaVersion: input.schemaVersion,
  payload: input.payload,
});

process.stdout.write(JSON.stringify({ digest }));
