/**
 * runtime-readiness.lifecycle oracle probe (differential harness, ADR
 * 0033, Stage 3R R10c).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Drives the deterministic fake-process fault scripts through the pure
 * supervisor transitions of the REAL TypeScript reference
 * (packages/core/src/runtime/{faults,supervision}.ts).
 */
import { readFileSync } from "node:fs";
import {
  createFakeProcessDriver,
  expectedFailureKind,
} from "../../../packages/core/src/runtime/faults.js";
import { transitionSupervisor } from "../../../packages/core/src/runtime/supervision.js";

const MAX_INPUT_BYTES = 64 * 1024;

const input = (() => {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
})();

if (input.op !== "drive") {
  throw new Error(`unknown runtime-readiness.lifecycle op ${JSON.stringify(input.op)}`);
}

const script = createFakeProcessDriver(input.script);
let state = {
  state: "prepared",
  startedAtMs: null,
  terminatedAtMs: null,
  terminalDisposition: null,
  failureKind: null,
};
const steps = (input.steps ?? []).map((step) => {
  const observations = script
    .observe(step.atMs, step.requested ?? [])
    .map((observation) => observation);
  if (step.inject !== undefined) {
    observations.push(step.inject);
  }
  for (const observation of observations) {
    state = transitionSupervisor(state, observation, step.atMs);
  }
  return {
    atMs: step.atMs,
    observations,
    state: {
      state: state.state,
      startedAtMs: state.startedAtMs,
      terminatedAtMs: state.terminatedAtMs,
      terminalDisposition: state.terminalDisposition,
      failureKind: state.failureKind,
    },
  };
});

process.stdout.write(
  JSON.stringify({
    steps,
    expectedFailureKind: expectedFailureKind(input.script),
  }),
);
