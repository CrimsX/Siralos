import { deepFreeze } from "../domain/deep-freeze.js";
import type { TaskContract, AcceptanceCriterionId } from "../tasks/task-contract.js";
import type { FindingRef } from "../tasks/task-model.js";
import type { TaskPlan, TouchpointConfidence } from "../planning/planning-model.js";
import type { ResolvedInstructionSet } from "../instructions/instruction-model.js";
import type { CapabilitySnapshot } from "../doctor/doctor-model.js";
import type { ExecutionContractRef } from "./execution-contract.js";
import type { MilestoneManifest, MilestoneManifestRef } from "./milestone-manifest.js";
import type { ArchitectureContextRef } from "./architecture-context.js";
import { selectArchitectureContext } from "./architecture-context.js";

/**
 * ExecutorContextPack (executor briefing foundation).
 *
 * The pack gathers ONLY the context relevant to one executor invocation,
 * derived from existing authoritative Solaris subsystems: TaskContract,
 * TaskPlan, path-scoped instructions, architecture docs/ADRs (via the
 * deterministic concern index), CapabilitySnapshot, and current audit
 * findings. It is DERIVED context — never another source of truth — and
 * it never embeds raw giant file contents: instructions and architecture
 * material are bounded references/renders, and the pack itself is a
 * disposable input to the ExecutorBriefCompiler.
 *
 * Verified/candidate touchpoints come straight from the plan and remain
 * distinct: a guess is never promoted to verified.
 */

export interface TaskContractRef {
  readonly id: string;
  readonly revision: number;
}

export interface TaskPlanRef {
  readonly id: string;
  readonly revision: number;
  readonly approval: "none" | "approved" | "invalidated";
}

export interface InstructionRef {
  readonly source: string;
  /** Bounded rendered instruction text for the task's focus paths. */
  readonly summary: string;
}

export interface TouchpointRef {
  readonly id: string;
  readonly path: string;
  readonly confidence: TouchpointConfidence;
}

export interface CapabilityRef {
  /** True when a capability snapshot was available at pack build time. */
  readonly available: boolean;
  /** Per-area state summary; empty when no snapshot was available. */
  readonly states: readonly { readonly area: string; readonly state: string }[];
}

export interface AcceptanceRequirementRef {
  readonly id: string;
  readonly description: string;
  readonly criterionId?: AcceptanceCriterionId;
}

export interface ExecutorContextPack {
  readonly task: TaskContractRef;
  readonly plan?: TaskPlanRef;
  readonly executionContract: ExecutionContractRef;
  readonly milestone?: MilestoneManifestRef;
  readonly instructions: readonly InstructionRef[];
  readonly architecture: readonly ArchitectureContextRef[];
  readonly verifiedTouchpoints: readonly TouchpointRef[];
  readonly candidateTouchpoints: readonly TouchpointRef[];
  readonly capabilities: CapabilityRef;
  readonly unresolvedFindings: readonly FindingRef[];
  readonly acceptance: readonly AcceptanceRequirementRef[];
}

/** Host-owned bounds for the context pack. */
export const EXECUTOR_CONTEXT_PACK_LIMITS = Object.freeze({
  maxInstructions: 8,
  maxInstructionSummaryBytes: 1024,
  maxArchitectureEntries: 4,
  maxFindings: 16,
  maxAcceptance: 32,
});

export interface BuildExecutorContextPackInput {
  readonly contract: TaskContract;
  readonly plan?: TaskPlan | null;
  readonly executionContract: ExecutionContractRef;
  readonly milestone?: MilestoneManifest | null;
  /** Resolved path-scoped instructions for the task focus paths. */
  readonly instructions?: ResolvedInstructionSet | null;
  /** Deterministic architecture-context selection (defaults to the manifest concerns). */
  readonly architectureConcerns?: readonly string[];
  readonly architectureIndex?: readonly import("./architecture-context.js").ArchitectureContextEntry[];
  readonly capabilitySnapshot?: CapabilitySnapshot | null;
  readonly findings?: readonly FindingRef[];
  /** Approval state of the current plan revision (from TaskState). */
  readonly planApproval?: "none" | "approved" | "invalidated";
}

