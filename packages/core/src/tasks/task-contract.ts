import { canonicalizeJson, sha256Hex } from "../godot/digest.js";
import { deepFreeze } from "../domain/deep-freeze.js";

/**
 * Provider-neutral structured task contract (Stage 3 milestone 1).
 *
 * A task contract distinguishes what the user requested, the constraints
 * the runtime must respect, the explicit acceptance criteria completion is
 * evaluated against, and the conditions that require pause/escalation. It
 * is never an arbitrary string blob and never a universal task-description
 * language: criteria are authored by the host runtime/workflow, not
 * extracted from prose by the model.
 *
 * Contracts are immutable and revisioned: a material change produces a new
 * revision; revision N is never mutated in place. Later milestones bind
 * plan/mutation approvals and workflow continuation to contract revisions.
 */

export type TaskContractId = string;

export type AcceptanceCriterionId = string;

export type TaskConstraintId = string;

/** How a criterion's satisfaction is established. */
export type VerificationKind = "deterministic" | "review" | "user";

export interface AcceptanceCriterion {
  readonly id: AcceptanceCriterionId;
  readonly description: string;
  readonly verificationKind: VerificationKind;
}

export type TaskConstraintKind = "scope" | "process" | "security" | "escalation";

export interface TaskConstraint {
  readonly id: TaskConstraintId;
  readonly description: string;
  readonly kind: TaskConstraintKind;
}

/**
 * Conditions that require the runtime to pause the task and surface it for
 * user attention. Host pause points (such as approval gates) are surfaced
 * as a blocked phase with a reason regardless of the policy; the policy
 * describes the *contractual* pause conditions for future orchestration.
 */
export type PausePolicy = "none" | "on_approval" | "on_escalation";

export interface TaskContract {
  readonly id: TaskContractId;
  /** Immutable revision identity; starts at 1 and only ever increases. */
  readonly revision: number;
  readonly request: string;
  readonly context?: string;
  readonly constraints: readonly TaskConstraint[];
  readonly acceptanceCriteria: readonly AcceptanceCriterion[];
  readonly pausePolicy: PausePolicy;
}

export interface CreateTaskContractInput {
  readonly id: TaskContractId;
  readonly request: string;
  readonly context?: string;
  readonly constraints?: readonly TaskConstraint[];
  readonly acceptanceCriteria: readonly AcceptanceCriterion[];
  readonly pausePolicy?: PausePolicy;
}

/** Host-owned hard bounds for revisioned task contracts. */
export const TASK_CONTRACT_LIMITS = Object.freeze({
  maxIdBytes: 95,
  maxRequestBytes: 16 * 1024,
  maxContextBytes: 32 * 1024,
  maxConstraints: 32,
  maxAcceptanceCriteria: 64,
  maxEntryIdBytes: 64,
  maxEntryDescriptionBytes: 4096,
});

const TASK_ID_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._-]{0,94}$/;
const ENTRY_ID_PATTERN = /^[A-Za-z][A-Za-z0-9._-]{0,63}$/;
const textEncoder = new TextEncoder();

interface ContractShape {
  readonly id: TaskContractId;
  readonly request: string;
  readonly context?: string;
  readonly constraints: readonly TaskConstraint[];
  readonly acceptanceCriteria: readonly AcceptanceCriterion[];
  readonly pausePolicy: PausePolicy;
  readonly revision: number;
}

function validateContractShape(input: ContractShape): TaskContract {
  if (!TASK_ID_PATTERN.test(input.id)) {
    throw new Error(`Invalid task contract id: ${input.id}`);
  }
  const request = input.request.trim();
  if (request.length === 0) {
    throw new Error("A task contract requires a non-empty request.");
  }
  if (textEncoder.encode(request).length > TASK_CONTRACT_LIMITS.maxRequestBytes) {
    throw new Error(
      `A task contract request exceeds ${TASK_CONTRACT_LIMITS.maxRequestBytes} UTF-8 bytes.`,
    );
  }
  if (!Number.isSafeInteger(input.revision) || input.revision < 1) {
    throw new Error("A task contract revision must be at least 1.");
  }
  if (input.acceptanceCriteria.length > TASK_CONTRACT_LIMITS.maxAcceptanceCriteria) {
    throw new Error(
      `A task contract accepts at most ${TASK_CONTRACT_LIMITS.maxAcceptanceCriteria} acceptance criteria.`,
    );
  }
  if (input.constraints.length > TASK_CONTRACT_LIMITS.maxConstraints) {
    throw new Error(
      `A task contract accepts at most ${TASK_CONTRACT_LIMITS.maxConstraints} constraints.`,
    );
  }
  const criterionIds = new Set<string>();
  for (const criterion of input.acceptanceCriteria) {
    if (!ENTRY_ID_PATTERN.test(criterion.id)) {
      throw new Error(`Invalid acceptance criterion id: ${criterion.id}`);
    }
    if (criterionIds.has(criterion.id)) {
      throw new Error(`Duplicate acceptance criterion id: ${criterion.id}`);
    }
    criterionIds.add(criterion.id);
    validateEntryDescription("acceptance criterion", criterion.id, criterion.description);
    if (!(["deterministic", "review", "user"] as const).includes(criterion.verificationKind)) {
      throw new Error(
        `Acceptance criterion ${criterion.id} has invalid verification kind ${String(criterion.verificationKind)}.`,
      );
    }
  }
  if (input.acceptanceCriteria.length === 0) {
    throw new Error("A task contract requires at least one acceptance criterion.");
  }
  const constraintIds = new Set<string>();
  for (const constraint of input.constraints) {
    if (!ENTRY_ID_PATTERN.test(constraint.id)) {
      throw new Error(`Invalid task constraint id: ${constraint.id}`);
    }
    if (constraintIds.has(constraint.id)) {
      throw new Error(`Duplicate task constraint id: ${constraint.id}`);
    }
    constraintIds.add(constraint.id);
    validateEntryDescription("task constraint", constraint.id, constraint.description);
    if (!(["scope", "process", "security", "escalation"] as const).includes(constraint.kind)) {
      throw new Error(
        `Task constraint ${constraint.id} has invalid kind ${String(constraint.kind)}.`,
      );
    }
  }
  if (!(["none", "on_approval", "on_escalation"] as const).includes(input.pausePolicy)) {
    throw new Error(`Invalid task pause policy: ${String(input.pausePolicy)}`);
  }
  const context =
    input.context === undefined || input.context.trim().length === 0
      ? undefined
      : input.context.trim();
  if (
    context !== undefined &&
    textEncoder.encode(context).length > TASK_CONTRACT_LIMITS.maxContextBytes
  ) {
    throw new Error(
      `A task contract context exceeds ${TASK_CONTRACT_LIMITS.maxContextBytes} UTF-8 bytes.`,
    );
  }
  return deepFreeze({
    id: input.id,
    revision: input.revision,
    request,
    ...(context === undefined ? {} : { context }),
    constraints: input.constraints.map((constraint) => ({
      ...constraint,
      description: constraint.description.trim(),
    })),
    acceptanceCriteria: input.acceptanceCriteria.map((criterion) => ({
      ...criterion,
      description: criterion.description.trim(),
    })),
    pausePolicy: input.pausePolicy,
  });
}

