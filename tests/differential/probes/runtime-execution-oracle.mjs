/**
 * runtime-execution oracle probe (differential harness, ADR 0033, Stage 4.1).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Exercises the host execution decision table from the REAL TypeScript
 * reference (packages/core/src/runtime/execution.ts).
 */
import { readFileSync } from "node:fs";
import {
  decideRuntimeExecution,
  isIdentityBoundLaunchPrimitiveAvailable,
  IDENTITY_BOUND_UNAVAILABLE_REASON,
} from "../../../packages/core/src/runtime/execution.js";

const MAX_INPUT_BYTES = 64 * 1024;

const input = (() => {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
})();

const op = input.op;

if (op === "decide") {
  const request = input.request;
  const policy = input.policy ?? { "process.execute": "allow" };
  const budget = input.budget ?? { artifactBytes: 64 * 1024 * 1024 };
  const isCancelled = input.isCancelled ?? false;
  try {
    const outcome = decideRuntimeExecution(request, policy, budget, isCancelled);
    const result = {
      disposition: outcome.disposition,
      reason: outcome.reason ?? null,
    };
    if (outcome.disposition === "success") {
      result.runId = outcome.runId;
      result.operationId = outcome.operationId;
    }
    process.stdout.write(
      JSON.stringify({
        outcome: result,
        available: isIdentityBoundLaunchPrimitiveAvailable(),
        reason: IDENTITY_BOUND_UNAVAILABLE_REASON,
      }),
    );
  } catch (error) {
    process.stdout.write(
      JSON.stringify({
        error: String(error.message ?? error),
        available: isIdentityBoundLaunchPrimitiveAvailable(),
      }),
    );
  }
} else {
  throw new Error(`unknown runtime-execution op ${JSON.stringify(op)}`);
}
