import { createHash } from "node:crypto";

export function hashBuffer(buffer: Buffer): string {
  return createHash("sha256").update(buffer).digest("hex");
}

export interface MutationPlanParts {
  readonly relativePath: string;
  readonly operation: "create" | "update" | "delete";
  readonly beforeSha256: string | null;
  readonly afterSha256: string | null;
}

/**
 * SHA-256 over the immutable prepared mutation plan. The digest binds
 * approval to the exact plan: the apply path verifies the plan it is asked
 * to execute against the approved digest and fails closed on mismatch.
 */
export function hashMutationPlan(plan: MutationPlanParts): string {
  const canonical = JSON.stringify([
    plan.relativePath,
    plan.operation,
    plan.beforeSha256,
    plan.afterSha256,
  ]);
  return createHash("sha256").update(canonical).digest("hex");
}
