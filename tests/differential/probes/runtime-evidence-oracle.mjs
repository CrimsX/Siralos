/**
 * runtime-evidence oracle probe (differential harness, ADR 0033, Stage 4.1).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Exercises bounded evidence projection from the REAL TypeScript reference
 * (packages/core/src/runtime/evidence.ts). The record carries byte lengths
 * and digests, never the captured buffers, so the output stays inside the
 * harness bound even for the 1 MiB truncation scenarios.
 */
import { readFileSync } from "node:fs";
import {
  createRuntimeEvidence,
  renderRuntimeEvidence,
} from "../../../packages/core/src/runtime/evidence.js";

const MAX_INPUT_BYTES = 64 * 1024;

const input = (() => {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
})();

const op = input.op;

if (op === "create") {
  let evidenceInput = { ...(input.input ?? {}) };
  if (evidenceInput.large === true) {
    evidenceInput = {
      ...evidenceInput,
      stdout: "a".repeat(1024 * 1024 + 10),
      stderr: "b".repeat(1024 * 1024 + 5),
    };
  } else if (evidenceInput.largeWithEmoji === true) {
    const prefix = "a".repeat(1024 * 1024 - 2);
    evidenceInput = {
      ...evidenceInput,
      stdout: prefix + "\u{1F600}",
      stderr: "b".repeat(1024 * 1024 + 5),
    };
  }
  try {
    const evidence = createRuntimeEvidence(evidenceInput);
    process.stdout.write(
      JSON.stringify({
        evidence: {
          runId: evidence.runId,
          operationId: evidence.operationId,
          exitCode: evidence.exitCode,
          durationMs: evidence.durationMs,
          stdoutLength: Buffer.byteLength(evidence.stdout, "utf8"),
          stderrLength: Buffer.byteLength(evidence.stderr, "utf8"),
          truncated: evidence.truncated,
          artifactDigest: evidence.artifactDigest,
          digest: evidence.digest,
        },
        rendered: renderRuntimeEvidence(evidence),
      }),
    );
  } catch (error) {
    process.stdout.write(JSON.stringify({ error: String(error.message ?? error) }));
  }
} else {
  throw new Error(`unknown runtime-evidence op ${JSON.stringify(op)}`);
}
