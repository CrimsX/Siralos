/**
 * content-identity-contract-digest oracle probe (differential harness,
 * ADR 0033, Stage 3R R10a; seam corrected at R11).
 *
 * Computes contract/plan content digests through the TYPED identity
 * seam of the TypeScript reference
 * (identity/contract-plan-identity.ts): the artifact type and schema
 * version are pinned by computeTaskContractArtifactDigest /
 * computeTaskPlanArtifactDigest, never caller-supplied strings.
 */
import { readFileSync } from "node:fs";
import {
  computeTaskContractArtifactDigest,
  computeTaskPlanArtifactDigest,
} from "../../../packages/core/src/identity/contract-plan-identity.js";

const MAX_INPUT_BYTES = 64 * 1024;

const input = (() => {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
})();

const op = input.op;

if (op === "contract") {
  const contract = input.contract;
  const digest = computeTaskContractArtifactDigest({
    id: contract.id,
    request: contract.request,
    ...(contract.context === undefined ? {} : { context: contract.context }),
    constraints: contract.constraints ?? [],
    acceptanceCriteria: contract.acceptanceCriteria ?? [],
    pausePolicy: contract.pausePolicy ?? "auto",
  }).value;
  process.stdout.write(JSON.stringify({ digest }));
} else if (op === "plan") {
  const plan = input.plan;
  const digest = computeTaskPlanArtifactDigest({
    objective: plan.objective,
    scope: plan.scope ?? [],
    nonGoals: plan.nonGoals ?? [],
    touchpoints: plan.touchpoints ?? [],
    constraints: plan.constraints ?? [],
    risks: plan.risks ?? [],
    steps: plan.steps ?? [],
    validation: plan.validation ?? [],
    ...(plan.rollback === undefined ? {} : { rollback: plan.rollback }),
    ...(plan.rationale === undefined ? {} : { rationale: plan.rationale }),
  }).value;
  process.stdout.write(JSON.stringify({ digest }));
} else {
  throw new Error(`unknown content-identity-contract-digest op ${JSON.stringify(op)}`);
}
