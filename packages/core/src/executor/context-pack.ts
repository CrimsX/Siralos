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
import type { DocumentationSelection } from "./documentation-context.js";
import { selectDocumentationContext } from "./documentation-context.js";
import type {
  ActiveWorkingSet,
  FileInclusionReason,
  SourceView,
  WorkspaceScope,
} from "./workspace-scope.js";
import type { NewFileRationale } from "./new-file-discipline.js";

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

/** Bounded workspace-scope references (derived; never file contents). */
export interface WorkspaceScopeRef {
  readonly verifiedFiles: readonly {
    readonly path: string;
    readonly revision?: string;
    readonly evidence?: string;
  }[];
  /** Candidate paths only — candidate contents never enter context. */
  readonly candidateFiles: readonly string[];
  readonly promotions: readonly { readonly path: string; readonly evidence: string }[];
}

/** Bounded current-step working-set references. */
export interface ActiveWorkingSetRef {
  readonly stepId: string;
  readonly files: readonly {
    readonly path: string;
    readonly reason: FileInclusionReason;
    readonly view: SourceView;
  }[];
}

/** Deterministic review signals (proliferation/scope expansion). */
export interface ScopeSignalRef {
  readonly id: string;
  readonly message: string;
}

/** Bounded new-production-file rationale references. */
export interface NewFileRef {
  readonly path: string;
  readonly reason: string;
  /** Existing owner modules inspected before creating the file. */
  readonly existingOwnersInspected: readonly string[];
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
  /** Derived task workspace scope (when provided). */
  readonly workspaceScope?: WorkspaceScopeRef;
  /** Current plan-step working set (when provided). */
  readonly activeWorkingSet?: ActiveWorkingSetRef;
  /** Deterministically selected documentation (root/nested/architecture/ADRs). */
  readonly documentation?: DocumentationSelection;
  /** Review signals: proliferation / unexplained expansion warnings. */
  readonly scopeSignals?: readonly ScopeSignalRef[];
  /** Recorded new-production-file rationales. */
  readonly newFiles?: readonly NewFileRef[];
}

