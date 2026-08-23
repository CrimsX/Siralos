/**
 * recovery-taxonomy oracle probe (differential harness, ADR 0033,
 * Stage 3R R11.2).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Drives the typed failure surfaces of the REAL TypeScript reference —
 * the host-owned retry policy (determinism/decisions.ts), the typed
 * domain failures with stable codes (domain/failure.ts), and the
 * conservative restart reconciliation (runtime/budget.ts) — so every
 * recovery-relevant dimension has a stable, machine-branchable code
 * proven identical across implementations.
 */
import { readFileSync } from "node:fs";
import { classifyRetry } from "../../../packages/core/src/determinism/decisions.js";
import { failureCode } from "../../../packages/core/src/domain/failure.js";
import { classifyIncompleteRun } from "../../../packages/core/src/runtime/budget.js";

const MAX_INPUT_BYTES = 64 * 1024;

const input = (() => {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
})();

const op = input.op;

if (op === "retry-classification") {
  const cases = (input.cases ?? []).map((entry) => {
    const result = classifyRetry(entry.category, entry.attemptsUsed ?? 0);
    return {
      category: entry.category,
      attemptsUsed: entry.attemptsUsed ?? 0,
      decision: result.decision,
      reason: result.reason,
      nextBackoffMs: result.nextBackoffMs,
    };
  });
  process.stdout.write(JSON.stringify({ cases }));
} else if (op === "domain-failure") {
  // Typed literals matching the DomainFailure union, run through the
  // reference's stable-code accessor.
  let failure;
  switch (input.kind) {
    case "CAPABILITY_DENIED":
      failure = { code: "CAPABILITY_DENIED", missing: [...(input.missing ?? [])] };
      break;
    case "STALE_ACTIVATION":
      failure = { code: "STALE_ACTIVATION" };
      break;
    case "RESOURCE_EXCEEDED":
      failure = { code: "RESOURCE_EXCEEDED", kind: input.resourceKind ?? "FUEL" };
      break;
    case "UNAVAILABLE":
      failure = { code: "UNAVAILABLE", reason: input.reason ?? "" };
      break;
    case "CANCELLED":
      failure = { code: "CANCELLED" };
      break;
    default:
      throw new Error(`unknown domain-failure kind ${JSON.stringify(input.kind)}`);
  }
  const record = { code: failureCode(failure) };
  if (input.kind === "CAPABILITY_DENIED") {
    record.missing = [...failure.missing];
  }
  if (input.kind === "RESOURCE_EXCEEDED") {
    record.resourceKind = failure.kind;
  }
  if (input.kind === "UNAVAILABLE") {
    record.reason = failure.reason;
  }
  process.stdout.write(JSON.stringify(record));
} else if (op === "incomplete-run") {
  const cases = (input.cases ?? []).map((entry) => {
    const result = classifyIncompleteRun(
      {
        runId: entry.runId ?? "run_runtime_probe",
        lastKnownState: entry.lastKnownState,
        lastObservedAtMs: entry.lastObservedAtMs ?? 0,
      },
      entry.runStateMayExist === true,
    );
    return {
      lastKnownState: entry.lastKnownState,
      runStateMayExist: entry.runStateMayExist === true,
      classification: result.classification,
      reason: result.reason,
    };
  });
  process.stdout.write(JSON.stringify({ cases }));
} else {
  throw new Error(`unknown recovery-taxonomy op ${JSON.stringify(op)}`);
}
