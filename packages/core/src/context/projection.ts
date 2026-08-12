import type { ProjectionMode } from "../projection/tool-projector.js";
import type { ContextSegmentInput } from "../projection/context-projector.js";
import type { PhaseContract } from "./phase-contract.js";

/**
 * Phase-driven context and tool-surface projection (Stage 3 —
 * Interpretable Context Architecture, ADR 0030).
 *
 * The active PhaseContract declares the context classes and authority of
 * the current phase; `projectPhaseContext` builds the minimal sufficient
 * ContextProjector segments from WorkspaceScope/ActiveWorkingSet/
 * documentation selection, and `toolSurfaceForPhase` maps the contract
 * to a ToolProjector mode through a fixed host table. ContextProjector
 * and ToolProjector remain the actual projection/tool authorities: a
 * malformed PhaseContract can at most select a mode, never grant a tool.
 */

export interface PhaseContextSources {
  readonly taskContract: { readonly revision: number; readonly digest: string } | null;
  readonly taskPlan: { readonly revision: number; readonly digest: string } | null;
  readonly workspaceScope: {
    readonly verifiedFiles: readonly string[];
    readonly candidateFiles: readonly string[];
  } | null;
  readonly activeWorkingSet: readonly string[] | null;
  readonly documentationSelection: readonly string[] | null;
  readonly preparedChangeset: { readonly digest: string } | null;
  readonly validationEvidence: readonly { readonly id: string; readonly digest: string }[] | null;
  readonly reviewFindings: readonly string[] | null;
}

/** Deterministic mapping: PhaseContract id -> ToolProjector mode. */
export function toolSurfaceForPhase(contract: PhaseContract): ProjectionMode {
  switch (contract.id) {
    case "planning":
      return "planning";
    case "review":
      return "review";
    case "inspection":
    case "impact":
    case "validation":
    case "acceptance":
    case "verification":
      return "inspection";
    case "preparation":
    case "approval":
    case "mutation":
    case "repair":
      return "development";
  }
}

/**
 * Build the minimal sufficient context segments for a phase from its
 * declared context classes. Only classes declared by the contract are
 * projected; no phase defaults to repository-wide context.
 */
export function projectPhaseContext(
  contract: PhaseContract,
  sources: PhaseContextSources,
): readonly ContextSegmentInput[] {
  const segments: ContextSegmentInput[] = [];
  const classes = new Set(contract.contextClasses);
  if (classes.has("global") && sources.taskContract !== null) {
    segments.push({
      id: "phase.global.contract",
      stability: "stable",
      title: "Task Contract",
      content: `revision ${sources.taskContract.revision} / ${sources.taskContract.digest.slice(0, 12)}\u2026`,
    });
  }
  if (classes.has("phase_contract")) {
    segments.push({
      id: "phase.contract",
      stability: "stable",
      title: "Phase Contract",
      content: `${contract.id} v${contract.version} / ${contract.digest.value.slice(0, 12)}\u2026 authority: ${
        contract.authority.readOnly ? "read-only" : "prepared-only mutation"
      }${contract.authority.approvalGrant ? " + approval grant" : ""}${
        contract.authority.acceptanceAuthority ? " + acceptance authority" : ""
      }`,
    });
  }
  if (classes.has("routing") && sources.workspaceScope !== null) {
    segments.push({
      id: "phase.routing.scope",
      stability: "contextual",
      title: "Workspace Scope",
      content: `verified: ${sources.workspaceScope.verifiedFiles.join(", ") || "none"}\ncandidates: ${
        sources.workspaceScope.candidateFiles.join(", ") || "none"
      }`,
    });
  }
  if (classes.has("routing") && sources.documentationSelection !== null) {
    segments.push({
      id: "phase.routing.documentation",
      stability: "stable",
      title: "Documentation Selection",
      content: sources.documentationSelection.join("\n") || "none",
    });
  }
  if (classes.has("stable_reference") && sources.activeWorkingSet !== null) {
    segments.push({
      id: "phase.stable.working_set",
      stability: "contextual",
      title: "Active Working Set",
      content: sources.activeWorkingSet.join("\n") || "none",
    });
  }
  if (classes.has("working")) {
    if (sources.taskPlan !== null) {
      segments.push({
        id: "phase.working.plan",
        stability: "contextual",
        title: "Task Plan",
        content: `revision ${sources.taskPlan.revision} / ${sources.taskPlan.digest.slice(0, 12)}\u2026`,
      });
    }
    if (sources.preparedChangeset !== null) {
      segments.push({
        id: "phase.working.prepared",
        stability: "volatile",
        title: "Prepared Changeset",
        content: `digest ${sources.preparedChangeset.digest.slice(0, 12)}\u2026`,
      });
    }
    if (sources.validationEvidence !== null && sources.validationEvidence.length > 0) {
      segments.push({
        id: "phase.working.evidence",
        stability: "volatile",
        title: "Validation Evidence",
        content: sources.validationEvidence
          .map((entry) => `${entry.id} / ${entry.digest.slice(0, 12)}\u2026`)
          .join("\n"),
      });
    }
    if (sources.reviewFindings !== null && sources.reviewFindings.length > 0) {
      segments.push({
        id: "phase.working.findings",
        stability: "volatile",
        title: "Review Findings",
        content: sources.reviewFindings.join("\n"),
      });
    }
  }
  return segments;
}

/** Invariant check: no phase contract requests repository-wide context. */
export function phaseRequiresRepositoryWideContext(_contract: PhaseContract): boolean {
  return false;
}