/** Host-owned bounds for the context pack. */
export const EXECUTOR_CONTEXT_PACK_LIMITS = Object.freeze({
  maxInstructions: 8,
  maxInstructionSummaryBytes: 1024,
  maxArchitectureEntries: 4,
  maxFindings: 16,
  maxAcceptance: 32,
  maxWorkspaceVerifiedFiles: 12,
  maxWorkspaceCandidateFiles: 12,
  maxWorkingSetFiles: 8,
  maxScopeSignals: 8,
  maxNewFiles: 8,
  maxDocumentationEntries: 12,
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
  /** Derived task workspace scope (verified/candidate files, budgets). */
  readonly workspaceScope?: WorkspaceScope | null;
  /** Current plan-step working set. */
  readonly activeWorkingSet?: ActiveWorkingSet | null;
  /** Documentation index override (behavior fixtures inject doc trees). */
  readonly documentationIndex?: readonly import("./documentation-context.js").DocumentationEntry[];
  /** Paths used to scope nested AGENTS.md selection (defaults to verified touchpoints). */
  readonly documentationPaths?: readonly string[];
  /** Deterministic review signals (proliferation / scope expansion). */
  readonly scopeSignals?: readonly ScopeSignalRef[];
  /** Recorded new-production-file rationales. */
  readonly newFiles?: readonly NewFileRationale[];
  /** Restrict capability guidance to these areas (capability-aware selection). */
  readonly capabilityAreas?: readonly string[];
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

function capabilityRef(
  snapshot: CapabilitySnapshot | null,
  areas: readonly string[] | undefined,
): CapabilityRef {
  if (snapshot === null) {
    return { available: false, states: [] };
  }
  const allAreas = [
    { area: "providers", state: snapshot.providers[0]?.state ?? "unknown" },
    { area: "sandbox", state: snapshot.sandbox.state },
    { area: "workspace", state: snapshot.workspace.state },
    { area: "godot", state: snapshot.godot.state },
    { area: "references", state: snapshot.references.state },
    { area: "research", state: snapshot.research.state },
    { area: "tools", state: snapshot.tools.state },
  ];
  const states =
    areas === undefined ? allAreas : allAreas.filter((entry) => areas.includes(entry.area));
  return { available: true, states };
}

function workspaceScopeRef(scope: WorkspaceScope): WorkspaceScopeRef {
  return {
    verifiedFiles: scope.verifiedFiles
      .slice(0, EXECUTOR_CONTEXT_PACK_LIMITS.maxWorkspaceVerifiedFiles)
      .map((file) => ({
        path: file.path,
        ...(file.revision === undefined ? {} : { revision: file.revision }),
        ...(file.evidence === undefined ? {} : { evidence: file.evidence }),
      })),
    candidateFiles: scope.candidateFiles
      .slice(0, EXECUTOR_CONTEXT_PACK_LIMITS.maxWorkspaceCandidateFiles)
      .map((file) => file.path),
    promotions: scope.promotions.map((record) => ({
      path: record.path,
      evidence: record.evidence,
    })),
  };
}

function activeWorkingSetRef(set: ActiveWorkingSet): ActiveWorkingSetRef {
  return {
    stepId: set.stepId,
    files: set.files.slice(0, EXECUTOR_CONTEXT_PACK_LIMITS.maxWorkingSetFiles).map((file) => ({
      path: file.path,
      reason: file.reason,
      view: file.view,
    })),
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
  const documentation = selectDocumentationContext({
    concerns,
    paths:
      input.documentationPaths ??
      input.plan?.touchpoints
        .filter((touchpoint) => touchpoint.confidence === "verified")
        .map((touchpoint) => touchpoint.path) ??
      [],
    ...(input.documentationIndex === undefined ? {} : { index: input.documentationIndex }),
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
    capabilities: capabilityRef(input.capabilitySnapshot ?? null, input.capabilityAreas),
    ...(input.workspaceScope == null
      ? {}
      : { workspaceScope: workspaceScopeRef(input.workspaceScope) }),
    ...(input.activeWorkingSet == null
      ? {}
      : { activeWorkingSet: activeWorkingSetRef(input.activeWorkingSet) }),
    documentation: {
      rootAgents: documentation.rootAgents.slice(
        0,
        EXECUTOR_CONTEXT_PACK_LIMITS.maxDocumentationEntries,
      ),
      nestedAgents: documentation.nestedAgents.slice(
        0,
        EXECUTOR_CONTEXT_PACK_LIMITS.maxDocumentationEntries,
      ),
      architectureDocs: documentation.architectureDocs.slice(
        0,
        EXECUTOR_CONTEXT_PACK_LIMITS.maxDocumentationEntries,
      ),
      adrs: documentation.adrs.slice(0, EXECUTOR_CONTEXT_PACK_LIMITS.maxDocumentationEntries),
      developmentDocs: documentation.developmentDocs.slice(
        0,
        EXECUTOR_CONTEXT_PACK_LIMITS.maxDocumentationEntries,
      ),
      dropped: documentation.dropped.slice(0, EXECUTOR_CONTEXT_PACK_LIMITS.maxDocumentationEntries),
    },
    ...(input.scopeSignals === undefined
      ? {}
      : {
          scopeSignals: input.scopeSignals
            .slice(0, EXECUTOR_CONTEXT_PACK_LIMITS.maxScopeSignals)
            .map((signal) => ({ id: signal.id, message: signal.message })),
        }),
    ...(input.newFiles === undefined
      ? {}
      : {
          newFiles: input.newFiles
            .slice(0, EXECUTOR_CONTEXT_PACK_LIMITS.maxNewFiles)
            .map((file) => ({
              path: file.path,
              reason: file.reason,
              existingOwnersInspected: [...file.existingOwnersInspected],
            })),
        }),
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