const textEncoder = new TextEncoder();

function instructionRefs(
  resolved: ResolvedInstructionSet | null,
  max: number,
): readonly InstructionRef[] {
  if (resolved === null) {
    return [];
  }
  const refs: InstructionRef[] = [];
  for (const instruction of resolved.instructions) {
    if (refs.length >= max) {
      break;
    }
    const summary = instruction.content.trim();
    refs.push({
      source: `${instruction.source.path ?? instruction.source.kind}:${instruction.scope.path ?? "."}`,
      summary:
        textEncoder.encode(summary).length > EXECUTOR_CONTEXT_PACK_LIMITS.maxInstructionSummaryBytes
          ? `${summary.slice(0, 512)}\u2026`
          : summary,
    });
  }
  return refs;
}

function touchpointRefs(
  plan: TaskPlan | null,
  confidence: TouchpointConfidence,
): readonly TouchpointRef[] {
  if (plan === null) {
    return [];
  }
  return plan.touchpoints
    .filter((touchpoint) => touchpoint.confidence === confidence)
    .map((touchpoint) => ({
      id: touchpoint.id,
      path: touchpoint.path,
      confidence: touchpoint.confidence,
    }));
}

function capabilityRef(snapshot: CapabilitySnapshot | null): CapabilityRef {
  if (snapshot === null) {
    return { available: false, states: [] };
  }
  return {
    available: true,
    states: [
      { area: "providers", state: snapshot.providers[0]?.state ?? "unknown" },
      { area: "sandbox", state: snapshot.sandbox.state },
      { area: "workspace", state: snapshot.workspace.state },
      { area: "godot", state: snapshot.godot.state },
      { area: "references", state: snapshot.references.state },
      { area: "research", state: snapshot.research.state },
      { area: "tools", state: snapshot.tools.state },
    ],
  };
}

/**
 * Build the derived context pack for one task. Deterministic: identical
 * inputs produce identical packs. The pack never includes raw source or
 * file contents, never includes provider-private reasoning, and never
 * grants capability.
 */
export function buildExecutorContextPack(
  input: BuildExecutorContextPackInput,
): ExecutorContextPack {
  const milestone = input.milestone ?? null;
  const concerns = input.architectureConcerns ?? milestone?.architectureConcerns ?? [];
  const architecture = selectArchitectureContext({
    concerns,
    ...(input.architectureIndex === undefined ? {} : { index: input.architectureIndex }),
    maxEntries: EXECUTOR_CONTEXT_PACK_LIMITS.maxArchitectureEntries,
  });
  const findings = (input.findings ?? []).slice(0, EXECUTOR_CONTEXT_PACK_LIMITS.maxFindings);
  const plan = input.plan ?? null;
  const pack: ExecutorContextPack = {
    task: { id: input.contract.id, revision: input.contract.revision },
    ...(plan === null
      ? {}
      : {
          plan: {
            id: plan.id,
            revision: plan.revision,
            approval: input.planApproval ?? "none",
          },
        }),
    executionContract: { ...input.executionContract },
    ...(milestone === null ? {} : { milestone: { id: milestone.id, version: milestone.version } }),
    instructions: instructionRefs(
      input.instructions ?? null,
      EXECUTOR_CONTEXT_PACK_LIMITS.maxInstructions,
    ),
    architecture,
    verifiedTouchpoints: touchpointRefs(plan, "verified"),
    candidateTouchpoints: touchpointRefs(plan, "candidate"),
    capabilities: capabilityRef(input.capabilitySnapshot ?? null),
    unresolvedFindings: findings,
    acceptance: (milestone?.acceptance ?? [])
      .slice(0, EXECUTOR_CONTEXT_PACK_LIMITS.maxAcceptance)
      .map((requirement) => ({
        id: requirement.id,
        description: requirement.description,
        ...(requirement.criterionId === undefined ? {} : { criterionId: requirement.criterionId }),
      })),
  };
  return deepFreeze(pack);
}
