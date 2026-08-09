import { canonicalizeJson, sha256Hex } from "../godot/digest.js";

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
  const request = input.request.trim();
  if (request.length === 0) {
    throw new Error("A task contract requires a non-empty request.");
  }
  if (input.revision < 1) {
    throw new Error("A task contract revision must be at least 1.");
  }
  const ids = new Set<string>();
  for (const criterion of input.acceptanceCriteria) {
    if (ids.has(criterion.id)) {
      throw new Error(`Duplicate acceptance criterion id: ${criterion.id}`);
    }
    ids.add(criterion.id);
  }
  if (input.acceptanceCriteria.length === 0) {
    throw new Error("A task contract requires at least one acceptance criterion.");
  }
  const context =
    input.context === undefined || input.context.trim().length === 0
      ? undefined
      : input.context.trim();
  return {
    id: input.id,
    revision: input.revision,
    request,
    ...(context === undefined ? {} : { context }),
    constraints: input.constraints.map((constraint) => ({ ...constraint })),
    acceptanceCriteria: input.acceptanceCriteria.map((criterion) => ({ ...criterion })),
    pausePolicy: input.pausePolicy,
  };
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
