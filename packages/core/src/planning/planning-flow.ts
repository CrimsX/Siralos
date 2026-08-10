import type { TaskContract } from "../tasks/task-contract.js";
import type { TaskHandle } from "../tasks/task-runtime.js";
import type { TaskPlan, TaskPlanContent } from "./planning-model.js";
import {
  hasMeaningfulAcceptanceCriteria,
  createTaskPlan,
  reviseTaskPlan,
} from "./planning-model.js";
import type { PlanningDecision, PlanningDecisionInput, PlanningPolicy } from "./planning-policy.js";
import { createPlanningPolicy } from "./planning-policy.js";
import { validatePlanCandidate } from "./planning-validation.js";

/**
 * Host-owned planning flow (Stage 3 milestone 7, ADR 0020).
 *
 * The flow is the application-owned control phase between the TaskContract
 * and execution. It routes planning depth through the deterministic
 * PlanningPolicy, invokes the read-only planner (through the injected
 * `PlannerPort` — never directly), validates the candidate, binds the
 * immutable `TaskPlan` to the current TaskContract revision, and records
 * plan approval against the exact plan revision. It never grants
 * capability, never prepares or applies mutations, and never touches the
 * workspace. TaskState remains the authority for execution progress.
 */

export interface PlannerRequest {
  readonly request: string;
  readonly contract: TaskContract;
  readonly depth: "light" | "full";
}

export type PlannerOutcome =
  | { readonly status: "ready"; readonly content: TaskPlanContent }
  | { readonly status: "failed"; readonly message: string }
  | { readonly status: "cancelled" }
  | { readonly status: "timed_out"; readonly message: string };

/** Provider-driven read-only planner boundary (implemented by adapters). */
export interface PlannerPort {
  plan(input: PlannerRequest, signal?: AbortSignal): Promise<PlannerOutcome>;
}

export type PlanFlowResult =
  | {
      readonly status: "routed";
      readonly decision: PlanningDecision;
    }
  | {
      readonly status: "planned";
      readonly decision: PlanningDecision;
      readonly plan: TaskPlan;
    }
  | {
      readonly status: "failed";
      readonly decision: PlanningDecision;
      readonly message: string;
    }
  | {
      readonly status: "cancelled";
      readonly decision: PlanningDecision;
    }
  | {
      readonly status: "timed_out";
      readonly decision: PlanningDecision;
      readonly message: string;
    };

export interface PlanningFlowOptions {
  readonly handle: TaskHandle;
  readonly planner: PlannerPort;
  readonly policy?: PlanningPolicy;
  readonly now?: () => number;
}

export interface PlanningFlow {
  /** Route planning depth (host-owned) and record the routing activity. */
  route(input: PlanningDecisionInput): PlanningDecision;
  /** The most recent routing decision; null before the first route. */
  decision(): PlanningDecision | null;
  /**
   * Run the planner for the routed depth (no planner call for `none`),
   * validate the candidate, and store the immutable plan revision in the
   * task. Returns the stored plan or a typed failure.
   */
  run(signal?: AbortSignal): Promise<PlanFlowResult>;
  /** Host approval after user review; binds to the exact plan revision. */
  approve(): { readonly status: "ok" } | { readonly status: "rejected"; readonly reason: string };
  /**
   * Full-plan mutation-execution gate (Part I §25): full plans require
   * meaningful acceptance criteria in the TaskContract before any source
   * mutation. Returns a blocking reason or null when execution may proceed.
   */
  mutationExecutionBlocked(): string | null;
}

export function createPlanningFlow(options: PlanningFlowOptions): PlanningFlow {
  const handle = options.handle;
  const policy = options.policy ?? createPlanningPolicy();
  const now = options.now ?? Date.now;
  let routed: PlanningDecision | null = null;

  return {
    route(input: PlanningDecisionInput): PlanningDecision {
      const decision = policy.decide(input);
      handle.routePlanning(decision.depth, decision.reason);
      routed = decision;
      return decision;
    },

    decision(): PlanningDecision | null {
      return routed;
    },

    async run(signal?: AbortSignal): Promise<PlanFlowResult> {
      if (routed === null) {
        return {
          status: "failed",
          decision: { depth: "none", reason: "inspection-or-no-mutation" },
          message: "Planning was not routed before run; the host must route planning first.",
        };
      }
      if (routed.depth === "none") {
        return { status: "routed", decision: routed };
      }
      const contract = handle.contract();
      const outcome = await options.planner.plan(
        { request: contract.request, contract, depth: routed.depth },
        signal,
      );
      if (outcome.status === "cancelled") {
        return { status: "cancelled", decision: routed };
      }
      if (outcome.status === "timed_out") {
        return { status: "timed_out", decision: routed, message: outcome.message };
      }
      if (outcome.status === "failed") {
        handle.rejectPlan(outcome.message);
        return { status: "failed", decision: routed, message: outcome.message };
      }
      // Host-side revalidation binds the candidate to the CURRENT contract
      // revision and the host-routed depth (defense in depth: the planner
      // already validated, but the flow re-checks before identity binding).
      const validated = validatePlanCandidate(outcome.content, {
        contract,
        depth: routed.depth,
      });
      if (!validated.ok) {
        const message = `The planner returned an invalid plan: ${validated.reasons.join(" ")}`;
        handle.rejectPlan(message);
        return { status: "failed", decision: routed, message };
      }
      const previous = handle.currentPlan();
      const planId = `plan-${handle.taskId}`;
      const plan: TaskPlan =
        previous !== null && previous.id === planId
          ? reviseTaskPlan(previous, { content: validated.content })
          : createTaskPlan({
              id: planId,
              taskId: handle.taskId,
              taskContractRevision: contract.revision,
              depth: routed.depth,
              content: validated.content,
              createdAt: now(),
            });
      const stored = handle.setPlan(plan);
      if (stored.status === "rejected") {
        handle.rejectPlan(stored.reason);
        return { status: "failed", decision: routed, message: stored.reason };
      }
      return { status: "planned", decision: routed, plan };
    },

    approve():
      { readonly status: "ok" } | { readonly status: "rejected"; readonly reason: string } {
      const current = handle.currentPlan();
      if (current === null) {
        return { status: "rejected", reason: "No current plan exists to approve." };
      }
      return handle.approvePlan(current.id, current.revision);
    },

    mutationExecutionBlocked(): string | null {
      const contract = handle.contract();
      if (routed?.depth === "full" && !hasMeaningfulAcceptanceCriteria(contract)) {
        return "Full-plan execution requires explicit acceptance criteria in the TaskContract (at least two criteria, one host-verifiable); the contract does not meet that bar, so mutation execution is blocked.";
      }
      const state = handle.snapshot().plan;
      if (state.state === "stale") {
        return `The current plan is stale (${state.staleReason ?? "plan invalidated"}); revalidate or replan before mutation.`;
      }
      return null;
    },
  };
}
