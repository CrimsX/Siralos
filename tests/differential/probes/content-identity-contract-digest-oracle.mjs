/**
 * content-identity-contract-digest oracle probe (differential harness,
 * ADR 0033, Stage 3R R10a).
 *
 * Computes a domain-separated content digest over a contract/plan
 * payload using the canonical JSON + SHA-256 primitives from the
 * TypeScript reference.
 */
import { readFileSync } from "node:fs";
import { canonicalizeJson } from "../../../packages/core/src/godot/digest.js";
import { sha256Hex } from "../../../packages/core/src/godot/digest.js";

const MAX_INPUT_BYTES = 64 * 1024;

const input = (() => {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
})();

const canonical = `siralos:${input.artifactType}:v${input.schemaVersion}\0${canonicalizeJson(input.content)}`;
const digest = sha256Hex(canonical);

process.stdout.write(JSON.stringify({ digest }));
