/**
 * runtime-readiness.budgets oracle probe (differential harness, ADR
 * 0033, Stage 3R R10c).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Exercises RuntimeBudget and artifact budget admission from the REAL
 * TypeScript reference (packages/core/src/runtime/{budget,artifacts}.ts).
 */
import { readFileSync } from "node:fs";
import {
  DEFAULT_RUNTIME_ARTIFACT_BUDGET,
  enforceArtifactBudget,
} from "../../../packages/core/src/runtime/artifacts.js";
import {
  createRuntimeBudget,
  renderRuntimeBudget,
} from "../../../packages/core/src/runtime/budget.js";

const MAX_INPUT_BYTES = 64 * 1024;

const input = (() => {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
})();

const op = input.op;

if (op === "budget") {
  const overrides = input.overrides ?? {};
  const budget = createRuntimeBudget({
    startupTimeoutMs: overrides.startupTimeoutMs,
    idleTimeoutMs: overrides.idleTimeoutMs,
    hardLifetimeMs: overrides.hardLifetimeMs,
    stdoutBytes: overrides.stdoutBytes,
    stderrBytes: overrides.stderrBytes,
    artifactBytes: overrides.artifactBytes,
    artifactCount: overrides.artifactCount,
    childProcessCount: overrides.childProcessCount,
    memoryMb: overrides.memoryMb === undefined ? undefined : overrides.memoryMb,
    cpuPercent: overrides.cpuPercent === undefined ? undefined : overrides.cpuPercent,
  });
  process.stdout.write(
    JSON.stringify({
      digest: budget.digest,
      rendered: renderRuntimeBudget(budget),
    }),
  );
} else if (op === "admission") {
  const results = (input.cases ?? []).map((entry) => {
    const admission = enforceArtifactBudget({
      budget: entry.budget ?? DEFAULT_RUNTIME_ARTIFACT_BUDGET,
      state: entry.state ?? { artifactCount: 0, aggregateBytes: 0 },
      incomingSize: entry.incomingSize ?? 0,
      incomingCount: entry.incomingCount ?? 1,
    });
    return admission.status === "admit"
      ? { status: "admit", truncated: admission.truncated }
      : { status: "artifact_limit", reason: admission.reason };
  });
  process.stdout.write(JSON.stringify({ cases: results }));
} else {
  throw new Error(`unknown runtime-readiness.budgets op ${JSON.stringify(op)}`);
}
