import { computeArtifactDigest } from "../identity/artifact-digest.js";

export const PROCESS_EXECUTE_CAPABILITY = "process.execute" as const;
export const IDENTITY_BOUND_UNAVAILABLE_REASON =
  "identity-bound launch primitive not available" as const;
export const MAX_COMMAND_BYTES = 8192;
export const MAX_ARGS = 64;
export const MAX_ARG_BYTES = 4096;
export const MAX_RUN_ID_BYTES = 128;
export const MAX_OPERATION_ID_BYTES = 128;

export function isIdentityBoundLaunchPrimitiveAvailable(): boolean {
  return false;
}

export type RuntimeExecutionDisposition =
  "success" | "COMMAND_DENIED" | "STALE" | "RESOURCE_EXCEEDED" | "CANCELLED" | "UNAVAILABLE";

export interface RuntimeExecutionRequest {
  readonly command: string;
  readonly args: readonly string[];
  readonly runId: string;
  readonly operationId?: string | null;
  readonly isStale: boolean;
  readonly requestedBytes: number;
}

export type RuntimeExecutionOutcome =
  | { readonly disposition: "success"; readonly runId: string; readonly operationId: string }
  | { readonly disposition: "COMMAND_DENIED"; readonly reason: string }
  | { readonly disposition: "STALE"; readonly reason: string }
  | { readonly disposition: "RESOURCE_EXCEEDED"; readonly reason: string }
  | { readonly disposition: "CANCELLED"; readonly reason: string }
  | { readonly disposition: "UNAVAILABLE"; readonly reason: string };

function runtimeError(message: string): Error {
  return new Error(message);
}

function validateRequest(request: RuntimeExecutionRequest): void {
  if (request.command.length === 0 || request.command.trim().length === 0) {
    throw runtimeError("A runtime execution requires a command.");
  }
  if (request.command.length > MAX_COMMAND_BYTES) {
    throw runtimeError(`The runtime command exceeds the ${MAX_COMMAND_BYTES}-byte bound.`);
  }
  if (request.command.includes("\0")) {
    throw runtimeError("The runtime command must not contain NUL.");
  }
  if (request.args.length > MAX_ARGS) {
    throw runtimeError(`The runtime args exceed the ${MAX_ARGS} entry bound.`);
  }
  for (const arg of request.args) {
    if (arg.length > MAX_ARG_BYTES) {
      throw runtimeError(`A runtime arg exceeds the ${MAX_ARG_BYTES}-byte bound.`);
    }
    if (arg.includes("\0")) {
      throw runtimeError("A runtime arg must not contain NUL.");
    }
  }
  if (request.runId.length === 0) {
    throw runtimeError("A runtime execution requires a run id.");
  }
  if (request.runId.length > MAX_RUN_ID_BYTES) {
    throw runtimeError(`The runtime run id exceeds the ${MAX_RUN_ID_BYTES}-byte bound.`);
  }
  if (request.operationId !== undefined && request.operationId !== null) {
    if (request.operationId.length === 0) {
      throw runtimeError("A runtime operation id must not be empty.");
    }
    if (request.operationId.length > MAX_OPERATION_ID_BYTES) {
      throw runtimeError(
        `The runtime operation id exceeds the ${MAX_OPERATION_ID_BYTES}-byte bound.`,
      );
    }
  }
}

export function decideRuntimeExecution(
  request: RuntimeExecutionRequest,
  policy: Record<string, string>,
  budget: { readonly artifactBytes: number },
  isCancelled: boolean,
): RuntimeExecutionOutcome {
  validateRequest(request);

  const rule = policy[PROCESS_EXECUTE_CAPABILITY];
  if (rule === "deny" || rule === undefined) {
    return {
      disposition: "COMMAND_DENIED",
      reason: "COMMAND_DENIED: Policy denies process.execute.",
    };
  }
  if (rule === "ask") {
    return {
      disposition: "COMMAND_DENIED",
      reason: "COMMAND_DENIED: Policy requires approval for process.execute.",
    };
  }

  if (request.isStale) {
    return { disposition: "STALE", reason: "STALE: revision is stale." };
  }

  if (request.requestedBytes > budget.artifactBytes) {
    return {
      disposition: "RESOURCE_EXCEEDED",
      reason: `RESOURCE_EXCEEDED: requested ${request.requestedBytes} exceeds budget ${budget.artifactBytes}.`,
    };
  }

  if (isCancelled) {
    return { disposition: "CANCELLED", reason: "CANCELLED: execution was cancelled." };
  }

  if (!isIdentityBoundLaunchPrimitiveAvailable()) {
    return {
      disposition: "UNAVAILABLE",
      reason: `UNAVAILABLE: ${IDENTITY_BOUND_UNAVAILABLE_REASON}`,
    };
  }

  const operationId = request.operationId ?? `op_${request.runId}_${request.command}`;
  return { disposition: "success", runId: request.runId, operationId };
}

export function digestRuntimeExecutionOutcome(outcome: RuntimeExecutionOutcome): string {
  const map: Record<string, unknown> = {
    disposition: outcome.disposition,
    reason: (outcome as { reason?: string }).reason ?? null,
  };
  if (outcome.disposition === "success") {
    map["runId"] = (outcome as { runId: string }).runId;
    map["operationId"] = (outcome as { operationId: string }).operationId;
  }
  return computeArtifactDigest({
    artifactType: "RuntimeExecutionOutcome",
    schemaVersion: 1,
    payload: map,
  }).value;
}
