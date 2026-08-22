/**
 * content-identity-manifests oracle probe (differential harness, ADR
 * 0033, Stage 3R R10a).
 *
 * Creates a guidance manifest from declared entries using the REAL
 * TypeScript reference (packages/core/src/identity/manifests.ts).
 */
import { readFileSync } from "node:fs";
import { createGuidanceManifest } from "../../../packages/core/src/identity/manifests.js";

const MAX_INPUT_BYTES = 64 * 1024;

const input = (() => {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
})();

const manifest = createGuidanceManifest(
  (input.entries ?? []).map((entry) => ({
    id: entry.id,
    kind: entry.kind,
    path: entry.path,
    digest: entry.digest,
  })),
);

process.stdout.write(
  JSON.stringify({
    entries: manifest.entries.map((entry) => ({
      id: entry.id,
      kind: entry.kind,
      path: entry.path,
      digest: entry.digest,
    })),
    aggregateDigest: manifest.aggregateDigest,
  }),
);
