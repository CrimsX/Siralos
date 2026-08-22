/**
 * content-identity-delta oracle probe (differential harness, ADR 0033,
 * Stage 3R R10a).
 *
 * Computes a section-level delta using the REAL TypeScript reference
 * (packages/core/src/identity/semantic-delta.ts).
 */
import { readFileSync } from "node:fs";
import { computeSectionDelta } from "../../../packages/core/src/identity/semantic-delta.js";

const MAX_INPUT_BYTES = 64 * 1024;

const input = (() => {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
})();

const { changed, unchanged } = computeSectionDelta(
  input.base ?? {},
  input.result ?? {},
  input.keys ?? [],
);

process.stdout.write(JSON.stringify({ changed, unchanged }));
