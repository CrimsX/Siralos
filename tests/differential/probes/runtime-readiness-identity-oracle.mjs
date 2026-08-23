/**
 * runtime-readiness.identity oracle probe (differential harness, ADR
 * 0033, Stage 3R R10c).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Exercises causal run identity from the REAL TypeScript reference
 * (packages/core/src/runtime/identity.ts).
 */
import { readFileSync } from "node:fs";
import {
  createOperationId,
  createRunId,
  createRunTraceRef,
  formatRunTraceRef,
} from "../../../packages/core/src/runtime/identity.js";

const MAX_INPUT_BYTES = 64 * 1024;

const input = (() => {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
})();

const op = input.op;

if (op === "run-id") {
  try {
    const runId = createRunId({
      taskId: input.taskId,
      phaseId: input.phaseId,
      sequence: input.sequence,
      kind: input.kind ?? undefined,
    });
    process.stdout.write(JSON.stringify({ ok: true, runId }));
  } catch (error) {
    process.stdout.write(JSON.stringify({ ok: false, error: String(error.message) }));
  }
} else if (op === "operation-id") {
  const operationId = createOperationId(input.runId, input.operation);
  process.stdout.write(JSON.stringify({ operationId }));
} else if (op === "trace-ref") {
  const trace = createRunTraceRef({
    taskId: input.trace.taskId,
    phaseId: input.trace.phaseId,
    runId: input.trace.runId,
    operationId: input.trace.operationId === undefined ? null : input.trace.operationId,
    producer: input.trace.producer,
  });
  process.stdout.write(
    JSON.stringify({
      ref: {
        taskId: trace.taskId,
        phaseId: trace.phaseId,
        runId: trace.runId,
        operationId: trace.operationId,
        producer: trace.producer,
      },
      formatted: formatRunTraceRef(trace),
    }),
  );
} else {
  throw new Error(`unknown runtime-readiness.identity op ${JSON.stringify(op)}`);
}