/**
 * Validate and detach an existing contract at a runtime boundary.
 *
 * TypeScript's readonly types do not protect the runtime from a cast,
 * deserialized object, or caller mutation. TaskRuntime uses this helper
 * before accepting a contract so only the same normalized shape produced
 * by `createTaskContract` can become authoritative state.
 */
export function validateTaskContract(input: TaskContract): TaskContract {
  return validateContractShape({
    id: input.id,
    revision: input.revision,
    request: input.request,
    ...(input.context === undefined ? {} : { context: input.context }),
    constraints: input.constraints,
    acceptanceCriteria: input.acceptanceCriteria,
    pausePolicy: input.pausePolicy,
  });
}

function validateEntryDescription(kind: string, id: string, description: string): void {
  const trimmed = description.trim();
  if (trimmed.length === 0) {
    throw new Error(`${kind} ${id} requires a non-empty description.`);
  }
  if (textEncoder.encode(trimmed).length > TASK_CONTRACT_LIMITS.maxEntryDescriptionBytes) {
    throw new Error(
      `${kind} ${id} description exceeds ${TASK_CONTRACT_LIMITS.maxEntryDescriptionBytes} UTF-8 bytes.`,
    );
  }
}

export function createTaskContract(input: CreateTaskContractInput): TaskContract {
  return validateContractShape({
    id: input.id,
    request: input.request,
    ...(input.context === undefined ? {} : { context: input.context }),
    constraints: input.constraints ?? [],
    acceptanceCriteria: input.acceptanceCriteria,
    pausePolicy: input.pausePolicy ?? "none",
    revision: 1,
  });
}

export interface ReviseTaskContractInput {
  readonly id: TaskContractId;
  readonly request?: string;
  readonly context?: string;
  readonly constraints?: readonly TaskConstraint[];
  readonly acceptanceCriteria?: readonly AcceptanceCriterion[];
  readonly pausePolicy?: PausePolicy;
}

/**
 * Produce the next immutable revision of a contract. The previous revision
 * object is untouched; the returned contract carries `revision + 1` and the
 * same contract id. Omitted fields carry the previous revision's value over.
 */
export function reviseTaskContract(
  previous: TaskContract,
  changes: ReviseTaskContractInput,
): TaskContract {
  if (changes.id !== previous.id) {
    throw new Error(
      `A task contract revision must preserve id ${previous.id}; received ${changes.id}.`,
    );
  }
  return validateContractShape({
    id: changes.id,
    request: changes.request ?? previous.request,
    ...(changes.context === undefined
      ? previous.context === undefined
        ? {}
        : { context: previous.context }
      : { context: changes.context }),
    constraints: changes.constraints ?? previous.constraints,
    acceptanceCriteria: changes.acceptanceCriteria ?? previous.acceptanceCriteria,
    pausePolicy: changes.pausePolicy ?? previous.pausePolicy,
    revision: previous.revision + 1,
  });
}

/**
 * Contract for a generic ad-hoc task (CLI `/task`). Completion requires
 * host verification of the single explicit criterion; without an
 * integrated workflow the host cannot verify it, so the task honestly
 * stays non-complete rather than letting a model claim success.
 */
export function createAdHocTaskContract(id: TaskContractId, request: string): TaskContract {
  return createTaskContract({
    id,
    request,
    constraints: [
      {
        id: "workspace-scope",
        kind: "scope",
        description: "Work is contained within the workspace root.",
      },
    ],
    acceptanceCriteria: [
      {
        id: "host-verified",
        description: "The requested work is complete and verified by the host.",
        verificationKind: "user",
      },
    ],
    pausePolicy: "none",
  });
}

/** Deterministic digest over a contract revision (canonical JSON). */
export function computeTaskContractDigest(contract: TaskContract): string {
  return sha256Hex(canonicalizeJson(contract));
}
